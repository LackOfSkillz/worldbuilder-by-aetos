//! Priority flood (Barnes, Lehman and Mulla 2014): water rises from the seeds and every node
//! learns the level it would stand at, and the way it was reached.
//!
//! Always the lowest frontier node next, so a hollow is met from its lowest rim and filled to
//! exactly that level. One pass gives every land node a route to a seed: that is the whole of
//! "everything drains" at this stage, before any hollow is kept or notched.

use crate::hydrology::heap::FloodQueue;
use crate::hydrology::landgraph::LandGraph;

pub const NO_NODE: u32 = u32::MAX;

#[derive(Debug, Clone)]
pub struct Flood {
    pub spill_m: Vec<f64>,
    pub parent: Vec<u32>,
    pub order: Vec<u32>,
    pub reached: Vec<bool>,
}

/// Every ocean node, at the datum.
pub fn ocean_seeds(graph: &LandGraph) -> Vec<(u32, f64)> {
    (0..graph.len())
        .filter(|&n| graph.ocean[n])
        .map(|n| (n as u32, 0.0)) // cast-ok: node index
        .collect()
}

pub fn flood(graph: &LandGraph, seeds: &[(u32, f64)], allowed: &dyn Fn(u32) -> bool) -> Flood {
    let n = graph.len();
    let mut spill_m = graph.height_m.clone();
    let mut parent = vec![NO_NODE; n];
    let mut reached = vec![false; n];
    let mut order = Vec::new();
    let mut queue = FloodQueue::new();
    let mut is_seed = vec![false; n];
    for &(node, level) in seeds {
        is_seed[node as usize] = true;
        reached[node as usize] = true;
        spill_m[node as usize] = level;
        queue.push(level, node);
    }
    while let Some((level, node)) = queue.pop() {
        if !is_seed[node as usize] {
            order.push(node);
        }
        for &next in graph.neighbours(node) {
            let i = next as usize;
            if reached[i] || !allowed(next) {
                continue;
            }
            reached[i] = true;
            parent[i] = node;
            let own = graph.height_m[i];
            let spill = if own > level { own } else { level };
            spill_m[i] = spill;
            queue.push(spill, next);
        }
    }
    Flood { spill_m, parent, order, reached }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hydrology::landgraph::LandGraph;
    use crate::sphere::SpherePoint;

    /// A line: ocean, rim 50, pit 10, rim 30, land 60. The pit fills to 30 (its lower rim).
    fn line(heights: &[f64]) -> LandGraph {
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
        LandGraph::from_parts(6_371_000.0, positions, heights.to_vec(), vec![1.0e6; n], &directed,
                              vec![0.5; n])
    }

    #[test]
    fn a_pit_fills_to_its_lowest_way_out() {
        // Below-datum water at both ends; the right-hand pair is the larger, so it is the ocean
        // and node 0 is an enclosed pocket. The pit at node 2 can only reach the ocean over
        // node 3 (30 m), not over node 1 (50 m).
        let g = line(&[-10.0, 50.0, 10.0, 30.0, -20.0, -30.0]);
        assert!(g.ocean[4] && g.ocean[5] && !g.ocean[0]);
        let f = flood(&g, &ocean_seeds(&g), &|_| true);
        assert_eq!(f.spill_m[2], 30.0, "the pit spills over the 30 m rim");
        assert_eq!(f.spill_m[3], 30.0);
        assert_eq!(f.parent[2], 3, "the pit is reached over its lowest rim");
    }

    #[test]
    fn every_reached_node_leads_back_to_a_seed_through_non_increasing_spill() {
        let surface = crate::surface::Surface::new(20_260_904, 6_371_000.0, 12, 0.29, None, None, None);
        let g = LandGraph::sample(&surface, 6_000, 300).expect("graph");
        let f = flood(&g, &ocean_seeds(&g), &|_| true);
        for &node in &f.order {
            let mut here = node;
            let mut steps = 0;
            while f.parent[here as usize] != NO_NODE {
                let up = f.parent[here as usize];
                assert!(f.spill_m[up as usize] <= f.spill_m[here as usize]);
                here = up;
                steps += 1;
                assert!(steps < 6_000, "a cycle");
            }
            assert!(g.ocean[here as usize], "chain from {node} ends at a seed");
        }
    }

    #[test]
    fn the_flood_is_bit_identical_run_to_run() {
        let surface = crate::surface::Surface::new(20_260_904, 6_371_000.0, 12, 0.29, None, None, None);
        let g = LandGraph::sample(&surface, 6_000, 300).expect("graph");
        let a = flood(&g, &ocean_seeds(&g), &|_| true);
        let b = flood(&g, &ocean_seeds(&g), &|_| true);
        assert_eq!(a.order, b.order);
        assert_eq!(a.parent, b.parent);
        assert!(a.spill_m.iter().zip(&b.spill_m).all(|(x, y)| x.to_bits() == y.to_bits()));
    }

    #[test]
    fn allowed_confines_the_flood() {
        let g = line(&[-10.0, 5.0, 6.0, 7.0, 8.0, 9.0]);
        let f = flood(&g, &ocean_seeds(&g), &|n| n <= 2);
        assert!(f.reached[2]);
        assert!(!f.reached[3]);
    }
}
