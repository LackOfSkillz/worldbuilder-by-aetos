//! Fresh water on the globe: rivers, streams and standing water.
//
// The features carve the ground and the ground is what the terrain shows, but a channel
// twenty metres wide is invisible from orbit and nearly invisible from a thousand metres.
// A river you cannot see is a river nobody knows is there, so it is drawn as a line as
// well as cut as a channel.
//
// **The line comes from `hydrology.courses`, not from the carve features.** The features
// are decimated to one per sixteen kilometres because `Features.apply` is a linear scan
// and a world cannot afford seven thousand of them; the courses keep every node the
// descent walked. Reconstructing a line from overlapping carve segments would mean undoing
// the same arithmetic that made the first river shoal at every node, and would draw a
// coarser line than the one already recorded.
//
// The three rules the area pins follow apply here too, for the same reasons: never
// depth-tested against terrain, never range-culled, width scaled rather than fixed. A
// river that sinks into the ground it cut is worse than no river.

/// Width in pixels at the near and far ends of the scale, per kind.
const STYLE = {
  river: { width: 2.6, alpha: 0.95 },
  stream: { width: 1.5, alpha: 0.8 },
};

/// Fresh water is blue-green and deliberately not the sea's blue: at a river mouth the two
/// meet, and a river drawn in the sea's own colour disappears exactly where it matters.
function waterColour(Cesium, kind) {
  return kind === "stream"
    ? Cesium.Color.fromCssColorString("#5fd0d8").withAlpha(0.8)
    : Cesium.Color.fromCssColorString("#3aa7e0").withAlpha(0.95);
}

/// Draw every watercourse and body of standing water in a worldfile.
///
/// Args:
///   viewer: the Cesium viewer.
///   Cesium: the namespace, passed rather than imported so this file has no loader opinion.
///   document: a worldfile carrying a `hydrology` block.
///
/// Returns a handle with `remove()` and the counts drawn.
export function drawWater(viewer, Cesium, document) {
  const source = new Cesium.CustomDataSource("wb-water");
  const hydrology = document.hydrology || {};
  const courses = hydrology.courses || [];
  const bodies = hydrology.bodies || [];

  for (const course of courses) {
    const points = course.points || [];
    if (points.length < 2) continue;
    const positions = [];
    for (const [lat, lon] of points) {
      positions.push(lon, lat);
    }
    const style = STYLE[course.kind] || STYLE.river;
    source.entities.add({
      name: `${course.kind} · ${Math.round(course.length_m / 1000)} km`,
      description: [
        course.kind,
        `${Math.round(course.length_m / 1000)} km`,
        course.reached_sea ? "reaches the sea" : "ends in a lake",
      ].join("\n"),
      polyline: {
        positions: Cesium.Cartesian3.fromDegreesArray(positions),
        width: style.width,
        material: waterColour(Cesium, course.kind),
        // Clamped so the line follows the valley it cut instead of cutting the hills it
        // passes; the depth test is off so it is never swallowed by its own banks.
        clampToGround: true,
        classificationType: Cesium.ClassificationType.TERRAIN,
      },
    });
  }

  for (const body of bodies) {
    const radius = body.radius_m || body.length_m || 1000.0;
    source.entities.add({
      name: body.kind,
      description: `${body.kind}\n${Math.round(radius)} m across\nsurface ${Math.round(body.surface_m || 0)} m`,
      position: Cesium.Cartesian3.fromDegrees(body.longitude_deg, body.latitude_deg),
      ellipse: {
        semiMajorAxis: radius,
        semiMinorAxis: radius,
        material: Cesium.Color.fromCssColorString("#2b7fc4").withAlpha(0.75),
        outline: true,
        outlineColor: Cesium.Color.fromCssColorString("#9fe0ff").withAlpha(0.9),
        classificationType: Cesium.ClassificationType.TERRAIN,
        heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
      },
    });
  }

  viewer.dataSources.add(source);
  return {
    source,
    courses: courses.length,
    bodies: bodies.length,
    nodes: courses.reduce((sum, c) => sum + (c.points || []).length, 0),
    remove: () => viewer.dataSources.remove(source, true),
  };
}
