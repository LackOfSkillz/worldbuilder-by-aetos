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
//! dominant term**: this module's own tile measurement puts a coastal sample around 3.6x a
//! deep-ocean one on medians (14.46 ms against 4.05, n = 137 and 131 of 480 level-12 tiles,
//! Chrome 151); the 9x once quoted here compares extremes, not typical tiles. So a ratio
//! quoted without its corpus and its statistic is not reproducible. A 65x65 tile
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

use crate::climate;
use crate::continentality::CoastParams;
use crate::detail::{GullyParams, ReliefParams};
use crate::erosion::{erode_to_convergence, receiver_distances_m, ErosionParams, ErosionRun};
use crate::features::Feature;
use crate::features::{CARVE, RAISE, SHAPE};
use crate::sphere::SpherePoint;
use crate::stream::{sample_nodes, BuildParams, SamplingKind, StreamGraph};
use crate::substrate::{MUD, ROCK, SAND};
use crate::surface::{FeatureInput, Surface};
use crate::tectonics::{
    TectonicParams, COASTAL_UPLIFT_OFFSET_M, COLLISION_SYMMETRIC, ISLAND_ARC_OFFSET_M,
    MAX_TECTONIC_RANGE_M,
};
use crate::detmath as m;
use crate::hydrology::{self, HydroError, HydroParams};
use crate::water;
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
/// A `wb_erosion_run` numeric parameter (node count, uplift, erodibility, timestep,
/// convergence threshold, or iteration cap) is non-finite or outside the documented domain
/// [`wb_erosion_run`] declares for it. See that function's doc for why each bound is drawn
/// where it is: every one of these values is multiplied together inside `erode_step`
/// (`c = k * dt * sqrt(A_drainage) / d`), and `erode_to_convergence` **asserts** a NaN
/// height change rather than returning a status -- correct inside Rust, an abort across
/// this `extern "C"` boundary. This refusal exists so that assertion is never reached from
/// here, the same way `wb_world_new`'s `land_fraction` bound exists so a different panic
/// deeper in the engine is never reached from there.
pub const WB_ERR_PARAM: u32 = 5;
/// `stream::sample_nodes` or `StreamGraph::build` refused the sampled node set for a
/// [`wb_erosion_run`] call whose other parameters already passed validation. **Measured as
/// reachable, not merely a defensive status**: the whole-branch review of slice 5a found
/// that `radius_m` near the tiny end of `wb_world_new`'s domain -- `5e-324` and `1e-300`,
/// both otherwise finite and strictly positive -- builds a world successfully but returns
/// this status from `wb_erosion_run`, through the same committed artifact this crate ships.
/// An earlier version of this doc claimed the opposite ("not reachable by any input this
/// function's own domain checks admit today"); that was wrong, and this status is doing
/// real work. Kept as a status rather than an `unwrap`, on the same reasoning as every
/// other export in this file: an internal invariant this function currently guarantees is
/// not a licence to panic if a later change to `sample_nodes` or `StreamGraph::build` ever
/// makes it false -- which, for the tiny-radius case, it already has.
pub const WB_ERR_GRAPH: u32 = 6;

/// The ceiling on `node_count` for [`wb_erosion_run`]. Not the planetary target -- slice 1p
/// measured a 20,000,000-node graph at 1.45 GB of arrays and 2.16 GB peak RSS, which does
/// not fit a 32-bit wasm heap under any field arrangement, and this export exists to make
/// erosion's native/WASM parity testable, not to run a planetary bake through the browser
/// door. `erosion_convergence_sweep`'s own largest row is 100,000 nodes; this doubles that
/// for headroom.
///
/// **This ceiling and [`WB_MAX_EROSION_ITERATIONS`] multiply, and the product is not
/// small.** Timed natively in release (native is the *fast* side; the browser is slower):
/// a graph build at `node_count = 200,000` takes ~1.5 s, and each `erode_step` +
/// `cap_slopes` pass over it costs ~8.5 ms once the build cost is subtracted out (measured
/// at 2, 20 and 200 iterations). At `max_iterations = 200,000` that native estimate is
/// `8.5 ms * 200,000 ~ 1,700 s`, about 28 minutes of uninterruptible synchronous work --
/// but **the browser side is the one that matters for this export, and it is worse, not
/// better.** The whole-branch review of slice 5a timed a single `wb_erosion_run` call
/// (`node_count = 200,000`, `max_iterations = 1`) through the committed
/// `viewer/public/wasm/worldbuilder_engine.wasm`, instantiated directly in Node with no
/// imports, and measured **~14-16 s wall clock for that ONE iteration** (~14.0 s on this
/// crate's own re-measurement, this host, Node v22.17.0, against the artifact rebuilt for
/// this fix round; the review's own independent run on its host read ~16 s), graph-build
/// dominated, at 545 wasm pages (~35.7 MB) of linear memory. Scaled the same naive way as
/// the native figure -- and just as much an estimate, not a second measured total -- that
/// is `14 s * 200,000 ~ 2,800,000 s`, well over a month, for a call this function's own
/// domain checks still accept and return `WB_OK` from. Each ceiling is individually far
/// below the point a *single* dimension alone would hang a tab; **the pair together is
/// not, and the browser side is the one a UI caller would actually hit.** Not exploitable
/// today -- nothing in this crate calls `wb_erosion_run` at both ceilings but the parity
/// harness, which chooses far smaller values -- but a future caller that exposes this
/// export to a UI must bound the *product* (e.g. `node_count * max_iterations`) or lower
/// one ceiling to what a frame or worker budget actually tolerates; neither ceiling here
/// does that on its own, and the domain-reasoning gap this same review round found in
/// `radius_m` (see [`WB_MAX_WORLD_RADIUS_M`]) is a reason to trust that reasoning less, not
/// more, until the product is actually bounded in code.
pub const WB_MAX_EROSION_NODES: u32 = 200_000;
/// The ceiling on `max_iterations` for [`wb_erosion_run`]. `erosion_convergence_sweep` caps
/// its own runs at 200,000; the same number here, chosen for parity with that sweep, **not**
/// because it is safe standing alone -- see [`WB_MAX_EROSION_NODES`]'s doc for the measured
/// joint cost of the two ceilings together, which is the actual bound a caller needs to
/// respect.
pub const WB_MAX_EROSION_ITERATIONS: u32 = 200_000;
/// The magnitude ceiling on `uplift_m_per_yr` and `erodibility_per_yr` for
/// [`wb_erosion_run`], in each field's own unit. This crate's own fixtures run
/// `uplift_m_per_yr = 1.0e-3` and `erodibility_per_yr = 1.0e-6` -- nine orders of magnitude
/// below this bound -- so it is drawn to keep `c = k * dt * sqrt(A_drainage) / d` and
/// `u * dt` (`erode_step`'s doc) from overflowing `f64` against
/// [`WB_MAX_EROSION_TIMESTEP_YR`] and a planetary drainage area, not to constrain any value
/// this crate's own tests use.
///
/// **`erodibility_per_yr` additionally requires `>= 0.0`; `uplift_m_per_yr` does not.** A
/// negative uplift (subsidence) enters the update additively (`h_i + u*dt`) and never
/// changes the sign of `1 + c`, so it stays a contraction. A negative `erodibility_per_yr`
/// makes `c` itself negative, and `implicit_receiver_update` divides by `1.0 + c`: for `c`
/// in `(-2, 0)`, `|1 / (1 + c)| > 1` is a per-iteration *amplifying* map rather than the
/// contraction the module doc's whole convergence argument assumes, so the height field
/// grows without bound, overflows to `+/-inf` within roughly a hundred iterations at this
/// crate's own `dt`, and the next iteration's `inf - inf` trips `erode_to_convergence`'s
/// release-time `assert!(!change.is_nan())` -- an abort across this boundary. **Measured,
/// both natively and in the shipped `.wasm`**: `erodibility_per_yr = -9.0e-4`
/// (`u = 1.0e-3`, `dt = 1000`, threshold `1.0e-9`, 3,000 nodes) aborts at iteration 95
/// natively and traps with `RuntimeError: unreachable` in the committed artifact,
/// poisoning the WASM instance for every later call. **This is a band, not a single
/// cliff** -- `-1.0e-2`, `-1.0` and `-1.0e-6` all stay finite -- so a caller sweeping `k`
/// through negative values walks into it, and a single spot-check at one negative value
/// would have missed the others. Negative `k` is also not physical for a stream-power law
/// in the first place, so refusing the whole sign costs nothing a real caller wants.
pub const WB_MAX_EROSION_RATE_PER_YR: f64 = 1.0e6;
/// The magnitude ceiling on `timestep_yr` for [`wb_erosion_run`], in years. See
/// [`WB_MAX_EROSION_RATE_PER_YR`]'s doc for why this bound exists: it is drawn to keep `c`
/// from overflowing, not to match any realistic geological timestep.
pub const WB_MAX_EROSION_TIMESTEP_YR: f64 = 1.0e9;

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

/// f64 words per relief record, **and the order is the contract**:
///
/// | index | field |
/// |---:|---|
/// | 0 | `canonical_wavelength_m` |
/// | 1 | `coarsest_wavelength_m` |
/// | 2 | `abyssal_m` |
/// | 3 | `shelf_m` |
/// | 4 | `coast_m` |
/// | 5 | `interior_m` |
/// | 6 | `mountain_m` |
/// | 7 | `quieting_strength` |
/// | 8 | `quieting_scale_m` |
/// | 9 | `octave_persistence` |
///
/// That is `ReliefParams`'s own declaration order, and [`wb_relief_preset`] writes it in
/// exactly this order so a host never has to transcribe a preset's numbers. **A host that
/// wants `hills()` asks the engine for it; it does not restate three literals.**
pub const WB_RELIEF_STRIDE: usize = 10;

/// [`wb_relief_preset`] selector: `ReliefParams::canonical()`, today's ten values and the
/// `None` path's exact equivalent.
pub const WB_RELIEF_CANONICAL: u32 = 0;
/// [`wb_relief_preset`] selector: `ReliefParams::hills()`, the named preset Task 3 chose
/// against the measured tables.
pub const WB_RELIEF_HILLS: u32 = 1;

/// The floor on both wavelengths in a relief record, and **the one bound here that closes a
/// hang rather than a surprise.**
///
/// `Detail::plan` walks `while wavelength >= relief.canonical_wavelength_m { wavelength *=
/// 0.5 }`. At `canonical_wavelength_m == 0.0` that loop **never terminates**: halving
/// reaches `0.0` after about 1,080 steps and `0.0 >= 0.0` stays true forever, with the band
/// `Vec` growing on every pass. Every negative value is the same loop with the same end.
///
/// **Measured, not reasoned about.** Lowering this constant to `0.0` and re-running this
/// task's sweep on this host does not hang politely: the band `Vec` grows until
/// `memory allocation of 103079215104 bytes failed` and the process dies with
/// `STATUS_STACK_BUFFER_OVERRUN` (exit `0xc0000409`) out of Rust's allocation-failure
/// handler. That is an **abort reachable from a single f64 a browser can send**, through an
/// `extern "C"` function -- exactly the failure class this task exists to close -- and it
/// is a *band* rather than a cliff in the way slice 5a's two aborts were: `250.0` is fine,
/// `1.0e-3` is fine, `0.0` is fatal.
/// `a_zero_canonical_wavelength_would_hang_plan_and_is_refused_before_it_can` asserts the
/// refusal without ever entering the loop -- a test that entered it would not return, which
/// is why the mechanism is written out here rather than left to a test alone.
///
/// A millimetre is far below anything this generator means by a wavelength (canonical is
/// 250 m) and is chosen for margin, not as the measured edge of anything -- the same
/// posture [`WB_MAX_WORLD_RADIUS_M`] takes.
pub const WB_MIN_RELIEF_WAVELENGTH_M: f64 = 1.0e-3;

/// The ceiling on both wavelengths in a relief record. `+inf` is the hang again from the
/// other end -- `inf * 0.5` is `inf`, so the loop above never descends -- and this bound
/// refuses it along with every finite value large enough to matter.
///
/// It is also what bounds the **octave count**, which nothing else does: the loop runs
/// `log2(coarsest / canonical) + 1` times, so this ceiling with
/// [`WB_MIN_RELIEF_WAVELENGTH_M`] caps a record at `log2(1e9 / 1e-3) + 1 ~ 41` bands
/// against canonical's 7. Every band is a noise sample on every elevation call, so an
/// unbounded ratio is a slow world rather than a wrong one -- still worth a bound at the
/// door. Set to [`WB_MAX_WORLD_RADIUS_M`]'s value for the same reason it was: a wavelength
/// larger than the largest admissible planet is a caller mistake.
pub const WB_MAX_RELIEF_WAVELENGTH_M: f64 = 1.0e9;

/// The ceiling on each of the five roughness amplitudes (`abyssal_m`, `shelf_m`, `coast_m`,
/// `interior_m`, `mountain_m`). The floor is `0.0`: `Detail::offset_m` already returns `0.0`
/// for a non-positive amplitude, so a negative one is a field that looks configured and does
/// nothing -- the silently-dropping-builder shape this project refuses elsewhere in this
/// same file. A million metres is a hundred and fifty times the largest amplitude this
/// generator ships (`MOUNTAIN_M * 4.0 = 600` in `hills()`) and is a domain statement, not a
/// measured hazard: no amplitude in this range makes `offset_m` non-finite, which the sweep
/// test asserts by sampling every accepted record rather than by argument.
pub const WB_MAX_RELIEF_AMPLITUDE_M: f64 = 1.0e6;

/// The magnitude bound on `quieting_strength`, which is signed and admits both ends.
///
/// `amplitude_m` computes `quieted = 1.0 - quieting_strength * smooth(..)`, and `smooth`
/// returns `[0, 1]`, so `|strength| <= 1` keeps `quieted` in `[0, 2]` -- roughness fully
/// suppressed at one end, doubled at the other. Past `1.0` the term changes the *sign* of
/// the roughness where tectonics are large, which is not "more relief" but inverted relief,
/// and is a different mechanism wearing this parameter's name. Canonical is `+0.7` and
/// `hills()` is `-0.7`; both ends of the calibrated slider travel sit well inside this.
pub const WB_MAX_QUIETING_STRENGTH: f64 = 1.0;

/// The floor and ceiling on `quieting_scale_m`, which is a divisor
/// (`smooth(tectonic_m.abs() / quieting_scale_m)`). Zero does not abort -- `smooth` clamps
/// `inf` and `NaN` alike to `1.0`, which the module's own doc records -- so this pair is a
/// domain statement rather than a closed hazard, and is documented as one. Canonical is
/// `1200.0`.
pub const WB_MIN_QUIETING_SCALE_M: f64 = 1.0e-3;
/// See [`WB_MIN_QUIETING_SCALE_M`].
pub const WB_MAX_QUIETING_SCALE_M: f64 = 1.0e9;

// ------------------------------------------------------------------ the tectonic channel
//
// The mountains slice, Task 4. The owner asked for two knobs in their own words -- "1 to
// raise and lower mountains and one to make more mountains and less as desired" -- and then,
// looking at the finished relief work, "we still have no mountains". The honest reason was
// that the peak on their own world is **98.9% tectonic** (1,454.04 m, of which 1,437.81 m is
// structural, measured on seed 123925603 / radius 4,500,000 m / 28 plates / land 0.16), so
// no relief parameter could ever have moved it. `TectonicParams` is the block that can, and
// this channel is how a browser reaches one.
//
// It is deliberately the same shape as the relief channel above -- a flat f64 record in a
// documented order, a preset export so no host transcribes a number, a checker that answers
// *why* rather than only *that*, and a constructor that refuses a record entire rather than
// admitting it with one field adjusted. Nothing here clamps.

/// f64 words per tectonic record, **and the order is the contract**:
///
/// | index | field |
/// |---:|---|
/// | 0 | `continent_collision_m` |
/// | 1 | `continent_collision_width_m` |
/// | 2 | `coastal_uplift_m` |
/// | 3 | `coastal_uplift_width_m` |
/// | 4 | `island_arc_m` |
/// | 5 | `island_arc_width_m` |
/// | 6 | `ridge_m` |
/// | 7 | `ridge_width_m` |
/// | 8 | `continental_blend` |
/// | 9 | `collision_asymmetry` |
/// | 10 | `suture_count` -- **an integer carried as an f64**, see [`WB_MAX_SUTURE_COUNT`] |
/// | 11 | `suture_spread_m` |
/// | 12 | `structure_depth` |
/// | 13 | `structure_wavelength_m` |
/// | 14 | `margin_warp_m` |
/// | 15 | `margin_warp_wavelength_m` |
///
/// That is `TectonicParams`'s own declaration order, and [`wb_tectonic_preset`] writes it in
/// exactly this order so a host never has to transcribe a value.
///
/// **Words 9-13 are Task 3 widening this from 9, and that widening is the whole task.** Task 2
/// built the structure field -- the three techniques that turn a smooth blade into parallel
/// belts with separate massifs and a ridge-and-valley interior -- and then stopped at this
/// line, because its own brief forbade viewer work. The comment `decode_tectonic` carried at
/// the point it filled these from `canonical()` named exactly what a later task had to do:
/// *"widen the stride, `decode`, `encode`, `tectonic_is_admissible` AND sweep the export"*.
/// All four are done here, and the sweep found what that comment predicted it would.
///
/// **Words 14-15 are Task 5 widening it again, from 14, and they are the only two fields on
/// this channel that change WHERE a range is rather than what it looks like.** Every margin
/// in this engine is a great circle, so every belt on one is straight by construction; the
/// along-margin warp is what bends it, and the owner asked for it in those words. Same four
/// things moved together, and the sweep gained a base and two cross products.
pub const WB_TECTONIC_STRIDE: usize = 16;

/// [`wb_tectonic_preset`] selector: `TectonicParams::canonical()`, today's fourteen values and
/// the `None` path's exact equivalent.
pub const WB_TECTONIC_CANONICAL: u32 = 0;

/// [`wb_tectonic_preset`] selector: `TectonicParams::ranges()`, the preset Task 3 chose.
///
/// **It crosses as FIELDS, never as a name.** The panel receives fourteen numbers and puts
/// them on its own sliders, so the owner sees what the preset asked for and can move any part
/// of it -- which is Ruling 7 of the relief slice, enforced there by a test that strips
/// comments out of the viewer's JavaScript and asserts the values appear in neither file. The
/// same test exists for this preset in `viewer/test/tectonic-params.test.mjs`.
pub const WB_TECTONIC_RANGES: u32 = 1;

/// The magnitude bound on each of the four profile amplitudes (`continent_collision_m`,
/// `coastal_uplift_m`, `island_arc_m`, `ridge_m`).
///
/// **Signed, unlike the relief amplitudes, and that is a statement about the engine rather
/// than laxity.** `tectonics.rs` ships `TRENCH_M = -2600.0` and `RIFT_M = -350.0` in the same
/// family of profiles, so a negative uplift is a shape this generator already draws and not a
/// caller mistake; the relief amplitudes are bounded below at zero for the opposite reason,
/// because `Detail::offset_m` returns `0.0` for a non-positive one and the field would look
/// configured while doing nothing.
///
/// A hundred kilometres of uplift is about eleven times Everest, and this is **a domain
/// statement, not a measured hazard**: the profile is a multiplication by a smoothstep in
/// `[0, 1]`, so no amplitude in this range makes `offset_m` non-finite. That is asserted by
/// *sampling* every accepted record in the sweep rather than by argument -- the same posture
/// [`WB_MAX_RELIEF_AMPLITUDE_M`] takes, and for the same reason: the two aborts slice 5a
/// found were both bands nobody would have picked by hand.
pub const WB_MAX_TECTONIC_AMPLITUDE_M: f64 = 1.0e5;

/// The floor on all four profile widths.
///
/// **Zero does not divide and does not abort** -- `tectonics::bump` opens with
/// `if width_m <= 0.0 { return 0.0 }`, which `a_zero_width_bump_is_nothing_rather_than_a_
/// division_by_zero` pins -- so this bound closes a *silence*, not a crash. A width of zero
/// is a profile that is present in the record, accepted by the constructor, and contributes
/// exactly nothing at every point on the planet: the silently-dropping-builder shape this
/// file already refuses in the feature channel, where a world built from five of six
/// requested features is refused entire.
///
/// Negative widths are the same nothing by the same branch, and NaN is refused by `within`
/// without a separate test. A millimetre is far below any width this generator means (the
/// narrowest canonical profile is `RIFT_WIDTH_M = 70_000`) and is chosen for margin, not as
/// the edge of anything.
pub const WB_MIN_TECTONIC_WIDTH_M: f64 = 1.0e-3;

/// The widest admissible `continent_collision_m` / `ridge_m` profile: the range gate itself.
///
/// `Tectonics::offset_m` asks `margins_within(point, MAX_TECTONIC_RANGE_M, ..)`, so **beyond
/// 420 km a margin is not evaluated at all**. A profile still carrying weight at that
/// distance is therefore truncated to zero rather than faded to it, which is a cliff in the
/// terrain -- and `MAX_TECTONIC_RANGE_M`'s own doc says so, and says where the check belongs:
/// *"Validating a caller-supplied block against this bound belongs at the boundary that
/// admits one (the WASM export, a later task), not here -- nothing clamps."* This is that
/// task and this is that boundary.
///
/// These two profiles are centred **on** the margin (`bump(across_m, width)`), so their reach
/// is exactly their width and the ceiling is exactly the gate.
pub const WB_MAX_CENTRED_TECTONIC_WIDTH_M: f64 = MAX_TECTONIC_RANGE_M;

/// The widest admissible `coastal_uplift_width_m`: the gate, less the offset that profile
/// sits at.
///
/// `from_margin`'s profile evaluates `bump(across_m - COASTAL_UPLIFT_OFFSET_M, width)` at
/// **both** `+distance_m` and `-distance_m`, so the near side still carries weight out to
/// `offset + width`. 350 km is what is left of the 420 km gate after the 70 km offset, and
/// canonical's 260 km reaches 330 km -- inside it, as `MAX_TECTONIC_RANGE_M`'s doc says every
/// canonical profile is by construction.
pub const WB_MAX_COASTAL_UPLIFT_WIDTH_M: f64 = MAX_TECTONIC_RANGE_M - COASTAL_UPLIFT_OFFSET_M;

/// The widest admissible `island_arc_width_m`: the gate, less the arc's 60 km offset, by
/// exactly the reasoning [`WB_MAX_COASTAL_UPLIFT_WIDTH_M`] gives. Canonical's 110 km reaches
/// 170 km.
///
/// **No slider is bound to this field**, and that is deliberate: Task 1's one-ULP
/// perturbation fixtures proved seven of the nine fields are read and recorded, in the test
/// itself, that `island_arc_m` and `island_arc_width_m` have **no coverage** -- the arc term
/// is multiplied by an oceanic weight a synthetic two-plate fixture never produced. The
/// *channel* carries them, because a record is `TectonicParams` and dropping two fields from
/// the ABI would be the silently-dropping shape again; the *panel* does not, because a
/// control with no evidence the path reads it is a control that might do nothing.
pub const WB_MAX_ISLAND_ARC_WIDTH_M: f64 = MAX_TECTONIC_RANGE_M - ISLAND_ARC_OFFSET_M;

/// The floor on `continental_blend`, the "how many mountains" field.
///
/// `continental_with` computes `(value - CONTINENTAL_ENOUGH) / blend * 0.5 + 0.5` and
/// smoothsteps the result, so this is a **divisor** and a width rather than a threshold.
///
/// **Zero is not a crash; it is the defect this parameter was introduced to remove.**
/// `CONTINENTAL_BLEND`'s own doc records it: the first version used a hard test -- continental
/// if above zero -- and *"the ground jumped five hundred and fifty metres wherever a margin
/// crossed it"*. At `blend == 0.0` the division gives `±inf`, both of which the two
/// comparisons resolve to a hard 1 or 0, and at a continentality of exactly zero it gives
/// `0.0 / 0.0` -- NaN, which `continental_with`'s `if fraction < 1.0` leaves at **1.0**, so a
/// margin would read *thoroughly continental* for the reason that a NaN compares false.
/// Negative values invert the ramp entirely: ocean reads continental and continent reads
/// oceanic, which is a different mechanism wearing this parameter's name, the same objection
/// [`WB_MAX_QUIETING_STRENGTH`] makes to a quieting strength past 1.
///
/// A thousandth is chosen for margin -- canonical is 0.45 and the panel's most-mountains end
/// is 0.1 -- not as a measured edge.
pub const WB_MIN_CONTINENTAL_BLEND: f64 = 1.0e-3;

/// The ceiling on `continental_blend`, and it is **derived, not picked**.
///
/// `Setting`'s two sides are `Continentality::at`, which is `Noise::fbm` -- and `fbm`
/// normalises by `2.0 * total / loudest` over inputs in `[0, 1]` offset by `-0.5`, so its
/// output is bounded to `[-1, 1]` for every point on every world. The ramp saturates where
/// `|value| >= blend`, so **at any blend of 2 or more no margin anywhere reaches either end
/// of the ramp**, and as the blend grows every margin converges on a flat half-continental
/// reading: at 1e3 every `continental_with` is 0.5 to within 5e-4, the collision, oceanic and
/// subduction weights are 0.25 / 0.25 / 0.5 planet-wide, and the parameter has stopped
/// selecting anything.
///
/// This is 500x the saturation point, which is the margin, and it is confirmed by
/// measurement rather than left as algebra: on the owner's world at 6,000 m / 150 km the
/// count of 0.5-degree sites above 1,000 m runs 925 at blend 0.10, 618 at canonical 0.45, 332
/// at 1.00 and is still 126 at 8.00 -- a curve that is already flat two orders of magnitude
/// below this bound. `src/bin/mountain_probe.rs` is that measurement.
pub const WB_MAX_CONTINENTAL_BLEND: f64 = 1.0e3;

// ------------------------------------------------- the five structure fields, bounded
//
// Every bound below is either **derived from arithmetic already in `tectonics.rs`** or is a
// measured band from Task 2's survey, and each says which. Nothing clamps: a record is
// admitted as the host wrote it or refused entire.
//
// **Validation code is where clamping is most tempting, and this is most of this task.** So:
// no `f64::min`, no `f64::max`, no `.clamp(` -- all three are NaN-asymmetric and the
// determinism guard does not catch them. Every comparison goes through `within`, which is
// `value >= low && value <= high` and is therefore false for NaN on both sides without a
// separate NaN test anywhere in this file.

/// The floor on `collision_asymmetry`, and it is `COLLISION_SYMMETRIC` -- the canonical
/// setting itself, which is the smallest admissible one.
///
/// **Below 1.0 is a CLIFF, and it is one `TectonicParams::collision_reach_m` cannot see.**
/// `asymmetric_bump` gives the overriding flank `width_m / asymmetry`, so an asymmetry of 0.1
/// makes that flank **ten times `continent_collision_width_m`** -- a 100 km profile reaching
/// 1,000 km. `collision_reach_m` reports the *wider of the two nominal flanks* as
/// `continent_collision_width_m`, because the field's own doc guarantees this parameter can
/// only ever narrow a range, so the reach check below would report 235 km for a profile
/// carrying weight at 1,000 km and [`MAX_TECTONIC_RANGE_M`] would truncate it mid-fade. That
/// guarantee is exactly the thing this floor buys, and it is why the floor is here rather
/// than a domain preference.
///
/// At or below zero `asymmetric_bump` already treats the value as symmetric, which is the
/// silently-adjusted-parameter shape this boundary refuses on principle.
pub const WB_MIN_COLLISION_ASYMMETRY: f64 = COLLISION_SYMMETRIC;

/// The ceiling on `suture_count`, **and it is the loop bound, which makes it the one bound
/// in this file that closes a HANG rather than an odd-looking world.**
///
/// `Tectonics::sutures` runs `while index < params.suture_count`, once per convergent sample.
/// A `u32` near its maximum makes every one of them walk four billion iterations, and a hang
/// through `extern "C"` is *uninterruptible*, because that boundary is nounwind: the tab does
/// not error, it stops. `TectonicParams::suture_count`'s own doc carries this hazard at the
/// field, written there by the task that could not expose it, addressed to whoever did.
///
/// This project has found **three aborts and one ~2,600-second hang** by sweeping export
/// inputs and **zero** by spot-checking, so this is swept
/// (`the_suture_count_loop_bound_is_refused_above_its_ceiling_and_bounded_below_it`) rather
/// than argued.
///
/// **8 is twice the measured band.** Task 2's sutures table sweeps 1 through 4 and finds the
/// technique degrading on two measured axes above 2 -- the peak inflates 64% at 4 x 60 km
/// because overlapping bumps add, and the reach passes the 420 km gate at 4 x 100 km. Nothing
/// above 4 was measured to buy anything. Doubling that is margin rather than a claim, and it
/// keeps the worst admissible case at eight `asymmetric_bump` calls per sample: a constant
/// factor on a hot path, which is a slow world, not a dead tab.
pub const WB_MAX_SUTURE_COUNT: u32 = 8;

/// The floor on `suture_spread_m`. **Negative is not a mirror image; it is an unreported
/// reach.**
///
/// `collision_reach_m` computes `last * spread * (1 + SUTURE_OFFSET_JITTER)` and reports
/// `0.0` when that is not positive -- so a spread of -300 km places sutures 300 km *outboard*
/// and reports a reach of exactly `continent_collision_width_m`, and the range gate truncates
/// them. Same cliff as a sub-1.0 asymmetry, reached from the other side, and the same answer.
pub const WB_MIN_SUTURE_SPREAD_M: f64 = 0.0;

/// The ceiling on `structure_depth`, and the floor is zero.
///
/// `structure_at` returns `1 - depth + depth * ridges * segments` with both fields in
/// `[0, 1]`, so the multiplier is in `[1 - depth, 1]` -- **which is only a multiplier while
/// depth is.** Above 1 the low end goes negative and the collision profile *inverts* wherever
/// the structure field is quiet: mountains become basins, which is a different mechanism
/// wearing this parameter's name. Below 0 the low end exceeds 1 and the field *amplifies* the
/// envelope past `continent_collision_m`, so the height the panel reports stops being the
/// height the ground has.
///
/// The same objection [`WB_MAX_QUIETING_STRENGTH`] makes to a quieting strength past 1, and
/// derived from `structure_at`'s own documented range rather than chosen.
pub const WB_MAX_STRUCTURE_DEPTH: f64 = 1.0;

/// The floor on `structure_wavelength_m`. **A silence, not a crash** -- exactly what
/// [`WB_MIN_TECTONIC_WIDTH_M`] closes, and the same value for the same reason.
///
/// `structure_at` opens with `if params.structure_wavelength_m <= 0.0 { return 1.0 }`, so a
/// zero or negative wavelength is a structure field that is present in the record, accepted by
/// the constructor, and contributes exactly nothing at every point on the planet -- while
/// `structure_depth` sits beside it looking configured. The silently-dropping-builder shape.
pub const WB_MIN_STRUCTURE_WAVELENGTH_M: f64 = WB_MIN_TECTONIC_WIDTH_M;

/// The floor on `margin_warp_m`. **A magnitude, and the sign is not a second parameter.**
///
/// A negative amplitude is the exact mirror of the positive one on the same margin -- the
/// belt is displaced the other way and nothing else about it changes -- so admitting it adds
/// a second spelling of a world the caller can already ask for, on a field whose whole
/// meaning is "how far". It also puts the field and
/// [`TectonicParams::collision_reach_m`] into two different sign conventions: the reach takes
/// `abs` precisely so a mirrored warp cannot report a reach the profile does not have, and a
/// floor here means that `abs` is the second line of defence rather than the only one. Both
/// are tested, on both sides.
pub const WB_MIN_MARGIN_WARP_M: f64 = 0.0;

/// The ceiling on `margin_warp_m`, **derived from the range gate**, and a restatement rather
/// than the only check.
///
/// The warp displaces the whole collision profile sideways, so
/// [`TectonicParams::collision_reach_m`] adds this amplitude and the reach check below holds
/// the total against [`MAX_TECTONIC_RANGE_M`]. That check is the binding one -- it is what
/// stops a warped belt being truncated into the cliff `MAX_TECTONIC_RANGE_M`'s own doc exists
/// to prevent -- and this per-field ceiling is kept beside it for the reason the width
/// ceilings are kept beside it: it stays true if `collision_reach_m`'s definition ever moves.
pub const WB_MAX_MARGIN_WARP_M: f64 = MAX_TECTONIC_RANGE_M;

/// The floor on `margin_warp_wavelength_m`. **A silence, not a crash**, and the same value
/// and the same reason as [`WB_MIN_STRUCTURE_WAVELENGTH_M`].
///
/// `Tectonics::margin_warp_m_at` opens with `if wavelength <= 0.0 { return 0.0 }`, so a zero
/// or negative wavelength is a warp that is present in the record, accepted by the
/// constructor, and displaces nothing anywhere on the planet -- with `margin_warp_m` sitting
/// beside it looking configured. The silently-dropping-builder shape.
pub const WB_MIN_MARGIN_WARP_WAVELENGTH_M: f64 = WB_MIN_TECTONIC_WIDTH_M;

/// The ceiling on `margin_warp_wavelength_m`, and it is **NOT** the range gate.
///
/// [`WB_MAX_STRUCTURE_WAVELENGTH_M`] is the gate because the structure field only multiplies
/// the collision profile, which is identically zero beyond it -- so a longer wavelength there
/// cannot complete a cycle anywhere it can act. **The warp is the other way round.** It varies
/// along the margin, and a margin is a great circle: it runs the whole way round the planet.
/// The distance the field has to work over is the circumference, not the belt width, so the
/// gate would be a ceiling two orders of magnitude below the field's own domain.
///
/// [`WB_MAX_WORLD_RADIUS_M`] is the bound instead, for the reason
/// [`WB_MAX_RELIEF_WAVELENGTH_M`] takes the same value: a wavelength larger than the largest
/// admissible planet is a caller mistake, and `+inf` is refused with it. It is a domain
/// statement and not a measured edge -- what IS measured is that long wavelengths go quiet
/// long before this: on a 350 km belt a 900 km wavelength moves the crest's deviation from
/// 3.6 km to 9.4 km against 34.1 km at 300 km, because **a bend longer than the belt is a
/// tilt**. Same shape as `structure_wavelength_m`'s dead 120-250 km band, and stated here so
/// nobody reads this ceiling as a useful setting.
pub const WB_MAX_MARGIN_WARP_WAVELENGTH_M: f64 = WB_MAX_WORLD_RADIUS_M;

/// The ceiling on `structure_wavelength_m`, **derived from the range gate rather than picked**.
///
/// The structure field is *multiplied into* the collision profile and nothing else, and that
/// profile is identically zero beyond [`MAX_TECTONIC_RANGE_M`] because `offset_m` never
/// evaluates a margin further away than that. So a ridge wavelength longer than the gate
/// cannot complete a cycle anywhere the field is able to act: it is a constant multiplier
/// wearing a wavelength's name, and `structure_depth` would then read as an amplitude.
///
/// Task 2's own table is the confirmation rather than the argument: at 250 km -- already
/// inside this ceiling -- the summit count falls back to 0-3 at every depth, against 12 at
/// 40 km. The measured working band is 40-80 km, two orders of magnitude clear of the floor
/// and five times clear of this ceiling.
pub const WB_MAX_STRUCTURE_WAVELENGTH_M: f64 = MAX_TECTONIC_RANGE_M;

// ------------------------------------------------------- the coastal channel, Task 6
//
// The fractal-coastline slice, Task 6. Task 5 built `CoastParams` -- a separate roughening
// term windowed by `|above_shore|`, applied AFTER calibration so land fraction cannot move --
// measured it (the coastline ratio GROWS with measurement resolution, 1.358 at a 100 km ruler
// to 1.639 at 12.5 km, while a deliberately smooth control at the same amplitude sits flat at
// 1.02-1.03; inlet heads 2 -> 175), and then **stopped at this boundary**. Its own report says
// so in as many words: "there is no `wb_world_new_coast` ... the owner cannot see this yet."
// This is that door.
//
// Same shape as the relief and tectonic channels above, for the third time: a flat f64 record
// in a documented order, a preset export so no host transcribes a number, a checker that
// answers *why* rather than only *that*, and a constructor that refuses a record entire rather
// than admitting it with one field adjusted. **Nothing here clamps.**

/// f64 words per coast record, **and the order is the contract**:
///
/// | index | field |
/// |---:|---|
/// | 0 | `amplitude` |
/// | 1 | `window_spreads` |
/// | 2 | `frequency` |
/// | 3 | `octaves` -- **an integer carried as an f64, and a LOOP BOUND**, see [`WB_MAX_COAST_OCTAVES`] |
/// | 4 | `gain` |
/// | 5 | `lacunarity` |
///
/// That is `CoastParams`'s own declaration order, and [`wb_coast_preset`] writes it in exactly
/// this order so a host never has to transcribe a value.
pub const WB_COAST_STRIDE: usize = 6;

/// [`wb_coast_preset`] selector: `CoastParams::canonical()`, today's six values and the `None`
/// path's exact equivalent. Its `amplitude` is exactly 0.0 and `Continentality::above_shore`
/// branches on that before sampling, so this preset is inert by construction.
pub const WB_COAST_CANONICAL: u32 = 0;

/// [`wb_coast_preset`] selector: `CoastParams::fractal()`, the preset Task 5 swept and chose.
///
/// **It crosses as FIELDS, never as a name**, the same Ruling 7 the tectonic preset is held to:
/// the panel receives six numbers, puts the one it has calibrated travel for on a slider and
/// shows the other five, so the owner sees what the preset asked for.
pub const WB_COAST_FRACTAL: u32 = 1;

/// The floor on `amplitude`. **A magnitude, and the sign is not a second parameter** -- the
/// same statement [`WB_MIN_MARGIN_WARP_M`] makes, for the same reason.
///
/// `coast_offset` multiplies `amplitude * spread * window * field`, and the coast lattice is
/// zero-mean, so a negative amplitude is the same displacement drawn from the negated field.
/// That is not a shape a caller can want and cannot ask for otherwise; it is a second spelling
/// of "how far", on a field whose whole meaning is how far. Exactly 0.0 is admitted, because
/// 0.0 is what `canonical()` carries and the canonical record must cross this channel.
pub const WB_MIN_COAST_AMPLITUDE: f64 = 0.0;

/// The ceiling on `amplitude`, in multiples of `spread`. **A domain statement with margin, and
/// the useful band is far below it** -- Task 5 measured the whole travel and this is not it.
///
/// The measured band on the owner's world (seed 562423712, R = 4,500,000 m, 28 plates, land
/// 0.16, a 25 km grid) is **0.10 to 0.75**: below 0.10 the coast lengthens by under 3% and the
/// inlet count is single digits, and from 0.75 upward the small-island count runs away
/// (17 -> 23 -> 28) while the >= 100,000 km2 count stays flat at 9-10, which is speckle rather
/// than new continents. `fractal()` takes 0.35 and the panel's slider stops at 0.75.
///
/// This ceiling sits at **four**, above every row Task 5 swept (its table stops at 1.5), and it
/// is a domain statement rather than a measured hazard: `elevation_from_above` saturates at
/// `|above| >= 1` and the window is bounded by 1, so no amplitude in this range can make an
/// elevation non-finite. That is asserted by *sampling* every accepted record in the sweep
/// rather than by argument -- the posture [`WB_MAX_TECTONIC_AMPLITUDE_M`] takes, and for the
/// reason its doc gives: the hazards this project has found were bands nobody would have picked
/// by hand.
pub const WB_MAX_COAST_AMPLITUDE: f64 = 4.0;

/// The floor on `window_spreads`. **A silence, not a crash**, and the same shape
/// [`WB_MIN_STRUCTURE_WAVELENGTH_M`] closes.
///
/// `Continentality::coast_offset` computes `reach = spread * window_spreads` and closes the
/// window entirely when that is not positive -- so a zero, negative or NaN width is a term
/// present in the record, accepted by the constructor, and contributing exactly nothing at
/// every point on the planet, with `amplitude` sitting beside it looking configured. The
/// silently-dropping-builder shape.
///
/// **And a strictly-positive floor would not close it**, which is why this is a number rather
/// than `> 0.0`: `calibrate` floors `spread` at `1e-6`, so any `window_spreads` below about
/// `2.2e-302` underflows the product to exactly zero and reaches the same silence from inside a
/// strict inequality. This floor is twelve orders above that underflow -- margin, not the
/// measured edge, the same posture [`WB_MAX_WORLD_RADIUS_M`] takes -- and a band a thousandth
/// of `spread` wide is already far narrower than the shore feature it is meant to roughen.
pub const WB_MIN_COAST_WINDOW_SPREADS: f64 = 1.0e-3;

/// The ceiling on `window_spreads`, in multiples of `spread`.
///
/// **Nothing above 1.0 buys much, and that is derived rather than asserted**:
/// `Continentality::base_elevation` normalises `above_shore` by `spread` and
/// `elevation_from_above` saturates at `|above| >= 1`, so past one spread from the shore the
/// drawn ground is already at its plateau and displacing it further cannot change what is
/// drawn. `canonical()` therefore carries exactly 1.0.
///
/// The ceiling is four rather than one because the window is not the only reader of
/// `above_shore` -- `shelf.rs` weights on the coast and `tectonics.rs` probes inboard along a
/// margin -- so a caller may legitimately want a band wider than the one the hypsometric curve
/// can show. Stated here so nobody reads this ceiling as a useful setting.
pub const WB_MAX_COAST_WINDOW_SPREADS: f64 = 4.0;

/// The floor on `frequency`, **derived from the sphere the field is sampled on**.
///
/// `Continentality::coast_offset` hands the point's unit-sphere coordinates straight to
/// `Noise::fbm`, so the noise-space extent of the whole planet is a diameter of 2 in each
/// coordinate. A frequency below one half therefore cannot complete a single cycle anywhere on
/// the world: the term becomes a near-constant displacement of the entire coastline -- every
/// shore moved the same way -- rather than a roughening of it, while `amplitude` sits beside it
/// reading as a roughness. Same silently-dropping-builder shape as a zero-width profile, and
/// the same answer.
pub const WB_MIN_COAST_FREQUENCY: f64 = 0.5;

/// The ceiling on `octaves`, and **this is the loop bound**.
///
/// `Noise::fbm` runs `for _ in 0..octaves`, once per sample, and a sample is taken per texel of
/// every tile. `octaves` crosses this channel as an f64 word, and `value as u32` in Rust
/// **saturates**: 1e300 would arrive as `u32::MAX` and every elevation on the planet would walk
/// four billion noise evaluations. That is a **HANG, not an abort**, and it is the shape this
/// project has already found once -- see [`WB_MAX_SUTURE_COUNT`], whose doc records the
/// ~2,600-second measurement that motivated bounding it. `decode_coast_octaves` refuses a word
/// that is not finite, exactly integral and inside this ceiling *before* the cast, and
/// `coast_is_admissible` states the same bound a second time for a caller that built its
/// `CoastParams` in Rust.
///
/// **16 is four times the measured setting.** `canonical()` carries 4, and at `frequency = 20`
/// with `lacunarity = 2` a sixteenth octave sits at 655,360 cycles over the sphere -- a
/// wavelength of about 43 m of arc on the owner's 4,500,000 m planet, three orders below the
/// finest post spacing any tile in this viewer asks for. Nothing above this buys a feature
/// anyone can see; the margin is margin.
pub const WB_MAX_COAST_OCTAVES: u32 = 16;

/// The ceiling on `gain`, and the floor is zero.
///
/// `Noise::fbm` multiplies the running amplitude by `gain` each octave, so a gain above one
/// makes each octave **louder** than the one before it: `frequency` stops naming the finest
/// band, `octaves` stops being a detail count, and the parameter that reads as "how quickly the
/// detail fades" is running the schedule backwards. Far enough above one the amplitude overflows
/// to `+inf`, `loudest` overflows with it, and `2.0 * total / loudest` is `inf / inf` -- **a NaN
/// in `above_shore`**.
///
/// **A NaN there used NOT to show up as a NaN, which is why this ceiling was drawn rather than
/// left to a finiteness assertion downstream.** `Continentality::elevation_from_above` read
/// `if above >= 0.0` (false for NaN) and then `if depth < 1.0` (false for NaN), so a NaN fell
/// through both comparisons to `ABYSS_M * 1.0`: **every affected point silently became the
/// deepest abyss and every `is_finite` check in this crate stayed green.** Measured, on the
/// fixture world at `gain = 1e300`: `above_shore` was NaN at all three probes and `elevation_m`
/// read -4,599 m to -4,625 m. A drowned planet that passes its own health checks is worse than
/// a refusal, and worse than a NaN.
///
/// **`elevation_from_above` now carries an explicit `is_nan` arm and propagates**, closing that
/// swallowing for every future term as well as this one -- see its own doc for the three entrants
/// it stands in front of. **The ceiling stays and is not made redundant by it.** A refusal names
/// the offending *field* through `wb_coast_check`'s status; a NaN elevation tells a host only
/// that something somewhere is wrong. This is the first line and the guard is the second.
/// `a_nan_in_the_coastal_term_surfaces_as_a_nan_instead_of_drowning_the_world` pins the pair so
/// this paragraph cannot go stale.
///
/// Zero is admitted and is not a silence: at `gain = 0` the first octave still carries its full
/// amplitude and `loudest` is 1.
pub const WB_MAX_COAST_GAIN: f64 = 1.0;

/// The floor on `lacunarity`. Below one the octave schedule runs **coarse**, not fine: each
/// octave's frequency is smaller than the last, so `frequency` names the finest band instead of
/// the coarsest and [`WB_MAX_COAST_FINEST_FREQUENCY`]'s product check -- which multiplies
/// forward -- would be watching the wrong end. At or below zero the frequency alternates sign
/// or collapses to a constant. One is admitted and means every octave at the same frequency.
pub const WB_MIN_COAST_LACUNARITY: f64 = 1.0;

/// The ceiling on `lacunarity`. A domain statement; `canonical()` carries 2.0, the doubling
/// every octave schedule in this engine uses. **The binding check is not this one** -- it is
/// [`WB_MAX_COAST_FINEST_FREQUENCY`], which holds the *product* this field compounds into.
pub const WB_MAX_COAST_LACUNARITY: f64 = 16.0;

/// **The ceiling on the FINEST octave's frequency, which is the one bound on this channel that
/// closes a measured abort -- and no per-field ceiling can see it.**
///
/// `Noise::fbm` multiplies the frequency by `lacunarity` once per octave, so the finest band a
/// record asks for is `frequency * lacunarity^(octaves - 1)`. Three fields compound into that
/// product and every one of them is individually inside its own domain at values whose product
/// is not: `frequency = 1e6`, `lacunarity = 16` and `octaves = 16` are each admissible alone and
/// together ask for `1e6 * 16^15`, about `1.15e24`.
///
/// **That is an abort.** `Noise::at` floors each coordinate and casts to `i64`; the cast
/// SATURATES at `i64::MAX` for anything above about `9.22e18`, and the very next line computes
/// `ix + 1`, which overflows -- a panic under the overflow checks Rust's dev and test profiles
/// enable by default, and a wrapped index into a different lattice cell in release. It is the
/// same hazard [`WB_MAX_WORLD_RADIUS_M`]'s doc records reaching through a huge radius, arrived
/// at from the other side. A panic across `extern "C"` is an abort; a silently wrapped lattice
/// index is a world nobody asked for. Neither is acceptable and this refuses both.
///
/// **This is the "bound the product" lesson [`WB_MAX_EROSION_NODES`]'s doc asks for, taken.**
/// That doc records two ceilings that are individually safe and jointly a month of work, and
/// says a future caller "must bound the *product*". This channel does, in code, in
/// `coast_is_admissible`, by walking the octave schedule the engine itself will walk.
///
/// 1e6 is twelve orders below the saturation point and is itself far past useful: a frequency of
/// 1e6 over the unit sphere is a wavelength of about 4.5 m of arc on the owner's planet, well
/// below any post spacing the viewer asks for.
pub const WB_MAX_COAST_FINEST_FREQUENCY: f64 = 1.0e6;

/// The ceiling on `plate_count`, and it is a *refusal*, not a clamp.
///
/// Every sample walks the plate table and `Surface::new` builds it, so a plate count in the
/// millions is not a slow world -- it is a hung tab. Earth has about fifteen; this crate's
/// own fixtures use 8, 12 and 24. Anything above this is a caller mistake, and the honest
/// answer to a caller mistake is a refusal rather than a silently different world.
pub const WB_MAX_PLATE_COUNT: u32 = 4096;

/// The ceiling on `radius_m` for [`wb_world_new`] -- defence in depth alongside
/// `StreamGraph::build`'s area check, not a replacement for it.
///
/// The whole-branch review of slice 5a found that `radius_m` above roughly `3.78e153`
/// (`sqrt(f64::MAX / (4*pi))`) overflows `4*pi*radius_m^2` to `+inf` inside
/// `stream::node_areas_m2`, which `StreamGraph::build`'s area check now refuses (it
/// previously admitted `+inf` as `> 0.0`) -- see that check's own doc for the abort this
/// closes. **Bounding `radius_m` here catches the same caller mistake one call earlier,
/// and catches something the area check alone cannot: sampling elevation at a huge-radius
/// world's own coordinates overflows a completely unrelated `i64` cast in `noise.rs`'s
/// lattice-cell arithmetic** (`(x as i64) + 1` on a floored coordinate that already
/// saturated to `i64::MAX`), which panics under the overflow checks Rust's dev/test
/// profile enables by default -- and panics *before* a caller ever reaches the area
/// overflow. Measured on this host: `Surface::elevation_m` panics at `noise.rs:95` for
/// radii from `1e25` upward and returns ordinary finite output through `1e20`; this bound
/// sits at `1e9` m (roughly 157x Earth's radius, already an absurd "planet" for anything
/// this generator is meant to produce), eleven orders of magnitude below the lower of the
/// two hazards and effectively immune to either moving with a future change to the noise
/// or area arithmetic. This is chosen for margin, not measured as the exact edge of
/// either hazard -- the same posture `WB_MAX_EROSION_RATE_PER_YR`'s doc already takes for
/// its own overflow margin.
pub const WB_MAX_WORLD_RADIUS_M: f64 = 1.0e9;

/// Alignment for every buffer `wb_alloc` hands out: 8, so the same allocation serves an f64
/// payload (`wb_bottom_at`) and an f32 tile without the host having to think about it.
const WB_ALIGN: usize = 8;

// -------------------------------------------------------------------- the gully channel
//
// The fifth block of this kind, and the first that ADDS A TERM to `elevation_m` rather than
// changing one that was already there. Same shape as the four above it: a flat f64 record in
// linear memory, decoded through a raw pointer, bounds-checked field by field, refused entire
// if any one field is outside its documented domain. It is a fifth instance of one convention
// and not a fifth convention.

/// f64 words per gully record, **and the order is the contract**:
///
/// | index | field |
/// |---:|---|
/// | 0 | `amplitude_m` |
/// | 1 | `cell_m` |
/// | 2 | `slope_reference` |
/// | 3 | `stripes_per_cell` |
/// | 4 | `max_stripes_per_cell` |
/// | 5 | `crest_sharpness` |
/// | 6 | `gate_elevation_m` |
/// | 7 | `gate_elevation_span_m` |
/// | 8 | `flat_energy_floor` |
/// | 9 | `steer_lattice_m` |
/// | 10 | `harmonic_weight` |
/// | 11 | `harmonic_band_m` |
///
/// That is `GullyParams`'s own declaration order, and [`wb_gully_preset`] writes it in exactly
/// this order so a host never transcribes a preset's numbers.
///
/// **The stride went from ten to twelve when the second harmonic shipped.** It is an
/// append -- indices 0 through 9 mean exactly what they meant -- but this boundary refuses a
/// record of the wrong length outright rather than defaulting the tail, which is the
/// silently-dropping-builder shape the rest of this file refuses: a host that still sends ten
/// words gets `WB_GULLY_REFUSED`, not a block whose new fields quietly read as zero.
///
/// **The stride did NOT move when the pitchfork moved to a local datum**, and that is worth
/// saying because the change was larger than the one that did move it. Word 11 kept its slot
/// and its name and changed its MEANING -- metres of local relief above the steer cell's own
/// mean `structural_m`, where it used to be metres of world elevation above
/// `gate_elevation_m` -- so a host that stored a gully record from before that slice and
/// replays it will be accepted and will get a different planet. There is no version word in
/// this record to catch that, and there is nowhere to put one without moving the stride; the
/// mitigation is that [`WB_MIN_GULLY_HARMONIC_BAND_M`] and the preset both moved by three
/// orders of magnitude, so a stale band of 1,800 saturates `a` rather than doing something
/// subtle. See `GullyParams::harmonic_band_m`.
pub const WB_GULLY_STRIDE: usize = 12;

/// [`wb_gully_preset`] selector: `GullyParams::canonical()` -- the drainage kernel switched
/// off, and the `None` path's exact equivalent.
pub const WB_GULLY_CANONICAL: u32 = 0;
/// [`wb_gully_preset`] selector: `GullyParams::drainage()`, the measured preset.
pub const WB_GULLY_DRAINAGE: u32 = 1;

/// The ceiling on `amplitude_m`. The floor is `0.0` and `0.0` is the off switch, exactly as
/// it is for the five relief amplitudes: `Detail::gully_offset_m` returns before it can add
/// anything, so a negative amplitude would be a field that looks configured and inverts the
/// term -- the silently-dropping-builder shape this file refuses elsewhere. Set to
/// [`WB_MAX_RELIEF_AMPLITUDE_M`]'s value because it is the same kind of quantity: metres of
/// vertical displacement.
pub const WB_MAX_GULLY_AMPLITUDE_M: f64 = WB_MAX_RELIEF_AMPLITUDE_M;

/// The floor and ceiling on `cell_m` and on `steer_lattice_m`, the two lengths in this
/// record. Both are divisors of the planet's radius on their way to a lattice index, and
/// both therefore have the same failure at zero: `radius / 0` is an infinity, `floor(inf)`
/// is an infinity, and `as i64` saturates onto the `+ 1` that `Noise::at`'s own guard
/// documents as an abort behind a nounwind boundary.
///
/// **Both are guarded twice**, and deliberately: this pair refuses the record at the door,
/// and `Detail::gully_offset_m` and `SteerLattice::at` each re-check the resulting lattice
/// index against 9e18 and answer a flat, zero term rather than indexing. First line and
/// second line, the same posture `WB_MAX_COAST_GAIN` takes beside
/// `Continentality::elevation_from_above`'s NaN guard. With this floor at one metre and
/// [`WB_MAX_WORLD_RADIUS_M`] at 1e9, the largest index either lattice can be asked for is
/// 1e9 -- nine orders below the guard.
pub const WB_MIN_GULLY_LENGTH_M: f64 = 1.0;
/// See [`WB_MIN_GULLY_LENGTH_M`]. Set to [`WB_MAX_RELIEF_WAVELENGTH_M`]'s value for the same
/// reason that one was: a length larger than the largest admissible planet is a caller
/// mistake.
pub const WB_MAX_GULLY_LENGTH_M: f64 = WB_MAX_RELIEF_WAVELENGTH_M;

/// The floor and ceiling on `slope_reference`, which is a divisor of a slope and is the one
/// field in this record whose value was measured rather than chosen (`GullyParams::drainage`
/// carries the population). Zero does not abort -- the quotient is an infinity or a NaN, and
/// every downstream comparison is written negated so a NaN leaves by the refusing door -- so
/// this pair is a domain statement rather than a closed hazard. The floor is far below any
/// slope this or any plausible generator produces; the ceiling is a slope of 1,000 m/m, which
/// is 89.94 degrees.
pub const WB_MIN_GULLY_SLOPE_REFERENCE: f64 = 1.0e-9;
/// See [`WB_MIN_GULLY_SLOPE_REFERENCE`].
pub const WB_MAX_GULLY_SLOPE_REFERENCE: f64 = 1.0e3;

/// The ceiling on `stripes_per_cell` and on `max_stripes_per_cell`; the floor on both is
/// `0.0`, at which the kernel degenerates to the pivot lattice's own blobs rather than
/// misbehaving.
///
/// **This is a bound on a PHASE, not on a loop.** The kernel has no loop a host can lengthen
/// -- the pivot window is always the eight corners of one cell -- so an unbounded frequency
/// is not a hung tab. What it is instead is a cosine of an argument large enough that its
/// argument reduction carries no information: at 1e3 stripes per cell the phase is at most
/// about 1e4 radians, where `detmath::cos` is still exact to the last few bits, and past
/// about 1e16 it would be a deterministic function of nothing in particular. A thousand
/// stripes across a cell is already 250 times finer than the finest the measured preset asks
/// for, so nothing this refuses is anything a caller could want.
pub const WB_MAX_GULLY_STRIPES: f64 = 1.0e3;

/// The floor and ceiling on `crest_sharpness`, **and this floor closes a real hazard rather
/// than stating a domain.**
///
/// The edge shaping is `1 - 2 * folded^crest_sharpness`, with `folded` in `[0, 1]` and
/// **exactly `0` at a crest**. At a non-positive exponent `0^s` is `+inf` (or `1` at exactly
/// zero), so a single negative f64 in word 5 turns every crest in the world into an infinite
/// height, which crosses this boundary as a non-finite elevation and into a host's vertex
/// buffer. Measured on this host: `wb_elevation_m` with `crest_sharpness = -0.5` returns
/// `-inf` on gated ground. A NaN or an infinity in a height is the plausible-value failure
/// this crate has now found several times, one level worse.
///
/// The ceiling is a domain statement: at 100 the term is a flat floor with needle crests, and
/// nothing above it is a different shape.
pub const WB_MIN_GULLY_SHARPNESS: f64 = 1.0e-3;
/// See [`WB_MIN_GULLY_SHARPNESS`].
pub const WB_MAX_GULLY_SHARPNESS: f64 = 1.0e2;

/// The magnitude bound on `gate_elevation_m`, which is signed because a caller may legitimately
/// want the gate open below datum. Same magnitude as the amplitude ceiling, and for the same
/// reason: it is metres of ground.
pub const WB_MAX_GULLY_GATE_ELEVATION_M: f64 = WB_MAX_RELIEF_AMPLITUDE_M;

/// The floor and ceiling on `gate_elevation_span_m`, which is a divisor. Zero does not abort
/// -- `smooth` clamps an infinity and a NaN alike to `1.0`, which `detail.rs` documents -- so
/// this pair is a domain statement. Set to [`WB_MIN_QUIETING_SCALE_M`]'s and
/// [`WB_MAX_QUIETING_SCALE_M`]'s values, which are the same kind of quantity in the same file.
pub const WB_MIN_GULLY_GATE_SPAN_M: f64 = WB_MIN_QUIETING_SCALE_M;
/// See [`WB_MIN_GULLY_GATE_SPAN_M`].
pub const WB_MAX_GULLY_GATE_SPAN_M: f64 = WB_MAX_QUIETING_SCALE_M;

/// The ceiling on `harmonic_weight`. The floor is `0.0`, and `0.0` is the off switch on the
/// same argument `WB_MAX_GULLY_AMPLITUDE_M` makes for `amplitude_m`: a NEGATIVE weight is not
/// merely a smaller effect, it moves the second harmonic's minima onto the first's maxima and
/// splits every channel where it should join them -- a field that looks configured and
/// inverts the term.
///
/// Four is chosen rather than one because the shaping stays bounded for any non-negative
/// weight -- it is divided by `1 + a`, so `|signal| <= 1` however large `a` is -- and the
/// surveyed cross product runs to 2.0. It is a domain, not a taste: the pitchfork is at
/// `a = 1/4` and everything above it is two channels per period, so beyond a few units the
/// field stops changing shape and only the ramp's position matters.
pub const WB_MAX_GULLY_HARMONIC_WEIGHT: f64 = 4.0;

/// The floor and ceiling on `harmonic_band_m`. **Zero is refused and that is not
/// politeness**: the band is a divisor, `(shaped - reference) / 0` is an infinity or -- at
/// exactly the reference -- a NaN, and while `smooth`'s clamp order makes both of those land
/// on a finite weight rather than on a NaN height, a band of zero is a step function in the
/// local relief and a step in `a` is a step in the ground. That is the artefact every smooth
/// gate in `detail.rs` exists to avoid, so it is refused at the door.
///
/// **The floor moved from 1 m to a millimetre when the band stopped being a world elevation
/// and became a LOCAL relief**, and that is a domain correction rather than a loosening. It
/// is no longer one of this record's lengths: the other two divide the planet's radius on
/// their way to a lattice index, and this one divides `shaped - reference`, whose measured
/// p10-to-p90 on this generator's flanks is **0.14 m** -- so the old floor sat an order of
/// magnitude ABOVE every useful setting and the shipped preset of 0.12 m would have been
/// refused by the door it came through. A millimetre is three orders below that measured
/// spread, which is already a step in practice; the floor's job is to refuse the division,
/// not to express a taste. The ceiling is unchanged and is still the two lengths' own.
pub const WB_MIN_GULLY_HARMONIC_BAND_M: f64 = 1.0e-3;
/// See [`WB_MIN_GULLY_HARMONIC_BAND_M`].
pub const WB_MAX_GULLY_HARMONIC_BAND_M: f64 = WB_MAX_GULLY_LENGTH_M;

// ------------------------------------------------------------------------- climate

/// f32 per sample [`wb_climate_tile_f32`] writes: `[temperature_c_at_datum, moisture_index]`,
/// in that order, and **the order is the ABI**.
///
/// # Why the temperature channel is at the DATUM and not at the ground
///
/// `climate::temperature_c` is a latitude profile minus a lapse rate times the elevation, and
/// the elevation term is the only one that varies at terrain frequencies. A consumer that
/// rasterises this grid coarsely -- which is the entire reason this export exists; see the
/// cost note below -- and then interpolates would get a temperature whose lapse term was
/// smoothed over the coarse grid, so a 2,000 m peak inside one coarse cell would come back at
/// its neighbourhood's mean height and lose its snow line. Writing the datum value instead
/// lets the consumer apply the lapse at **its own** raster's resolution, against the heights it
/// already has from [`wb_fill_tile_f32`]. The lapse rate it needs for that is reported by
/// [`wb_climate_calibration`], so the consumer does not carry its own copy of the constant.
///
/// The datum channel is therefore a pure function of latitude and could be computed by the
/// host. It is written anyway, per sample, because a host that computed it would be a second
/// place `EQUATOR_C` and `POLE_C` live -- and this project has now found eight transcription
/// defects across six slices.
pub const WB_CLIMATE_STRIDE: usize = 2;

/// f64 [`wb_climate_calibration`] writes: four moisture edges, two landform edges in metres,
/// the land sample count, and the lapse rate in C per km -- in that order, and **the order is
/// the ABI**.
///
/// It is `climate::MOISTURE_BELL_Z.len() + climate::LANDFORM_QUANTILES.len() + 2` rather than
/// a literal 8, so a band added on either quantiled axis moves this and does not silently
/// truncate the payload.
pub const WB_CLIMATE_CALIBRATION_STRIDE: usize =
    climate::MOISTURE_BELL_Z.len() + climate::LANDFORM_QUANTILES.len() + 2;

/// The `march_samples` sentinel meaning "the engine's own canonical march", i.e. the `None`
/// that `Surface::moisture_index` takes.
///
/// **It is `u32::MAX` and not `0`, and that is deliberate rather than perverse.**
/// `climate::MarchBudget::new(0)` is admitted on purpose -- a zero-step march is the identity
/// element, the air has travelled nowhere and is still saturated, and `climate.rs` pins that
/// answer as `1.0` rather than as "the loop did not run". Spelling canonical as `0` would
/// delete a value the engine deliberately admits, which is the shape of decision this project
/// keeps having to undo.
pub const WB_CLIMATE_CANONICAL_SAMPLES: u32 = u32::MAX;

/// The ceiling on `march_samples`, and **it is a LOOP BOUND**: the march walks this many steps
/// inside one uninterruptible `extern "C"` call, per sample, and a tile is `width * height`
/// samples. The spike measured `count = 2^20` at 0.64 s, so an unbounded `u32` extrapolates to
/// ~2,600 s inside a call a host cannot cancel -- a hang, not a slow answer. The same posture
/// [`WB_MAX_SUTURE_COUNT`] and [`WB_MAX_COAST_OCTAVES`] take, and for the same measured reason.
///
/// This is `climate::MAX_MARCH_SAMPLES` and not an independent number: the type already refuses
/// above it (`MarchBudget::new` returns `None`), so a larger ceiling here could not be honoured
/// and a smaller one would be a second, disagreeing bound. What this constant adds over the
/// type is a **named status** -- `WB_ERR_PARAM` says which argument was wrong, where the type
/// alone could only decline to build.
pub const WB_MAX_CLIMATE_MARCH_SAMPLES: u32 = climate::MAX_MARCH_SAMPLES as u32; // cast-ok: a u16 ceiling widened to the ABI's integer, exact for every u16

/// **The export list, declared.** A native test run cannot see the artifact's export
/// section, and a forgotten no-mangle attribute is invisible in a build that exits 0 -- so
/// this list is checked against this file's own source by a test, and the built `.wasm` is
/// checked against this list at build time.
pub const WB_EXPORTS: &[&str] = &[
    "wb_generator_version",
    "wb_alloc",
    "wb_dealloc",
    "wb_world_new",
    "wb_world_new_relief",
    "wb_relief_preset",
    "wb_relief_check",
    "wb_world_new_tectonic",
    "wb_tectonic_preset",
    "wb_tectonic_check",
    "wb_world_new_coast",
    "wb_coast_preset",
    "wb_coast_check",
    "wb_world_new_gully",
    "wb_gully_preset",
    "wb_gully_check",
    "wb_world_free",
    "wb_world_count",
    "wb_elevation_m",
    "wb_structural_m",
    "wb_bottom_at",
    "wb_fill_tile_f32",
    "wb_climate_tile_f32",
    "wb_climate_calibration",
    "wb_erosion_run",
    "wb_water_run",
    "wb_hydro_bake",
    "wb_hydro_len",
    "wb_hydro_copy",
    "wb_hydro_free",
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

    /// Held hydrology bakes, encoded to the flat f64 record `hydrology::record::encode`
    /// already produces. Same discipline as `WORLDS`: 1-based ids, never reused, a freed slot
    /// stays freed. A bake is not a world, so it gets its own table rather than sharing
    /// `WORLDS`'s handle space -- `wb_hydro_bake` takes a world handle as an *input* and
    /// returns a *different* kind of id, and conflating the two spaces would let a stale
    /// world handle and a live bake id collide by coincidence of the same integer.
    static HYDRO: RefCell<Vec<Option<Vec<f64>>>> = const { RefCell::new(Vec::new()) };
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

// ------------------------------------------------------------------ the relief channel

/// `low <= value <= high`, **and NaN answers `false`**, which is the whole reason this is a
/// named function rather than a `.clamp(` or a pair of `f64::min`/`f64::max` calls.
///
/// The house rule against those three forms exists because they are NaN-asymmetric, and
/// `tests/no_std_math.rs`'s guard does not catch them -- so validation code, which is where
/// clamping is most tempting, gets an explicit-branch form instead, the same shape
/// `plates.rs::margin_at` uses. Written as two comparisons because IEEE 754 makes every
/// comparison against NaN false: `NaN >= low` is false, so a NaN field is refused by the
/// same expression that bounds a finite one, with no separate `is_finite` test.
///
/// Both infinities fall out for free too: `+inf <= high` is false and `-inf >= low` is
/// false for every finite `low`/`high`. **A clamp here would have silently turned each of
/// those into a bound**, which is exactly the band-not-cliff failure this task's sweep is
/// looking for.
fn within(value: f64, low: f64, high: f64) -> bool {
    value >= low && value <= high
}

/// Whether a relief block is one this boundary will let reach `Surface::new`.
///
/// Every bound is documented on its own constant, with which ones close a real hazard
/// (`WB_MIN_RELIEF_WAVELENGTH_M` and `WB_MAX_RELIEF_WAVELENGTH_M` close a **non-terminating
/// loop** in `Detail::plan`) and which are domain statements. Nothing here clamps: a record
/// is admitted as the host wrote it or refused entire, because a silently-adjusted
/// parameter is a world nobody asked for.
fn relief_is_admissible(relief: &ReliefParams) -> bool {
    for wavelength in [relief.canonical_wavelength_m, relief.coarsest_wavelength_m] {
        if !within(wavelength, WB_MIN_RELIEF_WAVELENGTH_M, WB_MAX_RELIEF_WAVELENGTH_M) {
            return false;
        }
    }
    // The band schedule runs coarse to fine. A record whose coarsest band is finer than its
    // canonical one plans *zero* octaves -- a world with no detail at all, which reads as a
    // working world with the roughness silently switched off rather than as a refusal.
    if !(relief.coarsest_wavelength_m >= relief.canonical_wavelength_m) {
        return false;
    }
    for amplitude in [
        relief.abyssal_m,
        relief.shelf_m,
        relief.coast_m,
        relief.interior_m,
        relief.mountain_m,
    ] {
        if !within(amplitude, 0.0, WB_MAX_RELIEF_AMPLITUDE_M) {
            return false;
        }
    }
    if !within(relief.quieting_strength, -WB_MAX_QUIETING_STRENGTH, WB_MAX_QUIETING_STRENGTH) {
        return false;
    }
    if !within(relief.quieting_scale_m, WB_MIN_QUIETING_SCALE_M, WB_MAX_QUIETING_SCALE_M) {
        return false;
    }
    // Both ends are admitted on purpose and both are swept: at `0.0` every octave past the
    // first carries no share, at `1.0` every octave carries the same one. Neither is a
    // division and neither is a panic; `plan`'s own `if sum == 0.0 { 1.0 }` guard is what
    // makes the zero end safe, and it predates this task.
    if !within(relief.octave_persistence, 0.0, 1.0) {
        return false;
    }
    true
}

/// One relief record, decoded and validated, or `None` if this channel refuses it.
fn decode_relief(record: &[f64]) -> Option<ReliefParams> {
    let fields = <[f64; WB_RELIEF_STRIDE]>::try_from(record).ok()?;
    let relief = ReliefParams {
        canonical_wavelength_m: fields[0],
        coarsest_wavelength_m: fields[1],
        abyssal_m: fields[2],
        shelf_m: fields[3],
        coast_m: fields[4],
        interior_m: fields[5],
        mountain_m: fields[6],
        quieting_strength: fields[7],
        quieting_scale_m: fields[8],
        octave_persistence: fields[9],
    };
    if relief_is_admissible(&relief) {
        Some(relief)
    } else {
        None
    }
}

/// The inverse of [`decode_relief`]'s field order, in one place so the two cannot drift.
fn encode_relief(relief: &ReliefParams) -> [f64; WB_RELIEF_STRIDE] {
    [
        relief.canonical_wavelength_m,
        relief.coarsest_wavelength_m,
        relief.abyssal_m,
        relief.shelf_m,
        relief.coast_m,
        relief.interior_m,
        relief.mountain_m,
        relief.quieting_strength,
        relief.quieting_scale_m,
        relief.octave_persistence,
    ]
}

/// What a host's `(relief_ptr, relief_len)` pair means. Three outcomes, kept as a type so
/// the two exports that read one cannot confuse "canonical" with "refused" -- an
/// `Option<Option<..>>` would let them.
enum ReliefArg {
    /// A null pointer with a length of zero: the canonical path, `None`, byte-for-byte
    /// today's world. **This is what the viewer sends when nothing was touched** -- Ruling
    /// 1, held at the door rather than trusted to `canonical()` being equal to `None`.
    Canonical,
    /// A decoded, validated block.
    Chosen(ReliefParams),
    /// The buffer was unusable (null with a non-zero length, misaligned, or the wrong
    /// length) or a field was outside its documented domain.
    Refused(u32),
}

/// Read a relief argument out of linear memory.
///
/// # Safety
/// If `relief_len` is non-zero, `relief_ptr` must be a live, 8-aligned allocation of at
/// least `relief_len` f64.
unsafe fn read_relief(relief_ptr: *const f64, relief_len: u32) -> ReliefArg {
    if relief_len == 0 {
        // A null pointer is the canonical path. A non-null pointer with a length of zero is
        // a host that computed a length wrong, not a host asking for canonical, so it is
        // refused rather than silently answered with a different world.
        return if relief_ptr.is_null() {
            ReliefArg::Canonical
        } else {
            ReliefArg::Refused(WB_ERR_BUFFER)
        };
    }
    if relief_ptr.is_null() {
        return ReliefArg::Refused(WB_ERR_BUFFER);
    }
    let address = relief_ptr as usize; // cast-ok: a pointer to an integer for an alignment check, no float anywhere near it
    if address % core::mem::align_of::<f64>() != 0 {
        return ReliefArg::Refused(WB_ERR_BUFFER);
    }
    let words = match usize::try_from(relief_len) {
        Ok(words) if words == WB_RELIEF_STRIDE => words,
        _ => return ReliefArg::Refused(WB_ERR_BUFFER),
    };
    let record = core::slice::from_raw_parts(relief_ptr, words);
    match decode_relief(record) {
        Some(relief) => ReliefArg::Chosen(relief),
        None => ReliefArg::Refused(WB_ERR_PARAM),
    }
}

/// The preset a selector names, or `None` for a selector this build does not know.
///
/// **The only place a preset's values are read**, and there is no second copy of them
/// anywhere -- not in this file, not in the viewer. `ReliefParams::hills()` in `detail.rs`
/// carries the numbers and the measurement that chose them; this hands them across the
/// boundary unchanged so a host can present a preset button without restating `600.0`,
/// `-0.7` and `0.65` as literals that drift the first time anybody retunes one.
fn preset_by_selector(preset: u32) -> Option<ReliefParams> {
    if preset == WB_RELIEF_CANONICAL {
        Some(ReliefParams::canonical())
    } else if preset == WB_RELIEF_HILLS {
        Some(ReliefParams::hills())
    } else {
        None
    }
}

// ------------------------------------------------------ the tectonic channel, decoded

/// Whether a tectonic block is one this boundary will let reach `Surface::new`.
///
/// Every bound is documented on its own constant, with which ones close a real hazard (the
/// four width ceilings close the **range-gate truncation** `MAX_TECTONIC_RANGE_M`'s own doc
/// names, and `WB_MIN_TECTONIC_WIDTH_M` closes a field that is accepted and does nothing) and
/// which are domain statements. **Nothing here clamps**: a record is admitted as the host
/// wrote it or refused entire, because a silently-adjusted parameter is a world nobody asked
/// for -- and a caller sweeping this channel needs a refusal to mean refusal, not a quiet
/// substitution.
fn tectonic_is_admissible(tectonics: &TectonicParams) -> bool {
    for amplitude in [
        tectonics.continent_collision_m,
        tectonics.coastal_uplift_m,
        tectonics.island_arc_m,
        tectonics.ridge_m,
    ] {
        if !within(amplitude, -WB_MAX_TECTONIC_AMPLITUDE_M, WB_MAX_TECTONIC_AMPLITUDE_M) {
            return false;
        }
    }
    // Each width against its own ceiling, because each profile sits at its own offset from
    // the margin and therefore reaches a different distance for the same width. Writing one
    // shared ceiling here would admit a coastal profile that the range gate then cuts off
    // mid-fade -- the cliff, arrived at by tidiness.
    for (width, ceiling) in [
        (tectonics.continent_collision_width_m, WB_MAX_CENTRED_TECTONIC_WIDTH_M),
        (tectonics.coastal_uplift_width_m, WB_MAX_COASTAL_UPLIFT_WIDTH_M),
        (tectonics.island_arc_width_m, WB_MAX_ISLAND_ARC_WIDTH_M),
        (tectonics.ridge_width_m, WB_MAX_CENTRED_TECTONIC_WIDTH_M),
    ] {
        if !within(width, WB_MIN_TECTONIC_WIDTH_M, ceiling) {
            return false;
        }
    }
    if !within(tectonics.continental_blend, WB_MIN_CONTINENTAL_BLEND, WB_MAX_CONTINENTAL_BLEND) {
        return false;
    }

    // ------------------------------------------------- the five structure fields, Task 3
    //
    // `suture_count` is already a `u32` by the time it arrives here -- `decode_tectonic`
    // refuses a word that is not a finite integer in range before it can become one -- so the
    // ceiling below is a second statement of the same bound rather than the only one. It is
    // stated twice on purpose: a caller reaching `tectonic_is_admissible` through
    // `TectonicParams` it built itself (every test in this crate) gets the same answer as one
    // reaching it through the ABI, and the loop bound is not a thing to be right about once.
    if tectonics.suture_count < 1 || tectonics.suture_count > WB_MAX_SUTURE_COUNT {
        return false;
    }
    if !within(tectonics.collision_asymmetry, WB_MIN_COLLISION_ASYMMETRY, f64::INFINITY) {
        return false;
    }
    // The overriding flank, which is `continent_collision_width_m / collision_asymmetry`, held
    // against the SAME floor the width itself faces. **This is the asymmetry's upper bound and
    // it is derived rather than chosen**: a flank narrower than `WB_MIN_TECTONIC_WIDTH_M` is
    // the silence that floor exists to refuse, and an infinite asymmetry produces exactly zero
    // there. Nothing above needs to be picked, and nothing here divides by zero -- the floor
    // above has already refused every asymmetry below 1.0.
    let overriding_flank_m = tectonics.continent_collision_width_m / tectonics.collision_asymmetry;
    if !within(overriding_flank_m, WB_MIN_TECTONIC_WIDTH_M, WB_MAX_CENTRED_TECTONIC_WIDTH_M) {
        return false;
    }
    if !within(tectonics.suture_spread_m, WB_MIN_SUTURE_SPREAD_M, MAX_TECTONIC_RANGE_M) {
        return false;
    }
    // **A count above one at a spread of zero is a HEIGHT knob wearing a count's name.**
    // `sutures` places suture `i` at `i * suture_spread_m * jitter`, so at a spread of exactly
    // zero every one of them sits at offset zero and the profile is the amplitude multiplied
    // by the sum of the weights -- somewhere between 1x and 8x `continent_collision_m`,
    // decided by a hash of the plate pair. That is the same arithmetic Task 2 measured as a
    // 64% overshoot at 4 x 60 km, taken to its limit, and it would make the height slider read
    // a different number on every margin. Refused rather than adjusted.
    if tectonics.suture_count > 1 && !(tectonics.suture_spread_m > WB_MIN_SUTURE_SPREAD_M) {
        return false;
    }
    if !within(tectonics.structure_depth, 0.0, WB_MAX_STRUCTURE_DEPTH) {
        return false;
    }
    if !within(
        tectonics.structure_wavelength_m,
        WB_MIN_STRUCTURE_WAVELENGTH_M,
        WB_MAX_STRUCTURE_WAVELENGTH_M,
    ) {
        return false;
    }
    if !within(tectonics.margin_warp_m, WB_MIN_MARGIN_WARP_M, WB_MAX_MARGIN_WARP_M) {
        return false;
    }
    if !within(
        tectonics.margin_warp_wavelength_m,
        WB_MIN_MARGIN_WARP_WAVELENGTH_M,
        WB_MAX_MARGIN_WARP_WAVELENGTH_M,
    ) {
        return false;
    }
    // **THE RANGE GATE, asked of the whole profile rather than of one field.** Task 2 added
    // `collision_reach_m` for exactly this call site and said so: past this distance
    // `offset_m` does not evaluate the margin at all, so a profile still carrying weight there
    // is truncated rather than faded -- a cliff, measured by Task 2's own survey at a 41.3%
    // grade and 827 m of relief over 2 km on `4 sutures x 150 km`.
    //
    // The per-field width ceiling above cannot see this: it is `continent_collision_width_m`
    // alone, and two sutures 150 km apart carry a 100 km profile out to 302 km. This check
    // subsumes it at canonical structure settings, where the reach IS the width, and the width
    // check is kept anyway because it is the one that stays true if `collision_reach_m`'s
    // definition ever changes.
    //
    // **Task 5 put `margin_warp_m` into that sum, and that is the whole of its safety case.**
    // The warp translates the collision profile sideways off the bisector, so on the side it
    // pushes toward the profile carries weight exactly that much further out -- and this is
    // the only check that can see it. `WB_MAX_MARGIN_WARP_M` alone would admit the preset's
    // 80 km warp on top of two sutures 150 km apart, whose reach is 302 km before the warp
    // and 382 km after it, and 150 km of warp on the same pair reaches 452 km: past the gate,
    // and truncated rather than faded.
    if !within(tectonics.collision_reach_m(), 0.0, MAX_TECTONIC_RANGE_M) {
        return false;
    }
    true
}

/// One tectonic record, decoded and validated, or `None` if this channel refuses it.
fn decode_tectonic(record: &[f64]) -> Option<TectonicParams> {
    let fields = <[f64; WB_TECTONIC_STRIDE]>::try_from(record).ok()?;
    let tectonics = TectonicParams {
        continent_collision_m: fields[0],
        continent_collision_width_m: fields[1],
        coastal_uplift_m: fields[2],
        coastal_uplift_width_m: fields[3],
        island_arc_m: fields[4],
        island_arc_width_m: fields[5],
        ridge_m: fields[6],
        ridge_width_m: fields[7],
        continental_blend: fields[8],
        collision_asymmetry: fields[9],
        // **The one field on this channel that is not an f64, and the only place in this file
        // a word is narrowed rather than carried.** `suture_count` is a `u32` loop bound; the
        // ABI is a flat f64 record; so the word has to be a finite integer inside the ceiling
        // *before* it can become a count at all. `decode_suture_count` refuses everything
        // else, and refusing is the only safe move: `as u32` on an f64 SATURATES in Rust, so
        // 1e300 would arrive here as `u32::MAX` and every convergent sample would then walk
        // four billion iterations -- the hang `WB_MAX_SUTURE_COUNT` exists for, delivered by
        // the cast rather than by the caller.
        suture_count: decode_suture_count(fields[10])?,
        suture_spread_m: fields[11],
        structure_depth: fields[12],
        structure_wavelength_m: fields[13],
        margin_warp_m: fields[14],
        margin_warp_wavelength_m: fields[15],
    };
    if tectonic_is_admissible(&tectonics) {
        Some(tectonics)
    } else {
        None
    }
}

/// Word 10 of a tectonic record as a suture count, or `None` if it is not one.
///
/// Finite, exactly integral, and inside `1..=WB_MAX_SUTURE_COUNT` **before** the cast, which
/// is what makes the cast total rather than saturating. `value as u32` on an f64 saturates at
/// both ends and truncates the fraction, so every one of those three checks is load-bearing
/// and none of them is a restatement of another:
///
/// - **not finite** -- `f64::NAN as u32` is 0 and `f64::INFINITY as u32` is `u32::MAX`, so a
///   NaN would silently become a refused zero and an infinity a four-billion-iteration loop;
/// - **not integral** -- 2.5 would truncate to 2, which is a silently-adjusted parameter and
///   this boundary does not adjust;
/// - **outside the range** -- 1e300 saturates to `u32::MAX`, the hang itself.
///
/// `trunc` is not a transcendental and is not on `detmath`'s list; the comparison is written
/// against the value's own truncation so no rounding mode is involved.
fn decode_suture_count(value: f64) -> Option<u32> {
    if !value.is_finite() || value != value.trunc() {
        return None;
    }
    if !within(value, 1.0, f64::from(WB_MAX_SUTURE_COUNT)) {
        return None;
    }
    Some(value as u32) // cast-ok: proved finite, integral and inside 1..=WB_MAX_SUTURE_COUNT on the three lines above
}

/// The inverse of [`decode_tectonic`]'s field order, in one place so the two cannot drift.
fn encode_tectonic(tectonics: &TectonicParams) -> [f64; WB_TECTONIC_STRIDE] {
    [
        tectonics.continent_collision_m,
        tectonics.continent_collision_width_m,
        tectonics.coastal_uplift_m,
        tectonics.coastal_uplift_width_m,
        tectonics.island_arc_m,
        tectonics.island_arc_width_m,
        tectonics.ridge_m,
        tectonics.ridge_width_m,
        tectonics.continental_blend,
        tectonics.collision_asymmetry,
        // The inverse of `decode_suture_count`. A `u32` up to `WB_MAX_SUTURE_COUNT` is exactly
        // representable as an f64 with room to spare, so this round-trips by construction and
        // `f64::from` cannot be the lossy direction.
        f64::from(tectonics.suture_count),
        tectonics.suture_spread_m,
        tectonics.structure_depth,
        tectonics.structure_wavelength_m,
        tectonics.margin_warp_m,
        tectonics.margin_warp_wavelength_m,
    ]
}

/// What a host's `(tectonic_ptr, tectonic_len)` pair means. The same three outcomes
/// [`ReliefArg`] draws, kept as a separate type rather than made generic because the two
/// strides differ and a shared one would have to carry the length as data.
enum TectonicArg {
    /// A null pointer with a length of zero: the canonical path, `None`, byte-for-byte
    /// today's world. **This is what the viewer sends when nothing was touched** -- Ruling 1,
    /// held at the door rather than trusted to `canonical()` being equal to `None`.
    Canonical,
    /// A decoded, validated block.
    Chosen(TectonicParams),
    /// The buffer was unusable, or a field was outside its documented domain.
    Refused(u32),
}

/// Read a tectonic argument out of linear memory.
///
/// # Safety
/// If `tectonic_len` is non-zero, `tectonic_ptr` must be a live, 8-aligned allocation of at
/// least `tectonic_len` f64.
unsafe fn read_tectonic(tectonic_ptr: *const f64, tectonic_len: u32) -> TectonicArg {
    if tectonic_len == 0 {
        // A null pointer is the canonical path. A non-null pointer with a length of zero is a
        // host that computed a length wrong, not a host asking for canonical.
        return if tectonic_ptr.is_null() {
            TectonicArg::Canonical
        } else {
            TectonicArg::Refused(WB_ERR_BUFFER)
        };
    }
    if tectonic_ptr.is_null() {
        return TectonicArg::Refused(WB_ERR_BUFFER);
    }
    let address = tectonic_ptr as usize; // cast-ok: a pointer to an integer for an alignment check, no float anywhere near it
    if address % core::mem::align_of::<f64>() != 0 {
        return TectonicArg::Refused(WB_ERR_BUFFER);
    }
    let words = match usize::try_from(tectonic_len) {
        Ok(words) if words == WB_TECTONIC_STRIDE => words,
        _ => return TectonicArg::Refused(WB_ERR_BUFFER),
    };
    let record = core::slice::from_raw_parts(tectonic_ptr, words);
    match decode_tectonic(record) {
        Some(tectonics) => TectonicArg::Chosen(tectonics),
        None => TectonicArg::Refused(WB_ERR_PARAM),
    }
}

/// The tectonic preset a selector names, or `None` for one this build does not know.
///
/// **The only place `canonical()`'s and `ranges()`'s values are read**, and there is no second
/// copy of either anywhere -- not in this file, not in the viewer. The panel's slider anchors
/// and its preset button are both this function's answer, so `tectonics.rs` stays the only
/// place the numbers live. That is Ruling 7 of the relief slice, and `relief_preset_by_selector`
/// above is the same three lines for the same reason.
fn tectonic_preset_by_selector(preset: u32) -> Option<TectonicParams> {
    if preset == WB_TECTONIC_CANONICAL {
        Some(TectonicParams::canonical())
    } else if preset == WB_TECTONIC_RANGES {
        Some(TectonicParams::ranges())
    } else {
        None
    }
}

// --------------------------------------------------------- the coastal channel, decoded

/// Whether a coast block is one this boundary will let reach `Surface::with_coast`.
///
/// Every bound is documented on its own constant, with which ones close a real hazard
/// ([`WB_MAX_COAST_OCTAVES`] closes a **hang** and [`WB_MAX_COAST_FINEST_FREQUENCY`] closes an
/// **abort** no per-field ceiling can see) and which are domain statements. **Nothing here
/// clamps**: a record is admitted as the host wrote it or refused entire, because a
/// silently-adjusted parameter is a world nobody asked for -- and a caller sweeping this channel
/// needs a refusal to mean refusal, not a quiet substitution.
///
/// **Validation is where clamping is most tempting and there is none of it here.** No
/// `f64::min`, no `f64::max`, no `.clamp` -- all three are NaN-asymmetric and this repository's
/// own guard does not catch them. Every comparison below is written so that a NaN fails it:
/// `within` is two `>=`/`<=` tests, both false for NaN, and the octave count is refused for
/// non-finiteness before it is anything else.
fn coast_is_admissible(coast: &CoastParams) -> bool {
    if !within(coast.amplitude, WB_MIN_COAST_AMPLITUDE, WB_MAX_COAST_AMPLITUDE) {
        return false;
    }
    if !within(
        coast.window_spreads,
        WB_MIN_COAST_WINDOW_SPREADS,
        WB_MAX_COAST_WINDOW_SPREADS,
    ) {
        return false;
    }
    if !within(coast.frequency, WB_MIN_COAST_FREQUENCY, WB_MAX_COAST_FINEST_FREQUENCY) {
        return false;
    }
    // `octaves` is already a `u32` by the time it arrives here -- `decode_coast` refuses a word
    // that is not a finite integer in range before it can become one -- so this is a second
    // statement of the same bound rather than the only one. It is stated twice on purpose, the
    // same way `suture_count`'s is: a caller reaching this function through a `CoastParams` it
    // built in Rust gets the same answer as one reaching it through the ABI, and **a loop bound
    // is not a thing to be right about once**.
    if coast.octaves < 1 || coast.octaves > WB_MAX_COAST_OCTAVES {
        return false;
    }
    if !within(coast.gain, 0.0, WB_MAX_COAST_GAIN) {
        return false;
    }
    if !within(coast.lacunarity, WB_MIN_COAST_LACUNARITY, WB_MAX_COAST_LACUNARITY) {
        return false;
    }
    // **THE PRODUCT CHECK, and it is the binding one.** Three fields above are each inside their
    // own domain at values whose product is not, and the product is what `Noise::fbm` actually
    // walks -- see [`WB_MAX_COAST_FINEST_FREQUENCY`] for the `i64` saturation and the `ix + 1`
    // overflow it ends in.
    //
    // Walked rather than raised to a power: the schedule is multiplied forward exactly as `fbm`
    // multiplies it, so this cannot disagree with the loop it is protecting, and there is no
    // transcendental involved at all (`powf` would be one, and `detmath` is the only door for
    // those). The loop is bounded by an octave count that has already been refused above this
    // line if it is out of range, so this is not a second unbounded loop.
    let mut finest = coast.frequency;
    for _ in 1..coast.octaves {
        finest *= coast.lacunarity;
        if !finest.is_finite() {
            return false;
        }
    }
    if !within(finest, WB_MIN_COAST_FREQUENCY, WB_MAX_COAST_FINEST_FREQUENCY) {
        return false;
    }
    true
}

/// Word 3 of a coast record as an octave count, or `None` if it is not one.
///
/// The exact shape of `decode_suture_count`, and for the exact reason its doc gives: finite,
/// exactly integral and inside `1..=WB_MAX_COAST_OCTAVES` **before** the cast, which is what
/// makes the cast total rather than saturating.
///
/// - **not finite** -- `f64::NAN as u32` is 0 and `f64::INFINITY as u32` is `u32::MAX`, so a NaN
///   would silently become a refused zero and an infinity a four-billion-iteration loop **per
///   sample**;
/// - **not integral** -- 2.5 would truncate to 2, a silently-adjusted parameter, and this
///   boundary does not adjust;
/// - **outside the range** -- 1e300 saturates to `u32::MAX`, which is the hang itself.
///
/// Zero is refused by the range rather than admitted: `fbm` with zero octaves returns exactly
/// 0.0 (`loudest == 0.0`), so a zero-octave record is a coastal term present in the record,
/// accepted, and contributing nothing anywhere -- the silently-dropping-builder shape, with
/// `amplitude` sitting beside it looking configured.
///
/// `trunc` is not a transcendental and is not on `detmath`'s list; the comparison is written
/// against the value's own truncation so no rounding mode is involved.
fn decode_coast_octaves(value: f64) -> Option<u32> {
    if !value.is_finite() || value != value.trunc() {
        return None;
    }
    if !within(value, 1.0, f64::from(WB_MAX_COAST_OCTAVES)) {
        return None;
    }
    Some(value as u32) // cast-ok: proved finite, integral and inside 1..=WB_MAX_COAST_OCTAVES on the three lines above
}

/// One coast record, decoded and validated, or `None` if this channel refuses it.
fn decode_coast(record: &[f64]) -> Option<CoastParams> {
    let fields = <[f64; WB_COAST_STRIDE]>::try_from(record).ok()?;
    let coast = CoastParams {
        amplitude: fields[0],
        window_spreads: fields[1],
        frequency: fields[2],
        // The one field on this channel that is not an f64. See `decode_coast_octaves`.
        octaves: decode_coast_octaves(fields[3])?,
        gain: fields[4],
        lacunarity: fields[5],
    };
    if coast_is_admissible(&coast) {
        Some(coast)
    } else {
        None
    }
}

/// The inverse of [`decode_coast`]'s field order, in one place so the two cannot drift.
fn encode_coast(coast: &CoastParams) -> [f64; WB_COAST_STRIDE] {
    [
        coast.amplitude,
        coast.window_spreads,
        coast.frequency,
        // The inverse of `decode_coast_octaves`. A `u32` up to `WB_MAX_COAST_OCTAVES` is exactly
        // representable as an f64 with room to spare, so this round-trips by construction and
        // `f64::from` cannot be the lossy direction.
        f64::from(coast.octaves),
        coast.gain,
        coast.lacunarity,
    ]
}

/// What a host's `(coast_ptr, coast_len)` pair means. The same three outcomes [`ReliefArg`] and
/// [`TectonicArg`] draw, kept as a separate type for the same reason they are separate from each
/// other: the strides differ and a shared one would have to carry the length as data.
enum CoastArg {
    /// A null pointer with a length of zero: the canonical path, `None`, byte-for-byte today's
    /// coastline. **This is what the viewer sends when nothing was touched** -- RULING 1, held
    /// at the door rather than trusted to `canonical()` being equal to `None`.
    Canonical,
    /// A decoded, validated block.
    Chosen(CoastParams),
    /// The buffer was unusable, or a field was outside its documented domain.
    Refused(u32),
}

/// Read a coast argument out of linear memory.
///
/// # Safety
/// If `coast_len` is non-zero, `coast_ptr` must be a live, 8-aligned allocation of at least
/// `coast_len` f64.
unsafe fn read_coast(coast_ptr: *const f64, coast_len: u32) -> CoastArg {
    if coast_len == 0 {
        // A null pointer is the canonical path. A non-null pointer with a length of zero is a
        // host that computed a length wrong, not a host asking for canonical.
        return if coast_ptr.is_null() { CoastArg::Canonical } else { CoastArg::Refused(WB_ERR_BUFFER) };
    }
    if coast_ptr.is_null() {
        return CoastArg::Refused(WB_ERR_BUFFER);
    }
    let address = coast_ptr as usize; // cast-ok: a pointer to an integer for an alignment check, no float anywhere near it
    if address % core::mem::align_of::<f64>() != 0 {
        return CoastArg::Refused(WB_ERR_BUFFER);
    }
    let words = match usize::try_from(coast_len) {
        Ok(words) if words == WB_COAST_STRIDE => words,
        _ => return CoastArg::Refused(WB_ERR_BUFFER),
    };
    let record = core::slice::from_raw_parts(coast_ptr, words);
    match decode_coast(record) {
        Some(coast) => CoastArg::Chosen(coast),
        None => CoastArg::Refused(WB_ERR_PARAM),
    }
}

/// The coast preset a selector names, or `None` for one this build does not know.
///
/// **The only place `CoastParams::canonical()`'s and `fractal()`'s values are read**, and there
/// is no second copy of either anywhere -- not in this file, not in the viewer. The panel's
/// slider anchor and its preset button are both this function's answer, so `continentality.rs`
/// stays the only place the numbers live. Ruling 7 of the relief slice, for the third channel.
fn coast_preset_by_selector(preset: u32) -> Option<CoastParams> {
    if preset == WB_COAST_CANONICAL {
        Some(CoastParams::canonical())
    } else if preset == WB_COAST_FRACTAL {
        Some(CoastParams::fractal())
    } else {
        None
    }
}


// ------------------------------------------------------- the gully channel, decoded

/// Whether a gully block is one this boundary will let reach `Surface::with_gully`.
///
/// Every bound is documented on its own constant, and the one that closes a hazard rather
/// than stating a domain is [`WB_MIN_GULLY_SHARPNESS`] -- a non-positive exponent turns every
/// crest into an infinite height. **Nothing here clamps**: a record is admitted as the host
/// wrote it or refused entire.
fn gully_is_admissible(gully: &GullyParams) -> bool {
    fn within(value: f64, low: f64, high: f64) -> bool {
        // Negated comparisons so a NaN leaves by the refusing door.
        value.is_finite() && value >= low && value <= high
    }
    if !within(gully.amplitude_m, 0.0, WB_MAX_GULLY_AMPLITUDE_M) {
        return false;
    }
    for length in [gully.cell_m, gully.steer_lattice_m] {
        if !within(length, WB_MIN_GULLY_LENGTH_M, WB_MAX_GULLY_LENGTH_M) {
            return false;
        }
    }
    if !within(gully.slope_reference, WB_MIN_GULLY_SLOPE_REFERENCE, WB_MAX_GULLY_SLOPE_REFERENCE) {
        return false;
    }
    for stripes in [gully.stripes_per_cell, gully.max_stripes_per_cell] {
        if !within(stripes, 0.0, WB_MAX_GULLY_STRIPES) {
            return false;
        }
    }
    if !within(gully.crest_sharpness, WB_MIN_GULLY_SHARPNESS, WB_MAX_GULLY_SHARPNESS) {
        return false;
    }
    if !within(
        gully.gate_elevation_m,
        -WB_MAX_GULLY_GATE_ELEVATION_M,
        WB_MAX_GULLY_GATE_ELEVATION_M,
    ) {
        return false;
    }
    if !within(gully.harmonic_weight, 0.0, WB_MAX_GULLY_HARMONIC_WEIGHT) {
        return false;
    }
    if !within(
        gully.harmonic_band_m,
        WB_MIN_GULLY_HARMONIC_BAND_M,
        WB_MAX_GULLY_HARMONIC_BAND_M,
    ) {
        return false;
    }
    if !within(gully.gate_elevation_span_m, WB_MIN_GULLY_GATE_SPAN_M, WB_MAX_GULLY_GATE_SPAN_M) {
        return false;
    }
    if !within(gully.flat_energy_floor, 0.0, 1.0) {
        return false;
    }
    true
}

/// One gully record, decoded and validated, or `None` if this channel refuses it.
fn decode_gully(record: &[f64]) -> Option<GullyParams> {
    let fields = <[f64; WB_GULLY_STRIDE]>::try_from(record).ok()?;
    let gully = GullyParams {
        amplitude_m: fields[0],
        cell_m: fields[1],
        slope_reference: fields[2],
        stripes_per_cell: fields[3],
        max_stripes_per_cell: fields[4],
        crest_sharpness: fields[5],
        gate_elevation_m: fields[6],
        gate_elevation_span_m: fields[7],
        flat_energy_floor: fields[8],
        steer_lattice_m: fields[9],
        harmonic_weight: fields[10],
        harmonic_band_m: fields[11],
    };
    if gully_is_admissible(&gully) {
        Some(gully)
    } else {
        None
    }
}

/// The inverse of [`decode_gully`]'s field order, in one place so the two cannot drift.
fn encode_gully(gully: &GullyParams) -> [f64; WB_GULLY_STRIDE] {
    [
        gully.amplitude_m,
        gully.cell_m,
        gully.slope_reference,
        gully.stripes_per_cell,
        gully.max_stripes_per_cell,
        gully.crest_sharpness,
        gully.gate_elevation_m,
        gully.gate_elevation_span_m,
        gully.flat_energy_floor,
        gully.steer_lattice_m,
        gully.harmonic_weight,
        gully.harmonic_band_m,
    ]
}

/// What a host's `(gully_ptr, gully_len)` pair means. Three outcomes, kept as a type for the
/// same reason [`ReliefArg`] is.
enum GullyArg {
    /// A null pointer with a length of zero: the canonical path, `None`, byte-for-byte
    /// today's world -- **and here that means no steering lattice is built at all**, which is
    /// what makes Ruling 1 hold structurally rather than by an addition of zero. See
    /// `Surface`'s `steer` field.
    Canonical,
    Chosen(GullyParams),
    Refused(u32),
}

/// Read a gully argument out of linear memory.
///
/// # Safety
/// If `gully_len` is non-zero, `gully_ptr` must be a live, 8-aligned allocation of at least
/// `gully_len` f64.
unsafe fn read_gully(gully_ptr: *const f64, gully_len: u32) -> GullyArg {
    if gully_len == 0 {
        return if gully_ptr.is_null() {
            GullyArg::Canonical
        } else {
            GullyArg::Refused(WB_ERR_BUFFER)
        };
    }
    if gully_ptr.is_null() {
        return GullyArg::Refused(WB_ERR_BUFFER);
    }
    let address = gully_ptr as usize; // cast-ok: a pointer to an integer for an alignment check, no float anywhere near it
    if address % core::mem::align_of::<f64>() != 0 {
        return GullyArg::Refused(WB_ERR_BUFFER);
    }
    let words = match usize::try_from(gully_len) {
        Ok(words) if words == WB_GULLY_STRIDE => words,
        _ => return GullyArg::Refused(WB_ERR_BUFFER),
    };
    let record = core::slice::from_raw_parts(gully_ptr, words);
    match decode_gully(record) {
        Some(gully) => GullyArg::Chosen(gully),
        None => GullyArg::Refused(WB_ERR_PARAM),
    }
}

/// The gully preset a selector names, or `None` for one this build does not know.
///
/// **The only place `GullyParams::canonical()`'s and `drainage()`'s values are read.** Ruling
/// 7 of the relief slice, for the fifth channel: no host restates a measured constant.
fn gully_preset_by_selector(preset: u32) -> Option<GullyParams> {
    if preset == WB_GULLY_CANONICAL {
        Some(GullyParams::canonical())
    } else if preset == WB_GULLY_DRAINAGE {
        Some(GullyParams::drainage())
    } else {
        None
    }
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
/// - `radius_m` finite, strictly positive, and no larger than [`WB_MAX_WORLD_RADIUS_M`]. A
///   NaN radius produces NaN elevations at every point -- plausible-looking garbage,
///   measured -- rather than failing. The upper bound is new: the whole-branch review of
///   slice 5a found that an enormous-but-finite `radius_m` reaches two different overflow
///   hazards downstream (an `i64` cast in `noise.rs`'s lattice arithmetic, and `+inf` in
///   `stream::node_areas_m2`'s `4*pi*r^2`) that this domain check now closes at the door
///   both are reached through -- see [`WB_MAX_WORLD_RADIUS_M`]'s own doc for both hazards
///   and the margin chosen against them.
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
    // `None` -- canonical roughness, unchanged by the relief channel Task 4 added beside
    // this export. This signature is frozen: extending it in place would have broken every
    // existing caller's arity for a parameter most of them never want, so
    // `wb_world_new_relief` is a second door onto the same builder rather than a wider one
    // onto this.
    unsafe {
        build_world(
            world_seed,
            radius_m,
            plate_count,
            land_fraction,
            features_ptr,
            feature_count,
            None,
            None,
            // `None` -- today's coastline, byte-for-byte, unchanged by the coast channel Task
            // 6 added beside this export as a fourth door.
            None,
            // `None` -- no drainage texture, exactly what this export did before the gully
            // channel existed. Its arity is frozen for the same reason its coast argument is.
            None,
        )
    }
}

/// Build a world with a caller-chosen relief block, or **0** if it refused.
///
/// Exactly [`wb_world_new`] plus a relief record, and every one of that function's own
/// domains still applies unchanged -- read its doc for `radius_m`, `plate_count`,
/// `land_fraction` and the feature channel.
///
/// # The relief argument
///
/// - **`relief_ptr` null with `relief_len == 0` is the canonical path** and is exactly what
///   `wb_world_new` does: `None`, not `Some(canonical())`. Bit-identical to today's world,
///   which Ruling 1 requires and `the_relief_channel_default_path_is_the_untouched_world`
///   proves by sampling rather than by argument.
/// - Otherwise `relief_len` must be exactly [`WB_RELIEF_STRIDE`] and `relief_ptr` a live,
///   8-aligned buffer of that many f64 in the order that constant documents. Every field is
///   bounded; **a single field outside its domain refuses the whole call**, the same way one
///   bad feature record does, because a world built from nine of ten requested parameters is
///   the silently-dropping-builder shape.
///
/// A host that wants to know *why* a record was refused calls [`wb_relief_check`] on the
/// same buffer, which answers with a status instead of a handle.
///
/// # Safety
/// The feature-channel safety requirement of [`wb_world_new`] applies unchanged. If
/// `relief_len` is non-zero, `relief_ptr` must be a live, 8-aligned allocation of at least
/// `relief_len` f64.
#[no_mangle]
pub extern "C" fn wb_world_new_relief(
    world_seed: i64,
    radius_m: f64,
    plate_count: u32,
    land_fraction: f64,
    features_ptr: *const f64,
    feature_count: u32,
    relief_ptr: *const f64,
    relief_len: u32,
) -> u32 {
    let relief = match unsafe { read_relief(relief_ptr, relief_len) } {
        ReliefArg::Canonical => None,
        ReliefArg::Chosen(relief) => Some(relief),
        ReliefArg::Refused(_) => return 0,
    };
    // `None` -- canonical uplift, exactly what this export did before the tectonic channel
    // existed. Its arity is frozen for the same reason `wb_world_new`'s was.
    unsafe {
        build_world(
            world_seed,
            radius_m,
            plate_count,
            land_fraction,
            features_ptr,
            feature_count,
            relief,
            None,
            // `None` -- today's coastline. Its arity is frozen for the reason above.
            None,
            // `None` -- no drainage texture, exactly what this export did before the gully
            // channel existed. Its arity is frozen for the same reason its coast argument is.
            None,
        )
    }
}

/// Build a world with a caller-chosen relief block **and** a caller-chosen tectonic block, or
/// **0** if it refused.
///
/// Exactly [`wb_world_new_relief`] plus a tectonic record, and every one of that function's
/// domains -- and `wb_world_new`'s before it -- still applies unchanged.
///
/// # Why a third door rather than a wider second one
///
/// `wb_world_new_relief` already ships in a committed `.wasm` that the parity harness
/// compares against; widening its arity would break every existing caller for a parameter
/// most of them never want. This is the same reasoning `wb_world_new_relief` itself records
/// for not widening `wb_world_new`, and all three doors are one `build_world` behind the
/// boundary, so there is one `Surface::new` call in this file and not three.
///
/// # The tectonic argument
///
/// - **`tectonic_ptr` null with `tectonic_len == 0` is the canonical path** -- `None`, not
///   `Some(canonical())`. Ruling 1 of this slice: the default cannot move, and the viewer's
///   untouched path must reach the engine as `None`. Held at the door rather than trusted to
///   `canonical()` agreeing with `None`, because Ruling 1 of the slice ledger records that a
///   bit-identity test between those two arms **cannot** prove the params are read: both
///   resolve through `unwrap_or_else(TectonicParams::canonical)` and agree no matter what the
///   uplift path ignores. What proves it is `tectonics.rs`'s one-ULP perturbation fixtures.
/// - Otherwise `tectonic_len` must be exactly [`WB_TECTONIC_STRIDE`] and `tectonic_ptr` a
///   live, 8-aligned buffer of that many f64 in the order that constant documents. Every
///   field is bounded, and **a single field outside its domain refuses the whole call.**
///
/// A host that wants to know *why* a record was refused calls [`wb_tectonic_check`] on the
/// same buffer.
///
/// # Safety
/// The feature-channel and relief-channel safety requirements of [`wb_world_new_relief`]
/// apply unchanged. If `tectonic_len` is non-zero, `tectonic_ptr` must be a live, 8-aligned
/// allocation of at least `tectonic_len` f64.
#[no_mangle]
pub extern "C" fn wb_world_new_tectonic(
    world_seed: i64,
    radius_m: f64,
    plate_count: u32,
    land_fraction: f64,
    features_ptr: *const f64,
    feature_count: u32,
    relief_ptr: *const f64,
    relief_len: u32,
    tectonic_ptr: *const f64,
    tectonic_len: u32,
) -> u32 {
    let relief = match unsafe { read_relief(relief_ptr, relief_len) } {
        ReliefArg::Canonical => None,
        ReliefArg::Chosen(relief) => Some(relief),
        ReliefArg::Refused(_) => return 0,
    };
    let tectonics = match unsafe { read_tectonic(tectonic_ptr, tectonic_len) } {
        TectonicArg::Canonical => None,
        TectonicArg::Chosen(tectonics) => Some(tectonics),
        TectonicArg::Refused(_) => return 0,
    };
    unsafe {
        build_world(
            world_seed,
            radius_m,
            plate_count,
            land_fraction,
            features_ptr,
            feature_count,
            relief,
            tectonics,
            // `None` -- today's coastline, exactly what this export did before the coast
            // channel existed. Its arity is frozen for the same reason the two above it are.
            None,
            // `None` -- no drainage texture, exactly what this export did before the gully
            // channel existed. Its arity is frozen for the same reason its coast argument is.
            None,
        )
    }
}


/// Build a world with a caller-chosen relief block, a caller-chosen tectonic block **and** a
/// caller-chosen coast block, or **0** if it refused.
///
/// Exactly [`wb_world_new_tectonic`] plus a coast record, and every one of that function's
/// domains -- and `wb_world_new_relief`'s and `wb_world_new`'s before it -- still applies
/// unchanged.
///
/// # Why a fourth door rather than a wider third one
///
/// The same reason the third gives for not widening the second: `wb_world_new_tectonic` already
/// ships in a committed `.wasm` that the parity harness compares against, and widening its
/// arity would break every existing caller for a parameter most of them never want. All four
/// doors are one `build_world` behind the boundary, so there is one `Surface::with_coast` call
/// in this file and not four.
///
/// # The coast argument
///
/// - **`coast_ptr` null with `coast_len == 0` is the canonical path** -- `None`, not
///   `Some(canonical())`. RULING 1 of this slice: the default cannot move, and the viewer's
///   untouched path must reach the engine as `None`. Held at the door rather than trusted to
///   `canonical()` agreeing with `None` -- though here, unlike the tectonic channel, the two
///   *are* pinned bit-identical by `continentality.rs`'s and `surface.rs`'s own tests, because
///   `above_shore` early-returns on a zero amplitude rather than adding an exactly-zero offset.
/// - Otherwise `coast_len` must be exactly [`WB_COAST_STRIDE`] and `coast_ptr` a live,
///   8-aligned buffer of that many f64 in the order that constant documents. Every field is
///   bounded, and **a single field outside its domain refuses the whole call.**
///
/// **Two of those bounds are not politeness.** [`WB_MAX_COAST_OCTAVES`] is a per-sample loop
/// bound and an unbounded one is a hung tab, not a slow world; [`WB_MAX_COAST_FINEST_FREQUENCY`]
/// holds the *product* three admissible fields compound into, which ends in an `i64` overflow
/// inside `Noise::at` -- an abort across this boundary. Neither is visible to a per-field
/// ceiling alone.
///
/// A host that wants to know *why* a record was refused calls [`wb_coast_check`] on the same
/// buffer.
///
/// # Safety
/// The feature-, relief- and tectonic-channel safety requirements of [`wb_world_new_tectonic`]
/// apply unchanged. If `coast_len` is non-zero, `coast_ptr` must be a live, 8-aligned allocation
/// of at least `coast_len` f64.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn wb_world_new_coast(
    world_seed: i64,
    radius_m: f64,
    plate_count: u32,
    land_fraction: f64,
    features_ptr: *const f64,
    feature_count: u32,
    relief_ptr: *const f64,
    relief_len: u32,
    tectonic_ptr: *const f64,
    tectonic_len: u32,
    coast_ptr: *const f64,
    coast_len: u32,
) -> u32 {
    let relief = match unsafe { read_relief(relief_ptr, relief_len) } {
        ReliefArg::Canonical => None,
        ReliefArg::Chosen(relief) => Some(relief),
        ReliefArg::Refused(_) => return 0,
    };
    let tectonics = match unsafe { read_tectonic(tectonic_ptr, tectonic_len) } {
        TectonicArg::Canonical => None,
        TectonicArg::Chosen(tectonics) => Some(tectonics),
        TectonicArg::Refused(_) => return 0,
    };
    let coast = match unsafe { read_coast(coast_ptr, coast_len) } {
        CoastArg::Canonical => None,
        CoastArg::Chosen(coast) => Some(coast),
        CoastArg::Refused(_) => return 0,
    };
    unsafe {
        build_world(
            world_seed,
            radius_m,
            plate_count,
            land_fraction,
            features_ptr,
            feature_count,
            relief,
            tectonics,
            coast,
            // `None` -- no drainage texture, exactly what this export did before the gully
            // channel existed. Its arity is frozen for the same reason its coast argument is.
            None,
        )
    }
}

/// Write a named coast preset's six f64 into a caller buffer, in [`WB_COAST_STRIDE`]'s order.
///
/// `WB_OK`, or `WB_ERR_PARAM` for a selector this build does not know, or `WB_ERR_BUFFER` for a
/// null, misaligned, or wrongly-sized buffer. The selectors are [`WB_COAST_CANONICAL`] and
/// [`WB_COAST_FRACTAL`].
///
/// **This export exists so no host ever transcribes a coast default or a preset.** The panel's
/// amplitude slider is anchored on canonical and its preset button sends `fractal()` back, so
/// `continentality.rs` stays the only place `0.35`, `20.0`, `4`, `0.5` and `2.0` are written
/// down. The viewer holds none of them.
///
/// # Safety
/// `out_ptr` must be a live, 8-aligned allocation of at least `out_len` f64.
#[no_mangle]
pub extern "C" fn wb_coast_preset(preset: u32, out_ptr: *mut f64, out_len: u32) -> u32 {
    let coast = match coast_preset_by_selector(preset) {
        Some(coast) => coast,
        None => return WB_ERR_PARAM,
    };
    if out_ptr.is_null() {
        return WB_ERR_BUFFER;
    }
    let address = out_ptr as usize; // cast-ok: a pointer to an integer for an alignment check, no float anywhere near it
    if address % core::mem::align_of::<f64>() != 0 {
        return WB_ERR_BUFFER;
    }
    match usize::try_from(out_len) {
        Ok(words) if words == WB_COAST_STRIDE => {}
        _ => return WB_ERR_BUFFER,
    }
    let values = encode_coast(&coast);
    for (offset, value) in values.into_iter().enumerate() {
        unsafe { out_ptr.add(offset).write(value) };
    }
    WB_OK
}

/// Ask whether a coast record would be accepted, **without building a world**.
///
/// `WB_OK` for a record [`wb_world_new_coast`] would take (including the canonical null/zero
/// pair), `WB_ERR_BUFFER` for an unusable buffer, `WB_ERR_PARAM` for a field outside its
/// documented domain.
///
/// The constructor answers a refusal with a handle of 0, which says *that* it refused and never
/// *why*. A panel driving this channel needs the difference, and so does a sweep, which must be
/// able to tell "refused" from "accepted and then fatal".
/// `the_coast_checker_and_the_constructor_agree_on_every_swept_record` holds the two to each
/// other across the whole sweep so this cannot drift into a second, laxer validator.
///
/// # Safety
/// If `coast_len` is non-zero, `coast_ptr` must be a live, 8-aligned allocation of at least
/// `coast_len` f64.
#[no_mangle]
pub extern "C" fn wb_coast_check(coast_ptr: *const f64, coast_len: u32) -> u32 {
    match unsafe { read_coast(coast_ptr, coast_len) } {
        CoastArg::Canonical | CoastArg::Chosen(_) => WB_OK,
        CoastArg::Refused(status) => status,
    }
}


/// Build a world with a caller-chosen relief block, tectonic block, coast block **and** gully
/// block, or **0** if it refused.
///
/// Exactly [`wb_world_new_coast`] plus a gully record, and every one of that function's
/// domains -- and the three doors before it -- still applies unchanged.
///
/// # Why a fifth door rather than a wider fourth one
///
/// The same reason the fourth gives for not widening the third: `wb_world_new_coast` already
/// ships in a committed `.wasm` that the parity harness compares against, and widening its
/// arity would break every existing caller for a parameter most of them never want. All five
/// doors are one `build_world` behind the boundary, so there is one `Surface::with_gully` call
/// in this file and not five.
///
/// # The gully argument
///
/// - **`gully_ptr` null with `gully_len == 0` is the canonical path** -- `None`, not
///   `Some(canonical())`. Ruling 1, held at the door. This channel is the one where that
///   distinction has teeth: `None` builds no steering lattice, so the canonical world does not
///   merely add zero, it does not evaluate the term at all.
/// - Otherwise `gully_len` must be exactly [`WB_GULLY_STRIDE`] and `gully_ptr` a live,
///   8-aligned buffer of that many f64 in the order that constant documents. Every field is
///   bounded, and **a single field outside its domain refuses the whole call.**
///
/// **One of those bounds is not politeness.** [`WB_MIN_GULLY_SHARPNESS`] refuses an exponent
/// at or below zero, which would raise the crest of every gully in the world to an infinite
/// height and hand a host a non-finite vertex.
///
/// A host that wants to know *why* a record was refused calls [`wb_gully_check`] on the same
/// buffer.
///
/// # Safety
/// The feature-, relief-, tectonic- and coast-channel safety requirements of
/// [`wb_world_new_coast`] apply unchanged. If `gully_len` is non-zero, `gully_ptr` must be a
/// live, 8-aligned allocation of at least `gully_len` f64.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn wb_world_new_gully(
    world_seed: i64,
    radius_m: f64,
    plate_count: u32,
    land_fraction: f64,
    features_ptr: *const f64,
    feature_count: u32,
    relief_ptr: *const f64,
    relief_len: u32,
    tectonic_ptr: *const f64,
    tectonic_len: u32,
    coast_ptr: *const f64,
    coast_len: u32,
    gully_ptr: *const f64,
    gully_len: u32,
) -> u32 {
    let relief = match unsafe { read_relief(relief_ptr, relief_len) } {
        ReliefArg::Canonical => None,
        ReliefArg::Chosen(relief) => Some(relief),
        ReliefArg::Refused(_) => return 0,
    };
    let tectonics = match unsafe { read_tectonic(tectonic_ptr, tectonic_len) } {
        TectonicArg::Canonical => None,
        TectonicArg::Chosen(tectonics) => Some(tectonics),
        TectonicArg::Refused(_) => return 0,
    };
    let coast = match unsafe { read_coast(coast_ptr, coast_len) } {
        CoastArg::Canonical => None,
        CoastArg::Chosen(coast) => Some(coast),
        CoastArg::Refused(_) => return 0,
    };
    let gully = match unsafe { read_gully(gully_ptr, gully_len) } {
        GullyArg::Canonical => None,
        GullyArg::Chosen(gully) => Some(gully),
        GullyArg::Refused(_) => return 0,
    };
    unsafe {
        build_world(
            world_seed,
            radius_m,
            plate_count,
            land_fraction,
            features_ptr,
            feature_count,
            relief,
            tectonics,
            coast,
            gully,
        )
    }
}

/// Write a named gully preset's ten f64 into a caller buffer, in [`WB_GULLY_STRIDE`]'s order.
///
/// `WB_OK`, or `WB_ERR_PARAM` for a selector this build does not know, or `WB_ERR_BUFFER` for
/// a null, misaligned, or wrongly-sized buffer. The selectors are [`WB_GULLY_CANONICAL`] and
/// [`WB_GULLY_DRAINAGE`].
///
/// **This export exists so no host ever transcribes the slope scale.** It is the one number in
/// this record that is a measurement of this generator rather than a preference, and a second
/// copy of it in a panel would be a second answer to "how steep is steep here".
///
/// # Safety
/// `out_ptr` must be a live, 8-aligned allocation of at least `out_len` f64.
#[no_mangle]
pub extern "C" fn wb_gully_preset(preset: u32, out_ptr: *mut f64, out_len: u32) -> u32 {
    let gully = match gully_preset_by_selector(preset) {
        Some(gully) => gully,
        None => return WB_ERR_PARAM,
    };
    if out_ptr.is_null() {
        return WB_ERR_BUFFER;
    }
    let address = out_ptr as usize; // cast-ok: a pointer to an integer for an alignment check, no float anywhere near it
    if address % core::mem::align_of::<f64>() != 0 {
        return WB_ERR_BUFFER;
    }
    match usize::try_from(out_len) {
        Ok(words) if words == WB_GULLY_STRIDE => {}
        _ => return WB_ERR_BUFFER,
    }
    let values = encode_gully(&gully);
    for (offset, value) in values.into_iter().enumerate() {
        unsafe { out_ptr.add(offset).write(value) };
    }
    WB_OK
}

/// Ask whether a gully record would be accepted, **without building a world**.
///
/// `WB_OK` for a record [`wb_world_new_gully`] would take (including the canonical null/zero
/// pair), `WB_ERR_BUFFER` for an unusable buffer, `WB_ERR_PARAM` for a field outside its
/// documented domain. Same contract as [`wb_coast_check`], and held to the constructor by
/// `the_gully_checker_and_the_constructor_agree_on_every_swept_record` so this cannot drift
/// into a second, laxer validator.
///
/// # Safety
/// If `gully_len` is non-zero, `gully_ptr` must be a live, 8-aligned allocation of at least
/// `gully_len` f64.
#[no_mangle]
pub extern "C" fn wb_gully_check(gully_ptr: *const f64, gully_len: u32) -> u32 {
    match unsafe { read_gully(gully_ptr, gully_len) } {
        GullyArg::Canonical | GullyArg::Chosen(_) => WB_OK,
        GullyArg::Refused(status) => status,
    }
}

/// Write a named tectonic preset's nine f64 into a caller buffer, in
/// [`WB_TECTONIC_STRIDE`]'s order.
///
/// `WB_OK`, or `WB_ERR_PARAM` for a selector this build does not know, or `WB_ERR_BUFFER` for
/// a null, misaligned, or wrongly-sized buffer. The only selector is
/// [`WB_TECTONIC_CANONICAL`].
///
/// **This export exists so no host ever transcribes a tectonic default.** The panel's three
/// sliders are anchored on canonical -- every one of them reads its own centre or one of its
/// ends from here -- so `tectonics.rs` stays the only place `1500.0`, `400_000.0` and `0.45`
/// are written down. The viewer holds none of the three.
///
/// # Safety
/// `out_ptr` must be a live, 8-aligned allocation of at least `out_len` f64.
#[no_mangle]
pub extern "C" fn wb_tectonic_preset(preset: u32, out_ptr: *mut f64, out_len: u32) -> u32 {
    let tectonics = match tectonic_preset_by_selector(preset) {
        Some(tectonics) => tectonics,
        None => return WB_ERR_PARAM,
    };
    if out_ptr.is_null() {
        return WB_ERR_BUFFER;
    }
    let address = out_ptr as usize; // cast-ok: a pointer to an integer for an alignment check, no float anywhere near it
    if address % core::mem::align_of::<f64>() != 0 {
        return WB_ERR_BUFFER;
    }
    match usize::try_from(out_len) {
        Ok(words) if words == WB_TECTONIC_STRIDE => {}
        _ => return WB_ERR_BUFFER,
    }
    let values = encode_tectonic(&tectonics);
    for (offset, value) in values.into_iter().enumerate() {
        unsafe { out_ptr.add(offset).write(value) };
    }
    WB_OK
}

/// Ask whether a tectonic record would be accepted, **without building a world**.
///
/// `WB_OK` for a record [`wb_world_new_tectonic`] would take (including the canonical
/// null/zero pair), `WB_ERR_BUFFER` for an unusable buffer, `WB_ERR_PARAM` for a field
/// outside its documented domain.
///
/// The constructor answers a refusal with a handle of 0, which says *that* it refused and
/// never *why*. A panel driving three of these nine fields needs the difference, and so does
/// a sweep, which must be able to tell "refused" from "accepted and then fatal".
/// `the_tectonic_checker_and_the_constructor_agree_on_every_swept_record` holds the two to
/// each other across the whole sweep so this cannot drift into a second, laxer validator.
///
/// # Safety
/// If `tectonic_len` is non-zero, `tectonic_ptr` must be a live, 8-aligned allocation of at
/// least `tectonic_len` f64.
#[no_mangle]
pub extern "C" fn wb_tectonic_check(tectonic_ptr: *const f64, tectonic_len: u32) -> u32 {
    match unsafe { read_tectonic(tectonic_ptr, tectonic_len) } {
        TectonicArg::Canonical | TectonicArg::Chosen(_) => WB_OK,
        TectonicArg::Refused(status) => status,
    }
}

/// Write a named preset's ten f64 into a caller buffer, in [`WB_RELIEF_STRIDE`]'s order.
///
/// `WB_OK`, or `WB_ERR_PARAM` for a selector this build does not know, or `WB_ERR_BUFFER`
/// for a null, misaligned, or wrongly-sized buffer. Selectors are [`WB_RELIEF_CANONICAL`]
/// and [`WB_RELIEF_HILLS`].
///
/// **This export exists so no host ever transcribes a preset.** The pre-flight conflict
/// scan for this slice flagged exactly that: Task 3 names the preset, Task 4 exposes it, and
/// two copies of three numbers drift the first time one is retuned. The viewer's panel reads
/// its slider defaults *and* its preset button from here, so `detail.rs` stays the only
/// place the numbers live.
///
/// # Safety
/// `out_ptr` must be a live, 8-aligned allocation of at least `out_len` f64.
#[no_mangle]
pub extern "C" fn wb_relief_preset(preset: u32, out_ptr: *mut f64, out_len: u32) -> u32 {
    let relief = match preset_by_selector(preset) {
        Some(relief) => relief,
        None => return WB_ERR_PARAM,
    };
    if out_ptr.is_null() {
        return WB_ERR_BUFFER;
    }
    let address = out_ptr as usize; // cast-ok: a pointer to an integer for an alignment check, no float anywhere near it
    if address % core::mem::align_of::<f64>() != 0 {
        return WB_ERR_BUFFER;
    }
    match usize::try_from(out_len) {
        Ok(words) if words == WB_RELIEF_STRIDE => {}
        _ => return WB_ERR_BUFFER,
    }
    let values = encode_relief(&relief);
    for (offset, value) in values.into_iter().enumerate() {
        unsafe { out_ptr.add(offset).write(value) };
    }
    WB_OK
}

/// Ask whether a relief record would be accepted, **without building a world**.
///
/// `WB_OK` for a record [`wb_world_new_relief`] would take (including the canonical
/// null/zero pair), `WB_ERR_BUFFER` for an unusable buffer, `WB_ERR_PARAM` for a field
/// outside its documented domain.
///
/// The constructor answers a refusal with a handle of 0, which says *that* it refused and
/// never *why*. A UI wiring sliders to these parameters needs the difference -- a panel that
/// can only report "the engine said no" pushes the user into bisecting ten fields by hand --
/// and a test sweeping the domain needs it too, so that "refused" and "accepted but fatal"
/// cannot be confused for one another.
/// `the_relief_checker_and_the_constructor_agree_on_every_swept_record` holds the two to
/// each other across the whole sweep, so this cannot drift into a second, laxer validator.
///
/// # Safety
/// If `relief_len` is non-zero, `relief_ptr` must be a live, 8-aligned allocation of at
/// least `relief_len` f64.
#[no_mangle]
pub extern "C" fn wb_relief_check(relief_ptr: *const f64, relief_len: u32) -> u32 {
    match unsafe { read_relief(relief_ptr, relief_len) } {
        ReliefArg::Canonical | ReliefArg::Chosen(_) => WB_OK,
        ReliefArg::Refused(status) => status,
    }
}

/// The one `Surface::new` call in this file, behind both constructors.
///
/// # Safety
/// If `feature_count` is non-zero, `features_ptr` must be a live, 8-aligned allocation of at
/// least `feature_count * WB_FEATURE_STRIDE` f64.
#[allow(clippy::too_many_arguments)]
unsafe fn build_world(
    world_seed: i64,
    radius_m: f64,
    plate_count: u32,
    land_fraction: f64,
    features_ptr: *const f64,
    feature_count: u32,
    relief: Option<ReliefParams>,
    tectonics: Option<TectonicParams>,
    coast: Option<CoastParams>,
    gully: Option<GullyParams>,
) -> u32 {
    if !radius_m.is_finite() || radius_m <= 0.0 || radius_m > WB_MAX_WORLD_RADIUS_M {
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
        let records = core::slice::from_raw_parts(features_ptr, words);
        let mut decoded = Vec::with_capacity(count);
        for record in records.chunks_exact(WB_FEATURE_STRIDE) {
            match decode_feature(record) {
                Some(feature) => decoded.push(feature),
                None => return 0,
            }
        }
        Some(FeatureInput::Loose(decoded))
    };

    // All FOUR blocks arrive already validated -- `read_relief`, `read_tectonic`,
    // `read_coast` and `read_gully` refuse at the boundary, so nothing outside any documented
    // domain reaches here. `None` is the canonical path for each, and is what `wb_world_new`
    // always passes for all four. `with_gully` rather than `new` so this file still holds
    // exactly ONE `Surface` constructor call behind five doors; `new` delegates to
    // `with_coast`, which delegates to `with_gully` with `None`, so the canonical path is the
    // same code either way.
    let surface = Surface::with_gully(
        world_seed,
        radius_m,
        plates,
        land_fraction,
        features,
        relief,
        tectonics,
        coast,
        gully,
    );
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
/// tile costs about 3.6x a deep-ocean one on medians** -- 14.46 ms against 4.05, over 137
/// and 131 of 480 level-12 tiles classified from their filled heights. (An earlier "up to
/// 9x" here compared the extremes of that table rather than its typical tiles.) Coasts are
/// what a viewer looks at, so no tile can be filled on the main thread inside a frame. Fill
/// in workers, and cache.
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

/// What a `march_samples` argument decoded to. Three outcomes and not two, because
/// "canonical" and "explicitly 160 steps" are the same *field* and different *requests*:
/// `None` is the path `Surface::moisture_index` guarantees bit-identical to the engine's own
/// default, and a `Some` carrying `MoistureParams::canonical()` is only pinned equal to it by
/// a test. Collapsing them here would make the export unable to ask for the guaranteed path.
enum ClimateBudget {
    /// `WB_CLIMATE_CANONICAL_SAMPLES`: the engine's `None`.
    Canonical,
    /// A bounded explicit march.
    Explicit(climate::MoistureParams),
    /// Out of domain. The caller returns `WB_ERR_PARAM`.
    Refused,
}

/// Decode `march_samples`. See [`WB_MAX_CLIMATE_MARCH_SAMPLES`] for why the ceiling is the
/// type's own and what this adds over it.
fn climate_budget(march_samples: u32) -> ClimateBudget {
    if march_samples == WB_CLIMATE_CANONICAL_SAMPLES {
        return ClimateBudget::Canonical;
    }
    if march_samples > WB_MAX_CLIMATE_MARCH_SAMPLES {
        return ClimateBudget::Refused;
    }
    let samples = march_samples as u16; // cast-ok: bounded above by WB_MAX_CLIMATE_MARCH_SAMPLES, which is a u16 constant widened, so this narrowing is exact
    // The type is still asked, rather than trusted to agree with the comparison above: two
    // bounds for one fact is how a ceiling and a constructor drift apart.
    match climate::MarchBudget::new(samples) {
        Some(budget) => {
            let mut params = climate::MoistureParams::canonical();
            params.budget = budget;
            ClimateBudget::Explicit(params)
        }
        None => ClimateBudget::Refused,
    }
}

/// Fill a rectangular grid of **climate** into linear memory, shaped for a `Float32Array`:
/// two f32 per sample, `[temperature_c_at_datum, moisture_index]`. See [`WB_CLIMATE_STRIDE`]
/// for why the temperature is at the datum.
///
/// # The grid
///
/// Exactly [`wb_fill_tile_f32`]'s: row-major, `width` columns by `height` rows, both endpoints
/// included, `grid_coordinate` between the bounds, no hemisphere baked in. It is the same
/// function and the same interpolation deliberately, so a consumer can lay this grid over that
/// one without a second convention to get wrong. **Element `(row * width + col) * 2` is the
/// temperature and `+ 1` is the moisture.**
///
/// # `march_samples`, and why it is the only climate parameter here
///
/// [`WB_CLIMATE_CANONICAL_SAMPLES`] for the engine's own march; otherwise `0 ..=`
/// [`WB_MAX_CLIMATE_MARCH_SAMPLES`], and **it is a loop bound** -- see that constant. The other
/// four `MoistureParams` fields and all three `ClimateParams` fields are canonical and have no
/// argument: this export exists to feed a viewer's land colour, the viewer has no control that
/// sets them, and a parameter nobody can move is a sweep surface with no consumer. A caller who
/// wants them is calling the Rust API, not this door.
///
/// # Cost, and why a consumer must rasterise this COARSELY
///
/// One sample is one moisture march, and one march at the canonical budget is 161 elevation
/// queries. Measured on this project's owner world (radius 9,309,000 m, `ranges` and `fractal`
/// presets), a 160-sample query costs **560-770 us native**. Texels are quadratic in the raster
/// edge and samples are linear in the budget, so **the raster is the lever and the budget is
/// not**: at the relief layer's own 256-texel raster this grid would cost minutes per tile.
/// The viewer draws it at **16 x 16** and interpolates -- see `viewer/public/app/biome.js`.
///
/// # Returns
///
/// `WB_OK`, or `WB_ERR_GRID` for a zero dimension or a non-finite bound, `WB_ERR_PARAM` for a
/// `march_samples` outside its domain, `WB_ERR_BUFFER` for a null, misaligned or short buffer,
/// `WB_ERR_HANDLE` for an unknown world. **Nothing is written on any refusal**, exactly as
/// [`wb_fill_tile_f32`] writes nothing: a half-filled climate grid reads as a climate.
///
/// A NaN in the payload is **not** a refusal and is the contract working: `climate.rs`'s NaN
/// posture is propagate, so a moisture nobody can compute arrives as NaN rather than as the
/// saturated air the spike found it silently returning. A consumer must not band it.
///
/// # Safety
/// `out` must be null, or a live 4-aligned allocation of at least `out_len` f32.
#[no_mangle]
pub extern "C" fn wb_climate_tile_f32(
    handle: u32,
    lat0_deg: f64,
    lat1_deg: f64,
    lon0_deg: f64,
    lon1_deg: f64,
    width: u32,
    height: u32,
    resolution_m: f64,
    march_samples: u32,
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
    // Both multiplications are checked. The stride one matters as much as the other: a
    // `width * height` that fits and a `width * height * 2` that does not is exactly the
    // cross-product shape this project's one surviving abort had.
    let values = match columns
        .checked_mul(rows)
        .and_then(|samples| samples.checked_mul(WB_CLIMATE_STRIDE))
    {
        Some(values) => values,
        None => return WB_ERR_GRID,
    };
    let moisture = match climate_budget(march_samples) {
        ClimateBudget::Canonical => None,
        ClimateBudget::Explicit(params) => Some(params),
        ClimateBudget::Refused => return WB_ERR_PARAM,
    };
    if out.is_null() {
        return WB_ERR_BUFFER;
    }
    let address = out as usize; // cast-ok: a pointer to an integer for an alignment check, not a float truncation
    if address % core::mem::align_of::<f32>() != 0 {
        return WB_ERR_BUFFER;
    }
    match usize::try_from(out_len) {
        Ok(len) if len >= values => {}
        _ => return WB_ERR_BUFFER,
    }

    let filled = with_world(handle, |world| {
        let surface = world.surface();
        let resolution_m = resolution(resolution_m);
        let temperature = climate::ClimateParams::canonical();
        let last_row = f64::from(height - 1);
        let last_column = f64::from(width - 1);
        let buffer = unsafe { core::slice::from_raw_parts_mut(out, values) };
        for row in 0..rows {
            let down = row as f64; // cast-ok: a grid row index to float, exact for any tile that fits in memory
            let latitude_deg = grid_coordinate(lat0_deg, lat1_deg, down, last_row);
            // Hoisted out of the column loop because it does not depend on longitude, not as
            // an optimisation: the datum temperature IS a function of latitude alone, and
            // computing it per column would invite a later edit to make it one of two.
            let datum_c = climate::temperature_c(latitude_deg, 0.0, &temperature);
            for column in 0..columns {
                let across = column as f64; // cast-ok: a grid column index to float, exact for any tile that fits in memory
                let longitude_deg = grid_coordinate(lon0_deg, lon1_deg, across, last_column);
                let point = SpherePoint::from_latlon(latitude_deg, longitude_deg);
                let wetness = surface.moisture_index(&point, resolution_m, moisture);
                let base = (row * columns + column) * WB_CLIMATE_STRIDE;
                buffer[base] = datum_c as f32; // cast-ok: narrowing a temperature for a Float32Array, and the consumer bands it against edges 6 to 10 C apart
                buffer[base + 1] = wetness as f32; // cast-ok: narrowing a dimensionless index in [0, 1] for a Float32Array
            }
        }
    });

    match filled {
        Some(()) => WB_OK,
        None => WB_ERR_HANDLE,
    }
}

/// This world's own climate calibration: the band edges both quantiled axes are cut at, plus
/// the two numbers a consumer needs to use them. [`WB_CLIMATE_CALIBRATION_STRIDE`] f64, in that
/// order:
///
/// | index | value |
/// |---|---|
/// | 0..3 | the four moisture edges, ascending, in the march's own dimensionless units |
/// | 4..5 | the two landform edges, ascending, in metres of elevation |
/// | 6 | how many of `climate::BAND_CALIBRATION_SAMPLES` were land |
/// | 7 | the lapse rate, in C per km |
///
/// # The lapse rate is here because the temperature channel is at the datum
///
/// See [`WB_CLIMATE_STRIDE`]. The consumer applies the lapse at its own raster's resolution,
/// so it needs the constant; getting it from here rather than writing `6.5` down again is what
/// stops the viewer and the engine disagreeing about how fast air cools.
///
/// # It is a per-world constant and it is NOT cheap
///
/// `climate::BAND_CALIBRATION_SAMPLES` (4,000) elevations plus one march at every land point:
/// roughly 190,000 elevation queries on a 29%-land world, and seconds in WASM at the canonical
/// budget. **Call it once when the world is built and keep the answer.** In the viewer it runs
/// in a pool worker for exactly that reason, as `wb_water_run` does.
///
/// # NaN edges are an answer, not a failure
///
/// A world with no land, or one any sample of which could not be answered, calibrates to NaN
/// edges and `WB_OK` -- `climate::BandEdges::calibrate`'s own contract. Index 6 is what tells
/// the two apart, and `climate::band_index` refuses a NaN edge rather than banding against it.
/// A status code here would be a second mechanism for a fact the payload already carries.
///
/// # Returns
///
/// `WB_OK`, `WB_ERR_PARAM` for a `march_samples` outside its domain, `WB_ERR_BUFFER` for a
/// null, misaligned or short buffer, `WB_ERR_HANDLE` for an unknown world. Nothing is written
/// on any refusal.
///
/// # Safety
/// `out` must be null, or a live 8-aligned allocation of at least `out_len` f64.
#[no_mangle]
pub extern "C" fn wb_climate_calibration(
    handle: u32,
    resolution_m: f64,
    march_samples: u32,
    out: *mut f64,
    out_len: u32,
) -> u32 {
    let moisture = match climate_budget(march_samples) {
        ClimateBudget::Canonical => None,
        ClimateBudget::Explicit(params) => Some(params),
        ClimateBudget::Refused => return WB_ERR_PARAM,
    };
    if out.is_null() {
        return WB_ERR_BUFFER;
    }
    let address = out as usize; // cast-ok: a pointer to an integer for an alignment check, not a float truncation
    if address % core::mem::align_of::<f64>() != 0 {
        return WB_ERR_BUFFER;
    }
    match usize::try_from(out_len) {
        Ok(len) if len >= WB_CLIMATE_CALIBRATION_STRIDE => {}
        _ => return WB_ERR_BUFFER,
    }

    let written = with_world(handle, |world| {
        let edges = world.surface().band_edges(resolution(resolution_m), moisture);
        let buffer = unsafe { core::slice::from_raw_parts_mut(out, WB_CLIMATE_CALIBRATION_STRIDE) };
        let mut index = 0;
        for edge in edges.moisture() {
            buffer[index] = *edge;
            index += 1;
        }
        for edge in edges.landform() {
            buffer[index] = *edge;
            index += 1;
        }
        buffer[index] = edges.land_samples() as f64; // cast-ok: a count bounded by BAND_CALIBRATION_SAMPLES (4,000) to float, exact
        index += 1;
        buffer[index] = climate::LAPSE_C_PER_KM;
    });

    match written {
        Some(()) => WB_OK,
        None => WB_ERR_HANDLE,
    }
}

/// The `sea_level_m` [`wb_erosion_run`] builds its graph at. Not a parameter this task
/// exposes: `StreamGraph::build`'s classification into land/boundary and the lake/mouth
/// split for a root are slice 5b's concern to expose as a caller-chosen value. `0.0` is a
/// datum, not a claim about where any particular world's coastline sits.
///
/// **This is not inert, and an earlier version of this doc said it was ("the solver reads
/// no flag it produces").** The whole-branch review of slice 5a corrected that: `flags[i]`
/// is `LAND` or `BOUNDARY` depending on `height_m[i] > sea_level_m`
/// (`stream.rs::StreamGraph::build`), and a `BOUNDARY` node is a root -- `erode_step`
/// holds every root fixed for the whole run, as the local base level its basin erodes
/// toward. Moving `sea_level_m` moves the root set, and the root set IS the boundary
/// condition the solver relaxes toward. The solver's *step* does not read the flag
/// directly, only `downhill_of`, which the flag decides -- so the narrower claim ("no
/// per-step flag read") is true and the broader one ("inert") is not. Whoever exposes
/// lakes and a real sea level in slice 5b needs to revisit this constant with that in
/// mind, not merely add a parameter alongside it.
const WB_EROSION_SEA_LEVEL_M: f64 = 0.0;
/// The `pond_max_surface_area_m2` [`wb_erosion_run`] builds its graph at. Nothing here
/// reads `Lake::kind` (see [`WB_EROSION_SEA_LEVEL_M`]'s doc), so which side of this value
/// `StreamGraph::build`'s own placeholder classification lands on is inert either way --
/// this is a required field with no default (`BuildParams`'s own doc), not a real
/// calibration for this export. The same value `erosion.rs`'s own unit tests use, not
/// independently chosen.
const WB_EROSION_POND_MAX_SURFACE_AREA_M2: f64 = 1.0e10;

/// Erode an existing world's surface to (or toward) convergence, over a freshly sampled
/// stream graph, by the Cordonnier implicit stream-power method -- the *capped* path, i.e.
/// [`crate::erosion::erode_to_convergence`], which is what the engine actually ships (Task
/// 4 wired the thermal slope cap inside this function, not inside the uncapped
/// `erode_step`; see `erosion.rs`'s module doc).
///
/// # Why this takes a world handle rather than building its own `Surface`
///
/// `the_surface_is_built_once_per_world_and_never_per_sample` (`tests/wasm_exports.rs`)
/// holds this whole file to exactly one `Surface::new` call, inside `wb_world_new` -- for
/// the ~10^3x reason that test's doc gives. This function therefore samples height from an
/// **already-built** world's surface (`wb_world_new` first, same as every other export
/// here) rather than constructing a second one, which is also the only way a caller could
/// ever compare an eroded and an unerorded reading of the *same* planet.
///
/// # What this does NOT do
///
/// It does not store the resulting graph back onto the world (`World::attach_streams`
/// exists but this function never calls it), does not touch lakes, water, or `Surface`
/// itself, and does not change the solver's arithmetic -- `erosion.rs` is untouched by this
/// task. It exists to make erosion's native/WASM parity claim testable at all (see
/// `erosion.rs`'s module doc, "native against WASM... both hold bit-for-bit"), which
/// nothing could exercise before this export existed.
///
/// # Parameters and their domains
///
/// - `handle`: an existing world from `wb_world_new`. `WB_ERR_HANDLE` if stale or unknown.
/// - `node_count`: `2..=`[`WB_MAX_EROSION_NODES`]. Below 2, `stream::sample_nodes` refuses
///   (no neighbour relation, no drainage); above the ceiling, `WB_ERR_PARAM` -- see that
///   constant's doc for why the ceiling sits far below the 20,000,000-node planetary
///   target rather than at it.
/// - `uplift_m_per_yr`: finite, `abs() <=` [`WB_MAX_EROSION_RATE_PER_YR`].
/// - `erodibility_per_yr`: finite, `>= 0.0`, `<=` [`WB_MAX_EROSION_RATE_PER_YR`]. See that
///   constant's doc for why the lower bound is `0.0` rather than `-WB_MAX_EROSION_RATE_PER_YR`
///   like `uplift_m_per_yr`'s -- negative `erodibility_per_yr` is a distinct, measured abort,
///   not a symmetric extension of the magnitude ceiling.
/// - `timestep_yr`: finite, strictly positive, `<=` [`WB_MAX_EROSION_TIMESTEP_YR`].
/// - `max_height_change_per_step_m`: finite, `>= 0.0` (the convergence threshold; `0.0` is
///   accepted and simply never converges early).
/// - `max_iterations`: `1..=`[`WB_MAX_EROSION_ITERATIONS`], but see that constant's doc for
///   why this ceiling is not safe to use at [`WB_MAX_EROSION_NODES`] simultaneously.
///
/// **These bounds close the one abort this task found, not a proof that none remain.**
/// [`crate::erosion::erode_to_convergence`]'s release-time `assert!(!change.is_nan())` (see
/// that function's doc) is correct inside Rust -- a NaN height change is a real defect
/// worth failing loudly on -- but `extern "C"` is nounwind, so a panic that reaches this
/// boundary aborts the whole module rather than returning a status; this file's own doc
/// records that measured, for `wb_world_new`'s `land_fraction` bound, as
/// `STATUS_STACK_BUFFER_OVERRUN` taking twenty-seven unrelated tests down with it. The one
/// path this task found into that assertion was `erodibility_per_yr < 0.0` turning `1 + c`
/// into an amplifying map (see [`WB_MAX_EROSION_RATE_PER_YR`]'s doc) -- found by reasoning
/// about that one term's sign, confirmed both natively and in the shipped `.wasm`, and
/// closed by the `>= 0.0` bound above. **A second, independent path was found by the
/// whole-branch review that followed: an enormous but finite `radius_m`, entirely outside
/// this function's own six parameters, overflows `4*pi*radius_m^2` to `+inf` in
/// `stream::node_areas_m2` and reaches the identical assertion by a different route --
/// closed at `wb_world_new` and at `StreamGraph::build`'s own area check, not here, because
/// no bound on this function's parameters could have caught an input that was already
/// wrong before any of them were read** (see [`WB_MAX_WORLD_RADIUS_M`]'s doc). **No
/// exhaustive search of the remaining in-domain parameter space (in particular, adversarial
/// combinations of `A_drainage`, `k`, `dt` and a very small receiver distance `d`) was
/// made**, and finding two reachable paths into the same assertion by two different reviews
/// should raise the prior that a third exists rather than lower it.
///
/// # The cap is called, but inert over this crate's own corpus
///
/// This function always calls the *capped* path (see this doc's opening paragraph), but
/// calling it is not the same as exercising its arithmetic: `cap_slopes`' clamp branch
/// binds a slope to `slope_cap_tan()` (`tan(30 degrees)`, a `detmath` transcendental) only
/// when a slope exceeds it, and Task 4 measured that cap as inert at every node count this
/// crate has tested -- a 30-degree slope needs more rise than fits between neighbours at
/// these spacings. Measured directly for this export's own parity fixture (3,000 nodes,
/// this crate's default test constants, 20 iterations): `ClampStats { total_edges_clamped:
/// 0, iterations_with_a_clamp: 0 }`. So the native/WASM parity corpus this export feeds is
/// a genuine test of `sqrt`, `atan2` (via `receiver_distances_m`) and the implicit update --
/// and is not a test of `cap_slopes`' own clamp arithmetic, which stays untested by parity
/// until a corpus reaches the node density where the cap can fire at all.
///
/// # Output
///
/// `out_heights[0..node_count]` is the height field after the run -- the *last* step's
/// result whether or not it converged, exactly as [`crate::erosion::ErosionRun`] carries it.
/// `*out_iterations` is how many `erode_step` calls ran; `*out_converged` is `1` if the run
/// reached [`ErosionRun::Converged`] and `0` for [`ErosionRun::NotConverged`] -- a caller
/// that only reads `out_heights` cannot silently mistake a capped, unconverged run for a
/// settled one, the same reason `ErosionRun` is an enum and not a bare `Vec<f64>` at all.
///
/// # Returns
///
/// `WB_OK`, `WB_ERR_HANDLE` for an unknown world, `WB_ERR_PARAM` for a numeric argument
/// outside the domains above, `WB_ERR_BUFFER` for a null, misaligned or short output
/// buffer, or `WB_ERR_GRAPH` if the sampled node set could not be built into a graph (see
/// that constant's doc for why this is believed unreachable today and kept as a status
/// anyway). Nothing is written to any output buffer on a refusal.
///
/// # Safety
/// `out_heights` must be null, or a live 8-aligned allocation of at least `out_len` f64
/// with `out_len >= node_count`. `out_iterations` and `out_converged` must each be null, or
/// a live 4-aligned allocation of at least one `u32`.
#[no_mangle]
pub extern "C" fn wb_erosion_run(
    handle: u32,
    node_count: u32,
    uplift_m_per_yr: f64,
    erodibility_per_yr: f64,
    timestep_yr: f64,
    max_height_change_per_step_m: f64,
    max_iterations: u32,
    out_heights: *mut f64,
    out_len: u32,
    out_iterations: *mut u32,
    out_converged: *mut u32,
) -> u32 {
    if node_count < 2 || node_count > WB_MAX_EROSION_NODES {
        return WB_ERR_PARAM;
    }
    if !uplift_m_per_yr.is_finite() || uplift_m_per_yr.abs() > WB_MAX_EROSION_RATE_PER_YR {
        return WB_ERR_PARAM;
    }
    // `erodibility_per_yr` is refused below zero, unlike `uplift_m_per_yr` above -- a
    // negative `k` makes `c = k * dt * sqrt(A_drainage) / d` negative, which turns
    // `implicit_receiver_update`'s `1.0 / (1.0 + c)` into an amplifying map instead of a
    // contraction and overflows to `inf` within about a hundred iterations at this crate's
    // own `dt`, tripping `erode_to_convergence`'s release-time NaN assertion -- an abort
    // across this boundary. See `WB_MAX_EROSION_RATE_PER_YR`'s doc for the measured native
    // and WASM traces this refusal closes.
    if !erodibility_per_yr.is_finite() || !(erodibility_per_yr >= 0.0) || erodibility_per_yr > WB_MAX_EROSION_RATE_PER_YR {
        return WB_ERR_PARAM;
    }
    if !timestep_yr.is_finite() || !(timestep_yr > 0.0) || timestep_yr > WB_MAX_EROSION_TIMESTEP_YR {
        return WB_ERR_PARAM;
    }
    if !max_height_change_per_step_m.is_finite() || max_height_change_per_step_m < 0.0 {
        return WB_ERR_PARAM;
    }
    if max_iterations == 0 || max_iterations > WB_MAX_EROSION_ITERATIONS {
        return WB_ERR_PARAM;
    }

    let count = match usize::try_from(node_count) {
        Ok(count) => count,
        Err(_) => return WB_ERR_PARAM,
    };

    if out_heights.is_null() {
        return WB_ERR_BUFFER;
    }
    let heights_address = out_heights as usize; // cast-ok: a pointer to an integer for an alignment check, not a float truncation
    if heights_address % core::mem::align_of::<f64>() != 0 {
        return WB_ERR_BUFFER;
    }
    match usize::try_from(out_len) {
        Ok(len) if len >= count => {}
        _ => return WB_ERR_BUFFER,
    }
    if out_iterations.is_null() || out_converged.is_null() {
        return WB_ERR_BUFFER;
    }
    for address in [out_iterations as usize, out_converged as usize] {
        // cast-ok: a pointer to an integer for an alignment check, not a float truncation
        if address % core::mem::align_of::<u32>() != 0 {
            return WB_ERR_BUFFER;
        }
    }

    let params = ErosionParams {
        uplift_m_per_yr,
        erodibility_per_yr,
        timestep_yr,
        max_height_change_per_step_m,
        max_iterations,
    };

    let outcome = with_world(handle, |world| {
        let world_seed = world.surface().world_seed as u64; // cast-ok: two's-complement reinterpretation, the same one wb_world_new already makes for Noise
        let radius_m = world.surface().radius_m;
        let sampling = sample_nodes(world_seed, node_count, radius_m).ok_or(WB_ERR_GRAPH)?;
        let heights: Vec<f64> =
            sampling.positions.iter().map(|point| world.surface().elevation_m(point, None)).collect();
        let graph = StreamGraph::build(
            &BuildParams {
                world_seed,
                radius_m,
                sea_level_m: WB_EROSION_SEA_LEVEL_M,
                sampling_kind: SamplingKind::Spiral,
                pond_max_surface_area_m2: WB_EROSION_POND_MAX_SURFACE_AREA_M2,
            },
            &sampling.positions,
            &heights,
            &sampling.area_m2,
            &sampling.neighbours,
        )
        .map_err(|_| WB_ERR_GRAPH)?;
        let distances_m = receiver_distances_m(&graph, &sampling.positions);
        Ok(erode_to_convergence(&graph, &heights, &distances_m, &params))
    });

    let run = match outcome {
        None => return WB_ERR_HANDLE,
        Some(Err(code)) => return code,
        Some(Ok(run)) => run,
    };

    let (result_heights, iterations, converged) = match run {
        ErosionRun::Converged { heights, iterations } => (heights, iterations, 1u32),
        ErosionRun::NotConverged { heights, iterations } => (heights, iterations, 0u32),
    };
    debug_assert_eq!(result_heights.len(), count, "erode_to_convergence must return one height per node");

    unsafe {
        core::slice::from_raw_parts_mut(out_heights, count).copy_from_slice(&result_heights);
        *out_iterations = iterations;
        *out_converged = converged;
    }
    WB_OK
}

// ------------------------------------------------------------------- the water channel

/// The ceiling on `node_count` for [`wb_water_run`], chosen **below**
/// [`WB_MAX_EROSION_NODES`] rather than at it, and for a reason that is about memory rather
/// than time.
///
/// [`crate::water::fill_and_resolve_water_with_neighbours`] holds a *second*, symmetric copy
/// of the relation beside the directed one ([`crate::water::symmetric_adjacency`], measured
/// by slice 5b's Task 1 review to add 1.6-3.4% of entries and to raise the maximum degree
/// from 8 to 12). This export therefore holds **two** neighbour structures at once at its
/// peak -- `sample_nodes`' directed relation, which it now passes in rather than letting the
/// water path rebuild, and the symmetric copy made from it -- against `wb_erosion_run`'s one.
///
/// **It used to hold three.** Until the performance profile's Fix 1 this export called
/// `fill_and_resolve_water`, which regenerated the directed relation from the world seed and
/// discarded the caller's; `sample_nodes`' copy, the regenerated one and the symmetric one
/// were all live at the peak. Passing the relation in removes one of the three, so the
/// ceiling below is if anything now conservative -- it is left where it is because nothing
/// re-measured it, not because the old arithmetic still holds.
///
/// The 5b ledger records ~2.2 GB of live neighbour copies at 20,000,000 nodes; **wasm32's
/// linear memory is 4 GiB at the absolute limit and far less in practice**, so a ceiling that
/// is merely survivable natively is not the same as one a browser can meet.
///
/// `100_000` is where this crate's own measurements stop being extrapolations: Task 1
/// measured neighbour regeneration at 3.982 s for 500,000 nodes and 9.038 s for 1,000,000
/// (native, release, k = 8, **superlinear** -- 2x the nodes cost 2.27x the time), and the
/// Task 1 review re-derived the exponent at ~1.13 over three points. Nothing in that series
/// justifies a ceiling at 20,000,000, and this export is not the door to a planetary bake;
/// it is the door that makes slice 5b's water path reachable from a browser and therefore
/// checkable by the native-against-WASM parity harness at all, which nothing could do before
/// it existed.
pub const WB_MAX_WATER_NODES: u32 = 100_000;

/// f64 words per body row [`wb_water_run`] writes, **and the order is the contract**:
///
/// | index | field | note |
/// |---:|---|---|
/// | 0 | `root_node` | `crate::water::Body::root_node`, widened to f64 (lossless below 2^53) |
/// | 1 | `kind` | [`WB_BODY_KIND_LAKE`] or [`WB_BODY_KIND_POND`] |
/// | 2 | `level_m` | the body's filled surface level |
/// | 3 | `extent.min_latitude_deg` | |
/// | 4 | `extent.max_latitude_deg` | |
/// | 5 | `extent.min_longitude_deg` | **may exceed index 6** -- see [`crate::water::Extent`] |
/// | 6 | `extent.max_longitude_deg` | |
///
/// That is `Body`'s own declaration order with `Extent` flattened in place. Rows arrive
/// ascending by `root_node`, which is the order `crate::water::water_manifest` already sorts
/// them into -- a host does not re-sort, and a harness comparing two runs row by row is
/// comparing the same body on both sides by construction.
///
/// **The sea is not one of these rows.** Slice 5b's Ruling 6: the spec defines a mapping of
/// *named* waters and a fallback for the unnamed, and the sea is the mapping's miss rather
/// than an entry in it. The datum is carried once, in `out_sea_level_m`. That is not a
/// simplification made here: it is what `water_manifest` emits, and this export dumps the
/// shipped manifest rather than an intermediate.
pub const WB_WATER_BODY_STRIDE: usize = 7;

/// `kind` code for `crate::water::BodyKind::Lake` in a [`WB_WATER_BODY_STRIDE`] row. f64
/// because a row is a flat f64 array, and the comparison is exact equality -- the same
/// reason `WB_COMPOSE_RAISE` is an f64.
pub const WB_BODY_KIND_LAKE: f64 = 0.0;
/// `kind` code for `crate::water::BodyKind::Pond`. See [`WB_BODY_KIND_LAKE`].
///
/// **No code this generator currently emits at its calibrated threshold.** Slice 5b Task 3
/// calibrated `pond_max_surface_area_m2` at 1.0e5 m^2 on external ground (a shoreline a
/// person could walk in about fifteen minutes) and then measured that the *smallest* body
/// this mesh produces is 7.9e8 m^2 -- nearly four orders of magnitude larger -- at every
/// resolution tested. So at that threshold this value never appears. That is a recorded
/// finding about the mesh rather than a threshold to tune until something falls on each
/// side, and it is stated here so a reader does not conclude the code is dead.
pub const WB_BODY_KIND_POND: f64 = 1.0;

/// Whether every connected component of the symmetric neighbour relation holds at least one
/// node that is **not** in a lake basin -- i.e. at least one outlet to the sea.
///
/// **Not an export**, and the whole of [`wb_water_run`]'s no-rim defence. See that function's
/// doc for the two panics this closes and the measured input that reached the second one.
///
/// The argument, stated once so a later reader does not have to reconstruct it: a node set
/// `U` has no rim exactly when no edge leaves it, which is exactly when `U` is a union of
/// connected components. `water.rs`'s two panics both fire on a rimless union of *lake*
/// basins (one basin in `fill_lakes`, a tied group in `merge_tied_plateaus`). So if no
/// component consists entirely of lake-basin nodes, neither panic can fire, whichever groups
/// the merge forms -- which is why this is checked over components rather than over the
/// basins themselves, and why it does not need to predict the merge.
///
/// `neighbours` is the *directed* relation `stream::sample_nodes` returns; this symmetrises
/// it exactly as `water::fill_and_resolve_water` will, so the two look at the same edges.
/// Conservative in one direction only: it can refuse a graph whose merge would not in fact
/// have formed the offending union, and it can never admit one that would.
fn every_component_has_an_outlet(graph: &StreamGraph, neighbours: &[Vec<u32>]) -> bool {
    let symmetric = water::symmetric_adjacency(neighbours);
    let node_count = symmetric.len();
    let partition = water::basins_of(graph);

    let mut root_is_a_lake = vec![false; node_count];
    for lake in graph.lakes() {
        root_is_a_lake[lake.root_node as usize] = true; // cast-ok: a node index into usize
    }

    let mut seen = vec![false; node_count];
    let mut stack: Vec<u32> = Vec::new();
    for start in 0..node_count {
        if seen[start] {
            continue;
        }
        seen[start] = true;
        stack.push(start as u32); // cast-ok: a node index bounded by the graph's own node count
        let mut has_outlet = false;
        while let Some(node) = stack.pop() {
            if !root_is_a_lake[partition.root_of(node) as usize] { // cast-ok: a node index into usize
                has_outlet = true;
            }
            for &next in &symmetric[node as usize] { // cast-ok: a node index into usize
                if !seen[next as usize] { // cast-ok: a node index into usize
                    seen[next as usize] = true; // cast-ok: a node index into usize
                    stack.push(next);
                }
            }
        }
        if !has_outlet {
            return false;
        }
    }
    true
}

/// Resolve one world's water and write the shipped water manifest.
///
/// Samples a stream graph over `handle`'s surface exactly as [`wb_erosion_run`] does, runs
/// [`crate::water::fill_and_resolve_water`] (fill, overflow resolution, Ruling 7's
/// tied-plateau merge, and pond/lake classification -- the whole shipped path, not a step of
/// it), and writes [`crate::water::water_manifest_from_graph`]'s result.
///
/// # What this does NOT do
///
/// It does not erode first. The graph is built from the world's *own* surface heights, the
/// same field `wb_erosion_run` starts from, so the two exports describe the same planet at
/// the same moment and neither depends on the other having run. It does not store the graph
/// back onto the world, does not populate rivers (`WaterManifest::rivers` is empty at Mark 2
/// by slice 5b's Ruling 2 -- schema only), and changes no arithmetic in `water.rs`.
///
/// # Parameters and their domains
///
/// - `handle`: an existing world from [`wb_world_new`] or [`wb_world_new_relief`].
///   `WB_ERR_HANDLE` if stale or unknown.
/// - `node_count`: `2..=`[`WB_MAX_WATER_NODES`]. See that constant for why its ceiling sits
///   below `wb_erosion_run`'s.
/// - `sea_level_m`: finite, and `abs() <=` the world's own `radius_m`. **A datum outside the
///   planet is not a datum**, and this is the parameter `WB_EROSION_SEA_LEVEL_M`'s doc asks
///   slice 5b to expose rather than hardcode: `StreamGraph::build` flags a node `LAND` or
///   `BOUNDARY` on `height_m > sea_level_m`, a `BOUNDARY` node is a root, and the root set is
///   what separates a mouth from a lake. Moving it moves every body in the manifest.
/// - `pond_max_surface_area_m2`: finite and `>= 0.0`. **No upper bound**, and deliberately:
///   the value is only ever the right-hand side of a `<=` against a summed surface area
///   (`crate::water::classify_lake_kinds`) and never enters arithmetic, so a threshold above
///   the planet's own area is the meaningful statement "every body is a pond" rather than an
///   overflow hazard. NaN *is* refused: every comparison against it is false, so a NaN
///   threshold would silently classify every body as a lake -- a parameter that looks
///   configured and does nothing, which is the shape this project has been bitten by.
///
/// # The rimless-union refusal, which closes two measured aborts
///
/// `water.rs` carries **two** no-rim panics, and a sweep of this export's own parameters
/// reaches both:
///
/// - `crate::water::fill_lakes` panics when one basin's members are the whole graph.
///   Reachable at `node_count = 2`: two nodes above `sea_level_m` are one basin covering
///   everything.
/// - `crate::water::merge_tied_plateaus` panics when the *union* of a tied group's basins is
///   the whole graph. Reached here at `sea_level_m = -1.0e4`, `node_count = 3,000`, seed
///   20260904 -- every node then sits above the datum, so there is no boundary node, no
///   mouth, and no outlet anywhere on the planet. Measured before this guard existed:
///   `merge_tied_plateaus: the union of 157 lakes ... has no rim`, taken through `extern
///   "C"` as `thread caused non-unwinding panic. aborting.`, exit `0xc0000409`. **A band, not
///   a cliff:** `0.0`, `-1.0`, `+1.0e3`, `-1.0e3` and `+1.0e4` all return `WB_OK`.
///
/// `extern "C"` is nounwind, so either would abort the module rather than return a status --
/// in a browser, a dead instance and a blank viewer. Both are closed by
/// [`every_component_has_an_outlet`], which is the exact precondition rather than a proxy: a
/// node set is rimless in the symmetric adjacency exactly when it is a union of that graph's
/// connected components, so if every component holds at least one node outside every lake
/// basin, **no** union of lake basins -- including a single basin, and including whichever
/// tied group the merge happens to form -- can be rimless. Refused as [`WB_ERR_GRAPH`].
///
/// A world with no outlet is not a bug in the caller's arithmetic; it is a datum below the
/// planet's own lowest point, which leaves the water nowhere to go and the manifest with
/// nothing to say. Refusing says so; aborting does not.
///
/// # Output
///
/// `*out_body_count` is how many bodies the manifest holds, and `*out_sea_level_m` is
/// `WaterManifest::sea_level_m` -- the datum, echoed back rather than assumed, because the
/// same graph yields a different manifest at a different one. `out_bodies` receives
/// `body_count * `[`WB_WATER_BODY_STRIDE`] f64 in that constant's documented order.
///
/// **Sizing.** Pass `out_bodies` null with `out_len == 0` for a count-only query: the two
/// scalars are written and no row is. Otherwise `out_len` must be at least
/// `body_count * WB_WATER_BODY_STRIDE`, and a buffer shorter than that is `WB_ERR_BUFFER`
/// with **nothing written anywhere** -- the same all-or-nothing contract `wb_erosion_run`
/// keeps, and the reason the count-only query exists at all. A caller that would rather not
/// pay for the resolution twice can size at `node_count * WB_WATER_BODY_STRIDE`, which is
/// always sufficient: no body holds fewer than one node.
///
/// # Returns
///
/// `WB_OK`, `WB_ERR_HANDLE`, `WB_ERR_PARAM` for a numeric argument outside the domains above,
/// `WB_ERR_BUFFER` for a null, misaligned or short output buffer, or [`WB_ERR_GRAPH`] if the
/// node set could not be sampled or built into a graph, or produced a rimless basin.
///
/// # Safety
/// `out_bodies` must be null, or a live 8-aligned allocation of at least `out_len` f64.
/// `out_body_count` must be a live 4-aligned `u32`, and `out_sea_level_m` a live 8-aligned
/// `f64`.
#[no_mangle]
pub extern "C" fn wb_water_run(
    handle: u32,
    node_count: u32,
    sea_level_m: f64,
    pond_max_surface_area_m2: f64,
    out_bodies: *mut f64,
    out_len: u32,
    out_body_count: *mut u32,
    out_sea_level_m: *mut f64,
) -> u32 {
    if node_count < 2 || node_count > WB_MAX_WATER_NODES {
        return WB_ERR_PARAM;
    }
    if !sea_level_m.is_finite() {
        return WB_ERR_PARAM;
    }
    // Explicit comparisons, never `f64::min`/`f64::max`/`.clamp(` -- `plates.rs::margin_at`'s
    // house form. A NaN threshold is refused by `is_finite` for the reason this function's
    // doc gives, and the negated `>=` is the NaN-safe shape `wb_erosion_run` already uses.
    if !pond_max_surface_area_m2.is_finite() || !(pond_max_surface_area_m2 >= 0.0) {
        return WB_ERR_PARAM;
    }

    if out_body_count.is_null() || out_sea_level_m.is_null() {
        return WB_ERR_BUFFER;
    }
    if (out_body_count as usize) % core::mem::align_of::<u32>() != 0 { // cast-ok: a pointer to an integer for an alignment check, not a float truncation
        return WB_ERR_BUFFER;
    }
    if (out_sea_level_m as usize) % core::mem::align_of::<f64>() != 0 { // cast-ok: a pointer to an integer for an alignment check, not a float truncation
        return WB_ERR_BUFFER;
    }
    let capacity = if out_bodies.is_null() {
        if out_len != 0 {
            // A null pointer with a length is a host that computed a length wrong, not a host
            // asking for the count -- the same distinction `read_relief` draws.
            return WB_ERR_BUFFER;
        }
        0usize
    } else {
        if (out_bodies as usize) % core::mem::align_of::<f64>() != 0 { // cast-ok: a pointer to an integer for an alignment check, not a float truncation
            return WB_ERR_BUFFER;
        }
        match usize::try_from(out_len) {
            Ok(len) => len,
            Err(_) => return WB_ERR_BUFFER,
        }
    };

    let outcome = with_world(handle, |world| {
        let radius_m = world.surface().radius_m;
        if !(sea_level_m.abs() <= radius_m) {
            return Err(WB_ERR_PARAM);
        }
        let world_seed = world.surface().world_seed as u64; // cast-ok: two's-complement reinterpretation, the same one wb_world_new already makes for Noise
        let sampling = sample_nodes(world_seed, node_count, radius_m).ok_or(WB_ERR_GRAPH)?;
        let heights: Vec<f64> =
            sampling.positions.iter().map(|point| world.surface().elevation_m(point, None)).collect();
        let mut graph = StreamGraph::build(
            &BuildParams {
                world_seed,
                radius_m,
                sea_level_m,
                sampling_kind: SamplingKind::Spiral,
                pond_max_surface_area_m2,
            },
            &sampling.positions,
            &heights,
            &sampling.area_m2,
            &sampling.neighbours,
        )
        .map_err(|_| WB_ERR_GRAPH)?;

        // The rimless-union refusal, before anything can panic. See this function's doc.
        if !every_component_has_an_outlet(&graph, &sampling.neighbours) {
            return Err(WB_ERR_GRAPH);
        }

        // `..._with_neighbours`, not `fill_and_resolve_water`: `sample_nodes` above already
        // built this exact k-nearest-neighbour relation, and the regenerating entry point
        // would discard it and search again from the seed. Same seed, same count, same
        // function, so the answer is identical BY CONSTRUCTION -- and the performance profile
        // measured that second search at 57.7% of the dominant cost of this export.
        let basins = water::fill_and_resolve_water_with_neighbours(
            &mut graph,
            pond_max_surface_area_m2,
            &sampling.neighbours,
        );
        Ok(water::water_manifest_from_graph(&graph, &basins))
    });

    let manifest = match outcome {
        None => return WB_ERR_HANDLE,
        Some(Err(code)) => return code,
        Some(Ok(manifest)) => manifest,
    };

    let needed = manifest.bodies.len().saturating_mul(WB_WATER_BODY_STRIDE);
    if !out_bodies.is_null() && capacity < needed {
        // All-or-nothing: nothing is written on a refusal, which is why the count-only query
        // exists. See this function's doc, "Sizing".
        return WB_ERR_BUFFER;
    }

    let body_count = match u32::try_from(manifest.bodies.len()) {
        Ok(body_count) => body_count,
        Err(_) => return WB_ERR_BUFFER,
    };

    if !out_bodies.is_null() {
        for (row, body) in manifest.bodies.iter().enumerate() {
            let kind = match body.kind {
                water::BodyKind::Lake => WB_BODY_KIND_LAKE,
                water::BodyKind::Pond => WB_BODY_KIND_POND,
            };
            let fields = [
                f64::from(body.root_node),
                kind,
                body.level_m,
                body.extent.min_latitude_deg,
                body.extent.max_latitude_deg,
                body.extent.min_longitude_deg,
                body.extent.max_longitude_deg,
            ];
            for (offset, value) in fields.into_iter().enumerate() {
                unsafe { out_bodies.add(row * WB_WATER_BODY_STRIDE + offset).write(value) };
            }
        }
    }
    unsafe {
        *out_body_count = body_count;
        *out_sea_level_m = manifest.sea_level_m;
    }
    WB_OK
}

// ------------------------------------------------------------------- the hydrology channel
//
// Task 10 of `2026-09-10-water-1a-coarse-bake`: the browser door onto `hydrology::bake`. Same
// shape as every other channel in this file -- a flat f64 params record in a documented
// order, a status that refuses the whole record rather than adjusting one field silently --
// plus a second table, because a bake's *output* does not fit in a caller-sized buffer the
// way `wb_water_run`'s does: `hydrology::record::encode` writes a header whose length is not
// known until the bake has run, so this channel is measure-then-copy rather than
// pass-a-buffer-and-hope, exactly like `wb_climate_calibration`'s own count-then-fill shape
// but held across two calls instead of one.

/// f64 words per hydrology params record, **and the order is the contract**:
///
/// | index | field |
/// |---:|---|
/// | 0 | `total_nodes` -- an integer carried as an f64 |
/// | 1 | `wetness_nodes` -- an integer carried as an f64 |
/// | 2 | `keep_depth_m` |
/// | 3 | `keep_area_m2` |
/// | 4 | `pond_max_area_m2` |
/// | 5 | `stream_flow_m2` |
/// | 6 | `river_flow_m2` |
/// | 7 | `great_flow_m2` |
/// | 8 | `notch_fall_m` |
/// | 9 | `evaporation_factor` |
/// | 10 | `salt_flat_share` |
/// | 11 | `forced_count` -- an integer carried as an f64 |
///
/// followed by `forced_count` pairs of `[latitude_deg, longitude_deg]`. **The order is the
/// contract**, the same words every other flat record in this file carries at its own doc.
pub const WB_HYDRO_PARAMS_STRIDE: usize = 12;

/// The ceiling on `total_nodes` and `wetness_nodes` for [`wb_hydro_bake`]. Ruling I7 (final
/// review of water 1a): a measured hazard, not a domain margin -- the studio heap was about
/// 372 MB at 1,000,000 nodes against a 512 MB ceiling, so 1,300,000 is roughly where a bake
/// stops fitting. Spec section 6.1: when a bake does not fit, lower the node count; never raise
/// this ceiling. (It was 4,000,000, which would have aborted the browser tab, not refused.)
pub const WB_MAX_HYDRO_NODES: u32 = 1_300_000;

/// The ceiling on `forced_count` for [`wb_hydro_bake`] -- forced outlets are a handful of
/// owner overrides, never a data set.
pub const WB_MAX_HYDRO_FORCED: u32 = 1_024;

/// Decode a `wb_hydro_bake` params buffer into a `HydroParams`, or `None` if the record is
/// malformed. Every numeric domain check `hydrology::bake` itself would make is left to
/// `bake`; what this function refuses is a record `bake` cannot even be asked about --
/// non-finite words, a non-integral node count or forced-outlet count, a stride that does not
/// match its own declared `forced_count`, a node count above [`WB_MAX_HYDRO_NODES`], or a
/// `forced_count` above [`WB_MAX_HYDRO_FORCED`].
fn hydro_params_from(words: &[f64]) -> Option<HydroParams> {
    let whole = |w: f64| w.is_finite() && w >= 0.0 && m::floor(w) == w;
    if words.len() < WB_HYDRO_PARAMS_STRIDE || words.iter().any(|w| !w.is_finite()) {
        return None;
    }
    if !whole(words[0]) || !whole(words[1]) || !whole(words[11]) {
        return None;
    }
    if words[0] > WB_MAX_HYDRO_NODES as f64 || words[1] > WB_MAX_HYDRO_NODES as f64 { // cast-ok: a ceiling constant widened to f64 for a domain comparison, exact for every u32
        return None;
    }
    // Refused before any cast: an absurd forced_count (e.g. 2^63) would saturate `as usize`
    // and overflow `2 * forced` below -- a panic reachable from extern "C".
    if words[11] > WB_MAX_HYDRO_FORCED as f64 { // cast-ok: a ceiling constant widened to f64 for a domain comparison, exact for every u32
        return None;
    }
    let forced = words[11] as usize; // cast-ok: checked non-negative, integral and <= WB_MAX_HYDRO_FORCED above
    let expected_len = match forced.checked_mul(2).and_then(|doubled| doubled.checked_add(WB_HYDRO_PARAMS_STRIDE)) {
        Some(len) => len,
        None => return None,
    };
    if words.len() != expected_len {
        return None;
    }
    let total = words[0] as u32; // cast-ok: checked integral, non-negative and <= WB_MAX_HYDRO_NODES above
    let wet = words[1] as u32; // cast-ok: checked integral, non-negative and <= WB_MAX_HYDRO_NODES above
    let mut p = HydroParams::earth_like(total);
    p.wetness_nodes = wet;
    p.keep_depth_m = words[2];
    p.keep_area_m2 = words[3];
    p.pond_max_area_m2 = words[4];
    p.stream_flow_m2 = words[5];
    p.river_flow_m2 = words[6];
    p.great_flow_m2 = words[7];
    p.notch_fall_m = words[8];
    p.evaporation_factor = words[9];
    p.salt_flat_share = words[10];
    p.forced_outlets = (0..forced)
        .map(|k| SpherePoint::from_latlon(words[12 + 2 * k], words[13 + 2 * k]))
        .collect();
    Some(p)
}

/// Bake `handle`'s hydrology and hold the encoded record behind a fresh id in the `HYDRO`
/// table, for [`wb_hydro_len`] and [`wb_hydro_copy`] to read and [`wb_hydro_free`] to drop.
///
/// # Parameters
/// `params` is a [`WB_HYDRO_PARAMS_STRIDE`]-word record (plus two words per forced outlet),
/// in the order that constant documents. `params_len` must equal
/// `WB_HYDRO_PARAMS_STRIDE + 2 * forced_count`, every word must be finite, `total_nodes` and
/// `wetness_nodes` must be integral and no larger than [`WB_MAX_HYDRO_NODES`], and
/// `forced_count` must be integral. `params` must be non-null and 8-aligned.
///
/// # Returns
/// `WB_OK` with `*out_id` written to a fresh, never-reused id; `WB_ERR_PARAM` if the record
/// cannot be decoded or `hydrology::bake` refuses its contents (`HydroError::Params`);
/// `WB_ERR_BUFFER` if `params` or `out_id` is null or misaligned, or `params_len` is short;
/// `WB_ERR_HANDLE` if `handle` names no live world; [`WB_ERR_GRAPH`] if `bake` could not
/// sample or build a graph over the surface (`HydroError::Sampling`), or if the routing it
/// built failed the drainage check (`HydroError::Drainage`). `*out_id` is written only on
/// `WB_OK`.
///
/// # Safety
/// `params` must be null or a live, 8-aligned allocation of at least `params_len` f64.
/// `out_id` must be non-null and a live, 4-aligned `u32`.
#[no_mangle]
pub extern "C" fn wb_hydro_bake(handle: u32, params: *const f64, params_len: u32, out_id: *mut u32) -> u32 {
    if out_id.is_null() {
        return WB_ERR_BUFFER;
    }
    if (out_id as usize) % core::mem::align_of::<u32>() != 0 { // cast-ok: a pointer to an integer for an alignment check, not a float truncation
        return WB_ERR_BUFFER;
    }
    if params.is_null() {
        return WB_ERR_BUFFER;
    }
    if (params as usize) % core::mem::align_of::<f64>() != 0 { // cast-ok: a pointer to an integer for an alignment check, not a float truncation
        return WB_ERR_BUFFER;
    }
    let len = match usize::try_from(params_len) {
        Ok(len) => len,
        Err(_) => return WB_ERR_BUFFER,
    };
    if len < WB_HYDRO_PARAMS_STRIDE {
        return WB_ERR_BUFFER;
    }
    let words: &[f64] = unsafe { core::slice::from_raw_parts(params, len) };
    let hydro_params = match hydro_params_from(words) {
        Some(p) => p,
        None => return WB_ERR_PARAM,
    };

    let outcome = with_world(handle, |world| hydrology::bake(world.surface(), &hydro_params));
    let record = match outcome {
        None => return WB_ERR_HANDLE,
        Some(Err(HydroError::Params(_))) => return WB_ERR_PARAM,
        Some(Err(HydroError::Sampling)) => return WB_ERR_GRAPH,
        // Ruling C1-c: a routing that fails the drainage check is a graph the bake could not
        // make drain -- refused as a graph error, never handed out as a record that loses water.
        Some(Err(HydroError::Drainage(_))) => return WB_ERR_GRAPH,
        Some(Ok(record)) => record,
    };

    let encoded = hydrology::record::encode(&record);
    let id = HYDRO.with(|cell| {
        let mut table = cell.borrow_mut();
        table.push(Some(encoded));
        u32::try_from(table.len()).unwrap_or(0)
    });
    if id == 0 {
        // The table is already at u32::MAX entries, so no fresh id can be issued -- there is
        // no dedicated status for "the handle table is full", so this mirrors WORLDS's own
        // convention of collapsing that case into WB_ERR_HANDLE.
        return WB_ERR_HANDLE;
    }
    unsafe { *out_id = id };
    WB_OK
}

/// The length, in f64 words, of the hydro record held under `id` -- or `0` if `id` names no
/// live bake, which doubles as the answer after [`wb_hydro_free`].
#[no_mangle]
pub extern "C" fn wb_hydro_len(id: u32) -> u32 {
    HYDRO.with(|cell| {
        let table = cell.borrow();
        let index = match usize::try_from(id.checked_sub(1).unwrap_or(u32::MAX)) {
            Ok(index) => index,
            Err(_) => return 0,
        };
        table.get(index).and_then(|slot| slot.as_ref()).map(|record| record.len()).and_then(|len| u32::try_from(len).ok()).unwrap_or(0)
    })
}

/// Copy the hydro record held under `id` into `out`, all-or-nothing: `out_len` must be at
/// least the record's own length ([`wb_hydro_len`]), and nothing is written if it is short.
///
/// # Returns
/// `WB_OK` on a full copy; `WB_ERR_BUFFER` if `out` is null, misaligned, or `out_len` is
/// shorter than the record; `WB_ERR_HANDLE` if `id` names no live bake.
///
/// # Safety
/// `out` must be non-null and a live, 8-aligned allocation of at least `out_len` f64.
#[no_mangle]
pub extern "C" fn wb_hydro_copy(id: u32, out: *mut f64, out_len: u32) -> u32 {
    if out.is_null() {
        return WB_ERR_BUFFER;
    }
    if (out as usize) % core::mem::align_of::<f64>() != 0 { // cast-ok: a pointer to an integer for an alignment check, not a float truncation
        return WB_ERR_BUFFER;
    }
    let capacity = match usize::try_from(out_len) {
        Ok(len) => len,
        Err(_) => return WB_ERR_BUFFER,
    };
    HYDRO.with(|cell| {
        let table = cell.borrow();
        let index = match usize::try_from(id.checked_sub(1).unwrap_or(u32::MAX)) {
            Ok(index) => index,
            Err(_) => return WB_ERR_HANDLE,
        };
        let record = match table.get(index).and_then(|slot| slot.as_ref()) {
            Some(record) => record,
            None => return WB_ERR_HANDLE,
        };
        if capacity < record.len() {
            return WB_ERR_BUFFER;
        }
        for (offset, value) in record.iter().enumerate() {
            unsafe { out.add(offset).write(*value) };
        }
        WB_OK
    })
}

/// Drop the hydro record held under `id`. `WB_OK` if it was live, `WB_ERR_HANDLE` if `id`
/// names no live bake -- never issued, or already freed, same as [`wb_world_free`].
#[no_mangle]
pub extern "C" fn wb_hydro_free(id: u32) -> u32 {
    HYDRO.with(|cell| {
        let mut table = cell.borrow_mut();
        let index = match usize::try_from(id.checked_sub(1).unwrap_or(u32::MAX)) {
            Ok(index) => index,
            Err(_) => return WB_ERR_HANDLE,
        };
        match table.get_mut(index) {
            Some(slot @ Some(_)) => {
                *slot = None;
                WB_OK
            }
            _ => WB_ERR_HANDLE,
        }
    })
}
