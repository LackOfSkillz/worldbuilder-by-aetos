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
//     was run at the then-shipped `density: 0.11`; calibration then raised it to 0.36, and the
//     final fix wave re-surveyed it down to 0.14 after finding the seamount term suppressed
//     over 77% of the planet. The eight are still witnesses, and monotonicity is why rather
//     than luck:** `Tectonics::peak_of_cell` returns `None` when a cell's hash is `>= density`,
//     so raising the density strictly ADDS candidate cells and can never remove one. Every
//     density this preset has shipped is at or above the 0.11 the eight were derived at --
//     0.11, 0.36, 0.14 -- so a site that moved at 0.11 still moves at every one of them, by at
//     least as much. The per-probe assertions below check that directly rather than inheriting
//     it from this note, and they also gained a floor from the suppression fix rather than
//     losing one. The 991 is not re-stated for the shipped density because nothing re-ran that
//     scan; it is the provenance of the probe set, not a figure about the density. `PEAK_PROBES` in
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
  peakBootPlan,
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

/// `tectonics.rs` itself, read the same way `appFile` reads the viewer's own modules -- the
/// authoritative source `PeakParams`'s field order and `VOLCANIC_DENSITY` both live in.
const tectonicsSource = () =>
  readFileSync(
    fileURLToPath(new URL("../../crates/worldbuilder-engine/src/tectonics.rs", import.meta.url)),
    "utf8",
  );

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

test("the round trip catches a field swap, not just a length change", () => {
  // **The gap the test above cannot see.** `height_m` and `min_depth_m` share the domain
  // `[0, 1e5]`, so a `PEAK_FIELDS` that swapped their two positions would still pass every
  // assertion above: `canonical`/`volcanic` never move `height_m` or `min_depth_m` at all, and a
  // round trip of `volcanic` through a permuted order is invisible when the two swapped slots
  // happen to hold values that are still individually valid at the other's position.
  //
  // **What a round trip can and cannot do, stated precisely, because an earlier version of this
  // comment overclaimed.** `peakToRecord` and `peakFromRecord` both iterate the SAME
  // `PEAK_FIELDS` array (`peak-params.js:195` and `:201`), so they are inverses of each other
  // whatever order that array is in -- a permuted `PEAK_FIELDS` round-trips perfectly. The
  // round trip therefore closes the encode/decode SYMMETRY, not the order; the claim it used to
  // make, that it catches any swap "without ever reading `tectonics.rs`", was false. **The test
  // that actually closes the order is the next one**, which reads `PeakParams`'s own declaration
  // out of `tectonics.rs` -- and on the Rust side `wasm.rs`'s `peak_wire_format_tests` closes it
  // against the engine's `decode_peak` directly.
  //
  // So this test is made non-vacuous the only way a same-file test can be: a DISTINCT sentinel
  // per field, asserted against a **literal expected order written here**, not against
  // `PEAK_FIELDS`. A permutation of `PEAK_FIELDS` now fails the per-index assertions below,
  // because the index each value must land at is spelled out rather than read from the array
  // under test. The round-trip assertion is kept for what it does cover.
  const sentinel = { height_m: 1111, density: 0.2222, reach_m: 3333, min_depth_m: 4444, lattice_m: 5555 };
  const record = peakToRecord(sentinel);
  // The ABI order, spelled out. This is the one place in this file that writes it down, and that
  // is deliberate: a second, independent copy is what makes the comparison mean something. It is
  // checked against `tectonics.rs`'s own declaration by the next test, so the two cannot drift.
  const expected = [1111, 0.2222, 3333, 4444, 5555];
  assert.deepEqual(record, expected, "peakToRecord did not lay the five out in the ABI order");
  // And each name maps to the slot that order implies, so a failure says which field moved.
  ["height_m", "density", "reach_m", "min_depth_m", "lattice_m"].forEach((field, index) => {
    assert.equal(record[index], sentinel[field], `${field} is not at index ${index} of the record`);
  });
  assert.equal(PEAK_FIELDS.length, expected.length, "PEAK_FIELDS changed length");
  assert.deepEqual(peakFromRecord(record), sentinel);
});

test("PEAK_FIELDS is pinned against PeakParams's own declared order in tectonics.rs", () => {
  // **The authoritative source, not a second copy of the claim.** `PeakParams`'s doc comment
  // states its field order IS the ABI; this reads that struct's own declaration directly (the
  // same grep-the-source technique `no peak number is written down twice in the viewer` below
  // uses for `VOLCANIC_DENSITY`), so a future edit that reordered the struct -- or this file --
  // would be named rather than silently agreeing with itself.
  const source = tectonicsSource();
  const structMatch = source.match(/pub struct PeakParams \{([\s\S]*?)\n\}/);
  assert.ok(structMatch, "PeakParams struct not found in tectonics.rs");
  const declared = [...structMatch[1].matchAll(/pub (\w+):/g)].map((m) => m[1]);
  assert.deepEqual(
    PEAK_FIELDS, declared,
    `PEAK_FIELDS has drifted from PeakParams's declared order in tectonics.rs (Rust says: ` +
    `${declared.join(", ")}; JS says: ${PEAK_FIELDS.join(", ")})`,
  );
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

  // And the world itself: the default path is byte-for-byte the world with no peak argument, at
  // every probe and at every resolution -- the same three the gully channel's own version of this
  // test checks (250 m, 76.35 m and 5000 m), rather than one alone.
  const plain = engine.newWorld({ ...DEFAULT_WORLD });
  const defaulted = engine.newWorld({ ...DEFAULT_WORLD, peaks: null });
  const explicit = engine.newWorld({ ...DEFAULT_WORLD, peaks: canonical });
  for (const [lat, lon] of PROBES) {
    for (const resolution of [250, 76.35, 5000]) {
      const expected = engine.elevationM(plain, lat, lon, resolution);
      assert.equal(
        engine.elevationM(defaulted, lat, lon, resolution), expected,
        `null moved ${lat},${lon} at ${resolution} m`);
      assert.equal(
        engine.elevationM(explicit, lat, lon, resolution), expected,
        `canonical moved ${lat},${lon} at ${resolution} m`);
    }
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
  // **Read from `tectonics.rs` itself, not hard-coded.** A literal number here (`"0.11"`, the
  // pre-calibration density, or `"0.36"`, the value Task 7's survey landed on) would go stale the
  // day `VOLCANIC_DENSITY` next moves -- the grep below would keep passing, but vacuously: it
  // would be checking that nobody transcribed a number nobody would transcribe. `volcanic.density`
  // is already read live across the boundary (`wb_peak_preset`, above in `test.before`), and this
  // cross-checks it against the constant's own declaration in `tectonics.rs` -- the same
  // grep-the-source technique `PEAK_FIELDS is pinned against PeakParams's own declared order`
  // uses -- so a drift between the two is named rather than silently trusted.
  const volcanicDensityMatch = tectonicsSource().match(/const VOLCANIC_DENSITY: f64 = ([\d.]+)/);
  assert.ok(volcanicDensityMatch, "VOLCANIC_DENSITY not found in tectonics.rs");
  assert.equal(
    volcanic.density, Number(volcanicDensityMatch[1]),
    "wb_peak_preset(volcanic)'s density has drifted from VOLCANIC_DENSITY in tectonics.rs",
  );
  // `CoastParams::fractal()`'s amplitude is 0.35, and had the density landed there this assertion
  // would have been indistinguishable from the coast channel's identical one -- a scan that
  // passes only because another channel's guard already holds is a scan that tests nothing of its
  // own.
  const literals = [String(volcanic.density)];
  assert.notEqual(literals[0], "0.35", "the peak density must stay distinct from the coast one");
  assert.notEqual(String(canonical.density), literals[0], "the preset must move the density");
  // **Widened by the final whole-branch review's minor 8 from `controls.js`/`main.js` to all
  // four modules of this channel.** `peak-params.js`'s own header claimed the scan covered "this
  // file and `controls.js`", and it covered neither this module nor `engine.js`; both of those do
  // write the preset's numbers down in comments, so the gap was real rather than theoretical.
  // Widening the test was chosen over narrowing the comment: the comment described the scan
  // people would want, and the scan is cheap.
  for (const name of ["controls.js", "main.js", "peak-params.js", "engine.js"]) {
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

test("a hand-edited query string that breaks the joint bound reaches an owner, not a swallowed throw", () => {
  // **Blocker 1, exercised rather than only claimed.** Before this fix, the only path that could
  // ever make `peakAdmissibleNote` fire was a query string with `reach_m > lattice_m` -- and that
  // exact query string, run through `main.js`'s boot path, threw out of `wb_world_new_peak`
  // straight past `boot().catch(...)` at the bottom of `main.js`, which swallows the error without
  // ever publishing `window.__wb`. `controls.js`'s `wirePeaks` never runs, so the check this test
  // file's own header advertises never gets asked. The owner saw the same generic "engine
  // unavailable" text a truly dead engine produces -- worse than silence, because it blames the
  // wrong thing.
  //
  // This test drives the actual mechanism the fix adds, `peakBootPlan`, with the exact shape a
  // user's query string produces -- `peakFromParams` over `?peakReach=`/`?peakLattice=` -- and
  // proves two things a source grep cannot: the constructor never sees the refused block (so boot
  // does not throw and `window.__wb` gets published), and the block the panel would still show the
  // owner is the refused one, live-checked, so `peakAdmissibleNote` actually has something to say.
  const query = new URLSearchParams("peakReach=60000&peakLattice=50000");
  const requested = peakFromParams(query, canonical);
  assert.equal(requested.reach_m, 60000);
  assert.equal(requested.lattice_m, 50000);
  assert.equal(engine.checkPeak(requested), WB_ERR_PARAM, "this query string must be the refused shape");

  const plan = peakBootPlan(requested, engine.checkPeak(requested) === WB_OK);
  assert.equal(plan.refused, true);
  // **The property that keeps boot alive**: the refused block never reaches the constructor.
  assert.equal(plan.forConstructor, null);
  assert.doesNotThrow(
    () => {
      const handle = engine.newWorld({ ...DEFAULT_WORLD, peaks: plan.forConstructor });
      // And the fallback really is the canonical, island-free ocean, not merely "a world" --
      // every probe reads back exactly what the plain constructor call gives.
      const plain = engine.newWorld({ ...DEFAULT_WORLD });
      for (const [lat, lon] of PROBES) {
        assert.equal(
          engine.elevationM(handle, lat, lon, 250), engine.elevationM(plain, lat, lon, 250),
          `the boot fallback moved ${lat},${lon} away from canonical`,
        );
      }
      assert.equal(engine.freeWorld(handle), WB_OK);
      assert.equal(engine.freeWorld(plain), WB_OK);
    },
    "a refused peak block must never reach wb_world_new_peak at boot",
  );

  // **The property that keeps the note honest**: the panel's `chosen` block (what `main.js`
  // publishes on `window.__wb.peaks.chosen` when `plan.refused` is true) is the ORIGINAL request,
  // not the fallback -- so `presets.check(peakState)` in `controls.js`'s `paint()` is asked about
  // the block the owner actually typed, and it fails live rather than defaulting to "admissible".
  const chosenForPanel = plan.refused ? requested : plan.forConstructor;
  assert.equal(chosenForPanel, requested);
  assert.equal(engine.checkPeak(chosenForPanel), WB_ERR_PARAM);

  // And a request that was never refused must pass through untouched -- this fix must not turn
  // every peak block into a fallback, only the ones the engine actually declines.
  const fine = peakBootPlan(volcanic, engine.checkPeak(volcanic) === WB_OK);
  assert.deepEqual(fine, { forConstructor: volcanic, refused: false });
  const untouched = peakBootPlan(null, true);
  assert.deepEqual(untouched, { forConstructor: null, refused: false });

  // Finally, that this is actually how `main.js` is wired, not a helper that sits unused: it must
  // check the request before deciding what reaches the constructor, and the panel's `chosen`
  // getter must be able to see the refused request rather than only what got built.
  const mainSource = appFile("main.js");
  assert.match(mainSource, /peakBootPlan\(/, "main.js must call peakBootPlan");
  assert.match(
    mainSource, /engine\.checkPeak\(peaksRequested\)/,
    "main.js must ask the engine about the request before deciding what reaches the constructor",
  );
  assert.match(
    mainSource, /peaksRefused \? peaksRequested/,
    "window.__wb.peaks.chosen must fall back to the requested block when it was refused",
  );
});
