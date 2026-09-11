//! One worker, one engine instance, one world.
//
// This is a **module worker** (`new Worker(url, { type: "module" })`) so it can `import`
// the same `engine.js` the main thread uses. There is no bundler in this project and none
// is needed: the module graph a worker loads is the browser's own, and `engine.js` has no
// dependencies at all.
//
// # Why each worker builds its own world
//
// The engine wasm has **zero imports and no shared memory**. Every `WebAssembly.instantiate`
// makes a fresh linear memory, so a world handle from one instance is meaningless in
// another -- handles are indices into a table that lives *inside* that instance's memory.
// There is no way to hand a built world across; each worker has to call `wb_world_new`
// itself. That is affordable because `Surface::new` measures 2.2--3.2 ms, so eight workers
// spend ~25 ms of wall clock in parallel at boot and never rebuild.
//
// It also means the workers can silently disagree about which planet they are on, which is
// exactly what `stale-worker` below exists to prove the checks can see.
//
// # The reply transfers its buffer
//
// `postMessage(msg, [buffer])` moves the `ArrayBuffer` instead of copying it. The worker's
// `Float32Array` is detached by the transfer, which is correct: it was a copy off the wasm
// heap made by `fillTileF32` and the worker has no further use for it.
//
// # Four jobs: heights, relief, clouds and water -- and only three are about the world
//
// `cloud` is the third and it takes no world handle. See its own comment below.
//
// `water` is the fourth and it is **not a tile at all**: one call, tens of seconds, one manifest
// back. It was on the main thread and it was the whole cold load. See its own comment below.
//
// # Two jobs, one world: heights and relief
//
// `fill` answers the terrain mesh (a 65 x 65 `Float32Array` of heights). `relief` answers
// the imagery layer (a 256 x 256 RGBA raster). They are the same shape of work -- an engine
// fill against this worker's own world, then a reply that transfers its buffer -- and they
// deliberately share the world handle, because a relief raster drawn from a different world
// than the mesh is the `wrong-world` fault arrived at by accident and looks entirely
// plausible.
//
// **`relief` sends BYTES, not an image.** `ImageData` is structured-cloneable and
// `ImageBitmap` is transferable, so either could cross the wire; the raw
// `Uint8ClampedArray` is sent instead because it is the only one of the three that also
// works under `node --test`, where `relief.js` already returns an ImageData-shaped plain
// object. The main thread wraps the bytes back into an `ImageData` and blits them, which is
// microseconds against a rasterisation measured in hundreds of milliseconds -- see
// `relief-provider.js` for the measured split.

import { Engine } from "./engine.js";
import { reliefTile } from "./relief.js";
import { cloudTile } from "./clouds.js";

/// Faults that live on this side of the wire. Mirrored in `terrain.js`'s `FAULTS`; the
/// worker is told which one is active at init so a stale world is built *once*, the way a
/// real version-skew bug would be, rather than re-decided per tile.
const FAULT_STALE_WORKER = "stale-worker";
const FAULT_WRONG_WORLD = "wrong-world";

let engine = null;
let world = 0;
let index = -1;
let stale = false;

/// `structuredClone` turns a BigInt seed into a BigInt and a string into a string; both are
/// acceptable to `Engine.newWorld`, which calls `BigInt()` on whatever it is given. The
/// bump for a stale world is done in BigInt so a seed past 2^53 does not round.
function seedPlusOne(seed) {
  return (BigInt(seed) + 1n).toString();
}

async function init(message) {
  index = message.index;
  engine = await Engine.load(message.wasmUrl);
  const spec = { ...message.spec };
  // `wrong-world` makes *every* worker wrong -- the whole planet is a different one.
  // `stale-worker` makes exactly one worker wrong, which is the version-skew shape: most
  // tiles are right, a scattered eighth of them are not, and nothing looks broken.
  stale = message.fault === FAULT_WRONG_WORLD
    || (message.fault === FAULT_STALE_WORKER && index === 0);
  if (stale) spec.seed = seedPlusOne(spec.seed);
  const built = performance.now();
  world = engine.newWorld(spec);
  return {
    type: "ready",
    index,
    world,
    stale,
    generatorVersion: engine.generatorVersion(),
    buildMs: performance.now() - built,
  };
}

function fill(message) {
  const started = performance.now();
  const heights = engine.fillTileF32({ ...message.request, handle: world });
  const fillMs = performance.now() - started;
  return { message: { type: "tile", id: message.id, index, fillMs, heights }, heights };
}

/// Rasterise one relief tile. `message.request` is `relief.js`'s own argument object minus
/// the two things only this side has: the engine instance and the world handle.
///
/// The world handle is supplied HERE rather than sent, for the same reason `fill` does it:
/// a handle is an index into a table inside *this* instance's linear memory and means
/// nothing in another. A request that carried one would be reading someone else's world.
function relief(message) {
  const started = performance.now();
  // **The lake counter crosses the wire with the pixels.** A worker that received an empty
  // manifest draws exactly the same bytes as one that received a full manifest for a tile with
  // no water in it, so the picture cannot distinguish the two; this count can. The main thread
  // accumulates it into the provider's stats, where a check reads it.
  const counters = { lakeTexels: 0, lakeTiles: 0, climateMs: 0, climateSamples: 0 };
  const imageData = reliefTile({ ...message.request, engine, worldHandle: world, counters });
  const fillMs = performance.now() - started;
  return {
    message: {
      type: "relief",
      id: message.id,
      index,
      fillMs,
      lakeTexels: counters.lakeTexels,
      lakeTiles: counters.lakeTiles,
      // The climate half of `fillMs`, so the report can say what fraction of a relief tile
      // the fourth pool consumer costs. Same hop, same hazard as the lake counters above.
      climateMs: counters.climateMs,
      climateSamples: counters.climateSamples,
      data: imageData.data,
      width: imageData.width,
      height: imageData.height,
    },
    buffer: imageData.data.buffer,
  };
}

/// **Calibrate this world's climate band edges** -- the fifth job, and the second that is not
/// a tile.
///
/// `wb_climate_calibration` is 4,000 elevations plus one upwind march at every land point:
/// roughly 190,000 elevation queries, and seconds rather than milliseconds. It is here for
/// exactly the reason `water` is: on the main thread it is a single uninterruptible task that
/// freezes the tab, and the browser's own long-task observer records it as one.
///
/// **The world handle is supplied HERE and not sent**, as every other job does it, and here it
/// has the same teeth `water`'s does: the main thread is about to hand these edges to every
/// relief worker, so the only thing making them right is that `world` was built from the spec
/// the main thread built its own world from.
///
/// **No world is built and none is freed.**
function climate(message) {
  const started = performance.now();
  const result = engine.climateCalibration({ ...message.request, handle: world });
  const fillMs = performance.now() - started;
  return {
    type: "climate",
    id: message.id,
    index,
    fillMs,
    moistureEdges: result.moistureEdges,
    landformEdges: result.landformEdges,
    landSamples: result.landSamples,
    lapseCPerKm: result.lapseCPerKm,
    worldCount: engine.worldCount(),
  };
}

/// Rasterise one cloud tile. **The third job, and the only one that does not touch the engine
/// or the world handle at all** -- the cloud field is a point function of position, so unlike
/// `fill` and `relief` there is nothing here for a stale world to be stale about. That is worth
/// stating rather than leaving as an omission: a reader who has just read `relief` above will
/// look for the `handle: world` this one does not have.
///
/// It is still dispatched through the same pool as the other two, because the contention that
/// matters is CPU per core rather than which job is running on it.
function cloud(message) {
  const started = performance.now();
  const imageData = cloudTile(message.request);
  const fillMs = performance.now() - started;
  return {
    message: {
      type: "cloud",
      id: message.id,
      index,
      fillMs,
      data: imageData.data,
      width: imageData.width,
      height: imageData.height,
    },
    buffer: imageData.data.buffer,
  };
}

/// **Solve the water manifest against THIS worker's own world.**
///
/// # The fourth job, and the only one that is not a tile
///
/// `wb_water_run` was called on the main thread and measured 33.9--44.8 s at boot and
/// 45.4--53.4 s per live swap, recorded by the browser's own `longtask` observer as a single
/// task each time. Moving it here is a message type rather than an algorithm: the export
/// already existed, and this worker already holds a world built from the same spec.
///
/// **The world handle is supplied HERE and not sent, exactly as `fill` and `relief` do it**,
/// and here the reason has teeth: the main thread is about to draw lakes it did not compute,
/// so the only thing making the answer right is that `world` was built from the spec the main
/// thread built its own world from. `pool.rebuild` is awaited before this job is dispatched,
/// which is what makes that true. The `stale-worker` fault deliberately breaks it, and
/// `main.js` therefore keeps the main-thread solve whenever a fault is selected.
///
/// **No world is built and none is freed**, so `wb_world_count` must read exactly what it read
/// before. It is sent back with the manifest because that is the only place a per-worker count
/// can be taken from -- the handle table lives in this instance's linear memory -- and because
/// "a worker that solves water must not leak a world" is a claim that needs a number rather
/// than a reading of this function.
///
/// **Nothing is transferred.** The reply is a plain array of small objects, which
/// `structuredClone` copies; there is no `ArrayBuffer` here to move. 963 bodies on the owner's
/// world is a copy measured in single-digit milliseconds against a solve measured in tens of
/// seconds.
function water(message) {
  const started = performance.now();
  const result = engine.waterRun({ ...message.request, handle: world });
  const fillMs = performance.now() - started;
  return {
    type: "water",
    id: message.id,
    index,
    fillMs,
    bodies: result.bodies,
    seaLevelM: result.seaLevelM,
    worldCount: engine.worldCount(),
  };
}

/// **Bake this world's hydrology in a worker.** Mirrors `water` above: not a tile, one long
/// job (about 87 s at a million nodes on the owner's world), and this worker's own world is
/// what makes the answer right, for the same reason `water`'s comment gives.
///
/// **The reply transfers its buffer.** Unlike `water`'s manifest, `hydroBake` already copies
/// the record off the wasm heap onto a fresh `Float64Array` (see `engine.js`'s own comment on
/// that copy), so there is a real `ArrayBuffer` here to move instead of clone -- a schema-2
/// record at a million nodes is not the tens of small objects `water`'s manifest is.
function hydro(message) {
  const started = performance.now();
  const words = engine.hydroBake({ handle: world, params: message.request.params });
  const fillMs = performance.now() - started;
  return { type: "hydro", id: message.id, index, fillMs, words };
}

self.onmessage = async (event) => {
  const message = event.data;
  try {
    if (message.type === "init") {
      self.postMessage(await init(message));
      return;
    }
    if (message.type === "fill") {
      const { message: reply, heights } = fill(message);
      self.postMessage(reply, [heights.buffer]);
      return;
    }
    if (message.type === "relief") {
      const { message: reply, buffer } = relief(message);
      self.postMessage(reply, [buffer]);
      return;
    }
    if (message.type === "cloud") {
      const { message: reply, buffer } = cloud(message);
      self.postMessage(reply, [buffer]);
      return;
    }
    // No transfer list: see `water` above. A transfer list naming a buffer this reply does not
    // have would throw, and one naming nothing is what a reader would copy from the branches
    // above without noticing the difference.
    if (message.type === "water") {
      self.postMessage(water(message));
      return;
    }
    // No transfer list either: eight f64 in a plain object. See `climate` above.
    if (message.type === "climate") {
      self.postMessage(climate(message));
      return;
    }
    if (message.type === "hydro") {
      const reply = hydro(message);
      self.postMessage(reply, [reply.words.buffer]);
      return;
    }
    // **The live swap's worker half.** Each worker holds its own world in its own linear memory,
    // so a main-thread swap that did not reach here would leave eight workers filling tiles from
    // the previous planet -- which is precisely the `stale-worker` fault, arrived at by accident
    // and looking entirely plausible.
    //
    // The engine instance is REUSED: `Engine.load` is the expensive half of `init` (a fetch and a
    // `WebAssembly.instantiate` per worker) and nothing about it depends on the spec. Rebuilding
    // is `Surface::new`, measured at 2.2--3.2 ms.
    //
    // **The old world is freed after the new one is built, not before.** A refused spec then
    // leaves this worker able to keep answering with the world it already had, instead of holding
    // handle 0 and returning NaN at every post. Same ordering, same reason, as
    // `live-swap.js::WorldSwapper`.
    //
    // `stale` is re-applied rather than re-decided: a `stale-worker` fault chosen at boot must
    // survive a swap, or the fault would quietly heal itself the first time a slider moved.
    if (message.type === "rebuild") {
      if (!engine) throw new Error("rebuild before init");
      const spec = { ...message.spec };
      if (stale) spec.seed = seedPlusOne(spec.seed);
      const started = performance.now();
      const next = engine.newWorld(spec);
      const previous = world;
      world = next;
      if (previous) engine.freeWorld(previous);
      self.postMessage({
        type: "rebuilt",
        id: message.id,
        index,
        world,
        stale,
        // The engine's own live-world count, read after the free. A worker that leaked a world
        // per swap shows this climbing, and it is the only place the per-worker figure can be
        // taken from -- each worker's handle table lives in its own linear memory.
        worldCount: engine.worldCount(),
        buildMs: performance.now() - started,
      });
      return;
    }
    if (message.type === "free") {
      if (engine && world) engine.freeWorld(world);
      world = 0;
      self.postMessage({ type: "freed", index });
      return;
    }
    throw new Error(`unknown message type ${message.type}`);
  } catch (error) {
    self.postMessage({
      type: "error",
      id: message && message.id,
      index,
      message: String(error && error.stack ? error.stack : error),
    });
  }
};
