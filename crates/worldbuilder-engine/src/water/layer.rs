//! Spec §8.1: **the water layer** -- a river cuts the ground it runs through.
//!
//! A stage in `Surface::elevation_m`, after features and before detail. It reads a baked
//! `HydroRecord` as geometry and nothing else: each refined reach and each notch is a polyline
//! with a target height and a width at every recorded point, and the layer lowers the ground
//! around that line.
//!
//! # What it cuts, and what it does not
//!
//! - **River channels:** a trapezoid cut to `bed_m` along each refined reach -- `width_m` wide at
//!   the bank, and the banks blended over one width either side (`WaterParams::bank_widths`,
//!   canonically `1`). The bed is flat across the channel; from the channel's edge the cut falls
//!   off **linearly** to nothing one bank width further out, which is what makes the cross-section
//!   a trapezoid rather than a smoothed trough.
//! - **Notches:** cut the same way, to the notch point's own `surface_m` -- the lowered ground, which
//!   is the water surface through the cut (Ruling F-2).
//! - **Lake beds: not cut.** A kept lake is an existing hollow; its outline and its level decide the
//!   water surface, and the ground under it is already the ground the bake found it in. A point
//!   the query's own body test claims (`query::claim_bodies`) is returned untouched, even where a
//!   reach's channel runs into or through the body -- which is common, because the fine pond
//!   search finds its ponds along reach lines. The first version of this module simply never
//!   read `bodies`, and that cut a channel through most lake floors on a real bake;
//!   `no_body_is_cut_where_reaches_enter_it` pins the rule where it can actually fail.
//!
//! # Where the channel is: the query's answer, not a second one
//!
//! Inside a leg's half-width is **exactly** where `water::query` answers `River`, because both ask
//! the same functions -- `query::leg_foot` for the distance, `query::leg_width_m` for Ruling Q-7's
//! wider-endpoint width, `query::half_of` for half of it -- and the same index. The layer's
//! authority is `1` there and nowhere else, so "this is a river" and "this is a channel" cannot
//! disagree about where the channel is. `the_carve_and_the_query_agree_about_where_the_channel_is`
//! sweeps for any point where they would.
//!
//! **The bed is interpolated along a leg, and so is the query's level (Ruling C-13).** The cut
//! follows the leg's foot linearly from one recorded `bed_m` to the next -- exact at every recorded
//! point, continuous between them -- through `query::along_leg`, the same function the query reads
//! its river level with. Before C-13 the query reported the nearest recorded point's level, a step
//! function along a reach, and the interpolated bed stood above it in up to a fifth of all leg
//! halves on the parity worlds: a river reading dry in its own channel.
//!
//! # Three rules, each of which has bitten this project before
//!
//! 1. **The layer carries its authority out and applies no damping.** [`WaterLayer::cut_m`] hands
//!    back `(cut ground, authority)`, as `Features::apply` hands back `(shaped, authority)`, and
//!    `Surface::elevation_m` owns the composition -- damping detail and gullies by
//!    `(1 - features' authority) * (1 - this authority)` (Ruling C-14). Authority is `1` inside a
//!    channel and falls to `0` at the far edge of the blended bank.
//! 2. **A cut only ever lowers ground.** A river mouth's bed can *rise* at its last step (Ruling
//!    R-4's known I4 side effect), and a notch's cut surface can stand above ground the landform
//!    already carried lower. A "cut" to a target above the ground would be a dam, so a leg whose
//!    target is not below the ground here leaves it alone.
//! 3. **A point no channel reaches is an early return, not a zero cut.** It hands back the very
//!    `ground_m` it was given, untouched by any arithmetic: `-0.0 + 0.0` is `+0.0`, and "every value
//!    except one" is not bit-identical.
//!
//! # How the record reaches `Surface` (Ruling C-2)
//!
//! Shared, not copied: [`WaterLayer`] holds an `Arc<IndexedRecord>`, and `IndexedRecord` is the
//! decoded record with the index built over it and every recorded point pre-projected. A world
//! handle built on a held bake costs one pointer for the record, however many handles share it.

use std::sync::Arc;

use crate::hydrology::HydroRecord;
use crate::sphere::SpherePoint;
use crate::water::index::{WaterIndex, DEFAULT_CELL_M};
use crate::water::query::{
    along_leg, claim_bodies, half_of, leg_foot, leg_width_m, reach_position, Detail, Ground,
    Landform,
};

/// Spec §8.1's "banks blended over one width either side".
pub const CANONICAL_BANK_WIDTHS: f64 = 1.0;

/// The widest bank the layer will blend, in channel widths. **The index is built to this**, not
/// to the block's own value ([`footprint_m`]), so one index serves every admissible block and the
/// query that shares it; a block asking for more is refused at the join rather than cut with a
/// bank the index cannot see the far side of.
pub const MAX_BANK_WIDTHS: f64 = 4.0;

/// How far from a leg's centre line the layer can reach: half the width, plus the widest bank
/// any admissible block may blend. `water::index` lists a reach or notch in every cell within
/// this of its line, which is what lets the layer trust `WaterIndex::candidates` for a bank point
/// as well as a channel point.
///
/// Zero for a width that is negative or not a number, by `query::half_of`'s rule.
pub fn footprint_m(width_m: f64) -> f64 {
    let half = half_of(width_m);
    half + MAX_BANK_WIDTHS * (half + half)
}

/// The water block: what a caller chooses about the carve. The record is not in it -- the record
/// is what the carve is *of*, and it arrives beside the block ([`Carve`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaterParams {
    /// How far the bank is blended beyond the channel's edge, in channel widths. The channel's
    /// width is Ruling Q-7's -- the wider of a leg's two recorded endpoints.
    pub bank_widths: f64,
}

impl WaterParams {
    /// Spec §8.1 as written: banks blended over one width either side.
    pub fn canonical() -> Self {
        WaterParams { bank_widths: CANONICAL_BANK_WIDTHS }
    }

    /// Finite, above zero, and no wider than the index was built for ([`MAX_BANK_WIDTHS`]).
    /// Written as positive comparisons so a NaN fails every one of them.
    pub fn is_admissible(&self) -> bool {
        self.bank_widths > 0.0 && self.bank_widths <= MAX_BANK_WIDTHS
    }
}

/// A decoded record, the [`WaterIndex`] built over it, and every recorded point projected once.
///
/// **Built together so they cannot be paired wrongly.** An index is only an index over the record
/// it was built from; handing the layer a record and an index separately would let a caller pair
/// an index over one bake with another, and the layer would cut the wrong lines with no way to
/// notice. There is no constructor that takes them apart.
#[derive(Debug, Clone)]
pub struct IndexedRecord {
    record: HydroRecord,
    index: WaterIndex,
    /// `reach_points[i][k]` is `record.reaches[i].points[k]` on the sphere -- by position, not id.
    reach_points: Vec<Vec<SpherePoint>>,
    /// `notch_points[i][k]` is `record.notches[i].points[k]` on the sphere.
    notch_points: Vec<Vec<SpherePoint>>,
}

impl IndexedRecord {
    /// Build the index at `radius_m` and [`DEFAULT_CELL_M`] -- the query's own cell -- and project
    /// every recorded point once, so a sample never pays `from_latlon` for a leg's ends.
    pub fn new(record: HydroRecord, radius_m: f64) -> IndexedRecord {
        let index = WaterIndex::build(&record, radius_m, DEFAULT_CELL_M);
        let reach_points = record.reaches.iter()
            .map(|reach| reach.points.iter()
                .map(|p| SpherePoint::from_latlon(p.lat_deg, p.lon_deg)).collect())
            .collect();
        let notch_points = record.notches.iter()
            .map(|notch| notch.points.iter()
                .map(|&(lat, lon, _, _)| SpherePoint::from_latlon(lat, lon)).collect())
            .collect();
        IndexedRecord { record, index, reach_points, notch_points }
    }

    pub fn record(&self) -> &HydroRecord {
        &self.record
    }

    pub fn index(&self) -> &WaterIndex {
        &self.index
    }
}

/// What `Surface::with_water` joins to a world: the block, and the held bake it carves.
#[derive(Debug, Clone)]
pub struct Carve {
    pub params: WaterParams,
    pub bake: Arc<IndexedRecord>,
}

/// Why `Surface::with_water` would not join a record to a world.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CarveRefused {
    /// The block is not admissible ([`WaterParams::is_admissible`]).
    Params,
    /// The record's index was built at another radius, so its distances are another planet's.
    Radius,
    /// The record was baked from other ground (plan 2b Task 2's refusal, made once, at the join).
    Foreign(crate::hydrology::record::ForeignGround),
}

/// The layer itself: a block and the held bake it cuts.
#[derive(Debug, Clone)]
pub struct WaterLayer {
    params: WaterParams,
    bake: Arc<IndexedRecord>,
}

impl WaterLayer {
    /// A layer over `bake`. `Surface::with_water` is the only caller that puts one in a world, and
    /// it checks the block, the radius and the ground first; this constructor checks nothing.
    pub fn new(params: WaterParams, bake: Arc<IndexedRecord>) -> WaterLayer {
        WaterLayer { params, bake }
    }

    pub fn params(&self) -> WaterParams {
        self.params
    }

    pub fn bake(&self) -> &Arc<IndexedRecord> {
        &self.bake
    }

    /// Spec §8.1's cut at `point`, over ground standing at `ground_m`: **the cut ground and the
    /// layer's authority**, in that order.
    ///
    /// Every reach and notch the index offers is asked what it would cut to here. The answer is
    /// the **lowest** of those cuts -- channels are a union, and where two meet, the deeper one
    /// holds -- and the authority is the **highest** of theirs. A leg whose target stands at or
    /// above `ground_m` cuts nothing (rule 2 of the module doc; the running minimum is seeded with
    /// `ground_m`, so its answer is discarded) but keeps its authority: the point is still in a
    /// channel, and what the damping (Ruling C-14) needs to know is that, not whether this
    /// particular ground happened to need lowering.
    ///
    /// **A point no leg reaches returns `(ground_m, 0.0)` with `ground_m` untouched** (rule 3).
    /// `cut <= ground_m` always; never a single bit above it.
    ///
    /// **Nor is a point inside a body cut** (spec §8.1, "lake beds: not cut"): see
    /// [`WaterLayer::cut_with`], which this is with `ground_m` standing for both of the query's
    /// surfaces. That is right for a caller with one surface -- a fixture, a flat plain -- and
    /// wrong for a world with a pond on it, whose level was found in the detail field;
    /// `Surface::elevation_m` calls `cut_with`.
    pub fn cut_m(&self, point: &SpherePoint, ground_m: f64) -> (f64, f64) {
        let flat = move |_: &SpherePoint| ground_m;
        self.cut_with(point, ground_m, &Detail(&flat))
    }

    /// [`WaterLayer::cut_m`], with the query's second surface named: `ground_m` is the landform at
    /// `point` (`Surface::structural_m`, the ground being cut) and `detail` the bare detail field
    /// at the record's `pond_cell_m` (`Surface::bake_ground_m`) -- exactly the two surfaces
    /// `wasm.rs::with_ground` hands the query, and asked only at `point`.
    ///
    /// **A point a body claims is returned untouched, with no authority**, by the query's own body
    /// test (`query::claim_bodies`: inside the extent and at or under the level, each body judged
    /// against the surface its level was found in). The fine pond search looks for ponds along
    /// reach lines, so a pond on a river is the ordinary case, not the odd one; before this rule a
    /// channel was cut straight through the floor of 12 of the 14 bodies of `bake_tests::world()`
    /// at `earth_like(30_000)`, up to 197 m deep. Using the query's test rather than a second one
    /// is what makes "this is a lake" and "this is not a channel" the same answer (Ruling C-12).
    /// It is asked only where some leg reached the point, so a point far from any channel pays
    /// nothing for it.
    pub fn cut_with(&self, point: &SpherePoint, ground_m: f64, detail: &Detail) -> (f64, f64) {
        let bake = &*self.bake;
        let radius_m = bake.index.radius_m();
        let candidates = bake.index.candidates(point);
        let bank_widths = self.params.bank_widths;

        // Rule 2 lives in this seed: the answer starts as the ground itself and only a strictly
        // lower leg replaces it, so a leg whose target stands above the ground -- a rising mouth,
        // a notch over lower ground -- can never raise it.
        let mut cut = ground_m;
        let mut authority = 0.0;
        let mut touched = false;
        let mut take = |target_m: f64, weight: f64| {
            touched = true;
            let lowered = lowered_m(ground_m, target_m, weight);
            if lowered < cut {
                cut = lowered;
            }
            if weight > authority {
                authority = weight;
            }
        };

        for &id in candidates.reaches {
            let Some(position) = reach_position(&bake.record, id) else {
                continue; // an index over another record: refuse the candidate, never index blindly
            };
            let reach = &bake.record.reaches[position];
            let line = &bake.reach_points[position];
            let at = |k: usize| (reach.points[k].bed_m, reach.points[k].width_m);
            each_leg(point, line, &at, radius_m, bank_widths, &mut take);
        }
        for &id in candidates.notches {
            let position = id as usize; // cast-ok: a notch's candidate id IS its position (index.rs)
            let (Some(notch), Some(line)) =
                (bake.record.notches.get(position), bake.notch_points.get(position)) else {
                continue;
            };
            let at = |k: usize| (notch.points[k].2, notch.points[k].3);
            each_leg(point, line, &at, radius_m, bank_widths, &mut take);
        }

        if !touched {
            return (ground_m, 0.0); // rule 3: the ground exactly as it came in
        }
        if !candidates.bodies.is_empty() {
            let landform = move |_: &SpherePoint| ground_m;
            let ground = Ground { landform_m: Landform(&landform), detail_m: Detail(detail.0) };
            let claims = claim_bodies(&bake.record.bodies, candidates.bodies, &ground, point, radius_m, ground_m);
            if claims.best.is_some() {
                return (ground_m, 0.0); // a lake's bed: an existing hollow, never a channel
            }
        }
        (cut, authority)
    }
}

/// Ask every leg of one polyline what it would cut `point` to, and hand each answer that reaches
/// the point to `take(target_m, authority)`. `at(k)` is recorded point `k`'s `(target_m, width_m)`
/// -- `bed_m` for a reach, `surface_m` for a notch -- and `line[k]` is the same point projected.
///
/// A one-point line is a disc of its own width, as `query::river_claim` has it.
fn each_leg(point: &SpherePoint, line: &[SpherePoint], at: &dyn Fn(usize) -> (f64, f64),
            radius_m: f64, bank_widths: f64, take: &mut dyn FnMut(f64, f64)) {
    if line.is_empty() {
        return;
    }
    if line.len() == 1 {
        let (target_m, width_m) = at(0);
        let distance_m = point.distance_to(&line[0], radius_m);
        if let Some(weight) = authority_at(distance_m, width_m, bank_widths) {
            take(target_m, weight);
        }
        return;
    }
    for leg in 0..line.len() - 1 {
        let (target_a, width_a) = at(leg);
        let (target_b, width_b) = at(leg + 1);
        let (distance_m, along) = leg_foot(point, &line[leg], &line[leg + 1], radius_m);
        let Some(weight) = authority_at(distance_m, leg_width_m(width_a, width_b), bank_widths)
        else {
            continue;
        };
        // Ruling C-13: the query's own interpolation, so its water surface and this bed agree.
        take(along_leg(target_a, target_b, along), weight);
    }
}

/// The trapezoid's cross-section, as an authority: `1` within half the width of the centre line
/// -- exactly where the query answers `River` -- falling **linearly** to `0` one bank further out,
/// and `None` from there on. `bank_m` is `bank_widths` channel widths.
///
/// `None` rather than `Some(0.0)` at and past the far edge, so a leg that does not reach the point
/// cannot count as having touched it.
fn authority_at(distance_m: f64, width_m: f64, bank_widths: f64) -> Option<f64> {
    let half = half_of(width_m);
    if distance_m <= half {
        return Some(1.0);
    }
    let bank_m = bank_widths * (half + half);
    if distance_m < half + bank_m {
        // `bank_m > 0` here: `distance_m > half` and `distance_m < half + bank_m`.
        return Some(1.0 - (distance_m - half) / bank_m);
    }
    None
}

/// `ground_m` moved toward `target_m` by `weight`: the target itself at full authority, and the
/// straight line between them otherwise.
///
/// **Exactly `target_m` at `weight == 1`**, by a branch rather than by arithmetic: `ground -
/// (ground - target)` is not `target` in floating point, and "mid-channel sits at `bed_m`" is
/// meant exactly.
///
/// **This does not clamp, and that is deliberate: the clamp is `cut_m`'s running minimum.** A
/// target above the ground makes this answer above the ground, and `cut_m` discards it, because
/// its minimum is *seeded with `ground_m`* and only a strictly lower answer replaces it (a NaN
/// never does). A second clamp here was written, mutation-tested, and found dead -- deleting it
/// turned no test red -- so the guard lives in one place, where the test that pins it can see it.
fn lowered_m(ground_m: f64, target_m: f64, weight: f64) -> f64 {
    if weight >= 1.0 {
        return target_m;
    }
    ground_m - weight * (ground_m - target_m)
}

#[cfg(test)]
#[path = "layer_tests.rs"]
pub(crate) mod tests;
