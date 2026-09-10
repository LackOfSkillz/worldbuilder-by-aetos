// The curator's settings: a key that never comes back out, and a backend reached by
// whichever of its addresses is answering today.

import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { DEFAULTS, curationStatus, loadSettings, mergeSettings, publicView, probe, saveSettings }
  from "../scripts/curator.mjs";

test("a missing settings file is the defaults, LAN first and Tailscale second", async () => {
  const settings = await loadSettings(join(tmpdir(), "no-such-curator.json"));
  assert.deepEqual(settings.base_urls, DEFAULTS.base_urls);
  assert.equal(settings.base_urls[0], "http://192.168.1.200:8888/v1");
  assert.equal(settings.enabled, false);
});

test("the key is never shown to the page, only whether one is set", () => {
  const view = publicView({ ...DEFAULTS, key: "sk-secret-abcd" });
  assert.equal(view.key, undefined);
  assert.equal(view.key_set, true);
  assert.equal(view.key_hint, "…abcd");
  assert.ok(!JSON.stringify(view).includes("secret"));
});

test("the key is write-only: an empty one keeps the saved key, clear_key removes it", () => {
  const saved = { ...DEFAULTS, key: "sk-kept" };
  assert.equal(mergeSettings(saved, { key: "" }).key, "sk-kept");
  assert.equal(mergeSettings(saved, { model: "x" }).key, "sk-kept");
  assert.equal(mergeSettings(saved, { key: "sk-new" }).key, "sk-new");
  assert.equal(mergeSettings(saved, { clear_key: true }).key, "");
});

test("addresses may be typed as one line, and trailing slashes are dropped", () => {
  const next = mergeSettings(DEFAULTS, { base_urls: "http://a:1/v1/, http://b:2/v1" });
  assert.deepEqual(next.base_urls, ["http://a:1/v1", "http://b:2/v1"]);
});

test("malformed settings are refused rather than saved", () => {
  assert.throws(() => mergeSettings(DEFAULTS, { base_urls: [] }), /at least one/);
  assert.throws(() => mergeSettings(DEFAULTS, { base_urls: ["ftp://x"] }), /not an http/);
  assert.throws(() => mergeSettings(DEFAULTS, { at_once: 0 }), /1 to 32/);
  assert.throws(() => mergeSettings(DEFAULTS, { backend: "magic" }), /unknown backend/);
});

test("settings survive a save and a load", async () => {
  const dir = await mkdtemp(join(tmpdir(), "curator-"));
  const file = join(dir, "curator.local.json");
  await saveSettings(mergeSettings(DEFAULTS, { enabled: true, key: "sk-1", at_once: 4 }), file);
  const back = await loadSettings(file);
  assert.equal(back.enabled, true);
  assert.equal(back.at_once, 4);
  assert.equal(back.key, "sk-1");
  assert.ok((await readFile(file, "utf8")).includes("sk-1"), "the key is on disk, locally");
});

function aBackend(models) {
  const server = createServer((req, res) => {
    res.writeHead(200, { "content-type": "application/json" })
       .end(JSON.stringify({ data: models.map((id) => ({ id })) }));
  });
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server)));
}

function aClosedPort() {
  const server = createServer();
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => {
    const { port } = server.address();
    server.close(() => resolve(port));
  }));
}

test("away from home: the LAN address refuses and the Tailscale address answers", async () => {
  const closed = await aClosedPort();
  const backend = await aBackend(["deepseek-v4-flash-0731"]);
  try {
    const { port } = backend.address();
    const result = await probe({ ...DEFAULTS, base_urls: [
      `http://127.0.0.1:${closed}/v1`, `http://127.0.0.1:${port}/v1`] }, { timeoutMs: 2000 });
    assert.equal(result.ok, true);
    assert.equal(result.url, `http://127.0.0.1:${port}/v1`);
    assert.deepEqual(result.models, ["deepseek-v4-flash-0731"]);
    assert.equal(result.tried.length, 2);
    assert.ok(result.tried[0].error, "the refusal is reported, not hidden");
  } finally {
    backend.close();
  }
});

test("nothing answering is a clear no, with what each address did", async () => {
  const one = await aClosedPort();
  const two = await aClosedPort();
  const result = await probe({ ...DEFAULTS, base_urls: [
    `http://127.0.0.1:${one}/v1`, `http://127.0.0.1:${two}/v1`] }, { timeoutMs: 2000 });
  assert.equal(result.ok, false);
  assert.equal(result.tried.length, 2);
  assert.ok(result.tried.every((t) => t.error));
});

async function aRun(status, control) {
  const runsDir = await mkdtemp(join(tmpdir(), "runs-"));
  const dir = join(runsDir, "r1", "curate");
  await mkdir(dir, { recursive: true });
  if (status) await writeFile(join(dir, "status.json"), JSON.stringify(status));
  if (control) await writeFile(join(dir, "control.json"), JSON.stringify({ state: control }));
  return runsDir;
}

test("a paused run says paused, whatever the curator last wrote", async () => {
  // The curator's status.json still says "running" after it is killed on pause - it had no
  // chance to write anything else. The answer is what the person asked for.
  const runsDir = await aRun({ state: "running", done: 186, total: 322 }, "paused");
  const answer = await curationStatus(runsDir, "r1");
  assert.equal(answer.state, "paused");
  assert.equal(answer.done, 186, "the numbers still come through");
});

test("a run whose curator vanished without being paused is interrupted", async () => {
  const runsDir = await aRun({ state: "running", done: 40, total: 100 }, "running");
  assert.equal((await curationStatus(runsDir, "r1")).state, "interrupted");
});

test("a finished run is done even if it was once paused", async () => {
  const runsDir = await aRun({ state: "done", done: 100, total: 100 }, "paused");
  assert.equal((await curationStatus(runsDir, "r1")).state, "done");
});

test("a run never curated is none, and a malformed run id is refused", async () => {
  const runsDir = await aRun(null, null);
  assert.equal((await curationStatus(runsDir, "r1")).state, "none");
  await assert.rejects(() => curationStatus(runsDir, "../escape"), /bad run id/);
});
