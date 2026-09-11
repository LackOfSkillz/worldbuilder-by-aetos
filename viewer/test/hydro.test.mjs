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

test("a bake comes back with a schema-4 header and counts that add up", () => {
  const handle = engine.newWorld({ seed: 20260904, radiusM: 6371000, plateCount: 12, landFraction: 0.29 });
  const words = engine.hydroBake({ handle, params: PARAMS });
  const s = engine.hydroSummary(words);
  assert.equal(s.schema, 4);
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

test("hydroSummary reads the SCHEMA 4 params echo and forced-outlet match counts", () => {
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
  // SCHEMA 4 (word 32): decodeHydro's schema-4 assertion mirrors this same word.
  assert.equal(s.cappedBasins, words[32]);
});

test("hydroSummary throws on a schema other than 4 rather than misreading the header", () => {
  const handle = engine.newWorld({ seed: 20260904, radiusM: 6371000, plateCount: 12, landFraction: 0.29 });
  const words = engine.hydroBake({ handle, params: PARAMS });
  for (const schema of [2, 3, Number.NaN]) {
    const tampered = words.slice();
    tampered[0] = schema;
    assert.throws(() => engine.hydroSummary(tampered), /schema/);
  }
  assert.equal(engine.hydroSummary(words).schema, 4);
});

test("the same bake twice is the same words", () => {
  const handle = engine.newWorld({ seed: 20260904, radiusM: 6371000, plateCount: 12, landFraction: 0.29 });
  const a = engine.hydroBake({ handle, params: PARAMS });
  const b = engine.hydroBake({ handle, params: PARAMS });
  assert.deepEqual(Array.from(a), Array.from(b));
});
