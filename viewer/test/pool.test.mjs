// Node-native tests for the dispatcher half of pool.js.
//
// `TilePool.start` needs `Worker`, but `TilePool` itself does not: its constructor takes a
// list of objects with `postMessage`, and `receive` is a plain method that the browser's
// message listener calls. So the dispatch, the routing and the two statistic samples are
// all testable here against fake workers, and only the `new Worker(...)` line is not.
//
// **What these tests are for.** Task 4 added a second job (`relief`) alongside `fill`.
// Two things about that are silent when wrong: a reply that settles the wrong promise (both
// jobs share one `pending` map and one id counter), and a duration recorded in the other
// job's sample. A heightmap tile is 4,225 engine samples and a relief tile is 66,564 plus
// 65,536 texels of shading, so a mixed sample produces a median that describes neither, and
// the number still looks reasonable.

import test from "node:test";
import assert from "node:assert/strict";

const { TilePool, TileCache, summarise } = await import("../public/app/pool.js");

/// A worker that records what it was posted and answers only when told to.
function fakeWorker() {
  return { sent: [], postMessage(message) { this.sent.push(message); } };
}

function makePool(count = 2) {
  const workers = [];
  for (let i = 0; i < count; i += 1) workers.push(fakeWorker());
  const pool = new TilePool(workers, { spec: { seed: 1 } });
  pool.ready = workers.map((_, index) => ({ index, stale: false, buildMs: 1 }));
  return pool;
}

test("relief() posts type 'relief', and fill() still posts type 'fill'", () => {
  const pool = makePool(1);
  pool.relief({ size: 256 });
  pool.fill({ width: 65 });
  assert.deepEqual(
    pool.workers[0].sent.map((m) => m.type), ["relief", "fill"],
    "the worker matches on message.type; a wrong name falls through to the unknown-type " +
    "error and the tile is never drawn",
  );
  assert.deepEqual(pool.workers[0].sent[0].request, { size: 256 });
  assert.notEqual(
    pool.workers[0].sent[0].id, pool.workers[0].sent[1].id,
    "both jobs share one id counter; a collision would settle the wrong promise",
  );
});

test("a relief reply resolves with the raster, and a fill reply with the heights", async () => {
  const pool = makePool(1);
  const reliefPromise = pool.relief({ size: 2 });
  const fillPromise = pool.fill({ width: 2 });
  const [reliefId, fillId] = pool.workers[0].sent.map((m) => m.id);

  // Deliberately answered OUT OF ORDER. Cesium asks for a burst of both kinds at once and
  // the pool is least-outstanding, so replies genuinely do interleave.
  pool.receive({
    type: "tile", id: fillId, index: 0, fillMs: 4, heights: new Float32Array([1, 2, 3, 4]),
  });
  pool.receive({
    type: "relief", id: reliefId, index: 0, fillMs: 190,
    data: new Uint8ClampedArray(16), width: 2, height: 2,
  });

  const relief = await reliefPromise;
  const fill = await fillPromise;
  assert.equal(relief.width, 2);
  assert.equal(relief.height, 2);
  assert.equal(relief.data.length, 16);
  assert.equal(relief.fillMs, 190);
  assert.equal(fill.heights.length, 4);
  assert.equal(fill.fillMs, 4);
});

test("each job's duration lands in ITS OWN sample, not the other's", async () => {
  // A relief tile costs roughly 45x a heightmap tile. Pooling the two would leave `fillMs`
  // describing neither job while still looking like a plausible number, which is precisely
  // the failure this slice has already been misled by once.
  const pool = makePool(1);
  const reliefPromise = pool.relief({ size: 2 });
  const fillPromise = pool.fill({ width: 2 });
  const [reliefId, fillId] = pool.workers[0].sent.map((m) => m.id);
  pool.receive({
    type: "relief", id: reliefId, index: 0, fillMs: 190,
    data: new Uint8ClampedArray(4), width: 1, height: 1,
  });
  pool.receive({ type: "tile", id: fillId, index: 0, fillMs: 4, heights: new Float32Array(1) });
  await Promise.all([reliefPromise, fillPromise]);

  assert.deepEqual(pool.reliefMs, [190], "the relief duration belongs to reliefMs");
  assert.deepEqual(pool.fillMs, [4], "the heightmap duration belongs to fillMs");
  const stats = pool.stats();
  assert.equal(stats.fills, 1);
  assert.equal(stats.reliefs, 1);
  assert.equal(stats.reliefMs.n, 1);
  assert.equal(stats.reliefMs.median, 190);
  assert.equal(stats.fillMs.median, 4);
});

test("an error reply rejects the right promise and frees the worker slot", async () => {
  const pool = makePool(1);
  const promise = pool.relief({ size: 2 });
  const id = pool.workers[0].sent[0].id;
  assert.equal(pool.outstanding[0], 1);
  pool.receive({ type: "error", id, index: 0, message: "boom" });
  await assert.rejects(promise, /worker 0: boom/);
  assert.equal(
    pool.outstanding[0], 0,
    "a worker that stayed 'outstanding' after a failure would be starved of work forever",
  );
  assert.equal(pool.reliefMs.length, 0, "a failed job contributes no duration");
});

test("relief work is spread by least-outstanding, like fills", () => {
  const pool = makePool(4);
  for (let i = 0; i < 4; i += 1) pool.relief({ size: 256 });
  assert.deepEqual(
    pool.workers.map((w) => w.sent.length), [1, 1, 1, 1],
    "four relief tiles must reach four workers; serialising them onto one is the whole " +
    "cost this task exists to remove, moved rather than removed",
  );
});

test("relief is NOT memoised -- the decision not to cache rasters, written down", () => {
  // 1,024 masters of 65 x 65 float heights is ~17 MB, which is what `TileCache` was sized
  // for; 1,024 masters of 256 x 256 RGBA is 268 MB. `ImageryLayer` caches the uploaded
  // texture itself, so the hit rate a relief cache would serve is near zero, and the
  // heightmap cache exists for a reason imagery does not share (Cesium re-asks for a parent
  // tile whenever it upsamples a child).
  //
  // This is a real assertion, not a restatement: if a cache is added later it fails, and
  // the memory arithmetic above is what has to be answered.
  const pool = makePool(1);
  pool.relief({ size: 256, level: 2, x: 0, y: 0 });
  pool.relief({ size: 256, level: 2, x: 0, y: 0 });
  assert.equal(
    pool.workers[0].sent.length, 2,
    "two identical relief requests must both be dispatched; memoising 262 kB rasters at " +
    "TileCache's 1,024-tile capacity is 268 MB of masters for a near-zero hit rate",
  );
  // And the heightmap cache is untouched by any of this.
  const cache = new TileCache({ capacity: 2 });
  assert.equal(cache.key(3, 4, 5), "5/3/4");
  assert.equal(summarise([]).n, 0, "an empty sample reports n, not a fabricated median");
});
