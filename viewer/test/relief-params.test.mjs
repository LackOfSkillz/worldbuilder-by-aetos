// Node-native tests for the viewer's relief channel: `relief-params.js`, `engine.js`'s three
// new marshalling methods, and the two properties that are easy to claim and easy to get
// wrong -- that the untouched path is still the untouched world, and that no relief number is
// written down twice.
//
// No framework: `node:test` + `node:assert/strict`, run with `npm test` from `viewer/`.
// Nothing here touches Cesium or the DOM.
//
// Population/method/host, named once so every number below can be traced:
//   - World: Surface::new(20260904, 6_371_000, 12, 0.29, None) -- DEFAULT_WORLD in main.js,
//     the fixture whose elevation at lat 12 lon 34 was witnessed three independent ways.
//   - Host: node, this repository's checked-in
//     viewer/public/wasm/worldbuilder_engine.wasm, loaded directly from disk (no fetch).
//   - Probes: the six lat/lon in PROBES below, the same set the engine-side sweep uses.
//   - Slider population: every integer position each of the three sliders can take --
//     46 + 29 + 26 = 101 -- each turned into a relief block and put through wb_relief_check.

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { Engine, WB_OK } from "../public/app/engine.js";
import {
  RELIEF_CONTROLS,
  RELIEF_FIELDS,
  RELIEF_PARAM_NAMES,
  PERSISTENCE_STEPS,
  HURST_BAND,
  hurst,
  sliderTravel,
  reliefFromParams,
  reliefToParams,
} from "../public/app/relief-params.js";

const DEFAULT_WORLD = { seed: 20260904, radiusM: 6371000, plateCount: 12, landFraction: 0.29 };

const PROBES = [
  [12.0, 34.0],
  [0.0, 0.0],
  [-18.25, 121.5],
  [62.5, -145.0],
  [-71.0, 25.0],
  [35.0, 138.0],
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
let hills;
let travel;

test.before(async () => {
  engine = await loadEngine();
  canonical = engine.reliefPreset("canonical");
  hills = engine.reliefPreset("hills");
  travel = sliderTravel(canonical, hills);
});

test("the presets come across the boundary whole, in field order", () => {
  for (const field of RELIEF_FIELDS) {
    assert.equal(typeof canonical[field], "number", `canonical.${field}`);
    assert.equal(typeof hills[field], "number", `hills.${field}`);
    assert.ok(Number.isFinite(canonical[field]) && Number.isFinite(hills[field]));
  }
  // Ordering, not values: if the ten f64 were read in the wrong order the wavelengths and
  // the amplitudes would swap places, and every one of them would still be "a number". The
  // schedule's own invariant catches that -- the coarsest band is coarser than the canonical
  // one, by a lot, in both presets.
  for (const preset of [canonical, hills]) {
    assert.ok(preset.coarsestWavelengthM > preset.canonicalWavelengthM * 10);
    assert.ok(preset.quietingScaleM > preset.mountainM);
    assert.ok(preset.octavePersistence > 0 && preset.octavePersistence <= 1);
    assert.ok(Math.abs(preset.quietingStrength) <= 1);
  }
  // The three fields `hills()` moves are the three the panel drives, and nothing else moved.
  const moved = RELIEF_FIELDS.filter((f) => canonical[f] !== hills[f]);
  assert.deepEqual(moved.sort(), [...RELIEF_CONTROLS].sort());
});

test("no relief number is written down twice in the viewer", () => {
  // The hazard this whole design is arranged against, stated as a test rather than as a
  // comment. The panel's elevation-ramp defaults drifted from main.js's once and silently
  // reverted the ramp on every generate; the slice's pre-flight scan flagged the same shape
  // for hills()'s three values before any task ran.
  //
  // So: the three values `ReliefParams::hills()` moves must not appear as literals in the two
  // files that present them. Comments are stripped first -- prose is allowed to quote a
  // measurement, code is not allowed to restate one.
  const strip = (source) => source
    .split("\n")
    .map((line) => line.replace(/^\s*\/\/.*$/, "").replace(/^\s*\/\/\/.*$/, ""))
    .join("\n");
  const literals = RELIEF_CONTROLS.map((field) => String(hills[field]));
  assert.deepEqual(literals.sort(), ["-0.7", "0.65", "600"]);
  for (const name of ["controls.js", "main.js"]) {
    const code = strip(appFile(name));
    for (const literal of literals) {
      assert.ok(
        !code.includes(literal),
        `${name} restates the hills preset value ${literal}; it must read it from the engine`,
      );
    }
  }
  // And the panel must reach the engine for them: the import and the call have to be there,
  // or the assertion above would pass on a file that simply has no relief section.
  assert.match(appFile("controls.js"), /from "\.\/relief-params\.js"/);
  assert.match(appFile("main.js"), /engine\.reliefPreset\("canonical"\)/);
  assert.match(appFile("engine.js"), /wb_relief_preset/);
});

test("the slider travel lands exactly on every value that has a meaning", () => {
  // Both ends of two sliders, and the midpoint of one, are values with stated meanings. Float
  // arithmetic does not respect intentions: `0.7 - 14 * 0.05` is -1.1e-16, not 0. These are
  // asserted with `===` for that reason.
  assert.equal(travel.mountainM.toValue(travel.mountainM.min), canonical.mountainM);
  assert.equal(travel.mountainM.toValue(travel.mountainM.max), hills.mountainM);
  assert.equal(travel.quietingStrength.toValue(14), canonical.quietingStrength);
  assert.equal(travel.quietingStrength.toValue(-14), hills.quietingStrength);
  assert.equal(travel.quietingStrength.toValue(0), 0);
  assert.equal(travel.octavePersistence.toValue(0), canonical.octavePersistence);
  assert.equal(travel.octavePersistence.toValue(15), hills.octavePersistence);
  // The top of the travel is Task 2's own swept top, 0.75. Nothing above it has been
  // measured, which is why nothing above it is reachable.
  assert.equal(travel.octavePersistence.toValue(PERSISTENCE_STEPS), 0.75);
  // Round trips: a preset set on the sliders reads back as itself.
  for (const preset of [canonical, hills]) {
    for (const field of RELIEF_CONTROLS) {
      const position = travel[field].toPosition(preset[field]);
      assert.ok(Number.isInteger(position), `${field} position for a preset must be integral`);
      assert.equal(travel[field].toValue(position), preset[field], `${field} round trip`);
    }
  }
});

test("every position the sliders can take is a block the engine accepts", async () => {
  // The JS-side half of the engine's own slider sweep. The Rust test asserts the same 101
  // values are admissible through the native boundary; this asserts the *widget* produces
  // exactly those values, through the committed artifact, so the panel cannot offer a
  // position that turns the viewer blank.
  let checked = 0;
  for (const field of RELIEF_CONTROLS) {
    for (let position = travel[field].min; position <= travel[field].max; position += 1) {
      const relief = { ...canonical, [field]: travel[field].toValue(position) };
      assert.equal(
        engine.checkRelief(relief), WB_OK,
        `the ${field} slider can reach ${travel[field].toValue(position)} and the engine refuses it`,
      );
      checked += 1;
    }
  }
  assert.equal(checked, 46 + 29 + 26);
});

test("an untouched viewer is the untouched world", () => {
  // RULING 1, at the artifact rather than in the engine's own test binary. Three worlds: the
  // original two-argument constructor, the relief constructor with a null block, and the
  // relief constructor with the canonical block spelled out. All three must be the same
  // planet, bit for bit.
  assert.equal(reliefFromParams(new URLSearchParams(""), canonical), null);
  // A URL that names every relief parameter at its canonical value is still the canonical
  // path -- otherwise a shared link would build a different-shaped call for the same world.
  const atCanonical = new URLSearchParams(
    RELIEF_CONTROLS.map((f) => [RELIEF_PARAM_NAMES[f], String(canonical[f])]),
  );
  assert.equal(reliefFromParams(atCanonical, canonical), null);
  // A parameter that is not a number is ignored rather than forwarded: a typo in a shared
  // link should not be a blank page.
  assert.equal(reliefFromParams(new URLSearchParams("quieting=banana"), canonical), null);

  const legacy = engine.exports.wb_world_new(
    BigInt(DEFAULT_WORLD.seed), DEFAULT_WORLD.radiusM, DEFAULT_WORLD.plateCount,
    DEFAULT_WORLD.landFraction, 0, 0,
  ) >>> 0;
  assert.notEqual(legacy, 0);
  const defaulted = engine.newWorld({ ...DEFAULT_WORLD, relief: null });
  const spelled = engine.newWorld({ ...DEFAULT_WORLD, relief: canonical });
  try {
    for (const [lat, lon] of PROBES) {
      for (const resolution of [250, -1]) {
        const reference = engine.elevationM(legacy, lat, lon, resolution);
        assert.equal(engine.elevationM(defaulted, lat, lon, resolution), reference,
          `the null relief path moved the world at ${lat},${lon}`);
        assert.equal(engine.elevationM(spelled, lat, lon, resolution), reference,
          `the canonical block is not the null path at ${lat},${lon}`);
      }
    }
    // The witnessed value itself, unchanged by a slice that added a parameter block.
    assert.equal(engine.elevationM(defaulted, 12.0, 34.0, 250), 682.3921701573904);
  } finally {
    engine.freeWorld(legacy);
    engine.freeWorld(defaulted);
    engine.freeWorld(spelled);
  }
});

test("a chosen block reaches the ground, and hills is not canonical", () => {
  const chosen = reliefFromParams(new URLSearchParams("persistence=0.65&mountainM=600"), canonical);
  assert.equal(chosen.octavePersistence, 0.65);
  assert.equal(chosen.mountainM, 600);
  assert.equal(chosen.quietingStrength, canonical.quietingStrength, "untouched fields stay canonical");
  assert.equal(chosen.abyssalM, canonical.abyssalM);

  const plain = engine.newWorld({ ...DEFAULT_WORLD, relief: null });
  const rough = engine.newWorld({ ...DEFAULT_WORLD, relief: hills });
  try {
    const differences = PROBES.filter(
      ([lat, lon]) => engine.elevationM(plain, lat, lon, 250) !== engine.elevationM(rough, lat, lon, 250),
    );
    assert.ok(differences.length > 0, "the hills preset changed nothing at any probe");
  } finally {
    engine.freeWorld(plain);
    engine.freeWorld(rough);
  }
});

test("a shared link carries only what was moved", () => {
  assert.deepEqual(reliefToParams(canonical, canonical), {
    mountainM: null, quieting: null, persistence: null,
  });
  const moved = { ...canonical, mountainM: 300 };
  assert.deepEqual(reliefToParams(moved, canonical), {
    mountainM: "300", quieting: null, persistence: null,
  });
});

test("the Hurst readout is the published band, not this engine's own numbers", () => {
  // H = ln(1/p) / ln(lacunarity), lacunarity 2 -- `plan`'s `wavelength *= 0.5`.
  assert.equal(hurst(0.5), 1);
  assert.ok(Math.abs(hurst(0.65) - 0.6215) < 1e-4);
  assert.ok(Math.abs(hurst(0.75) - 0.4150) < 1e-4);
  // hills()'s persistence is the only value on the slider's travel that lands inside real
  // terrain's measured band, which is why the readout marks the band rather than the value.
  const inBand = [];
  for (let position = travel.octavePersistence.min; position <= travel.octavePersistence.max; position += 1) {
    const h = hurst(travel.octavePersistence.toValue(position));
    if (h >= HURST_BAND.low && h <= HURST_BAND.high) inBand.push(travel.octavePersistence.toValue(position));
  }
  assert.ok(inBand.includes(hills.octavePersistence), "hills() must sit inside the marked band");
  assert.ok(!inBand.includes(canonical.octavePersistence), "canonical (H=1.0) is outside it");
});

test("a refused relief block is refused, and says why", () => {
  // The engine's domain, reached from JS. `checkRelief` is what lets the panel say which
  // field, instead of "the engine said no".
  assert.notEqual(engine.checkRelief({ ...canonical, octavePersistence: 1.5 }), WB_OK);
  assert.notEqual(engine.checkRelief({ ...canonical, canonicalWavelengthM: 0 }), WB_OK);
  assert.notEqual(engine.checkRelief({ ...canonical, mountainM: Number.NaN }), WB_OK);
  assert.notEqual(engine.checkRelief({ ...canonical, quietingStrength: Infinity }), WB_OK);
  assert.equal(engine.checkRelief(null), WB_OK);
  assert.throws(
    () => engine.newWorld({ ...DEFAULT_WORLD, relief: { ...canonical, octavePersistence: 1.5 } }),
    /WB_ERR_PARAM/,
    "a refused world must name the relief status rather than only the handle",
  );
});
