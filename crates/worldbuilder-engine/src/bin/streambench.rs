//! What a stream graph actually costs, at sizes this machine can reach.
//!
//! **Nothing in slice 1p had been run at planet scale before this.** Task 4 measured the
//! sampler's geometry but never its clock and never `node_neighbours` at 20,000,000; Task 5
//! computed its file sizes as arithmetic on the layout and labelled them as such. The
//! extraction's 1.45 GB and 2.16 GB came from a standalone crate, not from this code. So
//! this exists, and it is a `[[bin]]` rather than an `#[ignore]`d test for one reason: a
//! test harness holds every fixture of every test in the same process, and the question
//! here is peak resident memory for **one** node count.
//!
//! ```text
//! cargo run --release --no-default-features --bin streambench -- <node-count> [stage]
//! ```
//!
//! `stage` is `sample`, `neighbours`, `areas`, `heights`, `build` or `all` (the default),
//! and it exists so that the size at which a stage stops fitting can be found without
//! paying for the stages after it.
//!
//! Every byte figure printed is `size_of` times a length -- the containers' own heap, not
//! an allocator's accounting -- and the peak RSS the caller measures around this process is
//! the number that includes everything those figures leave out.

use std::time::Instant;

use worldbuilder_engine::sphere::{SpherePoint, EARTH_RADIUS_M};
use worldbuilder_engine::stream::{
    node_areas_m2, node_neighbours, node_positions, BuildParams, SamplingKind, StreamGraph,
    NEIGHBOUR_COUNT,
};
use worldbuilder_engine::streamfmt::{encoded_len, prefix_len, write_graph, REGION_BYTES_PER_NODE};
use worldbuilder_engine::surface::Surface;

const SEED: i64 = 20_260_904;
const DATUM_M: f64 = 0.0;
const POND_MAX_M2: f64 = 5.0e9;

fn mib(bytes: u64) -> f64 {
    // cast-ok: a byte count to f64 for a printed magnitude, not a lattice decision
    (bytes as f64) / (1024.0 * 1024.0)
}

fn report(label: &str, seconds: f64, bytes: u64) {
    println!("  {label:<26} {seconds:>9.3} s   {:>12} B  {:>9.1} MiB", bytes, mib(bytes));
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let count: u32 = args
        .get(1)
        .and_then(|a| a.parse().ok())
        .expect("usage: streambench <node-count> [sample|neighbours|areas|heights|build|all]");
    let stage = args.get(2).map(String::as_str).unwrap_or("all");
    let world_seed = SEED as u64; // cast-ok: two's-complement reinterpretation, as Surface::new makes
    let point_bytes = u64_of(std::mem::size_of::<SpherePoint>());
    let vec_bytes = u64_of(std::mem::size_of::<Vec<u32>>());

    println!("streambench: n = {count}, stage = {stage}, seed {SEED}, radius {EARTH_RADIUS_M} m");
    println!("  SpherePoint = {point_bytes} B, Vec<u32> header = {vec_bytes} B, k = {NEIGHBOUR_COUNT}");

    let t = Instant::now();
    let positions = node_positions(world_seed, count);
    let positions_bytes = u64::from(count) * point_bytes;
    report("node_positions", t.elapsed().as_secs_f64(), positions_bytes);
    if stage == "sample" {
        return;
    }

    let t = Instant::now();
    let neighbours = node_neighbours(&positions, NEIGHBOUR_COUNT);
    let inner: u64 = u64::from(count) * u64_of(NEIGHBOUR_COUNT) * 4;
    let outer = u64::from(count) * vec_bytes;
    report("node_neighbours", t.elapsed().as_secs_f64(), outer + inner);
    println!("    of which {outer} B is Vec headers and {inner} B is the indices themselves");
    println!(
        "    a flat Vec<u32> of the same k would be {inner} B ({:.1} MiB), a {:.2}x saving",
        mib(inner),
        // cast-ok: two byte counts to f64 for a printed ratio
        ((outer + inner) as f64) / (inner as f64)
    );
    if stage == "neighbours" {
        return;
    }

    let t = Instant::now();
    let areas = node_areas_m2(&positions, &neighbours, EARTH_RADIUS_M);
    report("node_areas_m2", t.elapsed().as_secs_f64(), u64::from(count) * 8);
    if stage == "areas" {
        return;
    }

    let t = Instant::now();
    let surface = Surface::new(SEED, EARTH_RADIUS_M, 22, 0.29, None);
    println!("  Surface::new                {:>9.3} s", t.elapsed().as_secs_f64());
    let t = Instant::now();
    let mut heights: Vec<f64> = Vec::with_capacity(positions.len());
    for point in &positions {
        heights.push(surface.elevation_m(point, None));
    }
    let elapsed = t.elapsed().as_secs_f64();
    report("Surface::elevation_m x n", elapsed, u64::from(count) * 8);
    // cast-ok: a node count to f64 for a printed per-call rate
    println!("    {:.3} us per node", elapsed * 1.0e6 / f64::from(count));
    if stage == "heights" {
        return;
    }

    let params = BuildParams {
        world_seed,
        radius_m: EARTH_RADIUS_M,
        sea_level_m: DATUM_M,
        sampling_kind: SamplingKind::Spiral,
        pond_max_drainage_area_m2: POND_MAX_M2,
    };
    let t = Instant::now();
    let graph = StreamGraph::build(&params, &positions, &heights, &areas, &neighbours)
        .expect("a sampled planet builds");
    // height + area + downhill + drainage + flags, held in memory
    let graph_bytes = u64::from(count) * (8 + 8 + 4 + 8 + 1);
    report("StreamGraph::build", t.elapsed().as_secs_f64(), graph_bytes);

    let roots = graph.roots().len();
    let lakes = graph.lakes().len();
    let mouths = graph.mouth_count();
    let peel = graph.peel();
    // cast-ok: two node counts to f64 for a printed percentage
    let pct = 100.0 * (roots as f64) / f64::from(count);
    println!("    peeled {} of {count}; roots {roots} ({pct:.3}% of nodes) = {mouths} mouths + {lakes} lakes", peel.peeled);

    let t = Instant::now();
    let bytes = write_graph(&graph);
    let written = u64_of(bytes.len());
    report("write_graph", t.elapsed().as_secs_f64(), written);
    let predicted = encoded_len(count, u64_of(lakes), 0);
    println!("    encoded_len predicted {predicted} B; wrote {written} B; agree = {}", predicted == written);
    println!(
        "    prefix {} B + {} B/node x {count} + {} B of lake records",
        prefix_len(),
        REGION_BYTES_PER_NODE,
        u64_of(lakes) * 24
    );
    let region = 100_000u64.min(u64::from(count)) * REGION_BYTES_PER_NODE;
    println!(
        "    a {}-node region is {region} B ({:.2} MiB) in five range requests",
        100_000u64.min(u64::from(count)),
        mib(region)
    );
}

fn u64_of(x: usize) -> u64 {
    x as u64 // cast-ok: a container length; usize is at most 64 bits on every target
}
