//! The cloud imagery provider: `clouds.js`'s raster, wrapped so Cesium will drape it.
//
// Deliberately the same shape as `relief-provider.js`, because the traps that file records are
// already paid for and re-deriving them would be re-paying for them.
//
// # The two version traps, re-checked against the vendored 1.145.0 build for THIS provider
//
// - **`ready` and `readyPromise` were REMOVED from `ImageryProvider` in Cesium 1.107.** An
//   implementation copied from an older tutorial defines two properties nothing reads and looks
//   like diligence. `cloud-provider.test.mjs` asserts their *absence*.
// - **`requestImage` must return a PROMISE.** `ImageryLayer._requestImagery` guards the return
//   value with `defined()` and then calls `.then()` on it.
//
// The member list was produced by re-running the sweep the relief provider's was produced by --
// `grep -o "imageryProvider\.[A-Za-z_]*"` over the vendored `Cesium.js` -- rather than by
// copying that file's list. It came back with **one member the relief list does not have:
// `url`**, read by three wrapper classes that expose an inner provider's url as their own. This
// provider is never wrapped in one of those, so the read is unreachable here; `url` is defined
// as `undefined` anyway, on the same principle `proxy` is, and the fact is recorded in the task
// report rather than left as a difference between two lists nobody re-derived.
//
// # `getTileDataAvailable` is NOT an ImageryProvider member, and that matters
//
// The brief names `getTileDataAvailable` returning `undefined` as a trap. The sweep says it is a
// **TerrainProvider** member: in the vendored build it appears on `TerrainProvider.prototype`,
// `EllipsoidTerrainProvider`, `CesiumTerrainProvider`, `ArcGIS...`, `GoogleEarthEnterprise...`
// and `VRTheWorld...`, and `GlobeSurfaceTileProvider.canRefine` is what reads it. Nothing in the
// imagery path calls it. So this provider does not define it, and could not benefit from doing
// so: the trap belongs to `terrain.js`/`availability.js`, where `createAvailability` already
// returns definite booleans. Naming it here so a later reader does not "fix" its absence.
//
// # This rasterises in the WORKER POOL, and the main thread only blits
//
// The pool already carries two consumers (heightmap fills and relief rasters). This is the
// third, and it is on the same dispatcher for the same reason a second pool was rejected for
// relief: the contention that matters is engine instances per core, and a third pool of eight
// would triple the workers without tripling the cores.
//
// **The cloud job is the cheapest of the three by construction** -- 128 texels a side rather
// than 256, no engine fill at all, and a level cap of 5 rather than 12 -- which is why a third
// consumer can be added to a saturated pool at all. The combined cost with all three is measured
// in the task report against the two-consumer baseline, on a named host, rather than quoted as
// this layer's delta.
//
// `?workers=0` keeps the synchronous main-thread path, as the other two do.

import {
  calibrateClouds, cloudTile, CLOUD_MAX_LEVEL, CLOUD_TILE_SIZE, DEFAULT_CLOUD_COVER,
  makeCloudImageData,
} from "./clouds.js";
import { tileRectangleDegrees } from "./terrain.js";

/// **`?clouds=0` turns the layer off, and nothing else does.**
///
/// Same shape and same convention as `reliefLayerEnabled` and `biomeColourEnabled` in
/// `relief-provider.js`: one switch spelling in this viewer rather than a third one introduced
/// beside two.
///
/// `clouds=0` is also coverage zero, so the two readings agree -- but the layer is **not
/// constructed** rather than constructed and left transparent. That is what makes `?clouds=0`
/// byte-identical to the picture before this task rather than merely indistinguishable from it:
/// an added `ImageryLayer` that happens to be transparent still goes through Cesium's blend, and
/// "it should come out the same" is exactly the claim this project proves with a digest instead
/// of asserting.
export function cloudLayerEnabled(params) {
  return cloudCoverFromParams(params) > 0;
}

/// The requested coverage, in 0..1, from `?clouds=`.
///
/// Absent means `DEFAULT_CLOUD_COVER`. A value outside 0..1 or unparseable is clamped rather than
/// thrown: this is a look control, and a mistyped URL should not take the planet with it.
export function cloudCoverFromParams(params) {
  if (!params.has("clouds")) return DEFAULT_CLOUD_COVER;
  const raw = Number(params.get("clouds"));
  if (!Number.isFinite(raw)) return DEFAULT_CLOUD_COVER;
  return raw < 0 ? 0 : raw > 1 ? 1 : raw;
}

/// `ImageData` -> `HTMLCanvasElement`. Straight blit, no scaling and no compositing --
/// `putImageData` ignores `globalAlpha` and `globalCompositeOperation` by specification, which is
/// the property that matters here: the alpha bytes `clouds.js` wrote are the alpha bytes Cesium
/// uploads, unpremultiplied and untouched.
function imageDataToCanvas(imageData) {
  const canvas = document.createElement("canvas");
  canvas.width = imageData.width;
  canvas.height = imageData.height;
  canvas.getContext("2d").putImageData(imageData, 0, 0);
  return canvas;
}

/// Build the provider.
///
/// `radiusM` and `seed` are the same two the world was built from, and they are deliberately the
/// *same* values: weather at a different radius would be the wrong size, and weather from a
/// different seed would be a second planet's -- the `wrong-world` fault shape, arrived at by
/// accident, on a layer where it would be completely invisible.
export function createCloudImageryProvider({
  radiusM,
  seed = 0,
  cover = DEFAULT_CLOUD_COVER,
  tileSize = CLOUD_TILE_SIZE,
  minimumLevel = 0,
  maximumLevel = CLOUD_MAX_LEVEL,
  tilingScheme = new Cesium.GeographicTilingScheme(),
  credit = "worldbuilder cloud layer",
  /// A `calibrateClouds` result, or `undefined` to calibrate once here. Computed on the main
  /// thread and shipped in every tile request rather than recomputed per worker, exactly as the
  /// biome calibration is: 20,000 field evaluations in each of eight workers is eight times the
  /// cost for one threshold they must all agree on, and a worker that calibrated its own would be
  /// a second place they could disagree.
  clouds,
  toImage = imageDataToCanvas,
  onTile = null,
  pool = null,
}) {
  if (!Number.isFinite(radiusM) || radiusM <= 0) {
    throw new Error(`createCloudImageryProvider: radiusM must be positive, got ${radiusM}`);
  }

  const calibration = clouds === undefined ? calibrateClouds({ radiusM, cover, seed }) : clouds;

  /// Same counters, same meanings, as the relief provider's. **`totalMs` is MAIN-THREAD time
  /// only, in both modes**; `mainThreadRasters` must be 0 whenever a pool is present, which is
  /// the counter that would catch a provider that silently fell back to rasterising here and
  /// rendered identically while doing so.
  const stats = {
    tiles: 0, totalMs: 0, maxMs: 0, maxLevelRequested: -1,
    mainThreadRasters: 0, poolRasters: 0, workerMs: 0, wallMs: 0,
  };

  function record(elapsed) {
    stats.tiles += 1;
    stats.totalMs += elapsed;
    if (elapsed > stats.maxMs) stats.maxMs = elapsed;
  }

  const provider = {
    // --- the members Cesium 1.145.0 reads, in the order the sweep found them ---
    credit: new Cesium.Credit(credit),
    errorEvent: new Cesium.Event(),
    getTileCredits() {
      return [];
    },
    /// **`true`, unlike the relief layer's `false`, and this is the one member whose value had to
    /// change.** `ImageryLayer._createTextureWebGL` reads it to choose RGB over RGBA; a cloud
    /// raster whose whole content is in the alpha channel uploaded as RGB would be an opaque
    /// white sheet over the planet.
    hasAlphaChannel: true,
    maximumLevel,
    minimumLevel,
    pickFeatures() {
      return undefined;
    },
    proxy: undefined,
    /// Read only by wrapper providers that expose an inner provider's url as their own; nothing
    /// wraps this one. Defined for the same reason `proxy` is -- so the object matches the sweep
    /// rather than matching the relief provider's older copy of it.
    url: undefined,
    rectangle: tilingScheme.rectangle,
    tileDiscardPolicy: undefined,
    tileHeight: tileSize,
    tileWidth: tileSize,
    tilingScheme,

    /// Returns a promise, per the trap above.
    requestImage(x, y, level) {
      if (level > stats.maxLevelRequested) stats.maxLevelRequested = level;
      const rectangle = tileRectangleDegrees(tilingScheme, x, y, level);

      if (!pool) {
        const started = performance.now();
        let imageData;
        try {
          imageData = cloudTile({ rectangle, level, size: tileSize, clouds: calibration });
        } catch (error) {
          // Cesium's own failure path. Throwing synchronously out of `requestImage` would escape
          // the `.then/.catch` pair in `_requestImagery` and take the render loop with it.
          return Promise.reject(error);
        }
        const elapsed = performance.now() - started;
        record(elapsed);
        stats.mainThreadRasters += 1;
        const image = toImage(imageData);
        if (onTile) onTile({ x, y, level, rectangle, imageData, image, ms: elapsed, source: "main" });
        return Promise.resolve(image);
      }

      // The pool path. **The request carries no engine and no world handle** -- unlike the relief
      // request it never had one to carry, because the cloud field is a point function of
      // position and does not read the ground at all.
      const request = { rectangle, level, size: tileSize, clouds: calibration };
      const wallStarted = performance.now();
      return pool.cloud(request).then((result) => {
        const started = performance.now();
        const imageData = makeCloudImageData(result.data, result.width, result.height);
        const image = toImage(imageData);
        const elapsed = performance.now() - started;
        record(elapsed);
        stats.poolRasters += 1;
        stats.workerMs += result.fillMs;
        stats.wallMs += performance.now() - wallStarted;
        if (onTile) {
          onTile({
            x, y, level, rectangle, imageData, image, ms: elapsed,
            source: `worker:${result.worker}`, result,
          });
        }
        return image;
      });
    },

    /// Everything a check should read rather than recompute, mirroring the shape the relief
    /// provider publishes.
    worldbuilder: {
      radiusM,
      seed: String(seed),
      tileSize,
      /// The calibration this provider is drawing with -- read rather than recalibrated, so a
      /// check cannot arrive at a *different* threshold and compare against that.
      clouds: calibration,
      pool,
      stats,
      rectangleDegrees: (x, y, level) => tileRectangleDegrees(tilingScheme, x, y, level),
      meanMs: () => (stats.tiles > 0 ? stats.totalMs / stats.tiles : null),
      meanWorkerMs: () => (stats.poolRasters > 0 ? stats.workerMs / stats.poolRasters : null),
      meanWallMs: () => (stats.poolRasters > 0 ? stats.wallMs / stats.poolRasters : null),
    },
  };

  return provider;
}
