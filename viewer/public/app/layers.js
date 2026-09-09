//! The feature stack, as layers.
//
// The engine already works the way an image editor does and nobody designed it that way -
// it fell out of being point-evaluable. The seed is the base image and is never edited; the
// features are adjustment layers composited over it; and **order is meaning**, exactly as
// it is in a layer stack: a bar listed after the channel it lies across sits on the carved
// bottom, and listed before it the channel cuts straight through it. That is not a metaphor
// laid over the model, it is the model.
//
// So this is the panel that was missing rather than a new system. It shows the stack, lets
// it be reordered and lets a layer be switched off, and every one of those is a real
// operation on the world rather than a view setting.
//
// **Grouped by kind, not one row per feature.** A world with a river network carries a
// thousand carve records and nobody wants a thousand rows; what a person edits is "the
// rivers", "the dredged channel", "the lakes". A brush stroke will name its own group when
// painting lands, and then the grouping is by stroke rather than by kind - same panel.

/// What each composition mode does, in the fewest words that are still true.
const COMPOSE = {
  raise: "only ever shallower",
  carve: "only ever deeper",
  shape: "either way",
};

/// Group a worldfile's features into layers, keeping the file's own order.
///
/// Order is preserved because order is the composite. Sorting these for tidiness - by name,
/// by size, by anything - would silently re-author the seabed.
export function groupFeatures(features) {
  const groups = [];
  const index = new Map();
  (features || []).forEach((f, i) => {
    const kind = f.kind || "feature";
    if (!index.has(kind)) {
      const g = { kind, compose: f.compose || "raise", count: 0, first: i,
                  visible: true, targets: [] };
      index.set(kind, g);
      groups.push(g);
    }
    const g = index.get(kind);
    g.count += 1;
    if (typeof f.target_m === "number") g.targets.push(f.target_m);
  });
  for (const g of groups) {
    if (g.targets.length) {
      g.low = Math.min(...g.targets);
      g.high = Math.max(...g.targets);
    }
    delete g.targets;
  }
  return groups;
}

function el(tag, cls, text) {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (text !== undefined) node.textContent = text;
  return node;
}

/// Build the layers panel into a parent element.
///
/// Args:
///   parent: where to attach.
///   getFeatures: `() => features`, read fresh so a newly opened world repaints.
///   onChange: `(order, hidden) => void` when the stack is reordered or a layer toggled.
///
/// Returns `{ refresh }`.
export function buildLayers(parent, getFeatures, onChange = null) {
  const wrap = el("div", "wb-section");
  wrap.append(el("div", "wb-section-title", "layers"));
  const list = el("div", "wb-layers");
  const note = el("div", "wb-note-line");
  wrap.append(list, note);
  parent.append(wrap);

  let groups = [];

  const emit = () => {
    if (onChange) {
      onChange(groups.map((g) => g.kind),
               groups.filter((g) => !g.visible).map((g) => g.kind));
    }
  };

  const paint = () => {
    list.textContent = "";
    if (!groups.length) {
      note.textContent = "no features in this world";
      return;
    }
    const total = groups.reduce((n, g) => n + g.count, 0);
    // **The stack is drawn top-down, so the LAST feature is the top layer.** In the file
    // the last record has the final say, which is what "on top" means in a layer panel;
    // printing the file order straight down would put the most powerful layer at the
    // bottom and read backwards to anybody who has used an image editor.
    groups.slice().reverse().forEach((g, shown) => {
      const i = groups.length - 1 - shown;
      const row = el("div", "wb-layer" + (g.visible ? "" : " wb-layer-off"));

      const eye = el("button", "wb-layer-eye", g.visible ? "●" : "○");
      eye.type = "button";
      eye.title = g.visible ? "hide this layer" : "show this layer";
      eye.addEventListener("click", () => { g.visible = !g.visible; paint(); emit(); });

      const name = el("div", "wb-layer-name");
      name.append(el("span", "wb-layer-kind", g.kind));
      const depth = (g.low !== undefined && g.low !== g.high)
        ? `${g.low.toFixed(0)}..${g.high.toFixed(0)} m`
        : (g.low !== undefined ? `${g.low.toFixed(0)} m` : "");
      name.append(el("span", "wb-layer-meta",
                     `${g.count} · ${COMPOSE[g.compose] || g.compose}${depth ? " · " + depth : ""}`));

      const up = el("button", "wb-layer-move", "▲");
      up.type = "button";
      up.title = "later in the composite - this layer wins more arguments";
      up.disabled = i === groups.length - 1;
      up.addEventListener("click", () => {
        groups.splice(i + 1, 0, groups.splice(i, 1)[0]); paint(); emit();
      });

      const down = el("button", "wb-layer-move", "▼");
      down.type = "button";
      down.title = "earlier in the composite - later layers cut through this one";
      down.disabled = i === 0;
      down.addEventListener("click", () => {
        groups.splice(i - 1, 0, groups.splice(i, 1)[0]); paint(); emit();
      });

      row.append(eye, name, up, down);
      list.append(row);
    });
    const hidden = groups.filter((g) => !g.visible).length;
    note.textContent = `${total} features in ${groups.length} layers`
      + (hidden ? `, ${hidden} hidden` : "");
  };

  const refresh = () => {
    groups = groupFeatures(getFeatures());
    paint();
  };
  refresh();
  return { refresh, groups: () => groups };
}
