//! The probe field. Not the generator - a deliberately small function that touches every
//! class of operation the generator uses, so that a divergence in any of them shows up.

use crate::corpus::Input;
use crate::detmath as m;

/// Lattice hash, in the same shape the Python generator uses, but stated in wrapping u64
/// arithmetic. See the note in the spike README about why this is not automatically the
/// same as the Python original.
fn lattice(ix: i64, iy: i64, iz: i64, seed: u64) -> f64 {
    let h = (ix as u64).wrapping_mul(0x9E3779B97F4A7C15)
        ^ (iy as u64).wrapping_mul(0xC2B2AE3D27D4EB4F)
        ^ (iz as u64).wrapping_mul(0x165667B19E3779F9);
    let mut h = h ^ seed.wrapping_mul(0x27D4EB2F165667C5);
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51AFD7ED558CCD);
    h ^= h >> 33;
    h = h.wrapping_mul(0xC4CEB9FE1A85EC53);
    h ^= h >> 33;
    h as f64 / (u64::MAX as f64 + 1.0)
}

pub fn evaluate(input: &Input) -> f64 {
    // 1. Normalisation - sqrt, and the operation every sphere point begins with.
    let len = m::sqrt(input.x * input.x + input.y * input.y + input.z * input.z);
    let len = if len == 0.0 { 1.0 } else { len };
    let (x, y, z) = (input.x / len, input.y / len, input.z / len);

    // 2. Spherical coordinates - asin and atan2, as the geometry layer does.
    let lat = m::asin(z.clamp(-1.0, 1.0));
    let lon = m::atan2(y, x);

    // 3. Trigonometry back out, as tangent frames and Euler-pole rotation do.
    let a = m::sin(lat * 3.0) * m::cos(lon * 2.0);

    // 4. Planar distance - hypot, as margin and feature distance do.
    let d = m::hypot(x * 1000.0, y * 1000.0);

    // 5. A saturating blend - tanh, as the shelf and slope shaping do.
    let s = m::tanh(d / 700.0 + a);

    // 6. A fractional power - powf, ahead of the stream power equation of section 14.
    let p = m::powf(d.abs() + 1.0, 0.5);

    // 7. Integer hashing into a float, as the noise lattice does.
    let ix = (x * 4096.0) as i64;
    let iy = (y * 4096.0) as i64;
    let iz = (z * 4096.0) as i64;
    let n = lattice(ix, iy, iz, input.seed);

    // 8. An order-sensitive accumulation. Summed low-to-high deliberately: if any build
    //    reassociates this, the result changes, which is exactly what we want to detect.
    let mut acc = 0.0f64;
    for k in 1..=16u32 {
        let w = 1.0 / (k as f64);
        acc += w * m::sin(a * k as f64 + n);
    }

    a + s + p * 1e-6 + n + acc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus;

    #[test]
    fn evaluation_is_finite_across_the_corpus_head() {
        for i in 0..1000u64 {
            let v = evaluate(&corpus::input_at(i));
            assert!(v.is_finite(), "index {} produced {}", i, v);
        }
    }

    #[test]
    fn evaluation_is_reproducible() {
        let a = evaluate(&corpus::input_at(4242));
        let b = evaluate(&corpus::input_at(4242));
        assert_eq!(a.to_bits(), b.to_bits());
    }

    #[test]
    fn different_seeds_give_different_answers() {
        let a = evaluate(&Input { x: 0.3, y: 0.4, z: 0.5, seed: 1 });
        let b = evaluate(&Input { x: 0.3, y: 0.4, z: 0.5, seed: 2 });
        assert_ne!(a.to_bits(), b.to_bits());
    }
}
