//! Regression coverage for `build_fingerprint.rs` (crate root, shared with `build.rs` via
//! `#[path]`) -- the walking and hashing logic behind `source_fingerprint()`.
//!
//! Runs against SYNTHETIC trees under a temp directory, never against this repo's own
//! `src`/`examples`/`tests`, for two reasons: the ground-truth digest this task exists to
//! reproduce depends on this repo's real files (and moves the moment this very test file is
//! added, since `tests/` is one of the fingerprinted directories), so asserting an exact
//! value here would go stale on the next unrelated edit; and the failure-path tests need to
//! delete or empty an input directory, which must never happen to the real tree a mutation
//! test runs alongside.
//!
//! Ungated: unlike `tests/wasm_exports.rs` (`--features wasm` only), this logic is
//! feature-independent, so it runs -- and is counted -- in all five configurations, the
//! same way `tests/no_std_math.rs` does.

#[path = "../build_fingerprint.rs"]
mod build_fingerprint;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// A fresh temp directory per test, named uniquely so parallel `cargo test` threads never
/// collide on the same path.
fn fresh_temp_root(label: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "wb-fingerprint-test-{label}-{}-{n}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp root");
    dir
}

/// Builds the minimal tree `fingerprint_inputs` requires under `root`:
/// `crates/worldbuilder-engine/{src,examples,tests}` each with one file, plus the three
/// named files. Mirrors the real repo's shape without touching it.
fn write_minimal_tree(root: &Path) {
    let crate_dir = root.join("crates").join("worldbuilder-engine");
    for sub in ["src", "examples", "tests"] {
        let dir = crate_dir.join(sub);
        fs::create_dir_all(&dir).expect("create subdir");
        fs::write(dir.join("placeholder.rs"), b"// placeholder\n").expect("write placeholder");
    }
    fs::write(crate_dir.join("Cargo.toml"), b"[package]\nname = \"x\"\n").expect("write crate Cargo.toml");
    fs::write(root.join("Cargo.toml"), b"[workspace]\n").expect("write workspace Cargo.toml");
    fs::write(root.join("Cargo.lock"), b"# lock\n").expect("write Cargo.lock");
}

#[test]
fn agrees_with_itself_on_an_unchanged_tree() {
    let root = fresh_temp_root("stable");
    write_minimal_tree(&root);

    let (digest_a, count_a) =
        build_fingerprint::source_fingerprint(&root, "rustc TEST", "build TEST").expect("first run");
    let (digest_b, count_b) =
        build_fingerprint::source_fingerprint(&root, "rustc TEST", "build TEST").expect("second run");

    assert_eq!(digest_a, digest_b, "the same tree must produce the same digest every time");
    assert_eq!(count_a, count_b);
    assert_eq!(count_a, 6, "3 walked files + 3 named files in the minimal tree");
    assert_eq!(digest_a.len(), 64);
    assert!(digest_a.chars().all(|c| c.is_ascii_hexdigit()));

    let _ = fs::remove_dir_all(&root);
}

/// The whole point: a fingerprint that cannot disagree is decoration. Flip one byte in one
/// fingerprinted file and the digest MUST move.
#[test]
fn fingerprint_moves_when_a_single_byte_changes() {
    let root = fresh_temp_root("mutate");
    write_minimal_tree(&root);

    let (before, _) =
        build_fingerprint::source_fingerprint(&root, "rustc TEST", "build TEST").expect("before");

    let mutated_file = root
        .join("crates")
        .join("worldbuilder-engine")
        .join("src")
        .join("placeholder.rs");
    fs::write(&mutated_file, b"// PLACEHOLDER (one byte flipped)\n").expect("mutate");

    let (after, _) =
        build_fingerprint::source_fingerprint(&root, "rustc TEST", "build TEST").expect("after");

    assert_ne!(before, after, "changing a fingerprinted file's bytes must move the digest");

    // And reverting brings it back -- the digest is a pure function of current content,
    // not something that accumulates state across calls.
    fs::write(&mutated_file, b"// placeholder\n").expect("revert");
    let (reverted, _) =
        build_fingerprint::source_fingerprint(&root, "rustc TEST", "build TEST").expect("reverted");
    assert_eq!(before, reverted, "reverting the byte must restore the original digest");

    let _ = fs::remove_dir_all(&root);
}

/// A toolchain or build-args change (a different host, a different `cargo` invocation)
/// must also move the digest -- both are baked into the digested body, not just the file
/// contents.
#[test]
fn fingerprint_moves_when_toolchain_or_build_args_change() {
    let root = fresh_temp_root("toolchain");
    write_minimal_tree(&root);

    let (base, _) =
        build_fingerprint::source_fingerprint(&root, "rustc TEST", "build TEST").expect("base");
    let (diff_toolchain, _) =
        build_fingerprint::source_fingerprint(&root, "rustc OTHER", "build TEST").expect("diff toolchain");
    let (diff_args, _) =
        build_fingerprint::source_fingerprint(&root, "rustc TEST", "build OTHER").expect("diff args");

    assert_ne!(base, diff_toolchain);
    assert_ne!(base, diff_args);

    let _ = fs::remove_dir_all(&root);
}

/// The empty-directory defence, mirroring the Node script's `walkNonEmpty`: an input
/// directory that exists but contributes zero files is an error, never a silently smaller
/// digest. Exercised against a synthetic tree, never by emptying a real directory.
#[test]
fn empty_input_directory_is_an_error_not_a_smaller_digest() {
    let root = fresh_temp_root("empty-dir");
    write_minimal_tree(&root);

    // Empty out examples/ (remove its only file, keep the directory itself).
    let examples_dir = root.join("crates").join("worldbuilder-engine").join("examples");
    fs::remove_file(examples_dir.join("placeholder.rs")).expect("empty examples/");

    let result = build_fingerprint::fingerprint_inputs(&root);
    let err = result.expect_err("an empty fingerprint input directory must be an error");
    assert!(
        err.contains("empty"),
        "error should say the directory is empty, got: {err}"
    );

    let _ = fs::remove_dir_all(&root);
}

/// A renamed (here: deleted) input directory must fail the same way an emptied one does --
/// the directory contributing zero files is what matters, not why.
#[test]
fn missing_input_directory_is_an_error() {
    let root = fresh_temp_root("missing-dir");
    write_minimal_tree(&root);

    let tests_dir = root.join("crates").join("worldbuilder-engine").join("tests");
    fs::remove_dir_all(&tests_dir).expect("remove tests/ entirely");

    let result = build_fingerprint::fingerprint_inputs(&root);
    assert!(result.is_err(), "a missing input directory must be an error, not a silent skip");

    let _ = fs::remove_dir_all(&root);
}

/// A directory nested one level deeper than expected -- an entirely empty subdirectory sitting
/// alongside a real file -- must not make the walk THINK the parent contributed nothing when
/// it plainly did; the defence is about zero files total, not zero direct children.
#[test]
fn a_nested_empty_subdirectory_does_not_trip_the_defence_when_files_exist_elsewhere() {
    let root = fresh_temp_root("nested-empty-ok");
    write_minimal_tree(&root);

    let src_dir = root.join("crates").join("worldbuilder-engine").join("src");
    fs::create_dir_all(src_dir.join("empty_subdir")).expect("create nested empty dir");

    let inputs = build_fingerprint::fingerprint_inputs(&root).expect("still fine: src/ has a file");
    assert_eq!(inputs.len(), 6);

    let _ = fs::remove_dir_all(&root);
}

/// One of the three named files (crate `Cargo.toml`, workspace `Cargo.toml`, `Cargo.lock`)
/// missing entirely must also be an error, matching the Node script's `existsSync` guard.
#[test]
fn missing_named_file_is_an_error() {
    let root = fresh_temp_root("missing-file");
    write_minimal_tree(&root);

    fs::remove_file(root.join("Cargo.lock")).expect("remove Cargo.lock");

    let result = build_fingerprint::fingerprint_inputs(&root);
    let err = result.expect_err("a missing named fingerprint input must be an error");
    assert!(err.contains("Cargo.lock"), "error should name the missing file, got: {err}");

    let _ = fs::remove_dir_all(&root);
}

/// Relative paths in the fingerprint use `/`, matching
/// `relative(root, f).split(sep).join("/")` on the Node side, regardless of the host path
/// separator -- so the digest a Windows box computes agrees with the digest a Linux box
/// would compute over byte-identical files.
#[test]
fn relative_paths_use_forward_slashes() {
    let root = fresh_temp_root("slashes");
    write_minimal_tree(&root);

    let inputs = build_fingerprint::fingerprint_inputs(&root).expect("fingerprint inputs");
    for input in &inputs {
        assert!(
            !input.rel.contains('\\'),
            "rel path {:?} must use '/' separators, never '\\\\'",
            input.rel
        );
    }
    // And it must actually have picked up a nested path, not just top-level files.
    assert!(
        inputs.iter().any(|i| i.rel == "crates/worldbuilder-engine/src/placeholder.rs"),
        "expected the nested src file among the inputs: {:?}",
        inputs.iter().map(|i| &i.rel).collect::<Vec<_>>()
    );

    let _ = fs::remove_dir_all(&root);
}

/// Inputs are sorted on the repo-relative path, not on walk/discovery order -- proven by
/// checking the returned list is itself sorted (walk order for this tree already happens to
/// differ: `Cargo.lock`/`Cargo.toml` are appended after the walked directories, and
/// `crates/...` sorts before both alphabetically).
#[test]
fn inputs_are_sorted_by_relative_path() {
    let root = fresh_temp_root("sorted");
    write_minimal_tree(&root);

    let inputs = build_fingerprint::fingerprint_inputs(&root).expect("fingerprint inputs");
    let rels: Vec<&str> = inputs.iter().map(|i| i.rel.as_str()).collect();
    let mut sorted = rels.clone();
    sorted.sort();
    assert_eq!(rels, sorted, "inputs must come back sorted by repo-relative path");
    // The two workspace-root files, appended last by write order, must have sorted ahead
    // of everything under crates/ -- proving this isn't accidentally already-sorted input.
    assert_eq!(rels[0], "Cargo.lock");
    assert_eq!(rels[1], "Cargo.toml");

    let _ = fs::remove_dir_all(&root);
}
