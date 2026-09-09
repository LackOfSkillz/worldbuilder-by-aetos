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

import { drawAreas, enableAreaInput } from "./area-markers.js";
import { buildTally } from "./tally.js";
import { showLayer } from "./globe-layers.js";
import { fillFor, outlineFor, outlineWidthFor, legendRows } from "./palette.js";

/// How often to ask. Two seconds is slower than areas land and that is deliberate: the
/// point is to watch a world fill in, and a pin appearing every couple of seconds reads as
/// a world being built, where a hundred arriving at once reads as a page load.
const POLL_MS = 2000;

/// How long a run should take to appear, however long it took to compute.
///
/// **The generator is faster than the eye, and that is a problem worth solving in the
/// viewer.** A hundred areas are written in about five seconds, so a watcher that draws
/// whatever has arrived puts them up in two batches and the thing somebody wanted to watch
/// is over before they have looked at it. The world is not being faked - every pin is an
/// area that really landed, with the counts it really has. Only the reveal is paced.
///
/// Kept here rather than in the panel because it is a fact about watching, not about any
/// particular run.
const PACE_MS = 60000;

/// The fastest and slowest a pin may appear once the queue is draining.
///
/// The floor stops a very large run from turning into a flood at the end; the ceiling
/// stops a slow generator from leaving the globe apparently frozen between areas.
const PIN_MIN_MS = 120;
const PIN_MAX_MS = 1500;


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
                         { worldName = null, tally = true, paceMs = PACE_MS,
                           wanted = 0 } = {}) {
  const source = new Cesium.CustomDataSource(`wb-populate-${runId}`);
  const pins = showLayer(viewer, source);
  // The counts climb beside the pins. One card per run, removed with it.
  const counts = tally ? buildTally(window.document, worldName || "—", wanted) : null;
  let cursor = 0;
  let stopped = false;
  let timer = null;
  let drainTimer = null;
  //: What the run itself says about itself, once its manifest exists.
  let runStatus = null;
  let runSummary = null;
  //: Areas that have arrived from the feed and are waiting their turn to appear.
  const pending = [];
  const startedAt = performance.now();

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
  //: The rooms of the areas this run made, fetched once and kept.
  //
  // **The feed carries counts, not contents, and that is right** - a hundred areas of fifty
  // rooms is five thousand descriptions and this file is polled every two seconds. But it
  // leaves a live pin knowing how many rooms it has and not where any of them are, so
  // clicking one showed a place with no streets while the fish camp and the city showed
  // theirs. The run's own worldfile has them; it is read the first time somebody asks.
  let rooms = null;
  let roomsDrawn = null;
  let roadsSource = null;
  let roadLayer = null;

  /// Draw the roads and paths this run laid, as lines through their own rooms.
  ///
  /// **A network you cannot see is a claim, not a picture.** The generator joins every area
  /// to the nearest one already on the network and the manifest says nothing is stranded -
  /// but a globe of unconnected dots shows none of that, and the roads are the part that
  /// makes a hundred places read as a country rather than a scatter. They are drawn from
  /// their own rooms, so what appears is the road that exists, five miles a room.
  const drawRoads = (doc) => {
    const roads = doc.roads || [];
    const lines = doc.ferry_lines || [];
    if (!roads.length && !(doc.ferries || []).length && !lines.length) return;
    if (roadLayer) roadLayer.remove(true);
    roadsSource = new Cesium.CustomDataSource(`wb-roads-${runId}`);
    roadLayer = showLayer(viewer, roadsSource);
    // **Ferries are dotted, and only over water.** A boat crossing is not a road drawn in
    // another colour: the line follows the sea route the generator found, which is why it
    // goes round headlands instead of through them, and it is dashed because nobody walks
    // it. The taupe is shared with the roads so the network reads as one system.
    for (const crossing of doc.ferries || []) {
      const track = (crossing.track || []).flatMap((p) => [p[1], p[0]]);
      if (track.length < 4) continue;
      roadsSource.entities.add({
        name: `the crossing from ${crossing.from} to ${crossing.to}`,
        polyline: {
          positions: Cesium.Cartesian3.fromDegreesArray(track),
          width: 2.0,
          material: new Cesium.PolylineDashMaterialProperty({
            color: Cesium.Color.fromCssColorString("rgba(168,150,133,0.85)"),
            dashLength: 14,
          }),
          clampToGround: true,
        },
      });
    }
    // **The scheduled service, drawn heavier than an accidental crossing.** A ferry line
    // is a route with hulls on it and a timetable; an opportunistic crossing is a place a
    // road gave up. They are both dashed and both taupe because they are both water, and
    // the line is wider because it is the one a player plans a journey around. A line with
    // no track is not drawn: the generator drops those rather than guess a straight one
    // through a headland.
    for (const line of lines) {
      const track = (line.track || []).flatMap((p) => [p[1], p[0]]);
      if (track.length < 4) continue;
      const ends = line.ends || [];
      roadsSource.entities.add({
        name: `the ${Math.round(line.minutes || 0)}-minute ferry, ${ends[0]} to ${ends[1]}`,
        polyline: {
          positions: Cesium.Cartesian3.fromDegreesArray(track),
          width: 3.0,
          material: new Cesium.PolylineDashMaterialProperty({
            color: Cesium.Color.fromCssColorString("rgba(190,172,152,0.95)"),
            dashLength: 22,
          }),
          clampToGround: true,
        },
      });
    }
    for (const road of roads) {
      // **A road's rooms are not all on the road.** Its wayside shrines and hunters'
      // camps are interiors hanging off it, with no place in the line - and drawing the
      // list in order sent the road out to each one and back, which reads as a second road
      // beside the first and as a line across open water where the detour happened to
      // cross a strait.
      const points = (road.rooms || [])
        .filter((r) => r.longitude_deg !== undefined && !r.interior)
        .flatMap((r) => [r.longitude_deg, r.latitude_deg]);
      if (points.length < 4) continue;
      const path = road.purpose === "path";
      roadsSource.entities.add({
        name: road.display_name || road.name,
        polyline: {
          positions: Cesium.Cartesian3.fromDegreesArray(points),
          // A path to a hunting ground is thinner and dimmer than a road between towns,
          // which is the difference somebody is looking for at a glance.
          width: path ? 1.6 : 2.6,
          // Taupe: a road is dust and stone, not gold leaf. Light enough to read against
          // dark forest and dark enough not to compete with the coloured area pins, which
          // are the thing being connected and should stay the brightest marks on the globe.
          material: Cesium.Color.fromCssColorString(
            path ? "rgba(139,125,112,0.62)" : "rgba(168,150,133,0.92)"),
          clampToGround: true,
        },
      });
    }
  };
  const openRooms = async (name) => {
    if (rooms === null) {
      try {
        const doc = await (await fetch(`/runs/${encodeURIComponent(runId)}/worldfile.json`,
                                       { cache: "no-store" })).json();
        rooms = doc;
        drawRoads(doc);
      } catch {
        rooms = false;
      }
    }
    if (!rooms) return;
    const area = (rooms.areas || []).find((a) => a.name === name
                                          || a.display_name === name);
    if (!area) return;
    if (roomsDrawn) roomsDrawn.remove();
    // Drawn by the same function the worldfile pins use, so a generated area's streets
    // look and behave exactly like the fish camp's.
    roomsDrawn = drawAreas(viewer, Cesium, { areas: [area] });
  };

  const input = enableAreaInput(viewer, Cesium, window.document, source, (entity) => {
    const p = entity.position && entity.position.getValue(Cesium.JulianDate.now());
    if (!p) return null;
    if (entity.name) openRooms(entity.name);
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
        // Queued, not drawn. The cursor still advances on arrival so nothing is fetched
        // twice; what is paced is only when each one appears.
        for (const area of payload.areas || []) {
          pending.push(area);
          cursor += 1;
        }
        if (payload.status) runStatus = payload.status;
        if (payload.summary) runSummary = payload.summary;
        // The stage is told at once, not queued behind the pins: the whole point of it is
        // to speak during the stretches when no pin is landing.
        if (payload.stage && counts) {
          counts.stage(payload.stage.stage, payload.stage.note || "");
        }
      }
      // Once the generator has finished and everything it wrote has been queued, there is
      // nothing further to ask for. Polling on would be asking a finished run whether it
      // has changed its mind.
      if (runStatus === "complete" || runStatus === "failed") {
        if (timer) clearTimeout(timer);
        timer = null;
        return;
      }
    } catch {
      // A run that has not written yet, or a server blip. Keep polling; the cursor means
      // nothing is missed and nothing is drawn twice.
    }
    if (!stopped) timer = setTimeout(poll, POLL_MS);
  };

  /// Let one area through, then work out when the next should follow.
  ///
  /// The interval is recomputed every time rather than fixed, because the total is not
  /// known until the run ends: whatever is waiting is spread across whatever is left of
  /// the minute, so a run that finishes instantly still takes a minute to appear and a run
  /// that takes two minutes is never held back.
  const drain = () => {
    if (stopped) return;
    // **The reschedule is in a `finally`, and that is not defensive dressing.** Before the
    // pacing, `draw` ran inside the poll's own try/catch and a bad area cost that one pin;
    // now it drives a self-rescheduling chain, so the same throw ends the run's reveal
    // dead - measured here as a globe that stopped at twenty-eight of ninety-four with the
    // feed complete on disk and nothing in the console. One failed pin must not stop the
    // other sixty-six.
    try {
      if (pending.length) {
        const area = pending.shift();
        draw(area);
        if (counts) counts.add(area);
        if (onTick) onTick(drawn(), cursor, done());
      } else if (done() && !announced) {
        // The last pin has appeared and the run is over. Said once - and the roads are
        // drawn now, because the network is the point and nobody should have to click a
        // pin to discover it exists.
        announced = true;
        // The tally grows its sections now that there is a finished world to describe.
        if (counts) counts.finish(runSummary || {});
        fetch(`/runs/${encodeURIComponent(runId)}/worldfile.json`, { cache: "no-store" })
          .then((r) => r.json())
          .then((doc) => {
            rooms = doc;
            drawRoads(doc);
            // **Hand the world over so it can be saved.** The pins are drawn from the
            // progress feed and the rooms from this file, and neither of them is the world
            // panel's `lastAreas` - which is what "save world" writes. Until this line, a
            // run's areas were on screen, on disk under `runs/`, and absent from every
            // world file saved afterwards.
            window.dispatchEvent(new CustomEvent("wb-run-finished", {
              detail: { areas: doc.areas || [], roads: doc.roads || [] },
            }));
          })
          .catch(() => { /* the roads are a picture, not a promise */ });
        if (onTick) onTick(drawn(), cursor, done());
      }
    } catch (error) {
      console.error("worldbuilder: could not draw an area", error);
    } finally {
      const left = Math.max(0, paceMs - (performance.now() - startedAt));
      const each = pending.length ? left / pending.length : PIN_MAX_MS;
      const wait = Math.min(PIN_MAX_MS, Math.max(PIN_MIN_MS, each));
      drainTimer = setTimeout(drain, wait);
    }
  };

  const drawn = () => source.entities.values.length;

  /// Whether there is nothing more coming: the run is over and the queue is empty.
  const done = () => ((runStatus === "complete" || runStatus === "failed")
                      && pending.length === 0
                      ? { status: runStatus, summary: runSummary } : null);
  let announced = false;

  // **A hidden tab does not get timers, and that looks exactly like a hang.** Browsers
  // throttle `setTimeout` in a backgrounded page - to roughly once a second at first and
  // once a MINUTE after a few minutes hidden - so a run watched while somebody alt-tabs,
  // or while a recorder takes focus, crawls to a stop and then trickles. Measured here: a
  // reveal stalled at twenty-eight of ninety-four and resumed the instant the pane was
  // fronted.
  //
  // Nothing can make a hidden tab tick faster. What can be done is catch up the moment it
  // is looked at again, rather than waiting out whatever interval was scheduled while it
  // was away.
  const onVisible = () => {
    if (stopped || document.hidden) return;
    if (drainTimer) clearTimeout(drainTimer);
    drainTimer = setTimeout(drain, PIN_MIN_MS);
  };
  document.addEventListener("visibilitychange", onVisible);
  window.addEventListener("focus", onVisible);

  poll();
  drain();

  const halt = () => {
    stopped = true;
    if (timer) clearTimeout(timer);
    if (drainTimer) clearTimeout(drainTimer);
    document.removeEventListener("visibilitychange", onVisible);
    window.removeEventListener("focus", onVisible);
  };

  return {
    source,
    count: () => drawn(),
    /// Draw everything still queued at once, for somebody who does not want to wait.
    finish: () => {
      while (pending.length) {
        const area = pending.shift();
        draw(area);
        if (counts) counts.add(area);
      }
      if (onTick) onTick(drawn(), cursor);
    },
    stop: halt,
    tally: counts,
    status: () => ({ status: runStatus, summary: runSummary }),
    remove: () => {
      halt();
      if (roomsDrawn) roomsDrawn.remove();
      if (roadLayer) roadLayer.remove(true);
      input.stop();
      if (counts) counts.remove();
      pins.remove(true);
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
