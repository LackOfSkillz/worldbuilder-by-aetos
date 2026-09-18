//! The hydrology bake's flat wire form: every field of `HydroRecord`, in one order, as `f64`
//! words. See the record layout table in `.superpowers/sdd/2026-09-10-water-1a-coarse-bake/
//! task-9-brief.md` -- the order there is the contract this file writes and reads back.
//!
//! Enums and "no value" are exact `f64` values (0.0, 1.0, 2.0, ... and `-1.0` for none), never
//! derived from an arbitrary float by a truncating cast. `word_to_u32` is the one checked path
//! from a word back to an index or count: finite, non-negative, integral and in range, or the
//! whole record is refused.
//!
//! Two words share a position and not a meaning, and two flags share a name and not a meaning:
//!
//! - **Word 3 of a reach point** (`lat, lon, bed_m, width_m, depth_m, flow_m2`) is the **bed**:
//!   the water surface there minus the channel's depth (at a mouth, the water it runs into).
//! - **Word 3 of a notch point** (`lat, lon, surface_m, width_m`) is the **cut surface**: the
//!   lowered ground, which is the water surface through the cut. It is not a bed below that
//!   (Ruling F-2). Where a notch point and a reach point sit on the same node, `notch word 3 -
//!   reach depth == reach bed`, and the two widths are equal (both on the caller's params).
//! - **Ruling F-3:** a notch line splits wherever two consecutive points are not graph
//!   neighbours. **Ruling F-3a:** a line that keeps its route's last lowered node ends with one
//!   extra point, `receiver` of that node -- the water it drains into, an earlier cut's
//!   committed node, or lower ground the cut walked over without lowering (a route's `nodes`
//!   lists only what it lowered, so `receiver` of the last one is not always water). That
//!   point's word 3 is its own current water surface: 0.0 for the ocean, the lake's `level_m`
//!   for a lake member, or `routing.surface_m` otherwise.
//! - **Body `fresh`** means "not closed": the lake has an outlet. Its water may still end in a
//!   closed lake downstream rather than the sea.
//! - **A body's `shore_member_count`** (SCHEMA 6, plan 1b-4) is the wire discriminator spec §8.3
//!   needs: `0` means `outline` is a traced curve, not a shore-point set (Ruling T1-2), and that
//!   is how a pond, and a fine-search lake, are told apart from a coarse body. A coarse body
//!   ships a positive count and an outline of shore members then collar (Ruling E-1); the count
//!   never exceeds that outline's length, and `decode` refuses a record where it does, because
//!   every consumer slices the outline on it.
//! - **Reach `fresh`** means "its chain reaches the ocean": following its `downstream` through
//!   reaches and bodies ends at `Ocean`, not at a closed lake's `Sink`.

use blake2::digest::{Update, VariableOutput};
use blake2::Blake2bVar;

use crate::detmath as m;
use crate::hydrology::reaches::{Downstream, ReachClass};
use crate::hydrology::{BakeStats, Body, BodyKind, Fall, HydroRecord, NotchLine, ReachLine, ReachPoint};
use crate::sphere::SpherePoint;
use crate::surface::Surface;
use crate::vectors::Vec3;

/// 7.0 as of Task 1 (plan 2b): the record carries a fingerprint of the ground it was baked from
/// ([`ground_fingerprint`]). The header grows from 56 to **60** words: the 16-byte digest is
/// appended after `collar_points` as four words (56-59), each an exact integer in `0..=u32::MAX`
/// holding four digest bytes little-endian -- bytes 0-3 in word 56, 12-15 in word 59. Words 0-55
/// keep their meaning and position; everything after the header (the first body, reach, notch,
/// fall) starts four words later. `decode` reads the digest back but does not yet compare it to
/// anything -- refusing a record read against a foreign world is the next task's.
///
/// Why sample rather than declare: the engine's other on-disk record, the stream graph
/// (`streamfmt.rs`), has carried the world's radius at `OFF_RADIUS_M` all along, and
/// `GraphReader::open` checks it only for finiteness -- the versions are the only thing it
/// compares -- so a graph written at 9,309 km opens silently beside a 6,371 km world. A declared
/// field nothing checks is what we already had.
///
/// 6.0 as of Task 1 (plan 1b-4): the discriminator that spec §8.3 already needs -- a body with
/// `shore_member_count == 0` carries a traced curve, everything else a shore-point set -- had to
/// reach the wire before Task 2 could put any points behind it. Doing it the other way round
/// would ship a record whose lakes are silently readable as polygons the moment a coarse outline
/// stopped being empty, which is exactly what spec §7 forbids (see the plan's "one hard ordering
/// constraint"). The header grows from 54 to 56 words, adding `shore_members` and
/// `collar_points` after `pond_density_area_m2`; the body grows from 14 to 16 fixed words,
/// adding `shore_member_count` and `shore_reach_m` after `downstream_id` and before
/// `outline_len`. Every body ships both new fields as zero until Task 2 fills them.
///
/// 5.0 as of Task 3 (plan 1b-3): the header grew from 43 to 45 words, adding the crossing pass's
/// two counts after `meander_max_slope` -- `crossings_coarse` (word 43, how many crossings the
/// coarse record already had, which Ruling S-2 keeps) and `crossings_left` (word 44, how many are
/// left in the record as it ships, after the pass, the meander and simplification).
///
/// Task 5 of the same plan grew it again, from 45 to **54**, without a schema bump: the nine
/// words 45-53 are the fine pond search's two counts (`ponds_found`, `ponds_kept`) and its seven
/// params (`pond_cell_m`, `pond_search_radius_m`, `pond_keep_depth_m`, `pond_keep_area_m2`,
/// `pond_wetness_share`, `pond_max_slope`, `pond_density_area_m2`). The 45-word SCHEMA 5 existed
/// only between two tasks of one unmerged plan branch, so no record of that shape was ever
/// written anywhere a reader could find it, and there is nothing for `decode` to refuse.
///
/// SCHEMA 4 (Task 3 of plan 1b-2) grew the header from 32 to 43 words, adding eleven words after
/// `forced_matched` -- three counts of what capped basins keep (`capped_basins`, `capped_inner`,
/// `capped_inner_kept`, carry-forward I3) and an echo of the eight refinement params
/// (`refine_step_m`, `refine_simplify_m`, `refine_vertical_m`, `fall_min_drop_m`,
/// `fall_max_run_m`, `meander_wavelength_widths`, `meander_amplitude_widths`,
/// `meander_max_slope`). Earlier schemas are refused outright -- `decode` never adapts an old
/// record to the new shape.
pub const SCHEMA: f64 = 7.0;

/// Words in the header: everything up to and including the ground fingerprint.
pub const HEADER_WORDS: usize = 60;

/// How many points of ground [`ground_fingerprint`] samples.
pub const PROBE_COUNT: usize = 64;

/// Bytes in a ground fingerprint, and on the wire four `u32` words of four bytes each.
pub const GROUND_BYTES: usize = 16;

/// The same four bytes of a digest as one exact integer word, little-endian.
fn ground_words(ground: &[u8; GROUND_BYTES]) -> [f64; 4] {
    let mut out = [0.0; 4];
    for (word, chunk) in out.iter_mut().zip(ground.chunks_exact(4)) {
        let bytes = [chunk[0], chunk[1], chunk[2], chunk[3]];
        *word = f64::from(u32::from_le_bytes(bytes));
    }
    out
}

/// SplitMix64's finaliser: a cheap, well-mixed integer hash, all of it wrapping integer
/// arithmetic, so it gives the same bits on every host and every target.
fn mix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// A fraction in `[0, 1)` from the top 53 bits of an integer hash: `2^-53` times an integer
/// below `2^53`, which is exact in an `f64`, so no rounding step exists to differ between hosts.
fn unit_fraction(h: u64) -> f64 {
    const TWO_POW_MINUS_53: f64 = 1.0 / 9_007_199_254_740_992.0;
    // Not a float truncation: `h >> 11` is an integer below 2^53, which an `f64` holds exactly.
    (h >> 11) as f64 * TWO_POW_MINUS_53
}

/// Probe `index` of the fingerprint's fixed scatter, uniform over the sphere by area.
///
/// **Integers first, floats last.** The scatter is two integer hashes of the index -- not a float
/// sequence (a golden-angle spiral, say), where each point's rounding feeds the next and two hosts
/// can drift apart until every record looks foreign. Each hash becomes a fraction exactly (see
/// [`unit_fraction`]), and the only float maths is one `sqrt`, one `sin` and one `cos` per point,
/// all through `detmath`, whose pure-Rust `libm` gives the same bits natively and in wasm.
///
/// **Irregular on purpose.** A regular grid can land every probe on ground two different worlds
/// happen to share -- an abyssal plain, a shelf at the same depth -- and call them the same.
pub fn probe_point(index: usize) -> SpherePoint {
    // A fixed, arbitrary salt, so probe 0 is not the hash of zero.
    const SALT: u64 = 0x6772_6f75_6e64_7631; // "groundv1"
    let i = index as u64; // cast-ok: usize to u64 widens on every target this crate builds for
    let a = mix64(SALT ^ i.wrapping_mul(0x9e37_79b9_7f4a_7c15));
    let b = mix64(a ^ SALT.rotate_left(17));
    // z uniform in [-1, 1) and longitude uniform in [0, 2pi) is uniform by area (Archimedes).
    let z = 2.0 * unit_fraction(a) - 1.0;
    let longitude = 2.0 * std::f64::consts::PI * unit_fraction(b);
    let across = m::sqrt(1.0 - z * z);
    SpherePoint { vector: Vec3 { x: across * m::cos(longitude), y: across * m::sin(longitude), z } }
}

/// A 16-byte digest of the ground a bake read: [`PROBE_COUNT`] samples of
/// `Surface::bake_ground_m` at canonical resolution, at [`probe_point`]s, each rounded to the
/// millimetre, hashed with BLAKE2b.
///
/// **The ground the bake read, detail included -- not `structural_m`, and not raw
/// `elevation_m`.** Both look natural and both are wrong:
///
/// - `structural_m` leaves detail out, but the bake reads detail: the fine pond search finds its
///   ponds in `elevation_m` at `pond_cell_m` (Ruling S-9). A digest without detail would accept
///   a world differing only in its relief block as the same ground, and answer with ponds found
///   in other ground. It also barely sees a radius change -- the landform is laid out by
///   direction, and on the test world 55 of 64 probes read `structural_m` bit-identically at
///   9,309 km and 6,371 km, where detail's wavelengths are fixed in metres and move them all.
/// - raw `elevation_m` will, once plan 2b's carve lands, include the water layer the record
///   itself writes, making the record's fingerprint depend on the record -- circular.
///
/// So it reads `bake_ground_m`: elevation with the water layer skipped and nothing else skipped,
/// the same function the pond search reads through, so the two cannot drift apart. At `None`,
/// every configured octave down to the canonical floor -- deterministic, and what physics asks.
///
/// **Rounded to the millimetre before hashing**, so a bit-level difference with no physical
/// meaning does not make a record foreign. The rounded value is an integer-valued `f64` (exact
/// for any ground within 2^53 mm, about nine billion km, of datum), hashed as its bits: there is
/// no cast, so nothing truncates. `floor(x + 0.5)` never yields `-0.0` (the sum is never `-0.0`),
/// so zero has one encoding.
///
/// **Hashed with BLAKE2b at a 16-byte output**, the one runtime hash this crate already uses
/// (`generation.rs`); BLAKE2 mixes the output length into its initial state, so this is its own
/// hash and not a prefix of a longer one. A version tag leads the message, so a later probe
/// scheme cannot collide with this one by construction.
pub fn ground_fingerprint(surface: &Surface) -> [u8; GROUND_BYTES] {
    let mut hasher = Blake2bVar::new(GROUND_BYTES).expect("16 is a valid BLAKE2b output length");
    hasher.update(b"worldbuilder hydro ground v1");
    for index in 0..PROBE_COUNT {
        let metres = surface.bake_ground_m(&probe_point(index), None);
        let millimetres = m::floor(metres * 1000.0 + 0.5);
        hasher.update(&millimetres.to_bits().to_le_bytes());
    }
    let mut out = [0u8; GROUND_BYTES];
    hasher.finalize_variable(&mut out).expect("output buffer is exactly 16 bytes");
    out
}

fn word_to_u32(w: f64) -> Option<u32> {
    if w.is_finite() && w >= 0.0 && w <= u32::MAX as f64 && m::floor(w) == w {
        Some(w as u32) // cast-ok: checked finite, non-negative, integral and in range above
    } else {
        None
    }
}

/// `-1.0` decodes to `None`; anything else must be a valid index word.
fn word_to_optional_u32(w: f64) -> Option<Option<u32>> {
    if w == -1.0 {
        Some(None)
    } else {
        word_to_u32(w).map(Some)
    }
}

fn word_to_bool(w: f64) -> Option<bool> {
    if w == 0.0 {
        Some(false)
    } else if w == 1.0 {
        Some(true)
    } else {
        None
    }
}

fn bool_word(b: bool) -> f64 {
    if b { 1.0 } else { 0.0 }
}

fn body_kind_word(kind: BodyKind) -> f64 {
    match kind {
        BodyKind::Lake => 0.0,
        BodyKind::Pond => 1.0,
        BodyKind::SaltLake => 2.0,
        BodyKind::SaltFlat => 3.0,
    }
}

fn word_to_body_kind(w: f64) -> Option<BodyKind> {
    if w == 0.0 {
        Some(BodyKind::Lake)
    } else if w == 1.0 {
        Some(BodyKind::Pond)
    } else if w == 2.0 {
        Some(BodyKind::SaltLake)
    } else if w == 3.0 {
        Some(BodyKind::SaltFlat)
    } else {
        None
    }
}

fn reach_class_word(class: ReachClass) -> f64 {
    match class {
        ReachClass::Stream => 0.0,
        ReachClass::River => 1.0,
        ReachClass::Great => 2.0,
    }
}

fn word_to_reach_class(w: f64) -> Option<ReachClass> {
    if w == 0.0 {
        Some(ReachClass::Stream)
    } else if w == 1.0 {
        Some(ReachClass::River)
    } else if w == 2.0 {
        Some(ReachClass::Great)
    } else {
        None
    }
}

/// `(kind, id)`, `id` being `-1.0` for `Ocean`/`Sink`.
fn downstream_words(downstream: Downstream) -> (f64, f64) {
    match downstream {
        Downstream::Reach(id) => (0.0, id as f64),
        Downstream::Body(id) => (1.0, id as f64),
        Downstream::Ocean => (2.0, -1.0),
        Downstream::Sink => (3.0, -1.0),
    }
}

fn words_to_downstream(kind: f64, id: f64) -> Option<Downstream> {
    if kind == 0.0 {
        word_to_u32(id).map(Downstream::Reach)
    } else if kind == 1.0 {
        word_to_u32(id).map(Downstream::Body)
    } else if kind == 2.0 {
        if id == -1.0 { Some(Downstream::Ocean) } else { None }
    } else if kind == 3.0 {
        if id == -1.0 { Some(Downstream::Sink) } else { None }
    } else {
        None
    }
}

/// A cursor over the word slice: every read can run off the end, and every caller propagates
/// that as `None` rather than panicking on a short or truncated record.
struct Reader<'a> {
    words: &'a [f64],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(words: &'a [f64]) -> Self {
        Self { words, pos: 0 }
    }

    fn word(&mut self) -> Option<f64> {
        let w = *self.words.get(self.pos)?;
        self.pos += 1;
        Some(w)
    }

    fn u32(&mut self) -> Option<u32> {
        word_to_u32(self.word()?)
    }

    fn optional_u32(&mut self) -> Option<Option<u32>> {
        word_to_optional_u32(self.word()?)
    }

    fn boolean(&mut self) -> Option<bool> {
        word_to_bool(self.word()?)
    }

    fn remaining(&self) -> usize {
        self.words.len() - self.pos
    }
}

/// Ruling 1 (fix round 1): before trusting a decoded count to size a `Vec::with_capacity` or
/// drive a loop, check it against the words actually left in the record. `min_words` is the
/// fewest words one item of that kind can occupy (a body's outline, a reach's points, etc. add
/// more on top, but every item needs at least this many); `count * min_words` overflowing or
/// exceeding what remains means the count is bogus, so the whole record is refused instead of
/// allocating on it.
///
/// Unchecked: each call site's `min_words` literal (16 for a body, 7 for a reach, 1 for a
/// notch, 4 for a fall, and the nested per-point minimums) is not tied to the fixed reads its
/// own loop performs below it by anything the compiler enforces -- it holds only because the
/// comment above each call site is kept in sync by hand with that loop's field list. Widening a
/// record's fixed fields without raising the matching literal here would undercount, not
/// overcount, so a corrupt record with an inflated count could pass this gate and only fail
/// (safely, via `r.word()?`'s own bounds check) partway through decoding it.
fn count_fits(count: usize, min_words: usize, remaining: usize) -> bool {
    match count.checked_mul(min_words) {
        Some(need) => need <= remaining,
        None => false,
    }
}

pub fn encode(record: &HydroRecord) -> Vec<f64> {
    let mut out = Vec::new();
    let stats = &record.stats;

    out.push(SCHEMA);
    out.push(record.bodies.len() as f64);
    out.push(record.reaches.len() as f64);
    out.push(record.notches.len() as f64);
    out.push(record.falls.len() as f64);
    out.push(stats.nodes as f64);
    out.push(stats.land_nodes as f64);
    out.push(stats.hollows as f64);
    out.push(stats.kept as f64);
    out.push(stats.notched as f64);
    out.push(stats.closed as f64);
    out.push(stats.streams as f64);
    out.push(stats.rivers as f64);
    out.push(stats.great as f64);
    out.push(stats.max_order as f64);
    out.push(stats.bifurcation_min);
    out.push(stats.bifurcation_max);
    out.push(stats.stream_flow_m2);
    out.push(stats.river_flow_m2);
    out.push(stats.great_flow_m2);
    out.push(stats.total_nodes as f64);
    out.push(stats.wetness_nodes as f64);
    out.push(stats.keep_depth_m);
    out.push(stats.keep_area_m2);
    out.push(stats.pond_max_area_m2);
    out.push(stats.keep_max_area_m2);
    out.push(stats.min_stream_nodes);
    out.push(stats.notch_fall_m);
    out.push(stats.evaporation_factor);
    out.push(stats.salt_flat_share);
    out.push(stats.forced_requested as f64);
    out.push(stats.forced_matched as f64);
    out.push(stats.capped_basins as f64);
    out.push(stats.capped_inner as f64);
    out.push(stats.capped_inner_kept as f64);
    out.push(stats.refine_step_m);
    out.push(stats.refine_simplify_m);
    out.push(stats.refine_vertical_m);
    out.push(stats.fall_min_drop_m);
    out.push(stats.fall_max_run_m);
    out.push(stats.meander_wavelength_widths);
    out.push(stats.meander_amplitude_widths);
    out.push(stats.meander_max_slope);
    out.push(stats.crossings_coarse as f64);
    out.push(stats.crossings_left as f64);
    out.push(stats.ponds_found as f64);
    out.push(stats.ponds_kept as f64);
    out.push(stats.pond_cell_m);
    out.push(stats.pond_search_radius_m);
    out.push(stats.pond_keep_depth_m);
    out.push(stats.pond_keep_area_m2);
    out.push(stats.pond_wetness_share);
    out.push(stats.pond_max_slope);
    out.push(stats.pond_density_area_m2);
    out.push(stats.shore_members as f64);
    out.push(stats.collar_points as f64);
    out.extend_from_slice(&ground_words(&record.ground));

    for body in &record.bodies {
        out.push(body.id as f64);
        out.push(body_kind_word(body.kind));
        out.push(bool_word(body.fresh));
        out.push(bool_word(body.enclosed));
        out.push(bool_word(body.forced));
        out.push(body.level_m);
        out.push(body.area_m2);
        out.push(body.depth_m);
        out.push(match body.outlet_reach {
            Some(id) => id as f64,
            None => -1.0,
        });
        out.push(body.anchor.0);
        out.push(body.anchor.1);
        let (downstream_kind, downstream_id) = downstream_words(body.downstream);
        out.push(downstream_kind);
        out.push(downstream_id);
        out.push(body.shore_member_count as f64);
        out.push(body.shore_reach_m);
        out.push(body.outline.len() as f64);
        for &(lat, lon) in &body.outline {
            out.push(lat);
            out.push(lon);
        }
    }

    for reach in &record.reaches {
        out.push(reach.id as f64);
        out.push(reach_class_word(reach.class));
        out.push(reach.order as f64);
        let (kind, id) = downstream_words(reach.downstream);
        out.push(kind);
        out.push(id);
        out.push(bool_word(reach.fresh));
        out.push(reach.points.len() as f64);
        for point in &reach.points {
            out.push(point.lat_deg);
            out.push(point.lon_deg);
            out.push(point.bed_m);
            out.push(point.width_m);
            out.push(point.depth_m);
            out.push(point.flow_m2);
        }
    }

    for notch in &record.notches {
        out.push(notch.points.len() as f64);
        for &(lat, lon, surface_m, width_m) in &notch.points {
            out.push(lat);
            out.push(lon);
            out.push(surface_m);
            out.push(width_m);
        }
    }

    for fall in &record.falls {
        out.push(fall.reach as f64);
        out.push(fall.at.0);
        out.push(fall.at.1);
        out.push(fall.height_m);
    }

    out
}

pub fn decode(words: &[f64]) -> Option<HydroRecord> {
    let mut r = Reader::new(words);

    let schema = r.word()?;
    if schema != SCHEMA {
        return None;
    }
    let body_count = r.u32()? as usize;
    let reach_count = r.u32()? as usize;
    let notch_count = r.u32()? as usize;
    let fall_count = r.u32()? as usize;
    let stats = BakeStats {
        nodes: r.u32()?,
        land_nodes: r.u32()?,
        hollows: r.u32()?,
        kept: r.u32()?,
        notched: r.u32()?,
        closed: r.u32()?,
        streams: r.u32()?,
        rivers: r.u32()?,
        great: r.u32()?,
        max_order: r.u32()?,
        bifurcation_min: r.word()?,
        bifurcation_max: r.word()?,
        stream_flow_m2: r.word()?,
        river_flow_m2: r.word()?,
        great_flow_m2: r.word()?,
        total_nodes: r.u32()?,
        wetness_nodes: r.u32()?,
        keep_depth_m: r.word()?,
        keep_area_m2: r.word()?,
        pond_max_area_m2: r.word()?,
        keep_max_area_m2: r.word()?,
        min_stream_nodes: r.word()?,
        notch_fall_m: r.word()?,
        evaporation_factor: r.word()?,
        salt_flat_share: r.word()?,
        forced_requested: r.u32()?,
        forced_matched: r.u32()?,
        capped_basins: r.u32()?,
        capped_inner: r.u32()?,
        capped_inner_kept: r.u32()?,
        refine_step_m: r.word()?,
        refine_simplify_m: r.word()?,
        refine_vertical_m: r.word()?,
        fall_min_drop_m: r.word()?,
        fall_max_run_m: r.word()?,
        meander_wavelength_widths: r.word()?,
        meander_amplitude_widths: r.word()?,
        meander_max_slope: r.word()?,
        crossings_coarse: r.u32()?,
        crossings_left: r.u32()?,
        ponds_found: r.u32()?,
        ponds_kept: r.u32()?,
        pond_cell_m: r.word()?,
        pond_search_radius_m: r.word()?,
        pond_keep_depth_m: r.word()?,
        pond_keep_area_m2: r.word()?,
        pond_wetness_share: r.word()?,
        pond_max_slope: r.word()?,
        pond_density_area_m2: r.word()?,
        shore_members: r.u32()?,
        collar_points: r.u32()?,
    };
    // SCHEMA 7: the ground fingerprint, four exact u32 words of four little-endian bytes each.
    // Read back, not compared: refusing a record against a foreign world is the next task's.
    let mut ground = [0u8; GROUND_BYTES];
    for chunk in ground.chunks_exact_mut(4) {
        chunk.copy_from_slice(&r.u32()?.to_le_bytes());
    }

    // Body: id, kind, fresh, enclosed, forced, level_m, area_m2, depth_m, outlet_reach,
    // anchor_lat, anchor_lon, downstream_kind, downstream_id, shore_member_count, shore_reach_m,
    // outline_len -- 16 words, plus its outline.
    if !count_fits(body_count, 16, r.remaining()) {
        return None;
    }
    let mut bodies = Vec::with_capacity(body_count);
    for _ in 0..body_count {
        let id = r.u32()?;
        let kind = word_to_body_kind(r.word()?)?;
        let fresh = r.boolean()?;
        let enclosed = r.boolean()?;
        let forced = r.boolean()?;
        let level_m = r.word()?;
        let area_m2 = r.word()?;
        let depth_m = r.word()?;
        let outlet_reach = r.optional_u32()?;
        let anchor_lat = r.word()?;
        let anchor_lon = r.word()?;
        let downstream_kind = r.word()?;
        let downstream_id = r.word()?;
        let downstream = words_to_downstream(downstream_kind, downstream_id)?;
        let shore_member_count = r.u32()?;
        let shore_reach_m = r.word()?;
        // `shore_reach_m` is the one float on the wire a consumer uses as a distance: §8.3's
        // second clause puts a point inside the body when it is within `shore_reach_m` of a
        // shore member. A NaN makes every comparison false and a body vanish; an infinity (or
        // any negative value, which is not a length at all) makes the test meaningless in the
        // other direction -- an infinite band admits the whole planet as inside one lake, and
        // reports it as a fact rather than a decode failure. Refuse it here, at the trust
        // boundary, where the failure is still a `None`.
        if !(shore_reach_m.is_finite() && shore_reach_m >= 0.0) {
            return None;
        }
        let outline_len = r.u32()? as usize;
        // The extent invariant (Ruling E-1): the outline's first `shore_member_count` points
        // are the shore members and the rest are the collar, so the count can never exceed the
        // outline's length. It is not enough that it is a valid u32 -- every consumer slices on
        // it (`body.outline[..shore_member_count]` for the members, the remainder for the
        // collar, and `outline.len() - shore_member_count` for the collar's size), so a count
        // past the end panics the first reader in debug and wraps the collar size to about 4
        // billion in release. Refused here, in the same guard family as `count_fits`, so no
        // consumer downstream has to re-check it.
        if shore_member_count as usize > outline_len {
            return None;
        }
        // Ruling Q-15: a shore-point set with **no collar** is refused, not merely improbable.
        // §8.3's first clause admits a point when its nearest member is at least as near as its
        // nearest *collar* point; with no collar `dc` is infinite, the clause is vacuously true
        // everywhere, and that one body claims the whole planet -- with nothing but the index's
        // bounding circle left to bound the answer. `extent.rs` cannot write one (a shore member
        // is a member precisely because it has a non-member neighbour), so this refuses a record
        // no bake produces and every consumer downstream may assume a collar exists. A pond is
        // untouched: its count is zero, not equal to its outline's length.
        if shore_member_count > 0 && shore_member_count as usize == outline_len {
            return None;
        }
        // Outline pair: lat, lon -- 2 words per point.
        if !count_fits(outline_len, 2, r.remaining()) {
            return None;
        }
        let mut outline = Vec::with_capacity(outline_len);
        for _ in 0..outline_len {
            let lat = r.word()?;
            let lon = r.word()?;
            outline.push((lat, lon));
        }
        bodies.push(Body {
            id,
            kind,
            fresh,
            enclosed,
            forced,
            level_m,
            area_m2,
            depth_m,
            outlet_reach,
            anchor: (anchor_lat, anchor_lon),
            outline,
            downstream,
            shore_member_count,
            shore_reach_m,
        });
    }

    // Reach: id, class, order, downstream_kind, downstream_id, fresh, point_count -- 7 words,
    // plus its points.
    if !count_fits(reach_count, 7, r.remaining()) {
        return None;
    }
    let mut reaches = Vec::with_capacity(reach_count);
    for _ in 0..reach_count {
        let id = r.u32()?;
        let class = word_to_reach_class(r.word()?)?;
        let order = r.u32()?;
        let downstream_kind = r.word()?;
        let downstream_id = r.word()?;
        let downstream = words_to_downstream(downstream_kind, downstream_id)?;
        let fresh = r.boolean()?;
        let point_count = r.u32()? as usize;
        // Reach point: lat, lon, bed_m (surface minus depth -- see the module doc), width_m,
        // depth_m, flow_m2 -- 6 words per point.
        if !count_fits(point_count, 6, r.remaining()) {
            return None;
        }
        let mut points = Vec::with_capacity(point_count);
        for _ in 0..point_count {
            let lat_deg = r.word()?;
            let lon_deg = r.word()?;
            let bed_m = r.word()?;
            let width_m = r.word()?;
            let depth_m = r.word()?;
            let flow_m2 = r.word()?;
            points.push(ReachPoint { lat_deg, lon_deg, bed_m, width_m, depth_m, flow_m2 });
        }
        reaches.push(ReachLine { id, class, order, downstream, fresh, points });
    }

    // Notch: point_count -- 1 word, plus its points.
    if !count_fits(notch_count, 1, r.remaining()) {
        return None;
    }
    let mut notches = Vec::with_capacity(notch_count);
    for _ in 0..notch_count {
        let point_count = r.u32()? as usize;
        // Notch point: lat, lon, surface_m (the cut surface, not a bed -- see the module doc),
        // width_m -- 4 words per point.
        if !count_fits(point_count, 4, r.remaining()) {
            return None;
        }
        let mut points = Vec::with_capacity(point_count);
        for _ in 0..point_count {
            let lat = r.word()?;
            let lon = r.word()?;
            let surface_m = r.word()?;
            let width_m = r.word()?;
            points.push((lat, lon, surface_m, width_m));
        }
        notches.push(NotchLine { points });
    }

    // Fall: reach, lat, lon, height_m -- 4 words.
    if !count_fits(fall_count, 4, r.remaining()) {
        return None;
    }
    let mut falls = Vec::with_capacity(fall_count);
    for _ in 0..fall_count {
        let reach = r.u32()?;
        let lat = r.word()?;
        let lon = r.word()?;
        let height_m = r.word()?;
        falls.push(Fall { reach, at: (lat, lon), height_m });
    }

    if r.pos != words.len() {
        return None;
    }

    Some(HydroRecord { bodies, reaches, notches, falls, stats, ground })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hydrology::reaches::{Downstream, ReachClass};

    fn sample() -> HydroRecord {
        HydroRecord {
            bodies: vec![
                Body {
                    id: 0,
                    kind: BodyKind::Lake,
                    fresh: true,
                    enclosed: false,
                    forced: false,
                    level_m: 12.0,
                    area_m2: 2.0e6,
                    depth_m: 9.0,
                    outlet_reach: Some(1),
                    anchor: (10.0, 20.0),
                    outline: Vec::new(),
                    downstream: Downstream::Reach(1),
                    shore_member_count: 0,
                    shore_reach_m: 0.0,
                },
                Body {
                    id: 1,
                    kind: BodyKind::SaltFlat,
                    fresh: false,
                    enclosed: true,
                    forced: true,
                    level_m: 0.0,
                    area_m2: 5.0e5,
                    depth_m: 3.0,
                    outlet_reach: None,
                    anchor: (-5.0, 40.0),
                    outline: Vec::new(),
                    downstream: Downstream::Sink,
                    shore_member_count: 0,
                    shore_reach_m: 0.0,
                },
            ],
            reaches: vec![
                ReachLine {
                    id: 0,
                    class: ReachClass::River,
                    order: 2,
                    downstream: Downstream::Body(0),
                    fresh: true,
                    points: vec![
                        ReachPoint { lat_deg: 1.0, lon_deg: 2.0, bed_m: 3.0, width_m: 4.0, depth_m: 5.0, flow_m2: 6.0 },
                        ReachPoint { lat_deg: 7.0, lon_deg: 8.0, bed_m: 9.0, width_m: 10.0, depth_m: 11.0, flow_m2: 12.0 },
                    ],
                },
                ReachLine {
                    id: 1,
                    class: ReachClass::Stream,
                    order: 1,
                    downstream: Downstream::Ocean,
                    fresh: true,
                    points: vec![ReachPoint { lat_deg: 0.0, lon_deg: 0.0, bed_m: 0.0, width_m: 0.0, depth_m: 0.0, flow_m2: 0.0 }],
                },
            ],
            notches: vec![NotchLine { points: vec![(1.0, 2.0, 3.0, 0.5), (4.0, 5.0, 6.0, 1.5)] }],
            falls: vec![Fall { reach: 0, at: (1.0, 2.0), height_m: 3.0 }],
            stats: BakeStats {
                nodes: 100,
                land_nodes: 40,
                hollows: 5,
                kept: 2,
                notched: 3,
                closed: 1,
                streams: 1,
                rivers: 1,
                great: 0,
                max_order: 2,
                bifurcation_min: 2.5,
                bifurcation_max: 4.0,
                stream_flow_m2: 3.0e10,
                river_flow_m2: 3.0e11,
                great_flow_m2: 3.0e12,
                total_nodes: 100,
                wetness_nodes: 20,
                keep_depth_m: 8.0,
                keep_area_m2: 1.0e6,
                pond_max_area_m2: 1.0e6,
                keep_max_area_m2: 4.0e11,
                min_stream_nodes: 10.0,
                notch_fall_m: 1.0,
                evaporation_factor: 1.0,
                salt_flat_share: 0.1,
                forced_requested: 2,
                forced_matched: 1,
                capped_basins: 1,
                capped_inner: 2,
                capped_inner_kept: 1,
                refine_step_m: 1_500.0,
                refine_simplify_m: 500.0,
                refine_vertical_m: 1.0,
                fall_min_drop_m: 10.0,
                fall_max_run_m: 150.0,
                meander_wavelength_widths: 11.0,
                meander_amplitude_widths: 1.5,
                meander_max_slope: 0.002,
                crossings_coarse: 7,
                crossings_left: 2,
                ponds_found: 19,
                ponds_kept: 4,
                pond_cell_m: 250.0,
                pond_search_radius_m: 3_000.0,
                pond_keep_depth_m: 2.0,
                pond_keep_area_m2: 50_000.0,
                pond_wetness_share: 0.6,
                pond_max_slope: 0.03,
                pond_density_area_m2: 5.0e8,
                shore_members: 0,
                collar_points: 0,
            },
            // Every byte distinct, so a word written in the wrong order or the wrong endianness
            // cannot round-trip by coincidence.
            ground: [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
                     0x09, 0x0a, 0x0b, 0x0c, 0xfd, 0xfe, 0xff, 0x80],
        }
    }

    #[test]
    fn a_hand_built_record_round_trips_at_schema_7() {
        let record = sample();
        let words = encode(&record);
        assert_eq!(SCHEMA, 7.0, "Task 1 (plan 2b) bumped the schema for the ground fingerprint");
        assert_eq!(words[0], SCHEMA);
        assert_eq!(words[43], f64::from(record.stats.crossings_coarse));
        assert_eq!(words[44], f64::from(record.stats.crossings_left));
        // Task 5 of plan 1b-3's nine: the two pond counts, then the seven pond params, in order.
        assert_eq!(words[45], f64::from(record.stats.ponds_found));
        assert_eq!(words[46], f64::from(record.stats.ponds_kept));
        assert_eq!(words[47], record.stats.pond_cell_m);
        assert_eq!(words[48], record.stats.pond_search_radius_m);
        assert_eq!(words[49], record.stats.pond_keep_depth_m);
        assert_eq!(words[50], record.stats.pond_keep_area_m2);
        assert_eq!(words[51], record.stats.pond_wetness_share);
        assert_eq!(words[52], record.stats.pond_max_slope);
        assert_eq!(words[53], record.stats.pond_density_area_m2);
        // Task 1 of plan 1b-4's two, which closed SCHEMA 6's 56-word header.
        assert_eq!(words[54], f64::from(record.stats.shore_members));
        assert_eq!(words[55], f64::from(record.stats.collar_points));
        // Task 1 of plan 2b's four, closing the 60-word header: the digest, four bytes a word,
        // little-endian.
        assert_eq!(words[56], f64::from(0x0403_0201u32));
        assert_eq!(words[57], f64::from(0x0807_0605u32));
        assert_eq!(words[58], f64::from(0x0c0b_0a09u32));
        assert_eq!(words[59], f64::from(0x80ff_fefdu32));
        assert_eq!(decode(&words), Some(record));
    }

    /// The header is 60 words: everything after word 59 is the first body's first word.
    #[test]
    fn sample_bodies_start_after_the_sixty_word_header() {
        let record = sample();
        let words = encode(&record);
        assert_eq!(words[60], f64::from(record.bodies[0].id));
        let empty = HydroRecord { bodies: Vec::new(), reaches: Vec::new(), notches: Vec::new(),
                                  falls: Vec::new(), stats: record.stats.clone(),
                                  ground: record.ground };
        assert_eq!(encode(&empty).len(), HEADER_WORDS);
        assert_eq!(HEADER_WORDS, 60);
    }

    /// A pond's outline is a traced ring and a lake's is a shore-point set (spec §7, Ruling
    /// T1-2): the encoder writes both the same way, and `kind` is the only thing that says which
    /// it is. A ring round-trips point for point.
    #[test]
    fn a_ponds_traced_outline_round_trips() {
        let mut record = sample();
        record.bodies[0].kind = BodyKind::Pond;
        record.bodies[0].outline = vec![(1.0, 2.0), (1.0, 2.5), (1.5, 2.5)];
        let decoded = decode(&encode(&record)).expect("round trip");
        assert_eq!(decoded.bodies[0].outline, record.bodies[0].outline);
        assert_eq!(decoded.bodies[0].kind, BodyKind::Pond);
    }

    /// SCHEMA 4's 43-word header is a prefix of SCHEMA 5's 45, SCHEMA 6's 56 and SCHEMA 7's 60: a
    /// decoder that adapted rather than refused would read a body's first words as later header
    /// words and go wrong quietly.
    #[test]
    fn a_schema_4_record_is_refused() {
        let mut words = encode(&sample());
        words[0] = 4.0;
        assert_eq!(decode(&words), None, "SCHEMA 4 input must be refused outright, not adapted");
    }

    /// SCHEMA 6's 56-word header is a prefix of SCHEMA 7's 60, and a SCHEMA 6 record carries no
    /// fingerprint at all: adapted, its first body's first four words would be read as a digest.
    #[test]
    fn a_schema_6_record_is_refused() {
        let mut words = encode(&sample());
        words[0] = 6.0;
        assert_eq!(decode(&words), None, "SCHEMA 6 input must be refused outright, not adapted");
    }

    /// Each digest word is a u32 like any count: a fraction, a negative or a word past u32::MAX
    /// is not four bytes, and the record is refused rather than rounded into a digest.
    #[test]
    fn decode_refuses_a_ground_word_that_is_not_four_bytes() {
        for at in 56..60 {
            for bogus in [0.5, -1.0, 4_294_967_296.0, f64::NAN] {
                let mut words = encode(&sample());
                words[at] = bogus;
                assert_eq!(decode(&words), None, "ground word {at} = {bogus}");
            }
        }
        let mut words = encode(&sample());
        words[56] = f64::from(u32::MAX);
        assert_eq!(decode(&words).expect("u32::MAX is four bytes").ground[..4], [0xff; 4]);
    }

    #[test]
    fn a_schema_2_record_is_refused() {
        let mut words = encode(&sample());
        words[0] = 2.0;
        assert_eq!(decode(&words), None, "SCHEMA 2 input must be refused outright, not adapted");
    }

    #[test]
    fn truncation_and_trailing_words_are_refused() {
        let words = encode(&sample());
        assert_eq!(decode(&words[..words.len() - 1]), None, "truncated by one word");
        let mut padded = words.clone();
        padded.push(0.0);
        assert_eq!(decode(&padded), None, "trailing word");
        assert_eq!(decode(&[]), None, "empty");
    }

    #[test]
    fn a_wrong_schema_is_refused() {
        let mut words = encode(&sample());
        words[0] = SCHEMA + 1.0;
        assert_eq!(decode(&words), None);
    }

    #[test]
    fn decode_refuses_absurd_counts_without_allocating() {
        let mut words = encode(&sample());
        words[1] = 3.0e9; // body_count, absurd
        assert_eq!(decode(&words), None, "absurd body_count must be refused, not allocated");

        let mut words = encode(&sample());
        words[2] = 4.0e9; // reach_count, absurd
        assert_eq!(decode(&words), None, "absurd reach_count must be refused, not allocated");
    }

    #[test]
    fn the_header_is_sixty_words() {
        let record = HydroRecord {
            bodies: Vec::new(), reaches: Vec::new(), notches: Vec::new(), falls: Vec::new(),
            stats: sample().stats, ground: sample().ground,
        };
        assert_eq!(encode(&record).len(), 60);
        assert_eq!(encode(&record)[0], 7.0);
    }

    #[test]
    fn a_body_carries_its_extent_words() {
        let mut record = HydroRecord {
            bodies: vec![sample().bodies[0].clone()], reaches: Vec::new(), notches: Vec::new(),
            falls: Vec::new(), stats: sample().stats, ground: sample().ground,
        };
        // Three shore members and one collar point: the count must not exceed the outline it
        // indexes into, so a body carrying an extent carries the points to go with it.
        record.bodies[0].outline = vec![(1.0, 2.0), (1.0, 2.5), (1.5, 2.5), (1.5, 2.0)];
        record.bodies[0].shore_member_count = 3;
        record.bodies[0].shore_reach_m = 41_000.5;
        let words = encode(&record);
        assert_eq!(decode(&words).as_ref(), Some(&record));
        // 16 fixed words, then the outline pairs: shore_member_count and shore_reach_m sit
        // after downstream_id (word 12) and before outline_len (word 15).
        assert_eq!(words[HEADER_WORDS + 13], 3.0);
        assert_eq!(words[HEADER_WORDS + 14], 41_000.5);
    }

    #[test]
    fn a_schema_five_record_is_refused() {
        let mut words = encode(&HydroRecord {
            bodies: Vec::new(), reaches: Vec::new(), notches: Vec::new(), falls: Vec::new(),
            stats: sample().stats, ground: sample().ground,
        });
        words[0] = 5.0;
        assert_eq!(decode(&words), None);
    }

    /// A one-body record whose outline is `points` long and whose extent words are set by hand,
    /// so a decode guard can be aimed at exactly one word.
    fn one_body_with_extent(points: usize, shore_member_count: u32, shore_reach_m: f64) -> Vec<f64> {
        let mut body = sample().bodies[0].clone();
        body.outline = (0..points).map(|i| (i as f64, i as f64)).collect();
        body.shore_member_count = shore_member_count;
        body.shore_reach_m = shore_reach_m;
        encode(&HydroRecord {
            bodies: vec![body], reaches: Vec::new(), notches: Vec::new(), falls: Vec::new(),
            stats: sample().stats, ground: sample().ground,
        })
    }

    /// Ruling E-1 makes the shore members a prefix of the outline, and every consumer slices on
    /// the count -- `body.outline[..shore_member_count]` for the members, the remainder for the
    /// collar. A count past the outline's end is not a bad number to be carried around: it is a
    /// panic in the first reader, and a collar size that wraps to about 4 billion in release. It
    /// is a valid u32, so nothing but this guard refuses it.
    #[test]
    fn decode_refuses_a_body_claiming_more_shore_members_than_it_has_outline_points() {
        // Three of four is a legal extent: three members and one collar point.
        let some_members = one_body_with_extent(4, 3, 1_000.0);
        assert!(decode(&some_members).is_some(), "count < outline_len is a legal extent");

        // Past the outline's end is not. The extent words sit at 13 and 14 of a body's 16 fixed
        // words, after the 60-word header; `outline_len` is word 15. (Equalling it is refused
        // too, by Ruling Q-15 -- its own test below.)
        let outline_len_word = HEADER_WORDS + 15;
        for bogus in [5.0, 4.0e9, f64::from(u32::MAX)] {
            let mut words = one_body_with_extent(4, 3, 1_000.0);
            assert_eq!(words[outline_len_word], 4.0, "sanity: this world's body has 4 outline points");
            words[HEADER_WORDS + 13] = bogus;
            assert_eq!(decode(&words), None, "shore_member_count {bogus} exceeds the 4-point outline");
        }
    }

    /// Ruling Q-15: a shore-point set with no collar at all. §8.3's first clause admits a point
    /// when its nearest member is at least as near as its nearest *collar* point, and with no
    /// collar `dc` is infinite -- so that one body claims every point on the planet, and nothing
    /// but the index's bounding circle bounds the answer. `extent.rs` cannot write one, and now
    /// the decoder will not accept one either.
    #[test]
    fn decode_refuses_a_shore_point_body_with_no_collar() {
        for points in [1u32, 2, 3, 4, 9] {
            let length = points as usize; // cast-ok: a point count, not a float
            let all_members = one_body_with_extent(length, points, 1_000.0);
            assert_eq!(decode(&all_members), None,
                       "{points} members of a {points}-point outline leaves no collar");
            if points < 2 {
                continue;
            }
            // One fewer member is the same record with a collar, and it decodes.
            let with_collar = one_body_with_extent(length, points - 1, 1_000.0);
            assert!(decode(&with_collar).is_some(),
                    "{} members of a {points}-point outline is a legal extent", points - 1);
        }
        // A pond is untouched: its count is zero, which is the traced-ring discriminator and not
        // a collarless shore-point set, however few points its ring has.
        for points in [3usize, 4, 12] {
            assert!(decode(&one_body_with_extent(points, 0, 0.0)).is_some(),
                    "a {points}-point traced ring is not a collarless extent");
        }
    }

    /// §8.3's second clause tests a point against `shore_reach_m` as a distance. A NaN makes
    /// every such comparison false; an infinity admits the whole planet as inside one lake; a
    /// negative value is not a length. All three are valid `f64` words, so only this guard
    /// refuses them.
    #[test]
    fn decode_refuses_a_non_finite_or_negative_shore_reach() {
        for bogus in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, -0.5] {
            let mut words = one_body_with_extent(4, 2, 1_000.0);
            words[HEADER_WORDS + 14] = bogus;
            assert_eq!(decode(&words), None, "shore_reach_m {bogus} is not a usable distance");
        }
        // Zero is legal -- it is what a pond and a body with no usable edge both write -- and so
        // is any finite positive length, however large.
        for fine in [0.0, 1.0e300] {
            let mut words = one_body_with_extent(4, 2, 1_000.0);
            words[HEADER_WORDS + 14] = fine;
            assert_eq!(decode(&words).expect("decode").bodies[0].shore_reach_m, fine);
        }
    }

    #[test]
    fn word_to_u32_rejects_non_integral_negative_and_out_of_range_values() {
        assert_eq!(word_to_u32(3.0), Some(3));
        assert_eq!(word_to_u32(3.5), None);
        assert_eq!(word_to_u32(-1.0), None);
        assert_eq!(word_to_u32(f64::NAN), None);
        assert_eq!(word_to_u32(f64::INFINITY), None);
        assert_eq!(word_to_u32(u32::MAX as f64 + 2.0), None);
    }

    // ---- the ground fingerprint (plan 2b, Task 1) -------------------------------------------

    use crate::detail::ReliefParams;

    const SEED: i64 = 20_260_904;
    /// Not Earth's: the radius change below must be a real change, and 9,309 km -> 6,371 km is
    /// the pair the precedent in `SCHEMA`'s doc names.
    const RADIUS_M: f64 = 9_309_000.0;
    const PLATES: usize = 12;
    const LAND: f64 = 0.29;

    fn surface_with(seed: i64, radius_m: f64, plates: usize, land: f64) -> Surface {
        Surface::new(seed, radius_m, plates, land, None, None, None)
    }

    fn surface_at(radius_m: f64) -> Surface {
        surface_with(SEED, radius_m, PLATES, LAND)
    }

    fn fingerprint_of(seed: i64, radius_m: f64, plates: usize, land: f64) -> [u8; GROUND_BYTES] {
        ground_fingerprint(&surface_with(seed, radius_m, plates, land))
    }

    #[test]
    fn two_worlds_that_differ_anywhere_get_different_fingerprints() {
        // The property the whole guard rests on. A seed change, a plate change, a land change
        // and a radius change must each move the digest.
        let base = fingerprint_of(SEED, RADIUS_M, PLATES, LAND);
        assert_ne!(base, fingerprint_of(SEED + 1, RADIUS_M, PLATES, LAND), "seed");
        assert_ne!(base, fingerprint_of(SEED, 6_371_000.0, PLATES, LAND), "radius");
        assert_ne!(base, fingerprint_of(SEED, RADIUS_M, PLATES + 1, LAND), "plates");
        assert_ne!(base, fingerprint_of(SEED, RADIUS_M, PLATES, LAND + 0.05), "land");
    }

    #[test]
    fn the_same_world_fingerprints_the_same_every_time() {
        let once = fingerprint_of(SEED, RADIUS_M, PLATES, LAND);
        for _ in 0..8 {
            assert_eq!(once, fingerprint_of(SEED, RADIUS_M, PLATES, LAND));
        }
    }

    /// A digest that differs because ONE probe moved is a digest one interpolation change from
    /// colliding, so this asserts the margin rather than the inequality: on a 9,309 km -> 6,371 km
    /// change **every** probe must move past the millimetre rounding. It holds because the digest
    /// reads the ground with detail in, and detail's wavelengths are fixed in metres, so a radius
    /// change moves it everywhere. (`structural_m` alone failed this: 55 of 64 probes read it
    /// bit-identically at both radii. A 0.44 m minimum quoted elsewhere in this project belongs to
    /// a different probe scheme; this test's own run is the evidence for this one's.)
    #[test]
    fn every_probe_moves_when_the_radius_does_so_the_margin_is_not_one_lucky_point() {
        let wide = surface_at(RADIUS_M);
        let narrow = surface_at(6_371_000.0);
        let mut moved = 0usize;
        let mut smallest = f64::INFINITY;
        for index in 0..PROBE_COUNT {
            let point = probe_point(index);
            let a = wide.bake_ground_m(&point, None);
            let b = narrow.bake_ground_m(&point, None);
            let gap = if a > b { a - b } else { b - a };
            if gap >= 0.001 {
                moved += 1;
            }
            if gap < smallest {
                smallest = gap;
            }
        }
        println!("smallest probe movement, 9,309 km -> 6,371 km: {smallest} m");
        assert_eq!(moved, PROBE_COUNT, "only {moved} of {PROBE_COUNT} probes moved");
        assert!(smallest > 0.001, "smallest probe movement {smallest} m is at the rounding floor");
    }

    /// The test the `structural_m` design would have failed: two worlds identical except for
    /// their relief block. The bake reads detail (the pond search), so a record from one must not
    /// pass for the other. The structural assertion is the evidence that only the detail differs.
    #[test]
    fn two_worlds_differing_only_in_relief_get_different_fingerprints() {
        let mut rougher = ReliefParams::canonical();
        rougher.interior_m *= 1.5;
        let plain = surface_with(SEED, RADIUS_M, PLATES, LAND);
        for relief in [ReliefParams::hills(), rougher] {
            let other = Surface::new(SEED, RADIUS_M, PLATES, LAND, None, Some(relief), None);
            for index in 0..PROBE_COUNT {
                let point = probe_point(index);
                assert_eq!(plain.structural_m(&point).to_bits(), other.structural_m(&point).to_bits(),
                           "probe {index}: a relief block must not move the structure");
            }
            assert_ne!(ground_fingerprint(&plain), ground_fingerprint(&other),
                       "a relief-only change must move the digest");
        }
    }

    /// `ReliefParams::canonical()` is the `None` path bit for bit, so it must not move the digest:
    /// the fingerprint tells ground apart, not the way a caller spelled it.
    #[test]
    fn the_canonical_relief_spelled_out_is_the_same_ground() {
        let plain = surface_with(SEED, RADIUS_M, PLATES, LAND);
        let spelled = Surface::new(SEED, RADIUS_M, PLATES, LAND, None, Some(ReliefParams::canonical()), None);
        assert_eq!(ground_fingerprint(&plain), ground_fingerprint(&spelled));
    }

    #[test]
    fn a_probe_point_is_the_same_on_every_machine() {
        // The scatter must come from integer arithmetic, not a float sequence, or two hosts
        // disagree and every record looks foreign. The first three points are pinned bit for bit;
        // the values were pinned from an actual run of this test, not derived by hand.
        let pinned: [[u64; 3]; 3] = [
            [0xbfec_dafe_01b0_5edd, 0x3fdb_274f_9185_d339, 0xbfb5_37ee_b479_bda0],
            [0x3fdf_caa3_de04_078e, 0xbfea_5d8b_e8c5_81a8, 0x3fd1_750e_df85_c478],
            [0x3fb5_4dda_a35c_cc10, 0xbfef_b3c0_8446_befd, 0x3fbb_9367_af52_cb00],
        ];
        for (index, want) in pinned.iter().enumerate() {
            let v = probe_point(index).vector;
            let got = [v.x.to_bits(), v.y.to_bits(), v.z.to_bits()];
            println!("probe {index}: [{:#018x}, {:#018x}, {:#018x}]", got[0], got[1], got[2]);
            assert_eq!(&got, want, "probe {index} moved");
        }
    }

    /// The scatter is a scatter: every probe is a unit vector, and no two coincide.
    #[test]
    fn the_probes_are_distinct_unit_vectors() {
        let points: Vec<SpherePoint> = (0..PROBE_COUNT).map(probe_point).collect();
        for (i, p) in points.iter().enumerate() {
            let length = p.vector.length();
            assert!(length > 1.0 - 1e-12 && length < 1.0 + 1e-12, "probe {i} length {length}");
            for q in &points[..i] {
                assert!(p.vector != q.vector, "probe {i} repeats an earlier one");
            }
        }
    }

    /// The bake writes the fingerprint of the surface it was handed, and it survives the wire.
    #[test]
    fn a_bake_carries_its_surfaces_fingerprint_through_the_wire() {
        let surface = crate::hydrology::bake_tests::world();
        let record = crate::hydrology::bake(&surface, &crate::hydrology::bake_tests::params())
            .expect("bake");
        assert_eq!(record.ground, ground_fingerprint(&surface));
        let decoded = decode(&encode(&record)).expect("round trip");
        assert_eq!(decoded.ground, record.ground);
    }
}
