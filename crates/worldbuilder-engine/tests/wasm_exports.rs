//! The WASM export surface, exercised from outside the crate exactly as a JS host reaches
//! it: through the `extern "C"` entry points and raw pointers into linear memory.
//!
//! **Run natively, deliberately.** These tests are about the *boundary contract* -- what a
//! handle means, which grid a tile is, what a status code says, which inputs are refused --
//! and every one of those is host-independent.
//!
//! **Native-against-WASM parity is a separate, committed harness**, not an assertion made
//! here and not a figure in a task report: `examples/parity_dump.rs` plus
//! `parity/parity.mjs` compare 53,251 values through the *shipped* exports -- scattered
//! open water, inside a placed harbour, and two 65x65 tiles -- against the committed
//! `.wasm`, with a `--mutate seed` control that must turn them red. See
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
    Surface::new(SEED, RADIUS_M, 12, LAND, Some(FeatureInput::Loose(features)))
}

// ----------------------------------------------------------------- identity and memory

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

// -------------------------------------------------------------------- sampling by point

#[test]
fn elevation_and_structural_are_the_engine_verbatim_bit_for_bit() {
    let h = plain_world();
    let surface = Surface::new(SEED, RADIUS_M, 12, LAND, None);
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
    let surface = Surface::new(SEED, RADIUS_M, 12, LAND, None);
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
    let surface = Surface::new(SEED, RADIUS_M, 12, LAND, None);
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
    let surface = Surface::new(SEED, RADIUS_M, 12, LAND, None);
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
