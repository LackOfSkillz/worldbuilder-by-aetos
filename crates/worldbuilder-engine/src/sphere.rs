//! A place on the planet, as a unit vector from its centre.
//!
//! Ported from `worldbuilder/geometry/sphere.py`. The radius is deliberately not stored:
//! a point is a *direction*, and how big the planet is belongs to the world rather than
//! to each of the billions of places on it.

use crate::detmath as m;
use crate::vectors::Vec3;

pub const EARTH_RADIUS_M: f64 = 6_371_000.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpherePoint {
    pub vector: Vec3,
}

impl SpherePoint {
    /// `None` where Python raises `ValueError`, for a vector with no direction.
    pub fn from_vector(vector: &Vec3) -> Option<Self> {
        vector.normalised().map(|v| Self { vector: v })
    }

    /// Longitude is not normalised first and does not need to be: sine and cosine are
    /// periodic, so -180, +180 and +540 give the same vector by arithmetic rather than by
    /// a rule somebody has to remember.
    pub fn from_latlon(latitude_deg: f64, longitude_deg: f64) -> Self {
        let latitude = m::to_radians(latitude_deg);
        let longitude = m::to_radians(longitude_deg);
        let cos_lat = m::cos(latitude);
        Self {
            vector: Vec3::new(
                cos_lat * m::cos(longitude),
                cos_lat * m::sin(longitude),
                m::sin(latitude),
            ),
        }
    }

    /// At a pole the longitude returned is zero, which is a convention rather than a
    /// fact: every meridian meets there and none of them is the answer.
    pub fn to_latlon(&self) -> (f64, f64) {
        let clamped = if self.vector.z < -1.0 {
            -1.0
        } else if self.vector.z > 1.0 {
            1.0
        } else {
            self.vector.z
        };
        let latitude = m::to_degrees(m::asin(clamped));
        let longitude = if clamped == 1.0 || clamped == -1.0 {
            0.0
        } else {
            m::to_degrees(m::atan2(self.vector.y, self.vector.x))
        };
        (latitude, longitude)
    }

    /// By arc tangent of the cross and dot products rather than the arc cosine of the dot
    /// alone. The simpler form loses precision for points close together — exactly the
    /// case a ship spends its whole life in.
    pub fn angle_to(&self, other: &SpherePoint) -> f64 {
        let across = self.vector.cross(&other.vector).length();
        let along = self.vector.dot(&other.vector);
        m::atan2(across, along)
    }

    pub fn distance_to(&self, other: &SpherePoint, radius_m: f64) -> f64 {
        self.angle_to(other) * radius_m
    }
}

#[cfg(test)]
mod tests {
    use crate::vectors::Vec3;
    use super::{SpherePoint, EARTH_RADIUS_M};

    #[test]
    fn latlon_round_trips_at_an_ordinary_place() {
        let point = SpherePoint::from_latlon(51.5, -0.12);
        let (lat, lon) = point.to_latlon();
        assert!((lat - 51.5).abs() < 1e-12, "lat was {}", lat);
        assert!((lon + 0.12).abs() < 1e-12, "lon was {}", lon);
    }

    #[test]
    fn a_pole_reports_zero_longitude_by_convention() {
        let (lat, lon) = SpherePoint::from_latlon(90.0, 137.0).to_latlon();
        assert!((lat - 90.0).abs() < 1e-9, "lat was {}", lat);
        assert_eq!(lon.to_bits(), 0.0f64.to_bits());
    }

    #[test]
    fn longitude_is_periodic_without_normalising_it_first() {
        let a = SpherePoint::from_latlon(10.0, 180.0);
        let b = SpherePoint::from_latlon(10.0, 540.0);
        assert!(a.angle_to(&b) < 1e-12);
    }

    #[test]
    fn a_quarter_turn_subtends_a_right_angle() {
        let equator = SpherePoint::from_latlon(0.0, 0.0);
        let pole = SpherePoint::from_latlon(90.0, 0.0);
        let expected = std::f64::consts::FRAC_PI_2;
        assert!((equator.angle_to(&pole) - expected).abs() < 1e-12);
    }

    #[test]
    fn distance_scales_the_angle_by_the_radius() {
        let a = SpherePoint::from_latlon(0.0, 0.0);
        let b = SpherePoint::from_latlon(0.0, 1.0);
        let expected = a.angle_to(&b) * EARTH_RADIUS_M;
        assert_eq!(a.distance_to(&b, EARTH_RADIUS_M).to_bits(), expected.to_bits());
    }

    #[test]
    fn from_vector_normalises() {
        let point = SpherePoint::from_vector(&Vec3::new(0.0, 0.0, 9.0)).expect("non-zero");
        assert_eq!(point.vector.z.to_bits(), 1.0f64.to_bits());
    }
}
