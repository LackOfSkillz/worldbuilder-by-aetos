//! The water manifest, and the picture drawn from it.
//
// **Population, method and host, once, for every engine-backed figure below.**
//
//   - Population: the owner's world -- seed 562423712, radius 4,500,000 m, 28 plates, land 0.16,
//     the engine's `ranges` tectonic preset -- and `DEFAULT_WORLD` (seed 20260904, 6,371,000 m,
//     12 plates, land 0.29) where a second world is needed. The manifest is resolved at
//     `node_count = 30,000`, and **every body count in this file is quoted beside that number**,
//     because the population depends on it: 55 bodies on the owner's world at 30,000 and 351 at
//     100,000.
//   - Method: `wb_water_run` through `engine.waterRun`, i.e. the shipped export dumping the
//     shipped manifest. Rasters come from `relief.js::reliefTile` -- the same function the
//     workers call -- at `size = 64` over one body and `size = 128` over the eight tiles that
//     cover the sphere, with the heights re-fetched through the same
//     `marginedTileRequest` so a prediction is compared against the raster rather than against a
//     differently-sampled field.
//   - Host: node 22.17.0, this repository's checked-in
//     `viewer/public/wasm/worldbuilder_engine.wasm`, loaded from disk (no fetch).
//
// The pure functions in `water.js` are tested without the engine at all; only the four tests that
// say "the engine" need it.

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { Engine, WB_BODY_KIND, WB_MAX_WATER_NODES, WB_WATER_BODY_STRIDE } from "../public/app/engine.js";
import {
  DEFAULT_WATER_NODES, bodiesOverlappingRectangle, bodyContains, lakeLevelAt, longitudeSpanDeg,
  waterDiagnostics, waterEnabled, waterNodeCountFromParams,
} from "../public/app/water.js";
import {
  AMBIENT, DEFAULT_SUN, OCEAN_BANDS, coastDitherM, marginedTileRequest, reliefTile, shadeTint,
} from "../public/app/relief.js";
import { PANEL_RANGES, panelFieldFaults } from "../public/app/panel-fields.js";

/// One app source file, as text. The same device `coast-params.test.mjs` uses to hold `main.js`
/// and `controls.js` to their wiring: there is no DOM and no Cesium here, so the wiring between
/// the boot path and the provider is asserted on the source rather than by running it.
function appFile(name) {
  return readFileSync(fileURLToPath(new URL(`../public/app/${name}`, import.meta.url)), "utf8");
}

const OWNER_WORLD = { seed: 562423712, radiusM: 4500000, plateCount: 28, landFraction: 0.16 };
const NODES = 30000;

async function loadEngine() {
  const wasmPath = fileURLToPath(new URL("../public/wasm/worldbuilder_engine.wasm", import.meta.url));
  const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath), {});
  return new Engine(instance);
}

let engine;
let ownerHandle;
let manifest;

test.before(async () => {
  engine = await loadEngine();
  ownerHandle = engine.newWorld({ ...OWNER_WORLD, tectonics: engine.tectonicPreset("ranges") });
  // ONE resolution for the whole file: it costs ~4.2 s, and resolving it per test would be
  // paying that five times for an answer that cannot differ.
  manifest = engine.waterRun({ handle: ownerHandle, nodeCount: NODES });
});

/// The body with the largest bounding box that is actually drawable, by area on the sphere. This
/// is the world's largest inland sea and the subject of the close view in the report.
function largestDrawable() {
  const R = OWNER_WORLD.radiusM;
  let best = null;
  for (const body of manifest.bodies) {
    const span = longitudeSpanDeg(body);
    if (span === 0 || body.minLatitudeDeg === body.maxLatitudeDeg) continue;
    const area = 2 * Math.PI * R * R
      * (Math.sin((body.maxLatitudeDeg * Math.PI) / 180) - Math.sin((body.minLatitudeDeg * Math.PI) / 180))
      * (span / 360);
    if (best === null || area > best.area) best = { body, area };
  }
  return best;
}

/// A rectangle framing one body with a margin, and the raster of it.
function tileOver(body, { lakes, size = 64, marginDeg = 0.25 } = {}) {
  const rectangle = {
    northDeg: body.maxLatitudeDeg + marginDeg,
    southDeg: body.minLatitudeDeg - marginDeg,
    westDeg: body.minLongitudeDeg - marginDeg,
    eastDeg: body.maxLongitudeDeg + marginDeg,
  };
  const counters = { lakeTexels: 0, lakeTiles: 0 };
  const imageData = reliefTile({
    rectangle, size, engine, worldHandle: ownerHandle, radiusM: OWNER_WORLD.radiusM, lakes,
    counters,
  });
  return { rectangle, imageData, counters, size };
}

/// The heights `reliefTile` itself sampled, re-fetched through the same request builder, plus the
/// lat/lon of every output texel. This is what a prediction is built from: not a second sampling
/// of the field at different coordinates, which would disagree in the last bits and turn an exact
/// assertion into a tolerance.
function texelGrid(rectangle, size) {
  const request = marginedTileRequest({
    rectangle, size, worldHandle: ownerHandle, radiusM: OWNER_WORLD.radiusM,
  });
  const heights = engine.fillTileF32(request);
  const { grid, dLatStep, dLonStep } = request;
  const out = [];
  for (let row = 0; row < size; row += 1) {
    for (let col = 0; col < size; col += 1) {
      out.push({
        row,
        col,
        latitudeDeg: rectangle.northDeg + dLatStep * row,
        longitudeDeg: rectangle.westDeg + dLonStep * col,
        heightM: heights[(row + 1) * grid + (col + 1)],
      });
    }
  }
  return out;
}

/// **A second, independent implementation of the drawing rule**, written here on purpose.
///
/// Three tests in slice 5b and two in this viewer have turned red under a mutation for a reason
/// other than the one their name claimed, because the checker and the checked shared a function.
/// `coveringLevel` and `bandLookup` therefore do NOT call `water.js` or `slopeColor`: the arc test
/// is written the other way round (an explicit branch on the seam rather than a positive modulo)
/// and the palette lookup is eight lines of interpolation over the imported table.
function coveringLevel(bodies, { latitudeDeg, longitudeDeg, heightM }) {
  if (!(heightM > 0)) return null;
  let best = null;
  for (const b of bodies) {
    if (heightM > b.levelM) continue;
    if (latitudeDeg < b.minLatitudeDeg || latitudeDeg > b.maxLatitudeDeg) continue;
    const inside = b.minLongitudeDeg <= b.maxLongitudeDeg
      ? longitudeDeg >= b.minLongitudeDeg && longitudeDeg <= b.maxLongitudeDeg
      : longitudeDeg >= b.minLongitudeDeg || longitudeDeg <= b.maxLongitudeDeg;
    if (!inside) continue;
    if (best === null || b.levelM < best) best = b.levelM;
  }
  return best;
}

function bandLookup(bands, x) {
  if (x <= bands[0][0]) return bands[0][1];
  const last = bands[bands.length - 1];
  if (x >= last[0]) return last[1];
  for (let i = 1; i < bands.length; i += 1) {
    if (x <= bands[i][0]) {
      const t = (x - bands[i - 1][0]) / (bands[i][0] - bands[i - 1][0]);
      return [0, 1, 2].map((c) => bands[i - 1][1][c] + (bands[i][1][c] - bands[i - 1][1][c]) * t);
    }
  }
  return last[1];
}

// ---------------------------------------------------------------- the pure geometry

test("a body's box is an interval in latitude and an ARC in longitude", () => {
  const plain = {
    levelM: 100, minLatitudeDeg: -5, maxLatitudeDeg: 5, minLongitudeDeg: 10, maxLongitudeDeg: 20,
  };
  assert.equal(longitudeSpanDeg(plain), 10);
  assert.ok(bodyContains(plain, 0, 15));
  assert.ok(bodyContains(plain, -5, 10), "the box is inclusive at its corners");
  assert.ok(!bodyContains(plain, 0, 21));
  assert.ok(!bodyContains(plain, 6, 15));

  // **The seam, and it is the ONE place `minLongitudeDeg > maxLongitudeDeg` means something.**
  // `Extent` normalises to the smallest enclosing arc and expresses a seam-crossing arc by leaving
  // the pair out of order; a containment test that compared them as a plain interval would refuse
  // every point in such a body and draw nothing at all.
  const wrapped = {
    levelM: 100, minLatitudeDeg: -5, maxLatitudeDeg: 5, minLongitudeDeg: 170, maxLongitudeDeg: -170,
  };
  assert.equal(longitudeSpanDeg(wrapped), 20);
  assert.ok(bodyContains(wrapped, 0, 175));
  assert.ok(bodyContains(wrapped, 0, -175));
  assert.ok(bodyContains(wrapped, 0, 180));
  assert.ok(!bodyContains(wrapped, 0, 0), "a wrapped arc is 20 degrees wide, not 340");
  assert.ok(!bodyContains(wrapped, 0, 169));
});

test("the sea is never a lake texel, whatever the manifest says", () => {
  // Slice 5b's Ruling 6: the sea is not a body, the datum is carried once in `sea_level_m`, and a
  // texel at or below the datum belongs to the ocean's own colour path. This is the assertion that
  // makes "the ocean picture cannot move" a property rather than an intention -- a body whose box
  // covers the whole planet at a level far above the datum still claims nothing below it.
  const everywhere = [{
    levelM: 5000, minLatitudeDeg: -90, maxLatitudeDeg: 90,
    minLongitudeDeg: -180, maxLongitudeDeg: 180,
  }];
  assert.equal(lakeLevelAt(everywhere, 0, 0, -1), null);
  assert.equal(lakeLevelAt(everywhere, 0, 0, 0), null, "the datum itself is the sea's");
  assert.equal(lakeLevelAt(everywhere, 0, 0, 1e-9), 5000);
  // Ground above the surface is shore, not lake.
  assert.equal(lakeLevelAt(everywhere, 0, 0, 5000), 5000, "the surface itself is water");
  assert.equal(lakeLevelAt(everywhere, 0, 0, 5000.5), null);
});

test("where two boxes claim one point the SHALLOWEST body wins", () => {
  // The manifest carries no disambiguation rule and slice 5b met the same ambiguity from the other
  // side -- it is why ocean bodies were removed, 96.3% of their boxes overlapping another. The
  // lowest level is the conservative direction: least water drawn.
  const box = { minLatitudeDeg: -1, maxLatitudeDeg: 1, minLongitudeDeg: -1, maxLongitudeDeg: 1 };
  const two = [{ ...box, levelM: 900 }, { ...box, levelM: 400 }];
  assert.equal(lakeLevelAt(two, 0, 0, 300), 400, "the deeper body must not win the texel");
  assert.equal(lakeLevelAt([...two].reverse(), 0, 0, 300), 400, "order must not decide it");
  // And a point only one of them can hold still gets that one's level.
  assert.equal(lakeLevelAt(two, 0, 0, 600), 900);
});

test("the per-tile prefilter never drops a body the per-texel test would have hit", () => {
  // **The prefilter is an optimisation and an optimisation that changes the answer is a bug.** It
  // is checked by exhaustion against the unfiltered result rather than by inspection: 4,000
  // pseudo-random bodies and points, including seam-crossing arcs and polar boxes, with the two
  // paths required to agree exactly.
  let seed = 12345;
  const rand = () => {
    seed = (Math.imul(seed, 1103515245) + 12345) & 0x7fffffff;
    return seed / 0x7fffffff;
  };
  const rectangle = { northDeg: 20, southDeg: 10, westDeg: 170, eastDeg: 180 };
  let hits = 0;
  for (let i = 0; i < 4000; i += 1) {
    const minLat = rand() * 180 - 90;
    const minLon = rand() * 360 - 180;
    let maxLon = minLon + rand() * 90;
    if (maxLon > 180) maxLon -= 360; // a seam-crossing arc, expressed the way `Extent` expresses it
    const body = {
      levelM: 1000,
      minLatitudeDeg: minLat,
      maxLatitudeDeg: minLat + rand() * 30,
      minLongitudeDeg: minLon,
      maxLongitudeDeg: maxLon,
    };
    const bodies = [body];
    const filtered = bodiesOverlappingRectangle(bodies, rectangle);
    for (let k = 0; k < 4; k += 1) {
      const lat = rectangle.southDeg + rand() * (rectangle.northDeg - rectangle.southDeg);
      const lon = rectangle.westDeg + rand() * (rectangle.eastDeg - rectangle.westDeg);
      const withAll = lakeLevelAt(bodies, lat, lon, 100);
      const withFiltered = lakeLevelAt(filtered, lat, lon, 100);
      assert.equal(withFiltered, withAll, `prefilter disagreed at ${lat},${lon}`);
      if (withAll !== null) hits += 1;
    }
  }
  // ...and the sweep must actually have exercised the positive case, or agreeing on `null`
  // everywhere would pass a prefilter that returned nothing at all.
  assert.ok(hits > 100, `the sweep found only ${hits} covered points; it proves nothing`);
});

test("?lakes=0 is the off switch, and ?lakeNodes= is the only other knob", () => {
  assert.equal(waterEnabled(new URLSearchParams("")), true);
  assert.equal(waterEnabled(new URLSearchParams("lakes=0")), false);
  assert.equal(waterEnabled(new URLSearchParams("lakes=1")), true);
  assert.equal(waterNodeCountFromParams(new URLSearchParams("")), DEFAULT_WATER_NODES);
  assert.equal(waterNodeCountFromParams(new URLSearchParams("lakeNodes=8000")), 8000);
  // The panel's slider and the boot path must be talking about the same knob, and the slider must
  // be able to express its own default -- the fault four shipped sliders have had.
  const field = PANEL_RANGES.find((f) => f.query === "lakeNodes");
  assert.ok(field, "the panel has no lakeNodes slider");
  assert.equal(field.value, DEFAULT_WATER_NODES, "the slider's default is a second copy");
  assert.equal(field.max, WB_MAX_WATER_NODES, "the slider's ceiling is not the engine's");
  assert.deepEqual(panelFieldFaults([field]), []);
});

// ---------------------------------------------------------------- the engine and the picture

test("the engine's manifest is lakes, at levels above the datum, and no ponds", () => {
  assert.equal(WB_WATER_BODY_STRIDE, 7);
  assert.equal(manifest.seaLevelM, 0, "the datum the engine echoed back is not the one asked for");
  assert.ok(manifest.bodies.length > 0);
  for (const body of manifest.bodies) {
    // **Every body is a lake.** Not because this file filtered ponds out: the engine's calibrated
    // threshold is 1.0e5 m^2 and the smallest body this mesh makes is 7.9e8 m^2, so `Pond` is
    // unreachable. Asserted so that a generator change fine enough to produce one is seen here
    // rather than in a screenshot.
    assert.equal(body.kind, WB_BODY_KIND.lake);
    // A lake's surface is above the datum, which is what lets the ocean rule in `lakeLevelAt` be
    // a rule about texels rather than a rule about levels.
    assert.ok(body.levelM > 0, `a body at level ${body.levelM} is at or below the datum`);
    assert.ok(body.maxLatitudeDeg >= body.minLatitudeDeg);
  }
  // Rows arrive ascending by `rootNode`, which the export documents as the contract and which is
  // what lets a check pick a body BY ITS ID and compare two runs row by row.
  for (let i = 1; i < manifest.bodies.length; i += 1) {
    assert.ok(manifest.bodies[i].rootNode > manifest.bodies[i - 1].rootNode);
  }
});

test("what the manifest cannot say, counted rather than left to be rediscovered", () => {
  const facts = waterDiagnostics(manifest.bodies);
  assert.equal(facts.bodies, manifest.bodies.length);
  // **A single-node body's extent is a POINT, and a point cannot be drawn.** This is the finding
  // this task reports to the engine side: there is no footprint, no radius, and `rootNode` cannot
  // be turned into a position by any export, so such a body is water the viewer knows about and
  // cannot render. It is a large fraction of the manifest, not a corner case.
  assert.ok(facts.pointBoxes > 0, "no point boxes: the finding this asserts has gone away");
  assert.equal(facts.drawable, facts.bodies - facts.degenerateBoxes);
  assert.ok(facts.drawable > 0 && facts.drawable < facts.bodies);
  // On this world at this node count the wide-box case does not occur at all -- the slice's
  // ledger warns of 356-360 degree antimeridian boxes, and re-measured on the shipped export the
  // widest arc here is under five degrees. Pinned so that if it ever does occur, it is seen.
  assert.equal(facts.wideBoxes, 0);
  for (const body of manifest.bodies) assert.ok(longitudeSpanDeg(body) < 180);
});

test("a lake is drawn exactly where the engine says, at the level the engine says", () => {
  // **The assertion the task turns on**, and it is written against ONE body picked by its id --
  // not against "there is blue in roughly the right place".
  const { body } = largestDrawable();
  const { rectangle, imageData, counters, size } = tileOver(body, { lakes: manifest.bodies });
  const grid = texelGrid(rectangle, size);
  const nearby = bodiesOverlappingRectangle(manifest.bodies, rectangle);

  // The prediction is built from the MANIFEST and the HEIGHTS, independently of what the raster
  // did: a texel is lake exactly when it is above the datum, inside some nearby body's box, and at
  // or below that body's level.
  let predicted = 0;
  let fromThisBody = 0;
  for (const texel of grid) {
    const level = coveringLevel(nearby, texel);
    if (level === null) continue;
    predicted += 1;
    if (level === body.levelM) fromThisBody += 1;

    // ...and the colour is the OCEAN table read at the depth below THAT level, lit as a flat
    // plane. **Derived here, not asked of the module under test.** `slopeColor` is what draws
    // this, so predicting with it would make the two sides move together under any mutation of
    // the rule -- the shadowing this project has now found three times. The palette and the
    // dither are imported because they are DATA and a hash; the lookup, the depth and the flat
    // shade are re-derived.
    const [r, g, b] = bandLookup(OCEAN_BANDS,
      (texel.heightM - level) + coastDitherM(texel.latitudeDeg, texel.longitudeDeg));
    const shade = AMBIENT + (1 - AMBIENT) * DEFAULT_SUN.up;
    const [tr, tg, tb] = shadeTint(shade);
    const idx = (texel.row * size + texel.col) * 4;
    for (const [channel, value] of [[0, r * shade * tr], [1, g * shade * tg], [2, b * shade * tb]]) {
      const drawn = imageData.data[idx + channel];
      const want = value > 255 ? 255 : value < 0 ? 0 : value;
      assert.ok(
        Math.abs(drawn - want) <= 1,
        `texel ${texel.row},${texel.col} channel ${channel}: drew ${drawn}, the body at level ${
          level} m says ${want.toFixed(2)}`,
      );
    }
  }
  assert.ok(fromThisBody > 200, `the chosen body covers only ${fromThisBody} texels`);
  // **The counter, not the picture.** `reliefTile` counted these while it drew; the prediction
  // counted them from the manifest. A provider that ignored the manifest would draw plausible land
  // here and report zero.
  assert.equal(counters.lakeTexels, predicted);
  assert.equal(counters.lakeTiles, 1);

  // **The level is load-bearing, and here is the proof.** Moving this body's surface 100 m down
  // must shrink its footprint; a viewer that drew "the box" rather than "the box below the level"
  // would not notice.
  const lowered = manifest.bodies.map((b) => (b.rootNode === body.rootNode
    ? { ...b, levelM: b.levelM - 100 } : b));
  const shrunk = tileOver(body, { lakes: lowered });
  assert.ok(
    shrunk.counters.lakeTexels < counters.lakeTexels,
    "lowering the surface by 100 m drew the same number of texels; the level is not being read",
  );
});

test("a body whose extent is a point draws nothing, and the counter says so", () => {
  // The other half of the degenerate-extent finding, as a fact about the picture rather than about
  // the manifest: framing a point-box body and rasterising it paints no water at all.
  const point = manifest.bodies.find(
    (b) => longitudeSpanDeg(b) === 0 && b.minLatitudeDeg === b.maxLatitudeDeg,
  );
  assert.ok(point, "this world has no point-box body; the finding this asserts has gone away");
  const { counters } = tileOver(point, { lakes: [point], marginDeg: 0.5 });
  assert.equal(counters.lakeTexels, 0, "a zero-measure extent painted a texel");
  assert.equal(counters.lakeTiles, 1, "the body was not even considered for this tile");
});

test("the ocean is byte-identical with the lakes on and off", () => {
  // **No ocean change** is a constraint of this task, and this is the falsifiable form of it: over
  // a tile carrying coast, sea and a lake, every texel the engine calls water (height <= 0) has
  // the same four bytes in both rasters. Not "looks the same" -- the same bytes.
  // **Over the WHOLE PLANET, not over one frame.** Eight 90x90-degree tiles tile the sphere, so
  // this is a claim about every body in the manifest at once rather than about the one that
  // happens to be in shot -- and it cannot be satisfied by a frame that contains no sea.
  // 128 texels over 90 degrees is 0.70 degrees per texel -- coarse, and deliberately so: this
  // test is about the whole sphere, not about detail. At that spacing the manifest's lakes cover
  // roughly 120 texels of the planet's 32,768, which is the honest scale of inland water here.
  const size = 128;
  const counters = { lakeTexels: 0, lakeTiles: 0 };
  let sea = 0;
  let moved = 0;
  for (const westDeg of [-180, -90, 0, 90]) {
    for (const [northDeg, southDeg] of [[90, 0], [0, -90]]) {
      const rectangle = { northDeg, southDeg, westDeg, eastDeg: westDeg + 90 };
      const common = {
        rectangle, size, engine, worldHandle: ownerHandle, radiusM: OWNER_WORLD.radiusM,
      };
      const off = reliefTile({ ...common, lakes: [] });
      const on = reliefTile({ ...common, lakes: manifest.bodies, counters });
      for (const texel of texelGrid(rectangle, size)) {
        const idx = (texel.row * size + texel.col) * 4;
        const same = [0, 1, 2, 3].every((c) => off.data[idx + c] === on.data[idx + c]);
        if (texel.heightM <= 0) {
          sea += 1;
          assert.ok(same, `an ocean texel at ${texel.latitudeDeg},${texel.longitudeDeg} moved`);
        } else if (!same) {
          moved += 1;
        }
      }
    }
  }
  assert.ok(sea > 80000, `only ${sea} ocean texels over the whole planet; this proves little`);
  // ...and land DID move, or the two rasters would be identical for the boring reason that the
  // manifest never arrived. **This is the counter-argument in pixels**, and it is the half a
  // byte-identity test cannot supply on its own.
  assert.ok(moved > 60, `only ${moved} land texels changed; the lakes are not being drawn`);
  // Every texel that moved is a texel the counter counted. Not merely "both are positive": the
  // two numbers are arrived at by different means -- one by comparing rasters, one by counting
  // inside the loop that drew them -- and they must agree exactly.
  assert.equal(counters.lakeTexels, moved);
});

test("the boot path resolves the manifest before the provider exists, and can be turned off", () => {
  // **The wiring, asserted on the source**, the same way `coast-params.test.mjs` holds `main.js`
  // to the coast channel: there is no DOM and no Cesium here, so what can be checked is that the
  // boot path calls the export, hands the result to the layer that draws it, and skips both under
  // the off switch.
  const main = appFile("main.js");
  assert.match(main, /engine\.waterRun\(\{ handle: world, nodeCount: waterNodes \}\)/);
  assert.match(main, /lakes: water\.bodies/, "the manifest never reaches the relief provider");
  // **Off means NOT RESOLVED, not resolved-and-ignored.** Four seconds of boot spent on an answer
  // that is then discarded is the shape this line refuses; the ternary is what makes `?lakes=0`
  // restore the previous boot cost as well as the previous picture.
  assert.match(main, /lakesOn\s*\?\s*\n?\s*engine\.waterRun/);
  assert.match(main, /\{ seaLevelM: null, bodies: \[\] \}/);
  // And it must be resolved BEFORE the provider is constructed: a manifest that arrived later
  // would leave Cesium holding cached lake-free textures for whatever the camera saw first.
  assert.ok(
    main.indexOf("engine.waterRun(") < main.indexOf("createReliefImageryProvider("),
    "the manifest is resolved after the provider is built; early tiles would cache without lakes",
  );
});
