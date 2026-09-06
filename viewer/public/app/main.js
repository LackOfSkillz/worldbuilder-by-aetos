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
import { coastFromParams } from "./coast-params.js";
import { applyAtmosphere, formatAtmosphere } from "./atmosphere-params.js";
import {
  biomeColourEnabled, createReliefImageryProvider, reliefLayerEnabled, RELIEF_TILE_SIZE,
} from "./relief-provider.js";
import {
  cloudCoverFromParams, cloudLayerEnabled, createCloudImageryProvider,
} from "./cloud-provider.js";
import { CLOUD_MAX_LEVEL, CLOUD_TILE_SIZE } from "./clouds.js";
import {
  waterDiagnostics, waterEnabled, waterNodeCountFromParams,
} from "./water.js";
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
    coast: null,
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
  // # It is resolved SYNCHRONOUSLY, at boot, and that costs four seconds
  //
  // Measured on the owner's world through this repository's checked-in wasm: **4.21 s at
  // 30,000 nodes**, 0.98 s at 8,000, 9.32 s at 60,000. That is a real cost and it is named in
  // the status line rather than hidden.
  //
  // It is not moved into a worker, and the reason is correctness rather than effort. The
  // manifest has to be complete *before* the first relief tile rasterises: Cesium caches the
  // texture it is given, so any tile drawn while the manifest was still arriving would be a
  // permanently lake-free tile in a world that has lakes, scattered wherever the camera
  // happened to be looking first. That is the `stale-worker` fault shape arrived at by
  // accident, and it would also make the screenshot digests a race. `?lakes=0` is the escape
  // hatch and it skips the resolution entirely rather than resolving and discarding.
  const lakesOn = waterEnabled(params);
  const waterNodes = waterNodeCountFromParams(params);
  const waterStarted = performance.now();
  const water = lakesOn
    ? engine.waterRun({ handle: world, nodeCount: waterNodes })
    : { seaLevelM: null, bodies: [] };
  const waterMs = performance.now() - waterStarted;
  // What the manifest cannot say, counted rather than left to be rediscovered: bodies whose
  // box is a single point (undrawable -- no footprint, no radius, and `rootNode` cannot be
  // turned into a position by any export), boxes wider than half the planet (polar, not
  // antimeridian -- see `water.js`), and pairs of boxes that overlap and therefore make
  // `lakeLevelAt` choose.
  const waterFacts = waterDiagnostics(water.bodies);

  const reliefOn = reliefLayerEnabled(params);
  let reliefProvider = null;
  if (reliefOn) {
    reliefProvider = createReliefImageryProvider({
      engine,
      worldHandle: world,
      radiusM: spec.radiusM,
      tileSize: number("reliefSize", RELIEF_TILE_SIZE),
      // `undefined` means "calibrate this world's own band edges"; `null` is the height
      // ramp the layer drew before the land-colour work. See `biomeColourEnabled`.
      biome: biomeColourEnabled(params) ? undefined : null,
      // The bodies, resolved above. `[]` under `?lakes=0`, which is the picture this task
      // started from.
      lakes: water.bodies,
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
  const cloudCover = cloudCoverFromParams(params);
  let cloudProvider = null;
  if (cloudLayerEnabled(params) && !paint) {
    cloudProvider = createCloudImageryProvider({
      radiusM: spec.radiusM,
      // The same seed the world was built from. Weather from a different seed would be a second
      // planet's, on a layer where nothing about the picture would give it away.
      seed: spec.seed,
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
    });
    viewer.imageryLayers.addImageryProvider(cloudProvider);
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
        : "canonical"} coast=${
      spec.coast
        // The amplitude AND the schedule. A diagnostic line that named the amplitude alone would
        // say nothing about a block whose octaves or frequency had moved, and this line is what a
        // screenshot carries as its own caption.
        ? `amp ${spec.coast.amplitude.toFixed(2)} band ${
          spec.coast.windowSpreads} freq ${spec.coast.frequency} oct ${
          spec.coast.octaves} gain ${spec.coast.gain} lac ${spec.coast.lacunarity}`
        : "canonical"} | terrain=${provider.constructor.name} ` +
    `${provider.worldbuilder.size}x${provider.worldbuilder.size} ground cap=` +
    `${provider.worldbuilder.maxLevel} feature cap=${availability.featureMaxLevel} | ` +
    `workers=${pool ? pool.ready.length : 0} cache=${cache ? cache.capacity : "off"} | ` +
    `reliefLayer=${
      reliefProvider
        ? `${reliefProvider.tileWidth}px cap=${reliefProvider.maximumLevel} ${
          pool ? "workers" : "MAIN THREAD"}`
        : "off"} paint=${paint ? "ramp" : "off"} | ` +
    // The water manifest, named with the numbers that decide what it can draw. A screenshot
    // carries this line as its own caption, and "lakes=on" would say nothing: the body count
    // depends entirely on the node count, and the DRAWABLE count is smaller than the body count
    // because a single-node body's extent is a point. Both, plus the resolution's cost and the
    // datum the engine echoed back, are here.
    `lakes=${
      lakesOn
        ? `${waterFacts.drawable}/${waterFacts.bodies} drawable @${waterNodes} nodes ` +
          `datum ${water.seaLevelM} m in ${(waterMs / 1000).toFixed(2)}s` +
          `${waterFacts.wideBoxes > 0 ? ` WIDE=${waterFacts.wideBoxes}` : ""}` +
          `${waterFacts.overlappingPairs > 0 ? ` overlap=${waterFacts.overlappingPairs}` : ""}` +
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
    `fault=${fault ?? "none"}`;
  if (status) status.textContent = line;

  window.__wb = {
    engine, provider, viewer, spec, fault,
    world, reference, pool, cache, availability,
    /// `null` under `?relief=0`. Its `worldbuilder.stats` is the per-tile cost this task
    /// reports and Task 4's worker move is measured against.
    reliefProvider,
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
    water: {
      enabled: lakesOn, nodeCount: waterNodes, ms: waterMs,
      seaLevelM: water.seaLevelM, bodies: water.bodies, facts: waterFacts,
    },
    /// `null` under `?clouds=0` and under any configuration where the ramp material would cover
    /// it. Its `worldbuilder.clouds` is the calibration the layer is drawing with -- read rather
    /// than recalibrated, so a check cannot arrive at a different threshold and compare against
    /// that -- and its `worldbuilder.stats` is the per-tile cost the report quotes.
    cloudProvider,
    /// What `atmosphere-params.js` actually applied to the scene, read back rather than
    /// re-derived, so a driver asking "is the ground atmosphere on" gets the answer from the same
    /// call that set it. A second derivation is a second chance to disagree.
    atmosphere: atmosphereApplied,
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
    /// The engine's own coast presets, read across the boundary at boot. `controls.js` anchors
    /// the amplitude slider on `canonical` and fills the whole block from `fractal` when the
    /// preset button is pressed -- so the panel cannot drift from `continentality.rs`, because it
    /// holds no coast number of its own to drift.
    coast: {
      canonical: coastCanonical,
      fractal: engine.coastPreset("fractal"),
      chosen: spec.coast,
      /// Whether the engine would accept a block, asked of `wb_coast_check` itself. Two of this
      /// channel's bounds are joint -- the octave count is a loop bound and the finest frequency
      /// is a product of three fields -- so a panel re-deriving them in JavaScript would be a
      /// second copy of a bound and a second chance to disagree with it.
      check: (block) => engine.checkCoast(block) === 0,
    },
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
