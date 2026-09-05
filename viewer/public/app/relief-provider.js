//! The relief imagery provider: Task 1's raster, wrapped so Cesium will drape it.
//
// # Why an *imagery* layer and not terrain lighting
//
// `CustomHeightmapTerrainProvider` produces `HeightmapTerrainData`, whose
// `hasVertexNormals` is `false` -- always, on that class, with no option to change it.
// `GlobeFS` then falls back to `czm_geodeticSurfaceNormal`, the *ellipsoid* normal, so the
// mesh is lit as a perfect smooth sphere no matter what `enableLighting` says. That was
// verified live, after lighting was (wrongly) recommended as the fix and did nothing. A
// raster is the only surface in this stack that can carry a normal, so the shading is baked
// into one, and this file is what hands Cesium the raster.
//
// # Why the tile is 256 and the mesh is 65
//
// Cesium picks the imagery level from the *terrain* tile's geometric error and does not
// clamp it to the terrain level -- measured, not assumed. So a 256-texel imagery tile over
// the same rectangle as a 65-post terrain tile is a free 4x increase in colour resolution,
// paid for in rasterisation and not in triangles. That gap -- shading at a finer frequency
// than the tessellation -- is the actual answer to "when I zoom in it just looks blurry",
// because screen-space *post* density is invariant to heightmap width and cannot be the
// answer.
//
// # The two version traps, both checked against the vendored 1.145.0 source
//
// - **`ready` and `readyPromise` were REMOVED from `ImageryProvider` in Cesium 1.107.**
//   `ImageryLayer.ready` is now simply `defined(this._imageryProvider)`, so a provider is
//   usable the instant it is constructed. Nothing here is async and nothing here defines
//   those two properties; `relief-provider.test.mjs` asserts their *absence*, because
//   defining them is the failure mode that looks like diligence.
// - **`requestImage` must return a PROMISE.** `ImageryLayer._requestImagery` guards the
//   return value with `defined()` and then calls `.then()` on it. Returning the canvas
//   directly throws from inside Cesium once per tile; returning the raw `ImageData` gets
//   past that line and dies later at texture upload. Both are pinned by a test.
//
// The member list this object implements was produced by SWEEPING the vendored tree for
// `imageryProvider.<member>` reads rather than by copying a tutorial -- the same "sweep, do
// not spot-check" rule that has found three real aborts on the engine side and zero by
// spot-checking. The full set Cesium 1.145.0 reads is: `credit`, `errorEvent`,
// `getTileCredits`, `hasAlphaChannel`, `maximumLevel`, `minimumLevel`, `pickFeatures`,
// `proxy`, `rectangle`, `requestImage`, `tileDiscardPolicy`, `tileHeight`, `tileWidth`,
// `tilingScheme`. Cesium also *writes* `_reload`, which a plain object accepts.
//
// # This rasterises in the WORKER POOL (Task 4), and the main thread only blits
//
// One 256-texel tile is a (256+2)^2 = 66,564-sample engine fill plus 65,536 texels of
// shading. Task 3 measured that at **191.6 ms mean per tile** on the main thread, and an
// orbital view asks for 26 to 79 of them: five to seven seconds during which the camera
// does not move. That is not a slow viewer, it is a viewer that stops responding, which is
// why moving it was a prerequisite rather than an optimisation.
//
// The move is a substitution, not a redesign: `pool.js` already carried a dispatcher, and
// `requestImage` already returned a promise because Cesium requires one. What crosses back
// is the raw `Uint8ClampedArray` (transferred, not copied); the main thread wraps it in an
// `ImageData` and blits it to a canvas, and **that** -- not the rasterisation -- is what
// `stats.totalMs` now counts. The two are reported separately for exactly this reason: a
// figure that added the worker's time back in would describe work the camera never waits
// for, and a figure that quoted zero would hide the blit.
//
// **`?workers=0` keeps the synchronous main-thread path**, unchanged, which is both the
// fallback for a host without module workers and the A/B baseline every figure in the Task
// 4 report is quoted against -- same page, same world, same tiles, one flag apart.

import { reliefTile, DEFAULT_SUN, makeImageData } from "./relief.js";
import { MAX_LEVEL, tileRectangleDegrees } from "./terrain.js";

/// 256 texels per tile edge. Four times the terrain's 65 posts over the same rectangle --
/// see the module doc for why that multiplier is free rather than paid for in geometry.
export const RELIEF_TILE_SIZE = 256;

/// **`?relief=0` turns the layer off, and nothing else does.**
///
/// Deliberately the same shape as `main.js`'s existing `params.get("flat") !== "1"` and
/// `params.get("paint") !== "0"` switches: one convention in the file rather than a second
/// one introduced alongside it. Exported (rather than inlined at the call site) only so the
/// default can be asserted instead of eyeballed -- the panel's ramp defaults silently
/// drifted from `main.js`'s once already in this slice, and a default that is only ever
/// read by the code that sets it is exactly the shape that drifts.
export function reliefLayerEnabled(params) {
  return params.get("relief") !== "0";
}

/// `ImageData` -> `HTMLCanvasElement`, which is one of the four types Cesium's
/// `ImageryTypes` accepts. `putImageData` is a straight blit: no scaling, no colour-space
/// conversion, no compositing (it ignores globalAlpha and globalCompositeOperation by
/// specification), so the texels Cesium uploads are the bytes `relief.js` wrote.
///
/// `OffscreenCanvas`/`createImageBitmap` in the worker would remove even this blit, at the
/// price of a path `node --test` cannot see at all (neither global exists there, and
/// `relief.js` already returns a plain object in that host). The blit was measured instead
/// of assumed -- see the report -- and it is three orders of magnitude below the
/// rasterisation it replaced, so the untestable version buys nothing worth its blindness.
function imageDataToCanvas(imageData) {
  const canvas = document.createElement("canvas");
  canvas.width = imageData.width;
  canvas.height = imageData.height;
  canvas.getContext("2d").putImageData(imageData, 0, 0);
  return canvas;
}

/// Build the provider.
///
/// `engine`/`worldHandle`/`radiusM` are the same three things `terrain.js` takes, and are
/// deliberately the *same* handles: a relief layer drawn from a different world than the
/// mesh is the `wrong-world` fault by accident, and it would look completely plausible.
///
/// `toImage` exists so `node --test` can see the raster itself. There is no `ImageData` and
/// no `document` outside a browser; `relief.js` already returns an ImageData-shaped plain
/// object in that case, and an identity `toImage` carries it through. The browser never
/// passes this argument.
export function createReliefImageryProvider({
  engine,
  worldHandle,
  radiusM,
  tileSize = RELIEF_TILE_SIZE,
  minimumLevel = 0,
  // The terrain's own ground cap. Imagery must not stop refining before the mesh does, or
  // the colour goes soft exactly where the geometry gets sharp -- which is the reported
  // complaint. Past this level Cesium magnifies the parent texture, which is the correct
  // thing to do: at level 12 a 256-texel tile samples every 19.1 m, well below the field's
  // measured 78.125 m resolution floor, so there is no further generated detail to reveal.
  maximumLevel = MAX_LEVEL,
  tilingScheme = new Cesium.GeographicTilingScheme(),
  credit = "worldbuilder engine relief",
  sun = DEFAULT_SUN,
  toImage = imageDataToCanvas,
  onTile = null,
  /// The worker pool from `pool.js`, or `null` for the synchronous main-thread path.
  /// `engine`/`worldHandle` are still required either way: they are what `?workers=0`
  /// rasterises with, and what a check compares a worker's raster against.
  pool = null,
}) {
  if (!engine || typeof engine.fillTileF32 !== "function") {
    throw new Error("createReliefImageryProvider: engine.fillTileF32 is required");
  }
  if (!Number.isFinite(radiusM) || radiusM <= 0) {
    throw new Error(`createReliefImageryProvider: radiusM must be positive, got ${radiusM}`);
  }

  /// Counters a check or a report reads, taken here rather than in a separate harness so
  /// the population is the tiles the camera actually asked for.
  ///
  /// **`totalMs` is MAIN-THREAD time only, in both modes**, which is what makes the
  /// before/after a comparison of one quantity rather than two. On the synchronous path
  /// that is the whole rasterisation; on the pool path it is the `ImageData` wrap plus the
  /// `putImageData` blit, and the rasterisation shows up in `workerMs` instead.
  ///
  /// `mainThreadRasters` is the counter that matters for the claim: a pool that silently
  /// fell back to rasterising here would render identically and quote a fine `totalMs`
  /// only because it was measuring the wrong path. It must be 0 whenever a pool is present.
  const stats = {
    tiles: 0,
    totalMs: 0,
    maxMs: 0,
    maxLevelRequested: -1,
    mainThreadRasters: 0,
    poolRasters: 0,
    workerMs: 0,
    wallMs: 0,
  };

  /// One place that accumulates the main-thread cost, so both paths cannot disagree about
  /// what `totalMs` and `maxMs` mean.
  function record(elapsed) {
    stats.tiles += 1;
    stats.totalMs += elapsed;
    if (elapsed > stats.maxMs) stats.maxMs = elapsed;
  }

  const provider = {
    // --- the members Cesium 1.145.0 reads, in the order the sweep found them ---
    credit: new Cesium.Credit(credit),
    /// A real `Event`: `ImageryLayer` reads `numberOfListeners` and calls `raiseEvent`.
    errorEvent: new Cesium.Event(),
    getTileCredits() {
      // Per-tile credits on top of the provider-wide `credit` above would draw the same
      // string once per visible tile.
      return [];
    },
    /// `relief.js` writes alpha 255 at every texel, so declaring an alpha channel would
    /// upload a byte per texel that is always 255. `ImageryLayer._createTextureWebGL` reads
    /// this to choose RGB over RGBA.
    hasAlphaChannel: false,
    maximumLevel,
    minimumLevel,
    /// Nothing to pick on a relief raster. `undefined` is Cesium's "feature picking is not
    /// supported by this provider", and it is what `ImageryLayerCollection.pickImageryLayerFeatures`
    /// checks for.
    pickFeatures() {
      return undefined;
    },
    /// Named explicitly rather than left off the object: `proxy` is read, and an absent
    /// property and an `undefined` one behave the same to Cesium but not to a reader
    /// checking this list against the sweep.
    proxy: undefined,
    rectangle: tilingScheme.rectangle,
    /// No discard policy: every tile this provider produces is real. A `null` here would be
    /// wrong -- `defined(null)` is false in Cesium, so it would work, but `undefined` is
    /// what every built-in provider uses for "none".
    tileDiscardPolicy: undefined,
    tileHeight: tileSize,
    tileWidth: tileSize,
    tilingScheme,

    /// **Returns a promise**, per the trap in the module doc -- and now it is a promise
    /// that is genuinely pending, which is the whole of Task 4: Cesium already accepted an
    /// unresolved image, so nothing about this signature had to change to stop blocking.
    requestImage(x, y, level) {
      if (level > stats.maxLevelRequested) stats.maxLevelRequested = level;
      const rectangle = tileRectangleDegrees(tilingScheme, x, y, level);

      if (!pool) {
        // `?workers=0`. Synchronous, on the main thread, and the baseline every worker
        // figure in the report is measured against.
        const started = performance.now();
        let imageData;
        try {
          imageData = reliefTile({
            rectangle, level, size: tileSize, engine, worldHandle, radiusM, sun,
          });
        } catch (error) {
          // Cesium's own failure path: reject, and it retries or falls back to the parent
          // texture. Throwing synchronously out of `requestImage` instead would escape the
          // `.then/.catch` pair in `_requestImagery` and take the render loop with it.
          return Promise.reject(error);
        }
        const elapsed = performance.now() - started;
        record(elapsed);
        stats.mainThreadRasters += 1;
        const image = toImage(imageData);
        if (onTile) {
          onTile({ x, y, level, rectangle, imageData, image, ms: elapsed, source: "main" });
        }
        return Promise.resolve(image);
      }

      // The pool path. **The request carries no engine and no world handle**: a handle is
      // an index into a table inside one wasm instance's linear memory and is meaningless
      // in another, and an `Engine` object is not structured-cloneable at all -- posting
      // one throws `DataCloneError` per tile. The worker supplies both from its own world.
      const request = { rectangle, level, size: tileSize, radiusM, sun };
      const wallStarted = performance.now();
      return pool.relief(request).then((result) => {
        // The only main-thread work left. `makeImageData` is a view over the transferred
        // buffer (no copy); `putImageData` is the blit. Measured, not assumed -- see the
        // report for the figure and its host.
        const started = performance.now();
        const imageData = makeImageData(result.data, result.width);
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

    /// Everything a check should read rather than recompute, mirroring the shape
    /// `terrain.js` already publishes on its provider.
    worldbuilder: {
      engine,
      worldHandle,
      radiusM,
      tileSize,
      sun,
      pool,
      stats,
      rectangleDegrees: (x, y, level) => tileRectangleDegrees(tilingScheme, x, y, level),
      /// Mean **main-thread** milliseconds per tile so far, or null before any tile. Named
      /// `meanMs` rather than `avgMs` because the report has to say which statistic it is
      /// quoting -- and it is the same statistic in both modes, which is what makes the
      /// before/after a comparison rather than two numbers side by side.
      meanMs: () => (stats.tiles > 0 ? stats.totalMs / stats.tiles : null),
      /// Mean worker-side rasterisation per tile, or null on the synchronous path. This is
      /// work the camera does NOT wait for; it is quoted so the report can say the cost was
      /// moved rather than pretend it vanished.
      meanWorkerMs: () => (stats.poolRasters > 0 ? stats.workerMs / stats.poolRasters : null),
      /// Mean request-to-settle wall clock per tile, which includes queueing behind other
      /// tiles. It is what "tiles-to-settle" is made of, and it is not what the main thread
      /// blocks for.
      meanWallMs: () => (stats.poolRasters > 0 ? stats.wallMs / stats.poolRasters : null),
    },
  };

  return provider;
}
