//! Plates, and the Voronoi lookup that says which one a point is on.
//!
//! Ported from `worldbuilder/plates/model.py` and `worldbuilder/plates/lookup.py`.
//!
//! A point belongs to the plate whose seed is nearest, which is the whole of the Voronoi
//! construction. Everything else here is arithmetic in service of asking that question
//! quickly, and of asking how far the point is from the answer changing.

use crate::sphere::SpherePoint;
use crate::vectors::Vec3;

/// One plate: where it is, what it turns about, and how fast.
#[derive(Debug, Clone, Copy)]
pub struct Plate {
    /// Which plate this is. Stable for a given seed and count.
    pub index: usize,
    /// Its centre. A point belongs to the plate whose seed is nearest.
    pub seed: SpherePoint,
    /// The axis it turns about.
    pub euler_pole: SpherePoint,
    /// Radians per million years. **Signed**: the sign and the pole together give the
    /// sense of rotation, so there is no separate clockwise flag to get wrong.
    pub rate_rad_per_myr: f64,
}

impl Plate {
    /// The rotation vector — the pole, scaled by the rate.
    ///
    /// Combining the two into one vector is what makes surface velocity a single cross
    /// product rather than a special case at the pole itself.
    pub fn angular_velocity(&self) -> Vec3 {
        self.euler_pole.vector.scaled(self.rate_rad_per_myr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_plate(index: usize, rate: f64) -> Plate {
        Plate {
            index,
            seed: SpherePoint::from_latlon(10.0, 20.0),
            euler_pole: SpherePoint::from_latlon(80.0, 5.0),
            rate_rad_per_myr: rate,
        }
    }

    #[test]
    fn angular_velocity_is_the_pole_scaled_by_the_rate() {
        let p = a_plate(0, 0.01);
        let omega = p.angular_velocity();
        let pole = p.euler_pole.vector;
        assert_eq!(omega.x.to_bits(), (pole.x * 0.01).to_bits());
        assert_eq!(omega.y.to_bits(), (pole.y * 0.01).to_bits());
        assert_eq!(omega.z.to_bits(), (pole.z * 0.01).to_bits());
    }

    #[test]
    fn a_negative_rate_reverses_the_rotation() {
        // The sign and the pole together give the sense of rotation; there is no
        // separate clockwise flag to get wrong.
        let forward = a_plate(0, 0.01).angular_velocity();
        let backward = a_plate(0, -0.01).angular_velocity();
        assert_eq!(backward.x.to_bits(), (-forward.x).to_bits());
        assert_eq!(backward.z.to_bits(), (-forward.z).to_bits());
    }

    #[test]
    fn a_zero_rate_is_a_still_plate() {
        let omega = a_plate(0, 0.0).angular_velocity();
        assert_eq!(omega.x.to_bits(), 0.0f64.to_bits());
        assert_eq!(omega.y.to_bits(), 0.0f64.to_bits());
        assert_eq!(omega.z.to_bits(), 0.0f64.to_bits());
    }
}
