//! Dump a native corpus for the native-against-WASM parity harness.
//!
//! Every value crosses the *shipped* `extern "C"` surface -- `wb_world_new`,
//! `wb_elevation_m`, `wb_structural_m`, `wb_bottom_at`, `wb_fill_tile_f32` -- never an
//! internal function, because the claim under test is about what the browser calls.
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

    println!("version {}", wb_generator_version());
}

// Deliberate breakage, prove/python-counts-and-widened-fingerprint: one comment line
// the shipped artifact never saw. Before 2026-09-04 this file was outside the fingerprint
// and `npm run check:wasm` stayed green with it edited.
