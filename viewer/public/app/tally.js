//! The running count, and what the world turned out to be once the run is done.
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
//
// **Depth is folded away until it is asked for.** When the run ends the card grows sections
// - what levels the world covers, who lives where, what the shops hold, how the network
// hangs together - and each is a single line until it is opened. The point is to be able to
// answer a question about the world without turning the answer into a wall somebody has to
// read past to see the globe.

import { legendRows } from "./palette.js";

/// What is counted, in the order it reads best. `of` pulls the number out of one area.
const FIELDS = [
  { key: "areas", label: "Areas", of: () => 1 },
  { key: "rooms", label: "Rooms", of: (a) => a.rooms || 0 },
  { key: "npcs", label: "NPCs", of: (a) => a.npcs || 0 },
  { key: "shops", label: "Shops", of: (a) => a.shops || 0 },
  { key: "items", label: "Wares", of: (a) => a.items || 0 },
  { key: "settlements", label: "Settlements",
    of: (a) => (a.purpose === "hunting" || a.faction === "hostile" ? 0 : 1) },
  { key: "hunting", label: "Hunting grounds",
    of: (a) => (a.purpose === "hunting" || a.faction === "hostile" ? 1 : 0) },
  { key: "docks", label: "Docks", of: (a) => a.docks || 0 },
];

/// The level bands, in the order a character meets them.
const BANDS = ["1-5", "6-10", "11-20", "21-40", "41-60", "61-100"];

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

/// One folded section: a summary line that opens onto its own detail.
function section(title, build) {
  const box = document.createElement("details");
  box.className = "wb-tally-section";
  const head = document.createElement("summary");
  head.textContent = title;
  box.append(head);
  const body = el("div", "wb-tally-body");
  box.append(body);
  build(body);
  return box;
}

/// A labelled bar. `share` is 0 to 1; the number is written, never implied by width alone.
function bar(label, count, share, colour) {
  const row = el("div", "wb-bar-row");
  row.append(el("span", "wb-bar-label", label));
  const track = el("span", "wb-bar-track");
  const fill = el("span", "wb-bar-fill");
  fill.style.width = `${Math.round(Math.max(0.02, share) * 100)}%`;
  if (colour) fill.style.background = colour;
  track.append(fill);
  row.append(track, el("span", "wb-bar-count", count.toLocaleString()));
  return row;
}

function tallyBy(areas, key) {
  const out = new Map();
  for (const area of areas) {
    const value = typeof key === "function" ? key(area) : area[key];
    if (value === undefined || value === null) continue;
    out.set(value, (out.get(value) || 0) + 1);
  }
  return out;
}

/// Build the tally card. Returns the card plus `add`, `finish`, `show`, `hide` and `reset`.
export function buildTally(document_, worldName = "—") {
  const card = el("div", "wb-tally");
  const head = el("div", "wb-tally-head");
  head.append(el("span", "wb-tally-world-label", "World"),
              el("span", "wb-tally-world", worldName));
  // **A readout with no way to put it away is a readout that outstays its welcome.** It
  // covers a corner of the globe for the rest of the session, and the thing somebody wants
  // after looking at a world is to look at the world.
  const close = el("button", "wb-tally-close", "×");
  close.type = "button";
  close.title = "hide this - the run is kept";
  head.append(close);
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

  const detail = el("div", "wb-tally-detail");
  card.append(detail);

  const foot = el("div", "wb-tally-foot", "");
  card.append(foot);
  document_.body.appendChild(card);

  const landed = [];
  close.addEventListener("click", () => { card.style.display = "none"; });

  return {
    node: card,
    totals,
    areas: landed,
    setWorld: (name) => { head.querySelector(".wb-tally-world").textContent = name; },
    show: () => { card.style.display = ""; },
    hide: () => { card.style.display = "none"; },
    hidden: () => card.style.display === "none",

    /// Fold one landed area into the counts.
    add(area) {
      landed.push(area);
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

    /// Once the run is over, say what the world turned out to be.
    ///
    /// Built from the areas that actually landed and from the run's own manifest, so
    /// nothing here is a restatement of the plan.
    finish(summary = {}) {
      detail.textContent = "";
      if (!landed.length) return;

      const byBand = tallyBy(landed, (a) => (Array.isArray(a.level_band)
        ? a.level_band.join("-") : null));
      const bandMost = Math.max(1, ...byBand.values());
      detail.append(section("levels", (body) => {
        for (const band of BANDS) {
          const count = byBand.get(band) || 0;
          if (count) body.append(bar(`lvl ${band}`, count, count / bandMost));
        }
        const far = landed.reduce((a, b) =>
          ((b.from_origin_km || 0) > (a.from_origin_km || 0) ? b : a));
        body.append(el("div", "wb-tally-note",
          `furthest: ${far.display_name} at ${Math.round(far.from_origin_km || 0)} km`));
      }));

      const byRace = tallyBy(landed, "race");
      const raceMost = Math.max(1, ...byRace.values());
      const colours = new Map(legendRows().map((r) => [r.label, r.colour]));
      detail.append(section("peoples", (body) => {
        for (const [race, count] of [...byRace].sort((a, b) => b[1] - a[1])) {
          const theirs = landed.filter((a) => a.race === race);
          const lows = theirs.map((a) => (a.level_band || [0, 0])[0]);
          const highs = theirs.map((a) => (a.level_band || [0, 0])[1]);
          const rooms = theirs.reduce((n, a) => n + (a.rooms || 0), 0);
          body.append(bar(race, count, count / raceMost, colours.get(race)));
          body.append(el("div", "wb-tally-note",
            `levels ${Math.min(...lows)}-${Math.max(...highs)}, `
            + `${rooms.toLocaleString()} rooms`));
        }
      }));

      detail.append(section("trade", (body) => {
        const shops = totals.shops || 0;
        const items = totals.items || 0;
        body.append(el("div", "wb-tally-note",
          `${items.toLocaleString()} wares in ${shops.toLocaleString()} shops`
          + (shops ? `, ${(items / shops).toFixed(1)} a shop` : "")));
        const bySize = tallyBy(landed, "size");
        const sizeMost = Math.max(1, ...bySize.values());
        for (const [size, count] of [...bySize].sort((a, b) => b[1] - a[1])) {
          const theirs = landed.filter((a) => a.size === size);
          const here = theirs.reduce((n, a) => n + (a.shops || 0), 0);
          body.append(bar(size, count, count / sizeMost));
          body.append(el("div", "wb-tally-note",
            `${here} shops, ${(here / Math.max(1, count)).toFixed(1)} each`));
        }
      }));

      detail.append(section("network", (body) => {
        const line = (label, value) => {
          if (value === undefined || value === null) return;
          body.append(el("div", "wb-tally-note",
            `${label}: ${Number(value).toLocaleString()}`));
        };
        line("roads", summary.roads);
        line("side paths", summary.paths);
        line("ferry crossings", summary.ferries);
        line("crossroads", summary.crossings);
        line("rooms on the roads", summary.road_rooms);
        line("stranded areas", summary.stranded);
        const missing = Object.entries(summary.unfilled || {})
          .map(([race, n]) => `${n} ${race}`).join(", ");
        if (missing) body.append(el("div", "wb-tally-note", `no ground for: ${missing}`));
        if (summary.over_quota) {
          body.append(el("div", "wb-tally-note",
            `${summary.over_quota} placed outside the quota to reach the count`));
        }
      }));

      foot.textContent = `${landed.length} areas - open a section for detail`;
    },

    reset() {
      landed.length = 0;
      detail.textContent = "";
      for (const f of FIELDS) {
        totals[f.key] = 0;
        values[f.key].textContent = "0";
      }
      foot.textContent = "";
    },
    remove() { card.remove(); },
  };
}
