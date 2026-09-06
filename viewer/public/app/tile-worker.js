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
// # Three jobs: heights, relief, and clouds -- but only two of them are about the world
//
// `cloud` is the third and it takes no world handle. See its own comment below.
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
  const counters = { lakeTexels: 0, lakeTiles: 0 };
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
      data: imageData.data,
      width: imageData.width,
      height: imageData.height,
    },
    buffer: imageData.data.buffer,
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
