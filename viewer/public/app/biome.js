//! Land colour: a biome classifier over three axes, a palette derived from surface
//! reflectance, and the breakup noise that stops the result reading as a choropleth.
//
// # What this file is for
//
// `relief.js` colours land from `LAND_BANDS` -- a six-stop height ramp running pale strand,
// green, ochre, grey. **Both ends of it are mid-tone**, so the whole of land occupies a
// narrow luminance band and the picture reads as a diagram of a planet rather than a
// photograph of one. Real satellite imagery of land does not look like that: a closed
// canopy is near-black in the visible bands and a sand sea is near-white, and the distance
// between those two is most of what makes a true-colour image legible as terrain.
//
// So this module answers a different question than the ramp does. The ramp asks "how high is
// this?"; this asks "what is *growing* here?", and answers it from three axes the viewer can
// evaluate at a point:
//
// 1. **landform** -- coastal / interior / montane, from the world's own land hypsometry;
// 2. **temperature** -- a latitude profile with a lapse rate, in degrees C;
// 3. **moisture** -- a general-circulation latitude profile plus noise, dimensionless.
//
// **There is no climate model here and this file does not pretend otherwise.** Temperature
// is a curve in latitude and height; moisture is a curve in latitude plus fractal noise.
// That is deliberate and it is not a shortcut relative to the reference implementations:
// WorldEngine's `precipitation.py`, the source whose look this is aimed at, is *simplex
// noise times a gamma curve of temperature* -- the rain shadow its manual advertises is not
// in that file. Latitude plus noise is what the reference gets too. What we add over it is
// the third axis: WorldEngine has no landform axis at all, so it cannot tell a tropical
// shore from a boreal one, and for a MUD those are different places.
//
// # Licensing: this is a re-derivation, not a vendored table
//
// **No colour in this file was copied from any other project.** WorldEngine is MIT and its
// 41-entry `_biome_satellite_colors` could have been vendored with its notice attached;
// it was not. Instead every entry below carries a **visible-band reflectance** -- a
// published physical property of the cover type -- and its RGB is *computed* from that by
// `deriveColor()` at module load. The palette is therefore reproducible from the table of
// reflectances and hues, and a test asserts that each entry's luminance is the one its
// reflectance implies. The same goes for the technique: the layer order, the "add noise to
// every threshold's input" trick and the FBM parameters are ideas, and ideas are what this
// borrows.
//
// # The gain, and why land spans 10:1
//
// A closed tropical canopy reflects about **0.035** of incident light in the 0.4-0.7 um
// band; chlorophyll absorbs red and blue hard, and the 0.10-0.15 figure usually quoted for
// forest albedo is *broadband* and mostly near-infrared, which a true-colour image does not
// see. A dry sand sea reflects about **0.38** in the same band. Fresh snow is above 0.8.
// So the visible-band contrast inside land alone is already better than 10:1 before any
// display transform.
//
// A satellite true-colour product is displayed with a linear gain that saturates on cloud
// and fresh snow -- i.e. reflectance ~0.5 maps to white. `VISIBLE_GAIN = 2` is exactly that
// gain, and it is the only display transform here: **no gamma.** A gamma below 1 would lift
// the shadows and *compress* the very ratio this file exists to restore; a gamma above 1
// would crush the forests to black. Linear it is, and the numbers fall out: rainforest
// lands at luminance 18, hot desert at 194, ice clipped at 250.
//
// # Band edges without a grid
//
// The bands are **quantiles of this world's own land**, which is how WorldEngine places its
// too -- but WorldEngine finds them by bisecting over a global masked array, and a global
// array is the one thing this project has ruled out. `continentality.rs::calibrate` already
// shows the grid-free form: a 4,000-point Fibonacci-spiral sample, sorted, read at an order
// statistic. That is engine-side and this task is viewer-only, so `calibrate()` below does
// the same trick in JS -- the same spiral, the same sample count, the same order statistic,
// over `wb_elevation_m` at each point. One pass at world load, no grid anywhere, and the
// result is a handful of per-world constants.
//
// **The quantiles themselves are bell-spaced, not evenly spaced.** WorldEngine's own comment
// on its humidity bands says why: *"originally evenly spaced at 12.5% each but changing them
// to a bell curve produced better results"* -- evenly spaced bands give a world of middling
// terrain and no real desert. The spacing here is derived rather than copied: the edges sit
// at the normal CDF of **equally spaced z-scores** (-1.5, -0.5, +0.5, +1.5), which is the
// bell that comment describes. It yields band widths 6.7 / 24.2 / 38.3 / 24.2 / 6.7 percent
// of land -- narrow tails, wide middle.
//
// A useful side effect: because a moisture or landform edge is a quantile of this world's
// own land, **those bands are reached on every world by construction.** Three colour blends
// have shipped in this viewer having never once been selected against this terrain; a
// quantile cannot repeat that mistake. **The temperature axis is deliberately not
// quantiled** -- see `TEMP_BAND_EDGES_C` for why an axis with a unit should keep it -- so
// its coverage is a property of the world rather than of the calibration, and
// `biome.test.mjs` measures the coverage of all three rather than assuming any of them.
//
// # Breakup noise, and why it matters as much as the palette
//
// A biome boundary evaluated on a bare threshold draws a clean curve, and forty colours
// separated by clean curves is a choropleth map -- more colours, same diagram. The fix is
// the one Azgaar's satellite renderer uses: precompute a few FBM fields and **add one of
// them to the input of every threshold in the file.** The boundary then frays at the scale
// of the noise instead of following the isoline.
//
// Three fields, at three wavelengths, all evaluated on the *unit sphere* rather than in
// lat/lon, so there is no seam at the antimeridian and no pinch at the poles:
//
// - `macro`, 3,000 km -- continental tonal variation; also multiplies the final colour.
// - `patch`, 500 km -- vegetation clumping; moves the moisture axis most.
// - `breakup`, 90 km -- the boundary dither itself.
//
// Wavelengths are **metres**, converted to sphere frequency with the world's own radius, so
// a smaller planet gets proportionally smaller biomes rather than the same picture scaled.
//
// On top of that, per-texel colour jitter of +-15 per channel, **land only** -- the thing
// that stops a flat region reading as vector art. WorldEngine draws that from a sequential
// RNG, which makes a texel's colour depend on the order tiles were drawn. Ours is a **point
// hash on the position, quantised to 20 m**, so it is the same value whichever tile asks and
// at whatever zoom level, down past level 11. That is an improvement on the source, not a
// compromise for it.
//
// # What this file deliberately does not do
//
// - **No ocean.** `relief.js` owns water colour and this returns nothing for `heightM <= 0`.
// - **No second rock/snow system.** `relief.js` already blends toward rock by slope and
//   toward snow by a latitude-dependent snowline, and both were fixed against this terrain
//   one slice ago. The `montane` landform band and the `ice` biome are classifier outcomes
//   feeding the *base* colour those blends then act on -- not a fourth elevation system.
// - **No clouds, no rivers, no erosion.** Other tasks.
//
// # Known limitation, stated rather than hidden
//
// `breakup`'s finest octave is about 2 km. Below roughly level 9 the FBM fields are locally
// constant and the only within-biome variation left is the 20 m colour jitter. That is a
// real ceiling on how much this layer contributes when zoomed right in; adding octaves is
// the fix and it costs eight hash evaluations each, per texel.

/// Rec. 709 luminance weights -- the same ones `relief.js::luminanceStats` measures with, so
/// a palette entry's derived luminance and the test's measured luminance are the same
/// quantity rather than two conventions that happen to be close.
export const LUMA = [0.2126, 0.7152, 0.0722];

export function luminance([r, g, b]) {
  return LUMA[0] * r + LUMA[1] * g + LUMA[2] * b;
}

/// The display gain of a true-colour satellite product: a linear stretch that saturates at
/// a visible-band reflectance of 0.5 (cloud, fresh snow). See the module doc -- this is the
/// only display transform, and there is no gamma on purpose.
export const VISIBLE_GAIN = 2;

/// Ice reflects above 0.8, which `VISIBLE_GAIN` would push past white. Clipping at 250
/// rather than 255 keeps a little headroom so the shading multiply in `relief.js` still has
/// somewhere to brighten *to* on a sunlit slope.
export const MAX_TARGET_LUM = 250;

function clamp01(x) {
  return x < 0 ? 0 : x > 1 ? 1 : x;
}

function clampByte(v) {
  const r = Math.round(v);
  return r < 0 ? 0 : r > 255 ? 255 : r;
}

/// Turn a reflectance and a hue ratio into an RGB triple whose Rec. 709 luminance is the one
/// the reflectance implies.
///
/// `ratio` is the *chromaticity* -- the relative channel strengths of the cover type, with
/// its largest component at 1 so the scale below is the only thing setting brightness. The
/// target luminance is `255 * VISIBLE_GAIN * reflectance`, clipped at `MAX_TARGET_LUM`, and
/// the scale is chosen to hit it exactly. Every entry in `BIOMES` is checked by the test to
/// land within a rounding unit of its target *and* to leave every channel under 255, because
/// a clipped channel would silently move the luminance away from the derivation.
export function deriveColor(reflectance, ratio) {
  const target = Math.min(MAX_TARGET_LUM, 255 * VISIBLE_GAIN * reflectance);
  const scale = target / luminance(ratio);
  return [clampByte(scale * ratio[0]), clampByte(scale * ratio[1]), clampByte(scale * ratio[2])];
}

/// The target luminance for a reflectance, exported so the test states the derivation in the
/// same arithmetic the palette used rather than restating the constants.
export function targetLuminance(reflectance) {
  return Math.min(MAX_TARGET_LUM, 255 * VISIBLE_GAIN * reflectance);
}

// ============================================================================================
// The palette
// ============================================================================================

/// Every land biome: a name, a visible-band reflectance, and a chromaticity.
///
/// **Reflectances are the physical population this palette is derived from** -- typical
/// 0.4-0.7 um values for the cover type, the band a true-colour image actually records.
/// They are *not* broadband albedos, which for vegetation are dominated by near-infrared and
/// would put a rainforest at three times the brightness a photograph of one shows.
///
/// The chromaticities are chosen here, by eye, from what each cover type looks like: a
/// conifer stand is blue-green, a deciduous stand is yellow-green, a sand sea is warm, a
/// polar desert is cold. They set hue only -- `deriveColor` sets brightness from the
/// reflectance, so no chromaticity choice can move an entry off its derived luminance.
const BIOME_TABLE = [
  // --- interior, polar and boreal ---------------------------------------------------------
  ["polar desert", 0.33, [0.94, 0.98, 1.00]],
  ["tundra", 0.13, [0.88, 1.00, 0.72]],
  ["wet tundra", 0.10, [0.78, 1.00, 0.66]],
  ["cold desert", 0.28, [1.00, 0.94, 0.76]],
  ["cold steppe", 0.165, [1.00, 0.96, 0.62]],
  ["taiga", 0.055, [0.52, 1.00, 0.58]],
  ["boreal wet forest", 0.045, [0.48, 1.00, 0.55]],
  // --- interior, temperate ----------------------------------------------------------------
  ["steppe", 0.175, [1.00, 0.95, 0.55]],
  ["grassland", 0.13, [0.82, 1.00, 0.48]],
  ["deciduous forest", 0.075, [0.62, 1.00, 0.40]],
  ["temperate rain forest", 0.045, [0.50, 1.00, 0.42]],
  // --- interior, warm ---------------------------------------------------------------------
  ["hot desert", 0.38, [1.00, 0.88, 0.66]],
  ["desert scrub", 0.205, [1.00, 0.90, 0.62]],
  ["dry woodland", 0.12, [0.84, 1.00, 0.52]],
  ["subtropical moist forest", 0.055, [0.55, 1.00, 0.36]],
  ["thorn scrub", 0.16, [1.00, 0.94, 0.56]],
  ["savanna", 0.145, [0.92, 1.00, 0.50]],
  ["tropical seasonal forest", 0.065, [0.60, 1.00, 0.35]],
  ["tropical rain forest", 0.035, [0.52, 1.00, 0.34]],
  // --- coastal ------------------------------------------------------------------------------
  ["polar shore", 0.28, [0.92, 0.96, 1.00]],
  ["cold shore", 0.24, [1.00, 0.99, 0.94]],
  ["temperate strand", 0.30, [1.00, 0.96, 0.84]],
  ["warm strand", 0.36, [1.00, 0.93, 0.76]],
  ["tropical strand", 0.42, [1.00, 0.96, 0.82]],
  ["cold marsh", 0.10, [0.74, 1.00, 0.65]],
  ["salt marsh", 0.09, [0.72, 1.00, 0.50]],
  ["mangrove", 0.042, [0.50, 1.00, 0.48]],
  // --- montane ------------------------------------------------------------------------------
  ["ice", 0.85, [0.97, 0.99, 1.00]],
  ["alpine tundra", 0.145, [0.90, 1.00, 0.80]],
  ["montane steppe", 0.17, [1.00, 0.94, 0.60]],
  ["montane conifer", 0.055, [0.50, 1.00, 0.56]],
  ["arid highland", 0.26, [1.00, 0.88, 0.68]],
  ["montane cloud forest", 0.048, [0.52, 1.00, 0.45]],
];

/// `BIOMES[i] = { id, name, reflectance, ratio, rgb, targetLum }`, colours derived at load.
export const BIOMES = BIOME_TABLE.map(([name, reflectance, ratio], id) => ({
  id,
  name,
  reflectance,
  ratio,
  rgb: deriveColor(reflectance, ratio),
  targetLum: targetLuminance(reflectance),
}));

/// `B.tropical_rain_forest` etc. -- names to ids, so the classifier reads as a sentence and a
/// typo is a `TypeError` at load rather than a wrong colour on a globe.
export const B = Object.fromEntries(
  BIOMES.map((b) => [b.name.replace(/ /g, "_"), b.id]),
);

// ============================================================================================
// Noise: a point hash, value noise on the unit sphere, and FBM over it
// ============================================================================================

/// A 32-bit integer avalanche over three lattice coordinates and a salt, returned in [0,1).
///
/// **Everything about the look of this layer that is not the palette comes through here**, so
/// it is written as a point function of its inputs and nothing else: no state, no sequence,
/// no dependence on the order tiles are drawn in. Two tiles that share an edge evaluate the
/// same lattice cell to the same bits.
export function hashUnit(ix, iy, iz, salt) {
  let h = (salt | 0) ^ Math.imul(ix | 0, 0x27d4eb2d);
  h = Math.imul(h ^ (h >>> 15), 0x85ebca6b);
  h ^= Math.imul(iy | 0, 0x165667b1);
  h = Math.imul(h ^ (h >>> 13), 0xc2b2ae35);
  h ^= Math.imul(iz | 0, 0x9e3779b1);
  h = Math.imul(h ^ (h >>> 16), 0x7feb352d);
  h ^= h >>> 15;
  return (h >>> 0) / 4294967296;
}

/// Hermite fade, the standard value-noise interpolant: zero first derivative at both ends,
/// so adjacent lattice cells join without a visible crease.
function fade(t) {
  return t * t * (3 - 2 * t);
}

/// Trilinear value noise in [0,1) at a point in R^3. Eight hashes per call.
export function valueNoise3(x, y, z, salt) {
  const xi = Math.floor(x);
  const yi = Math.floor(y);
  const zi = Math.floor(z);
  const tx = fade(x - xi);
  const ty = fade(y - yi);
  const tz = fade(z - zi);

  const c000 = hashUnit(xi, yi, zi, salt);
  const c100 = hashUnit(xi + 1, yi, zi, salt);
  const c010 = hashUnit(xi, yi + 1, zi, salt);
  const c110 = hashUnit(xi + 1, yi + 1, zi, salt);
  const c001 = hashUnit(xi, yi, zi + 1, salt);
  const c101 = hashUnit(xi + 1, yi, zi + 1, salt);
  const c011 = hashUnit(xi, yi + 1, zi + 1, salt);
  const c111 = hashUnit(xi + 1, yi + 1, zi + 1, salt);

  const x00 = c000 + (c100 - c000) * tx;
  const x10 = c010 + (c110 - c010) * tx;
  const x01 = c001 + (c101 - c001) * tx;
  const x11 = c011 + (c111 - c011) * tx;
  const y0 = x00 + (x10 - x00) * ty;
  const y1 = x01 + (x11 - x01) * ty;
  return y0 + (y1 - y0) * tz;
}

/// FBM gain and lacunarity.
///
/// `GAIN = 0.55` puts the spectrum a little above 1/f -- rougher than a pure pink field, which
/// is what a *dither* wants: a boundary fray needs its energy at the small scales or it just
/// translates the boundary instead of breaking it. `LACUNARITY = 2.13` is deliberately not 2:
/// an integer step lines every octave's lattice up on the same planes and leaves visible axis
/// -aligned structure, and an irrational-ish step scatters them. `OFFSET` shifts the sample
/// point between octaves for the same reason.
export const FBM_GAIN = 0.55;
export const FBM_LACUNARITY = 2.13;
const FBM_OFFSET = 17.7;

/// **The standard deviation of a trilinear value-noise FBM, measured rather than assumed.**
///
/// Over a 20,000-point Fibonacci sample of all three fields at the owner's radius, an FBM
/// normalised by the sum of its own amplitudes has sd 0.105 (macro 0.118, patch 0.106,
/// breakup 0.103) against a nominal range of +-0.5. Averaging eight lattice corners is what
/// costs the variance, and the loss is a factor of five.
///
/// Dividing it out means **every weight below is in standard deviations of its field**,
/// which is the only unit those weights can be reasoned about in. The first pass of this
/// file wrote them as if the field really did span +-0.5, so every noise term was five times
/// weaker than intended, moisture came out very nearly a function of latitude alone, and the
/// anti-diagonal of the classifier's table -- hot desert, savanna, thorn scrub, temperate
/// rain forest -- was never once selected. That is the dead-band defect this repository has
/// already shipped three times, caught here by measuring the coverage rather than by looking
/// at a screenshot.
export const FBM_SD = 0.105;

/// FBM with unit standard deviation (see `FBM_SD`), normalised by the sum of its own
/// amplitudes first so the result does not depend on the octave count -- which matters
/// because the three fields below use three different counts. Roughly normal, so about two
/// thirds of samples land within +-1 and the tails run to about +-3.5.
export function fbm3(x, y, z, octaves, salt) {
  let amp = 1;
  let sum = 0;
  let norm = 0;
  let px = x;
  let py = y;
  let pz = z;
  for (let o = 0; o < octaves; o += 1) {
    sum += amp * (valueNoise3(px, py, pz, salt + o * 7919) - 0.5);
    norm += amp;
    amp *= FBM_GAIN;
    px = px * FBM_LACUNARITY + FBM_OFFSET;
    py = py * FBM_LACUNARITY + FBM_OFFSET;
    pz = pz * FBM_LACUNARITY + FBM_OFFSET;
  }
  return sum / norm / FBM_SD;
}

/// The unit vector for a latitude/longitude in degrees. **The noise domain is the sphere, not
/// the lat/lon rectangle**, which is what removes the antimeridian seam and the polar pinch a
/// 2D field would have.
export function unitVector(latitudeDeg, longitudeDeg) {
  const la = (latitudeDeg * Math.PI) / 180;
  const lo = (longitudeDeg * Math.PI) / 180;
  const c = Math.cos(la);
  return [c * Math.cos(lo), c * Math.sin(lo), Math.sin(la)];
}

/// Field wavelengths in **metres on the ground**, and octave counts.
///
/// A wavelength in metres divided into the planet's radius is the frequency in unit-sphere
/// coordinates, because a great circle is `2*pi*radiusM` metres long and `2*pi` long on the
/// unit sphere. So `freq = radiusM / wavelengthM`, and a smaller planet gets proportionally
/// smaller biomes rather than the same picture at a different size.
///
/// Octave counts are the cost/detail trade named in the module doc: `macro` and `patch` are
/// smooth by nature and get three and four, `breakup` is the dither and gets six, reaching a
/// finest octave of about `90 km / 2.13^5 = 2.1 km`.
export const MACRO_WAVELENGTH_M = 3_000_000;
export const PATCH_WAVELENGTH_M = 500_000;
export const BREAKUP_WAVELENGTH_M = 90_000;
export const MACRO_OCTAVES = 3;
export const PATCH_OCTAVES = 4;
export const BREAKUP_OCTAVES = 6;

/// Distinct salts, so the three fields are independent rather than three views of one.
export const MACRO_SALT = 0x5eed01;
export const PATCH_SALT = 0x5eed02;
export const BREAKUP_SALT = 0x5eed03;
export const JITTER_SALT = 0x5eed04;

/// The quantum of the per-texel colour jitter, in metres. Chosen just under the level-12
/// texel spacing (19.1 m) so the grain is **the same value at every zoom level** down to
/// there: a hash of a quantised position cannot change with the tile that asks for it, which
/// is the property the source's sequential RNG does not have.
export const JITTER_QUANTUM_M = 20;
/// Per-channel jitter amplitude. `+-15` is the source's figure and it is a legibility choice,
/// not a physical one: it is the smallest amplitude that reads as texture rather than as
/// banding at 8 bits.
export const JITTER_AMPLITUDE = 15;

/// The three fields at a point, in [-0.5, 0.5].
export function noiseFields(px, py, pz, radiusM) {
  const fMacro = radiusM / MACRO_WAVELENGTH_M;
  const fPatch = radiusM / PATCH_WAVELENGTH_M;
  const fBreakup = radiusM / BREAKUP_WAVELENGTH_M;
  return {
    macro: fbm3(px * fMacro, py * fMacro, pz * fMacro, MACRO_OCTAVES, MACRO_SALT),
    patch: fbm3(px * fPatch, py * fPatch, pz * fPatch, PATCH_OCTAVES, PATCH_SALT),
    breakup: fbm3(px * fBreakup, py * fBreakup, pz * fBreakup, BREAKUP_OCTAVES, BREAKUP_SALT),
  };
}

/// Per-channel colour jitter in `[-JITTER_AMPLITUDE, +JITTER_AMPLITUDE]`, from a point hash on
/// the position quantised to `JITTER_QUANTUM_M`. Three independent channels, as the source
/// has -- a single scalar would move luminance only and read as noise on a photocopy.
export function colorJitter(px, py, pz, radiusM) {
  const q = radiusM / JITTER_QUANTUM_M;
  const ix = Math.round(px * q);
  const iy = Math.round(py * q);
  const iz = Math.round(pz * q);
  return [
    (hashUnit(ix, iy, iz, JITTER_SALT) - 0.5) * 2 * JITTER_AMPLITUDE,
    (hashUnit(ix, iy, iz, JITTER_SALT ^ 0x9e3779b9) - 0.5) * 2 * JITTER_AMPLITUDE,
    (hashUnit(ix, iy, iz, JITTER_SALT ^ 0x7f4a7c15) - 0.5) * 2 * JITTER_AMPLITUDE,
  ];
}

// ============================================================================================
// The two climate axes
// ============================================================================================

/// Mean annual surface temperature at sea level, equator and pole, in degrees C, and the
/// environmental lapse rate.
///
/// The profile between them is `cos(latitude)`, and it is **fitted, not assumed**. Earth's
/// zonal annual-mean surface temperature runs roughly 26 / 20 / 12 / 0 / -25 C at 0 / 30 /
/// 45 / 60 / 90 degrees; `-25 + 52*cos(lat)` gives 27 / 20.0 / 11.8 / 1.0 / -25 -- within
/// about two degrees everywhere. The `sin^2` form this file used first is the one usually
/// written down, and it is much worse in the subtropics: it puts 30 degrees at 14 C, which
/// is 6 C too cold and is enough to move every desert latitude out of the hot band and into
/// the temperate one. That was measured, not reasoned about: with `sin^2` the `hot desert`
/// cell of the classifier was never once selected on the owner's world.
///
/// `6.5 C/km` is the standard environmental lapse rate.
export const TEMP_EQUATOR_C = 27;
export const TEMP_POLE_C = -25;
export const LAPSE_C_PER_KM = 6.5;
/// How far the two noise fields move temperature, **in degrees C per standard deviation of
/// the field**.
///
/// `macro` is continentality standing in for itself -- interiors run hotter and colder than
/// coasts, and 4 C is about the size of that effect at mid-latitudes.
///
/// **`breakup` is sized against the band widths, not against the climate.** The absolute
/// temperature bands are 8, 10 and 6 C wide, so 2.5 C is between a quarter and a third of a
/// band: enough that a boundary wanders across tens of kilometres at the field's 90 km
/// wavelength, not so much that a band's own interior dissolves into its neighbours. It was
/// 1.2 C first, and the transect check in `biome.test.mjs` measured the result -- 46 climate
/// -band crossings along a parallel against 28 with no noise at all, a ratio of 1.6, which
/// is a boundary that has been nudged rather than frayed.
export const TEMP_MACRO_C = 4;
export const TEMP_BREAKUP_C = 2.5;

/// Mean annual temperature in degrees C at a point, before banding.
export function temperatureC(latitudeDeg, heightM, fields) {
  const base = TEMP_POLE_C + (TEMP_EQUATOR_C - TEMP_POLE_C)
    * Math.cos((latitudeDeg * Math.PI) / 180);
  const lapse = (LAPSE_C_PER_KM * Math.max(0, heightM)) / 1000;
  return base - lapse + fields.macro * TEMP_MACRO_C + fields.breakup * TEMP_BREAKUP_C;
}

/// The moisture profile's latitude terms: `[centreDeg, weight, sigmaDeg]`, summed onto
/// `MOISTURE_BASE` and mirrored across the equator.
///
/// This is the general circulation as every physical-geography text draws it, and nothing
/// more: rising air at the **ITCZ** (wet), the descending limb of the Hadley cell at about
/// **30 deg** (every major desert on Earth), the **polar front** at about 55 deg (wet), and
/// the **polar cell**'s descending air (dry). Four terms, symmetric, no seasons -- a
/// Koppen-style classification would need seasonality and there is none here to have.
export const MOISTURE_BASE = 0.45;
export const MOISTURE_TERMS = [
  [0, 0.45, 12],
  [30, -0.29, 12],
  [55, 0.25, 14],
  [80, -0.20, 15],
];
/// How far the three noise fields move moisture.
///
/// **These are large relative to the latitude terms above, and that is the point.** The
/// zonal mean is only part of Earth's precipitation variance: at 25 degrees north you find
/// both the Sahara and the monsoon coast of the Bay of Bengal, because the *longitudinal*
/// term -- distance from an ocean, which side of a range you are on, which way the gyre
/// turns -- is comparable to the latitudinal one. Nothing in the viewer can compute that
/// term, so noise stands in for it, at a weight that matches its real share.
///
/// It was also measured. With the latitude terms at full strength, moisture was nearly a
/// function of latitude, so moisture band and temperature band were nearly the same
/// variable and **the anti-diagonal of the classifier's table was never selected** -- nine
/// of thirty-three colours were dead on the owner's world, which is precisely the defect
/// this viewer has already shipped three times. `patch` carries the largest continental
/// share: within-band clumping is what makes a forest a mosaic of stands rather than a flat
/// wash.
///
/// `MOISTURE_BREAKUP` is sized the same way `TEMP_BREAKUP_C` is: the calibrated moisture
/// bands come out 0.24 to 0.37 wide on the owner's world, so 0.075 is between a fifth and a
/// third of a band -- a boundary that frays, not one that dissolves.
export const MOISTURE_MACRO = 0.10;
export const MOISTURE_PATCH = 0.08;
export const MOISTURE_BREAKUP = 0.075;

/// **The temperature axis is absolute, and it is the only one of the three that is.**
///
/// Moisture here is a dimensionless index with no unit and no anchor, so the only meaning a
/// moisture band can have is "drier than this fraction of this world's land" -- a quantile.
/// Temperature is not like that. It is in degrees C, from a profile fitted to Earth's zonal
/// means and a standard lapse rate, and the boundaries that matter to vegetation are
/// *physical*: water freezes at 0, the boreal/temperate transition sits near an 8 C annual
/// mean, and 18 C is Koppen's own A/C boundary. Holdridge -- the model WorldEngine says it
/// implements -- puts its temperature axis in degrees too, for the same reason; WorldEngine
/// quantiles it only because its temperature layer is unitless noise, and ours is not.
///
/// This was also measured. With temperature quantiled, the subtropical desert latitudes of
/// the owner's world (25 degrees, 22 C) fell into the *median* temperature band, so the
/// classifier's `hot desert` cell -- the brightest land colour in the palette at luminance
/// 194 -- was never selected, and the desert belt came out as `cold desert` at 143. Fixing
/// the axis rather than the table is what put it back.
///
/// The consequence to be honest about: unlike the quantiled axes, **an absolute band can go
/// unvisited on a world whose land is all in one climate**, and that is correct rather than
/// dead -- a world with no polar land should have no tundra. `calibrate` therefore reports
/// the observed temperature span so a check can say which bands this world can reach.
export const TEMP_BAND_EDGES_C = [0, 8, 18, 24];

/// A dimensionless moisture index at a point, before banding.
///
/// **Not clamped.** The bands are quantiles of this quantity over the world's own land, and
/// clamping would pile ties on 0 and 1 and move the tail quantiles onto the clamp rather than
/// onto the terrain.
export function moistureIndex(latitudeDeg, fields) {
  const lat = Math.abs(latitudeDeg);
  let m = MOISTURE_BASE;
  for (const [centre, weight, sigma] of MOISTURE_TERMS) {
    const d = (lat - centre) / sigma;
    m += weight * Math.exp(-0.5 * d * d);
  }
  return m + fields.macro * MOISTURE_MACRO + fields.patch * MOISTURE_PATCH
    + fields.breakup * MOISTURE_BREAKUP;
}

// ============================================================================================
// The engine's own climate -- what this module's two approximations above are a stand-in for
// ============================================================================================

/// **The raster the climate grid is filled at, and it was chosen by measurement rather than
/// by taste.**
///
/// One climate sample is one upwind moisture march, and one march at the engine's canonical
/// budget is 161 `elevation_m` queries. Texels are QUADRATIC in the raster edge and samples
/// are LINEAR in the budget, so the raster is the only lever there is. Priced against this
/// world's own elevation cost (a 160-sample query at 560-770 us; see the task report for the
/// host and the population):
///
/// | raster | per tile | over 49 tiles | on the relief layer |
/// |---|---|---|---|
/// | **16 x 16 (shipped)** | 181-250 ms | 8.9-12.2 s | +33-45% |
/// | 32 x 32 | 647-890 ms | 31.7-43.6 s | +116-160% |
///
/// 32 would make climate the largest single item in the tile budget, for a field whose
/// spatial frequency is set by a 3,200 km march and cannot carry that detail. **The lever is
/// fewer or cheaper samples and never more workers**: the pool is starved rather than
/// saturated -- 23-40% utilisation, and sixteen workers settled *slower* than eight.
export const CLIMATE_RASTER = 16;

/// The band edges, in the shape `biomeAt` reads, taken from the ENGINE's calibration rather
/// than from `calibrate` below.
///
/// `climate` is `engine.climateCalibration`'s return value. The temperature edges are still
/// `TEMP_BAND_EDGES_C` and still absolute -- the engine bands temperature at the same four
/// degrees, for the reason that constant's own doc gives -- and the moisture and landform
/// edges are this world's own quantiles as the engine measured them, over a march rather than
/// over noise.
///
/// `engine: true` is read by `biomeAt` and is not decoration: on this path a per-texel
/// `climate` argument is REQUIRED, and a caller that forgot it would otherwise silently get
/// the noise field it was asked to replace.
export function engineCalibration({ radiusM, climate }) {
  if (!Number.isFinite(radiusM) || radiusM <= 0) {
    throw new Error(`engineCalibration: radiusM must be a positive number, got ${radiusM}`);
  }
  if (!climate || !Array.isArray(climate.moistureEdges) || !Array.isArray(climate.landformEdges)) {
    throw new Error("engineCalibration: climate must be engine.climateCalibration's payload");
  }
  return {
    engine: true,
    radiusM,
    samples: CALIBRATION_SAMPLES,
    landSamples: climate.landSamples,
    tempEdges: TEMP_BAND_EDGES_C.slice(),
    /// **Not reported on this path**, and that is deliberate rather than an omission: the
    /// engine's temperature is a closed form and its span over a world is a question about
    /// that world's hypsometry, which nothing here samples. `calibrate` reports one because
    /// it already had the 4,000 samples in hand.
    tempSpanC: null,
    moistEdges: climate.moistureEdges.slice(),
    landformEdges: climate.landformEdges.slice(),
    lapseCPerKm: climate.lapseCPerKm,
  };
}

/// Where sample `(row, col)` of a `size`-texel raster sits in a `CLIMATE_RASTER`-cell climate
/// grid laid over the same texel lattice, and the four-cell bilinear read of it.
///
/// **The two grids share their endpoints exactly.** `relief.js` puts texel `(0, 0)` at the
/// tile's north-west corner and texel `(size-1, size-1)` at `north + dLat*(size-1)`,
/// `west + dLon*(size-1)`; the climate grid is filled over precisely those bounds with both
/// endpoints included, so cell 0 IS texel 0 and cell `CLIMATE_RASTER-1` IS texel `size-1`.
/// There is no half-texel offset to get wrong, and that is why the provider passes the texel
/// lattice's own bounds rather than the tile rectangle.
///
/// Returns `{ datumC, moisture }`.
export function sampleClimateGrid(grid, gridSize, row, col, size) {
  const last = size - 1;
  const gLast = gridSize - 1;
  const t = last > 0 ? (row * gLast) / last : 0;
  const u = last > 0 ? (col * gLast) / last : 0;
  const r0 = Math.min(gLast, Math.floor(t));
  const c0 = Math.min(gLast, Math.floor(u));
  const r1 = Math.min(gLast, r0 + 1);
  const c1 = Math.min(gLast, c0 + 1);
  const fr = t - r0;
  const fc = u - c0;
  const at = (r, c, channel) => grid[((r * gridSize + c) * 2) + channel];
  const lerp2 = (channel) => {
    const top = at(r0, c0, channel) + (at(r0, c1, channel) - at(r0, c0, channel)) * fc;
    const bottom = at(r1, c0, channel) + (at(r1, c1, channel) - at(r1, c0, channel)) * fc;
    return top + (bottom - top) * fr;
  };
  return { datumC: lerp2(0), moisture: lerp2(1) };
}

/// Mean annual temperature in degrees C from the ENGINE's datum profile, with the lapse rate
/// applied here at the relief raster's own resolution.
///
/// **The lapse is applied on this side on purpose.** The climate grid is 16 x 16 over a tile
/// the relief layer draws at 256; a temperature sampled coarsely and interpolated would carry
/// a lapse term smoothed over 1/16 of the tile, so a peak inside one climate cell would come
/// back at its neighbourhood's mean height and lose its snow line entirely. The engine writes
/// the datum value for exactly this reason and reports `lapseCPerKm` alongside the edges, so
/// there is no second copy of `6.5` here.
///
/// **The two noise terms stay.** `climate.rs`'s own module doc says it is *the mean field* and
/// that a rendering term over it "stays where it is"; the temperature bands are 6 to 10 C wide
/// and `TEMP_BREAKUP_C` is 2.5, so it still frays a boundary rather than dissolving a band --
/// which is the sizing argument that constant was measured against and it is unchanged by
/// where the mean came from.
export function engineTemperatureC(datumC, heightM, lapseCPerKm, fields) {
  const lapse = (lapseCPerKm * Math.max(0, heightM)) / 1000;
  return datumC - lapse + fields.macro * TEMP_MACRO_C + fields.breakup * TEMP_BREAKUP_C;
}

/// **The engine's moisture carries no noise, and that is the one place this task deletes a
/// term rather than replacing it.**
///
/// `MOISTURE_MACRO` and `MOISTURE_PATCH` exist for a reason stated in their own doc: *"the
/// longitudinal term -- distance from an ocean, which side of a range you are on -- ... Nothing
/// in the viewer can compute that term, so noise stands in for it."* The engine now computes
/// it. Keeping the stand-in on top of the real thing would be adding noise to a measurement.
///
/// `MOISTURE_BREAKUP` goes too, and that is a sizing argument rather than a purity one. It was
/// sized as "a fifth to a third of a band" against bands 0.24 to 0.37 wide in the noise field.
/// The engine's field is heavily skewed dry -- Task 3 measured 44-68% of all land inside the
/// bottom fifth of its VALUE range -- so this world's own band edges come out at 0.022 / 0.119
/// / 0.408 / 0.759, i.e. an `arid` band 0.022 wide against a `perhumid` band 0.24 wide. One
/// additive breakup amplitude cannot be a third of both: 0.075 would erase the dry end
/// outright, and an amplitude small enough for the dry end would be invisible at the wet one.
/// Fraying it correctly needs the noise applied in QUANTILE space, which needs the sorted
/// sample the engine does not export. **Reported, not invented.**
///
/// What replaces it is not nothing: the march is an integral over real terrain, so its
/// isolines already follow coasts and ranges, where the noise model's isolines were literally
/// parallels of latitude. The transect measurement in the task report is what decides that,
/// not this paragraph.
export function engineMoisture(moisture) {
  return moisture;
}

// ============================================================================================
// Calibration: quantiles over a Fibonacci sample, no grid
// ============================================================================================

/// The same sample count `continentality.rs::calibrate` uses. It is not arbitrary there and
/// it is not arbitrary here: the spiral is area-uniform, so an order statistic over 4,000
/// points has a standard error of well under a percentile, which is finer than the band
/// edges need.
export const CALIBRATION_SAMPLES = 4000;

/// The golden angle, the increment that makes the spiral area-uniform.
const GOLDEN_ANGLE_DEG = 180 * (3 - Math.sqrt(5));

/// The `i`-th of `n` points of a Fibonacci spiral, as `{ latitudeDeg, longitudeDeg }`.
///
/// `z` is stepped through the *cell centres* (`(2i+1)/n`), not the edges, so neither pole is
/// sampled twice and the first and last points sit half a step in from +-90.
export function fibonacciPoint(i, n) {
  const z = 1 - (2 * i + 1) / n;
  const latitudeDeg = (Math.asin(z) * 180) / Math.PI;
  let longitudeDeg = (i * GOLDEN_ANGLE_DEG) % 360;
  if (longitudeDeg > 180) longitudeDeg -= 360;
  return { latitudeDeg, longitudeDeg };
}

/// Band edges as normal-CDF quantiles of equally spaced z-scores. See the module doc: this is
/// the bell spacing WorldEngine's comment describes, derived instead of transcribed.
///
/// z = -1.5, -0.5, +0.5, +1.5 -> 0.0668, 0.3085, 0.6915, 0.9332, i.e. band widths of
/// 6.7 / 24.2 / 38.3 / 24.2 / 6.7 percent of land.
export const BELL_Z = [-1.5, -0.5, 0.5, 1.5];

/// Abramowitz & Stegun 26.2.17 for the standard normal CDF: max absolute error 7.5e-8, which
/// is six orders of magnitude finer than the sample resolution these quantiles are read at.
export function normalCdf(z) {
  const t = 1 / (1 + 0.2316419 * Math.abs(z));
  const d = 0.3989422804014327 * Math.exp(-0.5 * z * z);
  const p = d * t * (0.319381530 + t * (-0.356563782 + t * (1.781477937
    + t * (-1.821255978 + t * 1.330274429))));
  return z >= 0 ? 1 - p : p;
}

export const BELL_QUANTILES = BELL_Z.map(normalCdf);

/// Landform edges as quantiles of the world's own land elevation.
///
/// **Placed against Earth's land hypsometry, not picked**: roughly a quarter of Earth's land
/// is coastal plain, and mountain terrain -- the ground that carries bare rock, alpine
/// vegetation and permanent snow -- is the top 15% or so. Reading them as quantiles rather
/// than as metres is what stops the family of dead bands this viewer has shipped three of:
/// a quantile is reached on every world by construction, however low that world's mountains.
export const LANDFORM_QUANTILES = [0.25, 0.85];

/// The `q`-th order statistic of a sorted array, linearly interpolated.
export function quantile(sorted, q) {
  if (sorted.length === 0) return NaN;
  if (sorted.length === 1) return sorted[0];
  const pos = clamp01(q) * (sorted.length - 1);
  const lo = Math.floor(pos);
  const hi = Math.ceil(pos);
  if (lo === hi) return sorted[lo];
  return sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo);
}

/// **The grid-free half.** One pass over a Fibonacci spiral at world load; the engine is asked
/// for an elevation at each point and nothing else, and the temperature, moisture and
/// landform band edges come back as per-world constants.
///
/// Returns `{ radiusM, samples, landSamples, tempEdges, moistEdges, landformEdges }`, a plain
/// object -- deliberately structured-cloneable, because it is posted to every relief worker
/// rather than recomputed there. Four thousand `wb_elevation_m` calls in each of four workers
/// would be four times the cost for exactly the same four numbers.
export function calibrate({ engine, worldHandle, radiusM, samples = CALIBRATION_SAMPLES }) {
  if (!engine || typeof engine.elevationM !== "function") {
    throw new Error("calibrate: engine.elevationM is required");
  }
  if (!Number.isFinite(radiusM) || radiusM <= 0) {
    throw new Error(`calibrate: radiusM must be a positive number, got ${radiusM}`);
  }
  const temps = [];
  const moists = [];
  const heights = [];
  for (let i = 0; i < samples; i += 1) {
    const { latitudeDeg, longitudeDeg } = fibonacciPoint(i, samples);
    const heightM = engine.elevationM(worldHandle, latitudeDeg, longitudeDeg);
    // Land only. The bands describe what grows on land; including the sea floor would drag
    // every quantile down into ground no biome is ever asked about.
    if (!(heightM > 0)) continue;
    const [px, py, pz] = unitVector(latitudeDeg, longitudeDeg);
    const fields = noiseFields(px, py, pz, radiusM);
    temps.push(temperatureC(latitudeDeg, heightM, fields));
    moists.push(moistureIndex(latitudeDeg, fields));
    heights.push(heightM);
  }
  temps.sort((a, b) => a - b);
  moists.sort((a, b) => a - b);
  heights.sort((a, b) => a - b);
  return {
    radiusM,
    samples,
    landSamples: temps.length,
    /// Absolute, not calibrated -- see `TEMP_BAND_EDGES_C`. Carried on this object anyway so
    /// every axis is read from one place by the code that bands them.
    tempEdges: TEMP_BAND_EDGES_C.slice(),
    /// The observed range of land temperature on this world, so a report can say which of
    /// the absolute bands this world's land can reach at all.
    tempSpanC: [quantile(temps, 0), quantile(temps, 1)],
    moistEdges: BELL_QUANTILES.map((q) => quantile(moists, q)),
    landformEdges: LANDFORM_QUANTILES.map((q) => quantile(heights, q)),
  };
}

/// Which band `x` falls in, given ascending edges. `edges.length + 1` outcomes.
export function bandIndex(edges, x) {
  let i = 0;
  while (i < edges.length && x >= edges[i]) i += 1;
  return i;
}

// ============================================================================================
// The classifier: landform x temperature x moisture
// ============================================================================================

/// How far the noise fields move the *landform* threshold, in metres.
///
/// The landform axis is the one whose boundary is most obviously a contour line if left bare
/// -- it is a threshold on elevation, so its isoline is literally a contour. These two terms
/// are what fray it. They are a fraction of the band widths this generator produces (its
/// land spans roughly 0 to 2,000 m), not of Earth's.
export const LANDFORM_BREAKUP_M = 90;
export const LANDFORM_PATCH_M = 60;

export const LANDFORM_COASTAL = 0;
export const LANDFORM_INTERIOR = 1;
export const LANDFORM_MONTANE = 2;

/// The interior table, `[temperature][moisture]`, coldest and driest first.
///
/// It is a full 5x5 rectangle on purpose. WorldEngine's collapses its cold rows -- polar has
/// two outcomes, alpine four -- which is defensible when temperature is the only thing
/// separating them, but here the montane band has already taken the genuinely cold-and-high
/// ground away, so a polar *lowland* still has a meaningful wet end (wet tundra and mire) and
/// it should not be folded into the dry one.
const INTERIOR = [
  // polar
  [B.polar_desert, B.polar_desert, B.tundra, B.tundra, B.wet_tundra],
  // boreal
  [B.cold_desert, B.cold_steppe, B.taiga, B.taiga, B.boreal_wet_forest],
  // temperate
  [B.cold_desert, B.steppe, B.grassland, B.deciduous_forest, B.temperate_rain_forest],
  // subtropical
  [B.hot_desert, B.desert_scrub, B.dry_woodland, B.subtropical_moist_forest,
    B.subtropical_moist_forest],
  // tropical
  [B.hot_desert, B.thorn_scrub, B.savanna, B.tropical_seasonal_forest, B.tropical_rain_forest],
];

/// The coastal row by temperature, dry end. A shore's *substrate* is what shows when the
/// vegetation does not: gravel and till where it is cold, quartz sand where it is temperate,
/// carbonate sand where it is warm.
const COASTAL_DRY = [
  B.polar_shore, B.cold_shore, B.temperate_strand, B.warm_strand, B.tropical_strand,
];
/// The coastal row by temperature, wet end -- a wet shore grows something, and what it grows
/// is strongly temperature-dependent. **This is the axis WorldEngine cannot express**: it has
/// no landform axis, so it has one answer for a wet tropical lowland whether or not it is on
/// the coast, and "tropical coastal" and "boreal coastal" are the same place to it.
const COASTAL_WET = [
  B.cold_marsh, B.cold_marsh, B.salt_marsh, B.mangrove, B.mangrove,
];
/// Moisture band at or above which a shore is wet rather than bare.
export const COASTAL_WET_BAND = 3;

/// The montane row: `[dry, wet]` by temperature. Cold and high is ice whatever the moisture;
/// warm and high is an arid highland when dry and a cloud forest when wet, which is the pair
/// every tropical mountain shows on opposite flanks.
const MONTANE_DRY = [
  B.ice, B.alpine_tundra, B.montane_steppe, B.arid_highland, B.arid_highland,
];
const MONTANE_WET = [
  B.ice, B.alpine_tundra, B.montane_conifer, B.montane_conifer, B.montane_cloud_forest,
];
/// Moisture band at or above which montane ground is wet rather than dry.
export const MONTANE_WET_BAND = 2;

/// **Three axes, one lookup.** `landform`, `temp` and `moist` are band indices.
export function classify(landform, temp, moist) {
  if (landform === LANDFORM_COASTAL) {
    return moist >= COASTAL_WET_BAND ? COASTAL_WET[temp] : COASTAL_DRY[temp];
  }
  if (landform === LANDFORM_MONTANE) {
    return moist >= MONTANE_WET_BAND ? MONTANE_WET[temp] : MONTANE_DRY[temp];
  }
  return INTERIOR[temp][moist];
}

/// How hard `macro` modulates the finished colour, as a multiplier. Small: this is the
/// "the whole of that continent is a little drier this year" term, and it should read as
/// light and season rather than as a second palette.
export const MACRO_TONE = 0.07;

/// **The function `relief.js` calls.** Everything above, composed, for one texel.
///
/// Returns `{ rgb, biome, landform, temp, moist, tempC, moisture, fields }` -- the extra
/// fields are what the test asserts band coverage over, and what a future ambient-occlusion
/// or cloud pass would reuse rather than recompute.
export function biomeAt({ heightM, latitudeDeg, longitudeDeg, calibration, climate = null }) {
  const { radiusM, tempEdges, moistEdges, landformEdges } = calibration;
  const [px, py, pz] = unitVector(latitudeDeg, longitudeDeg);
  const fields = noiseFields(px, py, pz, radiusM);

  // **Two paths, one palette.** `calibration.engine` selects the engine's measured climate;
  // anything else is the noise approximation this module shipped first, kept byte-for-byte so
  // `?climate=0` is a real A/B rather than a claim. The palette, the classifier, the jitter
  // and the tone are shared -- this task replaces the INPUTS.
  //
  // The two are not merged behind a null-coalescing default, deliberately: a caller that
  // built an engine calibration and forgot to pass the per-texel `climate` would then get the
  // noise field silently, against the engine's own band edges, which is a wrong picture that
  // looks entirely plausible. It throws instead.
  if (calibration.engine && !climate) {
    throw new Error("biomeAt: an engine calibration needs a per-texel climate sample");
  }
  const tempC = climate
    ? engineTemperatureC(climate.datumC, heightM, calibration.lapseCPerKm, fields)
    : temperatureC(latitudeDeg, heightM, fields);
  const moisture = climate
    ? engineMoisture(climate.moisture)
    : moistureIndex(latitudeDeg, fields);
  // **Every threshold's input carries a noise term.** For temperature and moisture the noise
  // is already inside the field itself; the landform threshold is a bare elevation
  // comparison, so it gets its own here.
  const heightEff = heightM + fields.breakup * LANDFORM_BREAKUP_M
    + fields.patch * LANDFORM_PATCH_M;

  const temp = bandIndex(tempEdges, tempC);
  const moist = bandIndex(moistEdges, moisture);
  const landform = bandIndex(landformEdges, heightEff);
  const biome = classify(landform, temp, moist);

  const base = BIOMES[biome].rgb;
  const tone = 1 + fields.macro * MACRO_TONE;
  const jitter = colorJitter(px, py, pz, radiusM);
  const rgb = [
    base[0] * tone + jitter[0],
    base[1] * tone + jitter[1],
    base[2] * tone + jitter[2],
  ];
  return { rgb, biome, landform, temp, moist, tempC, moisture, fields };
}

/// The colour alone, which is all `relief.js` wants per texel.
export function biomeColor(args) {
  return biomeAt(args).rgb;
}
