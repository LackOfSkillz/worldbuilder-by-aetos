// Minimal static server for viewer/public. Loopback only, no proxying, no upstream:
// anything the page fetches from this origin appears in the log below, and anything
// NOT in the log went somewhere else.
import { createServer } from "node:http";
import { createReadStream, existsSync } from "node:fs";
import { stat, readdir, readFile, writeFile, mkdir } from "node:fs/promises";
import { join, normalize, extname, dirname } from "node:path";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";

// ---------------------------------------------------------------------------------------
// Caching: a VALIDATOR, not a lifetime.
//
// Measured on one cold load of the owner's world, this server's own log: **158 requests,
// 11.79 MB, of which 4.08 MB is the same bytes fetched nine times.** The engine wasm goes
// out 9 times and each of 13 `/app/*.js` modules goes out 9 times, because there are nine
// engine instances -- the main thread plus eight pool workers -- and every one of them
// fetches the whole `engine.js` module graph and the wasm for itself. `cache-control:
// no-store` on every response is what defeats the HTTP cache and makes those eight repeats
// real network transfers.
//
// **`no-store` was right about the thing it was protecting.** This is a dev server with no
// build step: a file is edited and the page is reloaded, and a `max-age` would serve the
// old bytes until it expired. Trading a working edit-reload loop for ~0.3 s of loopback
// transfer would be a bad trade and it is not the one made here.
//
// What is used instead is a validator. `cache-control: no-cache` does NOT mean "do not
// cache" -- it means *store it, but revalidate before every reuse*. Paired with an `ETag`
// the browser sends `If-None-Match` and this server answers `304 Not Modified` with no
// body, so:
//
//   * an edit changes the file's size or mtime, so it changes the ETag, so the very next
//     request gets a 200 with the new bytes -- the edit-reload loop is bit-for-bit the
//     behaviour `no-store` gave;
//   * the eight repeat fetches per file become eight empty 304s.
//
// The request COUNT is unchanged (that is what "revalidate every time" means, and it is the
// price of never being stale); the bytes are not.
//
// The ETag is `size-mtimeMs` in hex, which is exactly the pair that changes when a file is
// edited, and it is weak (`W/`) because it is derived from the file's metadata rather than
// from a hash of its bytes -- a touch with no edit rotates it, which costs one re-transfer
// and cannot serve anything stale.
//
// **`/` and `*.html` keep `no-store`.** The document is the one response where a 304 buys
// nothing (it is fetched once per load either way) and where a caching mistake is the one
// that looks like "my edit did not appear".
function cacheHeaders(pathname, stats) {
  if (pathname === "/" || pathname.endsWith(".html")) return { "cache-control": "no-store" };
  return {
    "cache-control": "no-cache",
    etag: `W/"${stats.size.toString(16)}-${Math.floor(stats.mtimeMs).toString(16)}"`,
  };
}

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "public");

// **The world library, served from the repository rather than from `public/`.**
//
// A saved world is a file somebody keeps - checked into a repository, reviewed, handed to
// somebody else - so it lives with the source and not inside the served tree. Serving it needs
// one route rather than a copy, because a copy is a second answer to "which world is Aetosia"
// and this project has been bitten by two copies of one thing before.
//
// Read-only, and containment-checked exactly like `root`: a path that escapes the directory is
// refused before it reaches the filesystem.
const worldsDir = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "worlds");

/// Which interpreter runs the generator.
///
/// **`python` on PATH is not necessarily the one with the engine in it.** The pyo3 module
/// is built into a virtualenv by maturin, and a stale system install answers imports
/// perfectly well while missing everything added since - measured here as
/// `module 'worldbuilder_engine' has no attribute 'relief_canonical'`, which reads like a
/// broken build and is a wrong interpreter. `WB_PYTHON` overrides; otherwise the venvs
/// this project actually uses are tried in order before falling back.
function pythonForGenerator() {
  if (process.env.WB_PYTHON) return process.env.WB_PYTHON;
  const root = join(worldsDir, "..");
  const candidates = [
    join(root, ".venv", "Scripts", "python.exe"),
    join(root, ".venv", "bin", "python"),
    join(root, "..", "worldbuilder_by_aetos", ".venv", "Scripts", "python.exe"),
    join(root, "..", "worldbuilder_by_aetos", ".venv", "bin", "python"),
  ];
  for (const candidate of candidates) {
    if (existsSync(candidate)) return candidate;
  }
  return "python";
}


// Routes an author clicks onto the globe. Written from the browser, read from the shell -
// which is the whole point: an intent somebody drew should not have to be handed over as a
// file attachment before anything can be built from it.
const routesDir = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "routes");
const runsDir = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "runs");
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

  // `/worlds/` lists what is on disk; `/worlds/<name>.json` serves one. The listing carries the
  // name and the area count out of each file, so the panel can label a row without fetching
  // every world to find out what is in it.
  // POST /routes/ writes one authored route. The ONLY write this server accepts, and it
  // accepts it into one directory under one extension - a dev server that can be made to
  // write anywhere is a dev server that will eventually write somewhere bad.
  if ((raw === "/routes/" || raw === "/routes") && req.method === "POST") {
    let body = "";
    req.on("data", (chunk) => {
      body += chunk;
      // A route is a few dozen points. Anything larger is not a route.
      if (body.length > 2_000_000) req.destroy();
    });
    req.on("end", async () => {
      try {
        const document_ = JSON.parse(body);
        const safe = String(document_.name || "route")
          .replace(/[^A-Za-z0-9_-]+/g, "-").slice(0, 60) || "route";
        await mkdir(routesDir, { recursive: true });
        const file = join(routesDir, `${safe}.json`);
        if (!file.startsWith(routesDir)) throw new Error("path escape");
        await writeFile(file, `${JSON.stringify(document_, null, 2)}
`, "utf-8");
        console.log(`200 POST /routes/ -> ${safe}.json (${(document_.nodes || []).length} nodes)`);
        res.writeHead(200, { "content-type": "application/json" })
          .end(JSON.stringify({ saved: `${safe}.json`, nodes: (document_.nodes || []).length }));
      } catch (error) {
        console.log(`400 POST /routes/ ${error.message}`);
        res.writeHead(400, { "content-type": "application/json" })
          .end(JSON.stringify({ error: String(error.message) }));
      }
    });
    return;
  }

  // POST /worlds/ writes one worldfile into the same directory the library lists and the
  // generator reads. **A download is not a save.** The button said "save world" and handed
  // the browser a file in the downloads folder, where the world library cannot list it and
  // the Python side cannot open it - so a painted world looked saved and was not anywhere
  // the rest of the tool could reach.
  //
  // The same rule the routes writer follows: one directory, one extension, a sanitised
  // name, and a size cap. A worldfile carries features and areas, so the cap is larger than
  // a route's - the ranger camp file is a thousand features and four areas.
  if ((raw === "/worlds/" || raw === "/worlds") && req.method === "POST") {
    let body = "";
    req.on("data", (chunk) => {
      body += chunk;
      if (body.length > 64_000_000) req.destroy();
    });
    req.on("end", async () => {
      try {
        const document_ = JSON.parse(body);
        const safe = String(document_.name || "world")
          .replace(/[^A-Za-z0-9_-]+/g, "-").slice(0, 60) || "world";
        await mkdir(worldsDir, { recursive: true });
        const file = join(worldsDir, `${safe}.json`);
        if (!file.startsWith(worldsDir)) throw new Error("path escape");
        await writeFile(file, `${JSON.stringify(document_, null, 2)}
`, "utf-8");
        const counts = {
          saved: `${safe}.json`,
          areas: (document_.areas || []).length,
          features: (document_.features || []).length,
        };
        console.log(`200 POST /worlds/ -> ${safe}.json `
          + `(${counts.areas} areas, ${counts.features} features)`);
        res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify(counts));
      } catch (error) {
        console.log(`400 POST /worlds/ ${error.message}`);
        res.writeHead(400, { "content-type": "application/json" })
          .end(JSON.stringify({ error: String(error.message) }));
      }
    });
    return;
  }

  // POST /generate/ starts a populate run and answers with its id.
  //
  // **The generator is Python and the viewer is a browser**, so something has to stand
  // between them. This spawns the runner, reads the run id off its first line of stdout -
  // which the runner flushes before it starts work, for exactly this - and answers with it
  // so the page can begin following `/progress/` while the run is still going.
  //
  // The process is deliberately NOT waited on. A hundred areas takes seconds and could take
  // minutes on a bigger count; holding the response open until it finished would make the
  // live feed pointless and would time out the fetch.
  if ((raw === "/generate/" || raw === "/generate") && req.method === "POST") {
    let body = "";
    req.on("data", (chunk) => { body += chunk; if (body.length > 100_000) req.destroy(); });
    req.on("end", () => {
      let request;
      try {
        request = JSON.parse(body || "{}");
      } catch (error) {
        res.writeHead(400, { "content-type": "application/json" })
           .end(JSON.stringify({ error: String(error.message) }));
        return;
      }
      const world = String(request.world || "").replace(/[^A-Za-z0-9_.-]/g, "");
      if (!world) {
        res.writeHead(400, { "content-type": "application/json" })
           .end('{"error":"world is required"}');
        return;
      }
      const count = Math.max(1, Math.min(500, Number(request.count) || 100));
      const label = String(request.label || "populate")
        .replace(/[^A-Za-z0-9_-]+/g, "-").slice(0, 40) || "populate";
      const args = [
        "-m", "evennia_roundtrip.generate",
        "--world", join(worldsDir, world),
        "--root", join(worldsDir, ".."),
        "--count", String(count),
        "--label", label,
      ];
      // `--region=-26.5,...` and not `--region -26.5,...`: a value beginning with a minus
      // is read by argparse as the next FLAG, and the runner exits 2 saying the option
      // expected an argument. Every region south of the equator starts with a minus.
      if (request.region) args.push(`--region=${request.region}`);
      if (request.sea) args.push(`--sea=${request.sea}`);
      const python = pythonForGenerator();
      const child = spawn(python, args, { cwd: join(worldsDir, ".."), windowsHide: true });
      let out = "";
      let answered = false;
      const fail = (why) => {
        if (answered) return;
        answered = true;
        console.log(`500 POST /generate/ ${why}`);
        res.writeHead(500, { "content-type": "application/json" })
           .end(JSON.stringify({ error: why }));
      };
      child.stdout.on("data", (chunk) => {
        out += chunk;
        const line = out.split("\n")[0];
        if (answered || !out.includes("\n")) return;
        try {
          const first = JSON.parse(line);
          if (!first.run_id) throw new Error("no run_id");
          answered = true;
          console.log(`200 POST /generate/ -> ${first.run_id} (${count} areas of ${world})`);
          res.writeHead(200, { "content-type": "application/json" })
             .end(JSON.stringify({ run_id: first.run_id, count, world }));
        } catch (error) {
          fail(`the generator's first line was not a run id: ${line.slice(0, 120)}`);
        }
      });
      let errors = "";
      child.stderr.on("data", (chunk) => { errors += chunk; });
      child.on("error", (error) => fail(String(error.message)));
      child.on("close", (code) => {
        if (code !== 0) console.log(`generator exited ${code}: ${errors.slice(-400)}`);
        if (code !== 0) fail(`the generator exited ${code}: ${errors.slice(-300)}`);
      });
    });
    return;
  }

  // GET /progress/?run=<id>&from=<n> - the populate feed.
  //
  // **Polling a growing file, not a socket.** A run writes one JSON line per area as it
  // lands; the viewer asks for everything past the line it has already drawn. That
  // survives a page reload, a viewer opened halfway through, and a generator that dies -
  // all of which a socket handles badly and a cursor handles for free.
  if (raw === "/progress/" || raw === "/progress") {
    const runId = (url.searchParams.get("run") || "").replace(/[^A-Za-z0-9_.-]/g, "");
    const from = Math.max(0, Number(url.searchParams.get("from") || 0) | 0);
    if (!runId) {
      res.writeHead(400, { "content-type": "application/json" })
         .end('{"error":"run is required"}');
      return;
    }
    const file = join(runsDir, runId, "progress.ndjson");
    if (!file.startsWith(runsDir)) {
      res.writeHead(400, { "content-type": "application/json" }).end('{"error":"bad run"}');
      return;
    }
    try {
      const text = await readFile(file, "utf8");
      const lines = text.split("\n").filter((l) => l.trim().length);
      const areas = lines.slice(from).map((l) => { try { return JSON.parse(l); }
                                                  catch { return null; } })
                         .filter(Boolean);
      // **The feed says when it is over, because a watcher cannot tell.** A run that
      // stops at eighty-three of a hundred looks exactly like a run still working: the
      // pins stop appearing and nothing says whether the generator is thinking or done.
      // The manifest already knows, so it travels with the areas.
      let status = null;
      let summary = null;
      try {
        const manifest = JSON.parse(
          await readFile(join(runsDir, runId, "manifest.json"), "utf8"));
        status = manifest.status || null;
        summary = manifest.summary || null;
      } catch {
        // No manifest yet means the run has only just begun. Not an error.
      }
      const body = JSON.stringify({ total: lines.length, from, areas, status, summary });
      res.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" })
         .end(body);
    } catch {
      // No file yet is not an error: the run may not have written its first area.
      res.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" })
         .end(JSON.stringify({ total: 0, from, areas: [] }));
    }
    return;
  }

  // GET /runs/ - what runs exist, so the viewer can offer the live one.
  if (raw === "/runs/" || raw === "/runs") {
    try {
      const names = (await readdir(runsDir, { withFileTypes: true }))
        .filter((d) => d.isDirectory()).map((d) => d.name).sort().reverse();
      res.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" })
         .end(JSON.stringify({ runs: names }));
    } catch {
      res.writeHead(200, { "content-type": "application/json" }).end('{"runs":[]}');
    }
    return;
  }

  // One file out of one run - the worldfile in practice, which is the only place the
  // ROOMS of a generated area live.
  //
  // **The progress feed carries counts, not contents, and deliberately so**: a hundred
  // areas of fifty rooms is five thousand descriptions and the viewer polls that file every
  // two seconds. But it means a pin drawn from the feed knows how many rooms an area has
  // and not where any of them are, so clicking one could never show its streets the way
  // the fish camp and the city show theirs. This is where the rooms come from when
  // somebody asks for them.
  if (raw.startsWith("/runs/") && req.method === "GET") {
    let rest = normalize(raw.slice("/runs/".length));
    while (rest.startsWith("..")) rest = rest.slice(2);
    const file = join(runsDir, rest);
    if (!file.startsWith(runsDir) || !file.endsWith(".json")) {
      res.writeHead(403, { "content-type": "text/plain" }).end("forbidden");
      return;
    }
    try {
      const body = await readFile(file);
      console.log(`200 GET ${req.url} (${body.length} bytes)`);
      res.writeHead(200, { "content-type": "application/json",
                           "content-length": body.length,
                           "cache-control": "no-store" }).end(body);
    } catch {
      console.log(`404 GET ${req.url}`);
      res.writeHead(404, { "content-type": "text/plain" }).end("not found");
    }
    return;
  }

  if (raw === "/routes/" || raw === "/routes") {
    try {
      const names = (await readdir(routesDir)).filter((n) => n.endsWith(".json"));
      const body = JSON.stringify({ routes: names }, null, 2);
      res.writeHead(200, { "content-type": "application/json",
                           "cache-control": "no-store" }).end(body);
    } catch {
      res.writeHead(200, { "content-type": "application/json" }).end('{"routes":[]}');
    }
    return;
  }

  // One saved route. Listing them and serving one are different requests and the second
  // was missing, so a river could be written and never read back.
  if (raw.startsWith("/routes/") && req.method === "GET") {
    let name = normalize(raw.slice("/routes/".length));
    while (name.startsWith("..")) name = name.slice(2);
    const file = join(routesDir, name);
    if (!file.startsWith(routesDir) || !file.endsWith(".json")) {
      res.writeHead(403).end("forbidden");
      return;
    }
    try {
      const body = await readFile(file);
      console.log(`200 GET ${req.url}`);
      res.writeHead(200, { "content-type": "application/json",
                           "content-length": body.length,
                           "cache-control": "no-store" }).end(body);
    } catch {
      console.log(`404 GET ${req.url}`);
      res.writeHead(404, { "content-type": "text/plain" }).end("not found");
    }
    return;
  }

  if (raw === "/worlds/" || raw === "/worlds") {
    try {
      const names = (await readdir(worldsDir)).filter((n) => n.endsWith(".json"));
      const rows = [];
      for (const name of names) {
        try {
          const document_ = JSON.parse(await readFile(join(worldsDir, name), "utf-8"));
          rows.push({
            file: name,
            name: document_.name || name.replace(/\.json$/, ""),
            areas: (document_.areas || []).length,
            saved_at: document_.saved_at || null,
            seed: (document_.planet || {}).seed || null,
          });
        } catch {
          // A world that will not parse is listed as unreadable rather than hidden. A library
          // that silently omits a file is a library you cannot trust to be complete.
          rows.push({ file: name, name, areas: null, saved_at: null, seed: null,
                      error: "will not parse" });
        }
      }
      rows.sort((a, b) => String(a.name).localeCompare(String(b.name)));
      const body = JSON.stringify({ worlds: rows }, null, 2);
      console.log(`200 ${req.method} ${req.url} (${rows.length} worlds)`);
      res.writeHead(200, {
        "content-type": "application/json",
        "content-length": Buffer.byteLength(body),
        "cache-control": "no-store",
        "content-security-policy": CSP,
        "cross-origin-resource-policy": "same-origin",
        "cross-origin-opener-policy": "same-origin",
        "cross-origin-embedder-policy": "require-corp",
      }).end(body);
    } catch {
      console.log(`404 ${req.method} ${req.url} (no worlds directory)`);
      res.writeHead(404, { "content-type": "application/json" })
        .end(JSON.stringify({ worlds: [], error: "no worlds directory" }));
    }
    return;
  }

  if (raw.startsWith("/worlds/")) {
    let name = normalize(raw.slice("/worlds/".length));
    while (name.startsWith("..")) name = name.slice(2);
    const file = join(worldsDir, name);
    if (!file.startsWith(worldsDir) || !file.endsWith(".json")) {
      res.writeHead(403).end("forbidden");
      return;
    }
    try {
      const body = await readFile(file);
      console.log(`200 ${req.method} ${req.url}`);
      res.writeHead(200, {
        "content-type": "application/json",
        "content-length": body.length,
        "cache-control": "no-store",
        "content-security-policy": CSP,
        "cross-origin-resource-policy": "same-origin",
        "cross-origin-opener-policy": "same-origin",
        "cross-origin-embedder-policy": "require-corp",
      }).end(body);
    } catch {
      console.log(`404 ${req.method} ${req.url}`);
      res.writeHead(404, { "content-type": "text/plain" }).end("not found");
    }
    return;
  }

  if (raw.endsWith("/")) raw += "index.html";
  let p = normalize(raw);
  while (p.startsWith("..")) p = p.slice(2);
  const file = join(root, p);
  if (!file.startsWith(root)) { res.writeHead(403).end("forbidden"); return; }
  try {
    const s = await stat(file);
    if (!s.isFile()) throw new Error("not a file");
    // `raw` and not `p`: `normalize` produces backslashes on Windows and the two `.html`
    // and `/` tests below are written against a URL path.
    const cache = cacheHeaders(raw, s);
    // The conditional request. `If-None-Match` may carry a list, and Chrome sends back
    // exactly the token this server issued, so a `split`/`trim` membership test is what
    // matches rather than string equality -- equality would silently never hit and the
    // saving would quietly not exist.
    const inm = req.headers["if-none-match"];
    if (cache.etag && inm && inm.split(",").some((t) => t.trim() === cache.etag)) {
      console.log(`304 ${req.method} ${req.url}`);
      // A 304 carries the validator and the caching policy and NO body and no
      // content-length. The security headers go with it because a 304 refreshes the stored
      // response's headers.
      res.writeHead(304, {
        ...cache,
        "content-security-policy": CSP,
        "cross-origin-resource-policy": "same-origin",
        "cross-origin-opener-policy": "same-origin",
        "cross-origin-embedder-policy": "require-corp",
      }).end();
      return;
    }
    console.log(`200 ${req.method} ${req.url}`);
    res.writeHead(200, {
      "content-type": TYPES[extname(file).toLowerCase()] || "application/octet-stream",
      "content-length": s.size,
      ...cache,
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
