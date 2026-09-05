//! The engine module, as JavaScript sees it.
//
// `viewer/public/wasm/worldbuilder_engine.wasm` has **zero imports** by design, so
// `WebAssembly.instantiate(bytes, {})` is the entire loader: no wasm-bindgen, no glue
// module, no bundler. Everything below is hand-written marshalling over the fourteen
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

/// Status codes, mirrored from `wasm.rs`. Kept as names so a failure reads as a sentence.
export const WB_OK = 0;
export const WB_ERR_HANDLE = 1;
export const WB_ERR_BUFFER = 2;
export const WB_ERR_GRID = 3;
export const WB_ERR_SUBSTRATE = 4;
export const WB_ERR_PARAM = 5;

const STATUS_NAMES = {
  0: "WB_OK",
  1: "WB_ERR_HANDLE",
  2: "WB_ERR_BUFFER",
  3: "WB_ERR_GRID",
  4: "WB_ERR_SUBSTRATE",
  5: "WB_ERR_PARAM",
};

/// Feature record codes, mirrored from `wasm.rs`. A record is eight f64.
export const WB_FEATURE_STRIDE = 8;
export const COMPOSE = { raise: 0, carve: 1, shape: 2 };
export const SUBSTRATE = { derive: 0, sand: 1, mud: 2, rock: 3 };

/// The `resolution_m` sentinel: anything non-positive or non-finite means canonical ground
/// truth (the engine's `None`). `-1` is the spelling used here so it is obviously deliberate
/// rather than an uninitialised variable that happened to be zero.
export const CANONICAL_RESOLUTION = -1;

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
      "wb_world_new_tectonic", "wb_tectonic_preset", "wb_tectonic_check", "wb_world_free",
      "wb_world_count", "wb_elevation_m", "wb_structural_m", "wb_bottom_at",
      "wb_fill_tile_f32",
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

  /// `relief` is `null`/absent for the canonical path — a null pointer and a length of zero,
  /// which is the same `None` `wb_world_new` passes and is byte-for-byte today's world. An
  /// object keyed by `RELIEF_FIELDS` asks for a different one.
  /// `tectonics` is the same story: `null`/absent is `None` -- Ruling 1 of the mountains
  /// slice, which is that the default world cannot move -- and an object keyed by
  /// `TECTONIC_FIELDS` asks for a different uplift.
  newWorld({
    seed, radiusM, plateCount, landFraction, features = [], relief = null, tectonics = null,
  }) {
    let ptr = 0;
    let bytes = 0;
    let reliefPtr = 0;
    let reliefBytes = 0;
    let tectonicPtr = 0;
    let tectonicBytes = 0;
    try {
      if (tectonics) {
        tectonicBytes = TECTONIC_STRIDE * 8;
        tectonicPtr = this.exports.wb_alloc(tectonicBytes);
        if (tectonicPtr === 0) throw new Error("wb_alloc refused the tectonic buffer");
        new Float64Array(this.memory.buffer, tectonicPtr, TECTONIC_STRIDE)
          .set(tectonicToRecord(tectonics));
      }
      if (relief) {
        reliefBytes = RELIEF_STRIDE * 8;
        reliefPtr = this.exports.wb_alloc(reliefBytes);
        if (reliefPtr === 0) throw new Error("wb_alloc refused the relief buffer");
        new Float64Array(this.memory.buffer, reliefPtr, RELIEF_STRIDE).set(toRecord(relief));
      }
      if (features.length > 0) {
        bytes = features.length * WB_FEATURE_STRIDE * 8;
        ptr = this.exports.wb_alloc(bytes);
        if (ptr === 0) throw new Error("wb_alloc refused the feature buffer");
        const words = new Float64Array(this.memory.buffer, ptr, features.length * WB_FEATURE_STRIDE);
        features.forEach((f, i) => {
          words.set([
            f.latitudeDeg, f.longitudeDeg, f.targetM, f.lengthM, f.widthM, f.bearingDeg,
            COMPOSE[f.compose] ?? f.compose, SUBSTRATE[f.substrate ?? "derive"] ?? f.substrate,
          ], i * WB_FEATURE_STRIDE);
        });
      }
      // ONE constructor for all four paths, and `wb_world_new_tectonic` is it. With both
      // blocks null this is `(null, 0, null, 0)`, which the engine reads as `None` and
      // `None` — the same world `wb_world_new` builds, which the engine-side tests
      // `the_relief_channel_default_path_is_the_untouched_world` and
      // `the_tectonic_channel_default_path_is_the_untouched_world` pin bit for bit.
      //
      // Calling the widest door unconditionally rather than choosing between three is
      // deliberate: a branch here would mean the default path and the chosen path went
      // through different exports, and the byte-identity those tests assert would stop
      // covering what the viewer actually calls.
      const handle = this.exports.wb_world_new_tectonic(
        BigInt(seed), radiusM, plateCount, landFraction, ptr, features.length,
        reliefPtr, relief ? RELIEF_STRIDE : 0,
        tectonicPtr, tectonics ? TECTONIC_STRIDE : 0,
      ) >>> 0;
      if (handle === 0) {
        // A refused world is a blank viewer, so the message has to name the reason. The two
        // parameter blocks are the arguments here with checkers that can say which field.
        const why =
          (relief ? ` relief=${statusName(this.checkRelief(relief))}` : "") +
          (tectonics ? ` tectonics=${statusName(this.checkTectonic(tectonics))}` : "");
        throw new Error(
          `wb_world_new_tectonic refused seed=${seed} radius=${radiusM} plates=${plateCount} ` +
          `land=${landFraction} features=${features.length}${why}`,
        );
      }
      return handle;
    } finally {
      if (ptr !== 0) this.exports.wb_dealloc(ptr, bytes);
      if (reliefPtr !== 0) this.exports.wb_dealloc(reliefPtr, reliefBytes);
      if (tectonicPtr !== 0) this.exports.wb_dealloc(tectonicPtr, tectonicBytes);
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
}

export function statusName(code) {
  return STATUS_NAMES[code] ?? String(code);
}
