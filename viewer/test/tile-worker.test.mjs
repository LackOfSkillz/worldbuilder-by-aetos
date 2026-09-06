// Node-native tests for tile-worker.js -- the module that runs inside each pool worker.
//
// **Why this file exists, and why it is not "a worker cannot be tested in node".** Task 4
// moved relief rasterisation off the main thread by adding a second job to this worker. The
// slice's own standing lesson is that *dead code looks like a feature*: Task 5 found all
// three colour blends in `relief.js` had been written, committed, and never once selected
// against this terrain. A new `if (message.type === "relief")` branch is exactly that shape
// -- if the pool never sends the message, or sends it under another name, the viewer falls
// back to the parent texture and looks merely slow rather than broken. So the branch is
// exercised here, against the real wasm, rather than assumed.
//
// **How a worker module runs under `node --test`.** `tile-worker.js` touches exactly two
// worker globals: it reads `self.postMessage` and it assigns `self.onmessage`. Both are
// supplied below, so importing the module installs its real handler and the tests drive it
// by calling that handler directly -- the same function the browser's message loop calls,
// with the same message objects `pool.js` posts. `Engine.load` fetches its wasm, so `fetch`
// is stubbed to serve the checked-in file from disk; nothing else is faked.
//
// Population/method/host, named once:
//   - World: Surface::new(20260904, 6_371_000, 12, 0.29, None) -- DEFAULT_WORLD in main.js.
//   - Host: node v22.x, this repository's checked-in
//     viewer/public/wasm/worldbuilder_engine.wasm.
//   - Tile: level 2 of a 8x4 GeographicTilingScheme grid, stated in degrees rather than
//     built from Cesium, because this module never sees a tiling scheme.

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const WASM_PATH = fileURLToPath(
  new URL("../public/wasm/worldbuilder_engine.wasm", import.meta.url),
);
const WASM_BYTES = readFileSync(WASM_PATH);

// `Engine.load` does `fetch(url)` and then `instantiateStreaming` with an
// `arrayBuffer()` fallback. Serving the real file keeps the engine real; only the transport
// is replaced.
globalThis.fetch = async () =>
  new Response(WASM_BYTES, { headers: { "content-type": "application/wasm" } });

/// Everything the worker posts, in order, with the transfer list it asked for. The transfer
/// list is not decoration: without it `postMessage` COPIES the 262,144-byte raster per tile
/// instead of moving it, which is a silent regression that renders identically.
const posted = [];
globalThis.self = {
  postMessage(message, transfer) {
    posted.push({ message, transfer });
  },
};

const workerUrl = new URL("../public/app/tile-worker.js", import.meta.url);
await import(workerUrl.href);
const handle = globalThis.self.onmessage;

const DEFAULT_WORLD = { seed: 20260904, radiusM: 6371000, plateCount: 12, landFraction: 0.29 };

// Level 2 of a GeographicTilingScheme is exactly 45 deg x 45 deg, so this rectangle is
// stated rather than derived -- `tile-worker.js` has no tiling scheme and no Cesium.
const RECTANGLE = { northDeg: 45, southDeg: 0, westDeg: -45, eastDeg: 0 };

/// The main-thread engine the worker's answers are compared against. Built from the same
/// spec, in this process, so a worker that quietly built a different world is visible.
const { Engine } = await import("../public/app/engine.js");
const { reliefTile } = await import("../public/app/relief.js");
const { calibrateClouds, cloudTile } = await import("../public/app/clouds.js");

/// The cloud calibration the cloud-branch tests below post. Built here, on the main thread, the
/// way `cloud-provider.js` builds it -- the worker is handed the result rather than computing
/// its own, and that is the property these tests are asserting the wire honours.
const CLOUD_CALIBRATION = calibrateClouds({
  radiusM: DEFAULT_WORLD.radiusM, seed: DEFAULT_WORLD.seed, cover: 0.4, samples: 4000,
});

let reference;
let referenceWorld;

test.before(async () => {
  const { instance } = await WebAssembly.instantiate(WASM_BYTES, {});
  reference = new Engine(instance);
  referenceWorld = reference.newWorld(DEFAULT_WORLD);

  posted.length = 0;
  await handle({
    data: { type: "init", index: 3, wasmUrl: WASM_PATH, fault: null, spec: DEFAULT_WORLD },
  });
  assert.equal(posted.length, 1, "init must answer exactly once");
  assert.equal(posted[0].message.type, "ready", `init failed: ${posted[0].message.message}`);
  posted.length = 0;
});

test.after(() => {
  if (reference && referenceWorld) reference.freeWorld(referenceWorld);
});

/// Drive one message and return the single reply.
async function send(message) {
  posted.length = 0;
  await handle({ data: message });
  assert.equal(posted.length, 1, `expected exactly one reply to ${message.type}`);
  return posted[0];
}

test("the relief branch is REACHED, and answers with a raster of the requested size", async () => {
  const { message } = await send({
    type: "relief",
    id: 7,
    request: { rectangle: RECTANGLE, level: 2, size: 32, radiusM: DEFAULT_WORLD.radiusM },
  });
  assert.equal(
    message.type, "relief",
    "the worker answered something other than a relief raster -- if this is 'error', the " +
    "branch exists but throws; if it is 'tile', the message type is being matched wrongly",
  );
  assert.equal(message.id, 7, "the id must come back or pool.js can never settle the promise");
  assert.equal(message.index, 3, "the reply carries the worker's own index");
  assert.equal(message.width, 32);
  assert.equal(message.height, 32);
  assert.equal(message.data.length, 32 * 32 * 4, "RGBA, one byte per channel");
  assert.ok(
    Number.isFinite(message.fillMs) && message.fillMs >= 0,
    `fillMs must be a real duration; got ${message.fillMs}. It is the only measurement of ` +
    "the cost that was moved, and a missing one reads as a free lunch",
  );
});

test("the relief reply carries the lake counts the main thread cannot recompute", async () => {
  // **The counter has to cross the wire or it does not exist.** The provider accumulates
  // `stats.lakeTexels` from this reply; a worker that drew the lakes and did not report them
  // would render a perfectly correct globe and tell the check that nothing was drawn -- which is
  // exactly the "byte-identity proves the picture, never the path" failure, arrived at from the
  // other side. The synthetic body floods the tile so the count cannot be zero by accident.
  const flood = [{
    rootNode: 1, kind: 0, levelM: 10000,
    minLatitudeDeg: -90, maxLatitudeDeg: 90, minLongitudeDeg: -180, maxLongitudeDeg: 180,
  }];
  const base = { rectangle: RECTANGLE, level: 2, size: 24, radiusM: DEFAULT_WORLD.radiusM };
  const dry = await send({ type: "relief", id: 21, request: base });
  assert.equal(dry.message.lakeTexels, 0, "water was drawn with no manifest in the request");
  assert.equal(dry.message.lakeTiles, 0);
  const wet = await send({ type: "relief", id: 22, request: { ...base, lakes: flood } });
  assert.equal(wet.message.lakeTiles, 1, "the body was not even considered for this tile");
  assert.ok(
    wet.message.lakeTexels > 0,
    "the reply reports no lake texels for a body covering the whole planet",
  );
  // ...and the pixels moved too, so the count is not a number invented beside an unchanged raster.
  assert.notDeepEqual(Array.from(wet.message.data), Array.from(dry.message.data));
});

test("the raster's buffer is TRANSFERRED, not copied", async () => {
  // 256 x 256 x 4 = 262,144 bytes per tile, and an orbital view asks for 26 to 79 of them.
  // Omitting the transfer list is invisible -- same pixels, same code path -- and costs a
  // structured-clone copy of every one of them.
  const { message, transfer } = await send({
    type: "relief",
    id: 8,
    request: { rectangle: RECTANGLE, level: 2, size: 16, radiusM: DEFAULT_WORLD.radiusM },
  });
  assert.ok(Array.isArray(transfer), "postMessage must be given a transfer list");
  assert.equal(transfer.length, 1);
  assert.equal(
    transfer[0], message.data.buffer,
    "the transferred object must be the raster's own ArrayBuffer",
  );
});

test("the worker's raster is byte-identical to the main thread's for the same tile", async () => {
  // This is the assertion that says the MOVE changed nothing. It also catches the whole
  // family of request-mapping faults at once: a dropped `level`, a `size` read as `width`,
  // a lost `radiusM`, or a worker whose world is one seed away (the `stale-worker` shape)
  // all produce a plausible raster that is not this one.
  const request = {
    rectangle: RECTANGLE, level: 2, size: 24, radiusM: DEFAULT_WORLD.radiusM,
  };
  const { message } = await send({ type: "relief", id: 9, request });
  const expected = reliefTile({
    ...request, engine: reference, worldHandle: referenceWorld,
  });
  assert.deepEqual(
    Array.from(message.data), Array.from(expected.data),
    "the worker's raster differs from the main thread's for the same tile -- the pool path " +
    "and the ?workers=0 baseline are not drawing the same planet",
  );
});

test("two different rectangles produce different rasters -- the request is actually read", async () => {
  // A worker that ignored `request.rectangle` and always rasterised the same patch would
  // pass every assertion above except this one, and would render a globe tiled with one
  // repeated image that still has real relief structure in it.
  const base = { level: 2, size: 16, radiusM: DEFAULT_WORLD.radiusM };
  const a = await send({ type: "relief", id: 10, request: { ...base, rectangle: RECTANGLE } });
  const b = await send({
    type: "relief",
    id: 11,
    request: { ...base, rectangle: { northDeg: 0, southDeg: -45, westDeg: 90, eastDeg: 135 } },
  });
  assert.notDeepEqual(
    Array.from(a.message.data), Array.from(b.message.data),
    "two different rectangles produced byte-identical rasters",
  );
});

test("a relief request that throws comes back as an error reply carrying its id", async () => {
  // Cesium's own failure path depends on this: `requestImage`'s promise must REJECT, which
  // means the pool must settle it, which means the worker must answer even when the
  // rasteriser throws. A worker that let the exception escape would leave the pending entry
  // in `pool.js` forever and the tile permanently blank, with no error anywhere.
  const { message } = await send({
    type: "relief",
    id: 12,
    // marginedTileRequest refuses size < 2, which is a real refusal rather than a contrived
    // one: it is the guard that stops a degenerate grid reaching the engine.
    request: { rectangle: RECTANGLE, level: 2, size: 1, radiusM: DEFAULT_WORLD.radiusM },
  });
  assert.equal(message.type, "error");
  assert.equal(message.id, 12, "the id must survive the error path or the promise never settles");
  assert.match(message.message, /size must be an integer/);
});

test("fill still works, and is unchanged by the second job", async () => {
  const { message, transfer } = await send({
    type: "fill",
    id: 13,
    request: {
      lat0Deg: 45, lat1Deg: 0, lon0Deg: -45, lon1Deg: 0, width: 8, height: 8, resolutionM: null,
    },
  });
  assert.equal(message.type, "tile");
  assert.equal(message.id, 13);
  assert.equal(message.heights.length, 64);
  assert.equal(transfer[0], message.heights.buffer);
});

// -----------------------------------------------------------------------------------------
// The THIRD job: clouds.
//
// Same argument as the relief branch above, one slice later. `if (message.type === "cloud")` is
// exactly the shape that fails silently: if `pool.js` never sends the message, or sends it under
// another name, the imagery layer falls back to the parent texture and the planet looks merely
// cloudless -- which is what it looked like before this task, so nothing about the picture would
// say the branch was dead.

test("the cloud branch is REACHED, and answers with a raster of the requested size", async () => {
  const { message } = await send({
    type: "cloud",
    id: 21,
    request: { rectangle: RECTANGLE, level: 3, size: 32, clouds: CLOUD_CALIBRATION },
  });
  assert.equal(
    message.type, "cloud",
    "the worker answered something other than a cloud raster -- if this is 'error' the branch " +
    "exists but throws; if it is 'relief' the message type is being matched wrongly",
  );
  assert.equal(message.id, 21, "the id must come back or pool.js can never settle the promise");
  assert.equal(message.index, 3);
  assert.equal(message.width, 32);
  assert.equal(message.height, 32);
  assert.equal(message.data.length, 32 * 32 * 4);
  assert.ok(Number.isFinite(message.fillMs) && message.fillMs >= 0);
});

test("the cloud raster's buffer is TRANSFERRED, not copied", async () => {
  const { message, transfer } = await send({
    type: "cloud", id: 22,
    request: { rectangle: RECTANGLE, level: 3, size: 32, clouds: CLOUD_CALIBRATION },
  });
  // The stub `postMessage` records the transfer list rather than performing the transfer, so
  // what is asserted is that the worker ASKED for it -- omitting it is invisible (same pixels,
  // same code path) and costs a structured-clone copy of 65,536 bytes per tile.
  assert.ok(Array.isArray(transfer), "postMessage must be given a transfer list");
  assert.equal(transfer.length, 1);
  assert.equal(transfer[0], message.data.buffer, "the transferred object must be the raster's own ArrayBuffer");
});

test("the worker's cloud raster is byte-identical to the main thread's", async () => {
  const { message } = await send({
    type: "cloud", id: 23,
    request: { rectangle: RECTANGLE, level: 3, size: 24, clouds: CLOUD_CALIBRATION },
  });
  const expected = cloudTile({ rectangle: RECTANGLE, level: 3, size: 24, clouds: CLOUD_CALIBRATION });
  assert.deepEqual(Array.from(message.data), Array.from(expected.data));
});

test("the cloud job takes no world handle and does not disturb the world", async () => {
  // The cloud field is a point function of position: it reads neither the engine nor the world.
  // A `fill` before and after a `cloud` must answer the same heights -- which is what would fail
  // if the cloud branch ever grew an engine call and left a handle in a different state.
  const request = {
    lat0Deg: 45, lat1Deg: 0, lon0Deg: -45, lon1Deg: 0, width: 8, height: 8, resolutionM: null,
  };
  const before = await send({ type: "fill", id: 24, request });
  const heights = Array.from(before.message.heights);
  await send({
    type: "cloud", id: 25,
    request: { rectangle: RECTANGLE, level: 3, size: 16, clouds: CLOUD_CALIBRATION },
  });
  const after = await send({ type: "fill", id: 26, request });
  assert.deepEqual(Array.from(after.message.heights), heights);
});

test("a cloud request that throws comes back as an error reply carrying its id", async () => {
  const { message } = await send({ type: "cloud", id: 27, request: { rectangle: RECTANGLE, size: 16 } });
  assert.equal(message.type, "error");
  assert.equal(message.id, 27);
  assert.match(message.message, /calibrateClouds/);
});

// =========================================================================================
// The water branch -- the fourth job, and the one the whole cold load was
// =========================================================================================
//
// **This is the proof that moving the solve off the main thread did not move the answer.**
// The brief's rule is "prove the manifest matches, do not assume it", and the assumption
// available to be made here is a large one: the worker solves against a world IT built, from a
// spec it was posted, in a different linear memory, and hands back rows that crossed a
// `structuredClone`. Every one of those is a place the answer could change while the globe
// still looked like a globe -- a lake in the wrong place is a lake.
//
// `referenceWorld` is built in THIS process from the same `DEFAULT_WORLD` the worker was
// `init`ed with, so the comparison is main-thread against worker, not worker against itself.
// 4,000 nodes rather than the owner's 86,000: the node count is what the solve is affine in
// (2,310 ms at 8,000 against 31,418 ms at 86,000), and nothing about the transport changes
// with it.

const WATER_NODES = 4000;

test("the water branch is REACHED, and answers with a manifest", async () => {
  const { message, transfer } = await send({
    type: "water", id: 31, request: { nodeCount: WATER_NODES },
  });
  assert.equal(
    message.type, "water",
    "the worker answered something other than a manifest -- if this is 'error' the branch " +
    `exists but throws: ${message.message}`,
  );
  assert.equal(message.id, 31, "the id must come back or pool.js can never settle the promise");
  assert.equal(message.index, 3, "the reply carries the worker's own index");
  assert.ok(Array.isArray(message.bodies) && message.bodies.length > 0,
    "a 4,000-node solve of this world has bodies; an empty manifest here would draw a lakeless " +
    "planet and raise nothing");
  assert.ok(Number.isFinite(message.seaLevelM), "the datum must cross the wire as a number");
  assert.ok(Number.isFinite(message.fillMs) && message.fillMs >= 0,
    `fillMs must be a real duration; got ${message.fillMs}`);
  assert.equal(transfer, undefined,
    "there is no ArrayBuffer in this reply; a transfer list naming one would throw, and the " +
    "three branches above all pass one, so copying them is the easy mistake");
});

test("the worker's manifest is IDENTICAL to a main-thread solve of the same world", async () => {
  // **The datum is requested NON-ZERO on purpose, and that is a correction rather than a
  // flourish.** Written with the default `seaLevelM: 0`, this test could not fail on the datum:
  // a worker replying with a hard-coded `0` passed it, because 0 is what this world's manifest
  // comes back at anyway. That is the fifteenth assertion in this project to look load-bearing
  // and not be, and it was caught by mutating the worker to reply `seaLevelM: 0` and watching
  // the suite stay green. -250 m also moves the manifest itself (6 bodies at 0, 7 at -250), so
  // the same argument proves the whole request crosses rather than only the node count.
  const request = { nodeCount: WATER_NODES, seaLevelM: -250 };
  const truth = reference.waterRun({ handle: referenceWorld, ...request });
  assert.notEqual(truth.seaLevelM, 0,
    "the datum under comparison is 0, so an implementation that returned a constant 0 would " +
    "pass this test; the request is what makes the comparison able to fail");
  const { message } = await send({ type: "water", id: 32, request });
  assert.equal(
    message.bodies.length, truth.bodies.length,
    "a different body COUNT means the worker solved a different world, not a rounding " +
    "difference -- check that init's spec reached engine.newWorld intact",
  );
  assert.ok(Object.is(message.seaLevelM, truth.seaLevelM),
    `the datum differs: worker ${message.seaLevelM}, main thread ${truth.seaLevelM}`);
  // Field by field and `Object.is`, not `deepEqual` on the arrays: a bare deepEqual would pass
  // on two empty arrays, and the failure message from one would name neither the row nor the
  // field. Every field the export writes is compared -- there are seven and the stride says so.
  const FIELDS = ["rootNode", "kind", "levelM",
    "minLatitudeDeg", "maxLatitudeDeg", "minLongitudeDeg", "maxLongitudeDeg"];
  let compared = 0;
  for (let i = 0; i < truth.bodies.length; i += 1) {
    for (const field of FIELDS) {
      assert.ok(
        Object.is(message.bodies[i][field], truth.bodies[i][field]),
        `body ${i} field ${field}: worker ${message.bodies[i][field]}, ` +
        `main thread ${truth.bodies[i][field]}`,
      );
      compared += 1;
    }
  }
  assert.equal(compared, truth.bodies.length * FIELDS.length);
  assert.ok(compared > 0, "nothing was compared, so this test asserted nothing");
});

test("the request's nodeCount is HONOURED, not defaulted", async () => {
  // **A stage can be exercised and its arguments ignored.** If `water()` dropped
  // `message.request` and called `waterRun` with some constant, the test above would still pass
  // -- the reference call would be the constant too only by luck, and here it would not be. Two
  // node counts, two different manifests, each matching its own main-thread solve.
  const coarse = reference.waterRun({ handle: referenceWorld, nodeCount: 2000 });
  const fine = reference.waterRun({ handle: referenceWorld, nodeCount: 6000 });
  assert.notEqual(
    coarse.bodies.length, fine.bodies.length,
    "the two node counts must give different manifests or this test cannot fail",
  );
  const a = await send({ type: "water", id: 33, request: { nodeCount: 2000 } });
  const b = await send({ type: "water", id: 34, request: { nodeCount: 6000 } });
  assert.equal(a.message.bodies.length, coarse.bodies.length);
  assert.equal(b.message.bodies.length, fine.bodies.length);
});

test("solving water builds no world and frees none", async () => {
  // **The live-slider work proved no leak across 30 swaps with `wb_world_count` constant, and a
  // fourth consumer that took a world to solve in would undo that invisibly** -- the render
  // stays perfect right up to the allocation that fails. The count is read from inside the
  // worker's own linear memory, which is the only place the per-worker figure exists.
  const before = await send({ type: "water", id: 35, request: { nodeCount: 1000 } });
  const after = await send({ type: "water", id: 36, request: { nodeCount: 1000 } });
  assert.equal(before.message.worldCount, 1,
    "this worker holds exactly one world; anything else means the solve built one");
  assert.equal(after.message.worldCount, before.message.worldCount,
    "wb_world_count grew across two solves -- that is one leaked world per slider release");
  // And the world is still usable afterwards, which a free-then-solve would break silently.
  const filled = await send({
    type: "fill", id: 37,
    request: { lat0Deg: 45, lat1Deg: 0, lon0Deg: -45, lon1Deg: 0, width: 8, height: 8, resolutionM: null },
  });
  assert.equal(filled.message.type, "tile");
  assert.ok(filled.message.heights.every((h) => Number.isFinite(h)),
    "a fill after a solve returned NaN, which is what handle 0 answers with");
});

test("a water request that throws comes back as an error reply carrying its id", async () => {
  // `WB_MAX_WATER_NODES` is 100,000; the export refuses more, and a refusal has to reach the
  // pool as an error rather than as a promise that never settles.
  const { message } = await send({ type: "water", id: 38, request: { nodeCount: 10_000_000 } });
  assert.equal(message.type, "error");
  assert.equal(message.id, 38);
});

test("an unknown message type is refused rather than silently ignored", async () => {
  const { message } = await send({ type: "rasterise", id: 14 });
  assert.equal(message.type, "error");
  assert.match(message.message, /unknown message type rasterise/);
});
