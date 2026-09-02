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
                .wrapping_mul(0x0000_0001_0000_01B3)
                ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15),
        }
    }

    /// An integer avalanche rather than a cryptographic digest. A real digest would be just
    /// as deterministic and about thirty times slower, and this is called eight times per
    /// octave per sample.
    fn lattice(&self, ix: i64, iy: i64, iz: i64) -> f64 {
        let h = (ix as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)  // cast-ok: convert signed lattice coordinate to unsigned for hash
            ^ (iy as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)   // cast-ok: convert signed lattice coordinate to unsigned for hash
            ^ (iz as u64).wrapping_mul(0x1656_67B1_9E37_79F9);  // cast-ok: convert signed lattice coordinate to unsigned for hash
        let mut h = h ^ self.seed.wrapping_mul(0x27D4_EB2F_1656_67C5);
        h ^= h >> 33;
        h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
        h ^= h >> 33;
        h = h.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
        h ^= h >> 33;
        h as f64 / SCALE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
