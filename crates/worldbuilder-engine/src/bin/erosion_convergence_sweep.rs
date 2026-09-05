//! Measures whether §14.3's iteration-to-convergence claim holds on this implementation.
//!
//! **This is the measurement Task 3 exists to make, not a shipped feature.** The design
//! doc (`docs/design/2026-09-02-mark-2-world-studio.md` §14.3) says the Cordonnier implicit
//! solver converges in "100-300 iterations" and, separately, that the count does not depend
//! on resolution -- the second half is why a planetary-scale bake is believed to fit in a
//! reasonable wall-clock budget at all. Nobody had tested either half of that claim on this
//! codebase before this binary existed: the figure comes from a paper measured on a 50 x 50
//! km *planar* domain with the authors' own uplift and erodibility, and this project runs
//! on a *sphere*, with *uniform* uplift, at whatever `k`/`dt`/threshold its own tests use --
//! a different population, which the number is allowed to disagree with.
//!
//! This is a `[[bin]]`, not an `#[ignore]`d test or a `cargo test`-visible example, for the
//! same reason `streambench.rs` is one: a sweep across six node counts up to 300,000, each
//! run to convergence (tens of thousands of `erode_step` calls per size at the threshold
//! chosen below), does not belong in a suite that runs on every push (`.superpowers/sdd/
//! 2026-09-04-slice-5a-erosion-solver/task-3-brief.md` says so explicitly), and it is a
//! single wall-clock question about one machine's run, not a property `cargo test` fixtures
//! are built to assert.
//!
//! ```text
//! cargo run --release --bin erosion_convergence_sweep
//! ```
//!
//! # Method (every figure below names its population, method and host)
//!
//! - **Host:** this developer machine, `cargo run --release` (release only -- a debug-build
//!   timing figure is not a figure anyone can use, and here the iteration COUNT is what is
//!   reported, not a wall-clock figure, but the run must still complete in a reasonable time
//!   to be run at all, which release mode is what makes six sizes practical).
//! - **Population, per node count `n`:** `stream::sample_nodes(SEED, n, EARTH_RADIUS_M)`
//!   (`SamplingKind::Spiral`, the same sampler `erosion.rs`'s own unit tests use), heights
//!   from `Surface::new(SEED, EARTH_RADIUS_M, ...)::elevation_m` (the same generator
//!   `streambench.rs` uses for a non-flat, non-synthetic field) rather than the noise
//!   fixture `erosion.rs`'s unit tests use, so this sweep exercises a realistic-shaped
//!   terrain rather than i.i.d. per-node noise.
//! - **Method:** `erosion::erode_to_convergence` with `u = UPLIFT_M_PER_YR`,
//!   `k = ERODIBILITY_PER_YR`, `dt = TIMESTEP_YR` (the same three values `erosion.rs`'s own
//!   `default_test_params` uses, so this sweep's numbers are directly comparable to the
//!   unit tests' own convergence tests, not a second unrelated calibration), threshold
//!   `MAX_HEIGHT_CHANGE_PER_STEP_M`, cap `MAX_ITERATIONS`.
//! - **Step between sizes:** roughly a half-decade (~3.16x, `10^0.5`) per step, six points
//!   from 300 to 100,000 -- just over two full decades end to end, more than the "at least
//!   four (points), spanning a decade if you can" the brief asks for.
//!
//! # A loose threshold would make the claim true by construction
//!
//! If `MAX_HEIGHT_CHANGE_PER_STEP_M` were, say, 10 metres -- an order of magnitude above one
//! step's own uplift contribution `u * dt` -- every size below would converge in a handful
//! of iterations regardless of resolution, and the sweep would have proven nothing. The
//! threshold chosen here (`1.0e-3` m, one-thousandth of `u * dt = 1.0` m) is the same one
//! `erosion.rs`'s own `a_converged_run_is_a_fixed_point` and
//! `erode_to_convergence_is_bit_identical_...` tests use at n = 300, where it takes on the
//! order of 10,000 iterations rather than under a hundred -- see this file's own printed
//! output for the exact count, and the run at 1.0 m and 1.0e-4 m below it for what a looser
//! or tighter threshold would have shown instead.

use std::time::Instant;

use worldbuilder_engine::erosion::{erode_to_convergence, receiver_distances_m, ErosionParams, ErosionRun};
use worldbuilder_engine::sphere::EARTH_RADIUS_M;
use worldbuilder_engine::stream::{sample_nodes, BuildParams, SamplingKind, StreamGraph};
use worldbuilder_engine::surface::Surface;

const SEED: i64 = 20_260_904;
const DATUM_M: f64 = 0.0;
const POND_MAX_M2: f64 = 5.0e9;

// The same three values erosion.rs's own `default_test_params()` uses, so the numbers this
// binary prints are the same solver configuration the unit tests already exercise at n =
// 300, just carried to more sizes.
const UPLIFT_M_PER_YR: f64 = 1.0e-3;
const ERODIBILITY_PER_YR: f64 = 1.0e-6;
const TIMESTEP_YR: f64 = 1000.0;

const NODE_COUNTS: [u32; 6] = [300, 1_000, 3_000, 10_000, 30_000, 100_000];

/// Three thresholds, not one -- so the sweep can show, at every size, that a tighter
/// threshold moves the count (the property that proves the sweep is not vacuous) rather
/// than reporting a single number that could have come from any threshold at all.
const THRESHOLDS_M: [f64; 3] = [1.0, 1.0e-3, 1.0e-4];

const MAX_ITERATIONS: u32 = 100_000;

fn run_one(seed: i64, count: u32, threshold_m: f64) -> (ErosionRun, f64) {
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

    let distances = receiver_distances_m(&graph, &sampling.positions);
    let params = ErosionParams {
        uplift_m_per_yr: UPLIFT_M_PER_YR,
        erodibility_per_yr: ERODIBILITY_PER_YR,
        timestep_yr: TIMESTEP_YR,
        max_height_change_per_step_m: threshold_m,
        max_iterations: MAX_ITERATIONS,
    };

    let t = Instant::now();
    let result = erode_to_convergence(&graph, &heights, &distances, &params);
    (result, t.elapsed().as_secs_f64())
}

fn main() {
    println!("erosion_convergence_sweep: seed {SEED}, u = {UPLIFT_M_PER_YR} m/yr, k = {ERODIBILITY_PER_YR} /yr, dt = {TIMESTEP_YR} yr");
    println!("population: SamplingKind::Spiral, Surface::new(seed, EARTH_RADIUS_M, 22, 0.29, None) heights, cap = {MAX_ITERATIONS}");
    println!();
    println!("{:>10}  {:>14}  {:>14}  {:>10}", "nodes", "threshold (m)", "iterations", "seconds");

    for &count in &NODE_COUNTS {
        for &threshold_m in &THRESHOLDS_M {
            let (result, seconds) = run_one(SEED, count, threshold_m);
            match result {
                ErosionRun::Converged { iterations, .. } => {
                    println!("{count:>10}  {threshold_m:>14}  {iterations:>10} c  {seconds:>10.2}");
                }
                ErosionRun::NotConverged { iterations, .. } => {
                    println!(
                        "{count:>10}  {threshold_m:>14}  {iterations:>8} NC  {seconds:>10.2}  <- hit the {MAX_ITERATIONS} cap, did not converge"
                    );
                }
            }
        }
    }

    println!();
    println!("'c' = converged at that iteration count. 'NC' = hit the cap without converging.");
    println!("See this crate's task-3-report.md for the verdict on §14.3's resolution-independence claim.");
}
