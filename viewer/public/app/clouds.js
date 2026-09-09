//! The cloud layer: a procedural weather deck drawn over the globe as its own raster.
//
// # Why this file exists
//
// `docs/design/2026-09-05-north-star-gap.md` ranks clouds **difference #1 of 12** between this
// viewer and the owner's reference, and it is the one element of the top four that touches no
// engine code at all. The reference carries white cirrus and cumulus over roughly 40% of the
// disc, and that -- not the terrain -- is what a viewer's eye reads first as "photograph of a
// planet" rather than "diagram of a planet".
//
// # Bands, not uniform noise, and this is the whole design
//
// An fBm field thresholded at a constant reads as **static** at high frequency and as **marble**
// at low frequency. Neither reads as weather, because real cloud cover is not spatially
// stationary: it has meridional structure that comes from the general circulation, and the eye
// knows it even when it cannot name it.
//
// **What is being matched, stated:** the *shape* of the zonal-mean cloud fraction of a rotating
// planet with a three-cell circulation -- a maximum on the ITCZ where the Hadley cells converge
// and air ascends; minima in the subtropics near 20-30 deg where the same cells subside and dry
// the column; and secondary maxima along the mid-latitude storm tracks near 55-65 deg where the
// polar front lifts air along travelling cyclones. Those three features and their latitudes are
// the physics, and they are what `CLOUD_ZONES` below encodes.
//
// **What is NOT being done, equally deliberately:** no number here is transcribed from a
// published cloud climatology. The zone *latitudes* and *signs* are the circulation's; the zone
// *amplitudes* are in standard deviations of this file's own noise, chosen so the bands are
// legible against the dither, and the resulting global coverage is then set by calibration
// rather than by those amplitudes. A transcribed table would be a fourth copy of a number
// nobody in this repository can check.
//
// # The coverage control is calibrated from a MEASUREMENT, not from a nominal range
//
// This repository has already been bitten once, hard, by trusting a noise field's nominal
// range: `biome.js`'s `FBM_SD` records that a trilinear value-noise fBm normalised by its own
// amplitudes has standard deviation **0.105** against a nominal +-0.5, so every weight written
// against the nominal range was **five times too weak** and nine of thirty-three colours were
// unreachable. A cloud threshold picked against a nominal range would fail the same way and it
// would fail *invisibly*, because a layer that is transparent everywhere looks exactly like
// "light cloud cover".
//
// So the slider's travel is **the coverage itself**, and the threshold that produces it is
// found by inverting the field's own measured distribution: `calibrateClouds` samples the index
// on an equal-area Fibonacci spiral, takes the order statistic at `1 - cover`, and shifts it by
// the exact amount the alpha ramp needs so that the fraction of the sphere at or above
// `COVERAGE_ALPHA` is the number on the slider. Coverage in equals coverage out, by
// construction, and `clouds.test.mjs` measures it back out of a rendered raster rather than
// believing the construction.
//
// # Soft edges
//
// A hard alpha cutoff is the single clearest tell that a cloud layer is a threshold on noise.
// The density is a `smoothstep` over a band `+-CLOUD_EDGE_W` wide in index units, so every
// cloud has a real transition, and the thin end is tinted cooler and greyer than the core --
// cirrus is translucent throughout and cumulus is bright only where it is thick.
//
// # This file never touches the engine
//
// Weather is not a function of the ground here, and nothing below takes an `engine` or a world
// handle. That is asserted rather than commented: `clouds.test.mjs` hands `cloudTile` a spy
// engine and checks it is never called. It matters because it is what makes the cloud raster
// cost independent of the terrain fill, and because a cloud layer that quietly started reading
// elevations would be a third elevation-colour system arriving by the back door.
//
// It does import `biome.js`, but only for its **noise primitives** -- `fbm3`, `unitVector`,
// `fibonacciPoint`, `quantile`. Those are a library, not a climate: `?biome=0` does not and must
// not change a cloud.

import { fbm3, fibonacciPoint, quantile, unitVector } from "./biome.js";

// ============================================================================================
// Raster geometry
// ============================================================================================

/// 128 texels per tile edge, against the relief layer's 256.
///
/// **A narrower imagery tile does NOT halve the work, and the first draft of this file believed
/// it did.** Cesium picks the imagery level from the terrain tile's geometric error, and geometric
/// error is inversely proportional to `tileWidth` -- so halving the width lowers level-zero error
/// and Cesium refines ONE LEVEL FURTHER, landing on the same metres per texel. That is the same
/// detail-invariance `relief-provider.js` records for heightmap width, met from the other
/// direction -- and `viewer/README.md` already records the measurement for the relief layer:
/// `?reliefSize=` 128 / 256 / 512 gives **244 / 61 / 15 tiles** at **4.00 / 4.00 / 3.93 M
/// texels**. Measured again here, in the browser at the orbital camera on the owner's world: the
/// 128-texel cloud layer was asked for **292 tiles where the 256-texel relief layer was asked for
/// 73** -- exactly four times as many, one level deeper, for the same total texel count plus four
/// times the per-tile overhead.
///
/// So the narrow tile is only a saving when it is taken TOGETHER with the level cap below, which
/// is what stops that extra level from being requested at all.
export const CLOUD_TILE_SIZE = 128;

/// The cloud layer stops refining at level 3, where the relief layer goes to 12.
///
/// **This is the constant that pays for the third pool consumer**, and it is derived from the
/// field's own resolution rather than chosen for the number it produces. The finest structure the
/// field carries is `CLOUD_FINE_WAVELENGTH_M / FBM_LACUNARITY^(CLOUD_FINE_OCTAVES - 1)` = 350 km
/// / 2.13^3 = **36.2 km**. A level-3 geographic tile spans 22.5 deg, so 128 texels sample it every
/// **13.8 km** at the owner's radius -- **2.6 samples per finest wavelength**, above Nyquist,
/// with the margin stated rather than implied.
///
/// Past the cap Cesium magnifies the parent texture, and for a soft translucent alpha layer that
/// is invisible: a bilinearly magnified cloud edge is still a cloud edge, which is not true of a
/// coastline. `clouds.test.mjs` asserts the sample ratio rather than the level, so moving either
/// the cap or the field's wavelengths has to answer the same arithmetic.
///
/// **What it costs, measured, at the orbital camera on the owner's world:** without the cap the
/// layer draws 292 tiles of 16,384 texels (4.79 M texels, the same as the relief layer's 73 tiles
/// of 65,536); with it, 73 tiles and 1.20 M texels. A quarter of the work for a layer that was
/// already oversampled.
///
/// **And what it costs in the picture:** on a descent past level 3 the clouds stop sharpening
/// while the ground keeps going. That is the honest cost, and it is the right trade for a layer
/// that is an orbital feature -- the reference this task is chasing is a picture of a whole
/// planet.
export const CLOUD_MAX_LEVEL = 3;

// ============================================================================================
// The zonal profile -- the part that makes this weather rather than marble
// ============================================================================================

/// The meridional structure, as named circulation features.
///
/// Each entry is a Gaussian in latitude: `weight * exp(-0.5 * ((lat - centre) / width)^2)`.
/// **`weight` is in standard deviations of the noise index**, which is the only unit it can be
/// reasoned about in -- the same discipline `biome.js` adopted after `FBM_SD`. A weight of 0.55
/// against a noise standard deviation of ~1.30 is a band that shifts coverage substantially and
/// still lets the dither cross it, which is what stops the bands reading as painted stripes.
///
/// **Why these latitudes.** Hadley ascent puts the convective maximum on the ITCZ; the descending
/// branch of the same cell dries the subtropics, and the subtropical high belt sits near 25 deg
/// in both hemispheres -- it is where every major desert on Earth is, and the two facts are the
/// same fact. The polar front, where mid-latitude cyclones travel, sits near 60 deg. The profile
/// is symmetric about the equator here **on purpose**: a hemispheric asymmetry is seasonal, and
/// this viewer has no season.
///
/// Every zone is asserted to be *reached* -- `clouds.test.mjs` measures coverage per latitude
/// band on the owner's own world and requires the ITCZ and both storm tracks to beat the
/// subtropics by a stated margin. Dead code looks like a feature, and a zone term that never
/// moved a texel would look exactly like this table.
export const CLOUD_ZONES = [
  { name: "ITCZ", latitudeDeg: 0, widthDeg: 9, weight: 0.85 },
  { name: "subtropical high N", latitudeDeg: 25, widthDeg: 12, weight: -0.95 },
  { name: "subtropical high S", latitudeDeg: -25, widthDeg: 12, weight: -0.95 },
  { name: "storm track N", latitudeDeg: 60, widthDeg: 15, weight: 0.80 },
  { name: "storm track S", latitudeDeg: -60, widthDeg: 15, weight: 0.80 },
];

/// The zonal profile at one latitude, in standard deviations of the noise index.
export function zonalCloudiness(latitudeDeg) {
  let sum = 0;
  for (const zone of CLOUD_ZONES) {
    const t = (latitudeDeg - zone.latitudeDeg) / zone.widthDeg;
    sum += zone.weight * Math.exp(-0.5 * t * t);
  }
  return sum;
}

/// The area-weighted mean of `zonalCloudiness` over the sphere.
///
/// Computed rather than written down, over the same equal-area spiral the calibration uses. It
/// exists so a mutation can replace the profile with *this constant* -- the plausible mutant,
/// which preserves the index's global distribution almost exactly and destroys only the
/// meaning. A check that survives that mutation is not checking for bands.
export function meanZonalCloudiness(samples = 20000) {
  let sum = 0;
  for (let i = 0; i < samples; i += 1) sum += zonalCloudiness(fibonacciPoint(i, samples).latitudeDeg);
  return sum / samples;
}

// ============================================================================================
// The noise index
// ============================================================================================

/// Three fields, in metres of wavelength on the ground, with their octave counts.
///
/// Cloud systems are large: a synoptic weather system is thousands of kilometres and the cells
/// inside it are tens. `macro` is the system scale, `meso` the individual storm, `fine` the
/// texture that makes an edge ragged rather than drawn. Wavelengths are converted to unit-sphere
/// frequency by the world's own radius (`freq = radiusM / wavelengthM`), so a small planet gets
/// proportionally smaller weather rather than the same picture at a different size -- the same
/// rule `biome.js` uses, for the same reason.
export const CLOUD_MACRO_WAVELENGTH_M = 2_500_000;
export const CLOUD_MESO_WAVELENGTH_M = 900_000;
export const CLOUD_FINE_WAVELENGTH_M = 350_000;
export const CLOUD_MACRO_OCTAVES = 3;
export const CLOUD_MESO_OCTAVES = 4;
export const CLOUD_FINE_OCTAVES = 4;

/// Field weights, **in standard deviations of each field** -- `fbm3` is standardised by
/// `FBM_SD`, so a weight of 1 is one sd.
///
/// # These are the numbers the first draft got wrong, and the measurement is why
///
/// The first draft had `macro` at 1.0 against zone weights of ~0.55. The owner's planet has a
/// radius of 4,500 km, so its circumference is 28,274 km and a 4,000 km macro field fitted
/// **seven wavelengths around the whole globe** -- a field that large is not "system scale", it
/// is hemispheric. Measured on a 200,000-point spiral at 40% coverage, that draft produced
/// **3.9% coverage in the northern storm track and 70.8% in the southern one**: not bands, one
/// blob and its complement, with a symmetric profile buried underneath.
///
/// The rule that replaced the guess, and it is asserted in `clouds.test.mjs`: **the zonal
/// profile's peak-to-trough range must exceed the noise's standard deviation**, so latitude is
/// the dominant term and the noise is the dither on top of it. Otherwise the profile is a
/// modulation of a blob and the bands are decoration.
export const CLOUD_MACRO_W = 0.55;
export const CLOUD_MESO_W = 0.6;
export const CLOUD_FINE_W = 0.4;

/// The noise index's standard deviation with the weights above, if the three fields are
/// independent. Stated as a derivation and **measured back** in `clouds.test.mjs`, which is the
/// point -- `FBM_SD` exists because this repository once believed a field's nominal spread and
/// was wrong by a factor of five.
export const CLOUD_NOISE_SD = Math.sqrt(
  CLOUD_MACRO_W ** 2 + CLOUD_MESO_W ** 2 + CLOUD_FINE_W ** 2,
);

/// The zonal profile's peak-to-trough range. Computed over a 0.25-degree walk rather than read
/// off the table, because the zones overlap and the extremes of the SUM are not the extremes of
/// any one term -- the subtropical minimum is deepened by the ITCZ's and the storm track's tails
/// on either side of it.
export function zonalRange() {
  let lo = Infinity;
  let hi = -Infinity;
  for (let lat = -90; lat <= 90; lat += 0.25) {
    const z = zonalCloudiness(lat);
    if (z < lo) lo = z;
    if (z > hi) hi = z;
  }
  return { lo, hi, range: hi - lo };
}

/// The meridional squash applied to the `fine` field's sample point.
///
/// Multiplying the polar-axis component of the unit vector before scaling shortens the field's
/// features in latitude and leaves them alone in longitude, so the finest texture comes out
/// **elongated east-west**. That is what cirrus and jet-stream cloud actually look like from
/// orbit, and it is one multiplication rather than a second anisotropic noise implementation.
export const CLOUD_STRETCH = 3.5;

/// Base salts. Distinct so the three fields are independent rather than three views of one, and
/// distinct from `biome.js`'s (`0x5eed01..04`) so the weather is not a recolouring of the
/// vegetation.
export const CLOUD_MACRO_SALT = 0xc10d01;
export const CLOUD_MESO_SALT = 0xc10d02;
export const CLOUD_FINE_SALT = 0xc10d03;

/// Per-world salts, mixed from the seed.
///
/// **Without this every world would have the same weather**, because the salts above are module
/// constants and the field is a function of position alone. `biome.js` gets away with fixed
/// salts because its fields only ever perturb thresholds on a per-world *elevation*; a cloud
/// deck has no such anchor, so two seeds would produce pixel-identical cloud and it would take a
/// side-by-side to notice.
///
/// The seed may be a string, a number or a BigInt (it arrives from a URL parameter), so it is
/// reduced through `BigInt` and masked to 32 bits before the mix.
export function cloudSalts(seed) {
  const s = Number(BigInt.asUintN(32, BigInt(seed ?? 0)));
  const mix = (base) => (base ^ Math.imul(s, 0x9e3779b1)) | 0;
  return { macro: mix(CLOUD_MACRO_SALT), meso: mix(CLOUD_MESO_SALT), fine: mix(CLOUD_FINE_SALT) };
}

/// The cloudiness index at a point: zonal profile plus three weighted noise fields, in standard
/// deviations. Unbounded in principle, roughly normal with sd ~1.3 plus the profile's spread.
///
/// **The noise domain is the unit sphere, not the lat/lon rectangle.** That is what removes the
/// antimeridian seam and the polar pinch, and `clouds.test.mjs` proves it by cutting two
/// overlapping tiles and requiring the shared meridians to come back byte-identical -- a field
/// evaluated on (lat, lon) passes every visual inspection and shows a seam only at the one
/// longitude nobody photographs.
export function cloudIndex(latitudeDeg, longitudeDeg, radiusM, salts) {
  const [ux, uy, uz] = unitVector(latitudeDeg, longitudeDeg);
  const fMacro = radiusM / CLOUD_MACRO_WAVELENGTH_M;
  const fMeso = radiusM / CLOUD_MESO_WAVELENGTH_M;
  const fFine = radiusM / CLOUD_FINE_WAVELENGTH_M;
  const macro = fbm3(ux * fMacro, uy * fMacro, uz * fMacro, CLOUD_MACRO_OCTAVES, salts.macro);
  const meso = fbm3(ux * fMeso, uy * fMeso, uz * fMeso, CLOUD_MESO_OCTAVES, salts.meso);
  const fine = fbm3(
    ux * fFine, uy * fFine, uz * fFine * CLOUD_STRETCH, CLOUD_FINE_OCTAVES, salts.fine,
  );
  return zonalCloudiness(latitudeDeg)
    + CLOUD_MACRO_W * macro + CLOUD_MESO_W * meso + CLOUD_FINE_W * fine;
}

// ============================================================================================
// Index -> opacity
// ============================================================================================

/// Half-width of the alpha ramp, in index units (i.e. in noise standard deviations).
///
/// 0.45 against a noise sd of 1.30 is a transition band **0.69 sd wide**, which is wide enough
/// that every cloud has a visible soft margin and narrow enough that the field still has clear
/// sky and solid deck rather than being uniformly half-lit. Both halves of that sentence are
/// checked: `clouds.test.mjs` requires a substantial fraction of texels in the transition AND a
/// substantial fraction fully clear, so neither a hard cutoff nor a global haze passes.
export const CLOUD_EDGE_W = 0.45;

/// Peak opacity of a solid cloud deck. Not 1: even thick cloud from orbit sits over ground that
/// is faintly visible at the edges, and a fully opaque white patch reads as a hole cut in the
/// planet.
export const CLOUD_ALPHA_MAX = 0.94;

/// **The opacity a texel must reach to be counted as covered**, out of 255. Half-opaque, stated
/// once here so the coverage figure in the report, the calibration that produces it and the test
/// that measures it back are all talking about the same threshold. Every coverage number in this
/// task names this constant.
export const COVERAGE_ALPHA = 128;

/// The largest alpha byte this layer can write: `round(255 * CLOUD_ALPHA_MAX)`.
///
/// Derived rather than written as a literal, because the first draft of `alphaStats` counted
/// "solid deck" as `alpha >= 250` and `CLOUD_ALPHA_MAX = 0.94` caps the channel at **240** -- so
/// that counter was structurally zero on every raster and would have read as "this world has no
/// thick cloud". A branch that cannot be taken looks exactly like a feature, which is this
/// repository's own recorded defect family, and it was found here by printing the histogram
/// rather than by reading the code.
export const CLOUD_MAX_ALPHA_BYTE = Math.round(255 * CLOUD_ALPHA_MAX);

function clamp01(x) {
  return x < 0 ? 0 : x > 1 ? 1 : x;
}

/// The standard Hermite smoothstep on [0,1].
export function smoothstep01(t) {
  const x = clamp01(t);
  return x * x * (3 - 2 * x);
}

/// The inverse of `smoothstep01`, by bisection.
///
/// Needed because the calibration has to answer "at what value of the ramp's argument does alpha
/// reach `COVERAGE_ALPHA`" exactly, and `3t^2 - 2t^3 = y` has no pleasant closed form. Forty
/// bisections on [0,1] is an absolute error below 1e-12, which is nine orders finer than the
/// 20,000-sample order statistic it is added to.
export function smoothstepInverse(y) {
  let lo = 0;
  let hi = 1;
  for (let i = 0; i < 40; i += 1) {
    const mid = (lo + hi) / 2;
    if (smoothstep01(mid) < y) lo = mid; else hi = mid;
  }
  return (lo + hi) / 2;
}

/// Cloud density in [0,1] from the index and a threshold. `0.5` exactly at `index === threshold`.
export function cloudDensity(index, threshold) {
  return smoothstep01((index - threshold + CLOUD_EDGE_W) / (2 * CLOUD_EDGE_W));
}

/// The index at which the alpha BYTE reaches `COVERAGE_ALPHA`, relative to the threshold.
///
/// `cloudAlpha` is `round(255 * CLOUD_ALPHA_MAX * density)`, so the byte reaches 128 as soon as
/// the exact value passes **127.5**, not 128 -- the boundary is half a least-significant bit
/// below where the obvious arithmetic puts it. Writing `COVERAGE_ALPHA` here instead of
/// `COVERAGE_ALPHA - 0.5` leaves a sliver of index in which the calibration believes a texel is
/// clear and the raster writes it covered, and it is the sort of thing that gets shrugged off
/// and then shows up as a coverage that misses its target. `clouds.test.mjs` walks the boundary
/// from both sides and would fail on either form; the half-LSB one is the form that passes.
///
/// The rest is the ramp inversion: the covered set is `density >= (COVERAGE_ALPHA - 0.5) /
/// (255 * CLOUD_ALPHA_MAX)`, which is `smoothstepInverse(...)` of the way through a ramp of
/// half-width `CLOUD_EDGE_W` centred on the threshold.
export const COVERAGE_INDEX_OFFSET = CLOUD_EDGE_W
  * (2 * smoothstepInverse((COVERAGE_ALPHA - 0.5) / (255 * CLOUD_ALPHA_MAX)) - 1);

// ============================================================================================
// Calibration -- the slider's travel, from the field's own distribution
// ============================================================================================

/// The default coverage, and it is the number the gap analysis names: the owner's reference
/// carries cloud over **roughly 40% of the disc**. Two decimals, on a slider whose step is 0.01,
/// so `panelFieldFaults()` can express it -- see `panel-fields.js` for the four times this
/// project has shipped a default a slider could not land on.
export const DEFAULT_CLOUD_COVER = 0.4;

/// The calibration population. 20,000 equal-area Fibonacci points gives an order-statistic
/// standard error of `sqrt(c(1-c)/n)` = **0.35 percentage points** at c = 0.4, which is far
/// finer than the eye's ability to read coverage off a globe and finer than the tile-population
/// disagreement documented in the report.
export const CLOUD_CALIBRATION_SAMPLES = 20000;

/// **The measurement the slider is calibrated from.**
///
/// Samples the index on an equal-area spiral, takes the order statistic at `1 - cover`, and
/// subtracts `COVERAGE_INDEX_OFFSET` so that the fraction of the SPHERE at or above
/// `COVERAGE_ALPHA` is `cover`. Returns a plain, structured-cloneable object -- it is posted to
/// every worker with each tile request rather than recomputed there, exactly as `biome.js`'s
/// calibration is, because 20,000 evaluations in each of eight workers is eight times the cost
/// for one number they must all agree on anyway.
///
/// Also returns `mean` and `sd` of the sampled index, which the report quotes and a test pins:
/// the whole reason this function exists is that a nominal range is not a measured one.
export function calibrateClouds({
  radiusM, cover = DEFAULT_CLOUD_COVER, seed = 0, samples = CLOUD_CALIBRATION_SAMPLES,
}) {
  if (!Number.isFinite(radiusM) || radiusM <= 0) {
    throw new Error(`calibrateClouds: radiusM must be a positive number, got ${radiusM}`);
  }
  if (!(cover >= 0 && cover <= 1)) {
    throw new Error(`calibrateClouds: cover must be in 0..1, got ${cover}`);
  }
  const salts = cloudSalts(seed);
  const values = new Float64Array(samples);
  let sum = 0;
  let sumSq = 0;
  for (let i = 0; i < samples; i += 1) {
    const { latitudeDeg, longitudeDeg } = fibonacciPoint(i, samples);
    const v = cloudIndex(latitudeDeg, longitudeDeg, radiusM, salts);
    values[i] = v;
    sum += v;
    sumSq += v * v;
  }
  const sorted = Array.from(values).sort((a, b) => a - b);
  const mean = sum / samples;
  const sd = Math.sqrt(Math.max(0, sumSq / samples - mean * mean));
  // `cover = 0` and `cover = 1` are the two ends the order statistic cannot express (there is no
  // "above the maximum"), so they are placed a full ramp beyond the observed extremes. A layer
  // asked for zero cloud must be transparent everywhere, not 1/20000 covered.
  const q = cover <= 0
    ? sorted[samples - 1] + 2 * CLOUD_EDGE_W
    : cover >= 1
      ? sorted[0] - 2 * CLOUD_EDGE_W
      : quantile(sorted, 1 - cover);
  return {
    radiusM,
    cover,
    seed: String(seed),
    samples,
    salts,
    mean,
    sd,
    threshold: q - COVERAGE_INDEX_OFFSET,
  };
}

/// The area-weighted coverage the calibration actually produces, re-measured on a fresh spiral.
///
/// Separate from `calibrateClouds` on purpose: a function that measured its own answer with the
/// array it had just sorted would agree with itself by construction. This one re-evaluates the
/// field and counts `alpha >= COVERAGE_ALPHA`, which is the same predicate a rendered texel is
/// judged by.
export function measuredCoverage(calibration, samples = CLOUD_CALIBRATION_SAMPLES) {
  let covered = 0;
  for (let i = 0; i < samples; i += 1) {
    const { latitudeDeg, longitudeDeg } = fibonacciPoint(i, samples);
    const v = cloudIndex(latitudeDeg, longitudeDeg, calibration.radiusM, calibration.salts);
    if (cloudAlpha(cloudDensity(v, calibration.threshold)) >= COVERAGE_ALPHA) covered += 1;
  }
  return covered / samples;
}

/// The same measurement, restricted to a latitude band. The banding assertion is made of these.
export function bandCoverage(calibration, southDeg, northDeg, samples = CLOUD_CALIBRATION_SAMPLES) {
  let covered = 0;
  let seen = 0;
  for (let i = 0; i < samples; i += 1) {
    const { latitudeDeg, longitudeDeg } = fibonacciPoint(i, samples);
    if (latitudeDeg < southDeg || latitudeDeg > northDeg) continue;
    seen += 1;
    const v = cloudIndex(latitudeDeg, longitudeDeg, calibration.radiusM, calibration.salts);
    if (cloudAlpha(cloudDensity(v, calibration.threshold)) >= COVERAGE_ALPHA) covered += 1;
  }
  return { covered, seen, fraction: seen > 0 ? covered / seen : NaN };
}

// ============================================================================================
// Colour
// ============================================================================================

/// The thin end and the thick end of a cloud, as RGB.
///
/// Thin cloud is not "white but faint": it is optically thin, so what reaches the eye is a mix of
/// scattered sunlight and the blue of the air above it, which reads cooler and darker. Thick
/// cloud is very nearly the brightest thing in the frame -- `biome.js` derives its palette from
/// visible-band reflectance and saturates at 0.5, and its own comment names cloud and fresh snow
/// as what sits there. These two are the ends of that: a reflectance around 0.28 and around 0.85.
export const CIRRUS_RGB = [206, 216, 228];
export const CUMULUS_RGB = [252, 253, 255];

/// Where along the density axis the colour finishes crossing from cirrus to cumulus. Below 0.2
/// is unambiguously wisp; above 0.7 is unambiguously deck.
export const CLOUD_TONE_LOW = 0.2;
export const CLOUD_TONE_HIGH = 0.7;

/// Density -> the byte written to the alpha channel.
export function cloudAlpha(density) {
  return Math.round(255 * CLOUD_ALPHA_MAX * clamp01(density));
}

/// Density -> RGB, lerping cirrus to cumulus.
export function cloudRgb(density) {
  const t = smoothstep01((density - CLOUD_TONE_LOW) / (CLOUD_TONE_HIGH - CLOUD_TONE_LOW));
  return [
    Math.round(CIRRUS_RGB[0] + (CUMULUS_RGB[0] - CIRRUS_RGB[0]) * t),
    Math.round(CIRRUS_RGB[1] + (CUMULUS_RGB[1] - CIRRUS_RGB[1]) * t),
    Math.round(CIRRUS_RGB[2] + (CUMULUS_RGB[2] - CIRRUS_RGB[2]) * t),
  ];
}

// ============================================================================================
// The raster
// ============================================================================================

/// Wrap a finished RGBA buffer as an `ImageData`, or as an ImageData-shaped plain object where
/// that global does not exist. Identical in intent to `relief.js`'s, and re-implemented here
/// rather than imported so that `clouds.js` has no dependency on the relief pipeline at all --
/// these are two independent layers and the file structure should say so.
export function makeCloudImageData(data, width, height) {
  if (typeof ImageData !== "undefined") return new ImageData(data, width, height);
  return { data, width, height };
}

/// Rasterise one cloud tile.
///
/// `rectangle` is `{ northDeg, southDeg, westDeg, eastDeg }` in plain degrees, the same shape
/// `relief.js` takes. `clouds` is a `calibrateClouds` result. **No engine, no world handle, no
/// margin grid** -- the field is a point function, so unlike the relief raster there are no
/// neighbours to fetch and nothing to differentiate.
export function cloudTile({ rectangle, level = null, size = CLOUD_TILE_SIZE, clouds }) {
  if (!clouds || !Number.isFinite(clouds.threshold)) {
    throw new Error("cloudTile: a calibrateClouds() result is required");
  }
  if (!Number.isInteger(size) || size < 2) {
    throw new Error(`cloudTile: size must be an integer >= 2, got ${size}`);
  }
  void level; // carried through the provider for symmetry; the field does not read it

  const { northDeg, southDeg, westDeg, eastDeg } = rectangle;
  const last = size - 1;
  const dLat = (southDeg - northDeg) / last;
  const dLon = (eastDeg - westDeg) / last;
  const { radiusM, salts, threshold } = clouds;

  const data = new Uint8ClampedArray(size * size * 4);
  for (let row = 0; row < size; row += 1) {
    const latDeg = northDeg + dLat * row;
    for (let col = 0; col < size; col += 1) {
      const lonDeg = westDeg + dLon * col;
      const density = cloudDensity(cloudIndex(latDeg, lonDeg, radiusM, salts), threshold);
      const [r, g, b] = cloudRgb(density);
      const idx = (row * size + col) * 4;
      data[idx] = r;
      data[idx + 1] = g;
      data[idx + 2] = b;
      data[idx + 3] = cloudAlpha(density);
    }
  }
  return makeCloudImageData(data, size, size);
}

// ============================================================================================
// The checks that must be able to fail
// ============================================================================================

/// Alpha-channel statistics over an ImageData-shaped raster.
///
/// **Alpha, not luminance, and that is the whole point.** `relief.js`'s `luminanceStats` reads
/// RGB; a cloud raster's RGB spans only cirrus-to-cumulus and a layer that returned alpha 0
/// everywhere would still show a perfectly healthy RGB histogram. Every structural claim about
/// this layer has to be made about the channel that decides whether anything is visible.
///
/// `rectangle` is optional; when given, each row is weighted by `cos(latitude)` so the figure is
/// an **area** fraction rather than a texel fraction. In a `GeographicTilingScheme` those two
/// differ by a lot near the poles, and the calibration's population is equal-area.
export function alphaStats(imageData, rectangle = null) {
  const { data, width, height } = imageData;
  const hist = new Array(256).fill(0);
  let weight = 0;
  let covered = 0;
  let opaque = 0;
  let clear = 0;
  let transition = 0;
  let sum = 0;
  let sumSq = 0;
  let min = 255;
  let max = 0;
  const north = rectangle ? rectangle.northDeg : 0;
  const dLat = rectangle && height > 1 ? (rectangle.southDeg - north) / (height - 1) : 0;
  for (let row = 0; row < height; row += 1) {
    const w = rectangle ? Math.max(0, Math.cos(((north + dLat * row) * Math.PI) / 180)) : 1;
    for (let col = 0; col < width; col += 1) {
      const a = data[(row * width + col) * 4 + 3];
      hist[a] += 1;
      weight += w;
      sum += w * a;
      sumSq += w * a * a;
      if (a < min) min = a;
      if (a > max) max = a;
      if (a >= COVERAGE_ALPHA) covered += w;
      if (a >= CLOUD_MAX_ALPHA_BYTE) opaque += w;
      if (a <= 5) clear += w;
      else if (a < COVERAGE_ALPHA) transition += w;
    }
  }
  const mean = weight > 0 ? sum / weight : 0;
  const variance = weight > 0 ? Math.max(0, sumSq / weight - mean * mean) : 0;
  return {
    n: width * height,
    weight,
    mean,
    stdDev: Math.sqrt(variance),
    min,
    max,
    distinctBins: hist.reduce((c, k) => (k > 0 ? c + 1 : c), 0),
    coverage: weight > 0 ? covered / weight : 0,
    opaqueFraction: weight > 0 ? opaque / weight : 0,
    clearFraction: weight > 0 ? clear / weight : 0,
    transitionFraction: weight > 0 ? transition / weight : 0,
    histogram: hist,
  };
}

/// **The check that must be able to fail.**
///
/// A cloud layer that returns alpha 0 at every texel looks like "light cloud cover" and passes
/// every visual inspection -- exactly as a flat-grey relief raster looked like "subtle shading"
/// one slice ago. So this refuses three distinct degenerate layers at once and says which:
///
/// - **coverage outside `[minCoverage, maxCoverage]`** -- transparent everywhere (0) and opaque
///   everywhere (1) are both refused, and so is a layer that has quietly drifted a long way from
///   what the slider asked for.
/// - **no structure** -- alpha standard deviation, range and distinct-bin count, which is what
///   refuses a constant *non-zero* alpha. A uniform 40%-grey veil has coverage 0 or 1 depending
///   on which side of `COVERAGE_ALPHA` it lands, and would otherwise slip through the band above.
/// - **no soft edge** -- a stated minimum of texels strictly inside the transition. A hard
///   `index > threshold` cutoff has coverage and structure and is still the single clearest tell
///   that this is a threshold on noise rather than weather.
///
/// Defaults are loose enough for any real tile at any coverage the slider offers and tight enough
/// to refuse all three degenerate cases -- `clouds.test.mjs` constructs each of them and checks
/// the refusal.
export function hasCloudStructure(imageData, {
  rectangle = null,
  minCoverage = 0.02,
  maxCoverage = 0.98,
  minStdDev = 8,
  minRange = 32,
  minDistinctBins = 8,
  minTransitionFraction = 0.02,
} = {}) {
  const stats = alphaStats(imageData, rectangle);
  const reasons = [];
  if (stats.coverage < minCoverage) reasons.push(`coverage ${stats.coverage.toFixed(4)} < ${minCoverage}`);
  if (stats.coverage > maxCoverage) reasons.push(`coverage ${stats.coverage.toFixed(4)} > ${maxCoverage}`);
  if (stats.stdDev < minStdDev) reasons.push(`alpha sd ${stats.stdDev.toFixed(2)} < ${minStdDev}`);
  if (stats.max - stats.min < minRange) reasons.push(`alpha range ${stats.max - stats.min} < ${minRange}`);
  if (stats.distinctBins < minDistinctBins) reasons.push(`${stats.distinctBins} distinct alpha bins < ${minDistinctBins}`);
  if (stats.transitionFraction < minTransitionFraction) {
    reasons.push(`transition ${stats.transitionFraction.toFixed(4)} < ${minTransitionFraction} -- hard edge`);
  }
  return { ok: reasons.length === 0, reasons, stats };
}
