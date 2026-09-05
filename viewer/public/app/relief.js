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
// # Slope colour
//
// A height-only ramp cannot put rock on a steep face at low altitude -- it is given height
// and nothing else. So land colour is height-banded (as the existing `ElevationRamp` canvas
// in `main.js` already is) and then blended toward bare rock by slope angle, and toward
// snow by height above a snowline. **The engine's `detail.rs` roughness constants
// (`ABYSSAL_M` 55, `SHELF_M` 15, `COAST_M` 35, `INTERIOR_M` 80, `MOUNTAIN_M` 150) are
// amplitudes of per-octave noise, not slope-angle thresholds, so they do not convert into
// `ROCK_SLOPE_LOW_DEG`/`ROCK_SLOPE_HIGH_DEG` directly** -- there is no unit-preserving
// formula from "150 m of roughness at the detail scale" to "42 degrees". They are used only
// as a sanity check that mountainous terrain in this generator does in fact produce slopes
// in the chosen range at this raster's post spacing (see the report). The slope thresholds
// themselves are a physically-motivated but ultimately aesthetic choice: 22 deg is close to
// the angle of repose for loose soil/scree (below it, ground plausibly holds vegetation);
// 42 deg is within the range usually cited for scree/talus slopes and exposed rock faces.
// This is a look, not a conformance surface, exactly as the brief says.

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

/// Slope-angle band, in degrees, over which land colour blends from its height band toward
/// bare rock. See the module doc for what these are and are not derived from.
export const ROCK_SLOPE_LOW_DEG = 22;
export const ROCK_SLOPE_HIGH_DEG = 42;
export const ROCK_COLOR = [120, 112, 100];

/// Height band, in metres above the datum, over which land colour blends toward snow.
export const SNOW_LINE_M = 3500;
export const SNOW_BAND_M = 1200;
export const SNOW_COLOR = [246, 248, 250];

/// Hypsometric bands, height (m) -> RGB. Chosen to read the same way as the existing
/// `elevationRamp()` canvas in `main.js` (abyssal near-black, basin blue, pale shelf, strand,
/// lowland green, upland ochre) so the relief layer and the fallback material agree when
/// both are visible, without literally sharing code -- one is a 256x1 canvas gradient
/// consumed by a Cesium material, the other is a per-texel table consumed here, and forcing
/// them through one function would coupled two things that change for different reasons.
const OCEAN_BANDS = [
  [-9000, [2, 10, 20]],
  [-4000, [4, 24, 46]],
  [-1200, [10, 51, 88]],
  [-200, [20, 84, 140]],
  [-20, [47, 134, 189]],
  [0, [126, 197, 223]],
];

const LAND_BANDS = [
  [0, [221, 207, 168]],
  [50, [143, 154, 94]],
  [600, [74, 122, 60]],
  [1800, [125, 113, 80]],
  [3200, [163, 150, 120]],
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

/// Height + slope colour, before shading. Slope only ever moves land toward rock or snow;
/// underwater colour is height-only, because "rock on a steep face" is a subaerial idea and
/// this generator does not model underwater sediment angle of repose.
export function slopeColor(heightM, slopeDeg) {
  const color = baseColor(heightM);
  if (heightM <= 0) return color;
  const rockT = smoothstep(ROCK_SLOPE_LOW_DEG, ROCK_SLOPE_HIGH_DEG, slopeDeg);
  const withRock = rockT > 0 ? lerpColor(color, ROCK_COLOR, rockT) : color;
  const snowT = smoothstep(SNOW_LINE_M, SNOW_LINE_M + SNOW_BAND_M, heightM);
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
  resolutionM = null, sun = DEFAULT_SUN, ambient = AMBIENT,
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

      let nx = -dzdEast;
      let ny = -dzdNorth;
      let nz = 1;
      const nlen = Math.sqrt(nx * nx + ny * ny + nz * nz);
      // nlen is never 0 (nz is always 1 before normalising), so no NaN guard is needed here
      // -- the one place NaN could enter is a non-finite height, and Object.is/Number
      // comparisons below would already show it as a visible artefact rather than a false
      // "flat" shade.
      nx /= nlen; ny /= nlen; nz /= nlen;

      const dot = nx * sun.east + ny * sun.north + nz * sun.up;
      const shade = ambient + (1 - ambient) * Math.max(0, dot);

      const slopeRad = Math.atan(Math.sqrt(dzdEast * dzdEast + dzdNorth * dzdNorth));
      const slopeDeg = (slopeRad * 180) / Math.PI;

      const [r, gr, b] = slopeColor(hHere, slopeDeg);

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
