//! The peak (seamount) channel, as JavaScript sees it: field order, the one calibrated slider's
//! travel, and the query-string mapping. **No DOM, no Cesium and no engine instance** --
//! everything here is a pure function over plain numbers, exactly as `coast-params.js` and
//! `gully-params.js` are, which is why `viewer/test/peak-params.test.mjs` can hold it against the
//! real wasm without a browser.
//
// # Why this module exists at all
//
// Tasks 1 through 5 grew a cellular seamount field -- `PeakParams` and
// `Tectonics::peak_offset_m`, a lattice of jittered candidate nodes that stand islands (or
// submerged shoals) up out of deep ocean -- wired it through `Tectonics` and `Surface`, and
// exposed it across the wasm boundary as `wb_world_new_peak`, `wb_peak_preset` and
// `wb_peak_check`. And then, exactly as `CoastParams` and `GullyParams` both did before it, it
// **stopped at the boundary**: verified, measured, and unreachable from the viewer. No export
// call, no field, no slider. This module is the other half of that door.
//
// # There is no peak default literal anywhere in the viewer
//
// The same hazard `coast-params.js` and `gully-params.js` both open with, and the same answer.
// The density slider is **anchored on the engine's own `PeakParams::canonical()`**, read across
// the boundary through `wb_peak_preset` at boot, and the preset button sends back
// `wb_peak_preset(volcanic)`'s own five numbers. Nothing in `viewer/` writes down 8000, 0.36,
// 31500, 2500 or 45000, and `peak-params.test.mjs` asserts they are not.
//
// # Why the slider carries an integer position
//
// For the reason `coast-params.js` and `gully-params.js` both give at length: an
// `<input type="range">` computes its value as `min + n * step`, so a value-stepped widget can
// only reach the lattice that arithmetic produces, and a default off that lattice is silently
// replaced -- the defect `panelFieldFaults()` exists for. The widget carries an integer position
// and this module maps it to a density through DIVISION (`position / 100`), never through
// multiplication: `0.01 * 36` and `36 / 100` happen to be the same double in this build --
// checked, not assumed, and they also agreed at the pre-calibration density of 0.11 -- but
// division is the form the other three channels settled on for a reason that outlives any one
// preset's value, and hundredths are what let `density` (domain `[0, 1]`) and the preset's own
// `0.36` both land on the lattice exactly: position 100 and position 36.
//
// # The joint invariant, and the choice this module makes about it
//
// `reach_m <= lattice_m` is a real bound `wasm.rs`'s `peak_is_admissible` enforces, and it is
// joint rather than per-field: neither `reach_m`'s nor `lattice_m`'s own domain check can see it.
// Both fields are DRIVEN (`PEAK_CONTROLS` carries all five, so a shared link can set either) but
// **neither has a measured effect table**, so neither gets a slider -- the same reasoning
// `COAST_SLIDERS` and `GULLY_SLIDERS` give for the fields beside their own one lever. With no
// widget moving either field, the panel cannot silently walk a drag across the boundary; the
// invariant is only reachable by hand-editing the query string, which is exactly the shape
// `coastFromParams`'s own doc calls out: "a caller asking for something the engine declines is a
// different thing from a caller not asking for anything." So this module does not re-derive the
// bound -- a second copy of a bound is a second chance to disagree with it, the same posture
// `checkCoast` and `checkGully` both take on their own joint bounds -- and `engine.checkPeak`
// asks `wb_peak_check` on the same buffer instead. **The panel surfaces the refusal rather than
// silently building an inert world**: `controls.js`'s `peakAdmissibleNote` prints "the engine
// will refuse this block" exactly as `coastAdmissibleNote` and `gullyAdmissibleNote` do, so an
// owner who sets `?peakReach=` above `?peakLattice=` in a shared link is told why nothing rose,
// rather than watching a density slider all the way to 1.0 with no islands and no explanation.

/// f64 per peak record, and the field names in `wasm.rs`'s `WB_PEAK_STRIDE` order. That order
/// is the ABI: `wb_peak_preset` writes it and `wb_peak_check` reads it -- and it is
/// `PeakParams`'s own declaration order in `tectonics.rs`, which that struct's doc comment
/// states IS the ABI. There is no type error for a density written into the height slot, which
/// is why this order is imported by `engine.js` rather than restated there.
export const PEAK_STRIDE = 5;
export const PEAK_FIELDS = [
  "height_m",
  "density",
  "reach_m",
  "min_depth_m",
  "lattice_m",
];

/// `wb_peak_preset` selectors, mirrored from `wasm.rs`.
///
/// **The preset crosses as FIELDS, never as a name.** `engine.peakPreset("volcanic")` returns
/// five numbers; the panel puts the one it turns on -- `density` -- on a slider and prints the
/// other four, so the owner SEES what the preset asked for. That is Ruling 7 of the relief
/// slice, held here the way `COAST_PRESET` and `GULLY_PRESET` hold it: `peak-params.test.mjs`'s
/// "no peak number is written down twice in the viewer" strips the comments out of all four of
/// this channel's modules -- this file, `controls.js`, `main.js` and `engine.js` -- and asserts
/// the preset's own distinctive number appears in none of them, plus the same check over the
/// source text of `peakTravel`, `peakPanelFields`, `peakFromParams` and `peakToParams`.
export const PEAK_PRESET = { canonical: 0, volcanic: 1 };

/// The parameters the panel drives, and the query-string name each answers to.
///
/// **All five**, the same shape `COAST_CONTROLS` and `GULLY_CONTROLS` both take: a shared link
/// that could not carry the whole preset would be a link that loads a different planet from the
/// one it was copied off. `wasm.rs`'s `peak_is_admissible` sweeps every one of the five across
/// its own documented domain, plus the joint bound between `reach_m` and `lattice_m` this
/// module's header explains.
export const PEAK_CONTROLS = [
  "height_m",
  "density",
  "reach_m",
  "min_depth_m",
  "lattice_m",
];

/// The subset of `PEAK_CONTROLS` that gets a WIDGET. **One of the five, and it is the field
/// `PeakParams::canonical()` and `PeakParams::volcanic()` differ in and nothing else.**
///
/// This project's rule -- written into `TECTONIC_SLIDERS`, `COAST_SLIDERS` and `GULLY_SLIDERS`'
/// own comments -- is that **a slider whose travel nobody has measured is a slider nobody can
/// aim**. `height_m`, `reach_m`, `min_depth_m` and `lattice_m` have no swept effect table (Task
/// 7's survey is what would produce one, per `VOLCANIC_HEIGHT_M`'s and `VOLCANIC_DENSITY`'s own
/// "provisional, pending Task 7's survey" doc comments in `tectonics.rs`), so none of the four
/// gets a widget here. They are still driven: the preset sets them, the query string carries
/// them, and the panel prints them as a readout, so a preset that changed something the panel
/// never mentioned cannot happen.
///
/// `density` is also the field that turns the whole term on -- `canonical()`'s density is
/// exactly 0.0 and `Tectonics::peak_offset_m` returns before it ever touches the lattice at that
/// value, the same early-return shape `COAST_SLIDERS`' amplitude and `GULLY_SLIDERS`' amplitude
/// both describe -- so the one slider this channel gets is also its off switch.
export const PEAK_SLIDERS = ["density"];

/// The driven fields that have no widget and are therefore PRINTED, derived from the two lists
/// above rather than written as a third.
///
/// **A preset that changed something the panel never mentioned would be a parameter the owner
/// cannot see**, which is the defect this whole family of slices exists to fix. `controls.js`
/// builds its readout from this, and `peak-params.test.mjs` asserts that the union of
/// `PEAK_SLIDERS` and this is exactly `PEAK_CONTROLS` with nothing counted twice -- so a sixth
/// field added to the channel cannot arrive silently, and an inverted filter here cannot pass.
export function peakReadoutFields() {
  return PEAK_CONTROLS.filter((field) => !PEAK_SLIDERS.includes(field));
}

export const PEAK_PARAM_NAMES = {
  density: "peak",
  height_m: "peakHeight",
  reach_m: "peakReach",
  min_depth_m: "peakMinDepth",
  lattice_m: "peakLattice",
};

/// Hundredths of a density, 0 through 100. **`position / 100` and NOT `position * 0.01`**, for
/// the reason `coastTravel`'s own comment gives at length: the multiplied form is not guaranteed
/// to land on the same double the engine's own literal parses to. Hundredths because the domain
/// ceiling (`density` is `[0, 1]`, `WB_MAX_PEAK_DENSITY` in `wasm.rs`) and the shipped preset's
/// own value must both land on the lattice exactly.
export const DENSITY_STEPS = 100;
export const DENSITY_STEP_HUNDREDTHS = 100;

/// The slider travel for the one driven parameter with a widget, derived from the engine's own
/// canonical block.
///
/// One entry, because one field is the off switch and the other four have no measured travel.
/// `min`/`max` are positions, not values, and position 0 is canonical.
export function peakTravel(canonical) {
  return {
    // Canonical (0.0, the term unsampled) UP to the domain ceiling, a hundredth a step.
    //
    // **Position 0 is a dead setting and it is the one Ruling 1 requires.** At density 0
    // `Tectonics::peak_offset_m` returns before it ever samples the lattice, so the slider
    // starts at a value that does nothing -- and it has to, because position 0 must be
    // canonical bit-for-bit or an untouched panel writes a density into every shared link and
    // takes the reload off the engine's `None` path.
    density: {
      min: 0,
      max: DENSITY_STEPS,
      toValue: (position) => canonical.density + position / DENSITY_STEP_HUNDREDTHS,
      toPosition: (value) =>
        Math.round((value - canonical.density) * DENSITY_STEP_HUNDREDTHS),
      format: (value) => value.toFixed(2),
    },
  };
}

/// The slider control as a `PANEL_RANGES` row, **in density units rather than positions**, so
/// `panelFieldFaults()` can be run over it.
///
/// The widget itself carries integer positions, where a default off the lattice is impossible
/// by construction -- but "impossible by construction" is what was said about the radius slider
/// too, and that one shipped. This states the travel in the units the domain was documented in
/// and asks the check the question it exists to ask: **can this slider express its own
/// default?** `controls.js` runs this before enabling the slider and `peak-params.test.mjs` runs
/// it against the shipped `.wasm`, so it is a production check and not only a test.
export function peakPanelFields(canonical) {
  const travel = peakTravel(canonical);
  return PEAK_SLIDERS.map((field) => {
    const { min, max, toValue } = travel[field];
    const low = Math.min(toValue(min), toValue(max));
    const high = Math.max(toValue(min), toValue(max));
    // The lattice step in density units, taken from the map rather than restated: one position,
    // measured.
    const step = Math.abs(toValue(1) - toValue(0));
    return { query: PEAK_PARAM_NAMES[field], min: low, max: high, step, value: canonical[field] };
  });
}

/// A peak block as a flat record in ABI order, ready for `wb_world_new_peak`.
export function peakToRecord(peak) {
  return PEAK_FIELDS.map((name) => peak[name]);
}

/// A flat record back into a named object.
export function peakFromRecord(record) {
  const out = {};
  PEAK_FIELDS.forEach((name, index) => { out[name] = record[index]; });
  return out;
}

/// The peak block a query string asks for, over the engine's own canonical block -- or **`null`,
/// meaning the canonical path**, which is `None` in the engine and is byte-for-byte today's
/// picture: no seamount field, no islands, no shoals.
///
/// **RULING 1 lives in this function**, exactly as it lives in `coastFromParams` and
/// `gullyFromParams`. A page opened with no peak parameters, and a page whose peak parameters
/// all happen to equal canonical's, both return `null` here, so `wb_world_new_peak` gets a null
/// pointer and takes the same path `wb_world_new_gully` does.
///
/// A parameter that is present but not a finite number is **ignored rather than forwarded**:
/// `?peak=banana` is a typo in a shared link, and answering it with a refused world would turn a
/// typo into a blank page. Values that are numbers but out of domain -- or that satisfy every
/// per-field domain and still break the `reach_m <= lattice_m` joint invariant -- *are*
/// forwarded, and the engine refuses them: a caller asking for something the engine declines is
/// a different thing from a caller not asking for anything.
export function peakFromParams(params, canonical) {
  const peak = { ...canonical };
  let touched = false;
  for (const field of PEAK_CONTROLS) {
    const name = PEAK_PARAM_NAMES[field];
    if (!params.has(name)) continue;
    const value = Number(params.get(name));
    if (!Number.isFinite(value)) continue;
    if (value === canonical[field]) continue;
    peak[field] = value;
    touched = true;
  }
  return touched ? peak : null;
}

/// The query-string fields for a chosen peak block, dropping every field still at canonical so a
/// shared link carries only what was actually moved.
export function peakToParams(peak, canonical) {
  const out = {};
  for (const field of PEAK_CONTROLS) {
    const name = PEAK_PARAM_NAMES[field];
    out[name] = peak[field] === canonical[field] ? null : String(peak[field]);
  }
  return out;
}

/// Boot's answer to the refusal Blocker 1 names: what `wb_world_new_peak` should actually be
/// handed, given what a query string asked for (`peakFromParams`'s own return) and whether the
/// engine accepts it (the caller's own `wb_peak_check` verdict, so this module keeps its rule of
/// asking the engine rather than re-deriving the joint bound itself).
///
/// **The property this closes:** `requested` can be a block a hand-edited `?peakReach=` /
/// `?peakLattice=` breaks the `reach_m <= lattice_m` invariant on, and that is the ONLY path that
/// can reach it -- neither field has a widget, so no panel action alone can produce one.
/// Unfiltered, that block reaches `wb_world_new_peak`, which throws; `main.js`'s own
/// `boot().catch(...)` swallows that throw without ever publishing `window.__wb`, so the panel
/// never gets far enough to run its own refusal check and the owner sees the same generic "engine
/// unavailable" text a truly dead engine would produce -- worse than silence, because it blames
/// the wrong thing.
///
/// So a refused `requested` is never the block that reaches the constructor: `forConstructor` is
/// `null` instead, the canonical, island-free ocean, and `refused` says so. `requested` itself is
/// not discarded -- `main.js` keeps it so the panel can still show what was actually asked for and
/// its own `checkPeak`-backed note can still fire on it live.
export function peakBootPlan(requested, admissible) {
  const refused = requested !== null && !admissible;
  return { forConstructor: refused ? null : requested, refused };
}
