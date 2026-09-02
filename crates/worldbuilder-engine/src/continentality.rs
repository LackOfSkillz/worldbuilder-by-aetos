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
        // Calibration runs before the struct is built, so no partially-built value with
        // placeholder shore/spread can ever exist — mirroring the Python, where
        // `_calibrate()` runs inside `__init__` and there is no window to observe an
        // uncalibrated instance.
        let noise = Noise::new(world_seed, NOISE_SALT);
        let (shore, spread) = Self::calibrate(&noise, land_fraction);
        Self {
            radius_m,
            land_fraction,
            noise,
            shore,
            spread,
        }
    }

    /// Where sea level falls, and how varied the field is.
    ///
    /// The sample points are a fixed Fibonacci spiral and the field is a pure function, so
    /// this is generated-and-stored and still perfectly deterministic.
    fn calibrate(noise: &Noise, land_fraction: f64) -> (f64, f64) {
        let golden = core::f64::consts::PI * (3.0 - m::sqrt(5.0));
        let n = CALIBRATION_SAMPLES;
        let mut values: Vec<f64> = Vec::with_capacity(n);

        for index in 0..n {
            let z = 1.0 - 2.0 * (index as f64 + 0.5) / (n as f64); // cast-ok: loop counter to float, no truncation
            let inner = 1.0 - z * z;
            // Python writes max(0.0, 1.0 - z*z); two-argument max returns the second
            // argument when the comparison is false, so a NaN inner would yield 0.0.
            let ring = m::sqrt(if inner > 0.0 { inner } else { 0.0 });
            let angle = golden * index as f64; // cast-ok: loop counter to float, no truncation
            let point = SpherePoint {
                vector: crate::vectors::Vec3::new(m::cos(angle) * ring, m::sin(angle) * ring, z),
            };
            // The sample point is deliberately NOT normalised — the Python builds the
            // vector directly and hands it to SpherePoint, and the spiral already lies on
            // the unit sphere to within rounding.
            let v = point.vector;
            values.push(noise.fbm(v.x, v.y, v.z, BASE_FREQUENCY, OCTAVES, 0.5, 2.0));
        }

        // Python's list.sort() on floats and Rust's stable sort_by agree: no NaN is
        // produced here, and -0.0 compares equal to 0.0 in both, with stability keeping
        // the original order in that case.
        values.sort_by(|a, b| a.partial_cmp(b).expect("the field produces no NaN"));

        let last = (n - 1) as f64; // cast-ok: count to float, exact for n far below 2^53
        let shore_index = ((1.0 - land_fraction) * last) as usize; // cast-ok: truncation, matching Python's int()
        let spread_index = (0.84 * last) as usize; // cast-ok: truncation, matching Python's int()
        let shore = values[shore_index];
        let middle = values[n / 2];
        let difference = values[spread_index] - middle;
        // Python writes `... or 1e-6`, and both 0.0 and -0.0 are falsy there, so either
        // becomes 1e-6. NaN is truthy and passes through unchanged.
        let spread = if difference == 0.0 { 1e-6 } else { difference };
        (shore, spread)
    }

    /// The raw field, before sea level has been decided.
    pub fn at(&self, point: &SpherePoint) -> f64 {
        let v = point.vector;
        self.noise.fbm(v.x, v.y, v.z, BASE_FREQUENCY, OCTAVES, 0.5, 2.0)
    }

    /// How far above the shoreline this point stands, in field units. Zero exactly at the
    /// coast, positive inland.
    pub fn above_shore(&self, point: &SpherePoint) -> f64 {
        self.at(point) - self.shore
    }

    /// Elevation relative to datum, before tectonics or detail.
    pub fn base_elevation(&self, point: &SpherePoint) -> f64 {
        self.elevation_from_above(self.above_shore(point) / self.spread)
    }

    /// The curve itself, separated so it can be exercised without hunting for a point that
    /// happens to land at a given height.
    pub fn elevation_from_above(&self, above: f64) -> f64 {
        if above >= 0.0 {
            // Python: CONTINENT_M * min(1.0, above) ** 0.75
            let capped = if above < 1.0 { above } else { 1.0 };
            CONTINENT_M * m::powf(capped, 0.75)
        } else {
            // Linear on the seaward side, and that number was measured rather than chosen.
            // Python: ABYSS_M * min(1.0, -above)
            let depth = -above;
            let capped = if depth < 1.0 { depth } else { 1.0 };
            ABYSS_M * capped
        }
    }

    #[cfg(test)]
    pub fn shore_for_test(&self) -> f64 {
        self.shore
    }

    #[cfg(test)]
    pub fn spread_for_test(&self) -> f64 {
        self.spread
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

    #[test]
    fn calibration_reproduces_the_python_reference() {
        // Measured from the Python on seed 12345 at the default land fraction.
        let c = Continentality::new(12345, EARTH_RADIUS_M, LAND_FRACTION);
        assert!((c.shore_for_test() - 0.09556581019557257).abs() < 1e-12,
                "shore was {}", c.shore_for_test());
        assert!((c.spread_for_test() - 0.1984287160252961).abs() < 1e-12,
                "spread was {}", c.spread_for_test());
    }

    #[test]
    fn a_higher_land_fraction_lowers_the_shore() {
        // More land means sea level sits at a lower quantile of the same field.
        let less = Continentality::new(12345, EARTH_RADIUS_M, 0.2);
        let more = Continentality::new(12345, EARTH_RADIUS_M, 0.5);
        assert!(more.shore_for_test() < less.shore_for_test());
    }

    #[test]
    fn the_spread_is_never_zero() {
        let c = Continentality::new(12345, EARTH_RADIUS_M, LAND_FRACTION);
        assert!(c.spread_for_test() != 0.0);
    }

    #[test]
    fn above_shore_is_zero_at_the_calibrated_shoreline() {
        let c = Continentality::new(12345, EARTH_RADIUS_M, LAND_FRACTION);
        // A point whose raw field equals the shore has above_shore exactly zero.
        let p = SpherePoint::from_latlon(17.0, 43.0);
        let expected = c.at(&p) - c.shore_for_test();
        assert_eq!(c.above_shore(&p).to_bits(), expected.to_bits());
    }

    #[test]
    fn elevation_is_bounded_by_the_continent_and_the_abyss() {
        let c = Continentality::new(12345, EARTH_RADIUS_M, LAND_FRACTION);
        for lat in (-80..81).step_by(10) {
            for lon in (-180..181).step_by(20) {
                let e = c.base_elevation(&SpherePoint::from_latlon(lat as f64, lon as f64));
                assert!(e <= CONTINENT_M, "{} at {},{}", e, lat, lon);
                assert!(e >= ABYSS_M, "{} at {},{}", e, lat, lon);
            }
        }
    }

    #[test]
    fn the_seaward_side_is_linear() {
        // Twice as far below the shore is twice as deep, until the abyss clamps it.
        let c = Continentality::new(12345, EARTH_RADIUS_M, LAND_FRACTION);
        let spread = c.spread_for_test();
        let quarter = c.elevation_from_above(-0.25 * spread / spread);
        let half = c.elevation_from_above(-0.5 * spread / spread);
        assert!((half - 2.0 * quarter).abs() < 1e-9, "{} vs {}", half, quarter);
    }
}
