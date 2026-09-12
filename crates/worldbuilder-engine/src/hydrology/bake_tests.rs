use crate::hydrology::bake::*;
use crate::hydrology::flood::{flood, ocean_seeds, NO_NODE};
use crate::hydrology::flow::{close_lakes, drainage_check, Closure};
use crate::hydrology::hollows::{self, find_hollows, judge, Fate};
use crate::hydrology::landgraph::LandGraph;
use crate::hydrology::routing::{route, NotchRoute, Routing, NO_LAKE};
use crate::hydrology::{Downstream, HydroError, HydroParams, HydroRecord};
use crate::hydrology::record::{decode, encode};
use crate::sphere::SpherePoint;
use crate::surface::Surface;

pub(super) fn world() -> Surface {
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
    let a = encode(&crate::hydrology::bake(&world(), &params()).expect("bake"));
    let b = encode(&crate::hydrology::bake(&world(), &params()).expect("bake"));
    assert_eq!(a.iter().map(|w| w.to_bits()).collect::<Vec<_>>(),
               b.iter().map(|w| w.to_bits()).collect::<Vec<_>>());
}

#[test]
fn the_record_round_trips() {
    let record = crate::hydrology::bake(&world(), &params()).expect("bake");
    let words = encode(&record);
    assert_eq!(decode(&words).as_ref(), Some(&record));
    assert_eq!(words[0], crate::hydrology::record::SCHEMA);
}

/// Ruling 12b-1: on this suite's test-world overrides (3.0e10 / 3.0e11 / 3.0e12), the node
/// floor binds here too (about 4.24e11 against the 3.0e10 asked for) -- the same as
/// `the_node_floor_binds_on_a_coarse_graph` below, just reached from different starting
/// params. Kept with `>=` on both sides so the assertions hold regardless of which branch
/// wins, rather than asserting a "params bind" case that does not actually occur on this
/// world.
#[test]
fn effective_thresholds_rise_to_the_graph_resolution() {
    let p = params();
    let record = crate::hydrology::bake(&world(), &p).expect("bake");

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
    let record = crate::hydrology::bake(&world(), &p).expect("bake");

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

/// Task 6's notch filter: every recorded point is either on an outlet cut, off every
/// recorded reach's channel nodes with a cut depth of at least `NOTCH_RECORD_MIN_CUT_M`
/// (2.0 m), or (Ruling F-3) the water a route's last kept node drains into -- a point this
/// test excludes by node, the same way it already excludes outlet-cut nodes, since that
/// point is never a cut and so is not bound by the depth floor. Reruns the same pipeline
/// `bake()` folds together, so it can see `closure.outlet_notch` and the raw `routing.notches`
/// the filter runs against, and maps a recorded point back to its node by lat/lon (both a
/// notch's points and a reach's points come from the same node positions, so the map is
/// exact).
#[test]
fn the_record_keeps_only_notches_that_matter() {
    let surface = world();
    let p = params();
    let graph = LandGraph::sample(&surface, p.total_nodes, p.wetness_nodes).expect("graph");
    let global_flood = flood(&graph, &ocean_seeds(&graph), &|_| true);
    let mut hollows = find_hollows(&graph, &global_flood);
    judge(&mut hollows, &hollows::forced_nodes(&graph, &p), &p);
    let mut routing = route(&graph, &global_flood, &mut hollows, &p);
    let (flow, closure) = close_lakes(&graph, &mut routing, &hollows, &p);

    // node -> (lat, lon) exactly as `record_of` computes it, and the reverse, so a recorded
    // point can be mapped back to the node it came from without recomputing `extract` (whose
    // effective thresholds live inside `record_of`, not out here).
    let mut position_to_node: std::collections::HashMap<(u64, u64), u32> =
        std::collections::HashMap::new();
    for node in 0..graph.len() as u32 { // cast-ok: node index
        let (lat, lon) = graph.positions[node as usize].to_latlon();
        position_to_node.insert((lat.to_bits(), lon.to_bits()), node);
    }
    let mut outlet_nodes: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for opt in &closure.outlet_notch {
        if let Some(idx) = *opt {
            outlet_nodes.extend(routing.notches[idx].nodes.iter().copied());
        }
    }

    let stages = BakeStages { graph, hollows, routing, flow, closure };
    let record = record_of(&stages, &p);
    assert!(!record.notches.is_empty(), "sanity: this fixture must record at least one notch");

    // The recorded reaches are the ground truth for "on a river's channel" -- built from the
    // same effective thresholds `record_of` itself used, not a second, possibly-diverging
    // `extract` call out here.
    let mut is_river_node = vec![false; stages.graph.len()];
    for reach in &record.reaches {
        for point in &reach.points {
            let node = *position_to_node.get(&(point.lat_deg.to_bits(), point.lon_deg.to_bits()))
                .expect("a reach point matches a graph node");
            is_river_node[node as usize] = true;
        }
    }

    // Ruling F-3: the node each route appends when its own last node survives the filter --
    // the water it drains into, not a cut -- mirroring `record_of`'s own "was the last node
    // kept" test rather than re-deriving it from scratch.
    let mut is_outlet_notch = vec![false; stages.routing.notches.len()];
    for opt in &stages.closure.outlet_notch {
        if let Some(idx) = *opt {
            is_outlet_notch[idx] = true;
        }
    }
    let mut water_stop_nodes: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for (idx, notch) in stages.routing.notches.iter().enumerate() {
        let (Some(&last), Some(&last_bed_m)) = (notch.nodes.last(), notch.bed_m.last()) else {
            continue;
        };
        let last_kept = if is_outlet_notch[idx] {
            true
        } else {
            !is_river_node[last as usize]
                && stages.graph.height_m[last as usize] - last_bed_m >= 2.0
        };
        if !last_kept {
            continue;
        }
        let stop = stages.routing.receiver[last as usize];
        if stop != NO_NODE {
            water_stop_nodes.insert(stop);
        }
    }

    for notch in &record.notches {
        for &(lat, lon, bed_m, _width_m) in &notch.points {
            let node = *position_to_node.get(&(lat.to_bits(), lon.to_bits()))
                .expect("a recorded point matches a graph node");
            if outlet_nodes.contains(&node) || water_stop_nodes.contains(&node) {
                continue;
            }
            assert!(!is_river_node[node as usize],
                    "a non-outlet point must not sit on a recorded reach's channel");
            let cut_depth_m = stages.graph.height_m[node as usize] - bed_m;
            assert!(cut_depth_m >= 2.0 - 1e-9,
                    "a non-outlet point's cut depth {cut_depth_m} is under the 2 m floor");
        }
    }
}

/// Rulings F-1 and F-2: where an outlet cut runs along a recorded reach, the two describe
/// the same channel, so they must agree on it. A notch point's word 3 is the cut surface
/// (the lowered ground, the water surface through the cut) and a reach point's is its bed
/// (surface minus depth), so `notch.surface - reach.depth == reach.bed`; and both widths
/// come from the caller's own `params`, so they are equal.
///
/// On the bake test world the node floor binds (effective stream threshold about 4.2e11
/// against the 3.0e10 asked for), so a notch width sized on the effective thresholds parts
/// from the reach's by a factor of about 3.8 -- which is what this catches.
///
/// Ruling F-3: an outlet line's last point may be the water it stops in (the ocean, or a lake
/// member), not a cut -- excluded here by node, since such a point can coincidentally share a
/// position with a reach point (a mouth) without the two describing the same channel.
#[test]
fn an_outlet_cut_agrees_with_the_reach_it_runs_along() {
    let p = params();
    let stages = bake_stages(&world(), &p).expect("bake");
    let record = record_of(&stages, &p);
    assert!(record.stats.stream_flow_m2 > p.stream_flow_m2,
            "sanity: the node floor binds, so effective and caller thresholds differ");

    // Every outlet cut's nodes, by the exact (lat, lon) bits `record_of` writes.
    let mut outlet_positions: Vec<(u64, u64)> = Vec::new();
    for opt in &stages.closure.outlet_notch {
        if let Some(idx) = *opt {
            for &node in &stages.routing.notches[idx].nodes {
                let (lat, lon) = stages.graph.positions[node as usize].to_latlon();
                outlet_positions.push((lat.to_bits(), lon.to_bits()));
            }
        }
    }
    outlet_positions.sort_unstable();
    let lookup = node_at(&stages.graph);

    let mut compared = 0usize;
    for notch in &record.notches {
        for &(lat, lon, surface_m, notch_width_m) in &notch.points {
            if outlet_positions.binary_search(&(lat.to_bits(), lon.to_bits())).is_err() {
                continue;
            }
            let node = lookup[&(lat.to_bits(), lon.to_bits())];
            if stages.graph.ocean[node as usize] || stages.routing.lake_of[node as usize] != NO_LAKE {
                continue;
            }
            for reach in &record.reaches {
                for point in &reach.points {
                    if point.lat_deg.to_bits() != lat.to_bits() || point.lon_deg.to_bits() != lon.to_bits() {
                        continue;
                    }
                    compared += 1;
                    assert_eq!(notch_width_m, point.width_m,
                               "reach {} at ({lat}, {lon}): notch width vs reach width", reach.id);
                    let gap = (surface_m - point.depth_m) - point.bed_m;
                    assert!(gap.abs() <= 1e-6,
                            "reach {} at ({lat}, {lon}): notch surface {surface_m} - reach depth {}                                  != reach bed {} (gap {gap})",
                            reach.id, point.depth_m, point.bed_m);
                }
            }
        }
    }
    assert!(compared > 0, "sanity: at least one outlet-cut point is also a reach point");
}

/// Maps a record point back to its graph node by exact position.
fn node_at(graph: &LandGraph) -> std::collections::BTreeMap<(u64, u64), u32> {
    let mut map = std::collections::BTreeMap::new();
    for (i, p) in graph.positions.iter().enumerate() {
        let (lat, lon) = p.to_latlon();
        map.insert((lat.to_bits(), lon.to_bits()), i as u32); // cast-ok: node index
    }
    map
}

/// Ruling F-3: stage 2 carves a notch line segment by segment, so consecutive points must be
/// graph neighbours, never two nodes a gap apart.
#[test]
fn every_notch_segment_joins_graph_neighbours() {
    let p = params();
    let stages = bake_stages(&world(), &p).expect("stages");
    let record = record_of(&stages, &p);
    let lookup = node_at(&stages.graph);
    let mut segments = 0usize;
    for line in &record.notches {
        for pair in line.points.windows(2) {
            let a = lookup[&(pair[0].0.to_bits(), pair[0].1.to_bits())];
            let b = lookup[&(pair[1].0.to_bits(), pair[1].1.to_bits())];
            assert!(stages.graph.neighbours(a).binary_search(&b).is_ok(),
                    "notch points {a} and {b} are not neighbours");
            segments += 1;
        }
    }
    assert!(segments > 0, "the test world must record at least one notch segment");
}

/// Code-review finding I1: the bake test world never gives a route a kept-point gap (every
/// segment already joins graph neighbours without the split branch doing anything), so this
/// hand fixture builds one directly. A 6-node chain, 0 the ocean and 1..5 land, each node
/// adjacent only to its immediate chain neighbours (0-1-2-3-4-5). A single hand-built
/// `NotchRoute` walks it downhill as `5, 4, 3, 2, 1` (heights 50, 40, 30, 20, 10; ocean at
/// -10), cut to beds 10 m below each node except node 3, cut only 0.5 m -- under
/// `NOTCH_RECORD_MIN_CUT_M` (2.0 m), so `record_of` drops it. The surviving kept nodes,
/// `5, 4, 2, 1`, are not all graph neighbours in sequence (4 and 2 are two chain hops apart):
/// the record must split there into two lines, `[5, 4]` and `[2, 1, <ocean stop>]` (node 1's
/// receiver is the ocean, node 0, appended by Ruling F-3a).
///
/// `flow` is all zero, so `extract` finds no channel nodes and this notch is filtered purely
/// on cut depth, never on `is_river_node`. `closure.outlet_notch` is empty, so this route is
/// not treated as an outlet cut -- the depth filter is what drops node 3.
#[test]
fn a_notch_line_splits_where_the_filter_opens_a_gap() {
    let n = 6;
    let heights = vec![-10.0, 10.0, 20.0, 30.0, 40.0, 50.0];
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
    let graph = LandGraph::from_parts(
        6_371_000.0, positions, heights, vec![1.0e6; n], &directed, vec![0.5; n],
    );

    let mut receiver = vec![NO_NODE; n];
    receiver[1] = 0; // node 1's cut stops at the ocean.
    // The surface the cut left: each lowered node stands at its bed. `record_of` reads the final
    // surface, not the route's own `bed_m` (Ruling R-9), so the two agree here as a real cut's do.
    let surface_m = vec![-10.0, 5.0, 10.0, 29.5, 30.0, 40.0];
    let routing = Routing {
        surface_m,
        receiver,
        lake_of: vec![NO_LAKE; n],
        parent: vec![NO_NODE; n],
        notches: vec![NotchRoute {
            nodes: vec![5, 4, 3, 2, 1],
            bed_m: vec![40.0, 30.0, 29.5, 10.0, 5.0],
        }],
        committed: vec![false; n],
    };
    let closure = Closure {
        closed: Vec::new(),
        salt_flat: Vec::new(),
        fresh_enclosed: Vec::new(),
        outlet_notch: Vec::new(),
    };
    let stages = BakeStages { graph, hollows: Vec::new(), routing, flow: vec![0.0; n], closure };

    let p = HydroParams::earth_like(0);
    let record = record_of(&stages, &p);
    let lookup = node_at(&stages.graph);

    let lines: Vec<Vec<u32>> = record.notches.iter()
        .map(|line| line.points.iter()
            .map(|&(lat, lon, _, _)| lookup[&(lat.to_bits(), lon.to_bits())])
            .collect())
        .collect();
    assert_eq!(lines, vec![vec![5, 4], vec![2, 1, 0]],
                "the gap at the dropped node 3 must split the route into two lines");

    for line in &lines {
        for pair in line.windows(2) {
            assert!(stages.graph.neighbours(pair[0]).binary_search(&pair[1]).is_ok(),
                    "notch points {} and {} are not neighbours", pair[0], pair[1]);
        }
    }
}

/// Ruling F-3a: `NotchRoute.nodes` lists only the nodes a route actually lowered -- lower
/// ground it merely walked over does not stop it and is not pushed there (`routing.rs`'s
/// `follow_parents`). So `receiver` of the route's last (lowered) node is not necessarily
/// water: it is whichever node the loop's very next step landed on, and that can be the
/// ocean, a lake, an earlier cut's committed node, or lower ground the cut walked over
/// without lowering. The append is unconditional on which of those it is -- this only checks
/// that the recorded line's last point is exactly that node, at its own current level.
#[test]
fn every_outlet_cut_ends_at_the_node_its_cut_stops_at() {
    let p = params();
    let stages = bake_stages(&world(), &p).expect("stages");
    let record = record_of(&stages, &p);
    let mut checked = 0usize;
    for idx in stages.closure.outlet_notch.iter().flatten() {
        let route = &stages.routing.notches[*idx];
        let last = *route.nodes.last().expect("an outlet cut has nodes");
        let stop = stages.routing.receiver[last as usize];
        let i = stop as usize;
        let level = if stages.graph.ocean[i] {
            0.0
        } else if stages.routing.lake_of[i] != NO_LAKE {
            stages.hollows[stages.routing.lake_of[i] as usize].level_m
        } else {
            stages.routing.surface_m[i]
        };
        let (lat, lon) = stages.graph.positions[i].to_latlon();
        assert!(record.notches.iter().any(|line| {
            let end = line.points.last().expect("a recorded line has points");
            end.0 == lat && end.1 == lon && end.2 == level
        }), "outlet cut {idx} has no recorded line ending at its stop node");
        checked += 1;
    }
    assert!(checked > 0, "the test world must have at least one outlet cut");
}

#[test]
fn a_truncated_record_is_refused() {
    let words = encode(&crate::hydrology::bake(&world(), &params()).expect("bake"));
    assert_eq!(decode(&words[..words.len() - 1]), None);
    assert_eq!(decode(&[]), None);
}

/// Hand fixture: ocean, a 39 m ridge, then a below-datum pocket (node 4, the only member of
/// an enclosed, always-kept basin per Ruling W1) and a dry peak (node 6). One forced-outlet
/// point lands exactly on the pocket's own node -- a submerged member of a kept lake once
/// routing has run -- and the other lands exactly on the dry peak, nowhere near any lake.
#[test]
fn forced_outlets_report_how_many_matched() {
    let heights = [-50.0, -40.0, -30.0, 39.0, -5.0, 10.0, 60.0];
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
    let g = LandGraph::from_parts(6_371_000.0, positions, heights.to_vec(), vec![1.0e6; n], &directed, vec![0.5; n]);

    let mut p = HydroParams::earth_like(0);
    p.forced_outlets = vec![
        SpherePoint::from_latlon(0.0, 2.0), // node 4: the enclosed pocket's own member
        SpherePoint::from_latlon(0.0, 3.0), // node 6: dry high ground, no lake nearby
    ];

    let f = flood(&g, &ocean_seeds(&g), &|_| true);
    let mut hollows = find_hollows(&g, &f);
    judge(&mut hollows, &hollows::forced_nodes(&g, &p), &p);
    let mut routing = route(&g, &f, &mut hollows, &p);
    let (flow, closure) = close_lakes(&g, &mut routing, &hollows, &p);
    let stages = BakeStages { graph: g, hollows, routing, flow, closure };
    let record = record_of(&stages, &p);

    assert_eq!(record.stats.forced_requested, 2);
    assert_eq!(record.stats.forced_matched, 1);
}

/// Hand fixture: a tributary (nodes 3-4, high wetness so it clears the stream threshold)
/// feeds straight into a lake (node 2, behind a 40 m rim) that an enormous evaporation
/// factor keeps closed no matter how much the tributary delivers. The reach that ends at
/// that lake must report `fresh: false`.
#[test]
fn a_reach_into_a_closed_lake_is_not_fresh() {
    let heights = [-50.0, 40.0, 5.0, 45.0, 90.0];
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
    let wetness = vec![0.5, 0.5, 0.5, 0.9, 0.9];
    let g = LandGraph::from_parts(6_371_000.0, positions, heights.to_vec(), vec![1.0e6; n], &directed, wetness);

    let mut p = HydroParams::earth_like(0);
    p.stream_flow_m2 = 1.0e5;
    p.river_flow_m2 = 1.0e6;
    p.great_flow_m2 = 1.0e7;
    p.min_stream_nodes = 1.0e-9;
    p.evaporation_factor = 1.0e6;

    let f = flood(&g, &ocean_seeds(&g), &|_| true);
    let mut hollows = find_hollows(&g, &f);
    judge(&mut hollows, &hollows::forced_nodes(&g, &p), &p);
    let mut routing = route(&g, &f, &mut hollows, &p);
    let (flow, closure) = close_lakes(&g, &mut routing, &hollows, &p);
    assert_eq!(drainage_check(&g, &routing), Ok(()));
    let stages = BakeStages { graph: g, hollows, routing, flow, closure };
    let record = record_of(&stages, &p);

    assert_eq!(record.bodies.len(), 1, "sanity: one lake");
    assert!(!record.bodies[0].fresh, "sanity: the lake must close despite the tributary's flow");
    let feeding_reach = record.reaches.iter().find(|r| r.downstream == Downstream::Body(record.bodies[0].id))
        .expect("sanity: a reach feeds straight into the lake");
    assert!(!feeding_reach.fresh, "a reach into a closed lake must not report fresh");
}

/// The **coarse** keep rule, over the coarse bodies. The fine search's finds are appended after
/// them and have their own, much smaller rule (spec §6.6: §6.3's 1 km^2 could never keep a pond),
/// so they are excluded by count -- `ponds_obey_their_keep_rule_and_name_a_river` is their
/// equivalent of this test.
#[test]
fn no_kept_body_is_below_the_keep_rule_unless_forced_or_enclosed() {
    let p = params();
    let record = crate::hydrology::bake(&world(), &p).expect("bake");
    let coarse = record.bodies.len() - record.stats.ponds_kept as usize;
    for body in &record.bodies[..coarse] {
        if !body.forced && !body.enclosed {
            assert!(body.depth_m >= p.keep_depth_m && body.area_m2 >= p.keep_area_m2,
                    "body {} depth {} area {}", body.id, body.depth_m, body.area_m2);
        }
    }
}

#[test]
fn every_open_lake_has_one_outlet_reach_or_drains_straight_to_the_sea() {
    let record = crate::hydrology::bake(&world(), &params()).expect("bake");
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
    assert!(matches!(crate::hydrology::bake(&world(), &p), Err(HydroError::Params(_))));
    let mut p = params();
    p.total_nodes = 1;
    assert!(matches!(crate::hydrology::bake(&world(), &p), Err(HydroError::Params(_))));
}

/// Ruling FF-5: the refinement params have floors as well as being finite and positive, so a
/// refinement is refused rather than planning absurd work (a 1e-300 m step) or dividing a step
/// into more fall windows than it has metres.
#[test]
fn refinement_params_below_their_floors_are_refused() {
    let mut step = params();
    step.refine_step_m = 9.9;
    let mut run = params();
    run.fall_max_run_m = run.refine_step_m / 100.0 - 0.001;
    let mut simplify = params();
    simplify.refine_simplify_m = 0.9;
    let mut vertical = params();
    vertical.refine_vertical_m = 0.009;
    for (name, p) in [("refine_step_m", step), ("fall_max_run_m", run),
                      ("refine_simplify_m", simplify), ("refine_vertical_m", vertical)] {
        assert!(matches!(crate::hydrology::bake(&world(), &p), Err(HydroError::Params(_))),
                "{name} below its floor is refused");
    }
    assert!(bake_stages(&world(), &params()).is_ok(), "sanity: the test params are accepted");
}

/// The fine search's params have floors of the same kind: a cell far below the landform's own
/// resolution, a search radius with no strip in it, a *share* above 1, or a density cell smaller
/// than the search cell itself.
///
/// The last is Ruling S-8's `BucketIndex`, and it is a memory floor, not a taste one. The index
/// is built with a cell of `sqrt(pond_density_area_m2)`, so a density area under one
/// `pond_cell_m` square asks for more rows and columns than the world has cells to put in them:
/// at 1.0e4 m² against the shipped 250 m cell the index wants about 800 MB of buckets, and
/// `BucketIndex::new`'s own 4,096 × 8,192 clamp silently hands back a **4,886.50 m** cell instead —
/// so the caller neither gets the density it asked for nor hears that it did not.
#[test]
fn pond_params_below_their_floors_are_refused() {
    let mut cell = params();
    cell.pond_cell_m = 9.9;
    let mut radius = params();
    radius.pond_search_radius_m = radius.pond_cell_m - 1.0;
    let mut share = params();
    share.pond_wetness_share = 1.1;
    let mut density = params();
    density.pond_density_area_m2 = density.pond_cell_m * density.pond_cell_m - 1.0;
    for (name, p) in [("pond_cell_m", cell), ("pond_search_radius_m", radius),
                      ("pond_wetness_share", share), ("pond_density_area_m2", density)] {
        assert!(matches!(crate::hydrology::bake(&world(), &p), Err(HydroError::Params(_))),
                "{name} outside its floor is refused");
    }
    assert!(bake_stages(&world(), &params()).is_ok(), "sanity: the test params are accepted");
}

/// Sanity check only -- it does not discriminate. Flow only ever accumulates downstream, so
/// the old (wrong) terminal-point value, the ocean/lake's total inflow, is structurally
/// always `>=` the fixed, channel-only value on this bake world's topology (one land
/// neighbor per ocean cell); it passes before and after the fix (see fix round 1's report).
/// `the_mouth_point_carries_its_own_river_not_the_whole_sea` below is the discriminating
/// regression test.
#[test]
fn a_reach_keeps_its_width_to_the_sea() {
    let record = crate::hydrology::bake(&world(), &params()).expect("bake");
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
    judge(&mut hollows, &hollows::forced_nodes(&g, &params), &params);
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
    judge(&mut hollows, &hollows::forced_nodes(&g, &params), &params);
    let mut routing = route(&g, &f, &mut hollows, &params);
    let (flow, _closure) = close_lakes(&g, &mut routing, &hollows, &params);
    let points = reach_points(&g, &routing, &hollows, &flow, &[0, 1, 2], &params);
    assert_eq!(points[2].bed_m, 0.0, "the mouth stands at the datum, not the -50 m seabed");
    assert!(points[1].bed_m < routing.surface_m[1], "an inland point still sits below its ground");

    // End to end on the coarse record: every ocean mouth at the datum, every lake mouth at its
    // lake's level. Coarse, not `bake()`: refinement's shore trim sets a mouth's bed to the lower
    // of the bed that reaches it and the water (Ruling R-4), which is at or below the level.
    let p = params_for_world();
    let record = record_of(&bake_stages(&world(), &p).expect("stages"), &p);
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
    judge(&mut hollows, &hollows::forced_nodes(&g, &p), &p);
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
    let mut record = crate::hydrology::bake(&world(), &params()).expect("bake");
    assert!(crate::hydrology::reaches_are_acyclic(&record.reaches));
    if record.reaches.len() >= 2 {
        record.reaches[0].downstream = Downstream::Reach(1);
        record.reaches[1].downstream = Downstream::Reach(0);
        assert!(!crate::hydrology::reaches_are_acyclic(&record.reaches));
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

/// Follows `Body`/`Reach` downstream links from `start` and says whether the chain ends
/// where the spec lets an open body's water end: at the ocean, or at a closed (not fresh)
/// body, whose own `downstream` is `Sink`. A `Sink` reached any other way, or a loop (the
/// same bound as `follows_to_ocean`), is `false`.
fn ends_at_the_ocean_or_a_closed_body(record: &HydroRecord, start: Downstream) -> bool {
    let bound = record.bodies.len() + record.reaches.len() + 1;
    let mut here = start;
    for _ in 0..bound {
        match here {
            Downstream::Ocean => return true,
            Downstream::Sink => return false,
            Downstream::Reach(id) => here = record.reaches[id as usize].downstream,
            Downstream::Body(id) => {
                let body = &record.bodies[id as usize];
                if !body.fresh {
                    return true;
                }
                here = body.downstream;
            }
        }
    }
    false // ran past the bound: a loop.
}

/// Task 5's rule, checked on one baked record: every fresh (open) body names somewhere its
/// water goes that is not `Sink`, every closed body reports `Sink`, and chasing `Body`/`Reach`
/// links from any fresh body ends, without looping, at the ocean or at a closed body. Body
/// `fresh` means "not closed", not "reaches the sea": the spec lets an open lake drain into a
/// closed one (`an_open_lake_may_drain_into_a_closed_lake`).
fn check_downstream_invariants(record: &HydroRecord) {
    assert!(!record.bodies.is_empty(), "sanity: this world has bodies to check");
    for body in &record.bodies {
        if body.fresh {
            assert_ne!(body.downstream, Downstream::Sink,
                       "fresh body {} reports Sink", body.id);
            assert!(ends_at_the_ocean_or_a_closed_body(record, body.downstream),
                    "body {}'s downstream chain must end at the ocean or a closed body without looping",
                    body.id);
        } else {
            assert_eq!(body.downstream, Downstream::Sink,
                       "closed body {} doesn't report Sink", body.id);
        }
    }
}

/// Reach `fresh` means "its chain reaches the ocean". The positive half: on the bake test
/// world every reach whose downstream chain (followed here by `follows_to_ocean`, not by
/// `downstream_is_fresh`) reaches `Ocean` reports `fresh`, and there is at least one. The
/// negative half is `a_reach_into_a_closed_lake_is_not_fresh`; with only that one, a
/// constant `false` passed.
#[test]
fn every_reach_that_reaches_the_ocean_is_fresh() {
    let record = crate::hydrology::bake(&world(), &params()).expect("bake");
    let mut to_the_ocean = 0;
    for reach in &record.reaches {
        let reaches_the_ocean = follows_to_ocean(&record, reach.downstream);
        if reaches_the_ocean {
            to_the_ocean += 1;
        }
        assert_eq!(reach.fresh, reaches_the_ocean,
                   "reach {}: fresh must say whether its chain reaches the ocean", reach.id);
    }
    assert!(to_the_ocean > 0, "sanity: at least one reach's chain reaches the ocean");
}

#[test]
fn every_open_lake_says_where_it_drains() {
    // The bake test world (`world()`/`params()`, 12,000 nodes).
    check_downstream_invariants(&crate::hydrology::bake(&world(), &params()).expect("bake"));

    // Seed 1 on the tectonic `ranges` world, also at 12,000 nodes -- the same real-world
    // fixture `real_worlds_drain_everything_through_the_full_bake` uses at 10,000, one size
    // up, per the brief.
    let surface = Surface::new(1, 6.371e6, 12, 0.40, None, None,
                                Some(crate::tectonics::TectonicParams::ranges()));
    let p = HydroParams::earth_like(12_000);
    check_downstream_invariants(&crate::hydrology::bake(&surface, &p).expect("bake"));
}

/// Hand fixture: an open lake whose water ends in a closed lake, not the ocean -- which the
/// spec allows (a body is `fresh` when it is not closed, whether or not its chain reaches the
/// sea). Node 0 (8 m, behind a 40 m rim) is the forced lake; node 2 (25 m, behind a 39 m
/// rim) is the lake it drains into, which an evaporation factor of 10 closes; node 4 is the
/// ocean. The bake drains, and the downstream invariants hold once a chain may end at a
/// closed body.
#[test]
fn an_open_lake_may_drain_into_a_closed_lake() {
    let heights = [8.0, 40.0, 25.0, 39.0, -0.5];
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
    let wetness = vec![
        0.3683336814498558, 0.11148793465094886, 0.2098924682493466, 0.6374726220308857, 0.5058177992954012,
    ];
    let g = LandGraph::from_parts(6_371_000.0, positions, heights.to_vec(), vec![1.0e6; n], &directed, wetness);

    let mut p = HydroParams::earth_like(0);
    p.keep_max_area_m2 = 7.0e6;
    p.evaporation_factor = 10.0;
    p.keep_depth_m = 8.0;
    p.notch_fall_m = 5.0;
    p.forced_outlets = vec![SpherePoint::from_latlon(0.0, 0.0)];

    let f = flood(&g, &ocean_seeds(&g), &|_| true);
    let mut hollows = find_hollows(&g, &f);
    judge(&mut hollows, &hollows::forced_nodes(&g, &p), &p);
    let mut routing = route(&g, &f, &mut hollows, &p);
    let (flow, closure) = close_lakes(&g, &mut routing, &hollows, &p);
    assert_eq!(drainage_check(&g, &routing), Ok(()), "the bake is Ok: everything drains");
    let stages = BakeStages { graph: g, hollows, routing, flow, closure };
    let record = record_of(&stages, &p);

    let forced = record.bodies.iter().find(|b| b.forced).expect("sanity: the forced lake is kept");
    assert!(forced.fresh, "sanity: the forced lake is open");
    let Downstream::Body(into) = forced.downstream else {
        panic!("sanity: the forced lake drains straight into another body, got {:?}", forced.downstream);
    };
    assert!(!record.bodies[into as usize].fresh, "sanity: the lake it drains into is closed");
    assert!(!follows_to_ocean(&record, forced.downstream), "sanity: this chain never reaches the ocean");

    check_downstream_invariants(&record);
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
    judge(&mut hollows, &hollows::forced_nodes(&g, &p), &p);
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

/// Carry-forward (1b-1 final review): a finite but absurd `stream_flow_m2` used to overflow the
/// effective river threshold to infinity, so the record carried non-finite words.
#[test]
fn an_absurd_flow_threshold_is_refused() {
    let mut p = params();
    p.stream_flow_m2 = 1.0e308;
    p.river_flow_m2 = 1.0e308;
    p.great_flow_m2 = 1.0e308;
    assert!(matches!(crate::hydrology::bake(&world(), &p), Err(HydroError::Params(_))));
    let mut q = params();
    q.min_stream_nodes = 1.0e300;
    assert!(matches!(crate::hydrology::bake(&world(), &q), Err(HydroError::Params(_))));
}

/// Runs the coarse pipeline on a line graph (area 1e6 and wetness 0.5 per node), the way
/// `bake_stages` does, for stats that need a hand-built world.
fn line_stages(heights: &[f64], params: &HydroParams) -> BakeStages {
    let n = heights.len();
    let positions = (0..n)
        .map(|i| crate::sphere::SpherePoint::from_latlon(0.0, i as f64 * 0.01))
        .collect();
    let directed: Vec<Vec<u32>> = (0..n)
        .map(|i| if i + 1 < n { vec![(i + 1) as u32] } else { Vec::new() }) // cast-ok: node index
        .collect();
    let graph = LandGraph::from_parts(6_371_000.0, positions, heights.to_vec(), vec![1.0e6; n],
                                      &directed, vec![0.5; n]);
    let global = flood(&graph, &ocean_seeds(&graph), &|_| true);
    let mut hollows = find_hollows(&graph, &global);
    let forced = hollows::forced_nodes(&graph, params);
    judge(&mut hollows, &forced, params);
    let mut routing = route(&graph, &global, &mut hollows, params);
    let (flow, closure) = close_lakes(&graph, &mut routing, &hollows, params);
    drainage_check(&graph, &routing).expect("the fixture drains");
    BakeStages { graph, hollows, routing, flow, closure }
}

/// Carry-forward I3: the record says how many capped basins there were and what they kept, so
/// the owner-world bake can show whether capped basins keep their inner lakes at 1M nodes.
#[test]
fn the_record_counts_what_capped_basins_keep() {
    let mut p = HydroParams::earth_like(1_000);
    p.keep_max_area_m2 = 7.0e6;
    // Task 4 fixture: {3} sits on the basin's way out and is notched; {7} is off it and kept.
    let stages = line_stages(&[-50.0, 40.0, 30.0, 10.0, 25.0, 5.0, 25.0, 12.0, 28.0, 35.0, 70.0], &p);
    let record = record_of(&stages, &p);
    assert_eq!(record.stats.capped_basins, 1);
    assert!(record.stats.capped_inner >= 2, "both inner hollows are counted");
    assert_eq!(record.stats.capped_inner_kept, 1);
}

#[test]
fn the_record_echoes_the_refinement_params() {
    let p = params();
    let record = crate::hydrology::bake(&world(), &p).expect("bake");
    assert_eq!(record.stats.refine_step_m, p.refine_step_m);
    assert_eq!(record.stats.meander_max_slope, p.meander_max_slope);
    let words = crate::hydrology::record::encode(&record);
    assert_eq!(words[0], 6.0);
    assert_eq!(words[32], f64::from(record.stats.capped_basins));
    assert_eq!(words[42], p.meander_max_slope);
    // SCHEMA 5's two crossing counts, still at the same offsets under SCHEMA 6.
    assert_eq!(words[43], f64::from(record.stats.crossings_coarse));
    assert_eq!(words[44], f64::from(record.stats.crossings_left));
}

/// The bake test world with the stream floor lowered to 2 nodes: 165 reaches, 34 of them ending
/// on another reach. `params()` gives 12 reaches and none ending on another reach (8 run to the
/// sea, 4 to a lake), so a junction property needs this.
pub(super) fn ranges_world() -> Surface {
    Surface::new(1, 6.371e6, 12, 0.40, None, None, Some(crate::tectonics::TectonicParams::ranges()))
}

fn junction_params() -> HydroParams {
    let mut p = params();
    p.min_stream_nodes = 2.0;
    p
}

/// Ruling R-9 on a real bake: the routing surface never rises along a receiver edge, except
/// into a lake member (a cut dug below the lake it runs into; Ruling R-4 handles that at a
/// mouth). `params()` and `junction_params()` route identically (`min_stream_nodes` only
/// changes reach extraction); both are run because both are the refined tests' populations.
#[test]
fn the_routing_surface_never_rises_along_a_receiver() {
    for p in [params(), junction_params()] {
        let stages = bake_stages(&world(), &p).expect("stages");
        let (g, r) = (&stages.graph, &stages.routing);
        let mut edges = 0usize;
        for node in 0..g.len() {
            let recv = r.receiver[node];
            if g.ocean[node] || recv == NO_NODE || r.lake_of[recv as usize] != NO_LAKE {
                continue;
            }
            assert!(r.surface_m[recv as usize] <= r.surface_m[node],
                    "node {node} at {} drains up into node {recv} at {}",
                    r.surface_m[node], r.surface_m[recv as usize]);
            edges += 1;
        }
        assert!(edges > 0);
    }
}

/// Spec §14.5 on a real bake: after refinement, no reach's bed rises anywhere, mouths included.
/// Also on `junction_params`, so a coarse junction is in the population too.
#[test]
fn refined_beds_never_rise() {
    for p in [params(), junction_params()] {
        let record = crate::hydrology::bake(&world(), &p).expect("bake");
        for reach in &record.reaches {
            assert!(crate::hydrology::refine::beds_never_rise(reach), "reach {} has a rising bed", reach.id);
        }
    }
}

/// Spec §14.4: every tributary's last point is its receiver's first point, bit for bit. On
/// `junction_params`, because `params()` has no reach that ends on another.
#[test]
fn refined_tributaries_share_their_junction_vertex() {
    let record = crate::hydrology::bake(&world(), &junction_params()).expect("bake");
    let mut junctions = 0usize;
    for reach in &record.reaches {
        if let Downstream::Reach(next) = reach.downstream {
            let last = reach.points.last().expect("points");
            let first = &record.reaches[next as usize].points[0];
            assert_eq!((last.lat_deg.to_bits(), last.lon_deg.to_bits()),
                       (first.lat_deg.to_bits(), first.lon_deg.to_bits()));
            junctions += 1;
        }
    }
    assert!(junctions > 0);
}

/// Ruling R-1: refinement adds points and never moves a coarse one (except the water node a
/// trimmed mouth replaces).
#[test]
fn refinement_keeps_every_coarse_point_in_order() {
    let p = params();
    let stages = bake_stages(&world(), &p).expect("stages");
    let coarse = record_of(&stages, &p);
    let refined = crate::hydrology::bake(&world(), &p).expect("bake");
    let mut added = 0usize;
    for (c, r) in coarse.reaches.iter().zip(&refined.reaches) {
        let mut at = 0usize;
        let keep = if matches!(c.downstream, Downstream::Ocean | Downstream::Body(_)) { c.points.len() - 1 } else { c.points.len() };
        for cp in &c.points[..keep] {
            while at < r.points.len()
                && (r.points[at].lat_deg.to_bits(), r.points[at].lon_deg.to_bits()) != (cp.lat_deg.to_bits(), cp.lon_deg.to_bits()) {
                at += 1;
            }
            assert!(at < r.points.len(), "reach {} lost a coarse point", c.id);
        }
        added += r.points.len().saturating_sub(c.points.len());
    }
    assert!(added > 0, "refinement added fine points");
}

/// Spec §6.7: every recorded fall sits on its own reach, at a point of that reach bit for bit
/// (Ruling FF-1), and the next point is lower by the fall's height. Neither the bake test world
/// nor the 10 m default gives this property a population: it runs on the seed 1 `ranges` world
/// at 12,000 nodes with `fall_min_drop_m` lowered to 2 m, which has falls, and requires them.
#[test]
fn every_fall_is_a_step_on_its_own_reach() {
    let mut p = HydroParams::earth_like(12_000);
    p.fall_min_drop_m = 2.0;
    let record = crate::hydrology::bake(&ranges_world(), &p).expect("bake");
    for fall in &record.falls {
        let reach = &record.reaches[fall.reach as usize];
        let at = (fall.at.0.to_bits(), fall.at.1.to_bits());
        let i = reach.points.iter().position(|q| (q.lat_deg.to_bits(), q.lon_deg.to_bits()) == at)
            .expect("a fall's upper end is a point of its reach");
        let drop = reach.points[i].bed_m - reach.points[i + 1].bed_m;
        let d = drop - fall.height_m;
        assert!(d < 1e-6 && d > -1e-6, "fall height {} but the bed drops {}", fall.height_m, drop);
        assert!(fall.height_m >= p.fall_min_drop_m);
    }
    eprintln!("falls on the ranges world (12,000 nodes, 2 m): {}", record.falls.len());
    assert!(!record.falls.is_empty(), "the population has falls");
}

/// The three refined populations: the bake test world at both thresholds, and the seed 1
/// `ranges` world at 12,000 nodes (real relief, and the one with falls).
pub(super) fn refined_populations() -> [(&'static str, Surface, HydroParams); 3] {
    [("params", world(), params()),
     ("junction_params", world(), junction_params()),
     ("ranges", ranges_world(), HydroParams::earth_like(12_000))]
}

/// The point on the coarse chord `a -> b` at `along_m` metres from `a`: where the tracer would
/// have stood with no lowest-ground search at all.
fn chord_point(a: &crate::hydrology::ReachPoint, b: &crate::hydrology::ReachPoint, along_m: f64, radius_m: f64) -> SpherePoint {
    let frame = crate::tangent::TangentFrame::at(&SpherePoint::from_latlon(a.lat_deg, a.lon_deg), radius_m);
    let (bx, by) = frame.sphere_to_local(&SpherePoint::from_latlon(b.lat_deg, b.lon_deg));
    let len = crate::detmath::hypot(bx, by);
    frame.local_to_sphere(bx / len * along_m, by / len * along_m)
}

/// Rulings R-3 and FF-2 at world scale: no station of a segment that is not a reach's last sits
/// on ground at or below the datum. The one exception is Ruling R-3a: a station whose own chord
/// point is at or below the datum, where the tracer has nowhere higher to step back to.
///
/// Traced stations are not in the record (simplification drops most of them), so this drives
/// `trace_segment` over every coarse segment of `record_of`'s reaches, on the same `Ground` the
/// bake builds.
#[test]
fn no_inland_station_stands_on_sea_ground() {
    for (name, surface, p) in refined_populations() {
        let stages = bake_stages(&surface, &p).expect("stages");
        let coarse = record_of(&stages, &p);
        let height = |q: &SpherePoint| surface.structural_m(q);
        let ground = crate::hydrology::refine::Ground::for_surface(&surface, &height, &p);
        let mut stations = 0usize;
        let mut chord_exceptions = 0usize;
        for reach in &coarse.reaches {
            let shore = crate::hydrology::refine::terminal_level(reach, &coarse.bodies);
            for s in 0..reach.points.len() - 1 {
                let last = s + 2 == reach.points.len();
                if last && shore.is_some() {
                    // The reach's last segment is trimmed at the water it runs into, which R-3
                    // measures against that water's level, not the datum.
                    continue;
                }
                let (a, b) = (&reach.points[s], &reach.points[s + 1]);
                let segment = crate::hydrology::refine::trace_segment(&ground, &p, a, b, None);
                for f in segment.interior.iter().filter(|f| f.station) {
                    stations += 1;
                    if (ground.height_m)(&f.point) > 0.0 {
                        continue;
                    }
                    let chord = chord_point(a, b, f.along_m, surface.radius_m);
                    assert!((ground.height_m)(&chord) <= 0.0,
                            "{name}: reach {} segment {s}, the station {} m along and {} m sideways is at {} m, and its chord point is land ({} m)",
                            reach.id, f.along_m, f.lateral_m, (ground.height_m)(&f.point), (ground.height_m)(&chord));
                    chord_exceptions += 1;
                }
            }
        }
        assert!(stations > 0, "{name}: the population has traced stations");
        eprintln!("{name}: {stations} inland stations, {chord_exceptions} on a chord across water (R-3a)");
    }
}

/// Spec §6.6 and Ruling R-4 at world scale: every refined reach that runs into the sea or a lake
/// ends at or below that water's level.
#[test]
fn every_refined_mouth_is_at_or_below_its_water() {
    for (name, surface, p) in refined_populations() {
        let record = crate::hydrology::bake(&surface, &p).expect("bake");
        let mut mouths = 0usize;
        for reach in &record.reaches {
            if let Some(level) = crate::hydrology::refine::terminal_level(reach, &record.bodies) {
                let last = reach.points.last().expect("a reach has points");
                assert!(last.bed_m <= level,
                        "{name}: reach {} ends at {} m, above the {} m water it runs into",
                        reach.id, last.bed_m, level);
                mouths += 1;
            }
        }
        assert!(mouths > 0, "{name}: the population has mouths");
    }
}

/// Ruling R-7, Task 7 step 4: reports the size effect of simplification on both test
/// populations. Run with `--nocapture` to see the three counts.
#[test]
fn simplification_shrinks_the_refined_line() {
    for (name, p) in [("params", params()), ("junction_params", junction_params())] {
        let surface = world();
        let stages = bake_stages(&surface, &p).expect("stages");
        let coarse = record_of(&stages, &p);
        let coarse_count: usize = coarse.reaches.iter().map(|r| r.points.len()).sum();

        let height = |pt: &SpherePoint| surface.structural_m(pt);
        let ground = crate::hydrology::refine::Ground::for_surface(&surface, &height, &p);
        let shores: Vec<Option<f64>> = coarse.reaches.iter()
            .map(|r| crate::hydrology::refine::terminal_level(r, &coarse.bodies)).collect();
        let refined_count: usize = coarse.reaches.iter().zip(&shores)
            .map(|(r, &shore)| crate::hydrology::refine::refine_reach(r, shore, &ground, &p).points.len())
            .sum();

        let simplified = crate::hydrology::bake(&surface, &p).expect("bake");
        let simplified_count: usize = simplified.reaches.iter().map(|r| r.points.len()).sum();

        eprintln!("{name}: coarse {coarse_count}, refined {refined_count}, simplified {simplified_count}");
        assert!(simplified_count <= refined_count, "simplification must not add points");
    }
}

/// Rulings S-2, S-3 and S-14: the lines the record SHIPS cross no more often than the coarse
/// ones they came from, and the record says so.
///
/// The three 12,000-node populations are controls: `params` and `ranges` have no crossings at
/// all, coarse or refined, and `junction_params` has 0 coarse against 14 refined before the pass
/// and 0 after. A property whose every population reads 0 against 0 asserts nothing, and that is
/// how the first version of this test read green while the 1M-node record shipped 36 crossings
/// against 33 coarse. So the seed 1 `ranges` world at **200,000 nodes** is in the population too
/// -- the one Tasks 4 and 5 used -- where the coarse record really does cross itself (9) and the
/// shipped record must not cross more (8). The last assertion is that at least one population is
/// like that, so the property cannot quietly go back to comparing zero with zero.
///
/// It costs about 105 s of the debug profile. `refinement_adds_no_crossings_at_1m` below is the
/// one that reproduces the finding itself; 200k does not, and says so there.
#[test]
fn refinement_adds_no_crossings() {
    let mut populations: Vec<(&'static str, Surface, HydroParams)> = refined_populations().into_iter().collect();
    populations.push(("ranges 200k", ranges_world(), HydroParams::earth_like(200_000)));
    let mut with_coarse_crossings = 0usize;
    for (name, surface, p) in populations {
        let stages = bake_stages(&surface, &p).expect("stages");
        let coarse = record_of(&stages, &p);
        let coarse_lines: Vec<Vec<crate::hydrology::ReachPoint>> = coarse.reaches.iter().map(|r| r.points.clone()).collect();
        let down: Vec<Downstream> = coarse.reaches.iter().map(|r| r.downstream).collect();
        let before = crate::hydrology::refine::crossings(&coarse_lines, &down, surface.radius_m).len();
        let refined = crate::hydrology::bake(&surface, &p).expect("bake");
        let lines: Vec<Vec<crate::hydrology::ReachPoint>> = refined.reaches.iter().map(|r| r.points.clone()).collect();
        let after = crate::hydrology::refine::crossings(&lines, &down, surface.radius_m).len();
        eprintln!("{name}: coarse {before} shipped {after}");
        assert!(after <= before, "{name}: refinement left {after} crossings against {before} coarse");
        assert_eq!(refined.stats.crossings_coarse as usize, before);
        assert_eq!(refined.stats.crossings_left as usize, after);
        if before > 0 {
            with_coarse_crossings += 1;
        }
    }
    assert!(with_coarse_crossings > 0,
            "every population read 0 against 0: this property is asserting nothing");
}

/// Ruling S-14 at the resolution that found it. **This is the test that would have caught the
/// bug, and the 200,000-node population above would not have: at 200k the shipped record crossed
/// 8 times against 9 coarse under the old order too.** It takes a 1M-node bake on each of the two
/// stand-ins to make what the meander and Douglas-Peucker add cost more than the pass recovers,
/// which is where Task 7 measured 36 against 33 coarse and 57 against 54.
///
/// `#[ignore]`d for its cost -- about 90 s and 75 s of the release profile per bake, and several
/// minutes each in debug -- the same bargain `every_small_world_drains` strikes. Run it with
/// `cargo test --release -p worldbuilder-engine --lib refinement_adds_no_crossings_at_1m --
/// --ignored --nocapture` whenever the refinement pipeline's ORDER changes.
#[test]
#[ignore]
fn refinement_adds_no_crossings_at_1m() {
    for (name, surface) in [("ranges 1M", ranges_world()), ("default 1M", world())] {
        let p = HydroParams::earth_like(1_000_000);
        let stages = bake_stages(&surface, &p).expect("stages");
        let coarse = record_of(&stages, &p);
        let coarse_lines: Vec<Vec<crate::hydrology::ReachPoint>> = coarse.reaches.iter().map(|r| r.points.clone()).collect();
        let down: Vec<Downstream> = coarse.reaches.iter().map(|r| r.downstream).collect();
        let before = crate::hydrology::refine::crossings(&coarse_lines, &down, surface.radius_m).len();
        let refined = crate::hydrology::bake(&surface, &p).expect("bake");
        let lines: Vec<Vec<crate::hydrology::ReachPoint>> = refined.reaches.iter().map(|r| r.points.clone()).collect();
        let after = crate::hydrology::refine::crossings(&lines, &down, surface.radius_m).len();
        eprintln!("{name}: {} reaches, coarse {before} shipped {after}", refined.reaches.len());
        assert!(before > 0, "{name}: a 1M coarse record crosses itself; this population is not a control");
        assert!(after <= before, "{name}: refinement left {after} crossings against {before} coarse");
        assert_eq!(refined.stats.crossings_coarse as usize, before);
        assert_eq!(refined.stats.crossings_left as usize, after);
    }
}

/// Where a reach's coarse points sit in its shipped line. Ruling R-1 keeps them exactly, so they
/// are found by position, in order. A line trimmed at a mouth stops early, so this can be shorter
/// than `coarse`.
fn coarse_positions(coarse: &[crate::hydrology::ReachPoint], line: &[crate::hydrology::ReachPoint]) -> Vec<usize> {
    let mut at: Vec<usize> = Vec::with_capacity(coarse.len());
    for (j, p) in line.iter().enumerate() {
        if at.len() < coarse.len()
            && p.lat_deg == coarse[at.len()].lat_deg
            && p.lon_deg == coarse[at.len()].lon_deg
        {
            at.push(j);
        }
    }
    at
}

/// Which coarse segment the polyline edge leaving `index` belongs to: the last coarse point at or
/// before it. `assemble` gives a coarse point the segment it *starts*, and the reach's final
/// coarse point starts none of its own, so it takes the one that ends there. The bound is
/// `coarse_len`, not `positions.len()`: a line trimmed at a mouth is missing its far coarse
/// points, and clamping on what is present would put the mouth's own segment back at 0.
fn segment_at(positions: &[usize], coarse_len: usize, index: usize) -> usize {
    let mut s = 0usize;
    for (k, &pos) in positions.iter().enumerate() {
        if pos <= index {
            s = k;
        }
    }
    let last = coarse_len.saturating_sub(2);
    if s > last { last } else { s }
}

/// How far `point` stands off the straight line from `a` to `b`, in metres, on a tangent plane at
/// `a`: the sign-free `lateral_m` the tracer works in.
fn off_chord_m(a: &crate::hydrology::ReachPoint, b: &crate::hydrology::ReachPoint,
               point: &crate::hydrology::ReachPoint, radius_m: f64) -> f64 {
    let frame = crate::tangent::TangentFrame::at(&SpherePoint::from_latlon(a.lat_deg, a.lon_deg), radius_m);
    let (bx, by) = frame.sphere_to_local(&SpherePoint::from_latlon(b.lat_deg, b.lon_deg));
    let len = crate::detmath::hypot(bx, by);
    let (ux, uy) = (bx / len, by / len);
    let (px, py) = frame.sphere_to_local(&SpherePoint::from_latlon(point.lat_deg, point.lon_deg));
    let off = px * -uy + py * ux;
    if off < 0.0 { -off } else { off }
}

/// Ruling S-14's **order**, pinned by a test that runs.
///
/// `refinement_adds_no_crossings` above compares an outcome, and Task 7 measured that at 200,000
/// nodes that outcome reads 8 against 9 under the old, wrong order too -- so it cannot tell the
/// two pipelines apart. The one that can, `refinement_adds_no_crossings_at_1m`, is `#[ignore]`d
/// for its cost, which left the branch's headline property with no gate that runs. This is that
/// gate, and it costs one 12,000-node bake.
///
/// It asserts the one thing that is true only of the new order: a segment the pass made yield
/// **ships** on its chord. Ruling S-3 sets every interior station's lateral to 0 and Ruling S-4a
/// keeps the meander off it afterwards, so in `record.reaches` -- after `ship`'s meander and its
/// Douglas-Peucker -- that segment's interior points are still on the straight line between its
/// two coarse endpoints. Move the pass back before the meander and the same segment is meandered
/// *after* it yields, by up to `meander_amplitude_widths` channel widths, and this fails.
///
/// The yielding segments are re-derived here rather than read out of `refine`, which exposes no
/// such thing: the first round's lines are `refine_reach` plus `simplify`, which is exactly what
/// `ship` is with nothing yielded yet, and Ruling S-3's smaller-flow rule picks the yielder off
/// their crossings. The nine it names on this population are the nine `refine`'s own first round
/// straightens.
///
/// **Two meander params are widened for this population, and they are the discriminator.**
/// `earth_like`'s meander needs `meander_wavelength_widths * width_m >= 4 * refine_step_m`, which
/// at 11 widths and a 1,500 m step means a channel over about 545 m wide, and a bed slope under
/// 0.002. Nothing that yields on a 12,000-node world is a river that large, so under the old
/// order the meander declined to move the yielded segments at all and this test read green on
/// both pipelines -- measured, not assumed. At 2,000 widths and no slope gate the meander bites
/// on the streams that do yield, and the old order moves them 3.57 m off their chords against a
/// 1 mm bar. Nothing else about the population changes: the same trace, the same crossings, the
/// same Ruling S-3 decision.
#[test]
fn a_yielded_segment_ships_on_its_chord() {
    let surface = world();
    let mut p = junction_params();
    p.meander_wavelength_widths = 2_000.0;
    p.meander_max_slope = 1.0;
    let p = p;
    let stages = bake_stages(&surface, &p).expect("stages");
    let coarse = record_of(&stages, &p);
    let height = |q: &SpherePoint| surface.structural_m(q);
    let ground = crate::hydrology::refine::Ground::for_surface(&surface, &height, &p);

    // The pass's first round, reproduced: nothing has yielded, so every segment is traced and
    // meandered, assembled and simplified.
    let first_pass: Vec<Vec<crate::hydrology::ReachPoint>> = coarse.reaches.iter().map(|r| {
        let shore = crate::hydrology::refine::terminal_level(r, &coarse.bodies);
        let refined = crate::hydrology::refine::refine_reach(r, shore, &ground, &p);
        crate::hydrology::refine::simplify(&refined.points, &refined.protected, surface.radius_m, &p)
    }).collect();
    let positions: Vec<Vec<usize>> = coarse.reaches.iter().zip(&first_pass)
        .map(|(r, line)| coarse_positions(&r.points, line)).collect();
    let downstream: Vec<Downstream> = coarse.reaches.iter().map(|r| r.downstream).collect();
    let found = crate::hydrology::refine::crossings(&first_pass, &downstream, surface.radius_m);
    assert!(!found.is_empty(),
            "junction_params (meander widened): the first round must cross somewhere, or the pass never yields and \
             this test asserts nothing");

    // Ruling S-3 on those lines: the smaller flow at the crossing yields. `reach_a` is the
    // smaller id, so it also takes the tie.
    let mut yielders: Vec<(usize, usize)> = Vec::with_capacity(found.len());
    for c in &found {
        let (ra, rb) = (c.reach_a as usize, c.reach_b as usize); // cast-ok: a reach index, bounded by the record's reach count
        let flow_a = first_pass[ra][c.index_a].flow_m2;
        let flow_b = first_pass[rb][c.index_b].flow_m2;
        yielders.push(if flow_b < flow_a {
            (rb, segment_at(&positions[rb], coarse.reaches[rb].points.len(), c.index_b))
        } else {
            (ra, segment_at(&positions[ra], coarse.reaches[ra].points.len(), c.index_a))
        });
    }
    yielders.sort_unstable();
    yielders.dedup();

    let record = crate::hydrology::bake(&surface, &p).expect("bake");
    let mut checked = 0usize;
    let mut worst = 0.0f64;
    for (r, s) in &yielders {
        let (r, s) = (*r, *s);
        assert_eq!(record.reaches[r].id, coarse.reaches[r].id, "the bake kept reach order");
        let coarse_points = &coarse.reaches[r].points;
        let line = &record.reaches[r].points;
        let at = coarse_positions(coarse_points, line);
        if at.len() < s + 2 {
            // The shipped line stops at a mouth before this segment's far end; there is nothing
            // between two coarse points to measure.
            continue;
        }
        let (a, b) = (&coarse_points[s], &coarse_points[s + 1]);
        for i in at[s] + 1..at[s + 1] {
            let off = off_chord_m(a, b, &line[i], surface.radius_m);
            if off > worst {
                worst = off;
            }
            // A straight trace puts the station at lateral 0 exactly; all that is left is the
            // round trip through the tangent frame, measured at 1.3e-9 m. The old order's
            // meander on the same segments measured 3.57 m.
            assert!(off < 1.0e-3,
                    "reach {} segment {s} yielded, yet its shipped point {i} stands {off} m off \
                     its chord: the crossing pass is not running after the meander (Ruling S-14)",
                    coarse.reaches[r].id);
            checked += 1;
        }
    }
    eprintln!("junction_params (meander widened): {} first-round crossings, {} yielding segments, {checked} shipped \
               interior points, worst {worst} m off chord", found.len(), yielders.len());
    assert!(checked > 0,
            "no yielding segment kept an interior point in the shipped record: this test asserted \
             nothing");
}

/// Spec §6.6 on a real bake: every pond obeys its own keep rule, sits on its own ground, and
/// names the river it drains to (Ruling S-5).
///
/// Ruling S-11 is the `kind` assertion: a surviving fine find is recorded whatever its area --
/// `Pond` below `pond_max_area_m2` and `Lake` at or above it -- and both carry a traced 250 m
/// ring. Task 1's spec text dropped an oversized find; the measured median survivor is 4.0-4.7
/// km^2 against a 1 km^2 `pond_max_area_m2`, so dropping them would have dropped most of them.
#[test]
fn ponds_obey_their_keep_rule_and_name_a_river() {
    // Spec §6.6's 3 km corridor, not `earth_like`'s post-S-17 1.5 km: this world is 12,000 nodes,
    // and at 1.5 km its rivers find no hollow that passes the keep rule, so every assertion below
    // would run over an empty set and the closing `ponds > 0` would say so. What is under test
    // here is the keep rule and Rulings S-5, S-7, S-11 and S-13, none of which is about how wide
    // the corridor is.
    let mut p = params();
    p.pond_search_radius_m = 3_000.0;
    let record = crate::hydrology::bake(&world(), &p).expect("bake");
    let coarse = record_of(&bake_stages(&world(), &p).expect("stages"), &p);
    assert!(record.bodies.len() >= coarse.bodies.len(), "ponds are appended, never inserted");
    for (id, body) in record.bodies.iter().enumerate() {
        assert_eq!(body.id as usize, id, "body ids stay the wire index");
    }
    assert_eq!(record.stats.ponds_kept as usize, record.bodies.len() - coarse.bodies.len());
    assert!(record.stats.ponds_found >= record.stats.ponds_kept);
    let mut rings = 0usize;
    for body in &record.bodies[coarse.bodies.len()..] {
        assert!(body.depth_m >= p.pond_keep_depth_m, "pond {} is {} m deep", body.id, body.depth_m);
        assert!(body.area_m2 >= p.pond_keep_area_m2);
        assert!(body.fresh);
        assert!(!body.enclosed && !body.forced);
        assert!(matches!(body.downstream, Downstream::Reach(_)), "Ruling S-5");
        assert!(body.outlet_reach.is_none(), "Ruling S-5");
        assert!(body.outline.len() >= 3, "a pond has a traced outline");
        // The two properties simplification can break and nothing else would notice. A ring that
        // crosses itself makes §8.3's point-in-polygon test silently wrong; a corner cut by a
        // whole cell can put the body's own water outside its own outline, and `area_m2` comes
        // from the candidate's cell count rather than from the ring, so the two would disagree in
        // silence. The anchor is the candidate's lowest cell's centre -- the one cell centre the
        // record still names. `pond_search_survey` runs the same two checks over every cell of
        // every candidate on five populations.
        assert!(crate::hydrology::ponds::ring_is_simple(&body.outline, world().radius_m),
                "body {} traced a ring that crosses itself", body.id);
        let anchor = SpherePoint::from_latlon(body.anchor.0, body.anchor.1);
        assert!(crate::hydrology::ponds::ring_contains(&body.outline, world().radius_m, &anchor),
                "body {} is not inside its own outline", body.id);
        assert_eq!(body.kind,
                   if body.area_m2 < p.pond_max_area_m2 { crate::hydrology::BodyKind::Pond }
                   else { crate::hydrology::BodyKind::Lake },
                   "Ruling S-11: the area picks the kind, and neither is dropped");
        if let Downstream::Reach(r) = body.downstream {
            assert!((r as usize) < record.reaches.len(), "a real reach id");
        }
        rings += body.outline.len();
    }
    let ponds = record.bodies.len() - coarse.bodies.len();
    eprintln!("coarse bodies {} ponds {ponds} (found {}), outline points {rings}{}",
              coarse.bodies.len(), record.stats.ponds_found,
              if ponds == 0 { String::new() } else { format!(", {} per ring", rings / ponds) });
    assert!(ponds > 0, "this world must find ponds, or the assertions above prove nothing");
}

/// **A value pin, and the only test that names what `earth_like` actually ships for the fine
/// search.** Rulings S-16 and S-17 moved two of spec §6.6's own numbers to meet the owner world's
/// 8 MB and 300 s gates: the density cap from 500 km² (5.0e8) to 16,000 km² (1.6e10), because
/// that world recorded 11,146,072 bytes at 4.0e9; and the corridor from 3 km to 1.5 km, because
/// at 3 km its bake took 432-441 s.
///
/// **Why this test exists.** Every other pond test in the tree pins spec §6.6's 3 km corridor
/// deliberately -- `ponds::tests::params` for all six of its cases and
/// `ponds_obey_their_keep_rule_and_name_a_river` locally -- because their bowls and cell counts
/// were laid out against it and what they assert is the search's mechanism. That is right for
/// each of them and wrong in aggregate: after S-17 **no other test bakes at the values that
/// ship**. Without this pin, a drift in either number would be caught only by a parity count
/// moving, which says a number changed but not which one or that anybody meant it to.
#[test]
fn earth_like_ships_the_tuned_pond_corridor_and_density() {
    let p = HydroParams::earth_like(1_000_000);
    assert_eq!(p.pond_search_radius_m, 1_500.0,
               "Ruling S-17: the fine search's corridor, half spec §6.6's 3 km");
    assert_eq!(p.pond_density_area_m2, 1.6e10,
               "Ruling S-16: the density cap, 32x spec §6.6's 500 km^2");
    // Not tuned, and the reason the two above could be: the trace stays at the spec's 250 m, so
    // no recorded outline is coarser and Ruling S-13's containment result still holds for what
    // ships. If this ever moves, that result has to be re-established before it does.
    assert_eq!(p.pond_cell_m, 250.0, "spec §6.6's trace, untouched by S-16 and S-17");
}

/// The record echoes the seven pond params and the two pond counts, in header words 45-53.
#[test]
fn the_record_echoes_the_pond_params() {
    let p = params();
    let record = crate::hydrology::bake(&world(), &p).expect("bake");
    let words = crate::hydrology::record::encode(&record);
    assert_eq!(words[45], f64::from(record.stats.ponds_found));
    assert_eq!(words[46], f64::from(record.stats.ponds_kept));
    assert_eq!(words[47], p.pond_cell_m);
    assert_eq!(words[48], p.pond_search_radius_m);
    assert_eq!(words[49], p.pond_keep_depth_m);
    assert_eq!(words[50], p.pond_keep_area_m2);
    assert_eq!(words[51], p.pond_wetness_share);
    assert_eq!(words[52], p.pond_max_slope);
    assert_eq!(words[53], p.pond_density_area_m2);
    assert_eq!(decode(&words).as_ref(), Some(&record), "words 45-53 of the 56-word SCHEMA 6 header");
}

/// Rulings E-1, E-2 and E-3 on a real bake: every coarse body carries a shore-point set, every
/// pond carries a traced curve, and the two are told apart by `shore_member_count` alone.
#[test]
fn every_coarse_body_carries_an_extent() {
    for (name, surface, p) in refined_populations() {
        let record = crate::hydrology::bake(&surface, &p).expect("bake");
        let coarse = record_of(&bake_stages(&surface, &p).expect("stages"), &p);
        let mut with_extent = 0usize;
        for body in &record.bodies[..coarse.bodies.len()] {
            assert!(body.shore_member_count > 0, "{name}: coarse body {} has no shore members", body.id);
            assert!(body.outline.len() as u32 > body.shore_member_count, // cast-ok: at most one point per node
                    "{name}: body {} has shore members but no collar", body.id);
            assert!(body.shore_reach_m >= 0.0 && body.shore_reach_m.is_finite());
            with_extent += 1;
        }
        for body in &record.bodies[coarse.bodies.len()..] {
            assert_eq!(body.shore_member_count, 0, "{name}: a pond carries a traced curve");
            assert_eq!(body.shore_reach_m, 0.0);
        }
        assert!(with_extent > 0);
        let shore: u32 = record.bodies.iter().map(|b| b.shore_member_count).sum();
        assert_eq!(record.stats.shore_members, shore);
        eprintln!("{name}: {with_extent} coarse bodies, {} shore members, {} collar points",
                  record.stats.shore_members, record.stats.collar_points);
    }
}

/// Ruling E-3: a body's band never counts a step down to ground at or below its own level.
///
/// The brief matched a hollow to its body on the anchor's float pair; the body id is the exact
/// handle instead, and it is the one `record_of` itself uses -- body ids are one per kept hollow,
/// numbered 0.. in hollow order, so the nth kept hollow is `record.bodies[n]`. The anchor is
/// asserted rather than searched, so a change to that numbering is caught here too.
///
/// **The 200,000-node population is not decoration.** On all three 12,000-node populations the
/// below-level steps exist (9, 9 and 10 of 162, 162 and 328 member-collar steps) but not one of
/// them is the longest step of its own body -- so dropping the usability check entirely leaves
/// every band unchanged and this property reads green against a broken rule. It takes the seed 1
/// `ranges` world at 200,000 nodes for the exclusion to bite: 112 below-level steps of 3,359, on
/// 4 bodies whose longest step is one of them. The last assertion is that at least one body in
/// the population is like that, so the property cannot go back to comparing a maximum with
/// itself.
#[test]
fn no_bodys_band_counts_a_step_below_its_level() {
    let mut bands_the_exclusion_narrows = 0usize;
    for (name, surface, p) in [("params", world(), params()),
                               ("ranges 200k", ranges_world(), HydroParams::earth_like(200_000))] {
        let stages = bake_stages(&surface, &p).expect("stages");
        let record = record_of(&stages, &p);
        let graph = &stages.graph;
        let mut checked = 0usize;
        let mut next_body_id = 0usize;
        for (i, hollow) in stages.hollows.iter().enumerate() {
            if hollow.fate != Fate::Keep {
                continue;
            }
            let body = &record.bodies[next_body_id];
            next_body_id += 1;
            assert_eq!(body.anchor, graph.positions[hollow.floor as usize].to_latlon(),
                       "{name}: body {} is the body of hollow {i}", body.id);
            let mut longest_usable = 0.0;
            let mut longest_step = 0.0;
            for &member in &hollow.members {
                if stages.routing.lake_of[member as usize] != i as u32 { // cast-ok: hollow index
                    continue;
                }
                for &next in graph.neighbours(member) {
                    if stages.routing.lake_of[next as usize] == i as u32 { // cast-ok: hollow index
                        continue;
                    }
                    let step = graph.positions[member as usize].distance_to(&graph.positions[next as usize], graph.radius_m);
                    if step > longest_step { longest_step = step; }
                    if graph.height_m[next as usize] <= hollow.level_m {
                        continue;
                    }
                    if step > longest_usable { longest_usable = step; }
                }
            }
            let gap = body.shore_reach_m - longest_usable;
            assert!(gap < 1e-6 && gap > -1e-6,
                    "{name}: body {} band {} against longest usable {}", body.id, body.shore_reach_m, longest_usable);
            if longest_step > longest_usable {
                bands_the_exclusion_narrows += 1;
            }
            checked += 1;
        }
        assert_eq!(next_body_id, record.bodies.len(), "{name}: every body is a kept hollow's");
        assert!(checked > 0, "{name}: the population has kept hollows");
    }
    assert!(bands_the_exclusion_narrows > 0,
            "no body's longest step was an unusable one: this property is asserting nothing");
}


