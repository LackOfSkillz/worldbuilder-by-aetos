//! The gully channel, as JavaScript sees it: field order, the one calibrated slider's travel, and
//! the query-string mapping. **No DOM, no Cesium, no engine instance** -- everything here is a
//! pure function over plain numbers, exactly as `relief-params.js`, `tectonic-params.js` and
//! `coast-params.js` are, which is why `viewer/test/gully-params.test.mjs` can hold it against the
//! real wasm without a browser.
//
// # Why this module exists at all
//
// The owner put our mountains beside satellite photographs of real ranges: *"our mountains look
// like they were painted in with a knife... they should look more like the 3rd and 4th image"*.
// What those photographs have is **dendritic V-notched valleys branching down the flanks, with
// snow following the ridge lines** -- and `.superpowers/sdd/notes/erosion-architecture-spike.md`
// measured that the stream graph is **661x too coarse** to ever carry them.
//
// So the engine grew the term that can: `GullyParams` and `Detail::gully_offset_m`, a drainage
// texture added inside `elevation_m`, steered on `grad(structural_m)` through a world-anchored
// lattice. It was built, swept, mutated and measured -- and then **stopped at the WASM boundary**,
// exactly as `CoastParams` did for a whole task before it. `wb_world_new_gully`,
// `wb_gully_preset` and `wb_gully_check` ship in the committed artifact; nothing under
// `viewer/public/app/` sent a gully block, so the owner saw no change at all. Their next words
// were *"wire the gully to the viewer so I can see it"*. This module is the other half of that
// door.
//
// # There is no gully default literal anywhere in the viewer
//
// The same hazard `relief-params.js` opens with, and the same answer. The one slider is
// **anchored on the engine's own `GullyParams::canonical()`**, read across the boundary through
// `wb_gully_preset` at boot, and the preset button sends back `wb_gully_preset(drainage)`'s own
// ten numbers. **Above all `0.005` is not written down in `viewer/`** -- it is the one number in
// this record that is a *measurement of this generator* rather than a preference, and
// `gully-params.test.mjs` asserts that neither this file's functions nor `controls.js` nor
// `main.js` contains it.
//
// # Why the slider carries an integer position
//
// For the reason `relief-params.js` gives at length and `coast-params.js` restates: an
// `<input type="range">` computes its value as `min + n * step`, so a value-stepped widget can
// only reach the lattice that arithmetic produces, and a default off that lattice is silently
// replaced -- the defect `panelFieldFaults()` exists for, found four times in this viewer and the
// fourth time BY that check. The widget carries an integer position and this module maps it to a
// value by DIVISION (`position / 20`), never by multiplying a position by a fraction:
// `0.05 * 7` is `0.35000000000000003`, and that is how the fourth one arrived.

/// f64 per gully record, and the field names in `wasm.rs`'s `WB_GULLY_STRIDE` order. That order
/// is the ABI: `wb_gully_preset` writes it and `wb_gully_check` reads it. There is no type error
/// for a crest exponent written into the amplitude slot, which is why this order is imported by
/// `engine.js` rather than restated there.
export const GULLY_STRIDE = 10;
export const GULLY_FIELDS = [
  // Metres of vertical displacement, and **the off switch**: `GullyParams::canonical()` is
  // `drainage()` with this field at 0.0, so there is one set of numbers in the engine and not
  // two, and `Detail::gully_offset_m` returns before it can add anything.
  "amplitudeM",
  // The pivot lattice's pitch. A divisor of the planet's radius on its way to a lattice index,
  // which is why the engine floors it at one metre rather than at zero.
  "cellM",
  // **The measured one.** A slope in m/m, and the number that decides whether this kernel bites
  // at all on a generator whose flanks are about a hundredth of what the published technique
  // assumes. See `MEASURED_SLOPE` below.
  "slopeReference",
  "stripesPerCell",
  "maxStripesPerCell",
  // The crest/floor exponent, and the only field with a swept effect table. See `MEASURED_CREST`.
  "crestSharpness",
  "gateElevationM",
  "gateElevationSpanM",
  "flatEnergyFloor",
  // The steering lattice's pitch. Second divisor in the record, guarded twice for the same
  // reason `cellM` is.
  "steerLatticeM",
];

/// `wb_gully_preset` selectors, mirrored from `wasm.rs`.
///
/// **The preset crosses as FIELDS, never as a name.** `engine.gullyPreset("drainage")` returns ten
/// numbers; the panel puts the one it has a swept effect table for on a slider and prints the
/// other nine, so the owner SEES what the preset asked for. That is Ruling 7 of the relief slice,
/// and `gully-params.test.mjs` enforces it here the way the other three modules' tests do: it
/// strips the comments out of this file and `controls.js` and asserts the preset's distinctive
/// numbers appear in neither.
export const GULLY_PRESET = { canonical: 0, drainage: 1 };

/// The parameters the panel drives, and the query-string name each answers to.
///
/// **All ten.** A shared link that could not carry the whole preset would be a link that loads a
/// different planet from the one it was copied off, and the engine sweeps every one of the ten --
/// 1,140 records on the per-field ladder, 738 accepted and 402 refused, pinned exactly -- plus a
/// 7 x 7 x 6 cross product over `cellM`, `steerLatticeM` and `slopeReference`, because all three
/// become a lattice index as `radius / length` and one abort in this file's sibling channel was
/// reachable only through a product of individually-admissible fields.
export const GULLY_CONTROLS = [
  "amplitudeM",
  "cellM",
  "slopeReference",
  "stripesPerCell",
  "maxStripesPerCell",
  "crestSharpness",
  "gateElevationM",
  "gateElevationSpanM",
  "flatEnergyFloor",
  "steerLatticeM",
];

/// The subset of `GULLY_CONTROLS` that gets a WIDGET. **One of the ten, and it is not the
/// amplitude.**
///
/// This project's rule -- written into `TECTONIC_SLIDERS`' comment when four tectonic fields were
/// left off, and into `COAST_SLIDERS`' when five coast fields were -- is that **a slider whose
/// travel nobody has measured is a slider nobody can aim**, and this viewer has shipped a control
/// whose useful band was a fraction of its travel. So the question asked of each field is not "is
/// it read" (the engine's sweep answers that for all ten) but **"is there a table of this field's
/// value against a measured effect"**. `.superpowers/sdd/notes/gully-kernel.md` contains exactly
/// one such table, its section 4.1, and it sweeps `crestSharpness`.
///
/// The three that are deliberately absent, each for a stated reason rather than for want of a
/// widget:
///
/// - **`amplitudeM`, and this is the one the owner will ask for first.** It is measured at
///   **one point**: at 60 m the gated flank population's local relief over a 2 km run has a
///   median of 112 m, inside Hammond's 80-160 m hills band, against 17 m with the kernel off.
///   One point is not a travel. The obvious move -- assume the relief scales linearly with the
///   amplitude and solve the band's ends -- is a derivation from an unmeasured proportionality,
///   and inventing a calibration is the failure this whole family of comments exists to prevent.
///   **An amplitude sweep is the next measurement this channel needs.** Until it exists the
///   preset button is how the amplitude moves, and the readout prints it.
/// - **`slopeReference`.** Its measured population is a property of the *terrain* (500,000 spiral
///   points on each of three worlds), not a table of the kernel's response, and the kernel's
///   response to it is measured at exactly **two points**: the reference 0.005, where the term
///   varies by 85.8 m of p95-p05 spread against a 60 m amplitude, and 1.0 -- the published
///   unnormalised form spelled as a parameter -- where `detail::tests::the_slope_scale_is_what_
///   stops_the_kernel_being_a_constant` requires it to come out **flat to under half a metre**.
///   Two points 200x apart, one of them dead, is a direction and not a travel. That is the same
///   shape as the suture pair, whose whole useful range was a single point.
///
///   **And leaving it off closes the hazard rather than merely declining it.** A slope-scale
///   slider anchored anywhere near the published order of magnitude would put the entire live
///   band in its first percent of throw. The number the owner needs to see is 0.005, and the
///   readout prints it beside its angle.
/// - **`cellM`, `stripesPerCell`, `maxStripesPerCell`, `gateElevationM`, `gateElevationSpanM`,
///   `flatEnergyFloor`, `steerLatticeM`.** Each is chosen on external ground (the photographs'
///   0.5-2 km feature band; `CANONICAL_WAVELENGTH_M`; `amplitude_m`'s own existing "high" curve;
///   the gradient probe's measured 2 km step) and none of them was swept for effect.
export const GULLY_SLIDERS = ["crestSharpness"];

/// The driven fields that have no widget and are therefore PRINTED, derived from the two lists
/// above rather than written as a third.
///
/// **A preset that changed something the panel never mentioned would be a parameter the owner
/// cannot see**, which is the defect this family of slices exists to fix -- and it matters more
/// here than on any previous channel, because the field this panel does NOT put on a slider is
/// the one that turns the whole term on. `controls.js` builds its readout from this, and
/// `gully-params.test.mjs` asserts that the union of `GULLY_SLIDERS` and this is exactly
/// `GULLY_CONTROLS` with nothing counted twice -- so an eleventh field added to the channel cannot
/// arrive silently, and an inverted filter here cannot pass.
export function gullyReadoutFields() {
  return GULLY_CONTROLS.filter((field) => !GULLY_SLIDERS.includes(field));
}

export const GULLY_PARAM_NAMES = {
  amplitudeM: "gully",
  cellM: "gullyCell",
  slopeReference: "gullySlope",
  stripesPerCell: "gullyStripes",
  maxStripesPerCell: "gullyMaxStripes",
  crestSharpness: "gullyCrest",
  gateElevationM: "gullyGate",
  gateElevationSpanM: "gullyGateSpan",
  flatEnergyFloor: "gullyFloor",
  steerLatticeM: "gullySteer",
};

/// **THE SLOPE MEASUREMENT, as the panel's readout quotes it.** Not a travel -- see
/// `GULLY_SLIDERS` -- but the number the owner has to be able to see, because it is the reason
/// this kernel works here at all.
///
/// - **Population:** 500,000 Fibonacci-spiral points per world, land taken as `structural_m > 0`,
///   on three worlds: `DEFAULT_WORLD` (145,771 land points), the owner's 4,500 km / 28-plate world
///   (81,743) and the erosion sweep's world (145,695).
/// - **Method:** `|grad(structural_m)|` by central difference in the local `TangentFrame` at a
///   **2 km step** -- the step the gradient probe measured this gradient's direction stable at to
///   a p95 of 0.02 degrees across a 26x range, so it is not a free parameter.
/// - **Host:** Windows 11, i9-13900HX, `cargo build --release` on the workspace's
///   determinism-first profile, single-threaded, native only.
///
///       world                 all land p50 / p95 / p99      high ground (>800 m) p90 / p95 / p99
///       DEFAULT_WORLD         0.000472 / 0.002340 / 0.004553    0.005206 / 0.005838 / 0.007434
///       owner's 4,500 km      0.000824 / 0.002767 / 0.006608    0.003375 / 0.005290 / 0.009688
///       erosion sweep world   0.000426 / 0.002200 / 0.003751    0.003424 / 0.003787 / 0.005323
///
/// **0.005 m/m is 0.2865 degrees**, and that is the whole point: the published unnormalised-`dir`
/// trick assumes something of order 0.5 m/m -- **a hundred times steeper than this planet's
/// flanks**. Handed 0.5, every `cos(dot(dir, d))` becomes the cosine of nearly nothing and the
/// kernel is a constant multiple of its gate, planet-wide. This project has shipped that failure
/// before: three colour blends in `relief.js` were dead against this terrain until a slice
/// measured them, and nine of thirty-three palette colours were unreachable because a noise
/// field's real standard deviation was a fifth of its nominal one.
export const MEASURED_SLOPE = {
  reference: 0.005,
  degrees: 0.2865,
  publishedAssumption: 0.5,
  worlds: [
    { world: "DEFAULT_WORLD", landP50: 0.000472, landP99: 0.004553, highP90: 0.005206, highP99: 0.007434 },
    { world: "owner's 4,500 km", landP50: 0.000824, landP99: 0.006608, highP90: 0.003375, highP99: 0.009688 },
    { world: "erosion sweep", landP50: 0.000426, landP99: 0.003751, highP90: 0.003424, highP99: 0.005323 },
  ],
};

/// **THE ONE SWEPT TABLE, and therefore the one slider.** Section 4.1 of the kernel note.
///
/// - **Population:** 400 flank points -- the steepest decile of high ground (>800 m) on
///   `DEFAULT_WORLD` -- each walked 40 steps of 60 m, 16,000 samples, comparing
///   `elevation_m(gully)` against `elevation_m(plain)` in metres. `crestSharpness` swept alone;
///   every other field at `drainage()`.
/// - **Host:** as `MEASURED_SLOPE`.
///
/// `mean` is the load-bearing column and its **sign** is the finding:
///
///       crestSharpness    p05      p50      p95     mean
///       0.50            -51.87   -23.78   +22.47   -20.45
///       0.70 (shipped)  -49.10   -13.14   +35.76   -10.83
///       0.85            -46.99    -5.97   +42.46    -4.84
///       1.00            -45.02    +0.42   +47.17    +0.33
///
/// **At an exponent of one the kernel is symmetric and it BLANKETS the flank** -- mean +0.33 m,
/// which is a roughness added everywhere and is precisely not what the photographs show. Below one
/// the mean goes negative: the term carves valleys and leaves the crests as narrow spines, which
/// is what puts snow on ridge lines rather than over everything. At 0.50 it removes 20 m of mean
/// height from every gated flank, which is a systematic lowering wearing a texture's name.
export const MEASURED_CREST = [
  { crestSharpness: 0.50, p05: -51.87, p50: -23.78, p95: 22.47, mean: -20.45 },
  { crestSharpness: 0.70, p05: -49.10, p50: -13.14, p95: 35.76, mean: -10.83 },
  { crestSharpness: 0.85, p05: -46.99, p50: -5.97, p95: 42.46, mean: -4.84 },
  { crestSharpness: 1.00, p05: -45.02, p50: 0.42, p95: 47.17, mean: 0.33 },
];

/// **THE CARVE BAND, as two numbers, from the table above**, and both ends are where the
/// measurement stops being informative rather than where the engine stops accepting.
///
/// - **`high: 1.00` -- the blanket end.** The mean crosses zero here; above it the term is a
///   symmetric roughness and there is no fall line in the picture. The engine would accept 100.
/// - **`low: 0.50` -- the lowering end.** 20 m of mean height removed from every gated flank is a
///   systematic subsidence, not a texture. The engine would accept 0.001.
///
/// **The travel is that band and no more**, which is the whole of the calibration story from the
/// panel's side: eleven positions, all eleven inside the band, and each of the four measured rows
/// lands on the lattice exactly.
export const CARVE_BAND = { low: 0.50, high: 1.00 };

/// **What the amplitude delivers, at the one amplitude it was measured at.** Quoted by the panel
/// beside the printed `amplitudeM` so the readout is a statement rather than a number.
///
/// - **Population:** the same 400 flank points. Local relief is `max - min` of
///   `elevation_m(.., None)` over an 11 x 11 grid at 200 m spacing -- a 2 km run, which is the run
///   **Hammond's landform classification** is defined over.
/// - **Hammond:** hills are 80-160 m of local relief over 2 km; low mountains start at 300 m.
///
/// This is texture on a generator whose mountains are tectonic (Ruling 4 of the relief-amplitude
/// slice; the peak on the owner's world is 98.9% structural). **It is not a claim to have made
/// mountains**, and the panel's note says so in those words.
export const MEASURED_RELIEF = {
  amplitudeM: 60.0,
  offP50: 17.03,
  onP50: 112.37,
  offMean: 17.89,
  onMean: 110.79,
  hammondHills: { low: 80, high: 160 },
  hammondMountainFloor: 300,
};

/// Eleven positions, 10 through 20, in twentieths of an exponent.
///
/// **`position / 20` and NOT `position * 0.05`.** `0.05 * 17` is `0.8500000000000001` and the
/// measured table has a row at `0.85`, so the multiplied form would give a slider that cannot land
/// on its own measurement -- the panel-default defect this viewer has shipped four times, arriving
/// through the same door it arrived through for `structureDepth` and for the coast amplitude.
/// Twentieths because all four measured rows and the preset's own 0.70 must land on the lattice
/// exactly: 10/20, 14/20, 17/20 and 20/20.
export const CREST_MIN_TWENTIETHS = 10;
export const CREST_MAX_TWENTIETHS = 20;
export const CREST_TWENTIETHS = 20;

/// The slider travel for each driven parameter, derived from the measured band.
///
/// One entry, because one field has a swept effect table. `min`/`max` are positions, not values.
///
/// **Position 14 is canonical AND the preset**, which is the one thing this travel has to get
/// right: `GullyParams::canonical()` is `drainage()` with the amplitude zeroed, so the two blocks
/// share every other field including this one. An untouched panel must therefore write no
/// `gullyCrest` at all, or a shared link would carry a crest exponent that was never moved and the
/// reload would come off the engine's `None` path -- RULING 1, which is that the default picture
/// cannot move. `14 / 20` is the f64 `0.7` exactly, so `gullyToParams` drops it.
///
/// `canonical` is taken as an argument and ignored, deliberately: every other channel's travel is
/// anchored on the engine's block, and a signature that quietly did not take it would be the one
/// place a later edit could reintroduce a literal without the pattern looking wrong. The test
/// asserts the canonical value lands on this lattice rather than assuming it.
export function gullyTravel(canonical) {
  return {
    crestSharpness: {
      min: CREST_MIN_TWENTIETHS,
      max: CREST_MAX_TWENTIETHS,
      toValue: (position) => position / CREST_TWENTIETHS,
      toPosition: (value) => Math.round(value * CREST_TWENTIETHS),
      format: (value) => value.toFixed(2),
      /// Stated so the caller cannot accidentally build a travel that excludes the engine's own
      /// default: `controls.js` refuses to enable the slider if this is false, and the test asks
      /// the same question of the shipped artifact.
      holdsCanonical: Object.is(
        Math.round(canonical.crestSharpness * CREST_TWENTIETHS) / CREST_TWENTIETHS,
        canonical.crestSharpness,
      ),
    },
  };
}

/// The slider control as a `PANEL_RANGES` row, **in exponent units rather than positions**, so
/// `panelFieldFaults()` can be run over it.
///
/// The widget itself carries integer positions, where a default off the lattice ought to be
/// impossible by construction -- but "impossible by construction" is what was said about the radius
/// slider too, and that one shipped. This states the travel in the units the sweep was measured in
/// and asks the check the question it exists to ask: **can this slider express its own default?**
/// It is a real question here, because unlike the coast amplitude the canonical value is in the
/// MIDDLE of this travel rather than at its minimum, so `min + 0 * step` does not answer it for
/// free.
///
/// `controls.js` runs this before enabling the slider and `gully-params.test.mjs` runs it against
/// the shipped `.wasm`, so it is a production check and not only a test.
export function gullyPanelFields(canonical) {
  const travel = gullyTravel(canonical);
  return GULLY_SLIDERS.map((field) => {
    const { min, max, toValue } = travel[field];
    const low = Math.min(toValue(min), toValue(max));
    const high = Math.max(toValue(min), toValue(max));
    // The lattice step in exponent units, taken from the map rather than restated: one position,
    // measured.
    const step = Math.abs(toValue(min + 1) - toValue(min));
    return { query: GULLY_PARAM_NAMES[field], min: low, max: high, step, value: canonical[field] };
  });
}

/// A gully block as a flat record in ABI order, ready for `wb_world_new_gully`.
export function gullyToRecord(gully) {
  return GULLY_FIELDS.map((name) => gully[name]);
}

/// A flat record back into a named object.
export function gullyFromRecord(record) {
  const out = {};
  GULLY_FIELDS.forEach((name, index) => { out[name] = record[index]; });
  return out;
}

/// The gully block a query string asks for, over the engine's own canonical block -- or **`null`,
/// meaning the canonical path**, which is `None` in the engine and is byte-for-byte today's
/// picture.
///
/// **RULING 1 lives in this function**, exactly as it lives in `reliefFromParams`,
/// `tectonicFromParams` and `coastFromParams` -- and it is load-bearing here in a way it was not
/// there. `Surface::with_gully(None)` **builds no steering lattice at all and takes a different
/// branch of `elevation_m`**, so the canonical path is structurally the old one rather than the new
/// one plus zero. A page opened with no gully parameters, and a page whose gully parameters all
/// happen to equal canonical's, both return `null` here.
///
/// A parameter that is present but not a finite number is **ignored rather than forwarded**:
/// `?gully=banana` is a typo in a shared link, and answering it with a refused world would turn a
/// typo into a blank page. Values that are numbers but out of domain *are* forwarded, and the
/// engine refuses them -- a caller asking for something the engine declines is a different thing
/// from a caller not asking for anything.
export function gullyFromParams(params, canonical) {
  const gully = { ...canonical };
  let touched = false;
  for (const field of GULLY_CONTROLS) {
    const name = GULLY_PARAM_NAMES[field];
    if (!params.has(name)) continue;
    const value = Number(params.get(name));
    if (!Number.isFinite(value)) continue;
    if (value === canonical[field]) continue;
    gully[field] = value;
    touched = true;
  }
  return touched ? gully : null;
}

/// The query-string fields for a chosen gully block, dropping every field still at canonical so a
/// shared link carries only what was actually moved.
export function gullyToParams(gully, canonical) {
  const out = {};
  for (const field of GULLY_CONTROLS) {
    const name = GULLY_PARAM_NAMES[field];
    out[name] = gully[field] === canonical[field] ? null : String(gully[field]);
  }
  return out;
}
