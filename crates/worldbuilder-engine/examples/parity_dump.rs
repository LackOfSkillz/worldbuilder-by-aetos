//! Dump a native corpus for the native-against-WASM parity harness.
//!
//! Every value crosses the *shipped* `extern "C"` surface -- `wb_world_new`,
//! `wb_world_new_relief`, `wb_relief_preset`, `wb_elevation_m`, `wb_structural_m`,
//! `wb_bottom_at`, `wb_fill_tile_f32`, `wb_erosion_run`, `wb_water_run` -- never an internal
//! function, because the claim under test is about what the browser calls.
//!
//! **Two of those exist so that a module could be compared at all.** `wb_erosion_run` (slice
//! 5a Task 5) is the only door into `erosion.rs`, and `wb_water_run` (slice 5b Task 5) is the
//! only door into `water.rs`: before each existed, that module's own claim of native/WASM
//! agreement was *unfalsifiable* -- not unverified -- because nothing in the export surface
//! touched it. The relief entries are a different shape of gap: the surface *did* reach
//! `detail.rs`'s relief block after the relief slice's Task 4, and nothing here had ever sent
//! one, because every world above is built through `wb_world_new`, which sends `None`.
//!
//! **One derivation here is NOT a compared value and is labelled as such:** the `WCTL` record
//! carries the water control's predicted divergence, computed from
//! `water::lake_body_surface_areas_m2` rather than from the classifier the control perturbs,
//! and cross-checked against that classifier before it is written. A control gate read off
//! the control's own run is a rubber stamp; this one is a prediction the replaying side has
//! to meet.
//!
//! The output is the corpus *and* its answers: every f64 is written as its 16-hex-digit
//! bit pattern, so the replaying side parses no decimal text and the comparison is exact.
//! `parity/parity.mjs` reads this file, replays the identical inputs through the committed
//! `.wasm`, and compares bit patterns. The corpus is therefore defined once, here, and
//! cannot drift between the two sides.
//!
//! Run: `cargo run --release --example parity_dump --features wasm > native.txt`

use worldbuilder_engine::stream::{sample_nodes, BuildParams, SamplingKind, StreamGraph};
use worldbuilder_engine::surface::Surface;
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
        let surface = Surface::new(SEED, RADIUS_M, PLATES as usize, LAND, None, None); // cast-ok: a corpus-fixed plate count widened to usize
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

    println!("version {}", wb_generator_version());
}
