//! How much water passes each node, and which lakes it keeps full.

use crate::hydrology::flood::NO_NODE;
use crate::hydrology::hollows::{Fate, Hollow};
use crate::hydrology::landgraph::LandGraph;
use crate::hydrology::routing::{cut_path, set_sink, Routing, NO_LAKE};
use crate::hydrology::HydroParams;

#[derive(Debug, Clone, PartialEq)]
pub struct Closure {
    pub closed: Vec<bool>,
    pub salt_flat: Vec<bool>,
    pub fresh_enclosed: Vec<bool>,
    /// Ruling 12b-2: for a fresh enclosed basin, the index into `routing.notches` of the outlet
    /// cut `close_lakes` made for it, if `cut_path` actually pushed one. It pushes nothing for a
    /// path shorter than 2 nodes, or when it lowered no node. `None` for every hollow that never
    /// got an outlet notch here: it is not a fresh enclosed basin, or its cut lowered nothing.
    pub outlet_notch: Vec<Option<usize>>,
}

pub fn accumulate(graph: &LandGraph, routing: &Routing) -> Vec<f64> {
    let n = graph.len();
    let mut flow: Vec<f64> = (0..n)
        .map(|i| if graph.ocean[i] { 0.0 } else { graph.area_m2[i] * graph.wetness[i] })
        .collect();
    let mut upstream = vec![0u32; n];
    for i in 0..n {
        let r = routing.receiver[i];
        if r != NO_NODE {
            upstream[r as usize] += 1;
        }
    }
    let mut ready: Vec<u32> = (0..n).filter(|&i| upstream[i] == 0).map(|i| i as u32).collect(); // cast-ok: node index
    let mut head = 0;
    while head < ready.len() {
        let node = ready[head] as usize;
        head += 1;
        let r = routing.receiver[node];
        if r == NO_NODE {
            continue;
        }
        flow[r as usize] += flow[node];
        upstream[r as usize] -= 1;
        if upstream[r as usize] == 0 {
            ready.push(r);
        }
    }
    // Ruling C1-e: a receiver cycle never reaches `ready`, so its nodes' flow would be silently
    // dropped. `drainage_check` refuses such a routing in `bake()`; this fires in debug tests if
    // one ever reaches `accumulate` anyway.
    debug_assert!(head == n, "accumulate: {} of {n} nodes never became ready -- a receiver cycle", n - head);
    flow
}

/// Ruling C1-c: "everything drains", checked rather than assumed. Every non-ocean node's
/// receiver chain must end at an ocean node, or at a lake member whose receiver is `NO_NODE` (a
/// closed lake's sink), without revisiting a node. O(n): each node is walked once, three-colour
/// marked (unvisited, on the current walk, known to drain). `Err` carries the lowest-index node
/// whose chain fails -- nodes are started in index order and every earlier start already
/// drained -- so the answer is deterministic.
pub fn drainage_check(graph: &LandGraph, routing: &Routing) -> Result<(), u32> {
    const UNSEEN: u8 = 0;
    const ON_WALK: u8 = 1;
    const DRAINS: u8 = 2;
    let n = graph.len();
    let mut colour = vec![UNSEEN; n];
    let mut walk: Vec<usize> = Vec::new();
    for start in 0..n {
        if colour[start] != UNSEEN {
            continue;
        }
        walk.clear();
        let mut here = start;
        let good = loop {
            match colour[here] {
                DRAINS => break true,
                ON_WALK => break false,
                _ => {}
            }
            if graph.ocean[here] {
                colour[here] = DRAINS;
                break true;
            }
            colour[here] = ON_WALK;
            walk.push(here);
            let next = routing.receiver[here];
            if next == NO_NODE {
                break routing.lake_of[here] != NO_LAKE;
            }
            if next as usize >= n {
                break false;
            }
            here = next as usize;
        };
        if !good {
            return Err(start as u32); // cast-ok: node index, bounded by the node count
        }
        for &node in &walk {
            colour[node] = DRAINS;
        }
    }
    Ok(())
}

/// Mean wetness and total area of the members of pocket `id` that actually belong to it (a
/// pocket's `members` list is set at split time, but `lake_of` is the ground truth once
/// `route` has run).
fn evaporation(graph: &LandGraph, routing: &Routing, id: usize, hollow: &Hollow, factor: f64) -> f64 {
    let mut area = 0.0;
    let mut wet = 0.0;
    let mut count = 0.0;
    for &m in &hollow.members {
        if routing.lake_of[m as usize] == id as u32 { // cast-ok: hollow index
            area += graph.area_m2[m as usize];
            wet += graph.wetness[m as usize];
            count += 1.0;
        }
    }
    let mean = if count > 0.0 { wet / count } else { 0.0 };
    area * factor * (1.0 - mean)
}

/// Runs flow accumulation to a fixed point over the lakes, deciding which enclosed basins keep
/// a fresh outlet and which lakes (enclosed or not) evaporate faster than their catchment can
/// refill them and so close over their own water.
pub fn close_lakes(graph: &LandGraph, routing: &mut Routing, hollows: &[Hollow], params: &HydroParams) -> (Vec<f64>, Closure) {
    let count = hollows.len();
    let mut closure = Closure {
        closed: vec![false; count],
        salt_flat: vec![false; count],
        fresh_enclosed: vec![false; count],
        outlet_notch: vec![None; count],
    };

    // Enclosed basins first: they are sinks until their balance says otherwise. Ruling C1-b: a
    // pocket made fresh can pour its outlet into another pocket, whose inflow then grows -- so
    // the pockets are judged again after every pass that freshened one, until a pass freshens
    // none (at most `hollows.len() + 1` passes: each productive pass freshens at least one of
    // them). A pocket still not fresh at the end is closed, judged on its final inflow.
    let mut flow = accumulate(graph, routing);
    for _ in 0..=count {
        let mut freshened = false;
        for (id, hollow) in hollows.iter().enumerate() {
            if hollow.fate != Fate::Keep || !hollow.enclosed || closure.fresh_enclosed[id] {
                continue;
            }
            let inflow = flow[hollow.lake_entry as usize];
            let loss = evaporation(graph, routing, id, hollow, params.evaporation_factor);
            if hollow.forced || inflow >= loss {
                closure.fresh_enclosed[id] = true;
                freshened = true;
                // `cut_path` pushes at most one `NotchRoute`, and only if it lowered something
                // (see its own doc). Read the index after the call, so the record filter
                // (Ruling 12b-2) sees exactly the notch this outlet cut produced, or none.
                let before = routing.notches.len();
                cut_path(routing, graph, &hollow.outlet_path, hollow.level_m - params.notch_fall_m);
                closure.outlet_notch[id] = if routing.notches.len() > before { Some(before) } else { None };
            }
        }
        if !freshened {
            break;
        }
        flow = accumulate(graph, routing);
    }
    for (id, hollow) in hollows.iter().enumerate() {
        if hollow.fate != Fate::Keep || !hollow.enclosed || closure.fresh_enclosed[id] {
            continue;
        }
        let inflow = flow[hollow.lake_entry as usize];
        let loss = evaporation(graph, routing, id, hollow, params.evaporation_factor);
        closure.closed[id] = true;
        closure.salt_flat[id] = inflow < params.salt_flat_share * loss;
    }

    // Then the rest, until closing one lake starves no other. `flow` is already current: the
    // pocket loop above re-accumulated after its last cut.
    for _ in 0..=count {
        let mut changed = false;
        for (id, hollow) in hollows.iter().enumerate() {
            if hollow.fate != Fate::Keep || hollow.enclosed || hollow.forced || closure.closed[id] {
                continue;
            }
            let inflow = flow[hollow.lake_entry as usize];
            let loss = evaporation(graph, routing, id, hollow, params.evaporation_factor);
            if inflow < loss {
                closure.closed[id] = true;
                closure.salt_flat[id] = inflow < params.salt_flat_share * loss;
                set_sink(routing, hollow);
                changed = true;
            }
        }
        if !changed {
            break;
        }
        flow = accumulate(graph, routing);
    }
    (flow, closure)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hydrology::flood::{flood, ocean_seeds, NO_NODE};
    use crate::hydrology::hollows::{find_hollows, judge};
    use crate::hydrology::landgraph::LandGraph;
    use crate::hydrology::routing::route;
    use crate::hydrology::HydroParams;
    use crate::sphere::SpherePoint;

    fn line(heights: &[f64], area: f64, wetness: f64) -> LandGraph {
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
        LandGraph::from_parts(6_371_000.0, positions, heights.to_vec(), vec![area; n], &directed,
                              vec![wetness; n])
    }

    #[test]
    fn flow_adds_up_downhill_and_conserves_the_catchment() {
        // ocean, then land rising steadily: every land node drains to node 0
        let g = line(&[-10.0, 5.0, 10.0, 15.0, 20.0], 1.0e6, 0.5);
        let params = HydroParams::earth_like(0);
        let f = flood(&g, &ocean_seeds(&g), &|_| true);
        let mut hollows = find_hollows(&g, &f);
        judge(&mut hollows, &g, &params);
        let r = route(&g, &f, &mut hollows, &params);
        let flow = accumulate(&g, &r);
        assert_eq!(flow[4], 0.5e6);
        assert_eq!(flow[1], 2.0e6, "node 1 carries all four land nodes");
    }

    #[test]
    fn a_wet_enclosed_basin_is_fresh_and_cut_through_its_rim() {
        let g = line(&[-50.0, -40.0, -30.0, 39.0, -5.0, 10.0, 60.0, 80.0], 1.0e6, 0.9);
        let params = HydroParams::earth_like(0);
        let f = flood(&g, &ocean_seeds(&g), &|_| true);
        let mut hollows = find_hollows(&g, &f);
        judge(&mut hollows, &g, &params);
        let mut r = route(&g, &f, &mut hollows, &params);
        let (_, closure) = close_lakes(&g, &mut r, &hollows, &params);
        let id = hollows.iter().position(|h| h.enclosed).expect("one enclosed basin");
        assert!(closure.fresh_enclosed[id]);
        assert!(r.surface_m[3] < 0.0, "the 39 m ridge is cut below the datum");
        let mut here = 4u32;
        let mut steps = 0;
        while r.receiver[here as usize] != NO_NODE { here = r.receiver[here as usize]; steps += 1; assert!(steps < 100); }
        assert!(g.ocean[here as usize], "the fresh basin drains to the ocean");
    }

    #[test]
    fn a_dry_lake_closes_and_keeps_its_water() {
        let g = line(&[-50.0, 40.0, 5.0, 12.0, 25.0, 70.0], 2.0e6, 0.05);
        let params = HydroParams::earth_like(0);
        let f = flood(&g, &ocean_seeds(&g), &|_| true);
        let mut hollows = find_hollows(&g, &f);
        judge(&mut hollows, &g, &params);
        let mut r = route(&g, &f, &mut hollows, &params);
        let (_, closure) = close_lakes(&g, &mut r, &hollows, &params);
        assert!(closure.closed[0], "wetness 0.05 cannot keep a lake topped up");
        assert_eq!(r.receiver[hollows[0].lake_entry as usize], NO_NODE);
    }

    #[test]
    fn a_forced_enclosed_basin_is_fresh_however_dry() {
        let g = line(&[-50.0, -40.0, -30.0, 39.0, -5.0, 10.0, 60.0, 80.0], 1.0e6, 0.0);
        let mut params = HydroParams::earth_like(0);
        params.forced_outlets = vec![SpherePoint::from_latlon(0.0, 2.0)];
        let f = flood(&g, &ocean_seeds(&g), &|_| true);
        let mut hollows = find_hollows(&g, &f);
        judge(&mut hollows, &g, &params);
        let mut r = route(&g, &f, &mut hollows, &params);
        let (_, closure) = close_lakes(&g, &mut r, &hollows, &params);
        let id = hollows.iter().position(|h| h.enclosed).expect("one enclosed basin");
        assert!(closure.fresh_enclosed[id]);
    }

    /// Ruling C1-d, the repro: a fresh pocket (node 6) whose outlet path runs back over a kept
    /// shore lake (node 4, a nested hollow at 20 m) used to stop its cut at that lake, whose own
    /// exit led straight back toward the pocket -- receivers 4 -> 5 and 5 -> 4, and nodes 3-7
    /// never reached the ocean.
    #[test]
    fn a_fresh_pockets_outlet_never_closes_a_cycle_through_a_shore_lake() {
        let g = line(&[-50.0, -40.0, 39.0, 30.0, 5.0, 20.0, -5.0, 60.0], 1.0e6, 0.5);
        let params = HydroParams::earth_like(0);
        let f = flood(&g, &ocean_seeds(&g), &|_| true);
        let mut hollows = find_hollows(&g, &f);
        judge(&mut hollows, &g, &params);
        let mut r = route(&g, &f, &mut hollows, &params);
        let (_, closure) = close_lakes(&g, &mut r, &hollows, &params);
        let id = hollows.iter().position(|h| h.enclosed && h.fate == Fate::Keep).expect("the pocket");
        assert!(closure.fresh_enclosed[id], "sanity: the pocket is wet enough to be fresh");
        assert_eq!(drainage_check(&g, &r), Ok(()));
        for node in [3u32, 4, 5, 6, 7] {
            let mut here = node;
            let mut steps = 0;
            while r.receiver[here as usize] != NO_NODE {
                here = r.receiver[here as usize];
                steps += 1;
                assert!(steps < 100, "a cycle through node {node}");
            }
            assert!(g.ocean[here as usize], "node {node} reaches the ocean, ends at {here}");
        }
    }

    /// Ruling C1-b: a pocket fed by a pocket is judged again. Pocket B (node 3) is too dry on its
    /// own catchment (inflow 0.3 against a 0.8 loss, in units of one node's area); pocket A
    /// (node 5) is wet and fresh, and its outlet cut runs over the ridge into B. With A's water B
    /// is fresh too (2.1 against 0.8), so it must be cut out to the sea rather than closed as a
    /// salt lake that swallows A's river.
    #[test]
    fn a_pocket_fed_by_a_fresh_pocket_is_rejudged() {
        let heights = [-50.0, -40.0, 39.0, -5.0, 20.0, -5.0, 60.0];
        let wetness = vec![0.5, 0.5, 0.5, 0.2, 0.1, 0.9, 0.9];
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
        let g = LandGraph::from_parts(6_371_000.0, positions, heights.to_vec(), vec![1.0e6; n], &directed, wetness);
        let params = HydroParams::earth_like(0);
        let f = flood(&g, &ocean_seeds(&g), &|_| true);
        let mut hollows = find_hollows(&g, &f);
        judge(&mut hollows, &g, &params);
        let mut r = route(&g, &f, &mut hollows, &params);
        let (_, closure) = close_lakes(&g, &mut r, &hollows, &params);
        let pocket_b = hollows.iter().position(|h| h.enclosed && h.lake_entry == 3).expect("pocket B");
        let pocket_a = hollows.iter().position(|h| h.enclosed && h.lake_entry == 5).expect("pocket A");
        assert!(pocket_b < pocket_a, "sanity: B is judged first, before A's water reaches it");
        assert!(closure.fresh_enclosed[pocket_a], "A is wet enough on its own");
        assert!(closure.fresh_enclosed[pocket_b], "B is fresh once A's outlet pours into it");
        assert!(!closure.closed[pocket_b]);
        assert_eq!(drainage_check(&g, &r), Ok(()));
        let mut here = 6u32;
        let mut steps = 0;
        while r.receiver[here as usize] != NO_NODE { here = r.receiver[here as usize]; steps += 1; assert!(steps < 100); }
        assert!(g.ocean[here as usize], "A's catchment reaches the sea through B");
    }

    /// Routes a line fixture through the whole pipeline up to `close_lakes`, at `earth_like(0)`.
    fn closed(heights: &[f64]) -> (LandGraph, Vec<Hollow>, Routing, Closure) {
        let g = line(heights, 1.0e6, 0.5);
        let params = HydroParams::earth_like(0);
        let f = flood(&g, &ocean_seeds(&g), &|_| true);
        let mut hollows = find_hollows(&g, &f);
        judge(&mut hollows, &g, &params);
        let mut r = route(&g, &f, &mut hollows, &params);
        let (_, closure) = close_lakes(&g, &mut r, &hollows, &params);
        (g, hollows, r, closure)
    }

    /// Task 3, the review's first fixture: a cut that stopped on "lower ground" left that ground
    /// draining by steepest descent -- back up the flood tree into the cut -- and closed a cycle.
    #[test]
    fn a_cut_never_stops_on_ground_that_drains_back() {
        let (g, _, r, _) = closed(&[-50.0, -40.0, 39.0, 10.0, 0.02, 0.1, 0.3, 0.5, -5.0, 60.0]);
        assert_eq!(drainage_check(&g, &r), Ok(()));
    }

    /// Task 3, the review's second fixture: a notched shore hollow on a fresh pocket's outlet
    /// path must not close a cycle with the pocket's outlet cut.
    #[test]
    fn a_notched_shore_lake_cannot_close_a_cycle() {
        let (g, _, r, _) =
            closed(&[-50.0, -40.0, 39.0, 30.0, 0.05, 20.0, 14.0, 12.0, 10.0, 8.0, 6.0, 4.0, 2.0, -5.0, 60.0]);
        assert_eq!(drainage_check(&g, &r), Ok(()));
    }

    /// Task 3, the property sweep: seeds 1 to 24 at 12,000 nodes, each with and without the
    /// tectonic ranges, all through `bake_stages`, which refuses any routing that fails
    /// `drainage_check`. That is 48 bakes.
    ///
    /// Ignored by the ledger's sweep ruling. It took 384 s in a debug test build (and about 88 s
    /// in release) on the dev host, against a 60 s budget. Seeds 1 and 4242 stay in the default
    /// suite (`real_worlds_drain_everything_through_the_full_bake`). Run it with:
    /// `cargo test -p worldbuilder-engine --release --lib every_small_world_drains -- --ignored`
    #[test]
    #[ignore = "48 bakes, about 6 minutes in debug; run with --release -- --ignored"]
    fn every_small_world_drains() {
        let params = HydroParams::earth_like(12_000);
        let mut failures = Vec::new();
        for seed in 1..=24i64 {
            for tectonics in [None, Some(crate::tectonics::TectonicParams::ranges())] {
                let ranged = tectonics.is_some();
                let surface = crate::surface::Surface::new(seed, 6.371e6, 12, 0.35, None, None, tectonics);
                if let Err(e) = crate::hydrology::bake_stages(&surface, &params) {
                    failures.push((seed, ranged, e));
                }
            }
        }
        assert!(failures.is_empty(), "worlds that fail to drain (seed, ranges, error): {failures:?}");
    }

    /// Ruling C1-d, the mutation guard: a real routing that passes the check must fail it once
    /// two receivers are overwritten into a 2-cycle.
    #[test]
    fn the_drainage_check_catches_a_hand_made_cycle() {
        let surface = crate::surface::Surface::new(20_260_904, 6_371_000.0, 12, 0.29, None, None, None);
        let g = LandGraph::sample(&surface, 8_000, 400).expect("graph");
        let params = HydroParams::earth_like(0);
        let f = flood(&g, &ocean_seeds(&g), &|_| true);
        let mut hollows = find_hollows(&g, &f);
        judge(&mut hollows, &g, &params);
        let mut r = route(&g, &f, &mut hollows, &params);
        let _ = close_lakes(&g, &mut r, &hollows, &params);
        assert_eq!(drainage_check(&g, &r), Ok(()), "sanity: the unmutated routing drains");

        // The first land node with a land neighbour, both off every lake: a 2-cycle between them.
        let (a, b) = (0..g.len() as u32) // cast-ok: node index
            .filter(|&a| !g.ocean[a as usize] && r.lake_of[a as usize] == crate::hydrology::routing::NO_LAKE)
            .find_map(|a| {
                g.neighbours(a).iter().copied()
                    .find(|&b| !g.ocean[b as usize] && r.lake_of[b as usize] == crate::hydrology::routing::NO_LAKE)
                    .map(|b| (a, b))
            })
            .expect("two neighbouring land nodes");
        r.receiver[a as usize] = b;
        r.receiver[b as usize] = a;
        assert!(drainage_check(&g, &r).is_err(), "a 2-cycle between {a} and {b} must fail the check");
    }

    /// Task 4, Ruling 12b-5's carry-forward: a hollow too large to keep (`capped`) still holds a
    /// real inner basin. The outer basin (nodes 2-7, a 5 m floor under a 40 m rim, area 6.0e6 m^2
    /// against a 5.0e6 m^2 test cap) must drain, but the 15 m-deep pocket at node 4 (its own 25 m
    /// rim) inside it is deep and wide enough to be its own kept lake. It fails today: the single
    /// global flood never sees the pocket separately (it is fully submerged at the outer basin's
    /// 40 m level), so the whole basin is one hollow and the pocket is simply cut along with it.
    #[test]
    fn a_capped_basin_keeps_a_deep_inner_lake() {
        let g = line(&[-50.0, 40.0, 5.0, 25.0, 10.0, 25.0, 30.0, 35.0, 70.0], 1.0e6, 0.5);
        let mut params = HydroParams::earth_like(0);
        params.keep_max_area_m2 = 5.0e6;
        let f = flood(&g, &ocean_seeds(&g), &|_| true);
        let mut hollows = find_hollows(&g, &f);
        judge(&mut hollows, &g, &params);
        assert_eq!(hollows.len(), 1, "sanity: one hollow before routing splits it");
        assert_eq!(hollows[0].area_m2, 6.0e6, "sanity: over the test cap");
        let mut r = route(&g, &f, &mut hollows, &params);

        let outer = hollows.iter().find(|h| h.members.contains(&2)).expect("the outer basin");
        assert_eq!(outer.fate, Fate::Notch, "too large to keep, whatever the inner lake");

        let inner_id = hollows.iter().position(|h| h.members == vec![4]).expect("the inner pocket");
        let inner = &hollows[inner_id];
        assert_eq!(inner.fate, Fate::Keep, "deep and wide enough on its own");
        assert_eq!(r.lake_of[4], inner_id as u32, "the pocket floor is that lake's member"); // cast-ok: hollow index, bounded by hollows.len()
        assert_eq!(r.surface_m[4], inner.level_m, "the pocket floor stands at its lake's level");

        let (_, _closure) = close_lakes(&g, &mut r, &hollows, &params);
        assert_eq!(drainage_check(&g, &r), Ok(()));
    }

    /// Routes `g` through the whole pipeline up to `close_lakes`, at the given `params`.
    fn closed_with(g: &LandGraph, params: &HydroParams) -> (Vec<Hollow>, Routing) {
        let f = flood(g, &ocean_seeds(g), &|_| true);
        let mut hollows = find_hollows(g, &f);
        judge(&mut hollows, g, params);
        let mut r = route(g, &f, &mut hollows, params);
        let _ = close_lakes(g, &mut r, &hollows, params);
        (hollows, r)
    }

    /// Task 4 fix 1, the review's fixture A (Ruling 4-1): a capped shore hollow inside an
    /// enclosed basin. The enclosed step has already pointed its members down toward the pocket,
    /// so its escape chain must follow those parents. Walking the global flood's parents instead
    /// leads over the rim, and the chain it spares closes a 2-cycle with the members it re-points.
    #[test]
    fn a_capped_shore_hollow_walks_its_own_way_out() {
        let g = line(&[-50.0, 30.0, 10.0, 10.0, 20.0, -5.0], 1.0e6, 0.5);
        let mut params = HydroParams::earth_like(0);
        params.keep_max_area_m2 = 1.5e6;
        params.evaporation_factor = 3.0;
        let (_, r) = closed_with(&g, &params);
        assert_eq!(drainage_check(&g, &r), Ok(()));
    }

    /// Task 4 fix 1, the review's fixture B (Ruling 4-2): a 4-connected 6x3 grid where a capped
    /// basin keeps an inner lake that sits on a fresh pocket's outlet path. The pocket's cut stops
    /// at that lake, whose own way out leads back toward the pocket. C1-a must judge the capped
    /// step's inner hollows too, and notch that one.
    #[test]
    fn a_capped_basins_inner_lake_stays_off_a_pockets_outlet_path() {
        let (w, h) = (6usize, 3usize);
        let heights = [
            -50.0, 60.0, 60.0, 10.0, 60.0, 60.0,
            -50.0, 0.0, 40.0, 30.0, 60.0, 60.0,
            -50.0, 60.0, 10.0, 30.0, 50.0, 0.0,
        ];
        let n = w * h;
        let positions: Vec<SpherePoint> = (0..n)
            .map(|i| SpherePoint::from_latlon((i / w) as f64 * 0.5, (i % w) as f64 * 0.5))
            .collect();
        let directed: Vec<Vec<u32>> = (0..n)
            .map(|i| {
                let (row, col) = (i / w, i % w);
                let mut v = Vec::new();
                if row > 0 { v.push((i - w) as u32); } // cast-ok: tiny fixture
                if col > 0 { v.push((i - 1) as u32); } // cast-ok: tiny fixture
                if col + 1 < w { v.push((i + 1) as u32); } // cast-ok: tiny fixture
                if row + 1 < h { v.push((i + w) as u32); } // cast-ok: tiny fixture
                v
            })
            .collect();
        let g = LandGraph::from_parts(6_371_000.0, positions, heights.to_vec(), vec![1.0e6; n], &directed,
                                      vec![0.5; n]);
        let mut params = HydroParams::earth_like(0);
        params.keep_max_area_m2 = 1.5e6;
        params.evaporation_factor = 0.01;
        let (_, r) = closed_with(&g, &params);
        assert_eq!(drainage_check(&g, &r), Ok(()));
    }

    /// Task 4 fix 1: the capped basin (nodes 2-6, floor 5 m at node 5) leaves over the 40 m rim
    /// along 5 -> 4 -> 3 -> 2 -> 1. The inner hollow at node 3, deep and wide enough to keep on
    /// its own, sits on that way out, so it is notched and holds no water.
    #[test]
    fn a_lake_on_a_capped_basins_way_out_is_notched() {
        let g = line(&[-50.0, 40.0, 30.0, 10.0, 25.0, 5.0, 35.0, 70.0], 1.0e6, 0.5);
        let mut params = HydroParams::earth_like(0);
        params.keep_max_area_m2 = 4.0e6;
        let (hollows, r) = closed_with(&g, &params);
        let inner = hollows.iter().find(|h| h.members == vec![3]).expect("the inner hollow at node 3");
        assert_eq!(inner.fate, Fate::Notch, "on the escape chain");
        assert_eq!(r.lake_of[3], NO_LAKE);
        assert_eq!(drainage_check(&g, &r), Ok(()));
    }

    /// Task 4 fix 1: the same capped basin, wider. The inner hollow at node 3 is on the floor's
    /// way out and is notched; the one at node 7 is off it, and is kept as its own lake.
    #[test]
    fn a_capped_basin_keeps_the_lake_off_its_way_out() {
        let g = line(&[-50.0, 40.0, 30.0, 10.0, 25.0, 5.0, 25.0, 12.0, 28.0, 35.0, 70.0], 1.0e6, 0.5);
        let mut params = HydroParams::earth_like(0);
        params.keep_max_area_m2 = 7.0e6;
        let (hollows, r) = closed_with(&g, &params);
        let on_chain = hollows.iter().find(|h| h.members == vec![3]).expect("the inner hollow at node 3");
        assert_eq!(on_chain.fate, Fate::Notch, "on the escape chain");
        assert_eq!(r.lake_of[3], NO_LAKE);
        let off_id = hollows.iter().position(|h| h.members == vec![7]).expect("the inner hollow at node 7");
        assert_eq!(hollows[off_id].fate, Fate::Keep, "off the escape chain");
        assert_eq!(r.lake_of[7], off_id as u32, "node 7 is that lake's member"); // cast-ok: hollow index, bounded by hollows.len()
        assert_eq!(drainage_check(&g, &r), Ok(()));
    }

    /// Beyond the brief: a full pipeline run on a real sampled world must conserve the total
    /// wetness-weighted catchment between the ocean and any closed-lake sinks, and every flow
    /// value must be finite and non-negative.
    #[test]
    fn flow_on_a_real_world_is_finite_and_conserved() {
        let surface = crate::surface::Surface::new(20_260_904, 6_371_000.0, 12, 0.29, None, None, None);
        let g = LandGraph::sample(&surface, 8_000, 400).expect("graph");
        let params = HydroParams::earth_like(0);
        let f = flood(&g, &ocean_seeds(&g), &|_| true);
        let mut hollows = find_hollows(&g, &f);
        judge(&mut hollows, &g, &params);
        let mut r = route(&g, &f, &mut hollows, &params);
        let (flow, _closure) = close_lakes(&g, &mut r, &hollows, &params);

        for &v in &flow {
            assert!(v.is_finite() && v >= 0.0, "flow must be finite and non-negative: {v}");
        }

        let mut total_contribution = 0.0;
        for i in 0..g.len() {
            if !g.ocean[i] {
                total_contribution += g.area_m2[i] * g.wetness[i];
            }
        }

        let mut delivered = 0.0;
        for i in 0..g.len() {
            let recv = r.receiver[i];
            if recv != NO_NODE && g.ocean[recv as usize] {
                delivered += flow[i];
            }
        }
        for hollow in hollows.iter() {
            if hollow.fate == crate::hydrology::hollows::Fate::Keep
                && r.receiver[hollow.lake_entry as usize] == NO_NODE {
                delivered += flow[hollow.lake_entry as usize];
            }
        }

        let rel = (delivered - total_contribution).abs() / total_contribution;
        assert!(rel < 1e-9, "conservation failed: delivered {delivered}, expected {total_contribution}, rel {rel}");
    }
}
