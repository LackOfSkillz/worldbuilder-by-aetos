// Node-native tests for the viewer's tectonic channel: `tectonic-params.js`, `engine.js`'s
// three new marshalling methods, and the properties that are easy to claim and easy to get
// wrong -- that the untouched path is still the untouched world, that no tectonic number is
// written down twice, and that every position the three mountain sliders can take is a block
// the shipped artifact accepts.
//
// No framework: `node:test` + `node:assert/strict`, run with `npm test` from `viewer/`.
// Nothing here touches Cesium or the DOM.
//
// Population/method/host, named once so every number below can be traced:
//   - World: Surface::new(20260904, 6_371_000, 12, 0.29, ..) -- DEFAULT_WORLD, the fixture
//     whose elevation at lat 12 lon 34 was witnessed three independent ways.
//   - Host: node, this repository's checked-in
//     viewer/public/wasm/worldbuilder_engine.wasm, loaded directly from disk (no fetch).
//   - Probes: the nine lat/lon in PROBES below -- the relief channel's six, which cross deep
//     water, shelf, shoreline and land, PLUS three witness points measured for this channel.
//     The six alone are BLIND to the collision profile on this world: driving it from
//     1,500 m / 400 km to 6,000 m / 100 km changes not one bit at any of them, because none
//     is within MAX_TECTONIC_RANGE_M of a convergent continental margin. Found by the
//     engine-side test failing. See `TECTONIC_PROBES` in `tests/wasm_exports.rs`.
//   - Slider population: every integer position each of the SIX sliders can take --
//     46 + 31 + 19 + 9 + 10 + 5 = 120 -- each turned into a tectonic block and put through
//     wb_tectonic_check. Three of the six are Task 3's: the structure field is on the channel
//     now, and the whole point of this task is that the owner can reach it.

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { Engine, WB_OK, WB_ERR_PARAM } from "../public/app/engine.js";
import { panelFieldFaults } from "../public/app/panel-fields.js";
import {
  TECTONIC_CONTROLS,
  TECTONIC_SLIDERS,
  TECTONIC_FIELDS,
  TECTONIC_PARAM_NAMES,
  MEASURED_GRADES,
  HEIGHT_STEPS,
  HEIGHT_STEP_M,
  WIDTH_STEPS,
  WIDTH_STEP_M,
  BLEND_NINTHS,
  BLEND_POSITIONS_FEWER,
  BLEND_POSITIONS_MORE,
  ASYMMETRY_STEPS,
  DEPTH_STEPS,
  WAVELENGTH_STEPS,
  WAVELENGTH_STEP_M,
  tectonicTravel,
  tectonicPanelFields,
  tectonicFromParams,
  tectonicToParams,
} from "../public/app/tectonic-params.js";

const DEFAULT_WORLD = { seed: 20260904, radiusM: 6371000, plateCount: 12, landFraction: 0.29 };

const PROBES = [
  [12.0, 34.0],
  [0.0, 0.0],
  [-18.25, 121.5],
  [62.5, -145.0],
  [-71.0, 25.0],
  [35.0, 138.0],
  [-7.5, 66.0],
  [-3.0, 69.0],
  [-33.5, -22.0],
];

const appFile = (name) =>
  readFileSync(fileURLToPath(new URL(`../public/app/${name}`, import.meta.url)), "utf8");

async function loadEngine() {
  const wasmPath = fileURLToPath(new URL("../public/wasm/worldbuilder_engine.wasm", import.meta.url));
  const bytes = readFileSync(wasmPath);
  const { instance } = await WebAssembly.instantiate(bytes, {});
  return new Engine(instance);
}

let engine;
let canonical;
let ranges;
let travel;

test.before(async () => {
  engine = await loadEngine();
  canonical = engine.tectonicPreset("canonical");
  ranges = engine.tectonicPreset("ranges");
  travel = tectonicTravel(canonical);
});

test("the canonical block comes across the boundary whole, in field order", () => {
  for (const field of TECTONIC_FIELDS) {
    assert.equal(typeof canonical[field], "number", `canonical.${field}`);
    assert.ok(Number.isFinite(canonical[field]));
  }
  // Ordering, not values: if the nine f64 were read in the wrong order the amplitudes and the
  // widths would swap places and every one of them would still be "a number". The record's
  // own shape catches that -- the narrowest width is 73x the tallest amplitude, and the blend
  // is smaller than all of them.
  const widths = [
    canonical.continentCollisionWidthM, canonical.coastalUpliftWidthM,
    canonical.islandArcWidthM, canonical.ridgeWidthM,
  ];
  const amplitudes = [
    canonical.continentCollisionM, canonical.coastalUpliftM,
    canonical.islandArcM, canonical.ridgeM,
  ];
  // 73x at the narrowest, so 10x is a wide margin and still catches a swapped pair outright.
  assert.ok(Math.min(...widths) > Math.max(...amplitudes) * 10, "widths and amplitudes swapped");
  assert.ok(canonical.continentalBlend > 0 && canonical.continentalBlend < 1);
  // And the one field with a shape all its own: the collision profile is the tallest and the
  // widest, which is what makes it the mountain knob.
  assert.equal(canonical.continentCollisionM, Math.max(...amplitudes));
  assert.equal(canonical.continentCollisionWidthM, Math.max(...widths));
});

test("no tectonic number is written down twice in the viewer", () => {
  // The hazard the whole design is arranged against, stated as a test rather than as a
  // comment -- the same test the relief channel carries, over this channel's three anchors.
  // Comments are stripped first: prose is allowed to quote a measurement, code is not allowed
  // to restate one.
  const strip = (source) => source
    .split("\n")
    .map((line) => line.replace(/^\s*\/\/.*$/, "").replace(/^\s*\/\/\/.*$/, ""))
    .join("\n");
  // **Only the DISTINCTIVE values can be asked this question, and saying so is part of the
  // test.** Four of the fourteen canonical fields are 0, 0.0 or 1 -- the inert settings that
  // make the structure fields arithmetic identities on the canonical path -- and a source file
  // that never contains the character "1" or "0" is not a file. Those four are checked by the
  // *function-source* half below instead, which asks the sharper question anyway: not "does
  // this digit appear" but "is any tectonic value computed from a literal rather than from the
  // engine's block".
  const literals = ["continentCollisionM", "continentCollisionWidthM", "continentalBlend",
    "structureWavelengthM"].map((field) => String(canonical[field]));
  assert.deepEqual(literals, ["1500", "400000", "0.45", "120000"]);
  // And the PRESET's own values, which is Ruling 7 of the relief slice applied to this one:
  // the preset crosses as fields, so no file in `viewer/` may contain the numbers it chose.
  // If it did, the panel would be showing a copy rather than the engine's answer, and the copy
  // is what drifts.
  const presetLiterals = ["continentCollisionM", "continentCollisionWidthM", "structureDepth",
    "structureWavelengthM", "marginWarpWavelengthM"].map((field) => String(ranges[field]));
  // `marginWarpM` is deliberately NOT in this list and `marginWarpWavelengthM` is. The
  // wander's amplitude is 80000, which is `structureWavelengthM`'s value too -- already
  // checked, on the line above -- so adding it would be a second assertion about one string.
  // The wavelength, 300000, is its own number and nothing else in this channel is it.
  assert.deepEqual(presetLiterals, ["6000", "100000", "0.7", "80000", "300000"]);
  literals.push(...presetLiterals);
  for (const name of ["controls.js", "main.js"]) {
    const code = strip(appFile(name));
    for (const literal of literals) {
      assert.ok(
        !code.includes(literal),
        `${name} restates the canonical tectonic value ${literal}; it must read it from the engine`,
      );
    }
  }
  // `tectonic-params.js` is asked a SHARPER question rather than the same one, because it
  // legitimately contains "1500" and "400": `MEASURED_GRADES` is the probe's own table and
  // every row names the setting it was measured at, canonical included. A table that could
  // not say "1,500 m over 400 km is a 1.787% grade" would be a table about nothing.
  //
  // What must hold is that no value is *computed* from a literal. So the assertion is over
  // the source of the functions that produce tectonic values, taken from the live functions
  // rather than by parsing the file -- there is no way for that to pass by matching nothing.
  for (const fn of [tectonicTravel, tectonicPanelFields, tectonicFromParams, tectonicToParams]) {
    // Comments stripped here too, and for the same reason they are stripped from the files:
    // `tectonicTravel`'s own comment has to be able to say that `0.1 * 7` is
    // 0.7000000000000001, because that is WHY the map divides rather than multiplies, and a
    // check that forbade the explanation would push the reasoning out of the file it belongs
    // in. `String(fn)` includes comments; this asks about the code.
    const source = strip(String(fn));
    for (const literal of literals) {
      assert.ok(
        !source.includes(literal),
        `${fn.name} computes with the canonical literal ${literal} instead of its argument`,
      );
    }
    // And each one actually takes canonical as an argument, or the check above would pass on
    // a function that had no anchor at all.
    assert.match(source, /canonical/, `${fn.name} must be anchored on the engine's canonical`);
  }
  // And the panel must reach the engine for them, or the assertion above would pass on a file
  // that simply has no mountains section.
  assert.match(appFile("controls.js"), /from "\.\/tectonic-params\.js"/);
  assert.match(appFile("main.js"), /engine\.tectonicPreset\("canonical"\)/);
  assert.match(appFile("main.js"), /engine\.tectonicPreset\("ranges"\)/);
  assert.match(appFile("engine.js"), /wb_tectonic_preset/);
});

test("the two NOT_WIRED entries came off the panel, and came off together", () => {
  // They said the same thing -- that a mountain here is tectonic and no relief knob reaches
  // it -- so they come off together or neither does. Comments are NOT stripped for the second
  // half of this: the reason they came off is kept in the file, and that is deliberate.
  const controls = appFile("controls.js");
  const entries = controls.slice(controls.indexOf("const NOT_WIRED = ["));
  const list = entries.slice(0, entries.indexOf("];"));
  assert.ok(!list.includes('["mountain height"'), "the mountain height entry is still listed");
  assert.ok(!list.includes('["mountain count"'), "the mountain count entry is still listed");
  // The island arcs stay listed, with the reason: Task 1 proved the other seven fields are
  // read by one-ULP perturbation and recorded that these two have no coverage.
  assert.ok(list.includes('["island arcs"'), "the uncovered arc fields must still be declared");
});

test("every mountain slider can express its own default", () => {
  // `panelFieldFaults` over the travel in TECTONIC units, which is the question the check
  // exists to ask: an `<input type="range">` snaps to `min + n * step`, and a default off that
  // lattice is silently replaced. Four instances of that defect have shipped in this viewer
  // and the fourth was found by this check rather than by a person.
  //
  // The widget itself carries integer positions, so this ought to be impossible by
  // construction -- but "impossible by construction" is what was said about the radius slider
  // too, and `min`, `max` and `step` here are all derived from the engine's canonical through
  // f64 arithmetic.
  assert.deepEqual(panelFieldFaults(tectonicPanelFields(canonical)), []);
  // And it can still fail: the same table with a step that cannot express the default.
  const broken = tectonicPanelFields(canonical).map((f) => ({ ...f, step: 7 }));
  assert.ok(panelFieldFaults(broken).length > 0, "the check has stopped being able to fail");
});

test("position 0 is canonical exactly, on all six sliders", () => {
  // **This is the assertion that keeps RULING 1.** `tectonicToParams` drops a field that
  // EQUALS canonical, so a position-0 value one ULP off would be written into every shared
  // link and would take an untouched viewer off the engine's `None` path.
  //
  // `canonical * (n / 9)` is exact at n = 9 and `canonical * n / 9` is one ULP out -- the
  // engine-side test found that by failing, not by inspection. Asserted with `===`.
  for (const field of TECTONIC_SLIDERS) {
    assert.equal(travel[field].toValue(0), canonical[field], `${field} at position 0`);
    assert.equal(travel[field].toPosition(canonical[field]), 0, `${field} position of canonical`);
  }
});

test("the slider travel is the travel that was measured", () => {
  // The height slider: canonical up to 6,000 m, which is the top row of the probe's own
  // table, at 100 m a step.
  assert.equal(travel.continentCollisionM.min, 0);
  assert.equal(travel.continentCollisionM.max, HEIGHT_STEPS);
  assert.equal(travel.continentCollisionM.toValue(HEIGHT_STEPS), 6000);
  assert.equal(
    travel.continentCollisionM.toValue(HEIGHT_STEPS),
    canonical.continentCollisionM + HEIGHT_STEPS * HEIGHT_STEP_M,
  );

  // The steepness slider: canonical DOWN to 100 km, at 10 km a step. Down, because canonical
  // is already the widest a centred profile may be -- the range gate is 420 km.
  assert.equal(travel.continentCollisionWidthM.max, WIDTH_STEPS);
  assert.equal(travel.continentCollisionWidthM.toValue(WIDTH_STEPS), 100000);
  assert.equal(
    travel.continentCollisionWidthM.toValue(WIDTH_STEPS),
    canonical.continentCollisionWidthM - WIDTH_STEPS * WIDTH_STEP_M,
  );
  assert.ok(
    travel.continentCollisionWidthM.toValue(1) < travel.continentCollisionWidthM.toValue(0),
    "dragging the steepness slider right must narrow the profile",
  );

  // The count slider, negated so right is more: the blend runs the opposite way to its name.
  assert.equal(travel.continentalBlend.min, -BLEND_POSITIONS_FEWER);
  assert.equal(travel.continentalBlend.max, BLEND_POSITIONS_MORE);
  assert.ok(
    travel.continentalBlend.toValue(1) < travel.continentalBlend.toValue(0),
    "dragging the count slider right must NARROW the blend, which is more mountains",
  );
  // Both ends land on 1.00 and 0.10 to within an ULP and no closer, and that is stated rather
  // than rounded away. Only position 0 has to be exact.
  assert.ok(Math.abs(travel.continentalBlend.toValue(-BLEND_POSITIONS_FEWER) - 1.0) < 1e-15);
  assert.ok(Math.abs(travel.continentalBlend.toValue(BLEND_POSITIONS_MORE) - 0.1) < 1e-15);
  assert.equal(BLEND_NINTHS, 9);

  // Round trips: every position reads back as itself, on every slider.
  for (const field of TECTONIC_SLIDERS) {
    for (let position = travel[field].min; position <= travel[field].max; position += 1) {
      assert.equal(
        travel[field].toPosition(travel[field].toValue(position)), position,
        `${field} position ${position} does not round trip`,
      );
    }
  }
});

test("every position the sliders can take is a block the engine accepts", () => {
  // The JS-side half of the engine's own slider sweep. The Rust test asserts the same 96
  // values are admissible through the native boundary; this asserts the *widget* produces
  // exactly those values, through the committed artifact, so the panel cannot offer a
  // position that turns the viewer blank.
  let checked = 0;
  for (const field of TECTONIC_SLIDERS) {
    for (let position = travel[field].min; position <= travel[field].max; position += 1) {
      const tectonics = { ...canonical, [field]: travel[field].toValue(position) };
      assert.equal(
        engine.checkTectonic(tectonics), WB_OK,
        `the ${field} slider reaches ${travel[field].toValue(position)} and the engine refuses it`,
      );
      checked += 1;
    }
  }
  assert.equal(checked, 46 + 31 + 19 + 9 + 10 + 5);
});

test("the checker says WHY, and refuses what the panel cannot produce", () => {
  // The panel needs the difference between "refused" and "accepted": a refused world is a
  // blank viewer, and `wb_world_new_tectonic` answers a refusal with a handle of 0, which says
  // that it refused and never why.
  assert.equal(engine.checkTectonic(null), WB_OK);
  // A zero-width profile: NOT a division by zero -- `bump` returns 0.0 for it -- but a field
  // that is present, accepted and does exactly nothing, which is the silently-dropping shape.
  assert.equal(
    engine.checkTectonic({ ...canonical, continentCollisionWidthM: 0 }), WB_ERR_PARAM);
  // A blend of zero is the hard test the parameter exists to remove.
  assert.equal(engine.checkTectonic({ ...canonical, continentalBlend: 0 }), WB_ERR_PARAM);
  // A width past the range gate would be truncated rather than faded -- a cliff.
  assert.equal(
    engine.checkTectonic({ ...canonical, continentCollisionWidthM: 500000 }), WB_ERR_PARAM);
  // And the offset profiles have tighter ceilings than the centred one, because a bump centred
  // 70 km inboard still carries weight at `70 km + width`.
  assert.equal(engine.checkTectonic({ ...canonical, coastalUpliftWidthM: 400000 }), WB_ERR_PARAM);
  assert.equal(engine.checkTectonic({ ...canonical, coastalUpliftWidthM: 350000 }), WB_OK);
  // NaN from a `Number("")` or a bad parse.
  assert.equal(engine.checkTectonic({ ...canonical, continentCollisionM: NaN }), WB_ERR_PARAM);
  assert.equal(
    engine.checkTectonic({ ...canonical, continentCollisionM: Infinity }), WB_ERR_PARAM);
});

test("an untouched viewer is the untouched world", () => {
  // RULING 1, at the artifact rather than in the engine's own test binary.
  assert.equal(tectonicFromParams(new URLSearchParams(""), canonical), null);
  // A URL naming every tectonic parameter at its canonical value is still the canonical path.
  const atCanonical = new URLSearchParams(
    TECTONIC_CONTROLS.map((f) => [TECTONIC_PARAM_NAMES[f], String(canonical[f])]),
  );
  assert.equal(tectonicFromParams(atCanonical, canonical), null);
  // A parameter that is not a number is ignored rather than forwarded: a typo in a shared link
  // should not be a blank page.
  assert.equal(tectonicFromParams(new URLSearchParams("mtnHeight=banana"), canonical), null);

  const legacy = engine.exports.wb_world_new(
    BigInt(DEFAULT_WORLD.seed), DEFAULT_WORLD.radiusM, DEFAULT_WORLD.plateCount,
    DEFAULT_WORLD.landFraction, 0, 0,
  ) >>> 0;
  assert.notEqual(legacy, 0);
  const defaulted = engine.newWorld({ ...DEFAULT_WORLD });
  const spelled = engine.newWorld({ ...DEFAULT_WORLD, tectonics: canonical });
  try {
    for (const [lat, lon] of PROBES) {
      for (const resolution of [250, -1]) {
        const reference = engine.elevationM(legacy, lat, lon, resolution);
        assert.equal(engine.elevationM(defaulted, lat, lon, resolution), reference,
          `the null tectonic path moved the world at ${lat},${lon}`);
        assert.equal(engine.elevationM(spelled, lat, lon, resolution), reference,
          `the canonical block is not the null path at ${lat},${lon}`);
      }
    }
    // The witnessed value itself, unchanged by a slice that added a second parameter block.
    // `engine.newWorld` now calls `wb_world_new_tectonic` for EVERY world, so this is also the
    // assertion that the widest door with two null blocks is still the original constructor.
    assert.equal(engine.elevationM(defaulted, 12.0, 34.0, 250), 682.3921701573904);
  } finally {
    engine.freeWorld(legacy);
    engine.freeWorld(defaulted);
    engine.freeWorld(spelled);
  }
});

test("a chosen block reaches the ground, and it reaches it where it was measured", () => {
  const chosen = tectonicFromParams(
    new URLSearchParams("mtnHeight=6000&mtnWidth=100000"), canonical);
  assert.equal(chosen.continentCollisionM, 6000);
  assert.equal(chosen.continentCollisionWidthM, 100000);
  assert.equal(chosen.continentalBlend, canonical.continentalBlend, "untouched fields stay canonical");
  assert.equal(chosen.ridgeM, canonical.ridgeM);

  const plain = engine.newWorld({ ...DEFAULT_WORLD });
  const alps = engine.newWorld({ ...DEFAULT_WORLD, tectonics: chosen });
  try {
    // The witness point, and the measured size of the move. `mountain_probe.rs::witness_for`
    // scanned the same 0.5-degree global grid for the site of the largest change this block
    // makes anywhere on this world: -7.50, 66.00, from 1,886.798 m to 5,616.919 m -- a move of
    // 3,730.122 m. Asserted loosely (100 m) because the probe's figure is at canonical
    // resolution and this samples at 250 m, not because the number is soft.
    const before = engine.elevationM(plain, -7.5, 66.0, -1);
    const after = engine.elevationM(alps, -7.5, 66.0, -1);
    assert.ok(Math.abs(before - 1886.798) < 100, `canonical at the witness point: ${before}`);
    assert.ok(Math.abs(after - 5616.919) < 100, `6000/100km at the witness point: ${after}`);
    assert.ok(after - before > 3000, "the mountain knob did not raise the mountain");
  } finally {
    engine.freeWorld(plain);
    engine.freeWorld(alps);
  }
});

test("the count slider moves the ground in both directions", () => {
  // More AND fewer, because the whole point of the knob is that it goes both ways, and because
  // the parameter behind it runs the opposite way to its name. Witness points from
  // `mountain_probe.rs::witness_for` on this same world: widening the blend to 1.00 takes the
  // most away at -3.00, 69.00 (1,388.596 m -> 354.613 m), and narrowing it to 0.10 adds the
  // most at -33.50, -22.00 (-852.556 m -> 272.983 m, which is a coastline appearing).
  const plain = engine.newWorld({ ...DEFAULT_WORLD });
  const fewer = engine.newWorld({
    ...DEFAULT_WORLD,
    tectonics: { ...canonical, continentalBlend: travel.continentalBlend.toValue(-BLEND_POSITIONS_FEWER) },
  });
  const more = engine.newWorld({
    ...DEFAULT_WORLD,
    tectonics: { ...canonical, continentalBlend: travel.continentalBlend.toValue(BLEND_POSITIONS_MORE) },
  });
  try {
    assert.ok(
      engine.elevationM(fewer, -3.0, 69.0, -1) < engine.elevationM(plain, -3.0, 69.0, -1) - 900,
      "the fewest-mountains end did not lower the witness point",
    );
    assert.ok(
      engine.elevationM(more, -33.5, -22.0, -1) > engine.elevationM(plain, -33.5, -22.0, -1) + 900,
      "the most-mountains end did not raise the witness point",
    );
  } finally {
    for (const handle of [plain, fewer, more]) engine.freeWorld(handle);
  }
});

test("a shared link carries only what was moved", () => {
  const nothing = tectonicToParams(canonical, canonical);
  assert.equal(Object.keys(nothing).length, TECTONIC_CONTROLS.length);
  for (const [name, value] of Object.entries(nothing)) {
    assert.equal(value, null, `${name} is written into a link nobody touched`);
  }
  const moved = { ...canonical, continentCollisionM: 3000 };
  assert.equal(tectonicToParams(moved, canonical).mtnHeight, "3000");
  assert.equal(tectonicToParams(moved, canonical).mtnWidth, null);
  // **And the preset writes all six of its moved fields into the link, including the two with
  // no slider.** A preset that only round-tripped what had a widget would come back half
  // applied on reload -- the silently-dropping shape, arriving through a share link.
  const fromPreset = tectonicToParams(ranges, canonical);
  for (const field of ["continentCollisionM", "continentCollisionWidthM", "collisionAsymmetry",
    "sutureCount", "sutureSpreadM", "structureDepth", "structureWavelengthM"]) {
    assert.equal(
      fromPreset[TECTONIC_PARAM_NAMES[field]], String(ranges[field]),
      `${field} is dropped from a link that carries the preset`,
    );
  }
  assert.equal(fromPreset.mtnCount, null, "the preset does not move the blend");
});

test("the calibration table is the probe's table, and it says what it measured", () => {
  // The panel's note is built from this, so a drift between the sentence the owner reads and
  // the measurement it came from would be silent. Both corners, and the shape of the finding
  // that made the second slider necessary at all.
  const canonicalRow = MEASURED_GRADES[0];
  assert.equal(canonicalRow.heightM, 1500);
  assert.equal(canonicalRow.widthKm, 400);
  assert.equal(canonicalRow.grade, 1.787);
  const steepest = MEASURED_GRADES[MEASURED_GRADES.length - 1];
  assert.equal(steepest.heightM, 6000);
  assert.equal(steepest.widthKm, 100);
  assert.equal(steepest.grade, 7.030);
  // **Amplitude alone is not enough**, which is why there are two sliders and not one: at
  // 3,000 m over the canonical 400 km the grade is 2.677%, barely above canonical, while the
  // same 3,000 m over 150 km is 2.852% and 6,000 m over 100 km is 7.030%.
  const tallGentle = MEASURED_GRADES.find((r) => r.heightM === 3000 && r.widthKm === 400);
  assert.ok(tallGentle.grade < 3, "doubling amplitude alone must not reach a real grade");
  assert.ok(steepest.grade > tallGentle.grade * 2.5);
  // Every row's peak is below its own amplitude: the profile is a smoothstep, not an offset.
  for (const row of MEASURED_GRADES) assert.ok(row.peakM < row.heightM);
});

test("the preset crosses the boundary as fields the panel can show and the sliders can reach", () => {
  // **THE TASK, in one test.** Task 2 built the structure field and the owner could not see
  // one number of it: the five fields were not on the channel at all. So this asserts the
  // whole path -- engine, ABI, travel, widget -- rather than any one link of it.

  // 1. It is fourteen numbers, not a name.
  assert.equal(Object.keys(ranges).length, TECTONIC_FIELDS.length);
  for (const field of TECTONIC_FIELDS) {
    assert.ok(Number.isFinite(ranges[field]), `${field} did not come across as a number`);
  }

  // 2. It is genuinely a different block, and different in the structure fields specifically
  // -- a "preset" that only moved the envelope would be Task 4's sliders with a button on.
  assert.notDeepEqual(ranges, canonical);
  const structural = ["collisionAsymmetry", "sutureCount", "sutureSpreadM", "structureDepth",
    "structureWavelengthM"];
  for (const field of structural) {
    assert.notEqual(ranges[field], canonical[field], `${field} is still at its inert setting`);
  }

  // 3. Every field that HAS a slider lands on that slider's lattice EXACTLY. This is the
  // panel-default defect asked about the preset button: a value the widget cannot express is
  // silently replaced, and the panel then shows a number the engine never chose.
  for (const field of TECTONIC_SLIDERS) {
    const position = travel[field].toPosition(ranges[field]);
    assert.ok(
      Number.isInteger(position) && position >= travel[field].min && position <= travel[field].max,
      `${field} = ${ranges[field]} is off the slider's ${travel[field].min}..${travel[field].max}`,
    );
    assert.equal(
      travel[field].toValue(position), ranges[field],
      `the ${field} slider cannot express the preset's own value`,
    );
  }

  // 4. The engine accepts it. A preset its own boundary refuses is a button that blanks the
  // page, and the reach check is the one most likely to catch it: two sutures 100 km apart on
  // a 100 km flank reach 235 km of the 420 km gate.
  assert.equal(engine.checkTectonic(ranges), WB_OK);

  // 5. And it reaches the GROUND. Built and sampled at the nine probes, against canonical --
  // because a constructor that returned a handle has proved nothing about the block it stored,
  // and because the six relief probes alone are blind to a collision profile on this world.
  const plain = engine.newWorld({ ...DEFAULT_WORLD });
  const preset = engine.newWorld({ ...DEFAULT_WORLD, tectonics: ranges });
  try {
    let moved = 0;
    for (const [lat, lon] of PROBES) {
      const before = engine.elevationM(plain, lat, lon);
      const after = engine.elevationM(preset, lat, lon);
      assert.ok(Number.isFinite(after), `the preset gave a non-finite elevation at ${lat},${lon}`);
      if (before !== after) moved += 1;
    }
    assert.ok(moved > 0, "the preset builds the canonical world");
  } finally {
    for (const handle of [plain, preset]) engine.freeWorld(handle);
  }
});

test("the structure sliders' travel is the band Task 2 measured, and no wider", () => {
  // **A slider whose useful range is a tenth of its travel is a slider nobody can aim**, and
  // the wavelength is the live example: 40-80 km bites and 120-250 km does nothing at any
  // depth. So the travel stops at 40 km and never reaches 250.
  assert.equal(travel.structureWavelengthM.max, WAVELENGTH_STEPS);
  assert.equal(travel.structureWavelengthM.toValue(WAVELENGTH_STEPS), 40000);
  assert.equal(
    travel.structureWavelengthM.toValue(WAVELENGTH_STEPS),
    canonical.structureWavelengthM - WAVELENGTH_STEPS * WAVELENGTH_STEP_M,
  );
  // Three of the five positions are inside the measured working band, and the two that are not
  // include position 0, which Ruling 1 requires to be canonical. Stated as a proportion rather
  // than left implicit: this is the check the brief asked for.
  const live = [];
  for (let p = travel.structureWavelengthM.min; p <= travel.structureWavelengthM.max; p += 1) {
    const km = travel.structureWavelengthM.toValue(p) / 1000;
    if (km >= 40 && km <= 80) live.push(km);
  }
  assert.deepEqual(live, [80, 60, 40]);
  assert.ok(live.length * 2 >= travel.structureWavelengthM.max + 1, "most of the travel is dead");

  // The asymmetry: canonical (symmetric) to 3.00, which is exactly the interval swept.
  assert.equal(travel.collisionAsymmetry.toValue(ASYMMETRY_STEPS), 3);
  assert.equal(travel.collisionAsymmetry.toValue(0), 1);
  // The depth: 0.0 to 0.9, and 0.7 -- the preset's -- must land EXACTLY. `0.1 * 7` is
  // 0.7000000000000001, which is why the map divides rather than multiplies.
  assert.equal(travel.structureDepth.toValue(DEPTH_STEPS), 0.9);
  assert.equal(travel.structureDepth.toValue(7), 0.7);
});

test("the warp crosses to the panel as numbers, and canonical is a great circle", () => {
  // **Ruling 1, asked of Task 5's two fields specifically.** `canonical.marginWarpM` must be
  // exactly 0.0 -- not near it -- because `from_margin` branches on `!= 0.0` before sampling
  // the warp at all, and an untouched panel that wrote 1e-320 into a shared link would take
  // the reload off the engine's `None` path for a displacement of less than an atom.
  assert.equal(canonical.marginWarpM, 0);
  assert.ok(Object.is(canonical.marginWarpM, 0), "canonical wander must be positive zero");
  // And the preset must actually bend something, or the whole task shipped a no-op. The
  // numbers are read from the engine here rather than written down, which is the same rule
  // the rest of this file follows.
  assert.ok(ranges.marginWarpM > 0, "the preset does not bend the margin at all");
  assert.ok(
    ranges.marginWarpWavelengthM === canonical.marginWarpWavelengthM,
    "the preset's warp wavelength is the canonical placeholder, which is why it has no slider",
  );
  // The pair is carried on the channel and answers to a query name, even with no widget --
  // the suture pair's rule, applied to this pair.
  for (const field of ["marginWarpM", "marginWarpWavelengthM"]) {
    assert.ok(TECTONIC_FIELDS.includes(field), `${field} is not on the ABI record`);
    assert.ok(TECTONIC_CONTROLS.includes(field), `${field} is not driven`);
    assert.ok(!TECTONIC_SLIDERS.includes(field), `${field} has a widget it cannot aim`);
    assert.ok(TECTONIC_PARAM_NAMES[field], `${field} has no query-string name`);
  }
});

test("the wander is refused exactly where the range gate would truncate it", () => {
  // **This is why the warp has no slider, asserted rather than asserted-about-in-a-comment.**
  // The warp adds to `collision_reach_m`, the engine holds that sum against
  // `MAX_TECTONIC_RANGE_M`, and at the panel's widest steepness -- canonical's 400 km -- the
  // gate leaves 20 km of room. A slider anchored on canonical would offer one live position.
  //
  // Asked of the shipped artifact through `wb_tectonic_check`, which is the validator the
  // record would actually meet, rather than of a reach re-derived here.
  const gate = 420000;
  const room = gate - canonical.continentCollisionWidthM;
  assert.equal(
    engine.checkTectonic({ ...canonical, marginWarpM: room }), WB_OK,
    "a warp that reaches exactly the gate is inside it",
  );
  assert.notEqual(
    engine.checkTectonic({ ...canonical, marginWarpM: room + 1 }), WB_OK,
    "a warp one metre past the gate must be refused, not truncated",
  );
  // On the preset's own 100 km steepness there is real room, which is the configuration the
  // preset actually ships and the reason 80 km is admissible at all.
  assert.equal(engine.checkTectonic(ranges), WB_OK, "the shipped preset must be admissible");
  assert.notEqual(
    engine.checkTectonic({ ...ranges, marginWarpM: gate }), WB_OK,
    "the preset with a gate-sized warp reaches past the gate and must be refused",
  );
  // And a negative amplitude -- the mirror image -- is outside the channel's stated domain.
  assert.notEqual(
    engine.checkTectonic({ ...canonical, marginWarpM: -20000 }), WB_OK,
    "a negative wander is not this channel's spelling of a mirrored warp",
  );
});
