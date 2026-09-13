//! DETERMINISM-001's static guard. A rule in a document holds for a year and then quietly
//! stops; this fails the build instead.
//!
//! The ban list covers both call syntaxes: method form (`.sin()`) and fully-qualified
//! function form (`f64::sin(x)`), because only banning the first lets the second straight
//! through.
//!
//! It also bans `.abs()` (with a ledger of what predates the rule; see `ABS_LEDGER`), and
//! bans `as i64`/`as i32`/`as u64`/`as u32`, because Python's `int(x // 1)` floors
//! toward negative infinity while Rust's `as i64` truncates toward zero -- for any negative
//! coordinate they select a different lattice cell, silently. Integer-to-integer casts are
//! legitimate, so a line carrying the marker `// cast-ok: <reason>` is exempted -- use it
//! only when the cast genuinely is not a float truncation.
//!
//! The scan walks `src/` and `examples/` recursively, so a submodule directory added by a
//! later slice (`src/terrain/`, `src/plates/`, ...) is covered without anyone remembering to
//! update this file, and it skips `detmath.rs` wherever it appears in either tree.
//!
//! `examples/` is in because it stopped being scaffolding. `examples/parity_dump.rs` writes the
//! corpus the parity gate compares bit for bit, and `examples/pond_search_survey.rs` is an
//! instrument whose numbers go into plan reports. A float truncation in either is a wrong number
//! reported as a measurement, which is the same failure this guard exists for.

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

const CAST_OK_MARKER: &str = "// cast-ok:";

/// `.abs()` **is** banned -- "No `.abs()`: write the comparison" has been in the global
/// constraints all along -- and until plan 2a's final review this file did not scan for it, which
/// is exactly why two survived on a line that review found freshly moved.
///
/// It is scanned with a **ledger** rather than a flat ban. The rule arrived after most of this
/// crate was written, and rewriting every one of the surviving calls in a merge wave would be a
/// wide, unmeasured edit to the elevation path for no behavioural gain. So: a file in
/// [`ABS_LEDGER`] may hold **exactly** the number of calls recorded there, and a file that is not
/// in it may hold none. The count may only ever go down -- a file that has shed one fails until
/// its entry is lowered, and a file that has gained one fails at once. New code gets the rule in
/// full; the backlog gets it when a file is next touched.
const ABS_BANNED: &[&str] = &[".abs(", "f64::abs(", "i64::abs(", "i32::abs("];

/// Path (relative to the crate root, forward slashes) to the number of `.abs()` calls that
/// predate the ban. **Lower an entry when you clear one; never raise one.** `detmath.rs` is not
/// here because the whole scan skips it. 185 calls across 25 files at the moment plan 2a
/// merged; `src/hydrology/buckets.rs` and everything under `src/water/` are absent because they
/// are clear.
const ABS_LEDGER: &[(&str, usize)] = &[
    ("src/bin/climate_survey.rs", 1),
    ("src/bin/gully_merging_survey.rs", 2),
    ("src/bin/mountain_survey.rs", 5),
    ("src/bin/relief_survey.rs", 2),
    ("src/climate.rs", 18),
    ("src/continentality.rs", 9),
    ("src/detail.rs", 9),
    ("src/erosion.rs", 2),
    ("src/features.rs", 3),
    ("src/generation.rs", 4),
    ("src/hydrology/bake_tests.rs", 2),
    ("src/hydrology/flow.rs", 1),
    ("src/hydrology/reaches.rs", 2),
    ("src/kinematics.rs", 2),
    ("src/noise.rs", 4),
    ("src/plates.rs", 9),
    ("src/shelf.rs", 15),
    ("src/sphere.rs", 5),
    ("src/steer.rs", 6),
    ("src/stream.rs", 12),
    ("src/substrate.rs", 9),
    ("src/surface.rs", 38),
    ("src/tangent.rs", 14),
    ("src/tectonics.rs", 9),
    ("src/wasm.rs", 2),
];

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

/// How many `.abs()` calls a file's source text holds. A wholly-commented line does not count.
///
/// The `// cast-ok:` marker deliberately does NOT exempt a line here. That marker justifies a
/// cast, and a cast is not an `.abs()`; honouring it would make `x.abs() as u32; // cast-ok: ...`
/// invisible to this scan in every file, ledgered or clean, which is a hole wide enough to drive
/// the whole ban through.
fn count_abs(text: &str) -> usize {
    let mut found = 0;
    for line in text.lines() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        for needle in ABS_BANNED {
            found += line.matches(needle).count();
        }
    }
    found
}

/// Every `.rs` file of the scanned trees, as (path relative to the crate root with forward
/// slashes, source text), with `detmath.rs` skipped wherever it appears.
fn scanned_sources() -> Vec<(String, String)> {
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for tree in ["src", "examples"] {
        rust_files_recursive(&crate_root.join(tree), &mut files);
    }
    files
        .into_iter()
        .filter(|p| p.file_name().and_then(|n| n.to_str()) != Some("detmath.rs"))
        .map(|path| {
            let text = fs::read_to_string(&path).expect("read source file");
            let rel = path
                .strip_prefix(crate_root)
                .unwrap_or(&path)
                .display()
                .to_string()
                .replace('\\', "/");
            (rel, text)
        })
        .collect()
}

/// The ledger is a ratchet, and this is the thing that ratchets it: a file may hold exactly the
/// `.abs()` calls recorded for it and no others, and a file with no entry may hold none.
#[test]
fn abs_stays_within_its_legacy_ledger() {
    let mut offences = Vec::new();
    let mut seen = Vec::new();
    for (rel, text) in scanned_sources() {
        let found = count_abs(&text);
        let allowed = ABS_LEDGER.iter().find(|(p, _)| *p == rel).map(|(_, n)| *n);
        seen.push(rel.clone());
        match allowed {
            None if found > 0 => offences.push(format!(
                "{rel}: {found} `.abs()` call(s) in a file the ledger does not name — write the \
                 comparison instead (`if x < 0.0 {{ -x }} else {{ x }}`)"
            )),
            Some(n) if found > n => offences.push(format!(
                "{rel}: {found} `.abs()` call(s), ledgered at {n} — the ledger only goes down"
            )),
            Some(n) if found < n => offences.push(format!(
                "{rel}: {found} `.abs()` call(s), ledgered at {n} — lower the entry to {found} \
                 (or delete it, at zero)"
            )),
            _ => {}
        }
    }
    for (path, _) in ABS_LEDGER {
        if !seen.iter().any(|s| s == path) {
            offences.push(format!("{path}: ledgered but no longer scanned — delete the entry"));
        }
    }
    assert!(
        offences.is_empty(),
        "`.abs()` is banned (write the comparison); the ledger holds only what predates the \
         rule:\n{}",
        offences.join("\n")
    );
}

#[test]
fn no_std_float_maths_outside_detmath() {
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for tree in ["src", "examples"] {
        rust_files_recursive(&crate_root.join(tree), &mut files);
    }

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
fn the_abs_counter_counts_what_the_ledger_is_made_of() {
    // A ratchet only ever seen to pass proves nothing: prove the counter sees both call
    // syntaxes, counts two on one line, and honours the same line rules as the scan above.
    assert_eq!(count_abs("fn f(x: f64) -> f64 { x.abs() }"), 1);
    assert_eq!(count_abs("fn f(x: f64, y: f64) -> f64 { x.abs() + f64::abs(y) }"), 2);
    assert_eq!(count_abs("// a comment mentioning x.abs() is not a call"), 0);
    // A cast-ok marker justifies the cast, never the .abs() sitting in front of it.
    assert_eq!(count_abs("let n = x.abs() as u32; // cast-ok: an already-rounded magnitude"), 1);
    assert_eq!(count_abs("fn f(x: f64) -> f64 { if x < 0.0 { -x } else { x } }"), 0);
}
