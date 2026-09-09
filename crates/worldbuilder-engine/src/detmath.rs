//! The only place in this crate that may call a transcendental function.
//!
//! std's maths dispatches to the platform's libm, and the platform differs between a
//! native host and a WASM runtime. Slice 0 measured the consequence: native `f64::sin`
//! against `libm::sin` in WASM diverged on 2,441 of 100,000 samples, each by a single
//! bit. Coastlines are decided by last bits, so every call goes through here.
//!
//! `sqrt` is **routed through here like everything else, and is the one routed operation
//! whose body is std rather than `libm`.** That distinction is the whole of the exception,
//! and it is deliberately expressed *here* rather than in the guard:
//!
//! - The rule stays "no std maths outside detmath, ever". `tests/no_std_math.rs` is
//!   **unchanged** — it still bans `.sqrt(` and `f64::sqrt(` in every other file in the
//!   crate, and it skips `detmath.rs` as it always has. There is no new hole in the guard,
//!   and no new exempt name for a later slice to widen. `sqrt` did not stop being routed;
//!   only what `detmath::sqrt` calls changed, in one line, in the one file the guard already
//!   treats as the choke point.
//! - **`sqrt` is safe where `sin`/`cos`/`atan2`/`pow`/`exp` are not, and the difference is in
//!   the standard rather than in somebody's judgement.** IEEE-754 requires square root to be
//!   *correctly rounded*: the result is the exactly-rounded value of the true root, so every
//!   conforming implementation — the x86 `SQRTSD` instruction, the WASM `f64.sqrt` opcode,
//!   and any correct software routine — returns the identical bit pattern. The
//!   transcendentals carry no such requirement (the table-maker's dilemma is why), which is
//!   exactly what slice 0 measured when native `f64::sin` and `libm::sin` in WASM diverged on
//!   2,441 of 100,000 samples. **The pure-`libm` rule exists to protect the functions that
//!   have no correctness guarantee. `sqrt` has one.**
//! - **Why bother.** `libm 0.2.11`'s software `sqrt` compiles to a real called function in
//!   the WASM build instead of lowering to the `f64.sqrt` instruction, and the performance
//!   profile measured it as **the single hottest function in the engine** — 76.2% of the
//!   water solve and 35.4% of a relief tile fill, 93.2% of the water figure arriving through
//!   `Vec3::length` below `stream::nearest_neighbours`.
//!
//! **The specification argument is not taken on trust.** Two independent checks stand behind
//! it, and both are in the tree:
//!
//! 1. `sqrt_is_bit_identical_to_libm_across_five_magnitude_scales` below compares this
//!    function against `libm::sqrt` over a million values plus the edge cases. That is
//!    *native only* — `cargo test` never runs on wasm32 — so on its own it proves half.
//! 2. The other half is the **native-against-WASM parity harness**
//!    (`crates/worldbuilder-engine/parity/`), which is the instrument this rule was written
//!    for. Its 126,359-value corpus was byte-for-byte unchanged natively by this edit, and
//!    replayed through the rebuilt `.wasm` at 0 divergent with all six mutation controls
//!    unmoved. Chained, those give WASM-after == native-after == native-before ==
//!    WASM-before.
//!
//! **One thing IEEE-754 does not promise, stated here so nobody rediscovers it as a bug:**
//! the *payload* of the NaN returned for a negative argument. `sqrt(-1.0)` is a NaN either
//! way, but which NaN is unspecified — so the test below compares `is_nan()` there and bit
//! patterns everywhere else. Nothing in this crate takes the root of a negative: every call
//! site is a sum of squares, an area, or a literal, and the whole-corpus byte-identity above
//! is the evidence that none does.

/// Radians per degree, and degrees per radian, as explicit constants rather than std's
/// `to_radians`/`to_degrees`, so the conversion is visible and identical on both targets.
const RAD_PER_DEG: f64 = std::f64::consts::PI / 180.0;
const DEG_PER_RAD: f64 = 180.0 / std::f64::consts::PI;

pub fn sin(x: f64) -> f64 {
    libm::sin(x)
}

pub fn cos(x: f64) -> f64 {
    libm::cos(x)
}

/// The one routed operation whose body is std rather than `libm`. See the module doc for the
/// IEEE-754 argument, the two checks that stand behind it, and why the guard needed no
/// change to accommodate it.
pub fn sqrt(x: f64) -> f64 {
    x.sqrt()
}

pub fn hypot(x: f64, y: f64) -> f64 {
    libm::hypot(x, y)
}

pub fn atan2(y: f64, x: f64) -> f64 {
    libm::atan2(y, x)
}

pub fn asin(x: f64) -> f64 {
    libm::asin(x)
}

/// Used by `erosion.rs`'s slope cap to compute `tan(30 degrees)` once, at startup, rather
/// than comparing against an angle per edge (which would need `atan` per edge instead of a
/// single division-free comparison against a precomputed slope threshold).
pub fn tan(x: f64) -> f64 {
    libm::tan(x)
}

pub fn tanh(x: f64) -> f64 {
    libm::tanh(x)
}

pub fn powf(x: f64, y: f64) -> f64 {
    libm::pow(x, y)
}

/// `e^x`. Added for `climate.rs`'s upwind moisture march, whose orographic rain-out and
/// over-water recharge are both exponential relaxations.
///
/// It is routed here for the usual reason and for one extra worth stating: `exp(-x)` for a
/// non-negative `x` lands in `(0, 1]` **by arithmetic**, so a moisture written as a product
/// of such factors stays in range with **no clamp** -- and `f64::min`/`f64::max`/`.clamp(`
/// are NaN-asymmetric and banned in this crate for exactly the reason the march would have
/// needed them. A NaN argument comes back NaN, which is the march's stated contract.
pub fn exp(x: f64) -> f64 {
    libm::exp(x)
}

/// Floors toward negative infinity, which is what Python's `int(x // 1)` does and what
/// `as i64` does NOT do. Never derive a lattice coordinate with a cast.
pub fn floor(x: f64) -> f64 {
    libm::floor(x)
}

/// Natural log. Added for `relief_survey.rs`'s Hurst-exponent group
/// `H = ln(1/persistence) / ln(lacunarity)` -- the first transcendental this crate has
/// needed for a *report*, not a generator path, but the same divergence risk applies
/// (native vs WASM libm), so it goes through here rather than being a one-off `std`
/// call in a `src/bin` file the guard would have to special-case.
pub fn ln(x: f64) -> f64 {
    libm::log(x)
}

pub fn to_radians(degrees: f64) -> f64 {
    degrees * RAD_PER_DEG
}

pub fn to_degrees(radians: f64) -> f64 {
    radians * DEG_PER_RAD
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_op_is_routed_and_finite() {
        assert!(sin(0.7).is_finite());
        assert!(cos(0.7).is_finite());
        assert!(sqrt(2.0).is_finite());
        assert!(hypot(3.0, 4.0).is_finite());
        assert!(atan2(1.0, 2.0).is_finite());
        assert!(asin(0.5).is_finite());
        assert!(tan(0.5).is_finite());
        assert!(tanh(0.5).is_finite());
        assert!(powf(2.0, 0.5).is_finite());
        assert!(floor(-2.3).is_finite());
        // `ln` was added for `relief_survey.rs`'s Hurst-exponent group; checked here
        // rather than in its own test so this task does not disturb the engine's pinned
        // test counts beyond what `relief_survey.rs` itself needs (see task-2-report.md).
        assert!(ln(2.0).is_finite());
        assert!((ln(std::f64::consts::E) - 1.0).abs() < 1e-12);
        // `exp` was added for `climate.rs`'s moisture march; checked here rather than in
        // its own test so the march does not disturb the crate's five pinned counts by
        // more than the assertions it actually needs.
        assert!(exp(1.0).is_finite());
        assert!((exp(1.0) - std::f64::consts::E).abs() < 1e-12);
        assert_eq!(exp(0.0).to_bits(), 1.0f64.to_bits());
        // The property the march relies on instead of a clamp: a non-positive argument
        // never leaves (0, 1], and the far tail underflows to zero rather than to a
        // negative moisture.
        assert!(exp(-1e-300) <= 1.0 && exp(-1e-300) > 0.0);
        assert_eq!(exp(f64::NEG_INFINITY).to_bits(), 0.0f64.to_bits());
        assert!(exp(f64::NAN).is_nan());
    }

    /// **The evidence for the one exception in this module**, native half.
    ///
    /// `sqrt` is the only routed operation whose body is std rather than `libm`, on the
    /// grounds that IEEE-754 requires square root to be correctly rounded and therefore
    /// leaves no implementation freedom to disagree about. This is that claim as a
    /// measurement: `libm::sqrt` (what this function called before) against `x.sqrt()` (what
    /// it calls now), **on bit patterns**, over 1,000,000 pseudo-random values spread across
    /// five magnitude scales -- 1e-8, 1e-2, 1e0, 1e6 and 1e18, chosen to bracket everything
    /// this crate actually roots: normalised direction components near 1, squared-distance
    /// sums, node areas in m^2, and planetary radii squared.
    ///
    /// **Scoped to `sqrt` and nothing else.** `sin`, `cos`, `tan`, `atan2`, `asin`, `tanh`,
    /// `powf`, `exp`, `ln`, `hypot` and `floor` all still call `libm` and must keep doing so;
    /// none of them is correctly rounded, and the assertion below deliberately says nothing
    /// about any of them.
    ///
    /// The generator is a `wasm32` target where this test is not, so this proves the native
    /// half only. The WASM half is the parity harness -- see the module doc.
    #[test]
    fn sqrt_is_bit_identical_to_libm_across_five_magnitude_scales() {
        // A local splitmix64 rather than a dependency: this file must not grow one, and the
        // population only has to be spread, not cryptographic.
        fn splitmix64(state: &mut u64) -> u64 {
            *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = *state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }

        const SCALES: [f64; 5] = [1.0e-8, 1.0e-2, 1.0, 1.0e6, 1.0e18];
        const PER_SCALE: u32 = 200_000;

        let mut state: u64 = 0x5150_5F53_5152_5400;
        let mut compared: u64 = 0;
        for scale in SCALES {
            for _ in 0..PER_SCALE {
                // A uniform (0, 1] mantissa times the scale: 53 random bits divided by 2^53.
                // cast-ok: a 53-bit integer to the f64 that represents it exactly, not a
                // float truncation.
                let unit = ((splitmix64(&mut state) >> 11) as f64) / 9_007_199_254_740_992.0;
                let x = unit * scale;
                assert_eq!(
                    libm::sqrt(x).to_bits(),
                    sqrt(x).to_bits(),
                    "libm::sqrt and the routed sqrt disagree at {x:e} -- IEEE-754 requires \
                     square root to be correctly rounded, so this cannot happen unless one \
                     of them is not conforming",
                );
                compared += 1;
            }
        }
        assert_eq!(
            compared,
            u64::from(PER_SCALE) * (SCALES.len() as u64), // cast-ok: an array length to u64
            "the population must be the size this test's doc claims it is",
        );

        // The edges, where a correctly-rounded operation is most likely to be sloppy.
        for x in [
            0.0f64,
            -0.0f64,
            f64::MIN_POSITIVE,
            f64::MIN_POSITIVE / 2.0, // subnormal
            5.0e-324,                // the smallest subnormal
            1.0,
            2.0,
            4.0,
            f64::MAX,
            f64::INFINITY,
        ] {
            assert_eq!(
                libm::sqrt(x).to_bits(),
                sqrt(x).to_bits(),
                "libm::sqrt and the routed sqrt disagree at the edge case {x:e}",
            );
        }

        // NaN in, NaN out -- but IEEE-754 does NOT specify the payload, so this is the one
        // place the comparison is deliberately weaker than a bit comparison. Asserted rather
        // than left implicit, because a future reader finding `is_nan()` here should find the
        // reason beside it. Nothing in this crate roots a negative.
        assert!(libm::sqrt(-1.0).is_nan() && sqrt(-1.0).is_nan());
        assert!(libm::sqrt(f64::NAN).is_nan() && sqrt(f64::NAN).is_nan());
        assert!(libm::sqrt(f64::NEG_INFINITY).is_nan() && sqrt(f64::NEG_INFINITY).is_nan());
    }

    #[test]
    fn floor_goes_down_not_towards_zero() {
        // The trap this module exists to close. Python's int(x // 1) floors;
        // Rust's `as i64` truncates. For negative coordinates they disagree.
        assert_eq!(floor(-2.3), -3.0);
        assert_eq!(floor(-1e-9), -1.0);
        assert_eq!(floor(-1.0), -1.0);
        assert_eq!(floor(2.3), 2.0);
    }

    #[test]
    fn degrees_and_radians_round_trip_exactly_at_the_landmarks() {
        assert_eq!(to_radians(180.0).to_bits(), std::f64::consts::PI.to_bits());
        assert_eq!(to_degrees(std::f64::consts::PI).to_bits(), 180.0f64.to_bits());
    }
}
