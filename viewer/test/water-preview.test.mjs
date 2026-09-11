// Node-native tests for water-preview.js's decode half, against the checked-in wasm the way
// `hydro.test.mjs` does. `drawPreview` touches Cesium and is not exercised here; everything
// this file drives is DOM-free by design.

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { Engine } from "../public/app/engine.js";
import {
  decodeHydro, forcedOutletsFromParams, outletPath,
} from "../public/app/water-preview.js";

const wasmPath = fileURLToPath(new URL("../public/wasm/worldbuilder_engine.wasm", import.meta.url));
const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath), {});
const engine = new Engine(instance);

// The same test thresholds `hydro.test.mjs` bakes with, at the `plain` world named in the
// brief: seed 20260904, 6,371,000 m, 12 plates, 0.29 land fraction, 12,000 nodes.
const PARAMS = {
  totalNodes: 12000, wetnessNodes: 500, keepDepthM: 8, keepAreaM2: 1e6, pondMaxAreaM2: 1e6,
  streamFlowM2: 3e10, riverFlowM2: 3e11, greatFlowM2: 3e12, notchFallM: 1,
  evaporationFactor: 1, saltFlatShare: 0.1, forcedOutlets: [],
};

function bake() {
  const handle = engine.newWorld({ seed: 20260904, radiusM: 6371000, plateCount: 12, landFraction: 0.29 });
  return engine.hydroBake({ handle, params: PARAMS });
}

test("decodeHydro's body and reach counts match hydroSummary's, and it consumes the whole array", () => {
  const words = bake();
  const summary = engine.hydroSummary(words);
  const decoded = decodeHydro(words);
  assert.equal(decoded.bodies.length, summary.bodies);
  assert.equal(decoded.reaches.length, summary.reaches);
  assert.equal(decoded.notches, summary.notches);
  assert.equal(decoded.falls.length, summary.falls);
  assert.equal(decoded.header.schema, 3);
  assert.equal(decoded.header.nodes, summary.nodes);
  assert.equal(decoded.header.forcedRequested, summary.forcedRequested);
  assert.equal(decoded.header.forcedMatched, summary.forcedMatched);
});

test("decodeHydro consumes a real SCHEMA 3 bake exactly, reach fresh and body downstream included", () => {
  const decoded = decodeHydro(bake());
  assert.ok(decoded.reaches.length > 0, "sanity: this world has reaches");
  for (const reach of decoded.reaches) {
    assert.equal(typeof reach.fresh, "boolean");
  }
  assert.ok(decoded.bodies.length > 0, "sanity: this world has bodies");
  for (const body of decoded.bodies) {
    assert.ok(["reach", "body", "ocean", "sink"].includes(body.downstream.kind));
  }
});

test("decodeHydro throws on a truncated array", () => {
  const words = bake();
  assert.throws(() => decodeHydro(words.slice(0, words.length - 1)), /truncated|ran out of words/);
  assert.throws(() => decodeHydro(new Float64Array(0)), /truncated|ran out of words/);
});

test("decodeHydro throws on a schema-2 header", () => {
  const words = bake();
  const tampered = words.slice();
  tampered[0] = 2;
  assert.throws(() => decodeHydro(tampered), /unsupported schema/);
});

test("decodeHydro refuses an index or count word above 4294967295, as record.rs's decode does", () => {
  const words = bake();
  const U32_MAX = 4294967295;

  // The boundary itself is a valid u32: word 5 (`nodes`) at exactly u32::MAX still decodes.
  const atMax = words.slice();
  atMax[5] = U32_MAX;
  assert.equal(decodeHydro(atMax).header.nodes, U32_MAX);

  // One past it is refused, in a header count...
  const header = words.slice();
  header[5] = U32_MAX + 1;
  assert.throws(() => decodeHydro(header), /bad count\/index word/);

  // ...in a body's optional outlet reach (body 0's word 8, record word 40)...
  assert.ok(words[1] > 0, "sanity: this world has a body to tamper with");
  const outlet = words.slice();
  outlet[32 + 8] = U32_MAX + 1;
  assert.throws(() => decodeHydro(outlet), /bad optional index word/);

  // ...and in a downstream id (body 0's words 11-12, record words 43-44, made a body link).
  const downstream = words.slice();
  downstream[32 + 11] = 1;
  downstream[32 + 12] = U32_MAX + 1;
  assert.throws(() => decodeHydro(downstream), /bad downstream body id/);
});

test("decodeHydro throws on a trailing word", () => {
  const words = bake();
  const padded = new Float64Array(words.length + 1);
  padded.set(words);
  padded[words.length] = 0;
  assert.throws(() => decodeHydro(padded), /trailing/);
});

test("outletPath terminates and only names reaches/bodies that exist in the record", () => {
  const decoded = decodeHydro(bake());
  const result = outletPath(decoded);
  assert.ok(["ocean", "sink", "none"].includes(result.end));
  const reachIds = new Set(decoded.reaches.map((r) => r.id));
  const bodyIds = new Set(decoded.bodies.map((b) => b.id));
  for (const id of result.reachIds) assert.ok(reachIds.has(id), `reach ${id} is not in the record`);
  for (const id of result.bodyIds) assert.ok(bodyIds.has(id), `body ${id} is not in the record`);
});

test("outletPath does not loop forever on a record with a downstream cycle", () => {
  // A hand-built two-body, two-reach record whose downstream links point at each other: body 0
  // -> reach 0 -> body 1 -> reach 1 -> body 0. A walk with no loop guard never returns.
  const decoded = {
    bodies: [
      { id: 0, kind: "lake", fresh: true, enclosed: true, forced: false, levelM: 10,
        areaM2: 2e6, depthM: 5, outletReach: 0, anchor: [1, 2], outline: [] },
      { id: 1, kind: "lake", fresh: true, enclosed: true, forced: false, levelM: 8,
        areaM2: 1e6, depthM: 4, outletReach: 1, anchor: [3, 4], outline: [] },
    ],
    reaches: [
      { id: 0, class: "river", order: 1, downstream: { kind: "body", id: 1 }, points: [] },
      { id: 1, class: "river", order: 1, downstream: { kind: "body", id: 0 }, points: [] },
    ],
    notches: 0,
    falls: [],
  };
  const result = outletPath(decoded);
  assert.equal(result.end, "none");
  assert.ok(result.reachIds.length > 0);
});

test("forcedOutletsFromParams parses repeatable ?forcedOutlet= and skips bad input", () => {
  const params = new URLSearchParams(
    "forcedOutlet=12.5,-30&forcedOutlet=notanumber,4&forcedOutlet=1,2,3&forcedOutlet=5",
  );
  const forced = forcedOutletsFromParams(params);
  assert.deepEqual(forced, [{ latitudeDeg: 12.5, longitudeDeg: -30 }]);
});

test("forcedOutletsFromParams returns an empty array when nothing is given", () => {
  assert.deepEqual(forcedOutletsFromParams(new URLSearchParams()), []);
});
