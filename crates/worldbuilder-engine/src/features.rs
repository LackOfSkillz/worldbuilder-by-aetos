//! Specific things that must exist, put where somebody wants them.
//!
//! Ported from `worldbuilder/bathymetry/features.py`. Everything before this decides what
//! ordinary ground looks like. This is the other channel: a short list of named features,
//! stamped at chosen places, because a test region needs a channel *here* and a bar across
//! *that* harbour mouth, and no amount of noise will oblige.
//!
//! **Composition is explicit.** A feature says what the ground should be and how it should
//! argue with what is already there. A bank raises the seabed and must not deepen it; a
//! channel carves and must not fill; a reef stands proud of whatever it sits on.
//!
//! This module carries the module constants, `bump` (new to this crate) and `smooth`
//! (reused, not duplicated - see below), and the `Feature` type with its `reach_m`.
//! `Placed`, `Features`, `weight_at` and `apply` are later tasks.

use crate::detmath as m;
use crate::sphere::SpherePoint;

/// How a feature argues with the ground it is placed on.
pub const RAISE: &str = "raise";
pub const CARVE: &str = "carve";
pub const SHAPE: &str = "shape";

/// How much relief a feature must be asserting before it takes the ground's texture away
/// from it. Three metres, because that is below the relief of the shallowest thing worth
/// placing.
pub const SETTLE_M: f64 = 3.0;

/// `max(0.0, min(1.0, fraction))` then the smoothstep `x * x * (3.0 - 2.0 * x)`.
///
/// `features.py`'s `_smooth` is exactly `detail.py`'s `_smooth` - `max(0.0, min(1.0,
/// fraction))` then the same smoothstep, in the same operand order - so it is reused from
/// `detail` here rather than adding a third copy of an identical function (`shelf.rs`
/// already reuses it the same way).
pub use crate::detail::smooth;

/// One at the middle, nothing at the edge, flat at both ends.
///
/// Args:
/// distance_m: How far from the middle of the feature.
/// half_m: Where it reaches zero.
///
/// Notes:
/// `tectonics.rs` has a `bump(distance_m, width_m)` that is mathematically the same
/// curve, but it inlines the smoothstep itself rather than calling `smooth`, and it is
/// private to that module. `features.py`'s `_bump` explicitly composes `_smooth`, so
/// this is written the same way - `smooth(1.0 - min(1.0, abs(distance_m) / half_m))` -
/// rather than reusing `tectonics::bump`'s inlined variant.
pub fn bump(distance_m: f64, half_m: f64) -> f64 {
    if half_m <= 0.0 {
        return 0.0;
    }
    let raw = distance_m.abs() / half_m;
    // Python writes `min(1.0, abs(distance_m) / half_m)`; keep the second operand unless
    // it is not strictly less than the first, matching the house form in
    // `plates.rs::margin_at`.
    let away = if raw < 1.0 { raw } else { 1.0 };
    smooth(1.0 - away)
}

/// One placed thing.
///
/// Attributes:
/// kind: What it is called, for diagnostics and for chart symbols.
/// at: Its middle.
/// target_m: The elevation it wants the ground to be at its middle.
/// length_m: How far it reaches along its bearing. Equal to the width for something
/// round.
/// width_m: How far it reaches either side of its bearing.
/// bearing_deg: Which way it runs, degrees true.
/// compose: `RAISE`, `CARVE` or `SHAPE`.
/// marked: Whether a chart should carry a symbol for it regardless of what the
/// soundings say.
/// substrate: What it is made of, if it overrules the ordinary bottom. `None` leaves
/// the bottom to be derived from the shape of the ground, which is right for a bank
/// but wrong for a rock.
///
/// Notes:
/// `substrate` is an `Option<String>` used as an `is None` sentinel by `substrate.py`
/// (not yet ported) - the *third* optional-argument idiom in this codebase, and not
/// interchangeable with the other two: `radius_m=EARTH_RADIUS_M` elsewhere is plain
/// default substitution, and `detail.py` used a falsy check where `0.0` counted as
/// absent. Here, `None` specifically means "derive it", and an empty string would not
/// mean the same thing - so this is a sentinel, matched with `.is_none()` /
/// `.is_some()`, never with a falsy/truthy check.
#[derive(Debug, Clone, PartialEq)]
pub struct Feature {
    pub kind: String,
    pub at: SpherePoint,
    pub target_m: f64,
    pub length_m: f64,
    pub width_m: f64,
    pub bearing_deg: f64,
    pub compose: String,
    pub marked: bool,
    pub substrate: Option<String>,
}

impl Feature {
    /// Beyond this the bump is exactly nothing, so nothing need be evaluated.
    ///
    /// `math.hypot(length_m, width_m)` in the Python. `hypot` is a bounded call, not a
    /// strict one: since Python 3.8 CPython computes its own Neumaier-summed norm rather
    /// than calling libm, so this and `detmath::hypot` (`libm::hypot`) are different
    /// algorithms and are not expected to agree bit-for-bit in general - only measured to
    /// agree on some inputs by chance. See the module tests for measured drift.
    pub fn reach_m(&self) -> f64 {
        m::hypot(self.length_m, self.width_m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point() -> SpherePoint {
        SpherePoint::from_latlon(12.34, 56.78)
    }

    fn feature(length_m: f64, width_m: f64) -> Feature {
        Feature {
            kind: "test".to_string(),
            at: point(),
            target_m: -5.0,
            length_m,
            width_m,
            bearing_deg: 0.0,
            compose: RAISE.to_string(),
            marked: false,
            substrate: None,
        }
    }

    #[test]
    fn constants_match_the_python_character_for_character() {
        assert_eq!(RAISE, "raise");
        assert_eq!(CARVE, "carve");
        assert_eq!(SHAPE, "shape");
        assert_eq!(SETTLE_M, 3.0);
    }

    #[test]
    fn smooth_matches_the_python_pinned_values() {
        // python: _smooth(-0.5)=0.0, _smooth(0.0)=0.0, _smooth(0.25)=0.15625,
        // _smooth(0.5)=0.5, _smooth(0.75)=0.84375, _smooth(1.0)=1.0, _smooth(1.5)=1.0
        assert_eq!(smooth(-0.5), 0.0);
        assert_eq!(smooth(0.0), 0.0);
        assert_eq!(smooth(0.25), 0.15625);
        assert_eq!(smooth(0.5), 0.5);
        assert_eq!(smooth(0.75), 0.84375);
        assert_eq!(smooth(1.0), 1.0);
        assert_eq!(smooth(1.5), 1.0);
    }

    #[test]
    fn bump_matches_the_python_pinned_values() {
        // python: _bump(0.0,100.0)=1.0, _bump(50.0,100.0)=0.5, _bump(100.0,100.0)=0.0,
        // _bump(150.0,100.0)=0.0, _bump(-50.0,100.0)=0.5, _bump(10.0,0.0)=0.0,
        // _bump(10.0,-5.0)=0.0
        assert_eq!(bump(0.0, 100.0), 1.0);
        assert_eq!(bump(50.0, 100.0), 0.5);
        assert_eq!(bump(100.0, 100.0), 0.0);
        assert_eq!(bump(150.0, 100.0), 0.0);
        assert_eq!(bump(-50.0, 100.0), 0.5);
        assert_eq!(bump(10.0, 0.0), 0.0);
        assert_eq!(bump(10.0, -5.0), 0.0);
    }

    #[test]
    fn reach_m_is_hypot_of_length_and_width() {
        // python: Feature(length_m=300.0, width_m=400.0).reach_m() == 500.0 exactly
        assert_eq!(feature(300.0, 400.0).reach_m(), 500.0);
        // python: Feature(length_m=0.0, width_m=0.0).reach_m() == 0.0
        assert_eq!(feature(0.0, 0.0).reach_m(), 0.0);
        // python: Feature(length_m=-300.0, width_m=400.0).reach_m() == 500.0 (hypot is
        // symmetric in sign)
        assert_eq!(feature(-300.0, 400.0).reach_m(), 500.0);
    }

    #[test]
    fn reach_m_agrees_with_python_hypot_where_the_algorithms_happen_to_coincide() {
        // python: math.hypot(1200.0, 300.0) == 1236.9316876852981 (0x409353ba0c5629c4).
        // Task 1 measured this exact pair as one where CPython's Neumaier-summed hypot
        // and a libm hypot agree bit-for-bit; confirmed again here for this crate's own
        // libm build.
        assert_eq!(feature(1200.0, 300.0).reach_m(), 1236.931_687_685_298_1);
    }

    #[test]
    fn reach_m_is_bounded_not_strict_against_python_hypot() {
        // python: math.hypot(123456.789, 987.654321) == 123460.73955411215
        // (0x1.e244bd536b155p+16). CPython's Neumaier-summed hypot and libm::hypot are
        // different algorithms (Task 1); this pins the measured drift for this input
        // rather than assuming exactness or picking a round tolerance.
        let expected = 123460.739_554_112_15_f64;
        let actual = feature(123456.789, 987.654321).reach_m();
        let diff_ulps = (actual.to_bits() as i64 // cast-ok: bit reinterpretation for a ULP distance, not a float truncation
            - expected.to_bits() as i64) // cast-ok: bit reinterpretation for a ULP distance, not a float truncation
        .abs();
        assert!(
            diff_ulps <= 1,
            "expected {expected} ({:#x}), got {actual} ({:#x}), {diff_ulps} ULP apart",
            expected.to_bits(),
            actual.to_bits()
        );
    }

    #[test]
    fn substrate_defaults_to_none_as_a_sentinel_not_a_falsy_check() {
        let f = feature(1.0, 1.0);
        assert!(f.substrate.is_none());
    }
}
