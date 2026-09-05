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
  marginedTileRequest,
  hasStructure,
  slopeColor,
  snowLineM,
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
