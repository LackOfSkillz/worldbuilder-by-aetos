//! Vectors in the frame the whole planet is expressed in.
//!
//! Ported from `worldbuilder/geometry/vectors.py`. The arithmetic is transcribed rather
//! than rederived: floating-point addition is not associative, so a cross product written
//! in a different order is a different number, and this must agree with the Python it
//! replaces bit-for-bit.

use crate::detmath as m;

/// x towards longitude zero on the equator, y towards ninety east, z towards the pole.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub fn add(&self, other: &Vec3) -> Vec3 {
        Vec3::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }

    pub fn sub(&self, other: &Vec3) -> Vec3 {
        Vec3::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }

    pub fn scaled(&self, factor: f64) -> Vec3 {
        Vec3::new(self.x * factor, self.y * factor, self.z * factor)
    }

    pub fn dot(&self, other: &Vec3) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    pub fn cross(&self, other: &Vec3) -> Vec3 {
        Vec3::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    pub fn length(&self) -> f64 {
        m::sqrt(self.dot(self))
    }

    /// `None` where Python raises `ValueError`: a zero vector has no direction to keep.
    pub fn normalised(&self) -> Option<Vec3> {
        let magnitude = self.length();
        if magnitude == 0.0 {
            return None;
        }
        Some(self.scaled(1.0 / magnitude))
    }
}

/// The axis the planet turns about, and so the direction of the north pole.
pub const NORTH_AXIS: Vec3 = Vec3::new(0.0, 0.0, 1.0);

/// What to build a frame from at a pole, where east has no meaning. Which direction is
/// chosen does not matter; that the same one is chosen every time does.
pub const POLAR_FALLBACK: Vec3 = Vec3::new(1.0, 0.0, 0.0);

/// How nearly parallel two vectors may be before their cross product stops being a
/// trustworthy direction.
pub const DEGENERATE: f64 = 1e-9;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cross_follows_the_right_hand_rule() {
        let x = Vec3::new(1.0, 0.0, 0.0);
        let y = Vec3::new(0.0, 1.0, 0.0);
        let z = x.cross(&y);
        assert_eq!(z.x.to_bits(), 0.0f64.to_bits());
        assert_eq!(z.y.to_bits(), 0.0f64.to_bits());
        assert_eq!(z.z.to_bits(), 1.0f64.to_bits());
    }

    #[test]
    fn length_of_three_four_zero_is_exactly_five() {
        assert_eq!(Vec3::new(3.0, 4.0, 0.0).length().to_bits(), 5.0f64.to_bits());
    }

    #[test]
    fn normalised_preserves_direction_and_sets_length_one() {
        let unit = Vec3::new(0.0, 0.0, 7.0).normalised().expect("non-zero");
        assert_eq!(unit.z.to_bits(), 1.0f64.to_bits());
    }

    #[test]
    fn a_zero_vector_has_no_direction() {
        assert!(Vec3::new(0.0, 0.0, 0.0).normalised().is_none());
    }

    #[test]
    fn add_and_sub_are_componentwise() {
        let a = Vec3::new(1.0, 2.0, 3.0);
        let b = Vec3::new(0.5, 0.5, 0.5);
        assert_eq!(a.add(&b).x.to_bits(), 1.5f64.to_bits());
        assert_eq!(a.sub(&b).z.to_bits(), 2.5f64.to_bits());
    }
}
