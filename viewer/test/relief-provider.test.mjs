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
