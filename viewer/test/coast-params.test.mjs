// Node-native tests for the viewer's coast channel: `coast-params.js`, `engine.js`'s three new
// marshalling methods, and the properties that are easy to claim and easy to get wrong -- that the
// untouched path is still the untouched world, that no coast number is written down twice, and that
// every position the raggedness slider can take is a block the shipped artifact accepts.
//
// No framework: `node:test` + `node:assert/strict`, run with `npm test` from `viewer/`.
// Nothing here touches Cesium or the DOM.
//
// Population/method/host, named once so every number below can be traced:
//   - World: Surface::new(20260904, 6_371_000, 12, 0.29, ..) -- DEFAULT_WORLD, the fixture whose
//     elevation at lat 12 lon 34 was witnessed three independent ways.
//   - Host: node, this repository's checked-in
//     viewer/public/wasm/worldbuilder_engine.wasm, loaded directly from disk (no fetch).
//   - Probes: eight COASTAL witness points plus three of the older channels' probes. The coastal
//     eight were DERIVED rather than picked: a 0.5-degree global scan of this fixture compares the
//     canonical world against `CoastParams::fractal()` and finds 162,159 of 258,480 sites moving;
//     these are the largest movers subject to a 25-degree separation, so they are not eight
//     samples of one bay. **A probe set chosen for another channel is blind here**: the coastal
//     term is windowed by distance from the shore, and a scatter over a sphere that is 71% open
//     water mostly lands outside the window. Same list as `COAST_PROBES` in
//     `crates/worldbuilder-engine/tests/wasm_exports.rs`, and both sides say where it came from.
//   - Slider population: every integer position the one slider can take -- 16 -- each turned into
//     a coast block and put through `wb_coast_check`.

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { Engine, WB_OK, WB_ERR_PARAM } from "../public/app/engine.js";
import { panelFieldFaults } from "../public/app/panel-fields.js";
import {
  COAST_CONTROLS,
  COAST_SLIDERS,
  COAST_FIELDS,
  COAST_STRIDE,
  COAST_PARAM_NAMES,
  MEASURED_COAST,
  USEFUL_BAND,
  AMPLITUDE_STEPS,
  AMPLITUDE_STEP_TWENTIETHS,
  coastTravel,
  coastPanelFields,
  coastFromParams,
  coastToParams,
  coastToRecord,
  coastFromRecord,
  coastReadoutFields,
} from "../public/app/coast-params.js";

const DEFAULT_WORLD = { seed: 20260904, radiusM: 6371000, plateCount: 12, landFraction: 0.29 };

const PROBES = [
  [-71.5, 38.0],
  [-73.0, -132.0],
  [3.0, -107.5],
  [-14.5, -20.5],
  [17.5, 57.5],
  [11.5, -174.5],
  [66.0, -82.5],
  [71.5, 141.5],
  [12.0, 34.0],
  [0.0, 0.0],
  [-18.25, 121.5],
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
let fractal;
let travel;

test.before(async () => {
  engine = await loadEngine();
  canonical = engine.coastPreset("canonical");
  fractal = engine.coastPreset("fractal");
  travel = coastTravel(canonical);
});

test("the shipped artifact exports the coast channel at all", () => {
  // The original failure mode of this project was a 327-byte module exporting only `memory` from
  // a build that exited 0. A missing `#[no_mangle]` is invisible in green output, so the three
  // names are asked for by name against the artifact this test just loaded.
  for (const name of ["wb_world_new_coast", "wb_coast_preset", "wb_coast_check"]) {
    assert.equal(typeof engine.exports[name], "function", `${name} is missing from the artifact`);
  }
});

test("the preset crosses as six numbers and moves exactly one of them", () => {
  assert.equal(COAST_FIELDS.length, COAST_STRIDE);
  assert.deepEqual(Object.keys(canonical), COAST_FIELDS);
  const moved = COAST_FIELDS.filter((f) => !Object.is(canonical[f], fractal[f]));
  assert.deepEqual(moved, ["amplitude"], "fractal() must move the amplitude and nothing else");
  // And the record round-trips through the ABI order without drifting, which is the property a
  // second copy of a field order would break silently: there is no type error for a gain written
  // into the frequency slot.
  assert.deepEqual(coastFromRecord(coastToRecord(fractal)), fractal);
});

test("the untouched path is null, and null is the untouched world", () => {
  // RULING 1, on the viewer's side of the boundary. A page with no coast parameters, and a page
  // whose coast parameters all equal canonical's, must both reach the engine as a null pointer.
  assert.equal(coastFromParams(new URLSearchParams(""), canonical), null);
  const restated = new URLSearchParams(
    COAST_CONTROLS.map((f) => [COAST_PARAM_NAMES[f], String(canonical[f])]),
  );
  assert.equal(coastFromParams(restated, canonical), null);
  // A typo is ignored rather than forwarded: answering `?coast=banana` with a refused world would
  // turn a typo in a shared link into a blank page.
  assert.equal(coastFromParams(new URLSearchParams("coast=banana"), canonical), null);
  // But a number OUT of domain is forwarded, because a caller asking for something the engine
  // declines is a different thing from a caller not asking for anything.
  const refused = coastFromParams(new URLSearchParams("coast=-1"), canonical);
  assert.equal(refused.amplitude, -1);
  assert.equal(engine.checkCoast(refused), WB_ERR_PARAM);

  // And the world itself: the default path is byte-for-byte the world with no coast argument.
  const plain = engine.newWorld({ ...DEFAULT_WORLD });
  const defaulted = engine.newWorld({ ...DEFAULT_WORLD, coast: null });
  const explicit = engine.newWorld({ ...DEFAULT_WORLD, coast: canonical });
  for (const [lat, lon] of PROBES) {
    const expected = engine.elevationM(plain, lat, lon, 250);
    assert.equal(engine.elevationM(defaulted, lat, lon, 250), expected, `null moved ${lat},${lon}`);
    assert.equal(
      engine.elevationM(explicit, lat, lon, 250), expected, `canonical moved ${lat},${lon}`);
  }
  for (const h of [plain, defaulted, explicit]) assert.equal(engine.freeWorld(h), WB_OK);
});

test("the preset actually roughens the coast through the viewer's own constructor", () => {
  // The discriminator. Without it every assertion above would pass over a marshalling layer that
  // allocated six f64 and never handed the pointer to the export -- which is the shape a
  // `coast ? COAST_STRIDE : 0` length written the wrong way round would produce.
  const plain = engine.newWorld({ ...DEFAULT_WORLD });
  const rough = engine.newWorld({ ...DEFAULT_WORLD, coast: fractal });
  let moved = 0;
  for (const [lat, lon] of PROBES) {
    if (engine.elevationM(rough, lat, lon, 250) !== engine.elevationM(plain, lat, lon, 250)) {
      moved += 1;
    }
  }
  // Eight of the eleven probes are the derived coastal witnesses, so a majority moving is the
  // claim rather than "at least one".
  assert.ok(moved >= 8, `the fractal preset moved only ${moved} of ${PROBES.length} probes`);
  for (const h of [plain, rough]) assert.equal(engine.freeWorld(h), WB_OK);
});

test("every position the raggedness slider can take is a block the engine accepts", () => {
  // **A control most of whose travel the engine refuses is worse than no control.** The tectonic
  // channel's version of this test is what caught `margin_warp_m` having exactly one live
  // position at the panel's widest steepness, and it is the reason that field has no widget.
  const positions = [];
  for (let p = travel.amplitude.min; p <= travel.amplitude.max; p += 1) {
    const block = { ...canonical, amplitude: travel.amplitude.toValue(p) };
    assert.equal(
      engine.checkCoast(block), WB_OK, `position ${p} asks for a block the engine refuses`);
    positions.push(block.amplitude);
  }
  assert.equal(positions.length, AMPLITUDE_STEPS + 1);
  // Position 0 is canonical BIT-FOR-BIT, or an untouched panel writes an amplitude into every
  // shared link and takes the reload off the engine's `None` path.
  assert.ok(Object.is(positions[0], canonical.amplitude));
  // And the preset's own value is ON the lattice, or the slider cannot express the number its own
  // preset button sets -- the panel-default defect this viewer has shipped four times.
  assert.ok(
    positions.some((v) => Object.is(v, fractal.amplitude)),
    "the fractal preset's amplitude is not a value this slider can produce",
  );
  assert.equal(travel.amplitude.toPosition(fractal.amplitude), 7);
});

test("the slider travel is the band the survey measured, not a tenth of it", () => {
  // **The calibration is the point of the travel, so it is asserted rather than commented.** Task
  // 5 measured the coast becoming visible at about 0.10 and beginning to fragment above about
  // 0.75; a slider running to 4.0 -- the engine's own ceiling -- would put the whole useful band
  // in the first fifth of its throw and be a slider nobody can aim.
  const values = [];
  for (let p = travel.amplitude.min; p <= travel.amplitude.max; p += 1) {
    values.push(travel.amplitude.toValue(p));
  }
  const top = values[values.length - 1];
  assert.ok(Math.abs(top - USEFUL_BAND.high) < 1e-12, `the travel stops at ${top}, not the band's top`);
  // Fourteen of the sixteen positions are inside the measured band; only canonical's dead zero and
  // the one below-visible step are not.
  const inside = values.filter((v) => v >= USEFUL_BAND.low && v <= USEFUL_BAND.high).length;
  assert.equal(inside, values.length - 2);
  assert.ok(inside / values.length > 0.8, "most of the travel must be inside the measured band");
  // And the engine would accept far more than the panel offers, so the travel is a CHOICE made
  // from the measurement rather than the widest thing that happens to work.
  assert.equal(engine.checkCoast({ ...canonical, amplitude: 1.5 }), WB_OK);
  assert.equal(engine.checkCoast({ ...canonical, amplitude: 4.0 }), WB_OK);
});

test("the raggedness slider satisfies panelFieldFaults", () => {
  // The check that closed the family of four shipped defects, run over this channel's one range
  // control in the units the calibration was measured in.
  assert.deepEqual(panelFieldFaults(coastPanelFields(canonical)), []);
  // And it can still fail, over the same table with a deliberately mis-stepped row -- a check
  // nobody has seen fail is a check nobody knows the shape of.
  // Mis-stepped in the shape the defect actually took: a DEFAULT that is not on the lattice the
  // widget can produce. Changing the step alone is invisible here, and that is itself worth
  // knowing -- canonical's amplitude is the slider's own minimum, and `min + 0 * step` is on
  // every lattice, so this channel's default is one the check cannot fault by construction. The
  // fault has to be introduced in the value.
  const broken = coastPanelFields(canonical).map((f) => ({ ...f, value: f.value + 0.017 }));
  assert.ok(panelFieldFaults(broken).length > 0, "panelFieldFaults cannot see a mis-stepped coast row");
});

test("a shared link carries only what was moved", () => {
  const untouched = coastToParams({ ...canonical }, canonical);
  assert.deepEqual(Object.values(untouched).filter((v) => v !== null), []);
  const chosen = coastToParams(fractal, canonical);
  assert.equal(chosen[COAST_PARAM_NAMES.amplitude], String(fractal.amplitude));
  for (const field of COAST_CONTROLS) {
    if (field === "amplitude") continue;
    assert.equal(chosen[COAST_PARAM_NAMES[field]], null, `${field} was written but never moved`);
  }
  // Round trip: what the panel writes is what the boot path reads back.
  const params = new URLSearchParams(
    Object.entries(chosen).filter(([, v]) => v !== null),
  );
  assert.deepEqual(coastFromParams(params, canonical), fractal);
});

test("the joint bounds are the engine's, asked of the engine", () => {
  // Two of this channel's bounds are joint and neither is visible to a per-field check. The panel
  // asks `wb_coast_check` rather than re-deriving them, because a second copy of a bound is a
  // second chance to disagree with it -- so what is asserted here is that the engine really does
  // refuse these, i.e. that `checkCoast` is a live question.
  //
  // The octave count is a per-sample LOOP BOUND: `as u32` saturates in Rust, so 1e300 would
  // arrive as four billion octaves per elevation sample.
  for (const octaves of [0, 2.5, 17, 1e300, Infinity, NaN, 4294967295]) {
    assert.equal(
      engine.checkCoast({ ...fractal, octaves }), WB_ERR_PARAM, `octaves ${octaves} was admitted`);
  }
  // The finest octave's frequency is a PRODUCT of three fields that are each individually inside
  // their own domain -- `frequency * lacunarity^(octaves - 1)` -- and past the point where the
  // noise lattice's `i64` index saturates it is an abort, not a wrong answer.
  assert.equal(engine.checkCoast({ ...fractal, frequency: 1e6, octaves: 1, lacunarity: 1 }), WB_OK);
  assert.equal(engine.checkCoast({ ...fractal, frequency: 20, octaves: 16, lacunarity: 1 }), WB_OK);
  assert.equal(engine.checkCoast({ ...fractal, frequency: 20, octaves: 1, lacunarity: 16 }), WB_OK);
  assert.equal(
    engine.checkCoast({ ...fractal, frequency: 1e6, octaves: 16, lacunarity: 16 }),
    WB_ERR_PARAM,
    "three fields each inside their own domain were admitted with a product that is not",
  );
  // And a world asking for one is refused with a message that names the channel rather than a
  // blank page with a handle of 0.
  assert.throws(
    () => engine.newWorld({ ...DEFAULT_WORLD, coast: { ...fractal, octaves: 0 } }),
    // **The constructor's name moved and this regex moved with it**, deliberately rather than by
    // loosening. `newWorld` now calls the widest door, `wb_world_new_gully`, for every path -- the
    // gully channel's wiring -- so the message names that export. What this assertion is about is
    // unchanged and is the second half: a refused world says WHICH channel refused it, rather than
    // handing back a handle of 0 and a blank page.
    /wb_world_new_gully refused.*coast=WB_ERR_PARAM/s,
  );
});

test("no coast number is written down twice in the viewer", () => {
  // The hazard the whole design is arranged against, stated as a test rather than as a comment --
  // the same test the relief and tectonic channels carry. Comments are stripped first: prose is
  // allowed to quote a measurement, code is not allowed to restate one.
  const strip = (source) => source
    .split("\n")
    .map((line) => line.replace(/^\s*\/\/.*$/, "").replace(/^\s*\/\/\/.*$/, ""))
    .join("\n");
  // **Only the DISTINCTIVE values can be asked this question, and saying so is part of the test.**
  // FIVE of the six canonical fields are 1, 20, 4, 0.5 and 2, and none of those is a string a
  // source file can be asked not to contain -- `controls.js` has a `20` in an unrelated readout
  // and would fail on it. So the file-level half asks about the one value nothing else in this
  // viewer is, the preset's amplitude, and the other five are covered by the *function-source*
  // half below, which asks the sharper question anyway: not "does this digit appear" but "is any
  // coast value computed from a literal rather than from the engine's block".
  const literals = [String(fractal.amplitude)];
  assert.deepEqual(literals, ["0.35"]);
  assert.notEqual(String(canonical.amplitude), literals[0], "the preset must move the amplitude");
  for (const name of ["controls.js", "main.js"]) {
    const code = strip(appFile(name));
    for (const literal of literals) {
      assert.ok(
        !code.includes(literal),
        `${name} restates the coast value ${literal}; it must read it from the engine`,
      );
    }
  }
  // `coast-params.js` is asked the SHARPER question rather than the same one, because it
  // legitimately contains "0.35": `MEASURED_COAST` is the survey's own table and every row names
  // the amplitude it was measured at, the preset's included. A table that could not say "at 0.35
  // the coast is 1.591x longer with 175 inlet heads" would be a table about nothing.
  for (const fn of [coastTravel, coastPanelFields, coastFromParams, coastToParams]) {
    const source = strip(String(fn));
    for (const literal of literals) {
      assert.ok(
        !source.includes(literal),
        `${fn.name} computes with the coast literal ${literal} instead of its argument`,
      );
    }
    assert.match(source, /canonical/, `${fn.name} must be anchored on the engine's canonical`);
  }
  // And the panel must reach the engine for them, or the assertion above would pass on a viewer
  // that simply has no coastline section.
  assert.match(appFile("controls.js"), /from "\.\/coast-params\.js"/);
  assert.match(appFile("main.js"), /engine\.coastPreset\("canonical"\)/);
  assert.match(appFile("main.js"), /engine\.coastPreset\("fractal"\)/);
  assert.match(appFile("engine.js"), /wb_coast_preset/);
});

test("every driven coast field appears somewhere the owner can see it", () => {
  // A preset that changed something the panel never mentioned would be a parameter the owner
  // cannot see, which is the defect this whole slice exists to fix. One field has a slider; the
  // other five are printed. The union must be the whole driven set, so a seventh field added to
  // the channel cannot arrive silently.
  const shown = coastReadoutFields();
  const union = [...COAST_SLIDERS, ...shown];
  assert.deepEqual([...union].sort(), [...COAST_CONTROLS].sort());
  // Disjoint as well as exhaustive: an inverted filter would still cover the set by counting the
  // slider's own field twice, and that is the mutation this line exists for.
  assert.equal(new Set(union).size, union.length, "a field is both a slider and a readout");
  const controls = appFile("controls.js");
  assert.match(controls, /coastReadoutFields\(\)/, "the readout must be driven by the field list");
  assert.match(controls, /coastScheduleNote/);
});

test("the panel's not-wired list is honest about the two entries this task touched", () => {
  // Comments are NOT stripped here: the reason an entry came off is kept in the file, and that is
  // deliberate.
  const controls = appFile("controls.js");
  const entries = controls.slice(controls.indexOf("const NOT_WIRED = ["));
  const list = entries.slice(0, entries.indexOf("];"));
  // The coastline is wired now, and there is a section for it.
  assert.ok(!list.includes('["coastline"'), "the coastline is listed as not wired");
  assert.ok(!list.includes('["fractal coast'), "the coastline is listed as not wired");
  // The section is now titled `· live` rather than `· rebuilds`: the coast slider swaps in place.
  assert.match(controls, /coastline · live/);
  // **And the water entry was WRONG, not merely stale.** It said `no export yet`, which has been
  // false since slice 5b: `wb_water_run` ships in the committed artifact, as this test proves by
  // asking the artifact rather than the comment. What was missing was a viewer that calls it.
  assert.equal(typeof engine.exports.wb_water_run, "function");
  assert.ok(!list.includes("no export yet"), "the water entry still claims there is no export");
  // **The claim this line makes has been RAISED, not dropped.** It used to be "the list names the
  // export that ships"; the water task then made the entry itself obsolete by wiring it, so
  // asserting the old text would now pin a false statement. The stronger successor is that the
  // entry is GONE and the thing that replaced it exists -- a viewer that calls the export, and a
  // panel section for it. That is exactly what the coastline three lines above is held to, so the
  // two wired capabilities are now checked the same way rather than one being a special case.
  assert.ok(
    !list.includes("wb_water_run"),
    "the water entry still says nothing calls wb_water_run, but main.js does",
  );
  assert.match(appFile("main.js"), /engine\.waterRun\(/, "main.js must call the water export");
  assert.match(controls, /water · live/);
  // The two narrower entries that replaced it are real limitations, not a relabelling: a body
  // arrives as a level and a box, and no body is ever classified a pond on this mesh.
  assert.ok(list.includes('["lake shorelines"'), "the box limitation must stay on the list");
  assert.ok(list.includes('["ponds"'), "the zero-ponds finding must stay on the list");
  // And rivers must NOT have come off with them: reaches are carried and deliberately empty.
  assert.ok(list.includes('["rivers"'), "rivers are schema-only and must stay listed");
});

test("the measured table is the one the panel reads, and it says what the band is", () => {
  // The panel's note quotes this table; the table is the survey's output. Neither may drift from
  // the other, and the shape of the finding is asserted rather than described: the coastline gets
  // longer, monotonically, and the inlet count moves by two orders of magnitude.
  assert.ok(MEASURED_COAST.length >= 7);
  for (let i = 1; i < MEASURED_COAST.length; i += 1) {
    assert.ok(
      MEASURED_COAST[i].lengthRatio > MEASURED_COAST[i - 1].lengthRatio,
      "the measured length ratio must rise with amplitude",
    );
    assert.ok(MEASURED_COAST[i].amplitude > MEASURED_COAST[i - 1].amplitude);
  }
  assert.equal(MEASURED_COAST[0].amplitude, 0);
  assert.equal(MEASURED_COAST[0].lengthRatio, 1);
  const preset = MEASURED_COAST.find((r) => Object.is(r.amplitude, fractal.amplitude));
  assert.ok(preset, "the table has no row at the preset's own amplitude");
  assert.ok(preset.inletHeads / MEASURED_COAST[0].inletHeads > 50);
  // **The strait.** The largest landmass's share of the land falls from 87.8% to 48.6% between
  // amplitude 0.10 and 0.15, and the count of islands over 100,000 km2 rises 5 -> 9 at the same
  // step -- so what happened is not fragmentation but a strait opening through the supercontinent
  // and four large landmasses separating from it. The shipped preset is above that threshold.
  // Asserted here because it is a topology change the owner sees the first time it is drawn, and
  // a number in a report is easier to lose than a failing test.
  const below = MEASURED_COAST.find((r) => Object.is(r.amplitude, 0.1));
  const above = MEASURED_COAST.find((r) => Object.is(r.amplitude, 0.15));
  assert.ok(below.largestShare - above.largestShare > 30, "the strait is not in the table");
  assert.ok(above.largeIslands > below.largeIslands, "the pieces the strait made are not large");
  assert.ok(fractal.amplitude > above.amplitude, "the preset is below the strait threshold");
  // And the step is exactly the one the twentieths lattice was chosen for.
  assert.equal(AMPLITUDE_STEP_TWENTIETHS, 20);
});
