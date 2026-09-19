// Node-native tests for the viewer's carve channel (plan 2b, Task 6): `water-params.js`,
// `carve-session.js`, `engine.js`'s three water wrappers and the 13-word bake buffer -- held against
// the shipped `.wasm`, not against a model of it.
//
// What is asserted, and why each is here:
//   - `WATER_FIELDS` is pinned against `WaterParams`'s DECLARED order in `water/layer.rs`, not
//     against itself (the islands panel's first field-order test iterated one array both ways and
//     could never fail);
//   - each of the four refusals the carve can meet reaches the owner BY NAME, through the same
//     `CarveSession.carve` and `carveOutcomeText` the studio calls (the islands panel's refusal sat
//     in a path a failed boot swallowed);
//   - an untouched panel writes no water parameter, and the absent path is the untouched world;
//   - the 13-word buffer carries the flag, and the flag is what makes a record carve;
//   - a bank-width change never bakes and never calls the (world-building) checker;
//   - the owner is told what the carve does to their ponds, from the two actual records.
//
// Population/method/host, named once so every number below can be traced:
//   - World: DEFAULT_WORLD, Surface(20260904, 6_371_000 m, 12 plates, land 0.29), the fixture every
//     other channel test here holds against the same artifact.
//   - Bake: `hydro.test.mjs`'s small params at 30,000 nodes. Chosen by a scratch survey, not
//     picked: at 12,000 nodes this world keeps no pond at all, so a pond account would be 0 of 0;
//     at 30,000 the ordinary bake keeps 7 fine-found ponds and the bake for carving drains some of
//     them, which is the case the account exists for. About 1.2 s a bake on the developer machine.
//   - Host: node, this repository's checked-in viewer/public/wasm/worldbuilder_engine.wasm.

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import {
  Engine, WB_OK, WB_ERR_HANDLE, WB_ERR_PARAM, WB_ERR_WRONG_WORLD, WB_ERR_NOT_BAKED_FOR_CARVING,
  WB_ERR_CARVED, WB_HYDRO_PARAMS_STRIDE, WB_HYDRO_PARAMS_CARVE_STRIDE, statusName,
} from "../public/app/engine.js";
import { panelFieldFaults } from "../public/app/panel-fields.js";
import {
  WATER_FIELDS, WATER_STRIDE, WATER_PARAM_NAMES, CARVE_PARAM, BANK_WIDTHS_CEILING,
  BANK_STEPS_PER_WIDTH, waterTravel, waterPanelFields, waterToRecord, waterFromRecord,
  waterFromParams, waterToParams, hydroParamsWords, carveRefusal, pondChange, pondChangeText,
} from "../public/app/water-params.js";
import { CarveSession, carveOutcomeText, previewRecord } from "../public/app/carve-session.js";
import { decodeHydro } from "../public/app/water-preview.js";
import { nextQueryString, swapPlan } from "../public/app/live-swap.js";

const DEFAULT_WORLD = { seed: 20260904, radiusM: 6371000, plateCount: 12, landFraction: 0.29 };
/// Another world of the same radius: the same planet with more land. Its ground differs, so a
/// record baked on one is foreign to the other.
const OTHER_WORLD = { ...DEFAULT_WORLD, landFraction: 0.35 };

const BAKE = {
  totalNodes: 30000, wetnessNodes: 500, keepDepthM: 8, keepAreaM2: 1e6, pondMaxAreaM2: 1e6,
  streamFlowM2: 3e10, riverFlowM2: 3e11, greatFlowM2: 3e12, notchFallM: 1,
  evaporationFactor: 1, saltFlatShare: 0.1, forcedOutlets: [],
};

const PROBES = [[10, 20], [-35, 140], [45, -100], [0, 0], [60, 60], [-10, -60]];

const appFile = (name) =>
  readFileSync(fileURLToPath(new URL(`../public/app/${name}`, import.meta.url)), "utf8");
const engineSource = (path) =>
  readFileSync(
    fileURLToPath(new URL(`../../crates/worldbuilder-engine/src/${path}`, import.meta.url)), "utf8");

/// `WaterParams`'s field names in declaration order, read out of a Rust source text.
function declaredFields(source) {
  const match = source.match(/pub struct WaterParams \{([\s\S]*?)\n\}/);
  assert.ok(match, "WaterParams struct not found");
  return [...match[1].matchAll(/^\s*pub (\w+):/gm)].map((m) => m[1]);
}

async function loadEngine() {
  const wasmPath = fileURLToPath(new URL("../public/wasm/worldbuilder_engine.wasm", import.meta.url));
  const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath), {});
  return new Engine(instance);
}

/// An engine whose `wb_hydro_bake` and `wb_water_check` calls are counted -- the two exports that
/// cost a bake and a world build respectively, and so the two a slider drag must never reach.
async function countingEngine() {
  const engine = await loadEngine();
  const calls = { bake: 0, check: 0 };
  const real = engine.exports;
  engine.exports = {
    ...real,
    wb_hydro_bake: (...args) => { calls.bake += 1; return real.wb_hydro_bake(...args); },
    wb_water_check: (...args) => { calls.check += 1; return real.wb_water_check(...args); },
  };
  return { engine, calls };
}

let engine;
let canonical;

test.before(async () => {
  engine = await loadEngine();
  canonical = engine.waterPreset("canonical");
});

// ---------------------------------------------------------------------------- the field order

test("the shipped artifact exports the carve channel", () => {
  for (const name of ["wb_world_new_water", "wb_water_preset", "wb_water_check"]) {
    assert.equal(typeof engine.exports[name], "function", `${name} is missing from the artifact`);
  }
  assert.deepEqual(Object.keys(canonical), WATER_FIELDS);
});

test("WATER_FIELDS is pinned against WaterParams's own declared order in water/layer.rs", () => {
  // **Against the struct, not against itself.** `waterToRecord` and `waterFromRecord` iterate the
  // same array, so a round trip can never see an order that is wrong in both directions. This
  // reads the Rust declaration and the wasm stride and holds the JS to both.
  const declared = declaredFields(engineSource("water/layer.rs"));
  assert.deepEqual(
    WATER_FIELDS, declared,
    `WATER_FIELDS has drifted from WaterParams in water/layer.rs (Rust says: ${declared.join(", ")}; `
    + `JS says: ${WATER_FIELDS.join(", ")})`,
  );
  const stride = engineSource("wasm.rs").match(/pub const WB_WATER_BLOCK_STRIDE: usize = (\d+);/);
  assert.ok(stride, "WB_WATER_BLOCK_STRIDE not found in wasm.rs");
  assert.equal(WATER_STRIDE, Number(stride[1]));
  assert.equal(WATER_FIELDS.length, WATER_STRIDE);
  // **The pin is live for the case it exists for -- a second field -- even though the struct has
  // one today.** The same parser, over the struct with a second field declared, must name a
  // mismatch; a parser that only ever found the first field would pass the assertion above forever.
  const widened = "pub struct WaterParams {\n    pub bank_widths: f64,\n    pub bed_roughness: f64,\n}";
  assert.deepEqual(declaredFields(widened), ["bank_widths", "bed_roughness"]);
  assert.notDeepEqual(WATER_FIELDS, declaredFields(widened));
  // And the record lays each named field at its declared index -- a distinct sentinel, checked
  // against an index written here rather than read from the array under test.
  const record = waterToRecord({ bank_widths: 2.5 });
  assert.deepEqual(record, [2.5]);
  assert.deepEqual(waterFromRecord(record), { bank_widths: 2.5 });
});

// ------------------------------------------------------------------- the preset and the domain

test("the preset is the engine's own, and no bank width is written down in the viewer", () => {
  const constant = engineSource("water/layer.rs").match(/pub const CANONICAL_BANK_WIDTHS: f64 = ([\d.]+);/);
  assert.ok(constant, "CANONICAL_BANK_WIDTHS not found in water/layer.rs");
  assert.equal(canonical.bank_widths, Number(constant[1]),
    "wb_water_preset(canonical) has drifted from CANONICAL_BANK_WIDTHS");
  // **Comments included**, this time: the islands slice's stale density lived in a comment. The
  // canonical width is a small integer, which a grep cannot hunt in isolation, so what is asked is
  // the shape a transcription takes -- a numeral assigned to, or said of, the bank width.
  const transcription =
    /bank_widths\s*[:=]\s*\d|bank[ _]?widths?[^\n\d]{0,12}\b(is|of|=|:)\s*\d|\d(\.\d+)?\s*(channel )?widths? either side/i;
  for (const name of ["water-params.js", "carve-session.js", "controls.js", "main.js", "engine.js"]) {
    const hit = appFile(name).match(transcription);
    assert.equal(hit, null, `${name} writes a bank width down: "${hit && hit[0]}"`);
  }
  for (const fn of [waterTravel, waterPanelFields, waterFromParams, waterToParams]) {
    assert.match(String(fn), /canonical/, `${fn.name} must be anchored on the engine's canonical`);
  }
  assert.match(appFile("main.js"), /engine\.waterPreset\("canonical"\)/);
});

test("the ceiling is layer.rs's, and the engine admits it and refuses one double past it", () => {
  const constant = engineSource("water/layer.rs").match(/pub const MAX_BANK_WIDTHS: f64 = ([\d.]+);/);
  assert.ok(constant, "MAX_BANK_WIDTHS not found in water/layer.rs");
  assert.equal(BANK_WIDTHS_CEILING, Number(constant[1]));
  // `bake: null` with a block: a refused block is `WB_ERR_PARAM` before any world is built; an
  // admitted one gets as far as the missing bake, `WB_ERR_HANDLE`. So HANDLE here means "admitted".
  const ask = (bank_widths) => engine.checkWater({ ...DEFAULT_WORLD, water: { bank_widths }, bake: null });
  assert.equal(ask(BANK_WIDTHS_CEILING), WB_ERR_HANDLE);
  const above = new Float64Array(1);
  new DataView(above.buffer).setBigUint64(0, new DataView(new Float64Array([BANK_WIDTHS_CEILING]).buffer).getBigUint64(0, true) + 1n, true);
  assert.ok(above[0] > BANK_WIDTHS_CEILING);
  assert.equal(ask(above[0]), WB_ERR_PARAM, "one ULP past the ceiling was admitted");
  for (const bad of [0, -1, NaN, Infinity]) assert.equal(ask(bad), WB_ERR_PARAM, `${bad} was admitted`);
});

test("every position the bank slider can take is a block the engine admits", () => {
  const travel = waterTravel(canonical).bank_widths;
  const values = [];
  for (let p = travel.min; p <= travel.max; p += 1) {
    const bank_widths = travel.toValue(p);
    assert.equal(
      engine.checkWater({ ...DEFAULT_WORLD, water: { bank_widths }, bake: null }), WB_ERR_HANDLE,
      `position ${p} (${bank_widths}) asks for a block the engine refuses`);
    values.push(bank_widths);
  }
  assert.equal(values.length, BANK_WIDTHS_CEILING * BANK_STEPS_PER_WIDTH);
  assert.equal(values[values.length - 1], BANK_WIDTHS_CEILING);
  // The engine's canonical width is ON the lattice, and the slider's start is exactly it.
  assert.ok(Object.is(travel.toValue(travel.canonicalPosition), canonical.bank_widths));
  assert.deepEqual(panelFieldFaults(waterPanelFields(canonical)), []);
  const broken = waterPanelFields(canonical).map((f) => ({ ...f, value: f.value + 0.037 }));
  assert.ok(panelFieldFaults(broken).length > 0, "panelFieldFaults cannot see a mis-stepped bank row");
});

// -------------------------------------------------------------------- the untouched panel

test("an untouched panel writes no water parameter at all", () => {
  // Carve off is `null`, and `null` is every water field `null` -- which `nextQueryString` and
  // `apply` both delete, so the reload takes the absent path.
  const untouched = waterToParams(null, canonical);
  assert.deepEqual(Object.keys(untouched).sort(), [CARVE_PARAM, ...Object.values(WATER_PARAM_NAMES)].sort());
  for (const [key, value] of Object.entries(untouched)) {
    assert.equal(value, null, `${key} was written by an untouched panel`);
  }
  // Through the query-string writer the live swap uses: a page with no water parameter keeps
  // none, and a page's other parameters are left exactly alone.
  assert.equal(nextQueryString("seed=7&peak=0.2", untouched, {}), "seed=7&peak=0.2");
  // Read back: absent is null, and a bank width with no carve is still null -- it asks for nothing.
  assert.equal(waterFromParams(new URLSearchParams(""), canonical), null);
  assert.equal(waterFromParams(new URLSearchParams("bankWidths=2"), canonical), null);
  assert.equal(waterFromParams(new URLSearchParams("carve=0"), canonical), null);
  // Turned on at canonical, only the switch is written, and it reads back as the canonical block.
  const on = waterToParams({ ...canonical }, canonical);
  assert.deepEqual(Object.entries(on).filter(([, v]) => v !== null), [[CARVE_PARAM, "1"]]);
  assert.deepEqual(waterFromParams(new URLSearchParams(nextQueryString("", on, {})), canonical), canonical);
  // Moved, the width travels and comes back.
  const moved = { bank_widths: waterTravel(canonical).bank_widths.toValue(25) };
  const query = new URLSearchParams(nextQueryString("", waterToParams(moved, canonical), {}));
  assert.deepEqual(waterFromParams(query, canonical), moved);
  // Both writers the panel has -- the live swap's fields and generate's -- carry the carve through
  // `waterToParams`, and nothing else in the panel writes a water parameter.
  const controls = appFile("controls.js");
  assert.equal((controls.match(/waterToParams\(carveState, waterCanonical\)/g) || []).length, 2);
  assert.doesNotMatch(controls, /\bbankWidths\b|["']carve["']\s*:/);
});

test("the absent path through the widened door is the untouched world, bit for bit", () => {
  // `newWorld` now calls `wb_world_new_water` for every path. With no block and no bake it must
  // build exactly what `wb_world_new_peak` built -- the saved world stays the saved world.
  const viaDoor = engine.newWorld({ ...DEFAULT_WORLD });
  const viaPeak = engine.exports.wb_world_new_peak(
    BigInt(DEFAULT_WORLD.seed), DEFAULT_WORLD.radiusM, DEFAULT_WORLD.plateCount,
    DEFAULT_WORLD.landFraction, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0) >>> 0;
  assert.ok(viaPeak !== 0);
  for (const [lat, lon] of PROBES) {
    for (const resolution of [250, 5000, -1]) {
      assert.ok(Object.is(
        engine.elevationM(viaDoor, lat, lon, resolution), engine.elevationM(viaPeak, lat, lon, resolution)),
        `the absent path moved ${lat},${lon} at ${resolution}`);
    }
  }
  // And a bare world still bakes -- the door did not build a carved world by accident.
  assert.equal(engine.hydroSummary(engine.hydroBake({ handle: viaDoor, params: { ...BAKE, totalNodes: 12000 } })).schema, 7);
  for (const h of [viaDoor, viaPeak]) assert.equal(engine.freeWorld(h), WB_OK);
});

// ---------------------------------------------------------------------- the 13-word buffer

test("the 13-word buffer carries the flag, and the flag is what makes a record carve", () => {
  const forced = [{ latitudeDeg: 12.5, longitudeDeg: -40.25 }, { latitudeDeg: -3, longitudeDeg: 7 }];
  const ordinary = hydroParamsWords({ ...BAKE, forcedOutlets: forced });
  const carving = hydroParamsWords({ ...BAKE, forcedOutlets: forced, forCarving: true });
  assert.equal(WB_HYDRO_PARAMS_STRIDE, 12);
  assert.equal(WB_HYDRO_PARAMS_CARVE_STRIDE, 13);
  // The ordinary layout is untouched: twelve words, then the pairs from word 12.
  assert.equal(ordinary.length, 12 + 2 * forced.length);
  assert.deepEqual(ordinary.slice(12), [12.5, -40.25, -3, 7]);
  // The carving layout: the same twelve, word 12 exactly 1, the pairs from word 13 -- an ODD length,
  // which is how the engine tells the two apart.
  assert.equal(carving.length, 13 + 2 * forced.length);
  assert.equal(carving.length % 2, 1);
  assert.deepEqual(carving.slice(0, 12), ordinary.slice(0, 12));
  assert.ok(Object.is(carving[12], 1), `word 12 is ${carving[12]}, not the flag`);
  assert.deepEqual(carving.slice(13), [12.5, -40.25, -3, 7]);
  // Both layouts are what `wasm.rs` declares.
  const rust = engineSource("wasm.rs");
  assert.match(rust, /pub const WB_HYDRO_PARAMS_STRIDE: usize = 12;/);
  assert.match(rust, /pub const WB_HYDRO_PARAMS_CARVE_STRIDE: usize = WB_HYDRO_PARAMS_STRIDE \+ 1;/);

  // Through the engine: the flag makes a SCHEMA 8 record, and only that record carves.
  const bare = engine.newWorld({ ...DEFAULT_WORLD });
  const forCarving = engine.hydroHold({ handle: bare, params: { ...BAKE, forCarving: true } });
  const plain = engine.hydroHold({ handle: bare, params: BAKE });
  assert.equal(engine.hydroSummary(forCarving.words).schema, 8);
  assert.equal(engine.hydroSummary(forCarving.words).drainedForCarve, true);
  assert.equal(engine.hydroSummary(plain.words).schema, 7);
  const carved = engine.newWorld({ ...DEFAULT_WORLD, water: canonical, bake: forCarving });
  assert.throws(
    () => engine.newWorld({ ...DEFAULT_WORLD, water: canonical, bake: plain }),
    (error) => error.status === WB_ERR_NOT_BAKED_FOR_CARVING
      && /water=WB_ERR_NOT_BAKED_FOR_CARVING/.test(error.message),
  );
  // And the carve is real: the carved world is lower than the bare one somewhere on a reach.
  const decoded = decodeHydro(forCarving.words);
  let lowered = 0;
  for (const reach of decoded.reaches) {
    for (const point of reach.points) {
      if (engine.elevationM(carved, point.lat, point.lon) < engine.elevationM(bare, point.lat, point.lon)) lowered += 1;
    }
  }
  assert.ok(lowered > 0, "the carved world was not cut anywhere along its own record's reaches");
  engine.hydroFree(forCarving);
  engine.hydroFree(plain);
  for (const h of [bare, carved]) assert.equal(engine.freeWorld(h), WB_OK);
});

// ---------------------------------------------------------------- the four refusals, by name

/// A session over `engine`, with a `build` that keeps the carved handle so a test can free it.
function session(over, { held = null } = {}) {
  const waits = [];
  const carve = new CarveSession(over, {
    params: BAKE,
    wait: async (message) => { waits.push(message); },
  });
  if (held) carve.held = held;
  const worlds = [];
  const build = (carvedSpec) => { worlds.push(over.newWorld(carvedSpec)); };
  const free = () => {
    for (const h of worlds.splice(0)) over.freeWorld(h);
    carve.release();
  };
  return { carve, build, worlds, waits, free };
}

/// The owner-facing text for an outcome must carry the refusal's own name, and the name must be the
/// engine's own `statusName` for that status -- so the panel says what the engine said.
function assertNamed(outcome, status) {
  assert.equal(outcome.carved, false, "a refused carve was reported as carved");
  assert.ok(outcome.refusal, "the refusal was not surfaced at all");
  assert.equal(outcome.refusal.status, status);
  assert.equal(outcome.refusal.name, statusName(status));
  assert.ok(carveOutcomeText(outcome).includes(statusName(status)),
    `the owner's text does not name ${statusName(status)}: "${carveOutcomeText(outcome)}"`);
  assert.doesNotMatch(carveOutcomeText(outcome), /unavailable/);
}

test("a malformed block reaches the owner as WB_ERR_PARAM, before a minute's bake is spent on it", async () => {
  for (const query of ["carve=1&bankWidths=0", `carve=1&bankWidths=${BANK_WIDTHS_CEILING * 3}`, "carve=1&bankWidths=-2"]) {
    const water = waterFromParams(new URLSearchParams(query), canonical);
    assert.ok(water !== null, `${query} must be forwarded to the engine, not dropped`);
    const bare = engine.newWorld({ ...DEFAULT_WORLD });
    const s = session(engine);
    const outcome = await s.carve.carve({ spec: { ...DEFAULT_WORLD }, water, bare, build: s.build });
    assertNamed(outcome, WB_ERR_PARAM);
    assert.equal(s.carve.bakes, 0, `${query}: a bake was spent on a block the engine refuses`);
    assert.equal(s.worlds.length, 0);
    s.free();
    engine.freeWorld(bare);
  }
});

test("a record from another world reaches the owner as WB_ERR_WRONG_WORLD, and is re-baked", async () => {
  // The studio's own path to it: the carve is on, and the ground changes under the held bake (a
  // mountain slider, a land fraction). The session tries the held bake, the engine names it, and
  // the session says so and bakes again rather than drawing the old ground's channels.
  const bareA = engine.newWorld({ ...DEFAULT_WORLD });
  const s = session(engine);
  const first = await s.carve.carve({ spec: { ...DEFAULT_WORLD }, water: canonical, bare: bareA, build: s.build });
  assert.equal(first.carved, true);
  assert.equal(s.carve.bakes, 1);
  const bareB = engine.newWorld({ ...OTHER_WORLD });
  const second = await s.carve.carve({ spec: { ...OTHER_WORLD }, water: canonical, bare: bareB, build: s.build });
  assert.equal(second.carved, true);
  assert.deepEqual(second.notes.map((n) => n.name), ["WB_ERR_WRONG_WORLD"]);
  assert.ok(carveOutcomeText(second).includes("WB_ERR_WRONG_WORLD"),
    "the re-bake happened with no word to the owner about why");
  assert.equal(s.carve.bakes, 2);
  // And the refusal itself, as the engine names it on the join: the held bake (now OTHER_WORLD's)
  // handed back to DEFAULT_WORLD. `newWorld` asks `wb_water_check` after the handle of 0 and
  // carries the status, which is what the session's catch reads.
  assert.throws(
    () => engine.newWorld({ ...DEFAULT_WORLD, water: canonical, bake: s.carve.held }),
    (error) => error.status === WB_ERR_WRONG_WORLD && /water=WB_ERR_WRONG_WORLD/.test(error.message),
  );
  s.free();
  for (const h of [bareA, bareB]) engine.freeWorld(h);
});

test("a record not baked for carving reaches the owner as WB_ERR_NOT_BAKED_FOR_CARVING", async () => {
  const bare = engine.newWorld({ ...DEFAULT_WORLD });
  const ordinary = engine.hydroHold({ handle: bare, params: BAKE });
  const s = session(engine, { held: ordinary });
  const outcome = await s.carve.carve({ spec: { ...DEFAULT_WORLD }, water: canonical, bare, build: s.build });
  assertNamed(outcome, WB_ERR_NOT_BAKED_FOR_CARVING);
  s.free();
  engine.freeWorld(bare);
});

test("a bake asked of a carved world reaches the owner as WB_ERR_CARVED", async () => {
  // The studio hands the session the BARE world; this is the bug that would hand it the carved one.
  const bare = engine.newWorld({ ...DEFAULT_WORLD });
  const s = session(engine);
  assert.equal((await s.carve.carve({ spec: { ...DEFAULT_WORLD }, water: canonical, bare, build: s.build })).carved, true);
  const carvedHandle = s.worlds[s.worlds.length - 1];
  const again = session(engine);
  const outcome = await again.carve.carve({ spec: { ...DEFAULT_WORLD }, water: canonical, bare: carvedHandle, build: again.build });
  assertNamed(outcome, WB_ERR_CARVED);
  // And the other two bake-like exports the studio calls answer the same status, carried as
  // `.status` so the water preview and the water solve can name it too.
  for (const run of [
    () => engine.hydroBake({ handle: carvedHandle, params: BAKE }),
    () => engine.waterRun({ handle: carvedHandle, nodeCount: 8000 }),
  ]) {
    assert.throws(run, (error) => error.status === WB_ERR_CARVED);
  }
  assert.ok(carveRefusal(WB_ERR_CARVED).text.startsWith("WB_ERR_CARVED: "));
  // Wired where the owner would meet it: the water preview names a refusal through `carveRefusal`
  // and bakes the BARE world, and the main-thread water solve and climate read the bare world.
  const panel = appFile("world-panel.js");
  assert.match(panel, /carveRefusal\(error\.status\)\.text/);
  assert.match(panel, /handle: wb\.bareWorld \?\? wb\.world/);
  const main = appFile("main.js");
  assert.match(main, /engine\.waterRun\(\{ handle: worldSwapper\.handle/);
  assert.match(main, /engine\.climateCalibration\(\{ handle: worldSwapper\.handle/);
  assert.match(main, /bare: worldSwapper\.handle,/);
  s.free();
  again.free();
  engine.freeWorld(bare);
});

test("every refusal the carve can meet has its own name and its own sentence", () => {
  const statuses = [WB_ERR_PARAM, WB_ERR_WRONG_WORLD, WB_ERR_NOT_BAKED_FOR_CARVING, WB_ERR_CARVED, WB_ERR_HANDLE];
  const texts = new Set();
  for (const status of statuses) {
    const refusal = carveRefusal(status);
    assert.equal(refusal.name, statusName(status));
    assert.ok(refusal.text.startsWith(`${statusName(status)}: `));
    texts.add(refusal.text);
  }
  assert.equal(texts.size, statuses.length, "two refusals share a sentence");
  // The studio never catches an engine refusal into a generic message: the session returns it,
  // `main.js` publishes it, and the panel prints `carveOutcomeText` of it.
  const main = appFile("main.js");
  assert.match(main, /get last\(\) \{ return installed\.carve; \}/);
  assert.match(main, /if \(rebuildCarve\) \{\s*installed\.carve = await installCarve\(nextState\);/);
  // A carve-only commit rebuilds no ground, so it announces itself: the water preview drops a
  // picture drawn against the other record (Ruling C-27(b)).
  assert.match(main, /installCarve\(nextState\);\s*window\.dispatchEvent\(new CustomEvent\("wb-carve-changed"/);
  assert.match(appFile("world-panel.js"), /addEventListener\("wb-carve-changed", \(\) => dropPreview\(/);
  assert.match(appFile("controls.js"), /carveNote\.textContent = carveOutcomeText\(last\);/);
});

// ------------------------------------------------------- the slider path, and the pond account

test("a bank-width change never bakes and never calls the world-building checker", async () => {
  const { engine: counted, calls } = await countingEngine();
  const water = counted.waterPreset("canonical");
  const bare = counted.newWorld({ ...DEFAULT_WORLD });
  const s = session(counted);
  const first = await s.carve.carve({ spec: { ...DEFAULT_WORLD }, water, bare, build: s.build });
  assert.equal(first.carved, true);
  // Turning it on: one bake for carving, one ordinary bake for the pond account, and ONE check --
  // before the bake, so a malformed block could not waste it.
  assert.deepEqual(calls, { bake: 2, check: 1 });
  assert.equal(s.waits.length, 1, "the wait was not announced before the bake");
  const travel = waterTravel(water).bank_widths;
  const handles = [];
  for (const position of [travel.min, 17, travel.max, travel.canonicalPosition]) {
    const outcome = await s.carve.carve({
      spec: { ...DEFAULT_WORLD }, water: { bank_widths: travel.toValue(position) }, bare, build: s.build,
    });
    assert.equal(outcome.carved, true);
    assert.equal(outcome.baked, false);
    handles.push(s.worlds[s.worlds.length - 1]);
  }
  assert.deepEqual(calls, { bake: 2, check: 1 }, "a bank-width change baked or checked");
  assert.equal(s.waits.length, 1);
  // And the width is not ignored: the narrowest and widest banks cut different ground somewhere
  // beside a reach.
  const decoded = decodeHydro(s.carve.held.words);
  let differs = 0;
  for (const reach of decoded.reaches) {
    for (const p of reach.points) {
      const off = p.lat + 0.02;
      if (counted.elevationM(handles[0], off, p.lon) !== counted.elevationM(handles[2], off, p.lon)) differs += 1;
    }
  }
  assert.ok(differs > 0, "the narrowest and widest banks built the same ground");
  // The live swap agrees: a carve-only change rebuilds no worker world and solves no water.
  const state = { spec: { ...DEFAULT_WORLD }, waterNodes: 8000, waterEnabled: true, carve: water };
  const plan = swapPlan(state, { ...state, carve: { bank_widths: travel.toValue(17) } });
  assert.equal(plan.kind, "carve");
  assert.equal(plan.rebuildWorld, false);
  assert.equal(plan.resolveWater, false);
  assert.equal(plan.rebuildCarve, true);
  assert.equal(plan.rebuildTerrain, true);
  s.free();
  counted.freeWorld(bare);
});

test("turning the carve on tells the owner what it does to their ponds, from the two records", async () => {
  const bare = engine.newWorld({ ...DEFAULT_WORLD });
  const s = session(engine);
  const outcome = await s.carve.carve({ spec: { ...DEFAULT_WORLD }, water: canonical, bare, build: s.build });
  assert.equal(outcome.carved, true);
  const ponds = outcome.ponds;
  // Recomputed here from two fresh bakes, not read back from the session.
  const ordinary = decodeHydro(engine.hydroBake({ handle: bare, params: BAKE }));
  const carving = decodeHydro(s.carve.held.words);
  assert.deepEqual(ponds, pondChange(ordinary, carving));
  assert.equal(ponds.before, ordinary.header.pondsKept);
  assert.equal(ponds.after, carving.header.pondsKept);
  assert.equal(ponds.before - ponds.drained + ponds.arrived, ponds.after);
  assert.equal(ponds.kept + ponds.drained, ponds.before);
  // On this fixture the carve really drains ponds -- the case the sentence exists for.
  assert.ok(ponds.drained > 0, "the fixture drains no pond, so the account is untested");
  // Every drained pond is really gone from the carving record, and every kept one really there.
  const keyOf = (b) => `${b.anchor[0]},${b.anchor[1]},${b.levelM}`;
  const carvingPonds = new Set(carving.bodies.slice(carving.bodies.length - carving.header.pondsKept).map(keyOf));
  const ordinaryPonds = ordinary.bodies.slice(ordinary.bodies.length - ordinary.header.pondsKept);
  assert.equal(ordinaryPonds.filter((b) => !carvingPonds.has(keyOf(b))).length, ponds.drained);
  // The owner is told, with those numbers.
  const text = carveOutcomeText(outcome);
  assert.ok(text.includes(`${ponds.drained} of ${ponds.before} are drained`), text);
  assert.ok(text.includes(`${ponds.arrived} others`), text);
  assert.equal(pondChangeText({ before: 5, after: 5, drained: 0, arrived: 0, kept: 5 }),
    "carving changes no pond: all 5 are kept where they were.");
  s.free();
  engine.freeWorld(bare);
});

// Ruling C-27(b): while the carve is on, the water preview draws the record the carve cut from.
// Baking the bare world instead draws the ORDINARY record, which still holds the ponds a
// channel drains -- so the preview would show water the carved world does not have.
test("the water preview draws the carving record while the carve is on, and bakes the bare world otherwise", () => {
  const carving = new Float64Array([8, 1, 2, 3]);
  const session = { held: { id: 1, handle: 7, words: carving } };
  assert.equal(previewRecord({ carved: true }, session), carving, "carved: the held record, the very one");
  assert.equal(previewRecord({ carved: false }, session), null, "carve off: bake the bare world");
  assert.equal(previewRecord({ carved: false, refusal: { status: 9 } }, session), null, "refused: bake the bare world");
  assert.equal(previewRecord(null, session), null, "no outcome yet");
  assert.equal(previewRecord({ carved: true }, { held: null }), null, "carved but nothing held");
  assert.equal(previewRecord({ carved: true }, null), null, "no session");
});
