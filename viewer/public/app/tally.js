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
export function buildTally(document_, worldName = "—", wanted = 0) {
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

  // **What the generator is doing, from the click to the last line written.**
  //
  // A run is silent for thirty to forty-five seconds before its first pin, while the
  // ground is scored for every culture, and silent again afterwards while the roads are
  // laid and a six-megabyte worldfile is written. Both look exactly like a generator that
  // has hung, and the only cure is for it to say so.
  const dial = buildDial(document_, wanted);
  card.append(dial.node);
  const stageRow = dial.node;

  // **The clock is the part that never stops.** Every other signal on this card can sit
  // still for a minute at a time - the counts do not move while the ground is being scored
  // or the roads laid - and a readout that is not moving is one somebody starts refreshing
  // the page over.


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

  // Pause, resume and stop, under the dial while curation runs. Hidden until then: a
  // generation run has nothing to pause - it is over in minutes.
  const controls = el("div", "wb-tally-controls");
  controls.hidden = true;
  const control = (label, action) => {
    const button = el("button", "wb-mini", label);
    button.type = "button";
    button.dataset.action = action;
    controls.append(button);
    return button;
  };
  const pauseButton = control("pause", "pause");
  const resumeButton = control("resume", "resume");
  const stopButton = control("stop", "stop");
  card.insertBefore(controls, rows);
  let curating = false;
  let onControl = null;
  controls.addEventListener("click", (event) => {
    const action = event.target && event.target.dataset && event.target.dataset.action;
    if (action && onControl) onControl(action);
  });
  document_.body.appendChild(card);

  const landed = [];
  close.addEventListener("click", () => { card.style.display = "none"; });

  return {
    node: card,
    totals,
    areas: landed,
    setWorld: (name) => { head.querySelector(".wb-tally-world").textContent = name; },
    /// Say what the generator is doing now.
    ///
    /// `complete` is its own state rather than another line of text: the one thing a
    /// watcher most wants to know is whether it can stop watching.
    stage(named, note = "") { dial.stage(named, note); },

    show: () => { card.style.display = ""; },
    hide: () => { card.style.display = "none"; },
    hidden: () => card.style.display === "none",

    /// Show where the run's AI curation stands, from `/curate/`.
    ///
    /// Args:
    ///   status: the server's answer - `state`, `done`, `total`, `kept`, `left`,
    ///     `rate_per_min`, `eta_seconds`, `url`, and `stopped` or `error` when it ended badly.
    ///   act: `(action) => void`, called with "pause", "resume" or "stop".
    curation(status, act) {
      onControl = act;
      if (!curating) {
        curating = true;
        dial.phase("rooms", status.total || 0);
        controls.hidden = false;
      }
      dial.reading(status.done || 0);
      const state = status.state;
      const active = state === "running" || state === "starting";
      pauseButton.hidden = !active;
      resumeButton.hidden = !["paused", "stopped", "interrupted", "failed"].includes(state);
      stopButton.hidden = !(active || state === "paused");
      const named = {
        starting: "curating · finding the model", running: "curating rooms",
        paused: "curation paused", stopped: "curation stopped",
        interrupted: "curation interrupted", failed: "curation failed", done: "complete",
      }[state] || state;
      dial.stage(named, curationNote(status));
      if (state === "done") controls.hidden = true;
    },

    /// Fold one landed area into the counts.
    add(area) {
      landed.push(area);
      for (const f of FIELDS) {
        const before = totals[f.key];
        totals[f.key] = before + (f.of(area) || 0);
        if (totals[f.key] !== before) climb(values[f.key], before, totals[f.key]);
      }
      dial.reading(totals.areas);
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
    remove() {
      dial.stop();
      card.remove();
    },
  };
}


/// The line under the dial while curation runs: what was kept, how fast, how long, and which
/// road to the model it is using - the last because at home and away it is a different one,
/// and "why is it slow" is usually "because it is on Tailscale".
export function curationNote(status) {
  const parts = [];
  const kept = status.kept || 0;
  const left = status.left || 0;
  if (kept || left) {
    parts.push(`kept ${kept.toLocaleString()}`);
    if (left) parts.push(`${left.toLocaleString()} kept the template`);
  }
  if (status.rate_per_min) parts.push(`${Math.round(status.rate_per_min)} rooms/min`);
  const eta = status.eta_seconds;
  if (eta && (status.state === "running" || status.state === "starting")) {
    const hours = Math.floor(eta / 3600);
    const minutes = Math.round((eta % 3600) / 60);
    parts.push(hours ? `about ${hours}h ${minutes}m left` : `about ${minutes}m left`);
  }
  if (status.url) {
    const host = (() => { try { return new URL(status.url).hostname; } catch { return ""; } })();
    parts.push(host.startsWith("100.") ? "via Tailscale"
      : host.startsWith("192.168.") ? "via the house LAN" : host ? `via ${host}` : "");
  }
  if (status.state === "interrupted") parts.push("the curator stopped with the studio - resume carries on");
  if (status.state === "paused") parts.push("every finished room is kept; resume carries on");
  if (status.stopped) parts.push(String(status.stopped));
  if (status.error) parts.push(String(status.error));
  return parts.filter(Boolean).join(" · ");
}

/// The dial: how far along the run is, what it is doing, and how long it has been doing it.
///
/// **A number that does not move is indistinguishable from a program that has stopped.** A
/// run is silent for the better part of a minute before its first area lands, while the
/// ground is scored for every culture, and silent again at the end while the roads are laid
/// and a six-megabyte worldfile is written. Neither stretch moves a counter, so neither
/// stretch could be told from a hang.
///
/// Three things answer that, and they answer it in three different ways on purpose: the
/// needle says how far through, the words say what is happening, and the clock says that
/// something is still happening at all. The clock is the one that never stops, which is why
/// it is there even though it says nothing about progress.
function buildDial(document_, wanted = 0) {
  const NS = "http://www.w3.org/2000/svg";
  const SWEEP = 240;              // degrees of arc the needle travels
  const START = 150;              // where zero sits, measured clockwise from east
  const R = 46;

  const node = document_.createElement("div");
  node.className = "wb-dial";

  const svg = document_.createElementNS(NS, "svg");
  svg.setAttribute("viewBox", "0 0 120 92");
  svg.setAttribute("class", "wb-dial-face");

  const point = (degrees, radius) => {
    const radians = (degrees * Math.PI) / 180;
    return [60 + radius * Math.cos(radians), 60 + radius * Math.sin(radians)];
  };
  const arc = (from, to, radius) => {
    const [x1, y1] = point(from, radius);
    const [x2, y2] = point(to, radius);
    return `M ${x1} ${y1} A ${radius} ${radius} 0 ${to - from > 180 ? 1 : 0} 1 ${x2} ${y2}`;
  };

  const track = document_.createElementNS(NS, "path");
  track.setAttribute("d", arc(START, START + SWEEP, R));
  track.setAttribute("class", "wb-dial-track");
  svg.append(track);

  // Ticks every tenth, longer at the quarters, so the sweep reads as a scale and not a bar.
  for (let step = 0; step <= 10; step += 1) {
    const at = START + (SWEEP * step) / 10;
    const long = step % 5 === 0;
    const [x1, y1] = point(at, R - (long ? 9 : 5));
    const [x2, y2] = point(at, R - 1);
    const tick = document_.createElementNS(NS, "line");
    tick.setAttribute("x1", x1); tick.setAttribute("y1", y1);
    tick.setAttribute("x2", x2); tick.setAttribute("y2", y2);
    tick.setAttribute("class", long ? "wb-dial-tick wb-dial-tick-long" : "wb-dial-tick");
    svg.append(tick);
  }

  const filled = document_.createElementNS(NS, "path");
  filled.setAttribute("class", "wb-dial-filled");
  filled.setAttribute("d", arc(START, START + 0.01, R));
  svg.append(filled);

  const needle = document_.createElementNS(NS, "line");
  needle.setAttribute("class", "wb-dial-needle");
  svg.append(needle);
  const hub = document_.createElementNS(NS, "circle");
  hub.setAttribute("cx", 60); hub.setAttribute("cy", 60); hub.setAttribute("r", 3.4);
  hub.setAttribute("class", "wb-dial-hub");
  svg.append(hub);

  const reading = document_.createElementNS(NS, "text");
  reading.setAttribute("x", 60); reading.setAttribute("y", 54);
  reading.setAttribute("class", "wb-dial-reading");
  reading.textContent = "0";
  svg.append(reading);
  const scale = document_.createElementNS(NS, "text");
  scale.setAttribute("x", 60); scale.setAttribute("y", 68);
  scale.setAttribute("class", "wb-dial-scale");
  scale.textContent = wanted ? `of ${wanted} areas` : "areas";
  svg.append(scale);

  node.append(svg);

  const words = document_.createElement("div");
  words.className = "wb-dial-words";
  const name = document_.createElement("span");
  name.className = "wb-dial-stage";
  name.textContent = "starting the generator";
  const clock = document_.createElement("span");
  clock.className = "wb-dial-clock";
  clock.textContent = "0:00";
  words.append(name, clock);
  const note = document_.createElement("div");
  note.className = "wb-dial-note";
  note.textContent = "spawning the runner and loading the engine";
  node.append(words, note);

  const swing = (share) => {
    const at = START + SWEEP * Math.max(0, Math.min(1, share));
    const [x, y] = point(at, R - 12);
    needle.setAttribute("x1", 60); needle.setAttribute("y1", 60);
    needle.setAttribute("x2", x); needle.setAttribute("y2", y);
    filled.setAttribute("d", arc(START, Math.max(START + 0.01, at), R));
  };
  swing(0);

  let began = Date.now();
  const tick = () => {
    const seconds = Math.round((Date.now() - began) / 1000);
    clock.textContent =
      `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
  };
  tick();
  let ticking = window.setInterval(tick, 1000);

  return {
    node,
    /// Start a second measure on the same dial: the needle rescaled to a new total, the
    /// clock started again. Curation is a run of its own inside the run - hours to the
    /// generator's minutes - and a needle already pinned at "complete" says nothing about it.
    phase(unit, total) {
      wanted = Number(total) || 0;
      scale.textContent = wanted ? `of ${wanted.toLocaleString()} ${unit}` : unit;
      reading.textContent = "0";
      node.dataset.done = "";
      node.dataset.phase = unit;
      swing(0);
      began = Date.now();
      tick();
      if (!ticking) ticking = window.setInterval(tick, 1000);
    },
    /// Move the needle to a count of areas.
    reading(count) {
      reading.textContent = Number(count || 0).toLocaleString();
      if (wanted) swing(count / wanted);
    },
    /// Say what is happening, in words, and stop the clock when it is over.
    stage(named, said = "") {
      if (!named) return;
      const done = named === "complete";
      name.textContent = named;
      note.textContent = said || "";
      node.dataset.done = done ? "yes" : "";
      if (done) {
        swing(1);
        if (ticking) {
          tick();
          window.clearInterval(ticking);
          ticking = null;
        }
      }
    },
    stop() {
      if (ticking) window.clearInterval(ticking);
      ticking = null;
    },
  };
}
