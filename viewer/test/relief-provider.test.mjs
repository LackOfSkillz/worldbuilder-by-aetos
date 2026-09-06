// Node-native tests for relief-provider.js -- the Cesium `ImageryProvider` that wraps
// Task 1's `reliefTile`. `node:test` + `node:assert/strict`, no framework, no browser.
//
// **Why this can be a node test at all.** `relief-provider.js` reads the same global
// `Cesium` every other module in `viewer/public/app/` reads (the vendored build is the IIFE
// one; there is no bundler). Rather than hand-rolling a fake tiling scheme -- which would
// test the fake -- this file installs the REAL classes from the `@cesium/engine` package
// that `viewer/package.json` already pins at 1.145.0, the same version the vendored
// `public/vendor/cesium/Cesium.js` reports. So `GeographicTilingScheme`,
// `Rectangle`, `Event`, `Credit` and `CesiumMath` below are Cesium's own, and a
// registration assertion here compares the provider against Cesium's own tile geometry.
//
// Population/method/host, named once so every number below can be traced:
//   - World: Surface::new(20260904, 6_371_000, 12, 0.29, None) -- DEFAULT_WORLD in main.js.
//   - Host: node v22.x, this repository's checked-in
//     viewer/public/wasm/worldbuilder_engine.wasm, loaded directly from disk (no fetch).
//   - Tile choice: an 8x4 scan over the LEVEL 2 tiles of a GeographicTilingScheme (which
//     are exactly 45deg x 45deg), scored by (max - min) of five wb_elevation_m samples --
//     8*4*5 = 160 scalar calls, once, in `before()`.

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import {
  GeographicTilingScheme,
  Rectangle,
  Event as CesiumEvent,
  Credit,
  Math as CesiumMath,
} from "@cesium/engine";

// Installed BEFORE the modules under test are imported: they capture nothing at module
// scope, but `terrain.js`'s `tileRectangleDegrees` reads `Cesium.Math` at call time and the
// provider constructs a default tiling scheme at call time, so the global has to be real by
// then. A dynamic import keeps the ordering explicit rather than relying on hoisting.
globalThis.Cesium = {
  GeographicTilingScheme,
  Rectangle,
  Event: CesiumEvent,
  Credit,
  Math: CesiumMath,
};

const { Engine } = await import("../public/app/engine.js");
const { hasStructure } = await import("../public/app/relief.js");
const { MAX_LEVEL } = await import("../public/app/terrain.js");
const {
  createReliefImageryProvider,
  reliefLayerEnabled,
  RELIEF_TILE_SIZE,
} = await import("../public/app/relief-provider.js");

const DEFAULT_WORLD = { seed: 20260904, radiusM: 6371000, plateCount: 12, landFraction: 0.29 };

async function loadEngine() {
  const wasmPath = fileURLToPath(new URL("../public/wasm/worldbuilder_engine.wasm", import.meta.url));
  const bytes = readFileSync(wasmPath);
  const { instance } = await WebAssembly.instantiate(bytes, {});
  return new Engine(instance);
}

let engine;
let world;
let tilingScheme;
/// The level-2 tile with the most relief among its five probe samples, and a flat-ish one.
let mountainTile;
let mountainRelief;

test.before(async () => {
  engine = await loadEngine();
  world = engine.newWorld(DEFAULT_WORLD);
  tilingScheme = new GeographicTilingScheme();

  let bestRelief = -Infinity;
  for (let y = 0; y < 4; y += 1) {
    for (let x = 0; x < 8; x += 1) {
      const r = tilingScheme.tileXYToRectangle(x, y, 2);
      const north = CesiumMath.toDegrees(r.north);
      const south = CesiumMath.toDegrees(r.south);
      const west = CesiumMath.toDegrees(r.west);
      const east = CesiumMath.toDegrees(r.east);
      const samples = [
        engine.elevationM(world, north, west),
        engine.elevationM(world, north, east),
        engine.elevationM(world, south, west),
        engine.elevationM(world, south, east),
        engine.elevationM(world, (north + south) / 2, (west + east) / 2),
      ];
      const relief = Math.max(...samples) - Math.min(...samples);
      if (relief > bestRelief) {
        bestRelief = relief;
        mountainTile = { x, y, level: 2 };
      }
    }
  }
  mountainRelief = bestRelief;
});

test.after(() => {
  if (engine && world) engine.freeWorld(world);
});

function makeProvider(overrides = {}) {
  return createReliefImageryProvider({
    engine,
    worldHandle: world,
    radiusM: DEFAULT_WORLD.radiusM,
    // Keep the node suite cheap: a 256-texel tile is 258^2 = 66,564 engine samples and about
    // 120 ms. The interface assertions do not care about the raster size, and the one test
    // that does raster content says so.
    tileSize: 32,
    // ImageData does not exist under `node --test`; relief.js already returns a plain
    // {data,width,height} there, and the provider's canvas step is the browser's job. The
    // identity conversion is what lets these tests see the raster itself.
    toImage: (imageData) => imageData,
    ...overrides,
  });
}

// ---------------------------------------------------------------------------------------
// The interface sweep.
//
// This is the assertion the two recorded version traps are about. `ready` and
// `readyPromise` were REMOVED from ImageryProvider in Cesium 1.107, so an implementation
// copied from an older tutorial defines two properties nothing reads and omits nothing
// visible -- it looks like it works. The member list below was produced by SWEEPING the
// vendored 1.145.0 tree for `imageryProvider.<member>` reads
// (`grep -rno "imageryProvider\.[A-Za-z_]*" node_modules/@cesium/engine/Source`), not by
// spot-checking a tutorial, and it is the whole public set Cesium actually touches.
const MEMBERS_CESIUM_1_145_READS = [
  "credit",
  "errorEvent",
  "getTileCredits",
  "hasAlphaChannel",
  "maximumLevel",
  "minimumLevel",
  "pickFeatures",
  "proxy",
  "rectangle",
  "requestImage",
  "tileDiscardPolicy",
  "tileHeight",
  "tileWidth",
  "tilingScheme",
];

test("the provider defines every member Cesium 1.145 actually reads", () => {
  const provider = makeProvider();
  const missing = MEMBERS_CESIUM_1_145_READS.filter((name) => !(name in provider));
  assert.deepEqual(
    missing, [],
    `an ImageryProvider member Cesium reads is absent, which fails SILENTLY (undefined), ` +
    `not loudly: ${missing.join(", ")}`,
  );
});

test("the provider does NOT define ready/readyPromise -- both removed in Cesium 1.107", () => {
  const provider = makeProvider();
  assert.equal(
    "ready" in provider, false,
    "`ready` was removed from ImageryProvider in 1.107; defining it is dead weight that " +
    "makes an implementation look like it is doing version-correct work when it is not",
  );
  assert.equal("readyPromise" in provider, false, "`readyPromise` was removed in 1.107");
});

test("credit is a Cesium.Credit and errorEvent is a Cesium.Event", () => {
  const provider = makeProvider();
  assert.ok(provider.credit instanceof Credit, "credit must be a Credit, not a bare string");
  assert.ok(
    provider.errorEvent instanceof CesiumEvent,
    "errorEvent must be a real Event -- ImageryLayer calls raiseEvent/numberOfListeners on it",
  );
});

test("tile size and levels: square tiles, level 0 up to the terrain ground cap", () => {
  const provider = makeProvider({ tileSize: undefined });
  assert.equal(provider.tileWidth, RELIEF_TILE_SIZE);
  assert.equal(provider.tileHeight, RELIEF_TILE_SIZE);
  assert.equal(
    provider.maximumLevel, MAX_LEVEL,
    "imagery must not stop refining before the terrain does, or the colour goes blurry " +
    "exactly where the mesh gets sharper -- which is the complaint this slice exists for",
  );
  assert.equal(provider.minimumLevel, 0);
  assert.ok(
    Rectangle.equals(provider.rectangle, provider.tilingScheme.rectangle),
    "the provider covers the whole tiling scheme; a smaller rectangle leaves unpainted globe",
  );
  assert.equal(
    provider.hasAlphaChannel, false,
    "relief.js writes alpha 255 everywhere, so declaring an alpha channel would upload a " +
    "byte per texel that is always 255",
  );
});

test("requestImage returns a PROMISE, not a bare image", async () => {
  // ImageryLayer._requestImagery does `imagePromise.then(...)` on whatever comes back,
  // guarded only by `defined()`. A provider that returns a canvas directly therefore throws
  // "imagePromise.then is not a function" from inside Cesium, per tile, forever -- and a
  // provider that returns ImageData (which is what relief.js produces) is accepted by that
  // line and then fails much later at texture upload. Both are worth pinning here.
  const provider = makeProvider();
  const result = provider.requestImage(mountainTile.x, mountainTile.y, mountainTile.level);
  assert.ok(
    result && typeof result.then === "function",
    `requestImage must return a thenable; got ${Object.prototype.toString.call(result)}`,
  );
  const image = await result;
  assert.equal(image.width, 32);
  assert.equal(image.height, 32);
});

test("requestImage's raster has real relief structure, not flat grey", async () => {
  const provider = makeProvider({ tileSize: 128 });
  const image = await provider.requestImage(mountainTile.x, mountainTile.y, mountainTile.level);
  const check = hasStructure(image);
  assert.ok(
    check.ok,
    `level-2 tile (${mountainTile.x},${mountainTile.y}), relief ${mountainRelief.toFixed(1)} m ` +
    `across its five probes, produced a raster hasStructure refused: ${JSON.stringify(check.stats)}`,
  );
});

test("registration: the rectangle the provider rasterises is Cesium's own tile rectangle", () => {
  // A one-post registration error renders perfectly and is invisible by eye -- that is why
  // `FAULTS.shiftTile` exists for the terrain provider. The imagery layer can make exactly
  // the same mistake, and it would show up as relief that is subtly offset from the mesh it
  // is draped on. Compared against Cesium's own conversion, not against a restated formula.
  const provider = makeProvider();
  for (const [x, y, level] of [[0, 0, 0], [1, 0, 0], [5, 2, 3], [130, 61, 7]]) {
    const got = provider.worldbuilder.rectangleDegrees(x, y, level);
    const want = provider.tilingScheme.tileXYToRectangle(x, y, level);
    assert.equal(got.northDeg, CesiumMath.toDegrees(want.north), `north of ${x},${y},${level}`);
    assert.equal(got.southDeg, CesiumMath.toDegrees(want.south), `south of ${x},${y},${level}`);
    assert.equal(got.westDeg, CesiumMath.toDegrees(want.west), `west of ${x},${y},${level}`);
    assert.equal(got.eastDeg, CesiumMath.toDegrees(want.east), `east of ${x},${y},${level}`);
  }
});

test("two different tiles produce different rasters -- the swap guard", async () => {
  // Task 2's lesson, applied one layer up: a provider that ignores x/y (or reads them in
  // the wrong order) still returns a plausible, structured raster for every request. A
  // mean-based check would not see it; comparing whole buffers does.
  const provider = makeProvider({ tileSize: 64 });
  const a = await provider.requestImage(mountainTile.x, mountainTile.y, 2);
  const b = await provider.requestImage((mountainTile.x + 4) % 8, mountainTile.y, 2);
  assert.notDeepEqual(
    Array.from(a.data), Array.from(b.data),
    "two different level-2 tiles produced byte-identical rasters -- the provider is not " +
    "using x/y to place the tile",
  );
});

test("getTileCredits returns an array, and pickFeatures returns undefined", () => {
  const provider = makeProvider();
  assert.ok(Array.isArray(provider.getTileCredits(0, 0, 0)));
  assert.equal(
    provider.pickFeatures(0, 0, 0, 0, 0), undefined,
    "there is nothing to pick on a relief raster; undefined is Cesium's 'not supported'",
  );
});

// ---------------------------------------------------------------------------------------
// `?relief=0`.

test("reliefLayerEnabled: on by default, off only for the exact string 0", () => {
  const on = (query) => reliefLayerEnabled(new URLSearchParams(query));
  assert.equal(on(""), true, "the relief layer is the point of this slice; it is on by default");
  assert.equal(on("relief=0"), false, "?relief=0 must turn the layer off");
  assert.equal(on("relief=1"), true);
  // Same shape as main.js's existing `params.get("flat") !== "1"` and `paint !== "0"`
  // switches -- one convention in the file, not two.
  assert.equal(on("relief=false"), true, "only the literal 0 is off, matching ?paint=0/?flat=1");
  assert.equal(on("seed=7"), true);
});

// ---------------------------------------------------------------------------------------
// TASK 4: the rasterisation moved into the worker pool.
//
// The provider is handed a `pool` and stops calling `reliefTile` itself. What is asserted
// here is the seam: what crosses to the worker, that nothing rasterises on this side any
// more, that the pixels did not change, and that the two costs are reported as two numbers
// rather than added together.
//
// The fake pool below rasterises in-process, which is what makes the byte-identity
// comparison possible at all in node. It is NOT a stand-in for the worker's own message
// handler -- that is exercised against the real wasm in `tile-worker.test.mjs`, which
// drives `tile-worker.js`'s actual `onmessage`.

const { reliefTile } = await import("../public/app/relief.js");

/// A pool that answers `relief(request)` the way `tile-worker.js` does: by spreading the
/// request over `reliefTile` and supplying the engine and the world handle from its own
/// side. Records every request so the wire format can be asserted.
function fakePool({ fillMs = 190, reject = null } = {}) {
  return {
    requests: [],
    relief(request) {
      this.requests.push(request);
      if (reject) return Promise.reject(reject);
      // **The fake must reply with the same fields the real worker replies with.** `relief` in
      // `tile-worker.js` hands `reliefTile` a counters object and puts the two lake counts in the
      // message; a fake that omitted them would let a provider that never accumulates them pass.
      const counters = { lakeTexels: 0, lakeTiles: 0 };
      const imageData = reliefTile({ ...request, engine, worldHandle: world, counters });
      return Promise.resolve({
        data: imageData.data,
        width: imageData.width,
        height: imageData.height,
        fillMs,
        worker: 2,
        lakeTexels: counters.lakeTexels,
        lakeTiles: counters.lakeTiles,
      });
    },
  };
}

test("with a pool, NOTHING rasterises on the main thread", async () => {
  // The counter, not the picture: a provider that ignored the pool would render exactly the
  // same globe and quote exactly the same `meanMs`, because it would be measuring the path
  // it should no longer be on.
  const pool = fakePool();
  const provider = makeProvider({ pool, tileSize: 32 });
  await provider.requestImage(mountainTile.x, mountainTile.y, 2);
  const { stats } = provider.worldbuilder;
  assert.equal(
    stats.mainThreadRasters, 0,
    "a relief tile was rasterised on the main thread despite a pool being present -- this " +
    "is the whole of Task 4, and it fails silently",
  );
  assert.equal(stats.poolRasters, 1);
  assert.equal(pool.requests.length, 1, "the pool must actually have been asked");
});

test("the request sent to the pool is structured-cloneable and carries no engine", async () => {
  // `postMessage` structured-clones its argument. An `Engine` holds a `WebAssembly.Instance`
  // and functions, which throws `DataCloneError` -- once per tile, from inside the pool,
  // where it reads as a worker fault rather than as a provider bug. A world HANDLE clones
  // fine and is worse: it is an index into a table inside one wasm instance's linear
  // memory, so it would silently name a different world in the worker.
  const pool = fakePool();
  const provider = makeProvider({ pool, tileSize: 16 });
  await provider.requestImage(3, 1, 2);
  const [request] = pool.requests;
  assert.equal("engine" in request, false, "an Engine is not structured-cloneable");
  assert.equal(
    "worldHandle" in request, false,
    "a world handle is meaningless outside the instance that issued it; the worker must " +
    "supply its own",
  );
  assert.doesNotThrow(
    () => structuredClone(request),
    "the request must survive postMessage; structuredClone is the same algorithm",
  );
  assert.equal(request.size, 16, "the tile size must cross, or the worker guesses at 256");
  assert.equal(request.level, 2);
  assert.equal(request.radiusM, DEFAULT_WORLD.radiusM);
  assert.deepEqual(
    request.rectangle, provider.worldbuilder.rectangleDegrees(3, 1, 2),
    "the rectangle that crosses must be Cesium's own tile rectangle for that x/y/level",
  );
});

test("the pool path and the ?workers=0 path produce byte-identical rasters", async () => {
  // The claim Task 4 has to earn: the cost moved and the picture did not. Byte comparison,
  // not a mean -- Task 2's lesson about the plausible mutation is that a statistic can be
  // preserved by a change that destroys the meaning.
  const viaPool = makeProvider({ pool: fakePool(), tileSize: 24 });
  const viaMain = makeProvider({ tileSize: 24 });
  const a = await viaPool.requestImage(mountainTile.x, mountainTile.y, 2);
  const b = await viaMain.requestImage(mountainTile.x, mountainTile.y, 2);
  assert.deepEqual(Array.from(a.data), Array.from(b.data));
  assert.equal(viaMain.worldbuilder.stats.mainThreadRasters, 1, "?workers=0 is still the sync path");
  assert.equal(viaMain.worldbuilder.stats.poolRasters, 0);
});

test("totalMs stays MAIN-THREAD time; the worker's cost is reported separately", async () => {
  // The measurement this task exists to publish. If the worker's 190 ms were folded back
  // into `totalMs`, the before/after would show no improvement at all -- and if it were
  // dropped entirely, the report would claim the work vanished instead of moved.
  const provider = makeProvider({ pool: fakePool({ fillMs: 190 }), tileSize: 16 });
  await provider.requestImage(mountainTile.x, mountainTile.y, 2);
  const { stats, meanMs, meanWorkerMs } = provider.worldbuilder;
  assert.equal(stats.workerMs, 190, "the moved cost must still be counted, on its own line");
  assert.equal(meanWorkerMs(), 190);
  assert.ok(
    stats.totalMs < 190,
    `main-thread time per tile (${stats.totalMs.toFixed(3)} ms) must not include the ` +
    "worker's 190 ms; this is the assertion that would catch a before/after that measures " +
    "the same quantity twice",
  );
  assert.equal(meanMs(), stats.totalMs, "one tile, so the mean is the sample");
});

test("meanWorkerMs and meanWallMs are null on the synchronous path, not zero", async () => {
  // Zero would read as "the workers cost nothing", which is a claim. Null is the absence of
  // a measurement, which is the truth under ?workers=0.
  const provider = makeProvider({ tileSize: 16 });
  await provider.requestImage(mountainTile.x, mountainTile.y, 2);
  assert.equal(provider.worldbuilder.meanWorkerMs(), null);
  assert.equal(provider.worldbuilder.meanWallMs(), null);
  assert.ok(provider.worldbuilder.meanMs() > 0);
});

test("a rejected pool job REJECTS requestImage rather than throwing into the render loop", async () => {
  // `ImageryLayer._requestImagery` guards `requestImage` with `.then/.catch`, so a
  // rejection is Cesium's own retry-or-fall-back-to-the-parent path. An exception thrown
  // out of `requestImage` synchronously escapes that pair and takes the render loop down.
  const boom = new Error("worker 2: out of memory");
  const provider = makeProvider({ pool: fakePool({ reject: boom }), tileSize: 16 });
  let result;
  assert.doesNotThrow(() => { result = provider.requestImage(0, 0, 2); });
  await assert.rejects(result, /out of memory/);
});

// ---------------------------------------------------------------------------------------
// The water manifest, as far as this file is responsible for it.
//
// The manifest's CONTENT and the picture it produces are `water.test.mjs`'s subject; what is
// asserted here is the PLUMBING, with a synthetic body rather than a four-second resolution: that
// the bodies reach `relief.js` on both the synchronous and the pool path, that they cross the
// worker boundary inside the request, and that the two counters come back and accumulate.

/// One body covering the whole planet at a level no land on `DEFAULT_WORLD` reaches (its highest
/// point is 1,979 m). Synthetic on purpose: this makes every land texel a lake texel, so the
/// counter has a value that can be compared against the raster instead of a value that happens to
/// be small.
const FLOOD = [{
  rootNode: 1, kind: 0, levelM: 10000,
  minLatitudeDeg: -90, maxLatitudeDeg: 90, minLongitudeDeg: -180, maxLongitudeDeg: 180,
}];

test("the bodies reach the raster on the synchronous path, and the counters come back", async () => {
  const dry = makeProvider({ tileSize: 32 });
  await dry.requestImage(mountainTile.x, mountainTile.y, 2);
  assert.equal(dry.worldbuilder.stats.lakeTexels, 0, "no manifest, and yet water was drawn");
  assert.equal(dry.worldbuilder.stats.lakeTiles, 0);
  assert.deepEqual(dry.worldbuilder.lakes, [], "the default must be the pre-water picture");

  const wet = makeProvider({ tileSize: 32, lakes: FLOOD });
  const image = await wet.requestImage(mountainTile.x, mountainTile.y, 2);
  assert.equal(wet.worldbuilder.lakes, FLOOD, "a check must read the list the provider draws with");
  assert.equal(wet.worldbuilder.stats.lakeTiles, 1);
  assert.ok(
    wet.worldbuilder.stats.lakeTexels > 0,
    "a body covering the planet drew no water; the manifest never reached relief.js",
  );
  // ...and the picture moved with it, because a counter alone proves only that something counted.
  const dryImage = await dry.requestImage(mountainTile.x, mountainTile.y, 2);
  assert.notDeepEqual(Array.from(image.data), Array.from(dryImage.data));
});

test("the bodies cross the worker boundary inside the request, and the counts come back with the pixels", async () => {
  // A handle cannot cross (it indexes another instance's memory) and neither can an `Engine`; the
  // bodies are plain objects and DO cross, which is what lets the manifest be resolved once on the
  // main thread rather than eight times in the workers.
  const pool = fakePool({ fillMs: 190 });
  const provider = makeProvider({ pool, tileSize: 32, lakes: FLOOD });
  await provider.requestImage(mountainTile.x, mountainTile.y, 2);
  assert.equal(pool.requests.length, 1);
  assert.deepEqual(pool.requests[0].lakes, FLOOD, "the request carried no bodies");
  assert.equal(pool.requests[0].engine, undefined, "an Engine is not structured-cloneable");
  assert.equal(pool.requests[0].worldHandle, undefined, "a handle means nothing in another instance");
  assert.equal(provider.worldbuilder.stats.lakeTiles, 1);
  assert.ok(
    provider.worldbuilder.stats.lakeTexels > 0,
    "the worker's lake count did not reach the provider's stats",
  );
  assert.equal(provider.worldbuilder.stats.mainThreadRasters, 0, "this must be the pool path");
});
