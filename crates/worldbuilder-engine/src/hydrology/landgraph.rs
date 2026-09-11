//! The graph the water runs on: land nodes and their neighbours, on the landform.
//!
//! **The landform, not the texture.** Heights are `Surface::structural_m` - shelf, plates,
//! mountains and painted features - and never the detail noise. Sampled every sixty-odd
//! kilometres, detail noise is what made 1,361 "lakes" on the owner's world; water takes its
//! orders from the shape of the land.
//!
//! **Ruling W1 lives here.** Below-datum water joined to the largest body of it is the ocean;
//! every other below-datum component is an enclosed basin, numbered in node order, and becomes
//! a lake at the datum rather than sea.

use crate::hydrology::buckets::BucketIndex;
use crate::sphere::SpherePoint;
use crate::stream::sample_nodes;
use crate::surface::Surface;

pub const NO_BASIN: u32 = u32::MAX;

/// Resolution handed to `moisture_index`: the relief palette's coarse climate reads, where the
/// march saves about a quarter of its cost and loses nothing a river cares about.
const WETNESS_RESOLUTION_M: f64 = 20_000.0;

/// Salt that keeps the wetness sampling from landing on the same spiral as the graph.
const WETNESS_SEED_SALT: u64 = 0x5745_5454_4e45_5353;

#[derive(Debug, Clone)]
pub struct LandGraph {
    pub radius_m: f64,
    pub positions: Vec<SpherePoint>,
    pub height_m: Vec<f64>,
    pub area_m2: Vec<f64>,
    pub adj_start: Vec<u32>,
    pub adj: Vec<u32>,
    pub ocean: Vec<bool>,
    pub enclosed: Vec<u32>,
    pub enclosed_count: u32,
    pub wetness: Vec<f64>,
}

impl LandGraph {
    pub fn sample(surface: &Surface, total_nodes: u32, wetness_nodes: u32) -> Option<Self> {
        let radius_m = surface.radius_m;
        let seed = surface.world_seed as u64; // cast-ok: two's-complement reinterpretation, as Surface::new makes
        let sampling = sample_nodes(seed, total_nodes, radius_m)?;
        let height_m: Vec<f64> =
            sampling.positions.iter().map(|p| surface.structural_m(p)).collect();
        if height_m.iter().any(|h| !h.is_finite()) {
            return None;
        }
        let coarse = sample_nodes(seed ^ WETNESS_SEED_SALT, wetness_nodes, radius_m)?;
        let coarse_wetness: Vec<f64> = coarse
            .positions
            .iter()
            .map(|p| {
                let w = surface.moisture_index(p, Some(WETNESS_RESOLUTION_M), None);
                if w.is_finite() { w } else { 0.0 }
            })
            .collect();
        let mut index = BucketIndex::new(radius_m, crate::stream::nominal_spacing_m(wetness_nodes, radius_m));
        for (i, p) in coarse.positions.iter().enumerate() {
            index.insert(p, i as u32); // cast-ok: bounded by wetness_nodes, a u32
        }
        let wetness: Vec<f64> = sampling
            .positions
            .iter()
            .map(|p| match index.nearest(p, &coarse.positions) {
                Some(id) => coarse_wetness[id as usize],
                None => 0.0,
            })
            .collect();
        Some(Self::from_parts(radius_m, sampling.positions, height_m, sampling.area_m2,
                              &sampling.neighbours, wetness))
    }

    pub fn from_parts(
        radius_m: f64,
        positions: Vec<SpherePoint>,
        height_m: Vec<f64>,
        area_m2: Vec<f64>,
        directed: &[Vec<u32>],
        wetness: Vec<f64>,
    ) -> Self {
        let n = positions.len();
        let mut pairs: Vec<(u32, u32)> = Vec::new();
        for (a, list) in directed.iter().enumerate() {
            let a = a as u32; // cast-ok: node counts are bounded by stream::MAX_NODES
            for &b in list {
                if a != b {
                    pairs.push((a, b));
                    pairs.push((b, a));
                }
            }
        }
        pairs.sort_unstable();
        pairs.dedup();
        let mut adj_start = vec![0u32; n + 1];
        for &(a, _) in &pairs {
            adj_start[a as usize + 1] += 1;
        }
        for i in 0..n {
            adj_start[i + 1] += adj_start[i];
        }
        let adj: Vec<u32> = pairs.iter().map(|&(_, b)| b).collect();

        let mut graph = Self {
            radius_m, positions, height_m, area_m2, adj_start, adj,
            ocean: vec![false; n], enclosed: vec![NO_BASIN; n], enclosed_count: 0, wetness,
        };
        graph.label_water();
        graph
    }

    pub fn len(&self) -> usize {
        self.positions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }

    pub fn neighbours(&self, node: u32) -> &[u32] {
        let start = self.adj_start[node as usize] as usize;
        let end = self.adj_start[node as usize + 1] as usize;
        &self.adj[start..end]
    }

    /// Ruling W1: the largest below-datum component (by area, ties to the lower first node)
    /// is the ocean; the others are enclosed basins numbered in order of their first node.
    fn label_water(&mut self) {
        let n = self.len();
        let mut component = vec![u32::MAX; n];
        let mut areas: Vec<f64> = Vec::new();
        let mut stack: Vec<u32> = Vec::new();
        for start in 0..n {
            if self.height_m[start] > 0.0 || component[start] != u32::MAX {
                continue;
            }
            let id = areas.len() as u32; // cast-ok: at most one component per node
            let mut area = 0.0;
            component[start] = id;
            stack.push(start as u32); // cast-ok: node index
            while let Some(node) = stack.pop() {
                area += self.area_m2[node as usize];
                for &next in self.neighbours(node) {
                    if self.height_m[next as usize] <= 0.0 && component[next as usize] == u32::MAX {
                        component[next as usize] = id;
                        stack.push(next);
                    }
                }
            }
            areas.push(area);
        }
        if areas.is_empty() {
            return;
        }
        let mut ocean_id = 0u32;
        for (id, &area) in areas.iter().enumerate() {
            if area > areas[ocean_id as usize] {
                ocean_id = id as u32; // cast-ok: component index
            }
        }
        let mut renumber = vec![NO_BASIN; areas.len()];
        let mut next = 0u32;
        for (id, slot) in renumber.iter_mut().enumerate() {
            if id as u32 != ocean_id { // cast-ok: component index
                *slot = next;
                next += 1;
            }
        }
        for node in 0..n {
            let c = component[node];
            if c == u32::MAX {
                continue;
            }
            if c == ocean_id {
                self.ocean[node] = true;
            } else {
                self.enclosed[node] = renumber[c as usize];
            }
        }
        self.enclosed_count = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sphere::SpherePoint;

    /// A ring of 8 nodes around an equatorial strip: nodes 0-2 deep water (ocean-sized),
    /// node 5 a small enclosed below-datum pocket, the rest land.
    fn strip() -> LandGraph {
        let positions: Vec<SpherePoint> =
            (0..8).map(|i| SpherePoint::from_latlon(0.0, i as f64 * 1.0)).collect();
        let heights = vec![-100.0, -80.0, -60.0, 20.0, 30.0, -5.0, 25.0, 40.0];
        let areas = vec![10.0, 10.0, 10.0, 1.0, 1.0, 1.0, 1.0, 1.0];
        let directed: Vec<Vec<u32>> = (0..8u32)
            .map(|i| {
                let mut v = Vec::new();
                if i > 0 { v.push(i - 1); }
                if i < 7 { v.push(i + 1); }
                v
            })
            .collect();
        LandGraph::from_parts(6_371_000.0, positions, heights, areas, &directed, vec![0.5; 8])
    }

    #[test]
    fn adjacency_is_symmetric_sorted_and_self_free() {
        let g = strip();
        for node in 0..g.len() as u32 { // cast-ok: fixture of 8 nodes
            let n = g.neighbours(node);
            let mut sorted = n.to_vec();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(n, &sorted[..]);
            assert!(!n.contains(&node));
            for &other in n {
                assert!(g.neighbours(other).contains(&node));
            }
        }
    }

    #[test]
    fn the_largest_water_is_the_ocean_and_the_rest_is_enclosed() {
        let g = strip();
        assert_eq!(&g.ocean[..3], &[true, true, true]);
        assert!(!g.ocean[5]);
        assert_eq!(g.enclosed[5], 0);
        assert_eq!(g.enclosed_count, 1);
        for node in [3usize, 4, 6, 7] {
            assert!(!g.ocean[node]);
            assert_eq!(g.enclosed[node], NO_BASIN);
        }
    }

    #[test]
    fn a_sampled_world_has_land_ocean_and_wetness_in_range() {
        let surface = crate::surface::Surface::new(20_260_904, 6_371_000.0, 12, 0.29, None, None, None);
        let g = LandGraph::sample(&surface, 4_000, 400).expect("a land graph");
        assert_eq!(g.len(), 4_000);
        let land = g.ocean.iter().filter(|o| !**o).count();
        assert!(land > 400 && land < 3_600, "land nodes {land}");
        assert!(g.wetness.iter().all(|w| (0.0..=1.0).contains(w)));
        let again = LandGraph::sample(&surface, 4_000, 400).expect("a land graph");
        assert_eq!(g.height_m.iter().map(|h| h.to_bits()).collect::<Vec<_>>(),
                   again.height_m.iter().map(|h| h.to_bits()).collect::<Vec<_>>());
        assert_eq!(g.adj, again.adj);
    }
}
