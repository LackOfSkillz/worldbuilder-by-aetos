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

use worldbuilder_engine::continentality::{CoastParams, Continentality};
use worldbuilder_engine::features::{Feature, Features, CARVE, RAISE};
use worldbuilder_engine::sphere::SpherePoint;
use worldbuilder_engine::surface::{FeatureInput, Surface};
// The three range constants the tectonic width ceilings are DERIVED from. Imported rather
// than written down again, so the test asserting `WB_MAX_COASTAL_UPLIFT_WIDTH_M` equals
// `MAX_TECTONIC_RANGE_M - COASTAL_UPLIFT_OFFSET_M` compares the boundary against the engine
// and not against a third copy of two numbers.
use worldbuilder_engine::tectonics::{
    COASTAL_UPLIFT_OFFSET_M, ISLAND_ARC_OFFSET_M, MAX_TECTONIC_RANGE_M,
};
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
    // **The constructor is `Surface::with_gully`, and EVERY name is asserted.** The one call
    // has now moved twice -- to `with_coast` when the coast channel opened, and to `with_gully`
    // when the gully channel did. Each time the previous widest door delegates to the new one
    // with a `None`, so the canonical path is the same code either way and the widest door is
    // the only one that reaches the constructor. Counting only the current name would let a
    // second, older-shaped build reappear beside it without this noticing; counting all three is
    // the property this test actually means, which is that `wasm.rs` builds a `Surface` exactly
    // once, anywhere, by any name.
    let builds = code.matches("Surface::with_gully").count();
    let legacy =
        code.matches("Surface::with_coast").count() + code.matches("Surface::new").count();
    assert_eq!(
        builds, 1,
        "wasm.rs builds a Surface {builds} times; a sampling path that rebuilds costs ~10^3x"
    );
    assert_eq!(legacy, 0, "a second Surface constructor appeared beside the one in build_world");
    let before = &code[..code.find("Surface::with_gully").expect("one build")];
    assert!(
        before.contains("fn wb_world_new"),
        "the one Surface::with_gully is not inside the wb_world_new family"
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

// ========================================================================= the tectonic channel
//
// Slice `2026-09-05-slice-mountains`, Task 4 -- the task the owner has asked for by name.
// They asked for two knobs ("1 to raise and lower mountains and one to make more mountains
// and less as desired") and then, twice, said there were still no mountains. Task 1 built the
// `TectonicParams` block and was REQUIRED to change nothing; this channel is how a browser
// reaches one.
//
// The relief section above is the template and the reasons are identical, so they are not
// restated: `extern "C"` is nounwind, a panic here is a dead module and a blank viewer, and
// this project has found **three real aborts and one ~2,600-second hang** by sweeping export
// inputs and **zero** by spot-checking -- every one of them a band rather than a cliff, fine
// on both sides of a bad interior value. So every test below sweeps, every record a sweep
// produces goes through `wb_tectonic_check` AND `wb_world_new_tectonic`, and every record the
// boundary accepts is then actually **sampled**, because a bad tectonic block does not fail
// in the constructor -- `Tectonics::new` only stores it -- it fails, if it fails, the first
// time `from_margin` walks a profile.
//
// Population/method/host for every figure in this section: the world is
// `Surface::new(20_260_904, 6_371_000, 12, 0.29, ..)` -- the same `SEED`/`RADIUS_M`/
// `PLATES`/`LAND` fixture the rest of this file uses. The probe points are `TECTONIC_PROBES`,
// which is the relief channel's six PLUS three witness points measured for this channel --
// see that constant for why the six alone were not a population. The host is a native
// `cargo test -p worldbuilder-engine --features wasm` run.

/// Where every accepted tectonic record is sampled.
///
/// **`RELIEF_PROBES` alone is not a population for this channel, and that is measured rather
/// than suspected.** Those six were chosen to cross the five settings `Detail::amplitude_m`
/// blends between; a tectonic profile is a *margin* effect, and on this fixture not one of the
/// six lies within `MAX_TECTONIC_RANGE_M` of a convergent continental margin. Driving the
/// collision profile from 1,500 m / 400 km to 6,000 m / 100 km changes **not one bit** at any
/// of them -- found by `a_chosen_tectonic_block_actually_moves_the_ground_it_claims_to`
/// failing, which is the fifth time in this project an assertion has looked load-bearing and
/// not been, and the first time one was caught by the test that needed it.
///
/// So the six are kept -- an abort would still be an abort there, and they cover water and
/// shelf, which the three below do not -- and three witness points are added. Each is the
/// **site of the largest change** that one knob makes anywhere on this world, found by
/// `src/bin/mountain_probe.rs::witness_for` over the same 0.5-degree global grid
/// (720 x 359 = 258,480 sites, `elevation_m(point, None)` on two worlds differing in exactly
/// one block, release build, this host):
///
/// | knob | site | canonical | moved | delta |
/// |---|---|---|---|---|
/// | 6,000 m / 100 km | -7.50, 66.00 | 1,886.798 m | 5,616.919 m | **3,730.122 m** |
/// | blend 1.00 (fewer) | -3.00, 69.00 | 1,388.596 m | 354.613 m | **1,033.983 m** |
/// | blend 0.10 (more) | -33.50, -22.00 | -852.556 m | 272.983 m | **1,125.539 m** |
const TECTONIC_PROBES: &[(f64, f64)] = &[
    (12.0, 34.0), // the witnessed point
    (0.0, 0.0),
    (-18.25, 121.5), // the harbour, near a coast
    (62.5, -145.0),
    (-71.0, 25.0),
    (35.0, 138.0),
    (-7.5, 66.0),    // the collision profile's own witness
    (-3.0, 69.0),    // where widening the blend takes the most away
    (-33.5, -22.0),  // where narrowing it adds the most
];

/// A named preset, read across the boundary exactly as the viewer reads it. **Nothing in this
/// file writes a tectonic value down**, canonical or preset: both come from
/// `wb_tectonic_preset`, so `tectonics.rs` stays the one place the numbers live and a test
/// cannot agree with a stale copy of them.
fn tectonic_preset_record(selector: u32) -> [f64; WB_TECTONIC_STRIDE] {
    let mut record = [0.0; WB_TECTONIC_STRIDE];
    let status = wb_tectonic_preset(selector, record.as_mut_ptr(), WB_TECTONIC_STRIDE as u32);
    assert_eq!(status, WB_OK, "tectonic preset {selector} must be readable");
    record
}

fn canonical_tectonic_record() -> [f64; WB_TECTONIC_STRIDE] {
    tectonic_preset_record(WB_TECTONIC_CANONICAL)
}

fn world_with_tectonics(record: &[f64; WB_TECTONIC_STRIDE]) -> u32 {
    wb_world_new_tectonic(
        SEED,
        RADIUS_M,
        PLATES,
        LAND,
        core::ptr::null(),
        0,
        core::ptr::null(),
        0,
        record.as_ptr(),
        WB_TECTONIC_STRIDE as u32,
    )
}

/// Build the world a tectonic record asks for, walk every probe point, and free it.
///
/// **This is where an abort would happen, and that is the point of calling it.** Nothing in
/// `Tectonics::new` touches a field -- it stores the block -- so a constructor that returned a
/// handle has proved nothing at all about the record it was given.
fn sample_tectonics(record: &[f64; WB_TECTONIC_STRIDE], label: &str) -> Vec<f64> {
    let handle = world_with_tectonics(record);
    assert_ne!(handle, 0, "accepted record refused by the constructor: {label} {record:?}");
    let mut heights = Vec::with_capacity(TECTONIC_PROBES.len());
    for (lat, lon) in TECTONIC_PROBES {
        let height = wb_elevation_m(handle, *lat, *lon, RES_M);
        assert!(
            height.is_finite(),
            "accepted record produced a non-finite elevation at ({lat}, {lon}): {label} {record:?}",
        );
        // `structural_m` is the term a tectonic block actually moves -- 98.9% of the owner's
        // peak -- so sampling only `elevation_m` would leave the thing under test half
        // unwatched.
        let structural = wb_structural_m(handle, *lat, *lon);
        assert!(
            structural.is_finite(),
            "accepted record gave a non-finite structural at ({lat}, {lon}): {label} {record:?}",
        );
        heights.push(height);
    }
    assert_eq!(wb_world_free(handle), WB_OK);
    heights
}

/// The documented domain of each tectonic field, by its index in `WB_TECTONIC_STRIDE`'s
/// order. **Each width has its own ceiling**, because each profile sits at its own offset
/// from the margin and therefore reaches a different distance for the same width.
fn tectonic_field_domain(field: usize) -> (f64, f64) {
    match field {
        0 | 2 | 4 | 6 => (-WB_MAX_TECTONIC_AMPLITUDE_M, WB_MAX_TECTONIC_AMPLITUDE_M),
        1 => (WB_MIN_TECTONIC_WIDTH_M, WB_MAX_CENTRED_TECTONIC_WIDTH_M),
        3 => (WB_MIN_TECTONIC_WIDTH_M, WB_MAX_COASTAL_UPLIFT_WIDTH_M),
        5 => (WB_MIN_TECTONIC_WIDTH_M, WB_MAX_ISLAND_ARC_WIDTH_M),
        7 => (WB_MIN_TECTONIC_WIDTH_M, WB_MAX_CENTRED_TECTONIC_WIDTH_M),
        8 => (WB_MIN_CONTINENTAL_BLEND, WB_MAX_CONTINENTAL_BLEND),
        // The five structure fields, Task 3. Three of these ceilings are DERIVED, and they are
        // written here as their derivation rather than as a number, for the same reason the
        // width ceilings above are: a test that restates a bound cannot notice it moving.
        //
        // `collision_asymmetry` has no ceiling constant of its own -- it is bounded by the
        // OVERRIDING FLANK it produces, `continent_collision_width_m / asymmetry`, held against
        // the same floor every width faces. On the canonical 400 km flank that ceiling is
        // 4.0e8, so this ladder runs far past every setting Task 2 measured (1.00 to 3.00) and
        // out into the region where the narrow flank is under a millimetre, which is the point.
        9 => {
            (WB_MIN_COLLISION_ASYMMETRY, WB_MAX_CENTRED_TECTONIC_WIDTH_M / WB_MIN_TECTONIC_WIDTH_M)
        }
        10 => (1.0, f64::from(WB_MAX_SUTURE_COUNT)),
        11 => (WB_MIN_SUTURE_SPREAD_M, MAX_TECTONIC_RANGE_M),
        12 => (0.0, WB_MAX_STRUCTURE_DEPTH),
        13 => (WB_MIN_STRUCTURE_WAVELENGTH_M, WB_MAX_STRUCTURE_WAVELENGTH_M),
        // Task 5's two. `margin_warp_m`'s ceiling is the range gate and is a RESTATEMENT --
        // the binding check is `collision_reach_m`, which adds this amplitude to the suture
        // and flank reach, so most of the top of this ladder is refused by the reach and not
        // by the per-field ceiling. That is the intended shape and the reason the ladder runs
        // to the ceiling anyway: a sweep that stopped where the reach starts refusing would
        // never exercise the interaction it exists to find.
        14 => (WB_MIN_MARGIN_WARP_M, WB_MAX_MARGIN_WARP_M),
        // And this ceiling is NOT the range gate -- see `WB_MAX_MARGIN_WARP_WAVELENGTH_M`.
        // The warp varies ALONG the margin, which runs the whole way round the planet, so the
        // domain spans nine orders of magnitude and the ladder below goes geometric for it.
        15 => (WB_MIN_MARGIN_WARP_WAVELENGTH_M, WB_MAX_MARGIN_WARP_WAVELENGTH_M),
        _ => unreachable!("WB_TECTONIC_STRIDE is 16"),
    }
}

/// Word 10 of a tectonic record: `suture_count`, **the loop bound**.
///
/// Named because three separate places below have to treat it differently from the thirteen
/// f64 fields around it, and a bare `10` in each of them is three chances to mean a different
/// field.
const SUTURE_COUNT_FIELD: usize = 10;
/// Word 11: `suture_spread_m`. A count above one is inadmissible without one.
const SUTURE_SPREAD_FIELD: usize = 11;

/// Every value one tectonic field is driven through: `HOSTILE` in full, both documented
/// bounds and the values immediately either side of each, and a ladder across the admissible
/// interval -- geometric where the domain spans orders of magnitude (the four widths run
/// 1e-3 to ~4e5 and the blend 1e-3 to 1e3, where a linear ladder would put its first rung
/// tens of kilometres above the floor and never sample the small end at all) and linear where
/// it does not. Identical in construction to `field_sweep`, and deliberately so: an
/// evenly-spaced sample of a log-scaled domain is a spot-check wearing a sweep name.
fn tectonic_field_sweep(field: usize) -> Vec<f64> {
    let (low, high) = tectonic_field_domain(field);
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
    // **The loop bound is the one field a ladder sweeps badly, and it is the one field where
    // that matters most.** Its domain is the eight integers 1..=8, and a 25-rung ladder across
    // it is mostly fractions -- which the boundary refuses for being non-integral before the
    // count is ever exercised at all. So every integer either side of both ends is added by
    // hand, along with the values a SATURATING `as u32` would turn into a four-billion-
    // iteration walk: `HOSTILE` already carries `f64::MAX`, `INFINITY` and `1e300`, and
    // `u32::MAX` itself and its neighbours are added here because they are the exact number a
    // saturating cast produces and nothing else in this list is.
    if field == SUTURE_COUNT_FIELD {
        for integer in -2..=12 {
            values.push(f64::from(integer));
        }
        values.extend_from_slice(&[
            f64::from(WB_MAX_SUTURE_COUNT) + 1.0,
            f64::from(u32::MAX),
            f64::from(u32::MAX) - 1.0,
            f64::from(u32::MAX) + 1.0,
            4_294_967_296.0,
            2.5,
            1.5,
            1.0 + f64::EPSILON,
            2.0 - f64::EPSILON,
        ]);
    }
    values
}

/// The two bases every field is swept around.
///
/// **One base is not a sweep of a channel whose fields interact, and Task 2 measured that they
/// do.** Around `canonical()` every structure field is at its inert setting, so a sweep of
/// `structure_wavelength_m` there rides a `structure_depth` of zero and `structure_at` returns
/// before it ever reads the wavelength -- the field would be swept with the code under it
/// switched off, which is a sweep of nothing wearing a sweep's name. Around `ranges()` the
/// structure is on, the sutures are stacked, the warp is on, and the reach is **315 km of the
/// 420 km gate**, so the interaction bands are reachable -- and with only 105 km of headroom
/// left, the `margin_warp_m` ladder crosses the gate INSIDE the admissible per-field range,
/// which is exactly the band a one-base sweep would miss.  Task 5's warp is the second field
/// on this channel that is inert at `canonical()` and therefore invisible to a sweep run only
/// there: `margin_warp_wavelength_m` is never read while the amplitude is zero, the same
/// blindness `structure_wavelength_m` had. Both, therefore: every hazard the ledger records was a
/// **band, not a cliff**, and a band lives where two fields meet.
fn tectonic_sweep_bases() -> [(&'static str, [f64; WB_TECTONIC_STRIDE]); 2] {
    [
        ("canonical", canonical_tectonic_record()),
        ("ranges", tectonic_preset_record(WB_TECTONIC_RANGES)),
    ]
}

fn swept_tectonic_records() -> Vec<(String, [f64; WB_TECTONIC_STRIDE])> {
    let mut out = Vec::new();
    for (base_name, base) in tectonic_sweep_bases() {
        for field in 0..WB_TECTONIC_STRIDE {
            for value in tectonic_field_sweep(field) {
                let mut record = base;
                record[field] = value;
                out.push((format!("{base_name} + tectonic field {field} = {value:e}"), record));
            }
        }
    }
    out
}

#[test]
fn every_tectonic_field_swept_across_its_whole_range_and_beyond_never_aborts() {
    let records = swept_tectonic_records();
    // A sweep that refused everything would pass a "nothing aborted" assertion trivially, and
    // one that accepted everything would prove the validator absent. Both counts are asserted.
    let mut accepted = 0usize;
    let mut refused = 0usize;
    for (label, record) in &records {
        if wb_tectonic_check(record.as_ptr(), WB_TECTONIC_STRIDE as u32) == WB_OK {
            sample_tectonics(record, label);
            accepted += 1;
        } else {
            refused += 1;
        }
    }
    assert_eq!(accepted + refused, records.len());
    assert!(
        accepted >= 180,
        "only {accepted} records were accepted; the sweep is not exercising the engine",
    );
    assert!(
        refused >= 100,
        "only {refused} records were refused; the validator is not doing its job",
    );
}

#[test]
fn the_tectonic_checker_and_the_constructor_agree_on_every_swept_record() {
    // Two validators would be two chances to disagree, and the disagreement that matters is
    // "the checker said yes and the constructor aborted". Held to each other over the
    // identical population the sweep above uses.
    for (label, record) in swept_tectonic_records() {
        let status = wb_tectonic_check(record.as_ptr(), WB_TECTONIC_STRIDE as u32);
        let handle = world_with_tectonics(&record);
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
fn a_zero_width_profile_would_be_a_field_that_does_nothing_and_is_refused() {
    // `tectonics::bump` opens with `if width_m <= 0.0 { return 0.0 }`, which
    // `a_zero_width_bump_is_nothing_rather_than_a_division_by_zero` pins -- so a zero width is
    // NOT a division by zero and NOT an abort. It is worse in the way this project keeps
    // getting bitten by: a parameter present in the record, accepted by the constructor, and
    // contributing exactly nothing at every point on the planet. That is the
    // silently-dropping-builder shape, and this boundary refuses it rather than admitting it.
    //
    // The engine guard and this refusal are checked against each other here rather than
    // assumed to agree: every value below is one `bump` answers with 0.0.
    let base = canonical_tectonic_record();
    for width_field in [1usize, 3, 5, 7] {
        for nothing in [0.0, -0.0, -1.0, -400_000.0, f64::NEG_INFINITY] {
            let mut record = base;
            record[width_field] = nothing;
            assert_eq!(
                wb_tectonic_check(record.as_ptr(), WB_TECTONIC_STRIDE as u32),
                WB_ERR_PARAM,
                "width field {width_field} = {nothing} is a profile that does nothing",
            );
            assert_eq!(world_with_tectonics(&record), 0);
        }
    }
}

#[test]
fn a_width_the_range_gate_would_truncate_is_refused_rather_than_cut_off_mid_fade() {
    // `Tectonics::offset_m` asks `margins_within(point, MAX_TECTONIC_RANGE_M, ..)`, so beyond
    // 420 km a margin is not evaluated at all. A profile still carrying weight there is
    // truncated to zero rather than faded to it -- a cliff -- and `MAX_TECTONIC_RANGE_M`'s own
    // doc says the check belongs at the boundary that admits a caller-supplied width. It is
    // here.
    //
    // The two OFFSET profiles are the ones a single shared ceiling would have got wrong:
    // `bump(across_m - offset, width)` still carries weight out to `offset + width` on the
    // near side, so a coastal width of 400 km reaches 470 km and is refused, while a collision
    // width of 400 km reaches exactly 400 km and is the panel gentlest setting.
    let base = canonical_tectonic_record();

    // Accepted: centred profiles at exactly the gate.
    for centred in [1usize, 7] {
        let mut record = base;
        record[centred] = MAX_TECTONIC_RANGE_M;
        assert_eq!(
            wb_tectonic_check(record.as_ptr(), WB_TECTONIC_STRIDE as u32),
            WB_OK,
            "a centred profile that reaches zero exactly at the gate is inside it",
        );
        sample_tectonics(&record, "centred profile at the gate");
    }

    // Refused: the same width on either offset profile, and past each ceiling.
    let offsets: [(usize, f64); 2] =
        [(3, WB_MAX_COASTAL_UPLIFT_WIDTH_M), (5, WB_MAX_ISLAND_ARC_WIDTH_M)];
    for (field, ceiling) in offsets {
        assert!(ceiling < MAX_TECTONIC_RANGE_M, "an offset profile ceiling is below the gate");
        let mut at = base;
        at[field] = ceiling;
        assert_eq!(wb_tectonic_check(at.as_ptr(), WB_TECTONIC_STRIDE as u32), WB_OK);
        sample_tectonics(&at, "offset profile exactly at its own ceiling");

        for past in [ceiling + 1.0, MAX_TECTONIC_RANGE_M, MAX_TECTONIC_RANGE_M + 1.0] {
            let mut record = base;
            record[field] = past;
            assert_eq!(
                wb_tectonic_check(record.as_ptr(), WB_TECTONIC_STRIDE as u32),
                WB_ERR_PARAM,
                "field {field} = {past} reaches past the range gate and would be truncated",
            );
            assert_eq!(world_with_tectonics(&record), 0);
        }
    }

    // And the ceilings are what they are said to be, derived from the module own constants
    // rather than written down again here.
    assert_eq!(
        WB_MAX_COASTAL_UPLIFT_WIDTH_M.to_bits(),
        (MAX_TECTONIC_RANGE_M - COASTAL_UPLIFT_OFFSET_M).to_bits(),
    );
    assert_eq!(
        WB_MAX_ISLAND_ARC_WIDTH_M.to_bits(),
        (MAX_TECTONIC_RANGE_M - ISLAND_ARC_OFFSET_M).to_bits(),
    );
}

#[test]
fn a_continental_blend_of_zero_or_less_is_refused_because_it_is_the_hard_test_again() {
    // `continental_with` is `(value - CONTINENTAL_ENOUGH) / blend * 0.5 + 0.5`, smoothstepped.
    // At blend 0 the division gives +-inf -- a HARD threshold, which is the exact defect
    // `CONTINENTAL_BLEND`'s own doc records ("the ground jumped five hundred and fifty metres
    // wherever a margin crossed it") -- and at a continentality of exactly zero it gives
    // `0.0 / 0.0`, NaN, which the function `if fraction < 1.0` leaves at 1.0. So a margin
    // would read *thoroughly continental* for the reason that a NaN compares false.
    //
    // Asserted here rather than described: this is the arithmetic, run.
    let nan_fraction = (0.0f64 / 0.0) * 0.5 + 0.5;
    assert!(nan_fraction.is_nan());
    let resolved = if nan_fraction < 1.0 { nan_fraction } else { 1.0 };
    assert_eq!(resolved, 1.0, "a NaN fraction resolves to thoroughly continental");

    let base = canonical_tectonic_record();
    for blend in [0.0, -0.0, -0.45, -1.0, f64::NEG_INFINITY, f64::NAN] {
        let mut record = base;
        record[8] = blend;
        assert_eq!(
            wb_tectonic_check(record.as_ptr(), WB_TECTONIC_STRIDE as u32),
            WB_ERR_PARAM,
            "continental_blend = {blend} is not a transition width",
        );
        assert_eq!(world_with_tectonics(&record), 0);
    }
}

/// The exact slider travel Task 4's tables calibrated, at every step the widget can produce.
///
/// **Both ends of every one of the three are anchored on the engine own canonical record**,
/// read through `wb_tectonic_preset`, so nothing here restates 1500, 400,000 or 0.45. The
/// step sizes and step counts are this task's calibration and are the only numbers written
/// down -- the same posture `PERSISTENCE_STEPS` takes in `relief-params.js`.
///
/// - `continent_collision_m`: canonical up, 100 m a step, 45 steps -> 1,500..6,000 m.
/// - `continent_collision_width_m`: canonical DOWN, 10 km a step, 30 steps -> 400..100 km.
///   Down, because canonical is already the widest setting the range gate allows and every
///   narrower one is steeper.
/// - `continental_blend`: ninths of canonical, from -11 to +7 -> 1.00 down to 0.10. Written as
///   `canonical * (9 - position) / 9` rather than `canonical - position * 0.05` for the reason
///   `quieting_strength` is written as `canonical * n / 14`: both measured ends then land
///   exactly, and position 0 is canonical bit-for-bit.
///
/// **Task 3 adds three more, and every travel is calibrated from Task 2's measured columns --
/// a slider whose useful range is a tenth of its travel is a slider nobody can aim.**
///
/// - `collision_asymmetry`: canonical (1.00, symmetric) up to 3.00, a quarter a step. That is
///   exactly the interval Task 2 swept and found monotone in summit count, grade and flank
///   ratio at all six of its settings, and 2.0 -- the preset's -- lands on the lattice.
/// - `structure_depth`: canonical (0.0) up to 0.9, a tenth a step. Task 2's depth table runs
///   0.0 to 0.9 and stops there because the peak cost keeps rising while the summit count
///   flattens.
/// - `structure_wavelength_m`: canonical (120 km) DOWN to 40 km, 20 km a step -- five
///   positions, of which the last three (80, 60, 40 km) are the measured working band.
///   **The travel deliberately excludes 120-250 km, which Task 2 measured as doing NOTHING**
///   at any depth (summit counts fall back to 0-3 against 12 at 40 km), and it has to start at
///   120 km anyway because that is the canonical placeholder and position 0 must be canonical
///   bit-for-bit or an untouched panel writes a parameter into every shared link. So one of
///   five positions is dead and it is the one Ruling 1 requires; a 40-250 km slider would have
///   been two-thirds dead.
///
/// Written as `position / 4.0` and `position / 10.0` rather than `position * 0.25` and
/// `position * 0.1` for the reason Task 4 found the hard way: `0.1 * 7` is
/// 0.7000000000000001, and the preset's `structure_depth` is 0.7. A preset value that cannot
/// be expressed by the slider it lands on is the panel-default defect this viewer has now
/// shipped four times.
fn tectonic_slider_travel() -> Vec<(usize, Vec<f64>)> {
    let canonical = canonical_tectonic_record();
    let mut height = Vec::new();
    for position in 0..=45 {
        height.push(canonical[0] + f64::from(position) * 100.0);
    }
    let mut width = Vec::new();
    for position in 0..=30 {
        width.push(canonical[1] - f64::from(position) * 10_000.0);
    }
    let mut blend = Vec::new();
    for position in -11..=7 {
        blend.push(canonical[8] * (f64::from(9 - position) / 9.0));
    }
    let mut asymmetry = Vec::new();
    for position in 0..=8 {
        asymmetry.push(canonical[9] + f64::from(position) / 4.0);
    }
    let mut depth = Vec::new();
    for position in 0..=9 {
        depth.push(canonical[12] + f64::from(position) / 10.0);
    }
    let mut wavelength = Vec::new();
    for position in 0..=4 {
        wavelength.push(canonical[13] - f64::from(position) * 20_000.0);
    }
    vec![
        (0, height),
        (1, width),
        (8, blend),
        (9, asymmetry),
        (12, depth),
        (13, wavelength),
    ]
}

#[test]
fn the_calibrated_mountain_slider_travel_is_swept_at_every_step_the_widget_can_produce() {
    let base = canonical_tectonic_record();
    let travel = tectonic_slider_travel();
    assert_eq!(travel[0].1.len(), 46, "mountain height travel");
    assert_eq!(travel[1].1.len(), 31, "mountain width travel");
    assert_eq!(travel[2].1.len(), 19, "mountain count travel");
    assert_eq!(travel[3].1.len(), 9, "asymmetry travel");
    assert_eq!(travel[4].1.len(), 10, "structure depth travel");
    assert_eq!(travel[5].1.len(), 5, "structure wavelength travel");

    // Position 0 is canonical BIT FOR BIT on all three new sliders, for the reason Task 4
    // found by one ULP: `tectonicToParams` drops a field equal to canonical, so a position-0
    // value one ULP off would be written into every shared link and take an untouched viewer
    // off the engine's `None` path. Ruling 1, broken by a rounding mode.
    assert_eq!(travel[3].1[0].to_bits(), base[9].to_bits(), "asymmetry position 0");
    assert_eq!(travel[4].1[0].to_bits(), base[12].to_bits(), "depth position 0");
    assert_eq!(travel[5].1[0].to_bits(), base[13].to_bits(), "wavelength position 0");
    // And the three PRESET values land on the lattice EXACTLY, which is what makes the preset
    // button and the sliders the same control rather than two that nearly agree. `0.1 * 7` is
    // 0.7000000000000001 and would fail this; `7 / 10.0` is 0.7.
    let preset = tectonic_preset_record(WB_TECTONIC_RANGES);
    assert_eq!(travel[3].1[4].to_bits(), preset[9].to_bits(), "asymmetry 2.00 is position 4");
    assert_eq!(travel[4].1[7].to_bits(), preset[12].to_bits(), "depth 0.7 is position 7");
    assert_eq!(travel[5].1[2].to_bits(), preset[13].to_bits(), "wavelength 80 km is position 2");
    // Both measured ends of the three new travels, landed on rather than approached.
    assert_eq!(travel[3].1[8], 3.0, "the asymmetry sweep's far end");
    assert_eq!(travel[4].1[9], 0.9, "the depth table's far end");
    assert_eq!(travel[5].1[4], 40_000.0, "the working band's short end");

    // The measured ends, landed on exactly rather than approached. 6,000 m over 100 km is the
    // 7.030% grade the probe measured on the owner's world; real ranges run 3-8%, and today's
    // 1,500 m over 400 km is 1.787%.
    assert_eq!(travel[0].1[0].to_bits(), base[0].to_bits(), "height position 0 is canonical");
    assert_eq!(travel[0].1[45], 6_000.0);
    assert_eq!(travel[1].1[0].to_bits(), base[1].to_bits(), "width position 0 is canonical");
    assert_eq!(travel[1].1[30], 100_000.0);
    // Position 0 of the blend travel is its TWELFTH entry: the slider runs -11..+7.
    assert_eq!(travel[2].1[11].to_bits(), base[8].to_bits(), "blend position 0 is canonical");
    // The two ENDS are 1.00 and 0.10 to within an ULP and no closer, and that is stated
    // rather than rounded away: `0.45 * (20 / 9)` is 1.0000000000000002 and `0.45 * (2 / 9)`
    // is 0.09999999999999999. Only position 0 has to be exact, because position 0 is the one
    // that must reach the engine as `None` -- `tectonicToParams` drops a field that equals
    // canonical, and a field one ULP off canonical would be written into every shared link
    // and take the untouched viewer off the default path. Writing the map as
    // `canonical * (n / 9)` rather than `canonical * n / 9` is what makes position 0 a
    // multiplication by exactly 1.0; the second spelling was ONE ULP OUT and the assertion
    // above is what found it.
    assert!((travel[2].1[0] - 1.0).abs() < 1.0e-15, "the fewest end is 1.00: {}", travel[2].1[0]);
    assert!((travel[2].1[18] - 0.1).abs() < 1.0e-15, "the most end is 0.10: {}", travel[2].1[18]);

    for (field, values) in travel {
        for value in values {
            let mut record = base;
            record[field] = value;
            assert_eq!(
                wb_tectonic_check(record.as_ptr(), WB_TECTONIC_STRIDE as u32),
                WB_OK,
                "the panel can produce tectonic field {field} = {value} and the engine refuses it",
            );
            sample_tectonics(&record, &format!("slider field {field} = {value}"));
        }
    }
}

#[test]
fn the_three_exposed_tectonic_parameters_are_swept_together_not_one_at_a_time() {
    // The same argument the relief cross product makes: a one-axis-at-a-time sweep never
    // visits the corner where the three multiply, and a band-shaped failure lives exactly
    // there. Height and width multiply into a grade; the blend multiplies into the weight the
    // whole collision profile is carried by. 6 x 6 x 5 = 180 combinations, spanning each
    // slider end to end.
    let base = canonical_tectonic_record();
    let travel = tectonic_slider_travel();
    let (heights, widths, blends) = (&travel[0].1, &travel[1].1, &travel[2].1);
    let mut built = 0usize;
    for h in 0..6 {
        for w in 0..6 {
            for b in 0..5 {
                let mut record = base;
                record[0] = heights[h * (heights.len() - 1) / 5];
                record[1] = widths[w * (widths.len() - 1) / 5];
                record[8] = blends[b * (blends.len() - 1) / 4];
                assert_eq!(
                    wb_tectonic_check(record.as_ptr(), WB_TECTONIC_STRIDE as u32),
                    WB_OK,
                    "a corner of the panel own travel is refused: {record:?}",
                );
                sample_tectonics(&record, "tectonic cross product");
                built += 1;
            }
        }
    }
    assert_eq!(built, 180);
}

/// **`suture_count` IS A LOOP BOUND, and this is the test that says so.**
///
/// `Tectonics::sutures` runs `while index < params.suture_count` once per convergent sample.
/// The field was unreachable from outside until this task widened the stride; admitting one
/// without a ceiling is **a HANG, not an odd world**, and a hang through `extern "C"` is
/// uninterruptible because that boundary is nounwind -- the tab does not error, it stops.
///
/// Three separate refusals, because `as u32` in Rust **saturates** and each of them would
/// otherwise arrive as a different large number:
///
/// - **non-finite**: `f64::NAN as u32` is 0 and `f64::INFINITY as u32` is `u32::MAX`;
/// - **non-integral**: 2.5 would truncate to 2 -- a silently-adjusted parameter;
/// - **out of range**: 1e300 and `u32::MAX` saturate to `u32::MAX`, four billion iterations.
///
/// And then the positive half, which is what stops this being a test that passes by refusing
/// everything: every admissible count from 1 to the ceiling is built AND SAMPLED, so the loop
/// actually runs at its bound rather than merely being accepted at it.
#[test]
fn the_suture_count_loop_bound_is_refused_above_its_ceiling_and_sampled_below_it() {
    let mut base = canonical_tectonic_record();
    // A spread the counts can actually use, on a flank narrow enough that eight of them still
    // fit inside the range gate. **Both parts are load-bearing and the first draft had neither
    // right**: without a spread, a count above one is inadmissible for a different reason
    // (coincident sutures), and on the canonical 400 km flank even TWO sutures 30 km apart
    // reach 440 km and are refused by the range check -- so this test would have "proved" a
    // ceiling that was really the gate, at a count of 2. Eight at 30 km on a 100 km flank
    // reach 383.5 km, inside the 420 km gate, so the only thing left to refuse a count here is
    // the ceiling itself.
    base[0] = 6_000.0;
    base[1] = 100_000.0;
    base[SUTURE_SPREAD_FIELD] = 30_000.0;

    let mut refused = 0usize;
    for count in [
        f64::NAN,
        -f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        0.0,
        -0.0,
        -1.0,
        -8.0,
        0.5,
        1.5,
        2.5,
        1.0 + f64::EPSILON,
        f64::from(WB_MAX_SUTURE_COUNT) + 1.0,
        f64::from(WB_MAX_SUTURE_COUNT) + 0.5,
        9.0,
        64.0,
        1.0e6,
        1.0e300,
        f64::from(u32::MAX),
        f64::from(u32::MAX) - 1.0,
        4_294_967_296.0,
        f64::MAX,
    ] {
        let mut record = base;
        record[SUTURE_COUNT_FIELD] = count;
        assert_eq!(
            wb_tectonic_check(record.as_ptr(), WB_TECTONIC_STRIDE as u32),
            WB_ERR_PARAM,
            "suture_count = {count:e} is not a count this boundary admits",
        );
        assert_eq!(
            world_with_tectonics(&record),
            0,
            "the constructor admitted a suture_count the checker refused: {count:e}",
        );
        refused += 1;
    }
    assert_eq!(refused, 22, "the refusal list must not shrink silently");

    // The other half. Every count the boundary DOES admit is walked, because a ceiling that
    // refuses everything is not a ceiling, it is an off switch.
    let mut admitted = 0usize;
    for count in 1..=WB_MAX_SUTURE_COUNT {
        let mut record = base;
        record[SUTURE_COUNT_FIELD] = f64::from(count);
        assert_eq!(
            wb_tectonic_check(record.as_ptr(), WB_TECTONIC_STRIDE as u32),
            WB_OK,
            "suture_count = {count} is inside the ceiling and was refused",
        );
        sample_tectonics(&record, &format!("suture_count = {count}"));
        admitted += 1;
    }
    assert_eq!(admitted, WB_MAX_SUTURE_COUNT as usize); // cast-ok: a small ceiling to usize for a count comparison

    // **And a count above one at a spread of exactly zero is refused**, which is a different
    // hazard reached through the same field: `sutures` places suture `i` at
    // `i * suture_spread_m * jitter`, so at a spread of zero every one of them lands on offset
    // zero and the profile becomes the amplitude times the sum of the weights -- a HEIGHT knob
    // wearing a count's name, decided by a hash of the plate pair. Task 2 measured that same
    // arithmetic as a 64% peak overshoot at four sutures 60 km apart.
    for count in 2..=WB_MAX_SUTURE_COUNT {
        for spread in [0.0, -0.0, -1.0, -100_000.0] {
            let mut record = canonical_tectonic_record();
            record[SUTURE_COUNT_FIELD] = f64::from(count);
            record[SUTURE_SPREAD_FIELD] = spread;
            assert_eq!(
                wb_tectonic_check(record.as_ptr(), WB_TECTONIC_STRIDE as u32),
                WB_ERR_PARAM,
                "{count} coincident sutures at a spread of {spread} is a height knob",
            );
        }
    }
    // One suture at a spread of zero IS canonical, and must stay admissible -- otherwise this
    // rule would have refused the default world.
    let canonical = canonical_tectonic_record();
    assert_eq!(canonical[SUTURE_COUNT_FIELD], 1.0);
    assert_eq!(canonical[SUTURE_SPREAD_FIELD], 0.0);
    assert_eq!(wb_tectonic_check(canonical.as_ptr(), WB_TECTONIC_STRIDE as u32), WB_OK);
}

/// **The range gate, asked of the stacked profile rather than of one width.**
///
/// Task 2 drove off this cliff and measured the drop: four sutures 150 km apart reach 707 km
/// against a 420 km gate, and `offset_m` simply does not evaluate a margin past that -- so the
/// outer sutures are truncated mid-profile rather than faded, measured at **a 41.3% grade and
/// 827 m of relief over 2 km**. `TectonicParams::collision_reach_m` was added for this call
/// site and its own doc says so; this is the boundary asking it.
///
/// The interesting property is that **`continent_collision_width_m` alone cannot see this**:
/// every record below has a width of 100 km, comfortably inside the per-field ceiling, and the
/// refusals come entirely from the count and spread stacked on top of it. A boundary that
/// checked only the widths would have admitted every one.
#[test]
fn a_stacked_collision_profile_past_the_range_gate_is_refused_by_its_reach_not_its_width() {
    let mut base = canonical_tectonic_record();
    base[0] = 6_000.0;
    base[1] = 100_000.0;

    // Straight off Task 2's sutures table: the `inside` / `past` column, reproduced through
    // the boundary rather than restated. Every one of these has an admissible WIDTH.
    for (count, spread_km, inside) in [
        (1u32, 0.0, true),
        (2, 60.0, true),
        (2, 100.0, true),
        (2, 150.0, true),
        (3, 60.0, true),
        (3, 100.0, true),
        (4, 60.0, true),
        (4, 100.0, false),
        (4, 150.0, false),
        (8, 100.0, false),
    ] {
        let mut record = base;
        record[SUTURE_COUNT_FIELD] = f64::from(count);
        record[SUTURE_SPREAD_FIELD] = spread_km * 1_000.0;
        let status = wb_tectonic_check(record.as_ptr(), WB_TECTONIC_STRIDE as u32);
        if inside {
            assert_eq!(status, WB_OK, "{count} sutures at {spread_km} km is inside the gate");
            sample_tectonics(&record, "a stacked profile inside the gate");
        } else {
            assert_eq!(status, WB_ERR_PARAM, "{count} sutures at {spread_km} km reaches past it");
            assert_eq!(world_with_tectonics(&record), 0);
        }
        // The width alone is admissible in EVERY row, which is what makes the reach check the
        // thing doing the work rather than a restatement of the width ceiling.
        let mut width_only = canonical_tectonic_record();
        width_only[1] = record[1];
        assert_eq!(wb_tectonic_check(width_only.as_ptr(), WB_TECTONIC_STRIDE as u32), WB_OK);
    }
}

/// **A `collision_asymmetry` below 1.0 is a cliff `collision_reach_m` cannot see**, and this
/// is the test that pins the floor to that fact rather than to a preference.
///
/// `asymmetric_bump` gives the overriding flank `width_m / asymmetry`, so 0.1 makes that flank
/// ten times `continent_collision_width_m` -- a 400 km profile carrying weight at 4,000 km.
/// `collision_reach_m` reports `continent_collision_width_m`, because the field's own doc
/// guarantees this parameter can only ever NARROW a range, and the reach check would therefore
/// pass a profile the range gate truncates. The floor is what buys that guarantee.
///
/// The ceiling is the other side of the same arithmetic: the narrow flank held against the
/// same floor every width faces, so it is derived and there is no number to pick.
#[test]
fn the_collision_asymmetry_floor_and_ceiling_are_both_the_flank_it_produces() {
    let base = canonical_tectonic_record();
    // Refused below 1.0, at every hostile shape and at the interior values -- a band, not a
    // cliff, is what this project keeps finding.
    for asymmetry in [
        f64::NAN,
        -f64::NAN,
        f64::NEG_INFINITY,
        0.0,
        -0.0,
        -1.0,
        -1.67,
        1.0e-300,
        0.001,
        0.1,
        0.5,
        0.9,
        1.0 - f64::EPSILON,
    ] {
        let mut record = base;
        record[9] = asymmetry;
        assert_eq!(
            wb_tectonic_check(record.as_ptr(), WB_TECTONIC_STRIDE as u32),
            WB_ERR_PARAM,
            "collision_asymmetry = {asymmetry:e} widens the flank the reach check cannot see",
        );
        assert_eq!(world_with_tectonics(&record), 0);
    }
    // Admitted from exactly 1.0 upward, through the whole measured sweep and well past it.
    for asymmetry in [1.0, 1.0 + f64::EPSILON, 1.25, 1.67, 2.0, 2.5, 3.0, 10.0, 1.0e6] {
        let mut record = base;
        record[9] = asymmetry;
        assert_eq!(
            wb_tectonic_check(record.as_ptr(), WB_TECTONIC_STRIDE as u32),
            WB_OK,
            "collision_asymmetry = {asymmetry:e} only narrows the overriding flank",
        );
        sample_tectonics(&record, "an asymmetric wedge");
    }
    // And refused again once the flank it produces falls under the width floor, which is where
    // the ceiling is: `continent_collision_width_m / asymmetry < WB_MIN_TECTONIC_WIDTH_M`. On
    // the canonical 400 km flank that crossing is at 4e8, and the two sides of it are checked
    // rather than one -- the ceiling has to be a band edge, not an assertion.
    let crossing = base[1] / WB_MIN_TECTONIC_WIDTH_M;
    for (asymmetry, admissible) in [(crossing * 0.5, true), (crossing, true), (crossing * 2.0, false), (f64::INFINITY, false)] {
        let mut record = base;
        record[9] = asymmetry;
        let status = wb_tectonic_check(record.as_ptr(), WB_TECTONIC_STRIDE as u32);
        assert_eq!(
            status,
            if admissible { WB_OK } else { WB_ERR_PARAM },
            "asymmetry {asymmetry:e} gives a flank of {} m",
            base[1] / asymmetry,
        );
    }
}

/// The five structure fields swept **together**, on the envelope they are meant for.
///
/// The same argument the three-parameter cross product above makes, and it applies harder
/// here: Task 2 measured that these interact -- `structure_depth` costs 22% of the peak, a
/// tight `suture_spread_m` adds 64%, and the wavelength decides whether the depth does
/// anything at all. A one-axis-at-a-time sweep never visits the corner where they multiply,
/// and every hazard this project has found was a band sitting exactly there.
#[test]
fn the_structure_fields_are_swept_together_on_the_envelope_they_are_for() {
    let mut base = canonical_tectonic_record();
    base[0] = 6_000.0;
    base[1] = 100_000.0;
    let travel = tectonic_slider_travel();
    let (asymmetries, depths, wavelengths) = (&travel[3].1, &travel[4].1, &travel[5].1);

    let mut built = 0usize;
    let mut refusals = 0usize;
    for a in 0..5 {
        for d in 0..5 {
            for w in 0..wavelengths.len() {
                // The last pair reaches 707 km against the 420 km gate -- Task 2's own
                // `4 x 150 km` row, the one it measured at a 41.3% grade. It is in this list
                // so the cross product has a refused corner by construction rather than by
                // luck: a sweep whose every record is admitted proves nothing about a bound.
                for (count, spread) in
                    [(1.0, 0.0), (2.0, 100_000.0), (4.0, 60_000.0), (4.0, 150_000.0)]
                {
                    let mut record = base;
                    record[9] = asymmetries[a * (asymmetries.len() - 1) / 4];
                    record[12] = depths[d * (depths.len() - 1) / 4];
                    record[13] = wavelengths[w];
                    record[SUTURE_COUNT_FIELD] = count;
                    record[SUTURE_SPREAD_FIELD] = spread;
                    if wb_tectonic_check(record.as_ptr(), WB_TECTONIC_STRIDE as u32) == WB_OK {
                        sample_tectonics(&record, "structure cross product");
                        built += 1;
                    } else {
                        assert_eq!(world_with_tectonics(&record), 0);
                        refusals += 1;
                    }
                }
            }
        }
    }
    assert_eq!(built + refusals, 500);
    // Both sides non-trivial: a cross product that accepted everything would prove nothing
    // about the bounds, and one that refused everything would prove nothing about the fields.
    assert!(built >= 300, "only {built} of 500 structure corners were admitted");
    assert!(refusals > 0, "no corner of the structure cross product is refused");
}

/// **The warp against the reach, swept together, because the gate is where they meet.**
///
/// `margin_warp_m` is admitted by two different checks and only one of them can see the
/// interaction. `WB_MAX_MARGIN_WARP_M` looks at the field alone; `collision_reach_m` adds it
/// to the suture offsets and the flank width and holds the SUM against the range gate. On the
/// preset that sum is 315 km of 420 km, so the amplitude has 105 km of headroom -- and a
/// third suture, or a wider spread, spends it. A one-axis sweep visits neither corner.
///
/// This is the corner. Every combination is checked, every accepted one is BUILT and SAMPLED
/// (`Tectonics::new` only stores the block, so a constructor that returned a handle has proved
/// nothing), and both sides are asserted non-trivial.
#[test]
fn the_warp_and_the_reach_are_swept_together_against_the_range_gate() {
    let base = tectonic_preset_record(WB_TECTONIC_RANGES);
    let mut built = 0usize;
    let mut refusals = 0usize;
    for warp in [0.0, 20_000.0, 80_000.0, 120_000.0, 200_000.0, MAX_TECTONIC_RANGE_M] {
        for wavelength in [WB_MIN_MARGIN_WARP_WAVELENGTH_M, 300_000.0, 900_000.0, 1.0e9] {
            for (count, spread) in
                [(1.0, 0.0), (2.0, 100_000.0), (3.0, 100_000.0), (2.0, 150_000.0)]
            {
                let mut record = base;
                record[14] = warp;
                record[15] = wavelength;
                record[SUTURE_COUNT_FIELD] = count;
                record[SUTURE_SPREAD_FIELD] = spread;
                let status = wb_tectonic_check(record.as_ptr(), WB_TECTONIC_STRIDE as u32);
                // The reach is the claim, so it is computed here from the record and the two
                // are held to each other -- the check must refuse exactly what the gate would
                // truncate, not merely refuse something.
                let last = if count > 1.0 { (count - 1.0) * spread * 1.35 } else { 0.0 };
                let reach = last + record[1] + warp;
                let inside = reach <= MAX_TECTONIC_RANGE_M;
                assert_eq!(
                    status == WB_OK,
                    inside,
                    "warp {warp} @ {wavelength} with {count} sutures at {spread} reaches \
                     {reach} m against the {MAX_TECTONIC_RANGE_M} m gate",
                );
                if status == WB_OK {
                    sample_tectonics(&record, "warp x reach corner");
                    built += 1;
                } else {
                    assert_eq!(world_with_tectonics(&record), 0);
                    refusals += 1;
                }
            }
        }
    }
    assert_eq!(built + refusals, 96);
    assert!(built >= 30, "only {built} of 96 warp corners were admitted");
    assert!(refusals >= 20, "only {refusals} of 96 warp corners were refused");
}

/// **The wavelength floor is a SILENCE, and this is the shape that keeps biting.**
///
/// `Tectonics::margin_warp_m_at` opens with `if wavelength <= 0.0 { return 0.0 }`, so a zero
/// or negative wavelength is a record that is well formed, would be built without complaint,
/// and displaces nothing anywhere -- while `margin_warp_m` sits beside it at 80 km looking
/// configured. Refused, exactly as the four zero-width profiles are, and checked against the
/// engine guard rather than assumed to agree with it.
#[test]
fn a_warp_wavelength_that_displaces_nothing_is_refused_rather_than_admitted_silently() {
    let mut base = tectonic_preset_record(WB_TECTONIC_RANGES);
    base[14] = 80_000.0;
    for nothing in [0.0, -0.0, -1.0, -300_000.0, f64::NEG_INFINITY] {
        let mut record = base;
        record[15] = nothing;
        assert_eq!(
            wb_tectonic_check(record.as_ptr(), WB_TECTONIC_STRIDE as u32),
            WB_ERR_PARAM,
            "a warp wavelength of {nothing} is a field that looks configured and does nothing",
        );
        assert_eq!(world_with_tectonics(&record), 0);
    }
    // And a negative AMPLITUDE, which is the mirror image rather than a silence, is refused
    // too -- see `WB_MIN_MARGIN_WARP_M`. `collision_reach_m` takes `abs` so the reach would
    // still be honest; the floor is what makes that `abs` a second line of defence rather
    // than the only one, and both halves are asserted.
    for mirrored in [-1.0, -80_000.0, -MAX_TECTONIC_RANGE_M] {
        let mut record = base;
        record[14] = mirrored;
        assert_eq!(
            wb_tectonic_check(record.as_ptr(), WB_TECTONIC_STRIDE as u32),
            WB_ERR_PARAM,
            "a warp amplitude of {mirrored} is outside this channel's stated domain",
        );
        assert_eq!(world_with_tectonics(&record), 0);
    }
}

#[test]
fn the_tectonic_channel_default_path_is_the_untouched_world() {
    // RULING 1, and the one property this whole slice is not allowed to break: a null pointer
    // with a length of zero is `None`, not `Some(canonical())`, and the world it builds is
    // byte-for-byte the world `wb_world_new` builds.
    //
    // **This test cannot prove the params are read**, and the slice ledger Ruling 1 says so
    // explicitly: `None` resolves through `unwrap_or_else(TectonicParams::canonical)`, so the
    // `None` arm and the `Some(canonical())` arm call the same function and agree no matter
    // what the uplift path ignores. That proof is `tectonics.rs`'s one-ULP perturbation
    // fixtures, and `a_chosen_tectonic_block_actually_moves_the_ground_it_claims_to` below is
    // this channel own version of it. What THIS test proves is the different, still-necessary
    // thing: that opening the channel did not move the default world.
    let plain = plain_world();
    let defaulted = wb_world_new_tectonic(
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
    );
    assert_ne!(defaulted, 0);
    let explicit_canonical = world_with_tectonics(&canonical_tectonic_record());
    assert_ne!(explicit_canonical, 0);

    for (lat, lon) in TECTONIC_PROBES {
        for resolution in [RES_M, -1.0] {
            let expected = wb_elevation_m(plain, *lat, *lon, resolution);
            assert_eq!(
                wb_elevation_m(defaulted, *lat, *lon, resolution).to_bits(),
                expected.to_bits(),
                "the null tectonic path moved the world at ({lat}, {lon})",
            );
            assert_eq!(
                wb_elevation_m(explicit_canonical, *lat, *lon, resolution).to_bits(),
                expected.to_bits(),
                "an explicit canonical record moved the world at ({lat}, {lon})",
            );
        }
        let expected = wb_structural_m(plain, *lat, *lon);
        assert_eq!(wb_structural_m(defaulted, *lat, *lon).to_bits(), expected.to_bits());
        assert_eq!(wb_structural_m(explicit_canonical, *lat, *lon).to_bits(), expected.to_bits());
    }
    for handle in [plain, defaulted, explicit_canonical] {
        assert_eq!(wb_world_free(handle), WB_OK);
    }
}

#[test]
fn a_chosen_tectonic_block_actually_moves_the_ground_it_claims_to() {
    // The whole point of the slice, asserted at the boundary rather than only three modules
    // down: a taller, narrower collision profile has to produce a DIFFERENT planet through
    // this export. Without this, every test above would pass over a channel that decoded nine
    // f64 and threw them away -- which is the sixth assertion in this project to look
    // load-bearing and not be, and the reason the slice ledger Ruling 1 exists.
    let mut alps = canonical_tectonic_record();
    alps[0] = 6_000.0;
    alps[1] = 100_000.0;
    let steep = sample_tectonics(&alps, "6000 m / 100 km");
    let canonical = sample_tectonics(&canonical_tectonic_record(), "canonical");
    assert_eq!(steep.len(), canonical.len());
    assert!(
        steep.iter().zip(&canonical).any(|(a, b)| a.to_bits() != b.to_bits()),
        "a 4x collision amplitude at a quarter of the width changed nothing at six probes",
    );

    // And the blend, separately, because it reaches a different term -- the profile MIX, not
    // the profile. Measured on the owner world at 6,000 m / 150 km, the count of 0.5-degree
    // sites above 1,000 m runs 925 at blend 0.10, 618 at canonical 0.45 and 332 at 1.00.
    let mut fewer = canonical_tectonic_record();
    fewer[8] = 1.0;
    let thinned = sample_tectonics(&fewer, "blend 1.00");
    assert!(
        thinned.iter().zip(&canonical).any(|(a, b)| a.to_bits() != b.to_bits()),
        "more than doubling the continental transition width changed nothing at six probes",
    );
}

#[test]
fn wb_tectonic_preset_hands_back_the_engines_own_canonical_and_nothing_else() {
    // The preset export exists so no host transcribes a default. It must therefore BE the
    // default: `TectonicParams::canonical()`, in `WB_TECTONIC_STRIDE` order, and the module
    // constants it is built from.
    let record = canonical_tectonic_record();
    assert_eq!(record[0], 1500.0, "continent_collision_m");
    assert_eq!(record[1], 400_000.0, "continent_collision_width_m");
    assert_eq!(record[2], 900.0, "coastal_uplift_m");
    assert_eq!(record[3], 260_000.0, "coastal_uplift_width_m");
    assert_eq!(record[4], 700.0, "island_arc_m");
    assert_eq!(record[5], 110_000.0, "island_arc_width_m");
    assert_eq!(record[6], 900.0, "ridge_m");
    assert_eq!(record[7], 380_000.0, "ridge_width_m");
    assert_eq!(record[8], 0.45, "continental_blend");
    // And the five structure fields at their INERT settings -- the ones that make the
    // canonical path perform the same operations on the same numbers it did before those
    // fields existed. Word 13 is the placeholder wavelength, which `structure_at` never reads
    // while word 12 is zero, and it is stated here rather than left unasserted precisely
    // because it is the one canonical value that is not an arithmetic identity.
    assert_eq!(record[9], 1.0, "collision_asymmetry: symmetric, width / 1.0 is width");
    assert_eq!(record[10], 1.0, "suture_count: one bump, weight exactly 1.0 at offset 0.0");
    assert_eq!(record[11], 0.0, "suture_spread_m");
    assert_eq!(record[12], 0.0, "structure_depth: the multiplier is exactly 1.0, unsampled");
    assert_eq!(record[13], 120_000.0, "structure_wavelength_m: the unread placeholder");
    // Its own checker must accept it, or the panel starting position would be a refused world.
    assert_eq!(wb_tectonic_check(record.as_ptr(), WB_TECTONIC_STRIDE as u32), WB_OK);

    // There are exactly TWO selectors now: canonical, and the preset Task 3 chose. Every other
    // value is refused, and that is checked either side of both known ones rather than only
    // far away from them -- an off-by-one in a selector comparison is a band like any other.
    let mut scratch = [0.0; WB_TECTONIC_STRIDE];
    for unknown in [2u32, 3, 7, 100, u32::MAX, u32::MAX - 1] {
        assert_eq!(
            wb_tectonic_preset(unknown, scratch.as_mut_ptr(), WB_TECTONIC_STRIDE as u32),
            WB_ERR_PARAM,
            "selector {unknown} is not a preset this build knows",
        );
    }
}

/// **The preset, across the boundary, as fields.**
///
/// Task 3's whole point: the owner presses one button and the panel fills with fourteen
/// numbers it did not write down. So this asserts the export hands back
/// `TectonicParams::ranges()` word for word, that its own checker accepts it (a preset the
/// boundary refuses would be a button that produces a blank page), and that building the world
/// it describes moves the ground away from canonical at a point where the collision profile is
/// live.
#[test]
fn the_ranges_preset_crosses_the_boundary_as_fields_and_is_admissible() {
    let preset = tectonic_preset_record(WB_TECTONIC_RANGES);
    let canonical = canonical_tectonic_record();

    // The six moved fields, at the values `TectonicParams::ranges()`' doc comment argues for.
    assert_eq!(preset[0], 6_000.0, "continent_collision_m: Task 4's calibrated top");
    assert_eq!(preset[1], 100_000.0, "continent_collision_width_m: 7.030% measured");
    assert_eq!(preset[9], 2.0, "collision_asymmetry: the setting that MEASURES 1.67");
    assert_eq!(preset[10], 2.0, "suture_count: the useful pair");
    assert_eq!(preset[11], 100_000.0, "suture_spread_m: the 100-150 km band");
    assert_eq!(preset[12], 0.7, "structure_depth");
    assert_eq!(preset[13], 80_000.0, "structure_wavelength_m: inside the 40-80 km band");
    // And the eight it leaves alone, held against the canonical record rather than against a
    // second copy of eight numbers.
    for field in [2usize, 3, 4, 5, 6, 7, 8] {
        assert_eq!(preset[field], canonical[field], "the preset moved field {field}");
    }

    assert_eq!(
        wb_tectonic_check(preset.as_ptr(), WB_TECTONIC_STRIDE as u32),
        WB_OK,
        "the boundary refuses its own preset -- the button would produce a blank page",
    );

    // It must reach the ground, not merely the constructor. `TECTONIC_PROBES`' last three are
    // the sites Task 4 measured as where each knob moves this fixture world the most, and the
    // collision witness is where a collision block has to show up if it shows up anywhere.
    let moved = sample_tectonics(&preset, "the ranges preset");
    let flat = sample_tectonics(&canonical, "canonical");
    assert_ne!(moved, flat, "the preset builds the canonical world");
}

#[test]
fn the_tectonic_buffer_channel_refuses_what_it_cannot_read() {
    let record = canonical_tectonic_record();
    // Null with a length is a caller mistake, not a request for canonical.
    assert_eq!(wb_tectonic_check(core::ptr::null(), WB_TECTONIC_STRIDE as u32), WB_ERR_BUFFER);
    // Non-null with a length of zero is a host that computed a length wrong.
    assert_eq!(wb_tectonic_check(record.as_ptr(), 0), WB_ERR_BUFFER);
    // Null with zero IS canonical.
    assert_eq!(wb_tectonic_check(core::ptr::null(), 0), WB_OK);
    // 9 is the old stride, and it is in this list on purpose: a host built against the Task 4
    // channel must be REFUSED rather than quietly given five canonical structure fields, which
    // is the silently-dropping shape. 13 and 15 are one either side of the new stride.
    for length in [1u32, 8, 9, 10, 13, 15, 18, 28, u32::MAX] {
        assert_eq!(
            wb_tectonic_check(record.as_ptr(), length),
            WB_ERR_BUFFER,
            "a {length}-word tectonic record is not a tectonic record",
        );
    }
    // Misaligned: one byte into an f64-sized buffer.
    let mut bytes = [0u8; WB_TECTONIC_STRIDE * 8 + 8];
    let misaligned = unsafe { bytes.as_mut_ptr().add(1) } as *const f64; // cast-ok: a deliberately misaligned pointer for the alignment check
    assert_eq!(wb_tectonic_check(misaligned, WB_TECTONIC_STRIDE as u32), WB_ERR_BUFFER);

    // The constructor refuses the same things, with a handle of 0 rather than a status.
    assert_eq!(
        wb_world_new_tectonic(
            SEED,
            RADIUS_M,
            PLATES,
            LAND,
            core::ptr::null(),
            0,
            core::ptr::null(),
            0,
            core::ptr::null(),
            WB_TECTONIC_STRIDE as u32,
        ),
        0,
    );
    assert_eq!(
        wb_world_new_tectonic(
            SEED,
            RADIUS_M,
            PLATES,
            LAND,
            core::ptr::null(),
            0,
            core::ptr::null(),
            0,
            record.as_ptr(),
            0,
        ),
        0,
    );
    assert_eq!(
        wb_world_new_tectonic(
            SEED,
            RADIUS_M,
            PLATES,
            LAND,
            core::ptr::null(),
            0,
            core::ptr::null(),
            0,
            record.as_ptr(),
            4,
        ),
        0,
    );
    // A bad buffer for the OUT parameter of the preset export.
    let mut out = [0.0; WB_TECTONIC_STRIDE];
    assert_eq!(
        wb_tectonic_preset(WB_TECTONIC_CANONICAL, core::ptr::null_mut(), WB_TECTONIC_STRIDE as u32),
        WB_ERR_BUFFER,
    );
    // 9 is the old stride: a host built against the Task 4 channel gets a refusal, not nine
    // words and five it does not know are missing.
    for selector in [WB_TECTONIC_CANONICAL, WB_TECTONIC_RANGES] {
        for length in [8u32, 9, 13, 15, 0] {
            assert_eq!(
                wb_tectonic_preset(selector, out.as_mut_ptr(), length),
                WB_ERR_BUFFER,
                "preset {selector} wrote into a {length}-word buffer",
            );
        }
    }
}

#[test]
fn the_two_channels_are_independent_and_the_third_door_carries_both() {
    // `wb_world_new_tectonic` takes a relief record AND a tectonic record, and a host that
    // moves one must not silently lose the other. Four corners: neither, relief only,
    // tectonics only, both.
    let relief = hills_record();
    let mut tectonics = canonical_tectonic_record();
    tectonics[0] = 6_000.0;
    tectonics[1] = 100_000.0;

    let build = |r: Option<&[f64; WB_RELIEF_STRIDE]>, t: Option<&[f64; WB_TECTONIC_STRIDE]>| {
        let (rp, rl) = match r {
            Some(record) => (record.as_ptr(), WB_RELIEF_STRIDE as u32),
            None => (core::ptr::null(), 0),
        };
        let (tp, tl) = match t {
            Some(record) => (record.as_ptr(), WB_TECTONIC_STRIDE as u32),
            None => (core::ptr::null(), 0),
        };
        let handle = wb_world_new_tectonic(
            SEED,
            RADIUS_M,
            PLATES,
            LAND,
            core::ptr::null(),
            0,
            rp,
            rl,
            tp,
            tl,
        );
        assert_ne!(handle, 0);
        let heights: Vec<u64> = TECTONIC_PROBES
            .iter()
            .map(|(lat, lon)| wb_elevation_m(handle, *lat, *lon, RES_M).to_bits())
            .collect();
        assert_eq!(wb_world_free(handle), WB_OK);
        heights
    };

    let neither = build(None, None);
    let relief_only = build(Some(&relief), None);
    let tectonics_only = build(None, Some(&tectonics));
    let both = build(Some(&relief), Some(&tectonics));

    assert_ne!(neither, relief_only, "the relief record was dropped by the third door");
    assert_ne!(neither, tectonics_only, "the tectonic record was dropped by the third door");
    assert_ne!(both, relief_only, "the tectonic record was dropped when relief was also given");
    assert_ne!(both, tectonics_only, "the relief record was dropped when tectonics was also given");

    // And the third door with two null blocks is still the untouched world.
    let plain = plain_world();
    for (index, (lat, lon)) in TECTONIC_PROBES.iter().enumerate() {
        assert_eq!(
            neither[index],
            wb_elevation_m(plain, *lat, *lon, RES_M).to_bits(),
            "two null blocks moved the world at ({lat}, {lon})",
        );
    }
    assert_eq!(wb_world_free(plain), WB_OK);
}

/// **The size and shape of the tectonic sweep, stated as a number rather than left implicit.**
///
/// A report that says "the sweep found nothing" is worthless unless the sweep's size is
/// checkable, and a threshold assertion (`accepted >= 180`) says nothing about how far above
/// the threshold the run actually was. This pins the counts exactly, so a later change that
/// halves the sweep -- a field quietly dropped from `tectonic_field_domain`, a base removed --
/// turns this red with the two numbers side by side instead of passing at 181.
#[test]
fn the_tectonic_sweep_is_the_size_it_claims_to_be() {
    let records = swept_tectonic_records();
    let mut accepted = 0usize;
    for (_, record) in &records {
        if wb_tectonic_check(record.as_ptr(), WB_TECTONIC_STRIDE as u32) == WB_OK {
            accepted += 1;
        }
    }
    // 2 bases x (15 ordinary fields at 57 values each, plus the count field's 57 + 24
    // hand-added rungs -- a linear ladder over eight integers is almost all fractions).
    // Task 5's two fields take the ordinary count from 13 to 15.
    assert_eq!(records.len(), 2 * (15 * 57 + 81), "the sweep changed size");
    assert_eq!(records.len(), 1872, "and the arithmetic above says 1,872");
    // 1,044 accepted, 828 refused. Neither side is trivial, which is the property that makes
    // "no abort and no hang was found" a result rather than an absence. Re-derived after
    // Task 5 widened the stride; the previous pin was 947 of 1,644.
    assert_eq!(
        accepted, 1_044,
        "the accepted/refused split moved: {accepted} of {}",
        records.len()
    );
}

// ============================================================== the coastal channel, Task 6
//
// `wb_world_new_coast`, `wb_coast_preset` and `wb_coast_check`: the fourth door onto
// `build_world`, and the one that carries Task 5's `CoastParams` to a browser.
//
// **This channel is swept rather than spot-checked, and the sweep is the whole reason these
// tests are long.** This project has found three aborts and one ~2,600-second hang by sweeping
// export inputs and ZERO by spot-checking, and every one was a **band, not a cliff**. Two of
// this channel's bounds close such a band: `WB_MAX_COAST_OCTAVES` is a per-sample loop bound,
// and `WB_MAX_COAST_FINEST_FREQUENCY` holds a PRODUCT that three individually-admissible fields
// compound into and that no per-field ceiling can see.

/// Where the coastal term actually acts, on this file's fixture world.
///
/// **These are witnesses, not a scatter.** The term is windowed by `|above_shore|`, so a probe
/// set chosen for the tectonic channel -- or one scattered uniformly over a sphere that is 71%
/// open ocean -- would sit outside the coastal band and every coast test above would pass over a
/// channel that decoded six f64 and threw them away. That is the sixth assertion in this
/// project to look load-bearing and not be, and the reason the slice ledger's Ruling 1 exists.
///
/// **Derived, not picked.** A 0.5-degree global scan of the fixture world (`SEED`, `RADIUS_M`,
/// 12 plates, land 0.29) compares `Surface::with_coast(.., None)` against
/// `Some(CoastParams::fractal())` at `resolution_m = 250`: **162,159 of 258,480 sites move**,
/// and the eight below are the largest movers subject to a 25-degree separation so they are not
/// eight samples of one bay. The three after them are this file's own existing probes, kept so
/// the set is not exclusively coastal and a term that leaked into the deep interior would show
/// up as a *failure* of `the_coastal_term_is_not_a_global_one`.
const COAST_PROBES: &[(f64, f64)] = &[
    (-71.5, 38.0),    // 1,267 m of movement under `fractal()`
    (-73.0, -132.0),  // 1,197 m
    (3.0, -107.5),    // 1,177 m
    (-14.5, -20.5),   // 1,155 m
    (17.5, 57.5),     // 1,140 m
    (11.5, -174.5),   // 1,122 m
    (66.0, -82.5),    // 1,066 m
    (71.5, 141.5),    // 1,062 m
    (12.0, 34.0),     // the witnessed point
    (0.0, 0.0),
    (-18.25, 121.5),  // the harbour
];

/// A named preset, read across the boundary exactly as the viewer reads it. **Nothing in this
/// file writes a coast value down**, canonical or preset: both come from `wb_coast_preset`, so
/// `continentality.rs` stays the one place the numbers live and a test cannot agree with a stale
/// copy of them.
fn coast_preset_record(selector: u32) -> [f64; WB_COAST_STRIDE] {
    let mut record = [0.0; WB_COAST_STRIDE];
    let status = wb_coast_preset(selector, record.as_mut_ptr(), WB_COAST_STRIDE as u32);
    assert_eq!(status, WB_OK, "coast preset {selector} must be readable");
    record
}

fn canonical_coast_record() -> [f64; WB_COAST_STRIDE] {
    coast_preset_record(WB_COAST_CANONICAL)
}

fn world_with_coast(record: &[f64; WB_COAST_STRIDE]) -> u32 {
    wb_world_new_coast(
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
        record.as_ptr(),
        WB_COAST_STRIDE as u32,
    )
}

/// Build the world a coast record asks for, walk every probe point, and free it.
///
/// **This is where an abort or a hang would happen, and that is the point of calling it.**
/// `Continentality::with_coast` merely *stores* the block -- it is `above_shore` that reads it,
/// once per sample -- so a constructor that returned a handle has proved nothing at all about
/// the record it was given. The octave loop and the frequency product are both walked here and
/// nowhere earlier.
fn sample_coast(record: &[f64; WB_COAST_STRIDE], label: &str) -> Vec<f64> {
    let handle = world_with_coast(record);
    assert_ne!(handle, 0, "accepted record refused by the constructor: {label} {record:?}");
    let mut heights = Vec::with_capacity(COAST_PROBES.len());
    for (lat, lon) in COAST_PROBES {
        let height = wb_elevation_m(handle, *lat, *lon, RES_M);
        assert!(
            height.is_finite(),
            "accepted record produced a non-finite elevation at ({lat}, {lon}): {label} {record:?}",
        );
        // `structural_m` is the other half: the coastal term reaches the SHELF through the same
        // `Continentality`, so sampling only `elevation_m` would leave half the thing under test
        // unwatched. A NaN that only appeared in the shelf would be invisible above.
        let structural = wb_structural_m(handle, *lat, *lon);
        assert!(
            structural.is_finite(),
            "accepted record gave a non-finite structural at ({lat}, {lon}): {label} {record:?}",
        );
        heights.push(height);
    }
    assert_eq!(wb_world_free(handle), WB_OK);
    heights
}

/// The documented domain of each coast field, by its index in `WB_COAST_STRIDE`'s order.
///
/// **Written as the constants rather than as numbers**, for the reason `tectonic_field_domain`
/// gives: a test that restates a bound cannot notice it moving.
fn coast_field_domain(field: usize) -> (f64, f64) {
    match field {
        0 => (WB_MIN_COAST_AMPLITUDE, WB_MAX_COAST_AMPLITUDE),
        1 => (WB_MIN_COAST_WINDOW_SPREADS, WB_MAX_COAST_WINDOW_SPREADS),
        // The per-field ceiling on `frequency` IS the finest-octave ceiling: a single octave's
        // frequency cannot exceed the finest one the schedule reaches. The binding check for
        // this field is still the product, which is why the ladder runs the whole way to the top
        // -- at `fractal()`'s four octaves and lacunarity 2, everything above 125,000 is refused
        // by the PRODUCT while sitting inside this per-field range, and that band is exactly
        // what a sweep stopping at the first refusal would never see.
        2 => (WB_MIN_COAST_FREQUENCY, WB_MAX_COAST_FINEST_FREQUENCY),
        3 => (1.0, f64::from(WB_MAX_COAST_OCTAVES)),
        4 => (0.0, WB_MAX_COAST_GAIN),
        5 => (WB_MIN_COAST_LACUNARITY, WB_MAX_COAST_LACUNARITY),
        _ => unreachable!("WB_COAST_STRIDE is 6"),
    }
}

/// Word 3 of a coast record: `octaves`, **the loop bound**. Named because two places below have
/// to treat it differently from the five f64 fields around it.
const COAST_OCTAVES_FIELD: usize = 3;

/// Every value one coast field is driven through: `HOSTILE` in full, both documented bounds and
/// the values immediately either side of each, and a ladder across the admissible interval --
/// geometric where the domain spans orders of magnitude (`frequency` runs 0.5 to 1e6 and
/// `window_spreads` 1e-3 to 4, where a linear ladder would put its first rung a long way above
/// the floor and never sample the small end at all) and linear where it does not.
///
/// **The interior values between the sensible ones are the point**, not the endpoints: every
/// hazard this project has found was a band. `WB_MAX_EROSION_RATE_PER_YR`'s doc records a
/// negative erodibility where `-9.0e-4` aborts while `-1.0e-2`, `-1.0` and `-1.0e-6` all stay
/// finite, and a single spot-check at one negative value would have missed it.
fn coast_field_sweep(field: usize) -> Vec<f64> {
    let (low, high) = coast_field_domain(field);
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
    // **The loop bound is the one field a ladder sweeps badly, and it is the one field where
    // that matters most** -- exactly as it is for `suture_count`, and for the same reason. Its
    // domain is the sixteen integers 1..=16 and a 25-rung linear ladder across it is mostly
    // fractions, which the boundary refuses for being non-integral before the count is ever
    // exercised. So every integer either side of both ends is added by hand, along with the
    // values a SATURATING `as u32` would turn into a four-billion-iteration walk PER SAMPLE:
    // `HOSTILE` already carries `f64::MAX`, `INFINITY` and `1e300`, and `u32::MAX` itself and
    // its neighbours are added here because they are the exact number a saturating cast
    // produces and nothing else in this list is.
    if field == COAST_OCTAVES_FIELD {
        for integer in -2..=20 {
            values.push(f64::from(integer));
        }
        values.extend_from_slice(&[
            f64::from(WB_MAX_COAST_OCTAVES) + 1.0,
            f64::from(u32::MAX),
            f64::from(u32::MAX) - 1.0,
            f64::from(u32::MAX) + 1.0,
            4_294_967_296.0,
            2.5,
            1.5,
            1.0 + f64::EPSILON,
            2.0 - f64::EPSILON,
        ]);
    }
    values
}

/// The two bases every coast field is swept around.
///
/// **One base is not a sweep of this channel, and the reason is sharper here than it was for the
/// tectonic one.** `CoastParams::canonical()` carries `amplitude = 0.0`, and
/// `Continentality::above_shore` branches on exactly that BEFORE it touches the noise -- so
/// around canonical, `frequency`, `octaves`, `gain` and `lacunarity` are swept with the code
/// that reads them switched off entirely. Four of the six fields, including **both** halves of
/// the loop bound and the frequency product, would be swept against a function that returns
/// early. Around `fractal()` the term is live and every field is read.
///
/// The canonical base is kept anyway rather than dropped: it is the base a host reaches by
/// moving one slider off zero, and the *validator* is exercised there even where the engine is
/// not.
fn coast_sweep_bases() -> [(&'static str, [f64; WB_COAST_STRIDE]); 2] {
    [("canonical", canonical_coast_record()), ("fractal", coast_preset_record(WB_COAST_FRACTAL))]
}

fn swept_coast_records() -> Vec<(String, [f64; WB_COAST_STRIDE])> {
    let mut out = Vec::new();
    for (base_name, base) in coast_sweep_bases() {
        for field in 0..WB_COAST_STRIDE {
            for value in coast_field_sweep(field) {
                let mut record = base;
                record[field] = value;
                out.push((format!("{base_name} + coast field {field} = {value:e}"), record));
            }
        }
    }
    out
}

#[test]
fn every_coast_field_swept_across_its_whole_range_and_beyond_never_aborts() {
    let records = swept_coast_records();
    // A sweep that refused everything would pass a "nothing aborted" assertion trivially, and
    // one that accepted everything would prove the validator absent. Both counts are asserted.
    let mut accepted = 0usize;
    let mut refused = 0usize;
    for (label, record) in &records {
        if wb_coast_check(record.as_ptr(), WB_COAST_STRIDE as u32) == WB_OK {
            sample_coast(record, label);
            accepted += 1;
        } else {
            refused += 1;
        }
    }
    assert_eq!(accepted + refused, records.len());
    assert!(
        accepted >= 100,
        "only {accepted} records were accepted; the sweep is not exercising the engine",
    );
    assert!(
        refused >= 100,
        "only {refused} records were refused; the validator is not doing its job",
    );
}

#[test]
fn the_coast_checker_and_the_constructor_agree_on_every_swept_record() {
    // Two validators would be two chances to disagree, and the disagreement that matters is
    // "the checker said yes and the constructor aborted". Held to each other over the identical
    // population the sweep above uses.
    for (label, record) in swept_coast_records() {
        let status = wb_coast_check(record.as_ptr(), WB_COAST_STRIDE as u32);
        let handle = world_with_coast(&record);
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
fn the_coast_sweep_is_the_size_it_claims_to_be() {
    // **The size and shape of the coast sweep, stated as a number rather than left implicit.**
    // A report that says "the sweep found nothing" is worthless unless the sweep's size is
    // checkable, and a threshold assertion (`accepted >= 100`) says nothing about how far above
    // the threshold the run actually was. This pins the counts exactly, so a later change that
    // halves the sweep -- a field quietly dropped from `coast_field_domain`, a base removed --
    // turns this red with the two numbers side by side instead of passing at 101.
    let records = swept_coast_records();
    let mut accepted = 0usize;
    for (_, record) in &records {
        if wb_coast_check(record.as_ptr(), WB_COAST_STRIDE as u32) == WB_OK {
            accepted += 1;
        }
    }
    // 2 bases x (5 ordinary fields at 57 values each, plus the octave field's 57 + 32 hand-added
    // rungs -- a linear ladder over sixteen integers is almost all fractions).
    assert_eq!(records.len(), 2 * (5 * 57 + 89), "the sweep changed size");
    assert_eq!(records.len(), 748, "and the arithmetic above says 748");
    assert_eq!(
        accepted, COAST_SWEEP_ACCEPTED,
        "the accepted/refused split moved: {accepted} of {}",
        records.len()
    );
}

/// The accepted half of the coast sweep, pinned. Re-derived on this host by running the sweep;
/// see `the_coast_sweep_is_the_size_it_claims_to_be` for why a bare threshold is not enough.
const COAST_SWEEP_ACCEPTED: usize = 392;

#[test]
fn the_octave_count_loop_bound_is_refused_above_its_ceiling_and_bounded_below_it() {
    // **`octaves` is a LOOP BOUND and therefore a HANG, not an abort**, and the hang is delivered
    // by the cast rather than by the caller: `value as u32` in Rust SATURATES, so 1e300 arrives
    // as `u32::MAX` and `Noise::fbm` walks four billion octaves **per sample** -- once per texel
    // of every tile. `WB_MAX_SUTURE_COUNT`'s doc records the ~2,600-second measurement that
    // motivated bounding the other loop bound on this boundary; this is the same shape.
    //
    // Every value below is one the cast would turn into something other than what it says.
    let base = coast_preset_record(WB_COAST_FRACTAL);
    for hostile in [
        f64::NAN,
        -f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::MAX,
        1.0e300,
        f64::from(u32::MAX),
        4_294_967_296.0,
        f64::from(WB_MAX_COAST_OCTAVES) + 1.0,
        0.0,
        -0.0,
        -1.0,
        // Non-integral: 2.5 would TRUNCATE to 2, which is a silently-adjusted parameter, and
        // this boundary does not adjust.
        2.5,
        1.5,
        0.5,
        f64::EPSILON,
    ] {
        let mut record = base;
        record[COAST_OCTAVES_FIELD] = hostile;
        assert_eq!(
            wb_coast_check(record.as_ptr(), WB_COAST_STRIDE as u32),
            WB_ERR_PARAM,
            "an octave count of {hostile} is not a loop bound this boundary will pass on",
        );
        assert_eq!(world_with_coast(&record), 0);
    }
    // And every count it WILL pass on is walked, so "bounded" is a measurement rather than a
    // claim about a number. Sixteen worlds, each sampled at every probe.
    for count in 1..=WB_MAX_COAST_OCTAVES {
        let mut record = base;
        record[COAST_OCTAVES_FIELD] = f64::from(count);
        assert_eq!(
            wb_coast_check(record.as_ptr(), WB_COAST_STRIDE as u32),
            WB_OK,
            "octave count {count} is inside the ceiling and must be admitted",
        );
        sample_coast(&record, &format!("octaves {count}"));
    }
}

#[test]
fn the_finest_octave_frequency_is_bounded_as_a_product_no_per_field_ceiling_can_see() {
    // **THE BAND, AND IT IS NOT AT EITHER END.** `frequency`, `octaves` and `lacunarity` are each
    // inside their own documented domain at values whose PRODUCT is not: the finest octave is
    // `frequency * lacunarity^(octaves - 1)`, `Noise::at` floors that coordinate and casts to
    // `i64`, the cast saturates above ~9.22e18, and the very next line computes `ix + 1` --
    // which overflows. That is a panic under the test profile's overflow checks (an ABORT across
    // `extern "C"`) and a wrapped index into a different lattice cell in release (a world nobody
    // asked for). `WB_MAX_EROSION_NODES`'s doc asks a future caller to "bound the *product*";
    // this is that, taken.
    //
    // Each triple below has all three fields individually admissible.
    for (frequency, octaves, lacunarity) in [
        (WB_MAX_COAST_FINEST_FREQUENCY, 16.0, WB_MAX_COAST_LACUNARITY),
        (WB_MAX_COAST_FINEST_FREQUENCY, 2.0, 2.0),
        (1.0e5, 8.0, 4.0),
        (1.0, 16.0, WB_MAX_COAST_LACUNARITY),
        (1.0e3, 12.0, 8.0),
    ] {
        let mut record = coast_preset_record(WB_COAST_FRACTAL);
        record[2] = frequency;
        record[COAST_OCTAVES_FIELD] = octaves;
        record[5] = lacunarity;
        // **The premise, asserted rather than assumed: each field ALONE is admitted**, so this
        // record is refused for the product and for nothing else. "Alone" has to mean *with the
        // other two at their most permissive*, which is one octave at a lacunarity of one -- a
        // schedule whose finest band IS its base frequency. Asked against `fractal()` itself the
        // premise would be false and the test would be measuring the wrong thing: `fractal()`
        // carries four octaves at lacunarity 2, so a frequency of 1e6 there already asks for
        // 8e6 and is refused by the product before it is refused by anything else.
        let permissive = {
            let mut record = coast_preset_record(WB_COAST_FRACTAL);
            record[COAST_OCTAVES_FIELD] = 1.0;
            record[5] = WB_MIN_COAST_LACUNARITY;
            record
        };
        for (field, value) in [(2usize, frequency), (COAST_OCTAVES_FIELD, octaves), (5, lacunarity)]
        {
            let mut alone = permissive;
            alone[field] = value;
            assert_eq!(
                wb_coast_check(alone.as_ptr(), WB_COAST_STRIDE as u32),
                WB_OK,
                "field {field} = {value} must be admissible on its own for this to be a product \
                 test rather than a per-field one",
            );
        }
        assert_eq!(
            wb_coast_check(record.as_ptr(), WB_COAST_STRIDE as u32),
            WB_ERR_PARAM,
            "frequency {frequency} x lacunarity {lacunarity}^({octaves} - 1) is past the finest \
             frequency the noise lattice can index",
        );
        assert_eq!(world_with_coast(&record), 0);
    }
    // And the other side: a triple whose product lands just under the ceiling is ADMITTED and
    // walked, so the check is a bound rather than a blanket refusal of anything interesting.
    // 1e6 / 2^15 = 30.517578125, so fifteen doublings from there land exactly on the ceiling.
    let mut inside = coast_preset_record(WB_COAST_FRACTAL);
    inside[2] = WB_MAX_COAST_FINEST_FREQUENCY / 32_768.0;
    inside[COAST_OCTAVES_FIELD] = 16.0;
    inside[5] = 2.0;
    assert_eq!(wb_coast_check(inside.as_ptr(), WB_COAST_STRIDE as u32), WB_OK);
    sample_coast(&inside, "the finest schedule that fits");
}

#[test]
fn a_coast_term_that_would_do_nothing_anywhere_is_refused_rather_than_admitted() {
    // **The silently-dropping-builder shape, three doors onto it.** None of these is a crash and
    // none is a division by zero: each is a field present in the record, accepted by the
    // constructor, and contributing exactly nothing at every point on the planet -- with
    // `amplitude` sitting beside it looking configured. The engine's own guards are checked
    // against these refusals here rather than assumed to agree with them.
    let base = coast_preset_record(WB_COAST_FRACTAL);

    // 1. `coast_offset` computes `reach = spread * window_spreads` and closes the window when
    //    that is not positive.
    for silent in [0.0, -0.0, -1.0, -4.0, f64::NEG_INFINITY, f64::NAN, 5.0e-324, 1.0e-300] {
        let mut record = base;
        record[1] = silent;
        assert_eq!(
            wb_coast_check(record.as_ptr(), WB_COAST_STRIDE as u32),
            WB_ERR_PARAM,
            "a window width of {silent} is a term that never opens",
        );
        assert_eq!(world_with_coast(&record), 0);
    }

    // 2. A frequency below half a cycle over the unit sphere is a uniform DISPLACEMENT of every
    //    shore rather than a roughening of any of them.
    for silent in [0.0, -0.0, -1.0, -20.0, f64::NEG_INFINITY, f64::NAN, 1.0e-300, 0.25] {
        let mut record = base;
        record[2] = silent;
        assert_eq!(
            wb_coast_check(record.as_ptr(), WB_COAST_STRIDE as u32),
            WB_ERR_PARAM,
            "a frequency of {silent} cannot complete a cycle anywhere on the world",
        );
        assert_eq!(world_with_coast(&record), 0);
    }

    // 3. `fbm` with zero octaves returns exactly 0.0 -- `loudest == 0.0` -- so a zero-octave
    //    record is a coastal term that is read and answers nothing.
    let mut none = base;
    none[COAST_OCTAVES_FIELD] = 0.0;
    assert_eq!(wb_coast_check(none.as_ptr(), WB_COAST_STRIDE as u32), WB_ERR_PARAM);
    assert_eq!(world_with_coast(&none), 0);
}

#[test]
fn a_negative_coast_amplitude_is_refused_because_the_sign_is_not_a_second_parameter() {
    // The same statement `WB_MIN_MARGIN_WARP_M` makes about the warp: the coast lattice is
    // zero-mean, so a negative amplitude is the same displacement drawn from the negated field --
    // a second spelling of "how far", on a field whose whole meaning is how far.
    let base = coast_preset_record(WB_COAST_FRACTAL);
    for mirrored in [-f64::MIN_POSITIVE, -0.35, -1.0, -WB_MAX_COAST_AMPLITUDE, f64::NEG_INFINITY] {
        let mut record = base;
        record[0] = mirrored;
        assert_eq!(
            wb_coast_check(record.as_ptr(), WB_COAST_STRIDE as u32),
            WB_ERR_PARAM,
            "an amplitude of {mirrored} is outside this channel's stated domain",
        );
        assert_eq!(world_with_coast(&record), 0);
    }
    // `-0.0` is NOT refused, and that is deliberate rather than an oversight: `-0.0 >= 0.0` is
    // true in IEEE 754, `above_shore`'s guard is `amplitude == 0.0` which `-0.0` satisfies, and
    // the record is therefore exactly as inert as canonical. Refusing it would be refusing a
    // spelling of the canonical path.
    let mut negative_zero = base;
    negative_zero[0] = -0.0;
    assert_eq!(wb_coast_check(negative_zero.as_ptr(), WB_COAST_STRIDE as u32), WB_OK);
}

#[test]
fn the_coast_channel_default_path_is_the_untouched_world() {
    // RULING 1, and the one property this task is not allowed to break: a null pointer with a
    // length of zero is `None`, not `Some(canonical())`, and the world it builds is byte-for-byte
    // the world `wb_world_new` builds.
    //
    // **Unlike the tectonic channel, this is not a vacuous pairing.** `CoastParams::canonical()`
    // does not resolve through an `unwrap_or_else`: `above_shore` matches on the `Option` itself
    // and early-returns on a zero amplitude, so `None` and `Some(canonical())` reach the same
    // answer by two different routes and the second arm below is a real comparison.
    // `a_chosen_coast_block_actually_moves_the_ground_it_claims_to` is the discriminator that
    // stops this passing over a channel that ignores its argument.
    let plain = plain_world();
    let defaulted = wb_world_new_coast(
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
    );
    assert_ne!(defaulted, 0);
    let explicit_canonical = world_with_coast(&canonical_coast_record());
    assert_ne!(explicit_canonical, 0);

    for (lat, lon) in COAST_PROBES {
        for resolution in [RES_M, -1.0] {
            let expected = wb_elevation_m(plain, *lat, *lon, resolution);
            assert_eq!(
                wb_elevation_m(defaulted, *lat, *lon, resolution).to_bits(),
                expected.to_bits(),
                "the null coast path moved the world at ({lat}, {lon})",
            );
            assert_eq!(
                wb_elevation_m(explicit_canonical, *lat, *lon, resolution).to_bits(),
                expected.to_bits(),
                "an explicit canonical record moved the world at ({lat}, {lon})",
            );
        }
        let expected = wb_structural_m(plain, *lat, *lon);
        assert_eq!(wb_structural_m(defaulted, *lat, *lon).to_bits(), expected.to_bits());
        assert_eq!(wb_structural_m(explicit_canonical, *lat, *lon).to_bits(), expected.to_bits());
    }
    for handle in [plain, defaulted, explicit_canonical] {
        assert_eq!(wb_world_free(handle), WB_OK);
    }
}

#[test]
fn a_chosen_coast_block_actually_moves_the_ground_it_claims_to() {
    // The whole point of the task, asserted at the boundary rather than three modules down.
    // Without this, every test above would pass over a channel that decoded six f64 and threw
    // them away.
    let canonical = sample_coast(&canonical_coast_record(), "canonical");
    let fractal = sample_coast(&coast_preset_record(WB_COAST_FRACTAL), "fractal");
    assert_eq!(fractal.len(), canonical.len());
    let moved = fractal
        .iter()
        .zip(&canonical)
        .filter(|(a, b)| a.to_bits() != b.to_bits())
        .count();
    // Eight of the eleven probes are coastal witnesses and were derived as the largest movers on
    // this world, so a majority moving is the claim rather than "at least one".
    assert!(moved >= 8, "the fractal preset moved only {moved} of {} probes", fractal.len());

    // And the amplitude is the knob, separately: it is the ONE field `fractal()` moves, so a
    // channel that read the other five and dropped this one would still pass the pairing above.
    let mut half = coast_preset_record(WB_COAST_FRACTAL);
    half[0] /= 2.0;
    let halved = sample_coast(&half, "half amplitude");
    assert!(
        halved.iter().zip(&fractal).any(|(a, b)| a.to_bits() != b.to_bits()),
        "halving the amplitude changed nothing at any probe",
    );

    // And the frequency, which is the field the canonical base cannot see at all.
    let mut coarse = coast_preset_record(WB_COAST_FRACTAL);
    coarse[2] /= 4.0;
    let coarsened = sample_coast(&coarse, "quarter frequency");
    assert!(
        coarsened.iter().zip(&fractal).any(|(a, b)| a.to_bits() != b.to_bits()),
        "quartering the frequency changed nothing at any probe",
    );
}

#[test]
fn the_coastal_term_is_not_a_global_one() {
    // The term is windowed by `|above_shore|`, and the window is the property that keeps land
    // fraction where the calibrator put it. **A term that moved every point on the planet would
    // satisfy every other test in this file** -- including the one above, which only asks that
    // points move. So the complement is asserted: over a 4-degree global grid there must be a
    // substantial population the fractal preset leaves BIT-IDENTICAL.
    let canonical = world_with_coast(&canonical_coast_record());
    let fractal = world_with_coast(&coast_preset_record(WB_COAST_FRACTAL));
    assert_ne!(canonical, 0);
    assert_ne!(fractal, 0);
    let mut same = 0usize;
    let mut moved = 0usize;
    let mut latitude = -88.0;
    while latitude <= 88.0 {
        let mut longitude = -180.0;
        while longitude < 180.0 {
            let a = wb_elevation_m(canonical, latitude, longitude, RES_M);
            let b = wb_elevation_m(fractal, latitude, longitude, RES_M);
            if a.to_bits() == b.to_bits() {
                same += 1;
            } else {
                moved += 1;
            }
            longitude += 4.0;
        }
        latitude += 4.0;
    }
    assert!(same > 1_000, "only {same} of {} grid points were left alone", same + moved);
    assert!(moved > 1_000, "only {moved} of {} grid points moved at all", same + moved);
    assert_eq!(wb_world_free(canonical), WB_OK);
    assert_eq!(wb_world_free(fractal), WB_OK);
}

#[test]
fn wb_coast_preset_hands_back_the_engines_own_blocks_and_nothing_else() {
    // The preset export exists so no host transcribes a default or a preset. It must therefore
    // BE them, in `WB_COAST_STRIDE` order, compared against the module's own values rather than
    // against a third copy written here.
    for (selector, expected) in
        [(WB_COAST_CANONICAL, CoastParams::canonical()), (WB_COAST_FRACTAL, CoastParams::fractal())]
    {
        let record = coast_preset_record(selector);
        assert_eq!(record[0].to_bits(), expected.amplitude.to_bits());
        assert_eq!(record[1].to_bits(), expected.window_spreads.to_bits());
        assert_eq!(record[2].to_bits(), expected.frequency.to_bits());
        assert_eq!(record[3].to_bits(), f64::from(expected.octaves).to_bits());
        assert_eq!(record[4].to_bits(), expected.gain.to_bits());
        assert_eq!(record[5].to_bits(), expected.lacunarity.to_bits());
        // Every preset must be a record this channel would accept. A preset the checker refuses
        // is a button that produces a blank viewer.
        assert_eq!(wb_coast_check(record.as_ptr(), WB_COAST_STRIDE as u32), WB_OK);
    }
    // `fractal()` moves exactly ONE field off canonical, which is what makes the panel's single
    // slider an honest presentation of it.
    let canonical = canonical_coast_record();
    let fractal = coast_preset_record(WB_COAST_FRACTAL);
    let differing =
        (0..WB_COAST_STRIDE).filter(|i| canonical[*i].to_bits() != fractal[*i].to_bits()).count();
    assert_eq!(differing, 1, "fractal() moves {differing} fields, not one");
    assert_ne!(fractal[0].to_bits(), canonical[0].to_bits(), "and the one field is the amplitude");

    // An unknown selector is a parameter error and writes nothing.
    let mut out = [7.0; WB_COAST_STRIDE];
    for unknown in [2u32, 3, u32::MAX] {
        assert_eq!(
            wb_coast_preset(unknown, out.as_mut_ptr(), WB_COAST_STRIDE as u32),
            WB_ERR_PARAM,
        );
    }
    assert!(out.iter().all(|v| *v == 7.0), "a refused selector wrote into the buffer");
}

#[test]
fn every_amplitude_the_panel_slider_can_reach_is_a_block_the_engine_accepts() {
    // **The panel's travel, held against the real validator.** `viewer/public/app/coast-params.js`
    // maps an integer slider position to `canonical.amplitude + position / 20`, from 0 to 15 --
    // 0.00 to 0.75, the top of the band Task 5 measured as useful. A control most of whose travel
    // the engine refuses is worse than no control, and the tectonic channel's own version of this
    // test is what caught `margin_warp_m` having one live position.
    //
    // The arithmetic is restated here rather than imported because a Rust test cannot import a JS
    // module; `viewer/test/coast-params.test.mjs` holds the JS side against the shipped `.wasm`
    // and this holds the same lattice against the native build, so a drift between them fails on
    // one side or the other.
    let canonical = canonical_coast_record();
    for position in 0..=15 {
        let mut record = canonical;
        record[0] = canonical[0] + f64::from(position) / 20.0;
        assert_eq!(
            wb_coast_check(record.as_ptr(), WB_COAST_STRIDE as u32),
            WB_OK,
            "slider position {position} asks for an amplitude the engine refuses",
        );
    }
    // Position 0 must be canonical BIT-FOR-BIT, or an untouched panel writes an amplitude into
    // every shared link and takes the reload off the engine's `None` path.
    assert_eq!((canonical[0] + 0.0 / 20.0).to_bits(), canonical[0].to_bits());
    // And the preset's own value must land ON the lattice, or the slider cannot express the
    // number its own preset button sets -- the panel-default defect this viewer has shipped four
    // times. `7 / 20` and not `7 * 0.05`: the second is one ULP out.
    assert_eq!(
        (canonical[0] + 7.0 / 20.0).to_bits(),
        coast_preset_record(WB_COAST_FRACTAL)[0].to_bits(),
    );
}

#[test]
fn the_coast_buffer_channel_refuses_what_it_cannot_read() {
    let record = canonical_coast_record();
    // Null with a length is a caller mistake, not a request for canonical.
    assert_eq!(wb_coast_check(core::ptr::null(), WB_COAST_STRIDE as u32), WB_ERR_BUFFER);
    // Non-null with a length of zero is a host that computed a length wrong.
    assert_eq!(wb_coast_check(record.as_ptr(), 0), WB_ERR_BUFFER);
    // Null with zero IS canonical.
    assert_eq!(wb_coast_check(core::ptr::null(), 0), WB_OK);
    for length in [1u32, 5, 7, 8, 10, 16, u32::MAX] {
        assert_eq!(
            wb_coast_check(record.as_ptr(), length),
            WB_ERR_BUFFER,
            "a {length}-word coast record is not a coast record",
        );
    }
    // Misaligned: one byte into an f64-sized buffer.
    let mut bytes = [0u8; WB_COAST_STRIDE * 8 + 8];
    let misaligned = unsafe { bytes.as_mut_ptr().add(1) } as *const f64; // cast-ok: a deliberately misaligned pointer for the alignment check
    assert_eq!(wb_coast_check(misaligned, WB_COAST_STRIDE as u32), WB_ERR_BUFFER);

    // The constructor refuses the same things, with a handle of 0 rather than a status.
    for (ptr, len) in [
        (core::ptr::null(), WB_COAST_STRIDE as u32),
        (record.as_ptr(), 0u32),
        (record.as_ptr(), 4u32),
    ] {
        assert_eq!(
            wb_world_new_coast(
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
                ptr,
                len,
            ),
            0,
        );
    }
    // A bad buffer for the OUT parameter of the preset export.
    let mut out = [0.0; WB_COAST_STRIDE];
    assert_eq!(
        wb_coast_preset(WB_COAST_CANONICAL, core::ptr::null_mut(), WB_COAST_STRIDE as u32),
        WB_ERR_BUFFER,
    );
    assert_eq!(wb_coast_preset(WB_COAST_CANONICAL, out.as_mut_ptr(), 0), WB_ERR_BUFFER);
    assert_eq!(wb_coast_preset(WB_COAST_CANONICAL, out.as_mut_ptr(), 5), WB_ERR_BUFFER);
}

#[test]
fn the_fourth_door_carries_the_other_three_blocks_unchanged() {
    // `wb_world_new_coast` is `wb_world_new_tectonic` plus one record, and all four doors are one
    // `build_world` behind the boundary. **That is a claim about the other three channels still
    // working through this one**, and a channel that dropped its relief or tectonic argument on
    // the way through would look identical from every test above.
    let relief = {
        let mut record = [0.0; WB_RELIEF_STRIDE];
        assert_eq!(
            wb_relief_preset(WB_RELIEF_HILLS, record.as_mut_ptr(), WB_RELIEF_STRIDE as u32),
            WB_OK,
        );
        record
    };
    let tectonics = tectonic_preset_record(WB_TECTONIC_RANGES);
    let coast = coast_preset_record(WB_COAST_FRACTAL);

    let through_the_third = wb_world_new_tectonic(
        SEED,
        RADIUS_M,
        PLATES,
        LAND,
        core::ptr::null(),
        0,
        relief.as_ptr(),
        WB_RELIEF_STRIDE as u32,
        tectonics.as_ptr(),
        WB_TECTONIC_STRIDE as u32,
    );
    let through_the_fourth = wb_world_new_coast(
        SEED,
        RADIUS_M,
        PLATES,
        LAND,
        core::ptr::null(),
        0,
        relief.as_ptr(),
        WB_RELIEF_STRIDE as u32,
        tectonics.as_ptr(),
        WB_TECTONIC_STRIDE as u32,
        core::ptr::null(),
        0,
    );
    let with_coast = wb_world_new_coast(
        SEED,
        RADIUS_M,
        PLATES,
        LAND,
        core::ptr::null(),
        0,
        relief.as_ptr(),
        WB_RELIEF_STRIDE as u32,
        tectonics.as_ptr(),
        WB_TECTONIC_STRIDE as u32,
        coast.as_ptr(),
        WB_COAST_STRIDE as u32,
    );
    assert_ne!(through_the_third, 0);
    assert_ne!(through_the_fourth, 0);
    assert_ne!(with_coast, 0);

    let mut differs = 0usize;
    for (lat, lon) in COAST_PROBES {
        let third = wb_elevation_m(through_the_third, *lat, *lon, RES_M);
        assert_eq!(
            wb_elevation_m(through_the_fourth, *lat, *lon, RES_M).to_bits(),
            third.to_bits(),
            "the fourth door with a null coast is not the third door at ({lat}, {lon})",
        );
        if wb_elevation_m(with_coast, *lat, *lon, RES_M).to_bits() != third.to_bits() {
            differs += 1;
        }
    }
    // And the coast block still bites when the other two are non-canonical, which is the
    // interaction a single-channel test cannot see.
    assert!(differs >= 8, "the coast block moved only {differs} probes under a relief+tectonic world");
    for handle in [through_the_third, through_the_fourth, with_coast] {
        assert_eq!(wb_world_free(handle), WB_OK);
    }
}

#[test]
fn the_octave_schedule_is_swept_as_a_cross_product_because_the_hazard_is_a_product() {
    // **A one-field-at-a-time sweep cannot find this channel's abort, and that is not a
    // hypothetical.** `swept_coast_records` moves one word off a base and leaves the rest alone,
    // so the largest finest-octave frequency it ever asks for is `1e6 * 2^3 = 8e6` (the top of
    // the `frequency` ladder against `fractal()`'s four octaves) -- twelve orders below the
    // `i64` saturation in `Noise::at` that ends in an `ix + 1` overflow. The band lives where
    // three fields meet, and only a cross product reaches it.
    //
    // **Measured, not argued.** With the product check removed from `coast_is_admissible` and
    // nothing else changed, this test ABORTS -- `attempt to add with overflow`, `noise.rs:95`,
    // inside `Noise::at`, reached through `wb_elevation_m`. The single-field sweep above stays
    // green under the same mutation. That is recorded in the task report's mutation table.
    let base = coast_preset_record(WB_COAST_FRACTAL);
    let mut accepted = 0usize;
    let mut refused = 0usize;
    for frequency in [WB_MIN_COAST_FREQUENCY, 20.0, 1.0e3, 1.0e5, WB_MAX_COAST_FINEST_FREQUENCY] {
        for octaves in [1.0, 2.0, 4.0, 8.0, 12.0, f64::from(WB_MAX_COAST_OCTAVES)] {
            for lacunarity in [
                WB_MIN_COAST_LACUNARITY,
                2.0,
                4.0,
                8.0,
                WB_MAX_COAST_LACUNARITY,
            ] {
                let mut record = base;
                record[2] = frequency;
                record[COAST_OCTAVES_FIELD] = octaves;
                record[5] = lacunarity;
                let label = format!("f={frequency:e} o={octaves} l={lacunarity}");
                if wb_coast_check(record.as_ptr(), WB_COAST_STRIDE as u32) == WB_OK {
                    sample_coast(&record, &label);
                    accepted += 1;
                } else {
                    assert_eq!(world_with_coast(&record), 0, "checker refused, constructor built: {label}");
                    refused += 1;
                }
            }
        }
    }
    assert_eq!(accepted + refused, 5 * 6 * 5);
    // Neither side trivial: a grid that refused everything would pass "nothing aborted" for
    // free, and one that accepted everything would prove the product check absent.
    assert!(accepted >= 20, "only {accepted} of {} schedules were accepted", accepted + refused);
    assert!(refused >= 20, "only {refused} of {} schedules were refused", accepted + refused);
}

#[test]
fn the_amplitude_and_the_window_are_swept_together_not_one_at_a_time() {
    // The other pair on this channel that interacts: `coast_offset` multiplies
    // `amplitude * spread * window(|above_shore| / (spread * window_spreads))`, so the amplitude
    // decides how far the coast moves and the window decides over how wide a band it is allowed
    // to -- and the two together decide whether the displacement can carry a point out of the
    // band the window drew for it. A sweep of either alone rides the other at its preset value.
    //
    // Every combination is admissible by construction (both ladders are inside their own
    // domains), so what this test watches for is an abort or a non-finite sample, which
    // `sample_coast` asserts at every probe.
    let base = coast_preset_record(WB_COAST_FRACTAL);
    let mut built = 0usize;
    for amplitude in [
        WB_MIN_COAST_AMPLITUDE,
        0.1,
        0.35,
        0.75,
        1.5,
        WB_MAX_COAST_AMPLITUDE,
    ] {
        for window in [
            WB_MIN_COAST_WINDOW_SPREADS,
            0.01,
            0.25,
            1.0,
            2.0,
            WB_MAX_COAST_WINDOW_SPREADS,
        ] {
            let mut record = base;
            record[0] = amplitude;
            record[1] = window;
            assert_eq!(
                wb_coast_check(record.as_ptr(), WB_COAST_STRIDE as u32),
                WB_OK,
                "amplitude {amplitude} at window {window} is inside both domains and must be \
                 admitted",
            );
            sample_coast(&record, &format!("a={amplitude} w={window}"));
            built += 1;
        }
    }
    assert_eq!(built, 36);
}

#[test]
fn a_gain_above_one_is_refused_because_it_runs_the_octave_schedule_backwards_and_ends_in_a_nan() {
    // **The band, measured.** `Noise::fbm` multiplies its running amplitude by `gain` each
    // octave; far enough above one the amplitude overflows to `+inf`, `loudest` overflows with
    // it, and `2.0 * total / loudest` is `inf / inf` -- a NaN in `above_shore`. That NaN used
    // **not** to surface as a NaN: `elevation_from_above` failed both of its comparisons and
    // returned the abyssal floor, so the planet drowned silently and every finiteness assertion
    // stayed green. `elevation_from_above` now propagates instead; the refusal here stays because
    // it names the offending FIELD, which a NaN elevation cannot. See
    // `a_nan_in_the_coastal_term_surfaces_as_a_nan_instead_of_drowning_the_world`.
    //
    // Measured on the coast lattice at `frequency = 20`, `lacunarity = 2`, this host:
    //
    //     gain      1e20   1e21   1e50   1e102   1e103   1e300
    //     4 octaves  fin    fin    fin     fin     NaN     NaN
    //     16 octaves fin    NaN    NaN     NaN     NaN     NaN
    //
    // **A band whose edge moves with another field**, which is the shape every hazard this
    // project has found has had, and the reason nothing here is spot-checked: a probe at
    // `gain = 1e20` finds nothing, one at `1e100` finds nothing at four octaves and a NaN at
    // sixteen, and the value in between is where the edge actually is.
    //
    // The ceiling is drawn at 1.0 rather than at the measured edge because above one the
    // parameter has already stopped meaning what its name says -- each octave louder than the
    // last -- and a domain the caller cannot use is not worth the eighteen orders of margin.
    let base = coast_preset_record(WB_COAST_FRACTAL);
    for hostile in [
        WB_MAX_COAST_GAIN + f64::EPSILON,
        1.5,
        2.0,
        1.0e20,
        1.0e21,
        1.0e103,
        1.0e300,
        f64::MAX,
        f64::INFINITY,
        f64::NAN,
        -f64::MIN_POSITIVE,
        -0.5,
        f64::NEG_INFINITY,
    ] {
        let mut record = base;
        record[4] = hostile;
        assert_eq!(
            wb_coast_check(record.as_ptr(), WB_COAST_STRIDE as u32),
            WB_ERR_PARAM,
            "a gain of {hostile} is outside this channel's stated domain",
        );
        assert_eq!(world_with_coast(&record), 0);
    }
    // And every gain it does admit is walked, including the two ends. Zero is admitted and is
    // NOT a silence: at `gain = 0` the first octave carries its full amplitude and `loudest`
    // is 1, so the term still acts -- it is simply one octave wearing four octaves' name.
    for gain in [0.0, 0.25, 0.5, 0.75, WB_MAX_COAST_GAIN] {
        let mut record = base;
        record[4] = gain;
        assert_eq!(wb_coast_check(record.as_ptr(), WB_COAST_STRIDE as u32), WB_OK);
        sample_coast(&record, &format!("gain {gain}"));
    }
}

#[test]
fn a_nan_in_the_coastal_term_surfaces_as_a_nan_instead_of_drowning_the_world() {
    // **This test was written the other way up, and the engine changed under it.**
    //
    // It used to read `assert_eq!(nan_seen, 0, "the NaN surfaced after all -- this test's
    // premise has changed")`, pinning the defect the coastline sweep found: a NaN in
    // `above_shore` did NOT appear as a NaN anywhere a caller could see. It appeared as a
    // planet whose coastal band sat uniformly on the abyssal floor, passing every health
    // check this crate had, because `Continentality::elevation_from_above` read
    // `if above >= 0.0` and then `if depth < 1.0` and **a NaN was false for both**, so it fell
    // through to `ABYSS_M * 1.0`.
    //
    // `elevation_from_above` now carries an explicit `is_nan` arm and propagates instead. The
    // premise line did its job: it went red on this commit, naming itself, rather than the
    // change slipping through a test that had nothing to say about it. Both halves are kept
    // and both are now the other way round.
    //
    // **`WB_MAX_COAST_GAIN` stays a refusal, and the guard does not make it redundant.** A
    // refusal tells a host *which field* is wrong through `wb_coast_check`'s status; a NaN
    // elevation only tells it that something is. The guard is the second line, for a term
    // nobody has written yet.
    //
    // This is reached through the ENGINE rather than through the export, deliberately: the
    // boundary refuses every gain that can produce it, so there is no record
    // `wb_world_new_coast` will accept that gets here.
    let drowning = CoastParams { gain: 1.0e300, ..CoastParams::fractal() };
    // The boundary refuses it. If this ever stops being true, the rest of the test is the
    // description of what a host would then be able to build.
    let mut record = coast_preset_record(WB_COAST_FRACTAL);
    record[4] = drowning.gain;
    assert_eq!(wb_coast_check(record.as_ptr(), WB_COAST_STRIDE as u32), WB_ERR_PARAM);

    let surface =
        Surface::with_coast(SEED, RADIUS_M, PLATES as usize, LAND, None, None, None, Some(drowning));
    // The same field the surface above built, rebuilt here because `Shelf::land` is
    // `pub(crate)` and this file is an integration test. `SEED as u64` is exactly what
    // `Surface::new` does with its `i64` world seed -- a two's-complement reinterpretation.
    let seed = SEED as u64; // cast-ok: two's-complement reinterpretation, matching Surface::new
    let land = Continentality::with_coast(seed, RADIUS_M, LAND, Some(drowning));

    // **The claim, probe by probe: a NaN `above_shore` must come out as a NaN elevation.**
    // Written as a per-probe correspondence rather than as two counts, because the failure it
    // guards against is precisely a NaN that arrives looking like a legitimate depth -- and
    // one of these eleven probes IS legitimately below -4,000 m, so any assertion phrased on
    // "how many probes are deep" would be comparing against a number the fixture reaches on
    // its own.
    let mut poisoned = 0usize;
    let mut clean = 0usize;
    let mut drowned = Vec::new();
    for (lat, lon) in COAST_PROBES {
        let point = SpherePoint::from_latlon(*lat, *lon);
        let elevation = surface.elevation_m(&point, Some(RES_M));
        if land.above_shore(&point).is_nan() {
            poisoned += 1;
            if !elevation.is_nan() {
                drowned.push((*lat, *lon, elevation));
            }
        } else {
            clean += 1;
            assert!(
                elevation.is_finite(),
                "a probe outside the coastal band went non-finite at {lat},{lon}: {elevation}",
            );
        }
    }
    assert!(
        drowned.is_empty(),
        "the silent abyss is reachable again -- these probes have a NaN above_shore and a          plausible elevation: {drowned:?}",
    );
    // **Both populations are non-empty, and the split is pinned exactly.** The coastal term is
    // windowed by `|above_shore| <= window_spreads * spread`, so a probe outside the band never
    // samples the overflowing fBm and has no NaN to surface. Pinned as a number rather than as
    // `>= len - 1`: a threshold would let a second probe drift out of the band unnoticed, and a
    // shrinking population is the failure this project keeps finding.
    assert_eq!((poisoned, clean), (10, 1), "the probe population moved relative to the band");
    // Before the guard, every one of the ten read between -4,599 m and -4,625 m -- ordinary
    // abyssal ground, indistinguishable from the eleventh, which really is that deep.
    let genuinely_deep = COAST_PROBES
        .iter()
        .filter(|(lat, lon)| {
            let p = SpherePoint::from_latlon(*lat, *lon);
            !land.above_shore(&p).is_nan() && surface.elevation_m(&p, Some(RES_M)) < -4_000.0
        })
        .count();
    assert_eq!(
        genuinely_deep, 1,
        "the value the defect produced must be one this fixture also produces honestly, or          the assertions above are discriminating against nothing",
    );

    // And the same world at a canonical block is ordinary ground at every one of these
    // probes -- so the two assertions above are discriminating between two outcomes this
    // fixture can actually produce, rather than describing the only thing it ever does.
    let ordinary =
        Surface::with_coast(SEED, RADIUS_M, PLATES as usize, LAND, None, None, None, None);
    for (lat, lon) in COAST_PROBES {
        let point = SpherePoint::from_latlon(*lat, *lon);
        let elevation = ordinary.elevation_m(&point, Some(RES_M));
        assert!(elevation.is_finite(), "the canonical world is not finite at {lat},{lon}");
    }
}

// ---------------------------------------------------------------- the gully channel
//
// The fifth block channel, and the first that ADDS a term to `elevation_m`. Everything below
// is the coast channel's shape: a preset read across the boundary rather than transcribed, a
// per-field ladder over the documented domain and past both ends of it, a checker held to the
// constructor over the identical population, and a CROSS-PRODUCT sweep -- because this
// project has already had one abort that was reachable only through the product of three
// individually-admissible fields while a one-field-at-a-time sweep stayed green.

/// Where the gully gate is open on the fixture world, so the sweep exercises the kernel
/// rather than an early return.
///
/// **A scatter over a planet does not land on a flank.** These are the sites the steepest
/// decile of high ground actually occupies on `SEED` at 12 plates -- printed by a throwaway
/// that walked 400,000 spiral points and rounded to a quarter degree, with the rounded site's
/// own `structural_m` re-checked so each literal is the value the test will meet. Every one is
/// far above `GullyParams::drainage()`'s 200 m gate, so a record that reaches the kernel is
/// evaluated by it. The last three are deliberately NOT gated -- open ocean, the harbour, and
/// the equator -- because a validator that only ever saw open-gate ground would not exercise
/// the shut-gate early return at all.
const GULLY_PROBES: &[(f64, f64)] = &[
    (-9.00, 65.25),  // structural 1,256 m, 2 km slope 0.01602
    (-8.75, 64.75),  // 1,260 m, 0.01555
    (-8.50, 64.50),  // 1,366 m, 0.01376
    (-8.75, 65.00),  // 1,482 m, 0.01356
    (-9.00, 65.75),  // 1,351 m, 0.01327
    (-8.75, 65.50),  // 1,685 m, 0.01143
    (-8.25, 64.50),  // 1,585 m, 0.00928
    (-9.25, 66.00),  // 1,059 m, 0.00915
    (0.0, 0.0),
    (12.0, 34.0),
    (-18.25, 121.5), // the harbour
];

/// A named preset, read across the boundary exactly as a host reads it. **Nothing in this file
/// writes a gully value down** -- above all not `slope_reference`, the one number in the record
/// that is a measurement of this generator rather than a preference.
fn gully_preset_record(selector: u32) -> [f64; WB_GULLY_STRIDE] {
    let mut record = [0.0; WB_GULLY_STRIDE];
    let status = wb_gully_preset(selector, record.as_mut_ptr(), WB_GULLY_STRIDE as u32);
    assert_eq!(status, WB_OK, "gully preset {selector} must be readable");
    record
}

fn world_with_gully(record: &[f64; WB_GULLY_STRIDE]) -> u32 {
    wb_world_new_gully(
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
        record.as_ptr(),
        WB_GULLY_STRIDE as u32,
    )
}

/// Build the world a gully record asks for, walk every probe point, and free it.
///
/// **This is where an abort would happen, and that is the point of calling it.**
/// `Surface::with_gully` merely stores the block and builds a lattice; it is `elevation_m`
/// that reads it, once per sample. A constructor that returned a handle has proved nothing
/// about the record it was given -- the lattice index, the pivot window, the cosine's argument
/// and `powf`'s exponent are all reached here and nowhere earlier.
fn sample_gully(record: &[f64; WB_GULLY_STRIDE], label: &str) {
    let handle = world_with_gully(record);
    assert_ne!(handle, 0, "accepted record refused by the constructor: {label} {record:?}");
    for (lat, lon) in GULLY_PROBES {
        for resolution in [RES_M, 76.35, 5_000.0] {
            let height = wb_elevation_m(handle, *lat, *lon, resolution);
            assert!(
                height.is_finite(),
                "accepted record produced a non-finite elevation at ({lat}, {lon}) at \
                 resolution {resolution}: {label} {record:?}",
            );
        }
        // The gully term must not have reached `structural_m`: it is detail, and detail is
        // defined as the thing structure does not see. A record that moved this would be a
        // term that had escaped its layer.
        let structural = wb_structural_m(handle, *lat, *lon);
        assert!(
            structural.is_finite(),
            "accepted record gave a non-finite structural at ({lat}, {lon}): {label} {record:?}",
        );
    }
    assert_eq!(wb_world_free(handle), WB_OK);
}

/// The documented domain of each gully field, by its index in `WB_GULLY_STRIDE`'s order.
/// Written as the constants rather than as numbers: a test that restates a bound cannot
/// notice it moving.
fn gully_field_domain(field: usize) -> (f64, f64) {
    match field {
        0 => (0.0, WB_MAX_GULLY_AMPLITUDE_M),
        1 => (WB_MIN_GULLY_LENGTH_M, WB_MAX_GULLY_LENGTH_M),
        2 => (WB_MIN_GULLY_SLOPE_REFERENCE, WB_MAX_GULLY_SLOPE_REFERENCE),
        3 => (0.0, WB_MAX_GULLY_STRIPES),
        4 => (0.0, WB_MAX_GULLY_STRIPES),
        5 => (WB_MIN_GULLY_SHARPNESS, WB_MAX_GULLY_SHARPNESS),
        6 => (-WB_MAX_GULLY_GATE_ELEVATION_M, WB_MAX_GULLY_GATE_ELEVATION_M),
        7 => (WB_MIN_GULLY_GATE_SPAN_M, WB_MAX_GULLY_GATE_SPAN_M),
        8 => (0.0, 1.0),
        9 => (WB_MIN_GULLY_LENGTH_M, WB_MAX_GULLY_LENGTH_M),
        10 => (0.0, WB_MAX_GULLY_HARMONIC_WEIGHT),
        11 => (WB_MIN_GULLY_HARMONIC_BAND_M, WB_MAX_GULLY_HARMONIC_BAND_M),
        _ => unreachable!("WB_GULLY_STRIDE is 12"),
    }
}

/// Every value one gully field is driven through: `HOSTILE` in full, both documented bounds
/// and the values immediately either side of each, and a ladder across the admissible
/// interval -- geometric where the domain spans orders of magnitude and linear where it does
/// not. Same construction as `coast_field_sweep`, and for the same stated reason: **every
/// hazard this project has found was a band, not a cliff.**
fn gully_field_sweep(field: usize) -> Vec<f64> {
    let (low, high) = gully_field_domain(field);
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

/// The two bases every gully field is swept around.
///
/// **One base is not a sweep of this channel**, and the reason is the sharpest of the four
/// channels: `GullyParams::canonical()` carries `amplitude_m = 0.0`, and both
/// `Surface::with_gully` (which then builds no lattice) and `Detail::gully_offset_m` branch on
/// exactly that before anything else is read. Around canonical, **nine of the ten fields are
/// swept with the code that reads them switched off**. Around `drainage()` every one is live.
///
/// The canonical base is kept anyway: it is the base a host reaches by moving the amplitude
/// slider off zero, and the validator is exercised there even where the kernel is not.
fn gully_sweep_bases() -> [(&'static str, [f64; WB_GULLY_STRIDE]); 2] {
    [
        ("canonical", gully_preset_record(WB_GULLY_CANONICAL)),
        ("drainage", gully_preset_record(WB_GULLY_DRAINAGE)),
    ]
}

fn swept_gully_records() -> Vec<(String, [f64; WB_GULLY_STRIDE])> {
    let mut out = Vec::new();
    for (base_name, base) in gully_sweep_bases() {
        for field in 0..WB_GULLY_STRIDE {
            for value in gully_field_sweep(field) {
                let mut record = base;
                record[field] = value;
                out.push((format!("{base_name} + gully field {field} = {value:e}"), record));
            }
        }
    }
    out
}

#[test]
fn every_gully_field_swept_across_its_whole_range_and_beyond_never_aborts() {
    let records = swept_gully_records();
    // A sweep that refused everything would pass a "nothing aborted" assertion trivially, and
    // one that accepted everything would prove the validator absent. Both counts are asserted.
    let mut accepted = 0usize;
    let mut refused = 0usize;
    for (label, record) in &records {
        if wb_gully_check(record.as_ptr(), WB_GULLY_STRIDE as u32) == WB_OK {
            sample_gully(record, label);
            accepted += 1;
        } else {
            refused += 1;
        }
    }
    assert_eq!(accepted + refused, records.len());
    assert!(
        accepted >= 100,
        "only {accepted} records were accepted; the sweep is not exercising the engine",
    );
    assert!(
        refused >= 100,
        "only {refused} records were refused; the validator is not doing its job",
    );
}

#[test]
fn the_gully_checker_and_the_constructor_agree_on_every_swept_record() {
    // Two validators would be two chances to disagree, and the disagreement that matters is
    // "the checker said yes and the constructor aborted".
    for (label, record) in swept_gully_records() {
        let status = wb_gully_check(record.as_ptr(), WB_GULLY_STRIDE as u32);
        let handle = world_with_gully(&record);
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
fn a_non_positive_crest_sharpness_would_return_an_infinite_height_and_is_refused() {
    // **The one bound on this channel that closes a hazard rather than stating a domain, and
    // the mechanism is written out because a test that only checked the refusal would not say
    // why the refusal matters.**
    //
    // The edge shaping is `1 - 2 * folded^crest_sharpness`, and `folded` is EXACTLY zero at a
    // crest. `0^s` is `+inf` for every negative `s`, so one negative f64 in word 5 turns every
    // gully crest in the world into an infinite height -- across a nounwind boundary, into a
    // host's vertex buffer, as a plausible-looking f64 that is not one.
    //
    // The refusal is asserted here; the arithmetic behind it is demonstrated directly below,
    // on the engine side where the exponent can still be applied, so this test cannot become a
    // tautology if the hazard ever stops being one.
    let base = gully_preset_record(WB_GULLY_DRAINAGE);
    for hostile in [-0.5, -1.0, 0.0, -0.0, -f64::MIN_POSITIVE, f64::NAN, f64::NEG_INFINITY] {
        let mut record = base;
        record[5] = hostile;
        assert_eq!(
            wb_gully_check(record.as_ptr(), WB_GULLY_STRIDE as u32),
            WB_ERR_PARAM,
            "a crest sharpness of {hostile} must be refused",
        );
        assert_eq!(world_with_gully(&record), 0, "and the constructor must refuse it too");
    }
    // The hazard, demonstrated: at a crest the folded signal is zero, and zero to a negative
    // power is an infinity. This is arithmetic rather than a claim about the kernel, and it is
    // what the bound above is protecting a host from.
    assert!(worldbuilder_engine::detmath::powf(0.0, -0.5).is_infinite());
    assert!(worldbuilder_engine::detmath::powf(0.0, WB_MIN_GULLY_SHARPNESS).is_finite());
}

#[test]
fn the_lattice_lengths_are_swept_as_a_cross_product_because_the_index_is_a_quotient() {
    // **A one-field-at-a-time sweep is blind to a product by construction**, and this file
    // already carries one abort that was reachable only that way: `WB_MAX_COAST_FINEST_FREQUENCY`
    // exists because three individually-admissible coastal fields compounded past `Noise::at`'s
    // `i64` lattice index.
    //
    // This channel has the same shape twice over. Both `cell_m` and `steer_lattice_m` become a
    // lattice index as `radius / length`, and `slope_reference` sets how large the phase that
    // index feeds can get. So the three are swept TOGETHER, at both ends of each and at the
    // interior values between them, and every accepted combination is sampled on gated ground
    // where all three are live. 7 x 7 x 6 = 294 records.
    let base = gully_preset_record(WB_GULLY_DRAINAGE);
    let lengths = [
        WB_MIN_GULLY_LENGTH_M,
        WB_MIN_GULLY_LENGTH_M * 10.0,
        250.0,
        1_000.0,
        100_000.0,
        WB_MAX_GULLY_LENGTH_M * 0.5,
        WB_MAX_GULLY_LENGTH_M,
    ];
    let references = [
        WB_MIN_GULLY_SLOPE_REFERENCE,
        1.0e-6,
        1.0e-3,
        0.005,
        1.0,
        WB_MAX_GULLY_SLOPE_REFERENCE,
    ];
    let mut accepted = 0usize;
    let mut refused = 0usize;
    let mut records = 0usize;
    for cell in lengths {
        for steer in lengths {
            for reference in references {
                let mut record = base;
                record[1] = cell;
                record[2] = reference;
                record[9] = steer;
                records += 1;
                let label =
                    format!("cell {cell:e} x steer {steer:e} x reference {reference:e}");
                if wb_gully_check(record.as_ptr(), WB_GULLY_STRIDE as u32) == WB_OK {
                    sample_gully(&record, &label);
                    accepted += 1;
                } else {
                    refused += 1;
                }
            }
        }
    }
    assert_eq!(records, 7 * 7 * 6, "the cross product changed size");
    assert_eq!(accepted + refused, records);
    // Every one of these combinations is inside every per-field domain, so all of them are
    // expected to be accepted -- and every one of them is then SAMPLED, which is where an
    // abort would happen. The assertion is that none did, and the count says how many.
    assert_eq!(accepted, records, "{refused} combinations were refused; each is in-domain");
}

#[test]
fn the_gully_sweep_is_the_size_it_claims_to_be() {
    // The size and shape of the gully sweep, stated as a number rather than left implicit: a
    // threshold assertion says nothing about how far above the threshold the run actually was,
    // and a sweep that quietly halved would still pass at 101.
    let records = swept_gully_records();
    let mut accepted = 0usize;
    for (_, record) in &records {
        if wb_gully_check(record.as_ptr(), WB_GULLY_STRIDE as u32) == WB_OK {
            accepted += 1;
        }
    }
    // 2 bases x 12 fields x (20 hostile + 2 bounds x 6 + 25 ladder rungs) = 2 x 12 x 57.
    // TEN fields until the second harmonic shipped; the two new ones are swept exactly as the
    // other ten are, and the accepted/refused split below was re-derived rather than scaled.
    assert_eq!(records.len(), 2 * 12 * 57, "the sweep changed size");
    assert_eq!(records.len(), 1_368, "and the arithmetic above says 1,368");
    assert_eq!(
        accepted, GULLY_SWEEP_ACCEPTED,
        "the accepted/refused split moved: {accepted} of {}",
        records.len()
    );
}

/// The accepted half of the gully sweep, pinned. Re-derived on this host by running the sweep.
/// **738 while the record was ten fields wide; 884 now the second harmonic has added two.**
/// The 146 is not scaled from the 738 and could not be: the two new fields have different
/// domains from each other and from the ten, so their accepted fractions differ. It was run.
const GULLY_SWEEP_ACCEPTED: usize = 884;

/// **The two harmonic fields, swept against each other AND against a third -- the cross
/// product, not one field at a time.**
///
/// This file already carries one abort that a one-field-at-a-time sweep was blind to by
/// construction, which is why `every_gully_field_swept_across_its_whole_range_and_beyond_
/// never_aborts` has a `cell x steer x reference` product beside its per-field ladders. The
/// harmonic gets the same treatment and for the same reason: `harmonic_weight` and
/// `harmonic_band_m` meet inside one expression -- `weight * smooth((h - gate) / band)` --
/// and they meet `gate_elevation_m` there too, so the interesting records are the ones where
/// a large weight lands on a tiny band on ground the gate has just opened. None of those is
/// reachable by moving one field from the preset.
///
/// Every combination below is inside every per-field domain, so every one is expected to be
/// accepted and then SAMPLED -- and sampling is where an abort behind `extern "C"` would
/// happen, since that is what reaches the lattice index, the pivot window, the cosine's
/// argument and `powf`'s exponent.
///
/// **The last assertion is the one with teeth, and the other two are recorded as weak
/// rather than dropped.** Two mutations were run against this test:
///
///   * Substituting the double angle `2.0 * first * first - 1.0` with `1.0 / first`, so a
///     pivot whose cosine is zero contributes an infinity, **stayed GREEN**. It should be
///     recorded why: `gully_offset_m` folds the signal and then clamps `folded` into `[0, 1]`
///     through two negated tests, so an infinite or NaN signal lands on a finite height by
///     construction. The finiteness assertion inside `sample_gully` therefore cannot fail
///     through this expression, and saying so is worth more than leaving a claim standing.
///   * Removing the `harmonic_band_m` bound from `gully_is_admissible` turns
///     `the_gully_sweep_is_the_size_it_claims_to_be` red (the accepted count moves) and
///     leaves this test green, because a zero band is a step in the ground and not an abort.
///
/// So the assertion that carries this test is `the_new_words_reach_the_kernel`: it compares
/// two records that differ ONLY in `harmonic_weight` and requires the elevation to move.
/// **Proved red by mutation**: having `decode_gully` read `harmonic_weight` and
/// `harmonic_band_m` from `GullyParams::drainage()` instead of from `fields[10]` and
/// `fields[11]` -- which is precisely the silently-dropping-builder shape, a widened record
/// whose new words look configured and go nowhere -- turns it red. It also moves the accepted
/// count and so turns `the_gully_sweep_is_the_size_it_claims_to_be` red beside it; that is
/// recorded rather than claimed away, because a mutation caught by two assertions is still
/// only caught by the one whose message names the cause, and this is that one.
#[test]
fn the_two_harmonic_fields_are_swept_as_a_cross_product_with_the_gate() {
    let mut records = 0usize;
    let mut accepted = 0usize;
    for weight in [0.0f64, 0.25, 0.9, 2.0, WB_MAX_GULLY_HARMONIC_WEIGHT] {
        for band in [
            WB_MIN_GULLY_HARMONIC_BAND_M,
            10.0,
            1_800.0,
            100_000.0,
            WB_MAX_GULLY_HARMONIC_BAND_M,
        ] {
            for gate in [-1_000.0f64, 0.0, 200.0, 5_000.0] {
                records += 1;
                let mut record = encode_drainage();
                record[6] = gate;
                record[10] = weight;
                record[11] = band;
                let label = format!("weight {weight:e} x band {band:e} x gate {gate:e}");
                if wb_gully_check(record.as_ptr(), WB_GULLY_STRIDE as u32) == WB_OK {
                    sample_gully(&record, &label);
                    accepted += 1;
                }
            }
        }
    }
    assert_eq!(records, 5 * 5 * 4, "the cross product changed size");
    assert_eq!(records, 100, "and the arithmetic above says 100");
    assert_eq!(
        accepted, records,
        "every combination here is inside every per-field domain, so none may be refused"
    );
    the_new_words_reach_the_kernel();
}

/// Two records differing only in `harmonic_weight`, and the ground must not be the same.
///
/// Without this the whole widening could be a no-op at the boundary: the stride grows, the
/// bounds check passes, the constructor returns a handle, and the two new words are never
/// read. Every assertion above would still be green.
fn the_new_words_reach_the_kernel() {
    let mut off = encode_drainage();
    off[10] = 0.0;
    let mut on = encode_drainage();
    on[10] = 2.0;
    let handle_off = world_with_gully(&off);
    let handle_on = world_with_gully(&on);
    assert_ne!(handle_off, 0);
    assert_ne!(handle_on, 0);
    let mut moved = 0usize;
    for (lat, lon) in GULLY_PROBES {
        let a = wb_elevation_m(handle_off, *lat, *lon, 76.35);
        let b = wb_elevation_m(handle_on, *lat, *lon, 76.35);
        assert!(a.is_finite() && b.is_finite());
        if a != b {
            moved += 1;
        }
    }
    assert_eq!(wb_world_free(handle_off), WB_OK);
    assert_eq!(wb_world_free(handle_on), WB_OK);
    assert!(
        moved > 0,
        "harmonic_weight crossed the boundary as a word and changed nothing: the two records          differ only at index 10 and all {} probes gave identical ground",
        GULLY_PROBES.len()
    );
}

/// `GullyParams::drainage()` as a record, through the engine's own exporter so this file
/// never transcribes a preset's numbers.
fn encode_drainage() -> [f64; WB_GULLY_STRIDE] {
    let mut record = [0.0f64; WB_GULLY_STRIDE];
    assert_eq!(
        wb_gully_preset(WB_GULLY_DRAINAGE, record.as_mut_ptr(), WB_GULLY_STRIDE as u32),
        WB_OK
    );
    record
}

#[test]
fn the_gully_preset_is_the_engines_own_numbers_and_the_canonical_one_is_off() {
    // Ruling 7 of the relief slice, for the fifth channel: a host asks the engine for a preset
    // and never restates a measured constant. The two selectors differ in exactly one word --
    // the amplitude -- which is what `GullyParams::canonical()` means.
    let canonical = gully_preset_record(WB_GULLY_CANONICAL);
    let drainage = gully_preset_record(WB_GULLY_DRAINAGE);
    assert_eq!(canonical[0], 0.0, "canonical is the kernel switched off");
    assert!(drainage[0] > 0.0, "drainage is the kernel switched on");
    for word in 1..WB_GULLY_STRIDE {
        assert_eq!(
            canonical[word].to_bits(),
            drainage[word].to_bits(),
            "the two presets must differ in the amplitude alone; word {word} differs"
        );
    }
    // A selector this build does not know is refused rather than answered with a default.
    let mut out = [0.0; WB_GULLY_STRIDE];
    assert_eq!(wb_gully_preset(2, out.as_mut_ptr(), WB_GULLY_STRIDE as u32), WB_ERR_PARAM);
    assert_eq!(wb_gully_preset(WB_GULLY_DRAINAGE, out.as_mut_ptr(), 9), WB_ERR_BUFFER);
    assert_eq!(wb_gully_preset(WB_GULLY_DRAINAGE, core::ptr::null_mut(), WB_GULLY_STRIDE as u32), WB_ERR_BUFFER);
}

#[test]
fn the_fifth_door_carries_all_four_earlier_blocks_and_the_canonical_pair_is_still_canonical() {
    // The gully door is the widest, so it is the one that must still answer exactly what
    // `wb_world_new` answers when every block is the canonical null/zero pair. Ruling 1, held
    // at the boundary rather than inside.
    let plain = wb_world_new(SEED, RADIUS_M, PLATES, LAND, core::ptr::null(), 0);
    let widest = wb_world_new_gully(
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
        core::ptr::null(),
        0,
    );
    assert_ne!(plain, 0);
    assert_ne!(widest, 0);
    let mut compared = 0u32;
    for (lat, lon) in GULLY_PROBES {
        for resolution in [RES_M, 76.35] {
            let a = wb_elevation_m(plain, *lat, *lon, resolution);
            let b = wb_elevation_m(widest, *lat, *lon, resolution);
            assert_eq!(a.to_bits(), b.to_bits(), "the canonical pair moved at ({lat}, {lon})");
            compared += 1;
        }
        assert_eq!(
            wb_structural_m(plain, *lat, *lon).to_bits(),
            wb_structural_m(widest, *lat, *lon).to_bits(),
        );
    }
    assert_eq!(compared, (GULLY_PROBES.len() as u32) * 2);
    // A non-canonical gully record through the same door must move the ground, or the
    // comparison above is comparing a parameter nothing reads.
    let drainage = gully_preset_record(WB_GULLY_DRAINAGE);
    let gullied = world_with_gully(&drainage);
    assert_ne!(gullied, 0);
    let mut moved = 0u32;
    for (lat, lon) in GULLY_PROBES {
        if wb_elevation_m(plain, *lat, *lon, 76.35).to_bits()
            != wb_elevation_m(gullied, *lat, *lon, 76.35).to_bits()
        {
            moved += 1;
        }
    }
    assert!(moved >= 8, "only {moved} of the gated probes moved under drainage()");
    assert_eq!(wb_world_free(plain), WB_OK);
    assert_eq!(wb_world_free(widest), WB_OK);
    assert_eq!(wb_world_free(gullied), WB_OK);
}

#[test]
fn a_wrongly_sized_or_misaligned_gully_buffer_is_refused_rather_than_read() {
    let record = gully_preset_record(WB_GULLY_DRAINAGE);
    // A non-null pointer with a zero length is a host that computed a length wrong, not a host
    // asking for canonical.
    assert_eq!(wb_gully_check(record.as_ptr(), 0), WB_ERR_BUFFER);
    assert_eq!(wb_gully_check(core::ptr::null(), 1), WB_ERR_BUFFER);
    assert_eq!(wb_gully_check(record.as_ptr(), 9), WB_ERR_BUFFER);
    assert_eq!(wb_gully_check(record.as_ptr(), 11), WB_ERR_BUFFER);
    // The canonical pair.
    assert_eq!(wb_gully_check(core::ptr::null(), 0), WB_OK);
}
