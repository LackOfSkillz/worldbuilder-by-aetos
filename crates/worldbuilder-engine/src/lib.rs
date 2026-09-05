//! The Worldbuilder generator core.
//!
//! One implementation, compiled twice: natively for Evennia and maritime through Python
//! bindings, and to WebAssembly for the browser studio. Slice 0 measured that those two
//! targets agree bit-for-bit, which is the only reason a studio and a game can be trusted
//! to be looking at the same world.

pub mod detmath;
pub mod generation;
pub mod vectors;
pub mod sphere;
pub mod bindings;
pub mod noise;
pub mod tangent;
pub mod continentality;
pub mod plates;
pub mod kinematics;
pub mod tectonics;

use pyo3::prelude::*;

/// The engine's own version, so a caller can tell which core answered.
#[pyfunction]
fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[pymodule]
fn worldbuilder_engine(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::vec3_length, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::vec3_cross, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::vec3_normalised, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::sphere_from_latlon, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::sphere_to_latlon, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::sphere_angle_to, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::sphere_distance_to, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::noise_at, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::noise_fbm, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::frame_at, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::frame_local_to_sphere, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::frame_sphere_to_local, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::continentality_calibration, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::continentality_at, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::continentality_above_shore, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::continentality_base_elevation, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::continentality_gradient, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::plate_angular_velocity, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::plateset_bisector, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::plateset_nearest_two, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::plateset_margin_at, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::plateset_margin_normal, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::plateset_flattened, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::plateset_margins_within, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::plate_surface_velocity, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::plates_motion_between, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::plateset_motion_at, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::tectonics_bump, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::tectonics_continental, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::tectonics_setting_at, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::tectonics_offset_m, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::tectonics_elevation_m, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::generation_fraction, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::generation_spread, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::generation_pole, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::generation_rate, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::generation_plates_for, m)?)?;
    Ok(())
}
