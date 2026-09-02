//! Corpus generation. Integer hashing only - no transcendentals, so the corpus itself
//! cannot be the thing that differs between targets.

/// One corpus sample.
pub struct Input {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub seed: u64,
}

/// How many samples a full run evaluates.
pub const COUNT: u64 = 5_000_000;

/// A 64-bit avalanche. Same shape as the generator's lattice hash, in wrapping u64
/// arithmetic so the semantics are stated rather than inherited.
fn mix(mut h: u64) -> u64 {
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51AFD7ED558CCD);
    h ^= h >> 33;
    h = h.wrapping_mul(0xC4CEB9FE1A85EC53);
    h ^= h >> 33;
    h
}

/// A float in [-1, 1), from the top 53 bits so the mantissa is fully exercised.
fn unit(h: u64) -> f64 {
    let f = (h >> 11) as f64 / (1u64 << 53) as f64;
    f * 2.0 - 1.0
}

pub fn input_at(index: u64) -> Input {
    // The first four indices are pinned to the places a sphere is most likely to break.
    match index {
        0 => return Input { x: 0.0, y: 0.0, z: 1.0, seed: 1 },
        1 => return Input { x: 0.0, y: 0.0, z: -1.0, seed: 1 },
        2 => return Input { x: 1.0, y: 0.0, z: 0.0, seed: 1 },
        3 => return Input { x: -1.0, y: 0.0, z: 0.0, seed: 1 },
        _ => {}
    }

    let hx = mix(index.wrapping_mul(0x9E3779B97F4A7C15));
    let hy = mix(index.wrapping_mul(0xC2B2AE3D27D4EB4F) ^ 0xA5A5A5A5A5A5A5A5);
    let hz = mix(index.wrapping_mul(0x165667B19E3779F9) ^ 0x5A5A5A5A5A5A5A5A);
    let hs = mix(index ^ 0x27D4EB2F165667C5);

    Input {
        x: unit(hx),
        y: unit(hy),
        z: unit(hz),
        // A handful of seeds rather than one, so a seed-dependent divergence is visible.
        seed: (hs % 7) + 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inputs_are_reproducible() {
        let a = input_at(12345);
        let b = input_at(12345);
        assert_eq!(a.x.to_bits(), b.x.to_bits());
        assert_eq!(a.y.to_bits(), b.y.to_bits());
        assert_eq!(a.z.to_bits(), b.z.to_bits());
        assert_eq!(a.seed, b.seed);
    }

    #[test]
    fn inputs_differ_between_indices() {
        let a = input_at(1);
        let b = input_at(2);
        assert_ne!(a.x.to_bits(), b.x.to_bits());
    }

    #[test]
    fn includes_the_awkward_places() {
        // Poles, the meridian, and the equator are where a spherical field is most likely
        // to be wrong, so the first few indices are pinned to them rather than hashed.
        let pole = input_at(0);
        assert_eq!(pole.z.to_bits(), 1.0f64.to_bits());
        let equator = input_at(2);
        assert_eq!(equator.z.to_bits(), 0.0f64.to_bits());
    }
}
