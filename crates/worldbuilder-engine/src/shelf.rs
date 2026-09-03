//! The continental shelf: the water a ship actually sails in.
//!
//! Ported from `worldbuilder/bathymetry/shelf.py`. For a maritime simulation this is the
//! most important terrain on the planet. Mountains six hundred kilometres inland are
//! scenery; the first hundred metres below sea level is where anchoring, sounding,
//! grounding and pilotage all happen, and generic noise there produces gorgeous
//! continents with unusable coasts.
//!
//! Three rules, and two of them are scars.
//!
//! **The shelf sets a target depth, and the ground is blended towards it.** Not an offset
//! added on. A shelf describes what the coastal profile should *tend to*, and blending
//! leaves control over what it may override - so a trench crossing a continental margin
//! is not quietly flattened by something announcing that the water here is about a
//! hundred metres.
//!
//! **Every gate sits outside the support of what it gates.** The general form of M1.4's
//! worst bug, where a trench profile was still a thousand metres tall at the range limit
//! and the optimisation that skipped it became a cliff. Each gate below is either placed
//! where the function is already zero, or paired with a weight that has faded to nothing
//! before it.
//!
//! **Nothing is classified.** Not "is this a continent", not "is this an island", not "is
//! this near a coast". M1.4 produced four separate cliffs from four hard decisions taken
//! on continuous quantities, and every equivalent temptation here is answered with a
//! weight.
//!
//! This task carries the module constants, the `smooth` helper (reused from `detail`, not
//! duplicated - see below), and the two value types. `Shelf`'s behaviour comes in a later
//! task.

use crate::continentality::Continentality;
use crate::sphere::SpherePoint;
use crate::tectonics::Tectonics;

/// How far from the shore, in field units, a point may be and still be considered.
/// Measured against a gradient of about two parts in ten million per metre, this is
/// roughly a quarter of a thousand kilometres - comfortably wider than any shelf.
///
/// The cheap first gate: a value, no gradient, no probes. Deep interiors and deep basins
/// fail it having done one field evaluation, which is the whole performance strategy,
/// because a gradient costs six times what a value does.
pub const COASTAL_WINDOW: f64 = 0.055;

/// Below this, the field is too flat to say where the coast is - the estimate `c / |grad|`
/// divides by nearly nothing and returns a distance to a shore on the far side of the
/// world. The weight has already faded out by here; this only stops the arithmetic.
pub const MIN_GRADIENT: f64 = 1.0e-8;

/// A gradient typical of a continental margin, measured: the median near real coastlines
/// on this generator is about 1.9 parts in ten million per metre. Steeper means a smaller
/// landmass, which gets a correspondingly narrower platform - an island does not deserve a
/// hundred-kilometre apron merely for being above water.
pub const REFERENCE_GRADIENT: f64 = 2.0e-7;

/// How far offshore the shelf break lies on a broad margin, and how deep the shelf is at
/// its outer edge. Beyond the break the weight fades and the macro depth takes over, which
/// is what draws the continental slope without anybody having to model one.
pub const SHELF_BREAK_M: f64 = 80_000.0;
pub const SHELF_EDGE_M: f64 = -150.0;
pub const SLOPE_WIDTH_M: f64 = 70_000.0;

/// How far inland the shelf's influence reaches. Small: it shapes the approach, not the
/// country behind it.
pub const INLAND_REACH_M: f64 = 12_000.0;

/// How much tectonic relief it takes to hold the shelf off. A trench or an uplift belt is
/// deliberate structure and outranks a general statement about coastal depth.
///
/// Measured down from seven hundred. At that value a coast sitting on three hundred metres
/// of tectonic uplift still gave the shelf a weight of 0.59, which dragged the mountain
/// down to a hundred and twenty-five metres - the shelf quietly demolishing the range it
/// was supposed to defer to. Two hundred and fifty leaves it at 0.14 there, which shapes
/// the water without levelling the land.
pub const TECTONIC_AUTHORITY_M: f64 = 250.0;

/// Smoothstep, clamped. Flat at both ends, so nothing it gates leaves a crease.
///
/// `shelf.py`'s `_smooth` is `max(0.0, min(1.0, x))` then `x * x * (3.0 - 2.0 * x)` -
/// exactly `detail::smooth` (see that module's doc comment, which spells out the same
/// formula in the same operand order). Reused here rather than adding a third copy of an
/// identical function; `tectonics.rs` has no standalone `smooth` of its own to compare
/// against.
pub use crate::detail::smooth;

/// The ground at a point, with the expensive intermediates that produced it.
///
/// Attributes:
/// elevation_m: Relative to datum, structural only.
/// weight: How much say the shelf had, nothing to one.
/// tectonic_m: What the plates contributed.
#[derive(Debug, Clone, Copy)]
pub struct Reading {
    pub elevation_m: f64,
    pub weight: f64,
    pub tectonic_m: f64,
}

/// Where a point stands relative to the nearest shore, as far as can be told locally.
///
/// Attributes:
/// distance_m: Estimated metres to the shoreline. Positive inland, negative at sea.
/// breadth: How broad the landmass is, from nothing to one. A proxy, from how gently
/// continentality changes here.
#[derive(Debug, Clone, Copy)]
pub struct Coastal {
    pub distance_m: f64,
    pub breadth: f64,
}

/// Coastal bathymetry, laid over the macro terrain.
///
/// Notes:
/// Reads the tectonic layer rather than replacing it, and the composition is a blend
/// rather than a sum - which is what lets a trench survive crossing a margin.
pub struct Shelf {
    tectonics: Tectonics,
    land: Continentality,
    radius_m: f64,
}

impl Shelf {
    pub fn new(tectonics: Tectonics, continentality: Continentality, radius_m: f64) -> Self {
        Self { tectonics, land: continentality, radius_m }
    }

    /// How far the shore is, estimated from the field and its slope.
    ///
    /// A local linear estimate, and named to admit it. Continentality crosses zero at the
    /// shore, so dividing the value by the magnitude of its gradient gives the distance to
    /// that crossing if the field carried on at the same slope - which it does not,
    /// exactly. Over the tens of kilometres a shelf occupies, against a field whose
    /// features are thousands of kilometres wide, it is close enough to build on and
    /// nowhere near an exact geodesic distance to the final shoreline.
    ///
    /// The value is checked before the gradient is taken, and that ordering is the whole
    /// performance strategy of this file: the gradient costs six times what the value
    /// does, and most of a planet is deep interior or deep basin.
    pub fn coastal(&self, point: &SpherePoint) -> Option<Coastal> {
        let value = self.land.above_shore(point);
        if value.abs() > COASTAL_WINDOW {
            // Far from any shore. The weight is already zero here, so this gate sits
            // outside the support of what it gates rather than being a cliff in it.
            return None;
        }

        let gradient = self.land.gradient(point);
        let slope = gradient.magnitude();
        if slope < MIN_GRADIENT {
            // Below this, the field is too flat to say where the coast is - the estimate
            // `c / |grad|` divides by nearly nothing and returns a distance to a shore on
            // the far side of the world. The weight has already faded out by here; this
            // only stops the arithmetic.
            return None;
        }

        Some(Coastal {
            distance_m: value / slope,
            breadth: smooth(REFERENCE_GRADIENT / slope),
        })
    }

    /// What the water ought to be doing at this distance from shore.
    ///
    /// Only the shelf itself is described. The continental slope is not modelled at all -
    /// beyond the break the blend weight fades and the macro ocean depth comes back, and
    /// the transition between them *is* the slope. One fewer profile to write and one
    /// fewer place for two descriptions of the same water to disagree.
    pub fn target_depth_m(&self, coastal: &Coastal) -> f64 {
        let offshore = -coastal.distance_m;
        if offshore <= 0.0 {
            return 0.0;
        }
        // offshore = -(distance_m), and distance_m = value / slope with slope strictly
        // positive (coastal()'s MIN_GRADIENT gate has already returned otherwise).
        // Dividing by a strictly positive quantity cannot change a sign, so this branch
        // is decided by the sign of `value`, which never passes through the `hypot` in
        // `Gradient::magnitude` - it looks exposed to that mixed-sign hazard and is not.
        let break_at = SHELF_BREAK_M * if 0.15 > coastal.breadth { 0.15 } else { coastal.breadth };
        SHELF_EDGE_M * smooth(offshore / break_at)
    }

    /// How much say the shelf has here.
    ///
    /// `tectonic_m`: the tectonic offset, if the caller already has it. Worked out again
    /// if not, which is what makes it worth passing - and "if not" is an identity check
    /// against absence, not against zero: `Some(0.0)` is a supplied zero and is used as
    /// given.
    ///
    /// Four things fade it, and every one of them replaces a decision that could have
    /// been a hard test:
    ///
    /// *How far from the shore*, so the shelf reaches its own edge at nothing rather than
    /// being cut off at the window boundary.
    ///
    /// *How broad the landmass is*, so a continent gets a wide shelf and an isolated
    /// island a narrow platform with steep-to water - without either being classified as
    /// anything.
    ///
    /// *How far inland*, quickly, because a shelf shapes the approach and not the country
    /// behind it.
    ///
    /// *What the plates already said*. A trench is deliberate deep structure and outranks
    /// a general remark about coastal depth. Without this the shelf would cheerfully fill
    /// a subduction trench in with a hundred metres of water and the most dramatic thing
    /// on the chart would vanish.
    pub fn weight(&self, point: &SpherePoint, coastal: &Coastal, tectonic_m: Option<f64>) -> f64 {
        let offshore = -coastal.distance_m;
        let break_at = SHELF_BREAK_M * if 0.15 > coastal.breadth { 0.15 } else { coastal.breadth };

        // Same sign argument as target_depth_m: offshore is -(value / slope) with slope
        // strictly positive, so this comparison is decided by the sign of `value`, never
        // by the `hypot` inside `Gradient::magnitude`.
        let seaward = if offshore >= 0.0 {
            1.0 - smooth((offshore - break_at) / SLOPE_WIDTH_M)
        } else {
            1.0 - smooth(-offshore / INLAND_REACH_M)
        };

        let tectonic_m = match tectonic_m {
            Some(value) => value,
            None => self.tectonics.offset_m(point),
        };
        let authority = 1.0 - smooth(tectonic_m.abs() / TECTONIC_AUTHORITY_M);

        seaward * coastal.breadth * authority
    }
}

// The behaviour methods (`coastal`, `target_depth_m`, `weight`, `evaluate`,
// `elevation_m`) land in a later task; these accessors keep the fields from being
// flagged dead in the meantime and give that task somewhere to start from.
#[allow(dead_code)]
impl Shelf {
    pub(crate) fn tectonics(&self) -> &Tectonics {
        &self.tectonics
    }

    pub(crate) fn land(&self) -> &Continentality {
        &self.land
    }

    pub(crate) fn radius_m(&self) -> f64 {
        self.radius_m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Constants pinned character-for-character against worldbuilder/bathymetry/shelf.py.
    #[test]
    fn constants_match_the_python() {
        assert_eq!(COASTAL_WINDOW.to_bits(), 0.055f64.to_bits());
        assert_eq!(MIN_GRADIENT.to_bits(), 1.0e-8f64.to_bits());
        assert_eq!(REFERENCE_GRADIENT.to_bits(), 2.0e-7f64.to_bits());
        assert_eq!(SHELF_BREAK_M.to_bits(), 80_000.0f64.to_bits());
        assert_eq!(SHELF_EDGE_M.to_bits(), (-150.0f64).to_bits());
        assert_eq!(SLOPE_WIDTH_M.to_bits(), 70_000.0f64.to_bits());
        assert_eq!(INLAND_REACH_M.to_bits(), 12_000.0f64.to_bits());
        assert_eq!(TECTONIC_AUTHORITY_M.to_bits(), 250.0f64.to_bits());
    }

    #[test]
    fn smooth_saturates_at_both_ends_and_hits_the_midpoint() {
        assert_eq!(smooth(-10.0), 0.0);
        assert_eq!(smooth(0.0), 0.0);
        assert_eq!(smooth(10.0), 1.0);
        assert_eq!(smooth(1.0), 1.0);
        assert_eq!(smooth(0.5), 0.5);
    }

    // Fixture matching tests/test_shelf_gates.py's `build()`: seed 20260831, the default
    // plate count (22), Earth radius and land fraction - the same world Task 1 measured
    // its gate margins and firing point against.
    const SEED: i64 = 20260831;

    fn build() -> Shelf {
        let land = Continentality::new(
            SEED as u64, // cast-ok: seed is a fixed positive literal, no truncation
            crate::sphere::EARTH_RADIUS_M,
            crate::continentality::LAND_FRACTION,
        );
        let plates = crate::generation::plates_for(SEED, 22);
        let tectonics = Tectonics::new(plates, land, crate::sphere::EARTH_RADIUS_M);
        Shelf::new(tectonics, land, crate::sphere::EARTH_RADIUS_M)
    }

    fn point(x: f64, y: f64, z: f64) -> SpherePoint {
        SpherePoint::from_vector(&crate::vectors::Vec3::new(x, y, z))
            .expect("fixture vector has a direction")
    }

    #[test]
    fn coastal_is_none_deep_in_the_interior_on_the_value_gate_alone() {
        // The north pole on this world: above_shore is far outside COASTAL_WINDOW, so the
        // gate returns None having never touched the gradient.
        //
        // That ordering is the file's whole performance strategy -- the gradient costs six
        // times what the value does, and most of a planet is deep interior or deep basin --
        // but NO TEST HERE CHECKS IT. Taking the gradient eagerly would leave every value
        // identical and every test green, and only cost time. It is verified by reading
        // `coastal`, not by an assertion; observing it would need a call counter on
        // Continentality, which is a concrete type. Do not read this test as covering it.
        let shelf = build();
        let p = point(0.0, 0.0, 1.0);
        assert!(shelf.coastal(&p).is_none());
    }

    #[test]
    fn coastal_returns_some_at_a_genuine_coastal_point() {
        // Located by scanning the same corpus() the Python conformance suite uses (see
        // tests/test_shelf_gates.py): the first corpus point where shelf.coastal() is not
        // None on this world.
        let shelf = build();
        let p = point(
            -0.05860692729347337,
            -0.5960915358050722,
            0.8007746930409125,
        );
        let coastal = shelf.coastal(&p).expect("point should be coastal");
        assert!((coastal.distance_m - (-15538.459466828463)).abs() < 1e-6);
        assert!((coastal.breadth - 0.7793928482152788).abs() < 1e-9);
    }

    #[test]
    fn coastal_is_none_where_the_gradient_gate_genuinely_fires() {
        // Located the same way Task 1 did: scanning tests/test_conformance.py's corpus()
        // against this world's Continentality for the point whose slope comes closest to
        // (while remaining below) MIN_GRADIENT, inside COASTAL_WINDOW. This point has
        // above_shore = 5.247544e-02 (inside the 0.055 window) and slope =
        // 2.501102e-09 (~0.2501 x MIN_GRADIENT) - confirmed below to actually be inside
        // the window, so the None it produces is attributable to the gradient gate and
        // not the value gate.
        let shelf = build();
        let land = shelf.land();
        let p = point(0.5338502410791132, 0.8419619369120575, 0.07812820803697702);

        let above = land.above_shore(&p);
        assert!(
            above.abs() <= COASTAL_WINDOW,
            "fixture point must pass the value gate to exercise the gradient gate; above={above}"
        );
        let slope = land.gradient(&p).magnitude();
        assert!(
            slope < MIN_GRADIENT,
            "fixture point must be sub-threshold to exercise the gradient gate; slope={slope}"
        );

        assert!(shelf.coastal(&p).is_none());
    }

    #[test]
    fn coastal_and_reading_carry_their_fields() {
        let coastal = Coastal { distance_m: 1234.5, breadth: 0.75 };
        assert_eq!(coastal.distance_m.to_bits(), 1234.5f64.to_bits());
        assert_eq!(coastal.breadth.to_bits(), 0.75f64.to_bits());

        let reading = Reading { elevation_m: -12.0, weight: 0.4, tectonic_m: 3.0 };
        assert_eq!(reading.elevation_m.to_bits(), (-12.0f64).to_bits());
        assert_eq!(reading.weight.to_bits(), 0.4f64.to_bits());
        assert_eq!(reading.tectonic_m.to_bits(), 3.0f64.to_bits());
    }

    // -- target_depth_m and weight ------------------------------------------------
    //
    // Expected values below are hand-derived from shelf.py's formulas (lines 167-235),
    // not read off a run of the Rust code under test. `smooth` is treated as a verified
    // building block (it has its own pinned tests above and is shared with `detail`),
    // so using it to compute an oracle here is deriving from the formula, not
    // rerunning the implementation.

    #[test]
    fn target_depth_m_is_exactly_zero_at_and_seaward_of_the_shore() {
        // offshore = -distance_m; offshore <= 0.0 is a hard early return of 0.0.
        let shelf = build();
        let at_shore = Coastal { distance_m: 0.0, breadth: 0.5 };
        assert_eq!(shelf.target_depth_m(&at_shore), 0.0);

        let inland = Coastal { distance_m: 1.0, breadth: 0.5 };
        assert_eq!(shelf.target_depth_m(&inland), 0.0);
    }

    #[test]
    fn target_depth_m_saturates_to_the_shelf_edge_beyond_the_break() {
        // distance_m = -200_000.0 -> offshore = 200_000.0.
        // break_at = SHELF_BREAK_M * max(0.15, breadth) = 80_000 * 0.5 = 40_000.
        // offshore / break_at = 5.0 -> smooth clamps the fraction to 1.0 and returns
        // 1.0 * 1.0 * (3.0 - 2.0 * 1.0) = 1.0 exactly.
        // target = SHELF_EDGE_M * 1.0 = -150.0 exactly.
        let shelf = build();
        let far_offshore = Coastal { distance_m: -200_000.0, breadth: 0.5 };
        assert_eq!(shelf.target_depth_m(&far_offshore), -150.0);
    }

    #[test]
    fn target_depth_m_uses_the_015_floor_on_a_narrow_platform() {
        // breadth = 0.05 < 0.15, so break_at must use the floor, not breadth itself:
        // break_at = 80_000 * 0.15 = 12_000. offshore = 6_000.
        // fraction = 6_000 / 12_000 = 0.5 -> smooth(0.5) = 0.5*0.5*(3.0-1.0) = 0.5.
        // target = -150.0 * 0.5 = -75.0 exactly.
        let shelf = build();
        let narrow = Coastal { distance_m: -6_000.0, breadth: 0.05 };
        assert_eq!(shelf.target_depth_m(&narrow), -75.0);
    }

    #[test]
    fn weight_fades_to_nothing_well_beyond_the_shelf_break() {
        // distance_m = -200_000.0 -> offshore = 200_000.0, breadth = 0.5.
        // break_at = 80_000 * 0.5 = 40_000.
        // (offshore - break_at) / SLOPE_WIDTH_M = 160_000 / 70_000 = 2.2857... -> smooth
        // clamps to 1.0, so seaward = 1.0 - 1.0 = 0.0 exactly.
        // tectonic_m = Some(0.0) supplied directly -> authority = 1.0 - smooth(0.0) = 1.0.
        // weight = 0.0 * 0.5 * 1.0 = 0.0 exactly.
        let shelf = build();
        let far_offshore = Coastal { distance_m: -200_000.0, breadth: 0.5 };
        let p = point(0.0, 0.0, 1.0);
        assert_eq!(shelf.weight(&p, &far_offshore, Some(0.0)), 0.0);
    }

    #[test]
    fn weight_fades_quickly_inland() {
        // distance_m = 6_000.0 -> offshore = -6_000.0 (inland), breadth = 0.6.
        // offshore < 0.0, so seaward = 1.0 - smooth(-offshore / INLAND_REACH_M)
        //                              = 1.0 - smooth(6_000 / 12_000)
        //                              = 1.0 - smooth(0.5)
        //                              = 1.0 - 0.5 = 0.5.
        // tectonic_m = Some(0.0) -> authority = 1.0.
        // weight = 0.5 * 0.6 * 1.0 = 0.3.
        let shelf = build();
        let inland = Coastal { distance_m: 6_000.0, breadth: 0.6 };
        let p = point(0.0, 0.0, 1.0);
        let got = shelf.weight(&p, &inland, Some(0.0));
        assert!((got - 0.3).abs() < 1e-12, "got {got}");
    }

    #[test]
    fn weight_far_inland_saturates_to_zero() {
        // distance_m = 20_000.0 -> offshore = -20_000.0; -offshore / INLAND_REACH_M =
        // 20_000 / 12_000 = 1.6667, clamped by smooth to 1.0 -> seaward = 0.0 exactly.
        let shelf = build();
        let far_inland = Coastal { distance_m: 20_000.0, breadth: 0.6 };
        let p = point(0.0, 0.0, 1.0);
        assert_eq!(shelf.weight(&p, &far_inland, Some(0.0)), 0.0);
    }

    #[test]
    fn weight_is_suppressed_by_a_large_tectonic_offset() {
        // distance_m = -1_000.0 -> offshore = 1_000.0, breadth = 0.8.
        // break_at = 80_000 * 0.8 = 64_000.
        // (offshore - break_at) / SLOPE_WIDTH_M = (1_000 - 64_000) / 70_000 = -0.9,
        // clamped by smooth to 0.0 (fraction < 0.0) -> seaward = 1.0 - 0.0 = 1.0 exactly.
        //
        // With tectonic_m = Some(0.0): authority = 1.0 - smooth(0.0) = 1.0, so
        // weight = 1.0 * 0.8 * 1.0 = 0.8 exactly - the baseline this suppression is
        // measured against.
        //
        // With tectonic_m = Some(10_000.0): abs(10_000) / TECTONIC_AUTHORITY_M =
        // 10_000 / 250 = 40.0, clamped by smooth to 1.0 -> authority = 0.0 exactly.
        // weight = 1.0 * 0.8 * 0.0 = 0.0 exactly - the plates' say overrides completely.
        let shelf = build();
        let coastal = Coastal { distance_m: -1_000.0, breadth: 0.8 };
        let p = point(0.0, 0.0, 1.0);
        assert_eq!(shelf.weight(&p, &coastal, Some(0.0)), 0.8);
        assert_eq!(shelf.weight(&p, &coastal, Some(10_000.0)), 0.0);
    }

    #[test]
    fn weight_treats_some_zero_as_a_supplied_zero_not_as_absent() {
        // The trap: shelf.py's `if tectonic_m is None:` is an identity check, not a
        // float-falsy `if tectonic_m:`. Some(0.0) must be used as-is; only real
        // absence (None) may trigger `self.tectonics.offset_m(point)`.
        //
        // Located by scanning the same corpus() the Python conformance suite uses (see
        // tests/test_conformance.py), the same way the `coastal` fixtures above were
        // found: the first corpus point that is coastal (Some), within the shelf's live
        // seaward range (not saturated to a zero seaward term), and has a non-trivial
        // tectonic offset_m, so that `None` (recomputed) and `Some(0.0)` (supplied) are
        // guaranteed to produce different weights. Calling `shelf.tectonics().offset_m`
        // here exercises a dependency already ported and pinned in an earlier task, not
        // the code under test.
        let shelf = build();
        let p = point(-0.5745510503817439, -0.4648012188180899, 0.724135185515465);
        let coastal = shelf.coastal(&p).expect("fixture point is coastal");

        let actual_tectonic = shelf.tectonics().offset_m(&p);
        assert!(
            actual_tectonic.abs() > 1.0,
            "fixture point must have a non-trivial tectonic offset to distinguish \
             None from Some(0.0); actual_tectonic={actual_tectonic}"
        );

        // Hand-derive both expected values from the formula. offshore/break_at/seaward
        // are identical in both cases; only `authority` (and so the product) differs.
        let offshore = -coastal.distance_m;
        let break_at = SHELF_BREAK_M * if 0.15 > coastal.breadth { 0.15 } else { coastal.breadth };
        let seaward = if offshore >= 0.0 {
            1.0 - smooth((offshore - break_at) / SLOPE_WIDTH_M)
        } else {
            1.0 - smooth(-offshore / INLAND_REACH_M)
        };

        let authority_zero = 1.0 - smooth(0.0f64.abs() / TECTONIC_AUTHORITY_M);
        let expected_some_zero = seaward * coastal.breadth * authority_zero;

        let authority_actual = 1.0 - smooth(actual_tectonic.abs() / TECTONIC_AUTHORITY_M);
        let expected_none = seaward * coastal.breadth * authority_actual;

        // The two oracles must actually differ, or this test cannot detect the trap.
        assert!(
            (expected_some_zero - expected_none).abs() > 1e-6,
            "fixture must produce distinguishable expectations: some_zero={expected_some_zero} \
             none={expected_none}"
        );

        let got_some_zero = shelf.weight(&p, &coastal, Some(0.0));
        let got_none = shelf.weight(&p, &coastal, None);

        assert!(
            (got_some_zero - expected_some_zero).abs() < 1e-9,
            "Some(0.0) got {got_some_zero}, expected {expected_some_zero}"
        );
        assert!(
            (got_none - expected_none).abs() < 1e-9,
            "None got {got_none}, expected {expected_none}"
        );
        assert!(
            (got_some_zero - got_none).abs() > 1e-6,
            "Some(0.0) must NOT behave like None: got_some_zero={got_some_zero} \
             got_none={got_none}"
        );
    }
}
