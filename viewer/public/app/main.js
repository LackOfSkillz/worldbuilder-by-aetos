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
import {
  DEFAULT_EXAGGERATION, DEFAULT_WORLD, HARBOUR, RAMP_STOPS, RAMP_WINDOW, rampStopFraction,
} from "./panel-fields.js";
import { reliefFromParams } from "./relief-params.js";
import { tectonicFromParams } from "./tectonic-params.js";
import {
  createReliefImageryProvider, reliefLayerEnabled, RELIEF_TILE_SIZE,
} from "./relief-provider.js";
import { createTerrainProvider, FAULTS, HEIGHTMAP_SIZE, MAX_LEVEL } from "./terrain.js";
import { TileCache, TilePool, DEFAULT_WORKERS, DEFAULT_CACHE_TILES } from "./pool.js";
import { createAvailability, FEATURE_CEILING } from "./availability.js";
import { runChecks, formatChecks } from "./verify.js";
import { runBench, formatBench, frameTrace } from "./bench.js";

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
    features: params.has("harbour") ? HARBOUR : [],
    // `relief` and `tectonics` are filled in during boot, once the engine can be asked what
    // canonical is. Absent here on purpose: there is no relief or tectonic default in this
    // file to drift from the engine's, which is the shape the ramp defaults got wrong once
    // already.
    relief: null,
    tectonics: null,
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

  // Two handles on purpose. `world` is what the provider draws; `reference` is what the
  // checks compare against, and it is always built from the *stated* parameters. Under
  // `?fault=wrong-world` they are different planets, and the checks have to notice.
  const reference = engine.newWorld(spec);
  const world = fault === FAULTS.wrongWorld
    ? engine.newWorld({ ...spec, seed: BigInt(spec.seed) + 1n })
    : reference;

  const size = number("size", HEIGHTMAP_SIZE);
  const maxLevel = number("maxLevel", MAX_LEVEL);

  // The worker pool. `?workers=0` keeps Task 4's synchronous main-thread fill, which is
  // both the fallback and the A/B baseline every timing figure in the report is measured
  // against -- same page, same world, same tiles, one flag apart.
  const workerCount = number("workers", DEFAULT_WORKERS);
  const pool = workerCount > 0
    ? await TilePool.start({ count: workerCount, spec, fault })
    : null;
  const cache = params.get("cache") === "0"
    ? null
    : new TileCache({ capacity: number("cacheTiles", DEFAULT_CACHE_TILES), fault });

  // Feature-aware availability. With no features this is exactly the Task 4 cap: the
  // footprint list is empty, `featureMaxLevel` equals the ground cap, and every level past
  // it answers `false`.
  const tilingSchemeForAvailability = new Cesium.GeographicTilingScheme();
  const availability = createAvailability({
    radiusM: spec.radiusM,
    size,
    groundMaxLevel: maxLevel,
    features: spec.features,
    ceiling: number("featureCeiling", FEATURE_CEILING),
    tilingScheme: tilingSchemeForAvailability,
    fault,
  });

  const provider = createTerrainProvider({
    engine,
    world,
    radiusM: spec.radiusM,
    size,
    maxLevel,
    fault,
    pool,
    cache,
    availability,
    credit: `worldbuilder engine, generator v${engine.generatorVersion()}`,
  });

  viewer.terrainProvider = provider;
  viewer.scene.globe.depthTestAgainstTerrain = true;

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
  const reliefOn = reliefLayerEnabled(params);
  let reliefProvider = null;
  if (reliefOn) {
    reliefProvider = createReliefImageryProvider({
      engine,
      worldHandle: world,
      radiusM: spec.radiusM,
      tileSize: number("reliefSize", RELIEF_TILE_SIZE),
      // Defaults to the *terrain's* cap, so imagery is never the thing that stops refining
      // first. Read from `maxLevel` above rather than restated, so `?maxLevel=` moves both.
      maximumLevel: number("reliefMaxLevel", maxLevel),
      credit: `worldbuilder engine relief, generator v${engine.generatorVersion()}`,
      // The same pool the terrain mesh uses, and the same `?workers=0` escape hatch. One
      // pool and not two: the contention that matters is engine instances per core, and a
      // second pool of eight would double the workers without doubling the cores.
      pool,
    });
    viewer.imageryLayers.addImageryProvider(reliefProvider);
  }

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
  // ever the problem. The measured objection stands and is preserved: ground atmosphere
  // washes the ramp to a uniform pale green from orbit -- a deep-ocean point at 15,000 km
  // read (122,172,137), the same colour as land 500 m up -- so it stays off unless
  // `?atmosphere=1` asks for it.
  //
  // The sky atmosphere is the blue limb outside the silhouette. It touches no ground
  // fragment, so it cannot wash anything, and it is most of what makes a render read as a
  // planet rather than a textured ball. On by default; `?flat=1` turns it off with the rest.
  viewer.scene.skyAtmosphere.show = params.get("flat") !== "1";
  viewer.scene.fog.enabled = false;
  if (params.get("atmosphere") !== "1") {
    viewer.scene.globe.showGroundAtmosphere = false;
  }

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
  const paint = params.has("paint") ? params.get("paint") !== "0" : !reliefOn;
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

  if (params.has("fly")) {
    const [lat, lon, height] = params.get("fly").split(",").map(Number);
    viewer.camera.setView({
      destination: Cesium.Cartesian3.fromDegrees(lon, lat, height ?? 200000),
    });
  }

  const line =
    `Cesium ${Cesium.VERSION} | generator v${engine.generatorVersion()} | ` +
    `seed=${spec.seed} plates=${spec.plateCount} land=${spec.landFraction} ` +
    `features=${spec.features.length} relief=${
      spec.relief
        ? `mtn ${spec.relief.mountainM} quiet ${spec.relief.quietingStrength} pers ${
          spec.relief.octavePersistence}`
        : "canonical"} tectonics=${
      spec.tectonics
        // The envelope AND the structure. The structure half was added when the channel
        // widened to carry it: a diagnostic line that named three of eight fields would have
        // said "canonical" about a block whose whole shape had changed, and this line is what
        // a screenshot carries as its own caption.
        ? `mtn ${spec.tectonics.continentCollisionM} m / ${
          (spec.tectonics.continentCollisionWidthM / 1000).toFixed(0)} km blend ${
          spec.tectonics.continentalBlend.toFixed(3)} verg ${
          spec.tectonics.collisionAsymmetry.toFixed(2)} belts ${
          spec.tectonics.sutureCount}x${
          (spec.tectonics.sutureSpreadM / 1000).toFixed(0)}km struct ${
          spec.tectonics.structureDepth.toFixed(2)}@${
          (spec.tectonics.structureWavelengthM / 1000).toFixed(0)}km wander ${
          (spec.tectonics.marginWarpM / 1000).toFixed(0)}@${
          (spec.tectonics.marginWarpWavelengthM / 1000).toFixed(0)}km`
        : "canonical"} | terrain=${provider.constructor.name} ` +
    `${provider.worldbuilder.size}x${provider.worldbuilder.size} ground cap=` +
    `${provider.worldbuilder.maxLevel} feature cap=${availability.featureMaxLevel} | ` +
    `workers=${pool ? pool.ready.length : 0} cache=${cache ? cache.capacity : "off"} | ` +
    `reliefLayer=${
      reliefProvider
        ? `${reliefProvider.tileWidth}px cap=${reliefProvider.maximumLevel} ${
          pool ? "workers" : "MAIN THREAD"}`
        : "off"} paint=${paint ? "ramp" : "off"} | ` +
    // The cost knob, named where it can be found. `?sse=1` buys one more imagery level at
    // the whole-planet view for ~3x the tile cost -- measured above -- and a cost setting
    // nobody can find is a setting that does not exist.
    `sse=${sse}${sse === 2 ? " (?sse=1 for one more level, ~3x cost)" : ""} | ` +
    `fault=${fault ?? "none"}`;
  if (status) status.textContent = line;

  window.__wb = {
    engine, provider, viewer, spec, fault,
    world, reference, pool, cache, availability,
    /// `null` under `?relief=0`. Its `worldbuilder.stats` is the per-tile cost this task
    /// reports and Task 4's worker move is measured against.
    reliefProvider,
    FAULTS,
    /// The engine's own relief presets, read across the boundary at boot. `controls.js`
    /// takes its slider defaults, two of its three travel ends and its preset button from
    /// here -- so the panel cannot drift from `detail.rs`, because it holds no relief number
    /// of its own to drift.
    relief: {
      canonical: reliefCanonical,
      hills: engine.reliefPreset("hills"),
      chosen: spec.relief,
    },
    /// The engine's own tectonic presets, read across the boundary at boot. `controls.js`
    /// anchors all six mountain sliders on `canonical` and fills them from `ranges` when the
    /// preset button is pressed -- so the panel cannot drift from `tectonics.rs`, because it
    /// holds no tectonic number of its own to drift. `ranges` is read here beside `canonical`
    /// for exactly the reason `relief.hills` is: **the preset must reach the panel as fourteen
    /// NUMBERS rather than as a name**, so the owner sees what it asked for and can move it.
    tectonics: {
      canonical: tectonicCanonical,
      ranges: engine.tectonicPreset("ranges"),
      chosen: spec.tectonics,
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
    bench: (options = {}) => runBench({ viewer, engine, provider, spec, ...options }),
    /// The whole verification, callable from the console or from a driver.
    check: (options = {}) => runChecks({
      viewer, engine, provider, world: provider.worldbuilder.world, reference, spec, ...options,
    }),
    formatChecks,
    formatBench,
    /// Deepest tile level the quadtree has actually visited since the page loaded. This is
    /// Cesium's own debug counter, not a number this code maintains.
    maxDepthVisited: () => viewer.scene.globe._surface._debug.maxDepthVisited,
  };
  window.__wbReady = { ok: true, line };
  console.log("[worldbuilder]", line);

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
