//! Measures whether §14.3's iteration-to-convergence claim holds, and against WHICH parameter.
//!
//! **This is the measurement Task 3 exists to make, not a shipped feature.** The design
//! doc (`docs/design/2026-09-02-mark-2-world-studio.md` §14.3) says the Cordonnier implicit
//! solver converges in "100-300 iterations" and, separately, that the count does not depend
//! on resolution -- the second half is why a planetary-scale bake is believed to fit in a
//! reasonable wall-clock budget at all.
//!
//! A first pass at this measurement (task-3-report.md's original text) ran the sweep below,
//! got real and reproducible numbers, and then over-claimed what they meant: it said "this
//! implementation" disagrees with §14.3, when what actually disagrees is one *chosen*
//! `k * dt`. Task 3's review caught this (task-3-review.md, findings 1 and 2) by naming the
//! dimensionless group the method actually responds to --
//!
//! ```text
//! c = k * dt * sqrt(A_drainage) / d
//! ```
//!
//! (see `erosion.rs::erode_step`'s doc for the derivation) -- and showing that at this
//! crate's default test constants `c` is small (~1.0e-3) *because of the constants*, not
//! because of the sphere or the mesh, and that raising `k` alone to put `c` near 0.1
//! reproduces §14.3's 100-300 band on the *same, unmodified* solver. This binary now prints
//! `c` alongside every iteration count, and runs both regimes, so that comparison is
//! reproducible from this file rather than living only in a review document.
//!
//! This is a `[[bin]]`, not an `#[ignore]`d test or a `cargo test`-visible example, for the
//! same reason `streambench.rs` is one: two sweeps across six node counts each, run to
//! convergence (thousands to tens of thousands of `erode_step` calls per size at the small-`c`
//! constants), do not belong in a suite that runs on every push (`.superpowers/sdd/
//! 2026-09-04-slice-5a-erosion-solver/task-3-brief.md` says so explicitly), and this is a
//! single wall-clock question about one machine's run, not a property `cargo test` fixtures
//! are built to assert.
//!
//! ```text
//! cargo run --release --bin erosion_convergence_sweep
//! ```
//!
//! # Method (every figure below names its population, method -- including `c` -- and host)
//!
//! - **Host:** this developer machine, `cargo run --release` (release only -- a debug-build
//!   timing figure is not a figure anyone can use, and here the iteration COUNT is what is
//!   reported, not a wall-clock figure, but the run must still complete in a reasonable time
//!   to be run at all, which release mode is what makes this file's 42 total runs (18 in
//!   Table 1, six node counts times three thresholds; 24 in Table 2, six node counts times
//!   four `k` values) practical).
//! - **Population, per node count `n`:** `stream::sample_nodes(SEED, n, EARTH_RADIUS_M)`
//!   (`SamplingKind::Spiral`, the same sampler `erosion.rs`'s own unit tests use), heights
//!   from `Surface::new(SEED, EARTH_RADIUS_M, ...)::elevation_m` (the same generator
//!   `streambench.rs` uses for a non-flat, non-synthetic field) rather than the noise
//!   fixture `erosion.rs`'s unit tests use, so this sweep exercises a realistic-shaped
//!   terrain rather than i.i.d. per-node noise.
//! - **Method:** `erosion::erode_to_convergence`, `u = UPLIFT_M_PER_YR` throughout. Table 1
//!   holds `k = ERODIBILITY_PER_YR`, `dt = TIMESTEP_YR` (the same three values `erosion.rs`'s
//!   own `default_test_params` uses) and varies the threshold over `THRESHOLDS_M`. Table 2
//!   holds the threshold at `THRESHOLDS_M[1]` (1.0e-3 m) and instead varies `k` over
//!   `ERODIBILITY_MULTIPLIERS`, which is the parameter `c` actually depends on and the one
//!   the review's decisive experiment varied. Every row of both tables prints `c`'s median
//!   and max over that row's own graph (computed by `c_stats`, over every draining/non-root
//!   node -- a root has no `c`, see `erosion.rs`'s module doc) so a reader can see which
//!   regime a row is in without cross-referencing a separate document.
//! - **Step between sizes:** roughly a half-decade (~3.16x, `10^0.5`) per step, six points
//!   from 300 to 100,000 -- just over two full decades end to end, more than the "at least
//!   four (points), spanning a decade if you can" the brief asks for.
//!
//! # A loose threshold, or reporting only one `c`, would make either claim true by construction
//!
//! If `THRESHOLDS_M` held only one loose value, every size would converge in a handful of
//! iterations regardless of resolution and the sweep would have proven nothing about
//! resolution-independence. Symmetrically, if this file only ever ran `c ≈ 1.0e-3`, it could
//! only ever report the "spec's number does not appear" half of the finding -- Table 2 exists
//! specifically so the file cannot make that mistake by construction: it must show the count
//! landing inside 100-300 at *some* `c` it actually measures, not merely assert that such a
//! `c` exists.

use worldbuilder_engine::detmath;
use worldbuilder_engine::erosion::{erode_to_convergence_with_clamp_counts, receiver_distances_m, ClampStats, ErosionParams, ErosionRun};
use worldbuilder_engine::sphere::EARTH_RADIUS_M;
use worldbuilder_engine::stream::{sample_nodes, BuildParams, SamplingKind, StreamGraph};
use worldbuilder_engine::surface::Surface;

const SEED: i64 = 20_260_904;
const DATUM_M: f64 = 0.0;
const POND_MAX_M2: f64 = 5.0e9;

// The same three values erosion.rs's own `default_test_params()` uses, so Table 1's numbers
// are the same solver configuration the unit tests already exercise at n = 300, just carried
// to more sizes.
const UPLIFT_M_PER_YR: f64 = 1.0e-3;
const ERODIBILITY_PER_YR: f64 = 1.0e-6;
const TIMESTEP_YR: f64 = 1000.0;

const NODE_COUNTS: [u32; 6] = [300, 1_000, 3_000, 10_000, 30_000, 100_000];

/// Three thresholds, not one -- so Table 1 can show, at every size, that a tighter threshold
/// moves the count (the property that proves the sweep is not vacuous) rather than reporting
/// a single number that could have come from any threshold at all. `[1]` (1.0e-3 m) is also
/// the fixed threshold Table 2 holds while it varies `k` instead.
const THRESHOLDS_M: [f64; 3] = [1.0, 1.0e-3, 1.0e-4];

/// `k = ERODIBILITY_PER_YR * multiplier`, for Table 2. `1.0` reproduces Table 1's middle
/// threshold row exactly, as a cross-check that the two tables agree where they overlap.
/// `100.0` puts `c`'s median near 0.1 (see this file's own printed output), the regime the
/// review's decisive experiment used to reproduce §14.3's 100-300 band.
const ERODIBILITY_MULTIPLIERS: [f64; 4] = [1.0, 10.0, 100.0, 1_000.0];

const MAX_ITERATIONS: u32 = 200_000;

/// One node population, built once and reused across every threshold/`k` this file runs
/// against it -- rebuilding the graph per row would not change any measured count, only
/// waste the time to build it again.
struct Population {
    graph: StreamGraph,
    heights: Vec<f64>,
    distances_m: Vec<f64>,
}

fn build_population(seed: i64, count: u32) -> Population {
    let world_seed = seed as u64; // cast-ok: two's-complement reinterpretation, as Surface::new makes
    let sampling = sample_nodes(world_seed, count, EARTH_RADIUS_M).expect("a node set");

    let surface = Surface::new(seed, EARTH_RADIUS_M, 22, 0.29, None);
    let heights: Vec<f64> = sampling.positions.iter().map(|p| surface.elevation_m(p, None)).collect();

    let graph = StreamGraph::build(
        &BuildParams {
            world_seed,
            radius_m: EARTH_RADIUS_M,
            sea_level_m: DATUM_M,
            sampling_kind: SamplingKind::Spiral,
            pond_max_drainage_area_m2: POND_MAX_M2,
        },
        &sampling.positions,
        &heights,
        &sampling.area_m2,
        &sampling.neighbours,
    )
    .expect("a graph over a real Surface field builds");

    let distances_m = receiver_distances_m(&graph, &sampling.positions);
    Population { graph, heights, distances_m }
}

/// `c = k * dt * sqrt(A_drainage) / d` (see `erosion.rs::erode_step`'s doc) over every
/// draining node in `pop` -- a root has no receiver and so no `c` (`distances_m` there is an
/// unread `0.0` placeholder, per `receiver_distances_m`'s own doc, so roots are excluded
/// rather than fed through and divided by zero). Returns `(median, max)`, both computed with
/// the house NaN-safe forms: sorting for the median rather than a mean (a mean would let one
/// extreme node dominate the whole figure the way `c`'s own long tail can), and the
/// `plates.rs::margin_at` fold form for the max -- never `f64::max`/`.max()`.
fn c_stats(pop: &Population, erodibility_per_yr: f64, timestep_yr: f64) -> (f64, f64) {
    let mut values: Vec<f64> = (0..pop.graph.node_count())
        .filter(|&node| pop.graph.downhill_of(node).is_some())
        .map(|node| {
            let idx = node as usize; // cast-ok: a node index into usize
            let area_m2 = pop.graph.drainage_area_m2(node);
            let distance_m = pop.distances_m[idx];
            erodibility_per_yr * timestep_yr * detmath::sqrt(area_m2) / distance_m
        })
        .collect();
    assert!(!values.is_empty(), "a graph with no draining nodes at all is not a meaningful fixture");
    values.sort_by(|a, b| a.partial_cmp(b).expect("c is never NaN: area and distance are both finite and positive on a built graph"));
    let median = values[values.len() / 2];
    let max = values.iter().fold(0.0f64, |largest, &v| if v > largest { v } else { largest });
    (median, max)
}

/// Calls [`erode_to_convergence_with_clamp_counts`], never the plain `erode_to_convergence`
/// wrapper -- Task 4 added a slope cap INSIDE this same loop (see `erosion.rs::erode_to_convergence`'s
/// "The cap runs per-iteration, not once at the end" section), and a sweep whose whole point
/// is measuring whether Task 3's counts moved cannot answer "did the cap ever fire" from the
/// iteration count alone: a count that stayed the same is consistent with either "the cap
/// never bound at this `(u, k)`" or "the cap bound but coincidentally left the count
/// unchanged", and those are different findings. `ClampStats` (returned alongside the
/// `ErosionRun`, printed by `print_row`) is what tells the two apart.
fn run_one(pop: &Population, erodibility_per_yr: f64, threshold_m: f64) -> (ErosionRun, ClampStats) {
    let params = ErosionParams {
        uplift_m_per_yr: UPLIFT_M_PER_YR,
        erodibility_per_yr,
        timestep_yr: TIMESTEP_YR,
        max_height_change_per_step_m: threshold_m,
        max_iterations: MAX_ITERATIONS,
    };
    erode_to_convergence_with_clamp_counts(&pop.graph, &pop.heights, &pop.distances_m, &params)
}

fn print_row(count: u32, label: &str, c_median: f64, c_max: f64, result: &ErosionRun, clamps: &ClampStats) {
    // "clamped: 0" is printed explicitly rather than left blank on a zero count -- a blank
    // column and an omitted one look identical to a reader, and this project has already
    // shipped more than one check where a zero that looked like agreement was actually "this
    // never ran." Printing it unconditionally means a reader sees the zero, not the absence
    // of a number.
    let clamp_note = format!("clamped: {} edges over {} iterations", clamps.total_edges_clamped, clamps.iterations_with_a_clamp);
    match result {
        ErosionRun::Converged { iterations, .. } => {
            println!("{count:>10}  {label:>14}  {c_median:>10.3e}  {c_max:>10.3e}  {iterations:>10} c  {clamp_note}");
        }
        ErosionRun::NotConverged { iterations, .. } => {
            println!(
                "{count:>10}  {label:>14}  {c_median:>10.3e}  {c_max:>10.3e}  {iterations:>8} NC  <- hit the {MAX_ITERATIONS} cap  {clamp_note}"
            );
        }
    }
}

fn main() {
    println!("erosion_convergence_sweep: seed {SEED}, u = {UPLIFT_M_PER_YR} m/yr, dt = {TIMESTEP_YR} yr, cap = {MAX_ITERATIONS}");
    println!("population: SamplingKind::Spiral, Surface::new(seed, EARTH_RADIUS_M, 22, 0.29, None) heights");
    println!("c = k * dt * sqrt(A_drainage) / d, over every draining node -- see erosion.rs::erode_step's doc");
    println!();

    println!("== Table 1: k = {ERODIBILITY_PER_YR} /yr fixed, threshold varied ==");
    println!(
        "{:>10}  {:>14}  {:>10}  {:>10}  {:>10}",
        "nodes", "threshold (m)", "c median", "c max", "iterations"
    );
    for &count in &NODE_COUNTS {
        let pop = build_population(SEED, count);
        let (c_median, c_max) = c_stats(&pop, ERODIBILITY_PER_YR, TIMESTEP_YR);
        for &threshold_m in &THRESHOLDS_M {
            let (result, clamps) = run_one(&pop, ERODIBILITY_PER_YR, threshold_m);
            print_row(count, &format!("{threshold_m}"), c_median, c_max, &result, &clamps);
        }
    }

    println!();
    println!("== Table 2: threshold = {} m fixed, k varied (the parameter c actually depends on) ==", THRESHOLDS_M[1]);
    println!(
        "{:>10}  {:>14}  {:>10}  {:>10}  {:>10}",
        "nodes", "k (/yr)", "c median", "c max", "iterations"
    );
    for &count in &NODE_COUNTS {
        let pop = build_population(SEED, count);
        for &multiplier in &ERODIBILITY_MULTIPLIERS {
            let k = ERODIBILITY_PER_YR * multiplier;
            let (c_median, c_max) = c_stats(&pop, k, TIMESTEP_YR);
            let (result, clamps) = run_one(&pop, k, THRESHOLDS_M[1]);
            print_row(count, &format!("{k:.1e}"), c_median, c_max, &result, &clamps);
        }
    }

    println!();
    println!("'c' = converged at that iteration count. 'NC' = hit the cap without converging.");
    println!("'clamped' = ClampStats from Task 4's slope cap, wired into this same loop -- edges");
    println!("corrected summed over every iteration, and how many iterations corrected at least one.");
    println!("See this crate's task-3-report.md for the corrected verdict on both halves of §14.3's claim,");
    println!("and task-4-report.md for whether the cap changed any of these counts.");
}
