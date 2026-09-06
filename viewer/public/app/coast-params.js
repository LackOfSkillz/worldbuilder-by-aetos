//! The coast channel, as JavaScript sees it: field order, the calibrated slider travel, and the
//! query-string mapping. **No DOM, no Cesium, no engine instance** -- everything here is a pure
//! function over plain numbers, exactly as `relief-params.js` and `tectonic-params.js` are, which
//! is why `viewer/test/coast-params.test.mjs` can hold it against the real wasm without a browser.
//
// # Why this module exists at all
//
// The owner's coastlines are smooth. A coastline that gets *longer* the finer you measure it is
// the thing a real one has and a four-octave land/sea field does not: today's has a length ratio
// of 1.00 across an eightfold change of ruler, and two inlet heads on the whole planet.
//
// The engine grew the term that fixes that -- `CoastParams`, a separate roughening amplitude
// windowed by distance from the shore and applied AFTER calibration, so land fraction cannot move
// -- and then **stopped at the WASM boundary for a whole task.** It was verified, measured, and
// unreachable: no export, no field, no slider. This module is the other half of that door.
//
// # There is no coast default literal anywhere in the viewer
//
// The same hazard `relief-params.js` opens with, and the same answer. The amplitude slider is
// **anchored on the engine's own `CoastParams::canonical()`**, read across the boundary through
// `wb_coast_preset` at boot, and the preset button sends back `wb_coast_preset(fractal)`'s own six
// numbers. `0.35`, `20`, `4`, `0.5` and `2` are not written down in `viewer/`, and
// `coast-params.test.mjs` asserts they are not.
//
// # Why the slider carries an integer position
//
// For the reason `relief-params.js` gives at length: an `<input type="range">` computes its value
// as `min + n * step`, so a value-stepped widget can only reach the lattice that arithmetic
// produces, and a default off that lattice is silently replaced -- the defect `panelFieldFaults()`
// exists for, which has now been found four times in this viewer and the fourth time BY that
// check. The widget carries an integer position and this module maps it to an amplitude through an
// expression that lands on canonical **exactly** at position 0.
//
// That exactness is load-bearing rather than tidy: `coastToParams` drops every field still equal
// to canonical, so a page whose slider was never touched writes no coast parameter at all and the
// reload takes the engine's `None` path -- RULING 1, which is that the default world cannot move.

/// f64 per coast record, and the field names in `wasm.rs`'s `WB_COAST_STRIDE` order. That order
/// is the ABI: `wb_coast_preset` writes it and `wb_coast_check` reads it.
export const COAST_STRIDE = 6;
export const COAST_FIELDS = [
  "amplitude",
  "windowSpreads",
  "frequency",
  // An INTEGER carried in an f64 slot, and a **loop bound**. The engine refuses a word that is
  // not finite, exactly integral and inside its ceiling, because `as u32` in Rust saturates: 1e300
  // would otherwise arrive as four billion octaves of noise **per sample**, which is a hung tab
  // rather than a slow world. Measured natively in release at ~1.6e-8 s an octave, a saturated
  // count is ~68 seconds for ONE elevation sample.
  "octaves",
  "gain",
  "lacunarity",
];

/// `wb_coast_preset` selectors, mirrored from `wasm.rs`.
///
/// **The preset crosses as FIELDS, never as a name.** `engine.coastPreset("fractal")` returns six
/// numbers; the panel puts the one it has calibrated travel for on a slider and prints the other
/// five, so the owner SEES what the preset asked for. That is Ruling 7 of the relief slice, and
/// `coast-params.test.mjs` enforces it here the same way the other two modules' tests do: it
/// strips the comments out of this file and `controls.js` and asserts the preset's numbers appear
/// in neither.
export const COAST_PRESET = { canonical: 0, fractal: 1 };

/// The parameters the panel drives, and the query-string name each answers to.
///
/// **All six, unlike the tectonic channel.** There is no field here with no coverage: the engine
/// side sweeps every one of the six across its whole domain and beyond, the cross-product tests
/// drive `frequency`, `octaves` and `lacunarity` together, and
/// `a_chosen_coast_block_actually_moves_the_ground_it_claims_to` witnesses `amplitude` and
/// `frequency` moving the ground at eight coastal probes. A parameter a shared link can carry is
/// one the engine has been proved to read.
export const COAST_CONTROLS = [
  "amplitude",
  "windowSpreads",
  "frequency",
  "octaves",
  "gain",
  "lacunarity",
];

/// The subset of `COAST_CONTROLS` that gets a WIDGET. **One of the six.**
///
/// `amplitude` is the field Task 5 calibrated, and it is the only one it calibrated. The other
/// five have no measured travel, and this project's own rule -- written into
/// `TECTONIC_SLIDERS`' comment when four tectonic fields were left off for the same reason -- is
/// that **a slider whose travel nobody has measured is a slider nobody can aim**. They are driven:
/// the preset sets them, the query string carries them, and the panel prints them as a readout, so
/// a preset that changed something the panel never mentioned cannot happen.
///
/// It is also the field that the preset moves. `CoastParams::fractal()` differs from `canonical()`
/// in `amplitude` and in nothing else -- pinned engine-side by
/// `wb_coast_preset_hands_back_the_engines_own_blocks_and_nothing_else` -- so a single slider is an
/// honest presentation of the preset rather than a partial one.
export const COAST_SLIDERS = ["amplitude"];

/// The driven fields that have no widget and are therefore PRINTED, derived from the two lists
/// above rather than written as a third.
///
/// **A preset that changed something the panel never mentioned would be a parameter the owner
/// cannot see**, which is the defect this whole slice exists to fix. `controls.js` builds its
/// schedule readout from this, and `coast-params.test.mjs` asserts that the union of
/// `COAST_SLIDERS` and this is exactly `COAST_CONTROLS` with nothing counted twice -- so a seventh
/// field added to the channel cannot arrive silently, and an inverted filter here cannot pass.
export function coastReadoutFields() {
  return COAST_CONTROLS.filter((field) => !COAST_SLIDERS.includes(field));
}

export const COAST_PARAM_NAMES = {
  amplitude: "coast",
  windowSpreads: "coastBand",
  frequency: "coastFreq",
  octaves: "coastOctaves",
  gain: "coastGain",
  lacunarity: "coastLacunarity",
};

/// **THE CALIBRATION.** Measured on the owner's own world, not the default one, and it is the
/// reason this slider stops where it does.
///
/// - **Population:** seed 562423712, radius 4,500,000 m, 28 plates, land fraction 0.16 -- the world
///   from their screenshot. An equirectangular grid at **25 km** spacing, classified by
///   `Continentality::above_shore > 0`.
/// - **Method:** coastline length as a Cauchy-Crofton boundary-edge sum, reported as a **ratio**
///   against the same grid at amplitude 0 so the estimator's raster bias divides out; islands and
///   inland water as 4-connected components with longitude wrapping; **inlet heads** as water
///   samples with three or more land neighbours out of four, counted as components -- a bay, fjord
///   or strait head one sample wide.
/// - **Host:** this repository's machine, native release build.
///   `crates/worldbuilder-engine/src/bin/coastline_survey.rs`.
///
///     amplitude   0.00   0.05   0.10   0.15   0.35   0.50   0.75   1.00   1.50
///     len ratio   1.000  1.029  1.087  1.191  1.591  1.922  2.429  2.884  3.572
///     inlet heads     2      9     33     62    175    258    336    509    629
///     isl >=25k km2   9     10     10     12     13     14     17     23     28
///     isl >=100k km2  5      5      5      9      9      9     10      9     10
///     inland >=25k    2      1      1      1      1      3     10      9     14
///     land fraction  .15982 .15965 .15971 .15956 .15965 .16015 .16069 .16146 .16226
///
/// **And the metric is discriminating rather than merely sensitive.** Beside every fractal column
/// Task 5 ran a deliberately smooth control -- one octave at frequency 2.0, a term *coarser* than
/// the base field's own finest octave, at the identical amplitude. It displaces the coast just as
/// far and adds no structure to it, and it reads **1.02-1.03 flat across four ruler lengths**
/// against 1.36 -> 1.64 *rising* for the fractal term. A measure that reported "longer" for any
/// perturbation would have put the two together.
export const MEASURED_COAST = [
  { amplitude: 0.00, lengthRatio: 1.000, inletHeads: 2, largeIslands: 5, largestShare: 87.8 },
  { amplitude: 0.05, lengthRatio: 1.029, inletHeads: 9, largeIslands: 5, largestShare: 87.8 },
  { amplitude: 0.10, lengthRatio: 1.087, inletHeads: 33, largeIslands: 5, largestShare: 87.7 },
  { amplitude: 0.15, lengthRatio: 1.191, inletHeads: 62, largeIslands: 9, largestShare: 48.6 },
  { amplitude: 0.35, lengthRatio: 1.591, inletHeads: 175, largeIslands: 9, largestShare: 48.4 },
  { amplitude: 0.50, lengthRatio: 1.922, inletHeads: 258, largeIslands: 9, largestShare: 48.2 },
  { amplitude: 0.75, lengthRatio: 2.429, inletHeads: 336, largeIslands: 10, largestShare: 46.9 },
];

/// **THE USEFUL BAND, as two numbers, from the table above.**
///
/// - **Visible from about 0.10.** Below it the coast lengthens by under 3% and the inlet count is
///   single digits -- the sub-pixel-wobble failure mode, measured rather than assumed.
/// - **Fragmenting above about 0.75.** From there the small-island count runs away (17 -> 23 -> 28)
///   while the >= 100,000 km2 count stays flat at 9-10, so the new components are *speckle*, not
///   new continents; inland water bodies multiply 1 -> 10 -> 14; and the land-fraction residual
///   grows monotonically to +0.24 pp.
///
/// **The travel is that band and no more.** `AMPLITUDE_STEPS / AMPLITUDE_STEP_TWENTIETHS` is
/// exactly 0.75, and 0.10 is the second position -- so fourteen of the sixteen positions are inside
/// the measured band. A slider whose useful range is a tenth of its travel is one nobody can aim,
/// and that is the failure this file exists to avoid rather than a phrase.
export const USEFUL_BAND = { low: 0.10, high: 0.75 };

/// Sixteen positions, 0 through 15, in twentieths of an amplitude.
///
/// **`position / 20` and NOT `position * 0.05`.** `0.05 * 7` is `0.35000000000000003` and the
/// preset's amplitude is `0.35`, so the multiplied form would give a slider that cannot express the
/// value its own preset button sets -- the panel-default defect this viewer has shipped four times,
/// arriving through the same door it arrived through for `structureDepth`. Twentieths because the
/// band's top, 0.75, and the preset's 0.35 must both land on the lattice exactly: 15/20 and 7/20.
export const AMPLITUDE_STEPS = 15;
export const AMPLITUDE_STEP_TWENTIETHS = 20;

/// The slider travel for each driven parameter, derived from the engine's own canonical block.
///
/// One entry, because one field has measured travel. `min`/`max` are positions, not values, and
/// position 0 is canonical.
export function coastTravel(canonical) {
  return {
    // Canonical (0.00, the term unsampled) UP to 0.75, a twentieth a step.
    //
    // **Position 0 is a dead setting and it is the one RULING 1 requires.** At amplitude 0
    // `Continentality::above_shore` returns before it touches the coast lattice at all, so the
    // slider starts at a value that does nothing -- and it has to, because position 0 must be
    // canonical bit-for-bit or an untouched panel writes an amplitude into every shared link and
    // takes the reload off the engine's `None` path. Position 1 (0.05) is measured as
    // below-visible; from position 2 (0.10) upward every setting is inside the band.
    amplitude: {
      min: 0,
      max: AMPLITUDE_STEPS,
      toValue: (position) => canonical.amplitude + position / AMPLITUDE_STEP_TWENTIETHS,
      toPosition: (value) =>
        Math.round((value - canonical.amplitude) * AMPLITUDE_STEP_TWENTIETHS),
      format: (value) => value.toFixed(2),
    },
  };
}

/// The slider control as a `PANEL_RANGES` row, **in coast units rather than positions**, so
/// `panelFieldFaults()` can be run over it.
///
/// The widget itself carries integer positions, where a default off the lattice is impossible by
/// construction -- but "impossible by construction" is what was said about the radius slider too,
/// and that one shipped. This states the travel in the units the calibration was measured in and
/// asks the check the question it exists to ask: **can this slider express its own default?** It is
/// a real question here, because `min`, `max` and `step` are all derived from the engine's
/// canonical through f64 arithmetic and only `value` is untouched by it.
///
/// `controls.js` runs this before enabling the slider and `coast-params.test.mjs` runs it against
/// the shipped `.wasm`, so it is a production check and not only a test.
export function coastPanelFields(canonical) {
  const travel = coastTravel(canonical);
  return COAST_SLIDERS.map((field) => {
    const { min, max, toValue } = travel[field];
    const low = Math.min(toValue(min), toValue(max));
    const high = Math.max(toValue(min), toValue(max));
    // The lattice step in coast units, taken from the map rather than restated: one position,
    // measured.
    const step = Math.abs(toValue(1) - toValue(0));
    return { query: COAST_PARAM_NAMES[field], min: low, max: high, step, value: canonical[field] };
  });
}

/// A coast block as a flat record in ABI order, ready for `wb_world_new_coast`.
export function coastToRecord(coast) {
  return COAST_FIELDS.map((name) => coast[name]);
}

/// A flat record back into a named object.
export function coastFromRecord(record) {
  const out = {};
  COAST_FIELDS.forEach((name, index) => { out[name] = record[index]; });
  return out;
}

/// The coast block a query string asks for, over the engine's own canonical block -- or **`null`,
/// meaning the canonical path**, which is `None` in the engine and is byte-for-byte today's
/// coastline.
///
/// **RULING 1 lives in this function**, exactly as it lives in `reliefFromParams` and
/// `tectonicFromParams`. A page opened with no coast parameters, and a page whose coast parameters
/// all happen to equal canonical's, both return `null` here, so `wb_world_new_coast` gets a null
/// pointer and takes the same path `wb_world_new` does.
///
/// A parameter that is present but not a finite number is **ignored rather than forwarded**:
/// `?coast=banana` is a typo in a shared link, and answering it with a refused world would turn a
/// typo into a blank page. Values that are numbers but out of domain *are* forwarded, and the
/// engine refuses them -- a caller asking for something the engine declines is a different thing
/// from a caller not asking for anything.
export function coastFromParams(params, canonical) {
  const coast = { ...canonical };
  let touched = false;
  for (const field of COAST_CONTROLS) {
    const name = COAST_PARAM_NAMES[field];
    if (!params.has(name)) continue;
    const value = Number(params.get(name));
    if (!Number.isFinite(value)) continue;
    if (value === canonical[field]) continue;
    coast[field] = value;
    touched = true;
  }
  return touched ? coast : null;
}

/// The query-string fields for a chosen coast block, dropping every field still at canonical so a
/// shared link carries only what was actually moved.
export function coastToParams(coast, canonical) {
  const out = {};
  for (const field of COAST_CONTROLS) {
    const name = COAST_PARAM_NAMES[field];
    out[name] = coast[field] === canonical[field] ? null : String(coast[field]);
  }
  return out;
}
