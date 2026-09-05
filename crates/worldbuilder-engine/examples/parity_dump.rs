//! Dump a native corpus for the native-against-WASM parity harness.
//!
//! Every value crosses the *shipped* `extern "C"` surface -- `wb_world_new`,
//! `wb_elevation_m`, `wb_structural_m`, `wb_bottom_at`, `wb_fill_tile_f32`,
//! `wb_erosion_run` -- never an internal function, because the claim under test is about
//! what the browser calls. `wb_erosion_run` (Task 5) is the only one of these that reaches
//! `erosion.rs`: before it existed, that module's own claim of native/WASM agreement was
//! unfalsifiable, because nothing in the WASM export surface touched `StreamGraph` or
//! erosion at all.
//!
//! The output is the corpus *and* its answers: every f64 is written as its 16-hex-digit
//! bit pattern, so the replaying side parses no decimal text and the comparison is exact.
//! `parity/parity.mjs` reads this file, replays the identical inputs through the committed
//! `.wasm`, and compares bit patterns. The corpus is therefore defined once, here, and
//! cannot drift between the two sides.
//!
//! Run: `cargo run --release --example parity_dump --features wasm > native.txt`

use worldbuilder_engine::wasm::*;

const SEED: i64 = 20_260_904;
const RADIUS_M: f64 = 6_371_000.0;
const PLATES: u32 = 12;
const LAND: f64 = 0.29;
const RES_M: f64 = 250.0;
const HARBOUR_LAT: f64 = -18.25;
const HARBOUR_LON: f64 = 121.5;

/// The extraction's harbour, as this module's flat f64 records.
fn harbour_records() -> Vec<f64> {
    vec![
        HARBOUR_LAT, HARBOUR_LON, -12.0, 900.0, 260.0, 35.0, WB_COMPOSE_CARVE, WB_SUBSTRATE_DERIVE,
        HARBOUR_LAT, HARBOUR_LON, 4.0, 200.0, 60.0, 35.0, WB_COMPOSE_RAISE, WB_SUBSTRATE_DERIVE,
    ]
}

/// SplitMix64. The scatter has to be reproducible for the dump to be re-derivable, but the
/// replaying side never runs it -- it reads the points back out of the file.
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// A float in [0, 1), from 53 bits.
    fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    }
}

fn hex(value: f64) -> String {
    format!("{:016x}", value.to_bits())
}

fn hex32(value: f32) -> String {
    format!("{:08x}", value.to_bits())
}

fn main() {
    // Two worlds: open water, and the placed harbour. A scattered corpus never lands
    // inside a feature, and that gap has survived every earlier probe in this project.
    println!("world plain {SEED} {} {PLATES} {}", hex(RADIUS_M), hex(LAND));
    let records = harbour_records();
    let encoded: Vec<String> = records.iter().map(|v| hex(*v)).collect();
    println!("world harbour {SEED} {} {PLATES} {} {}", hex(RADIUS_M), hex(LAND), encoded.join(" "));

    let plain = wb_world_new(SEED, RADIUS_M, PLATES, LAND, core::ptr::null(), 0);
    let harbour = wb_world_new(SEED, RADIUS_M, PLATES, LAND, records.as_ptr(), 2);
    assert!(plain != 0 && harbour != 0, "both worlds must build");

    // --- scattered, open water: 10,000 points, elevation and structural each
    let mut rng = Rng(0x5EED_2B_0000_0001);
    for _ in 0..10_000 {
        let latitude_deg = rng.unit() * 180.0 - 90.0;
        let longitude_deg = rng.unit() * 360.0 - 180.0;
        println!(
            "E plain {} {} {} {}",
            hex(latitude_deg),
            hex(longitude_deg),
            hex(RES_M),
            hex(wb_elevation_m(plain, latitude_deg, longitude_deg, RES_M))
        );
        println!(
            "S plain {} {} {}",
            hex(latitude_deg),
            hex(longitude_deg),
            hex(wb_structural_m(plain, latitude_deg, longitude_deg))
        );
    }

    // --- inside the placed harbour: 10,000 points within +/-0.01 deg of it
    for _ in 0..10_000 {
        let latitude_deg = HARBOUR_LAT + (rng.unit() - 0.5) * 0.02;
        let longitude_deg = HARBOUR_LON + (rng.unit() - 0.5) * 0.02;
        println!(
            "E harbour {} {} {} {}",
            hex(latitude_deg),
            hex(longitude_deg),
            hex(RES_M),
            hex(wb_elevation_m(harbour, latitude_deg, longitude_deg, RES_M))
        );
        println!(
            "S harbour {} {} {}",
            hex(latitude_deg),
            hex(longitude_deg),
            hex(wb_structural_m(harbour, latitude_deg, longitude_deg))
        );
    }

    // --- the resolution sentinel, across the boundary: 200 harbour points x 4 sentinels
    for _ in 0..200 {
        let latitude_deg = HARBOUR_LAT + (rng.unit() - 0.5) * 0.02;
        let longitude_deg = HARBOUR_LON + (rng.unit() - 0.5) * 0.02;
        for sentinel in [0.0f64, -1.0, f64::INFINITY, f64::NAN] {
            println!(
                "E harbour {} {} {} {}",
                hex(latitude_deg),
                hex(longitude_deg),
                hex(sentinel),
                hex(wb_elevation_m(harbour, latitude_deg, longitude_deg, sentinel))
            );
        }
    }

    // --- the inspection tap: 500 points in each world, three fractions each
    for (name, handle, lat_c, lon_c, span) in
        [("plain", plain, 12.0, 34.0, 4.0), ("harbour", harbour, HARBOUR_LAT, HARBOUR_LON, 0.02)]
    {
        for _ in 0..500 {
            let latitude_deg = lat_c + (rng.unit() - 0.5) * span;
            let longitude_deg = lon_c + (rng.unit() - 0.5) * span;
            let mut out = [0.0f64; 3];
            let status = wb_bottom_at(handle, latitude_deg, longitude_deg, out.as_mut_ptr());
            println!(
                "B {name} {} {} {status} {} {} {}",
                hex(latitude_deg),
                hex(longitude_deg),
                hex(out[0]),
                hex(out[1]),
                hex(out[2])
            );
        }
    }

    // --- tiles, because a scalar corpus cannot see the grid: 65x65 in each world
    for (name, handle) in [("plain", plain), ("harbour", harbour)] {
        let (lat0, lat1) = (HARBOUR_LAT + 0.005, HARBOUR_LAT - 0.005);
        let (lon0, lon1) = (HARBOUR_LON - 0.005, HARBOUR_LON + 0.005);
        let (width, height) = (65u32, 65u32);
        let mut tile = vec![0.0f32; 65 * 65];
        let status = wb_fill_tile_f32(
            handle,
            lat0,
            lat1,
            lon0,
            lon1,
            width,
            height,
            RES_M,
            tile.as_mut_ptr(),
            width * height,
        );
        assert_eq!(status, WB_OK, "{name}: the tile must fill");
        let cells: Vec<String> = tile.iter().map(|v| hex32(*v)).collect();
        println!(
            "T {name} {} {} {} {} {width} {height} {} {}",
            hex(lat0),
            hex(lat1),
            hex(lon0),
            hex(lon1),
            hex(RES_M),
            cells.join(" ")
        );
    }

    // --- erosion: one capped bake over a real graph, through wb_erosion_run -----------
    //
    // Task 5's corpus. `erosion.rs`'s module doc claims native and WASM agree bit-for-bit;
    // before this export existed nothing could check that (erosion was unreachable from
    // any WASM export). 3,000 nodes exercises every arithmetic path `erode_step` has --
    // sqrt, atan2 via SpherePoint::distance_to, the implicit receiver update -- the same
    // way a 20,000,000-node planetary bake would, because bit-equality does not depend on
    // size; only the planetary bake's *memory footprint* does (slice 1p: 1.45 GB of arrays,
    // does not fit a 32-bit wasm heap), which is not what this corpus is testing.
    //
    // `EROSION_THRESHOLD_M` is deliberately far tighter than this graph reaches in
    // `EROSION_MAX_ITERATIONS` steps at these constants (c ~ 1.0e-3, see
    // `erosion.rs::erode_step`'s doc): the run is designed to hit the iteration cap on
    // every invocation, native and WASM alike, so the number of `erode_step` calls is fixed
    // by construction rather than a side effect of whichever constant a mutation touches.
    // The two `assert_eq!`s below hold that design to its own claim -- if either ever
    // fires, the corpus's "same step count regardless of perturbation" property (which
    // `parity.mjs --mutate erosion-k` depends on to isolate arithmetic divergence from
    // step-count divergence) no longer holds and the control's own doc is wrong.
    const EROSION_NODES: u32 = 3_000;
    const EROSION_UPLIFT_M_PER_YR: f64 = 1.0e-3;
    const EROSION_ERODIBILITY_PER_YR: f64 = 1.0e-6;
    const EROSION_TIMESTEP_YR: f64 = 1000.0;
    const EROSION_THRESHOLD_M: f64 = 1.0e-9;
    const EROSION_MAX_ITERATIONS: u32 = 20;

    let mut erosion_heights = vec![0.0f64; EROSION_NODES as usize];
    let mut erosion_iterations: u32 = 0;
    let mut erosion_converged: u32 = 0;
    let erosion_status = wb_erosion_run(
        plain,
        EROSION_NODES,
        EROSION_UPLIFT_M_PER_YR,
        EROSION_ERODIBILITY_PER_YR,
        EROSION_TIMESTEP_YR,
        EROSION_THRESHOLD_M,
        EROSION_MAX_ITERATIONS,
        erosion_heights.as_mut_ptr(),
        EROSION_NODES,
        &mut erosion_iterations,
        &mut erosion_converged,
    );
    assert_eq!(erosion_status, WB_OK, "the erosion run must succeed for the parity corpus");
    assert_eq!(
        erosion_iterations, EROSION_MAX_ITERATIONS,
        "the corpus is designed to hit the iteration cap on every run, not converge early -- \
         a different count here means EROSION_THRESHOLD_M is no longer tight enough for this \
         claim, and parity.mjs's erosion-k control can no longer assume a fixed step count"
    );
    assert_eq!(erosion_converged, 0, "see erosion_iterations above");
    let erosion_hex: Vec<String> = erosion_heights.iter().map(|v| hex(*v)).collect();
    println!(
        "R erosion {SEED} {} {PLATES} {} {EROSION_NODES} {} {} {} {} {EROSION_MAX_ITERATIONS} {erosion_status} {erosion_iterations} {erosion_converged} {}",
        hex(RADIUS_M),
        hex(LAND),
        hex(EROSION_UPLIFT_M_PER_YR),
        hex(EROSION_ERODIBILITY_PER_YR),
        hex(EROSION_TIMESTEP_YR),
        hex(EROSION_THRESHOLD_M),
        erosion_hex.join(" ")
    );

    println!("version {}", wb_generator_version());
}
