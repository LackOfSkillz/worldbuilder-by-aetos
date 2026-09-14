// Node-native tests for the viewer's peak (seamount/island) channel: `peak-params.js`,
// `engine.js`'s three new marshalling methods, and the properties that are easy to claim and easy
// to get wrong -- that the untouched path is still the untouched world, that no peak number is
// written down twice, that the joint invariant is asked of the engine rather than re-derived, and
// that every position the density slider can take is a block the shipped artifact accepts.
//
// No framework: `node:test` + `node:assert/strict`, run with `npm test` from `viewer/`.
// Nothing here touches Cesium or the DOM.
//
// Population/method/host, named once so every number below can be traced:
//   - World: Surface::new(20260904, 6_371_000, 12, 0.29, ..) -- DEFAULT_WORLD, the fixture the
//     coast and gully channel tests already hold against this same shipped artifact.
//   - Host: node, this repository's checked-in
//     viewer/public/wasm/worldbuilder_engine.wasm, loaded directly from disk (no fetch).
//   - Probes: eight open-ocean witness points, DERIVED rather than picked -- a 1-degree global
//     scan of this fixture compares the canonical world against `PeakParams::volcanic()` and
//     finds 991 of 64,800 sites moving; these are the eight largest movers subject to a
//     25-degree separation, so they are not eight seamounts on one plateau. **That derivation
//     was run at the then-shipped `density: 0.11`, and Task 7's calibration raised it to
//     0.36. The eight are still witnesses, and monotonicity is why rather than luck:**
//     `Tectonics::peak_of_cell` returns `None` when a cell's hash is `>= density`, so raising
//     the density strictly ADDS candidate cells and can never remove one. A site that moved at
//     0.11 therefore still moves at 0.36 (by at least as much), which the per-probe assertions
//     below check directly rather than inheriting from this note. The 991 is not re-stated for
//     0.36 because nothing re-ran that scan; it is the provenance of the probe set, not a
//     figure about the shipped density. `PEAK_PROBES` in
//     `crates/worldbuilder-engine/tests/wasm_exports.rs` is a *different* six-point set chosen to
//     exercise the land/harbour/ocean gating rather than to witness movement -- its own comment
//     says the on-land and shallow-harbour points are "expected to read back exactly the ground
//     the other five channels already produce there", and none of its open-ocean points happens
//     to land a candidate at this density. A probe set chosen for gating is blind here for the
//     same reason a probe set chosen for another channel is blind to the coast term: the seamount
//     field is a sparse cellular lattice, and a handful of arbitrary ocean points mostly miss it.

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { Engine, WB_OK, WB_ERR_PARAM } from "../public/app/engine.js";
import { panelFieldFaults } from "../public/app/panel-fields.js";
import {
  PEAK_CONTROLS,
  PEAK_SLIDERS,
  PEAK_FIELDS,
  PEAK_STRIDE,
  PEAK_PARAM_NAMES,
  DENSITY_STEPS,
  DENSITY_STEP_HUNDREDTHS,
  peakTravel,
  peakPanelFields,
  peakFromParams,
  peakToParams,
  peakToRecord,
  peakFromRecord,
  peakReadoutFields,
} from "../public/app/peak-params.js";

const DEFAULT_WORLD = { seed: 20260904, radiusM: 6371000, plateCount: 12, landFraction: 0.29 };

/// Eight open-ocean points, none within 25 degrees of another, chosen as the eight largest
/// movers of a 1-degree global scan comparing `PeakParams::canonical()` against
/// `PeakParams::volcanic()` on `DEFAULT_WORLD`. See the module doc for the method and why
/// `wasm_exports.rs`'s own `PEAK_PROBES` cannot answer the same question.
const PROBES = [
  [25.5, 116.5],
  [-53.5, 105.5],
  [67.5, 92.5],
  [-20.5, -151.5],
  [-8.5, 141.5],
  [-20.5, 69.5],
  [-9.5, -69.5],
  [37.5, 93.5],
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
let volcanic;
let travel;

test.before(async () => {
  engine = await loadEngine();
  canonical = engine.peakPreset("canonical");
  volcanic = engine.peakPreset("volcanic");
  travel = peakTravel(canonical);
});

test("the shipped artifact exports the peak channel at all", () => {
  // The original failure mode of this project was a 327-byte module exporting only `memory` from
  // a build that exited 0. A missing `#[no_mangle]` is invisible in green output, so the three
  // names are asked for by name against the artifact this test just loaded.
  for (const name of ["wb_world_new_peak", "wb_peak_preset", "wb_peak_check"]) {
    assert.equal(typeof engine.exports[name], "function", `${name} is missing from the artifact`);
  }
});

test("the preset crosses as five numbers and moves exactly one of them", () => {
  assert.equal(PEAK_FIELDS.length, PEAK_STRIDE);
  assert.deepEqual(Object.keys(canonical), PEAK_FIELDS);
  const moved = PEAK_FIELDS.filter((f) => !Object.is(canonical[f], volcanic[f]));
  assert.deepEqual(moved, ["density"], "volcanic() must move the density and nothing else");
  // And the record round-trips through the ABI order without drifting, which is the property a
  // second copy of a field order would break silently: there is no type error for a height
  // written into the density slot.
  assert.deepEqual(peakFromRecord(peakToRecord(volcanic)), volcanic);
});

test("the untouched path is null, and null is the untouched world", () => {
  // RULING 1, on the viewer's side of the boundary. A page with no peak parameters, and a page
  // whose peak parameters all equal canonical's, must both reach the engine as a null pointer.
  assert.equal(peakFromParams(new URLSearchParams(""), canonical), null);
  const restated = new URLSearchParams(
    PEAK_CONTROLS.map((f) => [PEAK_PARAM_NAMES[f], String(canonical[f])]),
  );
  assert.equal(peakFromParams(restated, canonical), null);
  // A typo is ignored rather than forwarded: answering `?peak=banana` with a refused world would
  // turn a typo in a shared link into a blank page.
  assert.equal(peakFromParams(new URLSearchParams("peak=banana"), canonical), null);
  // But a number OUT of domain is forwarded, because a caller asking for something the engine
  // declines is a different thing from a caller not asking for anything.
  const refused = peakFromParams(new URLSearchParams("peak=-1"), canonical);
  assert.equal(refused.density, -1);
  assert.equal(engine.checkPeak(refused), WB_ERR_PARAM);

  // And the world itself: the default path is byte-for-byte the world with no peak argument.
  const plain = engine.newWorld({ ...DEFAULT_WORLD });
  const defaulted = engine.newWorld({ ...DEFAULT_WORLD, peaks: null });
  const explicit = engine.newWorld({ ...DEFAULT_WORLD, peaks: canonical });
  for (const [lat, lon] of PROBES) {
    const expected = engine.elevationM(plain, lat, lon, 250);
    assert.equal(engine.elevationM(defaulted, lat, lon, 250), expected, `null moved ${lat},${lon}`);
    assert.equal(
      engine.elevationM(explicit, lat, lon, 250), expected, `canonical moved ${lat},${lon}`);
  }
  for (const h of [plain, defaulted, explicit]) assert.equal(engine.freeWorld(h), WB_OK);
});

test("the preset actually stands islands up through the viewer's own constructor", () => {
  // The discriminator. Without it every assertion above would pass over a marshalling layer that
  // allocated five f64 and never handed the pointer to the export -- which is the shape a
  // `peaks ? PEAK_STRIDE : 0` length written the wrong way round would produce.
  const plain = engine.newWorld({ ...DEFAULT_WORLD });
  const rough = engine.newWorld({ ...DEFAULT_WORLD, peaks: volcanic });
  let moved = 0;
  for (const [lat, lon] of PROBES) {
    const before = engine.elevationM(plain, lat, lon, 250);
    const after = engine.elevationM(rough, lat, lon, 250);
    if (after !== before) {
      moved += 1;
      // Every probe was chosen at a seabed well past the depth window's onset, so a probe that
      // moved at all must have moved UP: a seamount raises a seabed, it never lowers one.
      assert.ok(after > before, `probe ${lat},${lon} moved down, not up`);
    }
  }
  // All eight are the derived open-ocean witnesses, so a majority moving is the claim rather than
  // "at least one".
  assert.ok(moved >= 6, `the volcanic preset moved only ${moved} of ${PROBES.length} probes`);
  for (const h of [plain, rough]) assert.equal(engine.freeWorld(h), WB_OK);
});

test("every position the density slider can take is a block the engine accepts", () => {
  // **A control most of whose travel the engine refuses is worse than no control.** The coast and
  // gully channels' version of this test is what caught a mis-stepped default before this one
  // shipped with the same shape.
  const positions = [];
  for (let p = travel.density.min; p <= travel.density.max; p += 1) {
    const block = { ...canonical, density: travel.density.toValue(p) };
    assert.equal(
      engine.checkPeak(block), WB_OK, `position ${p} asks for a block the engine refuses`);
    positions.push(block.density);
  }
  assert.equal(positions.length, DENSITY_STEPS + 1);
  // Position 0 is canonical BIT-FOR-BIT, or an untouched panel writes a density into every shared
  // link and takes the reload off the engine's `None` path.
  assert.ok(Object.is(positions[0], canonical.density));
  // And the preset's own value is ON the lattice, or the slider cannot express the number its own
  // preset button sets.
  assert.ok(
    positions.some((v) => Object.is(v, volcanic.density)),
    "the volcanic preset's density is not a value this slider can produce",
  );
  assert.equal(travel.density.toPosition(volcanic.density), Math.round(volcanic.density * DENSITY_STEP_HUNDREDTHS));
  // And the top of the travel is the domain ceiling itself: every candidate node holds a peak.
  assert.equal(positions[positions.length - 1], 1);
});

test("the density slider satisfies panelFieldFaults", () => {
  // The check that closed a family of shipped panel-default defects, run over this channel's one
  // range control in the units the domain was documented in.
  assert.deepEqual(panelFieldFaults(peakPanelFields(canonical)), []);
  // And it can still fail, over the same table with a deliberately mis-stepped row -- a check
  // nobody has seen fail is a check nobody knows the shape of.
  const broken = peakPanelFields(canonical).map((f) => ({ ...f, value: f.value + 0.0017 }));
  assert.ok(panelFieldFaults(broken).length > 0, "panelFieldFaults cannot see a mis-stepped peak row");
});

test("a shared link carries only what was moved", () => {
  const untouched = peakToParams({ ...canonical }, canonical);
  assert.deepEqual(Object.values(untouched).filter((v) => v !== null), []);
  const chosen = peakToParams(volcanic, canonical);
  assert.equal(chosen[PEAK_PARAM_NAMES.density], String(volcanic.density));
  for (const field of PEAK_CONTROLS) {
    if (field === "density") continue;
    assert.equal(chosen[PEAK_PARAM_NAMES[field]], null, `${field} was written but never moved`);
  }
  // Round trip: what the panel writes is what the boot path reads back.
  const params = new URLSearchParams(
    Object.entries(chosen).filter(([, v]) => v !== null),
  );
  assert.deepEqual(peakFromParams(params, canonical), volcanic);
});

test("the joint bound is the engine's, asked of the engine", () => {
  // The one bound on this channel that no per-field check can see: `reach_m <= lattice_m` is what
  // keeps `peak_of_cell`'s 3x3x3 candidate scan complete. The panel asks `wb_peak_check` rather
  // than re-deriving it, because a second copy of a bound is a second chance to disagree with it
  // -- so what is asserted here is that the engine really does refuse a record that breaks it,
  // i.e. that `checkPeak` is a live question, not that this module knows the bound itself.
  assert.equal(engine.checkPeak({ ...canonical, reach_m: canonical.lattice_m }), WB_OK);
  assert.equal(
    engine.checkPeak({ ...canonical, reach_m: canonical.lattice_m + 1 }), WB_ERR_PARAM,
    "reach one metre past lattice was admitted",
  );
  // Both fields are individually inside their own documented domain here -- this is not a case a
  // per-field sweep of either field alone could ever catch.
  const reach = 60000;
  const lattice = 50000;
  assert.ok(reach > 0 && reach < 1e9 && lattice > 0 && lattice < 1e9);
  assert.equal(
    engine.checkPeak({ ...canonical, reach_m: reach, lattice_m: lattice }), WB_ERR_PARAM,
    "two individually-admissible fields whose ratio breaks the joint bound were admitted",
  );
  // And a world asking for one is refused with a message that names the channel rather than a
  // blank page with a handle of 0.
  assert.throws(
    () => engine.newWorld({ ...DEFAULT_WORLD, peaks: { ...canonical, reach_m: reach, lattice_m: lattice } }),
    /wb_world_new_peak refused.*peaks=WB_ERR_PARAM/s,
  );
});

test("per-field domains are refused past their documented edges", () => {
  for (const density of [-1, -1e-9, 1.5, NaN, Infinity]) {
    assert.equal(engine.checkPeak({ ...canonical, density }), WB_ERR_PARAM, `density ${density} was admitted`);
  }
  for (const height_m of [-1, -1e-9, 1.1e5, NaN, Infinity]) {
    assert.equal(engine.checkPeak({ ...canonical, height_m }), WB_ERR_PARAM, `height_m ${height_m} was admitted`);
  }
  for (const reach_m of [0, -1, NaN, Infinity]) {
    assert.equal(engine.checkPeak({ ...canonical, reach_m }), WB_ERR_PARAM, `reach_m ${reach_m} was admitted`);
  }
  for (const min_depth_m of [-1, -1e-9, 1.1e5, NaN, Infinity]) {
    assert.equal(
      engine.checkPeak({ ...canonical, min_depth_m }), WB_ERR_PARAM, `min_depth_m ${min_depth_m} was admitted`);
  }
  for (const lattice_m of [0, -1, 5, NaN, Infinity]) {
    assert.equal(engine.checkPeak({ ...canonical, lattice_m }), WB_ERR_PARAM, `lattice_m ${lattice_m} was admitted`);
  }
  // And the canonical block itself, and the full-density edge, are both accepted.
  assert.equal(engine.checkPeak(canonical), WB_OK);
  assert.equal(engine.checkPeak({ ...canonical, density: 1.0 }), WB_OK);
});

test("no peak number is written down twice in the viewer", () => {
  // The hazard the whole design is arranged against, stated as a test rather than as a comment --
  // the same test the coast and gully channels carry. Comments are stripped first: prose is
  // allowed to quote a measurement, code is not allowed to restate one.
  const strip = (source) => source
    .split("\n")
    .map((line) => line.replace(/^\s*\/\/.*$/, "").replace(/^\s*\/\/\/.*$/, ""))
    .join("\n");
  // **Only the DISTINCTIVE value can be asked this question, and saying so is part of the test.**
  // Four of the five canonical fields are 8000, 31500, 2500 and 45000, and none of those is a
  // string a source file can be asked not to contain in isolation the way `0.35` was for the
  // coast channel -- but the preset's own density is distinctive enough to ask about.
  //
  // **It is 0.36 and not 0.35, and that is deliberate.** Task 7's survey found seven admissible
  // hundredths (0.32 through 0.38) and picked 0.36 as the maximin. `CoastParams::fractal()`'s
  // amplitude is 0.35, and had the density landed there this assertion would have been
  // indistinguishable from the coast channel's identical one -- a scan that passes only because
  // another channel's guard already holds is a scan that tests nothing of its own. See
  // `VOLCANIC_DENSITY`'s doc in `tectonics.rs` for the sweep this came out of.
  const literals = [String(volcanic.density)];
  assert.deepEqual(literals, ["0.36"]);
  assert.notEqual(literals[0], "0.35", "the peak density must stay distinct from the coast one");
  assert.notEqual(String(canonical.density), literals[0], "the preset must move the density");
  for (const name of ["controls.js", "main.js"]) {
    const code = strip(appFile(name));
    for (const literal of literals) {
      assert.ok(
        !code.includes(literal),
        `${name} restates the peak value ${literal}; it must read it from the engine`,
      );
    }
  }
  for (const fn of [peakTravel, peakPanelFields, peakFromParams, peakToParams]) {
    const source = strip(String(fn));
    for (const literal of literals) {
      assert.ok(
        !source.includes(literal),
        `${fn.name} computes with the peak literal ${literal} instead of its argument`,
      );
    }
    assert.match(source, /canonical/, `${fn.name} must be anchored on the engine's canonical`);
  }
  // And the panel must reach the engine for them, or the assertion above would pass on a viewer
  // that simply has no islands section.
  assert.match(appFile("controls.js"), /from "\.\/peak-params\.js"/);
  assert.match(appFile("main.js"), /engine\.peakPreset\("canonical"\)/);
  assert.match(appFile("main.js"), /engine\.peakPreset\("volcanic"\)/);
  assert.match(appFile("engine.js"), /wb_peak_preset/);
  // **And the constructor really is the widened one.** `newWorld` now calls `wb_world_new_peak`
  // for every path, the peak channel's own door past `wb_world_new_gully`; a viewer that imported
  // this module and still called the gully door would pass every other assertion in this file
  // except the ones that build a world.
  assert.match(appFile("engine.js"), /wb_world_new_peak\(/);
});

test("every driven peak field appears somewhere the owner can see it", () => {
  // A preset that changed something the panel never mentioned would be a parameter the owner
  // cannot see, which is the defect this whole family of slices exists to fix. One field has a
  // slider; the other four are printed. The union must be the whole driven set, so a sixth field
  // added to the channel cannot arrive silently.
  const shown = peakReadoutFields();
  const union = [...PEAK_SLIDERS, ...shown];
  assert.deepEqual([...union].sort(), [...PEAK_CONTROLS].sort());
  // Disjoint as well as exhaustive: an inverted filter would still cover the set by counting the
  // slider's own field twice, and that is the mutation this line exists for.
  assert.equal(new Set(union).size, union.length, "a field is both a slider and a readout");
  const controls = appFile("controls.js");
  assert.match(controls, /peakReadoutFields\(\)/, "the readout must be driven by the field list");
  assert.match(controls, /peakScheduleNote/);
  assert.match(controls, /islands · live/);
});

test("an untouched panel writes no peak parameter at all", () => {
  // The property the whole module exists to hold: a page that never moved the density slider
  // must not put a `peak` (or any of the other four) parameter into a shared link, or the reload
  // would carry a peak block that was never asked for and RULING 1 would break at the one place a
  // generate or a live swap can break it. `peakToParams` over the untouched state is `null` for
  // every field, which is exactly what `apply`'s and `nextQueryString`'s drop rule needs to see.
  const untouched = peakToParams(canonical, canonical);
  for (const field of PEAK_CONTROLS) {
    assert.equal(untouched[PEAK_PARAM_NAMES[field]], null, `${field} was written by an untouched panel`);
  }
});

test("the panel surfaces a joint-bound refusal rather than staying silent", () => {
  // The honest requirement this channel's header names: an owner who sets `reach_m` above
  // `lattice_m` in a shared link must be told the engine refused the block, not left watching a
  // density slider at 1.0 with no islands and no explanation. `controls.js` wires this through
  // `presets.check`, which is `engine.checkPeak` -- asserted here as "the panel imports the check
  // and prints something when it fails" against the source, since the DOM-driving half needs a
  // browser this test suite does not have.
  const controls = appFile("controls.js");
  assert.match(controls, /peakAdmissibleNote/);
  assert.match(controls, /presets\.check\(peakState\)/);
  assert.match(controls, /the engine will refuse this block/);
});
