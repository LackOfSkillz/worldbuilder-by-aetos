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

/// What two named plates are doing to each other at a point.
///
/// `normal` points across the margin, tangent to the surface, towards `near`. That is
/// why `closing` is the *negative* of the relative velocity's component along it: the
/// nearest plate moving along the normal is moving *away* from the neighbour.
///
/// Split out from `motion_at` so a caller can ask about a margin it has chosen rather
/// than the one that happens to be nearest -- which is what lets several margins be
/// summed instead of one being picked.
pub fn motion_between(
    near: &Plate,
    far: &Plate,
    point: &SpherePoint,
    normal: &Vec3,
    radius_m: f64,
) -> Motion {
    let relative =
        surface_velocity(near, point, radius_m).sub(&surface_velocity(far, point, radius_m));
    let closing = -relative.dot(normal);
    let along = relative.sub(&normal.scaled(relative.dot(normal)));
    let sliding = along.length();

    let speed = relative.length();
    // Python writes `if speed <= 0.0 or abs(closing) / speed < ACROSS_ENOUGH`. The `or`
    // short-circuits, which is the only thing preventing a division by zero when the two
    // plates are moving identically. Do not precompute this condition.
    let kind = if speed <= 0.0 || closing.abs() / speed < ACROSS_ENOUGH {
        MarginKind::Transform
    } else if closing > 0.0 {
        MarginKind::Convergent
    } else {
        MarginKind::Divergent
    };

    Motion {
        margin: None,
        closing_m_per_myr: closing,
        sliding_m_per_myr: sliding,
        kind,
    }
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
        //
        // This vanishes EXACTLY, and the reason is worth pinning because two plausible
        // explanations are both wrong. It is not that `from_latlon(90, 0)` is exactly
        // (0, 0, 1) -- cos(90 deg) is about 6.1e-17, not zero. Nor is it that parallel
        // vectors cross to zero in floating point: `cross` computes (s*y)*z - (s*z)*y,
        // and those groupings do not agree bit for bit in general.
        //
        // It is exact because sin(90 deg) is exactly 1.0, so z is exactly 1.0, and both
        // components that could disagree collapse to s*y - s*y and s*x - s*x. Measured
        // at longitude 0 and 45: exactly zero at both.
        let plate = test_plate_with_pole(0, 0.0, 0.0, 90.0, 0.0, 0.01);
        let at_pole = SpherePoint::from_latlon(90.0, 0.0);
        let v = surface_velocity(&plate, &at_pole, EARTH_RADIUS_M);
        assert_eq!(
            v.length(),
            0.0,
            "this vanishes exactly, not merely to within a tolerance",
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

    /// Both plates' Euler poles at the north pole, so both angular velocities are
    /// `(0, 0, rate)`.
    fn spinning_pair(near_rate: f64, far_rate: f64) -> (Plate, Plate) {
        (
            test_plate_with_pole(0, 0.0, 0.0, 90.0, 0.0, near_rate),
            test_plate_with_pole(1, 0.0, 10.0, 90.0, 0.0, far_rate),
        )
    }

    fn on_the_equator() -> SpherePoint {
        SpherePoint::from_latlon(0.0, 0.0)
    }

    #[test]
    fn two_plates_driving_into_each_other_are_convergent() {
        // Relative velocity is due east; the normal points due west, into `near`. The
        // nearest plate is moving against it, so they are closing.
        let (near, far) = spinning_pair(0.01, 0.02);
        let motion = motion_between(
            &near,
            &far,
            &on_the_equator(),
            &Vec3::new(0.0, 1.0, 0.0),
            EARTH_RADIUS_M,
        );
        assert_eq!(motion.kind, MarginKind::Convergent);
        assert!(motion.closing_m_per_myr > 0.0);
    }

    #[test]
    fn two_plates_pulling_apart_are_divergent() {
        let (near, far) = spinning_pair(0.02, 0.01);
        let motion = motion_between(
            &near,
            &far,
            &on_the_equator(),
            &Vec3::new(0.0, 1.0, 0.0),
            EARTH_RADIUS_M,
        );
        assert_eq!(motion.kind, MarginKind::Divergent);
        assert!(motion.closing_m_per_myr < 0.0);
    }

    #[test]
    fn plates_sliding_past_one_another_are_transform() {
        // The normal points due north, perpendicular to the eastward relative motion,
        // so nothing is crossing the margin at all.
        let (near, far) = spinning_pair(0.02, 0.01);
        let motion = motion_between(
            &near,
            &far,
            &on_the_equator(),
            &Vec3::new(0.0, 0.0, 1.0),
            EARTH_RADIUS_M,
        );
        assert_eq!(motion.kind, MarginKind::Transform);
    }

    #[test]
    fn the_across_enough_threshold_is_hit_exactly_and_is_not_inclusive() {
        // |closing| / speed equals `a` exactly for a normal of (0, a, b). At a = 0.5 the
        // ratio is exactly ACROSS_ENOUGH, and the Python's test is a strict `<`, so this
        // must NOT be transform. At 0.4 it must be. This fails if ACROSS_ENOUGH is
        // mistyped, and it fails if the comparison is loosened to `<=`.
        let (near, far) = spinning_pair(0.02, 0.01);
        let root_three_over_two = crate::detmath::sqrt(0.75);
        let exactly_at = motion_between(
            &near,
            &far,
            &on_the_equator(),
            &Vec3::new(0.0, 0.5, root_three_over_two),
            EARTH_RADIUS_M,
        );
        assert_ne!(
            exactly_at.kind,
            MarginKind::Transform,
            "a ratio of exactly ACROSS_ENOUGH is not below it, so the strict `<` must not fire",
        );
        let just_below = motion_between(
            &near,
            &far,
            &on_the_equator(),
            &Vec3::new(0.0, 0.4, crate::detmath::sqrt(1.0 - 0.16)),
            EARTH_RADIUS_M,
        );
        assert_eq!(just_below.kind, MarginKind::Transform);
    }

    #[test]
    fn a_stationary_pair_is_transform_rather_than_dividing_by_zero() {
        // speed is exactly 0.0, so `abs(closing) / speed` would be 0.0 / 0.0. Only the
        // short-circuit prevents it.
        let (near, far) = spinning_pair(0.01, 0.01);
        let motion = motion_between(
            &near,
            &far,
            &on_the_equator(),
            &Vec3::new(0.0, 1.0, 0.0),
            EARTH_RADIUS_M,
        );
        assert_eq!(motion.kind, MarginKind::Transform);
        // Exact equality is legitimate here, and the reason is worth stating: these are
        // products of an exactly-zero vector, not the residue of cancellation between
        // unequal quantities.
        assert_eq!(motion.closing_m_per_myr, 0.0);
        assert_eq!(motion.sliding_m_per_myr, 0.0);
    }
}
