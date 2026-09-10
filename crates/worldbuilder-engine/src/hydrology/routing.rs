//! Where each drop goes once hollows are decided: lakes flat at their level, notches cut so the
//! drained hollows run out, and every other node down its steepest slope.

use crate::hydrology::flood::{flood, Flood, NO_NODE};
use crate::hydrology::hollows::{find_hollows, judge, Fate, Hollow};
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

    // 1. Enclosed kept hollows: the ring above the datum drains into the basin, not over the rim.
    let enclosed: Vec<usize> = (0..hollows.len())
        .filter(|&h| hollows[h].enclosed && hollows[h].fate == Fate::Keep)
        .collect();
    for h in enclosed {
        let members = hollows[h].members.clone();
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
        // Where the flood's way in reaches the water: the lake's own entry.
        let entry = hollows[h].entry;
        let mut lake_entry = entry;
        let mut guard = 0usize;
        while graph.height_m[lake_entry as usize] > 0.0 && sub.parent[lake_entry as usize] != NO_NODE
            && guard <= members.len() {
            lake_entry = sub.parent[lake_entry as usize];
            guard += 1;
        }
        // In-lake routing: every submerged member reaches the entry without leaving the water.
        let submerged: Vec<u32> = members.iter().copied()
            .filter(|&m| graph.height_m[m as usize] <= 0.0).collect();
        let under = |node: u32| submerged.binary_search(&node).is_ok();
        let inner = flood(graph, &[(lake_entry, 0.0)], &under);
        for &m in &submerged {
            parent[m as usize] = inner.parent[m as usize];
        }
        // The way out, should the basin prove fresh: entry, up the shore, over the rim, down.
        let mut ring = vec![entry];
        let mut up = entry;
        while up != lake_entry && sub.parent[up as usize] != NO_NODE && ring.len() <= members.len() {
            up = sub.parent[up as usize];
            ring.push(up);
        }
        ring.reverse();
        let mut path = ring;
        let mut down = hollows[h].outlet;
        while down != NO_NODE && !graph.ocean[down as usize] && path.len() <= graph.len() {
            path.push(down);
            down = global.parent[down as usize];
        }
        hollows[h].lake_entry = lake_entry;
        hollows[h].outlet_path = path;
        let mut nested = find_hollows(graph, &sub);
        for hollow in nested.iter_mut() {
            hollow.enclosed = false;
        }
        judge(&mut nested, graph, params);
        hollows.extend(nested);
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
/// already lower than the channel or the sea is reached.
pub fn cut_route(routing: &mut Routing, graph: &LandGraph, start: u32, start_bed_m: f64) {
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

/// Cut an explicit path so it strictly falls from `start_bed_m`; `path[0]` keeps its surface.
pub fn cut_path(routing: &mut Routing, graph: &LandGraph, path: &[u32], start_bed_m: f64) {
    let mut nodes = Vec::new();
    let mut beds = Vec::new();
    let mut bed = start_bed_m;
    for k in 1..path.len() {
        let node = path[k];
        routing.receiver[path[k - 1] as usize] = node;
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
