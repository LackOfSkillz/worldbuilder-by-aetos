// Node-native tests for biome.js and its effect on relief.js. `node:test` + `node:assert
// /strict`, no framework, no browser -- `biome.js` is a pure function of position and the
// real wasm, so it is testable exactly the way the Rust side is.
//
// **Population, method and host, named once so every number below can be traced:**
//
//   - Host: node v22.17.0, this repository's checked-in
//     `viewer/public/wasm/worldbuilder_engine.wasm`, loaded from disk (no fetch).
//   - `OWNER_WORLD`: seed 562423712, radius 4,500,000 m, 28 plates, land 0.16, with the
//     engine's `ranges` tectonic preset -- **the owner's world**, and the one the
//     screenshots in the report were taken on. Chosen over `DEFAULT_WORLD` because a colour
//     layer judged against the mountains slice's preset should be measured against it too.
//   - `SECOND_WORLD`: seed 7, radius 6,371,000 m, 12 plates, land 0.29, canonical
//     tectonics. Present for one reason: to separate "this colour is dead code" from "this
//     colour is world-dependent", which a single world cannot do.
//   - Coverage populations are Fibonacci-spiral samples, area-uniform, land only. The size
//     is stated at each call site because the smallest coverage figure that matters here is
//     about 0.05% of land and a sample that cannot resolve it would report a false zero.
//
// **What is deliberately NOT asserted here:** anything about the ocean's colour beyond
// "unchanged". Retuning the sea is its own task and this one must be able to prove it did
// not touch it.

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { Engine } from "../public/app/engine.js";
import {
  BIOMES,
  BELL_QUANTILES,
  BELL_Z,
  CALIBRATION_SAMPLES,
  JITTER_AMPLITUDE,
  MACRO_TONE,
  LANDFORM_QUANTILES,
  MAX_TARGET_LUM,
  TEMP_BAND_EDGES_C,
  VISIBLE_GAIN,
  bandIndex,
  biomeAt,
  calibrate,
  classify,
  deriveColor,
  fbm3,
  fibonacciPoint,
  hashUnit,
  luminance,
  moistureIndex,
  noiseFields,
  normalCdf,
  quantile,
  targetLuminance,
  temperatureC,
  unitVector,
} from "../public/app/biome.js";
import { marginedTileRequest, reliefTile, slopeColor } from "../public/app/relief.js";
import { biomeColourEnabled, reliefLayerEnabled } from "../public/app/relief-provider.js";

const OWNER_WORLD = { seed: 562423712, radiusM: 4500000, plateCount: 28, landFraction: 0.16 };
const SECOND_WORLD = { seed: 7, radiusM: 6371000, plateCount: 12, landFraction: 0.29 };

async function loadEngine() {
  const wasmPath = fileURLToPath(new URL("../public/wasm/worldbuilder_engine.wasm", import.meta.url));
  const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath), {});
  return new Engine(instance);
}

let engine;
let ownerHandle;
let ownerCal;
let secondHandle;
let secondCal;

test.before(async () => {
  engine = await loadEngine();
  ownerHandle = engine.newWorld({ ...OWNER_WORLD, tectonics: engine.tectonicPreset("ranges") });
  ownerCal = calibrate({ engine, worldHandle: ownerHandle, radiusM: OWNER_WORLD.radiusM });
  secondHandle = engine.newWorld(SECOND_WORLD);
  secondCal = calibrate({ engine, worldHandle: secondHandle, radiusM: SECOND_WORLD.radiusM });
});

/// Every land point of an `n`-point Fibonacci spiral on one world, classified.
function landScan(handle, cal, n) {
  const out = { land: 0, biome: new Array(BIOMES.length).fill(0), landform: [0, 0, 0], temp: [0, 0, 0, 0, 0], moist: [0, 0, 0, 0, 0], lum: [] };
  for (let i = 0; i < n; i += 1) {
    const { latitudeDeg, longitudeDeg } = fibonacciPoint(i, n);
    const heightM = engine.elevationM(handle, latitudeDeg, longitudeDeg);
    if (!(heightM > 0)) continue;
    out.land += 1;
    const r = biomeAt({ heightM, latitudeDeg, longitudeDeg, calibration: cal });
    out.biome[r.biome] += 1;
    out.landform[r.landform] += 1;
    out.temp[r.temp] += 1;
    out.moist[r.moist] += 1;
    out.lum.push(luminance(r.rgb));
  }
  return out;
}

function stats(values) {
  const a = values.slice().sort((x, y) => x - y);
  const mean = a.reduce((s, v) => s + v, 0) / a.length;
  const sd = Math.sqrt(a.reduce((s, v) => s + (v - mean) ** 2, 0) / a.length);
  const q = (p) => a[Math.min(a.length - 1, Math.max(0, Math.round(p * (a.length - 1))))];
  return { n: a.length, mean, sd, min: a[0], max: a[a.length - 1], p01: q(0.01), p99: q(0.99) };
}

// ============================================================================================
// The palette is DERIVED, and the derivation is the assertion
// ============================================================================================

test("every palette colour has the luminance its reflectance implies", () => {
  // This is the whole licence argument made checkable. If these colours had been transcribed
  // from someone else's hand-picked table they would land wherever that author's eye put
  // them; they land on `255 * VISIBLE_GAIN * reflectance` because that is what computed
  // them. A test that only checked "the colours are not all the same" would pass on a
  // vendored table, which is why this one checks the arithmetic instead.
  for (const b of BIOMES) {
    const target = targetLuminance(b.reflectance);
    assert.equal(b.targetLum, target, `${b.name}: target luminance drifted`);
    assert.ok(
      Math.abs(luminance(b.rgb) - target) <= 1,
      `${b.name}: rgb ${b.rgb} has luminance ${luminance(b.rgb).toFixed(2)}, derivation says ${target.toFixed(2)}`,
    );
  }
});

test("no palette entry is clipped, so no entry's luminance is an accident", () => {
  // A channel pinned at 255 would silently pull the entry off its derived luminance, and the
  // check above would then be asserting a coincidence. `ice` is the entry this is really
  // about: its reflectance of 0.85 would land at 433 without `MAX_TARGET_LUM`.
  for (const b of BIOMES) {
    for (const c of b.rgb) {
      assert.ok(c < 255, `${b.name}: channel clipped at 255 (${b.rgb})`);
      assert.ok(c >= 0, `${b.name}: negative channel (${b.rgb})`);
    }
  }
  const ice = BIOMES.find((b) => b.name === "ice");
  assert.equal(ice.targetLum, MAX_TARGET_LUM);
  assert.ok(255 * VISIBLE_GAIN * ice.reflectance > MAX_TARGET_LUM, "ice must be the clipped case");
});

test("the palette spans better than 10:1 inside land, which the height ramp cannot", () => {
  // **The measurement that defines the job.** The old `LAND_BANDS` ramp's six stops all sit
  // in the mid-tones; the claim being made here is that this palette does not.
  const lums = BIOMES.map((b) => luminance(b.rgb));
  const lo = Math.min(...lums);
  const hi = Math.max(...lums);
  assert.ok(lo < 20, `darkest land colour is ${lo.toFixed(1)}, expected under 20`);
  assert.ok(hi > 240, `brightest land colour is ${hi.toFixed(1)}, expected over 240`);
  assert.ok(hi / lo >= 10, `land palette spread is ${(hi / lo).toFixed(1)}:1, expected 10:1 or better`);

  // Neutralising the sibling: 10:1 is easy to get from two entries and a gap. Assert the
  // range is POPULATED too -- at least eight distinct 32-wide luminance buckets occupied --
  // or a palette of nineteen forests plus one snow would pass the line above.
  const buckets = new Set(lums.map((l) => Math.floor(l / 32)));
  assert.ok(buckets.size >= 7, `palette occupies only ${buckets.size} luminance octiles`);
});

test("deriveColor sets brightness from reflectance and hue from the ratio alone", () => {
  // The two arguments must not be able to trade against each other: doubling the ratio must
  // change nothing, and only the reflectance may move luminance.
  const a = deriveColor(0.2, [1.0, 0.9, 0.6]);
  const b = deriveColor(0.2, [0.5, 0.45, 0.3]);
  assert.deepEqual(a, b, "a scaled ratio must give the same colour");
  assert.ok(luminance(deriveColor(0.4, [1, 0.9, 0.6])) > luminance(a) * 1.9);
});

// ============================================================================================
// Calibration: quantiles over a Fibonacci sample, with no grid anywhere
// ============================================================================================

test("the bell quantiles are equally spaced z-scores, not evenly spaced fractions", () => {
  // The derivation, restated as arithmetic so a later hand-edit to the numbers is a failure
  // rather than a silent change of the world's climate. Evenly spaced fractions are the
  // thing WorldEngine's own comment says produced worse results.
  assert.deepEqual(BELL_Z, [-1.5, -0.5, 0.5, 1.5]);
  BELL_QUANTILES.forEach((q, i) => {
    assert.ok(Math.abs(q - normalCdf(BELL_Z[i])) < 1e-12);
  });
  const widths = [];
  let prev = 0;
  for (const q of BELL_QUANTILES) { widths.push(q - prev); prev = q; }
  widths.push(1 - prev);
  // Middle band strictly widest, tails strictly narrowest: that IS the bell.
  assert.ok(widths[2] > widths[1] && widths[1] > widths[0], `widths ${widths}`);
  assert.ok(Math.abs(widths[0] - 0.0668) < 0.001 && Math.abs(widths[2] - 0.383) < 0.001);
  // And the sibling this neutralises: evenly spaced fractions would give 0.2 everywhere.
  assert.ok(Math.abs(widths[0] - 0.2) > 0.1, "these are not evenly spaced bands");
});

test("calibrate reads the world through wb_elevation_m only -- no grid, no tile fill", () => {
  // The grid-free claim, made falsifiable. `fillTileF32` is the grid call, and a calibration
  // that reached for it would be doing exactly the global-array reduction this project
  // rejected. Counting the calls is the only way to see that from outside.
  let elevationCalls = 0;
  let gridCalls = 0;
  const spy = {
    elevationM: (...args) => { elevationCalls += 1; return engine.elevationM(...args); },
    fillTileF32: (...args) => { gridCalls += 1; return engine.fillTileF32(...args); },
  };
  const cal = calibrate({ engine: spy, worldHandle: ownerHandle, radiusM: OWNER_WORLD.radiusM });
  assert.equal(gridCalls, 0, "calibrate must not fill a grid");
  assert.equal(elevationCalls, CALIBRATION_SAMPLES);
  assert.deepEqual(cal.moistEdges, ownerCal.moistEdges, "and it must be deterministic");
});

test("the calibration sample recovers the world's own land fraction", () => {
  // The sample is area-uniform, so its land share estimates the world's. This is what makes
  // the quantiles below quantiles OF LAND rather than of an arbitrary subset -- and it is a
  // check on the spiral itself, which would still return plausible-looking edges if it were
  // clustered at a pole.
  const share = ownerCal.landSamples / ownerCal.samples;
  assert.ok(
    Math.abs(share - OWNER_WORLD.landFraction) < 0.02,
    `sampled land share ${share.toFixed(3)} vs requested ${OWNER_WORLD.landFraction}`,
  );
  const share2 = secondCal.landSamples / secondCal.samples;
  assert.ok(Math.abs(share2 - SECOND_WORLD.landFraction) < 0.02, `second world: ${share2.toFixed(3)}`);
});

test("the calibrated edges differ between worlds, and the absolute ones do not", () => {
  // The point of calibrating: a per-world constant that is not a constant. And its
  // counterpart: the temperature axis has a unit, so it must NOT move with the world.
  assert.notDeepEqual(ownerCal.moistEdges, secondCal.moistEdges);
  assert.notDeepEqual(ownerCal.landformEdges, secondCal.landformEdges);
  assert.deepEqual(ownerCal.tempEdges, TEMP_BAND_EDGES_C);
  assert.deepEqual(secondCal.tempEdges, TEMP_BAND_EDGES_C);
});

test("quantile and bandIndex agree about which side of an edge a value falls", () => {
  const sorted = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
  assert.equal(quantile(sorted, 0), 0);
  assert.equal(quantile(sorted, 1), 10);
  assert.equal(quantile(sorted, 0.5), 5);
  assert.equal(bandIndex([2, 5], 1), 0);
  assert.equal(bandIndex([2, 5], 2), 1); // on the edge belongs to the upper band
  assert.equal(bandIndex([2, 5], 9), 2);
  assert.equal(bandIndex([], 9), 0);
});

// ============================================================================================
// The bands are reached, and the biomes are reached
// ============================================================================================

test("every quantiled band is occupied on the owner's world, at the width it was given", () => {
  // 200,000 spiral points, ~32,400 of them land. The moisture bands were placed at 6.7 /
  // 24.2 / 38.3 / 24.2 / 6.7 percent of land and this asserts they landed there, which is
  // simultaneously a check on `calibrate`, on `bandIndex`, and on the classifier reading the
  // same edges the calibration wrote.
  const scan = landScan(ownerHandle, ownerCal, 200_000);
  assert.ok(scan.land > 30_000, `only ${scan.land} land samples`);
  const expected = [];
  let prev = 0;
  for (const q of BELL_QUANTILES) { expected.push(q - prev); prev = q; }
  expected.push(1 - prev);
  scan.moist.forEach((c, i) => {
    const share = c / scan.land;
    assert.ok(share > 0, `moisture band ${i} is empty`);
    assert.ok(
      Math.abs(share - expected[i]) < 0.02,
      `moisture band ${i}: ${(share * 100).toFixed(1)}% vs the ${(expected[i] * 100).toFixed(1)}% it was given`,
    );
  });
  // Landform against the quantiles it was given, read from the constant rather than
  // restated: 25% coastal and the top 15% montane.
  const landformShare = scan.landform.map((c) => c / scan.land);
  assert.ok(Math.abs(landformShare[0] - LANDFORM_QUANTILES[0]) < 0.04,
    `coastal ${landformShare[0].toFixed(3)} vs the ${LANDFORM_QUANTILES[0]} quantile it was cut at`);
  assert.ok(Math.abs(landformShare[2] - (1 - LANDFORM_QUANTILES[1])) < 0.04,
    `montane ${landformShare[2].toFixed(3)} vs the ${(1 - LANDFORM_QUANTILES[1]).toFixed(2)} it was cut at`);
  // Every temperature band too -- absolute, so this is a fact about the world rather than
  // about the calibration, and the world's span is what makes it true.
  scan.temp.forEach((c, i) => assert.ok(c > 0, `temperature band ${i} is empty on the owner's world`));
  assert.ok(ownerCal.tempSpanC[0] < 0 && ownerCal.tempSpanC[1] > 24,
    `land temperature span ${ownerCal.tempSpanC} cannot reach every absolute band`);
});

test("no palette entry is dead code: each is reached on the owner's world or on another", () => {
  // **The check that would have caught three shipped defects.** `relief.js` shipped a rock
  // band at 22 degrees against a 1.9-degree planet, a snow line at 3,500 m against a
  // 1,979 m one, and two height stops above the 99.9th percentile of land -- three colours
  // that were unreachable and looked like features. Thirty-three colours is eleven times the
  // opportunity to do it again.
  //
  // The sibling this neutralises: "reached on SOME world" alone would let a colour hide
  // behind an exotic seed, so the owner's world is counted separately and named.
  const owner = landScan(ownerHandle, ownerCal, 200_000);
  const second = landScan(secondHandle, secondCal, 200_000);
  const dead = BIOMES.filter((b) => owner.biome[b.id] === 0 && second.biome[b.id] === 0);
  assert.deepEqual(dead.map((b) => b.name), [], "these colours can never be drawn");

  // And the owner's world specifically: at most three of thirty-three may be world-dependent
  // there, and they must be the three the report names. Cold-and-superhumid is anti-
  // correlated by the circulation model, which is why these three and not others.
  const missing = BIOMES.filter((b) => owner.biome[b.id] === 0).map((b) => b.name);
  assert.deepEqual(missing, ["wet tundra", "boreal wet forest", "temperate rain forest"]);
});

test("classify covers its whole domain and never answers with a hole", () => {
  // Three axes x every band = 75 combinations. A missing row would be `undefined`, which
  // would index `BIOMES` to `undefined` and throw only on the world that reached it.
  for (let landform = 0; landform < 3; landform += 1) {
    for (let temp = 0; temp < 5; temp += 1) {
      for (let moist = 0; moist < 5; moist += 1) {
        const id = classify(landform, temp, moist);
        assert.ok(Number.isInteger(id) && BIOMES[id], `classify(${landform},${temp},${moist}) = ${id}`);
      }
    }
  }
});

test("the landform axis is a real axis: a shore is a different place at each temperature", () => {
  // The reason this project keeps three axes where WorldEngine has two. If coastal collapsed
  // to one answer, "tropical coastal" and "boreal coastal" would be the same place, which is
  // the thing a MUD cannot use.
  const coastalDry = [0, 1, 2, 3, 4].map((t) => classify(0, t, 0));
  assert.equal(new Set(coastalDry).size, 5, "five temperatures must give five shores");
  const coastalWet = [0, 1, 2, 3, 4].map((t) => classify(0, t, 4));
  assert.notDeepEqual(coastalDry, coastalWet, "moisture must move a shore too");
  // And the landform axis must not be a relabelling of temperature: at one temperature and
  // one moisture the three landforms must differ.
  const acrossLandform = [0, 1, 2].map((l) => classify(l, 3, 1));
  assert.equal(new Set(acrossLandform).size, 3);
});

// ============================================================================================
// Noise: a point hash, order-independent, and it actually frays the boundaries
// ============================================================================================

test("the noise is a point hash: same position, same answer, whoever asks", () => {
  // WorldEngine's per-pixel noise comes from a sequential RNG, so a texel's colour depends
  // on the order tiles were drawn. This asserts ours does not -- the property that lets two
  // adjacent tiles agree on their shared edge without sharing any state.
  const at = (lat, lon) => biomeAt({
    heightM: 400, latitudeDeg: lat, longitudeDeg: lon, calibration: ownerCal,
  }).rgb;
  for (const [lat, lon] of [[12.5, 47.25], [-63.125, -179.9], [0, 180], [89.9, 0]]) {
    assert.deepEqual(at(lat, lon), at(lat, lon));
    // Interleave a thousand other evaluations: a stateful generator would drift.
    for (let i = 0; i < 1000; i += 1) at(i * 0.031, i * 0.117);
    assert.deepEqual(at(lat, lon), at(lat, lon), "an interleaved call changed the answer");
  }
  assert.notDeepEqual(at(12.5, 47.25), at(12.5, 47.35), "and it must not be constant");
});

test("hashUnit avalanches: neighbouring lattice cells are uncorrelated", () => {
  // A hash that returned a smooth function of its inputs would give noise that is not noise.
  // 4,096 adjacent pairs, mean absolute difference of a uniform pair is 1/3.
  let sum = 0;
  let n = 0;
  for (let i = 0; i < 64; i += 1) {
    for (let j = 0; j < 64; j += 1) {
      sum += Math.abs(hashUnit(i, j, 0, 1) - hashUnit(i + 1, j, 0, 1));
      n += 1;
    }
  }
  assert.ok(Math.abs(sum / n - 1 / 3) < 0.03, `mean adjacent |difference| ${(sum / n).toFixed(4)}`);
});

test("fbm3 is standardised, which is the unit every noise weight is written in", () => {
  // `FBM_SD` divides out a measured factor of five. If it were wrong, every weight in the
  // file would be wrong by the same factor -- which is exactly what happened on the first
  // pass, and it cost nine of thirty-three colours.
  const samples = [];
  for (let i = 0; i < 20_000; i += 1) {
    const { latitudeDeg, longitudeDeg } = fibonacciPoint(i, 20_000);
    const [x, y, z] = unitVector(latitudeDeg, longitudeDeg);
    samples.push(fbm3(x * 12, y * 12, z * 12, 5, 0x1234));
  }
  const s = stats(samples);
  assert.ok(Math.abs(s.sd - 1) < 0.2, `fbm3 sd is ${s.sd.toFixed(3)}, expected ~1`);
  assert.ok(Math.abs(s.mean) < 0.2, `fbm3 mean is ${s.mean.toFixed(3)}`);
});

test("breakup noise frays the biome boundaries instead of drawing contour lines", () => {
  // **The finding that matters as much as the palette.** A threshold evaluated on a bare
  // input draws one clean curve; the same threshold with noise added to its input is crossed
  // many times as the two interdigitate. This walks a transect along a parallel and counts
  // how often the classification changes.
  //
  // **It counts the climate pair (temperature, moisture) only, and deliberately leaves the
  // landform axis out of the count.** Landform is a threshold on real elevation, so it is
  // crossed wherever the ground genuinely rises past a band edge, with or without noise --
  // including it puts a large constant in both columns and makes the ratio unreadable. The
  // two climate axes are functions of latitude and height alone, so on a parallel their
  // noise-free classification is very nearly constant, and *every* crossing the noisy one
  // adds is the fray.
  //
  // The noise-free comparison uses the same code path with the three fields zeroed, rather
  // than a second implementation that could differ for other reasons.
  const N = 4000;
  const lat = 24.5;
  const flat = { macro: 0, patch: 0, breakup: 0 };
  let withNoise = 0;
  let withoutNoise = 0;
  let landformFray = 0;
  let landformBare = 0;
  let prevA = null;
  let prevB = null;
  let prevLa = null;
  let prevLb = null;
  let n = 0;
  for (let i = 0; i < N; i += 1) {
    const lon = -180 + (360 * i) / N;
    const heightM = engine.elevationM(ownerHandle, lat, lon);
    if (!(heightM > 0)) { prevA = null; prevB = null; prevLa = null; prevLb = null; continue; }
    n += 1;
    const r = biomeAt({ heightM, latitudeDeg: lat, longitudeDeg: lon, calibration: ownerCal });
    const a = r.temp * 5 + r.moist;
    const b = bandIndex(ownerCal.tempEdges, temperatureC(lat, heightM, flat)) * 5
      + bandIndex(ownerCal.moistEdges, moistureIndex(lat, flat));
    const la = r.landform;
    const lb = bandIndex(ownerCal.landformEdges, heightM);
    if (prevA !== null && a !== prevA) withNoise += 1;
    if (prevB !== null && b !== prevB) withoutNoise += 1;
    if (prevLa !== null && la !== prevLa) landformFray += 1;
    if (prevLb !== null && lb !== prevLb) landformBare += 1;
    prevA = a; prevB = b; prevLa = la; prevLb = lb;
  }
  assert.ok(n > 500, `only ${n} land samples on the transect`);
  assert.ok(
    withNoise > withoutNoise * 5 + 5,
    `climate-band crossings along 24.5 N: ${withNoise} with noise vs ${withoutNoise} without ` +
    `-- the fray is not happening`,
  );
  // And the landform contour, which is the case the reference literature actually names: a
  // bare elevation threshold draws a contour line, and adding noise to its input must break
  // it into more pieces than the terrain alone does.
  assert.ok(
    landformFray > landformBare,
    `landform crossings: ${landformFray} frayed vs ${landformBare} bare -- the contour is intact`,
  );
});

test("colour jitter is present, bounded, and independent per channel", () => {
  // Bounded: a jitter that could exceed its amplitude would push a dark forest negative and
  // clip, which is a colour shift rather than a texture. Independent: one scalar on all three
  // channels moves luminance only and reads as a photocopy.
  const cal = ownerCal;
  const base = BIOMES.map((b) => b.rgb);
  let maxDelta = 0;
  let sameOnAllChannels = 0;
  let n = 0;
  for (let i = 0; i < 4000; i += 1) {
    const { latitudeDeg, longitudeDeg } = fibonacciPoint(i, 4000);
    const heightM = engine.elevationM(ownerHandle, latitudeDeg, longitudeDeg);
    if (!(heightM > 0)) continue;
    const r = biomeAt({ heightM, latitudeDeg, longitudeDeg, calibration: cal });
    const b = base[r.biome];
    // The macro tone multiply is the other term on this colour, so it is divided back out
    // here rather than absorbed into a looser bound: `1 + macro * MACRO_TONE` on a base of
    // 250 can move a channel by 50 on its own, which would hide a jitter three times its
    // stated amplitude inside a bound wide enough to accommodate it.
    const tone = 1 + r.fields.macro * MACRO_TONE;
    const d = [0, 1, 2].map((c) => r.rgb[c] - b[c] * tone);
    maxDelta = Math.max(maxDelta, ...d.map(Math.abs));
    if (Math.abs(d[0] - d[1]) < 1e-9 && Math.abs(d[1] - d[2]) < 1e-9) sameOnAllChannels += 1;
    n += 1;
  }
  assert.ok(n > 500, `only ${n} land samples`);
  assert.ok(maxDelta > JITTER_AMPLITUDE * 0.5, `jitter never exceeded ${maxDelta.toFixed(1)}`);
  assert.ok(maxDelta <= JITTER_AMPLITUDE + 1e-9, `jitter reached ${maxDelta.toFixed(2)}, past its stated ${JITTER_AMPLITUDE}`);
  assert.ok(sameOnAllChannels / n < 0.01, "the three channels are moving together");
});

test("the three noise fields are independent, not three views of one", () => {
  // Same salt on all three would give a field that only ever moves temperature and moisture
  // together -- which would collapse the classifier's table onto its diagonal, and is the
  // failure this file already had once for a different reason.
  const a = [];
  const b = [];
  const c = [];
  for (let i = 0; i < 5000; i += 1) {
    const { latitudeDeg, longitudeDeg } = fibonacciPoint(i, 5000);
    const [x, y, z] = unitVector(latitudeDeg, longitudeDeg);
    const f = noiseFields(x, y, z, OWNER_WORLD.radiusM);
    a.push(f.macro); b.push(f.patch); c.push(f.breakup);
  }
  const corr = (p, q) => {
    const mp = p.reduce((s, v) => s + v, 0) / p.length;
    const mq = q.reduce((s, v) => s + v, 0) / q.length;
    let num = 0; let dp = 0; let dq = 0;
    for (let i = 0; i < p.length; i += 1) {
      num += (p[i] - mp) * (q[i] - mq); dp += (p[i] - mp) ** 2; dq += (q[i] - mq) ** 2;
    }
    return num / Math.sqrt(dp * dq);
  };
  assert.ok(Math.abs(corr(a, b)) < 0.15, `macro/patch correlation ${corr(a, b).toFixed(3)}`);
  assert.ok(Math.abs(corr(a, c)) < 0.15, `macro/breakup correlation ${corr(a, c).toFixed(3)}`);
  assert.ok(Math.abs(corr(b, c)) < 0.15, `patch/breakup correlation ${corr(b, c).toFixed(3)}`);
});

// ============================================================================================
// What it does to the rendered tile
// ============================================================================================

/// The per-texel heights of a tile, derived exactly the way `reliefTile` derives them, so
/// "this texel is land" means the same thing in the test as in the raster.
function tileHeights(handle, rectangle, size, radiusM) {
  const request = marginedTileRequest({ rectangle, size, worldHandle: handle, radiusM });
  const grid = request.grid;
  const heights = engine.fillTileF32(request);
  const out = new Float32Array(size * size);
  for (let r = 0; r < size; r += 1) {
    for (let c = 0; c < size; c += 1) out[r * size + c] = heights[(r + 1) * grid + (c + 1)];
  }
  return out;
}

const SCAN_TILES = [];
test("a rendered land tile gains an order of magnitude of luminance spread", () => {
  // **The before/after.** Population: every 30-degree tile of a 12 x 6 global scan whose four
  // corners and centre are at least three-fifths land, rasterised at 64 texels a side on the
  // owner's world with the `ranges` preset -- then land texels only, because the ocean is
  // unchanged by construction and averaging it in would dilute the very quantity being
  // measured.
  for (let ty = 0; ty < 6; ty += 1) {
    for (let tx = 0; tx < 12; tx += 1) {
      const northDeg = 90 - ty * 30;
      const southDeg = northDeg - 30;
      const westDeg = -180 + tx * 30;
      const eastDeg = westDeg + 30;
      let landish = 0;
      for (const [la, lo] of [[northDeg, westDeg], [northDeg, eastDeg], [southDeg, westDeg],
        [southDeg, eastDeg], [(northDeg + southDeg) / 2, (westDeg + eastDeg) / 2]]) {
        if (engine.elevationM(ownerHandle, la, lo) > 0) landish += 1;
      }
      if (landish >= 3) SCAN_TILES.push({ northDeg, southDeg, westDeg, eastDeg });
    }
  }
  assert.ok(SCAN_TILES.length >= 4, `only ${SCAN_TILES.length} land tiles in the scan`);

  const size = 64;
  const collect = (biome) => {
    const lums = [];
    for (const rectangle of SCAN_TILES) {
      const heights = tileHeights(ownerHandle, rectangle, size, OWNER_WORLD.radiusM);
      const img = reliefTile({
        rectangle, size, engine, worldHandle: ownerHandle, radiusM: OWNER_WORLD.radiusM, biome,
      });
      for (let i = 0; i < heights.length; i += 1) {
        if (!(heights[i] > 0)) continue;
        const o = i * 4;
        lums.push(luminance([img.data[o], img.data[o + 1], img.data[o + 2]]));
      }
    }
    return stats(lums);
  };
  const before = collect(null);
  const after = collect(ownerCal);
  assert.equal(before.n, after.n, "the same land texels must be measured on both sides");
  assert.ok(before.n > 5000, `only ${before.n} land texels`);
  assert.ok(after.sd > before.sd * 3,
    `land luminance sd ${before.sd.toFixed(1)} -> ${after.sd.toFixed(1)}, expected at least 3x`);
  assert.ok(after.p99 / after.p01 > 8,
    `land p99/p01 is ${(after.p99 / after.p01).toFixed(1)}, expected better than 8:1`);
  assert.ok(before.p99 / before.p01 < 3,
    `the ramp's own p99/p01 is ${(before.p99 / before.p01).toFixed(1)} -- this baseline is not the flat one`);
});

test("the ocean is byte-identical, because retuning it is a different task", () => {
  // The sibling that has to be neutralised for the test above to mean anything: "the picture
  // changed" is satisfied by changing anything at all. This says the change is confined to
  // land, texel for texel, which is also the claim that this task did not quietly start the
  // ocean task.
  // Found, not guessed: the first 30-degree tile of the global scan whose texels are at
  // least a fifth land and a fifth sea. A hand-picked rectangle is how this test first
  // asserted the ocean was unchanged across 4,096 sea texels and no land at all.
  const size = 64;
  let rectangle = null;
  let heights = null;
  for (let ty = 0; ty < 6 && !rectangle; ty += 1) {
    for (let tx = 0; tx < 12 && !rectangle; tx += 1) {
      const northDeg = 90 - ty * 30;
      const candidate = { northDeg, southDeg: northDeg - 30, westDeg: -180 + tx * 30, eastDeg: -180 + tx * 30 + 30 };
      const h = tileHeights(ownerHandle, candidate, size, OWNER_WORLD.radiusM);
      let landCount = 0;
      for (let i = 0; i < h.length; i += 1) if (h[i] > 0) landCount += 1;
      if (landCount > h.length * 0.2 && landCount < h.length * 0.8) { rectangle = candidate; heights = h; }
    }
  }
  assert.ok(rectangle, "no mixed land/sea tile in the scan");
  const args = { rectangle, size, engine, worldHandle: ownerHandle, radiusM: OWNER_WORLD.radiusM };
  const before = reliefTile({ ...args, biome: null });
  const after = reliefTile({ ...args, biome: ownerCal });
  let sea = 0;
  let landChanged = 0;
  let land = 0;
  for (let i = 0; i < heights.length; i += 1) {
    const o = i * 4;
    const same = before.data[o] === after.data[o] && before.data[o + 1] === after.data[o + 1]
      && before.data[o + 2] === after.data[o + 2] && before.data[o + 3] === after.data[o + 3];
    if (heights[i] > 0) {
      land += 1;
      if (!same) landChanged += 1;
    } else {
      sea += 1;
      assert.ok(same, `sea texel ${i} changed: ${[...before.data.slice(o, o + 4)]} -> ${[...after.data.slice(o, o + 4)]}`);
    }
  }
  assert.ok(sea > 100 && land > 100, `tile is not mixed enough: ${land} land, ${sea} sea`);
  assert.ok(landChanged / land > 0.99, `only ${((landChanged / land) * 100).toFixed(1)}% of land texels moved`);
});

test("with no calibration relief.js is exactly the layer it was before", () => {
  // The escape hatch has to be a real one: the height ramp is the baseline every earlier
  // measurement in this repository was taken against, and a "default off" that quietly
  // differed would invalidate all of them.
  for (const [h, slope, lat] of [[0, 0, 0], [500, 3, 45], [1400, 20, -12], [-300, 0, 60]]) {
    assert.deepEqual(
      slopeColor(h, slope, lat, 100, null),
      slopeColor(h, slope, lat),
      `slopeColor(${h},${slope},${lat}) moved when longitude was supplied`,
    );
  }
});

test("longitude reaches the colour, which is what makes the third dimension real", () => {
  // `reliefTile` had no longitude in its colour path at all before this task. If the wiring
  // were wrong -- a constant, or the tile's west edge for every column -- the tile would
  // still look plausible, and every check above would still pass because they call `biomeAt`
  // directly. This is the one that sees the raster's own plumbing.
  const a = slopeColor(600, 2, 10, 20, ownerCal);
  const b = slopeColor(600, 2, 10, 21, ownerCal);
  assert.notDeepEqual(a, b, "moving a degree east changed nothing");

  // And in the raster, where the wiring actually lives. **Two tiles cut differently must
  // agree on the ground they share.** Tile A spans 0..20 E and tile B spans 10..30 E, both
  // 41 posts over 20 degrees, so A's columns 20..40 and B's columns 0..20 are the same
  // meridians; every one of those texels must come back byte-identical.
  //
  // This is the assertion that sees `reliefTile`'s own plumbing. The direct `slopeColor`
  // calls above pass longitude by hand and would still pass if the raster fed a constant --
  // and it did: wiring every column to the tile's west edge survived the first version of
  // this test intact. It cannot survive this one, because A and B have different west edges.
  // It is also a seam check in its own right: a colour that depends on which tile asked is
  // a visible grid line, which is the defect this file's margin logic exists to prevent.
  //
  // The rectangles are **all land**, found by a 10-degree scan of the owner's world rather
  // than picked: an ocean texel takes the height-only path by design, so a pair of sea tiles
  // would agree whatever longitude they were handed and the check would prove nothing. The
  // first version of this test used 0..30 E on the equator, which is open ocean here, and it
  // survived the mutation intact.
  const size = 41;
  const args = { size, engine, worldHandle: ownerHandle, radiusM: OWNER_WORLD.radiusM, biome: ownerCal };
  const north = 40;
  const south = 20;
  const tileA = reliefTile({ ...args, rectangle: { northDeg: north, southDeg: south, westDeg: 90, eastDeg: 110 } });
  const tileB = reliefTile({ ...args, rectangle: { northDeg: north, southDeg: south, westDeg: 100, eastDeg: 120 } });
  const sharedHeights = tileHeights(
    ownerHandle, { northDeg: north, southDeg: south, westDeg: 100, eastDeg: 120 }, size,
    OWNER_WORLD.radiusM,
  );
  let sharedLand = 0;
  for (let row = 0; row < size; row += 1) {
    for (let k = 0; k <= 20; k += 1) if (sharedHeights[row * size + k] > 0) sharedLand += 1;
  }
  assert.ok(sharedLand > size * 21 * 0.5,
    `only ${sharedLand} of ${size * 21} shared texels are land -- this pair proves nothing`);
  let compared = 0;
  for (let row = 0; row < size; row += 1) {
    for (let k = 0; k <= 20; k += 1) {
      const oa = (row * size + (20 + k)) * 4;
      const ob = (row * size + k) * 4;
      for (let c = 0; c < 4; c += 1) {
        assert.equal(tileA.data[oa + c], tileB.data[ob + c],
          `row ${row}, shared column ${k}: the two tiles disagree about the same meridian`);
      }
      compared += 1;
    }
  }
  assert.equal(compared, size * 21);
  // Neutralising the sibling: the two tiles must not be identical everywhere, or the check
  // above would pass on a raster that ignored longitude entirely.
  assert.notDeepEqual([...tileA.data], [...tileB.data]);
});

test("temperature falls with height, so a mountain is a colder place than its valley", () => {
  // The lapse rate is the only way elevation enters the climate axes, and a sign error there
  // would put jungle on the summits. Also the profile's own fit, restated: 0 / 30 / 45 / 60
  // / 90 degrees against Earth's zonal means.
  const flat = { macro: 0, patch: 0, breakup: 0 };
  assert.ok(temperatureC(0, 2000, flat) < temperatureC(0, 0, flat) - 12);
  const profile = [0, 30, 45, 60, 90].map((lat) => temperatureC(lat, 0, flat));
  const earth = [26, 20, 12, 0, -25];
  profile.forEach((t, i) => assert.ok(Math.abs(t - earth[i]) < 3,
    `${[0, 30, 45, 60, 90][i]} deg: profile says ${t.toFixed(1)} C, Earth's zonal mean is ${earth[i]}`));
});

test("moisture has the four circulation cells, in the right order", () => {
  // Wet equator, dry subtropics, wet polar front, dry pole -- and it has to be that shape or
  // there is no reason for a desert to be where a desert is.
  const flat = { macro: 0, patch: 0, breakup: 0 };
  const m = (lat) => moistureIndex(lat, flat);
  assert.ok(m(0) > m(30), "the ITCZ must be wetter than the subtropics");
  assert.ok(m(55) > m(30), "the polar front must be wetter than the subtropics");
  assert.ok(m(55) > m(85), "the pole must be drier than the polar front");
  assert.ok(m(0) > m(55), "and the ITCZ must be the wettest of all");
  assert.equal(m(30), m(-30), "the profile is symmetric about the equator");
});

test("?biome=0 is the A/B switch, and it is off by default", () => {
  // The before/after pair in the report is one build and one flag apart, so the flag has to
  // mean what the report says it means. Asserted rather than eyeballed for the reason
  // `reliefLayerEnabled` is: `panel-fields.js` opens with four defects of exactly this shape,
  // one of them a default nobody had read.
  assert.equal(biomeColourEnabled(new URLSearchParams("")), true);
  assert.equal(biomeColourEnabled(new URLSearchParams("biome=1")), true);
  assert.equal(biomeColourEnabled(new URLSearchParams("biome=0")), false);
  // And it must be its own switch, not a second name for the layer's.
  assert.equal(reliefLayerEnabled(new URLSearchParams("biome=0")), true);
  assert.equal(biomeColourEnabled(new URLSearchParams("relief=0")), true);
});
