//! The browser's door into the engine: a C ABI over `wasm32-unknown-unknown`, with no
//! binding generator, no JS glue and no imports.
//!
//! # Why this file exists at all
//!
//! A `cdylib` exports only symbols marked with the no-mangle attribute and `extern "C"`.
//! The engine had none, so the first wasm32 build of this crate was **327 bytes exporting
//! only `memory`** -- a green build containing nothing, reproduced deliberately by slice 1p
//! Task 1 and confirmed by hand-parsing export section id 7. Every function below is
//! therefore load-bearing in a way an ordinary Rust module is not: delete the attribute and
//! the function does not become dead code, it becomes *absent*, silently, from a build that
//! still exits 0. `WB_EXPORTS` is the declared list, and a test holds this source to it.
//!
//! # The shape, and why it is not `bindings.rs`
//!
//! `bindings.rs` is a conformance shim for Python: `surface_elevation_m(world_seed,
//! radius_m, plate_count, land_fraction, x, y, z, resolution_m, features)` **rebuilds the
//! `Surface` on every call**. The cost of that is **~10^3x a sample**, and the ratio is a
//! property of a host and a corpus rather than a constant -- so both are named here, and
//! the figure is quoted to an order of magnitude because that is what reproduces.
//!
//! Method, identical in each row: `Surface::new` timed over n = 20 worlds, `elevation_m`
//! over n = 20,000 points at `resolution_m = 250`, after warm-up.
//!
//! | host | `Surface::new` | `elevation_m` | ratio |
//! |---|---|---|---|
//! | author, native `--release`, `x86_64-pc-windows-msvc`, cargo 1.98.0 | 0.657 ms/world | 0.617 us/sample | 1,065x |
//! | reviewer, same target and toolchain, different machine | 0.5075 ms/world | 0.5642 us/sample | 900x |
//! | Chrome 151, wasm32 | 2.2-3.2 ms/world | ~0.9 us/sample | 2,400x-3,600x |
//!
//! The two native rows are 15% apart, and **the scatter of the 20,000 points is the
//! dominant term**: this module's own tile measurement puts a coastal sample at up to 9x a
//! deep-ocean one, so a ratio quoted without its corpus is not reproducible. A 65x65 tile
//! is 4,225 samples on every row.
//!
//! So the model here is a **world handle**: build one world from its parameters, sample it
//! as many times as you like, free it. That is not a compromise forced by cost -- because
//! `Surface::new` is milliseconds, a *parameter* change still rebuilds a world inside one
//! animation frame, which is what makes the studio's controls feel live.
//!
//! # What crosses, and what does not
//!
//! **Nothing is batched for throughput.** A boundary crossing with three f64 arguments was
//! measured at 0.008-0.013 us in Chrome 151 against ~0.9 us for the elevation it carries --
//! about 2% -- and per-call sampling against a single fill over an identical 256x256 grid
//! measured indistinguishable. `wb_fill_tile_f32` exists for *ergonomics*: it hands a
//! worker one buffer it can transfer, and it spares the host 4,225 loop iterations. It is
//! not an optimisation and must not be defended as one.
//!
//! **Output width is free**, so the tile is shaped for its consumer rather than for the
//! engine: f64, f32 and i16 all measured ~0.9 us/sample, and Cesium's `HeightmapTerrainData`
//! takes a `Float32Array` of metres above the ellipsoid directly. Narrowing costs
//! 1.93e-5 m at the witnessed probe, which is nothing against a height field whose finest
//! generated octave is 312.5 m across.
//!
//! **`bottom_at` is a cursor tap, never a tile.** It costs ~3.4x an elevation (six indirect
//! calls, four of them a finite-difference slope) and marshals as three f64 plus a status.
//!
//! # Safety, stated once for the whole file
//!
//! Every entry point here is a safe Rust `fn` even where it dereferences a raw pointer, and
//! that is deliberate rather than an oversight. This module is **one** trust boundary, not
//! ten: a JS host can pass any `u32` as a handle and any integer as a pointer, so marking
//! the four pointer-taking entry points `unsafe` while leaving the other six safe would
//! imply a guarantee about the six that the ABI cannot give. What this module does instead
//! is answer every *representable* invalid input with a status code -- a stale handle, a
//! null buffer, a short buffer, a degenerate grid, a parameter outside its domain -- and
//! state the one precondition it cannot check on each function that has it: **a non-null
//! pointer must be a live, correctly aligned allocation of at least the stated length.**
//!
//! # Panics are fatal here, and not only on wasm
//!
//! `wasm32-unknown-unknown` builds with `panic = abort`, so a panic inside any of these is
//! an unrecoverable trap that takes the module down and cannot be caught by the host.
//! **`extern "C"` is nounwind, so the same is true natively**: a panic that reaches one of
//! these boundaries does not unwind into the caller, it aborts the process. Measured, by
//! deleting the `land_fraction` lower bound and running the suite: the test binary did not
//! report a failing test, it died with `thread caused non-unwinding panic. aborting` and
//! `STATUS_STACK_BUFFER_OVERRUN`, taking the other twenty-seven tests with it.
//!
//! So the parameter validation in `wb_world_new` is not defensive politeness. A
//! `land_fraction` of -1.0 indexes past the end of the calibration sample inside
//! `Continentality::new` and panics at `continentality.rs:113` -- measured -- and there is
//! no layer above this one that can survive it.
#![allow(clippy::too_many_arguments)]
// The entry points take raw pointers from a foreign host by construction; see "Safety"
// above for why they are nevertheless safe `fn`s rather than four `unsafe` ones.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::alloc as sys;
use std::alloc::Layout;
use std::cell::RefCell;

use crate::features::Feature;
use crate::features::{CARVE, RAISE, SHAPE};
use crate::sphere::SpherePoint;
use crate::substrate::{MUD, ROCK, SAND};
use crate::surface::{FeatureInput, Surface};
use crate::{World, GENERATOR_VERSION};

// ---------------------------------------------------------------- the declared contract

/// The call did what it was asked.
pub const WB_OK: u32 = 0;
/// No world answers to that handle: never issued, or already freed.
pub const WB_ERR_HANDLE: u32 = 1;
/// The buffer is null, misaligned, or shorter than the call needs.
pub const WB_ERR_BUFFER: u32 = 2;
/// The grid is degenerate (a zero dimension) or a bound is not finite.
pub const WB_ERR_GRID: u32 = 3;
/// A placed feature declared a substrate the engine has no pure composition for. This is
/// the engine's own `UnknownSubstrate` forwarded rather than swallowed -- Python raises
/// `KeyError` at the same input, and both languages decline to answer at the same place.
pub const WB_ERR_SUBSTRATE: u32 = 4;

/// `compose` codes for a feature record. They are f64 because a record is a flat f64 array;
/// the comparison is exact equality, and `-0.0` reads as `RAISE` for the same reason.
pub const WB_COMPOSE_RAISE: f64 = 0.0;
/// See `WB_COMPOSE_RAISE`.
pub const WB_COMPOSE_CARVE: f64 = 1.0;
/// See `WB_COMPOSE_RAISE`.
pub const WB_COMPOSE_SHAPE: f64 = 2.0;

/// `substrate` codes for a feature record. `DERIVE` is the `None` sentinel -- "work the
/// bottom out from the shape of the ground", which is right for a bank and wrong for a
/// rock -- and is **not** the same as declaring an empty string, which the engine treats as
/// a word it has no composition for.
pub const WB_SUBSTRATE_DERIVE: f64 = 0.0;
/// See `WB_SUBSTRATE_DERIVE`.
pub const WB_SUBSTRATE_SAND: f64 = 1.0;
/// See `WB_SUBSTRATE_DERIVE`.
pub const WB_SUBSTRATE_MUD: f64 = 2.0;
/// See `WB_SUBSTRATE_DERIVE`.
pub const WB_SUBSTRATE_ROCK: f64 = 3.0;

/// f64 words per feature record: latitude, longitude, target_m, length_m, width_m,
/// bearing_deg, compose code, substrate code.
pub const WB_FEATURE_STRIDE: usize = 8;

/// The ceiling on `plate_count`, and it is a *refusal*, not a clamp.
///
/// Every sample walks the plate table and `Surface::new` builds it, so a plate count in the
/// millions is not a slow world -- it is a hung tab. Earth has about fifteen; this crate's
/// own fixtures use 8, 12 and 24. Anything above this is a caller mistake, and the honest
/// answer to a caller mistake is a refusal rather than a silently different world.
pub const WB_MAX_PLATE_COUNT: u32 = 4096;

/// Alignment for every buffer `wb_alloc` hands out: 8, so the same allocation serves an f64
/// payload (`wb_bottom_at`) and an f32 tile without the host having to think about it.
const WB_ALIGN: usize = 8;

/// **The export list, declared.** A native test run cannot see the artifact's export
/// section, and a forgotten no-mangle attribute is invisible in a build that exits 0 -- so
/// this list is checked against this file's own source by a test, and the built `.wasm` is
/// checked against this list at build time.
pub const WB_EXPORTS: &[&str] = &[
    "wb_generator_version",
    "wb_alloc",
    "wb_dealloc",
    "wb_world_new",
    "wb_world_free",
    "wb_world_count",
    "wb_elevation_m",
    "wb_structural_m",
    "wb_bottom_at",
    "wb_fill_tile_f32",
];

// -------------------------------------------------------------------- the handle table

thread_local! {
    /// **Slots are never reused, and that is the whole design.**
    ///
    /// A freed handle stays freed: its slot is emptied and no later world is ever issued
    /// that number. A host holding a stale handle -- a worker that outlived a parameter
    /// change, a cached tile request still in flight -- therefore gets `WB_ERR_HANDLE`
    /// instead of silently sampling a *different planet* that happens to occupy the same
    /// slot. The cost is one machine word per world ever created, against a constructor
    /// that takes milliseconds; a session would have to build four billion worlds to
    /// notice.
    ///
    /// **Thread-local, not a global mutex.** On `wasm32-unknown-unknown` there is one
    /// thread per instance and this is simply a static; the worker pool the viewer uses
    /// gives each worker its own module instance and therefore its own table, which is what
    /// it wants anyway. Natively it means the table is per-thread, so each test gets a
    /// fresh one -- convenient, and stated here so nobody reads a `wb_world_count` of zero
    /// on another thread as a bug.
    static WORLDS: RefCell<Vec<Option<Box<World>>>> = const { RefCell::new(Vec::new()) };
}

/// Install a world built by Rust and hand back its handle, or 0 if the table is full.
///
/// **Not an export**, and deliberately: it is the door for a world the flat feature channel
/// cannot describe -- one carrying a pre-built `Features` at its own radius, or a feature
/// declaring a substrate word the channel refuses. The tests reach `WB_ERR_SUBSTRATE`
/// through here, because nothing a JS host can pass reaches it.
pub fn insert_world(world: World) -> u32 {
    WORLDS.with(|cell| {
        let mut table = cell.borrow_mut();
        table.push(Some(Box::new(world)));
        u32::try_from(table.len()).unwrap_or(0)
    })
}

/// Borrow the world a handle names, or `None`. Handles are one-based, so 0 is never valid
/// and doubles as `wb_world_new`'s failure return.
fn with_world<T>(handle: u32, action: impl FnOnce(&World) -> T) -> Option<T> {
    WORLDS.with(|cell| {
        let table = cell.borrow();
        let index = usize::try_from(handle.checked_sub(1)?).ok()?;
        let world = table.get(index)?.as_deref()?;
        Some(action(world))
    })
}

/// The `resolution_m` sentinel, in one place so the two sampling paths cannot drift apart.
///
/// A positive finite value is passed through and lets detail finer than the sampling drop
/// out. **Anything else means canonical ground truth**, which is `None` -- zero, negative,
/// NaN and both infinities. The engine's `resolution_m` is a sampling distance, and a
/// nonpositive one is not a coarser answer but a nonsense one; forwarding it would let a
/// host's uninitialised variable choose a different field silently.
///
/// **Both call sites are pinned, and independently.** The drift this function exists to
/// prevent is measurable: at lat 12.0 lon 34.0 on the test world, `Some(-1.0)` and both
/// infinities give 681.2161549154603 where `None` gives 683.4579940205472 -- 2.24 m, at the
/// same point, between the tile and the scalar export. `wb_elevation_m` is held to it by
/// `the_resolution_sentinel_selects_canonical_ground_truth_from_anything_nonpositive` and
/// `wb_fill_tile_f32` by `the_tile_reads_the_resolution_sentinel_exactly_as_the_scalar_export_does`;
/// the second exists because the first does not cover the tile, and a whole test corpus
/// passing `resolution_m = 250` let that mutation survive.
fn resolution(resolution_m: f64) -> Option<f64> {
    if resolution_m.is_finite() && resolution_m > 0.0 {
        Some(resolution_m)
    } else {
        None
    }
}

/// One grid coordinate: where sample `index` of `last + 1` sits between two bounds.
///
/// **The form is the answer, not an implementation detail.** This is `a + (b - a) * t`, and
/// `a * (1 - t) + b * t` is a different function in binary floating point -- measured, they
/// disagree on **10 of the 65** row latitudes and **24 of the 65** column longitudes of the
/// 0.01-degree tile the tests use. It is exactly the sort of "equivalent" tidy-up that gets
/// waved through in review, which is why the choice lives in a named function with a test
/// on it rather than inline in a loop.
///
/// **It is also why that test cannot be an output test.** Swapping the two forms changed
/// **0 of 8,450** f32 tile samples across both test regimes -- open water and inside the
/// harbour -- because the disagreement is one ULP of latitude, about 4e-10 m on the ground,
/// and that vanishes when a height is narrowed for a `Float32Array`. The formula is decided
/// in f64, so it has to be pinned in f64.
///
/// `last == 0.0` is a one-row or one-column grid, which has no step to take and samples its
/// first bound; without the branch it would divide zero by zero and hand back NaN.
///
/// **Not an export** -- it takes no part in the ABI, and exists to be named and tested.
pub fn grid_coordinate(from_deg: f64, to_deg: f64, index: f64, last: f64) -> f64 {
    if last == 0.0 {
        from_deg
    } else {
        from_deg + (to_deg - from_deg) * (index / last)
    }
}

/// Write three f64 into a caller buffer.
///
/// # Safety
/// `out` must be non-null, 8-aligned, and good for three f64.
unsafe fn write_triple(out: *mut f64, values: [f64; 3]) {
    for (offset, value) in values.into_iter().enumerate() {
        out.add(offset).write(value);
    }
}

/// One feature record, decoded, or `None` if it is not one this channel can represent.
///
/// **`kind` is empty and `marked` is false**, and neither is a loss: `kind` is a name for
/// diagnostics and chart symbols, `marked` selects chart symbols, and no path this module
/// exposes -- elevation, structural, bottom -- reads either. What those paths do read is the
/// geometry, the compose rule and the substrate word, and all four are here.
fn decode_feature(record: &[f64]) -> Option<Feature> {
    let fields = <[f64; WB_FEATURE_STRIDE]>::try_from(record).ok()?;
    let [latitude_deg, longitude_deg, target_m, length_m, width_m, bearing_deg] =
        [fields[0], fields[1], fields[2], fields[3], fields[4], fields[5]];
    let (compose_code, substrate_code) = (fields[6], fields[7]);
    for value in [latitude_deg, longitude_deg, target_m, length_m, width_m, bearing_deg] {
        if !value.is_finite() {
            return None;
        }
    }
    if !(-90.0..=90.0).contains(&latitude_deg) {
        return None;
    }
    // A feature with no extent has no reach, so it would be a record that looks placed and
    // does nothing -- the silently-dropped-field shape. Refused instead.
    //
    // **The line is at zero, not at "negligible", and deliberately.** `5e-324` is accepted,
    // and it does about as much as `0.0` does. But every candidate floor above zero is an
    // invented number: the engine's reach falls off continuously, so a metre-scale floor
    // would refuse a legitimately tiny feature on a small world, and a floor keyed to
    // `radius_m` would make the same record decode on one planet and not another. Zero is
    // the one bound that follows from the type rather than from taste, and it is the bound
    // `the_feature_channel_refuses_what_it_cannot_represent` tests.
    if length_m <= 0.0 || width_m <= 0.0 {
        return None;
    }
    let compose = if compose_code == WB_COMPOSE_RAISE {
        RAISE
    } else if compose_code == WB_COMPOSE_CARVE {
        CARVE
    } else if compose_code == WB_COMPOSE_SHAPE {
        SHAPE
    } else {
        return None;
    };
    let substrate = if substrate_code == WB_SUBSTRATE_DERIVE {
        None
    } else if substrate_code == WB_SUBSTRATE_SAND {
        Some(SAND.to_string())
    } else if substrate_code == WB_SUBSTRATE_MUD {
        Some(MUD.to_string())
    } else if substrate_code == WB_SUBSTRATE_ROCK {
        Some(ROCK.to_string())
    } else {
        return None;
    };
    Some(Feature {
        kind: String::new(),
        at: SpherePoint::from_latlon(latitude_deg, longitude_deg),
        target_m,
        length_m,
        width_m,
        bearing_deg,
        compose: compose.to_string(),
        marked: false,
        substrate,
    })
}

// -------------------------------------------------------------------------- the exports

/// The generator's identity, per VERSION-001. Not the package version and never derived
/// from it: a host that caches tiles keys them on this, alongside the world's parameters.
#[no_mangle]
pub extern "C" fn wb_generator_version() -> u32 {
    GENERATOR_VERSION
}

/// Hand the host `bytes` of linear memory, 8-aligned, or null.
///
/// Null for a zero-byte request, which is not an allocation, and null if the allocator
/// declines. **The host must give the same `bytes` back to `wb_dealloc`**: Rust's allocator
/// is size-aware, so a mismatched length is undefined behaviour rather than a leak. The
/// probe module this replaces deliberately leaked instead of freeing; a viewer that fills
/// tiles for an hour cannot.
#[no_mangle]
pub extern "C" fn wb_alloc(bytes: u32) -> *mut u8 {
    let size = match usize::try_from(bytes) {
        Ok(size) if size > 0 => size,
        _ => return core::ptr::null_mut(),
    };
    match Layout::from_size_align(size, WB_ALIGN) {
        Ok(layout) => unsafe { sys::alloc(layout) },
        Err(_) => core::ptr::null_mut(),
    }
}

/// Give back a buffer `wb_alloc` handed out.
///
/// # Safety
/// `ptr` must have come from `wb_alloc`, and `bytes` must be the length it was asked for.
#[no_mangle]
pub extern "C" fn wb_dealloc(ptr: *mut u8, bytes: u32) -> u32 {
    if ptr.is_null() {
        return WB_ERR_BUFFER;
    }
    let size = match usize::try_from(bytes) {
        Ok(size) if size > 0 => size,
        _ => return WB_ERR_BUFFER,
    };
    match Layout::from_size_align(size, WB_ALIGN) {
        Ok(layout) => {
            unsafe { sys::dealloc(ptr, layout) };
            WB_OK
        }
        Err(_) => WB_ERR_BUFFER,
    }
}

/// Build a world from its parameters and return its handle, or **0** if it refused.
///
/// `world_seed` is the full `i64` and is not masked: `plates_for` keys a decimal string, so
/// -5 and 18446744073709551611 are different planets. A JS host passes it as a `BigInt`.
///
/// # The domains, and why each one is a refusal
///
/// - `radius_m` finite and strictly positive. A NaN radius produces NaN elevations at every
///   point -- plausible-looking garbage, measured -- rather than failing.
/// - `plate_count` in `1..=WB_MAX_PLATE_COUNT`. Zero is refused because it is what an
///   uninitialised host variable looks like; measured, plate counts of 0, 1 and 2 give an
///   identical field, so accepting 0 would quietly hand back a world nobody asked for.
/// - `land_fraction` finite and in `[0, 1]`. **This one prevents a trap, not a surprise.**
///   `Continentality::new` indexes `values[((1 - land_fraction) * (n - 1)) as usize]`, so a
///   negative fraction indexes past the end and panics -- and under `panic = abort` on
///   wasm32 that kills the module. Measured on this host: -1.0 and -inf panic at
///   `continentality.rs:113`; -1e-9 happens to land back in range. The line is drawn at the
///   documented domain rather than at the measured panic boundary, because that boundary is
///   an accident of the calibration sample count and would move if the count did.
///
/// # Features
///
/// `feature_count` records of `WB_FEATURE_STRIDE` f64 each, read from `features_ptr`; pass
/// a null pointer with a count of 0 for a world with none. Every record must decode, or the
/// whole call is refused -- a world built from five of six requested features is the
/// silently-dropping-builder shape this project has been bitten by before, where an
/// authored field looks configured and does nothing.
///
/// # Safety
/// If `feature_count` is non-zero, `features_ptr` must be a live, 8-aligned allocation of at
/// least `feature_count * WB_FEATURE_STRIDE` f64.
#[no_mangle]
pub extern "C" fn wb_world_new(
    world_seed: i64,
    radius_m: f64,
    plate_count: u32,
    land_fraction: f64,
    features_ptr: *const f64,
    feature_count: u32,
) -> u32 {
    if !radius_m.is_finite() || radius_m <= 0.0 {
        return 0;
    }
    if plate_count == 0 || plate_count > WB_MAX_PLATE_COUNT {
        return 0;
    }
    if !land_fraction.is_finite() || !(0.0..=1.0).contains(&land_fraction) {
        return 0;
    }
    let plates = match usize::try_from(plate_count) {
        Ok(plates) => plates,
        Err(_) => return 0,
    };

    let features = if feature_count == 0 {
        None
    } else {
        if features_ptr.is_null() {
            return 0;
        }
        let address = features_ptr as usize; // cast-ok: a pointer to an integer for an alignment check, no float anywhere near it
        if address % core::mem::align_of::<f64>() != 0 {
            return 0;
        }
        let count = match usize::try_from(feature_count) {
            Ok(count) => count,
            Err(_) => return 0,
        };
        let words = match count.checked_mul(WB_FEATURE_STRIDE) {
            Some(words) => words,
            None => return 0,
        };
        let records = unsafe { core::slice::from_raw_parts(features_ptr, words) };
        let mut decoded = Vec::with_capacity(count);
        for record in records.chunks_exact(WB_FEATURE_STRIDE) {
            match decode_feature(record) {
                Some(feature) => decoded.push(feature),
                None => return 0,
            }
        }
        Some(FeatureInput::Loose(decoded))
    };

    let surface = Surface::new(world_seed, radius_m, plates, land_fraction, features);
    insert_world(World::new(surface))
}

/// Drop a world. `WB_OK` if one was there, `WB_ERR_HANDLE` otherwise -- so a double free is
/// a reported mistake rather than a silent one.
#[no_mangle]
pub extern "C" fn wb_world_free(handle: u32) -> u32 {
    WORLDS.with(|cell| {
        let mut table = cell.borrow_mut();
        let index = match handle.checked_sub(1).and_then(|raw| usize::try_from(raw).ok()) {
            Some(index) => index,
            None => return WB_ERR_HANDLE,
        };
        match table.get_mut(index) {
            Some(slot) if slot.is_some() => {
                *slot = None;
                WB_OK
            }
            _ => WB_ERR_HANDLE,
        }
    })
}

/// How many worlds this instance is holding. A leak check the host can run itself, which is
/// the only reason it is exported: a viewer that rebuilds on every slider drag should watch
/// this stay flat.
#[no_mangle]
pub extern "C" fn wb_world_count() -> u32 {
    WORLDS.with(|cell| {
        let live = cell.borrow().iter().filter(|slot| slot.is_some()).count();
        u32::try_from(live).unwrap_or(u32::MAX)
    })
}

/// How high the ground is, in metres relative to datum. **NaN for an unknown handle**,
/// which is a value no valid world produces at a valid point, so the host needs no
/// out-parameter for one scalar.
///
/// See `resolution` for what `resolution_m` means, including which values mean canonical.
#[no_mangle]
pub extern "C" fn wb_elevation_m(
    handle: u32,
    latitude_deg: f64,
    longitude_deg: f64,
    resolution_m: f64,
) -> f64 {
    with_world(handle, |world| {
        let point = SpherePoint::from_latlon(latitude_deg, longitude_deg);
        world.surface().elevation_m(&point, resolution(resolution_m))
    })
    .unwrap_or(f64::NAN)
}

/// The ground before any roughness -- the same answer at every scale, with the shelf and
/// any placed features folded in but no detail octaves. NaN for an unknown handle.
#[no_mangle]
pub extern "C" fn wb_structural_m(handle: u32, latitude_deg: f64, longitude_deg: f64) -> f64 {
    with_world(handle, |world| {
        let point = SpherePoint::from_latlon(latitude_deg, longitude_deg);
        world.surface().structural_m(&point)
    })
    .unwrap_or(f64::NAN)
}

/// What the bottom is made of at one point: three fractions -- sand, mud, rock -- written to
/// `out`, and a status returned.
///
/// **A cursor tap, never a tile.** It costs about 3.4x an elevation, because it needs the
/// local slope and a slope is four more structural probes.
///
/// On any status other than `WB_OK` the payload is filled with NaN, so a host that ignores
/// the return value gets an obviously-wrong bottom rather than a stale plausible one.
///
/// # Safety
/// `out` must be null, or a live 8-aligned allocation of at least three f64.
#[no_mangle]
pub extern "C" fn wb_bottom_at(
    handle: u32,
    latitude_deg: f64,
    longitude_deg: f64,
    out: *mut f64,
) -> u32 {
    if out.is_null() {
        return WB_ERR_BUFFER;
    }
    let address = out as usize; // cast-ok: a pointer to an integer for an alignment check, not a float truncation
    if address % core::mem::align_of::<f64>() != 0 {
        return WB_ERR_BUFFER;
    }
    let answer = with_world(handle, |world| {
        let point = SpherePoint::from_latlon(latitude_deg, longitude_deg);
        world.surface().bottom_at(&point)
    });
    match answer {
        Some(Ok(composition)) => {
            unsafe { write_triple(out, [composition.sand, composition.mud, composition.rock]) };
            WB_OK
        }
        Some(Err(_)) => {
            unsafe { write_triple(out, [f64::NAN; 3]) };
            WB_ERR_SUBSTRATE
        }
        None => {
            unsafe { write_triple(out, [f64::NAN; 3]) };
            WB_ERR_HANDLE
        }
    }
}

/// Fill a rectangular grid of heights into linear memory, shaped for a `Float32Array`.
///
/// # The grid, exactly
///
/// Row-major, `width` columns by `height` rows, **both endpoints included**: row 0 sits at
/// `lat0_deg`, row `height - 1` at `lat1_deg`, column 0 at `lon0_deg`, column `width - 1` at
/// `lon1_deg`, and element `row * width + col` is the sample there. A single-column or
/// single-row grid samples its first bound and nothing else, because there is no step to
/// take. **No hemisphere is baked in**: pass `lat0_deg` as the northern edge to get Cesium's
/// north-to-south heightmap order, or the reverse for the reverse.
///
/// The interpolation is `grid_coordinate`, which is `a + (b - a) * t` with
/// `t = index / (count - 1)`; see that function for why the form is load-bearing and why
/// an f32 tile cannot pin it.
///
/// # Returns
///
/// `WB_OK`, or `WB_ERR_GRID` for a zero dimension or a non-finite bound, `WB_ERR_BUFFER` for
/// a null, misaligned or short buffer, `WB_ERR_HANDLE` for an unknown world. **Nothing is
/// written on any refusal** -- a half-filled tile is worse than none, because it reads as
/// terrain.
///
/// # Cost
///
/// Measured in Chrome 151 over 128 tile origins spread across the globe, 65x65 samples,
/// `resolution_m = 250`, each the mean of 8 fills: median 3.86 ms, p90 18.20 ms. **A coastal
/// tile costs up to 9x a deep-ocean one**, and coasts are what a viewer looks at, so no tile
/// can be filled on the main thread inside a frame. Fill in workers, and cache.
///
/// # Safety
/// `out` must be null, or a live 4-aligned allocation of at least `out_len` f32.
#[no_mangle]
pub extern "C" fn wb_fill_tile_f32(
    handle: u32,
    lat0_deg: f64,
    lat1_deg: f64,
    lon0_deg: f64,
    lon1_deg: f64,
    width: u32,
    height: u32,
    resolution_m: f64,
    out: *mut f32,
    out_len: u32,
) -> u32 {
    if width == 0 || height == 0 {
        return WB_ERR_GRID;
    }
    for bound in [lat0_deg, lat1_deg, lon0_deg, lon1_deg] {
        if !bound.is_finite() {
            return WB_ERR_GRID;
        }
    }
    let (columns, rows) = match (usize::try_from(width), usize::try_from(height)) {
        (Ok(columns), Ok(rows)) => (columns, rows),
        _ => return WB_ERR_GRID,
    };
    let samples = match columns.checked_mul(rows) {
        Some(samples) => samples,
        None => return WB_ERR_GRID,
    };
    if out.is_null() {
        return WB_ERR_BUFFER;
    }
    let address = out as usize; // cast-ok: a pointer to an integer for an alignment check, not a float truncation
    if address % core::mem::align_of::<f32>() != 0 {
        return WB_ERR_BUFFER;
    }
    match usize::try_from(out_len) {
        Ok(len) if len >= samples => {}
        _ => return WB_ERR_BUFFER,
    }

    let filled = with_world(handle, |world| {
        let surface = world.surface();
        let resolution_m = resolution(resolution_m);
        let last_row = f64::from(height - 1);
        let last_column = f64::from(width - 1);
        let buffer = unsafe { core::slice::from_raw_parts_mut(out, samples) };
        for row in 0..rows {
            let down = row as f64; // cast-ok: a grid row index to float, exact for any tile that fits in memory
            let latitude_deg = grid_coordinate(lat0_deg, lat1_deg, down, last_row);
            for column in 0..columns {
                let across = column as f64; // cast-ok: a grid column index to float, exact for any tile that fits in memory
                let longitude_deg = grid_coordinate(lon0_deg, lon1_deg, across, last_column);
                let point = SpherePoint::from_latlon(latitude_deg, longitude_deg);
                let metres = surface.elevation_m(&point, resolution_m);
                buffer[row * columns + column] = metres as f32; // cast-ok: narrowing a height for a Float32Array, measured at 1.93e-5 m against a 312.5 m finest octave
            }
        }
    });

    match filled {
        Some(()) => WB_OK,
        None => WB_ERR_HANDLE,
    }
}
