//! The spatial index over a baked record (spec §8.2): which bodies, reaches and notches can
//! possibly answer a sample, so the query tests a handful of them instead of the whole record.
//!
//! # It is a grid of cells, and it is [`BucketIndex`]'s grid
//!
//! Ruling Q-1: latitude rows and longitude columns sized so a cell is about `cell_m` square
//! everywhere, not spec §8.2's "cube-sphere cell grid". `BucketIndex` already draws that grid,
//! is deterministic, and is tested against brute force including the poles and the ±180 seam.
//! `BucketIndex` indexes *points*, though, and this index holds *areas*, so what is borrowed is
//! the arithmetic and nothing else: [`BucketIndex::cell_of`] says which cell a sample is in,
//! [`BucketIndex::cells_within`] says which cells a disc touches, and the payloads are this
//! module's own three `Vec<Vec<u32>>`.
//!
//! # What each cell lists, and why it is more than the items with a point in it
//!
//! - **A body** is listed in every cell within its **bounding circle**: the greatest distance
//!   from its `anchor` to any of its own recorded outline points, plus `shore_reach_m`. Ruling
//!   Q-13. One rule for all four kinds -- a shore-point set, a traced ring, and a band of zero
//!   are all covered by it, so there is nothing to branch on.
//!
//!   > **Guarantee.** Every point the query could answer for a body is in a cell that lists it.
//!
//!   Spec §8.3 admits a point three ways, and the circle covers all three. Clause 2 (within
//!   `shore_reach_m` of a **member**) reaches at most `span + band` from the anchor, because a
//!   member is one of the recorded points the span is measured over. The ring test (a traced
//!   curve) admits only points inside the curve, whose vertices are recorded points. Clause 1
//!   (the nearest **member** at least as near as the nearest **collar** point) is the one with
//!   no distance in it at all -- it is what holds a wide body's interior, arbitrarily far from
//!   anything recorded -- and what bounds it is that the **collar encloses the members**: it is
//!   built as the above-level neighbours of the member set, so from any point outside the
//!   outline the collar on that side is nearer than the members behind it and clause 1 fails.
//!   Everything clause 1 admits therefore lies within the outline, and so within the circle.
//!
//!   That last step is a property of what the bake records, not of arithmetic: a hand-written
//!   record whose collar did not enclose its members could admit a point outside the circle.
//!   No bake produces one.
//!
//!   **Why not a band around each shore member**, which is what spec §8.2 describes and what
//!   this index shipped with first: it satisfies clause 2 and nothing else. A body wider than
//!   about twice its band has interior cells that list it nowhere, and the query answers `Ocean`
//!   there. The owner's great lake is 3,627 km across with a 58 km band, so a query in the
//!   middle of it answered `Ocean` over a region 1,700 km wide. Task 6 corrects §8.2's text.
//! - **A reach or a notch** is listed in every cell within half its width of its centre line,
//!   along every recorded leg. Ruling Q-7: the width of a leg is the **larger** of its two
//!   endpoints', because a leg tapers between recorded points and the smaller value would put
//!   the wide end of the taper outside the index.
//!
//! # How a leg's cells are enumerated, and what that guarantees
//!
//! Walking a leg cell by cell is exact and fiddly on a sphere; this samples instead. Each leg is
//! sampled at no more than **half a cell** apart, and every sample is dilated by half the width
//! **plus half of the widest gap between neighbouring samples on that leg** -- a gap that is
//! measured from the samples actually produced, not assumed from the step. So:
//!
//! > **Guarantee.** Every cell holding any point within half the width of the polyline is listed
//! > for that reach or notch.
//!
//! The proof is two steps. A point `x` within half the width of the line has a nearest point `y`
//! on it; `y` lies on some leg between two neighbouring samples, so it is within half that leg's
//! widest gap of one of them, call it `s`; hence `d(x, s) <= half_width + widest/2`, the reach
//! `s` was dilated by. And `cells_within` lists every cell holding a point within its reach (it
//! sweeps a whole row/column range, so it lists more than that and never less).
//!
//! Measuring the gap rather than assuming it is what makes the guarantee hold at the awkward
//! ends: samples are placed by normalising a linear blend of the two endpoints' vectors, which
//! lands exactly on the great circle but not at even angles along it, and a leg long enough for
//! that to matter reports the wider gap and is dilated by it. The cost of the choice is over-
//! inclusion of about a cell around each leg, which costs the query a candidate to reject and
//! can never cost it an answer.
//!
//! # Ids, order and determinism
//!
//! A cell's list is ascending and deduplicated -- built by pushing and sorting once at the end,
//! never through a `HashSet`, whose order is not ours to depend on. `bodies` and `reaches` carry
//! `Body::id` and `ReachLine::id`; a `NotchLine` has no id, so `notches` carries its position in
//! `HydroRecord::notches`.
//!
//! Ruling Q-2: this is derived state, built from a decoded record and never recorded or
//! transmitted.

use crate::detmath as m;
use crate::hydrology::buckets::BucketIndex;
use crate::hydrology::HydroRecord;
use crate::sphere::SpherePoint;

/// About 50 km, spec §8.2. A cell holds every item whose influence reaches it.
pub const DEFAULT_CELL_M: f64 = 50_000.0;

/// The most samples one recorded leg is cut into, however long it is. The dilation is measured
/// from the samples produced, so hitting this cap widens the band rather than tearing a hole in
/// it: the index stays correct and gets coarser. Nothing in a real record comes near it -- a
/// refined leg is a few hundred metres -- and it exists so a hand-written or hostile record with
/// a leg across half the planet cannot turn one segment into an unbounded allocation.
const MAX_LEG_SAMPLES: usize = 4_096;

/// What one cell lists. Ascending by id, deduplicated; see the module doc for which id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Candidates<'a> {
    pub bodies: &'a [u32],
    pub reaches: &'a [u32],
    pub notches: &'a [u32],
}

#[derive(Debug, Clone)]
pub struct WaterIndex {
    grid: BucketIndex,
    radius_m: f64,
    bodies: Vec<Vec<u32>>,
    reaches: Vec<Vec<u32>>,
    notches: Vec<Vec<u32>>,
}

impl WaterIndex {
    pub fn build(record: &HydroRecord, radius_m: f64, cell_m: f64) -> WaterIndex {
        let grid = BucketIndex::new(radius_m, cell_m);
        let cells = grid.cell_count();
        let mut bodies = vec![Vec::new(); cells];
        let mut reaches = vec![Vec::new(); cells];
        let mut notches = vec![Vec::new(); cells];

        for body in &record.bodies {
            if body.outline.is_empty() {
                continue;
            }
            // Ruling Q-13: the bounding circle, not a band around each shore member. See the
            // module doc for why the query can be answered outside every band.
            let anchor = SpherePoint::from_latlon(body.anchor.0, body.anchor.1);
            let mut span_m = 0.0;
            for &(lat, lon) in &body.outline {
                let d = anchor.distance_to(&SpherePoint::from_latlon(lat, lon), radius_m);
                if d > span_m {
                    span_m = d;
                }
            }
            let band_m = if body.shore_reach_m > 0.0 { body.shore_reach_m } else { 0.0 };
            add_point(&grid, &mut bodies, body.id, &anchor, span_m + band_m);
        }

        for reach in &record.reaches {
            let line: Vec<SpherePoint> = reach.points.iter()
                .map(|p| SpherePoint::from_latlon(p.lat_deg, p.lon_deg)).collect();
            let widths: Vec<f64> = reach.points.iter().map(|p| p.width_m).collect();
            add_line(&grid, &mut reaches, reach.id, &line, &widths, radius_m);
        }

        for (i, notch) in record.notches.iter().enumerate() {
            let line: Vec<SpherePoint> = notch.points.iter()
                .map(|&(lat, lon, _, _)| SpherePoint::from_latlon(lat, lon)).collect();
            let widths: Vec<f64> = notch.points.iter().map(|&(_, _, _, w)| w).collect();
            let id = i as u32; // cast-ok: a position in `record.notches`, not a float
            add_line(&grid, &mut notches, id, &line, &widths, radius_m);
        }

        for family in [&mut bodies, &mut reaches, &mut notches] {
            for cell in family.iter_mut() {
                cell.sort_unstable();
                cell.dedup();
            }
        }
        WaterIndex { grid, radius_m, bodies, reaches, notches }
    }

    /// Every item whose influence may reach `point`, ascending by id, deduplicated.
    pub fn candidates(&self, point: &SpherePoint) -> Candidates<'_> {
        let cell = self.grid.cell_of(point);
        Candidates {
            bodies: &self.bodies[cell],
            reaches: &self.reaches[cell],
            notches: &self.notches[cell],
        }
    }

    /// The cell this index **realised**, which is the one it was asked for only when that was at
    /// or above `buckets::finest_cell_m` -- `BucketIndex::new` clamps silently.
    pub fn cell_m(&self) -> f64 {
        self.grid.cell_m()
    }

    /// The planet radius this index was built over, exactly as handed to [`WaterIndex::build`].
    ///
    /// Held for the query rather than for the index itself: `query::water_at` compares metres --
    /// a body's `shore_reach_m`, half a reach's `width_m` -- against distances on the sphere, and
    /// it must measure them on the same planet the record was baked on. Its own signature (spec
    /// §8.3) carries no radius, so it reads one from here rather than from a caller who could
    /// supply a different one.
    pub fn radius_m(&self) -> f64 {
        self.radius_m
    }

    /// For the survey: cells occupied, the largest cell's item count, and the totals.
    ///
    /// "Occupied" is a cell listing at least one item of any of the three families; "the largest
    /// cell's item count" is the largest of `bodies + reaches + notches` over all cells -- what a
    /// worst-case sample has to test. The three totals are **entries stored**, summed over cells,
    /// which is the index's size and not the record's: one body listed in 40 cells is 40.
    pub fn stats(&self) -> (usize, usize, usize, usize, usize) {
        let mut occupied = 0usize;
        let mut largest = 0usize;
        for cell in 0..self.bodies.len() {
            let here = self.bodies[cell].len() + self.reaches[cell].len()
                + self.notches[cell].len();
            if here > 0 {
                occupied += 1;
            }
            if here > largest {
                largest = here;
            }
        }
        (occupied, largest,
         self.bodies.iter().map(Vec::len).sum(),
         self.reaches.iter().map(Vec::len).sum(),
         self.notches.iter().map(Vec::len).sum())
    }
}

/// Push `id` unless the cell already ends with it. Every item is added in one contiguous run, so
/// this keeps the dilation's repeats out of the vector; `build` still sorts and dedups at the
/// end, because the runs of two different items can interleave across cells.
fn push_once(cell: &mut Vec<u32>, id: u32) {
    if cell.last() != Some(&id) {
        cell.push(id);
    }
}

/// List `id` in every cell within `reach_m` of `point` -- or just the point's own cell when the
/// reach is zero, which is what a body whose recorded points all sit on its anchor gets, and a
/// zero-width, zero-length leg.
fn add_point(grid: &BucketIndex, cells: &mut [Vec<u32>], id: u32, point: &SpherePoint,
             reach_m: f64) {
    if reach_m > 0.0 {
        for cell in grid.cells_within(point, reach_m) {
            push_once(&mut cells[cell], id);
        }
    } else {
        push_once(&mut cells[grid.cell_of(point)], id);
    }
}

/// List `id` along a polyline, half a width either side. See the module doc for the guarantee.
fn add_line(grid: &BucketIndex, cells: &mut [Vec<u32>], id: u32, line: &[SpherePoint],
            widths: &[f64], radius_m: f64) {
    if line.is_empty() {
        return;
    }
    if line.len() == 1 {
        add_point(grid, cells, id, &line[0], half_of(widths[0]));
        return;
    }
    for leg in 0..line.len() - 1 {
        // Ruling Q-7: the wider of the two endpoints, because a leg tapers between them and the
        // narrower value would leave the wide end of the taper out of the index.
        let (a, b) = (widths[leg], widths[leg + 1]);
        let wider = if b > a { b } else { a };
        add_leg(grid, cells, id, &line[leg], &line[leg + 1], half_of(wider), radius_m);
    }
}

/// Half a width, and zero for a width that is negative or not a number.
fn half_of(width_m: f64) -> f64 {
    if width_m > 0.0 { width_m * 0.5 } else { 0.0 }
}

fn add_leg(grid: &BucketIndex, cells: &mut [Vec<u32>], id: u32, a: &SpherePoint, b: &SpherePoint,
           half_width_m: f64, radius_m: f64) {
    let length_m = a.distance_to(b, radius_m);
    let step_m = grid.cell_m() * 0.5;
    let raw = length_m / step_m;
    // `ceil`, which detmath does not have: `-floor(-x)`. A NaN length falls to one step, and the
    // measured gap below then carries whatever that means rather than this deciding it.
    let mut steps = if raw > 1.0 {
        -m::floor(-raw) as usize // cast-ok: a sample count from a positive finite ratio
    } else {
        1
    };
    if steps > MAX_LEG_SAMPLES {
        steps = MAX_LEG_SAMPLES;
    }
    let samples: Vec<SpherePoint> = (0..=steps)
        .map(|k| along(a, b, k as f64 / steps as f64))
        .collect();
    let mut widest_gap_m = 0.0;
    for pair in samples.windows(2) {
        let gap = pair[0].distance_to(&pair[1], radius_m);
        if gap > widest_gap_m {
            widest_gap_m = gap;
        }
    }
    let reach_m = half_width_m + widest_gap_m * 0.5;
    for sample in &samples {
        add_point(grid, cells, id, sample, reach_m);
    }
}

/// The point a fraction `t` along the great circle from `a` to `b`: the normalised blend of the
/// two directions, which lies exactly on the arc though not at an even angle along it. Two
/// antipodal ends have no arc between them and collapse to `a`; the caller measures the gaps it
/// actually got, so that case dilates by the whole separation instead of losing the middle.
fn along(a: &SpherePoint, b: &SpherePoint, t: f64) -> SpherePoint {
    if t <= 0.0 {
        return *a;
    }
    if t >= 1.0 {
        return *b;
    }
    let blend = a.vector.scaled(1.0 - t).add(&b.vector.scaled(t));
    SpherePoint::from_vector(&blend).unwrap_or(*a)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hydrology::buckets::BucketIndex;
    use crate::hydrology::{
        BakeStats, Body, BodyKind, Downstream, HydroParams, HydroRecord, NotchLine, ReachClass,
        ReachLine, ReachPoint,
    };
    use crate::sphere::SpherePoint;

    const R: f64 = 6_371_000.0;
    const CELL: f64 = DEFAULT_CELL_M;
    /// Metres per degree of latitude on `R`: one cell is 0.4497 deg of it.
    const M_PER_DEG: f64 = core::f64::consts::PI * R / 180.0;

    /// The middle of body 3, the wide lake: 333 km from its nearest recorded point, and so
    /// listed by nothing at all under a rule that only dilates each shore member by the band.
    const WIDE_ANCHOR: (f64, f64) = (-50.0, -30.0);

    /// The lake's band. Wider than a cell on purpose -- spec 8.2's point is that the dilation
    /// "can add a ring of cells all the way round a body", not that it is a rounding allowance.
    const BAND_M: f64 = 60_000.0;

    fn at(lat: f64, lon: f64) -> SpherePoint {
        SpherePoint::from_latlon(lat, lon)
    }

    fn reach_point(lat: f64, lon: f64, width_m: f64) -> ReachPoint {
        ReachPoint { lat_deg: lat, lon_deg: lon, bed_m: 10.0, width_m, depth_m: 2.0, flow_m2: 1.0 }
    }

    fn body(id: u32, kind: BodyKind, shore_member_count: u32, shore_reach_m: f64,
            outline: Vec<(f64, f64)>) -> Body {
        Body {
            id,
            kind,
            fresh: true,
            enclosed: false,
            forced: false,
            level_m: 100.0,
            area_m2: 1.0e6,
            depth_m: 5.0,
            outlet_reach: None,
            anchor: outline[0],
            outline,
            downstream: Downstream::Ocean,
            shore_member_count,
            shore_reach_m,
        }
    }

    /// A `BakeStats` with every count zero: enough for a `HydroRecord` the index is driven
    /// against directly, with no bake behind it. The index reads nothing from `stats`.
    fn stats() -> BakeStats {
        let p = HydroParams::earth_like(1_000);
        BakeStats {
            nodes: 0, land_nodes: 0, hollows: 0, kept: 0, notched: 0, closed: 0,
            streams: 0, rivers: 0, great: 0, max_order: 0,
            bifurcation_min: 0.0, bifurcation_max: 0.0,
            stream_flow_m2: p.stream_flow_m2, river_flow_m2: p.river_flow_m2,
            great_flow_m2: p.great_flow_m2,
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

    /// One lake with a shore-point extent, one pond with a ring, one banded-at-zero lake, one
    /// reach of three points, one long reach, one seam-crossing reach, and one notch of two.
    /// Everything is far enough apart that no probe can be answered by the wrong item.
    fn fixture() -> HydroRecord {
        // Body 0: two shore members on the equator at lon 0, one collar south of them.
        let lake = body(0, BodyKind::Lake, 2, BAND_M,
                        vec![(0.0, 0.0), (0.0, 0.05), (-0.1, 0.0)]);
        // Body 1: a pond's traced ring. `shore_member_count == 0`, so nothing dilates.
        let pond = body(1, BodyKind::Pond, 0, 0.0,
                        vec![(10.0, 10.0), (10.02, 10.0), (10.02, 10.02), (10.0, 10.02)]);
        // Body 2: a shore-point body with a band of exactly zero -- 26 of 1,042 real bodies.
        let bandless = body(2, BodyKind::SaltFlat, 1, 0.0, vec![(20.0, 20.0), (20.1, 20.0)]);
        // Body 3: a lake far wider than its band. Four members on a 3-degree cross -- 333 km,
        // more than six cells out from the anchor -- with a collar half a degree beyond each.
        // The owner's great lake is this shape at ten times the size: 3,627 km across against a
        // 58 km band, and nothing within 1,750 km of its middle is a recorded point.
        let mut wide = body(3, BodyKind::Lake, 4, BAND_M,
                            vec![(-53.0, -30.0), (-47.0, -30.0), (-50.0, -34.67), (-50.0, -25.33),
                                 (-53.5, -30.0), (-46.5, -30.0), (-50.0, -35.45), (-50.0, -24.55)]);
        wide.anchor = WIDE_ANCHOR;

        // Reach 0: three points, each leg well under a cell.
        let short = ReachLine {
            id: 0, class: ReachClass::Stream, order: 1, downstream: Downstream::Ocean, fresh: true,
            points: vec![reach_point(0.0, 100.0, 400.0),
                         reach_point(0.0, 100.3, 400.0),
                         reach_point(0.0, 100.6, 400.0)],
        };
        // Reach 1: one leg 5 degrees long -- about 556 km, eleven cells.
        let long = ReachLine {
            id: 1, class: ReachClass::River, order: 2, downstream: Downstream::Ocean, fresh: true,
            points: vec![reach_point(0.0, -100.0, 500.0), reach_point(0.0, -95.0, 500.0)],
        };
        // Reach 2: across the +/-180 seam at latitude 40.
        let seam = ReachLine {
            id: 2, class: ReachClass::River, order: 2, downstream: Downstream::Ocean, fresh: true,
            points: vec![reach_point(40.0, 178.0, 500.0), reach_point(40.0, -178.0, 500.0)],
        };
        let notch = NotchLine {
            points: vec![(-30.0, 50.0, 20.0, 300.0), (-30.0, 50.3, 10.0, 300.0)],
        };
        HydroRecord {
            bodies: vec![lake, pond, bandless, wide],
            reaches: vec![short, long, seam],
            notches: vec![notch],
            falls: Vec::new(),
            stats: stats(),
        }
    }

    fn built() -> WaterIndex {
        WaterIndex::build(&fixture(), R, CELL)
    }

    #[test]
    fn a_point_on_a_shore_member_lists_its_body() {
        let index = built();
        assert!(index.candidates(&at(0.0, 0.0)).bodies.contains(&0), "the member's own cell");
        assert!(index.candidates(&at(0.0, 0.05)).bodies.contains(&0), "the other member's cell");
    }

    /// Spec 8.2's clause: a cell must list every body whose shore points come within
    /// `shore_reach_m` OF THE CELL, not only the bodies with a point in it -- which under Ruling
    /// Q-13 is the `+ shore_reach_m` term of the bounding circle. The probe is 0.9 of the band
    /// north of the nearest member and beyond body 0's whole span, so a circle that forgot to add
    /// the band would miss it; and the assertion first proves the body has no outline point in
    /// that cell, so nothing but the reach can put it there.
    fn dilation_probe() -> SpherePoint {
        at(0.9 * BAND_M / M_PER_DEG, 0.0)
    }

    #[test]
    fn a_cell_the_body_has_no_point_in_still_lists_it_when_the_band_reaches() {
        let record = fixture();
        let grid = BucketIndex::new(R, CELL);
        let probe = dilation_probe();
        let cell = grid.cell_of(&probe);
        for (i, &(lat, lon)) in record.bodies[0].outline.iter().enumerate() {
            assert_ne!(grid.cell_of(&at(lat, lon)), cell,
                       "fixture is wrong: outline point {i} shares the probe's cell");
        }
        let index = WaterIndex::build(&record, R, CELL);
        assert!(index.candidates(&probe).bodies.contains(&0),
                "a point {} m from a shore member, inside the {BAND_M} m band, missed the body",
                0.9 * BAND_M);
    }

    /// Ruling Q-13, and the hole it closes. Spec §8.3's first clause admits a point whenever its
    /// nearest **member** is at least as near as its nearest **collar** point, which is true
    /// throughout a body's interior however far that is from any recorded point. A rule that only
    /// dilated each member by `shore_reach_m` therefore listed a body nowhere in the middle of
    /// anything wider than about twice its band, and the query answered `Ocean` there -- on the
    /// owner's great lake, 3,627 km across with a 58 km band, over a region 1,700 km wide.
    ///
    /// The index's job is to offer a superset; the query is what decides. So the body is listed
    /// in every cell within its **bounding circle**: the greatest distance from its anchor to any
    /// of its own recorded points, plus the band.
    #[test]
    fn a_body_is_listed_in_the_middle_of_itself_however_far_that_is_from_a_recorded_point() {
        let record = fixture();
        let grid = BucketIndex::new(R, CELL);
        let wide = &record.bodies[3];
        let middle = at(WIDE_ANCHOR.0, WIDE_ANCHOR.1);

        // The premise: the middle is many cells from every one of the body's recorded points, so
        // nothing but the bounding circle can put the body there.
        let mut nearest_m = f64::INFINITY;
        for &(lat, lon) in &wide.outline {
            let d = middle.distance_to(&at(lat, lon), R);
            if d < nearest_m {
                nearest_m = d;
            }
            assert_ne!(grid.cell_of(&at(lat, lon)), grid.cell_of(&middle),
                       "fixture is wrong: {lat},{lon} shares the middle's cell");
        }
        assert!(nearest_m > 6.0 * CELL,
                "fixture is wrong: the middle is only {nearest_m} m from a recorded point");
        assert!(nearest_m > 4.0 * wide.shore_reach_m,
                "fixture is wrong: the {} m band alone would reach {nearest_m} m",
                wide.shore_reach_m);

        let index = WaterIndex::build(&record, R, CELL);
        assert!(index.candidates(&middle).bodies.contains(&3),
                "the middle of the body, {nearest_m} m from its nearest recorded point, lists it \
                 nowhere");
        // And so does everywhere else inside it.
        for (lat, lon) in [(-51.0, -31.0), (-49.5, -29.0), (-52.0, -30.0), (-50.0, -33.0)] {
            assert!(index.candidates(&at(lat, lon)).bodies.contains(&3), "{lat},{lon}");
        }
        // The circle is a bound, not a licence: ten degrees out is not offered.
        assert!(!index.candidates(&at(-40.0, -30.0)).bodies.contains(&3),
                "the bounding circle is 449 km, and this is 1,112 km out");
    }

    #[test]
    fn two_cells_beyond_the_band_does_not_list_the_body() {
        let index = built();
        let probe = at((BAND_M + 2.0 * CELL) / M_PER_DEG, 0.0);
        assert!(!index.candidates(&probe).bodies.contains(&0),
                "the band is {BAND_M} m and the probe is two cells past it");
    }

    /// 26 of 1,042 real bodies carry a band of exactly zero. They are answered by the
    /// nearest-point clause alone, so they must still be in the cells their own points sit in.
    #[test]
    fn a_body_with_no_band_is_still_listed_in_the_cells_its_own_points_fall_in() {
        let index = built();
        for (lat, lon) in [(20.0, 20.0), (20.1, 20.0)] {
            assert!(index.candidates(&at(lat, lon)).bodies.contains(&2), "{lat},{lon}");
        }
        // And a pond's traced ring dilates by nothing either.
        for (lat, lon) in [(10.0, 10.0), (10.02, 10.02)] {
            assert!(index.candidates(&at(lat, lon)).bodies.contains(&1), "{lat},{lon}");
        }
    }

    #[test]
    fn a_point_on_a_reach_segment_lists_it_and_a_cell_away_does_not() {
        let index = built();
        assert!(index.candidates(&at(0.0, 100.15)).reaches.contains(&0), "on the first leg");
        assert!(index.candidates(&at(0.0, 100.45)).reaches.contains(&0), "on the second leg");
        assert!(!index.candidates(&at(0.6, 100.15)).reaches.contains(&0),
                "0.6 deg north of the line is more than a cell away");
    }

    #[test]
    fn a_point_on_a_notch_segment_lists_it() {
        let index = built();
        assert!(index.candidates(&at(-30.0, 50.15)).notches.contains(&0), "on the notch");
        assert!(!index.candidates(&at(-29.4, 50.15)).notches.contains(&0), "a cell north of it");
    }

    /// The case that decides how a segment's cells are enumerated. This index samples each
    /// segment and dilates every sample by half the widest sample gap, so the guarantee is
    /// "every cell holding a point within half the width of the polyline is indexed" no matter
    /// how many cells one recorded leg spans. A walk that enumerated only the endpoints' cells,
    /// or that sampled at a fraction of the leg rather than of the cell, drops the middle of a
    /// leg eleven cells long -- and the seam-crossing leg is where hand-rolled column
    /// arithmetic goes wrong.
    #[test]
    fn a_leg_many_cells_long_is_listed_all_along_its_length_including_across_the_seam() {
        let index = built();
        for step in 0..=40 {
            let t = f64::from(step) / 40.0;
            let lon = -100.0 + 5.0 * t;
            assert!(index.candidates(&at(0.0, lon)).reaches.contains(&1),
                    "the 5-degree leg is missing at lon {lon}");
            let seam_lon = 178.0 + 4.0 * t;
            let wrapped = if seam_lon > 180.0 { seam_lon - 360.0 } else { seam_lon };
            assert!(index.candidates(&at(40.0, wrapped)).reaches.contains(&2),
                    "the seam-crossing leg is missing at lon {wrapped}");
        }
    }

    #[test]
    fn candidates_are_ascending_and_deduplicated_and_a_rebuild_repeats_them() {
        let a = built();
        let b = built();
        let probes = [(0.0, 0.0), (0.2, 0.0), (10.01, 10.01), (20.05, 20.0),
                      (0.0, 100.3), (0.0, -97.5), (40.0, 180.0), (-30.0, 50.15),
                      (89.9, 12.0), (-89.9, -3.0), (0.0, -180.0)];
        for (lat, lon) in probes {
            let p = at(lat, lon);
            let got = a.candidates(&p);
            for (name, list) in [("bodies", got.bodies), ("reaches", got.reaches),
                                 ("notches", got.notches)] {
                let mut sorted = list.to_vec();
                sorted.sort_unstable();
                sorted.dedup();
                assert_eq!(list, &sorted[..], "{name} at {lat},{lon} are ascending and unique");
            }
            let again = b.candidates(&p);
            assert_eq!((got.bodies, got.reaches, got.notches),
                       (again.bodies, again.reaches, again.notches),
                       "two builds of the same record disagree at {lat},{lon}");
        }
    }

    #[test]
    fn the_cell_and_the_stats_describe_what_was_built() {
        let index = built();
        let asked = CELL;
        let got = index.cell_m();
        let off = if got > asked { got - asked } else { asked - got };
        assert!(off < asked * 1.0e-3, "asked {asked} m, realised {got} m");
        let (occupied, largest, bodies, reaches, notches) = index.stats();
        assert!(occupied > 0 && largest > 0, "{occupied} cells, largest holds {largest}");
        assert!(bodies > 0 && reaches > 0 && notches > 0,
                "{bodies} body, {reaches} reach and {notches} notch entries");
        // The four bodies' bounding circles cover far more cells than the fifteen outline points
        // a bare per-point index would have stored one entry each for.
        assert!(bodies > 15, "the bounding circles store more than one entry per point: {bodies}");
    }

    #[test]
    fn an_empty_record_answers_nothing_rather_than_panicking() {
        let mut record = fixture();
        record.bodies.clear();
        record.reaches.clear();
        record.notches.clear();
        let index = WaterIndex::build(&record, R, CELL);
        let got = index.candidates(&at(0.0, 0.0));
        assert!(got.bodies.is_empty() && got.reaches.is_empty() && got.notches.is_empty());
        assert_eq!(index.stats().0, 0, "no cell is occupied");
    }
}
