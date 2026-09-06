//! The only place in this crate that may call a transcendental function.
//!
//! std's maths dispatches to the platform's libm, and the platform differs between a
//! native host and a WASM runtime. Slice 0 measured the consequence: native `f64::sin`
//! against `libm::sin` in WASM diverged on 2,441 of 100,000 samples, each by a single
//! bit. Coastlines are decided by last bits, so every call goes through here.
//!
//! `sqrt` is routed even though IEEE-754 requires it to be correctly rounded, so that the
//! rule is "no std maths, ever" rather than "no std maths except the ones somebody judged
//! safe" — a rule with an exception list is a rule that erodes.

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

pub fn sqrt(x: f64) -> f64 {
    libm::sqrt(x)
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
