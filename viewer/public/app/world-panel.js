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
  autosave, buildWorldfile, checkVersion, download, forget, planetFromSearch, remember,
  savedWorlds, searchFromPlanet, startAutosave, trail, urlFor,
} from "./worlds.js";
import { drawAreas } from "./area-markers.js";
import { findLakeIslands, flyTo } from "./find-places.js";

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
  const wrap = el("div", "wb-section");
  wrap.append(el("div", "wb-section-title", "worlds"));
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
    remember(document_);
    const filename = download(document_);
    note.textContent = `saved ${filename} · ${Object.keys(document_.planet).length} parameters, `
      + `${(document_.areas || []).length} areas`;
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
      const document_ = checkVersion(JSON.parse(await file.text()));
      lastAreas = document_.areas || [];
      note.textContent = `opened "${document_.name}" · ${lastAreas.length} areas`;
      const search = searchFromPlanet(document_.planet);
      if (search !== location.search) {
        // The planet in the file is not the planet on screen, so the areas would be pinned to
        // ground that does not exist. Reload onto the file's own world first; the areas are
        // redrawn on the way back in.
        sessionStorage.setItem("wb.pendingAreas", JSON.stringify(lastAreas));
        location.href = search;
        return;
      }
      redrawAreas();
    } catch (error) {
      note.textContent = `refused: ${error.message}`;
    }
  });
  saveRow.append(save, open);
  wrap.append(saveRow, openField);

  const saved = el("div", "wb-note");
  wrap.append(saved);
  function paintSaved() {
    const entries = savedWorlds();
    saved.textContent = "";
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

  let lastAreas = [];
  let drawn = null;
  function redrawAreas() {
    const viewer = getViewer();
    if (!viewer || typeof Cesium === "undefined") return;
    if (drawn) drawn.remove();
    drawn = drawAreas(viewer, Cesium, { areas: lastAreas });
    note.textContent = `${drawn.count} areas on the globe`;
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
