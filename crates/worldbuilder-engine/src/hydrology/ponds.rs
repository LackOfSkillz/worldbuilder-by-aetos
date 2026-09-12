//! The fine pond search (spec §6.6): strips along the refined rivers, and the hollows in them.
//!
//! Refinement traced the channel; this finds the standing water beside it. A refined reach is
//! walked segment by segment, and each segment becomes a **strip**: a lane of `pond_cell_m`
//! cells running the segment's length and reaching `pond_search_radius_m` either side. It is
//! sampled off `Surface::elevation_m` -- the landform **with** its detail field -- and that is
//! the one place in `hydrology` that reads texture; Ruling S-9 and the argument for it are at
//! [`pond_ground`]. Inside one strip a priority flood from every edge cell
//! gives each cell the level water would stand at, exactly as `flood.rs` does on the graph, and
//! a run of cells whose spill stands above their own ground is a **hollow**. The ones deep
//! enough and wide enough to be worth a body are this module's `Candidate`s.
//!
//! This module only *finds* them. Ruling S-6's wetness and slope gates, Ruling S-7's drops,
//! Ruling S-8's density cap and the dedup between strips that both saw the same water are Task
//! 5's, which turns the survivors into bodies in the record. A hollow the strip's window clipped
//! is returned here rather than dropped, flagged with [`Candidate::touches_side`] and
//! [`Candidate::touches_end`]: Ruling S-10 says Task 5 must not record a side-clipped one, and
//! flagging rather than filtering keeps the count visible.

use crate::detmath as m;
use crate::hydrology::buckets::BucketIndex;
use crate::hydrology::heap::FloodQueue;
use crate::hydrology::landgraph::LandGraph;
use crate::hydrology::refine::Ground;
use crate::hydrology::routing::NO_LAKE;
use crate::hydrology::{Body, BodyKind, Downstream, HydroParams, HydroRecord, ReachLine};
use crate::sphere::SpherePoint;
use crate::surface::Surface;
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
    /// **Ruling S-12:** segments whose corridor the coarse gate refused at the midpoint, before a
    /// single cell was sampled. See [`strips_where`].
    pub gated: u32,
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
    /// **Ruling S-10: Task 5 must not record this candidate.** The hollow ran up against one of
    /// the corridor's long sides, `pond_search_radius_m` from the refined line, and what stopped
    /// it was the window, not the terrain.
    ///
    /// For a side-clipped hollow it is not only the area and the level that are window numbers --
    /// its *existence* is. The flood seeds every edge cell at its own ground, so a hollow bounded
    /// by the window is a hollow whose water leaves through the window; whether it stands at all
    /// depends on ground the search never looked at. If the real terrain keeps descending past
    /// 3 km, that water drains away and there is no body there.
    ///
    /// The cost of the ruling, stated: a genuine pond more than 6 km across beside a river is
    /// lost. Nothing false is recorded, which is the trade. On the measured bakes this is about
    /// half of all candidates -- see the task 4 report -- so it is not a rare case, and the
    /// number is visible rather than silently absent because `hollows_in` flags and does not
    /// filter.
    pub touches_side: bool,
    /// The hollow ran up against the *upstream or downstream end* of the strip. This one is
    /// kept: an end is where the next segment's strip carries on, so the same water is seen by
    /// two strips of the same reach, and Task 5's cross-strip dedup is exactly what resolves it.
    /// Measured at 3.9-6.3% of candidates.
    pub touches_end: bool,
}

/// The clip test behind [`Candidate::touches_side`] and [`Candidate::touches_end`]: the first
/// *interior* ring, not the outer one.
///
/// An edge cell is a flood seed, so its spill equals its own ground and `spill > ground` is false
/// for it by construction -- a hollow can never contain one, and asking whether it does always
/// answers no. A hollow that reaches the ring one cell inside the edge is a hollow whose
/// neighbour is a seed: it stopped because the window stopped, which is the thing worth knowing.
fn clipped_at(cells: &[(usize, usize)], steps: usize, cells_across: usize) -> (bool, bool) {
    let side = cells.iter().any(|&(_, column)| column <= 1 || column + 2 >= cells_across);
    let end = cells.iter().any(|&(row, _)| row <= 1 || row + 2 >= steps);
    (side, end)
}

/// Ruling S-9: the ground the fine pond search reads, and the *only* thing in `hydrology` that
/// reads it -- `Surface::elevation_m` at the search's own cell size, which is the landform **plus
/// the detail field**, where everything else in the bake reads `structural_m` and never sees
/// texture at all.
///
/// The ruling, because a reader will reach for spec §5.1 and think this is forbidden:
///
/// * §5.1 bars texture from deciding **where water goes**. A pond decides nothing about where
///   water goes. It changes no routing, no receiver, no reach and no notch; by Ruling S-5 it is
///   recorded with `downstream = Reach(r)` and stage 2 draws no outflow from it.
/// * §6.6 itself calls small lakes and ponds texture, and says a found hollow that fails the keep
///   rule is "simply not recorded; at this scale they are texture and need no notch".
/// * The drainage network is still derived from the landform alone, so 1a's pit-lake failure
///   cannot come back through the rivers.
/// * The speckle risk is held off by four gates that all remain: the keep rule (>= 2 m deep,
///   >= 0.05 km^2), the corridor along the refined rivers (the spec's 3 km; `earth_like` ships
///   1.5 km after Ruling S-17), Ruling S-6's wetness and slope
///   gates, and Ruling S-8's density cap (the spec's 500 km^2; `earth_like` ships 16,000 km^2
///   after plan 1b-3's Task 7 size gate and Ruling S-16).
///
/// The cost, stated plainly: **ponds move when the detail field moves.** Any slider that changes
/// detail -- its amplitude, its seed, the roughness a feature authorises -- changes where the
/// ponds are, and re-baking is what moves them. The rivers stay put.
pub fn pond_ground<'a>(surface: &'a Surface, params: &HydroParams)
                       -> impl Fn(&SpherePoint) -> f64 + 'a {
    let cell_m = params.pond_cell_m;
    move |point: &SpherePoint| surface.elevation_m(point, Some(cell_m))
}

/// One strip per segment of `reach`, sampled at `pond_cell_m` and reaching
/// `pond_search_radius_m` either side. See [`strips_with_skips`] for what a missing strip means.
///
/// **`ground` and `pond_ground_m` are deliberately different sources** (Ruling S-9, argued at
/// [`pond_ground`]). `ground` supplies the geometry only -- the planet's radius, so a strip's
/// frame matches the one the tracer laid the refined line down in -- and its own `height_m` is
/// the landform, which this search does not read. Every cell's height comes from
/// `pond_ground_m`, which on a real bake is [`pond_ground`]: the landform *with* detail. Passing
/// `ground.height_m` here would compile and would find nothing (measured: 0 candidates in 8,277
/// strips on two populations, the deepest hollow anywhere 0.018 m against a 2 m rule).
pub fn strips(reach: &ReachLine, ground: &Ground, pond_ground_m: &dyn Fn(&SpherePoint) -> f64,
              params: &HydroParams) -> Vec<Strip> {
    strips_with_skips(reach, ground, pond_ground_m, params).0
}

/// [`strips`], and the count of segments it could not sample.
pub fn strips_with_skips(reach: &ReachLine, ground: &Ground,
                         pond_ground_m: &dyn Fn(&SpherePoint) -> f64, params: &HydroParams)
                         -> (Vec<Strip>, StripSkips) {
    strips_where(reach, ground, pond_ground_m, params, &|_| true)
}

/// **Ruling S-12:** [`strips_with_skips`], with a coarse gate on *where to look at all*. A segment
/// whose midpoint `look_here` refuses is never sampled, and is counted in [`StripSkips::gated`].
///
/// The gate's granularity is the point. Ruling S-6 calls the wetness and slope tests "coarse gates
/// on where to look", and until this ruling they were applied to a candidate *after* its whole
/// whole corridor had been sampled off `Surface::elevation_m` at 250 m -- which is where a bake's
/// time goes, so gating afterwards saved none of it. Asking the same question of the segment's
/// midpoint, before the sampling loop, is the gate doing what it was written to do.
///
/// **The cost, stated plainly: the gate now takes or skips a corridor whole.** A segment whose
/// midpoint sits just below the wetness floor loses every pond along its whole length, including
/// ones 3 km away in wetter ground that the per-candidate test would have kept; a segment whose
/// midpoint is just above it keeps looking through ground the per-candidate test would have
/// refused. So a few ponds near a wetness boundary appear or vanish against the per-candidate
/// answer, and `ponds_found` now counts hollows in the corridors that passed rather than hollows
/// in every corridor. Nothing false is recorded: `search` still applies Ruling S-6's and S-7's
/// tests to each surviving candidate's own anchor, and this gate only removes work.
pub fn strips_where(reach: &ReachLine, ground: &Ground,
                    pond_ground_m: &dyn Fn(&SpherePoint) -> f64, params: &HydroParams,
                    look_here: &dyn Fn(&SpherePoint) -> bool) -> (Vec<Strip>, StripSkips) {
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
        // `wanted` is a whole number and at least 1: `len_m > 0` and this is a ceiling. An absurd
        // one saturates at `usize::MAX` rather than wrapping, and the `checked_mul` below then
        // refuses the segment, so the cast needs no guard of its own.
        let wanted = -m::floor(-(len_m / cell));
        let steps = wanted as usize; // cast-ok: whole, >= 1, and saturating rather than wrapping
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
        // Ruling S-12's gate, the last thing before the only expensive part of this function.
        if !look_here(&frame.local_to_sphere(len_m * 0.5, 0.0)) {
            skips.gated += 1;
            continue;
        }
        let mut ground_m = Vec::with_capacity(steps * cells_across);
        for row in 0..steps {
            for column in 0..cells_across {
                let lateral = (column as f64 - k as f64) * cell;
                let point = frame.local_to_sphere(row as f64 * cell, lateral);
                ground_m.push(pond_ground_m(&point));
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
        // Ruling S-10 flags here and drops in Task 5, so a side-clipped candidate stays in the
        // count and can be reported rather than quietly never existing.
        let (touches_side, touches_end) = clipped_at(&cells, h, w);
        kept.push((depth_m, anchor_row, anchor_column,
                   Candidate { anchor, level_m, floor_m, area_m2, cells, strip: strip_index,
                               touches_side, touches_end }));
    }
    kept.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    kept.into_iter().map(|(_, _, _, c)| c).collect()
}

/// Ruling S-6's wetness floor: the `pond_wetness_share` quantile of the graph's **land** nodes'
/// wetness, at index `floor(share * (len - 1))` of the sorted values. `None` on a graph with no
/// land node at all, which is a graph with no reaches and so no candidates either.
fn wetness_floor(graph: &LandGraph, params: &HydroParams) -> Option<f64> {
    let mut wet: Vec<f64> = (0..graph.len())
        .filter(|&i| !graph.ocean[i])
        .map(|i| graph.wetness[i])
        .collect();
    if wet.is_empty() {
        return None;
    }
    wet.sort_unstable_by(|a, b| a.total_cmp(b));
    // `bake_stages` holds `0 < pond_wetness_share <= 1`, so the product is in `0..=len-1`.
    let at = m::floor(params.pond_wetness_share * (wet.len() - 1) as f64);
    Some(wet[at as usize]) // cast-ok: floor of a value held to 0..=len-1 above
}

/// The candidate's lowest cell, ties to the lower row then column -- the same rule `hollows_in`
/// used to place the anchor, recomputed here because a `Candidate` carries the anchor's *point*
/// and not its cell.
fn anchor_cell(strip: &Strip, cells: &[(usize, usize)]) -> (usize, usize) {
    let w = strip.cells_across;
    let mut best = cells[0];
    let mut lowest = strip.ground_m[best.0 * w + best.1];
    for &(row, column) in &cells[1..] {
        let own = strip.ground_m[row * w + column];
        if own < lowest {
            lowest = own;
            best = (row, column);
        }
    }
    best
}

/// Ruling S-6's slope gate, read off the strip the candidate was found in: does the ground rise
/// by more than `pond_max_slope` over one cell in any of the four directions at the anchor?
///
/// The anchor is the hollow's lowest cell, so every neighbour rises; the question is by how much.
/// A cell against the strip's edge simply has fewer neighbours to ask.
fn too_steep(strip: &Strip, anchor: (usize, usize), params: &HydroParams) -> bool {
    let w = strip.cells_across;
    let (row, column) = anchor;
    let own = strip.ground_m[row * w + column];
    let rise_m = params.pond_max_slope * params.pond_cell_m;
    let steep = |r: usize, c: usize| strip.ground_m[r * w + c] - own > rise_m;
    (row > 0 && steep(row - 1, column))
        || (row + 1 < strip.steps && steep(row + 1, column))
        || (column > 0 && steep(row, column - 1))
        || (column + 1 < w && steep(row, column + 1))
}

/// Douglas–Peucker over a closed ring of `(lat, lon)` points, keeping both ends (which are the
/// same point) and anything further than `tolerance_m` from the chord it would be dropped onto.
/// `refine::simplify`'s method, on a bare ring rather than on `ReachPoint`s: there is no bed here
/// to hold a vertical tolerance against, and no protected point but the closure itself.
fn simplify_ring(points: &[(f64, f64)], radius_m: f64, tolerance_m: f64) -> Vec<(f64, f64)> {
    let n = points.len();
    if n <= 3 {
        return points.to_vec();
    }
    let at = |p: &(f64, f64)| SpherePoint::from_latlon(p.0, p.1);
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;
    let mut spans: Vec<(usize, usize)> = vec![(0, n - 1)];
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
            // A ring's first and last point are the same, so the very first chord has no length
            // and every point is measured from that single point instead. That is the right
            // question to ask: the furthest point from the start is the one the ring cannot lose.
            let raw = if len2 > 0.0 { (px * bx + py * by) / len2 } else { 0.0 };
            let t = if raw < 0.0 { 0.0 } else if raw > 1.0 { 1.0 } else { raw };
            let off = m::hypot(px - t * bx, py - t * by);
            if off > worst {
                worst = off;
                worst_at = i;
            }
        }
        if worst > tolerance_m {
            keep[worst_at] = true;
            spans.push((lo, worst_at));
            spans.push((worst_at, hi));
        }
    }
    points.iter().zip(&keep).filter(|(_, &k)| k).map(|(p, _)| *p).collect()
}

/// The candidate's outline: a marching walk round the boundary of its cells at `pond_cell_m`,
/// through the strip's frame, simplified to a `pond_cell_m` tolerance.
///
/// **The ring closes implicitly.** The first point is not repeated at the end, so a ring of three
/// points is a triangle, and a consumer joins `outline[i]` to `outline[(i + 1) % len]`.
///
/// **The winding is fixed**, by the four-edge convention below rather than by any test of the
/// ring afterwards: for each cell of the set, in row-major order, a side whose neighbour is
/// outside the set contributes one directed edge -- the low-row side runs toward higher columns,
/// the high-column side toward higher rows, the high-row side toward lower columns, and the
/// low-column side toward lower rows. Chained from the lowest boundary corner (lowest row, then
/// lowest column), that is a clockwise walk in the strip's own `(along, lateral)` frame, which is
/// right-handed about the outward normal -- so clockwise seen from outside the sphere.
///
/// A hollow is one 4-connected component, so the walk from the lowest corner is its outer
/// boundary. A hole inside it (dry ground the water surrounds) is a second loop the walk does not
/// visit, and is not recorded: the outline is the body's extent, and its interior is not part of
/// the record's question.
///
/// **A ring is checked before it is returned, and an unusable candidate gets no ring at all.** It
/// must be simple ([`ring_is_simple`]) and must still contain every one of the candidate's own cell
/// centres ([`ring_contains`]). The simplified ring is tried first, then the untouched corner walk;
/// if neither holds, **this returns an empty `Vec` and `search` does not record the candidate**. A
/// body whose recorded shape does not contain its own recorded water is worse than no body: §8.3
/// answers `water_at` from the ring, and `area_m2` comes from the cell count, so the two would
/// disagree in silence. Ruling S-13 sets the tolerance that makes the fallback rare rather than
/// routine, but the check is what makes the property hold -- a tolerance that happens to work on
/// four worlds is not a guarantee.
///
/// The raw walk fails only where the cell set pinches: two cells meeting at a corner and nowhere
/// else make the boundary visit that corner twice, and a ring with a repeated vertex is not simple.
/// Measured at 1 candidate in 6,243.
pub fn outline(strip: &Strip, cells: &[(usize, usize)], params: &HydroParams) -> Vec<(f64, f64)> {
    let (mut first_row, mut last_row) = (cells[0].0, cells[0].0);
    let (mut first_column, mut last_column) = (cells[0].1, cells[0].1);
    for &(row, column) in cells {
        if row < first_row { first_row = row; }
        if row > last_row { last_row = row; }
        if column < first_column { first_column = column; }
        if column > last_column { last_column = column; }
    }
    let width = last_column - first_column + 1;
    let height = last_row - first_row + 1;
    let mut inside = vec![false; width * height];
    for &(row, column) in cells {
        inside[(row - first_row) * width + (column - first_column)] = true;
    }

    // Corner (r, c) of the local grid is the low-row, low-column corner of cell (r, c), so there
    // is one more corner than cell in each direction.
    let corner = |r: usize, c: usize| r * (width + 1) + c;
    let mut out_of: Vec<Vec<u32>> = vec![Vec::new(); (width + 1) * (height + 1)];
    for row in 0..height {
        for column in 0..width {
            if !inside[row * width + column] {
                continue;
            }
            let mut edge = |from: usize, to: usize| out_of[from].push(to as u32); // cast-ok: a corner index of this candidate's own bounding box
            if row == 0 || !inside[(row - 1) * width + column] {
                edge(corner(row, column), corner(row, column + 1));
            }
            if column + 1 == width || !inside[row * width + column + 1] {
                edge(corner(row, column + 1), corner(row + 1, column + 1));
            }
            if row + 1 == height || !inside[(row + 1) * width + column] {
                edge(corner(row + 1, column + 1), corner(row + 1, column));
            }
            if column == 0 || !inside[row * width + column - 1] {
                edge(corner(row + 1, column), corner(row, column));
            }
        }
    }

    // Corner ids ascend by row and then by column, so the first with an outgoing edge is the
    // lowest-row, lowest-column boundary corner the brief asks the walk to start from.
    let start = match out_of.iter().position(|edges| !edges.is_empty()) {
        Some(i) => i,
        None => return Vec::new(),
    };
    let edge_count: usize = out_of.iter().map(|edges| edges.len()).sum();
    let mut taken = vec![0usize; out_of.len()];
    let mut ring: Vec<usize> = Vec::new();
    let mut here = start;
    // A corner two cells meet at diagonally has two outgoing edges; taking them in the order they
    // were emitted keeps the walk total, and the bound keeps it finite whatever the set's shape.
    for _ in 0..edge_count {
        ring.push(here);
        let next = match out_of[here].get(taken[here]) {
            Some(&next) => next as usize, // cast-ok: the corner index this walk itself pushed
            None => break,
        };
        taken[here] += 1;
        if next == start {
            break;
        }
        here = next;
    }

    let cell = params.pond_cell_m;
    let middle = strip.middle_column() as f64;
    let traced: Vec<(f64, f64)> = ring
        .iter()
        .map(|&id| {
            let row = id / (width + 1);
            let column = id % (width + 1);
            // A cell is sampled at its centre, so its low corner is half a cell back in each
            // direction.
            let along_m = ((first_row + row) as f64 - 0.5) * cell;
            let lateral_m = ((first_column + column) as f64 - 0.5 - middle) * cell;
            strip.frame.local_to_sphere(along_m, lateral_m).to_latlon()
        })
        .collect();
    if traced.len() < 3 {
        return traced;
    }
    let mut closed = traced.clone();
    closed.push(traced[0]);
    let mut simplified = simplify_ring(&closed, strip.frame.radius_m, cell * SIMPLIFY_CELLS);
    simplified.pop(); // the repeated first point: the ring closes implicitly
    // The two properties simplification can break, checked rather than assumed.
    let radius_m = strip.frame.radius_m;
    let usable = |ring: &[(f64, f64)]| {
        ring.len() >= 3
            && ring_is_simple(ring, radius_m)
            && cells.iter().all(|&(row, column)| {
                ring_contains(ring, radius_m, &strip.point_at(row, column, cell))
            })
    };
    if usable(&simplified) {
        simplified
    } else if usable(&traced) {
        traced
    } else {
        Vec::new()
    }
}

/// A helper index's cell: about one entry per cell for a population of `count`, with `count`
/// held inside the range `nominal_spacing_m` is defined on.
///
/// **A `BucketIndex` allocates one bucket per cell of the whole planet**, so its cell must be
/// sized to what it holds and not to the distance a caller happens to be asking about. A 500 m
/// grid on Earth is 21 million buckets -- half a gigabyte of empty `Vec`s -- and `candidates` and
/// `nearest` are both correct at any cell size, so nothing is lost by choosing a coarse one.
fn index_cell_m(count: usize, radius_m: f64) -> f64 {
    let count = if count < 1 { 1 } else if count > 100_000_000 { 100_000_000 } else { count };
    crate::stream::nominal_spacing_m(count as u32, radius_m) // cast-ok: held to 1..=100,000,000 above
}

/// **Ruling S-13:** the Douglas–Peucker tolerance [`outline`] simplifies a traced ring at, as a
/// multiple of `pond_cell_m`.
///
/// A whole cell was the first choice and it was wrong, measured rather than argued: on 6,243 rings
/// across four populations no ring self-crossed, but **5,717 of them left at least one of their own
/// cells outside their own outline** (20,877 cells of 514,821). The reason is exact. A single
/// staircase corner has its middle vertex 250/sqrt(2) = 176.8 m off the chord across it, which a
/// 250 m tolerance drops; the chord that replaces it then passes through the corner cell's own
/// centre, and whether that centre reads as inside is a coin flip. At half a cell, 176.8 m is over
/// the tolerance and the corner is kept.
///
/// This matters because `area_m2` comes from the candidate's cell count and not from the ring, so a
/// body that leaks its own water out of its own outline is a body whose recorded area and recorded
/// shape disagree and nothing in the record says so. §8.3 answers `water_at` from the ring.
const SIMPLIFY_CELLS: f64 = 0.5;

/// A ring's points in a tangent plane at `at`, for the two tests below. A pond is a few kilometres
/// across at most, so one plane is exact enough to answer both questions about it.
fn ring_local(ring: &[(f64, f64)], radius_m: f64, at: &SpherePoint) -> Vec<(f64, f64)> {
    let frame = TangentFrame::at(at, radius_m);
    ring.iter()
        .map(|&(lat, lon)| frame.sphere_to_local(&SpherePoint::from_latlon(lat, lon)))
        .collect()
}

/// Does `ring` cross itself **transversally** anywhere? A closed ring with the implicit closure of
/// [`outline`], so segment `i` runs from `ring[i]` to `ring[(i + 1) % len]`.
///
/// **"Transversally" is the whole of the promise.** Two non-adjacent segments fail this only when
/// each has an endpoint strictly on either side of the other -- the sign tests below are all
/// strict, and a zero is read as a touch. A repeated vertex is caught separately, as an exact
/// equality. Everything else degenerate passes: a vertex lying *on* another segment's interior,
/// two collinear segments overlapping along a stretch, a segment grazing a vertex. That is the
/// same convention `refine::segments_cross` uses for reaches, and it is enough for the property
/// this is here to protect -- a simplified ring that folds through a facing wall -- but it is not
/// "the ring is a simple closed curve" in the topological sense, and a caller must not read it so.
///
/// **This exists because simplification can break it.** [`outline`]'s walk is simple by
/// construction -- it is the boundary of a set of cells -- but the Douglas–Peucker that follows
/// cuts corners by up to a whole cell, and on a concave rectilinear ring a cut corner can be made
/// to cross a facing wall. Ruling S-1's own history is the argument for measuring rather than
/// assuming: the ring it rejected self-crossed six times and nothing but a measurement found it.
/// §8.3's point-in-polygon test is silently wrong on a ring that crosses itself.
pub fn ring_is_simple(ring: &[(f64, f64)], radius_m: f64) -> bool {
    let n = ring.len();
    if n < 3 {
        return false;
    }
    let local = ring_local(ring, radius_m, &SpherePoint::from_latlon(ring[0].0, ring[0].1));
    // A repeated vertex is a degenerate crossing the segment test below cannot see.
    for i in 0..n {
        for j in i + 1..n {
            if local[i] == local[j] {
                return false;
            }
        }
    }
    let side = |a: (f64, f64), b: (f64, f64), c: (f64, f64)| {
        (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
    };
    for i in 0..n {
        let (a, b) = (local[i], local[(i + 1) % n]);
        for j in i + 1..n {
            // Segments that share an endpoint meet there by construction and are not crossings.
            if j == (i + 1) % n || i == (j + 1) % n {
                continue;
            }
            let (c, d) = (local[j], local[(j + 1) % n]);
            let (d1, d2) = (side(a, b, c), side(a, b, d));
            let (d3, d4) = (side(c, d, a), side(c, d, b));
            let opposite = |p: f64, q: f64| (p > 0.0 && q < 0.0) || (p < 0.0 && q > 0.0);
            if opposite(d1, d2) && opposite(d3, d4) {
                return false;
            }
        }
    }
    true
}

/// Is `point` inside `ring`? Even-odd crossings of a ray from the point, in a tangent plane at the
/// point itself, with the ring closed implicitly.
///
/// This is the test §8.3 will make of a pond, brought forward so the trace can be held to it:
/// `area_m2` comes from the candidate's cell count and not from the ring, so a simplification that
/// cut a cell out of its own outline would leave a body whose recorded area and recorded shape
/// disagree and nothing would notice.
pub fn ring_contains(ring: &[(f64, f64)], radius_m: f64, point: &SpherePoint) -> bool {
    if ring.len() < 3 {
        return false;
    }
    let local = ring_local(ring, radius_m, point);
    let n = local.len();
    let mut inside = false;
    for i in 0..n {
        let (xi, yi) = local[i];
        let (xj, yj) = local[(i + 1) % n];
        // Half-open in y, so a vertex exactly level with the ray counts once and not twice.
        if (yi > 0.0) != (yj > 0.0) {
            let t = -yi / (yj - yi);
            if xi + t * (xj - xi) > 0.0 {
                inside = !inside;
            }
        }
    }
    inside
}

/// One candidate that survived every gate, with everything the record needs, so the density cap
/// can sort and drop without holding on to its (very large) strip.
struct Survivor {
    anchor: SpherePoint,
    lat_deg: f64,
    lon_deg: f64,
    level_m: f64,
    depth_m: f64,
    area_m2: f64,
    outline: Vec<(f64, f64)>,
}

/// Spec §6.6's fine search, end to end: strips along every refined reach, the hollows in them,
/// Rulings S-6, S-7, S-8, S-10 and S-11, and the bodies that survive, appended to `record.bodies`.
///
/// **Appended, never inserted** (Ruling S-7): a body's id is its index on the wire, and stage 2
/// keys on it, so every coarse body keeps the id it had. A pond's id continues from the last
/// coarse one.
///
/// **`pond_ground_m` is not `ground.height_m`** (Ruling S-9, argued at [`pond_ground`]). `ground`
/// gives the geometry -- the radius the strips' frames are laid out on -- and `pond_ground_m`
/// gives every cell's height. On a bake the first is the landform and the second is the landform
/// with its detail field; passing the landform for both compiles and finds nothing.
///
/// The order of the work is the order of the rulings, and it is what makes the result the same
/// run to run: reaches in id order, each reach's strips in order, each strip's hollows deepest
/// first. Every gate below is a drop, so it cannot reorder what is left.
pub fn search(record: &mut HydroRecord, graph: &LandGraph, lake_of: &[u32], ground: &Ground,
              pond_ground_m: &dyn Fn(&SpherePoint) -> f64, params: &HydroParams) {
    let floor = match wetness_floor(graph, params) {
        Some(floor) => floor,
        None => return,
    };
    // `lake_of` is `Routing::lake_of`, one entry per node. A caller that pairs it with a different
    // graph gets nothing rather than an out-of-bounds panic: `wb_hydro_bake` reaches this code
    // through `bake`, and nothing reachable from `extern "C"` may panic.
    if record.reaches.is_empty() || graph.positions.is_empty() || lake_of.len() != graph.len() {
        return;
    }

    // The nearest refined reach line, for Ruling S-5's `downstream`. Every point of every reach,
    // so "nearest line" is nearest point on it rather than nearest of its ends.
    let mut line_points: Vec<SpherePoint> = Vec::new();
    let mut line_reach: Vec<u32> = Vec::new();
    for reach in &record.reaches {
        for point in &reach.points {
            line_points.push(SpherePoint::from_latlon(point.lat_deg, point.lon_deg));
            line_reach.push(reach.id);
        }
    }
    if line_points.is_empty() {
        return;
    }
    let mut lines = BucketIndex::new(ground.radius_m, index_cell_m(line_points.len(), ground.radius_m));
    for (i, at) in line_points.iter().enumerate() {
        lines.insert(at, i as u32); // cast-ok: a point index, bounded by the record's own size
    }

    let mut found = 0usize;
    // `nodes` and the dedup index are wanted only while the strips are walked, and each holds a
    // bucket per cell of a whole planet; the block drops them before the density grid is built.
    let mut survivors: Vec<Survivor> = {
        // The nearest graph node, for Ruling S-6's wetness and Ruling S-7's coarse-lake test.
        let mut nodes = BucketIndex::new(graph.radius_m, index_cell_m(graph.len(), graph.radius_m));
        for (i, position) in graph.positions.iter().enumerate() {
            nodes.insert(position, i as u32); // cast-ok: a node index, bounded by stream::MAX_NODES
        }
        // The dedup partner: kept anchors, so a pond two strips both saw is one pond. Sized to
        // the reach points rather than to the 500 m dedup distance -- a 500 m grid over a planet
        // is 21 million buckets, and there are never more anchors than there are river points.
        let dedup_m = params.pond_cell_m * 2.0;
        let mut anchors = BucketIndex::new(ground.radius_m, index_cell_m(line_points.len(), ground.radius_m));
        let mut anchor_points: Vec<SpherePoint> = Vec::new();

        // Ruling S-6's wetness and Ruling S-7's coarse-lake test, at one point. Ruling S-12 asks it
        // of a segment's midpoint before the corridor is sampled at all; the loop below asks it
        // again of each surviving candidate's own anchor, which can be 3 km away and is the point
        // the two rulings are actually written about. The pre-check does not subsume it and is not
        // meant to: it removes work, and the per-candidate test is what decides a body.
        let coarse_ok = |at: &SpherePoint| match nodes.nearest(at, &graph.positions) {
            Some(node) => {
                let node = node as usize; // cast-ok: the node index the index was built with
                lake_of[node] == NO_LAKE && graph.wetness[node] >= floor
            }
            None => false,
        };

        let mut survivors: Vec<Survivor> = Vec::new();
        for reach in &record.reaches {
            let (made, _) = strips_where(reach, ground, pond_ground_m, params, &coarse_ok);
            for (index, strip) in made.iter().enumerate() {
                for candidate in hollows_in(strip, index, params) {
                    found += 1;
                    // Ruling S-10: past the corridor the terrain may keep descending, so a
                    // side-clipped hollow's level, area and existence are all window numbers.
                    if candidate.touches_side {
                        continue;
                    }
                    if too_steep(strip, anchor_cell(strip, &candidate.cells), params) {
                        continue;
                    }
                    // Ruling S-7, the datum half: the *landform* at the anchor, not the detail
                    // field the hollow was found in. Water at or below the datum is the sea's.
                    if !((ground.height_m)(&candidate.anchor) > 0.0) {
                        continue;
                    }
                    // Ruling S-7's lake half and Ruling S-6's wetness gate, at the anchor itself.
                    if !coarse_ok(&candidate.anchor) {
                        continue;
                    }
                    // Two strips of one reach share an end, so the same water is seen twice; the
                    // first to see it is the one that keeps it.
                    let near = anchors.candidates(&candidate.anchor, dedup_m);
                    if near.iter().any(|&id| {
                        candidate.anchor.distance_to(&anchor_points[id as usize], ground.radius_m) <= dedup_m
                    }) {
                        continue;
                    }
                    anchors.insert(&candidate.anchor, anchor_points.len() as u32); // cast-ok: bounded by the candidate count
                    anchor_points.push(candidate.anchor);
                    let (lat_deg, lon_deg) = candidate.anchor.to_latlon();
                    survivors.push(Survivor {
                        anchor: candidate.anchor,
                        lat_deg,
                        lon_deg,
                        level_m: candidate.level_m,
                        depth_m: candidate.level_m - candidate.floor_m,
                        area_m2: candidate.area_m2,
                        outline: outline(strip, &candidate.cells, params),
                    });
                }
            }
        }
        survivors
    };

    // Ruling S-8: deepest first, ties by latitude then longitude bits -- total, and independent
    // of the order the strips happened to find them in.
    survivors.sort_by(|a, b| {
        b.depth_m.total_cmp(&a.depth_m)
            .then(a.lat_deg.to_bits().cmp(&b.lat_deg.to_bits()))
            .then(a.lon_deg.to_bits().cmp(&b.lon_deg.to_bits()))
    });
    // Ruling S-8's grid, and the one index here whose cell is not sized to its population: the
    // cell *is* the rule. At the spec's 500 km^2 that is about 22.4 km, a million buckets on an
    // Earth-sized planet; at the 16,000 km^2 `earth_like` ships after plan 1b-3's Task 7 size
    // gate and Ruling S-16, about 126.5 km. It is built only after the two indexes above have
    // been dropped.
    let density = BucketIndex::new(ground.radius_m, m::sqrt(params.pond_density_area_m2));
    // The cells already spoken for, sorted so membership is a binary search rather than a hash
    // set: a few thousand entries at most, and nothing here may depend on a hash order.
    let mut taken: Vec<usize> = Vec::new();
    let mut kept = 0usize;
    for survivor in &survivors {
        // Both of these are defensive -- a hollow of one cell already traces four corners, and
        // `line_points` was checked non-empty above -- but a survivor that cannot be recorded
        // must not claim the cell a later one could have used, so they come first.
        if survivor.outline.len() < 3 {
            continue;
        }
        // Ruling S-5: a pond beside a river drains to that river, and no channel is traced for it.
        let downstream = match lines.nearest(&survivor.anchor, &line_points) {
            Some(point) => Downstream::Reach(line_reach[point as usize]),
            None => continue,
        };
        let cell = density.cell_of(&survivor.anchor);
        match taken.binary_search(&cell) {
            Ok(_) => continue,
            Err(at) => taken.insert(at, cell),
        }
        let id = record.bodies.len() as u32; // cast-ok: a body index, bounded by the candidate count
        record.bodies.push(Body {
            id,
            // Ruling S-11: the area picks the kind and neither kind is dropped. Both carry the
            // traced ring; spec §7 makes `kind` the discriminator between a ring and a lake's
            // shore-point set, and a fine find has no shore points either way.
            kind: if survivor.area_m2 < params.pond_max_area_m2 { BodyKind::Pond } else { BodyKind::Lake },
            fresh: true,
            enclosed: false,
            forced: false,
            level_m: survivor.level_m,
            area_m2: survivor.area_m2,
            depth_m: survivor.depth_m,
            outlet_reach: None,
            anchor: (survivor.lat_deg, survivor.lon_deg),
            outline: survivor.outline.clone(),
            downstream,
        });
        kept += 1;
    }

    record.stats.ponds_found = found as u32; // cast-ok: at most one candidate per strip cell
    record.stats.ponds_kept = kept as u32; // cast-ok: bounded by `found`
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hydrology::landgraph::LandGraph;
    use crate::hydrology::refine::Ground;
    use crate::hydrology::routing::NO_LAKE;
    use crate::hydrology::{BakeStats, Downstream, HydroParams, HydroRecord, ReachClass, ReachLine,
                           ReachPoint};
    use crate::sphere::SpherePoint;

    const R: f64 = 6_371_000.0;
    const M_PER_DEG: f64 = R * std::f64::consts::PI / 180.0;

    /// Ruling S-17 took `earth_like`'s corridor to 1.5 km for the owner world's size and time
    /// gates. **These fixtures pin 3 km on purpose**: their bowls, offsets and expected cell
    /// counts were laid out against spec §6.6's corridor, and what they assert is the search's
    /// mechanism -- that a bowl is found, that a side clip is flagged, that Rulings S-6, S-7,
    /// S-8 and S-10 do what they say -- not which corridor width ships. Inheriting a number the
    /// gates tune would break every one of them the next time it is tuned, and would say nothing
    /// about the mechanism when it did.
    fn params() -> HydroParams {
        let mut p = HydroParams::earth_like(1_000);
        p.pond_search_radius_m = 3_000.0;
        p
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
        let strip = &strips(&reach_along_the_equator(10.0), &ground, &h, &p)[0];
        // 3 km either side at 250 m (this fixture's own corridor, see `params`): 12 cells each
        // way plus the middle.
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
        let found: Vec<Candidate> = strips(&reach_along_the_equator(10.0), &ground, &h, &p)
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
        let found: usize = strips(&reach_along_the_equator(10.0), &ground, &h, &p)
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
        let first = strips(&reach, &ground, &h, &p);
        let second = strips(&reach, &ground, &h, &p);
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
        let (made, skips) = strips_with_skips(&reach_along_the_equator(10_000.0), &ground, &h, &p);
        assert!(made.is_empty(), "no strip is made for a segment over the bound");
        assert_eq!(skips, StripSkips { over_budget: 1, degenerate: 0, gated: 0 });
        // And the same reach at the Earth-like cell is inside the bound, so the bound is a
        // bound on the work and not a refusal to search at all.
        let (made, skips) = strips_with_skips(&reach_along_the_equator(10_000.0), &ground, &h, &params());
        assert_eq!(made.len(), 1);
        assert_eq!(skips, StripSkips::default());
    }

    /// A round bowl `deep` metres deep and `radius` across, centred `north` metres from the line
    /// and `east` metres along it, on ground otherwise flat at 100 m.
    fn bowl(north: f64, east: f64, radius: f64, deep: f64) -> impl Fn(&SpherePoint) -> f64 {
        move |p: &SpherePoint| {
            let (lat, lon) = p.to_latlon();
            let (dn, de) = (lat * M_PER_DEG - north, lon * M_PER_DEG - east);
            let r2 = dn * dn + de * de;
            if r2 < radius * radius { 100.0 - deep * (1.0 - r2 / (radius * radius)) } else { 100.0 }
        }
    }

    /// Ruling S-10: a hollow that runs up against a long side of the corridor is flagged
    /// `touches_side`, because what stopped it was the window and not the terrain.
    #[test]
    fn a_bowl_against_the_corridors_side_is_flagged_as_side_clipped() {
        // Centred 2.75 km north of the line -- one cell inside the +3 km edge -- and 5 km along.
        let h = bowl(2_750.0, 5_000.0, 600.0, 30.0);
        let ground = Ground { height_m: &h, radius_m: R, corridor_m: 20_000.0, seed: 1 };
        let p = params();
        let found: Vec<Candidate> = strips(&reach_along_the_equator(10.0), &ground, &h, &p)
            .iter().enumerate().flat_map(|(i, s)| hollows_in(s, i, &p)).collect();
        assert_eq!(found.len(), 1, "one bowl, one candidate");
        assert!(found[0].touches_side, "the bowl reaches the corridor's northern side");
        assert!(!found[0].touches_end, "and is nowhere near either end");
    }

    /// And one against the strip's upstream end is flagged `touches_end` only: that is the case
    /// Task 5's cross-strip dedup resolves, so it survives S-10.
    #[test]
    fn a_bowl_against_the_strips_end_is_flagged_as_end_clipped() {
        // Centred on the line, 250 m along -- the strip's second row.
        let h = bowl(0.0, 250.0, 600.0, 30.0);
        let ground = Ground { height_m: &h, radius_m: R, corridor_m: 20_000.0, seed: 1 };
        let p = params();
        let found: Vec<Candidate> = strips(&reach_along_the_equator(10.0), &ground, &h, &p)
            .iter().enumerate().flat_map(|(i, s)| hollows_in(s, i, &p)).collect();
        assert_eq!(found.len(), 1, "one bowl, one candidate");
        assert!(found[0].touches_end, "the bowl reaches the strip's upstream end");
        assert!(!found[0].touches_side, "and neither of the corridor's sides");
    }

    /// The bowl the other tests use sits well inside the window, and neither flag is set --
    /// otherwise the two above would prove nothing.
    #[test]
    fn a_bowl_in_open_ground_is_flagged_neither_way() {
        let h = bowl(1_000.0, 5_000.0, 500.0, 4.0);
        let ground = Ground { height_m: &h, radius_m: R, corridor_m: 20_000.0, seed: 1 };
        let p = params();
        let found: Vec<Candidate> = strips(&reach_along_the_equator(10.0), &ground, &h, &p)
            .iter().enumerate().flat_map(|(i, s)| hollows_in(s, i, &p)).collect();
        assert_eq!(found.len(), 1);
        assert!(!found[0].touches_side && !found[0].touches_end);
    }

    /// Ruling S-9: the pond ground and the routing ground are different sources, and must stay
    /// so. If somebody quietly unifies them -- `pond_ground` reduced to `structural_m`, or the
    /// strips fed `Ground::height_m` again -- this goes red. The measurement behind the ruling is
    /// that the landform alone finds nothing: 0 candidates in 8,277 strips across two
    /// populations, the deepest hollow anywhere 0.018 m against a 2 m keep rule.
    #[test]
    fn the_pond_ground_is_not_the_routing_ground() {
        let world = crate::surface::Surface::new(20_260_904, R, 12, 0.29, None, None, None);
        let p = params();
        let detail = pond_ground(&world, &p);
        // Along a degree of longitude at latitude 11, at the search's own cell size: the two
        // fields must disagree somewhere, and by more than a rounding.
        let mut differ = 0;
        let mut widest: f64 = 0.0;
        for i in 0..400 {
            let q = SpherePoint::from_latlon(11.0, i as f64 * 0.0025);
            let landform = world.structural_m(&q);
            let gap = detail(&q) - landform;
            let gap = if gap < 0.0 { -gap } else { gap };
            if gap > 0.0 {
                differ += 1;
            }
            if gap > widest {
                widest = gap;
            }
        }
        assert!(differ > 200, "the two grounds agreed at {} of 400 probes", 400 - differ);
        assert!(widest > 1.0, "widest disagreement only {widest} m -- is detail still there?");
        // And a strip sampled off each is not the same strip.
        let ground = Ground { height_m: &|q: &SpherePoint| world.structural_m(q),
                              radius_m: R, corridor_m: 20_000.0, seed: 1 };
        let reach = reach_along_the_equator(10.0);
        let on_detail = &strips(&reach, &ground, &detail, &p)[0];
        let on_landform = &strips(&reach, &ground, ground.height_m, &p)[0];
        assert_ne!(on_detail.ground_m, on_landform.ground_m,
                   "the fine search must not be reading the landform");
    }

    /// A segment with no length is not sampled, and that too is counted.
    #[test]
    fn a_segment_of_no_length_is_counted_not_silently_dropped() {
        let h = |_: &SpherePoint| 100.0;
        let ground = Ground { height_m: &h, radius_m: R, corridor_m: 20_000.0, seed: 1 };
        let mut reach = reach_along_the_equator(10.0);
        reach.points.push(reach.points[1].clone());
        let (made, skips) = strips_with_skips(&reach, &ground, &h, &params());
        assert_eq!(made.len(), 1);
        assert_eq!(skips, StripSkips { over_budget: 0, degenerate: 1, gated: 0 });
    }

    /// A line of equally-wet land nodes along the equator, under the strip: every candidate's
    /// nearest node has wetness 0.5, which is the 60th percentile of a uniform 0.5, so Ruling
    /// S-6's wetness gate passes and the tests below are about the gate they name.
    fn graph_under_the_line() -> LandGraph {
        const N: usize = 11;
        let positions: Vec<SpherePoint> = (0..N)
            .map(|i| SpherePoint::from_latlon(0.0, i as f64 * 1_000.0 / M_PER_DEG))
            .collect();
        let directed: Vec<Vec<u32>> = (0..N)
            .map(|i| if i + 1 < N { vec![i as u32 + 1] } else { Vec::new() }) // cast-ok: node index, 11 of them
            .collect();
        LandGraph::from_parts(R, positions, vec![100.0; N], vec![1.0e6; N], &directed,
                              vec![0.5; N])
    }

    /// A record holding one reach and nothing else, for `search` to append ponds to. The stats
    /// are a zeroed fixture: `search` writes only the nine pond words, and the tests read only
    /// those.
    fn record_for(reach: ReachLine) -> HydroRecord {
        HydroRecord {
            bodies: Vec::new(),
            reaches: vec![reach],
            notches: Vec::new(),
            falls: Vec::new(),
            stats: BakeStats {
                nodes: 0, land_nodes: 0, hollows: 0, kept: 0, notched: 0, closed: 0,
                streams: 0, rivers: 0, great: 0, max_order: 0,
                bifurcation_min: 0.0, bifurcation_max: 0.0,
                stream_flow_m2: 0.0, river_flow_m2: 0.0, great_flow_m2: 0.0,
                total_nodes: 0, wetness_nodes: 0, keep_depth_m: 0.0, keep_area_m2: 0.0,
                pond_max_area_m2: 0.0, keep_max_area_m2: 0.0, min_stream_nodes: 0.0,
                notch_fall_m: 0.0, evaporation_factor: 0.0, salt_flat_share: 0.0,
                forced_requested: 0, forced_matched: 0,
                capped_basins: 0, capped_inner: 0, capped_inner_kept: 0,
                refine_step_m: 0.0, refine_simplify_m: 0.0, refine_vertical_m: 0.0,
                fall_min_drop_m: 0.0, fall_max_run_m: 0.0,
                meander_wavelength_widths: 0.0, meander_amplitude_widths: 0.0,
                meander_max_slope: 0.0,
                crossings_coarse: 0, crossings_left: 0,
                ponds_found: 0, ponds_kept: 0,
                pond_cell_m: 0.0, pond_search_radius_m: 0.0, pond_keep_depth_m: 0.0,
                pond_keep_area_m2: 0.0, pond_wetness_share: 0.0, pond_max_slope: 0.0,
                pond_density_area_m2: 0.0,
            },
        }
    }

    /// Ruling S-8: the density cap keeps the deepest first, one per cell of about 22.4 km.
    #[test]
    fn the_density_cap_keeps_the_deepest() {
        // Two bowls 2 km apart, 5 m and 3 m deep, both on the line and clear of the window's
        // edges: the same 500 km^2 cell, so only the deeper stays.
        let deep = bowl(0.0, 3_000.0, 600.0, 5.0);
        let shallow = bowl(0.0, 5_000.0, 600.0, 3.0);
        let h = move |p: &SpherePoint| {
            let (a, b) = (deep(p), shallow(p));
            if a < b { a } else { b }
        };
        let ground = Ground { height_m: &h, radius_m: R, corridor_m: 20_000.0, seed: 1 };
        let p = params();
        let graph = graph_under_the_line();
        let lake_of = vec![NO_LAKE; graph.len()];
        // Both bowls are candidates before the cap.
        let found: Vec<Candidate> = strips(&reach_along_the_equator(10.0), &ground, &h, &p)
            .iter().enumerate().flat_map(|(i, s)| hollows_in(s, i, &p)).collect();
        assert_eq!(found.len(), 2, "two bowls, two candidates");
        assert!(found.iter().all(|c| !c.touches_side && !c.touches_end),
                "neither bowl is clipped, so Ruling S-10 is not what this test measures");
        let mut record = record_for(reach_along_the_equator(10.0));
        search(&mut record, &graph, &lake_of, &ground, &h, &p);
        assert_eq!(record.stats.ponds_found, 2);
        assert_eq!(record.stats.ponds_kept, 1, "one 500 km^2 cell holds one body");
        assert_eq!(record.bodies.len(), 1);
        let body = &record.bodies[0];
        assert!(body.depth_m > 4.5 && body.depth_m < 5.5,
                "the deeper bowl is the one kept, not the shallower: {} m", body.depth_m);
        // And it is the deep bowl's place, 3 km along the line rather than 5.
        let (_, lon) = (body.anchor.0, body.anchor.1);
        assert!(lon * M_PER_DEG > 2_400.0 && lon * M_PER_DEG < 3_600.0,
                "kept body at {} m along", lon * M_PER_DEG);
    }

    /// The line graph with one extra node `north_m` metres off the line at 5,000 m along it. A
    /// bowl centred there has a **different** nearest node from the segment's midpoint, so Ruling
    /// S-6's and S-7's *per-candidate* tests can be driven without Ruling S-12's *per-strip* gate
    /// answering first. The extra node is the last one, so a test names it by `graph.len() - 1`.
    fn graph_with_an_off_line_node(north_m: f64) -> LandGraph {
        let mut positions: Vec<SpherePoint> = (0..11)
            .map(|i| SpherePoint::from_latlon(0.0, i as f64 * 1_000.0 / M_PER_DEG))
            .collect();
        positions.push(SpherePoint::from_latlon(north_m / M_PER_DEG, 5_000.0 / M_PER_DEG));
        let n = positions.len();
        let directed: Vec<Vec<u32>> = (0..n)
            .map(|i| if i + 1 < n { vec![i as u32 + 1] } else { Vec::new() }) // cast-ok: node index, 12 of them
            .collect();
        LandGraph::from_parts(R, positions, vec![100.0; n], vec![1.0e6; n], &directed,
                              vec![0.5; n])
    }

    /// Ruling S-7: a candidate inside a coarse lake, or on ground at or below the datum, is
    /// dropped. Every run below is the same bowl on the same ground, so the difference between
    /// them is the ruling and nothing else.
    #[test]
    fn a_candidate_inside_a_coarse_lake_is_dropped() {
        let h = bowl(0.0, 5_000.0, 600.0, 5.0);
        let ground = Ground { height_m: &h, radius_m: R, corridor_m: 20_000.0, seed: 1 };
        let p = params();
        let graph = graph_under_the_line();

        let mut open = record_for(reach_along_the_equator(10.0));
        search(&mut open, &graph, &vec![NO_LAKE; graph.len()], &ground, &h, &p);
        assert_eq!(open.stats.ponds_found, 1);
        assert_eq!(open.stats.ponds_kept, 1, "on open ground the bowl is a body");

        // Every node a member of coarse lake 0. Ruling S-12 answers first here: the segment's
        // own midpoint is inside the lake, so the corridor is never sampled and the hollow is
        // never even found. That is the ruling's stated cost, and this is what it looks like.
        let mut drowned = record_for(reach_along_the_equator(10.0));
        search(&mut drowned, &graph, &vec![0u32; graph.len()], &ground, &h, &p);
        assert_eq!(drowned.stats.ponds_found, 0, "Ruling S-12 skipped the whole corridor");
        assert_eq!(drowned.stats.ponds_kept, 0);
        assert!(drowned.bodies.is_empty());

        // The per-candidate half of the same ruling, with the strip gate deliberately passing:
        // the bowl sits 1.5 km off the line beside its own graph node, which is a lake member,
        // while the midpoint's node on the line is not. The corridor IS sampled, the hollow IS
        // found, and the candidate is dropped at its own anchor.
        let off_line = bowl(1_500.0, 5_000.0, 600.0, 5.0);
        let ground = Ground { height_m: &off_line, radius_m: R, corridor_m: 20_000.0, seed: 1 };
        let graph = graph_with_an_off_line_node(1_500.0);
        let mut lake_of = vec![NO_LAKE; graph.len()];
        let last = graph.len() - 1;
        lake_of[last] = 0;
        let mut beside = record_for(reach_along_the_equator(10.0));
        search(&mut beside, &graph, &lake_of, &ground, &off_line, &p);
        assert_eq!(beside.stats.ponds_found, 1, "the corridor passed the strip gate");
        assert_eq!(beside.stats.ponds_kept, 0, "and the candidate's own node is a lake member");
        // The control: the same bowl and graph with that node out of the lake is kept.
        let mut control = record_for(reach_along_the_equator(10.0));
        search(&mut control, &graph, &vec![NO_LAKE; graph.len()], &ground, &off_line, &p);
        assert_eq!(control.stats.ponds_kept, 1);

        // And the datum half of the ruling, which no strip gate tests: the same bowl on ground
        // 100 m lower is under the sea.
        let sunk = |q: &SpherePoint| h(q) - 100.0;
        let below = Ground { height_m: &sunk, radius_m: R, corridor_m: 20_000.0, seed: 1 };
        let mut drowned = record_for(reach_along_the_equator(10.0));
        search(&mut drowned, &graph_under_the_line(), &vec![NO_LAKE; 11], &below, &sunk, &p);
        assert_eq!(drowned.stats.ponds_found, 1);
        assert_eq!(drowned.stats.ponds_kept, 0, "the landform there is at the datum");
    }

    /// Ruling S-10 at the `search` level: a side-clipped hollow is **counted** in `ponds_found`
    /// and kept out of `bodies`. `the_density_cap_keeps_the_deepest` asserts the opposite case --
    /// that an unclipped bowl is recorded -- so without this nothing holds the ruling itself.
    #[test]
    fn a_side_clipped_candidate_is_counted_and_not_recorded() {
        // The `a_bowl_against_the_corridors_side_is_flagged_as_side_clipped` fixture: 2.75 km
        // north of the line, one cell inside the +3 km edge.
        let h = bowl(2_750.0, 5_000.0, 600.0, 30.0);
        let ground = Ground { height_m: &h, radius_m: R, corridor_m: 20_000.0, seed: 1 };
        let p = params();
        let graph = graph_under_the_line();
        let mut record = record_for(reach_along_the_equator(10.0));
        search(&mut record, &graph, &vec![NO_LAKE; graph.len()], &ground, &h, &p);
        assert_eq!(record.stats.ponds_found, 1, "the hollow is found and counted");
        assert_eq!(record.stats.ponds_kept, 0, "Ruling S-10: and not recorded");
        assert!(record.bodies.is_empty());
        // The control: the same bowl moved onto the line, clear of both sides, IS recorded -- so
        // what the assertions above measure is the clip and not the bowl.
        let clear = bowl(0.0, 5_000.0, 600.0, 30.0);
        let ground = Ground { height_m: &clear, radius_m: R, corridor_m: 20_000.0, seed: 1 };
        let mut record = record_for(reach_along_the_equator(10.0));
        search(&mut record, &graph, &vec![NO_LAKE; graph.len()], &ground, &clear, &p);
        assert_eq!(record.stats.ponds_kept, 1);
    }

    /// Ruling S-6's slope gate at the `search` level. `graph_under_the_line` makes every node
    /// equally wet, so nothing else in this run can be what drops the candidate.
    #[test]
    fn a_candidate_in_steep_ground_is_counted_and_not_recorded() {
        // A bowl 80 m deep over a 600 m radius: one 250 m cell from its floor the ground stands
        // about 13.9 m higher, against the 7.5 m that `pond_max_slope` 0.03 allows over a cell.
        let steep = bowl(0.0, 5_000.0, 600.0, 80.0);
        let ground = Ground { height_m: &steep, radius_m: R, corridor_m: 20_000.0, seed: 1 };
        let p = params();
        let graph = graph_under_the_line();
        let mut record = record_for(reach_along_the_equator(10.0));
        search(&mut record, &graph, &vec![NO_LAKE; graph.len()], &ground, &steep, &p);
        assert_eq!(record.stats.ponds_found, 1, "the hollow is found and counted");
        assert_eq!(record.stats.ponds_kept, 0, "Ruling S-6: and too steep to record");
        // The control: the same bowl at 5 m deep is gentle enough and is recorded.
        let gentle = bowl(0.0, 5_000.0, 600.0, 5.0);
        let ground = Ground { height_m: &gentle, radius_m: R, corridor_m: 20_000.0, seed: 1 };
        let mut record = record_for(reach_along_the_equator(10.0));
        search(&mut record, &graph, &vec![NO_LAKE; graph.len()], &ground, &gentle, &p);
        assert_eq!(record.stats.ponds_kept, 1);
    }

    /// Ruling S-6's wetness gate, both halves: Ruling S-12's per-strip pre-check, which skips a
    /// dry corridor whole, and the per-candidate test at the anchor, driven here with the strip
    /// gate deliberately passing.
    #[test]
    fn a_candidate_in_dry_ground_is_not_recorded() {
        let off_line = bowl(1_500.0, 5_000.0, 600.0, 5.0);
        let ground = Ground { height_m: &off_line, radius_m: R, corridor_m: 20_000.0, seed: 1 };
        let p = params();

        // The bowl's own node is dry and every other node is wet. With 12 nodes and a 0.6 share
        // the floor is the 7th of the sorted values, which is 0.5, and the dry node's 0.0 is
        // below it -- while the midpoint's node on the line is not.
        let mut graph = graph_with_an_off_line_node(1_500.0);
        let last = graph.len() - 1;
        graph.wetness[last] = 0.0;
        let mut record = record_for(reach_along_the_equator(10.0));
        search(&mut record, &graph, &vec![NO_LAKE; graph.len()], &ground, &off_line, &p);
        assert_eq!(record.stats.ponds_found, 1, "the corridor passed the strip gate");
        assert_eq!(record.stats.ponds_kept, 0, "and the candidate's own node is too dry");

        // The control: the same bowl and graph with that node as wet as the rest is kept.
        let graph = graph_with_an_off_line_node(1_500.0);
        let mut record = record_for(reach_along_the_equator(10.0));
        search(&mut record, &graph, &vec![NO_LAKE; graph.len()], &ground, &off_line, &p);
        assert_eq!(record.stats.ponds_kept, 1);

        // Ruling S-12's half: node 5 on the line, at 5,000 m along, is the one the segment's
        // midpoint lands on, and it alone is dry against eleven wet ones -- so the floor is 1.0,
        // the midpoint is below it, and the corridor is never sampled. The bowl's own node is
        // still wet, so the per-candidate test would have kept it. Nothing is even found.
        let mut graph = graph_with_an_off_line_node(1_500.0);
        for w in graph.wetness.iter_mut() {
            *w = 1.0;
        }
        graph.wetness[5] = 0.0;
        let mut record = record_for(reach_along_the_equator(10.0));
        search(&mut record, &graph, &vec![NO_LAKE; graph.len()], &ground, &off_line, &p);
        assert_eq!(record.stats.ponds_found, 0, "Ruling S-12 skipped the whole corridor");
        assert_eq!(record.stats.ponds_kept, 0);
    }

    /// The outline is a closed ring of at least 3 points, in one fixed winding, and the same
    /// ring bit for bit on a second run.
    #[test]
    fn a_ponds_outline_is_a_closed_ring_in_a_fixed_winding() {
        let h = bowl(0.0, 5_000.0, 600.0, 5.0);
        let ground = Ground { height_m: &h, radius_m: R, corridor_m: 20_000.0, seed: 1 };
        let p = params();
        let graph = graph_under_the_line();
        let lake_of = vec![NO_LAKE; graph.len()];
        let mut record = record_for(reach_along_the_equator(10.0));
        search(&mut record, &graph, &lake_of, &ground, &h, &p);
        assert_eq!(record.bodies.len(), 1);
        let ring = &record.bodies[0].outline;
        assert!(ring.len() >= 3, "a traced outline is a ring, not {} points", ring.len());
        assert_ne!(ring[0], ring[ring.len() - 1],
                   "the ring closes implicitly: the first point is not repeated at the end");
        // Signed area on the equator, in degrees squared: the sign is the winding, and the
        // magnitude says the ring encloses something rather than doubling back on itself.
        let mut twice_area = 0.0;
        for i in 0..ring.len() {
            let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
            twice_area += a.1 * b.0 - b.1 * a.0;
        }
        // The reach here runs east along the equator, so the strip's `along` is east and its
        // `lateral` is north: the ring's clockwise-from-outside winding is clockwise in
        // (lon, lat) too, which is a negative shoelace.
        assert!(twice_area < 0.0,
                "the winding is fixed by the trace's four-edge convention: {twice_area}");

        let mut again = record_for(reach_along_the_equator(10.0));
        search(&mut again, &graph, &lake_of, &ground, &h, &p);
        assert_eq!(record.bodies, again.bodies, "the same ring, twice");
    }

    #[test]
    fn a_slope_with_no_dip_has_no_candidate() {
        let h = |p: &SpherePoint| { let (_, lon) = p.to_latlon(); 100.0 - 0.002 * lon * M_PER_DEG };
        let ground = Ground { height_m: &h, radius_m: R, corridor_m: 20_000.0, seed: 1 };
        let p = params();
        let found: usize = strips(&reach_along_the_equator(10.0), &ground, &h, &p)
            .iter().enumerate().map(|(i, s)| hollows_in(s, i, &p).len()).sum();
        assert_eq!(found, 0);
    }
}
