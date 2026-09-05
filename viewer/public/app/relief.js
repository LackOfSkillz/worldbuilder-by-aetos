//! A relief raster for one tile: hillshade times a height-and-slope colour.
//
// **Why this exists at all**: `CustomHeightmapTerrainProvider` hands back
// `HeightmapTerrainData`, which has no vertex-normal path anywhere in Cesium --
// `hasVertexNormals` is `false`, always, on that class. `GlobeFS` falls back to
// `czm_geodeticSurfaceNormal`, the *ellipsoid* normal, so the mesh is lit as a perfect
// smooth sphere no matter what `enableLighting` says. A raster is the only surface in this
// stack that can carry a normal, so this file is what puts a mountain range's shadow where
// the mountain range is.
//
// **Main thread, one tile, no provider yet.** This module knows nothing about Cesium: it
// takes a plain `{ northDeg, southDeg, westDeg, eastDeg }` rectangle, not a
// `Cesium.Rectangle`, and returns `ImageData` (or an ImageData-shaped plain object when no
// `ImageData` global exists, e.g. under `node --test`). Task 3 is what wraps this in an
// `ImageryProvider`; Task 4 is what moves it into a worker. Nothing here reaches for either.
//
// # Sampling: an (n+2)^2 grid, so there is no seam at the tile edge
//
// A hillshade needs a normal at every output texel, and a normal needs a neighbour on each
// side. Computing the edge row/column of a tile from a *one-sided* difference (because
// there is no neighbour tile to ask) is a different, biased estimator of slope than the
// two-sided one used everywhere else in that tile -- and it is different again from
// whatever the *next* tile does at the matching edge, because the two tiles' one-sided
// differences point in opposite directions. That mismatch is a visible grid line across the
// whole globe, and it is reportedly the first thing anyone notices.
//
// The fix is the one used here: request a grid one texel wider and taller than the output
// on every side (`size + 2`), so every one of the `size x size` output texels has real
// engine-sampled neighbours on all four sides, and use central differences throughout. No
// output texel is ever a one-sided estimate. (This still leaves the *next* tile's margin
// texels as independently-sampled -- not shared buffers -- but because both tiles ask the
// engine for the same analytic field at the same coordinates, the margin samples agree
// bit-for-bit with the neighbour's own interior samples at that latitude/longitude. The
// seam this prevents is the one-sided-difference bias, not a hypothetical sampling
// mismatch.)
//
// # Post convention
//
// Posts are **edge-inclusive**, the same convention `wb_fill_tile_f32`'s `grid_coordinate`
// uses and `terrain.js`'s `postLatLonDeg` documents: post `i` of `n` sits at
// `a + (b - a) * i / (n - 1)`. The `size`-post *interior* row spans the tile's own north and
// south edges exactly (posts `0` and `size - 1`); the margin adds one extra post-spacing
// step of extrapolation on each side, so the full request is `size + 2` posts wide/tall,
// still edge-inclusive over its own (larger) span. This is deliberately the mesh's own post
// grid, not a pixel-centre raster grid, so a `size` equal to the terrain heightmap's own
// post count reproduces the same lat/lon lattice the mesh uses.
//
// # Sun direction
//
// **Chosen: a fixed direction in the local east-north-up frame, azimuth 315 deg (from the
// north-west) at 45 deg altitude** -- `DEFAULT_SUN_AZIMUTH_DEG` / `DEFAULT_SUN_ALTITUDE_DEG`
// below. This is the default every desktop GIS hillshade tool ships (ArcGIS, QGIS,
// `gdaldem hillshade`), so it is a known-good, already-validated choice rather than a guess.
// It is a *local* convention, not a single light source fixed in the planet's ECEF frame:
// every tile is lit from its own north-west regardless of where it sits on the globe, so
// there is no terminator and no globally coherent shadow direction. That is a real
// simplification -- this project has no simulated day/night cycle for a single fixed ECEF
// sun to model in the first place, so a per-tile cartographic convention costs nothing
// (only a lat/lon-independent constant vector, vs. rotating a global direction into each
// texel's local frame) and is the one every 2D relief map already uses. Deriving a sun
// direction from Cesium's actual `scene.light`/`sunPosition` was the other option on the
// table and is deferred, not ruled out: it would couple this pure function to live scene
// state, which Task 1 explicitly does not need yet.
//
// # The z-factor, and why without one this whole file was a 19% grey wash
//
// Task 3 shipped this raster and then measured that it does nothing: rendering each tile
// with and without shading and taking the per-texel ratio gave a shade factor of mean 0.808
// with sd 0.004, flat from level 5 to level 12. 0.8096 is exactly `AMBIENT + (1 - AMBIENT) *
// sin(45 deg)` -- the shade of *perfectly flat ground*. Every texel was flat.
//
// **The cause is measured, not guessed.** At this raster's own post spacing, the land slopes
// this generator produces are a fraction of a degree: at the highest-relief inland tile on
// `DEFAULT_WORLD` the median land slope is 0.21 deg at level 5 and 1.19 deg at level 12, and
// the steepest texel found anywhere in the probe set is 1.9 deg. A hillshade's response to a
// 1-degree slope is a 1-degree tilt of the normal, which is under half a luminance unit.
// That is not a bug in the shading; the planet really is that smooth. Its highest point is
// 1,381 m on a 6,371 km sphere -- about a sixth of Earth's relief.
//
// Every desktop hillshade tool carries a **z-factor** (vertical exaggeration) for exactly
// this reason; in GIS it is nominally a unit conversion, and it is used as an exaggeration
// just as often. `Z_FACTOR` below multiplies the two gradients before the normal is built.
// **This is a legibility choice and not a realism one, and it is the largest single lie this
// file tells**: at level 12 it renders a median 1.2-degree slope as a 27-degree one. A MUD's
// world map is read, not admired, and an unreadable honest picture loses to a readable
// exaggerated one -- but the exaggeration is named, constant, and reported per level rather
// than buried.
//
// It is one constant across every level on purpose. A level-dependent z would shade two
// adjacent tiles differently whenever the quadtree straddles a level -- which it does
// constantly during a descent -- and that is a seam. What *does* legitimately change with
// level is the terrain: finer sampling resolves steeper local faces, so the shading gets
// stronger and more detailed as you zoom. That is the answer to "when I zoom in it just
// looks blurry", not a defect to normalise away.
//
// # Slope colour
//
// A height-only ramp cannot put rock on a steep face at low altitude -- it is given height
// and nothing else. So land colour is height-banded and then blended toward bare rock by
// slope angle, and toward snow by height, latitude and shelter.
//
// **All three of those blends were dead code before this task, and the same measurement
// killed all three.** `ROCK_SLOPE_LOW_DEG` was 22 deg against a terrain whose steepest texel
// is 1.9 deg; `SNOW_LINE_M` was 3,500 m against a planet whose highest point is 1,381 m; and
// `LAND_BANDS`' top two stops (1,800 m and 3,200 m) sat above the 99.9th percentile of land
// elevation. The layer was a two-band green-and-ochre ramp wearing the vocabulary of a
// slope-aware one. The bands below are placed on the **measured** hypsometry of this
// generator (see the report for the population), and the rock threshold is placed on the
// **z-exaggerated** slope, which is the surface this file actually draws.
//
// What real satellite imagery does at the three transitions, and what is matched here:
//
// - **Water/land.** A calm sea surface is flat and its albedo does not vary with the seabed
//   under it; ocean colour in a true-colour image is depth *scattering*, not relief. So the
//   hillshade is switched off below the datum and the sea is lit as the flat plane it is,
//   while the depth bands stay. This also stops seabed ridges from reading as land.
// - **Vegetated/bare.** Bare rock is exposed where the slope exceeds what a soil mantle can
//   hold -- the angle of repose, ~30-37 deg for loose material -- which is why mountain
//   photographs show grey faces and green valleys at the *same* altitude. The band here is
//   15-38 deg on the exaggerated surface. It is a minority accent at coarse levels and a
//   real texture close in, and it is reported per level rather than claimed to be stable.
// - **Bare/snow.** The snowline is not a contour. It falls with latitude (roughly 4,900 m in
//   the tropics, ~2,100 m at 45 deg, sea level in the high Arctic) and snow is *shed*
//   from steep faces, which is why alpine peaks read as mottled rock-and-white rather than a
//   white cap with a hard rim. Both are modelled: a latitude-dependent line, and a shelter
//   term that is one minus the rock fraction. A pure elevation threshold would draw a
//   contour ring and read as a bug, which is what the brief asked to avoid.
//
// Not modelled, and visible: no continentality or precipitation in the snowline (a desert
// mountain and a maritime one get the same line), no sea ice, no vegetation zonation beyond
// altitude, no clouds.

import { metresPerDegree } from "./terrain.js";

/// Hillshade default, matching every mainstream GIS tool's default (see the sun-direction
/// note above).
export const DEFAULT_SUN_AZIMUTH_DEG = 315;
export const DEFAULT_SUN_ALTITUDE_DEG = 45;

/// A unit vector in the local east/north/up frame pointing *toward* the sun.
export function sunDirectionEnu(azimuthDeg = DEFAULT_SUN_AZIMUTH_DEG, altitudeDeg = DEFAULT_SUN_ALTITUDE_DEG) {
  const az = (azimuthDeg * Math.PI) / 180;
  const alt = (altitudeDeg * Math.PI) / 180;
  const cosAlt = Math.cos(alt);
  return {
    east: cosAlt * Math.sin(az),
    north: cosAlt * Math.cos(az),
    up: Math.sin(alt),
  };
}

export const DEFAULT_SUN = sunDirectionEnu();

/// Below this shade fraction the ground is never fully black -- real relief maps keep a
/// visible ambient floor rather than a pure cast-shadow silhouette, and a slope facing away
/// from the sun still needs to read as *something*, not as an unshaded-vs-shaded binary.
export const AMBIENT = 0.35;

/// **Vertical exaggeration, applied to both gradients before the normal is built.** See the
/// module doc: without it this file measured a shade factor of 0.808 +- 0.004 -- a constant
/// 19% darkening -- because this generator's land slopes at raster spacing are 0.2 to 1.9
/// degrees. Chosen from the sweep in the report: 25 is the smallest value that puts Task 2's
/// `hasStructure` (luminance sd >= 2) at three times its threshold at *every* level from 2 to
/// 12 on both land probes; 20 clears the threshold but only by 2.6x at level 11. It saturates
/// nothing: no texel on any probe tile at any level reaches the ambient floor, so nothing is
/// crushed to black and the exaggeration could be raised later without a cliff.
export const Z_FACTOR = 25;

/// Slope-angle band, in degrees **of the z-exaggerated surface**, over which land colour
/// blends from its height band toward bare rock. Placed against the measured exaggerated
/// slope distribution, not against the true one: on the true surface nothing on this planet
/// is steeper than 1.9 degrees, which is why the previous 22-42 band never once fired.
export const ROCK_SLOPE_LOW_DEG = 15;
export const ROCK_SLOPE_HIGH_DEG = 38;
export const ROCK_COLOR = [126, 118, 106];

/// The snowline, as a function of latitude rather than a single contour.
///
/// Earth's regional snowline runs about 4,900 m in the tropics and reaches sea level in the
/// high Arctic around 78-80 degrees; this is the straight line through those two ends. It is
/// a coarse approximation on purpose -- there is no climate model here to do better with,
/// and a single global elevation threshold is the thing being avoided, not the thing being
/// refined. On this generator (highest point 1,381 m) it puts no snow at all in the tropics,
/// which is correct: nothing there is tall enough.
///
/// **Checked against what it produces, not only against its ends.** A 0.5-degree global scan
/// of `DEFAULT_WORLD` (259,200 samples, area-weighted by cos(latitude)) puts 7.4% of land
/// fully above its own snowline and a further 3.4% inside the blend band. Earth's permanent
/// ice cover is about 10% of land area, so this lands where it was aimed rather than
/// painting half the planet white -- which the first pair of ends (zero at 72 degrees) did,
/// at 30% full plus 10% partial.
export const SNOW_LINE_EQUATOR_M = 4900;
export const SNOW_LINE_ZERO_LAT_DEG = 80;
/// Metres of height over which the snow blend completes, once the line is crossed.
export const SNOW_BAND_M = 260;
export const SNOW_COLOR = [246, 248, 250];

/// Snowline height, in metres above the datum, at a latitude.
export function snowLineM(latitudeDeg) {
  const t = clamp01(Math.abs(latitudeDeg) / SNOW_LINE_ZERO_LAT_DEG);
  return SNOW_LINE_EQUATOR_M * (1 - t);
}

/// Hypsometric bands, height (m) -> RGB. Chosen to read the same way as the existing
/// `elevationRamp()` canvas in `main.js` (abyssal near-black, basin blue, pale shelf, strand,
/// lowland green, upland ochre) so the relief layer and the fallback material agree when
/// both are visible, without literally sharing code -- one is a 256x1 canvas gradient
/// consumed by a Cesium material, the other is a per-texel table consumed here, and forcing
/// them through one function would coupled two things that change for different reasons.
/// **Every stop is a height this generator actually reaches**, which the previous table's
/// top two were not. Measured over three worlds (seed 20260904 / 7 / 424242) by a 4,170,724
/// -sample global fill at canonical resolution: land runs p50 421-619 m, p90 709-733 m,
/// p99 973-1,314 m, p99.9 1,507-1,643 m, max 1,645-2,051 m; the sea floor runs p10 -4,610 m,
/// p50 -3,261 to -4,264 m, min -6,345 to -6,807 m. The old table's 1,800 m and 3,200 m land
/// stops and its -9,000 m ocean stop were all outside that, so three of eleven colours in
/// this file were unreachable.
export const OCEAN_BANDS = [
  [-6800, [2, 10, 20]],
  [-4600, [4, 24, 46]],
  [-1200, [10, 51, 88]],
  [-200, [20, 84, 140]],
  [-20, [47, 134, 189]],
  [0, [126, 197, 223]],
];

export const LAND_BANDS = [
  [0, [221, 207, 168]],
  [40, [120, 142, 84]],
  [380, [74, 122, 60]],
  [700, [110, 116, 72]],
  [1000, [148, 132, 96]],
  [1500, [176, 168, 152]],
];

function clamp01(x) {
  return x < 0 ? 0 : x > 1 ? 1 : x;
}

/// Hermite smoothstep between `lo` and `hi`, 0 at and below `lo`, 1 at and above `hi`.
function smoothstep(lo, hi, x) {
  if (lo === hi) return x < lo ? 0 : 1;
  const t = clamp01((x - lo) / (hi - lo));
  return t * t * (3 - 2 * t);
}

function lerp(a, b, t) {
  return a + (b - a) * t;
}

function lerpColor(a, b, t) {
  return [lerp(a[0], b[0], t), lerp(a[1], b[1], t), lerp(a[2], b[2], t)];
}

/// Look up `x` in a sorted `[threshold, [r,g,b]]` table and linearly interpolate between the
/// straddling bands. Clamps to the end bands outside the table's range.
function bandColor(bands, x) {
  if (x <= bands[0][0]) return bands[0][1];
  const last = bands[bands.length - 1];
  if (x >= last[0]) return last[1];
  for (let i = 1; i < bands.length; i += 1) {
    const [hi, hiColor] = bands[i];
    if (x <= hi) {
      const [lo, loColor] = bands[i - 1];
      const t = (x - lo) / (hi - lo);
      return lerpColor(loColor, hiColor, t);
    }
  }
  return last[1];
}

/// Base colour by height alone -- no slope, no shading. Exported because Task 2's
/// discrimination check and Task 5's reference comparison both want to reason about the
/// height-only baseline this layer improves on.
export function baseColor(heightM) {
  return heightM <= 0 ? bandColor(OCEAN_BANDS, heightM) : bandColor(LAND_BANDS, heightM);
}

/// Height + slope + latitude colour, before shading.
///
/// `slopeDeg` is the slope of the **z-exaggerated** surface -- the one this file shades and
/// therefore the one a reader sees. Passing the true slope here is what made the rock band
/// dead code.
///
/// Slope only ever moves *land*: underwater colour is height-only, because "rock on a steep
/// face" is a subaerial idea and this generator does not model underwater sediment angle of
/// repose.
///
/// The snow term is deliberately **not** a pure elevation threshold. It is gated on the
/// latitude-dependent snowline and then multiplied by `1 - rockT`, the shelter term: a face
/// steep enough to read as bare rock is a face snow slides off. Together those two turn what
/// would be a contour ring into a mottled cap that follows the terrain, which is what a
/// photograph of a snowy range looks like.
export function slopeColor(heightM, slopeDeg, latitudeDeg = 0) {
  const color = baseColor(heightM);
  if (heightM <= 0) return color;
  const rockT = smoothstep(ROCK_SLOPE_LOW_DEG, ROCK_SLOPE_HIGH_DEG, slopeDeg);
  const withRock = rockT > 0 ? lerpColor(color, ROCK_COLOR, rockT) : color;
  const line = snowLineM(latitudeDeg);
  const snowT = smoothstep(line, line + SNOW_BAND_M, heightM) * (1 - rockT);
  return snowT > 0 ? lerpColor(withRock, SNOW_COLOR, snowT) : withRock;
}

/// Build the (n+2)^2 sample request for one tile: `size` interior posts edge-inclusive over
/// the tile's own rectangle, extended by one post-spacing step on every side. Exposed
/// separately from `reliefTile` so a caller (or a test) can inspect exactly what will be
/// asked of the engine without paying for a fill.
export function marginedTileRequest({ rectangle, size, worldHandle, radiusM, resolutionM = null }) {
  if (!Number.isInteger(size) || size < 2) {
    throw new Error(`marginedTileRequest: size must be an integer >= 2, got ${size}`);
  }
  const { northDeg, southDeg, westDeg, eastDeg } = rectangle;
  const last = size - 1;
  const dLatStep = (southDeg - northDeg) / last; // degrees per post, south-going (<=0 north>south)
  const dLonStep = (eastDeg - westDeg) / last; // degrees per post, east-going (>=0 west<east)

  const grid = size + 2;
  const lat0Deg = northDeg - dLatStep; // one step north of the tile's own north edge
  const lat1Deg = northDeg + dLatStep * size; // one step south of the tile's own south edge
  const lon0Deg = westDeg - dLonStep; // one step west of the tile's own west edge
  const lon1Deg = westDeg + dLonStep * size; // one step east of the tile's own east edge

  const metresPerLatDeg = metresPerDegree(radiusM);
  const rowStepM = Math.abs(dLatStep) * metresPerLatDeg;

  return {
    handle: worldHandle,
    lat0Deg, lat1Deg, lon0Deg, lon1Deg,
    width: grid, height: grid,
    // Default: this raster's own post spacing, which is finer than the terrain mesh's --
    // that gap is the entire point (detail below the mesh, carried by shading instead of
    // geometry). A caller may still pass a specific resolutionM (e.g. to match the mesh's
    // spacing, for an aliasing comparison).
    resolutionM: resolutionM ?? rowStepM,
    // Handed back so the caller doesn't recompute what this function already derived.
    dLatStep, dLonStep, rowStepM, metresPerLatDeg, grid,
  };
}

function makeImageData(data, size) {
  if (typeof ImageData !== "undefined") {
    return new ImageData(data, size, size);
  }
  // node --test and any other non-browser host: same shape, no browser global required.
  return { data, width: size, height: size };
}

/// The pure function this file exists to provide.
///
/// `rectangle` is `{ northDeg, southDeg, westDeg, eastDeg }` -- plain degrees, not a
/// `Cesium.Rectangle`; `level` is accepted but not otherwise used yet (kept in the
/// signature per the brief, for Task 3's `ImageryProvider` to pass through unchanged).
/// `engine` needs `fillTileF32`; `worldHandle` is the handle from `engine.newWorld(...)`.
export function reliefTile({
  rectangle, level = null, size = 256, engine, worldHandle, radiusM,
  resolutionM = null, sun = DEFAULT_SUN, ambient = AMBIENT, zFactor = Z_FACTOR,
}) {
  if (!engine || typeof engine.fillTileF32 !== "function") {
    throw new Error("reliefTile: engine.fillTileF32 is required");
  }
  if (!Number.isFinite(radiusM) || radiusM <= 0) {
    throw new Error(`reliefTile: radiusM must be a positive number, got ${radiusM}`);
  }
  void level; // reserved for the provider layer; this function does not need it

  const request = marginedTileRequest({ rectangle, size, worldHandle, radiusM, resolutionM });
  const { grid, dLatStep, dLonStep, rowStepM, metresPerLatDeg } = request;
  const heights = engine.fillTileF32(request);

  const { northDeg } = rectangle;
  const data = new Uint8ClampedArray(size * size * 4);

  for (let row = 0; row < size; row += 1) {
    const g = row + 1; // this row's index in the (size+2) margined grid
    const latDeg = northDeg + dLatStep * row;
    const colStepM = Math.abs(dLonStep) * metresPerLatDeg * Math.cos((latDeg * Math.PI) / 180);
    for (let col = 0; col < size; col += 1) {
      const gc = col + 1;
      const hHere = heights[g * grid + gc];
      const hNorth = heights[(g - 1) * grid + gc];
      const hSouth = heights[(g + 1) * grid + gc];
      const hWest = heights[g * grid + (gc - 1)];
      const hEast = heights[g * grid + (gc + 1)];

      // Central differences: every output texel, edge or interior, has real neighbours on
      // both sides, because the request grid is margined by one post all around.
      const dzdNorth = rowStepM > 0 ? (hNorth - hSouth) / (2 * rowStepM) : 0;
      const dzdEast = colStepM > 0 ? (hEast - hWest) / (2 * colStepM) : 0;

      // The z-factor is applied HERE, once, to the two gradients -- so the normal, the shade
      // and the slope angle the colour reads all describe one and the same exaggerated
      // surface. Exaggerating the shading but colouring from the true slope would put a rock
      // face's shadow on ground the colour still calls a meadow.
      const exEast = dzdEast * zFactor;
      const exNorth = dzdNorth * zFactor;

      let nx = -exEast;
      let ny = -exNorth;
      let nz = 1;
      const nlen = Math.sqrt(nx * nx + ny * ny + nz * nz);
      // nlen is never 0 (nz is always 1 before normalising), so no NaN guard is needed here
      // -- the one place NaN could enter is a non-finite height, and Object.is/Number
      // comparisons below would already show it as a visible artefact rather than a false
      // "flat" shade.
      nx /= nlen; ny /= nlen; nz /= nlen;

      // **Water is lit as the flat plane it is.** A sea surface does not carry the seabed's
      // relief, and shading the seabed through it made ocean ridges read as land. `sun.up`
      // is exactly the dot product of the sun direction with a vertical normal, so this is
      // the same expression evaluated at zero slope rather than a second lighting model.
      const dot = hHere <= 0 ? sun.up : nx * sun.east + ny * sun.north + nz * sun.up;
      const shade = ambient + (1 - ambient) * Math.max(0, dot);

      const slopeRad = Math.atan(Math.sqrt(exEast * exEast + exNorth * exNorth));
      const slopeDeg = (slopeRad * 180) / Math.PI;

      const [r, gr, b] = slopeColor(hHere, slopeDeg, latDeg);

      const idx = (row * size + col) * 4;
      data[idx] = r * shade;
      data[idx + 1] = gr * shade;
      data[idx + 2] = b * shade;
      data[idx + 3] = 255;
    }
  }

  return makeImageData(data, size);
}

/// Luminance (Rec. 709 weights) mean/spread/histogram over an ImageData-shaped raster.
/// Exported so Task 2's verifier and this file's own tests share one measurement.
export function luminanceStats(imageData) {
  const { data, width, height } = imageData;
  const n = width * height;
  const hist = new Array(256).fill(0);
  let sum = 0;
  let sumSq = 0;
  let min = 255;
  let max = 0;
  for (let i = 0; i < n; i += 1) {
    const o = i * 4;
    const lum = Math.round(0.2126 * data[o] + 0.7152 * data[o + 1] + 0.0722 * data[o + 2]);
    hist[lum] += 1;
    sum += lum;
    sumSq += lum * lum;
    if (lum < min) min = lum;
    if (lum > max) max = lum;
  }
  const mean = sum / n;
  const variance = Math.max(0, sumSq / n - mean * mean);
  const stdDev = Math.sqrt(variance);
  const distinctBins = hist.reduce((count, c) => (c > 0 ? count + 1 : count), 0);
  return { mean, stdDev, min, max, distinctBins, n, histogram: hist };
}

/// **The check that must be able to fail.** A relief layer returning flat grey looks like
/// "subtle shading" and passes every purely-visual inspection; this is the falsifiable
/// version. Defaults are loose enough to pass any tile with genuine terrain and tight
/// enough to refuse a constant image (see relief.test.mjs, which does exactly that and
/// checks the refusal).
export function hasStructure(imageData, { minStdDev = 2, minRange = 8, minDistinctBins = 4 } = {}) {
  const stats = luminanceStats(imageData);
  const ok = stats.stdDev >= minStdDev && stats.max - stats.min >= minRange && stats.distinctBins >= minDistinctBins;
  return { ok, stats, minStdDev, minRange, minDistinctBins };
}
