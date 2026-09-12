//! Ruling R-7: Douglas-Peucker over one refined reach, in the two units the record cares about --
//! sideways from a chord, and off a straight-line bed.

use crate::detmath as m;
use crate::hydrology::{HydroParams, ReachPoint};
use crate::sphere::SpherePoint;
use crate::tangent::TangentFrame;

/// Ruling R-7: Douglas–Peucker over one refined reach. Between two kept points, the point that
/// strays furthest -- sideways from their chord, in units of `refine_simplify_m`, or off their
/// straight-line bed, in units of `refine_vertical_m`, whichever is worse -- is kept if it strays
/// more than one unit, and the two halves are examined in turn. Protected points (coarse points,
/// fall ends, the mouth) and both ends are always kept; a `protected` shorter than `points` simply
/// protects nothing past its end. Keeping a subset of a falling bed keeps it
/// falling, so spec §14.5 survives. The outcome does not depend on the order spans are examined.
pub fn simplify(points: &[ReachPoint], protected: &[bool], radius_m: f64, params: &HydroParams) -> Vec<ReachPoint> {
    let keep = simplify_mask(points, protected, radius_m, params);
    points.iter().zip(&keep).filter(|(_, &k)| k).map(|(p, _)| p.clone()).collect()
}

/// `simplify`'s decision, as a mask parallel to `points`, for a caller with more than one array
/// to cut down. Ruling S-14 gave `refine` a second one: `Refined::segment_of` has to survive
/// simplification, because the crossing pass reads it off the *shipped* line.
pub(super) fn simplify_mask(points: &[ReachPoint], protected: &[bool], radius_m: f64, params: &HydroParams) -> Vec<bool> {
    let n = points.len();
    if n <= 2 {
        return vec![true; n];
    }
    let at = |p: &ReachPoint| SpherePoint::from_latlon(p.lat_deg, p.lon_deg);
    // Ruling FF-5: `protected` is a parallel array, but a short one is not an error -- entries it
    // does not have are unprotected, and nothing is read past it.
    let mut keep: Vec<bool> = (0..n).map(|i| protected.get(i) == Some(&true)).collect();
    keep[0] = true;
    keep[n - 1] = true;
    let anchors: Vec<usize> = (0..n).filter(|&i| keep[i]).collect();
    let mut spans: Vec<(usize, usize)> = anchors.windows(2).map(|w| (w[0], w[1])).collect();
    while let Some((lo, hi)) = spans.pop() {
        if hi <= lo + 1 {
            continue;
        }
        let frame = TangentFrame::at(&at(&points[lo]), radius_m);
        let (bx, by) = frame.sphere_to_local(&at(&points[hi]));
        let len2 = bx * bx + by * by;
        let mut worst = 0.0;
        let mut worst_at = lo;
        for i in lo + 1..hi {
            let (px, py) = frame.sphere_to_local(&at(&points[i]));
            let raw = if len2 > 0.0 { (px * bx + py * by) / len2 } else { 0.0 };
            let t = if raw < 0.0 { 0.0 } else if raw > 1.0 { 1.0 } else { raw };
            let sideways = m::hypot(px - t * bx, py - t * by) / params.refine_simplify_m;
            let straight_bed = points[lo].bed_m + t * (points[hi].bed_m - points[lo].bed_m);
            let off = points[i].bed_m - straight_bed;
            let vertical = (if off < 0.0 { -off } else { off }) / params.refine_vertical_m;
            let err = if sideways > vertical { sideways } else { vertical };
            if err > worst {
                worst = err;
                worst_at = i;
            }
        }
        if worst > 1.0 {
            keep[worst_at] = true;
            spans.push((lo, worst_at));
            spans.push((worst_at, hi));
        }
    }
    keep
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;

    #[test]
    fn a_straight_even_line_keeps_only_its_ends() {
        let pts = line_of(&[10.0, 9.0, 8.0, 7.0, 6.0], &[0.0; 5]);
        let out = simplify(&pts, &[true, false, false, false, true], R, &params());
        assert_eq!(out, vec![pts[0].clone(), pts[4].clone()]);
    }

    #[test]
    fn a_bend_wider_than_the_tolerance_is_kept_and_a_small_one_is_not() {
        // Offsets scale with the tolerance, so the test holds at whatever `earth_like` sets it
        // to (500 m since plan 1b-2 Task 8): 1.6 tolerances out is kept, 0.4 is not.
        let tol = params().refine_simplify_m;
        let pts = line_of(&[10.0, 9.0, 8.0], &[0.0, 1.6 * tol, 0.0]);
        assert_eq!(simplify(&pts, &[true, false, true], R, &params()).len(), 3);
        let small = line_of(&[10.0, 9.0, 8.0], &[0.0, 0.4 * tol, 0.0]);
        assert_eq!(simplify(&small, &[true, false, true], R, &params()).len(), 2);
    }

    #[test]
    fn a_bed_step_over_a_metre_is_kept() {
        let pts = line_of(&[10.0, 7.0, 6.5], &[0.0; 3]);
        assert_eq!(simplify(&pts, &[true, false, true], R, &params()).len(), 3);
    }

    #[test]
    fn protected_points_are_always_kept() {
        let pts = line_of(&[10.0, 9.0, 8.0, 7.0, 6.0], &[0.0; 5]);
        let out = simplify(&pts, &[true, false, true, false, true], R, &params());
        assert_eq!(out, vec![pts[0].clone(), pts[2].clone(), pts[4].clone()]);
    }

    /// Ruling FF-5: `protected` is a parallel array, but a caller can hand a short one. Entries
    /// beyond its length are simply unprotected, and nothing is read past it.
    #[test]
    fn a_short_protected_slice_leaves_the_rest_unprotected() {
        let pts = line_of(&[10.0, 9.0, 8.0, 7.0, 6.0], &[0.0; 5]);
        assert_eq!(simplify(&pts, &[true], R, &params()), vec![pts[0].clone(), pts[4].clone()]);
        assert_eq!(simplify(&pts, &[], R, &params()), vec![pts[0].clone(), pts[4].clone()]);
        let out = simplify(&pts, &[false, false, true], R, &params());
        assert_eq!(out, vec![pts[0].clone(), pts[2].clone(), pts[4].clone()],
                   "the entries it does have still count");
    }
}
