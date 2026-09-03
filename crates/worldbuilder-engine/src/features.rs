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
//! (reused, not duplicated - see below), the `Feature` type with its `reach_m`, and now
//! `Placed` with its public `weight_at`. `Features` and `apply` are a later task.

use crate::detmath as m;
use crate::sphere::SpherePoint;
use crate::tangent::TangentFrame;

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

/// A feature with its frame worked out, ready to be asked about.
///
/// Notes:
/// The tangent frame is built once at construction. Building one per sample would have
/// made a handful of stamped features cost more than the entire tectonic system.
pub struct Placed {
    pub feature: Feature,
    pub frame: TangentFrame,
    along_e: f64,
    along_n: f64,
    across_e: f64,
    across_n: f64,
    /// `cos(min(PI, reach_m / radius_m))`, worked out once. Bounded, not strict - see
    /// `weight_at`'s doc comment - and load-bearing: do not replace the gate that
    /// compares against it with anything that claims to be equivalent.
    cos_reach: f64,
}

impl Placed {
    /// Args:
    /// feature: The thing being placed.
    /// radius_m: The planet's radius. The Python defaults this to `EARTH_RADIUS_M`; the
    /// Rust port takes it explicitly, matching `TangentFrame::at`'s own signature.
    pub fn new(feature: Feature, radius_m: f64) -> Self {
        let frame = TangentFrame::at(&feature.at, radius_m);
        let radians = m::to_radians(feature.bearing_deg);
        let along_e = m::sin(radians);
        let along_n = m::cos(radians);
        let across_e = m::cos(radians);
        let across_n = -m::sin(radians);
        let ratio = feature.reach_m() / radius_m;
        // Python writes `min(math.pi, ratio)`. CPython's two-argument `min` returns its
        // FIRST argument (here PI) whenever the comparison `ratio < PI` is false -
        // matching the house form in `plates.rs::margin_at` and this module's own `bump`
        // above: keep the second operand only when it is strictly less than the first.
        let bounded = if ratio < std::f64::consts::PI { ratio } else { std::f64::consts::PI };
        let cos_reach = m::cos(bounded);
        Self { feature, frame, along_e, along_n, across_e, across_n, cos_reach }
    }

    /// How strongly this feature applies here.
    ///
    /// Args:
    /// point: Where.
    ///
    /// Returns:
    /// weight: Nothing to one, smooth everywhere.
    ///
    /// Notes:
    /// **Public because `Placed` has two independent consumers.** `substrate.py` (not
    /// yet ported) calls `placed.weight_at(point)` directly and reads
    /// `placed.feature.substrate` without going through `Features::apply` at all, so
    /// this cannot be a private helper of `apply` - it is a first-class entry point in
    /// its own right, same as in the Python.
    ///
    /// **The reach gate below is LOAD-BEARING, not an optimisation to simplify away, and
    /// the size of what it guards against is shape-dependent - not a single module-wide
    /// number.** An earlier extraction claimed both branches - gated and ungated - give
    /// approximately zero weight everywhere, and treated the gate as a no-op. Measurement
    /// showed that claim false, and moreover that its own follow-up figure (a single
    /// worst-case bound) was itself only true of the one feature shape it was measured
    /// on. A ring scan around `reach_m` alone finds nothing (30,240 rejected points
    /// probed at a 1200x300 m feature, 0 leaks) - the leak lives in the *corner*, where
    /// `along` lands a hair inside `length_m` and `across` a hair inside `width_m` at
    /// the same time, so both `bump` factors are individually non-zero even though the
    /// true distance already exceeds `reach_m`. At that 1200x300 shape, 15,417 of
    /// 146,359 gate-rejected corner probes (10.53%) carried a non-zero ungated weight,
    /// worst ~1.7e-32 - small enough that it is invisible to `result` (an elevation
    /// tolerance would never notice a weight that small) and shows up only in
    /// `Features::apply`'s `authority`, which starts at a hard `0.0` where
    /// `max(0.0, tiny)` is `tiny`.
    ///
    /// **That smallness does not generalise.** The disagreement band the leak lives in
    /// is a fixed sliver of arc regardless of feature size, so the leaked weight scales
    /// roughly as `1 / (length_m^2 * width_m^2)`: measured worst leaked weight was
    /// ~4.09e-26 at a 150x90 m feature, and ~1.13e-12 at a 3x2 m feature - at which point
    /// it is no longer negligible against `result` itself (an ungated `result` of
    /// `-29.999999999970655` against an exact `-30.0` has been measured at that shape,
    /// and a one-ULP nudge to the threshold moved a real `apply` result by ~3.75e-10 m,
    /// about 105,470 ULP). So for a small feature this gate protects `result`, not only
    /// `authority`. The gate is transcribed exactly regardless of shape: same
    /// `dot(...) < cos_reach` comparison, same early `return 0.0`, for every size of
    /// feature this module is ever asked to place.
    ///
    /// `cos_reach` is itself `cos(hypot(length_m, width_m) / radius_m)` bounded by PI -
    /// two bounded transcendental calls, not a strict quantity - and moving it by a
    /// single ULP has been measured to reclassify a double-digit percentage of probe
    /// points at the 1200x300 shape (21.6%). `weight_at` reaches only `atan2` and `sqrt`
    /// beyond that (through `sphere_to_local`), never `local_to_sphere`'s
    /// `hypot`/`cos`/`sin` - the two tangent-frame directions have different
    /// transcendental profiles and this one is the cheaper of the two.
    pub fn weight_at(&self, point: &SpherePoint) -> f64 {
        if point.vector.dot(&self.feature.at.vector) < self.cos_reach {
            return 0.0;
        }
        let (east, north) = self.frame.sphere_to_local(point);
        let along = east * self.along_e + north * self.along_n;
        let across = east * self.across_e + north * self.across_n;
        bump(along, self.feature.length_m) * bump(across, self.feature.width_m)
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

    use crate::sphere::EARTH_RADIUS_M;

    /// The probe geometry Task 1's harness used: feature at 12.34 N, 56.78 W,
    /// target_m = -4.0, length_m = 1200.0, width_m = 300.0, bearing_deg = 37.0,
    /// radius_m = EARTH_RADIUS_M.
    fn probe_feature() -> Feature {
        Feature {
            kind: "test".to_string(),
            at: SpherePoint::from_latlon(12.34, 56.78),
            target_m: -4.0,
            length_m: 1200.0,
            width_m: 300.0,
            bearing_deg: 37.0,
            compose: RAISE.to_string(),
            marked: false,
            substrate: None,
        }
    }

    fn probe_placed() -> Placed {
        Placed::new(probe_feature(), EARTH_RADIUS_M)
    }

    #[test]
    fn new_matches_the_python_pinned_along_across_and_cos_reach() {
        // python (live run against worldbuilder/bathymetry/features.py, Placed(probe_feature())):
        // _along_e=0.6018150231520483 _along_n=0.7986355100472928
        // _across_e=0.7986355100472928 _across_n=-0.6018150231520483
        // _cos_reach=0.9999999811528267 (bits 0x3feffffff5e1aa0b)
        // Bounded, not strict in principle: `radians` then `sin`/`cos` for the axis
        // pairs, and `hypot` then `cos` for `_cos_reach`, are all transcendental calls
        // and are not guaranteed to agree with CPython's libm. Measured here (this
        // crate's own libm build against this exact probe): 0 ULP apart on all five -
        // an exact coincidence at this input, asserted as such rather than assumed, the
        // same house style as `reach_m_agrees_with_python_hypot_where_the_algorithms_
        // happen_to_coincide` above. A genuine bound, if this ever needs one for a
        // different input, belongs in Task 5's conformance sweep, not guessed here.
        let placed = probe_placed();
        assert_eq!(placed.along_e, 0.6018150231520483_f64);
        assert_eq!(placed.along_n, 0.7986355100472928_f64);
        assert_eq!(placed.across_e, 0.7986355100472928_f64);
        assert_eq!(placed.across_n, -0.6018150231520483_f64);
        assert_eq!(placed.cos_reach, 0.9999999811528267_f64);
    }

    #[test]
    fn weight_at_is_exactly_one_at_the_feature_s_own_centre() {
        // python: Placed(probe_feature()).weight_at(probe_feature().at) == 1.0 exactly.
        // The dot product of a point with itself is exactly 1.0, well past the gate;
        // sphere_to_local of the origin returns (0.0, 0.0) exactly (the DEGENERATE
        // short-circuit), so both projections are exactly zero and both `bump` calls
        // reduce to `smooth(1.0) == 1.0` with no transcendental involved at all.
        let feature = probe_feature();
        let at = feature.at;
        let placed = Placed::new(feature, EARTH_RADIUS_M);
        assert_eq!(placed.weight_at(&at), 1.0);
    }

    #[test]
    fn weight_at_is_a_hard_zero_far_outside_reach() {
        // python: Placed(probe_feature()).weight_at(SpherePoint.from_latlon(-40.0, 130.0))
        // == 0.0, rejected by the dot-product gate (dot=0.07867405809750846, far below
        // cos_reach=0.9999999811528267).
        let placed = probe_placed();
        let far = SpherePoint::from_latlon(-40.0, 130.0);
        assert_eq!(placed.weight_at(&far), 0.0);
    }

    #[test]
    fn weight_at_a_nearby_probe_point_is_bounded_against_python() {
        // python: Placed(probe_feature()).weight_at(SpherePoint.from_latlon(12.3405, 56.7795))
        // == 0.8365730417848936 (bits 0x3feac534d3e5cdb2). Bounded in principle: this
        // path reaches atan2 + sqrt through sphere_to_local (Task 1), so cross-language
        // exactness is not guaranteed. Measured here: 0 ULP apart - an exact coincidence
        // at this probe, not a general guarantee (see `new_matches_the_python_pinned_
        // along_across_and_cos_reach` above for the same caveat).
        let placed = probe_placed();
        let probe = SpherePoint::from_latlon(12.3405, 56.7795);
        let expected = 0.8365730417848936_f64;
        let actual = placed.weight_at(&probe);
        assert_eq!(
            actual, expected,
            "got bits {:#x}, expected {:#x}", actual.to_bits(), expected.to_bits()
        );
    }

    /// **The reach-gate corner, not the ring - and the leak is shape-dependent, not a
    /// module-wide constant.** Task 1 measured that a ring scan around `reach_m` (2,000
    /// azimuths x 16 shapes, 30,240 gate-rejected points) finds zero leaks, and that the
    /// false "both branches are ~zero" claim almost certainly came from exactly that
    /// scan. The actual leak lives in the corner: a point where `along` sits a hair
    /// inside `length_m` **and** `across` sits a hair inside `width_m` at the same time,
    /// so both `bump` factors are individually non-zero even though the point's true
    /// distance already exceeds `reach_m`.
    ///
    /// A later review reproducing Task 1's numbers found the leak magnitude scales
    /// roughly as `1 / (length_m^2 * width_m^2)`, because the disagreement band is a
    /// fixed ~micron of arc regardless of feature size: measured worst leaked weight was
    /// ~1.7e-32 at 1200x300 (the shape Task 1 profiled), ~4.09e-26 at 150x90, and
    /// ~1.13e-12 at 3x2 - at which point it is no longer negligible against `result`
    /// (the review found an ungated `result` of -29.999999999970655 against an exact
    /// -30.0 at that shape, and a worst `|delta result|` of ~3.75e-10 m, ~105,470 ULP,
    /// from a one-ULP nudge to the threshold). **So this is not "moves authority off
    /// zero and nothing else" in general - that was only true at the one shape Task 1
    /// measured.** The gate is transcribed exactly regardless of shape; this test uses a
    /// small feature (3x2) specifically because that is where the leak is large enough
    /// for a test tolerance to mean anything - at 1200x300 the effect (~1e-32 to ~1e-44)
    /// hides under any plausible float comparison and the test would prove nothing.
    ///
    /// This reproduces the leak directly against this crate's own implementation, rather
    /// than importing a Python-computed coordinate, which the reach gate's own 1-ULP
    /// sensitivity (a 21.6% reclassification rate at the 1200x300 shape, per Task 1)
    /// would make brittle across the two languages' transcendentals.
    #[test]
    fn weight_at_gates_out_a_shape_dependent_corner_leak_that_a_ring_scan_would_miss() {
        let feature = Feature {
            kind: "test".to_string(),
            at: SpherePoint::from_latlon(12.34, 56.78),
            target_m: -4.0,
            length_m: 3.0,
            width_m: 2.0,
            bearing_deg: 37.0,
            compose: RAISE.to_string(),
            marked: false,
            substrate: None,
        };
        let placed = Placed::new(feature.clone(), EARTH_RADIUS_M);

        let mut leak: Option<(f64, f64, f64, f64)> = None;
        'search: for &span in &[1e-13_f64, 1e-12, 1e-11, 1e-10, 1e-9, 1e-8] {
            for i in -5..=5_i32 {
                for j in -5..=5_i32 {
                    if i == 0 && j == 0 {
                        continue;
                    }
                    let along_val = feature.length_m * (1.0 - f64::from(i) * span);
                    let across_val = feature.width_m * (1.0 - f64::from(j) * span);
                    let east = along_val * placed.along_e + across_val * placed.across_e;
                    let north = along_val * placed.along_n + across_val * placed.across_n;
                    let point = placed.frame.local_to_sphere(east, north);

                    let gated = placed.weight_at(&point);

                    let (e2, n2) = placed.frame.sphere_to_local(&point);
                    let along2 = e2 * placed.along_e + n2 * placed.along_n;
                    let across2 = e2 * placed.across_e + n2 * placed.across_n;
                    let ungated = bump(along2, feature.length_m) * bump(across2, feature.width_m);

                    let dot = point.vector.dot(&feature.at.vector);
                    let is_gate_rejected = dot < placed.cos_reach;

                    if is_gate_rejected && ungated != 0.0 {
                        leak = Some((gated, ungated, dot, placed.cos_reach));
                        break 'search;
                    }
                }
            }
        }

        let (gated, ungated, dot, cos_reach) = leak.expect(
            "expected to find at least one reach-gate corner leak on a small feature,              matching the (shape-dependent) measurement that small shapes leak far more              than the 1200x300 shape Task 1 profiled",
        );
        assert_eq!(gated, 0.0, "the gate must still return a hard zero");
        assert!(
            ungated > 0.0,
            "expected a strictly positive ungated weight at the corner, got {ungated}"
        );
        assert!(dot < cos_reach, "the point must genuinely be gate-rejected");
    }
}
