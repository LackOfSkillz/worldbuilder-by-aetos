//! Native survey of the hydrology bake: the wall time of each part of `hydrology::bake`, its
//! drainage check, a memory proxy, the body/reach counts the bake produces, and (plan 1b-2,
//! Task 8) the refinement's own time, point counts, falls, capped-basin counts and record size.
//!
//! ```text
//! cargo run --release --no-default-features --bin hydro_survey -- \
//!     [--simplify M] [--pond-density M2] [--pond-radius M] [--pond-cell M] [NODES ...]
//! ```
//!
//! With no `NODES`, every world is baked at 1,000,000 nodes (plan 1b-2, Task 8). Each flag
//! overrides one `HydroParams` field for the run, so a gate step can be measured before
//! `earth_like` is changed; without it the bake takes `earth_like`'s value, as a wasm bake does.
//! `--simplify` is `refine_simplify_m` (plan 1b-2, Task 8); `--pond-density`, `--pond-radius` and
//! `--pond-cell` are `pond_density_area_m2`, `pond_search_radius_m` and `pond_cell_m`, the size
//! and time levers of plan 1b-3's Task 7. `--help` prints this usage.
//!
//! This is a `[[bin]]`, not a `cargo test`-visible fixture, for the reason `streambench.rs`,
//! `pond_threshold_survey.rs` and `climate_survey.rs` already are: a 1,000,000-node bake is a
//! minutes-per-run, one-machine measurement, not a property a suite that runs on every push
//! should re-pay.
//!
//! **Native only.** The owner's saved studio world (`worlds/world-1788998299904.json`) is baked
//! through the wasm build in the browser by the controller. The worlds below are stand-ins used
//! to see how the bake's cost and size move, natively -- their numbers do not replace the owner
//! world's.
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
//!   - `seed1_ranges`: seed 1, radius 6,371,000 m, 12 plates, land fraction 0.40,
//!     `TectonicParams::ranges()` -- the world `real_worlds_drain_everything_through_the_full_bake`
//!     bakes at 10,000 nodes.
//! - **Params:** `HydroParams::earth_like(n)` (no forced outlets, no threshold tuning), with
//!   `refine_simplify_m` replaced only when `--simplify` is given.
//! - **Wall time:** `std::time::Instant` around the four parts `hydrology::bake` itself is made
//!   of, called here exactly as `bake` calls them -- `hydrology::bake_stages` (validation,
//!   `LandGraph::sample`, the flood, hollows and judging, `route`, `close_lakes`, and
//!   `flow::drainage_check`), `hydrology::record_of` (reach extraction on the effective
//!   thresholds, bodies, the crossing pass, notch filter, stats), `refine::refine` (tracing,
//!   falls, meander and simplification) and `ponds::search` with its own `ponds::pond_ground`
//!   (plan 1b-3: the fine pond search, strips along the refined lines), on a `refine::Ground`
//!   built exactly as `bake` builds it. Their sum is the time of the bake that ships; no step is
//!   re-implemented here (water 1a final review, I8). `record::encode` is timed separately and is
//!   not part of `bake`.
//! - **Crossings and ponds:** the record's own `BakeStats::crossings_coarse`, `crossings_left`,
//!   `ponds_found` and `ponds_kept`, so they are the bake's own counts rather than a
//!   re-derivation.
//! - **Record size:** `record::encode(&record).len() * 8` bytes -- the words the wasm export
//!   hands the studio.
//! - **Points:** `coarse` is the sum of `ReachLine::points` lengths after `record_of`, `refined`
//!   the same sum after `refine`. Notch points are the sum of `NotchLine::points` lengths.
//! - **Duplicate notch points (Ruling R-9):** a `(lat, lon)` pair (compared bit for bit) that
//!   appears in more than one notch line. `keys` counts such pairs; `extra` counts every
//!   appearance past the first line that holds it (the words it costs are `extra * 4 * 8`
//!   bytes). A pair repeated inside one line is not counted.
//! - **Drainage:** `hydrology::flow::drainage_check` on every world at every count, printed.
//!   `bake_stages` already refuses a routing that fails it (`HydroError::Drainage`), so a
//!   failure prints the refusal and the node instead of the run's figures.
//! - **Memory proxy:** the byte length of every `Vec` `LandGraph` holds -- `positions`
//!   (`size_of::<SpherePoint>()` each), `height_m`/`area_m2`/`wetness` (8 bytes each),
//!   `adj_start`/`enclosed` (4 bytes each, `adj_start` one longer than the node count),
//!   `adj` (4 bytes per directed half-edge), `ocean` (1 byte each) -- summed from each
//!   `Vec`'s own `len()`, not sampled from a live allocator.
//! - **Median land-node area:** the sorted median of `graph.area_m2` restricted to nodes with
//!   `!graph.ocean[i]`.
//! - **Body counts by kind:** read off the record (`Body::kind`), so they are the bake's own
//!   classification rather than a re-derivation of it.
//! - **Enclosed:** the count of hollows (any fate) whose own `.enclosed` flag is set, distinct
//!   from `closed`, the evaporative-closure outcome `flow::close_lakes` gives a kept basin.
//! - **Capped basins:** the record's own `BakeStats::capped_basins`, `capped_inner` and
//!   `capped_inner_kept` (carry-forward I3).
//! - **Bifurcation ratios:** the record's own `BakeStats` min/max over
//!   `reaches::bifurcation_ratios`, over whatever orders it returns a ratio for.

use std::collections::BTreeMap;
use std::time::Instant;

use worldbuilder_engine::hydrology::flow::drainage_check;
use worldbuilder_engine::hydrology::hollows::Fate;
use worldbuilder_engine::hydrology::{
    bake_stages, ponds, record, record_of, refine, BodyKind, HydroError, HydroParams, HydroRecord,
    ReachClass,
};
use worldbuilder_engine::sphere::SpherePoint;
use worldbuilder_engine::surface::Surface;
use worldbuilder_engine::tectonics::TectonicParams;

const DEFAULT_NODES: u32 = 1_000_000;

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
        World {
            name: "seed1_ranges",
            surface: Surface::new(1, 6_371_000.0, 12, 0.40, None, None, Some(TectonicParams::ranges())),
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

fn reach_points(record: &HydroRecord) -> u64 {
    record.reaches.iter().map(|r| r.points.len() as u64).sum() // cast-ok: a point count, never negative
}

/// `(keys, extra)`: see the module doc's "Duplicate notch points" paragraph.
fn duplicate_notch_points(record: &HydroRecord) -> (u64, u64) {
    let mut lines_holding: BTreeMap<(u64, u64), u64> = BTreeMap::new();
    for notch in &record.notches {
        let mut seen: Vec<(u64, u64)> =
            notch.points.iter().map(|&(lat, lon, _, _)| (lat.to_bits(), lon.to_bits())).collect();
        seen.sort_unstable();
        seen.dedup();
        for key in seen {
            *lines_holding.entry(key).or_insert(0) += 1;
        }
    }
    let mut keys = 0u64;
    let mut extra = 0u64;
    for &lines in lines_holding.values() {
        if lines > 1 {
            keys += 1;
            extra += lines - 1;
        }
    }
    (keys, extra)
}

/// `HydroParams` fields this run overrides, each `None` meaning "take `earth_like`'s value, as a
/// wasm bake does". `--simplify` is plan 1b-2's; the three pond flags are plan 1b-3 Task 7's size
/// and time levers, so the gate steps can be measured without editing `earth_like` between runs.
#[derive(Clone, Copy, Default)]
struct Overrides {
    simplify_m: Option<f64>,
    pond_density_area_m2: Option<f64>,
    pond_search_radius_m: Option<f64>,
    pond_cell_m: Option<f64>,
}

impl Overrides {
    fn apply(&self, params: &mut HydroParams) {
        if let Some(v) = self.simplify_m {
            params.refine_simplify_m = v;
        }
        if let Some(v) = self.pond_density_area_m2 {
            params.pond_density_area_m2 = v;
        }
        if let Some(v) = self.pond_search_radius_m {
            params.pond_search_radius_m = v;
        }
        if let Some(v) = self.pond_cell_m {
            params.pond_cell_m = v;
        }
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
    refine_s: f64,
    ponds_s: f64,
    encode_s: f64,
    crossings_coarse: u32,
    crossings_left: u32,
    ponds_found: u32,
    ponds_kept: u32,
    coarse_points: u64,
    refined_points: u64,
    notch_lines: u64,
    notch_points: u64,
    duplicate_notch_keys: u64,
    duplicate_notch_extra: u64,
    falls: u64,
    capped_basins: u32,
    capped_inner: u32,
    capped_inner_kept: u32,
    record_bytes: u64,
}

fn run(surface: &Surface, nodes: u32, overrides: Overrides) -> Result<RunResult, HydroError> {
    let mut params = HydroParams::earth_like(nodes);
    overrides.apply(&mut params);

    // `hydrology::bake` is exactly these four calls, in this order, on this `Ground`.
    let t = Instant::now();
    let stages = bake_stages(surface, &params)?;
    let stages_s = t.elapsed().as_secs_f64();

    let t = Instant::now();
    let mut record = record_of(&stages, &params);
    let record_s = t.elapsed().as_secs_f64();

    let coarse_points = reach_points(&record);
    let height = |p: &SpherePoint| surface.structural_m(p);
    let ground = refine::Ground::for_surface(surface, &height, &params);
    let t = Instant::now();
    refine::refine(&mut record, &ground, &params);
    let refine_s = t.elapsed().as_secs_f64();

    // Plan 1b-3, Task 7: the fine pond search is the bake's fourth part and is timed as its own.
    // `pond_ground` is built inside the timed region because `bake` builds it there too.
    let t = Instant::now();
    let detail = ponds::pond_ground(surface, &params);
    ponds::search(&mut record, &stages.graph, &stages.routing.lake_of, &ground, &detail, &params);
    let ponds_s = t.elapsed().as_secs_f64();

    let t = Instant::now();
    let words = record::encode(&record);
    let encode_s = t.elapsed().as_secs_f64();
    let record_bytes = words.len() as u64 * 8; // cast-ok: a word count, never negative

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

    let (duplicate_notch_keys, duplicate_notch_extra) = duplicate_notch_points(&record);
    let notch_points: u64 = record.notches.iter().map(|l| l.points.len() as u64).sum(); // cast-ok: a point count

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
        refine_s,
        ponds_s,
        encode_s,
        crossings_coarse: record.stats.crossings_coarse,
        crossings_left: record.stats.crossings_left,
        ponds_found: record.stats.ponds_found,
        ponds_kept: record.stats.ponds_kept,
        coarse_points,
        refined_points: reach_points(&record),
        notch_lines: record.notches.len() as u64, // cast-ok: a line count
        notch_points,
        duplicate_notch_keys,
        duplicate_notch_extra,
        falls: record.falls.len() as u64, // cast-ok: a fall count
        capped_basins: record.stats.capped_basins,
        capped_inner: record.stats.capped_inner,
        capped_inner_kept: record.stats.capped_inner_kept,
        record_bytes,
    })
}

fn print_result(r: &RunResult) {
    let drainage = match r.drainage {
        Ok(()) => "Ok".to_string(),
        Err(node) => format!("FAILED at node {node}"),
    };
    println!(
        "  n = {:>9}  bake {:>7.2} s  (bake_stages {:>6.2}  record_of {:>6.2}  refine {:>6.2}  \
         ponds {:>6.2})  encode {:>5.2} s  drainage {}",
        r.nodes,
        r.stages_s + r.record_s + r.refine_s + r.ponds_s,
        r.stages_s,
        r.record_s,
        r.refine_s,
        r.ponds_s,
        r.encode_s,
        drainage,
    );
    println!(
        "    record {:>10} bytes ({:.3} MB)  reach points coarse {:>8} -> refined {:>8}  falls {:>6}",
        r.record_bytes,
        (r.record_bytes as f64) / 1.0e6, // cast-ok: a byte count to f64 for a printed MB figure
        r.coarse_points,
        r.refined_points,
        r.falls,
    );
    println!(
        "    crossings: coarse {:>5}  left after the pass {:>5}   ponds: found {:>6}  kept {:>6}",
        r.crossings_coarse, r.crossings_left, r.ponds_found, r.ponds_kept,
    );
    println!(
        "    notches: lines {:>6}  points {:>8}  duplicate points: keys {:>6}  extra {:>6} ({} bytes)",
        r.notch_lines,
        r.notch_points,
        r.duplicate_notch_keys,
        r.duplicate_notch_extra,
        r.duplicate_notch_extra * 4 * 8,
    );
    println!(
        "    capped basins {:>5}  capped inner {:>5}  capped inner kept {:>5}",
        r.capped_basins, r.capped_inner, r.capped_inner_kept,
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

const USAGE: &str = "usage: hydro_survey [--simplify M] [--pond-density M2] [--pond-radius M] \
    [--pond-cell M] [NODES ...]\n\
    \n\
    Bakes three stand-in worlds natively and prints each part's time, the record's size and its\n\
    counts. NODES defaults to 1000000. --simplify M overrides refine_simplify_m (metres);\n\
    --pond-density overrides pond_density_area_m2 (m^2), --pond-radius pond_search_radius_m\n\
    (metres) and --pond-cell pond_cell_m (metres).";

fn main() {
    let mut node_counts: Vec<u32> = Vec::new();
    let mut overrides = Overrides::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let positive = |slot: &mut Option<f64>, value: Option<String>, flag: &str| {
            match value.and_then(|v| v.parse::<f64>().ok()) {
                Some(v) if v.is_finite() && v > 0.0 => *slot = Some(v),
                _ => {
                    eprintln!("{flag} needs a positive number\n{USAGE}");
                    std::process::exit(2);
                }
            }
        };
        if arg == "--help" || arg == "-h" {
            println!("{USAGE}");
            return;
        } else if arg == "--simplify" {
            positive(&mut overrides.simplify_m, args.next(), "--simplify");
        } else if arg == "--pond-density" {
            positive(&mut overrides.pond_density_area_m2, args.next(), "--pond-density");
        } else if arg == "--pond-radius" {
            positive(&mut overrides.pond_search_radius_m, args.next(), "--pond-radius");
        } else if arg == "--pond-cell" {
            positive(&mut overrides.pond_cell_m, args.next(), "--pond-cell");
        } else {
            match arg.parse::<u32>() {
                Ok(n) => node_counts.push(n),
                Err(_) => {
                    eprintln!("not a node count: {arg}\n{USAGE}");
                    std::process::exit(2);
                }
            }
        }
    }
    if node_counts.is_empty() {
        node_counts.push(DEFAULT_NODES);
    }

    println!("hydro_survey: native bake survey (plan 1b-2, Task 8)");
    println!("worlds: plain (20260904, 6.371 Mm, 12 plates, 0.29 land, no tectonics)");
    println!("        owner_survey (562423712, 4.5 Mm, 28 plates, 0.16 land, TectonicParams::ranges())");
    println!("        seed1_ranges (1, 6.371 Mm, 12 plates, 0.40 land, TectonicParams::ranges())");
    let mut shown = HydroParams::earth_like(DEFAULT_NODES);
    overrides.apply(&mut shown);
    println!("node counts: {node_counts:?}, each with HydroParams::earth_like(n)");
    println!(
        "params in force: refine_simplify_m {} m  pond_density_area_m2 {:.3e} m^2  \
         pond_search_radius_m {} m  pond_cell_m {} m",
        shown.refine_simplify_m,
        shown.pond_density_area_m2,
        shown.pond_search_radius_m,
        shown.pond_cell_m,
    );

    for world in worlds() {
        println!();
        println!("== {} ==", world.name);
        for &nodes in &node_counts {
            let t = Instant::now();
            let outcome = run(&world.surface, nodes, overrides);
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
