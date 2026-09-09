// The live swap: what a change of parameters costs, what may be skipped, and the handle
// arithmetic that decides whether sliding repeatedly exhausts wasm memory.
//
// Three of these tests are worth more than the rest, and they are the three this feature would
// most plausibly ship without:
//
// 1. `a tectonic change moves the water manifest` -- run against the REAL engine. It is the
//    measurement `swapPlan` is built on, and without it the "skip the water solve for mountain
//    sliders" optimisation would be re-invented by the next reader as an obvious win.
// 2. `swapPlan refuses to skip the water solve when the surface moved` -- the rule itself, so
//    (1) cannot be true and the code still wrong.
// 3. `wb_world_count does not grow across many swaps` -- the leak this design invites, asked of
//    the engine's own counter rather than of a variable this project maintains.

import { strict as assert } from "node:assert";
import { readFileSync } from "node:fs";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { Engine } from "../public/app/engine.js";
import {
  changedFields, debounceLatest, nextQueryString, RELOAD_ONLY, sameValue, SWAP_DEBOUNCE_MS,
  swapPlan, TILING_FIELDS, WATER_FIELDS, waterSolveIsOptional, WORLD_FIELDS, WorldSwapper,
} from "../public/app/live-swap.js";

/// A minimal spec in the shape `main.js` keeps in `installed.state`.
function state(overrides = {}) {
  return {
    spec: {
      seed: "20260904", radiusM: 6371000, plateCount: 12, landFraction: 0.29, features: [],
      relief: null, tectonics: null, coast: null,
    },
    waterNodes: 30000,
    waterEnabled: true,
    size: 65,
    maxLevel: 12,
    featureCeiling: 16,
    ...overrides,
  };
}

/// `state` with one spec field replaced, which is how every world-class change arrives.
function withSpec(base, spec) {
  return { ...base, spec: { ...base.spec, ...spec } };
}

/// A tectonic block that differs from `null`. **Not written down as numbers**: this project's
/// characteristic defect is a viewer-side copy of an engine number, and the plan tests only need
/// "some block that is not the canonical `null`". The engine-backed test below reads the real
/// preset across the boundary instead.
const TECTONICS = { continentCollisionM: 1500, continentCollisionWidthM: 400000 };

// ---------------------------------------------------------------------------------------
// Structural comparison: the thing every plan is built on
// ---------------------------------------------------------------------------------------

test("sameValue compares structurally, not by identity or by key order", () => {
  assert.equal(sameValue(1, 1), true);
  assert.equal(sameValue("a", "a"), true);
  assert.equal(sameValue(null, null), true);
  assert.equal(sameValue(null, 0), false);
  assert.equal(sameValue([], []), true);
  assert.equal(sameValue([1, 2], [1, 2]), true);
  assert.equal(sameValue([1, 2], [2, 1]), false);
  assert.equal(sameValue([1], [1, 2]), false);
  // Key order must not decide the answer: the panel builds a block field by field and the engine
  // preset reader builds it from a record, and the two orders differ.
  assert.equal(sameValue({ a: 1, b: 2 }, { b: 2, a: 1 }), true);
  assert.equal(sameValue({ a: 1 }, { a: 1, b: 2 }), false);
  assert.equal(sameValue({ a: 1 }, { a: 2 }), false);
  // An object is never equal to an array, whatever their contents.
  assert.equal(sameValue({ 0: 1 }, [1]), false);
  // NaN is not a value a spec field should ever hold; treating it as equal to itself here would
  // hide a broken parse instead of surfacing it as a change.
  assert.equal(sameValue(NaN, NaN), false);
});

test("changedFields names the fields that moved, in the order asked for", () => {
  const before = state();
  const after = withSpec(before, { landFraction: 0.4, plateCount: 20 });
  assert.deepEqual(changedFields(before.spec, after.spec, WORLD_FIELDS), ["plateCount", "landFraction"]);
  assert.deepEqual(changedFields(before.spec, before.spec, WORLD_FIELDS), []);
});

// ---------------------------------------------------------------------------------------
// The plan
// ---------------------------------------------------------------------------------------

test("swapPlan does nothing when nothing moved", () => {
  const plan = swapPlan(state(), state());
  assert.equal(plan.kind, "none");
  assert.equal(plan.rebuildWorld, false);
  assert.equal(plan.resolveWater, false);
  assert.deepEqual(plan.changed, []);
});

test("a lake-node change is the CHEAP class: no world rebuild, no terrain rebuild", () => {
  const plan = swapPlan(state(), state({ waterNodes: 60000 }));
  assert.equal(plan.kind, "water");
  assert.equal(plan.rebuildWorld, false, "the surface did not move, so the nine worlds survive");
  assert.equal(plan.rebuildTerrain, false, "and so does every cached terrain tile");
  assert.equal(plan.resolveWater, true);
  assert.deepEqual(plan.changed, ["waterNodes"]);
});

test("**swapPlan refuses to skip the water solve when the surface moved**", () => {
  // This is the rule the module's measurement exists to justify, and the one an optimiser would
  // delete. A mountain slider moves `tectonics` and nothing else; the water knobs are untouched.
  const before = state();
  const after = withSpec(before, { tectonics: TECTONICS });
  const plan = swapPlan(before, after);
  assert.equal(plan.kind, "world");
  assert.equal(plan.rebuildWorld, true);
  assert.equal(
    plan.resolveWater, true,
    "the lake bodies are read off the surface, so a mountain move creates and deletes lakes",
  );
  assert.deepEqual(changedFields(before, after, WATER_FIELDS), [],
    "and it says so even though NO water parameter changed, which is the whole point");
});

test("a coastline change is the same class, because it moves the drainage too", () => {
  const before = state();
  const after = withSpec(before, {
    coast: { amplitude: 0.5, windowSpreads: 20, frequency: 4, octaves: 4, gain: 0.5, lacunarity: 2 },
  });
  const plan = swapPlan(before, after);
  assert.equal(plan.kind, "world");
  assert.equal(plan.resolveWater, true);
});

test("with lakes off there is no manifest to be wrong, so a surface change skips the solve", () => {
  const before = state({ waterEnabled: false });
  const after = withSpec(before, { tectonics: TECTONICS });
  const plan = swapPlan(before, after);
  assert.equal(plan.kind, "world");
  assert.equal(plan.rebuildWorld, true);
  assert.equal(plan.resolveWater, false, "?lakes=0 is the honest fast mountain slide");
});

test("a tiling change resamples the same world: no rebuild, no solve", () => {
  const plan = swapPlan(state(), state({ size: 129 }));
  assert.equal(plan.kind, "tiling");
  assert.equal(plan.rebuildWorld, false);
  assert.equal(plan.rebuildTerrain, true);
  assert.equal(plan.resolveWater, false);
  assert.deepEqual(TILING_FIELDS.includes("size"), true);
});

test("waterSolveIsOptional is optional in exactly one of four cases", () => {
  assert.equal(waterSolveIsOptional({ worldChanged: false, waterChanged: false }), true);
  assert.equal(waterSolveIsOptional({ worldChanged: true, waterChanged: false }), false);
  assert.equal(waterSolveIsOptional({ worldChanged: false, waterChanged: true }), false);
  assert.equal(waterSolveIsOptional({ worldChanged: true, waterChanged: true }), false);
});

test("every reload-only parameter carries a reason, not just a name", () => {
  assert.ok(RELOAD_ONLY.length > 0);
  for (const entry of RELOAD_ONLY) {
    assert.equal(entry.length, 2);
    const [name, why] = entry;
    assert.ok(name.length > 0, "a nameless entry says nothing");
    assert.ok(why.length > 20, `"${name}" needs a reason, not a shrug`);
  }
});

// ---------------------------------------------------------------------------------------
// The URL
// ---------------------------------------------------------------------------------------

test("nextQueryString keeps a permalink honest: defaults dropped, others preserved", () => {
  const defaults = { lakeNodes: "30000", plates: "12" };
  // A field back at its default is deleted, not written -- the same rule `apply` follows, so a
  // link built by sliding and a link built by pressing generate are the same string.
  assert.equal(nextQueryString("?lakeNodes=60000", { lakeNodes: "30000" }, defaults), "");
  assert.equal(nextQueryString("", { lakeNodes: "60000" }, defaults), "lakeNodes=60000");
  // Parameters this swap does not own are carried through untouched.
  const carried = nextQueryString("?seed=7&relief=0", { lakeNodes: "60000" }, defaults);
  const q = new URLSearchParams(carried);
  assert.equal(q.get("seed"), "7");
  assert.equal(q.get("relief"), "0");
  assert.equal(q.get("lakeNodes"), "60000");
  // `null` deletes, which is how the panel expresses "this knob is off".
  assert.equal(nextQueryString("?harbour=1", { harbour: null }, defaults), "");
});

// ---------------------------------------------------------------------------------------
// The debounce
// ---------------------------------------------------------------------------------------

test("debounceLatest fires once per burst and drops the middle of it", async () => {
  const seen = [];
  const trigger = debounceLatest((n) => { seen.push(n); }, 5);
  trigger(1);
  trigger(2);
  trigger(3);
  assert.deepEqual(seen, [], "nothing runs while the burst is still arriving");
  await new Promise((r) => setTimeout(r, 30));
  assert.deepEqual(seen, [3], "the last ask wins; the two before it are dropped, not queued");
});

test("a release during a swap is remembered, and only the last one is", async () => {
  const seen = [];
  let release = null;
  const trigger = debounceLatest((n) => {
    seen.push(n);
    return new Promise((resolve) => { release = resolve; });
  }, 1);
  trigger("first");
  await new Promise((r) => setTimeout(r, 20));
  assert.deepEqual(seen, ["first"]);
  assert.equal(trigger.state.busy, true, "a swap in flight is busy");
  // Three more releases while the first swap is still running.
  trigger("a");
  await new Promise((r) => setTimeout(r, 10));
  trigger("b");
  await new Promise((r) => setTimeout(r, 10));
  assert.deepEqual(seen, ["first"], "none of them start while the first is in flight");
  release();
  await new Promise((r) => setTimeout(r, 20));
  assert.deepEqual(seen, ["first", "b"], "the newest queued ask runs; the stale one is dropped");
});

test("a swap that throws does not wedge the debouncer", async () => {
  const seen = [];
  const trigger = debounceLatest((n) => {
    seen.push(n);
    if (n === 1) throw new Error("refused");
    return undefined;
  }, 1);
  trigger(1);
  await new Promise((r) => setTimeout(r, 20));
  assert.deepEqual(seen, [1]);
  assert.equal(String(trigger.state.lastError).includes("refused"), true);
  trigger(2);
  await new Promise((r) => setTimeout(r, 20));
  assert.deepEqual(seen, [1, 2], "the next release still runs");
});

test("the debounce window is a release coalescer, not a smoothing window", () => {
  // Stated as a bound rather than an exact value: the claim is that it is short enough to feel
  // immediate on a mouse release and long enough to swallow keyboard auto-repeat (~30 Hz).
  assert.ok(SWAP_DEBOUNCE_MS >= 60 && SWAP_DEBOUNCE_MS <= 400, `${SWAP_DEBOUNCE_MS} ms`);
});

// ---------------------------------------------------------------------------------------
// The engine-backed half: handles, leaks, and the measurement the plan rests on
// ---------------------------------------------------------------------------------------

async function loadEngine() {
  const wasmPath = fileURLToPath(new URL("../public/wasm/worldbuilder_engine.wasm", import.meta.url));
  const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath), {});
  return new Engine(instance);
}

const SMALL_WORLD = {
  seed: "20260904", radiusM: 6371000, plateCount: 12, landFraction: 0.29, features: [],
};

test("WorldSwapper builds before it frees, so a refusal leaves the drawn world alone", async () => {
  const engine = await loadEngine();
  const swapper = new WorldSwapper(engine);
  const first = swapper.swap(SMALL_WORLD);
  assert.notEqual(first, 0);
  assert.equal(engine.worldCount(), 1);

  // A block the engine refuses. `landFraction` outside 0..1 is refused by `wb_world_new_coast`,
  // which returns handle 0 and makes `newWorld` throw.
  assert.throws(() => swapper.swap({ ...SMALL_WORLD, landFraction: 42 }));
  assert.equal(swapper.handle, first, "the previous handle is still installed");
  assert.equal(engine.worldCount(), 1, "and still alive -- not freed on the way to a refusal");
  // And it is still a usable world, which is the property that matters to a viewer.
  assert.equal(Number.isFinite(engine.elevationM(swapper.handle, 10, 20)), true);
  swapper.free();
  assert.equal(engine.worldCount(), 0);
});

test("**wb_world_count does not grow across many swaps**", async () => {
  const engine = await loadEngine();
  const swapper = new WorldSwapper(engine);
  const counts = [];
  for (let i = 0; i < 40; i += 1) {
    // A different world each time, so nothing can be quietly deduplicated.
    swapper.swap({ ...SMALL_WORLD, landFraction: 0.2 + i * 0.01 });
    counts.push(engine.worldCount());
  }
  assert.deepEqual(new Set(counts), new Set([1]),
    `40 swaps must leave exactly one live world; saw ${JSON.stringify([...new Set(counts)])}`);
  assert.equal(swapper.built, 40);
  assert.equal(swapper.freed, 39, "39 frees for 40 builds: the fortieth is the one on screen");
  swapper.free();
  assert.equal(engine.worldCount(), 0, "and the last one goes when the swapper does");
});

test("adopt takes ownership of a handle built before the swapper existed", async () => {
  const engine = await loadEngine();
  const swapper = new WorldSwapper(engine);
  // `main.js` builds the boot world through `installWorld`; a swapper that did not know about a
  // handle it did not build would leak exactly one world per page.
  const booted = engine.newWorld(SMALL_WORLD);
  swapper.adopt(booted);
  swapper.swap({ ...SMALL_WORLD, landFraction: 0.4 });
  assert.equal(engine.worldCount(), 1, "the adopted world was freed by the swap that replaced it");
  swapper.free();
  assert.equal(engine.worldCount(), 0);
});

test("**a tectonic change moves the water manifest** — the measurement swapPlan rests on", async (t) => {
  // Population: two worlds differing ONLY in `continentCollisionM` (the "height" slider), one
  // water solve each at 4,000 nodes. Method: `wb_water_run` on each, rows matched by `rootNode`.
  // Host: node, this repository's checked-in `worldbuilder_engine.wasm`.
  //
  // The claim is narrow and is the one `swapPlan.resolveWater` depends on: **the manifest is a
  // function of the surface**, so a mountain slider that kept the previous manifest would draw
  // the previous world's lakes on the new world's ground.
  const engine = await loadEngine();
  const world = { ...SMALL_WORLD, landFraction: 0.35 };
  const nodes = 4000;
  // **Read from the engine, not restated here.** `wb_tectonic_preset` is the only way this
  // viewer learns a tectonic number, and a hand-typed block was refused with `WB_ERR_PARAM` on
  // the first run of this very test -- which is the defect the rule exists to prevent, caught by
  // the rule.
  const canonical = engine.tectonicPreset("canonical");

  const solve = (tectonics) => {
    const handle = engine.newWorld({ ...world, tectonics });
    try {
      return engine.waterRun({ handle, nodeCount: nodes }).bodies;
    } finally {
      engine.freeWorld(handle);
    }
  };

  const base = solve(canonical);
  // **The control first.** Without it, a difference below could be the solve being
  // nondeterministic rather than the surface having moved -- which is a different finding and
  // would invalidate the whole design, because then a swap could not reproduce a load either.
  const control = solve(canonical);
  const row = (b) => `${b.rootNode}:${b.levelM}:${b.minLatitudeDeg}:${b.maxLatitudeDeg}:${b.minLongitudeDeg}:${b.maxLongitudeDeg}`;
  assert.deepEqual(control.map(row), base.map(row),
    "the solve must be deterministic, or a swap could not reproduce a load at all");

  const taller = solve({ ...canonical, continentCollisionM: canonical.continentCollisionM * 2 });
  assert.notDeepEqual(taller.map(row), base.map(row),
    "raising the mountains must move the lakes; if this ever passes, the skip becomes legal");
  t.diagnostic(`baseline ${base.length} bodies, taller ${taller.length} bodies at ${nodes} nodes`);
});
