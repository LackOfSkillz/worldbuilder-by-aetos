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

use crate::stream::{self, Lake, SamplingKind, StreamGraph, NO_LAKE};

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

/// Merge every tied plateau among freshly-resolved `edges`, in place, before anything is
/// written back to `graph`. Called unconditionally from `resolve_outflow_edges`.
///
/// # A tied plateau is one body of water, not a tie to break
///
/// `lowest_crossing`'s target always has a level at or below the source's -- the same
/// physical edge that produces the source's spill is itself one of the target's own
/// candidate crossings, so the target's own minimum cannot exceed it (Property 5, and the
/// reasoning `resolve_outflow_edges`'s own doc comment gives for it). Following outflow edges
/// therefore never *raises* the level, so a cycle -- levels returning to where they started
/// -- can only close if **every** edge in it holds the level exactly constant. A cycle is
/// therefore always a plateau of lakes at one bit-identical surface level, mutually
/// connected across shared saddles. That is the definition of one body of water, and treating
/// it as two separate lakes with an arbitrary tie-break (this module's own first-round fix,
/// `break_cycles`, reviewed and rejected) is wrong in a way that is not merely cosmetic: when
/// two basins' cheapest exits are each other, *neither has actually found its true outlet*.
/// A basin that shares its cheapest exit with a neighbour has strictly more capacity than
/// Task 1's independent per-basin minimum credits it with -- the pair's *true* outflow is
/// whatever the **union** of their members spills into once the shared internal saddle no
/// longer counts as an exit, and that union's rim can sit strictly higher than either
/// basin's own tied crossing. Cutting one lake to `NO_LAKE` fabricates a terminal lake at a
/// level the basin does not actually hold and discards the merged body's real downstream
/// continuation entirely; this task's own real-graph tests found this is not a rare case
/// (see the task report's re-derived Finding 3 figures).
///
/// # The algorithm, and the bug in this function's first draft
///
/// For each detected plateau (a cycle `peel_lake_relation` finds): union every member of
/// every basin in it, then re-scan that union's rim **excluding internal edges** (an
/// `outside` node that is itself inside the union is not a rim crossing, even though it
/// would have been one for either basin alone) using the same `max(h_inside, h_outside)`
/// formula every other rim scan in this module uses. The winning crossing is the plateau's
/// true, possibly-higher level and its true outflow target. Every member of the plateau has
/// its `revised_level_m` set to that new level (Task 1's field, deliberately revised here --
/// see this module's own top-level doc comment on why that fence is lifted for exactly this
/// case); the member with the smallest `root_node` (deterministic, since roots are unique)
/// becomes the group's single representative and carries the real outflow target (another
/// lake, or `NO_LAKE` if the union's own rim leads to the sea); every other member's
/// `outflow_lake` is set to point at the representative, turning the plateau into an
/// acyclic chain that ends there.
///
/// A merged group's new outflow can itself tie with some other lake at the identical new
/// level, forming a further plateau a single pass would not resolve -- so this runs across
/// passes until a peel finds no stuck entries at all, exactly the hierarchical basin-merging
/// standard priority-flood formulations do. **This function's first draft re-derived each
/// pass's union from scratch via `basins.members_of(root)` on whatever roots were stuck
/// *that pass*, which drops any member accumulated by an *earlier* pass's merge** (a lake `B`
/// merged into representative `A` in pass 1 is not `A`'s own basin, so a pass-2 tie between
/// `A` and some `C` re-scanned only `A`'s and `C`'s raw members, excluding `B`'s -- which can
/// make an edge into `B`'s territory look external when it is not, letting the identical tie
/// regenerate forever). Measured directly: at a real seed and node count this looped without
/// terminating, the same 22-lake stuck set recurring pass after pass with no progress. The
/// fix carries each group's accumulated membership (`groups`) and per-lake group lookup
/// (`owner`) across passes, so a later pass that touches an already-merged representative
/// expands the *whole* accumulated group, not just its most recent raw basin. Termination is
/// now a standard union-find argument: each pass that does any work strictly reduces the
/// number of distinct groups by merging at least two into one, and the number of groups is
/// bounded by the lake count, so the total number of merges across every pass is at most
/// `graph.lakes().len() - 1`.
///
/// # Panics
///
/// Via `peel_lake_relation`, if any non-sentinel `outflow_lake` names a root absent from
/// `edges` itself (unreachable in practice). If a union's own rim is empty -- every neighbour
/// of every member of the merged union is itself inside the union -- meaning the union covers
/// the graph's entire node set, the same "impossible on an actual sphere" case `fill_lakes`'
/// own no-rim panic describes for a single basin (see that function's doc comment); reachable
/// only on a fixture deliberately shaped that way (this crate's own `touching_lakes_fixture`
/// is exactly such a fixture, which is why this module's cycle-handling tests do not use it
/// for the merge-with-a-real-outlet case).
fn merge_tied_plateaus(
    graph: &StreamGraph,
    basins: &Basins,
    neighbours: &[Vec<u32>],
    edges: &mut Vec<LakeOutflow>,
) {
    // `groups[representative]` carries a merged group's full accumulated membership: every
    // lake root ever merged into it, and the union of every one of those lakes' own raw
    // basin members. A representative is always the smallest `root_node` in its group
    // (assigned below), so a later pass that finds the same representative stuck again looks
    // it up here and expands the *whole* accumulated group -- not just that representative's
    // own original basin -- which is the fix for the membership-loss bug this function's own
    // doc comment describes. Absent from this map means "still its own untouched basin".
    let mut groups: HashMap<u32, (Vec<u32>, Vec<u32>)> = HashMap::new(); // root -> (lake_roots, node_members)

    loop {
        let roots: Vec<u32> = edges.iter().map(|e| e.root_node).collect();
        let outflow_of: Vec<u32> = edges.iter().map(|e| e.outflow_lake).collect();
        let (index_of, stuck) = peel_lake_relation(&roots, &outflow_of);
        if stuck.is_empty() {
            return;
        }

        // The induced relation over `stuck` alone is a union of disjoint simple cycles (a
        // standard fact about functional graphs: every node has at most one outgoing edge, so
        // whatever does not peel away as a tree is exactly a set of cycles with no further
        // structure) -- walking from any unvisited stuck node and following outflow pointers
        // therefore always returns to that same node without ever needing to cross into
        // another cycle.
        let mut visited = vec![false; edges.len()];
        for &start in &stuck {
            if visited[start] {
                continue;
            }
            let mut cycle_indices = Vec::new();
            let mut cur = start;
            loop {
                if visited[cur] {
                    break;
                }
                visited[cur] = true;
                cycle_indices.push(cur);
                cur = index_of[&edges[cur].outflow_lake];
            }
            let cycle_roots: Vec<u32> = cycle_indices.iter().map(|&i| edges[i].root_node).collect();

            // Expand every root in this cycle to its FULL accumulated group -- not just this
            // one pass's raw basin -- so a root that was already the representative of an
            // earlier merge brings that merge's members along rather than losing them.
            let mut all_lake_roots: Vec<u32> = Vec::new();
            let mut all_node_members: Vec<u32> = Vec::new();
            for &root in &cycle_roots {
                match groups.remove(&root) {
                    Some((lake_roots, node_members)) => {
                        all_lake_roots.extend(lake_roots);
                        all_node_members.extend(node_members);
                    }
                    None => {
                        all_lake_roots.push(root);
                        all_node_members.extend_from_slice(basins.members_of(root));
                    }
                }
            }
            let union_set: std::collections::HashSet<u32> = all_node_members.iter().copied().collect();

            // Re-scan the union's own rim, excluding edges whose outside end is itself part
            // of the union (an edge between two plateau members is internal, however far
            // apart their original basins were or however many passes ago they merged, not a
            // rim crossing).
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

            let outflow_lake = if graph.lake_at(winner.target_root).is_some() {
                winner.target_root
            } else {
                NO_LAKE
            };
            let representative =
                *all_lake_roots.iter().min().expect("a cycle's expanded group is never empty");

            for &lake_root in &all_lake_roots {
                let i = index_of[&lake_root];
                edges[i].revised_level_m = Some(winner.level_m);
                edges[i].outflow_lake =
                    if lake_root == representative { outflow_lake } else { representative };
            }
            groups.insert(representative, (all_lake_roots, all_node_members));
        }
        // Loop again: a merged group's new outflow can itself tie with another lake at the
        // identical new level, which this pass alone would not have resolved.
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

/// The full slice 5b water pipeline in one call: `basins_of`, `fill_lakes` +
/// `apply_levels` (Task 1), then `resolve_outflow_edges` + `apply_outflows` (Task 2) --
/// regenerating the neighbour relation from the seed **exactly once** and sharing it between
/// both halves, rather than the two independent entry points
/// (`fill_basins_and_apply`/`resolve_outflows_and_apply`) each paying for their own copy.
///
/// This is the fix for review Finding 6: `fill_basins_and_apply` then
/// `resolve_outflows_and_apply` in sequence measured at ~4.8 s **each** at 500,000 nodes,
/// essentially the entire cost of either call, 100% of it duplicated work. A real world-build
/// pipeline that wants both halves should call this rather than the two separately;
/// `fill_basins`/`fill_basins_and_apply`/`resolve_outflows`/`resolve_outflows_and_apply` all
/// still exist, unchanged, for a caller that genuinely wants only one half (or, for the first
/// two, a caller with a non-`Spiral` graph that supplies its own neighbour relation).
///
/// Hands back the basin partition, matching `fill_basins_and_apply`'s own return value --
/// both Task 1's and Task 2's writes now sit on `graph` itself.
///
/// # Panics
///
/// Same restriction as `fill_basins`/`resolve_outflows`: `graph.header().sampling_kind` must
/// be `Spiral`.
pub fn fill_and_resolve_water(graph: &mut StreamGraph) -> Basins {
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

    basins
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
            pond_max_drainage_area_m2: 1.0,
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
            pond_max_drainage_area_m2: 1.0,
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
            pond_max_drainage_area_m2: 1.0,
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
    #[test]
    fn resolve_outflows_is_bit_identical_across_two_runs_on_a_real_graph() {
        let mut graph = real_graph(SEED);
        let basins = fill_basins_and_apply(&mut graph);
        assert!(!graph.lakes().is_empty(), "fixture must actually have lakes to test this");

        let first = resolve_outflows(&graph, &basins);
        let second = resolve_outflows(&graph, &basins);
        assert_eq!(first.len(), second.len());
        for (a, b) in first.iter().zip(second.iter()) {
            assert_eq!(a.root_node, b.root_node);
            assert_eq!(
                a.outflow_lake, b.outflow_lake,
                "lake {} resolved to different outflow targets across two runs",
                a.root_node,
            );
        }
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
}

