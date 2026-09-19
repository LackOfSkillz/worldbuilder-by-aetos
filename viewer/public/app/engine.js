//! The engine module, as JavaScript sees it.
//
// `viewer/public/wasm/worldbuilder_engine.wasm` has **zero imports** by design, so
// `WebAssembly.instantiate(bytes, {})` is the entire loader: no wasm-bindgen, no glue
// module, no bundler. Everything below is hand-written marshalling over the thirty-two
// `extern "C"` entry points documented in `crates/worldbuilder-engine/src/wasm.rs`
// (`WB_EXPORTS` is the declared list; a Rust test holds that file's source to it).
//
// Two things about linear memory are load-bearing and easy to get wrong:
//
// 1. **`memory.buffer` detaches when the wasm heap grows.** A `Float32Array` captured
//    before a `wb_alloc` that triggers growth is a view onto a detached buffer and reads
//    as length 0. Every view here is therefore created *after* the allocation it reads,
//    used immediately, and never cached on `this`.
// 2. **`wb_dealloc` is size-aware.** Rust's allocator needs the same byte count back that
//    `wb_alloc` was asked for; a mismatch is undefined behaviour, not a leak. Each helper
//    below frees in a `finally` with the length it asked for.

// The one import: `relief-params.js`, which is itself dependency-free and DOM-free (field
// order, slider travel, the query-string map). It is imported rather than restated because
// the relief record's field ORDER is the ABI, and two copies of an order drift silently --
// there is no type error for a shelf amplitude written into the coast slot.

import { RELIEF_STRIDE, RELIEF_PRESET, toRecord, fromRecord } from "./relief-params.js";
import {
  TECTONIC_STRIDE, TECTONIC_PRESET, tectonicToRecord, tectonicFromRecord,
} from "./tectonic-params.js";
import {
  COAST_STRIDE, COAST_PRESET, coastToRecord, coastFromRecord,
} from "./coast-params.js";
import {
  GULLY_STRIDE, GULLY_PRESET, gullyToRecord, gullyFromRecord,
} from "./gully-params.js";
import {
  PEAK_STRIDE, PEAK_PRESET, peakToRecord, peakFromRecord,
} from "./peak-params.js";
import {
  WATER_STRIDE, WATER_PRESET, waterToRecord, waterFromRecord, hydroParamsWords,
  HYDRO_PARAMS_STRIDE, HYDRO_PARAMS_CARVE_STRIDE,
} from "./water-params.js";

/// Status codes, mirrored from `wasm.rs`. Kept as names so a failure reads as a sentence.
export const WB_OK = 0;
export const WB_ERR_HANDLE = 1;
export const WB_ERR_BUFFER = 2;
export const WB_ERR_GRID = 3;
export const WB_ERR_SUBSTRATE = 4;
export const WB_ERR_PARAM = 5;
/// `wasm.rs`'s sixth status, and it was missing from this table until the water manifest
/// needed it. `wb_water_run` is the first export the viewer calls that can return it, and an
/// unnamed status prints as a bare `6` in the one message that has to say what went wrong.
export const WB_ERR_GRAPH = 6;
/// `wasm.rs`'s seventh status: `wb_hydro_bake` refused a world whose routing did not drain.
export const WB_ERR_DRAINAGE = 7;
/// `wasm.rs`'s eighth status (plan 2b, Task 2): `wb_water_at` or `wb_water_tile` was handed a
/// bake made from other ground than the world it was asked through -- the record's ground
/// fingerprint and the world's differ. Nothing is malformed; the pairing is wrong, and the fix is
/// to re-bake on this world, not to change a parameter. Named here so it reads as that sentence
/// and not as a bare `8` a caller would swallow as "engine unavailable".
export const WB_ERR_WRONG_WORLD = 8;
/// `wasm.rs`'s ninth status (plan 2b, Task 5): `wb_world_new_water` was handed a held bake that
/// was not baked FOR CARVING -- an ordinary record, SCHEMA 7. Such a record keeps the ponds its
/// own channels drain, and carving with it would stand a dam across a river. The world and the
/// bake may both be right; the fix is to re-bake for carving, which this name says and a bare
/// `9` would not.
export const WB_ERR_NOT_BAKED_FOR_CARVING = 9;
/// `wasm.rs`'s tenth status (plan 2b, Task 5; Rulings C-1 and C-24): a bake, an erosion run or a
/// water run was asked of a CARVED world. Each computes from the ground, and a carved world's
/// ground was cut from a record, so the question belongs to the bare world built from the same
/// parameters without the water block.
export const WB_ERR_CARVED = 10;
/// `wasm.rs`'s eleventh status (Ruling C-36): `wb_water_at` or `wb_water_tile` was asked about a
/// CARVED world through a bake other than the one it was carved from -- an ordinary bake of the
/// same ground, or a second carving bake of it. The ground check cannot see this (a carved world
/// fingerprints as its bare parent), and the answer would come from a record the terrain was not
/// cut from. The fix is to query through the carving bake the world holds.
export const WB_ERR_NOT_CARVED_FROM = 11;

const STATUS_NAMES = {
  0: "WB_OK",
  1: "WB_ERR_HANDLE",
  2: "WB_ERR_BUFFER",
  3: "WB_ERR_GRID",
  4: "WB_ERR_SUBSTRATE",
  5: "WB_ERR_PARAM",
  6: "WB_ERR_GRAPH",
  7: "WB_ERR_DRAINAGE",
  8: "WB_ERR_WRONG_WORLD",
  9: "WB_ERR_NOT_BAKED_FOR_CARVING",
  10: "WB_ERR_CARVED",
  11: "WB_ERR_NOT_CARVED_FROM",
};

/// Feature record codes, mirrored from `wasm.rs`. A record is eight f64.
export const WB_FEATURE_STRIDE = 8;
export const COMPOSE = { raise: 0, carve: 1, shape: 2 };
export const SUBSTRATE = { derive: 0, sand: 1, mud: 2, rock: 3 };

/// The `resolution_m` sentinel: anything non-positive or non-finite means canonical ground
/// truth (the engine's `None`). `-1` is the spelling used here so it is obviously deliberate
/// rather than an uninitialised variable that happened to be zero.
export const CANONICAL_RESOLUTION = -1;

/// f64 per body row `wb_water_run` writes, and the field order is the ABI. Mirrored from
/// `WB_WATER_BODY_STRIDE`; `waterRun` below is the only place the order is spelled out, for
/// the same reason `RELIEF_STRIDE` is imported rather than restated -- there is no type error
/// for a latitude written into the level slot.
export const WB_WATER_BODY_STRIDE = 7;

/// `kind` codes in a body row. **The viewer reads neither**: it draws every body at its own
/// level, and `pond` is a label this mesh never produces (see `waterRun`). They are named so
/// a diagnostic can print a word, and so the fact that only one is reachable is stated where
/// somebody would otherwise reintroduce a branch for the other.
export const WB_BODY_KIND = { lake: 0, pond: 1 };

/// The export's own ceiling on `node_count`, mirrored so a slider's travel can be bounded by
/// it rather than by a guess. Chosen in `wasm.rs` **below** the erosion ceiling for a memory
/// reason: the water path holds three neighbour structures at once and wasm32's linear memory
/// is far smaller than the native heap that ceiling would otherwise be sized against.
export const WB_MAX_WATER_NODES = 100000;

/// f32 per sample `wb_climate_tile_f32` writes, and **the order is the ABI**: index 0 is the
/// temperature at the DATUM (not at the ground) and index 1 is the marched moisture. Mirrored
/// from `WB_CLIMATE_STRIDE`; `climateTileF32` below is the only place the order is spelled
/// out, for the same reason `RELIEF_STRIDE` is imported rather than restated.
export const WB_CLIMATE_STRIDE = 2;

/// f64 `wb_climate_calibration` writes: four moisture edges, two landform edges, the land
/// sample count, the lapse rate. Mirrored from `WB_CLIMATE_CALIBRATION_STRIDE`.
export const WB_CLIMATE_CALIBRATION_STRIDE = 8;

/// The `march_samples` sentinel meaning "the engine's own canonical march". **Not 0**: the
/// engine deliberately admits a zero-step march as the identity element (the air has
/// travelled nowhere and is still saturated), so 0 is a real request and cannot double as a
/// default. Mirrored from `WB_CLIMATE_CANONICAL_SAMPLES`.
export const WB_CLIMATE_CANONICAL_SAMPLES = 0xffffffff;

/// The export's own ceiling on `march_samples`, mirrored so a caller can be bounded by it
/// rather than by a guess. **It is a loop bound**: the march walks this many steps inside one
/// uninterruptible call, per sample, and a tile is `width * height` samples.
export const WB_MAX_CLIMATE_MARCH_SAMPLES = 1024;

/// f64 per `wb_hydro_bake` params record before the forced-outlet pairs, and **the order is
/// the ABI**: `totalNodes`, `wetnessNodes`, `keepDepthM`, `keepAreaM2`, `pondMaxAreaM2`,
/// `streamFlowM2`, `riverFlowM2`, `greatFlowM2`, `notchFallM`, `evaporationFactor`,
/// `saltFlatShare`, `forcedCount`, followed by `forcedCount` pairs of
/// `[latitudeDeg, longitudeDeg]`. Mirrored from `WB_HYDRO_PARAMS_STRIDE`.
export const WB_HYDRO_PARAMS_STRIDE = HYDRO_PARAMS_STRIDE;

/// The same record **for a bake for carving**: the twelve words above, then word 12 =
/// `drain_for_carve`, exactly 1, then the forced-outlet pairs. Mirrored from
/// `WB_HYDRO_PARAMS_CARVE_STRIDE`; the two layouts are told apart by the length's parity, so no
/// tag word exists. `hydroHold` writes it when `params.forCarving` is true, through
/// `water-params.js`'s `hydroParamsWords`, which is the one place either layout is laid out.
export const WB_HYDRO_PARAMS_CARVE_STRIDE = HYDRO_PARAMS_CARVE_STRIDE;

/// The export's own ceiling on `totalNodes` and `wetnessNodes`, mirrored from
/// `WB_MAX_HYDRO_NODES` (1.3M since the water 1a final review: the measured heap at 1M is
/// about 372 MB of the 512 MB ceiling -- lower the count, never raise the ceiling).
export const WB_MAX_HYDRO_NODES = 1300000;

/// f64 per water sample `wb_water_at` and `wb_water_tile` write, and **the order is the ABI**:
/// `[kind, levelM, depthM, bodyId, reachId]`. Mirrored from `WB_WATER_STRIDE`.
///
/// **Five and not four.** The plan's first cut wrote four -- Ruling Q-8 -- and Ruling Q-18
/// widened it to carry the reach id, because §9.1 tints a river by its **class** and a class
/// lives on the reach: without the id a drawing path would have to re-run the whole query to
/// find out which reach had answered. Any reader of a tile buffer strides by this constant, so
/// a copy of the number that says 4 reads every sample after the first from the wrong offset.
export const WB_WATER_STRIDE = 5;

/// `kind` codes in a water sample, mirrored from `WB_WATER_STRIDE`'s own table in `wasm.rs`.
export const WB_WATER_KIND = {
  none: 0, ocean: 1, lake: 2, saltLake: 3, saltFlat: 4, pond: 5, river: 6,
};

/// `bodyId` when the answer belongs to no recorded body (ocean, river, none), and `reachId`
/// when it belongs to no recorded reach (everything but a river -- and a river through a notch no
/// reach claims, which is how a sample says "a notch": Ruling C-35). `u32::MAX`, mirrored from
/// `water::NO_BODY` and `water::NO_REACH` -- one value, two names, because the two words index
/// different tables.
export const WB_NO_BODY = 0xffffffff;
export const WB_NO_REACH = 0xffffffff;

export class Engine {
  constructor(instance) {
    this.instance = instance;
    this.exports = instance.exports;
    this.memory = this.exports.memory;
  }

  /// Fetch and instantiate. `instantiateStreaming` needs `application/wasm`, which
  /// `scripts/serve.mjs` already sends; the `arrayBuffer` path is the fallback for any
  /// host that does not.
  static async load(url = "/wasm/worldbuilder_engine.wasm") {
    const response = await fetch(url);
    if (!response.ok) {
      throw new Error(`engine wasm: ${response.status} ${response.statusText} for ${url}`);
    }
    let instance;
    try {
      ({ instance } = await WebAssembly.instantiateStreaming(response.clone(), {}));
    } catch (_) {
      const bytes = await response.arrayBuffer();
      ({ instance } = await WebAssembly.instantiate(bytes, {}));
    }
    const engine = new Engine(instance);
    // A module that exports only `memory` is this project's original failure mode, and it
    // is a green build. Refuse it here too rather than discovering it as a TypeError three
    // frames later.
    for (const name of [
      "wb_generator_version", "wb_alloc", "wb_dealloc", "wb_world_new", "wb_world_new_relief",
      "wb_relief_preset", "wb_relief_check",
      "wb_world_new_tectonic", "wb_tectonic_preset", "wb_tectonic_check",
      "wb_world_new_coast", "wb_coast_preset", "wb_coast_check",
      "wb_world_new_gully", "wb_gully_preset", "wb_gully_check",
      "wb_world_new_peak", "wb_peak_preset", "wb_peak_check",
      "wb_world_new_water", "wb_water_preset", "wb_water_check", "wb_world_free",
      "wb_world_count", "wb_elevation_m", "wb_structural_m", "wb_bottom_at",
      "wb_fill_tile_f32", "wb_water_run",
      "wb_hydro_bake", "wb_hydro_len", "wb_hydro_copy", "wb_hydro_free",
      "wb_water_at", "wb_water_tile",
    ]) {
      if (typeof engine.exports[name] !== "function") {
        throw new Error(`engine wasm is missing export ${name}`);
      }
    }
    return engine;
  }

  generatorVersion() {
    return this.exports.wb_generator_version() >>> 0;
  }

  worldCount() {
    return this.exports.wb_world_count() >>> 0;
  }

  /// Build a world and return its handle. Throws on refusal, because a handle of 0 used as
  /// a handle answers NaN at every point rather than failing.
  ///
  /// `features` is an array of `{ latitudeDeg, longitudeDeg, targetM, lengthM, widthM,
  /// bearingDeg, compose, substrate }`. The engine refuses the *whole* call if any record
  /// fails to decode — a world built from five of six requested features is the
  /// silently-dropping-builder shape, and the refusal is deliberate.
  /// The ten f64 of a named preset, as an object keyed by `RELIEF_FIELDS`.
  ///
  /// **The only way the viewer learns a relief number.** Nothing in `viewer/` restates
  /// `canonical()`'s or `hills()`'s values; the panel's slider defaults, both ends of two of
  /// its three sliders and its preset button all come from here, so `detail.rs` stays the
  /// single place those numbers live. `name` is a key of `RELIEF_PRESET`.
  reliefPreset(name = "canonical") {
    const selector = RELIEF_PRESET[name];
    if (selector === undefined) throw new Error(`unknown relief preset "${name}"`);
    const bytes = RELIEF_STRIDE * 8;
    const ptr = this.exports.wb_alloc(bytes);
    if (ptr === 0) throw new Error("wb_alloc refused the relief preset buffer");
    try {
      const status = this.exports.wb_relief_preset(selector, ptr, RELIEF_STRIDE) >>> 0;
      if (status !== WB_OK) {
        throw new Error(`wb_relief_preset(${name}) returned ${statusName(status)}`);
      }
      // The view is created after the allocation and copied immediately — a view taken
      // before `wb_alloc` could be detached by heap growth.
      return fromRecord(Array.from(new Float64Array(this.memory.buffer, ptr, RELIEF_STRIDE)));
    } finally {
      this.exports.wb_dealloc(ptr, bytes);
    }
  }

  /// Ask the engine whether a relief block would be accepted, **without building a world**.
  /// Returns a `WB_*` status: `WB_OK`, `WB_ERR_PARAM` for a field outside its domain, or
  /// `WB_ERR_BUFFER`. `null` is the canonical path and always answers `WB_OK`.
  ///
  /// A refused world comes back from `wb_world_new_relief` as a handle of 0, which says
  /// *that* it refused and never *why*; the panel calls this so it can say which.
  checkRelief(relief) {
    if (relief === null || relief === undefined) return WB_OK;
    const bytes = RELIEF_STRIDE * 8;
    const ptr = this.exports.wb_alloc(bytes);
    if (ptr === 0) throw new Error("wb_alloc refused the relief buffer");
    try {
      new Float64Array(this.memory.buffer, ptr, RELIEF_STRIDE).set(toRecord(relief));
      return this.exports.wb_relief_check(ptr, RELIEF_STRIDE) >>> 0;
    } finally {
      this.exports.wb_dealloc(ptr, bytes);
    }
  }

  /// The nine f64 of `TectonicParams::canonical()`, as an object keyed by `TECTONIC_FIELDS`.
  ///
  /// **The only way the viewer learns a tectonic number.** Nothing in `viewer/` restates
  /// 1500, 400000 or 0.45; all three of the panel's mountain sliders are anchored here, so
  /// `tectonics.rs` stays the single place those numbers live. `name` is a key of
  /// `TECTONIC_PRESET`, and there is only one -- a *named* tectonic preset is Task 3's
  /// decision, not this task's.
  tectonicPreset(name = "canonical") {
    const selector = TECTONIC_PRESET[name];
    if (selector === undefined) throw new Error(`unknown tectonic preset "${name}"`);
    const bytes = TECTONIC_STRIDE * 8;
    const ptr = this.exports.wb_alloc(bytes);
    if (ptr === 0) throw new Error("wb_alloc refused the tectonic preset buffer");
    try {
      const status = this.exports.wb_tectonic_preset(selector, ptr, TECTONIC_STRIDE) >>> 0;
      if (status !== WB_OK) {
        throw new Error(`wb_tectonic_preset(${name}) returned ${statusName(status)}`);
      }
      // The view is created after the allocation and copied immediately — a view taken
      // before `wb_alloc` could be detached by heap growth.
      return tectonicFromRecord(
        Array.from(new Float64Array(this.memory.buffer, ptr, TECTONIC_STRIDE)),
      );
    } finally {
      this.exports.wb_dealloc(ptr, bytes);
    }
  }

  /// Ask the engine whether a tectonic block would be accepted, **without building a world**.
  /// Returns a `WB_*` status. `null` is the canonical path and always answers `WB_OK`.
  checkTectonic(tectonics) {
    if (tectonics === null || tectonics === undefined) return WB_OK;
    const bytes = TECTONIC_STRIDE * 8;
    const ptr = this.exports.wb_alloc(bytes);
    if (ptr === 0) throw new Error("wb_alloc refused the tectonic buffer");
    try {
      new Float64Array(this.memory.buffer, ptr, TECTONIC_STRIDE).set(tectonicToRecord(tectonics));
      return this.exports.wb_tectonic_check(ptr, TECTONIC_STRIDE) >>> 0;
    } finally {
      this.exports.wb_dealloc(ptr, bytes);
    }
  }

  /// The six f64 of a named coast preset, as an object keyed by `COAST_FIELDS`.
  ///
  /// **The only way the viewer learns a coast number.** Nothing in `viewer/` restates 0.35, 20, 4,
  /// 0.5 or 2; the amplitude slider is anchored here and the preset button sends this answer
  /// straight back, so `continentality.rs` stays the single place those numbers live. `name` is a
  /// key of `COAST_PRESET`.
  coastPreset(name = "canonical") {
    const selector = COAST_PRESET[name];
    if (selector === undefined) throw new Error(`unknown coast preset "${name}"`);
    const bytes = COAST_STRIDE * 8;
    const ptr = this.exports.wb_alloc(bytes);
    if (ptr === 0) throw new Error("wb_alloc refused the coast preset buffer");
    try {
      const status = this.exports.wb_coast_preset(selector, ptr, COAST_STRIDE) >>> 0;
      if (status !== WB_OK) {
        throw new Error(`wb_coast_preset(${name}) returned ${statusName(status)}`);
      }
      // The view is created after the allocation and copied immediately — a view taken before
      // `wb_alloc` could be detached by heap growth.
      return coastFromRecord(Array.from(new Float64Array(this.memory.buffer, ptr, COAST_STRIDE)));
    } finally {
      this.exports.wb_dealloc(ptr, bytes);
    }
  }

  /// Ask the engine whether a coast block would be accepted, **without building a world**.
  /// Returns a `WB_*` status. `null` is the canonical path and always answers `WB_OK`.
  ///
  /// Two of this channel's bounds are not politeness: the octave count is a per-sample loop bound
  /// (a hung tab, not a slow world) and the finest-octave frequency is a PRODUCT of three fields
  /// that are each individually admissible. A panel that could only report "the engine said no"
  /// would push the owner into bisecting six fields by hand.
  checkCoast(coast) {
    if (coast === null || coast === undefined) return WB_OK;
    const bytes = COAST_STRIDE * 8;
    const ptr = this.exports.wb_alloc(bytes);
    if (ptr === 0) throw new Error("wb_alloc refused the coast buffer");
    try {
      new Float64Array(this.memory.buffer, ptr, COAST_STRIDE).set(coastToRecord(coast));
      return this.exports.wb_coast_check(ptr, COAST_STRIDE) >>> 0;
    } finally {
      this.exports.wb_dealloc(ptr, bytes);
    }
  }

  /// The ten f64 of a named gully preset, as an object keyed by `GULLY_FIELDS`.
  ///
  /// **The only way the viewer learns a gully number**, and it matters more on this channel than
  /// on any before it: `slope_reference` is 0.005 m/m, a *measurement of this generator's flanks*
  /// rather than a preference, and a viewer that transcribed it would be a second copy of a
  /// measurement. `name` is a key of `GULLY_PRESET`.
  gullyPreset(name = "canonical") {
    const selector = GULLY_PRESET[name];
    if (selector === undefined) throw new Error(`unknown gully preset "${name}"`);
    const bytes = GULLY_STRIDE * 8;
    const ptr = this.exports.wb_alloc(bytes);
    if (ptr === 0) throw new Error("wb_alloc refused the gully preset buffer");
    try {
      const status = this.exports.wb_gully_preset(selector, ptr, GULLY_STRIDE) >>> 0;
      if (status !== WB_OK) {
        throw new Error(`wb_gully_preset(${name}) returned ${statusName(status)}`);
      }
      // The view is created after the allocation and copied immediately — a view taken before
      // `wb_alloc` could be detached by heap growth.
      return gullyFromRecord(Array.from(new Float64Array(this.memory.buffer, ptr, GULLY_STRIDE)));
    } finally {
      this.exports.wb_dealloc(ptr, bytes);
    }
  }

  /// Ask the engine whether a gully block would be accepted, **without building a world**.
  /// Returns a `WB_*` status. `null` is the canonical path and always answers `WB_OK`.
  ///
  /// One of this channel's bounds closes a real hazard rather than stating a domain: the edge
  /// shaping is `1 - 2 * folded^crest_sharpness` and `folded` is exactly zero at a crest, so a
  /// single non-positive f64 in word 5 would turn **every gully crest in the world into an
  /// infinite height** and hand it across a nounwind boundary into a vertex buffer. And three
  /// fields are jointly constrained -- `cell_m`, `steer_lattice_m` and `slope_reference` all
  /// become a lattice index as `radius / length` -- so the panel asks the real validator rather
  /// than re-deriving the quotient in JavaScript.
  checkGully(gully) {
    if (gully === null || gully === undefined) return WB_OK;
    const bytes = GULLY_STRIDE * 8;
    const ptr = this.exports.wb_alloc(bytes);
    if (ptr === 0) throw new Error("wb_alloc refused the gully buffer");
    try {
      new Float64Array(this.memory.buffer, ptr, GULLY_STRIDE).set(gullyToRecord(gully));
      return this.exports.wb_gully_check(ptr, GULLY_STRIDE) >>> 0;
    } finally {
      this.exports.wb_dealloc(ptr, bytes);
    }
  }

  /// The five f64 of a named peak preset, as an object keyed by `PEAK_FIELDS`.
  ///
  /// **The only way the viewer learns a peak number, and this comment does not restate one.**
  /// Nothing in `viewer/` writes any of the preset's five values down — an earlier version of
  /// this comment listed them, and its density had to be hand-edited every time the constant was
  /// calibrated, which is the transcription the design prevents everywhere except in prose. The
  /// density slider is anchored here and the preset button sends this answer straight back, so
  /// `tectonics.rs` stays the single place those numbers live. `name` is a key of
  /// `PEAK_PRESET`.
  peakPreset(name = "canonical") {
    const selector = PEAK_PRESET[name];
    if (selector === undefined) throw new Error(`unknown peak preset "${name}"`);
    const bytes = PEAK_STRIDE * 8;
    const ptr = this.exports.wb_alloc(bytes);
    if (ptr === 0) throw new Error("wb_alloc refused the peak preset buffer");
    try {
      const status = this.exports.wb_peak_preset(selector, ptr, PEAK_STRIDE) >>> 0;
      if (status !== WB_OK) {
        throw new Error(`wb_peak_preset(${name}) returned ${statusName(status)}`);
      }
      // The view is created after the allocation and copied immediately — a view taken before
      // `wb_alloc` could be detached by heap growth.
      return peakFromRecord(Array.from(new Float64Array(this.memory.buffer, ptr, PEAK_STRIDE)));
    } finally {
      this.exports.wb_dealloc(ptr, bytes);
    }
  }

  /// Ask the engine whether a peak block would be accepted, **without building a world**.
  /// Returns a `WB_*` status. `null` is the canonical path and always answers `WB_OK`.
  ///
  /// One of this channel's bounds is joint rather than per-field: `reach_m <= lattice_m` is what
  /// keeps `peak_of_cell`'s 3x3x3 candidate scan complete, and neither field's own domain check
  /// can see it. The panel asks the real validator rather than re-deriving the comparison in
  /// JavaScript, the same posture `checkCoast` and `checkGully` both take on their own joint
  /// bounds.
  checkPeak(peak) {
    if (peak === null || peak === undefined) return WB_OK;
    const bytes = PEAK_STRIDE * 8;
    const ptr = this.exports.wb_alloc(bytes);
    if (ptr === 0) throw new Error("wb_alloc refused the peak buffer");
    try {
      new Float64Array(this.memory.buffer, ptr, PEAK_STRIDE).set(peakToRecord(peak));
      return this.exports.wb_peak_check(ptr, PEAK_STRIDE) >>> 0;
    } finally {
      this.exports.wb_dealloc(ptr, bytes);
    }
  }

  /// The water block of a named preset, as an object keyed by `WATER_FIELDS`.
  ///
  /// **The only way the viewer learns a water number, and this comment does not restate one.**
  /// The bank-width slider starts here when the carve is first turned on, so `water/layer.rs`
  /// stays the single place the canonical width is written. `name` is a key of `WATER_PRESET`.
  waterPreset(name = "canonical") {
    const selector = WATER_PRESET[name];
    if (selector === undefined) throw new Error(`unknown water preset "${name}"`);
    const bytes = WATER_STRIDE * 8;
    const ptr = this.exports.wb_alloc(bytes);
    if (ptr === 0) throw new Error("wb_alloc refused the water preset buffer");
    try {
      const status = this.exports.wb_water_preset(selector, ptr, WATER_STRIDE) >>> 0;
      if (status !== WB_OK) {
        throw new Error(`wb_water_preset(${name}) returned ${statusName(status)}`);
      }
      return waterFromRecord(Array.from(new Float64Array(this.memory.buffer, ptr, WATER_STRIDE)));
    } finally {
      this.exports.wb_dealloc(ptr, bytes);
    }
  }

  /// `relief` is `null`/absent for the canonical path — a null pointer and a length of zero,
  /// which is the same `None` `wb_world_new` passes and is byte-for-byte today's world. An
  /// object keyed by `RELIEF_FIELDS` asks for a different one.
  /// `tectonics` is the same story: `null`/absent is `None` -- Ruling 1 of the mountains
  /// slice, which is that the default world cannot move -- and an object keyed by
  /// `TECTONIC_FIELDS` asks for a different uplift.
  /// `coast` is the same story again: `null`/absent is `None` -- RULING 1 of the fractal-coastline
  /// slice, which is that the default coastline cannot move -- and an object keyed by
  /// `COAST_FIELDS` asks for a roughened one.
  /// `gully` is the fifth: `null`/absent is `None`, which in this channel means
  /// `Surface::with_gully` builds **no steering lattice at all** and `elevation_m` takes a
  /// different branch -- so the canonical path is structurally the old one rather than the new one
  /// plus zero. An object keyed by `GULLY_FIELDS` asks for the drainage texture.
  /// `peaks` is the sixth: `null`/absent is `None`, and on this channel that means
  /// `Tectonics::peak_offset_m` returns 0.0 on its very first line rather than evaluating the
  /// term at all. An object keyed by `PEAK_FIELDS` asks for the seamount field.
  /// `water` is the seventh, and **it is not a surface block like the six above**: it carves the
  /// channels of a HELD BAKE into the ground (plan 2b, Ruling C-1's second phase), so it travels
  /// with `bake`, the `{ id, handle, words }` object `hydroHold` handed back -- baked on the bare
  /// world of this same spec, **for carving**. `null`/absent for both is the uncarved path: a null
  /// block, a length of zero and a bake id of 0, which `wb_world_new_water` builds as the bare
  /// world, bit-identical to `wb_world_new_peak`'s. A block without a bake, or a bake without a
  /// block, is a caller bug and is refused by the engine rather than guessed at here.
  ///
  /// **A refused world throws an `Error` with `.status`**, the named status the handle of 0 could
  /// not carry. With a water block in the call that status is asked of `wb_water_check` -- which
  /// costs a whole world build (and a fingerprint), so it is asked here, **after** a refusal, and
  /// never before a build (Task 5's carry-forward). Without one, the six older checkers name the
  /// block they can judge alone, as before.
  newWorld(spec) {
    const { seed, radiusM, plateCount, landFraction, features = [], water = null, bake = null } = spec;
    return this.worldCall(spec, (args) => {
      // ONE constructor for all paths, and `wb_world_new_water` is now it. With all six blocks
      // null and no bake this is six `None`s -- the world `wb_world_new` builds, which the
      // engine-side tests pin bit for bit, including Task 5's
      // `a_canonical_block_over_a_record_with_no_reaches_is_the_untouched_world` and the door's
      // own null-block path. Calling the widest door unconditionally rather than choosing is
      // deliberate, as it was when `wb_world_new_peak` became the door: a branch here would send
      // the default path and the carved path through different exports, and the byte-identity
      // the digest control holds would stop covering what the viewer actually calls.
      const handle = this.exports.wb_world_new_water(...args) >>> 0;
      if (handle !== 0) return handle;
      // A refused world is a blank viewer, so the message has to name the reason.
      let status = null;
      let why = "";
      if (water !== null || bake !== null) {
        status = this.exports.wb_water_check(...args) >>> 0;
        why = ` water=${statusName(status)}`;
      } else {
        why =
          (spec.relief ? ` relief=${statusName(this.checkRelief(spec.relief))}` : "") +
          (spec.tectonics ? ` tectonics=${statusName(this.checkTectonic(spec.tectonics))}` : "") +
          (spec.coast ? ` coast=${statusName(this.checkCoast(spec.coast))}` : "") +
          (spec.gully ? ` gully=${statusName(this.checkGully(spec.gully))}` : "") +
          (spec.peaks ? ` peaks=${statusName(this.checkPeak(spec.peaks))}` : "");
      }
      const error = new Error(
        `wb_world_new_water refused seed=${seed} radius=${radiusM} plates=${plateCount} ` +
        `land=${landFraction} features=${features.length}${why}`,
      );
      error.status = status;
      throw error;
    });
  }

  /// Ask whether `newWorld(spec)` would build, **and if not, why**: `wb_water_check` over exactly
  /// the arguments `newWorld` would send. Returns a `WB_*` status.
  ///
  /// **It costs what the constructor costs** -- a world build, plus a fingerprint for a carve --
  /// because whether a record belongs to a world cannot be judged without the world. So it is not
  /// the peak channel's cheap `checkPeak`: call it after a refusal, or once before a bake that
  /// would otherwise be wasted, **never on a slider drag**. A malformed block is judged before the
  /// world is looked at, so `bake: null` with a block answers `WB_ERR_PARAM` at no build cost for a
  /// malformed block, and `WB_ERR_HANDLE` for an admissible one -- **also at no build cost**: the
  /// bake id is resolved (`wasm.rs::held_bake`) before any `Surface` is constructed
  /// (`wasm.rs::build_surface`), so a missing bake is refused before a world is built.
  checkWater(spec) {
    return this.worldCall(spec, (args) => this.exports.wb_water_check(...args) >>> 0);
  }

  /// Lay a world spec out in linear memory as `wb_world_new_water`'s nineteen arguments, hand them
  /// to `call`, and free every buffer on the way out whatever `call` did. The one marshaller for
  /// the constructor and its checker, so the two cannot be handed different arguments.
  worldCall({
    seed, radiusM, plateCount, landFraction, features = [], relief = null, tectonics = null,
    coast = null, gully = null, peaks = null, water = null, bake = null,
  }, call) {
    const held = [];
    const place = (words, what) => {
      const bytes = words.length * 8;
      const ptr = this.exports.wb_alloc(bytes);
      if (ptr === 0) throw new Error(`wb_alloc refused the ${what} buffer`);
      held.push([ptr, bytes]);
      // The view is created after the allocation and used at once -- the module doc's rule 1.
      new Float64Array(this.memory.buffer, ptr, words.length).set(words);
      return ptr;
    };
    try {
      const block = (value, stride, toRecord, what) =>
        (value ? [place(toRecord(value), what), stride] : [0, 0]);
      const featureWords = [];
      features.forEach((f) => {
        featureWords.push(
          f.latitudeDeg, f.longitudeDeg, f.targetM, f.lengthM, f.widthM, f.bearingDeg,
          COMPOSE[f.compose] ?? f.compose, SUBSTRATE[f.substrate ?? "derive"] ?? f.substrate,
        );
      });
      const featurePtr = features.length > 0 ? place(featureWords, "feature") : 0;
      const args = [
        BigInt(seed), radiusM, plateCount, landFraction, featurePtr, features.length,
        ...block(relief, RELIEF_STRIDE, toRecord, "relief"),
        ...block(tectonics, TECTONIC_STRIDE, tectonicToRecord, "tectonic"),
        ...block(coast, COAST_STRIDE, coastToRecord, "coast"),
        ...block(gully, GULLY_STRIDE, gullyToRecord, "gully"),
        ...block(peaks, PEAK_STRIDE, peakToRecord, "peak"),
        ...block(water, WATER_STRIDE, waterToRecord, "water"),
        // The held bake's id, or 0 -- Ruling C-2: the record is referenced, never copied.
        bake === null ? 0 : bake.id,
      ];
      return call(args);
    } finally {
      for (const [ptr, bytes] of held) this.exports.wb_dealloc(ptr, bytes);
    }
  }

  freeWorld(handle) {
    return this.exports.wb_world_free(handle) >>> 0;
  }

  /// Metres above datum. NaN means the handle is unknown — a value no valid world produces
  /// at a valid point, which is why this one needs no out-parameter.
  elevationM(handle, latitudeDeg, longitudeDeg, resolutionM = CANONICAL_RESOLUTION) {
    return this.exports.wb_elevation_m(handle, latitudeDeg, longitudeDeg, resolutionM);
  }

  structuralM(handle, latitudeDeg, longitudeDeg) {
    return this.exports.wb_structural_m(handle, latitudeDeg, longitudeDeg);
  }

  /// Fill one heightmap tile and hand back a **copy** as a `Float32Array` on the JS heap.
  ///
  /// The copy is not laziness: the wasm view has to die before the buffer is freed, and
  /// `HeightmapTerrainData` keeps its buffer for the lifetime of the tile (and transfers it
  /// to a worker during upsampling), so it cannot be a window onto linear memory.
  ///
  /// **`lat0Deg` is the row-0 latitude.** Cesium's heightmap convention puts row 0 at the
  /// *north* edge — `HeightmapTerrainData`'s own `interpolateHeight` reads
  /// `height - 1 - southInteger`, so the caller passes north first. The engine bakes in no
  /// hemisphere; getting this backwards is a silently upside-down planet, so the caller
  /// names the fields rather than passing four bare numbers.
  fillTileF32({ handle, lat0Deg, lat1Deg, lon0Deg, lon1Deg, width, height, resolutionM }) {
    const samples = width * height;
    const bytes = samples * 4;
    const ptr = this.exports.wb_alloc(bytes);
    if (ptr === 0) throw new Error(`wb_alloc refused ${bytes} bytes for a ${width}x${height} tile`);
    try {
      const status = this.exports.wb_fill_tile_f32(
        handle, lat0Deg, lat1Deg, lon0Deg, lon1Deg, width, height, resolutionM, ptr, samples,
      ) >>> 0;
      if (status !== WB_OK) {
        throw new Error(`wb_fill_tile_f32 returned ${STATUS_NAMES[status] ?? status}`);
      }
      // The view is created here, after the allocation, and copied immediately: a view
      // taken before wb_alloc could be detached by heap growth.
      return new Float32Array(this.memory.buffer, ptr, samples).slice();
    } finally {
      this.exports.wb_dealloc(ptr, bytes);
    }
  }

  /// Fill one **climate** tile and hand back a copy as a `Float32Array` on the JS heap: two
  /// f32 per sample, `[temperatureCAtDatum, moisture]`, row-major, both endpoints included.
  ///
  /// The copy is taken for the same reason `fillTileF32` takes one -- the wasm view has to die
  /// before the buffer is freed -- and the grid convention is byte-for-byte
  /// `wb_fill_tile_f32`'s, deliberately, so a caller can lay this grid over a height grid
  /// without a second convention to get wrong. `lat0Deg` is the row-0 latitude.
  ///
  /// **The temperature is at the DATUM.** Apply the lapse rate yourself, against the heights
  /// you already have at your own raster's resolution; `climateCalibration` reports the rate.
  /// See `WB_CLIMATE_STRIDE` in `wasm.rs` for why the engine does not do it for you.
  ///
  /// **One sample is one moisture march**, which is 161 elevation queries at the canonical
  /// budget. Texels are quadratic in the raster edge and samples are linear in the budget, so
  /// the raster is the lever. `biome.js::CLIMATE_RASTER` is the size this viewer ships and the
  /// measurement that chose it.
  climateTileF32({
    handle, lat0Deg, lat1Deg, lon0Deg, lon1Deg, width, height,
    resolutionM = CANONICAL_RESOLUTION, marchSamples = WB_CLIMATE_CANONICAL_SAMPLES,
  }) {
    const values = width * height * WB_CLIMATE_STRIDE;
    const bytes = values * 4;
    const ptr = this.exports.wb_alloc(bytes);
    if (ptr === 0) throw new Error(`wb_alloc refused ${bytes} bytes for a ${width}x${height} climate tile`);
    try {
      const status = this.exports.wb_climate_tile_f32(
        handle, lat0Deg, lat1Deg, lon0Deg, lon1Deg, width, height, resolutionM,
        marchSamples, ptr, values,
      ) >>> 0;
      if (status !== WB_OK) {
        throw new Error(`wb_climate_tile_f32 returned ${STATUS_NAMES[status] ?? status}`);
      }
      return new Float32Array(this.memory.buffer, ptr, values).slice();
    } finally {
      this.exports.wb_dealloc(ptr, bytes);
    }
  }

  /// This world's own climate calibration: the band edges both quantiled axes are cut at,
  /// how many of the engine's 4,000 spiral points were land, and the lapse rate.
  ///
  /// Returns `{ moistureEdges, landformEdges, landSamples, lapseCPerKm }` -- a plain
  /// structured-cloneable object, because it is posted to every relief worker rather than
  /// recomputed there.
  ///
  /// # It is seconds, not milliseconds, and it must not run on the main thread
  ///
  /// 4,000 elevations plus one march at every land point: roughly 190,000 elevation queries
  /// on a 29%-land world. `main.js` dispatches it to a pool worker for the same reason it
  /// dispatches `wb_water_run`, and times it.
  ///
  /// # NaN edges are an answer
  ///
  /// A world the engine could not calibrate reports NaN edges and `WB_OK`; `landSamples` is
  /// what tells that apart from a real banding, and `bandIndex` must refuse a NaN edge rather
  /// than band against it. Passed through unchanged rather than repaired here.
  climateCalibration({
    handle, resolutionM = CANONICAL_RESOLUTION, marchSamples = WB_CLIMATE_CANONICAL_SAMPLES,
  }) {
    const bytes = WB_CLIMATE_CALIBRATION_STRIDE * 8;
    const ptr = this.exports.wb_alloc(bytes);
    if (ptr === 0) throw new Error("wb_alloc refused the climate calibration buffer");
    try {
      const status = this.exports.wb_climate_calibration(
        handle, resolutionM, marchSamples, ptr, WB_CLIMATE_CALIBRATION_STRIDE,
      ) >>> 0;
      if (status !== WB_OK) {
        throw new Error(`wb_climate_calibration returned ${STATUS_NAMES[status] ?? status}`);
      }
      const words = new Float64Array(this.memory.buffer, ptr, WB_CLIMATE_CALIBRATION_STRIDE);
      return {
        moistureEdges: Array.from(words.subarray(0, 4)),
        landformEdges: Array.from(words.subarray(4, 6)),
        landSamples: words[6],
        lapseCPerKm: words[7],
      };
    } finally {
      this.exports.wb_dealloc(ptr, bytes);
    }
  }

  /// Resolve one world's water and hand back the **shipped** water manifest.
  ///
  /// This is slice 5b's whole output, read through the door it built: fill, overflow
  /// resolution, the tied-plateau merge and classification, run over a stream graph sampled
  /// from this world's own surface. It is not a step of that path and it is not a second
  /// implementation of it -- `wb_water_run` dumps `water_manifest_from_graph`'s result, which
  /// is the same object the parity corpus compares native against wasm.
  ///
  /// # One call, over-allocated, and the measurement that decided it
  ///
  /// `wb_water_run` is **all-or-nothing**: a short `out_bodies` is `WB_ERR_BUFFER` with
  /// nothing written anywhere. Its doc offers two ways to avoid that -- a count-only query
  /// (null pointer, `out_len == 0`) followed by a sized second call, or a single call sized
  /// at `node_count * WB_WATER_BODY_STRIDE`, "which is always sufficient: no body holds fewer
  /// than one node".
  ///
  /// **The count-only query costs a whole second resolution, and it was measured rather than
  /// assumed.** The owner's world (seed 562423712, radius 4,500,000 m, 28 plates, land 0.16,
  /// the `ranges` tectonic preset), node 22.17.0, this repository's checked-in
  /// `worldbuilder_engine.wasm`: one call at `node_count = 30,000` takes **4.21 s**; the
  /// two-call shape takes **8.36 s and 8.37 s** on two repeats, i.e. exactly twice. The
  /// export does no caching between calls -- it re-samples the nodes, re-asks the surface for
  /// 30,000 elevations and rebuilds the neighbour relation every time.
  ///
  /// The over-allocation it buys back is **1.68 MB** of transient linear memory
  /// (`30,000 * 7 * 8` bytes) to receive 55 bodies' worth of rows. Four seconds of a
  /// blocking boot against 1.7 MB freed immediately afterwards is not a close call.
  ///
  /// # Domains, restated here only as the messages a refusal produces
  ///
  /// Every bound below is the engine's and is checked *by* the engine; nothing here
  /// re-derives one. `nodeCount` is `2..=100_000`, `seaLevelM` finite and within the world's
  /// radius, `pondMaxSurfaceAreaM2` finite and `>= 0`.
  ///
  /// **`pondMaxSurfaceAreaM2` defaults to 0 and that is deliberate.** The engine calibrated
  /// it at `1.0e5` m^2 and then measured that the smallest body this mesh produces is
  /// `7.9e8` m^2 -- four orders larger -- so **no body is ever a pond at any resolution this
  /// project bakes at**. The threshold's only effect is `kind`, which this viewer does not
  /// read: it draws every body at its own level whatever the label. Passing 0 says that in
  /// the parameter rather than restating an engine number the engine does not export, which
  /// is this viewer's characteristic defect (a second copy of a number) in the shape it keeps
  /// taking. There is no `wb_water_preset`, and that gap is reported rather than papered over
  /// with a literal.
  ///
  /// Returns `{ seaLevelM, bodies }`, where each body is
  /// `{ rootNode, kind, levelM, minLatitudeDeg, maxLatitudeDeg, minLongitudeDeg,
  /// maxLongitudeDeg }` in `WB_WATER_BODY_STRIDE`'s documented order. Rows arrive ascending
  /// by `rootNode` and are not re-sorted here: the engine says that order is the contract, so
  /// a caller comparing two runs row by row is comparing the same body on both sides.
  ///
  /// **`minLongitudeDeg` may exceed `maxLongitudeDeg`.** That is not corruption: `Extent`
  /// normalises to the smallest enclosing arc on the circle, and an arc crossing the
  /// antimeridian is expressed by the pair being out of order. `water.js::bodyContains` is
  /// the one place that branch is written.
  waterRun({ handle, nodeCount, seaLevelM = 0, pondMaxSurfaceAreaM2 = 0 }) {
    const words = nodeCount * WB_WATER_BODY_STRIDE;
    const bodyBytes = words * 8;
    const scalarBytes = 16; // one 8-aligned block: the u32 count at +0, the f64 datum at +8
    const scalarPtr = this.exports.wb_alloc(scalarBytes);
    if (scalarPtr === 0) throw new Error("wb_alloc refused the water scalar buffer");
    let bodyPtr = 0;
    try {
      bodyPtr = this.exports.wb_alloc(bodyBytes);
      if (bodyPtr === 0) {
        throw new Error(`wb_alloc refused ${bodyBytes} bytes for up to ${nodeCount} bodies`);
      }
      const status = this.exports.wb_water_run(
        handle, nodeCount, seaLevelM, pondMaxSurfaceAreaM2,
        bodyPtr, words, scalarPtr, scalarPtr + 8,
      ) >>> 0;
      if (status !== WB_OK) {
        const error = new Error(
          `wb_water_run returned ${statusName(status)} for nodeCount=${nodeCount} ` +
          `seaLevel=${seaLevelM} pondMax=${pondMaxSurfaceAreaM2}`,
        );
        // `.status`, so `WB_ERR_CARVED` (Ruling C-24: the run reads the ground) can be named.
        error.status = status;
        throw error;
      }
      // Views after the allocation, read immediately, never cached -- the module doc's rule 1.
      const bodyCount = new Uint32Array(this.memory.buffer, scalarPtr, 1)[0] >>> 0;
      const seaLevelOut = new Float64Array(this.memory.buffer, scalarPtr + 8, 1)[0];
      const bodies = [];
      if (bodyCount > 0) {
        const row = new Float64Array(this.memory.buffer, bodyPtr, bodyCount * WB_WATER_BODY_STRIDE);
        for (let i = 0; i < bodyCount; i += 1) {
          const o = i * WB_WATER_BODY_STRIDE;
          bodies.push({
            rootNode: row[o],
            kind: row[o + 1],
            levelM: row[o + 2],
            minLatitudeDeg: row[o + 3],
            maxLatitudeDeg: row[o + 4],
            minLongitudeDeg: row[o + 5],
            maxLongitudeDeg: row[o + 6],
          });
        }
      }
      // The datum is the ENGINE's echo, not the argument sent in. `wb_water_run` writes
      // `WaterManifest::sea_level_m` back precisely so a host reads what the manifest was
      // built at rather than assuming its own request survived.
      return { seaLevelM: seaLevelOut, bodies };
    } finally {
      if (bodyPtr !== 0) this.exports.wb_dealloc(bodyPtr, bodyBytes);
      this.exports.wb_dealloc(scalarPtr, scalarBytes);
    }
  }

  /// Bake `handle`'s hydrology and hand back the encoded record as a **copy**, a
  /// `Float64Array` on the JS heap.
  ///
  /// `wb_hydro_bake` cannot be sized in one call the way `wb_water_run` is, because the
  /// record's own length depends on how many hollows, reaches, notches and falls the bake
  /// finds -- so this is measure-then-copy: bake and hold (`wb_hydro_bake`), ask the held
  /// record's length (`wb_hydro_len`), copy it out (`wb_hydro_copy`), then free the held
  /// copy (`wb_hydro_free`) whether the copy succeeded or not.
  ///
  /// `params` takes camelCase fields mirroring `WB_HYDRO_PARAMS_STRIDE`'s documented order:
  /// `totalNodes`, `wetnessNodes`, `keepDepthM`, `keepAreaM2`, `pondMaxAreaM2`,
  /// `streamFlowM2`, `riverFlowM2`, `greatFlowM2`, `notchFallM`, `evaporationFactor`,
  /// `saltFlatShare`, and `forcedOutlets` -- an array of `{ latitudeDeg, longitudeDeg }`,
  /// defaulting to none.
  ///
  /// The bake-and-hold half is `hydroHold`, which is what a caller that wants to *query* the
  /// bake (`waterAt`, `waterTile`) needs; this is that plus the free.
  hydroBake({ handle, params }) {
    const bake = this.hydroHold({ handle, params });
    this.hydroFree(bake);
    return bake.words;
  }

  /// `hydroBake`, but **the bake stays held** and comes back as `{ id, handle, words }`. The
  /// caller owns it and must pass the whole object to `hydroFree` when it is done.
  ///
  /// **The handle is in there on purpose** (Ruling Q-20): a query needs the record *and* the
  /// ground it was baked against, and issuing the two together keeps a call site from pairing
  /// this bake with a different world. Since plan 2b the engine also *checks* the pairing -- see
  /// `waterAt` -- so a drifted pair is refused rather than answered; carrying the handle is what
  /// keeps it from drifting in the first place.
  ///
  /// This exists because `waterAt` and `waterTile` query a *held* bake -- Ruling Q-2 builds the
  /// spatial index on the first query and caches it beside the record, and freeing the bake
  /// frees the index -- so a caller that wants to ask §8.3 questions cannot use the
  /// bake-and-free shape above. The record comes back anyway, and not as a second copy step,
  /// because the answers name bodies and reaches by **id**: `fresh`, a river's class and a
  /// body's kind are read out of the record entry the id points at.
  ///
  /// **`params.forCarving: true` bakes the record FOR CARVING** (Ruling C-20): the 13-word layout
  /// with word 12 set, a SCHEMA 8 record whose ponds a channel is cut beneath are drained. Only such
  /// a record can carve a world (`newWorld`'s `bake`); anything else is the ordinary 12-word bake,
  /// byte for byte the buffer this method sent before the flag existed.
  hydroHold({ handle, params }) {
    const paramWords = hydroParamsWords(params);
    const stride = paramWords.length;
    const paramsBytes = stride * 8;
    const paramsPtr = this.exports.wb_alloc(paramsBytes);
    if (paramsPtr === 0) throw new Error("wb_alloc refused the hydro params buffer");
    const idBytes = 4;
    const idPtr = this.exports.wb_alloc(idBytes);
    if (idPtr === 0) {
      this.exports.wb_dealloc(paramsPtr, paramsBytes);
      throw new Error("wb_alloc refused the hydro id buffer");
    }
    let id = 0;
    let wordsPtr = 0;
    let wordsBytes = 0;
    let held = false;
    try {
      new Float64Array(this.memory.buffer, paramsPtr, stride).set(paramWords);
      const status = this.exports.wb_hydro_bake(handle, paramsPtr, stride, idPtr) >>> 0;
      if (status !== WB_OK) {
        // `.status` so a caller can name the refusal -- `WB_ERR_CARVED` above all, which is what
        // a bake asked of a carved world answers (Rulings C-1 and C-24).
        const error = new Error(`wb_hydro_bake returned ${statusName(status)}`);
        error.status = status;
        throw error;
      }
      id = new Uint32Array(this.memory.buffer, idPtr, 1)[0] >>> 0;
      const len = this.exports.wb_hydro_len(id) >>> 0;
      wordsBytes = len * 8;
      wordsPtr = this.exports.wb_alloc(wordsBytes);
      if (wordsPtr === 0) throw new Error(`wb_alloc refused ${wordsBytes} bytes for a hydro record`);
      const copyStatus = this.exports.wb_hydro_copy(id, wordsPtr, len) >>> 0;
      if (copyStatus !== WB_OK) {
        throw new Error(`wb_hydro_copy returned ${statusName(copyStatus)}`);
      }
      // The view is created after the allocation and copied immediately, before this
      // function's own dealloc calls can detach the buffer -- the module doc's rule 1.
      const record = new Float64Array(this.memory.buffer, wordsPtr, len).slice();
      held = true;
      // Ruling Q-20: the handle travels WITH the id. `waterAt` and `waterTile` need both, and
      // the two are only correct together; returning them as one object is what stops a call
      // site pairing this bake with a different world by hand (and the engine refuses one that
      // does, with WB_ERR_WRONG_WORLD).
      return { id, handle, words: record };
    } finally {
      // The bake is freed on the way out ONLY if this call is failing: on success the id is
      // the caller's, which is the whole difference between this and `hydroBake`.
      if (id !== 0 && !held) this.exports.wb_hydro_free(id);
      if (wordsPtr !== 0) this.exports.wb_dealloc(wordsPtr, wordsBytes);
      this.exports.wb_dealloc(idPtr, idBytes);
      this.exports.wb_dealloc(paramsPtr, paramsBytes);
    }
  }

  /// Drop a bake held by `hydroHold`, and with it the query index Ruling Q-2 cached beside it.
  /// Takes the whole `{ id, handle, words }` object `hydroHold` handed back, so a bake is one
  /// currency everywhere (Ruling Q-20).
  ///
  /// Throws on an id that names no live bake, so a double free is a message rather than a
  /// silently ignored `1`.
  hydroFree(bake) {
    const status = this.exports.wb_hydro_free(bake.id) >>> 0;
    if (status !== WB_OK) {
      throw new Error(`wb_hydro_free returned ${statusName(status)}`);
    }
  }

  /// **Spec §8.3 at one point**: what water is at `latitudeDeg, longitudeDeg`, according to the
  /// held `bake` -- the whole `{ id, handle, words }` object `hydroHold` handed back.
  ///
  /// Returns `{ kind, levelM, depthM, bodyId, reachId }`, where `kind` is one of
  /// `WB_WATER_KIND`'s names -- `"none"`, `"ocean"`, `"lake"`, `"saltLake"`, `"saltFlat"`,
  /// `"pond"`, `"river"` -- and `bodyId` / `reachId` are `null` where the answer names no
  /// recorded body or reach rather than the raw `0xffffffff` sentinel. A `"river"` with a `null`
  /// `reachId` is a notch's water: a lake's outflow through its rim, too small to be recorded as a
  /// reach, which the carve cuts and the query therefore answers as water (Ruling C-35).
  ///
  /// **`fresh` is not here**, and is not missing: it belongs to the body or reach the ids name,
  /// so read it out of `bake.words`. See `WB_WATER_STRIDE`.
  ///
  /// **Why the bake carries its own handle (Ruling Q-20), and what the engine checks.** The
  /// answer needs a record *and* a ground. Handle equality is not the test of whether one belongs
  /// to the other -- handles are never reused and the studio re-creates one on every slider
  /// change, so it would invalidate every held bake including the bit-identical ones. Content is
  /// the right key, and since plan 2b the engine uses it: the record's ground fingerprint
  /// (`hydroSummary`'s `ground`) is compared with the world's, and a bake asked through a world
  /// of other ground -- even one of the same radius -- throws `WB_ERR_WRONG_WORLD` instead of
  /// answering that world's ground against this record's levels. A world re-created from the same
  /// parameters has the same ground and is accepted. The pair is still **issued together and
  /// passed together**, so a correct caller never meets the refusal.
  waterAt({ bake, latitudeDeg, longitudeDeg }) {
    const bytes = WB_WATER_STRIDE * 8;
    const ptr = this.exports.wb_alloc(bytes);
    if (ptr === 0) throw new Error("wb_alloc refused the water sample buffer");
    try {
      const status = this.exports.wb_water_at(
        bake.handle, bake.id, latitudeDeg, longitudeDeg, ptr, WB_WATER_STRIDE) >>> 0;
      if (status !== WB_OK) {
        throw new Error(`wb_water_at returned ${statusName(status)}`);
      }
      const words = new Float64Array(this.memory.buffer, ptr, WB_WATER_STRIDE);
      return decodeWaterSample(words, 0);
    } finally {
      this.exports.wb_dealloc(ptr, bytes);
    }
  }

  /// **The batch**: `rows * columns` §8.3 answers in one call against the held `bake`, as a
  /// `Float64Array` of `rows * columns * WB_WATER_STRIDE` words on the JS heap.
  ///
  /// `box` is `{ lat0, lon0, lat1, lon1 }` and the grid is row-major with **both endpoints
  /// included**, exactly `fillTileF32`'s shape: row 0 at `lat0`, row `rows - 1` at `lat1`,
  /// column 0 at `lon0`, column `columns - 1` at `lon1`. Sample `row * columns + column`
  /// starts at word `WB_WATER_STRIDE * (row * columns + column)`; `decodeWaterSample` reads one
  /// out. Nothing is interpolated and nothing is smoothed -- every sample is exactly what
  /// `waterAt` answers at that point.
  ///
  /// The raw buffer is returned rather than an array of objects because a tile is thousands of
  /// samples and the caller is a drawing path: it strides, it does not allocate.
  ///
  /// `rows` and `columns` are checked **before** anything is allocated; see `waterTileBytes`,
  /// which is where the reason lives.
  waterTile({ bake, box, rows, columns }) {
    const { words, bytes } = waterTileBytes(rows, columns);
    const ptr = this.exports.wb_alloc(bytes);
    if (ptr === 0) throw new Error(`wb_alloc refused ${bytes} bytes for a water tile`);
    try {
      const status = this.exports.wb_water_tile(
        bake.handle, bake.id, box.lat0, box.lon0, box.lat1, box.lon1,
        rows, columns, ptr, words) >>> 0;
      if (status !== WB_OK) {
        throw new Error(`wb_water_tile returned ${statusName(status)}`);
      }
      return new Float64Array(this.memory.buffer, ptr, words).slice();
    } finally {
      this.exports.wb_dealloc(ptr, bytes);
    }
  }

  /// Read a `hydroBake` record's 60-word header (schema 7, Task 1 of plan 2b) into a plain
  /// object. Words 0-19 are unchanged from schema 2: `schema`, `bodies`, `reaches`, `notches`,
  /// `falls`, `nodes`, `landNodes`, `hollows`, `kept`, `notched`, `closed`, `streams`, `rivers`,
  /// `great`, `maxOrder`, `bifurcationMin`, `bifurcationMax`, `streamFlowM2`, `riverFlowM2`,
  /// `greatFlowM2` -- the last three are the effective thresholds a coarse bake actually used
  /// (Ruling 12b-1), not necessarily the ones the caller asked for. Words 20-31 (schema 3) are
  /// the params echo (`totalNodes`, `wetnessNodes`, `keepDepthM`, `keepAreaM2`, `pondMaxAreaM2`,
  /// `keepMaxAreaM2`, `minStreamNodes`, `notchFallM`, `evaporationFactor`, `saltFlatShare`) and
  /// the forced-outlet match counts (`forcedRequested`, `forcedMatched`). Words 32-42 (schema 4)
  /// are new: what capped basins keep (`cappedBasins`, `cappedInner`, `cappedInnerKept`,
  /// carry-forward I3) and the refinement params echo (`refineStepM`, `refineSimplifyM`,
  /// `refineVerticalM`, `fallMinDropM`, `fallMaxRunM`, `meanderWavelengthWidths`,
  /// `meanderAmplitudeWidths`, `meanderMaxSlope`). Words 43-44 (schema 5) are the crossing
  /// pass's two counts: `crossingsCoarse`, how many crossings the coarse record already had
  /// (Ruling S-2 keeps those -- they are graph artifacts refinement did not make), and
  /// `crossingsLeft`, how many are left in the record as it ships. Words 45-53 (Task 5 of the
  /// same plan, still schema 5) are the fine pond search's two counts, `pondsFound` and
  /// `pondsKept`, then its seven params (`pondCellM`, `pondSearchRadiusM`, `pondKeepDepthM`,
  /// `pondKeepAreaM2`, `pondWetnessShare`, `pondMaxSlope`, `pondDensityAreaM2`). Words 54-55
  /// (schema 6, Task 1 of plan 1b-4) are the extent totals across all bodies, `shoreMembers` and
  /// `collarPoints` -- the record's own account of what the extent trim cost. Both total the
  /// COARSE bodies only: `shoreMembers` is the sum of their `shoreMemberCount` and
  /// `collarPoints` the sum of their outline length less that count. A pond contributes to
  /// neither -- Ruling E-6 zeroes its count, and its outline is a traced ring, not a collar -- so
  /// `collarPoints` is NOT the sum over every body of `outline.length - shoreMemberCount`. A
  /// schema 6 bake with any coarse body in it reports both above zero. Words 56-59 (schema 7,
  /// Task 1 of plan 2b) are the **ground fingerprint**, `ground`: 16 bytes of BLAKE2b over 64
  /// millimetre-rounded samples of the ground the bake read -- `Surface::bake_ground_m`,
  /// elevation with detail and without the water layer (`record.rs::ground_fingerprint`) --
  /// four little-endian bytes a word, returned as 32 lowercase hex digits in byte order. It is
  /// the record's tie to the world it was baked from, and `waterAt`/`waterTile` refuse a world
  /// whose own fingerprint differs (`WB_ERR_WRONG_WORLD`).
  ///
  /// **`pondsFound` counts hollows in the corridors the search sampled, not in every corridor.**
  /// Ruling S-12 skips a coarse segment whose midpoint is drier than the wetness floor or inside
  /// a coarse body before its corridor is sampled at all, so those hollows are never found and
  /// never counted -- a 65% step down from the old meaning, measured in Task 5. A consumer that
  /// reads it as "every hollow the world has near a river" will read it 3x too small.
  ///
  /// Of the schema 4 words this returns ONLY 32-34, the three capped-basin counts. The
  /// refinement params echo in words 35-42 is in the record and read by `water-preview.js`'s
  /// `decodeHydro`; it is not a field of this summary. Nothing here is a params echo the studio
  /// can set: by Ruling R-8 the refinement params are not wasm params, so a wasm bake always
  /// used `earth_like`'s values for them. The pond params in words 47-53 are the same story and
  /// are left out for the same reason. The four counts -- both crossing words and both pond
  /// words -- ARE returned: they are counts a bake produced, not params.
  ///
  /// Throws on a schema other than 7 or 8: another schema's header is not these 60 words, and a
  /// summary read off it would be wrong silently. **Word 0 is 8 for a record baked for carving**
  /// (Ruling C-20, `record.rs`'s `SCHEMA_CARVE`): the same 60 words in the same places, read the
  /// same way, and reported here as `drainedForCarve`. Throws too on a fingerprint word that is not
  /// a u32, since it cannot be four bytes.
  ///
  /// Past the header (read in full by `water-preview.js`'s `decodeHydro`), two positions share
  /// a slot and not a meaning, as `record.rs`'s module doc states: a reach point's third word is
  /// the bed (the water surface minus the depth), and a notch point's third word is the cut
  /// surface (the lowered ground, the water surface through the cut). Likewise body `fresh`
  /// means "not closed", and reach `fresh` means "its chain reaches the ocean".
  hydroSummary(words) {
    if (words[0] !== 7 && words[0] !== 8) {
      throw new Error(`hydro record: unsupported schema ${words[0]} (expected 7, or 8 baked for carving)`);
    }
    return {
      schema: words[0],
      drainedForCarve: words[0] === 8,
      bodies: words[1],
      reaches: words[2],
      notches: words[3],
      falls: words[4],
      nodes: words[5],
      landNodes: words[6],
      hollows: words[7],
      kept: words[8],
      notched: words[9],
      closed: words[10],
      streams: words[11],
      rivers: words[12],
      great: words[13],
      maxOrder: words[14],
      bifurcationMin: words[15],
      bifurcationMax: words[16],
      streamFlowM2: words[17],
      riverFlowM2: words[18],
      greatFlowM2: words[19],
      totalNodes: words[20],
      wetnessNodes: words[21],
      keepDepthM: words[22],
      keepAreaM2: words[23],
      pondMaxAreaM2: words[24],
      keepMaxAreaM2: words[25],
      minStreamNodes: words[26],
      notchFallM: words[27],
      evaporationFactor: words[28],
      saltFlatShare: words[29],
      forcedRequested: words[30],
      forcedMatched: words[31],
      cappedBasins: words[32],
      cappedInner: words[33],
      cappedInnerKept: words[34],
      crossingsCoarse: words[43],
      crossingsLeft: words[44],
      pondsFound: words[45],
      pondsKept: words[46],
      shoreMembers: words[54],
      collarPoints: words[55],
      ground: groundHex([words[56], words[57], words[58], words[59]]),
    };
  }
}

/// Four ground-fingerprint words (a SCHEMA 7 header's 56-59) as the digest's 32 hex digits, in
/// byte order: each word holds four bytes little-endian, exactly as `record.rs` writes them.
function groundHex(ground) {
  let hex = "";
  for (const w of ground) {
    if (!(Number.isInteger(w) && w >= 0 && w <= 0xffffffff)) {
      throw new Error(`hydro record: bad ground fingerprint word ${w}`);
    }
    for (let shift = 0; shift < 32; shift += 8) {
      hex += ((w >>> shift) & 0xff).toString(16).padStart(2, "0");
    }
  }
  return hex;
}

export function statusName(code) {
  return STATUS_NAMES[code] ?? String(code);
}

/// **Ruling Q-19.** How many words and how many bytes a `rows x columns` water tile needs,
/// refusing every shape whose answer cannot be carried honestly across the boundary.
///
/// **This is a memory-safety guard, not an ergonomics one.** Every integer argument to a wasm
/// export goes through `ToUint32`, and `words` and `bytes` are two different JS doubles that do
/// **not** wrap in step. `rows = 107_374_183, columns = 5` gives `words = 536_870_915` and
/// `bytes = 4_294_967_320`, and `ToUint32(bytes)` is **24**. Unguarded, `wb_alloc` hands back a
/// twenty-four byte buffer while `out_len` arrives at the far side intact at 536,870,915; Rust
/// recomputes exactly that from its own honest `u32` `rows` and `columns`, agrees the buffer is
/// long enough, and writes about 4.3 GB into 24 bytes. No Rust-side check can catch it -- every
/// number the export can see is self-consistent -- so it has to be caught here, and it has to be
/// caught **before the allocation**.
///
/// The binding bound is the **byte** count, not the word count: `bytes` is eight times `words`,
/// so `bytes <= 0xffffffff` caps a tile at `floor(0xffffffff / 8)` = 536,870,911 words -- one
/// eighth of the word bound (`u32::MAX` = 4,294,967,295), not a few words under it.
/// Both are stated because the wrap that does the damage is the byte one.
///
/// A zero or fractional dimension is refused here too, so `rows: 0` says which argument was
/// wrong instead of reporting "wb_alloc refused 0 bytes", which names the wrong thing.
export function waterTileBytes(rows, columns) {
  for (const [name, value] of [["rows", rows], ["columns", columns]]) {
    if (!Number.isInteger(value) || value <= 0) {
      throw new Error(`waterTile: ${name} must be a positive integer, got ${value}`);
    }
  }
  const words = rows * columns * WB_WATER_STRIDE;
  const bytes = words * 8;
  if (bytes > 0xffffffff) {
    throw new Error(
      `waterTile: ${rows}x${columns} samples need ${words} words (${bytes} bytes), past the ` +
      `u32 the boundary carries -- the byte count wraps and the word count does not, so this ` +
      `would allocate short and be written long`);
  }
  return { words, bytes };
}

/// `WB_WATER_KIND` inverted: the code a sample's word 0 carries to the name it means.
const WATER_KIND_NAMES = Object.fromEntries(
  Object.entries(WB_WATER_KIND).map(([name, code]) => [code, name]));

/// Read one water sample out of a `waterTile` buffer (or a `waterAt` one), starting at
/// `sample * WB_WATER_STRIDE`. **The only place the five-word order is spelled out**, so a
/// reader that gets it wrong gets it wrong once.
///
/// `bodyId` and `reachId` come back `null` where the answer names no recorded body or reach,
/// rather than as the raw `0xffffffff` sentinel -- an id that is a number is an id you can
/// index the record with, and a `null` is one you cannot mistake for entry 4,294,967,295.
export function decodeWaterSample(words, sample = 0) {
  const at = sample * WB_WATER_STRIDE;
  const code = words[at];
  const kind = WATER_KIND_NAMES[code];
  if (kind === undefined) {
    throw new Error(`water sample: unknown kind ${code}`);
  }
  const bodyId = words[at + 3];
  const reachId = words[at + 4];
  return {
    kind,
    levelM: words[at + 1],
    depthM: words[at + 2],
    bodyId: bodyId === WB_NO_BODY ? null : bodyId,
    reachId: reachId === WB_NO_REACH ? null : reachId,
  };
}
