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
  clearPainted, paintedWork,
  savedWorlds, searchFromPlanet, startAutosave, suspectValues, trail, urlFor,
} from "./worlds.js";
import { drawAreas } from "./area-markers.js";
import { findLakeIslands, flyTo } from "./find-places.js";
import { enablePicking, flyFragment, markPick, pickAt } from "./pick-point.js";
import { Route, drawRoute, saveRoute } from "./route.js";
import {
  PREVIEW_PARAMS, decodeHydro, drawPreview, forcedOutletsFromParams, outletPath,
} from "./water-preview.js";

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
  let lastRoads = [];
  //: The rest of the network, and what the run decided about levels. Held beside the roads
  //: because "save world" writes whatever is held here and nothing else.
  let lastNetwork = { ferries: [], ferry_lines: [], level_bands: [] };
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
  save.addEventListener("click", async () => {
    const name = nameField.value.trim() || `world-${Date.now()}`;
    // Whatever has been painted and applied, in the worldfile's own vocabulary. `main.js`
    // keeps it here as strokes are committed; without it a save writes a planet with no
    // mountains on it and says nothing.
    const wb = window.__wb || {};
    const painted = (wb.lastWorldfile && wb.lastWorldfile.features) || [];
    // **Strokes that were painted and never applied are not in the world yet.** They are
    // ghosts held for a commit, by design - painting is cheap and rebuilding the globe is
    // not. But that means a save can honestly write a world with no mountains in it while
    // the mountains are on screen, which looks exactly like the save losing them. Say so
    // rather than write it silently.
    const held = wb.heldFeatures ? wb.heldFeatures().length : 0;
    if (held) {
      note.textContent = `${held} painted strokes are not applied yet - press `
        + `"apply ${held} strokes" first, or they will not be in the file`;
      return;
    }
    const document_ = buildWorldfile(name, location.search, lastAreas, painted,
                                     wb.spec || null, lastRoads, lastNetwork);
    // **Say so before writing it, not after.** A parameter the engine could not parse fell back
    // to canonical, so the world drawn is not the world the file names - and a save that records
    // a rejected input is a save of a planet nobody has seen.
    const suspect = suspectValues(document_.planet);
    remember(document_);
    // **Written to the server first, downloaded only if that fails.** A download lands in a
    // folder the world library cannot list and the generator cannot read, so a world saved
    // that way is saved to nowhere the rest of the tool can see it. The file still comes
    // down when there is no server to take it, which is the offline case and not the normal
    // one.
    let where;
    try {
      const response = await fetch("/worlds/", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(document_),
      });
      if (!response.ok) throw new Error(`server said ${response.status}`);
      where = `${(await response.json()).saved} on the server`;
      clearPainted();
      paintLibrary();
    } catch (error) {
      where = `${download(document_)} to your downloads (the server refused: ${error.message})`;
    }
    note.textContent = `saved ${where} · ${Object.keys(document_.planet).length} parameters, `
      + `${(document_.features || []).length} painted features, `
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
  /// Take a finished run's areas and roads as this world's own.
  ///
  /// **A generated world that cannot be saved is a demonstration, not a tool.** The areas
  /// a run makes live in the populate layer; the world file was written from `lastAreas`,
  /// which only ever held what an OPENED file contained. So saving after a run wrote the
  /// planet and the four areas that were there before it, and a hundred and thirty
  /// generated ones went in the bin the moment the tab was closed - with no warning,
  /// because the save itself succeeded.
  ///
  /// Called by the populate layer when a run finishes. Replaces rather than merges: the
  /// run generated this world's areas, and appending would double them on a second run.
  function adoptRun(areas, roads, network = {}) {
    lastAreas = Array.isArray(areas) ? areas : [];
    lastRoads = Array.isArray(roads) ? roads : [];
    lastNetwork = { ferries: network.ferries || [],
                    ferry_lines: network.ferry_lines || [],
                    level_bands: network.level_bands || [] };
    paintAreas();
    return lastAreas.length;
  }
  // **Heard as an event, not published as a global.** `main.js` assigns `window.__wb` a
  // fresh object literal once the engine has loaded, which is after the panels mount and
  // after any timeout short enough to be worth writing - so a property set on that object
  // is a property thrown away, and the symptom is a hook that exists in the source and not
  // in the browser. An event has no ordering to get wrong.
  window.addEventListener("wb-run-finished", (event) => {
    const held = (event && event.detail) || {};
    adoptRun(held.areas || [], held.roads || [], held);
  });

  async function loadWorldfile(document_) {
    checkVersion(document_);
    lastAreas = document_.areas || [];
    lastRoads = document_.roads || [];
    lastNetwork = { ferries: document_.ferries || [],
                    ferry_lines: document_.ferry_lines || [],
                    level_bands: document_.level_bands || [] };
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
      // The features cross the reload beside the areas, for the same reason: the file is
      // about to be forgotten and the new page has no way back to it.
      sessionStorage.setItem("wb.pendingFeatures",
                             JSON.stringify(document_.features || []));
      location.href = search;
      return;
    }
    // **A saved world comes back with its mountains.** The features are what the planet
    // block cannot hold, so opening a file that has them and installing only its sliders
    // reopens a world missing everything anybody painted into it - the same loss as the
    // save that dropped them, one step later.
    const painted = document_.features || [];
    let restored = "";
    if (painted.length && window.__wb && window.__wb.holdFeatures) {
      window.__wb.discardFeatures();
      window.__wb.holdFeatures(painted);
      const applied = await window.__wb.commitFeatures();
      restored = `, ${applied.applied} painted features`;
    }
    note.textContent = `opened "${document_.name}" · ${lastAreas.length} areas${restored}`;
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

  /// The worldfile on disk whose planet is the one on screen, if any carries areas.
  ///
  /// Matched on the PLANET rather than the name, because a world can be saved under several
  /// names and only the parameters decide whether the coordinates in it mean anything here.
  /// A file for a different planet is skipped rather than drawn: its rooms would be pinned
  /// to ground that does not exist on this one.
  async function areasForThisPlanet() {
    let rows = [];
    try {
      rows = (await (await fetch("/worlds/")).json()).worlds || [];
    } catch {
      return null;
    }
    const here = new URLSearchParams(location.search);
    here.delete("fly");
    let best = null;
    for (const row of rows) {
      if (!row.areas) continue;
      let document_;
      try {
        document_ = await (await fetch(`/worlds/${encodeURIComponent(row.file)}`)).json();
      } catch {
        continue;
      }
      const there = new URLSearchParams(searchFromPlanet(document_.planet).replace(/^\?/, ""));
      // **A near miss is reported, not discarded.** One moved slider makes this planet a
      // different planet from the one the areas were sited on, which is true - and answering
      // "no worldfile on this server carries areas for this planet" makes a city of a hundred
      // and seventy-seven rooms simply vanish with no clue as to why. Moving the mountain
      // height and losing the Landing looks like a bug in the areas; it is one number in the
      // query string. So the closest worldfile is kept along with what differs about it.
      const differs = [...there.keys()]
        .filter((key) => here.get(key) !== there.get(key))
        .map((key) => `${key} ${there.get(key)} not ${here.get(key) ?? "unset"}`);
      const areas = document_.areas || [];
      const better = !best
        || differs.length < best.differs.length
        || (differs.length === best.differs.length && areas.length > best.areas.length);
      if (better) best = { name: document_.name || row.file, areas, differs };
    }
    return best;
  }

  const areaRow = el("div", "wb-jump");
  const showAreas = button("hide areas");

  /// Load the areas for this planet and put them on the globe.
  ///
  /// **On by default, and turned OFF by choice.** A world with areas in it that shows an
  /// empty globe until somebody finds the right button is a world that looks empty - and
  /// after a reload the first thing anybody wants is the thing they were just looking at.
  /// The button now says what pressing it will DO, so its label is the state you are not
  /// in.
  async function loadAndDrawAreas() {
    let warning = "";
    // **"Show areas" with nothing loaded used to say "0 areas on the globe" and stop.**
    // That is true and useless: the areas were on disk, in a worldfile for this very planet,
    // and the button that says "show areas" is exactly where somebody expects that to be
    // noticed. It looks for them now instead of reporting their absence.
    if (!lastAreas.length) {
      const found = await areasForThisPlanet();
      if (!found) {
        note.textContent = "no worldfile on this server carries any areas";
        return;
      }
      lastAreas = found.areas;
      // Shown either way. Areas sited on a planet whose parameters have since moved are
      // still worth looking at - they are just no longer standing on the ground they were
      // placed on, and that is the thing to say rather than the thing to hide.
      //
      // Said AFTER the redraw, because `redrawAreas` writes its own count into the same
      // line and would otherwise wipe the warning in the same tick it was written.
      warning = found.differs.length
        ? `on a different planet: ${found.differs.slice(0, 3).join("; ")} - the ground under `
          + "them has moved"
        : "";
    }
    redrawAreas();
    if (warning) note.textContent += ` (${warning})`;
  }

  showAreas.addEventListener("click", async () => {
    if (drawn) {
      drawn.remove();
      drawn = null;
      showAreas.textContent = "show areas";
      note.textContent = "areas hidden";
      return;
    }
    showAreas.textContent = "hide areas";
    await loadAndDrawAreas();
  });
  const flyAreas = button("fly to areas");
  flyAreas.addEventListener("click", async () => {
    if (!drawn) {
      showAreas.click();
      await new Promise((resolve) => setTimeout(resolve, 250));
    }
    if (drawn) drawn.flyToAll();
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

  // --- water preview ---------------------------------------------------------------------
  //
  // **View-only.** This bakes `wb_hydro_bake` for the world on screen and draws the result;
  // it carves nothing and saves nothing. The bake runs in the pool when there is one -- about
  // 87 s at a million nodes on the owner's world, which is why it goes through a worker
  // rather than the main thread wherever a worker is available.

  const previewTitle = el("div", "wb-section-title", "water preview");
  const previewRow = el("div", "wb-jump");
  const previewButton = button("preview water");
  previewRow.append(previewButton);
  const previewNote = el("div", "wb-note");
  wrap.append(previewTitle, previewRow, previewNote);

  let previewLayer = null;

  /// Take the preview off the globe, if it is on. Shared by the toggle-off click and by the
  /// "the world changed under it" listener below.
  function dropPreview(message) {
    if (!previewLayer) return;
    previewLayer.remove();
    previewLayer = null;
    previewButton.textContent = "preview water";
    if (message) previewNote.textContent = message;
  }
  // **The preview is drawn against one bake of one world.** A commit that rebuilds the ground
  // (`wb-world-rebuilt`) leaves a stale preview floating over new terrain, which looks like a
  // river that moved on its own rather than like a picture nobody re-drew.
  window.addEventListener("wb-world-rebuilt", () => dropPreview("water preview cleared - the world changed"));

  previewButton.addEventListener("click", async () => {
    if (previewLayer) {
      dropPreview("water preview hidden");
      return;
    }
    const viewer = getViewer();
    const wb = window.__wb;
    if (!viewer || typeof Cesium === "undefined" || !wb) {
      previewNote.textContent = "the globe is not ready yet";
      return;
    }
    previewButton.disabled = true;
    previewNote.textContent = "working out the water... (about a minute and a half at a million points)";
    try {
      const params = {
        ...PREVIEW_PARAMS,
        forcedOutlets: forcedOutletsFromParams(new URLSearchParams(location.search)),
      };
      let words;
      if (wb.pool) {
        words = (await wb.pool.hydro({ params })).words;
      } else {
        previewNote.textContent += " - no worker pool (?workers=0), running on the main thread";
        words = wb.engine.hydroBake({ handle: wb.world, params });
      }
      const decoded = decodeHydro(words);
      previewLayer = drawPreview(viewer, Cesium, decoded);
      const fresh = decoded.bodies.filter((b) => b.fresh).length;
      const salt = decoded.bodies.length - fresh;
      const byClass = { stream: 0, river: 0, great: 0 };
      for (const reach of decoded.reaches) byClass[reach.class] = (byClass[reach.class] || 0) + 1;
      const outlet = outletPath(decoded);
      let outletText = "";
      if (outlet.reachIds.length) {
        const lastId = outlet.reachIds[outlet.reachIds.length - 1];
        const lastReach = decoded.reaches.find((r) => r.id === lastId);
        const lastPoint = lastReach && lastReach.points[lastReach.points.length - 1];
        if (lastPoint) {
          outletText = ` · great-lake outlet -> ${outlet.end} at `
            + `${lastPoint.lat.toFixed(3)}, ${lastPoint.lon.toFixed(3)}`;
        }
      }
      let forcedText = "";
      if (decoded.header.forcedRequested > 0) {
        forcedText = ` · ${decoded.header.forcedMatched}/${decoded.header.forcedRequested} forced outlets matched`;
      }
      previewNote.textContent = `${decoded.bodies.length} lakes (${fresh} fresh, ${salt} salt), `
        + `${decoded.reaches.length} reaches (${byClass.stream}/${byClass.river}/${byClass.great})`
        + `, ${decoded.falls.length} waterfalls`
        + `, ${decoded.header.pondsKept} ponds`
        + `, ${decoded.header.crossingsLeft} crossings left`
        + outletText + forcedText;
      previewButton.textContent = "hide water preview";
    } catch (error) {
      previewNote.textContent = `water preview failed: ${error.message}`;
    } finally {
      previewButton.disabled = false;
    }
  });

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

  // --- route: click a path, describe each stop ------------------------------------------
  //
  // Separate from "pick a point" on purpose. Picking answers "where is this?" and replaces
  // its answer each time; routing answers "what goes here, in what order?" and accumulates.
  // One toggle doing both would mean every stray click while reading a coordinate silently
  // extended a path somebody was authoring.

  const routeTitle = el("div", "wb-section-title", "route");
  const routeRow = el("div", "wb-jump");
  const routeToggle = button("place nodes: off", "wb-mini wb-pick-toggle");
  const routeClear = button("clear all");
  const routeSave = button("save route");
  routeRow.append(routeToggle, routeClear, routeSave);
  const routeName = document.createElement("input");
  routeName.type = "text";
  routeName.placeholder = "name this route";
  routeName.className = "wb-text";
  const routeNote = el("div", "wb-note");
  const routeNodes = el("div", "wb-note");
  wrap.append(routeTitle, routeRow, routeName, routeNote, routeNodes);

  const route = new Route();
  let routeSource = null;
  let routeHandler = null;

  function paintRoute() {
    const viewer = getViewer();
    if (viewer && typeof Cesium !== "undefined") {
      routeSource = drawRoute(viewer, Cesium, route, routeSource);
    }
    const radius = Number(new URLSearchParams(location.search).get("radius")) || 6371000;
    const { legs, total } = route.legs(radius);
    routeNote.textContent = route.nodes.length
      ? `${route.nodes.length} nodes · ${(total / 1000).toFixed(2)} km total`
        + (legs.length ? ` · longest leg ${(Math.max(...legs) / 1000).toFixed(2)} km` : "")
      : "no nodes yet - turn placing on and click the globe";

    routeNodes.textContent = "";
    route.nodes.forEach((node, index) => {
      const line = el("div", "wb-row");
      const pick = button(
        `${index + 1}. ${node.latitude_deg.toFixed(5)}, ${node.longitude_deg.toFixed(5)}`
        + ` · ${node.elevation_m === null ? "?" : `${node.elevation_m.toFixed(1)} m`}`
        + (node.elevation_m !== null && node.elevation_m < 0 ? " (water)" : ""),
      );
      pick.addEventListener("click", () => {
        route.selected = index;
        paintRoute();
        const viewer = getViewer();
        if (viewer) {
          viewer.camera.flyTo({
            destination: Cesium.Cartesian3.fromDegrees(
              node.longitude_deg, node.latitude_deg, 2500),
            orientation: { heading: 0, pitch: -Math.PI / 2, roll: 0 },
            duration: 1.2,
          });
        }
      });
      const drop = button("×");
      drop.addEventListener("click", () => { route.remove(index); paintRoute(); });
      const note = document.createElement("input");
      note.type = "text";
      note.className = "wb-text";
      note.placeholder = "what should be here?";
      note.value = node.note;
      // Written straight onto the node as it is typed. A "save note" button is one more
      // thing to forget, and a note that was typed and not saved is worse than no note.
      note.addEventListener("input", () => { node.note = note.value; });
      note.addEventListener("change", paintRoute);
      line.append(pick, drop);
      routeNodes.append(line, note);
    });
  }

  routeToggle.addEventListener("click", () => {
    if (routeHandler) {
      routeHandler.stop();
      routeHandler = null;
      routeToggle.textContent = "place nodes: off";
      routeToggle.classList.remove("wb-pick-on");
      return;
    }
    const viewer = getViewer();
    if (!viewer || typeof Cesium === "undefined") {
      routeNote.textContent = "the globe is not ready yet";
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
    routeHandler = enablePicking(viewer, Cesium, (pick) => {
      route.add(pick.latitude, pick.longitude, pick.elevationM);
      paintRoute();
    }, elevationAt);
    routeToggle.textContent = "place nodes: ON - click the globe";
    routeToggle.classList.add("wb-pick-on");
  });

  routeClear.addEventListener("click", () => { route.clear(); paintRoute(); });

  routeSave.addEventListener("click", async () => {
    if (!route.nodes.length) {
      routeNote.textContent = "nothing to save";
      return;
    }
    route.name = routeName.value.trim() || `route-${Date.now()}`;
    try {
      const result = await saveRoute(route.toJSON(planetFromSearchLocal()));
      routeNote.textContent = `saved ${result.saved} · ${result.nodes} nodes`;
    } catch (error) {
      routeNote.textContent = `save failed: ${error.message}`;
    }
  });

  function planetFromSearchLocal() {
    const params = new URLSearchParams(location.search);
    const planet = {};
    for (const [key, value] of params.entries()) {
      if (key === "fly") continue;
      planet[key] = value;
    }
    return planet;
  }

  paintRoute();

  parent.append(wrap);

  // Autosave starts as soon as there is a viewer to read a camera from, and keeps trying until
  // there is - boot is asynchronous and a one-shot attempt would silently never arm.
  let stop = null;
  const arm = setInterval(() => {
    const viewer = getViewer();
    if (!viewer) return;
    clearInterval(arm);
    stop = startAutosave(viewer);
    // Areas are shown without being asked for; see `loadAndDrawAreas`.
    loadAndDrawAreas().catch((error) => {
      note.textContent = `could not show areas: ${error.message}`;
    });

    const pending = sessionStorage.getItem("wb.pendingAreas");
    if (pending) {
      sessionStorage.removeItem("wb.pendingAreas");
      try {
        lastAreas = JSON.parse(pending);
        redrawAreas();
      } catch { /* a bad handover is not worth a broken panel */ }
    }
    // **Painted work that was never saved comes back by itself.** An editor that loses a
    // refresh's worth of painting is an editor people stop trusting with an afternoon of it.
    // Only when the world has none of its own, so opening a saved file cannot double them.
    const unsaved = paintedWork(location.search);
    if (unsaved) {
      const restore = (tries = 0) => {
        const wb = window.__wb;
        if (!wb || !wb.holdFeatures) {
          if (tries < 40) setTimeout(() => restore(tries + 1), 250);
          return;
        }
        if ((wb.spec.features || []).length) return;
        wb.holdFeatures(unsaved.features);
        wb.commitFeatures().then(() => {
          note.textContent = `recovered ${unsaved.features.length} painted features from `
            + `${unsaved.saved_at.slice(0, 16).replace("T", " ")} - not yet saved to a world`;
        });
      };
      restore();
    }

    const painted = sessionStorage.getItem("wb.pendingFeatures");
    if (painted) {
      sessionStorage.removeItem("wb.pendingFeatures");
      // Waits for the commit path to exist: this arms half a second after the viewer, and
      // `main.js` publishes `holdFeatures` at the end of its boot.
      const apply = (tries = 0) => {
        const wb = window.__wb;
        if (!wb || !wb.holdFeatures) {
          if (tries < 40) setTimeout(() => apply(tries + 1), 250);
          return;
        }
        try {
          const records = JSON.parse(painted);
          if (!records.length) return;
          wb.holdFeatures(records);
          wb.commitFeatures();
        } catch { /* a bad handover is not worth a broken world */ }
      };
      apply();
    }
  }, 500);

  return { wrap, redrawAreas, stop: () => { clearInterval(arm); if (stop) stop(); } };
}
