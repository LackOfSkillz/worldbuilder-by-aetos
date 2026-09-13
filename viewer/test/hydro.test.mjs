import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { Engine, WB_WATER_STRIDE, decodeWaterSample } from "../public/app/engine.js";
// The record decoder lives with the preview drawing, not with the boundary: the query answers
// name bodies by id, and this is the reader that turns the record into entries to look them up
// in. Imported rather than restated -- the layout is a four-way twin already.
import { decodeHydro } from "../public/app/water-preview.js";

const wasmPath = fileURLToPath(new URL("../public/wasm/worldbuilder_engine.wasm", import.meta.url));
const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath), {});
const engine = new Engine(instance);

const PARAMS = {
  totalNodes: 12000, wetnessNodes: 500, keepDepthM: 8, keepAreaM2: 1e6, pondMaxAreaM2: 1e6,
  streamFlowM2: 3e10, riverFlowM2: 3e11, greatFlowM2: 3e12, notchFallM: 1,
  evaporationFactor: 1, saltFlatShare: 0.1, forcedOutlets: [],
};

test("a bake comes back with a schema-6 header and counts that add up", () => {
  const handle = engine.newWorld({ seed: 20260904, radiusM: 6371000, plateCount: 12, landFraction: 0.29 });
  const words = engine.hydroBake({ handle, params: PARAMS });
  const s = engine.hydroSummary(words);
  assert.equal(s.schema, 6);
  assert.equal(s.nodes, 12000);
  assert.ok(s.landNodes > 0 && s.landNodes < 12000);
  assert.equal(s.kept + s.notched, s.hollows);
  assert.equal(s.streams + s.rivers + s.great, s.reaches);
  // Task 12b (Ruling 12b-1): the effective thresholds the bake actually used, always at least
  // what PARAMS asked for, and each at least 10x the one below it.
  assert.ok(s.streamFlowM2 >= PARAMS.streamFlowM2);
  assert.ok(s.riverFlowM2 >= PARAMS.riverFlowM2);
  assert.ok(s.greatFlowM2 >= PARAMS.greatFlowM2);
  assert.ok(s.riverFlowM2 >= 10 * s.streamFlowM2);
  assert.ok(s.greatFlowM2 >= 10 * s.riverFlowM2);
});

test("hydroSummary reads the SCHEMA 5/6 params echo, forced-outlet matches, crossing counts and extent totals", () => {
  const handle = engine.newWorld({ seed: 20260904, radiusM: 6371000, plateCount: 12, landFraction: 0.29 });
  const params = {
    ...PARAMS,
    forcedOutlets: [{ latitudeDeg: 0, longitudeDeg: 0 }],
  };
  const words = engine.hydroBake({ handle, params });
  const s = engine.hydroSummary(words);
  // The params echo (word 20-29): what this bake actually ran with.
  assert.equal(s.totalNodes, params.totalNodes);
  assert.equal(s.wetnessNodes, params.wetnessNodes);
  assert.equal(s.keepDepthM, params.keepDepthM);
  assert.equal(s.keepAreaM2, params.keepAreaM2);
  assert.equal(s.pondMaxAreaM2, params.pondMaxAreaM2);
  assert.equal(s.notchFallM, params.notchFallM);
  assert.equal(s.evaporationFactor, params.evaporationFactor);
  assert.equal(s.saltFlatShare, params.saltFlatShare);
  // Not exposed as a wasm param (Ruling 12b-1's note); still carries its earth_like default.
  assert.ok(Number.isFinite(s.minStreamNodes) && s.minStreamNodes > 0);
  assert.ok(Number.isFinite(s.keepMaxAreaM2) && s.keepMaxAreaM2 > 0);
  // One forced outlet requested; whether it matched a submerged node on this world is not
  // pinned here (that is `water-preview.test.mjs`'s job with a controlled record) -- only that
  // the count round-trips and never exceeds what was requested.
  assert.equal(s.forcedRequested, 1);
  assert.ok(s.forcedMatched >= 0 && s.forcedMatched <= s.forcedRequested);
  // SCHEMA 4 (word 32): decodeHydro's schema assertion mirrors this same word.
  assert.equal(s.cappedBasins, words[32]);
  // SCHEMA 5 (words 43-44). Ruling S-2 keeps the coarse crossings, so neither is pinned to
  // zero here -- only that both are counts, and that what is left is no worse than what the
  // coarse record already had (the property `refinement_adds_no_crossings` holds engine-side).
  assert.equal(s.crossingsCoarse, words[43]);
  assert.equal(s.crossingsLeft, words[44]);
  assert.ok(Number.isInteger(s.crossingsCoarse) && s.crossingsCoarse >= 0);
  assert.ok(Number.isInteger(s.crossingsLeft) && s.crossingsLeft >= 0);
  assert.ok(s.crossingsLeft <= s.crossingsCoarse);
  // SCHEMA 5 (words 45-46), Task 5: the fine pond search's two counts. Neither is pinned to a
  // number -- the terrain decides that -- only that both are counts, that nothing can be kept
  // that was not found, and that the kept ones fit inside the bodies the record carries.
  assert.equal(s.pondsFound, words[45]);
  assert.equal(s.pondsKept, words[46]);
  assert.ok(Number.isInteger(s.pondsFound) && s.pondsFound >= 0);
  assert.ok(Number.isInteger(s.pondsKept) && s.pondsKept >= 0);
  assert.ok(s.pondsKept <= s.pondsFound);
  assert.ok(s.pondsKept <= s.bodies);
  // The seven pond params are in the record (words 47-53) but deliberately not in this summary:
  // like the refinement params, they are not wasm params.
  assert.equal(s.pondCellM, undefined);
  // SCHEMA 6 (words 54-55), plan 1b-4 Task 2: the extent totals across all bodies. This world's
  // bake keeps only coarse bodies (a shore-point set per Ruling E-2, never a pond), so both
  // totals are the sum of every body's own extent and must be positive -- not the zeroed stub
  // Task 1 shipped before Task 2 filled a real extent in. `hydroSummary` is header-only and
  // carries no per-body data, so the per-body sum-and-shape invariant these two totals must
  // satisfy is asserted where the bodies are actually decoded:
  // `water-preview.test.mjs`'s "decodeHydro's body and reach counts match hydroSummary's".
  assert.equal(s.shoreMembers, words[54]);
  assert.equal(s.collarPoints, words[55]);
  assert.ok(s.shoreMembers > 0, "sanity: this world's coarse bodies carry shore points");
  assert.ok(s.collarPoints > 0, "sanity: this world's coarse bodies carry a collar");
});

test("hydroSummary throws on a schema other than 6 rather than misreading the header", () => {
  const handle = engine.newWorld({ seed: 20260904, radiusM: 6371000, plateCount: 12, landFraction: 0.29 });
  const words = engine.hydroBake({ handle, params: PARAMS });
  for (const schema of [2, 3, 4, 5, Number.NaN]) {
    const tampered = words.slice();
    tampered[0] = schema;
    assert.throws(() => engine.hydroSummary(tampered), /schema/);
  }
  assert.equal(engine.hydroSummary(words).schema, 6);
});

test("the same bake twice is the same words", () => {
  const handle = engine.newWorld({ seed: 20260904, radiusM: 6371000, plateCount: 12, landFraction: 0.29 });
  const a = engine.hydroBake({ handle, params: PARAMS });
  const b = engine.hydroBake({ handle, params: PARAMS });
  assert.deepEqual(Array.from(a), Array.from(b));
});

// Plan 2a Task 4: the browser can ask what water is at a point. The bake is held rather than
// copied-and-freed (`hydroHold`), because Ruling Q-2 builds the query index beside the held
// record and drops it with the bake.

test("waterAt answers a body's own anchor with that body, and waterTile agrees sample for sample", () => {
  const handle = engine.newWorld({ seed: 20260904, radiusM: 6371000, plateCount: 12, landFraction: 0.29 });
  const { id, words } = engine.hydroHold({ handle, params: PARAMS });
  try {
    const { bodies } = decodeHydro(words);
    assert.ok(bodies.length > 0, "sanity: this world's bake keeps at least one body");

    // Ruling Q-14: the anchor is the one INTERIOR point the record names, and an interior hole
    // in the index is invisible from the outline. A body whose ground stands above its own
    // level is skipped -- Ruling Q-12 makes that dry ground, not a wrong answer -- so the test
    // asks for the first anchor that is actually under water.
    let claimed = null;
    for (const body of bodies) {
      const [latitudeDeg, longitudeDeg] = body.anchor;
      const got = engine.waterAt({ handle, bakeId: id, latitudeDeg, longitudeDeg });
      if (got.bodyId === body.id) {
        claimed = { body, got, latitudeDeg, longitudeDeg };
        break;
      }
    }
    assert.ok(claimed, "no body answered as itself at its own anchor");

    const { body, got, latitudeDeg, longitudeDeg } = claimed;
    assert.equal(got.kind, body.kind, "the answer's kind is the body's recorded kind");
    assert.equal(got.levelM, body.levelM, "the answer's level is the body's recorded level");
    assert.ok(got.depthM >= 0, `depth ${got.depthM} is below zero`);
    // A body answer is not a river answer (Ruling Q-18's fifth word).
    assert.equal(got.reachId, null);
    // `fresh` is deliberately not a sixth word: it is read off the record entry the id names.
    const named = bodies.find((b) => b.id === got.bodyId);
    assert.ok(named, "the answer named a body id the record does not carry");
    assert.equal(typeof named.fresh, "boolean");

    // The batch, over a 1x1 rectangle on the same point, is the same five words -- five, not
    // four (Ruling Q-18 supersedes Q-8's stride).
    const tile = engine.waterTile({
      handle, bakeId: id,
      box: { lat0: latitudeDeg, lon0: longitudeDeg, lat1: latitudeDeg, lon1: longitudeDeg },
      rows: 1, columns: 1,
    });
    assert.equal(tile.length, WB_WATER_STRIDE);
    assert.deepEqual(decodeWaterSample(tile, 0), got);

    // And over a real rectangle: every sample equals the scalar export at the same point.
    const box = {
      lat0: latitudeDeg + 0.5, lon0: longitudeDeg - 0.5,
      lat1: latitudeDeg - 0.5, lon1: longitudeDeg + 0.5,
    };
    const rows = 3;
    const columns = 3;
    const grid = engine.waterTile({ handle, bakeId: id, box, rows, columns });
    assert.equal(grid.length, rows * columns * WB_WATER_STRIDE);
    for (let row = 0; row < rows; row += 1) {
      for (let column = 0; column < columns; column += 1) {
        const lat = box.lat0 + (box.lat1 - box.lat0) * (row / (rows - 1));
        const lon = box.lon0 + (box.lon1 - box.lon0) * (column / (columns - 1));
        assert.deepEqual(
          decodeWaterSample(grid, row * columns + column),
          engine.waterAt({ handle, bakeId: id, latitudeDeg: lat, longitudeDeg: lon }),
          `row ${row} column ${column}`);
      }
    }
  } finally {
    engine.hydroFree(id);
  }
});

test("a freed bake stops answering rather than being served from the index it left behind", () => {
  const handle = engine.newWorld({ seed: 20260904, radiusM: 6371000, plateCount: 12, landFraction: 0.29 });
  const { id } = engine.hydroHold({ handle, params: PARAMS });
  // Two queries: the first builds the index, the second is served from the cache.
  const first = engine.waterAt({ handle, bakeId: id, latitudeDeg: 0, longitudeDeg: 0 });
  assert.deepEqual(engine.waterAt({ handle, bakeId: id, latitudeDeg: 0, longitudeDeg: 0 }), first);
  engine.hydroFree(id);
  assert.throws(
    () => engine.waterAt({ handle, bakeId: id, latitudeDeg: 0, longitudeDeg: 0 }),
    /WB_ERR_HANDLE/);
  assert.throws(() => engine.hydroFree(id), /WB_ERR_HANDLE/);
});

test("hydroBake still frees its own bake, so the old shape leaks nothing", () => {
  const handle = engine.newWorld({ seed: 20260904, radiusM: 6371000, plateCount: 12, landFraction: 0.29 });
  const held = engine.hydroHold({ handle, params: PARAMS });
  engine.hydroFree(held.id);
  // The next bake through `hydroBake` is issued the NEXT id, never a reused one, and is freed
  // by the time it returns -- so freeing that id again is refused.
  const words = engine.hydroBake({ handle, params: PARAMS });
  assert.deepEqual(Array.from(words), Array.from(held.words));
  assert.throws(() => engine.hydroFree(held.id + 1), /WB_ERR_HANDLE/);
});
