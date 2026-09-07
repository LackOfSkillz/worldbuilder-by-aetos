//! Does the drainage kernel's second harmonic actually merge channels? Measured, on the
//! shipped `Detail::gully_offset_m`, and choosing nothing.
//!
//! ```text
//! cargo run --release --bin gully_merging_survey
//! ```
//!
//! This is a `[[bin]]`, not a `cargo test`-visible fixture, for the reason
//! `relief_survey.rs`, `mountain_survey.rs` and `pond_threshold_survey.rs` already are: a
//! 200,000-point site scan followed by dozens of 320x320 fields is a one-machine
//! measurement question, not a property a suite that runs on every push should re-pay.
//! **It is not in CI**, and `cargo test -- --list` shows it as a binary with zero tests.
//!
//! # Why this file exists rather than a note
//!
//! `.superpowers/sdd/notes/ridge-merging-spike.md` established, in a throwaway probe that
//! was deleted, that the shipped single-cosine shaping produces **0 confluences and 0
//! divergences in 319 contour-row pairs**, and that a second harmonic with a weight falling
//! downhill produces genuine Y-junctions. A deleted probe is a claim; this is the same
//! measurement against the code that ships, re-runnable by anyone.
//!
//! # What is measured, and what deliberately is not
//!
//! **Rows are contours.** The grid's `+j` axis is the fall line -- the negated steering
//! gradient at the site -- and `+i` runs along the contour. That is checked rather than
//! asserted: the run prints the structural row means at the top and the bottom of every
//! grid, and they must fall.
//!
//! - **Channels per contour, against downhill row.** The number of maximal runs of channel
//!   pixels in each row, its regression slope over the grid, its top-quarter and
//!   bottom-quarter means, and its ten deciles. **In a branching field this falls downhill;
//!   in a combed one it is flat.**
//! - **Width, the same way.** Mean run length per row, in metres.
//! - **Confluences and divergences, counted directly.** Runs in row `j` are matched to runs
//!   in row `j+1` by pixel overlap. A run in the LOWER row touching two or more runs of the
//!   upper row is a **confluence**; a run in the UPPER row touching two or more of the lower
//!   is a **divergence**. This is the connectivity walk Strahler ordering is defined over,
//!   and it is the metric that discriminated.
//!
//! **Two metrics are deliberately absent, and their absence is a finding of the spike's
//! section 5, not an oversight.** Skeleton junction counts (Zhang-Suen, 8-neighbour degree
//! >= 3) scored the NON-merging pinned-weight control at 218.57 junctions per 100 km against
//! the shipped comb's 90.23 -- thinning a thresholded noisy field manufactures spurs at a
//! rate set by how ragged the mask edge is, not by how the network branches. And correlation
//! of width with D8 contributing area gave +0.27 for the comb against +0.31 for the merging
//! cascade, because the accumulation is computed on the elevation the field itself carved: a
//! deep wide furrow collects more flow BECAUSE it is deep and wide, in a comb exactly as in
//! a network. Neither may be used as an acceptance metric here.
//!
//! # Populations, methods and host -- every figure below names all three
//!
//! - **Host.** `cargo run --release`, single-threaded, one developer machine. **No timing is
//!   claimed anywhere in this file**, so there are no spreads to report.
//! - **World.** `Surface::new(SEED, EARTH_RADIUS_M, PLATES, LAND, None, None, None)` --
//!   `DEFAULT_WORLD`, the one the viewer draws.
//! - **Sites.** `SCAN` Fibonacci-spiral points; those with `structural_m > 800` ranked by
//!   `|grad(structural_m)|` from the shipped `SteerLattice` at its own spacing. Ranks
//!   `SITE_RANKS` are measured, so the population spans a steep flank and a shallow one
//!   rather than only the steepest corner.
//! - **Grid.** `N`x`N` samples at `STEP_M` spacing, with the field asked at
//!   `resolution_m = RES_M` -- the viewer's finest post, so the FIELD is the one that ships
//!   and the SAMPLING is finer than it, which is what makes the topology resolvable.
//! - **Field.** `Detail::gully_offset_m` itself, at the shipped seed and radius, handed the
//!   same `steer`, `shaped` and `resolution_m` that `Surface::elevation_m` hands it. Not a
//!   reimplementation: the function under measurement is the function that ships.
//! - **Channel mask.** The lowest `q` of gated texels by field value, `q` swept over
//!   `MASKS`, so every variant is compared at equal channel AREA and no conclusion rests on
//!   one threshold.
//!
//! # The control is an ARGUMENT mutation now, and that is forced rather than chosen
//!
//! The merging slice's control pinned `harmonic_band_m` so `a` could not fall. That door closed
//! when the datum became local: `a = weight * smooth((shaped - reference) / band)` and the
//! reference rides the ground, so no `GullyParams` pins `a` flat and non-zero. A huge band gives
//! `a = 0` everywhere, which is the single cosine and is already in the list.
//!
//! So the control hands the SHIPPED kernel a doctored `Steer` -- a reference a fixed distance
//! below every query point, chosen by bisection to put `a` at **0.90**, the spike's own pinned
//! value. It measures **54.34 -> 55.40 channels, width 46.7 -> 46.7 m, 0 confluences : 0
//! divergences** on site 0, which is the parameter-mutation control's number to the digit. Two
//! ways of pinning `a`, one answer.

use worldbuilder_engine::detail::{Detail, GullyParams};
use worldbuilder_engine::detmath as m;
use worldbuilder_engine::sphere::{SpherePoint, EARTH_RADIUS_M};
use worldbuilder_engine::steer::{Steer, SteerLattice};
use worldbuilder_engine::surface::Surface;
use worldbuilder_engine::tangent::TangentFrame;
use worldbuilder_engine::vectors::Vec3;

const SEED: i64 = 20_260_904;
const PLATES: usize = 12;
const LAND: f64 = 0.29;
/// The resolution the field is asked for: the viewer's finest post.
const RES_M: f64 = 76.35;
/// Grid sample spacing, finer than `RES_M` so the topology is resolved rather than aliased.
const STEP_M: f64 = 40.0;
const N: usize = 320;
const SCAN: usize = 200_000;
const SITE_RANKS: [usize; 3] = [0, 1, 200];
const MASKS: [f64; 2] = [0.20, 0.30];

fn spiral(i: usize, n: usize) -> SpherePoint {
    let nf = n as f64; // cast-ok: a point count to a float
    let z = 1.0 - 2.0 * (i as f64 + 0.5) / nf; // cast-ok: a loop index to a float
    let r2 = 1.0 - z * z;
    let r = if r2 > 0.0 { m::sqrt(r2) } else { 0.0 };
    let ga = std::f64::consts::PI * (3.0 - m::sqrt(5.0));
    let t = ga * i as f64; // cast-ok: as above
    SpherePoint::from_vector(&Vec3::new(r * m::cos(t), r * m::sin(t), z)).expect("unit vector")
}

fn quantile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let idx = ((sorted.len() - 1) as f64 * q).round() as usize; // cast-ok: an index through a float fraction and back
    sorted[idx]
}

fn mean(values: &[f64]) -> f64 {
    let kept: Vec<f64> = values.iter().copied().filter(|x| x.is_finite()).collect();
    if kept.is_empty() {
        f64::NAN
    } else {
        kept.iter().sum::<f64>() / kept.len() as f64 // cast-ok: a count to a float
    }
}

/// Least-squares slope of a series against its own index, skipping non-finite entries.
fn regress(v: &[f64]) -> f64 {
    let pts: Vec<(f64, f64)> = v
        .iter()
        .enumerate()
        .filter(|(_, x)| x.is_finite())
        .map(|(j, x)| (j as f64, *x)) // cast-ok: a row index to a float
        .collect();
    if pts.len() < 3 {
        return f64::NAN;
    }
    let n = pts.len() as f64; // cast-ok: a count to a float
    let mx = pts.iter().map(|p| p.0).sum::<f64>() / n;
    let my = pts.iter().map(|p| p.1).sum::<f64>() / n;
    let num: f64 = pts.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    let den: f64 = pts.iter().map(|p| (p.0 - mx) * (p.0 - mx)).sum();
    num / den
}

struct Topology {
    rows_used: usize,
    runs_q1: f64,
    runs_q4: f64,
    runs_slope: f64,
    width_q1: f64,
    width_q4: f64,
    width_slope: f64,
    deciles: Vec<f64>,
    confluences: usize,
    divergences: usize,
    pairs: usize,
    moved_p95: f64,
}

fn analyse(field: &[f64], gated: &[bool], fraction: f64) -> Topology {
    let mut vals: Vec<f64> =
        (0..N * N).filter(|&k| gated[k]).map(|k| field[k]).filter(|v| v.is_finite()).collect();
    vals.sort_by(|a, b| a.partial_cmp(b).expect("finite"));
    let threshold = quantile(&vals, fraction);
    let mask: Vec<bool> = (0..N * N).map(|k| gated[k] && field[k] < threshold).collect();
    let mut moved: Vec<f64> = field.iter().filter(|v| **v != 0.0).map(|v| v.abs()).collect();
    moved.sort_by(|a, b| a.partial_cmp(b).expect("finite"));

    let row_complete = |j: usize| (0..N).all(|i| gated[j * N + i]);
    let runs_of = |j: usize| -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < N {
            if !mask[j * N + i] {
                i += 1;
                continue;
            }
            let start = i;
            while i < N && mask[j * N + i] {
                i += 1;
            }
            out.push((start, i));
        }
        out
    };

    let mut row_runs: Vec<f64> = Vec::with_capacity(N);
    let mut row_width: Vec<f64> = Vec::with_capacity(N);
    let mut rows_used = 0usize;
    for j in 0..N {
        if !row_complete(j) {
            row_runs.push(f64::NAN);
            row_width.push(f64::NAN);
            continue;
        }
        rows_used += 1;
        let runs = runs_of(j);
        let pixels: usize = runs.iter().map(|r| r.1 - r.0).sum();
        row_runs.push(runs.len() as f64); // cast-ok: a run count to a float
        row_width.push(if runs.is_empty() {
            f64::NAN
        } else {
            pixels as f64 / runs.len() as f64 // cast-ok: counts to floats for a mean
        });
    }

    // Confluences and divergences between adjacent contour rows.
    let mut confluences = 0usize;
    let mut divergences = 0usize;
    let mut pairs = 0usize;
    let overlap = |a: &(usize, usize), b: &(usize, usize)| a.0 < b.1 && b.0 < a.1;
    for j in 0..N - 1 {
        if !row_complete(j) || !row_complete(j + 1) {
            continue;
        }
        pairs += 1;
        let up = runs_of(j);
        let down = runs_of(j + 1);
        for d in &down {
            if up.iter().filter(|u| overlap(u, d)).count() >= 2 {
                confluences += 1;
            }
        }
        for u in &up {
            if down.iter().filter(|d| overlap(u, d)).count() >= 2 {
                divergences += 1;
            }
        }
    }

    let deciles: Vec<f64> =
        (0..10).map(|d| mean(&row_runs[d * N / 10..(d + 1) * N / 10])).collect();

    Topology {
        rows_used,
        runs_q1: mean(&row_runs[..N / 4]),
        runs_q4: mean(&row_runs[3 * N / 4..]),
        runs_slope: regress(&row_runs),
        width_q1: mean(&row_width[..N / 4]) * STEP_M,
        width_q4: mean(&row_width[3 * N / 4..]) * STEP_M,
        width_slope: regress(&row_width) * STEP_M,
        deciles,
        confluences,
        divergences,
        pairs,
        moved_p95: quantile(&moved, 0.95),
    }
}

fn print_row(name: &str, t: &Topology) {
    println!(
        "  {name:<34} rows {:>3} | channels/contour Q1 {:>6.2} -> Q4 {:>6.2} (slope {:>+8.4}/row) \
         | width {:>6.1} -> {:>6.1} m (slope {:>+6.3} m/row) | conf {:>3} : div {:>3} over {} pairs | |d| p95 {:>5.1} m",
        t.rows_used,
        t.runs_q1,
        t.runs_q4,
        t.runs_slope,
        t.width_q1,
        t.width_q4,
        t.width_slope,
        t.confluences,
        t.divergences,
        t.pairs,
        t.moved_p95,
    );
    let d: Vec<String> = t.deciles.iter().map(|x| format!("{x:.1}")).collect();
    println!("  {:<34} by downhill decile: [{}]", "", d.join(", "));
}

/// One site's grid: the points, their structural elevation, and the gate mask.
struct Site {
    index: usize,
    lat: f64,
    lon: f64,
    structural_m: f64,
    slope: f64,
    points: Vec<SpherePoint>,
    frames: Vec<TangentFrame>,
    steer: Vec<Steer>,
    shaped: Vec<f64>,
    gated: Vec<bool>,
    /// `Surface::elevation_m` at `RES_M` on the CANONICAL surface -- no drainage term at all.
    /// The amplitude sweep needs it because the figure that matters is the local relief of
    /// the GROUND, not of the term: `max - min` of a sum is not the sum of the `max - min`s,
    /// so measuring the term alone would report a linearity that is true by construction.
    plain_elev: Vec<f64>,
}

fn build_site(plain: &Surface, lattice: &SteerLattice, gate_m: f64, rank: usize, candidates: &[(f64, usize)]) -> Site {
    let structural = |p: &SpherePoint| plain.structural_m(p);
    let site = spiral(candidates[rank].1, SCAN);
    let (lat, lon) = site.to_latlon();
    let frame = TangentFrame::at(&site, EARTH_RADIUS_M);
    let g0 = lattice.at(&site, &frame, &structural);
    let s0 = m::sqrt(g0.grad_x * g0.grad_x + g0.grad_y * g0.grad_y);
    // +j is the fall line (the negated steering gradient); +i runs along the contour.
    let down = (-g0.grad_x / s0, -g0.grad_y / s0);
    let across = (-down.1, down.0);

    let mut points = Vec::with_capacity(N * N);
    let mut frames = Vec::with_capacity(N * N);
    let mut steer = Vec::with_capacity(N * N);
    let mut shaped = vec![0.0f64; N * N];
    let mut gated = vec![false; N * N];
    let mut plain_elev = vec![0.0f64; N * N];
    let half = (N as f64 - 1.0) / 2.0; // cast-ok: a grid size to a float
    for j in 0..N {
        for i in 0..N {
            let u = (i as f64 - half) * STEP_M; // cast-ok: a grid index to a float
            let v = (j as f64 - half) * STEP_M; // cast-ok: as above
            let p = frame
                .local_to_sphere(u * across.0 + v * down.0, u * across.1 + v * down.1);
            let k = j * N + i;
            let fr = TangentFrame::at(&p, EARTH_RADIUS_M);
            steer.push(lattice.at(&p, &fr, &structural));
            shaped[k] = plain.structural_m(&p);
            gated[k] = shaped[k] > gate_m;
            plain_elev[k] = plain.elevation_m(&p, Some(RES_M));
            frames.push(fr);
            points.push(p);
        }
    }
    Site {
        index: rank,
        lat,
        lon,
        structural_m: plain.structural_m(&site),
        slope: s0,
        points,
        frames,
        steer,
        shaped,
        gated,
        plain_elev,
    }
}

impl Site {
    fn field(&self, detail: &Detail) -> Vec<f64> {
        (0..N * N)
            .map(|k| {
                detail.gully_offset_m(
                    &self.points[k],
                    &self.frames[k],
                    self.steer[k],
                    self.shaped[k],
                    Some(RES_M),
                )
            })
            .collect()
    }

    /// **The control that makes this a finding, and it is an ARGUMENT mutation rather than a
    /// parameter one.**
    ///
    /// The old pinned-weight control set `harmonic_band_m` so that `a` could not fall. That
    /// door is shut now: `a` is `weight * smooth((shaped - reference) / band)`, so with the
    /// reference riding the ground there is no `GullyParams` that pins `a` flat and non-zero.
    /// A band of `1e9` makes `a` zero everywhere and is therefore the single cosine again --
    /// which is a real control, but it is the one already in the list.
    ///
    /// So the control hands the SHIPPED kernel a doctored reference instead: one that sits a
    /// fixed distance below the query point everywhere, so `a` is the same non-zero number at
    /// every texel. If merging came from the harmonic being PRESENT rather than from its
    /// weight falling, this row would merge too.
    fn field_at_constant_a(&self, detail: &Detail, offset_m: f64) -> Vec<f64> {
        (0..N * N)
            .map(|k| {
                detail.gully_offset_m(
                    &self.points[k],
                    &self.frames[k],
                    Steer::new(
                        self.steer[k].grad_x,
                        self.steer[k].grad_y,
                        self.shaped[k] - offset_m,
                    ),
                    self.shaped[k],
                    Some(RES_M),
                )
            })
            .collect()
    }

    fn row_mean(&self, j: usize) -> f64 {
        (0..N).map(|i| self.shaped[j * N + i]).sum::<f64>() / N as f64 // cast-ok: a count to a float
    }

    fn report(&self) {
        let gated_n = self.gated.iter().filter(|x| **x).count();
        println!(
            "\n================ site rank {} : lat {:.4} lon {:.4}, structural {:.1} m, |grad structural| {:.6} m/m ({:.4} deg)",
            self.index,
            self.lat,
            self.lon,
            self.structural_m,
            self.slope,
            m::atan2(self.slope, 1.0) * 180.0 / std::f64::consts::PI
        );
        println!(
            "  grid {N}x{N} at {STEP_M} m ({:.2} km across); gated texels {gated_n} of {} ({:.1}%); structural row means {:.1} m (row 0) -> {:.1} m (row {}) -- the +j axis really is downhill",
            N as f64 * STEP_M / 1000.0, // cast-ok: a grid size to a float
            N * N,
            gated_n as f64 / (N * N) as f64 * 100.0, // cast-ok: counts to floats for a percentage
            self.row_mean(0),
            self.row_mean(N - 1),
            N - 1
        );
        let mut hs: Vec<f64> =
            (0..N * N).filter(|&k| self.gated[k]).map(|k| self.shaped[k]).collect();
        hs.sort_by(|a, b| a.partial_cmp(b).expect("finite"));
        println!(
            "  gated structural p20 {:.1} m .. p80 {:.1} m (the WORLD elevation band -- what the pitchfork used to be keyed to)",
            quantile(&hs, 0.20),
            quantile(&hs, 0.80)
        );
        let (p10, p50, p90) = self.local_quantiles();
        println!(
            "  LOCAL relief `shaped - steer.reference_m` over gated texels: p10 {p10:+.3} m, p50 {p50:+.3} m, p90 {p90:+.3} m,              Q1 row mean {:+.3} m -> Q4 {:+.3} m -- THIS is what the pitchfork is keyed to now",
            self.local_quarter_mean(0),
            self.local_quarter_mean(3),
        );
    }

    /// The p10, p50 and p90 of `shaped - reference` over this site's gated texels: the band
    /// of local relief a `harmonic_band_m` has to span to reach anything here.
    fn local_quantiles(&self) -> (f64, f64, f64) {
        let mut ls: Vec<f64> = (0..N * N)
            .filter(|&k| self.gated[k])
            .map(|k| self.shaped[k] - self.steer[k].reference_m)
            .collect();
        ls.sort_by(|a, b| a.partial_cmp(b).expect("finite"));
        (quantile(&ls, 0.10), quantile(&ls, 0.50), quantile(&ls, 0.90))
    }

    /// The mean local relief over one quarter of the grid's rows, uphill quarter 0 to
    /// downhill quarter 3.
    fn local_quarter_mean(&self, quarter: usize) -> f64 {
        let mut total = 0.0;
        let mut count = 0.0;
        for j in quarter * N / 4..(quarter + 1) * N / 4 {
            for i in 0..N {
                total += self.shaped[j * N + i] - self.steer[j * N + i].reference_m;
                count += 1.0;
            }
        }
        total / count
    }

    /// **The mechanism check, without evaluating the field at all**: what fraction of this
    /// site's gated texels sit ABOVE the pitchfork locus, in the uphill quarter and in the
    /// downhill quarter. Two channels per period above the locus and one below it, so this
    /// fraction FALLING is the necessary condition for a downhill merge -- and a block whose
    /// fraction is 0 or 1 in both quarters cannot merge anything whatever the field does.
    fn above_locus(&self, gully: &GullyParams) -> (f64, f64) {
        let share = |quarter: usize| -> f64 {
            let mut above = 0.0;
            let mut count = 0.0;
            for j in quarter * N / 4..(quarter + 1) * N / 4 {
                for i in 0..N {
                    let k = j * N + i;
                    if !self.gated[k] {
                        continue;
                    }
                    count += 1.0;
                    if weight_at(gully, self.shaped[k] - self.steer[k].reference_m) > 0.25 {
                        above += 1.0;
                    }
                }
            }
            above / count
        };
        (share(0), share(3))
    }
}

fn detail_for(gully: GullyParams) -> Detail {
    Detail::with_gully(SEED as u64, EARTH_RADIUS_M, None, Some(gully)) // cast-ok: the same two's-complement reinterpretation `Surface::with_gully` does
}

/// The harmonic weight this block puts on a point sitting `local_m` metres above its own
/// steer cell's mean structural ground. **The argument is a local relief, not a world
/// elevation, and that is this slice**: see `GullyParams::harmonic_band_m`.
fn weight_at(gully: &GullyParams, local_m: f64) -> f64 {
    gully.harmonic_weight
        * worldbuilder_engine::detail::smooth(local_m / gully.harmonic_band_m)
}

/// The local relief at which `a` crosses 1/4 -- the pitchfork locus, in metres above the
/// cell mean. `None` when the block never reaches 1/4 anywhere, which is a block that merges
/// nothing and is worth saying out loud rather than printing as a number.
fn locus_of(gully: &GullyParams) -> Option<f64> {
    if weight_at(gully, gully.harmonic_band_m) < 0.25 {
        return None;
    }
    let (mut lo, mut hi) = (0.0, gully.harmonic_band_m);
    for _ in 0..80 {
        let mid = 0.5 * (lo + hi);
        if weight_at(gully, mid) < 0.25 {
            lo = mid
        } else {
            hi = mid
        }
    }
    Some(0.5 * (lo + hi))
}

fn main() {
    let drainage = GullyParams::drainage();
    println!("== gully merging survey ==");
    println!(
        "world Surface::new({SEED}, {EARTH_RADIUS_M}, {PLATES}, {LAND}); field = Detail::gully_offset_m at resolution_m = {RES_M}"
    );
    println!(
        "shipped preset: harmonic_weight {}, harmonic_band_m {} -> the pitchfork a = 1/4 sits at {} m of LOCAL relief above the steer cell's own mean",
        drainage.harmonic_weight,
        drainage.harmonic_band_m,
        match locus_of(&drainage) {
            Some(locus) => format!("{locus:+.1}"),
            None => "NOWHERE -- this block merges nothing".to_string(),
        }
    );

    let plain = Surface::new(SEED, EARTH_RADIUS_M, PLATES, LAND, None, None, None);
    let lattice = SteerLattice::new(EARTH_RADIUS_M, drainage.steer_lattice_m);
    let structural = |p: &SpherePoint| plain.structural_m(p);

    let mut candidates: Vec<(f64, usize)> = Vec::new();
    for i in 0..SCAN {
        let p = spiral(i, SCAN);
        if plain.structural_m(&p) < 800.0 {
            continue;
        }
        let fr = TangentFrame::at(&p, EARTH_RADIUS_M);
        let g = lattice.at(&p, &fr, &structural);
        candidates.push((m::sqrt(g.grad_x * g.grad_x + g.grad_y * g.grad_y), i));
    }
    candidates.sort_by(|a, b| b.0.partial_cmp(&a.0).expect("finite"));
    println!(
        "candidate flank sites (structural > 800 m, of {SCAN} spiral points): {}",
        candidates.len()
    );

    // The variants. `harmonic_weight: 0.0` is the shipped single-cosine shaping, bit for bit.
    // `harmonic_band_m: 1.0` pins the weight flat at its ceiling everywhere on gated ground:
    // THE CONTROL THAT MAKES THIS A FINDING -- it has the harmonic and it does not vary, and
    // if merging came from the harmonic's presence rather than from its fall this row would
    // merge too.
    let variants: Vec<(String, GullyParams)> = vec![
        ("SHIPPED single cosine (a = 0)".to_string(), GullyParams { harmonic_weight: 0.0, ..drainage }),
        ("harmonic, the preset".to_string(), drainage),
    ];
    // The offset that pins `a` at 0.90 -- the spike's own pinned value -- under the shipped
    // preset, found by bisection on the shipped `smooth` rather than written down.
    let pinned_offset = {
        let (mut lo, mut hi) = (0.0, drainage.harmonic_band_m);
        for _ in 0..80 {
            let mid = 0.5 * (lo + hi);
            if weight_at(&drainage, mid) < 0.90 { lo = mid } else { hi = mid }
        }
        0.5 * (lo + hi)
    };

    let mut sites: Vec<Site> = Vec::new();
    for rank in SITE_RANKS {
        sites.push(build_site(&plain, &lattice, drainage.gate_elevation_m, rank, &candidates));
    }

    for site in &sites {
        site.report();
        let local = site.local_quantiles();
        for (name, gully) in &variants {
            println!(
                "  -- {name}: a at this site's local-relief p10/p50/p90 = {:.3} / {:.3} / {:.3}",
                weight_at(gully, local.0),
                weight_at(gully, local.1),
                weight_at(gully, local.2)
            );
        }
        for fraction in MASKS {
            println!("  -- channel mask = lowest {:.0}% of gated texels --", fraction * 100.0);
            for (name, gully) in &variants {
                let field = site.field(&detail_for(*gully));
                print_row(name, &analyse(&field, &site.gated, fraction));
            }
            let pinned = site.field_at_constant_a(&detail_for(drainage), pinned_offset);
            print_row(
                &format!("CONTROL: a pinned at {:.2}, cannot fall", weight_at(&drainage, pinned_offset)),
                &analyse(&pinned, &site.gated, fraction),
            );
        }
    }

    // ------------------------------------------- the reference-span sweep, which comes first
    //
    // The pitchfork is keyed to `shaped - steer.reference_m`, so the FIRST question is what
    // that quantity's dynamic range and downhill trend actually are -- a band cannot be aimed
    // at a distribution nobody has measured, and the four-point mean at the lattice pitch has
    // a range of about 0.2 m, which is not a distribution a 1 m ABI floor can even reach.
    //
    // Population: the three sites' gated texels, re-steered on a lattice whose reference
    // stencil is the swept span and whose GRADIENT stencil is untouched at
    // `steer_lattice_m`. Q1/Q4 are the uphill and downhill quarters of the grid's rows.
    println!("
================ the reference-span sweep: what `shaped - reference_m` actually is");
    println!(
        "  {:<10} {:>8} | {}",
        "span (m)", "cells", "per site: local relief p10 / p50 / p90, and its Q1 -> Q4 row means"
    );
    for span_cells in [1.0f64, 2.0, 4.0, 8.0, 16.0, 32.0] {
        let span = span_cells * drainage.steer_lattice_m;
        let probe = SteerLattice::with_reference_span(EARTH_RADIUS_M, drainage.steer_lattice_m, span);
        let mut cells: Vec<String> = Vec::new();
        for site in &sites {
            let local: Vec<f64> = (0..N * N)
                .map(|k| {
                    site.shaped[k]
                        - probe.at(&site.points[k], &site.frames[k], &structural).reference_m
                })
                .collect();
            let mut sorted: Vec<f64> =
                (0..N * N).filter(|&k| site.gated[k]).map(|k| local[k]).collect();
            sorted.sort_by(|a, b| a.partial_cmp(b).expect("finite"));
            let row_mean = |rows: std::ops::Range<usize>| -> f64 {
                let mut total = 0.0;
                let mut count = 0.0;
                for j in rows {
                    for i in 0..N {
                        total += local[j * N + i];
                        count += 1.0;
                    }
                }
                total / count
            };
            cells.push(format!(
                "r{}: {:>+8.1} {:>+8.1} {:>+8.1} | {:>+8.1} -> {:>+8.1}",
                site.index,
                quantile(&sorted, 0.10),
                quantile(&sorted, 0.50),
                quantile(&sorted, 0.90),
                row_mean(0..N / 4),
                row_mean(3 * N / 4..N)
            ));
        }
        println!("  {span:<10.0} {span_cells:>8.0} | {}", cells.join("   "));
    }

    // ------------------------------------------------------------------ the cross product
    //
    // Swept as a CROSS PRODUCT, not one field at a time: the pitchfork's locus is
    // `w * smooth((h - gate)/band) = 1/4`, which moves when EITHER field moves, so a
    // one-at-a-time sweep would be measuring two different loci and calling it two axes.
    println!("\n================ the harmonic cross product, both sites, mask {:.0}%", MASKS[0] * 100.0);
    println!(
        "  {:<10} {:<10} {:>10} | {}",
        "weight",
        "band_m",
        "a=1/4 at",
        "per site: channels Q1->Q4, width Q1->Q4, conf:div, and the share of texels above the locus"
    );
    for weight in [1.4f64, 1.6, 1.8, 2.0, 2.4] {
        for band in [0.07f64, 0.09, 0.10, 0.12, 0.15, 0.20] {
            let gully = GullyParams { harmonic_weight: weight, harmonic_band_m: band, ..drainage };
            let crossing = match locus_of(&gully) {
                Some(locus) => format!("{locus:+.3}"),
                None => "never".to_string(),
            };
            let detail = detail_for(gully);
            let mut cells: Vec<String> = Vec::new();
            for site in &sites {
                let t = analyse(&site.field(&detail), &site.gated, MASKS[0]);
                let (a1, a4) = site.above_locus(&gully);
                cells.push(format!(
                    "r{}: n {:>5.1}->{:>5.1} ({:>+5.0}%) w {:>5.1}->{:>5.1} ({:>+5.0}%) {:>3}:{:<3} above {:>4.0}%->{:<4.0}%",
                    site.index,
                    t.runs_q1,
                    t.runs_q4,
                    (t.runs_q4 / t.runs_q1 - 1.0) * 100.0,
                    t.width_q1,
                    t.width_q4,
                    (t.width_q4 / t.width_q1 - 1.0) * 100.0,
                    t.confluences,
                    t.divergences,
                    a1 * 100.0,
                    a4 * 100.0
                ));
            }
            println!(
                "  {weight:<10.2} {band:<10.3} {crossing:>10} | {}",
                cells.join("  ")
            );
        }
    }

    // ------------------------------------------------------------------ the amplitude sweep
    //
    // `gully-wiring.md` section 10 concern 3 asked for this and nobody had run it: the
    // amplitude was measured at exactly ONE point (60 m -> 112 m of local relief), and a
    // slider was withheld because solving a band from one point assumes a linearity nobody
    // had measured. Local relief here is `max - min` of the gully term over a 2 km transect
    // along the contour at the middle row of each site's grid -- the same 2 km run Hammond's
    // landform classification uses and the same one `relief_survey.rs` samples, and the
    // figure the wiring note's 112 m came from.
    println!("\n================ amplitude sweep");
    println!(
        "  Local relief of the GROUND -- median over {} 2 km transects along the contour, one per\n  \
         grid row, of `max - min` of `Surface::elevation_m` at resolution {RES_M} m. Measuring the\n  \
         relief of the TERM instead would report a linearity that is true by construction, since\n  \
         `amplitude_m` is a bare multiplier on it; `max - min` of a SUM is not the sum of the\n  \
         `max - min`s, so this is the question the wiring note actually asked.\n  \
         The term is added linearly, so the field is evaluated ONCE at `amplitude_m = 1` per\n  \
         site and scaled -- CHECKED, not assumed: the first row's identity against a real\n  \
         `Surface::with_gully` at the preset amplitude is printed below.",
        N
    );
    // The identity check. If `features.apply` returned any authority on this ground, the
    // surface would damp the term and the scaling below would be wrong.
    {
        let gullied = Surface::with_gully(
            SEED,
            EARTH_RADIUS_M,
            PLATES,
            LAND,
            None,
            None,
            None,
            None,
            Some(drainage),
        );
        let detail = detail_for(drainage);
        let mut worst: f64 = 0.0;
        for site in &sites {
            let field = site.field(&detail);
            for k in (0..N * N).step_by(997) {
                let direct = gullied.elevation_m(&site.points[k], Some(RES_M));
                let composed = site.plain_elev[k] + field[k];
                let d = (direct - composed).abs();
                if d > worst {
                    worst = d;
                }
            }
        }
        println!(
            "  identity check: |Surface::with_gully(drainage) - (canonical + gully_offset_m)| <= {worst:.3e} m over {} samples across {} sites",
            (0..N * N).step_by(997).count() * sites.len(),
            sites.len()
        );
    }
    let unit = {
        let detail = detail_for(GullyParams { amplitude_m: 1.0, ..drainage });
        sites.iter().map(|s| s.field(&detail)).collect::<Vec<_>>()
    };
    println!("  {:<12} | {}", "amplitude_m", "per site: median 2 km relief (m), and the increment over canonical");
    let mut previous: Vec<f64> = vec![f64::NAN; sites.len()];
    for amplitude in [0.0f64, 15.0, 30.0, 45.0, 60.0, 90.0, 120.0, 180.0, 240.0] {
        let mut cells: Vec<String> = Vec::new();
        for (s, site) in sites.iter().enumerate() {
            let mut reliefs: Vec<f64> = Vec::with_capacity(N);
            for j in 0..N {
                let (i0, i1) = (N / 2 - 25, N / 2 + 25);
                let mut lo = f64::INFINITY;
                let mut hi = f64::NEG_INFINITY;
                for i in i0..i1 {
                    let v = site.plain_elev[j * N + i] + amplitude * unit[s][j * N + i];
                    if v < lo {
                        lo = v;
                    }
                    if v > hi {
                        hi = v;
                    }
                }
                reliefs.push(hi - lo);
            }
            reliefs.sort_by(|a, b| a.partial_cmp(b).expect("finite"));
            let relief = quantile(&reliefs, 0.5);
            cells.push(format!(
                "r{}: {:>7.2} m ({:>+6.2})",
                site.index,
                relief,
                relief - previous[s]
            ));
            previous[s] = relief;
        }
        println!("  {amplitude:<12.1} | {}", cells.join("   "));
    }

    println!("\ndone.");
}
