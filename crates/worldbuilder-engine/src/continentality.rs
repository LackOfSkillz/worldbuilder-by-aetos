//! The broad shape of land and sea on a world.
//!
//! Ported from `worldbuilder/terrain/continentality.py`.
//!
//! This takes a seed and nothing else. It cannot consult the plates because it has no way
//! to reach them, which is the point — an architectural claim enforced by the import list
//! rather than by a comment asking people to behave. Do not add a `plates` import here.

use crate::detmath as m;
use crate::noise::Noise;
use crate::sphere::{SpherePoint, EARTH_RADIUS_M};

/// Cycles per unit of noise space at the first octave. About one and a quarter, which puts
/// the largest features somewhere near five thousand kilometres across — a continent, or
/// an ocean basin, and nothing smaller.
pub const BASE_FREQUENCY: f64 = 1.25;

/// How many octaves. Few, deliberately. Enough to stop the landmasses being simple blobs,
/// not enough to start carving a coast.
pub const OCTAVES: u32 = 4;

/// How high a continental interior stands, and how deep an ocean basin lies, in metres
/// before anything else has its say.
pub const CONTINENT_M: f64 = 700.0;
pub const ABYSS_M: f64 = -4600.0;

/// How much of the surface is dry, unless a world asks otherwise. Earth is about 29 per
/// cent, and it is the single most powerful thing a developer can turn.
pub const LAND_FRACTION: f64 = 0.29;

/// How many points to sample when working out where sea level falls.
pub const CALIBRATION_SAMPLES: usize = 4000;

/// How far apart the probes are when measuring which way the land rises, in metres.
pub const GRADIENT_STEP_M: f64 = 20000.0;

/// Salted so this field is independent of any other on the same world.
pub const NOISE_SALT: u64 = 0x0C0FFEE;

/// Which way continentality increases, here, and how sharply. Change per metre.
#[derive(Debug, Clone, Copy)]
pub struct Gradient {
    pub east: f64,
    pub north: f64,
}

impl Gradient {
    pub fn magnitude(&self) -> f64 {
        m::hypot(self.east, self.north)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Continentality {
    pub radius_m: f64,
    pub land_fraction: f64,
    noise: Noise,
    shore: f64,
    spread: f64,
}

impl Continentality {
    pub fn new(world_seed: u64, radius_m: f64, land_fraction: f64) -> Self {
        // shore and spread are placeholders until the calibration lands in the next task.
        Self {
            radius_m,
            land_fraction,
            noise: Noise::new(world_seed, NOISE_SALT),
            shore: 0.0,
            spread: 1.0,
        }
    }

    /// The raw field, before sea level has been decided.
    pub fn at(&self, point: &SpherePoint) -> f64 {
        let v = point.vector;
        self.noise.fbm(v.x, v.y, v.z, BASE_FREQUENCY, OCTAVES, 0.5, 2.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sphere::SpherePoint;

    #[test]
    fn the_constants_match_the_python() {
        assert_eq!(BASE_FREQUENCY.to_bits(), 1.25f64.to_bits());
        assert_eq!(OCTAVES, 4);
        assert_eq!(CONTINENT_M.to_bits(), 700.0f64.to_bits());
        assert_eq!(ABYSS_M.to_bits(), (-4600.0f64).to_bits());
        assert_eq!(LAND_FRACTION.to_bits(), 0.29f64.to_bits());
        assert_eq!(CALIBRATION_SAMPLES, 4000);
        assert_eq!(GRADIENT_STEP_M.to_bits(), 20000.0f64.to_bits());
        assert_eq!(NOISE_SALT, 0x0C0FFEE);
    }

    #[test]
    fn a_gradient_reports_its_magnitude() {
        let g = Gradient { east: 3.0, north: 4.0 };
        assert_eq!(g.magnitude().to_bits(), 5.0f64.to_bits());
    }

    #[test]
    fn the_field_varies_across_the_planet() {
        let c = Continentality::new(12345, EARTH_RADIUS_M, LAND_FRACTION);
        let a = c.at(&SpherePoint::from_latlon(0.0, 0.0));
        let b = c.at(&SpherePoint::from_latlon(45.0, 90.0));
        assert_ne!(a.to_bits(), b.to_bits());
        assert!(a.is_finite() && b.is_finite());
    }

    #[test]
    fn the_field_is_reproducible() {
        let a = Continentality::new(12345, EARTH_RADIUS_M, LAND_FRACTION);
        let b = Continentality::new(12345, EARTH_RADIUS_M, LAND_FRACTION);
        let p = SpherePoint::from_latlon(31.0, 7.0);
        assert_eq!(a.at(&p).to_bits(), b.at(&p).to_bits());
    }
}
