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
///   onPaint: `(features, brushName) => void` when a stroke is committed.
export function buildTools(parent, viewer, Cesium, onPaint) {
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

  arm.addEventListener("click", () => { armed = !armed; paintUI(); });
  chain.addEventListener("click", () => {
    chaining = !chaining;
    path = [];
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
    paintUI();
    if (onPaint) onPaint(features, current);
  });

  // Picking is on LEFT_CLICK and only while armed, so painting never competes with
  // dragging the globe or with the coordinate picker - the same rule the area pins follow.
  const handler = new Cesium.ScreenSpaceEventHandler(viewer.scene.canvas);
  handler.setInputAction((movement) => {
    if (!armed) return;
    const ray = viewer.camera.getPickRay(movement.position);
    const hit = ray && viewer.scene.globe.pick(ray, viewer.scene);
    if (!hit) return;
    const c = Cesium.Cartographic.fromCartesian(hit);
    const lat = Cesium.Math.toDegrees(c.latitude);
    const lon = Cesium.Math.toDegrees(c.longitude);
    if (chaining) {
      path.push([lat, lon]);
      paintUI();
      return;
    }
    const one = dab(current, lat, lon,
                    { size: Number(size.value), target: Number(height.value) });
    if (onPaint) onPaint([one], current);
  }, Cesium.ScreenSpaceEventType.LEFT_CLICK);

  sizeRow.append(sizeLabel);
  heightRow.append(heightLabel);
  wrap.append(grid, sizeRow, size, heightRow, height, hint,
              el("div", "wb-row").appendChild(arm).parentNode);
  const row2 = el("div", "wb-row");
  row2.append(chain, commit);
  wrap.append(row2);
  parent.append(wrap);

  size.value = String(BRUSHES[current].size);
  height.value = String(BRUSHES[current].target);
  paintUI();
  return { stop: () => handler.destroy(), brush: () => current };
}
