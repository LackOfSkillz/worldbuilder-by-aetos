// Where the game's areas are, drawn on the globe and legible at every range.
//
// **"Visible from any zoom level" is not one requirement, it is three**, and each has a different
// Cesium answer. Getting one right and the other two wrong produces a marker that looks perfect
// in whatever view it was developed in and vanishes elsewhere - which is how a map layer passes a
// screenshot and fails a user.
//
//   1. IT MUST NOT SINK INTO THE TERRAIN. A billboard clamped to the ground is depth-tested
//      against it, so from a low angle the hill in front hides the town behind. Cesium's answer
//      is `disableDepthTestDistance: Infinity` - draw it in front of the globe, always.
//
//   2. IT MUST NOT BE CULLED BY RANGE. Anything with a `distanceDisplayCondition` disappears
//      outside it. So nothing here gets one. That is the whole of it, and it is easy to
//      reintroduce by accident when adding a label.
//
//   3. IT MUST NOT SWAMP THE VIEW UP CLOSE, OR SHRINK TO NOTHING FROM ORBIT. A fixed pixel size
//      does the first; a fixed world size does the second. `scaleByDistance` and
//      `translucencyByDistance` interpolate between the two ends, which is what keeps one symbol
//      readable across five orders of magnitude of camera range.
//
// **An area is a region, not a pin, so it gets both.** The pin says where the anchor is at any
// range; an outline of the area's own extent appears when you are close enough for it to be
// bigger than the pin. Drawing only the pin loses the size of a city; drawing only the footprint
// makes a twenty-room guild invisible from orbit.

/// Camera ranges, in metres, that the size and fade ramps run between.
///
/// The near end is a street; the far end is most of a planet. Both are named here rather than
/// buried in four literals, because they have to agree with each other or a marker fades out and
/// grows at the same time.
const NEAR_M = 1.0e3;
const FAR_M = 2.0e7;

/// Pin size at the near and far ends, as a multiplier on the symbol's own pixels.
const NEAR_SCALE = 1.0;
const FAR_SCALE = 0.45;

// **Level of detail, and the ranges are the whole feature.**
//
// The owner's requirement, in his words: *"zoomed all the way out to planet scale you only see
// a dot and a name"*, and close in *"you should be able to see area details, like the fishcamp,
// the dock, and the path"*. That is three layers with three different visibilities, and Cesium
// expresses it with `distanceDisplayCondition` - a near and far camera range outside which an
// entity is not drawn at all.
//
// The one thing that must carry NO condition is the area pin, because it is what you navigate
// by. Everything below it is detail that earns its way on screen as you approach:
//
//   pin + name        always            it is the dot at planet scale
//   footprint         under 400 km      the area has a size worth seeing
//   exits             under 60 km       the shape of the map
//   rooms             under 60 km       where you can stand
//   room names        under 8 km        legible without becoming a wall of text
//
// Ranges are camera DISTANCE TO THE ENTITY, not altitude, so they behave the same looking down
// as looking along.
const FOOTPRINT_MAX_M = 4.0e5;
const DETAIL_MAX_M = 6.0e4;
const ROOM_LABEL_MAX_M = 8.0e3;

/// An area with a harbour and one without, so the map answers the port question without a click.
const PORT_COLOUR = "#4db2ff";
const INLAND_COLOUR = "#ffc857";

function colourFor(Cesium, area) {
  const port = area.port || {};
  return Cesium.Color.fromCssColorString(port.has_port ? PORT_COLOUR : INLAND_COLOUR);
}

/// The bounding circle of an area's rooms, in metres, so the footprint matches what was placed.
///
/// Measured from the rooms rather than assumed from the room count: two areas of eighty rooms are
/// different sizes if one is a grid and the other is a road.
function radiusOf(Cesium, area) {
  const anchor = area.anchor;
  if (!anchor || !area.rooms || area.rooms.length === 0) return 200.0;
  const centre = Cesium.Cartesian3.fromDegrees(anchor.longitude_deg, anchor.latitude_deg);
  let furthest = 0;
  for (const room of area.rooms) {
    const point = Cesium.Cartesian3.fromDegrees(room.longitude_deg, room.latitude_deg);
    furthest = Math.max(furthest, Cesium.Cartesian3.distance(centre, point));
  }
  // A single-room area has a radius of zero, which draws nothing at all. One room spacing is the
  // smallest honest footprint.
  return Math.max(furthest, anchor.room_spacing_m || 60.0);
}

/// The suggested character level for an area, as text, or "" if it has none.
///
/// An area carries `level_band` when it was sited into one of the world's level rings. A
/// hand-placed area has none and gets no level line rather than a guessed one: a suggested
/// level nobody chose is worse than no suggestion, because a player will believe it.
function levelOf(area) {
  const band = area.level_band;
  if (!Array.isArray(band) || band.length !== 2) return "";
  return `lvl ${band[0]}-${band[1]}`;
}

/// What a pin says from orbit: the name, and who it is for.
///
/// **Two lines and no more.** This is read at planet scale with hundreds of pins on screen,
/// so every word competes with every other pin's words for the same pixels. Room counts and
/// port status moved to the hover card, where there is one at a time and room to be useful.
function label(area) {
  const level = levelOf(area);
  return level ? `${area.name}\n${level}` : area.name;
}

/// What the hover card says: everything the pin had to leave out.
function detail(area) {
  const rooms = (area.rooms || []).length;
  const port = area.port || {};
  const lines = [area.name];
  const level = levelOf(area);
  if (level) lines.push(`Suggested level ${level.slice(4)}`);
  if (area.culture) lines.push(area.culture);
  const who = [area.race, area.profession].filter(Boolean).join(" - ");
  if (who) lines.push(who);
  if (area.faction && area.faction !== "friendly") lines.push(area.faction.toUpperCase());
  lines.push(`${rooms} room${rooms === 1 ? "" : "s"}`);
  if (port.has_port) lines.push("harbour");
  else if (port.port_area) lines.push(`port: ${port.port_area}`);
  const anchor = area.anchor || {};
  if (anchor.latitude_deg !== undefined) {
    lines.push(`${anchor.latitude_deg.toFixed(4)}, ${anchor.longitude_deg.toFixed(4)}`);
  }
  lines.push("click to fly down");
  return lines.join("\n");
}

/// Draw every area in a worldfile.
///
/// Args:
///   viewer: the Cesium viewer, from `window.__wb.viewer`.
///   Cesium: the namespace, passed rather than imported so this file has no loader opinion.
///   document: a worldfile, already version-checked.
///
/// Returns a handle with `remove()`, and the entities, so a caller can redraw without leaking a
/// previous set onto the globe.
export function drawAreas(viewer, Cesium, document) {
  const source = new Cesium.CustomDataSource("wb-areas");
  const areas = document.areas || [];

  for (const area of areas) {
    const anchor = area.anchor;
    if (!anchor) continue;
    const colour = colourFor(Cesium, area);
    const radius = radiusOf(Cesium, area);
    const position = Cesium.Cartesian3.fromDegrees(
      anchor.longitude_deg, anchor.latitude_deg,
    );

    // The footprint, drawn only when the area is big enough on screen to be worth a shape.
    source.entities.add({
      name: area.name,
      position,
      ellipse: {
        semiMajorAxis: radius,
        semiMinorAxis: radius,
        material: colour.withAlpha(0.18),
        outline: true,
        outlineColor: colour.withAlpha(0.9),
        outlineWidth: 2,
        heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
        distanceDisplayCondition:
          new Cesium.DistanceDisplayCondition(0.0, FOOTPRINT_MAX_M),
      },
    });

    // --- the detail layers -------------------------------------------------------------
    //
    // Exits first, so rooms draw over their own lines rather than under them.
    const byId = new Map((area.rooms || []).map((room) => [room.id, room]));
    for (const exit of area.exits || []) {
      const from = byId.get(exit.source);
      const to = byId.get(exit.destination);
      if (!from || !to) continue;
      source.entities.add({
        name: `${from.key} ${exit.name} ${to.key}`,
        polyline: {
          positions: Cesium.Cartesian3.fromDegreesArray([
            from.longitude_deg, from.latitude_deg, to.longitude_deg, to.latitude_deg,
          ]),
          width: 2,
          material: colour.withAlpha(0.55),
          clampToGround: true,
          distanceDisplayCondition:
            new Cesium.DistanceDisplayCondition(0.0, DETAIL_MAX_M),
        },
      });
    }

    for (const room of area.rooms || []) {
      source.entities.add({
        name: room.key,
        position: Cesium.Cartesian3.fromDegrees(room.longitude_deg, room.latitude_deg),
        point: {
          pixelSize: 6,
          color: colour.brighten(0.4, new Cesium.Color()),
          outlineColor: Cesium.Color.BLACK.withAlpha(0.7),
          outlineWidth: 1,
          heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
          disableDepthTestDistance: Number.POSITIVE_INFINITY,
          distanceDisplayCondition:
            new Cesium.DistanceDisplayCondition(0.0, DETAIL_MAX_M),
        },
        label: {
          text: room.key,
          font: "11px system-ui, sans-serif",
          fillColor: Cesium.Color.WHITE,
          outlineColor: Cesium.Color.BLACK,
          outlineWidth: 3,
          style: Cesium.LabelStyle.FILL_AND_OUTLINE,
          pixelOffset: new Cesium.Cartesian2(0, -12),
          verticalOrigin: Cesium.VerticalOrigin.BOTTOM,
          heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
          disableDepthTestDistance: Number.POSITIVE_INFINITY,
          // Names come in last of all. Twenty-two of them at 60 km is a wall of text
          // over a map you cannot then read.
          distanceDisplayCondition:
            new Cesium.DistanceDisplayCondition(0.0, ROOM_LABEL_MAX_M),
        },
      });
    }

    // The pin. Never depth-tested, never range-culled, and scaled rather than fixed.
    source.entities.add({
      name: area.name,
      position,
      point: {
        pixelSize: 11,
        color: colour,
        outlineColor: Cesium.Color.BLACK.withAlpha(0.85),
        outlineWidth: 2,
        disableDepthTestAgainstTerrain: true,
        disableDepthTestDistance: Number.POSITIVE_INFINITY,
        heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
        scaleByDistance: new Cesium.NearFarScalar(NEAR_M, NEAR_SCALE, FAR_M, FAR_SCALE),
      },
      // Everything the hover card needs, carried on the entity. Cesium hands back the
      // picked entity and nothing else, so anything the card wants has to travel with it.
      description: detail(area),
      properties: { wbArea: area.name, wbAnchor: area.anchor },
      label: {
        text: label(area),
        font: "13px system-ui, sans-serif",
        fillColor: Cesium.Color.WHITE,
        outlineColor: Cesium.Color.BLACK,
        outlineWidth: 3,
        style: Cesium.LabelStyle.FILL_AND_OUTLINE,
        pixelOffset: new Cesium.Cartesian2(0, -20),
        verticalOrigin: Cesium.VerticalOrigin.BOTTOM,
        disableDepthTestDistance: Number.POSITIVE_INFINITY,
        heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
        scaleByDistance: new Cesium.NearFarScalar(NEAR_M, NEAR_SCALE, FAR_M, FAR_SCALE),
        // Labels fade rather than vanish, so a crowded region at orbital range stays readable
        // as a cluster of pins instead of a wall of overlapping text.
        translucencyByDistance: new Cesium.NearFarScalar(NEAR_M, 1.0, FAR_M, 0.75),
      },
    });
  }

  viewer.dataSources.add(source);
  const input = enableAreaInput(viewer, Cesium, document, source);
  return {
    source,
    count: areas.length,
    /// Fly to everything at once - the "where is my world" button.
    flyToAll: () => viewer.flyTo(source, { duration: 1.5 }),
    remove: () => {
      input.stop();
      viewer.dataSources.remove(source, true);
    },
  };
}


/// How close the camera comes when you click a pin: near enough to see the rooms, far
/// enough that the whole area is on screen.
const AREA_VIEW_M = 2500.0;

//: How far above the ground the camera must end up, whatever the pitch and the range.
const CLEARANCE_M = 700.0;


/// Hover to read, click to fly down.
///
/// **Both handlers pick an ENTITY first and do nothing when the pick misses.** The globe
/// already has a left-click handler for dropping coordinate pins, and a second one that
/// acted on every click would drop a pin every time somebody tried to visit an area. So
/// this one is silent unless the cursor is actually on a pin, and the two coexist without
/// either knowing about the other.
///
/// Installed on LEFT_CLICK rather than mouse-down, for the reason `pick-point` records: a
/// drag to rotate the globe must not count as a click.
export function enableAreaInput(viewer, Cesium, document, source, place = null) {
  const handler = new Cesium.ScreenSpaceEventHandler(viewer.scene.canvas);
  const card = makeCard();

  const areaAt = (windowPosition) => {
    const picked = viewer.scene.pick(windowPosition);
    if (!picked || !picked.id || !picked.id.properties) return null;
    const owner = picked.id;
    if (!source.entities.contains(owner)) return null;
    return owner;
  };

  handler.setInputAction((movement) => {
    const entity = areaAt(movement.endPosition);
    if (!entity) {
      card.style.display = "none";
      viewer.scene.canvas.style.cursor = "";
      return;
    }
    card.textContent = entity.description ? entity.description.getValue() : entity.name;
    card.style.display = "block";
    card.style.left = `${movement.endPosition.x + 16}px`;
    card.style.top = `${movement.endPosition.y + 16}px`;
    viewer.scene.canvas.style.cursor = "pointer";
  }, Cesium.ScreenSpaceEventType.MOUSE_MOVE);

  handler.setInputAction((movement) => {
    const entity = areaAt(movement.position);
    if (!entity) return;
    card.style.display = "none";
    // **A computed camera move, not `viewer.flyTo(entity)`.** The entity form is the
    // obvious one and it hangs: it waits for the entity's data source and for terrain
    // under a CLAMP_TO_GROUND pin to be ready, and against an offline terrain provider
    // that promise never settles. The camera never moved and nothing was thrown - the
    // click simply did nothing, which reads as a dead handler rather than as a pending
    // promise. Flying to a coordinate asks nothing of the terrain and cannot wait.
    const where = place ? place(entity) : (
      entity.properties && entity.properties.wbAnchor
        ? entity.properties.wbAnchor.getValue() : null);
    if (!where) return;
    flyToPlace(viewer, Cesium, where.latitude_deg, where.longitude_deg);
  }, Cesium.ScreenSpaceEventType.LEFT_CLICK);

  return {
    stop: () => {
      handler.destroy();
      card.remove();
      viewer.scene.canvas.style.cursor = "";
    },
  };
}


//: The pending arrival snap, so a second click cancels the first one's.
let arrivalTimer = null;

/// How high the ground stands at a place, and around it, in metres above datum.
///
/// **The camera has to clear the terrain, not the datum.** `fromDegrees` with no height
/// puts a target at sea level, so a fly-to at two and a half kilometres range and fifty
/// degrees of pitch stands the camera about nineteen hundred metres above SEA LEVEL - which
/// is fine over a bayou and is two kilometres inside the rock once somebody paints a
/// four-thousand-metre range. Clicking an area then flew the camera into the mountain.
///
/// The neighbourhood is sampled as well as the point, because the peak that swallows the
/// camera is rarely the one directly under the pin.
function groundAround(latitudeDeg, longitudeDeg, reachM = 4000.0, rays = 8) {
  const wb = window.__wb;
  if (!wb || !wb.engine || typeof wb.engine.elevationM !== "function") return 0;
  const radiusM = (wb.spec && wb.spec.radiusM) || 6371000;
  const degrees = (180 / Math.PI) * (reachM / radiusM);
  let highest = 0;
  const look = (lat, lon) => {
    try {
      const height = wb.engine.elevationM(wb.world, lat, lon);
      if (Number.isFinite(height) && height > highest) highest = height;
    } catch {
      // A world mid-swap answers nothing; sea level is the safe floor.
    }
  };
  look(latitudeDeg, longitudeDeg);
  const here = highest;
  for (let i = 0; i < rays; i += 1) {
    const bearing = (2 * Math.PI * i) / rays;
    look(latitudeDeg + degrees * Math.cos(bearing),
         longitudeDeg + degrees * Math.sin(bearing) / Math.max(0.2, Math.cos(
           (latitudeDeg * Math.PI) / 180)));
  }
  return { here, highest };
}

/// Put a place in the MIDDLE of the screen and look at it.
///
/// **A pitched camera does not look at what is beneath it, and that was the bug.** Flying
/// to `fromDegrees(lon, lat, height)` with a pitch of -55 puts the CAMERA over the target
/// and then tilts it, so the target slides out of frame and you arrive looking at the
/// country beyond it - which is exactly "the zoom doesn't centre, I have to scroll to find
/// the area". `flyToBoundingSphere` solves the other problem: it positions the camera
/// around a target at a given heading, pitch and range, so the thing stays in the middle
/// however the view is tilted.
///
/// Not `viewer.flyTo(entity)`, which is the obvious call and hangs: it waits on the data
/// source and on terrain under a clamped pin, and against an offline terrain provider that
/// promise never settles. A bounding sphere built from a coordinate asks nothing of either.
export function flyToPlace(viewer, Cesium, latitudeDeg, longitudeDeg,
                           rangeM = AREA_VIEW_M, pitchDeg = -50.0, durationS = 1.8) {
  const camera = viewer.camera;
  // **Land the previous flight before starting another.** `lookAt` computes a pose from the
  // camera's current state, and a camera still in flight - or still locked to the last
  // target's reference frame - gives a pose that is neither where it was nor where it is
  // going. Measured: the first click flew correctly and every click after it left the
  // camera exactly where it stood, at a height belonging to neither place. Two lines, and
  // they have to be the first two.
  if (typeof camera.cancelFlight === "function") camera.cancelFlight();
  camera.lookAtTransform(Cesium.Matrix4.IDENTITY);
  if (arrivalTimer) clearTimeout(arrivalTimer);

  // Anchored to the ground, not to the datum, and given room to clear the highest thing
  // nearby. See `groundAround`.
  const ground = groundAround(latitudeDeg, longitudeDeg);
  const base = Math.max(0, ground.here);
  const centre = Cesium.Cartesian3.fromDegrees(longitudeDeg, latitudeDeg, base);
  // The camera ends up `base + range * sin(pitch)` above the datum, and that has to clear
  // the HIGHEST ground nearby, not the ground under the pin - the peak that swallows a
  // camera is rarely the one directly beneath it. Solved for range rather than nudged.
  const lift = Math.abs(Math.sin(Cesium.Math.toRadians(pitchDeg))) || 0.5;
  const wanted = (Math.max(0, ground.highest) + CLEARANCE_M - base) / lift;
  const range = Math.max(rangeM, wanted);
  const hpr = new Cesium.HeadingPitchRange(0.0, Cesium.Math.toRadians(pitchDeg), range);

  // **`lookAt` is used to COMPUTE the pose, not to move the camera.** It is the only call
  // that reliably works out where a camera must stand to hold a target in the middle of
  // the screen at a given pitch - flying to `fromDegrees(lon, lat, height)` and then
  // tilting puts the camera *over* the target and looks past it, which is the whole "the
  // zoom does not centre, I have to scroll to find the area" complaint. Measured: the
  // computed pose lands the target 0 px from centre; the pitched fly-to left it 392 px off.
  //
  // `flyToBoundingSphere` is the documented way to do this in one call and did nothing
  // here - called without throwing, camera never moved - so the pose is taken from
  // `lookAt` and flown to explicitly, which does work.
  camera.lookAt(centre, hpr);
  // **`positionWC`, not `position`.** Under `lookAt` the camera is locked to a reference
  // frame around the target and `camera.position` is expressed IN that frame; cloning it
  // and then releasing the transform hands `flyTo` a local offset to be read as a world
  // coordinate. Measured: the pose itself was right - 5,879 m over a 3,964 m peak - and the
  // camera flew from it back to where it had started, every time, for every target. The
  // world-coordinate reading is the same point without the frame.
  const destination = Cesium.Cartesian3.clone(camera.positionWC);
  const orientation = { heading: camera.heading, pitch: camera.pitch, roll: camera.roll };
  // **Release the transform before flying.** `lookAt` locks the camera to a reference
  // frame around the target; left set, every later movement is interpreted in that frame
  // and the globe stops dragging normally.
  camera.lookAtTransform(Cesium.Matrix4.IDENTITY);

  // **The flight is the animation; the arrival is set.** `flyTo` follows an arc and does
  // not always finish on the pose it was given - measured, the same target reached from two
  // different starting points landed at 1,917 m from one and 2,502 m from the other, with
  // the computed pose identical to the metre in both cases and the second sitting 226 px
  // off centre. Snapping at the end costs nothing visually, because it is the frame the
  // flight was already trying to reach.
  //
  // `complete` does not fire on a cancelled flight, so a second click interrupting a first
  // does not fight it.
  const arrive = () => camera.setView({ destination, orientation });
  camera.flyTo({ destination, orientation, duration: durationS, complete: arrive });
  // **A timer as well as `complete`, because `complete` does not always run.** Measured:
  // arriving at the coast from a four-thousand-metre peak left the camera at the PEAK's
  // altitude, moved horizontally and 572 px off centre, with `complete` never firing - the
  // flight had ended by another route. The timer fires just past the flight's own duration
  // and sets the pose the flight was aiming at; it is cleared by the next click, so two
  // clicks in quick succession do not fight.
  arrivalTimer = setTimeout(arrive, durationS * 1000 + 150);
}

/// The hover card. One per draw, reused, hidden when nothing is under the cursor.
function makeCard() {
  const existing = window.document.getElementById("wb-area-card");
  if (existing) existing.remove();
  const card = window.document.createElement("div");
  card.id = "wb-area-card";
  card.style.cssText = [
    "position:absolute", "z-index:20", "display:none", "pointer-events:none",
    "white-space:pre", "padding:8px 10px", "border-radius:6px",
    "background:rgba(12,16,22,0.92)", "color:#e8eef6",
    "border:1px solid rgba(255,255,255,0.18)",
    "font:12px/1.45 system-ui, sans-serif",
    "box-shadow:0 6px 20px rgba(0,0,0,0.45)",
  ].join(";");
  window.document.body.appendChild(card);
  return card;
}
