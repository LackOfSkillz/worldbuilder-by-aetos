//! Task 12 (plan 1a) native survey of the coarse hydrology bake: the wall time of `bake`
//! itself, its drainage check, a memory proxy, and the body/reach counts the bake produces, at
//! three node counts on two worlds.
//!
//! ```text
//! cargo run --release --no-default-features --bin hydro_survey
//! ```
//!
//! This is a `[[bin]]`, not a `cargo test`-visible fixture, for the reason `streambench.rs`,
//! `pond_threshold_survey.rs` and `climate_survey.rs` already are: a 2,000,000-node bake is a
//! minutes-per-run, one-machine measurement, not a property a suite that runs on every push
//! should re-pay.
//!
//! **Native half only.** Step 2 of Task 12's brief (calibrating on the owner's saved studio
//! world, `worlds/world-1788998299904.json`, baked through the wasm build in the browser) is
//! the controller's half, per Ruling E in the task-12 ledger, and is not attempted here. The
//! `owner_survey` world below is a tectonics-shaped stand-in used only to see how the bake's
//! own cost and thresholds move on a tectonics world, natively -- it is not the owner's saved
//! world and its numbers do not replace Step 2's.
//!
//! # Method (every figure below names its population, its method with parameters, and its host)
//!
//! - **Host:** this developer machine, `cargo run --release --no-default-features`.
//! - **Worlds, each `Surface::new(seed, radius_m, plate_count, land_fraction, None, None,
//!   tectonics)`:**
//!   - `plain`: seed 20,260,904, radius 6,371,000 m, 12 plates, land fraction 0.29, no
//!     tectonics -- the crate's other everyday test/survey world.
//!   - `owner_survey`: seed 562,423,712, radius 4,500,000 m, 28 plates, land fraction 0.16,
//!     `TectonicParams::ranges()`.
//! - **Node counts:** 250,000 / 1,000,000 / 2,000,000, each run once, with `HydroParams::
//!   earth_like(n)` (no forced outlets, no threshold tuning -- Task 12's calibration step is
//!   the owner-world half).
//! - **Wall time:** `std::time::Instant` around the two halves `hydrology::bake` itself is made
//!   of, called here exactly as `bake` calls them -- `hydrology::bake_stages` (validation,
//!   `LandGraph::sample`, the flood, hollows and judging, `route`, `close_lakes`, and
//!   `flow::drainage_check`) and `hydrology::record_of` (reach extraction on the effective
//!   thresholds, bodies, notch filter, stats). Their sum is the time of the bake that ships; no
//!   step is re-implemented here (water 1a final review, I8: the survey used to re-stage the
//!   pipeline by hand, and had drifted from `bake`).
//! - **Drainage:** `hydrology::flow::drainage_check` on every world at every count, printed.
//!   `bake_stages` already refuses a routing that fails it (`HydroError::Drainage`), so a
//!   failure prints the refusal and the node instead of the run's figures.
//! - **Memory proxy:** the byte length of every `Vec` `LandGraph` holds -- `positions`
//!   (`size_of::<SpherePoint>()` each), `height_m`/`area_m2`/`wetness` (8 bytes each),
//!   `adj_start`/`enclosed` (4 bytes each, `adj_start` one longer than the node count),
//!   `adj` (4 bytes per directed half-edge), `ocean` (1 byte each) -- summed from each
//!   `Vec`'s own `len()`, not sampled from a live allocator. This is the bake's own working
//!   set; the wasm heap Step 2 reads separately also carries the wasm runtime and the studio's
//!   own state around it.
//! - **Median land-node area:** the sorted median of `graph.area_m2` restricted to nodes with
//!   `!graph.ocean[i]`, reported so the controller can reason about how coarse a "land node"
//!   is at each count, on each world.
//! - **Body counts by kind:** read off the record `record_of` returns (`Body::kind`), so
//!   they are the bake's own classification rather than a re-derivation of it.
//! - **Enclosed:** the count of hollows (any fate) whose own `.enclosed` flag is set -- a
//!   below-datum basin `LandGraph::label_water` did not call the ocean -- distinct from
//!   `closed`, which is the evaporative-closure outcome `flow::close_lakes` gives a kept
//!   basin.
//! - **Bifurcation ratios:** the record's own `BakeStats` min/max over
//!   `reaches::bifurcation_ratios`, over whatever orders it returns a ratio for (its own `>= 10` / `>= 1` population floors, per
//!   order, decide which orders appear at all -- reported as they fall, per the task brief,
//!   since `HydroParams::earth_like`'s thresholds are tuned for a much finer graph).

use std::time::Instant;

use worldbuilder_engine::hydrology::flow::drainage_check;
use worldbuilder_engine::hydrology::hollows::Fate;
use worldbuilder_engine::hydrology::{bake_stages, record_of, BodyKind, HydroError, HydroParams, ReachClass};
use worldbuilder_engine::sphere::SpherePoint;
use worldbuilder_engine::surface::Surface;
use worldbuilder_engine::tectonics::TectonicParams;

const NODE_COUNTS: &[u32] = &[250_000, 1_000_000, 2_000_000];

struct World {
    name: &'static str,
    surface: Surface,
}

fn worlds() -> Vec<World> {
    vec![
        World {
            name: "plain",
            surface: Surface::new(20_260_904, 6_371_000.0, 12, 0.29, None, None, None),
        },
        World {
            name: "owner_survey",
            surface: Surface::new(
                562_423_712,
                4_500_000.0,
                28,
                0.16,
                None,
                None,
                Some(TectonicParams::ranges()),
            ),
        },
    ]
}

/// House explicit-branch form for the median -- `f64::min`, `f64::max` and `.clamp(` are
/// banned by this slice's own house rules, matching `pond_threshold_survey.rs`'s own helpers.
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

struct RunResult {
    nodes: u32,
    land_nodes: u32,
    median_land_area_m2: f64,
    memory_bytes: u64,
    hollow_count: u32,
    kept: u32,
    notched: u32,
    closed: u32,
    enclosed: u32,
    lakes: u32,
    ponds: u32,
    salt_lakes: u32,
    salt_flats: u32,
    streams: u32,
    rivers: u32,
    great: u32,
    max_order: u32,
    bifurcation: Option<(f64, f64)>,
    drainage: Result<(), u32>,
    stages_s: f64,
    record_s: f64,
}

fn run(surface: &Surface, nodes: u32) -> Result<RunResult, HydroError> {
    let params = HydroParams::earth_like(nodes);

    // `hydrology::bake` is exactly these two calls, in this order.
    let t = Instant::now();
    let stages = bake_stages(surface, &params)?;
    let stages_s = t.elapsed().as_secs_f64();

    let t = Instant::now();
    let record = record_of(&stages, &params);
    let record_s = t.elapsed().as_secs_f64();

    let graph = &stages.graph;
    let drainage = drainage_check(graph, &stages.routing);

    // Memory proxy: see the module doc's "Memory proxy" paragraph for exactly what is summed.
    let point_size = std::mem::size_of::<SpherePoint>() as u64; // cast-ok: a type size, never negative
    let n = graph.len() as u64; // cast-ok: node count, bounded by stream::MAX_NODES
    let adj_len = graph.adj.len() as u64; // cast-ok: directed half-edge count, bounded by 2x adjacency
    let f64_bytes = 8u64;
    let u32_bytes = 4u64;
    let bool_bytes = 1u64;
    let memory_bytes = n * point_size
        + n * f64_bytes // height_m
        + n * f64_bytes // area_m2
        + (n + 1) * u32_bytes // adj_start
        + adj_len * u32_bytes // adj
        + n * bool_bytes // ocean
        + n * u32_bytes // enclosed
        + n * f64_bytes; // wetness

    let mut land_areas: Vec<f64> =
        (0..graph.len()).filter(|&i| !graph.ocean[i]).map(|i| graph.area_m2[i]).collect();
    land_areas.sort_unstable_by(|a, b| a.total_cmp(b));
    let land_nodes = land_areas.len() as u32; // cast-ok: bounded by total_nodes
    let median_land_area_m2 = if land_areas.is_empty() { 0.0 } else { median_of_sorted(&land_areas) };

    let hollows = &stages.hollows;
    let notched = hollows.iter().filter(|h| h.fate == Fate::Notch).count() as u32; // cast-ok: bounded by hollow count
    let enclosed = hollows.iter().filter(|h| h.enclosed).count() as u32; // cast-ok: bounded by hollow count

    let count_kind = |kind: BodyKind| record.bodies.iter().filter(|b| b.kind == kind).count() as u32; // cast-ok: bounded by body count
    let count_class = |class: ReachClass| record.reaches.iter().filter(|r| r.class == class).count() as u32; // cast-ok: bounded by reach count

    // `record_of` reports (0, 0) when no order met `bifurcation_ratios`' population floor.
    let bifurcation = if record.stats.bifurcation_min == 0.0 && record.stats.bifurcation_max == 0.0 {
        None
    } else {
        Some((record.stats.bifurcation_min, record.stats.bifurcation_max))
    };

    Ok(RunResult {
        nodes: record.stats.nodes,
        land_nodes,
        median_land_area_m2,
        memory_bytes,
        hollow_count: record.stats.hollows,
        kept: record.stats.kept,
        notched,
        closed: record.stats.closed,
        enclosed,
        lakes: count_kind(BodyKind::Lake),
        ponds: count_kind(BodyKind::Pond),
        salt_lakes: count_kind(BodyKind::SaltLake),
        salt_flats: count_kind(BodyKind::SaltFlat),
        streams: count_class(ReachClass::Stream),
        rivers: count_class(ReachClass::River),
        great: count_class(ReachClass::Great),
        max_order: record.stats.max_order,
        bifurcation,
        drainage,
        stages_s,
        record_s,
    })
}

fn print_result(r: &RunResult) {
    let drainage = match r.drainage {
        Ok(()) => "Ok".to_string(),
        Err(node) => format!("FAILED at node {node}"),
    };
    println!(
        "  n = {:>9}  bake {:>7.2} s  (bake_stages {:>6.2}  record_of {:>6.2})  drainage {}",
        r.nodes,
        r.stages_s + r.record_s,
        r.stages_s,
        r.record_s,
        drainage,
    );
    println!(
        "    memory proxy {:>10.3} MiB  land nodes {:>9}  median land area {:>10.3e} m^2",
        (r.memory_bytes as f64) / (1024.0 * 1024.0), // cast-ok: a byte count to f64 for a printed MiB figure
        r.land_nodes,
        r.median_land_area_m2,
    );
    println!(
        "    hollows {:>7}  kept {:>6}  notched {:>6}  closed {:>6}  enclosed {:>6}",
        r.hollow_count, r.kept, r.notched, r.closed, r.enclosed,
    );
    println!(
        "    bodies: lakes {:>5}  ponds {:>5}  salt lakes {:>5}  salt flats {:>5}",
        r.lakes, r.ponds, r.salt_lakes, r.salt_flats,
    );
    match r.bifurcation {
        Some((lo, hi)) => println!(
            "    reaches: streams {:>7}  rivers {:>6}  great {:>4}  max order {:>3}  \
             bifurcation ratio [{:.2}, {:.2}]",
            r.streams, r.rivers, r.great, r.max_order, lo, hi,
        ),
        None => println!(
            "    reaches: streams {:>7}  rivers {:>6}  great {:>4}  max order {:>3}  \
             bifurcation ratio: no order met the population floor",
            r.streams, r.rivers, r.great, r.max_order,
        ),
    }
}

fn main() {
    println!("hydro_survey: native half of Task 12's calibration (plan 1a)");
    println!("worlds: plain (20260904, 6.371 Mm, 12 plates, 0.29 land, no tectonics)");
    println!(
        "        owner_survey (562423712, 4.5 Mm, 28 plates, 0.16 land, TectonicParams::ranges())"
    );
    println!("node counts: 250,000 / 1,000,000 / 2,000,000, each with HydroParams::earth_like(n)");

    for world in worlds() {
        println!();
        println!("== {} ==", world.name);
        for &nodes in NODE_COUNTS {
            let t = Instant::now();
            let outcome = run(&world.surface, nodes);
            let wall_s = t.elapsed().as_secs_f64();
            match outcome {
                Ok(r) => print_result(&r),
                Err(HydroError::Drainage(node)) => {
                    println!("  n = {nodes:>9}  drainage FAILED: the bake refused, first bad node {node}")
                }
                Err(e) => println!("  n = {nodes:>9}  the bake refused: {e:?}"),
            }
            println!("    (wall clock for this run, including setup: {wall_s:.2} s)");
        }
    }
}
