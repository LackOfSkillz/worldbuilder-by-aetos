//! The studio's view-only water preview: bake the engine's hydrology for the world on
//! screen and draw it over the globe. It carves nothing and saves nothing.
//
// **Decoding is DOM-free on purpose.** `decodeHydro` and `outletPath` read only arrays and
// plain objects, so `node --test` can hold them against the checked-in wasm the same way
// `hydro.test.mjs` does; `drawPreview` is the only export that touches Cesium.
//
// The wire format is `crates/worldbuilder-engine/src/hydrology/record.rs`'s `encode`/`decode`
// pair -- schema 4, Task 3 (plan 1b-2)'s 43-word header. This file is the JS side of that
// contract and mirrors its field order and its refusals (a truncated record, a wrong schema, a
// trailing word, an index or count word outside u32) rather than trusting the words blindly.
//
// Two positions share a slot and not a meaning, and two flags share a name and not a meaning,
// exactly as `record.rs`'s module doc states:
// - a reach point's third word (`bedM`) is the bed: the water surface minus the depth;
// - a notch point's third word is the cut surface: the lowered ground, which is the water
//   surface through the cut, not a bed below it;
// - body `fresh` means "not closed" (it has an outlet, though its water may end in a closed
//   lake); reach `fresh` means "its chain reaches the ocean".

import { showLayer } from "./globe-layers.js";

const SCHEMA = 4;

/// `u32::MAX`: the largest index or count word `record.rs`'s `word_to_u32` accepts.
const U32_MAX = 4294967295;

/// A valid index or count word, the same test `record.rs`'s `word_to_u32` applies: finite,
/// non-negative, integral, and no larger than `u32::MAX`.
function isU32Word(w) {
  return Number.isFinite(w) && w >= 0 && w <= U32_MAX && Math.floor(w) === w;
}

const BODY_KIND = ["lake", "pond", "saltLake", "saltFlat"];
const REACH_CLASS = ["stream", "river", "great"];

/// The engine's `earth_like` hydro-bake defaults, at the resolution the preview runs at:
/// 1,000,000 nodes, about 87 s on the owner's world. The engine raises `streamFlowM2` /
/// `riverFlowM2` / `greatFlowM2` to the graph's own resolution itself (Ruling 12b-1), so
/// these are the floor asked for rather than the thresholds that end up in the header.
export const PREVIEW_PARAMS = {
  totalNodes: 1_000_000,
  wetnessNodes: 20_000,
  keepDepthM: 8,
  keepAreaM2: 1e6,
  pondMaxAreaM2: 1e6,
  streamFlowM2: 2.5e8,
  riverFlowM2: 2.5e9,
  greatFlowM2: 1e11,
  notchFallM: 1,
  evaporationFactor: 1,
  saltFlatShare: 0.1,
  forcedOutlets: [],
};

/// Read `?forcedOutlet=lat,lon` (repeatable) into the `forcedOutlets` shape `hydroBake`
/// expects. A malformed entry -- the wrong number of parts, or either half not a finite
/// number -- is skipped rather than thrown on: a bad query parameter is not a reason to
/// refuse the whole preview.
export function forcedOutletsFromParams(searchParams) {
  const out = [];
  for (const raw of searchParams.getAll("forcedOutlet")) {
    const parts = raw.split(",");
    if (parts.length !== 2) continue;
    const latitudeDeg = Number(parts[0]);
    const longitudeDeg = Number(parts[1]);
    if (!Number.isFinite(latitudeDeg) || !Number.isFinite(longitudeDeg)) continue;
    out.push({ latitudeDeg, longitudeDeg });
  }
  return out;
}

/// A cursor over the word array, mirroring `record.rs`'s `Reader`: every read can run off
/// the end, and every caller is expected to let that throw rather than read past it.
class Cursor {
  constructor(words) {
    this.words = words;
    this.pos = 0;
  }

  word() {
    if (this.pos >= this.words.length) {
      throw new Error("hydro record: truncated (ran out of words mid-record)");
    }
    return this.words[this.pos++];
  }

  /// A count or index word: finite, non-negative, integral and at most `u32::MAX`, as
  /// `record.rs`'s `word_to_u32` requires. `count_fits`' words-left check is not mirrored --
  /// a length mismatch at the end of `decodeHydro` catches the same absurd-count case without
  /// duplicating that arithmetic here.
  u32() {
    const w = this.word();
    if (!isU32Word(w)) {
      throw new Error(`hydro record: bad count/index word ${w}`);
    }
    return w;
  }

  /// `-1` decodes to `null`; anything else must be a valid index word.
  optionalU32() {
    const w = this.word();
    if (w === -1) return null;
    if (!isU32Word(w)) {
      throw new Error(`hydro record: bad optional index word ${w}`);
    }
    return w;
  }

  boolean() {
    const w = this.word();
    if (w === 0) return false;
    if (w === 1) return true;
    throw new Error(`hydro record: bad boolean word ${w}`);
  }
}

function readDownstream(cursor) {
  const kindWord = cursor.word();
  const idWord = cursor.word();
  if (kindWord === 0) {
    if (!isU32Word(idWord)) {
      throw new Error(`hydro record: bad downstream reach id ${idWord}`);
    }
    return { kind: "reach", id: idWord };
  }
  if (kindWord === 1) {
    if (!isU32Word(idWord)) {
      throw new Error(`hydro record: bad downstream body id ${idWord}`);
    }
    return { kind: "body", id: idWord };
  }
  if (kindWord === 2) {
    if (idWord !== -1) throw new Error("hydro record: ocean downstream must carry -1");
    return { kind: "ocean" };
  }
  if (kindWord === 3) {
    if (idWord !== -1) throw new Error("hydro record: sink downstream must carry -1");
    return { kind: "sink" };
  }
  throw new Error(`hydro record: bad downstream kind ${kindWord}`);
}

/// Decode a `hydroBake` record. Throws on a schema other than 4, on a truncated array, or on
/// a length mismatch (extra trailing words, or a count that does not add up) -- never returns
/// a partial record.
export function decodeHydro(words) {
  const cursor = new Cursor(words);

  const schema = cursor.word();
  if (schema !== SCHEMA) {
    throw new Error(`hydro record: unsupported schema ${schema} (expected ${SCHEMA})`);
  }

  const bodyCount = cursor.u32();
  const reachCount = cursor.u32();
  const notchCount = cursor.u32();
  const fallCount = cursor.u32();

  const header = {
    schema,
    bodies: bodyCount,
    reaches: reachCount,
    notches: notchCount,
    falls: fallCount,
    nodes: cursor.u32(),
    landNodes: cursor.u32(),
    hollows: cursor.u32(),
    kept: cursor.u32(),
    notched: cursor.u32(),
    closed: cursor.u32(),
    streams: cursor.u32(),
    rivers: cursor.u32(),
    great: cursor.u32(),
    maxOrder: cursor.u32(),
    bifurcationMin: cursor.word(),
    bifurcationMax: cursor.word(),
    streamFlowM2: cursor.word(),
    riverFlowM2: cursor.word(),
    greatFlowM2: cursor.word(),
    // SCHEMA 3's params echo (words 20-29) and forced-outlet match counts (words 30-31) --
    // mirrors `hydroSummary`'s field names in `engine.js`.
    totalNodes: cursor.u32(),
    wetnessNodes: cursor.u32(),
    keepDepthM: cursor.word(),
    keepAreaM2: cursor.word(),
    pondMaxAreaM2: cursor.word(),
    keepMaxAreaM2: cursor.word(),
    minStreamNodes: cursor.word(),
    notchFallM: cursor.word(),
    evaporationFactor: cursor.word(),
    saltFlatShare: cursor.word(),
    forcedRequested: cursor.u32(),
    forcedMatched: cursor.u32(),
    // SCHEMA 4's counts of what capped basins keep (words 32-34, carry-forward I3) and the
    // refinement params echo (words 35-42).
    cappedBasins: cursor.u32(),
    cappedInner: cursor.u32(),
    cappedInnerKept: cursor.u32(),
    refineStepM: cursor.word(),
    refineSimplifyM: cursor.word(),
    refineVerticalM: cursor.word(),
    fallMinDropM: cursor.word(),
    fallMaxRunM: cursor.word(),
    meanderWavelengthWidths: cursor.word(),
    meanderAmplitudeWidths: cursor.word(),
    meanderMaxSlope: cursor.word(),
  };

  const bodies = [];
  for (let i = 0; i < bodyCount; i += 1) {
    const id = cursor.u32();
    const kindWord = cursor.word();
    if (!(Number.isInteger(kindWord) && kindWord >= 0 && kindWord < BODY_KIND.length)) {
      throw new Error(`hydro record: bad body kind ${kindWord}`);
    }
    const kind = BODY_KIND[kindWord];
    const fresh = cursor.boolean();
    const enclosed = cursor.boolean();
    const forced = cursor.boolean();
    const levelM = cursor.word();
    const areaM2 = cursor.word();
    const depthM = cursor.word();
    const outletReach = cursor.optionalU32();
    const anchorLat = cursor.word();
    const anchorLon = cursor.word();
    const downstream = readDownstream(cursor);
    const outlineLen = cursor.u32();
    const outline = [];
    for (let j = 0; j < outlineLen; j += 1) {
      outline.push([cursor.word(), cursor.word()]);
    }
    bodies.push({
      id, kind, fresh, enclosed, forced, levelM, areaM2, depthM, outletReach,
      anchor: [anchorLat, anchorLon], downstream, outline,
    });
  }

  const reaches = [];
  for (let i = 0; i < reachCount; i += 1) {
    const id = cursor.u32();
    const classWord = cursor.word();
    if (!(Number.isInteger(classWord) && classWord >= 0 && classWord < REACH_CLASS.length)) {
      throw new Error(`hydro record: bad reach class ${classWord}`);
    }
    const reachClass = REACH_CLASS[classWord];
    const order = cursor.u32();
    const downstream = readDownstream(cursor);
    const fresh = cursor.boolean();
    const pointCount = cursor.u32();
    const points = [];
    for (let j = 0; j < pointCount; j += 1) {
      points.push({
        lat: cursor.word(), lon: cursor.word(), bedM: cursor.word(),
        widthM: cursor.word(), depthM: cursor.word(), flowM2: cursor.word(),
      });
    }
    reaches.push({ id, class: reachClass, order, downstream, fresh, points });
  }

  // Notch geometry carves the ground; the preview draws none of it, so only the count is
  // kept -- but every word still has to be walked, or the falls below would be read starting
  // mid-notch. Each point is lat, lon, the cut surface (not a bed -- see the file comment
  // above), width.
  for (let i = 0; i < notchCount; i += 1) {
    const pointCount = cursor.u32();
    for (let j = 0; j < pointCount; j += 1) {
      cursor.word(); cursor.word(); cursor.word(); cursor.word();
    }
  }

  const falls = [];
  for (let i = 0; i < fallCount; i += 1) {
    falls.push({
      reach: cursor.u32(), lat: cursor.word(), lon: cursor.word(), heightM: cursor.word(),
    });
  }

  if (cursor.pos !== words.length) {
    throw new Error(
      `hydro record: ${words.length - cursor.pos} trailing word(s) after a complete record`,
    );
  }

  return { header, bodies, reaches, notches: notchCount, falls };
}

/// Follow the outlet chain from a starting body -- a forced body if one exists, else the
/// largest fresh enclosed body -- through `outlet_reach`, each reach's downstream link, and
/// any body that link lands on, until Ocean, Sink or a body/reach with no further outlet.
///
/// Guarded against loops: each body and reach visited is remembered, and revisiting one ends
/// the walk at `"none"` instead of spinning forever on a record with a cycle in it.
export function outletPath(decoded) {
  const bodyById = new Map(decoded.bodies.map((b) => [b.id, b]));
  const reachById = new Map(decoded.reaches.map((r) => [r.id, r]));

  let start = decoded.bodies.find((b) => b.forced);
  if (!start) {
    for (const b of decoded.bodies) {
      if (!b.fresh || !b.enclosed) continue;
      if (!start || b.areaM2 > start.areaM2) start = b;
    }
  }

  const reachIds = [];
  const bodyIds = [];
  if (!start) return { end: "none", reachIds, bodyIds };

  const visited = new Set();
  let node = { kind: "body", id: start.id };

  while (node) {
    const key = `${node.kind}:${node.id}`;
    if (visited.has(key)) return { end: "none", reachIds, bodyIds };
    visited.add(key);

    if (node.kind === "body") {
      bodyIds.push(node.id);
      const body = bodyById.get(node.id);
      if (!body || body.outletReach === null || body.outletReach === undefined) {
        return { end: "none", reachIds, bodyIds };
      }
      node = { kind: "reach", id: body.outletReach };
      continue;
    }

    reachIds.push(node.id);
    const reach = reachById.get(node.id);
    if (!reach) return { end: "none", reachIds, bodyIds };
    const { downstream } = reach;
    if (downstream.kind === "ocean") return { end: "ocean", reachIds, bodyIds };
    if (downstream.kind === "sink") return { end: "sink", reachIds, bodyIds };
    if (downstream.kind === "reach") { node = { kind: "reach", id: downstream.id }; continue; }
    if (downstream.kind === "body") { node = { kind: "body", id: downstream.id }; continue; }
    return { end: "none", reachIds, bodyIds };
  }
  return { end: "none", reachIds, bodyIds };
}

/// Width and colour by reach class -- the stream and river colours match `hydrology.js`'s
/// carved-course lines; `great` is a deeper blue, one step past `river`, for a class that
/// file never draws.
const REACH_STYLE = {
  stream: { width: 1.5, color: "#5fd0d8" },
  river: { width: 3, color: "#3aa7e0" },
  great: { width: 5, color: "#1c4e80" },
};

/// **Own scaling choice, not from the brief or the engine**: `sqrt(areaM2)` is metres, and
/// mapping it straight to pixels would put every lake at the clamp. Divided by 500 so the
/// 1 km2 keep-area floor sits near the small end and a lake two orders of magnitude bigger
/// sits near the large end, then clamped to 6..22 px either way.
function bodyPixelSize(areaM2) {
  return Math.max(6, Math.min(22, Math.sqrt(Math.max(0, areaM2)) / 500));
}

/// Draw a decoded hydro-bake record over the globe: every reach as a polyline, the outlet
/// path from the largest (or forced) fresh body drawn again on top in amber, every body as a
/// point sized by its area, and every waterfall as a small white point.
///
/// Never depth-tested and always clamped to ground, the same rules `hydrology.js` draws
/// carved courses by -- a preview line that sinks into the terrain it has not carved is worse
/// than no preview.
///
/// Returns `{ source, counts, remove() }`.
export function drawPreview(viewer, Cesium, decoded) {
  const source = new Cesium.CustomDataSource("wb-water-preview");
  const outlet = outletPath(decoded);
  const reachById = new Map(decoded.reaches.map((r) => [r.id, r]));

  let reachesDrawn = 0;
  for (const reach of decoded.reaches) {
    if (reach.points.length < 2) continue;
    const positions = [];
    for (const point of reach.points) positions.push(point.lon, point.lat);
    const style = REACH_STYLE[reach.class] || REACH_STYLE.river;
    source.entities.add({
      polyline: {
        positions: Cesium.Cartesian3.fromDegreesArray(positions),
        width: style.width,
        material: Cesium.Color.fromCssColorString(style.color).withAlpha(0.9),
        clampToGround: true,
      },
    });
    reachesDrawn += 1;
  }

  for (const reachId of outlet.reachIds) {
    const reach = reachById.get(reachId);
    if (!reach || reach.points.length < 2) continue;
    const positions = [];
    for (const point of reach.points) positions.push(point.lon, point.lat);
    source.entities.add({
      polyline: {
        positions: Cesium.Cartesian3.fromDegreesArray(positions),
        width: 5,
        material: Cesium.Color.fromCssColorString("#f4a300").withAlpha(0.95),
        clampToGround: true,
      },
    });
  }

  let bodiesDrawn = 0;
  for (const body of decoded.bodies) {
    const salt = body.kind === "saltLake" || body.kind === "saltFlat";
    source.entities.add({
      position: Cesium.Cartesian3.fromDegrees(body.anchor[1], body.anchor[0]),
      point: {
        pixelSize: bodyPixelSize(body.areaM2),
        color: Cesium.Color.fromCssColorString(salt ? "#ffffff" : "#3aa7e0"),
        disableDepthTestDistance: Number.POSITIVE_INFINITY,
      },
      description: [
        body.kind,
        `${(body.areaM2 / 1e6).toFixed(2)} km2`,
        `level ${body.levelM.toFixed(1)} m`,
        body.fresh ? "fresh" : "salt",
        `outlet reach ${body.outletReach === null ? "none" : body.outletReach}`,
      ].join("\n"),
    });
    bodiesDrawn += 1;
  }

  // Spec §6.7 falls, at their upper end (Ruling R-5): white points over every reach line, so a
  // fall on a river still shows.
  let fallsDrawn = 0;
  for (const fall of decoded.falls) {
    source.entities.add({
      position: Cesium.Cartesian3.fromDegrees(fall.lon, fall.lat),
      point: {
        pixelSize: 7,
        color: Cesium.Color.WHITE,
        disableDepthTestDistance: Number.POSITIVE_INFINITY,
      },
      description: `waterfall, ${fall.heightM.toFixed(1)} m`,
    });
    fallsDrawn += 1;
  }

  const layer = showLayer(viewer, source);
  return {
    source,
    counts: {
      reaches: reachesDrawn, bodies: bodiesDrawn, falls: fallsDrawn, outletReaches: outlet.reachIds.length,
    },
    remove: () => layer.remove(true),
  };
}
