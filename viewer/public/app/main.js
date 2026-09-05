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
import { createTerrainProvider, FAULTS, HEIGHTMAP_SIZE, MAX_LEVEL } from "./terrain.js";
import { TileCache, TilePool, DEFAULT_WORKERS, DEFAULT_CACHE_TILES } from "./pool.js";
import { createAvailability, FEATURE_CEILING } from "./availability.js";
import { runChecks, formatChecks } from "./verify.js";
import { runBench, formatBench, frameTrace } from "./bench.js";

const params = new URLSearchParams(location.search);
const number = (name, fallback) => (params.has(name) ? Number(params.get(name)) : fallback);

/// The default world is the one this slice's fixtures pin: `Surface::new(20260904,
/// 6_371_000, 12, 0.29, None)`. The extraction witnessed an elevation on it three
/// independent ways -- Python wheel, native Rust, browser WASM -- so it is the world with a
/// known answer at a named point, and that is why it is the default rather than something
/// prettier.
export const DEFAULT_WORLD = {
  seed: 20260904,
  radiusM: 6371000,
  plateCount: 12,
  landFraction: 0.29,
};

/// The extraction's harbour: a 900 x 260 m carve to -12 m with a 200 x 60 m mole to +4 m
/// inside it, both on bearing 35 deg, at 18.25 S 121.5 E. Off by default -- a bare world is
/// what the zoom-cap reasoning is about, and this is what contradicts it.
export const HARBOUR = [
  {
    latitudeDeg: -18.25, longitudeDeg: 121.5, targetM: -12, lengthM: 900, widthM: 260,
    bearingDeg: 35, compose: "carve", substrate: "derive",
  },
  {
    latitudeDeg: -18.25, longitudeDeg: 121.5, targetM: 4, lengthM: 200, widthM: 60,
    bearingDeg: 35, compose: "raise", substrate: "derive",
  },
];

function worldSpecFromParams() {
  return {
    seed: params.has("seed") ? params.get("seed") : DEFAULT_WORLD.seed,
    radiusM: number("radius", DEFAULT_WORLD.radiusM),
    plateCount: number("plates", DEFAULT_WORLD.plateCount),
    landFraction: number("land", DEFAULT_WORLD.landFraction),
    features: params.has("harbour") ? HARBOUR : [],
  };
}

/// A hypsometric ramp, drawn on a canvas at runtime.
///
/// This is the only reason the picture says anything: with `baseLayer: false` there is no
/// imagery at all, so an unpainted globe is one flat colour and a screenshot of it is
/// indistinguishable from a screenshot of a smooth ellipsoid. `Material.ElevationRampType`
/// colours each fragment by `materialInput.height`, which is the terrain height this
/// provider supplied -- so if the ramp shows a coastline, the coastline came from the
/// engine. No network: the ramp is a 256 x 1 canvas.
function elevationRamp() {
  const canvas = document.createElement("canvas");
  canvas.width = 256;
  canvas.height = 1;
  const ctx = canvas.getContext("2d");
  const gradient = ctx.createLinearGradient(0, 0, 256, 0);
  // The stops are placed against a -9000..+6000 m ramp, so 0 m -- sea level, the datum --
  // sits at 0.6 and the colour changes hard across it. A soft transition there would hide
  // exactly the thing being checked.
  //
  // Below sea level the ramp now carries depth rather than one flat blue: an abyssal
  // near-black, a basin blue, and a bright shelf immediately under the coast. The shelf
  // stop is what draws the pale rim around every landmass, and it is the engine's
  // bathymetry doing it, not a halo effect.
  //
  // Above it the land is banded by height the way a physical atlas is -- lowland green,
  // upland ochre, bare rock, then snow -- with the snow band deliberately narrow so it
  // reads as caps and ridges rather than a white hemisphere. This is still colour-by-height
  // only: it cannot put rock on a steep face at low altitude, because a ramp gets height
  // and nothing else. Slope needs a per-fragment normal, which needs the relief-imagery
  // work that is a slice of its own.
  gradient.addColorStop(0.00, "#020a14");   // abyssal plain
  gradient.addColorStop(0.28, "#04182e");
  gradient.addColorStop(0.47, "#0a3358");   // basin
  gradient.addColorStop(0.565, "#14548c");
  gradient.addColorStop(0.592, "#2f86bd");  // shelf, just under the coast
  gradient.addColorStop(0.5985, "#7ec5df");
  gradient.addColorStop(0.6, "#ddcfa8");    // the datum: strand
  gradient.addColorStop(0.615, "#8f9a5e");
  gradient.addColorStop(0.66, "#4a7a3c");   // lowland
  gradient.addColorStop(0.74, "#5d7440");
  gradient.addColorStop(0.82, "#7d7150");   // upland
  gradient.addColorStop(0.89, "#8e8272");
  gradient.addColorStop(0.945, "#b9b2a8");  // bare rock
  gradient.addColorStop(0.975, "#e8e6e2");
  gradient.addColorStop(1.0, "#ffffff");    // snow
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
  viewer.scene.verticalExaggeration = number("exaggeration", 1);

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

  if (params.get("paint") !== "0") {
    const material = Cesium.Material.fromType("ElevationRamp");
    material.uniforms.image = elevationRamp();
    // -9000..+6000 spans every height the engine can produce, but this generator's land
    // tops out far below +6000, so the upper third of the ramp -- rock and snow -- never
    // got used and every continent rendered in two greens. Narrowing the window to the
    // range the terrain actually occupies is what puts the bands back on the mountains.
    // Both ends stay overridable, and `?rampMax=6000` restores the old framing exactly.
    material.uniforms.minimumHeight = number("rampMin", -7000);
    material.uniforms.maximumHeight = number("rampMax", 2400);
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
    `features=${spec.features.length} | terrain=${provider.constructor.name} ` +
    `${provider.worldbuilder.size}x${provider.worldbuilder.size} ground cap=` +
    `${provider.worldbuilder.maxLevel} feature cap=${availability.featureMaxLevel} | ` +
    `workers=${pool ? pool.ready.length : 0} cache=${cache ? cache.capacity : "off"} | ` +
    `fault=${fault ?? "none"}`;
  if (status) status.textContent = line;

  window.__wb = {
    engine, provider, viewer, spec, fault,
    world, reference, pool, cache, availability,
    FAULTS,
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

boot().catch((error) => {
  window.__wbReady = { ok: false, error: String(error && error.stack ? error.stack : error) };
  const status = document.getElementById("status");
  if (status) status.textContent = `FAILED: ${error}`;
  console.error("[worldbuilder] boot failed", error);
});
