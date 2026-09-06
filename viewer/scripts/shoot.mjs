// A dependency-free headless-Chromium driver for photographing (and digesting) the viewer.
//
// # Why this file is committed
//
// It has now been built three times. Task 1 of the photoreal slice wrote it, Task 6 rebuilt it
// from Task 1's recipe, and Task 6's own report says so as concern 7: *"The screenshot harness is
// not committed. It lives in the task scratchpad... a third task will rebuild it again. Somewhere
// under `viewer/scripts/` would end that."* This is that.
//
// # Why it is not Playwright
//
// The `viewer` package has exactly one dependency (`cesium`, for the vendored build and for the
// real classes the node tests assert against) and adding a browser automation framework to
// photograph a page would be a large dependency for a small job. Node 22 ships a `WebSocket`
// client, and the Chrome DevTools Protocol over that socket is enough: navigate, evaluate,
// capture. Roughly two hundred lines against a hundred megabytes.
//
// # Two jobs, deliberately in one file
//
// `shoot` writes a PNG. `digest` writes a SHA-256 of a PNG captured under a **pinned viewport,
// camera and frame time** -- which is how this project proves an "unchanged" claim. They are one
// file because they must agree about what settling means; two harnesses that settled differently
// would produce a digest of a picture nobody photographed.
//
// **The frame-time trap, recorded and avoided.** `Scene.render()` with no argument defaults its
// frame time to `JulianDate.now()`, so pinning `viewer.clock` alone does nothing and a digest run
// twice on the same build comes back different. An earlier task fell into exactly that and its
// first control failed. `digest` stops the render loop (`useDefaultRenderLoop = false`) and drives
// frames by hand with an explicit `JulianDate`.
//
// # Usage
//
//   node scripts/shoot.mjs shoot   --url "<query string>" --out shot.png [--width 1600] [--height 900]
//   node scripts/shoot.mjs digest  --url "<query string>" [--out shot.png]
//   node scripts/shoot.mjs measure --url "<query string>"
//
// `measure` adds the third job: time-to-settle, the quadtree's depth, the worker pool's own
// per-consumer statistics, and the browser's own long-task count from a PerformanceObserver
// installed before the first module runs. That is what an A/B of pool consumers is quoted from.
//
// `--url` is appended to `http://127.0.0.1:<port>/`. The server is started by this script (the
// same `serve.mjs` the viewer uses) on `--port` (default 8137); pass `--attach` when one is
// already listening there.
//
// The chromium binary is found by `--chrome`, then `$WB_CHROME`, then a short list of usual
// places. It is never downloaded.

import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));

// ------------------------------------------------------------------------------------------
// Arguments
// ------------------------------------------------------------------------------------------

function parseArgs(argv) {
  const out = { _: [] };
  for (let i = 0; i < argv.length; i += 1) {
    const token = argv[i];
    if (token.startsWith("--")) {
      const key = token.slice(2);
      const next = argv[i + 1];
      if (next === undefined || next.startsWith("--")) out[key] = true;
      else { out[key] = next; i += 1; }
    } else out._.push(token);
  }
  return out;
}

const args = parseArgs(process.argv.slice(2));
const command = args._[0] || "shoot";
const WIDTH = Number(args.width || 1600);
const HEIGHT = Number(args.height || 900);
/// How long the tile-load queue must stay empty before the picture is called settled. Four
/// seconds is what Tasks 1 and 6 used and what their figures were taken at; it is stated here so
/// a later run can be compared with those rather than merely look similar.
const SETTLE_MS = Number(args.settle || 4000);
/// The frame time every digest is taken at. A constant, because the whole point is that two runs
/// of the same build produce the same bytes.
const FRAME_TIME_ISO = String(args.time || "2026-09-05T12:00:00Z");
const FRAMES = Number(args.frames || 20);

const CHROME_CANDIDATES = [
  args.chrome,
  process.env.WB_CHROME,
  "C:/Users/" + (process.env.USERNAME || "") + "/AppData/Local/ms-playwright/chromium-1243/chrome-win64/chrome.exe",
  "C:/Program Files/Google/Chrome/Application/chrome.exe",
  "C:/Program Files (x86)/Google/Chrome/Application/chrome.exe",
  "/usr/bin/chromium",
  "/usr/bin/google-chrome",
].filter(Boolean);

function findChrome() {
  for (const candidate of CHROME_CANDIDATES) if (existsSync(candidate)) return candidate;
  throw new Error(
    `no chromium found. Pass --chrome <path> or set WB_CHROME. Looked in:\n  ${CHROME_CANDIDATES.join("\n  ")}`,
  );
}

// ------------------------------------------------------------------------------------------
// The CDP client -- a session id, a message counter, and a promise per command
// ------------------------------------------------------------------------------------------

class Cdp {
  constructor(socket) {
    this.socket = socket;
    this.next = 1;
    this.pending = new Map();
    this.sessionId = null;
    socket.addEventListener("message", (event) => {
      const message = JSON.parse(event.data);
      const entry = this.pending.get(message.id);
      if (!entry) return;
      this.pending.delete(message.id);
      if (message.error) entry.reject(new Error(`${entry.method}: ${message.error.message}`));
      else entry.resolve(message.result);
    });
  }

  send(method, params = {}) {
    const id = this.next;
    this.next += 1;
    const payload = { id, method, params };
    if (this.sessionId) payload.sessionId = this.sessionId;
    this.socket.send(JSON.stringify(payload));
    return new Promise((res, rej) => this.pending.set(id, { resolve: res, reject: rej, method }));
  }

  /// Evaluate an expression in the page and return its value. `awaitPromise` is on, so an
  /// expression may be an async IIFE -- which is how the settle loop below is written.
  async evaluate(expression) {
    const result = await this.send("Runtime.evaluate", {
      expression, awaitPromise: true, returnByValue: true,
    });
    if (result.exceptionDetails) {
      throw new Error(`page threw: ${JSON.stringify(result.exceptionDetails.exception?.description ?? result.exceptionDetails)}`);
    }
    return result.result.value;
  }
}

async function connect(port) {
  // The browser's own websocket endpoint, from its HTTP interface.
  let version = null;
  for (let attempt = 0; attempt < 100; attempt += 1) {
    try {
      version = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();
      break;
    } catch {
      await new Promise((r) => setTimeout(r, 100));
    }
  }
  if (!version) throw new Error(`chromium did not open a devtools port on ${port}`);
  const socket = new WebSocket(version.webSocketDebuggerUrl);
  await new Promise((res, rej) => {
    socket.addEventListener("open", res, { once: true });
    socket.addEventListener("error", rej, { once: true });
  });
  const cdp = new Cdp(socket);
  const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
  const { sessionId } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  cdp.sessionId = sessionId;
  await cdp.send("Page.enable");
  await cdp.send("Runtime.enable");
  await cdp.send("Emulation.setDeviceMetricsOverride", {
    width: WIDTH, height: HEIGHT, deviceScaleFactor: 1, mobile: false,
  });
  return { cdp, version };
}

// ------------------------------------------------------------------------------------------
// The page: boot, settle, tidy
// ------------------------------------------------------------------------------------------

/// Wait for `boot()` to finish, then for the quadtree's load queue to be empty for `SETTLE_MS`.
///
/// **The queue, not a fixed sleep.** A fixed sleep is a bet that the tiles arrived, and this
/// viewer's tile cost varies by an order of magnitude with the camera; a run that photographed a
/// half-loaded planet would look like a rendering defect. `_surface._tileLoadQueue*` is Cesium's
/// own bookkeeping, not a counter this project maintains.
const SETTLE_EXPRESSION = (settleMs) => `(async () => {
  // **Wait for the module graph to have RUN before waiting on what it publishes.**
  // \`Page.navigate\` resolves when the navigation is committed, not when the page's modules have
  // evaluated, so an evaluate that fires immediately after it sees \`window.__wbBoot === undefined\`
  // -- and \`await undefined\` resolves at once, so the boot check then reads a \`__wbReady\` that
  // does not exist yet and reports "boot failed: undefined". That is a race in the harness
  // wearing the costume of a failure in the page, and it cost a run to find.
  const appeared = Date.now() + 60000;
  while (typeof window.__wbBoot === "undefined" && Date.now() < appeared) {
    await new Promise((r) => setTimeout(r, 50));
  }
  if (typeof window.__wbBoot === "undefined") throw new Error("main.js never evaluated");
  const ready = await Promise.race([
    (async () => { await window.__wbBoot; return window.__wbReady; })(),
    new Promise((r) => setTimeout(() => r({ ok: false, error: "boot timed out" }), 60000)),
  ]);
  if (!ready || !ready.ok) throw new Error("boot failed: " + (ready && ready.error));
  const surface = window.viewer.scene.globe._surface;
  const queued = () => surface._tileLoadQueueHigh.length + surface._tileLoadQueueMedium.length
    + surface._tileLoadQueueLow.length;
  const deadline = Date.now() + 180000;
  let quietSince = null;
  let quietAt = null;
  while (Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, 100));
    if (queued() === 0) {
      if (quietSince === null) { quietSince = Date.now(); quietAt = performance.now(); }
      if (Date.now() - quietSince >= ${settleMs}) break;
    } else { quietSince = null; quietAt = null; }
  }
  const wb = window.__wb || {};
  return {
    line: ready.line,
    // **Time from page start to the load queue going quiet**, in milliseconds, NOT including the
    // ${settleMs} ms the loop then waits to be sure. A figure that included the quiet window
    // would be measuring this harness's patience.
    settleMs: quietAt,
    maxDepthVisited: typeof wb.maxDepthVisited === "function" ? wb.maxDepthVisited() : null,
    pool: wb.pool ? wb.pool.stats() : null,
    relief: wb.reliefProvider ? {
      ...wb.reliefProvider.worldbuilder.stats,
      meanMs: wb.reliefProvider.worldbuilder.meanMs(),
      meanWorkerMs: wb.reliefProvider.worldbuilder.meanWorkerMs(),
      meanWallMs: wb.reliefProvider.worldbuilder.meanWallMs(),
    } : null,
    clouds: wb.cloudProvider ? {
      ...wb.cloudProvider.worldbuilder.stats,
      meanMs: wb.cloudProvider.worldbuilder.meanMs(),
      meanWorkerMs: wb.cloudProvider.worldbuilder.meanWorkerMs(),
      meanWallMs: wb.cloudProvider.worldbuilder.meanWallMs(),
      calibration: {
        cover: wb.cloudProvider.worldbuilder.clouds.cover,
        threshold: wb.cloudProvider.worldbuilder.clouds.threshold,
        sd: wb.cloudProvider.worldbuilder.clouds.sd,
      },
    } : null,
    // Long tasks: the browser's own definition (a task holding the main thread for 50 ms or
    // more), from its own PerformanceObserver, collected from before the first module ran. This
    // is the number the relief slice's worker move was measured by -- 53 long tasks before, zero
    // after -- and it is the number a third pool consumer has to be checked against.
    longTasks: (window.__wbLongTasks || []).length,
    longTaskMs: (window.__wbLongTasks || []).reduce((a, b) => a + b, 0),
    longTaskMax: (window.__wbLongTasks || []).reduce((a, b) => Math.max(a, b), 0),
    // Every long task, in order, rounded. A count and a sum can move for two different reasons
    // -- one more task, or one task grown longer -- and the list is what tells them apart.
    longTaskList: (window.__wbLongTasks || []).map((d) => Math.round(d)),
  };
})()`;

/// Installed before ANY page script runs, via `Page.addScriptToEvaluateOnNewDocument`. A
/// PerformanceObserver registered after boot would miss exactly the long tasks that boot causes,
/// which are the ones worth counting.
const LONG_TASK_PROBE = `
window.__wbLongTasks = [];
try {
  new PerformanceObserver((list) => {
    for (const entry of list.getEntries()) window.__wbLongTasks.push(entry.duration);
  }).observe({ entryTypes: ["longtask"] });
} catch (e) { window.__wbLongTaskError = String(e); }
`;

/// Hide the panel and Cesium's credit overlay. Two elements, named, rather than a stylesheet:
/// a screenshot with the panel in it is a screenshot of the panel.
const HIDE_CHROME = `(() => {
  for (const selector of ["#wb-panel", ".cesium-widget-credits", "#status"]) {
    for (const node of document.querySelectorAll(selector)) node.style.display = "none";
  }
  return true;
})()`;

/// Pin the camera, stop the render loop, and drive `FRAMES` frames at one explicit time.
const PIN_EXPRESSION = (iso, frames) => `(() => {
  const viewer = window.viewer;
  viewer.useDefaultRenderLoop = false;
  const time = Cesium.JulianDate.fromIso8601(${JSON.stringify(iso)});
  viewer.clock.currentTime = time;
  // Scene.render() with NO ARGUMENT defaults its frame time to JulianDate.now(), so pinning the
  // clock alone does nothing. Passing it explicitly is the whole of the fix.
  for (let i = 0; i < ${frames}; i += 1) viewer.scene.render(time);
  return true;
})()`;

async function capture(cdp) {
  const { data } = await cdp.send("Page.captureScreenshot", { format: "png", captureBeyondViewport: false });
  return Buffer.from(data, "base64");
}

// ------------------------------------------------------------------------------------------
// Driver
// ------------------------------------------------------------------------------------------

async function main() {
  const chrome = findChrome();
  const port = Number(args.port || 8137);
  // `--port` chooses the port; `--attach` says one is already listening there. They were the
  // same flag for one run, which meant asking for a second port silently skipped starting the
  // server and the page came back "main.js never evaluated" -- a 404 wearing the costume of a
  // broken build.
  let server = null;
  if (!args.attach) {
    server = spawn(process.execPath, [join(HERE, "serve.mjs")], {
      env: { ...process.env, PORT: String(port) }, stdio: "ignore",
    });
    await new Promise((r) => setTimeout(r, 400));
  }

  const devtoolsPort = Number(args.devtools || 9333);
  const userDir = join(process.env.TEMP || "/tmp", `wb-shoot-${process.pid}`);
  mkdirSync(userDir, { recursive: true });
  const browser = spawn(chrome, [
    "--headless=new",
    // SwiftShader rather than the host GPU: a headless run on a machine with a different driver
    // must produce the same bytes, which is what makes a digest comparable across hosts at all.
    "--use-angle=swiftshader",
    "--disable-gpu-sandbox",
    "--hide-scrollbars",
    "--no-first-run",
    "--no-default-browser-check",
    `--user-data-dir=${userDir}`,
    `--remote-debugging-port=${devtoolsPort}`,
    `--window-size=${WIDTH},${HEIGHT}`,
    "about:blank",
  ], { stdio: "ignore" });

  let code = 0;
  try {
    const { cdp, version } = await connect(devtoolsPort);
    await cdp.send("Page.addScriptToEvaluateOnNewDocument", { source: LONG_TASK_PROBE });
    const query = typeof args.url === "string" ? args.url : "";
    const url = `http://127.0.0.1:${port}/${query.startsWith("?") || query === "" ? query : `?${query}`}`;
    await cdp.send("Page.navigate", { url });
    const settled = await cdp.evaluate(SETTLE_EXPRESSION(SETTLE_MS));
    await cdp.evaluate(HIDE_CHROME);
    if (command === "digest") await cdp.evaluate(PIN_EXPRESSION(FRAME_TIME_ISO, FRAMES));
    // `measure` does not need a picture, but taking one costs a frame and keeps every command on
    // one settle rule -- two commands that settled differently would be two populations.
    const png = await capture(cdp);

    const out = args.out ? resolve(String(args.out)) : null;
    if (out) {
      mkdirSync(dirname(out), { recursive: true });
      writeFileSync(out, png);
    }
    const sha = createHash("sha256").update(png).digest("hex");
    if (command === "measure") {
      process.stdout.write(`${JSON.stringify({
        command, url, browser: version.Browser, viewport: `${WIDTH}x${HEIGHT}`,
        settleMs: settled.settleMs,
        maxDepthVisited: settled.maxDepthVisited,
        longTasks: settled.longTasks,
        longTaskMs: settled.longTaskMs,
        longTaskMax: settled.longTaskMax,
        longTaskList: settled.longTaskList,
        pool: settled.pool,
        relief: settled.relief,
        clouds: settled.clouds,
        status: settled.line,
      }, null, 2)}\n`);
      return;
    }
    // The status line goes out with every capture, because a screenshot's own caption is the
    // only thing that proves which world and which parameters it is a picture of.
    process.stdout.write(`${JSON.stringify({
      command,
      url,
      browser: version.Browser,
      viewport: `${WIDTH}x${HEIGHT}`,
      settleMs: SETTLE_MS,
      frameTime: command === "digest" ? FRAME_TIME_ISO : null,
      frames: command === "digest" ? FRAMES : null,
      bytes: png.length,
      sha256: sha,
      out,
      status: settled.line,
      tilesRendered: settled.tiles,
    }, null, 2)}\n`);
  } catch (error) {
    process.stderr.write(`${error.stack || error}\n`);
    code = 1;
  } finally {
    browser.kill();
    if (server) server.kill();
  }
  process.exit(code);
}

await main();
