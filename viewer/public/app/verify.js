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

function ok(name, pass, detail) {
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
    checks.push(ok(
      "zoom-cap",
      correct && !anyUndefined,
      anyUndefined
        ? "getTileDataAvailable answered undefined, which refines until out of memory"
        : `available through level ${maxLevel} (post spacing ` +
          `${wb.postSpacingM(maxLevel).toFixed(2)} m, resolution floor 78.125 m), ` +
          `false from ${maxLevel + 1} (${wb.postSpacingM(maxLevel + 1).toFixed(2)} m)`,
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
    checks.push(ok(
      "interpolate-height",
      worst < 1e-3,
      `worst |delta| ${worst.toExponential(2)} m through ` +
      `HeightmapTerrainData.interpolateHeight\n    ` + lines.join("\n    "),
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
    ));
  }

  // ----------------------------------------------- 8. the cap, as the quadtree saw it
  if (viewer) {
    const depth = viewer.scene.globe._surface._debug.maxDepthVisited;
    // Measured, from a camera parked 300 m above the witnessed point and left to settle
    // for 400 frames: `maxDepthVisited` reaches 15 and stays there, `tilesVisited` stays
    // at 89 and the JS heap stays at ~40 MB. **Three levels beyond the cap, not one** --
    // the gate is `allAreUpsampled`, and a tile is only marked upsampled once it has been
    // visited and processed, so the traversal overshoots by a small fixed amount before
    // settling. The number that matters is that it settles: with the cap removed (the
    // prototype's `undefined`) the same camera goes 13 -> 16 -> 18 -> 22 -> 25 with
    // `tilesVisited` climbing 80 -> 379 and the heap 33 -> 54 MB, still rising when the
    // run was stopped.
    // A page that has never rendered reports depth 0, and 0 <= anything. Passing on that
    // would be the "green build containing nothing" trap in miniature, so an unrendered
    // scene is reported as *not run* rather than as a pass.
    const frames = viewer.scene.frameState.frameNumber;
    checks.push(ok(
      "quadtree-depth",
      frames > 0 && depth <= maxLevel + 3,
      frames === 0
        ? "NOT EXERCISED: the scene has never rendered (frameNumber 0), so maxDepthVisited " +
          "is trivially 0. Render frames with the camera near the ground before believing this."
        : `Cesium's own maxDepthVisited is ${depth} over ${frames} frames, against a cap of ` +
          `${maxLevel} (measured steady state is maxLevel + 3, because a tile is marked ` +
          `upsampled only after it has been visited)`,
    ));
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
