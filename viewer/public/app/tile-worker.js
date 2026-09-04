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

import { Engine } from "./engine.js";

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
