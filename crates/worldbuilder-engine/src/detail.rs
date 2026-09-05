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

    /// The roughness itself.
    ///
    /// `resolution_m` is `None` for canonical ground truth -- every configured octave,
    /// down to `CANONICAL_WAVELENGTH_M`. Python's `if resolution_m:` is false for both
    /// `None` and `0.0` (and `-0.0`, also falsy), so a caller passing zero gets every
    /// octave at full strength, exactly as if nothing had been passed -- it must not
    /// divide by zero. The `match Some(r) if r != 0.0 => Some(r), _ => None` below makes
    /// `Some(0.0)` and `Some(-0.0)` take the same canonical path as `None`.
    ///
    /// **Octaves fade rather than switch off.** Dropping one the instant it becomes
    /// unrepresentable would be a cliff in *resolution* rather than in position -- the
    /// ground would jump as somebody zoomed, which is the same bug M1.4 kept producing,
    /// in a different axis. Each octave dims between twice and four times the sample
    /// spacing and is gone by the far end.
    ///
    /// Sub-sample frequencies are not merely wasted work. They alias: an octave shorter
    /// than the spacing lands somewhere different in every grid, so a chart would
    /// shimmer as a ship moved rather than showing generalised ground.
    pub fn offset_m(&self, point: &SpherePoint, amplitude_m: f64, resolution_m: Option<f64>) -> f64 {
        if amplitude_m <= 0.0 {
            return 0.0;
        }

        // Python's `if resolution_m:` is false for None, 0.0 and -0.0. `r != 0.0`
        // matches both zeros (since -0.0 == 0.0 in IEEE 754), collapsing them to `None`
        // here.
        //
        // A NaN resolution is different: `NaN != 0.0` is true, so `Some(NaN)` takes the
        // *resolution* branch below, not this canonical one -- Python's `if resolution_m`
        // is also true for NaN (NaN is truthy), so both languages agree on which branch
        // runs. They still produce the same total, but not because NaN reaches this arm:
        // inside the loop, `wavelength / NaN` is NaN, and `smooth(NaN)` clamps to `1.0`
        // in both languages (the comparisons `fraction < 1.0` / `min(1.0, fraction)` and
        // `upper > 0.0` / `max(0.0, ...)` are false against NaN, so the upper-clamp value
        // wins on both sides), giving `visible = 1.0` for every band -- identical to the
        // canonical arm's literal `1.0`. The equivalence comes from `smooth`'s clamp
        // order, not from `Some(NaN)` reaching the `None` path.
        let resolution = match resolution_m {
            Some(r) if r != 0.0 => Some(r),
            _ => None,
        };

        let vector = point.vector;
        let mut total = 0.0;
        for band in &self.bands {
            let visible = match resolution {
                Some(r) => {
                    let v = smooth((band.wavelength_m / r - BARELY_M) / (CLEARLY_M - BARELY_M));
                    if v <= 0.0 {
                        // Everything finer is finer still, so nothing below can be
                        // visible.
                        break;
                    }
                    v
                }
                None => 1.0,
            };
            total += (self.noise.at(
                vector.x * band.frequency,
                vector.y * band.frequency,
                vector.z * band.frequency,
            ) - 0.5)
                * 2.0
                * band.share
                * visible;
        }
        total * amplitude_m
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

    #[test]
    fn a_resolution_of_zero_behaves_exactly_like_canonical() {
        // Python's `if resolution_m:` is false for BOTH None and 0.0, so a caller
        // passing zero gets every octave, not a division by zero. A Rust Option port
        // diverges here unless Some(0.0) is special-cased -- this is the test that
        // catches it, and it must be bit-exact rather than approximate.
        let d = Detail::new(20260831, EARTH_RADIUS_M);
        let p = SpherePoint::from_latlon(17.0, 43.0);
        let canonical = d.offset_m(&p, 100.0, None);
        let zero = d.offset_m(&p, 100.0, Some(0.0));
        assert_eq!(zero.to_bits(), canonical.to_bits(), "Some(0.0) must equal None");
    }

    #[test]
    fn a_resolution_of_negative_zero_behaves_exactly_like_canonical() {
        // -0.0 is falsy in Python too, so Some(-0.0) must take the canonical path
        // exactly as Some(0.0) and None do.
        let d = Detail::new(20260831, EARTH_RADIUS_M);
        let p = SpherePoint::from_latlon(17.0, 43.0);
        let canonical = d.offset_m(&p, 100.0, None);
        let neg_zero = d.offset_m(&p, 100.0, Some(-0.0));
        assert_eq!(neg_zero.to_bits(), canonical.to_bits(), "Some(-0.0) must equal None");
    }

    #[test]
    fn a_nan_resolution_behaves_exactly_like_canonical() {
        // Some(NaN) takes the *resolution* branch (NaN != 0.0), not the canonical one --
        // unlike the zero cases above. But every band's `visible` still comes out to
        // exactly 1.0: `wavelength / NaN` is NaN, and `smooth(NaN)` clamps to 1.0 in both
        // languages because the comparisons that drive the clamp are false against NaN.
        // So the result matches canonical bit-for-bit, for a different reason than the
        // zero cases -- guarded here rather than merely asserted in a comment.
        let d = Detail::new(20260831, EARTH_RADIUS_M);
        let p = SpherePoint::from_latlon(17.0, 43.0);
        let canonical = d.offset_m(&p, 100.0, None);
        let nan_res = d.offset_m(&p, 100.0, Some(f64::NAN));
        assert_eq!(nan_res.to_bits(), canonical.to_bits(), "Some(f64::NAN) must equal None");
    }

    #[test]
    fn zero_amplitude_returns_exactly_zero() {
        let d = Detail::new(20260831, EARTH_RADIUS_M);
        let p = SpherePoint::from_latlon(17.0, 43.0);
        assert_eq!(d.offset_m(&p, 0.0, None), 0.0);
        assert_eq!(d.offset_m(&p, -1.0, None), 0.0);
    }

    #[test]
    fn a_coarse_resolution_drops_the_fine_octaves() {
        // At a sample spacing of 5 km, an octave of 312.5 m is far below Nyquist and
        // must contribute nothing, so the coarse answer differs from the canonical one.
        let d = Detail::new(20260831, EARTH_RADIUS_M);
        let p = SpherePoint::from_latlon(17.0, 43.0);
        let canonical = d.offset_m(&p, 100.0, None);
        let coarse = d.offset_m(&p, 100.0, Some(5000.0));
        assert!(canonical != coarse, "a coarse resolution must drop fine octaves");
    }

    #[test]
    fn the_fade_is_gradual_rather_than_a_step() {
        // The docstring's claim is that octaves fade rather than switch off: "a cliff in
        // resolution rather than in position - the ground would jump as somebody
        // zoomed". A port that dropped an octave abruptly at its Nyquist-ish threshold
        // would pass every test above (they only check that *some* difference exists)
        // but fail this one.
        //
        // The coarsest band is 20000.0 m. It fades as `wavelength / resolution_m` moves
        // from BARELY_M (2.0) to CLEARLY_M (4.0), i.e. resolution_m from
        // 20000/4 = 5000.0 up to 20000/2 = 10000.0. Sampling resolution_m across
        // [4000.0, 11000.0] in fixed steps crosses that whole fade window on both sides,
        // so the range is guaranteed not to be vacuous (unlike a range that stayed
        // entirely above or below the window, which would report a max step of 0.0 and
        // pass without testing anything -- that vacuity has hit this port three times).
        //
        // Deriving the bound, from the actual shape of `visible`, not a round number:
        //
        // `visible = smooth(x)` where `x = (wavelength / r - BARELY_M) / (CLEARLY_M -
        // BARELY_M)`, so `dx/dr = -wavelength / (2 * r^2)` (the 2 is `CLEARLY_M -
        // BARELY_M`), and `smooth`'s derivative peaks at 1.5 (at x = 0.5). Over one
        // sample step `d_res` the largest possible change in `visible` is therefore
        // `1.5 * wavelength * d_res / (2 * r_min^2)`, evaluated at the smallest `r` in
        // the sampled range that still lies in the fade window -- the slope is steepest
        // there. For the coarsest band (wavelength 20000.0) with d_res = 100.0 and
        // r_min = 5000.0 (the low edge of its fade window, also the low edge of the
        // sampled range):
        //   1.5 * 20000.0 * 100.0 / (2 * 5000.0^2) = 1.5 * 2_000_000.0 / 50_000_000.0
        //     = 0.06
        // A single band's contribution to `total` is `noise_factor * share * visible`
        // with `noise_factor` (`(noise - 0.5) * 2.0`) in [-1, 1], so one step can move
        // that band's term by at most `0.06 * share_of_coarsest_band`, times
        // `amplitude_m` once the final multiply is applied: a genuine fade cannot move
        // the total by more than `0.06 * share_of_coarsest_band * amplitude_m` per
        // sample -- call this bound `gradual_ceiling`, about 3.02 for this table
        // (share_of_coarsest_band ~= 0.504, amplitude_m = 100.0). Measured empirically
        // against the real implementation below, the actual max step is ~0.96, comfortably
        // under that analytic ceiling, which confirms the derivation rather than just
        // asserting it.
        //
        // An abrupt cutoff (`visible` jumping 1 -> 0 instead of fading) moves the total
        // by up to `share_of_coarsest_band * amplitude_m` in one step (`noise_factor`'s
        // full swing) -- about 50.4 here, and measured empirically at ~25.7 for the
        // actual noise value at this crossing. That is over 8x `gradual_ceiling`, so a
        // test bound placed between the two discriminates: pick a factor of 0.2 rather
        // than 0.06, i.e. `0.2 * share_of_coarsest_band * amplitude_m` (~10.1 here) --
        // above `gradual_ceiling` by more than 3x so a real fade never trips it, and
        // below the measured abrupt-step magnitude by more than 2x so a hard cutoff
        // does. This was proven, not assumed: mutating `visible`'s computation to a hard
        // step (`if frac > 0.0 { 1.0 } else { 0.0 }`) and rerunning this test failed it
        // (max_step ~25.7 against this bound of ~10.1); reverting the mutation passed it
        // again (max_step ~0.96) -- see task-3-report.md for the numbers from that run.
        let d = Detail::new(20260831, EARTH_RADIUS_M);
        let p = SpherePoint::from_latlon(17.0, 43.0);
        let share_of_coarsest_band = d.bands()[0].share;
        let bound = 0.2 * share_of_coarsest_band * 100.0; // amplitude_m = 100.0

        let mut resolution_m = 4000.0_f64;
        let mut previous = d.offset_m(&p, 100.0, Some(resolution_m));
        let mut max_step: f64 = 0.0;
        let mut low_seen = false;
        let mut high_seen = false;
        while resolution_m <= 11000.0 {
            if resolution_m < 5000.0 {
                low_seen = true;
            }
            if resolution_m > 10000.0 {
                high_seen = true;
            }
            let current = d.offset_m(&p, 100.0, Some(resolution_m));
            let step = (current - previous).abs();
            if step > max_step {
                max_step = step;
            }
            previous = current;
            resolution_m += 100.0;
        }

        // The range must actually cross the fade window (below BARELY_M's threshold and
        // above CLEARLY_M's), or this test would pass vacuously.
        assert!(low_seen, "the sampled range must dip below the fade window");
        assert!(high_seen, "the sampled range must rise above the fade window");
        assert!(max_step > 0.0, "the range must actually show the octave fading");
        assert!(
            max_step < bound,
            "a step of {max_step} between adjacent samples exceeds the derived bound \
             of {bound}, suggesting a cliff rather than a fade"
        );
    }
}
