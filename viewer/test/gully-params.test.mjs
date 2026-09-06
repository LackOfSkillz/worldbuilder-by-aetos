// Node-native tests for the viewer's gully channel: `gully-params.js`, `engine.js`'s two new
// marshalling methods and its widened constructor, and the properties that are easy to claim and
// easy to get wrong -- that the untouched path is still the untouched world, that no gully number
// (above all the measured slope reference) is written down twice, that every position the one
// slider can take is a block the shipped artifact accepts, and **that the layer actually changes
// pixels**.
//
// No framework: `node:test` + `node:assert/strict`, run with `npm test` from `viewer/`.
// Nothing here touches Cesium or the DOM.
//
// Population/method/host, named once so every number below can be traced:
//   - World: Surface::new(20260904, 6_371_000, 12, 0.29, ..) -- DEFAULT_WORLD, the fixture whose
//     elevation at lat 12 lon 34 was witnessed three independent ways.
//   - Host: node 22 on Windows 11, this repository's checked-in
//     viewer/public/wasm/worldbuilder_engine.wasm, loaded directly from disk (no fetch).
//   - Probes: the eight FLANK witnesses plus three of the older channels' probes. The flank eight
//     are the same `GULLY_PROBES` list `crates/worldbuilder-engine/tests/wasm_exports.rs` uses,
//     and both sides say where it came from: a throwaway walked 400,000 spiral points on this
//     fixture and took the steepest high ground. **A probe set chosen for another channel is blind
//     here** -- the term is gated on elevation above 200 m and steered on slope, so a scatter over
//     a sphere that is 71% ocean mostly lands where the gate is shut, and the three inherited
//     probes below are here to witness exactly that.
//   - Resolutions: 250 m (what the scalar exports and the parity corpus use), 76.35 m (exactly
//     `relief.js::marginedTileRequest`'s spacing at size 256) and 5,000 m (a coarse caller).
//   - Slider population: every integer position the one slider can take -- 11 -- each turned into
//     a gully block and put through `wb_gully_check`.

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { Engine, WB_OK, WB_ERR_PARAM } from "../public/app/engine.js";
import { panelFieldFaults } from "../public/app/panel-fields.js";
import { WORLD_FIELDS } from "../public/app/live-swap.js";
import {
  GULLY_CONTROLS,
  GULLY_SLIDERS,
  GULLY_FIELDS,
  GULLY_STRIDE,
  GULLY_PARAM_NAMES,
  MEASURED_CREST,
  MEASURED_SLOPE,
  MEASURED_RELIEF,
  CARVE_BAND,
  CREST_MIN_TWENTIETHS,
  CREST_MAX_TWENTIETHS,
  CREST_TWENTIETHS,
  gullyTravel,
  gullyPanelFields,
  gullyFromParams,
  gullyToParams,
  gullyToRecord,
  gullyFromRecord,
  gullyReadoutFields,
} from "../public/app/gully-params.js";

const DEFAULT_WORLD = { seed: 20260904, radiusM: 6371000, plateCount: 12, landFraction: 0.29 };

/// The eight flank witnesses, then three probes inherited from the older channels. The comment on
/// each flank row is the structural height and the 2 km slope the engine's own probe measured
/// there, quoted from `wasm_exports.rs` rather than re-derived.
const FLANK = [
  [-9.00, 65.25],  // structural 1,256 m, 2 km slope 0.01602
  [-8.75, 64.75],  // 1,260 m, 0.01555
  [-8.50, 64.50],  // 1,366 m, 0.01376
  [-8.75, 65.00],  // 1,482 m, 0.01356
  [-9.00, 65.75],  // 1,351 m, 0.01327
  [-8.75, 65.50],  // 1,685 m, 0.01143
  [-8.25, 64.50],  // 1,585 m, 0.00928
  [-9.25, 66.00],  // 1,059 m, 0.00915
];
/// Two of these are BELOW THE GATE and one is on ordinary land, which is why they are here: the
/// gate is what makes this a drainage texture rather than a planet-wide roughness, and a test
/// where everything moves cannot see a gate wired open.
const OFF_FLANK = [
  [0.0, 0.0],
  [12.0, 34.0],
  [-18.25, 121.5], // the harbour
];
const PROBES = [...FLANK, ...OFF_FLANK];

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
let drainage;
let travel;

test.before(async () => {
  engine = await loadEngine();
  canonical = engine.gullyPreset("canonical");
  drainage = engine.gullyPreset("drainage");
  travel = gullyTravel(canonical);
});

test("the shipped artifact exports the gully channel at all", () => {
  // The original failure mode of this project was a 327-byte module exporting only `memory` from a
  // build that exited 0. A missing `#[no_mangle]` is invisible in green output, so the three names
  // are asked for by name against the artifact this test just loaded.
  for (const name of ["wb_world_new_gully", "wb_gully_preset", "wb_gully_check"]) {
    assert.equal(typeof engine.exports[name], "function", `${name} is missing from the artifact`);
  }
});

test("the preset crosses as ten numbers and moves exactly one of them", () => {
  assert.equal(GULLY_FIELDS.length, GULLY_STRIDE);
  assert.deepEqual(Object.keys(canonical), GULLY_FIELDS);
  const moved = GULLY_FIELDS.filter((f) => !Object.is(canonical[f], drainage[f]));
  assert.deepEqual(moved, ["amplitudeM"], "drainage() must move the amplitude and nothing else");
  // Which is the fact the panel's design turns on: **the field that switches the kernel on is the
  // one with no slider**, so the drainage button has to write the whole block rather than move
  // widgets. A preset half-applied because nine of its fields had no widget is the
  // silently-dropping-builder shape.
  assert.equal(canonical.amplitudeM, 0);
  assert.ok(drainage.amplitudeM > 0);
  assert.ok(!GULLY_SLIDERS.includes("amplitudeM"));
  // And the record round-trips through the ABI order without drifting, which is the property a
  // second copy of a field order would break silently: there is no type error for a crest exponent
  // written into the amplitude slot.
  assert.deepEqual(gullyFromRecord(gullyToRecord(drainage)), drainage);
});

test("the untouched path is null, and null is the untouched world", () => {
  // RULING 1, on the viewer's side of the boundary, and it is STRUCTURAL on this channel rather
  // than arithmetic: `Surface::with_gully(None)` builds no steering lattice and `elevation_m` takes
  // a different branch, so `null` is the pre-gully surface rather than the gully one adding zero.
  assert.equal(gullyFromParams(new URLSearchParams(""), canonical), null);
  const restated = new URLSearchParams(
    GULLY_CONTROLS.map((f) => [GULLY_PARAM_NAMES[f], String(canonical[f])]),
  );
  assert.equal(gullyFromParams(restated, canonical), null);
  // A typo is ignored rather than forwarded: answering `?gully=banana` with a refused world would
  // turn a typo in a shared link into a blank page.
  assert.equal(gullyFromParams(new URLSearchParams("gully=banana"), canonical), null);
  // But a number OUT of domain is forwarded, because a caller asking for something the engine
  // declines is a different thing from a caller not asking for anything.
  const refused = gullyFromParams(new URLSearchParams("gullyCrest=-0.5"), canonical);
  assert.equal(refused.crestSharpness, -0.5);
  assert.equal(engine.checkGully(refused), WB_ERR_PARAM);

  // And the world itself: the default path is byte-for-byte the world with no gully argument, at
  // every probe and at every resolution. **This is the assertion the whole wiring rests on** --
  // `newWorld` now calls `wb_world_new_gully` for EVERY path, including the one that used to call
  // `wb_world_new_coast`, so if this were wrong today's picture would have moved.
  const plain = engine.newWorld({ ...DEFAULT_WORLD });
  const defaulted = engine.newWorld({ ...DEFAULT_WORLD, gully: null });
  const explicit = engine.newWorld({ ...DEFAULT_WORLD, gully: canonical });
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

test("the drainage preset actually changes the ground through the viewer's own constructor", () => {
  // **THE DISCRIMINATOR, and the reason this test file exists at all.** Every assertion above
  // would pass over a marshalling layer that allocated ten f64 and never handed the pointer to the
  // export -- which is exactly the shape a `gully ? GULLY_STRIDE : 0` length written the wrong way
  // round produces, and it is indistinguishable from a subtle kernel by eye. This project has
  // shipped a flat-grey relief raster that read as "subtle shading", three colour blends that were
  // never once selected, and nine palette colours that were unreachable.
  const plain = engine.newWorld({ ...DEFAULT_WORLD });
  const drained = engine.newWorld({ ...DEFAULT_WORLD, gully: drainage });

  // Measured on this host, at 250 m, over the eight flank witnesses: every one of the eight moves,
  // the largest by 51.41 m against a 60 m amplitude, the mean absolute displacement 32.3 m. The
  // assertion is stated well inside those figures so a re-measurement on another host is not a
  // failure, but it is far outside anything a no-op could produce: a constant term would print
  // zero at every probe.
  const flankDeltas = FLANK.map(([lat, lon]) =>
    engine.elevationM(drained, lat, lon, 250) - engine.elevationM(plain, lat, lon, 250));
  assert.equal(flankDeltas.filter((d) => d !== 0).length, FLANK.length,
    `the drainage preset left a flank probe untouched: ${flankDeltas.join(", ")}`);
  const maxAbs = Math.max(...flankDeltas.map(Math.abs));
  assert.ok(maxAbs > 30, `the largest flank displacement was only ${maxAbs.toFixed(2)} m`);
  // **It carves rather than blankets**, which is the sign of the mean and is the whole difference
  // between drainage and roughness. Measured -22.8 m over these eight; the kernel note's own
  // 16,000-sample population reads -10.83 m. The assertion is on the SIGN plus a floor well
  // inside both.
  const mean = flankDeltas.reduce((a, b) => a + b, 0) / flankDeltas.length;
  assert.ok(mean < -2, `the term blanketed rather than carved: mean ${mean.toFixed(2)} m`);
  // And it has both signs in it -- a term that only ever subtracted would be a lowering, not a
  // texture.
  assert.ok(flankDeltas.some((d) => d > 0), "no flank probe was raised; this is a lowering");

  // **The gate is shut off the flanks**, and that is what makes this drainage rather than a
  // planet-wide roughness. Two of the three inherited probes are below the 200 m gate and do not
  // move at all; this is the assertion a gate wired open would fail.
  const gated = OFF_FLANK.map(([lat, lon]) =>
    engine.elevationM(drained, lat, lon, 250) - engine.elevationM(plain, lat, lon, 250));
  assert.equal(gated.filter((d) => d === 0).length, 2,
    `the gate is not shut where it should be: ${gated.join(", ")}`);

  for (const h of [plain, drained]) assert.equal(engine.freeWorld(h), WB_OK);
});

test("the term fades out with the sampling, so a coarse caller pays nothing and sees nothing", () => {
  // **A finding the viewer has to know about, not a detail of the kernel.** The term is faded on
  // the same two-to-four-samples-per-wavelength rule the octaves use, so at 5,000 m it is gone --
  // exactly, at every probe. That is correct (a 1 km wavelength is not representable at 5 km
  // sampling) and it is also why an ORBITAL view of a gullied planet is the same picture as an
  // orbital view of a canonical one: the coarse quadtree levels never sample fine enough to see
  // it. Recorded as a test so nobody reports "the preset does nothing" from a whole-planet
  // screenshot.
  const plain = engine.newWorld({ ...DEFAULT_WORLD });
  const drained = engine.newWorld({ ...DEFAULT_WORLD, gully: drainage });
  for (const [lat, lon] of PROBES) {
    assert.equal(
      engine.elevationM(drained, lat, lon, 5000), engine.elevationM(plain, lat, lon, 5000),
      `the term survived 5 km sampling at ${lat},${lon}`);
  }
  // And it is emphatically alive at a level-12 tile's own spacing, or the fade would be a switch
  // that is off everywhere.
  const fine = FLANK.map(([lat, lon]) =>
    engine.elevationM(drained, lat, lon, 76.35) - engine.elevationM(plain, lat, lon, 76.35));
  assert.equal(fine.filter((d) => d !== 0).length, FLANK.length);
  assert.ok(Math.max(...fine.map(Math.abs)) > 30);
  for (const h of [plain, drained]) assert.equal(engine.freeWorld(h), WB_OK);
});

test("the one slider changes pixels once the kernel is on, and cannot while it is off", () => {
  // A slider that moves a readout and no ground is the failure this file is written against. Both
  // halves are asserted, because the second is the honest caveat the panel prints: canonical zeroes
  // the amplitude, so at canonical NO position of this slider changes anything at all.
  const plain = engine.newWorld({ ...DEFAULT_WORLD });
  const drained = engine.newWorld({ ...DEFAULT_WORLD, gully: drainage });
  const blanket = engine.newWorld({
    ...DEFAULT_WORLD, gully: { ...drainage, crestSharpness: CARVE_BAND.high },
  });
  const deltas = FLANK.map(([lat, lon]) =>
    engine.elevationM(blanket, lat, lon, 250) - engine.elevationM(drained, lat, lon, 250));
  assert.equal(deltas.filter((d) => d !== 0).length, FLANK.length,
    "moving the carve slider to the blanket end left the ground alone");
  // The direction is the sweep's own: raising the exponent towards one raises the mean, because
  // the kernel becomes symmetric and stops carving. Measured +10.49 m over these eight.
  const mean = deltas.reduce((a, b) => a + b, 0) / deltas.length;
  assert.ok(mean > 2, `the blanket end did not raise the mean: ${mean.toFixed(2)} m`);

  // And with the kernel off, the same slider position is bit-identical to canonical -- there is no
  // second copy of the off switch.
  const offSharp = engine.newWorld({
    ...DEFAULT_WORLD, gully: { ...canonical, crestSharpness: CARVE_BAND.high },
  });
  for (const [lat, lon] of FLANK) {
    assert.equal(
      engine.elevationM(offSharp, lat, lon, 250), engine.elevationM(plain, lat, lon, 250),
      `the slider moved the ground with the amplitude at zero at ${lat},${lon}`);
  }
  for (const h of [plain, drained, blanket, offSharp]) assert.equal(engine.freeWorld(h), WB_OK);
});

test("every position the carve slider can take is a block the engine accepts", () => {
  // **A control most of whose travel the engine refuses is worse than no control.** The tectonic
  // channel's version of this test is what caught `margin_warp_m` having exactly one live position,
  // and it is the reason that field has no widget.
  const positions = [];
  for (let p = travel.crestSharpness.min; p <= travel.crestSharpness.max; p += 1) {
    const value = travel.crestSharpness.toValue(p);
    for (const block of [{ ...canonical, crestSharpness: value }, { ...drainage, crestSharpness: value }]) {
      assert.equal(
        engine.checkGully(block), WB_OK, `position ${p} asks for a block the engine refuses`);
    }
    positions.push(value);
  }
  assert.equal(positions.length, CREST_MAX_TWENTIETHS - CREST_MIN_TWENTIETHS + 1);
  // **The canonical value is ON the lattice**, or an untouched panel writes a crest exponent into
  // every shared link and takes the reload off the engine's `None` path. It is not the slider's
  // minimum here -- it sits in the middle of the travel -- so this is a real question rather than
  // `min + 0 * step`.
  assert.ok(
    positions.some((v) => Object.is(v, canonical.crestSharpness)),
    "the engine's canonical crest exponent is not a value this slider can produce",
  );
  assert.ok(travel.crestSharpness.holdsCanonical);
  assert.equal(travel.crestSharpness.toPosition(canonical.crestSharpness), 14);
  // Every row of the measured sweep lands on the lattice exactly, which is what twentieths were
  // chosen for: a panel that could not stand on the value a table measured would be a panel whose
  // note described a setting it cannot reach.
  for (const r of MEASURED_CREST) {
    assert.ok(
      positions.some((v) => Object.is(v, r.crestSharpness)),
      `the sweep measured ${r.crestSharpness} and the slider cannot express it`);
  }
});

test("the slider travel is the band the sweep measured, not the domain the engine allows", () => {
  const values = [];
  for (let p = travel.crestSharpness.min; p <= travel.crestSharpness.max; p += 1) {
    values.push(travel.crestSharpness.toValue(p));
  }
  assert.ok(Math.abs(values[0] - CARVE_BAND.low) < 1e-12);
  assert.ok(Math.abs(values[values.length - 1] - CARVE_BAND.high) < 1e-12);
  // **All eleven positions are inside the measured band**, which is the coast slider's failure mode
  // avoided rather than merely named: there, fourteen of sixteen were inside and the two outside
  // were the dead zero and one below-visible step.
  const inside = values.filter((v) => v >= CARVE_BAND.low && v <= CARVE_BAND.high).length;
  assert.equal(inside, values.length);
  // And the engine would accept five orders of magnitude more than the panel offers, so the travel
  // is a CHOICE made from the sweep rather than the widest thing that happens to work.
  assert.equal(engine.checkGully({ ...drainage, crestSharpness: 0.001 }), WB_OK);
  assert.equal(engine.checkGully({ ...drainage, crestSharpness: 100 }), WB_OK);
  assert.equal(CREST_TWENTIETHS, 20);
});

test("the carve slider satisfies panelFieldFaults", () => {
  // The check that closed the family of four shipped defects, run over this channel's one range
  // control in the units the sweep was measured in. `position * 0.05` cannot express 0.85 and
  // could not express 0.35 either, which is how the fourth one arrived; this travel divides.
  assert.deepEqual(panelFieldFaults(gullyPanelFields(canonical)), []);
  // And it can still fail, over the same table with a deliberately mis-stepped row -- a check
  // nobody has seen fail is a check nobody knows the shape of. The fault is introduced in the
  // VALUE, which is the shape the defect actually took four times.
  const broken = gullyPanelFields(canonical).map((f) => ({ ...f, value: f.value + 0.017 }));
  assert.ok(panelFieldFaults(broken).length > 0, "panelFieldFaults cannot see a mis-stepped row");
  // Unlike the coast row, the STEP can be faulted here too, because canonical is not the minimum:
  // a step that cannot walk from 0.50 to 0.70 is visible to the same check.
  const misstepped = gullyPanelFields(canonical).map((f) => ({ ...f, step: 0.03 }));
  assert.ok(panelFieldFaults(misstepped).length > 0, "a mis-stepped travel is invisible here");
});

test("a shared link carries only what was moved, and round-trips", () => {
  const untouched = gullyToParams({ ...canonical }, canonical);
  assert.deepEqual(Object.values(untouched).filter((v) => v !== null), []);
  const chosen = gullyToParams(drainage, canonical);
  assert.equal(chosen[GULLY_PARAM_NAMES.amplitudeM], String(drainage.amplitudeM));
  for (const field of GULLY_CONTROLS) {
    if (field === "amplitudeM") continue;
    assert.equal(chosen[GULLY_PARAM_NAMES[field]], null, `${field} was written but never moved`);
  }
  // Round trip: what the panel writes is what the boot path reads back. This is the property that
  // makes "the picture a slider reaches equals the picture its URL loads" structural -- the live
  // swap reads its spec back out of the query string through this same reader.
  const params = new URLSearchParams(Object.entries(chosen).filter(([, v]) => v !== null));
  assert.deepEqual(gullyFromParams(params, canonical), drainage);
});

test("the crest floor closes an infinite-height hazard, and the engine is what says so", () => {
  // The edge shaping is `1 - 2 * folded^crest_sharpness` and `folded` is EXACTLY zero at a crest,
  // so at a non-positive exponent `0^s` is `+inf`: one negative f64 in word 5 turns every gully
  // crest in the world into an infinite height, crosses a nounwind boundary as an f64 that looks
  // like an f64, and lands in a vertex buffer. The panel asks the real validator rather than
  // re-deriving the bound, so what is asserted here is that the validator is a LIVE question.
  for (const crestSharpness of [0, -0.5, -1e-6, NaN, Infinity, 1e6]) {
    assert.equal(
      engine.checkGully({ ...drainage, crestSharpness }), WB_ERR_PARAM,
      `crest exponent ${crestSharpness} was admitted`);
  }
  // The two lattice lengths are divisors of the radius, and zero is an abort rather than a wrong
  // answer.
  for (const field of ["cellM", "steerLatticeM"]) {
    assert.equal(engine.checkGully({ ...drainage, [field]: 0 }), WB_ERR_PARAM);
    assert.equal(engine.checkGully({ ...drainage, [field]: -1 }), WB_ERR_PARAM);
  }
  assert.equal(engine.checkGully({ ...drainage, amplitudeM: -1 }), WB_ERR_PARAM);
  assert.equal(engine.checkGully({ ...drainage, flatEnergyFloor: 1.5 }), WB_ERR_PARAM);
  // And a world asking for one is refused with a message that names the channel rather than a blank
  // page with a handle of 0.
  assert.throws(
    () => engine.newWorld({ ...DEFAULT_WORLD, gully: { ...drainage, crestSharpness: -0.5 } }),
    /wb_world_new_gully refused.*gully=WB_ERR_PARAM/s,
  );
});

test("repeated gully swaps do not grow the engine's world count", () => {
  // The leak this whole live-slider design invites: a swap builds before it frees, so for one
  // instant there are two, and forgetting the free is invisible in the picture right up until the
  // allocation that fails. `live-swap.test.mjs` asserts this for the swapper; this asserts it for
  // the constructor path the gully block newly takes, because `newWorld` now allocates a tenth
  // buffer and frees it in a `finally`.
  const before = engine.worldCount();
  for (let i = 0; i < 12; i += 1) {
    const block = i % 2 === 0 ? drainage : canonical;
    const handle = engine.newWorld({ ...DEFAULT_WORLD, gully: block });
    assert.notEqual(handle, 0);
    assert.equal(engine.freeWorld(handle), WB_OK);
  }
  assert.equal(engine.worldCount(), before, "wb_world_count grew across repeated gully worlds");
});

test("the gully block is surface-class, so a swap rebuilds the world and re-solves the water", () => {
  // `live-swap.js`'s own measurement is that the stream graph samples the surface, so anything
  // that moves the ground moves the lake manifest. A term that carves tens of metres into a flank
  // and was NOT on this list would swap the terrain and draw the previous world's lakes on it.
  assert.ok(WORLD_FIELDS.includes("gully"));
});

test("no gully number is written down twice in the viewer", () => {
  // The hazard the whole design is arranged against, stated as a test rather than as a comment --
  // the same test the relief, tectonic and coast channels carry. Comments are stripped first:
  // prose is allowed to quote a measurement, code is not allowed to restate one.
  const strip = (source) => source
    .split("\n")
    .map((line) => line.replace(/^\s*\/\/.*$/, "").replace(/^\s*\/\/\/.*$/, ""))
    .join("\n");
  // **Only the DISTINCTIVE values can be asked this question, and saying so is part of the test.**
  // Six of the ten canonical fields are 0, 1000, 1, 4, 200 and 900, and none of those is a string a
  // source file can be asked not to contain. So the file-level half asks about the two values
  // nothing else in this viewer is -- **the measured slope reference above all**, which is the one
  // number in this record that is a measurement of this generator rather than a preference -- and
  // the rest are covered by the function-source half below, which asks the sharper question: not
  // "does this digit appear" but "is any gully value computed from a literal".
  const literals = [String(canonical.slopeReference), String(canonical.flatEnergyFloor)];
  assert.deepEqual(literals, ["0.005", "0.25"]);
  for (const name of ["controls.js", "main.js"]) {
    const code = strip(appFile(name));
    for (const literal of literals) {
      assert.ok(
        !code.includes(literal),
        `${name} restates the gully value ${literal}; it must read it from the engine`,
      );
    }
  }
  // `gully-params.js` is asked the SHARPER question rather than the same one, because it
  // legitimately contains "0.005": `MEASURED_SLOPE` is the probe's own table and it exists to say
  // what was measured and on what population. A table that could not name its own reference would
  // be a table about nothing.
  for (const fn of [gullyTravel, gullyPanelFields, gullyFromParams, gullyToParams]) {
    const source = strip(String(fn));
    for (const literal of literals) {
      assert.ok(
        !source.includes(literal),
        `${fn.name} computes with the gully literal ${literal} instead of its argument`,
      );
    }
  }
  for (const fn of [gullyTravel, gullyPanelFields, gullyFromParams, gullyToParams]) {
    assert.match(strip(String(fn)), /canonical/, `${fn.name} must be anchored on the engine's block`);
  }
  // And the panel must reach the engine for them, or the assertion above would pass on a viewer
  // that simply has no drainage section.
  assert.match(appFile("controls.js"), /from "\.\/gully-params\.js"/);
  assert.match(appFile("main.js"), /engine\.gullyPreset\("canonical"\)/);
  assert.match(appFile("main.js"), /engine\.gullyPreset\("drainage"\)/);
  assert.match(appFile("engine.js"), /wb_gully_preset/);
  // **And the constructor really is the widened one.** This is the line that makes the channel
  // reachable at all; a viewer that imported the module and still called `wb_world_new_coast`
  // would pass every other assertion in this file except the ones that build a world.
  assert.match(appFile("engine.js"), /wb_world_new_gully\(/);
});

test("every driven gully field appears somewhere the owner can see it", () => {
  // A preset that changed something the panel never mentioned would be a parameter the owner cannot
  // see -- and on this channel that would include the field that turns the whole term on. One field
  // has a slider; the other nine are printed. The union must be the whole driven set, so an
  // eleventh field cannot arrive silently.
  const shown = gullyReadoutFields();
  const union = [...GULLY_SLIDERS, ...shown];
  assert.deepEqual([...union].sort(), [...GULLY_CONTROLS].sort());
  // Disjoint as well as exhaustive: an inverted filter would still cover the set by counting the
  // slider's own field twice, and that is the mutation this line exists for.
  assert.equal(new Set(union).size, union.length, "a field is both a slider and a readout");
  assert.ok(shown.includes("amplitudeM"), "the amplitude is neither a slider nor a readout");
  assert.ok(shown.includes("slopeReference"), "the measured slope is nowhere the owner can see it");
  const controls = appFile("controls.js");
  assert.match(controls, /gullyReadoutFields\(\)/, "the readout must be driven by the field list");
  assert.match(controls, /gullyScheduleNote/);
  // The driven set is the whole ABI record, so a shared link can carry a whole preset.
  assert.deepEqual([...GULLY_CONTROLS].sort(), [...GULLY_FIELDS].sort());
});

test("the panel's not-wired list is honest about this channel and about the entry it corrected", () => {
  // Comments are NOT stripped here: the reason an entry came off is kept in the file, and that is
  // deliberate.
  const controls = appFile("controls.js");
  const entries = controls.slice(controls.indexOf("const NOT_WIRED = ["));
  const list = entries.slice(0, entries.indexOf("];"));
  // The gully channel is wired, and there is a section for it. It was never ON this list -- it
  // postdates the last edit to it, exactly as the coastline did -- so what is asserted is that it
  // did not quietly get added as unreachable while being reachable.
  assert.ok(!list.includes('["gully"'), "the drainage is listed as not wired");
  assert.ok(!list.includes('["drainage"'), "the drainage is listed as not wired");
  assert.match(controls, /drainage · live/);
  assert.match(appFile("main.js"), /engine\.gullyPreset\(/, "main.js must read the gully preset");
  // **The stale entry, corrected rather than deleted.** It said "climate + biomes ... not built",
  // and both halves were false: biomes are the DEFAULT LAND COLOUR on this page (`relief.js` takes
  // its land base from `biome.js`), and climate is built in the engine and simply has no export.
  // A list that is wrong about the project is worse than no list.
  assert.ok(!list.includes("climate + biomes"), "the conflated climate/biome entry is still here");
  assert.ok(!list.includes("biomes"), "the panel still claims biomes are not built");
  assert.match(appFile("relief.js"), /from "\.\/biome\.js"/, "biomes really are drawn");
  assert.ok(list.includes('["climate"'), "the honest climate entry is missing");
  assert.equal(typeof engine.exports.wb_climate_preset, "undefined",
    "climate now has an export and its entry needs rewriting again");
  // And the entries this task did NOT touch are still there, because none of them became true.
  for (const name of ['["erosion"', '["rivers"', '["ponds"', '["island arcs"']) {
    assert.ok(list.includes(name), `${name} came off the list without a reason`);
  }
  assert.equal(typeof engine.exports.wb_erosion_run, "function");
  assert.ok(
    !appFile("main.js").includes("wb_erosion_run") && !appFile("main.js").includes("erosionRun"),
    "the erosion entry says nothing calls the export, and something now does",
  );
});

test("the measured tables are the ones the panel reads, and they say what the finding is", () => {
  // The panel's note quotes these; the tables are the sweep's output. Neither may drift from the
  // other, and the SHAPE of the finding is asserted rather than described.
  assert.equal(MEASURED_CREST.length, 4);
  for (let i = 1; i < MEASURED_CREST.length; i += 1) {
    assert.ok(MEASURED_CREST[i].crestSharpness > MEASURED_CREST[i - 1].crestSharpness);
    // **The mean rises monotonically towards zero as the exponent rises towards one.** That single
    // column is the whole difference between a drainage texture and a roughness.
    assert.ok(
      MEASURED_CREST[i].mean > MEASURED_CREST[i - 1].mean,
      "the measured mean must rise with the crest exponent");
  }
  const symmetric = MEASURED_CREST[MEASURED_CREST.length - 1];
  assert.equal(symmetric.crestSharpness, CARVE_BAND.high);
  assert.ok(symmetric.mean > 0, "the symmetric end must blanket");
  const shipped = MEASURED_CREST.find((r) => Object.is(r.crestSharpness, canonical.crestSharpness));
  assert.ok(shipped, "the sweep has no row at the exponent the engine actually ships");
  assert.ok(shipped.mean < 0, "the shipped exponent must carve");
  // The slope reference the panel prints is the engine's own, and it is two orders of magnitude
  // below what the published technique assumes -- which is the reason this kernel needed measuring
  // at all rather than transcribing.
  assert.equal(MEASURED_SLOPE.reference, canonical.slopeReference);
  assert.ok(MEASURED_SLOPE.publishedAssumption / MEASURED_SLOPE.reference > 50);
  for (const row of MEASURED_SLOPE.worlds) {
    // On every world in the set the reference lies above the p99 of ALL land and at or below the
    // p99 of high ground -- so the steepest tenth or so of high ground is at or above it, and no
    // world is either saturated or dead.
    assert.ok(row.landP99 < MEASURED_SLOPE.reference * 1.5);
    assert.ok(row.highP99 >= MEASURED_SLOPE.reference);
  }
  // And the amplitude claim is the published band, not a superlative: the gated flanks land inside
  // Hammond's hills and nowhere near his mountain floor.
  assert.equal(MEASURED_RELIEF.amplitudeM, drainage.amplitudeM);
  assert.ok(MEASURED_RELIEF.onP50 > MEASURED_RELIEF.hammondHills.low);
  assert.ok(MEASURED_RELIEF.onP50 < MEASURED_RELIEF.hammondHills.high);
  assert.ok(MEASURED_RELIEF.onP50 < MEASURED_RELIEF.hammondMountainFloor);
  assert.ok(MEASURED_RELIEF.offP50 < MEASURED_RELIEF.hammondHills.low,
    "the kernel-off baseline must be below the hills band, or the term adds nothing");
});
