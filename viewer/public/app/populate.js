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

/// How often to ask. Two seconds is slower than areas land and that is deliberate: the
/// point is to watch a world fill in, and a pin appearing every couple of seconds reads as
/// a world being built, where a hundred arriving at once reads as a page load.
const POLL_MS = 2000;

/// Colour by what kind of place it is, so the map tells a story as it fills.
function markColour(Cesium, area) {
  const faction = (area.faction || "friendly").toLowerCase();
  if (faction === "hostile") return Cesium.Color.fromCssColorString("#e2564a");
  if (faction === "neutral") return Cesium.Color.fromCssColorString("#e0a94a");
  return Cesium.Color.fromCssColorString("#5fd08a");
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
export function watchRun(viewer, Cesium, runId, onTick = null) {
  const source = new Cesium.CustomDataSource(`wb-populate-${runId}`);
  viewer.dataSources.add(source);
  let cursor = 0;
  let stopped = false;
  let timer = null;

  const draw = (area) => {
    if (area.latitude_deg === undefined || area.longitude_deg === undefined) return;
    source.entities.add({
      name: area.name,
      description: [
        area.display_name || area.name,
        area.culture || "",
        area.race ? `race: ${area.race}` : "",
        Array.isArray(area.level_band) ? `levels ${area.level_band.join("-")}` : "",
        area.rooms ? `${area.rooms} rooms` : "",
      ].filter(Boolean).join("\n"),
      position: Cesium.Cartesian3.fromDegrees(area.longitude_deg, area.latitude_deg),
      point: {
        pixelSize: 10,
        color: markColour(Cesium, area),
        outlineColor: Cesium.Color.BLACK.withAlpha(0.8),
        outlineWidth: 2,
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
    remove: () => {
      stopped = true;
      if (timer) clearTimeout(timer);
      input.stop();
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
