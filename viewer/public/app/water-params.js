//! The carve channel, as JavaScript sees it: the water block's field order, the bank-width
//! slider's travel, the query-string mapping, the named refusals and the pond account. **No DOM,
//! no Cesium and no engine instance** -- everything here is a pure function over plain numbers
//! and plain records, exactly as `peak-params.js` is, which is why
//! `viewer/test/water-params.test.mjs` can hold it against the real wasm without a browser.
//
// # What the carve is, and why it has two phases
//
// Plan 2b cuts a bake's river channels into the ground. Ruling C-1 makes that two phases: the
// bake reads the BARE ground (a bake over carved ground would read the cut it is about to make),
// and a carved world is the same world built again from the same parameters plus a water block
// and a held bake. `wb_world_new_water` is that second phase; `wb_hydro_bake` with the 13-word
// params buffer (word 12 = 1, `WB_HYDRO_PARAMS_CARVE_STRIDE`) is the first. `carve-session.js`
// runs the two in order; this module holds the numbers and words they are made of.
//
// # There is no water default literal anywhere in the viewer
//
// The bank-width slider is anchored on the engine's own `WaterParams::canonical()`, read across
// the boundary through `wb_water_preset` at boot. **Nothing in `viewer/` writes the canonical
// bank width down, including this comment** -- the islands slice's stale density in a comment is
// the reason for that last clause. If you want to know it, ask `wb_water_preset`.
//
// # The one number this module does mirror: the domain ceiling
//
// `BANK_WIDTHS_CEILING` is `water::layer::MAX_BANK_WIDTHS`, the width the index is built to. It is
// a DOMAIN bound, not a preset value -- the same standing `DENSITY_STEPS` has in `peak-params.js`
// for `WB_MAX_PEAK_DENSITY` -- and the slider's travel cannot be stated without it, because no
// export reports it. It is pinned two ways by `water-params.test.mjs`: against the constant's own
// declaration in `layer.rs`, and against the engine itself (the ceiling admitted, the next double
// up refused), so a drift is named rather than trusted.

/// f64 per water block, and the field names in `wasm.rs`'s `WB_WATER_BLOCK_STRIDE` order. That
/// order is the ABI -- `wb_water_preset` writes it and `wb_world_new_water` reads it -- and it is
/// `WaterParams`'s own declaration order in `water/layer.rs`. **One field today**, which makes the
/// order trivially right; `water-params.test.mjs` pins it against the struct's declaration anyway,
/// so a second field added in Rust cannot silently misalign the record written here.
export const WATER_STRIDE = 1;
export const WATER_FIELDS = ["bank_widths"];

/// `wb_water_preset` selectors, mirrored from `wasm.rs`. One: `WB_WATER_CANONICAL`. There is no
/// second named block because the one field was chosen, not tuned against a world.
export const WATER_PRESET = { canonical: 0 };

/// The ceiling on `bank_widths`, inclusive, mirrored from `water::layer::MAX_BANK_WIDTHS` (and so
/// `WB_MAX_WATER_BANK_WIDTHS`). The floor is 0, **exclusive**. See the module header.
export const BANK_WIDTHS_CEILING = 4;

/// Slider positions per channel width. Tenths, so a position `p` is `p / 10` widths, by DIVISION
/// for the reason `peakTravel` gives; the ceiling is position `BANK_WIDTHS_CEILING * 10`, and the
/// engine's canonical width must land on the lattice (asserted against the engine, not here).
export const BANK_STEPS_PER_WIDTH = 10;

/// The query-string names. `carve=1` turns the carve on; `bankWidths` is the block's one field,
/// written only when it differs from the engine's canonical AND the carve is on.
export const CARVE_PARAM = "carve";
export const WATER_PARAM_NAMES = { bank_widths: "bankWidths" };

/// The one slider's travel. `min`/`max` are integer positions. **Position 0 is absent on
/// purpose**: a zero-width bank is refused by the engine (the floor is exclusive), and a slider
/// whose travel includes a refused value is a slider that can walk the owner into a refusal.
export function waterTravel(canonical) {
  return {
    bank_widths: {
      min: 1,
      max: BANK_WIDTHS_CEILING * BANK_STEPS_PER_WIDTH,
      toValue: (position) => position / BANK_STEPS_PER_WIDTH,
      toPosition: (value) => Math.round(value * BANK_STEPS_PER_WIDTH),
      format: (value) => `${value.toFixed(1)} widths`,
      // Where the slider starts when the carve is first turned on: the engine's own value.
      canonicalPosition: Math.round(canonical.bank_widths * BANK_STEPS_PER_WIDTH),
    },
  };
}

/// The slider as a `PANEL_RANGES` row, in channel widths rather than positions, so
/// `panelFieldFaults()` can ask it the question it exists to ask: **can this slider express its
/// own default?** Run in production by `controls.js` before the slider is enabled.
export function waterPanelFields(canonical) {
  const { min, max, toValue } = waterTravel(canonical).bank_widths;
  return [{
    query: WATER_PARAM_NAMES.bank_widths,
    min: toValue(min),
    max: toValue(max),
    step: Math.abs(toValue(1) - toValue(0)),
    value: canonical.bank_widths,
  }];
}

/// A water block as a flat record in ABI order, ready for `wb_world_new_water`.
export function waterToRecord(water) {
  return WATER_FIELDS.map((name) => water[name]);
}

/// A flat record back into a named object.
export function waterFromRecord(record) {
  const out = {};
  WATER_FIELDS.forEach((name, index) => { out[name] = record[index]; });
  return out;
}

/// The water block a query string asks for, or **`null`, meaning no carve at all** -- the bare
/// world, `wb_world_new_water`'s null-block path, bit-identical to what the viewer built before
/// this channel existed.
///
/// **The carve is the switch, not the block.** Unlike the peak channel, a block equal to
/// canonical is NOT the untouched path here: canonical bank widths still cut every channel. So
/// `null` means `carve` is absent (or anything but `1`), and a `bankWidths` without a `carve` is
/// ignored -- a bank that blends nothing is not a request for anything.
///
/// A `bankWidths` that is not a finite number is ignored rather than forwarded (a typo in a
/// shared link is not a reason for a refused world); a number outside the domain IS forwarded, so
/// the engine refuses it by name -- a caller asking for something the engine declines is a
/// different thing from a caller not asking for anything.
export function waterFromParams(params, canonical) {
  if (params.get(CARVE_PARAM) !== "1") return null;
  const water = { ...canonical };
  for (const field of WATER_FIELDS) {
    const name = WATER_PARAM_NAMES[field];
    if (!params.has(name)) continue;
    const value = Number(params.get(name));
    if (!Number.isFinite(value)) continue;
    water[field] = value;
  }
  return water;
}

/// The query-string fields for the panel's state -- `water` is the block, or `null` with the
/// carve off. **An untouched panel writes no water parameter at all**: carve off is every field
/// `null`, so `nextQueryString` and `apply` drop them, a reload takes the absent path, and a saved
/// world stays bit-identical. With the carve on, `carve=1` is written and `bankWidths` only if it
/// moved off canonical.
export function waterToParams(water, canonical) {
  const out = { [CARVE_PARAM]: water === null ? null : "1" };
  for (const field of WATER_FIELDS) {
    const name = WATER_PARAM_NAMES[field];
    out[name] = water === null || water[field] === canonical[field] ? null : String(water[field]);
  }
  return out;
}

// ------------------------------------------------------------------------ the bake for carving

/// `wb_hydro_bake`'s params words, in `WB_HYDRO_PARAMS_STRIDE`'s order, then -- **only for a bake
/// for carving** -- word 12, the flag, exactly 1; then the forced-outlet pairs.
///
/// **Two layouts, told apart by length parity** (Task 5): `12 + 2n` words is an ordinary bake and
/// is untouched -- byte for byte the buffer the viewer sent before this channel existed -- and
/// `13 + 2n` words carries the flag. Without the flag the door refuses the record with
/// `WB_ERR_NOT_BAKED_FOR_CARVING`, because an ordinary record keeps the ponds its own channels
/// drain and carving with it would stand a dam across a river.
export const HYDRO_PARAMS_STRIDE = 12;
export const HYDRO_PARAMS_CARVE_STRIDE = 13;

export function hydroParamsWords(params) {
  const forced = params.forcedOutlets ?? [];
  const header = params.forCarving ? HYDRO_PARAMS_CARVE_STRIDE : HYDRO_PARAMS_STRIDE;
  const words = [
    params.totalNodes, params.wetnessNodes, params.keepDepthM, params.keepAreaM2,
    params.pondMaxAreaM2, params.streamFlowM2, params.riverFlowM2, params.greatFlowM2,
    params.notchFallM, params.evaporationFactor, params.saltFlatShare, forced.length,
  ];
  if (params.forCarving) words.push(1);
  for (const outlet of forced) words.push(outlet.latitudeDeg, outlet.longitudeDeg);
  if (words.length !== header + 2 * forced.length) {
    throw new Error(`hydro params: ${words.length} words, expected ${header + 2 * forced.length}`);
  }
  return words;
}

// ---------------------------------------------------------------------- the refusals, by name

/// Every status the carve can be refused with, and the sentence the owner is shown for it.
///
/// **Each by its own name** (Ruling C-3). The islands panel's refusal sat in a path a failed boot
/// swallowed, and the owner read "engine unavailable" for a block the engine had named precisely;
/// a status with no sentence here would be the same failure one level down. The four the plan
/// names are first, then Ruling C-36's (a carved world's water asked through another bake); the
/// two after them are what the door says about a buffer or a bake id the studio itself got wrong,
/// and are named for the same reason.
export const CARVE_REFUSALS = {
  5: ["WB_ERR_PARAM", `the water block is malformed: the bank width must be above 0 and at most `
    + `${BANK_WIDTHS_CEILING} channel widths (check bankWidths in the address). Nothing was `
    + `baked.`],
  8: ["WB_ERR_WRONG_WORLD", "the held bake is from another world: the ground changed after the "
    + "rivers were baked, and a record only carves the ground it was baked from."],
  9: ["WB_ERR_NOT_BAKED_FOR_CARVING", "the held bake was not baked for carving: an ordinary "
    + "record keeps the ponds its own channels drain, and carving with it would dam the rivers."],
  10: ["WB_ERR_CARVED", "a bake was asked of a carved world: bakes read the bare ground, and this "
    + "ground has already been cut."],
  11: ["WB_ERR_NOT_CARVED_FROM", "the carved world's water was asked through a bake it was not "
    + "carved from: a carved world answers only against its own carving bake."],
  1: ["WB_ERR_HANDLE", "the held bake is gone (freed, or never issued in this engine)."],
  2: ["WB_ERR_BUFFER", "the water block could not be read (a viewer bug, not a setting)."],
};

/// The named refusal for a status: `{ status, name, text }`, where `text` begins with the name so
/// a note that shows only `text` still names it. An unlisted status is named by its number rather
/// than dropped.
export function carveRefusal(status) {
  const [name, why] = CARVE_REFUSALS[status] ?? [`status ${status}`, "the engine refused the carve."];
  return { status, name, text: `${name}: ${why}` };
}

// ---------------------------------------------------------------------------- the pond account

/// **What turning the carve on does to the owner's ponds**, counted from the two actual records.
///
/// A bake for carving drains every fine-found pond a channel crosses below its level (Ruling
/// C-16 / C-20) -- in the carved world the channel is cut beneath it, so it is not a hollow -- and
/// the pond search then keeps other finds in the density cells those freed. So the carved world's
/// pond set is not the ordinary one: some are gone and some are new. That is correct and it must
/// not happen silently, so the studio counts it rather than asserting it.
///
/// `ordinary` and `carving` are `decodeHydro` results baked from the same ground with the same
/// params, one without the flag and one with it. A pond is the fine-found tail of `bodies` (the
/// last `header.pondsKept` entries, which `record.rs` appends after every coarse body), and one
/// pond is the same pond in both records when its anchor and level are the same doubles -- the
/// bake is deterministic, so a pond neither rule touched is written identically.
///
/// Returns `{ before, after, drained, arrived, kept }`: `drained` are in the ordinary record and
/// not the carved one; `arrived` are the reverse; `kept` are in both.
///
/// **Or `null` when the two were baked from different ground** -- their headers' `ground`
/// fingerprints differ. The ordinary record comes from a pool worker baking from params alone, and
/// Ruling C-28 declined to reuse the preview's bake precisely because nothing fingerprinted that
/// comparison: counted across two grounds, every pond would read as drained and every other as new.
/// No account is better than a wrong one, and the fingerprint makes the check one comparison.
export function pondChange(ordinary, carving) {
  if (ordinary.header.ground !== carving.header.ground) return null;
  const key = (body) => `${body.anchor[0]},${body.anchor[1]},${body.levelM}`;
  const ponds = (decoded) => {
    const n = decoded.header.pondsKept;
    return decoded.bodies.slice(decoded.bodies.length - n).map(key);
  };
  const before = ponds(ordinary);
  const after = ponds(carving);
  const afterSet = new Set(after);
  const beforeSet = new Set(before);
  const kept = before.filter((k) => afterSet.has(k)).length;
  return {
    before: before.length,
    after: after.length,
    drained: before.filter((k) => !afterSet.has(k)).length,
    arrived: after.filter((k) => !beforeSet.has(k)).length,
    kept,
  };
}

/// The sentence the owner is shown about their ponds when the carve is baked.
export function pondChangeText(change) {
  if (change.drained === 0 && change.arrived === 0) {
    return `carving changes no pond: all ${change.before} are kept where they were.`;
  }
  return `carving changes your ponds: ${change.drained} of ${change.before} are drained (a channel `
    + `is cut beneath them), ${change.arrived} others are found in the cells they freed, and `
    + `${change.kept} are untouched — ${change.after} ponds in the carved world. Turn the carve `
    + `off to get the ordinary set back.`;
}
