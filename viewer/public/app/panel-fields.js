//! The world defaults and the panel's slider travel, in ONE place, with a check that a
//! slider can express its own default.
//
// # Why this file exists
//
// **A panel value that is not the engine's value is this viewer's characteristic defect.**
// It has now happened four times, in the same shape every time -- a number written down in
// `controls.js` next to a *different* copy of the same number somewhere else, with no way
// for either to notice:
//
// 1. The elevation-ramp defaults: the panel offered -9000/6000 while `main.js` had narrowed
//    to -7000/2400, so opening the panel and pressing generate silently reverted the ramp
//    and the rock and snow bands vanished. Fixed at `f24a9d9` by copying the new numbers
//    across -- which is to say, by making a second correct copy.
// 2. The ramp *stops*: the window moved to -7000..2400 and the gradient's fractions did not,
//    so the "strand" stop at 0.60 landed at -1,360 m and the default path drew every
//    coastline 1.4 km below sea level.
// 3. The radius slider: `min 1e6, step 1e5` cannot express `6371000`, so the panel showed
//    6.4 Mm and pressing generate on a clean page built a 6,400 km planet.
// 4. And the same arithmetic in `rampMax`: `min 500, step 250` cannot express `2400` either.
//    Nobody had noticed this one; the check below found it.
//
// Two of those are drift between copies and two are a slider that cannot land on its own
// default. So this file removes the copies -- every default here is either defined once,
// here, or imported from the module that defines it -- and `panelFieldFaults()` is the
// falsifiable version of the second kind: **for every range input, the default must be a
// value the slider can actually produce.** It is one check over a table rather than two
// bug fixes, which is what makes it close the family instead of extending it.
//
// This module is DOM-free and Cesium-free on purpose, so `node --test` can hold the same
// table the browser builds its panel from. If it needed a `document`, the check would have
// to restate the numbers, and a check that restates the numbers is the defect.

import { FEATURE_CEILING } from "./availability.js";
import { HEIGHTMAP_SIZE, MAX_LEVEL } from "./terrain.js";

/// The default world is the one this slice's fixtures pin: `Surface::new(20260904,
/// 6_371_000, 12, 0.29, None)`. The extraction witnessed an elevation on it three
/// independent ways -- Python wheel, native Rust, browser WASM -- so it is the world with a
/// known answer at a named point, and that is why it is the default rather than something
/// prettier.
export const DEFAULT_WORLD = {
  seed: 20260904,
  radiusM: 6371000,
  plateCount: 12,
  landFraction: 0.29,
};

/// The window `ElevationRamp` maps onto its 256 x 1 gradient. Both ends are the range this
/// generator actually occupies: a 4,170,724-sample global fill puts the sea floor's minimum
/// at -6,345 m and the highest land at 1,979 m on `DEFAULT_WORLD` (-6,807 m and 2,051 m on a
/// second seed), so -9000..+6000 spent two thirds of the gradient on heights that do not
/// exist. **Moving these does not move the coastline** -- see `elevationRamp` in `main.js`,
/// whose stops are placed in metres from the datum rather than as fractions of this window.
/// That independence is the whole point; defect 2 above was the two being coupled.
export const RAMP_WINDOW = { minimumHeight: -7000, maximumHeight: 2400 };

/// `scene.verticalExaggeration`'s default: Cesium's own, restated nowhere else.
export const DEFAULT_EXAGGERATION = 1;

/// The extraction's harbour: a 900 x 260 m carve to -12 m with a 200 x 60 m mole to +4 m
/// inside it, both on bearing 35 deg, at 18.25 S 121.5 E. Off by default -- a bare world is
/// what the zoom-cap reasoning is about, and this is what contradicts it.
export const HARBOUR = [
  {
    latitudeDeg: -18.25, longitudeDeg: 121.5, targetM: -12, lengthM: 900, widthM: 260,
    bearingDeg: 35, compose: "carve", substrate: "derive",
  },
  {
    latitudeDeg: -18.25, longitudeDeg: 121.5, targetM: 4, lengthM: 200, widthM: 60,
    bearingDeg: 35, compose: "raise", substrate: "derive",
  },
];

/// Every `<input type="range">` the panel builds, as data.
///
/// `query` is the URL parameter `main.js` reads, so the check can also assert that the panel
/// and the boot path are talking about the same knob. `value` is the default, and it is
/// **imported** wherever another module already owns it.
///
/// The relief sliders are deliberately absent: their travel is computed at runtime from
/// `wb_relief_preset`, so they hold no number to drift and `relief-params.test.mjs` already
/// pins that they hold none.
export const PANEL_RANGES = [
  { query: "plates", min: 3, max: 40, step: 1, value: DEFAULT_WORLD.plateCount },
  { query: "land", min: 0.05, max: 0.95, step: 0.01, value: DEFAULT_WORLD.landFraction },
  // step 1e5 was the third defect: it cannot express 6,371,000. 1 km can, and a kilometre is
  // a finer knob than a planet radius needs anyway.
  { query: "radius", min: 1e6, max: 2e7, step: 1e3, value: DEFAULT_WORLD.radiusM },
  { query: "maxLevel", min: 8, max: 16, step: 1, value: MAX_LEVEL },
  { query: "size", min: 33, max: 129, step: 32, value: HEIGHTMAP_SIZE },
  { query: "featureCeiling", min: 12, max: 22, step: 1, value: FEATURE_CEILING },
  { query: "exaggeration", min: 1, max: 40, step: 1, value: DEFAULT_EXAGGERATION },
  // step 250 was the fourth defect, found by the check rather than by a screenshot: it
  // cannot express 2,400 either, so the ramp's top silently became 2,250 or 2,500.
  { query: "rampMin", min: -11000, max: 0, step: 100, value: RAMP_WINDOW.minimumHeight },
  { query: "rampMax", min: 500, max: 12000, step: 100, value: RAMP_WINDOW.maximumHeight },
];

/// Panel defaults as the strings `controls.js` compares against and writes into the URL.
/// `seed` is a text field with no travel, so it is added here rather than to `PANEL_RANGES`.
export const PANEL_DEFAULTS = Object.fromEntries([
  ["seed", String(DEFAULT_WORLD.seed)],
  ...PANEL_RANGES.map((f) => [f.query, String(f.value)]),
]);

/// **The check that closes the family.** Returns one string per fault, empty when every
/// range input can express its own default.
///
/// An `<input type="range">` snaps its value to `min + n * step`; a default that is not on
/// that lattice is silently replaced by the nearest value that is, and the panel then reads
/// back a number the boot path never chose. That is defects 3 and 4, and it is invisible
/// unless you either read the rendered slider or run this.
///
/// The tolerance is relative and exists for binary floating point, not for slack:
/// `0.05 + 24 * 0.01` is `0.29000000000000004`, so an exact test would refuse a `land`
/// default the browser accepts. `1e-9` of a step is far tighter than any real mis-step
/// (the two real ones are off by 0.71 and 0.6 of a step) and far looser than one ulp.
export function panelFieldFaults(fields = PANEL_RANGES) {
  const faults = [];
  for (const { query, min, max, step, value } of fields) {
    if (!(value >= min && value <= max)) {
      faults.push(`${query}: default ${value} is outside the slider's ${min}..${max} travel`);
      continue;
    }
    const steps = (value - min) / step;
    if (Math.abs(steps - Math.round(steps)) > 1e-9) {
      const snapped = min + Math.round(steps) * step;
      faults.push(
        `${query}: min ${min} step ${step} cannot express the default ${value}; ` +
        `the slider would show ${snapped}`,
      );
    }
  }
  return faults;
}

/// The hypsometric ramp's stops, in metres above the datum.
///
/// **The stops are metres above the datum, not fractions of the window.** That is the fix
/// for a bug that shipped: the window was narrowed from -9000..6000 to -7000..2400 and the
/// gradient's fractions were left where they were, so the "strand" stop at 0.60 landed at
/// `-7000 + 0.6 * 9400 =` **-1,360 m** and the default path drew every coastline 1.4 km below sea
/// level. Comparing the relief layer's coastline with the ramp's is how it was found -- the
/// landmasses were different sizes and the relief layer's were the correct ones.
///
/// **Sea level is a datum the engine knows** (`wb_elevation_m` is documented as metres above
/// datum, so the datum is 0 by construction), so `0` appears in the table below as a stop
/// like any other and `rampStopFraction` puts it where the window says it goes. Move the window
/// anywhere and the coastline stays on the coast; that is the property the old form did not
/// have, and re-placing the fractions by hand would not have given it either.
///
/// Every stop is also a height this generator reaches. A 4,170,724-sample global fill puts
/// the sea floor's minimum at -6,345 m and the highest land at 1,979 m, so the previous
/// table's implied 2,165 m and 2,400 m whites were unreachable and the ramp's own snow band
/// had never once been drawn.
export const RAMP_STOPS = [
  [-6800, "#020a14"],   // abyssal plain
  [-4600, "#04182e"],   // the sea floor's own plateau: p10..p25 of every sampled depth
  [-1200, "#0a3358"],   // basin
  [-200, "#14548c"],
  [-60, "#2f86bd"],     // shelf, just under the coast -- the pale rim around every landmass
  [-8, "#7ec5df"],
  [0, "#ddcfa8"],       // THE DATUM: strand. Derived, not placed.
  [40, "#8f9a5e"],
  [380, "#4a7a3c"],     // lowland
  [700, "#5d7440"],
  [1000, "#7d7150"],    // upland
  [1300, "#8e8272"],
  [1500, "#b9b2a8"],    // bare rock
  [1750, "#e8e6e2"],
  [1980, "#ffffff"],    // snow
];

/// The engine's datum. `wb_elevation_m` is documented as *metres above datum*, so sea level
/// is 0 by construction -- it is not a number this viewer chose and it cannot drift. Exported
/// so the check can say which stop it is asserting about.
export const SEA_LEVEL_M = 0;

/// Where a stop in metres lands on the 0..1 gradient, for a given window. Clamped, so a stop
/// outside the window anchors its colour at the edge rather than vanishing.
export function rampStopFraction(metres, minimumHeight, maximumHeight) {
  const t = (metres - minimumHeight) / (maximumHeight - minimumHeight);
  return t < 0 ? 0 : t > 1 ? 1 : t;
}

/// The inverse: the height a **hand-placed fraction** actually lands on, for a given window.
/// Nothing calls this in the browser. It exists so the ramp-stop test can state the bug it
/// guards against in the units the bug happened in -- 0.60 against -7000..2400 is -1,360 m,
/// and that sentence is only checkable if the arithmetic is somewhere a test can reach it.
export function rampFractionHeight(fraction, minimumHeight, maximumHeight) {
  return minimumHeight + fraction * (maximumHeight - minimumHeight);
}
