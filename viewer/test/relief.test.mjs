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
import { OCEAN_STOPS, RAMP_STOPS } from "../public/app/panel-fields.js";
import { biomeColor, engineCalibration } from "../public/app/biome.js";
import {
  baseColor,
  coastDitherM,
  FOAM_DITHER_M,
  reliefTile,
  luminanceStats,
  marginedTileRequest,
  hasStructure,
  slopeColor,
  snowLineM,
  freezingLineM,
  sunDirectionEnu,
  AMBIENT,
  DEFAULT_SUN,
  DEFAULT_SUN_AZIMUTH_DEG,
  DEFAULT_SUN_ALTITUDE_DEG,
  LAND_BANDS,
  OCEAN_BANDS,
  ROCK_COLOR,
  ROCK_SLOPE_HIGH_DEG,
  ROCK_SLOPE_LOW_DEG,
  SNOW_BAND_M,
  SNOW_COLOR,
  SNOW_LINE_EQUATOR_M,
  SNOW_LINE_ZERO_LAT_DEG,
  shadeTint,
  Z_FACTOR,
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

// ---------------------------------------------------------------------------------------
// Task 2: why `minStdDev = 2` (hasStructure's default) is not a number picked to make
// today's output pass.
//
// Measured once, on this file's own fixtures (DEFAULT_WORLD, the same 8x4-scan
// mountainRect/abyssalRect named above, size=128):
//
//   flat grey (128,128,128,255 everywhere)     luminance stdDev  0.00   (0 distinct bins)
//   abyssal tile (depth -4615.6 m, weakest real spread observed)  21.00 (134 distinct bins)
//   mountain tile (peak 743.5 m)                                 37.36 (149 distinct bins)
//
// The gap the threshold has to sit in is therefore between 0 (flat) and 21.0 (the
// *weakest* real tile this fixture produces), not between 0 and the mountain's 37.36 --
// using the mountain number alone would overstate the margin. `minStdDev = 2` sits at
// under a tenth of that weaker real value, so it fails a flat raster by a wide margin
// (mutation 1, below) while asking almost nothing of a genuinely varied one -- it is
// intentionally loose in the "is there any relief signal at all" direction.
//
// **What this threshold does NOT catch, stated rather than papered over**: a partial
// amplitude regression -- e.g. a bug that blends every sampled height toward the tile's
// own mean by some fraction before shading -- keeps *some* spread. Measured on the same
// mountain tile, blending 50% of the way to the mean still leaves stdDev 12.29 (comfortably
// above 2); blending 80% of the way leaves 3.08 (still above 2); only past roughly 85-90%
// does it drop below 2. So `hasStructure`'s luminance-spread threshold is well separated
// from "no relief at all" and not from "most of the relief is gone" -- those two
// populations (a flat mutant and a moderately-flattened one) are not comfortably
// separated by any single stdDev cutoff without risking false failures on real
// low-relief terrain (the abyssal tile's own 21.0 sits closer to a tightened cutoff than
// to 2). This is a real limit of a single luminance-spread number, not a gap this task
// closes by picking a different constant -- see the "swap" mutation below for the
// discrimination this suite gets from a second, independent kind of assertion instead.
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
  // **The swap guard.** `mStats.mean !== aStats.mean` above would already be satisfied by
  // luck if a bug served the wrong tile for one of the two requests but that tile's *mean*
  // happened to differ from the real one -- means collide far more often than whole
  // buffers do. This compares the raw bytes directly: two distinct rectangles must not
  // produce byte-identical rasters. This is the assertion mutation 2 (below) is built to
  // exercise -- it is the one a "serve the mountain tile's raster for the abyssal
  // request" regression actually violates, directly, rather than through the mean.
  assert.notDeepEqual(
    Array.from(mountain.data), Array.from(abyssal.data),
    "mountain and abyssal tiles produced byte-identical rasters -- one request's output " +
    "was served for the other's",
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


// =========================================================================================
// Task 5: the z-factor, and the three colour blends that were dead code before it.
//
// Population/method/host for everything below, named once:
//   - `peakLatDeg`/`peakLonDeg`: the highest post found by a 129 x 129 scan of `mountainRect`
//     (the highest-relief 45-degree tile, found by the scan in `before()` above). Rectangles
//     are built centred on that point with the EDGE LENGTH of a level-L geographic tile
//     (180 / 2^L degrees tall), because it is the edge length -- not the tile's registration
//     -- that sets the post spacing every figure here is about.
//   - Rasters are `size = 256`, the shipped tile size, because **the shade factor IS a
//     function of `size`**: `marginedTileRequest` derives the post spacing from the tile's
//     edge over `size - 1` posts, so a 64-texel raster of a level-5 rectangle samples at a
//     level-3 spacing and measures a smoother surface. Halving the raster to keep the suite
//     quick would quietly halve the thing being measured, which is the mistake this whole
//     task exists to undo.
//   - Host: node v22.17.0, this repository's checked-in wasm.
//
// **The number this task exists to move**: Task 3 measured the shade factor -- the per-texel
// ratio of a tile rendered normally to the same tile rendered with `ambient = 1`, which turns
// shading off and leaves colour untouched -- at mean 0.808, sd 0.004, flat from level 5 to 12.
// 0.8096 is `AMBIENT + (1 - AMBIENT) * sin(45 deg)`, the shade of perfectly flat ground.

/// `AMBIENT + (1 - AMBIENT) * sin(altitude)`: the shade a texel gets when its normal is
/// straight up. Derived from the module's own constants rather than restated, so it tracks
/// them if either moves.
const FLAT_SHADE = AMBIENT + (1 - AMBIENT) * DEFAULT_SUN.up;
// The cool-to-warm axis makes the lit/unlit ratio per-CHANNEL rather than one scalar. These
// two constants let the assertions below divide it out and keep comparing against
// `FLAT_SHADE` exactly, instead of widening a bound to absorb a real change.
const FLAT_TINT = shadeTint(FLAT_SHADE);
const UNLIT_TINT = shadeTint(1);

/// The land point with the most **local relief** on `DEFAULT_WORLD`, found once and memoised.
///
/// Not `mountainRect`'s highest post, which was tried and was the wrong probe: the 45-degree
/// scan above maximises the spread of five corner samples, and on this world that picks the
/// polar tile, whose highest post sits on a flat 770 m plateau at latitude -85.8. A tile there
/// is genuinely smooth, so it would have measured this task's own change as absent. This scans
/// a 2-degree grid (40,764 `wb_elevation_m` calls, ~60 ms) and scores each land point by the
/// range of itself and its four neighbours a quarter degree away -- relief at roughly the
/// scale a mid-level tile spans. On this world it lands at 32 S 28 W with 641 m of relief over
/// half a degree, which is the same kind of place Task 3's own -9,65 probe was.
///
/// Deliberately NOT a second `test.before` hook: node:test runs top-level `before` hooks in
/// registration order but the first one here is async, and depending on a second hook having
/// seen the first one's output is a scheduling assumption this file does not need to make.
let probe = null;
function reliefProbe() {
  if (probe) return probe;
  let best = -Infinity;
  for (let latDeg = -88; latDeg <= 88; latDeg += 2) {
    for (let lonDeg = -180; lonDeg < 180; lonDeg += 2) {
      const here = engine.elevationM(world, latDeg, lonDeg);
      if (here <= 0) continue;
      const d = 0.25;
      const neighbours = [
        engine.elevationM(world, latDeg + d, lonDeg), engine.elevationM(world, latDeg - d, lonDeg),
        engine.elevationM(world, latDeg, lonDeg + d), engine.elevationM(world, latDeg, lonDeg - d),
      ];
      const relief = Math.max(here, ...neighbours) - Math.min(here, ...neighbours);
      if (relief > best) { best = relief; probe = { latDeg, lonDeg, heightM: here, reliefM: relief }; }
    }
  }
  return probe;
}

/// A rectangle the size of a level-`level` geographic tile, centred on the relief probe.
function levelRect(level) {
  const { latDeg, lonDeg } = reliefProbe();
  const half = 90 / 2 ** level;
  return rect(latDeg + half, latDeg - half, lonDeg - half, lonDeg + half);
}

/// The shade factor of one raster: the per-texel ratio of shaded to unshaded. Texels whose
/// unshaded channel is under 8 are skipped -- the ratio of two small integers is 8-bit
/// quantisation noise, not shade, and including them is what put a floor of about 0.005 under
/// Task 3's own sd.
function shadeFactor({ rectangle, level, size = 256, zFactor }) {
  const base = {
    rectangle, level, size, engine, worldHandle: world, radiusM: DEFAULT_WORLD.radiusM,
  };
  const args = zFactor === undefined ? base : { ...base, zFactor };
  const lit = reliefTile(args);
  const flat = reliefTile({ ...args, ambient: 1 });
  let n = 0;
  let sum = 0;
  let sumSq = 0;
  let far = 0;
  for (let i = 0; i < size * size; i += 1) {
    for (let c = 0; c < 3; c += 1) {
      const denominator = flat.data[i * 4 + c];
      if (denominator < 8) continue;
      // The cool-to-warm axis is divided out on BOTH sides, per channel, so this ratio
      // still measures shading alone. `flat` is rendered at ambient 1 and therefore carries
      // `shadeTint(1)`; the lit side's own tint depends on its shade, so it is recovered
      // from the ratio by one fixed-point step -- the tint varies by under 10% across the
      // whole shade range, so one step is far inside the 0.02 bound this feeds.
      const raw = (lit.data[i * 4 + c] / denominator) * UNLIT_TINT[c];
      const ratio = raw / shadeTint(raw)[c];
      n += 1;
      sum += ratio;
      sumSq += ratio * ratio;
      if (Math.abs(ratio - FLAT_SHADE) > 0.02) far += 1;
    }
  }
  const mean = sum / n;
  return {
    mean, stdDev: Math.sqrt(Math.max(0, sumSq / n - mean * mean)), n, farFraction: far / n,
  };
}

test("the hillshade is a hillshade: the shade factor varies, at every level from 5 to 12", () => {
  // The success criterion of this task, as an assertion. `0.02` is well above both the 0.004
  // Task 3 measured and the ~0.005 quantisation floor that number sat on, and well below the
  // 0.03-0.06 this file now produces -- it separates "shading" from "constant darkening"
  // without being tuned to the current value.
  for (const level of [5, 8, 12]) {
    const shade = shadeFactor({ rectangle: levelRect(level), level });
    assert.ok(
      shade.stdDev > 0.02,
      `level ${level}: shade factor sd ${shade.stdDev.toFixed(4)} over ${shade.n} channels is a `
      + "near-constant darkening, not relief",
    );
    // The spread, not the mean, is what "not a constant darkening" means -- and the mean is
    // the wrong test for it in both directions. Task 3's mean was 0.808 because every texel
    // was flat; this file's L5 mean is 0.816, ABOVE flat, because the probe's sunward aspects
    // outnumber its shaded ones. A mean assertion would have failed a working hillshade.
    assert.ok(
      shade.farFraction > 0.25,
      `level ${level}: only ${(100 * shade.farFraction).toFixed(1)}% of channels differ from the `
      + `flat-ground shade ${FLAT_SHADE.toFixed(4)} by more than 0.02 -- that is a wash, not relief`,
    );
  }
});

test("and the z-factor is what does it: at zFactor 1 the same tiles collapse to flat ground", () => {
  // The control. Without it the assertion above could be passing for some other reason and
  // `Z_FACTOR` could be doing nothing. This is Task 3's tree re-measured through the same
  // function: the shade factor must return to the flat-ground constant at every level, which
  // is exactly the finding this task was written to answer.
  for (const level of [5, 8, 12]) {
    const shade = shadeFactor({ rectangle: levelRect(level), level, zFactor: 1 });
    assert.ok(
      shade.stdDev < 0.01,
      `level ${level} at zFactor 1: sd ${shade.stdDev.toFixed(4)} -- this control is supposed to `
      + "reproduce the flat wash, so either the terrain or the measurement has changed",
    );
    // Not zero: with `zFactor` 1 the residual spread is 8-bit rounding on the darkest
    // channels (a green of 12 moves by 0.04 of its own value for one byte), so a few per cent
    // of channels clear a 0.02 ratio. Measured at 4.9% (L5) and 0.0% (L8, L12) here, against
    // 37.5%, 79.0% and 91.5% with the shipped z-factor -- the separation these bounds assert.
    assert.ok(
      shade.farFraction < 0.10,
      `level ${level} at zFactor 1: ${(100 * shade.farFraction).toFixed(1)}% of channels are more `
      + "than 0.02 from flat, so the control is not reproducing the flat wash",
    );
    assert.ok(
      Math.abs(shade.mean - FLAT_SHADE) < 0.01,
      `level ${level} at zFactor 1: mean ${shade.mean.toFixed(4)} vs flat ${FLAT_SHADE.toFixed(4)}`,
    );
  }
  assert.ok(Z_FACTOR > 1, "Z_FACTOR must exaggerate, or the control above is the shipped path");
});

test("hasStructure now passes at the levels that refused these tiles", () => {
  // Task 3: "Task 2's own hasStructure refuses these tiles from level 8 down -- luminance sd
  // 0.73 at L9 against a minStdDev of 2." Those are the levels asserted here.
  for (const level of [8, 9, 12]) {
    const image = reliefTile({
      rectangle: levelRect(level), level, size: 256, engine, worldHandle: world,
      radiusM: DEFAULT_WORLD.radiusM,
    });
    const check = hasStructure(image);
    assert.ok(
      check.ok,
      `level ${level}: hasStructure still refuses a real mountain tile: `
      + JSON.stringify(check.stats),
    );
  }
});

test("water is lit as the flat plane it is, not as its own seabed", () => {
  // A sea surface does not carry the relief under it, and a true-colour image of the ocean is
  // depth scattering rather than seabed shading. The abyssal tile is entirely below the datum,
  // so EVERY texel must carry exactly the flat-ground shade -- not approximately.
  const size = 32;
  const shared = {
    rectangle: abyssalRect, size, engine, worldHandle: world, radiusM: DEFAULT_WORLD.radiusM,
  };
  const lit = reliefTile(shared);
  const unlit = reliefTile({ ...shared, ambient: 1 });
  // Which texels are water is asked of the ENGINE, through the same margined request
  // `reliefTile` itself builds, rather than guessed from the colour: the abyssal tile is the
  // deepest 45-degree tile and 6% of it is land, so a colour-based filter would be testing
  // the filter.
  const request = marginedTileRequest({
    rectangle: abyssalRect, size, worldHandle: world, radiusM: DEFAULT_WORLD.radiusM,
  });
  const heights = engine.fillTileF32(request);
  const { grid } = request;
  let worst = 0;
  let waterTexels = 0;
  for (let row = 0; row < size; row += 1) {
    for (let col = 0; col < size; col += 1) {
      if (heights[(row + 1) * grid + (col + 1)] > 0) continue;
      waterTexels += 1;
      const i = row * size + col;
      const denominator = unlit.data[i * 4 + 2]; // blue: the channel deep water actually has
      if (denominator < 8) continue;
      // Compared in BYTES, not in ratio. Deep water's blue is around 35, so one unit of
      // `Uint8ClampedArray` rounding is 0.029 of the ratio -- three times any tolerance worth
      // asserting, and it would look exactly like a real shading leak.
      // `unlit` is rendered at ambient 1, so its own tint is `shadeTint(1)`; dividing that
      // out recovers the untinted colour before applying the flat-ground expectation.
      const base = denominator / UNLIT_TINT[2];
      worst = Math.max(worst, Math.abs(lit.data[i * 4 + 2] - base * FLAT_SHADE * FLAT_TINT[2]));
    }
  }
  assert.ok(waterTexels > 500, `only ${waterTexels} of ${size * size} texels were under water`);
  assert.ok(
    worst <= 1,
    `an under-water texel's blue is ${worst.toFixed(3)} bytes away from its flat-plane value `
    + `(colour x ${FLAT_SHADE.toFixed(4)}); the seabed is showing through the sea surface`,
  );
});

test("the rock band fires on this terrain, and the band it replaced could not have", () => {
  // The measurement that condemned the old thresholds: nothing on this planet is steeper than
  // 1.9 degrees at raster spacing, so a 22-42 degree band was unreachable by construction. The
  // band now reads the z-exaggerated slope, which is the surface actually drawn.
  assert.ok(ROCK_SLOPE_LOW_DEG < 22, "the rock band must sit below the old unreachable 22 deg");
  const green = slopeColor(500, 0);
  const steep = slopeColor(500, (ROCK_SLOPE_LOW_DEG + ROCK_SLOPE_HIGH_DEG) / 2);
  assert.notDeepEqual(green, steep, "slope does not move the colour at all");
  // Toward rock, on every channel, and at the SAME height -- which is the thing a height ramp
  // provably cannot do and the reason this layer exists at all.
  for (let c = 0; c < 3; c += 1) {
    assert.ok(
      Math.abs(steep[c] - ROCK_COLOR[c]) < Math.abs(green[c] - ROCK_COLOR[c]),
      `channel ${c}: a steep face at 500 m is not closer to rock than a flat one at 500 m`,
    );
  }
  // And the constants have to be consistent with the TERRAIN, not just with each other: the
  // steepest slope this generator produces, exaggerated, must clear the band's lower end.
  const steepestExaggeratedDeg =
    (Math.atan(Math.tan((1.9 * Math.PI) / 180) * Z_FACTOR) * 180) / Math.PI;
  assert.ok(
    steepestExaggeratedDeg > ROCK_SLOPE_LOW_DEG,
    `the steepest slope measured on this generator reaches only ${steepestExaggeratedDeg.toFixed(1)} `
    + `deg after exaggeration, below the ${ROCK_SLOPE_LOW_DEG} deg the rock band starts at`,
  );
});

test("snow follows latitude and shelter, not a contour", () => {
  // A single global elevation line draws a ring around every peak at the same height and reads
  // as a bug. Three properties, each of which a contour-ring implementation fails.
  assert.equal(snowLineM(0), SNOW_LINE_EQUATOR_M);
  assert.equal(snowLineM(SNOW_LINE_ZERO_LAT_DEG), 0);
  assert.equal(snowLineM(-60), snowLineM(60), "the snowline must be symmetric about the equator");
  assert.ok(snowLineM(30) > snowLineM(60), "the snowline must fall with latitude");

  // 1. Height alone does not decide it: the planet's highest point, at the equator, is bare.
  assert.notDeepEqual(slopeColor(1381, 0, 0), SNOW_COLOR);
  // 2. The same height at high latitude is snow.
  assert.deepEqual(slopeColor(1381, 0, 78), [...SNOW_COLOR]);
  // 3. Shelter: at the same height AND the same latitude, a steep face keeps less snow than
  //    flat ground. This is the term that breaks the ring.
  const sheltered = slopeColor(1200, 0, 70);
  const exposed = slopeColor(1200, ROCK_SLOPE_HIGH_DEG, 70);
  assert.ok(
    sheltered[0] > exposed[0] && sheltered[1] > exposed[1] && sheltered[2] > exposed[2],
    `a steep face (${exposed}) is not less snowy than flat ground (${sheltered}) at the same `
    + "height and latitude",
  );
});

test("every colour band is a height this generator reaches", () => {
  // The other half of the dead-code finding: the previous table's 1,800 m and 3,200 m land
  // stops and its -9,000 m ocean stop were outside the range this generator produces, so three
  // of eleven colours in the file could never appear. Bounds from the global fill quoted in
  // relief.js; loose on purpose -- the property is "reachable", not "exact".
  const MEASURED_MIN_M = -6807;
  const MEASURED_MAX_M = 2051;
  for (const [metres] of [...OCEAN_BANDS, ...LAND_BANDS]) {
    assert.ok(
      metres >= MEASURED_MIN_M && metres <= MEASURED_MAX_M,
      `band stop at ${metres} m is outside the ${MEASURED_MIN_M}..${MEASURED_MAX_M} m range this `
      + "generator produces, so it can never be drawn",
    );
  }
});

// ------------------------------------------------------------------------------------------
// The ocean retune: one palette, stops that are attained, and a water's edge that is not a
// contour.
//
// Population/method/host for every figure quoted below, and for the fill this file runs:
//   - Worlds: `DEFAULT_WORLD` (above) and the owner's -- seed 562423712, radius 4,500,000 m,
//     28 plates, land fraction 0.16, the engine's own `ranges` tectonic preset.
//   - Method: a `REACH_W x REACH_H` edge-inclusive global fill at canonical resolution, the
//     same `wb_fill_tile_f32` the raster uses. The headline figures in `panel-fields.js` are
//     from the same method at 2,880 x 1,440 (4,147,200 samples); this file runs it at
//     720 x 360 (259,200) so `node --test` stays in seconds, and every stop asserted below
//     clears the smaller grid by a margin stated in its own message.
//   - Host: node, this repository's checked-in `worldbuilder_engine.wasm`.
// ------------------------------------------------------------------------------------------

const REACH_W = 720;
const REACH_H = 360;

/// Sea depths from one global fill, as a plain array. Row 0 is north, edge-inclusive, matching
/// `marginedTileRequest`'s own post convention.
function seaDepths(handle) {
  const out = [];
  for (let r0 = 0; r0 < REACH_H; r0 += 30) {
    const rows = Math.min(30, REACH_H - r0);
    const buf = engine.fillTileF32({
      handle,
      lat0Deg: 90 - (180 * r0) / (REACH_H - 1),
      lat1Deg: 90 - (180 * (r0 + rows - 1)) / (REACH_H - 1),
      lon0Deg: -180, lon1Deg: 180, width: REACH_W, height: rows, resolutionM: -1,
    });
    for (let i = 0; i < buf.length; i += 1) if (buf[i] <= 0) out.push(buf[i]);
  }
  return out;
}

test("every ocean stop is a depth this generator attains, and the one that was not is named", () => {
  // **This is the check the previous one could not make.** `every colour band is a height this
  // generator reaches` asserts a BOX -- a min and a max over three worlds -- and a box whose
  // floor is the deepest sample found anywhere cannot notice a stop that no world reaches. The
  // -6,800 m stop satisfied that box for the whole of its life and was drawn zero times.
  //
  // So this one asks the engine for a distribution instead of two extremes, and asserts the
  // property that matters: for every ocean stop there is water at or below it, so its colour is
  // a colour something is actually painted.
  const ownerHandle = engine.newWorld({
    seed: 562423712, radiusM: 4500000, plateCount: 28, landFraction: 0.16,
    tectonics: engine.tectonicPreset("ranges"),
  });
  for (const [label, depths] of [
    ["DEFAULT_WORLD", seaDepths(world)],
    ["the owner's world", seaDepths(ownerHandle)],
  ]) {
    assert.ok(depths.length > 10000, `${label}: only ${depths.length} sea samples`);
    for (const [metres, hex] of OCEAN_STOPS) {
      const n = depths.reduce((count, d) => (d <= metres ? count + 1 : count), 0);
      assert.ok(
        n > 0,
        `${label}: no sample of ${depths.length} is at or below the ${hex} stop at ${metres} m, `
        + "so that colour is never drawn",
      );
    }
    // And the finding, pinned: the stop this table replaced is unreachable on BOTH worlds. If a
    // later change made -6,800 m reachable this would fail, which is the correct outcome -- the
    // sentence in `panel-fields.js` would then be wrong and would have to be re-measured.
    assert.equal(
      depths.reduce((count, d) => (d <= -6800 ? count + 1 : count), 0), 0,
      `${label}: the retired -6,800 m stop is reachable after all; re-measure the table`,
    );
  }
});

test("the two colour systems draw the ocean from ONE table", () => {
  // The defect this file and `panel-fields.js` both open by describing, in its fifth instance:
  // two copies of one palette, which had drifted. Identity, not equality -- a copy that happens
  // to hold equal values today is exactly the state the last four started in.
  for (let i = 0; i < OCEAN_STOPS.length; i += 1) {
    assert.equal(RAMP_STOPS[i], OCEAN_STOPS[i], `ramp stop ${i} is not the shared ocean stop`);
  }
  assert.equal(OCEAN_BANDS.length, OCEAN_STOPS.length);
  for (let i = 0; i < OCEAN_STOPS.length; i += 1) {
    assert.equal(OCEAN_BANDS[i][0], OCEAN_STOPS[i][0], `band ${i} is at a different depth`);
  }
  // Every stop below the datum is in the shared table and none above it is: the retune was told
  // not to touch land colour, and this is that constraint as an assertion rather than a promise.
  for (const [metres] of OCEAN_STOPS) assert.ok(metres < 0, `${metres} m is not below the datum`);
  for (const [metres] of RAMP_STOPS.slice(OCEAN_STOPS.length)) {
    assert.ok(metres >= 0, `${metres} m is below the datum but outside the shared table`);
  }
});

test("the 60 metres holding half the ocean carry a visible gradient", () => {
  // **The measured defect, as a check that can fail.** 44-49% of the sea on the two worlds lies
  // between -4,620 m and -4,560 m; the table this replaced interpolated straight through that
  // band, from -4,600 m to -1,200 m, and gave the whole of it a luminance difference of **0.5 of
  // one unit** -- which is why the ocean read flat while its bathymetry was fully in use.
  //
  // The threshold is 8 units: sixteen times what the old table produced there, half of what this
  // one does, and far above the one-unit rounding of a `Uint8ClampedArray`.
  const lum = ([r, g, b]) => 0.2126 * r + 0.7152 * g + 0.0722 * b;
  const spread = lum(baseColor(-4560)) - lum(baseColor(-4620));
  assert.ok(
    spread >= 8,
    `the abyssal plain spans ${spread.toFixed(2)} luminance units between -4620 m and -4560 m; `
    + "half the ocean is one colour again",
  );
  // Monotone brightening from the trench to the surf, across the whole table. A ramp that folded
  // back would put a bright band in the deeps and read as a seabed feature rather than as depth.
  for (let i = 1; i < OCEAN_BANDS.length; i += 1) {
    assert.ok(
      lum(OCEAN_BANDS[i][1]) > lum(OCEAN_BANDS[i - 1][1]),
      `ocean band ${i} at ${OCEAN_BANDS[i][0]} m is not brighter than the one below it`,
    );
  }
});

test("the water's edge is dithered, and the dither does not move the coastline", () => {
  // A hard rim at a fixed depth is the strongest "diagram, not photograph" tell in the picture,
  // because the coastline is its highest-contrast edge. Three properties, and the first is the
  // one a missing dither fails.

  // 1. THE BAND'S EDGE IS RAGGED. At 8 m down -- two metres outside the 6 m surf stop and inside
  //    the 4 m dither -- some texels read as surf and some do not. With no dither every one of
  //    them reads the same, and this assertion is what says so.
  const surf = OCEAN_BANDS[OCEAN_BANDS.length - 1][1];
  const isSurf = (c) => c[0] === surf[0] && c[1] === surf[1] && c[2] === surf[2];
  const trials = 400;
  let inBand = 0;
  for (let i = 0; i < trials; i += 1) {
    if (isSurf(slopeColor(-8, 0, 12.5, -30 + i * 0.37))) inBand += 1;
  }
  assert.ok(
    inBand > 0 && inBand < trials,
    `${inBand} of ${trials} texels at -8 m read as surf; the band's edge is a contour, not a dither`,
  );

  // 2. IT NEVER TOUCHES LAND. The same longitudes, one metre above the datum, must all be the
  //    land base colour -- if the dither were applied before the land/sea test, a coastal texel
  //    would flicker between sand and surf and the coastline itself would fray.
  for (let i = 0; i < trials; i += 1) {
    assert.deepEqual(
      slopeColor(1, 0, 12.5, -30 + i * 0.37), baseColor(1),
      "a land texel moved with longitude; the dither is on the wrong side of the datum",
    );
  }

  // 3. IT IS DETERMINISTIC AND BOUNDED. Two calls at one point agree -- a tile is rebuilt every
  //    time the cache evicts it, and a re-drawn tile that differed would shimmer -- and no call
  //    exceeds the stated amplitude, which is the number the "this is a coast dither and not an
  //    ocean texture" claim rests on.
  let worst = 0;
  for (let i = 0; i < 2000; i += 1) {
    const lat = -80 + i * 0.08;
    const lon = -170 + i * 0.17;
    const a = coastDitherM(lat, lon);
    assert.equal(a, coastDitherM(lat, lon), "the dither is not deterministic");
    worst = Math.max(worst, Math.abs(a));
  }
  assert.ok(worst <= FOAM_DITHER_M, `the dither reached ${worst} m, past its stated ${FOAM_DITHER_M} m`);
  assert.ok(
    worst > FOAM_DITHER_M * 0.9,
    `the dither only ever reached ${worst} m of its ${FOAM_DITHER_M} m; it is not using its range`,
  );
});

// ==============================================================================================
// The snow line, replaced. Task 5 of the climate slice.
// ==============================================================================================

/// The freezing contour the engine's own `climate::freezing_elevation_m` produces, at the
/// latitudes `climate_survey.rs` prints. **Transcribed nowhere**: `theEngineSnowLine` below
/// derives every one of these from a real climate tile read out of the committed `.wasm`, and
/// this table is only what the assertion is checked against.
const ENGINE_CONTOUR_M = [[0, 4153.8], [20, 3671.4], [45, 1810.7], [60, 153.8], [70, -1110.0]];

/// The engine's snow line at a latitude, read the way the viewer reads it: a 1x1 climate tile
/// for the datum temperature and the world's calibration for the lapse rate. Nothing here
/// knows 27, -25 or 6.5.
function theEngineSnowLine(latitudeDeg, lapseCPerKm) {
  const grid = engine.climateTileF32({
    handle: world,
    lat0Deg: latitudeDeg, lat1Deg: latitudeDeg, lon0Deg: 0, lon1Deg: 0,
    width: 1, height: 1,
    // A zero-step march: the moisture channel is not read here and the canonical 160-step
    // budget would be 161 elevation queries for a number this test throws away.
    marchSamples: 0,
  });
  return { datumC: grid[0], line: freezingLineM(grid[0], lapseCPerKm) };
}

test("the snow line is the engine's freezing contour, and it is a cosine where the old band was a line", async () => {
  const cal = engine.climateCalibration({ handle: world, marchSamples: 0 });
  assert.ok(Number.isFinite(cal.lapseCPerKm) && cal.lapseCPerKm > 0,
    `the engine must report a lapse rate, got ${cal.lapseCPerKm}`);

  // 1. THE LINE IS THE ENGINE'S, at five latitudes, read out of the shipped artifact.
  for (const [latitudeDeg, expectedM] of ENGINE_CONTOUR_M) {
    const { line } = theEngineSnowLine(latitudeDeg, cal.lapseCPerKm);
    assert.ok(Math.abs(line - expectedM) < 1,
      `at ${latitudeDeg} deg the engine snow line is ${line.toFixed(1)} m, not ${expectedM}`);
  }

  // 2. THE TWO DISAGREE BY THE MEASURED AMOUNTS, which is what makes this a replacement and
  //    not a re-spelling. 746 m at the equator, and the old line's zero crossing is 18.74
  //    degrees further north than the new one's.
  const equator = theEngineSnowLine(0, cal.lapseCPerKm).line;
  assert.ok(Math.abs((snowLineM(0) - equator) - 746) < 1,
    `the old band was ${(snowLineM(0) - equator).toFixed(1)} m too high at the equator, not 746`);

  let lo = 61;
  let hi = 62;
  for (let i = 0; i < 30; i += 1) {
    const mid = (lo + hi) / 2;
    if (theEngineSnowLine(mid, cal.lapseCPerKm).line > 0) lo = mid; else hi = mid;
  }
  assert.ok(Math.abs(lo - 61.26) < 0.02,
    `the engine line reaches the datum at ${lo.toFixed(2)} deg, not 61.26`);
  assert.ok(Math.abs((SNOW_LINE_ZERO_LAT_DEG - lo) - 18.74) < 0.02,
    `the old band crossed ${(SNOW_LINE_ZERO_LAT_DEG - lo).toFixed(2)} degrees late, not 18.74`);

  // 3. SHAPE, not offset. A straight line through the ENGINE line's own two ends sits far
  //    below it in the middle -- 708 m at 45 degrees. A snow line that had merely been
  //    lowered by 746 m everywhere would fail this and pass everything above it.
  const straightAt45 = equator * (1 - 45 / lo);
  const gap = theEngineSnowLine(45, cal.lapseCPerKm).line - straightAt45;
  assert.ok(gap > 700 && gap < 720,
    `the contour stands ${gap.toFixed(1)} m above a line through its own ends at 45 deg, not ~708`);
});

test("the engine line paints snow the old band missed, and the old band is still what ?climate=0 draws", () => {
  const cal = engineCalibration({
    radiusM: DEFAULT_WORLD.radiusM,
    climate: engine.climateCalibration({ handle: world, marchSamples: 0 }),
  });
  const equator = theEngineSnowLine(0, cal.lapseCPerKm);
  const mid = theEngineSnowLine(45, cal.lapseCPerKm);

  // The whiteness of a texel, isolated: `slopeColor` against `biomeColor` at the same point
  // with the same inputs. `biomeColor` is the land base this file's snow blend acts ON, so
  // the difference between the two IS the snow term and nothing else.
  const snowGain = (heightM, latitudeDeg, climate) => {
    const base = biomeColor({ heightM, latitudeDeg, longitudeDeg: 0, calibration: cal, climate });
    const drawn = slopeColor(heightM, 0, latitudeDeg, 0, cal, null, climate);
    return { base, drawn };
  };

  // 1. A 4,500 m EQUATORIAL SUMMIT. The old band puts its line at 4,900 m, so this peak is
  //    bare on the `?climate=0` path; the engine puts it at 4,154 m, so a summit 346 m past
  //    the blend width is fully snow. That one texel is half one of this task as a colour.
  assert.ok(snowLineM(0) > 4500, "the old band must leave a 4,500 m equatorial peak bare");
  assert.ok(equator.line + SNOW_BAND_M < 4500, "the engine line must bury a 4,500 m equatorial peak");
  assert.deepEqual(slopeColor(4500, 0, 0), baseColor(4500), "no climate: the old band, unchanged");
  assert.notDeepEqual(slopeColor(4500, 0, 0), [...SNOW_COLOR]);
  assert.deepEqual(
    slopeColor(4500, 0, 0, 0, cal, null, { datumC: equator.datumC, moisture: 0.5 }),
    [...SNOW_COLOR],
  );

  // 2. A 1,900 m PEAK AT 45 DEGREES goes the same way -- old line 2,143.75 m, engine line
  //    1,810.7 m. Two latitudes, because one of them would pass on a snow line that had
  //    simply been LOWERED by a constant, which is the thing §the-contour-test rules out.
  assert.ok(snowLineM(45) > 1900, "the old band must leave a 1,900 m peak at 45 deg bare");
  assert.ok(mid.line < 1900, "the engine line must put a 1,900 m peak at 45 deg into snow");
  const at45 = snowGain(1900, 45, { datumC: mid.datumC, moisture: 0.5 });
  assert.notDeepEqual(at45.drawn, at45.base, "the snow term did not fire at 45 deg / 1,900 m");
  assert.ok(
    at45.drawn[0] > at45.base[0] && at45.drawn[1] > at45.base[1] && at45.drawn[2] > at45.base[2],
    `the drawn texel ${at45.drawn} is not whiter than its biome base ${at45.base}`,
  );

  // 3. AND BELOW THE LINE NOTHING HAPPENS, so this is a line and not a global brightening.
  const under = snowGain(mid.line - 1, 45, { datumC: mid.datumC, moisture: 0.5 });
  assert.deepEqual(under.drawn, under.base, "a texel below the engine line must be untouched");

  // 4. THE FALLBACK IS UNTOUCHED. `?climate=0` passes no climate, and every snow assertion
  //    written before this task is written against that path.
  assert.equal(snowLineM(0), SNOW_LINE_EQUATOR_M);
  assert.deepEqual(slopeColor(1381, 0, 78), [...SNOW_COLOR]);
});

test("an engine climate with no lapse rate throws, rather than drawing a texel with silently no snow", () => {
  // `freezingLineM` on a NaN or missing lapse gives a NaN line; `smoothstep` clamps through a
  // comparison, every comparison against NaN is false, and the texel comes back with `snowT`
  // NaN and therefore no snow at all -- a plausible picture for an unanswerable input, which
  // is the failure family this project keeps finding. It throws instead.
  const cal = engineCalibration({
    radiusM: DEFAULT_WORLD.radiusM,
    climate: engine.climateCalibration({ handle: world, marchSamples: 0 }),
  });
  const climate = { datumC: 5, moisture: 1 };
  assert.throws(() => slopeColor(3000, 0, 0, 0, null, null, climate), /lapse rate/);
  assert.throws(() => slopeColor(3000, 0, 0, 0, { ...cal, lapseCPerKm: NaN }, null, climate), /lapse rate/);
  assert.throws(() => slopeColor(3000, 0, 0, 0, { ...cal, lapseCPerKm: 0 }, null, climate), /snow line is Infinity/);
  assert.throws(
    () => slopeColor(3000, 0, 0, 0, cal, null, { datumC: NaN, moisture: 1 }),
    /snow line is NaN/,
  );
});
