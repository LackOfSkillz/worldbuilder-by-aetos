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
//! # This module holds the strict bit-for-bit contract
//!
//! With `n = 1`, `s^1` is `s` -- no call at all. With `m = 0.5`, `A^0.5` is `sqrt`, which
//! IEEE-754 requires to be *correctly rounded*, unlike `sin`, `cos`, `pow`, `exp`, `log` and
//! the rest of `detmath.rs`, whose own doc records native and WASM libm disagreeing by a bit
//! on roughly 2.4% of sampled inputs. Because the inner loop of the equation this module
//! documents contains no transcendental -- only `sqrt`, multiplication, subtraction and
//! addition -- a test of it compares **bit patterns**, never a tolerance. A tolerance
//! anywhere in this file (or in Tasks 2-4, which build the solver on top of it) is a defect,
//! not a reasonable allowance, because nothing here has an excuse to disagree by even one
//! bit between native and WASM.
//!
//! If a later task needs `pow`, `exp`, `log` or a trig function inside the per-iteration
//! step, the formulation has drifted away from `n = 1, m = 0.5` and this contract no longer
//! holds for it -- that is the moment to come back and change this doc, not to add a
//! tolerance and move on.

use crate::stream::StreamGraph;

/// The area exponent `m` in `dh/dt = u - k * A^m * s^n`.
///
/// Fixed by Cordonnier et al. (2016) and by
/// `docs/design/2026-09-02-mark-2-world-studio.md` §14.1 ("n = 1, m = 0.5"). A spec
/// constant, not a tunable: solving the equation with a different exponent is solving a
/// different equation, so this is named and cited rather than left as a literal `0.5`
/// wherever the solver needs it.
pub const STREAM_POWER_AREA_EXPONENT: f64 = 0.5;

/// The slope exponent `n` in `dh/dt = u - k * A^m * s^n`.
///
/// Fixed at `1` by the same source as [`STREAM_POWER_AREA_EXPONENT`]. At this value `s^n`
/// is `s` itself -- multiplication, not a call -- which is exactly why this module's inner
/// loop contract is the strict bit-for-bit one and not the bounded one `detmath.rs` requires
/// of `sin`/`cos`/`pow`.
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

    fn small_graph() -> StreamGraph {
        let sampling = sample_nodes(SEED, NODES, EARTH_RADIUS_M).expect("a node set");
        let heights = heights_for(SEED, NODES);
        StreamGraph::build(
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
        .expect("a graph over a real field builds")
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
        let a = ErosionParams { uplift_m_per_yr: 1.0e-4, erodibility_per_yr: 2.0e-6 };
        let b = a; // Copy, not a move -- `a` must still be usable below.
        assert_eq!(a, b);

        let different = ErosionParams { uplift_m_per_yr: 1.0e-4, erodibility_per_yr: 3.0e-6 };
        assert_ne!(a, different, "two runs differing only in erodibility must not compare equal");
    }

    #[test]
    fn debug_output_shows_both_fields() {
        let params = ErosionParams { uplift_m_per_yr: 1.0e-4, erodibility_per_yr: 2.0e-6 };
        let text = format!("{params:?}");
        assert!(text.contains("uplift_m_per_yr"));
        assert!(text.contains("erodibility_per_yr"));
    }

    // ---- uniform now, and honestly so ------------------------------------------------------

    #[test]
    fn uplift_and_erodibility_are_uniform_across_every_node() {
        let params = ErosionParams { uplift_m_per_yr: 5.0e-4, erodibility_per_yr: 7.0e-7 };
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
}
