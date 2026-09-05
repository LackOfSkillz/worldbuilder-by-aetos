// Node-native tests for relief.js. No framework: `node:test` + `node:assert/strict`, run
// with `node --test viewer/test/` (or `npm test` from `viewer/`). Nothing here touches
// Cesium or the DOM -- relief.js is a pure function over plain rectangle objects and the
// real engine, so it is testable the same way the Rust side is: against the real wasm,
// with no browser required.
//
// Population/method/host, named once so every number below can be traced:
//   - World: Surface::new(20260904, 6_371_000, 12, 0.29, None) -- DEFAULT_WORLD in main.js,
//     the fixture with a witnessed elevation.
//   - Host: node v22.17.0, this repository's checked-in
//     viewer/public/wasm/worldbuilder_engine.wasm, loaded directly (no fetch, no Engine.load
//     -- see loadEngine() below).
//   - Tiles: an 8x4 scan at 45deg x 45deg over the whole globe, picking the highest-relief
//     and lowest (most negative) tiles by sampling their four corners and centre with
//     wb_elevation_m -- 8*4*5 = 160 scalar calls, once, in `before()`.

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { Engine } from "../public/app/engine.js";
import {
  reliefTile,
  luminanceStats,
  hasStructure,
  sunDirectionEnu,
  DEFAULT_SUN,
  DEFAULT_SUN_AZIMUTH_DEG,
  DEFAULT_SUN_ALTITUDE_DEG,
} from "../public/app/relief.js";

const DEFAULT_WORLD = { seed: 20260904, radiusM: 6371000, plateCount: 12, landFraction: 0.29 };

/// Load the real wasm directly from disk. `Engine.load()` uses `fetch`, which has no
/// meaning for a relative `/wasm/...` path outside a browser; this does the same
/// `instantiate` call `Engine.load` does, just fed bytes from `fs` instead of `fetch`.
async function loadEngine() {
  const wasmPath = fileURLToPath(new URL("../public/wasm/worldbuilder_engine.wasm", import.meta.url));
  const bytes = readFileSync(wasmPath);
  const { instance } = await WebAssembly.instantiate(bytes, {});
  return new Engine(instance);
}

/// A rectangle in degrees, the shape `reliefTile` expects. `relief.js` never sees a Cesium
/// `Rectangle` -- Task 3 is what will hand it one, converted at that boundary.
function rect(northDeg, southDeg, westDeg, eastDeg) {
  return { northDeg, southDeg, westDeg, eastDeg };
}

let engine;
let world;
let mountainRect;
let abyssalRect;
let mountainPeakM;
let abyssalDepthM;

test.before(async () => {
  engine = await loadEngine();
  world = engine.newWorld(DEFAULT_WORLD);

  // Coarse 8x4 scan over 45deg tiles: for each, sample the 4 corners and the centre, and
  // score it by (max - min) of those 5 samples -- crude relief -- while also tracking the
  // single most negative sample seen anywhere, for the abyssal candidate.
  let bestRelief = -Infinity;
  let bestPeak = -Infinity;
  let worstDepth = Infinity;
  for (let iy = 0; iy < 4; iy += 1) {
    const north = 90 - iy * 45;
    const south = north - 45;
    for (let ix = 0; ix < 8; ix += 1) {
      const west = -180 + ix * 45;
      const east = west + 45;
      const samples = [
        engine.elevationM(world, north, west),
        engine.elevationM(world, north, east),
        engine.elevationM(world, south, west),
        engine.elevationM(world, south, east),
        engine.elevationM(world, (north + south) / 2, (west + east) / 2),
      ];
      const max = Math.max(...samples);
      const min = Math.min(...samples);
      const relief = max - min;
      if (relief > bestRelief) {
        bestRelief = relief;
        mountainRect = rect(north, south, west, east);
        mountainPeakM = max;
      }
      if (min < worstDepth) {
        worstDepth = min;
        abyssalRect = rect(north, south, west, east);
        abyssalDepthM = min;
      }
    }
  }
});

test.after(() => {
  if (engine && world) engine.freeWorld(world);
});

test("sun direction: 315deg/45deg hillshade default is a unit vector pointing NW-and-up", () => {
  const sun = sunDirectionEnu(DEFAULT_SUN_AZIMUTH_DEG, DEFAULT_SUN_ALTITUDE_DEG);
  const len = Math.sqrt(sun.east ** 2 + sun.north ** 2 + sun.up ** 2);
  assert.ok(Math.abs(len - 1) < 1e-9, `sun direction must be unit length, got ${len}`);
  assert.ok(sun.east < 0, "azimuth 315 (from the NW) must have a westward component");
  assert.ok(sun.north > 0, "azimuth 315 (from the NW) must have a northward component");
  assert.ok(sun.up > 0, "altitude 45deg above the horizon must have a positive up component");
  assert.deepEqual(DEFAULT_SUN, sun, "DEFAULT_SUN must be the named default, not a different fixed value");
});

test("reliefTile returns an ImageData-shaped size x size RGBA raster, fully opaque", () => {
  const image = reliefTile({
    rectangle: mountainRect, size: 64, engine, worldHandle: world, radiusM: DEFAULT_WORLD.radiusM,
  });
  assert.equal(image.width, 64);
  assert.equal(image.height, 64);
  assert.equal(image.data.length, 64 * 64 * 4);
  let allOpaque = true;
  for (let i = 3; i < image.data.length; i += 4) {
    if (image.data[i] !== 255) allOpaque = false;
  }
  assert.ok(allOpaque, "every alpha texel must be 255 -- this is relief, not a mask");
});

test("a mountainous tile's raster has real luminance structure (not flat grey)", () => {
  const image = reliefTile({
    rectangle: mountainRect, size: 128, engine, worldHandle: world, radiusM: DEFAULT_WORLD.radiusM,
  });
  const stats = luminanceStats(image);
  // Named so a failure says what was actually measured, not just "assertion failed".
  assert.ok(
    stats.stdDev > 2,
    `mountain tile (peak ${mountainPeakM.toFixed(1)} m in ${JSON.stringify(mountainRect)}): ` +
    `luminance stdDev ${stats.stdDev.toFixed(3)} is not real spread`,
  );
  assert.ok(
    stats.distinctBins >= 8,
    `mountain tile: only ${stats.distinctBins} distinct luminance values across ${stats.n} texels`,
  );
  const check = hasStructure(image);
  assert.ok(check.ok, `hasStructure refused a real mountain tile: ${JSON.stringify(check)}`);
});

test("a mountainous tile is measurably different from an abyssal one", () => {
  const mountain = reliefTile({
    rectangle: mountainRect, size: 128, engine, worldHandle: world, radiusM: DEFAULT_WORLD.radiusM,
  });
  const abyssal = reliefTile({
    rectangle: abyssalRect, size: 128, engine, worldHandle: world, radiusM: DEFAULT_WORLD.radiusM,
  });
  const mStats = luminanceStats(mountain);
  const aStats = luminanceStats(abyssal);
  assert.notEqual(
    mStats.mean, aStats.mean,
    `mountain (peak ${mountainPeakM.toFixed(1)} m) and abyssal (depth ${abyssalDepthM.toFixed(1)} m) ` +
    `tiles produced the identical mean luminance ${mStats.mean}`,
  );
  // The abyssal tile should be markedly darker on average -- deep water is a near-black
  // band in the colour ramp regardless of slope.
  assert.ok(
    aStats.mean < mStats.mean,
    `abyssal mean luminance ${aStats.mean.toFixed(2)} is not darker than mountain mean ${mStats.mean.toFixed(2)}`,
  );
});

test("hasStructure can fail: a constant image is refused", () => {
  const size = 32;
  const data = new Uint8ClampedArray(size * size * 4);
  for (let i = 0; i < data.length; i += 4) {
    data[i] = 128; data[i + 1] = 128; data[i + 2] = 128; data[i + 3] = 255;
  }
  const flat = { data, width: size, height: size };
  const stats = luminanceStats(flat);
  assert.equal(stats.stdDev, 0, "a constant image must measure zero spread");
  const check = hasStructure(flat);
  assert.equal(check.ok, false, "hasStructure passed a flat grey image -- the check cannot fail");
});

test("central differences use margin, not one-sided edge differences: interior row 0 and the tile centre agree in kind", () => {
  // A tile-edge seam would show up as a visibly different normal/shade regime at row 0 vs
  // the interior, because a one-sided difference at the edge is a different (and biased)
  // estimator of the slope than the two-sided one used everywhere else. This does not
  // re-render the neighbour tile (that needs a provider, Task 3); it asserts the one thing
  // Task 1 can: that every output row, including row 0 and row size-1, was computed the
  // same way, by requesting a grid that is size+2 wide/tall from the engine.
  let requestedWidth = null;
  let requestedHeight = null;
  const spyEngine = {
    fillTileF32(request) {
      requestedWidth = request.width;
      requestedHeight = request.height;
      return engine.fillTileF32(request);
    },
    elevationM: (...args) => engine.elevationM(...args),
  };
  const size = 16;
  reliefTile({ rectangle: mountainRect, size, engine: spyEngine, worldHandle: world, radiusM: DEFAULT_WORLD.radiusM });
  assert.equal(requestedWidth, size + 2, "must sample an (n+2) grid so edge texels get real neighbours");
  assert.equal(requestedHeight, size + 2, "must sample an (n+2) grid so edge texels get real neighbours");
});
