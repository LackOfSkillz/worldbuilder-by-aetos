//! The between-reach crossing check the refinement pass repeats: which pairs of shipped polyline
//! segments intersect, in a deterministic order.

use crate::hydrology::buckets::BucketIndex;
use crate::hydrology::{Downstream, ReachPoint};
use crate::sphere::SpherePoint;
use crate::tangent::TangentFrame;

/// Two refined segments of different reaches that intersect. `index_a` and `index_b` are the
/// first point of each crossing segment in its own reach's list, and `reach_a < reach_b`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Crossing {
    pub reach_a: u32,
    pub index_a: usize,
    pub reach_b: u32,
    pub index_b: usize,
}

/// Do the segments `p0 -> p1` and `q0 -> q1` cross, measured on a tangent plane at `p0`? Shared
/// endpoints and touching ends count as no crossing: a junction is a shared vertex by Ruling R-1,
/// and two lines that merely meet do not need straightening.
fn segments_cross(radius_m: f64, p0: &SpherePoint, p1: &SpherePoint, q0: &SpherePoint, q1: &SpherePoint) -> bool {
    let frame = TangentFrame::at(p0, radius_m);
    let (ax, ay) = (0.0, 0.0);
    let (bx, by) = frame.sphere_to_local(p1);
    let (cx, cy) = frame.sphere_to_local(q0);
    let (dx, dy) = frame.sphere_to_local(q1);
    let side = |x0: f64, y0: f64, x1: f64, y1: f64, x: f64, y: f64| {
        (x1 - x0) * (y - y0) - (y1 - y0) * (x - x0)
    };
    let d1 = side(ax, ay, bx, by, cx, cy);
    let d2 = side(ax, ay, bx, by, dx, dy);
    let d3 = side(cx, cy, dx, dy, ax, ay);
    let d4 = side(cx, cy, dx, dy, bx, by);
    // Strictly opposite sides on both tests. A zero is a touch, not a crossing.
    ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
}

/// Every crossing between segments of different reaches, in a deterministic order. A confluence
/// pair is skipped: a reach and its receiver, or two reaches with the same receiver, share a
/// vertex by design (spec §14.4), and nothing there needs straightening.
///
/// The index is a `BucketIndex` over segment midpoints, at a cell of one refinement step, so the
/// work is proportional to the segments, not to their square.
pub fn crossings(lines: &[Vec<ReachPoint>], downstream: &[Downstream], radius_m: f64) -> Vec<Crossing> {
    // Segment id -> (reach index, point index), and the midpoint that indexes it.
    let mut owner: Vec<(u32, usize)> = Vec::new();
    let mut mid: Vec<SpherePoint> = Vec::new();
    let mut ends: Vec<(SpherePoint, SpherePoint)> = Vec::new();
    let mut longest_m = 0.0;
    for (r, points) in lines.iter().enumerate() {
        for i in 0..points.len().saturating_sub(1) {
            let a = SpherePoint::from_latlon(points[i].lat_deg, points[i].lon_deg);
            let b = SpherePoint::from_latlon(points[i + 1].lat_deg, points[i + 1].lon_deg);
            let span = a.distance_to(&b, radius_m);
            if span > longest_m {
                longest_m = span;
            }
            let frame = TangentFrame::at(&a, radius_m);
            let (bx, by) = frame.sphere_to_local(&b);
            owner.push((r as u32, i)); // cast-ok: reach index, bounded by the reach count
            mid.push(frame.local_to_sphere(bx * 0.5, by * 0.5));
            ends.push((a, b));
        }
    }
    if mid.is_empty() {
        return Vec::new();
    }
    let cell_m = if longest_m > 1.0 { longest_m } else { 1.0 };
    let mut index = BucketIndex::new(radius_m, cell_m);
    for (id, point) in mid.iter().enumerate() {
        index.insert(point, id as u32); // cast-ok: segment index, bounded by the point count
    }
    let related = |a: usize, b: usize| -> bool {
        let (ra, rb) = (owner[a].0 as usize, owner[b].0 as usize);
        matches!(downstream[ra], Downstream::Reach(next) if next as usize == rb)
            || matches!(downstream[rb], Downstream::Reach(next) if next as usize == ra)
            || match (downstream[ra], downstream[rb]) {
                (Downstream::Reach(x), Downstream::Reach(y)) => x == y,
                _ => false,
            }
    };
    let mut found = Vec::new();
    for a in 0..mid.len() {
        for b in index.candidates(&mid[a], cell_m) {
            let b = b as usize;
            if b <= a {
                continue;
            }
            if owner[a].0 == owner[b].0 || related(a, b) {
                continue;
            }
            if !segments_cross(radius_m, &ends[a].0, &ends[a].1, &ends[b].0, &ends[b].1) {
                continue;
            }
            let (first, second) = if owner[a].0 < owner[b].0 { (a, b) } else { (b, a) };
            found.push(Crossing {
                reach_a: owner[first].0,
                index_a: owner[first].1,
                reach_b: owner[second].0,
                index_b: owner[second].1,
            });
        }
    }
    found.sort_unstable_by_key(|c| (c.reach_a, c.index_a, c.reach_b, c.index_b));
    found.dedup();
    found
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;

    fn line(points: &[(f64, f64)]) -> Vec<ReachPoint> {
        points.iter().map(|&(lat, lon)| point(lat, lon, 0.0)).collect()
    }

    #[test]
    fn two_lines_that_cross_are_found() {
        // An X: one line west to east, one south to north, crossing near (0, 0.1).
        let a = line(&[(-0.2, 0.0), (0.2, 0.2)]);
        let b = line(&[(0.2, 0.0), (-0.2, 0.2)]);
        let found = crossings(&[a, b], &[Downstream::Ocean, Downstream::Ocean], R);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].reach_a, found[0].index_a, found[0].reach_b, found[0].index_b), (0, 0, 1, 0));
    }

    #[test]
    fn lines_that_only_come_close_are_not_a_crossing() {
        let a = line(&[(0.0, 0.0), (0.0, 0.2)]);
        let b = line(&[(0.001, 0.0), (0.001, 0.2)]);
        assert!(crossings(&[a, b], &[Downstream::Ocean, Downstream::Ocean], R).is_empty());
    }

    #[test]
    fn a_tributary_meeting_its_receiver_is_not_a_crossing() {
        // b ends on a's first point, which is what a junction is (Ruling R-1).
        let a = line(&[(0.0, 0.0), (0.0, 0.2)]);
        let b = line(&[(-0.2, -0.2), (0.0, 0.0)]);
        let found = crossings(&[a, b], &[Downstream::Ocean, Downstream::Reach(0)], R);
        assert!(found.is_empty());
    }

    #[test]
    fn two_tributaries_of_one_receiver_are_not_a_crossing_at_their_junction() {
        let a = line(&[(0.0, 0.0), (0.0, 0.2)]);
        let b = line(&[(-0.2, -0.2), (0.0, 0.0)]);
        let c = line(&[(0.2, -0.2), (0.0, 0.0)]);
        let down = [Downstream::Ocean, Downstream::Reach(0), Downstream::Reach(0)];
        assert!(crossings(&[a, b, c], &down, R).is_empty());
    }

    #[test]
    fn a_reach_crossing_itself_is_not_reported_here() {
        // Self-crossings are a separate question; this pass is about unrelated reaches.
        let a = line(&[(-0.2, 0.0), (0.2, 0.1), (-0.2, 0.1), (0.2, 0.2)]);
        assert!(crossings(&[a], &[Downstream::Ocean], R).is_empty());
    }
}
