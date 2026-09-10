import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { Engine } from "../public/app/engine.js";

const wasmPath = fileURLToPath(new URL("../public/wasm/worldbuilder_engine.wasm", import.meta.url));
const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath), {});
const engine = new Engine(instance);

const PARAMS = {
  totalNodes: 12000, wetnessNodes: 500, keepDepthM: 8, keepAreaM2: 1e6, pondMaxAreaM2: 1e6,
  streamFlowM2: 3e10, riverFlowM2: 3e11, greatFlowM2: 3e12, notchFallM: 1,
  evaporationFactor: 1, saltFlatShare: 0.1, forcedOutlets: [],
};

test("a bake comes back with a schema-2 header and counts that add up", () => {
  const handle = engine.newWorld({ seed: 20260904, radiusM: 6371000, plateCount: 12, landFraction: 0.29 });
  const words = engine.hydroBake({ handle, params: PARAMS });
  const s = engine.hydroSummary(words);
  assert.equal(s.schema, 2);
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

test("the same bake twice is the same words", () => {
  const handle = engine.newWorld({ seed: 20260904, radiusM: 6371000, plateCount: 12, landFraction: 0.29 });
  const a = engine.hydroBake({ handle, params: PARAMS });
  const b = engine.hydroBake({ handle, params: PARAMS });
  assert.deepEqual(Array.from(a), Array.from(b));
});
