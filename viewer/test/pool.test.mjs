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
    data: new Uint8ClampedArray(16), width: 2, height: 2, lakeTexels: 37, lakeTiles: 1,
  });

  const relief = await reliefPromise;
  const fill = await fillPromise;
  assert.equal(relief.width, 2);
  assert.equal(relief.height, 2);
  assert.equal(relief.data.length, 16);
  assert.equal(relief.fillMs, 190);
  // **The dispatcher rebuilds this object from named fields, so anything not named is dropped.**
  // That is the silently-dropping-builder shape, and it did drop these two: the worker counted
  // them, the provider was ready to accumulate them, and the browser reported zero lake texels
  // over a globe that was visibly drawing lakes. The picture cannot catch this; only the counter
  // can, and only if the counter survives the hop.
  assert.equal(relief.lakeTexels, 37, "the worker's lake texel count was dropped by the pool");
  assert.equal(relief.lakeTiles, 1, "the worker's lake tile count was dropped by the pool");
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

// =========================================================================================
// The water job -- the pool's fourth consumer, and the only one that is not a tile
// =========================================================================================

test("water() posts type 'water' with its request, and keeps its own sample", async () => {
  const pool = makePool(1);
  const waterPromise = pool.water({ nodeCount: 4000 });
  const cloudPromise = pool.cloud({ size: 2 });
  const [waterId, cloudId] = pool.workers[0].sent.map((m) => m.id);
  assert.deepEqual(
    pool.workers[0].sent.map((m) => m.type), ["water", "cloud"],
    "the worker matches on message.type; a wrong name falls through to the unknown-type error " +
    "and the manifest never arrives, which hangs installWorld forever",
  );
  assert.deepEqual(pool.workers[0].sent[0].request, { nodeCount: 4000 });
  pool.receive({
    type: "water", id: waterId, index: 0, fillMs: 31418,
    bodies: [{ rootNode: 7, kind: 0, levelM: 12.5 }], seaLevelM: -3.25, worldCount: 1,
  });
  pool.receive({
    type: "cloud", id: cloudId, index: 0, fillMs: 74,
    data: new Uint8ClampedArray(4), width: 1, height: 1,
  });
  await Promise.all([waterPromise, cloudPromise]);
  // **One solve is 31,418 ms and one cloud tile is 74 ms.** A shared sample would produce a
  // median describing neither, and a "median tile cost" of fifteen seconds would be believed by
  // nobody -- which is the good case. The bad case is the one where it looks plausible.
  assert.deepEqual(pool.waterMs, [31418], "the solve's duration belongs to waterMs");
  assert.deepEqual(pool.cloudMs, [74], "the cloud duration belongs to cloudMs");
  const stats = pool.stats();
  assert.equal(stats.waters, 1);
  assert.equal(stats.clouds, 1);
  assert.equal(stats.waterMs.median, 31418);
  assert.equal(stats.cloudMs.median, 74);
});

test("a water reply's manifest, datum and world count all survive the dispatcher", async () => {
  // **The dispatcher rebuilds every reply from a fixed key set and discards the rest.** That is
  // how the relief lake counters arrived as zero: right at the worker, right at the provider,
  // dropped in between, with a correct picture throughout. All four of these are that shape.
  // `seaLevelM` is the worst of them -- it only shows as lake colour at a depth, so a manifest
  // that arrived with `undefined` would draw an entirely plausible planet.
  const pool = makePool(1);
  const promise = pool.water({ nodeCount: 8000 });
  const id = pool.workers[0].sent[0].id;
  const bodies = [
    { rootNode: 3, kind: 0, levelM: 1204.5, minLatitudeDeg: 1, maxLatitudeDeg: 2,
      minLongitudeDeg: 3, maxLongitudeDeg: 4 },
    { rootNode: 9, kind: 0, levelM: -55.25, minLatitudeDeg: -2, maxLatitudeDeg: -1,
      minLongitudeDeg: -4, maxLongitudeDeg: -3 },
  ];
  pool.receive({
    type: "water", id, index: 5, fillMs: 2310, bodies, seaLevelM: -12.5, worldCount: 1,
  });
  const result = await promise;
  assert.deepEqual(result.bodies, bodies, "the manifest itself was dropped or reshaped");
  assert.equal(result.seaLevelM, -12.5, "the datum was dropped by the pool");
  assert.equal(result.worldCount, 1, "the worker's wb_world_count was dropped by the pool");
  assert.equal(result.fillMs, 2310);
  assert.equal(result.worker, 5, "which worker answered is what the status line reports");
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

// -------------------------------------------------------------------------------------------
// THE THIRD CONSUMER: clouds.
//
// The pool now carries three jobs, and the reason each keeps its own duration sample is the
// same reason `reliefMs` was split from `fillMs` one slice ago, only stronger: the three
// populations are an order of magnitude apart in cost (a heightmap tile is 4,225 engine
// samples; a relief tile is 66,564 plus 65,536 texels of shading; a cloud tile is 16,384 texels
// of pure hash noise and NO engine fill at all). A single pooled median would describe none of
// them, and the whole argument for adding a third consumer to a saturated pool rests on knowing
// which of the three a millisecond belongs to.

test("cloud() posts type 'cloud', and the other two jobs still post their own", () => {
  const pool = makePool(1);
  pool.cloud({ size: 128 });
  pool.relief({ size: 256 });
  pool.fill({ width: 65 });
  assert.deepEqual(
    pool.workers[0].sent.map((m) => m.type), ["cloud", "relief", "fill"],
    "the worker matches on message.type; a wrong name falls through to the unknown-type error " +
    "and the cloud tile is never drawn -- which looks exactly like a planet with no weather",
  );
  assert.deepEqual(pool.workers[0].sent[0].request, { size: 128 });
  const ids = pool.workers[0].sent.map((m) => m.id);
  assert.equal(new Set(ids).size, 3, "all three jobs share one id counter; a collision settles the wrong promise");
});

test("a cloud reply resolves with the raster", async () => {
  const pool = makePool(1);
  const promise = pool.cloud({ size: 2 });
  const [id] = pool.workers[0].sent.map((m) => m.id);
  pool.receive({
    type: "cloud", id, index: 1, fillMs: 17,
    data: new Uint8ClampedArray(16), width: 2, height: 2,
  });
  const cloud = await promise;
  assert.equal(cloud.width, 2);
  assert.equal(cloud.height, 2);
  assert.equal(cloud.data.length, 16);
  assert.equal(cloud.fillMs, 17);
  assert.equal(cloud.worker, 1);
});

test("all three durations land in their OWN samples", async () => {
  const pool = makePool(1);
  const cloudPromise = pool.cloud({ size: 2 });
  const reliefPromise = pool.relief({ size: 2 });
  const fillPromise = pool.fill({ width: 2 });
  const [cloudId, reliefId, fillId] = pool.workers[0].sent.map((m) => m.id);
  pool.receive({ type: "cloud", id: cloudId, index: 0, fillMs: 17, data: new Uint8ClampedArray(4), width: 1, height: 1 });
  pool.receive({ type: "relief", id: reliefId, index: 0, fillMs: 190, data: new Uint8ClampedArray(4), width: 1, height: 1 });
  pool.receive({ type: "tile", id: fillId, index: 0, fillMs: 4, heights: new Float32Array(1) });
  await Promise.all([cloudPromise, reliefPromise, fillPromise]);

  assert.deepEqual(pool.cloudMs, [17]);
  assert.deepEqual(pool.reliefMs, [190]);
  assert.deepEqual(pool.fillMs, [4]);
  const stats = pool.stats();
  assert.equal(stats.clouds, 1);
  assert.equal(stats.reliefs, 1);
  assert.equal(stats.fills, 1);
  assert.equal(stats.cloudMs.median, 17);
  assert.equal(stats.reliefMs.median, 190);
  assert.equal(stats.fillMs.median, 4);
});

test("a mislabelled reply cannot choose its own bucket", async () => {
  // The dispatcher decides which sample a duration belongs in, from what it ASKED for, not from
  // what came back. So a worker that answered a cloud request with `type: "relief"` still has
  // its duration recorded against the cloud sample -- which is what stops one job's statistics
  // being polluted by another's mislabelling, and it is the property `pool.js`'s own comment
  // claims.
  const pool = makePool(1);
  const promise = pool.cloud({ size: 2 });
  const [id] = pool.workers[0].sent.map((m) => m.id);
  pool.receive({ type: "relief", id, index: 0, fillMs: 190, data: new Uint8ClampedArray(4), width: 1, height: 1 });
  await promise;
  assert.deepEqual(pool.cloudMs, [190], "the duration belongs to the job that was dispatched");
  assert.deepEqual(pool.reliefMs, []);
});

test("cloud work is spread by least-outstanding, like the other two", () => {
  const pool = makePool(4);
  for (let i = 0; i < 4; i += 1) pool.cloud({ size: 128 });
  assert.deepEqual(
    pool.dispatched, [1, 1, 1, 1],
    "four cloud tiles must reach four workers; serialising them onto one would put the third " +
    "consumer's whole cost on a single core",
  );
});

test("the three jobs share ONE queue, so a cloud burst queues behind relief rather than beside it", () => {
  // One pool and not three, and this is what that decision means in practice: the contention is
  // engine instances per core, and dispatching clouds on their own rotation would let eight
  // cloud tiles and eight relief tiles land on the same eight cores at once.
  const pool = makePool(2);
  pool.relief({ size: 256 });
  pool.relief({ size: 256 });
  pool.cloud({ size: 128 });
  pool.cloud({ size: 128 });
  assert.deepEqual(pool.dispatched, [2, 2]);
  assert.deepEqual(pool.outstanding, [2, 2]);
});

test("clouds are NOT memoised either", () => {
  const pool = makePool(1);
  pool.cloud({ size: 128, level: 2 });
  pool.cloud({ size: 128, level: 2 });
  assert.equal(pool.workers[0].sent.length, 2);
});
