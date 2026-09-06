//! The water manifest, as the picture needs it: which body's surface -- if any -- covers a
//! point, and which bodies can possibly touch one tile.
//
// **This file consumes slice 5b. It computes no hydrology of its own.** Every level here came
// out of `wb_water_run`, which runs the whole shipped path (basin fill, overflow resolution,
// the tied-plateau merge, classification) and dumps `water_manifest_from_graph`'s result. A
// second copy of any of that arithmetic in JavaScript would be a second thing that can
// disagree with the engine, which is this project's characteristic defect; there is none here.
//
// DOM-free, Cesium-free and engine-free on purpose, exactly like `panel-fields.js` and
// `clouds.js`: `node --test` holds the same functions the browser draws with, and the body
// records are plain structured-cloneable objects so the worker pool can post them per tile.
//
// # What the manifest gives, and the three things it does not
//
// A body row is `{ rootNode, kind, levelM, minLatitudeDeg, maxLatitudeDeg, minLongitudeDeg,
// maxLongitudeDeg }` -- a **surface level** and a **bounding box**, and nothing else. So:
//
// 1. **There is no footprint.** The box is a deliberate over-approximation of the body's true
//    irregular shore (`water.rs::Extent` argues the choice at length and says so). The only
//    other thing that can narrow it back down is the height field itself, so the test drawn
//    here is `inside the box AND at or below the body's level`. That is exact wherever the box
//    holds no *other* depression below the same level, and over-paints where it does.
// 2. **There is no representative point and no radius.** `rootNode` is an index into a stream
//    graph the viewer cannot see -- `stream::node_positions` is not an export -- so a body
//    cannot be located except through its box.
// 3. **A single-node body's box is a POINT**, `minLat == maxLat` and `minLon == maxLon`, and a
//    point has zero measure: no texel centre ever lands on one, so such a body can never be
//    drawn. It is not skipped by a rule in this file -- the containment test simply never
//    fires. **Measured, and it is not a rare corner:** on the owner's world at
//    `node_count = 30,000`, **38 of 55 bodies are point boxes and 17 are drawable**; on
//    `DEFAULT_WORLD`, 60 of 156 and 96 drawable. Both counts are in the task report with their
//    method, and the gap is reported to the engine side rather than papered over here with an
//    invented radius.
//
// # The pole, not the antimeridian
//
// `Extent` normalises the seam: it stores the **smallest enclosing arc** on the longitude
// circle, and expresses an arc that crosses +/-180 by leaving `minLongitudeDeg >
// maxLongitudeDeg`. `bodyContains` branches on exactly that and nothing else.
//
// The slice's ledger warns that a body straddling the antimeridian "gets a bounding box
// spanning nearly the planet", quoting **6 of 171 bodies at 30,000 nodes** with 356-360 degree
// spans. **Re-measured on the shipped export, that is no longer the failure mode, and the one
// that remains is a different one.** On the owner's world at 30,000 nodes **no body has a
// longitude span over 4.50 degrees** and none wraps the seam at all; on `DEFAULT_WORLD` two of
// 156 have spans over 180 degrees (216.02 and 211.42) and **both are POLAR** -- latitude
// 87.28..89.64 and -89.21..-86.19. That is not the seam and normalising the seam cannot help
// it: near a pole a physically small body genuinely occupies most of the longitude circle. The
// two boxes are 216 and 211 degrees wide and **17.9 and 30.4 thousand km^2 in actual area**,
// because a degree of longitude is nearly nothing at latitude 88.
//
// **No body is filtered out here on account of a wide box.** A filter would need a criterion
// the manifest cannot support -- it exports no surface area to compare a box against -- so it
// would be the viewer inventing a threshold and silently dropping real water. What is done
// instead is `waterDiagnostics` below, which counts the degenerate and the wide-box cases so
// they are visible in the status line and in `window.__wb` rather than discovered later.
//
// # Overlapping boxes, and why the shallowest wins
//
// Two bodies' boxes can cover one point, and the manifest carries no rule for that. Slice 5b
// met the same ambiguity from the other side and it is why the sea is not enumerated: ocean
// boxes overlapped 96.3% of the time and "a sea position selected an ambiguous set with
// identical levels and no disambiguation rule". `lakeLevelAt` takes the **lowest** level among
// the candidates, which is the conservative direction -- it draws the least water and the
// shallowest sheet -- and `waterDiagnostics` counts how often the choice is even available so
// the ambiguity is a number rather than a silence.

/// Node count handed to `wb_water_run`, and **the figure every body count in this task is
/// quoted beside**, because the population depends on it: the owner's world resolves 55 bodies
/// at 30,000 nodes and 351 at 100,000.
///
/// 30,000 is chosen for cost, and the cost was measured rather than assumed. On the owner's
/// world, node 22.17.0, this repository's checked-in wasm, one `wb_water_run` call takes
/// **0.98 s at 8,000 nodes, 2.05 s at 15,000, 4.21 s at 30,000 and 9.32 s at 60,000** --
/// roughly linear in the node count, and paid **once, synchronously, at boot**. See
/// `main.js` for why it is not moved off the main thread.
export const DEFAULT_WATER_NODES = 30000;

/// **`?lakes=0` turns the water off, and nothing else does.**
///
/// Deliberately the same shape as `reliefLayerEnabled`'s `?relief=0` and `cloudLayerEnabled`'s
/// `?clouds=0`: one switch convention in this viewer rather than a fourth one introduced
/// beside them. With it off no manifest is resolved at all -- not resolved and ignored -- so
/// the boot cost above is not paid either, and the picture is byte-for-byte the one this task
/// started from.
export function waterEnabled(params) {
  return params.get("lakes") !== "0";
}

/// `?lakeNodes=N`. Absent is `DEFAULT_WATER_NODES`; a value the engine will refuse is left to
/// the engine to refuse, because `wb_water_run` owns that bound and a second copy of it here
/// would be a second chance to disagree with it.
export function waterNodeCountFromParams(params, fallback = DEFAULT_WATER_NODES) {
  return params.has("lakeNodes") ? Number(params.get("lakeNodes")) : fallback;
}

/// The width of a body's longitude arc, in degrees, `0..360`.
///
/// This is the one place the `minLongitudeDeg > maxLongitudeDeg` convention is turned into
/// arithmetic. A wrapped arc's raw difference is negative; adding 360 recovers the arc the
/// engine meant.
export function longitudeSpanDeg(body) {
  const raw = body.maxLongitudeDeg - body.minLongitudeDeg;
  return raw < 0 ? raw + 360 : raw;
}

/// How far east of `fromDeg` a longitude sits, in degrees, `0..360`. Positive-modulo, so it is
/// correct for both signs and for the seam.
function eastwardDeg(fromDeg, longitudeDeg) {
  return (((longitudeDeg - fromDeg) % 360) + 360) % 360;
}

/// Is a point inside a body's box? Latitude is a plain interval; longitude is an arc.
///
/// Inclusive at both ends, which matters only for a measure-zero set of texels and is the same
/// convention `Extent`'s min/max carry.
export function bodyContains(body, latitudeDeg, longitudeDeg) {
  if (latitudeDeg < body.minLatitudeDeg || latitudeDeg > body.maxLatitudeDeg) return false;
  return eastwardDeg(body.minLongitudeDeg, longitudeDeg) <= longitudeSpanDeg(body);
}

/// The surface level of the body covering this point, or `null`.
///
/// Three conditions, and the first is the one that keeps the sea out of this:
///
/// - **`heightM > 0`.** The datum is 0 by construction (`wb_elevation_m` is documented as
///   metres above datum) and slice 5b's Ruling 6 says the sea is not a body at all -- it is the
///   named-waters mapping's *miss*, carried once in `sea_level_m`. So a texel at or below the
///   datum is the ocean's and this function never claims it. That is what makes the ocean
///   picture provably untouched by this task rather than merely intended to be.
/// - **inside the box** (`bodyContains`).
/// - **at or below the body's level.** A lake surface is flat at its spill level; ground above
///   that level is the shore, not the lake.
///
/// The lowest qualifying level wins -- see the module doc.
export function lakeLevelAt(bodies, latitudeDeg, longitudeDeg, heightM) {
  if (!(heightM > 0)) return null;
  let best = null;
  for (const body of bodies) {
    if (heightM > body.levelM) continue;
    if (!bodyContains(body, latitudeDeg, longitudeDeg)) continue;
    if (best === null || body.levelM < best) best = body.levelM;
  }
  return best;
}

/// The bodies whose box can possibly touch a `{ northDeg, southDeg, westDeg, eastDeg }`
/// rectangle.
///
/// **This is why drawing lakes is affordable.** A 256-texel tile is 65,536 texels, and testing
/// every one against all 55 bodies would be 3.6 million box tests per tile in a worker that
/// already costs ~190 ms. Almost every tile touches **no** body, and the ones that do touch
/// one or two, so the whole cost collapses to one pass over the manifest per tile and then a
/// loop over a list that is usually empty.
export function bodiesOverlappingRectangle(bodies, rectangle) {
  const { northDeg, southDeg, westDeg, eastDeg } = rectangle;
  const tileSpan = eastDeg - westDeg;
  return bodies.filter((body) => {
    if (body.maxLatitudeDeg < southDeg || body.minLatitudeDeg > northDeg) return false;
    // Two arcs overlap when either one's start lies inside the other. Written both ways round
    // rather than with a single comparison, because "inside" is not symmetric for arcs.
    const bodySpan = longitudeSpanDeg(body);
    return eastwardDeg(westDeg, body.minLongitudeDeg) <= tileSpan
      || eastwardDeg(body.minLongitudeDeg, westDeg) <= bodySpan;
  });
}

/// What the manifest cannot tell the picture, counted.
///
/// Every field here is a **finding with a number**, not a health check: `pointBoxes` is water
/// this viewer cannot draw, `wideBoxes` is where the over-approximation stops being tight, and
/// `overlappingPairs` is where two bodies claim one place and `lakeLevelAt` has to choose.
/// They are surfaced in the status line and on `window.__wb` because a limitation an owner
/// cannot see is a limitation that gets rediscovered.
export function waterDiagnostics(bodies) {
  let pointBoxes = 0;
  let degenerateBoxes = 0;
  let wideBoxes = 0;
  for (const body of bodies) {
    const lonZero = longitudeSpanDeg(body) === 0;
    const latZero = body.minLatitudeDeg === body.maxLatitudeDeg;
    // A box with EITHER span zero is a line, and a line has zero measure just as a point does
    // -- so `degenerateBoxes` is the count that decides what can be drawn, and `pointBoxes` is
    // reported beside it only because it is the case that actually occurs (measured: on both
    // worlds tested, every zero-span box is zero in BOTH axes, so the two counts coincide.
    // They are separated here so that if that ever stops being true it is visible rather than
    // hidden inside one number).
    if (lonZero && latZero) pointBoxes += 1;
    if (lonZero || latZero) degenerateBoxes += 1;
    if (longitudeSpanDeg(body) > 180) wideBoxes += 1;
  }
  let overlappingPairs = 0;
  for (let i = 0; i < bodies.length; i += 1) {
    for (let j = i + 1; j < bodies.length; j += 1) {
      const a = bodies[i];
      const b = bodies[j];
      if (a.maxLatitudeDeg < b.minLatitudeDeg || b.maxLatitudeDeg < a.minLatitudeDeg) continue;
      const aSpan = longitudeSpanDeg(a);
      const bSpan = longitudeSpanDeg(b);
      if (eastwardDeg(a.minLongitudeDeg, b.minLongitudeDeg) <= aSpan
        || eastwardDeg(b.minLongitudeDeg, a.minLongitudeDeg) <= bSpan) {
        overlappingPairs += 1;
      }
    }
  }
  return {
    bodies: bodies.length,
    drawable: bodies.length - degenerateBoxes,
    pointBoxes,
    degenerateBoxes,
    wideBoxes,
    overlappingPairs,
  };
}
