// The panel-default family, as a check instead of as four separate bug fixes.
//
// Population/method/host, named once:
//   - The whole of `PANEL_RANGES` (nine range inputs) and `RAMP_STOPS` (fifteen stops),
//     which are the same objects `controls.js` builds its sliders from and `main.js` draws
//     its gradient from -- not copies. This file asserts about the shipped table.
//   - Host: node v22.17.0, no browser, no wasm. Nothing here needs either.
//
// **Why one file for two bugs.** The brief asked for a fix each for the ramp stops and the
// radius slider, and then asked whether a single check could assert that *every* panel
// default equals the value the boot path uses. It can, and it is worth more than either fix:
// running it found a third instance nobody had reported (`rampMax`, min 500 step 250, cannot
// express 2,400), which is the difference between fixing two members of a family and closing
// it.

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import {
  DEFAULT_WORLD,
  PANEL_DEFAULTS,
  PANEL_RANGES,
  RAMP_STOPS,
  RAMP_WINDOW,
  SEA_LEVEL_M,
  panelFieldFaults,
  rampFractionHeight,
  rampStopFraction,
} from "../public/app/panel-fields.js";

const appFile = (name) =>
  readFileSync(fileURLToPath(new URL(`../public/app/${name}`, import.meta.url)), "utf8");

test("every panel slider can express its own default", () => {
  // The whole family in one assertion. An `<input type="range">` snaps to `min + n * step`,
  // so a default off that lattice is silently replaced and the panel reads back a number the
  // boot path never chose -- which is how a clean page with an untouched panel built a
  // 6,400 km planet while the status line said 6,371,000.
  assert.deepEqual(panelFieldFaults(), []);
});

test("that check can fail: both shipped mis-stepped sliders are refused", () => {
  // The red proof, kept rather than described. These two rows are the travel that was
  // actually in `controls.js` at ac9ea5b. If `panelFieldFaults` ever stops refusing them it
  // has stopped being able to fail, and the assertion above would be decoration.
  const asShipped = [
    { query: "radius", min: 1e6, max: 2e7, step: 1e5, value: DEFAULT_WORLD.radiusM },
    { query: "rampMax", min: 500, max: 12000, step: 250, value: RAMP_WINDOW.maximumHeight },
  ];
  const faults = panelFieldFaults(asShipped);
  assert.equal(faults.length, 2, `expected both to be refused, got ${JSON.stringify(faults)}`);
  assert.match(faults[0], /radius: .*cannot express the default 6371000; the slider would show 6400000/);
  assert.match(faults[1], /rampMax: .*cannot express the default 2400; the slider would show 2500/);
  // And a default outside its own travel is the other way to get the same class of wrong.
  assert.deepEqual(
    panelFieldFaults([{ query: "plates", min: 3, max: 40, step: 1, value: 99 }]),
    ["plates: default 99 is outside the slider's 3..40 travel"],
  );
});

test("the panel holds no copy of a default: controls.js writes none of them down", () => {
  // The same shape as `relief-params.test.mjs`'s "no relief number is written down twice",
  // extended to the world and appearance knobs, because those are the ones that drifted.
  // Comments are stripped first: prose may quote a number, code may not restate one.
  const strip = (source) => source
    .split("\n")
    .map((line) => line.replace(/^\s*\/\/\/?.*$/, ""))
    .join("\n");
  const code = strip(appFile("controls.js"));
  // **A stated limit rather than a tuned one.** Only defaults with three or more digits are
  // searched for. `12`, `65`, `18` and `1` occur inside unrelated arithmetic in any file that
  // formats numbers (`toFixed(1)`, `/ 1000`, `121.5`), so searching for them would fire on
  // correct code, and loosening the pattern until it stopped would make the check meaningless
  // for the long values too. So this covers seed, radius, land and both ramp ends -- which is
  // every default that has actually drifted -- and does not cover plates, posts, feature cap
  // or exaggeration. Those four are still single-sourced; they are simply not *proved* to be
  // by this assertion.
  const checked = Object.entries(PANEL_DEFAULTS)
    .filter(([, value]) => value.replace(/[^0-9]/g, "").length >= 3);
  assert.deepEqual(
    checked.map(([name]) => name).sort(),
    ["land", "radius", "rampMax", "rampMin", "seed"],
    "the set this assertion actually covers changed; say so in the comment above",
  );
  for (const [name, value] of checked) {
    assert.ok(
      !code.includes(value),
      `controls.js restates the ${name} default ${value}; it must import it from panel-fields.js`,
    );
  }
  // ...and it must actually reach for them, or the assertion above would pass on a file with
  // no panel in it at all.
  assert.match(appFile("controls.js"), /from "\.\/panel-fields\.js"/);
  assert.match(appFile("main.js"), /from "\.\/panel-fields\.js"/);
});

test("the ramp's coastline is at the datum, and stays there when the window moves", () => {
  // The bug this replaces, in the units it happened in: the window was narrowed to
  // -7000..2400 and the "strand" stop was left at the fraction 0.60, which is -1,360 m.
  assert.equal(
    rampFractionHeight(0.6, RAMP_WINDOW.minimumHeight, RAMP_WINDOW.maximumHeight),
    -1360,
    "the arithmetic that made this a bug must still be the arithmetic being avoided",
  );

  // The stop table is in metres, and one of its entries IS the datum.
  const strand = RAMP_STOPS.findIndex(([metres]) => metres === SEA_LEVEL_M);
  assert.ok(strand > 0, "no stop sits at the datum; the coastline is placed, not derived");
  assert.ok(
    RAMP_STOPS[strand - 1][0] < SEA_LEVEL_M && RAMP_STOPS[strand + 1][0] > SEA_LEVEL_M,
    "the datum stop must straddle water and land, or the hard colour change is elsewhere",
  );

  // And the property the fractions never had: for ANY window, the strand lands exactly where
  // that window puts 0 m. Four windows including the one that broke it and the one before.
  for (const [lo, hi] of [[-7000, 2400], [-9000, 6000], [-11000, 500], [-6800, 1980]]) {
    assert.equal(
      rampStopFraction(SEA_LEVEL_M, lo, hi), (0 - lo) / (hi - lo),
      `strand misplaced on window ${lo}..${hi}`,
    );
  }

  // Monotone and inside the table's own reach, so the gradient is a ramp and not a fold.
  for (let i = 1; i < RAMP_STOPS.length; i += 1) {
    assert.ok(RAMP_STOPS[i][0] > RAMP_STOPS[i - 1][0], `stop ${i} is not above stop ${i - 1}`);
  }
});

test("every ramp stop is a height this generator actually reaches", () => {
  // The other half of the same defect: the previous table's top two colours sat at an implied
  // 2,165 m and 2,400 m on a planet whose highest measured land is 1,979 m, so the ramp's own
  // snow band had never once been drawn. Bounds are from the global fill quoted in
  // `panel-fields.js`; they are deliberately loose (the check is "reachable", not "exact"),
  // and they are asserted so that widening the table again fails here rather than silently.
  const MEASURED_MIN_M = -6807;
  const MEASURED_MAX_M = 2051;
  for (const [metres, color] of RAMP_STOPS) {
    assert.ok(
      metres >= MEASURED_MIN_M && metres <= MEASURED_MAX_M,
      `ramp stop ${color} at ${metres} m is outside the ${MEASURED_MIN_M}..${MEASURED_MAX_M} m ` +
      "range this generator produces, so it can never be drawn",
    );
  }
});

test("the panel's field names are the query parameters the boot path reads", () => {
  // A slider wired to a parameter nothing reads is a knob that does nothing, and it looks
  // exactly like a knob that works. Checked against the boot path's own source rather than
  // against a second list.
  //
  // **The boot path is `main.js` PLUS the modules it imports**, and that widening was forced by
  // a real case rather than chosen: the cloud layer's `?clouds=` read lives in
  // `cloud-provider.js::cloudCoverFromParams`, next to `reliefLayerEnabled` and
  // `biomeColourEnabled`, which is where a layer's own switch belongs. Narrowing the search to
  // `main.js` alone would have forced the read up into the wiring file purely to satisfy a test,
  // which is the check dictating the architecture.
  //
  // It is still one hop, deliberately, not a sweep of `app/`: a parameter mentioned anywhere in
  // the directory would be satisfied by a stale comment in a file nothing loads, and this check
  // exists to prove the knob is on the path the browser actually takes.
  const main = appFile("main.js");
  const imported = [...main.matchAll(/from\s+"\.\/([\w-]+\.js)"/g)].map((m) => m[1]);
  assert.ok(imported.length >= 8, "main.js's import list did not parse -- the sweep below is empty");
  const sources = [main, ...imported.map(appFile)];
  for (const { query } of PANEL_RANGES) {
    assert.ok(
      sources.some((source) => source.includes(`"${query}"`)),
      `neither main.js nor any module it imports reads the "${query}" parameter the panel writes`,
    );
  }
});
