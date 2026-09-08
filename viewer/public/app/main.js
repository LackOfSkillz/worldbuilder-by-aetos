//! Wiring: build a world in the engine, hang a terrain provider off it, and hand the
//! `Viewer` that Task 1 already created a planet to draw.
//
// This module runs *after* the classic scripts in `index.html`, which is what a
// `<script type="module">` guarantees, so `window.viewer` and the global `Cesium` are both
// there. Cesium stays a global because the vendored build is the IIFE one; there is no
// bundler in this project and none is needed.
//
// Everything is driven by URL parameters so a check can ask for a *different* world without
// a code change -- including the deliberately wrong ones.

import { Engine } from "./engine.js";
import { holdUntilRendered } from "./loading.js";
import { riverFromRoute, soundChannel } from "./river.js";
import {
  DEFAULT_EXAGGERATION, DEFAULT_WORLD, HARBOUR, RAMP_STOPS, RAMP_WINDOW, rampStopFraction,
} from "./panel-fields.js";
import { reliefFromParams } from "./relief-params.js";
import { tectonicFromParams } from "./tectonic-params.js";
import { coastFromParams } from "./coast-params.js";
import { gullyFromParams } from "./gully-params.js";
import { applyAtmosphere, formatAtmosphere } from "./atmosphere-params.js";
import {
  biomeColourEnabled, engineClimateEnabled, createReliefImageryProvider,
  reliefLayerEnabled, RELIEF_TILE_SIZE,
  COARSE_RELIEF_TILE_SIZE, COARSE_RELIEF_BELOW_LEVEL,
} from "./relief-provider.js";
import {
  cloudCoverFromParams, cloudLayerEnabled, createCloudImageryProvider, followCamera,
  DEFAULT_CLOUD_CACHE_TILES,
} from "./cloud-provider.js";
import { CLOUD_MAX_LEVEL, CLOUD_TILE_SIZE } from "./clouds.js";
import { CLIMATE_RASTER } from "./biome.js";
import {
  dilateBodyExtents, waterDiagnostics, waterEnabled, waterNodeCountFromParams,
} from "./water.js";
import { createTerrainProvider, FAULTS, HEIGHTMAP_SIZE, MAX_LEVEL } from "./terrain.js";
import { TileCache, TilePool, DEFAULT_WORKERS, DEFAULT_CACHE_TILES } from "./pool.js";
import { createAvailability, FEATURE_CEILING } from "./availability.js";
import { runChecks, formatChecks } from "./verify.js";
import { runBench, formatBench, frameTrace } from "./bench.js";
import { WorldSwapper, swapPlan } from "./live-swap.js";

const params = new URLSearchParams(location.search);
const number = (name, fallback) => (params.has(name) ? Number(params.get(name)) : fallback);

/// `DEFAULT_WORLD` and `HARBOUR` moved to `panel-fields.js` and are re-exported here so the
/// old import path still resolves. They moved because `controls.js` held a second copy of
/// every one of those numbers and the two drifted twice; there is now one copy, which both
/// this file and the panel import.
export { DEFAULT_WORLD, HARBOUR };

function worldSpecFromParams() {
  return {
    seed: params.has("seed") ? params.get("seed") : DEFAULT_WORLD.seed,
    radiusM: number("radius", DEFAULT_WORLD.radiusM),
    plateCount: number("plates", DEFAULT_WORLD.plateCount),
    landFraction: number("land", DEFAULT_WORLD.landFraction),
    // `?river=<route name>` carves a saved route into the ground before the world is
    // built. It is filled in during boot, because reading the route is a fetch and this
    // function is not async - see `boot`, which awaits it and rebuilds the spec.
    features: params.has("harbour") ? HARBOUR : [],
    // `relief` and `tectonics` are filled in during boot, once the engine can be asked what
    // canonical is. Absent here on purpose: there is no relief or tectonic default in this
    // file to drift from the engine's, which is the shape the ramp defaults got wrong once
    // already.
    relief: null,
    tectonics: null,
    coast: null,
    gully: null,
  };
}

/// A hypsometric ramp, drawn on a canvas at runtime.
///
/// This is the only reason the picture says anything when the relief layer is off: with
/// `baseLayer: false` there is no imagery at all, so an unpainted globe is one flat colour
/// and a screenshot of it is indistinguishable from a screenshot of a smooth ellipsoid.
/// `Material.ElevationRampType` colours each fragment by `materialInput.height`, which is the
/// terrain height this provider supplied -- so if the ramp shows a coastline, the coastline
/// came from the engine. No network: the ramp is a 256 x 1 canvas.
///
/// This is still colour-by-height only: it cannot put rock on a steep face at low altitude,
/// because a ramp gets height and nothing else. That is what `relief.js` is for, and it is
/// why the relief layer is the default and this is the `?relief=0` fallback.
function elevationRamp(minimumHeight, maximumHeight) {
  const canvas = document.createElement("canvas");
  canvas.width = 256;
  canvas.height = 1;
  const ctx = canvas.getContext("2d");
  const gradient = ctx.createLinearGradient(0, 0, 256, 0);
  // The stop table and the metres-to-fraction map both live in `panel-fields.js`, next to
  // the window whose two ends the panel drives, so `node --test` can hold the same table
  // this canvas is drawn from. `rampStopFraction` clamps: a stop outside the window still
  // anchors its colour at the edge it fell off, so narrowing the window darkens the deep end
  // instead of deleting it. `addColorStop` accepts repeated offsets and takes the last,
  // which is the behaviour that makes the clamp safe.
  for (const [metres, color] of RAMP_STOPS) {
    gradient.addColorStop(rampStopFraction(metres, minimumHeight, maximumHeight), color);
  }
  ctx.fillStyle = gradient;
  ctx.fillRect(0, 0, 256, 1);
  return canvas;
}

async function boot() {
  const status = document.getElementById("status");
  const viewer = window.viewer;
  const spec = worldSpecFromParams();
  const fault = params.get("fault");
  if (fault && !Object.values(FAULTS).includes(fault)) {
    throw new Error(`unknown fault "${fault}"; expected one of ${Object.values(FAULTS)}`);
  }

  const engine = await Engine.load();

  // The relief block, and RULING 1 in one line: with no relief parameter in the query string
  // `reliefFromParams` returns `null`, which reaches the engine as a null pointer and a
  // length of zero -- `None`, the canonical path, byte-for-byte the world this viewer built
  // before the relief channel existed. Canonical is read FROM THE ENGINE rather than
  // restated here, so there is no second copy of a default to drift from the first.
  //
  // Deliberately after `Engine.load()` and before the pool: the workers are handed this same
  // `spec` by `structuredClone`, so a relief block chosen here reaches every worker's own
  // `wb_world_new_relief` call and the tiles they fill are the same planet as the main
  // thread's. A relief block applied on only one side would be the `stale-worker` fault
  // shape, arrived at by accident.
  const reliefCanonical = engine.reliefPreset("canonical");
  spec.relief = reliefFromParams(params, reliefCanonical);

  // The tectonic block, and RULING 1 of the mountains slice in one more line. Same shape as
  // the relief block above and for the same reasons -- canonical is read FROM THE ENGINE,
  // `tectonicFromParams` returns `null` when nothing was asked for, and that `null` reaches
  // `wb_world_new_tectonic` as a null pointer with a length of zero.
  //
  // **This is the block that can actually move a mountain.** The peak on the owner's world is
  // 98.9% tectonic (1,454.04 m, of which 1,437.81 m is structural), which is why the relief
  // panel's note says mountain height is tectonic and why this one exists at all.
  //
  // Placed beside the relief read and before the pool for the identical reason: the workers
  // are handed this same `spec` by `structuredClone`, so a tectonic block chosen here reaches
  // every worker's own constructor and the tiles they fill are the same planet as the main
  // thread's. Applied on only one side it would be the `stale-worker` fault shape, arrived at
  // by accident -- and it would look entirely plausible.
  const tectonicCanonical = engine.tectonicPreset("canonical");
  spec.tectonics = tectonicFromParams(params, tectonicCanonical);

  // The coast block, and RULING 1 of the fractal-coastline slice in one more line. Same shape as
  // the two above and for the same reasons -- canonical is read FROM THE ENGINE,
  // `coastFromParams` returns `null` when nothing was asked for, and that `null` reaches
  // `wb_world_new_coast` as a null pointer with a length of zero.
  //
  // **This is the block that can make a coastline fractal.** Today's is not: its length ratio is
  // flat across an eightfold change of measuring ruler, which is the estimator saying the coast
  // has no structure below its own finest octave, and the whole planet has two inlet heads.
  //
  // Placed beside the other two reads and before the pool for the identical reason: the workers
  // are handed this same `spec` by `structuredClone`, so a coast block chosen here reaches every
  // worker's own constructor and the tiles they fill are the same planet as the main thread's.
  // Applied on only one side it would be the `stale-worker` fault shape arrived at by accident --
  // and a coastline that disagreed between the terrain and the tiles is exactly what it would
  // look like.
  const coastCanonical = engine.coastPreset("canonical");
  spec.coast = coastFromParams(params, coastCanonical);

  // The gully block, and RULING 1 a fifth time. Same shape as the three above -- canonical is read
  // FROM THE ENGINE, `gullyFromParams` returns `null` when nothing was asked for, and that `null`
  // reaches `wb_world_new_gully` as a null pointer with a length of zero.
  //
  // **This is the block that puts branching valleys on a mountain flank.** The owner compared our
  // ranges to satellite photographs and said ours look "painted in with a knife"; what the
  // photographs have is dendritic V-notched valleys with snow following the ridge lines, and the
  // stream graph is 661x too coarse to ever carry them. `GullyParams` is the term that can, and
  // until this line existed it shipped in the artifact and was unreachable.
  //
  // **The `null` here is stronger than the other three.** `Surface::with_gully(None)` builds no
  // steering lattice and `elevation_m` takes a different branch, so the default path is
  // structurally the pre-gully one rather than the gully one adding zero.
  //
  // Placed beside the other three reads and before the pool for the identical reason: the workers
  // are handed this same `spec` by `structuredClone`, so a gully block chosen here reaches every
  // worker's own constructor and the tiles they fill are the same planet as the main thread's.
  const gullyCanonical = engine.gullyPreset("canonical");
  spec.gully = gullyFromParams(params, gullyCanonical);

  const size = number("size", HEIGHTMAP_SIZE);
  const maxLevel = number("maxLevel", MAX_LEVEL);

  // The worker pool. `?workers=0` keeps Task 4's synchronous main-thread fill, which is
  // both the fallback and the A/B baseline every timing figure in the report is measured
  // against -- same page, same world, same tiles, one flag apart.
  const workerCount = number("workers", DEFAULT_WORKERS);
  const pool = workerCount > 0
    ? await TilePool.start({ count: workerCount, spec, fault })
    : null;
  const reliefOn = reliefLayerEnabled(params);
  const cloudCover = cloudCoverFromParams(params);
  const lakesOn = waterEnabled(params);

  // # Why `paint` is decided HERE, above the cloud layer rather than below it
  //
  // The `ElevationRamp` material composites OVER all imagery (see the block further down: the
  // material's alpha is 1 everywhere, so it hides every layer beneath it). That is already why
  // the ramp defaults to off when the relief layer is on. It applies to the cloud layer for
  // exactly the same reason, and with the ramp painting, a cloud layer would be constructed,
  // rasterised in the pool, uploaded -- and invisible.
  //
  // **Dead code looks like a feature**, and this repository has shipped three colour blends that
  // were never once selected. So the cloud layer is not built when the ramp would cover it, and
  // the status line says so rather than leaving an owner to wonder why `?relief=0&clouds=0.4`
  // shows no weather. The alternative -- changing `paint`'s default so clouds suppress the ramp
  // too -- was rejected: with `?relief=0` and no ramp there is no imagery at all and the globe is
  // one flat colour, and it would have moved the `?relief=0` picture that Task 1 recorded digests
  // for.
  const paint = params.has("paint") ? params.get("paint") !== "0" : !reliefOn;

  /// **The live-swap record**, and everything in it is replaced together or not at all.
  ///
  /// A swap that replaced the terrain provider and kept the old relief layer, or kept the old
  /// water manifest, would be the `wrong-world` fault arrived at by accident -- two halves of one
  /// picture drawn from two planets, with nothing about the render to give it away. So this is one
  /// object, `installWorld` writes all of it, and every reader below reads from here rather than
  /// from a `const` captured at boot.
  const installed = {
    state: null, world: 0, reference: 0, provider: null, cache: null, availability: null,
    reliefProvider: null, reliefLayer: null, cloudProvider: null, cloudLayer: null,
    water: null, climate: null, swaps: 0, lastSwap: null,
  };

  /// The two world handles, owned. `reference` is what the checks compare against and is always
  /// built from the *stated* parameters; `world` is what the provider draws, and under
  /// `?fault=wrong-world` they are different planets, which the checks have to notice.
  ///
  /// **Two swappers and not one.** They are two handles with two lifetimes; a single owner would
  /// have to special-case the no-fault path where they are the same number, and the failure mode
  /// of getting that wrong is a double free.
  ///
  /// **This is a real change and it is stated rather than buried:** before the live swap the
  /// no-fault path built ONE world and used it as both, so `wb_world_count` on the main thread
  /// read 1 and now reads 2. What that costs is one extra `Surface::new` -- 8 to 13 ms on the
  /// owner's world -- and one world's worth of linear memory; the whole engine heap after thirty
  /// swaps measures 39 wasm pages (2.56 MB) and does not move across them. What it buys is that
  /// `swap` is the same three lines whether or not a fault is selected. Nothing in `verify.js`
  /// compares the two by handle: every check compares VALUES, and two worlds built from one spec
  /// are bit-identical, which the engine's determinism makes an identity rather than a hope.
  const worldSwapper = new WorldSwapper(engine);
  const referenceSwapper = new WorldSwapper(engine);

  /// The tiling scheme availability is computed against. One per page: it holds no world state.
  const tilingSchemeForAvailability = new Cesium.GeographicTilingScheme();

  /// **Start the water solve, off the main thread, and return a promise of the manifest.**
  ///
  /// # Why this is a scheduling change and not an algorithm change
  ///
  /// `wb_water_run` on the main thread measured **33,925 / 42,947 / 43,467 / 44,804 ms** at boot
  /// on the owner's world at 86,000 nodes, and the browser's own `longtask` observer recorded
  /// each as a **single task**: the tab was unresponsive for the whole of it. Per live swap it
  /// was **45,428 and 53,436 ms of a 45,428/53,436 ms swap -- 99.5%.** That is 55-77% of a cold
  /// load, and it is all one call.
  ///
  /// `wb_water_run` is already an export and every pool worker already holds a world built from
  /// this same spec, so moving it is a message type. **`pool.rebuild(nextSpec)` above is awaited
  /// before this is called**, which is what makes "the same world" true rather than hopeful:
  /// same seed, same radius, same plate count, same land fraction, same `features`, and the same
  /// four opt-in blocks (relief, tectonic, coast), because the worker is sent the whole spec
  /// object. `nodeCount` is not part of the spec and rides in the request.
  ///
  /// # When it stays on the main thread, and why that is not a hedge
  ///
  /// - **`?workers=0`.** There is no pool. This is the existing escape hatch and the A/B
  ///   baseline every figure in the report is measured against.
  /// - **Any `?fault=`.** The faults are the whole point of `verify.js`, and two of them make a
  ///   worker's world deliberately *different* from this one: `stale-worker` moves exactly one
  ///   worker's seed, so dispatching the solve would make the manifest wrong or right depending
  ///   on which worker `pick()` chose -- a fault that expressed itself differently run to run,
  ///   which is worse than either answer. The faulted paths keep the behaviour they were
  ///   checked with.
  /// - **`?waterWorker=0`**, and this one exists because of a measurement rather than a
  ///   principle. **The same call is 1.8x slower in a worker on this host**, and it is not
  ///   contention: on a settled, idle page, back to back in one session, the owner's world at
  ///   86,000 nodes solved in **36,910 ms on the main thread and 67,330 ms in a worker**
  ///   (worker-side `fillMs` 67,325, so none of it is queueing). Repeated at boot across three
  ///   runs: 68,970 / 70,481 / 77,003 ms in a worker against 37,117 / 39,636 / 41,450 ms on the
  ///   main thread. **The trade is therefore real and it is a trade**: the 34-45 s single long
  ///   task becomes 0.2-5.0 s and the tab stays alive, and the wall clock to a drawn planet
  ///   gets *longer*. Chromium on this host is a 13th-gen Intel with 8 performance and 16
  ///   efficiency cores and it schedules a dedicated worker at a lower thread priority than the
  ///   renderer's main thread; that is the obvious suspect and it has **not** been proved, so
  ///   it is named as a suspect. This flag is how an owner, or a different host, gets the other
  ///   side of the trade without editing a file.
  ///
  /// The solve is deterministic -- `live-swap.js`'s control measured an identical rebuild
  /// bit-identical -- so the worker's manifest is the main thread's manifest.
  /// `tile-worker.test.mjs` proves that against the real wasm, field by field, rather than
  /// resting on this paragraph.
  /// **Calibrate this world's climate band edges, in a pool worker.**
  ///
  /// The fifth pool consumer and the second that is not a tile. It is the same shape as
  /// `startWaterSolve` above and for the same measured reason: `wb_climate_calibration` is
  /// 4,000 elevations plus one 160-step upwind march at every land point -- roughly 190,000
  /// elevation queries -- and on the main thread that is a single uninterruptible task the
  /// browser's own long-task observer records as one freeze.
  ///
  /// **It is started beside the water solve and awaited beside it**, so the two run in
  /// parallel on different workers instead of in series. Both must be complete before the
  /// relief layer is constructed, for the identical reason: `ImageryLayer` caches the texture
  /// it is given, so a tile rasterised against edges that had not arrived would be a
  /// permanently wrongly-banded tile.
  ///
  /// `?climate=0` skips it entirely rather than resolving and discarding. `?workers=0` and any
  /// `?fault=` keep the main-thread call, exactly as the water solve does -- a faulted pool is
  /// deliberately allowed to disagree with the main thread about which planet it is on, and a
  /// calibration taken from it would then be a different world's edges.
  async function startClimateCalibration() {
    const request = {};
    const started = performance.now();
    if (!pool || fault || params.get("climateWorker") === "0") {
      const climate = engine.climateCalibration({ handle: installed.world, ...request });
      return { ...climate, ms: performance.now() - started, worker: null };
    }
    const result = await pool.climate(request);
    return {
      moistureEdges: result.moistureEdges,
      landformEdges: result.landformEdges,
      landSamples: result.landSamples,
      lapseCPerKm: result.lapseCPerKm,
      /// Wall clock, for the reason `startWaterSolve` gives: it is what the owner waited for.
      ms: performance.now() - started,
      worker: result.worker,
      workerWorldCount: result.worldCount,
      workerMs: result.fillMs,
    };
  }

  async function startWaterSolve(nextState) {
    const request = { nodeCount: nextState.waterNodes };
    const started = performance.now();
    if (!pool || fault || params.get("waterWorker") === "0") {
      const water = engine.waterRun({ handle: installed.world, ...request });
      return { ...water, ms: performance.now() - started, worker: null };
    }
    const result = await pool.water(request);
    return {
      bodies: result.bodies,
      seaLevelM: result.seaLevelM,
      /// **Wall clock, not the worker's own `fillMs`.** They differ by whatever the job spent
      /// queued behind tiles, and the number the status line has always quoted is what the
      /// owner waited for. `pool.stats().waterMs` carries the worker-side figure beside it.
      ms: performance.now() - started,
      /// Which worker answered, and its `wb_world_count` AFTER the solve. A solve builds no
      /// world, so this must be 1; it is the only place a per-worker count can be read.
      worker: result.worker,
      workerWorldCount: result.worldCount,
      workerMs: result.fillMs,
    };
  }

  /// **Build the world, resolve its water, build the providers, and put them on the globe.**
  ///
  /// Called once at boot with `plan === null`, which means "everything", and again for every live
  /// swap with a plan from `swapPlan`. **One function for both paths on purpose**: a swap that
  /// went through different code than the boot it has to agree with would be a second
  /// construction of the picture, and the digest control this task is measured by would then be
  /// comparing two implementations rather than two ways of reaching one.
  ///
  /// **The camera is never read and never written here.** That is the whole feature.
  async function installWorld(nextState, plan) {
    const nextSpec = nextState.spec;
    const rebuildWorld = plan === null || plan.rebuildWorld;
    const rebuildTerrain = plan === null || plan.rebuildTerrain;
    const resolveWater = plan === null ? nextState.waterEnabled : plan.resolveWater;

    if (rebuildWorld) {
      // Built before the old one is freed -- `WorldSwapper.swap`'s own rule -- so a refused block
      // throws with the previous world still owned and still drawn.
      referenceSwapper.swap(nextSpec);
      worldSwapper.swap(fault === FAULTS.wrongWorld
        ? { ...nextSpec, seed: (BigInt(nextSpec.seed) + 1n).toString() }
        : nextSpec);
      installed.reference = referenceSwapper.handle;
      installed.world = worldSwapper.handle;
      // **Every worker, before a single tile is requested.** Eight workers still holding the
      // previous planet IS the `stale-worker` fault: a scattered eighth of the tiles would come
      // from the world the slider moved away from, and the globe would still look like a globe.
      if (pool) await pool.rebuild(nextSpec);
    }

    // **The water manifest, re-resolved whenever the surface moved -- measured, not assumed.**
    //
    // The tempting optimisation is to skip this for a mountain slider, on the reasoning that
    // mountains are terrain and lakes are water. `live-swap.js`'s module doc holds the measurement
    // that refutes it: on the owner's world at 8,000 nodes, raising `continentCollisionM` by half
    // creates two lake bodies that did not exist and moves three more by up to 43.19 m of surface
    // level, because `wb_water_run` samples its stream graph off this world's own surface. The
    // control -- an identical rebuild -- is bit-identical, so re-solving is deterministic and
    // skipping is what would be unsound. `waterSolveIsOptional` is the one place that rule lives.
    // # It is started HERE and awaited BELOW, and the gap is the whole scheduling fix
    //
    // The solve is dispatched to a pool worker and NOT awaited yet, so the terrain provider
    // installs and the mesh paints while it runs. Nothing about a heightmap needs the manifest:
    // only the relief rasteriser's lake texels and the water overlay consume it, and both are
    // below the await.
    //
    // **What the viewer does while it solves is a choice, not an accident.** Between here and
    // the await the globe shows the terrain mesh and no imagery at all; the relief and cloud
    // layers are added only after the manifest is complete. Lakes are therefore never
    // half-drawn and no relief tile is ever rasterised against a partial manifest -- which
    // matters because `ImageryLayer` caches the texture it is given, so a tile drawn early
    // would be a permanently lake-free tile in a world that has lakes. The rejected
    // alternative was to install the relief layer with an empty manifest and replace it when
    // the solve landed: that draws every relief tile twice, and a relief tile is 66,564 engine
    // samples.
    const waterJob = resolveWater ? startWaterSolve(nextState) : null;
    // **Started here, beside the water solve, and awaited beside it.** Two independent jobs on
    // two different workers: dispatching them one after the other would add their durations
    // where running them together adds only the longer. Both are required before the relief
    // layer is built and neither is required before the terrain mesh paints.
    const climateJob = reliefOn && engineClimateEnabled(params) && biomeColourEnabled(params)
      ? startClimateCalibration()
      : null;

    if (rebuildTerrain) {
      // Feature-aware availability. With no features this is exactly the Task 4 cap: the
      // footprint list is empty, `featureMaxLevel` equals the ground cap, and every level past
      // it answers `false`.
      installed.availability = createAvailability({
        radiusM: nextSpec.radiusM,
        size,
        groundMaxLevel: maxLevel,
        features: nextSpec.features,
        ceiling: number("featureCeiling", FEATURE_CEILING),
        tilingScheme: tilingSchemeForAvailability,
        fault,
      });
      // **A NEW cache, not a cleared one.** `TileCache`'s key is `level/x/y` and carries no world
      // identity -- its own comment says so, and says why: there is exactly one world per page.
      // That was true until this task. A cache carried across a swap would serve the previous
      // planet's heights under the new planet's tile ids, which is exactly the `cache-key` fault.
      installed.cache = params.get("cache") === "0"
        ? null
        : new TileCache({ capacity: number("cacheTiles", DEFAULT_CACHE_TILES), fault });
      installed.provider = createTerrainProvider({
        engine,
        world: installed.world,
        radiusM: nextSpec.radiusM,
        size,
        maxLevel,
        fault,
        pool,
        cache: installed.cache,
        availability: installed.availability,
        credit: `worldbuilder engine, generator v${engine.generatorVersion()}`,
      });
      // Assigning the provider is what makes Cesium drop every terrain tile and re-request it.
      // The camera is not touched by this line, which is the whole reason a swap is possible.
      viewer.terrainProvider = installed.provider;
      viewer.scene.globe.depthTestAgainstTerrain = true;
    }

    // The relief imagery layer.
    //
    // **Why an imagery layer rather than terrain lighting**: `CustomHeightmapTerrainProvider`
    // gives `HeightmapTerrainData`, whose `hasVertexNormals` is `false` -- always, on that
    // class -- so `GlobeFS` lights the mesh with the *ellipsoid* normal and no amount of
    // `enableLighting` produces relief. Verified live. A raster is the only surface here that
    // can carry a normal, and Cesium picks the imagery level from the terrain tile's geometric
    // error without clamping it to the terrain level, so a 256-texel tile over a 65-post
    // rectangle is a free 4x of colour resolution.
    //
    // Built from `world`, the same handle the terrain provider draws -- a relief layer from a
    // *different* world would be the `wrong-world` fault arrived at by accident, and it would
    // look entirely plausible.
    //
    // `?relief=0` turns it off. That branch, and the `paint` default below, are the only two
    // things this block changes about the page, and with `relief=0` both land on exactly the
    // code that ran before it existed.
    // The water manifest -- **slice 5b, drawn at last.**
    //
    // `wb_water_run` has shipped in the artifact since that slice and nothing called it. This is
    // the call. It runs the whole shipped water path over a stream graph sampled from this
    // world's own surface -- basin fill, overflow resolution, the tied-plateau merge,
    // classification -- and hands back the manifest: one row per body, carrying a **surface
    // level** and a bounding box. `relief.js` draws each body as a flat sheet at its own level,
    // in the ocean's own colour table read at the depth below that level.
    //
    // **The sea is deliberately not in it.** Slice 5b's Ruling 6: §13.2 defines a mapping of
    // *named* waters and a fallback for the unnamed, and the sea is the mapping's miss rather
    // than a row in it -- the datum is carried once, in `sea_level_m`, which is echoed back here
    // rather than assumed. Ocean bodies were measured to be 86.1% of the manifest with 96.3% of
    // their boxes overlapping another, so re-adding them would be re-adding the noise that
    // removal deleted.
    //
    // # It IS resolved in a worker now, and it is still complete before the first relief tile
    //
    // **This paragraph used to say the opposite, and the reasoning it gave was sound but the
    // conclusion was wrong.** It said the solve could not move off the main thread because the
    // manifest has to be complete before the first relief tile rasterises -- Cesium caches the
    // texture it is given, so a tile drawn against a half-arrived manifest is a permanently
    // lake-free tile in a world that has lakes. All of that is still true. What it missed is
    // that "complete before the first relief tile" and "computed on the main thread" are
    // different requirements: `startWaterSolve` above dispatches it to a pool worker and the
    // relief layer is not constructed until the promise resolves, so the ordering is unchanged
    // and the 34-45 s freeze is gone. Measured on the owner's world at 86,000 nodes, this
    // repository's checked-in wasm: 4.21 s at 30,000 nodes, 0.98 s at 8,000, 9.32 s at 60,000,
    // and 33.9-44.8 s at 86,000.
    //
    // `?lakes=0` is still the escape hatch and still skips the resolution entirely rather than
    // resolving and discarding. `?workers=0` and any `?fault=` keep the main-thread call.
    // What the manifest cannot say, counted rather than left to be rediscovered: bodies whose
    // box is a single point (undrawable -- no footprint, no radius, and `rootNode` cannot be
    // turned into a position by any export), boxes wider than half the planet (polar, not
    // antimeridian -- see `water.js`), and pairs of boxes that overlap and therefore make
    // `lakeLevelAt` choose.
    // (`waterDiagnostics` is called in `installWorld`, on the RAW rows, for the reason below.)

    // **The boxes bound node CENTRES, so they are grown by one node cell before anything draws
    // them.** `water.rs::lake_body_extents` takes `Extent::from_points` over the submerged members'
    // positions, and a node stands for `4 * pi * R^2 / nodeCount` of sphere; the box is therefore
    // one cell radius short on every side, and a one-node body's box is a point rather than a cell.
    // **One cell radius is an ANGULAR radius, so the shape it grows the box into is a disc on the
    // great circle and not a bigger rectangle** -- which is why a point body draws as a spherical
    // cap and why the picture stopped being full of straight lines.
    // `dilateBodyExtents` carries the whole argument and the calibration -- including the one piece
    // of ground truth available here, that a one-node body cannot hold more than one cell of water.
    // `waterFacts` above is deliberately taken on the RAW rows: it is a statement about what the
    // manifest carries, and dilating first would make it report a fact about this file instead.
    // (`dilateBodyExtents` is applied in `installWorld`, on this world's own manifest.)

    // **The manifest, awaited at last.** Everything below this line reads it.
    if (waterJob) {
      const water = await waterJob;
      const facts = waterDiagnostics(water.bodies);
      installed.water = {
        enabled: true, nodeCount: nextState.waterNodes, ms: water.ms, worker: water.worker,
        seaLevelM: water.seaLevelM, bodies: water.bodies,
        // **The boxes bound node CENTRES, so they are grown by one node cell before anything
        // draws them.** See `dilateBodyExtents` for the whole argument; `facts` is deliberately
        // taken on the RAW rows, because it is a statement about what the manifest carries and
        // dilating first would make it report a fact about this file instead.
        drawnBodies: dilateBodyExtents(water.bodies, nextState.waterNodes), facts,
      };
    } else if (!nextState.waterEnabled) {
      installed.water = {
        enabled: false, nodeCount: nextState.waterNodes, ms: 0, seaLevelM: null, worker: null,
        bodies: [], drawnBodies: [], facts: waterDiagnostics([]),
      };
    }

    // **The calibration, awaited at last**, next to the manifest and for the same reason.
    installed.climate = climateJob ? await climateJob : null;

    if (reliefOn) {
      // **The old layer is removed and a new one added**, rather than the provider being mutated.
      // `ImageryLayer` caches the uploaded texture per tile and there is no public "invalidate";
      // keeping the layer and swapping its provider's world handle would leave every already-drawn
      // tile showing the previous planet's colour over the new planet's mesh, indefinitely.
      const previousLayer = installed.reliefLayer;
      installed.reliefProvider = createReliefImageryProvider({
        engine,
        worldHandle: installed.world,
        radiusM: nextSpec.radiusM,
        tileSize: number("reliefSize", RELIEF_TILE_SIZE),
        // **The coarse-level escape hatch, and the A/B this change is quoted against.**
        // `?reliefCoarseSize=256` restores the picture before it -- one page, one world, one
        // camera, one flag apart -- and `?reliefCoarseBelow=0` does the same by turning the
        // policy off at every level. Same shape as `?cloudCacheTiles=0` on the layer above.
        coarseTileSize: number("reliefCoarseSize", COARSE_RELIEF_TILE_SIZE),
        coarseBelowLevel: number("reliefCoarseBelow", COARSE_RELIEF_BELOW_LEVEL),
        // `undefined` means "calibrate this world's own band edges"; `null` is the height
        // ramp the layer drew before the land-colour work. See `biomeColourEnabled`.
        // **Recalibrated per swap, not carried over**: the band edges are this world's own
        // hypsometry, and reusing the previous world's would colour the new one by the old one's
        // heights -- a difference no counter would show and no exception would report.
        biome: biomeColourEnabled(params) ? undefined : null,
        // **The engine's own climate, or `null` for the noise approximation this layer drew
        // before the climate slice.** `null` is `?climate=0`, and it is byte-identical rather
        // than merely similar -- which is what makes the digest in the task report a proof.
        // Re-calibrated per swap and never carried over, for exactly the reason the band edges
        // are: these are this world's own quantiles over this world's own march.
        climate: installed.climate,
        // The A/B for the raster-size measurement: `?climateRaster=32` is the arm the task
        // report prices, one page and one flag apart.
        climateRaster: number("climateRaster", CLIMATE_RASTER),
        // The bodies, resolved above. `[]` under `?lakes=0`, which is the picture this task
        // started from.
        lakes: installed.water.drawnBodies,
        // Defaults to the *terrain's* cap, so imagery is never the thing that stops refining
        // first. Read from `maxLevel` above rather than restated, so `?maxLevel=` moves both.
        maximumLevel: number("reliefMaxLevel", maxLevel),
        credit: `worldbuilder engine relief, generator v${engine.generatorVersion()}`,
        // The same pool the terrain mesh uses, and the same `?workers=0` escape hatch. One
        // pool and not two: the contention that matters is engine instances per core, and a
        // second pool of eight would double the workers without doubling the cores.
        pool,
      });
      installed.reliefLayer = viewer.imageryLayers.addImageryProvider(installed.reliefProvider);
      // **Order is the composite, and `addImageryProvider` appends.** On a swap the cloud deck is
      // already in the collection, so a freshly-added relief layer would land ON TOP of it and the
      // (opaque) ground would hide the weather completely -- the same failure shape the block below
      // describes for the reverse order, and it would look like the cloud layer had silently
      // stopped working. `lowerToBottom` puts it back under everything, which is where the boot
      // path had it.
      viewer.imageryLayers.lowerToBottom(installed.reliefLayer);
      if (previousLayer) viewer.imageryLayers.remove(previousLayer, true);
    }

    // The cloud layer -- **difference #1 of 12 in the gap analysis**, and the element a viewer's
    // eye reads first as "photograph of a planet" rather than "diagram of a planet".
    //
    // # It is added AFTER the relief layer, and the order is the composite
    //
    // `ImageryLayerCollection` composites in index order, so the last layer added is drawn over
    // the ones before it. Clouds must be last: they are a translucent deck and the ground is what
    // shows through them. Adding them first would have Cesium blend the (opaque) relief layer over
    // them and the whole layer would silently do nothing -- the same failure shape as the
    // `ElevationRamp` material hiding the relief imagery, which is documented a few lines below
    // and was a real bug here.
    //
    // # Altitude and parallax: what was chosen, and what it costs
    //
    // An `ImageryLayer` is DRAPED ON THE TERRAIN. There is no altitude option on it and no
    // parallax: a cloud texel is painted at the ground point beneath it. The alternative -- a
    // second, slightly larger textured ellipsoid primitive floating above the globe -- is the only
    // thing in this stack that would give real parallax, and it would need its own tiling, its own
    // level-of-detail and its own request path, none of which the worker pool and provider
    // machinery this task was told to reuse would serve.
    //
    // **What that costs, computed rather than waved at:** for a deck at altitude `h` on a planet
    // of radius `R`, the ground point directly under a cloud and the ground point the cloud
    // appears over differ by an arc that is zero at the sub-camera point and grows towards the
    // limb. At the owner's radius of 4,500 km and a 10 km deck, the displacement reaches ~10 km
    // near the disc centre-to-mid and diverges only in the last few percent of the disc radius,
    // where the surface is edge-on. At the orbital camera used for this task's screenshots the
    // disc is ~700 px across, so 10 km is under a pixel over most of the disc. The visible
    // consequence is at the limb, where a real cloud deck would overhang the silhouette and this
    // one stops exactly at it. That is the honest limitation of the choice and it is named here
    // rather than discovered later.
    //
    // `?clouds=0` turns the layer off, and it is not constructed at all in that case -- see
    // `cloudLayerEnabled` for why "constructed but transparent" is not good enough.
    //
    // **The cloud layer is built ONCE and survives every swap.** Its field is a point function of
    // seed and position -- `tile-worker.js`'s `cloud` job takes no world handle at all -- and seed
    // and coverage are both on `RELOAD_ONLY`. Rebuilding it per swap would re-rasterise every cloud
    // tile in the pool to produce byte-identical texels, which is the third pool consumer's whole
    // cost paid for nothing.
    if (cloudLayerEnabled(params) && !paint && installed.cloudProvider === null) {
      installed.cloudProvider = createCloudImageryProvider({
        radiusM: nextSpec.radiusM,
        // The same seed the world was built from. Weather from a different seed would be a second
        // planet's, on a layer where nothing about the picture would give it away.
        seed: nextSpec.seed,
        cover: cloudCover,
        tileSize: number("cloudSize", CLOUD_TILE_SIZE),
        // **Deliberately NOT the terrain's cap.** The relief layer follows `maxLevel` because
        // colour must not stop refining before geometry does; the cloud field's finest structure is
        // 41 km and it is already oversampled twelve times over at level 5, so following the
        // terrain to level 12 would ask the pool for seven levels of tiles carrying no new content.
        maximumLevel: number("cloudMaxLevel", CLOUD_MAX_LEVEL),
        credit: "worldbuilder cloud layer",
        // The same pool the mesh and the relief layer use. One pool and not three: the contention
        // that matters is engine instances per core.
        pool,
        // **The raster cache that makes "built once and survives every swap" actually save
        // anything.** The paragraph above was already true and was already defeated one level
        // down: assigning `viewer.terrainProvider` discards Cesium's whole quadtree, and every
        // replacement `QuadtreeTile` re-requests imagery from every layer -- so this provider
        // was re-rasterising 72 byte-identical cloud tiles per slider release, 12.0--14.9 s of
        // worker CPU each. `?cloudCacheTiles=0` restores that, which is what the A/B is
        // measured against.
        cacheTiles: number("cloudCacheTiles", DEFAULT_CLOUD_CACHE_TILES),
      });
      installed.cloudLayer = viewer.imageryLayers.addImageryProvider(installed.cloudProvider);
      // Weather from orbit, clear air below the deck. See `followCamera`.
      installed.cloudFollow = followCamera(viewer, installed.cloudLayer);
    }

    installed.state = nextState;
    installed.swaps += plan === null ? 0 : 1;
    return installed;
  }

  // The first install: the boot path, and the only one that passes `null` for a plan.
  const bootState = {
    spec,
    waterNodes: waterNodeCountFromParams(params),
    waterEnabled: lakesOn,
    size,
    maxLevel,
    featureCeiling: number("featureCeiling", FEATURE_CEILING),
  };
  // **The river, carved into the ground before the world is built.**
  //
  // Not a layer over the terrain and not a decoration: `?river=<route>` reads a route the
  // owner drew, turns each leg into a `carve` feature, and hands them to the constructor. So
  // the tiles, the water solve, the biome colours and every elevation query all see the same
  // channel, because there is only one ground and the river is in it.
  //
  // Awaited here rather than in `worldSpecFromParams`, which is not async. A failure is
  // reported and the world is built without it - a river that will not load is a reason to
  // say so, not a reason to show nothing.
  if (params.has("river")) {
    try {
      const wanted = await riverFromRoute(params.get("river"), bootState.spec.radiusM, {
        mouthDepthM: number("riverDepth", -9),
        headDepthM: number("riverHeadDepth", -3),
        mouthWidthM: number("riverWidth", 110),
        headWidthM: number("riverHeadWidth", 45),
      });
      bootState.spec = {
        ...bootState.spec,
        features: [...bootState.spec.features, ...wanted.features],
      };
      window.__wbRiver = { name: wanted.name, points: wanted.points,
                           segments: wanted.features.length };
      console.log(`[worldbuilder] river "${wanted.name}": ${wanted.features.length} segments`);
    } catch (error) {
      window.__wbRiver = { error: String(error.message) };
      console.warn(`[worldbuilder] river refused: ${error.message}`);
    }
  }

  await installWorld(bootState, null);

  // Two scheduling knobs, neither of which changes a generated height.
  //
  // `tileCacheSize` defaults to 100, which was sized for a networked provider fetching a
  // handful of tiles. Generation here is local and a whole-planet view already asks for 16
  // tiles before a single camera move, so a cache that small evicts tiles the camera is
  // about to want again and pays to regenerate them. `preloadSiblings` is normally a
  // bandwidth decision and costs nothing when there is no network.
  //
  // Deliberately NOT changed here: `scene.fog.screenSpaceErrorFactor`, which subtracts from
  // the screen-space error and would suppress a level on descent. It is inert already,
  // because Cesium gates that subtraction on `frameState.fog.enabled` and the block above
  // disables fog outright. Left alone so the two decisions stay in one place.
  viewer.scene.globe.tileCacheSize = number("tileCache", 1000);
  viewer.scene.globe.preloadSiblings = params.get("preloadSiblings") !== "0";

  // The detail knob, and the only honest one.
  //
  // Screen-space post density is INVARIANT to heightmap width -- geometric error is
  // inversely proportional to `tileImageWidth`, so a wider tile lowers level-zero error and
  // Cesium simply stops refining a level sooner, landing on the same metres per post. A
  // 257-wide provider stops at level 0 and gives 180/256 = 0.703 deg = 78 km, which is
  // exactly what 65-wide gives at level 2. Widening tiles is a real batching win and is not
  // a detail knob; this is.
  //
  // Halving it buys one extra level at four times the tiles. Left at Cesium's default so
  // nothing changes without being asked for, and exposed so the cost can be measured rather
  // than guessed at.
  //
  // # TASK 4 MEASURED IT, AND THE DEFAULT STAYS AT 2
  //
  // This is the only knob that moves the whole-planet view, which is the view the owner's
  // complaint was about: `?reliefSize=` does not, because imagery tile width is
  // detail-invariant the same way heightmap width is (measured again below, in the report).
  // At the default orbital camera, 8 workers, hardware ANGLE/D3D11 on 32 cores:
  //
  //   sse=2 (this default)  61 relief tiles, level 3, 1.95 s to settle, 4.8 s of worker CPU
  //   sse=1                155 relief tiles, level 4, 3.35 s to settle, 14.8 s of worker CPU
  //   sse=1.5               83 relief tiles, level 3 -- 36% more tiles and NOT one more
  //                         level, because the level is a step function of this value
  //
  // 3.1x the CPU and 1.7x the time-to-settle for one extra imagery level at one camera
  // distance, on a knob that is global rather than relief-specific (it refines the terrain
  // mesh too). So the cheap one ships. `?sse=1` is what buys the extra level, and it is
  // named in the status line below and in `viewer/README.md` rather than left as folklore.
  const sse = number("sse", 2);
  viewer.scene.globe.maximumScreenSpaceError = sse;
  viewer.scene.verticalExaggeration = number("exaggeration", DEFAULT_EXAGGERATION);

  // Sun shading, on by default. Without it the globe is coloured purely by height and every
  // slope reads flat -- a heightfield rendered as a paint-by-numbers map rather than a
  // surface. It is the single largest visual difference available for one line, and it
  // costs nothing: the terrain normals already exist, they were simply unlit.
  //
  // `?flat=1` restores the unlit look, which is what every screenshot before this change
  // shows and the only way to compare like with like.
  viewer.scene.globe.enableLighting = params.get("flat") !== "1";

  // GROUND atmosphere and SKY atmosphere are different effects and only one of them was
  // ever the problem.
  //
  // # The ground atmosphere is ON, and the measurement that turned it on is below
  //
  // It was off for a **measured** reason: it washed the ocean to `(122, 172, 137)` from
  // orbit -- the same colour as land 500 m up. That was a real measurement and switching it
  // off was right at the time. **It was taken against the ocean as it was before the depth
  // retune**, whose contrast is 2.45x what it was, so the finding's basis had moved and it
  // was re-measured rather than inherited.
  //
  // **Re-measured, it does not hold.** Owner's world, `?clouds=0`, orbital camera, 17,931
  // ocean pixels and 9,671 land pixels ray-picked against the ellipsoid and their heights
  // asked of the engine (`scripts/probe.mjs`, which is committed for this reason):
  //
  //   mean ocean pixel   off (13.9, 46.2, 65.9)   on (18.5, 57.3, 78.7)   still blue-dominant
  //   mean land pixel    off (36.2, 38.7, 24.7)   on (44.2, 47.9, 31.2)
  //   ocean sd           off 18.58                on 21.31                +14.7 %
  //   ocean p5-p95       off 56.57                on 63.64                +12.5 %
  //   land/ocean chromaticity distance   off 0.2439   on 0.2315           -5.1 %
  //
  // The sea and the land are 0.2315 apart in chromaticity with it on -- **the wash it was
  // switched off for would be a distance near zero** -- and the ocean's own contrast rises
  // rather than falls. Repeated at 15,000 km and on the `?relief=0` ramp path, where the
  // original objection was actually written ("washes the RAMP to a uniform pale green"): same
  // sign, same conclusion.
  //
  // **The one cost, stated rather than buried:** the land's luminance ratio p99/p01 falls
  // 15.02 -> 12.03. The haze lifts the darkest land, so the deepest forest shadow is 20 % less
  // deep. The land's absolute spread rises (sd 27.76 -> 31.22) and its top end rises with it;
  // what is lost is the very bottom of the range.
  //
  // **And it costs nothing where a coastline lives.** Cesium fades the ground atmosphere out
  // with camera distance, so at 2,000 km and at 220 km the probe returns figures identical to
  // three decimals over 15,012 ocean and 9,026 land pixels, and the digests are byte-identical.
  // The effect exists only from far orbit, which is the one view it was wanted for.
  //
  // `?atmosphere=0` restores the previous picture exactly, which is what a like-for-like
  // comparison needs, and `?flat=1` still turns everything off together.
  //
  // # The sky atmosphere is the blue limb outside the silhouette
  //
  // It touches no ground fragment, so it cannot wash anything -- measured: `?limb=24000` moves
  // no ocean or land figure past the third decimal -- and it is most of what makes a render read
  // as a planet rather than a textured ball. On by default; `?flat=1` turns it off with the rest.
  //
  // # Everything else here is OFFERED, not shipped, and the reason is the difference between
  // # a measurement and a preference
  //
  // The limb's thickness is taste. Cesium's scale heights are Earth's real ones over an
  // Earth-sized ellipsoid, so our ring is thin because it is CORRECT; the reference
  // illustration's thick blue haze is an illustrator's convention. Measured at the whole-planet
  // camera, `?limb=24000` moves the lit band's peak from the silhouette itself (0-10 km,
  // luminance 164.5, chromaticity r 0.367 / b 0.284 -- a warm-white RIM) out to 60 km
  // (luminance 219.6, r 0.300 / b 0.345 -- a blue-cyan HALO). That is the gap analysis's
  // difference #10 in numbers, and it is still a preference, because it also opens a dark gap
  // at the silhouette that some eyes will like less than the rim. **A measured finding may
  // overturn a measured finding; a preference may not**, so it is a parameter and the report
  // carries the screenshot of it.
  //
  // Everything this block reads is applied by `atmosphere-params.js`, whose defaults are read
  // off the live `Scene` rather than restated -- so with none of these parameters the picture is
  // byte-for-byte Cesium's own, and a Cesium upgrade moves with it.
  viewer.scene.fog.enabled = false;
  const atmosphereApplied = applyAtmosphere(viewer.scene, params, Cesium);

  // The `ElevationRamp` material and the relief layer CANNOT both be on, and this is not a
  // taste call. `GlobeFS`'s `APPLY_MATERIAL` block ends in
  // `color = alphaBlend(materialColor, color)` -- the material is composited *over* the
  // imagery, and this material's alpha is 1 everywhere, so the ramp would hide the relief
  // completely and the layer would look like it had silently failed to load.
  //
  // So the *default* becomes "ramp only when there is no relief layer". `?paint` still
  // forces it either way, and the expression is written so that with `?relief=0` it reduces
  // to the previous `params.get("paint") !== "0"` for all three of paint absent, `paint=0`
  // and `paint=1`: `!reliefOn` is `true`, which is what the absent case evaluated to before.
  if (paint) {
    const material = Cesium.Material.fromType("ElevationRamp");
    // The window, from `panel-fields.js` -- the same object the panel's two sliders take
    // their defaults from, rather than a second copy of the pair. Both ends stay
    // overridable, and `?rampMax=6000` restores the pre-narrowing framing exactly; the
    // coastline stays at the datum either way now that the stops are in metres.
    const minimumHeight = number("rampMin", RAMP_WINDOW.minimumHeight);
    const maximumHeight = number("rampMax", RAMP_WINDOW.maximumHeight);
    material.uniforms.image = elevationRamp(minimumHeight, maximumHeight);
    material.uniforms.minimumHeight = minimumHeight;
    material.uniforms.maximumHeight = maximumHeight;
    viewer.scene.globe.material = material;
  }

  // `?fly=lat,lon,height[,heading,pitch]`.
  //
  // **The two optional fields are why this task touched this file.** With three fields the camera
  // looks straight down, and straight down is the one angle at which a *depth* gradient cannot be
  // judged against anything: there is no horizon and no land above the waterline in frame, so a
  // shelf-to-abyss transition is a wash with nothing to read it against. A low-angle view across a
  // shelf into deep water is what the ocean brief asks to be photographed, and the alternative was
  // a fourth private camera hack in the capture harness -- which is the file the last task
  // committed specifically to stop being rebuilt.
  //
  // Absent heading and pitch reduce to exactly the previous behaviour: `Camera.setView`'s own
  // orientation default is heading 0, pitch -90, roll 0, which is the straight-down view every
  // earlier screenshot was taken at, so no existing URL moves.
  if (params.has("fly")) {
    const [lat, lon, height, headingDeg, pitchDeg] = params.get("fly").split(",").map(Number);
    const orientation = Number.isFinite(headingDeg) || Number.isFinite(pitchDeg)
      ? {
        heading: Cesium.Math.toRadians(Number.isFinite(headingDeg) ? headingDeg : 0),
        pitch: Cesium.Math.toRadians(Number.isFinite(pitchDeg) ? pitchDeg : -90),
        roll: 0,
      }
      : undefined;
    viewer.camera.setView({
      destination: Cesium.Cartesian3.fromDegrees(lon, lat, height ?? 200000),
      ...(orientation ? { orientation } : {}),
    });
  }

  /// **The status line, rebuilt after every swap.**
  ///
  /// It used to be a `const` computed once. That cannot survive this task: a caption that still
  /// described the world before the slider moved would be worse than no caption, because every
  /// screenshot in this project's reports carries this line as its own proof of what it is a
  /// picture of. Everything it names is read out of `installed` or off the live scene.
  function statusLine() {
    const s = installed.state.spec;
    const provider = installed.provider;
    const availability = installed.availability;
    const cache = installed.cache;
    const reliefProvider = installed.reliefProvider;
    const cloudProvider = installed.cloudProvider;
    const water = installed.water;
    const climate = installed.climate;
    return `Cesium ${Cesium.VERSION} | generator v${engine.generatorVersion()} | ` +
    `seed=${s.seed} plates=${s.plateCount} land=${s.landFraction} ` +
    `features=${s.features.length} relief=${
      s.relief
        ? `mtn ${s.relief.mountainM} quiet ${s.relief.quietingStrength} pers ${
          s.relief.octavePersistence}`
        : "canonical"} tectonics=${
      s.tectonics
        // The envelope AND the structure. The structure half was added when the channel
        // widened to carry it: a diagnostic line that named three of eight fields would have
        // said "canonical" about a block whose whole shape had changed, and this line is what
        // a screenshot carries as its own caption.
        ? `mtn ${s.tectonics.continentCollisionM} m / ${
          (s.tectonics.continentCollisionWidthM / 1000).toFixed(0)} km blend ${
          s.tectonics.continentalBlend.toFixed(3)} verg ${
          s.tectonics.collisionAsymmetry.toFixed(2)} belts ${
          s.tectonics.sutureCount}x${
          (s.tectonics.sutureSpreadM / 1000).toFixed(0)}km struct ${
          s.tectonics.structureDepth.toFixed(2)}@${
          (s.tectonics.structureWavelengthM / 1000).toFixed(0)}km wander ${
          (s.tectonics.marginWarpM / 1000).toFixed(0)}@${
          (s.tectonics.marginWarpWavelengthM / 1000).toFixed(0)}km`
        : "canonical"} coast=${
      s.coast
        // The amplitude AND the schedule. A diagnostic line that named the amplitude alone would
        // say nothing about a block whose octaves or frequency had moved, and this line is what a
        // screenshot carries as its own caption.
        ? `amp ${s.coast.amplitude.toFixed(2)} band ${
          s.coast.windowSpreads} freq ${s.coast.frequency} oct ${
          s.coast.octaves} gain ${s.coast.gain} lac ${s.coast.lacunarity}`
        : "canonical"} gully=${
      s.gully
        // The amplitude AND the shape. A caption naming the amplitude alone would say nothing
        // about a block whose crest exponent or gate had moved, and the SLOPE REFERENCE is here
        // because it is the number that decides whether this term bites on this planet at all.
        ? `amp ${s.gully.amplitudeM} m cell ${s.gully.cellM} m slope ${
          s.gully.slopeReference} crest ${s.gully.crestSharpness} gate ${
          s.gully.gateElevationM}/${s.gully.gateElevationSpanM} m floor ${
          s.gully.flatEnergyFloor} steer ${s.gully.steerLatticeM} m`
        : "canonical"} | terrain=${provider.constructor.name} ` +
    `${provider.worldbuilder.size}x${provider.worldbuilder.size} ground cap=` +
    `${provider.worldbuilder.maxLevel} feature cap=${availability.featureMaxLevel} | ` +
    `workers=${pool ? pool.ready.length : 0} cache=${cache ? cache.capacity : "off"} | ` +
    `reliefLayer=${
      reliefProvider
        ? `${reliefProvider.tileWidth}px cap=${reliefProvider.maximumLevel} ${
          pool ? "workers" : "MAIN THREAD"}`
        : "off"} paint=${paint ? "ramp" : "off"} | ` +
    // **The climate, named with the numbers that decide what the land is coloured by.** A
    // caption saying "climate=on" would say nothing: the whole difference between this
    // picture and the one before it is which four moisture edges the bands were cut at, how
    // many land samples they were read from, and how coarse the raster that carries them is.
    // The DRIEST edge is quoted because it is the one the report has to be honest about --
    // Task 3 measured that 79-90% of the arid band on an Earth-sized world is dried by a
    // truncated integral rather than by a measured fetch, so `arid` currently means "beyond
    // the march's reach, and hilly" more than "measured driest".
    `climate=${
      climate
        ? `moist edges ${climate.moistureEdges.map((e) => e.toFixed(3)).join("/")} ` +
          `land ${climate.landformEdges.map((e) => Math.round(e)).join("/")} m ` +
          `from ${climate.landSamples} land samples lapse ${climate.lapseCPerKm} C/km ` +
          `${reliefProvider ? reliefProvider.worldbuilder.climateConfig.rasterSize : "?"}px raster ` +
          `in ${(climate.ms / 1000).toFixed(2)}s on ${
            climate.worker === null || climate.worker === undefined ? "main" : `w${climate.worker}`}` +
          ` (ARID IS PARTLY A TRUNCATED MARCH -- see the task report)`
        : "off (land colour is the noise approximation)"} | ` +
    // The water manifest, named with the numbers that decide what it can draw. A screenshot
    // carries this line as its own caption, and "lakes=on" would say nothing: the body count
    // depends entirely on the node count, and the DRAWABLE count is smaller than the body count
    // because a single-node body's extent is a point. Both, plus the resolution's cost and the
    // datum the engine echoed back, are here.
    `lakes=${
      water.enabled
        ? `${water.drawnBodies.length}/${water.facts.bodies} drawn (${water.facts.pointBoxes} ` +
          `point boxes drawn as a node-cell cap) @${water.nodeCount} nodes ` +
          `datum ${water.seaLevelM} m in ${(water.ms / 1000).toFixed(2)}s` +
          // WHERE it was solved, on the screenshot itself. The whole of this task's first fix
          // is that this says `w<n>` rather than `main`, and a status line that did not say
          // which would leave the one visible difference invisible.
          ` on ${water.worker === null || water.worker === undefined ? "main" : `w${water.worker}`}` +
          `${water.facts.wideBoxes > 0 ? ` WIDE=${water.facts.wideBoxes}` : ""}` +
          `${water.facts.overlappingPairs > 0 ? ` overlap=${water.facts.overlappingPairs}` : ""}` +
          `${reliefOn ? "" : " (NOT DRAWN: relief layer off)"}`
        : "off"} | ` +
    // The cloud layer, named with the number that decides its look. A screenshot carries this
    // line as its own caption, and "clouds=on" would say nothing about a layer whose entire
    // control is one coverage figure -- so the REQUESTED coverage and the threshold the
    // calibration derived from it are both here, and the reason is stated when it is off.
    `clouds=${
      cloudProvider
        ? `${cloudCover.toFixed(2)} cover thr ${
          cloudProvider.worldbuilder.clouds.threshold.toFixed(3)} sd ${
          cloudProvider.worldbuilder.clouds.sd.toFixed(3)} ${
          cloudProvider.tileWidth}px cap=${cloudProvider.maximumLevel}`
        : cloudCover <= 0
          ? "off"
          : "off (ramp material covers imagery)"} | ` +
    // The cost knob, named where it can be found. `?sse=1` buys one more imagery level at
    // the whole-planet view for ~3x the tile cost -- measured above -- and a cost setting
    // nobody can find is a setting that does not exist.
    // Atmosphere and tone, named with the numbers that decide them. A screenshot carries this
    // line as its own caption, and "atmosphere=on" would say nothing about an effect whose whole
    // argument is a set of scale heights -- so the ground switch, the limb's two scale heights,
    // the two light intensities and whether the haze follows the sun are all here, and so is
    // whether this host can tonemap at all rather than merely whether it was asked to.
    `${formatAtmosphere(atmosphereApplied)} | ` +
    `sse=${sse}${sse === 2 ? " (?sse=1 for one more level, ~3x cost)" : ""} | ` +
    // **What the swap has cost so far**, on the line every screenshot carries. A live swap that
    // silently did nothing and a live swap that worked are the same picture when the parameters
    // barely moved; this counter is what tells them apart, and the milliseconds are the
    // release-to-provider-installed figure the report quotes rather than an estimate of it.
    `swaps=${installed.swaps}${
      installed.lastSwap
        ? ` last ${installed.lastSwap.kind} ${(installed.lastSwap.ms / 1000).toFixed(2)}s`
        : ""} | ` +
    `fault=${fault ?? "none"}`;
  }

  const line = statusLine();
  if (status) status.textContent = line;

  /// **Swap the drawn world in place.** The entry point `controls.js` calls on slider release.
  ///
  /// `next` is a partial world state -- typically one of `{ tectonics }`, `{ coast }` or
  /// `{ waterNodes }` -- merged over what is currently installed. The plan decides what is
  /// rebuilt; `live-swap.js` holds the rules and the measurement behind them.
  ///
  /// Returns `{ kind, ms, changed, reason, line }`. `ms` is release-to-installed: the world
  /// build, the eight worker rebuilds, the water solve and the provider swap. **It is not
  /// time-to-repaint** -- the tiles are requested by the assignment and arrive afterwards, and
  /// conflating the two is how a swap that hangs for twenty seconds gets reported as fast. A
  /// driver measuring repaint waits on Cesium's own tile-load queue, which is what
  /// `scripts/shoot.mjs measure` already does.
  async function swap(next) {
    const previous = installed.state;
    const nextState = {
      ...previous,
      ...next,
      spec: { ...previous.spec, ...(next.spec ?? {}) },
    };
    const plan = swapPlan(previous, nextState);
    if (plan.kind === "none") return { ...plan, ms: 0, line: statusLine() };
    const started = performance.now();
    await installWorld(nextState, plan);
    installed.lastSwap = { kind: plan.kind, ms: performance.now() - started, changed: plan.changed };
    const swapped = statusLine();
    if (status) status.textContent = swapped;
    window.__wbReady = { ok: true, line: swapped };
    return { ...plan, ms: installed.lastSwap.ms, line: swapped };
  }

  window.__wb = {
    engine, viewer, fault, pool, FAULTS,
    /// **Everything world-shaped is a getter**, because a live swap replaces all of it and a
    /// captured `const` would leave a driver, the panel and `verify.js` reading the world that
    /// was on screen before the slider moved -- which is the `wrong-world` fault wearing the
    /// costume of a stale variable.
    get spec() { return installed.state.spec; },
    get provider() { return installed.provider; },
    get world() { return installed.world; },
    get reference() { return installed.reference; },
    get cache() { return installed.cache; },
    get availability() { return installed.availability; },
    /// The live swap itself, plus what it has cost. `swaps` counts the swaps that did work;
    /// `worldCount` is the engine's own live-world count on the MAIN thread, and
    /// `pool.stats().worldCounts` is the same figure inside each worker. Both are the leak
    /// evidence this design needs: a swapper that forgot to free would show them climbing while
    /// the picture stayed perfect.
    swap,
    get swaps() { return installed.swaps; },
    get lastSwap() { return installed.lastSwap; },
    worldCount: () => engine.worldCount(),
    swapCounters: () => ({
      main: engine.worldCount(),
      builtWorlds: worldSwapper.built + referenceSwapper.built,
      freedWorlds: worldSwapper.freed + referenceSwapper.freed,
      workers: pool ? pool.worldCounts : null,
    }),
    /// `null` under `?relief=0`. Its `worldbuilder.stats` is the per-tile cost this task
    /// reports and Task 4's worker move is measured against.
    get reliefProvider() { return installed.reliefProvider; },
    /// **The water manifest as the engine handed it over**, plus what it cannot say.
    ///
    /// `bodies` is `wb_water_run`'s own rows, unsorted and unfiltered -- ascending by
    /// `rootNode`, which the export documents as the contract -- so a driver picks a body BY
    /// ITS ID out of this list and asserts the drawn surface against that body's own level and
    /// box, rather than resolving a second manifest and comparing two guesses. `seaLevelM` is
    /// the datum the engine echoed back, not the one this file asked for.
    ///
    /// `facts` is `waterDiagnostics`: the counts of what the box representation cannot express.
    /// `ms` is what the resolution cost, so a report quotes the measurement rather than an
    /// estimate.
    get water() { return installed.water; },
    /// `null` under `?clouds=0` and under any configuration where the ramp material would cover
    /// it. Its `worldbuilder.clouds` is the calibration the layer is drawing with -- read rather
    /// than recalibrated, so a check cannot arrive at a different threshold and compare against
    /// that -- and its `worldbuilder.stats` is the per-tile cost the report quotes.
    get cloudProvider() { return installed.cloudProvider; },
    /// What `atmosphere-params.js` actually applied to the scene, read back rather than
    /// re-derived, so a driver asking "is the ground atmosphere on" gets the answer from the same
    /// call that set it. A second derivation is a second chance to disagree.
    atmosphere: atmosphereApplied,
    /// The engine's own relief presets, read across the boundary at boot. `controls.js`
    /// takes its slider defaults, two of its three travel ends and its preset button from
    /// here -- so the panel cannot drift from `detail.rs`, because it holds no relief number
    /// of its own to drift.
    relief: {
      canonical: reliefCanonical,
      hills: engine.reliefPreset("hills"),
      get chosen() { return installed.state.spec.relief; },
    },
    /// The engine's own tectonic presets, read across the boundary at boot. `controls.js`
    /// anchors all six mountain sliders on `canonical` and fills them from `ranges` when the
    /// preset button is pressed -- so the panel cannot drift from `tectonics.rs`, because it
    /// holds no tectonic number of its own to drift. `ranges` is read here beside `canonical`
    /// for exactly the reason `relief.hills` is: **the preset must reach the panel as fourteen
    /// NUMBERS rather than as a name**, so the owner sees what it asked for and can move it.
    /// The engine's own coast presets, read across the boundary at boot. `controls.js` anchors
    /// the amplitude slider on `canonical` and fills the whole block from `fractal` when the
    /// preset button is pressed -- so the panel cannot drift from `continentality.rs`, because it
    /// holds no coast number of its own to drift.
    coast: {
      canonical: coastCanonical,
      fractal: engine.coastPreset("fractal"),
      get chosen() { return installed.state.spec.coast; },
      /// Whether the engine would accept a block, asked of `wb_coast_check` itself. Two of this
      /// channel's bounds are joint -- the octave count is a loop bound and the finest frequency
      /// is a product of three fields -- so a panel re-deriving them in JavaScript would be a
      /// second copy of a bound and a second chance to disagree with it.
      check: (block) => engine.checkCoast(block) === 0,
    },
    /// The engine's own gully presets, read across the boundary at boot. `controls.js` anchors
    /// its one slider on `canonical` and fills the whole block from `drainage` when the preset
    /// button is pressed -- so the panel cannot drift from `detail.rs`, because it holds no gully
    /// number of its own to drift. **Above all it holds no `slope_reference`**: that field is a
    /// measurement of this generator's flanks, and a transcribed measurement is a measurement with
    /// two answers.
    gully: {
      canonical: gullyCanonical,
      drainage: engine.gullyPreset("drainage"),
      get chosen() { return installed.state.spec.gully; },
      /// Whether the engine would accept a block, asked of `wb_gully_check` itself. Three of this
      /// channel's lengths become a lattice index as `radius / length`, and the crest exponent has
      /// a floor that closes an infinite-height hazard rather than stating a domain -- so a panel
      /// re-deriving any of that in JavaScript would be a second copy of a bound.
      check: (block) => engine.checkGully(block) === 0,
    },
    tectonics: {
      canonical: tectonicCanonical,
      ranges: engine.tectonicPreset("ranges"),
      get chosen() { return installed.state.spec.tectonics; },
      /// Whether the engine would accept a block, asked of `wb_tectonic_check` itself.
      ///
      /// Task 5's `marginWarpM` is the first panel control whose travel can combine with
      /// another slider's into a record the engine refuses -- it adds to
      /// `collision_reach_m`, and the boundary holds that against `MAX_TECTONIC_RANGE_M`.
      /// The panel warns instead of finding out at generate time, and it asks the real
      /// validator rather than re-deriving the reach in JavaScript, because a second copy of
      /// a bound is a second chance to disagree with it.
      check: (block) => engine.checkTectonic(block) === 0,
    },
    /// The frame-budget measurement. Populations, not a single number.
    bench: (options = {}) => runBench({
      viewer, engine, provider: installed.provider, spec: installed.state.spec, ...options,
    }),
    /// The whole verification, callable from the console or from a driver. **Read out of
    /// `installed` at call time**, so a check run after a swap checks the world that is drawn
    /// rather than the one that booted -- which is the property that makes "a slid world equals a
    /// loaded world" checkable at all.
    check: (options = {}) => runChecks({
      viewer,
      engine,
      provider: installed.provider,
      world: installed.provider.worldbuilder.world,
      reference: installed.reference,
      spec: installed.state.spec,
      ...options,
    }),
    formatChecks,
    formatBench,
    /// Deepest tile level the quadtree has actually visited since the page loaded. This is
    /// Cesium's own debug counter, not a number this code maintains.
    maxDepthVisited: () => viewer.scene.globe._surface._debug.maxDepthVisited,
    /// **The levels Cesium is actually drawing right now**, as `{ level: tileCount }`, from its
    /// own `_tilesToRender` array rather than from anything this code maintains.
    ///
    /// It exists because "a relief tile was rasterised for a level that never reaches the render
    /// set" is otherwise an unfalsifiable claim: the provider's own `stats.levels` says what was
    /// *asked for*, and only this says what was *shown*. The two together are the whole argument
    /// for the coarse-level tile size, and either alone is a number that looks load-bearing and
    /// is not.
    renderedLevels: () => {
      const counts = {};
      for (const tile of viewer.scene.globe._surface._tilesToRender) {
        counts[tile.level] = (counts[tile.level] || 0) + 1;
      }
      return counts;
    },
  };
  window.__wbReady = { ok: true, line };
  console.log("[worldbuilder]", line);

  // **Strokes are held, not applied.** A brush shows a ghost the instant it is drawn and
  // the ground does not move until somebody commits, because rebuilding the globe costs
  // seconds and a drag lays a node every twenty-six pixels. Cheap preview, one expensive
  // commit - which is how every editor with a brush behaves, and is what makes painting
  // feel like painting rather than like waiting.
  //
  // Held records are in the *worldfile's* shape, not the engine's, so `paint.js`, the
  // layers panel and the exporter all keep reading one vocabulary. Only the commit renames
  // the fields, and only at the moment they reach `installWorld`.
  {
    let held = [];
    window.__wb.holdFeatures = (records) => {
      held = held.concat(records);
      return held.length;
    };
    window.__wb.heldFeatures = () => held.slice();
    window.__wb.discardFeatures = () => { const n = held.length; held = []; return n; };
    window.__wb.commitFeatures = async () => {
      if (!held.length) return { applied: 0 };
      const worldfileRecords = held.slice();
      held = [];
      const engineRecords = worldfileRecords.map((f) => ({
        latitudeDeg: f.latitude_deg,
        longitudeDeg: f.longitude_deg,
        targetM: f.target_m,
        lengthM: f.length_m,
        widthM: f.width_m,
        bearingDeg: f.bearing_deg || 0,
        compose: f.compose || "raise",
        substrate: f.substrate || "derive",
      }));
      bootState.spec = {
        ...bootState.spec,
        features: [...bootState.spec.features, ...engineRecords],
      };
      // The worldfile keeps its own copy in its own shape, so a save writes what the
      // generator reads rather than the engine's internal field names.
      const doc = window.__wb.lastWorldfile || (window.__wb.lastWorldfile = {});
      doc.features = (doc.features || []).concat(worldfileRecords);
      await installWorld(bootState, null);
      if (window.__wb.refreshLayers) window.__wb.refreshLayers();
      window.dispatchEvent(new CustomEvent("wb-world-rebuilt",
        { detail: { applied: engineRecords.length,
                    features: bootState.spec.features.length } }));
      return { applied: engineRecords.length };
    };
  }


  // **Hold the globe back until it is actually finished.** Cesium refines from coarse to fine,
  // so a half-loaded world looks like a finished one built badly - and the water solve lands
  // after the first tiles do, which would show a world whose lakes appear later. `?loading=0`
  // opts out, because the screenshot harness wants the canvas from the first frame and an
  // overlay of its own is the last thing a byte-comparison needs.
  if (params.get("loading") !== "0") {
    window.__wbRendered = holdUntilRendered(viewer, {
      waitFor: installed.water ? Promise.resolve(installed.water) : null,
    });
  }

  // `?trace=N` records N frame deltas starting the instant the provider is installed --
  // while tiles are actually being requested, which is the only time the fill can cost a
  // frame. Compared between `?workers=8` and `?workers=0` this is the whole point of the
  // task, and it has to be taken from boot: a trace run after everything is cached measures
  // an idle render loop.
  if (params.has("trace")) {
    window.__wbFrames = { running: true };
    frameTrace({ viewer, frames: number("trace", 300) }).then((result) => {
      window.__wbFrames = { running: false, ...result };
    });
  }
}

// **A promise, published synchronously at module evaluation.** `window.__wbReady` is set at
// the *end* of `boot`, so a later module script that wants to wait for the engine has
// nothing to wait on -- `controls.js` already carried a `typeof __wbReady.then === "function"`
// branch that has never once been taken. The relief section of the panel genuinely needs the
// engine (it reads its defaults from `wb_relief_preset`), so the wait has to be real. Module
// scripts execute in document order, so this assignment happens before `controls.js` runs.
window.__wbBoot = boot().catch((error) => {
  window.__wbReady = { ok: false, error: String(error && error.stack ? error.stack : error) };
  const status = document.getElementById("status");
  if (status) status.textContent = `FAILED: ${error}`;
  console.error("[worldbuilder] boot failed", error);
});
