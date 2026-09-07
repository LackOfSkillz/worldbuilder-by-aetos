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

function label(area) {
  const rooms = (area.rooms || []).length;
  const port = area.port || {};
  const where = port.has_port
    ? "harbour"
    : port.port_area
      ? `port: ${port.port_area}`
      : "no port";
  return `${area.name}\n${rooms} rooms · ${where}`;
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

    // The footprint. `heightReference: CLAMP_TO_GROUND` puts it on the terrain rather than
    // through it, and it carries no distance condition of its own - at orbital range it is
    // simply smaller than a pixel and costs nothing.
    source.entities.add({
      name: area.name,
      position,
      ellipse: {
        semiMajorAxis: radius,
        semiMinorAxis: radius,
        material: colour.withAlpha(0.22),
        outline: true,
        outlineColor: colour.withAlpha(0.9),
        outlineWidth: 2,
        heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
      },
    });

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
  return {
    source,
    count: areas.length,
    /// Fly to everything at once - the "where is my world" button.
    flyToAll: () => viewer.flyTo(source, { duration: 1.5 }),
    remove: () => viewer.dataSources.remove(source, true),
  };
}
