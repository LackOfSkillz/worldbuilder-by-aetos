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
//! - **Reach `fresh`** means "its chain reaches the ocean": following its `downstream` through
//!   reaches and bodies ends at `Ocean`, not at a closed lake's `Sink`.

use crate::detmath as m;
use crate::hydrology::reaches::{Downstream, ReachClass};
use crate::hydrology::{BakeStats, Body, BodyKind, Fall, HydroRecord, NotchLine, ReachLine, ReachPoint};

/// 5.0 as of Task 3 (plan 1b-3): the header grew from 43 to 45 words, adding the crossing pass's
/// two counts after `meander_max_slope` -- `crossings_coarse` (word 43, how many crossings the
/// coarse record already had, which Ruling S-2 keeps) and `crossings_left` (word 44, how many are
/// left in the record as it ships, after the pass, the meander and simplification).
///
/// SCHEMA 4 (Task 3 of plan 1b-2) grew the header from 32 to 43 words, adding eleven words after
/// `forced_matched` -- three counts of what capped basins keep (`capped_basins`, `capped_inner`,
/// `capped_inner_kept`, carry-forward I3) and an echo of the eight refinement params
/// (`refine_step_m`, `refine_simplify_m`, `refine_vertical_m`, `fall_min_drop_m`,
/// `fall_max_run_m`, `meander_wavelength_widths`, `meander_amplitude_widths`,
/// `meander_max_slope`). Earlier schemas are refused outright -- `decode` never adapts an old
/// record to the new shape.
pub const SCHEMA: f64 = 5.0;

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
/// Unchecked: each call site's `min_words` literal (14 for a body, 7 for a reach, 1 for a
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
    };

    // Body: id, kind, fresh, enclosed, forced, level_m, area_m2, depth_m, outlet_reach,
    // anchor_lat, anchor_lon, downstream_kind, downstream_id, outline_len -- 14 words, plus its
    // outline.
    if !count_fits(body_count, 14, r.remaining()) {
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
        let outline_len = r.u32()? as usize;
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

    Some(HydroRecord { bodies, reaches, notches, falls, stats })
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
            },
        }
    }

    #[test]
    fn a_hand_built_record_round_trips_at_schema_5() {
        let record = sample();
        let words = encode(&record);
        assert_eq!(SCHEMA, 5.0, "Task 3 (plan 1b-3) bumped the schema for the 45-word header");
        assert_eq!(words[0], SCHEMA);
        assert_eq!(words[43], f64::from(record.stats.crossings_coarse));
        assert_eq!(words[44], f64::from(record.stats.crossings_left));
        assert_eq!(decode(&words), Some(record));
    }

    /// SCHEMA 4 is the schema this one replaced, and its 43-word header is a prefix of this
    /// one's 45: a decoder that adapted rather than refused would read a body's first two words
    /// as the two new counts and go wrong quietly.
    #[test]
    fn a_schema_4_record_is_refused() {
        let mut words = encode(&sample());
        words[0] = 4.0;
        assert_eq!(decode(&words), None, "SCHEMA 4 input must be refused outright, not adapted");
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
    fn word_to_u32_rejects_non_integral_negative_and_out_of_range_values() {
        assert_eq!(word_to_u32(3.0), Some(3));
        assert_eq!(word_to_u32(3.5), None);
        assert_eq!(word_to_u32(-1.0), None);
        assert_eq!(word_to_u32(f64::NAN), None);
        assert_eq!(word_to_u32(f64::INFINITY), None);
        assert_eq!(word_to_u32(u32::MAX as f64 + 2.0), None);
    }
}
