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
import { DEFAULT_CLOUD_COVER } from "./clouds.js";
import { DEFAULT_WATER_NODES } from "./water.js";
import { WB_MAX_WATER_NODES } from "./engine.js";

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
  // **The slider's travel IS the coverage**, 0 to 1 in hundredths, and that is the whole of the
  // calibration story from the panel's side: `clouds.js` inverts the field's own measured
  // distribution so the number here is the fraction of the sphere that actually comes back
  // covered. The alternative -- exposing the index threshold and letting the owner discover
  // empirically which end is cloudy -- is a slider whose travel is in the units of an
  // implementation detail, and the *nominal* range of that detail is precisely what this
  // project has been wrong about before.
  //
  // `min 0, step 0.01` and a default of 0.40 lands exactly on the lattice; note Task 1's related
  // trap, where `position * 0.05` could not express 0.35 and had to become `position / 20`.
  // Nothing here multiplies a slider position by a fraction: the position IS the value.
  { query: "clouds", min: 0, max: 1, step: 0.01, value: DEFAULT_CLOUD_COVER },
  // The water manifest's node count, and **the only knob that decides how many lakes exist**:
  // the owner's world resolves 55 bodies at 30,000 nodes and 351 at 100,000, because a finer
  // stream graph resolves more basins. It rebuilds, and it is the most expensive knob on the
  // panel -- 4.2 s at the default, 9.3 s at 60,000 -- which is why the panel's note quotes
  // seconds rather than leaving them to be discovered.
  //
  // **The top end is the ENGINE's ceiling**, imported rather than written down:
  // `WB_MAX_WATER_NODES` is 100,000 and sits below the erosion ceiling for a linear-memory
  // reason `wasm.rs` argues at length. The bottom end is a LATTICE choice and not a bound --
  // the engine accepts 2 -- because `min` must be congruent to the default modulo `step` or
  // the slider cannot express its own default, which is defects 3 and 4 at the top of this
  // file. The engine still owns every refusal; nothing here re-derives one.
  {
    query: "lakeNodes", min: 1000, max: WB_MAX_WATER_NODES, step: 1000,
    value: DEFAULT_WATER_NODES,
  },
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

/// **The ocean palette, defined ONCE.** `relief.js` imports this and derives its `OCEAN_BANDS`
/// from it; before this task the two files each held their own copy of the same six colours and
/// **the copies had already drifted** -- `RAMP_STOPS` carried stops at -60 m and -8 m where
/// `OCEAN_BANDS` carried one at -20 m, so the imagery layer and the `?relief=0` fallback drew
/// different shelves. That is this viewer's characteristic defect (see the four cases at the top
/// of this file) in its fifth instance, and the fix is the same one: one copy.
///
/// # Where these stops are placed, and why they are not evenly spaced
///
/// **Population:** a 2,880 x 1,440 edge-inclusive global fill (4,147,200 samples) at canonical
/// resolution, area-weighted by cos(latitude), on two worlds -- `DEFAULT_WORLD` (2,514,697 sea
/// samples, 70.8% of the sphere) and the owner's world, seed 562423712 / 4,500,000 m / 28 plates
/// / land 0.16 with the `ranges` tectonic preset (3,623,605 sea samples, 83.8%).
/// **Host:** node 22, this repository's checked-in `worldbuilder_engine.wasm`.
///
/// **The sea floor is a slab, not a slope.** Cumulative share of sea area at or below a depth:
///
/// ```text
///   depth      DEFAULT_WORLD      owner's world
///   -6000 m      0.025 %            0.114 %
///   -4700 m      1.306 %            1.878 %
///   -4620 m      3.746 %            4.794 %
///   -4560 m     47.829 %           53.412 %     <-- 44 to 49 % of the ocean in 60 metres
///   -4300 m     52.122 %           60.651 %
///   -3000 m     66.192 %           75.727 %
///   -1800 m     79.873 %           86.260 %
///    -900 m     89.981 %           93.034 %
///    -350 m     95.465 %           96.503 %
///    -120 m     98.301 %           98.712 %
///     -30 m     99.366 %           99.552 %
///      -6 m     99.779 %           99.857 %
/// ```
///
/// **That single line is the whole finding.** Roughly half the ocean lies inside a 60-metre band
/// around -4,590 m -- the abyssal plain -- and the previous table interpolated straight from
/// -4,600 m to -1,200 m, so those 3,400 metres of gradient were spent on 62% of the sea while the
/// 47% living in 60 metres of it received a colour difference of **under one RGB unit**. The
/// ocean did not read as flat because its deeps were not dark enough; it read as flat because
/// **half of it was one colour**. The stops at -4620 and -4560 are what give the plain a gradient
/// of its own, and everything above -4560 is spaced so that no band carries less than ~1% or more
/// than ~15% of the sea.
///
/// **Every stop is a depth both worlds attain**, which the previous table's -6,800 m stop was not:
/// zero samples at or below it in 8,294,400 samples over the two worlds. The deepest stop here is
/// -6,000 m (496 samples on `DEFAULT_WORLD`, 4,755 on the owner's); water below it clamps to the
/// abyssal colour, which is a colour that is actually drawn rather than an anchor nothing reaches.
///
/// **The colours brighten rather than darken.** The reference render's water runs `#3D94B5` shelf
/// -> `#26709E` open sea -> `#1A4F7A` abyss, which is *lighter than ours at every depth*; and our
/// deeps were already at luminance 19 of 255, with nowhere below them to go. So the contrast is
/// bought at the bright end: the shelf and the surf band are much brighter than before, the plain
/// is given an internal gradient, and only the trench floor is darkened.
///
/// The last stop is the **surf band**: from -6 m to the datum every ocean sample takes `#c9edf0`,
/// because `bandColor`/`addColorStop` both hold the last stop's colour above it. `relief.js`
/// scatters that band's outer edge with a per-texel dither, which is the half of a foam line a
/// 1-D height table cannot express.
export const OCEAN_STOPS = [
  [-6000, "#04101f"],   // trench floor -- 0.03% / 0.11% of sea area at or below it
  [-4620, "#082036"],   // the abyssal plain's floor
  [-4560, "#0e3357"],   // the plain's top: these two straddle half the ocean
  [-4300, "#114271"],
  [-4000, "#134d82"],
  [-3000, "#175b93"],
  [-1800, "#1d6da3"],
  [-900, "#2b88bd"],
  [-350, "#41a5d0"],    // slope
  [-120, "#66c2de"],    // shelf -- the pale rim around every landmass
  [-30, "#95dae9"],
  [-6, "#c9edf0"],      // surf, dithered by relief.js
];

/// **The lake palette, and it is NOT the ocean's.** `relief.js` imports this and derives its
/// `LAKE_BANDS` from it, exactly as it derives `OCEAN_BANDS` from `OCEAN_STOPS` above -- one
/// table each, in one place, for the same reason.
///
/// # Why a lake cannot share the sea's table
///
/// `OCEAN_STOPS` was placed against a measured *sea*-depth distribution whose median sits at
/// -4,580 m, and it is correct for that. **Every lake this generator makes is shallower than the
/// ocean table's third-shallowest stop**, so a lake could only ever draw from `-120 #66c2de`,
/// `-30 #95dae9` and `-6 #c9edf0` -- the shelf and the surf, luminance 176 to 230. Nine of the
/// twelve stops are unreachable on a lake and the three that remain are the palest in the table,
/// which is why the lakes read as sheets of ice rather than as water.
///
/// **Measured, over the whole drawn population rather than over one body.**
/// **Population:** every body `wb_water_run` resolves at 30,000 nodes, on two worlds -- the
/// owner's (seed 562423712, radius 4,500,000 m, 28 plates, land 0.16, `ranges`; 55 bodies) and
/// `DEFAULT_WORLD` (20260904, 6,371,000 m, 12 plates, 0.29; 156 bodies) -- each body's box grown
/// by one node cell radius first, because that is the box the picture is actually drawn from
/// (`water.js::dilateBodyExtents`). **Method:** each dilated box sampled at 0.02 degrees through
/// `wb_fill_tile_f32` at canonical resolution, a sample counted where `0 < h <= level_m`,
/// area-weighted by cos(latitude): 272,206 lake samples on the owner's world and 1,583,747 on
/// `DEFAULT_WORLD`. **Host:** node 22, this repository's checked-in `worldbuilder_engine.wasm`.
///
/// ```text
///   depth below the surface     owner's      DEFAULT_WORLD
///   p5                             1.3 m           1.5 m
///   p25                            7.0 m           8.2 m
///   p50                           15.4 m          18.5 m
///   p75                           27.1 m          36.8 m
///   p90                           40.2 m          82.3 m
///   p99                           72.1 m         237.4 m
///   max                          127.5 m         401.7 m
///
///   share of lake area SHALLOWER than
///     3 m      11.0 %    9.7 %
///     6 m      21.5 %   18.8 %
///    10 m      34.5 %   30.1 %
///    15 m      49.0 %   42.5 %
///    20 m      61.4 %   52.9 %
///    30 m      79.3 %   68.1 %
///    45 m      93.0 %   80.6 %
///    60 m      97.7 %   86.0 %
///   120 m      99.99%   93.8 %
///   200 m     100.0 %   97.7 %
/// ```
///
/// **Half a lake's area lies in its first twenty metres**, which is the mirror image of the sea's
/// finding: the sea is a slab at one depth, a lake is a wedge whose mass is at the shore. So the
/// stops are dense over 0..48 m -- nine of the thirteen -- and no band carries more than about
/// 15% of the lake area on either world. The tail to -390 m exists because `DEFAULT_WORLD` reaches
/// 401.7 m and the owner's world reaches only 127.5; a stop nothing attains is the defect
/// `OCEAN_STOPS`'s retired -6,800 m entry was, so -390 is inside the deepest sample found on
/// either world rather than a round number beyond it -- and inside it at every sampling density
/// measured, not only the finest: a 40x40 per-body grid finds 394.2 m, 128x128 finds 401.4 m.
///
/// # Where the colours come from, which is optics rather than taste
///
/// Inland water is dark for a physical reason, and it is the same reason the sea's shallows are
/// bright: what you see in shallow water is the **bottom**, and the two bottoms are different.
/// A shelf sea is a pale carbonate floor under water whose diffuse attenuation is around
/// 0.04 /m; an inland lake is a drowned soil-and-vegetation floor under water carrying dissolved
/// organic matter, an order of magnitude more absorbing. So a lake reaches its own asymptotic
/// colour within tens of metres, where the sea is still showing its floor at 120.
///
/// Each stop below is the two-term evaluation, at that stop's depth, of
///
/// ```text
///   colour(d) = bottom * exp(-2 * Kd * d)  +  column(d) * (1 - exp(-2 * Kd * d))
///   column(d) = lerp([27,58,72], [8,20,30], 1 - exp(-d / 200))
/// ```
///
/// with `Kd = 0.075 /m` (one way; the path is doubled because the light goes down and comes back)
/// and `bottom = [94,125,112]`, a wet valley floor. The bottom's contribution is 86% at 1 m, 41%
/// at 6 m, 10% at 15 m and under 1% by 32 m: **the gradient is spent over 0..32 m, where 73% of
/// the owner's lake area and 60% of `DEFAULT_WORLD`'s lives.** The second term is what keeps the
/// deep tail from being one flat colour -- it carries luminance 48 down to 24 over the remaining
/// 318 m, slowly, which is what a large lake looks like from orbit: a dark uniform centre inside a
/// visible shallow rim.
///
/// The model is evaluated once, here, and the results written down. It is not evaluated per texel:
/// `bandColor` interpolates between these thirteen exactly as it does between the ocean's twelve,
/// so a lake and the sea go through one lookup and not two code paths.
///
/// **No stop is at or above the datum.** The last stop IS `0`, which is the shoreline -- a texel
/// exactly at the body's own level -- and `slopeColor` never reaches this table for a texel at or
/// below the datum, because `lakeLevelAt` refuses one. The ocean is untouched by this table's
/// existence and a test asserts it byte for byte.
export const LAKE_STOPS = [
  [-390, "#0b1924"],    // inside the deepest lake sample on either world (DEFAULT_WORLD, 401.7 m)
  [-180, "#10232f"],
  [-110, "#132a36"],
  [-70, "#152f3c"],
  [-48, "#17323f"],     // 91% of the owner's lake area is shallower than this
  [-32, "#193542"],     // the bottom's contribution has fallen under 1% here
  [-22, "#1c3945"],
  [-15, "#213f4a"],
  [-10, "#29484f"],
  [-6, "#365558"],      // the bottom is still 41% of what is seen
  [-3, "#466561"],
  [-1, "#55746a"],
  [0, "#5e7d70"],       // the shoreline: the wet floor itself, under no water at all
];

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
///
/// **That check was made with a loose bound and one stop slipped through it.** The bound was the
/// deepest sample over three worlds (-6,807 m), so a stop at -6,800 m satisfied it -- while being
/// attained by **zero of the 8,294,400 samples** of the two worlds this viewer actually draws.
/// The ocean stops are now placed against a *cumulative* distribution rather than against a pair
/// of extremes, and `ocean stop reachability` in `panel-fields.test.mjs` asks the engine.
///
/// **One land stop is in the same position and is deliberately left there.** `[1980, "#ffffff"]`
/// is 2.4 m above `DEFAULT_WORLD`'s highest measured land (1,977.6 m in the fill above), so pure
/// white is never drawn on that world -- it is drawn on the owner's, whose peak is 4,978 m. This
/// task was told not to change land colour, so it is reported here rather than moved.
///
/// **The ocean half is `OCEAN_STOPS` above, spread in, not a second copy of it.** The two tables
/// had already drifted apart at the shelf before this task; spreading is what makes drifting
/// again impossible rather than merely unlikely.
export const RAMP_STOPS = [
  ...OCEAN_STOPS,
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
