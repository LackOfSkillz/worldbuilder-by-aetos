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
        queue.push_tied(level, level, node);
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
            // Inside a filled hollow every node shares the same spill; break the tie by the
            // node's own ground height so the flood reaches the lowest ground first, and parent
            // chains follow valley floors instead of node index.
            queue.push_tied(spill, own, next);
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

    /// A 5x5 lattice, 4-neighbour adjacency, positions on a 0.1-degree patch near the equator.
    /// `(0,0)` is the only below-datum node (the ocean). `(0,1)` is a single rim node the ocean
    /// touches, at 31 m. Everything else is a 30 m flat, except a staircase of nodes running
    /// from the far corner `(4,4)` to `(1,1)` at 29 m -- a valley one metre below the flat it
    /// crosses. Once the flood reaches the rim, the whole flat (valley included) spills at the
    /// same 31 m level: a single filled basin where only the tie-break can choose the path.
    fn grid_with_a_diagonal_valley() -> LandGraph {
        let rows = 5usize;
        let cols = 5usize;
        let n = rows * cols;
        let idx = |r: usize, c: usize| -> u32 { (r * cols + c) as u32 }; // cast-ok: tiny fixture

        let positions: Vec<SpherePoint> = (0..n)
            .map(|i| {
                let r = i / cols;
                let c = i % cols;
                SpherePoint::from_latlon(r as f64 * 0.1, c as f64 * 0.1)
            })
            .collect();

        let mut height_m = vec![30.0; n];
        height_m[idx(0, 0) as usize] = -10.0;
        height_m[idx(0, 1) as usize] = 31.0;
        let valley = [(4, 4), (3, 4), (3, 3), (2, 3), (2, 2), (1, 2), (1, 1)];
        for &(r, c) in &valley {
            height_m[idx(r, c) as usize] = 29.0;
        }

        let mut directed: Vec<Vec<u32>> = vec![Vec::new(); n];
        for r in 0..rows {
            for c in 0..cols {
                let mut neighbours = Vec::new();
                if r > 0 { neighbours.push(idx(r - 1, c)); }
                if r + 1 < rows { neighbours.push(idx(r + 1, c)); }
                if c > 0 { neighbours.push(idx(r, c - 1)); }
                if c + 1 < cols { neighbours.push(idx(r, c + 1)); }
                directed[idx(r, c) as usize] = neighbours;
            }
        }
        // The ocean touches a single rim node, (0,1). Cut its edge to (1,0).
        let ocean = idx(0, 0) as usize;
        let south = idx(1, 0);
        directed[ocean].retain(|&x| x != south);
        directed[south as usize].retain(|&x| x != idx(0, 0));

        LandGraph::from_parts(6_371_000.0, positions, height_m, vec![1.0e6; n], &directed,
                              vec![0.5; n])
    }

    #[test]
    fn inside_a_flat_the_flood_follows_the_low_ground() {
        let g = grid_with_a_diagonal_valley();
        assert!(g.ocean[0], "the corner is the only below-datum node");

        let f = flood(&g, &ocean_seeds(&g), &|_| true);

        // The staircase valley, far corner (24) toward the rim (1), excluding the rim itself.
        let valley: [u32; 7] = [24, 19, 18, 13, 12, 7, 6];
        let rim = 1u32;

        let mut here = 24u32;
        let mut steps = 0;
        loop {
            let parent = f.parent[here as usize];
            steps += 1;
            assert!(steps < 25, "cycle in the parent chain");
            if parent == rim {
                break;
            }
            assert!(
                valley.contains(&parent),
                "expected the flood to follow the valley floor, found node {parent}"
            );
            here = parent;
        }
        assert_eq!(f.parent[rim as usize], 0, "the rim's parent is the ocean");
    }
}
