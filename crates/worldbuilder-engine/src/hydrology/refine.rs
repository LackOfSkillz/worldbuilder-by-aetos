//! Refinement (spec §6.6): every coarse reach re-traced on the landform at fine steps.
//!
//! A coarse segment runs from one reach point to the next, about one graph spacing long. Each
//! is walked in `refine_step_m` stations along its chord. At each station the tracer looks a
//! little to either side and takes the lowest ground, so the line settles into the valley floor
//! the coarse graph only saw every few tens of kilometres. It never leaves the corridor (one
//! graph spacing either side of the chord). It never steps onto ground at or below the datum
//! before its mouth (Ruling R-3), stepping back toward its chord where it must, and keeping the
//! chord point where even that is water (Ruling R-3a). It always arrives back on the next coarse
//! point. Coarse points are kept exactly, so a tributary still ends on its receiver's first
//! vertex (Ruling R-1, spec §14.4).
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
use crate::surface::Surface;
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

impl<'a> Ground<'a> {
    /// The `Ground` a bake traces on: the surface's landform (never its detail noise), its radius,
    /// a corridor of one nominal graph spacing at this node count, and the world's own seed. The
    /// caller owns the height closure, because it borrows the surface.
    pub fn for_surface(surface: &Surface, height_m: &'a dyn Fn(&SpherePoint) -> f64, params: &HydroParams) -> Ground<'a> {
        Ground {
            height_m,
            radius_m: surface.radius_m,
            corridor_m: crate::stream::nominal_spacing_m(params.total_nodes, surface.radius_m),
            seed: surface.world_seed as u64, // cast-ok: two's-complement reinterpretation, as Surface::new makes
        }
    }
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
    /// A station the tracer chose, rather than an end interpolated inside a step for a fall.
    /// Rulings R-3 and R-3a are about stations: they are what the lowest-ground search picks.
    pub station: bool,
}

/// A fall found on one segment. Its upper end is named by position, never by coordinates, so
/// `refine_reach` can copy `Fall.at` from the very `ReachPoint` it pushed there (Ruling FF-1): a
/// sphere round trip of a coarse start's latitude and longitude is not always exact.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SegmentFall {
    /// The upper end: an index into `Segment::interior`, or `None` for the segment's own start.
    pub upper: Option<usize>,
    pub height_m: f64,
}

/// What one segment traced to: its interior points; the mouth that replaces the coarse end, on a
/// last segment that reached the shore first; and any falls.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub interior: Vec<Fine>,
    pub mouth: Option<Fine>,
    pub falls: Vec<SegmentFall>,
}

impl Segment {
    /// Records a fall `find_fall` returned on the step from the last point so far (the segment's
    /// start if there is none yet) to `to`, which the caller pushes next: inserts the fall's upper
    /// end, or marks the point it starts at as kept (Ruling P-1); then inserts its lower end, or
    /// marks `to` as that end when the fall's window ends there (Ruling FF-4).
    fn insert_fall(&mut self, found: (Option<Fine>, Option<Fine>, f64), to: &mut Fine) {
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

/// One refined reach: its points, which of them simplification must keep, which coarse segment
/// each came from, and its falls.
#[derive(Debug, Clone, PartialEq)]
pub struct Refined {
    pub points: Vec<ReachPoint>,
    pub protected: Vec<bool>,
    /// Parallel to `points`: the coarse segment the polyline segment *leaving* that point lies
    /// in. That is what the crossing pass needs -- a `Crossing` names the first point of a
    /// crossing polyline segment, and Ruling S-3 straightens the whole coarse segment it is
    /// inside. A coarse point therefore names the segment it starts, not the one it ends, and
    /// the very last point (which leaves nothing) names the segment it ends.
    pub segment_of: Vec<u32>,
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

/// One coarse segment's chord, and the map from a station's `(along, lateral)` in metres to a
/// point on the sphere. `trace` and `meander` both need it, and neither should build it twice.
struct Chord {
    start: SpherePoint,
    end: SpherePoint,
    frame: TangentFrame,
    ux: f64,
    uy: f64,
    vx: f64,
    vy: f64,
    len_m: f64,
}

impl Chord {
    fn new(ground: &Ground, a: &ReachPoint, b: &ReachPoint) -> Chord {
        let start = SpherePoint::from_latlon(a.lat_deg, a.lon_deg);
        let end = SpherePoint::from_latlon(b.lat_deg, b.lon_deg);
        let frame = TangentFrame::at(&start, ground.radius_m);
        let (bx, by) = frame.sphere_to_local(&end);
        let len_m = m::hypot(bx, by);
        let (ux, uy) = (bx / len_m, by / len_m);
        Chord { start, end, frame, ux, uy, vx: -uy, vy: ux, len_m }
    }

    fn at(&self, along_m: f64, lateral_m: f64) -> SpherePoint {
        self.frame.local_to_sphere(self.ux * along_m + self.vx * lateral_m,
                                   self.uy * along_m + self.vy * lateral_m)
    }
}

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
fn trace(ground: &Ground, params: &HydroParams, a: &ReachPoint, b: &ReachPoint, shore: Option<f64>, straight: bool) -> Segment {
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

/// Ruling R-6: a meander only where it can be drawn at this step (a wavelength of at least
/// four steps), where the river is flat (bed slope under `meander_max_slope`), and where no
/// fall was found. It is tapered to zero at both coarse points and kept inside the corridor.
/// It moves the line, never the bed.
///
/// A segment trimmed at the shore is left alone: `trace` returns at its mouth, and a mouth is a
/// position on the water's edge, not a line to be decorated.
fn meander(segment: &mut Segment, ground: &Ground, params: &HydroParams, a: &ReachPoint, b: &ReachPoint) {
    if segment.mouth.is_some() || !segment.falls.is_empty() || segment.interior.is_empty() {
        return;
    }
    let chord = Chord::new(ground, a, b);
    let len = chord.len_m;
    let floor_m = if b.bed_m < a.bed_m { b.bed_m } else { a.bed_m };
    let wavelength = params.meander_wavelength_widths * a.width_m;
    let slope = (a.bed_m - floor_m) / len;
    let amplitude = params.meander_amplitude_widths * a.width_m;
    if !(slope < params.meander_max_slope && wavelength >= 4.0 * params.refine_step_m && amplitude > 0.0) {
        return;
    }
    let v = chord.start.vector;
    let n = Noise::new(ground.seed, MEANDER_SALT)
        .at(v.x * MEANDER_FREQUENCY, v.y * MEANDER_FREQUENCY, v.z * MEANDER_FREQUENCY);
    if !n.is_finite() {
        return;
    }
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
        fine.point = chord.at(fine.along_m, fine.lateral_m);
    }
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
fn find_fall(ground: &Ground, params: &HydroParams, at: &dyn Fn(f64, f64) -> SpherePoint, from: &Fine, to: &Fine) -> Option<(Option<Fine>, Option<Fine>, f64)> {
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

/// Ruling R-7: Douglas–Peucker over one refined reach. Between two kept points, the point that
/// strays furthest -- sideways from their chord, in units of `refine_simplify_m`, or off their
/// straight-line bed, in units of `refine_vertical_m`, whichever is worse -- is kept if it strays
/// more than one unit, and the two halves are examined in turn. Protected points (coarse points,
/// fall ends, the mouth) and both ends are always kept; a `protected` shorter than `points` simply
/// protects nothing past its end. Keeping a subset of a falling bed keeps it
/// falling, so spec §14.5 survives. The outcome does not depend on the order spans are examined.
pub fn simplify(points: &[ReachPoint], protected: &[bool], radius_m: f64, params: &HydroParams) -> Vec<ReachPoint> {
    let n = points.len();
    if n <= 2 {
        return points.to_vec();
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
    points.iter().zip(&keep).filter(|(_, &k)| k).map(|(p, _)| p.clone()).collect()
}

use crate::hydrology::buckets::BucketIndex;

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

fn fine_point(fine: &Fine, like: &ReachPoint) -> ReachPoint {
    let (lat_deg, lon_deg) = fine.point.to_latlon();
    ReachPoint { lat_deg, lon_deg, bed_m: fine.bed_m, width_m: like.width_m, depth_m: like.depth_m, flow_m2: like.flow_m2 }
}

/// Every coarse segment of one reach, traced with the meander suppressed. Always one entry per
/// coarse segment: only a reach's *last* segment is given a `shore`, so only it can end at a
/// mouth, and the count never depends on what the ground turned out to be.
fn trace_reach(reach: &ReachLine, shore: Option<f64>, ground: &Ground, params: &HydroParams) -> Vec<Segment> {
    let coarse = &reach.points;
    (0..coarse.len().saturating_sub(1))
        .map(|s| {
            let here_shore = if s + 2 == coarse.len() { shore } else { None };
            trace(ground, params, &coarse[s], &coarse[s + 1], here_shore, false)
        })
        .collect()
}

/// Strings one reach's traced segments into a line: coarse points kept exactly, fine points
/// between them, trimmed at the shore on its last segment. Interior points carry their segment's
/// upstream width, depth and flow. Flow only steps at a coarse point, where a tributary joins.
fn assemble(reach: &ReachLine, segments: &[Segment], shore: Option<f64>) -> Refined {
    let coarse = &reach.points;
    let mut refined = Refined { points: Vec::new(), protected: Vec::new(), segment_of: Vec::new(), falls: Vec::new() };
    if coarse.is_empty() {
        return refined;
    }
    refined.points.push(coarse[0].clone());
    refined.protected.push(true);
    refined.segment_of.push(0);
    for (s, segment) in segments.iter().enumerate() {
        let a = &coarse[s];
        let b = &coarse[s + 1];
        let last = s + 2 == coarse.len();
        // `a` is the point pushed last; the segment's interior follows it.
        let start = refined.points.len() - 1;
        for fine in &segment.interior {
            refined.points.push(fine_point(fine, a));
            refined.protected.push(fine.keep);
            refined.segment_of.push(s as u32); // cast-ok: a coarse segment index, bounded by the reach's point count
        }
        for fall in &segment.falls {
            let upper = &refined.points[fall.upper.map_or(start, |i| start + 1 + i)];
            refined.falls.push(Fall { reach: reach.id, at: (upper.lat_deg, upper.lon_deg), height_m: fall.height_m });
        }
        if let Some(mouth) = segment.mouth {
            refined.points.push(fine_point(&mouth, b));
            refined.protected.push(true);
            refined.segment_of.push(s as u32); // cast-ok: as above
            break;
        }
        let mut end = b.clone();
        if last && shore.is_some() {
            // Ruling R-4: a mouth's bed never rises above the bed that reaches it.
            let before = refined.points.last().expect("at least the first point").bed_m;
            if before < end.bed_m {
                end.bed_m = before;
            }
        }
        refined.points.push(end);
        refined.protected.push(true);
        // A coarse point's outgoing polyline segment is the start of the *next* coarse segment.
        let next = if s + 1 < segments.len() { s + 1 } else { s };
        refined.segment_of.push(next as u32); // cast-ok: as above
    }
    refined
}

/// Refines one reach: traced, meandered and strung together. The crossing pass does not run
/// here -- it is between reaches, and `refine` owns it.
pub fn refine_reach(reach: &ReachLine, shore: Option<f64>, ground: &Ground, params: &HydroParams) -> Refined {
    let mut segments = trace_reach(reach, shore, ground, params);
    for (s, segment) in segments.iter_mut().enumerate() {
        meander(segment, ground, params, &reach.points[s], &reach.points[s + 1]);
    }
    assemble(reach, &segments, shore)
}

/// Rulings S-3 and S-4: where two reaches cross, the one carrying less flow at the crossing
/// yields -- its whole coarse segment goes back to its chord, so it cannot bend into anything new
/// -- and the pass repeats until nothing crosses or `MAX_CROSSING_PASSES` is done. Ties go to the
/// larger reach id. Ruling S-2: a crossing the coarse record already had cannot be straightened
/// away, which is why the count that is left is recorded rather than asserted to be zero.
const MAX_CROSSING_PASSES: usize = 3;

/// Refines every reach in the record, in reach order, and records the falls in the same order.
///
/// Ruling S-4 sets the order: trace with the meander suppressed, run the crossing pass, then
/// meander and simplify. A meander is cosmetic, and straightening one away where it is not the
/// cause of a crossing would cost a river its shape for nothing.
///
/// Two counts go into the record. `crossings_coarse` is measured on the coarse lines before
/// anything is traced: those are graph artifacts refinement did not make and does not fix
/// (Ruling S-2). `crossings_left` is measured on the lines as they ship, after the meander and
/// simplification, so it is the record's own honest count and not a mid-pass number.
pub fn refine(record: &mut HydroRecord, ground: &Ground, params: &HydroParams) {
    let shores: Vec<Option<f64>> = record.reaches.iter().map(|r| terminal_level(r, &record.bodies)).collect();
    let downstream: Vec<Downstream> = record.reaches.iter().map(|r| r.downstream).collect();
    let coarse_lines: Vec<Vec<ReachPoint>> = record.reaches.iter().map(|r| r.points.clone()).collect();
    let crossings_coarse = crossings(&coarse_lines, &downstream, ground.radius_m).len();

    let mut traced: Vec<Vec<Segment>> = Vec::with_capacity(record.reaches.len());
    let mut yielded: Vec<Vec<bool>> = Vec::with_capacity(record.reaches.len());
    for (reach, &shore) in record.reaches.iter().zip(&shores) {
        let segments = trace_reach(reach, shore, ground, params);
        yielded.push(vec![false; segments.len()]);
        traced.push(segments);
    }
    let mut refineds: Vec<Refined> = record.reaches.iter().zip(&traced).zip(&shores)
        .map(|((reach, segments), &shore)| assemble(reach, segments, shore))
        .collect();

    for _ in 0..MAX_CROSSING_PASSES {
        let lines: Vec<Vec<ReachPoint>> = refineds.iter().map(|r| r.points.clone()).collect();
        let found = crossings(&lines, &downstream, ground.radius_m);
        if found.is_empty() {
            break;
        }
        // `(reach, coarse segment)` pairs to straighten. `crossings` is already in a fixed order
        // and this sort is total, so the set and the order it is applied in are the same run to
        // run.
        let mut giving: Vec<(usize, usize)> = Vec::with_capacity(found.len());
        for c in &found {
            let (ra, rb) = (c.reach_a as usize, c.reach_b as usize);
            // cast-ok: a coarse segment index, bounded by the reach's own point count
            let (sa, sb) = (refineds[ra].segment_of[c.index_a] as usize, refineds[rb].segment_of[c.index_b] as usize);
            let flow_a = refineds[ra].points[c.index_a].flow_m2;
            let flow_b = refineds[rb].points[c.index_b].flow_m2;
            // Ruling S-3: the smaller flow gives way. On a tie the larger reach id keeps its
            // valley, and `reach_a` is always the smaller id, so it is the one that yields.
            let (yielder, other) = if flow_b < flow_a { ((rb, sb), (ra, sa)) } else { ((ra, sa), (rb, sb)) };
            // Ruling S-4's repeat only means anything if a later pass can decide differently. A
            // segment already on its chord has nothing left to give, so where the smaller flow
            // has yielded and the two still cross, the larger one yields next. Without this the
            // second and third passes re-take the first pass's decision and the crossing stands:
            // 2 of the junction world's 14 survived that way, and none survive this.
            giving.push(if yielded[yielder.0][yielder.1] { other } else { yielder });
        }
        giving.sort_unstable();
        giving.dedup();
        let mut moved: Vec<usize> = Vec::new();
        for (r, s) in giving {
            if yielded[r][s] {
                // Already on its chord: it has nothing more to give, and re-tracing it would
                // only spend the pass budget.
                continue;
            }
            yielded[r][s] = true;
            let coarse = &record.reaches[r].points;
            let here_shore = if s + 2 == coarse.len() { shores[r] } else { None };
            traced[r][s] = trace(ground, params, &coarse[s], &coarse[s + 1], here_shore, true);
            moved.push(r);
        }
        if moved.is_empty() {
            break;
        }
        moved.dedup();
        for r in moved {
            refineds[r] = assemble(&record.reaches[r], &traced[r], shores[r]);
        }
    }

    let mut falls = Vec::new();
    for (r, reach) in record.reaches.iter_mut().enumerate() {
        for (s, segment) in traced[r].iter_mut().enumerate() {
            meander(segment, ground, params, &reach.points[s], &reach.points[s + 1]);
        }
        let refined = assemble(reach, &traced[r], shores[r]);
        reach.points = simplify(&refined.points, &refined.protected, ground.radius_m, params);
        falls.extend(refined.falls);
    }
    record.falls = falls;

    let lines: Vec<Vec<ReachPoint>> = record.reaches.iter().map(|r| r.points.clone()).collect();
    let crossings_left = crossings(&lines, &downstream, ground.radius_m).len();
    record.stats.crossings_coarse = crossings_coarse as u32; // cast-ok: bounded by the segment count, which is bounded by the record's point count
    record.stats.crossings_left = crossings_left as u32; // cast-ok: as above
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hydrology::reaches::ReachClass;
    use crate::hydrology::BakeStats;

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

    /// A `BakeStats` with every count zero and `p`'s numbers echoed: enough for a `HydroRecord`
    /// a refinement test drives directly, with no bake behind it.
    fn stats_for(p: &HydroParams) -> BakeStats {
        BakeStats {
            nodes: 0, land_nodes: 0, hollows: 0, kept: 0, notched: 0, closed: 0,
            streams: 0, rivers: 0, great: 0, max_order: 0,
            bifurcation_min: 0.0, bifurcation_max: 0.0,
            stream_flow_m2: p.stream_flow_m2, river_flow_m2: p.river_flow_m2, great_flow_m2: p.great_flow_m2,
            total_nodes: p.total_nodes, wetness_nodes: p.wetness_nodes,
            keep_depth_m: p.keep_depth_m, keep_area_m2: p.keep_area_m2,
            pond_max_area_m2: p.pond_max_area_m2, keep_max_area_m2: p.keep_max_area_m2,
            min_stream_nodes: p.min_stream_nodes, notch_fall_m: p.notch_fall_m,
            evaporation_factor: p.evaporation_factor, salt_flat_share: p.salt_flat_share,
            forced_requested: 0, forced_matched: 0,
            capped_basins: 0, capped_inner: 0, capped_inner_kept: 0,
            refine_step_m: p.refine_step_m, refine_simplify_m: p.refine_simplify_m,
            refine_vertical_m: p.refine_vertical_m,
            fall_min_drop_m: p.fall_min_drop_m, fall_max_run_m: p.fall_max_run_m,
            meander_wavelength_widths: p.meander_wavelength_widths,
            meander_amplitude_widths: p.meander_amplitude_widths,
            meander_max_slope: p.meander_max_slope,
            crossings_coarse: 0, crossings_left: 0,
        }
    }

    /// Rulings S-3 and S-4: where two reaches cross, the smaller flow yields its whole coarse
    /// segment back to its chord, and the pass repeats until nothing crosses or three passes are
    /// done.
    #[test]
    fn the_smaller_river_yields_its_segment() {
        // Ground with a valley that pulls both reaches north of their chords, so their traced
        // lines cross even though their chords do not.
        let h = |p: &SpherePoint| {
            let (n, e) = north_east(p);
            let off = if n > 6_000.0 { n - 6_000.0 } else { 6_000.0 - n };
            200.0 - 0.001 * e + 0.02 * off
        };
        let ground = Ground { height_m: &h, radius_m: R, corridor_m: 20_000.0, seed: 3 };
        let big = ReachLine {
            id: 0, class: ReachClass::Great, order: 3, downstream: Downstream::Ocean, fresh: true,
            points: vec![wide(0.0, 0.0, 199.0, 900.0), wide(0.0, 30_000.0 / M_PER_DEG, 169.0, 900.0)],
        };
        // The brief's own chords (0.1 N to 0.1 S) cross each other, which Ruling S-2 keeps and no
        // straightening can remove. These two are PARALLEL -- the great river along the equator,
        // the stream 11,200 m north of it -- so the only crossing is the one the valley makes.
        // The valley is 6,000 m north: the great river climbs 750 m a station and lands on it
        // exactly, the stream comes down from 11,200 m and overshoots to 5,950 m (11,200 is
        // 5,200 past the valley, and 5,200 is 700 past seven whole steps, so the eighth step is
        // worth taking). That 50 m is the great river passing the stream.
        let small = ReachLine {
            id: 1, class: ReachClass::Stream, order: 1, downstream: Downstream::Ocean, fresh: true,
            points: vec![point(11_200.0 / M_PER_DEG, 0.0, 199.0),
                         point(11_200.0 / M_PER_DEG, 30_000.0 / M_PER_DEG, 169.0)],
        };
        let mut record = HydroRecord {
            bodies: Vec::new(),
            reaches: vec![big.clone(), small.clone()],
            notches: Vec::new(),
            falls: Vec::new(),
            stats: stats_for(&params()),
        };
        refine(&mut record, &ground, &params());
        let lines: Vec<Vec<ReachPoint>> = record.reaches.iter().map(|r| r.points.clone()).collect();
        let down: Vec<Downstream> = record.reaches.iter().map(|r| r.downstream).collect();
        assert!(crossings(&lines, &down, R).is_empty(), "the pass left a crossing");
        assert_eq!(record.stats.crossings_left, 0);
        // The brief's `crossings_coarse >= 0` is always true of a `u32` and warns; the fixture's
        // two chords do not cross, so the count it should have is nailed down instead.
        assert_eq!(record.stats.crossings_coarse, 0, "the two coarse chords do not cross");
        // The great river kept its valley; the stream was straightened to its chord. The brief's
        // 0.01 deg is not a threshold this fixture can use: the great river's own meander is 1.5
        // widths, 1,350 m, or 0.0121 deg, so a river that HAD been straightened would still clear
        // it and the mutation guard would pass. 0.03 deg is 3,336 m -- above anything the meander
        // alone can reach and well below the 6,000 m valley. Measured: 0.044, 0.052, 0.046.
        let great_offsets = record.reaches[0].points.iter().skip(1).take(3)
            .map(|p| p.lat_deg).any(|lat| lat > 0.03);
        assert!(great_offsets, "the larger river keeps its valley");
        // And the stream is on its chord, to within the chord's own great-circle sagitta.
        for p in &record.reaches[1].points {
            let off = (p.lat_deg - 11_200.0 / M_PER_DEG) * M_PER_DEG;
            assert!(off < 10.0 && off > -10.0, "the stream is {off} m off its chord");
        }
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
