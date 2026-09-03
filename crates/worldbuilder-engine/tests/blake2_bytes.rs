//! Task 1 of slice 1j: prove the RustCrypto `blake2` crate reproduces CPython's
//! `hashlib.blake2b(message, digest_size=8)` bit-for-bit, before anything in the engine
//! depends on it.
//!
//! Every vector's expected bytes were computed independently in Python
//! (`hashlib.blake2b(s.encode("utf-8"), digest_size=8).digest()`) against this crate's
//! venv, then re-derived a second time to guard against a transcription error. This file
//! is permanent regression coverage for the dependency pin; the throwaway cross-language
//! harness for this task is `tests/test_blake2_bytes.py` at the repo root, deleted in
//! Task 7.

use blake2::digest::{Update, VariableOutput};
use blake2::Blake2bVar;

/// `hashlib.blake2b(message, digest_size=8).digest()`, byte for byte.
///
/// `Blake2bVar` (variable-output BLAKE2b) is used rather than the fixed `Blake2b512` type
/// plus a truncation, because BLAKE2's initial state mixes in the requested output length:
/// an 8-byte BLAKE2b digest is a different hash from the first 8 bytes of a 64-byte one,
/// not a prefix of it.
fn digest8(message: &[u8]) -> [u8; 8] {
    let mut hasher = Blake2bVar::new(8).expect("8 is a valid BLAKE2b output length");
    hasher.update(message);
    let mut out = [0u8; 8];
    hasher
        .finalize_variable(&mut out)
        .expect("output buffer is exactly 8 bytes");
    out
}

/// The measured vector from the task brief, re-derived independently against the live
/// Python before being trusted: `hashlib.blake2b(b"20260831|plate|7|pole-z",
/// digest_size=8).digest()`.
#[test]
fn measured_vector_from_the_brief() {
    let message = "20260831|plate|7|pole-z";
    let digest = digest8(message.as_bytes());
    let expected: [u8; 8] = [0x2d, 0x72, 0x9d, 0x25, 0x7c, 0x6a, 0x15, 0x50];

    assert_eq!(
        digest, expected,
        "message {message:?} ({} bytes) hashed to {digest:02x?}, expected {expected:02x?} \
         -- check that the message is not being passed as BLAKE2's key parameter, and that \
         digest_size=8 is a real 8-byte BLAKE2b rather than a truncated 64-byte one",
        message.len()
    );

    let u = u64::from_le_bytes(digest);
    assert_eq!(
        u, 5_770_635_578_984_722_989,
        "little-endian u64 from digest {digest:02x?} was {u}, expected 5770635578984722989"
    );

    // Python: struct.unpack("<Q", digest)[0] / float(1 << 64)
    let fraction = (u as f64) / 18_446_744_073_709_551_616.0_f64;
    assert_eq!(
        fraction.to_bits(),
        0.3128267815678692_f64.to_bits(),
        "fraction from u64 {u} was {fraction}, expected 0.3128267815678692 bit-for-bit"
    );
    assert!(
        u > (1u64 << 53),
        "the measured vector's u64 {u} does not exceed 2^53, so it does not exercise the \
         f64 rounding path this task is required to check"
    );
}

/// `"{world_seed}|plate|{index}|{label}"` for every label `_fraction` is ever called
/// with, across several seeds and indices plus the edge cases the brief calls out:
/// `world_seed = 0`, a negative seed, a very large seed (beyond 2^53, so its own decimal
/// string already exercises unusual digit patterns), and `index = 0`.
///
/// Expected bytes below were produced by:
/// `hashlib.blake2b(s.encode("utf-8"), digest_size=8).digest()` for each `s`, run twice
/// independently in the project venv (Python 3.11) against a from-scratch reimplementation
/// of the string-joining rule, not copied from any earlier plan artifact.
fn conformance_vectors() -> Vec<(&'static str, [u8; 8])> {
    vec![
        // world_seed = 0, index = 0 -- both zero edge cases at once.
        ("0|plate|0|jitter-a", [0x65, 0x79, 0x48, 0x0d, 0x28, 0x9e, 0x02, 0x46]),
        ("0|plate|0|jitter-b", [0x66, 0x61, 0x0e, 0xc0, 0x3e, 0xef, 0xed, 0x6b]),
        ("0|plate|0|pole-z", [0x46, 0x24, 0x6c, 0x9e, 0x85, 0xe0, 0xd4, 0x89]),
        ("0|plate|0|pole-angle", [0xb0, 0x6e, 0xd5, 0xb6, 0x62, 0xbc, 0x58, 0x75]),
        ("0|plate|0|rate", [0xdf, 0x16, 0x50, 0x7c, 0x82, 0xe6, 0xf5, 0xe3]),
        ("0|plate|0|sense", [0x92, 0x3f, 0xb0, 0x33, 0xd4, 0x42, 0x09, 0x5d]),
        // world_seed = 0, index = 21 (last index of the default 22-plate count).
        ("0|plate|21|jitter-a", [0x11, 0xaa, 0x8e, 0x5e, 0x79, 0x71, 0xa0, 0x91]),
        ("0|plate|21|pole-z", [0xfc, 0x74, 0x98, 0xa9, 0x82, 0xa0, 0xfd, 0x7b]),
        ("0|plate|21|sense", [0xf1, 0x32, 0xe2, 0xb5, 0x5d, 0x46, 0xc4, 0x76]),
        // negative seed.
        ("-1|plate|0|jitter-a", [0xdf, 0x52, 0x7f, 0xeb, 0xdc, 0xc8, 0x41, 0x06]),
        ("-1|plate|7|pole-z", [0x24, 0xdb, 0x0d, 0xf4, 0x4a, 0x40, 0x61, 0x49]),
        ("-1|plate|21|sense", [0x3c, 0x4f, 0x54, 0xa3, 0xbc, 0x40, 0xfb, 0x3b]),
        // a very large seed (2^62), and its negation.
        ("4611686018427387904|plate|0|pole-z", [0x2d, 0xfd, 0x5b, 0x94, 0x06, 0xa8, 0x78, 0xd8]),
        ("4611686018427387904|plate|7|rate", [0xb4, 0x16, 0x54, 0xf7, 0x86, 0xa2, 0x2d, 0x76]),
        ("-4611686018427387904|plate|1|pole-angle", [0xce, 0x9d, 0xf2, 0x00, 0xd7, 0x8d, 0xff, 0x6b]),
        // an ordinary mid-size seed, several indices, all six labels.
        ("987654321|plate|0|jitter-a", [0x28, 0xda, 0x1f, 0x39, 0x1e, 0xee, 0x79, 0x7f]),
        ("987654321|plate|1|jitter-b", [0x9a, 0x1e, 0xd2, 0x49, 0x35, 0x62, 0xd2, 0x4c]),
        ("987654321|plate|7|pole-z", [0x17, 0xb1, 0x40, 0x75, 0x00, 0x47, 0xbf, 0x34]),
        ("987654321|plate|7|pole-angle", [0x24, 0x06, 0x44, 0x66, 0x8b, 0x7e, 0x1d, 0x47]),
        ("987654321|plate|21|rate", [0xee, 0x74, 0xe5, 0xa1, 0xd0, 0xe4, 0xf4, 0xcd]),
        ("987654321|plate|21|sense", [0x2e, 0x3d, 0xb7, 0x20, 0x3e, 0x18, 0x14, 0x30]),
        // 20260831 -- the same seed the measured vector uses -- across the other labels.
        ("20260831|plate|7|jitter-a", [0x0a, 0x3e, 0x49, 0x02, 0x08, 0xd2, 0xc2, 0xf1]),
        ("20260831|plate|7|jitter-b", [0x2c, 0x5c, 0xec, 0x1b, 0xaa, 0x15, 0x04, 0x38]),
        ("20260831|plate|7|pole-angle", [0x5a, 0xba, 0x4a, 0xf4, 0xa2, 0x48, 0x83, 0x5c]),
        ("20260831|plate|7|rate", [0x9a, 0x1d, 0xe7, 0x76, 0x60, 0x19, 0xab, 0x68]),
        ("20260831|plate|7|sense", [0xa3, 0xfc, 0x80, 0x46, 0xf2, 0x77, 0xe7, 0xae]),
    ]
}

#[test]
fn digests_match_python_hashlib_byte_for_byte() {
    for (message, expected) in conformance_vectors() {
        let digest = digest8(message.as_bytes());
        assert_eq!(
            digest, expected,
            "message {message:?} hashed to {digest:02x?} in Rust, expected {expected:02x?} \
             from Python's hashlib.blake2b(..., digest_size=8) -- a single differing bit \
             here means the crate or its parameters are wrong and nothing downstream can \
             be trusted"
        );
    }
}

/// `u64::from_le_bytes` matches Python's `struct.unpack("<Q", digest)[0]`, and the
/// `u64 as f64 / 2^64` conversion is bit-identical to Python's `/ float(1 << 64)` --
/// including for a u64 that exceeds 2^53 and therefore rounds when it becomes an f64.
#[test]
fn u64_and_fraction_conversion_matches_python() {
    // (message, expected u64, expected fraction as an f64 bit pattern, from Python)
    let cases: [(&str, u64, f64); 4] = [
        ("20260831|plate|7|pole-z", 5_770_635_578_984_722_989, 0.3128267815678692),
        ("0|plate|0|jitter-a", 5_044_768_427_467_110_757, 0.27347744443730615),
        ("0|plate|0|jitter-b", 7_777_135_184_327_893_350, 0.4215993431280878),
        ("0|plate|0|pole-z", 9_931_809_942_751_945_798, 0.5384044958322397),
    ];

    let mut saw_a_value_that_rounded = false;

    for (message, expected_u, expected_fraction) in cases {
        let digest = digest8(message.as_bytes());
        let u = u64::from_le_bytes(digest);
        assert_eq!(u, expected_u, "u64 from {message:?} was {u}, expected {expected_u}");

        let fraction = (u as f64) / 18_446_744_073_709_551_616.0_f64;
        assert_eq!(
            fraction.to_bits(),
            expected_fraction.to_bits(),
            "fraction from u64 {u} was {fraction}, expected {expected_fraction} bit-for-bit"
        );

        // A u64 above 2^53 cannot be represented exactly as an f64, so converting it back
        // and forth loses the low bits -- this is the rounding path step 3 asks about.
        if u > (1u64 << 53) {
            let roundtrip_loses_precision = (u as f64) as u64 != u;
            if roundtrip_loses_precision {
                saw_a_value_that_rounded = true;
            }
        }
    }

    assert!(
        saw_a_value_that_rounded,
        "none of the tested u64 values actually rounded on conversion to f64 -- the \
         rounding path this task must check would be untested"
    );
}

/// `_spread`'s degeneracy guard (`sideways.length() < 1e-9`) is unreachable: `sideways` is
/// the cross product of the z axis with the seed point, so its length is the spiral's ring
/// radius. With `z = 1 - 2u` for `u = (index + 0.5) / count`, `1 - z^2 = 4u(1-u)`, so
/// `ring = 2*sqrt(u(1-u))`, smallest at `index = 0` where `u = 0.5/count` and `ring`
/// approaches `sqrt(2/count)` for large `count`.
#[test]
fn spread_degeneracy_guard_is_unreachable() {
    let counts: [u64; 6] = [1, 2, 3, 22, 1_000, 100_000];
    let mut minimum = f64::INFINITY;

    for count in counts {
        let count_f = count as f64;
        let u = 0.5 / count_f;
        let z = 1.0 - 2.0 * u;
        let ring_from_z = (1.0 - z * z).max(0.0).sqrt();
        let ring_from_derivation = 2.0 * (u * (1.0 - u)).sqrt();

        assert!(
            (ring_from_z - ring_from_derivation).abs() < 1e-12,
            "for count {count}, ring from z ({ring_from_z}) and ring from the derivation \
             ({ring_from_derivation}) disagree -- the algebra in the task brief does not hold"
        );

        if ring_from_z < minimum {
            minimum = ring_from_z;
        }
    }

    // The derivation says the smallest ring (at index 0, largest count) approaches
    // sqrt(2/count) for large count.
    let largest_count = 100_000.0_f64;
    let approx = (2.0 / largest_count).sqrt();
    assert!(
        (minimum - approx).abs() / approx < 1e-3,
        "measured minimum ring {minimum} does not match the sqrt(2/count) approximation \
         {approx} for count={largest_count} to within 0.1% -- the brief's algebra may be wrong"
    );

    assert!(
        minimum > 1e-3,
        "minimum ring {minimum} across counts {counts:?} is not many orders of magnitude \
         above the 1e-9 guard threshold as expected"
    );
    assert!(
        minimum > 1e-9 * 1_000_000.0,
        "minimum ring {minimum} is not at least a million times the 1e-9 guard threshold"
    );
}
