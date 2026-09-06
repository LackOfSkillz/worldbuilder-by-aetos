// A dependency-free headless-Chromium probe that measures the viewer's RENDERED PIXELS
// against the depth or height the ENGINE reports at the point each pixel is looking at.
//
// # Why this file is committed
//
// The ocean task built exactly this, in a scratchpad, and named its loss as concern 6 of its
// report: *"The pixel probe is not committed... It is the only tool in the slice that can measure
// RENDERED colour against a STATED depth, and it is exactly the kind of thing the last task
// committed `shoot.mjs` to stop losing. Somewhere under `viewer/scripts/` would end it."* This is
// that, and the atmosphere task is the second consumer -- which is the point at which a scratchpad
// tool has demonstrably been rebuilt once too often.
//
// # What it measures, and why the measurement is honest
//
// Cesium's sun lighting multiplies all three channels of a ground texel EQUALLY, by a scalar this
// code cannot see. So an absolute RGB comparison through the renderer is not a comparison of the
// two things you meant to compare. Two quantities survive that scalar:
//
//   * **chromaticity**, `r/(r+g+b)` and `g/(r+g+b)` -- invariant under the scalar entirely, which
//     is how the ocean task established WHICH colour system paints the sea;
//   * **the ratio of a spread to its own mean** -- the scalar cancels, so a contrast figure is
//     comparable between builds even when the absolute level is not.
//
// Both are reported. Absolute luminance is reported too, because within ONE build at ONE camera
// the scalar is a constant and a before/after of the same camera is a fair comparison of it --
// which is the comparison every figure in the photoreal slice's reports is.
//
// **Ground truth comes from the engine, not from the colour.** Every sampled pixel is ray-picked
// against the scene's ellipsoid, converted to lat/lon, and its height asked of `wb_elevation_m`.
// Inferring depth from the pixel's own blue-ness would be assuming the answer.
//
// # Usage
//
//   node scripts/probe.mjs --url "<query string>" [--step 6] [--json out.json]
//
// `--step` is the pixel stride in x and y (default 6, which is what the ocean task used and what
// its 17,921-pixel ocean population was drawn at). Same `--url`, `--port`, `--attach`, `--chrome`,
// `--width`, `--height`, `--settle`, `--time` and `--frames` conventions as `shoot.mjs`, and the
// SAME settle rule and the SAME pinned frame time, so a probe and a digest are photographs of one
// picture rather than of two.

import { spawn } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { inflateSync } from "node:zlib";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));

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
const WIDTH = Number(args.width || 1600);
const HEIGHT = Number(args.height || 900);
const SETTLE_MS = Number(args.settle || 4000);
const FRAME_TIME_ISO = String(args.time || "2026-09-05T12:00:00Z");
const FRAMES = Number(args.frames || 20);
const STEP = Number(args.step || 6);
/// Depth below which a water pixel counts as "ocean" rather than "shore". 200 m is the ocean
/// task's own cut and is repeated here so the two tasks quote one population.
const OCEAN_DEPTH_M = Number(args.oceanDepth || -200);
/// The limb profile's bin width, in kilometres. One Cesium Rayleigh scale height.
const LIMB_BIN_KM = Number(args.limbBinKm || 10);

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
  throw new Error(`no chromium found. Pass --chrome <path> or set WB_CHROME.`);
}

// ------------------------------------------------------------------------------------------
// PNG decode -- 8-bit RGB/RGBA, non-interlaced, which is what `Page.captureScreenshot` emits
// ------------------------------------------------------------------------------------------
//
// **The pixels are taken from the SAME PNG the digest is a hash of**, rather than read back out
// of the WebGL canvas in the page. The viewer's context is created without
// `preserveDrawingBuffer`, so an in-page `drawImage` of the canvas is only defined inside the
// task that rendered it; going through the screenshot removes that race entirely and, more
// usefully, means the numbers below describe the exact bytes `shoot.mjs digest` fingerprints.

function decodePng(buffer) {
  if (buffer.readUInt32BE(0) !== 0x89504e47) throw new Error("not a PNG");
  let offset = 8;
  let width = 0; let height = 0; let bitDepth = 0; let colorType = 0; let interlace = 0;
  const idat = [];
  while (offset < buffer.length) {
    const length = buffer.readUInt32BE(offset);
    const type = buffer.toString("ascii", offset + 4, offset + 8);
    const data = buffer.subarray(offset + 8, offset + 8 + length);
    if (type === "IHDR") {
      width = data.readUInt32BE(0);
      height = data.readUInt32BE(4);
      bitDepth = data[8]; colorType = data[9]; interlace = data[12];
    } else if (type === "IDAT") idat.push(data);
    else if (type === "IEND") break;
    offset += 12 + length;
  }
  if (bitDepth !== 8) throw new Error(`unsupported PNG bit depth ${bitDepth}`);
  if (interlace !== 0) throw new Error("interlaced PNG is not supported");
  const channels = { 0: 1, 2: 3, 4: 2, 6: 4 }[colorType];
  if (!channels) throw new Error(`unsupported PNG colour type ${colorType}`);
  const raw = inflateSync(Buffer.concat(idat));
  const stride = width * channels;
  const out = Buffer.alloc(height * stride);
  let pos = 0;
  for (let y = 0; y < height; y += 1) {
    const filter = raw[pos]; pos += 1;
    const line = raw.subarray(pos, pos + stride); pos += stride;
    const dst = y * stride;
    const up = dst - stride;
    for (let x = 0; x < stride; x += 1) {
      const a = x >= channels ? out[dst + x - channels] : 0;
      const b = y > 0 ? out[up + x] : 0;
      const c = (x >= channels && y > 0) ? out[up + x - channels] : 0;
      let value = line[x];
      if (filter === 1) value += a;
      else if (filter === 2) value += b;
      else if (filter === 3) value += (a + b) >> 1;
      else if (filter === 4) {
        const p = a + b - c;
        const pa = Math.abs(p - a); const pb = Math.abs(p - b); const pc = Math.abs(p - c);
        value += (pa <= pb && pa <= pc) ? a : (pb <= pc ? b : c);
      } else if (filter !== 0) throw new Error(`unknown PNG filter ${filter}`);
      out[dst + x] = value & 0xff;
    }
  }
  return { width, height, channels, data: out };
}

// ------------------------------------------------------------------------------------------
// Statistics
// ------------------------------------------------------------------------------------------

function quantile(sorted, q) {
  if (sorted.length === 0) return null;
  const index = (sorted.length - 1) * q;
  const low = Math.floor(index); const high = Math.ceil(index);
  return sorted[low] + (sorted[high] - sorted[low]) * (index - low);
}

function summarise(values) {
  if (values.length === 0) return { n: 0 };
  const sorted = Float64Array.from(values).sort();
  const n = sorted.length;
  let sum = 0;
  for (const v of sorted) sum += v;
  const mean = sum / n;
  let sq = 0;
  for (const v of sorted) sq += (v - mean) * (v - mean);
  const sd = Math.sqrt(sq / n);
  const p01 = quantile(sorted, 0.01); const p05 = quantile(sorted, 0.05);
  const p50 = quantile(sorted, 0.5);
  const p95 = quantile(sorted, 0.95); const p99 = quantile(sorted, 0.99);
  return {
    n,
    mean: +mean.toFixed(3),
    sd: +sd.toFixed(3),
    // The spread the sun-lighting scalar CANCELS out of, and therefore the only spread figure
    // that is comparable between two builds whose overall brightness differs.
    cv: +(sd / (mean || 1)).toFixed(4),
    min: +sorted[0].toFixed(2),
    p01: +p01.toFixed(2),
    p05: +p05.toFixed(2),
    p50: +p50.toFixed(2),
    p95: +p95.toFixed(2),
    p99: +p99.toFixed(2),
    max: +sorted[n - 1].toFixed(2),
    p95_p05: +(p95 - p05).toFixed(2),
    p99_over_p01: +(p99 / (p01 || 1)).toFixed(3),
  };
}

// ------------------------------------------------------------------------------------------
// CDP
// ------------------------------------------------------------------------------------------

class Cdp {
  constructor(socket) {
    this.socket = socket; this.next = 1; this.pending = new Map(); this.sessionId = null;
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
    const id = this.next; this.next += 1;
    const payload = { id, method, params };
    if (this.sessionId) payload.sessionId = this.sessionId;
    this.socket.send(JSON.stringify(payload));
    return new Promise((res, rej) => this.pending.set(id, { resolve: res, reject: rej, method }));
  }

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
  let version = null;
  for (let attempt = 0; attempt < 100; attempt += 1) {
    try {
      version = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();
      break;
    } catch { await new Promise((r) => setTimeout(r, 100)); }
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

/// Byte-for-byte `shoot.mjs`'s settle rule. Repeated rather than imported because `shoot.mjs` is
/// a script with a top-level `await main()`; importing it would run it. Any change to settling
/// must be made in both, and this comment is where a future reader is told so.
const SETTLE_EXPRESSION = (settleMs) => `(async () => {
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
  while (Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, 100));
    if (queued() === 0) {
      if (quietSince === null) quietSince = Date.now();
      if (Date.now() - quietSince >= ${settleMs}) break;
    } else quietSince = null;
  }
  return { line: ready.line };
})()`;

const HIDE_CHROME = `(() => {
  for (const selector of ["#wb-panel", ".cesium-widget-credits", "#status"]) {
    for (const node of document.querySelectorAll(selector)) node.style.display = "none";
  }
  return true;
})()`;

const PIN_EXPRESSION = (iso, frames) => `(() => {
  const viewer = window.viewer;
  viewer.useDefaultRenderLoop = false;
  const time = Cesium.JulianDate.fromIso8601(${JSON.stringify(iso)});
  viewer.clock.currentTime = time;
  for (let i = 0; i < ${frames}; i += 1) viewer.scene.render(time);
  return true;
})()`;

/// Ray-pick every sampled pixel against the scene's ellipsoid and ask the ENGINE for the height
/// there. Returns three parallel plain arrays -- x, y and metres -- because a hundred thousand
/// small objects over the DevTools protocol is the slow way to move the same three numbers.
///
/// `pickEllipsoid` uses the ellipsoid, not the terrain, so a pixel's ground point is the point the
/// SPHERE is showing there. At the orbital camera this differs from the displaced surface by well
/// under the sampling stride, and using it means the classification does not depend on which
/// terrain tiles happened to be resident.
const PICK_EXPRESSION = (step) => `(() => {
  const scene = window.viewer.scene;
  const wb = window.__wb;
  const canvas = scene.canvas;
  const width = canvas.width, height = canvas.height;
  const ellipsoid = scene.globe.ellipsoid;
  const xs = [], ys = [], ms = [];
  const scratch = new Cesium.Cartesian2();
  const carto = new Cesium.Cartographic();
  for (let y = 0; y < height; y += ${step}) {
    for (let x = 0; x < width; x += ${step}) {
      scratch.x = x + 0.5; scratch.y = y + 0.5;
      const cart = scene.camera.pickEllipsoid(scratch, ellipsoid);
      if (!cart) continue;
      Cesium.Cartographic.fromCartesian(cart, ellipsoid, carto);
      const lat = Cesium.Math.toDegrees(carto.latitude);
      const lon = Cesium.Math.toDegrees(carto.longitude);
      xs.push(x); ys.push(y);
      ms.push(wb.engine.elevationM(wb.world, lat, lon));
    }
  }

  // ----------------------------------------------------------------------------------------
  // The LIMB, measured in kilometres of altitude rather than in pixels
  // ----------------------------------------------------------------------------------------
  //
  // A pixel outside the silhouette is looking THROUGH the atmosphere at a grazing angle, and
  // the physically meaningful coordinate for it is the lowest altitude its ray reaches --
  // \`IntersectionTests.grazingAltitudeLocation\`, which is Cesium's own solver for exactly that
  // point. Profiling brightness against that altitude gives a limb thickness in KILOMETRES,
  // which is a property of the atmosphere; profiling it against pixels would give a property of
  // the camera distance, and the two tasks that photograph this planet use two distances.
  //
  // Rays whose grazing point is behind the camera are the far side of the sky and are dropped.
  const lxs = [], lys = [], lalt = [];
  const ray = new Cesium.Ray();
  const forward = scene.camera.direction;
  const eye = scene.camera.positionWC;
  for (let y = 0; y < height; y += ${step}) {
    for (let x = 0; x < width; x += ${step}) {
      scratch.x = x + 0.5; scratch.y = y + 0.5;
      if (scene.camera.pickEllipsoid(scratch, ellipsoid)) continue;
      scene.camera.getPickRay(scratch, ray);
      const grazing = Cesium.IntersectionTests.grazingAltitudeLocation(ray, ellipsoid);
      if (!grazing) continue;
      const toward = Cesium.Cartesian3.subtract(grazing, eye, new Cesium.Cartesian3());
      if (Cesium.Cartesian3.dot(toward, forward) <= 0) continue;
      const c = ellipsoid.cartesianToCartographic(grazing);
      if (!c) continue;
      lxs.push(x); lys.push(y); lalt.push(c.height);
    }
  }
  return { width, height, cssWidth: canvas.clientWidth, xs, ys, ms, lxs, lys, lalt };
})()`;

// ------------------------------------------------------------------------------------------
// Driver
// ------------------------------------------------------------------------------------------

async function main() {
  const chrome = findChrome();
  const port = Number(args.port || 8137);
  let server = null;
  if (!args.attach) {
    server = spawn(process.execPath, [join(HERE, "serve.mjs")], {
      env: { ...process.env, PORT: String(port) }, stdio: "ignore",
    });
    await new Promise((r) => setTimeout(r, 400));
  }

  const devtoolsPort = Number(args.devtools || 9333);
  const userDir = join(process.env.TEMP || "/tmp", `wb-probe-${process.pid}`);
  mkdirSync(userDir, { recursive: true });
  const browser = spawn(chrome, [
    "--headless=new", "--use-angle=swiftshader", "--disable-gpu-sandbox", "--hide-scrollbars",
    "--no-first-run", "--no-default-browser-check",
    `--user-data-dir=${userDir}`,
    `--remote-debugging-port=${devtoolsPort}`,
    `--window-size=${WIDTH},${HEIGHT}`,
    "about:blank",
  ], { stdio: "ignore" });

  let code = 0;
  try {
    const { cdp, version } = await connect(devtoolsPort);
    const query = typeof args.url === "string" ? args.url : "";
    const url = `http://127.0.0.1:${port}/${query.startsWith("?") || query === "" ? query : `?${query}`}`;
    await cdp.send("Page.navigate", { url });
    const settled = await cdp.evaluate(SETTLE_EXPRESSION(SETTLE_MS));
    await cdp.evaluate(HIDE_CHROME);
    await cdp.evaluate(PIN_EXPRESSION(FRAME_TIME_ISO, FRAMES));
    const shot = await cdp.send("Page.captureScreenshot", {
      format: "png", captureBeyondViewport: false,
    });
    const png = decodePng(Buffer.from(shot.data, "base64"));
    const picked = await cdp.evaluate(PICK_EXPRESSION(STEP));

    // The drawing buffer and the screenshot can differ in scale if the device pixel ratio is not
    // 1. It is pinned to 1 above; this asserts it rather than assuming it, because a silent 2x
    // would index the wrong pixels and still produce plausible statistics.
    if (picked.width !== png.width || picked.height !== png.height) {
      throw new Error(
        `canvas ${picked.width}x${picked.height} does not match screenshot ${png.width}x${png.height}`,
      );
    }

    const ocean = { lum: [], rc: [], gc: [], r: [], g: [], b: [] };
    const land = { lum: [], rc: [], gc: [], r: [], g: [], b: [] };
    let offGlobe = 0;
    for (let i = 0; i < picked.xs.length; i += 1) {
      const index = (picked.ys[i] * png.width + picked.xs[i]) * png.channels;
      const r = png.data[index]; const g = png.data[index + 1]; const b = png.data[index + 2];
      const luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b;
      const total = r + g + b;
      const metres = picked.ms[i];
      const bucket = metres <= OCEAN_DEPTH_M ? ocean : (metres > 0 ? land : null);
      if (!bucket) { offGlobe += 1; continue; }
      bucket.lum.push(luminance);
      bucket.r.push(r); bucket.g.push(g); bucket.b.push(b);
      if (total > 0) { bucket.rc.push(r / total); bucket.gc.push(g / total); }
    }
    /// Sky pixels, binned by the lowest altitude their ray reaches. 10 km a bin over 0..400 km:
    /// Cesium's Rayleigh scale height is 10 km, so one bin is one scale height and a ring that is
    /// one bin wide is the physically thin one the gap analysis complains about.
    const limbBins = Array.from({ length: 40 }, () => ({ lum: [], rgb: [] }));
    for (let i = 0; i < picked.lxs.length; i += 1) {
      const bin = Math.floor(picked.lalt[i] / (LIMB_BIN_KM * 1000));
      if (bin < 0 || bin >= limbBins.length) continue;
      const index = (picked.lys[i] * png.width + picked.lxs[i]) * png.channels;
      const r = png.data[index]; const g = png.data[index + 1]; const b = png.data[index + 2];
      limbBins[bin].lum.push(0.2126 * r + 0.7152 * g + 0.0722 * b);
      limbBins[bin].rgb.push([r, g, b]);
    }

    /// The mean colour of each population, as a triple. Scaled by the lighting scalar like any
    /// absolute figure -- but it is the form the ORIGINAL ground-atmosphere objection was written
    /// in ("a deep-ocean point read (122,172,137), the same colour as land 500 m up"), so a
    /// re-test of that finding has to be able to quote the same kind of number back.
    const meanRgb = (bucket) => (bucket.r.length === 0 ? null : [
      +(bucket.r.reduce((a, v) => a + v, 0) / bucket.r.length).toFixed(1),
      +(bucket.g.reduce((a, v) => a + v, 0) / bucket.g.length).toFixed(1),
      +(bucket.b.reduce((a, v) => a + v, 0) / bucket.b.length).toFixed(1),
    ]);

    const report = {
      url,
      browser: version.Browser,
      viewport: `${WIDTH}x${HEIGHT}`,
      step: STEP,
      frameTime: FRAME_TIME_ISO,
      frames: FRAMES,
      settleMs: SETTLE_MS,
      sampled: picked.xs.length,
      shoreOrShallow: offGlobe,
      oceanDepthCutM: OCEAN_DEPTH_M,
      ocean: {
        meanRgb: meanRgb(ocean),
        luminance: summarise(ocean.lum),
        chromaticityR: summarise(ocean.rc),
        chromaticityG: summarise(ocean.gc),
      },
      land: {
        meanRgb: meanRgb(land),
        luminance: summarise(land.lum),
        chromaticityR: summarise(land.rc),
        chromaticityG: summarise(land.gc),
      },
      // **The wash-out metric, and the reason this block exists.**
      //
      // Ground atmosphere was switched off because it made the ocean *the same colour as land* --
      // so the quantity that decides whether it may come back is not either population's own
      // spread but the SEPARATION between them. Three forms, because they fail differently:
      //
      //   * `luminanceGap` -- how many levels of 255 apart the two medians are. Direct, and
      //     scaled by the unknown lighting scalar, so comparable only within one camera.
      //   * `luminanceD` -- that gap over the pooled standard deviation. A d of 0 is a wash;
      //     the scalar cancels top and bottom, so this one travels between builds.
      //   * `chromaticityGap` -- Euclidean distance between the two mean chromaticities.
      //     **Immune to the lighting scalar entirely**, and the form that would catch a haze
      //     that greys sea and land towards one hue while leaving them different brightnesses.
      separation: (() => {
        const oceanLum = summarise(ocean.lum); const landLum = summarise(land.lum);
        if (!oceanLum.n || !landLum.n) return { n: 0 };
        const pooled = Math.sqrt((oceanLum.sd ** 2 + landLum.sd ** 2) / 2);
        const oceanR = summarise(ocean.rc).mean; const oceanG = summarise(ocean.gc).mean;
        const landR = summarise(land.rc).mean; const landG = summarise(land.gc).mean;
        return {
          luminanceGap: +(landLum.p50 - oceanLum.p50).toFixed(3),
          luminanceGapMeans: +(landLum.mean - oceanLum.mean).toFixed(3),
          luminanceD: +((landLum.mean - oceanLum.mean) / (pooled || 1)).toFixed(4),
          chromaticityGap: +Math.hypot(landR - oceanR, landG - oceanG).toFixed(5),
        };
      })(),
      // **The limb profile: brightness against the ray's lowest altitude, in 10 km bins.**
      //
      // `thicknessKm` is the altitude at which the glow has fallen to HALF the brightness of the
      // lowest bin -- a half-height, which is the standard way to state the extent of something
      // with no edge. `peak` is the lowest bin's own mean colour, i.e. what the ring is made of.
      // Difference #10 of the gap analysis is "thin hard white ring" against "thick blue haze",
      // and those are the two numbers that say which of the two this is: a thickness in km, and
      // a chromaticity that is either neutral (white) or blue-dominant.
      limb: (() => {
        // **The BRIGHT limb, not the mean limb.** A ring around a lit planet is bright on the
        // sunward side and black on the night side, and its mean is a statement about where the
        // terminator happens to fall in frame. The quantity the gap analysis is about is how far
        // the glow reaches where there IS glow, so each altitude bin is summarised by its
        // brightest tenth (p90) and the profile is read off that.
        const bins = [];
        for (let i = 0; i < limbBins.length; i += 1) {
          const bin = limbBins[i];
          if (bin.lum.length < 8) { bins.push(null); continue; }
          const order = bin.lum.map((v, k) => [v, k]).sort((a, b) => a[0] - b[0]);
          const at = Math.floor((order.length - 1) * 0.9);
          const bright = order.slice(at).map(([, k]) => bin.rgb[k]);
          const mean = [0, 1, 2].map((c) => bright.reduce((a, v) => a + v[c], 0) / bright.length);
          bins.push({
            fromKm: i * LIMB_BIN_KM,
            n: bin.lum.length,
            p90Luminance: +order[at][0].toFixed(3),
            brightRgb: mean.map((v) => +v.toFixed(1)),
          });
        }
        const filled = bins.filter(Boolean);
        if (filled.length === 0) return { n: 0 };
        // The peak is the brightest bin, not the lowest one: at a camera close enough for the
        // disc to overflow the frame the lowest bins are only sampled where the silhouette cuts
        // the picture edge, and taking bin zero as the peak would measure that accident.
        let peak = filled[0];
        for (const bin of filled) if (bin.p90Luminance > peak.p90Luminance) peak = bin;
        const half = peak.p90Luminance / 2;
        let halfHeightKm = null;
        for (const bin of filled) {
          if (bin.fromKm > peak.fromKm && bin.p90Luminance <= half) {
            halfHeightKm = bin.fromKm - peak.fromKm;
            break;
          }
        }
        const total = peak.brightRgb[0] + peak.brightRgb[1] + peak.brightRgb[2];
        return {
          n: filled.reduce((a, bin) => a + bin.n, 0),
          binKm: LIMB_BIN_KM,
          peakKm: peak.fromKm,
          peakLuminance: peak.p90Luminance,
          peakRgb: peak.brightRgb,
          // r and b as fractions of the sum. A neutral WHITE ring reads 0.333/0.333; a BLUE haze
          // reads r well under b. This is the number difference #10 is written in.
          peakChromaticityRB: total > 0
            ? [+(peak.brightRgb[0] / total).toFixed(4), +(peak.brightRgb[2] / total).toFixed(4)]
            : null,
          halfHeightKm,
          // Total altitude over which the glow is at least half its peak, counted as bins rather
          // than as an interval: a real limb profile is not monotone (the terminator crosses it)
          // and an interval measured by walking outwards from the peak stops at the first dip.
          widthKm: filled.filter((bin) => bin.p90Luminance >= half).length * LIMB_BIN_KM,
          // What the silhouette's own edge looks like. `base` is the bottom bin: a HARD RIM is a
          // bright bin zero, a HALO is a dark bin zero with the glow standing off above it.
          edgeLuminance: filled[0].fromKm === 0 ? filled[0].p90Luminance : null,
          bins: filled.slice(0, 24),
        };
      })(),
      status: settled.line,
    };
    const text = `${JSON.stringify(report, null, 2)}\n`;
    process.stdout.write(text);
    if (typeof args.json === "string") {
      mkdirSync(dirname(args.json), { recursive: true });
      writeFileSync(args.json, text);
    }
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
