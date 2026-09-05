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
//!
//! # Computing a level and writing it are two different steps
//!
//! [`fill_basins`] (and [`fill_lakes`] underneath it) only *compute* -- they take `&StreamGraph`
//! and hand back a [`WaterFill`]/`Vec<FilledLake>` alongside it, touching nothing. `Lake::level_m`
//! itself is not written until something calls [`apply_levels`] (or the combined
//! [`fill_basins_and_apply`]), which is the only place in this crate that can move it
//! (`StreamGraph::set_lake_level_m`, added for exactly this write-back). Keeping the two apart
//! means the formula stays testable against a plain, unowned `&StreamGraph` fixture; a caller
//! that actually wants `Lake::level_m` to carry the filled value -- which every real caller
//! does -- must reach for the `_and_apply` entry point rather than assume `fill_basins` alone
//! did it.

use std::collections::HashMap;

use crate::sphere::SpherePoint;
use crate::stream::{self, Lake, LakeKind, SamplingKind, StreamGraph, NO_LAKE};

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
/// house form for a `min`/`max` pair, an `if`/`else` rather than a call. The two branches are
/// **not** symmetric in how they treat a NaN, and that asymmetry is intentional only for the
/// outer one:
///
/// - the inner `max` (`if h_inside > h_outside { h_inside } else { h_outside }`) floors a NaN
///   `h_inside` to `h_outside` (the comparison is false, so the `else` arm runs), but a NaN
///   `h_outside` **propagates**: the comparison is still false, so the `else` arm returns the
///   NaN itself. This is the opposite of `plates.rs:266-268`'s own stated reason for choosing
///   its operand order ("a NaN ratio saturates ... rather than propagating") -- that guarantee
///   holds for one operand of this `max`, not both.
/// - the outer `min` (`if crossing < best { crossing } else { best }`) discards a NaN
///   `crossing` cleanly (false comparison keeps `best`), so a NaN that reaches the outer
///   accumulator is dropped rather than spread.
///
/// Put together: a NaN on the very first boundary edge scanned poisons `spill` outright and is
/// caught loudly by the `is_finite` check below; a NaN surfacing from `h_outside` on any
/// *later* edge is silently discarded by the outer `min` once a finite `spill` already exists.
/// Neither behaviour is wrong -- every height feeding this loop is validated finite at
/// `StreamGraph::build`, so none of it is reachable today -- but it is not the uniform
/// "NaN floors" story a reader skimming `plates.rs`'s own comment might expect, so it is
/// spelled out rather than claimed away.
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
/// Also panics if `neighbours.len()` does not match `graph.node_count()`, if the computed
/// spill is non-finite, or if `level_m >= height_m(root_node)` fails (the root is its basin's
/// lowest point by construction -- every downhill chain inside it strictly descends to it --
/// so the water surface can never sit under it). There is no separate `level_m <= spill` check:
/// `level_m` is *defined* as `spill` in this function, so that comparison would be `x <= x` and
/// could never fail under any mutation of the formula -- Ruling 2's cap has nothing to enforce
/// until Task 2 gives a lake a second way to reach a level (via an overflow chain), at which
/// point a real check belongs here.
pub fn fill_lakes(graph: &StreamGraph, basins: &Basins, neighbours: &[Vec<u32>]) -> Vec<FilledLake> {
    // Two comparisons, once per call, not once per node: the same shape 5a settled on for a
    // release-time bounds check on a caller-supplied slice
    // (`erosion.rs`'s own length checks ahead of its per-node loops). Without it, a
    // `neighbours` shorter than the graph's node count gives a raw index-out-of-bounds deep
    // inside the loop below instead of a panic that names the actual mismatch.
    assert_eq!(
        neighbours.len(),
        graph.node_count() as usize, // cast-ok: a node count into usize
        "fill_lakes: neighbours has {} entries but the graph has {} nodes -- every node needs \
         an entry (an empty one is fine) for the rim scan to index safely.",
        neighbours.len(),
        graph.node_count(),
    );
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
                // `f64::max`. The inside height wins ties; a NaN `h_inside` falls through to
                // `h_outside`, but a NaN `h_outside` propagates instead -- this function's own
                // doc comment spells out why that asymmetry is fine here (unreachable today,
                // and caught loudly if it ever were).
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

        // No separate `level_m <= spill` check: `level_m` is *defined* as `spill` two lines
        // below, so that comparison would be `x <= x`, provably unable to fail under any
        // mutation of the formula above it. See this function's own doc comment ("Panics")
        // for why that check is deferred rather than kept as decoration.
        let level_m = spill;
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

/// Write every computed level back onto `graph`'s own `Lake` table.
///
/// `fill_basins`/`fill_lakes` only compute; nothing calls `StreamGraph::set_lake_level_m`
/// until this does. A graph a caller has not run this over still reads back exactly as
/// `StreamGraph::build` left it -- `level_m` at the root's own elevation, "an empty basin"
/// per `Lake::level_m`'s own doc comment -- regardless of how many `WaterFill`s have been
/// computed from it on the side.
///
/// # Panics
///
/// If `lakes` names a `root_node` that `graph.lakes()` has no record of. `fill_lakes` always
/// produces one `FilledLake` per entry in `graph.lakes()` (see its own loop), so this should
/// be unreachable for a `lakes` slice that actually came from `fill_lakes`/`fill_basins`
/// over this same `graph` -- reachable only by handing this a mismatched pair, which is a
/// caller error to surface loudly rather than silently skip.
pub fn apply_levels(graph: &mut StreamGraph, lakes: &[FilledLake]) {
    for filled in lakes {
        let found = graph.set_lake_level_m(filled.root_node, filled.level_m);
        assert!(
            found,
            "apply_levels: no lake recorded at root {} -- this FilledLake did not come from \
             fill_lakes/fill_basins over this graph.",
            filled.root_node,
        );
    }
}

/// The production entry point that leaves nothing on the side: [`fill_basins`], then
/// [`apply_levels`] over its result, so `graph.lakes()` reads back with `level_m` actually
/// raised rather than only a `WaterFill` a caller might forget to apply. Hands back the basin
/// partition alone, since the lake levels are now sitting on `graph` itself where `Lake`'s own
/// doc comment says they belong.
pub fn fill_basins_and_apply(graph: &mut StreamGraph) -> Basins {
    let WaterFill { basins, lakes } = fill_basins(graph);
    apply_levels(graph, &lakes);
    basins
}

// ---- Task 2: the lake super-graph and its overflow edges --------------------------------
//
// Every lake gets **at most one** outflow edge: the basin on the far side of its own lowest
// rim crossing -- the exact same crossing whose height Task 1 already wrote into
// `Lake::level_m`. That crossing's target is either another lake (a real super-graph edge)
// or a mouth's basin (the sea): a lake spilling straight into the sea is real overflow, but
// not a *lake* super-graph edge, so `outflow_lake` stays at `NO_LAKE` for it -- the
// "terminal lake" case Property 6 names.
//
// # Why this never needs to compare `height_m(root)` to anything
//
// The module doc above this line, and the brief this task was written against, both warn
// against ordering two basins by `height_m(root_node)` -- a basin's root can sit at a low
// elevation while its filled `level_m` sits high, and vice versa, so root elevation is not a
// safe proxy for "which of two adjacent basins is downstream". This module's resolution
// never makes that comparison at all: `resolve_outflow_target` below re-scans the same
// `max(h_inside, h_outside)` rim formula `fill_lakes` used, over the *members'* actual
// elevations, and picks the minimum exactly as `fill_lakes` does. It is, by construction,
// the same quantity that produced `level_m` in the first place -- there is no second,
// independent notion of "basin order" for a root-height comparison to be substituted into.
//
// That is also why `resolve_outflow_edges` below asserts, for every lake, that the crossing
// it finds is bit-identical to the `level_m` Task 1 already applied: the two are defined to
// be the same computation, so any drift -- including the specific mistake of substituting a
// basin's root elevation for a member's actual elevation somewhere in the scan -- shows up
// immediately as that assertion firing, rather than as a target that merely looks plausible.
// `outflow_direction_follows_level_not_root_height` (this module's tests) proves the
// assertion is load-bearing by performing exactly that substitution and watching it fail.
//
// # `Lake::level_m` is Task 1's field -- except for a merged plateau
//
// The brief fences `level_m` off as Task 1's to write and this task's to leave alone. That
// fence is lifted in exactly one case: `merge_tied_plateaus` (review Finding 2) revises
// `level_m` for the members of a tied plateau, because Task 1's per-basin minimum is
// provably incomplete for exactly that case -- two basins whose cheapest exit is each other
// have not actually found their true outlet, and the union's own rim can sit strictly higher
// than either basin's tied crossing. This is not scope creep: the cycle a tied plateau
// produces is a defect in Task 1's per-basin computation that Task 2's own acyclicity check
// is what surfaces. Every lake merging does not touch keeps the `level_m` `fill_lakes`
// computed, untouched.

/// One rim crossing from a basin's members out to a neighbouring basin: the elevation at
/// which water crosses, and which basin it lands in. `fill_lakes` computes the same
/// `max(h_inside, h_outside)` value per boundary edge but keeps only the minimum, discarding
/// which basin produced it (Task 1's own module doc, and `task-1-report.md`'s hand-off
/// note); this is the differently-shaped record Task 2 needs instead -- the target survives,
/// not just the elevation.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Crossing {
    level_m: f64,
    target_root: u32,
}

/// Re-scan basin `root`'s rim and return its lowest crossing -- the same value and the same
/// scan order `fill_lakes` uses for `level_m`, with the target basin kept this time.
///
/// **Ranking, not sorting**: this is a running minimum over crossings (the house explicit-
/// branch form, `plates.rs::margin_at`'s model, never `f64::min`), exactly the same "min over
/// crossings" shape `fill_lakes`'s own doc comment names and warns is banned-op territory.
/// Ties at the minimum are broken by scan order (basin members ascending by index, each
/// member's neighbour list in the order `neighbours` gives it) via a strict `<` -- the first
/// candidate to reach a given value keeps it, never replaced by an equal later one -- which
/// is deterministic because both source orders already are.
///
/// # Panics
///
/// If the basin has no rim at all. `fill_lakes` already panics on exactly this condition
/// (over the same `basins`/`neighbours` pair, since both functions scan the same relation)
/// so reaching this panic instead of that one would itself indicate the two scans have
/// fallen out of step.
fn lowest_crossing(graph: &StreamGraph, basins: &Basins, neighbours: &[Vec<u32>], root: u32) -> Crossing {
    let mut best: Option<Crossing> = None;
    for &inside in basins.members_of(root) {
        let h_inside = graph.height_m(inside);
        for &outside in &neighbours[inside as usize] { // cast-ok: a node index into usize
            let target_root = basins.root_of(outside);
            if target_root == root {
                continue; // still inside this basin, not a rim edge
            }
            let h_outside = graph.height_m(outside);
            // House form for `max` (`plates.rs::margin_at`): an explicit branch, never
            // `f64::max`. Matches `fill_lakes`'s own inner `max` bit-for-bit -- same operand
            // order, same tie rule -- so the value found here can be compared against
            // `Lake::level_m` and expected to agree exactly, not merely approximately.
            let crossing_level = if h_inside > h_outside { h_inside } else { h_outside };
            best = Some(match best {
                None => Crossing { level_m: crossing_level, target_root },
                // House form for `min`: an explicit branch, never `f64::min`. Strict `<`
                // keeps the first-scanned candidate on a tie, which is what makes the target
                // deterministic without a second sort key.
                Some(current) => if crossing_level < current.level_m {
                    Crossing { level_m: crossing_level, target_root }
                } else {
                    current
                },
            });
        }
    }
    best.unwrap_or_else(|| {
        panic!(
            "lowest_crossing: the basin rooted at node {root} has no rim -- fill_lakes should \
             already have panicked over this same basins/neighbours pair before this function \
             ever ran, so reaching this instead means the two scans have fallen out of step."
        )
    })
}

/// One lake's resolved overflow edge, computed but not yet written to `graph`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LakeOutflow {
    pub root_node: u32,
    /// `NO_LAKE` for a terminal lake -- one whose lowest crossing lands in a mouth's basin
    /// (the sea) rather than another lake's. Never equal to `root_node` (a lake's own basin
    /// is excluded from its own rim scan by construction, so a self-edge cannot arise), and
    /// whenever it is not `NO_LAKE` it names a real lake root, never an arbitrary node index
    /// or a mouth's root.
    pub outflow_lake: u32,
    /// `Some(new_level)` when `merge_tied_plateaus` revised this lake's level because it
    /// was part of a tied plateau -- Task 1's per-basin minimum understates a merged pair's
    /// true level exactly in that case (see that function's own doc comment). `None` for
    /// every lake merging did not touch: `Lake::level_m` already carries the right value
    /// from `fill_lakes`, and `apply_outflows` leaves it untouched.
    pub revised_level_m: Option<f64>,
}

/// Resolve every lake's overflow edge in `graph`: the basin across its lowest rim crossing,
/// or `NO_LAKE` when that crossing leads to a mouth's basin instead of another lake's.
///
/// `basins` must be `basins_of(graph)` (or an equivalent partition over the same graph)
/// -- **reuse the partition a caller already has** (from `fill_basins`/
/// `fill_basins_and_apply`) rather than recomputing it; `neighbours` must be the same
/// **symmetric** relation `fill_lakes` was given when `graph`'s lake levels were filled, and
/// is asserted symmetric here rather than merely documented as such (review Finding 5: the
/// acyclicity proof this module relies on needs the same rim edge to be visible from both
/// basins it separates, and a caller passing an asymmetric relation to this function while
/// `fill_lakes` got a correctly-symmetrised one would sail through the bit-identical
/// consistency check below while quietly invalidating that proof).
///
/// # Every result is checked against the level Task 1 already applied
///
/// For each lake, the crossing found here must be **bit-identical** to `Lake::level_m` as
/// `graph` already carries it -- both are the same `min over crossings of max(h_inside,
/// h_outside)` formula, over the same basin, so they cannot legitimately differ. This is not
/// a redundant sanity check kept out of caution: it is what turns a wrong operand (a basin's
/// root elevation substituted for a member's actual elevation anywhere in the scan, the exact
/// class of bug this module's own doc comment warns `height_m(root)` invites) into an
/// immediate, loud failure instead of a target that merely looks plausible. See
/// `outflow_direction_follows_level_not_root_height_regression_guard` for the mutation that
/// proves this, and `ranking_crossings_by_target_root_height_is_wrong` for the *other*
/// mutation the brief actually names -- ranking by the target's root elevation instead of by
/// crossing level -- which this consistency check does **not** catch on a graph where every
/// lake has only one external candidate (review Finding 4); the second fixture exists
/// because the first one, alone, was not a discriminating test of that specific mistake.
///
/// # Tied plateaus are merged, not cut
///
/// A cycle in the naive per-lake resolution below is not a tie-break problem -- it is
/// evidence the tied lakes are one under-filled body of water. `merge_tied_plateaus` (run
/// unconditionally, after every lake's independent candidate is found) unions every member of
/// a detected plateau and re-scans the union's own rim, which is what actually finds the
/// group's true outflow and its true, possibly higher, level. See that function's own doc
/// comment for the algebraic argument and the physical reasoning both.
///
/// # Panics
///
/// If any lake's re-scanned crossing does not bit-match its already-applied `level_m` (see
/// above), if `neighbours.len() != graph.node_count()`, or if `neighbours` is not symmetric.
pub fn resolve_outflow_edges(
    graph: &StreamGraph,
    basins: &Basins,
    neighbours: &[Vec<u32>],
) -> Vec<LakeOutflow> {
    assert_eq!(
        neighbours.len(),
        graph.node_count() as usize, // cast-ok: a node count into usize
        "resolve_outflow_edges: neighbours has {} entries but the graph has {} nodes -- every \
         node needs an entry (an empty one is fine) for the rim scan to index safely.",
        neighbours.len(),
        graph.node_count(),
    );
    assert_symmetric(neighbours);

    let mut out = Vec::with_capacity(graph.lakes().len());
    for lake in graph.lakes() {
        let root = lake.root_node;
        let crossing = lowest_crossing(graph, basins, neighbours, root);

        assert_eq!(
            crossing.level_m.to_bits(),
            lake.level_m.to_bits(),
            "resolve_outflow_edges: the re-scanned crossing for lake {root} is {} m, but \
             Lake::level_m already carries {} m from fill_lakes -- these are defined to be the \
             same computation over the same basin, so any difference (including the class of \
             bug this module warns about: a basin's root elevation used in place of a member's \
             actual elevation somewhere in the scan) is a defect in this function, not in \
             fill_lakes.",
            crossing.level_m,
            lake.level_m,
        );

        // A lake's own basin is excluded from its own rim scan (`lowest_crossing` skips any
        // `outside` whose root is `root`), so `target_root == root` cannot arise here -- no
        // lake can point at itself.
        assert_ne!(
            crossing.target_root, root,
            "resolve_outflow_edges: lake {root}'s own basin scan found itself across its own \
             rim -- lowest_crossing's basin-membership skip should make this unreachable.",
        );

        // The target is a lake only when `graph.lakes()` has a record for it; a mouth's root
        // has none (every root is exactly one of mouth or lake, `GraphDefect`'s own
        // "neither"/"both" pair). Overflowing straight into the sea is real, but it is not a
        // *lake* super-graph edge -- Property 6's terminal-lake case.
        let outflow_lake = if graph.lake_at(crossing.target_root).is_some() {
            crossing.target_root
        } else {
            NO_LAKE
        };

        out.push(LakeOutflow { root_node: root, outflow_lake, revised_level_m: None });
    }
    merge_tied_plateaus(graph, basins, neighbours, &mut out);
    out
}

/// Every entry in `neighbours[i]` must be reciprocated: `j` in `neighbours[i]` implies `i` in
/// `neighbours[j]`. `symmetric_adjacency` is the one function in this module that produces
/// a relation satisfying this by construction; this asserts a caller actually handed one in,
/// rather than trusting the doc comment alone (review Finding 5).
///
/// `O(n*k)` lookups at `O(k)` each via `.contains` -- the same cost `symmetric_adjacency`
/// itself already accepts for the same reason (its own doc comment: negligible at this
/// crate's `k = 8`, and not the bottleneck against the neighbour regeneration this module
/// already pays for).
///
/// # Panics
///
/// If any edge is one-directional.
fn assert_symmetric(neighbours: &[Vec<u32>]) {
    for (i, list) in neighbours.iter().enumerate() {
        let i = i as u32; // cast-ok: an index into `neighbours`, bounded by its own length
        for &j in list {
            let j_index = j as usize; // cast-ok: a node index into usize
            assert!(
                j_index < neighbours.len() && neighbours[j_index].contains(&i),
                "assert_symmetric: node {i} names {j} as a neighbour, but {j} does not name \
                 {i} back -- resolve_outflow_edges requires a symmetric relation \
                 (symmetric_adjacency produces one; a raw directed relation from \
                 stream::node_neighbours does not).",
            );
        }
    }
}

/// Peel a lake-outflow relation leaves-first, generically over anything that names a root and
/// an outflow target -- `stream.rs::peel()`'s own "the forest test" applied one level up, over
/// `outflow_lake` instead of `StreamGraph::downhill`. Shared by `assert_lake_graph_acyclic`
/// (the applied `Lake` table, as a defensive re-check) and `merge_tied_plateaus`
/// (freshly-resolved edges, before anything is written back). Returns the index (into
/// `roots`/`outflow_of`) of every entry that did **not** peel off -- the union of every cycle
/// present, empty if none.
///
/// # Panics
///
/// If any non-sentinel entry in `outflow_of` names a value absent from `roots` (Property 4).
fn peel_lake_relation(roots: &[u32], outflow_of: &[u32]) -> (HashMap<u32, usize>, Vec<usize>) {
    let index_of: HashMap<u32, usize> =
        roots.iter().enumerate().map(|(index, &root)| (root, index)).collect();
    let count = roots.len();

    let mut indegree = vec![0u32; count];
    for &outflow in outflow_of {
        if outflow == NO_LAKE {
            continue;
        }
        let target_index = *index_of.get(&outflow).unwrap_or_else(|| {
            panic!(
                "peel_lake_relation: outflow_lake {outflow} names no lake root in this table \
                 -- Property 4 (every non-sentinel outflow_lake names a real lake root) is \
                 violated.",
            )
        });
        indegree[target_index] += 1;
    }

    let mut order: Vec<usize> = (0..count).filter(|&i| indegree[i] == 0).collect();
    let mut head = 0usize;
    while head < order.len() {
        let i = order[head];
        head += 1;
        let outflow = outflow_of[i];
        if outflow == NO_LAKE {
            continue;
        }
        // Already validated above: every non-sentinel entry is a key in `index_of`.
        let target_index = index_of[&outflow];
        indegree[target_index] -= 1;
        if indegree[target_index] == 0 {
            order.push(target_index);
        }
    }

    let peeled: std::collections::HashSet<usize> = order.into_iter().collect();
    let stuck: Vec<usize> = (0..count).filter(|i| !peeled.contains(i)).collect();
    (index_of, stuck)
}

/// One live group in `merge_tied_plateaus`'s working set: either an as-yet-untouched single
/// lake, or the accumulated result of merging two or more. `target_root` is `NO_LAKE` or a
/// raw basin root -- the same namespace `LakeOutflow::outflow_lake` uses -- so a group's
/// current level and target are exactly what `edges` would read back if finalised right now.
struct ActiveGroup {
    lake_roots: Vec<u32>,
    node_members: Vec<u32>,
    level_m: f64,
    target_root: u32,
}

/// Merge every tied group among freshly-resolved `edges`, in place, before anything is
/// written back to `graph`. Called unconditionally from `resolve_outflow_edges`.
///
/// # One bit-identical surface level, connected, is one body (Ruling 7)
///
/// `lowest_crossing`'s target always has a level at or below the source's -- the same
/// physical edge that produces the source's spill is itself one of the target's own
/// candidate crossings, so the target's own minimum cannot exceed it (Property 5, and the
/// reasoning `resolve_outflow_edges`'s own doc comment gives for it). Following outflow edges
/// therefore never *raises* the level, so **any two lakes connected by an outflow edge whose
/// levels are bit-identical are one body of water**, whether or not that edge closes into a
/// cycle. A cycle is the special case where the whole tied set drains into itself; a lake
/// that ties with its own target while that target goes on to drain elsewhere at a *lower*
/// level is the same "one body" fact without a cycle to make a peel notice it. Fix-round
/// review Finding 4: an earlier version of this function only ever merged cycles, so a tied
/// edge that happened to drain onward in a chain -- rare, but real (measured: 2 of 203 lakes
/// at N = 30,000, 3 of 1,024 at N = 100,000) -- was left as two lakes reporting one body
/// between them. Grouping now runs over *every* tied edge, cyclic or not.
///
/// Treating either shape as two separate lakes with an arbitrary tie-break (this module's own
/// first-round fix, `break_cycles`, reviewed and rejected) is wrong in a way that is not
/// merely cosmetic: when two basins' cheapest exits are each other (or a chain member's
/// cheapest exit ties with what it drains into), *neither has actually found its true
/// outlet*. A basin that shares its cheapest exit with a neighbour at the identical level has
/// strictly more capacity than Task 1's independent per-basin minimum credits it with -- the
/// group's *true* outflow is whatever the **union** of their members spills into once the
/// shared internal saddle no longer counts as an exit, and that union's rim can sit strictly
/// higher than any single member's own tied crossing. Cutting one lake to `NO_LAKE`
/// fabricates a terminal lake at a level the basin does not actually hold and discards the
/// merged body's real downstream continuation entirely; this task's own real-graph tests
/// found this is not a rare case (see the task report's re-derived Finding 3 figures).
///
/// # The algorithm, and two bugs its earlier drafts had
///
/// The working set is one `ActiveGroup` per **currently live** group, starting one per lake
/// (using the per-lake resolution `edges` already carries in from the loop above, before this
/// function does anything). Each pass: union live groups `i` and `j` whenever `i`'s
/// `target_root` names a lake root `root_to_group` currently maps to `j`, and both groups'
/// `level_m` are bit-identical. Every resulting union-find component of size two or more is
/// **replaced by one new `ActiveGroup`**: its members are the union of the old groups'
/// members, its rim is re-scanned **excluding internal edges** (an `outside` node already
/// inside the union is not a rim crossing, even though it would have been one for any single
/// old group alone) using the same `max(h_inside, h_outside)` formula every other rim scan in
/// this module uses, and the winning crossing becomes the new group's `level_m`/`target_root`.
/// The old groups are retired (removed from the live set, `root_to_group` repointed at the
/// new one) rather than merely re-labelled -- this is what makes "no new ties found" a
/// well-defined stopping condition, and it is why this draft does not repeat the first
/// draft's mistake (below).
///
/// **First draft (this task's first attempt at this fix-round finding): re-derived each
/// pass's union from scratch via `basins.members_of(root)` on whatever roots were touched
/// *that pass*, which drops any member accumulated by an *earlier* pass's merge.** A lake `B`
/// merged into representative `A` in pass 1 is not `A`'s own basin, so a pass-2 tie between
/// `A` and some `C` re-scanned only `A`'s and `C`'s raw members, excluding `B`'s -- which can
/// make an edge into `B`'s territory look external when it is not, letting the identical tie
/// regenerate forever. Measured directly: at a real seed and node count this looped without
/// terminating, the same 22-lake stuck set recurring pass after pass with no progress.
///
/// **Second draft (generalising cycles to arbitrary ties, this round): kept every original
/// lake index live forever, including a group's own already-absorbed non-representative
/// members, and re-ran a fresh union-find over *all* of them every pass.** A representative
/// and its non-representative both carry the identical revised level permanently (that tie
/// is not new information, it is the merge that already happened), so a plain "is `i` tied to
/// its target" scan re-discovers that same pair as a "component" on every subsequent pass,
/// tripping the progress guard (below) immediately -- caught before shipping by the very
/// termination assertion this finding asked for, on the module's own two-lake merge fixture
/// (`pass 3` over 2 lakes). The `ActiveGroup` working set fixes this by construction: a merge
/// retires its inputs, so there is exactly one live entry per group at all times and nothing
/// for a stable, already-resolved tie to be rediscovered as.
///
/// # Why termination is guaranteed, not merely observed, and the guard that backs it up
///
/// Live groups only ever *decrease* in count: a union-find pass either finds zero components
/// of size two or more (and the loop returns) or finds at least one, which replaces two or
/// more live groups with exactly one, strictly reducing the live count. The live count starts
/// at `edges.len()` and is bounded below by `1`, so there are at most `edges.len() - 1`
/// productive passes. **That argument is enforced, not merely documented**: `max_passes`
/// below turns a violation (which would mean this function's own accounting broke -- a
/// retired group's index reappearing as live, or `root_to_group` pointing at a stale index)
/// into a named panic rather than a hang. The fix-round review demonstrated the hang directly
/// (a 10-node fixture, killed at 240 s) against the membership-losing first draft; this
/// assertion is what stands between a future regression and a repeat of that.
///
/// # Panics
///
/// If more passes run than `edges.len()` without reaching a fixed point. If a union's own rim
/// is empty -- every neighbour of every member of the merged union is itself inside the
/// union, meaning the union covers the graph's entire node set, the same "impossible on an
/// actual sphere" case `fill_lakes`' own no-rim panic describes for a single basin (see that
/// function's doc comment); reachable only on a fixture deliberately shaped that way (this
/// crate's own `touching_lakes_fixture` is exactly such a fixture, which is why this module's
/// cycle-handling tests do not use it for the merge-with-a-real-outlet case).
fn merge_tied_plateaus(
    graph: &StreamGraph,
    basins: &Basins,
    neighbours: &[Vec<u32>],
    edges: &mut Vec<LakeOutflow>,
) {
    fn find(parent: &mut [usize], x: usize) -> usize {
        if parent[x] != x {
            parent[x] = find(parent, parent[x]);
        }
        parent[x]
    }
    fn union(parent: &mut [usize], a: usize, b: usize) {
        let ra = find(parent, a);
        let rb = find(parent, b);
        if ra != rb {
            // Lower index always becomes the union-find root -- deterministic, and
            // irrelevant to the eventual lake representative (chosen by `root_node` at
            // finalisation), only to which slot this bookkeeping structure happens to use.
            let (lo, hi) = if ra < rb { (ra, rb) } else { (rb, ra) };
            parent[hi] = lo;
        }
    }

    let index_of_root: HashMap<u32, usize> =
        edges.iter().enumerate().map(|(i, e)| (e.root_node, i)).collect();

    // The live working set: one `ActiveGroup` per lake initially, using the per-lake
    // resolution `resolve_outflow_edges`'s own loop already computed and left in `edges`.
    // `root_to_group` maps every lake root ever seen to whichever live slot currently holds
    // it -- kept exact on every merge (repointed for every member of a retired group, not
    // only its representative), which is what lets a later pass's tie-check resolve a target
    // root to its *current* group regardless of how many times that group has already grown.
    let mut active: Vec<Option<ActiveGroup>> = Vec::with_capacity(edges.len());
    let mut root_to_group: HashMap<u32, usize> = HashMap::with_capacity(edges.len());
    for e in edges.iter() {
        let level_m = graph
            .lake_at(e.root_node)
            .expect("resolve_outflow_edges only ever builds one LakeOutflow per real lake")
            .level_m;
        let slot = active.len();
        active.push(Some(ActiveGroup {
            lake_roots: vec![e.root_node],
            node_members: basins.members_of(e.root_node).to_vec(),
            level_m,
            target_root: e.outflow_lake,
        }));
        root_to_group.insert(e.root_node, slot);
    }

    let max_passes = edges.len() + 1;
    let mut pass = 0usize;

    loop {
        pass += 1;
        assert!(
            pass <= max_passes,
            "merge_tied_plateaus: exceeded {max_passes} passes over {} lakes without \
             reaching a fixed point. Termination is guaranteed by construction (at most \
             lakes().len() - 1 productive passes, since every pass that merges anything \
             strictly reduces the live group count), so this means the live-group accounting \
             itself has broken -- most likely a retired slot treated as live, or \
             `root_to_group` pointing at a stale slot. This assertion exists so that failure \
             is a named panic here, not a hang in CI (the fix-round review demonstrated the \
             hang directly on a 10-node fixture built against an earlier, membership-losing \
             draft of this function).",
            edges.len(),
        );

        let live: Vec<usize> = (0..active.len()).filter(|&i| active[i].is_some()).collect();
        let mut parent: Vec<usize> = (0..active.len()).collect();
        for &i in &live {
            let group = active[i].as_ref().expect("i is live");
            if group.target_root == NO_LAKE {
                continue;
            }
            let Some(&j) = root_to_group.get(&group.target_root) else {
                // The target is not (and was never) a lake root -- a mouth's basin. No tie
                // is possible against something that carries no tracked level.
                continue;
            };
            if i == j {
                continue; // unreachable: a group's own rim scan excludes its own members
            }
            let level_i = active[i].as_ref().expect("i is live").level_m;
            let level_j = active[j].as_ref().expect("j is live").level_m;
            if level_i.to_bits() == level_j.to_bits() {
                union(&mut parent, i, j);
            }
        }

        // Group live slots by union-find root without iterating a HashMap: a stable sort by
        // the (already-computed) find-root is fully deterministic and needs no hashing.
        let find_of: Vec<usize> = live.iter().map(|&i| find(&mut parent, i)).collect();
        let mut order: Vec<usize> = (0..live.len()).collect();
        order.sort_by_key(|&k| find_of[k]);

        let mut any_merge = false;
        let mut start = 0usize;
        while start < order.len() {
            let mut end = start + 1;
            while end < order.len() && find_of[order[end]] == find_of[order[start]] {
                end += 1;
            }
            let component: Vec<usize> = order[start..end].iter().map(|&k| live[k]).collect();
            start = end;
            if component.len() < 2 {
                continue;
            }
            any_merge = true;

            let mut all_lake_roots: Vec<u32> = Vec::new();
            let mut all_node_members: Vec<u32> = Vec::new();
            for &slot in &component {
                let group = active[slot].take().expect("component members are live");
                all_lake_roots.extend(group.lake_roots);
                all_node_members.extend(group.node_members);
            }
            let union_set: std::collections::HashSet<u32> = all_node_members.iter().copied().collect();

            // Re-scan the union's own rim, excluding edges whose outside end is itself part
            // of the union (an edge between two group members is internal, however many
            // passes ago either side joined, not a rim crossing).
            let mut best: Option<Crossing> = None;
            for &inside in &all_node_members {
                let h_inside = graph.height_m(inside);
                for &outside in &neighbours[inside as usize] { // cast-ok: a node index into usize
                    if union_set.contains(&outside) {
                        continue;
                    }
                    let target_root = basins.root_of(outside);
                    let h_outside = graph.height_m(outside);
                    // House form for `max`/`min`, matching `lowest_crossing` bit-for-bit.
                    let crossing_level = if h_inside > h_outside { h_inside } else { h_outside };
                    best = Some(match best {
                        None => Crossing { level_m: crossing_level, target_root },
                        Some(current) => if crossing_level < current.level_m {
                            Crossing { level_m: crossing_level, target_root }
                        } else {
                            current
                        },
                    });
                }
            }
            let winner = best.unwrap_or_else(|| {
                panic!(
                    "merge_tied_plateaus: the union of {} lakes ({:?}) has no rim -- every \
                     neighbour of every member is itself inside the union, meaning the union \
                     covers the graph's entire node set. Impossible on an actual sphere \
                     (fill_lakes' own no-rim panic makes the same argument for a single \
                     basin); this is a fixture shaped to be one closed system with no outlet \
                     at all, not a real planet.",
                    all_lake_roots.len(),
                    all_lake_roots,
                )
            });
            let target_root = if graph.lake_at(winner.target_root).is_some() {
                winner.target_root
            } else {
                NO_LAKE
            };

            let new_slot = active.len();
            for &root in &all_lake_roots {
                root_to_group.insert(root, new_slot);
            }
            active.push(Some(ActiveGroup {
                lake_roots: all_lake_roots,
                node_members: all_node_members,
                level_m: winner.level_m,
                target_root,
            }));
        }

        if !any_merge {
            break;
        }
        // Loop again: a merged group's new outflow can itself tie with another lake at the
        // identical new level, which this pass alone would not have resolved.
    }

    // Finalise: every live group of size two or more writes back a representative (smallest
    // `root_node`, deterministic and unique) plus every other member pointing at it, and a
    // revised level for all of them. A group that was never merged (still a lone singleton)
    // needs no write at all -- `edges` already carries exactly what `resolve_outflow_edges`'s
    // per-lake loop computed for it.
    for group in active.into_iter().flatten() {
        if group.lake_roots.len() < 2 {
            continue;
        }
        let representative =
            *group.lake_roots.iter().min().expect("a merged group's roots are never empty");
        for &lake_root in &group.lake_roots {
            let i = index_of_root[&lake_root];
            edges[i].revised_level_m = Some(group.level_m);
            edges[i].outflow_lake =
                if lake_root == representative { group.target_root } else { representative };
        }
    }
}

/// Peel the lake super-graph leaves-first and refuse silently swallowing what does not come
/// off. Runs over the `Lake` table *after* `apply_outflows` has written every edge, as a
/// defensive re-check that `merge_tied_plateaus` (already run inside
/// `resolve_outflow_edges`) actually left an acyclic result -- "assert it over every graph
/// you build", not on a sample, per this task's own brief, and not merely trusted because the
/// construction above argues it should hold.
///
/// # Panics
///
/// If any `outflow_lake` names a node that is not `NO_LAKE` and not another lake's
/// `root_node` in this same table (Property 4), or if any lake fails to peel (a cycle
/// `merge_tied_plateaus` should already have made unreachable).
fn assert_lake_graph_acyclic(lakes: &[Lake]) {
    let roots: Vec<u32> = lakes.iter().map(|lake| lake.root_node).collect();
    let outflow_of: Vec<u32> = lakes.iter().map(|lake| lake.outflow_lake).collect();
    let (_, stuck) = peel_lake_relation(&roots, &outflow_of);
    assert!(
        stuck.is_empty(),
        "assert_lake_graph_acyclic: {} of {} lakes did not peel -- the lake super-graph has a \
         cycle in outflow_lake (two or more lakes draining into each other), which \
         merge_tied_plateaus should already have made impossible by the time apply_outflows \
         runs this check.",
        stuck.len(),
        lakes.len(),
    );
}

/// Write every resolved overflow edge back onto `graph`'s own `Lake` table, then assert the
/// whole table is acyclic.
///
/// `resolve_outflow_edges` only computes; nothing calls `StreamGraph::set_lake_outflow_lake`
/// (or, for a merged plateau, `StreamGraph::set_lake_level_m`) until this does, mirroring
/// `apply_levels`'s own separation from `fill_lakes`. **Writing `level_m` here revises Task
/// 1's field for merged plateau members only** -- every other lake's `level_m` is left
/// exactly as `fill_lakes` computed it, since `revised_level_m` is `None` for anything
/// `merge_tied_plateaus` did not touch. The acyclicity assertion runs here, over every lake
/// actually on `graph` after every edge has landed, rather than only in a test -- "assert it
/// over every graph you build", not on a sample, per this task's own brief.
///
/// # Panics
///
/// If `edges` names a `root_node` that `graph.lakes()` has no record of (mirrors
/// `apply_levels`'s own caller-error guard), or if the resulting table fails
/// `assert_lake_graph_acyclic`.
pub fn apply_outflows(graph: &mut StreamGraph, edges: &[LakeOutflow]) {
    for edge in edges {
        let found = graph.set_lake_outflow_lake(edge.root_node, edge.outflow_lake);
        assert!(
            found,
            "apply_outflows: no lake recorded at root {} -- this LakeOutflow did not come \
             from resolve_outflow_edges over this graph.",
            edge.root_node,
        );
        if let Some(new_level) = edge.revised_level_m {
            let found = graph.set_lake_level_m(edge.root_node, new_level);
            assert!(
                found,
                "apply_outflows: no lake recorded at root {} for a revised level -- this \
                 LakeOutflow did not come from resolve_outflow_edges over this graph.",
                edge.root_node,
            );
        }
    }
    assert_lake_graph_acyclic(graph.lakes());
}

/// Resolve outflow edges for every lake in `graph`, regenerating the neighbour relation the
/// graph was actually built over -- the same regeneration `fill_basins` pays for, paid a
/// second time here because `fill_basins`/`fill_basins_and_apply` do not hand their
/// neighbour relation back (it is not part of `WaterFill`, and adding it would change Task
/// 1's frozen surface). `basins` must be `basins_of(graph)` -- **reuse the partition
/// `fill_basins_and_apply` already returned**, do not call `basins_of` a second time.
///
/// **Prefer `fill_and_resolve_water`** if you also need Task 1's fill: it regenerates the
/// neighbour relation exactly once and shares it between both, rather than paying this
/// regeneration a second time as calling this after `fill_basins_and_apply` does (review
/// Finding 6, measured at ~4.8 s each at 500,000 nodes -- essentially the whole cost of
/// either call). This function still exists, unchanged, for a caller that genuinely wants
/// only the resolve half.
///
/// # Panics
///
/// Same restriction as `fill_basins`: `graph.header().sampling_kind` must be `Spiral`,
/// since regenerating positions from the seed only reconstructs the geometry a graph
/// actually sampled that way. Call `resolve_outflow_edges` directly with a fixture's own
/// neighbour list instead -- that is what this module's own tests do.
pub fn resolve_outflows(graph: &StreamGraph, basins: &Basins) -> Vec<LakeOutflow> {
    assert!(
        graph.header().sampling_kind == SamplingKind::Spiral,
        "resolve_outflows regenerates positions from the world seed via \
         stream::node_positions, which only reconstructs the geometry a graph was actually \
         built over when sampling_kind is Spiral. This graph's sampling_kind is {:?}; call \
         resolve_outflow_edges directly with its own neighbour relation instead of \
         regenerating one that would describe a different graph.",
        graph.header().sampling_kind,
    );

    let positions = stream::node_positions(graph.header().world_seed, graph.node_count());
    let directed = stream::node_neighbours(&positions, stream::NEIGHBOUR_COUNT);
    let neighbours = symmetric_adjacency(&directed);

    resolve_outflow_edges(graph, basins, &neighbours)
}

/// The production entry point for Task 2 alone: `resolve_outflows`, then
/// `apply_outflows` over its result, so `graph.lakes()` reads back with `outflow_lake`
/// (and, for a merged plateau, a revised `level_m`) actually resolved rather than only a
/// `Vec<LakeOutflow>` a caller might forget to apply. Mirrors `fill_basins_and_apply`'s own
/// shape one field over. **Prefer `fill_and_resolve_water`** for a real pipeline that also
/// needs Task 1's fill -- see `resolve_outflows`'s own doc comment for why.
///
/// `basins` should be the value `fill_basins_and_apply` returned for this same `graph`
/// (after that call has already raised `Lake::level_m` -- this function's own internal
/// consistency check assumes the levels it re-derives will match what is already applied).
pub fn resolve_outflows_and_apply(graph: &mut StreamGraph, basins: &Basins) {
    let edges = resolve_outflows(graph, basins);
    apply_outflows(graph, &edges);
}

// ---- Task 3: pond/lake classification -----------------------------------------------------

/// Reclassify every `Lake` row's `kind` by its **physical body's** total surface area,
/// rather than trusting each row's own pre-merge, pre-fill value.
///
/// **Owner decision, 2026-09-05:** the pond/lake split is decided by surface area, not
/// drainage area. `BuildParams::pond_max_surface_area_m2`'s own doc comment has the
/// measurement that drove it: over the same bodies, the bottom decile by drainage area and
/// the bottom decile by surface area overlapped only 17-24% (task-3-report.md's addendum) --
/// a catchment-based threshold was naming bodies by something a player cannot see. Drainage
/// area is not deleted (`lake_body_drainage_totals_m2` still exists below): it remains the
/// right quantity for flow, flooding and foraging, just not for this classification.
///
/// `StreamGraph::build` gives every root an initial `kind` from a placeholder (its own
/// single root cell's `area_m2` -- see the build loop's own comment) -- necessarily a
/// placeholder, because a real surface needs a *filled* `level_m` and basin membership,
/// neither of which exist at build time. This function is what makes the classification
/// real, using `basins` (from `fill_basins`/`fill_basins_and_apply`, already run over this
/// same graph) to sum every basin member at or below the lake's own filled `level_m`.
///
/// A merged plateau's surface is the **union's** surface, not either root's own basin
/// summed alone: Ruling 7 makes a tied plateau one body of water, so every basin belonging
/// to any member of a merged group contributes to the same total. A merge does not itself
/// touch a member's basin membership (only `level_m` and `outflow_lake`), so this still
/// classifies the same `Lake` rows `StreamGraph::build` and `merge_tied_plateaus` already
/// produced (Ruling 4: no second discovery path), reading only `outflow_lake` and `level_m`
/// to find each row's body, plus `basins` and `graph.height_m`/`graph.area_m2` to total it.
///
/// # Finding a body without re-deriving the merge
///
/// `merge_tied_plateaus`' finalisation step writes, for every group of two or more tied
/// roots, the *same* revised `level_m` (bit-identical, not merely close) onto every member,
/// and points every member except the representative's own `outflow_lake` directly at the
/// representative -- one hop, never chained, because finalisation only ever runs once per
/// group and a retired group is never revisited (see that function's own "Finalise" comment
/// and its termination argument). So a satellite is recognised exactly: its `outflow_lake`
/// names another row in this same table, and that row's `level_m` matches its own,
/// bit-for-bit. A row with no such match is its own body, whether or not anything else in
/// the table happens to overflow into it (an ordinary downstream lake at a genuinely lower
/// level is not a tie).
///
/// # Panics
///
/// Never, on any table `resolve_outflow_edges`/`apply_outflows` produced -- a lake's own
/// `outflow_lake`, when it names another lake at all, always names one already present in
/// `graph.lakes()` (`apply_outflows`'s own acyclicity check already guarantees this).
pub fn classify_lake_kinds(graph: &mut StreamGraph, basins: &Basins, pond_max_surface_area_m2: f64) {
    let (lakes, body) = lake_body_index(graph);
    let surface_by_body = lake_body_surface_totals_m2(graph, basins, &lakes, &body);
    for (i, lake) in lakes.iter().enumerate() {
        let total = surface_by_body[&body[i]];
        let kind = if total <= pond_max_surface_area_m2 { LakeKind::Pond } else { LakeKind::Lake };
        let found = graph.set_lake_kind(lake.root_node, kind);
        assert!(
            found,
            "classify_lake_kinds: no lake recorded at root {} -- this row came from \
             graph.lakes() itself and must still be there.",
            lake.root_node,
        );
    }
}

/// One entry per physical body (not per `Lake` row -- a merged body's satellites are folded
/// into their representative's total), its **drainage** area in square metres. **Not the
/// classification's own quantity any more** (see `classify_lake_kinds`'s own doc comment for
/// the owner decision that moved classification to surface area), kept because catchment
/// remains the right quantity for flow, flooding and foraging -- Task 4 may want to carry it
/// on the body alongside `kind` rather than only the quantity that decided `kind`.
pub fn lake_body_drainage_totals_m2(graph: &StreamGraph) -> Vec<f64> {
    let (lakes, body) = lake_body_index(graph);
    let mut total_by_body: HashMap<usize, f64> = HashMap::with_capacity(lakes.len());
    for (i, lake) in lakes.iter().enumerate() {
        *total_by_body.entry(body[i]).or_insert(0.0) += graph.drainage_area_m2(lake.root_node);
    }
    total_by_body.into_values().collect()
}

/// One entry per physical body, its **surface** area in square metres -- the same quantity
/// `classify_lake_kinds` classifies by, exposed so a caller (a measurement binary, or a
/// future manifest task) can see the distribution without re-deriving the body partition.
pub fn lake_body_surface_areas_m2(graph: &StreamGraph, basins: &Basins) -> Vec<f64> {
    let (lakes, body) = lake_body_index(graph);
    lake_body_surface_totals_m2(graph, basins, &lakes, &body).into_values().collect()
}

/// Shared by every body-total function: partitions `graph.lakes()` into physical bodies.
/// See `classify_lake_kinds`'s own doc comment for why a merge satellite is recognised by
/// `outflow_lake` naming another row at a bit-identical `level_m`. `body[i]` is the index
/// (into the returned `Vec<Lake>`) of the row whose totals row `i`'s own contribution counts
/// toward -- itself, unless it is a merge satellite.
fn lake_body_index(graph: &StreamGraph) -> (Vec<Lake>, Vec<usize>) {
    let lakes: Vec<Lake> = graph.lakes().to_vec();
    let index_of_root: HashMap<u32, usize> =
        lakes.iter().enumerate().map(|(i, lake)| (lake.root_node, i)).collect();

    let mut body: Vec<usize> = (0..lakes.len()).collect();
    for (i, lake) in lakes.iter().enumerate() {
        if lake.outflow_lake == NO_LAKE {
            continue;
        }
        let Some(&target) = index_of_root.get(&lake.outflow_lake) else {
            continue;
        };
        if lakes[target].level_m.to_bits() == lake.level_m.to_bits() {
            body[i] = target;
        }
    }

    (lakes, body)
}

/// Every basin member at or below its own lake's filled `level_m` counts its `area_m2`
/// toward that lake's body -- the union's surface, per `classify_lake_kinds`'s own doc
/// comment on why a merged plateau's surface is not either root's basin summed alone (every
/// basin belonging to any member of a merged group contributes to the one shared total,
/// keyed by `body[i]`, not by each root's own index). An approximation at the granularity
/// every other area figure in this crate already carries: a member at the lake's edge is
/// Voronoi-cell-sized, not shoreline-exact.
fn lake_body_surface_totals_m2(
    graph: &StreamGraph,
    basins: &Basins,
    lakes: &[Lake],
    body: &[usize],
) -> HashMap<usize, f64> {
    let mut total_by_body: HashMap<usize, f64> = HashMap::with_capacity(lakes.len());
    for (i, lake) in lakes.iter().enumerate() {
        for &member in basins.members_of(lake.root_node) {
            if graph.height_m(member) <= lake.level_m {
                *total_by_body.entry(body[i]).or_insert(0.0) += graph.area_m2(member);
            }
        }
    }
    total_by_body
}

/// The full slice 5b water pipeline in one call: `basins_of`, `fill_lakes` +
/// `apply_levels` (Task 1), `resolve_outflow_edges` + `apply_outflows` (Task 2), then
/// `classify_lake_kinds` (Task 3) -- regenerating the neighbour relation from the seed
/// **exactly once** and sharing it between the first two, rather than the two independent
/// entry points (`fill_basins_and_apply`/`resolve_outflows_and_apply`) each paying for their
/// own copy.
///
/// This is the fix for review Finding 6: `fill_basins_and_apply` then
/// `resolve_outflows_and_apply` in sequence measured at ~4.8 s **each** at 500,000 nodes,
/// essentially the entire cost of either call, 100% of it duplicated work. A real world-build
/// pipeline that wants every half should call this rather than the separate entry points;
/// `fill_basins`/`fill_basins_and_apply`/`resolve_outflows`/`resolve_outflows_and_apply`/
/// `classify_lake_kinds` all still exist, unchanged, for a caller that genuinely wants only
/// one part (or, for the first two, a caller with a non-`Spiral` graph that supplies its own
/// neighbour relation).
///
/// `pond_max_surface_area_m2` must be the same value the graph's own `BuildParams` was built
/// with -- classification must run over the same threshold the caller already stated, not a
/// second, independently chosen one. Passed explicitly, not read back off the graph, because
/// `StreamGraph` does not retain its `BuildParams` (only `GraphHeader`, which has no room for
/// a threshold that never affects the format itself -- `streamfmt.rs`'s own module doc: "it
/// produced `LakeKind`, and `LakeKind` is what is stored").
///
/// Hands back the basin partition, matching `fill_basins_and_apply`'s own return value --
/// every task's writes now sit on `graph` itself.
///
/// # Panics
///
/// Same restriction as `fill_basins`/`resolve_outflows`: `graph.header().sampling_kind` must
/// be `Spiral`.
pub fn fill_and_resolve_water(graph: &mut StreamGraph, pond_max_surface_area_m2: f64) -> Basins {
    assert!(
        graph.header().sampling_kind == SamplingKind::Spiral,
        "fill_and_resolve_water regenerates positions from the world seed via \
         stream::node_positions, which only reconstructs the geometry a graph was actually \
         built over when sampling_kind is Spiral. This graph's sampling_kind is {:?}.",
        graph.header().sampling_kind,
    );

    let positions = stream::node_positions(graph.header().world_seed, graph.node_count());
    let directed = stream::node_neighbours(&positions, stream::NEIGHBOUR_COUNT);
    let neighbours = symmetric_adjacency(&directed);

    let basins = basins_of(graph);
    let filled = fill_lakes(graph, &basins, &neighbours);
    apply_levels(graph, &filled);

    let edges = resolve_outflow_edges(graph, &basins, &neighbours);
    apply_outflows(graph, &edges);

    classify_lake_kinds(graph, &basins, pond_max_surface_area_m2);

    basins
}

// ---- the water manifest --------------------------------------------------------------------
//
// Slice 5b Task 4. §13.2: "Worldbuilder therefore emits a water manifest: named bodies, each
// with extent, surface level and kind" -- lake, pond, and "river, an ordered set of reaches".
// Review round (Finding 1): §13.2 also describes the sea as the *complement* of that mapping
// -- "a mapping of named waters" and "anything unnamed falls back to the sea" are two
// mechanisms, not one, and the sea is the miss the fallback answers, never a row in the
// mapping itself. So the sea is not a `BodyKind` and is never enumerated as a `Body` at all;
// it is exactly the one scalar `WaterManifest::sea_level_m` already carries. Tasks 1-3 leave
// `graph.lakes()` fully resolved (filled, merged, classified); this is the module that reads
// that table into the artifact maritime actually consumes. `graph`'s mouths are read only to
// confirm this (`no_boundary_root_appears_as_a_body`) -- never to build a body from.
//
// **Rivers ship with reaches from the start, even though Mark 2 populates only lake and
// pond** (adapting §13.2, verbatim except for the sea) -- so `River` exists and
// `WaterManifest::rivers` is always empty. And **a manifest that cannot represent a
// waterfall has failed even though Mark 2 produces none** (§13.3): a fall is a property of a
// reach's `gradient`, not a fourth kind of body, so `BodyKind` has exactly two variants and
// never gains a `Waterfall` one (nor an `Ocean` one -- see above). Both are scope lines, not
// omissions -- see `River`'s and `BodyKind`'s own doc comments.

/// A body's footprint: the bounding rectangle in geographic coordinates (latitude and
/// longitude, in degrees) over every node this crate counted as the body's actual water --
/// not its catchment.
///
/// # Why a bounding box, and not a node set or a representative point plus area
///
/// §13.2's maritime side already keeps "a mapping of named waters" where "a world position
/// carries the region that decides which water answers" -- a point-in-region test run for
/// every position maritime ever asks about. That lookup is this manifest's entire purpose,
/// so the extent representation is chosen for it, not for how cheaply this crate can compute
/// one:
///
/// - **A node set** answers the lookup exactly, but costs O(members) per query and ties the
///   answer to this crate's own node indexing, which maritime does not share and has no
///   reason to.
/// - **A representative point plus an area** is cheap but answers a different question
///   ("how far is this position from the body's centre") than "is this position inside the
///   body" -- and this crate has already measured, once, for a different quantity, that a
///   single summary number does not stand in for a body's true irregular shape: Task 3's
///   addendum found drainage area and surface area agreeing on which bodies are smallest
///   only 17-24% of the time. A circle drawn from a centroid has no reason to agree with an
///   irregular shoreline either.
/// - **A bounding box** is a single comparison per axis, needs nothing from this crate's own
///   node indexing to evaluate on the far side, and is the standard first-pass shape for
///   exactly this query in every spatial structure this codebase already leans on for a
///   comparable lookup (`plates.rs::margin_at`'s nearest-plate test, `stream::
///   node_neighbours`'s k-NN). It is a conservative over-approximation: a position inside the
///   box is not guaranteed inside the body's true shore. That is the right direction to be
///   wrong in *when the box stays a tight over-approximation of the body's true shape* --
///   review round (Finding 3): a false hit that is wildly larger than the body itself is not
///   recoverable downstream either, which is exactly what an un-normalised antimeridian box
///   (below) would have been.
///
/// # The antimeridian, measured rather than assumed away
///
/// A lat/lon box degenerates when a body's members straddle longitude +/-180 -- not, as an
/// earlier draft of this doc comment reasoned, only "as a body's own scale approaches the
/// planet's circumference". `SpherePoint::to_latlon` returns longitude from `atan2` in
/// `(-180, 180]`, so two nodes 0.2 degrees apart on the actual planet, one at 179.9 and one
/// at -179.9, are 359.8 degrees apart in raw coordinates regardless of how small the body is.
/// Review round measurement (real `Spiral`-sampled graphs, `SEED = 20_260_905`): **6 of 171
/// lake bodies at n = 30,000 nodes and 8 of 799 at n = 100,000** have raw longitude spans over
/// 180 degrees, including a **single-node** body at 358.7 degrees. That is reachable today,
/// at resolutions this project already bakes at, not a future concern.
///
/// So this type normalises the seam rather than documenting around it: `min_longitude_deg`
/// and `max_longitude_deg` name the smallest arc on the circle that contains every point
/// (found by sorting the body's longitudes and taking the complement of the widest gap
/// between consecutive values -- the standard construction for "smallest enclosing arc on a
/// circle"). When that arc does not cross the seam, `min_longitude_deg <= max_longitude_deg`,
/// exactly the plain box a caller would expect. When it does, **`min_longitude_deg >
/// max_longitude_deg`**, and the arc is understood to run from `min_longitude_deg` up to
/// +180, wrap to -180, and continue up to `max_longitude_deg`. A containment test against
/// this box must branch on that comparison; this crate has no such test to write (Finding 13:
/// `WaterManifest` has no consumer here yet), so the convention is documented for whoever
/// writes maritime's side rather than encoded in a method this crate does not need.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Extent {
    pub min_latitude_deg: f64,
    pub max_latitude_deg: f64,
    /// See the struct doc comment's antimeridian section: greater than
    /// `max_longitude_deg` denotes an arc that wraps through +/-180, not an invalid box.
    pub min_longitude_deg: f64,
    pub max_longitude_deg: f64,
}

/// The box that contains nothing -- the fallback for a representative row [`water_manifest`]
/// finds no underwater member for (see [`water_manifest`]'s own guard comment; not reachable
/// from a real graph today, but the fallback is cheaper than a panic and just as legible).
const EMPTY_EXTENT: Extent = Extent {
    min_latitude_deg: f64::INFINITY,
    max_latitude_deg: f64::NEG_INFINITY,
    min_longitude_deg: f64::INFINITY,
    max_longitude_deg: f64::NEG_INFINITY,
};

impl Extent {
    /// The smallest box containing every point in `points` (latitude, longitude, in degrees).
    /// Latitude is a plain min/max scan -- it never wraps, since `to_latlon` keeps it in
    /// [-90, 90]. Longitude is resolved on the circle by [`smallest_enclosing_longitude_arc`];
    /// see this struct's own doc comment for the wrap convention that returns.
    ///
    /// House form throughout this crate for the latitude scan: an explicit comparison, never
    /// `f64::min`, `f64::max` or `.clamp(` (`plates.rs::margin_at`'s own comment states the
    /// same rule -- the `no_std_math.rs` build guard does not catch any of the three, so this
    /// is enforced by convention and by this module's own test coverage, not by the build).
    /// One consequence of that form, undocumented before review: a NaN latitude leaves both
    /// bounds untouched (every comparison against it is false), so it silently vanishes from
    /// the box rather than poisoning it -- unreachable from a real `SpherePoint::to_latlon`,
    /// but worth naming since this file's other extrema (`fill_lakes`'s spill formula) spell
    /// their own NaN behaviour out rather than leaving it implicit.
    ///
    /// # Panics
    ///
    /// If `points` is empty. Every caller only invokes this once at least one point has been
    /// collected for the body it describes.
    fn from_points(points: &[(f64, f64)]) -> Self {
        assert!(!points.is_empty(), "Extent::from_points requires at least one point");

        let mut min_latitude_deg = f64::INFINITY;
        let mut max_latitude_deg = f64::NEG_INFINITY;
        let mut longitudes: Vec<f64> = Vec::with_capacity(points.len());
        for &(lat, lon) in points {
            min_latitude_deg = if lat < min_latitude_deg { lat } else { min_latitude_deg };
            max_latitude_deg = if lat > max_latitude_deg { lat } else { max_latitude_deg };
            longitudes.push(lon);
        }

        let (min_longitude_deg, max_longitude_deg) =
            smallest_enclosing_longitude_arc(&mut longitudes);

        Extent { min_latitude_deg, max_latitude_deg, min_longitude_deg, max_longitude_deg }
    }
}

/// The smallest arc on the circle of longitude that contains every value in `lons` (degrees,
/// each in the `atan2` range `(-180, 180]` `SpherePoint::to_latlon` returns). Sorts `lons` in
/// place, finds the widest gap between consecutive sorted values (circularly -- the gap from
/// the last value back around to the first, plus 360, is one of the candidates), and returns
/// the complement of that gap: the smallest arc cannot contain the widest gap, and the
/// remaining arc, by construction, contains every point.
///
/// Returned as `(start, end)`. When the widest gap is the array's own natural wraparound (the
/// points do not straddle the seam), `start <= end` and this is exactly the ordinary min/max.
/// Otherwise the arc crosses the seam and `start > end` -- [`Extent`]'s own doc comment names
/// this convention for the box built from it.
fn smallest_enclosing_longitude_arc(lons: &mut [f64]) -> (f64, f64) {
    lons.sort_by(|a, b| a.partial_cmp(b).expect("a SpherePoint longitude is never NaN"));
    let n = lons.len();
    if n == 1 {
        return (lons[0], lons[0]);
    }

    let mut widest_gap = f64::NEG_INFINITY;
    let mut widest_gap_at = 0usize; // the gap immediately after lons[widest_gap_at]
    for i in 0..n {
        let next = if i + 1 < n { lons[i + 1] } else { lons[0] + 360.0 };
        let gap = next - lons[i];
        if gap > widest_gap {
            widest_gap = gap;
            widest_gap_at = i;
        }
    }

    if widest_gap_at == n - 1 {
        // The widest gap is the array's own wraparound: the points cluster away from the
        // seam, and the plain min/max already is the minimal arc.
        (lons[0], lons[n - 1])
    } else {
        // The minimal arc runs the other way around the widest gap: starting just past it
        // and wrapping through the seam back down to just before it.
        (lons[widest_gap_at + 1], lons[widest_gap_at])
    }
}

/// §13.2 asks for a body's `kind`, and it is exactly one of two -- **never three, never
/// four.**
///
/// **No `Ocean` variant** (review round, Finding 1). §13.2 names two complementary
/// mechanisms: "a mapping of named waters" and "anything unnamed falls back to the sea". The
/// sea is what the mapping's *miss* returns -- it is the complement of the mapping, not a row
/// in it. A per-mouth `Ocean` body would put the sea into the very mapping it is defined
/// against, and it is measurably unusable besides: at n = 30,000/100,000 nodes, 86%/81% of
/// all bodies would be `Ocean`, 96% of their boxes would overlap each other with no rule to
/// pick one, and over a third of real lakes would sit inside at least one of them. "Ocean --
/// the datum" is a statement about level semantics, not an instruction to enumerate one row
/// per mouth: the datum is `WaterManifest::sea_level_m`, once, and that is the whole of the
/// sea's representation here. A boundary root never appears in [`WaterManifest::bodies`] at
/// all (see `no_boundary_root_appears_as_a_body`).
///
/// **No `Waterfall` variant.** A fall is not a body (§13.3): it is a property of a *reach*
/// (the `gradient` field `stream::Reach` already carries), and to maritime it is a limit --
/// the upstream end of navigability, absolute rather than tidal, that belongs on a marks
/// channel rather than in soundings. A `Waterfall` variant here would put a reach-derived
/// limit where a body's kind belongs, which is exactly the mistake §13.3 exists to prevent.
/// Mark 2 produces no waterfalls; that is a fact about this generator's output, not a reason
/// to widen this enum to make room for one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    Lake,
    Pond,
}

/// One named body of water: `graph.lakes()`'s and `graph.roots()`'s own root-node identity,
/// plus everything §13.2 asks a body to carry. Never a mouth (a boundary root): see
/// [`BodyKind`]'s own doc comment for why the sea is never one of these.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Body {
    /// This body's identity in §13.2's "mapping of named waters" -- the mapping needs a key,
    /// `Body` carries no separate name field, and this is the only candidate: `graph.lakes()`'s
    /// own root-node identity, matching `Lake::root_node`. It does leak this crate's own node
    /// indexing, but no other stable identifier exists, and correlating a body across two
    /// manifests of the same world (or back to `graph.lakes()`) is a real need the mapping's
    /// key exists to serve, not an internal detail leaking out.
    pub root_node: u32,
    pub kind: BodyKind,
    /// The (post-merge) filled `Lake::level_m` Task 1 and Task 2 already computed for this
    /// body's representative row.
    pub level_m: f64,
    pub extent: Extent,
}

/// One river: "an ordered set of reaches" (§13.2's own phrase). `stream::Reach` already
/// carries `gradient`, which is what lets a future task hang a waterfall off the upstream end
/// of one (§13.3) without a schema break; this wrapper is what lets the manifest carry a
/// *sequence* of them as a single named river, rather than an unordered bag with no notion of
/// "the ordered set belonging to one river" at all.
///
/// **Mark 2 populates none.** `WaterManifest::rivers` is always empty -- this task's scope
/// line is explicit that river population is not this task's business -- but the type exists
/// now, fully able to hold a real river, so a later slice's population is additive rather
/// than the schema break retrofitting it later would be (§13.2, verbatim: "Retrofitting that
/// later would be a schema break; carrying it now costs nothing").
#[derive(Debug, Clone, PartialEq)]
pub struct River {
    pub reaches: Vec<stream::Reach>,
}

/// The water manifest: every named body [`water_manifest`] found in one `StreamGraph`, plus
/// the datum they were found at.
#[derive(Debug, Clone, PartialEq)]
pub struct WaterManifest {
    /// The sea, in full: not a `Body`, not a `BodyKind`, exactly this one scalar
    /// (`BodyKind`'s own doc comment argues why). It is both the datum the mapping's fallback
    /// answers with (§13.2: "anything unnamed falls back to the sea") and the datum
    /// mouth-versus-lake was decided at for every body that *is* in `bodies`
    /// (`GraphHeader::sea_level_m`'s own doc comment: "Mouth-versus-lake is a function of
    /// it"). The same graph yields a different manifest at a different datum, so a manifest
    /// that does not name its own cannot be checked against the world it describes.
    pub sea_level_m: f64,
    pub bodies: Vec<Body>,
    /// Reserved and always empty at Mark 2. See [`River`]'s own doc comment.
    pub rivers: Vec<River>,
}

/// Every basin member across every row folded into physical body `target_body` (Ruling 7's
/// merge target index, from [`lake_body_index`]), restricted to the ones actually at or below
/// that body's own filled `level_m` -- the underwater footprint, not the catchment. Mirrors
/// [`lake_body_surface_totals_m2`]'s own double loop exactly, collecting points for an
/// [`Extent`] instead of summing an area, for the same reason that function gives: a merged
/// plateau's footprint is the union's footprint, so every basin belonging to any member of a
/// merged group must contribute to the same box, keyed by `body[i]`, not by each row's own
/// index. Points are collected rather than folded incrementally because the antimeridian fix
/// (`Extent::from_points`'s own doc comment) needs every longitude in the body at once.
fn lake_body_extents(
    graph: &StreamGraph,
    basins: &Basins,
    positions: &[SpherePoint],
    lakes: &[Lake],
    body: &[usize],
) -> HashMap<usize, Extent> {
    let mut points_by_body: HashMap<usize, Vec<(f64, f64)>> = HashMap::with_capacity(lakes.len());
    for (i, lake) in lakes.iter().enumerate() {
        for &member in basins.members_of(lake.root_node) {
            if graph.height_m(member) <= lake.level_m {
                let latlon = positions[member as usize].to_latlon(); // cast-ok: a node index into usize
                points_by_body.entry(body[i]).or_default().push(latlon);
            }
        }
    }
    points_by_body.into_iter().map(|(b, points)| (b, Extent::from_points(&points))).collect()
}

/// Build the water manifest: every lake and pond body in `graph`, at the datum recorded in
/// `graph.header().sea_level_m`, plus an always-empty river arm (see [`River`]'s own doc
/// comment). The sea itself is never one of `bodies` -- see [`BodyKind`]'s own doc comment for
/// why, and `no_boundary_root_appears_as_a_body` for the property this function upholds.
///
/// `positions` must be the same node positions `graph` was built over -- the same requirement
/// [`fill_lakes`] and [`resolve_outflow_edges`] already place on their own `neighbours`
/// argument, and for the same reason: passed in rather than regenerated here, so a small
/// hand-authored fixture (this module's own tests, over `SamplingKind::Supplied`) can exercise
/// this function directly without also having to agree with the spiral sampler about where
/// thousands of nodes sit. [`water_manifest_from_graph`] is the entry point that pays the
/// regeneration cost for a real, `Spiral`-sampled graph.
///
/// `basins` must be [`basins_of`]`(graph)` (or an equivalent partition over the same graph),
/// the same requirement every other function in this module places on it.
///
/// # No new merging
///
/// Ruling 7's merge already folded every tied plateau into one physical body's worth of
/// `outflow_lake`/`level_m` (`lake_body_index`'s own doc comment); this only reads that result
/// -- once per physical body, via the same representative test [`classify_lake_kinds`] uses
/// (`body[i] == i`) -- and never merges further.
pub fn water_manifest(graph: &StreamGraph, basins: &Basins, positions: &[SpherePoint]) -> WaterManifest {
    let sea_level_m = graph.header().sea_level_m;
    let mut bodies = Vec::new();

    let (lakes, body) = lake_body_index(graph);
    let extents = lake_body_extents(graph, basins, positions, &lakes, &body);
    for (i, lake) in lakes.iter().enumerate() {
        if body[i] != i {
            // A merge satellite (Ruling 7): the same physical body already appears (or will
            // appear) under its representative's own root, at index `body[i]`. Emitting a
            // second entry here would be the exact totality failure Property 2 forbids.
            continue;
        }
        let kind = match lake.kind {
            LakeKind::Pond => BodyKind::Pond,
            LakeKind::Lake => BodyKind::Lake,
        };
        // Ruling 2's `level_m >= height_m(root)` invariant means the representative's own
        // root always passes `lake_body_extents`'s filter, so this row is unreachable today
        // -- but the invariant it relies on lives in another module (Task 1/2's), not here,
        // so the dependency is made legible with a fallback rather than an unguarded index
        // that would panic on the artifact this function ships.
        let extent = extents.get(&i).copied().unwrap_or(EMPTY_EXTENT);
        bodies.push(Body { root_node: lake.root_node, kind, level_m: lake.level_m, extent });
    }

    // Ascending by root node -- deterministic regardless of the `HashMap` iteration order
    // `lake_body_extents` built its intermediate result in.
    bodies.sort_by_key(|b| b.root_node);

    WaterManifest { sea_level_m, bodies, rivers: Vec::new() }
}

/// [`water_manifest`], over a real `Spiral`-sampled graph: regenerates node positions from
/// the world seed, exactly as [`fill_basins`] and [`resolve_outflows`] already do for their
/// own inputs, rather than asking every real caller to keep a copy of `positions` around
/// after the graph itself no longer needs one.
///
/// # Panics
///
/// `graph.header().sampling_kind` must be `Spiral` -- the same restriction [`fill_basins`]
/// and [`resolve_outflows`] already carry, for the same reason: this regenerates positions
/// from the world seed via `stream::node_positions`, which only reconstructs the geometry a
/// graph was actually built over when `sampling_kind` says so.
pub fn water_manifest_from_graph(graph: &StreamGraph, basins: &Basins) -> WaterManifest {
    assert!(
        graph.header().sampling_kind == SamplingKind::Spiral,
        "water_manifest_from_graph regenerates positions from the world seed via \
         stream::node_positions, which only reconstructs the geometry a graph was actually \
         built over when sampling_kind is Spiral. This graph's sampling_kind is {:?}.",
        graph.header().sampling_kind,
    );
    let positions = stream::node_positions(graph.header().world_seed, graph.node_count());
    water_manifest(graph, basins, &positions)
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
    //
    // Lake A's root sitting at h=0.0 -- the lowest elevation anywhere in the fixture -- is
    // itself load-bearing, and for a different test: it is what keeps
    // `spill_uses_the_inner_max_not_the_inner_min` a real discrimination rather than one the
    // `level_m >= height_m(root)` invariant would catch on its own. Under the mutated `min`,
    // lake A's spill drops to `min(0.0, 5.0) = 0.0`, which still satisfies `0.0 >= 0.0` --
    // the invariant is blind to this particular swap, so only `assert_eq!(a.level_m, 1.0)`
    // actually catches it. Raise lake A's root above every other node's height and that
    // assertion stops constraining the operator with no test failure to say so.
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
            pond_max_surface_area_m2: 1.0,
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

    /// Finding 1's fix: `fill_lakes` alone leaves `graph`'s own `Lake` table untouched --
    /// `apply_levels` is the step that actually moves it.
    #[test]
    fn apply_levels_writes_the_filled_value_onto_the_graphs_own_lake_table() {
        let (mut graph, directed) = touching_lakes_fixture();
        let basins = basins_of(&graph);
        let symmetric = symmetric_adjacency(&directed);
        let filled = fill_lakes(&graph, &basins, &symmetric);

        // Before applying, the graph's own record still reads exactly what `build` left it
        // at: the root's own elevation, `Lake::level_m`'s own doc comment's "empty basin".
        assert_eq!(graph.lake_at(0).expect("lake A").level_m, 0.0, "unapplied: still root A's own elevation");

        apply_levels(&mut graph, &filled);

        assert_eq!(graph.lake_at(0).expect("lake A").level_m, 1.0, "lake A's level_m was not written back by apply_levels");
        assert_eq!(graph.lake_at(2).expect("lake B").level_m, 1.0);
        // Nothing else on either record moved.
        assert_eq!(graph.lake_at(0).expect("lake A").kind, LakeKind::Lake);
        assert_eq!(graph.lake_at(0).expect("lake A").outflow_lake, crate::stream::NO_LAKE);
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
            pond_max_surface_area_m2: 1.0,
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
        let field = Surface::new(seed, EARTH_RADIUS_M, 22, 0.29, None, None, None);
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
                pond_max_surface_area_m2: 5.0e9,
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

    /// `fill_basins_and_apply` is the production entry point (finding 1's fix): confirms it
    /// actually moves `graph.lakes()`'s own `level_m`, not merely a `WaterFill` on the side.
    #[test]
    fn fill_basins_and_apply_moves_the_graphs_own_lake_levels() {
        let mut graph = real_graph(SEED);
        assert!(!graph.lakes().is_empty(), "fixture must actually have lakes to test this");
        let before: HashMap<u32, f64> =
            graph.lakes().iter().map(|l| (l.root_node, l.level_m)).collect();

        let basins = fill_basins_and_apply(&mut graph);
        assert_eq!(basins.node_count(), graph.node_count() as usize); // cast-ok: a node count into usize

        let mut any_raised = false;
        for lake in graph.lakes() {
            let root_height = graph.height_m(lake.root_node);
            assert!(lake.level_m >= root_height);
            let prior = before[&lake.root_node];
            assert_eq!(
                prior, root_height,
                "fixture assumption broken: StreamGraph::build should leave level_m at the \
                 root's own elevation before anything applies a fill"
            );
            if lake.level_m > prior {
                any_raised = true;
            }
        }
        assert!(
            any_raised,
            "at least one real lake should fill strictly above its own root's elevation, or \
             this test cannot tell a real write from a no-op"
        );
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

    /// Message-pinned, matching `a_basin_covering_the_whole_graph_has_no_rim_and_panics`'s own
    /// precedent one test above -- an unpinned `#[should_panic]` (or a bare `catch_unwind` +
    /// `is_err()`) would pass for any panic at all, including an unrelated one.
    #[test]
    #[should_panic(expected = "call fill_lakes directly")]
    fn fill_basins_refuses_a_graph_it_did_not_regenerate_from_a_seed() {
        let (graph, _) = touching_lakes_fixture();
        assert_eq!(graph.header().sampling_kind, crate::stream::SamplingKind::Supplied);
        let _ = fill_basins(&graph);
    }

    // ==== Task 2: the lake super-graph and its overflow edges ============================

    /// Fill a `Supplied`-position fixture's lake levels directly (`basins_of` + `fill_lakes`
    /// + `apply_levels`), since `fill_basins`/`fill_basins_and_apply` refuse anything but a
    /// `Spiral` graph (`fill_basins_refuses_a_graph_it_did_not_regenerate_from_a_seed`, above,
    /// is exactly this restriction). Every fixture-based test below needs exactly this
    /// sequence, so it is factored out once rather than repeated per test.
    fn fill_fixture_lakes(graph: &mut StreamGraph, symmetric: &[Vec<u32>]) -> Basins {
        let basins = basins_of(graph);
        let filled = fill_lakes(graph, &basins, symmetric);
        apply_levels(graph, &filled);
        basins
    }

    // ---- the ordering-disagreement fixture -----------------------------------------------
    //
    // Two lakes, A (root 0) and B (root 2), plus a mouth (node 4) reachable only from B. The
    // neighbour lists are deliberately asymmetric in the same way Task 1's own
    // `touching_lakes_fixture` is (see that fixture's comment): node 2's own list omits node
    // 4, and node 4's list names node 2, so `symmetric_adjacency` is what gives node 2 back
    // its rim edge to the mouth -- without it, node 2 would see no external edge to the sea
    // at all. This omission is also what keeps node 2 a *root*: `StreamGraph::build` computes
    // downhill purely from each node's own raw list, so if node 2's own list named node 4
    // (h = -100.0, far lower), node 2 would downhill straight to the sea and never become a
    // lake root in the first place.
    //
    //   node 0 (root A, h=0.0)   -- neighbours [1]         (A's root: nothing lower nearby)
    //   node 1 (in A,   h=64.0)  -- neighbours [0, 3]       (downhill -> 0)
    //   node 2 (root B, h=60.0)  -- neighbours [3]          (B's root: sits far higher than A's)
    //   node 3 (in B,   h=65.0)  -- neighbours [1, 2]       (downhill -> 2: steeper drop, 5 > 1)
    //   node 4 (mouth,  h=-100.0) -- neighbours [2]          (sea; sea_level_m = -50.0)
    //
    // Basin A = {0, 1}, basin B = {2, 3}. A's only external edge is 1--3: max(64, 65) = 65, so
    // level_m(A) = 65. B has two external edges (after symmetrisation): 3--1 (max(65, 64) =
    // 65, the same physical edge as A's) and 2--4 (max(60, -100) = 60); B's minimum is 60, so
    // level_m(B) = 60 and its target is the mouth's basin, not a lake -- B is this fixture's
    // terminal lake.
    //
    // root(A) = 0.0 is LOWER than root(B) = 60.0, but level_m(A) = 65.0 is HIGHER than
    // level_m(B) = 60.0 -- the two orderings genuinely disagree, which is the whole point:
    // A's water, once it reaches its spill, crosses into B's basin (65 >= 60, Property 5), so
    // the correct edge is A -> B. Ordering by root height instead would say the opposite --
    // B, with the higher root, "looks downhill of" A only if root height is mistaken for
    // level, which is exactly the mistake this fixture exists to catch.
    fn ordering_disagreement_fixture() -> (StreamGraph, Vec<Vec<u32>>) {
        let positions = vec![
            SpherePoint::from_latlon(0.0, 0.0),
            SpherePoint::from_latlon(10.0, 0.0),
            SpherePoint::from_latlon(0.0, 10.0),
            SpherePoint::from_latlon(10.0, 10.0),
            SpherePoint::from_latlon(20.0, 10.0),
        ];
        let heights = vec![0.0, 64.0, 60.0, 65.0, -100.0];
        let areas = vec![1.0e9; 5];
        let neighbours = vec![vec![1], vec![0, 3], vec![3], vec![1, 2], vec![2]];
        let params = BuildParams {
            world_seed: 3,
            radius_m: EARTH_RADIUS_M,
            sea_level_m: -50.0, // node 4 (-100.0) is BOUNDARY; nodes 0-3 are LAND.
            sampling_kind: crate::stream::SamplingKind::Supplied,
            pond_max_surface_area_m2: 1.0,
        };
        let graph = StreamGraph::build(&params, &positions, &heights, &areas, &neighbours)
            .expect("the ordering-disagreement fixture builds a valid graph");
        (graph, neighbours)
    }

    #[test]
    fn ordering_disagreement_fixture_has_the_roots_and_mouth_this_test_relies_on() {
        let (graph, _) = ordering_disagreement_fixture();
        assert_eq!(graph.roots(), vec![0, 2, 4], "fixture drifted: expected roots at 0, 2, 4");
        assert_eq!(graph.lakes().len(), 2, "fixture drifted: expected exactly two lakes");
        assert!(graph.lake_at(4).is_none(), "node 4 must be a mouth, not a lake");
    }

    // ---- the two-candidate ordering fixture -----------------------------------------------
    //
    // Review Finding 4: `ordering_disagreement_fixture` above has exactly one external
    // candidate for lake A, so no ranking rule could ever change its target -- the fixture
    // proved the two *orderings* disagree (root height vs level) but never exercised the
    // *ranking step itself*. This fixture gives lake A two external candidates, to two
    // different lakes, so a mutation to the ranking key actually has something to change.
    //
    //   node 0  (root A,  h=0.0)    -- neighbours []          (A's root)
    //   node 1  (in A,    h=10.0)   -- neighbours [0, 4]       (downhill -> 0; touches T1)
    //   node 2  (in A,    h=60.0)   -- neighbours [0]          (downhill -> 0; touches T2)
    //   node 3  (root T1, h=50.0)   -- neighbours []           (T1's root: HIGH)
    //   node 4  (in T1,   h=52.0)   -- neighbours [3]          (downhill -> 3; touches A)
    //   node 5  (root T2, h=5.0)    -- neighbours []           (T2's root: LOW)
    //   node 6  (in T2,   h=6.0)    -- neighbours [5, 2]       (downhill -> 5; touches A)
    //   node 7  (in T1,   h=50.5)   -- neighbours [3]          (downhill -> 3; T1's own escape)
    //   node 8  (mouth,   h=-100.0) -- neighbours [7]          (sea for T1; sea_level_m = -50.0)
    //   node 9  (in T2,   h=7.0)    -- neighbours [5]          (downhill -> 5; T2's own escape)
    //   node 10 (mouth,   h=-200.0) -- neighbours [9]          (sea for T2)
    //
    // Basin A = {0, 1, 2}. Its two external crossings: 1--4 (max(10, 52) = 52, into T1) and
    // 2--6 (max(60, 6) = 60, into T2). **52 < 60, so the correct target is T1** -- the lake
    // with the HIGHER root (50.0), not T2 (root 5.0). Ranking by the target's own root
    // elevation instead of by crossing height would compare height_m(3) = 50.0 against
    // height_m(5) = 5.0 and wrongly prefer T2 (the lower root), which is exactly the class of
    // mistake `ranking_crossings_by_target_root_height_is_wrong` mutates in and watches fail.
    //
    // Basin T1 = {3, 4, 7}. Its two external crossings: 4--1 (52, back into A) and 7--8
    // (max(50.5, -100) = 50.5, into its own mouth). **50.5 < 52**, so T1's own level is 50.5
    // and its target is that mouth (terminal) -- T1 does NOT tie back with A (50.5 != 52).
    //
    // Basin T2 = {5, 6, 9}. Its two external crossings: 6--2 (60, into A) and 9--10
    // (max(7, -200) = 7, into its own separate mouth). **7 < 60**, so T2's own level is 7 and
    // its target is that mouth (terminal) -- **T2 does not point back at A either.** This is
    // deliberate and is what makes the mutation test below discriminating: without T2's own
    // independent escape, a lowest_crossing mutation that wrongly sends A to T2 would create
    // a genuine A<->T2 cycle (T2's only candidate would be A), and `merge_tied_plateaus`
    // would then *correctly* re-derive A's true target (T1) as the union's own real rim
    // crossing while resolving that cycle -- silently masking the mutation instead of
    // exposing it. A confirmed, not a hypothetical: an earlier version of this fixture had
    // exactly that shape, and `ranking_crossings_by_target_root_height_is_wrong` passed under
    // the mutation with the shadow neutralised, for precisely this reason.
    fn two_candidate_ordering_fixture() -> (StreamGraph, Vec<Vec<u32>>) {
        let positions = vec![
            SpherePoint::from_latlon(0.0, 0.0),
            SpherePoint::from_latlon(10.0, 0.0),
            SpherePoint::from_latlon(0.0, 10.0),
            SpherePoint::from_latlon(20.0, 0.0),
            SpherePoint::from_latlon(20.0, 10.0),
            SpherePoint::from_latlon(0.0, 20.0),
            SpherePoint::from_latlon(0.0, 30.0),
            SpherePoint::from_latlon(30.0, 10.0),
            SpherePoint::from_latlon(30.0, 20.0),
            SpherePoint::from_latlon(0.0, 40.0),
            SpherePoint::from_latlon(0.0, 50.0),
        ];
        let heights = vec![0.0, 10.0, 60.0, 50.0, 52.0, 5.0, 6.0, 50.5, -100.0, 7.0, -200.0];
        let areas = vec![1.0e9; 11];
        let neighbours = vec![
            vec![],
            vec![0, 4],
            vec![0],
            vec![],
            vec![3],
            vec![],
            vec![5, 2],
            vec![3],
            vec![7],
            vec![5],
            vec![9],
        ];
        let params = BuildParams {
            world_seed: 4,
            radius_m: EARTH_RADIUS_M,
            sea_level_m: -50.0, // nodes 8 and 10 are BOUNDARY; nodes 0-7 and 9 are LAND.
            sampling_kind: crate::stream::SamplingKind::Supplied,
            pond_max_surface_area_m2: 1.0,
        };
        let graph = StreamGraph::build(&params, &positions, &heights, &areas, &neighbours)
            .expect("the two-candidate ordering fixture builds a valid graph");
        (graph, neighbours)
    }

    #[test]
    fn two_candidate_ordering_fixture_has_the_roots_this_test_relies_on() {
        let (graph, _) = two_candidate_ordering_fixture();
        assert_eq!(
            graph.roots(),
            vec![0, 3, 5, 8, 10],
            "fixture drifted: expected roots at 0, 3, 5, 8, 10"
        );
        assert_eq!(graph.lakes().len(), 3, "fixture drifted: expected exactly three lakes");
        assert!(graph.lake_at(8).is_none(), "node 8 must be a mouth, not a lake");
    }

    /// The discriminating property itself: with two external candidates, the correct target
    /// is the one with the lower *crossing height* (T1, 52 m), not the one with the lower
    /// *root elevation* (T2, root 5.0 m).
    #[test]
    fn outflow_prefers_the_lower_crossing_not_the_lower_root() {
        let (mut graph, directed) = two_candidate_ordering_fixture();
        let symmetric = symmetric_adjacency(&directed);
        let basins = fill_fixture_lakes(&mut graph, &symmetric);

        assert_eq!(graph.lake_at(0).expect("lake A").level_m, 52.0);
        assert_eq!(graph.lake_at(3).expect("lake T1").level_m, 50.5);
        assert_eq!(graph.lake_at(5).expect("lake T2").level_m, 7.0);
        assert!(
            graph.height_m(3) > graph.height_m(5),
            "fixture drifted: T1's root must be higher than T2's, or a root-height ranking \
             would not disagree with the correct crossing-height ranking"
        );

        let edges = resolve_outflow_edges(&graph, &basins, &symmetric);
        let a = edges.iter().find(|e| e.root_node == 0).expect("lake A's edge");
        assert_eq!(a.outflow_lake, 3, "A must drain into T1 (the lower crossing), not T2 (the lower root)");
    }

    /// The mutation the brief actually names, verified by actually performing it (Finding 4:
    /// the previous mutation-regression test used a fixture where this comparison never had
    /// a second candidate to prefer, so it could not fail no matter how the ranking key was
    /// mutated). `lowest_crossing`'s running-minimum comparison
    /// (`if crossing_level < current.level_m { ... }`, water.rs) was edited in place to `if
    /// graph.height_m(target_root) < graph.height_m(current.target_root) { ... }` -- ranking
    /// candidates by the target basin's own root elevation instead of by the crossing height
    /// itself.
    ///
    /// **Two runs, matching the reviewer's own rigor.** With `resolve_outflow_edges`'s
    /// bit-identical consistency assertion (this module's other guard against exactly this
    /// class of bug) left in place, this test failed there first -- the mutated scan finds a
    /// crossing of `60 m` for lake A instead of the applied `52 m`, since `lowest_crossing`
    /// still records the *correct* crossing height for whichever candidate wins; only the
    /// *choice* of winner is wrong. That result alone would not prove this fixture's own
    /// named assertion (`assert_eq!(a.outflow_lake, 3)`) is what caught the mutation, since
    /// the consistency check could be doing all the work. With that consistency assertion
    /// temporarily replaced by a tautology (`crossing.level_m.to_bits() ==
    /// crossing.level_m.to_bits()`) and the mutation still in place, this test failed again,
    /// this time on its own named assertion directly: `left: 5, right: 3` -- T2 (root 5.0)
    /// won the mutated comparison over T1 (root 50.0), exactly as designed. Both mutations
    /// were then reverted and the full `water::` suite re-run clean (32/32). Recorded here as
    /// this module's own comment, per the brief's instruction, rather than left to be taken
    /// on faith; the task report records the transcript of both runs.
    ///
    /// This fixture's earlier revision (where T2's only external edge led back to A) did
    /// **not** survive this same two-run check: with the shadow neutralised,
    /// `merge_tied_plateaus` treated the mutation-induced A<->T2 cycle as a genuine tied
    /// plateau, re-scanned the union with the *unmutated* formula, and re-derived A's correct
    /// target (T1) as a side effect of resolving the cycle -- silently masking the mutation
    /// instead of exposing it. T2 was given its own independent escape (nodes 9-10)
    /// specifically to remove that interaction; see the fixture's own doc comment above.
    #[test]
    fn ranking_crossings_by_target_root_height_is_wrong() {
        // Same body as `outflow_prefers_the_lower_crossing_not_the_lower_root` -- a second,
        // separately named entry point so the mutation record above can point at one test by
        // name distinct from "the property test", even though today they exercise the same
        // fixture and assertion.
        let (mut graph, directed) = two_candidate_ordering_fixture();
        let symmetric = symmetric_adjacency(&directed);
        let basins = fill_fixture_lakes(&mut graph, &symmetric);
        let edges = resolve_outflow_edges(&graph, &basins, &symmetric);
        let a = edges.iter().find(|e| e.root_node == 0).expect("lake A's edge");
        assert_eq!(a.outflow_lake, 3);
    }

    /// The fixture's whole reason to exist: prove the two orderings actually disagree, then
    /// assert the resolved edge follows level, not root height.
    #[test]
    fn outflow_follows_level_not_root_height() {
        let (mut graph, directed) = ordering_disagreement_fixture();
        let symmetric = symmetric_adjacency(&directed);
        let basins = fill_fixture_lakes(&mut graph, &symmetric);

        let root_a = graph.height_m(0);
        let root_b = graph.height_m(2);
        let level_a = graph.lake_at(0).expect("lake A").level_m;
        let level_b = graph.lake_at(2).expect("lake B").level_m;
        assert!(root_a < root_b, "fixture drifted: A's root must be lower than B's");
        assert!(
            level_a > level_b,
            "fixture drifted: A's level must be higher than B's -- otherwise the two \
             orderings do not actually disagree and this test proves nothing"
        );
        assert_eq!(level_a, 65.0);
        assert_eq!(level_b, 60.0);

        let edges = resolve_outflow_edges(&graph, &basins, &symmetric);
        let a = edges.iter().find(|e| e.root_node == 0).expect("lake A's edge");
        let b = edges.iter().find(|e| e.root_node == 2).expect("lake B's edge");
        assert_eq!(a.outflow_lake, 2, "A must drain into B, following level, not root height");
        assert_eq!(
            b.outflow_lake, NO_LAKE,
            "B's lowest crossing leads to the mouth's basin, not a lake -- B is terminal"
        );
    }

    /// The mutation this module's own doc comment (`resolve_outflow_edges`) promises was
    /// actually run: `lowest_crossing`'s inner `h_outside` (`graph.height_m(outside)`, the
    /// *member's* own elevation) was edited in place to `graph.height_m(basins.root_of
    /// (outside))` -- the neighbouring *basin's root* elevation instead -- which is exactly
    /// the height_m(root) contamination this module's doc comment warns `outflow_lake`
    /// resolution must never fall into.
    ///
    /// It failed as this test predicted: with the mutation in place,
    /// `ordering_disagreement_fixture`'s `1--3` edge computes `max(64.0, height_m(root_of(3)))
    /// = max(64.0, 60.0) = 64.0` instead of the correct `max(64.0, 65.0) = 65.0` (node 3's own
    /// elevation is 65.0; its basin's root, node 2, is 60.0). That is not the value `Lake::
    /// level_m` already carries for A (65.0, applied by `fill_fixture_lakes` before the
    /// mutation and never recomputed), so `resolve_outflow_edges`'s own internal consistency
    /// assertion fired immediately: `"the re-scanned crossing for lake 0 is 64 m, but Lake::
    /// level_m already carries 65 m"`. The mutation was then reverted and this test re-run
    /// clean. This is recorded here as a comment, per the brief's own instruction, rather
    /// than left to be taken on faith; see the task report for the transcript of the mutated
    /// run.
    #[test]
    fn outflow_direction_follows_level_not_root_height_regression_guard() {
        // This test's body is `outflow_follows_level_not_root_height` in substance -- it
        // exists as a second, separately named entry point specifically so the mutation
        // record above can point at one test by name without conflating "the property test"
        // and "the mutation-proof test", even though today they exercise the same fixture.
        let (mut graph, directed) = ordering_disagreement_fixture();
        let symmetric = symmetric_adjacency(&directed);
        let basins = fill_fixture_lakes(&mut graph, &symmetric);
        let edges = resolve_outflow_edges(&graph, &basins, &symmetric);
        let a = edges.iter().find(|e| e.root_node == 0).expect("lake A's edge");
        assert_eq!(a.outflow_lake, 2);
    }

    #[test]
    fn resolve_outflow_edges_is_bit_identical_across_two_runs() {
        let (mut graph, directed) = ordering_disagreement_fixture();
        let symmetric = symmetric_adjacency(&directed);
        let basins = fill_fixture_lakes(&mut graph, &symmetric);

        let first = resolve_outflow_edges(&graph, &basins, &symmetric);
        let second = resolve_outflow_edges(&graph, &basins, &symmetric);
        assert_eq!(first, second, "the same graph, resolved twice, must agree exactly");
    }

    #[test]
    fn apply_outflows_writes_outflow_lake_onto_the_graphs_own_lake_table() {
        let (mut graph, directed) = ordering_disagreement_fixture();
        let symmetric = symmetric_adjacency(&directed);
        let basins = fill_fixture_lakes(&mut graph, &symmetric);

        assert_eq!(graph.lake_at(0).expect("lake A").outflow_lake, NO_LAKE, "unapplied: still the sentinel");
        resolve_outflows_and_apply_over(&mut graph, &basins, &symmetric);
        assert_eq!(graph.lake_at(0).expect("lake A").outflow_lake, 2);
        assert_eq!(graph.lake_at(2).expect("lake B").outflow_lake, NO_LAKE);
        // Nothing else on either record moved.
        assert_eq!(graph.lake_at(0).expect("lake A").level_m, 65.0);
        assert_eq!(graph.lake_at(2).expect("lake B").level_m, 60.0);
    }

    /// Property 6: a terminal lake (no outflow to another lake) keeps `NO_LAKE`, and the
    /// fixture actually has one -- without this, the property would be untested rather than
    /// merely unexercised.
    #[test]
    fn a_terminal_lake_keeps_the_sentinel() {
        let (mut graph, directed) = ordering_disagreement_fixture();
        let symmetric = symmetric_adjacency(&directed);
        let basins = fill_fixture_lakes(&mut graph, &symmetric);
        resolve_outflows_and_apply_over(&mut graph, &basins, &symmetric);
        assert_eq!(graph.lake_at(2).expect("lake B").outflow_lake, NO_LAKE);
    }

    /// A small `Supplied`-graph equivalent of [`resolve_outflows_and_apply`], which cannot be
    /// used directly over a fixture (it regenerates neighbours from the seed and refuses
    /// anything but `Spiral`, same as `fill_basins`). Test-only plumbing, not a second
    /// production entry point.
    fn resolve_outflows_and_apply_over(graph: &mut StreamGraph, basins: &Basins, symmetric: &[Vec<u32>]) {
        let edges = resolve_outflow_edges(graph, basins, symmetric);
        apply_outflows(graph, &edges);
    }

    /// `touching_lakes_fixture` (Task 1's own two-lake fixture) is a genuine tied plateau --
    /// both lakes fill to the identical `level_m = 1.0` and each one's lowest crossing points
    /// at the other -- but it is also a *closed system*: the union of both basins covers this
    /// fixture's entire four-node graph, so it has no rim of its own at all, the same
    /// "impossible on an actual sphere" case `fill_lakes`' own no-rim panic makes for a
    /// single basin. `merge_tied_plateaus` refuses it for the identical reason, rather than
    /// silently reporting a level or an outflow the union does not actually have.
    #[test]
    #[should_panic(expected = "has no rim")]
    fn touching_lakes_fixture_is_a_closed_system_and_merge_refuses_it() {
        let (mut graph, directed) = touching_lakes_fixture();
        let symmetric = symmetric_adjacency(&directed);
        let basins = fill_fixture_lakes(&mut graph, &symmetric);
        let _ = resolve_outflow_edges(&graph, &basins, &symmetric);
    }

    // ---- the merge fixture: a real tied plateau with a real outlet ------------------------
    //
    // Review Finding 2: two basins sharing a 10 m saddle, each with its own separate exit to
    // the sea at 20 m. Both basins' cheapest exit is each other (the saddle), so the naive
    // per-basin minimum ties them at 10 m -- but neither basin has actually found its true
    // outlet: the union of both, once the shared saddle no longer counts as an exit, spills
    // at 20 m instead. Cutting one lake to `NO_LAKE` (this module's first-round fix,
    // reviewed and rejected) would report two lakes at 10 m, one of them a false terminal,
    // with the real 20 m sea outlet represented nowhere. Merging reports both at the true
    // 20 m and a real path to the sea.
    //
    //   node 0 (root A,  h=0.0)   -- neighbours []          (A's root)
    //   node 1 (in A,    h=10.0)  -- neighbours [0, 4]       (downhill -> 0; the saddle side)
    //   node 3 (root B,  h=0.0)   -- neighbours []           (B's root)
    //   node 4 (in B,    h=10.0)  -- neighbours [3]          (downhill -> 3; the saddle side)
    //   node 2 (in A,    h=20.0)  -- neighbours [0]          (downhill -> 0; A's own escape)
    //   node 5 (in B,    h=20.0)  -- neighbours [3]          (downhill -> 3; B's own escape)
    //   node 6 (mouth,   h=-100.0) -- neighbours [2, 5]       (sea; sea_level_m = -50.0)
    //
    // Basin A = {0, 1, 2}. External crossings: 1--4 (max(10, 10) = 10, into B) and 2--6
    // (max(20, -100) = 20, into the mouth). Min = 10 -> level_m(A) = 10, target = B.
    // Basin B = {3, 4, 5}. External crossings: 4--1 (10, into A) and 5--6 (20, into the
    // mouth). Min = 10 -> level_m(B) = 10, target = A. **A tied plateau, exactly as
    // designed.**
    //
    // The union {0,1,2,3,4,5}'s own rim, excluding the internal 1--4 saddle: 2--6 (20) and
    // 5--6 (20). Both candidates agree at 20 -- the merged body's true level, matching the
    // physical expectation exactly (both basins' independent 20 m escapes are, of course,
    // the same height, since the fixture is symmetric by construction).
    fn merge_fixture() -> (StreamGraph, Vec<Vec<u32>>) {
        let positions = vec![
            SpherePoint::from_latlon(0.0, 0.0),
            SpherePoint::from_latlon(10.0, 0.0),
            SpherePoint::from_latlon(20.0, 0.0),
            SpherePoint::from_latlon(0.0, 10.0),
            SpherePoint::from_latlon(10.0, 10.0),
            SpherePoint::from_latlon(20.0, 10.0),
            SpherePoint::from_latlon(30.0, 5.0),
        ];
        let heights = vec![0.0, 10.0, 20.0, 0.0, 10.0, 20.0, -100.0];
        let areas = vec![1.0e9; 7];
        let neighbours =
            vec![vec![], vec![0, 4], vec![0], vec![], vec![3], vec![3], vec![2, 5]];
        let params = BuildParams {
            world_seed: 5,
            radius_m: EARTH_RADIUS_M,
            sea_level_m: -50.0, // node 6 (-100.0) is BOUNDARY; nodes 0-5 are LAND.
            sampling_kind: crate::stream::SamplingKind::Supplied,
            pond_max_surface_area_m2: 1.0,
        };
        let graph = StreamGraph::build(&params, &positions, &heights, &areas, &neighbours)
            .expect("the merge fixture builds a valid graph");
        (graph, neighbours)
    }

    #[test]
    fn merge_fixture_has_the_two_tied_roots_this_test_relies_on() {
        let (mut graph, directed) = merge_fixture();
        let symmetric = symmetric_adjacency(&directed);
        let _basins = fill_fixture_lakes(&mut graph, &symmetric);
        assert_eq!(graph.roots(), vec![0, 3, 6], "fixture drifted: expected roots at 0, 3, 6");
        assert_eq!(graph.lake_at(0).expect("lake A").level_m, 10.0, "fixture drifted: A must tie at 10");
        assert_eq!(graph.lake_at(3).expect("lake B").level_m, 10.0, "fixture drifted: B must tie at 10");
        assert!(graph.lake_at(6).is_none(), "node 6 must be a mouth, not a lake");
    }

    /// The silent failure the brief names by name, and the ruling that replaced the first
    /// fix: a tied plateau is one body of water, merged rather than cut. Both lakes' levels
    /// rise from the tied 10 m to the union's true 20 m outlet, and the pair stays connected
    /// to the sea through the lower-`root_node` representative (0) rather than one of them
    /// being fabricated into a false terminal at 10 m.
    #[test]
    fn a_tied_plateau_is_merged_into_one_body_at_its_true_level() {
        let (mut graph, directed) = merge_fixture();
        let symmetric = symmetric_adjacency(&directed);
        let basins = fill_fixture_lakes(&mut graph, &symmetric);

        let edges = resolve_outflow_edges(&graph, &basins, &symmetric);
        let a = edges.iter().find(|e| e.root_node == 0).expect("lake A's edge");
        let b = edges.iter().find(|e| e.root_node == 3).expect("lake B's edge");

        // The representative is the smaller root_node: 0 < 3.
        assert_eq!(a.revised_level_m, Some(20.0), "A's level must rise to the union's true outlet");
        assert_eq!(b.revised_level_m, Some(20.0), "B's level must rise to the union's true outlet");
        assert_eq!(a.outflow_lake, NO_LAKE, "the representative (0) must carry the real outlet: the sea");
        assert_eq!(b.outflow_lake, 0, "the non-representative (3) must point at the representative");

        apply_outflows(&mut graph, &edges);
        assert_eq!(graph.lake_at(0).expect("lake A").level_m, 20.0, "apply_outflows must write the revised level");
        assert_eq!(graph.lake_at(3).expect("lake B").level_m, 20.0, "apply_outflows must write the revised level");
        assert_eq!(graph.lake_at(0).expect("lake A").outflow_lake, NO_LAKE);
        assert_eq!(graph.lake_at(3).expect("lake B").outflow_lake, 0);
    }

    /// A lake merging did not touch keeps `Lake::level_m` exactly as `fill_lakes` computed
    /// it -- `revised_level_m` being `Some` for the tied pair must not leak onto an unrelated
    /// lake in the same resolve call. Reuses `ordering_disagreement_fixture`, which has no
    /// tie at all (Property 5 holds strictly, 65 > 60), specifically because it is a
    /// different fixture than the merge one: a shared fixture would not distinguish "merge
    /// leaves untouched lakes alone" from "this fixture never triggers merge at all".
    #[test]
    fn merge_leaves_an_untied_lakes_level_untouched() {
        let (mut graph, directed) = ordering_disagreement_fixture();
        let symmetric = symmetric_adjacency(&directed);
        let basins = fill_fixture_lakes(&mut graph, &symmetric);
        let edges = resolve_outflow_edges(&graph, &basins, &symmetric);
        for edge in &edges {
            assert_eq!(
                edge.revised_level_m, None,
                "lake {} was not part of any tied plateau in this fixture and must not have \
                 a revised level",
                edge.root_node,
            );
        }
    }

    // ---- the chained-merge fixture: a merge whose second pass needs the first's members ---
    //
    // Fix-round review Finding 3: no test exercised a merge that needs more than one pass,
    // which is exactly the shape the membership-loss bug (this module's own top-level doc
    // comment on `merge_tied_plateaus`) lived in. Ten nodes, three basins:
    //
    //   node 0 (root A, h=0.0)   -- neighbours []          (A's root)
    //   node 1 (in A,   h=10.0)  -- neighbours [0, 4]       (downhill -> 0; the A-B saddle)
    //   node 2 (in A,   h=20.0)  -- neighbours [0]          (downhill -> 0; touches C)
    //   node 3 (root B, h=0.0)   -- neighbours []           (B's root)
    //   node 4 (in B,   h=10.0)  -- neighbours [3]          (downhill -> 3; the A-B saddle)
    //   node 5 (in B,   h=25.0)  -- neighbours [3]          (downhill -> 3; B's own escape)
    //   node 6 (mouth,  h=-100.0) -- neighbours [5]          (sea; sea_level_m = -50.0)
    //   node 7 (root C, h=15.0)  -- neighbours []           (C's root)
    //   node 8 (in C,   h=20.0)  -- neighbours [7, 2]       (downhill -> 7; touches A)
    //
    // Pass 1: A's candidates are 1--4 (max(10,10)=10, into B) and 2--8 (max(20,20)=20, into
    // C); min 10, target B. B's only candidate is 4--1 (10, into A); tied with A -- a genuine
    // plateau, merged. C's own candidate is 8--2 (20, into A's *original* basin, root 0) --
    // **not tied with A's own pre-merge level (20 != 10), so C does not merge in pass 1.**
    //
    // The pass-1 merge's own rim scan (union {0,1,2,3,4,5}, excluding the internal 1--4
    // saddle) finds two candidates: 2--8 (20, into C) and 5--6 (25, into the mouth); the
    // group's new level is **20**, matching C's own level exactly -- a tie that only exists
    // *because* the merged group's rim scan reached past the first saddle. This is pass 2's
    // tie: the group (containing A **and B**) merges with C.
    //
    // The decisive check is the *second* merge's own rim scan. If the group's accumulated
    // membership were lost (re-derived from A's and C's raw basins alone, forgetting B --
    // this function's own doc comment names exactly this mistake), the 1--4 saddle would be
    // wrongly treated as external again, surfacing a spurious 10 m candidate and pointing
    // back at a root (B) already inside the same body -- the exact shape of the original
    // non-terminating bug. With B's membership correctly carried forward, the only rim edge
    // that survives exclusion is 5--6 (25, into the mouth): the final level is **25**, not
    // 10, and the final target is the sea, not B.
    fn chained_merge_fixture() -> (StreamGraph, Vec<Vec<u32>>) {
        let positions = vec![
            SpherePoint::from_latlon(0.0, 0.0),
            SpherePoint::from_latlon(10.0, 0.0),
            SpherePoint::from_latlon(20.0, 0.0),
            SpherePoint::from_latlon(0.0, 10.0),
            SpherePoint::from_latlon(10.0, 10.0),
            SpherePoint::from_latlon(20.0, 10.0),
            SpherePoint::from_latlon(30.0, 5.0),
            SpherePoint::from_latlon(0.0, 20.0),
            SpherePoint::from_latlon(10.0, 20.0),
        ];
        let heights = vec![0.0, 10.0, 20.0, 0.0, 10.0, 25.0, -100.0, 15.0, 20.0];
        let areas = vec![1.0e9; 9];
        let neighbours = vec![
            vec![],
            vec![0, 4],
            vec![0],
            vec![],
            vec![3],
            vec![3],
            vec![5],
            vec![],
            vec![7, 2],
        ];
        let params = BuildParams {
            world_seed: 6,
            radius_m: EARTH_RADIUS_M,
            sea_level_m: -50.0, // node 6 (-100.0) is BOUNDARY; every other node is LAND.
            sampling_kind: crate::stream::SamplingKind::Supplied,
            pond_max_surface_area_m2: 1.0,
        };
        let graph = StreamGraph::build(&params, &positions, &heights, &areas, &neighbours)
            .expect("the chained-merge fixture builds a valid graph");
        (graph, neighbours)
    }

    #[test]
    fn chained_merge_fixture_has_the_three_roots_this_test_relies_on() {
        let (mut graph, directed) = chained_merge_fixture();
        let symmetric = symmetric_adjacency(&directed);
        let _basins = fill_fixture_lakes(&mut graph, &symmetric);
        assert_eq!(graph.roots(), vec![0, 3, 6, 7], "fixture drifted: expected roots at 0, 3, 6, 7");
        assert_eq!(graph.lake_at(0).expect("lake A").level_m, 10.0, "fixture drifted: A must tie at 10 with B");
        assert_eq!(graph.lake_at(3).expect("lake B").level_m, 10.0, "fixture drifted: B must tie at 10 with A");
        assert_eq!(graph.lake_at(7).expect("lake C").level_m, 20.0, "fixture drifted: C's own level must be 20");
        assert!(graph.lake_at(6).is_none(), "node 6 must be a mouth, not a lake");
    }

    /// The regression this finding asks for: a merge that needs a **second** pass, and whose
    /// second pass only gets the right answer if the first pass's full membership (including
    /// `B`, which is not a party to the second tie at all) survives into the rescan. All
    /// three lakes end up at the true 25 m sea outlet, not the intermediate 20 m tie C's own
    /// independent resolution found, and not the original 10 m A-B saddle.
    #[test]
    fn a_chained_merge_carries_the_first_passs_full_membership_into_the_second() {
        let (mut graph, directed) = chained_merge_fixture();
        let symmetric = symmetric_adjacency(&directed);
        let basins = fill_fixture_lakes(&mut graph, &symmetric);

        let edges = resolve_outflow_edges(&graph, &basins, &symmetric);
        let a = edges.iter().find(|e| e.root_node == 0).expect("lake A's edge");
        let b = edges.iter().find(|e| e.root_node == 3).expect("lake B's edge");
        let c = edges.iter().find(|e| e.root_node == 7).expect("lake C's edge");

        // The representative is the smallest root_node across all three: 0.
        assert_eq!(a.revised_level_m, Some(25.0), "A must end at the union's true 25 m outlet");
        assert_eq!(b.revised_level_m, Some(25.0), "B must end at the union's true 25 m outlet");
        assert_eq!(c.revised_level_m, Some(25.0), "C must end at the union's true 25 m outlet");
        assert_eq!(a.outflow_lake, NO_LAKE, "the representative (0) must carry the real outlet: the sea");
        assert_eq!(b.outflow_lake, 0, "B must point at the representative, not at A's pre-merge root only");
        assert_eq!(c.outflow_lake, 0, "C must point at the representative, not stay tied at its own 20 m");

        apply_outflows(&mut graph, &edges);
        assert_eq!(graph.lake_at(0).expect("lake A").level_m, 25.0);
        assert_eq!(graph.lake_at(3).expect("lake B").level_m, 25.0);
        assert_eq!(graph.lake_at(7).expect("lake C").level_m, 25.0);
    }

    /// `assert_lake_graph_acyclic` at length three, not just two -- a two-lake check alone
    /// would not distinguish "detects a cycle" from "detects a *mutual* pair specifically".
    /// Built directly from hand-authored `Lake` records (bypassing `StreamGraph::build`
    /// entirely, which has no way to construct a lake table with a cycle already in it) --
    /// this is the one test in this module that reaches for the private
    /// `assert_lake_graph_acyclic` directly rather than through the public entry points.
    #[test]
    #[should_panic(expected = "the lake super-graph has a cycle")]
    fn a_three_lake_cycle_is_also_caught() {
        let lakes = vec![
            Lake { root_node: 10, level_m: 1.0, kind: LakeKind::Lake, outflow_lake: 20 },
            Lake { root_node: 20, level_m: 1.0, kind: LakeKind::Lake, outflow_lake: 30 },
            Lake { root_node: 30, level_m: 1.0, kind: LakeKind::Lake, outflow_lake: 10 },
        ];
        assert_lake_graph_acyclic(&lakes);
    }

    /// The non-cyclic sibling of the test above, over the same hand-authored shape: a chain
    /// that terminates must peel cleanly and must not panic.
    #[test]
    fn a_lake_chain_terminating_at_the_sentinel_peels_cleanly() {
        let lakes = vec![
            Lake { root_node: 10, level_m: 3.0, kind: LakeKind::Lake, outflow_lake: 20 },
            Lake { root_node: 20, level_m: 2.0, kind: LakeKind::Lake, outflow_lake: 30 },
            Lake { root_node: 30, level_m: 1.0, kind: LakeKind::Lake, outflow_lake: NO_LAKE },
        ];
        assert_lake_graph_acyclic(&lakes); // must not panic
    }

    /// Property 4, directly: an `outflow_lake` that names a node absent from the table at
    /// all (not `NO_LAKE`, and not any `root_node` present) must be refused, distinctly from
    /// a cycle -- this table has no cycle in it, only a dangling reference.
    #[test]
    #[should_panic(expected = "names no lake root in this table")]
    fn an_outflow_lake_naming_no_real_lake_root_is_refused() {
        let lakes = vec![Lake { root_node: 10, level_m: 1.0, kind: LakeKind::Lake, outflow_lake: 999 }];
        assert_lake_graph_acyclic(&lakes);
    }

    // ---- over a real, Spiral-sampled graph -----------------------------------------------

    /// Properties 1 through 6, over every lake a real graph produces -- not a sample.
    #[test]
    fn resolve_outflows_over_a_real_graph_satisfies_every_property() {
        let mut graph = real_graph(SEED);
        let basins = fill_basins_and_apply(&mut graph);
        assert!(!graph.lakes().is_empty(), "fixture must actually have lakes to test this");

        resolve_outflows_and_apply(&mut graph, &basins); // Property 2 (no cycles): would
        // panic here if violated.

        let lake_roots: std::collections::HashSet<u32> =
            graph.lakes().iter().map(|l| l.root_node).collect();
        // Sanity for the test itself, not a property: every root really is unique (already
        // guaranteed by GraphDefect::DuplicateLakeRoot at build time).
        assert_eq!(lake_roots.len(), graph.lakes().len());

        let mut terminal_count = 0usize;
        for lake in graph.lakes() {
            // Property 3: no self-edges.
            assert_ne!(lake.outflow_lake, lake.root_node, "lake {} points at itself", lake.root_node);

            if lake.outflow_lake == NO_LAKE {
                terminal_count += 1;
                continue;
            }
            // Property 4: every non-sentinel outflow_lake names a real lake root.
            assert!(
                lake_roots.contains(&lake.outflow_lake),
                "lake {}'s outflow_lake {} does not name a real lake root",
                lake.root_node,
                lake.outflow_lake,
            );
            // Property 5: the target's level sits at or below the source's.
            let target_level = graph.lake_at(lake.outflow_lake).expect("target lake").level_m;
            assert!(
                target_level <= lake.level_m,
                "lake {} (level {}) outflows into lake {} (level {}), which is higher -- the \
                 super-graph runs uphill",
                lake.root_node,
                lake.level_m,
                lake.outflow_lake,
                target_level,
            );
        }
        // Property 6: at least one terminal lake exists, or the sentinel path is untested.
        assert!(terminal_count > 0, "no terminal lake in this fixture -- Property 6 untested");
    }

    /// Property 1 (determinism), over a real graph rather than the small fixture above.
    ///
    /// Fix-round review Finding 5: this compared `root_node` and `outflow_lake` only --
    /// `revised_level_m`, the field this round's merge added and the whole reason a second
    /// run could disagree with the first (a float recomputed from a `HashMap`-backed
    /// union-find, rather than merely copied), went unchecked. A nondeterministic level
    /// revision would have passed this test silently. The reviewer verified determinism of
    /// the full record independently (203 lakes at 30,000 nodes, 54 revised, bit-identical
    /// across runs); this closes the test gap that made that verification necessary by hand.
    #[test]
    fn resolve_outflows_is_bit_identical_across_two_runs_on_a_real_graph() {
        let mut graph = real_graph(SEED);
        let basins = fill_basins_and_apply(&mut graph);
        assert!(!graph.lakes().is_empty(), "fixture must actually have lakes to test this");

        let first = resolve_outflows(&graph, &basins);
        let second = resolve_outflows(&graph, &basins);
        assert_eq!(first.len(), second.len());
        let mut any_revised = false;
        for (a, b) in first.iter().zip(second.iter()) {
            assert_eq!(a.root_node, b.root_node);
            assert_eq!(
                a.outflow_lake, b.outflow_lake,
                "lake {} resolved to different outflow targets across two runs",
                a.root_node,
            );
            match (a.revised_level_m, b.revised_level_m) {
                (None, None) => {}
                (Some(x), Some(y)) => {
                    any_revised = true;
                    assert_eq!(
                        x.to_bits(),
                        y.to_bits(),
                        "lake {} revised to different levels across two runs: {x} vs {y}",
                        a.root_node,
                    );
                }
                (x, y) => panic!(
                    "lake {}: one run revised its level and the other did not ({x:?} vs {y:?})",
                    a.root_node,
                ),
            }
        }
        assert!(
            any_revised,
            "fixture must actually revise at least one lake's level, or this test does not \
             exercise the field Finding 5 named"
        );
    }

    /// Measures the lake count M this generator produces at a stated node count N, for
    /// section 14.2's O(N + M log M) claim. Review Finding 8: `assert!(m > 0 && m < n)` alone
    /// cannot discriminate anything this task could plausibly break (M would have to reach
    /// N/4,000 nodes' worth of lakes, or drop to zero, before either bound moved) -- pinned
    /// to the exact count instead, so a change to the seed, `NODES`, `real_graph`'s
    /// parameters, or an actual regression in lake classification all show up as a named
    /// failure rather than a still-green test. See the task report for the fuller table
    /// across multiple N and the conclusion drawn from it.
    #[test]
    fn lake_count_is_measured_at_a_stated_node_count() {
        let graph = real_graph(SEED);
        let m = graph.lakes().len();
        let n = graph.node_count();
        assert_eq!(
            m, 16,
            "M drifted from the pinned figure at N = {n}, seed {SEED} -- re-measure and \
             update both this pin and the task report's table if the drift is intended \
             (a generator or fixture change), not silently accepted."
        );
    }

    // ---- Task 3: pond/lake classification --------------------------------------------
    //
    // Owner decision, 2026-09-05: the pond/lake split moved from drainage area to surface
    // area (`classify_lake_kinds`'s own doc comment has the measurement that drove it).
    // Every test below was re-derived against the new quantity, not merely renamed: a
    // single isolated-root fixture's "surface" and "drainage" happen to be the same number
    // (its one basin member is the root itself, always at or below its own level), so those
    // fixtures below transfer unchanged in value; `merge_fixture`'s merged-body test needed
    // its own re-derivation because a body's surface sums only the members *at or below the
    // filled level*, not the whole basin -- see that test's own doc comment for why this
    // particular fixture's numbers come out equal anyway.

    /// A single isolated root -- no neighbours at all, so its only basin member is itself,
    /// always at or below its own filled `level_m` (`level_m` never drops below the root's
    /// own height -- `fill_lakes`' own invariant). Its surface area is therefore exactly
    /// `area_m2[0]`, chosen here to land precisely on the threshold every boundary test
    /// below uses. Far below `sea_level_m` would make it a mouth, not a lake, so
    /// `sea_level_m` sits far beneath every height here, mirroring `touching_lakes_fixture`'s
    /// own reason for the same choice.
    const SINGLE_LAKE_SURFACE_AREA_M2: f64 = 5.0e9;

    fn single_lake_fixture(pond_max_surface_area_m2: f64) -> StreamGraph {
        let positions = vec![SpherePoint::from_latlon(0.0, 0.0)];
        let heights = vec![0.0];
        let areas = vec![SINGLE_LAKE_SURFACE_AREA_M2];
        let neighbours: Vec<Vec<u32>> = vec![vec![]];
        let params = BuildParams {
            world_seed: 2,
            radius_m: EARTH_RADIUS_M,
            sea_level_m: -1.0e6,
            sampling_kind: crate::stream::SamplingKind::Supplied,
            pond_max_surface_area_m2,
        };
        StreamGraph::build(&params, &positions, &heights, &areas, &neighbours)
            .expect("the single-lake fixture builds a valid graph")
    }

    /// Property 3 and the trap the brief names by name: a lake whose surface area is
    /// exactly the threshold is a pond ("at or below", `stream.rs::BuildParams::
    /// pond_max_surface_area_m2`'s own doc comment), not merely one strictly under it. This
    /// fixture's one lake has no basin members beyond itself (empty neighbours), so its
    /// surface area is exactly `area_m2[0]` -- set to precisely
    /// `SINGLE_LAKE_SURFACE_AREA_M2`, the same value passed as the threshold, so this test
    /// sits exactly on the boundary rather than near it.
    ///
    /// The whole test is one assertion on purpose: this slice has already shipped three
    /// tests that caught a mutation through a sibling assertion rather than their own
    /// (the brief's own finding). A single-assertion test cannot be shadowed -- verified by
    /// actually making the mutation this test exists to catch: `classify_lake_kinds`'s `<=`
    /// changed to `<` turns this test red (surface 5.0e9 is not `< 5.0e9`, so the lake comes
    /// out `Lake` instead of `Pond`); reverting the operator turns it back green. Restored
    /// afterwards; this comment records that the mutation was actually run, not merely
    /// reasoned about.
    #[test]
    fn a_lake_whose_surface_area_equals_the_threshold_is_a_pond() {
        // Built-time threshold deliberately does not match the real one below, so this test
        // cannot pass by accident of `StreamGraph::build`'s own initial classification.
        let mut graph = single_lake_fixture(0.0);
        let basins = basins_of(&graph);
        classify_lake_kinds(&mut graph, &basins, SINGLE_LAKE_SURFACE_AREA_M2);
        assert_eq!(graph.lake_at(0).expect("the fixture's one lake").kind, LakeKind::Pond);
    }

    /// The mirror boundary case: strictly over the threshold must not be a pond. Paired with
    /// the equals-case test above so the boundary is bracketed on both sides, not asserted
    /// from one side only. A fresh fixture (rather than reusing `single_lake_fixture` at a
    /// shifted area) so this test's own surface area is a literal, visible number, not one
    /// derived by arithmetic on the other test's constant.
    #[test]
    fn a_lake_whose_surface_area_is_strictly_over_the_threshold_is_not_a_pond() {
        let positions = vec![SpherePoint::from_latlon(0.0, 0.0)];
        let heights = vec![0.0];
        let areas = vec![5.000_000_001e9];
        let neighbours: Vec<Vec<u32>> = vec![vec![]];
        let params = BuildParams {
            world_seed: 3,
            radius_m: EARTH_RADIUS_M,
            sea_level_m: -1.0e6,
            sampling_kind: crate::stream::SamplingKind::Supplied,
            pond_max_surface_area_m2: 0.0,
        };
        let mut graph = StreamGraph::build(&params, &positions, &heights, &areas, &neighbours)
            .expect("the over-threshold fixture builds a valid graph");
        let basins = basins_of(&graph);
        classify_lake_kinds(&mut graph, &basins, SINGLE_LAKE_SURFACE_AREA_M2);
        assert_eq!(graph.lake_at(0).expect("the fixture's one lake").kind, LakeKind::Lake);
    }

    /// Property 2: classification is total, and specifically **not** a pass-through of
    /// whatever `StreamGraph::build`'s own initial, pre-merge guess happened to be.
    /// `single_lake_fixture` is built with an enormous throwaway threshold, so `build`'s own
    /// placeholder classification (the root's own single-cell `area_m2` against that
    /// threshold) calls the lake a `Pond`; `classify_lake_kinds` is then run with a real
    /// threshold of `0.0`, under which every real (positive) surface area must be a `Lake`.
    /// If `classify_lake_kinds` merely trusted the row `build` had already written, this
    /// would still read `Pond` and the test would fail.
    #[test]
    fn classify_lake_kinds_overwrites_the_builders_own_initial_guess() {
        let mut graph = single_lake_fixture(1.0e30);
        assert_eq!(
            graph.lake_at(0).expect("the fixture's one lake").kind,
            LakeKind::Pond,
            "fixture drifted: build's own classification must start as Pond for this test to \
             mean anything"
        );
        let basins = basins_of(&graph);
        classify_lake_kinds(&mut graph, &basins, 0.0);
        assert_eq!(graph.lake_at(0).expect("the fixture's one lake").kind, LakeKind::Lake);
    }

    /// Determinism (Property 1): the same graph, classified twice at the same threshold,
    /// must produce the same `kind` for every lake -- `LakeKind` is a plain enum rather than
    /// a float, so this is an exact `==`, not a bits comparison (`StreamGraph::
    /// bit_identical_to`'s own convention is for float fields; `kind` has none here).
    #[test]
    fn classify_lake_kinds_is_deterministic_across_two_runs() {
        let mut a = real_graph(SEED);
        let mut b = real_graph(SEED);
        fill_and_resolve_water(&mut a, 5.0e9);
        fill_and_resolve_water(&mut b, 5.0e9);
        assert!(!a.lakes().is_empty(), "fixture must actually have lakes to test this");
        let kinds_a: Vec<(u32, LakeKind)> = a.lakes().iter().map(|l| (l.root_node, l.kind)).collect();
        let kinds_b: Vec<(u32, LakeKind)> = b.lakes().iter().map(|l| (l.root_node, l.kind)).collect();
        assert_eq!(kinds_a, kinds_b);
    }

    /// The reason this task exists as a whole module addition rather than trusting
    /// `StreamGraph::build`'s own per-root pass: a merged plateau's surface is the union's
    /// surface (Ruling 7 -- a tied plateau is one body of water), not either root's own
    /// basin summed alone, and a threshold between the individual and the combined figure
    /// must classify both members by the combined one.
    ///
    /// `merge_fixture` ties roots 0 and 3 into one physical body with a real outlet through
    /// node 6, both revised to the union's true spill level of 20.0 m
    /// (`a_tied_plateau_is_merged_into_one_body_at_its_true_level` pins this exact figure).
    /// At that level every member of both basins (`{0,1,2}` at heights 0/10/20 and
    /// `{3,4,5}` at heights 0/10/20, three 1.0e9 m² nodes each) sits at or below 20.0 m, so
    /// each root's own basin surface comes out to the same 3.0e9 m² its drainage area would
    /// -- **this fixture's basins are fully submerged at the merged level, which is why its
    /// numbers do not distinguish surface from drainage; the assertions below pin that this
    /// is actually true here, not assumed.** What the fixture DOES distinguish is a body's
    /// combined total (6.0e9) from either root's own total alone (3.0e9): `4.0e9` sits
    /// strictly between them, so this test fails if `classify_lake_kinds` used each row's
    /// own basin surface instead of its body's.
    #[test]
    fn a_merged_bodys_kind_is_decided_by_its_combined_surface_area_not_either_roots_own() {
        let (mut graph, directed) = merge_fixture();
        let symmetric = symmetric_adjacency(&directed);
        let basins = basins_of(&graph);
        let filled = fill_lakes(&graph, &basins, &symmetric);
        apply_levels(&mut graph, &filled);
        let edges = resolve_outflow_edges(&graph, &basins, &symmetric);
        apply_outflows(&mut graph, &edges);

        let surface_of = |root: u32| -> f64 {
            let level_m = graph.lake_at(root).expect("root names a lake in this table").level_m;
            let mut total = 0.0;
            for &member in basins.members_of(root) {
                if graph.height_m(member) <= level_m {
                    total += graph.area_m2(member);
                }
            }
            total
        };
        assert_eq!(
            surface_of(0),
            3.0e9,
            "fixture drifted: root 0's own basin surface must total 3.0e9 m^2 (fully \
             submerged at the merged 20.0 m level) for this test's threshold to sit where \
             this test's doc comment says it does"
        );
        assert_eq!(
            surface_of(3),
            3.0e9,
            "fixture drifted: root 3's own basin surface must total 3.0e9 m^2 (fully \
             submerged at the merged 20.0 m level) for this test's threshold to sit where \
             this test's doc comment says it does"
        );

        classify_lake_kinds(&mut graph, &basins, 4.0e9);

        assert_eq!(
            graph.lake_at(0).expect("root 0's lake row").kind,
            LakeKind::Lake,
            "root 0's own basin surface (3.0e9) is under the 4.0e9 threshold, but its \
             merged body's combined surface (6.0e9) is over it -- the body's total must \
             decide, not the row's own figure"
        );
        assert_eq!(
            graph.lake_at(3).expect("root 3's lake row (the merge satellite)").kind,
            LakeKind::Lake,
            "the merge satellite must carry the same body-total classification as its \
             representative, not its own smaller pre-merge figure"
        );
    }

    /// The quantity this classification no longer uses is still exposed and still correct:
    /// `lake_body_drainage_totals_m2` (drainage) and `lake_body_surface_areas_m2` (surface)
    /// must partition the same lake table into the same NUMBER of bodies -- both fold
    /// `merge_fixture`'s two tied roots into one body, so both report exactly one figure,
    /// not two.
    #[test]
    fn drainage_and_surface_body_totals_agree_on_how_many_bodies_there_are() {
        let (mut graph, directed) = merge_fixture();
        let symmetric = symmetric_adjacency(&directed);
        let basins = basins_of(&graph);
        let filled = fill_lakes(&graph, &basins, &symmetric);
        apply_levels(&mut graph, &filled);
        let edges = resolve_outflow_edges(&graph, &basins, &symmetric);
        apply_outflows(&mut graph, &edges);

        let drainage = lake_body_drainage_totals_m2(&graph);
        let surface = lake_body_surface_areas_m2(&graph, &basins);
        assert_eq!(drainage.len(), 1, "the merged pair must fold into exactly one body");
        assert_eq!(surface.len(), 1, "the merged pair must fold into exactly one body");
        assert_eq!(drainage[0], 6.0e9);
        assert_eq!(surface[0], 6.0e9, "fully submerged at the merged level, per the test above");
    }

    // ---- Task 4: the water manifest ----------------------------------------------------

    /// `smallest_enclosing_longitude_arc` on the ordinary case (no seam involved) must
    /// reproduce plain min/max exactly.
    #[test]
    fn smallest_enclosing_longitude_arc_matches_plain_min_max_away_from_the_seam() {
        let mut lons = vec![10.0, -5.0, 3.0];
        assert_eq!(smallest_enclosing_longitude_arc(&mut lons), (-5.0, 10.0));
    }

    /// A single point is its own (degenerate) arc.
    #[test]
    fn smallest_enclosing_longitude_arc_handles_a_single_point() {
        let mut lons = vec![42.0];
        assert_eq!(smallest_enclosing_longitude_arc(&mut lons), (42.0, 42.0));
    }

    /// The case Finding 2 named: two points a hair apart on the real planet, on opposite
    /// sides of the +/-180 seam, must produce the narrow arc across the seam (start > end),
    /// not the ~359-degree arc a naive min/max would report.
    #[test]
    fn smallest_enclosing_longitude_arc_wraps_across_the_antimeridian() {
        let mut lons = vec![179.5, -179.5];
        assert_eq!(smallest_enclosing_longitude_arc(&mut lons), (179.5, -179.5));
    }

    /// Three points, still all near the seam on one side or the other, none of them near the
    /// 0-degree meridian -- the widest gap must still be found on the far side (near 0), not
    /// assumed to be at the seam itself just because two points straddle it.
    #[test]
    fn smallest_enclosing_longitude_arc_finds_the_widest_gap_wherever_it_falls() {
        let mut lons = vec![179.0, -179.0, -178.0];
        // Sorted: [-179.0, -178.0, 179.0]. Gaps: (-179 to -178) = 1, (-178 to 179) = 357,
        // (179 to -179+360=181) = 2. Widest is the middle one (357, between -178 and 179),
        // so the arc runs the other way: from 179 up through the seam down to -178.
        assert_eq!(smallest_enclosing_longitude_arc(&mut lons), (179.0, -178.0));
    }

    /// Property 1: the same graph, manifested twice, must produce a bit-identical result --
    /// not merely a numerically close one. Compared on bits throughout, per this crate's own
    /// convention (`StreamGraph::bit_identical_to`'s doc comment).
    #[test]
    fn water_manifest_is_bit_identical_across_two_runs() {
        let mut a = real_graph(SEED);
        let mut b = real_graph(SEED);
        let basins_a = fill_and_resolve_water(&mut a, 5.0e9);
        let basins_b = fill_and_resolve_water(&mut b, 5.0e9);

        let manifest_a = water_manifest_from_graph(&a, &basins_a);
        let manifest_b = water_manifest_from_graph(&b, &basins_b);

        assert_eq!(manifest_a.sea_level_m.to_bits(), manifest_b.sea_level_m.to_bits());
        assert!(!manifest_a.bodies.is_empty(), "fixture must actually produce bodies to test this");
        assert_eq!(manifest_a.bodies.len(), manifest_b.bodies.len());
        for (x, y) in manifest_a.bodies.iter().zip(manifest_b.bodies.iter()) {
            assert_eq!(x.root_node, y.root_node);
            assert_eq!(x.kind, y.kind);
            assert_eq!(x.level_m.to_bits(), y.level_m.to_bits());
            assert_eq!(x.extent.min_latitude_deg.to_bits(), y.extent.min_latitude_deg.to_bits());
            assert_eq!(x.extent.max_latitude_deg.to_bits(), y.extent.max_latitude_deg.to_bits());
            assert_eq!(x.extent.min_longitude_deg.to_bits(), y.extent.min_longitude_deg.to_bits());
            assert_eq!(x.extent.max_longitude_deg.to_bits(), y.extent.max_longitude_deg.to_bits());
        }
        assert!(manifest_a.rivers.is_empty());
    }

    /// Property 2: every physical lake body appears exactly once, and no body's `root_node`
    /// is duplicated -- checked against a count derived independently of `water_manifest`'s
    /// own logic (the same representative test `classify_lake_kinds` and `lake_body_index`
    /// already use), so this does not simply re-run the function under test on itself. No
    /// mouth count is added in (review round, Finding 1): the sea is never enumerated as a
    /// body at all, so `bodies.len()` is exactly the lake/pond count, not that count plus one
    /// per mouth.
    #[test]
    fn every_physical_body_appears_exactly_once() {
        let mut graph = real_graph(SEED);
        let basins = fill_and_resolve_water(&mut graph, 5.0e9);
        let manifest = water_manifest_from_graph(&graph, &basins);

        let mut roots: Vec<u32> = manifest.bodies.iter().map(|b| b.root_node).collect();
        let before = roots.len();
        roots.sort_unstable();
        roots.dedup();
        assert_eq!(roots.len(), before, "a body's root_node must not repeat in the manifest");

        let (lakes, body) = lake_body_index(&graph);
        assert!(!lakes.is_empty(), "fixture must actually have lakes to test this");
        let distinct_lake_bodies = body.iter().enumerate().filter(|&(i, &b)| b == i).count();

        assert_eq!(manifest.bodies.len(), distinct_lake_bodies);
    }

    /// Property 3 (review round, Finding 1 -- inverted from the pre-review version, which
    /// asserted the opposite: that every boundary root appeared as an `Ocean` body). The sea
    /// is the mapping's fallback, not a row in it (`BodyKind`'s own doc comment): no boundary
    /// root may appear in `bodies` at all, and the one scalar that stands in for the sea,
    /// `WaterManifest::sea_level_m`, must be bit-identical to the graph's own datum.
    #[test]
    fn no_boundary_root_appears_as_a_body() {
        let mut graph = real_graph(SEED);
        let basins = fill_and_resolve_water(&mut graph, 5.0e9);
        let manifest = water_manifest_from_graph(&graph, &basins);

        let mouths: Vec<u32> =
            graph.roots().into_iter().filter(|&r| graph.has_flag(r, stream::flag::MOUTH)).collect();
        assert!(!mouths.is_empty(), "fixture must actually have mouths to test this");
        for root in mouths {
            assert!(
                manifest.bodies.iter().all(|b| b.root_node != root),
                "mouth {root} must not appear in `bodies` -- the sea is the mapping's \
                 fallback, never a row in it"
            );
        }
        assert_eq!(manifest.sea_level_m.to_bits(), graph.header().sea_level_m.to_bits());
    }

    /// Property 4: nothing is two things. Every body's `root_node` must be a lake root and
    /// never a mouth -- checked against that independent ground truth, not merely against
    /// `BodyKind` having no variant left to hold a mouth under.
    #[test]
    fn every_body_is_a_lake_root_never_a_mouth() {
        let mut graph = real_graph(SEED);
        let basins = fill_and_resolve_water(&mut graph, 5.0e9);
        let manifest = water_manifest_from_graph(&graph, &basins);
        assert!(!manifest.bodies.is_empty(), "fixture must actually produce bodies to test this");

        for found in &manifest.bodies {
            let is_mouth = graph.has_flag(found.root_node, stream::flag::MOUTH);
            let is_lake_root = graph.lake_at(found.root_node).is_some();
            assert_ne!(
                is_mouth, is_lake_root,
                "root {} must be exactly one of a mouth and a lake root -- StreamGraph::build's \
                 own validate() already refuses a graph where it is both or neither",
                found.root_node,
            );
            assert!(is_lake_root, "every body's root must be a lake root: root {}", found.root_node);
            match found.kind {
                BodyKind::Lake | BodyKind::Pond => {}
            }
        }
    }

    /// Property 5: the river arm exists, is expressible, and is empty. "Expressible" is
    /// checked directly -- a `Reach` carrying a real `gradient` is constructed and held by a
    /// `River` right here -- even though nothing in this slice populates one from a graph.
    #[test]
    fn the_river_arm_is_expressible_and_stays_empty() {
        let river = River { reaches: vec![stream::Reach { from_node: 0, to_node: 1, gradient: 0.25 }] };
        assert_eq!(river.reaches.len(), 1);
        assert_eq!(river.reaches[0].gradient, 0.25);

        let mut graph = real_graph(SEED);
        let basins = fill_and_resolve_water(&mut graph, 5.0e9);
        let manifest = water_manifest_from_graph(&graph, &basins);
        assert!(
            manifest.rivers.is_empty(),
            "Mark 2 populates no rivers; the manifest must not invent one"
        );
    }

    /// Property 6, and this slice's own finding stated as a test rather than left for someone
    /// to discover from an empty column: **no graph this generator bakes at any resolution
    /// this project uses produces a pond.** Task 3 measured the smallest body any
    /// `Spiral`-sampled graph makes at 7.9e8 m^2 against a 1.0e5 m^2 threshold -- nearly four
    /// orders of magnitude apart -- so `BodyKind::Pond` is only reachable from a hand-built
    /// fixture, never from `real_graph`. This one is built exactly like `touching_lakes_
    /// fixture` and `merge_fixture` above (a hand-authored `Supplied` graph, node 2's edge to
    /// node 0 asymmetric in the raw list so `symmetric_adjacency` is the thing that closes the
    /// rim), sized so node 0's own filled level submerges only itself, not its land neighbour.
    #[test]
    fn a_pond_appears_when_a_hand_built_fixture_makes_one() {
        let positions = vec![
            SpherePoint::from_latlon(0.0, 0.0),
            SpherePoint::from_latlon(10.0, 0.0),
            SpherePoint::from_latlon(0.0, 10.0),
        ];
        let heights = vec![0.0, 5.0, -100.0];
        let areas = vec![1.0e4, 1.0e9, 1.0e9];
        // Node 2 names node 0; node 0 does not name node 2 back. A direct 0->2 edge in
        // node 0's own list would give node 0 a downhill target (node 2 sits far below it),
        // which would disqualify node 0 as a lake root entirely. One-directional in the raw
        // list, closed by `symmetric_adjacency` for the rim scan alone -- exactly
        // `touching_lakes_fixture`'s own trick above, reused for the same reason.
        let neighbours = vec![vec![1], vec![0], vec![0]];
        let params = BuildParams {
            world_seed: 9,
            radius_m: EARTH_RADIUS_M,
            sea_level_m: -50.0, // node 2 (-100.0) is BOUNDARY; nodes 0-1 (0.0, 5.0) are LAND.
            sampling_kind: crate::stream::SamplingKind::Supplied,
            pond_max_surface_area_m2: 1.0e5,
        };
        let mut graph = StreamGraph::build(&params, &positions, &heights, &areas, &neighbours)
            .expect("the pond fixture builds a valid graph");
        assert_eq!(graph.roots(), vec![0, 2], "fixture drifted: expected roots at 0 and 2");

        let basins = basins_of(&graph);
        let symmetric = symmetric_adjacency(&neighbours);
        let filled = fill_lakes(&graph, &basins, &symmetric);
        apply_levels(&mut graph, &filled);
        let edges = resolve_outflow_edges(&graph, &basins, &symmetric);
        apply_outflows(&mut graph, &edges);
        classify_lake_kinds(&mut graph, &basins, 1.0e5);

        assert_eq!(
            graph.lake_at(0).expect("lake root 0").level_m,
            0.0,
            "fixture drifted: the only crossing out of {{0}} is 0--2, max(0.0, -100.0) = 0.0"
        );
        assert_eq!(
            graph.lake_at(0).expect("lake root 0").kind,
            LakeKind::Pond,
            "fixture drifted: node 0 alone (area 1.0e4) sits under the 1.0e5 threshold"
        );

        let manifest = water_manifest(&graph, &basins, &positions);

        let pond = manifest.bodies.iter().find(|b| b.root_node == 0).expect("the pond must appear");
        assert_eq!(pond.kind, BodyKind::Pond, "node 0's body must be classified Pond");
        assert_eq!(pond.level_m, 0.0);
        // The lake's underwater footprint is node 0 alone: node 1 sits at 5.0 m, above the
        // 0.0 m filled level, so it must NOT widen the extent even though it is a member of
        // the same basin. This is the discrimination this test exists for -- see below.
        assert_eq!(
            pond.extent.min_latitude_deg, 0.0,
            "the pond's extent must be node 0 alone, not node 0's whole basin"
        );
        assert_eq!(
            pond.extent.max_latitude_deg, 0.0,
            "the pond's extent must be node 0 alone (lat 0.0), not widened to include node 1 \
             (lat 10.0, above the filled level) -- a mutation that dropped the `height_m(member) \
             <= lake.level_m` filter in `lake_body_extents` would widen this to 10.0 and this \
             assertion is what catches it; `pond.kind` above is a different code path (`Lake::
             kind`, not the extent filter) and would still read `Pond` under that mutation, so \
             it cannot shadow this one."
        );
        assert_eq!(pond.extent.min_longitude_deg, 0.0);
        assert_eq!(pond.extent.max_longitude_deg, 0.0);

        // Node 2 is the mouth (BOUNDARY at -100.0 <= sea_level_m -50.0). Review round
        // (Finding 1): the sea is never enumerated as a body, so it must not appear here at
        // all, and `sea_level_m` alone carries its datum.
        assert!(
            manifest.bodies.iter().all(|b| b.root_node != 2),
            "the mouth (root 2) must not appear in `bodies` -- the sea is the mapping's \
             fallback, never a row in it"
        );
        assert_eq!(manifest.sea_level_m, -50.0);
    }

    /// Review round (Finding 2): the antimeridian is reachable at real generator scale, not
    /// only a possibility to reason about away in a doc comment (6 of 171 lake bodies at
    /// n = 30,000 nodes; 8 of 799 at n = 100,000), so it needs its own fixture rather than a
    /// paragraph asserting it unreachable. This lake's two underwater members sit 0.2 degrees
    /// apart on the actual planet -- one at 179.9, one at -179.8 -- but on opposite sides of
    /// the +/-180 seam. A plain min/max over raw longitudes would report a ~359.7-degree box,
    /// almost the entire planet, for a lake that is in truth a sliver; the fix
    /// (`smallest_enclosing_longitude_arc`) must report the narrow arc instead.
    #[test]
    fn a_lake_straddling_the_antimeridian_gets_the_narrow_arc_not_the_globe_spanning_one() {
        let positions = vec![
            SpherePoint::from_latlon(0.0, 179.9),
            SpherePoint::from_latlon(0.0, -179.8),
            SpherePoint::from_latlon(0.0, 0.0),
        ];
        let heights = vec![0.0, 3.0, -100.0];
        let areas = vec![1.0e9; 3];
        // Node 1's downhill target is 0 (height drop 3.0, the only candidate in its list).
        // Node 2 (the mouth) names node 1 in its own raw list: node 1 is *higher* than node 2,
        // so that drop is negative and never becomes a downhill candidate for node 2 (it stays
        // a root), but it does give `symmetric_adjacency` the 1--2 rim edge this lake's spill
        // depends on -- the one-directional trick `a_pond_appears_when_a_hand_built_fixture_
        // makes_one` uses, reused for the same reason. That rim edge sets the spill (and so
        // the level) to max(3.0, -100.0) = 3.0, which is exactly node 1's own height, so both
        // members end up underwater and both longitudes must enter the extent.
        let neighbours = vec![vec![], vec![0], vec![1]];
        let params = BuildParams {
            world_seed: 11,
            radius_m: EARTH_RADIUS_M,
            sea_level_m: -50.0, // node 2 (-100.0) is BOUNDARY; nodes 0-1 (0.0, 3.0) are LAND.
            sampling_kind: crate::stream::SamplingKind::Supplied,
            pond_max_surface_area_m2: 1.0,
        };
        let mut graph = StreamGraph::build(&params, &positions, &heights, &areas, &neighbours)
            .expect("the antimeridian fixture builds a valid graph");
        assert_eq!(graph.roots(), vec![0, 2], "fixture drifted: expected roots at 0 and 2");

        let basins = basins_of(&graph);
        let symmetric = symmetric_adjacency(&neighbours);
        let filled = fill_lakes(&graph, &basins, &symmetric);
        apply_levels(&mut graph, &filled);
        assert_eq!(
            graph.lake_at(0).expect("lake root 0").level_m,
            3.0,
            "fixture drifted: the only crossing out of {{0,1}} is 1--2, max(3.0, -100.0) = 3.0"
        );

        let manifest = water_manifest(&graph, &basins, &positions);
        let lake = manifest.bodies.iter().find(|b| b.root_node == 0).expect("the lake must appear");

        assert_eq!(lake.extent.min_latitude_deg, 0.0);
        assert_eq!(lake.extent.max_latitude_deg, 0.0);
        assert_eq!(
            lake.extent.min_longitude_deg, 179.9,
            "the minimal arc starts just past the widest gap, at this lake's own higher-side \
             member -- not at -179.8, which is what a plain min() over raw longitudes would \
             have picked"
        );
        assert_eq!(
            lake.extent.max_longitude_deg, -179.8,
            "the minimal arc ends at this lake's lower-side member after wrapping through the \
             seam -- not at 179.9, which is what a plain max() would have picked"
        );
        assert!(
            lake.extent.min_longitude_deg > lake.extent.max_longitude_deg,
            "a seam-crossing extent must report min > max, exactly the convention `Extent`'s \
             own doc comment names, so a naive `min <= max` reader cannot mistake this for an \
             ordinary (and here wildly wrong) 359.7-degree box"
        );
    }
}

