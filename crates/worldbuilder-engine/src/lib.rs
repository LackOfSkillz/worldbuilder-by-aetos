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
#[cfg(feature = "python")]
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
pub mod stream;
pub mod streamfmt;

#[cfg(feature = "python")]
use pyo3::prelude::*;

/// The engine's own version, so a caller can tell which core answered.
#[cfg(feature = "python")]
#[pyfunction]
fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// The generator's identity, per VERSION-001 (`docs/design/2026-09-02-mark-2-world-studio.md`
/// §6): "Worldbuilder never silently evaluates a worldfile with a generator other than the
/// one it declares." A worldfile (slice 3) will declare the `GENERATOR_VERSION` it was
/// created under; the format header (`streamfmt`, a later task in this slice) carries it;
/// a reader refuses to evaluate a worldfile whose declared version it does not recognise
/// rather than guess.
///
/// **This is not `CARGO_PKG_VERSION` and must never be derived from it.** The crate
/// version bumps for anything -- a docs fix, a new Python binding, a dependency patch --
/// none of which changes what a seed produces. `GENERATOR_VERSION` must stay fixed across
/// every one of those, or the whole point of VERSION-001 (a version a caller can trust) is
/// lost to noise on the first package-version bump.
///
/// A bare integer, not a semantic triple: VERSION-001 draws no distinction between a
/// "major" and a "minor" incompatibility, because it recognises none -- an unsupported
/// version is refused outright, never negotiated or partially honoured (no version
/// negotiation, no compatibility matrix is explicitly out of scope here). A triple would
/// invite exactly that negotiation. An integer says only "same" or "different", which is
/// all the invariant asks for.
///
/// # The bump test
///
/// Bump `GENERATOR_VERSION` when, and only when, this is true:
///
/// > The same seed and the same parameters, run through the new code, would produce a
/// > different world than they did before.
///
/// Not "did the code change" -- most changes here (formatting, comments, adding a dead
/// branch, widening an internal cache, exposing a new binding, adding a *new* generator
/// stage that is off unless explicitly requested) leave every existing world reproducible
/// and do not bump it. Bump it for: changing an existing stage's math (a different noise
/// octave count, a changed continentality curve, a different plate-boundary formula),
/// changing evaluation order where order affects output, changing a constant that feeds
/// generation, or -- the case that motivated this slice -- changing the data model in a
/// way that changes what a fixed seed evaluates to (CORE-001's own worry: "retrofitting a
/// graph into an engine built only for scalar fields" would be exactly such a change, since
/// the graph is derived *from* the surface the seed already produced).
///
/// A different kind of version lives beside this one and must stay separate from it: the
/// on-disk *format* version (the stream-graph file's section-table layout) bumps when the
/// file's byte layout changes, not when a world changes. Appending a new optional section
/// to that format is a format-version bump and **not** a generator-version bump --
/// conflating the two would mean every future format extension silently forces every
/// existing worldfile through a `GENERATOR_VERSION` migration it does not need. See
/// `.superpowers/sdd/notes/2026-09-04-core-001-extraction.md` §5.1 for the third version
/// (worldfile schema) this project also keeps distinct from both.
pub const GENERATOR_VERSION: u32 = 1;

#[cfg(feature = "python")]
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
    m.add_function(wrap_pyfunction!(bindings::surface_fields, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::surface_structural_m, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::surface_elevation_m, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::surface_bottom_at, m)?)?;
    // The refusal has to be reachable as a TYPE from Python, not merely raisable: a test
    // that catches `KeyError` alone cannot tell the port's refusal from an unrelated dict
    // miss inside the binding, and the whole point of this one is that both languages
    // decline to answer at the same input.
    m.add("UnknownSubstrateError", m.py().get_type_bound::<bindings::UnknownSubstrateError>())?;
    Ok(())
}

#[cfg(test)]
mod generator_version_tests {
    use super::GENERATOR_VERSION;

    /// VERSION-001 needs an identity that does not move when the package version does --
    /// the package version bumps for a docs fix, which must never look like a generator
    /// change to anything that reads a worldfile.
    #[test]
    fn generator_version_is_not_the_package_version() {
        assert_ne!(
            GENERATOR_VERSION.to_string(),
            env!("CARGO_PKG_VERSION"),
            "GENERATOR_VERSION must never be derived from CARGO_PKG_VERSION"
        );
    }

    /// Pins the current value so an accidental bump (or accidental non-bump) shows up as
    /// a diff a reviewer has to explain, per the bump test documented on the constant.
    #[test]
    fn generator_version_is_pinned() {
        assert_eq!(GENERATOR_VERSION, 1);
    }
}
