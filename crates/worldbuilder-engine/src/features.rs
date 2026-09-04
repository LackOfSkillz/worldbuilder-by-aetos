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
pub const SETTLE_M: f64 = 3.5; // DELIBERATE BREAKAGE (slice CI, task 2)

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
    /// **That smallness does not generalise, and it falls away far faster than a first
    /// reading suggested.** The band the leak lives in is where `dot` and `cos_reach`,
    /// both within an ULP of 1.0, stop resolving distance at all, so its width in metres
    /// runs as `ULP(1.0) * radius_m^2 / reach_m` - it *shrinks* as the feature grows
    /// rather than staying a fixed sliver of arc. Scanned in absolute insets, the worst
    /// leaked weight measured **1.2047e-12 at 3x2**, **1.1055e-26 at 150x90** and
    /// **8.4188e-32 at 1200x300**: a fall-off of roughly a fourth power in each of
    /// `length_m` and `width_m`, not the `1 / (length_m^2 * width_m^2)` recorded earlier
    /// in this slice from a relative-span grid that never reached the band's widest part.
    /// At 3x2 the leak is no longer negligible against `result` itself (an ungated
    /// `result` of `-29.999999999970655` against an exact `-30.0` has been measured at
    /// that shape, and a one-ULP nudge to the threshold moved a real `apply` result by
    /// ~3.75e-10 m, about 105,470 ULP). So for a small feature this gate protects
    /// `result`, not only `authority`. The gate is transcribed exactly regardless of
    /// shape: same `dot(...) < cos_reach` comparison, same early `return 0.0`, for every
    /// size of feature this module is ever asked to place.
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

/// Every placed feature on a world.
///
/// Notes:
/// A list, iterated. There are a dozen of these, not a million: they are the things
/// somebody deliberately put somewhere, and a world wanting thousands has wanted a
/// generator rather than a stamp.
pub struct Features {
    /// Matches Python's `self.placed` by name: a tuple of `Placed`, one per feature
    /// given to `new`, built once and kept in the order given.
    pub placed: Vec<Placed>,
    pub radius_m: f64,
}

impl Features {
    pub fn new(features: impl IntoIterator<Item = Feature>, radius_m: f64) -> Self {
        let placed = features.into_iter().map(|feature| Placed::new(feature, radius_m)).collect();
        Self { placed, radius_m }
    }

    pub fn len(&self) -> usize {
        self.placed.len()
    }

    pub fn is_empty(&self) -> bool {
        self.placed.is_empty()
    }

    /// `__iter__` in the Python: the features themselves, not the `Placed` wrappers,
    /// in the same order they were given to `new`.
    pub fn iter(&self) -> impl Iterator<Item = &Feature> {
        self.placed.iter().map(|placed| &placed.feature)
    }

    /// The ground after everything placed here has had its say, and how much say it had.
    ///
    /// Args:
    /// point: Where.
    /// elevation_m: The ground before features.
    ///
    /// Returns:
    /// `(shaped_metres, authority)`. `shaped_metres` is an **absolute elevation**, the
    /// same quantity as `elevation_m`, not a delta. `authority` runs nothing to one and
    /// is what detail uses to get out of the way. The two are not interchangeable and a
    /// later task binding this must not confuse them.
    ///
    /// Notes:
    /// **`RAISE` and `CARVE` are one-way, and that is not a hard decision in disguise.**
    /// A raise whose target is already below the ground contributes nothing, and at the
    /// moment the two are equal it contributes nothing either - so the switch happens
    /// exactly where the effect is zero and the ground stays continuous. The same
    /// argument every tectonic gate had to survive.
    ///
    /// Authority needs that argument made a second time and differently. It is not zero
    /// at the switch merely because the contribution is: it would jump from nothing to
    /// the full weight the instant a feature began to apply. So it ramps over
    /// `SETTLE_M` of relief - which is also the behaviour worth having, since a feature
    /// reshaping the bed by centimetres should not take its texture away.
    ///
    /// **Order is meaning here, not merely float non-associativity.** A bar listed
    /// after the channel it lies across sits on the carved bottom, which is the right
    /// story; listed before, the channel would cut straight through it. `self.placed`
    /// is therefore iterated in the order it was built, in a plain `for` loop over the
    /// slice - never sorted, never accumulated in parallel and combined afterwards. Both
    /// of those would still be deterministic; neither would tell the same story, because
    /// each iteration's `result` feeds the *next* feature's `lift`, not the original
    /// `elevation_m`.
    ///
    /// **The RAISE/CARVE guards are transcribed as two separate `if`s, not folded into
    /// one, because they are not one rule wearing two hats.** Both converge at
    /// `lift == 0.0` - a raise skips there and a carve skips there too - but that is a
    /// fact about where each one's effect is zero, discovered independently, not a
    /// shared reason to merge them. With `result == -0.0` and `target_m == 0.0`
    /// (`lift == 0.0`), the guard skips and `result` stays `-0.0`; a "simplified" rewrite
    /// that instead let the `lift == 0.0` case fall through to `result += weight * lift`
    /// would compute `-0.0 + weight * 0.0`, which is `+0.0` - value-equal, bit-different.
    /// Transcribing the guards exactly, rather than reasoning about them and writing
    /// something "equivalent", is what keeps the sign.
    ///
    /// **`authority = max(authority, ...)` is CPython's two-argument `max`**, which
    /// returns its FIRST argument whenever the comparison against the second is not
    /// true - so this is `if candidate > authority { candidate } else { authority }` in
    /// that operand order, the same house form as `plates.rs::margin_at`, not
    /// `f64::max`.
    pub fn apply(&self, point: &SpherePoint, elevation_m: f64) -> (f64, f64) {
        let mut result = elevation_m;
        let mut authority = 0.0;
        // Order is meaning here (see doc comment above) - a plain for loop over
        // `self.placed` in construction order, each iteration reading and writing
        // `result` in place. No sorting, no reordering, no parallel accumulation.
        for placed in &self.placed {
            let weight = placed.weight_at(point);
            if weight <= 0.0 {
                continue;
            }
            let lift = placed.feature.target_m - result;
            if placed.feature.compose == RAISE && lift <= 0.0 {
                continue;
            }
            if placed.feature.compose == CARVE && lift >= 0.0 {
                continue;
            }
            result += weight * lift;
            let candidate = weight * smooth(lift.abs() / SETTLE_M);
            // Python: authority = max(authority, candidate). Two-argument max returns
            // the FIRST argument (authority) unless candidate is strictly greater.
            authority = if candidate > authority { candidate } else { authority };
        }
        (result, authority)
    }

    /// Placed features close enough to belong on a chart as symbols.
    ///
    /// Args:
    /// point: Where the chart is centred.
    /// within_m: How much sea it covers.
    ///
    /// Returns:
    /// `(distance_m, feature)` pairs, nearest first.
    ///
    /// Notes:
    /// **The second channel, and the reason it has to exist.** A pinnacle a hundred
    /// metres across cannot survive a chart sampled every four hundred: it is not
    /// smoothed away, it is *missed* - and worse, whether it is missed depends on where
    /// the sample grid happens to fall, so it would blink in and out as a ship moved.
    /// Real charts answer this by giving isolated dangers a symbol instead of a contour,
    /// and so does this.
    ///
    /// The comparison is `distance <= within_m`, inclusive, exactly as the Python
    /// writes it - a feature sitting precisely on the chart's edge still gets a mark.
    /// The sort key is the distance alone, `pair[0]` in the Python; Rust's `sort_by` is
    /// a stable sort exactly as CPython's `list.sort` is, so two features at the same
    /// distance keep the order they were found in (construction order over `self.placed`),
    /// matching the Python's behaviour without either language having to say so.
    ///
    /// `distance_m` is a bounded quantity (through `SpherePoint::distance_to`, which
    /// reaches `atan2`/`sqrt`/`cos`/`sin` via `angle_to`) feeding a **discrete** output:
    /// chart membership (in or out at the `within_m` edge) and chart ordering (which
    /// mark is listed first). A drift of even one ULP in `distance_m` could, at a point
    /// sitting exactly on the `within_m` boundary or at a tie between two marks'
    /// distances, flip which side of `<=` it falls on or swap two entries' order -
    /// Task 5's conformance sweep is where that gets measured, not here.
    pub fn marks_near(&self, point: &SpherePoint, within_m: f64) -> Vec<(f64, &Feature)> {
        let mut found: Vec<(f64, &Feature)> = Vec::new();
        for placed in &self.placed {
            if !placed.feature.marked {
                continue;
            }
            let distance = point.distance_to(&placed.feature.at, self.radius_m);
            if distance <= within_m {
                found.push((distance, &placed.feature));
            }
        }
        // Python: found.sort(key=lambda pair: pair[0]) - stable, ascending, by distance
        // alone. `partial_cmp` is used rather than a total-order comparator because a
        // NaN distance is not expected to occur here (see `continentality.rs` for the
        // same house convention on a sortable f64 field).
        found.sort_by(|a, b| a.0.partial_cmp(&b.0).expect("distance_to produces no NaN"));
        found
    }
}

impl<'a> IntoIterator for &'a Features {
    type Item = &'a Feature;
    type IntoIter = std::iter::Map<std::slice::Iter<'a, Placed>, fn(&'a Placed) -> &'a Feature>;

    fn into_iter(self) -> Self::IntoIter {
        self.placed.iter().map(|placed| &placed.feature)
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

    /// **The reach-gate corner, not the ring.** A ring scan around `reach_m` finds
    /// nothing at all - Task 1 probed 30,240 gate-rejected points over 16 shapes and got
    /// zero leaks, and the false "both branches are ~zero, the gate is a no-op" claim
    /// almost certainly came from exactly that scan. Reproduced here while this test was
    /// last revised: 2,000 azimuths x 8 radial offsets at each of 3x2, 150x90 and
    /// 1200x300 gives 29,712 gate-rejected ring points and **0** leaks. The leak lives in
    /// the *corner*: a point where `along` sits a hair inside `length_m` **and** `across`
    /// sits a hair inside `width_m` at the same time, so both `bump` factors are
    /// individually non-zero even though `dot` has already fallen below `cos_reach`.
    ///
    /// **What the grid below actually scans, and why that is the right band.** The grid
    /// is *relative*: spans of 1e-13 to 1e-8 of each extent, five steps either way. At
    /// 3x2 that reaches insets from ~3e-13 m to 1.5e-7 m along and 1e-7 m across - 720
    /// probes, of which **189 are gate-rejected and 40 carry a non-zero ungated weight**.
    /// The loop breaks on the first of those, at span 1e-10, `(i, j) = (2, 5)`, ungated
    /// weight **7.525633814669704e-38**; the worst anywhere on this grid is
    /// **5.523881881698213e-29**. Those are tiny, and it does not matter in the least,
    /// because **the assertion is `assert_eq!(gated, 0.0)` - raw bits, not a tolerance.**
    /// What the band has to supply is a point that is genuinely gate-rejected while both
    /// `bump` factors are non-zero, and it supplies forty of them. Delete the gate and
    /// `gated` becomes `ungated`, which the test has just asserted is strictly positive,
    /// and it fails. A magnitude is only needed by a test that compares against a
    /// threshold, and this one never does.
    ///
    /// **Why 3x2 rather than a large feature.** Not because the leak here is big enough
    /// to see - at this grid it is not. The band exists because near the origin `dot` and
    /// `cos_reach` are both within an ULP of 1.0, so the comparison stops resolving
    /// distance at all; its width in metres runs as `ULP(1.0) * radius_m^2 / reach_m`,
    /// which is ~2.5 mm at 3x2 against ~7 um at 1200x300. The small shape therefore leaks
    /// far more densely on the same relative grid (40 leaks of 189 rejected, against 19 of
    /// 154 at 1200x300), so the search terminates quickly and is not sensitive to where
    /// the grid happens to land. And it is the shape where the leak *matters*: scanned in
    /// absolute insets rather than relative spans, the worst leaked weight measured
    /// **1.2047e-12 at 3x2** (at ~1.2 mm along, ~1.8 mm across), against **1.1055e-26 at
    /// 150x90** and **8.4188e-32 at 1200x300** - so on a small feature the gate is
    /// protecting `apply`'s `result`, not only its `authority`, while on a large one it is
    /// only ever protecting `authority`, which starts at a hard `0.0` where
    /// `max(0.0, tiny)` is `tiny`.
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

    fn at12_3456_78() -> SpherePoint {
        SpherePoint::from_latlon(12.34, 56.78)
    }

    fn named_feature(
        kind: &str,
        at: SpherePoint,
        target_m: f64,
        length_m: f64,
        width_m: f64,
        bearing_deg: f64,
        compose: &str,
    ) -> Feature {
        Feature {
            kind: kind.to_string(),
            at,
            target_m,
            length_m,
            width_m,
            bearing_deg,
            compose: compose.to_string(),
            marked: false,
            substrate: None,
        }
    }

    #[test]
    fn features_len_and_iter_match_construction_order() {
        let at = at12_3456_78();
        let a = named_feature("bank", at, -2.0, 50.0, 30.0, 0.0, RAISE);
        let b = named_feature("channel", at, -20.0, 50.0, 30.0, 90.0, CARVE);
        let features = Features::new(vec![a.clone(), b.clone()], EARTH_RADIUS_M);
        assert_eq!(features.len(), 2);
        assert!(!features.is_empty());
        let kinds: Vec<&str> = features.iter().map(|f| f.kind.as_str()).collect();
        assert_eq!(kinds, vec!["bank", "channel"]);
        let kinds_via_into_iter: Vec<&str> = (&features).into_iter().map(|f| f.kind.as_str()).collect();
        assert_eq!(kinds_via_into_iter, vec!["bank", "channel"]);
    }

    #[test]
    fn apply_with_no_features_returns_elevation_unchanged_and_zero_authority() {
        let features = Features::new(Vec::<Feature>::new(), EARTH_RADIUS_M);
        let (result, authority) = features.apply(&at12_3456_78(), -5.0);
        assert_eq!(result, -5.0);
        assert_eq!(authority, 0.0);
    }

    #[test]
    fn apply_a_single_raise_at_its_own_centre() {
        // python: Features([Feature(kind="bank", at=at, target_m=-2.0, length_m=50.0,
        // width_m=30.0, bearing_deg=0.0, compose=RAISE)]).apply(at, -5.0)
        // == (-2.0, 1.0). weight=1.0 exactly at centre; lift = -2.0 - (-5.0) = 3.0 > 0,
        // RAISE applies: result = -5.0 + 1.0*3.0 = -2.0; authority =
        // max(0.0, 1.0*smooth(3.0/3.0)) = smooth(1.0) = 1.0.
        let at = at12_3456_78();
        let f = named_feature("bank", at, -2.0, 50.0, 30.0, 0.0, RAISE);
        let features = Features::new(vec![f], EARTH_RADIUS_M);
        let (result, authority) = features.apply(&at, -5.0);
        assert_eq!(result, -2.0);
        assert_eq!(authority, 1.0);
    }

    #[test]
    fn apply_a_single_carve_at_its_own_centre() {
        // python: Features([Feature(kind="channel", at=at, target_m=-20.0, length_m=50.0,
        // width_m=30.0, bearing_deg=0.0, compose=CARVE)]).apply(at, -5.0) == (-20.0, 1.0).
        // lift = -20.0 - (-5.0) = -15.0 < 0, CARVE applies: result = -5.0 + 1.0*-15.0 =
        // -20.0; authority = max(0.0, smooth(15.0/3.0)) = smooth(5.0) = 1.0 (clamped).
        let at = at12_3456_78();
        let f = named_feature("channel", at, -20.0, 50.0, 30.0, 0.0, CARVE);
        let features = Features::new(vec![f], EARTH_RADIUS_M);
        let (result, authority) = features.apply(&at, -5.0);
        assert_eq!(result, -20.0);
        assert_eq!(authority, 1.0);
    }

    #[test]
    fn apply_a_raise_whose_lift_is_not_positive_contributes_nothing() {
        // A RAISE feature whose target is already below the incoming elevation: lift <=
        // 0.0, guard skips, result and authority are untouched.
        let at = at12_3456_78();
        let f = named_feature("bank", at, -10.0, 50.0, 30.0, 0.0, RAISE);
        let features = Features::new(vec![f], EARTH_RADIUS_M);
        let (result, authority) = features.apply(&at, -5.0);
        assert_eq!(result, -5.0);
        assert_eq!(authority, 0.0);
    }

    #[test]
    fn apply_raise_carve_guards_converge_at_lift_zero_and_keep_negative_zero() {
        // python: elevation_m=-0.0, target_m=0.0 (lift == 0.0 exactly).
        // Features([Feature(..., target_m=0.0, compose=RAISE)]).apply(at, -0.0)
        // == (-0.0, 0.0), and the same for compose=CARVE. The guard (`lift <= 0.0` for
        // RAISE, `lift >= 0.0` for CARVE) is true at lift == 0.0 either way, so both
        // skip and `result` keeps the elevation's own sign bit rather than becoming
        // `-0.0 + weight * 0.0` (== +0.0). Transcribed as two separate `if`s, not folded,
        // per the module doc comment.
        let at = at12_3456_78();
        let raise_f = named_feature("bank", at, 0.0, 50.0, 30.0, 0.0, RAISE);
        let (raise_result, raise_authority) =
            Features::new(vec![raise_f], EARTH_RADIUS_M).apply(&at, -0.0);
        assert!(raise_result.is_sign_negative(), "expected -0.0, got {raise_result}");
        assert_eq!(raise_result, 0.0);
        assert_eq!(raise_authority, 0.0);

        let carve_f = named_feature("channel", at, 0.0, 50.0, 30.0, 0.0, CARVE);
        let (carve_result, carve_authority) =
            Features::new(vec![carve_f], EARTH_RADIUS_M).apply(&at, -0.0);
        assert!(carve_result.is_sign_negative(), "expected -0.0, got {carve_result}");
        assert_eq!(carve_result, 0.0);
        assert_eq!(carve_authority, 0.0);
    }

    #[test]
    fn apply_a_partial_weight_matches_the_formula_and_the_python() {
        // python: Features([Feature(kind="bank", at=at, target_m=-1.0, length_m=1200.0,
        // width_m=300.0, bearing_deg=37.0, compose=RAISE)]).apply(
        //     SpherePoint.from_latlon(12.3405, 56.7795), -5.0)
        // == (-1.6537078328604258, 0.8365730417848936). weight at that probe point was
        // independently pinned at 0.8365730417848936 in
        // `weight_at_a_nearby_probe_point_is_bounded_against_python` above; lift =
        // -1.0 - (-5.0) = 4.0; result = -5.0 + weight*4.0; authority =
        // max(0.0, weight*smooth(4.0/3.0)) = weight*1.0 = weight (smooth saturates past
        // 1.0), so authority == weight exactly here.
        let at = at12_3456_78();
        let probe = SpherePoint::from_latlon(12.3405, 56.7795);
        let f = named_feature("bank", at, -1.0, 1200.0, 300.0, 37.0, RAISE);
        let weight = Placed::new(f.clone(), EARTH_RADIUS_M).weight_at(&probe);
        assert_eq!(weight, 0.8365730417848936_f64);

        let features = Features::new(vec![f], EARTH_RADIUS_M);
        let (result, authority) = features.apply(&probe, -5.0);
        assert_eq!(result, -5.0 + weight * 4.0);
        assert_eq!(authority, weight);
        assert_eq!(result, -1.6537078328604258_f64);
        assert_eq!(authority, 0.8365730417848936_f64);
    }

    /// **Order is meaning, proved rather than asserted.** Same two features - a bar
    /// (RAISE, target -2.0) and a channel (CARVE, target -20.0) crossing at the same
    /// point, so both have weight exactly 1.0 there regardless of bearing - applied in
    /// opposite orders. Starting from `elevation_m = -5.0`:
    ///
    /// `[channel, bar]` (bar listed after, sits on the carved bottom - the docstring's
    /// story): channel first, lift = -20.0 - (-5.0) = -15.0 < 0, CARVE applies:
    /// result = -20.0. Then bar, lift = -2.0 - (-20.0) = 18.0 > 0, RAISE applies:
    /// result = -20.0 + 18.0 = -2.0. Final: `(-2.0, 1.0)`.
    ///
    /// `[bar, channel]` (bar listed first, channel cuts straight through it): bar
    /// first, lift = -2.0 - (-5.0) = 3.0 > 0, RAISE applies: result = -2.0. Then
    /// channel, lift = -20.0 - (-2.0) = -18.0 < 0, CARVE applies: result =
    /// -2.0 + -18.0 = -20.0. Final: `(-20.0, 1.0)`.
    ///
    /// Both orders reach authority 1.0 (smooth saturates well past `SETTLE_M` of
    /// relief either way), so the 18 metre gap between `-2.0` and `-20.0` in `result` is
    /// the whole of the story, not a rounding artefact of the last few bits - matching
    /// a live run of `worldbuilder/bathymetry/features.py`'s `Features.apply` at this
    /// exact input.
    #[test]
    fn apply_order_of_placed_features_changes_the_result_not_just_its_bits() {
        let at = at12_3456_78();
        let bar = named_feature("bar", at, -2.0, 50.0, 30.0, 0.0, RAISE);
        let channel = named_feature("channel", at, -20.0, 50.0, 30.0, 90.0, CARVE);

        let channel_then_bar = Features::new(vec![channel.clone(), bar.clone()], EARTH_RADIUS_M);
        let bar_then_channel = Features::new(vec![bar, channel], EARTH_RADIUS_M);

        let (result_a, authority_a) = channel_then_bar.apply(&at, -5.0);
        let (result_b, authority_b) = bar_then_channel.apply(&at, -5.0);

        assert_eq!(result_a, -2.0);
        assert_eq!(result_b, -20.0);
        assert_ne!(result_a, result_b, "swapping order must change the shaped elevation");
        assert_eq!(authority_a, 1.0);
        assert_eq!(authority_b, 1.0);
    }

    #[test]
    fn marks_near_finds_only_marked_features_within_reach_nearest_first() {
        // python (live run): rock at the centre (distance 0.0), wreck 1111.9492664455072 m
        // away, both marked=True; an unmarked bank in between is excluded regardless of
        // distance. within_m=2000.0 includes both marked features.
        let centre = at12_3456_78();
        let rock_at = centre;
        let wreck_at = SpherePoint::from_latlon(12.35, 56.78);
        let unmarked_at = SpherePoint::from_latlon(12.341, 56.78);

        let mut rock = named_feature("rock", rock_at, -0.5, 10.0, 10.0, 0.0, RAISE);
        rock.marked = true;
        let mut wreck = named_feature("wreck", wreck_at, -3.0, 5.0, 5.0, 0.0, RAISE);
        wreck.marked = true;
        let unmarked = named_feature("bank", unmarked_at, -2.0, 5.0, 5.0, 0.0, RAISE);

        // Construction order deliberately puts wreck before rock, so a passing test
        // proves the sort (not construction order) puts rock first.
        let features = Features::new(vec![wreck, rock, unmarked], EARTH_RADIUS_M);
        let marks = features.marks_near(&centre, 2000.0);

        assert_eq!(marks.len(), 2);
        assert_eq!(marks[0].1.kind, "rock");
        assert_eq!(marks[0].0, 0.0);
        assert_eq!(marks[1].1.kind, "wreck");
        assert_eq!(marks[1].0, 1111.9492664455072_f64);
    }

    #[test]
    fn marks_near_boundary_is_inclusive_of_within_m_exactly() {
        // python (live run): distance from centre to the rock's own `at` is exactly
        // 0.0, and marks_near(centre, 0.0) with within_m == distance included it
        // (`distance <= within_m`), count 1. This pins the `<=`, not `<`.
        let centre = at12_3456_78();
        let mut rock = named_feature("rock", centre, -0.5, 10.0, 10.0, 0.0, RAISE);
        rock.marked = true;
        let exact_distance = centre.distance_to(&rock.at, EARTH_RADIUS_M);
        assert_eq!(exact_distance, 0.0);

        let features = Features::new(vec![rock], EARTH_RADIUS_M);
        let marks = features.marks_near(&centre, exact_distance);
        assert_eq!(marks.len(), 1);
    }

    #[test]
    fn marks_near_excludes_a_feature_just_beyond_within_m() {
        let centre = at12_3456_78();
        let mut wreck = named_feature("wreck", SpherePoint::from_latlon(12.35, 56.78), -3.0, 5.0, 5.0, 0.0, RAISE);
        wreck.marked = true;
        let features = Features::new(vec![wreck], EARTH_RADIUS_M);
        let marks = features.marks_near(&centre, 1111.9492664455072_f64 - 1.0);
        assert_eq!(marks.len(), 0);
    }
}
