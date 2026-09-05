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

/// The ten values that decide how rough a world is, broken out so a caller who wants a
/// different world can ask for one without touching what "canonical" means.
///
/// `Detail::new` takes `Option<ReliefParams>`, following the house pattern already on
/// `Surface::new`'s `features: Option<FeatureInput>`: `None` is the canonical path, not an
/// implicit `Default::default()` -- this codebase deliberately rejects defaults nobody
/// chose (see `stream.rs::BuildParams`). `ReliefParams::canonical()` is the only way to
/// get today's nine constants (plus the coarsest wavelength, kept as the other end of the
/// same schedule) as a value, and every field is commented with the constant or literal it
/// was measured from.
///
/// **This task adds the block. It does not change what it defaults to.** A changed
/// default here is a change to `worldbuilder/terrain/detail.py`'s constants, which the
/// conformance suite in `tests/test_conformance.py` treats as ground truth.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReliefParams {
    /// `CANONICAL_WAVELENGTH_M`: the finest octave; the loop bound.
    pub canonical_wavelength_m: f64,
    /// `COARSEST_WAVELENGTH_M`: the coarsest detail band.
    pub coarsest_wavelength_m: f64,
    /// `ABYSSAL_M`: roughness in deep water.
    pub abyssal_m: f64,
    /// `SHELF_M`: roughness over the continental shelf.
    pub shelf_m: f64,
    /// `COAST_M`: roughness right at the shoreline.
    pub coast_m: f64,
    /// `INTERIOR_M`: roughness on ordinary land.
    pub interior_m: f64,
    /// `MOUNTAIN_M`: roughness at the tops.
    pub mountain_m: f64,
    /// The `0.7` in `amplitude_m`'s quieting term: how much deliberate deep structure
    /// can suppress roughness, at its strongest.
    pub quieting_strength: f64,
    /// The `1200.0` in `amplitude_m`'s quieting term: the tectonic-offset scale over
    /// which the quieting ramps in.
    pub quieting_scale_m: f64,
    /// The `0.5` in `plan`'s `share *= 0.5`: how much amplitude each octave keeps of the
    /// one before it.
    pub octave_persistence: f64,
}

impl ReliefParams {
    /// Exactly today's nine values (plus the coarsest wavelength), each traceable to the
    /// module constant or literal it was measured from. Building a `Detail` with `None`
    /// and one with `Some(ReliefParams::canonical())` must produce bit-identical output --
    /// see `surface.rs`'s `relief_none_matches_relief_some_canonical`.
    pub fn canonical() -> Self {
        Self {
            canonical_wavelength_m: CANONICAL_WAVELENGTH_M,
            coarsest_wavelength_m: COARSEST_WAVELENGTH_M,
            abyssal_m: ABYSSAL_M,
            shelf_m: SHELF_M,
            coast_m: COAST_M,
            interior_m: INTERIOR_M,
            mountain_m: MOUNTAIN_M,
            quieting_strength: 0.7,
            quieting_scale_m: 1200.0,
            octave_persistence: 0.5,
        }
    }

    /// A named, opt-in preset -- Task 3 of the relief-amplitude slice
    /// (`.superpowers/sdd/2026-09-05-slice-relief-amplitude/progress.md`), chosen from
    /// Task 2's measured tables, not a new default. **`canonical()` above is untouched by
    /// this constructor and stays the `None` path -- Ruling 1.**
    ///
    /// Three fields move, each on stated, external ground:
    ///
    /// - **`octave_persistence: 0.65`.** Real terrain's measured Hurst exponent is
    ///   H ~= 0.6-0.71 (Gagnon, Lovejoy & Schertzer, over four DEMs). At this schedule's own
    ///   lacunarity of 2.0, `H = ln(1/persistence) / ln(2)`, so persistence 0.65 gives
    ///   H = 0.6215 -- inside that band. This is the only swept persistence value that
    ///   lands there; 0.5 (H=1.0) and 0.71/0.75 (H=0.49/0.42) both fall outside it. Task
    ///   2 also found persistence is not the scale-neutral knob an infinite-series argument
    ///   would suggest -- this schedule is a finite seven octaves -- so 0.65 is chosen for
    ///   matching real terrain's measured roughness, not for being scale-invariant.
    /// - **`quieting_strength: -0.7`.** The exact negation of `canonical()`'s `0.7`, not a
    ///   new magnitude picked from the sweep's extreme corner. Two grounds for the sign
    ///   flip: Task 2 measured that on the peak population (real uplift, large
    ///   `tectonic_m`) inverting the sign more than doubles relief (median 4.02 -> 5.19 m,
    ///   max 10.32 -> 19.67 m at today's other settings) while on generic land it is
    ///   inert -- so the flip is choosing where tectonics are already large, not
    ///   manufacturing relief from nothing. And Outerra's published terrain generator does
    ///   the same thing in spirit: its amplitude *rises* with slope and curvature instead
    ///   of being suppressed by them ("the amplitude of noise is modulated by slope --
    ///   flat areas have less noise, while the steeper get more"; curvature raises it too,
    ///   "depending also on elevation", to fix flat mountaintops). This engine keys on
    ///   tectonic magnitude rather than slope or curvature, so the match is an
    ///   approximation of a shipped mechanism, not a reproduction of it -- matching
    ///   Outerra's actual inputs would be a mechanism change, out of scope here.
    /// - **`mountain_m: MOUNTAIN_M * 4.0` (600.0).** Grounded on Hammond's landform
    ///   classification: hills are 80-160 m of local relief over a 2 km run; low mountains
    ///   start at 300 m. Combined with the two choices above, Task 2's sweep measured this
    ///   exact combination (`mountain_m` x4, `quieting_strength=-0.7`,
    ///   `octave_persistence=0.65`) giving the peak population a relief max of 124.25 m --
    ///   inside the hills band, with headroom to the 160 m ceiling and nowhere near the
    ///   300 m mountain floor. `mountain_m` x3 (450.0) also reaches the hills band (93.37 m
    ///   max) but with less of the band filled; x4 was chosen as the multiplier, of the
    ///   ones Task 2 swept, whose measured result best fills a published target band rather
    ///   than for being the largest number tried.
    ///
    /// **Ruling 6, and what this preset does not attempt:** the roughness spectrum alone
    /// cannot make mountains -- Task 2's most extreme corner (this same `mountain_m` x4,
    /// `quieting_strength=-0.7`, but `octave_persistence=0.75`, outside real terrain's
    /// Hurst range) topped out at 161 m on land and never reached Hammond's low-mountains
    /// band. Mountain height is tectonic (Ruling 4, a separate slice); this preset is the
    /// best hills the roughness spectrum can produce, not mountains it cannot.
    pub fn hills() -> Self {
        Self { mountain_m: MOUNTAIN_M * 4.0, quieting_strength: -0.7, octave_persistence: 0.65, ..Self::canonical() }
    }
}

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
    relief: ReliefParams,
}

impl Detail {
    /// `relief`: `None` for canonical -- today's nine values (plus the coarsest
    /// wavelength), byte-for-byte what `ReliefParams::canonical()` returns. `Some(params)`
    /// for a caller-chosen block. Resolved once here rather than re-checked on every call,
    /// so `amplitude_m` and `plan` never see the `Option` at all.
    pub fn new(world_seed: u64, radius_m: f64, relief: Option<ReliefParams>) -> Self {
        let relief = relief.unwrap_or_else(ReliefParams::canonical);
        let noise = Noise::new(world_seed, 0x5EABED);
        let bands = Self::plan(radius_m, &relief);
        Self { radius_m, noise, bands, relief }
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
    fn plan(radius_m: f64, relief: &ReliefParams) -> Vec<Band> {
        let mut raw: Vec<(f64, f64, f64)> = Vec::new();
        let mut wavelength = relief.coarsest_wavelength_m;
        let mut share = 1.0;
        while wavelength >= relief.canonical_wavelength_m {
            // Wavelength in metres to cycles per unit of noise space on the unit sphere.
            // Transcribed as the Python's four operations, in order -- not simplified to
            // radius_m / wavelength. The two forms agree at Earth's radius for every
            // configured wavelength, but diverge at other radii, and radius_m is a
            // constructor parameter here.
            let frequency = 2.0 * std::f64::consts::PI * radius_m / wavelength
                / (2.0 * std::f64::consts::PI);
            raw.push((wavelength, frequency, share));
            wavelength *= 0.5;
            share *= relief.octave_persistence;
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

        let mut rough = deep * self.relief.abyssal_m
            + (1.0 - deep) * (1.0 - high) * self.relief.interior_m
            + high * self.relief.mountain_m;
        rough = rough * (1.0 - near_shore) + self.relief.coast_m * near_shore;
        rough = rough * (1.0 - shelf_weight) + self.relief.shelf_m * shelf_weight;

        // Deliberate deep structure keeps its shape.
        let quieted = 1.0
            - self.relief.quieting_strength * smooth(tectonic_m.abs() / self.relief.quieting_scale_m);
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
        let d = Detail::new(20260831, EARTH_RADIUS_M, None);
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
        let d = Detail::new(20260831, EARTH_RADIUS_M, None);
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
        let d = Detail::new(20260831, EARTH_RADIUS_M, None);
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
        let d = Detail::new(20260831, EARTH_RADIUS_M, None);
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
        let d = Detail::new(20260831, EARTH_RADIUS_M, None);
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
        let d = Detail::new(20260831, EARTH_RADIUS_M, None);
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
        let d = Detail::new(20260831, EARTH_RADIUS_M, None);
        let p = SpherePoint::from_latlon(17.0, 43.0);
        let canonical = d.offset_m(&p, 100.0, None);
        let zero = d.offset_m(&p, 100.0, Some(0.0));
        assert_eq!(zero.to_bits(), canonical.to_bits(), "Some(0.0) must equal None");
    }

    #[test]
    fn a_resolution_of_negative_zero_behaves_exactly_like_canonical() {
        // -0.0 is falsy in Python too, so Some(-0.0) must take the canonical path
        // exactly as Some(0.0) and None do.
        let d = Detail::new(20260831, EARTH_RADIUS_M, None);
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
        let d = Detail::new(20260831, EARTH_RADIUS_M, None);
        let p = SpherePoint::from_latlon(17.0, 43.0);
        let canonical = d.offset_m(&p, 100.0, None);
        let nan_res = d.offset_m(&p, 100.0, Some(f64::NAN));
        assert_eq!(nan_res.to_bits(), canonical.to_bits(), "Some(f64::NAN) must equal None");
    }

    #[test]
    fn zero_amplitude_returns_exactly_zero() {
        let d = Detail::new(20260831, EARTH_RADIUS_M, None);
        let p = SpherePoint::from_latlon(17.0, 43.0);
        assert_eq!(d.offset_m(&p, 0.0, None), 0.0);
        assert_eq!(d.offset_m(&p, -1.0, None), 0.0);
    }

    #[test]
    fn a_coarse_resolution_drops_the_fine_octaves() {
        // At a sample spacing of 5 km, an octave of 312.5 m is far below Nyquist and
        // must contribute nothing, so the coarse answer differs from the canonical one.
        let d = Detail::new(20260831, EARTH_RADIUS_M, None);
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
        let d = Detail::new(20260831, EARTH_RADIUS_M, None);
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

    // ---- Task 3: the `hills()` preset -------------------------------------------------

    /// `hills()` must not be `canonical()` with the serial numbers filed off -- every
    /// field it does not deliberately move must still equal `canonical()`'s.
    #[test]
    fn hills_only_moves_the_three_named_fields() {
        let hills = ReliefParams::hills();
        let canonical = ReliefParams::canonical();
        assert_eq!(hills.canonical_wavelength_m, canonical.canonical_wavelength_m);
        assert_eq!(hills.coarsest_wavelength_m, canonical.coarsest_wavelength_m);
        assert_eq!(hills.abyssal_m, canonical.abyssal_m);
        assert_eq!(hills.shelf_m, canonical.shelf_m);
        assert_eq!(hills.coast_m, canonical.coast_m);
        assert_eq!(hills.interior_m, canonical.interior_m);
        assert_eq!(hills.quieting_scale_m, canonical.quieting_scale_m);
        assert_eq!(hills.mountain_m, canonical.mountain_m * 4.0);
        assert_eq!(hills.quieting_strength, -canonical.quieting_strength);
        assert_eq!(hills.octave_persistence, 0.65);
    }

    /// Ruling 1's central claim, restated at the point this task touches: adding
    /// `hills()` beside `canonical()` must not perturb what `None` means.
    /// `surface.rs::relief_none_matches_relief_some_canonical_bit_for_bit` already checks
    /// this at the full `Surface::elevation_m` level over 5,402 comparisons; this is the
    /// same claim checked directly at `Detail::offset_m`, so a reviewer of this file does
    /// not have to cross to `surface.rs` to see it hold.
    #[test]
    fn canonical_is_still_bit_identical_to_none_after_adding_hills() {
        let with_none = Detail::new(20260905, EARTH_RADIUS_M, None);
        let with_canonical = Detail::new(20260905, EARTH_RADIUS_M, Some(ReliefParams::canonical()));
        let p = SpherePoint::from_latlon(12.0, 34.0);
        for resolution in [None, Some(5000.0)] {
            let a = with_none.offset_m(&p, 100.0, resolution);
            let b = with_canonical.offset_m(&p, 100.0, resolution);
            assert_eq!(
                a.to_bits(),
                b.to_bits(),
                "None and Some(canonical()) must stay bit-identical at resolution {resolution:?}"
            );
        }
    }

    /// The mechanism `hills()` is FOR, isolated from noise sampling: at a site with large
    /// deliberate tectonic offset (what Task 2 called the "peak population"),
    /// `amplitude_m` must come out materially larger under `hills()` than under
    /// `canonical()`. `tectonic_m = 1200.0` fully saturates the quieting term's own scale
    /// (`quieting_scale_m`) on both sides, so this isolates the sign flip and the
    /// `mountain_m` multiplier from any partial-saturation effect.
    ///
    /// **Verified by mutation, not just written and trusted**: reverting `hills()` to
    /// `canonical()`'s `quieting_strength` (0.7) while leaving `mountain_m` at x4 still
    /// grows amplitude (mountain_m alone does that), so a weak version of this assertion
    /// could pass under a mutation that silently drops the sign flip. Run directly against
    /// that mutation with `hills_only_moves_the_three_named_fields` disabled (so it could
    /// not shadow this one by failing first): `mountain_m` alone gives an EXACT ratio of
    /// `4.0 * (1.0 - 0.7) / (1.0 - 0.7) = 4.0` at this tectonic magnitude -- not the
    /// "~2x" a rougher estimate might guess, measured directly instead. The ratio bound
    /// below (>= 10.0) is calibrated to that measurement: a mutation dropping the sign
    /// flip but keeping the x4 multiplier gives exactly 4.0, comfortably under 10.0, while
    /// `hills()` unmutated gives 22.67, comfortably over it. This is the sibling this
    /// test's "materially" claim could otherwise be shadowed by, and the exact-ratio
    /// assertion just above it would also have caught this same mutation on its own.
    #[test]
    fn hills_gives_materially_more_amplitude_than_canonical_on_tectonic_ground() {
        let peak_point = SpherePoint::from_latlon(0.0, 0.0);
        let peak_elevation_m = 2000.0; // saturates `high` in amplitude_m, as in a_mountain_gives_exactly_mountain_m
        let saturating_tectonic_m = 1200.0; // == canonical()'s quieting_scale_m: smooth(1.0) = 1.0 on both sides

        let canonical = Detail::new(20260905, EARTH_RADIUS_M, Some(ReliefParams::canonical()));
        let hills = Detail::new(20260905, EARTH_RADIUS_M, Some(ReliefParams::hills()));

        let canonical_amplitude =
            canonical.amplitude_m(&peak_point, peak_elevation_m, 0.0, saturating_tectonic_m);
        let hills_amplitude = hills.amplitude_m(&peak_point, peak_elevation_m, 0.0, saturating_tectonic_m);

        // Canonical: MOUNTAIN_M * (1.0 - 0.7) = MOUNTAIN_M * 0.3, fully quieted.
        // hills(): (MOUNTAIN_M * 4.0) * (1.0 - (-0.7)) = MOUNTAIN_M * 4.0 * 1.7, fully
        // amplified instead of quieted. The ratio is exactly (4.0 * 1.7) / 0.3 =
        // 22.666..., a large, exactly-derivable multiple -- not a vague "bigger".
        let want_ratio = (4.0 * 1.7) / 0.3;
        let got_ratio = hills_amplitude / canonical_amplitude;
        assert!(
            (got_ratio - want_ratio).abs() < 1e-9,
            "expected hills()/canonical() amplitude ratio {want_ratio}, got {got_ratio} \
             ({hills_amplitude} / {canonical_amplitude})"
        );
        assert!(
            got_ratio >= 10.0,
            "hills() must give materially (>= 10x) more amplitude than canonical() on \
             saturated tectonic ground, got {got_ratio}x -- a mutation dropping the sign \
             flip but keeping the x4 mountain_m multiplier alone gives exactly 4.0x, which \
             this bound must reject"
        );
    }

    /// The same claim, on the actual peak population Task 2 measured against, over the
    /// full `Surface` pipeline rather than `amplitude_m` in isolation -- noise sampling,
    /// octave planning and all. A modest 19x36 grid (10 deg spacing, 684 candidates,
    /// poles excluded) stands in for Task 2's own denser peak search: cheap enough for a
    /// unit test, ranked by `structural_m` against a canonical reference so the site
    /// chosen does not depend on `ReliefParams` at all, the same guarantee Task 2's own
    /// survey relied on.
    #[test]
    fn hills_gives_materially_more_relief_than_canonical_on_a_real_peak() {
        use crate::continentality::LAND_FRACTION;
        use crate::generation::DEFAULT_PLATE_COUNT;
        use crate::surface::Surface;
        use crate::tangent::TangentFrame;

        const SEED: i64 = 20_260_904; // task-2-report.md's own seed
        const LAT_STEPS: i32 = 17; // -80..=80 at 10 deg, poles excluded (TangentFrame degenerates there)
        const LON_STEPS: i32 = 36; // -180..170 at 10 deg

        let reference = Surface::new(SEED, EARTH_RADIUS_M, DEFAULT_PLATE_COUNT, LAND_FRACTION, None, None);

        let mut best: Option<(f64, f64, f64)> = None; // (structural_m, lat, lon)
        for i in 0..LAT_STEPS {
            let lat = -80.0 + (i as f64) * 10.0; // cast-ok: grid index widened to a coordinate
            for j in 0..LON_STEPS {
                let lon = -180.0 + (j as f64) * 10.0; // cast-ok: grid index widened to a coordinate
                let point = SpherePoint::from_latlon(lat, lon);
                let structural_m = reference.structural_m(&point);
                let replace = match best {
                    None => true,
                    Some((best_structural_m, _, _)) => structural_m > best_structural_m,
                };
                if replace {
                    best = Some((structural_m, lat, lon));
                }
            }
        }
        let (peak_structural_m, peak_lat, peak_lon) = best.expect("684-candidate grid is non-empty");
        // Sanity: this must actually be a peak, or the test proves nothing about the
        // population it claims to stand in for.
        assert!(
            peak_structural_m > 300.0,
            "the highest site on this grid ({peak_structural_m} m) is not high ground -- \
             the grid found no real peak to test against"
        );

        let world_canonical = Surface::new(
            SEED,
            EARTH_RADIUS_M,
            DEFAULT_PLATE_COUNT,
            LAND_FRACTION,
            None,
            Some(ReliefParams::canonical()),
        );
        let world_hills = Surface::new(
            SEED,
            EARTH_RADIUS_M,
            DEFAULT_PLATE_COUNT,
            LAND_FRACTION,
            None,
            Some(ReliefParams::hills()),
        );

        let relief_of = |surface: &Surface| -> f64 {
            let frame = TangentFrame::at_latlon(peak_lat, peak_lon, EARTH_RADIUS_M);
            let mut min_elevation_m = f64::INFINITY;
            let mut max_elevation_m = f64::NEG_INFINITY;
            let mut x = -1000.0_f64;
            while x <= 1000.0 {
                let p = frame.local_to_sphere(x, 0.0);
                let elevation_m = surface.elevation_m(&p, None);
                if elevation_m < min_elevation_m {
                    min_elevation_m = elevation_m;
                }
                if elevation_m > max_elevation_m {
                    max_elevation_m = elevation_m;
                }
                x += 50.0;
            }
            max_elevation_m - min_elevation_m
        };

        let canonical_relief_m = relief_of(&world_canonical);
        let hills_relief_m = relief_of(&world_hills);

        assert!(
            hills_relief_m > canonical_relief_m * 2.0,
            "hills() must give materially (> 2x) more 2 km transect relief than \
             canonical() at a real peak (lat {peak_lat}, lon {peak_lon}, structural \
             {peak_structural_m} m): canonical {canonical_relief_m} m, hills \
             {hills_relief_m} m"
        );
    }
}
