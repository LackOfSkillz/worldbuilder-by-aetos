// The AI curator's settings, as the studio's server keeps them.
//
// **The browser never talks to the model.** The page asks this server, and this server asks
// the backend - so the page's `connect-src 'self'` guarantee holds, and a key typed into the
// settings is never handed back to the page once it is saved. It lives in
// `viewer/curator.local.json`, which git ignores.
//
// **A backend is a list of addresses, tried in order.** The GX10 cluster answers on the
// house LAN at home and on its Tailscale address away from it; one saved address would be
// wrong half the time. Whichever answers first is the one used, and the curator falls over
// to the next if the one it is using stops answering mid-run.

import { mkdir, readFile, writeFile } from "node:fs/promises";
import { spawn } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export const SETTINGS_FILE = join(dirname(fileURLToPath(import.meta.url)), "..",
                                  "curator.local.json");

export const BACKENDS = ["cluster", "local", "api"];

export const DEFAULTS = Object.freeze({
  enabled: false,
  backend: "cluster",
  // Home LAN first: it is the shorter path when it answers, and Tailscale is the one that
  // answers everywhere else.
  base_urls: ["http://192.168.1.200:8888/v1", "http://100.92.130.112:8888/v1"],
  model: "deepseek-v4-flash-0731",
  at_once: 6,
  key: "",
});

/// The saved settings, over the defaults. A missing or unreadable file is the defaults.
export async function loadSettings(file = SETTINGS_FILE) {
  try {
    const saved = JSON.parse(await readFile(file, "utf8"));
    return { ...DEFAULTS, ...saved,
             base_urls: Array.isArray(saved.base_urls) && saved.base_urls.length
               ? saved.base_urls : [...DEFAULTS.base_urls] };
  } catch {
    return { ...DEFAULTS, base_urls: [...DEFAULTS.base_urls] };
  }
}

/// What the page may see: everything but the key, which is reduced to whether one is set.
export function publicView(settings) {
  const { key, ...rest } = settings;
  const k = String(key || "");
  return { ...rest, key_set: k.length > 0, key_hint: k.length > 4 ? `…${k.slice(-4)}` : "" };
}

/// Merge what the page sent into the saved settings, refusing anything malformed.
///
/// **The key is write-only.** An empty or absent `key` keeps the saved one - the page never
/// has it to send back - and only `clear_key: true` removes it.
export function mergeSettings(current, incoming) {
  const next = { ...current };
  if ("enabled" in incoming) next.enabled = Boolean(incoming.enabled);
  if ("backend" in incoming) {
    if (!BACKENDS.includes(incoming.backend)) throw new Error(`unknown backend ${incoming.backend}`);
    next.backend = incoming.backend;
  }
  if ("base_urls" in incoming) {
    const urls = (Array.isArray(incoming.base_urls) ? incoming.base_urls
                  : String(incoming.base_urls || "").split(/[\s,]+/))
      .map((u) => String(u).trim().replace(/\/+$/, "")).filter(Boolean);
    if (!urls.length) throw new Error("at least one address is required");
    if (urls.length > 5) throw new Error("five addresses at most");
    for (const u of urls) {
      if (!/^https?:\/\/[^\s/]+/.test(u)) throw new Error(`not an http address: ${u}`);
    }
    next.base_urls = urls;
  }
  if ("model" in incoming) {
    const model = String(incoming.model || "").trim();
    if (model.length > 200) throw new Error("model name too long");
    next.model = model;
  }
  if ("at_once" in incoming) {
    const n = Number(incoming.at_once);
    if (!Number.isInteger(n) || n < 1 || n > 32) throw new Error("rooms at once must be 1 to 32");
    next.at_once = n;
  }
  if (incoming.clear_key) next.key = "";
  else if (typeof incoming.key === "string" && incoming.key.trim()) next.key = incoming.key.trim();
  return next;
}

export async function saveSettings(settings, file = SETTINGS_FILE) {
  await writeFile(file, `${JSON.stringify(settings, null, 2)}\n`, "utf8");
}

/// Ask each address in turn for its model list; the first that answers wins.
///
/// Returns `{ok, url, models, ms, tried}` - `tried` says what every address did, so a
/// failure reads "LAN: timed out, Tailscale: refused" rather than just "no".
export async function probe(settings, { timeoutMs = 4000, fetchImpl = fetch } = {}) {
  const tried = [];
  for (const base of settings.base_urls || []) {
    const started = Date.now();
    try {
      const response = await fetchImpl(`${base}/models`, {
        headers: settings.key ? { authorization: `Bearer ${settings.key}` } : {},
        signal: AbortSignal.timeout(timeoutMs),
      });
      if (!response.ok) {
        tried.push({ url: base, error: `HTTP ${response.status}` });
        continue;
      }
      const body = await response.json();
      const models = (body.data || []).map((m) => m.id).filter(Boolean);
      const ms = Date.now() - started;
      tried.push({ url: base, ok: true, ms });
      return { ok: true, url: base, models, ms, tried };
    } catch (error) {
      const why = error && error.name === "TimeoutError" ? "timed out"
        : String((error && error.cause && error.cause.code) || (error && error.message) || error);
      tried.push({ url: base, error: why });
    }
  }
  return { ok: false, tried };
}

function readBody(req, limit = 100_000) {
  return new Promise((resolve, reject) => {
    let body = "";
    req.on("data", (chunk) => {
      body += chunk;
      if (body.length > limit) { reject(new Error("body too large")); req.destroy(); }
    });
    req.on("end", () => resolve(body));
    req.on("error", reject);
  });
}

function send(res, status, value) {
  res.writeHead(status, { "content-type": "application/json", "cache-control": "no-store" })
     .end(JSON.stringify(value));
}

// ---------------------------------------------------------------------------------------
// Curation jobs.
//
// **The curator's own `status.json` is the truth about progress; this only adds what a file
// cannot know** - whether the process is alive, and whether somebody paused it. So the
// server holds nothing it could lose: after a restart the files still say how far a run got,
// and a run whose process is gone but whose status says "running" is reported as
// interrupted, which is exactly what it is.

const jobs = new Map();

const RUN_ID = /^[A-Za-z0-9_.-]+$/;

function curateDir(runsDir, runId) {
  if (!RUN_ID.test(runId)) throw new Error("bad run id");
  const dir = join(runsDir, runId, "curate");
  if (!dir.startsWith(runsDir)) throw new Error("path escape");
  return dir;
}

async function readJson(file) {
  try {
    return JSON.parse(await readFile(file, "utf8"));
  } catch {
    return null;
  }
}

/// Where a run's curation stands: the curator's numbers, and one word for its state.
///
/// `state` is one of: none, starting, running, paused, stopped, interrupted, done, failed.
export async function curationStatus(runsDir, runId) {
  const dir = curateDir(runsDir, runId);
  const status = await readJson(join(dir, "status.json"));
  const control = await readJson(join(dir, "control.json"));
  const job = jobs.get(runId);
  let state;
  if (job && job.alive) state = job.state === "starting" ? "starting" : "running";
  else if (control && (control.state === "paused" || control.state === "stopped")
           && !(status && status.state === "done")) state = control.state;
  else if (job && job.error) state = "failed";
  else if (!status) state = "none";
  else if (status.state === "running" || status.state === "starting") state = "interrupted";
  else state = status.state;
  // The curator's numbers first and the computed state after: spread the other way round,
  // the curator's own "running" overwrote "paused" and a paused run looked alive.
  return { ...(status || {}), run: runId, state, state_detail: status && status.state,
           error: job && job.error ? job.error : undefined,
           control: control ? control.state : undefined };
}

/// Start (or resume) curating a run. A run already being curated is left alone.
export async function startCuration(runId, { runsDir, root, python, settingsFile = SETTINGS_FILE }) {
  const dir = curateDir(runsDir, runId);
  const running = jobs.get(runId);
  if (running && running.alive) return { started: false, reason: "already running" };
  const settings = await loadSettings(settingsFile);
  await mkdir(dir, { recursive: true });
  await writeFile(join(dir, "control.json"), JSON.stringify({ state: "running" }), "utf8");
  const args = [
    "-m", "evennia_roundtrip.curate",
    "--run", join(runsDir, runId),
    "--base-url", settings.base_urls.join(","),
    "--model", settings.model,
    "--at-once", String(settings.at_once),
    "--quiet",
  ];
  // **The key goes in the environment, never the command line**, where any process listing
  // on the machine would show it.
  const env = { ...process.env };
  if (settings.key) env.WB_CURATOR_KEY = settings.key;
  // **Stdout is thrown away, not piped and ignored.** The curator can print a line a room,
  // and a pipe nobody reads fills after a few hundred rooms and blocks the writer - which
  // would look exactly like a hung model. Progress comes from status.json instead.
  const child = spawn(python(), args, { cwd: root, env, windowsHide: true,
                                        stdio: ["ignore", "ignore", "pipe"] });
  const job = { child, alive: true, state: "starting", error: null, stderr: "" };
  jobs.set(runId, job);
  child.stderr.on("data", (chunk) => { job.stderr = (job.stderr + chunk).slice(-4000); });
  child.on("spawn", () => { job.state = "running"; });
  child.on("error", (error) => { job.alive = false; job.error = String(error.message); });
  child.on("close", (code) => {
    job.alive = false;
    // A pause or stop kills the process on purpose; that is not a failure. Code 2 is the
    // curator saying no address answered, which its status file already explains.
    if (!job.killed && code !== 0 && code !== 2) {
      job.error = `the curator exited ${code}: ${job.stderr.slice(-300)}`;
    }
    console.log(`curation of ${runId} ended (${job.killed ? job.state : `exit ${code}`})`);
  });
  console.log(`curation of ${runId} started (${settings.model} at ${settings.base_urls.join(" / ")})`);
  return { started: true };
}

/// Pause or stop a run's curation. Both kill the process - every finished room is already
/// in the journal - and differ only in what the studio offers next.
export async function haltCuration(runId, how, { runsDir }) {
  const dir = curateDir(runsDir, runId);
  await mkdir(dir, { recursive: true });
  await writeFile(join(dir, "control.json"), JSON.stringify({ state: how }), "utf8");
  const job = jobs.get(runId);
  if (job && job.alive) {
    job.killed = true;
    job.state = how;
    job.child.kill();
  }
  return { halted: how };
}

/// Curate a freshly generated run if the settings say to. Called when the generator exits.
export async function curateIfWanted(runId, context) {
  const settings = await loadSettings(context.settingsFile || SETTINGS_FILE);
  if (!settings.enabled) return false;
  await startCuration(runId, context);
  return true;
}

/// Handle `/curator/...` and `/curate/...`. Returns true when the request was one of these.
export async function handleCurator(req, res, pathname, context = {}) {
  const { file = SETTINGS_FILE } = context;
  if (pathname === "/curate/" || pathname === "/curate") {
    try {
      const runId = String(context.url.searchParams.get("run") || "");
      if (req.method === "GET") {
        send(res, 200, await curationStatus(context.runsDir, runId));
        return true;
      }
      if (req.method === "POST") {
        const action = String(context.url.searchParams.get("action") || "");
        if (action === "start" || action === "resume") {
          await startCuration(runId, { ...context, settingsFile: file });
        } else if (action === "pause" || action === "stop") {
          await haltCuration(runId, action === "pause" ? "paused" : "stopped", context);
        } else {
          throw new Error(`unknown action ${action}`);
        }
        send(res, 200, await curationStatus(context.runsDir, runId));
        return true;
      }
      send(res, 405, { error: `${req.method} not allowed on /curate/` });
    } catch (error) {
      send(res, 400, { error: String(error.message || error) });
    }
    return true;
  }
  if (pathname !== "/curator/settings" && pathname !== "/curator/test") return false;
  try {
    if (pathname === "/curator/settings" && req.method === "GET") {
      send(res, 200, publicView(await loadSettings(file)));
      return true;
    }
    if (pathname === "/curator/settings" && req.method === "POST") {
      const incoming = JSON.parse((await readBody(req)) || "{}");
      const next = mergeSettings(await loadSettings(file), incoming);
      await saveSettings(next, file);
      console.log(`200 POST /curator/settings (${next.backend}, ${next.base_urls.length} `
        + `address${next.base_urls.length === 1 ? "" : "es"}, curate ${next.enabled ? "on" : "off"})`);
      send(res, 200, publicView(next));
      return true;
    }
    if (pathname === "/curator/test" && req.method === "POST") {
      // Test what the page is showing, not only what was last saved: unsaved edits are
      // exactly what somebody wants to try before committing to them.
      const incoming = JSON.parse((await readBody(req)) || "{}");
      const trial = mergeSettings(await loadSettings(file), incoming);
      const result = await probe(trial);
      if (result.ok && trial.model && !result.models.includes(trial.model)) {
        result.warning = `the backend answered but does not serve "${trial.model}"`;
      }
      send(res, 200, result);
      return true;
    }
    send(res, 405, { error: `${req.method} not allowed on ${pathname}` });
    return true;
  } catch (error) {
    send(res, 400, { error: String(error.message || error) });
    return true;
  }
}
