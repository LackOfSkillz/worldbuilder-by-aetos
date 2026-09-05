//! The tectonic channel, as JavaScript sees it: field order, the calibrated slider travel,
//! and the query-string mapping. **No DOM, no Cesium, no engine instance** -- everything here
//! is a pure function over plain numbers, exactly as `relief-params.js` is, which is why
//! `viewer/test/tectonic-params.test.mjs` can hold it against the real wasm without a browser.
//
// # Why this module exists at all
//
// The owner asked for two sliders in their own words -- *"1 to raise and lower mountains and
// one to make more mountains and less as desired"* -- and then, looking at the finished
// relief work, *"we still have no mountains. why is it so hard to make mountains?"*
//
// The honest answer was that **the peak on their own world is 98.9% tectonic**: 1,454.04 m,
// of which 1,437.81 m is structural and 16.24 m is detail, measured on seed 123925603,
// radius 4,500,000 m, 28 plates, land fraction 0.16, over a 0.5-degree global grid refined to
// 0.05 degrees at the maximum. No relief parameter could ever have moved it -- and the relief
// panel says so, in the note under its own sliders. This module is the other knob.
//
// # There is no tectonic default literal anywhere in the viewer
//
// The same hazard `relief-params.js` opens with, and the same answer. Every one of the three
// sliders is **anchored on the engine's own `TectonicParams::canonical()`**, read across the
// boundary through `wb_tectonic_preset` at boot: the height slider starts there, the width
// slider starts there, and the count slider is centred there. `1500`, `400000` and `0.45`
// are not written down in `viewer/`, and `tectonic-params.test.mjs` asserts they are not.
//
// # Why the sliders carry integer positions
//
// For the reason `relief-params.js` gives at length: an `<input type="range">` computes its
// value as `min + n * step`, so a value-stepped widget can only reach the lattice that
// arithmetic produces, and a default off that lattice is silently replaced -- the defect
// `panelFieldFaults()` exists for, which has now been found four times in this viewer. The
// widget therefore carries an integer position and this module maps it to a tectonic value
// through an expression that lands on canonical **exactly** at position 0.
//
// That exactness is load-bearing rather than tidy: `tectonicToParams` drops every field still
// equal to canonical, so a page whose sliders were never touched writes no tectonic parameter
// at all and the reload takes the engine's `None` path -- RULING 1 of this slice, which is
// that the default world cannot move. A position-0 value one ULP off canonical would be
// written into every shared link and would take the untouched viewer off the default path.
// **`canonical * (n / 9)` is exact at n = 9 and `canonical * n / 9` is one ULP out**, and the
// engine-side test found that by failing rather than by inspection.

/// f64 per tectonic record, and the field names in `wasm.rs`'s `WB_TECTONIC_STRIDE` order.
/// That order is the ABI: `wb_tectonic_preset` writes it and `wb_tectonic_check` reads it.
export const TECTONIC_STRIDE = 9;
export const TECTONIC_FIELDS = [
  "continentCollisionM",
  "continentCollisionWidthM",
  "coastalUpliftM",
  "coastalUpliftWidthM",
  "islandArcM",
  "islandArcWidthM",
  "ridgeM",
  "ridgeWidthM",
  "continentalBlend",
];

/// `wb_tectonic_preset` selectors, mirrored from `wasm.rs`. There is exactly one, and that is
/// Ruling 1 rather than an omission: a *named* tectonic preset is Task 3's decision, taken
/// against Task 2's fuller survey, and inventing one here would be choosing for the owner
/// before they can turn the knob themselves.
export const TECTONIC_PRESET = { canonical: 0 };

/// The three parameters the panel drives, and the query-string name each answers to.
///
/// Three of nine. **`islandArcM` and `islandArcWidthM` are deliberately absent**, and not for
/// want of a widget: Task 1 proved seven of the nine fields are read by perturbing each by
/// one ULP across three fixtures, and its tests state explicitly that those two have **no
/// coverage** -- the arc term is multiplied by an oceanic weight a synthetic two-plate fixture
/// at land fraction 0.02 still did not produce. A control with no evidence that the path reads
/// it is a control that might do nothing. The four remaining unexposed fields (the coastal
/// pair and the ridge pair) are proven-read but uncalibrated, and a slider whose travel nobody
/// has measured is a slider nobody can aim.
export const TECTONIC_CONTROLS = [
  "continentCollisionM",
  "continentCollisionWidthM",
  "continentalBlend",
];
export const TECTONIC_PARAM_NAMES = {
  continentCollisionM: "mtnHeight",
  continentCollisionWidthM: "mtnWidth",
  continentalBlend: "mtnCount",
};

/// **THE CALIBRATION.** Measured on the owner's own world, not the default one.
///
/// - **Population:** seed 123925603, radius 4,500,000 m, 28 plates, land fraction 0.16 -- the
///   world from their screenshot. A 0.5-degree global grid (720 x 359 = 258,480 sites)
///   refined at 0.05 degrees in a 2-degree box around the coarse maximum.
/// - **Method:** the peak of `Surface::elevation_m(point, None)`, and a grade measured as the
///   **steepest single 2 km step on the flank** -- 12 bearings walked out from the peak to
///   250 km. Measuring across the summit measures the one place a mountain is flat, and the
///   first version of the probe did exactly that and reported the opposite of the truth.
/// - **Host:** this repository's machine, native release build. `src/bin/mountain_probe.rs`.
///
/// Real ranges run 3-8%. Today's canonical pair is the first row, and it is a ramp.
export const MEASURED_GRADES = [
  { heightM: 1500, widthKm: 400, peakM: 1454.0, grade: 1.787, note: "canonical" },
  { heightM: 1500, widthKm: 100, peakM: 1377.6, grade: 2.575 },
  { heightM: 3000, widthKm: 400, peakM: 2500.0, grade: 2.677 },
  { heightM: 3000, widthKm: 150, peakM: 2441.1, grade: 2.852 },
  { heightM: 6000, widthKm: 150, peakM: 4551.5, grade: 4.936 },
  { heightM: 6000, widthKm: 100, peakM: 4540.5, grade: 7.030 },
];

/// **Amplitude alone is not enough, and the table above says so.** 3,000 m at 400 km is
/// 2.677%, barely above canonical's 1.787%, while 6,000 m at 100 km is 7.030%. Height comes
/// from amplitude; steepness comes from the *pair*. A panel offering only height would let
/// the owner raise a 4 km peak that still looked like a ramp -- which is what "wheelchair
/// ramps" meant the first time they said it.
export const HEIGHT_STEPS = 45;
export const HEIGHT_STEP_M = 100;
export const WIDTH_STEPS = 30;
export const WIDTH_STEP_M = 10000;

/// The count slider's travel, in ninths of canonical, and **this one had no table**.
///
/// Task 1's probe did not vary `continental_blend`, so Task 4 measured it. Same world, same
/// 0.5-degree grid, counting sites rather than maximising: a peak height cannot see how many
/// margins run the collision profile, and a count of sites over a height can.
///
/// At 6,000 m / 150 km, sites above 1,000 m:
///
///     blend  0.01  0.05  0.10  0.20  0.30  0.45  0.60  0.80  1.00  1.50  2.00  4.00  8.00
///     count   964   949   925   847   757   618   496   402   332   244   212   153   126
///
/// **The knob runs the opposite way to its name**: `collision = inboard * outboard`, and each
/// side is `smoothstep((value / blend) * 0.5 + 0.5)`, so a *narrower* transition lets a
/// genuinely continental side saturate to 1 and a wider one drags every margin towards a
/// diluted half. More mountains is a SMALLER blend, so the widget's position is negated --
/// dragging right is more, which is the only orientation a control called "count" can have.
///
/// The ends are where the measurement stops being informative in each direction. **Below
/// 0.10 the counts saturate** (925 / 949 / 964 for a tenfold narrowing) into the hard-test
/// regime `CONTINENTAL_BLEND`'s own doc records -- *"the ground jumped five hundred and fifty
/// metres wherever a margin crossed it"*. **Above 1.00 the curve flattens** (332 to 212 over
/// the next whole unit) as the ramp grows past the `[-1, 1]` range `fbm` can produce at all.
/// Ninths because canonical is 0.45: a ninth is 0.05, and 11 down / 7 up lands on 1.00 and
/// 0.10 to within an ULP while keeping position 0 exact.
export const BLEND_NINTHS = 9;
export const BLEND_POSITIONS_FEWER = 11;
export const BLEND_POSITIONS_MORE = 7;

/// The slider travel for each driven parameter, derived from the engine's own canonical block.
///
/// Each entry is an integer position range plus the map from a position to a tectonic value.
/// `min`/`max` are positions, not values. Position 0 is canonical for all three.
export function tectonicTravel(canonical) {
  return {
    // Canonical (1,500 m) up to 6,000 m, 100 m a step. Both ends measured; the top is the
    // Alps and the bottom is today.
    continentCollisionM: {
      min: 0,
      max: HEIGHT_STEPS,
      toValue: (position) => canonical.continentCollisionM + position * HEIGHT_STEP_M,
      toPosition: (value) =>
        Math.round((value - canonical.continentCollisionM) / HEIGHT_STEP_M),
      format: (value) => `${value.toFixed(0)} m`,
    },
    // Canonical (400 km) DOWN to 100 km, 10 km a step. Down, because canonical is already the
    // widest a centred profile may be -- `MAX_TECTONIC_RANGE_M` is 420 km and beyond it a
    // margin is not evaluated at all -- so every setting this slider can reach is narrower,
    // and narrower is steeper.
    continentCollisionWidthM: {
      min: 0,
      max: WIDTH_STEPS,
      toValue: (position) => canonical.continentCollisionWidthM - position * WIDTH_STEP_M,
      toPosition: (value) =>
        Math.round((canonical.continentCollisionWidthM - value) / WIDTH_STEP_M),
      format: (value) => `${(value / 1000).toFixed(0)} km`,
    },
    // Ninths of canonical, negated so that right is more. `canonical * (n / 9)`, NOT
    // `canonical * n / 9`: the second is one ULP out at position 0.
    continentalBlend: {
      min: -BLEND_POSITIONS_FEWER,
      max: BLEND_POSITIONS_MORE,
      toValue: (position) =>
        canonical.continentalBlend * ((BLEND_NINTHS - position) / BLEND_NINTHS),
      toPosition: (value) =>
        Math.round(
          BLEND_NINTHS - (value * BLEND_NINTHS) / canonical.continentalBlend,
        ),
      format: (value) => value.toFixed(2),
    },
  };
}

/// The same three controls as `PANEL_RANGES` rows, **in tectonic units rather than positions**,
/// so `panelFieldFaults()` can be run over them.
///
/// The widget itself carries integer positions, where a default off the lattice is impossible
/// by construction -- but "impossible by construction" is what was said about the radius
/// slider too. This states the travel in the units the calibration was measured in and asks
/// the check the question it exists to ask: **can this slider express its own default?** It
/// is a real question here, because `min`, `max` and `value` are all derived from the engine's
/// canonical through f64 arithmetic and only `value` is untouched by it.
///
/// `controls.js` runs this before enabling the sliders and `tectonic-params.test.mjs` runs it
/// against the shipped `.wasm`, so it is a production check and not only a test.
export function tectonicPanelFields(canonical) {
  const travel = tectonicTravel(canonical);
  return TECTONIC_CONTROLS.map((field) => {
    const { min, max, toValue } = travel[field];
    const low = Math.min(toValue(min), toValue(max));
    const high = Math.max(toValue(min), toValue(max));
    // The lattice step in tectonic units, taken from the map rather than restated: one
    // position, measured.
    const step = Math.abs(toValue(1) - toValue(0));
    return { query: TECTONIC_PARAM_NAMES[field], min: low, max: high, step, value: canonical[field] };
  });
}

/// A tectonic block as a flat record in ABI order, ready for `wb_world_new_tectonic`.
export function tectonicToRecord(tectonics) {
  return TECTONIC_FIELDS.map((name) => tectonics[name]);
}

/// A flat record back into a named object.
export function tectonicFromRecord(record) {
  const out = {};
  TECTONIC_FIELDS.forEach((name, index) => { out[name] = record[index]; });
  return out;
}

/// The tectonic block a query string asks for, over the engine's own canonical block -- or
/// **`null`, meaning the canonical path**, which is `None` in the engine and is byte-for-byte
/// today's world.
///
/// **RULING 1 lives in this function**, exactly as it lives in `reliefFromParams`. A page
/// opened with no tectonic parameters, and a page whose tectonic parameters all happen to
/// equal canonical's, both return `null` here, so `wb_world_new_tectonic` gets a null pointer
/// and takes the same path `wb_world_new` does.
///
/// A parameter that is present but not a finite number is **ignored rather than forwarded**:
/// `?mtnHeight=banana` is a typo in a shared link, and answering it with a refused world would
/// turn a typo into a blank page. Values that are numbers but out of domain *are* forwarded,
/// and the engine refuses them -- a caller asking for something the engine declines is a
/// different thing from a caller not asking for anything.
export function tectonicFromParams(params, canonical) {
  const tectonics = { ...canonical };
  let touched = false;
  for (const field of TECTONIC_CONTROLS) {
    const name = TECTONIC_PARAM_NAMES[field];
    if (!params.has(name)) continue;
    const value = Number(params.get(name));
    if (!Number.isFinite(value)) continue;
    if (value === canonical[field]) continue;
    tectonics[field] = value;
    touched = true;
  }
  return touched ? tectonics : null;
}

/// The query-string fields for a chosen tectonic block, dropping every field still at
/// canonical so a shared link carries only what was actually moved.
export function tectonicToParams(tectonics, canonical) {
  const out = {};
  for (const field of TECTONIC_CONTROLS) {
    const name = TECTONIC_PARAM_NAMES[field];
    out[name] = tectonics[field] === canonical[field] ? null : String(tectonics[field]);
  }
  return out;
}
