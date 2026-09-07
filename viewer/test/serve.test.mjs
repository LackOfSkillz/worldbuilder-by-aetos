// Node-native tests for scripts/serve.mjs -- the dev server's CACHING behaviour, and
// nothing else about it.
//
// # Why this file exists
//
// One cold load of the owner's world is 158 requests and 11.79 MB, of which 4.08 MB is the
// same bytes fetched nine times: nine engine instances (the main thread plus eight pool
// workers) each fetch the wasm and the whole `engine.js` module graph, and
// `cache-control: no-store` on every response meant none of it could be reused.
//
// The fix has to hold two things at once, and only one of them is about speed:
//
//   1. repeat fetches must not re-transfer the body;
//   2. **an edit must still appear on the next reload** -- this is a dev server with no
//      build step, and `no-store` was there for that. A `max-age` would buy (1) by giving
//      up (2), which is a bad trade and is not the one made.
//
// Both are asserted here against a real listening server and real `fetch` calls, because
// "the header string looks right" is exactly the shape of assertion this project has been
// caught by: the `If-None-Match` comparison below is written against a header that arrives
// as a comma-separated list, and an equality test on it would silently never match, never
// send a 304, and pass any test that only read `res.headers`.
//
// Host/population: node v22 on this repository's own `scripts/serve.mjs`, one server
// instance per test on an ephemeral port (`listen(0)`), fetching real files out of
// `viewer/public` plus one temporary file this suite creates and edits.

import test from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const SERVE = join(HERE, "..", "scripts", "serve.mjs");
const PUBLIC = join(HERE, "..", "public");

/// Start `serve.mjs` on `port` and wait for its own "viewer: http://..." line, which it
/// prints from inside the `listen` callback. Waiting on a fixed sleep instead is a bet, and
/// a lost bet here reads as a connection-refused failure in whichever test ran first.
async function startServer(port) {
  const child = spawn(process.execPath, [SERVE], {
    env: { ...process.env, PORT: String(port) }, stdio: ["ignore", "pipe", "pipe"],
  });
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("serve.mjs never listened")), 10000);
    child.stdout.on("data", (chunk) => {
      if (String(chunk).includes("viewer:")) { clearTimeout(timer); resolve(); }
    });
    child.on("error", reject);
  });
  return child;
}

const PORT = 8231 + (process.pid % 200);
let server = null;
const base = () => `http://127.0.0.1:${PORT}`;

test.before(async () => { server = await startServer(PORT); });
test.after(() => { if (server) server.kill(); });

test("a static asset carries a validator, and a conditional request gets a bodyless 304", async () => {
  // The whole of the 4.08 MB. `no-cache` is NOT "do not cache" -- it is "store it and
  // revalidate before reuse" -- so the browser keeps the bytes and this exchange is what
  // replaces the eight repeat downloads.
  const first = await fetch(`${base()}/app/pool.js`);
  assert.equal(first.status, 200);
  assert.equal(first.headers.get("cache-control"), "no-cache");
  const etag = first.headers.get("etag");
  assert.ok(etag, "no ETag means no validator means every repeat fetch is a full transfer");
  const body = await first.text();
  assert.ok(body.length > 0);

  const second = await fetch(`${base()}/app/pool.js`, { headers: { "if-none-match": etag } });
  assert.equal(second.status, 304, "the conditional request must be answered 304, not 200");
  assert.equal((await second.text()).length, 0, "a 304 must carry no body -- that is the saving");
  assert.equal(second.headers.get("etag"), etag);
  assert.equal(
    second.headers.get("content-security-policy"), first.headers.get("content-security-policy"),
    "a 304 refreshes the stored response's headers, so the CSP must go out with it",
  );
});

test("the ETag is compared against a LIST, the way browsers send it", async () => {
  // `If-None-Match` is a comma-separated list by specification and Chrome sends the token
  // this server issued. An equality test would work in a hand-written curl and silently
  // never match in the browser -- the saving would simply not exist, and no response header
  // would say so.
  const etag = (await fetch(`${base()}/app/engine.js`)).headers.get("etag");
  const listed = await fetch(`${base()}/app/engine.js`, {
    headers: { "if-none-match": `W/"deadbeef-1", ${etag}` },
  });
  assert.equal(listed.status, 304);
  const wrong = await fetch(`${base()}/app/engine.js`, {
    headers: { "if-none-match": 'W/"deadbeef-1"' },
  });
  assert.equal(wrong.status, 200, "a validator that does not match must re-send the body");
  assert.ok((await wrong.text()).length > 0);
});

test("AN EDIT STILL APPEARS: changed bytes rotate the validator and the 304 stops", async () => {
  // The half of this change that is not about speed. `no-store` guaranteed this; a
  // `max-age` would have traded it away. Written as a real edit to a real file served by a
  // real server rather than as a claim about what `no-cache` means.
  const dir = mkdtempSync(join(tmpdir(), "wb-serve-"));
  const served = join(PUBLIC, "app", "__serve-edit-probe.js");
  try {
    writeFileSync(served, "export const value = 1;\n", { encoding: "utf8" });
    const before = await fetch(`${base()}/app/__serve-edit-probe.js`);
    const etag = before.headers.get("etag");
    assert.equal((await before.text()).trim(), "export const value = 1;");

    // Same length, different bytes, and a later mtime -- the case a size-only validator
    // would miss.
    await new Promise((r) => setTimeout(r, 20));
    writeFileSync(served, "export const value = 2;\n", { encoding: "utf8" });

    const after = await fetch(`${base()}/app/__serve-edit-probe.js`, {
      headers: { "if-none-match": etag },
    });
    assert.equal(after.status, 200, "the edit must be re-sent, not answered 304 from cache");
    assert.equal((await after.text()).trim(), "export const value = 2;");
    assert.notEqual(after.headers.get("etag"), etag, "the validator must have rotated");
  } finally {
    rmSync(served, { force: true });
    rmSync(dir, { recursive: true, force: true });
  }
});

test("the DOCUMENT keeps no-store and gets no validator at all", async () => {
  // The one response where a 304 buys nothing -- it is fetched once per load either way --
  // and where a caching mistake is precisely the one that reads as "my edit did not
  // appear". Both the bare `/` and the explicit filename, because `/` is rewritten to
  // `index.html` before the decision is taken and a test on only one of them would not see
  // a policy that fell through on the other.
  for (const path of ["/", "/index.html"]) {
    const res = await fetch(`${base()}${path}`);
    assert.equal(res.status, 200, path);
    assert.equal(res.headers.get("cache-control"), "no-store", path);
    assert.equal(res.headers.get("etag"), null, path);
    await res.text();
  }
});
