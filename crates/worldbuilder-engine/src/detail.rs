//! Texture, and only texture.
//!
//! Ported from `worldbuilder/terrain/detail.py`. Detail roughens ground that structure has
//! already decided; it does not decide anything itself. This module carries the module
//! constants, the `smooth` helper, and the band table that plans the octaves — the noise
//! sampling and evaluation come in a later task.

use crate::noise::Noise;
use crate::sphere::SpherePoint;

/// The finest ground truth this generator has. Physics sees detail down to here and no
/// further; there is no finer octave to add without changing what canonical means.
pub const CANONICAL_WAVELENGTH_M: f64 = 250.0;

/// The coarsest detail band. Above this, structure has the say.
pub const COARSEST_WAVELENGTH_M: f64 = 20_000.0;

/// How many multiples of the sample spacing an octave's wavelength must be before it is
/// worth drawing, and where it has faded out entirely.
///
/// Nyquist puts the floor at two, but barely representable is not usefully representable -
/// an octave at twice the sample spacing is four points a cycle, which reads as noise
/// rather than as landform and aliases while doing it. It fades between two and four.
pub const BARELY_M: f64 = 2.0;
pub const CLEARLY_M: f64 = 4.0;

/// How rough the ground is, in metres, in each setting. Every one of these is far below
/// the structural relief it decorates: a shelf falls a hundred and fifty metres over
/// eighty kilometres, so fifteen metres of roughness on it is texture and not topography.
pub const ABYSSAL_M: f64 = 55.0;
pub const SHELF_M: f64 = 15.0;
pub const COAST_M: f64 = 35.0;
pub const INTERIOR_M: f64 = 80.0;
pub const MOUNTAIN_M: f64 = 150.0;

/// `max(0.0, min(1.0, fraction))` then the smoothstep `x * x * (3.0 - 2.0 * x)`, in the
/// Python's operand order.
pub fn smooth(fraction: f64) -> f64 {
    let upper = if fraction < 1.0 { fraction } else { 1.0 };
    let clamped = if upper > 0.0 { upper } else { 0.0 };
    clamped * clamped * (3.0 - 2.0 * clamped)
}

/// One octave: a wavelength in metres, the frequency it maps to in noise space, and the
/// share of total amplitude it carries once normalised.
#[derive(Debug, Clone, Copy)]
pub struct Band {
    pub wavelength_m: f64,
    pub frequency: f64,
    pub share: f64,
}

/// Roughness, scaled to what is being roughened and to what can be seen.
pub struct Detail {
    #[allow(dead_code)]
    radius_m: f64,
    #[allow(dead_code)]
    noise: Noise,
    bands: Vec<Band>,
}

impl Detail {
    pub fn new(world_seed: u64, radius_m: f64) -> Self {
        let noise = Noise::new(world_seed, 0x5EABED);
        let bands = Self::plan(radius_m);
        Self { radius_m, noise, bands }
    }

    pub fn bands(&self) -> &[Band] {
        &self.bands
    }

    /// The octaves, as wavelengths in metres with the share of amplitude each carries.
    ///
    /// Worked out once. Each octave is half the wavelength and half the amplitude of the
    /// one before, and the shares are normalised so that the total amplitude is what the
    /// caller asked for however many bands there happen to be - otherwise adding an octave
    /// would quietly make every world rougher.
    fn plan(radius_m: f64) -> Vec<Band> {
        let mut raw: Vec<(f64, f64, f64)> = Vec::new();
        let mut wavelength = COARSEST_WAVELENGTH_M;
        let mut share = 1.0;
        while wavelength >= CANONICAL_WAVELENGTH_M {
            // Wavelength in metres to cycles per unit of noise space on the unit sphere.
            // Transcribed as the Python's four operations, in order -- not simplified to
            // radius_m / wavelength. The two forms agree at Earth's radius for every
            // configured wavelength, but diverge at other radii, and radius_m is a
            // constructor parameter here.
            let frequency = 2.0 * std::f64::consts::PI * radius_m / wavelength
                / (2.0 * std::f64::consts::PI);
            raw.push((wavelength, frequency, share));
            wavelength *= 0.5;
            share *= 0.5;
        }
        let sum: f64 = raw.iter().map(|(_, _, s)| *s).sum();
        // `sum(...) or 1.0` in Python: 0.0 and -0.0 are falsy, NaN is truthy. `== 0.0`
        // matches both (since -0.0 == 0.0) and lets NaN pass through unchanged.
        let total = if sum == 0.0 { 1.0 } else { sum };
        raw.into_iter()
            // Shares are normalised so the total amplitude is what the caller asked for
            // however many bands there happen to be -- otherwise adding an octave would
            // quietly make every world rougher.
            .map(|(w, f, s)| Band { wavelength_m: w, frequency: f, share: s / total })
            .collect()
    }

    /// How rough the ground should be here.
    ///
    /// `point` is accepted but not used in the Python either -- kept here for signature
    /// fidelity with the reference and so the binding in a later task doesn't need to
    /// special-case this method.
    ///
    /// Blended from smooth weights rather than chosen from a category, for the same
    /// reason as everything else in this engine. And the trench term is deliberate: a
    /// deep, deliberate piece of structure stays legible instead of being buried under
    /// texture that has no idea it is there.
    #[allow(unused_variables)]
    pub fn amplitude_m(
        &self,
        point: &SpherePoint,
        elevation_m: f64,
        shelf_weight: f64,
        tectonic_m: f64,
    ) -> f64 {
        // How high, from deep water through the shelf to the tops.
        let deep = 1.0 - smooth((elevation_m + 3000.0) / 2500.0);
        let high = smooth((elevation_m - 200.0) / 900.0);
        let near_shore = smooth(1.0 - elevation_m.abs() / 350.0);

        let mut rough = deep * ABYSSAL_M
            + (1.0 - deep) * (1.0 - high) * INTERIOR_M
            + high * MOUNTAIN_M;
        rough = rough * (1.0 - near_shore) + COAST_M * near_shore;
        rough = rough * (1.0 - shelf_weight) + SHELF_M * shelf_weight;

        // Deliberate deep structure keeps its shape.
        let quieted = 1.0 - 0.7 * smooth(tectonic_m.abs() / 1200.0);
        rough * quieted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sphere::EARTH_RADIUS_M;

    #[test]
    fn the_band_table_is_seven_octaves_from_twenty_kilometres_down() {
        // Measured from the Python, not computed here: the loop halves the wavelength
        // from COARSEST_WAVELENGTH_M while it stays at or above CANONICAL_WAVELENGTH_M,
        // and 312.5 is the last that qualifies -- 156.25 is below 250.
        let d = Detail::new(20260831, EARTH_RADIUS_M);
        let want: [(f64, f64); 7] = [
            (20000.0, 318.55),
            (10000.0, 637.1),
            (5000.0, 1274.2),
            (2500.0, 2548.4),
            (1250.0, 5096.8),
            (625.0, 10193.6),
            (312.5, 20387.2),
        ];
        assert_eq!(d.bands().len(), 7);
        for (i, (w, f)) in want.iter().enumerate() {
            assert_eq!(d.bands()[i].wavelength_m.to_bits(), w.to_bits(), "band {i} wavelength");
            assert_eq!(d.bands()[i].frequency.to_bits(), f.to_bits(), "band {i} frequency");
        }
    }

    #[test]
    fn the_shares_are_normalised_to_exactly_one() {
        // "otherwise adding an octave would quietly make every world rougher". The raw
        // shares halve from 1.0, so they sum to 2 - 0.5^6; dividing through gives 1.0,
        // and it lands exactly on 1.0 for this table -- measured, not assumed.
        let d = Detail::new(20260831, EARTH_RADIUS_M);
        let total: f64 = d.bands().iter().map(|b| b.share).sum();
        assert_eq!(total, 1.0, "shares must normalise to exactly one, got {total}");
    }

    #[test]
    fn smooth_saturates_at_both_ends() {
        assert_eq!(smooth(-10.0), 0.0);
        assert_eq!(smooth(10.0), 1.0);
        assert_eq!(smooth(0.5), 0.5);
    }

    // `point` is unused by the formula (see the doc comment on `amplitude_m`), so any
    // point will do for these tests.
    fn anywhere() -> SpherePoint {
        SpherePoint::from_latlon(0.0, 0.0)
    }

    #[test]
    fn deep_abyssal_ground_gives_exactly_abyssal_m() {
        // elevation_m = -6000.0, shelf_weight = 0.0, tectonic_m = 0.0.
        //   deep:  (elevation + 3000) / 2500 = -3000 / 2500 = -1.2, clamped to 0.0,
        //          smooth(0.0) = 0.0, so deep = 1.0 - 0.0 = 1.0 exactly.
        //   high:  (elevation - 200) / 900 = -6200 / 900, negative, clamped to 0.0,
        //          smooth(0.0) = 0.0.
        //   near_shore: 1.0 - abs(elevation) / 350 = 1.0 - 6000/350, deeply negative,
        //          clamped to 0.0, smooth(0.0) = 0.0.
        //   rough = 1.0*ABYSSAL_M + (1.0-1.0)*(1.0-0.0)*INTERIOR_M + 0.0*MOUNTAIN_M
        //         = ABYSSAL_M + 0.0 + 0.0 = ABYSSAL_M exactly.
        //   rough = rough*(1.0-0.0) + COAST_M*0.0 = rough (near_shore term drops out).
        //   rough = rough*(1.0-0.0) + SHELF_M*0.0 = rough (shelf_weight is 0.0).
        //   quieted: abs(0.0)/1200 = 0.0, smooth(0.0) = 0.0, quieted = 1.0 - 0.0 = 1.0.
        //   result = ABYSSAL_M * 1.0 = ABYSSAL_M exactly.
        let d = Detail::new(20260831, EARTH_RADIUS_M);
        let got = d.amplitude_m(&anywhere(), -6000.0, 0.0, 0.0);
        assert_eq!(got, ABYSSAL_M);
    }

    #[test]
    fn a_mountain_gives_exactly_mountain_m() {
        // elevation_m = 2000.0, shelf_weight = 0.0, tectonic_m = 0.0.
        //   deep:  (2000+3000)/2500 = 2.0, clamped to 1.0, smooth(1.0) = 1.0,
        //          so deep = 1.0 - 1.0 = 0.0 exactly.
        //   high:  (2000-200)/900 = 1800/900 = 2.0, clamped to 1.0, smooth(1.0) = 1.0
        //          exactly.
        //   near_shore: 1.0 - abs(2000)/350 = 1.0 - 5.714... , negative, clamped to 0.0,
        //          smooth(0.0) = 0.0.
        //   rough = 0.0*ABYSSAL_M + (1.0-0.0)*(1.0-1.0)*INTERIOR_M + 1.0*MOUNTAIN_M
        //         = 0.0 + 0.0 + MOUNTAIN_M = MOUNTAIN_M exactly.
        //   rough = rough*(1.0-0.0) + COAST_M*0.0 = rough.
        //   rough = rough*(1.0-0.0) + SHELF_M*0.0 = rough (shelf_weight is 0.0).
        //   quieted = 1.0 - 0.7*smooth(0.0) = 1.0.
        //   result = MOUNTAIN_M * 1.0 = MOUNTAIN_M exactly.
        let d = Detail::new(20260831, EARTH_RADIUS_M);
        let got = d.amplitude_m(&anywhere(), 2000.0, 0.0, 0.0);
        assert_eq!(got, MOUNTAIN_M);
    }

    #[test]
    fn full_shelf_weight_pulls_the_answer_to_exactly_shelf_m() {
        // Same elevation as the deep-abyssal case (-6000.0), so by that derivation
        // `rough` reaches SHELF_M's blend step as ABYSSAL_M exactly, i.e. 55.0.
        // shelf_weight = 1.0:
        //   rough = rough*(1.0-1.0) + SHELF_M*1.0 = 0.0 + SHELF_M = SHELF_M exactly,
        //   independent of what `rough` was going in.
        //   tectonic_m = 0.0, so quieted = 1.0 as before.
        //   result = SHELF_M * 1.0 = SHELF_M exactly.
        let d = Detail::new(20260831, EARTH_RADIUS_M);
        let got = d.amplitude_m(&anywhere(), -6000.0, 1.0, 0.0);
        assert_eq!(got, SHELF_M);
    }

    #[test]
    fn a_large_tectonic_m_quiets_the_result_to_thirty_percent() {
        // Same elevation/shelf_weight as the deep-abyssal case, so `rough` reaches the
        // quieting step as ABYSSAL_M exactly, i.e. 55.0.
        // tectonic_m = 5000.0: abs(5000)/1200 = 4.1666..., clamped to 1.0,
        //   smooth(1.0) = 1.0 exactly, so quieted = 1.0 - 0.7*1.0 = 1.0 - 0.7.
        //   In f64, 1.0 - 0.7 does not land on 0.3 -- it rounds to 0.30000000000000004
        //   (0x1.3333333333334p-2). result = 55.0 * (1.0 - 0.7), computed here the same
        //   way the formula computes it, not read back from the implementation.
        let d = Detail::new(20260831, EARTH_RADIUS_M);
        let got = d.amplitude_m(&anywhere(), -6000.0, 0.0, 5000.0);
        let expected: f64 = ABYSSAL_M * (1.0 - 0.7);
        assert_eq!(got, expected);
    }
}
