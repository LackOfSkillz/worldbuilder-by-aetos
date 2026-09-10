//! Where each drop goes once hollows are decided: lakes flat at their level, notches cut so the
//! drained hollows run out, and every other node down its steepest slope.

use crate::hydrology::flood::{flood, Flood, NO_NODE};
use crate::hydrology::hollows::{find_hollows, forced_nodes, judge, Fate, Hollow};
use crate::hydrology::landgraph::LandGraph;
use crate::hydrology::HydroParams;

pub const NO_LAKE: u32 = u32::MAX;

/// How much each step of a notch drops below the one before, so the bed strictly falls.
const NOTCH_GRADE_M: f64 = 0.01;

#[derive(Debug, Clone, PartialEq)]
pub struct NotchRoute {
    pub nodes: Vec<u32>,
    pub bed_m: Vec<f64>,
}

#[derive(Debug, Clone)]
pub struct Routing {
    pub surface_m: Vec<f64>,
    pub receiver: Vec<u32>,
    pub lake_of: Vec<u32>,
    pub parent: Vec<u32>,
    pub notches: Vec<NotchRoute>,
}

pub fn route(graph: &LandGraph, global: &Flood, hollows: &mut Vec<Hollow>, params: &HydroParams) -> Routing {
    let n = graph.len();
    let mut parent = global.parent.clone();

    // Rank of each node in the global flood's pop order, for picking each pocket's lake_entry.
    let mut global_rank = vec![u32::MAX; n];
    for (r, &node) in global.order.iter().enumerate() {
        global_rank[node as usize] = r as u32; // cast-ok: order length is bounded by the node count
    }
    let forced = forced_nodes(graph, params);

    // 1. Enclosed kept hollows: the ring above the datum drains into the basin, not over the rim,
    //    and each below-datum pocket inside becomes its own lake (Ruling W1).
    let enclosed: Vec<usize> = (0..hollows.len())
        .filter(|&h| hollows[h].enclosed && hollows[h].fate == Fate::Keep)
        .collect();
    for h in enclosed {
        let members = hollows[h].members.clone();

        // Sub-flood seeded from every submerged member, confined to H; shore nodes reached from
        // it get re-parented into the basin instead of over the rim.
        let seeds: Vec<(u32, f64)> = members
            .iter()
            .filter(|&&m| graph.height_m[m as usize] <= 0.0)
            .map(|&m| (m, 0.0))
            .collect();
        let inside = |node: u32| members.binary_search(&node).is_ok();
        let sub = flood(graph, &seeds, &inside);
        for &m in &members {
            if sub.reached[m as usize] && sub.parent[m as usize] != NO_NODE {
                parent[m as usize] = sub.parent[m as usize];
            }
        }

        // Nested hollows found in the sub-flood: a hollow that itself contains a submerged seed
        // member is one of the pockets below, not a real nested hollow (Ruling 1) - drop it.
        // The rest (shore-only pools above the datum) are judged as ordinary, non-enclosed
        // hollows.
        let mut nested = find_hollows(graph, &sub);
        nested.retain(|hollow| !hollow.members.iter().any(|&m| graph.height_m[m as usize] <= 0.0));
        for hollow in nested.iter_mut() {
            hollow.enclosed = false;
        }
        judge(&mut nested, graph, params);
        hollows.extend(nested);

        // Group the below-datum members into pockets, one per enclosed component id (Ruling 2).
        let mut keyed: Vec<(u32, u32)> = members
            .iter()
            .copied()
            .filter(|&m| graph.height_m[m as usize] <= 0.0)
            .map(|m| (graph.enclosed[m as usize], m))
            .collect();
        keyed.sort_unstable();

        let mut pockets: Vec<Hollow> = Vec::new();
        let mut i = 0;
        while i < keyed.len() {
            let component = keyed[i].0;
            let mut pocket_members = Vec::new();
            while i < keyed.len() && keyed[i].0 == component {
                pocket_members.push(keyed[i].1);
                i += 1;
            }
            pocket_members.sort_unstable();

            let mut floor = pocket_members[0];
            let mut entry = pocket_members[0];
            let mut area_m2 = 0.0;
            for &m in &pocket_members {
                area_m2 += graph.area_m2[m as usize];
                if graph.height_m[m as usize] < graph.height_m[floor as usize] {
                    floor = m;
                }
                if global_rank[m as usize] < global_rank[entry as usize] {
                    entry = m;
                }
            }
            let floor_m = graph.height_m[floor as usize];

            let raw_outlet = global.parent[entry as usize];
            let outlet = if raw_outlet == NO_NODE { entry } else { raw_outlet };

            let mut outlet_path = vec![entry];
            let mut cur = entry;
            while outlet_path.len() < graph.len() {
                let next = global.parent[cur as usize];
                if next == NO_NODE || graph.ocean[next as usize] {
                    break;
                }
                outlet_path.push(next);
                cur = next;
            }

            let is_forced = pocket_members.iter().any(|m| forced.binary_search(m).is_ok());

            pockets.push(Hollow {
                members: pocket_members,
                floor,
                floor_m,
                level_m: 0.0,
                depth_m: 0.0 - floor_m,
                area_m2,
                entry,
                outlet,
                enclosed: true,
                forced: is_forced,
                fate: Fate::Keep,
                lake_entry: entry,
                outlet_path,
            });
        }

        // In-lake flood per pocket: every pocket member reaches its own entry without leaving the
        // water (without it, a submerged node's parent can be a shore node whose own descent
        // points back into the lake - a cycle).
        for pocket in &pockets {
            let under = |node: u32| pocket.members.binary_search(&node).is_ok();
            let inner = flood(graph, &[(pocket.lake_entry, 0.0)], &under);
            for &m in &pocket.members {
                parent[m as usize] = inner.parent[m as usize];
            }
        }

        // Replace hollows[h] with the first pocket (lowest component id); append the rest.
        let mut pockets = pockets.into_iter();
        if let Some(first) = pockets.next() {
            hollows[h] = first;
        }
        hollows.extend(pockets);
    }

    // 2. Kept hollows stand flat at their level.
    let mut surface_m = graph.height_m.clone();
    let mut lake_of = vec![NO_LAKE; n];
    for (id, hollow) in hollows.iter().enumerate() {
        if hollow.fate != Fate::Keep {
            continue;
        }
        for &m in &hollow.members {
            let h = graph.height_m[m as usize];
            let under = if hollow.enclosed { h <= 0.0 } else { h < hollow.level_m };
            if under {
                lake_of[m as usize] = id as u32; // cast-ok: hollow index
                surface_m[m as usize] = hollow.level_m;
            }
        }
    }

    let mut routing = Routing { surface_m, receiver: vec![NO_NODE; n], lake_of, parent, notches: Vec::new() };

    // 3. Receivers.
    for node in 0..n {
        if graph.ocean[node] {
            continue;
        }
        routing.receiver[node] = if routing.lake_of[node] != NO_LAKE {
            routing.parent[node]
        } else {
            steepest(graph, &routing.surface_m, node as u32) // cast-ok: node index
        };
    }
    for hollow in hollows.iter() {
        if hollow.fate == Fate::Keep && !hollow.enclosed {
            routing.receiver[hollow.entry as usize] = hollow.outlet;
        }
    }
    for hollow in hollows.iter() {
        if hollow.fate == Fate::Keep && hollow.enclosed {
            set_sink(&mut routing, hollow);
        }
    }

    // 4. Local minima are cut out along their parent chain.
    for node in 0..n {
        let i = node;
        if graph.ocean[i] || routing.lake_of[i] != NO_LAKE || routing.receiver[i] != NO_NODE {
            continue;
        }
        let start_bed = routing.surface_m[i] - params.notch_fall_m;
        cut_route(&mut routing, graph, node as u32, start_bed); // cast-ok: node index
    }
    routing
}

/// The steepest strictly-lower neighbour on `surface`, ties to the lower index.
fn steepest(graph: &LandGraph, surface: &[f64], node: u32) -> u32 {
    let here = surface[node as usize];
    let mut best = NO_NODE;
    let mut best_drop = 0.0;
    for &next in graph.neighbours(node) {
        let drop = here - surface[next as usize];
        if drop <= 0.0 {
            continue;
        }
        let run = graph.positions[node as usize].distance_to(&graph.positions[next as usize], graph.radius_m);
        let slope = drop / run;
        if best == NO_NODE || slope > best_drop {
            best = next;
            best_drop = slope;
        }
    }
    best
}

/// Cut a channel from `start` along its parent chain so it strictly falls, until the ground is
/// already lower than the channel, a lake member is reached, or the sea is reached. A start node
/// that is ocean or already a lake member does nothing.
pub fn cut_route(routing: &mut Routing, graph: &LandGraph, start: u32, start_bed_m: f64) {
    if graph.ocean[start as usize] || routing.lake_of[start as usize] != NO_LAKE {
        return;
    }
    let mut nodes = vec![start];
    let mut beds = vec![start_bed_m];
    routing.surface_m[start as usize] = start_bed_m;
    let mut bed = start_bed_m;
    let mut here = start;
    loop {
        let next = routing.parent[here as usize];
        if next == NO_NODE {
            break;
        }
        routing.receiver[here as usize] = next;
        if routing.lake_of[next as usize] != NO_LAKE {
            // A lake member stops the cut here; its surface is never touched.
            break;
        }
        if graph.ocean[next as usize] || routing.surface_m[next as usize] < bed {
            break;
        }
        bed -= NOTCH_GRADE_M;
        routing.surface_m[next as usize] = bed;
        nodes.push(next);
        beds.push(bed);
        here = next;
    }
    routing.notches.push(NotchRoute { nodes, bed_m: beds });
}

/// A closed lake keeps its water: its entry drains nowhere.
pub fn set_sink(routing: &mut Routing, hollow: &Hollow) {
    routing.receiver[hollow.lake_entry as usize] = NO_NODE;
}

/// Cut an explicit path so it strictly falls from `start_bed_m`; `path[0]` keeps its surface. A
/// path shorter than 2 does nothing (no empty `NotchRoute`).
pub fn cut_path(routing: &mut Routing, graph: &LandGraph, path: &[u32], start_bed_m: f64) {
    if path.len() < 2 {
        return;
    }
    let mut nodes = Vec::new();
    let mut beds = Vec::new();
    let mut bed = start_bed_m;
    for k in 1..path.len() {
        let node = path[k];
        routing.receiver[path[k - 1] as usize] = node;
        if routing.lake_of[node as usize] != NO_LAKE {
            // A lake member stops the cut here; its surface is never touched.
            break;
        }
        if graph.ocean[node as usize] || routing.surface_m[node as usize] < bed {
            break;
        }
        routing.surface_m[node as usize] = bed;
        nodes.push(node);
        beds.push(bed);
        bed -= NOTCH_GRADE_M;
    }
    if let (Some(&last), Some(&cut)) = (path.last(), nodes.last()) {
        if last == cut {
            routing.receiver[last as usize] = routing.parent[last as usize];
        }
    }
    routing.notches.push(NotchRoute { nodes, bed_m: beds });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hydrology::flood::{flood, ocean_seeds};
    use crate::hydrology::hollows::{find_hollows, judge};
    use crate::hydrology::landgraph::LandGraph;
    use crate::hydrology::HydroParams;
    use crate::sphere::SpherePoint;

    fn line(heights: &[f64], area: f64) -> LandGraph {
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
                              vec![0.5; n])
    }

    fn routed(g: &LandGraph) -> (Vec<crate::hydrology::hollows::Hollow>, Routing) {
        let params = HydroParams::earth_like(0);
        let f = flood(g, &ocean_seeds(g), &|_| true);
        let mut hollows = find_hollows(g, &f);
        judge(&mut hollows, g, &params);
        let r = route(g, &f, &mut hollows, &params);
        (hollows, r)
    }

    /// Follows receivers from `node`; returns where it ends.
    fn terminus(r: &Routing, node: u32) -> u32 {
        let mut here = node;
        let mut steps = 0;
        while r.receiver[here as usize] != NO_NODE {
            here = r.receiver[here as usize];
            steps += 1;
            assert!(steps < 10_000, "a cycle");
        }
        here
    }

    #[test]
    fn a_notched_hollow_drains_and_its_route_only_falls() {
        let g = line(&[-50.0, 30.0, 26.0, 27.0, 28.0, 70.0], 2.0e6);
        let (_, r) = routed(&g);
        assert_eq!(terminus(&r, 2), 0, "the notched pit drains to the ocean");
        assert_eq!(r.notches.len(), 1);
        let beds = &r.notches[0].bed_m;
        assert!(beds.windows(2).all(|w| w[1] < w[0]), "a notch bed only falls: {beds:?}");
    }

    #[test]
    fn a_kept_lake_flows_out_through_its_outlet() {
        let g = line(&[-50.0, 40.0, 5.0, 12.0, 25.0, 70.0], 2.0e6);
        let (hollows, r) = routed(&g);
        assert_eq!(r.lake_of[2], 0);
        assert_eq!(r.surface_m[2], 40.0);
        assert_eq!(terminus(&r, 3), 0, "lake water leaves over the outlet and reaches the sea");
        assert_eq!(hollows[0].outlet, 1);
        assert!(r.notches.is_empty(), "a kept lake above the datum needs no cut");
    }

    #[test]
    fn an_enclosed_basin_is_a_sink_until_judged_fresh() {
        let g = line(&[-50.0, -40.0, -30.0, 39.0, -5.0, 10.0, 60.0], 1.0e6);
        let (_, r) = routed(&g);
        assert_eq!(r.surface_m[4], 0.0);
        assert_eq!(terminus(&r, 5), 4, "the shore above the datum drains into the basin");
    }

    #[test]
    fn seed_pockets_are_not_nested_hollows_and_nothing_loops() {
        let g = line(&[60.0, 10.0, -12.0, -10.0, 39.0, -30.0, -40.0, -50.0], 1.0e6);
        let (_, r) = routed(&g);
        for node in 0..g.len() as u32 { // cast-ok: node index
            if g.ocean[node as usize] {
                continue;
            }
            let end = terminus(&r, node);
            assert!(g.ocean[end as usize] || r.lake_of[end as usize] != NO_LAKE,
                    "node {node} ends at {end}, neither sea nor lake");
        }
    }

    #[test]
    fn every_below_datum_pocket_is_its_own_lake() {
        let g = line(&[60.0, -5.0, 5.0, -5.0, 39.0, -30.0, -40.0, -50.0], 1.0e6);
        let (hollows, r) = routed(&g);
        assert_ne!(r.lake_of[1], NO_LAKE, "node 1's pocket is a lake");
        assert_ne!(r.lake_of[3], NO_LAKE, "node 3's pocket is a lake");
        assert_ne!(r.lake_of[1], r.lake_of[3], "the two pockets are different lakes");
        for node in 0..g.len() as u32 { // cast-ok: node index
            if g.ocean[node as usize] {
                continue;
            }
            let end = terminus(&r, node);
            assert!(g.ocean[end as usize] || r.lake_of[end as usize] != NO_LAKE,
                    "node {node} ends at {end}, neither sea nor lake");
        }
        for hollow in hollows.iter().filter(|h| h.enclosed && h.fate == Fate::Keep) {
            assert_eq!(hollow.outlet_path[0], hollow.lake_entry,
                       "each pocket lake's outlet_path starts at its own lake_entry");
        }
    }

    #[test]
    fn a_notch_never_cuts_a_lake() {
        let g = line(&[-50.0, 25.0, 12.0, 30.0, 24.0, 70.0], 2.0e6);
        let (hollows, r) = routed(&g);
        for hollow in hollows.iter().filter(|h| h.fate == Fate::Keep) {
            for &m in &hollow.members {
                if r.lake_of[m as usize] != NO_LAKE {
                    assert_eq!(r.surface_m[m as usize], hollow.level_m,
                               "a notch must never cut through a kept lake member");
                }
            }
        }
    }

    #[test]
    fn the_enclosed_fixture_has_exactly_one_lake() {
        let g = line(&[-50.0, -40.0, -30.0, 39.0, -5.0, 10.0, 60.0], 1.0e6);
        let (hollows, _r) = routed(&g);
        let kept_enclosed: Vec<&crate::hydrology::hollows::Hollow> =
            hollows.iter().filter(|h| h.enclosed && h.fate == Fate::Keep).collect();
        assert_eq!(kept_enclosed.len(), 1, "exactly one enclosed lake");
        assert_eq!(kept_enclosed[0].lake_entry, 4);
        assert_eq!(kept_enclosed[0].outlet_path, vec![4, 3]);
    }

    #[test]
    fn on_a_real_world_every_land_node_ends_in_the_ocean_or_a_lake() {
        let surface = crate::surface::Surface::new(20_260_904, 6_371_000.0, 12, 0.29, None, None, None);
        let g = LandGraph::sample(&surface, 8_000, 400).expect("graph");
        let (_, r) = routed(&g);
        for node in 0..g.len() as u32 { // cast-ok: node index
            if g.ocean[node as usize] { continue; }
            let end = terminus(&r, node);
            assert!(g.ocean[end as usize] || r.lake_of[end as usize] != NO_LAKE,
                    "node {node} ends at {end}, neither sea nor lake");
        }
    }
}
