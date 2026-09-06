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
import {
  COAST_STRIDE, COAST_PRESET, coastToRecord, coastFromRecord,
} from "./coast-params.js";
import {
  GULLY_STRIDE, GULLY_PRESET, gullyToRecord, gullyFromRecord,
} from "./gully-params.js";

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

const STATUS_NAMES = {
  0: "WB_OK",
  1: "WB_ERR_HANDLE",
  2: "WB_ERR_BUFFER",
  3: "WB_ERR_GRID",
  4: "WB_ERR_SUBSTRATE",
  5: "WB_ERR_PARAM",
  6: "WB_ERR_GRAPH",
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
      "wb_world_new_gully", "wb_gully_preset", "wb_gully_check", "wb_world_free",
      "wb_world_count", "wb_elevation_m", "wb_structural_m", "wb_bottom_at",
      "wb_fill_tile_f32", "wb_water_run",
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

  /// `relief` is `null`/absent for the canonical path — a null pointer and a length of zero,
  /// which is the same `None` `wb_world_new` passes and is byte-for-byte today's world. An
  /// object keyed by `RELIEF_FIELDS` asks for a different one.
  /// `tectonics` is the same story: `null`/absent is `None` -- Ruling 1 of the mountains
  /// slice, which is that the default world cannot move -- and an object keyed by
  /// `TECTONIC_FIELDS` asks for a different uplift.
  /// `coast` is the same story again: `null`/absent is `None` -- RULING 1 of the fractal-coastline
  /// slice, which is that the default coastline cannot move -- and an object keyed by
  /// `COAST_FIELDS` asks for a roughened one.
  /// `gully` is the fifth and last: `null`/absent is `None`, which in this channel means
  /// `Surface::with_gully` builds **no steering lattice at all** and `elevation_m` takes a
  /// different branch -- so the canonical path is structurally the old one rather than the new one
  /// plus zero. An object keyed by `GULLY_FIELDS` asks for the drainage texture.
  newWorld({
    seed, radiusM, plateCount, landFraction, features = [], relief = null, tectonics = null,
    coast = null, gully = null,
  }) {
    let ptr = 0;
    let bytes = 0;
    let reliefPtr = 0;
    let reliefBytes = 0;
    let tectonicPtr = 0;
    let tectonicBytes = 0;
    let coastPtr = 0;
    let coastBytes = 0;
    let gullyPtr = 0;
    let gullyBytes = 0;
    try {
      if (gully) {
        gullyBytes = GULLY_STRIDE * 8;
        gullyPtr = this.exports.wb_alloc(gullyBytes);
        if (gullyPtr === 0) throw new Error("wb_alloc refused the gully buffer");
        new Float64Array(this.memory.buffer, gullyPtr, GULLY_STRIDE).set(gullyToRecord(gully));
      }
      if (coast) {
        coastBytes = COAST_STRIDE * 8;
        coastPtr = this.exports.wb_alloc(coastBytes);
        if (coastPtr === 0) throw new Error("wb_alloc refused the coast buffer");
        new Float64Array(this.memory.buffer, coastPtr, COAST_STRIDE).set(coastToRecord(coast));
      }
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
      // ONE constructor for all SIXTEEN paths, and `wb_world_new_gully` is now it. With all four
      // blocks null this is `(null, 0, null, 0, null, 0, null, 0)`, which the engine reads as four
      // `None`s — the same world `wb_world_new` builds, which the engine-side tests
      // `the_relief_channel_default_path_is_the_untouched_world`,
      // `the_tectonic_channel_default_path_is_the_untouched_world`,
      // `the_coast_channel_default_path_is_the_untouched_world` and
      // `gully_none_matches_gully_some_canonical_bit_for_bit` pin bit for bit.
      //
      // Calling the widest door unconditionally rather than choosing between five is
      // deliberate: a branch here would mean the default path and the chosen path went
      // through different exports, and the byte-identity those tests assert would stop
      // covering what the viewer actually calls. **This line moving from `wb_world_new_coast` to
      // `wb_world_new_gully` is the whole of the wiring**, and it is the line the digest control
      // exists to hold: the default picture must not move because of it.
      const handle = this.exports.wb_world_new_gully(
        BigInt(seed), radiusM, plateCount, landFraction, ptr, features.length,
        reliefPtr, relief ? RELIEF_STRIDE : 0,
        tectonicPtr, tectonics ? TECTONIC_STRIDE : 0,
        coastPtr, coast ? COAST_STRIDE : 0,
        gullyPtr, gully ? GULLY_STRIDE : 0,
      ) >>> 0;
      if (handle === 0) {
        // A refused world is a blank viewer, so the message has to name the reason. The three
        // parameter blocks are the arguments here with checkers that can say which field.
        const why =
          (relief ? ` relief=${statusName(this.checkRelief(relief))}` : "") +
          (tectonics ? ` tectonics=${statusName(this.checkTectonic(tectonics))}` : "") +
          (coast ? ` coast=${statusName(this.checkCoast(coast))}` : "") +
          (gully ? ` gully=${statusName(this.checkGully(gully))}` : "");
        throw new Error(
          `wb_world_new_gully refused seed=${seed} radius=${radiusM} plates=${plateCount} ` +
          `land=${landFraction} features=${features.length}${why}`,
        );
      }
      return handle;
    } finally {
      if (ptr !== 0) this.exports.wb_dealloc(ptr, bytes);
      if (reliefPtr !== 0) this.exports.wb_dealloc(reliefPtr, reliefBytes);
      if (tectonicPtr !== 0) this.exports.wb_dealloc(tectonicPtr, tectonicBytes);
      if (coastPtr !== 0) this.exports.wb_dealloc(coastPtr, coastBytes);
      if (gullyPtr !== 0) this.exports.wb_dealloc(gullyPtr, gullyBytes);
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
        throw new Error(
          `wb_water_run returned ${statusName(status)} for nodeCount=${nodeCount} ` +
          `seaLevel=${seaLevelM} pondMax=${pondMaxSurfaceAreaM2}`,
        );
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
}

export function statusName(code) {
  return STATUS_NAMES[code] ?? String(code);
}
