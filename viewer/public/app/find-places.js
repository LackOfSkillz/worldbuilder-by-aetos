// Finding a place again on a world you already have.
//
// **This file exists because somebody refreshed a browser and thought they had lost a world.**
// They had not. A generated planet is entirely described by its seed and its parameters, and
// those live in the query string - so a refresh reproduces the same planet, bit for bit. What a
// refresh loses is the CAMERA, because `?fly=` is read at boot and never written back. The world
// survived; the place in it did not.
//
// That is a navigation problem, and a planet you can ask questions of can answer it. The water
// manifest already knows where every lake is, and the engine answers elevation anywhere, so
// "the island in the lake on the island" is a query rather than a memory.
//
// **What a body row can and cannot say**, because the search is built on its limits:
//
//   - It carries `{ rootNode, kind, levelM, minLatitudeDeg, maxLatitudeDeg, minLongitudeDeg,
//     maxLongitudeDeg }`. A surface level and a bounding box, and nothing else.
//   - There is NO footprint and NO representative point. A body is located only through its box.
//   - A single-node body's box is a POINT, and on the owner's world 38 of 55 bodies are point
//     boxes. Those cannot be searched for an island inside them, because they have no inside.
//   - A body straddling the antimeridian gets a box "spanning nearly the planet" - 6 of 171 at
//     30,000 nodes. Those are skipped rather than scanned, and counted, because scanning one
//     would sample most of the globe to no purpose.
//
// So the search reports what it looked at as well as what it found. A finder that says "nothing
// here" without saying how much of the world it could see is the kind of answer this project
// does not accept.

/// A lake box smaller than this in either axis has no interior worth sampling.
const MIN_SPAN_DEG = 0.01;

/// A box wider than this is the antimeridian artefact, not a lake.
const MAX_SPAN_DEG = 60.0;

// **Two wrong tests preceded the one below, and both failed in the way this project keeps
// finding: they measured something the data does not carry.**
//
// The first asked "is any sample in this box above the water" and reported 91 hits from 91
// lakes. A hundred percent hit rate is not a result - over a box six hundred kilometres across,
// of course some sample stands dry.
//
// The second capped the lake by the width of its bounding box and found nothing at all, from
// twenty boxes of a hundred and fifty-six. That one was worse, because it looked like a
// careful answer. **A body's box is not its footprint** - `water.js` says so in its own header:
// "There is no footprint", "the box is a search hint". Filtering a lake by the size of its
// search hint discards real lakes whose hint happens to be wide, and slice 5b measured exactly
// that case: two boxes 216 and 211 DEGREES wide covering 17.9 and 30.4 thousand square km of
// actual water.
//
// So the test below asks a question the samples CAN answer, and box size never enters it:
// **an island is a dry patch that the grid shows completely surrounded by water.** Flood-fill
// the dry cells, discard any component touching the edge of the box (that is the shore, or
// ground outside the lake), and whatever is left is land with water all the way round it.
// A wider box needs a finer grid, not a rejection.

/// Samples per axis. Raised from 24 because the grid now has to resolve a shape, not a fraction.
const GRID_MIN = 32;
const GRID_MAX = 96;

/// How high above a lake's surface ground has to stand before it is an island rather than a shoal.
const ISLAND_FREEBOARD_M = 1.0;

function span(body) {
  return {
    lat: body.maxLatitudeDeg - body.minLatitudeDeg,
    lon: body.maxLongitudeDeg - body.minLongitudeDeg,
  };
}

/// Search one lake for ground standing above its own surface.
///
/// Returns `{ points, wet, dry }` - where the island samples are, and how the box divided, so a
/// caller can tell "a lake with an island" from "a box that mostly is not lake".
function islandsIn(body, elevationAt, grid) {
  // Sample the box once. `wet[i]` is true where the ground is at or below this body's surface.
  const wet = new Array(grid * grid).fill(false);
  const height = new Array(grid * grid).fill(Number.NaN);
  const latAt = (iy) => body.minLatitudeDeg
    + (body.maxLatitudeDeg - body.minLatitudeDeg) * (iy + 0.5) / grid;
  const lonAt = (ix) => body.minLongitudeDeg
    + (body.maxLongitudeDeg - body.minLongitudeDeg) * (ix + 0.5) / grid;

  let wetCells = 0;
  for (let iy = 0; iy < grid; iy += 1) {
    for (let ix = 0; ix < grid; ix += 1) {
      const ground = elevationAt(latAt(iy), lonAt(ix));
      const index = iy * grid + ix;
      height[index] = ground;
      if (Number.isFinite(ground) && ground <= body.levelM + ISLAND_FREEBOARD_M) {
        wet[index] = true;
        wetCells += 1;
      }
    }
  }

  // Flood-fill the DRY cells. A component that never touches the border has water all the way
  // round it inside this box, which is the definition being tested.
  const seen = new Array(grid * grid).fill(false);
  const islands = [];
  for (let start = 0; start < grid * grid; start += 1) {
    if (wet[start] || seen[start]) continue;
    const stack = [start];
    seen[start] = true;
    const cells = [];
    let touchesEdge = false;
    while (stack.length) {
      const index = stack.pop();
      const ix = index % grid;
      const iy = (index - ix) / grid;
      if (ix === 0 || iy === 0 || ix === grid - 1 || iy === grid - 1) touchesEdge = true;
      cells.push(index);
      const neighbours = [
        ix > 0 ? index - 1 : -1,
        ix < grid - 1 ? index + 1 : -1,
        iy > 0 ? index - grid : -1,
        iy < grid - 1 ? index + grid : -1,
      ];
      for (const next of neighbours) {
        if (next < 0 || seen[next] || wet[next]) continue;
        seen[next] = true;
        stack.push(next);
      }
    }
    if (touchesEdge) continue;
    // A single cell is as likely to be a sampling artefact as an island; two is a shape.
    if (cells.length < 2) continue;
    let best = cells[0];
    for (const index of cells) if (height[index] > height[best]) best = index;
    const ix = best % grid;
    const iy = (best - ix) / grid;
    islands.push({
      latitude: latAt(iy),
      longitude: lonAt(ix),
      heightAboveLakeM: height[best] - body.levelM,
      cells: cells.length,
    });
  }
  return { islands, wetCells, cells: grid * grid };
}

/// Find lakes that hold an island.
///
/// Args:
///   water: `window.__wb.water` - the manifest as the engine handed it over.
///   elevationAt: `(latitudeDeg, longitudeDeg) -> metres`.
///
/// Returns `{ hits, examined, skippedPointBox, skippedWideBox }`. The three counts are the
/// honest denominator: a search that examined nine of a hundred bodies has not searched a world.
export function lakesWithIslands(water, elevationAt) {
  const bodies = (water && water.bodies) || [];
  const hits = [];
  let examined = 0;
  let skippedPointBox = 0;
  let skippedWideBox = 0;

  for (const body of bodies) {
    const size = span(body);
    if (size.lat < MIN_SPAN_DEG || size.lon < MIN_SPAN_DEG) {
      skippedPointBox += 1;
      continue;
    }
    if (size.lat > MAX_SPAN_DEG || size.lon > MAX_SPAN_DEG) {
      skippedWideBox += 1;
      continue;
    }
    const spanKm = {
      lat: size.lat * 111.32,
      lon: Math.abs(size.lon * 111.32 * Math.cos((body.minLatitudeDeg * Math.PI) / 180)),
    };
    // A bigger box gets a finer grid rather than a rejection, so an island stays about the same
    // number of kilometres across whatever the hint's size. Capped, because cost is quadratic.
    const widest = Math.max(spanKm.lat, spanKm.lon);
    const grid = Math.max(GRID_MIN, Math.min(GRID_MAX, Math.round(widest / 3)));
    examined += 1;
    const inside = islandsIn(body, elevationAt, grid);
    if (inside.islands.length === 0) continue;
    // A box only a quarter under water is a flooded landscape, not a lake, and it shows: one
    // such box came back with seventy-seven separate enclosed dry patches. Seventy-seven
    // islands is not a lake with an island in it, it is a grid of puddles between hills.
    const lakeFraction = inside.wetCells / inside.cells;
    if (lakeFraction < 0.5) continue;
    inside.islands.sort((a, b) => b.cells - a.cells || b.heightAboveLakeM - a.heightAboveLakeM);
    const best = inside.islands[0];
    const cellKm = widest / grid;
    hits.push({
      body,
      latitude: best.latitude,
      longitude: best.longitude,
      heightAboveLakeM: best.heightAboveLakeM,
      lakeLevelM: body.levelM,
      islands: inside.islands.length,
      // How big the island is, in kilometres, from how many cells it covered.
      islandKm: Math.sqrt(best.cells) * cellKm,
      lakeFraction,
      gridKm: cellKm,
      spanKm,
    });
  }

  // Biggest island in the wettest lake first. Somebody who remembers an island in a lake
  // remembers a visible island AND an obvious lake; ranking on size alone puts a speck in a
  // marsh above a real one.
  hits.sort((a, b) => (b.islandKm * b.lakeFraction) - (a.islandKm * a.lakeFraction));
  return { hits, examined, skippedPointBox, skippedWideBox, total: bodies.length };
}

/// Is this lake itself on an island? Ring-sample outward for sea.
///
/// "An island in a lake on an island" needs the outer claim as well as the inner one, and the
/// outer one is the cheaper of the two: walk out from the lake until either sea or the search
/// radius arrives. Sea is ground below the datum that is not the lake.
export function onAnIsland(latitude, longitude, elevationAt, seaLevelM = 0,
                           radiusKm = 120, rings = 24, rays = 16) {
  let seaBearings = 0;
  for (let ray = 0; ray < rays; ray += 1) {
    const angle = (2 * Math.PI * ray) / rays;
    for (let ring = 1; ring <= rings; ring += 1) {
      const distanceDeg = (radiusKm * ring / rings) / 111.32;
      const sampleLat = latitude + distanceDeg * Math.sin(angle);
      const sampleLon = longitude
        + (distanceDeg * Math.cos(angle)) / Math.max(0.2, Math.cos((latitude * Math.PI) / 180));
      if (elevationAt(sampleLat, sampleLon) < seaLevelM) {
        seaBearings += 1;
        break;
      }
    }
  }
  return {
    seaBearings,
    rays,
    // Sea in every direction within the radius is an island. Sea in most of them is a peninsula
    // or a cape, which is worth telling apart rather than rounding up.
    isIsland: seaBearings === rays,
    fraction: seaBearings / rays,
  };
}

/// The whole question, in one call: islands in lakes, ranked, each marked island-or-not.
export function findLakeIslands(water, elevationAt, seaLevelM = 0) {
  const result = lakesWithIslands(water, elevationAt);
  for (const hit of result.hits) {
    hit.surroundings = onAnIsland(hit.latitude, hit.longitude, elevationAt, seaLevelM);
  }
  // An island in a lake on an island first, then the rest - but nothing is discarded, because
  // "on an island" is a judgement about a radius and the owner may mean a bigger one.
  result.hits.sort((a, b) => {
    if (a.surroundings.isIsland !== b.surroundings.isIsland) {
      return a.surroundings.isIsland ? -1 : 1;
    }
    return b.surroundings.fraction - a.surroundings.fraction;
  });
  return result;
}

/// A `?fly=` fragment for a hit, so finding a place and getting back to it are one step.
export function flyTo(hit, heightM = 30000) {
  return `fly=${hit.latitude.toFixed(5)},${hit.longitude.toFixed(5)},${Math.round(heightM)}`;
}
