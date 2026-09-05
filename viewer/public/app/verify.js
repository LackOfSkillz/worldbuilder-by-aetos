//! The checks. Not "does a globe appear" -- **is it this planet, at these coordinates.**
//
// This project has been fooled twice by a green result: by a 327-byte wasm build that
// exported nothing and still exited 0, and by a 100%-divergence run that turned out to be a
// stale cached harness. A screenshot of a smooth ellipsoid looks a great deal like a
// working viewer, so nothing here believes a picture.
//
// Every check compares against something that existed before this task:
//
// - `WITNESSED_ELEVATION_M` is the extraction's pinned value at 12 N 34 E, agreed on by the
//   Python wheel, native Rust and browser WASM. It is a *known engine value at a named
//   point* in the strict sense -- nothing in `viewer/` produced it.
// - Every other height is compared against `wb_elevation_m`, the engine's own scalar entry
//   point, which Task 2 showed agrees bit-for-bit with native Rust over 48,450 values. So
//   what is under test here is the **provider wiring** -- the rectangle, the units, the row
//   order, the resolution -- and it is tested to the last bit of an f32 rather than to a
//   tolerance.
//
// And it is built to be falsifiable: `?fault=` installs a plausible wrong implementation,
// and a check that stays green under one of those is a check that would not have caught the
// real mistake either.

import { postLatLonDeg } from "./terrain.js";
import { extraTiles, featureLevel } from "./availability.js";

/// The witnessed elevation: `Surface::new(20260904, 6_371_000, 12, 0.29, None)` at
/// latitude 12.0, longitude 34.0, `resolution_m = 250`. Pinned three ways by the
/// extraction, and carried in `crates/worldbuilder-engine/tests/wasm_exports.rs` as
/// `WITNESSED_ELEVATION_M`. Compared here for **exact** f64 equality, because that is what
/// "the same planet" means.
export const WITNESSED = {
  latitudeDeg: 12.0,
  longitudeDeg: 34.0,
  resolutionM: 250,
  elevationM: 682.3921701573904,
};

/// Points the checks name. The first two are fixtures with meaning elsewhere in the slice;
/// the rest are spread so a hemisphere or sign error cannot hide in one quadrant.
export const NAMED_POINTS = [
  { name: "witnessed 12N 34E", latitudeDeg: 12.0, longitudeDeg: 34.0 },
  { name: "harbour 18.25S 121.5E", latitudeDeg: -18.25, longitudeDeg: 121.5 },
  { name: "origin 0N 0E", latitudeDeg: 0, longitudeDeg: 0 },
  { name: "north 70N 20W", latitudeDeg: 70, longitudeDeg: -20 },
  { name: "south 45S 150E", latitudeDeg: -45, longitudeDeg: 150 },
  { name: "dateline 5N 179E", latitudeDeg: 5, longitudeDeg: 179 },
];

/// A check result. `witnesses` is an object of **named quantities whose being zero would
/// make this check's pass vacuous**; every one of them must be positive or the check is
/// reported as not exercised, whatever `pass` says.
///
/// ## Why this is a set of named quantities and not one number
///
/// It used to be one number called `work`: **how many things the check looked at**. That
/// was written because the same bug had been found three times -- a check reporting success
/// when it did nothing. Task 5 found one comparing against its own footprint list (passing
/// by agreeing with itself) and another iterating an empty list. Task 6 found
/// `quadtree-depth` reporting 12/0 while Cesium's blob workers were blocked, the globe
/// rendered nothing and `tilesVisited` sat at 0 -- an empty ellipsoid, called correct.
///
/// **A fourth walked straight through it, because `work` measured the wrong quantity.**
/// `feature-resolves` counted the heightmap posts it scanned, and scanned plenty -- but its
/// assertions were gated on `compose === "raise"`, so a `carve` feature contributed
/// thousands to the counter and nothing that could fail. On a world whose features are all
/// carves the check reported a confident pass having asserted nothing at all, with a work
/// count in the tens of thousands. Volume examined and assertions made are different
/// numbers, and it was the second one that was zero.
///
/// **The obvious repair -- count assertions instead -- is not a repair, because neither
/// quantity contains the other.** `quadtree-depth` makes exactly one assertion
/// (`maxLevelRequested <= ceiling`) and always makes it; counting assertions there would
/// report a healthy 1 on the very globe that drew nothing, which is the case that produced
/// the backstop in the first place. Its zero is a *volume* zero: the quadtree walked no
/// tiles. `feature-resolves`'s zero is an *assertion* zero: the loop ran and asserted
/// nothing. One counter cannot see both.
///
/// So the check names every quantity that has to be non-zero for its pass to mean anything,
/// and any of them being zero refuses. `feature-resolves` names both.
///
/// The specific guards are still written out at their call sites, because "the camera never
/// rendered" and "the globe traversed nothing" are different problems with the same symptom
/// and the operator needs to be told which. This is the backstop under all of them.
function ok(name, pass, detail, witnesses) {
  const empty = Object.entries(witnesses ?? {})
    .filter(([, count]) => !(Number.isFinite(count) && count > 0));
  if (empty.length > 0) {
    return {
      name,
      ok: false,
      // A check that already diagnosed its own idleness says it better than this can, so
      // do not shout over it -- just refuse.
      detail: detail.startsWith("NOT EXERCISED") ? detail :
        `NOT EXERCISED: ` +
        empty.map(([label, count]) => `${count} ${label}`).join(", ") +
        `, so a pass would say only that it never had a chance to fail.\n    ${detail}`,
    };
  }
  return { name, ok: pass, detail };
}

/// Which tile of `level` holds a point, by Cesium's own tiling scheme -- not by arithmetic
/// repeated here.
function tileFor(tilingScheme, latitudeDeg, longitudeDeg, level) {
  const carto = Cesium.Cartographic.fromDegrees(longitudeDeg, latitudeDeg);
  return tilingScheme.positionToTileXY(carto, level);
}

/// One tile, fetched **through the provider** rather than through the fill, so the path
/// under test is the one Cesium uses.
async function tileData(provider, x, y, level) {
  const data = await provider.requestTileGeometry(x, y, level);
  if (!data) throw new Error(`requestTileGeometry(${x},${y},${level}) returned undefined`);
  return data;
}

/// Compare every post of a tile against `wb_elevation_m` at the latitude and longitude
/// Cesium will place that post at.
///
/// **The row convention is taken from Cesium, not from the fill.** `postLatLonDeg` puts row
/// 0 at the north edge because `HeightmapTerrainData.interpolateHeight` reads
/// `height - 1 - southInteger`; if the provider handed the engine its latitudes the other
/// way up, every row but the middle one diverges here.
///
/// The comparison is `Math.fround(engineHeight) === bufferValue` -- exact. Rust's `as f32`
/// and `Math.fround` are both round-to-nearest-even, so anything other than 0 divergent is
/// a real disagreement, not a rounding allowance.
function comparePosts({ engine, world, data, rect, size, resolutionM }) {
  const buffer = data._buffer;
  let divergent = 0;
  let firstDivergence = null;
  let minM = Infinity;
  let maxM = -Infinity;
  for (let row = 0; row < size; row += 1) {
    for (let column = 0; column < size; column += 1) {
      const { latitudeDeg, longitudeDeg } = postLatLonDeg(rect, size, row, column);
      const expected = Math.fround(engine.elevationM(world, latitudeDeg, longitudeDeg, resolutionM));
      const actual = buffer[row * size + column];
      if (actual < minM) minM = actual;
      if (actual > maxM) maxM = actual;
      if (!Object.is(expected, actual)) {
        divergent += 1;
        if (!firstDivergence) {
          firstDivergence = { row, column, latitudeDeg, longitudeDeg, expected, actual };
        }
      }
    }
  }
  return { samples: size * size, divergent, firstDivergence, minM, maxM };
}

export async function runChecks({
  viewer,
  engine,
  provider,
  reference,
  spec,
  coastLevel = 3,
  coastStepDeg = 5,
} = {}) {
  const wb = provider.worldbuilder;
  const { size, maxLevel, radiusM, tilingScheme } = wb;
  const checks = [];

  // ---------------------------------------------------------------- 1. the right planet
  {
    const actual = engine.elevationM(
      reference, WITNESSED.latitudeDeg, WITNESSED.longitudeDeg, WITNESSED.resolutionM,
    );
    checks.push(ok(
      "witnessed-elevation",
      Object.is(actual, WITNESSED.elevationM),
      `wb_elevation_m(${WITNESSED.latitudeDeg}, ${WITNESSED.longitudeDeg}, ` +
      `res=${WITNESSED.resolutionM}) = ${actual} (expected exactly ${WITNESSED.elevationM}); ` +
      `generator v${engine.generatorVersion()}`,
    ));
  }

  // -------------------------------------------------- 2. the provider's declared shape
  {
    const ellipsoid = tilingScheme.ellipsoid;
    const expectedLevelZero = Cesium.TerrainProvider
      .getEstimatedLevelZeroGeometricErrorForAHeightmap(
        ellipsoid, size, tilingScheme.getNumberOfXTilesAtLevel(0),
      );
    const problems = [];
    // `instanceof`, not `constructor.name`: the vendored Cesium is the minified build and
    // every class name is mangled (`CustomHeightmapTerrainProvider` reports as `xA`). A
    // name comparison here passed nothing and failed everything -- found by running it.
    if (!(provider instanceof Cesium.CustomHeightmapTerrainProvider)) {
      problems.push(`provider is not a CustomHeightmapTerrainProvider (${provider.constructor.name})`);
    }
    // `ready`/`readyPromise` were removed in 1.107. Their *absence* is the check: a
    // provider that grew them back would mean the vendored Cesium is not the one assumed.
    if ("ready" in provider || "readyPromise" in provider) {
      problems.push("provider exposes ready/readyPromise, which 1.107 removed");
    }
    if (!(tilingScheme instanceof Cesium.GeographicTilingScheme)) {
      problems.push("tiling scheme is not GeographicTilingScheme");
    }
    if (provider.getLevelMaximumGeometricError(0) !== expectedLevelZero) {
      problems.push(
        `level-0 geometric error ${provider.getLevelMaximumGeometricError(0)} != ` +
        `getEstimatedLevelZeroGeometricErrorForAHeightmap ${expectedLevelZero}`,
      );
    }
    for (const level of [1, 5, 12]) {
      if (provider.getLevelMaximumGeometricError(level) !== expectedLevelZero / (1 << level)) {
        problems.push(`level-${level} geometric error is not level-0 / 2^${level}`);
      }
    }
    checks.push(ok(
      "provider-shape",
      problems.length === 0,
      problems.length ? problems.join("; ")
        : `CustomHeightmapTerrainProvider ${size}x${size}, no ready/readyPromise, ` +
          `level-0 geometric error ${expectedLevelZero.toFixed(3)} m from ` +
          `getEstimatedLevelZeroGeometricErrorForAHeightmap`,
    ));
  }

  // ---------------------------------------------------------------- 3. the zoom cap
  {
    const answers = [];
    for (let level = 0; level <= maxLevel + 4; level += 1) {
      answers.push([level, provider.getTileDataAvailable(0, 0, level)]);
    }
    const anyUndefined = answers.some(([, a]) => a === undefined);
    const correct = answers.every(([level, a]) => a === (level <= maxLevel));
    // `every` on an empty array is `true`. Were `maxLevel` ever undefined or NaN this
    // loop would not run once, and the check would report a confident pass having asked
    // the provider nothing -- so the number of levels interrogated is its witness. Every
    // one of them is also an assertion (`a === (level <= maxLevel)`), so the two counts
    // the backstop distinguishes are the same number here.
    checks.push(ok(
      "zoom-cap",
      correct && !anyUndefined,
      anyUndefined
        ? "getTileDataAvailable answered undefined, which refines until out of memory"
        : `available through level ${maxLevel} (post spacing ` +
          `${wb.postSpacingM(maxLevel).toFixed(2)} m, resolution floor 78.125 m), ` +
          `false from ${maxLevel + 1} (${wb.postSpacingM(maxLevel + 1).toFixed(2)} m); ` +
          `${answers.length} levels interrogated`,
      { "levels interrogated": answers.length },
    ));
  }

  // ------------------------------------------- 4. the heightmap Cesium actually receives
  {
    const data = await tileData(provider, 0, 0, 0);
    const s = data._structure;
    const problems = [];
    if (!(data._buffer instanceof Float32Array)) problems.push("buffer is not a Float32Array");
    if (data._width !== size || data._height !== size) problems.push("width/height mismatch");
    if (s.heightScale !== 1) problems.push(`heightScale ${s.heightScale}, expected 1`);
    if (s.heightOffset !== 0) problems.push(`heightOffset ${s.heightOffset}, expected 0`);
    if (s.elementsPerHeight !== 1) problems.push(`elementsPerHeight ${s.elementsPerHeight}`);
    if (s.stride !== 1) problems.push(`stride ${s.stride}`);
    checks.push(ok(
      "heightmap-structure",
      problems.length === 0,
      problems.length ? problems.join("; ")
        : `Float32Array ${data._width}x${data._height}, default structure ` +
          `(scale 1, offset 0, stride 1): the values are metres above the ellipsoid, directly`,
    ));
  }

  // ------------------------------------------------- 5. every post of every named tile
  {
    const tiles = [];
    for (const point of NAMED_POINTS) {
      const { x, y } = tileFor(tilingScheme, point.latitudeDeg, point.longitudeDeg, maxLevel);
      tiles.push({ label: `${point.name} @L${maxLevel}`, x, y, level: maxLevel });
    }
    // Coarse levels too: level 0 spans the poles, where a latitude sign error is loudest.
    tiles.push({ label: "west hemisphere @L0", x: 0, y: 0, level: 0 });
    tiles.push({ label: "east hemisphere @L0", x: 1, y: 0, level: 0 });
    tiles.push({ label: "arbitrary @L5", x: 21, y: 9, level: 5 });

    let totalSamples = 0;
    let totalDivergent = 0;
    let first = null;
    const lines = [];
    for (const tile of tiles) {
      const rect = wb.rectangleDegrees(tile.x, tile.y, tile.level);
      const data = await tileData(provider, tile.x, tile.y, tile.level);
      const result = comparePosts({
        engine, world: reference, data, rect, size,
        resolutionM: wb.postSpacingM(tile.level),
      });
      totalSamples += result.samples;
      totalDivergent += result.divergent;
      if (!first && result.firstDivergence) first = { tile: tile.label, ...result.firstDivergence };
      lines.push(
        `${tile.label} (${tile.x},${tile.y},L${tile.level}): ${result.divergent}/` +
        `${result.samples} divergent, heights ${result.minM.toFixed(1)}..${result.maxM.toFixed(1)} m`,
      );
    }
    // "0 divergent" is also what an empty tile list, or a zero-sized heightmap, reports.
    // The sample count is the witness: 0 posts compared is 0 divergent. Each post compared
    // is itself an assertion, so this one number is both quantities.
    checks.push(ok(
      "tile-posts-exact",
      totalDivergent === 0,
      `${totalDivergent}/${totalSamples} posts divergent from wb_elevation_m at the ` +
      `latitude/longitude Cesium places them\n    ` + lines.join("\n    ") +
      (first
        ? `\n    first divergence: ${first.tile} row ${first.row} col ${first.column} ` +
          `(${first.latitudeDeg.toFixed(6)}, ${first.longitudeDeg.toFixed(6)}) ` +
          `expected ${first.expected} got ${first.actual}`
        : ""),
      { "posts compared": totalSamples },
    ));
  }

  // ------------------------------- 6. Cesium's own interpolation, at the named points
  {
    const lines = [];
    let worst = 0;
    for (const point of NAMED_POINTS) {
      const { x, y } = tileFor(tilingScheme, point.latitudeDeg, point.longitudeDeg, maxLevel);
      const rect = wb.rectangleDegrees(x, y, maxLevel);
      const data = await tileData(provider, x, y, maxLevel);
      // Snap to the nearest post so the comparison is against a value the tile *holds*
      // rather than against a bilinear blend of four of them; the blend is correct but its
      // error is a property of the terrain's roughness, not of the wiring.
      const row = Math.round(
        ((rect.northDeg - point.latitudeDeg) / (rect.northDeg - rect.southDeg)) * (size - 1),
      );
      const column = Math.round(
        ((point.longitudeDeg - rect.westDeg) / (rect.eastDeg - rect.westDeg)) * (size - 1),
      );
      const post = postLatLonDeg(rect, size, row, column);
      const throughCesium = data.interpolateHeight(
        rect.radians,
        Cesium.Math.toRadians(post.longitudeDeg),
        Cesium.Math.toRadians(post.latitudeDeg),
      );
      const fromEngine = engine.elevationM(
        reference, post.latitudeDeg, post.longitudeDeg, wb.postSpacingM(maxLevel),
      );
      const delta = Math.abs(throughCesium - fromEngine);
      if (delta > worst) worst = delta;
      lines.push(
        `${point.name}: post(${row},${column}) Cesium ${throughCesium.toFixed(4)} m vs ` +
        `engine ${fromEngine.toFixed(4)} m, |d| = ${delta.toExponential(2)} m`,
      );
    }
    // 1 mm. The only difference that should survive is the f32 narrowing in the tile,
    // which the engine's own note measures at 1.93e-5 m at the witnessed probe.
    // `worst` starts at 0, and 0 < 1e-3: a run that measured nothing reports the best
    // possible result. The number of points measured is the work.
    checks.push(ok(
      "interpolate-height",
      worst < 1e-3,
      `worst |delta| ${worst.toExponential(2)} m over ${lines.length} points through ` +
      `HeightmapTerrainData.interpolateHeight\n    ` + lines.join("\n    "),
      { "points measured": lines.length },
    ));
  }

  // ------------------------------------------------------- 7. land where land should be
  {
    // Sign agreement between the engine's canonical field and the height the *terrain
    // Cesium holds* reports, over a global grid. A hemisphere flip, a longitude offset or
    // a wrong world moves coastlines; this is the check that says the picture is of the
    // right planet, in the same units, the right way up.
    const cache = new Map();
    let compared = 0;
    let agree = 0;
    let land = 0;
    let weighted = 0;
    const disagreements = [];
    for (let latitudeDeg = -85; latitudeDeg <= 85; latitudeDeg += coastStepDeg) {
      for (let longitudeDeg = -180; longitudeDeg < 180; longitudeDeg += coastStepDeg) {
        const { x, y } = tileFor(tilingScheme, latitudeDeg, longitudeDeg, coastLevel);
        const key = `${x}/${y}`;
        if (!cache.has(key)) {
          cache.set(key, {
            rect: wb.rectangleDegrees(x, y, coastLevel),
            data: await tileData(provider, x, y, coastLevel),
          });
        }
        const { rect, data } = cache.get(key);
        const throughCesium = data.interpolateHeight(
          rect.radians,
          Cesium.Math.toRadians(longitudeDeg),
          Cesium.Math.toRadians(latitudeDeg),
        );
        const fromEngine = engine.elevationM(
          reference, latitudeDeg, longitudeDeg, wb.postSpacingM(coastLevel),
        );
        // Area weight. A uniform lat/lon grid over-counts the poles badly -- unweighted,
        // this world reads 37.6% land against a requested 0.29 -- so each sample carries
        // cos(latitude), which is the width of its cell.
        const weight = Math.cos(Cesium.Math.toRadians(latitudeDeg));
        compared += 1;
        weighted += weight;
        if (fromEngine > 0) land += weight;
        if ((throughCesium > 0) === (fromEngine > 0)) agree += 1;
        else if (disagreements.length < 5) {
          disagreements.push(
            `${latitudeDeg},${longitudeDeg}: Cesium ${throughCesium.toFixed(1)} vs ` +
            `engine ${fromEngine.toFixed(1)}`,
          );
        }
      }
    }
    // Bilinear interpolation across a coastline legitimately crosses zero at a slightly
    // different place than a point sample, so a handful of near-shore disagreements is
    // expected and a wholesale one is not.
    // 0/0 is NaN and NaN > 0.98 is false, so an empty grid already failed here -- but by
    // IEEE accident rather than by intent. Stated, so it stays true.
    const rate = agree / compared;
    checks.push(ok(
      "land-and-sea",
      rate > 0.98,
      `${agree}/${compared} points agree on land-vs-sea between the loaded terrain and ` +
      `wb_elevation_m (${(rate * 100).toFixed(2)}%); engine land fraction over this grid, ` +
      `cos(lat)-weighted, ${(land / weighted * 100).toFixed(1)}%, against a requested ` +
      `land_fraction of ` +
      `${spec ? spec.landFraction : "?"}` +
      (disagreements.length ? `\n    e.g. ${disagreements.join("; ")}` : ""),
      { "points compared": compared },
    ));
  }

  // ------------------------------------- 8. the cap, as the quadtree and the fills saw it
  if (viewer) {
    const debug = viewer.scene.globe._surface._debug;
    const depth = debug.maxDepthVisited;
    const frames = viewer.scene.frameState.frameNumber;
    const ceiling = (wb.availability && wb.availability.featureMaxLevel) || maxLevel;

    // **What the cap bounds is the work, not the traversal.** This check used to assert
    // `maxDepthVisited <= maxLevel + 3`, from Task 4's measurement of 15 flat over 400
    // frames. That figure does not survive a real canvas: with the camera 300 m above
    // 12 N 34 E in a 1200 x 800 tab, `maxDepthVisited` settles at **26** against a cap of
    // 12 -- and it does so **identically on Task 4's own synchronous path**
    // (`?workers=0&cache=0`), so it is not a Task 5 regression, it is Task 4's number
    // being an artifact of a hand-driven 560 x 560 backing buffer with a much larger
    // screen-space error. What is flat in both cases is everything that costs anything:
    // 43 tiles visited, no tile requested above the cap, and a heap oscillating 33-40 MB
    // on GC with no trend over thousands of frames.
    //
    // So the assertion is now on the quantity the availability function actually
    // controls: **no tile was ever requested above the ceiling.** Under the prototype's
    // `undefined`, Task 4 measured tilesVisited climbing 80 -> 379 and the heap 33 -> 54 MB
    // still rising, which this catches directly -- those are requested tiles.
    //
    // A page that has never rendered reports depth 0, and 0 <= anything. Passing on that
    // would be the "green build containing nothing" trap in miniature, so an unrendered
    // scene is reported as *not run* rather than as a pass.
    //
    // **`frameNumber > 0` was not enough, and a real page proved it.** While Cesium's blob
    // workers were blocked by an early cut of the Content-Security-Policy, the page
    // rendered frames continuously and traversed *nothing*: `tilesVisited` 0,
    // `maxDepthVisited` 0, an empty ellipsoid on screen -- and this check reported PASS,
    // because "no tile was requested above the ceiling" is trivially true when no tile was
    // requested at all. `maxLevelRequested` starts at -1 and every comparison against the
    // ceiling succeeds from there.
    //
    // So the traversal has to have happened before its depth means anything. `tilesVisited`
    // is the right witness and not `maxLevelRequested`, because the checks above this one
    // call `requestTileGeometry` themselves: `maxLevelRequested` is non-negative by the
    // time we get here even on a globe that drew nothing, while `tilesVisited` counts only
    // what the quadtree itself walked.
    const tilesVisited = debug.tilesVisited;
    const overCeiling = wb.stats.maxLevelRequested > ceiling;
    let notExercised = null;
    if (!(frames > 0)) {
      notExercised =
        "the scene has never rendered (frameNumber 0), so maxDepthVisited is trivially 0. " +
        "Render frames with the camera near the ground before believing this.";
    } else if (!(tilesVisited > 0)) {
      notExercised =
        `the scene rendered ${frames} frames and the quadtree visited 0 tiles, so every ` +
        "depth statistic here is 0 by default rather than by measurement. This is what a " +
        "globe that draws an empty ellipsoid looks like from the inside -- it is how a " +
        "blocked worker pool was found -- and it is not a pass.";
    }
    checks.push(ok(
      "quadtree-depth",
      notExercised === null && !overCeiling,
      notExercised
        ? `NOT EXERCISED: ${notExercised}`
        : (overCeiling
            ? `a tile was requested at L${wb.stats.maxLevelRequested}, above the ceiling of ` +
              `${ceiling}: `
            : "") +
          `deepest tile actually requested L${wb.stats.maxLevelRequested} against a ground ` +
          `cap of ${maxLevel} and a feature cap of ${ceiling}; Cesium's own maxDepthVisited ` +
          `${depth} over ${frames} frames, ${debug.tilesVisited} tiles visited ` +
          `(traversal overshoots the cap and costs nothing -- it requests no tiles there)`,
      // The backstop, saying the same thing a second way: a traversal of zero tiles is
      // zero work, whatever the branch above decided.
      //
      // **This check's witness is a volume and not an assertion count, deliberately.** It
      // makes exactly one assertion, `maxLevelRequested <= ceiling`, and always makes it;
      // counting assertions here would report a healthy 1 on the globe that drew nothing,
      // which is the case that produced the backstop. See `ok()`.
      { "tiles visited by the quadtree": tilesVisited },
    ));
  }

  // ------------------------------------------------- 9. the tiles came from workers
  {
    const pool = wb.pool;
    const stats = wb.stats;
    if (!pool) {
      checks.push(ok(
        "worker-path",
        false,
        "NOT EXERCISED: there is no pool (?workers=0), so every tile above was filled " +
        "synchronously on the main thread. Nothing here says anything about workers.",
      ));
    } else {
      const problems = [];
      // This check already refuses zero work explicitly -- `poolFills === 0` is a problem
      // and an idle worker is a problem -- so it needs no backstop. Left as it is, and
      // recorded here so the next reader does not have to re-derive that.
      // The one that matters. A pool that quietly fell back to the main thread would
      // render identically and every bit-exact check above would still pass, so "0
      // divergent" would be a statement about the engine and not about the worker path.
      if (stats.mainThreadFills !== 0) {
        problems.push(`${stats.mainThreadFills} tiles were filled on the main thread`);
      }
      if (stats.poolFills === 0) problems.push("no tile was filled by a worker");
      const idle = pool.dispatched.filter((d) => d === 0).length;
      if (idle > 0) problems.push(`${idle} of ${pool.workers.length} workers were never used`);
      // **Computed, printed, and -- until now -- never failed on.** A worker announces in
      // its `ready` message which world it built; `stale-worker` puts exactly one of eight
      // on seed + 1, and the one check named for the worker path reported PASS on it. That
      // is not a false record -- the README's fault table credits tile-posts, interpolate
      // and land-and-sea with catching it, and they do -- but it is a number this check
      // went to the trouble of collecting and then declined to use.
      //
      // **It is corroboration, not detection, and the difference matters.** This is the
      // worker's own confession: a real version-skew bug does not set a `stale` flag, it
      // just builds a different world, and only the bit-exact checks above can see that.
      // What this catches is the pool having accepted a worker whose world is not the one
      // the page asked for -- a defect in the pool whether or not any tile happens to
      // diverge, and one that can never be true on a healthy run.
      const stale = pool.stats().staleWorkers;
      if (stale.length > 0) {
        problems.push(
          `${stale.length} of ${pool.workers.length} workers reported building a world ` +
          `other than the one requested (indices ${stale.join(",")})`,
        );
      }
      checks.push(ok(
        "worker-path",
        problems.length === 0,
        problems.length ? problems.join("; ")
          : `${stats.poolFills} fills across ${pool.workers.length} workers ` +
            `(dispatched ${pool.dispatched.join("/")}), ${stats.mainThreadFills} on the ` +
            `main thread, ${stats.handouts} handouts; worker fill ` +
            `median ${pool.stats().fillMs.median.toFixed(2)} ms over n=` +
            `${pool.stats().fillMs.n}` +
            (stale.length ? `; workers on a stale world: ${stale.join(",")}` : ""),
      ));
    }
  }

  // ------------------------------------ 10. the cache serves the tile it was asked for
  {
    const cache = wb.cache;
    if (!cache) {
      checks.push(ok("cache-identity", false, "NOT EXERCISED: no cache (?cache=0)"));
    } else {
      // Two horizontally adjacent tiles at the ground cap. A key that drops x collides
      // them, and the second request is answered with the first tile's heights -- a
      // rendered globe that stays entirely plausible.
      const { x, y } = tileFor(tilingScheme, 12.0, 34.0, maxLevel);
      const problems = [];
      const details = [];
      // Every post this check actually compared. A zero-sized heightmap would make each
      // comparison below vacuous -- `identical` and `repeatEqual` both start true over an
      // empty buffer -- so the total is carried out as this check's work.
      let posts = 0;
      const before = cache.stats();
      const first = await tileData(provider, x, y, maxLevel);
      const second = await tileData(provider, x + 1, y, maxLevel);
      for (const [label, tx, data] of [["left", x, first], ["right", x + 1, second]]) {
        const result = comparePosts({
          engine, world: reference, data,
          rect: wb.rectangleDegrees(tx, y, maxLevel), size,
          resolutionM: wb.postSpacingM(maxLevel),
        });
        posts += result.samples;
        details.push(
          `${label} (${tx},${y},L${maxLevel}): ${result.divergent}/${result.samples} divergent`,
        );
        if (result.divergent !== 0) {
          problems.push(`${label} tile diverges from wb_elevation_m at its own rectangle`);
        }
      }
      let identical = true;
      posts += first._buffer.length;
      for (let i = 0; i < first._buffer.length; i += 1) {
        if (!Object.is(first._buffer[i], second._buffer[i])) { identical = false; break; }
      }
      if (identical) problems.push("two different tiles came back bit-identical");

      // A repeat of the same tile must be a hit, must be equal, and must be a *different*
      // object: `HeightmapTerrainData` transfers its buffer to a Cesium worker when
      // upsampling, so handing the same array out twice hands out a detached one.
      const hitsBefore = cache.hits;
      const repeat = await tileData(provider, x, y, maxLevel);
      if (cache.hits <= hitsBefore) problems.push("re-requesting a tile did not hit the cache");
      if (repeat._buffer === first._buffer) {
        problems.push("the cache handed out the same ArrayBuffer twice (Cesium detaches it)");
      }
      let repeatEqual = repeat._buffer.length === first._buffer.length;
      for (let i = 0; repeatEqual && i < first._buffer.length; i += 1) {
        if (!Object.is(first._buffer[i], repeat._buffer[i])) repeatEqual = false;
      }
      if (!repeatEqual) problems.push("a cache hit returned different heights from the miss");
      checks.push(ok(
        "cache-identity",
        problems.length === 0,
        (problems.length ? problems.join("; ") + "\n    " : "") +
        details.join("; ") +
        `\n    cache ${cache.size}/${cache.capacity} tiles, ${cache.hits} hits, ` +
        `${cache.misses} misses, ${cache.evictions} evictions ` +
        `(was ${before.hits}/${before.misses}); ${posts} posts compared`,
        { "posts compared": posts },
      ));
    }
  }

  // ------------------------------------------------- 11. availability is feature-aware
  {
    const availability = wb.availability;
    const footprints = availability.footprints ?? [];
    const problems = [];
    const lines = [];
    let undefinedSeen = false;
    // Task 5 already stopped this one passing by agreeing with itself. What is counted
    // here is the other half: how many times the availability function was actually
    // interrogated. Both branches below are loops, and a loop that does not run is a
    // silent pass.
    let probes = 0;

    // Nothing may answer `undefined` anywhere, at any level, feature or no feature.
    for (const level of [0, maxLevel, maxLevel + 1, availability.featureMaxLevel + 1, 25]) {
      for (const [x, y] of [[0, 0], [1, 0], [5, 3]]) {
        probes += 1;
        if (availability(x, y, level) === undefined) undefinedSeen = true;
      }
    }
    if (undefinedSeen) problems.push("getTileDataAvailable answered undefined");

    // **Compared against the spec, not against the availability object's own idea of how
    // many features there are.** An availability function that simply ignored features
    // would otherwise walk into the "no features on this world" branch below and pass by
    // agreeing with itself -- which is precisely the Task 4 behaviour this task removes,
    // and is reachable as `?fault=feature-blind`.
    const requested = (spec && spec.features) ? spec.features.length : 0;
    if (footprints.length !== requested) {
      problems.push(
        `the world was built with ${requested} features and availability knows about ` +
        `${footprints.length}`,
      );
    }

    if (footprints.length === 0) {
      if (availability.featureMaxLevel !== maxLevel) {
        problems.push(`no features but featureMaxLevel is ${availability.featureMaxLevel}`);
      }
      for (const level of [maxLevel + 1, maxLevel + 4]) {
        for (const [x, y] of [[0, 0], [1000, 500], [3000, 1200]]) {
          probes += 1;
          if (availability(x, y, level) !== false) {
            problems.push(`available at (${x},${y},L${level}) on a world with no features`);
          }
        }
      }
      lines.push(
        "no features on this world: availability is exactly the Task 4 cap, true through " +
        `L${maxLevel} and false above it`,
      );
    } else {
      for (const f of footprints) {
        const { latitudeDeg, longitudeDeg } = f.feature;
        // Every level from the ground cap to the feature's own level must be available at
        // the feature.
        for (let level = maxLevel + 1; level <= f.level; level += 1) {
          const { x, y } = tileFor(tilingScheme, latitudeDeg, longitudeDeg, level);
          probes += 1;
          if (availability(x, y, level) !== true) {
            problems.push(
              `feature at ${latitudeDeg},${longitudeDeg} is not available at L${level}`,
            );
          }
        }
        // And a tile a long way from any feature must not be, at the first level past the
        // ground cap -- "and nowhere else" is half the requirement.
        const away = tileFor(tilingScheme, latitudeDeg + 20, longitudeDeg + 40, maxLevel + 1);
        probes += 1;
        if (availability(away.x, away.y, maxLevel + 1) !== false) {
          problems.push(`available 20 deg away from every feature at L${maxLevel + 1}`);
        }
        lines.push(
          `${f.feature.compose} ${f.feature.lengthM}x${f.feature.widthM} m at ` +
          `${latitudeDeg},${longitudeDeg}: reach ` +
          `${Math.hypot(f.feature.lengthM, f.feature.widthM).toFixed(1)} m, footprint ` +
          `${(f.northDeg - f.southDeg).toFixed(5)} deg tall, refines to L${f.level} ` +
          `(${wb.postSpacingM(f.level).toFixed(2)} m posts)`,
        );
      }
      // Past the deepest feature, nothing is available anywhere -- including at the
      // features themselves. This is the bound that keeps the answer from being `true`
      // forever, which is the same out-of-memory failure as `undefined`.
      const deepest = availability.featureMaxLevel;
      for (const f of footprints) {
        const t = tileFor(tilingScheme, f.feature.latitudeDeg, f.feature.longitudeDeg, deepest + 1);
        probes += 1;
        if (availability(t.x, t.y, deepest + 1) !== false) {
          problems.push(`still available at L${deepest + 1}, past the deepest feature`);
        }
      }
      const extra = extraTiles(availability, tilingScheme, maxLevel);
      lines.push(
        "cost, enumerated by descending from the ground cap: " +
        extra.map((r) => `L${r.level} ${r.tiles}`).join(", ") +
        ` = ${extra.reduce((a, r) => a + r.tiles, 0)} extra tiles in total`,
      );
    }
    lines.push(`${probes} availability interrogations made`);
    checks.push(ok("feature-availability", problems.length === 0,
      (problems.length ? problems.join("; ") + "\n    " : "") + lines.join("\n    "),
      { "availability interrogations": probes }));
  }

  // ------------------------------------ 12. and the deeper tile actually resolves it
  if (spec && spec.features && spec.features.length > 0) {
    const lines = [];
    const problems = [];
    // Heights actually scanned. Task 5 made this loop spec-driven so it cannot run empty
    // over `availability.footprints`; this counts the posts too, so a zero-length
    // heightmap cannot leave `extreme` at its sentinel and pass by not looking.
    let scanned = 0;
    // Assertions actually attempted, which is not the same number as posts read. See the
    // note on compose modes below, and `ok()`.
    let asserted = 0;
    // **Driven from the spec, through `featureLevel` directly.** An earlier cut of this
    // iterated `availability.footprints`, and under `?fault=feature-blind` that list is
    // empty, so the check reported nothing and passed -- a check that cannot fail because
    // it has no work to do. Same trap as Task 4's depth check on a page that never
    // rendered, one task later.
    // **Both compose modes are asserted on, and not with the same statistic.**
    //
    // An earlier cut asserted only on `raise`, reasoning that the extreme height over a
    // tile is the right statistic for a mole standing proud of the seabed but not for a
    // carve, whose tile also holds thousands of posts of ordinary abyssal floor. The
    // reasoning was right and the consequence was not: a carve still added its posts to
    // `scanned`, so on a world whose features are all carves this check passed green having
    // asserted nothing, with a work count in the tens of thousands. That is the fourth
    // check in this slice to pass having done nothing, and the first the `work` backstop
    // could not see -- see `ok()` for why counting assertions instead is not the answer
    // either. Both are counted now, and both must be positive.
    //
    // The carve's assertion is one-sided, because that is what is actually guaranteed.
    // `CARVE` is one-way (`features.rs`: it applies only where `lift < 0`), so over the
    // feature's own tile the minimum either comes down to the target -- if the ambient
    // ground was above it -- or stays where it was, if the ambient ground was already
    // deeper. It can never end up *above* the target. `min <= targetM` therefore holds on
    // every world, and fails exactly when a carve fills instead of cutting.
    //
    // This slice's harbour is the second case and now says so out loud: its -12 m basin
    // sits over abyssal floor near -4,600 m, so the carve is **inert** -- it has nothing to
    // cut -- and the old detail line reported that as a carve "4,604 m off target", which
    // reads like a failure and is not one. Effective and inert are distinguished by name.
    for (const feature of spec.features) {
      const { latitudeDeg, longitudeDeg, targetM, compose } = feature;
      const wants = featureLevel(feature, radiusM, size);
      const readings = [];
      for (const level of [maxLevel, wants]) {
        const { x, y } = tileFor(tilingScheme, latitudeDeg, longitudeDeg, level);
        const data = await tileData(provider, x, y, level);
        let extreme = compose === "carve" ? Infinity : -Infinity;
        scanned += data._buffer.length;
        for (const h of data._buffer) {
          if (compose === "carve") { if (h < extreme) extreme = h; }
          else if (h > extreme) extreme = h;
        }
        readings.push({ level, extreme, error: Math.abs(extreme - targetM) });
      }
      const [atCap, atFeature] = readings;
      const statistic = compose === "carve" ? "min" : "max";
      if (compose === "raise") {
        lines.push(
          `raise to ${targetM} m: L${atCap.level} ${statistic} ${atCap.extreme.toFixed(1)} m ` +
          `(|err| ${atCap.error.toFixed(1)}), L${atFeature.level} ${statistic} ` +
          `${atFeature.extreme.toFixed(1)} m (|err| ${atFeature.error.toFixed(1)})`,
        );
        asserted += 2;
        if (!(atFeature.error <= 2)) {
          problems.push(
            `raise to ${targetM} m still off by ${atFeature.error.toFixed(1)} m at ` +
            `L${atFeature.level}`,
          );
        }
        if (!(atCap.error > 10)) {
          problems.push(
            `the ground cap L${atCap.level} was already within ${atCap.error.toFixed(1)} m ` +
            `of ${targetM} m, so refining past it proves nothing here`,
          );
        }
      } else if (compose === "carve") {
        // 2 m, the same tolerance the raise arm uses and for the same reason: the tile
        // holds f32 posts on a finite grid, so a met target is met to within a post.
        const effective = atFeature.error <= 2;
        const inert = atFeature.extreme < targetM - 2;
        lines.push(
          `carve to ${targetM} m: L${atFeature.level} ${statistic} ` +
          `${atFeature.extreme.toFixed(1)} m -- ` +
          (effective
            ? `cut to target (|err| ${atFeature.error.toFixed(1)})`
            : inert
              ? `INERT: the ambient ground here is already ` +
                `${(targetM - atFeature.extreme).toFixed(1)} m below the target, so a ` +
                `one-way carve has nothing to cut and correctly does nothing`
              : `neither at its target nor below it`),
        );
        asserted += 1;
        if (!(atFeature.extreme <= targetM + 2)) {
          problems.push(
            `carve to ${targetM} m left the L${atFeature.level} minimum at ` +
            `${atFeature.extreme.toFixed(1)} m, above its own target: a one-way carve ` +
            `filled instead of cutting`,
          );
        }
      } else {
        // A compose mode nobody wrote an assertion for must not slip through as work done.
        asserted += 1;
        problems.push(
          `feature at ${latitudeDeg},${longitudeDeg} composes as "${compose}", which this ` +
          `check has no assertion for`,
        );
      }
    }
    lines.push(
      `${scanned} heights scanned, ${asserted} assertion${asserted === 1 ? "" : "s"} ` +
      `made over ${spec.features.length} ` +
      `feature${spec.features.length === 1 ? "" : "s"}`,
    );
    checks.push(ok("feature-resolves", problems.length === 0,
      (problems.length ? problems.join("; ") + "\n    " : "") + lines.join("\n    "),
      // Both, because neither contains the other: a spec whose loop reads nothing has
      // `scanned` 0, and a spec every one of whose features falls past every assertion
      // branch has `asserted` 0 while `scanned` runs into the tens of thousands.
      { "heights scanned": scanned, "assertions made": asserted }));
  }

  const passed = checks.filter((c) => c.ok).length;
  return {
    fault: wb.fault ?? "none",
    world: { spec, maxLevel, size, radiusM },
    passed,
    failed: checks.length - passed,
    checks,
  };
}

export function formatChecks(result) {
  const head =
    `fault=${result.fault} | ${result.passed} passed, ${result.failed} failed`;
  const body = result.checks
    .map((c) => `${c.ok ? "PASS" : "FAIL"}  ${c.name}\n    ${c.detail}`)
    .join("\n");
  return `${head}\n${body}`;
}
