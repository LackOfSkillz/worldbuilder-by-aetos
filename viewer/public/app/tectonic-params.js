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
export const TECTONIC_STRIDE = 14;
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
  // The five structure fields. `WB_TECTONIC_STRIDE` widened from 9 to 14 to carry them, and
  // that widening is what makes any of the work below reachable from a browser at all: they
  // existed in the engine for a whole task while `decode_tectonic` filled them from
  // `canonical()`, so the owner could not see one of them.
  "collisionAsymmetry",
  // An INTEGER carried in an f64 slot. The engine refuses a word that is not finite, exactly
  // integral and inside its ceiling -- it is a loop bound, and `as u32` in Rust saturates, so
  // 1e300 would otherwise arrive as four billion iterations of a per-sample loop.
  "sutureCount",
  "sutureSpreadM",
  "structureDepth",
  "structureWavelengthM",
];

/// `wb_tectonic_preset` selectors, mirrored from `wasm.rs`. Two now: canonical, and the preset
/// Task 3 chose.
///
/// **The preset crosses as FIELDS, never as a name.** `engine.tectonicPreset("ranges")` returns
/// fourteen numbers and the panel puts every one of them on its own slider, so the owner SEES
/// what the preset asked for and can move any part of it. That is Ruling 7 of the relief slice,
/// and `tectonic-params.test.mjs` enforces it here the same way `relief-params.test.mjs` does
/// there: it strips the comments out of this file and `controls.js` and asserts that 6000,
/// 100000, 0.7 and 80000 appear in neither.
export const TECTONIC_PRESET = { canonical: 0, ranges: 1 };

/// The parameters the panel drives, and the query-string name each answers to.
///
/// Eight of fourteen -- three from Task 4's envelope and five from Task 3's structure field. **`islandArcM` and `islandArcWidthM` are deliberately absent**, and not for
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
  "collisionAsymmetry",
  "sutureCount",
  "sutureSpreadM",
  "structureDepth",
  "structureWavelengthM",
];

/// The subset of `TECTONIC_CONTROLS` that gets a WIDGET. Six of the eight.
///
/// **The suture pair is driven but not sliderable, and that is a measured decision rather than
/// an omission.** The two are jointly constrained: `sutures` places suture `i` at
/// `i * sutureSpreadM * jitter`, so a COUNT slider at the canonical spread of 0.0 would stack
/// every suture on offset zero and multiply the amplitude by the sum of their weights -- a
/// height knob wearing a count's name, which the engine now refuses outright. And the useful
/// setting Task 2 found is a single point (two sutures, 100-150 km apart, where the
/// across-range crest count doubles and the peak does not move) rather than a travel: tighter
/// inflates the peak 64%, wider drives the profile past the range gate. A slider whose whole
/// useful range is one position is not a slider.
///
/// So the preset carries them, the query string carries them, the panel SHOWS them as a
/// readout, and nothing offers a control nobody could aim.
export const TECTONIC_SLIDERS = [
  "continentCollisionM",
  "continentCollisionWidthM",
  "continentalBlend",
  "collisionAsymmetry",
  "structureDepth",
  "structureWavelengthM",
];
export const TECTONIC_PARAM_NAMES = {
  continentCollisionM: "mtnHeight",
  continentCollisionWidthM: "mtnWidth",
  continentalBlend: "mtnCount",
  collisionAsymmetry: "mtnAsym",
  sutureCount: "mtnBelts",
  sutureSpreadM: "mtnBeltSpacing",
  structureDepth: "mtnStructure",
  structureWavelengthM: "mtnStructureWave",
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

/// **THE STRUCTURE TABLES, and the travel each one calibrates.** Task 2 measured all three on
/// the owner's world, on the steep 6,000 m / 100 km envelope, so each row answers "what does
/// this add to what we already ship".
///
/// The asymmetry, a doubly-vergent wedge -- one flank steeper than the other, which is what a
/// collisional range actually is (Willett, Beaumont & Fullsack 1993 for the mechanism; Naylor
/// & Sinclair 2008 for the numbers, a 115 km pro-wedge against a 69 km retro-wedge):
///
///     asymmetry  1.00   1.25   1.67   2.00   2.50   3.00
///     summits       2      5      7     10     12     16
///     grade     7.03%  7.91%  9.90% 11.36% 13.93% 16.54%
///     flanks     1.08   1.30   1.50   1.64   2.09   2.56
///
/// **Monotone in every column over six settings, and it costs 8 m of peak across the whole
/// sweep and no reach at all** -- the wide flank keeps its width and only the narrow one is
/// divided, so raising this can only ever make a range narrower.
///
/// The structure field, a ridged multifractal times a coarse segmentation field. Summits, at
/// each depth and wavelength:
///
///     depth        0.0   0.3   0.5   0.7   0.9
///     at 40 km       2     4     7    12    15
///     at 80 km       2     -     7     6     -
///     at 250 km      2     -     -     1     1
///
/// **Wavelength decides whether it works at all: 40-80 km bites, 120-250 km does nothing.**
/// That is why `WAVELENGTH_STEPS` stops at 40 km rather than running out to the 250 km the
/// table also measured -- two thirds of that travel would be dead.
export const ASYMMETRY_STEPS = 8;
export const ASYMMETRY_STEP_QUARTERS = 4;
export const DEPTH_STEPS = 9;
export const DEPTH_STEP_TENTHS = 10;
export const WAVELENGTH_STEPS = 4;
export const WAVELENGTH_STEP_M = 20000;

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
    // Canonical (1.00, exactly symmetric) UP to 3.00, a quarter a step -- the whole interval
    // Task 2 swept, and 2.00 (the preset's) lands on the lattice. Written `position / 4`
    // rather than `position * 0.25` for the reason below.
    collisionAsymmetry: {
      min: 0,
      max: ASYMMETRY_STEPS,
      toValue: (position) =>
        canonical.collisionAsymmetry + position / ASYMMETRY_STEP_QUARTERS,
      toPosition: (value) =>
        Math.round((value - canonical.collisionAsymmetry) * ASYMMETRY_STEP_QUARTERS),
      format: (value) => `${value.toFixed(2)}x`,
    },
    // Canonical (0.0, the field unsampled) UP to 0.9, a tenth a step.
    //
    // **`position / 10` and NOT `position * 0.1`.** `0.1 * 7` is 0.7000000000000001 and the
    // preset's depth is 0.7, so the multiplied form would give a slider that cannot express
    // the value its own preset button sets -- the panel-default defect this viewer has now
    // shipped four times, arriving through a new door.
    structureDepth: {
      min: 0,
      max: DEPTH_STEPS,
      toValue: (position) => canonical.structureDepth + position / DEPTH_STEP_TENTHS,
      toPosition: (value) =>
        Math.round((value - canonical.structureDepth) * DEPTH_STEP_TENTHS),
      format: (value) => value.toFixed(1),
    },
    // Canonical (120 km) DOWN to 40 km, 20 km a step. Five positions.
    //
    // **One of the five is dead and it is the one Ruling 1 requires.** The measured working
    // band is 40-80 km; 120 km is the canonical placeholder, which `structureAt` never reads
    // while the depth is zero, and position 0 has to be canonical bit-for-bit or an untouched
    // panel writes a wavelength into every shared link and takes the reload off the engine's
    // `None` path. So the travel starts at a value that does nothing, and stops well short of
    // the 250 km the table proves does nothing either.
    structureWavelengthM: {
      min: 0,
      max: WAVELENGTH_STEPS,
      toValue: (position) =>
        canonical.structureWavelengthM - position * WAVELENGTH_STEP_M,
      toPosition: (value) =>
        Math.round((canonical.structureWavelengthM - value) / WAVELENGTH_STEP_M),
      format: (value) => `${(value / 1000).toFixed(0)} km`,
    },
  };
}

/// The same six slider controls as `PANEL_RANGES` rows, **in tectonic units rather than positions**,
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
  return TECTONIC_SLIDERS.map((field) => {
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
