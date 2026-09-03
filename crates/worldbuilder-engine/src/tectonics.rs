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
use crate::kinematics::{motion_between, ACROSS_ENOUGH};
use crate::plates::{Plate, PlateSet};
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

    /// How much the plates raise or lower the ground here.
    ///
    /// Args:
    ///     point: Anywhere on the planet.
    ///
    /// Returns metres, to be *added* to the continental base elevation.
    ///
    /// Notes:
    ///     **Every margin in range, summed - not the nearest one, chosen.**
    ///
    ///     Picking the nearest margin is not continuous even though its distance is. The
    ///     identity of the neighbour jumps: at a point equidistant from two of a plate's
    ///     margins the choice flips under a step of a metre, and the relative motion, the
    ///     normal and what lies either side all flip with it. Measured at five hundred and
    ///     sixty metres of cliff, a hundred and thirty kilometres from any boundary, where
    ///     one margin was transform and the other divergent.
    ///
    ///     Summing is continuous because each term depends only on its own distance and
    ///     fades to nothing at its own range. It is also the truer answer: near a triple
    ///     junction there really are two margins acting on the ground.
    ///
    ///     Costs nothing where nothing is happening. A plate interior fails the distance
    ///     test on every bisector, having done one dot product each - and that is 69 per
    ///     cent of the planet.
    ///
    ///     **Iteration order is load-bearing.** Floating-point addition is not
    ///     associative, so the total depends on the order the margins are summed in.
    ///     `margins_within` returns them in plate-position order, and this loop must
    ///     accumulate in that same order - no sorting, no reversing, no parallel
    ///     accumulation.
    pub fn offset_m(&self, point: &SpherePoint) -> f64 {
        let (nearest, margins) =
            self.plates.margins_within(point, MAX_TECTONIC_RANGE_M, self.radius_m);
        if margins.is_empty() {
            return 0.0;
        }
        let near = match nearest {
            Some(plate) => plate,
            None => return 0.0,
        };

        let mut total = 0.0;
        for margin in &margins {
            // `margin.normal` is the bisector's plane normal; `flattened` projects it
            // into the tangent plane at `point` to get the across-margin direction
            // `from_margin` needs. Skipped entirely, not zeroed, when the projection is
            // degenerate - a zero contribution and a skip are different things, and the
            // Python skips.
            let normal = match self.plates.flattened(point, &margin.normal) {
                Some(n) => n,
                None => continue,
            };
            total += margin.weight
                * self.from_margin(point, &near, &margin.other, margin.distance_m, &normal);
        }
        total
    }

    /// The macro elevation: continental base plus whatever the plates have done to it.
    ///
    /// Args:
    ///     point: Anywhere on the planet.
    ///
    /// Returns metres, relative to datum, before shelves or detail.
    pub fn elevation_m(&self, point: &SpherePoint) -> f64 {
        self.land.base_elevation(point) + self.offset_m(point)
    }

    /// One margin's contribution to the ground here.
    ///
    /// Args:
    ///     point: Where.
    ///     near: The plate the point is on.
    ///     far: The plate across this margin.
    ///     distance_m: How far the margin is.
    ///     normal: Across it, tangent to the surface, pointing towards `near`.
    ///
    /// Returns metres, which may be zero, and usually is.
    fn from_margin(
        &self,
        point: &SpherePoint,
        near: &Plate,
        far: &Plate,
        distance_m: f64,
        normal: &Vec3,
    ) -> f64 {
        let motion = motion_between(near, far, point, normal, self.radius_m);

        // How much of the relative motion is across the margin rather than along it, from
        // -1 (pulling apart) through 0 (pure sliding) to +1 (head on).
        //
        // **Weighed, not classified.** `motion.kind` is a name given by a threshold, and
        // using the name to pick a profile meant a margin drifting from convergent to
        // transform went from a full mountain belt to nothing in one step. The name
        // survives for diagnostics; the terrain uses the number.
        let speed = m::hypot(motion.closing_m_per_myr, motion.sliding_m_per_myr);
        // Safe regardless of `hypot`'s algorithm: Task 1 measured that `math.hypot` and
        // `libm::hypot` differ by at most 1 ULP but both are exactly zero only when both
        // arguments are exactly zero. So this comparison decides identically either way.
        if speed <= 0.0 {
            return 0.0;
        }
        let across = motion.closing_m_per_myr / speed;

        // A transform margin still leaves no mark. It arrives at no mark smoothly.
        let mut engagement = (across.abs() - ACROSS_ENOUGH) / (1.0 - ACROSS_ENOUGH);
        // This is the one branch that genuinely depends on `hypot`'s precision: `across`
        // is built from `speed`, and a 1-ULP disagreement between `math.hypot` and
        // `libm::hypot` propagates into it. Task 1 measured the margin here at 1.19e-4 --
        // about 1.07e12 ULP -- twelve orders of magnitude clear of where a 1-ULP `hypot`
        // disagreement could flip this comparison.
        if engagement <= 0.0 {
            return 0.0;
        }
        // Python writes `min(1.0, engagement)`; house form keeps the first operand unless
        // the second is strictly less than it.
        engagement = if engagement < 1.0 { engagement } else { 1.0 };
        engagement = engagement * engagement * (3.0 - 2.0 * engagement);

        // Python writes `min(1.0, speed / FULL_RATE_M_PER_MYR)`.
        let rate_fraction = speed / FULL_RATE_M_PER_MYR;
        let capped_rate = if rate_fraction < 1.0 { rate_fraction } else { 1.0 };
        let strength = capped_rate * engagement;
        // Nearly unreachable, not dead: this can only fire if `engagement`'s smoothstep
        // above has underflowed to exactly zero, which needs the pre-smoothstep
        // `engagement` below roughly 1e-162. Ported and kept rather than deleted as
        // unreachable.
        if strength <= 0.0 {
            return 0.0;
        }

        if across < 0.0 {
            // Pulling apart. Symmetric about the axis, so it needs no sense of side.
            //
            // Safe regardless of `hypot`'s precision, even though it looks like the most
            // dangerous of these branches: `hypot` is never negative, and the zero case
            // already returned above, so `speed` is strictly positive here and dividing
            // by it cannot change the sign of `motion.closing_m_per_myr`. This branch is
            // decided by that sign, which is algebraic, not measured through `hypot`.
            return strength
                * (RIDGE_M * bump(distance_m, RIDGE_WIDTH_M)
                    + RIFT_M * bump(distance_m, RIFT_WIDTH_M));
        }

        let setting = self.setting_at(point, distance_m, normal);
        let inboard = setting.inboard_continental();
        let outboard = setting.outboard_continental();
        let collision = inboard * outboard;
        let oceanic = (1.0 - inboard) * (1.0 - outboard);
        // Python writes `max(0.0, 1.0 - collision - oceanic)`; house form keeps the first
        // operand unless the second is strictly greater than it.
        let remainder = 1.0 - collision - oceanic;
        let subduction = if remainder > 0.0 { remainder } else { 0.0 };

        // The convergent response at a signed distance across the margin. A closure
        // rather than a free function: it exists only to capture `collision`, `oceanic`
        // and `subduction`, which are local to this call, and a free function would need
        // all three threaded through as extra parameters for no benefit.
        let profile = |across_m: f64| -> f64 {
            let collided = CONTINENT_COLLISION_M * bump(across_m, CONTINENT_COLLISION_WIDTH_M);
            let trench = TRENCH_M * bump(across_m + TRENCH_OFFSET_M, TRENCH_WIDTH_M);
            let arc = ISLAND_ARC_M * bump(across_m - ISLAND_ARC_OFFSET_M, ISLAND_ARC_WIDTH_M);
            // The literal below is deliberately bare: it coincidentally equals
            // `RIFT_WIDTH_M`, but the two are unrelated quantities and binding this one
            // to that constant would couple two profiles that must be free to vary
            // independently.
            let uplift = COASTAL_UPLIFT_M * bump(across_m - 70_000.0, COASTAL_UPLIFT_WIDTH_M);
            collision * collided + oceanic * (arc + trench) + subduction * (uplift + trench)
        };

        // Which side of the margin this point is on, weighed rather than decided.
        //
        // The obvious form is `signed = distance * lean`, and it is wrong in a way that
        // took a diagnostic to see: scaling the axis *compresses* distance, so with a
        // lean of -0.22 a point four hundred and nineteen kilometres out mapped to -90
        // km, which is exactly where the trench sits. The trench fired at four hundred
        // kilometres and the range gate then cut it off mid-profile.
        //
        // The distance stays true. The profile is evaluated on both sides and blended by
        // the lean, which keeps every feature at its intended range and reaches zero by
        // the gate because each profile does.
        let toward = (1.0 + setting.lean()) * 0.5;
        strength * (toward * profile(distance_m) + (1.0 - toward) * profile(-distance_m))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::continentality::LAND_FRACTION;
    use crate::plates::tests::three_plate_set;
    use crate::sphere::EARTH_RADIUS_M;

    impl Tectonics {
        /// Test-only window onto the continental base, so `elevation_is_the_base_...`
        /// can compute the same sum `elevation_m` computes internally and compare
        /// bit-for-bit, without duplicating `Continentality`'s internals in the test.
        fn land_base_elevation_for_test(&self, point: &SpherePoint) -> f64 {
            self.land.base_elevation(point)
        }
    }

    /// A `Tectonics` over `three_plate_set()`, for the `offset_m`/`elevation_m` tests
    /// that only need *some* world, not a particular geometry.
    fn test_world() -> Tectonics {
        let land = Continentality::new(20260902, EARTH_RADIUS_M, LAND_FRACTION);
        Tectonics::new(three_plate_set(), land, EARTH_RADIUS_M)
    }

    #[test]
    fn a_plate_interior_contributes_exactly_nothing() {
        // three_plate_set's nearest bisector to seed 0 is about 3,900 km away, an order
        // of magnitude beyond MAX_TECTONIC_RANGE_M, so margins_within returns an empty
        // list and offset_m exits before doing any arithmetic at all. Exactly zero, not
        // approximately -- it is a literal early return.
        //
        // Confirmed rather than assumed: querying three_plate_set() directly at this
        // point returns zero margins (measured), so the assertion below exercises the
        // early return and not a sum that happens to cancel.
        let set = three_plate_set();
        let point = SpherePoint::from_latlon(0.0, 0.0);
        let (_, found) = set.margins_within(&point, MAX_TECTONIC_RANGE_M, EARTH_RADIUS_M);
        assert!(found.is_empty(), "expected no margins in range, found {}", found.len());

        let world = test_world();
        assert_eq!(world.offset_m(&SpherePoint::from_latlon(0.0, 0.0)), 0.0);
    }

    #[test]
    fn elevation_is_the_base_plus_the_offset_bit_for_bit() {
        // elevation_m is defined as exactly that sum, in that order. Anything less than
        // bit-equality means the composition was rewritten.
        let world = test_world();
        let point = SpherePoint::from_latlon(12.0, 20.0);
        let expected = world.land_base_elevation_for_test(&point) + world.offset_m(&point);
        assert_eq!(world.elevation_m(&point).to_bits(), expected.to_bits());
    }

    #[test]
    fn a_point_near_two_margins_sums_both_contributions() {
        // The reason offset_m exists. Find a point with more than one margin in range
        // and assert the total differs from either margin's contribution alone -- that
        // is what distinguishes summing from choosing, and choosing was worth 560 m of
        // cliff.
        //
        // three_plate_set() has no such point: its three seeds sit tens of thousands of
        // kilometres apart (two 90 degrees apart on the equator, one lifted to 60N),
        // so no two margins ever fall within MAX_TECTONIC_RANGE_M (420 km) of the same
        // spot -- confirmed by sampling margins_within across that set and finding at
        // most one margin in range anywhere.
        //
        // A dedicated three-plate set with seeds a few degrees apart puts a genuine
        // near-triple-junction inside MAX_TECTONIC_RANGE_M. Found by brute-force
        // sampling of margins_within over a small lat/lon grid around the seeds' rough
        // midpoint: with seeds at (0,0), (0,4) and (3,2), the point (0.5N, 1.5E) has
        // two margins in range, at measured distances of about 55,595 m and 61,682 m --
        // neither equal to the other, so the point is not an artefact of symmetry.
        // Distinct rates about a shared pole, as in `lopsided_world` above: relative
        // motion is the *difference* of two plates' angular velocities, so equal rates
        // would leave every margin here motionless (speed == 0.0, from_margin's first
        // early return) regardless of geometry.
        let plate = |index: usize, lat: f64, lon: f64, rate: f64| Plate {
            index,
            seed: SpherePoint::from_latlon(lat, lon),
            euler_pole: SpherePoint::from_latlon(80.0, 5.0),
            rate_rad_per_myr: rate,
        };
        let set = PlateSet::new(vec![
            plate(0, 0.0, 0.0, 0.02),
            plate(1, 0.0, 4.0, -0.015),
            plate(2, 3.0, 2.0, 0.01),
        ]);
        let point = SpherePoint::from_latlon(0.5, 1.5);

        let (nearest, margins) = set.margins_within(&point, MAX_TECTONIC_RANGE_M, EARTH_RADIUS_M);
        assert_eq!(margins.len(), 2, "expected exactly two margins in range at this point");
        let near = nearest.expect("a nearest plate when margins were found");

        let land = Continentality::new(20260902, EARTH_RADIUS_M, LAND_FRACTION);
        let world = Tectonics::new(set, land, EARTH_RADIUS_M);

        let total = world.offset_m(&point);

        // Each margin's own contribution, computed the same way offset_m computes it,
        // so the comparison is against what "choosing just this one" would have given.
        let solo: Vec<f64> = margins
            .iter()
            .filter_map(|margin| {
                let normal = world.plates.flattened(&point, &margin.normal)?;
                let contribution = world.from_margin(
                    &point,
                    &near,
                    &margin.other,
                    margin.distance_m,
                    &normal,
                );
                Some(margin.weight * contribution)
            })
            .collect();
        assert_eq!(solo.len(), 2, "both margins must survive the flattened() projection");

        assert_ne!(total, solo[0], "the sum must not collapse to just the first margin");
        assert_ne!(total, solo[1], "the sum must not collapse to just the second margin");
        assert_eq!(
            total.to_bits(),
            (solo[0] + solo[1]).to_bits(),
            "the total must be exactly the two contributions summed in plate-position order"
        );
    }

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

    /// A fixed point, margin geometry and pair of plates for exercising `from_margin`
    /// directly, bypassing `offset_m`'s margin lookup.
    ///
    /// Both Euler poles sit at the north pole, so both plates' angular velocities are
    /// `(0, 0, rate)` and the point is on the equator -- the same construction
    /// `kinematics.rs`'s `two_plates_driving_into_each_other_are_convergent` uses. The
    /// relative velocity is then due east-west and the normal `(0, 1, 0)` points due
    /// east, straight along it, so the margin is fully engaged (no sliding component at
    /// all) regardless of distance -- `from_margin`'s `strength` does not depend on
    /// `distance_m`, only on `near`, `far`, `point` and `normal`, which this struct holds
    /// fixed.
    struct LopsidedWorld {
        tectonics: Tectonics,
        point: SpherePoint,
        near: Plate,
        far: Plate,
        normal: Vec3,
    }

    impl LopsidedWorld {
        fn from_margin_for_test(&self, distance_m: f64) -> f64 {
            self.tectonics.from_margin(&self.point, &self.near, &self.far, distance_m, &self.normal)
        }
    }

    fn lopsided_world() -> LopsidedWorld {
        let near = Plate {
            index: 0,
            seed: SpherePoint::from_latlon(0.0, 0.0),
            euler_pole: SpherePoint::from_latlon(90.0, 0.0),
            rate_rad_per_myr: 0.01,
        };
        let far = Plate {
            index: 1,
            seed: SpherePoint::from_latlon(0.0, 10.0),
            euler_pole: SpherePoint::from_latlon(90.0, 0.0),
            rate_rad_per_myr: 0.02,
        };
        let point = SpherePoint::from_latlon(0.0, 0.0);
        let normal = Vec3::new(0.0, 1.0, 0.0);

        // A world seed chosen only because it happens to give the two probe points --
        // 300 km either side of the margin, per `PROBE_M` -- genuinely different
        // continentality. Confirmed by the `lean_is_genuinely_non_zero` test below,
        // which is what makes the 419 km test meaningful rather than vacuous.
        let land = Continentality::new(20260902, EARTH_RADIUS_M, LAND_FRACTION);
        let plates = PlateSet::new(vec![near, far]);
        let tectonics = Tectonics::new(plates, land, EARTH_RADIUS_M);

        LopsidedWorld { tectonics, point, near, far, normal }
    }

    #[test]
    fn lopsided_world_has_a_genuinely_non_zero_lean() {
        // Guards against the exact failure mode that made an earlier fade test in this
        // port vacuous: a symmetric margin would make the 419 km test below pass for the
        // wrong reason, by making `toward` exactly 0.5 rather than by making both sides
        // of the blend genuinely zero.
        let world = lopsided_world();
        let setting = world.tectonics.setting_at(&world.point, 419_000.0, &world.normal);
        let lean = setting.lean();
        // Measured: 0.679.
        assert!(lean.abs() > 1e-6, "expected a genuinely non-zero lean, got {lean}");
    }

    #[test]
    fn a_close_convergent_margin_contributes_something_non_zero() {
        // Confirms the 419 km test below cannot pass merely because `from_margin`
        // returns zero everywhere -- close in, at least one profile term must be within
        // its width on the near side of the blend.
        let world = lopsided_world();
        let contribution = world.from_margin_for_test(10_000.0);
        assert_ne!(contribution, 0.0, "a margin 10 km away must contribute something");
    }

    #[test]
    fn every_profile_reaches_zero_before_the_range_gate() {
        // MAX_TECTONIC_RANGE_M's own docstring: "Every profile below must reach exactly
        // zero by here, or the gate itself becomes a cliff." At 419 km every bump
        // argument is outside its width, on both sides of the blend, so the sum is
        // exactly zero -- not merely small.
        //
        // This is the regression test for the 419 km mismapping. The obvious form,
        // `signed = distance * lean`, compresses the axis: with a lean of -0.22 this
        // same point maps to about -92 km, which is the trench centre, and returns
        // roughly -2597 m instead of zero.
        let world = lopsided_world();
        let contribution = world.from_margin_for_test(419_000.0);
        assert_eq!(contribution, 0.0, "a margin 419 km away must contribute exactly nothing");
    }
}
