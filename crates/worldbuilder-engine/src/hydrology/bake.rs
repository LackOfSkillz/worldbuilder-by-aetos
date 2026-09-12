//! The bake itself: `bake_stages` through the lake closure, `record_of` folding that into the
//! public `HydroRecord`, and the tests that exercise the two together. Split out of `mod.rs` so
//! the module that owns the public types stays a manageable size; nothing here changes behaviour.

use crate::surface::Surface;

use crate::hydrology::flood::{flood, ocean_seeds, NO_NODE};
use crate::hydrology::flow::{self, close_lakes, drainage_check};
use crate::hydrology::hollows::{self, find_hollows, judge, Fate};
use crate::hydrology::landgraph::LandGraph;
use crate::hydrology::reaches::{bifurcation_ratios, depth_m, extract, width_m};
use crate::hydrology::routing::{self, route, NO_LAKE};

use super::{
    Body, BodyKind, BakeStats, Downstream, Fall, HydroError, HydroParams, HydroRecord, NotchLine,
    ReachClass, ReachLine, ReachPoint,
};

/// The bake's working state up to and including the lake closure, before it is folded into a
/// `HydroRecord`. `bake()` is exactly `bake_stages` then `record_of`, so a survey that times
/// the two halves is timing what ships.
#[derive(Debug, Clone)]
pub struct BakeStages {
    pub graph: LandGraph,
    pub hollows: Vec<hollows::Hollow>,
    pub routing: routing::Routing,
    pub flow: Vec<f64>,
    pub closure: flow::Closure,
}

/// A threshold-like parameter must be a finite positive number; `name` is the field name, for
/// the error.
fn require_finite_positive(name: &'static str, value: f64) -> Result<(), HydroError> {
    if value.is_finite() && value > 0.0 {
        Ok(())
    } else {
        Err(HydroError::Params(name))
    }
}

/// Builds the `ReachPoint`s for one reach's node list, in order. Extracted out of `bake()` so
/// the terminal-point fix (Ruling 2, fix round 1) can be exercised directly on a hand fixture,
/// not only end to end.
///
/// Ruling 2 (fix round 1): a terminal ocean or lake node's own accumulated flow is the
/// ocean/lake's total inflow (everything that drains there), not this channel's -- the same
/// reason `reaches::extract` substitutes the second-to-last node's flow for classification.
/// Reuse that same q here so width_m/depth_m/flow_m2 stay the channel's own values all the way
/// to the last point.
///
/// Ruling I4: a terminal point's bed is the water it runs into, not the ground under it -- the
/// datum (0.0) for an ocean node, so a river mouth never carves the seabed, and the lake's own
/// level for a lake node.
pub(crate) fn reach_points(graph: &LandGraph, routing: &routing::Routing, hollows: &[hollows::Hollow], flow: &[f64], nodes: &[u32], params: &HydroParams) -> Vec<ReachPoint> {
    let last_index = nodes.len() - 1;
    let mut points = Vec::with_capacity(nodes.len());
    for (idx, &node) in nodes.iter().enumerate() {
        let (lat_deg, lon_deg) = graph.positions[node as usize].to_latlon();
        let at_ocean = graph.ocean[node as usize];
        let lake = routing.lake_of[node as usize];
        let is_terminal = idx == last_index && (at_ocean || lake != NO_LAKE);
        let q = if is_terminal && idx > 0 { flow[nodes[idx - 1] as usize] } else { flow[node as usize] };
        let w = width_m(q, params);
        let d = depth_m(q, params);
        let bed_m = if is_terminal && at_ocean {
            0.0
        } else if is_terminal {
            hollows[lake as usize].level_m
        } else {
            routing.surface_m[node as usize] - d
        };
        points.push(ReachPoint { lat_deg, lon_deg, bed_m, width_m: w, depth_m: d, flow_m2: q });
    }
    points
}

/// Where a kept hollow's water goes, past its own shore: follows `routing.receiver` from
/// `lake_entry` node by node, giving the first reach it reaches (`Reach`), the first member of a
/// *different* body it reaches with no reach between them (`Body`), an ocean node (`Ocean`), or
/// `Sink` if the chain runs out (`NO_NODE`) or the walk hits its bound without doing either --
/// the same `graph.len()` cap `reaches::extract` uses for a reach's own downstream, since by the
/// time `record_of` runs `bake_stages` has already called `drainage_check` and proved every
/// receiver chain terminates without revisiting, so this bound is never actually hit; it exists
/// only so a future caller that skips that check can't loop forever. `hollow_index` is this
/// hollow's own position in `hollows` (not its body id), the value `routing.lake_of` carries, so
/// a node still inside this same lake never counts as "a different body".
pub(crate) fn body_downstream(
    graph: &LandGraph,
    routing: &routing::Routing,
    hollow_index: u32,
    lake_entry: u32,
    reach_of_first_node: &[u32],
    body_id_of_hollow: &[Option<u32>],
) -> Downstream {
    let mut here = routing.receiver[lake_entry as usize];
    let mut steps = 0usize;
    loop {
        if here == NO_NODE {
            return Downstream::Sink;
        }
        if steps >= graph.len() {
            return Downstream::Sink;
        }
        steps += 1;
        let i = here as usize;
        // Precedence, checked in this order at each node: the ocean first, then a member of
        // another lake, then the start of a reach. A node that is more than one of these answers
        // with the first.
        if graph.ocean[i] {
            return Downstream::Ocean;
        }
        let lake = routing.lake_of[i];
        if lake != NO_LAKE && lake != hollow_index {
            let body_id = body_id_of_hollow[lake as usize]
                .expect("a lake member's hollow is always kept");
            return Downstream::Body(body_id);
        }
        if reach_of_first_node[i] != u32::MAX {
            return Downstream::Reach(reach_of_first_node[i]);
        }
        here = routing.receiver[i];
    }
}

/// SCHEMA 3's `ReachLine::fresh`: `false` if following `start` through reaches (`reach_downstream`,
/// id-indexed, remapped to body ids already) and bodies (`Body::downstream`) ends at `Sink` --
/// which only a closed lake ever reports (an open body always names somewhere else to go, and a
/// reach with nowhere downstream is itself `Sink`, the same terminal) -- `true` if it ends at
/// `Ocean`. Bounded the same way `bake_tests::follows_to_ocean` is: more hops than any acyclic
/// chain in this record could have, so running past it (a cycle) reports `false` rather than
/// looping -- "everything drains" (Ruling C1-c) means a real bake never does.
pub(crate) fn downstream_is_fresh(bodies: &[Body], reach_downstream: &[Downstream], start: Downstream) -> bool {
    let bound = reach_downstream.len() + bodies.len() + 1;
    let mut here = start;
    for _ in 0..bound {
        match here {
            Downstream::Ocean => return true,
            Downstream::Sink => return false,
            Downstream::Reach(id) => here = reach_downstream[id as usize],
            Downstream::Body(id) => here = bodies[id as usize].downstream,
        }
    }
    false
}

/// The bake up to the lake closure: validation, `LandGraph::sample` -> `flood(ocean_seeds)` ->
/// `find_hollows` + `judge` -> `route` -> `close_lakes`, then `flow::drainage_check` (Ruling
/// C1-c), which refuses a routing where any node's water fails to reach the sea or a sink.
pub fn bake_stages(surface: &Surface, params: &HydroParams) -> Result<BakeStages, HydroError> {
    if params.total_nodes < 2 || params.total_nodes > crate::stream::MAX_NODES {
        return Err(HydroError::Params("total_nodes must be in 2..=stream::MAX_NODES"));
    }
    require_finite_positive("keep_depth_m", params.keep_depth_m)?;
    require_finite_positive("keep_area_m2", params.keep_area_m2)?;
    require_finite_positive("pond_max_area_m2", params.pond_max_area_m2)?;
    require_finite_positive("stream_flow_m2", params.stream_flow_m2)?;
    require_finite_positive("river_flow_m2", params.river_flow_m2)?;
    require_finite_positive("great_flow_m2", params.great_flow_m2)?;
    require_finite_positive("notch_fall_m", params.notch_fall_m)?;
    require_finite_positive("evaporation_factor", params.evaporation_factor)?;
    require_finite_positive("salt_flat_share", params.salt_flat_share)?;
    require_finite_positive("min_stream_nodes", params.min_stream_nodes)?;
    require_finite_positive("keep_max_area_m2", params.keep_max_area_m2)?;
    require_finite_positive("refine_step_m", params.refine_step_m)?;
    require_finite_positive("refine_simplify_m", params.refine_simplify_m)?;
    require_finite_positive("refine_vertical_m", params.refine_vertical_m)?;
    require_finite_positive("fall_min_drop_m", params.fall_min_drop_m)?;
    require_finite_positive("fall_max_run_m", params.fall_max_run_m)?;
    require_finite_positive("meander_wavelength_widths", params.meander_wavelength_widths)?;
    require_finite_positive("meander_amplitude_widths", params.meander_amplitude_widths)?;
    require_finite_positive("meander_max_slope", params.meander_max_slope)?;
    require_finite_positive("pond_cell_m", params.pond_cell_m)?;
    require_finite_positive("pond_search_radius_m", params.pond_search_radius_m)?;
    require_finite_positive("pond_keep_depth_m", params.pond_keep_depth_m)?;
    require_finite_positive("pond_keep_area_m2", params.pond_keep_area_m2)?;
    require_finite_positive("pond_wetness_share", params.pond_wetness_share)?;
    require_finite_positive("pond_max_slope", params.pond_max_slope)?;
    require_finite_positive("pond_density_area_m2", params.pond_density_area_m2)?;
    if !(params.stream_flow_m2 <= params.river_flow_m2 && params.river_flow_m2 <= params.great_flow_m2) {
        return Err(HydroError::Params("stream_flow_m2 <= river_flow_m2 <= great_flow_m2 required"));
    }
    /// Above any catchment a planet can hold (Earth's whole surface is 5.1e14 m^2). Flow params
    /// above it are a typo, and `record_of`'s x10 steps would overflow them to infinity.
    const MAX_FLOW_M2: f64 = 1.0e20;
    /// More than any graph has nodes.
    const MAX_MIN_STREAM_NODES: f64 = 1.0e7;
    if params.great_flow_m2 > MAX_FLOW_M2 {
        return Err(HydroError::Params("flow thresholds must be <= 1e20 m^2"));
    }
    if params.min_stream_nodes > MAX_MIN_STREAM_NODES {
        return Err(HydroError::Params("min_stream_nodes must be <= 1e7"));
    }
    // Ruling FF-5: floors for the refinement params. Being finite and positive is not enough --
    // a 1e-300 m step plans a segment's worth of stations no machine will finish, a fall run far
    // under the step divides every step into millions of windows, and tolerances below these
    // cannot survive the arithmetic of a line measured in metres.
    if !(params.refine_step_m >= 10.0) {
        return Err(HydroError::Params("refine_step_m must be >= 10 m"));
    }
    if !(params.fall_max_run_m >= params.refine_step_m / 100.0) {
        return Err(HydroError::Params("fall_max_run_m must be >= refine_step_m / 100"));
    }
    if !(params.refine_simplify_m >= 1.0) {
        return Err(HydroError::Params("refine_simplify_m must be >= 1 m"));
    }
    if !(params.refine_vertical_m >= 0.01) {
        return Err(HydroError::Params("refine_vertical_m must be >= 0.01 m"));
    }
    // The same argument for the fine search: a cell far below the landform's own resolution
    // makes a strip no machine will finish, a search radius under one cell has no strip to
    // sample, and a wetness *share* above 1 can never be met.
    if !(params.pond_cell_m >= 10.0) {
        return Err(HydroError::Params("pond_cell_m must be >= 10 m"));
    }
    if !(params.pond_search_radius_m >= params.pond_cell_m) {
        return Err(HydroError::Params("pond_search_radius_m must be >= pond_cell_m"));
    }
    if !(params.pond_wetness_share <= 1.0) {
        return Err(HydroError::Params("pond_wetness_share must be <= 1"));
    }

    let graph = LandGraph::sample(surface, params.total_nodes, params.wetness_nodes)
        .ok_or(HydroError::Sampling)?;

    let global_flood = flood(&graph, &ocean_seeds(&graph), &|_| true);
    let mut hollows = find_hollows(&graph, &global_flood);
    let forced = hollows::forced_nodes(&graph, params);
    judge(&mut hollows, &forced, params);
    let mut routing = route(&graph, &global_flood, &mut hollows, params);
    let (flow, closure) = close_lakes(&graph, &mut routing, &hollows, params);
    drainage_check(&graph, &routing).map_err(HydroError::Drainage)?;
    Ok(BakeStages { graph, hollows, routing, flow, closure })
}

/// Folds a drained `BakeStages` into the public record: reaches (`extract`, on the effective
/// thresholds), bodies, the recorded notches and the stats.
pub fn record_of(stages: &BakeStages, params: &HydroParams) -> HydroRecord {
    let BakeStages { graph, hollows, routing, flow, closure } = stages;

    // Ruling 12b-1: the effective thresholds a graph this coarse can actually resolve. Sorted,
    // deterministic median of the land-node areas (the lower of the two middles on an even
    // count), never a HashMap and never an arbitrary tie-break.
    let mut land_areas: Vec<f64> = (0..graph.len())
        .filter(|&i| !graph.ocean[i])
        .map(|i| graph.area_m2[i])
        .collect();
    land_areas.sort_unstable_by(|a, b| a.total_cmp(b));
    let median_land_area_m2 = if land_areas.is_empty() {
        0.0
    } else {
        let mid = if land_areas.len() % 2 == 0 { land_areas.len() / 2 - 1 } else { land_areas.len() / 2 };
        land_areas[mid]
    };

    let node_based_stream = params.min_stream_nodes * median_land_area_m2;
    let effective_stream_flow_m2 =
        if params.stream_flow_m2 > node_based_stream { params.stream_flow_m2 } else { node_based_stream };
    let node_based_river = 10.0 * effective_stream_flow_m2;
    let effective_river_flow_m2 =
        if params.river_flow_m2 > node_based_river { params.river_flow_m2 } else { node_based_river };
    let node_based_great = 10.0 * effective_river_flow_m2;
    let effective_great_flow_m2 =
        if params.great_flow_m2 > node_based_great { params.great_flow_m2 } else { node_based_great };

    // `extract` reads its thresholds off a `HydroParams`; the smallest clean way to hand it the
    // effective values without a second parameter type is a cloned copy with just those three
    // fields overwritten. Everything else -- including `reach_points`' width/depth anchor and the
    // notch widths below, which both stay on the caller's own `stream_flow_m2` -- keeps using the
    // params `bake` was called with.
    let mut effective_params = params.clone();
    effective_params.stream_flow_m2 = effective_stream_flow_m2;
    effective_params.river_flow_m2 = effective_river_flow_m2;
    effective_params.great_flow_m2 = effective_great_flow_m2;
    let reaches = extract(graph, routing, flow, &effective_params);

    // hollow index -> body id, kept hollows only, in hollow order (Controller ruling: body ids
    // are one per kept hollow, numbered 0.., and a reach's Downstream::Body carries the hollow
    // index that this map remaps to a body id).
    let mut body_id_of_hollow: Vec<Option<u32>> = vec![None; hollows.len()];
    let mut next_body_id = 0u32;
    for (i, hollow) in hollows.iter().enumerate() {
        if hollow.fate == Fate::Keep {
            body_id_of_hollow[i] = Some(next_body_id);
            next_body_id += 1;
        }
    }

    // node -> reach id, for first nodes only (a Vec indexed by node, not a HashMap: no HashMap
    // order may reach output, and node ids are already a dense bounded range).
    let mut reach_of_first_node: Vec<u32> = vec![u32::MAX; graph.len()];
    for (id, reach) in reaches.iter().enumerate() {
        let first = reach.nodes[0] as usize;
        reach_of_first_node[first] = id as u32; // cast-ok: at most one reach per node
    }

    let mut bodies = Vec::new();
    for (i, hollow) in hollows.iter().enumerate() {
        if hollow.fate != Fate::Keep {
            continue;
        }
        let closed = closure.closed[i];
        let kind = if closed && closure.salt_flat[i] {
            BodyKind::SaltFlat
        } else if closed {
            BodyKind::SaltLake
        } else if hollow.area_m2 < params.pond_max_area_m2 {
            BodyKind::Pond
        } else {
            BodyKind::Lake
        };
        // Ruling I1: a closed lake has no outlet. An open lake's outlet reach is the one that
        // starts where its water actually leaves -- the receiver of its lake entry (the rim
        // outlet for a lake above the datum, the first node of the cut for a fresh pocket) --
        // or none if no reach starts there.
        let leaves_to = routing.receiver[hollow.lake_entry as usize];
        let outlet_reach = if closed || leaves_to == NO_NODE || reach_of_first_node[leaves_to as usize] == u32::MAX {
            None
        } else {
            Some(reach_of_first_node[leaves_to as usize])
        };
        let (anchor_lat, anchor_lon) = graph.positions[hollow.floor as usize].to_latlon();
        // A closed lake has nowhere to go; an open lake follows its receiver chain past its own
        // shore (Ruling: Task 5).
        let downstream = if closed {
            Downstream::Sink
        } else {
            body_downstream(
                graph,
                routing,
                i as u32, // cast-ok: hollow index, bounded by hollows.len()
                hollow.lake_entry,
                &reach_of_first_node,
                &body_id_of_hollow,
            )
        };
        bodies.push(Body {
            // body_id_of_hollow[i] was just set to Some(..) above, for every i whose fate is
            // Keep -- this loop only reaches here when hollow.fate == Fate::Keep.
            id: body_id_of_hollow[i].expect("kept hollow has a body id"),
            kind,
            fresh: !closed,
            enclosed: hollow.enclosed,
            forced: hollow.forced,
            level_m: hollow.level_m,
            area_m2: hollow.area_m2,
            depth_m: hollow.depth_m,
            outlet_reach,
            anchor: (anchor_lat, anchor_lon),
            outline: Vec::new(),
            downstream,
        });
    }

    // Every reach's downstream, remapped to a body id where it lands on one, id-indexed so a
    // `Downstream::Reach` link can be followed without borrowing `reach_lines` while it is still
    // being built.
    let reach_downstream: Vec<Downstream> = reaches.iter().map(|reach| match reach.downstream {
        Downstream::Body(hollow_index) => {
            // route() only ever writes lake_of (and so extract's Downstream::Body(hollow))
            // for a hollow whose fate is Keep, so every lake member's hollow has a body id.
            let body_id = body_id_of_hollow[hollow_index as usize]
                .expect("a lake reach's target hollow is always kept");
            Downstream::Body(body_id)
        }
        other => other,
    }).collect();

    let mut reach_lines = Vec::with_capacity(reaches.len());
    for (id, reach) in reaches.iter().enumerate() {
        let points = reach_points(graph, routing, hollows, flow, &reach.nodes, params);
        reach_lines.push(ReachLine {
            id: id as u32, // cast-ok: at most one reach per index
            class: reach.class,
            order: reach.order,
            downstream: reach_downstream[id],
            fresh: downstream_is_fresh(&bodies, &reach_downstream, reach_downstream[id]),
            points,
        });
    }

    // Task 6's notch filter (replaces 12b-2): routing keeps every cut (drainage correctness
    // needs them all), but the record only describes the notches, and the points within them,
    // that matter.
    //
    // (a) An outlet cut from `close_lakes` (`closure.outlet_notch`) is recorded whole -- it is
    //     the one channel a fresh enclosed pocket actually has, however shallow any one step of
    //     it is.
    // (b) Any other notch is reduced to the points that are not a recorded reach's own channel
    //     nodes (its bed already carries that cut) *and* whose cut depth --
    //     `graph.height_m - bed` -- is at least `NOTCH_RECORD_MIN_CUT_M`. A notch left with no
    //     such points is dropped entirely, not recorded empty.
    //
    // Node-id equality stands in for a lat/lon comparison here: both a notch's points and a
    // reach's points come from the same `graph.positions`, keyed by this same node index.
    //
    // A point's width is `width_m` at its own flow for an outlet cut (it is the pocket's only
    // way out, whatever it carries), or at `max(flow, effective stream threshold)` otherwise --
    // never narrower than the narrowest stream reach the record holds. Either way it is sized
    // on the caller's own `params`, exactly as `reach_points` sizes a reach (Ruling F-1): where
    // an outlet cut runs along a reach, the two report the same width.
    //
    // A point's third word is the cut surface -- the lowered ground, which is the water surface
    // through the cut -- not a bed below it (Ruling F-2). A reach point's third word is its bed,
    // surface minus depth, so where the two coincide `notch - reach.depth == reach.bed`. The
    // surface is the final `routing.surface_m`, not the route's stored `bed_m` (Ruling R-9).
    //
    // Ruling F-3: stage 2 carves a notch line segment by segment, so a recorded line must split
    // wherever consecutive kept points are not graph neighbours.
    //
    // Ruling F-3a: `NotchRoute.nodes` lists only the nodes a route actually lowered -- lower
    // ground it merely walked over does not stop it and is nowhere recorded there (`routing.rs`'s
    // `follow_parents`). So when the route's own last (lowered) node survives the filter above,
    // one extra point is appended: `routing.receiver` of that last node, which is one of --
    //   * the water it drains into (the ocean or a lake member);
    //   * a node an earlier cut already committed, standing at or below this cut's bed (Ruling
    //     R-9: one standing above it is re-lowered and walked through);
    //   * lower ground the cut walked over without lowering it.
    // Either way that point's third word is that node's own current water surface -- 0.0 for the
    // ocean, the lake's own `level_m` for a lake member, or `routing.surface_m` otherwise -- not
    // necessarily a cut this route itself made.
    const NOTCH_RECORD_MIN_CUT_M: f64 = 2.0;

    let mut is_river_node = vec![false; graph.len()];
    for reach in &reaches {
        for &node in &reach.nodes {
            is_river_node[node as usize] = true;
        }
    }
    let mut is_outlet_notch = vec![false; routing.notches.len()];
    for &opt in &closure.outlet_notch {
        if let Some(idx) = opt {
            is_outlet_notch[idx] = true;
        }
    }

    let is_adjacent = |a: u32, b: u32| graph.neighbours(a).binary_search(&b).is_ok();
    // The level of the water a cut stops in -- the datum for the ocean, a lake's own level, or
    // the committed bed of an earlier cut.
    let level_at = |node: u32| -> f64 {
        let i = node as usize;
        if graph.ocean[i] {
            0.0
        } else if routing.lake_of[i] != NO_LAKE {
            hollows[routing.lake_of[i] as usize].level_m
        } else {
            routing.surface_m[i]
        }
    };

    let mut notches = Vec::new();
    for (idx, notch) in routing.notches.iter().enumerate() {
        let mut kept: Vec<(u32, f64, f64)> = Vec::with_capacity(notch.nodes.len() + 1);
        for &node in &notch.nodes {
            // Ruling R-9: the final surface, not the route's own `bed_m`. A later cut (or
            // `cut_path`) can re-lower a node after this route graded it, and every route that
            // holds a node must agree on it, as must a reach running through it (F-2).
            let surface_m = routing.surface_m[node as usize];
            if is_outlet_notch[idx] {
                kept.push((node, surface_m, width_m(flow[node as usize], params)));
                continue;
            }
            if is_river_node[node as usize] {
                continue;
            }
            let cut_depth_m = graph.height_m[node as usize] - surface_m;
            if cut_depth_m < NOTCH_RECORD_MIN_CUT_M {
                continue;
            }
            let q = flow[node as usize];
            let q_for_width = if q > effective_stream_flow_m2 { q } else { effective_stream_flow_m2 };
            kept.push((node, surface_m, width_m(q_for_width, params)));
        }
        // The node the cut stopped at: appended when the route's own last node was kept, so the
        // line reaches the water (or the earlier cut) it drains into.
        if let (Some(&last), Some(&(kept_last, _, kept_width))) = (notch.nodes.last(), kept.last()) {
            let stop = routing.receiver[last as usize];
            if kept_last == last && stop != NO_NODE {
                kept.push((stop, level_at(stop), kept_width));
            }
        }
        // Split wherever two consecutive kept nodes are not graph neighbours.
        let mut line: Vec<(f64, f64, f64, f64)> = Vec::new();
        let mut previous: Option<u32> = None;
        for &(node, third, width) in &kept {
            if let Some(prev) = previous {
                if !is_adjacent(prev, node) && !line.is_empty() {
                    notches.push(NotchLine { points: std::mem::take(&mut line) });
                }
            }
            let (lat_deg, lon_deg) = graph.positions[node as usize].to_latlon();
            line.push((lat_deg, lon_deg, third, width));
            previous = Some(node);
        }
        if !line.is_empty() {
            notches.push(NotchLine { points: line });
        }
    }

    // Task 6's forced-outlet accounting: how many forced-outlet points `params` asked for, and
    // of those, how many landed on a submerged member of a kept lake -- the same nearest-node
    // mapping `hollows::forced_nodes` uses (`nearest_forced_nodes`, point by point rather than
    // deduplicated), checked against `routing.lake_of` now that routing has settled. `lake_of`
    // is only ever set for a hollow whose fate is Keep, so this check alone is "submerged member
    // of a *kept* lake" -- no separate fate lookup is needed.
    let nearest_forced = hollows::nearest_forced_nodes(graph, params);
    let forced_requested = nearest_forced.len() as u32; // cast-ok: bounded by WB_MAX_HYDRO_FORCED
    let forced_matched = nearest_forced.iter()
        .filter(|&&node| node.map_or(false, |n| routing.lake_of[n as usize] != NO_LAKE))
        .count() as u32; // cast-ok: bounded by forced_requested

    let falls: Vec<Fall> = Vec::new();

    let land_nodes = graph.ocean.iter().filter(|&&o| !o).count();
    let kept = hollows.iter().filter(|h| h.fate == Fate::Keep).count();
    let notched = hollows.iter().filter(|h| h.fate == Fate::Notch).count();
    let closed_count = closure.closed.iter().filter(|&&c| c).count();
    let streams = reach_lines.iter().filter(|r| r.class == ReachClass::Stream).count();
    let rivers = reach_lines.iter().filter(|r| r.class == ReachClass::River).count();
    let great = reach_lines.iter().filter(|r| r.class == ReachClass::Great).count();
    let max_order = reach_lines.iter().map(|r| r.order).fold(0u32, |top, o| if o > top { o } else { top });

    let ratios = bifurcation_ratios(&reaches);
    let (bifurcation_min, bifurcation_max) = if ratios.is_empty() {
        (0.0, 0.0)
    } else {
        let mut min = ratios[0];
        let mut max = ratios[0];
        for &r in &ratios[1..] {
            if r < min {
                min = r;
            }
            if r > max {
                max = r;
            }
        }
        (min, max)
    };

    let stats = BakeStats {
        nodes: graph.len() as u32, // cast-ok: bounded by stream::MAX_NODES, validated above
        land_nodes: land_nodes as u32, // cast-ok: bounded by node count
        hollows: hollows.len() as u32, // cast-ok: at most one hollow per node
        kept: kept as u32, // cast-ok: bounded by hollow count
        notched: notched as u32, // cast-ok: bounded by hollow count
        closed: closed_count as u32, // cast-ok: bounded by hollow count
        streams: streams as u32, // cast-ok: bounded by reach count
        rivers: rivers as u32, // cast-ok: bounded by reach count
        great: great as u32, // cast-ok: bounded by reach count
        max_order,
        bifurcation_min,
        bifurcation_max,
        stream_flow_m2: effective_stream_flow_m2,
        river_flow_m2: effective_river_flow_m2,
        great_flow_m2: effective_great_flow_m2,
        total_nodes: params.total_nodes,
        wetness_nodes: params.wetness_nodes,
        keep_depth_m: params.keep_depth_m,
        keep_area_m2: params.keep_area_m2,
        pond_max_area_m2: params.pond_max_area_m2,
        keep_max_area_m2: params.keep_max_area_m2,
        min_stream_nodes: params.min_stream_nodes,
        notch_fall_m: params.notch_fall_m,
        evaporation_factor: params.evaporation_factor,
        salt_flat_share: params.salt_flat_share,
        forced_requested,
        forced_matched,
        capped_basins: hollows.iter().filter(|h| h.capped).count() as u32, // cast-ok: bounded by hollow count
        capped_inner: hollows.iter().filter(|h| h.inner_of_capped).count() as u32, // cast-ok: bounded by hollow count
        capped_inner_kept: hollows.iter().filter(|h| h.inner_of_capped && h.fate == Fate::Keep).count() as u32, // cast-ok: bounded by hollow count
        refine_step_m: params.refine_step_m,
        refine_simplify_m: params.refine_simplify_m,
        refine_vertical_m: params.refine_vertical_m,
        fall_min_drop_m: params.fall_min_drop_m,
        fall_max_run_m: params.fall_max_run_m,
        meander_wavelength_widths: params.meander_wavelength_widths,
        meander_amplitude_widths: params.meander_amplitude_widths,
        meander_max_slope: params.meander_max_slope,
        // SCHEMA 5: both are refinement's own counts, and `record_of` is the record *before*
        // refinement. `refine::refine` fills them in.
        crossings_coarse: 0,
        crossings_left: 0,
        // SCHEMA 5, spec §6.6: the fine pond search runs after refinement, so its two counts are
        // `ponds::search`'s to fill in. The seven params it will run with are echoed here, where
        // every other params echo is written.
        ponds_found: 0,
        ponds_kept: 0,
        pond_cell_m: params.pond_cell_m,
        pond_search_radius_m: params.pond_search_radius_m,
        pond_keep_depth_m: params.pond_keep_depth_m,
        pond_keep_area_m2: params.pond_keep_area_m2,
        pond_wetness_share: params.pond_wetness_share,
        pond_max_slope: params.pond_max_slope,
        pond_density_area_m2: params.pond_density_area_m2,
    };

    HydroRecord { bodies, reaches: reach_lines, notches, falls, stats }
}

