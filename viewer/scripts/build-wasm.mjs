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
import {
  readFileSync, writeFileSync, mkdirSync, existsSync, rmSync, copyFileSync,
  readdirSync, cpSync, mkdtempSync, appendFileSync,
} from "node:fs";
import { join, dirname, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";

const viewerDir = join(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = join(viewerDir, "..");
const crateDir = join(repoRoot, "crates", "worldbuilder-engine");
const wasmSrc = join(crateDir, "src", "wasm.rs");
const builtArtifact = join(repoRoot, "target", "wasm32-unknown-unknown", "release", "worldbuilder_engine.wasm");
const destDir = join(viewerDir, "public", "wasm");
const destArtifact = join(destDir, "worldbuilder_engine.wasm");

const MIN_PLAUSIBLE_BYTES = 20_000; // the empty build was 327 bytes; a real one is ~85 KB.

const MANIFEST = join(destDir, "MANIFEST.txt");

/// The exact cargo arguments the real build uses. Part of the staleness fingerprint,
/// because the same source built with a different feature set is a different artifact.
const BUILD_ARGS = [
  "build", "-p", "worldbuilder-engine", "--release",
  "--target", "wasm32-unknown-unknown", "--no-default-features", "--features", "wasm",
];

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

// =======================================================================================
// THE STALENESS GUARD
//
// Everything above proves things about the artifact's *shape*, and the parity harness
// (crates/worldbuilder-engine/parity) proves the shipped bytes agree with native source
// to the bit. Neither asks the one remaining question: were these bytes built from the
// source that is here now?
//
// That gap is not a defect on its own, and neither is the parity harness's silence about
// provenance. Together they are: a stale .wasm passes parity green forever while the
// source moves underneath it, because the corpus it is replayed against was recorded from
// the same stale build.
//
// This is not hypothetical. When this guard was written the committed artifact WAS stale,
// and had been since commit d0c2eff: five panic-location records inside it named lines of
// wasm.rs that had shifted by +11 and +28 -- exactly the net +28 lines d0c2eff added to
// that file, in a commit that landed AFTER 0562500 committed the artifact. The stale
// bytes still passed parity, because line numbers in panic metadata never execute.
//
// The guard is a content hash over every input that can change the artifact, recorded in
// MANIFEST.txt at build time and re-checkable at any time:
//
//   * every file under crates/worldbuilder-engine/src, recursively;
//   * crates/worldbuilder-engine/Cargo.toml, the workspace Cargo.toml, Cargo.lock;
//   * the compiler version;
//   * the literal cargo argument list.
//
// Deliberately over-inclusive: bindings.rs and src/bin/ cannot affect a
// `--no-default-features --features wasm` build, and editing them will still trip this.
// A false "rebuild it" is a cheap failure; a false "it is current" is the one that costs.
//
// A hash over inputs is only sound if the artifact is a function of those inputs. That
// was checked, not assumed: two consecutive rebuilds of identical source produced
// byte-identical artifacts (sha256 1395f246...), while the committed one differed
// (f2a42266...) for the reason above -- older source, not a nondeterministic build.
// The artifact's own sha256 is recorded too, so a hand-edited or swapped .wasm is caught
// by the same command.

function sha256(buf) {
  return createHash("sha256").update(buf).digest("hex");
}

/// Every fingerprint input, as repo-relative paths, sorted. `root` is a parameter so the
/// self-test can run the real function against a mutated COPY of the tree rather than
/// against a mutated repo.
function fingerprintInputs(root) {
  const files = [];
  const walk = (dir) => {
    const entries = readdirSync(dir, { withFileTypes: true });
    entries.sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
    for (const entry of entries) {
      const full = join(dir, entry.name);
      if (entry.isDirectory()) walk(full);
      else if (entry.isFile()) files.push(full);
    }
  };
  walk(join(root, "crates", "worldbuilder-engine", "src"));
  for (const extra of [
    join(root, "crates", "worldbuilder-engine", "Cargo.toml"),
    join(root, "Cargo.toml"),
    join(root, "Cargo.lock"),
  ]) {
    if (!existsSync(extra)) throw new Error(`fingerprint input missing: ${extra}`);
    files.push(extra);
  }
  // Sort on the repo-relative path with a stable separator, so the digest depends on the
  // inputs' content and not on where the tree sits or which OS walked it.
  return files
    .map((f) => ({ rel: relative(root, f).split(sep).join("/"), full: f }))
    .sort((a, b) => (a.rel < b.rel ? -1 : a.rel > b.rel ? 1 : 0));
}

function toolchainId(cargo) {
  const dir = dirname(cargo);
  for (const candidate of [join(dir, "rustc.exe"), join(dir, "rustc"), "rustc"]) {
    const r = spawnSync(candidate, ["-vV"], { stdio: "pipe" });
    if (r.status === 0) {
      const out = r.stdout.toString();
      const release = /^release: (.+)$/m.exec(out);
      const commit = /^commit-hash: (.+)$/m.exec(out);
      const host = /^host: (.+)$/m.exec(out);
      if (release && commit && host) {
        return `rustc ${release[1].trim()} ${commit[1].trim()} ${host[1].trim()}`;
      }
    }
  }
  // Never guess. A fingerprint that silently omitted the compiler would call an
  // artifact built by a different rustc "current".
  throw new Error(`could not determine the rustc version next to ${cargo}; refusing to fingerprint without it`);
}

function sourceFingerprint(root, toolchain) {
  const inputs = fingerprintInputs(root);
  const lines = inputs.map(({ rel, full }) => `${sha256(readFileSync(full))}  ${rel}`);
  const body =
    `toolchain: ${toolchain}\n` +
    `build: cargo ${BUILD_ARGS.join(" ")}\n` +
    lines.join("\n") + "\n";
  return { digest: sha256(Buffer.from(body, "utf8")), inputs, lines };
}

function readManifestField(text, key) {
  const m = new RegExp(`^${key}: (.+)$`, "m").exec(text);
  return m ? m[1].trim() : null;
}

/// Re-check the SHIPPED artifact against the source that is here now. Returns a list of
/// problems; empty means current.
function checkFreshness() {
  if (!existsSync(destArtifact)) return [`no shipped artifact at ${destArtifact}`];
  if (!existsSync(MANIFEST)) return [`no ${MANIFEST}; rebuild with \`npm run build:wasm\``];

  const manifest = readFileSync(MANIFEST, "utf8");
  const recordedArtifact = readManifestField(manifest, "artifact-sha256");
  const recordedSource = readManifestField(manifest, "source-fingerprint");
  const recordedToolchain = readManifestField(manifest, "toolchain");

  if (!recordedSource || !recordedArtifact) {
    return [
      `${MANIFEST} predates the staleness guard (no source-fingerprint / artifact-sha256),` +
        ` so nothing can vouch for the shipped bytes' provenance. Rebuild with \`npm run build:wasm\`.`,
    ];
  }

  const problems = [];
  const actualArtifact = sha256(readFileSync(destArtifact));
  if (actualArtifact !== recordedArtifact) {
    problems.push(
      `the shipped .wasm is not the one this manifest describes:\n` +
        `      on disk:  ${actualArtifact}\n` +
        `      manifest: ${recordedArtifact}`
    );
  }

  const toolchain = toolchainId(findCargo());
  const { digest, lines } = sourceFingerprint(repoRoot, toolchain);
  if (digest !== recordedSource) {
    const detail = [
      `the shipped .wasm was NOT built from the source that is here now:`,
      `      source now:          ${digest}`,
      `      artifact built from: ${recordedSource}`,
    ];
    if (recordedToolchain && recordedToolchain !== toolchain) {
      detail.push(`      toolchain changed:   "${recordedToolchain}" -> "${toolchain}"`);
    }
    detail.push(`    Rebuild with \`npm run build:wasm\`. Until then the parity harness is`);
    detail.push(`    comparing a stale artifact against a corpus recorded from that same stale`);
    detail.push(`    build, and will stay green while proving nothing about current source.`);
    detail.push(`    (${lines.length} inputs fingerprinted.)`);
    problems.push(detail.join("\n"));
  }
  return problems;
}

/// Prove the guard can refuse, without touching crates/: copy the fingerprint inputs into
/// a temp tree, change one line of one file there, and confirm the real fingerprint
/// function reports a different digest.
///
/// This is the part that can run unattended. The whole proof also includes the on-disk
/// version -- edit wasm.rs for real, run `npm run check:wasm`, watch it fail -- which is
/// recorded in README.md.
function staleSelfTest() {
  const toolchain = toolchainId(findCargo());
  const before = sourceFingerprint(repoRoot, toolchain).digest;
  console.log(`repository fingerprint:                   ${before}`);

  const tmp = mkdtempSync(join(tmpdir(), "wb-stale-"));
  try {
    cpSync(join(repoRoot, "crates"), join(tmp, "crates"), { recursive: true });
    copyFileSync(join(repoRoot, "Cargo.toml"), join(tmp, "Cargo.toml"));
    copyFileSync(join(repoRoot, "Cargo.lock"), join(tmp, "Cargo.lock"));

    const copied = sourceFingerprint(tmp, toolchain).digest;
    if (copied !== before) {
      console.error("SELF-TEST FAILED: an unmodified copy of the tree fingerprinted differently.");
      console.error(`  repo: ${before}`);
      console.error(`  copy: ${copied}`);
      console.error("  The fingerprint depends on something other than the inputs' content.");
      process.exit(1);
    }
    console.log(`unmodified copy fingerprints identically: ${copied}`);

    // The mutation: one comment line appended to wasm.rs. It changes no behaviour and
    // moves no instruction -- but it moves every panic line number below it, which is
    // exactly the drift that made the real committed artifact stale.
    const victim = join(tmp, "crates", "worldbuilder-engine", "src", "wasm.rs");
    appendFileSync(victim, "\n// staleness self-test: one line the shipped artifact never saw.\n");
    const after = sourceFingerprint(tmp, toolchain).digest;
    if (after === before) {
      console.error("SELF-TEST FAILED: appending a line to wasm.rs did not change the fingerprint.");
      console.error("  The guard cannot notice a stale artifact. It is decoration.");
      process.exit(1);
    }
    console.log(`after one appended line:                  ${after}`);
    console.log("SELF-TEST PASSED: the fingerprint refuses a source tree that has moved.");
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
}

function main() {
  const mode = process.argv[2] || "build";

  if (mode === "check") {
    // Ask only the provenance question, and ask it without cargo touching the tree:
    // are the shipped bytes the ones current source produces?
    const problems = checkFreshness();
    if (problems.length !== 0) {
      console.error("STALE ARTIFACT:");
      for (const p of problems) console.error(`  - ${p}`);
      process.exit(1);
    }
    console.log(`Current: ${destArtifact} matches its manifest and the source that is here now.`);
    return;
  }

  if (mode === "stale-self-test") {
    console.log("== self-test: can the staleness fingerprint refuse a source tree that moved? ==");
    staleSelfTest();
    return;
  }

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

  // Provenance, recorded at the only moment it is knowable: the sha256 of the bytes just
  // shipped, and the fingerprint of the source they were built from. `npm run check:wasm`
  // recomputes both later. Without these two lines, nothing downstream -- including the
  // parity harness, which replays a corpus into exactly these bytes -- can tell a current
  // artifact from one built several commits ago.
  const toolchain = toolchainId(cargo);
  const { digest: sourceDigest, lines: inputLines } = sourceFingerprint(repoRoot, toolchain);
  const artifactDigest = sha256(readFileSync(destArtifact));

  writeFileSync(
    MANIFEST,
    `worldbuilder_engine.wasm\n` +
      `bytes: ${result.size}\n` +
      `exports (${result.exports.length}): ${result.exports.join(", ")}\n` +
      `imports: ${result.imports}\n` +
      `built: ${new Date().toISOString()}\n` +
      `artifact-sha256: ${artifactDigest}\n` +
      `source-fingerprint: ${sourceDigest}\n` +
      `toolchain: ${toolchain}\n` +
      `build: cargo ${BUILD_ARGS.join(" ")}\n` +
      `fingerprint-inputs: ${inputLines.length}\n`
  );
  console.log(`Copied to ${destArtifact}`);
  console.log(`artifact-sha256:    ${artifactDigest}`);
  console.log(`source-fingerprint: ${sourceDigest} (${inputLines.length} inputs)`);

  // The build has no excuse for leaving a tree its own guard would reject.
  const stale = checkFreshness();
  if (stale.length !== 0) {
    console.error("BUILD FAILED ITS OWN STALENESS GUARD -- the guard and the build disagree:");
    for (const p of stale) console.error(`  - ${p}`);
    process.exit(1);
  }
}

main();
