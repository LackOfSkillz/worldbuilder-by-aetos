//! The WASM export surface, exercised from outside the crate exactly as a JS host reaches
//! it: through the `extern "C"` entry points and raw pointers into linear memory.
//!
//! **Run natively, deliberately.** These tests are about the *boundary contract* -- what a
//! handle means, which grid a tile is, what a status code says, which inputs are refused --
//! and every one of those is host-independent.
//!
//! **Native-against-WASM parity is a separate, committed harness**, not an assertion made
//! here and not a figure in a task report: `examples/parity_dump.rs` plus
//! `parity/parity.mjs` compare 71,596 values through the *shipped* exports -- scattered
//! open water, inside a placed harbour, three 65x65 tiles, one `wb_erosion_run` corpus, both
//! relief presets, a world built from a non-canonical relief block, and one `wb_water_run`
//! water manifest -- against the committed `.wasm`, with `--mutate seed`, `--mutate erosion-k`
//! and `--mutate water-pond` controls that must all three turn some of it red. See
//! `parity/README.md` for the populations, the invocation and the recorded output. It is
//! not a `cargo test` because the only two ways to make it one are a WASM runtime
//! dev-dependency or a test that skips when `node` is absent; that README says so, and
//! why. What a native run *cannot* see either way is the artifact's export section, which
//! is why the module keeps a declared export list and a test that holds the source to it.
//!
//! **The whole file is off unless `--features wasm`**, so the other three configurations
//! see an empty test binary rather than a missing symbol.
#![cfg(feature = "wasm")]

use worldbuilder_engine::features::{Feature, Features, CARVE, RAISE};
use worldbuilder_engine::sphere::SpherePoint;
use worldbuilder_engine::surface::{FeatureInput, Surface};
use worldbuilder_engine::wasm::*;
use worldbuilder_engine::{World, GENERATOR_VERSION};

const SEED: i64 = 20_260_904;
const RADIUS_M: f64 = 6_371_000.0;
const PLATES: u32 = 12;
const LAND: f64 = 0.29;
const RES_M: f64 = 250.0;

/// The witnessed value the extraction pinned three ways -- Python wheel, native Rust and
/// browser WASM all give this at lat 12.0, lon 34.0, `resolution_m = 250`, on
/// `Surface::new(20260904, 6_371_000.0, 12, 0.29, None)`.
const WITNESSED_ELEVATION_M: f64 = 682.3921701573904;

/// Where `harbour_records` places its harbour.
const HARBOUR_LAT: f64 = -18.25;
const HARBOUR_LON: f64 = 121.5;

fn plain_world() -> u32 {
    wb_world_new(SEED, RADIUS_M, PLATES, LAND, core::ptr::null(), 0)
}

/// The extraction's harbour: a 900x260 m `CARVE` to -12 m with a 200x60 m `RAISE` to +4 m
/// inside it, both on the same bearing, packed as this module's flat f64 records.
fn harbour_records() -> Vec<f64> {
    vec![
        HARBOUR_LAT, HARBOUR_LON, -12.0, 900.0, 260.0, 35.0, WB_COMPOSE_CARVE, WB_SUBSTRATE_DERIVE,
        HARBOUR_LAT, HARBOUR_LON, 4.0, 200.0, 60.0, 35.0, WB_COMPOSE_RAISE, WB_SUBSTRATE_DERIVE,
    ]
}

fn harbour_world() -> u32 {
    let records = harbour_records();
    wb_world_new(SEED, RADIUS_M, PLATES, LAND, records.as_ptr(), 2)
}

/// The same two features, built through the engine's own types, so a test can compare the
/// channel against what it claims to construct.
fn harbour_surface() -> Surface {
    let mut features = Vec::new();
    for (target_m, length_m, width_m, compose) in
        [(-12.0, 900.0, 260.0, CARVE), (4.0, 200.0, 60.0, RAISE)]
    {
        features.push(Feature {
            kind: String::new(),
            at: SpherePoint::from_latlon(HARBOUR_LAT, HARBOUR_LON),
            target_m,
            length_m,
            width_m,
            bearing_deg: 35.0,
            compose: compose.to_string(),
            marked: false,
            substrate: None,
        });
    }
    Surface::new(SEED, RADIUS_M, 12, LAND, Some(FeatureInput::Loose(features)), None, None)
}

// ----------------------------------------------------------------- identity and memory

#[test]
fn the_generator_version_export_is_the_crate_constant() {
    assert_eq!(wb_generator_version(), GENERATOR_VERSION);
}

#[test]
fn alloc_returns_an_eight_aligned_buffer_that_dealloc_takes_back() {
    for bytes in [8u32, 64, 4225 * 4, 65536] {
        let p = wb_alloc(bytes);
        assert!(!p.is_null(), "wb_alloc({bytes}) returned null");
        assert_eq!(p as usize % 8, 0, "wb_alloc({bytes}) is not 8-aligned");
        // Writable across its whole declared length, which is the only thing a host cares
        // about: a short allocation that reads back fine for eight bytes is the bug.
        let len = usize::try_from(bytes).expect("a test-sized allocation");
        unsafe { core::ptr::write_bytes(p, 0xA5, len) };
        assert_eq!(unsafe { *p.add(len - 1) }, 0xA5);
        assert_eq!(wb_dealloc(p, bytes), WB_OK);
    }
}

#[test]
fn alloc_of_nothing_is_null_and_dealloc_of_nothing_is_refused() {
    assert!(wb_alloc(0).is_null());
    assert_eq!(wb_dealloc(core::ptr::null_mut(), 8), WB_ERR_BUFFER);
    assert_eq!(wb_dealloc(core::ptr::null_mut(), 0), WB_ERR_BUFFER);
}

// ------------------------------------------------------------------- the handle model

#[test]
fn a_handle_is_never_zero_and_a_freed_slot_is_never_reissued() {
    let a = plain_world();
    let b = plain_world();
    assert_ne!(a, 0);
    assert_ne!(b, 0);
    assert_ne!(a, b);
    assert_eq!(wb_world_count(), 2);

    assert_eq!(wb_world_free(a), WB_OK);
    assert_eq!(wb_world_count(), 1);

    let c = plain_world();
    assert_ne!(c, a, "a freed handle was reissued -- a stale host reference now aliases");
    assert_ne!(c, b);
    assert_eq!(wb_world_count(), 2);
}

#[test]
fn a_stale_handle_is_refused_by_every_entry_point() {
    let h = plain_world();
    assert_eq!(wb_world_free(h), WB_OK);
    assert_eq!(wb_world_free(h), WB_ERR_HANDLE, "a double free must be refused");

    assert!(wb_elevation_m(h, 12.0, 34.0, RES_M).is_nan());
    assert!(wb_structural_m(h, 12.0, 34.0).is_nan());

    let mut out = [0.0f64; 3];
    assert_eq!(wb_bottom_at(h, 12.0, 34.0, out.as_mut_ptr()), WB_ERR_HANDLE);
    assert!(out.iter().all(|v| v.is_nan()), "an error must not leave stale payload behind");

    let mut tile = [0.0f32; 4];
    assert_eq!(
        wb_fill_tile_f32(h, 1.0, 0.0, 0.0, 1.0, 2, 2, RES_M, tile.as_mut_ptr(), 4),
        WB_ERR_HANDLE
    );
}

#[test]
fn handle_zero_is_refused_without_being_special_cased_anywhere_else() {
    assert!(wb_elevation_m(0, 12.0, 34.0, RES_M).is_nan());
    assert_eq!(wb_world_free(0), WB_ERR_HANDLE);
}

// ------------------------------------------------------ construction, and its refusals

#[test]
fn a_world_is_built_from_its_parameters_and_answers_the_witnessed_value() {
    let h = plain_world();
    assert_eq!(wb_elevation_m(h, 12.0, 34.0, RES_M), WITNESSED_ELEVATION_M);
    assert_eq!(wb_world_free(h), WB_OK);
}

/// The latent panic, and it is not hypothetical. `Continentality::new` indexes
/// `values[((1 - land_fraction) * (n - 1)) as usize]`, so a `land_fraction` below about
/// `-1/(n - 1)` indexes past the end -- which on `wasm32-unknown-unknown` (panic = abort)
/// is an unrecoverable trap that takes the whole module down and cannot be caught by the
/// JS host. Measured on this host: `-1.0` and `-inf` panic at `continentality.rs:113`;
/// `-1e-9` happens not to, which is exactly why the refusal is drawn at the *documented*
/// domain rather than at the panic boundary.
#[test]
fn a_land_fraction_outside_zero_to_one_is_refused_rather_than_trapping() {
    for bad in [-1.0f64, -1e-9, -0.5, 1.000001, 2.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            wb_world_new(SEED, RADIUS_M, PLATES, bad, core::ptr::null(), 0),
            0,
            "land_fraction {bad} must be refused"
        );
    }
    for good in [0.0f64, 0.29, 1.0] {
        let h = wb_world_new(SEED, RADIUS_M, PLATES, good, core::ptr::null(), 0);
        assert_ne!(h, 0, "land_fraction {good} is inside the domain");
        assert_eq!(wb_world_free(h), WB_OK);
    }
}

#[test]
fn a_nonpositive_or_nonfinite_radius_is_refused() {
    for bad in [0.0f64, -1.0, -0.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(wb_world_new(SEED, bad, PLATES, LAND, core::ptr::null(), 0), 0, "radius {bad}");
    }
}

/// The abort the whole-branch review found: an enormous-but-finite `radius_m` used to build
/// successfully at `wb_world_new` and only fail later, deep inside `wb_erosion_run` --
/// `4*pi*radius_m^2` overflowing to `+inf` in `stream::node_areas_m2`, which
/// `StreamGraph::build`'s area check admitted as `> 0.0`, producing a NaN height change that
/// aborted the module at iteration 1 (`extern "C"` is nounwind). **Independent of every
/// `wb_erosion_run` parameter** -- including `k = 0.0`, which the crate's own refusal tests
/// assert is otherwise valid input -- because the overflow happens before any erosion
/// parameter is read at all.
///
/// The review found a second hazard while confirming this fix: the SAME huge-radius door
/// also overflows an unrelated `i64` cast in `noise.rs`'s lattice arithmetic when elevation
/// is sampled at such a world's own coordinates, which panics under Rust's dev/test-profile
/// overflow checks -- and does so at a much smaller radius than the area overflow, so a
/// naive end-to-end test through this door in a normal (non-`--release`) `cargo test` run
/// would abort the whole test binary on the WRONG bug before ever exercising the one this
/// finding is about. `WB_MAX_WORLD_RADIUS_M` closes both hazards at their one shared point
/// of entry -- this test therefore asserts the refusal happens at `wb_world_new` itself, not
/// downstream in `wb_erosion_run`, which is what makes it safe to run in every configuration
/// rather than only under `--release`. `stream::tests::build_refuses_an_infinite_or_nan_area`
/// covers the area check directly, at the graph level, independent of any radius at all.
#[test]
fn a_radius_that_would_overflow_downstream_arithmetic_is_refused_at_world_creation() {
    for bad in [WB_MAX_WORLD_RADIUS_M * 2.0, 1.0e20, 1.0e100, 1.0e154, 1.0e200, 1.0e300, f64::MAX] {
        assert_eq!(wb_world_new(SEED, bad, PLATES, LAND, core::ptr::null(), 0), 0, "radius {bad}");
    }
    // The bound is inclusive-below: right at the ceiling still builds.
    let h = wb_world_new(SEED, WB_MAX_WORLD_RADIUS_M, PLATES, LAND, core::ptr::null(), 0);
    assert_ne!(h, 0, "radius_m == WB_MAX_WORLD_RADIUS_M is the documented edge and must be accepted");
    assert_eq!(wb_world_free(h), WB_OK);
}

#[test]
fn a_zero_or_absurd_plate_count_is_refused() {
    assert_eq!(wb_world_new(SEED, RADIUS_M, 0, LAND, core::ptr::null(), 0), 0);
    assert_eq!(
        wb_world_new(SEED, RADIUS_M, WB_MAX_PLATE_COUNT + 1, LAND, core::ptr::null(), 0),
        0
    );
    let h = wb_world_new(SEED, RADIUS_M, 1, LAND, core::ptr::null(), 0);
    assert_ne!(h, 0, "one plate is degenerate but not invalid");
    assert_eq!(wb_world_free(h), WB_OK);
}

// ------------------------------------------------------------------------------ erosion

/// This crate's own default test constants (`erosion.rs::default_test_params`,
/// `examples/parity_dump.rs`'s corpus): `node_count`, `uplift_m_per_yr`,
/// `erodibility_per_yr`, `timestep_yr`, `max_height_change_per_step_m`, `max_iterations`.
/// A threshold this tight and an iteration cap this low guarantee `NotConverged` on every
/// call, which is irrelevant to these tests -- they check refusal and survival, not the
/// solver's arithmetic (`erosion.rs` owns that; untouched by this task).
fn erosion_defaults() -> (u32, f64, f64, f64, f64, u32) {
    (3_000, 1.0e-3, 1.0e-6, 1000.0, 1.0e-9, 20)
}

/// Allocate the three output buffers `wb_erosion_run` needs and call it, returning the
/// status and (if `WB_OK`) the written heights, iterations and converged flag. Frees its
/// own scratch allocations either way, so a caller never has to on a refusal.
fn call_erosion_run(
    handle: u32,
    node_count: u32,
    uplift_m_per_yr: f64,
    erodibility_per_yr: f64,
    timestep_yr: f64,
    max_height_change_per_step_m: f64,
    max_iterations: u32,
) -> (u32, Vec<f64>, u32, u32) {
    // `node_count` itself may be one of the bad values under test (0, 1, over the ceiling)
    // -- allocate at least one f64's worth regardless, so a deliberately-invalid
    // `node_count` never collides with `wb_alloc(0)`'s own "null on nothing" contract
    // (`alloc_of_nothing_is_null_and_dealloc_of_nothing_is_refused`, elsewhere in this
    // file) and produces a spurious failure in this helper instead of in `wb_erosion_run`.
    let alloc_bytes = node_count.max(1) * 8;
    let heights_ptr = wb_alloc(alloc_bytes);
    let iterations_ptr = wb_alloc(4) as *mut u32;
    let converged_ptr = wb_alloc(4) as *mut u32;
    assert!(!heights_ptr.is_null() && !iterations_ptr.is_null() && !converged_ptr.is_null());
    let status = wb_erosion_run(
        handle,
        node_count,
        uplift_m_per_yr,
        erodibility_per_yr,
        timestep_yr,
        max_height_change_per_step_m,
        max_iterations,
        heights_ptr as *mut f64,
        node_count,
        iterations_ptr,
        converged_ptr,
    );
    let heights = if status == WB_OK {
        unsafe { core::slice::from_raw_parts(heights_ptr as *const f64, node_count as usize).to_vec() }
    } else {
        Vec::new()
    };
    let (iterations, converged) = if status == WB_OK {
        unsafe { (*iterations_ptr, *converged_ptr) }
    } else {
        (0, 0)
    };
    wb_dealloc(heights_ptr, alloc_bytes);
    wb_dealloc(iterations_ptr as *mut u8, 4);
    wb_dealloc(converged_ptr as *mut u8, 4);
    (status, heights, iterations, converged)
}

/// The abort this review found: `erodibility_per_yr < 0.0` makes `c` negative, turns
/// `1 / (1 + c)` into an amplifying map, and the field overflows to `inf` within about a
/// hundred iterations -- `-9.0e-4` at this crate's own `u`/`dt` reached `erode_to_convergence`'s
/// release-time NaN assertion at iteration 95, an abort across this `extern "C"` boundary
/// both natively and in the shipped `.wasm`. `WB_MAX_EROSION_RATE_PER_YR`'s doc names
/// `-1.0e-3` as also reachable (iteration 107) and `-1.0e-2`, `-1.0`, `-1.0e-6` as finite --
/// a band, not a single cliff, which is why this sweeps several negative values rather than
/// checking one.
#[test]
fn a_negative_erodibility_is_refused_rather_than_reaching_the_amplifying_band() {
    let h = plain_world();
    let (node_count, uplift, _k, dt, threshold, _max_iter) = erosion_defaults();
    // A wider iteration budget than the parity corpus's own 20 -- the amplifying band needs
    // ~95-107 iterations to overflow, so a refusal has to hold even when a caller asks for
    // enough steps to actually reach it.
    let max_iterations = 2_000;
    for bad in [-9.0e-4, -1.0e-3, -1.0e-2, -1.0, -1.0e-6, f64::NEG_INFINITY] {
        let (status, heights, _iterations, _converged) =
            call_erosion_run(h, node_count, uplift, bad, dt, threshold, max_iterations);
        assert_eq!(status, WB_ERR_PARAM, "erodibility_per_yr = {bad} must be refused, not run");
        assert!(heights.is_empty(), "a refusal must write nothing");
    }
    // The same call, `k` flipped positive, must still succeed -- this is a sign refusal,
    // not a magnitude one that happened to also catch the good values.
    let (status, heights, _iterations, _converged) =
        call_erosion_run(h, node_count, uplift, 9.0e-4, dt, threshold, max_iterations);
    assert_eq!(status, WB_OK);
    assert!(heights.iter().all(|v| v.is_finite()), "a valid run must not carry an inf or NaN through");
    assert_eq!(wb_world_free(h), WB_OK);
}

#[test]
fn erosion_numeric_parameters_outside_their_domain_are_refused() {
    let h = plain_world();
    let (node_count, uplift, k, dt, threshold, max_iterations) = erosion_defaults();

    // node_count: below 2, or above the ceiling.
    for bad in [0u32, 1, WB_MAX_EROSION_NODES + 1] {
        let (status, ..) = call_erosion_run(h, bad, uplift, k, dt, threshold, max_iterations);
        assert_eq!(status, WB_ERR_PARAM, "node_count = {bad}");
    }
    // uplift_m_per_yr: non-finite, or magnitude over the ceiling (either sign).
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, WB_MAX_EROSION_RATE_PER_YR * 2.0, -WB_MAX_EROSION_RATE_PER_YR * 2.0] {
        let (status, ..) = call_erosion_run(h, node_count, bad, k, dt, threshold, max_iterations);
        assert_eq!(status, WB_ERR_PARAM, "uplift_m_per_yr = {bad}");
    }
    // erodibility_per_yr: non-finite, or over the ceiling (negative already covered above).
    for bad in [f64::NAN, f64::INFINITY, WB_MAX_EROSION_RATE_PER_YR * 2.0] {
        let (status, ..) = call_erosion_run(h, node_count, uplift, bad, dt, threshold, max_iterations);
        assert_eq!(status, WB_ERR_PARAM, "erodibility_per_yr = {bad}");
    }
    // timestep_yr: non-positive, non-finite, or over the ceiling.
    for bad in [0.0, -1000.0, f64::NAN, f64::INFINITY, WB_MAX_EROSION_TIMESTEP_YR * 2.0] {
        let (status, ..) = call_erosion_run(h, node_count, uplift, k, bad, threshold, max_iterations);
        assert_eq!(status, WB_ERR_PARAM, "timestep_yr = {bad}");
    }
    // max_height_change_per_step_m (the threshold): negative or non-finite. `0.0` is valid
    // (never converges early) so it is not in this refusal list.
    for bad in [-1.0e-9, f64::NAN, f64::NEG_INFINITY] {
        let (status, ..) = call_erosion_run(h, node_count, uplift, k, dt, bad, max_iterations);
        assert_eq!(status, WB_ERR_PARAM, "max_height_change_per_step_m = {bad}");
    }
    // max_iterations: zero, or over the ceiling.
    for bad in [0u32, WB_MAX_EROSION_ITERATIONS + 1] {
        let (status, ..) = call_erosion_run(h, node_count, uplift, k, dt, threshold, bad);
        assert_eq!(status, WB_ERR_PARAM, "max_iterations = {bad}");
    }

    // Every bound is inclusive at its stated edge.
    let (status, heights, iterations, _converged) =
        call_erosion_run(h, 2, uplift, 0.0, WB_MAX_EROSION_TIMESTEP_YR, 0.0, WB_MAX_EROSION_ITERATIONS.min(50));
    assert_eq!(status, WB_OK, "the edges of the domain (node_count=2, k=0.0, dt at its ceiling, threshold=0.0) must be accepted");
    assert_eq!(heights.len(), 2);
    assert!(iterations > 0);

    assert_eq!(wb_world_free(h), WB_OK);
}

#[test]
fn wb_erosion_run_refuses_an_unknown_or_freed_handle() {
    let (node_count, uplift, k, dt, threshold, max_iterations) = erosion_defaults();
    let h = plain_world();
    assert_eq!(wb_world_free(h), WB_OK);
    let (status, heights, iterations, converged) =
        call_erosion_run(h, node_count, uplift, k, dt, threshold, max_iterations);
    assert_eq!(status, WB_ERR_HANDLE);
    assert!(heights.is_empty());
    assert_eq!(iterations, 0);
    assert_eq!(converged, 0);

    let (status, ..) = call_erosion_run(0, node_count, uplift, k, dt, threshold, max_iterations);
    assert_eq!(status, WB_ERR_HANDLE, "handle 0 is never valid");
}

#[test]
fn wb_erosion_run_refuses_a_null_or_short_output_buffer() {
    let h = plain_world();
    let (node_count, uplift, k, dt, threshold, max_iterations) = erosion_defaults();
    let iterations_ptr = wb_alloc(4) as *mut u32;
    let converged_ptr = wb_alloc(4) as *mut u32;

    // Null heights buffer.
    assert_eq!(
        wb_erosion_run(h, node_count, uplift, k, dt, threshold, max_iterations, core::ptr::null_mut(), node_count, iterations_ptr, converged_ptr),
        WB_ERR_BUFFER
    );

    // A heights buffer one element short of node_count.
    let short = wb_alloc((node_count - 1) * 8) as *mut f64;
    assert_eq!(
        wb_erosion_run(h, node_count, uplift, k, dt, threshold, max_iterations, short, node_count - 1, iterations_ptr, converged_ptr),
        WB_ERR_BUFFER
    );
    wb_dealloc(short as *mut u8, (node_count - 1) * 8);

    let heights_ptr = wb_alloc(node_count * 8) as *mut f64;

    // Null iterations / converged out-params.
    assert_eq!(
        wb_erosion_run(h, node_count, uplift, k, dt, threshold, max_iterations, heights_ptr, node_count, core::ptr::null_mut(), converged_ptr),
        WB_ERR_BUFFER
    );
    assert_eq!(
        wb_erosion_run(h, node_count, uplift, k, dt, threshold, max_iterations, heights_ptr, node_count, iterations_ptr, core::ptr::null_mut()),
        WB_ERR_BUFFER
    );

    wb_dealloc(heights_ptr as *mut u8, node_count * 8);
    wb_dealloc(iterations_ptr as *mut u8, 4);
    wb_dealloc(converged_ptr as *mut u8, 4);
    assert_eq!(wb_world_free(h), WB_OK);
}

/// A valid call succeeds, reports the fixed step count this crate's own default test
/// constants are designed to hit (see `erosion_defaults`'s doc), and every returned height
/// is finite -- a minimal end-to-end check that the export's happy path actually runs the
/// capped solver rather than only ever being reached by these refusal tests.
#[test]
fn wb_erosion_run_succeeds_on_a_valid_call_and_writes_finite_heights() {
    let h = plain_world();
    let (node_count, uplift, k, dt, threshold, max_iterations) = erosion_defaults();
    let (status, heights, iterations, converged) =
        call_erosion_run(h, node_count, uplift, k, dt, threshold, max_iterations);
    assert_eq!(status, WB_OK);
    assert_eq!(heights.len(), node_count as usize);
    assert!(heights.iter().all(|v| v.is_finite()), "every height must be finite on a valid run");
    assert_eq!(iterations, max_iterations, "this fixture is designed to hit the iteration cap, not converge early");
    assert_eq!(converged, 0);
    assert_eq!(wb_world_free(h), WB_OK);
}

// -------------------------------------------------------------------- sampling by point

#[test]
fn elevation_and_structural_are_the_engine_verbatim_bit_for_bit() {
    let h = plain_world();
    let surface = Surface::new(SEED, RADIUS_M, 12, LAND, None, None, None);
    for (lat, lon) in [(12.0, 34.0), (-63.5, -170.25), (0.0, 0.0), (89.9, 179.9), (-89.9, -179.9)] {
        let p = SpherePoint::from_latlon(lat, lon);
        assert_eq!(
            wb_elevation_m(h, lat, lon, RES_M).to_bits(),
            surface.elevation_m(&p, Some(RES_M)).to_bits(),
            "elevation at {lat},{lon}"
        );
        assert_eq!(
            wb_structural_m(h, lat, lon).to_bits(),
            surface.structural_m(&p).to_bits(),
            "structural at {lat},{lon}"
        );
    }
}

/// The sentinel, both ways: a positive finite resolution is *passed through*, and anything
/// else means canonical `None`. Getting this backwards is silent -- both branches return a
/// plausible elevation -- so the test also pins that the two branches differ at all.
#[test]
fn the_resolution_sentinel_selects_canonical_ground_truth_from_anything_nonpositive() {
    let h = plain_world();
    let surface = Surface::new(SEED, RADIUS_M, 12, LAND, None, None, None);
    let p = SpherePoint::from_latlon(12.0, 34.0);
    let canonical = surface.elevation_m(&p, None);
    let resolved = surface.elevation_m(&p, Some(RES_M));
    assert_ne!(canonical.to_bits(), resolved.to_bits(), "the two branches must be tellable apart");

    assert_eq!(wb_elevation_m(h, 12.0, 34.0, RES_M).to_bits(), resolved.to_bits());
    for sentinel in [0.0f64, -0.0, -1.0, -250.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            wb_elevation_m(h, 12.0, 34.0, sentinel).to_bits(),
            canonical.to_bits(),
            "resolution {sentinel} must mean canonical, never be passed through"
        );
    }
}

// ------------------------------------------------------------------- the inspection tap

#[test]
fn bottom_at_writes_three_fractions_and_says_ok() {
    let h = plain_world();
    let surface = Surface::new(SEED, RADIUS_M, 12, LAND, None, None, None);
    let mut out = [0.0f64; 3];
    assert_eq!(wb_bottom_at(h, 12.0, 34.0, out.as_mut_ptr()), WB_OK);
    let truth = surface.bottom_at(&SpherePoint::from_latlon(12.0, 34.0)).expect("ok");
    assert_eq!(out[0].to_bits(), truth.sand.to_bits());
    assert_eq!(out[1].to_bits(), truth.mud.to_bits());
    assert_eq!(out[2].to_bits(), truth.rock.to_bits());
}

#[test]
fn bottom_at_refuses_a_null_buffer() {
    let h = plain_world();
    assert_eq!(wb_bottom_at(h, 12.0, 34.0, core::ptr::null_mut()), WB_ERR_BUFFER);
}

/// The refusal the engine's `Result` carries, which this surface must forward rather than
/// swallow. Unreachable through `wb_world_new` -- the feature channel refuses an unknown
/// substrate code at construction -- so it is reached through `insert_world`, the Rust-side
/// door for a world built some other way.
#[test]
fn bottom_at_forwards_an_unknown_substrate_as_its_own_status() {
    let feature = Feature {
        kind: String::new(),
        at: SpherePoint::from_latlon(HARBOUR_LAT, HARBOUR_LON),
        target_m: -12.0,
        length_m: 900.0,
        width_m: 260.0,
        bearing_deg: 35.0,
        compose: CARVE.to_string(),
        marked: false,
        substrate: Some("granite".to_string()),
    };
    let surface = Surface::new(
        SEED,
        RADIUS_M,
        12,
        LAND,
        Some(FeatureInput::Built(Features::new(vec![feature], RADIUS_M))),
        None,
        None,
    );
    let h = insert_world(World::new(surface));
    let mut out = [0.0f64; 3];
    assert_eq!(wb_bottom_at(h, HARBOUR_LAT, HARBOUR_LON, out.as_mut_ptr()), WB_ERR_SUBSTRATE);
    assert!(out.iter().all(|v| v.is_nan()), "a refusal must leave NaN, not a plausible bottom");
}

// ---------------------------------------------------------------------------- the tile

/// The grid convention, stated as arithmetic: row-major, both endpoints included, row 0 at
/// `lat0` and row `height - 1` at `lat1`. A transposed or half-open tile still fills the
/// buffer with plausible elevations, so this is pinned against the scalar export at every
/// one of the 4,225 samples of a `HeightmapTerrainData`-shaped tile -- **in open water and
/// again inside a placed harbour**, because a scattered corpus never lands in a feature and
/// that gap survived every earlier probe in this project.
#[test]
fn every_tile_sample_is_the_scalar_export_narrowed_to_f32() {
    for (label, h) in [("open water", plain_world()), ("inside the harbour", harbour_world())] {
        assert_ne!(h, 0, "{label}: the world did not build");
        let (lat0, lat1) = (HARBOUR_LAT + 0.005, HARBOUR_LAT - 0.005);
        let (lon0, lon1) = (HARBOUR_LON - 0.005, HARBOUR_LON + 0.005);
        let (w, hgt) = (65u32, 65u32);
        let mut tile = vec![0.0f32; 65 * 65];
        assert_eq!(
            wb_fill_tile_f32(h, lat0, lat1, lon0, lon1, w, hgt, RES_M, tile.as_mut_ptr(), w * hgt),
            WB_OK,
            "{label}"
        );
        let mut nonzero = 0;
        for j in 0..hgt {
            for i in 0..w {
                let lat = lat0 + (lat1 - lat0) * (f64::from(j) / f64::from(hgt - 1));
                let lon = lon0 + (lon1 - lon0) * (f64::from(i) / f64::from(w - 1));
                let scalar = wb_elevation_m(h, lat, lon, RES_M);
                let narrowed = scalar as f32;
                let got = tile[usize::try_from(j * w + i).expect("index")];
                assert_eq!(
                    got.to_bits(),
                    narrowed.to_bits(),
                    "{label}: tile[{j},{i}] disagrees with the scalar export"
                );
                if got != 0.0 {
                    nonzero += 1;
                }
            }
        }
        assert_eq!(nonzero, 65 * 65, "{label}: a tile of zeroes would prove nothing");
        assert_eq!(wb_world_free(h), WB_OK);
    }
}

/// The harbour has to actually reach the tile, or the test above compares a tile against a
/// scalar export of the same nothing.
#[test]
fn the_feature_channel_moves_the_ground_under_the_tile() {
    let plain = plain_world();
    let harbour = harbour_world();
    assert_ne!(harbour, 0, "the harbour world did not build");
    let mut a = vec![0.0f32; 33 * 33];
    let mut b = vec![0.0f32; 33 * 33];
    let (lat0, lat1) = (HARBOUR_LAT + 0.002, HARBOUR_LAT - 0.002);
    let (lon0, lon1) = (HARBOUR_LON - 0.002, HARBOUR_LON + 0.002);
    for (h, buf) in [(plain, &mut a), (harbour, &mut b)] {
        assert_eq!(
            wb_fill_tile_f32(h, lat0, lat1, lon0, lon1, 33, 33, RES_M, buf.as_mut_ptr(), 33 * 33),
            WB_OK
        );
    }
    let differing = a.iter().zip(b.iter()).filter(|(x, y)| x != y).count();
    // **Pinned, not floored.** Measured: 261 of the 1,089 samples move (33x33 tile,
    // +/-0.002 deg about the harbour, `RES_M = 250`, x86_64-pc-windows-msvc). The previous
    // `> 100` left 61% headroom -- the harbour's reach could more than halve and this test
    // would still pass -- which is the same shape as an output test that cannot see the
    // decision it is guarding. An exact count is how
    // `the_grid_coordinate_is_pinned_to_one_of_the_two_lerp_forms` pins its 10 and 24.
    assert_eq!(differing, 261, "the harbour moved {differing} of 1089 samples, not 261");
    let deepest = b.iter().copied().fold(f32::INFINITY, f32::min);
    assert!(deepest <= -11.9, "the CARVE to -12 m never took: deepest is {deepest}");
    let highest = b.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    assert!(highest >= 3.9, "the RAISE to +4 m never took: highest is {highest}");
}

/// **The tile reads the sentinel through `resolution()`, exactly as the scalar export
/// does.** This is the third instance of one shape: an argument a whole corpus holds
/// constant. All twelve other `wb_fill_tile_f32` call sites in this file pass `RES_M`, and
/// `the_resolution_sentinel_selects_canonical_ground_truth_from_anything_nonpositive`
/// covers only `wb_elevation_m` -- so replacing `resolution(resolution_m)` with
/// `Some(resolution_m)` inside the tile passed the entire suite.
///
/// What that would cost, measured at lat 12.0 lon 34.0 on the plain world
/// (x86_64-pc-windows-msvc, release): `Some(-1.0)`, `Some(+inf)` and `Some(-inf)` all give
/// 681.2161549154603 where `None` gives 683.4579940205472 -- the tile and the scalar export
/// silently disagreeing by **2.24 m at the same point**, which is verbatim the drift
/// `resolution()`'s own doc says it exists to prevent. `Some(0.0)`, `Some(-0.0)` and
/// `Some(NaN)` happen to agree with `None`, so a test using only zero would not catch it;
/// all seven sentinels are here for that reason.
#[test]
fn the_tile_reads_the_resolution_sentinel_exactly_as_the_scalar_export_does() {
    let h = plain_world();
    let surface = Surface::new(SEED, RADIUS_M, 12, LAND, None, None, None);
    let p = SpherePoint::from_latlon(12.0, 34.0);
    let canonical = surface.elevation_m(&p, None) as f32;
    let resolved = surface.elevation_m(&p, Some(RES_M)) as f32;
    assert_ne!(
        canonical.to_bits(),
        resolved.to_bits(),
        "the two branches must be tellable apart even after narrowing to f32"
    );
    for sentinel in [0.0f64, -0.0, -1.0, -250.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut tile = [0.0f32; 1];
        assert_eq!(
            wb_fill_tile_f32(h, 12.0, 12.0, 34.0, 34.0, 1, 1, sentinel, tile.as_mut_ptr(), 1),
            WB_OK,
            "resolution {sentinel}"
        );
        assert_eq!(
            tile[0].to_bits(),
            canonical.to_bits(),
            "the tile passed resolution {sentinel} through instead of reading it as canonical"
        );
        assert_eq!(
            tile[0].to_bits(),
            (wb_elevation_m(h, 12.0, 34.0, sentinel) as f32).to_bits(),
            "the tile and the scalar export disagree at resolution {sentinel}"
        );
    }
    let mut tile = [0.0f32; 1];
    assert_eq!(
        wb_fill_tile_f32(h, 12.0, 12.0, 34.0, 34.0, 1, 1, RES_M, tile.as_mut_ptr(), 1),
        WB_OK
    );
    assert_eq!(
        tile[0].to_bits(),
        resolved.to_bits(),
        "a positive finite resolution must be passed through, not read as canonical"
    );
}

/// The channel builds exactly the features the engine's own types build -- bit for bit,
/// not approximately.
#[test]
fn the_feature_channel_builds_what_the_engine_types_build() {
    let h = harbour_world();
    let truth = harbour_surface();
    for (dlat, dlon) in [(0.0, 0.0), (0.001, 0.0), (0.0, 0.001), (-0.002, 0.002)] {
        let (lat, lon) = (HARBOUR_LAT + dlat, HARBOUR_LON + dlon);
        assert_eq!(
            wb_elevation_m(h, lat, lon, RES_M).to_bits(),
            truth.elevation_m(&SpherePoint::from_latlon(lat, lon), Some(RES_M)).to_bits(),
            "at {lat},{lon}"
        );
    }
}

#[test]
fn the_feature_channel_refuses_what_it_cannot_represent() {
    let base = harbour_records();
    let mut bad = base.clone();
    bad[6] = 7.0;
    assert_eq!(wb_world_new(SEED, RADIUS_M, PLATES, LAND, bad.as_ptr(), 2), 0, "compose 7");

    let mut bad = base.clone();
    bad[7] = 9.0;
    assert_eq!(wb_world_new(SEED, RADIUS_M, PLATES, LAND, bad.as_ptr(), 2), 0, "substrate 9");

    for field in 0..6 {
        let mut bad = base.clone();
        bad[field] = f64::NAN;
        assert_eq!(wb_world_new(SEED, RADIUS_M, PLATES, LAND, bad.as_ptr(), 2), 0, "field {field}");
    }

    let mut bad = base.clone();
    bad[0] = 91.0;
    assert_eq!(wb_world_new(SEED, RADIUS_M, PLATES, LAND, bad.as_ptr(), 2), 0, "lat 91");

    for field in [3usize, 4] {
        let mut bad = base.clone();
        bad[field] = 0.0;
        assert_eq!(wb_world_new(SEED, RADIUS_M, PLATES, LAND, bad.as_ptr(), 2), 0, "extent {field}");
    }

    assert_eq!(wb_world_new(SEED, RADIUS_M, PLATES, LAND, core::ptr::null(), 2), 0, "null with 2");
    assert_eq!(wb_world_count(), 0, "a refused construction leaked a world");
}

#[test]
fn a_one_by_one_tile_is_the_first_corner_and_nothing_else() {
    let h = plain_world();
    let mut tile = [0.0f32; 1];
    assert_eq!(
        wb_fill_tile_f32(h, 12.0, 13.0, 34.0, 35.0, 1, 1, RES_M, tile.as_mut_ptr(), 1),
        WB_OK
    );
    let corner = wb_elevation_m(h, 12.0, 34.0, RES_M) as f32;
    assert_eq!(tile[0].to_bits(), corner.to_bits());
}

/// **A non-square tile, because every other tile test here is square and a square tile
/// cannot tell the two axes apart.** Interpolating the columns against the *row* count
/// survived every one of the other 27 tests: 65x65, 33x33, 8x8 and 1x1 all have
/// `last_column == last_row`, and the one 1x65 case has `columns == 1`, where the
/// no-step branch returns the first bound whichever count is used. 96 columns by 17 rows
/// has neither property.
#[test]
fn a_non_square_tile_uses_its_own_count_on_each_axis() {
    let h = plain_world();
    let (w, hgt) = (96u32, 17u32);
    let (lat0, lat1) = (HARBOUR_LAT + 0.004, HARBOUR_LAT - 0.004);
    let (lon0, lon1) = (HARBOUR_LON - 0.006, HARBOUR_LON + 0.006);
    let mut tile = vec![0.0f32; 96 * 17];
    assert_eq!(
        wb_fill_tile_f32(h, lat0, lat1, lon0, lon1, w, hgt, RES_M, tile.as_mut_ptr(), w * hgt),
        WB_OK
    );
    for row in 0..hgt {
        for column in 0..w {
            let lat = grid_coordinate(lat0, lat1, f64::from(row), f64::from(hgt - 1));
            let lon = grid_coordinate(lon0, lon1, f64::from(column), f64::from(w - 1));
            let truth = wb_elevation_m(h, lat, lon, RES_M) as f32;
            assert_eq!(
                tile[usize::try_from(row * w + column).expect("index")].to_bits(),
                truth.to_bits(),
                "tile[{row},{column}] of a 96x17 grid"
            );
        }
    }
    // The last column must reach lon1 exactly, which is what says the column axis used
    // its own count: against the row count it would stop 79/95 of the way across.
    let last = grid_coordinate(lon0, lon1, f64::from(w - 1), f64::from(w - 1));
    assert_eq!(last.to_bits(), lon1.to_bits(), "the grid is not endpoint-inclusive across");
}

#[test]
fn the_tile_refuses_a_buffer_one_element_short_and_writes_nothing() {
    let h = plain_world();
    let mut tile = vec![7.0f32; 64];
    assert_eq!(
        wb_fill_tile_f32(h, 1.0, 0.0, 0.0, 1.0, 8, 8, RES_M, tile.as_mut_ptr(), 63),
        WB_ERR_BUFFER
    );
    assert!(tile.iter().all(|v| *v == 7.0), "a refused tile must not be half-written");
    assert_eq!(
        wb_fill_tile_f32(h, 1.0, 0.0, 0.0, 1.0, 8, 8, RES_M, core::ptr::null_mut(), 64),
        WB_ERR_BUFFER
    );
    assert_eq!(
        wb_fill_tile_f32(h, 1.0, 0.0, 0.0, 1.0, 8, 8, RES_M, tile.as_mut_ptr(), 64),
        WB_OK
    );
}

#[test]
fn the_tile_refuses_a_degenerate_grid_or_a_nonfinite_bound() {
    let h = plain_world();
    let mut tile = vec![0.0f32; 64];
    let p = tile.as_mut_ptr();
    assert_eq!(wb_fill_tile_f32(h, 1.0, 0.0, 0.0, 1.0, 0, 8, RES_M, p, 64), WB_ERR_GRID);
    assert_eq!(wb_fill_tile_f32(h, 1.0, 0.0, 0.0, 1.0, 8, 0, RES_M, p, 64), WB_ERR_GRID);
    for bad in [f64::NAN, f64::INFINITY] {
        assert_eq!(wb_fill_tile_f32(h, bad, 0.0, 0.0, 1.0, 8, 8, RES_M, p, 64), WB_ERR_GRID);
        assert_eq!(wb_fill_tile_f32(h, 1.0, bad, 0.0, 1.0, 8, 8, RES_M, p, 64), WB_ERR_GRID);
        assert_eq!(wb_fill_tile_f32(h, 1.0, 0.0, bad, 1.0, 8, 8, RES_M, p, 64), WB_ERR_GRID);
        assert_eq!(wb_fill_tile_f32(h, 1.0, 0.0, 0.0, bad, 8, 8, RES_M, p, 64), WB_ERR_GRID);
    }
}

/// The interpolation is `a + (b - a) * t`, not `a * (1 - t) + b * t`. They are not the same
/// function in binary floating point, and swapping one for the other is exactly the kind of
/// "equivalent" tidy-up a reviewer waves through -- so the difference is measured here
/// rather than asserted.
///
/// **And it is pinned in f64, not through the tile, because the tile cannot see it.**
/// Swapping the two forms in `wb_fill_tile_f32` changed **0 of 8,450** f32 samples over
/// both regimes of `every_tile_sample_is_the_scalar_export_narrowed_to_f32` -- one ULP of
/// latitude is about 4e-10 m on the ground and does not survive narrowing to f32. That
/// mutation survived the whole suite until this test was written against `grid_coordinate`
/// directly.
///
/// Population: the 65 row latitudes (lat0 = -18.245 to lat1 = -18.255) and the 65 column
/// longitudes (lon0 = 121.495 to lon1 = 121.505) of the tile the equality test uses, step
/// 1/64. Measured: the two forms disagree on **10 of 65** rows and **24 of 65** columns.
#[test]
fn the_grid_coordinate_is_pinned_to_one_of_the_two_lerp_forms() {
    for (a, b, expected_disagreements) in [
        (HARBOUR_LAT + 0.005, HARBOUR_LAT - 0.005, 10),
        (HARBOUR_LON - 0.005, HARBOUR_LON + 0.005, 24),
    ] {
        let mut differing = 0;
        for index in 0..65u32 {
            let t = f64::from(index) / f64::from(64u32);
            let ours = grid_coordinate(a, b, f64::from(index), f64::from(64u32));
            assert_eq!(ours.to_bits(), (a + (b - a) * t).to_bits(), "not a + (b - a) * t");
            if ours.to_bits() != (a * (1.0 - t) + b * t).to_bits() {
                differing += 1;
            }
        }
        assert_eq!(
            differing, expected_disagreements,
            "{a}..{b}: the two lerp forms disagree {differing} times, not {expected_disagreements}"
        );
    }
    // A one-row or one-column grid has no step, and must not divide zero by zero.
    assert_eq!(grid_coordinate(12.0, 13.0, 0.0, 0.0).to_bits(), 12.0f64.to_bits());
}

/// And the tile really does lay its rows out through `grid_coordinate`, rather than through
/// something that merely agrees with it at f32.
#[test]
fn the_tile_rows_are_where_grid_coordinate_puts_them() {
    let (a, b) = (HARBOUR_LAT + 0.005, HARBOUR_LAT - 0.005);
    let h = plain_world();
    let mut tile = vec![0.0f32; 65];
    assert_eq!(
        wb_fill_tile_f32(h, a, b, 34.0, 34.0, 1, 65, RES_M, tile.as_mut_ptr(), 65),
        WB_OK
    );
    for index in 0..65u32 {
        let lat = grid_coordinate(a, b, f64::from(index), f64::from(64u32));
        let truth = wb_elevation_m(h, lat, 34.0, RES_M) as f32;
        assert_eq!(
            tile[usize::try_from(index).expect("index")].to_bits(),
            truth.to_bits(),
            "row {index} is not where grid_coordinate puts it"
        );
    }
}

// -------------------------------------------------- what the module is, structurally

/// **The design decision, in executable form.** `bindings.rs::surface_elevation_m` rebuilds
/// the `Surface` on every call; this module must build one exactly once, in `wb_world_new`.
/// The ratio is **~10^3**, and it is a property of a host, not a constant. Two
/// measurements, both native `--release`, `x86_64-pc-windows-msvc`, cargo 1.98.0, both
/// after warm-up, both `Surface::new` over n = 20 worlds against `elevation_m` over
/// n = 20,000 points at `resolution_m = 250`:
///
/// - author's host: 0.657 ms/world vs 0.617 us/sample -> **1,065x**
/// - reviewer's host: 0.5075 ms/world vs 0.5642 us/sample -> **900x**
///
/// Same conclusion, 15% apart, and the scatter of the 20,000 points is the dominant term:
/// elevation cost varies ~9x between a coastal tile and a deep-ocean one, so a corpus that
/// is not named is a ratio nobody can reproduce. A 65x65 tile is 4,225 samples either way.
/// A source scan is the only check of that shape which survives a refactor.
#[test]
fn the_surface_is_built_once_per_world_and_never_per_sample() {
    let source = include_str!("../src/wasm.rs");
    let code = source
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    let builds = code.matches("Surface::new").count();
    assert_eq!(
        builds, 1,
        "wasm.rs builds a Surface {builds} times; a sampling path that rebuilds costs ~10^3x"
    );
    let before = &code[..code.find("Surface::new").expect("one build")];
    assert!(
        before.contains("fn wb_world_new"),
        "the one Surface::new is not inside wb_world_new"
    );
}

/// **The allocator's alignment, pinned in the source, because no host can see it.**
///
/// Dropping `WB_ALIGN` to 1 passes the whole suite: this host's MSVC `HeapAlloc`
/// guarantees 16 bytes and wasm32's dlmalloc over-aligns too, so every buffer comes back
/// 8-aligned anyway and no *behavioural* test on either target can tell the difference.
/// That is not the same as unfixable -- it is the same situation as `Surface::new` and the
/// export list, and it takes the same answer. Measured: with `WB_ALIGN = 1` this test
/// fails (437 passed / 1 failed); unmutated it passes (438 / 0).
///
/// The count is 3 because the constant is defined once and used at both
/// `Layout::from_size_align` sites -- `wb_alloc` and `wb_dealloc`. A mismatched pair is
/// undefined behaviour rather than a leak, so what is held here is "both sites use the same
/// named constant", not merely the number 8.
#[test]
fn the_allocator_alignment_is_pinned_in_the_source_because_no_host_can_see_it() {
    let source = include_str!("../src/wasm.rs");
    assert!(
        source.contains("const WB_ALIGN: usize = 8;"),
        "wb_alloc's alignment is not 8; an f64 payload would be misaligned on any host that \
         does not over-align, and no behavioural test here can see it"
    );
    assert_eq!(
        source.matches("WB_ALIGN").count(),
        3,
        "WB_ALIGN must appear exactly three times: the definition, and the from_size_align \
         site in each of wb_alloc and wb_dealloc"
    );
}

/// **Zero imports, no JS glue.** A `wasm-bindgen` attribute anywhere here would add an
/// import object and a build dependency in place of hand marshalling already measured at
/// ~2% of a sample. That is the ruling, not a preference.
#[test]
fn nothing_here_reaches_for_a_binding_generator() {
    let source = include_str!("../src/wasm.rs");
    for banned in ["wasm_bindgen", "js_sys", "web_sys", "extern crate"] {
        assert!(!source.contains(banned), "wasm.rs reaches for {banned}");
    }
}

/// The export list is a *declaration*, checked against the source, because a native test
/// run cannot see the artifact's export section and a forgotten `#[no_mangle]` is exactly
/// the failure mode that produced a 327-byte module exporting only `memory`.
#[test]
fn the_declared_export_list_is_the_source() {
    let source = include_str!("../src/wasm.rs");
    let found: Vec<String> = source
        .lines()
        .filter(|l| l.starts_with("pub extern \"C\" fn "))
        .map(|l| {
            let rest = l.trim_start_matches("pub extern \"C\" fn ");
            rest[..rest.find('(').expect("a signature")].to_string()
        })
        .collect();
    let mut declared: Vec<String> = WB_EXPORTS.iter().map(|s| (*s).to_string()).collect();
    let mut found_sorted = found.clone();
    declared.sort();
    found_sorted.sort();
    assert_eq!(found_sorted, declared, "WB_EXPORTS disagrees with the source");

    let no_mangle = source.matches("#[no_mangle]").count();
    assert_eq!(no_mangle, WB_EXPORTS.len(), "a pub extern fn without #[no_mangle] is invisible");
}

// ============================================================================ the relief channel
//
// Slice `2026-09-05-slice-relief-amplitude`, Task 4. The rest of this file tests a boundary
// that was already there; this section tests one being opened, and the thing it is mostly
// about is the **nounwind** property `wasm.rs`'s own module doc states: an `extern "C"`
// function that panics aborts the process, and in wasm that is a dead module and a blank
// viewer with nothing useful in the console.
//
// Slice 5a shipped two reachable aborts through exports whose bounds looked complete, and
// **both were bands rather than cliffs** -- fine at one end, fine at the other, fatal
// somewhere in between. So the tests below **sweep**. Every record a sweep produces is put
// through `wb_relief_check` AND `wb_world_new_relief`; every record the boundary accepts is
// then actually *sampled*, because a bad relief block does not fail in the constructor, it
// fails the first time `Detail::plan`'s schedule or `Detail::offset_m`'s loop is walked. A
// test that only asserted a validator returns false for three hand-picked values would have
// missed both of slice 5a's aborts, and it would miss the non-terminating loop this channel
// closes.
//
// Population/method/host for every figure in this section: the world is
// `Surface::new(20_260_904, 6_371_000, 12, 0.29, None)` -- the same `SEED`/`RADIUS_M`/
// `PLATES`/`LAND` fixture the rest of this file uses, whose elevation at lat 12 lon 34 is
// witnessed three ways. The probe points are `RELIEF_PROBES` below. The host is a native
// `cargo test -p worldbuilder-engine --features wasm` run; native and WASM parity for the
// export surface is a separate committed harness (see this file's own module doc).

/// Where every accepted relief record is sampled. Chosen to cross the settings
/// `Detail::amplitude_m` blends between -- deep water, shelf, shoreline, ordinary land, and
/// the witnessed point -- because a record that only ever touched one of those five terms
/// would leave the other four unswept.
const RELIEF_PROBES: &[(f64, f64)] = &[
    (12.0, 34.0), // the witnessed point
    (0.0, 0.0),
    (-18.25, 121.5), // the harbour, near a coast
    (62.5, -145.0),
    (-71.0, 25.0),
    (35.0, 138.0),
];

fn canonical_record() -> [f64; WB_RELIEF_STRIDE] {
    let mut record = [0.0; WB_RELIEF_STRIDE];
    let status = wb_relief_preset(WB_RELIEF_CANONICAL, record.as_mut_ptr(), WB_RELIEF_STRIDE as u32);
    assert_eq!(status, WB_OK, "the canonical preset must be readable");
    record
}

fn hills_record() -> [f64; WB_RELIEF_STRIDE] {
    let mut record = [0.0; WB_RELIEF_STRIDE];
    let status = wb_relief_preset(WB_RELIEF_HILLS, record.as_mut_ptr(), WB_RELIEF_STRIDE as u32);
    assert_eq!(status, WB_OK, "the hills preset must be readable");
    record
}

fn world_with_relief(record: &[f64; WB_RELIEF_STRIDE]) -> u32 {
    wb_world_new_relief(
        SEED,
        RADIUS_M,
        PLATES,
        LAND,
        core::ptr::null(),
        0,
        record.as_ptr(),
        WB_RELIEF_STRIDE as u32,
    )
}

/// Build the world a record asks for, walk every probe point, and free it.
///
/// **This is where an abort would happen, and that is the point of calling it.** The
/// constructor plans the octave schedule; the sampling walks it. Returning the elevations
/// rather than just asserting on them lets a caller compare two records at the same points.
fn sample_relief(record: &[f64; WB_RELIEF_STRIDE], label: &str) -> Vec<f64> {
    let handle = world_with_relief(record);
    assert_ne!(handle, 0, "accepted record refused by the constructor: {label} {record:?}");
    let mut heights = Vec::with_capacity(RELIEF_PROBES.len());
    for (lat, lon) in RELIEF_PROBES {
        let height = wb_elevation_m(handle, *lat, *lon, RES_M);
        assert!(
            height.is_finite(),
            "accepted record produced a non-finite elevation at ({lat}, {lon}): {label} {record:?}",
        );
        heights.push(height);
        // The canonical-resolution path walks EVERY planned band rather than breaking out of
        // the loop early, so it is the one that actually exercises a 41-octave schedule.
        let canonical = wb_elevation_m(handle, *lat, *lon, -1.0);
        assert!(canonical.is_finite(), "non-finite at canonical resolution: {label} {record:?}");
    }
    assert_eq!(wb_world_free(handle), WB_OK);
    heights
}

/// The values a browser can hand across this boundary that are not numbers anybody meant.
///
/// `Number` is an f64, so every one of these is reachable from JS without trying: `NaN` from
/// a `parseFloat` of an empty field, both infinities from an overflow or a division, `-0.0`
/// from `Number("-0")`, the denormals from a slider whose step was computed rather than
/// typed. They are swept against **every one of the ten fields**, not against a chosen few.
const HOSTILE: &[f64] = &[
    f64::NAN,
    -f64::NAN,
    f64::INFINITY,
    f64::NEG_INFINITY,
    0.0,
    -0.0,
    f64::MIN_POSITIVE,
    -f64::MIN_POSITIVE,
    5.0e-324, // the smallest positive denormal
    -5.0e-324,
    f64::EPSILON,
    -f64::EPSILON,
    -1.0,
    1.0,
    1.0e-300,
    -1.0e-300,
    1.0e300,
    -1.0e300,
    f64::MAX,
    -f64::MAX,
];

/// The documented domain of each field, by its index in `WB_RELIEF_STRIDE`'s order.
fn field_domain(field: usize) -> (f64, f64) {
    match field {
        0 | 1 => (WB_MIN_RELIEF_WAVELENGTH_M, WB_MAX_RELIEF_WAVELENGTH_M),
        2..=6 => (0.0, WB_MAX_RELIEF_AMPLITUDE_M),
        7 => (-WB_MAX_QUIETING_STRENGTH, WB_MAX_QUIETING_STRENGTH),
        8 => (WB_MIN_QUIETING_SCALE_M, WB_MAX_QUIETING_SCALE_M),
        9 => (0.0, 1.0),
        _ => unreachable!("WB_RELIEF_STRIDE is 10"),
    }
}

/// Every value one field is driven through: the hostile set, both documented bounds and the
/// values immediately either side of each, and a ladder across the admissible interval.
///
/// **The ladder is geometric where the field's domain spans orders of magnitude** (the two
/// wavelengths and the quieting scale run 1e-3 to 1e9, where a linear ladder would put its
/// first rung 40 million metres above the floor and never sample the small end at all) and
/// linear where it does not. That is the "bands, not cliffs" lesson applied to the sweep's
/// own design: an evenly-spaced sample of a log-scaled domain is a spot-check wearing a
/// sweep's name.
fn field_sweep(field: usize) -> Vec<f64> {
    let (low, high) = field_domain(field);
    let mut values: Vec<f64> = HOSTILE.to_vec();
    for bound in [low, high] {
        values.extend_from_slice(&[
            bound,
            bound - bound.abs() * 1.0e-12,
            bound + bound.abs() * 1.0e-12,
            bound * 0.5,
            bound * 2.0,
            -bound,
        ]);
    }
    let steps = 24;
    let geometric = low > 0.0 && high / low >= 1.0e3;
    for step in 0..=steps {
        let t = f64::from(step) / f64::from(steps);
        values.push(if geometric { low * (high / low).powf(t) } else { low + (high - low) * t });
    }
    values
}

/// Every record the field-by-field sweep produces, labelled by the field it moved.
fn swept_records() -> Vec<(String, [f64; WB_RELIEF_STRIDE])> {
    let base = canonical_record();
    let mut out = Vec::new();
    for field in 0..WB_RELIEF_STRIDE {
        for value in field_sweep(field) {
            let mut record = base;
            record[field] = value;
            out.push((format!("field {field} = {value:e}"), record));
        }
    }
    out
}

#[test]
fn every_relief_field_swept_across_its_whole_range_and_beyond_never_aborts() {
    let records = swept_records();
    // A sweep that refused everything would pass a "nothing aborted" assertion trivially, and
    // one that accepted everything would prove the validator absent. Both counts are asserted.
    let mut accepted = 0usize;
    let mut refused = 0usize;
    for (label, record) in &records {
        if wb_relief_check(record.as_ptr(), WB_RELIEF_STRIDE as u32) == WB_OK {
            sample_relief(record, label);
            accepted += 1;
        } else {
            refused += 1;
        }
    }
    // The split is stated as a floor rather than an equality so adding a value to HOSTILE
    // does not force this line to be re-derived, but both sides have to be substantial or the
    // sweep is not sweeping.
    assert_eq!(accepted + refused, records.len());
    assert!(
        accepted >= 200,
        "only {accepted} records were accepted; the sweep is not exercising the engine",
    );
    assert!(
        refused >= 100,
        "only {refused} records were refused; the validator is not doing its job",
    );
}

#[test]
fn the_relief_checker_and_the_constructor_agree_on_every_swept_record() {
    // Two validators would be two chances to disagree, and the disagreement that matters is
    // "the checker said yes and the constructor aborted". They are held to each other here
    // over the identical population the sweep above uses.
    for (label, record) in swept_records() {
        let status = wb_relief_check(record.as_ptr(), WB_RELIEF_STRIDE as u32);
        let handle = world_with_relief(&record);
        if status == WB_OK {
            assert_ne!(handle, 0, "checker accepted, constructor refused: {label}");
            assert_eq!(wb_world_free(handle), WB_OK);
        } else {
            assert_eq!(
                status, WB_ERR_PARAM,
                "a well-formed buffer refused for a buffer reason: {label}",
            );
            assert_eq!(handle, 0, "checker refused, constructor built: {label}");
        }
    }
}

#[test]
fn a_zero_canonical_wavelength_would_hang_plan_and_is_refused_before_it_can() {
    // `Detail::plan` runs `while wavelength >= canonical_wavelength_m { wavelength *= 0.5 }`.
    // At 0.0 that loop never terminates -- halving reaches 0.0 and `0.0 >= 0.0` stays true --
    // and a hung tab has no console message and no stack. Negative values and -0.0 are the
    // same loop; +inf hangs it from the coarse end, since `inf * 0.5` is `inf`.
    //
    // This test can only ever assert the refusal, never demonstrate the hang: a test that
    // entered the loop would not return. That asymmetry is why the bound lives on a constant
    // with the mechanism written out, rather than only in a test.
    let base = canonical_record();
    for hang in [0.0, -0.0, -1.0, -250.0, f64::NEG_INFINITY] {
        let mut record = base;
        record[0] = hang;
        assert_eq!(
            wb_relief_check(record.as_ptr(), WB_RELIEF_STRIDE as u32),
            WB_ERR_PARAM,
            "canonical_wavelength_m = {hang} would not terminate Detail::plan",
        );
        assert_eq!(world_with_relief(&record), 0);
    }
    for hang in [f64::INFINITY, f64::MAX] {
        let mut record = base;
        record[1] = hang;
        assert_eq!(
            wb_relief_check(record.as_ptr(), WB_RELIEF_STRIDE as u32),
            WB_ERR_PARAM,
            "coarsest_wavelength_m = {hang} would not descend to the canonical band",
        );
        assert_eq!(world_with_relief(&record), 0);
    }
    // The ordering rule itself: a coarsest band finer than the canonical one plans zero
    // octaves, which is a world with the roughness silently switched off.
    let mut inverted = base;
    inverted[0] = 20_000.0;
    inverted[1] = 250.0;
    assert_eq!(wb_relief_check(inverted.as_ptr(), WB_RELIEF_STRIDE as u32), WB_ERR_PARAM);
    assert_eq!(world_with_relief(&inverted), 0);
}

/// The exact slider travel Task 2's tables calibrated, at every step the widget can produce.
///
/// `mountain_m` 150 -> 600 in steps of 10 (46 positions), `quieting_strength` +0.7 -> -0.7 in
/// steps of 0.05 (29 positions, and **zero is one of them**, which is the point -- it is the
/// setting at which the quieting term is off), `octave_persistence` 0.50 -> 0.75 in steps of
/// 0.01 (26 positions). These are the panel's own `step` attributes, so this test sweeps
/// every value a person dragging the slider can actually land on. Both ends of the first two
/// come from the engine's own presets rather than from literals here.
fn slider_travel() -> Vec<(usize, Vec<f64>)> {
    let canonical = canonical_record();
    let hills = hills_record();
    let mut mountain = Vec::new();
    let mut step = 0;
    while canonical[6] + f64::from(step) * 10.0 <= hills[6] {
        mountain.push(canonical[6] + f64::from(step) * 10.0);
        step += 1;
    }
    // **Not `0.7 - i * 0.05`.** That is the obvious spelling and it never lands on zero: the
    // fourteenth step comes out at `-1.1e-16`, not `0.0`, so a slider built that way could
    // not switch the quieting term off -- the one setting on this axis with a stated meaning.
    // Found by the assertion below rather than by inspection. The panel's widget is an
    // integer slider mapped through this same expression, so the two produce the same 29
    // values and neither has to trust the other.
    let mut quieting = Vec::new();
    for i in 0..=28 {
        quieting.push(canonical[7] * f64::from(14 - i) / 14.0);
    }
    let mut persistence = Vec::new();
    for i in 0..=25 {
        persistence.push(canonical[9] + f64::from(i) * 0.01);
    }
    vec![(6, mountain), (7, quieting), (9, persistence)]
}

#[test]
fn the_calibrated_slider_travel_is_swept_at_every_step_the_widget_can_produce() {
    let base = canonical_record();
    let travel = slider_travel();
    assert_eq!(travel[0].1.len(), 46, "mountain_m travel");
    assert_eq!(travel[1].1.len(), 29, "quieting_strength travel");
    assert_eq!(travel[2].1.len(), 26, "octave_persistence travel");
    // Zero has to be landable exactly, not approached: it is the setting at which the term is
    // off, and a slider that can only get within 1e-17 of it cannot say so.
    assert!(travel[1].1.iter().any(|v| *v == 0.0), "the quieting slider must land on exactly 0.0");
    for (field, values) in travel {
        for value in values {
            let mut record = base;
            record[field] = value;
            assert_eq!(
                wb_relief_check(record.as_ptr(), WB_RELIEF_STRIDE as u32),
                WB_OK,
                "the panel can produce field {field} = {value}, and the engine refuses it",
            );
            sample_relief(&record, &format!("slider field {field} = {value}"));
        }
    }
}

#[test]
fn the_three_exposed_parameters_are_swept_together_not_one_at_a_time() {
    // Task 2's finding 3: the three parameters compound multiplicatively rather than adding.
    // A one-axis-at-a-time sweep therefore never visits the corner where they multiply, which
    // is exactly where a band-shaped failure would live. 6 x 5 x 5 = 150 combinations,
    // spanning each slider end to end.
    let base = canonical_record();
    let hills = hills_record();
    let mut built = 0usize;
    for m in 0..6 {
        for q in 0..5 {
            for p in 0..5 {
                let mut record = base;
                record[6] = base[6] + (hills[6] - base[6]) * f64::from(m) / 5.0;
                record[7] = base[7] + (hills[7] - base[7]) * f64::from(q) / 4.0;
                record[9] = base[9] + 0.25 * f64::from(p) / 4.0;
                // The quieting expression above walks +0.7 -> -0.7 by construction; assert it
                // rather than trusting the arithmetic, since a sweep that silently covered
                // half its axis would still pass every other assertion here.
                assert!(record[7] >= -0.7001 && record[7] <= 0.7001, "quieting axis: {}", record[7]);
                assert_eq!(wb_relief_check(record.as_ptr(), WB_RELIEF_STRIDE as u32), WB_OK);
                sample_relief(&record, "cross product");
                built += 1;
            }
        }
    }
    assert_eq!(built, 150);
}

#[test]
fn the_relief_channel_default_path_is_the_untouched_world() {
    // RULING 1. `worldbuilder/terrain/detail.py` is the conformance oracle for 157 tests and
    // no default moves. The viewer's untouched path is a null pointer with a length of zero,
    // and this proves it is the same planet as `wb_world_new`'s -- bit for bit at the probe
    // points, not "close enough".
    let plain = plain_world();
    let defaulted = wb_world_new_relief(
        SEED,
        RADIUS_M,
        PLATES,
        LAND,
        core::ptr::null(),
        0,
        core::ptr::null(),
        0,
    );
    assert_ne!(defaulted, 0);
    let explicit_canonical = world_with_relief(&canonical_record());
    assert_ne!(explicit_canonical, 0);
    for (lat, lon) in RELIEF_PROBES {
        for resolution in [RES_M, -1.0] {
            let reference = wb_elevation_m(plain, *lat, *lon, resolution);
            assert_eq!(
                wb_elevation_m(defaulted, *lat, *lon, resolution).to_bits(),
                reference.to_bits(),
                "the null relief path moved the world at ({lat}, {lon})",
            );
            assert_eq!(
                wb_elevation_m(explicit_canonical, *lat, *lon, resolution).to_bits(),
                reference.to_bits(),
                "Some(canonical()) is not None at ({lat}, {lon})",
            );
        }
    }
    // And the witnessed value survives the refactor that put both constructors behind one
    // builder -- the single number this whole repository pinned three independent ways.
    assert_eq!(wb_elevation_m(defaulted, 12.0, 34.0, RES_M), WITNESSED_ELEVATION_M);
    assert_eq!(wb_world_free(defaulted), WB_OK);
    assert_eq!(wb_world_free(explicit_canonical), WB_OK);
}

#[test]
fn wb_relief_preset_hands_back_the_engines_own_presets_and_nothing_else() {
    use worldbuilder_engine::detail::ReliefParams;
    // The preset export is the reason no host has to transcribe `600.0`, `-0.7` and `0.65`.
    // It is held to `detail.rs`'s own values field by field, so a retune there fails here
    // rather than leaving a viewer quietly showing the old numbers.
    for (selector, expected) in
        [(WB_RELIEF_CANONICAL, ReliefParams::canonical()), (WB_RELIEF_HILLS, ReliefParams::hills())]
    {
        let mut record = [0.0; WB_RELIEF_STRIDE];
        assert_eq!(wb_relief_preset(selector, record.as_mut_ptr(), WB_RELIEF_STRIDE as u32), WB_OK);
        assert_eq!(record[0], expected.canonical_wavelength_m);
        assert_eq!(record[1], expected.coarsest_wavelength_m);
        assert_eq!(record[2], expected.abyssal_m);
        assert_eq!(record[3], expected.shelf_m);
        assert_eq!(record[4], expected.coast_m);
        assert_eq!(record[5], expected.interior_m);
        assert_eq!(record[6], expected.mountain_m);
        assert_eq!(record[7], expected.quieting_strength);
        assert_eq!(record[8], expected.quieting_scale_m);
        assert_eq!(record[9], expected.octave_persistence);
        // Every preset this build offers must also be one the boundary accepts, or the panel
        // could offer a button that refuses itself.
        assert_eq!(wb_relief_check(record.as_ptr(), WB_RELIEF_STRIDE as u32), WB_OK);
    }
    // A preset that round-trips has to also *do* something: hills must not be canonical.
    let canonical = canonical_record();
    let hills = hills_record();
    assert_ne!(canonical, hills);
    let plain_heights = sample_relief(&canonical, "canonical");
    let hills_heights = sample_relief(&hills, "hills");
    assert!(
        plain_heights.iter().zip(&hills_heights).any(|(a, b)| a != b),
        "the hills preset produced the canonical world at every probe",
    );
    // Unknown selectors are refused rather than silently answered with canonical, which would
    // be a viewer showing "hills" and rendering today's world.
    let mut scratch = [0.0; WB_RELIEF_STRIDE];
    for unknown in [2u32, 3, u32::MAX] {
        assert_eq!(
            wb_relief_preset(unknown, scratch.as_mut_ptr(), WB_RELIEF_STRIDE as u32),
            WB_ERR_PARAM,
        );
    }
}

#[test]
fn the_relief_buffer_channel_refuses_what_it_cannot_read() {
    let record = canonical_record();
    // A null pointer with a length: the host computed a length and forgot the buffer.
    assert_eq!(wb_relief_check(core::ptr::null(), WB_RELIEF_STRIDE as u32), WB_ERR_BUFFER);
    // A buffer with a length of zero: the host has a buffer and computed the length wrong.
    // This is NOT read as "canonical" -- answering it with a different world than the caller
    // described is the silently-dropping-builder shape.
    assert_eq!(wb_relief_check(record.as_ptr(), 0), WB_ERR_BUFFER);
    // Wrong lengths, either side. A short buffer read as ten f64 would read past the end.
    for length in [1u32, 9, 11, 20, u32::MAX] {
        assert_eq!(
            wb_relief_check(record.as_ptr(), length),
            WB_ERR_BUFFER,
            "a {length}-word relief record is not a relief record",
        );
    }
    // Misaligned. Refused on the address, BEFORE any slice is formed over it -- forming one
    // would be undefined behaviour rather than a status code.
    let bytes = [0u8; WB_RELIEF_STRIDE * 8 + 8];
    let base = bytes.as_ptr() as usize; // cast-ok: a pointer to an integer to construct a deliberately odd address
    let misaligned = ((base | 1) as *const u8) as *const f64; // cast-ok: an integer back to a pointer, never dereferenced
    assert_eq!(wb_relief_check(misaligned, WB_RELIEF_STRIDE as u32), WB_ERR_BUFFER);
    // The same three refusals through the constructor, which answers with a handle rather
    // than a status.
    assert_eq!(
        wb_world_new_relief(
            SEED,
            RADIUS_M,
            PLATES,
            LAND,
            core::ptr::null(),
            0,
            core::ptr::null(),
            WB_RELIEF_STRIDE as u32,
        ),
        0,
    );
    assert_eq!(
        wb_world_new_relief(SEED, RADIUS_M, PLATES, LAND, core::ptr::null(), 0, record.as_ptr(), 0),
        0,
    );
    assert_eq!(
        wb_world_new_relief(
            SEED,
            RADIUS_M,
            PLATES,
            LAND,
            core::ptr::null(),
            0,
            misaligned,
            WB_RELIEF_STRIDE as u32,
        ),
        0,
    );
    // And the preset writer's own buffer checks.
    let mut out = [0.0; WB_RELIEF_STRIDE];
    assert_eq!(
        wb_relief_preset(WB_RELIEF_CANONICAL, core::ptr::null_mut(), WB_RELIEF_STRIDE as u32),
        WB_ERR_BUFFER,
    );
    assert_eq!(wb_relief_preset(WB_RELIEF_CANONICAL, out.as_mut_ptr(), 9), WB_ERR_BUFFER);
    assert_eq!(wb_relief_preset(WB_RELIEF_CANONICAL, out.as_mut_ptr(), 11), WB_ERR_BUFFER);
    assert_eq!(out, [0.0; WB_RELIEF_STRIDE], "a refused preset call must write nothing");
    // A refused world is not a leaked world: nothing above should have taken a slot.
    let before = wb_world_count();
    assert_eq!(
        wb_world_new_relief(SEED, RADIUS_M, PLATES, LAND, core::ptr::null(), 0, record.as_ptr(), 3),
        0,
    );
    assert_eq!(wb_world_count(), before);
}

// ---- the water channel (slice 5b Task 5) -------------------------------------------------
//
// `wb_water_run` is the first export that reaches `water.rs` at all. Before it existed, that
// module's native-against-WASM agreement was unfalsifiable for exactly the reason
// `wb_erosion_run`'s own doc gives about `erosion.rs`: nothing in the export surface touched
// it, so the parity harness could not compare a single value it produced.
//
// The sweeps below are sweeps rather than spot-checks because `extern "C"` is nounwind and
// **two** of `water.rs`'s no-rim panics are reachable from this export's own parameters. Both
// were found here by sweeping, not by reasoning; see `a_planet_with_no_outlet_...` for the
// second one's measured trace.

/// One water run through the export, sized generously so the count-only query is not needed.
/// `node_count * WB_WATER_BODY_STRIDE` is always enough: no body holds fewer than one node.
fn water_run(handle: u32, node_count: u32, sea_level_m: f64, pond_max_m2: f64) -> (u32, Vec<f64>, f64) {
    let mut rows = vec![0.0f64; node_count as usize * WB_WATER_BODY_STRIDE];
    let capacity = rows.len() as u32;
    let mut body_count: u32 = 0;
    let mut sea_out: f64 = 0.0;
    let status = wb_water_run(
        handle,
        node_count,
        sea_level_m,
        pond_max_m2,
        rows.as_mut_ptr(),
        capacity,
        &mut body_count,
        &mut sea_out,
    );
    rows.truncate(body_count as usize * WB_WATER_BODY_STRIDE);
    (status, rows, sea_out)
}

/// Every property a manifest row must satisfy, asserted on every accepted record of every
/// sweep -- because a bad water record does not fail in the export's domain checks, it fails
/// (or worse, silently succeeds) in what it writes.
fn assert_rows_are_a_manifest(rows: &[f64], sea_level_m: f64, sea_out: f64, label: &str) {
    assert_eq!(sea_out.to_bits(), sea_level_m.to_bits(), "{label}: the datum was not echoed back");
    assert_eq!(rows.len() % WB_WATER_BODY_STRIDE, 0, "{label}: a partial row");
    let mut previous_root = -1.0f64;
    for row in rows.chunks_exact(WB_WATER_BODY_STRIDE) {
        assert!(row[0] > previous_root, "{label}: rows must ascend by root_node, strictly");
        previous_root = row[0];
        assert!(
            row[1] == WB_BODY_KIND_LAKE || row[1] == WB_BODY_KIND_POND,
            "{label}: kind {} is neither code",
            row[1],
        );
        assert!(row[2].is_finite(), "{label}: a non-finite surface level");
        assert!(row[3] >= -90.0 && row[3] <= 90.0, "{label}: min latitude {} off the sphere", row[3]);
        assert!(row[4] >= -90.0 && row[4] <= 90.0, "{label}: max latitude {} off the sphere", row[4]);
        assert!(row[3] <= row[4], "{label}: latitude bounds inverted");
        // Longitude may run min > max: that is `Extent`'s documented antimeridian wrap, not
        // an invalid box, and 6 of 171 bodies at n=30,000 already take it (Task 4's own
        // measurement). So only the range is asserted, never the ordering.
        for k in [5usize, 6] {
            assert!(row[k] >= -180.0 && row[k] <= 180.0, "{label}: longitude {} off the circle", row[k]);
        }
    }
}

/// The hostile set every numeric field of every sweep is driven through, plus a geometric
/// ladder in both signs and a linear one across the band a real datum lives in. An evenly
/// spaced sample of a domain that spans orders of magnitude is a spot-check wearing a
/// sweep's name -- the same reasoning the relief sweep records for its own ladders.
fn sea_level_sweep() -> Vec<f64> {
    let mut values = vec![
        0.0,
        -0.0,
        f64::NAN,
        -f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::MIN_POSITIVE,
        -f64::MIN_POSITIVE,
        5e-324,
        -5e-324,
        f64::EPSILON,
        -f64::EPSILON,
        f64::MAX,
        f64::MIN,
        RADIUS_M,
        -RADIUS_M,
        RADIUS_M + RADIUS_M * 1e-12,
        -RADIUS_M - RADIUS_M * 1e-12,
    ];
    for exponent in -6..=9 {
        let magnitude = 10f64.powi(exponent);
        values.push(magnitude);
        values.push(-magnitude);
    }
    for step in -60..=60 {
        values.push(f64::from(step) * 250.0);
    }
    values
}

#[test]
fn every_water_parameter_is_swept_across_its_whole_range_and_beyond_and_never_aborts() {
    // 3,000 nodes: the same size `wb_erosion_run`'s parity corpus uses, and large enough
    // that the graph has real basins (13 bodies at the datum) while a 171-record sweep still
    // runs in seconds. Every accepted record is *read back*, not merely built: the relief
    // sweep's sharpest lesson was that a NaN-permissive validator passed every
    // construction-only assertion and was caught only by sampling the world it admitted.
    const NODES: u32 = 3_000;
    let world = wb_world_new(SEED, RADIUS_M, PLATES, LAND, core::ptr::null(), 0);
    assert!(world != 0);

    let mut accepted = 0usize;
    let mut refused = 0usize;
    for sea_level_m in sea_level_sweep() {
        let (status, rows, sea_out) = water_run(world, NODES, sea_level_m, 1.0e5);
        match status {
            WB_OK => {
                accepted += 1;
                assert_rows_are_a_manifest(&rows, sea_level_m, sea_out, &format!("sea {sea_level_m}"));
            }
            WB_ERR_PARAM | WB_ERR_GRAPH => refused += 1,
            other => panic!("sea {sea_level_m}: unexpected status {other}"),
        }
    }
    assert!(accepted > 0 && refused > 0, "a sweep that accepts everything or nothing is not a sweep");

    // The threshold is only ever the right-hand side of a `<=`, so its own hostile set is
    // small -- but a NaN there would classify every body as a lake in silence, which is the
    // silently-dropping-builder shape, so it is refused and the refusal is asserted.
    for pond_max_m2 in
        [0.0, -0.0, 5e-324, 1.0e5, 1.0e10, 1.0e20, f64::MAX, f64::MIN_POSITIVE]
    {
        let (status, rows, sea_out) = water_run(world, NODES, 0.0, pond_max_m2);
        assert_eq!(status, WB_OK, "pond threshold {pond_max_m2} should be admissible");
        assert_rows_are_a_manifest(&rows, 0.0, sea_out, &format!("pond {pond_max_m2}"));
    }
    for pond_max_m2 in [f64::NAN, -f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, -1.0e-300] {
        let (status, _, _) = water_run(world, NODES, 0.0, pond_max_m2);
        assert_eq!(status, WB_ERR_PARAM, "pond threshold {pond_max_m2} should be refused");
    }

    // The node-count ladder, both ends and every power of two between. `2` is refused for a
    // measured reason (see the no-outlet test); the ceiling and one past it are the domain.
    for node_count in [2u32, 3, 4, 5, 8, 16, 32, 64, 128, 256, 1_000, 3_000] {
        let (status, rows, sea_out) = water_run(world, node_count, 0.0, 1.0e5);
        assert!(
            status == WB_OK || status == WB_ERR_GRAPH,
            "node_count {node_count}: unexpected status {status}",
        );
        if status == WB_OK {
            assert_rows_are_a_manifest(&rows, 0.0, sea_out, &format!("n {node_count}"));
        }
    }
    for node_count in [0u32, 1, WB_MAX_WATER_NODES + 1, u32::MAX] {
        let (status, _, _) = water_run(world, node_count.min(4), 0.0, 1.0e5);
        let _ = status; // the tiny stand-in above is only to keep the allocation small
        let mut count: u32 = 0;
        let mut sea_out: f64 = 0.0;
        let refusal = wb_water_run(
            world,
            node_count,
            0.0,
            1.0e5,
            core::ptr::null_mut(),
            0,
            &mut count,
            &mut sea_out,
        );
        assert_eq!(refusal, WB_ERR_PARAM, "node_count {node_count} should be refused");
    }

    assert_eq!(wb_world_free(world), WB_OK);
}

#[test]
fn a_planet_with_no_outlet_would_panic_in_water_rs_and_is_refused_before_it_can() {
    // THE ABORT THIS EXPORT'S SWEEP FOUND, and it is the second no-rim panic in `water.rs`
    // rather than the obvious first one. At `sea_level_m` below the world's own lowest
    // sampled point there is no BOUNDARY node, so no mouth, so every basin is a lake and
    // their union is the entire graph. Measured before `every_component_has_an_outlet`
    // existed, this host, native release, seed 20260904, n = 3,000, sea = -1.0e4:
    //
    //   merge_tied_plateaus: the union of 157 lakes ([2423, 2279, ...]) has no rim
    //   thread caused non-unwinding panic. aborting.
    //   exit code: 0xc0000409 (STATUS_STACK_BUFFER_OVERRUN)
    //
    // In wasm that is a dead module and a blank viewer. **A band, not a cliff**, which is
    // why the ladder below brackets it from both sides rather than checking one value: the
    // transition on this world sits at the minimum sampled elevation, bisected to
    // -5698.763334509833 m, and every datum above it is fine.
    const NODES: u32 = 3_000;
    let world = wb_world_new(SEED, RADIUS_M, PLATES, LAND, core::ptr::null(), 0);
    assert!(world != 0);

    for sea_level_m in [0.0, -1.0, -1.0e2, -1.0e3, -5.0e3, -5_698.0] {
        let (status, _, _) = water_run(world, NODES, sea_level_m, 1.0e5);
        assert_eq!(status, WB_OK, "sea {sea_level_m} is above the floor and must be accepted");
    }
    for sea_level_m in [-5_699.0, -1.0e4, -1.0e5, -1.0e6, -RADIUS_M] {
        let (status, _, _) = water_run(world, NODES, sea_level_m, 1.0e5);
        assert_eq!(status, WB_ERR_GRAPH, "sea {sea_level_m} leaves the planet no outlet");
    }

    // Two nodes are one basin covering everything -- `fill_lakes`' own no-rim panic, the
    // first of the two, reached without any unusual datum at all.
    let (status, _, _) = water_run(world, 2, 0.0, 1.0e5);
    assert_eq!(status, WB_ERR_GRAPH, "a two-node world is one rimless basin");

    assert_eq!(wb_world_free(world), WB_OK);
}

#[test]
fn the_pond_threshold_moves_kind_and_nothing_else() {
    // The property `parity.mjs --mutate water-pond` rests on, asserted here so the control's
    // claim is not carried only by the harness that makes it. `pond_max_surface_area_m2`
    // reaches exactly one field: `classify_lake_kinds` compares it against a summed surface
    // area and writes `LakeKind`. If it ever reached a level, an extent or the body
    // partition, the control would be measuring something other than what it says.
    const NODES: u32 = 10_000;
    let world = wb_world_new(SEED, RADIUS_M, PLATES, LAND, core::ptr::null(), 0);
    assert!(world != 0);

    let (status, base, _) = water_run(world, NODES, 0.0, 1.0e5);
    assert_eq!(status, WB_OK);
    let bodies = base.len() / WB_WATER_BODY_STRIDE;
    assert_eq!(bodies, 56, "the corpus world's body count at n=10,000 moved");
    assert_eq!(
        base.chunks_exact(WB_WATER_BODY_STRIDE).filter(|r| r[1] == WB_BODY_KIND_POND).count(),
        0,
        "Task 3's calibrated 1.0e5 threshold produces no ponds on this mesh -- a recorded \
         finding about the mesh, not a threshold to tune until something falls on each side",
    );

    // 5.0e10 m^2 splits this population; 1.0e5 does not, and neither would a value chosen to
    // move everything. A control that moves all of them says as little as one that moves none.
    let (status, moved, _) = water_run(world, NODES, 0.0, 5.0e10);
    assert_eq!(status, WB_OK);
    assert_eq!(moved.len(), base.len(), "the body count must not move with the threshold");
    let ponds = moved.chunks_exact(WB_WATER_BODY_STRIDE).filter(|r| r[1] == WB_BODY_KIND_POND).count();
    assert_eq!(ponds, 9, "9 of 56 bodies at n=10,000, seed 20260904, threshold 5.0e10 m^2");
    assert!(ponds > 0 && ponds < bodies, "neither none nor all");

    let mut kind_moved = 0usize;
    for (a, b) in base.chunks_exact(WB_WATER_BODY_STRIDE).zip(moved.chunks_exact(WB_WATER_BODY_STRIDE))
    {
        for field in [0usize, 2, 3, 4, 5, 6] {
            assert_eq!(
                a[field].to_bits(),
                b[field].to_bits(),
                "field {field} moved with the pond threshold, which reaches only `kind`",
            );
        }
        if a[1].to_bits() != b[1].to_bits() {
            kind_moved += 1;
        }
    }
    assert_eq!(kind_moved, ponds, "every moved field is a kind, and every pond is a moved field");

    assert_eq!(wb_world_free(world), WB_OK);
}

#[test]
fn the_water_buffer_channel_refuses_what_it_cannot_read() {
    const NODES: u32 = 3_000;
    let world = wb_world_new(SEED, RADIUS_M, PLATES, LAND, core::ptr::null(), 0);
    assert!(world != 0);
    let mut count: u32 = 0;
    let mut sea_out: f64 = 0.0;

    // The count-only query: null with a length of zero writes the two scalars and no row.
    assert_eq!(
        wb_water_run(world, NODES, 0.0, 1.0e5, core::ptr::null_mut(), 0, &mut count, &mut sea_out),
        WB_OK,
    );
    let expected_bodies = count;
    assert!(expected_bodies > 0, "the fixture world must have bodies for this test to say anything");
    assert_eq!(sea_out.to_bits(), 0.0f64.to_bits());

    // A null pointer *with* a length is a host that computed a length wrong, not a host
    // asking for the count -- the distinction `read_relief` already draws for relief.
    assert_eq!(
        wb_water_run(world, NODES, 0.0, 1.0e5, core::ptr::null_mut(), 8, &mut count, &mut sea_out),
        WB_ERR_BUFFER,
    );

    // A buffer one element short is refused and writes NOTHING -- all-or-nothing, which is
    // why the count-only query exists at all.
    let needed = expected_bodies as usize * WB_WATER_BODY_STRIDE;
    let mut short = vec![f64::NAN; needed - 1];
    let mut untouched_count = u32::MAX;
    let mut untouched_sea = f64::NAN;
    assert_eq!(
        wb_water_run(
            world,
            NODES,
            0.0,
            1.0e5,
            short.as_mut_ptr(),
            (needed - 1) as u32,
            &mut untouched_count,
            &mut untouched_sea,
        ),
        WB_ERR_BUFFER,
    );
    assert!(short.iter().all(|v| v.is_nan()), "a refusal wrote into the row buffer");
    assert_eq!(untouched_count, u32::MAX, "a refusal wrote the count");
    assert!(untouched_sea.is_nan(), "a refusal wrote the datum");

    // Exactly enough is enough.
    let mut exact = vec![0.0f64; needed];
    assert_eq!(
        wb_water_run(
            world,
            NODES,
            0.0,
            1.0e5,
            exact.as_mut_ptr(),
            needed as u32,
            &mut count,
            &mut sea_out,
        ),
        WB_OK,
    );

    // A deliberately misaligned address, refused on the address itself -- before any slice is
    // formed over it, because forming one would be UB rather than a status.
    let mut bytes = vec![0u8; needed * 8 + 8];
    let misaligned = unsafe { bytes.as_mut_ptr().add(1) } as *mut f64;
    assert_eq!(
        wb_water_run(world, NODES, 0.0, 1.0e5, misaligned, needed as u32, &mut count, &mut sea_out),
        WB_ERR_BUFFER,
    );
    let misaligned_u32 = unsafe { bytes.as_mut_ptr().add(1) } as *mut u32;
    assert_eq!(
        wb_water_run(
            world,
            NODES,
            0.0,
            1.0e5,
            exact.as_mut_ptr(),
            needed as u32,
            misaligned_u32,
            &mut sea_out,
        ),
        WB_ERR_BUFFER,
    );
    assert_eq!(
        wb_water_run(
            world,
            NODES,
            0.0,
            1.0e5,
            exact.as_mut_ptr(),
            needed as u32,
            &mut count,
            core::ptr::null_mut(),
        ),
        WB_ERR_BUFFER,
    );
    assert_eq!(
        wb_water_run(
            world,
            NODES,
            0.0,
            1.0e5,
            exact.as_mut_ptr(),
            needed as u32,
            core::ptr::null_mut(),
            &mut sea_out,
        ),
        WB_ERR_BUFFER,
    );

    // An unknown handle, and a freed one.
    assert_eq!(
        wb_water_run(u32::MAX, NODES, 0.0, 1.0e5, core::ptr::null_mut(), 0, &mut count, &mut sea_out),
        WB_ERR_HANDLE,
    );
    assert_eq!(wb_world_free(world), WB_OK);
    assert_eq!(
        wb_water_run(world, NODES, 0.0, 1.0e5, core::ptr::null_mut(), 0, &mut count, &mut sea_out),
        WB_ERR_HANDLE,
    );
    assert_eq!(wb_world_count(), 0, "a refused water run must not leak a world");
}

#[test]
fn the_water_manifest_never_enumerates_the_sea() {
    // Slice 5b's Ruling 6, held at the export rather than only in `water.rs`: the sea is the
    // mapping's miss, not a row in it. Task 4 measured what enumerating it cost -- 1,061 of
    // 1,232 rows were ocean, 96.3% of ocean boxes overlapped another, and 61 of 171 lakes had
    // boxes hit by one -- so a regression here would not look like an error, it would look
    // like a manifest with eight times as many entries and no way to tell them apart.
    //
    // The property with teeth: every row's `level_m` is strictly above the datum. A mouth
    // sits at or below it by construction (`StreamGraph::build` flags a node BOUNDARY on
    // `height_m > sea_level_m`), so an ocean row could not satisfy this, and the assertion
    // fails on the shape of the defect rather than on a count that a different world moves.
    const NODES: u32 = 3_000;
    let world = wb_world_new(SEED, RADIUS_M, PLATES, LAND, core::ptr::null(), 0);
    assert!(world != 0);
    for sea_level_m in [0.0, -1.0e3, 1.0e2, 5.0e2] {
        let (status, rows, _) = water_run(world, NODES, sea_level_m, 1.0e5);
        assert_eq!(status, WB_OK);
        for row in rows.chunks_exact(WB_WATER_BODY_STRIDE) {
            assert!(
                row[2] > sea_level_m,
                "a body at level {} is at or below the datum {sea_level_m} -- that is the sea, \
                 and the sea is never a row",
                row[2],
            );
        }
    }
    assert_eq!(wb_world_free(world), WB_OK);
}
