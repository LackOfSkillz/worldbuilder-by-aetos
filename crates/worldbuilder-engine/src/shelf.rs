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
}
