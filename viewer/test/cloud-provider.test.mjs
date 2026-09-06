// Node-native tests for cloud-provider.js -- the Cesium `ImageryProvider` that drapes the
// cloud raster over the globe.
//
// **Why this can be a node test at all.** Same reason `relief-provider.test.mjs` can: the
// vendored Cesium build is the IIFE one and every module reads the global `Cesium`, so rather
// than hand-rolling a fake tiling scheme -- which would test the fake -- the REAL classes from
// the `@cesium/engine` package `viewer/package.json` pins at 1.145.0 are installed below.
//
// Population/method/host, named once:
//   - World: the owner's radius, 4,500,000 m, seed 562423712. **No wasm and no world handle** --
//     the cloud field reads neither, which is asserted here as well as in `clouds.test.mjs`.
//   - Host: node v22.x.
//   - Tiles: level 2 and 3 of a `GeographicTilingScheme`, whose rectangles come from Cesium
//     itself rather than from arithmetic in this file.

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

// Installed BEFORE the module under test is imported: the provider constructs a default tiling
// scheme at call time and `terrain.js`'s `tileRectangleDegrees` reads `Cesium.Math` at call time.
globalThis.Cesium = {
  GeographicTilingScheme, Rectangle, Event: CesiumEvent, Credit, Math: CesiumMath,
};

const {
  CLOUD_MAX_LEVEL, CLOUD_TILE_SIZE, DEFAULT_CLOUD_COVER, alphaStats, calibrateClouds, cloudTile,
  hasCloudStructure,
} = await import("../public/app/clouds.js");
const {
  cloudCoverFromParams, cloudLayerEnabled, createCloudImageryProvider,
} = await import("../public/app/cloud-provider.js");
const { MAX_LEVEL } = await import("../public/app/terrain.js");

const RADIUS_M = 4_500_000;
const SEED = "562423712";

function makeProvider(overrides = {}) {
  return createCloudImageryProvider({
    radiusM: RADIUS_M,
    seed: SEED,
    // ImageData does not exist under `node --test`; `clouds.js` already returns a plain
    // {data,width,height} there and the canvas step is the browser's job. The identity
    // conversion is what lets these tests see the raster itself.
    toImage: (imageData) => imageData,
    // Keep the suite cheap where the raster's content is not what is under test.
    tileSize: 32,
    ...overrides,
  });
}

// =========================================================================================
// The interface sweep -- the two recorded version traps, re-checked for THIS provider
// =========================================================================================

// Produced by re-running the sweep the relief provider's list came from
// (`grep -o "imageryProvider\.[A-Za-z_]*"` over the vendored `public/vendor/cesium/Cesium.js`),
// not by copying that list. It came back with `url`, which the relief list does not carry: three
// wrapper classes expose an inner provider's `url` as their own. Nothing wraps this provider, so
// the read is unreachable here -- but a member list that silently differs from the sweep is a
// list nobody re-derived, which is the failure mode this whole assertion exists to refuse.
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
  "url",
];

test("the provider defines every member Cesium 1.145 actually reads", () => {
  const provider = makeProvider();
  const missing = MEMBERS_CESIUM_1_145_READS.filter((name) => !(name in provider));
  assert.deepEqual(
    missing, [],
    `an ImageryProvider member Cesium reads is absent, which fails SILENTLY (undefined), not ` +
    `loudly: ${missing.join(", ")}`,
  );
});

test("the provider does NOT define ready/readyPromise -- both removed in Cesium 1.107", () => {
  const provider = makeProvider();
  assert.equal(
    "ready" in provider, false,
    "`ready` was removed from ImageryProvider in 1.107; defining it is dead weight that makes " +
    "an implementation look like it is doing version-correct work when it is not",
  );
  assert.equal("readyPromise" in provider, false, "`readyPromise` was removed in 1.107");
});

test("getTileDataAvailable is NOT defined, because it is not an ImageryProvider member", () => {
  // The brief names `getTileDataAvailable` returning `undefined` as a trap. The sweep says it
  // lives on TerrainProvider: in the vendored build it is on `TerrainProvider.prototype`,
  // `EllipsoidTerrainProvider`, `CesiumTerrainProvider` and three others, and
  // `GlobeSurfaceTileProvider.canRefine` is what reads it. Nothing in the imagery path calls it.
  //
  // Asserted as an ABSENCE for the same reason `ready` is: adding it here would look like
  // diligence, would never be called, and would be one more branch nothing reaches.
  const provider = makeProvider();
  assert.equal("getTileDataAvailable" in provider, false);
});

test("hasAlphaChannel is TRUE, which is the one member whose value had to change", () => {
  // `ImageryLayer._createTextureWebGL` reads this to choose RGB over RGBA. The relief layer sets
  // it false because it writes 255 at every texel; a cloud raster whose entire content is in the
  // alpha channel, uploaded as RGB, is an opaque white sheet over the planet. Pinned because a
  // provider copied from the relief one inherits `false` and looks finished.
  assert.equal(makeProvider().hasAlphaChannel, true);
});

test("credit is a Cesium.Credit and errorEvent is a Cesium.Event", () => {
  const provider = makeProvider();
  assert.ok(provider.credit instanceof Credit);
  assert.ok(
    provider.errorEvent instanceof CesiumEvent,
    "ImageryLayer calls raiseEvent/numberOfListeners on it",
  );
});

test("requestImage returns a PROMISE, not the raster", async () => {
  // `ImageryLayer._requestImagery` guards the return value with `defined()` and then calls
  // `.then()` on it. Returning the canvas directly throws from inside Cesium once per tile.
  const provider = makeProvider();
  const returned = provider.requestImage(3, 1, 2);
  assert.ok(typeof returned.then === "function", "requestImage must return a thenable");
  const image = await returned;
  assert.equal(image.width, 32);
  assert.equal(image.height, 32);
});

test("the tile rectangle is Cesium's own, not this file's arithmetic", () => {
  const scheme = new GeographicTilingScheme();
  const provider = makeProvider({ tilingScheme: scheme });
  const r = scheme.tileXYToRectangle(5, 2, 3);
  // `tileRectangleDegrees` also carries the source `Rectangle` on a `radians` field, so the four
  // degree values are compared by name rather than the whole object -- comparing the object
  // would be asserting about that convenience field rather than about the geometry.
  const got = provider.worldbuilder.rectangleDegrees(5, 2, 3);
  assert.equal(got.northDeg, CesiumMath.toDegrees(r.north));
  assert.equal(got.southDeg, CesiumMath.toDegrees(r.south));
  assert.equal(got.westDeg, CesiumMath.toDegrees(r.west));
  assert.equal(got.eastDeg, CesiumMath.toDegrees(r.east));
  assert.equal(provider.rectangle, scheme.rectangle);
});

// =========================================================================================
// The level cap, which is deliberately NOT the terrain's
// =========================================================================================

test("the cloud layer's default cap is its own, not the terrain's", () => {
  // The relief layer follows `maxLevel` because colour must not stop refining before geometry
  // does. Clouds must not: the field's finest structure is oversampled twelve times over at
  // level 5, so following the terrain to 12 would ask the pool for seven levels of tiles
  // carrying no new content. Pinned because the natural thing to do when copying the relief
  // provider is to copy its cap too.
  assert.equal(makeProvider().maximumLevel, CLOUD_MAX_LEVEL);
  assert.ok(CLOUD_MAX_LEVEL < MAX_LEVEL, `the cloud cap ${CLOUD_MAX_LEVEL} is not below the terrain's ${MAX_LEVEL}`);
  assert.equal(CLOUD_TILE_SIZE, 128);
});

// =========================================================================================
// The switch and the coverage parameter
// =========================================================================================

test("?clouds= parses, clamps, and defaults in one place", () => {
  const at = (search) => cloudCoverFromParams(new URLSearchParams(search));
  assert.equal(at(""), DEFAULT_CLOUD_COVER, "an absent parameter must take the module default");
  assert.equal(at("clouds=0"), 0);
  assert.equal(at("clouds=0.65"), 0.65);
  assert.equal(at("clouds=1"), 1);
  // A look control should not take the planet down with a mistyped URL.
  assert.equal(at("clouds=5"), 1);
  assert.equal(at("clouds=-3"), 0);
  assert.equal(at("clouds=nonsense"), DEFAULT_CLOUD_COVER);
});

test("?clouds=0 turns the layer off and nothing else does", () => {
  const on = (search) => cloudLayerEnabled(new URLSearchParams(search));
  assert.equal(on(""), true);
  assert.equal(on("clouds=0"), false);
  assert.equal(on("clouds=0.01"), true);
  // The neighbouring switches must not move it, which is what keeps the before/after of the
  // land-colour work and the before/after of this one independent.
  assert.equal(on("relief=0"), true);
  assert.equal(on("biome=0"), true);
  assert.equal(on("flat=1"), true);
});

// =========================================================================================
// The pool -- the third consumer
// =========================================================================================

/// A pool that answers `cloud(request)` the way `tile-worker.js` does: by spreading the request
/// over `cloudTile`. Records every request so the wire format can be asserted. It is NOT a
/// stand-in for the worker's own message handler -- that is exercised in `tile-worker.test.mjs`,
/// which drives `tile-worker.js`'s actual `onmessage`.
function fakePool({ fillMs = 17, reject = null } = {}) {
  return {
    requests: [],
    cloud(request) {
      this.requests.push(request);
      if (reject) return Promise.reject(reject);
      const imageData = cloudTile(request);
      return Promise.resolve({
        data: imageData.data, width: imageData.width, height: imageData.height,
        fillMs, worker: 2,
      });
    },
  };
}

test("with a pool, NOTHING rasterises on the main thread", async () => {
  // The counter, not the picture: a provider that ignored the pool would render exactly the same
  // globe and quote exactly the same `meanMs`, because it would be measuring the path it should
  // no longer be on. Byte-identity proves the picture, never the path.
  const pool = fakePool();
  const provider = makeProvider({ pool });
  await provider.requestImage(3, 1, 2);
  const { stats } = provider.worldbuilder;
  assert.equal(stats.mainThreadRasters, 0, "a cloud tile was rasterised on the main thread despite a pool");
  assert.equal(stats.poolRasters, 1);
  assert.equal(pool.requests.length, 1, "the pool must actually have been asked");
});

test("the request sent to the pool is structured-cloneable and carries no engine at all", async () => {
  const pool = fakePool();
  const provider = makeProvider({ pool, tileSize: 16 });
  await provider.requestImage(3, 1, 2);
  const [request] = pool.requests;
  assert.equal("engine" in request, false);
  assert.equal("worldHandle" in request, false, "the cloud field never had a world handle to send");
  assert.doesNotThrow(() => structuredClone(request), "the request must survive postMessage");
  assert.equal(request.size, 16, "the tile size must cross, or the worker guesses at its default");
  assert.equal(request.level, 2);
  assert.deepEqual(request.rectangle, provider.worldbuilder.rectangleDegrees(3, 1, 2));
  assert.ok(Number.isFinite(request.clouds.threshold), "the calibration must cross, not be recomputed");
});

test("the calibration is computed ONCE and shipped with every tile", async () => {
  // 20,000 field evaluations in each of eight workers is eight times the cost for one number
  // they must all agree on, and a worker that calibrated its own would be a second place they
  // could disagree. The identity check is what proves it is the same object rather than an equal
  // one recomputed per request.
  const pool = fakePool();
  const provider = makeProvider({ pool, tileSize: 8 });
  await provider.requestImage(0, 0, 1);
  await provider.requestImage(1, 0, 1);
  const [a, b] = pool.requests;
  assert.equal(a.clouds, b.clouds);
  assert.equal(a.clouds, provider.worldbuilder.clouds);
});

test("the pool path and the ?workers=0 path produce byte-identical rasters", async () => {
  const viaMain = makeProvider({ tileSize: 24 });
  const viaPool = makeProvider({ pool: fakePool(), tileSize: 24 });
  const [a, b] = await Promise.all([
    viaMain.requestImage(6, 2, 3), viaPool.requestImage(6, 2, 3),
  ]);
  assert.deepEqual(Array.from(a.data), Array.from(b.data));
  assert.equal(viaMain.worldbuilder.stats.poolRasters, 0);
  assert.equal(viaMain.worldbuilder.stats.mainThreadRasters, 1);
});

test("the two costs are reported as two numbers rather than added together", async () => {
  const provider = makeProvider({ pool: fakePool({ fillMs: 17 }), tileSize: 16 });
  await provider.requestImage(3, 1, 2);
  const wb = provider.worldbuilder;
  assert.equal(wb.meanWorkerMs(), 17, "the worker's own time must be reported, not folded in");
  assert.ok(wb.meanMs() < 17, "main-thread time must be the blit alone, not the rasterisation");
  assert.ok(wb.meanWallMs() >= 0);
});

test("a rejected pool job REJECTS requestImage rather than throwing into the render loop", async () => {
  const boom = new Error("worker 2: exploded");
  const provider = makeProvider({ pool: fakePool({ reject: boom }), tileSize: 16 });
  await assert.rejects(() => provider.requestImage(3, 1, 2), /exploded/);
});

test("a rasterisation failure on the ?workers=0 path rejects rather than throwing", async () => {
  // Throwing synchronously out of `requestImage` escapes the `.then/.catch` pair in
  // `_requestImagery` and takes the render loop with it.
  const provider = makeProvider({ clouds: { threshold: NaN } });
  await assert.rejects(() => provider.requestImage(3, 1, 2));
});

// =========================================================================================
// What the provider actually draws
// =========================================================================================

test("the raster a real tile request produces is cloud, not a transparent sheet", async () => {
  const provider = makeProvider({ tileSize: CLOUD_TILE_SIZE });
  const rectangle = provider.worldbuilder.rectangleDegrees(7, 3, 3);
  const image = await provider.requestImage(7, 3, 3);
  const verdict = hasCloudStructure(image, { rectangle });
  assert.ok(verdict.ok, verdict.reasons.join("; "));
});

test("the provider refuses a nonsensical radius rather than drawing something", () => {
  assert.throws(() => createCloudImageryProvider({ radiusM: 0 }), /radiusM/);
  assert.throws(() => createCloudImageryProvider({ radiusM: NaN }), /radiusM/);
});

test("the calibration the provider publishes is the one it draws with", async () => {
  // Read rather than recalibrated, so a check cannot arrive at a different threshold and compare
  // against that -- which would agree with itself and prove nothing.
  const provider = makeProvider({ cover: 0.25, tileSize: 64 });
  assert.equal(provider.worldbuilder.clouds.cover, 0.25);
  const rectangle = provider.worldbuilder.rectangleDegrees(7, 3, 3);
  const image = await provider.requestImage(7, 3, 3);
  const direct = cloudTile({ rectangle, size: 64, clouds: provider.worldbuilder.clouds });
  assert.deepEqual(Array.from(image.data), Array.from(direct.data));
});

test("a lower coverage really does draw less cloud", async () => {
  // Dead code looks like a feature, and a coverage parameter that reached the calibration but
  // not the raster would look exactly like a working slider.
  const stats = async (cover) => {
    const provider = makeProvider({ cover, tileSize: 64 });
    const rectangle = provider.worldbuilder.rectangleDegrees(7, 3, 3);
    return alphaStats(await provider.requestImage(7, 3, 3), rectangle);
  };
  const light = await stats(0.1);
  const heavy = await stats(0.8);
  assert.ok(
    heavy.coverage > light.coverage + 0.2,
    `0.8 gave ${heavy.coverage.toFixed(4)} against 0.1's ${light.coverage.toFixed(4)}`,
  );
});

test("two providers on different radii are different weather", () => {
  const a = makeProvider({ radiusM: 4_500_000 });
  const b = makeProvider({ radiusM: 9_000_000 });
  assert.notEqual(a.worldbuilder.clouds.threshold, b.worldbuilder.clouds.threshold);
});

test("the shipped default cover reaches the provider", () => {
  assert.equal(
    createCloudImageryProvider({ radiusM: RADIUS_M, seed: SEED }).worldbuilder.clouds.cover,
    DEFAULT_CLOUD_COVER,
  );
  assert.equal(calibrateClouds({ radiusM: RADIUS_M, seed: SEED }).cover, DEFAULT_CLOUD_COVER);
});

// =========================================================================================
// The wiring decisions in main.js that nothing else can guard
// =========================================================================================

// `main.js` needs a `Viewer`, a WebGL context and the wasm, so it cannot be exercised under
// `node --test`. Two of its decisions about this layer are load-bearing and fail SILENTLY, so
// they are asserted against its source. A source assertion is weaker than a behavioural one and
// is used here only where there is no behavioural one to be had -- which is the same reason
// `panel-fields.test.mjs` reads `main.js` rather than importing it.

const mainSource = readFileSync(
  fileURLToPath(new URL("../public/app/main.js", import.meta.url)), "utf8",
);

test("the cloud layer is added AFTER the relief layer, or it does nothing", () => {
  // `ImageryLayerCollection` composites in index order, so the last layer added is drawn over the
  // ones before it. Clouds are translucent and the ground shows through them; added first, Cesium
  // would blend the opaque relief layer over them and the whole layer would silently vanish --
  // the same failure shape as the `ElevationRamp` material hiding the relief imagery, which was a
  // real bug in this file.
  const relief = mainSource.indexOf("addImageryProvider(installed.reliefProvider)");
  const cloud = mainSource.indexOf("addImageryProvider(installed.cloudProvider)");
  assert.ok(relief > 0 && cloud > 0, "both layers must be added through addImageryProvider");
  assert.ok(cloud > relief, "the cloud layer must be added after the relief layer, or it is hidden");
  // **Add-order is no longer sufficient, because the relief layer is now re-added on every live
  // swap and `addImageryProvider` APPENDS.** On the second install the freshly-added relief layer
  // would land on top of the cloud deck and hide it completely -- the same silent vanishing this
  // test was written for, reached by a path that did not exist when it was written. `lowerToBottom`
  // is what restores the boot-path order after each swap, and it is asserted rather than assumed.
  assert.match(
    mainSource, /lowerToBottom\(installed\.reliefLayer\)/,
    "a swapped-in relief layer must be lowered under the clouds, or it hides them",
  );
});

test("the cloud layer is not built when the ramp material would cover it", () => {
  // The `ElevationRamp` material's alpha is 1 everywhere and `GlobeFS` composites it OVER all
  // imagery, so with the ramp painting a cloud layer would be constructed, rasterised in the
  // pool, uploaded -- and invisible. Dead code looks like a feature.
  assert.match(
    mainSource, /if \(cloudLayerEnabled\(params\) && !paint(?: &&[^)]*)?\)/,
    "the cloud layer's construction must be gated on the ramp material not painting",
  );
  assert.ok(
    mainSource.indexOf("const paint =") < mainSource.indexOf("cloudLayerEnabled(params) && !paint"),
    "`paint` must be decided before the cloud block reads it",
  );
});
