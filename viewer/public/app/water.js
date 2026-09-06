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
// 1. **There is no footprint** -- but the box is not the over-approximation it was taken for.
//    `water.rs::lake_body_extents` builds it as `Extent::from_points` over
//    `positions[member].to_latlon()` for the body's submerged members, so **it bounds node
//    CENTRES, not water**, and its resolution is one node spacing: 0.6616 degrees of arc at the
//    shipped 30,000 nodes, which is 52 km on the owner's world and a quarter of its largest
//    body's own width. That is why `dilateBodyExtents` exists and why one node cell radius is
//    the size of the correction -- see that function for the calibration, which has actual
//    ground truth in it. The test drawn here is `within one node cell radius of the box ON THE
//    GREAT CIRCLE, AND at or below the body's level`; the level test is what stops the water, and
//    the box is a search hint. **The dilation is a distance, so the shape it draws is a disc**,
//    which is the whole of `bodyContains`'s second half and the answer to "why all the straight
//    lines".
// 2. **There is no representative point and no radius.** `rootNode` is an index into a stream
//    graph the viewer cannot see -- `stream::node_positions` is not an export -- so a body
//    cannot be located except through its box. A point box IS that position, though, which is
//    what makes (3) recoverable.
// 3. **A single-node body's box is a POINT**, `minLat == maxLat` and `minLon == maxLon`, and a
//    point has zero measure: no texel centre ever lands on one. **Measured, and it is not a rare
//    corner:** on the owner's world at `node_count = 30,000`, **38 of 55 bodies are point boxes**;
//    on `DEFAULT_WORLD`, 60 of 156. Until this task all 38 and all 60 were undrawn.
//    `dilateBodyExtents` gives each of them the one node cell it stands for -- not an invented
//    radius, but `4 * pi * R^2 / nodeCount`, the share of the sphere `wb_water_run` itself
//    allotted that node -- and **both worlds go to 100% of bodies drawn**, with no point body
//    drawing more water than that one cell can hold. That cell is drawn as **a spherical cap**,
//    which is what "this body occupies one node cell" means; drawn as a square it was 4/pi of a
//    cell, and on the owner's 86,000-node world one of the 422 point bodies exceeded the cell it
//    cannot exceed. The counts are still reported, because a dilated point box is still a body
//    whose true shape the manifest never carried.
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

/// **The angular radius of one stream node's cell**, in degrees of great-circle arc.
///
/// This is the number the whole box question turns on, and it comes from the engine's own
/// construction rather than from anything chosen here. `water.rs::lake_body_extents` builds a
/// body's `Extent` as `Extent::from_points` over `positions[member].to_latlon()` for every member
/// whose height is at or below the level -- so **the box bounds submerged NODE CENTRES, not
/// water**. Each of those centres owns a share of the sphere: `wb_water_run` samples `nodeCount`
/// nodes over `4 * pi * R^2`, so one node's share is `4 * pi * R^2 / nodeCount`, and the radius of
/// the equal-area disc is `sqrt(4 * R^2 / nodeCount) = 2 * R / sqrt(nodeCount)`.
///
/// **The planet's radius cancels when that is expressed as an angle**, which is why this function
/// does not take one: `r / R` in radians is `2 / sqrt(nodeCount)` whatever the world is. At the
/// shipped 30,000 nodes that is **0.6616 degrees** -- 52.0 km on the owner's 4,500,000 m world and
/// 73.6 km on `DEFAULT_WORLD`'s 6,371,000 m one.
///
/// Nothing here is a footprint the engine did not compute. It is the statement that a bounding box
/// over points is smaller than the bounding box over the cells those points stand for, by exactly
/// one cell radius on every side.
export function nodeCellRadiusDeg(nodeCount) {
  if (!(nodeCount > 0)) return 0;
  return (2 / Math.sqrt(nodeCount)) * (180 / Math.PI);
}

/// A longitude folded into `[-180, 180)`. Positive-modulo, so it is correct for both signs.
function wrapLongitudeDeg(longitudeDeg) {
  return ((((longitudeDeg + 180) % 360) + 360) % 360) - 180;
}

/// **The great-circle distance from a point to a lat/lon box, in degrees of arc.** Zero inside.
///
/// This is what makes the dilation the shape its own construction describes. One node cell radius
/// is an ANGULAR radius -- `2 / sqrt(nodeCount)` radians, the planet's radius having cancelled --
/// and the set of points within an angular radius of a region is a **great-circle disc swept along
/// that region**, not an axis-aligned lat/lon box. A box was never the shape the radius licensed:
/// it is anisotropic (a degree of longitude is `cos(latitude)` of a degree of arc, so a box that
/// gains one cell at its poleward edge gains more than one at its equatorward edge), and it has
/// four corners that sit `sqrt(2)` cell radii from the nearest node centre -- ground the
/// construction never reached.
///
/// Three cases, and the first two are exact by inspection:
///
/// - **Inside the longitude arc.** A meridian is a great circle, so the distance is the plain
///   latitude gap, and zero when the latitude is inside too.
/// - **Outside the arc.** The nearest point lies on the nearer of the two meridian edges. Along a
///   meridian at longitude offset `dl`, the cosine of the distance to a point at latitude `f` is
///   `sin(f) sin(g) + cos(f) cos(g) cos(dl)` for the edge point's latitude `g`; that is maximised
///   at `tan(g) = tan(f) / cos(dl)`, so the candidates are that stationary latitude when it falls
///   inside the box's range, and the two ends of the range otherwise. Taking the largest cosine of
///   the three is exact and needs no case analysis for corners: a corner IS the range end.
///
/// **No `Math.min`/`Math.max` and no clamp**, this project's standing rule: a NaN latitude gives a
/// NaN cosine, every `>` comparison against it is false, `Math.acos` returns NaN, and the caller's
/// `<= padDeg` is false -- so a NaN propagates into a body that visibly fails to draw rather than
/// being silently absorbed into a plausible one.
export function angularDistanceToBoxDeg(box, latitudeDeg, longitudeDeg) {
  const span = longitudeSpanDeg(box);
  const east = eastwardDeg(box.minLongitudeDeg, longitudeDeg);
  if (east <= span) {
    if (latitudeDeg < box.minLatitudeDeg) return box.minLatitudeDeg - latitudeDeg;
    if (latitudeDeg > box.maxLatitudeDeg) return latitudeDeg - box.maxLatitudeDeg;
    // Neither below nor above is "inside" for a real latitude and **also true for NaN**, so the
    // inside case is asserted positively and NaN falls through to NaN. Written as a bare `return
    // 0` this branch was the one place in the function that silently absorbed a NaN latitude into
    // a body that draws -- found by the test that asks, not by reading.
    if (latitudeDeg >= box.minLatitudeDeg && latitudeDeg <= box.maxLatitudeDeg) return 0;
    return Number.NaN;
  }
  // Outside the arc: the shorter way round to an edge, west or east.
  const toWest = 360 - east;
  const toEast = east - span;
  const deltaLonRad = ((toWest < toEast ? toWest : toEast) * Math.PI) / 180;
  const f = (latitudeDeg * Math.PI) / 180;
  const south = (box.minLatitudeDeg * Math.PI) / 180;
  const north = (box.maxLatitudeDeg * Math.PI) / 180;
  const cosDelta = Math.cos(deltaLonRad);
  const stationary = Math.atan(Math.tan(f) / cosDelta);
  const cosine = (g) => Math.sin(f) * Math.sin(g) + Math.cos(f) * Math.cos(g) * cosDelta;
  let best = cosine(south);
  const atNorth = cosine(north);
  if (atNorth > best) best = atNorth;
  if (stationary > south && stationary < north) {
    const atStationary = cosine(stationary);
    if (atStationary > best) best = atStationary;
  }
  // `acos` is undefined a hair outside [-1, 1], which rounding can produce for a coincident point.
  // Written as two comparisons rather than a clamp so NaN passes straight through.
  const bounded = best > 1 ? 1 : (best < -1 ? -1 : best);
  return (Math.acos(bounded) * 180) / Math.PI;
}

/// **Every body's box, grown by one node cell radius -- as a distance on the sphere, not as a
/// bigger rectangle.**
///
/// # The shape, and why the rectangle was always the wrong one
///
/// The owner's report was "why am I getting all these straight lines? Nature builds very little
/// straight." They are right, and the cause is in this function rather than in the engine: the
/// dilation is an ANGULAR radius, `2 / sqrt(nodeCount)` radians, and **a great-circle distance test
/// against that radius draws the disc the radius actually describes.** An axis-aligned lat/lon box
/// was never that disc. It is anisotropic; it distorts with latitude; and its corners lie
/// `sqrt(2)` cell radii from the nearest node centre, which the construction below does not
/// license. A single-node body drawn as its node cell IS a spherical cap, and 422 of this world's
/// 963 bodies are single-node.
///
/// So the drawn set is `{ p : distance(p, raw box) <= one cell radius }` -- the union of one node
/// cell's disc centred at every point the box bounds, which for a point box is exactly one cap.
/// The returned `minLatitudeDeg`..`maxLongitudeDeg` are still a rectangle, and still the dilated
/// one, because `bodiesOverlappingRectangle` needs a cheap conservative box to reject tiles with;
/// the rectangle is a **search hint** and `core*` plus `padDeg` are the shape. The rectangle
/// contains the shape by construction: its longitude pad is measured at the poleward edge, where a
/// degree of longitude is shortest, so it is at least the pad any latitude inside it needs.
///
/// **The measurement is in `bodyContains` and it did not go the way the obvious guess does**: the
/// perimeter exposure barely moves (25.5% to 25.8%), because exposure is a property of a shape's
/// SIZE and the disc is smaller than the square that held it. What collapses is the straight cut
/// itself -- 25.5% of the boundary to 15.3%, and to 0.0% on every point body.
///

/// # What this fixes, and it is two separate defects with one cause
///
/// 1. **38 of the owner's 55 bodies could not be drawn at all** (60 of `DEFAULT_WORLD`'s 156),
///    because a single-node body's box is a *point* and a point has zero measure, so no texel
///    centre ever landed on one. A single node is not a body of zero size; it is a body of one
///    **cell**, and this gives it that cell. Both worlds go to **100% drawable**.
/// 2. **The box's straight edges were visible at 900 km**, because the drawn set is
///    `box AND at-or-below-level` and the box was cutting through ground the level test would
///    have kept. Growing the box moves that cut outward, where more of it lands on ground above
///    the level and the *terrain* becomes what stops the water.
///
/// # Why one cell radius, and not a number picked to look right
///
/// **The single-node bodies are ground truth, which is rare enough in this project to say out
/// loud.** A one-node body's water is at most exactly one cell -- `4 * pi * R^2 / nodeCount`,
/// 8,482 km^2 on the owner's world and 17,002 km^2 on `DEFAULT_WORLD` -- and that is an identity,
/// not an estimate. So the dilation can be calibrated against a bound it must not cross. Measured,
/// at three dilations, over all 38 and all 60 point-box bodies, sampling each dilated box at 64x64
/// through `wb_fill_tile_f32`:
///
/// ```text
///   dilation      mean water drawn per point body      bodies exceeding their one-cell ceiling
///   (cell radii)     owner's        DEFAULT_WORLD          owner's        DEFAULT_WORLD
///     0.5            9.1 %              7.5 %               0 / 38            0 / 60
///     1.0           28.6 %             26.5 %               0 / 38            0 / 60
///     1.27          43.0 %             42.9 %               1 / 38            2 / 60
/// ```
///
/// At **1.0** every body draws, and not one draws more water than the single cell it can
/// physically hold -- the level test, not the box, is what stops it. At 1.27 (the square that
/// circumscribes the equal-area disc rather than inscribes its radius) the physical bound is
/// crossed on both worlds, which is the measurement that rules out going further. At 0.5 the
/// bodies draw under a tenth of their cell, which is the measurement that rules out going less.
///
/// **And that is the calibration's own argument for the disc, arrived at from the other side.** A
/// box of half-width one cell radius has area `4r^2`; the disc of that radius has `pi r^2`. The
/// box IS the 1.27 row -- it is the circumscribing square, by `4/pi = 1.2732` -- and the table says
/// the circumscribing square crosses the physical bound. It did not show on the owner's earlier
/// world because the level test happened to absorb it there. It shows now: re-measured on the
/// world where the defect was reported (**seed 636659598, radius 9,309,000 m, 20 plates, land
/// 0.4, mountains `ranges` and coastline `fractal`, 86,000 nodes: 963 bodies, 422 of them point
/// boxes**), 96x96 per body through `wb_fill_tile_f32` at canonical resolution:
///
/// ```text
///   shape                   mean water drawn per point body    bodies over their one-cell ceiling
///   dilated BOX (shipped)             33.4 %                            1 / 422
///   one node cell's DISC              26.9 %                            0 / 422
/// ```
///
/// The disc restores the bound the box was calibrated against. Total drawn water moves by 2.3%
/// (40.62 to 39.69 million km^2 over all 963 bodies), which is the corner area the construction
/// never licensed and nothing else.
///
/// And the straight-edge exposure, over **all** bodies -- the fraction of the box perimeter that
/// is at or below the body's own level, i.e. the fraction of the boundary at which the box rather
/// than the terrain decides where the water stops (512 samples per edge, `wb_elevation_m` at
/// canonical resolution):
///
/// ```text
///   dilation      owner's world      DEFAULT_WORLD
///     0.0            47.8 %             31.3 %
///     1.0            20.7 %             22.5 %
/// ```
///
/// **Halved, not eliminated.** A fifth of the boundary is still a straight cut, and that is the
/// residue of the real gap: the manifest carries no footprint, and one cell radius is the largest
/// correction its construction actually licenses. See this file's module doc.
///
/// # What the disc changed, measured the same way
///
/// Same definition of exposure, same estimator, on the world the defect was reported on (963
/// bodies at 86,000 nodes, 1,024 arc-length-weighted samples per body's boundary,
/// `wb_elevation_m` at canonical resolution). **`straight` is the share of the boundary that is
/// exposed AND is an axis-aligned lat/lon edge** -- the owner's actual complaint, since a corner
/// arc that follows the terrain's own outline is not a straight line:
///
/// ```text
///                                    exposure    of which straight
///   raw box (multi-node only)          34.0 %          34.0 %
///   dilated BOX (shipped)              25.5 %          25.5 %
///   one node cell's DISC               25.8 %          15.3 %
///     -- of which, point bodies        23.5 %           0.0 %
///     -- of which, multi-node          26.3 %          18.7 %
/// ```
///
/// **Exposure does not improve, and it was never going to.** It is the share of a boundary the
/// terrain does not choose, and that is decided by the shape's SIZE; the disc is 21% smaller than
/// the square that held it, so a hair more of its shorter boundary cuts water. Reporting a win
/// there would have meant measuring something else. What the shape decides is whether the cut is
/// straight, and **straight exposure falls by 40%, to zero on every one of the 422 point bodies.**
/// The 18.7% that survives on multi-node bodies is the raw box's own four edges, offset outward:
/// no dilation can round them, because a rectangle is what the engine actually exported.
///
/// # The seam and the poles
///
/// Latitude is bounded at the poles by explicit comparison -- never `Math.min`/`Math.max`, this
/// project's standing rule, so a NaN latitude propagates into a visible artefact instead of being
/// silently absorbed. Longitude is padded by `cellRadius / cos(latitude)`, because a degree of
/// longitude is `cos(latitude)` of a degree of arc and a polar body needs a much wider box to gain
/// the same distance; where that would meet or exceed the whole circle -- which is the case for a
/// body over a pole -- the arc becomes the full 360 degrees, which `bodyContains` already accepts.
/// The `min > max` wrap convention is preserved: `wrapLongitudeDeg` folds the new start back into
/// `[-180, 180)` and the end is the start plus the new span, folded the same way.
export function dilateBodyExtents(bodies, nodeCount) {
  const cellDeg = nodeCellRadiusDeg(nodeCount);
  if (!(cellDeg > 0)) return bodies;
  return bodies.map((body) => {
    // **The shape**, carried beside the search box: the raw box the engine exported, and the
    // angular radius every point of it is grown by. `bodyContains` reads these; the rectangle
    // below is only what rejects a tile cheaply.
    const core = {
      padDeg: cellDeg,
      coreMinLatitudeDeg: body.minLatitudeDeg,
      coreMaxLatitudeDeg: body.maxLatitudeDeg,
      coreMinLongitudeDeg: body.minLongitudeDeg,
      coreMaxLongitudeDeg: body.maxLongitudeDeg,
    };
    let south = body.minLatitudeDeg - cellDeg;
    let north = body.maxLatitudeDeg + cellDeg;
    if (south < -90) south = -90;
    if (north > 90) north = 90;
    // The pad in longitude is measured at whichever of the two latitudes is nearer a pole, so
    // the box gains at least `cellDeg` of arc along its whole height rather than only at its
    // equatorward edge.
    const worstLat = Math.abs(south) > Math.abs(north) ? south : north;
    const cosLat = Math.cos((worstLat * Math.PI) / 180);
    const span = longitudeSpanDeg(body);
    const lonPad = cosLat > 0 ? cellDeg / cosLat : 360;
    const newSpan = span + 2 * lonPad;
    if (!(newSpan < 360)) {
      return {
        ...body, ...core, minLatitudeDeg: south, maxLatitudeDeg: north,
        minLongitudeDeg: -180, maxLongitudeDeg: 180,
      };
    }
    const west = wrapLongitudeDeg(body.minLongitudeDeg - lonPad);
    return {
      ...body,
      ...core,
      minLatitudeDeg: south,
      maxLatitudeDeg: north,
      minLongitudeDeg: west,
      maxLongitudeDeg: wrapLongitudeDeg(west + newSpan),
    };
  });
}

/// Is a point inside a body?
///
/// **Two shapes, and which one is used depends on whether the row has been through
/// `dilateBodyExtents`.** A raw manifest row is only ever a box, so it is tested as one: latitude
/// a plain interval, longitude an arc. Inclusive at both ends, which matters only for a
/// measure-zero set of texels and is the same convention `Extent`'s min/max carry.
///
/// A dilated row carries `padDeg` and its `core*` box, and then the shape is **the set of points
/// within `padDeg` of that box on the great circle** -- one node cell's disc swept along the box,
/// and a single spherical cap when the box is a point. The rectangle is still tested first, and
/// only as a cheap reject: it contains the disc, so a texel outside it cannot be inside, and the
/// trigonometry is paid only by the small minority of texels that survive. That ordering is why
/// the shape change does not show in the tile-fill cost.
export function bodyContains(body, latitudeDeg, longitudeDeg) {
  if (latitudeDeg < body.minLatitudeDeg || latitudeDeg > body.maxLatitudeDeg) return false;
  if (eastwardDeg(body.minLongitudeDeg, longitudeDeg) > longitudeSpanDeg(body)) return false;
  if (!(body.padDeg > 0)) return true;
  const core = {
    minLatitudeDeg: body.coreMinLatitudeDeg,
    maxLatitudeDeg: body.coreMaxLatitudeDeg,
    minLongitudeDeg: body.coreMinLongitudeDeg,
    maxLongitudeDeg: body.coreMaxLongitudeDeg,
  };
  return angularDistanceToBoxDeg(core, latitudeDeg, longitudeDeg) <= body.padDeg;
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
