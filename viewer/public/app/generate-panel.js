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
  count.max = "250";
  count.step = "5";
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
  const note = el("div", "wb-note-line", "pick a world and a count");

  let watching = null;

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
      go.textContent = "stop watching";
      watching = watchRun(viewer, window.Cesium, result.run_id,
                          (drawn, total) => {
                            note.textContent = `${drawn} areas on the globe (run ${result.run_id})`;
                          },
                          { worldName: worlds.value.replace(/\.json$/, "") });
    } catch (error) {
      note.textContent = `refused: ${error.message}`;
    } finally {
      go.disabled = false;
    }
  });

  wrap.append(worldRow, countRow, count, regionRow,
              el("div", "wb-row").appendChild(go).parentNode, note);
  parent.append(wrap);
  refreshWorlds();
  return { wrap, refreshWorlds, stop: () => { if (watching) watching.stop(); } };
}
