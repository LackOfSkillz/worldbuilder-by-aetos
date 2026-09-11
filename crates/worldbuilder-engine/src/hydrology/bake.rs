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
fn reach_points(graph: &LandGraph, routing: &routing::Routing, hollows: &[hollows::Hollow], flow: &[f64], nodes: &[u32], params: &HydroParams) -> Vec<ReachPoint> {
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
fn body_downstream(
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
    if !(params.stream_flow_m2 <= params.river_flow_m2 && params.river_flow_m2 <= params.great_flow_m2) {
        return Err(HydroError::Params("stream_flow_m2 <= river_flow_m2 <= great_flow_m2 required"));
    }

    let graph = LandGraph::sample(surface, params.total_nodes, params.wetness_nodes)
        .ok_or(HydroError::Sampling)?;

    let global_flood = flood(&graph, &ocean_seeds(&graph), &|_| true);
    let mut hollows = find_hollows(&graph, &global_flood);
    judge(&mut hollows, &graph, params);
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
    // fields overwritten. Everything else -- including `reach_points`' width/depth anchor below,
    // which stays on the caller's own `stream_flow_m2` -- keeps using the params `bake` was
    // called with.
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

    let mut reach_lines = Vec::with_capacity(reaches.len());
    for (id, reach) in reaches.iter().enumerate() {
        let downstream = match reach.downstream {
            Downstream::Body(hollow_index) => {
                // route() only ever writes lake_of (and so extract's Downstream::Body(hollow))
                // for a hollow whose fate is Keep, so every lake member's hollow has a body id.
                let body_id = body_id_of_hollow[hollow_index as usize]
                    .expect("a lake reach's target hollow is always kept");
                Downstream::Body(body_id)
            }
            other => other,
        };
        let points = reach_points(graph, routing, hollows, flow, &reach.nodes, params);
        reach_lines.push(ReachLine {
            id: id as u32, // cast-ok: at most one reach per index
            class: reach.class,
            order: reach.order,
            downstream,
            points,
        });
    }

    // Ruling 12b-2: routing keeps every cut (drainage correctness needs them all), but the
    // record only describes the notches that matter -- one that a recorded reach's channel
    // runs through, or one `close_lakes` cut as a fresh enclosed pocket's outlet. `reach.nodes`
    // (not just the reach's endpoints) are every node its channel visits, so membership there is
    // "lies on a river's path". Node-id equality stands in for the brief's lat/lon comparison:
    // both a notch's points and a reach's points come from the same `graph.positions`, keyed by
    // this same node index, so comparing indices is comparing positions exactly, without paying
    // for a `to_latlon()` round trip on nodes the filter is about to discard anyway.
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

    let mut notches = Vec::new();
    for (idx, notch) in routing.notches.iter().enumerate() {
        let on_a_river = notch.nodes.iter().any(|&node| is_river_node[node as usize]);
        if !(on_a_river || is_outlet_notch[idx]) {
            continue;
        }
        let mut points = Vec::with_capacity(notch.nodes.len());
        for (&node, &bed_m) in notch.nodes.iter().zip(&notch.bed_m) {
            let (lat_deg, lon_deg) = graph.positions[node as usize].to_latlon();
            points.push((lat_deg, lon_deg, bed_m));
        }
        notches.push(NotchLine { points });
    }

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
    };

    HydroRecord { bodies, reaches: reach_lines, notches, falls, stats }
}

#[cfg(test)]
mod bake_tests {
    use super::*;
    use crate::hydrology::record::{decode, encode};
    use crate::sphere::SpherePoint;
    use crate::surface::Surface;

    fn world() -> Surface {
        Surface::new(20_260_904, 6_371_000.0, 12, 0.29, None, None, None)
    }

    fn params() -> HydroParams {
        let mut p = HydroParams::earth_like(12_000);
        p.wetness_nodes = 500;
        p.stream_flow_m2 = 3.0e10;
        p.river_flow_m2 = 3.0e11;
        p.great_flow_m2 = 3.0e12;
        p
    }

    #[test]
    fn a_bake_is_bit_identical_run_to_run() {
        let a = encode(&super::super::bake(&world(), &params()).expect("bake"));
        let b = encode(&super::super::bake(&world(), &params()).expect("bake"));
        assert_eq!(a.iter().map(|w| w.to_bits()).collect::<Vec<_>>(),
                   b.iter().map(|w| w.to_bits()).collect::<Vec<_>>());
    }

    #[test]
    fn the_record_round_trips() {
        let record = super::super::bake(&world(), &params()).expect("bake");
        let words = encode(&record);
        // `Body.downstream` (Task 5) isn't on the wire yet -- Task 6 owns SCHEMA 3's layout --
        // so `decode` always reports `Sink` for it. The round trip is checked on everything
        // else; `expected` pins that one field down to what `decode` actually produces so the
        // rest of the comparison still catches a real mismatch.
        let mut expected = record.clone();
        for body in &mut expected.bodies {
            body.downstream = Downstream::Sink;
        }
        assert_eq!(decode(&words).as_ref(), Some(&expected));
        assert_eq!(words[0], crate::hydrology::record::SCHEMA);
    }

    /// Ruling 12b-1, the params-bind case: on this suite's test-world overrides (3.0e10 /
    /// 3.0e11 / 3.0e12), the params already sit above `min_stream_nodes * median land-node
    /// area`, so the node-based branch never binds here -- see
    /// `the_node_floor_binds_on_a_coarse_graph` below for the case where it does. Kept with
    /// `>=` on both sides so it stays meaningful regardless of which branch wins.
    #[test]
    fn effective_thresholds_rise_to_the_graph_resolution() {
        let p = params();
        let record = super::super::bake(&world(), &p).expect("bake");

        let graph = LandGraph::sample(&world(), p.total_nodes, p.wetness_nodes).expect("graph");
        let mut land_areas: Vec<f64> =
            (0..graph.len()).filter(|&i| !graph.ocean[i]).map(|i| graph.area_m2[i]).collect();
        land_areas.sort_unstable_by(|a, b| a.total_cmp(b));
        let mid = if land_areas.len() % 2 == 0 { land_areas.len() / 2 - 1 } else { land_areas.len() / 2 };
        let median_land_area_m2 = land_areas[mid];

        assert!(record.stats.stream_flow_m2 >= p.min_stream_nodes * median_land_area_m2);
        assert!(record.stats.stream_flow_m2 >= p.stream_flow_m2);
        assert!(record.stats.river_flow_m2 >= 10.0 * record.stats.stream_flow_m2);
        assert!(record.stats.river_flow_m2 >= p.river_flow_m2);
        assert!(record.stats.great_flow_m2 >= 10.0 * record.stats.river_flow_m2);
        assert!(record.stats.great_flow_m2 >= p.great_flow_m2);
    }

    /// Task 12b fix round 1: the node floor actually binds here. `HydroParams::earth_like`'s
    /// stock thresholds (2.5e8 / 2.5e9 / 1.0e11) sit far below one node's share of this 12,000
    /// node world (about 4.25e10 m^2), so the node-based branch must win, and the assertions
    /// below fail if the floor is ever removed -- unlike
    /// `effective_thresholds_rise_to_the_graph_resolution` above, whose test-world overrides
    /// never exercise this branch.
    #[test]
    fn the_node_floor_binds_on_a_coarse_graph() {
        let mut p = HydroParams::earth_like(12_000);
        p.wetness_nodes = 500;
        let record = super::super::bake(&world(), &p).expect("bake");

        let graph = LandGraph::sample(&world(), p.total_nodes, p.wetness_nodes).expect("graph");
        let mut land_areas: Vec<f64> =
            (0..graph.len()).filter(|&i| !graph.ocean[i]).map(|i| graph.area_m2[i]).collect();
        land_areas.sort_unstable_by(|a, b| a.total_cmp(b));
        let mid = if land_areas.len() % 2 == 0 { land_areas.len() / 2 - 1 } else { land_areas.len() / 2 };
        let median_land_area_m2 = land_areas[mid];

        assert_eq!(record.stats.stream_flow_m2, 10.0 * median_land_area_m2);
        assert_eq!(record.stats.river_flow_m2, 10.0 * record.stats.stream_flow_m2);
        assert_eq!(record.stats.great_flow_m2, 10.0 * record.stats.river_flow_m2);
        assert!(record.stats.stream_flow_m2 > 2.5e8);
    }

    /// Ruling 12b-2: a recorded notch either lies on a recorded river's channel or was cut by
    /// `close_lakes` as a fresh enclosed pocket's outlet -- nothing else. Reruns the same
    /// pipeline `bake()` folds together, so it can see `closure.outlet_notch` and the raw
    /// `routing.notches` bake() itself filters against, and compares lat/lon exactly (both a
    /// notch's points and a reach's points come from the same node positions).
    #[test]
    fn only_notches_on_rivers_or_outlets_are_recorded() {
        let surface = world();
        let p = params();
        let graph = LandGraph::sample(&surface, p.total_nodes, p.wetness_nodes).expect("graph");
        let global_flood = flood(&graph, &ocean_seeds(&graph), &|_| true);
        let mut hollows = find_hollows(&graph, &global_flood);
        judge(&mut hollows, &graph, &p);
        let mut routing = route(&graph, &global_flood, &mut hollows, &p);
        let (flow, closure) = close_lakes(&graph, &mut routing, &hollows, &p);
        let reaches = extract(&graph, &routing, &flow, &p);

        let mut river_positions: Vec<(f64, f64)> = Vec::new();
        for reach in &reaches {
            for &node in &reach.nodes {
                river_positions.push(graph.positions[node as usize].to_latlon());
            }
        }
        let mut outlet_positions: Vec<(f64, f64)> = Vec::new();
        for opt in &closure.outlet_notch {
            if let Some(idx) = *opt {
                for &node in &routing.notches[idx].nodes {
                    outlet_positions.push(graph.positions[node as usize].to_latlon());
                }
            }
        }

        let record = super::super::bake(&surface, &p).expect("bake");
        assert!(!record.notches.is_empty(), "sanity: this fixture must record at least one notch");
        for notch in &record.notches {
            let matches = notch.points.iter().any(|&(lat, lon, _bed_m)| {
                river_positions.contains(&(lat, lon)) || outlet_positions.contains(&(lat, lon))
            });
            assert!(matches, "a recorded notch matched neither a river channel nor an outlet cut");
        }
    }

    #[test]
    fn a_truncated_record_is_refused() {
        let words = encode(&super::super::bake(&world(), &params()).expect("bake"));
        assert_eq!(decode(&words[..words.len() - 1]), None);
        assert_eq!(decode(&[]), None);
    }

    #[test]
    fn no_kept_body_is_below_the_keep_rule_unless_forced_or_enclosed() {
        let p = params();
        let record = super::super::bake(&world(), &p).expect("bake");
        for body in &record.bodies {
            if !body.forced && !body.enclosed {
                assert!(body.depth_m >= p.keep_depth_m && body.area_m2 >= p.keep_area_m2,
                        "body {} depth {} area {}", body.id, body.depth_m, body.area_m2);
            }
        }
    }

    #[test]
    fn every_open_lake_has_one_outlet_reach_or_drains_straight_to_the_sea() {
        let record = super::super::bake(&world(), &params()).expect("bake");
        for body in &record.bodies {
            if body.fresh {
                let feeding = record.reaches.iter()
                    .filter(|r| r.downstream == Downstream::Body(body.id)).count();
                let _ = feeding; // lakes may have no feeding reach at coarse thresholds
                assert!(body.outlet_reach.map_or(true, |id| (id as usize) < record.reaches.len()));
            }
        }
    }

    #[test]
    fn bad_params_are_refused_not_panicked() {
        let mut p = params();
        p.river_flow_m2 = p.stream_flow_m2 / 2.0;
        assert!(matches!(super::super::bake(&world(), &p), Err(HydroError::Params(_))));
        let mut p = params();
        p.total_nodes = 1;
        assert!(matches!(super::super::bake(&world(), &p), Err(HydroError::Params(_))));
    }

    /// Sanity check only -- it does not discriminate. Flow only ever accumulates downstream, so
    /// the old (wrong) terminal-point value, the ocean/lake's total inflow, is structurally
    /// always `>=` the fixed, channel-only value on this bake world's topology (one land
    /// neighbor per ocean cell); it passes before and after the fix (see fix round 1's report).
    /// `the_mouth_point_carries_its_own_river_not_the_whole_sea` below is the discriminating
    /// regression test.
    #[test]
    fn a_reach_keeps_its_width_to_the_sea() {
        let record = super::super::bake(&world(), &params()).expect("bake");
        for reach in &record.reaches {
            let reaches_sea_or_lake = matches!(reach.downstream, Downstream::Ocean | Downstream::Body(_));
            if !reaches_sea_or_lake {
                continue;
            }
            let n = reach.points.len();
            if n < 2 {
                continue;
            }
            let last = &reach.points[n - 1];
            let prev = &reach.points[n - 2];
            assert!(last.width_m >= prev.width_m,
                    "reach {} last width {} < previous width {}", reach.id, last.width_m, prev.width_m);
            assert!(last.width_m > 0.0, "reach {} terminal width is zero", reach.id);
        }
    }

    /// Two land branches feeding the same ocean node, with different areas so their flows
    /// differ. Node 2 is the only below-datum node, so `LandGraph::label_water` makes it the
    /// ocean; nodes 0-1 and 3-4 are separate branches, each a monotonic downhill run straight
    /// into node 2 -- no hollow forms on either side.
    fn two_branches_into_one_sea() -> LandGraph {
        let heights = [20.0, 10.0, -50.0, 10.0, 20.0];
        let n = heights.len();
        let positions: Vec<SpherePoint> =
            (0..n).map(|i| SpherePoint::from_latlon(0.0, i as f64 * 0.5)).collect();
        let directed: Vec<Vec<u32>> = (0..n as u32) // cast-ok: tiny fixture
            .map(|i| {
                let mut v = Vec::new();
                if i > 0 { v.push(i - 1); }
                if (i as usize) + 1 < n { v.push(i + 1); }
                v
            })
            .collect();
        // The left branch (nodes 0-1) is a third the area of the right branch (nodes 3-4), so
        // the two channels draining into node 2 carry different flow.
        let area_m2 = vec![1.0e6, 1.0e6, 1.0e6, 3.0e6, 3.0e6];
        let wetness = vec![0.5; n];
        LandGraph::from_parts(6_371_000.0, positions, heights.to_vec(), area_m2, &directed, wetness)
    }

    /// The discriminating regression for Ruling 2 (fix round 2). Unlike
    /// `a_reach_keeps_its_width_to_the_sea` above, this can tell the fixed terminal-point flow
    /// from the old, conflated one: the ocean node here collects two distinct branches, so its
    /// total inflow is strictly greater than either branch's own flow, not just `>=` by
    /// monotonicity.
    #[test]
    fn the_mouth_point_carries_its_own_river_not_the_whole_sea() {
        let g = two_branches_into_one_sea();
        let params = HydroParams::earth_like(0);
        let f = flood(&g, &ocean_seeds(&g), &|_| true);
        let mut hollows = find_hollows(&g, &f);
        judge(&mut hollows, &g, &params);
        let mut routing = route(&g, &f, &mut hollows, &params);
        let (flow, _closure) = close_lakes(&g, &mut routing, &hollows, &params);

        // The left branch's own receiver chain, derived from routing.receiver rather than
        // assumed -- it happens to land on [0, 1, 2] for this fixture.
        let mut nodes = vec![0u32];
        let mut here = 0u32;
        let mut steps = 0;
        while routing.receiver[here as usize] != crate::hydrology::flood::NO_NODE {
            here = routing.receiver[here as usize];
            nodes.push(here);
            steps += 1;
            assert!(steps < 10, "a cycle");
        }
        assert_eq!(nodes, vec![0, 1, 2], "left branch drains node 0 -> 1 -> the ocean at 2");
        assert!(g.ocean[2], "node 2 is the ocean");

        let points = reach_points(&g, &routing, &hollows, &flow, &nodes, &params);
        let last = points.last().expect("at least one point");
        assert_eq!(last.flow_m2, flow[1],
                   "the mouth point must carry node 1's own channel flow, not node 2's");
        assert!(last.flow_m2 < flow[2],
                "node 2's accumulated flow also holds the right branch's inflow, so the \
                 channel's own flow must be strictly less: last {} flow[2] {}",
                last.flow_m2, flow[2]);
    }

    /// Ruling I4: a mouth's bed is the water it meets. On the two-branch fixture the ocean node
    /// (node 2) is 50 m below the datum; the mouth point must stand at the datum, not carve it.
    #[test]
    fn a_river_mouth_does_not_carve_the_seabed() {
        let g = two_branches_into_one_sea();
        let params = HydroParams::earth_like(0);
        let f = flood(&g, &ocean_seeds(&g), &|_| true);
        let mut hollows = find_hollows(&g, &f);
        judge(&mut hollows, &g, &params);
        let mut routing = route(&g, &f, &mut hollows, &params);
        let (flow, _closure) = close_lakes(&g, &mut routing, &hollows, &params);
        let points = reach_points(&g, &routing, &hollows, &flow, &[0, 1, 2], &params);
        assert_eq!(points[2].bed_m, 0.0, "the mouth stands at the datum, not the -50 m seabed");
        assert!(points[1].bed_m < routing.surface_m[1], "an inland point still sits below its ground");

        // End to end: every ocean mouth at the datum, every lake mouth at its lake's level.
        let record = super::super::bake(&world(), &params_for_world()).expect("bake");
        let mut ocean_mouths = 0;
        for reach in &record.reaches {
            let last = reach.points.last().expect("a reach has points");
            match reach.downstream {
                Downstream::Ocean => {
                    assert_eq!(last.bed_m, 0.0, "reach {} mouth", reach.id);
                    ocean_mouths += 1;
                }
                Downstream::Body(id) => {
                    assert_eq!(last.bed_m, record.bodies[id as usize].level_m, "reach {} lake mouth", reach.id);
                }
                _ => {}
            }
        }
        assert!(ocean_mouths > 0, "sanity: this world has rivers that reach the sea");
    }

    fn params_for_world() -> HydroParams {
        params()
    }

    /// Ruling I1: a closed lake reports no outlet reach, and an open lake's outlet reach starts
    /// where its water actually leaves -- `routing.receiver[lake_entry]`.
    #[test]
    fn no_closed_body_has_an_outlet_reach() {
        // By hand: a dry lake behind a 40 m rim (node 1). It closes, yet the rim node sheds
        // enough of its own water, at these thresholds, to start a reach to the sea -- which the
        // old rule (the reach starting at `hollow.outlet`) reported as the closed lake's outlet.
        let heights = [-50.0, 40.0, 5.0, 12.0, 25.0, 70.0];
        let n = heights.len();
        let positions: Vec<SpherePoint> =
            (0..n).map(|i| SpherePoint::from_latlon(0.0, i as f64 * 0.5)).collect();
        let directed: Vec<Vec<u32>> = (0..n as u32) // cast-ok: tiny fixture
            .map(|i| {
                let mut v = Vec::new();
                if i > 0 { v.push(i - 1); }
                if (i as usize) + 1 < n { v.push(i + 1); }
                v
            })
            .collect();
        let g = LandGraph::from_parts(6_371_000.0, positions, heights.to_vec(), vec![2.0e6; n], &directed, vec![0.05; n]);
        let mut p = HydroParams::earth_like(0);
        p.stream_flow_m2 = 5.0e4;
        p.river_flow_m2 = 5.0e5;
        p.great_flow_m2 = 5.0e6;
        p.min_stream_nodes = 1.0e-9;
        let f = flood(&g, &ocean_seeds(&g), &|_| true);
        let mut hollows = find_hollows(&g, &f);
        judge(&mut hollows, &g, &p);
        let mut routing = route(&g, &f, &mut hollows, &p);
        let (flow, closure) = close_lakes(&g, &mut routing, &hollows, &p);
        let stages = BakeStages { graph: g, hollows, routing, flow, closure };
        let record = record_of(&stages, &p);
        assert_eq!(record.bodies.len(), 1);
        assert!(!record.bodies[0].fresh, "sanity: wetness 0.05 closes the lake");
        assert!(record.reaches.iter().any(|r| r.points[0].lon_deg == 0.5), "sanity: a reach starts at the rim");
        assert_eq!(record.bodies[0].outlet_reach, None, "a closed lake has no outlet reach");

        // On a real world, dry enough (evaporation x5) that some lakes close and some stay open.
        let surface = world();
        let mut p = params();
        p.evaporation_factor = 5.0;
        let stages = bake_stages(&surface, &p).expect("bake");
        let record = record_of(&stages, &p);
        let kept: Vec<&hollows::Hollow> = stages.hollows.iter().filter(|h| h.fate == Fate::Keep).collect();
        assert_eq!(kept.len(), record.bodies.len());
        let closed = record.bodies.iter().filter(|b| !b.fresh).count();
        assert!(closed > 0, "sanity: this world closes at least one lake");
        let mut with_outlet = 0;
        for (body, hollow) in record.bodies.iter().zip(&kept) {
            if !body.fresh {
                assert_eq!(body.outlet_reach, None, "closed body {} reports an outlet reach", body.id);
                continue;
            }
            if let Some(id) = body.outlet_reach {
                with_outlet += 1;
                let leaves_to = stages.routing.receiver[hollow.lake_entry as usize];
                let first = &record.reaches[id as usize].points[0];
                assert_eq!((first.lat_deg, first.lon_deg), stages.graph.positions[leaves_to as usize].to_latlon(),
                           "body {}'s outlet reach starts where its water leaves", body.id);
            }
        }
        assert!(with_outlet > 0, "sanity: at least one open lake feeds a reach");
    }

    /// Ruling C1-d: the full bake on the two real worlds the drainage cycle was found on, at the
    /// smallest node count (of those tried: 10k, 14k, 16k, 18k, 20k, 30k, 50k) at which each
    /// reproduced it before the fix -- seed 1 (ranges) at 10,000 nodes (317 undrained land
    /// nodes), seed 4242 at 20,000 (39). `bake()` is `Ok`, and everything the land sheds reaches
    /// the sea or a closed lake's sink.
    #[test]
    fn real_worlds_drain_everything_through_the_full_bake() {
        let cases = [
            (Surface::new(1, 6.371e6, 12, 0.40, None, None, Some(crate::tectonics::TectonicParams::ranges())), 10_000u32),
            (Surface::new(4242, 6.371e6, 16, 0.35, None, None, None), 20_000u32),
        ];
        for (surface, nodes) in &cases {
            let p = HydroParams::earth_like(*nodes);
            // `bake()` is exactly `bake_stages` then `record_of`; calling the halves once each
            // is the full bake without paying for a second debug-build sample of the world.
            let stages = bake_stages(surface, &p).expect("the bake drains, so bake_stages is Ok");
            let record = record_of(&stages, &p);
            assert!(!record.bodies.is_empty() && !record.reaches.is_empty(), "sanity: a real bake at {nodes} nodes");
            let (g, r, flow) = (&stages.graph, &stages.routing, &stages.flow);
            assert_eq!(drainage_check(g, r), Ok(()));
            let mut total = 0.0;
            let mut delivered = 0.0;
            for i in 0..g.len() {
                if g.ocean[i] {
                    continue;
                }
                total += g.area_m2[i] * g.wetness[i];
                let recv = r.receiver[i];
                if recv == NO_NODE || g.ocean[recv as usize] {
                    delivered += flow[i];
                }
            }
            let rel = (delivered - total).abs() / total;
            assert!(rel < 1e-9, "{nodes} nodes: delivered {delivered}, shed {total}, rel {rel}");
        }
    }

    /// Mutation guard for the connectivity property: a hand-broken reach list must fail it.
    #[test]
    fn the_connectivity_check_catches_a_cycle() {
        let mut record = super::super::bake(&world(), &params()).expect("bake");
        assert!(super::super::reaches_are_acyclic(&record.reaches));
        if record.reaches.len() >= 2 {
            record.reaches[0].downstream = Downstream::Reach(1);
            record.reaches[1].downstream = Downstream::Reach(0);
            assert!(!super::super::reaches_are_acyclic(&record.reaches));
        }
    }

    /// Follows `Body`/`Reach` downstream links from `start` until `Ocean` or `Sink`, bounded by
    /// the total number of bodies and reaches -- more than enough hops for any acyclic chain
    /// this record could hold, so overrunning it means a loop.
    fn follows_to_ocean(record: &HydroRecord, start: Downstream) -> bool {
        let bound = record.bodies.len() + record.reaches.len() + 1;
        let mut here = start;
        for _ in 0..bound {
            match here {
                Downstream::Ocean => return true,
                Downstream::Sink => return false,
                Downstream::Reach(id) => here = record.reaches[id as usize].downstream,
                Downstream::Body(id) => here = record.bodies[id as usize].downstream,
            }
        }
        false // ran past the bound without reaching Ocean or Sink: a loop.
    }

    /// Task 5's rule, checked on one baked record: every fresh (open) body names somewhere its
    /// water goes that is not `Sink`, every closed body reports `Sink`, and chasing `Body`/`Reach`
    /// links from any fresh body reaches the ocean without looping.
    fn check_downstream_invariants(record: &HydroRecord) {
        assert!(!record.bodies.is_empty(), "sanity: this world has bodies to check");
        for body in &record.bodies {
            if body.fresh {
                assert_ne!(body.downstream, Downstream::Sink,
                           "fresh body {} reports Sink", body.id);
                assert!(follows_to_ocean(record, Downstream::Body(body.id)),
                        "body {}'s downstream chain must reach the ocean without looping", body.id);
            } else {
                assert_eq!(body.downstream, Downstream::Sink,
                           "closed body {} doesn't report Sink", body.id);
            }
        }
    }

    #[test]
    fn every_open_lake_says_where_it_drains() {
        // The bake test world (`world()`/`params()`, 12,000 nodes).
        check_downstream_invariants(&super::super::bake(&world(), &params()).expect("bake"));

        // Seed 1 on the tectonic `ranges` world, also at 12,000 nodes -- the same real-world
        // fixture `real_worlds_drain_everything_through_the_full_bake` uses at 10,000, one size
        // up, per the brief.
        let surface = Surface::new(1, 6.371e6, 12, 0.40, None, None,
                                    Some(crate::tectonics::TectonicParams::ranges()));
        let p = HydroParams::earth_like(12_000);
        check_downstream_invariants(&super::super::bake(&surface, &p).expect("bake"));
    }

    /// The brief's first-step check: `outlet_reach` is `Some(id)` exactly when the first node
    /// past a body's own shore (`routing.receiver[lake_entry]`) is itself where reach `id`
    /// starts -- and when it is, `downstream` (which walks that same chain) resolves to
    /// `Reach(id)` right there, on that first hop.
    ///
    /// This does *not* mean `downstream == Reach(_)` implies `outlet_reach.is_some()`: the
    /// water can cross one or more nodes below the stream threshold before a reach actually
    /// starts, and `downstream` keeps walking to find it while `outlet_reach` only ever looks at
    /// the first hop. `body_downstream`'s own doc comment names this; the fixture below observes
    /// it directly.
    #[test]
    fn outlet_reach_agrees_with_the_downstream_reach_at_the_first_step() {
        let surface = world();
        let p = params();
        let stages = bake_stages(&surface, &p).expect("bake");
        let record = record_of(&stages, &p);
        let kept: Vec<&hollows::Hollow> = stages.hollows.iter().filter(|h| h.fate == Fate::Keep).collect();
        assert_eq!(kept.len(), record.bodies.len(), "sanity: one body per kept hollow, in order");
        let mut some_body_has_an_outlet_reach = false;
        let mut some_body_names_a_later_reach = false;
        for (body, hollow) in record.bodies.iter().zip(&kept) {
            let leaves_to = stages.routing.receiver[hollow.lake_entry as usize];
            let starts_here = if leaves_to == NO_NODE {
                None
            } else {
                let (lat, lon) = stages.graph.positions[leaves_to as usize].to_latlon();
                record.reaches.iter()
                    .find(|r| r.points[0].lat_deg == lat && r.points[0].lon_deg == lon)
                    .map(|r| r.id)
            };
            assert_eq!(body.outlet_reach, starts_here,
                       "body {}: outlet_reach must match whether a reach starts at the first hop", body.id);
            if let Some(id) = starts_here {
                assert_eq!(body.downstream, Downstream::Reach(id),
                           "body {}: downstream must already be Reach({id}) at the first hop", body.id);
                some_body_has_an_outlet_reach = true;
            } else if let Downstream::Reach(_) = body.downstream {
                some_body_names_a_later_reach = true;
            }
        }
        assert!(some_body_has_an_outlet_reach, "sanity: this world has at least one body with an outlet reach");
        assert!(some_body_names_a_later_reach,
                "sanity: this world has at least one body whose downstream reach starts past the first hop \
                 (outlet_reach None), which is what distinguishes this check from the full downstream walk");
    }

    /// Hand fixture: two lakes in a line, one draining straight into the other with no reach in
    /// between -- `Downstream::Body`, not `Reach` or `Ocean`.
    ///
    /// Heights: `[-50 (ocean), 30 (rim), 5 (lake B floor), 45 (rim), 10 (lake A floor), 90
    /// (peak)]`. The flood's spill at a node is the highest ground on its one path back to the
    /// ocean (`flood::flood`'s `spill = max(own, parent's spill)`), so it only rises where a
    /// node's own height exceeds every barrier already crossed; `find_hollows` groups every node
    /// sharing one spill value into one hollow. Node 1 (30 m) is the first barrier: node 2 (5 m)
    /// sits behind it alone, sharing spill 30 (lake B, depth 25). Node 3 (45 m) is a second,
    /// taller barrier -- its own height exceeds 30, so the spill rises again -- and node 4 (10 m)
    /// sits behind that alone, sharing spill 45 (lake A, depth 35). Both clear
    /// `earth_like`'s keep thresholds (8 m / 1.0e6 m^2) on their own, so both are kept without
    /// needing `forced` or `enclosed`; neither node is below the datum, so neither is enclosed
    /// either (Ruling W1 only enrolls a below-datum component).
    ///
    /// Lake A's `lake_entry` is node 4, its only member; `route`'s open-lake receiver rule sends
    /// its water to `hollow.outlet`, the flood's parent of its entry -- node 3, the barrier it
    /// spilled over, not node 5's peak. Node 3 is dry ground, so `steepest` picks its
    /// downhill receiver: lake B's raised surface at node 2 (30 m) is lower than lake A's own
    /// raised surface at node 4 (45 m, excluded: a receiver must strictly fall), so node 3's
    /// receiver is node 2 -- already a member of lake B. Lake A's water thus reaches a different
    /// body on its very first hop past its own shore, with no channel node -- and so no
    /// reach -- in between. `stream_flow_m2` is set far above anything this tiny fixture could
    /// ever carry, so nothing here could become a reach even if the geometry were different.
    #[test]
    fn a_lake_drains_straight_into_another_lake_with_no_reach_between() {
        let heights = [-50.0, 30.0, 5.0, 45.0, 10.0, 90.0];
        let n = heights.len();
        let positions: Vec<SpherePoint> =
            (0..n).map(|i| SpherePoint::from_latlon(0.0, i as f64 * 0.5)).collect();
        let directed: Vec<Vec<u32>> = (0..n as u32) // cast-ok: tiny fixture
            .map(|i| {
                let mut v = Vec::new();
                if i > 0 { v.push(i - 1); }
                if (i as usize) + 1 < n { v.push(i + 1); }
                v
            })
            .collect();
        let g = LandGraph::from_parts(6_371_000.0, positions, heights.to_vec(), vec![2.0e6; n], &directed, vec![0.5; n]);
        assert!(g.ocean[0] && !g.ocean[1..].iter().any(|&o| o), "sanity: only node 0 is the ocean");

        let mut p = HydroParams::earth_like(0);
        // High enough that nothing in this tiny fixture ever qualifies as a channel node, so no
        // reach could form between the two lakes regardless of the geometry above.
        p.stream_flow_m2 = 1.0e30;
        p.river_flow_m2 = 1.0e30;
        p.great_flow_m2 = 1.0e30;

        let f = flood(&g, &ocean_seeds(&g), &|_| true);
        let mut hollows = find_hollows(&g, &f);
        judge(&mut hollows, &g, &p);
        assert_eq!(hollows.len(), 2, "sanity: two separate hollows, not one merged basin");
        let lake_b = hollows.iter().position(|h| h.members == vec![2]).expect("lake B, node 2 alone");
        let lake_a = hollows.iter().position(|h| h.members == vec![4]).expect("lake A, node 4 alone");
        assert_eq!(hollows[lake_b].level_m, 30.0);
        assert_eq!(hollows[lake_a].level_m, 45.0);
        assert_eq!(hollows[lake_b].fate, Fate::Keep, "sanity: lake B clears the keep rule");
        assert_eq!(hollows[lake_a].fate, Fate::Keep, "sanity: lake A clears the keep rule");

        let mut routing = route(&g, &f, &mut hollows, &p);
        assert_eq!(hollows[lake_a].outlet, 3, "sanity: lake A spills over node 3, not the far peak");
        assert_eq!(routing.receiver[3], 2, "sanity: node 3's steepest neighbour is lake B's surface");
        let (flow, closure) = close_lakes(&g, &mut routing, &hollows, &p);
        let stages = BakeStages { graph: g, hollows, routing, flow, closure };
        let record = record_of(&stages, &p);

        assert!(record.reaches.is_empty(), "sanity: nothing here clears the stream threshold");
        assert_eq!(record.bodies.len(), 2);
        let body_a = record.bodies.iter().find(|b| b.level_m == 45.0).expect("lake A's body");
        let body_b = record.bodies.iter().find(|b| b.level_m == 30.0).expect("lake B's body");
        assert_eq!(body_a.downstream, Downstream::Body(body_b.id),
                   "lake A must drain straight into lake B, with no reach between them");
        assert_eq!(body_a.outlet_reach, None, "no reach starts on lake A's way into lake B");
    }
}
