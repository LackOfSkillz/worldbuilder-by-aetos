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

    /// Trilinear between the eight surrounding lattice values, with each fraction put
    /// through a smoothstep first. Straight linear interpolation would leave visible
    /// creases along every lattice plane — and on terrain a crease is a cliff somebody
    /// sails into.
    ///
    /// Written flat rather than tidily, matching the Python: this is called about forty
    /// times per terrain sample and several million times per chart. It is also transcribed
    /// in exactly the Python's order because floating-point addition is not associative and
    /// this must agree bit-for-bit.
    pub fn at(&self, x: f64, y: f64, z: f64) -> f64 {
        // floor, never a cast: Python uses int(x // 1), which floors toward negative
        // infinity, and every negative coordinate would otherwise land in the wrong cell.
        let fx_floor = m::floor(x);
        let fy_floor = m::floor(y);
        let fz_floor = m::floor(z);
        let ix = fx_floor as i64; // cast-ok: already floored, mirrors Python's int(x // 1)
        let iy = fy_floor as i64; // cast-ok: already floored, mirrors Python's int(y // 1)
        let iz = fz_floor as i64; // cast-ok: already floored, mirrors Python's int(z // 1)

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
}

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
    fn more_octaves_stay_centred_near_zero() {
        let n = Noise::new(12345, 0x0C0FFEE);
        for i in 0..500 {
            let t = i as f64 * 0.021;
            let v = n.fbm(t, -t, t * 0.5, 1.25, 4, 0.5, 2.0);
            assert!((-1.5..1.5).contains(&v), "fbm at {} was {}", t, v);
        }
    }
}
