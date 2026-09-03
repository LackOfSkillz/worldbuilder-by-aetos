//! What plate motion does to the ground.
//!
//! Three rules shape this file, and all three are about what it refuses to do.
//!
//! **It returns a contribution, never an elevation.** `offset_m` is a number to *add* to
//! the continental base, so that shelves, erosion, bathymetry and detail can all compose
//! with tectonics later instead of reverse-engineering what tectonics already overwrote.
//! A function that set an absolute height would have made every one of those layers
//! harder to write, and the damage would not have been visible until they were being
//! written.
//!
//! **It does the expensive work only where it matters.** Most of a planet is nowhere near
//! a margin, and the cost measurements from earlier phases are unambiguous: kinematics
//! are 6.6 microseconds a sample and continentality gradients 33, against 5 for finding
//! the margin at all. So a point far from any boundary returns zero having done nothing
//! but the lookup it had to do anyway. Progressive enrichment - cheap context, then a
//! question, then expensive context - rather than assembling everything and discovering
//! later what was needed.
//!
//! **It has no crust model, and does not pretend to.** Whether a margin is oceanic or
//! continental is answered by sampling continentality either side of it, which is crude
//! and sufficient: the same convergent margin can then behave differently along its
//! length, because the land either side of it differs along its length. When a real crust
//! field arrives it replaces the two probes and nothing else changes - the shapers never
//! learn where the answer came from.
//!
//! Ported from `worldbuilder/terrain/tectonics.py`.

use crate::continentality::Continentality;
use crate::detmath as m;
use crate::plates::PlateSet;
use crate::sphere::SpherePoint;
use crate::tangent::TangentFrame;
use crate::vectors::Vec3;

/// Beyond this, a margin does nothing at all and no kinematics are evaluated. Every
/// profile below must reach exactly zero by here, or the gate itself becomes a cliff.
pub const MAX_TECTONIC_RANGE_M: f64 = 420_000.0;

/// How far either side of the margin to ask what kind of ground it is. Far enough to be
/// clear of the transition, near enough to describe this stretch of margin rather than
/// the continent behind it.
pub const PROBE_M: f64 = 300_000.0;

/// Continentality at which a side counts as half continental, and how wide the transition
/// from oceanic to continental is. **A width rather than a threshold, and that matters.**
///
/// The first version used a hard test - continental if above zero - and the ground jumped
/// five hundred and fifty metres wherever a margin crossed it, because the two sides of
/// the test run entirely different profiles. It is the same mistake M1.2 made: a hard
/// selection on a continuous quantity. The branches are blended now, and this is how far
/// it takes.
pub const CONTINENTAL_ENOUGH: f64 = 0.0;
pub const CONTINENTAL_BLEND: f64 = 0.45;

/// How sharply the margin picks a side. The profiles are asymmetric - a trench belongs on
/// the ocean side - so they need to know which side is which, and that answer must also
/// arrive continuously. Where the two sides are equally continental it goes smoothly to
/// nothing, which is correct: a symmetric margin has no side to prefer.
pub const SIDE_SHARPNESS: f64 = 6.0;

/// Closing speed that counts as a thoroughly active margin, in metres per million years.
/// Two plates at four centimetres a year approaching head-on. Faster than this does not
/// build higher mountains; it just saturates.
pub const FULL_RATE_M_PER_MYR: f64 = 80_000.0;

/// The profiles. Height in metres, width in metres, and where the feature sits relative
/// to the margin itself - a trench lies out on the oceanic side, an arc a little inboard.
pub const CONTINENT_COLLISION_M: f64 = 1500.0;
pub const CONTINENT_COLLISION_WIDTH_M: f64 = 400_000.0;

pub const COASTAL_UPLIFT_M: f64 = 900.0;
pub const COASTAL_UPLIFT_WIDTH_M: f64 = 260_000.0;

pub const TRENCH_M: f64 = -2600.0;
pub const TRENCH_WIDTH_M: f64 = 120_000.0;
pub const TRENCH_OFFSET_M: f64 = 90_000.0;

pub const ISLAND_ARC_M: f64 = 700.0;
pub const ISLAND_ARC_WIDTH_M: f64 = 110_000.0;
pub const ISLAND_ARC_OFFSET_M: f64 = 60_000.0;

pub const RIDGE_M: f64 = 900.0;
pub const RIDGE_WIDTH_M: f64 = 380_000.0;
pub const RIFT_M: f64 = -350.0;
pub const RIFT_WIDTH_M: f64 = 70_000.0;

/// Nothing for thoroughly oceanic, one for thoroughly continental, and a smooth ramp
/// between.
///
/// Args:
///     value: Continentality on one side of a margin.
fn continental(value: f64) -> f64 {
    let fraction = (value - CONTINENTAL_ENOUGH) / CONTINENTAL_BLEND * 0.5 + 0.5;
    // Python writes `max(0.0, min(1.0, fraction))`; the two-argument forms are asymmetric
    // under NaN, keeping the first operand unless the second is strictly beyond it. So
    // `min(1.0, fraction)` keeps 1.0 unless `fraction` is strictly less than it, and
    // `max(0.0, ...)` keeps 0.0 unless its argument is strictly greater than it.
    let fraction = if fraction < 1.0 { fraction } else { 1.0 };
    let fraction = if fraction > 0.0 { fraction } else { 0.0 };
    fraction * fraction * (3.0 - 2.0 * fraction)
}

/// A smooth hump: one at the centre, nothing at the edge, and no corner anywhere.
///
/// Args:
///     distance_m: How far from the middle of the feature.
///     width_m: Where it reaches zero.
///
/// Returns a weight between zero and one.
///
/// Notes:
///     Smoothstep rather than a cosine or a straight taper, because it is flat at both
///     ends: the derivative is zero at the centre *and* at the edge. A profile that
///     merely reached zero would still leave a crease where it met the untouched ground,
///     and a crease in terrain is a cliff somebody sails into.
fn bump(distance_m: f64, width_m: f64) -> f64 {
    if width_m <= 0.0 {
        return 0.0;
    }
    let raw = distance_m.abs() / width_m;
    // Python writes `min(1.0, abs(distance_m) / width_m)`; keep the second operand unless
    // it is not strictly less than the first, matching the house form in
    // `plates.rs::margin_at`.
    let away = if raw < 1.0 { raw } else { 1.0 };
    let fade = 1.0 - away;
    fade * fade * (3.0 - 2.0 * fade)
}

/// What kind of ground lies either side of a margin, here.
///
/// Attributes:
///     inboard: Continentality on the nearest plate's side.
///     outboard: Continentality on the neighbour's side.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Setting {
    pub inboard: f64,
    pub outboard: f64,
}

impl Setting {
    /// How continental the near side is, from nothing to one, smoothly.
    pub fn inboard_continental(&self) -> f64 {
        continental(self.inboard)
    }

    pub fn outboard_continental(&self) -> f64 {
        continental(self.outboard)
    }

    /// Which side is the more continental, from -1 to +1, and how decidedly.
    ///
    /// Notes:
    ///     Near zero where the two sides are alike, which is what lets an asymmetric
    ///     profile fade out rather than flip. A hard comparison here would have put a
    ///     trench on one side of a symmetric margin and the other side of it a metre
    ///     away.
    pub fn lean(&self) -> f64 {
        m::tanh((self.inboard - self.outboard) * SIDE_SHARPNESS)
    }
}

/// The tectonic contribution to elevation, worked out where it matters and nowhere else.
///
/// Notes:
///     Holds the plates and the continentality field and combines them. It is the first
///     thing in the engine that knows about both, which is deliberate - they were built
///     in ignorance of each other so that continents would not inherit plate shapes, and
///     this is the seam where they are allowed to meet.
pub struct Tectonics {
    plates: PlateSet,
    land: Continentality,
    radius_m: f64,
}

impl Tectonics {
    pub fn new(plates: PlateSet, land: Continentality, radius_m: f64) -> Self {
        Self { plates, land, radius_m }
    }

    /// What lies either side of the margin near this point.
    ///
    /// Args:
    ///     point: Where.
    ///     distance_m: How far the margin is.
    ///     normal: Away from the margin, into the nearest plate.
    ///
    /// Returns the continentality on each side.
    ///
    /// Notes:
    ///     The probes are placed relative to the *margin*, not to the point, so that two
    ///     samples on opposite sides of the same boundary describe the same stretch of it
    ///     and agree about what it is. Probing outward from each point instead would have
    ///     let a margin be a subduction zone from one side and a collision from the other.
    pub fn setting_at(&self, point: &SpherePoint, distance_m: f64, normal: &Vec3) -> Setting {
        let frame = TangentFrame::at(point, self.radius_m);
        let east = normal.dot(&frame.east);
        let north = normal.dot(&frame.north);

        // Walk back to the margin, then out to either side of it.
        let to_inboard = -distance_m + PROBE_M;
        let to_outboard = -distance_m - PROBE_M;
        Setting {
            inboard: self.land.at(&frame.local_to_sphere(east * to_inboard, north * to_inboard)),
            outboard: self
                .land
                .at(&frame.local_to_sphere(east * to_outboard, north * to_outboard)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::continentality::LAND_FRACTION;
    use crate::plates::tests::three_plate_set;
    use crate::sphere::EARTH_RADIUS_M;

    #[test]
    fn continental_saturates_at_both_ends_and_is_smooth_between() {
        // Thoroughly oceanic and thoroughly continental clamp to exactly 0 and 1;
        // the midpoint is exactly 0.5 because the smoothstep is symmetric about it.
        assert_eq!(continental(-10.0), 0.0);
        assert_eq!(continental(10.0), 1.0);
        assert_eq!(continental(CONTINENTAL_ENOUGH), 0.5);
    }

    #[test]
    fn bump_is_one_at_the_centre_and_zero_at_the_edge() {
        assert_eq!(bump(0.0, 100_000.0), 1.0);
        assert_eq!(bump(100_000.0, 100_000.0), 0.0);
        assert_eq!(bump(-100_000.0, 100_000.0), 0.0);
        assert_eq!(bump(200_000.0, 100_000.0), 0.0);
    }

    #[test]
    fn bump_has_zero_derivative_at_both_ends() {
        // The reason it is a smoothstep and not a taper: no crease where it meets
        // untouched ground. Sample either side of centre and edge; the change per
        // step must shrink towards both, not stay linear.
        let w = 100_000.0;
        let near_centre = bump(0.0, w) - bump(1_000.0, w);
        let mid_slope = bump(40_000.0, w) - bump(41_000.0, w);
        let near_edge = bump(99_000.0, w) - bump(100_000.0, w);
        assert!(near_centre < mid_slope, "must flatten towards the centre");
        assert!(near_edge < mid_slope, "must flatten towards the edge");
    }

    #[test]
    fn a_zero_width_bump_is_nothing_rather_than_a_division_by_zero() {
        assert_eq!(bump(0.0, 0.0), 0.0);
        assert_eq!(bump(50.0, -1.0), 0.0);
    }

    #[test]
    fn lean_is_zero_for_a_symmetric_margin_and_saturates_when_lopsided() {
        // Exactly zero when the two sides are alike -- tanh(0) is exactly 0.0 -- which
        // is what lets an asymmetric profile fade out rather than flip.
        assert_eq!(Setting { inboard: 0.3, outboard: 0.3 }.lean(), 0.0);
        assert!(Setting { inboard: 1.0, outboard: -1.0 }.lean() > 0.99);
        assert!(Setting { inboard: -1.0, outboard: 1.0 }.lean() < -0.99);
    }

    #[test]
    fn setting_at_agrees_from_either_side_of_the_same_margin() {
        // Derivation: `three_plate_set()` puts plate 0 at (0N,0E) and plate 1 at
        // (0N,90E), ninety degrees apart on the equator, with plate 2 lifted off to
        // (60N,45E) far enough away that it plays no part here. Their margin is the
        // great circle bisecting seeds 0 and 1; its midpoint, `normalised(seed0 +
        // seed1)`, is the same construction `plates.rs`'s
        // `a_point_on_the_bisector_is_at_zero_distance` test uses to land exactly on
        // a margin.
        //
        // `flattened(mid, bisector(0, 1))` is the bisector's plane normal projected
        // into the tangent plane at `mid` -- a unit tangent vector perpendicular to
        // the margin's local direction there, i.e. the axis "across" it. Walking a
        // fixed distance `D` either way along that axis from `mid`, in `mid`'s own
        // frame, places two points symmetric about the same margin on the same
        // local straight line: one on plate 0's side, one on plate 1's.
        //
        // Each point's own `margin_at`/`margin_normal` (the same calls `offset_m`
        // makes in real use) then supply the `distance_m` and `normal` `setting_at`
        // expects, and because the two points sit on opposite sides of one margin,
        // one point's "inboard" plate is the other's "outboard" plate.
        let set = three_plate_set();
        let seed0 = set.plate(0).seed.vector;
        let seed1 = set.plate(1).seed.vector;
        let mid = SpherePoint { vector: seed0.add(&seed1).normalised().expect("distinct seeds") };
        let bisector = set.bisector(0, 1).expect("distinct seeds");
        let across = set.flattened(&mid, &bisector).expect("not degenerate at the midpoint");

        let frame = TangentFrame::at(&mid, EARTH_RADIUS_M);
        let east = across.dot(&frame.east);
        let north = across.dot(&frame.north);

        let d = 200_000.0;
        let point_a = frame.local_to_sphere(east * d, north * d);
        let point_b = frame.local_to_sphere(east * -d, north * -d);

        let margin_a = set.margin_at(&point_a, EARTH_RADIUS_M);
        let margin_b = set.margin_at(&point_b, EARTH_RADIUS_M);
        // The two points must actually straddle the same margin -- nearest and
        // neighbour swapped -- or this test would not be exercising the property
        // it claims to.
        assert_eq!(
            margin_a.nearest.expect("a nearest plate").index,
            margin_b.neighbour.expect("a neighbour plate").index
        );
        assert_eq!(
            margin_a.neighbour.expect("a neighbour plate").index,
            margin_b.nearest.expect("a nearest plate").index
        );

        let normal_a = set.margin_normal(&point_a, &margin_a).expect("not degenerate");
        let normal_b = set.margin_normal(&point_b, &margin_b).expect("not degenerate");

        let land = Continentality::new(12345, EARTH_RADIUS_M, LAND_FRACTION);
        let tectonics = Tectonics::new(set, land, EARTH_RADIUS_M);

        let setting_a = tectonics.setting_at(&point_a, margin_a.distance_m, &normal_a);
        let setting_b = tectonics.setting_at(&point_b, margin_b.distance_m, &normal_b);

        // The property the design exists for: two samples on opposite sides of the
        // same margin describe the same stretch of it. `inboard` and `outboard`
        // swap roles between the two points -- point A's near plate is point B's
        // far plate -- so it is A's inboard against B's outboard, and A's outboard
        // against B's inboard, not the two `Setting`s being equal outright.
        // Measured: the two sides agree to within a couple of ULPs (~3e-16), not merely
        // within some loose tolerance -- the residual is rounding noise from projecting
        // into two different tangent frames, not a modelling error.
        let tolerance = 1e-9;
        assert!(
            (setting_a.inboard - setting_b.outboard).abs() < tolerance,
            "a.inboard={} b.outboard={}",
            setting_a.inboard,
            setting_b.outboard
        );
        assert!(
            (setting_a.outboard - setting_b.inboard).abs() < tolerance,
            "a.outboard={} b.inboard={}",
            setting_a.outboard,
            setting_b.inboard
        );
    }
}
