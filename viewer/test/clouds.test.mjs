// Node-native tests for clouds.js -- the procedural weather layer.
//
// # The check that must be able to fail
//
// **A cloud layer that returns transparent everywhere looks like "light cloud cover" and passes
// every visual inspection**, exactly as a flat-grey relief raster looked like "subtle shading"
// one slice ago. So the assertions below are about three quantities that a degenerate layer
// cannot have all of at once:
//
//   1. **coverage inside a stated band**, measured back out of the field rather than assumed
//      from the parameter that produced it;
//   2. **structure** -- and specifically structure in the ALPHA channel, because a cloud
//      raster's RGB is healthy even when nothing is visible;
//   3. **latitude bands** -- the ITCZ and both storm tracks measurably cloudier than the
//      subtropics, which is what separates weather from uniform noise.
//
// (3) is the one that matters most, and this file **demonstrates its own falsifiability**: one
// test flattens `CLOUD_ZONES` in place -- the plausible mutant, which preserves the index's
// distribution and the global coverage almost exactly and destroys only the meaning -- and
// asserts that the coverage check still passes while the banding check goes red. A check that
// survived that mutation would not be checking for bands.
//
// # Population / method / host, named once
//
//   - **World:** the owner's -- seed 562423712, radius 4,500,000 m. Plate count and land
//     fraction are not inputs here: **nothing in `clouds.js` reads the ground**, which is itself
//     asserted below.
//   - **Second world:** the default, seed 20260904 at radius 6,371,000 m, used for the
//     per-world and per-radius checks.
//   - **Method:** the field is evaluated on equal-area Fibonacci spirals of the stated size, or
//     rasterised into 128-texel tiles over stated rectangles and read back through `alphaStats`
//     with cos(latitude) weighting. Coverage always means "at or above `COVERAGE_ALPHA` = 128,
//     i.e. half opacity", stated once here and imported everywhere rather than restated.
//   - **Host:** node v22.x. **No wasm and no engine** -- see the test that proves that is a
//     property of the module rather than of this file.

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import {
  alphaStats, bandCoverage, calibrateClouds, CLOUD_ALPHA_MAX, CLOUD_EDGE_W, CLOUD_FINE_OCTAVES,
  CLOUD_FINE_WAVELENGTH_M, CLOUD_MAX_ALPHA_BYTE, CLOUD_MAX_LEVEL, CLOUD_NOISE_SD, CLOUD_STRETCH,
  CLOUD_TILE_SIZE, CLOUD_ZONES, COVERAGE_ALPHA, COVERAGE_INDEX_OFFSET, cloudAlpha, cloudDensity,
  cloudIndex, cloudSalts, cloudTile, DEFAULT_CLOUD_COVER, hasCloudStructure, measuredCoverage,
  meanZonalCloudiness, smoothstep01, smoothstepInverse, zonalCloudiness, zonalRange,
} from "../public/app/clouds.js";
import { FBM_LACUNARITY, fibonacciPoint } from "../public/app/biome.js";
import { PANEL_RANGES } from "../public/app/panel-fields.js";

/// The owner's world. Radius is the only field the cloud field reads.
const OWNER = { seed: "562423712", radiusM: 4_500_000 };
const DEFAULT = { seed: "20260904", radiusM: 6_371_000 };

/// The spiral size every figure in this file is measured over unless it says otherwise.
/// 60,000 equal-area points is an order-statistic standard error of 0.20 percentage points at
/// 40% coverage, which is five times finer than the tightest tolerance asserted below.
const N = 60000;

const OWNER_CAL = calibrateClouds({ ...OWNER, cover: DEFAULT_CLOUD_COVER });

/// A tile straddling the ITCZ on the owner's world, and one in the northern subtropics. Stated
/// as level-3 geographic rectangles (22.5 deg square) rather than built from Cesium, because
/// `clouds.js` has no tiling scheme and no Cesium.
const ITCZ_TILE = { northDeg: 11.25, southDeg: -11.25, westDeg: 157.5, eastDeg: 180 };
const SUBTROPICAL_TILE = { northDeg: 33.75, southDeg: 11.25, westDeg: 157.5, eastDeg: 180 };

const appFile = (name) =>
  readFileSync(fileURLToPath(new URL(`../public/app/${name}`, import.meta.url)), "utf8");

// ==========================================================================================
// The field: measured, not nominal
// ==========================================================================================

test("the index's spread is MEASURED, and the noise half matches its derivation", () => {
  // This is the `FBM_SD` lesson restated for this field. `biome.js` records that a trilinear
  // value-noise fBm normalised by its own amplitudes has sd 0.105 against a nominal +-0.5, so
  // every weight written against the nominal range was five times too weak and nine of
  // thirty-three colours were unreachable. `CLOUD_NOISE_SD` is a DERIVATION from the three
  // weights; this asserts the field agrees with it, which is the only thing that makes the zone
  // weights below -- also written in standard deviations -- mean what they say.
  const salts = cloudSalts(OWNER.seed);
  let sum = 0;
  let sumSq = 0;
  for (let i = 0; i < N; i += 1) {
    const { latitudeDeg, longitudeDeg } = fibonacciPoint(i, N);
    const noiseOnly = cloudIndex(latitudeDeg, longitudeDeg, OWNER.radiusM, salts)
      - zonalCloudiness(latitudeDeg);
    sum += noiseOnly;
    sumSq += noiseOnly * noiseOnly;
  }
  const mean = sum / N;
  const sd = Math.sqrt(sumSq / N - mean * mean);
  assert.ok(
    Math.abs(sd - CLOUD_NOISE_SD) / CLOUD_NOISE_SD < 0.15,
    `noise-only sd ${sd.toFixed(4)} is more than 15% off the derived ${CLOUD_NOISE_SD.toFixed(4)}; ` +
    "every zone weight is in units of this number, so a mismatch scales the whole profile",
  );
  assert.ok(Math.abs(mean) < 0.15, `noise-only mean ${mean.toFixed(4)} is not near zero`);
});

test("the zonal profile DOMINATES the noise -- the rule that fixed the first draft", () => {
  // The first draft had macro weight 1.0 and a 4,000 km wavelength on a 4,500 km-radius planet,
  // which is seven wavelengths around the entire globe. Measured on a 200,000-point spiral at
  // 40% coverage it produced 3.9% coverage in the northern storm track and 70.8% in the
  // southern one: one hemispheric blob with a symmetric profile buried underneath. The bands
  // existed and could not be seen.
  //
  // The rule is that the profile's peak-to-trough must exceed the noise's standard deviation,
  // so latitude is the dominant term and the noise is the dither. A ratio near or below 1 is
  // the failed draft; this asserts it is comfortably above.
  const { range, lo, hi } = zonalRange();
  assert.ok(
    range / CLOUD_NOISE_SD >= 1.5,
    `zonal peak-to-trough ${range.toFixed(3)} (${lo.toFixed(3)}..${hi.toFixed(3)}) is only ` +
    `${(range / CLOUD_NOISE_SD).toFixed(2)}x the noise sd ${CLOUD_NOISE_SD.toFixed(3)}; ` +
    "below ~1 the bands are a modulation of a blob",
  );
  // And the other side of it: a profile that dwarfed the noise would give painted stripes with
  // straight edges, which is the other failure and looks nothing like weather.
  assert.ok(range / CLOUD_NOISE_SD <= 4, `zonal range is ${(range / CLOUD_NOISE_SD).toFixed(2)}x the noise sd -- the dither cannot cross it`);
});

test("the profile's extrema sit where the circulation puts them", () => {
  // Dead code looks like a feature: five zone terms is five chances to write a Gaussian that
  // never becomes the local extremum of the sum. Walked at 0.25 deg rather than evaluated at
  // the table's own latitudes, which would only prove the table can be read.
  const at = (lat) => zonalCloudiness(lat);
  const localMax = (centre) => {
    let best = -Infinity;
    let where = NaN;
    for (let lat = centre - 12; lat <= centre + 12; lat += 0.25) {
      if (at(lat) > best) { best = at(lat); where = lat; }
    }
    return where;
  };
  const localMin = (centre) => {
    let best = Infinity;
    let where = NaN;
    for (let lat = centre - 12; lat <= centre + 12; lat += 0.25) {
      if (at(lat) < best) { best = at(lat); where = lat; }
    }
    return where;
  };
  assert.ok(Math.abs(localMax(0)) <= 1, `ITCZ maximum is at ${localMax(0)} deg, not the equator`);
  for (const sign of [1, -1]) {
    assert.ok(
      Math.abs(localMin(sign * 25) - sign * 25) <= 5,
      `the subtropical minimum near ${sign * 25} deg is at ${localMin(sign * 25)}`,
    );
    assert.ok(
      Math.abs(localMax(sign * 60) - sign * 60) <= 6,
      `the storm-track maximum near ${sign * 60} deg is at ${localMax(sign * 60)}`,
    );
  }
  // Symmetric by construction, and it is a decision rather than an accident: a hemispheric
  // asymmetry in cloud cover is seasonal and this viewer has no season.
  for (let lat = 0; lat <= 90; lat += 5) {
    assert.ok(Math.abs(at(lat) - at(-lat)) < 1e-12, `profile is asymmetric at ${lat} deg`);
  }
});

// ==========================================================================================
// The coverage control, calibrated from the measurement
// ==========================================================================================

test("coverage in equals coverage out, across the slider's whole travel", () => {
  // The slider's travel IS the coverage. This is the assertion that makes that sentence true,
  // and it is measured on a FRESH spiral by `measuredCoverage` rather than read off the sorted
  // array the calibration had in hand -- a function that measured its own answer with its own
  // array would agree with itself by construction.
  for (const cover of [0.05, 0.1, 0.2, 0.3, 0.4, 0.5, 0.7, 0.9]) {
    const cal = calibrateClouds({ ...OWNER, cover });
    const got = measuredCoverage(cal, N);
    assert.ok(
      Math.abs(got - cover) <= 0.01,
      `asked for ${cover} coverage, measured ${got.toFixed(4)} (threshold ${cal.threshold.toFixed(4)})`,
    );
  }
});

test("coverage is strictly monotone in the slider", () => {
  let previous = -1;
  for (const cover of [0.05, 0.15, 0.3, 0.45, 0.6, 0.8, 0.95]) {
    const got = measuredCoverage(calibrateClouds({ ...OWNER, cover }), N);
    assert.ok(got > previous, `coverage ${got.toFixed(4)} at ${cover} did not exceed ${previous.toFixed(4)}`);
    previous = got;
  }
});

test("the two ends are exact: nothing at 0, everything at 1", () => {
  // An order statistic has no "above the maximum", so both ends are placed a full ramp beyond
  // the observed extremes rather than at a quantile. Without that, `cover = 0` would leave one
  // sample in 20,000 faintly lit and the layer would not be honestly off.
  const clear = calibrateClouds({ ...OWNER, cover: 0 });
  assert.equal(measuredCoverage(clear, N), 0);
  const clearTile = cloudTile({ rectangle: ITCZ_TILE, size: 64, clouds: clear });
  assert.equal(alphaStats(clearTile, ITCZ_TILE).max, 0, "cover 0 must write alpha 0 at every texel");

  const solid = calibrateClouds({ ...OWNER, cover: 1 });
  assert.equal(measuredCoverage(solid, N), 1);
  const solidTile = cloudTile({ rectangle: ITCZ_TILE, size: 64, clouds: solid });
  assert.equal(alphaStats(solidTile, ITCZ_TILE).min, CLOUD_MAX_ALPHA_BYTE);
});

test("COVERAGE_INDEX_OFFSET is exact, not a rounding shrug", () => {
  // alpha reaches COVERAGE_ALPHA part way UP the smoothstep, not at its midpoint, because
  // CLOUD_ALPHA_MAX is 0.94 rather than 1. The offset is small -- about 4.6% of a half-width --
  // and the temptation is to ignore it; ignoring it moves the delivered coverage by roughly a
  // point, which is inside nobody's ability to see and outside the tolerance asserted above.
  const threshold = 0;
  const justAbove = threshold + COVERAGE_INDEX_OFFSET + 1e-6;
  const justBelow = threshold + COVERAGE_INDEX_OFFSET - 1e-6;
  assert.ok(cloudAlpha(cloudDensity(justAbove, threshold)) >= COVERAGE_ALPHA);
  assert.ok(cloudAlpha(cloudDensity(justBelow, threshold)) < COVERAGE_ALPHA);
});

test("smoothstepInverse actually inverts smoothstep01", () => {
  for (let y = 0.01; y < 1; y += 0.01) {
    assert.ok(Math.abs(smoothstep01(smoothstepInverse(y)) - y) < 1e-9);
  }
});

// ==========================================================================================
// Bands, not uniform noise -- and the plausible mutant that proves this check has teeth
// ==========================================================================================

/// The five bands the banding claim is made over. Stated once, so the assertion and the mutation
/// demonstration below cannot disagree about what "the subtropics" means.
const BANDS = {
  itcz: [-8, 8],
  subtropicalN: [18, 32],
  subtropicalS: [-32, -18],
  stormN: [52, 68],
  stormS: [-68, -52],
};

/// The margin the wet bands must beat the dry ones by, in absolute coverage. 0.12 is roughly an
/// eighth of the disc and is comfortably visible; the measured margins at the shipped default
/// are far larger, and the gap between the two is the headroom this assertion has.
const BAND_MARGIN = 0.12;

function bandFractions(calibration) {
  const out = {};
  for (const [name, [south, north]] of Object.entries(BANDS)) {
    out[name] = bandCoverage(calibration, south, north, N).fraction;
  }
  return out;
}

function bandingFaults(calibration) {
  const f = bandFractions(calibration);
  const faults = [];
  for (const wet of ["itcz", "stormN", "stormS"]) {
    for (const dry of ["subtropicalN", "subtropicalS"]) {
      if (!(f[wet] - f[dry] >= BAND_MARGIN)) {
        faults.push(`${wet} ${f[wet].toFixed(4)} does not beat ${dry} ${f[dry].toFixed(4)} by ${BAND_MARGIN}`);
      }
    }
  }

  return { faults, fractions: f };
}

/// Each band's coverage minus the global coverage, at a stated sample count. Signed, so a wet
/// zone's number is positive and a dry zone's negative, and a zone that does nothing sits at zero.
function bandDepartures(samples) {
  const cal = calibrateClouds({ ...OWNER, cover: DEFAULT_CLOUD_COVER, samples });
  const global = measuredCoverage(cal, samples);
  const out = {};
  for (const [name, [south, north]] of Object.entries(BANDS)) {
    out[name] = bandCoverage(cal, south, north, samples).fraction - global;
  }
  return out;
}

/// Which zone owns which band, and which sign its departure must have.
const ZONE_OF_BAND = {
  itcz: ["ITCZ", +1],
  subtropicalN: ["subtropical high N", -1],
  subtropicalS: ["subtropical high S", -1],
  stormN: ["storm track N", +1],
  stormS: ["storm track S", +1],
};

test("the ITCZ and BOTH storm tracks are measurably cloudier than BOTH subtropical belts", () => {
  const { faults, fractions } = bandingFaults(OWNER_CAL);
  assert.deepEqual(faults, [], `band coverage: ${JSON.stringify(fractions)}`);
});

test("EVERY ONE of the five zone terms is load-bearing, proved by removing it", () => {
  // **This test exists because the pairwise check above was not enough, and a mutation found
  // that out.** Zeroing ONLY the ITCZ term turned exactly one test red -- and not the banding
  // one. With no ITCZ term the equator still beat the subtropics by a wide margin, because the
  // two subtropical minima sitting either side of it manufacture an apparent equatorial maximum
  // out of nothing. The check looked like it was testing five things and was testing two, which
  // is the shadowed-assertion shape this project keeps finding.
  //
  // The honest form is to remove each term and measure: a zone that does nothing puts its own
  // band's coverage at the global mean, so removing it must collapse that band's departure to at
  // most **40%** of what the shipped table produces. Measured shipped departures on the owner's
  // world (60,000 points): ITCZ +0.248, subtropical N -0.258, subtropical S -0.297, storm N
  // +0.078, storm S +0.434. With each zone removed in turn: +0.029, -0.036, -0.101, -0.161,
  // +0.205. Every one collapses; storm S has the least headroom and still clears it.
  //
  // The northern storm track's shipped departure is the smallest by a long way (+0.078 against
  // the southern one's +0.434) and that is NOT a defect: the macro noise field happens to sit dry
  // over the northern mid-latitudes on this world. It is the reason a fixed absolute margin
  // against the global mean was rejected in favour of this relative one -- no single threshold
  // separates "storm N is weak here" from "storm N does nothing".
  const samples = 20000;
  const shipped = bandDepartures(samples);
  const saved = CLOUD_ZONES.map((z) => z.weight);
  try {
    for (const [band, [zoneName, sign]] of Object.entries(ZONE_OF_BAND)) {
      const zone = CLOUD_ZONES.find((z) => z.name === zoneName);
      assert.ok(zone, `no zone named ${zoneName}`);
      assert.ok(
        sign * shipped[band] > 0,
        `${band}'s shipped departure ${shipped[band].toFixed(4)} has the wrong sign for ${zoneName}`,
      );
      zone.weight = 0;
      const without = bandDepartures(samples);
      zone.weight = saved[CLOUD_ZONES.indexOf(zone)];
      assert.ok(
        sign * without[band] <= 0.6 * sign * shipped[band],
        `removing ${zoneName} left ${band} at ${without[band].toFixed(4)} against a shipped ` +
        `${shipped[band].toFixed(4)} -- that term is not what makes its own band`,
      );
    }
  } finally {
    CLOUD_ZONES.forEach((zone, i) => { zone.weight = saved[i]; });
  }
});

test("THE PLAUSIBLE MUTANT: flattening the profile keeps the coverage and kills the bands", () => {
  // This is the mutation the brief asks for, run inside the suite rather than only outside it,
  // because it is the one that says what the banding check is worth. Flattening every zone
  // weight to zero is exactly "replace the profile with a constant" -- a constant offset is
  // absorbed by a quantile calibration, so the index's distribution, its spread and the global
  // coverage all survive it. Only the meaning is destroyed.
  //
  // A check that went red here for the wrong reason -- because the coverage moved -- would be
  // checking that something changed, not that the bands are gone. So both halves are asserted.
  const saved = CLOUD_ZONES.map((z) => z.weight);
  try {
    for (const zone of CLOUD_ZONES) zone.weight = 0;
    const flat = calibrateClouds({ ...OWNER, cover: DEFAULT_CLOUD_COVER });

    const coverage = measuredCoverage(flat, N);
    assert.ok(
      Math.abs(coverage - DEFAULT_CLOUD_COVER) <= 0.01,
      `the mutant's global coverage is ${coverage.toFixed(4)} -- if the coverage check went red ` +
      "here, the banding assertion below would be riding on the wrong signal",
    );

    const { faults, fractions } = bandingFaults(flat);
    assert.ok(
      faults.length > 0,
      `a profile-free field satisfied the banding check: ${JSON.stringify(fractions)}. ` +
      "The check is not checking for bands.",
    );
  } finally {
    CLOUD_ZONES.forEach((zone, i) => { zone.weight = saved[i]; });
  }
  // And the shipped table is restored, proven by the real thing passing again.
  assert.deepEqual(bandingFaults(OWNER_CAL).faults, []);
});

test("the banding survives RASTERISATION, measured over whole rings of tiles", () => {
  // The banding above is measured on the field. This is the same claim measured on the bytes,
  // which is a different population and can disagree with it: `alphaStats` counts a quantised
  // channel through a `round`, over a geographic grid, cos-weighted back to area.
  //
  // **It is a ring of sixteen tiles per band, not one tile against one tile**, and that is the
  // honest form rather than the convenient one. The first version of this check compared the
  // single ITCZ tile at 157.5-180 E against the subtropical tile directly north of it and FAILED
  // -- 0.2137 against 0.3036 -- because the macro field's own dry patch happens to sit on the
  // equator at that longitude. That is not a defect: a zonal mean is a claim about a ring, and a
  // single meridian is a sample of size one. Quoting one tile pair that happened to agree would
  // have been picking the longitude to fit the claim.
  const ring = (north, south) => {
    let covered = 0;
    let weight = 0;
    for (let lon = -180; lon < 180; lon += 22.5) {
      const rect = { northDeg: north, southDeg: south, westDeg: lon, eastDeg: lon + 22.5 };
      const stats = alphaStats(cloudTile({ rectangle: rect, size: 64, clouds: OWNER_CAL }), rect);
      covered += stats.coverage * stats.weight;
      weight += stats.weight;
    }
    return covered / weight;
  };
  const itcz = ring(ITCZ_TILE.northDeg, ITCZ_TILE.southDeg);
  const dry = ring(SUBTROPICAL_TILE.northDeg, SUBTROPICAL_TILE.southDeg);
  assert.ok(
    itcz - dry >= BAND_MARGIN,
    `ITCZ ring ${itcz.toFixed(4)} vs subtropical ring ${dry.toFixed(4)}`,
  );
});

test("meanZonalCloudiness is near zero, so the profile redistributes rather than shifts", () => {
  // If the profile had a large area-weighted mean it would be a global brightness offset with a
  // wobble, and the calibration would silently absorb the offset -- which is fine numerically
  // and misleading structurally, because the "bands" would then be doing less than they look.
  assert.ok(Math.abs(meanZonalCloudiness(20000)) < 0.15, `profile area-mean is ${meanZonalCloudiness(20000)}`);
});

// ==========================================================================================
// Structure, softness, and the three degenerate layers that must be refused
// ==========================================================================================

/// A raster of one constant alpha, for the refusal tests.
function constantAlphaTile(alpha, size = 32) {
  const data = new Uint8ClampedArray(size * size * 4);
  for (let i = 0; i < size * size; i += 1) {
    data[i * 4] = 250; data[i * 4 + 1] = 251; data[i * 4 + 2] = 253; data[i * 4 + 3] = alpha;
  }
  return { data, width: size, height: size };
}

test("a real tile has coverage, structure and a soft edge", () => {
  const image = cloudTile({ rectangle: ITCZ_TILE, size: CLOUD_TILE_SIZE, clouds: OWNER_CAL });
  const verdict = hasCloudStructure(image, { rectangle: ITCZ_TILE });
  assert.ok(verdict.ok, verdict.reasons.join("; "));
  // Sibling neutralisation: "it has structure" is satisfied by anything non-constant, so the
  // three quantities that matter are also asserted individually with their own numbers.
  assert.ok(verdict.stats.clearFraction >= 0.05, `only ${verdict.stats.clearFraction.toFixed(4)} of the tile is clear sky`);
  assert.ok(verdict.stats.transitionFraction >= 0.05, `only ${verdict.stats.transitionFraction.toFixed(4)} of the tile is in the soft edge`);
  assert.ok(verdict.stats.distinctBins >= 64, `only ${verdict.stats.distinctBins} distinct alpha values -- a soft ramp should fill the channel`);
});

test("TRANSPARENT EVERYWHERE is refused -- the failure that looks like light cloud", () => {
  const verdict = hasCloudStructure(constantAlphaTile(0));
  assert.equal(verdict.ok, false);
  assert.ok(verdict.reasons.some((r) => r.startsWith("coverage")), verdict.reasons.join("; "));
});

test("OPAQUE EVERYWHERE is refused", () => {
  const verdict = hasCloudStructure(constantAlphaTile(CLOUD_MAX_ALPHA_BYTE));
  assert.equal(verdict.ok, false);
  assert.ok(verdict.reasons.some((r) => r.startsWith("coverage")), verdict.reasons.join("; "));
});

test("A UNIFORM VEIL is refused even though its coverage is in band", () => {
  // The one a coverage band alone cannot catch. A constant alpha just above the threshold has
  // coverage 1 -- so it is caught here by the band -- but a raster that is HALF constant-clear
  // and half constant-solid has coverage near 0.5, perfect spread, and no cloud in it at all.
  const size = 32;
  const data = new Uint8ClampedArray(size * size * 4);
  for (let row = 0; row < size; row += 1) {
    for (let col = 0; col < size; col += 1) {
      const i = (row * size + col) * 4;
      data[i] = 250; data[i + 1] = 251; data[i + 2] = 253;
      data[i + 3] = row < size / 2 ? 0 : CLOUD_MAX_ALPHA_BYTE;
    }
  }
  const verdict = hasCloudStructure({ data, width: size, height: size });
  assert.equal(verdict.ok, false, "a hard-edged half-and-half raster has coverage 0.5 and full spread");
  assert.ok(
    verdict.reasons.some((r) => r.includes("hard edge")),
    `expected the soft-edge reason, got: ${verdict.reasons.join("; ")}`,
  );
});

test("the alpha channel reaches BOTH ends on the owner's world -- no dead range", () => {
  // `CLOUD_MAX_ALPHA_BYTE` exists because the first `alphaStats` counted solid deck as
  // `alpha >= 250` against a channel that caps at 240, so that counter was structurally zero.
  // This is the check that would have caught it: over a global scan, both ends must be reached.
  let sawClear = false;
  let sawSolid = false;
  for (let lat = 75; lat >= -75; lat -= 30) {
    for (let lon = -180; lon < 180; lon += 45) {
      const rect = { northDeg: lat, southDeg: lat - 30, westDeg: lon, eastDeg: lon + 45 };
      const stats = alphaStats(cloudTile({ rectangle: rect, size: 32, clouds: OWNER_CAL }), rect);
      if (stats.min === 0) sawClear = true;
      if (stats.max === CLOUD_MAX_ALPHA_BYTE) sawSolid = true;
      if (stats.opaqueFraction > 0) sawSolid = true;
    }
  }
  assert.ok(sawClear, "no texel anywhere on this world is fully clear");
  assert.ok(sawSolid, `no texel anywhere reaches ${CLOUD_MAX_ALPHA_BYTE}, so the deck end of the ramp is dead`);
});

// ==========================================================================================
// The field is a point function of position on the SPHERE
// ==========================================================================================

test("two overlapping tiles agree byte-for-byte on their shared meridians", () => {
  // The seam check, in the form `biome.js` arrived at after its first longitude assertion was
  // found not load-bearing. Tile A spans 90..110 E and tile B 100..120 E, both 41 posts over
  // 20 deg, so A's columns 20..40 and B's columns 0..20 are the SAME meridians and must come
  // back identical. A field evaluated on the lat/lon rectangle instead of the sphere passes
  // every visual inspection and shows a seam only where nobody photographs.
  const size = 41;
  const north = 20;
  const south = 0;
  const a = cloudTile({ rectangle: { northDeg: north, southDeg: south, westDeg: 90, eastDeg: 110 }, size, clouds: OWNER_CAL });
  const b = cloudTile({ rectangle: { northDeg: north, southDeg: south, westDeg: 100, eastDeg: 120 }, size, clouds: OWNER_CAL });
  let compared = 0;
  for (let row = 0; row < size; row += 1) {
    for (let k = 0; k <= 20; k += 1) {
      const ai = (row * size + (20 + k)) * 4;
      const bi = (row * size + k) * 4;
      for (let c = 0; c < 4; c += 1) {
        assert.equal(a.data[ai + c], b.data[bi + c], `row ${row} shared column ${k} channel ${c}`);
      }
      compared += 1;
    }
  }
  assert.ok(compared === size * 21);
});

test("the antimeridian is not a seam", () => {
  // Not bit-exact, and the reason is worth stating rather than papering over with a loose
  // tolerance: `Math.sin(Math.PI)` is 1.2246e-16 and `Math.sin(-Math.PI)` is its negative, so
  // +-180 deg map to unit vectors that differ in the y component by 2.4e-16. That is the
  // floating-point representation of pi, not a seam -- a field evaluated on the lat/lon
  // rectangle would differ here by a whole lattice cell, twelve orders of magnitude larger. The
  // tolerance is set to catch that and nothing looser.
  const salts = cloudSalts(OWNER.seed);
  for (const lat of [-60, -20, 0, 20, 60]) {
    const east = cloudIndex(lat, 180, OWNER.radiusM, salts);
    const west = cloudIndex(lat, -180, OWNER.radiusM, salts);
    assert.ok(
      Math.abs(east - west) < 1e-9,
      `the field disagrees with itself at +-180 deg on latitude ${lat}: ${east} vs ${west}`,
    );
  }
  // And the alpha bytes, which is what actually reaches the screen, must be identical.
  for (const lat of [-60, -20, 0, 20, 60]) {
    const at = (lon) => cloudAlpha(cloudDensity(cloudIndex(lat, lon, OWNER.radiusM, salts), OWNER_CAL.threshold));
    assert.equal(at(180), at(-180), `the rendered alpha differs across the antimeridian at ${lat} deg`);
  }
});

test("the field does not depend on the order it is asked in", () => {
  const salts = cloudSalts(OWNER.seed);
  const first = cloudIndex(12.5, -47.25, OWNER.radiusM, salts);
  for (let i = 0; i < 1000; i += 1) cloudIndex(i * 0.37 - 90, i * 1.13 - 180, OWNER.radiusM, salts);
  assert.equal(cloudIndex(12.5, -47.25, OWNER.radiusM, salts), first);
});

test("every world gets its own weather", () => {
  // Without the seed mix the salts are module constants and the field is a function of position
  // alone, so two seeds would produce pixel-identical cloud -- and unlike the biome fields there
  // is no per-world elevation underneath to disguise it.
  const a = cloudTile({ rectangle: ITCZ_TILE, size: 32, clouds: calibrateClouds({ radiusM: OWNER.radiusM, seed: "1", cover: 0.4 }) });
  const b = cloudTile({ rectangle: ITCZ_TILE, size: 32, clouds: calibrateClouds({ radiusM: OWNER.radiusM, seed: "2", cover: 0.4 }) });
  let differing = 0;
  for (let i = 3; i < a.data.length; i += 4) if (a.data[i] !== b.data[i]) differing += 1;
  assert.ok(differing > 0.5 * (a.data.length / 4), `only ${differing} of ${a.data.length / 4} alpha texels differ between two seeds`);

  const again = cloudTile({ rectangle: ITCZ_TILE, size: 32, clouds: calibrateClouds({ radiusM: OWNER.radiusM, seed: "1", cover: 0.4 }) });
  assert.deepEqual(Array.from(again.data), Array.from(a.data), "the same seed did not reproduce");
});

test("a smaller planet gets smaller weather, not the same picture scaled", () => {
  // The wavelengths are in metres on the ground and are divided into the world's radius, so a
  // smaller planet fits more systems around it. Counted as threshold crossings along a
  // 2,000-point equatorial transect -- a count, not a duration, so it is a property of the
  // algorithm rather than of the moment.
  const crossings = (radiusM) => {
    const cal = calibrateClouds({ radiusM, seed: OWNER.seed, cover: 0.4, samples: 20000 });
    let previous = null;
    let count = 0;
    for (let i = 0; i < 2000; i += 1) {
      const lon = -180 + (360 * i) / 2000;
      const covered = cloudIndex(3, lon, radiusM, cal.salts) >= cal.threshold;
      if (previous !== null && covered !== previous) count += 1;
      previous = covered;
    }
    return count;
  };
  const small = crossings(1_500_000);
  const large = crossings(9_000_000);
  assert.ok(large > small * 1.5, `9,000 km radius gave ${large} crossings against ${small} at 1,500 km`);
});

test("the fine field is stretched east-west, which is what makes it read as cirrus", () => {
  // `CLOUD_STRETCH` squashes the polar-axis component of the sample point, so features are
  // shorter in latitude than in longitude. Measured as threshold crossings along a meridional
  // transect against a zonal one of the same angular length, with the zonal profile removed so
  // the count is about the noise rather than about the bands.
  const salts = cloudSalts(OWNER.seed);
  const noise = (lat, lon) => cloudIndex(lat, lon, OWNER.radiusM, salts) - zonalCloudiness(lat);
  const count = (points) => {
    let previous = null;
    let n = 0;
    for (const value of points) {
      const up = value > 0;
      if (previous !== null && up !== previous) n += 1;
      previous = up;
    }
    return n;
  };
  const meridional = [];
  const zonal = [];
  for (let i = 0; i < 1200; i += 1) {
    const t = -30 + (60 * i) / 1200;
    meridional.push(noise(t, 40));
    zonal.push(noise(5, 40 + t));
  }
  assert.ok(
    count(meridional) > count(zonal),
    `CLOUD_STRETCH = ${CLOUD_STRETCH} should make the field cross more often going north-south ` +
    `(${count(meridional)}) than east-west (${count(zonal)})`,
  );
});

// ==========================================================================================
// What this layer must NOT be
// ==========================================================================================

test("cloudTile never touches an engine, and clouds.js cannot", () => {
  // Weather is not a function of the ground here. Asserted two ways, because either alone is
  // weak: a proxy that throws on ANY property access proves this call did not reach for one, and
  // a source sweep proves no other path in the module could.
  const forbidden = new Proxy({}, {
    get(_t, prop) { throw new Error(`clouds.js reached for engine.${String(prop)}`); },
  });
  const image = cloudTile({
    rectangle: ITCZ_TILE, size: 16, clouds: OWNER_CAL,
    engine: forbidden, worldHandle: forbidden,
  });
  assert.equal(image.width, 16);

  const source = appFile("clouds.js");
  for (const name of ["fillTileF32", "elevationM", "worldHandle", "newWorld"]) {
    // The module doc names `worldHandle`; the check is about CALLS, so an occurrence inside a
    // comment line is not a fault. Lines are filtered rather than the whole file searched,
    // which is the difference between this passing for the right reason and passing by luck.
    const offending = source.split("\n")
      .filter((line) => !line.trimStart().startsWith("//") && !line.trimStart().startsWith("///"))
      .filter((line) => line.includes(name));
    assert.deepEqual(offending, [], `clouds.js has executable code mentioning ${name}`);
  }
});

test("the level cap is derived from the field's own resolution floor", () => {
  // The cloud layer stops at level 5 where the relief layer goes to 12, and that has to be a
  // derivation rather than a saving taken blind: past the cap Cesium magnifies the parent
  // texture, and if the field still had detail there the layer would be throwing it away.
  const finestM = CLOUD_FINE_WAVELENGTH_M / FBM_LACUNARITY ** (CLOUD_FINE_OCTAVES - 1);
  // A level-N geographic tile spans 180 / 2^N degrees of latitude.
  const tileDeg = 180 / 2 ** CLOUD_MAX_LEVEL;
  const metresPerDeg = (Math.PI * OWNER.radiusM) / 180;
  const texelM = (tileDeg * metresPerDeg) / CLOUD_TILE_SIZE;
  const samplesPerFeature = finestM / texelM;
  // Above Nyquist, with the margin stated rather than implied. This is asserted about the RATIO
  // and not about the level number, so moving the cap, the tile size, or any of the field's
  // wavelengths all have to answer the same arithmetic.
  assert.ok(
    samplesPerFeature >= 2.5,
    `at level ${CLOUD_MAX_LEVEL} a ${CLOUD_TILE_SIZE}-texel tile samples every ${texelM.toFixed(0)} m ` +
    `against a finest feature of ${finestM.toFixed(0)} m -- only ${samplesPerFeature.toFixed(2)} ` +
    "samples per wavelength, so the cap is cutting real detail rather than magnifying a resolved field",
  );
  // And the other side: a cap far above the field's own floor is asking the pool for levels of
  // tiles that carry no new content, which is what the 292-against-73 measurement caught.
  assert.ok(
    samplesPerFeature <= 12,
    `${samplesPerFeature.toFixed(2)} samples per finest wavelength is oversampling the field by ` +
    "an order of magnitude; the cap is buying tiles that carry nothing",
  );
});

test("the coverage slider can express its own default, and its travel is the coverage", () => {
  // `panelFieldFaults()` covers the lattice for every field; this pins the two things specific
  // to this one -- that the panel's `clouds` field IS a 0..1 coverage rather than an index
  // threshold in disguise, and that the shipped default is the figure the gap analysis read off
  // the reference.
  const field = PANEL_RANGES.find((f) => f.query === "clouds");
  assert.ok(field, "the panel has no clouds field");
  assert.equal(field.min, 0);
  assert.equal(field.max, 1);
  assert.equal(field.value, DEFAULT_CLOUD_COVER);
  assert.equal(DEFAULT_CLOUD_COVER, 0.4);
});

test("the default's delivered coverage is what the report quotes", () => {
  // The number the owner is being shown, measured rather than assumed, on the world they look
  // at. If this moves, the report is wrong and should fail here rather than be believed.
  const got = measuredCoverage(OWNER_CAL, N);
  assert.ok(Math.abs(got - 0.4) <= 0.01, `shipped default delivers ${got.toFixed(4)} coverage`);
  assert.ok(OWNER_CAL.sd > 1 && OWNER_CAL.sd < 1.3, `index sd ${OWNER_CAL.sd.toFixed(4)} moved`);
});

test("the second world is calibrated on its own field, not on the first one's", () => {
  const other = calibrateClouds({ ...DEFAULT, cover: DEFAULT_CLOUD_COVER });
  assert.notEqual(other.threshold, OWNER_CAL.threshold);
  assert.ok(Math.abs(measuredCoverage(other, N) - DEFAULT_CLOUD_COVER) <= 0.01);
  assert.deepEqual(bandingFaults(other).faults, [], "the banding does not hold on the default world");
});

test("CLOUD_EDGE_W is a soft edge and not a token one", () => {
  // The transition band's width in standard deviations of the noise. Below about a fifth of an
  // sd the ramp is narrower than the field's own texel-to-texel variation and the edge is hard
  // in practice however smooth the function is.
  const widthInSd = (2 * CLOUD_EDGE_W) / CLOUD_NOISE_SD;
  assert.ok(widthInSd >= 0.4 && widthInSd <= 2, `the alpha ramp is ${widthInSd.toFixed(2)} sd wide`);
  assert.ok(CLOUD_ALPHA_MAX < 1, "a fully opaque deck reads as a hole cut in the planet");
});
