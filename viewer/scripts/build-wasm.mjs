// Builds the worldbuilder-engine wasm32-unknown-unknown artifact, ASSERTS what it
// contains, and copies it where the page can fetch it.
//
// The one thing this script must not get wrong: a green `cargo build` proves nothing.
// The first artifact in this project was 327 bytes exporting only `memory`, because a
// `cdylib` discards every module when nothing is `#[no_mangle] extern "C"`. So this
// script does not trust the exit code -- it hand-parses the built module's import
// (section id 2) and export (section id 7) sections, cross-checks that against Node's
// own `WebAssembly.Module.exports/imports`, and cross-checks the expected export names
// against the `pub extern "C" fn` declarations in wasm.rs itself (not a hardcoded list
// that could drift from the source). Any disagreement, or a size/shape that looks like
// the empty build, fails the script.
//
// No wasm-bindgen, no wasm-opt, no bundler. The module has zero imports by design;
// `WebAssembly.instantiate(bytes, {})` is the entire loader.

import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync, mkdirSync, existsSync, rmSync, copyFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const viewerDir = join(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = join(viewerDir, "..");
const crateDir = join(repoRoot, "crates", "worldbuilder-engine");
const wasmSrc = join(crateDir, "src", "wasm.rs");
const builtArtifact = join(repoRoot, "target", "wasm32-unknown-unknown", "release", "worldbuilder_engine.wasm");
const destDir = join(viewerDir, "public", "wasm");
const destArtifact = join(destDir, "worldbuilder_engine.wasm");

const MIN_PLAUSIBLE_BYTES = 20_000; // the empty build was 327 bytes; a real one is ~85 KB.

function findCargo() {
  const candidates = [
    process.env.CARGO,
    "cargo",
    "C:\\Users\\gary\\.cargo\\bin\\cargo.exe",
    "/c/Users/gary/.cargo/bin/cargo.exe",
  ].filter(Boolean);
  for (const c of candidates) {
    const r = spawnSync(c, ["--version"], { stdio: "pipe" });
    if (r.status === 0) return c;
  }
  throw new Error("No working cargo found. Tried: " + candidates.join(", "));
}

function runCargoBuild(cargo, extraArgs = []) {
  const args = [
    "build",
    "-p", "worldbuilder-engine",
    "--release",
    "--target", "wasm32-unknown-unknown",
    "--no-default-features",
    ...extraArgs,
  ];
  console.log(`> ${cargo} ${args.join(" ")}`);
  const r = spawnSync(cargo, args, { cwd: repoRoot, stdio: "inherit" });
  if (r.status !== 0) {
    throw new Error(`cargo build exited ${r.status}`);
  }
}

// ---- minimal hand-rolled wasm binary parser (section 2 = import, section 7 = export) ----

function readVarUint(buf, offset) {
  let result = 0n;
  let shift = 0n;
  let pos = offset;
  for (;;) {
    const byte = buf[pos++];
    result |= BigInt(byte & 0x7f) << shift;
    if ((byte & 0x80) === 0) break;
    shift += 7n;
  }
  return { value: Number(result), next: pos };
}

function readName(buf, offset) {
  const { value: len, next } = readVarUint(buf, offset);
  const bytes = buf.subarray(next, next + len);
  return { name: bytes.toString("utf8"), next: next + len };
}

function parseWasmSections(buf) {
  if (buf.length < 8 || buf.readUInt32LE(0) !== 0x6d736100) {
    throw new Error("not a wasm module (bad magic)");
  }
  let offset = 8; // magic (4) + version (4)
  // A module section is entirely absent when its vector is empty, not present-with-0 --
  // that is why this module (0 imports by design) has no section id 2 at all. So the
  // absence of the section means 0, and exports default to [] for the same reason.
  let importCount = 0;
  let exportNames = [];
  while (offset < buf.length) {
    const sectionId = buf[offset];
    const { value: sectionLen, next: afterLen } = readVarUint(buf, offset + 1);
    const sectionStart = afterLen;
    const sectionEnd = sectionStart + sectionLen;
    if (sectionId === 2) {
      // import section: vector of imports
      const { value: count } = readVarUint(buf, sectionStart);
      importCount = count;
    } else if (sectionId === 7) {
      // export section: vector of (name, kind byte, index varuint)
      let { value: count, next } = readVarUint(buf, sectionStart);
      const names = [];
      for (let i = 0; i < count; i++) {
        const { name, next: afterName } = readName(buf, next);
        const kind = buf[afterName];
        const { next: afterIdx } = readVarUint(buf, afterName + 1);
        names.push(name);
        next = afterIdx;
      }
      exportNames = names;
    }
    offset = sectionEnd;
  }
  return { importCount, exportNames };
}

// ---- expected export set, derived from the source, not hardcoded ----

function expectedExportNamesFromSource() {
  const src = readFileSync(wasmSrc, "utf8");
  const re = /pub extern "C" fn (\w+)/g;
  const names = ["memory"];
  let m;
  while ((m = re.exec(src)) !== null) names.push(m[1]);
  return names.sort();
}

// ---- the assertion itself ----

function assertArtifact(path, { allowEmpty = false } = {}) {
  const buf = readFileSync(path);
  const size = buf.length;

  const { importCount: handImports, exportNames: handExportsRaw } = parseWasmSections(buf);
  const handExports = [...handExportsRaw].sort();

  const mod = new WebAssembly.Module(buf);
  const jsExports = WebAssembly.Module.exports(mod).map((e) => e.name).sort();
  const jsImports = WebAssembly.Module.imports(mod);

  const problems = [];

  if (JSON.stringify(handExports) !== JSON.stringify(jsExports)) {
    problems.push(
      `hand-parsed export list disagrees with WebAssembly.Module.exports:\n  hand: ${JSON.stringify(handExports)}\n  js:   ${JSON.stringify(jsExports)}`
    );
  }
  if (handImports !== jsImports.length) {
    problems.push(`hand-parsed import count (${handImports}) disagrees with WebAssembly.Module.imports (${jsImports.length})`);
  }

  if (!allowEmpty) {
    if (size < MIN_PLAUSIBLE_BYTES) {
      problems.push(`artifact is only ${size} bytes -- looks like the empty build (327 bytes, memory-only). Expected >= ${MIN_PLAUSIBLE_BYTES}.`);
    }
    if (jsImports.length !== 0) {
      problems.push(`artifact has ${jsImports.length} imports; this module is supposed to have zero imports.`);
    }
    const expected = expectedExportNamesFromSource();
    if (JSON.stringify(jsExports) !== JSON.stringify(expected)) {
      problems.push(
        `export set does not match the pub extern "C" fn declarations in wasm.rs:\n  built:    ${JSON.stringify(jsExports)}\n  expected: ${JSON.stringify(expected)}`
      );
    }
  }

  return { size, exports: jsExports, imports: jsImports.length, problems };
}

function main() {
  const mode = process.argv[2] || "build";

  if (mode === "self-test") {
    // Prove the assertion can actually fail: build WITHOUT the wasm feature, which
    // recompiles wasm.rs out of the module entirely and reproduces the historical
    // 327-byte, memory-only, cdylib-discards-everything failure mode.
    console.log("== self-test: building a deliberately broken (feature-less) artifact ==");
    const cargo = findCargo();
    if (existsSync(builtArtifact)) rmSync(builtArtifact);
    runCargoBuild(cargo, []); // no --features wasm
    const result = assertArtifact(builtArtifact, { allowEmpty: true });
    console.log(`broken artifact: ${result.size} bytes, exports=${JSON.stringify(result.exports)}, imports=${result.imports}`);
    // Now run it through the REAL assertion (allowEmpty: false) and confirm it is rejected.
    const strict = assertArtifact(builtArtifact, { allowEmpty: false });
    if (strict.problems.length === 0) {
      console.error("SELF-TEST FAILED: the broken artifact passed the strict assertion. The check does not work.");
      process.exit(1);
    }
    console.log("SELF-TEST PASSED: the broken artifact was correctly rejected:");
    for (const p of strict.problems) console.log(`  - ${p}`);
    console.log("\nRebuilding the real artifact with --features wasm to leave the tree in a good state...");
    if (existsSync(builtArtifact)) rmSync(builtArtifact);
    runCargoBuild(cargo, ["--features", "wasm"]);
    const real = assertArtifact(builtArtifact, { allowEmpty: false });
    if (real.problems.length !== 0) {
      console.error("Rebuilding the real artifact failed its own assertion:");
      for (const p of real.problems) console.error(`  - ${p}`);
      process.exit(1);
    }
    console.log(`Real artifact restored: ${real.size} bytes, ${real.exports.length} exports, ${real.imports} imports.`);
    return;
  }

  // Normal path: build the real thing and assert it.
  const cargo = findCargo();
  if (existsSync(builtArtifact)) rmSync(builtArtifact); // so this run cannot be a stale no-op
  runCargoBuild(cargo, ["--features", "wasm"]);

  if (!existsSync(builtArtifact)) {
    throw new Error(`cargo reported success but no artifact was found at ${builtArtifact}`);
  }

  const result = assertArtifact(builtArtifact, { allowEmpty: false });
  if (result.problems.length !== 0) {
    console.error("ARTIFACT VERIFICATION FAILED:");
    for (const p of result.problems) console.error(`  - ${p}`);
    process.exit(1);
  }

  console.log(`Verified: ${result.size} bytes, ${result.exports.length} exports (${result.exports.join(", ")}), ${result.imports} imports.`);

  mkdirSync(destDir, { recursive: true });
  copyFileSync(builtArtifact, destArtifact);
  writeFileSync(
    join(destDir, "MANIFEST.txt"),
    `worldbuilder_engine.wasm\n` +
      `bytes: ${result.size}\n` +
      `exports (${result.exports.length}): ${result.exports.join(", ")}\n` +
      `imports: ${result.imports}\n` +
      `built: ${new Date().toISOString()}\n`
  );
  console.log(`Copied to ${destArtifact}`);
}

main();
