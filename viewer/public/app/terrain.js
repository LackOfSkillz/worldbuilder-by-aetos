//! The terrain provider: one callback, no subclass.
//
// `CustomHeightmapTerrainProvider` exists for exactly this -- a procedural source. It takes
// a callback returning a typed array of heights (or a promise for one) and wraps it in a
// `HeightmapTerrainData` itself. **No provider class is written here at all**, and none
// should be: a subclass would have to re-implement `getLevelMaximumGeometricError` and the
// `HeightmapTerrainData` construction identically.
//
// Three Cesium facts this file depends on, each checked against the vendored 1.145.0
// source rather than remembered:
//
// - **There is no `ready` or `readyPromise`.** Both were removed in 1.107. A provider is
//   usable the instant it is constructed, so nothing here is async.
// - **`getEstimatedLevelZeroGeometricErrorForAHeightmap` is already applied.** The
//   constructor calls it with the ellipsoid, `max(width, height)` and the level-0 X tile
//   count, and `getLevelMaximumGeometricError(level)` is that over `1 << level`. Using the
//   class *is* using the function; re-deriving it would be a second, drifting copy.
// - **`HeightmapTerrainData` with a `Float32Array` and the default `structure` reads the
//   values as metres above the ellipsoid, directly** -- `heightScale` 1, `heightOffset` 0,
//   `stride` 1. No packing, no scaling, no offset.
//
// # Row order
//
// Row 0 of the buffer is the **north** edge. This is not a convention picked here; it is
// what `HeightmapTerrainData.interpolateHeight` does -- it computes `fromSouth`, then
// flips with `southInteger = height - 1 - southInteger`. So the fill is handed the
// rectangle's north latitude as `lat0Deg`. Getting it backwards produces a planet that
// renders perfectly and is upside down, which is why `FAULTS.flipLatitude` exists below.
//
// # The zoom cap
//
// **`getTileDataAvailable` returning `undefined` is the trap.** The prototype returns
// `undefined`, and in `GlobeSurfaceTile.prepareNewTile` an `undefined` answer falls through
// to `terrainData.isChildAvailable(...)`, which for a `HeightmapTerrainData` with the
// default child tile mask is always true. Refinement is then bounded only by screen-space
// error, and `getLevelMaximumGeometricError` halves every level, so a camera near the
// ground refines until the tab dies.
//
// Returning `false` bounds it, through a path worth naming because it is not obvious:
// `prepareNewTile` sets the tile's terrain state to FAILED, the state machine upsamples it
// from its parent, `tile.upsampledFromParent` becomes true, and
// `QuadtreePrimitive.visitTile` stops refining a tile whose four children are *all*
// upsampled. That is the real gate -- `GlobeSurfaceTileProvider.canRefine` is not, since it
// returns `childAvailable !== undefined`, which is true for `false`.
//
// ## Why the cap is level 12, in metres
//
// A `GeographicTilingScheme` puts 2x1 tiles at level 0, so a tile spans `180 / 2^level`
// degrees and a 65-post heightmap samples it every `180 / (2^level * 64)` degrees. On this
// project's world radius (6,371,000 m) one degree is `pi * R / 180` = 111,194.93 m, so
//
//     post spacing = 312,735.73 m / 2^level        (= pi * 6,371,000 / 64)
//
//     level 10 -> 305.4 m     level 12 -> 76.35 m
//     level 11 -> 152.7 m     level 13 -> 38.18 m
//
// This line read **312,735.98 m** from `24e4216` through `4595f5e`, seven commits. The
// error is 25 cm at level 0 and vanishes by level 3, so the tabulated levels were right all
// along; `postSpacingM` computes the figure and never read this comment. It is corrected
// here rather than only in the README because this is the file it originated in: the README
// was fixed first and claimed the line had read that way "until now", which was true of the
// record and not of the source. A correction that reaches the write-up and not the code
// leaves the next reader to find the wrong number in the place they will actually look.
//
// The generated field has a **measured resolution floor of 78.125 m**: octaves fade in by
// `smooth((lambda/r - 2) / 2)` and reach full strength at `r <= lambda/4`, and on a 2 km
// transect the peak-to-peak relief rises monotonically from 0.19 m at r = 20,000 to 5.58 m
// at r = 78.125 and is then **bit-identical** for r = 50, r = 25 and canonical. Below about
// 100 m the field is a tilted plane -- 4.5 cm of chord deviation over 100 m.
//
// **Level 12 is the first level whose post spacing is at or below that floor** (76.35 m),
// so it is the last level at which zooming reveals any generated ground that was not
// already there. Level 13 would quadruple the tile count to draw the same plane.
//
// **This cap is about generated ground only, and Task 5 no longer applies it to anything
// else.** `Features::apply` is analytic and outside the octave schedule, so an authored
// feature is resolution-independent point-wise -- the extraction's harbour reads exactly
// +4.00 m at its centre at every resolution tried -- but a heightmap is a grid, and Task 4
// measured the level-12 tile containing that harbour topping out at **-819.4 m against a
// +4 m target**, reaching +3.2 m only at level 16. `MAX_LEVEL` is therefore the *ground*
// cap; `availability.js` refines past it inside a feature's footprint and nowhere else, and
// `maxLevel` stays a constructor option and a `?maxLevel=` URL parameter so the claim above
// stays testable.
import { POOL_FAULTS } from "./pool.js";

export const MAX_LEVEL = 12;

/// 65 x 65 posts. The standard Cesium heightmap tile size, and the one every cost figure in
/// this slice was measured at (median 3.86 ms, p90 18.20 ms per fill in Chrome 151).
export const HEIGHTMAP_SIZE = 65;

/// Metres per degree of latitude on a sphere of `radiusM`.
export function metresPerDegree(radiusM) {
  return (Math.PI * radiusM) / 180;
}

/// The post spacing, in metres, of a `size`-post heightmap over a level-`level` tile of a
/// `GeographicTilingScheme` on a world of radius `radiusM`. North-south; east-west is this
/// times `cos(latitude)` and therefore finer everywhere but the equator.
export function postSpacingM(level, size, radiusM) {
  return (180 / (2 ** level * (size - 1))) * metresPerDegree(radiusM);
}

/// Deliberate wrong implementations, reachable by `?fault=`.
///
/// These exist because a verification that has never rejected anything is not known to
/// work. Each one is a plausible bug -- not a random corruption -- chosen so that a check
/// which cannot see it is a check that would not have caught the real mistake either.
///
/// Task 5 extends this set rather than inventing a second mechanism, and its two additions
/// live in `pool.js` because that is the code they are wrong about.
export const FAULTS = {
  /// Row 0 at the *south* edge: the upside-down planet. Renders beautifully.
  flipLatitude: "flip-latitude",
  /// The tile filled from one post east of where Cesium will place it. A registration
  /// error of 1/64 of a tile -- invisible by eye at any zoom.
  shiftTile: "shift-tile",
  /// A world one seed away from the one the checks compare against. A different planet,
  /// rendered without complaint.
  wrongWorld: "wrong-world",
  /// One worker of eight built its world from a stale seed. A scattered eighth of the
  /// tiles come from a different planet and the globe still looks like a globe.
  staleWorker: POOL_FAULTS.staleWorker,
  /// The cache key drops the tile x, so every tile in a row is served its neighbour.
  cacheKey: POOL_FAULTS.cacheKey,
  /// Availability ignores features entirely -- the Task 4 behaviour, which is the bug this
  /// task exists to remove.
  featureBlind: "feature-blind",
  /// Availability refines to the feature depth over the whole globe. The opposite mistake,
  /// and the more expensive one.
  featureEverywhere: "feature-everywhere",
};

/// The rectangle of a tile, in **degrees**, with the north edge named.
///
/// Cesium's rectangles are radians; every engine entry point takes degrees. Converting in
/// one named place means the two systems meet once.
export function tileRectangleDegrees(tilingScheme, x, y, level) {
  const r = tilingScheme.tileXYToRectangle(x, y, level);
  return {
    northDeg: Cesium.Math.toDegrees(r.north),
    southDeg: Cesium.Math.toDegrees(r.south),
    westDeg: Cesium.Math.toDegrees(r.west),
    eastDeg: Cesium.Math.toDegrees(r.east),
    radians: r,
  };
}

/// The latitude and longitude of one post of a tile, by Cesium's heightmap convention.
///
/// Row 0 north, column 0 west, both endpoints included. The interpolation form is
/// `a + (b - a) * (i / last)` -- the same form as the engine's `grid_coordinate`, and that
/// matters: `a * (1 - t) + b * t` is a *different* function in binary floating point and
/// disagrees on 10 of 65 row latitudes for a 0.01-degree tile. A post-for-post exact
/// comparison against the engine needs the identical form or it measures the arithmetic
/// instead of the wiring.
export function postLatLonDeg(rect, size, row, column) {
  const last = size - 1;
  return {
    latitudeDeg: rect.northDeg + (rect.southDeg - rect.northDeg) * (row / last),
    longitudeDeg: rect.westDeg + (rect.eastDeg - rect.westDeg) * (column / last),
  };
}

/// Build the provider.
///
/// # What changed in Task 5
///
/// The fill used to be a synchronous `engine.fillTileF32` on the main thread. It is now
/// `cache.get(...)` over `pool.fill(...)`, and the callback returns a **promise** --
/// `CustomHeightmapTerrainProvider` resolves whatever the callback returns, which is what
/// makes the whole change possible without a provider subclass or a fight with Cesium.
///
/// `engine` and `world` are still taken, and are still used for two things: the
/// synchronous fallback (`?workers=0`, which is also the A/B baseline every timing figure
/// is quoted against) and the main-thread handles the checks compare against.
///
/// `availability` is the `getTileDataAvailable` function. It is passed in rather than built
/// here so `availability.js` can import this module's `postSpacingM` without a cycle.
export function createTerrainProvider({
  engine,
  world,
  radiusM,
  size = HEIGHTMAP_SIZE,
  maxLevel = MAX_LEVEL,
  fault = null,
  credit = "worldbuilder engine",
  onTile = null,
  pool = null,
  cache = null,
  availability = null,
}) {
  const tilingScheme = new Cesium.GeographicTilingScheme();

  /// What the engine is asked for, given a tile. Separated from the callback so a check can
  /// ask "what would you request for this tile" without filling it.
  function tileRequest(x, y, level) {
    const rect = tileRectangleDegrees(tilingScheme, x, y, level);
    let { northDeg, southDeg, westDeg, eastDeg } = rect;

    if (fault === FAULTS.flipLatitude) {
      const swap = northDeg;
      northDeg = southDeg;
      southDeg = swap;
    }
    if (fault === FAULTS.shiftTile) {
      const step = (eastDeg - westDeg) / (size - 1);
      westDeg += step;
      eastDeg += step;
    }

    return {
      handle: world,
      lat0Deg: northDeg,
      lat1Deg: southDeg,
      lon0Deg: westDeg,
      lon1Deg: eastDeg,
      width: size,
      height: size,
      // Sample at the tile's own post spacing, so detail finer than the grid drops out
      // instead of aliasing. At the ground cap this is 76.35 m, below the 78.125 m
      // resolution floor and therefore already the canonical field. Past the ground cap --
      // inside a feature footprint -- it keeps shrinking, which is the point: an authored
      // feature is analytic and has no floor.
      resolutionM: postSpacingM(level, size, radiusM),
      rect,
    };
  }

  /// Counters a check reads. `mainThreadFills` is the one that matters: a pool that
  /// silently fell back to filling on the main thread would render identically, and
  /// "0 divergent" would say nothing about workers.
  const stats = { mainThreadFills: 0, poolFills: 0, handouts: 0, maxLevelRequested: -1 };

  const provider = new Cesium.CustomHeightmapTerrainProvider({
    tilingScheme,
    width: size,
    height: size,
    credit,
    callback(x, y, level) {
      // The deepest level Cesium ever actually asked for a tile at. This, not
      // `maxDepthVisited`, is what an availability cap bounds -- see the `quadtree-depth`
      // check for why the distinction turned out to matter.
      if (level > stats.maxLevelRequested) stats.maxLevelRequested = level;
      const request = tileRequest(x, y, level);
      if (!pool) {
        // The Task 4 path, kept as `?workers=0`. Synchronous, on the main thread, and the
        // baseline every worker figure in the report is measured against.
        stats.mainThreadFills += 1;
        const heights = engine.fillTileF32(request);
        stats.handouts += 1;
        if (onTile) onTile({ x, y, level, request, heights, source: "main" });
        return heights;
      }
      const produce = () => pool.fill(request).then((result) => {
        stats.poolFills += 1;
        return result;
      });
      const entry = cache ? cache.get(x, y, level, produce) : produce();
      return entry.then((result) => {
        // **The copy is load-bearing.** `HeightmapTerrainData` keeps the buffer it is given
        // and transfers it to a Cesium worker when upsampling a child, which detaches it.
        // Handing the cached master out twice would hand out a detached, length-0 buffer
        // the second time -- no error, a flat tile. 16,900 bytes, microseconds.
        const heights = result.heights.slice();
        stats.handouts += 1;
        if (onTile) {
          onTile({ x, y, level, request, heights, source: `worker:${result.worker}`, result });
        }
        return heights;
      });
    },
  });

  // The cap. Set on the instance because the prototype's answer is `undefined`, which is
  // the out-of-memory case described at the top of this file. Never `undefined` here, and
  // feature-aware if an availability function was supplied.
  provider.getTileDataAvailable = availability ?? ((x, y, level) => level <= maxLevel);

  // Handles a check needs, and the numbers it should quote rather than recompute.
  provider.worldbuilder = {
    engine,
    world,
    radiusM,
    size,
    maxLevel,
    fault,
    tilingScheme,
    tileRequest,
    pool,
    cache,
    availability: provider.getTileDataAvailable,
    stats,
    rectangleDegrees: (x, y, level) => tileRectangleDegrees(tilingScheme, x, y, level),
    postSpacingM: (level) => postSpacingM(level, size, radiusM),
  };

  return provider;
}
