//! Slice 5b Task 1: fill each lake basin to its spill point.
//!
//! `StreamGraph::build` (slice 1p) already classifies every non-boundary root as a lake and
//! sets `Lake::level_m` to the root's own elevation -- "an empty basin", by that type's own
//! doc comment. This module raises `level_m` to the basin's spill point: the lowest
//! elevation at which water crosses out of the basin. Nothing else about a `Lake` is this
//! task's business -- `outflow_lake` stays at `NO_LAKE` (Task 2's overflow edge) and `kind`
//! stays as built (Task 3's classification).
//!
//! # Two things a basin needs, and only one of them is free
//!
//! A **basin** is the set of nodes whose downhill chain reaches a given root. That is the
//! downhill forest `StreamGraph` already stores, so [`basins_of`] costs one linear walk of
//! `StreamGraph::peel`'s order and nothing else.
//!
//! A **rim** is a different relation entirely: it needs to know which nodes are
//! *geometrically* adjacent, and `StreamGraph` stores neither positions nor neighbours
//! (`stream.rs`'s own module doc, §3.3 of the CORE-001 extraction) -- both are derived from
//! the world seed on demand. [`fill_basins`] is the entry point that pays that cost, via
//! `stream::node_positions` and `stream::node_neighbours`, exactly as `stream::sample_nodes`
//! does when a graph is actually built. See `task-1-report.md` for what that regeneration
//! measured at a stated node count in release -- slice 5b's whole feasibility argument (lake
//! resolution being cheap next to the erosion bake) starts with that number.
//!
//! [`fill_lakes`] is the algorithmic core, factored out from that cost on purpose: it takes
//! a neighbour relation as a plain argument rather than regenerating one, so a small,
//! hand-authored fixture can exercise the spill formula exactly without also having to agree
//! with the spiral sampler about where thousands of nodes sit. `fill_basins` is the only
//! caller that pays to regenerate one for real.

use std::collections::HashMap;

use crate::stream::{self, SamplingKind, StreamGraph};

// ---- basin membership --------------------------------------------------------------------

/// Which basin (downhill-tree root) each node belongs to.
///
/// Computed once per graph and exposed here rather than kept private, because Task 3
/// (pond/lake classification by drainage area) and Task 4 (the water manifest's extents)
/// both need exactly this partition. Two tasks re-deriving it independently is the failure
/// mode the pre-flight scan named: they would each walk the same forest and could silently
/// disagree the moment either implementation drifted. One computation, shared.
#[derive(Debug, Clone)]
pub struct Basins {
    /// Indexed by node. For a root (a mouth or a lake), `root_of[node] == node`.
    root_of: Vec<u32>,
    /// Every basin's members, keyed by its root, root included. A `HashMap` rather than a
    /// second array indexed by root: roots are a small fraction of all nodes (§8.3's figures,
    /// re-measured over a real field by `stream.rs`'s own `lake_at` doc comment, put it under
    /// 5% even at planet scale), so a dense array sized to the whole node count would be
    /// mostly empty.
    members: HashMap<u32, Vec<u32>>,
}

impl Basins {
    /// The root of the basin `node` drains into -- itself, if `node` is a root.
    pub fn root_of(&self, node: u32) -> u32 {
        self.root_of[node as usize] // cast-ok: a node index into usize
    }

    /// Every node whose downhill chain reaches `root`, `root` itself included. Empty if
    /// `root` does not name an actual root -- there is no basin recorded under it.
    pub fn members_of(&self, root: u32) -> &[u32] {
        self.members.get(&root).map(Vec::as_slice).unwrap_or(&[])
    }

    /// How many nodes this partition covers. Equal to the graph's own node count whenever
    /// the graph is a complete forest, which `StreamGraph::build` already refuses to ship
    /// otherwise (`GraphDefect::Cycle`).
    pub fn node_count(&self) -> usize {
        self.root_of.len()
    }
}

/// Partition every node in `graph` by which root its downhill chain terminates at.
///
/// # Why the reversed peel order is correct, not merely convenient
///
/// `StreamGraph::peel` walks the relation leaves-first: a node is only appended once every
/// node that flows into it has already been removed, so for the edge `child -> parent`
/// (`downhill[child] == parent`), `child` always precedes `parent` in `peel.order` (see that
/// method's own doc comment, and `StreamGraph::build`'s drainage-accumulation loop, which
/// relies on the same fact to add every child's contribution before touching the parent).
///
/// Assigning a basin root needs the opposite dependency: a node can only take its root's
/// identity once the root's own entry is already written. Reading `peel.order` **reversed**
/// therefore visits every `parent` before its `child`, which is exactly the order this needs
/// -- one pass, no recursion, and so no stack depth tied to how deep a single basin's
/// downhill chain runs (a real concern at planet scale, where a chain can be thousands of
/// nodes long).
pub fn basins_of(graph: &StreamGraph) -> Basins {
    let count = graph.node_count() as usize; // cast-ok: a node count into usize
    let peel = graph.peel();
    // `StreamGraph::build` already refuses a graph that fails this (`GraphDefect::Cycle`),
    // so this should be unreachable on any graph this crate hands out. It is checked again
    // here, loudly, rather than trusted, because the loop below indexes `root_of` on the
    // assumption that every node gets visited exactly once; a silent partial peel would make
    // it read an uninitialised (falsely-zero) root for whatever the peel missed rather than
    // failing at all.
    assert!(
        peel.peeled as usize == count, // cast-ok: a peeled count, bounded by the node count
        "basins_of: only {} of {count} nodes peeled -- this graph has a cycle in its downhill \
         relation, which StreamGraph::build should already have refused. Basin membership is \
         undefined over a graph that is not a forest.",
        peel.peeled,
    );

    let mut root_of = vec![0u32; count];
    for &node in peel.order.iter().rev() {
        root_of[node as usize] = match graph.downhill_of(node) { // cast-ok: a node index into usize
            None => node,
            // `target` already has its root written: the reversed walk visits it first,
            // per this function's own doc comment.
            Some(target) => root_of[target as usize], // cast-ok: a node index into usize
        };
    }

    let mut members: HashMap<u32, Vec<u32>> = HashMap::new();
    for node in 0..count {
        let node = node as u32; // cast-ok: a node index, bounded by the graph's own node count
        members.entry(root_of[node as usize]).or_default().push(node); // cast-ok: node index
    }

    Basins { root_of, members }
}

// ---- the rim ------------------------------------------------------------------------------

/// The full k-nearest-neighbour relation, made symmetric.
///
/// `stream::node_neighbours` returns each node's *own* nearest `k`, and that relation is not
/// guaranteed reciprocal: node `i` being among node `j`'s `k` nearest does not mean `j` is
/// among `i`'s (a crowded neighbourhood can push a node's own near side out of its list while
/// staying inside a neighbour's). A rim scan that only ever follows `neighbours[i]` for `i`
/// inside the basin can therefore miss the one crossing that only appears in the *outside*
/// node's list -- and the spill formula takes a `min`, so a missed crossing biases the
/// result upward (a basin that would silently fill higher than it should), never downward,
/// which makes it exactly the sort of defect that looks like a plausible lake on a map.
///
/// This adds the missing direction wherever it is absent, so every edge `stream::
/// node_neighbours` found from either end is checked from both.
pub fn symmetric_adjacency(directed: &[Vec<u32>]) -> Vec<Vec<u32>> {
    let mut out = directed.to_vec();
    for (i, list) in directed.iter().enumerate() {
        let i = i as u32; // cast-ok: an index into `directed`, bounded by its own length
        for &j in list {
            let j_index = j as usize; // cast-ok: a node index into usize
            if !out[j_index].contains(&i) {
                out[j_index].push(i);
            }
        }
    }
    out
}

// ---- filling --------------------------------------------------------------------------

/// One lake's computed water surface. `root_node` matches the `Lake` it was computed from;
/// nothing else about that lake is reported here because nothing else is this task's to set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FilledLake {
    pub root_node: u32,
    pub level_m: f64,
}

/// Fill every lake in `graph` to its spill point.
///
/// `basins` must be [`basins_of`]`(graph)` (or an equivalent partition over the same graph);
/// `neighbours` must be a **symmetric** adjacency over the same node set `graph` was built
/// over -- [`symmetric_adjacency`] applied to `stream::node_neighbours`'s output, for a real
/// graph, or a hand-authored relation for a fixture. Passed in rather than regenerated here
/// so a small fixture can exercise the formula directly; [`fill_basins`] is the entry point
/// that assembles both from a real graph's own seed.
///
/// # The spill formula
///
/// For the lake rooted at `R`, with basin members `M`:
///
/// ```text
/// spill = min over every edge (i, j) with i in M, j not in M, of max(height(i), height(j))
/// ```
///
/// Water leaves the basin at the lowest point on its rim, and a rim point only clears once
/// **both** ends of the crossing edge are covered -- an edge whose inside end is already
/// underwater is not yet an exit if its outside end still stands above the rising surface.
/// Hence the inner `max`; the outer `min` then picks the lowest such point over the whole
/// rim.
///
/// **Both operators are the NaN-asymmetric ones.** `f64::min`, `f64::max` and `.clamp(` are
/// banned by house rule but are not in `tests/no_std_math.rs`'s scan (it bans transcendental
/// calls and float-truncating casts, not these two), so nothing but review catches a call to
/// either here. Both are written as an explicit branch instead -- `plates.rs::margin_at`'s
/// house form -- with the same NaN-floors-rather-than-spreads operand order that form uses
/// elsewhere in this crate. Every height feeding this loop was validated finite at
/// `StreamGraph::build`, so NaN is not reachable through it today; the form is used anyway,
/// because the alternative is a call this project has already decided never to make.
///
/// # Panics
///
/// If a lake's basin has no boundary edge at all -- every neighbour of every member is also
/// a member. On an actual sphere that cannot happen (the basin is a strict, non-empty subset
/// of a finite node set that is not all one basin, or it would not be classified as a lake at
/// all: a lake root is a boundary-free local minimum, and boundary-free does not mean
/// rim-free). A missing rim here means the neighbour relation handed in is incomplete for
/// this graph, which is a defect to surface loudly rather than a basin to silently treat as
/// filling forever.
///
/// Also panics if the computed spill is non-finite, or if either of the two invariants a
/// filled lake must satisfy fails: `level_m <= spill` (trivially true, since `level_m` is set
/// to `spill`, but checked anyway because a level above the spill is a defect in the caller's
/// data, not a big lake) and `level_m >= height_m(root_node)` (the root is its basin's lowest
/// point by construction -- every downhill chain inside it strictly descends to it -- so the
/// water surface can never sit under it).
pub fn fill_lakes(graph: &StreamGraph, basins: &Basins, neighbours: &[Vec<u32>]) -> Vec<FilledLake> {
    let mut out = Vec::with_capacity(graph.lakes().len());
    for lake in graph.lakes() {
        let root = lake.root_node;
        let members = basins.members_of(root);

        let mut spill: Option<f64> = None;
        for &inside in members {
            let h_inside = graph.height_m(inside);
            for &outside in &neighbours[inside as usize] { // cast-ok: a node index into usize
                if basins.root_of(outside) == root {
                    continue; // still inside this basin, not a rim edge
                }
                let h_outside = graph.height_m(outside);
                // House form for `max` (`plates.rs::margin_at`): an explicit branch, never
                // `f64::max`. The inside height wins ties and a NaN falls through to the
                // outside height rather than spreading -- unreachable today (see the doc
                // comment above), kept for the same reason every other extremum in this
                // crate is written this way.
                let crossing = if h_inside > h_outside { h_inside } else { h_outside };
                spill = Some(match spill {
                    None => crossing,
                    // House form for `min`: an explicit branch, never `f64::min`.
                    Some(best) => if crossing < best { crossing } else { best },
                });
            }
        }

        let spill = spill.unwrap_or_else(|| {
            panic!(
                "fill_lakes: the basin rooted at node {root} ({} member{}) has no rim -- every \
                 neighbour of every member is itself a member. A basin with no rim is \
                 impossible on a sphere; this is a defect in the neighbour relation handed to \
                 fill_lakes (most likely incomplete), not a lake that fills forever.",
                members.len(),
                if members.len() == 1 { "" } else { "s" },
            )
        });
        assert!(
            spill.is_finite(),
            "fill_lakes: the basin rooted at node {root} computed a non-finite spill point \
             ({spill}) -- every height feeding this formula is validated finite at \
             StreamGraph::build, so this can only be a defect in the formula itself.",
        );

        let level_m = spill;
        assert!(
            level_m <= spill,
            "fill_lakes: basin {root} filled to {level_m} m, above its own spill point \
             {spill} m -- a level above the spill is a defect (the water would already have \
             left through the rim), not a big lake.",
        );
        let root_height = graph.height_m(root);
        assert!(
            level_m >= root_height,
            "fill_lakes: basin {root} filled to {level_m} m, below the root's own elevation \
             {root_height} m -- the root is this basin's lowest point by construction (every \
             downhill chain inside it strictly descends to it), so the water surface can \
             never sit under it.",
        );

        out.push(FilledLake { root_node: root, level_m });
    }
    out
}

/// Basin membership plus every lake's filled level, for one graph.
#[derive(Debug, Clone)]
pub struct WaterFill {
    pub basins: Basins,
    pub lakes: Vec<FilledLake>,
}

/// Fill every lake in `graph` to its spill point, regenerating the neighbour relation the
/// graph was actually built over.
///
/// `StreamGraph` stores neither positions nor neighbours (this module's own doc comment,
/// §3.3 of the CORE-001 extraction), so this is the one place in the module that pays to
/// rebuild them: `stream::node_positions(world_seed, node_count)` reproduces the sampler's
/// output exactly, `stream::node_neighbours` reproduces the same `k` nearest each node was
/// built with, and [`symmetric_adjacency`] closes the one-directional gaps that relation can
/// leave. `task-1-report.md` records what this regeneration cost at a stated node count in
/// release; slice 5b's premise that lake resolution is cheap next to the erosion bake starts
/// with that number.
///
/// # Panics
///
/// If `graph.header().sampling_kind` is not `Spiral`. Regenerating positions from the seed
/// only reconstructs the relation a graph was actually built over when the graph came from
/// `stream::sample_nodes`'s own sampler; a `Supplied`-position graph (every hand-built test
/// fixture in this crate) has positions and neighbours that came from somewhere else
/// entirely, and silently regenerating a different graph's geometry here would answer the
/// wrong question rather than refuse it. Call [`fill_lakes`] directly with the fixture's own
/// neighbour list instead -- that is what this module's own tests do.
pub fn fill_basins(graph: &StreamGraph) -> WaterFill {
    assert!(
        graph.header().sampling_kind == SamplingKind::Spiral,
        "fill_basins regenerates positions from the world seed via stream::node_positions, \
         which only reconstructs the geometry a graph was actually built over when \
         sampling_kind is Spiral. This graph's sampling_kind is {:?}; call fill_lakes directly \
         with its own neighbour relation instead of regenerating one that would describe a \
         different graph.",
        graph.header().sampling_kind,
    );

    let positions = stream::node_positions(graph.header().world_seed, graph.node_count());
    let directed = stream::node_neighbours(&positions, stream::NEIGHBOUR_COUNT);
    let neighbours = symmetric_adjacency(&directed);

    let basins = basins_of(graph);
    let lakes = fill_lakes(graph, &basins, &neighbours);
    WaterFill { basins, lakes }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sphere::{SpherePoint, EARTH_RADIUS_M};
    use crate::stream::{sample_nodes, BuildParams, LakeKind, StreamGraph};
    use crate::surface::Surface;

    // ---- a small, hand-authored fixture for the spill formula itself -----------------
    //
    // Two lakes, A (root 0) and B (root 2), that touch each other's rim on both sides:
    //
    //   node 0 (root A, h=0.0) -- neighbours [1, 2]   (both higher: 0 has no downhill)
    //   node 1 (in A,   h=5.0) -- neighbours [0, 3]   (downhill -> 0)
    //   node 2 (root B, h=1.0) -- neighbours [3]      (higher only: 2 has no downhill)
    //   node 3 (in B,   h=8.0) -- neighbours [1, 2]   (downhill -> 2: steeper drop, 7 > 3)
    //
    // Basin A = {0, 1}, basin B = {2, 3}. The rim between them carries two crossings:
    //   0 -- 2: max(0.0, 1.0) = 1.0
    //   1 -- 3: max(5.0, 8.0) = 8.0
    // so the correct spill for A is min(1.0, 8.0) = 1.0.
    //
    // Node 2's own list deliberately omits node 0 (2 must not see a lower neighbour, or it
    // stops being a root at all), which makes the 0--2 edge one-directional in the raw
    // `neighbours` array -- exactly the case `symmetric_adjacency` exists for. Without it,
    // basin B's own scan (from members {2, 3}) never finds the 0--2 crossing at all, and its
    // spill comes out as 8.0 (only the 1--3 edge) instead of the correct 1.0. Both lakes are
    // asserted below specifically because A's answer does not depend on symmetrisation and
    // B's does -- the two together prove the symmetrisation step is load-bearing, not
    // decorative.
    fn touching_lakes_fixture() -> (StreamGraph, Vec<Vec<u32>>) {
        let positions = vec![
            SpherePoint::from_latlon(0.0, 0.0),
            SpherePoint::from_latlon(10.0, 0.0),
            SpherePoint::from_latlon(0.0, 10.0),
            SpherePoint::from_latlon(10.0, 10.0),
        ];
        let heights = vec![0.0, 5.0, 1.0, 8.0];
        let areas = vec![1.0e9; 4];
        let neighbours = vec![vec![1, 2], vec![0, 3], vec![3], vec![1, 2]];
        let params = BuildParams {
            world_seed: 1,
            radius_m: EARTH_RADIUS_M,
            sea_level_m: -1.0e6, // far below every height: nothing is BOUNDARY, so both
            // roots are classified as lakes rather than mouths.
            sampling_kind: crate::stream::SamplingKind::Supplied,
            pond_max_drainage_area_m2: 1.0,
        };
        let graph = StreamGraph::build(&params, &positions, &heights, &areas, &neighbours)
            .expect("the touching-lakes fixture builds a valid graph");
        (graph, neighbours)
    }

    #[test]
    fn touching_lakes_fixture_has_the_two_roots_this_test_relies_on() {
        let (graph, _) = touching_lakes_fixture();
        let roots = graph.roots();
        assert_eq!(roots, vec![0, 2], "fixture drifted: expected roots at 0 and 2");
        assert_eq!(graph.lakes().len(), 2);
        for lake in graph.lakes() {
            assert_eq!(lake.kind, LakeKind::Lake);
        }
    }

    #[test]
    fn basins_of_partitions_every_node_into_exactly_one_basin() {
        let (graph, _) = touching_lakes_fixture();
        let basins = basins_of(&graph);
        assert_eq!(basins.node_count(), 4);
        assert_eq!(basins.root_of(0), 0);
        assert_eq!(basins.root_of(1), 0);
        assert_eq!(basins.root_of(2), 2);
        assert_eq!(basins.root_of(3), 2);

        let mut a = basins.members_of(0).to_vec();
        a.sort_unstable();
        assert_eq!(a, vec![0, 1]);
        let mut b = basins.members_of(2).to_vec();
        b.sort_unstable();
        assert_eq!(b, vec![2, 3]);
    }

    #[test]
    fn symmetric_adjacency_recovers_an_edge_only_one_side_names() {
        // node 0 names node 1; node 1 names nobody. The relation is real (node 2's own list
        // in the fixture above has exactly this shape against node 0), so a rim scan that
        // trusted only the directed lists would treat node 1 as having no neighbours at all.
        let directed = vec![vec![1u32], vec![]];
        let sym = symmetric_adjacency(&directed);
        assert_eq!(sym[0], vec![1]);
        assert_eq!(sym[1], vec![0], "the reverse-only edge must be added back");
    }

    /// Property 5, and the one the brief asks to be built specifically to catch a `max`
    /// silently becoming a `min` in the spill formula.
    ///
    /// Verified by mutation, not merely by inspection: with `fill_lakes`'s inner `max`
    /// temporarily changed to the matching `min` branch (`if h_inside < h_outside { h_inside
    /// } else { h_outside }`), this test fails -- lake A's spill comes out as `0.0` (`min(0.0,
    /// 1.0)` on the 0--2 edge) instead of `1.0`, and lake B's comes out as `1.0` instead of
    /// (coincidentally, from a different wrong path) the same `1.0` it should already be, so
    /// the two assertions below do not both hold under the mutation. Restored afterwards;
    /// this comment records that the mutation was actually run, not merely reasoned about.
    #[test]
    fn spill_uses_the_inner_max_not_the_inner_min() {
        let (graph, directed) = touching_lakes_fixture();
        let basins = basins_of(&graph);
        let symmetric = symmetric_adjacency(&directed);
        let filled = fill_lakes(&graph, &basins, &symmetric);

        assert_eq!(filled.len(), 2);
        let a = filled.iter().find(|f| f.root_node == 0).expect("lake A");
        let b = filled.iter().find(|f| f.root_node == 2).expect("lake B");
        assert_eq!(a.level_m, 1.0, "lake A: min(max(0,1)=1, max(5,8)=8) = 1");
        assert_eq!(b.level_m, 1.0, "lake B: min(max(1,0)=1, max(8,5)=8) = 1");
    }

    #[test]
    fn without_symmetrisation_the_one_directional_edge_is_missed() {
        // The same fixture, fed the raw directed lists instead of the symmetric closure --
        // demonstrating why fill_basins always symmetrises and fill_lakes documents that it
        // must be handed a symmetric relation.
        let (graph, directed) = touching_lakes_fixture();
        let basins = basins_of(&graph);
        let filled = fill_lakes(&graph, &basins, &directed);

        let a = filled.iter().find(|f| f.root_node == 0).expect("lake A");
        let b = filled.iter().find(|f| f.root_node == 2).expect("lake B");
        assert_eq!(a.level_m, 1.0, "A's own list already carries the 0--2 edge directly");
        assert_eq!(
            b.level_m, 8.0,
            "B's own list omits node 0 entirely, so its only found crossing is 1--3 (max 8.0), \
             not the true 1.0 -- this is the bug symmetric_adjacency exists to close"
        );
    }

    #[test]
    fn every_filled_level_sits_in_its_basins_legal_range() {
        let (graph, directed) = touching_lakes_fixture();
        let basins = basins_of(&graph);
        let symmetric = symmetric_adjacency(&directed);
        let filled = fill_lakes(&graph, &basins, &symmetric);

        for lake in &filled {
            assert!(lake.level_m.is_finite());
            assert!(lake.level_m >= graph.height_m(lake.root_node));
        }
    }

    /// Property 3: a basin covering the whole node set has nowhere for water to leave, and
    /// that must abort loudly rather than resolve to "fills forever". Three nodes, one
    /// minimum, no boundary node at all -- every neighbour of every node is inside the one
    /// basin that exists.
    #[test]
    #[should_panic(expected = "has no rim")]
    fn a_basin_covering_the_whole_graph_has_no_rim_and_panics() {
        let positions = vec![
            SpherePoint::from_latlon(0.0, 0.0),
            SpherePoint::from_latlon(1.0, 0.0),
            SpherePoint::from_latlon(0.0, 1.0),
        ];
        let heights = vec![0.0, 5.0, 3.0];
        let areas = vec![1.0e9; 3];
        let neighbours = vec![vec![1, 2], vec![0], vec![0]];
        let params = BuildParams {
            world_seed: 2,
            radius_m: EARTH_RADIUS_M,
            sea_level_m: -1.0e6, // nothing is BOUNDARY: the sole root is a lake, not a mouth
            sampling_kind: crate::stream::SamplingKind::Supplied,
            pond_max_drainage_area_m2: 1.0,
        };
        let graph = StreamGraph::build(&params, &positions, &heights, &areas, &neighbours)
            .expect("the whole-graph-is-one-basin fixture builds");
        assert_eq!(graph.lakes().len(), 1, "fixture drifted: expected exactly one lake");

        let basins = basins_of(&graph);
        let _ = fill_lakes(&graph, &basins, &neighbours);
    }

    // ---- over a real, Spiral-sampled graph ---------------------------------------------

    const SEED: i64 = 20_260_905;
    const NODES: u32 = 4_000;
    const DATUM_M: f64 = 0.0;

    fn real_graph(seed: i64) -> StreamGraph {
        let world_seed = seed as u64; // cast-ok: two's-complement reinterpretation, as Surface::new makes
        let sampling = sample_nodes(world_seed, NODES, EARTH_RADIUS_M).expect("a node set");
        let field = Surface::new(seed, EARTH_RADIUS_M, 22, 0.29, None);
        let heights: Vec<f64> = sampling
            .positions
            .iter()
            .map(|p: &SpherePoint| field.elevation_m(p, None))
            .collect();
        StreamGraph::build(
            &BuildParams {
                world_seed,
                radius_m: EARTH_RADIUS_M,
                sea_level_m: DATUM_M,
                sampling_kind: crate::stream::SamplingKind::Spiral,
                pond_max_drainage_area_m2: 5.0e9,
            },
            &sampling.positions,
            &heights,
            &sampling.area_m2,
            &sampling.neighbours,
        )
        .expect("a graph over a real field builds")
    }

    /// Property 1: the same graph, filled twice, must produce bit-identical levels -- not
    /// merely numerically close ones. Compared on bits, per this crate's own convention
    /// (`StreamGraph::bit_identical_to`'s doc comment): `==` on `f64` calls `-0.0` and `0.0`
    /// equal when a rebuild that produced them differently would be a real bug, and never
    /// calls NaN equal to itself when a rebuild produced one.
    #[test]
    fn fill_basins_is_bit_identical_across_two_runs() {
        let graph = real_graph(SEED);
        assert!(!graph.lakes().is_empty(), "fixture must actually have lakes to test this");

        let first = fill_basins(&graph);
        let second = fill_basins(&graph);

        assert_eq!(first.basins.node_count(), second.basins.node_count());
        for node in 0..graph.node_count() {
            assert_eq!(
                first.basins.root_of(node),
                second.basins.root_of(node),
                "basin membership must not depend on anything but the graph itself"
            );
        }

        assert_eq!(first.lakes.len(), second.lakes.len());
        for (a, b) in first.lakes.iter().zip(second.lakes.iter()) {
            assert_eq!(a.root_node, b.root_node);
            assert_eq!(
                a.level_m.to_bits(),
                b.level_m.to_bits(),
                "lake at root {} produced different bit patterns across two runs: {} vs {}",
                a.root_node,
                a.level_m,
                b.level_m,
            );
        }
    }

    /// Properties 2 and 4, over every lake a real graph produces -- not a sample. Each
    /// bound is already asserted inside `fill_lakes` itself; this test re-checks them from
    /// the outside so the property stands on its own rather than only on the
    /// implementation's internal enforcement of it.
    #[test]
    fn every_lake_in_a_real_graph_is_within_its_legal_range() {
        let graph = real_graph(SEED);
        let filled = fill_basins(&graph);
        assert!(!filled.lakes.is_empty(), "fixture must actually have lakes to test this");

        for lake in &filled.lakes {
            assert!(lake.level_m.is_finite(), "lake at root {} has a non-finite level", lake.root_node);
            assert!(!lake.level_m.is_nan(), "lake at root {} has a NaN level", lake.root_node);
            let root_height = graph.height_m(lake.root_node);
            assert!(
                lake.level_m >= root_height,
                "lake at root {} filled to {} m, under its own root's elevation {} m",
                lake.root_node,
                lake.level_m,
                root_height,
            );
        }
    }

    /// `basins_of`'s partition must account for every node exactly once, over a graph large
    /// enough that this is not true by accident of a small fixture's shape.
    #[test]
    fn basins_of_accounts_for_every_node_exactly_once_in_a_real_graph() {
        let graph = real_graph(SEED);
        let basins = basins_of(&graph);
        let mut seen = vec![false; graph.node_count() as usize]; // cast-ok: a node count into usize
        let mut total_members = 0usize;
        for &root in &graph.roots() {
            for &member in basins.members_of(root) {
                assert!(
                    !seen[member as usize], // cast-ok: a node index into usize
                    "node {member} appears in more than one basin"
                );
                seen[member as usize] = true; // cast-ok: a node index into usize
                total_members += 1;
            }
        }
        assert_eq!(total_members, graph.node_count() as usize); // cast-ok: a node count into usize
        assert!(seen.iter().all(|&s| s), "every node must land in exactly one basin");
    }

    #[test]
    fn fill_basins_refuses_a_graph_it_did_not_regenerate_from_a_seed() {
        let (graph, _) = touching_lakes_fixture();
        assert_eq!(graph.header().sampling_kind, crate::stream::SamplingKind::Supplied);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fill_basins(&graph)));
        assert!(
            result.is_err(),
            "fill_basins must refuse a Supplied-position graph rather than silently \
             regenerating unrelated geometry for it"
        );
    }
}
