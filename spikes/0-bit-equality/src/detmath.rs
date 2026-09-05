//! The only place in this crate that may call a transcendental function.
//!
//! std's trigonometry dispatches to the platform's libm, and the platform differs between a
//! native host and a WASM runtime. The differences are in the last bits, which is precisely
//! where a coastline is decided. Routing every call through the pure-Rust `libm` crate means
//! both targets execute the same instructions over the same values.
//!
//! `sqrt` is included even though IEEE-754 requires it to be correctly rounded, and is
//! therefore safe. It is routed anyway so that the rule is "no std math, ever" rather than
//! "no std math except the ones somebody judged safe" - a rule with an exception list is a
//! rule that erodes.

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

pub fn tanh(x: f64) -> f64 {
    libm::tanh(x)
}

pub fn powf(x: f64, y: f64) -> f64 {
    libm::pow(x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_op_is_routed_and_returns_a_finite_value() {
        assert!(sin(0.7).is_finite());
        assert!(cos(0.7).is_finite());
        assert!(sqrt(2.0).is_finite());
        assert!(hypot(3.0, 4.0).is_finite());
        assert!(atan2(1.0, 2.0).is_finite());
        assert!(asin(0.5).is_finite());
        assert!(tanh(0.5).is_finite());
        assert!(powf(2.0, 0.5).is_finite());
    }

    #[test]
    fn hypot_of_three_and_four_is_exactly_five() {
        assert_eq!(hypot(3.0, 4.0).to_bits(), 5.0f64.to_bits());
    }
}
