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
export const FROM_TERRAIN = "terrain (depth buffer)";
export const FROM_GLOBE_RAY = "terrain (ray cast)";
export const FROM_ELLIPSOID = "ellipsoid - the terrain picks failed, so this may be off the "
  + "ground you clicked";

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

  // **The depth buffer is not always readable, and it fails silently.** Measured in this
  // project's own browser harness: `pickPositionSupported` true, `depthTexture` true,
  // `depthTestAgainstTerrain` true, tiles loaded, camera 60 km over land - and
  // `scene.pickPosition` returns undefined at the centre of the canvas. Software rasterisers
  // and some drivers will not hand the depth attachment back.
  //
  // `globe.pick` intersects the loaded TERRAIN TILES with a ray instead. It needs no depth
  // texture, so it works where the buffer will not, and it is still the real ground rather
  // than the smooth ellipsoid. It has its own limit, and it is an honest one: it can only hit
  // tiles that are loaded, so a click on a region still streaming falls through to the
  // ellipsoid - which is exactly when the answer should be labelled as suspect.
  if (!Cesium.defined(cartesian)) {
    const ray = viewer.camera.getPickRay(windowPosition);
    if (Cesium.defined(ray)) {
      cartesian = scene.globe.pick(ray, scene);
      if (Cesium.defined(cartesian)) source = FROM_GLOBE_RAY;
    }
  }

  if (!Cesium.defined(cartesian)) {
    cartesian = viewer.camera.pickEllipsoid(windowPosition, scene.globe.ellipsoid);
    source = FROM_ELLIPSOID;
  }
  if (!Cesium.defined(cartesian)) return null;

  const carto = Cesium.Cartographic.fromCartesian(cartesian);
  const latitude = Cesium.Math.toDegrees(carto.latitude);
  const longitude = Cesium.Math.toDegrees(carto.longitude);
  const ground = elevationAt ? elevationAt(latitude, longitude) : null;

  // **How wrong the ellipsoid answer can be, as a distance rather than a warning.**
  //
  // An ellipsoid pick lands where the ray crosses sea level, but the ground is `ground` metres
  // above that, so the point actually clicked is displaced along the ray by `ground * tan(t)`,
  // where `t` is the angle between the ray and the local vertical. Straight down that is zero
  // and the fallback is exact; at forty-five degrees over a kilometre of ground it is a
  // kilometre out.
  //
  // Saying "may be off" tells somebody to worry. Saying "about 40 m" tells them whether to.
  let offsetM = 0;
  if (source === FROM_ELLIPSOID && ground !== null && Number.isFinite(ground)) {
    const up = Cesium.Cartesian3.normalize(cartesian, new Cesium.Cartesian3());
    const ray = viewer.camera.getPickRay(windowPosition);
    if (Cesium.defined(ray)) {
      const direction = Cesium.Cartesian3.normalize(ray.direction, new Cesium.Cartesian3());
      // The ray points down into the planet, so the cosine against "up" is negative.
      const cosine = -Cesium.Cartesian3.dot(direction, up);
      const clamped = cosine > 1 ? 1 : cosine < 1e-6 ? 1e-6 : cosine;
      const tangent = Math.sqrt(Math.max(0, 1 - clamped * clamped)) / clamped;
      offsetM = Math.abs(ground) * tangent;
    }
  }

  return {
    latitude,
    longitude,
    renderedHeightM: carto.height,
    /// Metres the ellipsoid fallback may be displaced from what was clicked. Zero for a terrain
    /// pick, and zero looking straight down.
    offsetM,
    // Canonical ground, from the engine. `renderedHeightM` is what the screen showed, which is
    // the same number only when exaggeration is 1 and the finest tile happens to be loaded.
    elevationM: ground,
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
