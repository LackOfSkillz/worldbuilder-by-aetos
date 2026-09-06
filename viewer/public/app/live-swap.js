//! Swapping a world **in place**, under a camera that does not move.
//
// # Why this file exists
//
// Every world knob on the panel used to do `location.search = q.toString()`. That is a full page
// reload: it throws away the engine module, nine wasm instances (the main thread's and one per
// worker), the biome calibration and the water solve, and then rebuilds all of them -- and it
// puts the camera back where the URL says rather than where the owner left it. The owner asked
// to "slide them and watch them redraw on the planet I have", and a reload is by construction
// not that.
//
// This module is the DECISION half of the replacement: what a change of parameters actually
// costs, what has to be rebuilt, and what may be kept. The DOING half is `main.js`'s
// `installWorld`, which reads a plan from here and executes it. They are separate because the
// decision is the part with a measured answer, and a decision that can only be exercised through
// a browser is a decision no test can pin.
//
// # THE MEASUREMENT THAT DECIDED THE SHAPE, AND IT IS NOT THE ONE THAT WAS EXPECTED
//
// The obvious optimisation is: *mountain sliders change terrain, lake sliders change water, so a
// mountain slide should skip the 25.5 s water solve.* **That is false, and it was measured
// rather than argued.**
//
// `wb_water_run` samples a stream graph **off the world's own surface** -- basin fill, overflow
// resolution and the tied-plateau merge are all functions of the elevations it reads. Move a
// mountain and you move the drainage. Population: the owner's world (seed 636659598, radius
// 9,309,000 m, 20 plates, land 0.40, `ranges` tectonic and `fractal` coast presets), one water
// solve per variant at 8,000 nodes, rows compared field by field against the baseline's 74
// bodies. Host: node 22, this repository's checked-in `worldbuilder_engine.wasm`.
//
// ```text
//   variant                              bodies   new roots   shared roots moved   max level delta
//   identical rebuild (the control)         74         0              0                0.000 m
//   tectonics.continentCollisionM x1.5      74         2              3               43.186 m
//   tectonics.continentalBlend -20%         72         0              7               43.186 m
//   tectonics.structureDepth +0.1           70         0              1               12.229 m
//   coast.amplitude x1.5                    71         7             25               55.293 m
//   relief.mountainM x1.5                   78         6             62               16.698 m
// ```
//
// The control is bit-identical, so the solve is deterministic and re-solving is safe. Every other
// row is a different manifest -- **including the pure mountain-height slider, which deletes and
// creates lakes.** A swap that kept the old manifest would draw the old world's lakes on the new
// world's ground, and the resulting picture would NOT equal a fresh load of the same URL. That is
// the one bar this feature is not allowed to fail, so the water solve is re-run whenever the
// world changes, and `waterSolveIsOptional` below is the single place that rule is written.
//
// **The asymmetry is real, it is just the other way round.** The cheap class is the LAKE slider,
// not the mountain one: `lakeNodes` changes the stream graph's resolution and nothing about the
// surface, so the world handles, the nine engine instances and every cached terrain tile survive
// it untouched. A mountain slide is the expensive class. `swapPlan` says which is which and
// `main.js` skips exactly the work the plan says can be skipped.

/// The spec fields that decide the SURFACE. Any of these moving means nine new worlds (the main
/// thread's and one per worker), a new terrain provider and a cold tile cache.
///
/// `features` and the three parameter blocks are compared structurally rather than by identity,
/// because `main.js` rebuilds them from the query string on every call and two equal blocks are
/// never the same object.
export const WORLD_FIELDS = [
  "seed", "radiusM", "plateCount", "landFraction", "features", "relief", "tectonics", "coast",
];

/// The fields that decide the WATER manifest and nothing else. Deliberately short: everything
/// else that moves the manifest does so *through* the surface, and is in `WORLD_FIELDS`.
export const WATER_FIELDS = ["waterNodes", "waterEnabled"];

/// The fields that decide the TILING -- how the surface is sampled, not what it is. They need a
/// new terrain provider and a cold cache, but no new world and no new water.
export const TILING_FIELDS = ["size", "maxLevel", "featureCeiling"];

/// Structural equality over the JSON-shaped values a spec carries: numbers, strings, `null`,
/// arrays of flat objects, flat objects. Deliberately not a deep-equal library and deliberately
/// not `JSON.stringify` comparison -- key order would then decide the answer, and `tectonicToParams`
/// and the preset reader build their blocks in different orders.
export function sameValue(a, b) {
  if (a === b) return true;
  if (a === null || b === null || a === undefined || b === undefined) return false;
  if (typeof a !== "object" || typeof b !== "object") {
    // `NaN` is not a value any spec field should hold, and treating it as equal to itself here
    // would hide a broken parse rather than surface it.
    return false;
  }
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  if (Array.isArray(a)) {
    if (a.length !== b.length) return false;
    return a.every((item, i) => sameValue(item, b[i]));
  }
  const keys = new Set([...Object.keys(a), ...Object.keys(b)]);
  for (const key of keys) if (!sameValue(a[key], b[key])) return false;
  return true;
}

/// The fields of `fields` on which the two specs disagree, in the order given.
export function changedFields(previous, next, fields) {
  return fields.filter((f) => !sameValue(previous[f], next[f]));
}

/// **Whether the water solve may be skipped, in one place.**
///
/// It may be skipped only when the surface did not move AND the water knobs did not move. It may
/// NOT be skipped merely because the water knobs did not move -- that is the false optimisation
/// the module doc measures, and writing the rule as a named function is what stops it being
/// re-invented as an `if` in `main.js`.
export function waterSolveIsOptional({ worldChanged, waterChanged }) {
  return !worldChanged && !waterChanged;
}

/// What a swap from `previous` to `next` has to do.
///
/// `kind` is the class the report quotes a cost for. The three that can occur:
///
/// - `"none"`         nothing moved; the caller should do nothing at all.
/// - `"water"`        the cheap class. The surface is untouched, so nine world handles, nine wasm
///                    instances and every cached terrain tile survive. Cost is the solve plus an
///                    imagery re-raster.
/// - `"world"`        the expensive class. New worlds everywhere, a cold tile cache, and -- unless
///                    lakes are off entirely -- the water solve as well, for the measured reason
///                    in the module doc.
/// - `"tiling"`       a new terrain provider over the SAME world: sampling changed, the surface
///                    did not. No world rebuild and no solve.
/// `previous` and `next` are the whole installed state: `{ spec, waterNodes, waterEnabled, size,
/// maxLevel, featureCeiling }`. The world fields live inside `spec` and the rest live beside it,
/// which is why the two comparisons below read from different objects.
export function swapPlan(previous, next) {
  const world = changedFields(previous.spec, next.spec, WORLD_FIELDS);
  const water = changedFields(previous, next, WATER_FIELDS);
  const tiling = changedFields(previous, next, TILING_FIELDS);
  const worldChanged = world.length > 0;
  const waterChanged = water.length > 0;
  const changed = [...world, ...water, ...tiling];

  if (changed.length === 0) {
    return {
      kind: "none", changed, rebuildWorld: false, resolveWater: false, rebuildProviders: false,
      reason: "nothing changed",
    };
  }
  // Lakes off is the one honest way to buy the fast mountain slide the brief hoped for: with
  // `?lakes=0` there is no manifest to be wrong, so a surface change costs a rebuild and no solve.
  const lakesOff = next.waterEnabled === false;
  const resolveWater = !lakesOff && !waterSolveIsOptional({ worldChanged, waterChanged });
  const kind = worldChanged ? "world" : waterChanged ? "water" : "tiling";
  return {
    kind,
    changed,
    rebuildWorld: worldChanged,
    resolveWater,
    /// Terrain and imagery both re-request whenever the surface or the sampling moved; a
    /// water-only change leaves the terrain mesh alone and re-rasters the imagery only.
    rebuildProviders: true,
    rebuildTerrain: worldChanged || tiling.length > 0,
    reason: worldChanged
      ? `surface moved (${world.join(", ")})${
        lakesOff ? "; lakes are off, so no water solve" : "; the water manifest moves with it"}`
      : waterChanged
        ? `water only (${water.join(", ")}); the surface and every cached tile survive`
        : `sampling only (${tiling.join(", ")}); the same world, resampled`,
  };
}

/// **The handle owner, and the answer to the leak this whole design invites.**
///
/// A swap builds a world before it stops using the old one, so for one instant there are two.
/// Freeing in the wrong order -- or forgetting -- is how repeated sliding exhausts wasm linear
/// memory, and it is invisible in the picture: the render is correct right up until the
/// allocation that fails.
///
/// Two properties, both asserted in `live-swap.test.mjs` against the real engine:
///
/// 1. **Build first, free second.** A refused world (`newWorld` throws) must leave the previous
///    one installed and drawable, not leave the viewer holding a freed handle.
/// 2. **`wb_world_count` does not grow across repeated swaps.** That is the test this feature
///    most needs and the one easiest to forget.
export class WorldSwapper {
  constructor(engine) {
    this.engine = engine;
    this.handle = 0;
    /// How many worlds this swapper has built and freed. A report quotes these rather than
    /// inferring them from a count that is also moved by every other holder of a handle.
    this.built = 0;
    this.freed = 0;
  }

  /// Build `spec` and retire the previous handle. Returns the new handle.
  ///
  /// The build is first and is NOT in a try: a throw propagates with `this.handle` unchanged, so
  /// a refused block leaves the drawn world exactly as it was.
  swap(spec) {
    const next = this.engine.newWorld(spec);
    this.built += 1;
    const previous = this.handle;
    this.handle = next;
    if (previous !== 0 && previous !== next) {
      this.engine.freeWorld(previous);
      this.freed += 1;
    }
    return next;
  }

  /// Adopt a handle built elsewhere -- `main.js` builds the first world before this object
  /// exists, and a swapper that did not know about it would leak exactly one world per page.
  adopt(handle) {
    this.handle = handle;
    this.built += 1;
    return handle;
  }

  free() {
    if (this.handle !== 0) {
      this.engine.freeWorld(this.handle);
      this.freed += 1;
      this.handle = 0;
    }
  }
}

/// **Debounce on RELEASE, not on every input event**, with a coalescing tail.
///
/// Every terrain change invalidates every visible tile, so there is no smooth-dragging story to
/// tell here and this file does not pretend there is one: the panel listens for `change` (which a
/// range input fires when the drag ends or the arrow key settles) rather than `input`, and this
/// wrapper coalesces the burst that keyboard auto-repeat still produces.
///
/// `pending` is what makes it safe to hold a swap that takes half a minute: a release arriving
/// while a swap is in flight is remembered and run once the flight finishes, so the last thing the
/// owner asked for is the thing that ends up on screen -- and the intermediate asks are dropped
/// rather than queued, because a queue of nine 30-second swaps is a hung tab.
export function debounceLatest(run, waitMs, { setTimer = setTimeout, clearTimer = clearTimeout } = {}) {
  let timer = null;
  let inFlight = false;
  let queued = null;
  const fire = (args) => {
    inFlight = true;
    queued = null;
    Promise.resolve()
      .then(() => run(...args))
      .catch((error) => { state.lastError = error; })
      .finally(() => {
        inFlight = false;
        if (queued) {
          const next = queued;
          queued = null;
          fire(next);
        }
      });
  };
  const state = {
    lastError: null,
    /// True while a swap is running or one is waiting to run. The panel disables `generate` on
    /// this so a reload cannot race a swap.
    get busy() { return inFlight || timer !== null || queued !== null; },
    cancel() { if (timer !== null) { clearTimer(timer); timer = null; } },
  };
  const trigger = (...args) => {
    if (timer !== null) clearTimer(timer);
    timer = setTimer(() => {
      timer = null;
      if (inFlight) { queued = args; return; }
      fire(args);
    }, waitMs);
  };
  trigger.state = state;
  return trigger;
}

/// Milliseconds a release waits before a swap starts. Long enough to coalesce keyboard
/// auto-repeat (Windows repeats at ~30 Hz after a 500 ms delay), short enough that a mouse
/// release feels immediate. It is not a smoothing window: nothing here can make a terrain change
/// cheap enough to run per input event.
export const SWAP_DEBOUNCE_MS = 180;

/// **The URL, kept in sync without a navigation.**
///
/// A permalink that no longer describes the picture is the failure this feature would otherwise
/// introduce: the owner slides six sliders, copies the address bar, and gets a different planet
/// back. `history.replaceState` writes the same query string `apply()` would have navigated to,
/// so the address bar and the drawn world stay the same statement -- and pressing reload lands on
/// exactly the world already on screen, which is the property the digest control below proves.
///
/// Same drop rule as `controls.js`'s `apply`: absent, empty and still-default fields are deleted
/// rather than written, so a shared link carries only what was actually moved.
export function nextQueryString(search, fields, defaults) {
  const q = new URLSearchParams(search);
  for (const [key, value] of Object.entries(fields)) {
    if (value === null || value === "" || value === defaults[key]) q.delete(key);
    else q.set(key, String(value));
  }
  return q.toString();
}

/// The parameters the panel still reaches by RELOADING, and why -- named rather than left to do
/// nothing silently.
///
/// The swap path is generic over a whole spec, so the boundary here is not a technical ceiling; it
/// is what this task built and measured. Anything on this list keeps the pre-existing
/// `location.search` path exactly, which is the path the verification harness already covers.
export const RELOAD_ONLY = [
  ["seed", "a new planet, not a change to this one: nothing on screen survives it, so a reload "
    + "costs nothing a swap would save"],
  ["plates / land / radius", "swappable in principle and untested here; they move the whole "
    + "surface, so the swap would cost what a reload costs"],
  ["harbour feature", "changes availability as well as the surface; the feature-cap path has its "
    + "own checks and none of them have been run against a swap"],
  ["max zoom / posts / feat. cap", "tiling, not the world: cheap to swap and not wired, because "
    + "the owner asked for mountains, lakes and islands"],
  ["fault", "the deliberately-wrong implementations are chosen at boot and baked into the "
    + "workers; a live fault would be a fault this project has no check for"],
  ["cloud coverage", "re-rasterises every cloud tile; the layer is independent of the world and "
    + "was left on the reload path rather than measured as a fourth class"],
  ["relief sliders", "roughness, and the owner's three words were mountains, lakes and islands"],
];
