//! The Worldbuilder generator core.
//!
//! One implementation, compiled twice: natively for Evennia and maritime through Python
//! bindings, and to WebAssembly for the browser studio. Slice 0 measured that those two
//! targets agree bit-for-bit, which is the only reason a studio and a game can be trusted
//! to be looking at the same world.

pub mod detmath;
pub mod vectors;
pub mod sphere;
pub mod bindings;
pub mod noise;

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
    Ok(())
}
