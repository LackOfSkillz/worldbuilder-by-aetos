//! Measures the distribution `stream.rs::BuildParams::pond_max_surface_area_m2` has to be
//! calibrated against, and is the source for task-3-report.md's addendum.
//!
//! **Owner decision, 2026-09-05: the pond/lake split moved from drainage area to surface
//! area.** This binary's own first-round measurement (drainage area vs surface area, over
//! the same bodies) was the evidence: only 17-24% overlap in which bodies either quantity
//! calls smallest, at every node count tried. That comparison is kept below, alongside the
//! new primary measurement -- the surface-area distribution this field now calibrates
//! against.
//!
//! ```text
//! cargo run --release --no-default-features --bin pond_threshold_survey
//! ```
//!
//! This is a `[[bin]]`, not a `cargo test`-visible fixture, for the reason `streambench.rs`
//! and `erosion_convergence_sweep.rs` already are: three full builds up to 500,000 nodes
//! plus the whole slice 5b water pipeline over each is a multi-second-per-size, one-machine
//! measurement, not a property a suite that runs on every push should re-pay.
//!
//! # Method (every figure below names its population, method, and host)
//!
//! - **Host:** this developer machine, `cargo run --release`.
//! - **Population, per node count `n`:** `stream::sample_nodes(SEED, n, EARTH_RADIUS_M)`
//!   (`SamplingKind::Spiral`), heights from `Surface::new(SEED, EARTH_RADIUS_M, 22, 0.29,
//!   None)::elevation_m` -- the same generator `streambench.rs` and `water.rs`'s own
//!   `real_graph` fixture use, so this is the crate's one real elevation field, not a
//!   synthetic fixture built to have lakes.
//! - **Method:** `StreamGraph::build` (a throwaway `pond_max_surface_area_m2` -- this
//!   binary's whole purpose is to choose that value, so the build-time placeholder
//!   classification it produces is discarded), then `water::fill_basins_and_apply` (Task 1)
//!   and `water::resolve_outflows_and_apply` (Task 2), then `water::
//!   lake_body_surface_areas_m2` and `water::lake_body_drainage_totals_m2` -- **one figure
//!   per physical body**, already folding a merged plateau's several roots into its one
//!   combined total, not one figure per pre-merge `Lake` row.
//! - **Node counts:** 30,000 / 100,000 / 500,000, matching Task 2's own re-derivation scale.
//!
//! # Reading the table
//!
//! For each node count: row/body counts, min/median/mean/max for both quantities, the
//! bottom-decile overlap between them (the evidence for the owner decision), then, for a
//! fixed candidate-threshold ladder over **surface area**, how many bodies fall at-or-below
//! each (the spec's own boundary), what fraction of all bodies that is, and how many are
//! strictly over it (so a reader can see whether the "Lake" side of the split holds a
//! roughly stable count across `n` while the "Pond" side grows -- the resolution question
//! task-3-report.md's addendum answers).

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use worldbuilder_engine::sphere::{SpherePoint, EARTH_RADIUS_M};
use worldbuilder_engine::stream::{sample_nodes, BuildParams, SamplingKind, StreamGraph, NO_LAKE};
use worldbuilder_engine::surface::Surface;
use worldbuilder_engine::water::{self, Basins};

const SEED: i64 = 20_260_905;
const DATUM_M: f64 = 0.0;
/// Deliberately unused as a real classification -- see the module doc. Large enough that
/// `StreamGraph::build`'s own placeholder pass calls everything a lake, so nothing this
/// binary reports about *bodies* is contaminated by that pass's pre-fill, per-root answer.
const THROWAWAY_POND_MAX_M2: f64 = 1.0e30;
const NODE_COUNTS: &[u32] = &[30_000, 100_000, 500_000];
/// A log-spaced ladder wide enough to bracket both "everything is a pond" and "nothing is",
/// so the table itself shows whether a break exists rather than assuming one and hunting
/// near it.
const CANDIDATE_THRESHOLDS_M2: &[f64] = &[
    1.0e4, 3.0e4, 1.0e5, 3.0e5, 1.0e6, 3.0e6, 1.0e7, 3.0e7, 1.0e8, 3.0e8, 1.0e9, 3.0e9, 1.0e10,
    3.0e10, 1.0e11, 3.0e11, 1.0e12,
];

fn build_graph(count: u32) -> StreamGraph {
    let world_seed = SEED as u64; // cast-ok: two's-complement reinterpretation, as Surface::new makes
    let sampling = sample_nodes(world_seed, count, EARTH_RADIUS_M).expect("a node set");
    let field = Surface::new(SEED, EARTH_RADIUS_M, 22, 0.29, None, None, None);
    let heights: Vec<f64> =
        sampling.positions.iter().map(|p: &SpherePoint| field.elevation_m(p, None)).collect();
    StreamGraph::build(
        &BuildParams {
            world_seed,
            radius_m: EARTH_RADIUS_M,
            sea_level_m: DATUM_M,
            sampling_kind: SamplingKind::Spiral,
            pond_max_surface_area_m2: THROWAWAY_POND_MAX_M2,
        },
        &sampling.positions,
        &heights,
        &sampling.area_m2,
        &sampling.neighbours,
    )
    .expect("a sampled planet builds")
}

/// House explicit-branch form for `min`/`max`/percentile-style folds -- `f64::min`,
/// `f64::max` and `.clamp(` are banned by this slice's own brief (not caught by the build
/// guard, which is exactly why they must not appear here), matching `plates.rs::margin_at`
/// and `water.rs::merge_tied_plateaus`' own `if h_inside > h_outside { .. } else { .. }` form.
fn min_of(values: &[f64]) -> f64 {
    let mut m = values[0];
    for &v in &values[1..] {
        if v < m {
            m = v;
        }
    }
    m
}

fn max_of(values: &[f64]) -> f64 {
    let mut m = values[0];
    for &v in &values[1..] {
        if v > m {
            m = v;
        }
    }
    m
}

fn mean_of(values: &[f64]) -> f64 {
    let mut total = 0.0;
    for &v in values {
        total += v;
    }
    total / (values.len() as f64) // cast-ok: a body count to f64 for an average, not a lattice decision
}

/// Sorted-array median; `values` must already be sorted ascending.
fn median_of_sorted(values: &[f64]) -> f64 {
    let n = values.len();
    if n % 2 == 1 {
        values[n / 2]
    } else {
        let a = values[n / 2 - 1];
        let b = values[n / 2];
        (a + b) / 2.0
    }
}

/// The pairing this binary needs and `water.rs` does not expose (its own accessors return
/// unordered per-body totals, which is all a real caller needs): every physical body's
/// member `root_node`s, grouped the same way `water::classify_lake_kinds` groups them
/// internally (a satellite's `outflow_lake` names another row at a bit-identical
/// `level_m`). Duplicated here rather than exposed from `water.rs`, deliberately -- this is
/// a one-off measurement question (which bodies do two quantities each call smallest), not
/// something a caller of the real classification ever needs.
fn lake_body_groups(graph: &StreamGraph) -> Vec<Vec<u32>> {
    let lakes = graph.lakes();
    let index_of_root: HashMap<u32, usize> =
        lakes.iter().enumerate().map(|(i, l)| (l.root_node, i)).collect();
    let mut body: Vec<usize> = (0..lakes.len()).collect();
    for (i, lake) in lakes.iter().enumerate() {
        if lake.outflow_lake == NO_LAKE {
            continue;
        }
        if let Some(&target) = index_of_root.get(&lake.outflow_lake) {
            if lakes[target].level_m.to_bits() == lake.level_m.to_bits() {
                body[i] = target;
            }
        }
    }
    let mut groups: HashMap<usize, Vec<u32>> = HashMap::new();
    for (i, lake) in lakes.iter().enumerate() {
        groups.entry(body[i]).or_default().push(lake.root_node);
    }
    groups.into_values().collect()
}

fn body_surface_area_m2(graph: &StreamGraph, basins: &Basins, roots: &[u32]) -> f64 {
    let mut total = 0.0;
    for &root in roots {
        let level_m = graph.lake_at(root).expect("root names a lake in this table").level_m;
        for &member in basins.members_of(root) {
            if graph.height_m(member) <= level_m {
                total += graph.area_m2(member);
            }
        }
    }
    total
}

fn main() {
    println!("pond_threshold_survey: seed {SEED}, radius {EARTH_RADIUS_M} m");
    println!(
        "  per node count: rows = pre-merge Lake entries, bodies = post-merge physical \
         bodies (this task's own unit)"
    );

    for &count in NODE_COUNTS {
        let t = Instant::now();
        let mut graph = build_graph(count);
        let build_s = t.elapsed().as_secs_f64();

        let rows = graph.lakes().len();

        let t = Instant::now();
        let basins = water::fill_basins_and_apply(&mut graph);
        water::resolve_outflows_and_apply(&mut graph, &basins);
        let water_s = t.elapsed().as_secs_f64();

        let mut drainage = water::lake_body_drainage_totals_m2(&graph);
        drainage.sort_by(|a, b| a.partial_cmp(b).expect("no NaN drainage area"));
        let mut surface = water::lake_body_surface_areas_m2(&graph, &basins);
        surface.sort_by(|a, b| a.partial_cmp(b).expect("no NaN surface area"));
        let bodies = surface.len();

        println!();
        println!(
            "n = {count:>7}  build {build_s:>6.2} s  water {water_s:>6.2} s  rows {rows:>6}  \
             bodies {bodies:>6}  (rows - bodies = {} merge satellites)",
            rows - bodies,
        );
        if bodies == 0 {
            println!("  no lake bodies at this node count -- nothing to classify");
            continue;
        }
        println!(
            "  drainage area (m^2): min {:.3e}  median {:.3e}  mean {:.3e}  max {:.3e}",
            min_of(&drainage),
            median_of_sorted(&drainage),
            mean_of(&drainage),
            max_of(&drainage),
        );
        println!(
            "  surface area  (m^2): min {:.3e}  median {:.3e}  mean {:.3e}  max {:.3e}",
            min_of(&surface),
            median_of_sorted(&surface),
            mean_of(&surface),
            max_of(&surface),
        );

        // The evidence for the owner decision: do the two quantities agree on which bodies
        // are smallest? Paired body-for-body via `lake_body_groups`, not by independently
        // sorted rank, so the overlap below is exact membership, not an approximation.
        let groups = lake_body_groups(&graph);
        let paired: Vec<(f64, f64)> = groups
            .iter()
            .map(|roots| {
                let d: f64 = roots.iter().map(|&r| graph.drainage_area_m2(r)).sum();
                let s = body_surface_area_m2(&graph, &basins, roots);
                (d, s)
            })
            .collect();
        let decile = if bodies / 10 == 0 { 1 } else { bodies / 10 };
        let mut by_drainage: Vec<usize> = (0..paired.len()).collect();
        by_drainage.sort_by(|&a, &b| paired[a].0.partial_cmp(&paired[b].0).expect("no NaN"));
        let mut by_surface: Vec<usize> = (0..paired.len()).collect();
        by_surface.sort_by(|&a, &b| paired[a].1.partial_cmp(&paired[b].1).expect("no NaN"));
        let bottom_drainage: HashSet<usize> = by_drainage[..decile].iter().copied().collect();
        let bottom_surface: HashSet<usize> = by_surface[..decile].iter().copied().collect();
        let overlap = bottom_drainage.intersection(&bottom_surface).count();
        let overlap_fraction = (overlap as f64) / (decile as f64); // cast-ok: two body counts to f64 for a printed fraction
        println!(
            "  bottom decile ({decile} of {bodies} bodies) by drainage area vs by surface \
             area: {overlap} in common ({overlap_fraction:.2} overlap)"
        );

        println!(
            "  candidate threshold (surface) ->  ponds (<=) / lakes (>) / bodies   fraction pond"
        );
        for &threshold in CANDIDATE_THRESHOLDS_M2 {
            let ponds = surface.iter().filter(|&&a| a <= threshold).count();
            let lakes_over = bodies - ponds;
            let fraction = (ponds as f64) / (bodies as f64); // cast-ok: two body counts to f64 for a printed fraction
            println!(
                "    {threshold:>10.3e}  ->  {ponds:>6} / {lakes_over:>6} / {bodies:<6}   {fraction:>6.4}",
            );
        }
    }
}
