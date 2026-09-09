//! The relief channel, as JavaScript sees it: field order, slider travel, and the
//! query-string mapping. **No DOM, no Cesium, no engine instance** -- everything here is a
//! pure function over plain numbers, which is why `viewer/test/relief-params.test.mjs` can
//! hold it against the real wasm without a browser.
//
// # Why this module exists at all
//
// Two hazards, both of which have already bitten this repository, and both of which are
// hazards of *duplication* rather than of logic:
//
// 1. **The panel's defaults drifted from the engine's.** `controls.js`'s own comment records
//    it: the panel offered a -9000/6000 elevation ramp while `main.js` had narrowed to
//    -7000/2400, so opening the panel and pressing generate silently reverted the ramp. The
//    fix there was to copy the right numbers; the fix here is to have no numbers to copy.
//    **There is no relief default literal anywhere in the viewer.** Every default, and both
//    ends of two of the three sliders, is read from `wb_relief_preset` at boot -- the same
//    `ReliefParams::canonical()` and `ReliefParams::hills()` the engine itself uses.
// 2. **A second copy of `hills()`'s three values would drift the first time one is
//    retuned.** The slice's pre-flight conflict scan flagged exactly this pairing before any
//    task ran. So the preset button below sends the engine's own record back to the engine;
//    it does not restate `600`, `-0.7` and `0.65`.
//
// # Why the sliders are integer-stepped
//
// Because `0.7 - 14 * 0.05` is `-1.1e-16`, not `0.0`. `quieting_strength = 0` is the setting
// at which the quieting term is *off*, which is the one value on that axis with a stated
// meaning, and a slider that can only get within 1e-16 of it cannot say so. An `<input
// type="range">` computes its value the same way (`min + n * step`), so the widget carries
// integer positions and this module maps them to floats through an expression that lands on
// the named values exactly. The engine-side sweep
// (`the_calibrated_slider_travel_is_swept_at_every_step_the_widget_can_produce`) walks the
// same 46 / 29 / 26 values and asserts the same thing from the other side.

/// f64 per relief record, and the field names in `wasm.rs`'s `WB_RELIEF_STRIDE` order. That
/// order is the ABI: `wb_relief_preset` writes it and `wb_relief_check` reads it.
export const RELIEF_STRIDE = 10;
export const RELIEF_FIELDS = [
  "canonicalWavelengthM",
  "coarsestWavelengthM",
  "abyssalM",
  "shelfM",
  "coastM",
  "interiorM",
  "mountainM",
  "quietingStrength",
  "quietingScaleM",
  "octavePersistence",
];

/// `wb_relief_preset` selectors, mirrored from `wasm.rs`.
export const RELIEF_PRESET = { canonical: 0, hills: 1 };

/// The lacunarity of `detail.rs`'s octave schedule: each band is half the wavelength of the
/// one before, so the ratio is exactly 2. Not a tuning parameter -- it is what `plan`'s
/// `wavelength *= 0.5` means.
export const LACUNARITY = 2;

/// The Hurst exponent this persistence corresponds to, `H = ln(1/p) / ln(lacunarity)`.
///
/// This is the number with meaning outside this engine: 0.65 is an implementation detail of
/// `plan`'s schedule, H is a measured property of real ground. Gagnon, Lovejoy & Schertzer
/// measured H ~= 0.6-0.71 across four DEMs, which is [`HURST_BAND`] below and is what the
/// readout marks.
export function hurst(persistence) {
  return Math.log(1 / persistence) / Math.log(LACUNARITY);
}

/// Real terrain's measured Hurst band -- the target the persistence slider is aimed at.
export const HURST_BAND = { low: 0.6, high: 0.71 };

/// How far past canonical the persistence slider travels, in 0.01 steps.
///
/// **The one calibration bound that is not read from the engine, and it is measured rather
/// than chosen.** Task 2 swept persistence at 0.50, 0.65, 0.71 and 0.75 and stopped there;
/// 0.75 is the top of that sweep, so it is the top of the travel. Beyond it nothing has been
/// measured, and a slider whose upper half is unmeasured is a slider that invites a report
/// nobody can reproduce. 25 steps of 0.01 from canonical's 0.50 lands on 0.75, and on
/// `hills()`'s 0.65 exactly at step 15 (checked in f64, not assumed:
/// `0.5 + 15 * 0.01 === 0.65`).
export const PERSISTENCE_STEPS = 25;

/// The three parameters the panel drives, and the query-string name each answers to.
///
/// Three of ten. The other seven -- the two wavelengths, the four non-mountain amplitudes
/// and the quieting scale -- are reachable through the export but have no widget: Task 2
/// measured the three below and nothing else, and a slider whose travel nobody has measured
/// is a slider nobody can aim.
export const RELIEF_CONTROLS = ["mountainM", "quietingStrength", "octavePersistence"];
export const RELIEF_PARAM_NAMES = {
  mountainM: "mountainM",
  quietingStrength: "quieting",
  octavePersistence: "persistence",
};

/// The slider travel for each driven parameter, derived from the engine's own two presets.
///
/// Each entry is an integer range plus the map from a slider position to a relief value.
/// `min`/`max` are positions, not values.
///
/// - **`mountainM`: canonical (150) to `hills()` (600), 10 m a step.** Linear in the widget
///   because it is linear in effect: Task 2's peak population reads 10.32 / 20.18 / 30.04 /
///   39.90 m of relief at x1 / x2 / x3 / x4, which is a straight line through the origin to
///   within 3%. **This is a roughness budget on high ground, not a mountain's height** --
///   Ruling 4 -- and the panel labels it as roughness for that reason.
/// - **`quietingStrength`: canonical (+0.7) through 0 to `hills()` (-0.7), 28 steps.** Zero
///   is the midpoint and is landable exactly; see the module note above for why that needs
///   an integer widget.
/// - **`octavePersistence`: canonical (0.50) to Task 2's swept top (0.75), 0.01 a step.**
///   Linear over that quarter, per the plan's own instruction. Measured over it: worst
///   gradient runs 1.194% -> 5.196% on Task 2's land population, so a slider spanning
///   0.0-1.0 would have put that entire live range inside a quarter of its travel.
///
/// Every bound but `PERSISTENCE_STEPS` comes from `canonical` and `hills`, which come from
/// the engine.
export function sliderTravel(canonical, hills) {
  const mountainSteps = Math.round((hills.mountainM - canonical.mountainM) / 10);
  return {
    mountainM: {
      min: 0,
      max: mountainSteps,
      toValue: (position) => canonical.mountainM + position * 10,
      toPosition: (value) => Math.round((value - canonical.mountainM) / 10),
      format: (value) => `${value.toFixed(0)} m`,
    },
    quietingStrength: {
      min: -14,
      max: 14,
      // `canonical * n / 14` rather than `canonical - n * 0.05`: the second never reaches
      // exactly 0, and both ends still land exactly on the two presets' own values.
      toValue: (position) => (canonical.quietingStrength * position) / 14,
      toPosition: (value) => Math.round((value * 14) / canonical.quietingStrength),
      format: (value) => (value === 0 ? "0 (off)" : value.toFixed(2)),
    },
    octavePersistence: {
      min: 0,
      max: PERSISTENCE_STEPS,
      toValue: (position) => canonical.octavePersistence + position * 0.01,
      toPosition: (value) => Math.round((value - canonical.octavePersistence) / 0.01),
      format: (value) => `${value.toFixed(2)} · H ${hurst(value).toFixed(2)}`,
    },
  };
}

/// A preset object as a flat record in ABI order, ready for `wb_world_new_relief`.
export function toRecord(relief) {
  return RELIEF_FIELDS.map((name) => relief[name]);
}

/// A flat record back into a named object.
export function fromRecord(record) {
  const out = {};
  RELIEF_FIELDS.forEach((name, index) => { out[name] = record[index]; });
  return out;
}

/// The relief block a query string asks for, over the engine's own canonical block -- or
/// **`null`, meaning the canonical path**, which is `None` in the engine and is byte-for-byte
/// today's world.
///
/// **RULING 1 lives in this function.** A page opened with no relief parameters, and a page
/// whose relief parameters all happen to equal canonical's, both return `null` here, so
/// `wb_world_new_relief` gets a null pointer and takes the same path `wb_world_new` does.
/// Nothing about the untouched viewer changes because this module exists.
///
/// A parameter that is present but not a finite number is **ignored rather than forwarded**:
/// `?quieting=banana` is a typo in a shared link, and answering it with a refused world
/// would turn a typo into a blank page. Values that are numbers but out of domain *are*
/// forwarded, and the engine refuses them -- that is a caller asking for something the engine
/// declines, which is different from a caller not asking for anything.
export function reliefFromParams(params, canonical) {
  const relief = { ...canonical };
  let touched = false;
  for (const field of RELIEF_CONTROLS) {
    const name = RELIEF_PARAM_NAMES[field];
    if (!params.has(name)) continue;
    const value = Number(params.get(name));
    if (!Number.isFinite(value)) continue;
    if (value === canonical[field]) continue;
    relief[field] = value;
    touched = true;
  }
  return touched ? relief : null;
}

/// The query-string fields for a chosen relief block, dropping every field still at
/// canonical so a shared link carries only what was actually moved.
export function reliefToParams(relief, canonical) {
  const out = {};
  for (const field of RELIEF_CONTROLS) {
    const name = RELIEF_PARAM_NAMES[field];
    out[name] = relief[field] === canonical[field] ? null : String(relief[field]);
  }
  return out;
}
