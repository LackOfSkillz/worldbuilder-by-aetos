//! Brushes that paint landforms, by writing the same feature records the engine already
//! composites.
//
// **A stroke is not pixels, it is a landform.** Every brush here emits exactly the record
// type the generator has always used - `RAISE` for ground that must stand proud, `CARVE`
// for ground that must be cut - so a painted mountain is a real rise the site scorer can
// measure prominence on, and a painted channel is water a ferry can actually sail. Nothing
// is baked: the seed still makes the base, and strokes composite over it in order, which is
// why the layers panel can reorder them and mean it.
//
// **Five brushes, deliberately.** The value is a small set of tools over a real physical
// model, not a large set over a bitmap. Each one is a couple of numbers over the same
// record, and the differences between them are honest differences in what the ground does.

/// The brushes. `compose` and the sign of `target` are the whole difference between them.
export const BRUSHES = {
  mountain: {
    label: "Mountain", compose: "raise", target: 1800, size: 40000,
    hint: "raises ground to a peak height. Only ever shallower, so it stands on whatever is there.",
  },
  island: {
    label: "Island", compose: "raise", target: 60, size: 6000,
    hint: "raises seabed above datum. A round footprint - draw several for a chain.",
  },
  hill: {
    label: "Hills", compose: "raise", target: 260, size: 18000,
    hint: "gentler ground. The band the site scorer likes for settlements.",
  },
  lake: {
    label: "Lake", compose: "carve", target: -12, size: 4000,
    hint: "cuts a basin. Below the ground it sits in, so an upland lake is not at sea level.",
  },
  channel: {
    label: "Channel", compose: "carve", target: -14, size: 2000,
    hint: "cuts navigable water. Chain them along a line to make a river or a fairway.",
  },
};

/// How much a chained stroke's segments overlap so the chain does not shoal at its nodes.
///
/// **Measured, not chosen.** The first river carved deepest at the midpoints and shallowest
/// at the nodes, shoaling at 34 of 90 soundings, because a chain of bumps meets at its
/// edges. 4.5 is the factor that makes the joins as deep as the middles, and it is the same
/// constant `river.js` uses - imported in spirit rather than restated, and if it ever moves
/// this must move with it.
export const OVERLAP = 4.5;

function bearing(a, b) {
  const y = Math.sin((b[1] - a[1]) * Math.PI / 180) * Math.cos(b[0] * Math.PI / 180);
  const x = Math.cos(a[0] * Math.PI / 180) * Math.sin(b[0] * Math.PI / 180)
          - Math.sin(a[0] * Math.PI / 180) * Math.cos(b[0] * Math.PI / 180)
            * Math.cos((b[1] - a[1]) * Math.PI / 180);
  return (Math.atan2(y, x) * 180 / Math.PI + 360) % 360;
}

function metres(a, b, radiusM) {
  const p = Math.PI / 180;
  const h = Math.sin((b[0] - a[0]) * p / 2) ** 2
    + Math.cos(a[0] * p) * Math.cos(b[0] * p) * Math.sin((b[1] - a[1]) * p / 2) ** 2;
  return 2 * Math.asin(Math.sqrt(h)) * radiusM;
}

/// One dab: a single round feature at a point.
export function dab(brush, latitudeDeg, longitudeDeg, { size, target, layer } = {}) {
  const b = BRUSHES[brush];
  const width = size || b.size;
  return {
    kind: layer || `painted ${brush}`,
    latitude_deg: Number(latitudeDeg.toFixed(6)),
    longitude_deg: Number(longitudeDeg.toFixed(6)),
    target_m: target === undefined ? b.target : target,
    length_m: width,
    width_m: width,
    bearing_deg: 0,
    compose: b.compose,
    substrate: "derive",
    marked: false,
  };
}

/// A stroke along a path: one feature per segment, overlapped so the joins do not shoal.
///
/// This is what makes a river a river rather than a row of ponds, and it is the same
/// arithmetic `river.features_from_points` does - kept here so a brush can draw any of the
/// carve kinds along a line, not only water.
export function stroke(brush, points, radiusM, { size, target, layer } = {}) {
  const b = BRUSHES[brush];
  const width = size || b.size;
  const out = [];
  for (let i = 0; i < points.length - 1; i += 1) {
    const a = points[i], c = points[i + 1];
    const leg = metres(a, c, radiusM);
    if (leg <= 0) continue;
    out.push({
      kind: layer || `painted ${brush}`,
      latitude_deg: Number(((a[0] + c[0]) / 2).toFixed(6)),
      longitude_deg: Number(((a[1] + c[1]) / 2).toFixed(6)),
      target_m: target === undefined ? b.target : target,
      length_m: (leg / 2) * OVERLAP,
      width_m: width,
      bearing_deg: Number(bearing(a, c).toFixed(3)),
      compose: b.compose,
      substrate: "derive",
      marked: false,
    });
  }
  return out;
}

//: What a held stroke looks like before it is real ground.
//:
//: **A ghost is not a preview of the terrain, it is a preview of the intent.** Rebuilding
//: the globe to show one dab costs seconds, so the honest cheap thing to draw is the
//: feature's own footprint - where it is, how big it is, which way it lies and whether it
//: raises or carves. That is exactly the record the engine will composite, drawn flat.
const GHOST = {
  raise: { fill: "rgba(214,166,96,0.34)", edge: "rgba(214,166,96,0.85)" },
  carve: { fill: "rgba(86,150,214,0.34)", edge: "rgba(86,150,214,0.85)" },
};

/// Walk `distanceM` from a point along a bearing, on a sphere of `radiusM`.
function along(latDeg, lonDeg, bearingDeg, distanceM, radiusM) {
  const rad = Math.PI / 180;
  const lat = latDeg * rad, lon = lonDeg * rad, brg = bearingDeg * rad;
  const d = distanceM / radiusM;
  const lat2 = Math.asin(Math.sin(lat) * Math.cos(d)
    + Math.cos(lat) * Math.sin(d) * Math.cos(brg));
  const lon2 = lon + Math.atan2(Math.sin(brg) * Math.sin(d) * Math.cos(lat),
                                Math.cos(d) - Math.sin(lat) * Math.sin(lat2));
  return [lat2 / rad, ((lon2 / rad + 540) % 360) - 180];
}

/// Draw held features as translucent footprints, and take them away again.
///
/// Returns `{ show, clear, count }`. Entities are clamped to the ground so a ghost sits on
/// the terrain that is there now, which is the terrain the stroke is about to change.
///
/// **A footprint alone is invisible at the zoom people paint from.** A forty-kilometre
/// brush is under a pixel wide from twenty thousand kilometres up, so the first ghosts
/// were drawn correctly and could not be seen - which is indistinguishable from a brush
/// that does nothing. So every feature also gets a screen-space mark: a line along its own
/// axis if it is a stroke, a dot if it is a dab. Those keep their width in pixels, so a
/// ghost is legible from orbit and the true footprint appears underneath it on approach.
function ghostLayer(viewer, Cesium) {
  const entities = [];
  const radiusM = viewer.scene.globe.ellipsoid.maximumRadius;
  return {
    show(features) {
      for (const f of features) {
        const paint = GHOST[f.compose] || GHOST.raise;
        const edge = Cesium.Color.fromCssColorString(paint.edge);
        if (f.length_m > f.width_m * 1.5) {
          const half = f.length_m / 2;
          const a = along(f.latitude_deg, f.longitude_deg, f.bearing_deg || 0, half, radiusM);
          const b = along(f.latitude_deg, f.longitude_deg,
                          (f.bearing_deg || 0) + 180, half, radiusM);
          entities.push(viewer.entities.add({
            polyline: {
              positions: Cesium.Cartesian3.fromDegreesArray([a[1], a[0], b[1], b[0]]),
              width: 3,
              material: edge,
              clampToGround: true,
            },
          }));
        } else {
          entities.push(viewer.entities.add({
            position: Cesium.Cartesian3.fromDegrees(f.longitude_deg, f.latitude_deg),
            point: {
              pixelSize: 8,
              color: Cesium.Color.fromCssColorString(paint.fill),
              outlineColor: edge,
              outlineWidth: 1.5,
              heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
            },
          }));
        }
        entities.push(viewer.entities.add({
          position: Cesium.Cartesian3.fromDegrees(f.longitude_deg, f.latitude_deg),
          ellipse: {
            // The engine reads `length_m` and `width_m` as full extents; Cesium wants
            // semi-axes, so both are halved here rather than in the record.
            semiMajorAxis: Math.max(f.length_m, f.width_m) / 2,
            semiMinorAxis: Math.min(f.length_m, f.width_m) / 2,
            rotation: Cesium.Math.toRadians(90 - (f.bearing_deg || 0)),
            material: Cesium.Color.fromCssColorString(paint.fill),
            // **Flat at datum, not clamped.** Cesium refuses an outline on ground-clamped
            // geometry and ignores a `heightReference` with no height, so asking for both
            // bought two warnings and neither effect. A footprint is a plan view of where
            // the feature will be; sea level is the honest place to draw it, and the
            // screen-space mark above carries legibility at any zoom.
            height: 0,
            outline: true,
            outlineColor: edge,
          },
        }));
      }
    },
    clear() {
      for (const entity of entities) viewer.entities.remove(entity);
      entities.length = 0;
    },
    count: () => entities.length,
  };
}

function el(tag, cls, text) {
  const n = document.createElement(tag);
  if (cls) n.className = cls;
  if (text !== undefined) n.textContent = text;
  return n;
}

/// Build the paint tools into a parent element.
///
/// Args:
///   parent: where to attach.
///   viewer, Cesium: for picking a point on the globe.
///   onPaint: `(features, brushName) => void` when a stroke is laid down. The stroke is
///     HELD, not applied - it is ghosted on the globe and nothing rebuilds.
///   hooks: `{ onApply, onDiscard }`. `onApply` is what actually makes the held strokes
///     ground, and is the only expensive call in this file.
export function buildTools(parent, viewer, Cesium, onPaint, hooks = {}) {
  const wrap = el("div", "wb-section");
  wrap.append(el("div", "wb-section-title", "paint"));

  const grid = el("div", "wb-brushes");
  const sizeRow = el("div", "wb-row");
  const sizeLabel = el("label", "wb-brush-field");
  const size = document.createElement("input");
  size.type = "range"; size.min = "500"; size.max = "120000"; size.step = "500";
  const heightRow = el("div", "wb-row");
  const heightLabel = el("label", "wb-brush-field");
  const height = document.createElement("input");
  height.type = "range"; height.min = "-400"; height.max = "4000"; height.step = "10";
  const hint = el("div", "wb-note-line");
  const arm = el("button", "wb-mini", "click the globe: off");
  arm.type = "button";
  const chain = el("button", "wb-mini", "chain: off");
  chain.type = "button";
  const commit = el("button", "wb-mini", "finish stroke");
  commit.type = "button";
  commit.disabled = true;
  const apply = el("button", "wb-mini wb-mini-go", "apply 0 strokes");
  apply.type = "button";
  apply.disabled = true;
  const discard = el("button", "wb-mini", "discard");
  discard.type = "button";
  discard.disabled = true;
  const ghosts = ghostLayer(viewer, Cesium);
  let heldCount = 0;

  let current = "mountain";
  let armed = false;
  let chaining = false;
  let path = [];

  const paintUI = () => {
    const b = BRUSHES[current];
    for (const node of grid.children) {
      node.classList.toggle("wb-brush-on", node.dataset.brush === current);
    }
    sizeLabel.textContent = `size ${(Number(size.value) / 1000).toFixed(1)} km`;
    heightLabel.textContent = (b.compose === "carve" ? "depth " : "height ")
      + `${Number(height.value)} m`;
    hint.textContent = b.hint;
    arm.textContent = armed ? "click the globe: ON" : "click the globe: off";
    arm.classList.toggle("wb-brush-on", armed);
    chain.textContent = chaining ? `chain: ${path.length} node(s)` : "chain: off";
    chain.classList.toggle("wb-brush-on", chaining);
    commit.disabled = !(chaining && path.length > 1);
    apply.textContent = `apply ${heldCount} stroke${heldCount === 1 ? "" : "s"}`;
    apply.disabled = heldCount === 0;
    discard.disabled = heldCount === 0;
  };

  for (const [name, b] of Object.entries(BRUSHES)) {
    const node = el("button", "wb-brush", b.label);
    node.type = "button";
    node.dataset.brush = name;
    node.addEventListener("click", () => {
      current = name;
      size.value = String(b.size);
      height.value = String(b.target);
      paintUI();
    });
    grid.append(node);
  }
  size.addEventListener("input", paintUI);
  height.addEventListener("input", paintUI);

  arm.addEventListener("click", () => {
    armed = !armed;
    takeTheDrag(armed);
    if (!armed) { dragging = false; path = []; }
    paintUI();
  });
  chain.addEventListener("click", () => {
    // Kept as an explicit multi-click mode for placing a long line node by node, which a
    // drag cannot do across a camera move. A drag is the ordinary way; this is the careful
    // one.
    chaining = !chaining;
    path = [];
    paintUI();
  });


  // **One road for every finished stroke.** Chain-commit, drag-release and single dab all
  // end here, so there is exactly one place that decides a stroke is ghosted and held
  // rather than applied - and no way for a gesture to quietly take the expensive path.
  const lay = (features) => {
    if (!features.length) return;
    ghosts.show(features);
    heldCount += features.length;
    paintUI();
    if (onPaint) onPaint(features, current);
  };

  apply.addEventListener("click", async () => {
    apply.disabled = true;
    apply.textContent = "applying...";
    try {
      if (hooks.onApply) await hooks.onApply();
      ghosts.clear();
      heldCount = 0;
    } finally {
      paintUI();
    }
  });
  discard.addEventListener("click", () => {
    ghosts.clear();
    heldCount = 0;
    if (hooks.onDiscard) hooks.onDiscard();
    paintUI();
  });

  const radiusM = () => {
    const spec = (window.__wb && window.__wb.spec) || {};
    return Number(spec.radius) || 6371000;
  };

  commit.addEventListener("click", () => {
    if (path.length < 2) return;
    const features = stroke(current, path, radiusM(),
                            { size: Number(size.value), target: Number(height.value) });
    path = [];
    lay(features);
  });

  // **A brush must take the drag away from the camera.** Cesium owns click-and-drag for
  // rotating the globe, so an armed brush that only listened for clicks did nothing while
  // the world spun under the cursor - which is exactly what a paint tool must not do. So
  // arming the brush disables camera rotation and disarming gives it back, and the drag
  // becomes a stroke.
  //
  // Down, move, up: press to begin, drag to lay a line, release to commit. A press and
  // release without moving is a single dab, which is the same gesture an image editor
  // gives you and needs no separate mode.
  const controller = viewer.scene.screenSpaceCameraController;
  const cameraDefaults = {
    rotate: controller.enableRotate,
    translate: controller.enableTranslate,
    tilt: controller.enableTilt,
    look: controller.enableLook,
  };
  const takeTheDrag = (mine) => {
    controller.enableRotate = mine ? false : cameraDefaults.rotate;
    controller.enableTranslate = mine ? false : cameraDefaults.translate;
    controller.enableTilt = mine ? false : cameraDefaults.tilt;
    controller.enableLook = mine ? false : cameraDefaults.look;
    viewer.scene.canvas.style.cursor = mine ? "crosshair" : "";
  };

  const groundAt = (windowPosition) => {
    const ray = viewer.camera.getPickRay(windowPosition);
    const hit = ray && viewer.scene.globe.pick(ray, viewer.scene);
    if (!hit) return null;
    const c = Cesium.Cartographic.fromCartesian(hit);
    return [Cesium.Math.toDegrees(c.latitude), Cesium.Math.toDegrees(c.longitude)];
  };

  //: How far the cursor must travel before a drag lays another node, in screen pixels.
  //: Small enough to follow a curve, large enough that a stroke is not a thousand
  //: features - the same decimation the rivers needed, applied at the input end.
  const NODE_EVERY_PX = 26;

  let dragging = false;
  let lastPixel = null;
  let moved = false;

  const handler = new Cesium.ScreenSpaceEventHandler(viewer.scene.canvas);

  handler.setInputAction((event) => {
    if (!armed) return;
    dragging = true;
    moved = false;
    lastPixel = event.position;
    const point = groundAt(event.position);
    if (point) path.push(point);
    paintUI();
  }, Cesium.ScreenSpaceEventType.LEFT_DOWN);

  handler.setInputAction((event) => {
    if (!armed || !dragging) return;
    const px = event.endPosition;
    if (lastPixel && Math.hypot(px.x - lastPixel.x, px.y - lastPixel.y) < NODE_EVERY_PX) {
      return;
    }
    lastPixel = px;
    moved = true;
    const point = groundAt(px);
    if (point) path.push(point);
    paintUI();
  }, Cesium.ScreenSpaceEventType.MOUSE_MOVE);

  handler.setInputAction(() => {
    if (!armed || !dragging) return;
    dragging = false;
    const features = (moved && path.length > 1)
      ? stroke(current, path, radiusM(),
               { size: Number(size.value), target: Number(height.value) })
      : (path.length
         ? [dab(current, path[0][0], path[0][1],
                { size: Number(size.value), target: Number(height.value) })]
         : []);
    path = [];
    lay(features);
  }, Cesium.ScreenSpaceEventType.LEFT_UP);

  sizeRow.append(sizeLabel);
  heightRow.append(heightLabel);
  wrap.append(grid, sizeRow, size, heightRow, height, hint,
              el("div", "wb-row").appendChild(arm).parentNode);
  const row2 = el("div", "wb-row");
  row2.append(chain, commit);
  const row3 = el("div", "wb-row");
  row3.append(apply, discard);
  wrap.append(row2, row3);
  parent.append(wrap);

  size.value = String(BRUSHES[current].size);
  height.value = String(BRUSHES[current].target);
  paintUI();
  return {
    stop: () => { takeTheDrag(false); handler.destroy(); ghosts.clear(); },
    held: () => heldCount,
    brush: () => current,
  };
}
