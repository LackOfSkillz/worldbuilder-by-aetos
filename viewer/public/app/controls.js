// A control panel over the parameters `main.js` already reads from the query string, plus
// an honest list of the things it cannot drive yet.
//
// Every knob in the first four sections existed before this file did, and every one of them
// was reachable only by typing a URL. Nothing new is exposed to the engine; what is new is
// that a person can turn them. The last section is the opposite: capabilities that exist in
// the engine or the roadmap and have **no viewer path at all**, listed rather than omitted
// so the panel does not read as a complete account of what the project does.
//
// ## Why changing a world knob reloads the page
//
// The world is built once, at boot: `main.js` calls `engine.newWorld(...)` and hands the
// handle to a terrain provider, a worker pool and a tile cache that all close over it.
// Rebuilding in place would mean tearing down the pool, invalidating every cached tile and
// swapping the provider under a camera mid-flight -- three things with their own failure
// modes, none of which this panel needs. Setting `location.search` reuses the boot path the
// verification harness already covers. It costs a reload; it cannot leave the viewer in a
// state no test has seen.
//
// View and appearance knobs that Cesium can change live do so without a reload, and are
// marked as such. The readouts never reload.
//
// ## CSP
//
// Served under `default-src 'self'` with no `'unsafe-inline'`, so this is a module file and
// its styles live in `viewer.css`. See `index.html`'s comment.

import { PANEL_DEFAULTS, PANEL_RANGES } from "./panel-fields.js";
import {
  RELIEF_CONTROLS, RELIEF_PARAM_NAMES, HURST_BAND, hurst, sliderTravel, reliefToParams,
} from "./relief-params.js";

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
const NOT_WIRED = [
  ["erosion", "wb_erosion_run ships in the .wasm; nothing in the viewer calls it"],
  // Ruling 4 of the relief-amplitude slice, and it is a measurement, not a scheduling note.
  // The highest point on this planet is 1,381 m and 1,378 m of that is the structural
  // (tectonic) term, so no relief parameter can move a mountain's height at all; Ruling 6
  // measured the roughness spectrum topping out at 161 m on peaks and 82 m on land at the
  // most extreme corner ever swept. `mountainM` below is a roughness budget on high ground,
  // and labelling it "mountain height" would be the wrong thing wearing the right label.
  ["mountain height", "tectonic, not relief: 1,378 m of the 1,381 m peak is structural"],
  ["mountain count", "tectonic: plate collisions place them, and no relief knob reaches that"],
  ["lakes + water", "slice 5b, in progress: no export yet"],
  ["rivers", "schema only in Mark 2; reaches are carried, not populated"],
  ["place areas", "slice 3, the studio: not started"],
  ["export to Evennia", "slice 2a apply: not started"],
  ["climate + biomes", "designed, approved for after 5b: not built"],
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
    // Every relief field still at canonical is dropped, so an untouched panel writes no
    // relief parameter at all and the reload takes the `None` path -- Ruling 1, held in the
    // one place a generate can break it.
    ...(reliefState && reliefCanonical ? reliefToParams(reliefState, reliefCanonical) : {}),
  });

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
  return { readout, wireRelief, reliefNote };
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
const { readout, wireRelief, reliefNote } = build();
const booted = window.__wbBoot && typeof window.__wbBoot.then === "function"
  ? window.__wbBoot
  : Promise.resolve();
booted
  .then(() => {
    wireReadout(readout);
    const presets = window.__wb && window.__wb.relief;
    if (presets) wireRelief(presets);
    else reliefNote.textContent = "engine unavailable — relief cannot be read or set";
  })
  .catch((error) => {
    wireReadout(readout);
    reliefNote.textContent = `engine unavailable — relief cannot be set (${error})`;
  });
