//! Where the plates are, and how fast they turn.
//!
//! Ported from `worldbuilder/plates/generation.py`. **Every value here is hashed, never
//! drawn from a sequence.** A plate's pole and rate come from
//! `hash(world_seed, "plate", index, what)` rather than from successive calls to a random
//! number generator, and that is a hard requirement rather than a preference.
//!
//! A generator that consumes a mutable sequence makes every plate depend on the order in
//! which plates were built and on how many values each one happened to take. Add a
//! property to a plate six weeks from now -- a crust thickness, a colour, anything -- and
//! every subsequent plate silently changes, because the sequence shifted under it. Worlds
//! people had sailed would quietly become different worlds.
//!
//! Hashing removes the possibility rather than the temptation. Plate 7's pole depends on
//! nothing but the seed and the number 7.

use blake2::digest::{Update, VariableOutput};
use blake2::Blake2bVar;

/// How many plates, unless a world asks otherwise. Earth has seven or eight major ones
/// and a good many minor; a couple of dozen gives enough boundary to make varied
/// geography without cells so small that every coast is a margin.
pub const DEFAULT_PLATE_COUNT: usize = 22;

/// How far a seed may be nudged off the even spiral, in radians. Enough to break the
/// regularity of the pattern, not enough to let two seeds collide and produce a sliver of
/// a plate that nothing sensible can be done with.
pub const JITTER_RAD: f64 = 0.18;

/// Plausible plate speeds, in radians per million years. At Earth's radius the upper end
/// is about ten centimetres a year, which is roughly the fastest real plates manage.
pub const SLOWEST_RAD_PER_MYR: f64 = 0.002;
pub const FASTEST_RAD_PER_MYR: f64 = 0.016;

/// One part of a `_fraction` key. Python's `_fraction(world_seed, *parts)` is variadic
/// over mixed `int` and `str` arguments; Rust has no such thing, so callers build a slice
/// of `Part` instead. A borrowed `&str` (rather than `&'static str`) keeps this usable
/// with labels built at runtime, not just literals.
pub enum Part<'a> {
    Int(i64),
    Str(&'a str),
}

impl std::fmt::Display for Part<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Part::Int(value) => write!(f, "{value}"),
            Part::Str(value) => write!(f, "{value}"),
        }
    }
}

/// `"|".join(str(part) for part in (world_seed,) + parts)` -- the world seed first, then
/// every part, pipe-separated, with none trailing.
fn joined_key(world_seed: i64, parts: &[Part]) -> String {
    let mut joined = world_seed.to_string();
    for part in parts {
        joined.push('|');
        joined.push_str(&part.to_string());
    }
    joined
}

/// `hashlib.blake2b(message, digest_size=8).digest()`, byte for byte. `Blake2bVar`
/// (variable-output BLAKE2b) is used rather than the fixed `Blake2b512` type plus a
/// truncation, because BLAKE2's initial state mixes in the requested output length: an
/// 8-byte BLAKE2b digest is a different hash from the first 8 bytes of a 64-byte one, not
/// a prefix of it.
fn digest8(message: &[u8]) -> [u8; 8] {
    let mut hasher = Blake2bVar::new(8).expect("8 is a valid BLAKE2b output length");
    hasher.update(message);
    let mut out = [0u8; 8];
    hasher
        .finalize_variable(&mut out)
        .expect("output buffer is exactly 8 bytes");
    out
}

/// A number in `[0, 1)` from a seed and a label, with no sequence anywhere.
///
/// Ported from `_fraction` in `worldbuilder/plates/generation.py`.
pub fn fraction(world_seed: i64, parts: &[Part]) -> f64 {
    let key = joined_key(world_seed, parts);
    let digest = digest8(key.as_bytes());
    let bits = u64::from_le_bytes(digest);
    // cast-ok: matching Python's `int / float(1 << 64)`; 2^64 is an exact power of two so
    // the division adds no rounding beyond the u64-to-f64 conversion itself.
    (bits as f64) / 18_446_744_073_709_551_616.0_f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_negative_world_seed_formats_and_hashes_like_the_python() {
        // The seed is an i64 and may be negative. Python's str(-20260831) is
        // "-20260831"; Rust's Display for i64 agrees, but the two only have to
        // agree here for the digest to match, so pin it rather than assume it.
        // Vector measured from the live Python, not derived from this code.
        assert_eq!(
            joined_key(-20260831, &[Part::Str("plate"), Part::Int(3), Part::Str("sense")]),
            "-20260831|plate|3|sense",
        );
        let f = fraction(-20260831, &[Part::Str("plate"), Part::Int(3), Part::Str("sense")]);
        assert_eq!(f.to_bits(), 0.6028567705813382_f64.to_bits());
    }

    #[test]
    fn the_joined_key_is_byte_identical_to_the_python() {
        // "20260831|plate|7|pole-z" -- pipes between every part, none trailing.
        assert_eq!(
            joined_key(20260831, &[Part::Str("plate"), Part::Int(7), Part::Str("pole-z")]),
            "20260831|plate|7|pole-z"
        );
    }

    #[test]
    fn a_fraction_is_in_the_unit_interval_and_never_reaches_one() {
        // u64 / 2^64 is in [0, 1) by construction: the largest u64 is 2^64 - 1.
        for index in 0..64 {
            let f = fraction(20260831, &[Part::Str("plate"), Part::Int(index), Part::Str("rate")]);
            assert!(f >= 0.0 && f < 1.0, "fraction {f} out of range at index {index}");
        }
    }

    #[test]
    fn the_same_arguments_always_give_the_same_fraction() {
        let a = fraction(20260831, &[Part::Str("plate"), Part::Int(7), Part::Str("pole-z")]);
        let b = fraction(20260831, &[Part::Str("plate"), Part::Int(7), Part::Str("pole-z")]);
        assert_eq!(a.to_bits(), b.to_bits());
    }

    #[test]
    fn different_labels_give_unrelated_fractions() {
        // The whole design rests on this: plate 7's pole does not move when plate 6
        // gains a property. Different labels must not collide.
        let z = fraction(20260831, &[Part::Str("plate"), Part::Int(7), Part::Str("pole-z")]);
        let a = fraction(20260831, &[Part::Str("plate"), Part::Int(7), Part::Str("pole-angle")]);
        assert_ne!(z.to_bits(), a.to_bits());
    }

    /// The measured vector from the task brief, re-derived independently against the
    /// live Python during Task 1 and reproduced twice.
    #[test]
    fn measured_vector_from_the_brief() {
        let f = fraction(20260831, &[Part::Str("plate"), Part::Int(7), Part::Str("pole-z")]);
        assert_eq!(f.to_bits(), 0.3128267815678692_f64.to_bits());
    }
}
