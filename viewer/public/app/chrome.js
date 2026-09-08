//! The window dressing: a status line that folds away and a panel that does not scroll.
//
// The viewer grew a diagnostics HUD and a control panel at the same time and neither was
// ever designed - the HUD prints every number the generator knows in a five-line wall
// across the top of the globe, and the panel is one long column that needs scrolling to
// reach anything below the fold. Both are fine for building the thing and wrong for
// showing it to anybody.
//
// **This changes presentation only.** No control is removed, no number is lost: the HUD
// still holds every field, folded behind a summary, and every panel section still exists,
// folded behind its own heading. Deleting information to tidy a screen is how a tool loses
// the thing that made it useful, so nothing here deletes.
//
// **It works by walking the DOM rather than by editing the panel.** `world-panel.js` and
// `controls.js` build their sections from several places and are still being changed; a
// tidy-up that required editing all of them would rot the first time a section moved. This
// finds `.wb-section-title` wherever it appears and makes it a header, so a section added
// tomorrow is collapsible without anybody remembering this file exists.

/// Which panel sections stand open when the viewer loads.
///
/// Everything else folds. These are the two somebody reaches for first: what world am I
/// looking at, and what is in it.
const OPEN_BY_DEFAULT = ["worlds", "areas"];

/// Pull the few facts worth seeing at a glance out of the HUD's wall of text.
///
/// Deliberately forgiving: it matches what it recognises and shows the rest only when
/// expanded. A summary that threw because a field moved would be worse than the wall.
function summarise(text) {
  const grab = (re) => { const m = text.match(re); return m ? m[1] : null; };
  const seed = grab(/seed=(\d+)/);
  const plates = grab(/plates=(\d+)/);
  const land = grab(/land=([\d.]+)/);
  const areas = grab(/(\d+) areas/);
  const bits = [];
  if (seed) bits.push(`seed ${seed}`);
  if (plates) bits.push(`${plates} plates`);
  if (land) bits.push(`land ${land}`);
  if (areas) bits.push(`${areas} areas`);
  return bits.join("  ·  ") || "ready";
}

/// The top bar: the name, the menu, and the status, in the space the debug wall used.
///
/// **The widest, most valuable strip of the window was printing diagnostics.** Five lines
/// of generator internals ran across the top of the globe at all times - useful while
/// building the thing, and the first thing anybody sees. A menu bar belongs there; the
/// diagnostics belong behind a button on it.
///
/// The panel toggles are the other half of the point. Two columns fit without scrolling,
/// but for a screenshot or a recording you want the globe alone, and hiding both columns
/// is one click each rather than a layout mode nobody can find.
export function topBar(document) {
  if (document.getElementById("wb-topbar")) return null;
  const bar = document.createElement("div");
  bar.id = "wb-topbar";

  // The brand moves up here from the bottom corner: a name belongs in the chrome, not
  // floating over the sea.
  const brand = document.getElementById("wb-brand");
  const mark = document.createElement("div");
  mark.className = "wb-brand-inline";
  if (brand) {
    brand.remove();
    while (brand.firstChild) mark.append(brand.firstChild);
  } else {
    mark.textContent = "World Builder by Aetos";
  }
  bar.append(mark);

  const menu = document.createElement("div");
  menu.className = "wb-menu";
  bar.append(menu);

  const spacer = document.createElement("div");
  spacer.className = "wb-spacer";
  bar.append(spacer);

  const makeToggle = (label, targetId, startOn = true) => {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "wb-menu-item";
    b.textContent = label;
    let on = startOn;
    const paint = () => {
      b.classList.toggle("wb-menu-on", on);
      const target = document.getElementById(targetId);
      if (target) target.style.display = on ? "" : "none";
    };
    b.addEventListener("click", () => { on = !on; paint(); });
    // The panel may not exist yet when the bar is built.
    setTimeout(paint, 300);
    menu.append(b);
    return { paint, set: (v) => { on = v; paint(); } };
  };
  makeToggle("World", "wb-panel-left");
  makeToggle("Parameters", "wb-panel");

  document.body.appendChild(bar);
  return { bar, menu, spacer };
}

/// Fold the diagnostics HUD into one line, expandable.
export function tidyStatus(document) {
  const status = document.getElementById("status");
  if (!status || status.dataset.wbTidied) return null;
  status.dataset.wbTidied = "1";

  const summary = document.createElement("span");
  summary.className = "wb-status-summary";
  const toggle = document.createElement("button");
  toggle.type = "button";
  toggle.className = "wb-menu-item";
  toggle.textContent = "details";
  // Into the menu bar if there is one, so the status reads as part of the chrome rather
  // than as a second thing floating over the globe.
  const host = document.getElementById("wb-topbar");
  if (host) host.append(summary, toggle);
  else {
    const pill = document.createElement("div");
    pill.id = "wb-statusbar";
    pill.append(summary, toggle);
    status.parentNode.insertBefore(pill, status);
  }
  status.classList.add("wb-status-full");

  let open = false;
  const paint = () => {
    summary.textContent = summarise(status.textContent || "");
    status.style.display = open ? "block" : "none";
    toggle.textContent = open ? "hide" : "details";
  };
  toggle.addEventListener("click", () => { open = !open; paint(); });

  // The HUD is rewritten by the generator on every swap, so the summary follows it.
  new MutationObserver(paint).observe(status,
    { childList: true, characterData: true, subtree: true });
  paint();
  return { expand: () => { open = true; paint(); } };
}

/// Turn every panel section heading into a fold.
export function tidyPanel(document) {
  const panels = ["wb-panel", "wb-panel-left"]
    .map((id) => document.getElementById(id)).filter(Boolean);
  if (!panels.length) return null;

  const apply = () => {
    for (const panel of panels)
    for (const title of panel.querySelectorAll(".wb-section-title")) {
      if (title.dataset.wbFold) continue;
      title.dataset.wbFold = "1";
      title.classList.add("wb-fold-head");
      const caret = document.createElement("span");
      caret.className = "wb-caret";
      title.prepend(caret);

      // The section's body is its siblings up to the next heading - which is how these
      // panels are built, as a flat list rather than as nested boxes.
      const body = [];
      let node = title.nextElementSibling;
      while (node && !node.classList.contains("wb-section-title")) {
        body.push(node);
        node = node.nextElementSibling;
      }
      const name = (title.textContent || "").trim().toLowerCase();
      let open = OPEN_BY_DEFAULT.some((k) => name.startsWith(k));
      const paint = () => {
        title.classList.toggle("wb-open", open);
        for (const b of body) b.style.display = open ? "" : "none";
      };
      title.addEventListener("click", () => { open = !open; paint(); });
      paint();
    }
  };
  apply();
  // Sections are added after load - the area list appears when a world opens - so keep
  // folding whatever arrives rather than only what was there at boot.
  for (const panel of panels)
    new MutationObserver(apply).observe(panel, { childList: true, subtree: true });
  return { apply };
}

/// Sections that belong on the left: what world this is and what is in it.
///
/// The split is by KIND, not by size. The left column answers "what am I looking at" -
/// worlds, areas, routes, the library - and the right column answers "what is it made of",
/// the generator's parameters. Splitting by length instead would put related controls in
/// different places whenever a list grew.
const LEFT_SECTIONS = ["worlds", "saved in this browser", "on disk", "areas", "route",
                       "places", "recent"];

/// Give the panel two columns, using the empty half of the screen.
///
/// **The globe was losing a quarter of its width to a column that then still scrolled.**
/// One tall panel on the right meant everything below the fold needed a scroll to reach,
/// while the entire left half of the window held nothing at all. Two shorter columns fit
/// the same controls with no scrolling and leave the globe a clear middle.
export function splitColumns(document) {
  const right = document.getElementById("wb-panel");
  if (!right || document.getElementById("wb-panel-left")) return null;

  const left = document.createElement("div");
  left.id = "wb-panel-left";
  const heading = document.createElement("div");
  heading.className = "wb-panel-heading";
  heading.textContent = "world";
  left.append(heading);
  right.parentNode.insertBefore(left, right);

  const move = () => {
    for (const title of Array.from(right.querySelectorAll(".wb-section-title"))) {
      const name = (title.textContent || "").trim().toLowerCase();
      if (!LEFT_SECTIONS.some((k) => name.startsWith(k))) continue;
      // A section is its heading plus every sibling up to the next heading; move them
      // together or the body is orphaned in the column its title just left.
      const block = [title];
      let node = title.nextElementSibling;
      while (node && !node.classList.contains("wb-section-title")) {
        block.push(node);
        node = node.nextElementSibling;
      }
      for (const n of block) left.append(n);
    }
  };
  move();
  new MutationObserver(move).observe(right, { childList: true, subtree: true });
  return { left, move };
}

export function tidy(document) {
  const columns = splitColumns(document);
  const bar = topBar(document);
  return {
    bar,
    status: tidyStatus(document),
    panel: tidyPanel(document),
    columns,
  };
}
