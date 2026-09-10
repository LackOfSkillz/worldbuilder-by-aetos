//! How much water passes each node, and which lakes it keeps full.

use crate::hydrology::flood::NO_NODE;
use crate::hydrology::hollows::{Fate, Hollow};
use crate::hydrology::landgraph::LandGraph;
use crate::hydrology::routing::{cut_path, set_sink, Routing};
use crate::hydrology::HydroParams;

#[derive(Debug, Clone, PartialEq)]
pub struct Closure {
    pub closed: Vec<bool>,
    pub salt_flat: Vec<bool>,
    pub fresh_enclosed: Vec<bool>,
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
    flow
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
    };

    // Enclosed basins first: they are sinks until their balance says otherwise.
    let flow = accumulate(graph, routing);
    for (id, hollow) in hollows.iter().enumerate() {
        if hollow.fate != Fate::Keep || !hollow.enclosed {
            continue;
        }
        let inflow = flow[hollow.lake_entry as usize];
        let loss = evaporation(graph, routing, id, hollow, params.evaporation_factor);
        if hollow.forced || inflow >= loss {
            closure.fresh_enclosed[id] = true;
            cut_path(routing, graph, &hollow.outlet_path, hollow.level_m - params.notch_fall_m);
        } else {
            closure.closed[id] = true;
            closure.salt_flat[id] = inflow < params.salt_flat_share * loss;
        }
    }

    // Then the rest, until closing one lake starves no other.
    let mut flow = accumulate(graph, routing);
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
