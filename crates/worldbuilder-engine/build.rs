//! Gives the extension the same source fingerprint the shipped `.wasm` already carries
//! (`viewer/public/wasm/MANIFEST.txt`'s `source-fingerprint` field, computed by
//! `viewer/scripts/build-wasm.mjs`'s `sourceFingerprint`/`fingerprintInputs`). Task 3 will
//! compare the two; this task only makes the Rust side computable and exposes it.
//!
//! Runs in all five feature configurations this crate builds in (`--no-default-features`,
//! default, `--features python`, `--features wasm`, `--features python,wasm`) -- a
//! `build.rs` always runs on the HOST regardless of `--target`, so the
//! `wasm32-unknown-unknown` build runs this same code path and has full filesystem access
//! to do it, even though the resulting cdylib will not.
//!
//! The shared walking/hashing logic lives in `build_fingerprint.rs`, a crate-ROOT file
//! (sibling of this one), included with `#[path]` rather than placed under `src/`. That
//! placement is load-bearing, not tidiness:
//!
//! **Ruling: `build.rs` (and this file) stay OUTSIDE the fingerprint inputs.** They sit at
//! the crate root, which is in none of the three directories `fingerprintInputs` walks
//! (`src/`, `examples/`, `tests/`) and none of the three named files it reads
//! (`crates/worldbuilder-engine/Cargo.toml`, the workspace `Cargo.toml`, `Cargo.lock`). If
//! this file's own bytes were folded into the digest it computes, the Rust digest would
//! depend on a file the Node digest structurally cannot see -- any two-implementation
//! comparison gate (Task 3) would then fail permanently, by construction, the moment a
//! comment in this file changed. Known limitation for Task 5: a `build.rs` that DID want to
//! audit its own bytes would need the Node side to grow a matching input, which is out of
//! scope here and is not this task's problem to solve.
//!
//! **Ruling: a build.rs that cannot find the repo root must fail loudly, not fingerprint
//! nothing.** A build script that silently fell back to "no inputs" or "empty digest" on a
//! missing `Cargo.lock` would produce a stable, wrong, confident answer -- exactly the
//! silent-zero failure class this whole mechanism exists to catch elsewhere. `find_repo_root`
//! below panics (which fails the build) rather than guessing.

#[path = "build_fingerprint.rs"]
mod build_fingerprint;

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The exact cargo arguments `viewer/scripts/build-wasm.mjs`'s `BUILD_ARGS` records for the
/// shipped `.wasm`. This is baked into the digested body verbatim and does NOT vary with
/// how this particular `cargo build`/`cargo test` invocation was actually run: the ground
/// -truth digest in `MANIFEST.txt` was computed once, by the Node script, describing that
/// one recipe, and matching it requires the same fixed string every time -- not a
/// description of whichever feature flags happened to build this crate today.
const BUILD_ARGS: &str = "build -p worldbuilder-engine --release --target wasm32-unknown-unknown --no-default-features --features wasm";

/// `CARGO_MANIFEST_DIR` is `<repo root>/crates/worldbuilder-engine`; the repo root is two
/// levels up. Verified, not assumed: a workspace `Cargo.toml` and a `Cargo.lock` must both
/// exist there, or this is not the tree `fingerprintInputs` expects and guessing further
/// would be the exact silent-wrong-answer failure this build script must not produce.
fn find_repo_root() -> PathBuf {
    let manifest_dir = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is always set for a build script"),
    );
    let root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| {
            panic!(
                "cannot walk up from CARGO_MANIFEST_DIR ({}) to a repo root",
                manifest_dir.display()
            )
        })
        .to_path_buf();
    if !root.join("Cargo.toml").is_file() || !root.join("Cargo.lock").is_file() {
        panic!(
            "refusing to fingerprint: {} does not look like the workspace root \
             (no Cargo.toml/Cargo.lock there). CARGO_MANIFEST_DIR was {}.",
            root.display(),
            manifest_dir.display()
        );
    }
    root
}

/// `rustc -vV`, parsed for the three fields `viewer/scripts/build-wasm.mjs`'s
/// `toolchainId` folds into the fingerprint. Never falls back to a guessed or partial
/// string: a fingerprint that silently omitted the compiler would call an artifact built by
/// a different rustc "current", which is the failure `toolchainId` itself refuses to make.
fn toolchain_id() -> String {
    let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let output = Command::new(&rustc)
        .arg("-vV")
        .output()
        .unwrap_or_else(|e| panic!("could not run `{rustc} -vV`: {e}"));
    if !output.status.success() {
        panic!("`{rustc} -vV` exited with {}", output.status);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let field = |name: &str| -> String {
        stdout
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .unwrap_or_else(|| {
                panic!("`{rustc} -vV` output had no `{name}` line:\n{stdout}")
            })
            .trim()
            .to_string()
    };
    format!(
        "rustc {} {} {}",
        field("release: "),
        field("commit-hash: "),
        field("host: "),
    )
}

fn main() {
    let root = find_repo_root();
    let toolchain = toolchain_id();
    let (digest, input_count) = build_fingerprint::source_fingerprint(&root, &toolchain, BUILD_ARGS)
        .unwrap_or_else(|e| panic!("source fingerprint failed: {e}"));

    println!("cargo:rustc-env=WORLDBUILDER_SOURCE_FINGERPRINT={digest}");
    println!("cargo:rustc-env=WORLDBUILDER_SOURCE_FINGERPRINT_INPUTS={input_count}");

    // Rerun on any change to ANY of the 28-or-so fingerprinted inputs, named explicitly and
    // individually. This is NOT belt-and-suspenders: the moment a build script emits even
    // one `cargo:rerun-if-changed` line, Cargo drops its default "rerun if anything under
    // the crate directory changes" heuristic entirely and reruns the script ONLY for the
    // paths named -- there is no way to keep the default AND add specific paths. A first
    // draft of this file emitted `rerun-if-changed` for just the two workspace-level files
    // (Cargo.toml, Cargo.lock) on the reasoning that "the crate directory is covered by the
    // default anyway" -- and that reasoning is exactly backwards once any explicit line
    // exists. Caught by mutating a byte in src/vectors.rs, rebuilding twice: the fingerprint
    // moved on the first rebuild, then STAYED at the mutated value after reverting the byte
    // and rebuilding again, because Cargo saw none of ITS watched paths change and replayed
    // the cached (now-wrong) `cargo:rustc-env` output instead of rerunning this script. A
    // fingerprint that can silently serve a stale answer is worse than no fingerprint.
    for input in build_fingerprint::fingerprint_inputs(&root)
        .unwrap_or_else(|e| panic!("source fingerprint failed while listing rerun paths: {e}"))
    {
        println!("cargo:rerun-if-changed={}", input.full.display());
    }

    // Naming the files individually covers a CHANGED input and misses an ADDED one. A file
    // appearing under any of the three walked directories is a new fingerprint input, but it
    // is not on the watch list precisely because it did not exist when the list was built, so
    // Cargo replays the cached output and the digest silently stays at its old value while
    // the node script would already have moved. Demonstrated by adding `src/reviewer_probe.txt`:
    // the rebuild finished in 0.04 s with the digest and the input count both unmoved.
    //
    // Watching the directories themselves closes it -- a directory's mtime moves when an
    // entry is created or removed, which is exactly the event the file list cannot see. Both
    // forms are needed: directories alone would miss an edit that does not touch the mtime.
    for dir in ["src", "examples", "tests"] {
        println!(
            "cargo:rerun-if-changed={}",
            root.join("crates").join("worldbuilder-engine").join(dir).display()
        );
    }
}
