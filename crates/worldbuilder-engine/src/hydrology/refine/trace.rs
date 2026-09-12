//! Walking one coarse segment on the landform: the lowest-ground search at each station, the bed
//! rule, Ruling FF-2's step back, and the shore trim that ends a reach at its mouth.

use super::falls::find_fall;
use super::meander::meander;
use super::{Chord, Fine, Ground, Segment, MAX_STATIONS};
use crate::detmath as m;
use crate::hydrology::{HydroParams, ReachLine, ReachPoint};
use crate::sphere::SpherePoint;

/// Lateral candidates at each station, as fractions of the station spacing, in tie-break order:
/// straight on first, then the nearer sides, left before right.
const CANDIDATES: [f64; 5] = [0.0, -0.5, 0.5, -1.0, 1.0];

/// Traces the coarse segment `a -> b`. `shore` is the level of the water the reach runs into,
/// given only for its last segment.
pub fn trace_segment(ground: &Ground, params: &HydroParams, a: &ReachPoint, b: &ReachPoint, shore: Option<f64>) -> Segment {
    let mut segment = trace(ground, params, a, b, shore, false);
    meander(&mut segment, ground, params, a, b);
    segment
}

/// The lowest-ground trace of `a -> b`, with no meander: `refine` runs the crossing pass between
/// the two (Ruling S-4), so the meander is a separate step there. `trace_segment` is this
/// followed by `meander`, which is what every caller outside `refine` wants.
///
/// `straight` is Ruling S-3's yielding segment: the lateral search is cut down to its first
/// candidate, which is the chord itself, so every interior station stands on the chord. Nothing
/// else changes -- the bed rule, the fall search, the shore trim and Ruling R-3a's step back
/// (which, with no other candidate to reach, simply keeps the chord point) are the same code.
pub fn trace(ground: &Ground, params: &HydroParams, a: &ReachPoint, b: &ReachPoint, shore: Option<f64>, straight: bool) -> Segment {
    let mut segment = Segment { interior: Vec::new(), mouth: None, falls: Vec::new() };
    let chord = Chord::new(ground, a, b);
    let len = chord.len_m;
    if !(len > params.refine_step_m) {
        return segment;
    }
    let wanted = -m::floor(-(len / params.refine_step_m));
    let stations = if wanted > MAX_STATIONS { MAX_STATIONS } else { wanted };
    let k = stations as usize; // cast-ok: a whole number in 2..=MAX_STATIONS
    let spacing = len / stations;
    let at = |along: f64, lateral: f64| chord.at(along, lateral);
    // CANDIDATES[0] is the chord itself, so a straight trace is the same search over just it.
    let candidates: &[f64] = if straight { &CANDIDATES[..1] } else { &CANDIDATES };

    // The bed may fall to the segment's lower end and no further.
    let floor_m = if b.bed_m < a.bed_m { b.bed_m } else { a.bed_m };
    let mut lateral = 0.0;
    let mut bed = a.bed_m;
    let mut previous = Fine { along_m: 0.0, lateral_m: 0.0, point: chord.start, bed_m: a.bed_m, keep: true, station: true };
    for i in 1..k {
        let along = spacing * i as f64; // cast-ok: i < k <= MAX_STATIONS
        let remaining = spacing * (k - i) as f64; // cast-ok: i < k <= MAX_STATIONS
        let limit = if remaining < ground.corridor_m { remaining } else { ground.corridor_m };
        let mut best: Option<(f64, f64, SpherePoint)> = None;
        for &j in candidates.iter() {
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
            None => step_back(ground, &at, along, lateral, limit, spacing, shore, bed + a.depth_m),
        };
        lateral = o;
        if let Some(level) = shore {
            if g <= level {
                let mouth_bed = if bed < level { bed } else { level };
                segment.mouth = Some(Fine { along_m: along, lateral_m: o, point: p, bed_m: mouth_bed, keep: false, station: true });
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
        let mut here = Fine { along_m: along, lateral_m: o, point: p, bed_m: bed, keep: false, station: true };
        if let Some(found) = find_fall(ground, params, &at, &previous, &here) {
            segment.insert_fall(found, &mut here);
        }
        segment.interior.push(here);
        previous = here;
    }
    // The coarse end is protected by `refine_reach` whatever happens here, so a fall that ends on
    // it needs nothing marking.
    let mut into_end = Fine { along_m: len, lateral_m: 0.0, point: chord.end, bed_m: b.bed_m, keep: true, station: true };
    if let Some(found) = find_fall(ground, params, &at, &previous, &into_end) {
        segment.insert_fall(found, &mut into_end);
    }
    segment
}

/// Ruling FF-2: no candidate at this station was allowed (inland, they were all at or below the
/// datum, or the ground there was not a number). Rather than hold the line where it is -- which
/// left stations on sea ground up to 100 km sideways, on a coast the tracer had wandered onto --
/// step back toward the chord in half-spacing increments and take the first lateral whose ground
/// is allowed, trying the chord point itself last.
///
/// If even the chord point is at or below the datum (a coarse chord across a bay), the chord point
/// is kept: that is Ruling R-3a, the one recorded exception to R-3. Ground that is not a number
/// anywhere along the step back leaves the bed where it is (`hold_m`, the caller's current bed
/// plus the channel's depth), the same fallback this arm has always used.
fn step_back(ground: &Ground, at: &dyn Fn(f64, f64) -> SpherePoint, along: f64, lateral: f64, limit: f64, spacing: f64, shore: Option<f64>, hold_m: f64) -> (f64, f64, SpherePoint) {
    let from = if lateral > limit { limit } else if lateral < -limit { -limit } else { lateral };
    let inward = if from > 0.0 { -0.5 * spacing } else { 0.5 * spacing };
    let steps = if spacing > 0.0 {
        let wanted = (if from > 0.0 { from } else { -from }) / (0.5 * spacing);
        if wanted > MAX_STATIONS { MAX_STATIONS } else { wanted }
    } else {
        0.0
    };
    let n = steps as usize; // cast-ok: a whole number in 0..=MAX_STATIONS
    for i in 0..=n {
        let o = from + inward * i as f64; // cast-ok: i <= MAX_STATIONS
        // Past the chord: it is tried last, below.
        if (from > 0.0 && o <= 0.0) || (from < 0.0 && o >= 0.0) {
            break;
        }
        let p = at(along, o);
        let g = (ground.height_m)(&p);
        if g.is_finite() && (shore.is_some() || g > 0.0) {
            return (g, o, p);
        }
    }
    let p = at(along, 0.0);
    let g = (ground.height_m)(&p);
    (if g.is_finite() { g } else { hold_m }, 0.0, p)
}

/// Every coarse segment of one reach, traced with the meander suppressed. Always one entry per
/// coarse segment: only a reach's *last* segment is given a `shore`, so only it can end at a
/// mouth, and the count never depends on what the ground turned out to be.
pub fn trace_reach(reach: &ReachLine, shore: Option<f64>, ground: &Ground, params: &HydroParams) -> Vec<Segment> {
    let coarse = &reach.points;
    (0..coarse.len().saturating_sub(1))
        .map(|s| {
            let here_shore = if s + 2 == coarse.len() { shore } else { None };
            trace(ground, params, &coarse[s], &coarse[s + 1], here_shore, false)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;

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

    /// Ruling FF-2: when every candidate at a station is at or below the datum, the tracer steps
    /// back toward its chord rather than holding the line where it is -- which used to leave an
    /// inland station on sea ground tens of kilometres sideways, on a coast.
    #[test]
    fn a_blocked_station_steps_back_toward_the_chord() {
        // A valley 5 km north of the chord, crossed at 14-16 km east by an arm of sea that
        // reaches to within 600 m of the chord.
        let h = |p: &SpherePoint| {
            let (n, e) = north_east(p);
            let off = if n > 5_000.0 { n - 5_000.0 } else { 5_000.0 - n };
            if e > 14_000.0 && e < 16_000.0 && n > 600.0 { -10.0 } else { 100.0 - 0.001 * e + 0.02 * off }
        };
        let (a, b) = ends(99.0, 69.0);
        let seg = trace_segment(&ground(&h), &params(), &a, &b, None);
        let mut stepped_back = 0usize;
        for f in seg.interior.iter().filter(|f| f.station) {
            assert!(h(&f.point) > 0.0, "station at {} m, {} m sideways is on sea ground",
                    f.along_m, f.lateral_m);
            if f.along_m > 14_000.0 && f.along_m < 16_000.0 {
                assert!(f.lateral_m < 1_000.0, "the blocked station is still {} m sideways", f.lateral_m);
                stepped_back += 1;
            }
        }
        assert!(stepped_back > 0, "at least one station is in the arm of sea");
    }

    /// Ruling R-3a, the one recorded exception to R-3: when even the chord point is at or below
    /// the datum (a coarse chord across a bay), the tracer keeps the chord point.
    #[test]
    fn a_station_whose_chord_point_is_sea_keeps_the_chord_point() {
        let h = |p: &SpherePoint| {
            let (_, e) = north_east(p);
            if e > 14_000.0 && e < 16_000.0 { -10.0 } else { 100.0 - 0.001 * e }
        };
        let (a, b) = ends(99.0, 69.0);
        let seg = trace_segment(&ground(&h), &params(), &a, &b, None);
        let across: Vec<&Fine> = seg.interior.iter()
            .filter(|f| f.station && f.along_m > 14_000.0 && f.along_m < 16_000.0).collect();
        assert!(!across.is_empty(), "at least one station is in the bay");
        for f in across {
            assert_eq!(f.lateral_m, 0.0, "the station at {} m is the chord point itself", f.along_m);
            assert!(h(&f.point) <= 0.0, "sanity: the chord point is the sea here");
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
}
