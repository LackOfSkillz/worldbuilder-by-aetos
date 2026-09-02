//! DETERMINISM-001's static guard. A rule in a document holds for a year and then quietly
//! stops; this fails the build instead.

use std::fs;
use std::path::Path;

const BANNED: &[&str] = &[
    ".sin()", ".cos()", ".sqrt()", ".hypot(", ".atan2(", ".asin(", ".tanh(",
    ".powf(", ".powi(", ".floor()", ".ceil()", ".round()", ".exp()", ".ln()",
    ".to_radians()", ".to_degrees()",
];

#[test]
fn no_std_float_maths_outside_detmath() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offences = Vec::new();

    for entry in fs::read_dir(&src).expect("read src/") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        if path.file_name().and_then(|n| n.to_str()) == Some("detmath.rs") {
            continue;
        }
        let text = fs::read_to_string(&path).expect("read source file");
        for (lineno, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            for needle in BANNED {
                if line.contains(needle) {
                    offences.push(format!(
                        "{}:{}: {} — route it through detmath",
                        path.display(),
                        lineno + 1,
                        needle
                    ));
                }
            }
        }
    }

    assert!(
        offences.is_empty(),
        "std float maths found outside detmath:\n{}",
        offences.join("\n")
    );
}
