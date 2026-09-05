//! PyO3 surface. Conversion only — no arithmetic lives here, so the maths modules stay
//! usable from a plain Rust or WASM build with no Python anywhere in the picture.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use pyo3::prelude::*;

use crate::continentality::Continentality;
use crate::plates::{Plate, PlateSet};
use crate::sphere::SpherePoint;
use crate::vectors::Vec3;

/// Cache of calibrated `Continentality` instances, keyed on `(seed, land_fraction bits,
/// radius_m bits)`. Calibration is a 4,000-sample sort and costs a few milliseconds;
/// building one per binding call would make a corpus loop over thousands of points
/// unusably slow. Keying on the bit pattern of the floats (rather than the floats
/// themselves) sidesteps `f64: !Eq` while never comparing two distinct callers' values as
/// equal when they are not bit-identical -- which is exactly the equality the cache is
/// permitted to use, since anything coarser could serve a value calibrated for a
/// different land fraction. The map only ever grows, which is fine here: callers pass a
/// small, fixed number of (seed, land_fraction) pairs per process (tests, or one running
/// game world), not an unbounded stream.
type ContinentalityCache = Mutex<HashMap<(u64, u64, u64), Continentality>>;

fn continentality_cache() -> &'static ContinentalityCache {
    static CACHE: OnceLock<ContinentalityCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cached_continentality(seed: u64, radius_m: f64, land_fraction: f64) -> Continentality {
    let key = (seed, radius_m.to_bits(), land_fraction.to_bits());
    let mut cache = continentality_cache().lock().expect("continentality cache poisoned");
    *cache
        .entry(key)
        .or_insert_with(|| Continentality::new(seed, radius_m, land_fraction))
}

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

#[pyfunction]
pub fn continentality_calibration(seed: u64, land_fraction: f64) -> (f64, f64) {
    let c = cached_continentality(seed, crate::sphere::EARTH_RADIUS_M, land_fraction);
    (c.shore(), c.spread())
}

#[pyfunction]
pub fn continentality_at(seed: u64, land_fraction: f64, x: f64, y: f64, z: f64) -> f64 {
    let c = cached_continentality(seed, crate::sphere::EARTH_RADIUS_M, land_fraction);
    c.at(&SpherePoint { vector: Vec3::new(x, y, z) })
}

#[pyfunction]
pub fn continentality_above_shore(seed: u64, land_fraction: f64, x: f64, y: f64, z: f64) -> f64 {
    let c = cached_continentality(seed, crate::sphere::EARTH_RADIUS_M, land_fraction);
    c.above_shore(&SpherePoint { vector: Vec3::new(x, y, z) })
}

#[pyfunction]
pub fn continentality_base_elevation(seed: u64, land_fraction: f64, x: f64, y: f64, z: f64) -> f64 {
    let c = cached_continentality(seed, crate::sphere::EARTH_RADIUS_M, land_fraction);
    c.base_elevation(&SpherePoint { vector: Vec3::new(x, y, z) })
}

/// Rebuilds a `PlateSet` from a flat list of seed components.
///
/// Each plate gets its position in the list as `index`, and a placeholder Euler pole and
/// rate (its own seed, and zero). Neither field is read by `PlateSet::new` or
/// `nearest_two` -- only `seed.vector` is -- so the placeholder cannot affect either
/// function under test; that is a property of the Rust source, checked by reading
/// `PlateSet::new` and `nearest_two` in `plates.rs`, not assumed.
fn plateset_from_seeds(seeds_flat: &[f64]) -> PlateSet {
    let plates = seeds_flat
        .chunks_exact(3)
        .enumerate()
        .map(|(index, chunk)| {
            let vector = Vec3::new(chunk[0], chunk[1], chunk[2]);
            let seed = SpherePoint { vector };
            Plate { index, seed, euler_pole: seed, rate_rad_per_myr: 0.0 }
        })
        .collect();
    PlateSet::new(plates)
}

#[pyfunction]
pub fn plate_angular_velocity(pole_x: f64, pole_y: f64, pole_z: f64, rate: f64) -> (f64, f64, f64) {
    let euler_pole = SpherePoint { vector: Vec3::new(pole_x, pole_y, pole_z) };
    // The seed is irrelevant to `angular_velocity`, which reads only the pole and rate.
    let plate = Plate { index: 0, seed: euler_pole, euler_pole, rate_rad_per_myr: rate };
    let omega = plate.angular_velocity();
    (omega.x, omega.y, omega.z)
}

#[pyfunction]
pub fn plateset_bisector(seeds_flat: Vec<f64>, a: usize, b: usize) -> Option<(f64, f64, f64)> {
    let set = plateset_from_seeds(&seeds_flat);
    set.bisector(a, b).map(|v| (v.x, v.y, v.z))
}

#[pyfunction]
pub fn plateset_nearest_two(seeds_flat: Vec<f64>, x: f64, y: f64, z: f64) -> (Option<usize>, Option<usize>) {
    let set = plateset_from_seeds(&seeds_flat);
    let point = SpherePoint { vector: Vec3::new(x, y, z) };
    let (best, second) = set.nearest_two(&point);
    (best.map(|p| p.index), second.map(|p| p.index))
}

#[pyfunction]
#[allow(clippy::too_many_arguments)]
pub fn continentality_gradient(
    seed: u64,
    land_fraction: f64,
    radius_m: f64,
    x: f64,
    y: f64,
    z: f64,
) -> (f64, f64) {
    let c = cached_continentality(seed, radius_m, land_fraction);
    let g = c.gradient(&SpherePoint { vector: Vec3::new(x, y, z) });
    (g.east, g.north)
}
