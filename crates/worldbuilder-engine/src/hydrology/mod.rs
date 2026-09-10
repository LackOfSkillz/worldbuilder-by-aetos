//! Automatic water: the bake that finds where water collects and where it runs.
//!
//! See `docs/superpowers/specs/2026-09-10-automatic-water-design.md`. This module reads the
//! landform (`Surface::structural_m`), never the detail noise, and it changes nothing in the
//! default elevation path: a bake is requested, not implied.

pub mod heap;
pub mod buckets;
pub mod landgraph;
pub mod flood;
pub mod hollows;
pub mod routing;
pub mod flow;
pub mod reaches;
pub mod record;

use crate::sphere::SpherePoint;
use crate::surface::Surface;

use crate::hydrology::flood::{flood, ocean_seeds};
use crate::hydrology::flow::close_lakes;
use crate::hydrology::hollows::{find_hollows, judge, Fate};
use crate::hydrology::landgraph::LandGraph;
use crate::hydrology::reaches::{bifurcation_ratios, depth_m, extract, width_m};
use crate::hydrology::routing::{route, NO_LAKE};

// Ruling A: `ReachLine` and the bake tests need to name these without a second import path.
pub use reaches::{Downstream, ReachClass};

/// Everything a bake is told. Recorded in the world beside the record, so a re-bake with the
/// same params and the same land is the same water.
#[derive(Debug, Clone, PartialEq)]
pub struct HydroParams {
    pub total_nodes: u32,
    pub wetness_nodes: u32,
    pub keep_depth_m: f64,
    pub keep_area_m2: f64,
    pub pond_max_area_m2: f64,
    pub stream_flow_m2: f64,
    pub river_flow_m2: f64,
    pub great_flow_m2: f64,
    pub notch_fall_m: f64,
    pub evaporation_factor: f64,
    pub salt_flat_share: f64,
    pub forced_outlets: Vec<SpherePoint>,
    /// Ruling 12b-1: the floor for the stream threshold, in graph nodes rather than m^2 -- the
    /// effective stream threshold is `max(stream_flow_m2, min_stream_nodes * median land-node
    /// area)`, so a coarse graph's threshold rises to what it can actually resolve. Not a wasm
    /// param in 1a: `hydro_params_from` never sets this field, so a wasm bake always takes
    /// `earth_like`'s value.
    pub min_stream_nodes: f64,
    /// Ruling 12b-5: an open hollow (neither enclosed nor forced) larger than this is notched
    /// however deep it is -- a broad landform basin filled to its rim is a drained lowland at
    /// graph scale, not an inland sea several Caspians wide. Not a wasm param in 1a, for the
    /// same reason as `min_stream_nodes`.
    pub keep_max_area_m2: f64,
}

impl HydroParams {
    /// The spec's Earth-like starting values (section 6), tuned against the owner's world by
    /// Task 12b of plan 1a (see `.superpowers/sdd/2026-09-10-water-1a-coarse-bake/
    /// task-12b-report.md` for the measurements behind `min_stream_nodes` and
    /// `keep_max_area_m2`).
    pub fn earth_like(total_nodes: u32) -> Self {
        Self {
            total_nodes,
            wetness_nodes: 20_000,
            keep_depth_m: 8.0,
            keep_area_m2: 1.0e6,
            pond_max_area_m2: 1.0e6,
            stream_flow_m2: 2.5e8,
            river_flow_m2: 2.5e9,
            great_flow_m2: 1.0e11,
            notch_fall_m: 1.0,
            evaporation_factor: 1.0,
            salt_flat_share: 0.1,
            forced_outlets: Vec::new(),
            min_stream_nodes: 10.0,
            keep_max_area_m2: 4.0e11,
        }
    }
}

/// Ruling 12b-3: the node budget for a coarse bake, measured against the owner's world (studio
/// heap 372 MB at this count, under the 512 MB ceiling; an 80 s wasm bake). Bifurcation ratios
/// are reported by `BakeStats`, not forced toward the Earth-like 3-5 target -- the gap between
/// what this budget's graph resolves and that target is documented, not closed here.
pub const DEFAULT_TOTAL_NODES: u32 = 1_000_000;

/// What kind of standing water a body is. Salt vs fresh is `Body::fresh`; this is the shape
/// and the surface, not the chemistry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    Lake,
    Pond,
    SaltLake,
    SaltFlat,
}

/// One kept hollow: a lake, pond, salt lake or salt flat. `id` is the hollow's position among
/// kept hollows, in hollow order -- not the hollow's own index, which also counts notched ones.
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    pub id: u32,
    pub kind: BodyKind,
    pub fresh: bool,
    pub enclosed: bool,
    pub forced: bool,
    pub level_m: f64,
    pub area_m2: f64,
    pub depth_m: f64,
    pub outlet_reach: Option<u32>,
    pub anchor: (f64, f64),
    pub outline: Vec<(f64, f64)>,
}

/// One node along a reach's course.
#[derive(Debug, Clone, PartialEq)]
pub struct ReachPoint {
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub bed_m: f64,
    pub width_m: f64,
    pub depth_m: f64,
    pub flow_m2: f64,
}

/// A stream, river or great river, as a polyline of `ReachPoint`s.
#[derive(Debug, Clone, PartialEq)]
pub struct ReachLine {
    pub id: u32,
    pub class: ReachClass,
    pub order: u32,
    pub downstream: Downstream,
    pub points: Vec<ReachPoint>,
}

/// A cut channel through a notched hollow's rim, as the falling course `routing::cut_route` or
/// `cut_path` left behind.
#[derive(Debug, Clone, PartialEq)]
pub struct NotchLine {
    pub points: Vec<(f64, f64, f64)>,
}

/// A drop along a reach. Empty in 1a: no waterfall geometry is derived yet.
#[derive(Debug, Clone, PartialEq)]
pub struct Fall {
    pub reach: u32,
    pub at: (f64, f64),
    pub height_m: f64,
}

/// Counts and summary numbers from one bake, for a survey or a log line -- never round-tripped
/// through anything but the record itself.
#[derive(Debug, Clone, PartialEq)]
pub struct BakeStats {
    pub nodes: u32,
    pub land_nodes: u32,
    pub hollows: u32,
    pub kept: u32,
    pub notched: u32,
    pub closed: u32,
    pub streams: u32,
    pub rivers: u32,
    pub great: u32,
    pub max_order: u32,
    pub bifurcation_min: f64,
    pub bifurcation_max: f64,
    /// Ruling 12b-1: the effective thresholds this bake actually used, after the
    /// resolution-aware floor -- so a record says what it used, not just what `HydroParams`
    /// asked for.
    pub stream_flow_m2: f64,
    pub river_flow_m2: f64,
    pub great_flow_m2: f64,
}

/// Everything a bake produces: the standing water, the channels, the notches that drain the
/// hollows the keep rule declined, the falls, and the stats. `record::encode`/`decode` give
/// this a flat `f64` wire form.
#[derive(Debug, Clone, PartialEq)]
pub struct HydroRecord {
    pub bodies: Vec<Body>,
    pub reaches: Vec<ReachLine>,
    pub notches: Vec<NotchLine>,
    pub falls: Vec<Fall>,
    pub stats: BakeStats,
}

/// Why a bake could not be produced.
#[derive(Debug, Clone, PartialEq)]
pub enum HydroError {
    Params(&'static str),
    Sampling,
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
/// to the last point; bed_m keeps the terminal node's own surface, unchanged.
fn reach_points(graph: &LandGraph, routing: &routing::Routing, flow: &[f64], nodes: &[u32], params: &HydroParams) -> Vec<ReachPoint> {
    let last_index = nodes.len() - 1;
    let mut points = Vec::with_capacity(nodes.len());
    for (idx, &node) in nodes.iter().enumerate() {
        let (lat_deg, lon_deg) = graph.positions[node as usize].to_latlon();
        let is_terminal = idx == last_index
            && (graph.ocean[node as usize] || routing.lake_of[node as usize] != NO_LAKE);
        let q = if is_terminal && idx > 0 { flow[nodes[idx - 1] as usize] } else { flow[node as usize] };
        let w = width_m(q, params);
        let d = depth_m(q, params);
        let bed_m = if is_terminal { routing.surface_m[node as usize] } else { routing.surface_m[node as usize] - d };
        points.push(ReachPoint { lat_deg, lon_deg, bed_m, width_m: w, depth_m: d, flow_m2: q });
    }
    points
}

/// The bake, end to end: `LandGraph::sample` -> `flood(ocean_seeds)` -> `find_hollows` +
/// `judge` -> `route` -> `close_lakes` -> `extract`, folded into the public record types.
pub fn bake(surface: &Surface, params: &HydroParams) -> Result<HydroRecord, HydroError> {
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

    // Ruling 12b-1: the effective thresholds a graph this coarse can actually resolve. Sorted,
    // deterministic median of the land-node areas (the lower of the two middles on an even
    // count), never a HashMap and never an arbitrary tie-break.
    let mut land_areas: Vec<f64> = (0..graph.len())
        .filter(|&i| !graph.ocean[i])
        .map(|i| graph.area_m2[i])
        .collect();
    land_areas.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
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
    let reaches = extract(&graph, &routing, &flow, &effective_params);

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
        let outlet_node = hollow.outlet as usize;
        let outlet_reach = if reach_of_first_node[outlet_node] != u32::MAX {
            Some(reach_of_first_node[outlet_node])
        } else {
            None
        };
        let (anchor_lat, anchor_lon) = graph.positions[hollow.floor as usize].to_latlon();
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
        let points = reach_points(&graph, &routing, &flow, &reach.nodes, params);
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

    Ok(HydroRecord { bodies, reaches: reach_lines, notches, falls, stats })
}

/// Walks downstream from every reach and fails on a revisit. `mod.rs` owns it (rather than
/// `reaches.rs`) because the survey (a later task) reuses it against `ReachLine`, the public
/// record type, not `reaches::Reach`.
pub fn reaches_are_acyclic(reaches: &[ReachLine]) -> bool {
    for start in 0..reaches.len() {
        let mut seen = vec![false; reaches.len()];
        let mut here = start;
        loop {
            if seen[here] {
                return false;
            }
            seen[here] = true;
            match reaches[here].downstream {
                Downstream::Reach(next) => {
                    let next = next as usize;
                    if next >= reaches.len() {
                        return false;
                    }
                    here = next;
                }
                _ => break,
            }
        }
    }
    true
}

#[cfg(test)]
mod bake_tests {
    use super::*;
    use crate::hydrology::record::{decode, encode};
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
        let a = encode(&bake(&world(), &params()).expect("bake"));
        let b = encode(&bake(&world(), &params()).expect("bake"));
        assert_eq!(a.iter().map(|w| w.to_bits()).collect::<Vec<_>>(),
                   b.iter().map(|w| w.to_bits()).collect::<Vec<_>>());
    }

    #[test]
    fn the_record_round_trips() {
        let record = bake(&world(), &params()).expect("bake");
        let words = encode(&record);
        assert_eq!(decode(&words).as_ref(), Some(&record));
        assert_eq!(words[0], record::SCHEMA);
    }

    /// Ruling 12b-1: the effective thresholds rise to what a graph this coarse can resolve --
    /// at least the floor `min_stream_nodes * median land-node area`, and at least the Earth
    /// value where that binds instead, with river/great kept at 10x the reach below them.
    #[test]
    fn effective_thresholds_rise_to_the_graph_resolution() {
        let p = params();
        let record = bake(&world(), &p).expect("bake");

        let graph = LandGraph::sample(&world(), p.total_nodes, p.wetness_nodes).expect("graph");
        let mut land_areas: Vec<f64> =
            (0..graph.len()).filter(|&i| !graph.ocean[i]).map(|i| graph.area_m2[i]).collect();
        land_areas.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mid = if land_areas.len() % 2 == 0 { land_areas.len() / 2 - 1 } else { land_areas.len() / 2 };
        let median_land_area_m2 = land_areas[mid];

        assert!(record.stats.stream_flow_m2 >= p.min_stream_nodes * median_land_area_m2);
        assert!(record.stats.stream_flow_m2 >= p.stream_flow_m2);
        assert!(record.stats.river_flow_m2 >= 10.0 * record.stats.stream_flow_m2);
        assert!(record.stats.river_flow_m2 >= p.river_flow_m2);
        assert!(record.stats.great_flow_m2 >= 10.0 * record.stats.river_flow_m2);
        assert!(record.stats.great_flow_m2 >= p.great_flow_m2);
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

        let record = bake(&surface, &p).expect("bake");
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
        let words = encode(&bake(&world(), &params()).expect("bake"));
        assert_eq!(decode(&words[..words.len() - 1]), None);
        assert_eq!(decode(&[]), None);
    }

    #[test]
    fn no_kept_body_is_below_the_keep_rule_unless_forced_or_enclosed() {
        let p = params();
        let record = bake(&world(), &p).expect("bake");
        for body in &record.bodies {
            if !body.forced && !body.enclosed {
                assert!(body.depth_m >= p.keep_depth_m && body.area_m2 >= p.keep_area_m2,
                        "body {} depth {} area {}", body.id, body.depth_m, body.area_m2);
            }
        }
    }

    #[test]
    fn every_open_lake_has_one_outlet_reach_or_drains_straight_to_the_sea() {
        let record = bake(&world(), &params()).expect("bake");
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
        assert!(matches!(bake(&world(), &p), Err(HydroError::Params(_))));
        let mut p = params();
        p.total_nodes = 1;
        assert!(matches!(bake(&world(), &p), Err(HydroError::Params(_))));
    }

    /// Sanity check only -- it does not discriminate. Flow only ever accumulates downstream, so
    /// the old (wrong) terminal-point value, the ocean/lake's total inflow, is structurally
    /// always `>=` the fixed, channel-only value on this bake world's topology (one land
    /// neighbor per ocean cell); it passes before and after the fix (see fix round 1's report).
    /// `the_mouth_point_carries_its_own_river_not_the_whole_sea` below is the discriminating
    /// regression test.
    #[test]
    fn a_reach_keeps_its_width_to_the_sea() {
        let record = bake(&world(), &params()).expect("bake");
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

        let points = reach_points(&g, &routing, &flow, &nodes, &params);
        let last = points.last().expect("at least one point");
        assert_eq!(last.flow_m2, flow[1],
                   "the mouth point must carry node 1's own channel flow, not node 2's");
        assert!(last.flow_m2 < flow[2],
                "node 2's accumulated flow also holds the right branch's inflow, so the \
                 channel's own flow must be strictly less: last {} flow[2] {}",
                last.flow_m2, flow[2]);
    }

    /// Mutation guard for the connectivity property: a hand-broken reach list must fail it.
    #[test]
    fn the_connectivity_check_catches_a_cycle() {
        let mut record = bake(&world(), &params()).expect("bake");
        assert!(reaches_are_acyclic(&record.reaches));
        if record.reaches.len() >= 2 {
            record.reaches[0].downstream = Downstream::Reach(1);
            record.reaches[1].downstream = Downstream::Reach(0);
            assert!(!reaches_are_acyclic(&record.reaches));
        }
    }
}
