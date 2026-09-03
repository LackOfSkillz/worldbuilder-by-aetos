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

/// An evenly distributed axis of rotation.
///
/// Ported from `_pole` in `worldbuilder/plates/generation.py`.
///
/// Even over the sphere, which needs the z component to be uniform rather than the
/// latitude -- sampling latitude uniformly would crowd the poles, and a set of plates
/// all turning about nearly the same axis would drift as one sheet.
pub fn pole(world_seed: i64, index: usize) -> SpherePoint {
    let index_i64 = index as i64; // cast-ok: plate index to i64 for the hash key
    let z_key = [Part::Str("plate"), Part::Int(index_i64), Part::Str("pole-z")];
    let angle_key = [Part::Str("plate"), Part::Int(index_i64), Part::Str("pole-angle")];
    let z = 2.0 * fraction(world_seed, &z_key) - 1.0;
    let angle = 2.0 * std::f64::consts::PI * fraction(world_seed, &angle_key);
    let inner = 1.0 - z * z;
    // Python writes `max(0.0, 1.0 - z * z)`; explicit if/else in the Python's operand
    // order, as in spread_impl above, rather than f64::max, which would propagate a NaN
    // inner instead of flooring it at 0.0.
    let ring = m::sqrt(if inner > 0.0 { inner } else { 0.0 });
    // `SpherePoint(Vec3(...))` -- the direct, NON-normalising constructor, deliberately
    // not `from_vector`. The vector here is already unit by construction (ring is
    // sqrt(1 - z^2), so (cos*ring)^2 + (sin*ring)^2 + z^2 sums to 1), and normalising it
    // would look like a tidy-up but would change every pole's bits.
    SpherePoint { vector: Vec3::new(m::cos(angle) * ring, m::sin(angle) * ring, z) }
}

/// Radians per million years, signed.
///
/// Ported from `_rate` in `worldbuilder/plates/generation.py`.
///
/// The sign is what makes a rotation clockwise or otherwise, so it lives here rather
/// than in a separate flag that could disagree with the pole.
pub fn rate(world_seed: i64, index: usize) -> f64 {
    let index_i64 = index as i64; // cast-ok: plate index to i64 for the hash key
    let rate_key = [Part::Str("plate"), Part::Int(index_i64), Part::Str("rate")];
    let sense_key = [Part::Str("plate"), Part::Int(index_i64), Part::Str("sense")];
    let speed = SLOWEST_RAD_PER_MYR
        + fraction(world_seed, &rate_key) * (FASTEST_RAD_PER_MYR - SLOWEST_RAD_PER_MYR);
    // `turning = fraction < 0.5` is a discrete decision on a continuous quantity -- the
    // shape that has caused trouble throughout this port -- but it is safe here for a
    // better reason than usual: the quantity is *exactly* reproducible. It comes from a
    // byte-identical BLAKE2 digest through an integer-to-float conversion and a division
    // by an exact power of two, with no transcendental anywhere in the path, so Rust and
    // Python compare bit-identical values rather than two independently-rounded
    // approximations that could land on opposite sides of 0.5.
    let turning = fraction(world_seed, &sense_key) < 0.5;
    if turning {
        -speed
    } else {
        speed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pole_uses_the_non_normalising_constructor() {
        // The distinction is otherwise unguarded. `pole`'s vector is unit by
        // construction, so swapping the direct constructor for `from_vector` changes
        // no test's expectations and no conformance bound either -- pole goes through
        // cos/sin, so its differential comparison is bounded at 4 ULP and a
        // normalisation difference of about one ULP hides inside that.
        //
        // This pins it: rebuild the vector exactly as `pole` does, without
        // normalising, and require bit equality. `from_vector` divides by a length
        // that is 1.0 only to within a ULP or two, so it would move some component
        // for at least one of these indices.
        let mut checked = 0;
        for index in 0..16usize {
            let i = index as i64; // cast-ok: plate index to i64 for the hash key
            let z = 2.0 * fraction(20260831, &[Part::Str("plate"), Part::Int(i), Part::Str("pole-z")]) - 1.0;
            let angle = 2.0 * std::f64::consts::PI
                * fraction(20260831, &[Part::Str("plate"), Part::Int(i), Part::Str("pole-angle")]);
            let inner = 1.0 - z * z;
            let ring = m::sqrt(if inner > 0.0 { inner } else { 0.0 });
            let want = Vec3::new(m::cos(angle) * ring, m::sin(angle) * ring, z);
            let got = pole(20260831, index).vector;
            assert_eq!(got.x.to_bits(), want.x.to_bits(), "pole {index} x was normalised");
            assert_eq!(got.y.to_bits(), want.y.to_bits(), "pole {index} y was normalised");
            assert_eq!(got.z.to_bits(), want.z.to_bits(), "pole {index} z was normalised");
            checked += 1;
        }
        assert_eq!(checked, 16, "every index must have been compared");
    }

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
    fn poles_are_unit_vectors() {
        // pole() uses the direct, non-normalising SpherePoint constructor, so this is
        // not true by construction the way it is for spread() -- it is a genuine check
        // that the trig identity holds in floating point.
        for index in 0..22 {
            let p = pole(20260831, index);
            assert!((p.vector.length() - 1.0).abs() < 1e-9, "pole {index} is not unit");
        }
    }

    #[test]
    fn z_is_uniform_rather_than_latitude_uniform() {
        // Derivation of the sample size, so it isn't a round number picked by feel:
        //
        // If z came from sampling latitude uniformly instead (the bug the docstring
        // warns against), z = sin(latitude) would follow the arcsine distribution, whose
        // density 1/(pi * sqrt(1 - z^2)) diverges at the poles and dips at the equator.
        // Integrating that density over the outer bin [0.8, 1.0] gives about 20.5% of
        // samples (vs 10% expected if z were flat); over the centre bin [-0.1, 0.1] it
        // gives about 6.4% (vs 10%). A tolerance band of +/-30% around the flat 10%-per-
        // bin expectation -- i.e. counts within [7%, 13%] of N -- sits strictly between
        // "flat" and either of those bugged figures, so it distinguishes the two in
        // either direction: under the bug, the outer bins would overshoot the band and
        // the centre bins would undershoot it.
        //
        // For that band to almost never trip under the true, flat distribution, the
        // binomial standard deviation per bin (sqrt(N * 0.1 * 0.9)) must be small next
        // to the band's half-width (0.03 * N):
        //   5 * sqrt(0.09 * N) < 0.03 * N  =>  sqrt(N) > 50  =>  N > 2500.
        // N = 5000 clears that with room (5 sigma is ~106 samples against a 150-sample
        // half-width) while staying fast to run.
        const N: usize = 5000;
        const BINS: usize = 10;
        let bins_f = BINS as f64; // cast-ok: bin count to f64 for bucketing
        let mut counts = [0usize; BINS];
        for index in 0..N {
            let p = pole(20260831, index);
            let z = p.vector.z;
            let mut bin = ((z + 1.0) / 2.0 * bins_f) as usize; // cast-ok: fractional position to a bin index
            if bin >= BINS {
                bin = BINS - 1; // z == 1.0 exactly falls in the last bin
            }
            counts[bin] += 1;
        }
        let expected = N / BINS;
        let expected_f = expected as f64; // cast-ok: expected bin count to f64 for tolerance math
        let tolerance = (0.3 * expected_f) as usize; // cast-ok: a fraction back to a sample count
        for (bin, &count) in counts.iter().enumerate() {
            let low = expected - tolerance;
            let high = expected + tolerance;
            assert!(
                count >= low && count <= high,
                "bin {bin} has {count} samples, expected {expected} +/- {tolerance} \
                 (a bug sampling latitude uniformly would crowd z near the poles \
                 and thin it near the equator, pushing counts far outside this band)"
            );
        }
    }

    #[test]
    fn rates_are_within_the_configured_speed_range() {
        for index in 0..64 {
            let r = rate(20260831, index);
            assert!(
                r.abs() >= SLOWEST_RAD_PER_MYR && r.abs() <= FASTEST_RAD_PER_MYR,
                "rate {r} at index {index} out of [{SLOWEST_RAD_PER_MYR}, {FASTEST_RAD_PER_MYR}]"
            );
        }
    }

    #[test]
    fn both_signs_of_rate_occur_across_a_run_of_indices() {
        let mut saw_negative = false;
        let mut saw_positive = false;
        for index in 0..64 {
            let r = rate(20260831, index);
            if r < 0.0 {
                saw_negative = true;
            }
            if r > 0.0 {
                saw_positive = true;
            }
        }
        assert!(saw_negative && saw_positive, "expected both signs across 64 indices");
    }

    #[test]
    fn the_jitter_actually_moves_the_point() {
        // The production call site is `spread`, not `spread_impl` -- comparing two
        // `spread_impl` calls against each other (1.0 vs 0.0) only proves the parameter
        // works, not that `spread` passes 1.0. So the "with jitter" side goes through the
        // public `spread` function itself; only the "without" side reaches into
        // `spread_impl` to force the nudges to zero. If `spread` ever stopped passing
        // 1.0 -- dropped the argument, hardcoded 0.0, anything -- the two sides would
        // become identical and this assertion would fail.
        let jittered = spread(20260831, 5, 22);
        let unjittered = spread_impl(20260831, 5, 22, 0.0);
        let moved = jittered.vector.sub(&unjittered.vector).length();
        assert!(moved > 1e-6, "jitter moved the point by only {moved}");

        // States the wiring directly, in addition to the above's proof by consequence:
        // spread(...) is exactly spread_impl(..., 1.0), bit-for-bit.
        let via_impl = spread_impl(20260831, 5, 22, 1.0);
        assert_eq!(jittered.vector.x.to_bits(), via_impl.vector.x.to_bits());
        assert_eq!(jittered.vector.y.to_bits(), via_impl.vector.y.to_bits());
        assert_eq!(jittered.vector.z.to_bits(), via_impl.vector.z.to_bits());
    }
}
