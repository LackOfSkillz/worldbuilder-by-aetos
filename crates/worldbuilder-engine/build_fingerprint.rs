//! Shared between `build.rs` (which runs this at compile time in all five feature
//! configurations, including the `wasm32-unknown-unknown` target build) and
//! `tests/build_fingerprint.rs` (which pulls this file in with `#[path]` so the same code
//! under test is the code that ships, not a hand-copied re-description of it).
//!
//! This file sits at the crate root, next to `build.rs` and NOT under `src/`, `examples/`
//! or `tests/` -- deliberately. `viewer/scripts/build-wasm.mjs`'s `fingerprintInputs` walks
//! those three directories plus three named files; a crate-root file is in none of them.
//! If this logic lived in `src/` instead, editing it would move the digest on the Node side
//! too (harmless, since both sides would see it) -- but if it lived in `src/` and this
//! module were ALSO used to explain what changed, it would tempt someone into thinking
//! `build.rs` itself is one of the fingerprinted inputs. It is not, and must not become
//! one: `build.rs`
//! computes the digest, so if `build.rs` fed itself into that same digest the two
//! independent implementations (this one and the Node script) would disagree by
//! construction -- the Rust side would always see one more file than the Node side ever
//! could. See the crate's `build.rs` for the full ruling.
//!
//! Reproduces `viewer/scripts/build-wasm.mjs`'s `fingerprintInputs` + `sourceFingerprint`
//! EXACTLY: same walk order (directory entries sorted by name, depth-first), same
//! repo-relative sort key (`/`-separated, independent of the host path separator), same
//! per-file line format (`<sha256 hex>  <two spaces><rel path>`), same body assembly
//! (`toolchain: {toolchain}\n` + `build: cargo {args}\n` + lines joined by `\n` + a final
//! `\n`), same outer digest (sha256 of that body as UTF-8).

use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

/// One input file's identity for the manifest: its repo-relative, `/`-separated path, and
/// where to actually read its bytes from.
#[derive(Debug)]
pub struct Input {
    pub rel: String,
    pub full: PathBuf,
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Depth-first walk of `dir`, directory entries sorted by name at each level (matching
/// Node's `readdirSync(..., { withFileTypes: true })` + `entries.sort(...)` on
/// `entry.name`), appending every FILE (not directory) it finds to `files` in that order.
fn walk_sorted(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    let read_dir = fs::read_dir(dir)
        .map_err(|e| format!("cannot read fingerprint input directory {}: {e}", dir.display()))?;
    let mut entries: Vec<_> = read_dir
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("cannot read an entry under {}: {e}", dir.display()))?;
    entries.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    for entry in entries {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|e| format!("cannot stat {}: {e}", path.display()))?;
        if file_type.is_dir() {
            walk_sorted(&path, files)?;
        } else if file_type.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

/// `walk_sorted`, but a directory that contributes zero files is an error rather than a
/// silently smaller digest -- the empty-directory defence the Node script carries for the
/// same reason: this repository keeps finding silent zeroes, and a fingerprint that stays
/// "healthy" over a renamed or emptied directory is exactly that shape of bug.
fn walk_non_empty(dir: &Path, why: &str, files: &mut Vec<PathBuf>) -> Result<(), String> {
    let before = files.len();
    walk_sorted(dir, files)?;
    if files.len() == before {
        return Err(format!(
            "fingerprint input directory is empty: {} ({why})",
            dir.display()
        ));
    }
    Ok(())
}

/// The 28-input set (today), sorted on the repo-relative path -- content-addressed, not
/// walk-order-addressed, so the digest depends on what the files say and not on which OS or
/// which directory-entry order produced them.
pub fn fingerprint_inputs(root: &Path) -> Result<Vec<Input>, String> {
    let crate_dir = root.join("crates").join("worldbuilder-engine");
    let mut files = Vec::new();

    walk_non_empty(&crate_dir.join("src"), "the engine itself", &mut files)?;
    walk_non_empty(
        &crate_dir.join("examples"),
        "the parity corpus generator",
        &mut files,
    )?;
    walk_non_empty(
        &crate_dir.join("tests"),
        "the determinism guard and the integration tests",
        &mut files,
    )?;

    for extra in [
        crate_dir.join("Cargo.toml"),
        root.join("Cargo.toml"),
        root.join("Cargo.lock"),
    ] {
        if !extra.is_file() {
            return Err(format!("fingerprint input missing: {}", extra.display()));
        }
        files.push(extra);
    }

    let mut inputs: Vec<Input> = files
        .into_iter()
        .map(|full| {
            let rel = full
                .strip_prefix(root)
                .map_err(|e| format!("{} is not under {}: {e}", full.display(), root.display()))?
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            Ok(Input { rel, full })
        })
        .collect::<Result<_, String>>()?;

    inputs.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok(inputs)
}

/// `toolchain` and `build_args` are baked into the digested body exactly as
/// `sourceFingerprint(root, toolchain)` in `build-wasm.mjs` does -- the digest is not just
/// "these files' bytes", it also pins the exact toolchain and the exact `cargo` invocation
/// the shipped `.wasm` was built with, which is why the ground-truth digest is reproducible
/// only on the host and toolchain that produced it.
pub fn source_fingerprint(
    root: &Path,
    toolchain: &str,
    build_args: &str,
) -> Result<(String, usize), String> {
    let inputs = fingerprint_inputs(root)?;
    let mut body = format!("toolchain: {toolchain}\nbuild: cargo {build_args}\n");
    let mut lines = Vec::with_capacity(inputs.len());
    for input in &inputs {
        let bytes = fs::read(&input.full)
            .map_err(|e| format!("cannot read fingerprint input {}: {e}", input.full.display()))?;
        lines.push(format!("{}  {}", sha256_hex(&bytes), input.rel));
    }
    body.push_str(&lines.join("\n"));
    body.push('\n');
    let digest = sha256_hex(body.as_bytes());
    Ok((digest, inputs.len()))
}
