//! A body's extent: the shore points spec §8.3 decides "is this point in that lake?" from.
//!
//! Plan 1b-3's Task 1 ruled the shape by measurement (Candidate B, that plan's design note §5.3):
//! an unordered set of recorded points and a nearest-point test, because the ring the spec first
//! imagined cannot be built on a k-nearest graph — both walks leave members outside their own ring
//! — and a 250 m shoreline of the owner's great lake alone would cost 3.3 MB against a 1 MB budget.
//!
//! **What is recorded** (Rulings E-1 and E-2): every *shore* member, then every collar node, each
//! ascending by node index. A shore member is a member with a neighbour that is not a member of
//! the same body; a collar node is a non-member with a member neighbour. Interior members are not
//! recorded — the great lake has 37,844 members and 1,178 shore members — and the claim that
//! dropping them cannot move the extent's boundary is on trial in this plan's Task 3.
//!
//! **The band** (Ruling E-3): `shore_reach_m` is the longest *usable* member-to-collar step, where
//! usable means the collar end's landform stands above the body's level. The level contour crosses
//! each usable step somewhere along it, so a band that wide holds the contour inside the extent.
//! A step whose collar end is at or below the level carries no contour to hold — it is dry ground
//! downhill of a perched rim, measured at 1.0-2.7% of shore steps — and counting it would only
//! widen the band.

use crate::hydrology::landgraph::LandGraph;

#[derive(Debug, Clone, PartialEq)]
pub struct Extent {
    /// Ruling E-1: shore members first, then collar, each ascending by node index. The order
    /// carries no geometry; `shore_member_count` is the only thing that reads it.
    pub points: Vec<(f64, f64)>,
    pub shore_member_count: u32,
    /// Ruling E-3, in metres. Zero when no step is usable.
    pub shore_reach_m: f64,
}

/// The extent of the body whose hollow index is `hollow_index`. `members` is that hollow's member
/// list; only the nodes `lake_of` actually assigns to this body count, which is what excludes a
/// hollow's dry members above an enclosed basin's datum.
pub fn extent_of(graph: &LandGraph, lake_of: &[u32], hollow_index: u32, members: &[u32], level_m: f64) -> Extent {
    let mine = |node: u32| lake_of[node as usize] == hollow_index;
    let mut shore: Vec<u32> = Vec::new();
    let mut collar: Vec<u32> = Vec::new();
    let mut reach_m = 0.0;
    for &member in members {
        if !mine(member) {
            continue;
        }
        let mut is_shore = false;
        for &next in graph.neighbours(member) {
            if mine(next) {
                continue;
            }
            is_shore = true;
            collar.push(next);
            // Ruling E-3: only a step whose collar end stands above the level carries the contour.
            if graph.height_m[next as usize] > level_m {
                let step = graph.positions[member as usize]
                    .distance_to(&graph.positions[next as usize], graph.radius_m);
                if step > reach_m {
                    reach_m = step;
                }
            }
        }
        if is_shore {
            shore.push(member);
        }
    }
    shore.sort_unstable();
    shore.dedup();
    collar.sort_unstable();
    collar.dedup();
    let mut points = Vec::with_capacity(shore.len() + collar.len());
    for &node in shore.iter().chain(collar.iter()) {
        points.push(graph.positions[node as usize].to_latlon());
    }
    Extent {
        points,
        shore_member_count: shore.len() as u32, // cast-ok: at most one shore member per node
        shore_reach_m: reach_m,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hydrology::landgraph::LandGraph;
    use crate::hydrology::routing::NO_LAKE;
    use crate::sphere::SpherePoint;

    const R: f64 = 6_371_000.0;

    /// A chain of `n` nodes along the equator, one degree apart, each neighbouring the next.
    fn chain(heights: &[f64]) -> LandGraph {
        let n = heights.len();
        let positions = (0..n).map(|i| SpherePoint::from_latlon(0.0, i as f64)).collect();
        let directed: Vec<Vec<u32>> = (0..n)
            .map(|i| if i + 1 < n { vec![(i + 1) as u32] } else { Vec::new() }) // cast-ok: node index
            .collect();
        LandGraph::from_parts(R, positions, heights.to_vec(), vec![1.0e6; n], &directed, vec![0.5; n])
    }

    /// Nodes 2, 3 and 4 are a lake at level 10; 1 and 5 are its collar.
    fn lake_in_a_chain() -> (LandGraph, Vec<u32>, Vec<u32>) {
        let graph = chain(&[40.0, 30.0, 5.0, 2.0, 6.0, 30.0, 40.0]);
        let mut lake_of = vec![NO_LAKE; graph.len()];
        for m in [2u32, 3, 4] {
            lake_of[m as usize] = 0;
        }
        (graph, lake_of, vec![2, 3, 4])
    }

    #[test]
    fn the_extent_is_shore_members_then_collar() {
        let (graph, lake_of, members) = lake_in_a_chain();
        let e = extent_of(&graph, &lake_of, 0, &members, 10.0);
        // 2 and 4 touch a non-member; 3 is interior and is trimmed (Ruling E-2).
        assert_eq!(e.shore_member_count, 2);
        assert_eq!(e.points.len(), 4);
        let at = |i: usize| graph.positions[i].to_latlon();
        assert_eq!(e.points[0], at(2));
        assert_eq!(e.points[1], at(4));
        assert_eq!(e.points[2], at(1));
        assert_eq!(e.points[3], at(5));
    }

    #[test]
    fn shore_reach_is_the_longest_usable_step() {
        let (graph, lake_of, members) = lake_in_a_chain();
        let e = extent_of(&graph, &lake_of, 0, &members, 10.0);
        let one_degree = graph.positions[1].distance_to(&graph.positions[2], R);
        let gap = e.shore_reach_m - one_degree;
        assert!(gap < 1.0 && gap > -1.0, "shore_reach_m {} against one degree {}", e.shore_reach_m, one_degree);
    }

    #[test]
    fn a_collar_at_or_below_the_level_is_not_usable() {
        // Node 5 stands at 6 m, below the lake's level of 10, so its step is excluded (E-3);
        // node 1 at 30 m is usable, and it is the only one left.
        let graph = chain(&[40.0, 30.0, 5.0, 2.0, 5.0, 6.0, 40.0]);
        let mut lake_of = vec![NO_LAKE; graph.len()];
        for m in [2u32, 3, 4] {
            lake_of[m as usize] = 0;
        }
        let e = extent_of(&graph, &lake_of, 0, &[2, 3, 4], 10.0);
        assert_eq!(e.shore_member_count, 2, "both shore members are still recorded");
        assert_eq!(e.points.len(), 4, "the collar is still recorded, usable or not");
        let one_degree = graph.positions[1].distance_to(&graph.positions[2], R);
        let gap = e.shore_reach_m - one_degree;
        assert!(gap < 1.0 && gap > -1.0, "only node 1's step counts: {}", e.shore_reach_m);
    }

    #[test]
    fn a_body_with_no_usable_step_reaches_nowhere() {
        // Every collar node stands below the level: no usable edge, so the band is zero and the
        // extent is decided by the nearest-point clause alone.
        let graph = chain(&[1.0, 2.0, 5.0, 2.0, 5.0, 2.0, 1.0]);
        let mut lake_of = vec![NO_LAKE; graph.len()];
        for m in [2u32, 3, 4] {
            lake_of[m as usize] = 0;
        }
        let e = extent_of(&graph, &lake_of, 0, &[2, 3, 4], 10.0);
        assert_eq!(e.shore_reach_m, 0.0);
        assert_eq!(e.shore_member_count, 2);
    }

    #[test]
    fn the_extent_is_the_same_twice() {
        let (graph, lake_of, members) = lake_in_a_chain();
        assert_eq!(extent_of(&graph, &lake_of, 0, &members, 10.0),
                   extent_of(&graph, &lake_of, 0, &members, 10.0));
    }
}
