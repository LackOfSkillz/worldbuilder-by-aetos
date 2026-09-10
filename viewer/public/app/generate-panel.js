//! The button that populates a world, and the readout that watches it happen.
//
// **The generator is Python and this is a browser**, so the server stands between them:
// `POST /generate/` spawns the runner and answers with a run id, and `watchRun` follows
// `/progress/` from there. The run keeps going whether or not this page is open, which is
// the property that made a polled file the right choice over a socket - a reload picks the
// feed up where it left off.
//
// **Nothing here decides anything about the world.** The count, the region and the world
// are the only inputs; every judgement about what goes where belongs to the generator, and
// duplicating any of it in a panel would give the tool two answers to one question.

import { watchRun } from "./populate.js";
import { buildTally } from "./tally.js";

function el(tag, cls, text) {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (text !== undefined) node.textContent = text;
  return node;
}

/// The demo region: the frame the first hundred areas are asked to land in.
///
/// Written here rather than in the generator because it is a decision about THIS world -
/// where somebody wants to build - and not a fact about any planet.
export const DEMO_REGION = "-26.5,26.5,-36.5,36.0";

//: Where the run being watched is remembered across a reload.
//
// **A refresh must not lose the run.** The generator is a process on the server and the
// feed is a file, so a reload can pick up exactly where it left off - but only if the page
// remembers which run it was watching. Without this a stray refresh mid-run leaves a
// finished world on disk and an empty globe, with no way back to it.
const WATCHING_KEY = "wb.watchingRun";

/// Build the populate section into a parent element.
///
/// Args:
///   parent: where to attach.
///   getViewer: `() => viewer`, because the globe may not exist yet when this is built.
export function buildGeneratePanel(parent, getViewer) {
  const wrap = el("div", "wb-section");
  wrap.append(el("div", "wb-section-title", "populate"));

  const worldRow = el("div", "wb-row");
  const worlds = document.createElement("select");
  worlds.className = "wb-text";
  worldRow.append(worlds);

  const countRow = el("div", "wb-row");
  const countLabel = el("label", "wb-brush-field", "areas 100");
  const count = document.createElement("input");
  count.type = "range";
  count.min = "5";
  count.max = "400";
  // **Step of one, because the number is a number.** Fives felt tidy on a slider and meant
  // somebody who wanted a hundred and twenty-three areas got a hundred and twenty-five - a
  // control quietly overruling the person using it.
  count.step = "1";
  count.value = "100";
  count.addEventListener("input", () => {
    countLabel.textContent = `areas ${count.value}`;
  });
  countRow.append(countLabel);

  const regionRow = el("div", "wb-row");
  const wholePlanet = el("button", "wb-mini", "region: inland sea");
  wholePlanet.type = "button";
  let region = DEMO_REGION;
  wholePlanet.addEventListener("click", () => {
    region = region ? "" : DEMO_REGION;
    wholePlanet.textContent = region ? "region: inland sea" : "region: whole planet";
  });
  regionRow.append(wholePlanet);

  const go = el("button", "wb-mini wb-mini-go", "populate world");
  go.type = "button";
  // **Hidden is not gone.** The tally can be put away with its own close button, and a
  // readout you cannot get back is one nobody dares close - so the way back sits beside the
  // button that made it.
  const showSummary = el("button", "wb-mini", "run summary");
  showSummary.type = "button";
  showSummary.addEventListener("click", async () => {
    const tally = watching && watching.tally;
    if (tally) {
      if (tally.hidden()) tally.show();
      else tally.hide();
      return;
    }
    // **A run outlives the tab it was watched in.** The watcher is remembered in
    // `sessionStorage`, which a closed browser throws away - so somebody who looked at a
    // world, went to bed and came back had no way to see its summary again, and the run was
    // sitting on disk the whole time. Rebuilt from the run's own files instead.
    note.textContent = "looking for the last run...";
    const rebuilt = await summariseLastRun();
    note.textContent = rebuilt || "no run to summarise yet";
  });

  //: The tally rebuilt from disk, kept so a second press puts it away again.
  let recalled = null;

  /// Draw the last finished run's summary, from the files it left behind.
  ///
  /// Returns:
  ///   said (string): What to tell the panel, or "" if there was nothing to show.
  const summariseLastRun = async () => {
    if (recalled) {
      if (recalled.hidden()) recalled.show();
      else recalled.hide();
      return "";
    }
    try {
      const runs = (await (await fetch("/runs/", { cache: "no-store" })).json()).runs || [];
      if (!runs.length) return "";
      // Newest first, and the newest COMPLETE one: a run that failed half way has a
      // manifest and a worldfile and is not a world anybody wants summarised.
      for (const id of runs) {
        const manifest = await (await fetch(`/runs/${id}/manifest.json`,
                                            { cache: "no-store" })).json();
        if (manifest.status !== "complete") continue;
        const world = await (await fetch(`/runs/${id}/worldfile.json`,
                                         { cache: "no-store" })).json();
        // The world's own name, taken off the path the run was given. Both separators
        // are looked for by hand: a backslash inside a character class is one escape
        // too many to survive being written, and a Windows path then never split.
        const given = String((manifest.inputs || {}).worldfile || "");
        const cut = Math.max(given.lastIndexOf("/"), given.lastIndexOf(String.fromCharCode(92)));
        const named = (cut >= 0 ? given.slice(cut + 1) : given).replace(".json", "");
        recalled = buildTally(window.document, named || manifest.label || id);
        for (const area of world.areas || []) {
          // The tally counts a feed's shape, where `rooms` is a number. A worldfile holds
          // the rooms themselves, so the count is handed over rather than the list.
          recalled.add({ ...area, rooms: (area.rooms || []).length });
        }
        recalled.finish(manifest.summary || {});
        return `summary of ${manifest.label || id}`;
      }
      return "";
    } catch {
      return "";
    }
  };
  const note = el("div", "wb-note-line", "pick a world and a count");

  let watching = null;

  /// Follow a run, live or finished, and report as it lands.
  /// Follow a run, live or finished, and report as it lands.
  ///
  /// **The card goes up before the generator answers.** Spawning Python, loading the engine
  /// and scoring the ground take the better part of a minute between the click and the
  /// first pin, and until the card exists there is nothing on screen saying anything has
  /// happened at all.
  // --- curation: the same run, carried on by the server -------------------------------
  //
  // **The server decides whether a run is curated, not this page.** It starts the curator
  // the moment the generator exits, if curation is switched on - so a four-hundred-area run
  // is curated even if nobody is watching when it finishes. This only watches, adopts the
  // result, and forwards pause, resume and stop.
  let curationTimer = null;
  let curationRun = null;
  const stopCurationWatch = () => {
    if (curationTimer) clearTimeout(curationTimer);
    curationTimer = null;
  };
  const forgetRun = () => {
    try {
      sessionStorage.removeItem(WATCHING_KEY);
    } catch { /* nothing to clean up */ }
  };

  /// Take the curated world as this world: the room cards read it, and "save world" writes
  /// it, so the new text is not left behind in `runs/` while the old text is saved.
  const adoptCurated = async (runId) => {
    try {
      const response = await fetch(`/runs/${encodeURIComponent(runId)}/curate/curated.json`,
                                    { cache: "no-store" });
      const doc = await response.json();
      window.dispatchEvent(new CustomEvent("wb-run-curated", { detail: { run: runId, doc } }));
      window.dispatchEvent(new CustomEvent("wb-run-finished", {
        detail: { areas: doc.areas || [], roads: doc.roads || [], ferries: doc.ferries || [],
                  ferry_lines: doc.ferry_lines || [], level_bands: doc.level_bands || [] },
      }));
      note.textContent = "curated · save world keeps the new text";
    } catch (error) {
      note.textContent = `curated, but the curated world could not be read: ${error.message}`;
    }
  };

  const watchCuration = (runId, tally) => {
    stopCurationWatch();
    curationRun = runId;
    let waited = 0;
    const act = async (action) => {
      try {
        await fetch(`/curate/?run=${encodeURIComponent(runId)}&action=${action}`,
                    { method: "POST" });
      } catch { /* the next poll says what actually happened */ }
      stopCurationWatch();
      poll();
    };
    const poll = async () => {
      if (curationRun !== runId) return;
      let status;
      try {
        const response = await fetch(`/curate/?run=${encodeURIComponent(runId)}`,
                                     { cache: "no-store" });
        status = await response.json();
      } catch {
        curationTimer = setTimeout(poll, 5000);
        return;
      }
      if (status.state === "none") {
        // The server starts curation as the generator exits. A run with curation switched
        // off never gets past here, and after a few looks the page stops asking.
        waited += 1;
        if (waited < 5) curationTimer = setTimeout(poll, 1500);
        else forgetRun();
        return;
      }
      if (tally) tally.curation(status, act);
      if (status.state === "running" || status.state === "starting") {
        curationTimer = setTimeout(poll, 2000);
        return;
      }
      if (status.state === "done") {
        await adoptCurated(runId);
        forgetRun();
        return;
      }
      // Paused, stopped, interrupted or failed: the dial says which and the buttons offer
      // what can be done about it. Still looked at, slowly - another tab, or the command
      // line, may resume it, and a page that stopped asking would go on saying "paused".
      curationTimer = setTimeout(poll, 15000);
    };
    poll();
  };

  const follow = (runId, wanted, worldName) => {
    const viewer = getViewer();
    if (!viewer || !window.Cesium) return null;
    stopCurationWatch();
    let curationWatched = false;
    // **The previous run's pins come off before this one's go on.** Each run adds its own
    // data source and nothing removed the last, so running three times stacked three worlds
    // on one globe - the same good sites picked repeatedly, drawn as clumps of overlapping
    // dots with the labels on top of each other. It reads as a generator that piles areas
    // up, and it is a viewer that never cleared the table.
    if (watching) watching.remove();
    go.textContent = "stop watching";
    return watchRun(viewer, window.Cesium, runId, (drawn, total, finished) => {
      if (!finished) {
        note.textContent = wanted ? `${drawn} of ${wanted} areas...` : `${drawn} areas...`;
        return;
      }
      // **A run that stops short must say so.** Eighty-three of a hundred looks identical
      // to a run still working, and the difference between "thinking" and "finished, and
      // here is why it could not place the rest" is the whole of whether somebody trusts
      // the tool. The generator already records which quotas went unfilled; this is that,
      // said out loud.
      // **Short of the count and short of a quota are different things and used to read
      // the same.** The second pass fills the number that was asked for with whatever the
      // ground will take, so a run can deliver every area AND still have found nowhere for
      // a dwarf hold. Saying "125 of 125 areas - no ground left for 5 dwarf" states both
      // at once and sounds like a contradiction.
      const summary = finished.summary || {};
      const short = summary.unfilled || {};
      const missing = Object.entries(short).map(([race, n]) => `${n} ${race}`).join(", ");
      const overQuota = summary.over_quota || 0;
      if (summary.short_of) {
        note.textContent = `done: ${drawn} of ${wanted} areas - the region ran out of `
          + `ground${missing ? ` for ${missing}` : ""}`;
      } else if (overQuota) {
        note.textContent = `done: ${drawn} areas - ${overQuota} outside the quota, `
          + `no ground for ${missing}`;
      } else {
        note.textContent = `done: ${drawn} areas, every quota filled`;
      }
      go.textContent = "populate world";
      // Generation is over; curation, if it is on, is only starting. The run stays
      // remembered until curation settles, so a reload mid-curation finds its way back.
      if (!curationWatched) {
        curationWatched = true;
        watchCuration(runId, watching && watching.tally);
      }
    }, { worldName, wanted });
  };

  // Pick a run back up after a reload. It replays from the first line, so the globe comes
  // back exactly as it was - and if the generator is still going, the rest arrives live.
  const resume = () => {
    let kept = null;
    try {
      kept = JSON.parse(sessionStorage.getItem(WATCHING_KEY) || "null");
    } catch {
      kept = null;
    }
    if (!kept || !kept.run_id) return;
    const attach = (tries = 0) => {
      if (!getViewer() || !window.Cesium) {
        if (tries < 40) setTimeout(() => attach(tries + 1), 250);
        return;
      }
      watching = follow(kept.run_id, kept.count, kept.world);
      note.textContent = `picking up run ${kept.run_id}...`;
    };
    attach();
  };

  const refreshWorlds = async () => {
    try {
      const rows = (await (await fetch("/worlds/")).json()).worlds || [];
      worlds.textContent = "";
      for (const row of rows) {
        const option = document.createElement("option");
        option.value = row.file;
        option.textContent = `${row.file.replace(/\.json$/, "")} · ${row.areas || 0} areas`;
        worlds.append(option);
      }
      if (!rows.length) note.textContent = "no worlds on the server to build on";
    } catch (error) {
      note.textContent = `could not list worlds: ${error.message}`;
    }
  };

  go.addEventListener("click", async () => {
    const viewer = getViewer();
    if (!viewer || !window.Cesium) {
      note.textContent = "the globe is not ready yet";
      return;
    }
    if (watching) {
      // A second press stops following. The run itself keeps going - it is a process on
      // the server, and killing it from here would be a different and more dangerous
      // button than the one somebody pressed.
      watching.stop();
      watching = null;
      stopCurationWatch();
      curationRun = null;
      go.textContent = "populate world";
      note.textContent = "stopped watching; the run carries on";
      return;
    }
    go.disabled = true;
    note.textContent = "starting the generator...";
    try {
      const response = await fetch("/generate/", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          world: worlds.value,
          count: Number(count.value),
          label: `populate-${count.value}`,
          region: region || undefined,
        }),
      });
      const result = await response.json();
      if (!response.ok) throw new Error(result.error || `server said ${response.status}`);
      note.textContent = `run ${result.run_id} - ${result.count} areas wanted`;
      try {
        sessionStorage.setItem(WATCHING_KEY, JSON.stringify({
          run_id: result.run_id, count: result.count,
          world: worlds.value.replace(/\.json$/, ""),
        }));
      } catch { /* a refresh will simply not resume */ }
      watching = follow(result.run_id, result.count, worlds.value.replace(/\.json$/, ""));
    } catch (error) {
      note.textContent = `refused: ${error.message}`;
    } finally {
      go.disabled = false;
    }
  });

  const buttons = el("div", "wb-row");
  buttons.append(go, showSummary);
  wrap.append(worldRow, countRow, count, regionRow, buttons, note);
  parent.append(wrap);
  refreshWorlds();
  resume();
  return { wrap, refreshWorlds, stop: () => { if (watching) watching.stop(); } };
}
