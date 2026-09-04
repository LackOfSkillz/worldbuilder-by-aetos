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
import { runChecks, formatChecks } from "./verify.js";

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
  gradient.addColorStop(0.0, "#031b33");
  gradient.addColorStop(0.45, "#0b3c66");
  gradient.addColorStop(0.598, "#2f7fb8");
  gradient.addColorStop(0.6, "#d9c9a3");
  gradient.addColorStop(0.63, "#3f7a3a");
  gradient.addColorStop(0.75, "#8a7b4a");
  gradient.addColorStop(0.9, "#7a6a5a");
  gradient.addColorStop(1.0, "#ffffff");
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

  const provider = createTerrainProvider({
    engine,
    world,
    radiusM: spec.radiusM,
    size: number("size", HEIGHTMAP_SIZE),
    maxLevel: number("maxLevel", MAX_LEVEL),
    fault,
    credit: `worldbuilder engine, generator v${engine.generatorVersion()}`,
  });

  viewer.terrainProvider = provider;
  viewer.scene.globe.depthTestAgainstTerrain = true;
  viewer.scene.verticalExaggeration = number("exaggeration", 1);

  // Atmosphere off by default. With no imagery layer the hypsometric ramp *is* the
  // picture, and the ground atmosphere washes it to a uniform pale green from orbit --
  // measured: a deep-ocean point at 15,000 km read (122,172,137), the same colour as land
  // 500 m up. `?atmosphere=1` puts it back.
  if (params.get("atmosphere") !== "1") {
    viewer.scene.globe.showGroundAtmosphere = false;
    viewer.scene.skyAtmosphere.show = false;
    viewer.scene.fog.enabled = false;
  }

  if (params.get("paint") !== "0") {
    const material = Cesium.Material.fromType("ElevationRamp");
    material.uniforms.image = elevationRamp();
    material.uniforms.minimumHeight = number("rampMin", -9000);
    material.uniforms.maximumHeight = number("rampMax", 6000);
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
    `${provider.worldbuilder.size}x${provider.worldbuilder.size} maxLevel=` +
    `${provider.worldbuilder.maxLevel} | fault=${fault ?? "none"}`;
  if (status) status.textContent = line;

  window.__wb = {
    engine, provider, viewer, spec, fault,
    world, reference,
    FAULTS,
    /// The whole verification, callable from the console or from a driver.
    check: (options = {}) => runChecks({
      viewer, engine, provider, world: provider.worldbuilder.world, reference, spec, ...options,
    }),
    formatChecks,
    /// Deepest tile level the quadtree has actually visited since the page loaded. This is
    /// Cesium's own debug counter, not a number this code maintains.
    maxDepthVisited: () => viewer.scene.globe._surface._debug.maxDepthVisited,
  };
  window.__wbReady = { ok: true, line };
  console.log("[worldbuilder]", line);
}

boot().catch((error) => {
  window.__wbReady = { ok: false, error: String(error && error.stack ? error.stack : error) };
  const status = document.getElementById("status");
  if (status) status.textContent = `FAILED: ${error}`;
  console.error("[worldbuilder] boot failed", error);
});
