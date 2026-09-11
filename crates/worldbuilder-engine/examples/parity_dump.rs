//! Dump a native corpus for the native-against-WASM parity harness.
//!
//! Every value crosses the *shipped* `extern "C"` surface -- `wb_world_new`,
//! `wb_world_new_relief`, `wb_world_new_tectonic`, `wb_relief_preset`, `wb_tectonic_preset`,
//! `wb_tectonic_check`, `wb_elevation_m`, `wb_structural_m`, `wb_bottom_at`,
//! `wb_fill_tile_f32`, `wb_erosion_run`, `wb_water_run` -- never an internal function,
//! because the claim under test is about what the browser calls.
//!
//! **Two of those exist so that a module could be compared at all.** `wb_erosion_run` (slice
//! 5a Task 5) is the only door into `erosion.rs`, and `wb_water_run` (slice 5b Task 5) is the
//! only door into `water.rs`: before each existed, that module's own claim of native/WASM
//! agreement was *unfalsifiable* -- not unverified -- because nothing in the export surface
//! touched it. The relief entries are a different shape of gap: the surface *did* reach
//! `detail.rs`'s relief block after the relief slice's Task 4, and nothing here had ever sent
//! one, because every world above is built through `wb_world_new`, which sends `None`.
//!
//! **The tectonic entries are a third shape of gap: three tasks flagged it and none owned
//! it.** Mountains Task 4, Task 3 and Task 5 each reported, correctly, that no value in this
//! corpus went through a tectonic export -- while `TectonicParams::ranges()` is a preset the
//! owner presses on the panel and drives seven of sixteen words across the boundary. Native
//! and WASM are the same Rust over the same pure-Rust `libm`, so the comparison is *strict
//! bit-for-bit* even with transcendentals in the path; what is boundary-only is the DECODE,
//! and nothing exercised it.
//!
//! **Two derivations here are NOT compared values and are labelled as such:** the `WCTL`
//! record carries the water control's predicted divergence, computed from
//! `water::lake_body_surface_areas_m2` rather than from the classifier the control perturbs,
//! and cross-checked against that classifier before it is written; and the `TCTL` record
//! carries the tectonic control's predicted divergence per group, computed through the
//! exports *and* through the library's own `Surface` with blocks read from `tectonics.rs`
//! rather than from the words that crossed the boundary, and cross-checked between the two
//! before it is written. A control gate read off the control's own run is a rubber stamp;
//! both of these are predictions the replaying side has to meet.
//!
//! The output is the corpus *and* its answers: every f64 is written as its 16-hex-digit
//! bit pattern, so the replaying side parses no decimal text and the comparison is exact.
//! `parity/parity.mjs` reads this file, replays the identical inputs through the committed
//! `.wasm`, and compares bit patterns. The corpus is therefore defined once, here, and
//! cannot drift between the two sides.
//!
//! Run: `cargo run --release --example parity_dump --features wasm > native.txt`

use worldbuilder_engine::continentality::CoastParams;
use worldbuilder_engine::detail::GullyParams;
use worldbuilder_engine::hydrology;
use worldbuilder_engine::sphere::SpherePoint;
use worldbuilder_engine::stream::{sample_nodes, BuildParams, SamplingKind, StreamGraph};
use worldbuilder_engine::surface::Surface;
use worldbuilder_engine::tectonics::TectonicParams;
use worldbuilder_engine::wasm::*;
use worldbuilder_engine::water;

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

/// One hydro bake through the shipped exports, native side. Returns the status and, on
/// `WB_OK`, the full word vector; on any other status the word vector is empty and the length
/// is 0, since `out_id` was never written and there is nothing to copy.
fn bake_hydro_native(world: u32, params: &[f64]) -> (u32, u32, Vec<f64>) {
    let mut id: u32 = 0;
    let status = wb_hydro_bake(world, params.as_ptr(), params.len() as u32, &mut id); // cast-ok: params is a small, compile-time-bounded local buffer
    if status != WB_OK {
        return (status, 0, Vec::new());
    }
    let len = wb_hydro_len(id);
    let mut words = vec![0.0f64; len as usize]; // cast-ok: a freshly measured record length used to size its own buffer
    assert_eq!(wb_hydro_copy(id, words.as_mut_ptr(), len), WB_OK);
    assert_eq!(wb_hydro_free(id), WB_OK);
    (status, len, words)
}

/// I6 (final review ruling, rule (a)): the divergence between a recorded hydro record
/// (`status_on`/`len`/`words_on`) and a freshly measured one under a control
/// (`status_off`/`n`/`words_off`). One tally for the status, one for the length equality, and
/// then bit-for-bit words `i < min(n, len)`; a recorded word at `i >= n` counts as divergent
/// without reading anything at that index, since `words_off` never held that many words in the
/// first place -- this is the native prediction the `--mutate tectonic-warp` control replays.
fn divergent_count(
    status_on: u32,
    len: u32,
    words_on: &[f64],
    status_off: u32,
    n: u32,
    words_off: &[f64],
) -> usize {
    let mut divergent = 0usize;
    if status_on != status_off {
        divergent += 1;
    }
    if len != n {
        divergent += 1;
    }
    let len = len as usize; // cast-ok: a hydro record length, already used to size a Vec above
    let n = n as usize; // cast-ok: as above
    for i in 0..len {
        if i < n {
            if words_on[i].to_bits() != words_off[i].to_bits() {
                divergent += 1;
            }
        } else {
            divergent += 1;
        }
    }
    divergent
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

    // --- climate: the two new doors onto climate.rs -----------------------------------
    //
    // Slice `2026-09-06-slice-climate`, Task 4. `wb_climate_tile_f32` and
    // `wb_climate_calibration` are the only exports that reach `climate.rs`, so before they
    // existed that module's native/WASM agreement was **unfalsifiable** in exactly the sense
    // this file's own module doc gives for `wb_erosion_run` and `wb_water_run`.
    //
    // What is genuinely new in the arithmetic, rather than more of what is already compared:
    // the march is an accumulation over up to `march_samples` `elevation_m` probes taken
    // along a `TangentFrame`, so it compounds `detmath::exp` (never previously crossed at
    // all) over a path -- a place where a single-ULP disagreement would grow rather than
    // stay put. The calibration adds a **sort and an order statistic over a 4,000-point
    // Fibonacci spiral**, whose answer depends on the ordering of values that a one-ULP
    // difference could swap.
    //
    // **The control is `--mutate climate-samples`, and it is a control with a shape rather
    // than a size**: one more upwind step changes the rain-out integral and cannot change a
    // closed-form temperature or a quantile of elevation. So it must move every
    // `climate-moist/*` group and leave every `climate-temp/*` and `climate-land/*` group at
    // exactly zero. A control that moved all three would be indistinguishable from a
    // harness bug; that prediction is asserted in `parity.mjs`, not observed.
    //
    // The budgets here are deliberately small (0, 8, 24) except for one canonical-width
    // 16x16 tile: the cost is samples x budget and the replaying side runs in WASM at ~3x
    // native. 16x16 is also the raster the viewer ships, so the compared grid is the grid
    // that is drawn.
    for (name, handle) in [("plain", plain), ("harbour", harbour)] {
        // **The window is over a continent, and that is a finding rather than a preference.**
        // The obvious choice -- the harbour, which every other section of this corpus uses --
        // produced 640 f32 all equal to `3f800000`: the whole 3,200 km upwind path is open
        // water there, so the march recharges to exactly 1.0 and STAYS there whatever the
        // budget is. The first run of `--mutate climate-samples` therefore moved 8 values of
        // 648, all of them calibration edges, and the tile records compared a constant to
        // itself. That is this project's "an assertion that looked load-bearing and was not"
        // arriving in a parity corpus.
        //
        // 6 N to 10 S, 40 E to 56 E is this world's largest dry interior: a global 72 x 36
        // scan through this same export puts its driest land cells at moisture 0.009 against
        // 1.0 offshore, so the window spans nearly the whole range the field has. The
        // assertions below hold the corpus to that rather than trusting this comment.
        let (lat0, lat1) = (6.0, -10.0);
        let (lon0, lon1) = (40.0, 56.0);
        // 16x16 at the shipped budget, then 8x8 at zero -- the identity march, whose answer
        // is exactly 1.0 and which is therefore the one cell of this corpus that a wrong
        // `exp` could not move. It is here so the control has something to be measured
        // against that is NOT sensitive to the same term.
        for (width, height, samples) in [(16u32, 16u32, 160u32), (8, 8, 0)] {
            let values = (width * height) as usize * WB_CLIMATE_STRIDE; // cast-ok: a compile-time grid back to a length
            let mut tile = vec![0.0f32; values];
            let status = wb_climate_tile_f32(
                handle,
                lat0,
                lat1,
                lon0,
                lon1,
                width,
                height,
                RES_M,
                samples,
                tile.as_mut_ptr(),
                values as u32, // cast-ok: a compile-time length back to the ABI's u32
            );
            assert_eq!(status, WB_OK, "{name}: the climate tile must fill");
            // **A corpus of constants compares nothing.** The moisture channel of the
            // canonical-budget tile must actually vary, and it must reach well below
            // saturation, or `--mutate climate-samples` has nothing to move and the parity
            // comparison is a constant against itself. Asserted here, in the generator, so
            // the corpus cannot quietly become uninformative again.
            if samples > 0 {
                let mut distinct: Vec<u32> = tile.iter().skip(1).step_by(2).map(|v| v.to_bits()).collect();
                distinct.sort_unstable();
                distinct.dedup();
                let driest = tile.iter().skip(1).step_by(2).fold(f32::INFINITY, |a, b| if *b < a { *b } else { a });
                assert!(
                    distinct.len() > 100,
                    "{name}: only {} distinct moisture values in the climate tile",
                    distinct.len(),
                );
                assert!(driest < 0.5, "{name}: the driest cell is {driest}; this window is all sea");
            }
            let cells: Vec<String> = tile.iter().map(|v| hex32(*v)).collect();
            println!(
                "CL {name} {} {} {} {} {width} {height} {} {samples} {}",
                hex(lat0),
                hex(lat1),
                hex(lon0),
                hex(lon1),
                hex(RES_M),
                cells.join(" ")
            );
        }
        // The calibration, at a budget small enough to replay in WASM: 4,000 elevations plus
        // one 8-step march at every land point.
        let mut edges = [0.0f64; WB_CLIMATE_CALIBRATION_STRIDE];
        let status = wb_climate_calibration(
            handle,
            RES_M,
            8,
            edges.as_mut_ptr(),
            WB_CLIMATE_CALIBRATION_STRIDE as u32, // cast-ok: a compile-time stride back to the ABI's u32
        );
        assert_eq!(status, WB_OK, "{name}: the calibration must answer");
        assert!(edges[6] > 100.0, "{name}: this corpus needs a world with real land");
        let encoded: Vec<String> = edges.iter().map(|v| hex(*v)).collect();
        println!("CK {name} {} 8 {}", hex(RES_M), encoded.join(" "));
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


    // --- the relief channel: the presets themselves, then a world built from one ---------
    //
    // Relief Task 4 changed the export surface for the first time in that slice and flagged,
    // correctly, that parity had not been re-run against it. The relief block travels to the
    // tile workers, so a native-versus-WASM difference in how it is *decoded* is exactly the
    // class of bug this harness exists to catch, and nothing in the corpus above could have
    // seen one: every world here was built through `wb_world_new`, which sends `None`.
    //
    // The preset's ten fields are compared first, and then used. **They are never retyped**
    // -- `wb_relief_preset` is the only place they come from, on both sides, which is that
    // export's whole reason for existing (its own doc: "so no host ever transcribes a
    // preset"). The replaying side reads them back out of this file rather than calling its
    // own `wb_relief_preset`... and then also calls it, because the comparison of the two is
    // itself a parity claim about the newest export in the surface.
    let mut hills = [0.0f64; WB_RELIEF_STRIDE];
    let mut canonical = [0.0f64; WB_RELIEF_STRIDE];
    let hills_status = wb_relief_preset(WB_RELIEF_HILLS, hills.as_mut_ptr(), WB_RELIEF_STRIDE as u32); // cast-ok: a compile-time stride into the export's u32 length
    let canonical_status =
        wb_relief_preset(WB_RELIEF_CANONICAL, canonical.as_mut_ptr(), WB_RELIEF_STRIDE as u32); // cast-ok: a compile-time stride into the export's u32 length
    assert_eq!(hills_status, WB_OK, "the hills preset must be readable");
    assert_eq!(canonical_status, WB_OK, "the canonical preset must be readable");
    for (selector, status, record) in [
        (WB_RELIEF_CANONICAL, canonical_status, &canonical),
        (WB_RELIEF_HILLS, hills_status, &hills),
    ] {
        let encoded: Vec<String> = record.iter().map(|v| hex(*v)).collect();
        println!("P {selector} {status} {}", encoded.join(" "));
    }

    // A world carrying a NON-canonical relief block. `hills()` is the obvious choice and the
    // brief named it: it is the one preset the viewer's panel can reach with a button, so it
    // is the block most likely to be in flight when a decode differs.
    let relief_encoded: Vec<String> = hills.iter().map(|v| hex(*v)).collect();
    println!(
        "worldr hills {SEED} {} {PLATES} {} {}",
        hex(RADIUS_M),
        hex(LAND),
        relief_encoded.join(" ")
    );
    let hills_world = wb_world_new_relief(
        SEED,
        RADIUS_M,
        PLATES,
        LAND,
        core::ptr::null(),
        0,
        hills.as_ptr(),
        WB_RELIEF_STRIDE as u32, // cast-ok: a compile-time stride into the export's u32 length
    );
    assert!(hills_world != 0, "the hills world must build");

    // 5,000 scattered points on the hills world, elevation and structural each. Structural is
    // included deliberately even though relief cannot move it: a relief block that leaked
    // into the tectonic path would show up here and nowhere else, and an entry that can only
    // agree is still evidence about which paths the block does not reach.
    for _ in 0..5_000 {
        let latitude_deg = rng.unit() * 180.0 - 90.0;
        let longitude_deg = rng.unit() * 360.0 - 180.0;
        println!(
            "E hills {} {} {} {}",
            hex(latitude_deg),
            hex(longitude_deg),
            hex(RES_M),
            hex(wb_elevation_m(hills_world, latitude_deg, longitude_deg, RES_M))
        );
        println!(
            "S hills {} {} {}",
            hex(latitude_deg),
            hex(longitude_deg),
            hex(wb_structural_m(hills_world, latitude_deg, longitude_deg))
        );
    }

    // And a tile, because the tile worker is where the relief block actually lands: the
    // viewer attaches it to the spec before `TilePool.start`, so every worker builds its own
    // world from it. A scalar corpus cannot see the grid, and the grid is the consumer.
    {
        let (lat0, lat1) = (HARBOUR_LAT + 0.005, HARBOUR_LAT - 0.005);
        let (lon0, lon1) = (HARBOUR_LON - 0.005, HARBOUR_LON + 0.005);
        let (width, height) = (65u32, 65u32);
        let mut tile = vec![0.0f32; 65 * 65];
        let status = wb_fill_tile_f32(
            hills_world,
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
        assert_eq!(status, WB_OK, "hills: the tile must fill");
        let cells: Vec<String> = tile.iter().map(|v| hex32(*v)).collect();
        println!(
            "T hills {} {} {} {} {width} {height} {} {}",
            hex(lat0),
            hex(lat1),
            hex(lon0),
            hex(lon1),
            hex(RES_M),
            cells.join(" ")
        );
    }

    // --- water: the shipped manifest, through the shipped export ------------------------
    //
    // Slice 5b Task 5's own corpus. `water.rs` was unreachable from the export surface until
    // `wb_water_run` existed, exactly as `erosion.rs` was until `wb_erosion_run` did, so its
    // native/WASM claim was unfalsifiable rather than unverified.
    //
    // What is dumped is the SHIPPED manifest -- `water_manifest_from_graph`'s own rows, after
    // fill, overflow resolution, Ruling 7's tied-plateau merge and classification -- and not
    // an intermediate. The pre-flight conflict scan named that trap for this pair of tasks,
    // and 5a's equivalent was `erode_step` being uncapped outside the loop.
    //
    // `WATER_SEA_LEVEL_M = 0.0` is the datum every other record here is already implicitly
    // at. `WATER_POND_MAX_SURFACE_AREA_M2 = 1.0e5` is Task 3's CALIBRATED threshold, on
    // external ground (a shoreline walkable in about fifteen minutes), and it produces ZERO
    // ponds on this mesh -- the smallest body the generator makes is nearly four orders of
    // magnitude larger. That is a recorded finding about the mesh, and the corpus carries the
    // calibrated value rather than a value chosen to make ponds appear.
    const WATER_NODES: u32 = 30_000;
    const WATER_SEA_LEVEL_M: f64 = 0.0;
    const WATER_POND_MAX_SURFACE_AREA_M2: f64 = 1.0e5;
    // The control's threshold. Chosen off the measured distribution rather than by taste: the
    // 5b Task 3 survey put this mesh's body surface areas over roughly one order of magnitude
    // around a median of ~1.9e10 m^2, so a threshold at 2.0e10 splits the population instead
    // of moving all of it or none of it. `--mutate water-pond` replays the record below at
    // this value and at nothing else.
    const WATER_CONTROL_POND_MAX_SURFACE_AREA_M2: f64 = 2.0e10;

    let mut water_rows = vec![0.0f64; WATER_NODES as usize * WB_WATER_BODY_STRIDE];
    let water_capacity = water_rows.len() as u32; // cast-ok: a corpus-fixed buffer length back into the export's u32
    let mut water_body_count: u32 = 0;
    let mut water_sea_level_m: f64 = 0.0;
    let water_status = wb_water_run(
        plain,
        WATER_NODES,
        WATER_SEA_LEVEL_M,
        WATER_POND_MAX_SURFACE_AREA_M2,
        water_rows.as_mut_ptr(),
        water_capacity,
        &mut water_body_count,
        &mut water_sea_level_m,
    );
    assert_eq!(water_status, WB_OK, "the water run must succeed for the parity corpus");
    assert!(water_body_count > 0, "a corpus of zero bodies would compare nothing");
    water_rows.truncate(water_body_count as usize * WB_WATER_BODY_STRIDE);
    assert!(
        water_rows.chunks_exact(WB_WATER_BODY_STRIDE).all(|row| row[1] == WB_BODY_KIND_LAKE),
        "the calibrated threshold produces no ponds on this mesh -- if this ever fires, the \
         corpus has quietly 'fixed' Task 3's recorded finding rather than carrying it",
    );

    // THE CONTROL'S PREDICTION, DERIVED INDEPENDENTLY OF THE CLASSIFIER IT PREDICTS.
    //
    // A control gate that is read off the control's own run is a rubber stamp. So the number
    // below comes from the other side: `water::lake_body_surface_areas_m2` is the summed
    // surface area per physical body, and a body flips to `Pond` exactly when that area is at
    // or below the threshold. Counting the distribution is arithmetic over areas;
    // `classify_lake_kinds` is the thing being predicted. The assertion that the two agree is
    // the cross-check, and it runs here, natively, before the harness ever replays anything.
    let predicted_flips = {
        let sampling = sample_nodes(SEED as u64, WATER_NODES, RADIUS_M) // cast-ok: two's-complement reinterpretation, the same one wb_world_new makes for Noise
            .expect("the corpus node set must sample");
        let surface = Surface::new(SEED, RADIUS_M, PLATES as usize, LAND, None, None, None); // cast-ok: a corpus-fixed plate count widened to usize
        let heights: Vec<f64> =
            sampling.positions.iter().map(|point| surface.elevation_m(point, None)).collect();
        let mut graph = StreamGraph::build(
            &BuildParams {
                world_seed: SEED as u64, // cast-ok: two's-complement reinterpretation, as above
                radius_m: RADIUS_M,
                sea_level_m: WATER_SEA_LEVEL_M,
                sampling_kind: SamplingKind::Spiral,
                pond_max_surface_area_m2: WATER_POND_MAX_SURFACE_AREA_M2,
            },
            &sampling.positions,
            &heights,
            &sampling.area_m2,
            &sampling.neighbours,
        )
        .expect("the corpus graph must build");
        let basins = water::fill_and_resolve_water(&mut graph, WATER_POND_MAX_SURFACE_AREA_M2);
        let areas = water::lake_body_surface_areas_m2(&graph, &basins);
        assert_eq!(
            areas.len(),
            water_body_count as usize,
            "the area distribution and the manifest must describe the same bodies",
        );
        areas.iter().filter(|a| **a <= WATER_CONTROL_POND_MAX_SURFACE_AREA_M2).count()
    };
    assert!(
        predicted_flips > 0 && predicted_flips < water_body_count as usize,
        "the control threshold must split the population -- {predicted_flips} of {water_body_count} \
         is not a control, it is either a no-op or a different corpus",
    );

    // The cross-check: the classifier, run at the control's threshold, must produce exactly
    // the ponds the area distribution predicts. If these two ever disagree, the prediction is
    // wrong and the gate below it is meaningless, and that must fail here rather than be
    // absorbed into a divergent count.
    {
        let mut control_rows = vec![0.0f64; WATER_NODES as usize * WB_WATER_BODY_STRIDE];
        let capacity = control_rows.len() as u32; // cast-ok: as above
        let mut count: u32 = 0;
        let mut datum: f64 = 0.0;
        let status = wb_water_run(
            plain,
            WATER_NODES,
            WATER_SEA_LEVEL_M,
            WATER_CONTROL_POND_MAX_SURFACE_AREA_M2,
            control_rows.as_mut_ptr(),
            capacity,
            &mut count,
            &mut datum,
        );
        assert_eq!(status, WB_OK, "the control's own native run must succeed");
        assert_eq!(count, water_body_count, "the threshold must not move the body count");
        control_rows.truncate(count as usize * WB_WATER_BODY_STRIDE);
        let ponds = control_rows
            .chunks_exact(WB_WATER_BODY_STRIDE)
            .filter(|row| row[1] == WB_BODY_KIND_POND)
            .count();
        assert_eq!(
            ponds, predicted_flips,
            "the classifier and the independently summed surface areas disagree about how many \
             bodies fall under the control threshold",
        );
        // And nothing but `kind` moved, which is what makes this control narrow rather than
        // gross. Asserted here as well as in `tests/wasm_exports.rs` because it is the
        // property the gate's own number depends on.
        for (a, b) in water_rows
            .chunks_exact(WB_WATER_BODY_STRIDE)
            .zip(control_rows.chunks_exact(WB_WATER_BODY_STRIDE))
        {
            for field in [0usize, 2, 3, 4, 5, 6] {
                assert_eq!(
                    a[field].to_bits(),
                    b[field].to_bits(),
                    "field {field} moved with the pond threshold; the control would then be \
                     measuring something other than what it claims",
                );
            }
        }
    }

    println!(
        "WCTL {} {predicted_flips}",
        hex(WATER_CONTROL_POND_MAX_SURFACE_AREA_M2)
    );
    let water_hex: Vec<String> = water_rows.iter().map(|v| hex(*v)).collect();
    println!(
        "W plain {WATER_NODES} {} {} {water_status} {water_body_count} {} {}",
        hex(WATER_SEA_LEVEL_M),
        hex(WATER_POND_MAX_SURFACE_AREA_M2),
        hex(water_sea_level_m),
        water_hex.join(" ")
    );

    // --- the tectonic channel: the presets, the checker, and a world built from one -------
    //
    // Task 4 of the mountains slice flagged this gap, Task 3 flagged it again larger, and
    // Task 5 flagged it a third time two fields larger still. **None of them owned it.** The
    // corpus above compares 71,596 values and not one of them goes through
    // `wb_tectonic_preset`, `wb_tectonic_check` or a tectonic block on
    // `wb_world_new_tectonic` -- while `ranges()` is a preset the owner presses on the panel,
    // and it drives seven of `TectonicParams`' sixteen words across this boundary.
    //
    // The shape is the relief channel's, one row for one row, because the relief channel is
    // the thing this harness already got right: the presets first, field by field; then a
    // world carrying a NON-canonical block; then scalars on it; then a tile, because the tile
    // worker is the block's real consumer in the browser.
    //
    // **Why this is worth doing even though native and WASM are the same Rust.** They are --
    // over the same pure-Rust `libm`, so this comparison is strict bit-for-bit even where
    // transcendentals are in the path, which is a stronger contract than the bounded one
    // Python-versus-Rust conformance holds. What is NOT shared is the *decode*: the block
    // crosses as sixteen f64 in linear memory, is read back through a raw pointer, and is
    // bounds-checked before it becomes a `TectonicParams`. That path exists only on this
    // boundary, and until now nothing exercised it.
    let mut tectonic_canonical = [0.0f64; WB_TECTONIC_STRIDE];
    let mut tectonic_ranges = [0.0f64; WB_TECTONIC_STRIDE];
    let tectonic_canonical_status = wb_tectonic_preset(
        WB_TECTONIC_CANONICAL,
        tectonic_canonical.as_mut_ptr(),
        WB_TECTONIC_STRIDE as u32, // cast-ok: a compile-time stride into the export's u32 length
    );
    let tectonic_ranges_status = wb_tectonic_preset(
        WB_TECTONIC_RANGES,
        tectonic_ranges.as_mut_ptr(),
        WB_TECTONIC_STRIDE as u32, // cast-ok: a compile-time stride into the export's u32 length
    );
    assert_eq!(tectonic_canonical_status, WB_OK, "the canonical tectonic preset must be readable");
    assert_eq!(tectonic_ranges_status, WB_OK, "the ranges tectonic preset must be readable");
    assert_ne!(
        tectonic_ranges, tectonic_canonical,
        "a preset identical to canonical would make every tectonic row below a second copy of \
         the plain world's rows",
    );
    for (selector, status, record) in [
        (WB_TECTONIC_CANONICAL, tectonic_canonical_status, &tectonic_canonical),
        (WB_TECTONIC_RANGES, tectonic_ranges_status, &tectonic_ranges),
    ] {
        let encoded: Vec<String> = record.iter().map(|v| hex(*v)).collect();
        println!("TP {selector} {status} {}", encoded.join(" "));
    }

    // The control's block: `ranges()` with `margin_warp_m` -- word 14, `encode_tectonic`'s
    // own order -- turned off, and nothing else touched. Built from the preset the export
    // just handed back, so the fifteen fields it keeps are not retyped either.
    const TECTONIC_WARP_INDEX: usize = 14;
    const TECTONIC_CONTROL_WARP_M: f64 = 0.0;
    let mut tectonic_control = tectonic_ranges;
    tectonic_control[TECTONIC_WARP_INDEX] = TECTONIC_CONTROL_WARP_M;
    assert_ne!(
        tectonic_ranges[TECTONIC_WARP_INDEX], TECTONIC_CONTROL_WARP_M,
        "the control must actually change the field it names -- a preset that already ships \
         the control's value would make the whole control a no-op wearing a control's name",
    );

    // `wb_tectonic_check`, the third tectonic export and the only one that answers *why* a
    // record was refused. Six records, three accepted and three refused, so the group cannot
    // be trivially uniform in either direction: a checker that refused everything and a
    // checker that accepted everything would both pass a corpus of one kind.
    //
    // The refusals are the three the sweep found matter: the saturating `as u32` cast on a
    // loop bound, a `structure_depth` outside `structure_at`'s documented range, and a warp
    // amplitude that carries the collision profile past `MAX_TECTONIC_RANGE_M` on canonical's
    // 400 km flank -- the reach check, which no per-field ceiling can see.
    let mut tectonic_saturating = tectonic_ranges;
    tectonic_saturating[10] = 1.0e300;
    let mut tectonic_deep = tectonic_ranges;
    tectonic_deep[12] = 1.5;
    let mut tectonic_far = tectonic_canonical;
    tectonic_far[TECTONIC_WARP_INDEX] = 400_000.0;
    let check_records = [
        ("canonical", tectonic_canonical),
        ("ranges", tectonic_ranges),
        ("control", tectonic_control),
        ("saturating", tectonic_saturating),
        ("deep", tectonic_deep),
        ("far", tectonic_far),
    ];
    let mut accepted = 0usize;
    let mut refused = 0usize;
    for (name, record) in &check_records {
        let status = wb_tectonic_check(
            record.as_ptr(),
            WB_TECTONIC_STRIDE as u32, // cast-ok: a compile-time stride into the export's u32 length
        );
        if status == WB_OK {
            accepted += 1;
        } else {
            refused += 1;
        }
        let encoded: Vec<String> = record.iter().map(|v| hex(*v)).collect();
        println!("TC {name} {status} {}", encoded.join(" "));
    }
    assert_eq!(accepted, 3, "three of the six check records are meant to be accepted");
    assert_eq!(refused, 3, "three of the six check records are meant to be refused");

    // A world carrying the non-canonical block, twice under two names. **Two names, one
    // configuration, and that is deliberate**: the scattered points and the concentrated
    // ones then tally as separate groups, so the control's report says in its own output
    // that the belt moved and the rest of the planet did not. One mixed group would have
    // hidden exactly that.
    //
    // `TWARP` goes out first because the replaying side needs it *here*, when it builds
    // these worlds; the prediction it belongs to (`TCTL`) cannot be written until the corpus
    // has been sampled, so the control arrives as two records rather than one.
    println!("TWARP {}", hex(TECTONIC_CONTROL_WARP_M));
    let tectonic_encoded: Vec<String> = tectonic_ranges.iter().map(|v| hex(*v)).collect();
    for name in ["ranges", "belt"] {
        println!(
            "worldt {name} {SEED} {} {PLATES} {} {}",
            hex(RADIUS_M),
            hex(LAND),
            tectonic_encoded.join(" ")
        );
    }
    let tectonic_world = wb_world_new_tectonic(
        SEED,
        RADIUS_M,
        PLATES,
        LAND,
        core::ptr::null(),
        0,
        core::ptr::null(),
        0,
        tectonic_ranges.as_ptr(),
        WB_TECTONIC_STRIDE as u32, // cast-ok: a compile-time stride into the export's u32 length
    );
    assert!(tectonic_world != 0, "the ranges world must build");
    let tectonic_control_world = wb_world_new_tectonic(
        SEED,
        RADIUS_M,
        PLATES,
        LAND,
        core::ptr::null(),
        0,
        core::ptr::null(),
        0,
        tectonic_control.as_ptr(),
        WB_TECTONIC_STRIDE as u32, // cast-ok: a compile-time stride into the export's u32 length
    );
    assert!(tectonic_control_world != 0, "the control world must build");

    // WHERE THE BELT IS, AND WHY IT IS NOT A ROUND NUMBER.
    //
    // `src/bin/mountain_probe.rs`'s `witness_between` scans this exact fixture world on a
    // 0.5-degree global grid for the site where turning `margin_warp_m` off moves
    // `elevation_m` the most: **-5.00, 66.00, where the ground reads 821.955 m with the warp
    // off and 2,432.773 m with it on.** A corpus scattered uniformly over a planet does not
    // land on a 100 km belt -- the same reason this file already carries a second world for
    // the placed harbour -- and a control that moves nothing proves nothing.
    const BELT_LAT: f64 = -5.0;
    const BELT_LON: f64 = 66.0;
    const BELT_SPAN_DEG: f64 = 2.0;
    {
        let point_on = wb_elevation_m(tectonic_world, BELT_LAT, BELT_LON, RES_M);
        let point_off = wb_elevation_m(tectonic_control_world, BELT_LAT, BELT_LON, RES_M);
        let moved = if point_on > point_off { point_on - point_off } else { point_off - point_on };
        assert!(
            moved > 1_000.0,
            "the belt site must be somewhere the control's one field actually moves the \
             ground; it moved {moved} m, so either the witness is stale or the field no \
             longer reaches this world",
        );
    }

    // 5,000 scattered points, exactly as the relief world takes them: global, uniform, and
    // mostly nowhere near a convergent continental margin. That is the point -- they are the
    // corpus's evidence that the block does NOT reach the rest of the planet, and under the
    // control they are the group that mostly stays equal.
    let mut scattered_points = Vec::with_capacity(5_000);
    for _ in 0..5_000 {
        let latitude_deg = rng.unit() * 180.0 - 90.0;
        let longitude_deg = rng.unit() * 360.0 - 180.0;
        scattered_points.push((latitude_deg, longitude_deg));
        println!(
            "E ranges {} {} {} {}",
            hex(latitude_deg),
            hex(longitude_deg),
            hex(RES_M),
            hex(wb_elevation_m(tectonic_world, latitude_deg, longitude_deg, RES_M))
        );
        println!(
            "S ranges {} {} {}",
            hex(latitude_deg),
            hex(longitude_deg),
            hex(wb_structural_m(tectonic_world, latitude_deg, longitude_deg))
        );
    }

    // 2,000 points on the belt itself, in a +/-1 degree box on the witness site.
    let mut belt_points = Vec::with_capacity(2_000);
    for _ in 0..2_000 {
        let latitude_deg = BELT_LAT + (rng.unit() - 0.5) * BELT_SPAN_DEG;
        let longitude_deg = BELT_LON + (rng.unit() - 0.5) * BELT_SPAN_DEG;
        belt_points.push((latitude_deg, longitude_deg));
        println!(
            "E belt {} {} {} {}",
            hex(latitude_deg),
            hex(longitude_deg),
            hex(RES_M),
            hex(wb_elevation_m(tectonic_world, latitude_deg, longitude_deg, RES_M))
        );
        println!(
            "S belt {} {} {}",
            hex(latitude_deg),
            hex(longitude_deg),
            hex(wb_structural_m(tectonic_world, latitude_deg, longitude_deg))
        );
    }

    // And a tile across the belt, because the tile worker is where a tectonic block lands in
    // the browser exactly as a relief block does: the viewer attaches it to the spec before
    // `TilePool.start` and every worker rebuilds the world from it.
    let belt_tile = {
        let (lat0, lat1) = (BELT_LAT + 0.5, BELT_LAT - 0.5);
        let (lon0, lon1) = (BELT_LON - 0.5, BELT_LON + 0.5);
        let (width, height) = (65u32, 65u32);
        let mut tile = vec![0.0f32; 65 * 65];
        let status = wb_fill_tile_f32(
            tectonic_world,
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
        assert_eq!(status, WB_OK, "belt: the tile must fill");
        let cells: Vec<String> = tile.iter().map(|v| hex32(*v)).collect();
        println!(
            "T belt {} {} {} {} {width} {height} {} {}",
            hex(lat0),
            hex(lat1),
            hex(lon0),
            hex(lon1),
            hex(RES_M),
            cells.join(" ")
        );
        (lat0, lat1, lon0, lon1, width, height, tile)
    };

    // THE TECTONIC CONTROL'S PREDICTION, PER GROUP, MADE HERE AND CHECKED ON THE OTHER SIDE.
    //
    // `--mutate tectonic-warp` replays the `worldt` records with word 14 set to 0.0 and
    // touches nothing else, so `margin_warp_m` is the only thing that differs. Everything
    // the mutation cannot reach must compare EQUAL, and that list is the informative half:
    // the two `TP` preset groups (a world block cannot move an export that hands back
    // `tectonics.rs`' own constants -- the same reason `preset/0` and `version` sit at zero
    // under `--mutate seed`), the `TC` checker group, and every group of every world above.
    //
    // The counts are computed natively, per group, and `parity.mjs` must meet each of them
    // exactly. Three things hold the prediction to something other than its own output:
    //
    //   1. **The library agrees with the exports.** The same counts are recomputed through
    //      `Surface::elevation_m` / `structural_m` directly rather than through
    //      `wb_elevation_m` / `wb_structural_m`, and the two must be equal. A disagreement
    //      means the export layer adds or hides a difference, and it fails HERE rather than
    //      being absorbed into a divergent tally later.
    //   2. **A structural containment.** `margin_warp_m` reaches `elevation_m` only through
    //      the tectonic offset, which is `structural_m`'s own content -- so every point whose
    //      elevation moved must be a point whose structural moved. Asserted as a subset, not
    //      as an equality: the reverse does not hold, and claiming it would be claiming
    //      something false.
    //   3. **Both ends refused.** Every group's count must be strictly between zero and the
    //      group's size. A control that moves everything is as uninformative as one that
    //      moves nothing, and this file will not write a corpus where either is true.
    let (
        control_elevation_ranges,
        control_structural_ranges,
        control_elevation_belt,
        control_structural_belt,
        control_tile_belt,
    ) = {
        // The library side reads its two blocks from `tectonics.rs` rather than from the
        // sixteen words that crossed the boundary, which is what makes this a second
        // derivation instead of the same one twice: if `encode_tectonic` and
        // `decode_tectonic` disagreed anywhere, the two counts below would part company.
        let on = Surface::new(
            SEED,
            RADIUS_M,
            PLATES as usize, // cast-ok: a corpus-fixed plate count widened to usize
            LAND,
            None,
            None,
            Some(TectonicParams::ranges()),
        );
        let off = Surface::new(
            SEED,
            RADIUS_M,
            PLATES as usize, // cast-ok: as above
            LAND,
            None,
            None,
            Some(TectonicParams {
                margin_warp_m: TECTONIC_CONTROL_WARP_M,
                ..TectonicParams::ranges()
            }),
        );

        // Both scalar groups, both ways round, over the points actually dumped.
        let mut moved = [0usize; 4];
        let mut moved_lib = [0usize; 4];
        for (slot, points) in [(0usize, &scattered_points), (2, &belt_points)] {
            for (latitude_deg, longitude_deg) in points {
                let e_on = wb_elevation_m(tectonic_world, *latitude_deg, *longitude_deg, RES_M);
                let e_off =
                    wb_elevation_m(tectonic_control_world, *latitude_deg, *longitude_deg, RES_M);
                let s_on = wb_structural_m(tectonic_world, *latitude_deg, *longitude_deg);
                let s_off = wb_structural_m(tectonic_control_world, *latitude_deg, *longitude_deg);
                let e_moved = e_on.to_bits() != e_off.to_bits();
                let s_moved = s_on.to_bits() != s_off.to_bits();
                if e_moved {
                    moved[slot] += 1;
                }
                if s_moved {
                    moved[slot + 1] += 1;
                }
                assert!(
                    !e_moved || s_moved,
                    "the warp moved elevation at {latitude_deg},{longitude_deg} without moving \
                     structural -- it reaches elevation only through the tectonic offset, so \
                     this would mean it now reaches something else",
                );
                let point = SpherePoint::from_latlon(*latitude_deg, *longitude_deg);
                if on.elevation_m(&point, Some(RES_M)).to_bits()
                    != off.elevation_m(&point, Some(RES_M)).to_bits()
                {
                    moved_lib[slot] += 1;
                }
                if on.structural_m(&point).to_bits() != off.structural_m(&point).to_bits() {
                    moved_lib[slot + 1] += 1;
                }
            }
        }
        assert_eq!(
            moved, moved_lib,
            "the exports and the library disagree about how many values the warp moves; the \
             sixteen words that crossed the boundary and `TectonicParams::ranges()` itself \
             are describing different worlds",
        );

        let (lat0, lat1, lon0, lon1, width, height, on_cells) = belt_tile;
        let mut control_tile = vec![0.0f32; (width * height) as usize]; // cast-ok: a compile-time 65x65 back to a length
        let status = wb_fill_tile_f32(
            tectonic_control_world,
            lat0,
            lat1,
            lon0,
            lon1,
            width,
            height,
            RES_M,
            control_tile.as_mut_ptr(),
            width * height,
        );
        assert_eq!(status, WB_OK, "belt: the control's tile must fill");
        let tile_moved = on_cells
            .iter()
            .zip(control_tile.iter())
            .filter(|(a, b)| a.to_bits() != b.to_bits())
            .count();

        (moved[0], moved[1], moved[2], moved[3], tile_moved)
    };

    for (label, moved, total) in [
        ("elevation/ranges", control_elevation_ranges, 5_000usize),
        ("structural/ranges", control_structural_ranges, 5_000),
        ("elevation/belt", control_elevation_belt, 2_000),
        ("structural/belt", control_structural_belt, 2_000),
        ("tile/belt", control_tile_belt, 65 * 65),
    ] {
        assert!(
            moved > 0 && moved < total,
            "{label}: the control moved {moved} of {total}. A control that moves everything is \
             as uninformative as one that moves nothing, and this corpus refuses to write \
             either",
        );
    }

    // I6 (final review of water 1a): a second `H` record, on this same tectonic `ranges`
    // world, with one forced outlet inside its first enclosed pocket. `earth_like`'s
    // thresholds at 60,000 nodes put the 12b-1 node-area floor in charge of the stream
    // threshold, which is the point -- the plain `H` record above never binds that floor.
    //
    // The forced point is not chosen by hand: an unforced probe bake finds the world's
    // enclosed pockets, and the FIRST one (in bake order) gives its anchor lat/lon, exactly as
    // the ruling asks. That keeps the corpus reproducible from the tectonic block alone,
    // without a hand-picked coordinate this file would otherwise have to justify.
    const HYDRO_TECTONIC_PARAMS_BASE: [f64; 12] =
        [60_000.0, 20_000.0, 8.0, 1.0e6, 1.0e6, 2.5e8, 2.5e9, 1.0e11, 1.0, 1.0, 0.1, 0.0];

    let forced_anchor = {
        let (probe_status, probe_len, probe_words) =
            bake_hydro_native(tectonic_world, &HYDRO_TECTONIC_PARAMS_BASE);
        assert_eq!(probe_status, WB_OK, "the unforced probe bake on the ranges world must succeed");
        assert!(probe_len > 0, "a probe record of zero words has no body to anchor on");
        let record = hydrology::record::decode(&probe_words)
            .expect("the probe record must decode -- it was just encoded by this same binary");
        let body = record
            .bodies
            .iter()
            .find(|b| b.enclosed)
            .expect("the ranges world at 60,000 nodes must have at least one enclosed body");
        body.anchor
    };

    let mut hydro_tectonic_params = HYDRO_TECTONIC_PARAMS_BASE.to_vec();
    hydro_tectonic_params[11] = 1.0; // one forced outlet
    hydro_tectonic_params.push(forced_anchor.0);
    hydro_tectonic_params.push(forced_anchor.1);

    let (h_ranges_status, h_ranges_len, h_ranges_words) =
        bake_hydro_native(tectonic_world, &hydro_tectonic_params);
    assert_eq!(h_ranges_status, WB_OK, "the forced-outlet bake on the ranges world must succeed");
    assert!(h_ranges_len > 0, "a corpus of zero words would compare nothing");

    let h_ranges_params_hex: Vec<String> = hydro_tectonic_params.iter().map(|v| hex(*v)).collect();
    let h_ranges_words_hex: Vec<String> = h_ranges_words.iter().map(|v| hex(*v)).collect();
    println!(
        "H ranges {} {} {h_ranges_status} {h_ranges_len} {}",
        hydro_tectonic_params.len(),
        h_ranges_params_hex.join(" "),
        h_ranges_words_hex.join(" ")
    );

    // The native prediction for `--mutate tectonic-warp`: the same forced params, baked on the
    // warp-0 world instead, compared against the record just printed above under rule (a). This
    // is what lets `parity.mjs` require `hydro/ranges` to move by exactly this many words under
    // that control and by nothing under any other -- the same discipline `TCTL`'s other four
    // counts already hold it to.
    let (h_ranges_off_status, h_ranges_off_len, h_ranges_off_words) =
        bake_hydro_native(tectonic_control_world, &hydro_tectonic_params);
    let hydro_ranges_control = divergent_count(
        h_ranges_status,
        h_ranges_len,
        &h_ranges_words,
        h_ranges_off_status,
        h_ranges_off_len,
        &h_ranges_off_words,
    );
    assert!(
        hydro_ranges_control > 0 && hydro_ranges_control < h_ranges_len as usize + 2, // cast-ok: the record's own length, widened to compare against a divergent count over the same 2+len accounting
        "hydro/ranges: the control moved {hydro_ranges_control} of {}. A control that moves \
         everything is as uninformative as one that moves nothing, and this corpus refuses to \
         write either",
        h_ranges_len + 2,
    );

    println!(
        "TCTL {control_elevation_ranges} {control_structural_ranges} \
         {control_elevation_belt} {control_structural_belt} {control_tile_belt} \
         {hydro_ranges_control}"
    );

    // --- the coast channel: the presets, the checker, and a world built from one -----------
    //
    // Task 5 built `CoastParams` and could not expose it; Task 6 opened
    // `wb_world_new_coast` / `wb_coast_preset` / `wb_coast_check`, and **a new crossing value
    // with no corpus coverage is a crossing value nothing compares.** The tectonic block sat
    // in exactly that position for three tasks before anybody owned it, and this section is
    // written at the same time as the export rather than three tasks later.
    //
    // The shape is the tectonic channel's, one row for one row: the presets first, field by
    // field; then the checker, accepted and refused; then a world carrying a NON-canonical
    // block; then scalars on it; then a tile, because the tile worker is the block's real
    // consumer in the browser.
    //
    // **What is NOT shared between the two sides is the decode.** The block crosses as six
    // f64 in linear memory, is read back through a raw pointer, and is bounds-checked --
    // including one bound that is a *product* of three fields and one that is a loop bound
    // narrowed from an f64 -- before it becomes a `CoastParams`. That path exists only on
    // this boundary.
    let mut coast_canonical = [0.0f64; WB_COAST_STRIDE];
    let mut coast_fractal = [0.0f64; WB_COAST_STRIDE];
    let coast_canonical_status = wb_coast_preset(
        WB_COAST_CANONICAL,
        coast_canonical.as_mut_ptr(),
        WB_COAST_STRIDE as u32, // cast-ok: a compile-time stride into the export's u32 length
    );
    let coast_fractal_status = wb_coast_preset(
        WB_COAST_FRACTAL,
        coast_fractal.as_mut_ptr(),
        WB_COAST_STRIDE as u32, // cast-ok: as above
    );
    assert_eq!(coast_canonical_status, WB_OK, "the canonical coast preset must be readable");
    assert_eq!(coast_fractal_status, WB_OK, "the fractal coast preset must be readable");
    assert_ne!(
        coast_fractal, coast_canonical,
        "a preset identical to canonical would make every coast row below a second copy of \
         the plain world's rows",
    );
    for (selector, status, record) in [
        (WB_COAST_CANONICAL, coast_canonical_status, &coast_canonical),
        (WB_COAST_FRACTAL, coast_fractal_status, &coast_fractal),
    ] {
        let encoded: Vec<String> = record.iter().map(|v| hex(*v)).collect();
        println!("CP {selector} {status} {}", encoded.join(" "));
    }

    // The control's block: `fractal()` with `amplitude` -- word 0, `encode_coast`'s own order
    // -- turned back to canonical's inert value, and nothing else touched. Built from the
    // preset the export just handed back, so the five fields it keeps are not retyped either.
    const COAST_AMPLITUDE_INDEX: usize = 0;
    let coast_control_amplitude = coast_canonical[COAST_AMPLITUDE_INDEX];
    let mut coast_control = coast_fractal;
    coast_control[COAST_AMPLITUDE_INDEX] = coast_control_amplitude;
    assert_ne!(
        coast_fractal[COAST_AMPLITUDE_INDEX], coast_control_amplitude,
        "the control must actually change the field it names -- a preset that already ships \
         the control's value would make the whole control a no-op wearing a control's name",
    );

    // `wb_coast_check`, the third coast export and the only one that answers *why* a record
    // was refused. Six records, three accepted and three refused, so the group cannot be
    // trivially uniform in either direction.
    //
    // The refusals are the three the sweep found matter: the saturating `as u32` on a
    // per-sample loop bound, a **product** of three individually-admissible fields that walks
    // the noise lattice's `i64` index past saturation, and a negative amplitude -- the sign
    // this channel refuses because the lattice is zero-mean and a mirrored field is a second
    // spelling of "how far".
    let mut coast_saturating = coast_fractal;
    coast_saturating[3] = 1.0e300;
    let mut coast_product = coast_fractal;
    coast_product[2] = WB_MAX_COAST_FINEST_FREQUENCY;
    coast_product[3] = f64::from(WB_MAX_COAST_OCTAVES);
    coast_product[5] = WB_MAX_COAST_LACUNARITY;
    let mut coast_mirrored = coast_fractal;
    coast_mirrored[COAST_AMPLITUDE_INDEX] = -coast_fractal[COAST_AMPLITUDE_INDEX];
    let coast_check_records = [
        ("canonical", coast_canonical),
        ("fractal", coast_fractal),
        ("control", coast_control),
        ("saturating", coast_saturating),
        ("product", coast_product),
        ("mirrored", coast_mirrored),
    ];
    let mut coast_accepted = 0usize;
    let mut coast_refused = 0usize;
    for (name, record) in &coast_check_records {
        let status = wb_coast_check(
            record.as_ptr(),
            WB_COAST_STRIDE as u32, // cast-ok: a compile-time stride into the export's u32 length
        );
        if status == WB_OK {
            coast_accepted += 1;
        } else {
            coast_refused += 1;
        }
        let encoded: Vec<String> = record.iter().map(|v| hex(*v)).collect();
        println!("CC {name} {status} {}", encoded.join(" "));
    }
    assert_eq!(coast_accepted, 3, "three of the six coast check records are meant to be accepted");
    assert_eq!(coast_refused, 3, "three of the six coast check records are meant to be refused");

    // A world carrying the non-canonical block, twice under two names, for the reason the
    // tectonic world is: the scattered points and the concentrated ones then tally as
    // separate groups, so the control's own report says which population moved and by how
    // much. One mixed group would have hidden exactly that.
    //
    // `CAMP` goes out first because the replaying side needs it *here*, when it builds these
    // worlds; the prediction it belongs to (`CCTL`) cannot be written until the corpus has
    // been sampled.
    println!("CAMP {}", hex(coast_control_amplitude));
    let coast_encoded: Vec<String> = coast_fractal.iter().map(|v| hex(*v)).collect();
    for name in ["fractal", "shore"] {
        println!(
            "worldc {name} {SEED} {} {PLATES} {} {}",
            hex(RADIUS_M),
            hex(LAND),
            coast_encoded.join(" ")
        );
    }
    let coast_world = wb_world_new_coast(
        SEED,
        RADIUS_M,
        PLATES,
        LAND,
        core::ptr::null(),
        0,
        core::ptr::null(),
        0,
        core::ptr::null(),
        0,
        coast_fractal.as_ptr(),
        WB_COAST_STRIDE as u32, // cast-ok: a compile-time stride into the export's u32 length
    );
    assert!(coast_world != 0, "the fractal coast world must build");
    let coast_control_world = wb_world_new_coast(
        SEED,
        RADIUS_M,
        PLATES,
        LAND,
        core::ptr::null(),
        0,
        core::ptr::null(),
        0,
        core::ptr::null(),
        0,
        coast_control.as_ptr(),
        WB_COAST_STRIDE as u32, // cast-ok: as above
    );
    assert!(coast_control_world != 0, "the coast control world must build");

    // WHERE THE SHORE IS, AND WHY IT IS NOT A ROUND NUMBER.
    //
    // A 0.5-degree global scan of this exact fixture compares the canonical world against
    // `CoastParams::fractal()` and finds **162,159 of 258,480 sites moving**; the largest
    // mover is **-71.50, 38.00, where the ground moves 1,267.34 m**. A corpus scattered
    // uniformly over a sphere that is 71% open water does reach the coastal band -- that band
    // is wide -- but it does not concentrate on it, and the concentrated group is what makes
    // the control's report readable.
    const COAST_LAT: f64 = -71.5;
    const COAST_LON: f64 = 38.0;
    // **Twenty degrees, not two, and the first attempt at two is why.** The coastal window is
    // `|above_shore| <= window_spreads * spread`, which on this world is a band wide enough that
    // a 0.5-degree global scan finds 63% of all sites moving -- so a 2-degree box on the largest
    // mover is entirely INSIDE the band and every one of its 2,000 points moved. This file's own
    // both-ends-refused guard caught that and refused to write the corpus, which is the guard
    // doing exactly what it is for: a group that moves 2,000 of 2,000 is as uninformative as one
    // that moves none. Twenty degrees straddles the band and the ground either side of it.
    const COAST_SPAN_DEG: f64 = 20.0;
    {
        let point_on = wb_elevation_m(coast_world, COAST_LAT, COAST_LON, RES_M);
        let point_off = wb_elevation_m(coast_control_world, COAST_LAT, COAST_LON, RES_M);
        let moved = if point_on > point_off { point_on - point_off } else { point_off - point_on };
        assert!(
            moved > 500.0,
            "the shore site must be somewhere the control's one field actually moves the \
             ground; it moved {moved} m, so either the witness is stale or the field no \
             longer reaches this world",
        );
    }

    // 5,000 scattered points, exactly as the other two worlds take them.
    let mut coast_scattered = Vec::with_capacity(5_000);
    for _ in 0..5_000 {
        let latitude_deg = rng.unit() * 180.0 - 90.0;
        let longitude_deg = rng.unit() * 360.0 - 180.0;
        coast_scattered.push((latitude_deg, longitude_deg));
        println!(
            "E fractal {} {} {} {}",
            hex(latitude_deg),
            hex(longitude_deg),
            hex(RES_M),
            hex(wb_elevation_m(coast_world, latitude_deg, longitude_deg, RES_M))
        );
        println!(
            "S fractal {} {} {}",
            hex(latitude_deg),
            hex(longitude_deg),
            hex(wb_structural_m(coast_world, latitude_deg, longitude_deg))
        );
    }

    // 2,000 points on the shore itself, in a +/-1 degree box on the witness site.
    let mut coast_shore = Vec::with_capacity(2_000);
    for _ in 0..2_000 {
        let latitude_deg = COAST_LAT + (rng.unit() - 0.5) * COAST_SPAN_DEG;
        let longitude_deg = COAST_LON + (rng.unit() - 0.5) * COAST_SPAN_DEG;
        coast_shore.push((latitude_deg, longitude_deg));
        println!(
            "E shore {} {} {} {}",
            hex(latitude_deg),
            hex(longitude_deg),
            hex(RES_M),
            hex(wb_elevation_m(coast_world, latitude_deg, longitude_deg, RES_M))
        );
        println!(
            "S shore {} {} {}",
            hex(latitude_deg),
            hex(longitude_deg),
            hex(wb_structural_m(coast_world, latitude_deg, longitude_deg))
        );
    }

    // And a tile across the shore, because the tile worker is where a coast block lands in
    // the browser: the viewer attaches it to the spec before `TilePool.start` and every
    // worker rebuilds the world from it. A coastline that disagreed between the terrain and
    // the tiles is exactly what a decode difference would look like.
    let shore_tile = {
        // The same box the shore points take, and for the same reason: a 1-degree tile here is
        // entirely inside the coastal band and every one of its 4,225 cells moved under the
        // control. The guard below caught that too.
        let half = COAST_SPAN_DEG / 2.0;
        let (lat0, lat1) = (COAST_LAT + half, COAST_LAT - half);
        let (lon0, lon1) = (COAST_LON - half, COAST_LON + half);
        let (width, height) = (65u32, 65u32);
        let mut tile = vec![0.0f32; 65 * 65];
        let status = wb_fill_tile_f32(
            coast_world,
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
        assert_eq!(status, WB_OK, "shore: the tile must fill");
        let cells: Vec<String> = tile.iter().map(|v| hex32(*v)).collect();
        println!(
            "T shore {} {} {} {} {width} {height} {} {}",
            hex(lat0),
            hex(lat1),
            hex(lon0),
            hex(lon1),
            hex(RES_M),
            cells.join(" ")
        );
        (lat0, lat1, lon0, lon1, width, height, tile)
    };

    // THE COAST CONTROL'S PREDICTION, PER GROUP, MADE HERE AND CHECKED ON THE OTHER SIDE.
    //
    // `--mutate coast-amplitude` replays the `worldc` records with word 0 set back to
    // canonical's inert value and touches nothing else, so `amplitude` is the only thing that
    // differs. Everything the mutation cannot reach must compare EQUAL, and that list is the
    // informative half: the two `CP` preset groups (a world block cannot move an export that
    // hands back `continentality.rs`' own constants), the `CC` checker group, and every group
    // of every world above -- including both tectonic worlds, whose blocks this mutation does
    // not touch.
    //
    // The counts are computed natively, per group, and `parity.mjs` must meet each of them
    // exactly. Two things hold the prediction to something other than its own output:
    //
    //   1. **The library agrees with the exports.** The same counts are recomputed through
    //      `Surface::elevation_m` / `structural_m` directly rather than through
    //      `wb_elevation_m` / `wb_structural_m`, and the two must be equal. A disagreement
    //      means the export layer adds or hides a difference, and it fails HERE rather than
    //      being absorbed into a divergent tally later.
    //   2. **Both ends refused.** Every group's count must be strictly between zero and the
    //      group's size. A control that moves everything is as uninformative as one that
    //      moves nothing, and this file will not write a corpus where either is true.
    //
    // **No structural-containment claim is made here, unlike the tectonic control's**, and
    // that is deliberate rather than an omission: `CoastParams` reaches `elevation_m` through
    // `Continentality::base_elevation` as well as through the shelf, so an elevation that
    // moves without a structural moving is expected. Claiming the subset the warp satisfies
    // would be claiming something false.
    let (
        control_elevation_fractal,
        control_structural_fractal,
        control_elevation_shore,
        control_structural_shore,
        control_tile_shore,
    ) = {
        // The library side reads its block from `continentality.rs` rather than from the six
        // words that crossed the boundary, which is what makes this a second derivation
        // instead of the same one twice: if `encode_coast` and `decode_coast` disagreed
        // anywhere, the two counts below would part company.
        let on = Surface::with_coast(
            SEED,
            RADIUS_M,
            PLATES as usize, // cast-ok: a corpus-fixed plate count widened to usize
            LAND,
            None,
            None,
            None,
            Some(CoastParams::fractal()),
        );
        let off = Surface::with_coast(
            SEED,
            RADIUS_M,
            PLATES as usize, // cast-ok: as above
            LAND,
            None,
            None,
            None,
            Some(CoastParams { amplitude: CoastParams::canonical().amplitude, ..CoastParams::fractal() }),
        );

        let mut moved = [0usize; 4];
        let mut moved_lib = [0usize; 4];
        for (slot, points) in [(0usize, &coast_scattered), (2, &coast_shore)] {
            for (latitude_deg, longitude_deg) in points {
                let e_on = wb_elevation_m(coast_world, *latitude_deg, *longitude_deg, RES_M);
                let e_off =
                    wb_elevation_m(coast_control_world, *latitude_deg, *longitude_deg, RES_M);
                let s_on = wb_structural_m(coast_world, *latitude_deg, *longitude_deg);
                let s_off = wb_structural_m(coast_control_world, *latitude_deg, *longitude_deg);
                if e_on.to_bits() != e_off.to_bits() {
                    moved[slot] += 1;
                }
                if s_on.to_bits() != s_off.to_bits() {
                    moved[slot + 1] += 1;
                }
                let point = SpherePoint::from_latlon(*latitude_deg, *longitude_deg);
                if on.elevation_m(&point, Some(RES_M)).to_bits()
                    != off.elevation_m(&point, Some(RES_M)).to_bits()
                {
                    moved_lib[slot] += 1;
                }
                if on.structural_m(&point).to_bits() != off.structural_m(&point).to_bits() {
                    moved_lib[slot + 1] += 1;
                }
            }
        }
        assert_eq!(
            moved, moved_lib,
            "the exports and the library disagree about how many values the coast amplitude \
             moves; the six words that crossed the boundary and `CoastParams::fractal()` \
             itself are describing different worlds",
        );

        let (lat0, lat1, lon0, lon1, width, height, on_cells) = shore_tile;
        let mut control_tile = vec![0.0f32; (width * height) as usize]; // cast-ok: a compile-time 65x65 back to a length
        let status = wb_fill_tile_f32(
            coast_control_world,
            lat0,
            lat1,
            lon0,
            lon1,
            width,
            height,
            RES_M,
            control_tile.as_mut_ptr(),
            width * height,
        );
        assert_eq!(status, WB_OK, "shore: the control's tile must fill");
        let tile_moved = on_cells
            .iter()
            .zip(control_tile.iter())
            .filter(|(a, b)| a.to_bits() != b.to_bits())
            .count();

        (moved[0], moved[1], moved[2], moved[3], tile_moved)
    };

    for (label, moved, total) in [
        ("elevation/fractal", control_elevation_fractal, 5_000usize),
        ("structural/fractal", control_structural_fractal, 5_000),
        ("elevation/shore", control_elevation_shore, 2_000),
        ("structural/shore", control_structural_shore, 2_000),
        ("tile/shore", control_tile_shore, 65 * 65),
    ] {
        assert!(
            moved > 0 && moved < total,
            "{label}: the control moved {moved} of {total}. A control that moves everything is \
             as uninformative as one that moves nothing, and this corpus refuses to write \
             either",
        );
    }

    println!(
        "CCTL {control_elevation_fractal} {control_structural_fractal} \
         {control_elevation_shore} {control_structural_shore} {control_tile_shore}"
    );

    // --- the gully channel: presets, checker, a world built from one, and its control ----
    //
    // **A new crossing value with no corpus coverage is a crossing value nothing compares**,
    // which is the sentence the coast rows were written under and is why these rows are in the
    // same commit as the exports they watch, not a slice later.
    //
    // What is not shared between the two sides is the DECODE: ten f64 in linear memory, read
    // back through a raw pointer, bounds-checked field by field, and only then a
    // `GullyParams`. One of those bounds is not politeness -- `WB_MIN_GULLY_SHARPNESS` refuses
    // an exponent at or below zero, and `0^s` at a gully crest is an INFINITE HEIGHT crossing
    // this boundary into a host's vertex buffer. The `GC` records below carry that case.
    let mut gully_canonical = [0.0f64; WB_GULLY_STRIDE];
    let mut gully_drainage = [0.0f64; WB_GULLY_STRIDE];
    let gully_canonical_status = wb_gully_preset(
        WB_GULLY_CANONICAL,
        gully_canonical.as_mut_ptr(),
        WB_GULLY_STRIDE as u32, // cast-ok: a compile-time stride into the export's u32 length
    );
    let gully_drainage_status = wb_gully_preset(
        WB_GULLY_DRAINAGE,
        gully_drainage.as_mut_ptr(),
        WB_GULLY_STRIDE as u32, // cast-ok: as above
    );
    assert_eq!(gully_canonical_status, WB_OK, "the canonical gully preset must be readable");
    assert_eq!(gully_drainage_status, WB_OK, "the drainage gully preset must be readable");
    for (selector, status, record) in [
        (WB_GULLY_CANONICAL, gully_canonical_status, &gully_canonical),
        (WB_GULLY_DRAINAGE, gully_drainage_status, &gully_drainage),
    ] {
        let encoded: Vec<String> = record.iter().map(|v| hex(*v)).collect();
        println!("GP {selector} {status} {}", encoded.join(" "));
    }

    // The control's substitute for word 9, `steer_lattice_m`. **A quarter of the shipped
    // 2,000 m, and inside the domain rather than outside it**: the point of this control is
    // that a plausible value a host could send moves the ground, not that a refused one does.
    // It is carried in the corpus rather than written in `parity.mjs`, so the one number the
    // mutation substitutes arrives like every other input.
    const GULLY_CONTROL_STEER_M: f64 = 500.0;
    const GULLY_STEER_INDEX: usize = 9;

    // Six checker records, three accepted and three refused, asserted here so a checker stuck
    // at either answer cannot pass that group.
    let mut gully_infinite_crest = gully_drainage;
    gully_infinite_crest[5] = -0.5; // the crest-height infinity
    let mut gully_zero_cell = gully_drainage;
    gully_zero_cell[1] = 0.0; // the lattice index the `as i64` saturation lives behind
    let mut gully_negative_amplitude = gully_drainage;
    gully_negative_amplitude[0] = -gully_drainage[0]; // a field that looks configured and inverts the term
    let mut gully_control_record = gully_drainage;
    gully_control_record[GULLY_STEER_INDEX] = GULLY_CONTROL_STEER_M;
    let gully_check_records = [
        ("canonical", gully_canonical),
        ("drainage", gully_drainage),
        ("control", gully_control_record),
        ("infinite-crest", gully_infinite_crest),
        ("zero-cell", gully_zero_cell),
        ("negative-amplitude", gully_negative_amplitude),
    ];
    let mut gully_accepted = 0usize;
    let mut gully_refused = 0usize;
    for (name, record) in &gully_check_records {
        let status = wb_gully_check(
            record.as_ptr(),
            WB_GULLY_STRIDE as u32, // cast-ok: a compile-time stride into the export's u32 length
        );
        if status == WB_OK {
            gully_accepted += 1;
        } else {
            gully_refused += 1;
        }
        let encoded: Vec<String> = record.iter().map(|v| hex(*v)).collect();
        println!("GC {name} {status} {}", encoded.join(" "));
    }
    assert_eq!(gully_accepted, 3, "three of the six gully check records are meant to be accepted");
    assert_eq!(gully_refused, 3, "three of the six gully check records are meant to be refused");

    println!("GSTEER {}", hex(GULLY_CONTROL_STEER_M));
    let gully_encoded: Vec<String> = gully_drainage.iter().map(|v| hex(*v)).collect();
    for name in ["drainage", "flank"] {
        println!(
            "worldg {name} {SEED} {} {PLATES} {} {}",
            hex(RADIUS_M),
            hex(LAND),
            gully_encoded.join(" ")
        );
    }
    let gully_world = wb_world_new_gully(
        SEED,
        RADIUS_M,
        PLATES,
        LAND,
        core::ptr::null(),
        0,
        core::ptr::null(),
        0,
        core::ptr::null(),
        0,
        core::ptr::null(),
        0,
        gully_drainage.as_ptr(),
        WB_GULLY_STRIDE as u32, // cast-ok: a compile-time stride into the export's u32 length
    );
    assert!(gully_world != 0, "the drainage gully world must build");
    let gully_control_world = wb_world_new_gully(
        SEED,
        RADIUS_M,
        PLATES,
        LAND,
        core::ptr::null(),
        0,
        core::ptr::null(),
        0,
        core::ptr::null(),
        0,
        core::ptr::null(),
        0,
        gully_control_record.as_ptr(),
        WB_GULLY_STRIDE as u32, // cast-ok: as above
    );
    assert!(gully_control_world != 0, "the gully control world must build");

    // WHERE THE GATE IS OPEN, AND WHY IT IS NOT A ROUND NUMBER.
    //
    // A uniform scatter over a planet does not land on a flank. `GullyParams::drainage()`'s
    // gate opens over 200-1,100 m of structural ground, and 0.9% of this world is above 800 m.
    // These coordinates are the steepest decile of that -- found by walking 400,000 spiral
    // points on this exact fixture and rounding to a quarter degree, with the rounded site's
    // own `structural_m` re-checked. The concentrated group is what makes the control's report
    // readable; the scattered group is the evidence that the block does NOT reach the rest of
    // the planet.
    const GULLY_LAT: f64 = -8.75;
    const GULLY_LON: f64 = 65.0;
    // **Twenty degrees, not four, and the first attempt at four is why.** A four-degree box on
    // this witness is entirely inside the landmass the flank belongs to, and every one of its
    // 2,000 points moved under the control -- this file's own both-ends-refused guard caught
    // that and refused to write the corpus, exactly as it caught the coast control's first
    // two-degree cut. A group that moves everything is as uninformative as one that moves
    // nothing. Twenty degrees straddles the gate: the high ground, the low ground around it,
    // and the sea beyond that.
    const GULLY_SPAN_DEG: f64 = 20.0;
    {
        let point_on = wb_elevation_m(gully_world, GULLY_LAT, GULLY_LON, RES_M);
        let point_off = wb_elevation_m(plain, GULLY_LAT, GULLY_LON, RES_M);
        let moved = if point_on > point_off { point_on - point_off } else { point_off - point_on };
        assert!(
            moved > 1.0,
            "the flank site must be somewhere the drainage block actually moves the ground; \
             it moved {moved} m, so either the witness is stale or the gate no longer opens \
             there",
        );
    }

    let mut gully_scattered = Vec::with_capacity(5_000);
    for _ in 0..5_000 {
        let latitude_deg = rng.unit() * 180.0 - 90.0;
        let longitude_deg = rng.unit() * 360.0 - 180.0;
        gully_scattered.push((latitude_deg, longitude_deg));
        println!(
            "E drainage {} {} {} {}",
            hex(latitude_deg),
            hex(longitude_deg),
            hex(RES_M),
            hex(wb_elevation_m(gully_world, latitude_deg, longitude_deg, RES_M))
        );
        println!(
            "S drainage {} {} {}",
            hex(latitude_deg),
            hex(longitude_deg),
            hex(wb_structural_m(gully_world, latitude_deg, longitude_deg))
        );
    }

    let mut gully_flank = Vec::with_capacity(2_000);
    for _ in 0..2_000 {
        let latitude_deg = GULLY_LAT + (rng.unit() - 0.5) * GULLY_SPAN_DEG;
        let longitude_deg = GULLY_LON + (rng.unit() - 0.5) * GULLY_SPAN_DEG;
        gully_flank.push((latitude_deg, longitude_deg));
        println!(
            "E flank {} {} {} {}",
            hex(latitude_deg),
            hex(longitude_deg),
            hex(RES_M),
            hex(wb_elevation_m(gully_world, latitude_deg, longitude_deg, RES_M))
        );
        println!(
            "S flank {} {} {}",
            hex(latitude_deg),
            hex(longitude_deg),
            hex(wb_structural_m(gully_world, latitude_deg, longitude_deg))
        );
    }

    // And a tile across the flank, because the tile worker is where a gully block lands in the
    // browser -- and because this is the only block in the corpus whose term is FADED BY
    // RESOLUTION. `Detail::gully_offset_m` drops the whole term when the caller's spacing is
    // coarser than half a stripe wavelength, so a scalar corpus at one resolution cannot see
    // the branch the tiles take.
    let flank_tile = {
        let half = GULLY_SPAN_DEG / 2.0;
        let (lat0, lat1) = (GULLY_LAT + half, GULLY_LAT - half);
        let (lon0, lon1) = (GULLY_LON - half, GULLY_LON + half);
        let (width, height) = (65u32, 65u32);
        let mut tile = vec![0.0f32; 65 * 65];
        let status = wb_fill_tile_f32(
            gully_world,
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
        assert_eq!(status, WB_OK, "flank: the tile must fill");
        let cells: Vec<String> = tile.iter().map(|v| hex32(*v)).collect();
        println!(
            "T flank {} {} {} {} {width} {height} {} {}",
            hex(lat0),
            hex(lat1),
            hex(lon0),
            hex(lon1),
            hex(RES_M),
            cells.join(" ")
        );
        (lat0, lat1, lon0, lon1, width, height, tile)
    };

    // THE GULLY CONTROL'S PREDICTION, PER GROUP, MADE HERE AND CHECKED ON THE OTHER SIDE.
    //
    // `--mutate gully-steer` replays the `worldg` records with word 9, `steer_lattice_m`, set
    // to 500 m and touches nothing else. That word reaches ONE thing: the world-anchored
    // lattice the gully kernel takes its steering gradient from. It cannot reach
    // `wb_gully_preset` (which hands back `detail.rs`'s own constants), it cannot reach
    // `wb_gully_check` (whose records it does not touch), and it cannot reach any world built
    // without a gully block.
    //
    // **THE STRUCTURAL GROUPS ARE PREDICTED AT EXACTLY ZERO, AND THAT IS THE CLAIM WORTH
    // MAKING.** The gully term is detail; `structural_m` is defined before detail exists and
    // is the very signal the steering lattice reads. If a structural value moved here, the
    // term would have escaped its layer and would be steering on itself -- the recursion
    // `.superpowers/sdd/notes/gradient-probe.md` section 2.4 settled architecturally. So this
    // control's zeros are not an absence of evidence; they are the assertion.
    let (
        control_elevation_drainage,
        control_structural_drainage,
        control_elevation_flank,
        control_structural_flank,
        control_tile_flank,
    ) = {
        // The library side reads its block from `detail.rs` rather than from the ten words
        // that crossed the boundary, which is what makes this a second derivation rather than
        // the same one twice: if `encode_gully` and `decode_gully` disagreed anywhere, the two
        // counts below would part company.
        let on = Surface::with_gully(
            SEED,
            RADIUS_M,
            PLATES as usize, // cast-ok: a corpus-fixed plate count widened to usize
            LAND,
            None,
            None,
            None,
            None,
            Some(GullyParams::drainage()),
        );
        let off = Surface::with_gully(
            SEED,
            RADIUS_M,
            PLATES as usize, // cast-ok: as above
            LAND,
            None,
            None,
            None,
            None,
            Some(GullyParams {
                steer_lattice_m: GULLY_CONTROL_STEER_M,
                ..GullyParams::drainage()
            }),
        );

        let mut moved = [0usize; 4];
        let mut moved_lib = [0usize; 4];
        for (slot, points) in [(0usize, &gully_scattered), (2, &gully_flank)] {
            for (latitude_deg, longitude_deg) in points {
                let e_on = wb_elevation_m(gully_world, *latitude_deg, *longitude_deg, RES_M);
                let e_off =
                    wb_elevation_m(gully_control_world, *latitude_deg, *longitude_deg, RES_M);
                let s_on = wb_structural_m(gully_world, *latitude_deg, *longitude_deg);
                let s_off = wb_structural_m(gully_control_world, *latitude_deg, *longitude_deg);
                if e_on.to_bits() != e_off.to_bits() {
                    moved[slot] += 1;
                }
                if s_on.to_bits() != s_off.to_bits() {
                    moved[slot + 1] += 1;
                }
                let point = SpherePoint::from_latlon(*latitude_deg, *longitude_deg);
                if on.elevation_m(&point, Some(RES_M)).to_bits()
                    != off.elevation_m(&point, Some(RES_M)).to_bits()
                {
                    moved_lib[slot] += 1;
                }
                if on.structural_m(&point).to_bits() != off.structural_m(&point).to_bits() {
                    moved_lib[slot + 1] += 1;
                }
            }
        }
        assert_eq!(
            moved, moved_lib,
            "the exports and the library disagree about how many values the steering lattice \
             moves; the ten words that crossed the boundary and `GullyParams::drainage()` \
             itself are describing different worlds",
        );

        let (lat0, lat1, lon0, lon1, width, height, on_cells) = flank_tile;
        let mut control_tile = vec![0.0f32; (width * height) as usize]; // cast-ok: a compile-time 65x65 back to a length
        let status = wb_fill_tile_f32(
            gully_control_world,
            lat0,
            lat1,
            lon0,
            lon1,
            width,
            height,
            RES_M,
            control_tile.as_mut_ptr(),
            width * height,
        );
        assert_eq!(status, WB_OK, "flank: the control's tile must fill");
        let tile_moved = on_cells
            .iter()
            .zip(control_tile.iter())
            .filter(|(a, b)| a.to_bits() != b.to_bits())
            .count();

        (moved[0], moved[1], moved[2], moved[3], tile_moved)
    };

    for (label, moved, total) in [
        ("elevation/drainage", control_elevation_drainage, 5_000usize),
        ("elevation/flank", control_elevation_flank, 2_000),
        ("tile/flank", control_tile_flank, 65 * 65),
    ] {
        assert!(
            moved > 0 && moved < total,
            "{label}: the control moved {moved} of {total}. A control that moves everything is \
             as uninformative as one that moves nothing, and this corpus refuses to write \
             either",
        );
    }
    // The containment claim, asserted rather than observed. See the comment above.
    assert_eq!(
        control_structural_drainage, 0,
        "a gully block moved structural_m on the scattered points; the term has escaped detail",
    );
    assert_eq!(
        control_structural_flank, 0,
        "a gully block moved structural_m on the flank points; the term has escaped detail",
    );

    println!(
        "GCTL {control_elevation_drainage} {control_structural_drainage} \
         {control_elevation_flank} {control_structural_flank} {control_tile_flank}"
    );

    // --- the hydrology channel: one capped bake, through the shipped export -------------
    //
    // Task 11. `wb_hydro_bake` is the only door onto the hydrology bake across the shipped
    // surface -- before it existed, that bake's native/WASM agreement was unfalsifiable,
    // exactly the sense this file's own doc gives for `wb_erosion_run` and `wb_water_run`.
    //
    // **Controller Ruling C.** This twelve-word params array is a SEPARATE literal from
    // `tests/wasm_exports.rs::hydro_params`, not a shared function: an example cannot see a
    // test module's helpers, so the corpus and that export's own parameter-validation tests
    // each carry their own copy of the fixture. Keep the two equal by inspection if either
    // changes; a silent drift between them would mean the corpus and the unit tests are no
    // longer describing the same bake.
    const HYDRO_PARAMS: [f64; 12] =
        [12_000.0, 500.0, 8.0, 1.0e6, 1.0e6, 3.0e10, 3.0e11, 3.0e12, 1.0, 1.0, 0.1, 0.0];

    let mut hydro_id: u32 = 0;
    let hydro_status =
        wb_hydro_bake(plain, HYDRO_PARAMS.as_ptr(), HYDRO_PARAMS.len() as u32, &mut hydro_id); // cast-ok: a compile-time twelve-word buffer
    assert_eq!(hydro_status, WB_OK, "the hydro bake must succeed for the parity corpus");
    let hydro_len = wb_hydro_len(hydro_id);
    assert!(hydro_len > 0, "a corpus of zero words would compare nothing");
    let mut hydro_words = vec![0.0f64; hydro_len as usize];
    assert_eq!(wb_hydro_copy(hydro_id, hydro_words.as_mut_ptr(), hydro_len), WB_OK);
    assert_eq!(wb_hydro_free(hydro_id), WB_OK);

    let params_hex: Vec<String> = HYDRO_PARAMS.iter().map(|v| hex(*v)).collect();
    let words_hex: Vec<String> = hydro_words.iter().map(|v| hex(*v)).collect();
    println!(
        "H plain {} {} {hydro_status} {hydro_len} {}",
        HYDRO_PARAMS.len(),
        params_hex.join(" "),
        words_hex.join(" ")
    );

    println!("version {}", wb_generator_version());
}
