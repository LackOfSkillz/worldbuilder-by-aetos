//! A local flat coordinate system, tangent to the planet at one point.
//!
//! Ported from `worldbuilder/geometry/tangent.py`. This is how a curved planet becomes a
//! flat chart in metres and back: maritime works in local metres and never learns the
//! world is a sphere, and the shelf shaper walks fixed distances along geodesics rather
//! than nudging raw coordinates, which would step off the sphere entirely.

use crate::detmath as m;
use crate::sphere::{SpherePoint, EARTH_RADIUS_M};
use crate::vectors::{Vec3, DEGENERATE, NORTH_AXIS, POLAR_FALLBACK};

#[derive(Debug, Clone, Copy)]
pub struct TangentFrame {
    /// Where the chart touches the globe; local (0, 0).
    pub origin: SpherePoint,
    /// Unit vector, increasing local x.
    pub east: Vec3,
    /// Unit vector, increasing local y.
    pub north: Vec3,
    /// Unit vector, away from the centre. Equal to the origin's vector.
    pub up: Vec3,
    pub radius_m: f64,
}

impl TangentFrame {
    /// East is the direction at right angles to both straight up and the planet's axis.
    ///
    /// At a pole east means nothing — every direction from the north pole is south — and
    /// that is a fact about poles rather than a failure of the maths. The cross product
    /// goes to zero there and the basis cannot be derived, so one is chosen instead.
    /// Which direction it is does not matter in the slightest. That it is the *same* one
    /// on every call is the whole requirement.
    pub fn at(origin: &SpherePoint, radius_m: f64) -> Self {
        let up = origin.vector;
        let mut sideways = NORTH_AXIS.cross(&up);
        if sideways.length() <= DEGENERATE {
            // At a pole, or near enough that the arithmetic has lost its nerve.
            sideways = POLAR_FALLBACK.cross(&up);
            if sideways.length() <= DEGENERATE {
                // The fallback was itself parallel to up, which cannot happen for a
                // planet whose axis is z — but a fixed second answer costs one line and
                // removes the only path here that could ever fail.
                sideways = Vec3::new(0.0, 1.0, 0.0).cross(&up);
            }
        }
        // The Python calls .normalised() and would raise on a zero vector; by this point
        // the fallback chain has guaranteed a non-zero result, so the None case is
        // unreachable. Falling back to POLAR_FALLBACK rather than panicking keeps the
        // function total, and the conformance corpus covers the poles.
        let east = sideways.normalised().unwrap_or(POLAR_FALLBACK);
        let north = up.cross(&east);
        Self { origin: *origin, east, north, up, radius_m }
    }

    /// Convenience: a frame centred on a named latitude and longitude.
    pub fn at_latlon(latitude_deg: f64, longitude_deg: f64, radius_m: f64) -> Self {
        Self::at(&SpherePoint::from_latlon(latitude_deg, longitude_deg), radius_m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sphere::{SpherePoint, EARTH_RADIUS_M};
    use crate::vectors::Vec3;

    #[test]
    fn the_basis_is_orthonormal_at_an_ordinary_place() {
        let frame = TangentFrame::at_latlon(51.5, -0.12, EARTH_RADIUS_M);
        assert!((frame.east.length() - 1.0).abs() < 1e-12);
        assert!((frame.north.length() - 1.0).abs() < 1e-12);
        assert!((frame.up.length() - 1.0).abs() < 1e-12);
        assert!(frame.east.dot(&frame.north).abs() < 1e-12);
        assert!(frame.east.dot(&frame.up).abs() < 1e-12);
        assert!(frame.north.dot(&frame.up).abs() < 1e-12);
    }

    #[test]
    fn east_points_east_on_the_equator() {
        // At (0, 0) the up vector is +x, so east must be +y.
        let frame = TangentFrame::at_latlon(0.0, 0.0, EARTH_RADIUS_M);
        assert!((frame.east.y - 1.0).abs() < 1e-12, "east was {:?}", frame.east);
    }

    #[test]
    fn a_pole_still_yields_an_orthonormal_basis() {
        for lat in [90.0, -90.0] {
            let frame = TangentFrame::at_latlon(lat, 0.0, EARTH_RADIUS_M);
            assert!((frame.east.length() - 1.0).abs() < 1e-9, "at {}", lat);
            assert!((frame.north.length() - 1.0).abs() < 1e-9, "at {}", lat);
            assert!(frame.east.dot(&frame.up).abs() < 1e-9, "at {}", lat);
        }
    }

    #[test]
    fn a_pole_yields_the_same_basis_every_time() {
        // The whole requirement. A frame that reshuffled itself between two calls would
        // move every ship it held.
        let a = TangentFrame::at(&SpherePoint { vector: Vec3::new(0.0, 0.0, 1.0) }, EARTH_RADIUS_M);
        let b = TangentFrame::at(&SpherePoint { vector: Vec3::new(0.0, 0.0, 1.0) }, EARTH_RADIUS_M);
        assert_eq!(a.east.x.to_bits(), b.east.x.to_bits());
        assert_eq!(a.east.y.to_bits(), b.east.y.to_bits());
        assert_eq!(a.east.z.to_bits(), b.east.z.to_bits());
    }

    #[test]
    fn up_is_the_origins_own_vector() {
        let origin = SpherePoint::from_latlon(31.0, 7.0);
        let frame = TangentFrame::at(&origin, EARTH_RADIUS_M);
        assert_eq!(frame.up.x.to_bits(), origin.vector.x.to_bits());
        assert_eq!(frame.up.y.to_bits(), origin.vector.y.to_bits());
        assert_eq!(frame.up.z.to_bits(), origin.vector.z.to_bits());
    }
}
