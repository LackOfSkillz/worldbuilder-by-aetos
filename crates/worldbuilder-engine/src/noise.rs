//! Deterministic value noise, sampled in three dimensions on the sphere.
//!
//! Ported from `worldbuilder/terrain/noise.py`. Three dimensions rather than two because a
//! two-dimensional field cannot be wrapped onto a sphere without a seam down one meridian
//! and a pinch at each pole; sampling a volume at the point's own position has neither.
//!
//! Every lattice value is an integer hash of its own coordinates and the seed, so it
//! depends on nothing but that point — there is no generator whose position could matter
//! and no order that could change an answer.
//!
//! **The Python memoises the eight corners of each cell; this does not.** That cache exists
//! because a Python-level call costs more than the arithmetic it avoids — the Python's own
//! comment records 2.9 million calls in one chart redraw, where call overhead was twice the
//! cost of the dictionary lookup. Rust has no such overhead. Dropping the cache returns
//! exactly the same values (it memoises a pure function of three integers and a seed), and
//! it buys a `Noise` that is immutable, `Sync`, and free of interior mutability — which the
//! WebAssembly build and any future parallel bake both want.

use crate::detmath as m;

const SCALE: f64 = 18_446_744_073_709_551_616.0; // 2^64, exactly representable

/// The largest lattice coordinate `Noise::at` can name, in either direction.
///
/// `i64::MAX` is about 9.223e18, so 9e18 is comfortably inside it with room for the `+ 1`
/// that addresses the far corner of the cell. The bound is drawn at a round number rather
/// than at the exact overflow boundary for the same reason `wasm.rs` draws `land_fraction`
/// at its documented domain rather than at the measured panic: the exact boundary is an
/// accident of the integer width and would move if that did.
///
/// It is astronomically outside any admissible sample, so this guard REFUSES NOTHING THE
/// C ABI ACCEPTS. The canonical field is a unit-sphere component times
/// `BASE_FREQUENCY * 2^3`, so |coordinate| <= 10, and `WB_MAX_COAST_FINEST_FREQUENCY`
/// caps the most extreme admissible record at 1e6 — twelve orders of magnitude below this
/// line. What lies above it is only what that ceiling already refuses (the ~1.15e24 the
/// frequency/lacunarity/octaves cross product asks for) and what nothing validates at all
/// (an infinity handed straight to `bindings::continentality_at`).
const LATTICE_LIMIT: f64 = 9.0e18;

#[derive(Debug, Clone, Copy)]
pub struct Noise {
    seed: u64,
}

impl Noise {
    /// Salted so that two fields on the same world — continentality here, roughness later —
    /// are independent rather than the same shape at different amplitudes.
    ///
    /// The Python leaves this product unmasked and masks inside the lattice hash instead.
    /// Wrapping here is equivalent: multiplication and XOR both commute with truncation
    /// mod 2^64, so masking once at the end is the same as masking throughout. Verified
    /// against the Python across seeds including 2^63.
    pub fn new(seed: u64, salt: u64) -> Self {
        Self {
            seed: seed
                .wrapping_mul(0x100000001B3)
                ^ salt.wrapping_mul(0x9E3779B97F4A7C15),
        }
    }

    /// Exposes the mixed seed for the pinning test below. `#[cfg(test)]` only: this is not
    /// part of the crate's public surface, just a window for a test that would otherwise
    /// have no way to observe a private field.
    #[cfg(test)]
    fn seed_for_test(&self) -> u64 {
        self.seed
    }

    /// An integer avalanche rather than a cryptographic digest. A real digest would be just
    /// as deterministic and about thirty times slower, and this is called eight times per
    /// octave per sample.
    ///
    /// Exposed to the crate as [`Noise::lattice_at`] for the gully kernel's pivot jitter,
    /// which wants one deterministic number per lattice node and not an interpolated field.
    fn lattice(&self, ix: i64, iy: i64, iz: i64) -> f64 {
        let h = (ix as u64).wrapping_mul(0x9E3779B97F4A7C15)  // cast-ok: convert signed lattice coordinate to unsigned for hash
            ^ (iy as u64).wrapping_mul(0xC2B2AE3D27D4EB4F)   // cast-ok: convert signed lattice coordinate to unsigned for hash
            ^ (iz as u64).wrapping_mul(0x165667B19E3779F9);  // cast-ok: convert signed lattice coordinate to unsigned for hash
        let mut h = h ^ self.seed.wrapping_mul(0x27D4EB2F165667C5);
        h ^= h >> 33;
        h = h.wrapping_mul(0xFF51AFD7ED558CCD);
        h ^= h >> 33;
        h = h.wrapping_mul(0xC4CEB9FE1A85EC53);
        h ^= h >> 33;
        h as f64 / SCALE
    }

    /// One lattice node's hash, in `[0, 1)`, without any interpolation.
    ///
    /// `Noise::at` blends eight of these; the gully kernel wants the raw value at a named
    /// node, because its pivots ARE lattice nodes and what it needs from each is a fixed
    /// per-node jitter rather than a field. Same function, same seed mixing, same bits --
    /// there is deliberately no second hash in this crate.
    pub(crate) fn lattice_at(&self, ix: i64, iy: i64, iz: i64) -> f64 {
        self.lattice(ix, iy, iz)
    }

    /// Trilinear between the eight surrounding lattice values, with each fraction put
    /// through a smoothstep first. Straight linear interpolation would leave visible
    /// creases along every lattice plane — and on terrain a crease is a cliff somebody
    /// sails into.
    ///
    /// Written flat rather than tidily, matching the Python: this is called about forty
    /// times per terrain sample and several million times per chart. It is also transcribed
    /// in exactly the Python's order because floating-point addition is not associative and
    /// this must agree bit-for-bit.
    ///
    /// # A coordinate this lattice cannot address returns NaN
    ///
    /// Python's `int(x // 1)` is an arbitrary-precision integer: it has no lattice cell it
    /// cannot name, and it raises rather than inventing one for `inf` or `nan`. `i64` has
    /// both problems, and neither of them is loud on its own:
    ///
    /// - **`as i64` SATURATES.** `+inf`, and any finite coordinate past ~9.22e18, both come
    ///   out as `i64::MAX`, and the very next line asks for `ix + 1`. In a debug profile
    ///   that is `attempt to add with overflow`; behind `extern "C"`, which is nounwind,
    ///   it is an **abort** that takes the host down and cannot be caught.
    /// - **In release it is worse than an abort**, because the overflow wraps to `i64::MIN`
    ///   and the function returns an ordinary-looking height from a cell chosen by
    ///   wrap-around — the plausible-value failure this crate has now found four times.
    ///
    /// Both entrants are the SAME LINE, reached two ways, and this guard closes both:
    ///
    /// 1. **A non-finite vector component**, through `bindings::continentality_at` and its
    ///    two siblings, which normalise nothing. (`SpherePoint::from_latlon` cannot reach
    ///    it: it turns an infinity into a NaN, and a NaN already came out as a NaN here.)
    /// 2. **The octave schedule's product**, `frequency * lacunarity^(octaves - 1)`, which
    ///    three individually admissible coastal fields compound past the same saturation —
    ///    the abort `WB_MAX_COAST_FINEST_FREQUENCY` was added to refuse, and which a
    ///    one-field-at-a-time sweep is blind to by construction.
    ///
    /// The C ABI's refusal is not made redundant by this: a refusal names the offending
    /// *field*, a NaN tells a host only that something somewhere is wrong. First line and
    /// second line, exactly as `WB_MAX_COAST_GAIN` stands beside
    /// `Continentality::elevation_from_above`'s NaN guard.
    ///
    /// Returning NaN rather than panicking is the same contract `elevation_from_above`
    /// chose and for the same reason: loud-as-a-panic is unavailable across a nounwind
    /// boundary, and NaN is what these exports already document for a question they cannot
    /// answer. It is invisible on the canonical path, where every coordinate is a unit
    /// sphere component times a frequency of order one. See
    /// `a_lattice_coordinate_the_index_cannot_name_surfaces_as_a_nan_rather_than_aborting`.
    pub fn at(&self, x: f64, y: f64, z: f64) -> f64 {
        // floor, never a cast: Python uses int(x // 1), which floors toward negative
        // infinity, and every negative coordinate would otherwise land in the wrong cell.
        let fx_floor = m::floor(x);
        let fy_floor = m::floor(y);
        let fz_floor = m::floor(z);

        // Written as a negated pair of `>=`/`<=` rather than as `abs(..) > LIMIT`, so a NaN
        // takes this branch too: a NaN fails both comparisons, the `!` makes that `true`,
        // and it leaves by the same door. (A NaN coordinate already produced a NaN here
        // through `x - floor(x)`; this only makes that answer explicit and cheaper.)
        // `f64::max`/`min`/`clamp` are banned in this crate precisely because they would
        // NOT have caught the NaN.
        if !(fx_floor >= -LATTICE_LIMIT && fx_floor <= LATTICE_LIMIT)
            || !(fy_floor >= -LATTICE_LIMIT && fy_floor <= LATTICE_LIMIT)
            || !(fz_floor >= -LATTICE_LIMIT && fz_floor <= LATTICE_LIMIT)
        {
            return f64::NAN;
        }

        // The three markers below are true only BECAUSE of the guard above: without it
        // these casts saturate rather than truncate, and Python's `int()` has no
        // saturating behaviour to mirror.
        let ix = fx_floor as i64; // cast-ok: already floored and bounded above, mirrors Python's int(x // 1)
        let iy = fy_floor as i64; // cast-ok: already floored and bounded above, mirrors Python's int(y // 1)
        let iz = fz_floor as i64; // cast-ok: already floored and bounded above, mirrors Python's int(z // 1)

        let fx = x - fx_floor;
        let fy = y - fy_floor;
        let fz = z - fz_floor;

        let ux = fx * fx * (3.0 - 2.0 * fx);
        let uy = fy * fy * (3.0 - 2.0 * fy);
        let uz = fz * fz * (3.0 - 2.0 * fz);

        let (jx, jy, jz) = (ix + 1, iy + 1, iz + 1);
        let c000 = self.lattice(ix, iy, iz);
        let c100 = self.lattice(jx, iy, iz);
        let c010 = self.lattice(ix, jy, iz);
        let c110 = self.lattice(jx, jy, iz);
        let c001 = self.lattice(ix, iy, jz);
        let c101 = self.lattice(jx, iy, jz);
        let c011 = self.lattice(ix, jy, jz);
        let c111 = self.lattice(jx, jy, jz);

        let x00 = c000 + (c100 - c000) * ux;
        let x10 = c010 + (c110 - c010) * ux;
        let x01 = c001 + (c101 - c001) * ux;
        let x11 = c011 + (c111 - c011) * ux;
        let y0 = x00 + (x10 - x00) * uy;
        let y1 = x01 + (x11 - x01) * uy;
        y0 + (y1 - y0) * uz
    }

    /// Several octaves summed, each half the amplitude and twice the frequency of the last.
    ///
    /// The octave count is a parameter rather than a constant because a chart drawn at
    /// twenty-two miles has samples four hundred metres apart, and octaves finer than that
    /// are invisible — they cost time to produce detail below the resolution being drawn,
    /// and they alias while doing it. The caller decides.
    ///
    /// The loop's update order is transcribed from the Python and must not be rearranged:
    /// the sum is order-dependent and this has to agree bit-for-bit.
    pub fn fbm(
        &self,
        x: f64,
        y: f64,
        z: f64,
        frequency: f64,
        octaves: u32,
        gain: f64,
        lacunarity: f64,
    ) -> f64 {
        let mut total = 0.0f64;
        let mut amplitude = 1.0f64;
        let mut loudest = 0.0f64;
        let mut frequency = frequency;
        for _ in 0..octaves {
            total += (self.at(x * frequency, y * frequency, z * frequency) - 0.5) * amplitude;
            loudest += amplitude;
            amplitude *= gain;
            frequency *= lacunarity;
        }
        if loudest == 0.0 {
            0.0
        } else {
            2.0 * total / loudest
        }
    }

    /// A ridged multifractal: creases where the underlying field crosses its midpoint, and
    /// each octave gated by the one above it so fine ridges only appear where a coarse
    /// ridge already is. Returns a value in `[0, 1]`, one at the crest.
    ///
    /// **This is not in `worldbuilder/terrain/noise.py`, and the canonical path never
    /// reaches it.** It is a new primitive, added for the opt-in tectonic structure field
    /// in `tectonics.rs`. `TectonicParams::canonical()` sets `structure_depth` to exactly
    /// 0.0, and `Tectonics::from_margin` branches on that before sampling, so a canonical
    /// world never calls this at all -- which is why a new primitive here does not touch
    /// `worldbuilder/`. Verified rather than assumed: the conformance suite is 398/398 with
    /// `test_conformance.py = 157` on both sides of this commit.
    ///
    /// The technique is Musgrave's and is long published. **Nothing here is transcribed
    /// from any implementation**; [`RIDGE_FEEDBACK`] was swept on this project's own
    /// 4,500 km worlds by `src/bin/mountain_survey.rs`.
    ///
    /// **`1 - abs(n)` folded and re-scaled is nothing but extrema, which makes this the
    /// exact place `f64::min` / `f64::max` / `.clamp()` would have been reached for.** They
    /// are NaN-asymmetric and the repo guard does not catch them, so every bound below is
    /// an explicit branch in the operand order `plates.rs::margin_at` uses -- keep the
    /// first operand unless the second is strictly beyond it, so a NaN floors rather than
    /// propagating into the terrain as a height.
    ///
    /// Args:
    /// frequency: Cycles per unit of input, before lacunarity.
    /// octaves: How many.
    /// gain: Amplitude ratio between successive octaves.
    /// lacunarity: Frequency ratio between successive octaves.
    #[allow(clippy::too_many_arguments)]
    pub fn ridged(
        &self,
        x: f64,
        y: f64,
        z: f64,
        frequency: f64,
        octaves: u32,
        gain: f64,
        lacunarity: f64,
    ) -> f64 {
        self.ridged_with_feedback(x, y, z, frequency, octaves, gain, lacunarity, RIDGE_FEEDBACK)
    }

    /// The same field with its octave feedback supplied rather than taken from the module
    /// constant, so `src/bin/mountain_survey.rs` can sweep for [`RIDGE_FEEDBACK`]'s value on
    /// this project's own worlds instead of inheriting somebody else's tuning.
    ///
    /// Split out exactly as `tectonics.rs`'s `continental` / `continental_with` pair is, and
    /// for the same reason: one implementation, one caller-facing default, and a sweep that
    /// exercises the shipped code rather than a copy of it. `ridged(..)` is
    /// `ridged_with_feedback(.., RIDGE_FEEDBACK)`, the same operations in the same order.
    #[allow(clippy::too_many_arguments)]
    pub fn ridged_with_feedback(
        &self,
        x: f64,
        y: f64,
        z: f64,
        frequency: f64,
        octaves: u32,
        gain: f64,
        lacunarity: f64,
        feedback: f64,
    ) -> f64 {
        let mut total = 0.0f64;
        let mut amplitude = 1.0f64;
        let mut loudest = 0.0f64;
        let mut frequency = frequency;
        let mut weight = 1.0f64;
        for _ in 0..octaves {
            // `at` is in [0, 1); recentre to [-1, 1), fold at zero, and square. The fold is
            // where the crease comes from; the square is what makes it a ridge rather than
            // a corner, because the derivative of `folded^2` is zero at the crest.
            let sample = self.at(x * frequency, y * frequency, z * frequency);
            let folded = 1.0 - (2.0 * sample - 1.0).abs();
            let signal = folded * folded * weight;

            // An octave is only audible where the coarser one was already near its crest,
            // which is what puts fine ridges along the spine of big ones instead of
            // spraying them evenly over the field.
            let raised = signal * feedback;
            let capped = if raised < 1.0 { raised } else { 1.0 };
            weight = if capped > 0.0 { capped } else { 0.0 };

            total += signal * amplitude;
            loudest += amplitude;
            amplitude *= gain;
            frequency *= lacunarity;
        }
        if loudest == 0.0 {
            return 0.0;
        }
        let value = total / loudest;
        // Each octave's `signal` is in [0, 1] and the sum is amplitude-normalised, so this
        // is already in range for every finite input. These two branches are for the inputs
        // that are not finite: a NaN or infinite `gain`, `frequency` or `lacunarity` would
        // otherwise be handed on to the terrain as a height.
        let capped = if value < 1.0 { value } else { 1.0 };
        if capped > 0.0 {
            capped
        } else {
            0.0
        }
    }
}

/// How strongly one octave of [`Noise::ridged`] gates the next.
///
/// Musgrave's construction has a feedback term of this shape; the VALUE is ours. **Swept on
/// this project's own worlds by `src/bin/mountain_survey.rs`** through
/// [`Noise::ridged_with_feedback`], not taken from anyone's tuning.
///
/// The sweep, 0.5 through 4.0 over 40,000 samples of a 200x200 lattice at 0.01 spacing, at
/// the frequency a 120 km wavelength gives on a 4,500 km planet: the fraction of the field
/// above 0.5 -- how much of the belt reads as crest rather than valley -- runs 0.169, 0.278,
/// 0.375, **0.433**, 0.463, 0.479, 0.493. It is a saturating curve and 2.0 is its knee:
/// doubling again to 4.0 buys 0.060 more crest, having bought 0.264 on the way in. The
/// crease sharpness (largest absolute second difference along a line at 1e-3) is flat at
/// 1.83e-1 from 2.0 upward, so nothing above 2.0 buys sharpness either. Full table and host
/// in task-2-report.md.
pub const RIDGE_FEEDBACK: f64 = 2.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_seed_multiplier_is_the_fnv_prime() {
        // Transcribed from worldbuilder/terrain/noise.py. Written without separators
        // because a mis-grouped one survived two reviews and only the differential
        // conformance harness caught it.
        //
        // This must observe `Noise::new`'s actual literal, not just restate it -- a
        // bare `assert_eq!(0x100000001B3u64, 1_099_511_628_211)` is a tautology the
        // compiler folds away without ever reading noise.rs's own constant, so it stays
        // green even if that literal is changed or corrupted. With `salt = 0`, `new`
        // mixes to `seed.wrapping_mul(0x100000001B3) ^ 0`, so `Noise::new(1, 0)`'s
        // internal seed equals the multiplier exactly.
        let n = Noise::new(1, 0);
        assert_eq!(n.seed_for_test(), 1_099_511_628_211);
    }

    #[test]
    fn the_lattice_is_a_pure_function_of_its_coordinates() {
        let n = Noise::new(12345, 0x0C0FFEE);
        assert_eq!(n.lattice(3, -4, 5).to_bits(), n.lattice(3, -4, 5).to_bits());
    }

    #[test]
    fn the_lattice_lands_in_the_unit_interval() {
        let n = Noise::new(12345, 0x0C0FFEE);
        for (ix, iy, iz) in [(0, 0, 0), (1, 2, 3), (-1, -2, -3), (i64::MAX, i64::MIN, 7)] {
            let v = n.lattice(ix, iy, iz);
            assert!((0.0..1.0).contains(&v), "lattice({},{},{}) was {}", ix, iy, iz, v);
        }
    }

    #[test]
    fn neighbouring_cells_differ() {
        let n = Noise::new(12345, 0x0C0FFEE);
        assert_ne!(n.lattice(0, 0, 0).to_bits(), n.lattice(1, 0, 0).to_bits());
        assert_ne!(n.lattice(0, 0, 0).to_bits(), n.lattice(0, 1, 0).to_bits());
        assert_ne!(n.lattice(0, 0, 0).to_bits(), n.lattice(0, 0, 1).to_bits());
    }

    #[test]
    fn salt_separates_two_fields_on_one_world() {
        let a = Noise::new(12345, 0);
        let b = Noise::new(12345, 0x0C0FFEE);
        assert_ne!(a.lattice(2, 2, 2).to_bits(), b.lattice(2, 2, 2).to_bits());
    }

    #[test]
    fn sampling_is_continuous_across_a_cell_boundary() {
        let n = Noise::new(12345, 0x0C0FFEE);
        let just_below = n.at(0.999_999_999, 0.3, 0.3);
        let just_above = n.at(1.000_000_001, 0.3, 0.3);
        assert!((just_below - just_above).abs() < 1e-6, "{} vs {}", just_below, just_above);
    }

    #[test]
    fn sampling_at_a_lattice_point_returns_that_corner() {
        let n = Noise::new(12345, 0x0C0FFEE);
        assert_eq!(n.at(2.0, 3.0, 4.0).to_bits(), n.lattice(2, 3, 4).to_bits());
    }

    #[test]
    fn sampling_stays_in_the_unit_interval() {
        let n = Noise::new(12345, 0x0C0FFEE);
        for i in 0..1000 {
            let t = i as f64 * 0.0137;
            let v = n.at(t, -t * 0.5, t * 0.25);
            assert!((0.0..1.0).contains(&v), "at({}) was {}", t, v);
        }
    }

    #[test]
    fn negative_coordinates_floor_rather_than_truncate() {
        // The trap this port exists to avoid. -0.5 lies in cell -1, not cell 0, so a
        // sample just below zero must interpolate from the -1 cell's corners.
        let n = Noise::new(12345, 0x0C0FFEE);
        let below = n.at(-0.000_000_001, 0.5, 0.5);
        let above = n.at(0.000_000_001, 0.5, 0.5);
        assert!((below - above).abs() < 1e-6, "discontinuity at zero: {} vs {}", below, above);
    }

    #[test]
    fn zero_octaves_is_silent() {
        let n = Noise::new(12345, 0x0C0FFEE);
        assert_eq!(n.fbm(0.3, 0.4, 0.5, 1.25, 0, 0.5, 2.0).to_bits(), 0.0f64.to_bits());
    }

    #[test]
    fn one_octave_is_the_sample_recentred() {
        // With a single octave, loudest is 1.0 and the result is 2 * (at(..) - 0.5).
        let n = Noise::new(12345, 0x0C0FFEE);
        let expected = 2.0 * (n.at(0.3 * 1.25, 0.4 * 1.25, 0.5 * 1.25) - 0.5);
        assert_eq!(n.fbm(0.3, 0.4, 0.5, 1.25, 1, 0.5, 2.0).to_bits(), expected.to_bits());
    }

    #[test]
    fn ridged_stays_in_the_unit_interval() {
        // The bound this primitive's own doc claims. Swept rather than spot-checked, per
        // the branch rule that every abort and hang found in this project was a band and
        // not a cliff -- 4,000 samples across three coordinate scales.
        let n = Noise::new(12345, 0x0C0FFEE);
        for i in 0..1000 {
            let t = i as f64 * 0.0137; // cast-ok: loop counter to float, no truncation
            for scale in [0.1, 1.0, 37.0, 1_000.0] {
                let v = n.ridged(t * scale, -t * scale, t * 0.25 * scale, 1.25, 5, 0.5, 2.0);
                assert!((0.0..=1.0).contains(&v), "ridged at {t}x{scale} was {v}");
            }
        }
    }

    #[test]
    fn ridged_is_a_pure_function_of_its_arguments() {
        let n = Noise::new(12345, 0x0C0FFEE);
        let a = n.ridged(0.31, -0.22, 0.77, 3.0, 4, 0.5, 2.0);
        let b = n.ridged(0.31, -0.22, 0.77, 3.0, 4, 0.5, 2.0);
        assert_eq!(a.to_bits(), b.to_bits());
    }

    #[test]
    fn ridged_has_creases_where_fbm_does_not() {
        // **The property that makes this primitive worth adding**, and it is not "the
        // numbers differ" -- that would be true of any two unrelated fields. A ridged field
        // reaches its MAXIMUM on a crease: the second difference along a line is large and
        // one-signed near the crest, where an equivalent fbm's is small. Measured as the
        // largest absolute second difference over 4,000 samples at 1e-3 spacing, both
        // fields normalised to unit range first so this compares SHAPE and not amplitude.
        let n = Noise::new(12345, 0x0C0FFEE);
        let step = 1e-3;
        let mut sharpest_ridged = 0.0f64;
        let mut sharpest_fbm = 0.0f64;
        for i in 1..4000 {
            let t = i as f64 * step; // cast-ok: loop counter to float, no truncation
            let curve = |f: &dyn Fn(f64) -> f64| {
                let d = f(t - step) - 2.0 * f(t) + f(t + step);
                d.abs()
            };
            let r = curve(&|u: f64| n.ridged(u, 0.5, 0.5, 4.0, 4, 0.5, 2.0));
            // fbm is centred on zero with roughly unit range; ridged is [0, 1]. Halving
            // fbm puts the two on the same scale, so the comparison is not won by units.
            let f = curve(&|u: f64| 0.5 * n.fbm(u, 0.5, 0.5, 4.0, 4, 0.5, 2.0));
            if r > sharpest_ridged {
                sharpest_ridged = r;
            }
            if f > sharpest_fbm {
                sharpest_fbm = f;
            }
        }
        // Measured on this host: ridged 1.29e-3, fbm 1.13e-5 -- two orders of magnitude.
        // Asserted at one order so a change of octave defaults does not make this brittle.
        assert!(
            sharpest_ridged > 10.0 * sharpest_fbm,
            "ridged {sharpest_ridged:e} is not sharply creased against fbm {sharpest_fbm:e}"
        );
    }

    #[test]
    fn ridged_zero_octaves_is_silent() {
        let n = Noise::new(12345, 0x0C0FFEE);
        assert_eq!(n.ridged(0.3, 0.4, 0.5, 1.25, 0, 0.5, 2.0).to_bits(), 0.0f64.to_bits());
    }

    #[test]
    fn ridged_floors_a_nan_rather_than_passing_it_on() {
        // The reason every bound in `ridged` is an explicit branch rather than `clamp`.
        // A NaN reaching the terrain as a height is the failure mode; saturating to a
        // real number in range is the designed behaviour, and this is the assertion that
        // a later refactor to `.clamp()` -- which returns NaN for a NaN input -- breaks.
        let n = Noise::new(12345, 0x0C0FFEE);
        let v = n.ridged(0.3, 0.4, 0.5, f64::NAN, 4, 0.5, 2.0);
        assert!(!v.is_nan(), "a NaN frequency produced {v}");
        assert!((0.0..=1.0).contains(&v), "a NaN frequency produced {v}");
    }

    /// The test that would have caught the abort.
    ///
    /// `at` floors each coordinate and casts it to `i64`, then asks for `ix + 1`. The cast
    /// SATURATES, so `+inf` and any finite coordinate past ~9.22e18 both arrive as
    /// `i64::MAX` and the `+ 1` overflows: `attempt to add with overflow` in a debug profile,
    /// and behind `extern "C"` -- which is nounwind -- an **abort**, not a failing test.
    ///
    /// **Two entrants, one line, and the second is the one a sweep cannot see.** An infinite
    /// vector component reaches it through `bindings::continentality_at` on its own. The
    /// coastal octave schedule reaches the *same* line only as a PRODUCT: `frequency`,
    /// `lacunarity` and `octaves` are each individually admissible at 1e6, 16 and 16, and
    /// `frequency * lacunarity^(octaves - 1)` is about 1.15e24. The four assertions below
    /// prove that blindness rather than describing it -- each field alone, moved off the
    /// canonical base to its own ceiling, still produces a finite answer.
    #[test]
    fn a_lattice_coordinate_the_index_cannot_name_surfaces_as_a_nan_rather_than_aborting() {
        let n = Noise::new(12345, 0x0C0FFEE);

        // FIRST: the field this is discriminating against is a real one. `at` returns
        // ordinary values in [0, 1) at ordinary coordinates, so "is not a NaN" below is a
        // statement about a function that answers, not about a function that never runs.
        for (x, y, z) in [(0.3, 0.4, 0.5), (-7.25, 11.5, -0.125), (1e3, -1e3, 0.0)] {
            let v = n.at(x, y, z);
            assert!(v.is_finite() && (0.0..1.0).contains(&v), "at({x},{y},{z}) was {v}");
        }

        // ENTRANT 1: a non-finite component, every axis, both signs. `from_latlon` cannot
        // produce an infinity, but the three `bindings::continentality_*` take the vector's
        // components raw and normalise nothing.
        let mut hostile = 0usize;
        for bad in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN, -f64::NAN] {
            for axis in 0..3 {
                let (x, y, z) = match axis {
                    0 => (bad, 0.4, 0.5),
                    1 => (0.3, bad, 0.5),
                    _ => (0.3, 0.4, bad),
                };
                hostile += 1;
                assert!(n.at(x, y, z).is_nan(), "at({x},{y},{z}) was {}", n.at(x, y, z));
                // And out through fbm, which is what every field in this crate actually calls.
                assert!(n.fbm(x, y, z, 1.25, 4, 0.5, 2.0).is_nan());
            }
        }
        assert_eq!(hostile, 12, "the hostile population shrank");

        // Finite, and still unnameable: the saturation boundary is not at infinity.
        for bad in [9.3e18, -9.3e18, 1.0e24, -1.0e24, f64::MAX, f64::MIN] {
            assert!(n.at(bad, 0.4, 0.5).is_nan(), "at({bad},..) was {}", n.at(bad, 0.4, 0.5));
        }

        // ENTRANT 2: THE CROSS PRODUCT. This exact record aborted at the same line.
        assert!(
            n.fbm(0.3, 0.4, 0.5, 1.0e6, 16, 0.5, 16.0).is_nan(),
            "the octave schedule's product no longer reaches the lattice bound"
        );
        // ...and the proof that a one-field-at-a-time sweep is blind to it: each of the
        // three fields alone, at the same ceiling, off the canonical base -- all finite.
        // The finest band each asks for is 1e6, 1e6 * 2^15 = 3.3e10 and 1.25 * 16^3 = 5120,
        // every one of them twelve or more orders below the saturation.
        for (frequency, octaves, lacunarity) in [
            (1.0e6, 4u32, 2.0),
            (1.25, 16u32, 2.0),
            (1.25, 4u32, 16.0),
        ] {
            let v = n.fbm(0.3, 0.4, 0.5, frequency, octaves, 0.5, lacunarity);
            assert!(
                v.is_finite(),
                "the single-field sweep stopped being blind at ({frequency}, {octaves}, \
                 {lacunarity}) -- it produced {v}, so the cross-product claim needs redoing"
            );
        }

        // AND THE OTHER HALF: the guard is invisible to every coordinate anything real
        // reaches. The pre-guard body is transcribed inline and compared BY BITS, over a
        // range that spans the canonical field's whole extent and far past it.
        let mut samples = 0usize;
        let mut distinct = std::collections::BTreeSet::new();
        for i in -400i64..400 {
            let t = i as f64 * 0.037; // cast-ok: loop counter to float, exact far below 2^53
            let (x, y, z) = (t, -t * 1.5, t * 0.25 + 1e9);
            let (fx_floor, fy_floor, fz_floor) = (m::floor(x), m::floor(y), m::floor(z));
            let (ix, iy, iz) = (fx_floor as i64, fy_floor as i64, fz_floor as i64); // cast-ok: floored, bounded by construction here
            let (fx, fy, fz) = (x - fx_floor, y - fy_floor, z - fz_floor);
            let ux = fx * fx * (3.0 - 2.0 * fx);
            let uy = fy * fy * (3.0 - 2.0 * fy);
            let uz = fz * fz * (3.0 - 2.0 * fz);
            let (jx, jy, jz) = (ix + 1, iy + 1, iz + 1);
            let x00 = n.lattice(ix, iy, iz)
                + (n.lattice(jx, iy, iz) - n.lattice(ix, iy, iz)) * ux;
            let x10 = n.lattice(ix, jy, iz)
                + (n.lattice(jx, jy, iz) - n.lattice(ix, jy, iz)) * ux;
            let x01 = n.lattice(ix, iy, jz)
                + (n.lattice(jx, iy, jz) - n.lattice(ix, iy, jz)) * ux;
            let x11 = n.lattice(ix, jy, jz)
                + (n.lattice(jx, jy, jz) - n.lattice(ix, jy, jz)) * ux;
            let y0 = x00 + (x10 - x00) * uy;
            let y1 = x01 + (x11 - x01) * uy;
            let want = y0 + (y1 - y0) * uz;
            assert_eq!(n.at(x, y, z).to_bits(), want.to_bits(), "the guard moved at({x},{y},{z})");
            samples += 1;
            distinct.insert(want.to_bits());
        }
        assert_eq!(samples, 800);
        // The comparison is against a field that actually varies, not against a constant.
        assert!(distinct.len() > 700, "only {} distinct values over 800 samples", distinct.len());
    }

    #[test]
    fn more_octaves_stay_centred_near_zero() {
        let n = Noise::new(12345, 0x0C0FFEE);
        for i in 0..500 {
            let t = i as f64 * 0.021;
            let v = n.fbm(t, -t, t * 0.5, 1.25, 4, 0.5, 2.0);
            assert!((-1.5..1.5).contains(&v), "fbm at {} was {}", t, v);
        }
    }
}
