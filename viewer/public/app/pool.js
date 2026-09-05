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
    /// Every worker-side fill duration, in order. The population the report quotes.
    this.fillMs = [];
    /// Main-thread time from `fill()` call to promise settle, per tile. Wall clock, so it
    /// includes queueing behind other tiles -- it is not what the main thread *blocks* for.
    this.wallMs = [];
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
    if (message.type !== "tile" && message.type !== "error") return;
    const entry = this.pending.get(message.id);
    if (!entry) return;
    this.pending.delete(message.id);
    this.outstanding[entry.worker] -= 1;
    if (message.type === "error") {
      entry.reject(new Error(`worker ${message.index}: ${message.message}`));
      return;
    }
    this.fillMs.push(message.fillMs);
    this.wallMs.push(performance.now() - entry.started);
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

  /// Fill one tile. Resolves `{ heights, fillMs, worker }`; `heights` is the master copy
  /// and must not be handed to Cesium without a `slice()`.
  fill(request) {
    const worker = this.pick();
    const id = this.nextId;
    this.nextId += 1;
    this.outstanding[worker] += 1;
    this.dispatched[worker] += 1;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject, worker, started: performance.now() });
      this.workers[worker].postMessage({ type: "fill", id, request });
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
      buildMs: this.ready.map((r) => r.buildMs),
      dispatched: this.dispatched.slice(),
      fills: this.fillMs.length,
      fillMs: summarise(this.fillMs),
      wallMs: summarise(this.wallMs),
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
