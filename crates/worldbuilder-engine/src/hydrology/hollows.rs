//! A hollow is a connected set of nodes the flood had to raise to one flat level. It is kept as
//! a lake or pond, or notched so it drains (spec section 6.3).

use crate::hydrology::buckets::BucketIndex;
use crate::hydrology::flood::{Flood, NO_NODE};
use crate::hydrology::heap::sortable;
use crate::hydrology::landgraph::{LandGraph, NO_BASIN};
use crate::hydrology::HydroParams;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fate {
    Keep,
    Notch,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hollow {
    pub members: Vec<u32>,
    pub floor: u32,
    pub floor_m: f64,
    pub level_m: f64,
    pub depth_m: f64,
    pub area_m2: f64,
    pub entry: u32,
    pub outlet: u32,
    pub enclosed: bool,
    pub forced: bool,
    /// Set by `judge`: too large to keep, whatever its depth (Ruling 12b-5). `route` sub-floods
    /// a capped hollow from its floor so any real inner basin still gets judged as its own lake.
    pub capped: bool,
    /// SCHEMA 4, carry-forward I3: `false` from `find_hollows`; `route` sets this to `true` on
    /// every inner hollow its capped step appends (Ruling R-8's kin -- not itself echoed, but
    /// what `BakeStats::capped_inner` and `capped_inner_kept` count).
    pub inner_of_capped: bool,
    pub fate: Fate,
    /// Where lake water gathers to leave: the entry for a hollow above the datum; for an
    /// enclosed basin, the submerged node the flood's way in leads down to (set by `route`).
    pub lake_entry: u32,
    /// For an enclosed basin: lake entry, up the shore, over the rim and down to the sea -
    /// the channel cut if the basin proves fresh (set by `route`, cut by `close_lakes`).
    pub outlet_path: Vec<u32>,
}

pub fn find_hollows(graph: &LandGraph, flood: &Flood) -> Vec<Hollow> {
    let n = graph.len();
    let raised = |i: usize| flood.reached[i] && flood.spill_m[i] > graph.height_m[i];
    // Rank of each node in pop order, so "first reached" is a number.
    let mut rank = vec![u32::MAX; n];
    for (r, &node) in flood.order.iter().enumerate() {
        rank[node as usize] = r as u32; // cast-ok: pop order is bounded by the node count
    }
    let mut label = vec![u32::MAX; n];
    let mut hollows = Vec::new();
    let mut stack = Vec::new();
    for start in 0..n {
        if !raised(start) || label[start] != u32::MAX || graph.ocean[start] {
            continue;
        }
        let key = sortable(flood.spill_m[start]);
        let id = hollows.len() as u32; // cast-ok: at most one hollow per node
        let mut members = Vec::new();
        label[start] = id;
        stack.push(start as u32); // cast-ok: node index
        while let Some(node) = stack.pop() {
            members.push(node);
            for &next in graph.neighbours(node) {
                let i = next as usize;
                if label[i] == u32::MAX && raised(i) && !graph.ocean[i]
                    && sortable(flood.spill_m[i]) == key {
                    label[i] = id;
                    stack.push(next);
                }
            }
        }
        members.sort_unstable();
        let level_m = flood.spill_m[start];
        let mut floor = members[0];
        let mut entry = members[0];
        let mut area_m2 = 0.0;
        let mut enclosed = false;
        for &m in &members {
            let i = m as usize;
            area_m2 += graph.area_m2[i];
            let lower = graph.height_m[i] < graph.height_m[floor as usize];
            if lower {
                floor = m;
            }
            if rank[i] < rank[entry as usize] {
                entry = m;
            }
            if graph.enclosed[i] != NO_BASIN {
                enclosed = true;
            }
        }
        let floor_m = graph.height_m[floor as usize];
        let outlet = flood.parent[entry as usize];
        if enclosed {
            area_m2 = members.iter()
                .filter(|&&m| graph.height_m[m as usize] <= 0.0)
                .map(|&m| graph.area_m2[m as usize])
                .sum();
        }
        hollows.push(Hollow {
            members,
            floor,
            floor_m,
            level_m: if enclosed { 0.0 } else { level_m },
            depth_m: if enclosed { 0.0 - floor_m } else { level_m - floor_m },
            area_m2,
            entry,
            // `flood.parent[entry]` is `NO_NODE` only for a flood seed, and a hollow's entry is
            // never a seed (`find_hollows` only groups non-ocean nodes the flood had to raise --
            // seeds start at their own ground level and are never members of a raised hollow).
            // Falling back to `entry` itself is defensive, not reachable on that invariant.
            outlet: if outlet == NO_NODE { entry } else { outlet },
            enclosed,
            forced: false,
            capped: false,
            inner_of_capped: false,
            fate: Fate::Notch,
            lake_entry: entry,
            outlet_path: Vec::new(),
        });
    }
    hollows
}

/// The nearest node to each requested forced-outlet point, one entry per point and in request
/// order (`None` only when the graph has no nodes to match against). `forced_nodes` collapses
/// this same mapping to a sorted, deduplicated node list; Task 6's forced-outlet accounting
/// (`forced_requested`/`forced_matched`) needs it point by point, with duplicates and misses
/// still visible.
pub fn nearest_forced_nodes(graph: &LandGraph, params: &HydroParams) -> Vec<Option<u32>> {
    if params.forced_outlets.is_empty() {
        return Vec::new();
    }
    let spacing =
        crate::stream::nominal_spacing_m(graph.len() as u32, graph.radius_m); // cast-ok: node count fits in u32 by construction
    let mut index = BucketIndex::new(graph.radius_m, spacing);
    for (i, p) in graph.positions.iter().enumerate() {
        index.insert(p, i as u32); // cast-ok: node index
    }
    params.forced_outlets.iter().map(|point| index.nearest(point, &graph.positions)).collect()
}

/// Every node nearest a forced-outlet point, sorted and deduplicated.
pub fn forced_nodes(graph: &LandGraph, params: &HydroParams) -> Vec<u32> {
    let mut forced: Vec<u32> = nearest_forced_nodes(graph, params).into_iter().flatten().collect();
    forced.sort_unstable();
    forced.dedup();
    forced
}

pub fn judge(hollows: &mut [Hollow], forced: &[u32], params: &HydroParams) {
    for hollow in hollows.iter_mut() {
        hollow.forced = hollow.members.iter().any(|m| forced.binary_search(m).is_ok());
        let big = hollow.depth_m >= params.keep_depth_m && hollow.area_m2 >= params.keep_area_m2;
        // Ruling 12b-5: a hollow that is neither enclosed nor forced and whose area exceeds
        // `keep_max_area_m2` is notched regardless of depth -- a broad landform basin filled to
        // its rim is a drained lowland at graph scale, not an inland sea the size of several
        // Caspians. Enclosed basins are exempt: the coastline lock (Ruling W1) already keeps
        // them, whatever their size.
        let too_large = !hollow.enclosed && !hollow.forced && hollow.area_m2 > params.keep_max_area_m2;
        hollow.capped = too_large;
        hollow.fate = if too_large {
            Fate::Notch
        } else if hollow.enclosed || hollow.forced || big {
            Fate::Keep
        } else {
            Fate::Notch
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hydrology::flood::{flood, ocean_seeds};
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

    #[test]
    fn a_hollow_knows_its_floor_depth_area_and_outlet() {
        // ocean, then a 40 m ridge; behind it 5, 12, 25, then a 70 m wall. The only way out is
        // back over the 40 m ridge, so the hollow fills to 40.
        let g = line(&[-50.0, 40.0, 5.0, 12.0, 25.0, 70.0], 2.0e6);
        let f = flood(&g, &ocean_seeds(&g), &|_| true);
        let hollows = find_hollows(&g, &f);
        assert_eq!(hollows.len(), 1);
        let h = &hollows[0];
        assert_eq!(h.members, vec![2, 3, 4]);
        assert_eq!(h.floor, 2);
        assert_eq!(h.level_m, 40.0);
        assert_eq!(h.depth_m, 35.0);
        assert_eq!(h.area_m2, 6.0e6);
        assert_eq!(h.entry, 2);
        assert_eq!(h.outlet, 1, "it spills back over the 40 m ridge");
    }

    #[test]
    fn deep_wide_hollows_are_kept_and_shallow_ones_notched() {
        let deep = line(&[-50.0, 40.0, 5.0, 12.0, 25.0, 70.0], 2.0e6);
        let f = flood(&deep, &ocean_seeds(&deep), &|_| true);
        let mut hollows = find_hollows(&deep, &f);
        let params = HydroParams::earth_like(0);
        judge(&mut hollows, &forced_nodes(&deep, &params), &params);
        assert_eq!(hollows[0].fate, Fate::Keep);

        let shallow = line(&[-50.0, 30.0, 26.0, 27.0, 28.0, 70.0], 2.0e6);
        let f = flood(&shallow, &ocean_seeds(&shallow), &|_| true);
        let mut hollows = find_hollows(&shallow, &f);
        let params = HydroParams::earth_like(0);
        judge(&mut hollows, &forced_nodes(&shallow, &params), &params);
        assert_eq!(hollows[0].depth_m, 4.0);
        assert_eq!(hollows[0].fate, Fate::Notch, "4 m deep is under the 8 m rule");
    }

    #[test]
    fn an_enclosed_basin_is_always_kept_at_the_datum() {
        // big ocean on the left, a below-datum pocket at node 4 behind a 39 m ridge
        let g = line(&[-50.0, -40.0, -30.0, 39.0, -5.0, 10.0, 60.0], 1.0e6);
        let f = flood(&g, &ocean_seeds(&g), &|_| true);
        let mut hollows = find_hollows(&g, &f);
        let params = HydroParams::earth_like(0);
        judge(&mut hollows, &forced_nodes(&g, &params), &params);
        let enclosed: Vec<&Hollow> = hollows.iter().filter(|h| h.enclosed).collect();
        assert_eq!(enclosed.len(), 1);
        assert_eq!(enclosed[0].fate, Fate::Keep);
        assert_eq!(enclosed[0].level_m, 0.0, "Ruling W1: the shoreline does not move");
        assert_eq!(enclosed[0].outlet, 3, "its lowest way to the ocean is the 39 m ridge");
    }

    /// Ruling 12b-5: a hollow larger than `keep_max_area_m2` drains, whatever its depth --
    /// unless it is enclosed, in which case the coastline lock (Ruling W1) keeps it regardless.
    #[test]
    fn a_hollow_larger_than_the_caspian_drains() {
        // Deep and wide enough to pass `big` (the ordinary keep rule), but its 6.0e6 m^2 area
        // exceeds a test-scale `keep_max_area_m2` of 5.0e6 -- so the open hollow must drain.
        let g = line(&[-50.0, 40.0, 5.0, 12.0, 25.0, 70.0], 2.0e6);
        let f = flood(&g, &ocean_seeds(&g), &|_| true);
        let mut hollows = find_hollows(&g, &f);
        let mut params = HydroParams::earth_like(0);
        params.keep_max_area_m2 = 5.0e6;
        judge(&mut hollows, &forced_nodes(&g, &params), &params);
        assert_eq!(hollows[0].area_m2, 6.0e6, "sanity: this hollow is above the test cap");
        assert_eq!(hollows[0].fate, Fate::Notch, "a hollow this large is not an open lake, whatever its depth");

        // The same size, but enclosed -- exempt, per Ruling W1.
        let g = line(&[-50.0, -40.0, -30.0, 39.0, -5.0, 10.0, 60.0], 2.0e6);
        let f = flood(&g, &ocean_seeds(&g), &|_| true);
        let mut hollows = find_hollows(&g, &f);
        let mut params = HydroParams::earth_like(0);
        params.keep_max_area_m2 = 1.0;
        judge(&mut hollows, &forced_nodes(&g, &params), &params);
        let enclosed: Vec<&Hollow> = hollows.iter().filter(|h| h.enclosed).collect();
        assert_eq!(enclosed.len(), 1);
        assert_eq!(enclosed[0].fate, Fate::Keep, "an enclosed basin is kept however large");
    }

    #[test]
    fn a_forced_outlet_keeps_a_hollow_the_rule_would_notch() {
        let g = line(&[-50.0, 30.0, 26.0, 27.0, 28.0, 70.0], 2.0e6);
        let f = flood(&g, &ocean_seeds(&g), &|_| true);
        let mut hollows = find_hollows(&g, &f);
        let mut params = HydroParams::earth_like(0);
        params.forced_outlets = vec![SpherePoint::from_latlon(0.0, 1.0)];
        judge(&mut hollows, &forced_nodes(&g, &params), &params);
        assert!(hollows[0].forced);
        assert_eq!(hollows[0].fate, Fate::Keep);
    }
}
