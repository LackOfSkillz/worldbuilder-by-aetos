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
#[cfg(feature = "wasm")]
pub mod wasm;

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

/// DELIBERATE BREAKAGE (slice CI, task 2). A fully-qualified `f64::` form the determinism
/// guard must reject. Nothing calls it; the guard is a text scan of src/, not a call graph.
/// It is ALONE on this branch: on the batched branch the changed constant failed the lib
/// target first, and `cargo test` never reached tests/no_std_math.rs.
#[allow(dead_code)]
fn ci_proof_of_a_red_gate(x: f64) -> f64 {
    f64::sqrt(x)
}

use crate::stream::StreamGraph;
use crate::surface::Surface;

/// A planet: the stateless field, and the drainage network built over it.
///
/// # Why the core holds two representations, and holds them from the start
///
/// This is the whole of CORE-001. `Surface` answers "how high is it here" from a seed
/// alone, for ever, at any point, without storing anything that resembles a map. A
/// drainage network cannot be answered that way: whether the water leaving a place reaches
/// the sea depends on every other place, so the answer has to be *built* over a node set
/// and then *held*. Those are two different kinds of object and no amount of care makes
/// them one.
///
/// **`Surface` is not modified, and that is the point rather than a restriction.** The
/// extraction's named worry was "retrofitting a graph into an engine built only for scalar
/// fields", which under VERSION-001 is not a refactor but a `GENERATOR_VERSION` bump --
/// every existing worldfile through a migration it did not ask for. Because the second
/// representation exists now, with its field list already fixed, slice 5 *populates* this
/// type rather than restructuring it: it fills `Lake::level_m` and `outflow_lake`, fills
/// `reaches`, sets `LAKE_MEMBER` on non-root members, and adds not one field to `Surface`.
///
/// # `Option`, and what the two states mean
///
/// `None` is not "not built yet" in a lazy sense -- nothing here builds one on demand. It
/// is the honest statement that this world has no drainage network, which is the state
/// every `Surface` is in until somebody samples nodes, evaluates heights and calls
/// `StreamGraph::build`. A studio that only wants elevation never pays for a graph, and a
/// worldfile that carries no stream section reads back into exactly this state.
///
/// # What is deliberately not here
///
/// No erosion, no stream power, no lake fill, no climate, no land cover. Slice 5 owns the
/// first three; the last two are designed and not approved. And no sea level: the datum is
/// a property of a *classification*, so it lives in `GraphHeader` where the classification
/// was made, and a `World` with no graph has no datum to hold.
/// **No `derive`s, because `Surface` has none and this task may not modify it.** That is
/// not an oversight in either type: adding `Clone` here would make a whole planet copyable
/// by accident, and `Debug` on a type holding twenty million nodes is a formatter nobody
/// should be one keystroke away from calling. Both are addable later without a schema
/// break, which is exactly the class of change this slice exists to make cheap.
pub struct World {
    surface: Surface,
    streams: Option<StreamGraph>,
}

/// Why a graph was refused as this world's drainage network.
///
/// The graph carries the seed and radius it was built for, so a graph built against a
/// different planet is detectable rather than merely wrong. **This is the check nothing
/// else can make**: `StreamGraph::build` never sees a `Surface`, and `Surface` never sees a
/// graph, so the only place the two can be held to the same planet is where they are held
/// together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldMismatch {
    Seed { surface: i64, graph: u64 },
    Radius { surface_bits: u64, graph_bits: u64 },
    Generator { world: u32, graph: u32 },
}

impl World {
    /// A world with no drainage network.
    pub fn new(surface: Surface) -> Self {
        World { surface, streams: None }
    }

    pub fn surface(&self) -> &Surface {
        &self.surface
    }

    /// The drainage network, if this world has one.
    pub fn streams(&self) -> Option<&StreamGraph> {
        self.streams.as_ref()
    }

    pub fn has_streams(&self) -> bool {
        self.streams.is_some()
    }

    /// Attach a graph, or refuse it.
    ///
    /// The radius is compared **on its bits**, not its value: a graph built at a radius one
    /// ULP away is a different planet by DETERMINISM-001, and an approximate comparison
    /// here would be exactly the "nearly right predicate" that invariant exists to forbid.
    ///
    /// The seed comparison crosses a signedness boundary that is real and is not this
    /// type's to resolve: `Surface::world_seed` is an `i64` because `plates_for` keys a
    /// decimal string, and `GraphHeader::world_seed` is a `u64` because the node sampler
    /// hashes it. The two's-complement reinterpretation is the same one `Surface::new`
    /// already makes when it hands the seed to `Noise`, so a graph built from a surface's
    /// seed matches, and one built from any other seed does not.
    ///
    /// The generator version is checked too, because a graph is a *derived* artifact: it
    /// was built from the field a particular generator produced, and one built under a
    /// different generator describes a landscape this surface does not have.
    pub fn attach_streams(&mut self, streams: StreamGraph) -> Result<(), WorldMismatch> {
        let header = *streams.header();
        let seed = self.surface.world_seed as u64; // cast-ok: two's-complement reinterpretation, the same one Surface::new makes for Noise -- not a float truncation
        if header.world_seed != seed {
            return Err(WorldMismatch::Seed { surface: self.surface.world_seed, graph: header.world_seed });
        }
        if header.radius_m.to_bits() != self.surface.radius_m.to_bits() {
            return Err(WorldMismatch::Radius {
                surface_bits: self.surface.radius_m.to_bits(),
                graph_bits: header.radius_m.to_bits(),
            });
        }
        if header.generator_version != GENERATOR_VERSION {
            return Err(WorldMismatch::Generator {
                world: GENERATOR_VERSION,
                graph: header.generator_version,
            });
        }
        self.streams = Some(streams);
        Ok(())
    }

    /// Remove the drainage network and hand it back. Slice 5 rebuilds one after eroding;
    /// dropping the old graph silently would lose the only record of what was eroded from.
    pub fn take_streams(&mut self) -> Option<StreamGraph> {
        self.streams.take()
    }
}

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
mod world_tests {
    use super::*;
    use crate::sphere::{SpherePoint, EARTH_RADIUS_M};
    use crate::stream::{sample_nodes, BuildParams, SamplingKind, StreamGraph};
    use crate::surface::Surface;

    const SEED: i64 = 20_260_904;
    const NODES: u32 = 2_000;
    const DATUM_M: f64 = 0.0;

    fn surface(seed: i64, radius_m: f64) -> Surface {
        Surface::new(seed, radius_m, 22, 0.29, None)
    }

    /// Heights straight from the field, which is the only coupling between the two
    /// representations: the graph's nodes are *where*, and `Surface` says *how high*.
    fn graph_for(seed: i64, radius_m: f64) -> StreamGraph {
        let world_seed = seed as u64; // cast-ok: two's-complement reinterpretation, matching Surface::new
        let sampling = sample_nodes(world_seed, NODES, radius_m).expect("a node set");
        let field = surface(seed, radius_m);
        let heights: Vec<f64> = sampling
            .positions
            .iter()
            .map(|p: &SpherePoint| field.elevation_m(p, None))
            .collect();
        StreamGraph::build(
            &BuildParams {
                world_seed,
                radius_m,
                sea_level_m: DATUM_M,
                sampling_kind: SamplingKind::Spiral,
                pond_max_drainage_area_m2: 5.0e9,
            },
            &sampling.positions,
            &heights,
            &sampling.area_m2,
            &sampling.neighbours,
        )
        .expect("a graph over a real field builds")
    }

    #[test]
    fn a_world_starts_with_no_drainage_network() {
        let world = World::new(surface(SEED, EARTH_RADIUS_M));
        assert!(!world.has_streams());
        assert!(world.streams().is_none());
        assert_eq!(world.surface().world_seed, SEED);
    }

    /// The graph is *attached*, not rebuilt, and the surface is untouched by attaching it.
    #[test]
    fn a_graph_for_the_same_planet_attaches_and_the_surface_is_unchanged() {
        let mut world = World::new(surface(SEED, EARTH_RADIUS_M));
        let probe = SpherePoint::from_latlon(12.5, -47.25);
        let before = world.surface().elevation_m(&probe, None).to_bits();

        world.attach_streams(graph_for(SEED, EARTH_RADIUS_M)).expect("same planet");
        assert!(world.has_streams());
        let graph = world.streams().expect("attached");
        assert_eq!(graph.node_count(), NODES);
        assert_eq!(graph.header().generator_version, GENERATOR_VERSION);
        assert!(graph.validate().is_ok());

        assert_eq!(world.surface().elevation_m(&probe, None).to_bits(), before);
    }

    /// The check nothing else in the crate can make: `build` never sees a `Surface`.
    #[test]
    fn a_graph_from_another_seed_is_refused() {
        let mut world = World::new(surface(SEED, EARTH_RADIUS_M));
        let err = world
            .attach_streams(graph_for(SEED + 1, EARTH_RADIUS_M))
            .expect_err("another seed is another planet");
        assert_eq!(
            err,
            WorldMismatch::Seed { surface: SEED, graph: (SEED + 1) as u64 } // cast-ok: the same two's-complement reinterpretation the type documents
        );
        assert!(!world.has_streams(), "a refused graph must not be half-attached");
    }

    /// Bits, not values. One ULP of radius is a different planet under DETERMINISM-001,
    /// and an approximate comparison here would be the nearly-right predicate that
    /// invariant forbids.
    #[test]
    fn a_graph_at_a_radius_one_ulp_away_is_refused() {
        let nudged = f64::from_bits(EARTH_RADIUS_M.to_bits() + 1);
        assert_ne!(nudged, EARTH_RADIUS_M);
        let mut world = World::new(surface(SEED, EARTH_RADIUS_M));
        let err = world
            .attach_streams(graph_for(SEED, nudged))
            .expect_err("one ULP of radius is a different planet");
        assert_eq!(
            err,
            WorldMismatch::Radius {
                surface_bits: EARTH_RADIUS_M.to_bits(),
                graph_bits: nudged.to_bits(),
            }
        );
        assert!(!world.has_streams());
    }

    /// `Surface` gains no field, and this is the executable form of that promise: the
    /// eight fields the port settled on, by name, and nothing about streams among them.
    #[test]
    fn the_surface_is_not_modified_by_this_slice() {
        let source = include_str!("surface.rs");
        let decl = "pub struct Surface {";
        let start = source.find(decl).expect("the struct is declared") + decl.len();
        let end = start + source[start..].find('}').expect("the struct closes");
        let body = &source[start..end];
        for field in [
            "pub world_seed: i64,",
            "pub radius_m: f64,",
            "pub plates: PlateSet,",
            "pub land: Continentality,",
            "pub tectonics: Tectonics,",
            "pub shelf: Shelf,",
            "pub detail: Detail,",
            "pub features: Features,",
        ] {
            assert!(body.contains(field), "Surface lost the field {field}");
        }
        assert_eq!(body.matches("pub ").count(), 8, "Surface must still have eight fields");
        for banned in ["stream", "Stream", "graph", "Graph", "drainage", "downhill"] {
            assert!(!body.contains(banned), "Surface grew a graph field: {banned}");
        }
    }

    /// `take_streams` hands the graph back rather than dropping it, and leaves the world in
    /// the state it started in. Slice 5 rebuilds after eroding and needs the old one.
    #[test]
    fn taking_the_streams_returns_them_and_empties_the_world() {
        let mut world = World::new(surface(SEED, EARTH_RADIUS_M));
        world.attach_streams(graph_for(SEED, EARTH_RADIUS_M)).expect("attaches");
        let taken = world.take_streams().expect("the graph comes back");
        assert_eq!(taken.node_count(), NODES);
        assert!(!world.has_streams());
        assert!(world.take_streams().is_none());
    }
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
