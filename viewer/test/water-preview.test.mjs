// Node-native tests for water-preview.js's decode half, against the checked-in wasm the way
// `hydro.test.mjs` does. `drawPreview` touches Cesium, so its one test here drives it with a
// minimal stand-in that records what is added; everything else this file drives is DOM-free by
// design.

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { Engine } from "../public/app/engine.js";
import {
  decodeHydro, drawPreview, forcedOutletsFromParams, outletPath,
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

function bake(totalNodes = PARAMS.totalNodes) {
  const handle = engine.newWorld({ seed: 20260904, radiusM: 6371000, plateCount: 12, landFraction: 0.29 });
  return engine.hydroBake({ handle, params: { ...PARAMS, totalNodes } });
}

// Ruling S-17 halved `earth_like`'s pond corridor to 1.5 km, and a wasm bake always takes
// `earth_like`'s pond params -- they are not on the wire, so this side cannot widen it back the
// way the Rust fixtures do. At 12,000 nodes the plain world's rivers now find 5 candidates and
// keep 0, which would leave the pond test below asserting over an empty set. 50,000 nodes on the
// SAME world keeps 19 in about two seconds, so the pond test bakes at that and everything else
// stays on the 12,000-node bake `hydro.test.mjs` shares.
const POND_NODES = 50000;

test("decodeHydro's body and reach counts match hydroSummary's, and it consumes the whole array", () => {
  const words = bake();
  const summary = engine.hydroSummary(words);
  const decoded = decodeHydro(words);
  assert.equal(decoded.bodies.length, summary.bodies);
  assert.equal(decoded.reaches.length, summary.reaches);
  assert.equal(decoded.notches, summary.notches);
  assert.equal(decoded.falls.length, summary.falls);
  assert.equal(decoded.header.schema, 6);
  assert.equal(decoded.header.nodes, summary.nodes);
  assert.equal(decoded.header.forcedRequested, summary.forcedRequested);
  assert.equal(decoded.header.forcedMatched, summary.forcedMatched);
  assert.equal(decoded.header.cappedBasins, words[32]);
  assert.equal(decoded.header.crossingsCoarse, words[43]);
  assert.equal(decoded.header.crossingsLeft, words[44]);
  // Task 5's nine words: both pond counts, then the seven pond params, closing the 54-word
  // header. `decodeHydro` reads all nine, where `hydroSummary` returns only the two counts.
  assert.equal(decoded.header.pondsFound, words[45]);
  assert.equal(decoded.header.pondsKept, words[46]);
  assert.equal(decoded.header.pondCellM, words[47]);
  assert.equal(decoded.header.pondSearchRadiusM, words[48]);
  assert.equal(decoded.header.pondKeepDepthM, words[49]);
  assert.equal(decoded.header.pondKeepAreaM2, words[50]);
  assert.equal(decoded.header.pondWetnessShare, words[51]);
  assert.equal(decoded.header.pondMaxSlope, words[52]);
  assert.equal(decoded.header.pondDensityAreaM2, words[53]);
  // SCHEMA 6, plan 1b-4 Task 2 (words 54-55): the extent totals across all bodies. This world's
  // 12,000-node bake keeps only coarse bodies (Ruling E-2's shore-point set, never a pond), so
  // both totals are the sum of every body's own extent and must be positive, not the zeroed
  // stub Task 1 shipped before Task 2 filled a real extent in.
  assert.equal(decoded.header.shoreMembers, words[54]);
  assert.equal(decoded.header.collarPoints, words[55]);
  assert.ok(decoded.header.shoreMembers > 0, "sanity: this world's coarse bodies carry shore points");
  assert.ok(decoded.header.collarPoints > 0, "sanity: this world's coarse bodies carry a collar");
  const shoreMemberTotal = decoded.bodies.reduce((sum, b) => sum + b.shoreMemberCount, 0);
  const collarTotal = decoded.bodies.reduce((sum, b) => sum + (b.outline.length - b.shoreMemberCount), 0);
  assert.equal(decoded.header.shoreMembers, shoreMemberTotal,
               "the header total is the sum of every decoded body's own shoreMemberCount");
  // The collar identity below holds over EVERY body only because this bake keeps no pond. Both
  // header totals count the coarse bodies only; a pond's whole traced ring would land in
  // `collarTotal` and in neither header word, so the guard is asserted, not assumed. On the
  // Rust side's `params` population, which does keep ponds, the all-body sum reads 269 against
  // the header's 101 -- the 168 ring points of its ponds.
  assert.equal(decoded.header.pondsKept, 0,
               "this world's 12,000-node bake keeps no pond, which is what makes the collar identity below total-body");
  assert.equal(decoded.header.collarPoints, collarTotal,
               "the header total is the sum of every coarse body's own collar (outline.length - shoreMemberCount)");
  // Ruling E-1: a coarse body's outline is shore members first, then collar -- so a body with
  // any shore members at all must have MORE outline points than shore members, i.e. a collar
  // too. This is the shape `water_at` will depend on; only the Rust side asserted it before.
  for (const body of decoded.bodies) {
    if (body.shoreMemberCount > 0) {
      assert.ok(body.outline.length > body.shoreMemberCount,
                 `body ${body.id} has ${body.shoreMemberCount} shore members but only ` +
                 `${body.outline.length} outline points -- no collar`);
    }
  }
  // The header is 56 words, so word 56 is the first body's id.
  assert.equal(words[56], decoded.bodies[0].id);
});

test("every pond the record kept is a traced ring at the end of bodies", () => {
  const decoded = decodeHydro(bake(POND_NODES));
  const kept = decoded.header.pondsKept;
  assert.ok(kept > 0, "sanity: this world's fine search keeps ponds");
  for (const body of decoded.bodies.slice(decoded.bodies.length - kept)) {
    // Ruling S-11: the area picks `pond` or `lake`, and both carry the traced 250 m ring.
    assert.ok(["pond", "lake"].includes(body.kind));
    assert.ok(body.outline.length >= 3, "a traced ring, not a shore-point set");
    assert.equal(body.shoreMemberCount, 0, "Ruling T1-2: a fine-search body is always a traced ring");
    assert.equal(body.downstream.kind, "reach", "Ruling S-5");
    assert.equal(body.outletReach, null, "Ruling S-5");
    assert.ok(body.depthM >= decoded.header.pondKeepDepthM);
    assert.ok(body.areaM2 >= decoded.header.pondKeepAreaM2);
  }
});

test("decodeHydro consumes a real SCHEMA 6 bake exactly, reach fresh and body downstream included", () => {
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

test("decodeHydro throws on a schema-2, schema-4 or schema-5 header", () => {
  const words = bake();
  // SCHEMA 4's 43-word header is a PREFIX of SCHEMA 5's 54, which is itself a prefix of SCHEMA
  // 6's 56, so a decoder that adapted rather than refused would read a body's first words as
  // later header words.
  for (const schema of [2, 4, 5]) {
    const tampered = words.slice();
    tampered[0] = schema;
    assert.throws(() => decodeHydro(tampered), /unsupported schema/);
  }
});

test("decodeHydro refuses an index or count word above 4294967295, as record.rs's decode does", () => {
  const words = bake();
  const U32_MAX = 4294967295;
  // The header's length, which Task 1 of plan 1b-4 took from 54 words to 56.
  const HEADER = 56;

  // The boundary itself is a valid u32: word 5 (`nodes`) at exactly u32::MAX still decodes.
  const atMax = words.slice();
  atMax[5] = U32_MAX;
  assert.equal(decodeHydro(atMax).header.nodes, U32_MAX);

  // One past it is refused, in a header count...
  const header = words.slice();
  header[5] = U32_MAX + 1;
  assert.throws(() => decodeHydro(header), /bad count\/index word/);

  // ...in a body's optional outlet reach (body 0's word 8, record word 64)...
  assert.ok(words[1] > 0, "sanity: this world has a body to tamper with");
  const outlet = words.slice();
  outlet[HEADER + 8] = U32_MAX + 1;
  assert.throws(() => decodeHydro(outlet), /bad optional index word/);

  // ...and in a downstream id (body 0's words 11-12, record words 67-68, made a body link).
  const downstream = words.slice();
  downstream[HEADER + 11] = 1;
  downstream[HEADER + 12] = U32_MAX + 1;
  assert.throws(() => decodeHydro(downstream), /bad downstream body id/);
});

// A body's 16 fixed words start after the 56-word header: `shoreMemberCount` is word 13 of
// them, `shoreReachM` word 14 and `outlineLen` word 15.
const BODY_0 = 56;
const SHORE_MEMBER_COUNT = BODY_0 + 13;
const SHORE_REACH_M = BODY_0 + 14;
const OUTLINE_LEN = BODY_0 + 15;

test("decodeHydro refuses a body claiming more shore members than it has outline points, as record.rs's decode does", () => {
  const words = bake();
  const outlineLen = words[OUTLINE_LEN];
  assert.ok(outlineLen > 0, "sanity: this world's first body carries an outline");

  // The boundary is legal: every outline point may be a shore member.
  const atLen = words.slice();
  atLen[SHORE_MEMBER_COUNT] = outlineLen;
  assert.equal(decodeHydro(atLen).bodies[0].shoreMemberCount, outlineLen);

  // One past it is not, and neither is any larger count. All are valid u32 words, so nothing
  // else refuses them, and `outline.slice(0, shoreMemberCount)` on any of them hands a reader a
  // short member set and a negative collar size instead of a decode failure.
  for (const bogus of [outlineLen + 1, 4e9, 4294967295]) {
    const tampered = words.slice();
    tampered[SHORE_MEMBER_COUNT] = bogus;
    assert.throws(() => decodeHydro(tampered), /shore members of a/);
  }
});

test("decodeHydro refuses a non-finite or negative shoreReachM, as record.rs's decode does", () => {
  const words = bake();
  for (const bogus of [NaN, Infinity, -Infinity, -1, -0.5]) {
    const tampered = words.slice();
    tampered[SHORE_REACH_M] = bogus;
    assert.throws(() => decodeHydro(tampered), /bad shore reach/);
  }
  // Zero is legal -- it is what a pond and a body with no usable edge both write -- and so is
  // any finite positive length, however large.
  for (const fine of [0, 1e300]) {
    const tampered = words.slice();
    tampered[SHORE_REACH_M] = fine;
    assert.equal(decodeHydro(tampered).bodies[0].shoreReachM, fine);
  }
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

test("drawPreview draws every waterfall as a white point, labelled with its height, and counts them", () => {
  // Just enough of Cesium for drawPreview: positions and colours are passed through as plain
  // values, and the data source keeps every entity it is handed.
  const Cesium = {
    CustomDataSource: class { constructor(name) { this.name = name; this.entities = { list: [], add(e) { this.list.push(e); return e; } }; } },
    Cartesian3: {
      fromDegrees: (lon, lat) => ({ lon, lat }),
      fromDegreesArray: (flat) => flat,
    },
    Color: {
      WHITE: "white",
      fromCssColorString: (css) => ({ css, withAlpha: () => css }),
    },
  };
  const viewer = { dataSources: { contains: () => false, add: (source) => source } };
  const decoded = {
    bodies: [],
    reaches: [],
    notches: 0,
    falls: [
      { reach: 0, lat: 10, lon: 20, heightM: 12.34 },
      { reach: 3, lat: -5, lon: 7, heightM: 40 },
    ],
  };
  const drawn = drawPreview(viewer, Cesium, decoded);
  assert.equal(drawn.counts.falls, 2);
  const points = drawn.source.entities.list.filter((e) => e.point);
  assert.equal(points.length, 2);
  assert.deepEqual(points[0].position, { lon: 20, lat: 10 });
  assert.equal(points[0].point.color, "white");
  assert.equal(points[0].point.pixelSize, 7);
  assert.equal(points[0].point.disableDepthTestDistance, Number.POSITIVE_INFINITY);
  assert.equal(points[0].description, "waterfall, 12.3 m");
  assert.equal(points[1].description, "waterfall, 40.0 m");
});

function fakeCesium() {
  return {
    CustomDataSource: class {
      constructor(name) {
        this.name = name;
        this.entities = { list: [], add(e) { this.list.push(e); return e; } };
      }
    },
    Cartesian3: {
      fromDegrees: (lon, lat) => ({ lon, lat }),
      fromDegreesArray: (flat) => flat,
    },
    Color: {
      WHITE: "white",
      fromCssColorString: (css) => ({ css, withAlpha: () => css }),
    },
    HeightReference: { CLAMP_TO_GROUND: "clamp" },
  };
}

test("drawPreview draws every body as a true-size ring, a point only for the small ones, and counts rings", () => {
  const Cesium = fakeCesium();
  const viewer = { dataSources: { contains: () => false, add: (source) => source } };
  // radius = sqrt(area / pi): 4e6 m2 -> ~1,128 m (small); 4e12 m2 -> ~1,128 km (big).
  const small = {
    id: 0, kind: "lake", fresh: true, levelM: 1, areaM2: 4e6, outletReach: null, anchor: [0, 0],
  };
  const big = {
    id: 1, kind: "lake", fresh: true, levelM: 1, areaM2: 4e12, outletReach: null, anchor: [1, 1],
  };
  const decoded = { bodies: [small, big], reaches: [], notches: 0, falls: [] };
  const drawn = drawPreview(viewer, Cesium, decoded);

  const rings = drawn.source.entities.list.filter((e) => e.ellipse);
  assert.equal(rings.length, 2);
  assert.equal(drawn.counts.rings, 2);
  assert.equal(drawn.counts.bodies, 2);

  const smallRadius = Math.sqrt(4e6 / Math.PI);
  const bigRadius = Math.sqrt(4e12 / Math.PI);
  const smallRing = rings.find((e) => e.position.lat === 0);
  const bigRing = rings.find((e) => e.position.lat === 1);
  assert.ok(smallRing && bigRing, "both bodies got a ring");
  assert.ok(Math.abs(smallRing.ellipse.semiMajorAxis - smallRadius) < 1);
  assert.ok(Math.abs(smallRing.ellipse.semiMinorAxis - smallRadius) < 1);
  assert.ok(Math.abs(bigRing.ellipse.semiMajorAxis - bigRadius) < 1);
  assert.ok(Math.abs(bigRing.ellipse.semiMinorAxis - bigRadius) < 1);
  assert.equal(smallRing.ellipse.fill, false);
  assert.equal(smallRing.ellipse.outline, true);
  assert.equal(smallRing.ellipse.outlineWidth, 2);
  assert.equal(smallRing.ellipse.height, undefined);
  assert.equal(smallRing.ellipse.heightReference, "clamp");

  const points = drawn.source.entities.list.filter((e) => e.point);
  assert.equal(points.length, 1, "only the small body also gets a point");
  assert.equal(points[0].position.lat, 0);
  assert.equal(points[0].point.pixelSize, 5);
});

test("drawPreview draws a pond's outline as a polygon and counts it, instead of a true-size ring", () => {
  const Cesium = fakeCesium();
  const viewer = { dataSources: { contains: () => false, add: (source) => source } };
  const decoded = {
    header: { schema: 5, pondsKept: 1, crossingsLeft: 0 },
    bodies: [
      {
        id: 0, kind: "lake", fresh: true, levelM: 100, areaM2: 4e6, depthM: 9,
        anchor: [1, 1], outline: [], shoreMemberCount: 0, downstream: { kind: "ocean" }, outletReach: null,
      },
      {
        id: 1, kind: "pond", fresh: true, levelM: 90, areaM2: 6e4, depthM: 3,
        anchor: [2, 2],
        outline: [[2, 2], [2.001, 2], [2.001, 2.001], [2, 2.001]],
        shoreMemberCount: 0,
        downstream: { kind: "reach", id: 0 }, outletReach: null,
      },
    ],
    reaches: [], notches: 0, falls: [],
  };
  const drawn = drawPreview(viewer, Cesium, decoded);

  assert.equal(drawn.counts.ponds, 1);
  assert.equal(drawn.counts.pondOutlines, 1);
  // Body count still counts both bodies; the ring count only counts the one drawn as a ring.
  assert.equal(drawn.counts.bodies, 2);
  assert.equal(drawn.counts.rings, 1);

  const polygons = drawn.source.entities.list.filter((e) => e.polygon);
  assert.equal(polygons.length, 1, "only the pond is drawn as a polygon");
  const [pondEntity] = polygons;
  // fakeCesium's fromDegreesArray is the identity, so the hierarchy is the flat lon/lat array
  // built from the outline's [lat, lon] points, same order the reach polylines use.
  const expectedFlat = decoded.bodies[1].outline.flatMap(([lat, lon]) => [lon, lat]);
  assert.deepEqual(pondEntity.polygon.hierarchy, expectedFlat);
  assert.equal(pondEntity.polygon.clampToGround, true);
  // fakeCesium's withAlpha ignores its argument and returns the underlying css string.
  assert.equal(pondEntity.polygon.material, "#3aa7e0");
  assert.equal(pondEntity.polygon.outline, true);
  assert.equal(pondEntity.polygon.outlineColor.css, "#3aa7e0");
  assert.match(pondEntity.description, /3\.0 m/); // depth
  assert.match(pondEntity.description, /6\.00 ha/); // 6e4 m2 -> 6.00 ha
  assert.match(pondEntity.description, /reach 0/); // downstream reach it drains to

  // The lake with an empty outline still gets its usual true-size ring, no polygon.
  const rings = drawn.source.entities.list.filter((e) => e.ellipse);
  assert.equal(rings.length, 1);
  assert.equal(rings[0].position.lat, 1);
});

test("drawPreview draws a coarse body with shore points (a non-empty outline, shoreMemberCount > 0) as a ring, not a polygon", () => {
  // SCHEMA 6, plan 1b-4, Task 1: the discriminator is on the wire but every coarse extent is
  // still empty in this task -- Task 2 is what actually gives a coarse body a non-empty
  // outline. This is the forward-looking case the hard ordering constraint exists for: a body
  // whose outline is non-empty AND whose shoreMemberCount is > 0 is a shore-point set, and must
  // never be drawn as the traced-ring polygon `outline.length > 0` alone would have drawn.
  const Cesium = fakeCesium();
  const viewer = { dataSources: { contains: () => false, add: (source) => source } };
  const decoded = {
    header: { schema: 6, pondsKept: 0, crossingsLeft: 0 },
    bodies: [
      {
        id: 0, kind: "lake", fresh: true, levelM: 100, areaM2: 4e6, depthM: 9,
        anchor: [1, 1],
        outline: [[1, 1], [1.001, 1], [1.001, 1.001], [1, 1.001]],
        shoreMemberCount: 3,
        downstream: { kind: "ocean" }, outletReach: null,
      },
    ],
    reaches: [], notches: 0, falls: [],
  };
  const drawn = drawPreview(viewer, Cesium, decoded);

  assert.equal(drawn.counts.ponds, 0);
  assert.equal(drawn.counts.pondOutlines, 0);
  assert.equal(drawn.counts.bodies, 1);
  assert.equal(drawn.counts.rings, 1);

  const polygons = drawn.source.entities.list.filter((e) => e.polygon);
  assert.equal(polygons.length, 0, "a shore-point set is never drawn as a polygon");
  const rings = drawn.source.entities.list.filter((e) => e.ellipse);
  assert.equal(rings.length, 1, "it gets the usual true-size ring instead");
});

test("drawPreview outlines a salt body in the salt colour and a fresh body in the fresh colour", () => {
  const Cesium = fakeCesium();
  const viewer = { dataSources: { contains: () => false, add: (source) => source } };
  const fresh = {
    id: 0, kind: "lake", fresh: true, levelM: 1, areaM2: 4e6, outletReach: null, anchor: [0, 0],
  };
  const salt = {
    id: 1, kind: "saltLake", fresh: false, levelM: 1, areaM2: 4e6, outletReach: null, anchor: [1, 1],
  };
  const decoded = { bodies: [fresh, salt], reaches: [], notches: 0, falls: [] };
  const drawn = drawPreview(viewer, Cesium, decoded);

  const rings = drawn.source.entities.list.filter((e) => e.ellipse);
  const freshRing = rings.find((e) => e.position.lat === 0);
  const saltRing = rings.find((e) => e.position.lat === 1);
  assert.equal(freshRing.ellipse.outlineColor.css, "#3aa7e0");
  assert.equal(saltRing.ellipse.outlineColor.css, "#ffffff");
});
