//! The running count, so a world can be watched being built rather than reported at the end.
//
// A populate run is the one moment this tool has something to show, and a progress bar
// throws it away - it says how far through we are and nothing about what is being made. A
// tally says what the world now contains and grows while you look at it, which is both the
// more useful readout and the more watchable one.
//
// **Every number here is counted from the areas as they land, never predicted.** A readout
// that showed a target and filled toward it would be describing the plan; this describes the
// world. If the generator produces ninety-three areas because seven failed their gates, the
// tally reads ninety-three - and the difference between that and the plan is exactly the
// thing worth noticing.

import { legendRows } from "./palette.js";

/// What is counted, in the order it reads best. `of` pulls the number out of one area.
const FIELDS = [
  { key: "areas", label: "Areas", of: () => 1 },
  { key: "rooms", label: "Rooms", of: (a) => a.rooms || 0 },
  { key: "npcs", label: "NPCs", of: (a) => a.npcs || 0 },
  { key: "shops", label: "Shops", of: (a) => a.shops || 0 },
  { key: "settlements", label: "Settlements",
    of: (a) => (a.purpose === "hunting" || a.faction === "hostile" ? 0 : 1) },
  { key: "hunting", label: "Hunting grounds",
    of: (a) => (a.purpose === "hunting" || a.faction === "hostile" ? 1 : 0) },
  { key: "docks", label: "Docks", of: (a) => a.docks || 0 },
];

function el(tag, cls, text) {
  const n = document.createElement(tag);
  if (cls) n.className = cls;
  if (text !== undefined) n.textContent = text;
  return n;
}

/// A number that climbs to its new value instead of jumping.
///
/// **The climb is the point.** A count that snaps from 40 to 87 when a batch lands reads as
/// a page refresh; the same count rolling up reads as a world being built, which is what is
/// actually happening. Kept short - a quarter second - so it never lags behind the pins.
function climb(node, from, to) {
  const start = performance.now();
  const span = 240;
  const step = (now) => {
    const t = Math.min(1, (now - start) / span);
    const eased = t * t * (3 - 2 * t);
    node.textContent = Math.round(from + (to - from) * eased).toLocaleString();
    if (t < 1) requestAnimationFrame(step);
  };
  requestAnimationFrame(step);
  // **The number must be right even when nothing animates it.** `requestAnimationFrame`
  // does not fire in a backgrounded tab, so a run watched from another window - or from a
  // hidden pane - filled its totals correctly and displayed nought across the board. The
  // animation is the nice part; the value is the point, so a timer lands it regardless and
  // the last frame agrees with it.
  setTimeout(() => { node.textContent = to.toLocaleString(); }, span + 60);
}

/// Build the tally card. Returns `{ add, reset, node, totals }`.
export function buildTally(document, worldName = "—") {
  const card = el("div", "wb-tally");
  const head = el("div", "wb-tally-head");
  head.append(el("span", "wb-tally-world-label", "World"),
              el("span", "wb-tally-world", worldName));
  card.append(head);

  const rows = el("div", "wb-tally-rows");
  const values = {};
  const totals = {};
  for (const f of FIELDS) {
    totals[f.key] = 0;
    const row = el("div", "wb-tally-row");
    row.append(el("span", "wb-tally-label", f.label));
    const v = el("span", "wb-tally-value", "0");
    values[f.key] = v;
    row.append(v);
    rows.append(row);
  }
  card.append(rows);

  // A key, because a screen of coloured dots means nothing without one - and this is the
  // readout somebody will be looking at while the world fills in.
  const key = el("div", "wb-legend");
  for (const row of legendRows()) {
    const item = el("span", "wb-legend-item");
    const swatch = el("span", "wb-swatch");
    swatch.style.background = row.colour;
    item.append(swatch, el("span", "wb-legend-label", row.label));
    key.append(item);
  }
  card.append(key);

  const foot = el("div", "wb-tally-foot", "");
  card.append(foot);
  document.body.appendChild(card);

  return {
    node: card,
    totals,
    setWorld: (name) => { head.querySelector(".wb-tally-world").textContent = name; },
    /// Fold one landed area into the counts.
    add(area) {
      for (const f of FIELDS) {
        const before = totals[f.key];
        totals[f.key] = before + (f.of(area) || 0);
        if (totals[f.key] !== before) climb(values[f.key], before, totals[f.key]);
      }
      const name = area.display_name || area.name;
      // The last thing built, named. During a long run this is the line that tells you the
      // generator is still finding new kinds of place rather than repeating one.
      foot.textContent = name ? `+ ${name}` : "";
    },
    reset() {
      for (const f of FIELDS) {
        totals[f.key] = 0;
        values[f.key].textContent = "0";
      }
      foot.textContent = "";
    },
    remove() { card.remove(); },
  };
}
