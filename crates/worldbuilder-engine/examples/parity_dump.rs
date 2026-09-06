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

    println!(
        "TCTL {control_elevation_ranges} {control_structural_ranges} \
         {control_elevation_belt} {control_structural_belt} {control_tile_belt}"
    );

    println!("version {}", wb_generator_version());
}
