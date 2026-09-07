// The save, reopen, recover and find controls, as a section of the existing panel.
//
// Kept out of `controls.js` deliberately. That file's job is the sliders that describe a planet;
// this one's job is keeping a planet you liked and getting back to a place in it. They fail
// differently, they are read by different people at different moments, and `controls.js` is
// already twelve hundred lines.
//
// **"Recover" is the first button rather than a footnote**, because losing a world you liked is
// the thing that actually happened, and a recovery buried under a save you never made is no use
// at all.

import {
  autosave, buildWorldfile, checkVersion, download, forget, remember,
  savedWorlds, searchFromPlanet, startAutosave, suspectValues, trail, urlFor,
} from "./worlds.js";
import { drawAreas } from "./area-markers.js";
import { findLakeIslands, flyTo } from "./find-places.js";
import { enablePicking, flyFragment, markPick } from "./pick-point.js";

function el(tag, cls, text) {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (text !== undefined) node.textContent = text;
  return node;
}

function button(label, cls = "wb-mini") {
  const node = el("button", cls, label);
  node.type = "button";
  return node;
}

function when(iso) {
  const then = new Date(iso);
  const seconds = Math.max(0, (Date.now() - then.getTime()) / 1000);
  if (seconds < 90) return `${Math.round(seconds)}s ago`;
  if (seconds < 5400) return `${Math.round(seconds / 60)}m ago`;
  if (seconds < 172800) return `${Math.round(seconds / 3600)}h ago`;
  return then.toISOString().slice(0, 10);
}

/// Build the section and attach it to a parent element.
///
/// `getViewer` is a function rather than a viewer, because the panel is built before boot
/// publishes `window.__wb` and a captured `undefined` would be permanent.
export function mountWorldPanel(parent, getViewer) {
  // Declared here rather than beside the area buttons: the save, the picker and the library all
  // touch them, and `let` in a later block is a temporal-dead-zone error rather than undefined.
  let lastAreas = [];
  let drawn = null;

  const wrap = el("div", "wb-section");
  wrap.append(el("div", "wb-section-title", "worlds"));

  // **The pick readout goes FIRST, not last.** It was appended at the bottom of a section that
  // already carried six control rows, so a click produced a correct answer below the fold and
  // the feature read as broken. A result nobody can find has not been delivered.
  const pickRow = el("div", "wb-jump");
  const pickToggle = button("pick a point: off", "wb-mini wb-pick-toggle");
  pickRow.append(pickToggle);
  const pickOut = el("div", "wb-pick");
  pickOut.hidden = true;
  wrap.append(pickRow, pickOut);

  const note = el("div", "wb-note");
  wrap.append(note);

  // --- recover -------------------------------------------------------------------------------

  const recoverRow = el("div", "wb-jump");
  const recover = button("recover last view");
  recover.addEventListener("click", () => {
    const entries = trail();
    if (entries.length === 0) {
      note.textContent = "nothing recorded yet - autosave starts with this page";
      return;
    }
    // The newest entry is where we are now. The one before it is where we were.
    const target = entries.find((entry) => urlFor(entry) !== `${location.pathname}${location.search}`)
      || entries[0];
    note.textContent = `going back to ${when(target.at)}`;
    location.href = urlFor(target);
  });
  const listRecent = button("recent places");
  recoverRow.append(recover, listRecent);
  wrap.append(recoverRow);

  const recent = el("div", "wb-note");
  recent.hidden = true;
  wrap.append(recent);
  listRecent.addEventListener("click", () => {
    recent.hidden = !recent.hidden;
    if (recent.hidden) return;
    recent.textContent = "";
    const entries = trail();
    if (entries.length === 0) {
      recent.textContent = "autosave has recorded nothing yet";
      return;
    }
    for (const entry of entries.slice(0, 10)) {
      const line = el("div", "wb-row");
      const go = button(
        entry.camera
          ? `${when(entry.at)} · ${entry.camera[0]}, ${entry.camera[1]} · ${Math.round(entry.camera[2] / 1000)} km`
          : `${when(entry.at)} · no camera`,
      );
      go.addEventListener("click", () => { location.href = urlFor(entry); });
      line.append(go);
      recent.append(line);
    }
  });

  // --- save and open -------------------------------------------------------------------------

  const nameField = document.createElement("input");
  nameField.type = "text";
  nameField.placeholder = "name this world";
  nameField.className = "wb-text";
  wrap.append(nameField);

  const saveRow = el("div", "wb-jump");
  const save = button("save world");
  save.addEventListener("click", () => {
    const name = nameField.value.trim() || `world-${Date.now()}`;
    const document_ = buildWorldfile(name, location.search, lastAreas);
    // **Say so before writing it, not after.** A parameter the engine could not parse fell back
    // to canonical, so the world drawn is not the world the file names - and a save that records
    // a rejected input is a save of a planet nobody has seen.
    const suspect = suspectValues(document_.planet);
    remember(document_);
    const filename = download(document_);
    note.textContent = `saved ${filename} · ${Object.keys(document_.planet).length} parameters, `
      + `${(document_.areas || []).length} areas`
      + (suspect.length
        ? ` · WARNING: ${suspect.map(([k, v]) => `${k}=${v}`).join(", ")} `
          + "is not a number, so the engine ignored it and drew its canonical value instead"
        : "");
    paintSaved();
  });

  const openField = document.createElement("input");
  openField.type = "file";
  openField.accept = "application/json,.json";
  openField.className = "wb-file";
  const open = button("open worldfile");
  open.addEventListener("click", () => openField.click());
  openField.addEventListener("change", async () => {
    const file = openField.files && openField.files[0];
    if (!file) return;
    try {
      await loadWorldfile(JSON.parse(await file.text()));
    } catch (error) {
      note.textContent = `refused: ${error.message}`;
    }
  });
  saveRow.append(save, open);
  wrap.append(saveRow, openField);

  const saved = el("div", "wb-note");
  wrap.append(saved);

  /// Load one worldfile by URL: check it, take its areas, and go to its planet.
  ///
  /// Shared by the file picker and the library list, because "open a file" and "open a file the
  /// server already has" differ only in where the JSON comes from. Two copies of this would be
  /// two chances for the library to load a world the picker would have refused.
  async function loadWorldfile(document_) {
    checkVersion(document_);
    lastAreas = document_.areas || [];
    const search = searchFromPlanet(document_.planet);
    const here = new URLSearchParams(location.search);
    const there = new URLSearchParams(search.replace(/^\?/, ""));
    here.delete("fly");
    // Compared as parsed parameters rather than as strings: the same planet written in a
    // different order is the same planet, and a string test would reload the page forever.
    const same = [...there.keys()].every((key) => here.get(key) === there.get(key))
      && [...here.keys()].every((key) => there.get(key) === here.get(key));
    if (!same) {
      // The areas are pinned to ground that only exists on the file's own planet, so the world
      // has to arrive before they are drawn.
      sessionStorage.setItem("wb.pendingAreas", JSON.stringify(lastAreas));
      location.href = search;
      return;
    }
    note.textContent = `opened "${document_.name}" · ${lastAreas.length} areas`;
    redrawAreas();
  }

  /// The library: worldfiles the server has on disk, listed by the server rather than guessed.
  const library = el("div", "wb-note");
  wrap.append(library);
  async function paintLibrary() {
    library.textContent = "";
    let rows = [];
    try {
      const response = await fetch("/worlds/");
      rows = (await response.json()).worlds || [];
    } catch {
      library.textContent = "no world library on this server";
      return;
    }
    if (rows.length === 0) {
      library.textContent = "the world library is empty";
      return;
    }
    library.append(el("div", "wb-section-title", `on disk (${rows.length})`));
    for (const row of rows) {
      const line = el("div", "wb-row");
      const label = row.error
        ? `${row.file} · ${row.error}`
        : `${row.name} · ${row.areas} areas${row.seed ? ` · seed ${row.seed}` : ""}`;
      const go = button(label);
      go.disabled = Boolean(row.error);
      go.addEventListener("click", async () => {
        try {
          const response = await fetch(`/worlds/${encodeURIComponent(row.file)}`);
          await loadWorldfile(await response.json());
        } catch (error) {
          note.textContent = `refused: ${error.message}`;
        }
      });
      line.append(go);
      library.append(line);
    }
  }
  paintLibrary();

  function paintSaved() {
    const entries = savedWorlds();
    saved.textContent = "";
    if (entries.length) saved.append(el("div", "wb-section-title", "saved in this browser"));
    for (const entry of entries.slice(0, 6)) {
      const line = el("div", "wb-row");
      const go = button(`${entry.name} · ${entry.areas} areas`);
      go.addEventListener("click", () => { location.href = entry.search || location.pathname; });
      const drop = button("×");
      drop.addEventListener("click", () => { forget(entry.name); paintSaved(); });
      line.append(go, drop);
      saved.append(line);
    }
  }
  paintSaved();

  // --- areas ---------------------------------------------------------------------------------

  function redrawAreas() {
    const viewer = getViewer();
    if (!viewer || typeof Cesium === "undefined") return;
    if (drawn) drawn.remove();
    drawn = drawAreas(viewer, Cesium, { areas: lastAreas });
    paintAreas();
    note.textContent = `${drawn.count} areas on the globe`;
  }

  // **A list, because pins are not findable by eye at planet scale.** Three areas two
  // thousand kilometres apart are three pixels, and "show areas" drew them correctly while
  // leaving the owner with no way to reach any of them. The list is how you get there.
  const areaList = el("div", "wb-note");
  wrap.append(areaList);
  function paintAreas() {
    areaList.textContent = "";
    if (!lastAreas.length) return;
    areaList.append(el("div", "wb-section-title", `areas (${lastAreas.length})`));
    for (const area of lastAreas) {
      const anchor = area.anchor || {};
      const rooms = (area.rooms || []).length;
      const line = el("div", "wb-row");
      const go = button(`${area.name} · ${rooms} rooms`);
      go.addEventListener("click", () => {
        const viewer = getViewer();
        if (!viewer) return;
        // Framed on the area's own extent rather than a fixed height: a 177-room city and
        // an 11-room camp need very different ranges to fill the same screen.
        let span = 0;
        for (const room of area.rooms || []) {
          span = Math.max(span,
            Math.abs(room.latitude_deg - anchor.latitude_deg),
            Math.abs(room.longitude_deg - anchor.longitude_deg));
        }
        const height = Math.max(1200, span * 111320 * 6);
        viewer.camera.flyTo({
          destination: Cesium.Cartesian3.fromDegrees(
            anchor.longitude_deg, anchor.latitude_deg, height),
          orientation: { heading: 0, pitch: -Math.PI / 2, roll: 0 },
          duration: 2.0,
        });
        note.textContent = `${area.name} · ${anchor.latitude_deg.toFixed(5)}, `
          + `${anchor.longitude_deg.toFixed(5)} · ${Math.round(height)} m up`;
      });
      line.append(go);
      areaList.append(line);
    }
  }

  const areaRow = el("div", "wb-jump");
  const showAreas = button("show areas");
  showAreas.addEventListener("click", redrawAreas);
  const flyAreas = button("fly to areas");
  flyAreas.addEventListener("click", () => {
    if (drawn) drawn.flyToAll();
    else note.textContent = "no areas loaded - open a worldfile with areas in it";
  });
  areaRow.append(showAreas, flyAreas);
  wrap.append(areaRow);

  // --- find ----------------------------------------------------------------------------------

  const findRow = el("div", "wb-jump");
  const findIslands = button("find lake islands");
  findIslands.addEventListener("click", () => {
    const wb = window.__wb;
    if (!wb || !wb.water) {
      note.textContent = "the water solve has not finished yet";
      return;
    }
    const viewer = getViewer();
    // `elevationM` takes the world HANDLE as its first argument - `wb.world` is that handle, not
    // an object with methods on it. Reading `wb.world` fresh on every call rather than capturing
    // it is deliberate: a live swap replaces the handle, and a captured one would sample the
    // world that was on screen before the slider moved.
    const elevationAt = (latitude, longitude) => {
      try {
        return wb.engine.elevationM(wb.world, latitude, longitude);
      } catch {
        return Number.NaN;
      }
    };
    const result = findLakeIslands(wb.water, elevationAt, (wb.water.seaLevelM || 0));
    // The denominator, always. A search that could only look at nine bodies of a hundred has
    // not searched the world, and saying "none found" without that is a lie of omission.
    const looked = `looked at ${result.examined} of ${result.total} lakes `
      + `(${result.skippedPointBox} too small to have an inside, ${result.skippedWideBox} `
      + "with a box too wide to be one lake)";
    if (result.hits.length === 0) {
      note.textContent = `no island in any lake · ${looked}`;
      return;
    }
    const best = result.hits[0];
    note.textContent = `${result.hits.length} lakes with an island · ${looked}`;
    found.textContent = "";
    for (const hit of result.hits.slice(0, 8)) {
      const line = el("div", "wb-row");
      const go = button(
        `${hit.surroundings.isIsland ? "on an island · " : ""}`
        + `${hit.latitude.toFixed(3)}, ${hit.longitude.toFixed(3)} · `
        + `${Math.round(hit.heightAboveLakeM)} m above a lake ${hit.spanKm.lat.toFixed(1)} km across`,
      );
      go.addEventListener("click", () => {
        if (!viewer) return;
        viewer.camera.flyTo({
          destination: Cesium.Cartesian3.fromDegrees(hit.longitude, hit.latitude, 25000),
          duration: 2,
        });
        autosave(location.search, [hit.latitude, hit.longitude, 25000, 0, -90]);
      });
      line.append(go);
      found.append(line);
    }
    void best;
  });
  findRow.append(findIslands);
  wrap.append(findRow);

  const found = el("div", "wb-note");
  wrap.append(found);


  // --- pick a point: the wiring. The controls are mounted at the top of the section. --------

  let picking = null;
  let pickMarker = null;
  pickToggle.addEventListener("click", () => {
    if (picking) {
      picking.stop();
      picking = null;
      pickToggle.textContent = "pick a point: off";
      pickToggle.classList.remove("wb-pick-on");
      return;
    }
    const viewer = getViewer();
    if (!viewer || typeof Cesium === "undefined") {
      note.textContent = "the globe is not ready yet";
      return;
    }
    const wb = window.__wb;
    const elevationAt = wb
      ? (latitude, longitude) => {
        try {
          return wb.engine.elevationM(wb.world, latitude, longitude);
        } catch {
          return null;
        }
      }
      : null;
    picking = enablePicking(viewer, Cesium, (pick) => {
      pickMarker = markPick(viewer, Cesium, pick, pickMarker);
      const height = viewer.camera.positionCartographic.height;
      const fragment = flyFragment(pick, height);
      pickOut.hidden = false;
      pickOut.textContent = "";

      // A selectable field rather than only a button. Clipboard access can be refused by the
      // browser and by the page's own policy, and a number you cannot select is a number you
      // have to retype off the screen.
      const field = document.createElement("input");
      field.type = "text";
      field.readOnly = true;
      field.className = "wb-text";
      field.value = `${pick.latitude.toFixed(6)}, ${pick.longitude.toFixed(6)}`;
      field.addEventListener("focus", () => field.select());

      pickOut.append(field);
      pickOut.append(el("div", "wb-pick-line",
        `ground ${pick.elevationM === null ? "?" : `${Math.round(pick.elevationM)} m`}`
        + `  ·  from ${pick.source}`
        + (pick.offsetM > 1
          ? `  ·  up to ${Math.round(pick.offsetM)} m out at this angle`
            + " - look straight down to remove it"
          : "")));

      const flyField = document.createElement("input");
      flyField.type = "text";
      flyField.readOnly = true;
      flyField.className = "wb-text";
      flyField.value = `?${fragment}`;
      flyField.addEventListener("focus", () => flyField.select());
      pickOut.append(flyField);

      const copy = button("copy both");
      copy.addEventListener("click", async () => {
        const text = `${field.value}
?${fragment}`;
        try {
          await navigator.clipboard.writeText(text);
          copy.textContent = "copied";
          setTimeout(() => { copy.textContent = "copy both"; }, 1500);
        } catch {
          field.focus();
          copy.textContent = "clipboard refused - the fields are selected";
        }
      });
      pickOut.append(copy);

      autosave(location.search, [
        Number(pick.latitude.toFixed(6)), Number(pick.longitude.toFixed(6)),
        Math.round(height), 0, -90,
      ]);
    }, elevationAt);
    pickToggle.textContent = "pick a point: ON - click the globe";
    pickToggle.classList.add("wb-pick-on");
  });

  parent.append(wrap);

  // Autosave starts as soon as there is a viewer to read a camera from, and keeps trying until
  // there is - boot is asynchronous and a one-shot attempt would silently never arm.
  let stop = null;
  const arm = setInterval(() => {
    const viewer = getViewer();
    if (!viewer) return;
    clearInterval(arm);
    stop = startAutosave(viewer);
    const pending = sessionStorage.getItem("wb.pendingAreas");
    if (pending) {
      sessionStorage.removeItem("wb.pendingAreas");
      try {
        lastAreas = JSON.parse(pending);
        redrawAreas();
      } catch { /* a bad handover is not worth a broken panel */ }
    }
  }, 500);

  return { wrap, redrawAreas, stop: () => { clearInterval(arm); if (stop) stop(); } };
}
