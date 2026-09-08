//! Watch a populate run land on the globe, one area at a time.
//
// **A cursor over a growing file, not a socket.** The run appends one JSON line per area
// as it finishes; this asks for everything past the line it has already drawn. That
// survives a page reload, a viewer opened halfway through the run, and a generator that
// dies at area sixty - a socket handles all three badly and a cursor handles them for free.
//
// **Pins are appended, never redrawn.** The area-marker work already learned this the hard
// way: `dataSources.add` is asynchronous, so a remove-then-add redraw leaks, and after one
// "clear all" there were three sources on the globe with two still holding pins. This owns
// exactly one source and only ever pushes into it.

import { enableAreaInput } from "./area-markers.js";
import { buildTally } from "./tally.js";
import { fillFor, outlineFor, outlineWidthFor, legendRows } from "./palette.js";

/// How often to ask. Two seconds is slower than areas land and that is deliberate: the
/// point is to watch a world fill in, and a pin appearing every couple of seconds reads as
/// a world being built, where a hundred arriving at once reads as a page load.
const POLL_MS = 2000;

/// The hover card's text: what this place is and what is in it.
///
/// `unique_items` counts DISTINCT things purchasable here, not stock on the shelves -
/// "forty swords" tells a player nothing, "nine kinds of blade" tells them whether the trip
/// is worth making. A field the generator did not fill is left out rather than shown as
/// zero, because a missing count and a genuine none are different facts.
function describe(area) {
  const lines = [area.display_name || area.name || "area"];
  if (area.culture) lines.push(area.culture);
  const who = [area.race, area.profession].filter(Boolean).join(" · ");
  if (who) lines.push(who);
  if (area.faction && area.faction !== "friendly") lines.push(area.faction.toUpperCase());
  lines.push("");
  const row = (k, v) => lines.push(k.padEnd(15) + v);
  if (area.rooms !== undefined) row("rooms", area.rooms);
  if (area.shops !== undefined) row("shops", area.shops);
  if (area.unique_items !== undefined) row("goods on sale", area.unique_items);
  if (area.npcs !== undefined) row("inhabitants", area.npcs);
  if (Array.isArray(area.level_band) && area.level_band.length === 2) {
    row("levels", area.level_band.join("-"));
  }
  return lines.join("\n");
}

function label(area) {
  const band = Array.isArray(area.level_band) && area.level_band.length === 2
    ? `lvl ${area.level_band[0]}-${area.level_band[1]}` : "";
  const name = area.display_name || area.name || "area";
  return band ? `${name}\n${band}` : name;
}

/// Follow a run and drop a pin as each area lands.
///
/// Args:
///   viewer: the Cesium viewer.
///   Cesium: the namespace.
///   runId: which run to watch.
///   onTick: optional `(drawn, total)` callback, for a counter in the UI.
///
/// Returns a handle with `stop()`, `count()` and the data source.
export function watchRun(viewer, Cesium, runId, onTick = null,
                         { worldName = null, tally = true } = {}) {
  const source = new Cesium.CustomDataSource(`wb-populate-${runId}`);
  viewer.dataSources.add(source);
  // The counts climb beside the pins. One card per run, removed with it.
  const counts = tally ? buildTally(window.document, worldName || "—") : null;
  let cursor = 0;
  let stopped = false;
  let timer = null;

  const draw = (area) => {
    if (area.latitude_deg === undefined || area.longitude_deg === undefined) return;
    source.entities.add({
      name: area.name,
      // What the hover card says. Labelled rather than a run-on line: this is read
      // while a hundred land, so the eye wants the same fact in the same place.
      description: describe(area),
      position: Cesium.Cartesian3.fromDegrees(area.longitude_deg, area.latitude_deg),
      point: {
        pixelSize: 10,
        color: fillFor(Cesium, area),
        // Faction fills, race rings: a hostile lizard-folk town is a red dot ringed in
        // jade, so "dangerous" reads from orbit without losing who lives there.
        outlineColor: outlineFor(Cesium, area),
        outlineWidth: outlineWidthFor(area),
        // The three rules the area pins already follow, for the same reasons: never
        // depth-tested into the terrain, never range-culled, scaled rather than fixed.
        disableDepthTestAgainstTerrain: true,
        disableDepthTestDistance: Number.POSITIVE_INFINITY,
        heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
        scaleByDistance: new Cesium.NearFarScalar(2.0e5, 1.6, 2.0e7, 0.5),
      },
      label: {
        text: label(area),
        font: "12px system-ui, sans-serif",
        fillColor: Cesium.Color.WHITE,
        outlineColor: Cesium.Color.BLACK,
        outlineWidth: 3,
        style: Cesium.LabelStyle.FILL_AND_OUTLINE,
        pixelOffset: new Cesium.Cartesian2(0, -16),
        verticalOrigin: Cesium.VerticalOrigin.BOTTOM,
        disableDepthTestDistance: Number.POSITIVE_INFINITY,
        heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
        // **Tuned for the zoom this is actually watched from.** A populate run is watched
        // at regional range - the whole inland sea on screen - so the labels have to be
        // readable there, not only from a thousand metres. Full size and opacity from
        // 100 km out to 8,000 km, fading only past that so a planet-wide view does not
        // become a wall of overlapping text.
        scaleByDistance: new Cesium.NearFarScalar(1.0e5, 1.0, 8.0e6, 0.85),
        translucencyByDistance: new Cesium.NearFarScalar(8.0e6, 1.0, 2.4e7, 0.0),
      },
    });
  };

  // Hover to read and click to fly down, the same as the worldfile pins - and through the
  // same handler, so the two cannot drift apart. `place` tells it where a live pin is,
  // since these carry their coordinates directly rather than an area anchor.
  const input = enableAreaInput(viewer, Cesium, window.document, source, (entity) => {
    const p = entity.position && entity.position.getValue(Cesium.JulianDate.now());
    if (!p) return null;
    const c = Cesium.Cartographic.fromCartesian(p);
    return { latitude_deg: Cesium.Math.toDegrees(c.latitude),
             longitude_deg: Cesium.Math.toDegrees(c.longitude) };
  });

  const poll = async () => {
    if (stopped) return;
    try {
      const r = await fetch(`/progress/?run=${encodeURIComponent(runId)}&from=${cursor}`,
                            { cache: "no-store" });
      if (r.ok) {
        const payload = await r.json();
        for (const area of payload.areas || []) {
          draw(area);
          if (counts) counts.add(area);
          cursor += 1;
        }
        if (onTick) onTick(cursor, payload.total || cursor);
      }
    } catch {
      // A run that has not written yet, or a server blip. Keep polling; the cursor means
      // nothing is missed and nothing is drawn twice.
    }
    if (!stopped) timer = setTimeout(poll, POLL_MS);
  };
  poll();

  return {
    source,
    count: () => cursor,
    stop: () => {
      stopped = true;
      if (timer) clearTimeout(timer);
    },
    tally: counts,
    remove: () => {
      stopped = true;
      if (timer) clearTimeout(timer);
      input.stop();
      if (counts) counts.remove();
      viewer.dataSources.remove(source, true);
    },
  };
}

/// The runs the server knows about, newest first.
export async function listRuns() {
  try {
    const r = await fetch("/runs/", { cache: "no-store" });
    return r.ok ? (await r.json()).runs || [] : [];
  } catch {
    return [];
  }
}
