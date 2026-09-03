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

use crate::detmath as m;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
