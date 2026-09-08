//! One colour table for area pins, so nothing on the globe disagrees with anything else.
//
// **Faction decides the fill, race decides the ring.** Hostile ground has to read as
// dangerous from orbit at a glance - that is the whole point of watching a world populate -
// so every hostile place is red whatever lives there. But "a raider town" and "a raider
// town of lizard folk" are different facts and the second is worth keeping, so the race
// goes to the outline. A hostile settlement is a red dot ringed in its people's colour; a
// wild hunting ground is red ringed in nothing.
//
// **The colours are chosen against the globe they sit on, not against each other on paper.**
// Aetosia is blue water and green land, so the two colours a map most wants - blue and
// green - are the two least visible ones. Human blue and elf green are kept because they
// are what anybody expects, and they are lifted well clear of the terrain's own blues and
// greens rather than being true to them.

/// Race colours. Eleven, and every one has to survive being a nine-pixel dot.
export const RACE_COLOURS = {
  human:    "#5aa9f0",   // blue, as asked
  elf:      "#5fd08a",   // green, as asked
  gnome:    "#f2cf45",   // yellow, as asked
  dwarf:    "#d2793a",   // copper - forges and mountain ore
  halfling: "#e8c98a",   // wheat - farmland and river valleys
  volgrin:  "#9fb4cc",   // steel - open country, big frames
  saurathi: "#35c8b2",   // jade - the swamps they prefer
  valran:   "#c05f3c",   // brick - rugged, weathered, hard country
  aethari:  "#a88ce6",   // violet - libraries and observatories
  felari:   "#f0a850",   // amber - and the brand's own accent
  lunari:   "#cfe0f2",   // moonlight - the pack, the shifting
};

/// Everything hostile, whoever lives there.
export const HOSTILE = "#e2564a";

/// Wild ground with no settlement: darker, so a camp and a beast range read apart.
export const WILD = "#a8342a";

/// Neutral settlements that belong to nobody on the race list.
export const NEUTRAL = "#e0a94a";

/// Friendly settlements with no race recorded.
export const FRIENDLY = "#7fc9a6";

function hostile(area) {
  return (area.faction || "").toLowerCase() === "hostile";
}

function hunting(area) {
  return (area.purpose || "").toLowerCase() === "hunting";
}

/// The fill for one area.
export function fillFor(Cesium, area) {
  if (hunting(area)) return Cesium.Color.fromCssColorString(WILD);
  if (hostile(area)) return Cesium.Color.fromCssColorString(HOSTILE);
  const race = (area.race || "").toLowerCase();
  if (RACE_COLOURS[race]) return Cesium.Color.fromCssColorString(RACE_COLOURS[race]);
  if ((area.faction || "").toLowerCase() === "neutral") {
    return Cesium.Color.fromCssColorString(NEUTRAL);
  }
  return Cesium.Color.fromCssColorString(FRIENDLY);
}

/// The ring for one area: its people's colour where the fill could not carry it.
///
/// A black ring on everything else, which is what keeps a pale dot legible over cloud and
/// a dark one legible over deep water.
export function outlineFor(Cesium, area) {
  const race = (area.race || "").toLowerCase();
  if ((hostile(area) || hunting(area)) && RACE_COLOURS[race]) {
    return Cesium.Color.fromCssColorString(RACE_COLOURS[race]);
  }
  return Cesium.Color.BLACK.withAlpha(0.85);
}

/// A hostile place gets a heavier ring, so danger reads at a glance and at small size.
export function outlineWidthFor(area) {
  return hostile(area) || hunting(area) ? 3 : 2;
}

/// The legend, as rows a panel can draw. Order reads: danger first, then peoples.
export function legendRows() {
  const rows = [
    { label: "hostile", colour: HOSTILE },
    { label: "hunting ground", colour: WILD },
  ];
  for (const [race, colour] of Object.entries(RACE_COLOURS)) {
    rows.push({ label: race, colour });
  }
  return rows;
}
