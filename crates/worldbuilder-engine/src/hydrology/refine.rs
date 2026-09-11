//! Refinement (spec §6.6): every coarse reach re-traced on the landform at fine steps.
//!
//! A coarse segment runs from one reach point to the next, about one graph spacing long. Each
//! is walked in `refine_step_m` stations along its chord. At each station the tracer looks a
//! little to either side and takes the lowest ground, so the line settles into the valley floor
//! the coarse graph only saw every few tens of kilometres. It never leaves the corridor (one
//! graph spacing either side of the chord). It never steps onto ground at or below the datum
//! before its mouth (Ruling R-3). It always arrives back on the next coarse point. Coarse points
//! are kept exactly, so a tributary still ends on its receiver's first vertex (Ruling R-1, spec
//! §14.4).
//!
//! The bed never rises (spec §14.5). Inside a segment it follows the ground down, less the
//! channel's depth, but never below the segment's lower end. Where the ground rises, the bed
//! holds, which is a cut. A fine dip met on the way is not judged as a new lake: the bed stays
//! level across it (Ruling R-2), and plan 1b-3's pond search owns fine lakes. The last segment of
//! a reach into the sea or a lake ends at the first station on the shore, and the mouth's bed is
//! the lower of the bed so far and the water level (Rulings R-3, R-4).

use crate::detmath as m;
use crate::hydrology::{Body, Downstream, Fall, HydroParams, HydroRecord, ReachLine, ReachPoint};
use crate::noise::Noise;
use crate::sphere::SpherePoint;
use crate::tangent::TangentFrame;

/// Salt for the meander's phase, so it is independent of every other noise field on the world.
const MEANDER_SALT: u64 = 0x4d45_414e_4445_5253;
/// Scales a unit vector into the noise lattice, so neighbouring segments get unrelated phases.
const MEANDER_FREQUENCY: f64 = 64.0;

/// A tracer never plans more stations (or fall windows) than this on one segment, whatever the
/// params ask.
const MAX_STATIONS: f64 = 100_000.0;

/// Lateral candidates at each station, as fractions of the station spacing, in tie-break order:
/// straight on first, then the nearer sides, left before right.
const CANDIDATES: [f64; 5] = [0.0, -0.5, 0.5, -1.0, 1.0];

/// The ground and the geometry a trace needs, apart from the reach itself. A closure rather than
/// a `Surface`, so the tests can trace over ground written by hand.
pub struct Ground<'a> {
    pub height_m: &'a dyn Fn(&SpherePoint) -> f64,
    pub radius_m: f64,
    /// One graph spacing: how far either side of a coarse chord the line may wander.
    pub corridor_m: f64,
    /// The world seed, for the meander's phase.
    pub seed: u64,
}

/// One traced point inside a coarse segment, in the segment's own frame (metres along the chord
/// from its start, and to its left).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fine {
    pub along_m: f64,
    pub lateral_m: f64,
    pub point: SpherePoint,
    pub bed_m: f64,
    /// Survives simplification and is never meandered (a fall's two ends).
    pub keep: bool,
}

/// What one segment traced to: its interior points; the mouth that replaces the coarse end, on a
/// last segment that reached the shore first; and any falls, as (upper end, height).
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub interior: Vec<Fine>,
    pub mouth: Option<Fine>,
    pub falls: Vec<(SpherePoint, f64)>,
}

/// One refined reach: its points, which of them simplification must keep, and its falls.
#[derive(Debug, Clone, PartialEq)]
pub struct Refined {
    pub points: Vec<ReachPoint>,
    pub protected: Vec<bool>,
    pub falls: Vec<Fall>,
}

/// The water level a reach runs into at its end: the datum for the ocean, a lake's own level.
/// `None` for a reach that ends on another reach or nowhere.
pub fn terminal_level(reach: &ReachLine, bodies: &[Body]) -> Option<f64> {
    match reach.downstream {
        Downstream::Ocean => Some(0.0),
        Downstream::Body(id) => bodies.get(id as usize).map(|b| b.level_m),
        Downstream::Reach(_) | Downstream::Sink => None,
    }
}

/// Spec §14.5 on one reach: the bed never rises from one point to the next.
pub fn beds_never_rise(reach: &ReachLine) -> bool {
    reach.points.windows(2).all(|w| w[1].bed_m <= w[0].bed_m)
}

/// Traces the coarse segment `a -> b`. `shore` is the level of the water the reach runs into,
/// given only for its last segment.
pub fn trace_segment(ground: &Ground, params: &HydroParams, a: &ReachPoint, b: &ReachPoint, shore: Option<f64>) -> Segment {
    let mut segment = Segment { interior: Vec::new(), mouth: None, falls: Vec::new() };
    let start = SpherePoint::from_latlon(a.lat_deg, a.lon_deg);
    let end = SpherePoint::from_latlon(b.lat_deg, b.lon_deg);
    let frame = TangentFrame::at(&start, ground.radius_m);
    let (bx, by) = frame.sphere_to_local(&end);
    let len = m::hypot(bx, by);
    if !(len > params.refine_step_m) {
        return segment;
    }
    let wanted = -m::floor(-(len / params.refine_step_m));
    let stations = if wanted > MAX_STATIONS { MAX_STATIONS } else { wanted };
    let k = stations as usize; // cast-ok: a whole number in 2..=MAX_STATIONS
    let spacing = len / stations;
    let (ux, uy) = (bx / len, by / len);
    let (vx, vy) = (-uy, ux);
    let at = |along: f64, lateral: f64| frame.local_to_sphere(ux * along + vx * lateral, uy * along + vy * lateral);

    // The bed may fall to the segment's lower end and no further.
    let floor_m = if b.bed_m < a.bed_m { b.bed_m } else { a.bed_m };
    let mut lateral = 0.0;
    let mut bed = a.bed_m;
    let mut previous = Fine { along_m: 0.0, lateral_m: 0.0, point: start, bed_m: a.bed_m, keep: true };
    for i in 1..k {
        let along = spacing * i as f64; // cast-ok: i < k <= MAX_STATIONS
        let remaining = spacing * (k - i) as f64; // cast-ok: i < k <= MAX_STATIONS
        let limit = if remaining < ground.corridor_m { remaining } else { ground.corridor_m };
        let mut best: Option<(f64, f64, SpherePoint)> = None;
        for &j in CANDIDATES.iter() {
            let o = lateral + j * spacing;
            if o > limit || o < -limit {
                continue;
            }
            let p = at(along, o);
            let g = (ground.height_m)(&p);
            if !g.is_finite() {
                continue;
            }
            if shore.is_none() && g <= 0.0 {
                continue;
            }
            let better = match best {
                None => true,
                Some((best_g, _, _)) => g < best_g,
            };
            if better {
                best = Some((g, o, p));
            }
        }
        let (g, o, p) = match best {
            Some(found) => found,
            None => {
                // Nothing allowed: hold the line as close to where it was as the limit lets it be.
                let o = if lateral > limit { limit } else if lateral < -limit { -limit } else { lateral };
                let p = at(along, o);
                let g = (ground.height_m)(&p);
                (if g.is_finite() { g } else { bed + a.depth_m }, o, p)
            }
        };
        lateral = o;
        if let Some(level) = shore {
            if g <= level {
                let mouth_bed = if bed < level { bed } else { level };
                segment.mouth = Some(Fine { along_m: along, lateral_m: o, point: p, bed_m: mouth_bed, keep: false });
                return segment;
            }
        }
        let want = g - a.depth_m;
        if want < bed {
            bed = want;
        }
        if bed < floor_m {
            bed = floor_m;
        }
        let here = Fine { along_m: along, lateral_m: o, point: p, bed_m: bed, keep: false };
        if let Some((upper, lower, height)) = find_fall(ground, params, &at, &previous, &here) {
            match upper {
                Some(u) => {
                    segment.falls.push((u.point, height));
                    segment.interior.push(u);
                }
                None => {
                    segment.falls.push((previous.point, height));
                    if let Some(last) = segment.interior.last_mut() {
                        last.keep = true;
                    }
                }
            }
            segment.interior.push(lower);
        }
        segment.interior.push(here);
        previous = here;
    }
    let into_end = Fine { along_m: len, lateral_m: 0.0, point: end, bed_m: b.bed_m, keep: true };
    if let Some((upper, lower, height)) = find_fall(ground, params, &at, &previous, &into_end) {
        match upper {
            Some(u) => {
                segment.falls.push((u.point, height));
                segment.interior.push(u);
            }
            None => {
                segment.falls.push((previous.point, height));
                if let Some(last) = segment.interior.last_mut() {
                    last.keep = true;
                }
            }
        }
        segment.interior.push(lower);
    }

    // Ruling R-6: a meander only where it can be drawn at this step (a wavelength of at least
    // four steps), where the river is flat (bed slope under `meander_max_slope`), and where no
    // fall was found. It is tapered to zero at both coarse points and kept inside the corridor.
    // It moves the line, never the bed.
    let wavelength = params.meander_wavelength_widths * a.width_m;
    let slope = (a.bed_m - floor_m) / len;
    let amplitude = params.meander_amplitude_widths * a.width_m;
    if segment.falls.is_empty() && slope < params.meander_max_slope
        && wavelength >= 4.0 * params.refine_step_m && amplitude > 0.0 {
        let v = start.vector;
        let n = Noise::new(ground.seed, MEANDER_SALT)
            .at(v.x * MEANDER_FREQUENCY, v.y * MEANDER_FREQUENCY, v.z * MEANDER_FREQUENCY);
        if n.is_finite() {
            let pi = std::f64::consts::PI;
            let phase = pi * (1.0 + n);
            for fine in segment.interior.iter_mut() {
                let envelope = m::sin(pi * fine.along_m / len);
                let mut shift = amplitude * envelope * m::sin(2.0 * pi * fine.along_m / wavelength + phase);
                let room_left = ground.corridor_m - fine.lateral_m;
                let room_right = ground.corridor_m + fine.lateral_m;
                if shift > room_left {
                    shift = room_left;
                }
                if shift < -room_right {
                    shift = -room_right;
                }
                fine.lateral_m += shift;
                fine.point = at(fine.along_m, fine.lateral_m);
            }
        }
    }
    segment
}

/// Spec §6.7 on one step `from -> to` of a trace: a fall is where the bed drops at least
/// `fall_min_drop_m` across the step, and the ground drops at least that much inside one window
/// of at most `fall_max_run_m`. Returns the fall's lower end and its height (Ruling R-5), and the
/// upper end to insert into `segment.interior` -- or `None` when the fall's best window starts
/// right at `from` (Ruling P-1): then `from` is the upper end already (either the segment's
/// coarse start, already protected, or the last `Fine` pushed into `segment.interior`, which the
/// caller must mark `keep = true`). Inserting a separate upper end in that case would duplicate
/// `from`'s position, and the "step" after it would be zero. The height is the smaller of the
/// bed's drop and the window's, so the bed after the lower end still never rises.
fn find_fall(ground: &Ground, params: &HydroParams, at: &dyn Fn(f64, f64) -> SpherePoint, from: &Fine, to: &Fine) -> Option<(Option<Fine>, Fine, f64)> {
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
    let height = if drop < bed_drop { drop } else { bed_drop };
    let end = |t: f64, bed_m: f64| {
        let along_m = from.along_m + dx * t;
        let lateral_m = from.lateral_m + dy * t;
        Fine { along_m, lateral_m, point: at(along_m, lateral_m), bed_m, keep: true }
    };
    let t1 = (w + 1) as f64 / windows;
    let lower = end(t1, from.bed_m - height);
    if w == 0 {
        Some((None, lower, height))
    } else {
        let t0 = w as f64 / windows;
        Some((Some(end(t0, from.bed_m)), lower, height))
    }
}

/// Ruling R-7: Douglas–Peucker over one refined reach. Between two kept points, the point that
/// strays furthest -- sideways from their chord, in units of `refine_simplify_m`, or off their
/// straight-line bed, in units of `refine_vertical_m`, whichever is worse -- is kept if it strays
/// more than one unit, and the two halves are examined in turn. Protected points (coarse points,
/// fall ends, the mouth) and both ends are always kept. Keeping a subset of a falling bed keeps it
/// falling, so spec §14.5 survives. The outcome does not depend on the order spans are examined.
pub fn simplify(points: &[ReachPoint], protected: &[bool], radius_m: f64, params: &HydroParams) -> Vec<ReachPoint> {
    let n = points.len();
    if n <= 2 {
        return points.to_vec();
    }
    let at = |p: &ReachPoint| SpherePoint::from_latlon(p.lat_deg, p.lon_deg);
    let mut keep: Vec<bool> = protected.to_vec();
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
    points.iter().zip(&keep).filter(|(_, &k)| k).map(|(p, _)| p.clone()).collect()
}

fn fine_point(fine: &Fine, like: &ReachPoint) -> ReachPoint {
    let (lat_deg, lon_deg) = fine.point.to_latlon();
    ReachPoint { lat_deg, lon_deg, bed_m: fine.bed_m, width_m: like.width_m, depth_m: like.depth_m, flow_m2: like.flow_m2 }
}

/// Refines one reach: coarse points kept exactly, fine points between them, trimmed at the shore
/// on its last segment. Interior points carry their segment's upstream width, depth and flow.
/// Flow only steps at a coarse point, where a tributary joins.
pub fn refine_reach(reach: &ReachLine, shore: Option<f64>, ground: &Ground, params: &HydroParams) -> Refined {
    let coarse = &reach.points;
    let mut refined = Refined { points: Vec::new(), protected: Vec::new(), falls: Vec::new() };
    if coarse.is_empty() {
        return refined;
    }
    refined.points.push(coarse[0].clone());
    refined.protected.push(true);
    for s in 0..coarse.len() - 1 {
        let a = &coarse[s];
        let b = &coarse[s + 1];
        let last = s + 2 == coarse.len();
        let here_shore = if last { shore } else { None };
        let segment = trace_segment(ground, params, a, b, here_shore);
        for fine in &segment.interior {
            refined.points.push(fine_point(fine, a));
            refined.protected.push(fine.keep);
        }
        for &(at, height_m) in &segment.falls {
            refined.falls.push(Fall { reach: reach.id, at: at.to_latlon(), height_m });
        }
        if let Some(mouth) = segment.mouth {
            refined.points.push(fine_point(&mouth, b));
            refined.protected.push(true);
            break;
        }
        let mut end = b.clone();
        if last && here_shore.is_some() {
            // Ruling R-4: a mouth's bed never rises above the bed that reaches it.
            let before = refined.points.last().expect("at least the first point").bed_m;
            if before < end.bed_m {
                end.bed_m = before;
            }
        }
        refined.points.push(end);
        refined.protected.push(true);
    }
    refined
}

/// Refines every reach in the record, in reach order, and records the falls in the same order.
pub fn refine(record: &mut HydroRecord, ground: &Ground, params: &HydroParams) {
    let shores: Vec<Option<f64>> = record.reaches.iter().map(|r| terminal_level(r, &record.bodies)).collect();
    let mut falls = Vec::new();
    for (reach, shore) in record.reaches.iter_mut().zip(shores) {
        let refined = refine_reach(reach, shore, ground, params);
        reach.points = simplify(&refined.points, &refined.protected, ground.radius_m, params);
        falls.extend(refined.falls);
    }
    record.falls = falls;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hydrology::reaches::ReachClass;

    const R: f64 = 6_371_000.0;
    /// Metres per degree on this test radius.
    const M_PER_DEG: f64 = R * std::f64::consts::PI / 180.0;

    fn point(lat_deg: f64, lon_deg: f64, bed_m: f64) -> ReachPoint {
        ReachPoint { lat_deg, lon_deg, bed_m, width_m: 10.0, depth_m: 1.0, flow_m2: 1.0e9 }
    }

    /// A 30 km segment due east along the equator: a at 0 E, b at 30 km east.
    fn ends(bed_a: f64, bed_b: f64) -> (ReachPoint, ReachPoint) {
        (point(0.0, 0.0, bed_a), point(0.0, 30_000.0 / M_PER_DEG, bed_b))
    }

    /// North of the equator in metres, and east of 0 E in metres (small-angle, test only).
    fn north_east(p: &SpherePoint) -> (f64, f64) {
        let (lat, lon) = p.to_latlon();
        (lat * M_PER_DEG, lon * M_PER_DEG)
    }

    fn ground<'a>(height: &'a dyn Fn(&SpherePoint) -> f64) -> Ground<'a> {
        Ground { height_m: height, radius_m: R, corridor_m: 20_000.0, seed: 7 }
    }

    fn params() -> HydroParams {
        HydroParams::earth_like(1_000)
    }

    #[test]
    fn a_trace_settles_into_the_valley_floor() {
        // A straight valley 5 km north of the chord, falling gently east.
        let h = |p: &SpherePoint| {
            let (n, e) = north_east(p);
            let off = if n > 5_000.0 { n - 5_000.0 } else { 5_000.0 - n };
            100.0 - 0.001 * e + 0.02 * off
        };
        let (a, b) = ends(99.0, 69.0);
        let seg = trace_segment(&ground(&h), &params(), &a, &b, None);
        let n = seg.interior.len();
        assert!(n == 19 || n == 20, "30 km at 1.5 km: 20 steps (21 if the chord rounds just over), got {n} interior stations");
        let middle: Vec<&Fine> = seg.interior.iter()
            .filter(|f| f.along_m >= 8_000.0 && f.along_m <= 20_000.0).collect();
        assert!(!middle.is_empty());
        for f in middle {
            let off = f.lateral_m - 5_000.0;
            assert!(off <= 750.0 && off >= -750.0, "station at {} m is {} m off the valley", f.along_m, off);
        }
    }

    #[test]
    fn the_bed_never_rises_and_never_drops_below_the_segment_end() {
        let h = |p: &SpherePoint| {
            let (_, e) = north_east(p);
            50.0 - 0.001 * e + 30.0 * crate::detmath::sin(e / 2_000.0)
        };
        let (a, b) = ends(49.0, 19.0);
        let seg = trace_segment(&ground(&h), &params(), &a, &b, None);
        let mut prev = a.bed_m;
        for f in &seg.interior {
            assert!(f.bed_m <= prev, "bed rose from {prev} to {}", f.bed_m);
            assert!(f.bed_m >= b.bed_m, "bed {} fell below the segment end {}", f.bed_m, b.bed_m);
            prev = f.bed_m;
        }
    }

    #[test]
    fn a_trace_stays_in_its_corridor_and_returns_to_the_next_point() {
        // Ground falling to the north without end: the tracer goes as far as it may, and comes back.
        let h = |p: &SpherePoint| { let (n, _) = north_east(p); 100.0 - 0.01 * n };
        let (a, b) = ends(99.0, 90.0);
        let g = ground(&h);
        let seg = trace_segment(&g, &params(), &a, &b, None);
        let spacing = 30_000.0 / 20.0;
        for f in &seg.interior {
            assert!(f.lateral_m <= g.corridor_m && f.lateral_m >= -g.corridor_m);
            let remaining = 30_000.0 - f.along_m;
            assert!(f.lateral_m <= remaining + 1e-6 && f.lateral_m >= -remaining - 1e-6,
                    "station at {} m cannot get back to the chord", f.along_m);
            let _ = spacing;
        }
        let last = seg.interior.last().expect("stations");
        assert!(last.lateral_m <= spacing + 1e-6 && last.lateral_m >= -spacing - 1e-6);
    }

    #[test]
    fn an_inland_segment_never_steps_into_the_sea() {
        // Sea south of 2 km south; the land just north of it is the lowest land.
        let h = |p: &SpherePoint| { let (n, _) = north_east(p); if n < -2_000.0 { -10.0 } else { 10.0 + 0.001 * n } };
        let (a, b) = ends(9.0, 8.0);
        let seg = trace_segment(&ground(&h), &params(), &a, &b, None);
        for f in &seg.interior {
            assert!(h(&f.point) > 0.0, "station at {} m stepped into the sea", f.along_m);
        }
    }

    #[test]
    fn a_river_ends_at_the_shore() {
        // Land falling east to the sea at 20 km: the last segment stops there, not at its coarse end.
        let h = |p: &SpherePoint| { let (_, e) = north_east(p); 100.0 - 0.005 * e };
        let (a, b) = ends(99.0, 0.0);
        let seg = trace_segment(&ground(&h), &params(), &a, &b, Some(0.0));
        let mouth = seg.mouth.expect("the trace reaches the shore before b");
        assert!(h(&mouth.point) <= 0.0);
        assert!(mouth.along_m >= 19_500.0 && mouth.along_m <= 21_600.0, "mouth at {} m", mouth.along_m);
        for f in &seg.interior {
            assert!(h(&f.point) > 0.0 && f.along_m < mouth.along_m);
        }
        assert!(mouth.bed_m <= 0.0);
    }

    #[test]
    fn coarse_points_are_kept_exactly_and_the_mouth_bed_never_rises() {
        let h = |p: &SpherePoint| { let (_, e) = north_east(p); 100.0 - 0.001 * e };
        let pts = vec![point(0.0, 0.0, 99.0), point(0.0, 30_000.0 / M_PER_DEG, 69.0),
                       point(0.0, 60_000.0 / M_PER_DEG, 5.0)];
        let reach = ReachLine { id: 0, class: ReachClass::Stream, order: 1,
                                downstream: Downstream::Ocean, fresh: true, points: pts.clone() };
        let refined = refine_reach(&reach, Some(20.0), &ground(&h), &params());
        assert_eq!(refined.points.len(), refined.protected.len());
        assert_eq!(refined.points[0], pts[0]);
        assert!(refined.points.iter().any(|p| p == &pts[1]), "the middle coarse point is kept");
        let line = ReachLine { points: refined.points.clone(), ..reach.clone() };
        assert!(beds_never_rise(&line));
    }

    #[test]
    fn beds_never_rise_catches_a_rising_bed() {
        let reach = ReachLine { id: 0, class: ReachClass::Stream, order: 1, downstream: Downstream::Ocean,
                                fresh: true, points: vec![point(0.0, 0.0, 10.0), point(0.0, 0.1, 11.0)] };
        assert!(!beds_never_rise(&reach));
    }

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
        let (at, height) = seg.falls[0];
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

    fn wide(lat_deg: f64, lon_deg: f64, bed_m: f64, width_m: f64) -> ReachPoint {
        ReachPoint { lat_deg, lon_deg, bed_m, width_m, depth_m: 5.0, flow_m2: 1.0e12 }
    }

    /// Nearly flat ground falling east: 0.05% slope.
    fn flat(p: &SpherePoint) -> f64 { let (_, e) = north_east(p); 50.0 - 0.0005 * e }

    #[test]
    fn a_flat_wide_river_meanders_inside_its_corridor() {
        let a = wide(0.0, 0.0, 45.0, 1_000.0);
        let b = wide(0.0, 30_000.0 / M_PER_DEG, 30.0, 1_000.0);
        let g = ground(&flat);
        let seg = trace_segment(&g, &params(), &a, &b, None);
        let mut straight = params();
        straight.meander_amplitude_widths = 0.0;
        let plain = trace_segment(&g, &straight, &a, &b, None);
        assert_eq!(seg.interior.len(), plain.interior.len());
        let mut moved = 0usize;
        for (f, q) in seg.interior.iter().zip(&plain.interior) {
            let shift = f.lateral_m - q.lateral_m;
            assert!(shift <= 1_500.0 + 1e-9 && shift >= -1_500.0 - 1e-9, "shift {shift} exceeds 1.5 widths");
            assert!(f.lateral_m <= g.corridor_m && f.lateral_m >= -g.corridor_m);
            assert_eq!(f.bed_m, q.bed_m, "a meander moves the line, not the bed");
            if shift > 1.0 || shift < -1.0 { moved += 1; }
        }
        assert!(moved > seg.interior.len() / 2, "most stations moved: {moved}");
    }

    /// Step 3 mutation guard's permanent variant: the same flat river, on a corridor tight
    /// enough (500 m, half the meander's own 1.5-width amplitude) that clamping actually binds.
    #[test]
    fn a_flat_wide_river_meander_stays_inside_a_tight_corridor() {
        let a = wide(0.0, 0.0, 45.0, 1_000.0);
        let b = wide(0.0, 30_000.0 / M_PER_DEG, 30.0, 1_000.0);
        let mut g = ground(&flat);
        g.corridor_m = 500.0;
        let seg = trace_segment(&g, &params(), &a, &b, None);
        for f in &seg.interior {
            assert!(f.lateral_m <= 500.0 + 1e-9 && f.lateral_m >= -500.0 - 1e-9,
                    "station at {} m strayed to {} m outside the 500 m corridor", f.along_m, f.lateral_m);
        }
    }

    #[test]
    fn a_narrow_river_does_not_meander_at_this_resolution() {
        let a = wide(0.0, 0.0, 45.0, 100.0);
        let b = wide(0.0, 30_000.0 / M_PER_DEG, 30.0, 100.0);
        let g = ground(&flat);
        let mut straight = params();
        straight.meander_amplitude_widths = 0.0;
        assert_eq!(trace_segment(&g, &params(), &a, &b, None), trace_segment(&g, &straight, &a, &b, None));
    }

    #[test]
    fn a_steep_river_does_not_meander() {
        let steep = |p: &SpherePoint| { let (_, e) = north_east(p); 500.0 - 0.01 * e };
        let a = wide(0.0, 0.0, 495.0, 1_000.0);
        let b = wide(0.0, 30_000.0 / M_PER_DEG, 195.0, 1_000.0);
        let g = ground(&steep);
        let mut straight = params();
        straight.meander_amplitude_widths = 0.0;
        assert_eq!(trace_segment(&g, &params(), &a, &b, None), trace_segment(&g, &straight, &a, &b, None));
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

    fn line_of(beds: &[f64], north_m: &[f64]) -> Vec<ReachPoint> {
        beds.iter().zip(north_m).enumerate()
            .map(|(i, (&bed, &n))| point(n / M_PER_DEG, (i as f64 * 1_500.0) / M_PER_DEG, bed))
            .collect()
    }

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

    #[test]
    fn terminal_levels() {
        let reach = |d| ReachLine { id: 0, class: ReachClass::Stream, order: 1, downstream: d,
                                    fresh: true, points: Vec::new() };
        assert_eq!(terminal_level(&reach(Downstream::Ocean), &[]), Some(0.0));
        assert_eq!(terminal_level(&reach(Downstream::Reach(3)), &[]), None);
        assert_eq!(terminal_level(&reach(Downstream::Sink), &[]), None);
    }
}
