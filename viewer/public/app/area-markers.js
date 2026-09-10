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
import { showLayer } from "./globe-layers.js";

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
/// The colour of a room worth stopping at: a shop, or anywhere with a keeper.
///
/// **Names are not drawn on the map any more.** Twenty-two of them over a village is a wall
/// of text laid across the thing they label, and at a hundred rooms a town is unreadable.
/// The name is what a hover is for and the description is what a click is for, so the map
/// stays a map.
const POI_COLOUR = (Cesium) => Cesium.Color.fromCssColorString("#ffcc66");

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
/// What a hover says about an area.
///
/// **The same facts a live run's pins carry.** These two layers draw the same places from
/// different sources - the progress feed while a run is going, the worldfile once it is
/// saved - and they said different things about them: a pin hovered during a run listed its
/// shops and its people, and the same pin after a reload listed neither. A player cannot be
/// expected to know which of the two they happen to be looking at.
///
/// A field the generator did not fill is left out rather than shown as zero, because a
/// missing count and a genuine none are different facts.
function detail(area) {
  const port = area.port || {};
  const lines = [area.display_name || area.name];
  if (area.culture) lines.push(area.culture);
  const who = [area.race, area.profession].filter(Boolean).join(" - ");
  if (who) lines.push(who);
  // What the place is FOR, said plainly. "hunting" is not a faction and a hunting ground
  // full of deer is not hostile, so the two are separate lines and both are wanted.
  if (area.purpose && area.purpose !== "home") lines.push(area.purpose);
  if (area.faction && area.faction !== "friendly") lines.push(area.faction.toUpperCase());

  lines.push("");
  const row = (key, value) => lines.push(key.padEnd(15) + value);
  // `rooms` is a list in a worldfile and a count in a feed, and `room_count` is written by
  // the generator either way - so the count is right whichever layer drew this pin.
  const rooms = Array.isArray(area.rooms) ? area.rooms.length : area.rooms;
  const roomCount = area.room_count !== undefined ? area.room_count : rooms;
  if (roomCount !== undefined) row("rooms", roomCount);
  if (area.shops !== undefined) row("shops", area.shops);
  if (area.items !== undefined) row("goods on sale", area.items);
  if (area.npcs !== undefined) row("inhabitants", area.npcs);
  if (area.docks) row("docks", area.docks);
  const level = levelOf(area);
  if (level) row("levels", level.slice(4));
  if (port.has_port) row("harbour", "yes");
  else if (port.port_area) row("port", port.port_area);

  lines.push("");
  lines.push("click to fly down");
  return lines.join(String.fromCharCode(10));
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

    // **What is through the door is what the street is for.** A shop is an interior now, off
    // the lattice and not a place on the map - so the frontage it opens off carries it,
    // exactly as the game's own land map does it. Without this the marker went where the
    // shop is and the shop is nowhere, and a town drew as streets with nothing on them.
    const insideOf = new Map((area.rooms || []).filter((one) => one.interior)
      .map((one) => [one.from, one]));

    for (const room of area.rooms || []) {
      // An interior is drawn by its street, never as a dot of its own: it sits at the same
      // coordinates and would be a second marker on top of the first.
      if (room.interior) continue;
      const shop = insideOf.get(room.id);
      const keeper = ((shop || room).people || []).find((who) => who.role === "keeper");
      const goods = (shop || room).stock || [];
      const trade = !!(keeper || goods.length);
      source.entities.add({
        name: room.key,
        // **The hover text, and the room's whole story, carried on the entity.** Cesium
        // hands the picked entity back and nothing else, so anything a card wants to say
        // has to be here at draw time.
        description: room.key,
        properties: {
          wbRoom: {
            key: room.key,
            desc: room.desc || "",
            // The curator's text, carried beside the generator's so the card can show
            // either. Absent until a run has been curated, and then the card offers both.
            key_ai: room.key_ai || null,
            desc_ai: room.desc_ai || null,
            area: area.display_name || area.name,
            // What is behind the door, named so a click on the street says what is there
            // and what it sells - which is the question the marker raises.
            shop: shop ? shop.key : null,
            shop_ai: shop ? (shop.key_ai || null) : null,
            // Things to look at (laws F1, F2), by name - what `look` will find here.
            things: (room.fixtures || []).map((thing) => thing.key),
            door: shop ? shop.noun : null,
            keeper: keeper ? keeper.name : null,
            people: (room.people || []).map((who) => who.name),
            stock: goods,
            // The curator's reworked goods, `{name, desc}` item for item with `stock`.
            stock_ai: (shop || room).stock_ai || null,
            latitude_deg: room.latitude_deg,
            longitude_deg: room.longitude_deg,
          },
        },
        position: Cesium.Cartesian3.fromDegrees(room.longitude_deg, room.latitude_deg),
        point: {
          // A point of interest is bigger and warmer than the street it stands on. Sized
          // rather than shaped, because a shape at six pixels is a smudge.
          pixelSize: trade ? 9 : 6,
          color: trade ? POI_COLOUR(Cesium) : colour.brighten(0.4, new Cesium.Color()),
          outlineColor: Cesium.Color.BLACK.withAlpha(0.7),
          outlineWidth: trade ? 2 : 1,
          heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
          disableDepthTestDistance: Number.POSITIVE_INFINITY,
          distanceDisplayCondition:
            new Cesium.DistanceDisplayCondition(0.0, DETAIL_MAX_M),
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

  const layer = showLayer(viewer, source);
  const input = enableAreaInput(viewer, Cesium, document, source);
  return {
    source,
    count: areas.length,
    /// Fly to everything at once - the "where is my world" button.
    flyToAll: () => viewer.flyTo(source, { duration: 1.5 }),
    remove: () => {
      input.stop();
      return layer.remove(true);
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
//: How many input handlers have ever been made, so each can be told from the others.
let claims = 0;


export function enableAreaInput(viewer, Cesium, document, source, place = null) {
  const handler = new Cesium.ScreenSpaceEventHandler(viewer.scene.canvas);
  const card = makeCard();
  const roomPanel = makeRoomPanel();

  const areaAt = (windowPosition) => {
    const picked = viewer.scene.pick(windowPosition);
    if (!picked || !picked.id) return null;
    const owner = picked.id;
    // **Belonging to this source is the test, and `properties` never was.** Pins drawn
    // from a worldfile carry a `wbAnchor` property and pins drawn from a populate feed
    // carry their position directly, so demanding `properties` silently ignored every
    // live pin: no hover card, no click, on exactly the areas somebody had just watched
    // land. The source membership check below is the one that means anything.
    if (!source.entities.contains(owner)) return null;
    return owner;
  };

  //: Which source last put something in the shared card.
  //
  // **One card, several handlers, and a miss must not erase a hit.** The worldfile's pins
  // and a live run's pins each run their own handler over the same mouse move, so the one
  // that finds nothing hides the card the other has just filled - and which of them goes
  // last is a matter of registration order. A handler now only clears the card if the card
  // is showing ITS pin.
  // **Unique per handler, not per source name.** Every area layer is called "wb-areas", so
  // naming the owner after the source made two handlers claim one identity - and the one
  // that missed then hid the card the other had just filled, which is the very fault the
  // owner check exists to prevent.
  claims += 1;
  const mine = `${source.name || "layer"}#${claims}`;

  handler.setInputAction((movement) => {
    const entity = areaAt(movement.endPosition);
    if (!entity) {
      if (card.dataset.owner === mine) {
        card.style.display = "none";
        card.dataset.owner = "";
      }
      viewer.scene.canvas.style.cursor = "";
      return;
    }
    card.dataset.owner = mine;
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
    // **A room is read, not flown to.** Clicking a street to be taken two thousand metres
    // above it is the opposite of what a click on a room means: the camera is already
    // where it needs to be, and what is wanted is what the room says.
    const held = entity.properties && entity.properties.wbRoom;
    if (held) {
      showRoom(roomPanel, held.getValue(), movement.position.x, movement.position.y);
      return;
    }
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
      // **The card is shared and must survive this.** `makeCard` hands every input handler
      // the same element on purpose, and removing it here took it out of the document for
      // all of them - so the next layer torn down anywhere killed hover everywhere, while
      // click went on working because click never touches the card. That is what a night
      // away from a working map and a dead hover in the morning looks like: something was
      // redrawn in between, and clicking an area to see its rooms is enough to do it.
      if (card.dataset.owner === mine) {
        card.style.display = "none";
        card.dataset.owner = "";
      }
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
/// The room card: what a click on a room opens, and what its close button shuts.
///
/// **Separate from the hover card, because they answer different questions.** The hover
/// says which room the cursor is over and must vanish the instant it leaves; this one is
/// read, so it has to stay until it is dismissed. One element, shared by every source, for
/// the reason `makeCard` records.
export function makeRoomPanel() {
  const existing = window.document.getElementById("wb-room-card");
  if (existing) return existing;
  const panel = window.document.createElement("div");
  panel.id = "wb-room-card";
  panel.style.cssText = [
    "position:absolute", "z-index:24", "display:none", "max-width:340px",
    "padding:12px 14px 14px", "border-radius:8px",
    "background:rgba(12,16,22,0.96)", "color:#e8eef6",
    "border:1px solid rgba(255,255,255,0.22)",
    "font:13px/1.55 system-ui, sans-serif",
    "box-shadow:0 10px 30px rgba(0,0,0,0.55)",
  ].join(";");
  window.document.body.appendChild(panel);
  return panel;
}


//: Which text the room card shows when a room has both: "ai" or "template".
//
// Remembered per browser, because the natural way to judge the curator is to walk a town
// reading one side and then walk it again reading the other - and a toggle that forgot its
// setting on every click would make that a click per room.
const CARD_TEXT_KEY = "wb.cardText";

function cardText() {
  try {
    return window.localStorage.getItem(CARD_TEXT_KEY) === "template" ? "template" : "ai";
  } catch {
    return "ai";
  }
}

function setCardText(mode) {
  try {
    window.localStorage.setItem(CARD_TEXT_KEY, mode);
  } catch { /* a private window simply forgets */ }
}

/// What the card says for a room, in the chosen text. Exported for the tests.
export function roomText(room, mode) {
  const shelf = Array.isArray(room.stock_ai) && room.stock_ai.length ? room.stock_ai : null;
  const ai = mode === "ai" && Boolean(room.desc_ai || shelf);
  return {
    ai,
    key: ai && room.key_ai ? room.key_ai : room.key,
    desc: ai && room.desc_ai ? room.desc_ai : room.desc,
    shop: ai && room.shop_ai ? room.shop_ai : room.shop,
    // Each ware as `{name, desc}`; the template's have no description to show.
    wares: ai && shelf
      ? shelf.map((ware) => ({ name: ware.name, desc: ware.desc || "" }))
      : (room.stock || []).map((name) => ({ name, desc: "" })),
  };
}

/// Fill the room card and show it beside the click.
export function showRoom(panel, room, x, y) {
  const make = (tag, css, text) => {
    const node = window.document.createElement(tag);
    if (css) node.style.cssText = css;
    if (text !== undefined) node.textContent = text;
    return node;
  };
  panel.textContent = "";

  const close = make("button", [
    "position:absolute", "top:6px", "right:8px", "border:0", "background:none",
    "color:#9fb0c4", "font:16px/1 system-ui, sans-serif", "cursor:pointer",
    "padding:2px 4px",
  ].join(";"), "×");
  close.type = "button";
  close.title = "close";
  close.addEventListener("click", () => { panel.style.display = "none"; });
  panel.append(close);

  const shown = roomText(room, cardText());
  panel.append(make("div", "font-weight:600;padding-right:16px", shown.key));
  if (room.area) {
    panel.append(make("div", "color:#8fa3ba;font-size:11px;margin-bottom:6px", room.area));
  }
  // **AI | template, only where there is a choice.** A room the curator never touched has
  // one text, and a toggle with nothing behind one side would look broken.
  if (room.desc_ai || (room.stock_ai && room.stock_ai.length)) {
    const toggle = make("div", "display:flex;gap:4px;margin-bottom:6px");
    for (const [mode, label] of [["ai", "AI"], ["template", "template"]]) {
      const active = (mode === "ai") === shown.ai;
      const button = make("button", [
        "border:1px solid rgba(255,255,255,0.25)", "border-radius:4px", "cursor:pointer",
        "font:11px/1.4 system-ui, sans-serif", "padding:1px 8px",
        active ? "background:#b89a6a;color:#10141a" : "background:none;color:#9fb0c4",
      ].join(";"), label);
      button.type = "button";
      button.dataset.cardText = mode;
      button.addEventListener("click", () => {
        setCardText(mode);
        showRoom(panel, room, x, y);
      });
      toggle.append(button);
    }
    panel.append(toggle);
  }
  if (shown.desc) {
    panel.append(make("div", "margin-bottom:6px", shown.desc));
  }
  if (room.things && room.things.length) {
    panel.append(make("div", "color:#b89a6a;font-size:12px",
                      `to look at: ${room.things.join(", ")}`));
  }
  if (shown.shop) {
    panel.append(make("div", "color:#ffcc66;font-size:12px;margin-top:4px",
                      `${shown.shop} - go ${room.door}`));
  }
  if (room.keeper) {
    panel.append(make("div", "color:#cfe0f2;font-size:12px", room.keeper));
  }
  if (shown.wares.length) {
    const list = make("ul", "margin:4px 0 0;padding-left:18px;color:#cfe0f2;font-size:12px");
    for (const ware of shown.wares) {
      const item = make("li", null, ware.name);
      // What the ware looks like, on hover: the list stays a list a player can scan.
      if (ware.desc) item.title = ware.desc;
      list.append(item);
    }
    panel.append(list);
  } else if (room.people && room.people.length && !room.keeper) {
    panel.append(make("div", "color:#8fa3ba;font-size:12px", room.people.join(", ")));
  }

  panel.style.display = "block";
  panel.style.left = `${Math.min(x + 16, window.innerWidth - 360)}px`;
  panel.style.top = `${Math.min(y + 16, window.innerHeight - 220)}px`;
}


function makeCard() {
  // **Reused, not replaced.** There is more than one set of pins on the globe - the
  // worldfile's areas and a live populate run each get their own input handler - and each
  // one built a card with the same id, removing the other's. The handler that lost the
  // race then wrote every hover into a node detached from the document, so hovering the
  // areas somebody had just watched land showed nothing at all while the code ran
  // perfectly. One card, shared.
  const existing = window.document.getElementById("wb-area-card");
  if (existing) return existing;
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
