//! How fast the ground is moving, and what that does where two plates meet.
//!
//! Ported from `worldbuilder/plates/kinematics.py`. A plate turning about an axis has a
//! different surface velocity everywhere -- fast at the equator of its own rotation,
//! nothing at all at its pole -- and that variation is not a detail. It is why one margin
//! can be pulling apart at one end and grinding sideways at the other, which is what
//! makes a generated world's geology look like it has reasons.
//!
//! **Nothing here is stored.** A margin is not classified once and remembered; it is
//! worked out at the point somebody asks about, from the two plates' motion there.

use crate::plates::{Margin, Plate};
use crate::sphere::SpherePoint;
use crate::vectors::Vec3;

/// How much of the relative motion must be across the margin rather than along it before
/// the margin is called convergent or divergent rather than transform. Sine of thirty
/// degrees: below that, the plates are mostly sliding past one another.
pub const ACROSS_ENOUGH: f64 = 0.5;

/// `convergent`, `divergent` or `transform` -- what a margin's relative motion is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarginKind {
    Convergent,
    Divergent,
    Transform,
}

impl MarginKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            MarginKind::Convergent => "convergent",
            MarginKind::Divergent => "divergent",
            MarginKind::Transform => "transform",
        }
    }
}

/// What the two plates at a point are doing to each other.
#[derive(Debug, Clone, Copy)]
pub struct Motion {
    /// Which plates, and how far away their margin is.
    pub margin: Option<Margin>,
    /// How fast they approach across the margin. Negative means they are separating.
    pub closing_m_per_myr: f64,
    /// How fast they move along it, unsigned.
    pub sliding_m_per_myr: f64,
    /// `convergent`, `divergent` or `transform`.
    pub kind: MarginKind,
}

/// How fast the ground of one plate is moving at a point.
///
/// The cross product of the rotation vector with the position, scaled by the radius. It
/// is automatically tangent to the sphere, and automatically zero at the plate's own
/// Euler pole, without either being a special case anybody had to write.
pub fn surface_velocity(plate: &Plate, point: &SpherePoint, radius_m: f64) -> Vec3 {
    plate
        .angular_velocity()
        .cross(&point.vector)
        .scaled(radius_m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sphere::EARTH_RADIUS_M;

    /// Slice 1g's `test_plate(index, lat, lon)` fixes the pole at (80.0, 5.0) and the
    /// rate at 0.01, which cannot vary the pole and rate independently. These tests need
    /// both, so this one takes them as explicit parameters.
    fn test_plate_with_pole(
        index: usize,
        seed_lat: f64,
        seed_lon: f64,
        pole_lat: f64,
        pole_lon: f64,
        rate: f64,
    ) -> Plate {
        Plate {
            index,
            seed: SpherePoint::from_latlon(seed_lat, seed_lon),
            euler_pole: SpherePoint::from_latlon(pole_lat, pole_lon),
            rate_rad_per_myr: rate,
        }
    }

    #[test]
    fn a_plate_is_motionless_at_its_own_euler_pole() {
        // Not a special case in the code -- it falls out of the cross product, because
        // the position vector is parallel to the rotation axis there.
        let plate = test_plate_with_pole(0, 0.0, 0.0, 90.0, 0.0, 0.01);
        let at_pole = SpherePoint::from_latlon(90.0, 0.0);
        let v = surface_velocity(&plate, &at_pole, EARTH_RADIUS_M);
        assert!(
            v.length() < 1e-9,
            "velocity at the plate's own Euler pole should vanish, got {}",
            v.length(),
        );
    }

    #[test]
    fn surface_velocity_is_tangent_to_the_sphere() {
        // Also not a special case: a cross product with the position is perpendicular to
        // it.
        let plate = test_plate_with_pole(0, 0.0, 0.0, 90.0, 0.0, 0.01);
        let point = SpherePoint::from_latlon(17.0, 43.0);
        let v = surface_velocity(&plate, &point, EARTH_RADIUS_M);
        assert!(
            v.dot(&point.vector).abs() < 1e-9,
            "velocity must be tangent, dot with position was {}",
            v.dot(&point.vector),
        );
    }

    #[test]
    fn doubling_the_rate_exactly_doubles_the_velocity() {
        // Exact, not approximate, and the reason is worth stating: angular_velocity is
        // linear in the rate; doubling every component multiplies the sum of squares by
        // four; and sqrt(4x) is exactly 2*sqrt(x) in IEEE-754. Scaling by a power of two
        // is exact throughout, so there is no rounding anywhere in this chain.
        let slow = test_plate_with_pole(0, 0.0, 0.0, 90.0, 0.0, 0.01);
        let fast = test_plate_with_pole(0, 0.0, 0.0, 90.0, 0.0, 0.02);
        let point = SpherePoint::from_latlon(17.0, 43.0);
        let a = surface_velocity(&slow, &point, EARTH_RADIUS_M).length();
        let b = surface_velocity(&fast, &point, EARTH_RADIUS_M).length();
        assert_eq!(
            b.to_bits(),
            (2.0 * a).to_bits(),
            "expected exact doubling, got {b} vs {}",
            2.0 * a
        );
    }
}
