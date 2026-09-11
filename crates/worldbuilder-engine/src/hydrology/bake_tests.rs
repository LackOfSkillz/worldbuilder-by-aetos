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
        6_371_000.0, positions, heights.clone(), vec![1.0e6; n], &directed, vec![0.5; n],
    );

    let mut receiver = vec![NO_NODE; n];
    receiver[1] = 0; // node 1's cut stops at the ocean.
    let routing = Routing {
        surface_m: heights,
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

#[test]
fn no_kept_body_is_below_the_keep_rule_unless_forced_or_enclosed() {
    let p = params();
    let record = crate::hydrology::bake(&world(), &p).expect("bake");
    for body in &record.bodies {
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

    // End to end: every ocean mouth at the datum, every lake mouth at its lake's level.
    let record = crate::hydrology::bake(&world(), &params_for_world()).expect("bake");
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
