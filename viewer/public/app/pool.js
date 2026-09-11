//! The worker pool and the tile cache.
//
// Task 4 filled tiles synchronously on the main thread. A 65 x 65 tile measured **3.86 ms
// median but 18.20 ms at p90** there, and 16.7 ms is a whole frame at 60 Hz, so the tail
// was dropping frames on exactly the tiles a viewer looks at: a coastal tile costs up to
// 9x a deep-ocean one, because the shelf and detail systems do real work where the ground
// is interesting and short-circuit where it is not.
//
// Two mechanisms, and they are independent:
//
// - **The pool** moves the cost off the main thread. `CustomHeightmapTerrainProvider`'s
//   callback resolves whatever it returns, so handing back a promise needs no fight with
//   Cesium and no provider subclass.
// - **The cache** stops the cost recurring. Nothing here recomputes per frame; a tile is
//   filled once per page load and then copied.
//
// # The copy on the way out is not optional
//
// `HeightmapTerrainData` keeps the buffer it is given for the lifetime of the tile **and
// transfers it to a Cesium worker when upsampling a child from it**, which detaches it. A
// cache that handed the same `Float32Array` to Cesium twice would hand out a detached
// buffer the second time -- length 0, no error, a flat tile. So the cache holds a master
// copy that Cesium never sees, and every handout is `master.slice()`: 16,900 bytes for a
// 65 x 65 tile, microseconds, against a fill measured in milliseconds.

/// Faults on this side of the wire. `terrain.js` re-exports the whole set; these two are
/// named here because this is the file that has to implement them.
export const POOL_FAULTS = {
  /// Exactly one worker of the pool builds a world one seed away. Most tiles are right.
  /// This is the version-skew shape -- a worker that answers with a stale world -- and it
  /// is deliberately *partial*, because a check that only looks at the first tile passes.
  staleWorker: "stale-worker",
  /// The cache key drops the tile's x. Every tile in a row collides with its neighbours,
  /// so the cache confidently returns the wrong tile. A one-token typo, and the rendered
  /// globe stays plausible.
  cacheKey: "cache-key",
};

/// Default pool size. Eight workers measured **6.08x** against one on this machine
/// (900 -> 462 -> 250 -> 148 ms for 256 tiles at 1 / 2 / 4 / 8), which is the shape of a
/// CPU-bound job on a machine with enough cores and not a claim about any other machine.
export const DEFAULT_WORKERS = 8;

/// Default cache capacity, in tiles. 1,024 x 16,900 bytes is ~17 MB of master copies --
/// the same order as the ~40 MB the whole capped page was measured at, and far below the
/// heap climb an uncapped quadtree produces.
export const DEFAULT_CACHE_TILES = 1024;

/// An LRU cache of filled tiles, keyed by tile identity.
///
/// The value stored is a **promise** of the master `Float32Array`, not the array, so two
/// requests for the same tile in the same frame collapse into one fill instead of two. A
/// rejected fill is evicted, so a transient failure is retried rather than cached forever.
export class TileCache {
  constructor({ capacity = DEFAULT_CACHE_TILES, fault = null } = {}) {
    this.capacity = capacity;
    this.fault = fault;
    this.entries = new Map();
    this.hits = 0;
    this.misses = 0;
    this.evictions = 0;
  }

  /// Tile identity. Level, x and y -- all three, because there is exactly one world per
  /// page and nothing else distinguishes a tile.
  key(x, y, level) {
    if (this.fault === POOL_FAULTS.cacheKey) return `${level}/${y}`;
    return `${level}/${x}/${y}`;
  }

  /// Look up, or fill and remember. `produce` is called only on a miss.
  get(x, y, level, produce) {
    const key = this.key(x, y, level);
    const found = this.entries.get(key);
    if (found) {
      this.hits += 1;
      // Move to the most-recent end. `Map` iterates in insertion order, so delete-then-set
      // is the whole LRU.
      this.entries.delete(key);
      this.entries.set(key, found);
      return found;
    }
    this.misses += 1;
    const promise = produce().catch((error) => {
      this.entries.delete(key);
      throw error;
    });
    this.entries.set(key, promise);
    while (this.entries.size > this.capacity) {
      const oldest = this.entries.keys().next().value;
      this.entries.delete(oldest);
      this.evictions += 1;
    }
    return promise;
  }

  get size() {
    return this.entries.size;
  }

  stats() {
    return {
      size: this.size, capacity: this.capacity,
      hits: this.hits, misses: this.misses, evictions: this.evictions,
    };
  }
}

/// A pool of workers, each holding its own engine instance and its own world.
export class TilePool {
  constructor(workers, { spec, fault = null }) {
    this.workers = workers;
    this.spec = spec;
    this.fault = fault;
    this.pending = new Map();
    this.nextId = 1;
    this.outstanding = workers.map(() => 0);
    this.dispatched = workers.map(() => 0);
    this.cursor = 0;
    /// Per-worker `wb_world_count`, filled by `rebuild`. `null` until a live swap has happened;
    /// see `rebuild` for why the main thread's own count cannot stand in for it.
    this.worldCounts = null;
    /// Every worker-side fill duration, in order. The population the report quotes.
    this.fillMs = [];
    /// Main-thread time from `fill()` call to promise settle, per tile. Wall clock, so it
    /// includes queueing behind other tiles -- it is not what the main thread *blocks* for.
    this.wallMs = [];
    /// The same two, for relief rasters. **Kept separate on purpose.** A relief tile is
    /// 66,564 engine samples plus 65,536 texels of shading and a heightmap tile is 4,225
    /// samples; pooling them into one `fillMs` would produce a median that describes
    /// neither job, and this slice has already been misled once by a statistic quoted
    /// without its population.
    this.reliefMs = [];
    this.reliefWallMs = [];
    /// And the same two again for cloud rasters -- **the pool's third consumer**, kept separate
    /// for exactly the reason the relief samples are. A cloud tile is 16,384 texels of pure
    /// hash-noise and **no engine fill at all**; a relief tile is 66,564 engine samples plus
    /// 65,536 texels of shading. Pooling those into one median would describe neither, and the
    /// whole reason a third consumer can be added to a saturated pool is that these two
    /// populations are an order of magnitude apart -- which is only visible if they are counted
    /// apart.
    this.cloudMs = [];
    this.cloudWallMs = [];
    /// And again for the water solve -- **the pool's fourth consumer, and the only one that is
    /// not a tile.** Kept apart for the same reason as the other three and more so: one sample
    /// here is 33--53 SECONDS where a cloud tile is 71--76 ms, so a pooled median would be a
    /// number describing nothing at all. `n` is 1 per swap, not 72.
    this.waterMs = [];
    this.waterWallMs = [];
    /// And once more for the climate calibration -- **the pool's FIFTH consumer**, and the
    /// second that is not a tile. It is seconds where a cloud tile is milliseconds, for the
    /// same reason the water solve is, so pooling it into any of the four samples above would
    /// produce a median describing nothing.
    this.climateMs = [];
    this.climateWallMs = [];
    /// And once more for the hydro bake -- the studio's view-only water preview, and the
    /// only consumer this pool has that is neither a tile nor part of installing a world. One
    /// sample here is tens of seconds at a million nodes, the same order as the water solve,
    /// so pooling it into any of the samples above would produce a median describing nothing.
    this.hydroMs = [];
    this.hydroWallMs = [];
  }

  /// Start `count` workers and wait for every one to have built its world.
  ///
  /// All of them, not the first: a pool that answers before its last worker is ready would
  /// send a fill to a worker with `world === 0`, and handle 0 is the refusal value.
  static async start({
    count = DEFAULT_WORKERS,
    spec,
    fault = null,
    wasmUrl = "/wasm/worldbuilder_engine.wasm",
    workerUrl = "/app/tile-worker.js",
  }) {
    const workers = [];
    const readies = [];
    for (let index = 0; index < count; index += 1) {
      const worker = new Worker(workerUrl, { type: "module" });
      workers.push(worker);
      readies.push(new Promise((resolve, reject) => {
        const onFirst = (event) => {
          if (event.data.type === "ready") {
            worker.removeEventListener("message", onFirst);
            resolve(event.data);
          } else if (event.data.type === "error") {
            worker.removeEventListener("message", onFirst);
            reject(new Error(`worker ${index} init: ${event.data.message}`));
          }
        };
        worker.addEventListener("message", onFirst);
        worker.addEventListener("error", (e) => reject(new Error(`worker ${index}: ${e.message}`)));
      }));
      // The seed may be a string from a URL parameter; the worker calls `BigInt()` on
      // whatever arrives, so it is sent as-is rather than converted here.
      worker.postMessage({ type: "init", index, wasmUrl, fault, spec });
    }
    const ready = await Promise.all(readies);
    const pool = new TilePool(workers, { spec, fault });
    pool.ready = ready;
    for (const worker of workers) {
      worker.addEventListener("message", (event) => pool.receive(event.data));
    }
    return pool;
  }

  receive(message) {
    if (message.type !== "tile" && message.type !== "relief" && message.type !== "cloud"
      && message.type !== "water" && message.type !== "climate" && message.type !== "hydro"
      && message.type !== "error") return;
    const entry = this.pending.get(message.id);
    if (!entry) return;
    this.pending.delete(message.id);
    this.outstanding[entry.worker] -= 1;
    if (message.type === "error") {
      entry.reject(new Error(`worker ${message.index}: ${message.message}`));
      return;
    }
    // Which sample the duration belongs in is decided by the entry, not by the reply: the
    // dispatcher knows what it asked for, and a reply that could choose its own bucket
    // would let a mislabelled worker reply silently pollute the other job's statistics.
    entry.workMs.push(message.fillMs);
    entry.wallMs.push(performance.now() - entry.started);
    // Relief and cloud replies have the same raster shape, so they are resolved the same way.
    // They are still two message types rather than one: the DISPATCHER decides which sample a
    // duration lands in, and it can only do that if the reply it is matching says which job it
    // was -- a shared `raster` type would make the two indistinguishable at exactly the point
    // where the report needs them apart.
    if (message.type === "relief" || message.type === "cloud") {
      entry.resolve({
        data: message.data,
        width: message.width,
        height: message.height,
        fillMs: message.fillMs,
        worker: message.index,
        // **This object maps a fixed key set and discards the rest, and that cost a bug.** The
        // lake counters were added to the worker's relief reply and to the provider's stats at
        // the same time; both ends were right, both were tested, and the numbers arrived at the
        // browser as zero -- because this dispatcher rebuilt the reply from five named fields and
        // silently dropped the two new ones. The picture was correct throughout, which is exactly
        // why a counter was added in the first place, and it took a live `measure` run rather
        // than any unit test to see it. `?? 0` because a CLOUD reply legitimately carries
        // neither; `pool.test.mjs` now asserts that a relief reply's counts survive this hop.
        lakeTexels: message.lakeTexels ?? 0,
        lakeTiles: message.lakeTiles ?? 0,
        // **The third and fourth fields this fixed key set could have dropped**, and the
        // comment above is the reason they are named here rather than spread. `?? 0` because
        // a CLOUD reply carries neither, and because a relief reply with climate off carries
        // them as zero rather than as absent -- which is the same number and a different
        // fact, so `climateTiles` on the provider is what separates the two.
        climateMs: message.climateMs ?? 0,
        climateSamples: message.climateSamples ?? 0,
      });
      return;
    }
    // The climate calibration: a plain object of two small arrays and two scalars, rebuilt
    // field by field with the same hazard the water reply above records.
    if (message.type === "climate") {
      entry.resolve({
        moistureEdges: message.moistureEdges,
        landformEdges: message.landformEdges,
        landSamples: message.landSamples,
        lapseCPerKm: message.lapseCPerKm,
        fillMs: message.fillMs,
        worker: message.index,
        worldCount: message.worldCount,
      });
      return;
    }
    // **A water reply, and this is the hop the lake counters were silently dropped on.** Same
    // hazard, same shape: this dispatcher rebuilds every reply from a fixed key set and
    // discards the rest, so a field added at both ends and tested at both ends still arrives
    // as `undefined` here. `pool.test.mjs` asserts each of these four survives the hop, and
    // `seaLevelM` in particular has no visible consequence -- the datum only shows as lake
    // colour at a depth -- so nothing about the picture would report its loss.
    if (message.type === "water") {
      entry.resolve({
        bodies: message.bodies,
        seaLevelM: message.seaLevelM,
        fillMs: message.fillMs,
        worker: message.index,
        worldCount: message.worldCount,
      });
      return;
    }
    // A hydro reply: the schema-2 record `water-preview.js` decodes, plus the same
    // named-field rebuild every other job's reply goes through here.
    if (message.type === "hydro") {
      entry.resolve({ words: message.words, fillMs: message.fillMs, worker: message.index });
      return;
    }
    entry.resolve({ heights: message.heights, fillMs: message.fillMs, worker: message.index });
  }

  /// Least-outstanding dispatch, round-robin on a tie.
  ///
  /// Round-robin alone bunches: Cesium asks for a burst of tiles in one turn, and the
  /// coastal ones cost 9x the ocean ones, so a fixed rotation can leave one worker with
  /// four expensive tiles while another idles.
  pick() {
    let best = 0;
    let bestLoad = Infinity;
    for (let i = 0; i < this.workers.length; i += 1) {
      const index = (this.cursor + i) % this.workers.length;
      if (this.outstanding[index] < bestLoad) {
        bestLoad = this.outstanding[index];
        best = index;
      }
    }
    this.cursor = (best + 1) % this.workers.length;
    return best;
  }

  /// Send one job to the least-loaded worker and record its two durations.
  ///
  /// `type` is the worker's message type; `workMs`/`wallMs` are the samples this job's
  /// durations belong in. Both jobs share the dispatcher because both are the same
  /// contention: one engine instance per worker, and the queue depth is what decides
  /// whether a burst of tiles finishes in parallel or in series.
  dispatch(type, request, workMs, wallMs) {
    const worker = this.pick();
    const id = this.nextId;
    this.nextId += 1;
    this.outstanding[worker] += 1;
    this.dispatched[worker] += 1;
    return new Promise((resolve, reject) => {
      this.pending.set(id, {
        resolve, reject, worker, started: performance.now(), workMs, wallMs,
      });
      this.workers[worker].postMessage({ type, id, request });
    });
  }

  /// Fill one tile. Resolves `{ heights, fillMs, worker }`; `heights` is the master copy
  /// and must not be handed to Cesium without a `slice()`.
  fill(request) {
    return this.dispatch("fill", request, this.fillMs, this.wallMs);
  }

  /// Rasterise one relief tile. Resolves `{ data, width, height, fillMs, worker }`, where
  /// `data` is a `Uint8ClampedArray` of RGBA texels transferred out of the worker.
  ///
  /// # No cache here, and the reason is NOT the one this file used to give
  ///
  /// **The old reason was "an imagery tile is asked for once per layer lifetime", and that is
  /// now measurably false.** It was true before the live swap shipped. Assigning
  /// `viewer.terrainProvider` makes Cesium discard the entire quadtree, and every replacement
  /// `QuadtreeTile` re-requests imagery from *every* layer: 72 cloud tiles and a full set of
  /// relief tiles per slider release, on the owner's world at the orbital camera. The sentence
  /// is corrected rather than deleted, because it misled a reader once already.
  ///
  /// The real reason a relief raster is not cached is **world identity**: a relief tile is
  /// 66,564 engine samples against one world handle, and a live swap is precisely a change of
  /// that handle, so a raster kept across one would be the previous planet's colour over the
  /// new planet's mesh -- the `cache-key` fault by another route. `main.js` builds a NEW relief
  /// provider per swap for exactly this reason, and a cache would have to be keyed by world to
  /// survive one. **Clouds are the opposite case and ARE cached**, in `cloud-provider.js`: that
  /// job takes no world handle at all.
  relief(request) {
    return this.dispatch("relief", request, this.reliefMs, this.reliefWallMs);
  }

  /// Rasterise one cloud tile. Resolves `{ data, width, height, fillMs, worker }`, where `data`
  /// is a `Uint8ClampedArray` of RGBA texels transferred out of the worker.
  ///
  /// **The cache for these lives in `cloud-provider.js`, keyed `level/x/y`**, and not here.
  /// A cloud raster depends on `seed`, `cover`, `radiusM` and `tileSize` and on nothing this
  /// pool knows; the provider closure is where all four are fixed, so that is where a key of
  /// three numbers is a complete key. See `CloudRasterCache` for the measurement.
  cloud(request) {
    return this.dispatch("cloud", request, this.cloudMs, this.cloudWallMs);
  }

  /// **Calibrate this world's climate band edges in a worker.** Resolves
  /// `{ moistureEdges, landformEdges, landSamples, lapseCPerKm, fillMs, worker, worldCount }`.
  ///
  /// # The fifth consumer, and the second that is not a tile
  ///
  /// `wb_climate_calibration` is 4,000 elevations plus one upwind march at every land point.
  /// The noise calibration it replaces was 4,000 `wb_elevation_m` calls and ran on the main
  /// thread at construction in tens of milliseconds; this one is **seconds**, and a
  /// constructor that silently blocked a boot for that long would be a cost with no name in
  /// the status line -- which is the exact sentence `relief-provider.js` already writes about
  /// the water manifest. So it moves here, and `main.js` resolves it, times it and says so.
  ///
  /// One call, once per world. It is NOT per tile and not per worker: the edges are four plus
  /// two numbers that are identical in every worker, and a worker that calibrated its own
  /// would be a second place they could disagree.
  climate(request) {
    return this.dispatch("climate", request, this.climateMs, this.climateWallMs);
  }

  /// **Solve this world's water manifest in a worker.** Resolves
  /// `{ bodies, seaLevelM, fillMs, worker, worldCount }`.
  ///
  /// # The fourth consumer, and the only one that is not a tile
  ///
  /// `wb_water_run` was called on the MAIN THREAD and measured **33.9--44.8 s at 86,000 nodes
  /// at boot** and **45.4--53.4 s per live swap**, which the browser's own `longtask` observer
  /// recorded as a *single task* each time: one slider nudge froze the tab for three quarters
  /// of a minute. It is 55--77% of a cold load.
  ///
  /// **This is a message type, not a new algorithm.** `wb_water_run` is already an export,
  /// every worker already holds a world built from this same spec (`rebuild` below is what
  /// guarantees it, and `main.js` awaits it before dispatching this), and the solve is
  /// deterministic -- an identical rebuild was measured bit-identical. So the worker's answer
  /// is the main thread's answer, and `tile-worker.test.mjs` proves that against the real wasm
  /// rather than assuming it.
  ///
  /// **It occupies one worker for the whole solve**, and that is affordable because the pool
  /// was measured **starved, not saturated**: 23--40% utilisation, because Cesium will not
  /// request level n+1 until level n has landed. `pick()` is least-outstanding, so the other
  /// seven carry the tiles that arrive meanwhile.
  ///
  /// **The reply carries the worker's own `wb_world_count`.** A solve must not build a world,
  /// and this is the only place that can be seen: a handle table lives inside one instance's
  /// linear memory, so the main thread's count says nothing about a worker's.
  water(request) {
    return this.dispatch("water", request, this.waterMs, this.waterWallMs);
  }

  /// **Rebuild every worker's world from a new spec, reusing the engine instances.**
  ///
  /// This is the pool's half of the live swap. It is not `terminate()` plus `start()`: that would
  /// re-fetch and re-instantiate the wasm once per worker, which is the expensive half of boot
  /// and is exactly what a live swap exists to stop paying again.
  ///
  /// **All of them, and it waits for all of them** -- the same rule `start` follows and for the
  /// same reason. A pool that resolved after the first reply would have the main thread install a
  /// provider while seven workers were still filling tiles from the previous planet, which is the
  /// `stale-worker` fault arrived at by accident. Every reply is matched by its own id, so a tile
  /// reply landing mid-rebuild cannot be mistaken for one.
  ///
  /// The `rebuilt` reply carries each worker's own `wb_world_count`, and it is kept because it is
  /// the only place that figure exists: a world handle is an index into a table inside one
  /// instance's linear memory, so the main thread's count says nothing at all about the workers'.
  /// **Bake this world's hydrology in a worker.** Resolves `{ words, fillMs, worker }`, where
  /// `words` is the schema-2 record `water-preview.js` decodes.
  ///
  /// # The studio's view-only water preview, the pool's sixth consumer
  ///
  /// Mirrors `water()`: `wb_hydro_bake` is already an export, every worker already holds a
  /// world built from this same spec, and the bake is deterministic. It occupies one worker
  /// for the whole bake -- tens of seconds at a million nodes -- which is affordable for the
  /// same reason `water()`'s is: the pool is starved rather than saturated between tile
  /// bursts, and `pick()` sends the other seven tiles meanwhile.
  hydro(request) {
    return this.dispatch("hydro", request, this.hydroMs, this.hydroWallMs);
  }

  rebuild(spec) {
    this.spec = spec;
    const replies = this.workers.map((worker, index) => new Promise((resolve, reject) => {
      const id = this.nextId;
      this.nextId += 1;
      const onMessage = (event) => {
        const message = event.data;
        if (!message || message.id !== id) return;
        worker.removeEventListener("message", onMessage);
        if (message.type === "rebuilt") resolve(message);
        else reject(new Error(`worker ${index} rebuild: ${message.message}`));
      };
      worker.addEventListener("message", onMessage);
      worker.postMessage({ type: "rebuild", id, spec });
    }));
    return Promise.all(replies).then((rebuilt) => {
      // `ready` is what `stats()` and the status line report the pool from, so it has to describe
      // the world the workers are on NOW rather than the one they booted with. `stale` comes back
      // FROM the worker rather than being carried over here: the worker is the only side that
      // knows whether it applied the fault, and re-deriving it would be a second copy of that
      // decision.
      const previous = this.ready ?? [];
      this.ready = rebuilt.map((r) => ({
        type: "ready",
        index: r.index,
        world: r.world,
        stale: r.stale,
        generatorVersion: previous[r.index] ? previous[r.index].generatorVersion : undefined,
        buildMs: r.buildMs,
      }));
      /// Per-worker live-world counts after the swap. **This is the leak evidence for eight
      /// ninths of the wasm memory this feature touches** -- the main thread's `wb_world_count`
      /// cannot see any of it.
      this.worldCounts = rebuilt.map((r) => r.worldCount);
      return rebuilt;
    });
  }

  terminate() {
    for (const worker of this.workers) worker.terminate();
    this.workers = [];
  }

  stats() {
    return {
      workers: this.ready.length,
      staleWorkers: this.ready.filter((r) => r.stale).map((r) => r.index),
      /// `null` until the first live swap; an array of eight `wb_world_count` readings
      /// afterwards. Each must stay at 1: a worker holding two worlds is a worker leaking one
      /// per slider release.
      worldCounts: this.worldCounts,
      buildMs: this.ready.map((r) => r.buildMs),
      dispatched: this.dispatched.slice(),
      fills: this.fillMs.length,
      fillMs: summarise(this.fillMs),
      wallMs: summarise(this.wallMs),
      reliefs: this.reliefMs.length,
      reliefMs: summarise(this.reliefMs),
      reliefWallMs: summarise(this.reliefWallMs),
      clouds: this.cloudMs.length,
      cloudMs: summarise(this.cloudMs),
      cloudWallMs: summarise(this.cloudWallMs),
      /// One per solve: one at boot and one per world-class swap. `waterWallMs` minus
      /// `waterMs` is what the solve spent queued behind tiles, which is the figure that would
      /// say the pool had become the bottleneck.
      waters: this.waterMs.length,
      waterMs: summarise(this.waterMs),
      waterWallMs: summarise(this.waterWallMs),
      climates: this.climateMs.length,
      climateMs: summarise(this.climateMs),
      climateWallMs: summarise(this.climateWallMs),
      hydros: this.hydroMs.length,
      hydroMs: summarise(this.hydroMs),
      hydroWallMs: summarise(this.hydroWallMs),
    };
  }
}

/// Median, p90, max and mean of a sample, with its population size stated.
///
/// The population is part of the answer, not decoration: "p90 18.2 ms" over eleven samples
/// and over eleven hundred are different claims, and this slice has already been misled
/// once by a number quoted without its n.
export function summarise(values) {
  if (values.length === 0) return { n: 0 };
  const sorted = [...values].sort((a, b) => a - b);
  const at = (q) => sorted[Math.min(sorted.length - 1, Math.floor(q * sorted.length))];
  return {
    n: sorted.length,
    min: sorted[0],
    median: at(0.5),
    p90: at(0.9),
    p99: at(0.99),
    max: sorted[sorted.length - 1],
    mean: sorted.reduce((a, b) => a + b, 0) / sorted.length,
  };
}
