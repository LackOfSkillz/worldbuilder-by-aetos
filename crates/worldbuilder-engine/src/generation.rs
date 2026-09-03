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

use crate::detmath as m;
use crate::sphere::SpherePoint;
use crate::vectors::{Vec3, DEGENERATE};

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

/// `_spread`, split so the tests can compare against the un-jittered spiral point
/// directly rather than re-deriving it by hand. `spread` below is `spread_impl(..., 1.0)`;
/// a test calls this with `jitter_scale: 0.0` and asserts the two differ, which a jitter
/// wired to nothing would fail.
fn spread_impl(world_seed: i64, index: usize, count: usize, jitter_scale: f64) -> SpherePoint {
    // `golden = math.pi * (3.0 - math.sqrt(5.0))` computed, not written as a decimal
    // literal: sqrt(5.0) is correctly rounded, `3.0 - sqrt(5.0)` is exact, and one
    // multiplication by PI follows. A decimal literal would introduce a rounding this
    // code does not have.
    let golden = std::f64::consts::PI * (3.0 - m::sqrt(5.0));
    // cast-ok: plate index and count to f64, matching Python's true division.
    let z = 1.0 - 2.0 * (index as f64 + 0.5) / (count as f64);
    let inner = 1.0 - z * z;
    // Python writes `max(0.0, 1.0 - z * z)`; two-argument max returns the second argument
    // when the comparison is false, so a NaN `inner` yields 0.0. Explicit if/else in the
    // Python's operand order reproduces that rather than `f64::max`, which propagates NaN.
    let ring = m::sqrt(if inner > 0.0 { inner } else { 0.0 });
    let angle = golden * (index as f64); // cast-ok: plate index to f64

    let point = Vec3::new(m::cos(angle) * ring, m::sin(angle) * ring, z);

    // Nudge along two arbitrary but deterministic tangent directions.
    let index_i64 = index as i64; // cast-ok: plate index to i64 for the hash key
    let key_a = [Part::Str("plate"), Part::Int(index_i64), Part::Str("jitter-a")];
    let key_b = [Part::Str("plate"), Part::Int(index_i64), Part::Str("jitter-b")];
    let nudge_a = (2.0 * fraction(world_seed, &key_a) - 1.0) * JITTER_RAD * jitter_scale;
    let nudge_b = (2.0 * fraction(world_seed, &key_b) - 1.0) * JITTER_RAD * jitter_scale;

    let mut sideways = Vec3::new(0.0, 0.0, 1.0).cross(&point);
    // The Python guard: `if sideways.length() < 1e-9: sideways = Vec3(1, 0, 0).cross(point)`.
    // Unreachable for any usable count: `sideways` here is `(0,0,1).cross(point)`, which
    // is `(-point.y, point.x, 0)`, so its length is exactly the spiral's ring radius. With
    // `z = 1 - 2u` for `u = (index + 0.5) / count`, `1 - z^2 = 4u(1-u)`, so
    // `ring = 2*sqrt(u(1-u))`, smallest at index 0 where it is about `sqrt(2/count)`.
    // Task 1 measured the minimum across counts up to 100,000 at 0.004472, about 4.5
    // million times this guard; reaching it would need `count > 2e18`. Ported anyway --
    // removing it would change behaviour for an absurd count, and a future reader should
    // not have to re-derive why it never fires.
    if sideways.length() < DEGENERATE {
        sideways = Vec3::new(1.0, 0.0, 0.0).cross(&point);
    }
    let east = sideways
        .normalised()
        .expect("point is a unit vector, so sideways cannot be the zero vector here");
    let north = point.cross(&east);
    let nudged = point.add(&east.scaled(nudge_a)).add(&north.scaled(nudge_b));
    SpherePoint::from_vector(&nudged).expect("point plus a bounded nudge is never the zero vector")
}

/// One seed position on the Fibonacci spiral, nudged.
///
/// Ported from `_spread` in `worldbuilder/plates/generation.py`.
pub fn spread(world_seed: i64, index: usize, count: usize) -> SpherePoint {
    spread_impl(world_seed, index, count, 1.0)
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

    #[test]
    fn rust_pi_matches_cpython_math_pi() {
        // Slice 1h confirmed FRAC_PI_2 matched CPython's; `golden`'s correctness leans on
        // the full PI matching too. CPython's math.pi is 3.141592653589793.
        assert_eq!(std::f64::consts::PI.to_bits(), 3.141592653589793_f64.to_bits());
    }

    #[test]
    fn the_degeneracy_guard_never_fires_for_a_usable_count() {
        // ring = 2*sqrt(u(1-u)) with u = (i + 0.5)/count, smallest at i = 0 where it is
        // about sqrt(2/count). Reaching the 1e-9 guard needs count > 2e18. Assert it
        // rather than believing the algebra: this pins the claim the port's comment makes.
        for &count in &[1usize, 2, 3, 22, 1000, 100_000] {
            let u = 0.5 / (count as f64); // cast-ok: plate count to f64 for the bound
            let ring = 2.0 * m::sqrt(u * (1.0 - u));
            assert!(ring > 1e-6, "ring {ring} at count {count} approaches the guard");
        }
    }

    #[test]
    fn every_seed_is_a_unit_vector() {
        // from_vector normalises, so this holds by construction -- it is here to catch
        // the direct constructor being substituted for it.
        for index in 0..22 {
            let p = spread(20260831, index, 22);
            assert!((p.vector.length() - 1.0).abs() < 1e-12, "seed {index} is not unit");
        }
    }

    #[test]
    fn distinct_indices_give_distinct_seeds() {
        let seeds: Vec<_> = (0..22).map(|i| spread(20260831, i, 22)).collect();
        for i in 0..seeds.len() {
            for j in (i + 1)..seeds.len() {
                let d = seeds[i].vector.sub(&seeds[j].vector).length();
                assert!(d > 1e-6, "seeds {i} and {j} collide");
            }
        }
    }

    #[test]
    fn the_jitter_actually_moves_the_point() {
        // spread_impl exposes a jitter_scale so the test can compare the jittered point
        // against the same computation with the nudges forced to zero. A jitter wired to
        // nothing would make this fail, while every other test in this file would still
        // pass.
        let jittered = spread_impl(20260831, 5, 22, 1.0);
        let unjittered = spread_impl(20260831, 5, 22, 0.0);
        let moved = jittered.vector.sub(&unjittered.vector).length();
        assert!(moved > 1e-6, "jitter moved the point by only {moved}");
    }
}
