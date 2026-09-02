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

/// Rebuilds a `PlateSet` from three flat lists -- seeds, Euler poles, and rates -- so the
/// harness supplies real, independently-varying values for every field of `Plate` rather
/// than fabricating `euler_pole` and `rate_rad_per_myr` from the seed alone.
///
/// Each plate gets its position in the list as `index`. Checked by reading `plates.rs`,
/// not assumed: `bisector`, `nearest_two`, `margin_at`, `margin_normal` and `flattened`
/// read only `seed.vector` (and, for the two margin functions, positions in the bisector
/// table) -- none of them touch `euler_pole` or `rate_rad_per_myr`. Only
/// `Plate::angular_velocity()` reads those two fields, and it is not reachable through any
/// binding in this slice. So carrying real poles and rates through this function does not,
/// by itself, make today's conformance tests exercise a fabrication regression -- verified
/// by mutating this function back to `pole = seed`, `rate = 0.0` and observing all 44
/// conformance tests still pass. They are carried anyway because the binding contract
/// calls for the whole `Plate`, and because the kinematics slice will add a binding to
/// `angular_velocity`, where a fabricated pole or rate would be caught. That guard belongs
/// there, not here.
fn plateset_from_parts(seeds_flat: &[f64], poles_flat: &[f64], rates: &[f64]) -> PlateSet {
    let plates = seeds_flat
        .chunks_exact(3)
        .zip(poles_flat.chunks_exact(3))
        .zip(rates.iter())
        .enumerate()
        .map(|(index, ((seed, pole), rate))| Plate {
            index,
            seed: SpherePoint { vector: Vec3::new(seed[0], seed[1], seed[2]) },
            euler_pole: SpherePoint { vector: Vec3::new(pole[0], pole[1], pole[2]) },
            rate_rad_per_myr: *rate,
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
pub fn plateset_bisector(
    seeds_flat: Vec<f64>,
    poles_flat: Vec<f64>,
    rates: Vec<f64>,
    a: usize,
    b: usize,
) -> Option<(f64, f64, f64)> {
    let set = plateset_from_parts(&seeds_flat, &poles_flat, &rates);
    set.bisector(a, b).map(|v| (v.x, v.y, v.z))
}

#[pyfunction]
pub fn plateset_nearest_two(
    seeds_flat: Vec<f64>,
    poles_flat: Vec<f64>,
    rates: Vec<f64>,
    x: f64,
    y: f64,
    z: f64,
) -> (Option<usize>, Option<usize>) {
    let set = plateset_from_parts(&seeds_flat, &poles_flat, &rates);
    let point = SpherePoint { vector: Vec3::new(x, y, z) };
    let (best, second) = set.nearest_two(&point);
    (best.map(|p| p.index), second.map(|p| p.index))
}

/// Nearest index, neighbour index, distance in metres. Conversion only: `margin_at` does
/// all the arithmetic; this just unwraps the `Margin` it returns into a shape PyO3 can
/// hand back, positionally -- a `None` on the Rust side must come back as `None` here,
/// not be coerced into anything that could compare equal to a real index by accident.
#[pyfunction]
#[allow(clippy::too_many_arguments)]
pub fn plateset_margin_at(
    seeds_flat: Vec<f64>,
    poles_flat: Vec<f64>,
    rates: Vec<f64>,
    x: f64,
    y: f64,
    z: f64,
    radius_m: f64,
) -> (Option<usize>, Option<usize>, f64) {
    let set = plateset_from_parts(&seeds_flat, &poles_flat, &rates);
    let point = SpherePoint { vector: Vec3::new(x, y, z) };
    let margin = set.margin_at(&point, radius_m);
    (
        margin.nearest.map(|p| p.index),
        margin.neighbour.map(|p| p.index),
        margin.distance_m,
    )
}

#[pyfunction]
#[allow(clippy::too_many_arguments)]
pub fn plateset_margin_normal(
    seeds_flat: Vec<f64>,
    poles_flat: Vec<f64>,
    rates: Vec<f64>,
    x: f64,
    y: f64,
    z: f64,
    radius_m: f64,
) -> Option<(f64, f64, f64)> {
    let set = plateset_from_parts(&seeds_flat, &poles_flat, &rates);
    let point = SpherePoint { vector: Vec3::new(x, y, z) };
    let margin = set.margin_at(&point, radius_m);
    let normal = set.margin_normal(&point, &margin)?;
    Some((normal.x, normal.y, normal.z))
}

/// TEMPORARY -- slice 1g Task 1 measurement scaffolding only, see
/// `plates::margins_within_limit`. Deleted or replaced by Task 4/5.
#[pyfunction]
pub fn margins_within_limit(range_m: f64, radius_m: f64) -> f64 {
    crate::plates::margins_within_limit(range_m, radius_m)
}

#[pyfunction]
#[allow(clippy::too_many_arguments)]
pub fn plateset_flattened(
    seeds_flat: Vec<f64>,
    poles_flat: Vec<f64>,
    rates: Vec<f64>,
    x: f64,
    y: f64,
    z: f64,
    nx: f64,
    ny: f64,
    nz: f64,
) -> Option<(f64, f64, f64)> {
    let set = plateset_from_parts(&seeds_flat, &poles_flat, &rates);
    let point = SpherePoint { vector: Vec3::new(x, y, z) };
    let normal = Vec3::new(nx, ny, nz);
    let flat = set.flattened(&point, &normal)?;
    Some((flat.x, flat.y, flat.z))
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
