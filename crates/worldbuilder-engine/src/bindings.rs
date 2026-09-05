//! PyO3 surface. Conversion only — no arithmetic lives here, so the maths modules stay
//! usable from a plain Rust or WASM build with no Python anywhere in the picture.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use pyo3::prelude::*;

use crate::continentality::Continentality;
use crate::kinematics::{motion_at, motion_between, surface_velocity};
use crate::plates::{Plate, PlateSet};
use crate::sphere::SpherePoint;
use crate::tectonics::Tectonics;
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

/// Nearest index, then every margin in range as `(other_index, distance_m, normal, weight)`.
/// Conversion only: `margins_within` does all the arithmetic; this unwraps its `Plate`
/// values into indices and its `Vec3` into a triple, positionally -- a `None` nearest on
/// the Rust side must come back as `None` here, not coerced into anything that could
/// compare equal to a real index by accident, and an empty list must come back empty
/// rather than padded with placeholders.
#[pyfunction]
#[allow(clippy::too_many_arguments)]
pub fn plateset_margins_within(
    seeds_flat: Vec<f64>,
    poles_flat: Vec<f64>,
    rates: Vec<f64>,
    x: f64,
    y: f64,
    z: f64,
    range_m: f64,
    radius_m: f64,
) -> (Option<usize>, Vec<(usize, f64, (f64, f64, f64), f64)>) {
    let set = plateset_from_parts(&seeds_flat, &poles_flat, &rates);
    let point = SpherePoint { vector: Vec3::new(x, y, z) };
    let (nearest, found) = set.margins_within(&point, range_m, radius_m);
    let margins = found
        .into_iter()
        .map(|m| (m.other.index, m.distance_m, (m.normal.x, m.normal.y, m.normal.z), m.weight))
        .collect();
    (nearest.map(|p| p.index), margins)
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

/// How fast one plate's ground is moving at a point. Conversion only: builds the `Plate`
/// from its pole and rate directly (the seed is irrelevant to `surface_velocity`, exactly
/// as in `plate_angular_velocity` above) and hands it to `surface_velocity`.
#[pyfunction]
#[allow(clippy::too_many_arguments)]
pub fn plate_surface_velocity(
    pole_x: f64,
    pole_y: f64,
    pole_z: f64,
    rate: f64,
    x: f64,
    y: f64,
    z: f64,
    radius_m: f64,
) -> (f64, f64, f64) {
    let euler_pole = SpherePoint { vector: Vec3::new(pole_x, pole_y, pole_z) };
    let plate = Plate { index: 0, seed: euler_pole, euler_pole, rate_rad_per_myr: rate };
    let point = SpherePoint { vector: Vec3::new(x, y, z) };
    let v = surface_velocity(&plate, &point, radius_m);
    (v.x, v.y, v.z)
}

/// What two named plates -- given directly by pole and rate, not looked up in a
/// `PlateSet` -- are doing to each other at a point. Conversion only: `motion_between`
/// does all the arithmetic; this just builds the two `Plate` values and the normal, and
/// unwraps the returned `Motion` into `(closing, sliding, kind)`.
#[pyfunction]
#[allow(clippy::too_many_arguments)]
pub fn plates_motion_between(
    near_pole_x: f64,
    near_pole_y: f64,
    near_pole_z: f64,
    near_rate: f64,
    far_pole_x: f64,
    far_pole_y: f64,
    far_pole_z: f64,
    far_rate: f64,
    x: f64,
    y: f64,
    z: f64,
    nx: f64,
    ny: f64,
    nz: f64,
    radius_m: f64,
) -> (f64, f64, &'static str) {
    let near_pole = SpherePoint { vector: Vec3::new(near_pole_x, near_pole_y, near_pole_z) };
    let near = Plate { index: 0, seed: near_pole, euler_pole: near_pole, rate_rad_per_myr: near_rate };
    let far_pole = SpherePoint { vector: Vec3::new(far_pole_x, far_pole_y, far_pole_z) };
    let far = Plate { index: 1, seed: far_pole, euler_pole: far_pole, rate_rad_per_myr: far_rate };
    let point = SpherePoint { vector: Vec3::new(x, y, z) };
    let normal = Vec3::new(nx, ny, nz);
    let motion = motion_between(&near, &far, &point, &normal, radius_m);
    (motion.closing_m_per_myr, motion.sliding_m_per_myr, motion.kind.as_str())
}

/// What is happening across the nearest plate edge, here -- the first binding that both
/// goes through `plateset_from_parts` AND reads poles and rates, since `motion_at` calls
/// `surface_velocity`, which calls `Plate::angular_velocity()`. Every prior binding onto
/// `plateset_from_parts` fed only functions that ignore `euler_pole` and `rate_rad_per_myr`
/// (see the comment on `plateset_from_parts` above), so this is the first one a fabricated
/// pole or zeroed rate would actually break.
///
/// `None` is returned, positionally, exactly where `motion_at` returns `None` -- not a
/// zeroed tuple -- so a caller cannot mistake "no margin here" for "a margin with no
/// motion".
#[pyfunction]
#[allow(clippy::too_many_arguments)]
pub fn plateset_motion_at(
    seeds_flat: Vec<f64>,
    poles_flat: Vec<f64>,
    rates: Vec<f64>,
    x: f64,
    y: f64,
    z: f64,
    radius_m: f64,
) -> Option<(usize, usize, f64, f64, f64, &'static str)> {
    let set = plateset_from_parts(&seeds_flat, &poles_flat, &rates);
    let point = SpherePoint { vector: Vec3::new(x, y, z) };
    let motion = motion_at(&point, &set, radius_m)?;
    let margin = motion.margin?;
    let nearest = margin.nearest?;
    let neighbour = margin.neighbour?;
    Some((
        nearest.index,
        neighbour.index,
        margin.distance_m,
        motion.closing_m_per_myr,
        motion.sliding_m_per_myr,
        motion.kind.as_str(),
    ))
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

/// The pure algebraic helper behind every profile in `tectonics.rs`. No `PlateSet`, no
/// `Continentality`, no arithmetic beyond what `tectonics::bump` itself does -- conversion
/// only.
#[pyfunction]
pub fn tectonics_bump(distance_m: f64, width_m: f64) -> f64 {
    crate::tectonics::bump(distance_m, width_m)
}

/// The other pure algebraic helper: nothing transcendental, a smoothstep on a clamped
/// fraction.
#[pyfunction]
pub fn tectonics_continental(value: f64) -> f64 {
    crate::tectonics::continental(value)
}

/// Builds the `Tectonics` a binding needs: a `PlateSet` from the same flat seeds/poles/
/// rates convention as every other `plateset_*` binding, and a `Continentality` from the
/// cache keyed the same way `continentality_*` bindings key it. Conversion only -- shared
/// by every `tectonics_*` binding below so each one only has to supply the arguments that
/// are actually its own.
fn tectonics_from_parts(
    seeds_flat: &[f64],
    poles_flat: &[f64],
    rates: &[f64],
    continentality_seed: u64,
    land_fraction: f64,
    radius_m: f64,
) -> Tectonics {
    let plates = plateset_from_parts(seeds_flat, poles_flat, rates);
    let land = cached_continentality(continentality_seed, radius_m, land_fraction);
    Tectonics::new(plates, land, radius_m)
}

/// `Setting.inboard`/`Setting.outboard` at a point, given a margin distance and normal.
/// `setting_at` itself only reads `self.land` and `self.radius_m`, never `self.plates`,
/// but building the `Tectonics` it hangs off still needs a `PlateSet` -- so this binding
/// takes the same full parameter set, in the same order, as
/// `tectonics_offset_m`/`tectonics_elevation_m` rather than inventing a narrower
/// convention just for this one call.
#[pyfunction]
#[allow(clippy::too_many_arguments)]
pub fn tectonics_setting_at(
    seeds_flat: Vec<f64>,
    poles_flat: Vec<f64>,
    rates: Vec<f64>,
    continentality_seed: u64,
    land_fraction: f64,
    x: f64,
    y: f64,
    z: f64,
    distance_m: f64,
    nx: f64,
    ny: f64,
    nz: f64,
    radius_m: f64,
) -> (f64, f64) {
    let tectonics =
        tectonics_from_parts(&seeds_flat, &poles_flat, &rates, continentality_seed, land_fraction, radius_m);
    let point = SpherePoint { vector: Vec3::new(x, y, z) };
    let normal = Vec3::new(nx, ny, nz);
    let setting = tectonics.setting_at(&point, distance_m, &normal);
    (setting.inboard, setting.outboard)
}

/// How much the plates raise or lower the ground at a point -- every margin in range,
/// summed. Conversion only: `Tectonics::offset_m` does all the arithmetic.
#[pyfunction]
#[allow(clippy::too_many_arguments)]
pub fn tectonics_offset_m(
    seeds_flat: Vec<f64>,
    poles_flat: Vec<f64>,
    rates: Vec<f64>,
    continentality_seed: u64,
    land_fraction: f64,
    x: f64,
    y: f64,
    z: f64,
    radius_m: f64,
) -> f64 {
    let tectonics =
        tectonics_from_parts(&seeds_flat, &poles_flat, &rates, continentality_seed, land_fraction, radius_m);
    let point = SpherePoint { vector: Vec3::new(x, y, z) };
    tectonics.offset_m(&point)
}

/// The macro elevation: continental base plus `offset_m`. Conversion only:
/// `Tectonics::elevation_m` does all the arithmetic.
#[pyfunction]
#[allow(clippy::too_many_arguments)]
pub fn tectonics_elevation_m(
    seeds_flat: Vec<f64>,
    poles_flat: Vec<f64>,
    rates: Vec<f64>,
    continentality_seed: u64,
    land_fraction: f64,
    x: f64,
    y: f64,
    z: f64,
    radius_m: f64,
) -> f64 {
    let tectonics =
        tectonics_from_parts(&seeds_flat, &poles_flat, &rates, continentality_seed, land_fraction, radius_m);
    let point = SpherePoint { vector: Vec3::new(x, y, z) };
    tectonics.elevation_m(&point)
}
