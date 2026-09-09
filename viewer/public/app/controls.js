// A control panel over the parameters `main.js` already reads from the query string, plus
// an honest list of the things it cannot drive yet.
//
// Every knob in the first four sections existed before this file did, and every one of them
// was reachable only by typing a URL. Nothing new is exposed to the engine; what is new is
// that a person can turn them. The last section is the opposite: capabilities that exist in
// the engine or the roadmap and have **no viewer path at all**, listed rather than omitted
// so the panel does not read as a complete account of what the project does.
//
// ## Why changing a world knob USED to reload the page, and what replaced it
//
// This comment used to read, correctly: *"the world is built once, at boot... rebuilding in place
// would mean tearing down the pool, invalidating every cached tile and swapping the provider
// under a camera mid-flight -- three things with their own failure modes, none of which this
// panel needs."* The owner asked for the thing it declined to build: *"a slider for more
// mountains, and taller mountains, and I can watch them redraw on the planet I have."*
//
// So the three failure modes are handled rather than avoided. `main.js`'s `installWorld` rebuilds
// the world on the main thread and in every worker, resolves the water again, builds fresh
// providers and a fresh tile cache, and swaps them under a camera it never touches;
// `live-swap.js` decides what may be skipped and holds the measurement behind every rule.
// **The mountain, coastline and water sliders swap live. Everything else still reloads**, and
// `RELOAD_ONLY` names each one with the reason rather than leaving it to silently do nothing.
//
// Three properties this panel is responsible for, none of them optional:
//
// - **The picture a slider reaches must equal the picture its URL loads.** The swapped spec is
//   built by round-tripping through the SAME `*FromParams` readers the boot path uses, over the
//   SAME query string this file writes -- so "equal" is structural rather than hoped for.
// - **The URL keeps up.** `history.replaceState`, not a navigation: a permalink that no longer
//   describes the picture is worse than a reload.
// - **Release, not drag.** Every terrain change invalidates every visible tile, so there is no
//   smooth-dragging story here and this file does not pretend there is one: the listener is
//   `change` (which fires on release), through a debounce that drops intermediate asks rather
//   than queueing them.
//
// View and appearance knobs that Cesium can change live do so without a reload, and are
// marked as such. The readouts never reload.
//
// ## CSP
//
// Served under `default-src 'self'` with no `'unsafe-inline'`, so this is a module file and
// its styles live in `viewer.css`. See `index.html`'s comment.

import { PANEL_DEFAULTS, PANEL_RANGES, panelFieldFaults } from "./panel-fields.js";
import { mountWorldPanel } from "./world-panel.js";
import {
  RELIEF_CONTROLS, RELIEF_PARAM_NAMES, HURST_BAND, hurst, sliderTravel, reliefToParams,
} from "./relief-params.js";
import {
  TECTONIC_SLIDERS, MEASURED_GRADES, tectonicTravel, tectonicPanelFields, tectonicToParams,
  tectonicFromParams,
} from "./tectonic-params.js";
import {
  COAST_SLIDERS, MEASURED_COAST, USEFUL_BAND, coastReadoutFields, coastTravel, coastPanelFields,
  coastToParams, coastFromParams,
} from "./coast-params.js";
import {
  GULLY_SLIDERS, MEASURED_CREST, MEASURED_RELIEF, MEASURED_SLOPE, CARVE_BAND,
  AMPLITUDE_BAND, MEASURED_AMPLITUDE_SWEEP,
  gullyReadoutFields, gullyTravel, gullyPanelFields, gullyToParams, gullyFromParams,
} from "./gully-params.js";
import { debounceLatest, nextQueryString, RELOAD_ONLY, SWAP_DEBOUNCE_MS } from "./live-swap.js";
import { waterNodeCountFromParams } from "./water.js";

const params = new URLSearchParams(location.search);

/// **This file now writes down no default and no slider travel of its own.**
///
/// It used to hold a copy of each, annotated with the module it was copied from, and the
/// copies drifted: the panel offered a -9000/6000 elevation ramp while `main.js` had
/// narrowed to -7000/2400, so opening the panel and pressing generate silently reverted the
/// ramp. That was fixed by making the copy correct again, which is a fix with a half-life.
/// `panel-fields.js` holds one copy, `main.js` reads the same one, and `node --test` holds
/// the same table -- including a check that each slider's travel can express its own
/// default, which is the fault the radius and `rampMax` sliders both had.
const DEFAULTS = PANEL_DEFAULTS;

/// The travel for one range input, by its query-string name. Throws rather than returning a
/// silent `undefined`: a mistyped name here would build a slider with no bounds at all.
function travelFor(query) {
  const field = PANEL_RANGES.find((f) => f.query === query);
  if (!field) throw new Error(`controls.js: no panel field named "${query}"`);
  return { min: field.min, max: field.max, step: field.step, value: current(query) };
}

/// From `panel-fields.js`'s `HARBOUR`: a 900 x 260 m carve to -12 m with a 200 x 60 m mole
/// to +4 m. Flying here is the only way to watch the zoom cap do its job -- ground detail
/// stops at level 12 and refinement continues past it *only* inside a feature's footprint.
const HARBOUR_VIEW = { lat: -18.25, lon: 121.5, height: 2200 };

/// `terrain.js` FAULTS and `pool.js` POOL_FAULTS: deliberately wrong implementations, kept
/// so the verification harness can prove its checks can fail. Exposed here because a
/// verifier nobody has seen fail is a verifier nobody knows the shape of, and this is the
/// cheapest way to see one fail.
const FAULT_OPTIONS = [
  ["", "none"],
  ["flip-latitude", "flip latitude"],
  ["shift-tile", "shift tile"],
  ["wrong-world", "wrong world"],
  ["stale-worker", "stale worker"],
  ["cache-key", "cache key"],
];

/// Things the engine or the roadmap can do that this viewer has no path to. Listed so the
/// panel is not mistaken for the whole product. `state` is deliberately blunt.
// **Two entries came off this list together**, and their old text is kept here because it was
// true and is the reason the "mountains" section below exists:
//
//   ["mountain height", "tectonic, not relief: roughness tops out at 161 m over 2 km"]
//   ["mountain count",  "tectonic: plate collisions place them, and no relief knob reaches that"]
//
// Both said the same thing -- that a mountain on this planet is a *tectonic* feature and no
// relief knob could reach it. Ruling 4 of the relief-amplitude slice measured it (the peak is
// dominated by the structural term, and the roughness spectrum tops out at 161 m on peaks over
// 2 km), and the mountains slice measured it again on the owner's own world: **1,454.04 m of
// peak, 1,437.81 m of it structural -- 98.9% tectonic.** They come off together because
// `wb_tectonic_preset` / `wb_tectonic_check` / `wb_world_new_tectonic` reach the block that
// carries both, and neither would be honest to remove alone.
//
// **A THIRD ENTRY CAME OFF THIS LIST, AND A FOURTH WAS WRONG.**
//
// The one that came off was the coastline. It was never written down here, because the coast
// channel did not exist in the engine when this list was last touched -- `CoastParams` shipped
// for a whole task with no export, no field and no slider, verified and measured and invisible.
// `wb_coast_preset` / `wb_coast_check` / `wb_world_new_coast` reach it now and the "coastline"
// section below is what turns it.
//
// The one that was WRONG is "lakes + water", which said `no export yet`. That has been false
// since slice 5b: `wb_water_run` is in `WB_EXPORTS` and ships in the committed artifact. What was
// actually missing was a viewer that CALLED it, which is a different sentence and a smaller claim,
// and the entry was corrected to say that instead. A "not wired yet" list that is wrong about the
// engine is worse than no list: it is the panel telling the owner a capability does not exist when
// it does.
//
// **A FIFTH ENTRY HAS NOW COME OFF, AND IT IS THAT ONE.** The viewer calls `wb_water_run` at boot
// and `relief.js` draws every body as a flat sheet at its resolved level. Its old text is kept
// here for the same reason the mountains' two are -- it is the reason the "water" section below
// exists:
//
//   ["lakes + water", "wb_water_run ships in the .wasm; nothing in the viewer calls it"]
//
// **Two narrower entries replace it, and both are measured rather than guessed.** A body arrives
// as a level and a bounding BOX, so its true shoreline is not available at all -- what is drawn is
// the box intersected with the level. **The box's own resolution has since been identified and is
// one stream-node spacing** -- `water.rs::lake_body_extents` bounds submerged node CENTRES, so
// `water.js::dilateBodyExtents` grows every box by the one cell radius that construction licenses,
// which is what made the 38 point-box bodies drawable at all. It halves the straight-edge
// exposure and does not remove it: measured over every body on two worlds, the fraction of a
// box's perimeter that is still at or below the body's own level -- i.e. where the box rather
// than the terrain decides where the water stops -- falls from 47.8% to 20.7% on the owner's
// world and from 31.3% to 22.5% on `DEFAULT_WORLD`. That residue is the entry.
// And `LakeKind::Pond` cannot occur: the engine calibrated its threshold at 1.0e5 m^2 and
// then measured the smallest body this mesh produces at 7.9e8 m^2.
//
// **What did NOT come off is "rivers", deliberately.** `WaterManifest` carries `reaches` and
// slice 5b's Ruling 2 leaves them unpopulated at Mark 2 -- schema only. Drawing an empty
// collection is not a feature, so that entry stays exactly as it was.
//
// **A SIXTH ENTRY WAS WRONG, AND IT SAID TWO THINGS AT ONCE.** It read:
//
//   ["climate + biomes", "designed, approved for after 5b: not built"]
//
// Both halves are false, in different ways, and a list that is wrong about the project is worse
// than no list. **Biomes are built and are the default land colour on this page**: `biome.js`
// classifies every land texel on three axes and `relief.js` takes its land base colour from it,
// so the entry claimed a shipped, drawn, visible feature did not exist. And **climate is built
// too** -- `crates/worldbuilder-engine/src/climate.rs` carries `ClimateParams` and
// `Surface::temperature_c` -- it simply has **no wasm export**, which is a narrower and truer
// sentence and is the one entry that survives. That is the same correction the water entry got:
// name what is actually missing rather than what looks missing.
//
// **The gully channel was never on this list**, for the same reason the coastline never was: it
// did not exist when the list was last touched. `GullyParams` shipped with `wb_world_new_gully`,
// `wb_gully_preset` and `wb_gully_check` in the committed artifact and no viewer path at all --
// exactly `CoastParams`' story -- and the "drainage" section below is what turns it.
const NOT_WIRED = [
  ["erosion", "wb_erosion_run ships in the .wasm; nothing in the viewer calls it"],
  ["climate", "climate.rs ships ClimateParams and Surface::temperature_c; there is no wasm "
    + "export, so the viewer cannot reach it — biome.js's temperature is a curve of its own"],
  ["island arcs", "TectonicParams carries them; Task 1 proved no coverage, so no slider"],
  ["lake shorelines", "the box bounds NODE CENTRES, so it is grown by one cell; 21% of a body's "
    + "boundary is still a straight cut, because no true footprint is exported"],
  ["ponds", "the calibrated 1.0e5 m² threshold classifies none: the smallest body is 7.9e8 m²"],
  ["rivers", "schema only in Mark 2; reaches are carried, not populated"],
  ["place areas", "slice 3, the studio: not started"],
  ["export to Evennia", "slice 2a apply: not started"],
  ["cartography", "own slice after the studio: not started"],
];

function current(name) {
  return params.has(name) ? params.get(name) : DEFAULTS[name];
}

/// Rebuild the query string and reload. Absent or default-valued fields are dropped rather
/// than written, so a shared link carries only what was actually changed.
function apply(next) {
  const q = new URLSearchParams(location.search);
  for (const [key, value] of Object.entries(next)) {
    if (value === null || value === "" || value === DEFAULTS[key]) q.delete(key);
    else q.set(key, value);
  }
  location.search = q.toString();
}

function el(tag, cls, text) {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (text !== undefined) node.textContent = text;
  return node;
}

function section(parent, label) {
  const wrap = el("div", "wb-section");
  wrap.append(el("div", "wb-section-title", label));
  parent.append(wrap);
  return wrap;
}

function row(parent, label, id, type, attrs = {}) {
  const line = el("label", "wb-row");
  line.append(el("span", null, label));
  const input = document.createElement("input");
  input.type = type;
  input.id = id;
  Object.assign(input, attrs);
  const out = el("output");
  out.id = `${id}-out`;
  line.append(input, out);
  parent.append(line);
  return { input, out };
}

function build() {
  const panel = el("div");
  panel.id = "wb-panel";

  const head = el("div", "wb-head");
  head.append(el("strong", null, "worldbuilder"));
  const toggle = el("button", "wb-toggle", "–");
  toggle.type = "button";
  head.append(toggle);
  panel.append(head);

  const body = el("div", "wb-body");
  panel.append(body);
  toggle.addEventListener("click", () => {
    const hidden = body.style.display === "none";
    body.style.display = hidden ? "" : "none";
    toggle.textContent = hidden ? "–" : "+";
  });

  // === live update ========================================================================
  //
  // On by default, because it is the thing that was asked for. Off restores this panel exactly as
  // it was: every slider waits for `generate`, and `generate` still reloads.
  //
  // **The note under it says the honest thing about cost**, because the alternative is an owner
  // discovering it by waiting. A mountain release re-solves the water, and the water solve is the
  // dominant cost at a high node count -- the reason is measured in `live-swap.js` and is not a
  // choice this panel is free to make differently.

  const liveSection = section(body, "live update");
  const liveLine = el("label", "wb-row wb-check");
  const live = document.createElement("input");
  live.type = "checkbox";
  live.id = "wb-live";
  live.checked = params.get("live") !== "0";
  liveLine.append(live, el("span", null, "swap in place on release (no reload)"));
  liveSection.append(liveLine);
  const swapNote = el("div", "wb-note", "");
  liveSection.append(swapNote);
  liveSection.append(el("div", "wb-note",
    "Mountains, coastline and water swap in place; the camera stays where you left it and the "
    + "address bar keeps up. A terrain change invalidates every visible tile, so this fires on "
    + "release rather than while you drag. Moving the surface also re-resolves the water — the "
    + "lake bodies are read off the surface, so a mountain move creates and deletes lakes."));
  const reloadNote = el("ul", "wb-missing");
  for (const [name, why] of RELOAD_ONLY) {
    const item = document.createElement("li");
    item.append(el("span", "wb-missing-name", name));
    item.append(el("span", "wb-missing-why", why));
    reloadNote.append(item);
  }
  liveSection.append(el("div", "wb-note", "still reloads:"));
  liveSection.append(reloadNote);

  // === the world — these rebuild ==========================================================

  const world = section(body, "world · rebuilds");

  const seed = row(world, "seed", "wb-seed", "text", { value: current("seed"), size: 10 });
  const dice = el("button", "wb-mini", "random");
  dice.type = "button";
  // Integer seeds only: engine.js passes the seed through BigInt(), which throws otherwise.
  dice.addEventListener("click", () => {
    seed.input.value = String(Math.floor(Math.random() * 1e9));
  });
  seed.out.replaceWith(dice);

  const plates = row(world, "plates", "wb-plates", "range", travelFor("plates"));
  const land = row(world, "land", "wb-land", "range", travelFor("land"));
  const radius = row(world, "radius", "wb-radius", "range", travelFor("radius"));

  const harbourLine = el("label", "wb-row wb-check");
  const harbour = document.createElement("input");
  harbour.type = "checkbox";
  harbour.id = "wb-harbour";
  harbour.checked = params.has("harbour");
  harbourLine.append(harbour, el("span", null, "harbour feature"));
  world.append(harbourLine);

  // === the tiling — these rebuild =========================================================

  const tiling = section(body, "tiling · rebuilds");
  const zoom = row(tiling, "max zoom", "wb-zoom", "range", travelFor("maxLevel"));
  const size = row(tiling, "posts", "wb-size", "range", travelFor("size"));
  const ceiling = row(tiling, "feat. cap", "wb-ceiling", "range", travelFor("featureCeiling"));

  // === relief — these rebuild =============================================================
  //
  // **Rebuild-class, not live**, and not by preference: relief parameters are an argument to
  // `Surface::new`. They decide the octave schedule once, in the constructor, and every
  // worker holds its own already-built world. There is no uniform to poke the way the ramp
  // and the exaggeration have one, so these follow `location.search` like every other world
  // knob in this panel.
  //
  // **No relief number is written in this file.** Every default and two of the three travel
  // ends are read from the engine's own `wb_relief_preset` in `wireRelief` below. The panel's
  // ramp defaults drifted from `main.js`'s once and silently reverted the ramp on every
  // generate; the answer this time is to hold no copy at all rather than a correct copy.
  // Until the engine answers, these sliders are disabled and say so.

  const reliefSection = section(body, "relief · rebuilds");
  const reliefLabels = {
    mountainM: "hi-gnd rough",
    quietingStrength: "quieting",
    octavePersistence: "persistence",
  };
  const reliefRows = {};
  for (const field of RELIEF_CONTROLS) {
    reliefRows[field] = row(reliefSection, reliefLabels[field], `wb-relief-${field}`, "range",
      { min: 0, max: 1, step: 1, value: 0, disabled: true });
    reliefRows[field].out.textContent = "—";
  }
  const reliefNote = el("div", "wb-note", "waiting for the engine…");
  reliefSection.append(reliefNote);
  const reliefActions = el("div", "wb-actions");
  const hillsButton = el("button", "wb-mini", "hills preset");
  hillsButton.type = "button";
  hillsButton.disabled = true;
  hillsButton.title = "ReliefParams::hills(), read from the engine — not restated here";
  const reliefReset = el("button", "wb-mini", "canonical");
  reliefReset.type = "button";
  reliefReset.disabled = true;
  reliefReset.title = "back to the engine's canonical block, which is the untouched world";
  reliefActions.append(hillsButton, reliefReset);
  reliefSection.append(reliefActions);

  /// The relief block the sliders currently describe, or `null` while the engine has not
  /// answered. `rebuildFields` closes over this, so it is read at click time, not now.
  let reliefState = null;
  let reliefCanonical = null;

  /// Fill in the travel, the defaults and the readouts once the engine can be asked.
  function wireRelief(presets) {
    reliefCanonical = presets.canonical;
    const travel = sliderTravel(presets.canonical, presets.hills);
    // The chosen block, if the URL carries one; otherwise canonical. Read through the same
    // `reliefFromParams` the boot path uses, via `__wb.relief.chosen`, so the panel and the
    // world cannot disagree about what was asked for.
    reliefState = { ...presets.canonical, ...(presets.chosen ?? {}) };

    const paint = () => {
      for (const field of RELIEF_CONTROLS) {
        const value = travel[field].toValue(Number(reliefRows[field].input.value));
        reliefState[field] = value;
        reliefRows[field].out.textContent = travel[field].format(value);
      }
      // The Hurst exponent is the number with meaning outside this engine: 0.65 persistence
      // is an implementation detail of `plan`'s schedule, H is a measured property of real
      // ground. Gagnon, Lovejoy & Schertzer put real terrain at H 0.6-0.71 across four DEMs,
      // and the note says whether the current setting is inside that band rather than
      // leaving the reader to compare two numbers.
      const h = hurst(reliefState.octavePersistence);
      const inBand = h >= HURST_BAND.low && h <= HURST_BAND.high;
      reliefNote.textContent =
        `H ${h.toFixed(3)} — real terrain is ${HURST_BAND.low}–${HURST_BAND.high} ` +
        `(Gagnon, Lovejoy & Schertzer, four DEMs): ${inBand ? "inside" : "outside"}. ` +
        "Roughness only — mountain height is tectonic.";
    };

    for (const field of RELIEF_CONTROLS) {
      const { input } = reliefRows[field];
      input.min = travel[field].min;
      input.max = travel[field].max;
      input.step = 1;
      input.value = travel[field].toPosition(reliefState[field]);
      input.disabled = false;
      input.addEventListener("input", paint);
    }
    const setAll = (block) => {
      for (const field of RELIEF_CONTROLS) {
        reliefRows[field].input.value = travel[field].toPosition(block[field]);
      }
      paint();
    };
    hillsButton.disabled = false;
    reliefReset.disabled = false;
    // Both buttons send the engine's own records back to the engine. Neither restates a
    // number, which is the whole point of `wb_relief_preset` existing.
    hillsButton.addEventListener("click", () => setAll(presets.hills));
    reliefReset.addEventListener("click", () => setAll(presets.canonical));
    paint();
  }

  // === mountains — these rebuild ==========================================================
  //
  // **THE SECTION THE OWNER ASKED FOR.** In their words: "1 to raise and lower mountains and
  // one to make more mountains and less as desired."
  //
  // Rebuild-class for the same reason the relief sliders are: `TectonicParams` is an argument
  // to `Surface::new`, resolved once in `Tectonics::new`, and every worker holds its own
  // already-built world. There is no uniform to poke.
  //
  // **No tectonic number is written in this file.** All SIX sliders and both buttons are
  // anchored on `wb_tectonic_preset`'s answer in `wireTectonics` below, and until the engine
  // answers they are disabled and say so. The panel's ramp defaults drifted from `main.js`'s
  // once and silently reverted the ramp on every generate; the answer here, as with relief, is
  // to hold no copy at all rather than a correct copy.
  //
  // **Three of the six are Task 3's, and they are the reason this section is worth looking at
  // again.** Task 2 built the structure field -- the doubly-vergent wedge, the stacked
  // sutures, and the ridged-multifractal-times-segmentation field that turns one smooth welt
  // into separate massifs -- and stopped at the WASM boundary, so for a whole task none of it
  // was reachable from here. `WB_TECTONIC_STRIDE` went 9 -> 14 and these are what it carries.

  const mountainSection = section(body, "mountains · live");
  const mountainLabels = {
    continentCollisionM: "height",
    // Labelled for its EFFECT rather than its unit, because "width" reads as a size and this
    // is the knob that decides whether a 4 km peak is a mountain or a ramp. The readout still
    // shows the kilometres.
    continentCollisionWidthM: "steepness",
    continentalBlend: "count",
    // The three structure sliders. Labelled for what they DO to the picture, because none of
    // their parameter names would mean anything to someone looking at a mountain: this is the
    // difference between a smooth blade and two parallel belts of separate massifs with a
    // ridge-and-valley interior, which is what Task 2 built and nobody could see.
    collisionAsymmetry: "vergence",
    structureDepth: "structure",
    structureWavelengthM: "massif size",
  };
  const mountainRows = {};
  for (const field of TECTONIC_SLIDERS) {
    mountainRows[field] = row(mountainSection, mountainLabels[field], `wb-tectonic-${field}`,
      "range", { min: 0, max: 1, step: 1, value: 0, disabled: true });
    mountainRows[field].out.textContent = "—";
  }
  // The suture pair, SHOWN but not sliderable. `TECTONIC_SLIDERS`' own comment gives the
  // measured reason there is no widget: count and spread are jointly constrained, and the one
  // useful setting is a point rather than a travel. But the preset moves them, the query string
  // carries them, and a preset that changed something the panel never mentioned would be a
  // parameter the owner cannot see -- which is the whole defect this slice exists to fix. So
  // they are a readout.
  const beltNote = el("div", "wb-note", "—");
  mountainSection.append(beltNote);
  // **The block the sliders describe is now one the engine can REFUSE, and the panel says so
  // before the owner presses generate.**
  //
  // The warp adds to `collision_reach_m`, which is held against `MAX_TECTONIC_RANGE_M` -- so
  // "wander" at its top with "steepness" at its widest asks for a profile the range gate would
  // truncate, and the engine refuses the record entire rather than adjusting it. That is the
  // right behaviour and it used to be unreachable from here; it is not any more, and a
  // generate that silently produced the previous world would be worse than a note. Asked of
  // `wb_tectonic_check` through the engine, which is the same validator the record will meet,
  // rather than by re-deriving the reach in JavaScript.
  const admissibleNote = el("div", "wb-note", "");
  mountainSection.append(admissibleNote);
  const mountainNote = el("div", "wb-note", "waiting for the engine…");
  mountainSection.append(mountainNote);
  const mountainActions = el("div", "wb-actions");
  const rangesButton = el("button", "wb-mini", "ranges preset");
  rangesButton.type = "button";
  rangesButton.disabled = true;
  rangesButton.title =
    "TectonicParams::ranges(), read from the engine — every value is set on a slider you can see and move";
  const mountainReset = el("button", "wb-mini", "canonical");
  mountainReset.type = "button";
  mountainReset.disabled = true;
  mountainReset.title = "back to the engine's canonical block, which is the untouched world";
  mountainActions.append(rangesButton, mountainReset);
  mountainSection.append(mountainActions);

  /// The tectonic block the sliders currently describe, or `null` while the engine has not
  /// answered. `rebuildFields` closes over this, so it is read at click time, not now.
  let tectonicState = null;
  let tectonicCanonical = null;

  /// The measured grade at the two corners of the calibration, as a sentence. Read from
  /// `MEASURED_GRADES` rather than typed here, so the panel and the probe cannot disagree
  /// about what was measured.
  const gradeNote = () => {
    const gentle = MEASURED_GRADES[0];
    const steep = MEASURED_GRADES[MEASURED_GRADES.length - 1];
    return `${gentle.heightM} m over ${gentle.widthKm} km is a ${gentle.grade}% grade; ` +
      `${steep.heightM} m over ${steep.widthKm} km is ${steep.grade}%. Real ranges run 3–8%.`;
  };

  /// Fill in the travel, the defaults and the readouts once the engine can be asked.
  function wireTectonics(presets) {
    tectonicCanonical = presets.canonical;
    // **The panel-default family check, run in production and not only in a test.** The
    // widget carries integer positions, so a mis-stepped default ought to be impossible by
    // construction -- but "impossible by construction" is what was said about the radius
    // slider too, and this asks the question in the units the calibration was measured in.
    // Four instances of this defect have shipped; the fourth was found by this check.
    const faults = panelFieldFaults(tectonicPanelFields(presets.canonical));
    if (faults.length > 0) {
      mountainNote.textContent = `slider travel refused: ${faults.join("; ")}`;
      return;
    }
    const travel = tectonicTravel(presets.canonical);
    tectonicState = { ...presets.canonical, ...(presets.chosen ?? {}) };

    const paint = () => {
      for (const field of TECTONIC_SLIDERS) {
        const value = travel[field].toValue(Number(mountainRows[field].input.value));
        tectonicState[field] = value;
        mountainRows[field].out.textContent = travel[field].format(value);
      }
      // The two fields with no widget, read out of the state the preset button writes rather
      // than from a literal. `sutureCount` is an integer in an f64 slot; `sutureSpreadM` is
      // metres.
      const belts = tectonicState.sutureCount;
      beltNote.textContent = belts > 1
        ? `${belts} parallel belts, ${(tectonicState.sutureSpreadM / 1000).toFixed(0)} km apart`
        : "one belt (canonical)";
      // Which way "count" runs, said in words, because the parameter behind it runs the
      // opposite way to its name: `collision = inboard * outboard` and each side is a
      // smoothstep over `value / blend`, so a NARROWER transition is MORE mountains. The
      // slider is negated for that reason and the note says which end you are at.
      const position = Number(mountainRows.continentalBlend.input.value);
      const direction = position > 0 ? "more" : position < 0 ? "fewer" : "canonical";
      // And what the structure slider costs, because it is not free and the height slider's
      // reading is no longer independent of it: the multiplier is at most 1, so turning
      // structure up can only LOWER the delivered peak. Measured at −22% at depth 0.7.
      //
      // The note carries no NUMBER for that depth on purpose -- a literal here is a literal
      // the "no tectonic number written twice" test would have to allow through, and this file
      // has already shipped four panel values that were not the engine's.
      const structure = tectonicState.structureDepth > 0
        ? " Structure carves the delivered peak down by up to a fifth; 40–80 km is where it bites."
        : "";
      mountainNote.textContent = `${gradeNote()} Count: ${direction}.${structure}`;
      // The wander's own readout, and the wavelength beside it -- the field with no widget,
      // read out of the state the preset button writes rather than from a literal here.
      // Task 5's pair, driven rather than sliderable for the reason `TECTONIC_SLIDERS` gives,
      // and SHOWN for the reason the suture pair is: a preset that changed something the panel
      // never mentioned would be a parameter the owner cannot see, which is the defect this
      // whole slice exists to fix. Their words were "they look like they were drawn with a
      // straight line tool"; this is the line that says whether they still are.
      const wander = tectonicState.marginWarpM;
      beltNote.textContent += wander > 0
        ? ` · wander ±${(wander / 1000).toFixed(0)} km over ${
          (tectonicState.marginWarpWavelengthM / 1000).toFixed(0)} km`
        : " · no wander (the margin is a great circle)";
      // And whether the engine would take it. `presets.check` is `wb_tectonic_check`; if the
      // host did not supply it the note stays empty rather than claiming a block is fine.
      if (typeof presets.check === "function") {
        admissibleNote.textContent = presets.check(tectonicState)
          ? ""
          : "this block reaches past the range gate — the engine will refuse it. " +
            "Lower wander, or narrow steepness.";
      }
    };

    for (const field of TECTONIC_SLIDERS) {
      const { input } = mountainRows[field];
      input.min = travel[field].min;
      input.max = travel[field].max;
      input.step = 1;
      input.value = travel[field].toPosition(tectonicState[field]);
      input.disabled = false;
      input.addEventListener("input", paint);
    }
    mountainReset.disabled = false;
    rangesButton.disabled = false;
    // **Both buttons send the ENGINE'S OWN record back to the engine and restate nothing.**
    // `setAllMountains` writes every field of a preset -- the six that have sliders onto their
    // sliders, and the two that do not straight into the state the readout and the query
    // string both read. That second half is why this is a loop over the block rather than over
    // the widgets: a preset half-applied because two of its fields had no widget is the
    // silently-dropping-builder shape, and this file's own history is four instances of a
    // panel value that was not the engine's value.
    const setAllMountains = (block) => {
      tectonicState = { ...block };
      for (const field of TECTONIC_SLIDERS) {
        mountainRows[field].input.value = travel[field].toPosition(block[field]);
      }
      paint();
    };
    rangesButton.addEventListener("click", () => setAllMountains(presets.ranges));
    mountainReset.addEventListener("click", () => setAllMountains(presets.canonical));
    paint();
  }

  // === coastline — these rebuild ==========================================================
  //
  // **THE SECTION THAT MAKES A COASTLINE FRACTAL.** The owner's coasts are smooth: measured on
  // their own world, the coastline's length is FLAT across an eightfold change of measuring
  // ruler (101,323 km at 100 km spacing, 100,981 km at 12.5 km), which is the estimator saying
  // there is no structure below the land/sea field's own finest octave -- and the whole planet
  // has two inlet heads.
  //
  // Rebuild-class for the same reason the relief and mountain sliders are: `CoastParams` is an
  // argument to `Surface::with_coast`, resolved once in `Continentality::with_coast`, and every
  // worker holds its own already-built world. There is no uniform to poke.
  //
  // **No coast number is written in this file.** The one slider and both buttons are anchored on
  // `wb_coast_preset`'s answer in `wireCoast` below, and until the engine answers they are
  // disabled and say so.

  const coastSection = section(body, "coastline · live");
  const coastLabels = {
    // Labelled for its EFFECT, not its unit. The parameter is "how far the coast may be pushed,
    // in multiples of the field's own spread", which means nothing to someone looking at a bay.
    amplitude: "raggedness",
  };
  const coastRows = {};
  for (const field of COAST_SLIDERS) {
    coastRows[field] = row(coastSection, coastLabels[field], `wb-coast-${field}`,
      "range", { min: 0, max: 1, step: 1, value: 0, disabled: true });
    coastRows[field].out.textContent = "—";
  }
  // The five fields with no widget, SHOWN. `COAST_SLIDERS`' own comment gives the measured reason
  // there is no control for them: Task 5 calibrated the amplitude and calibrated nothing else, and
  // a slider whose travel nobody has measured is a slider nobody can aim. But the preset carries
  // all six, the query string carries all six, and a preset that changed something the panel never
  // mentioned would be a parameter the owner cannot see -- which is the whole defect this slice
  // exists to fix. So they are a readout.
  const coastScheduleNote = el("div", "wb-note", "—");
  coastSection.append(coastScheduleNote);
  // And whether the engine would take the block, before generate rather than after. Two of this
  // channel's bounds are JOINT -- the octave count is a per-sample loop bound, and the finest
  // octave's frequency is a product of `frequency`, `octaves` and `lacunarity` that can be past
  // the noise lattice's index range while all three fields sit inside their own domains -- so a
  // hand-typed query string can ask for a record the engine refuses entire. Asked of
  // `wb_coast_check` through the engine, which is the same validator the record will meet, rather
  // than by re-deriving the product in JavaScript.
  const coastAdmissibleNote = el("div", "wb-note", "");
  coastSection.append(coastAdmissibleNote);
  const coastNote = el("div", "wb-note", "waiting for the engine…");
  coastSection.append(coastNote);
  const coastActions = el("div", "wb-actions");
  const fractalButton = el("button", "wb-mini", "fractal preset");
  fractalButton.type = "button";
  fractalButton.disabled = true;
  fractalButton.title =
    "CoastParams::fractal(), read from the engine — its one moved field is on the slider you can see";
  const coastReset = el("button", "wb-mini", "canonical");
  coastReset.type = "button";
  coastReset.disabled = true;
  coastReset.title = "back to the engine's canonical block, which is the untouched coastline";
  coastActions.append(fractalButton, coastReset);
  coastSection.append(coastActions);

  /// The coast block the slider currently describes, or `null` while the engine has not answered.
  let coastState = null;
  let coastCanonical = null;

  /// What the measured table says about the position the slider is on, as a sentence. Read from
  /// `MEASURED_COAST` rather than typed here, so the panel and the survey cannot disagree about
  /// what was measured.
  const coastRowFor = (amplitude) => {
    let best = MEASURED_COAST[0];
    for (const row of MEASURED_COAST) {
      if (Math.abs(row.amplitude - amplitude) < Math.abs(best.amplitude - amplitude)) best = row;
    }
    return best;
  };

  /// Fill in the travel, the defaults and the readouts once the engine can be asked.
  function wireCoast(presets) {
    coastCanonical = presets.canonical;
    // **The panel-default family check, run in production and not only in a test.** The widget
    // carries integer positions, so a mis-stepped default ought to be impossible by construction
    // -- but "impossible by construction" is what was said about the radius slider too, and this
    // asks the question in the units the calibration was measured in. Four instances of this
    // defect have shipped; the fourth was found by this check.
    const faults = panelFieldFaults(coastPanelFields(presets.canonical));
    if (faults.length > 0) {
      coastNote.textContent = `slider travel refused: ${faults.join("; ")}`;
      return;
    }
    const travel = coastTravel(presets.canonical);
    coastState = { ...presets.canonical, ...(presets.chosen ?? {}) };

    const paint = () => {
      for (const field of COAST_SLIDERS) {
        const value = travel[field].toValue(Number(coastRows[field].input.value));
        coastState[field] = value;
        coastRows[field].out.textContent = travel[field].format(value);
      }
      // The schedule, read out of the state the preset button writes rather than from literals.
      // Every driven field appears somewhere the owner can see it: the slider shows one and this
      // line shows the rest. Asserted rather than trusted -- `coast-params.test.mjs` checks that
      // the union of `COAST_SLIDERS` and the fields named on this line is `COAST_CONTROLS`, so a
      // seventh field added to the channel cannot arrive silently.
      const shown = coastReadoutFields().map((f) => `${f} ${coastState[f]}`).join(" · ");
      coastScheduleNote.textContent = shown;
      // What the measured survey says about where the slider is standing. The numbers come from
      // the table, and the BAND's two ends come from `USEFUL_BAND`, so this file states neither.
      const amplitude = coastState.amplitude;
      const measured = coastRowFor(amplitude);
      const where = amplitude < USEFUL_BAND.low
        ? "below the visible floor — a sub-pixel wobble"
        : amplitude > USEFUL_BAND.high
          ? "past the fragmenting end — small islands multiply and the large ones do not"
          : "inside the measured band";
      coastNote.textContent =
        `${where}. At ${measured.amplitude.toFixed(2)} the coast measures ` +
        `${measured.lengthRatio.toFixed(3)}x longer at a 25 km ruler with ` +
        `${measured.inletHeads} inlet heads (canonical has ${MEASURED_COAST[0].inletHeads}), ` +
        `and the largest landmass holds ${measured.largestShare.toFixed(1)}% of the land.`;
      if (typeof presets.check === "function") {
        coastAdmissibleNote.textContent = presets.check(coastState)
          ? ""
          : "the engine will refuse this block — check the octave schedule in the query string.";
      }
    };

    for (const field of COAST_SLIDERS) {
      const { input } = coastRows[field];
      input.min = travel[field].min;
      input.max = travel[field].max;
      input.step = 1;
      input.value = travel[field].toPosition(coastState[field]);
      input.disabled = false;
      input.addEventListener("input", paint);
    }
    coastReset.disabled = false;
    fractalButton.disabled = false;
    // **Both buttons send the ENGINE'S OWN record back to the engine and restate nothing.**
    // `setAllCoast` writes every field of a preset -- the one that has a slider onto its slider,
    // and the five that do not straight into the state the readout and the query string both
    // read. That second half is why this is a loop over the block rather than over the widgets: a
    // preset half-applied because five of its fields had no widget is the silently-dropping-
    // builder shape, and this file's own history is four instances of a panel value that was not
    // the engine's value.
    const setAllCoast = (block) => {
      coastState = { ...block };
      for (const field of COAST_SLIDERS) {
        coastRows[field].input.value = travel[field].toPosition(block[field]);
      }
      paint();
    };
    fractalButton.addEventListener("click", () => setAllCoast(presets.fractal));
    coastReset.addEventListener("click", () => setAllCoast(presets.canonical));
    paint();
  }

  // === drainage — these rebuild ===========================================================
  //
  // **THE SECTION THAT PUTS VALLEYS ON A MOUNTAIN FLANK.** The owner held our ranges up against
  // satellite photographs of real ones: *"our mountains look like they were painted in with a
  // knife... they should look more like the 3rd and 4th image"*. What those photographs have is
  // dendritic V-notched valleys branching down the flanks with snow following the ridge lines, and
  // `erosion-architecture-spike.md` measured that the stream graph is **661x too coarse** to carry
  // them at any interpolation. `GullyParams` is the term that can, it shipped in the committed
  // artifact, and until this section existed **nothing in the viewer sent a gully block, so the
  // owner saw no change at all**.
  //
  // Rebuild-class for the same reason the relief, mountain and coast sliders are: `GullyParams` is
  // an argument to `Surface::with_gully`, resolved once at construction -- it builds the steering
  // lattice -- and every worker holds its own already-built world. There is no uniform to poke.
  //
  // **No gully number is written in this file**, and one of them matters more than any number on
  // any previous channel: `slope_reference` is a measurement of *this generator's flanks*, roughly
  // a hundredth of what the published technique assumes. A transcribed measurement is a
  // measurement with two answers, so the slider, both buttons and the readout are all anchored on
  // `wb_gully_preset`'s own record in `wireGully` below, and until the engine answers they are
  // disabled and say so.

  const gullySection = section(body, "drainage · live");
  const gullyLabels = {
    // Labelled for its EFFECT, not its exponent. "crest sharpness" means nothing to somebody
    // looking at a hillside; what the measured sweep actually moves is whether the term carves
    // valleys or blankets the whole flank.
    crestSharpness: "carve vs blanket",
    // Labelled for its effect too, and for the fact that its bottom position is the off switch.
    // The measured band is on the readout beside it.
    amplitudeM: "channel depth",
  };
  const gullyRows = {};
  for (const field of GULLY_SLIDERS) {
    gullyRows[field] = row(gullySection, gullyLabels[field], `wb-gully-${field}`,
      "range", { min: 0, max: 1, step: 1, value: 0, disabled: true });
    gullyRows[field].out.textContent = "—";
  }
  // The nine fields with no widget, SHOWN -- and on this channel that includes the amplitude, the
  // field that turns the whole term on. `GULLY_SLIDERS`' own comment gives the measured reason
  // each one is here rather than on a slider: the kernel's report contains exactly one table of a
  // field's value against a measured effect, and it sweeps the crest exponent. A slider whose
  // travel nobody has measured is a slider nobody can aim, and this viewer has shipped one. But
  // the preset carries all ten and the query string carries all ten, so a preset that changed
  // something the panel never mentioned would be a parameter the owner cannot see.
  const gullyScheduleNote = el("div", "wb-note", "—");
  gullySection.append(gullyScheduleNote);
  // And whether the engine would take the block, before generate rather than after. This channel
  // has a bound that closes a hazard rather than stating a domain -- a non-positive crest exponent
  // makes `0^s` infinite and turns **every gully crest in the world into an infinite height**,
  // which crosses the boundary as an f64 that looks like an f64 -- and three of its lengths become
  // a lattice index as `radius / length`. Asked of `wb_gully_check` through the engine, which is
  // the same validator the record will meet, rather than by re-deriving any of it in JavaScript.
  const gullyAdmissibleNote = el("div", "wb-note", "");
  gullySection.append(gullyAdmissibleNote);
  const gullyNote = el("div", "wb-note", "waiting for the engine…");
  gullySection.append(gullyNote);
  const gullyActions = el("div", "wb-actions");
  const drainageButton = el("button", "wb-mini", "drainage preset");
  drainageButton.type = "button";
  drainageButton.disabled = true;
  drainageButton.title =
    "GullyParams::drainage(), read from the engine — all twelve of its numbers, on the two sliders "
    + "and in the readout above";
  const gullyReset = el("button", "wb-mini", "canonical");
  gullyReset.type = "button";
  gullyReset.disabled = true;
  gullyReset.title =
    "back to the engine's canonical block, which builds no steering lattice at all — today's "
    + "picture, byte for byte";
  gullyActions.append(drainageButton, gullyReset);
  gullySection.append(gullyActions);

  /// The gully block the panel currently describes, or `null` while the engine has not answered.
  let gullyState = null;
  let gullyCanonical = null;

  /// What the swept table says about the exponent the slider is on. Read from `MEASURED_CREST`
  /// rather than typed here, so the panel and the sweep cannot disagree about what was measured.
  const crestRowFor = (exponent) => {
    let best = MEASURED_CREST[0];
    for (const r of MEASURED_CREST) {
      if (Math.abs(r.crestSharpness - exponent) < Math.abs(best.crestSharpness - exponent)) best = r;
    }
    return best;
  };

  /// Fill in the travel, the defaults and the readouts once the engine can be asked.
  function wireGully(presets) {
    gullyCanonical = presets.canonical;
    // **The panel-default family check, run in production and not only in a test.** It is a
    // sharper question on this channel than on the coast one: there the canonical value was the
    // slider's own minimum, so `min + 0 * step` answered it for free, and here canonical sits in
    // the MIDDLE of the travel and has to land on the lattice by arithmetic.
    const faults = panelFieldFaults(gullyPanelFields(presets.canonical));
    if (faults.length > 0) {
      gullyNote.textContent = `slider travel refused: ${faults.join("; ")}`;
      return;
    }
    const travel = gullyTravel(presets.canonical);
    // **Asked of EVERY slider, not of the one that had it first.** The second gully slider
    // arrived a slice after this check did, and a check named after one field is a check the next
    // field silently escapes -- which is the silently-dropping-builder shape wearing a panel's
    // clothes.
    const notHeld = GULLY_SLIDERS.filter((field) => !travel[field].holdsCanonical);
    if (notHeld.length > 0) {
      gullyNote.textContent =
        `slider travel refused: the engine's canonical ${notHeld.join(", ")} is not on this lattice`;
      return;
    }
    gullyState = { ...presets.canonical, ...(presets.chosen ?? {}) };

    const paint = () => {
      for (const field of GULLY_SLIDERS) {
        const value = travel[field].toValue(Number(gullyRows[field].input.value));
        gullyState[field] = value;
        gullyRows[field].out.textContent = travel[field].format(value);
      }
      // The other nine, read out of the state the preset button writes rather than from literals.
      // Asserted rather than trusted -- `gully-params.test.mjs` checks that the union of
      // `GULLY_SLIDERS` and the fields named on this line is `GULLY_CONTROLS`, so an eleventh
      // field added to the channel cannot arrive silently.
      const shown = gullyReadoutFields().map((f) => `${f} ${gullyState[f]}`).join(" · ");
      gullyScheduleNote.textContent = shown;
      const exponent = gullyState.crestSharpness;
      const measured = crestRowFor(exponent);
      // **The off switch is the amplitude, not the slider**, and a panel that let that go unsaid
      // would have a control that appears to do nothing on a fresh page. Said first, because it is
      // the first thing an owner sliding this will notice.
      const off = !(gullyState.amplitudeM > 0);
      const where = exponent >= CARVE_BAND.high
        ? "at the blanket end — the kernel is symmetric here and adds roughness everywhere "
          + "rather than carving valleys"
        : exponent <= CARVE_BAND.low
          ? "at the lowering end — this removes mean height from every gated flank rather than "
            + "texturing it"
          : "inside the carving band — valleys cut, crests left as spines";
      gullyNote.textContent = off
        ? "the kernel is OFF: canonical zeroes the amplitude, so no slider here changes a pixel. "
          + "Press the drainage preset to turn it on."
        : `${where}. At ${measured.crestSharpness.toFixed(2)} the term's mean over 16,000 samples `
          + `on 400 flank points is ${measured.mean.toFixed(2)} m `
          + `(p05 ${measured.p05.toFixed(1)}, p95 ${measured.p95.toFixed(1)}); a symmetric kernel `
          + `would read ${MEASURED_CREST[MEASURED_CREST.length - 1].mean.toFixed(2)} m. `
          + `The depth slider is at ${gullyState.amplitudeM} m, which the measured sweep puts at `
          + `about ${(gullyState.amplitudeM * MEASURED_AMPLITUDE_SWEEP.sites[0].reliefPerMetre)
            .toFixed(0)} m of local relief over a 2 km run `
          + `(${MEASURED_AMPLITUDE_SWEEP.sites[0].reliefPerMetre} m per metre of amplitude, `
          + `linear above ${MEASURED_AMPLITUDE_SWEEP.linearAboveM} m). Hammond's `
          + `${MEASURED_RELIEF.hammondHills.low}-${MEASURED_RELIEF.hammondHills.high} m hills band `
          + `is ${AMPLITUDE_BAND.hammondHillsLow}-${AMPLITUDE_BAND.hammondHillsHigh} m of depth `
          + `and his ${MEASURED_RELIEF.hammondMountainFloor} m mountain floor is `
          + `${AMPLITUDE_BAND.hammondMountainFloor} m, past the end of this travel. `
          + `This is texture on a generator whose mountains are tectonic; it is not a claim to `
          + `have made mountains. The steering bites at a slope of ${gullyState.slopeReference} `
          + `m/m (${MEASURED_SLOPE.degrees}°), measured on this generator — the published `
          + `technique assumes about ${MEASURED_SLOPE.publishedAssumption}, `
          + `${Math.round(MEASURED_SLOPE.publishedAssumption / MEASURED_SLOPE.reference)}x steeper.`;
      if (typeof presets.check === "function") {
        gullyAdmissibleNote.textContent = presets.check(gullyState)
          ? ""
          : "the engine will refuse this block — check the gully fields in the query string.";
      }
    };

    for (const field of GULLY_SLIDERS) {
      const { input } = gullyRows[field];
      input.min = travel[field].min;
      input.max = travel[field].max;
      input.step = 1;
      input.value = travel[field].toPosition(gullyState[field]);
      input.disabled = false;
      input.addEventListener("input", paint);
    }
    gullyReset.disabled = false;
    drainageButton.disabled = false;
    // **Both buttons send the ENGINE'S OWN record back to the engine and restate nothing.**
    // `setAllGully` writes every field of a preset -- the one that has a slider onto its slider,
    // and the nine that do not straight into the state the readout and the query string both
    // read. That second half is what makes the drainage button work at all: the field that turns
    // the kernel on has no widget, so a button that only moved widgets would change nine readouts
    // and not one pixel. That is the silently-dropping-builder shape exactly.
    const setAllGully = (block) => {
      gullyState = { ...block };
      for (const field of GULLY_SLIDERS) {
        gullyRows[field].input.value = travel[field].toPosition(block[field]);
      }
      paint();
    };
    drainageButton.addEventListener("click", () => setAllGully(presets.drainage));
    gullyReset.addEventListener("click", () => setAllGully(presets.canonical));
    paint();
  }

  // === clouds — these rebuild =============================================================
  //
  // **Rebuild-class, not live**, and for the same reason the relief sliders are: the coverage
  // decides a threshold every rasterised cloud texel is compared against, so moving it means
  // re-rasterising every tile in the layer. There is no uniform to poke the way the ramp and the
  // exaggeration have one.
  //
  // **The slider's travel IS the coverage**, and it comes from `panel-fields.js`, so this file
  // holds no cloud number at all — not the default and not either end. The readout says what the
  // number means, because a bare "0.40" against a label reading "coverage" is a number with no
  // referent, and the referent here is a measurement: the fraction of the sphere the calibrated
  // threshold actually puts at or above half opacity.
  const cloudSection = section(body, "clouds · rebuilds");
  const cloudCover = row(cloudSection, "coverage", "wb-clouds", "range", travelFor("clouds"));
  const cloudNote = el("div", "wb-note", "");
  cloudSection.append(cloudNote);
  const paintClouds = () => {
    const c = Number(cloudCover.input.value);
    cloudCover.out.textContent = c <= 0 ? "off" : `${(c * 100).toFixed(0)}%`;
    cloudNote.textContent = c <= 0
      ? "no cloud layer at all — it is not constructed, so the picture is the pre-cloud one byte "
        + "for byte."
      : `${(c * 100).toFixed(0)}% of the sphere at or above half opacity, set by inverting the `
        + "field's own measured distribution rather than by a nominal range. The north-star "
        + "reference reads about 40%. Banded by latitude: the ITCZ and both storm tracks carry "
        + "more than this, the subtropics less.";
  };
  cloudCover.input.addEventListener("input", paintClouds);
  paintClouds();

  // === water — this rebuilds ==============================================================
  //
  // **Rebuild-class, and the most expensive knob on this panel.** The node count is the stream
  // graph's resolution, so moving it re-resolves every basin, every spill level and every merge:
  // 4.2 s at the default and 9.3 s at 60,000 on the owner's world, measured through this
  // repository's checked-in wasm. There is no uniform to poke and no partial answer to show, so
  // the note says the seconds out loud — a cost an owner discovers by waiting is a cost the panel
  // failed to state.
  //
  // The travel comes from `panel-fields.js`, whose top end is the engine's own
  // `WB_MAX_WATER_NODES`, so this file holds no water number at all.
  //
  // **The readout names DRAWABLE bodies, not bodies.** A single-node body's extent is a point and
  // no texel centre ever lands on one, so a count of bodies would promise water the picture cannot
  // contain — 55 bodies on the owner's world at the default, of which 17 can be drawn. The live
  // figures are read off `window.__wb.water`, which is what the boot path actually resolved,
  // rather than recomputed here.
  const waterSection = section(body, "water · live");
  const lakeNodes = row(waterSection, "graph nodes", "wb-lake-nodes", "range", travelFor("lakeNodes"));
  const waterNote = el("div", "wb-note", "");
  waterSection.append(waterNote);
  // **Read at paint time, not at build time.** This used to be a `const` taken while the panel
  // was being constructed -- which is *before* `main.js` publishes `window.__wb`, so it was
  // always `undefined` and the live half of this note never once appeared. A live swap makes the
  // defect matter twice over: after a swap the figures are new, and a cached `undefined` would
  // report the resolution's cost as blank forever.
  const paintWater = () => {
    const resolved = window.__wb && window.__wb.water;
    const n = Number(lakeNodes.input.value);
    lakeNodes.out.textContent = n >= 1000 ? `${(n / 1000).toFixed(0)}k` : String(n);
    const live = resolved && resolved.nodeCount === n && resolved.enabled
      ? `${resolved.facts.drawable} drawable of ${resolved.facts.bodies} bodies, resolved in ${
        (resolved.ms / 1000).toFixed(2)} s. `
      : "";
    waterNote.textContent = `${live}A finer stream graph resolves more basins and therefore more `
      + "lakes; it also costs roughly linearly — about 4.2 s at 30k nodes. Each body is drawn as "
      + "a flat sheet at its own spill level. ?lakes=0 turns the whole thing off, including the "
      + "resolution.";
  };
  lakeNodes.input.addEventListener("input", paintWater);
  paintWater();

  // === appearance — live, no reload =======================================================

  const look = section(body, "appearance · live");
  const exag = row(look, "vert. exag", "wb-exag", "range", travelFor("exaggeration"));
  exag.input.addEventListener("input", () => {
    window.viewer.scene.verticalExaggeration = Number(exag.input.value);
    exag.out.textContent = `${exag.input.value}x`;
  });

  const rampLo = row(look, "ramp min", "wb-ramp-lo", "range", travelFor("rampMin"));
  const rampHi = row(look, "ramp max", "wb-ramp-hi", "range", travelFor("rampMax"));
  const paintRamp = () => {
    const m = window.viewer.scene.globe.material;
    if (m && m.uniforms && "minimumHeight" in m.uniforms) {
      m.uniforms.minimumHeight = Number(rampLo.input.value);
      m.uniforms.maximumHeight = Number(rampHi.input.value);
    }
    rampLo.out.textContent = `${(rampLo.input.value / 1000).toFixed(1)} km`;
    rampHi.out.textContent = `${(rampHi.input.value / 1000).toFixed(1)} km`;
  };
  rampLo.input.addEventListener("input", paintRamp);
  rampHi.input.addEventListener("input", paintRamp);

  const wireLine = el("label", "wb-row wb-check");
  const wire = document.createElement("input");
  wire.type = "checkbox";
  wire.id = "wb-wire";
  wire.addEventListener("change", () => {
    window.viewer.scene.globe._surface.tileProvider._debug.wireframe = wire.checked;
    window.viewer.scene.requestRender && window.viewer.scene.requestRender();
  });
  wireLine.append(wire, el("span", null, "wireframe (tile grid)"));
  look.append(wireLine);

  // === diagnostics — rebuild ==============================================================

  const diag = section(body, "diagnostics · rebuilds");
  const faultLine = el("label", "wb-row");
  faultLine.append(el("span", null, "fault"));
  const fault = document.createElement("select");
  fault.id = "wb-fault";
  for (const [value, label] of FAULT_OPTIONS) {
    const opt = document.createElement("option");
    opt.value = value;
    opt.textContent = label;
    if ((params.get("fault") || "") === value) opt.selected = true;
    fault.append(opt);
  }
  faultLine.append(fault, el("output"));
  diag.append(faultLine);
  diag.append(el("div", "wb-note",
    "A deliberately wrong implementation, so a check can be seen failing."));

  // === actions ============================================================================

  const sync = () => {
    plates.out.textContent = plates.input.value;
    land.out.textContent = Number(land.input.value).toFixed(2);
    // Three decimals, not one: at one decimal the snapped 6,400,000 m and the intended
    // 6,371,000 m both read "6.4 Mm", which is how the snap survived every screenshot.
    radius.out.textContent = `${(radius.input.value / 1e6).toFixed(3)} Mm`;
    size.out.textContent = `${size.input.value}²`;
    ceiling.out.textContent = `L${ceiling.input.value}`;
    // Post spacing from terrain.js's own derivation: pi * 6,371,000 / 64 m at level 0,
    // halving each level. Shown because "16" means nothing and "4.8 m" does.
    const spacing = 312735.73 / Math.pow(2, Number(zoom.input.value));
    zoom.out.textContent = `L${zoom.input.value} · ${
      spacing >= 1000 ? `${(spacing / 1000).toFixed(1)} km` : `${spacing.toFixed(0)} m`}`;
  };
  for (const r of [plates, land, radius, zoom, size, ceiling]) {
    r.input.addEventListener("input", sync);
  }
  sync();
  exag.out.textContent = `${exag.input.value}x`;
  paintRamp();

  const rebuildFields = () => ({
    seed: seed.input.value.trim(),
    plates: plates.input.value,
    land: land.input.value,
    radius: radius.input.value,
    maxLevel: zoom.input.value,
    size: size.input.value,
    featureCeiling: ceiling.input.value,
    harbour: harbour.checked ? "1" : null,
    fault: fault.value || null,
    // The cloud coverage. Written like every other range field: `apply` drops it when it still
    // equals the panel default, so an untouched panel writes no `clouds` parameter at all and
    // the boot path takes `DEFAULT_CLOUD_COVER` from one place rather than from a query string
    // that agrees with it.
    clouds: cloudCover.input.value,
    // The water manifest's node count. Written like every other range field: `apply` drops it
    // when it still equals the panel default, so an untouched panel writes no `lakeNodes`
    // parameter and the boot path takes `DEFAULT_WATER_NODES` from one place rather than from a
    // query string that agrees with it.
    lakeNodes: lakeNodes.input.value,
    // Every relief field still at canonical is dropped, so an untouched panel writes no
    // relief parameter at all and the reload takes the `None` path -- Ruling 1, held in the
    // one place a generate can break it.
    ...(reliefState && reliefCanonical ? reliefToParams(reliefState, reliefCanonical) : {}),
    // And the same for the mountains: every tectonic field still at canonical is dropped, so
    // an untouched panel writes no tectonic parameter at all and the reload takes the `None`
    // path -- Ruling 1, held in the one place a generate can break it.
    ...(tectonicState && tectonicCanonical
      ? tectonicToParams(tectonicState, tectonicCanonical)
      : {}),
    // And the same for the coastline: every coast field still at canonical is dropped, so an
    // untouched panel writes no coast parameter at all and the reload takes the `None` path --
    // RULING 1, held in the one place a generate can break it.
    ...(coastState && coastCanonical ? coastToParams(coastState, coastCanonical) : {}),
    // And the same for the drainage: every gully field still at canonical is dropped, so an
    // untouched panel writes no gully parameter at all and the reload takes the `None` path --
    // RULING 1, held in the one place a generate can break it. On this channel `None` is not
    // "the term adding zero": it is a `Surface` with no steering lattice built at all.
    ...(gullyState && gullyCanonical ? gullyToParams(gullyState, gullyCanonical) : {}),
  });

  // === the live swap ======================================================================
  //
  // **The three sections above that no longer reload**, and the whole of the owner's request.
  //
  // Only the fields those three sections own are sent. A live swap deliberately does NOT carry
  // whatever else is sitting on the panel: an owner who slid `plates` without pressing generate
  // has not asked for a new plate count, and applying it because they then nudged a mountain
  // would be the panel doing something nobody asked for.

  /// The query fields the live sections own, in the form `apply` would have written them.
  /// Canonical-valued fields are dropped by `nextQueryString`, exactly as `apply` drops them.
  const liveFields = () => ({
    ...(tectonicState && tectonicCanonical
      ? tectonicToParams(tectonicState, tectonicCanonical)
      : {}),
    ...(coastState && coastCanonical ? coastToParams(coastState, coastCanonical) : {}),
    ...(gullyState && gullyCanonical ? gullyToParams(gullyState, gullyCanonical) : {}),
    lakeNodes: lakeNodes.input.value,
  });

  /// **The swap, and the one line that makes "slid equals loaded" structural rather than hoped
  /// for.**
  ///
  /// The panel does not hand its slider state to the engine. It writes the query string it would
  /// have navigated to, and then reads the parameter blocks back out of that query string with
  /// `tectonicFromParams` / `coastFromParams` / `waterNodeCountFromParams` -- **the same readers
  /// `main.js`'s boot path calls, on the same string.** So a swapped world is by construction the
  /// world that URL loads, including the canonical-is-`null` path (Ruling 1): a block back at
  /// canonical is dropped from the query, and the reader then returns `null`, which reaches the
  /// engine as a null pointer rather than as a block that merely equals canonical.
  ///
  /// Rebuilding the panel's own numbers from the URL would be the drift hazard this file exists
  /// to avoid; reading the ENGINE's numbers from the URL is the opposite of it.
  const runSwap = debounceLatest(async () => {
    const wb = window.__wb;
    if (!wb || typeof wb.swap !== "function") return;
    const query = nextQueryString(location.search, liveFields(), DEFAULTS);
    const nextParams = new URLSearchParams(query);
    const spec = {};
    if (tectonicCanonical) spec.tectonics = tectonicFromParams(nextParams, tectonicCanonical);
    if (coastCanonical) spec.coast = coastFromParams(nextParams, coastCanonical);
    if (gullyCanonical) spec.gully = gullyFromParams(nextParams, gullyCanonical);
    generate.disabled = true;
    swapNote.textContent = "swapping…";
    try {
      const result = await wb.swap({ spec, waterNodes: waterNodeCountFromParams(nextParams) });
      // **The URL, after the swap rather than before it.** A `replaceState` that ran first would
      // leave the address bar describing a world the engine had refused.
      history.replaceState(null, "", query ? `?${query}` : location.pathname);
      swapNote.textContent = result.kind === "none"
        ? "nothing to swap — the sliders are where the drawn world already is"
        : `${result.kind} swap in ${(result.ms / 1000).toFixed(2)} s (${result.reason}). ` +
          "Tiles redraw after this; the camera has not moved.";
      paintWater();
    } catch (error) {
      // A refused block leaves the previous world drawn -- `WorldSwapper` builds before it frees
      // -- so the honest thing to say is that nothing changed, not that the viewer is broken.
      swapNote.textContent = `the engine refused this world; the drawn one is unchanged (${error})`;
    } finally {
      generate.disabled = false;
    }
  }, SWAP_DEBOUNCE_MS);

  /// Every live slider release goes through here. `change` and not `input`: a range input fires
  /// `change` when the drag ends, which is the event this feature can afford.
  const liveOn = () => live.checked;
  function wireLive(input) {
    input.addEventListener("change", () => { if (liveOn()) runSwap(); });
  }
  for (const field of TECTONIC_SLIDERS) wireLive(mountainRows[field].input);
  for (const field of COAST_SLIDERS) wireLive(coastRows[field].input);
  for (const field of GULLY_SLIDERS) wireLive(gullyRows[field].input);
  wireLive(lakeNodes.input);
  // The preset buttons move several sliders at once and fire no `change` at all, so they ask for
  // the swap themselves. A "ranges preset" that changed six readouts and left the planet alone
  // would look exactly like a broken preset.
  for (const button of [
    rangesButton, mountainReset, fractalButton, coastReset, drainageButton, gullyReset,
  ]) {
    button.addEventListener("click", () => { if (liveOn()) runSwap(); });
  }

  const actions = el("div", "wb-actions");
  const generate = el("button", "wb-go", "generate");
  generate.type = "button";
  generate.addEventListener("click", () => apply(rebuildFields()));
  const reset = el("button", "wb-mini", "reset");
  reset.type = "button";
  reset.addEventListener("click", () => { location.search = ""; });
  actions.append(generate, reset);
  body.append(actions);

  const jump = el("div", "wb-actions");
  const toHarbour = el("button", "wb-mini", "fly to harbour");
  toHarbour.type = "button";
  toHarbour.title = "the only place refinement continues past the ground cap";
  toHarbour.addEventListener("click", () => {
    if (!params.has("harbour")) {
      // Flying there without the feature shows bare ground and looks like a bug, so turn it
      // on and fly in one step rather than silently doing half of what was asked.
      apply({
        ...rebuildFields(), harbour: "1",
        fly: `${HARBOUR_VIEW.lat},${HARBOUR_VIEW.lon},${HARBOUR_VIEW.height}`,
      });
      return;
    }
    window.viewer.camera.flyTo({
      destination: Cesium.Cartesian3.fromDegrees(
        HARBOUR_VIEW.lon, HARBOUR_VIEW.lat, HARBOUR_VIEW.height),
      duration: 2.5,
    });
  });
  const home = el("button", "wb-mini", "whole planet");
  home.type = "button";
  home.addEventListener("click", () => window.viewer.camera.flyHome(1.5));
  jump.append(toHarbour, home);
  body.append(jump);

  const readout = el("div", "wb-readout");
  readout.id = "wb-readout";
  body.append(readout);

  // === worlds: save it, get back to it, find things in it ===================================
  //
  // Mounted before "not wired yet" because it is wired, and because the thing it prevents -
  // losing a world you liked to a browser refresh - has already happened once.
  mountWorldPanel(body, () => window.viewer);

  // === not wired yet ======================================================================

  const missing = section(body, "not wired yet");
  const list = el("ul", "wb-missing");
  for (const [name, why] of NOT_WIRED) {
    const item = document.createElement("li");
    item.append(el("span", "wb-missing-name", name));
    item.append(el("span", "wb-missing-why", why));
    list.append(item);
  }
  missing.append(list);

  document.body.append(panel);
  return {
    readout, wireRelief, reliefNote, wireTectonics, mountainNote, wireCoast, coastNote,
    wireGully, gullyNote,
    /// Repainted once boot has published `window.__wb`, so the water note can state what THIS
    /// page actually resolved rather than only what the slider asks for.
    paintWater,
  };
}

/// Camera altitude, cursor position and the terrain height under it.
///
/// `globe.getHeight` reads the tiles actually loaded, so it reports what is on screen rather
/// than what the provider would eventually produce. That is the honest number for a readout
/// labelled "ground": at a coarse zoom it is the coarse value, and watching it sharpen as
/// tiles arrive is the zoom cap made visible.
function wireReadout(readout) {
  const viewer = window.viewer;
  let cursor = null;

  viewer.canvas.addEventListener("mousemove", (event) => {
    const rect = viewer.canvas.getBoundingClientRect();
    cursor = new Cesium.Cartesian2(event.clientX - rect.left, event.clientY - rect.top);
  });
  viewer.canvas.addEventListener("mouseleave", () => { cursor = null; });

  const fmt = (m) => (Math.abs(m) >= 1000 ? `${(m / 1000).toFixed(2)} km` : `${m.toFixed(0)} m`);

  viewer.scene.postRender.addEventListener(() => {
    const cam = viewer.camera.positionCartographic;
    const lines = [`eye ${fmt(cam.height)}`];
    if (cursor) {
      const ray = viewer.camera.getPickRay(cursor);
      const hit = ray && viewer.scene.globe.pick(ray, viewer.scene);
      if (hit) {
        const c = Cesium.Cartographic.fromCartesian(hit);
        const h = viewer.scene.globe.getHeight(c);
        lines.push(
          `${Cesium.Math.toDegrees(c.latitude).toFixed(3)}° ${
            Cesium.Math.toDegrees(c.longitude).toFixed(3)}°`,
          Number.isFinite(h) ? `ground ${fmt(h)}` : "ground —",
        );
      }
    }
    readout.textContent = lines.join("  ·  ");
  });
}

// Module scripts run in document order, so `window.viewer` exists by the time this does, and
// `main.js` has already published `window.__wbBoot` -- a real promise, assigned at module
// evaluation rather than at the end of `boot`.
//
// The readout waits on it so it does not report ellipsoid heights and call them ground. The
// relief section waits on it because it genuinely cannot exist without the engine: its
// defaults and two of its three travel ends ARE `wb_relief_preset`'s answer, and there is no
// fallback set of numbers here to fall back to. If boot fails, the sliders stay disabled and
// the note says why, which is honest; a panel that showed plausible relief defaults over a
// dead engine would be the drift hazard again, wearing a different hat.
const {
  readout, wireRelief, reliefNote, wireTectonics, mountainNote, wireCoast, coastNote,
  wireGully, gullyNote, paintWater,
} = build();
const booted = window.__wbBoot && typeof window.__wbBoot.then === "function"
  ? window.__wbBoot
  : Promise.resolve();
booted
  .then(() => {
    wireReadout(readout);
    const presets = window.__wb && window.__wb.relief;
    if (presets) wireRelief(presets);
    else reliefNote.textContent = "engine unavailable — relief cannot be read or set";
    const tectonics = window.__wb && window.__wb.tectonics;
    if (tectonics) wireTectonics(tectonics);
    else mountainNote.textContent = "engine unavailable — mountains cannot be read or set";
    const coast = window.__wb && window.__wb.coast;
    if (coast) wireCoast(coast);
    else coastNote.textContent = "engine unavailable — the coastline cannot be read or set";
    const gully = window.__wb && window.__wb.gully;
    if (gully) wireGully(gully);
    else gullyNote.textContent = "engine unavailable — the drainage cannot be read or set";
    paintWater();
  })
  .catch((error) => {
    wireReadout(readout);
    reliefNote.textContent = `engine unavailable — relief cannot be set (${error})`;
    mountainNote.textContent = `engine unavailable — mountains cannot be set (${error})`;
    coastNote.textContent = `engine unavailable — the coastline cannot be set (${error})`;
    gullyNote.textContent = `engine unavailable — the drainage cannot be set (${error})`;
  });
