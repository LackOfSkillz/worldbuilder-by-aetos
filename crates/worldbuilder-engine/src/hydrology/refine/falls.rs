//! Spec §6.7: finding a waterfall on one step of a trace, and inserting its two ends into the
//! segment the tracer is building.

use super::{Fine, Ground, Segment, SegmentFall, MAX_STATIONS};
use crate::detmath as m;
use crate::hydrology::HydroParams;
use crate::sphere::SpherePoint;

impl Segment {
    /// Records a fall `find_fall` returned on the step from the last point so far (the segment's
    /// start if there is none yet) to `to`, which the caller pushes next: inserts the fall's upper
    /// end, or marks the point it starts at as kept (Ruling P-1); then inserts its lower end, or
    /// marks `to` as that end when the fall's window ends there (Ruling FF-4).
    pub(super) fn insert_fall(&mut self, found: (Option<Fine>, Option<Fine>, f64), to: &mut Fine) {
        let (upper, lower, height_m) = found;
        let upper = match upper {
            Some(u) => {
                self.interior.push(u);
                Some(self.interior.len() - 1)
            }
            None => match self.interior.last_mut() {
                Some(last) => {
                    last.keep = true;
                    Some(self.interior.len() - 1)
                }
                None => None,
            },
        };
        self.falls.push(SegmentFall { upper, height_m });
        match lower {
            Some(l) => self.interior.push(l),
            None => to.keep = true,
        }
    }
}

/// Spec §6.7 on one step `from -> to` of a trace: a fall is where the bed drops at least
/// `fall_min_drop_m` across the step, and the ground drops at least that much inside one window
/// of at most `fall_max_run_m`. Returns the fall's two ends to insert into `segment.interior`, and
/// its height (Ruling R-5).
///
/// Either end can be an end the step already has, and is then returned as `None` rather than
/// inserted, because a separate point at the same position would leave a zero-length "step":
///
/// * the upper end when the best window starts right at `from` (Ruling P-1) -- `from` is the
///   upper end already, either the segment's coarse start or the last `Fine` pushed, which the
///   caller marks `keep = true`;
/// * the lower end when the best window is the step's last, so it ends on `to` (Ruling FF-4) --
///   `to` is the lower end, and the caller marks it kept.
///
/// The height is the smaller of the bed's drop and the window's, so the bed after the lower end
/// still never rises -- except when `to` is the lower end, where it is the bed's own drop to `to`,
/// which keeps "the next point after the fall is lower by exactly its height" true. That drop is
/// at least `fall_min_drop_m`, so the fall still qualifies.
pub(super) fn find_fall(ground: &Ground, params: &HydroParams, at: &dyn Fn(f64, f64) -> SpherePoint, from: &Fine, to: &Fine) -> Option<(Option<Fine>, Option<Fine>, f64)> {
    let bed_drop = from.bed_m - to.bed_m;
    if !(bed_drop >= params.fall_min_drop_m) {
        return None;
    }
    let dx = to.along_m - from.along_m;
    let dy = to.lateral_m - from.lateral_m;
    let run = m::hypot(dx, dy);
    let wanted = -m::floor(-(run / params.fall_max_run_m));
    let windows = if wanted < 1.0 { 1.0 } else if wanted > MAX_STATIONS { MAX_STATIONS } else { wanted };
    let count = windows as usize; // cast-ok: a whole number in 1..=MAX_STATIONS
    let mut best: Option<(usize, f64)> = None;
    let mut upper = (ground.height_m)(&from.point);
    for w in 0..count {
        let t = (w + 1) as f64 / windows;
        let lower = (ground.height_m)(&at(from.along_m + dx * t, from.lateral_m + dy * t));
        let drop = upper - lower;
        if drop.is_finite() {
            let better = match best { None => true, Some((_, d)) => drop > d };
            if better {
                best = Some((w, drop));
            }
        }
        upper = lower;
    }
    let (w, drop) = best?;
    if !(drop >= params.fall_min_drop_m) {
        return None;
    }
    let end = |t: f64, bed_m: f64| {
        let along_m = from.along_m + dx * t;
        let lateral_m = from.lateral_m + dy * t;
        Fine { along_m, lateral_m, point: at(along_m, lateral_m), bed_m, keep: true, station: false }
    };
    let upper = if w == 0 { None } else { Some(end(w as f64 / windows, from.bed_m)) };
    if w + 1 == count {
        return Some((upper, None, bed_drop));
    }
    let height = if drop < bed_drop { drop } else { bed_drop };
    Some((upper, Some(end((w + 1) as f64 / windows, from.bed_m - height)), height))
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::super::{refine_reach, simplify, trace_segment};
    use super::*;
    use crate::hydrology::reaches::ReachClass;
    use crate::hydrology::{Downstream, ReachLine, ReachPoint};
    use crate::tangent::TangentFrame;

    /// A cliff 50 m high and 100 m wide at 15 km along an otherwise gentle segment.
    fn cliff(p: &SpherePoint) -> f64 {
        let (_, e) = north_east(p);
        let base = 200.0 - 0.001 * e;
        if e < 15_000.0 { base } else if e > 15_100.0 { base - 50.0 } else { base - 50.0 * (e - 15_000.0) / 100.0 }
    }

    #[test]
    fn a_cliff_on_the_line_is_a_waterfall() {
        let (a, b) = ends(199.0, 119.0);
        let seg = trace_segment(&ground(&cliff), &params(), &a, &b, None);
        assert_eq!(seg.falls.len(), 1, "one fall");
        let fall = seg.falls[0];
        let height = fall.height_m;
        let at = fall.upper.map_or_else(|| SpherePoint::from_latlon(a.lat_deg, a.lon_deg), |i| seg.interior[i].point);
        assert!(height >= 10.0 && height <= 51.0, "height {height} (the cliff plus at most 150 m of the base slope)");
        let (_, e) = north_east(&at);
        assert!(e >= 14_800.0 && e <= 15_100.0, "fall's upper end at {e} m east");
        let kept: Vec<&Fine> = seg.interior.iter().filter(|f| f.keep).collect();
        assert_eq!(kept.len(), 2, "the fall's two protected points: from (marked keep) plus the lower end, or the inserted upper end plus the lower end");
        let d = kept[0].bed_m - kept[1].bed_m - height;
        assert!(d < 1e-9 && d > -1e-9, "the bed drops by the fall's height between them");
        let mut prev = a.bed_m;
        for f in &seg.interior {
            assert!(f.bed_m <= prev);
            prev = f.bed_m;
        }
    }

    #[test]
    fn a_steep_but_even_slope_has_no_waterfall() {
        // 60 m over 30 km: steep, but never 10 m in 150 m.
        let h = |p: &SpherePoint| { let (_, e) = north_east(p); 200.0 - 0.002 * e };
        let (a, b) = ends(199.0, 139.0);
        assert!(trace_segment(&ground(&h), &params(), &a, &b, None).falls.is_empty());
    }

    #[test]
    fn a_step_just_under_ten_metres_is_not_a_waterfall() {
        let h = |p: &SpherePoint| {
            let (_, e) = north_east(p);
            let base = 200.0 - 0.0001 * e;
            if e < 15_000.0 { base } else if e > 15_100.0 { base - 9.0 } else { base - 9.0 * (e - 15_000.0) / 100.0 }
        };
        let (a, b) = ends(199.0, 185.0);
        assert!(trace_segment(&ground(&h), &params(), &a, &b, None).falls.is_empty());
    }

    /// Step 3: the fall's two protected ends (Ruling R-5) survive simplification. The upper end
    /// may be `from` itself under Ruling P-1 (already a coarse point, protected regardless), or an
    /// inserted point at the top of the cliff; either way it is wherever `refine_reach` marked
    /// `protected` for the point just before the fall's recorded lower end.
    #[test]
    fn a_falls_two_ends_survive_simplification() {
        let (a, b) = ends(199.0, 119.0);
        let reach = ReachLine { id: 0, class: ReachClass::Stream, order: 1,
                                downstream: Downstream::Sink, fresh: true, points: vec![a.clone(), b.clone()] };
        let refined = refine_reach(&reach, None, &ground(&cliff), &params());
        assert_eq!(refined.falls.len(), 1, "one fall on this reach");
        let (fall_lat, fall_lon) = refined.falls[0].at;
        let upper_idx = refined.points.iter().position(|p| p.lat_deg == fall_lat && p.lon_deg == fall_lon)
            .expect("the fall's upper end is one of the refined points");
        assert!(refined.protected[upper_idx], "the fall's upper end is protected");
        let lower_idx = upper_idx + 1;
        assert!(refined.protected[lower_idx], "the fall's lower end is protected");
        let upper = refined.points[upper_idx].clone();
        let lower = refined.points[lower_idx].clone();
        assert!(upper.bed_m - lower.bed_m >= 10.0, "the fall's drop survives between the two ends");

        let simplified = simplify(&refined.points, &refined.protected, R, &params());
        assert!(simplified.contains(&upper), "the fall's upper end survives simplification");
        assert!(simplified.contains(&lower), "the fall's lower end survives simplification");
    }

    /// A 30 km reach due east from `(lat, lon)`, and the start's own tangent frame to write its
    /// ground in.
    fn reach_east_of(lat: f64, lon: f64) -> (ReachLine, TangentFrame) {
        let start = SpherePoint::from_latlon(lat, lon);
        let frame = TangentFrame::at(&start, R);
        let (lb, lob) = frame.local_to_sphere(30_000.0, 0.0).to_latlon();
        let a = ReachPoint { lat_deg: lat, lon_deg: lon, bed_m: 199.0, width_m: 10.0, depth_m: 1.0, flow_m2: 1.0e9 };
        let b = ReachPoint { lat_deg: lb, lon_deg: lob, bed_m: 119.0, width_m: 10.0, depth_m: 1.0, flow_m2: 1.0e9 };
        (ReachLine { id: 0, class: ReachClass::Stream, order: 1, downstream: Downstream::Sink,
                     fresh: true, points: vec![a, b] }, frame)
    }

    /// Ruling FF-1: a cliff in the very first window, so the fall's upper end is the coarse start
    /// itself, at a latitude and longitude whose sphere round trip is not exact. `Fall.at` must
    /// still be that point of the reach, bit for bit.
    #[test]
    fn a_fall_at_the_coarse_start_is_that_point_bit_for_bit() {
        // The final review's real-world case first, then a walk until the round trip is inexact.
        let mut start = None;
        for k in 0..1_000u32 {
            let lat = 26.589_862_789_647_377 + f64::from(k) * 0.001_37;
            let lon = 96.156_071_934_665_86 + f64::from(k) * 0.002_11;
            let (la, lo) = SpherePoint::from_latlon(lat, lon).to_latlon();
            if la.to_bits() != lat.to_bits() || lo.to_bits() != lon.to_bits() {
                start = Some((lat, lon));
                break;
            }
        }
        let (lat, lon) = start.expect("some latitude and longitude does not round-trip exactly");
        let (reach, frame) = reach_east_of(lat, lon);
        let h = move |q: &SpherePoint| {
            let (e, _) = frame.sphere_to_local(q);
            let base = 200.0 - 0.001 * e;
            if e < 20.0 { base } else if e > 120.0 { base - 50.0 } else { base - 50.0 * (e - 20.0) / 100.0 }
        };
        let refined = refine_reach(&reach, None, &ground(&h), &params());
        assert_eq!(refined.falls.len(), 1, "one fall, in the first window");
        let at = refined.falls[0].at;
        let first = &refined.points[0];
        assert_eq!((at.0.to_bits(), at.1.to_bits()), (first.lat_deg.to_bits(), first.lon_deg.to_bits()),
                   "the fall's upper end is the reach's first point exactly");
    }

    /// Ruling FF-4: a cliff in a step's *last* window would put the fall's lower end on the step's
    /// own end point, and the step after it would be zero-length. The end point is the lower end
    /// instead (the mirror of Ruling P-1), so no two consecutive points share a position, and the
    /// bed still drops by exactly the fall's height across the fall.
    #[test]
    fn a_cliff_in_the_last_window_leaves_no_zero_length_step() {
        // 14,850..15,000 m is the last window of the step into station 10; 29,850..30,000 m is
        // the last window of the final step, into the coarse end `b` itself.
        for cliff_at in [14_860.0, 29_860.0] {
            let h = move |p: &SpherePoint| {
                let (_, e) = north_east(p);
                let base = 200.0 - 0.001 * e;
                if e < cliff_at { base }
                else if e > cliff_at + 130.0 { base - 50.0 }
                else { base - 50.0 * (e - cliff_at) / 130.0 }
            };
            let (a, b) = ends(199.0, 119.0);
            let reach = ReachLine { id: 0, class: ReachClass::Stream, order: 1, downstream: Downstream::Sink,
                                    fresh: true, points: vec![a.clone(), b.clone()] };
            let refined = refine_reach(&reach, None, &ground(&h), &params());
            assert_eq!(refined.falls.len(), 1, "one fall for a cliff at {cliff_at} m");
            for w in refined.points.windows(2) {
                let d = SpherePoint::from_latlon(w[0].lat_deg, w[0].lon_deg)
                    .distance_to(&SpherePoint::from_latlon(w[1].lat_deg, w[1].lon_deg), R);
                assert!(d > 1.0, "a zero-length step at {}, {} (cliff at {cliff_at} m)", w[0].lat_deg, w[0].lon_deg);
            }
            let fall = &refined.falls[0];
            let i = refined.points.iter()
                .position(|q| (q.lat_deg.to_bits(), q.lon_deg.to_bits()) == (fall.at.0.to_bits(), fall.at.1.to_bits()))
                .expect("the fall's upper end is a point of the reach");
            let drop = refined.points[i].bed_m - refined.points[i + 1].bed_m;
            let d = drop - fall.height_m;
            assert!(d < 1e-9 && d > -1e-9, "the step after the fall is {drop} m, the fall {} m", fall.height_m);
            assert!(fall.height_m >= params().fall_min_drop_m);
        }
    }
}
