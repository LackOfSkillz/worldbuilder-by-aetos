// Minimal static server for viewer/public. Loopback only, no proxying, no upstream:
// anything the page fetches from this origin appears in the log below, and anything
// NOT in the log went somewhere else.
import { createServer } from "node:http";
import { createReadStream } from "node:fs";
import { stat } from "node:fs/promises";
import { join, normalize, extname, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "public");
const port = Number(process.env.PORT || 8137);
const TYPES = {
  ".html": "text/html; charset=utf-8", ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8", ".css": "text/css; charset=utf-8",
  ".json": "application/json", ".wasm": "application/wasm", ".png": "image/png",
  ".jpg": "image/jpeg", ".gif": "image/gif", ".svg": "image/svg+xml",
  ".ktx2": "image/ktx2", ".glb": "model/gltf-binary", ".gltf": "model/gltf+json",
  ".xml": "application/xml", ".ico": "image/x-icon", ".woff": "font/woff",
  ".woff2": "font/woff2", ".ttf": "font/ttf", ".terrain": "application/octet-stream",
  ".md": "text/plain; charset=utf-8", ".txt": "text/plain; charset=utf-8",
};

// ---------------------------------------------------------------------------------------
// The Content-Security-Policy.
//
// Task 1 *witnessed* that nothing leaves the origin: 19 requests, 0 off-origin, still 0
// after flying the camera to five points. But absence of traffic is not absence of
// capability -- that trace shows this browser DID NOT phone home, not that the page
// CANNOT. `default-src 'self'` is what converts the observation into a guarantee: every
// fetch, image, style, font, worker, frame and XHR must be same-origin or it is refused
// by the browser before it reaches the network.
//
// Every directive below was arrived at by starting from `default-src 'self'` alone and
// adding only what the browser actually reported as a violation. Nothing here is
// precautionary, and each relaxation is named with the thing that forced it:
//
//   script-src 'self' 'unsafe-eval' blob:
//       'self'          -- /app/*.js and /vendor/cesium/Cesium.js. index.html has NO
//                          inline <script>: the two former inline blocks are now
//                          /app/cesium-base-url.js and /app/boot.js precisely so that
//                          'unsafe-inline' is not needed here.
//       'unsafe-eval'   -- forced by the vendored bundle, not by us. Cesium 1.145.0
//                          embeds Knockout, whose UMD preamble at Cesium.js:18266 is
//                          `var t = this || (0,eval)("this")`. The bundle is strict, so
//                          `this` is undefined there and the eval always runs; without
//                          this token Cesium.js throws EvalError at load and `Cesium` is
//                          never defined. It is in index.js and index.cjs too, so no
//                          other Cesium build avoids it, and patching the vendored tree
//                          would break the byte-for-byte check in cesium-manifest.txt.
//                          NOTE: 'unsafe-eval' subsumes 'wasm-unsafe-eval', which is
//                          otherwise required here -- WebAssembly.instantiate is blocked
//                          by a bare `default-src 'self'`, and was, five times from
//                          Cesium's own KTX2/Draco modules and twice from
//                          /app/engine.js. It does NOT weaken the network guarantee:
//                          script-src governs code execution, not egress, and eval'd
//                          code is still bound by connect-src/img-src below.
//       blob:           -- Cesium's workers are blob: URLs and `importScripts()` further
//                          blob: URLs from inside them; a worker inherits this policy.
//   worker-src 'self' blob:
//       'self'          -- /app/tile-worker.js, the eight Task 5 module workers.
//       blob:           -- Cesium's own worker pool, same as above.
//   style-src 'self' 'unsafe-inline'
//       'self'          -- /vendor/cesium/Widgets/widgets.css and /app/viewer.css.
//       'unsafe-inline' -- forced by Cesium, which both sets style attributes
//                          (style-src-attr, Cesium.js:79, :6070, :6071) and injects
//                          <style> elements (style-src-elem, Cesium.js:13394). index.html
//                          itself has no inline <style> any more.
//   img-src 'self' data:
//       data:           -- the `<link rel="icon" href="data:,">` that stops the browser
//                          asking for /favicon.ico. No network reach.
//   object-src / base-uri / form-action / frame-ancestors 'none'
//                       -- nothing here uses plugins, a <base> tag, form submission or
//                          being framed, so they are shut rather than left on the
//                          default-src fallback (base-uri and form-action do not fall
//                          back to default-src at all).
//
// connect-src, font-src, media-src, frame-src and the rest are deliberately ABSENT: they
// fall back to `default-src 'self'`, which is exactly what is wanted. connect-src is the
// one that refuses `?net-probe=1`.
//
// Proved able to refuse, `?net-probe=1`, same page, one header apart:
//   policy ON : 1 securitypolicyviolation (connect-src, api.cesium.com), request never
//               completes, 0 hosts reached.
//   policy OFF: 34 off-origin resource entries across 6 hosts -- api.cesium.com,
//               dev.virtualearth.net and ecn.t{0,1,2,3}.tiles.virtualearth.net over
//               plaintext http.
const CSP = [
  "default-src 'self'",
  "script-src 'self' 'unsafe-eval' blob:",
  "worker-src 'self' blob:",
  "style-src 'self' 'unsafe-inline'",
  "img-src 'self' data:",
  "object-src 'none'",
  "base-uri 'none'",
  "form-action 'none'",
  "frame-ancestors 'none'",
].join("; ");

createServer(async (req, res) => {
  const url = new URL(req.url, "http://localhost");
  let raw = decodeURIComponent(url.pathname);
  if (raw.endsWith("/")) raw += "index.html";
  let p = normalize(raw);
  while (p.startsWith("..")) p = p.slice(2);
  const file = join(root, p);
  if (!file.startsWith(root)) { res.writeHead(403).end("forbidden"); return; }
  try {
    const s = await stat(file);
    if (!s.isFile()) throw new Error("not a file");
    console.log(`200 ${req.method} ${req.url}`);
    res.writeHead(200, {
      "content-type": TYPES[extname(file).toLowerCase()] || "application/octet-stream",
      "content-length": s.size,
      "cache-control": "no-store",
      "content-security-policy": CSP,
      "cross-origin-resource-policy": "same-origin",
      // COOP/COEP. These arrived in Task 5 labelled "for the SharedArrayBuffer worker
      // pool", and that label was wrong: Task 5 shipped eight *module* workers, the
      // engine wasm has zero imports and no shared memory, and nothing in this tree
      // touches SharedArrayBuffer. Task 6 measured what removing them actually changes,
      // and kept them for the one thing that did change:
      //
      //   crossOriginIsolated      true -> false
      //   performance.now() step   5 us -> 100 us   (Chrome's non-isolated clamp)
      //   16,900-byte slice(),     0.010 ms -> 0.000 ms, with 1,731 of 1,920 samples
      //     n=1,920 median         reading exactly zero
      //
      // That copy is `bench.js`'s `timeHandouts`, and the "0.02 ms median over n=1,920"
      // in README.md is the evidence for "the copy on the way out of the cache is not
      // optional". Below the 100 us clamp that measurement does not get worse, it stops
      // existing. Everything else was identical with and without: 12/0 checks, no CSP
      // violations, maxDepthVisited 16 over 39 tiles either way.
      //
      // They cost nothing here -- every response is same-origin and already carries
      // CORP -- but they are not free forever: under COEP require-corp any subresource
      // that ever loses that header fails. If `timeHandouts` is ever changed to time a
      // batch of copies instead of one, these two headers stop earning their place and
      // should go.
      "cross-origin-opener-policy": "same-origin",
      "cross-origin-embedder-policy": "require-corp",
    });
    createReadStream(file).pipe(res);
  } catch {
    console.log(`404 ${req.method} ${req.url}`);
    res.writeHead(404, { "content-type": "text/plain" }).end("not found");
  }
}).listen(port, "127.0.0.1", () => console.log(`viewer: http://127.0.0.1:${port}/`));
