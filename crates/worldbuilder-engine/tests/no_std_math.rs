//! DETERMINISM-001's static guard. A rule in a document holds for a year and then quietly
//! stops; this fails the build instead.
//!
//! The ban list covers both call syntaxes: method form (`.sin()`) and fully-qualified
//! function form (`f64::sin(x)`), because only banning the first lets the second straight
//! through.
//!
//! It also bans `as i64`/`as i32`/`as u64`/`as u32`, because Python's `int(x // 1)` floors
//! toward negative infinity while Rust's `as i64` truncates toward zero -- for any negative
//! coordinate they select a different lattice cell, silently. Integer-to-integer casts are
//! legitimate, so a line carrying the marker `// cast-ok: <reason>` is exempted -- use it
//! only when the cast genuinely is not a float truncation.
//!
//! The scan walks `src/` recursively, so a submodule directory added by a later slice
//! (`src/terrain/`, `src/plates/`, ...) is covered without anyone remembering to update
//! this file, and it skips `detmath.rs` wherever it appears in that tree.

use std::fs;
use std::path::{Path, PathBuf};

const BANNED: &[&str] = &[
    // method syntax (open-ended so it matches whether or not the method takes an argument)
    ".sin(", ".cos(", ".tan(", ".sqrt(", ".hypot(", ".atan2(", ".asin(", ".acos(",
    ".atan(", ".tanh(", ".sinh(", ".cosh(", ".powf(", ".powi(", ".floor(", ".ceil(",
    ".round(", ".exp(", ".exp2(", ".ln(", ".log2(", ".log10(", ".cbrt(",
    ".to_radians(", ".to_degrees(", ".mul_add(",
    // fully-qualified function syntax -- `.method()` needles above do not catch `f64::sin(x)`
    "f64::sin(", "f64::cos(", "f64::tan(", "f64::sqrt(", "f64::hypot(", "f64::atan2(",
    "f64::asin(", "f64::acos(", "f64::atan(", "f64::tanh(", "f64::sinh(", "f64::cosh(",
    "f64::powf(", "f64::powi(", "f64::floor(", "f64::ceil(", "f64::round(", "f64::exp(",
    "f64::exp2(", "f64::ln(", "f64::log2(", "f64::log10(", "f64::cbrt(", "f64::mul_add(",
    // the floor/truncate trap: Python floors, `as i64` truncates
    " as i64", " as i32", " as u64", " as u32",
];

/// `.abs()` is deliberately NOT banned: it is exact (sign-bit clear, no rounding) and is
/// used legitimately throughout the geometry code.
const CAST_OK_MARKER: &str = "// cast-ok:";

fn rust_files_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            rust_files_recursive(&path, out);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// Scan one file's source text for banned calls, returning a human-readable offence per
/// hit. `label` is used only in the offence message, so tests can call this with a
/// synthetic name instead of a real path.
fn scan_text(label: &str, text: &str) -> Vec<String> {
    let mut offences = Vec::new();
    for (lineno, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            continue;
        }
        if line.contains(CAST_OK_MARKER) {
            continue;
        }
        for needle in BANNED {
            if line.contains(needle) {
                offences.push(format!(
                    "{}:{}: {} — route it through detmath (or mark with `{}` if this is a genuine integer cast)",
                    label,
                    lineno + 1,
                    needle,
                    CAST_OK_MARKER,
                ));
            }
        }
    }
    offences
}

#[test]
fn no_std_float_maths_outside_detmath() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files_recursive(&src, &mut files);

    let mut offences = Vec::new();
    for path in files {
        if path.file_name().and_then(|n| n.to_str()) == Some("detmath.rs") {
            continue;
        }
        let text = fs::read_to_string(&path).expect("read source file");
        offences.extend(scan_text(&path.display().to_string(), &text));
    }

    assert!(
        offences.is_empty(),
        "std float maths (or an unmarked float-truncating cast) found outside detmath:\n{}",
        offences.join("\n")
    );
}

#[test]
fn the_guard_can_actually_fail() {
    // A guard only ever seen to pass proves nothing. This proves the scanner detects
    // what it claims to, without needing anyone to plant a call in real source.
    let offences = scan_text("fake.rs", "fn f() { let _ = (2.0f64).sqrt(); }");
    assert!(!offences.is_empty(), "the scanner missed a banned call");

    let clean = scan_text("fake.rs", "fn f() { let _ = crate::detmath::sqrt(2.0); }");
    assert!(clean.is_empty(), "the scanner flagged a legitimate detmath call");
}

#[test]
fn the_guard_catches_the_function_call_form_too() {
    // Fix 3(b): `.sin()` needles do not catch `f64::sin(x)`. Prove the fully-qualified
    // form is actually caught rather than merely asserted to be.
    let offences = scan_text("fake.rs", "fn f(x: f64) -> f64 { f64::sin(x) }");
    assert!(!offences.is_empty(), "the scanner missed f64::sin(x) in function-call form");
}

#[test]
fn the_guard_catches_the_floor_vs_truncate_trap() {
    // Fix 4: `as i64` truncates where Python's `int(x // 1)` floors. This must be banned.
    let offences = scan_text("fake.rs", "fn f(x: f64) -> i64 { x as i64 }");
    assert!(!offences.is_empty(), "the scanner missed a float-to-int cast");
}

#[test]
fn the_guard_respects_the_cast_ok_escape_hatch() {
    // Integer-to-integer casts are legitimate and must not be forced through detmath.
    let clean = scan_text(
        "fake.rs",
        "fn f(x: u32) -> i64 { x as i64 } // cast-ok: widening an already-integer index",
    );
    assert!(clean.is_empty(), "the scanner flagged a marked, legitimate cast");
}

#[test]
fn the_guard_does_not_ban_abs() {
    // .abs() is exact and used legitimately; it must never be treated as banned.
    let clean = scan_text("fake.rs", "fn f(x: f64) -> f64 { x.abs() }");
    assert!(clean.is_empty(), "the scanner incorrectly flagged .abs()");
}
