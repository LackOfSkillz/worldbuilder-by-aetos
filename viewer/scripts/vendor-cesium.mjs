// Copy the CesiumJS release build out of node_modules into the served, committed
// vendor tree. Run after `npm ci`. The copy is byte-for-byte: re-running this
// script and finding no `git diff` is the check that the vendored tree still
// matches the version pinned in package-lock.json.
import { cp, rm, mkdir, readFile, writeFile, readdir, stat } from "node:fs/promises";
import { createHash } from "node:crypto";
import { join, dirname, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const pkgDir = join(root, "node_modules", "cesium");
const dest = join(root, "public", "vendor", "cesium");

const pkg = JSON.parse(await readFile(join(pkgDir, "package.json"), "utf8"));
console.log(`vendoring cesium@${pkg.version} (${pkg.license})`);

await rm(dest, { recursive: true, force: true });
await mkdir(dest, { recursive: true });
await cp(join(pkgDir, "Build", "Cesium"), dest, { recursive: true });
// Licence and attribution travel with the code.
for (const f of ["LICENSE.md", "ThirdParty.json", "ThirdParty.extra.json"]) {
  await cp(join(pkgDir, f), join(dest, f));
}

async function* walk(dir) {
  for (const e of await readdir(dir, { withFileTypes: true })) {
    const p = join(dir, e.name);
    if (e.isDirectory()) yield* walk(p);
    else yield p;
  }
}

const lines = [];
let bytes = 0;
const files = [];
for await (const p of walk(dest)) files.push(p);
files.sort();
for (const p of files) {
  const buf = await readFile(p);
  bytes += buf.length;
  lines.push(`${createHash("sha256").update(buf).digest("hex")}  ${relative(dest, p).split(sep).join("/")}`);
}
await writeFile(
  join(root, "cesium-manifest.txt"),
  `# cesium@${pkg.version}  license=${pkg.license}\n` +
    `# ${files.length} files, ${bytes} bytes, vendored from node_modules/cesium/Build/Cesium\n` +
    `# regenerate: npm ci && npm run vendor\n` +
    lines.join("\n") +
    "\n",
);
console.log(`${files.length} files, ${bytes} bytes -> public/vendor/cesium`);
