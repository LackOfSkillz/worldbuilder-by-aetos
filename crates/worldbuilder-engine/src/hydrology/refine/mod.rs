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
//!
//! This module is the shape of a refinement -- the ground it runs on, the points it makes, the
//! reach it assembles -- and the pass that drives the whole record. The stages live beside it:
//! `trace` walks a coarse segment, `falls` finds the steps in it, `meander` shapes a flat one,
//! `simplify` cuts the line down, and `crossings` is the between-reach check the pass repeats.

use crate::detmath as m;
use crate::hydrology::{Body, Downstream, Fall, HydroParams, HydroRecord, ReachLine, ReachPoint};
use crate::sphere::SpherePoint;
use crate::surface::Surface;
use crate::tangent::TangentFrame;

mod crossings;
mod falls;
mod meander;
mod simplify;
#[cfg(test)]
mod test_support;
mod trace;

pub use crossings::{crossings, Crossing};
pub use simplify::simplify;
pub use trace::trace_segment;

use meander::meander;
use simplify::simplify_mask;
use trace::{trace, trace_reach};

/// A tracer never plans more stations (or fall windows) than this on one segment, whatever the
/// params ask.
const MAX_STATIONS: f64 = 100_000.0;

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
/// point on the sphere.
///
/// `trace` and `meander` each build their own rather than sharing one. That is a second
/// `TangentFrame::at` per segment, and it is deliberate: threading the chord out of `trace` would
/// put it in the return type of a function whose result is a `Segment`, for a saving of one frame
/// construction on the segments that actually meander -- which, after Rulings R-6 and S-4a, is
/// neither the ones with falls, nor the ones trimmed at a shore, nor the ones that yielded.
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

fn fine_point(fine: &Fine, like: &ReachPoint) -> ReachPoint {
    let (lat_deg, lon_deg) = fine.point.to_latlon();
    ReachPoint { lat_deg, lon_deg, bed_m: fine.bed_m, width_m: like.width_m, depth_m: like.depth_m, flow_m2: like.flow_m2 }
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
///
/// **Four, not S-4's three, and that is a measurement.** Once Ruling S-14 moved the check on to
/// the shipped lines, the pass has the meander's own crossings to clear as well, and the descent
/// on the 1M-node stand-ins is 2,909 -> 1,204 -> 118 -> 55 -> 50 (seed 1 `ranges`) and
/// 1,493 -> 606 -> 54 -> 32 -> 28 (the bake test world). Three passes stop at 55 against 54
/// coarse -- over by one, and the guarantee broken. The fourth is where both converge: a fifth
/// straightening round moves nothing at all, so the loop would break on its own.
const MAX_CROSSING_PASSES: usize = 4;

/// One reach's line as the record would ship it: its traced segments meandered where Ruling S-4a
/// allows, strung together, and simplified. `segment_of` is cut down with the points, because
/// Ruling S-14 has the crossing pass read it off this line and not off the unsimplified one.
///
/// The traced segments are borrowed, never meandered in place: a later pass builds its line from
/// the same trace rather than compounding a second meander on the last pass's.
fn ship(reach: &ReachLine, segments: &[Segment], yielded: &[bool], shore: Option<f64>, ground: &Ground, params: &HydroParams) -> Refined {
    let mut shaped: Vec<Segment> = Vec::with_capacity(segments.len());
    for (s, segment) in segments.iter().enumerate() {
        let mut shape = segment.clone();
        // Ruling S-4a: a segment that yielded is not meandered. The meander is worth up to
        // `meander_amplitude_widths` channel widths of lateral shift, which on a wide reach is
        // the same order as what the pass straightened away -- so meandering a yielded segment
        // would make Ruling S-3's "every interior station's lateral set to 0" true of the pass
        // and false of the record. The crossing fix outranks a cosmetic meander on one segment.
        // The cost: a yielded flat wide segment ships dead straight, for about one graph spacing.
        if !yielded[s] {
            meander(&mut shape, ground, params, &reach.points[s], &reach.points[s + 1]);
        }
        shaped.push(shape);
    }
    let mut refined = assemble(reach, &shaped, shore);
    let keep = simplify_mask(&refined.points, &refined.protected, ground.radius_m, params);
    let mut points = Vec::new();
    let mut protected = Vec::new();
    let mut segment_of = Vec::new();
    for (i, &k) in keep.iter().enumerate() {
        if k {
            points.push(refined.points[i].clone());
            protected.push(refined.protected[i]);
            segment_of.push(refined.segment_of[i]);
        }
    }
    refined.points = points;
    refined.protected = protected;
    refined.segment_of = segment_of;
    refined
}

/// Refines every reach in the record, in reach order, and records the falls in the same order.
///
/// **Ruling S-14: the crossing guarantee is about the lines the record ships.** The pass sits at
/// the end of the pipeline -- trace, meander, simplify, then check -- and repeats up to
/// `MAX_CROSSING_PASSES`. It used to sit between tracing and the meander (Ruling S-4), which left
/// two stages free to move a line after the last check had passed: a meander of up to 1.5 channel
/// widths, and Douglas-Peucker at `refine_simplify_m`. On the 1M-node stand-ins that put the
/// shipped record ABOVE the coarse record it came from -- 36 against 33, 5 against 4, 57 against
/// 54 -- which is the opposite of what the pass exists to promise. Ruling S-4a still stands
/// inside the loop: a segment that yields is straightened, skips the meander, and is simplified
/// like any other.
///
/// Two counts go into the record. `crossings_coarse` is measured on the coarse lines before
/// anything is traced: those are graph artifacts refinement did not make and does not fix
/// (Ruling S-2). `crossings_left` is the count over the shipped lines -- the very
/// `record.reaches` this function leaves behind.
pub fn refine(record: &mut HydroRecord, ground: &Ground, params: &HydroParams) {
    let shores: Vec<Option<f64>> = record.reaches.iter().map(|r| terminal_level(r, &record.bodies)).collect();
    let downstream: Vec<Downstream> = record.reaches.iter().map(|r| r.downstream).collect();
    let crossings_coarse = {
        let coarse_lines: Vec<&[ReachPoint]> = record.reaches.iter().map(|r| r.points.as_slice()).collect();
        crossings(&coarse_lines, &downstream, ground.radius_m).len()
    };

    let mut traced: Vec<Vec<Segment>> = Vec::with_capacity(record.reaches.len());
    let mut yielded: Vec<Vec<bool>> = Vec::with_capacity(record.reaches.len());
    for (reach, &shore) in record.reaches.iter().zip(&shores) {
        let segments = trace_reach(reach, shore, ground, params);
        yielded.push(vec![false; segments.len()]);
        traced.push(segments);
    }
    let mut shipped: Vec<Refined> = (0..record.reaches.len())
        .map(|r| ship(&record.reaches[r], &traced[r], &yielded[r], shores[r], ground, params))
        .collect();

    let mut crossings_left;
    let mut pass = 0usize;
    loop {
        // The shipped points are borrowed, never copied: this loop runs up to five times and a
        // copy of every point each time is the pass's largest avoidable cost. The borrow ends
        // with the block, so `shipped` is free to be read and re-shipped below.
        let found = {
            let lines: Vec<&[ReachPoint]> = shipped.iter().map(|r| r.points.as_slice()).collect();
            crossings(&lines, &downstream, ground.radius_m)
        };
        crossings_left = found.len();
        if found.is_empty() || pass == MAX_CROSSING_PASSES {
            break;
        }
        pass += 1;
        // `(reach, coarse segment)` pairs to straighten. `crossings` is already in a fixed order
        // and this sort is total, so the set and the order it is applied in are the same run to
        // run.
        let mut giving: Vec<(usize, usize)> = Vec::with_capacity(found.len());
        for c in &found {
            let (ra, rb) = (c.reach_a as usize, c.reach_b as usize);
            // cast-ok: a coarse segment index, bounded by the reach's own point count
            let (sa, sb) = (shipped[ra].segment_of[c.index_a] as usize, shipped[rb].segment_of[c.index_b] as usize);
            let flow_a = shipped[ra].points[c.index_a].flow_m2;
            let flow_b = shipped[rb].points[c.index_b].flow_m2;
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
            // Nothing was straightened, so the next pass would index and test the very same
            // lines and find the very same crossings. `crossings_left` above already counts
            // them, so the loop stops here rather than paying for that call.
            break;
        }
        moved.dedup();
        for r in moved {
            shipped[r] = ship(&record.reaches[r], &traced[r], &yielded[r], shores[r], ground, params);
        }
    }

    let mut falls = Vec::new();
    for (reach, refined) in record.reaches.iter_mut().zip(shipped) {
        reach.points = refined.points;
        falls.extend(refined.falls);
    }
    record.falls = falls;
    record.stats.crossings_coarse = crossings_coarse as u32; // cast-ok: bounded by the segment count, which is bounded by the record's point count
    record.stats.crossings_left = crossings_left as u32; // cast-ok: as above
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;
    use crate::hydrology::reaches::ReachClass;
    use crate::hydrology::BakeStats;

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
            ponds_found: 0, ponds_kept: 0,
            pond_cell_m: p.pond_cell_m, pond_search_radius_m: p.pond_search_radius_m,
            pond_keep_depth_m: p.pond_keep_depth_m, pond_keep_area_m2: p.pond_keep_area_m2,
            pond_wetness_share: p.pond_wetness_share, pond_max_slope: p.pond_max_slope,
            pond_density_area_m2: p.pond_density_area_m2,
            shore_members: 0, collar_points: 0,
        }
    }

    /// Ground with a valley 6,000 m north of the equator, falling gently east. The brief's own
    /// ground for the crossing tests: it pulls two parallel reaches toward the same line, so
    /// their traced lines cross even though their chords do not.
    fn valley_6km(p: &SpherePoint) -> f64 {
        let (n, e) = north_east(p);
        let off = if n > 6_000.0 { n - 6_000.0 } else { 6_000.0 - n };
        200.0 - 0.001 * e + 0.02 * off
    }

    /// The great river of the crossing fixture: along the equator, 900 m wide, and by far the
    /// larger flow, so it is never the one that yields.
    fn great_along_the_equator() -> ReachLine {
        ReachLine {
            id: 0, class: ReachClass::Great, order: 3, downstream: Downstream::Ocean, fresh: true,
            points: vec![wide(0.0, 0.0, 199.0, 900.0), wide(0.0, 30_000.0 / M_PER_DEG, 169.0, 900.0)],
        }
    }

    /// The crossing fixture's second reach: PARALLEL to the great river, 11,200 m north of it, so
    /// the chords never cross and the only crossing is the one the valley makes.
    ///
    /// The brief's own chords (0.1 N to 0.1 S) cross each other, which Ruling S-2 keeps and no
    /// straightening can remove. With the valley 6,000 m north: the great river climbs 750 m a
    /// station and lands on it exactly, while this one comes down from 11,200 m and overshoots to
    /// 5,950 m (11,200 is 5,200 past the valley, and 5,200 is 700 past seven whole steps, so the
    /// eighth step is worth taking). That 50 m is the great river passing it.
    fn lesser_flow_north_of_it(width_m: f64) -> ReachLine {
        let at = |lon_deg: f64, bed_m: f64| ReachPoint {
            lat_deg: 11_200.0 / M_PER_DEG, lon_deg, bed_m, width_m, depth_m: 1.0, flow_m2: 1.0e9,
        };
        ReachLine {
            id: 1, class: ReachClass::Stream, order: 1, downstream: Downstream::Ocean, fresh: true,
            points: vec![at(0.0, 199.0), at(30_000.0 / M_PER_DEG, 169.0)],
        }
    }

    const LESSER_CHORD_LAT: f64 = 11_200.0 / M_PER_DEG;

    /// Rulings S-3 and S-4: where two reaches cross, the smaller flow yields its whole coarse
    /// segment back to its chord, and the pass repeats until nothing crosses or three passes are
    /// done.
    #[test]
    fn the_smaller_river_yields_its_segment() {
        let ground = Ground { height_m: &valley_6km, radius_m: R, corridor_m: 20_000.0, seed: 3 };
        let big = great_along_the_equator();
        let small = lesser_flow_north_of_it(10.0);
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
            let off = (p.lat_deg - LESSER_CHORD_LAT) * M_PER_DEG;
            assert!(off < 10.0 && off > -10.0, "the stream is {off} m off its chord");
        }
    }

    /// Ruling S-4a: a segment that yielded is not meandered. The meander is worth up to
    /// `meander_amplitude_widths` channel widths of lateral shift -- 1,500 m on this fixture --
    /// which is the same order as what the pass straightened away. Put it back and Ruling S-3's
    /// "every interior station's lateral set to 0" would be true at the end of the pass and false
    /// of the record as it ships.
    ///
    /// The yielding reach here is 1,000 m wide and flat, so it qualifies for a meander on every
    /// count, and carries a thousandth of the great river's flow, so it is still the one that
    /// gives way.
    #[test]
    fn a_yielded_segment_is_not_meandered() {
        let ground = Ground { height_m: &valley_6km, radius_m: R, corridor_m: 20_000.0, seed: 3 };
        let calm = lesser_flow_north_of_it(1_000.0);

        // Sanity: traced on its own, this reach really does meander -- some station is hundreds
        // of metres from where the same trace with the amplitude turned off would put it.
        let (a, b) = (&calm.points[0], &calm.points[1]);
        let mut no_meander = params();
        no_meander.meander_amplitude_widths = 0.0;
        let meandered = trace_segment(&ground, &params(), a, b, Some(0.0));
        let plain = trace_segment(&ground, &no_meander, a, b, Some(0.0));
        assert!(meandered.interior.iter().zip(&plain.interior).any(|(f, q)| {
            let d = f.lateral_m - q.lateral_m;
            d > 500.0 || d < -500.0
        }), "sanity: this reach qualifies for a meander");

        let mut record = HydroRecord {
            bodies: Vec::new(),
            reaches: vec![great_along_the_equator(), calm],
            notches: Vec::new(),
            falls: Vec::new(),
            stats: stats_for(&params()),
        };
        refine(&mut record, &ground, &params());
        assert_eq!(record.stats.crossings_left, 0, "the pass still clears the crossing");
        for p in &record.reaches[1].points {
            let off = (p.lat_deg - LESSER_CHORD_LAT) * M_PER_DEG;
            assert!(off < 10.0 && off > -10.0,
                    "a yielded segment shipped {off} m off its chord: the meander was put back");
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
