//! PyO3 surface. Conversion only — no arithmetic lives here, so the maths modules stay
//! usable from a plain Rust or WASM build with no Python anywhere in the picture.

use pyo3::prelude::*;

use crate::sphere::SpherePoint;
use crate::vectors::Vec3;

#[pyfunction]
pub fn vec3_length(x: f64, y: f64, z: f64) -> f64 {
    Vec3::new(x, y, z).length()
}

#[pyfunction]
pub fn vec3_cross(ax: f64, ay: f64, az: f64, bx: f64, by: f64, bz: f64) -> (f64, f64, f64) {
    let c = Vec3::new(ax, ay, az).cross(&Vec3::new(bx, by, bz));
    (c.x, c.y, c.z)
}

#[pyfunction]
pub fn vec3_normalised(x: f64, y: f64, z: f64) -> Option<(f64, f64, f64)> {
    Vec3::new(x, y, z).normalised().map(|v| (v.x, v.y, v.z))
}

#[pyfunction]
pub fn sphere_from_latlon(latitude_deg: f64, longitude_deg: f64) -> (f64, f64, f64) {
    let v = SpherePoint::from_latlon(latitude_deg, longitude_deg).vector;
    (v.x, v.y, v.z)
}

#[pyfunction]
pub fn sphere_to_latlon(x: f64, y: f64, z: f64) -> (f64, f64) {
    SpherePoint { vector: Vec3::new(x, y, z) }.to_latlon()
}

#[pyfunction]
pub fn sphere_angle_to(ax: f64, ay: f64, az: f64, bx: f64, by: f64, bz: f64) -> f64 {
    let a = SpherePoint { vector: Vec3::new(ax, ay, az) };
    let b = SpherePoint { vector: Vec3::new(bx, by, bz) };
    a.angle_to(&b)
}

#[pyfunction]
pub fn sphere_distance_to(
    ax: f64, ay: f64, az: f64,
    bx: f64, by: f64, bz: f64,
    radius_m: f64,
) -> f64 {
    let a = SpherePoint { vector: Vec3::new(ax, ay, az) };
    let b = SpherePoint { vector: Vec3::new(bx, by, bz) };
    a.distance_to(&b, radius_m)
}

#[pyfunction]
pub fn noise_at(seed: u64, salt: u64, x: f64, y: f64, z: f64) -> f64 {
    crate::noise::Noise::new(seed, salt).at(x, y, z)
}

#[pyfunction]
#[allow(clippy::too_many_arguments)]
pub fn noise_fbm(
    seed: u64,
    salt: u64,
    x: f64,
    y: f64,
    z: f64,
    frequency: f64,
    octaves: u32,
    gain: f64,
    lacunarity: f64,
) -> f64 {
    crate::noise::Noise::new(seed, salt).fbm(x, y, z, frequency, octaves, gain, lacunarity)
}

#[pyfunction]
pub fn frame_at(x: f64, y: f64, z: f64, radius_m: f64) -> (f64, f64, f64, f64, f64, f64, f64, f64, f64) {
    let origin = SpherePoint { vector: Vec3::new(x, y, z) };
    let f = crate::tangent::TangentFrame::at(&origin, radius_m);
    (f.east.x, f.east.y, f.east.z, f.north.x, f.north.y, f.north.z, f.up.x, f.up.y, f.up.z)
}

#[pyfunction]
pub fn frame_local_to_sphere(
    x: f64, y: f64, z: f64, radius_m: f64, east_m: f64, north_m: f64,
) -> (f64, f64, f64) {
    let origin = SpherePoint { vector: Vec3::new(x, y, z) };
    let f = crate::tangent::TangentFrame::at(&origin, radius_m);
    let p = f.local_to_sphere(east_m, north_m);
    (p.vector.x, p.vector.y, p.vector.z)
}

#[pyfunction]
#[allow(clippy::too_many_arguments)]
pub fn frame_sphere_to_local(
    x: f64, y: f64, z: f64, radius_m: f64, px: f64, py: f64, pz: f64,
) -> (f64, f64) {
    let origin = SpherePoint { vector: Vec3::new(x, y, z) };
    let f = crate::tangent::TangentFrame::at(&origin, radius_m);
    f.sphere_to_local(&SpherePoint { vector: Vec3::new(px, py, pz) })
}
