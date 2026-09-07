// Click the globe, get the place.
//
// **The readout already shows the position under the cursor, and that is not the same thing.**
// A hover readout tells you where you are pointing right now and forgets it the moment you move,
// which is exactly wrong for the job somebody actually has: zoom into a feature, put the cursor
// on it, and keep the number. So this pins it, keeps it, and hands it over in the forms it is
// wanted in - a `?fly=` fragment to get back, and a latitude/longitude pair to paste.
//
// **Two ways to turn a click into a place, and only one of them respects the terrain.**
//
//   - `camera.pickEllipsoid` intersects the smooth ellipsoid. It always answers, and on a
//     mountain it answers with a point that is not where you clicked, because the ellipsoid is
//     kilometres below the ground you can see.
//   - `scene.pickPosition` intersects what is actually rendered, which is the terrain. It is the
//     right answer and it can FAIL - it needs a depth buffer, and it returns undefined when the
//     click misses the globe or the depth texture is unavailable.
//
// So the terrain pick is tried first and the ellipsoid is the fallback, and **the result says
// which one answered**. A coordinate whose provenance is unstated is the kind of number this
// project has been wrong about before: an ellipsoid pick on a 4,000 m peak can be several
// kilometres from the summit somebody was aiming at, and it looks identical to a good one.
//
// The ELEVATION is read from the engine rather than from the pick, always. The rendered height
// is subject to vertical exaggeration and to whatever level of detail happened to be loaded;
// the engine's answer is canonical and is what the game and the exporter will use.

/// How the position was obtained. Carried into the result, never inferred later.
export const FROM_TERRAIN = "terrain";
export const FROM_ELLIPSOID = "ellipsoid (terrain pick failed - may be off the ground you clicked)";

/// Turn a screen position into a place on the planet.
///
/// Returns `{ latitude, longitude, renderedHeightM, elevationM, source }`, or null if the click
/// missed the globe entirely.
export function pickAt(viewer, Cesium, windowPosition, elevationAt = null) {
  const scene = viewer.scene;
  let cartesian = null;
  let source = FROM_TERRAIN;

  if (scene.pickPositionSupported) {
    cartesian = scene.pickPosition(windowPosition);
  }
  if (!Cesium.defined(cartesian)) {
    cartesian = viewer.camera.pickEllipsoid(windowPosition, scene.globe.ellipsoid);
    source = FROM_ELLIPSOID;
  }
  if (!Cesium.defined(cartesian)) return null;

  const carto = Cesium.Cartographic.fromCartesian(cartesian);
  const latitude = Cesium.Math.toDegrees(carto.latitude);
  const longitude = Cesium.Math.toDegrees(carto.longitude);
  return {
    latitude,
    longitude,
    renderedHeightM: carto.height,
    // Canonical ground, from the engine. `renderedHeightM` is what the screen showed, which is
    // the same number only when exaggeration is 1 and the finest tile happens to be loaded.
    elevationM: elevationAt ? elevationAt(latitude, longitude) : null,
    source,
  };
}

/// A `?fly=` fragment that brings the camera back to a pick.
export function flyFragment(pick, heightM) {
  const height = Math.round(
    heightM || Math.max(2000, Math.abs(pick.elevationM || 0) * 6 + 4000),
  );
  return `fly=${pick.latitude.toFixed(6)},${pick.longitude.toFixed(6)},${height}`;
}

/// Wire click-to-pick onto a viewer. Returns `{ stop, last }`.
///
/// `onPick` is called with the result. The handler is installed on LEFT_CLICK rather than on
/// mouse-down, so a drag to rotate the globe does not drop a pin every time the camera moves.
export function enablePicking(viewer, Cesium, onPick, elevationAt = null) {
  const handler = new Cesium.ScreenSpaceEventHandler(viewer.scene.canvas);
  let last = null;
  handler.setInputAction((movement) => {
    const pick = pickAt(viewer, Cesium, movement.position, elevationAt);
    if (!pick) return;
    last = pick;
    onPick(pick);
  }, Cesium.ScreenSpaceEventType.LEFT_CLICK);
  return {
    stop: () => handler.destroy(),
    last: () => last,
  };
}

/// Drop a marker at a pick, replacing any previous one.
///
/// The same three rules the area pins follow, for the same reason: never depth-tested, never
/// range-culled, scaled rather than fixed. A pin you cannot see from orbit is a pin you will
/// drop twice.
export function markPick(viewer, Cesium, pick, previous = null) {
  if (previous) viewer.entities.remove(previous);
  return viewer.entities.add({
    name: "picked point",
    position: Cesium.Cartesian3.fromDegrees(pick.longitude, pick.latitude),
    point: {
      pixelSize: 10,
      color: Cesium.Color.fromCssColorString("#ff4d6d"),
      outlineColor: Cesium.Color.WHITE,
      outlineWidth: 2,
      disableDepthTestDistance: Number.POSITIVE_INFINITY,
      heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
      scaleByDistance: new Cesium.NearFarScalar(1.0e3, 1.0, 2.0e7, 0.5),
    },
    label: {
      text: `${pick.latitude.toFixed(5)}, ${pick.longitude.toFixed(5)}`,
      font: "12px system-ui, sans-serif",
      fillColor: Cesium.Color.WHITE,
      outlineColor: Cesium.Color.BLACK,
      outlineWidth: 3,
      style: Cesium.LabelStyle.FILL_AND_OUTLINE,
      pixelOffset: new Cesium.Cartesian2(0, -18),
      verticalOrigin: Cesium.VerticalOrigin.BOTTOM,
      disableDepthTestDistance: Number.POSITIVE_INFINITY,
      heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
      scaleByDistance: new Cesium.NearFarScalar(1.0e3, 1.0, 2.0e7, 0.5),
    },
  });
}
