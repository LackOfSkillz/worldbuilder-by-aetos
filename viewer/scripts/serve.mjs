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
      // Cross-origin isolation, for the SharedArrayBuffer worker pool in Task 5.
      "cross-origin-opener-policy": "same-origin",
      "cross-origin-embedder-policy": "require-corp",
      "cross-origin-resource-policy": "same-origin",
    });
    createReadStream(file).pipe(res);
  } catch {
    console.log(`404 ${req.method} ${req.url}`);
    res.writeHead(404, { "content-type": "text/plain" }).end("not found");
  }
}).listen(port, "127.0.0.1", () => console.log(`viewer: http://127.0.0.1:${port}/`));
