//! The Worldbuilder generator core.
//!
//! One implementation, compiled twice: natively for Evennia and maritime through Python
//! bindings, and to WebAssembly for the browser studio. Slice 0 measured that those two
//! targets agree bit-for-bit, which is the only reason a studio and a game can be trusted
//! to be looking at the same world.

pub mod detail;
pub mod detmath;
pub mod features;
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
pub mod shelf;
pub mod substrate;
pub mod surface;

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
    m.add_function(wrap_pyfunction!(bindings::detail_smooth, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::detail_bands, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::detail_amplitude_m, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::detail_offset_m, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::shelf_coastal, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::shelf_target_depth_m, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::shelf_weight, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::shelf_evaluate, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::features_constants, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::features_bump, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::features_reach_m, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::features_weight_at, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::features_apply, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::features_marks_near, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::features_round_trip, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::substrate_constants, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::substrate_smooth, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::substrate_composition, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::substrate_blended_towards, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::substrate_pure, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::substrate_natural, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::substrate_slope_at, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::substrate_at, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::substrate_dominant_at, m)?)?;
    // The refusal has to be reachable as a TYPE from Python, not merely raisable: a test
    // that catches `KeyError` alone cannot tell the port's refusal from an unrelated dict
    // miss inside the binding, and the whole point of this one is that both languages
    // decline to answer at the same input.
    m.add("UnknownSubstrateError", m.py().get_type_bound::<bindings::UnknownSubstrateError>())?;
    Ok(())
}
