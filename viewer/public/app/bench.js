//! The frame budget, measured rather than asserted.
//
// Every number this module produces carries its population. "p90 18.2 ms" over eleven
// samples and over eleven hundred are different claims, and a tile cost quoted without
// saying whether it was coastal or abyssal is not a number at all: the two differ by most
// of an order of magnitude, and a viewer looks at coasts.
//
// Four measurements, in order of how much they matter:
//
// 1. **Cost per tile, split by population.** Coastal, land and deep-ocean tiles filled the
//    same way, timed separately. The split is made from the *filled heights*, not guessed
//    beforehand, so a tile is called coastal because its posts straddle the datum.
// 2. **What the main thread actually pays**, which after Task 5 is a `slice()` and a
//    promise, not a fill.
// 3. **The scaling curve**, 1 / 2 / 4 / 8 workers over the same tile list, by spinning up
//    extra pools. This is the figure that is easiest to inherit and hardest to trust.
// 4. **Frame deltas from `requestAnimationFrame`**, with the camera at a coast. Reported as
//    NOT EXERCISED if the page has never rendered -- Task 4 was caught by a depth check
//    passing trivially on a page with a zero-sized canvas, and a frame-time figure from a
//    render loop that never ran would be the same trap with a worse number in it.

import { TilePool, summarise } from "./pool.js";

/// Deep ocean: every post below this. Well clear of the shelf.
const DEEP_M = -2000;

/// Find level-`level` tiles of each kind, by scanning the field coarsely and then filling
/// candidates and classifying from what came back.
///
/// The scan is on the main thread and deliberately coarse -- it only has to nominate
/// candidates. The classification is from the tile's own 4,225 posts.
///
/// **The coastal nomination bisects.** A 3-degree grid step is 330 km, so "the sign changed
/// between these two samples" nominates a tile that is usually nowhere near the shore: a
/// first cut of this produced a coastal population of **4 tiles out of 480**, which is not
/// a population, and a coastal-versus-ocean split quoted from it would have been noise. So
/// a sign change is followed by twenty bisections, which lands within about 300 m of the
/// zero crossing -- inside a single level-12 tile.
export async function classifyTiles({ engine, world, provider, level, perClass = 96, stepDeg = 3 }) {
  const wb = provider.worldbuilder;
  const seen = new Set();
  const candidates = [];
  const resolution = wb.postSpacingM(6);
  const at = (lat, lon) => engine.elevationM(world, lat, lon, resolution);

  /// Walk to the zero crossing between two points of opposite sign.
  function bisect(aLat, aLon, bLat, bLon) {
    let lo = [aLat, aLon];
    let hi = [bLat, bLon];
    const loLand = at(lo[0], lo[1]) > 0;
    for (let i = 0; i < 20; i += 1) {
      const mid = [(lo[0] + hi[0]) / 2, (lo[1] + hi[1]) / 2];
      if ((at(mid[0], mid[1]) > 0) === loLand) lo = mid;
      else hi = mid;
    }
    return [(lo[0] + hi[0]) / 2, (lo[1] + hi[1]) / 2];
  }

  const nominate = (latitudeDeg, longitudeDeg, hint) => {
    const carto = Cesium.Cartographic.fromDegrees(
      ((((longitudeDeg + 180) % 360) + 360) % 360) - 180, latitudeDeg,
    );
    const { x, y } = wb.tilingScheme.positionToTileXY(carto, level);
    const key = `${x}/${y}`;
    if (seen.has(key)) return;
    seen.add(key);
    candidates.push({ x, y, level, hint });
  };

  for (let latitudeDeg = -80; latitudeDeg <= 80; latitudeDeg += stepDeg) {
    for (let longitudeDeg = -180; longitudeDeg < 180; longitudeDeg += stepDeg) {
      const here = at(latitudeDeg, longitudeDeg);
      const east = at(latitudeDeg, longitudeDeg + stepDeg);
      const north = at(latitudeDeg + stepDeg, longitudeDeg);
      if ((here > 0) !== (east > 0)) {
        const [lat, lon] = bisect(latitudeDeg, longitudeDeg, latitudeDeg, longitudeDeg + stepDeg);
        nominate(lat, lon, "coastal");
      } else if ((here > 0) !== (north > 0)) {
        const [lat, lon] = bisect(latitudeDeg, longitudeDeg, latitudeDeg + stepDeg, longitudeDeg);
        nominate(lat, lon, "coastal");
      } else {
        nominate(latitudeDeg, longitudeDeg, here > 0 ? "land" : "ocean");
      }
    }
  }
  // Take a bounded number of each hint, spread through the list rather than clustered at
  // one end of it -- a stride, not a slice.
  const chosen = [];
  for (const hint of ["coastal", "land", "ocean"]) {
    const of = candidates.filter((c) => c.hint === hint);
    const stride = Math.max(1, Math.floor(of.length / perClass));
    for (let i = 0; i < of.length && chosen.filter((c) => c.hint === hint).length < perClass; i += stride) {
      chosen.push(of[i]);
    }
  }
  return chosen;
}

/// Fill every tile in `tiles` through the pool, all dispatched before any is awaited, and
/// return per-tile timings classified by what the heights turned out to be.
///
/// The cache is bypassed on purpose: this measures the cost of a *miss*, which is the cost
/// that can drop a frame.
export async function timePoolFills({ provider, pool, tiles }) {
  const wb = provider.worldbuilder;
  const started = performance.now();
  const results = await Promise.all(tiles.map((tile) => {
    const request = wb.tileRequest(tile.x, tile.y, tile.level);
    return pool.fill(request).then((r) => ({ tile, ...r }));
  }));
  const wallMs = performance.now() - started;
  return { wallMs, samples: results.map((r) => ({ tile: r.tile, ms: r.fillMs, heights: r.heights })) };
}

/// The Task 4 path: fill on the main thread, one at a time.
export function timeMainThreadFills({ provider, engine, world, tiles, warmUp = true }) {
  const wb = provider.worldbuilder;
  // One discarded pass, for the same reason the scaling curve takes one: the first run
  // through a code path measures the compiler tiering up as much as it measures the work.
  if (warmUp) {
    for (const tile of tiles.slice(0, Math.min(tiles.length, 64))) {
      engine.fillTileF32({ ...wb.tileRequest(tile.x, tile.y, tile.level), handle: world });
    }
  }
  const samples = [];
  const started = performance.now();
  for (const tile of tiles) {
    const request = { ...wb.tileRequest(tile.x, tile.y, tile.level), handle: world };
    const at = performance.now();
    const heights = engine.fillTileF32(request);
    samples.push({ tile, ms: performance.now() - at, heights });
  }
  return { wallMs: performance.now() - started, samples };
}

/// Split a sample list into populations by what the heights say, not by the hint.
///
/// Four buckets, chosen so that **nothing lands in a leftover pile**. An earlier cut of
/// this used `land`, `ocean` and `coastal` with a gap between them, and 40% of the tiles
/// fell into "other" -- which is not a population, it is an admission that the split was
/// wrong. The shelf is a real thing with its own cost, so it gets its own name.
export function splitByTerrain(samples) {
  const groups = { coastal: [], land: [], shelf: [], deep: [] };
  for (const sample of samples) {
    let min = Infinity;
    let max = -Infinity;
    for (const h of sample.heights) {
      if (h < min) min = h;
      if (h > max) max = h;
    }
    if (min <= 0 && max >= 0) groups.coastal.push(sample);
    else if (min > 0) groups.land.push(sample);
    else if (max <= DEEP_M) groups.deep.push(sample);
    else groups.shelf.push(sample);
  }
  const out = {};
  for (const [name, list] of Object.entries(groups)) {
    out[name] = summarise(list.map((s) => s.ms));
  }
  return out;
}

/// What the main thread pays per tile once the fill is elsewhere: the `slice()` handout.
export function timeHandouts(samples, repeats = 4) {
  const times = [];
  for (let r = 0; r < repeats; r += 1) {
    for (const sample of samples) {
      const at = performance.now();
      const copy = sample.heights.slice();
      times.push(performance.now() - at);
      if (copy.length !== sample.heights.length) throw new Error("slice lost data");
    }
  }
  return summarise(times);
}

/// Total wall clock, and per-tile cost, for the same tile list at 1, 2, 4 and 8 workers.
///
/// Fresh pools, because pool size is fixed at construction. Each pool pays its own
/// `Surface::new` per worker at start-up, which is excluded: `start` is awaited before the
/// clock begins, and the build times are reported separately.
///
/// **Each pool runs the list twice and the first pass is discarded.** A worker starts on
/// the baseline wasm compiler and tiers up while it works, so a cold pass measures the
/// compiler as much as the engine -- and the one-worker pass is the denominator of every
/// speedup below, so a cold one inflates the whole curve.
///
/// `fillMs` here is the *per-tile* cost **under that concurrency**, which is not the same
/// quantity as the serial main-thread cost: eight workers sharing memory bandwidth and a
/// turbo budget each take longer per tile than one worker alone. The wall clock is the
/// figure that answers "did this help"; the per-tile figure answers "why not more".
export async function scalingCurve({ provider, spec, tiles, counts = [1, 2, 4, 8] }) {
  const wb = provider.worldbuilder;
  const requests = tiles.map((t) => wb.tileRequest(t.x, t.y, t.level));
  const rows = [];
  for (const count of counts) {
    const pool = await TilePool.start({ count, spec });
    const buildMs = summarise(pool.ready.map((r) => r.buildMs));
    await Promise.all(requests.map((r) => pool.fill(r)));   // warm-up, discarded
    pool.fillMs.length = 0;
    pool.wallMs.length = 0;
    const started = performance.now();
    await Promise.all(requests.map((r) => pool.fill(r)));
    const wallMs = performance.now() - started;
    const fillMs = summarise(pool.fillMs);
    pool.terminate();
    rows.push({ workers: count, tiles: tiles.length, wallMs, fillMs, surfaceNewMs: buildMs });
  }
  const base = rows[0].wallMs;
  for (const row of rows) row.speedup = base / row.wallMs;
  return rows;
}

/// Frame deltas from the browser's own render loop.
///
/// **A page that has never rendered reports nothing.** `viewer.canvas.width === 0` or a
/// `frameNumber` that does not advance means the figure would be a lie, and this slice has
/// already shipped one check that passed on exactly that.
export async function frameTrace({ viewer, frames = 240 }) {
  if (!viewer || viewer.canvas.width === 0 || viewer.canvas.height === 0) {
    return { exercised: false, why: `canvas is ${viewer ? viewer.canvas.width : "?"} px wide` };
  }
  const startFrame = viewer.scene.frameState.frameNumber;
  const deltas = [];
  await new Promise((resolve) => {
    let previous = performance.now();
    let seen = 0;
    const step = () => {
      const now = performance.now();
      deltas.push(now - previous);
      previous = now;
      seen += 1;
      if (seen >= frames) resolve();
      else requestAnimationFrame(step);
    };
    requestAnimationFrame(step);
  });
  const advanced = viewer.scene.frameState.frameNumber - startFrame;
  return {
    exercised: advanced > 0,
    why: advanced > 0 ? null : "frameNumber did not advance during the trace",
    renderedFrames: advanced,
    // The first delta is the gap to the first callback, not a frame.
    deltaMs: summarise(deltas.slice(1)),
    over16ms: deltas.slice(1).filter((d) => d > 16.7).length,
  };
}

/// Everything, in one call.
export async function runBench({
  viewer,
  engine,
  provider,
  spec,
  level = null,
  perClass = 96,
  scaling = true,
  frames = 240,
} = {}) {
  const wb = provider.worldbuilder;
  const at = level ?? wb.maxLevel;
  const tiles = await classifyTiles({
    engine, world: wb.world, provider, level: at, perClass,
  });

  const main = timeMainThreadFills({ provider, engine, world: wb.world, tiles });
  const pooled = wb.pool ? await timePoolFills({ provider, pool: wb.pool, tiles }) : null;

  const report = {
    level: at,
    postSpacingM: wb.postSpacingM(at),
    tiles: tiles.length,
    mainThread: {
      wallMs: main.wallMs,
      all: summarise(main.samples.map((s) => s.ms)),
      byTerrain: splitByTerrain(main.samples),
    },
    workers: pooled ? {
      count: wb.pool.ready.length,
      wallMs: pooled.wallMs,
      all: summarise(pooled.samples.map((s) => s.ms)),
      byTerrain: splitByTerrain(pooled.samples),
      speedupVsMainThread: main.wallMs / pooled.wallMs,
    } : null,
    handoutMs: timeHandouts(main.samples),
    cache: wb.cache ? wb.cache.stats() : null,
    scaling: scaling ? await scalingCurve({ provider, spec, tiles }) : null,
    frames: await frameTrace({ viewer, frames }),
  };
  return report;
}

/// A human-readable rendering. Every line names its n.
export function formatBench(report) {
  const q = (s) => (s.n === 0 ? "n=0" :
    `n=${s.n} median ${s.median.toFixed(2)} p90 ${s.p90.toFixed(2)} max ${s.max.toFixed(2)} ms`);
  const lines = [
    `level ${report.level} (${report.postSpacingM.toFixed(2)} m posts), ${report.tiles} tiles`,
    `main thread, per tile:   ${q(report.mainThread.all)}   [${report.mainThread.wallMs.toFixed(0)} ms total]`,
  ];
  for (const [name, s] of Object.entries(report.mainThread.byTerrain)) {
    lines.push(`  ${name.padEnd(8)} ${q(s)}${s.n ? ` mean ${s.mean.toFixed(2)}` : ""}`);
  }
  if (report.workers) {
    lines.push(
      `workers (${report.workers.count}), per tile: ${q(report.workers.all)}` +
      `   [${report.workers.wallMs.toFixed(0)} ms total, ` +
      `${report.workers.speedupVsMainThread.toFixed(2)}x wall clock]`,
    );
    for (const [name, s] of Object.entries(report.workers.byTerrain)) {
      lines.push(`  ${name.padEnd(8)} ${q(s)}${s.n ? ` mean ${s.mean.toFixed(2)}` : ""}`);
    }
  }
  lines.push(`main-thread handout (slice): ${q(report.handoutMs)}`);
  if (report.cache) {
    lines.push(
      `cache: ${report.cache.size}/${report.cache.capacity} tiles, ` +
      `${report.cache.hits} hits, ${report.cache.misses} misses, ` +
      `${report.cache.evictions} evictions`,
    );
  }
  if (report.scaling) {
    for (const row of report.scaling) {
      lines.push(
        `  ${String(row.workers).padStart(2)} workers: ${row.wallMs.toFixed(0)} ms for ` +
        `${row.tiles} tiles, ${row.speedup.toFixed(2)}x; per tile ${q(row.fillMs)}; ` +
        `Surface::new median ${row.surfaceNewMs.median.toFixed(2)} ms`,
      );
    }
  }
  lines.push(report.frames.exercised
    ? `frames: ${report.frames.renderedFrames} rendered, ${q(report.frames.deltaMs)}, ` +
      `${report.frames.over16ms} over 16.7 ms`
    : `frames: NOT EXERCISED (${report.frames.why})`);
  return lines.join("\n");
}
