//! The fine pond search (spec §6.6): strips along the refined rivers, and the hollows in them.
//!
//! Refinement traced the channel; this finds the standing water beside it. A refined reach is
//! walked segment by segment, and each segment becomes a **strip**: a lane of `pond_cell_m`
//! cells running the segment's length and reaching `pond_search_radius_m` either side, sampled
//! off the same landform the tracer read. Inside one strip a priority flood from every edge cell
//! gives each cell the level water would stand at, exactly as `flood.rs` does on the graph, and
//! a run of cells whose spill stands above their own ground is a **hollow**. The ones deep
//! enough and wide enough to be worth a body are this module's `Candidate`s.
//!
//! This module only *finds* them. Ruling S-6's wetness and slope gates, Ruling S-7's drops,
//! Ruling S-8's density cap and the dedup between strips that both saw the same water are Task
//! 5's, which turns the survivors into bodies in the record. A hollow that touches its strip's
//! edge is kept here for exactly that reason: the strip is a window on the world, not a
//! boundary in it, and the neighbouring strip's view of the same water is the dedup's problem.

use crate::detmath as m;
use crate::hydrology::heap::FloodQueue;
use crate::hydrology::refine::Ground;
use crate::hydrology::{HydroParams, ReachLine};
use crate::sphere::SpherePoint;
use crate::tangent::TangentFrame;

/// No strip is ever larger than this, whatever the params and the reach ask for. A pathological
/// segment -- a coarse pair that refinement left thousands of kilometres apart, or a cell size
/// far below what was intended -- would otherwise allocate a `Vec<f64>` sized by its own length,
/// and a bake would die on the allocation rather than reporting anything. 4,000,000 cells is
/// 32 MB of `f64`, and at the Earth-like params (250 m cells, 25 across) it is a segment 40,000
/// steps -- 10,000 km -- long, which no refined segment is. A segment over the bound is skipped,
/// and the skip is counted in [`StripSkips`] rather than dropped.
pub const MAX_STRIP_CELLS: usize = 4_000_000;

/// A lane along one refined segment: `steps` rows from the upstream end, `cells_across` columns,
/// row-major, with lateral 0 at the middle column.
///
/// `frame` is the strip's own frame, not the geographic one: its local x runs along the segment's
/// chord and its local y to the chord's left, so `frame.local_to_sphere(along_m, lateral_m)` is
/// the point a cell was sampled at. `along_m` is the chord's length.
#[derive(Debug, Clone)]
pub struct Strip {
    pub frame: TangentFrame,
    pub along_m: f64,
    pub cells_across: usize,
    pub steps: usize,
    pub ground_m: Vec<f64>,
}

impl Strip {
    /// The middle column: the one at lateral 0, on the segment's own chord.
    pub fn middle_column(&self) -> usize {
        (self.cells_across - 1) / 2
    }

    /// Where a cell was sampled.
    pub fn point_at(&self, row: usize, column: usize, cell_m: f64) -> SpherePoint {
        let lateral = (column as f64 - self.middle_column() as f64) * cell_m;
        self.frame.local_to_sphere(row as f64 * cell_m, lateral)
    }
}

/// Segments that produced no strip. Returned rather than swallowed, so Task 5 can say how much
/// of a reach the fine search actually looked at.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StripSkips {
    /// Segments skipped because their strip would have exceeded [`MAX_STRIP_CELLS`].
    pub over_budget: u32,
    /// Segments with no length to sample: the two ends are the same point.
    pub degenerate: u32,
}

/// One hollow the fine search found, before any of Task 5's gates.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub anchor: SpherePoint,
    pub level_m: f64,
    pub floor_m: f64,
    pub area_m2: f64,
    /// Every cell in the hollow, as `(row, column)` in its strip, in row-major order.
    pub cells: Vec<(usize, usize)>,
    pub strip: usize,
}

/// One strip per segment of `reach`, sampled at `pond_cell_m` and reaching
/// `pond_search_radius_m` either side. See [`strips_with_skips`] for what a missing strip means.
pub fn strips(reach: &ReachLine, ground: &Ground, params: &HydroParams) -> Vec<Strip> {
    strips_with_skips(reach, ground, params).0
}

/// [`strips`], and the count of segments it could not sample.
pub fn strips_with_skips(reach: &ReachLine, ground: &Ground, params: &HydroParams)
                         -> (Vec<Strip>, StripSkips) {
    let cell = params.pond_cell_m;
    let half = -m::floor(-(params.pond_search_radius_m / cell));
    let k = half as usize; // cast-ok: a whole number, and bake_stages holds radius >= cell >= 10 m
    let cells_across = 2 * k + 1;
    let mut out = Vec::new();
    let mut skips = StripSkips::default();
    for pair in reach.points.windows(2) {
        let start = SpherePoint::from_latlon(pair[0].lat_deg, pair[0].lon_deg);
        let end = SpherePoint::from_latlon(pair[1].lat_deg, pair[1].lon_deg);
        let base = TangentFrame::at(&start, ground.radius_m);
        let (bx, by) = base.sphere_to_local(&end);
        let len_m = m::hypot(bx, by);
        if !(len_m > 0.0) {
            skips.degenerate += 1;
            continue;
        }
        let wanted = -m::floor(-(len_m / cell));
        let steps = if wanted > 1.0 { wanted as usize } else { 1 }; // cast-ok: a whole number, bounded below by the MAX_STRIP_CELLS check
        let too_big = match steps.checked_mul(cells_across) {
            Some(cells) => cells > MAX_STRIP_CELLS,
            None => true,
        };
        if too_big {
            skips.over_budget += 1;
            continue;
        }
        // The strip's frame: x along the chord, y to its left. `east` and `north` are orthonormal
        // and `(bx, by) / len_m` is a unit vector, so the combination is already unit; the
        // fallback is there only because `normalised` is total.
        let along = base.east.scaled(bx / len_m).add(&base.north.scaled(by / len_m));
        let along = along.normalised().unwrap_or(base.east);
        let frame = TangentFrame {
            origin: start,
            east: along,
            north: base.up.cross(&along),
            up: base.up,
            radius_m: ground.radius_m,
        };
        let mut ground_m = Vec::with_capacity(steps * cells_across);
        for row in 0..steps {
            for column in 0..cells_across {
                let lateral = (column as f64 - k as f64) * cell;
                let point = frame.local_to_sphere(row as f64 * cell, lateral);
                ground_m.push((ground.height_m)(&point));
            }
        }
        out.push(Strip { frame, along_m: len_m, cells_across, steps, ground_m });
    }
    (out, skips)
}

/// The spill level every cell of the strip would stand at, by a priority flood from every edge
/// cell -- `flood.rs`'s algorithm on the strip's own 4-neighbour grid.
fn spill_levels(strip: &Strip) -> Vec<f64> {
    let w = strip.cells_across;
    let h = strip.steps;
    let n = w * h;
    let mut spill = strip.ground_m.clone();
    let mut reached = vec![false; n];
    let mut queue = FloodQueue::new();
    for row in 0..h {
        for column in 0..w {
            if row > 0 && row + 1 < h && column > 0 && column + 1 < w {
                continue;
            }
            let i = row * w + column;
            reached[i] = true;
            let own = strip.ground_m[i];
            spill[i] = own;
            queue.push_tied(own, own, i as u32); // cast-ok: a cell index, bounded by MAX_STRIP_CELLS
        }
    }
    while let Some((level, node)) = queue.pop() {
        let i = node as usize; // cast-ok: the index this flood pushed
        let (row, column) = (i / w, i % w);
        let step = |r: usize, c: usize, spill: &mut Vec<f64>, reached: &mut Vec<bool>,
                        queue: &mut FloodQueue| {
            let j = r * w + c;
            if reached[j] {
                return;
            }
            reached[j] = true;
            let own = strip.ground_m[j];
            let level = if own > level { own } else { level };
            spill[j] = level;
            // The same tie-break `flood.rs` argues for: inside a filled hollow every cell shares
            // one spill, and breaking by the cell's own ground reaches the floor first.
            queue.push_tied(level, own, j as u32); // cast-ok: a cell index, bounded by MAX_STRIP_CELLS
        };
        if row > 0 {
            step(row - 1, column, &mut spill, &mut reached, &mut queue);
        }
        if row + 1 < h {
            step(row + 1, column, &mut spill, &mut reached, &mut queue);
        }
        if column > 0 {
            step(row, column - 1, &mut spill, &mut reached, &mut queue);
        }
        if column + 1 < w {
            step(row, column + 1, &mut spill, &mut reached, &mut queue);
        }
    }
    spill
}

/// Every hollow in one strip that passes the pond keep rule, deepest first, ties by the anchor's
/// row then column.
pub fn hollows_in(strip: &Strip, strip_index: usize, params: &HydroParams) -> Vec<Candidate> {
    let w = strip.cells_across;
    let h = strip.steps;
    if w == 0 || h == 0 {
        return Vec::new();
    }
    let cell = params.pond_cell_m;
    let cell_area = cell * cell;
    let spill = spill_levels(strip);
    let under_water = |i: usize| spill[i] > strip.ground_m[i];
    let mut seen = vec![false; w * h];
    // (depth, anchor row, anchor column, candidate) -- the first three only to sort by.
    let mut kept: Vec<(f64, usize, usize, Candidate)> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    for start in 0..w * h {
        if seen[start] || !under_water(start) {
            continue;
        }
        let level_m = spill[start];
        seen[start] = true;
        stack.push(start);
        let mut cells: Vec<(usize, usize)> = Vec::new();
        while let Some(i) = stack.pop() {
            let (row, column) = (i / w, i % w);
            cells.push((row, column));
            let step = |r: usize, c: usize, seen: &mut Vec<bool>, stack: &mut Vec<usize>| {
                let j = r * w + c;
                if seen[j] || !under_water(j) || spill[j] != level_m {
                    return;
                }
                seen[j] = true;
                stack.push(j);
            };
            if row > 0 {
                step(row - 1, column, &mut seen, &mut stack);
            }
            if row + 1 < h {
                step(row + 1, column, &mut seen, &mut stack);
            }
            if column > 0 {
                step(row, column - 1, &mut seen, &mut stack);
            }
            if column + 1 < w {
                step(row, column + 1, &mut seen, &mut stack);
            }
        }
        // Row-major, whatever order the flood fill walked them in.
        cells.sort();
        let mut floor_m = strip.ground_m[cells[0].0 * w + cells[0].1];
        let (mut anchor_row, mut anchor_column) = cells[0];
        for &(row, column) in &cells[1..] {
            let own = strip.ground_m[row * w + column];
            if own < floor_m {
                floor_m = own;
                anchor_row = row;
                anchor_column = column;
            }
        }
        let depth_m = level_m - floor_m;
        let area_m2 = cells.len() as f64 * cell_area;
        if !(depth_m >= params.pond_keep_depth_m && area_m2 >= params.pond_keep_area_m2) {
            continue;
        }
        let anchor = strip.point_at(anchor_row, anchor_column, cell);
        kept.push((depth_m, anchor_row, anchor_column,
                   Candidate { anchor, level_m, floor_m, area_m2, cells, strip: strip_index }));
    }
    kept.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    kept.into_iter().map(|(_, _, _, c)| c).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hydrology::refine::Ground;
    use crate::hydrology::{Downstream, HydroParams, ReachClass, ReachLine, ReachPoint};
    use crate::sphere::SpherePoint;

    const R: f64 = 6_371_000.0;
    const M_PER_DEG: f64 = R * std::f64::consts::PI / 180.0;

    fn params() -> HydroParams {
        HydroParams::earth_like(1_000)
    }

    fn reach_along_the_equator(km: f64) -> ReachLine {
        let end = (km * 1_000.0) / M_PER_DEG;
        ReachLine {
            id: 0, class: ReachClass::Stream, order: 1, downstream: Downstream::Ocean, fresh: true,
            points: vec![
                ReachPoint { lat_deg: 0.0, lon_deg: 0.0, bed_m: 100.0, width_m: 5.0, depth_m: 1.0, flow_m2: 1.0e9 },
                ReachPoint { lat_deg: 0.0, lon_deg: end, bed_m: 90.0, width_m: 5.0, depth_m: 1.0, flow_m2: 1.0e9 },
            ],
        }
    }

    #[test]
    fn a_strip_covers_the_search_radius_at_the_cell_size() {
        let h = |_: &SpherePoint| 100.0;
        let ground = Ground { height_m: &h, radius_m: R, corridor_m: 20_000.0, seed: 1 };
        let p = params();
        let strip = &strips(&reach_along_the_equator(10.0), &ground, &p)[0];
        // 3 km either side at 250 m: 12 cells each way plus the middle.
        assert_eq!(strip.cells_across, 25);
        assert_eq!(strip.steps, 40, "10 km at 250 m");
        assert_eq!(strip.ground_m.len(), 25 * 40);
    }

    #[test]
    fn a_dip_beside_the_river_is_a_candidate() {
        // A bowl 4 m deep and 1 km across, centred 1 km north of the line at 5 km along.
        let h = |p: &SpherePoint| {
            let (lat, lon) = p.to_latlon();
            let (n, e) = (lat * M_PER_DEG, lon * M_PER_DEG);
            let dn = n - 1_000.0;
            let de = e - 5_000.0;
            let r2 = dn * dn + de * de;
            if r2 < 500.0 * 500.0 { 100.0 - 4.0 * (1.0 - r2 / (500.0 * 500.0)) } else { 100.0 }
        };
        let ground = Ground { height_m: &h, radius_m: R, corridor_m: 20_000.0, seed: 1 };
        let p = params();
        let found: Vec<Candidate> = strips(&reach_along_the_equator(10.0), &ground, &p)
            .iter().enumerate().flat_map(|(i, s)| hollows_in(s, i, &p)).collect();
        assert_eq!(found.len(), 1, "one bowl, one candidate");
        let c = &found[0];
        assert!(c.level_m - c.floor_m >= p.pond_keep_depth_m, "depth {}", c.level_m - c.floor_m);
        assert!(c.area_m2 >= p.pond_keep_area_m2, "area {}", c.area_m2);
        let (lat, lon) = c.anchor.to_latlon();
        let (n, e) = (lat * M_PER_DEG, lon * M_PER_DEG);
        assert!(n > 500.0 && n < 1_500.0 && e > 4_500.0 && e < 5_500.0, "anchor at {n}, {e}");
    }

    #[test]
    fn a_dip_under_two_metres_is_not_a_candidate() {
        let h = |p: &SpherePoint| {
            let (lat, lon) = p.to_latlon();
            let (n, e) = (lat * M_PER_DEG, lon * M_PER_DEG);
            let dn = n - 1_000.0;
            let de = e - 5_000.0;
            let r2 = dn * dn + de * de;
            if r2 < 500.0 * 500.0 { 100.0 - 1.5 * (1.0 - r2 / (500.0 * 500.0)) } else { 100.0 }
        };
        let ground = Ground { height_m: &h, radius_m: R, corridor_m: 20_000.0, seed: 1 };
        let p = params();
        let found: usize = strips(&reach_along_the_equator(10.0), &ground, &p)
            .iter().enumerate().map(|(i, s)| hollows_in(s, i, &p).len()).sum();
        assert_eq!(found, 0);
    }

    /// Two samplings of the same strip are the same bits, and the same hollows in the same
    /// order. Nothing here reads a hash map, a clock or an allocation address, and this is what
    /// says so.
    #[test]
    fn the_same_strip_samples_and_floods_identically_twice() {
        // Two bowls of different depths on the same strip, so the order is not a one-element
        // accident: 5 m at 3 km along, 3 m at 7 km along.
        let h = |p: &SpherePoint| {
            let (lat, lon) = p.to_latlon();
            let (n, e) = (lat * M_PER_DEG, lon * M_PER_DEG);
            let bowl = |cn: f64, ce: f64, deep: f64| {
                let (dn, de) = (n - cn, e - ce);
                let r2 = dn * dn + de * de;
                if r2 < 600.0 * 600.0 { deep * (1.0 - r2 / (600.0 * 600.0)) } else { 0.0 }
            };
            let a = bowl(1_000.0, 3_000.0, 5.0);
            let b = bowl(-1_000.0, 7_000.0, 3.0);
            100.0 - if a > b { a } else { b }
        };
        let ground = Ground { height_m: &h, radius_m: R, corridor_m: 20_000.0, seed: 1 };
        let p = params();
        let reach = reach_along_the_equator(10.0);
        let first = strips(&reach, &ground, &p);
        let second = strips(&reach, &ground, &p);
        assert_eq!(first.len(), second.len());
        for (a, b) in first.iter().zip(&second) {
            assert_eq!(a.steps, b.steps);
            assert_eq!(a.cells_across, b.cells_across);
            let a_bits: Vec<u64> = a.ground_m.iter().map(|g| g.to_bits()).collect();
            let b_bits: Vec<u64> = b.ground_m.iter().map(|g| g.to_bits()).collect();
            assert_eq!(a_bits, b_bits, "the same strip sampled twice is the same bits");
        }
        let a: Vec<Candidate> = first.iter().enumerate()
            .flat_map(|(i, s)| hollows_in(s, i, &p)).collect();
        let b: Vec<Candidate> = second.iter().enumerate()
            .flat_map(|(i, s)| hollows_in(s, i, &p)).collect();
        assert_eq!(a.len(), 2, "two bowls, two candidates");
        assert!(a[0].level_m - a[0].floor_m > a[1].level_m - a[1].floor_m, "deepest first");
        assert_eq!(a, b, "the same candidates in the same order");
    }

    /// A segment whose strip would not fit is skipped, and the skip is counted rather than
    /// dropped: at 10 m cells, 3 km either side is 601 columns, so 10,000 km of reach asks for
    /// 601,000,000 cells.
    #[test]
    fn a_segment_over_the_cell_bound_is_skipped_and_counted() {
        let h = |_: &SpherePoint| 100.0;
        let ground = Ground { height_m: &h, radius_m: R, corridor_m: 20_000.0, seed: 1 };
        let mut p = params();
        p.pond_cell_m = 10.0;
        let (made, skips) = strips_with_skips(&reach_along_the_equator(10_000.0), &ground, &p);
        assert!(made.is_empty(), "no strip is made for a segment over the bound");
        assert_eq!(skips, StripSkips { over_budget: 1, degenerate: 0 });
        // And the same reach at the Earth-like cell is inside the bound, so the bound is a
        // bound on the work and not a refusal to search at all.
        let (made, skips) = strips_with_skips(&reach_along_the_equator(10_000.0), &ground, &params());
        assert_eq!(made.len(), 1);
        assert_eq!(skips, StripSkips::default());
    }

    /// A segment with no length is not sampled, and that too is counted.
    #[test]
    fn a_segment_of_no_length_is_counted_not_silently_dropped() {
        let h = |_: &SpherePoint| 100.0;
        let ground = Ground { height_m: &h, radius_m: R, corridor_m: 20_000.0, seed: 1 };
        let mut reach = reach_along_the_equator(10.0);
        reach.points.push(reach.points[1].clone());
        let (made, skips) = strips_with_skips(&reach, &ground, &params());
        assert_eq!(made.len(), 1);
        assert_eq!(skips, StripSkips { over_budget: 0, degenerate: 1 });
    }

    #[test]
    fn a_slope_with_no_dip_has_no_candidate() {
        let h = |p: &SpherePoint| { let (_, lon) = p.to_latlon(); 100.0 - 0.002 * lon * M_PER_DEG };
        let ground = Ground { height_m: &h, radius_m: R, corridor_m: 20_000.0, seed: 1 };
        let p = params();
        let found: usize = strips(&reach_along_the_equator(10.0), &ground, &p)
            .iter().enumerate().map(|(i, s)| hollows_in(s, i, &p).len()).sum();
        assert_eq!(found, 0);
    }
}
