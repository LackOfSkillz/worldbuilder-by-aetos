//! Uplift and erodibility as explicit, recorded inputs to the erosion bake.
//!
//! This module implements no solver yet -- that is Tasks 2 through 4 of the same slice.
//! What it settles is the *interface* every one of those tasks consumes: how `u` (uplift)
//! and `k` (erodibility) enter the equation
//!
//! ```text
//! dh/dt = u - k * A^m * s^n        with n = 1, m = 0.5
//! ```
//!
//! (docs/design/2026-09-02-mark-2-world-studio.md §14.1, Cordonnier et al. 2016), and how
//! that entry is recorded so a baked graph can say which inputs produced it.
//!
//! # A run is reproducible from its recorded parameters and nothing else
//!
//! [`ErosionParams`] exists because two bakes that differ in uplift or erodibility are not
//! comparable, and a stored graph has to say which it was baked with. Every value the
//! solver's answer depends on belongs in this type -- a constant that instead lives as a
//! literal inside the solver is a hidden input: two runs that differ only in it look
//! identical in the record. `StreamGraph::build`'s `BuildParams` sets the house rule for
//! this already: `pond_max_drainage_area_m2` is a required field with no default, "so that
//! nobody inherits a number nobody chose" (`stream.rs`). `ErosionParams` follows it --
//! `#[derive(Default)]` is deliberately absent, and every field is populated by the caller.
//!
//! [`STREAM_POWER_AREA_EXPONENT`] and [`STREAM_POWER_SLOPE_EXPONENT`] are the opposite
//! case: the paper fixes `m = 0.5` and `n = 1`, so those are spec constants with their
//! source cited here, not fields on `ErosionParams` and not bare literals scattered through
//! the solver. A caller cannot tune them because the equation being solved is only the
//! stream power equation when they hold; a different exponent is a different equation, not
//! a calibration of this one.
//!
//! # Uniform for now, and what that forecloses
//!
//! §14.4 lists both `u` and `k` as **recomputable** from parameters rather than fields that
//! must be stored per node, so storing them on every node (and growing every serialised
//! graph by two `f64`s per node for values a formula can regenerate) is not required. This
//! task takes the smallest legitimate first step: **uniform values, identical at every
//! node.** [`ErosionParams::uplift_at`] and [`ErosionParams::erodibility_at`] already take a
//! node index, so a caller (and every later task) already calls them per node -- but today
//! both ignore the index and return the same scalar for the whole planet.
//!
//! **This is a real limitation, not a placeholder that is already finished.** A uniform
//! uplift field has no tectonic signal in it: a planet with an active collision margin on
//! one side and a stable craton on the other erodes as if both were rising at the same
//! rate, because nothing here reads `plates.rs`, `tectonics.rs` or the node's position to
//! find out otherwise. The two methods keep the per-node call shape so that a later task can
//! make `u` and `k` vary with a plate margin, a rock type from `substrate.rs`, or a stored
//! per-node field, without changing the signature every earlier caller already uses. Making
//! that variation real is not this task's job and is not done by writing the shape down.
//!
//! # The traversal direction the solver needs is the opposite of what `peel()` returns
//!
//! `StreamGraph::peel()` documents its own order as "a topological order from leaves to
//! roots" -- exactly what drainage-area accumulation needs, since a node's contribution to
//! its receiver can only be added once every one of *its own* contributors has been folded
//! in.
//!
//! §14.3 says the implicit solver instead walks "the stream trees from root to leaves",
//! because each node's height update reads its receiver's *already-updated* height -- a
//! receiver has to be resolved before the node that drains into it, which is the reverse
//! dependency of the accumulation pass. **The solver therefore consumes `peel().order`
//! reversed, not a new traversal.** `peel()` is deterministic by construction (its ready
//! queue is seeded in ascending index order and consumed FIFO), and that determinism is
//! exactly what makes reversing it -- rather than writing a second walk -- safe to rely on
//! for a reproducible bake. Getting this backwards produces a graph that still looks like
//! terrain, because every node still gets *some* update; it is simply the wrong one, and
//! nothing about the output announces that.
//!
//! # Bit-exact, but not because the inner loop avoids transcendentals
//!
//! **An earlier draft of this doc claimed this module could hold the strict bit-for-bit
//! contract because its exponents avoid transcendentals. Wrong on both halves -- see the
//! plan's `cea453f` correction, which this section now matches.**
//!
//! With `n = 1`, `s^1` is `s` -- no call at all. With `m = 0.5`, `A^0.5` is `sqrt`, which
//! IEEE-754 requires to be *correctly rounded*. That is real and worth keeping: it means
//! `detmath::powf` never enters the inner loop of a 100-300 iteration bake. **But slope
//! needs the distance from a node to its receiver**, and
//! [`crate::sphere::SpherePoint::distance_to`] is `angle_to * radius_m` with
//! `angle_to = detmath::atan2(across, along)` -- so the step this module implements calls
//! `atan2` after all, once per node per step. That is expected and correct here, not a
//! violation to route around.
//!
//! The strict-versus-bounded split this project otherwise cares about governs
//! *Python-against-Rust* conformance, and it does not apply to this module at all:
//! `worldbuilder/` has no stream power and this plan forbids writing one, so there is no
//! Python side to diverge from. The only equality claims a test here can make are **native
//! against WASM** and **run against run** -- and both hold bit-for-bit regardless of
//! `atan2`, because native and WASM share the same pure-Rust `libm` crate rather than each
//! dispatching to its own platform's. A test in this module therefore still compares **bit
//! patterns, never a tolerance** -- not because the loop is transcendental-free, but because
//! both sides of every comparison this module can make run the identical `libm`.
//!
//! Distances are computed **once per step, before the walk**, via [`receiver_distances_m`],
//! rather than once per node per iteration inside a caller's loop: Task 3's iteration calls
//! it once and reuses the result across all 100-300 iterations, instead of recomputing
//! `atan2` for the same node/receiver pair on every one of them. Positions themselves are
//! not stored on the graph (`GraphHeader` keeps only a `position_checksum`); a caller
//! regenerates them once with `stream::node_positions` and passes them in.
//!
//! If a later task finds itself reaching for `powf` in the inner loop, the formulation has
//! drifted away from `n = 1, m = 0.5` -- that is the moment to come back and change this
//! doc, not to add a tolerance and move on. `atan2` in the distance computation is not that
//! signal; it was always expected here.
//!
//! # A root has no receiver, so it is held fixed rather than uplifted
//!
//! `StreamGraph::downhill_of` returns `None` exactly at a root, and `stream.rs`'s own
//! invariant (`RootIsNeitherMouthNorLake` / `RootIsBothMouthAndLake`) says every root is
//! either a mouth (the sea) or a pond/lake outlet -- in either case, the local **base
//! level** its whole basin erodes toward, not a slope with a downhill neighbour to measure.
//! Uplifting a root anyway would raise that base level independently of anything eroding
//! into it, which is not what "base level" means: the land upstream would spend the whole
//! bake chasing a floor that never stopped rising under it. This module instead holds a
//! root's height fixed for the step ([`erode_step`] copies it through unchanged). Applying
//! `u * dt` at roots instead is the other defensible choice -- it is a different equation,
//! not a bug in this one, and a later task that wants a rising sea floor or a filling lake
//! should change this decision explicitly rather than inherit it by accident.

use crate::detmath;
use crate::sphere::SpherePoint;
use crate::stream::StreamGraph;

/// The area exponent `m` in `dh/dt = u - k * A^m * s^n`.
///
/// Fixed by Cordonnier et al. (2016) and by
/// `docs/design/2026-09-02-mark-2-world-studio.md` §14.1 ("n = 1, m = 0.5"). A spec
/// constant, not a tunable: solving the equation with a different exponent is solving a
/// different equation, so this is named and cited rather than left as a literal `0.5`
/// wherever the solver needs it.
///
/// **This constant documents the spec's value; it is not a switch.** `implicit_receiver_update`
/// hardcodes `detmath::sqrt(area_m2)` for `A^0.5` rather than reading this constant, because
/// `sqrt` is a distinct, cheaper, correctly-rounded operation and not `powf` called with an
/// argument -- there is no generic `A^m` in the inner loop to parameterise. Editing this
/// constant's value therefore changes nothing except `the_stream_power_exponents_match_the_spec`,
/// which exists to fail loudly the moment that becomes true, rather than let a wrong sense of
/// "increased granularity" imply that increasing this constant changes the solver's behaviour.
/// If `A^m` is ever generalised past `m = 0.5`, that is the moment this constant becomes a real
/// input to `powf` and this note should be deleted, not the moment to add a tolerance.
pub const STREAM_POWER_AREA_EXPONENT: f64 = 0.5;

/// The slope exponent `n` in `dh/dt = u - k * A^m * s^n`.
///
/// Fixed at `1` by the same source as [`STREAM_POWER_AREA_EXPONENT`]. At this value `s^n`
/// is `s` itself -- multiplication, not a call. **This buys a cost win, not a conformance
/// upgrade**: it means `detmath::powf` never enters the inner loop of a 100-300 iteration
/// bake. It does not make this module's inner loop transcendental-free -- `erode_step`
/// still calls `detmath::atan2` by way of `SpherePoint::distance_to` (see the module doc's
/// "Bit-exact, but not because the inner loop avoids transcendentals" section) -- and the
/// strict/bounded contract split this project otherwise cares about governs
/// Python-against-Rust conformance, which does not apply here at all since erosion has no
/// Python counterpart. An earlier version of this doc claimed the opposite ("the strict
/// bit-for-bit contract... not the bounded one") and was corrected in `cea453f`; this
/// paragraph must keep agreeing with the module doc above, not repeat the retracted claim.
///
/// **Like [`STREAM_POWER_AREA_EXPONENT`], this constant documents the spec's value; it is
/// not a switch.** `implicit_receiver_update` computes slope as a bare `(h_i - h_r) / d`,
/// which *is* `s^1` -- there is no `powf(s, n)` call anywhere for this to parameterise, so
/// editing this constant changes nothing except the pin test that checks it still equals
/// `1.0`. Wiring it through a real `powf(s, STREAM_POWER_SLOPE_EXPONENT)` would put a
/// transcendental in the inner loop for a case that is always `n = 1` today -- a real cost
/// paid for a cosmetic generality -- so this stays a documented, pinned value rather than a
/// parameter threaded through the solver.
pub const STREAM_POWER_SLOPE_EXPONENT: f64 = 1.0;

/// Everything the stream power equation needs beyond the graph itself, and everything a
/// bake must record for its answer to be reproducible.
///
/// `Copy`, `Debug`, `PartialEq` to match `stream.rs::BuildParams`, so a test (or a caller
/// deciding whether two bakes are comparable) can compare two parameter sets directly.
/// No `Default`: every field is something a caller chose, not something inherited silently.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ErosionParams {
    /// `u`, the tectonic uplift rate, in metres per year, applied uniformly across the
    /// whole graph (see the module doc's "Uniform for now" section). Positive raises the
    /// land; the equation subtracts erosion from it, so a node with no drainage above it
    /// still rises at this rate.
    pub uplift_m_per_yr: f64,

    /// `k`, erodibility, in inverse years. Dimensional check: `A^0.5` (`A` in m^2)
    /// contributes a length in metres, `s` is dimensionless (rise over run), so for
    /// `k * A^0.5 * s` to be an erosion rate in metres per year, `k` itself carries units of
    /// `1/yr`. Uniform across the graph, for the same reason and with the same limitation as
    /// `uplift_m_per_yr`.
    pub erodibility_per_yr: f64,

    /// `dt`, the timestep of one implicit step, in years. Task 1's review flagged the
    /// timestep as a value the result unambiguously depends on: two bakes that differ only
    /// in `dt` erode by different amounts and are not comparable runs, so `dt` belongs on
    /// this recorded, `PartialEq`-comparable type rather than living as a solver constant or
    /// a bare argument that a stored graph's header says nothing about. Named with its unit
    /// like its siblings, for the same reason they are.
    pub timestep_yr: f64,
}

impl ErosionParams {
    /// `u` at a node. Uniform today -- every node receives `uplift_m_per_yr` regardless of
    /// `node` -- but the per-node signature is the seam a later task uses to source uplift
    /// from something spatial (a plate margin, a stored field) without moving every caller.
    /// See the module doc for what uniform uplift forecloses.
    pub fn uplift_at(&self, _node: u32) -> f64 {
        self.uplift_m_per_yr
    }

    /// `k` at a node. Uniform today, for the same reason and with the same limitation as
    /// [`ErosionParams::uplift_at`].
    pub fn erodibility_at(&self, _node: u32) -> f64 {
        self.erodibility_per_yr
    }
}

/// `peel().order` reversed: root-to-leaves, the direction the implicit solver walks so that
/// a node's receiver is already updated when the node itself is processed (see the module
/// doc's traversal-direction section). A thin, deliberately trivial wrapper -- **not a
/// second traversal** -- so every later task reaches for this name instead of writing
/// `graph.peel().order.iter().rev()` at each call site and one of them getting it backwards.
pub fn root_to_leaves(graph: &StreamGraph) -> Vec<u32> {
    let mut order = graph.peel().order;
    order.reverse();
    order
}

/// The great-circle distance from every node to its downhill receiver, in metres --
/// `distances[node]` is `positions[node].distance_to(&positions[receiver], radius_m)`.
///
/// Computed as a single pass over the whole graph, **once**, so [`erode_step`] (and Task
/// 3's loop over it) never recomputes `atan2` for a node/receiver pair that has not moved:
/// positions are fixed for the whole bake, only heights change per step. A root has no
/// receiver, so its entry is `0.0` -- a placeholder that [`erode_step`]'s `None` branch
/// structurally never indexes into, since a root's update does not consult distance at
/// all (see the module doc). `0.0` was kept rather than `f64::NAN` -- which would fail
/// louder if some future caller ever did read it, at the cost of turning today's `is_finite`
/// check into something that would need to special-case roots -- because the read really is
/// unreachable today: making it loud is a Task 3 concern, for whichever task first starts
/// holding this vector across an iteration loop and so first has an opportunity to misuse it.
///
/// `positions` must be indexed exactly like the graph, i.e. `stream::node_positions` called
/// with the same seed and count the graph itself was built from; this function trusts that
/// rather than re-deriving it, the same way `StreamGraph::build` trusts the positions it is
/// handed.
pub fn receiver_distances_m(graph: &StreamGraph, positions: &[SpherePoint]) -> Vec<f64> {
    debug_assert_eq!(
        positions.len(),
        graph.node_count() as usize, // cast-ok: node_count is bounded (stream.rs::MAX_NODES)
        "positions must be exactly as long as the graph has nodes"
    );
    let radius_m = graph.header().radius_m;
    (0..graph.node_count())
        .map(|node| match graph.downhill_of(node) {
            Some(receiver) => {
                let from = &positions[node as usize]; // cast-ok: a node index into usize
                let to = &positions[receiver as usize]; // cast-ok: a node index into usize
                from.distance_to(to, radius_m)
            }
            None => 0.0, // a root: never read by erode_step, see the module doc
        })
        .collect()
}

/// One implicit step of `dh/dt = u - k * A^0.5 * s` over the whole graph, walking
/// [`root_to_leaves`] so every node's receiver is already at its new height when the node
/// itself is updated.
///
/// # Derivation
///
/// Slope at node `i` with receiver `r` and receiver-distance `d` is `s = (h_i - h_r) / d`.
/// Discretising implicitly -- the receiver height on the right-hand side is the *new* one,
/// not the old -- gives
///
/// ```text
/// (h_i' - h_i) / dt = u - k * sqrt(A_i) * (h_i' - h_r') / d
/// ```
///
/// Multiply through by `dt`, expand, and collect every `h_i'` term on the left. With
/// `c = k * dt * sqrt(A_i) / d`:
///
/// ```text
/// h_i' - h_i = u*dt - c*h_i' + c*h_r'
/// h_i' * (1 + c) = h_i + u*dt + c*h_r'
/// h_i' = (h_i + u*dt + c*h_r') / (1 + c)
/// ```
///
/// `h_r'` is the receiver's already-updated height, which only exists at this point in the
/// walk because the walk is root-to-leaves.
///
/// A root has no `r`, `s`, or `d` at all -- see the module doc for why this function holds
/// a root's height fixed rather than applying `u * dt` to it.
///
/// `heights[node]` is `h` before the step; the returned vector is `h'` after it, same
/// length and same indexing. `distances_m` is [`receiver_distances_m`]'s output for this
/// graph; passing it in rather than recomputing it here is what keeps `atan2` out of a
/// caller's per-iteration cost (see the module doc).
///
/// `d` can only be zero if two nodes coincide, which `StreamGraph::build` already rejects
/// (`GraphError::CoincidentNodes`) before a graph exists to call this on -- so this function
/// adds no division guard. A guard that silently substituted a value for `d = 0` would hide
/// exactly the defect the builder already refuses to let through; the well-formed
/// precondition is enforced upstream, once, rather than defended against redundantly here.
pub fn erode_step(graph: &StreamGraph, heights: &[f64], distances_m: &[f64], params: &ErosionParams) -> Vec<f64> {
    let count = graph.node_count() as usize; // cast-ok: node_count is bounded (stream.rs::MAX_NODES)
    // `assert_eq!`, not `debug_assert_eq!`: the bake runs in release, which is the one build
    // a debug assertion does not reach. The two failure modes are not symmetric. A slice that
    // is too SHORT panics anyway on the first out-of-range index, loudly and safely. A slice
    // that is too LONG never trips a bounds check at all -- `next` is cloned from `heights`,
    // so the extra entries are carried through untouched and returned, and the caller gets an
    // array longer than the graph with a silent tail nobody wrote. That is a quiet
    // wrong-length result, which is the failure this project keeps finding.
    //
    // These are two comparisons per STEP, not per node, so the release-time cost is nil
    // against a 100-300 iteration bake over millions of nodes.
    assert_eq!(heights.len(), count, "heights must be exactly as long as the graph has nodes");
    assert_eq!(
        distances_m.len(),
        count,
        "distances_m must be exactly as long as the graph has nodes"
    );
    let mut next = heights.to_vec();
    let dt = params.timestep_yr;

    for node in root_to_leaves(graph) {
        let idx = node as usize; // cast-ok: a node index into usize
        debug_assert!(idx < count, "root_to_leaves must only yield indices within the graph");
        let uplift = params.uplift_at(node);

        match graph.downhill_of(node) {
            None => {
                // A root: held fixed for the step, not uplifted. See the module doc's
                // "A root has no receiver" section for why.
                next[idx] = heights[idx];
            }
            Some(receiver) => {
                let receiver_idx = receiver as usize; // cast-ok: a node index into usize
                // `next[receiver_idx]` is that node's NEW height, not the old one -- sound
                // only because root_to_leaves visits every receiver before the node that
                // drains into it, so by the time this line runs the receiver has already
                // been overwritten in this same pass.
                let receiver_h_new = next[receiver_idx];
                next[idx] = implicit_receiver_update(
                    heights[idx],
                    receiver_h_new,
                    graph.drainage_area_m2(node),
                    distances_m[idx],
                    uplift,
                    params.erodibility_at(node),
                    dt,
                );
            }
        }
    }

    next
}

/// The closed form itself, isolated from graph traversal so it can be tested against exact
/// arithmetic identities directly (a real `StreamGraph` node can never have `area_m2 == 0`
/// -- `drainage_area_m2` includes the node's own area, and `build` rejects non-positive
/// area -- so the zero-area case is only reachable by calling this function directly).
///
/// See [`erode_step`]'s doc for the derivation this implements.
fn implicit_receiver_update(
    h_i: f64,
    receiver_h_new: f64,
    area_m2: f64,
    distance_m: f64,
    uplift: f64,
    erodibility: f64,
    dt: f64,
) -> f64 {
    let c = erodibility * dt * detmath::sqrt(area_m2) / distance_m;
    (h_i + uplift * dt + c * receiver_h_new) / (1.0 + c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sphere::EARTH_RADIUS_M;
    use crate::stream::{sample_nodes, BuildParams, SamplingKind};

    // ---- a small, real graph, built the same way lib.rs's World tests build one ---------

    const SEED: u64 = 20_260_904;
    const NODES: u32 = 300;

    fn splitmix64(x: u64) -> u64 {
        let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A deterministic, non-flat height field -- this module owns no field of its own, and
    /// borrowing `Surface` here would give `erosion.rs`'s tests a dependency this module
    /// does not otherwise have. The only property the fixture needs is a real graph whose
    /// `peel()` actually forms a forest, which any finite, non-constant height field over a
    /// connected node set gives it.
    fn heights_for(seed: u64, count: u32) -> Vec<f64> {
        (0..count)
            .map(|i| {
                let bits = splitmix64(seed ^ splitmix64(i as u64)); // cast-ok: a node index, already an integer
                let unit = ((bits >> 11) as f64) / 9_007_199_254_740_992.0; // cast-ok: 53 bits into f64
                -500.0 + 4000.0 * unit
            })
            .collect()
    }

    /// A graph and the positions it was built from, together -- `StreamGraph` does not
    /// store positions (only their checksum), so any test that needs a distance has to
    /// keep the positions alongside the graph itself, exactly as a real caller would.
    fn small_graph_and_positions() -> (StreamGraph, Vec<SpherePoint>, Vec<f64>) {
        let sampling = sample_nodes(SEED, NODES, EARTH_RADIUS_M).expect("a node set");
        let heights = heights_for(SEED, NODES);
        let graph = StreamGraph::build(
            &BuildParams {
                world_seed: SEED,
                radius_m: EARTH_RADIUS_M,
                sea_level_m: -600.0,
                sampling_kind: SamplingKind::Spiral,
                pond_max_drainage_area_m2: 1.0e10,
            },
            &sampling.positions,
            &heights,
            &sampling.area_m2,
            &sampling.neighbours,
        )
        .expect("a graph over a real field builds");
        (graph, sampling.positions, heights)
    }

    fn small_graph() -> StreamGraph {
        small_graph_and_positions().0
    }

    fn default_test_params() -> ErosionParams {
        ErosionParams { uplift_m_per_yr: 1.0e-3, erodibility_per_yr: 1.0e-6, timestep_yr: 1000.0 }
    }

    // ---- the spec constants are pinned, not silently driftable ---------------------------

    #[test]
    fn the_stream_power_exponents_match_the_spec() {
        assert_eq!(STREAM_POWER_AREA_EXPONENT, 0.5);
        assert_eq!(STREAM_POWER_SLOPE_EXPONENT, 1.0);
    }

    // ---- ErosionParams is the recorded, comparable interface ------------------------------

    #[test]
    fn erosion_params_is_copy_and_compares_by_value() {
        let a = ErosionParams { uplift_m_per_yr: 1.0e-4, erodibility_per_yr: 2.0e-6, timestep_yr: 1000.0 };
        let b = a; // Copy, not a move -- `a` must still be usable below.
        assert_eq!(a, b);

        let different =
            ErosionParams { uplift_m_per_yr: 1.0e-4, erodibility_per_yr: 3.0e-6, timestep_yr: 1000.0 };
        assert_ne!(a, different, "two runs differing only in erodibility must not compare equal");

        let different_dt =
            ErosionParams { uplift_m_per_yr: 1.0e-4, erodibility_per_yr: 2.0e-6, timestep_yr: 2000.0 };
        assert_ne!(a, different_dt, "two runs differing only in timestep must not compare equal");
    }

    #[test]
    fn debug_output_shows_all_fields() {
        let params = ErosionParams { uplift_m_per_yr: 1.0e-4, erodibility_per_yr: 2.0e-6, timestep_yr: 1000.0 };
        let text = format!("{params:?}");
        assert!(text.contains("uplift_m_per_yr"));
        assert!(text.contains("erodibility_per_yr"));
        assert!(text.contains("timestep_yr"));
    }

    // ---- uniform now, and honestly so ------------------------------------------------------

    #[test]
    fn uplift_and_erodibility_are_uniform_across_every_node() {
        let params =
            ErosionParams { uplift_m_per_yr: 5.0e-4, erodibility_per_yr: 7.0e-7, timestep_yr: 1000.0 };
        for node in [0u32, 1, 42, NODES - 1] {
            assert_eq!(params.uplift_at(node), params.uplift_m_per_yr);
            assert_eq!(params.erodibility_at(node), params.erodibility_per_yr);
        }
    }

    // ---- the traversal direction: reversed peel order starts at the roots ----------------

    #[test]
    fn root_to_leaves_visits_every_receiver_before_the_node_that_drains_into_it() {
        // The property the solver actually depends on, and the one an off-by-a-direction
        // bug breaks: for every edge `node -> target` in the downhill relation, `target`
        // must be processed BEFORE `node`, because the implicit step reads the receiver's
        // already-updated height. This holds edge-by-edge regardless of how many separate
        // trees the forest has, unlike "all roots come before all leaves" -- which is false
        // whenever one tree is shallower than another (a small tree's root can finish
        // peeling before a deep tree's leaves do).
        let graph = small_graph();
        let peel = graph.peel();
        assert_eq!(peel.peeled, graph.node_count(), "the height field must not have plateaued into a cycle");

        let reversed = root_to_leaves(&graph);
        assert_eq!(reversed.len(), peel.order.len());

        let mut position_of = vec![0usize; graph.node_count() as usize]; // cast-ok: node_count is bounded, mirroring stream.rs's own casts
        for (position, &node) in reversed.iter().enumerate() {
            position_of[node as usize] = position; // cast-ok: a valid node index into usize
        }

        let mut edges_checked = 0;
        for node in 0..graph.node_count() {
            if let Some(target) = graph.downhill_of(node) {
                assert!(
                    position_of[target as usize] < position_of[node as usize], // cast-ok: a valid node index into usize
                    "receiver {target} of node {node} must come before it in root-to-leaves order"
                );
                edges_checked += 1;
            }
        }
        assert!(edges_checked > 0, "a graph with no downhill edges at all is not a meaningful fixture");

        // The reverse claim, stated directly rather than only implied by the edge check:
        // `peel.order` UNREVERSED puts every receiver AFTER the node that drains into it
        // (that is what "leaves to roots" means), which is the wrong order for the solver.
        // Using it directly instead of `root_to_leaves` would violate the property above on
        // every single edge, which is the shape of bug this test exists to catch.
        let mut unreversed_position_of = vec![0usize; graph.node_count() as usize]; // cast-ok: node_count is bounded
        for (position, &node) in peel.order.iter().enumerate() {
            unreversed_position_of[node as usize] = position; // cast-ok: a valid node index into usize
        }
        for node in 0..graph.node_count() {
            if let Some(target) = graph.downhill_of(node) {
                assert!(
                    unreversed_position_of[target as usize] > unreversed_position_of[node as usize], // cast-ok: a valid node index into usize
                    "peel().order unreversed is expected to put every receiver after its own node"
                );
            }
        }
    }

    #[test]
    fn root_to_leaves_reorders_rather_than_reinvents() {
        // Same multiset of nodes as `peel().order`, exactly reversed -- proving this
        // function is `peel()`'s own order read backwards, not an independent walk that
        // could disagree with it.
        let graph = small_graph();
        let peel = graph.peel();
        let mut expected = peel.order.clone();
        expected.reverse();
        assert_eq!(root_to_leaves(&graph), expected);
    }

    // ---- receiver_distances_m -------------------------------------------------------------

    #[test]
    fn receiver_distances_m_matches_sphere_point_distance_to_directly() {
        let (graph, positions, _heights) = small_graph_and_positions();
        let radius_m = graph.header().radius_m;
        let distances = receiver_distances_m(&graph, &positions);
        assert_eq!(distances.len(), graph.node_count() as usize); // cast-ok: node_count is bounded

        let mut receiver_edges_checked = 0;
        for node in 0..graph.node_count() {
            match graph.downhill_of(node) {
                Some(receiver) => {
                    let expected = positions[node as usize] // cast-ok: a node index into usize
                        .distance_to(&positions[receiver as usize], radius_m); // cast-ok: a node index into usize
                    assert_eq!(
                        distances[node as usize].to_bits(), // cast-ok: a node index into usize
                        expected.to_bits(),
                        "node {node}'s receiver distance must be exactly SpherePoint::distance_to's own answer"
                    );
                    receiver_edges_checked += 1;
                }
                None => assert_eq!(distances[node as usize], 0.0, "a root's distance entry is an unread placeholder"), // cast-ok: a node index into usize
            }
        }
        assert!(receiver_edges_checked > 0, "a graph with no downhill edges at all is not a meaningful fixture");
    }

    // ---- erode_step: determinism -----------------------------------------------------------

    #[test]
    fn erode_step_is_bit_identical_across_two_runs_of_the_same_inputs() {
        // Population: every one of NODES = 300 nodes, built TWICE from independent calls to
        // `small_graph_and_positions()` -- two distinct `StreamGraph`s, `Vec<SpherePoint>`s
        // and `Vec<f64>`s at different addresses, rather than one set of inputs reused --
        // so this cannot pass by accident of aliasing or allocator reuse. Method:
        // `receiver_distances_m` then `erode_step`, run once per independent build,
        // compared by `f64::to_bits()` element-by-element (never `==`, which would let two
        // differently-signed zeros or a native/WASM tolerance pass unnoticed). Host: this
        // crate's own test process (native), run-against-run rather than
        // native-against-WASM.
        //
        // What this test cannot claim: it says nothing about the module doc's
        // native-against-WASM equality (that would need a WASM harness this crate's unit
        // tests do not run -- see `viewer/`'s `build:wasm:self-test`), and both builds still
        // execute the same code path in the same process, so it cannot catch a
        // hypothetical dependency on iteration order over an actual `HashMap` (this module
        // has none; `StreamGraph::peel()`'s queue is a `Vec`, not a hash-ordered
        // structure). It does rule out the weaker failure modes above it, which is what a
        // "same inputs, same process" version of this test could not.
        let (graph_a, positions_a, heights_a) = small_graph_and_positions();
        let (graph_b, positions_b, heights_b) = small_graph_and_positions();
        let distances_a = receiver_distances_m(&graph_a, &positions_a);
        let distances_b = receiver_distances_m(&graph_b, &positions_b);
        let params = default_test_params();

        let first = erode_step(&graph_a, &heights_a, &distances_a, &params);
        let second = erode_step(&graph_b, &heights_b, &distances_b, &params);

        assert_eq!(first.len(), second.len());
        for i in 0..first.len() {
            assert_eq!(first[i].to_bits(), second[i].to_bits(), "node {i} disagreed bit-for-bit between two independent builds");
        }
    }

    // ---- erode_step: the traversal direction is load-bearing -------------------------------

    #[test]
    #[should_panic(expected = "heights must be exactly as long as the graph has nodes")]
    fn erode_step_rejects_a_heights_slice_longer_than_the_graph() {
        // The asymmetric case, and the reason these checks are `assert_eq!` rather than
        // `debug_assert_eq!`. A slice that is too SHORT would panic anyway on the first
        // out-of-range index. A slice that is too LONG trips no bounds check at all: `next`
        // is cloned from `heights`, so the surplus entries ride through untouched and get
        // returned, handing the caller an array longer than the graph with a tail nobody
        // wrote. Without a release-time assertion that is silent, and the bake runs in
        // release -- the one build a debug assertion does not reach.
        let (graph, positions, heights) = small_graph_and_positions();
        let distances = receiver_distances_m(&graph, &positions);
        let mut too_long = heights.clone();
        too_long.push(0.0);
        let _ = erode_step(&graph, &too_long, &distances, &default_test_params());
    }

    #[test]
    #[should_panic(expected = "distances_m must be exactly as long as the graph has nodes")]
    fn erode_step_rejects_a_distances_slice_that_does_not_match_the_graph() {
        // The same check on the other slice, named separately so a failure says WHICH
        // argument was wrong. Two arguments validated by one message is a message that
        // sends the reader to the wrong place half the time.
        let (graph, positions, heights) = small_graph_and_positions();
        let mut distances = receiver_distances_m(&graph, &positions);
        distances.pop();
        let _ = erode_step(&graph, &heights, &distances, &default_test_params());
    }

    #[test]
    fn erode_step_would_differ_if_walked_leaves_to_roots_instead_of_root_to_leaves() {
        // The property an off-by-a-direction bug breaks, made concrete rather than only
        // asserted by inspection: re-run the exact same per-node closed form but drive it
        // with `peel().order` (leaves-to-roots, `erosion.rs`'s own module doc names this as
        // the WRONG direction for this solver) instead of `root_to_leaves`. The only
        // difference between this block and `erode_step` is which order `next[]` gets
        // written in, so any disagreement below comes from the direction, not the formula.
        let (graph, positions, heights) = small_graph_and_positions();
        let distances = receiver_distances_m(&graph, &positions);
        let params = default_test_params();

        let correct = erode_step(&graph, &heights, &distances, &params);

        let mut wrong_direction = heights.clone();
        for node in graph.peel().order {
            // NOTE: `peel().order`, NOT `root_to_leaves(&graph)` -- the bug under test.
            let idx = node as usize; // cast-ok: a node index into usize
            match graph.downhill_of(node) {
                None => wrong_direction[idx] = heights[idx],
                Some(receiver) => {
                    let receiver_idx = receiver as usize; // cast-ok: a node index into usize
                    // In this (wrong) order the receiver has NOT been overwritten yet when
                    // a leaf-ward node reaches it, so this reads the OLD receiver height --
                    // exactly the defect root-to-leaves exists to avoid.
                    let receiver_h_stale = wrong_direction[receiver_idx];
                    wrong_direction[idx] = implicit_receiver_update(
                        heights[idx],
                        receiver_h_stale,
                        graph.drainage_area_m2(node),
                        distances[idx],
                        params.uplift_at(node),
                        params.erodibility_at(node),
                        params.timestep_yr,
                    );
                }
            }
        }

        assert_ne!(
            correct, wrong_direction,
            "walking leaves-to-roots must disagree with root-to-leaves somewhere in this fixture"
        );

        // Not vacuous: the fixture must actually contain a chain two hops deep, i.e. some
        // node whose receiver itself has its own receiver -- otherwise every h_r' is a
        // root's untouched height under both orders and the two walks would coincide by
        // accident rather than because the direction matters.
        let has_two_hop_chain = (0..graph.node_count())
            .any(|node| graph.downhill_of(node).and_then(|r| graph.downhill_of(r)).is_some());
        assert!(has_two_hop_chain, "fixture must contain a downhill chain at least two hops deep");
    }

    // ---- erode_step: the root policy is held-fixed, not uplift-only -----------------------

    #[test]
    fn a_root_is_held_at_its_old_height_rather_than_uplifted() {
        // The module doc chose "held fixed" over "uplift-only" because a root is the local
        // base level of its basin (a mouth or a lake outlet -- stream.rs's own invariant
        // says every root is exactly one of those), and the equation erodes everything
        // else toward that level rather than raising the level itself. That choice was
        // documented but previously asserted nowhere: a review confirmed by mutation that
        // flipping the root branch to `heights[idx] + uplift * dt` left all other tests
        // green. This test exists so that mutation is instead caught here, exactly, by bit
        // pattern -- not "close to unchanged", since holding fixed means literally copying
        // the old value through with no arithmetic at all.
        let (graph, positions, heights) = small_graph_and_positions();
        let distances = receiver_distances_m(&graph, &positions);
        // Uplift is deliberately large and nonzero: if a future edit silently switched the
        // root branch to uplift-only, a zero uplift would let it pass unnoticed by
        // coincidence.
        let params = ErosionParams { uplift_m_per_yr: 5.0e-2, erodibility_per_yr: 1.0e-6, timestep_yr: 1000.0 };

        let next = erode_step(&graph, &heights, &distances, &params);

        let roots = graph.roots();
        assert!(!roots.is_empty(), "fixture must contain at least one root to check this against");
        for root in roots {
            let idx = root as usize; // cast-ok: a node index into usize
            assert_eq!(
                next[idx].to_bits(),
                heights[idx].to_bits(),
                "root {root} must be held at its old height exactly, not uplifted by u*dt"
            );
        }
    }

    // ---- implicit_receiver_update: the exact arithmetic identities -------------------------

    #[test]
    fn zero_drainage_area_rises_by_exactly_uplift_times_dt() {
        // sqrt(0) = 0, so c = 0 and the closed form collapses to `(h_i + u*dt + 0) / 1`,
        // an exact identity rather than a tolerance -- dividing by 1.0 is a no-op, and the
        // numerator is a single addition chain with no term to round away.
        let h_i = 1234.5;
        let uplift = 3.0e-3;
        let dt = 500.0;
        let result = implicit_receiver_update(h_i, /* receiver_h_new */ -999.0, 0.0, 10.0, uplift, 1.0e-5, dt);
        assert_eq!(result.to_bits(), (h_i + uplift * dt).to_bits());
    }

    #[test]
    fn a_receiver_at_the_same_height_erodes_nothing() {
        // s = (h_i - h_r) / d = 0 when h_i == h_r, so with uplift also zero the update
        // is `(h + 0 + c*h) / (1 + c)` -- algebraically `h*(1+c)/(1+c)`, which cancels to
        // `h` exactly. Division by `1 + c` is not guaranteed bit-exact for an arbitrary
        // `c` the way dividing by the literal `1.0` is (the zero-drainage-area test
        // above), so this was originally written with a tolerance on the assumption that
        // rounding here was unavoidable. It was measured, not assumed: for these inputs
        // `c` evaluates to exactly `0.024`, and every intermediate -- `c*h`, `h + c*h`,
        // `1.0 + c`, and the final quotient -- lands on its exact value, so the result is
        // bit-identical to `h`. `f64::to_bits()` says so directly; there is nothing here
        // for a tolerance to forgive. (The general claim that an arbitrary `c` is not
        // guaranteed exact is still true -- it just doesn't apply to this `c`.)
        let h = 777.0;
        let result = implicit_receiver_update(h, h, 4.0e6, 250.0, 0.0, 3.0e-6, 1000.0);
        assert_eq!(result.to_bits(), h.to_bits(), "expected exactly {h}, got {result}");
    }

    // ---- erode_step: no NaN, no infinity, anywhere -----------------------------------------

    #[test]
    fn erode_step_output_is_finite_everywhere() {
        let (graph, positions, heights) = small_graph_and_positions();
        let distances = receiver_distances_m(&graph, &positions);
        let params = default_test_params();
        let next = erode_step(&graph, &heights, &distances, &params);

        let non_finite: Vec<usize> = next.iter().enumerate().filter(|(_, h)| !h.is_finite()).map(|(i, _)| i).collect();
        assert!(non_finite.is_empty(), "non-finite output at node indices {non_finite:?}");
    }

    // ---- erode_step: monotone in erodibility --------------------------------------------

    #[test]
    fn increasing_erodibility_never_raises_a_node_with_drainage_above_it() {
        // Every non-root node in this graph has `h_i > h_r` (the downhill relation is
        // strictly descending -- stream.rs's own invariant), and uplift is held positive
        // and fixed, so `h_r' < h_i + u*dt` holds at every node. Under that condition the
        // closed form's derivative with respect to `c` (and so with respect to k, since
        // `c` is monotone increasing in k) has a fixed sign -- see erode_step's derivation
        // -- so increasing k while everything else is held fixed must not raise any
        // non-root node's answer.
        let (graph, positions, heights) = small_graph_and_positions();
        let distances = receiver_distances_m(&graph, &positions);

        let low_k = ErosionParams { uplift_m_per_yr: 1.0e-3, erodibility_per_yr: 1.0e-8, timestep_yr: 1000.0 };
        let high_k = ErosionParams { uplift_m_per_yr: 1.0e-3, erodibility_per_yr: 1.0e-4, timestep_yr: 1000.0 };

        let with_low_k = erode_step(&graph, &heights, &distances, &low_k);
        let with_high_k = erode_step(&graph, &heights, &distances, &high_k);

        // Zero tolerance, not a rounding allowance: measured on this fixture (300 nodes,
        // seed 20_260_904, `SamplingKind::Spiral`, these two `k` values, `u = 1.0e-3`,
        // `dt = 1000.0`), every one of the 262 draining nodes falls by at least 3.46 m
        // between `low_k` and `high_k` -- nowhere near the boundary a rounding error could
        // cross. A tolerance here would be six-plus orders of magnitude looser than that
        // margin and would silently absorb a real formula regression instead of catching
        // it, which is exactly backwards for a module whose claim is bit-exact
        // determinism. If a future fixture or parameter choice ever needs slack, add it
        // deliberately and cite the measured margin that justifies it, the way this
        // comment does.
        let mut violations = Vec::new();
        let mut a_real_decrease_happened = false;
        for node in 0..graph.node_count() {
            let idx = node as usize; // cast-ok: a node index into usize
            if graph.downhill_of(node).is_none() {
                continue; // roots are held fixed regardless of k; nothing to check
            }
            if with_high_k[idx] > with_low_k[idx] {
                violations.push((node, with_low_k[idx], with_high_k[idx]));
            }
            if with_high_k[idx] < with_low_k[idx] {
                a_real_decrease_happened = true;
            }
        }
        assert!(violations.is_empty(), "higher erodibility raised a draining node: {violations:?}");
        assert!(a_real_decrease_happened, "fixture and k values must produce a measurable difference somewhere");
    }
}
