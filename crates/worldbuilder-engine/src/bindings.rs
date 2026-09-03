//! PyO3 surface. Conversion only — no arithmetic lives here, so the maths modules stay
//! usable from a plain Rust or WASM build with no Python anywhere in the picture.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use pyo3::prelude::*;

use crate::continentality::Continentality;
use crate::generation;
use crate::generation::Part;
use crate::kinematics::{motion_at, motion_between, surface_velocity};
use crate::plates::{Plate, PlateSet};
use crate::shelf::{Coastal, Shelf};
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

/// `_fraction(world_seed, *parts)` -- Python's variadic mixed int/str arguments become a
/// flat `Vec<String>` here, the same flattening convention every other binding in this
/// file uses for structured input (a `PlateSet` as flat `seeds_flat`/`poles_flat`/`rates`
/// lists, a point as three floats). `generation::Part` distinguishes `Int` from `Str` only
/// for its `Display` impl, and an integer part's `Display` output is exactly its decimal
/// string, so a caller-supplied string is indistinguishable from the integer it names --
/// `str(7)` and `"7"` hash identically. Every part becomes `Part::Str` here; conversion
/// only, no arithmetic.
#[pyfunction]
pub fn generation_fraction(world_seed: i64, label_parts: Vec<String>) -> f64 {
    let parts: Vec<Part> = label_parts.iter().map(|s| Part::Str(s.as_str())).collect();
    generation::fraction(world_seed, &parts)
}

#[pyfunction]
pub fn generation_spread(world_seed: i64, index: usize, count: usize) -> (f64, f64, f64) {
    let p = generation::spread(world_seed, index, count).vector;
    (p.x, p.y, p.z)
}

#[pyfunction]
pub fn generation_pole(world_seed: i64, index: usize) -> (f64, f64, f64) {
    let p = generation::pole(world_seed, index).vector;
    (p.x, p.y, p.z)
}

#[pyfunction]
pub fn generation_rate(world_seed: i64, index: usize) -> f64 {
    generation::rate(world_seed, index)
}

/// Every plate on a world, as `(index, seed_xyz, pole_xyz, rate)` tuples in index order.
/// Conversion only: `generation::plates_for` does all the arithmetic; this just unwraps
/// each `Plate`'s two `SpherePoint`s into `(f64, f64, f64)` triples.
#[pyfunction]
pub fn generation_plates_for(
    world_seed: i64,
    count: usize,
) -> Vec<(usize, (f64, f64, f64), (f64, f64, f64), f64)> {
    generation::plates_for(world_seed, count)
        .plates()
        .iter()
        .map(|plate| {
            let seed = plate.seed.vector;
            let pole = plate.euler_pole.vector;
            (plate.index, (seed.x, seed.y, seed.z), (pole.x, pole.y, pole.z), plate.rate_rad_per_myr)
        })
        .collect()
}

#[pyfunction]
pub fn detail_smooth(fraction: f64) -> f64 {
    crate::detail::smooth(fraction)
}

/// The band table for a world/radius, as `(wavelength_m, frequency, share)` tuples,
/// coarsest first. Conversion only: `Detail::plan` (run inside `Detail::new`) does all the
/// arithmetic; this just unwraps each `Band` into a triple. `Detail::new` is cheap -- no
/// calibration, unlike `Continentality` -- so there is no cache here, unlike
/// `cached_continentality` above.
#[pyfunction]
pub fn detail_bands(world_seed: u64, radius_m: f64) -> Vec<(f64, f64, f64)> {
    crate::detail::Detail::new(world_seed, radius_m)
        .bands()
        .iter()
        .map(|b| (b.wavelength_m, b.frequency, b.share))
        .collect()
}

#[pyfunction]
#[allow(clippy::too_many_arguments)]
pub fn detail_amplitude_m(
    world_seed: u64,
    radius_m: f64,
    x: f64,
    y: f64,
    z: f64,
    elevation_m: f64,
    shelf_weight: f64,
    tectonic_m: f64,
) -> f64 {
    let detail = crate::detail::Detail::new(world_seed, radius_m);
    let point = SpherePoint { vector: Vec3::new(x, y, z) };
    detail.amplitude_m(&point, elevation_m, shelf_weight, tectonic_m)
}

/// `resolution_m` defaults to `None` so a caller can omit it entirely, exactly as the
/// Python's `resolution_m=None` default allows -- and `Some(0.0)` is passed straight
/// through to `Detail::offset_m`, which is itself responsible for collapsing `0.0` (and
/// `-0.0`) onto the same canonical path as `None`, matching Python's `if resolution_m:`
/// falsiness. No special-casing belongs here: that would duplicate logic the maths module
/// already owns.
#[pyfunction]
#[pyo3(signature = (world_seed, radius_m, x, y, z, amplitude_m, resolution_m=None))]
#[allow(clippy::too_many_arguments)]
pub fn detail_offset_m(
    world_seed: u64,
    radius_m: f64,
    x: f64,
    y: f64,
    z: f64,
    amplitude_m: f64,
    resolution_m: Option<f64>,
) -> f64 {
    let detail = crate::detail::Detail::new(world_seed, radius_m);
    let point = SpherePoint { vector: Vec3::new(x, y, z) };
    detail.offset_m(&point, amplitude_m, resolution_m)
}

/// Builds the `Shelf` a binding needs: a `Tectonics` from the same flat seeds/poles/rates
/// convention as every `tectonics_*` binding, plus a second, independent draw on the
/// `Continentality` cache for `Shelf`'s own `land` field. `Tectonics` does not expose its
/// internal `Continentality` (no accessor exists), so this asks `cached_continentality`
/// again with the same key rather than trying to extract one from the `Tectonics` just
/// built -- the cache makes the second call free, and both draws are guaranteed identical
/// because the key (seed, radius_m bits, land_fraction bits) is the same. Conversion only.
fn shelf_from_parts(
    seeds_flat: &[f64],
    poles_flat: &[f64],
    rates: &[f64],
    continentality_seed: u64,
    land_fraction: f64,
    radius_m: f64,
) -> Shelf {
    let tectonics =
        tectonics_from_parts(seeds_flat, poles_flat, rates, continentality_seed, land_fraction, radius_m);
    let land = cached_continentality(continentality_seed, radius_m, land_fraction);
    Shelf::new(tectonics, land, radius_m)
}

/// A `Shelf` for `shelf_target_depth_m` alone, which -- read from `shelf.rs` -- never
/// touches `self.tectonics`, `self.land` or `self.radius_m`; its whole computation is a
/// function of the `Coastal` argument. So the plates and continentality behind this
/// instance are arbitrary filler to satisfy the type, not live inputs: an empty
/// `PlateSet` and a fixed, cached `Continentality` (seed 0, the module's own
/// `LAND_FRACTION`), never varied and never read by the method this binding calls.
fn dummy_shelf(radius_m: f64) -> Shelf {
    let land = cached_continentality(0, radius_m, crate::continentality::LAND_FRACTION);
    let tectonics = Tectonics::new(PlateSet::new(Vec::new()), land, radius_m);
    Shelf::new(tectonics, land, radius_m)
}

/// Where the shore is from here, or `None` exactly where `Shelf::coastal` returns `None`
/// -- positionally, not merely where both sides happen to agree. Conversion only:
/// `Shelf::coastal` does all the arithmetic (including the one indirect `hypot`, inside
/// `Gradient::magnitude`, that this module reaches); this just unwraps `Coastal` into a
/// pair.
#[pyfunction]
#[allow(clippy::too_many_arguments)]
pub fn shelf_coastal(
    seeds_flat: Vec<f64>,
    poles_flat: Vec<f64>,
    rates: Vec<f64>,
    continentality_seed: u64,
    land_fraction: f64,
    x: f64,
    y: f64,
    z: f64,
    radius_m: f64,
) -> Option<(f64, f64)> {
    let shelf = shelf_from_parts(&seeds_flat, &poles_flat, &rates, continentality_seed, land_fraction, radius_m);
    let point = SpherePoint { vector: Vec3::new(x, y, z) };
    shelf.coastal(&point).map(|c| (c.distance_m, c.breadth))
}

/// What the water ought to be doing at `distance_m`/`breadth`. Conversion only, and no
/// live `Shelf` state is needed to call it -- see `dummy_shelf` above.
#[pyfunction]
pub fn shelf_target_depth_m(distance_m: f64, breadth: f64) -> f64 {
    let shelf = dummy_shelf(crate::sphere::EARTH_RADIUS_M);
    shelf.target_depth_m(&Coastal { distance_m, breadth })
}

/// How much say the shelf has here. Conversion only: `Shelf::weight` does all the
/// arithmetic. `tectonic_m` is threaded through as the `Option<f64>` PyO3 hands back from
/// Python's `None`/a real float -- not flattened or defaulted here -- so `Some(0.0)`
/// reaches `Shelf::weight` as a supplied zero, exactly as it would coming from Python's
/// `tectonic_m=0.0`, and only Python's real `None` reaches it as `None`, taking the
/// `self.tectonics.offset_m(point)` recompute branch. Flattening `Option` on the way
/// through (e.g. `tectonic_m.unwrap_or(0.0)`) would be the previous slice's trap in
/// reverse: it would make a supplied zero and an absent value indistinguishable.
#[pyfunction]
#[pyo3(signature = (
    seeds_flat, poles_flat, rates, continentality_seed, land_fraction,
    x, y, z, distance_m, breadth, radius_m, tectonic_m=None,
))]
#[allow(clippy::too_many_arguments)]
pub fn shelf_weight(
    seeds_flat: Vec<f64>,
    poles_flat: Vec<f64>,
    rates: Vec<f64>,
    continentality_seed: u64,
    land_fraction: f64,
    x: f64,
    y: f64,
    z: f64,
    distance_m: f64,
    breadth: f64,
    radius_m: f64,
    tectonic_m: Option<f64>,
) -> f64 {
    let shelf = shelf_from_parts(&seeds_flat, &poles_flat, &rates, continentality_seed, land_fraction, radius_m);
    let point = SpherePoint { vector: Vec3::new(x, y, z) };
    let coastal = Coastal { distance_m, breadth };
    shelf.weight(&point, &coastal, tectonic_m)
}

/// The ground here, and the working that produced it, as `(elevation_m, weight,
/// tectonic_m)`. Conversion only: `Shelf::evaluate` does all the arithmetic; this just
/// unwraps its `Reading` into a triple, positionally.
#[pyfunction]
#[allow(clippy::too_many_arguments)]
pub fn shelf_evaluate(
    seeds_flat: Vec<f64>,
    poles_flat: Vec<f64>,
    rates: Vec<f64>,
    continentality_seed: u64,
    land_fraction: f64,
    x: f64,
    y: f64,
    z: f64,
    radius_m: f64,
) -> (f64, f64, f64) {
    let shelf = shelf_from_parts(&seeds_flat, &poles_flat, &rates, continentality_seed, land_fraction, radius_m);
    let point = SpherePoint { vector: Vec3::new(x, y, z) };
    let reading = shelf.evaluate(&point);
    (reading.elevation_m, reading.weight, reading.tectonic_m)
}

// --- features: bump, Feature.reach_m, Placed.weight_at, Features.apply/marks_near -------
//
// Conversion only, as everywhere else in this file. The one shape decision worth naming:
// a `Feature` is not all-f64 (it carries two strings, a bool and an `Option<String>`
// sentinel), so the flat-`Vec<f64>` idiom the `PlateSet` bindings use does not fit it.
// Each feature crosses as one positional tuple in field order instead --
// `(kind, x, y, z, target_m, length_m, width_m, bearing_deg, compose, marked, substrate)`
// -- which keeps `substrate`'s `None` distinguishable from an empty string, exactly as
// `features.py` requires of that sentinel.

/// One `Feature` as PyO3 hands it over: `at` flattened to its three vector components,
/// everything else in `features.py`'s declaration order.
type FeatureTuple = (String, f64, f64, f64, f64, f64, f64, f64, String, bool, Option<String>);

fn feature_from_tuple(t: &FeatureTuple) -> crate::features::Feature {
    let (kind, x, y, z, target_m, length_m, width_m, bearing_deg, compose, marked, substrate) = t;
    crate::features::Feature {
        kind: kind.clone(),
        at: SpherePoint { vector: Vec3::new(*x, *y, *z) },
        target_m: *target_m,
        length_m: *length_m,
        width_m: *width_m,
        bearing_deg: *bearing_deg,
        compose: compose.clone(),
        marked: *marked,
        // `.clone()` on the `Option`, never `.unwrap_or_default()`: `None` here means
        // "derive the bottom from the ground" and an empty string does not mean the same
        // thing, so the sentinel has to survive the crossing intact.
        substrate: substrate.clone(),
    }
}

fn features_from_tuples(features: &[FeatureTuple], radius_m: f64) -> crate::features::Features {
    crate::features::Features::new(features.iter().map(feature_from_tuple), radius_m)
}

/// The module's three compose names and `SETTLE_M`, so the Python side can assert it is
/// comparing against the same constants rather than its own copies of the literals.
#[pyfunction]
pub fn features_constants() -> (&'static str, &'static str, &'static str, f64) {
    (
        crate::features::RAISE,
        crate::features::CARVE,
        crate::features::SHAPE,
        crate::features::SETTLE_M,
    )
}

/// `_bump(distance_m, half_m)`. Conversion only.
#[pyfunction]
pub fn features_bump(distance_m: f64, half_m: f64) -> f64 {
    crate::features::bump(distance_m, half_m)
}

/// `Feature.reach_m()`. Conversion only -- `hypot` of the two extents, nothing else, so
/// the whole feature need not cross for it.
#[pyfunction]
pub fn features_reach_m(length_m: f64, width_m: f64) -> f64 {
    crate::features::Feature {
        kind: String::new(),
        at: SpherePoint { vector: Vec3::new(0.0, 0.0, 1.0) },
        target_m: 0.0,
        length_m,
        width_m,
        bearing_deg: 0.0,
        compose: crate::features::RAISE.to_string(),
        marked: false,
        substrate: None,
    }
    .reach_m()
}

/// `Placed(feature, radius_m).weight_at(point)`. Conversion only: the `Placed` is built
/// here per call (as `Features::new` would build it) and `weight_at` does the arithmetic,
/// reach gate included.
#[pyfunction]
pub fn features_weight_at(
    feature: FeatureTuple,
    x: f64,
    y: f64,
    z: f64,
    radius_m: f64,
) -> f64 {
    let placed = crate::features::Placed::new(feature_from_tuple(&feature), radius_m);
    placed.weight_at(&SpherePoint { vector: Vec3::new(x, y, z) })
}

/// `Features(features, radius_m).apply(point, elevation_m)`.
///
/// Returns `(shaped_metres, authority)` in that order -- an **absolute elevation** first
/// and a nothing-to-one blend weight second. They are not interchangeable and the tuple
/// order is the Python's, not a convenience.
#[pyfunction]
#[allow(clippy::too_many_arguments)]
pub fn features_apply(
    features: Vec<FeatureTuple>,
    x: f64,
    y: f64,
    z: f64,
    elevation_m: f64,
    radius_m: f64,
) -> (f64, f64) {
    let built = features_from_tuples(&features, radius_m);
    built.apply(&SpherePoint { vector: Vec3::new(x, y, z) }, elevation_m)
}

/// `Features(features, radius_m).marks_near(point, within_m)`, as `(distance_m, index)`
/// pairs nearest first.
///
/// `marks_near` returns borrowed `&Feature`s. Cloning each one back across the boundary
/// would hand Python a *copy* whose identity says nothing, and two features may share a
/// `kind`, so the mark would no longer name which of the placed features it is. The
/// index into the list as given is that identity, and it also makes the stable-sort
/// question testable: a tie between two distances must come back in construction order,
/// which is only observable if the two are told apart. Recovered by pointer identity
/// against `built.placed` (the borrow points into it), which is O(n^2) over a list the
/// module's own docs cap at "a dozen, not a million".
///
/// **The shared-`kind` hazard is a fixture, not a story.** It was a story until this was
/// written: every conformance fixture used distinct kinds, and the Python-side helper
/// mapped the returned index straight back to `features[index].kind`, so substituting
/// `placed.feature.kind == feature.kind` for the `ptr::eq` below passed both suites
/// outright -- `Vec::position` would then answer index 0 for every mark of a repeated
/// kind and nothing would notice. `test_features_marks_near_names_which_feature_when
/// _several_share_a_kind` places five rocks all called `rock` in an order that is not
/// their distance order and compares raw indices, so the substitution now fails. `kind` is
/// documented as a chart symbol class, and symbol classes repeat.
#[pyfunction]
pub fn features_marks_near(
    features: Vec<FeatureTuple>,
    x: f64,
    y: f64,
    z: f64,
    within_m: f64,
    radius_m: f64,
) -> Vec<(f64, usize)> {
    let built = features_from_tuples(&features, radius_m);
    let marks = built.marks_near(&SpherePoint { vector: Vec3::new(x, y, z) }, within_m);
    marks
        .into_iter()
        .map(|(distance_m, feature)| {
            let index = built
                .placed
                .iter()
                .position(|placed| std::ptr::eq(&placed.feature, feature))
                .expect("marks_near borrows from this same Features");
            (distance_m, index)
        })
        .collect()
}

/// `len(Features(...))`, and the `kind` and `substrate` of each feature in `__iter__`
/// order.
///
/// Two things are observable here that are observable nowhere else in this surface.
/// Construction order, because order is semantic in this module -- and `substrate`,
/// because **nothing inside the crate reads that field yet**: `substrate.py` is not
/// ported, so `Features::apply` and `marks_near` are both indifferent to it. Without this
/// round trip, a binding that flattened the sentinel (`substrate.clone().unwrap_or_default()`,
/// turning Python's `None` into `""`) would be undetectable by any test in the conformance
/// suite, and the field would silently stop meaning "derive the bottom from the shape of
/// the ground" at the moment `substrate.py` arrived to depend on it.
#[pyfunction]
pub fn features_round_trip(
    features: Vec<FeatureTuple>,
    radius_m: f64,
) -> (usize, Vec<String>, Vec<Option<String>>) {
    let built = features_from_tuples(&features, radius_m);
    (
        built.len(),
        built.iter().map(|feature| feature.kind.clone()).collect(),
        built.iter().map(|feature| feature.substrate.clone()).collect(),
    )
}

// --- substrate: constants, Composition, natural, slope_at, at, dominant_at ------------
//
// Conversion only, as everywhere else in this file, with one shape decision that is not
// like anything above it: `slope_at` and `at` take the HOST'S `structural_m` (and, for
// `at`, `tectonics.offset_m`) as PYTHON CALLABLES and call back into Python through them.
//
// **That is the whole point of these two bindings and it is not a convenience.**
// Substituting the port's own elevation field here would measure `shelf.rs` and
// `features.rs` at the same time as `slope_at`, and the sum of the three cannot be
// attributed to any one of them. It is not a small effect: `slope_at` driven by the
// port's own `structural_m` moves by up to 7.968304e-11 relative, against a
// `SLOPE_DRIFT_REL` of 2.3e-16 -- a factor of 3.46e5, five orders of magnitude, not a
// widened tolerance. Driven by the SAME field on both sides it is one ULP. So the field
// crosses the boundary as a callable, and the comparison isolates the function under test.
//
// The callables take three floats (a unit vector's components) rather than a
// `SpherePoint`, so no Python type has to be constructed or imported on this side; the
// test-side wrapper adapts.

/// Carries a `PyErr` out of a `&dyn Fn(&SpherePoint) -> f64` callback, which has nowhere
/// to put one.
///
/// A failing Python callback returns `0.0` into the arithmetic and stashes its error here;
/// the binding re-raises after the engine call returns, so the value the engine computed
/// from that `0.0` is discarded and never reaches Python. The FIRST error is kept, not the
/// last -- a `structural_m` that raises on its first probe would otherwise be reported
/// through whichever later probe also failed.
struct CallbackError {
    error: std::cell::RefCell<Option<PyErr>>,
}

impl CallbackError {
    fn new() -> Self {
        CallbackError { error: std::cell::RefCell::new(None) }
    }

    fn note(&self, error: PyErr) {
        let mut slot = self.error.borrow_mut();
        if slot.is_none() {
            *slot = Some(error);
        }
    }

    fn into_result<T>(self, value: T) -> PyResult<T> {
        match self.error.into_inner() {
            Some(error) => Err(error),
            None => Ok(value),
        }
    }
}

/// Wraps a Python callable as the `&dyn Fn(&SpherePoint) -> f64` the engine wants.
///
/// The call is positional and flat -- `f(x, y, z)` -- which keeps every Python type out of
/// this file. Probe ORDER is preserved by construction, because the engine calls this
/// closure exactly where the Python calls its own `self.surface.structural_m`, so a
/// recording callable on the Python side sees the same sequence from both languages. That
/// is what makes `at`'s elevation/tectonic/slope resolution order observable across the
/// FFI rather than only from inside the crate.
fn host_field<'py>(
    callable: &'py Bound<'py, PyAny>,
    errors: &'py CallbackError,
) -> impl Fn(&SpherePoint) -> f64 + 'py {
    move |point: &SpherePoint| {
        let v = &point.vector;
        match callable.call1((v.x, v.y, v.z)).and_then(|value| value.extract::<f64>()) {
            Ok(value) => value,
            Err(error) => {
                errors.note(error);
                0.0
            }
        }
    }
}

pyo3::create_exception!(
    worldbuilder_engine,
    UnknownSubstrateError,
    pyo3::exceptions::PyKeyError,
    "A placed feature declared a substrate `PURE` has no entry for -- where Python's \
     `PURE[declared]` raises `KeyError`. Subclasses `KeyError` so a caller catching what \
     the Python raises catches this too, and is its own type so a test can tell the \
     port's refusal from an unrelated dict miss."
);

/// The module's three names and its five numeric constants, plus `SLOPE_DRIFT_REL`, so
/// the Python side asserts against the crate's own literals rather than its own copies.
///
/// `SLOPE_DRIFT_REL` is exported here for ONE purpose: so the conformance suite can assert
/// the bound it uses is the bound the engine documents, and fail if either moves. It is
/// not a licence to apply it anywhere `slope_at` is not the only thing being measured.
#[pyfunction]
pub fn substrate_constants(
) -> (&'static str, &'static str, &'static str, f64, f64, f64, f64, f64, f64) {
    (
        crate::substrate::SAND,
        crate::substrate::MUD,
        crate::substrate::ROCK,
        crate::substrate::ROCK_SLOPE,
        crate::substrate::ROCK_TECTONIC_M,
        crate::substrate::SWEPT_M,
        crate::substrate::SETTLED_M,
        crate::substrate::SLOPE_BASELINE_M,
        crate::substrate::SLOPE_DRIFT_REL,
    )
}

/// `substrate.py`'s `_smooth`, reached through `substrate`'s own re-export rather than
/// through `detail`.
///
/// Deliberately a second binding onto what is deliberately one function. `detail_smooth`
/// already exposes `detail::smooth`, and `substrate.rs` reuses that same item by `pub use`
/// instead of transcribing a fourth copy -- so this binding is the assertion that the
/// re-export still resolves to a function agreeing with `substrate.py`'s `_smooth`. If
/// somebody ever gives `substrate` a private copy, the two Python modules' `_smooth`
/// functions can drift apart and this is where that would show.
#[pyfunction]
pub fn substrate_smooth(fraction: f64) -> f64 {
    crate::substrate::smooth(fraction)
}

/// `Composition(sand, mud, rock)` -- the normalising constructor, including its
/// `total <= 0.0` branch -- as the three normalised fractions, plus `dominant` and
/// `holding` off the same instance.
///
/// All three answers come from ONE constructed `Composition`, not from three
/// reconstructions, because `dominant` reads the fractions AFTER normalisation and a
/// binding that renormalised per answer could disagree with itself.
#[pyfunction]
pub fn substrate_composition(
    sand: f64,
    mud: f64,
    rock: f64,
) -> (f64, f64, f64, &'static str, f64) {
    let c = crate::substrate::Composition::new(sand, mud, rock);
    (c.sand, c.mud, c.rock, c.dominant(), c.holding())
}

/// `Composition(...).blended_towards(Composition(...), weight)`.
///
/// **NEITHER triple is passed through `Composition::new` on the way in, and an earlier
/// version of this binding that did was wrong by a measurable ULP.** In Python the
/// receiver and the target are both *existing instances*: their fields have already been
/// divided by their total once, and `blended_towards` reads them as they stand. Rebuilding
/// them here would normalise a second time -- and the fractions of a real composition do
/// NOT sum to exactly 1.0 (an exhaustive sweep of `natural`'s domain puts the total two
/// ULP below one), so the second division moves them. Measured on the demonstration
/// coast's own grid: `0.2781153660496104` against `0.27811536604961046`, in a comparison
/// that has no business needing a tolerance at all.
///
/// So the caller hands in the fields of two constructed compositions and this assembles
/// them verbatim. `substrate_composition` is where the normalising constructor is exposed.
#[pyfunction]
#[allow(clippy::too_many_arguments)]
pub fn substrate_blended_towards(
    sand: f64,
    mud: f64,
    rock: f64,
    other_sand: f64,
    other_mud: f64,
    other_rock: f64,
    weight: f64,
) -> (f64, f64, f64) {
    let c = crate::substrate::Composition { sand, mud, rock };
    let other = crate::substrate::Composition {
        sand: other_sand,
        mud: other_mud,
        rock: other_rock,
    };
    let blended = c.blended_towards(&other, weight);
    (blended.sand, blended.mud, blended.rock)
}

/// `PURE[kind]`, or `None` where the Python's dict lookup would raise `KeyError`.
///
/// Returns the miss rather than raising, because this binding is how the Python side
/// checks WHICH words the table has; `substrate_at` is where a miss becomes a refusal.
#[pyfunction]
pub fn substrate_pure(kind: &str) -> Option<(f64, f64, f64)> {
    crate::substrate::pure(kind).map(|c| (c.sand, c.mud, c.rock))
}

/// `Substrate.natural(elevation_m, slope, tectonic_m)`. Conversion only, and STRICT --
/// zero transcendentals in its path, so the Python side asserts raw bits.
#[pyfunction]
pub fn substrate_natural(elevation_m: f64, slope: f64, tectonic_m: f64) -> (f64, f64, f64) {
    let c = crate::substrate::natural(elevation_m, slope, tectonic_m);
    (c.sand, c.mud, c.rock)
}

/// `Substrate.slope_at(point, baseline_m)`, driven by the HOST'S `structural_m`.
///
/// `structural_m` is a Python callable taking `(x, y, z)`. The engine's four probes go
/// straight back through it, so this measures `TangentFrame::at`, `local_to_sphere` and
/// `hypot` against the Python's -- and nothing else. See the section note above for why
/// substituting the port's own elevation field would make the number unattributable.
#[pyfunction]
pub fn substrate_slope_at(
    radius_m: f64,
    x: f64,
    y: f64,
    z: f64,
    baseline_m: f64,
    structural_m: &Bound<'_, PyAny>,
) -> PyResult<f64> {
    let errors = CallbackError::new();
    let point = SpherePoint { vector: Vec3::new(x, y, z) };
    let slope = {
        let field = host_field(structural_m, &errors);
        crate::substrate::slope_at(radius_m, &point, baseline_m, &field)
    };
    errors.into_result(slope)
}

/// `Substrate.at(point, elevation_m=None, slope=None, tectonic_m=None)`.
///
/// **THE KEYWORD ORDER HERE IS PYTHON'S AND THE ENGINE'S IS NOT, AND THAT IS THE LIVE
/// TRAP IN THIS BINDING.** `substrate.py` declares `elevation_m, slope, tectonic_m`;
/// `substrate::at` takes `elevation_m, tectonic_m, slope`, in resolution order, because
/// Task 1 ruled the Rust signature must run in the order the body resolves so the two
/// cannot drift apart. All three are `Option<f64>`, so forwarding this binding's
/// arguments POSITIONALLY into the engine call -- `at(.., elevation_m, slope, tectonic_m,
/// ..)` -- COMPILES CLEANLY, type-checks, and is silently wrong: it feeds the slope in as
/// the tectonic contribution and the tectonic metres in as a dimensionless slope. Nothing
/// but a test can catch it. The mapping below is therefore by NAME at the call site, and
/// `test_substrate_at_maps_its_keywords_by_name_not_by_position` is what holds it there.
///
/// The two host members cross as Python callables for the reason given at the head of this
/// section, and each `None` left here reaches a DIFFERENT one of them, which is what makes
/// the resolution order observable from Python rather than only from inside the crate.
///
/// Raises `UnknownSubstrateError` (a `KeyError`) exactly where Python's `PURE[declared]`
/// raises -- a feature that declared a word the table has no entry for, at a point where
/// its weight is above zero.
#[pyfunction]
#[pyo3(signature = (
    features, radius_m, x, y, z, structural_m, tectonic_offset_m,
    elevation_m=None, slope=None, tectonic_m=None,
))]
#[allow(clippy::too_many_arguments)]
pub fn substrate_at(
    features: Vec<FeatureTuple>,
    radius_m: f64,
    x: f64,
    y: f64,
    z: f64,
    structural_m: &Bound<'_, PyAny>,
    tectonic_offset_m: &Bound<'_, PyAny>,
    elevation_m: Option<f64>,
    slope: Option<f64>,
    tectonic_m: Option<f64>,
) -> PyResult<(f64, f64, f64)> {
    let errors = CallbackError::new();
    let built = features_from_tuples(&features, radius_m);
    let point = SpherePoint { vector: Vec3::new(x, y, z) };
    let outcome = {
        let structural = host_field(structural_m, &errors);
        let tectonic = host_field(tectonic_offset_m, &errors);
        crate::substrate::at(
            radius_m,
            &point,
            // By name, never by position -- see the note above.
            elevation_m,
            tectonic_m,
            slope,
            &structural,
            &tectonic,
            &built,
        )
    };
    let composition = errors
        .into_result(outcome)?
        .map_err(|unknown| UnknownSubstrateError::new_err(unknown.to_string()))?;
    Ok((composition.sand, composition.mud, composition.rock))
}

/// `Substrate.dominant_at(point, **known)`. Same keyword shape and the same by-name
/// mapping as `substrate_at`, and the same refusal.
#[pyfunction]
#[pyo3(signature = (
    features, radius_m, x, y, z, structural_m, tectonic_offset_m,
    elevation_m=None, slope=None, tectonic_m=None,
))]
#[allow(clippy::too_many_arguments)]
pub fn substrate_dominant_at(
    features: Vec<FeatureTuple>,
    radius_m: f64,
    x: f64,
    y: f64,
    z: f64,
    structural_m: &Bound<'_, PyAny>,
    tectonic_offset_m: &Bound<'_, PyAny>,
    elevation_m: Option<f64>,
    slope: Option<f64>,
    tectonic_m: Option<f64>,
) -> PyResult<&'static str> {
    let errors = CallbackError::new();
    let built = features_from_tuples(&features, radius_m);
    let point = SpherePoint { vector: Vec3::new(x, y, z) };
    let outcome = {
        let structural = host_field(structural_m, &errors);
        let tectonic = host_field(tectonic_offset_m, &errors);
        crate::substrate::dominant_at(
            radius_m,
            &point,
            elevation_m,
            tectonic_m,
            slope,
            &structural,
            &tectonic,
            &built,
        )
    };
    errors
        .into_result(outcome)?
        .map_err(|unknown| UnknownSubstrateError::new_err(unknown.to_string()))
}
