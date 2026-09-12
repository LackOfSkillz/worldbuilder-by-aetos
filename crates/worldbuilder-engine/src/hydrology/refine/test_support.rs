//! Fixtures the refinement tests share, now that the stages sit in files of their own. Nothing
//! here is new: it is the helper set the single `refine.rs` test module had, lifted out so each
//! stage's tests can reach it. Fixtures only one stage uses stay in that stage's test module.

use super::Ground;
use crate::hydrology::{HydroParams, ReachPoint};
use crate::sphere::SpherePoint;

pub const R: f64 = 6_371_000.0;
/// Metres per degree on this test radius.
pub const M_PER_DEG: f64 = R * std::f64::consts::PI / 180.0;

pub fn point(lat_deg: f64, lon_deg: f64, bed_m: f64) -> ReachPoint {
    ReachPoint { lat_deg, lon_deg, bed_m, width_m: 10.0, depth_m: 1.0, flow_m2: 1.0e9 }
}

/// A 30 km segment due east along the equator: a at 0 E, b at 30 km east.
pub fn ends(bed_a: f64, bed_b: f64) -> (ReachPoint, ReachPoint) {
    (point(0.0, 0.0, bed_a), point(0.0, 30_000.0 / M_PER_DEG, bed_b))
}

/// North of the equator in metres, and east of 0 E in metres (small-angle, test only).
pub fn north_east(p: &SpherePoint) -> (f64, f64) {
    let (lat, lon) = p.to_latlon();
    (lat * M_PER_DEG, lon * M_PER_DEG)
}

pub fn ground<'a>(height: &'a dyn Fn(&SpherePoint) -> f64) -> Ground<'a> {
    Ground { height_m: height, radius_m: R, corridor_m: 20_000.0, seed: 7 }
}

pub fn params() -> HydroParams {
    HydroParams::earth_like(1_000)
}

pub fn wide(lat_deg: f64, lon_deg: f64, bed_m: f64, width_m: f64) -> ReachPoint {
    ReachPoint { lat_deg, lon_deg, bed_m, width_m, depth_m: 5.0, flow_m2: 1.0e12 }
}

/// A line of points 1,500 m apart along the equator, each pushed `north_m` north of it.
pub fn line_of(beds: &[f64], north_m: &[f64]) -> Vec<ReachPoint> {
    beds.iter().zip(north_m).enumerate()
        .map(|(i, (&bed, &n))| point(n / M_PER_DEG, (i as f64 * 1_500.0) / M_PER_DEG, bed))
        .collect()
}
