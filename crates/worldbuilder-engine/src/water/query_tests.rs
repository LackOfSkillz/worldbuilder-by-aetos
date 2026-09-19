//! The index and the query against real bakes, rather than against a hand-written record.
//!
//! `index.rs`'s and `query.rs`'s own tests pin the rules one at a time on a fixture small enough
//! to reason about. This file asks the two questions only a real record can answer.
//!
//! The index's: does it ever *lose* something? Every recorded body, reach and notch must be a
//! candidate at every one of its own recorded points -- the weakest property the query can be
//! built on, and the one a wrong cell, a dropped leg or a mis-swept seam all break.
//!
//! The query's, which is spec **§14.9** and the property plan 2a exists to satisfy: **the query
//! agrees with the record** at every sampled point of every body outline and reach.

use crate::hydrology::bake_tests::refined_populations;
use crate::hydrology::buckets::BucketIndex;
use crate::hydrology::{Body, BodyKind, HydroParams};
use crate::sphere::SpherePoint;
use crate::surface::Surface;
use crate::water::index::{body_circle_m, WaterIndex, DEFAULT_CELL_M};
use crate::water::query::{water_at, Detail, Ground, Landform, WaterKind};

/// What Ruling Q-13's bounding circle costs, measured rather than argued. Reports, per
/// population: cells occupied, entries stored per family, the largest cell's item count, and the
/// mean number of candidates over a fixed 10,000-point scatter -- the number Task 6 gates at
/// under 50, asserted here so a later change cannot walk past it quietly.
#[test]
fn the_index_costs_what_it_costs_over_a_fixed_ten_thousand_point_scatter() {
    const PROBES: u32 = 10_000;
    for (name, surface, params) in refined_populations() {
        let record = crate::hydrology::bake(&surface, &params).expect("bake");
        let index = WaterIndex::build(&record, surface.radius_m, DEFAULT_CELL_M);
        let (occupied, largest, body_entries, reach_entries, notch_entries) = index.stats();

        let mut total = 0usize;
        let mut worst = 0usize;
        for i in 0..PROBES {
            let got = index.candidates(&crate::stream::spiral_point(i, PROBES));
            let here = got.bodies.len() + got.reaches.len() + got.notches.len();
            total += here;
            if here > worst {
                worst = here;
            }
        }
        let mean = total as f64 / f64::from(PROBES);
        eprintln!(
            "{name}: {occupied} cells occupied, largest lists {largest}; \
             {body_entries} body + {reach_entries} reach + {notch_entries} notch entries \
             ({} bodies / {} reaches / {} notches recorded). \
             Over {PROBES} spiral probes: mean {mean:.3} candidates, worst {worst}.",
            record.bodies.len(), record.reaches.len(), record.notches.len());
        assert!(mean < 50.0, "{name}: mean {mean} candidates is over Task 6's gate of 50");
    }
}

/// The same measurement at a million nodes, where the bodies the bounding circle is expensive
/// for actually exist -- a 12,000-node bake has no great lake to be alarmed about.
///
/// `#[ignore]`d for the reason the rest of this crate's million-node work is: minutes in the
/// debug profile CI runs `cargo test` under. Run it with
/// `cargo test --release -p worldbuilder-engine --lib the_index_costs_what_it_costs_at_a_million_nodes
/// -- --ignored --nocapture`.
#[test]
#[ignore]
fn the_index_costs_what_it_costs_at_a_million_nodes() {
    const PROBES: u32 = 10_000;
    let params = crate::hydrology::HydroParams::earth_like(1_000_000);
    for (name, surface) in [("1M plain", crate::hydrology::bake_tests::world()),
                            ("1M ranges", crate::hydrology::bake_tests::ranges_world())] {
        let record = crate::hydrology::bake(&surface, &params).expect("bake");
        let index = WaterIndex::build(&record, surface.radius_m, DEFAULT_CELL_M);
        let (occupied, largest, body_entries, reach_entries, notch_entries) = index.stats();

        // The single most expensive body, so the great lake can be named rather than inferred.
        // Its own footprint through the same sweep `build` uses, without rebuilding the index.
        let grid = BucketIndex::new(surface.radius_m, DEFAULT_CELL_M);
        let mut worst_body = (0u32, 0.0f64, 0usize);
        for body in &record.bodies {
            let anchor = SpherePoint::from_latlon(body.anchor.0, body.anchor.1);
            // `build`'s own rule, called rather than restated: a later change to Q-13 moves this
            // measurement with it instead of leaving it reporting the superseded one.
            let circle_m = body_circle_m(body, surface.radius_m);
            let cells = grid.cells_within(&anchor, circle_m).len();
            if cells > worst_body.2 {
                worst_body = (body.id, circle_m, cells);
            }
        }

        let mut total = 0usize;
        let mut worst = 0usize;
        for i in 0..PROBES {
            let got = index.candidates(&crate::stream::spiral_point(i, PROBES));
            let here = got.bodies.len() + got.reaches.len() + got.notches.len();
            total += here;
            if here > worst {
                worst = here;
            }
        }
        let mean = total as f64 / f64::from(PROBES);
        eprintln!(
            "{name}: {} bodies / {} reaches / {} notches. \
             {occupied} cells occupied, largest lists {largest}; \
             {body_entries} body + {reach_entries} reach + {notch_entries} notch entries. \
             Widest body is {} at a {:.0} m bounding circle over {} cells. \
             Over {PROBES} spiral probes: mean {mean:.3} candidates, worst {worst}.",
            record.bodies.len(), record.reaches.len(), record.notches.len(),
            worst_body.0, worst_body.1, worst_body.2);
        assert!(mean < 50.0, "{name}: mean {mean} candidates is over Task 6's gate of 50");
    }
}

/// Every item is a candidate at every one of its own recorded points, on three 12,000-node
/// bakes. Asserts the three counts as well as the membership, so a population that silently
/// stopped producing notches (or bodies, or reaches) fails here rather than passing vacuously.
#[test]
fn every_recorded_point_lists_its_own_item() {
    for (name, surface, params) in refined_populations() {
        let record = crate::hydrology::bake(&surface, &params).expect("bake");
        let index = WaterIndex::build(&record, surface.radius_m, DEFAULT_CELL_M);

        let mut body_points = 0usize;
        for body in &record.bodies {
            for &(lat, lon) in &body.outline {
                let point = SpherePoint::from_latlon(lat, lon);
                assert!(index.candidates(&point).bodies.contains(&body.id),
                        "{name}: body {} is missing at its own outline point {lat},{lon}",
                        body.id);
                body_points += 1;
            }
        }

        let mut reach_points = 0usize;
        for reach in &record.reaches {
            for rp in &reach.points {
                let point = SpherePoint::from_latlon(rp.lat_deg, rp.lon_deg);
                assert!(index.candidates(&point).reaches.contains(&reach.id),
                        "{name}: reach {} is missing at its own point {},{}",
                        reach.id, rp.lat_deg, rp.lon_deg);
                reach_points += 1;
            }
        }

        let mut notch_points = 0usize;
        for (i, notch) in record.notches.iter().enumerate() {
            let id = i as u32; // cast-ok: a position in `record.notches`, not a float
            for &(lat, lon, _, _) in &notch.points {
                let point = SpherePoint::from_latlon(lat, lon);
                assert!(index.candidates(&point).notches.contains(&id),
                        "{name}: notch {i} is missing at its own point {lat},{lon}");
                notch_points += 1;
            }
        }

        // Not a vacuous pass: this population really does record all three families.
        assert!(!record.bodies.is_empty(), "{name}: no bodies to check");
        assert!(!record.reaches.is_empty(), "{name}: no reaches to check");
        assert!(!record.notches.is_empty(), "{name}: no notches to check");

        let (occupied, largest, body_entries, reach_entries, notch_entries) = index.stats();
        eprintln!(
            "{name}: {} bodies / {} reaches / {} notches; \
             checked {body_points} + {reach_points} + {notch_points} recorded points. \
             Index at {:.0} m cells: {occupied} cells occupied, largest lists {largest} items, \
             {body_entries} body + {reach_entries} reach + {notch_entries} notch entries.",
            record.bodies.len(), record.reaches.len(), record.notches.len(), index.cell_m());
    }
}

// ---- spec §14.9: the query agrees with the record ------------------------------------------

/// Is this what a *body* answers, as against ocean, river or dry ground?
fn is_body(kind: WaterKind) -> bool {
    matches!(kind, WaterKind::Lake | WaterKind::SaltLake | WaterKind::SaltFlat | WaterKind::Pond)
}

/// What the record says a body is, stated here independently of the query's own mapping so that
/// this test compares the query against the record rather than against itself.
fn recorded_kind(kind: BodyKind) -> WaterKind {
    match kind {
        BodyKind::Lake => WaterKind::Lake,
        BodyKind::Pond => WaterKind::Pond,
        BodyKind::SaltLake => WaterKind::SaltLake,
        BodyKind::SaltFlat => WaterKind::SaltFlat,
    }
}

/// The three refined populations, **plus the one that actually keeps a `Pond`**.
///
/// At 12,000 nodes none of the three records a single body of kind `Pond`. Two of them carry
/// traced rings -- 12 and 3 of them -- but Ruling S-11 labels a fine find by its area, `Pond`
/// below `pond_max_area_m2` and `Lake` at or above it, and the smallest survivor these worlds
/// keep is 1.3 km² against a shipped threshold of 1.0 km². Widening the corridor does not help:
/// at §6.6's 3 km it keeps 14 rings and the smallest is still over the line. A property that
/// asserted anything about ponds on those three alone would be asserting it about an empty set.
///
/// So the fourth population is `junction_params` -- the one with the most rings -- with
/// `pond_max_area_m2` at 5 km², which is the *one parameter* that decides the label and touches
/// nothing else the query reads. Nothing coarse moves with it: the coarse bodies these worlds
/// record are 8.5e10 m² and up.
///
/// **That makes the fourth population a re-bake of the second, and every count in it that is not
/// about the pond label is the second's count again.** The `bool` says so: `true` for the three
/// stock populations, whose counts are distinct and are what the closing totals sum, and `false`
/// for the pond probe, which is printed for itself and summed into nothing but the pond count.
/// Adding all four would report 169 shore members where there are 142, and 61 anchors where there
/// are 41, in a table Task 6 pins.
fn query_populations() -> Vec<(String, bool, Surface, HydroParams)> {
    let mut out: Vec<(String, bool, Surface, HydroParams)> = refined_populations().into_iter()
        .map(|(name, surface, params)| (name.to_string(), true, surface, params))
        .collect();
    let (name, surface, mut params) = refined_populations().into_iter().nth(1)
        .expect("junction_params");
    params.pond_max_area_m2 = 5.0e6;
    out.push((format!("{name} at a 5 km² pond threshold (a re-bake of it: only the pond count \
                       below is new)"), false, surface, params));
    out
}

/// The §14.9 counts summed over the populations that are **not** re-bakes of one another, so the
/// totals Task 6 pins are of distinct points rather than of points counted twice.
#[derive(Default)]
struct Distinct {
    members: usize,
    member_dry: usize,
    anchors: usize,
    anchor_own: usize,
    anchor_above: usize,
    vertices: usize,
    vertex_own: usize,
    vertex_above: usize,
    vertex_unclaimed: usize,
    vertex_above_max_m: f64,
    vertex_above_sum_m: f64,
    collars: usize,
    collar_own: usize,
    reach_points: usize,
    reach_own: usize,
    reach_confluence: usize,
    midpoints: usize,
    notch_points: usize,
}

/// The midpoint of the great-circle arc `a`-`b`: the normalised sum of the two directions, which
/// is `index.rs`'s own `along(a, b, 0.5)` without the module boundary in the way.
fn midpoint(a: &SpherePoint, b: &SpherePoint) -> SpherePoint {
    SpherePoint::from_vector(&a.vector.add(&b.vector)).unwrap_or(*a)
}

/// **Spec §14.9.** The query agrees with the record at every sampled point of every body outline
/// and reach, on the three 12,000-node populations the bake tests use and the fourth
/// [`query_populations`] adds so that the pond clause is answered by an actual pond.
///
/// **The fourth population is a re-bake of the second** with one parameter changed, so every count
/// in it that is not about the pond label duplicates the second's. The closing `DISTINCT TOTALS`
/// line sums the three stock populations only; the per-population lines above it are printed as
/// they are measured, duplicates included, and say so. See [`query_populations`].
///
/// # What is asserted, clause by clause
///
/// - **Every shore member** of a shore-point body answers *that* body, by kind and by id. A member
///   is a submerged node of the body by construction, so this holds exactly -- 142 of 142 distinct
///   -- except for Ruling Q-12's case, a member the ground stands above its
///   own body's level: an extent suppresses the ocean while a *claim* decides the body, so such a
///   member is dry ground, `None`, and never `Ocean`. It is asserted rather than assumed away, and
///   the count is reported either way. (It is zero everywhere measured, and it is asserted anyway
///   because the ring bodies below show the case is real.)
/// - **Every body's anchor** (Ruling Q-14) answers some body. This is the assertion a point-only
///   property cannot make: every recorded point of a body is on its shore by construction, so a
///   hole in the *interior* -- exactly the defect Ruling Q-13 fixed in the index -- is invisible
///   from the outline alone. The anchor is the body's deepest node, interior by construction.
/// - **Every ring vertex** of a body recorded as a traced curve: not `Ocean`, and where the body
///   itself answers, its own recorded kind. It is *not* asserted that some body answers, and the
///   counts say why. Ruling Q-17 closed one reason -- a vertex projects to the crossing count's own
///   ray origin, and 88 vertices were claimed by nothing until the boundary was made explicitly
///   inside; that count is now zero. The other reason is not a defect and does not go away: **a
///   ring IS the shoreline contour**, so a vertex stands on the boundary between a filled cell and
///   an unfilled one and is as likely to be a little above its own level as a little below. Ruling
///   Q-16 made that comparison ask the surface the level was written against, which dropped the
///   *magnitude* from 45.8 m worst / 20.8 m mean to 1.28 m / 0.41 m while barely moving the count
///   (144 of 226 to 141 of 226). Both the count and the magnitude are printed, because it is the
///   magnitude that tells a resolution residual from a wrong surface. The count itself has a
///   mechanism, and it is not "about half": a vertex is a cell **corner**, placed by `ponds.rs` at
///   `(index - 0.5) * pond_cell_m`, half a cell outside the outermost sample the search tested as
///   wet -- so it is biased proud by construction, and 141 of 226 is ~3.7 sigma from a coin flip.
/// - **Every collar point**: how many answer their own body is reported, because the band admits
///   some by design, and no point may answer a body whose level is *below* the landform there.
/// - **Every reach point** answers `River`, and then **Ruling Q-18's `reach_id` says which reach**
///   -- so "its own reach" and "a level that happens to match" are two questions, and on the
///   branch where the reach names itself the `bed_m + depth_m` check is a hard assertion within
///   1e-6. The alternatives are a *different* reach (Ruling Q-11), a body where the reach ends in
///   one, or the sea at its mouth. All four are counted, and a point answering the sea anywhere
///   but at the reach's last point is a failure.
/// - **Every leg's midpoint** answers `River` unless a body claims it, or the sea on a last leg.
///   This is the case a point-only property misses: a recorded point is where the record is right
///   by definition.
/// - **Every notch point**: *no* notch point answers a body -- a notch is a cut through a rim, not
///   the lake it drains -- and one that answers the sea must stand at or below the datum on this
///   test's own landform closure. **And no notch point is dry** (Ruling C-35): a notch carries a
///   lake's outflow, and the query answers `River` in its footprint -- before C-35 it never read
///   `Candidates::notches`, and a notch point no reach ran through answered `None`.
///
/// Every count is printed. They are Task 6's verification table, and two of them -- ring vertices
/// standing above their own recorded level, and vertices claimed by nothing -- are what produced
/// Rulings Q-16 and Q-17.
#[test]
fn the_query_agrees_with_the_record_at_every_recorded_point() {
    let mut ponds_seen = 0usize;
    let mut total = Distinct::default();
    for (name, distinct, surface, params) in query_populations() {
        let record = crate::hydrology::bake(&surface, &params).expect("bake");
        let index = WaterIndex::build(&record, surface.radius_m, DEFAULT_CELL_M);
        // Ruling Q-3: the LANDFORM for a coarse body, a reach and the ocean -- every level and
        // bed the coarse bake wrote is landform-derived, so the level test must ask the same
        // surface. Ruling Q-16: the DETAIL FIELD at `pond_cell_m` for a body the fine search
        // found, because Ruling S-9 levelled it off exactly that, and `ponds::pond_ground` is
        // the very closure that did it.
        let landform = |p: &SpherePoint| surface.structural_m(p);
        let detail = crate::hydrology::ponds::pond_ground(&surface, &params);
        let ground = Ground { landform_m: Landform(&landform), detail_m: Detail(&detail) };
        let ask = |p: &SpherePoint| water_at(&record, &index, &ground, p);
        // The surface the query itself compares THIS body's level against (Ruling Q-16). Every
        // "stands above its own level" test below asks through this rather than through
        // `landform`, because asking the wrong surface is the defect this ruling fixed.
        let level_ground = |body: &Body, p: &SpherePoint| {
            if body.shore_member_count == 0 { detail(p) } else { landform(p) }
        };

        let (mut members, mut member_dry) = (0usize, 0usize);
        let (mut collars, mut collar_own, mut collar_other, mut collar_dry) =
            (0usize, 0usize, 0usize, 0usize);
        let (mut anchors, mut anchor_own, mut anchor_other, mut anchor_above) =
            (0usize, 0usize, 0usize, 0usize);
        let (mut rings, mut ring_ponds) = (0usize, 0usize);
        let (mut vertices, mut vertex_own, mut vertex_other, mut vertex_above,
             mut vertex_unclaimed) = (0usize, 0usize, 0usize, 0usize, 0usize);
        // How far above, not just how many: a ring IS the shoreline contour, so a vertex sitting
        // a few centimetres proud of its own level is the trace's own resolution and not a wrong
        // surface. Reported so Task 6 can tell those two apart at a glance.
        let mut vertex_above_max_m = 0.0f64;
        let mut vertex_above_sum_m = 0.0f64;

        for body in &record.bodies {
            let member_count = body.shore_member_count as usize; // cast-ok: a recorded count
            let want = recorded_kind(body.kind);

            // Ruling Q-14: the anchor, which is the one interior point the record names.
            let anchor = SpherePoint::from_latlon(body.anchor.0, body.anchor.1);
            let got = ask(&anchor);
            anchors += 1;
            // Ruling Q-13's own property, and the reason Q-14 asks about the anchor at all:
            // **is the body even offered here?** It is a question about the index alone, so it is
            // asked unconditionally, before any level branch. Without it the branch below is
            // satisfied by a missing candidate -- a body that is in no cell at its own anchor
            // answers `None`, which is not `Ocean`, and the assertion there would pass. The
            // interior is exactly where Q-13's hole was, and no outline point can see it.
            assert!(index.candidates(&anchor).bodies.contains(&body.id),
                    "{name}: body {} is not even a candidate at its own anchor {},{} (Q-13)",
                    body.id, body.anchor.0, body.anchor.1);
            if level_ground(body, &anchor) > body.level_m {
                // Ruling Q-12 again. This branch used to be where the Q-16 defect showed --
                // 10 of 41 anchors, because a fine-search body's level was compared against
                // `structural_m` when Ruling S-9 had levelled it off the detail field. With
                // `level_ground` asking the surface the query asks, it is empty everywhere
                // measured. It is kept, and kept strict about the one thing that matters if a
                // body ever does stand above its own level: whatever it is, it is not sea.
                anchor_above += 1;
                assert_ne!(got.kind, WaterKind::Ocean,
                           "{name}: body {}'s own anchor {},{} answers Ocean -- an extent \
                            suppresses the ocean whether or not the body claims (Q-12)",
                           body.id, body.anchor.0, body.anchor.1);
            } else {
                assert!(is_body(got.kind),
                        "{name}: body {} answers {:?} at its own anchor {},{}, where the landform \
                         is {} m under its level {} m -- an interior hole",
                        body.id, got.kind, body.anchor.0, body.anchor.1,
                        body.level_m - level_ground(body, &anchor), body.level_m);
                if got.body_id == body.id {
                    assert_eq!(got.kind, want,
                               "{name}: body {} is recorded {:?} and answers {:?} at its own \
                                anchor", body.id, body.kind, got.kind);
                    anchor_own += 1;
                } else {
                    anchor_other += 1;
                }
            }

            if member_count == 0 {
                rings += 1;
                if body.kind == BodyKind::Pond {
                    ring_ponds += 1;
                }
            }

            for (i, &(lat, lon)) in body.outline.iter().enumerate() {
                let point = SpherePoint::from_latlon(lat, lon);
                let got = ask(&point);
                if member_count == 0 {
                    // A traced ring's vertex: its own body, or a neighbour by Ruling T1-3.
                    vertices += 1;
                    // The same unconditional candidacy question as at the anchor. Every branch
                    // below is about the *claim*; this one is about the index, and without it a
                    // vertex the index lost would fall through the `above its own level` branch
                    // and assert nothing at all.
                    assert!(index.candidates(&point).bodies.contains(&body.id),
                            "{name}: body {} is not even a candidate at its own ring vertex \
                             {lat},{lon} (Q-13)", body.id);
                    if level_ground(body, &point) > body.level_m {
                        vertex_above += 1; // the anchor's case, on the ring
                        let over = level_ground(body, &point) - body.level_m;
                        vertex_above_sum_m += over;
                        if over > vertex_above_max_m { vertex_above_max_m = over; }
                    } else if is_body(got.kind) {
                        if got.body_id == body.id {
                            assert_eq!(got.kind, want,
                                       "{name}: body {} is recorded {:?} and answers {:?} at its \
                                        own ring vertex {lat},{lon}", body.id, body.kind, got.kind);
                            vertex_own += 1;
                        } else {
                            vertex_other += 1;
                        }
                    } else {
                        // A vertex sits *on* its own ring, where an even-odd crossing count is at
                        // its least decisive -- the vertex projects to the ray's own origin.
                        vertex_unclaimed += 1;
                    }
                    assert_ne!(got.kind, WaterKind::Ocean,
                               "{name}: body {}'s own ring vertex {lat},{lon} answers Ocean",
                               body.id);
                } else if i < member_count {
                    members += 1;
                    if level_ground(body, &point) > body.level_m {
                        // Ruling Q-12: inside its own extent, above its own level. The extent
                        // suppresses the ocean; the claim, which fails, is what would have
                        // decided the body. So it is dry ground, not sea.
                        member_dry += 1;
                        assert_eq!(got.kind, WaterKind::None,
                                   "{name}: body {}'s member {lat},{lon} stands {} m above its \
                                    own level {} m, so it is dry -- not {:?}",
                                   body.id, level_ground(body, &point) - body.level_m,
                                   body.level_m, got.kind);
                    } else {
                        assert_eq!(got.body_id, body.id,
                                   "{name}: body {}'s own shore member {lat},{lon} answers body \
                                    {} ({:?})", body.id, got.body_id, got.kind);
                        assert_eq!(got.kind, want,
                                   "{name}: body {} is recorded {:?} and answers {:?} at its own \
                                    member {lat},{lon}", body.id, body.kind, got.kind);
                    }
                } else {
                    collars += 1;
                    if is_body(got.kind) {
                        let claimed = record.bodies.iter().find(|b| b.id == got.body_id)
                            .expect("the query answered a body id the record does not carry");
                        assert!(claimed.level_m >= level_ground(claimed, &point),
                                "{name}: collar point {lat},{lon} answers body {} at level {} m, \
                                 under the ground's {} m",
                                claimed.id, claimed.level_m, level_ground(claimed, &point));
                        if got.body_id == body.id { collar_own += 1 } else { collar_other += 1 }
                    } else {
                        collar_dry += 1;
                    }
                }
            }
        }

        let (mut reach_points, mut reach_own, mut reach_confluence, mut reach_in_body) =
            (0usize, 0usize, 0usize, 0usize);
        let (mut reach_sea_last, mut reach_sea_inland) = (0usize, 0usize);
        let (mut midpoints, mut mid_river, mut mid_body, mut mid_sea) =
            (0usize, 0usize, 0usize, 0usize);
        for reach in &record.reaches {
            let last = reach.points.len().saturating_sub(1);
            for (i, rp) in reach.points.iter().enumerate() {
                let point = SpherePoint::from_latlon(rp.lat_deg, rp.lon_deg);
                let got = ask(&point);
                reach_points += 1;
                if got.kind == WaterKind::River {
                    // **Ruling Q-18 is what makes this an assertion rather than a guess.** Which
                    // reach answered is now named, so "its own reach" and "the level happens to
                    // match" are two different questions, and the level check is a real assertion
                    // on the branch where it belongs. Before `reach_id` existed, a different reach
                    // carrying a numerically equal level counted silently as this reach's own.
                    if got.reach_id == reach.id {
                        let want = rp.bed_m + rp.depth_m;
                        let off = if got.level_m > want { got.level_m - want }
                                  else { want - got.level_m };
                        assert!(off <= 1.0e-6,
                                "{name}: reach {} answers its own point {},{} at level {} m, and \
                                 the record says bed {} m plus depth {} m",
                                reach.id, rp.lat_deg, rp.lon_deg, got.level_m, rp.bed_m, rp.depth_m);
                        reach_own += 1;
                    } else {
                        // Ruling Q-11: the nearer centre line wins, ties to the lower reach id.
                        // Both halves are reachable at a recorded point -- see the closing note.
                        reach_confluence += 1;
                    }
                } else if is_body(got.kind) {
                    // A reach that ends in a body has its last point inside it, by construction.
                    reach_in_body += 1;
                } else if got.kind == WaterKind::Ocean {
                    // The same case at the coast rather than at a lake shore: a reach that
                    // reaches the sea has its mouth in it, and Ruling Q-5 puts the ocean ahead
                    // of the river. Counted by whether it is the mouth, because a *midstream*
                    // point answering `Ocean` would be a different thing entirely.
                    if i == last { reach_sea_last += 1 } else { reach_sea_inland += 1 }
                } else {
                    panic!("{name}: reach {} answers {:?} at its own point {},{}",
                           reach.id, got.kind, rp.lat_deg, rp.lon_deg);
                }
            }

            // Step 2: between the points too. A recorded point is where the record is right by
            // definition; the middle of a leg is where a width rule or a band can be wrong.
            for leg in 0..reach.points.len().saturating_sub(1) {
                let a = SpherePoint::from_latlon(reach.points[leg].lat_deg,
                                                 reach.points[leg].lon_deg);
                let b = SpherePoint::from_latlon(reach.points[leg + 1].lat_deg,
                                                 reach.points[leg + 1].lon_deg);
                let mid = midpoint(&a, &b);
                let got = ask(&mid);
                midpoints += 1;
                if got.kind == WaterKind::River {
                    mid_river += 1;
                } else if is_body(got.kind) {
                    mid_body += 1;
                } else if got.kind == WaterKind::Ocean && leg + 1 == reach.points.len() - 1 {
                    // The last leg of a reach that ends in the sea: half of it is already sea.
                    mid_sea += 1;
                } else {
                    panic!("{name}: reach {} answers {:?} at the midpoint of leg {leg}, between \
                            its recorded points {},{} and {},{}",
                           reach.id, got.kind, reach.points[leg].lat_deg,
                           reach.points[leg].lon_deg, reach.points[leg + 1].lat_deg,
                           reach.points[leg + 1].lon_deg);
                }
            }
        }

        // A notch carries a lake's outflow through its rim, and since Ruling C-35 the query answers
        // `River` in its footprint -- the reach that runs through the cut where one does, the
        // notch's own water where none does.
        //
        // Three of the four outcomes are assertions, not counters. **No notch point is dry**: that
        // was every notch point no reach ran through, before C-35. **No notch point answers a
        // body**: a notch is cut through a rim to drain a hollow, so standing water at one would
        // say the cut runs through the lake it drains. And a notch point that answers the sea
        // must actually stand at or below the datum -- Ruling Q-4's own clause, checked against
        // this test's own landform closure rather than taken on the query's word, so `Ocean`
        // cannot be accepted anywhere the ocean has no business being.
        let (mut notch_points, mut notch_river, mut notch_dry) = (0usize, 0usize, 0usize);
        let (mut notch_sea, mut notch_body) = (0usize, 0usize);
        for notch in &record.notches {
            for &(lat, lon, _, _) in &notch.points {
                let point = SpherePoint::from_latlon(lat, lon);
                let got = ask(&point);
                notch_points += 1;
                match got.kind {
                    WaterKind::River => notch_river += 1,
                    WaterKind::None => notch_dry += 1,
                    WaterKind::Ocean => {
                        assert!(landform(&point) <= 0.0,
                                "{name}: a notch point at {lat},{lon} answers Ocean on landform \
                                 standing {} m above the datum", landform(&point));
                        notch_sea += 1;
                    }
                    _ => notch_body += 1,
                }
            }
        }
        assert_eq!(notch_body, 0,
                   "{name}: {notch_body} notch points stand in a recorded body -- a notch is a \
                    cut through a rim, not the lake it drains");
        assert_eq!(notch_dry, 0,
                   "{name}: {notch_dry} of {notch_points} notch points answer dry -- a notch \
                    carries its lake's outflow (Ruling C-35)");

        eprintln!(
            "{name}: {} bodies ({rings} traced rings, {ring_ponds} of them ponds) / {} reaches / \
             {} notches.\n  \
             shore members {members}, of which {member_dry} stand above their own level (Q-12);\n  \
             anchors {anchors}: {anchor_own} answer their own body, {anchor_other} a neighbour \
             (Q-14), {anchor_above} stand above their own level;\n  \
             ring vertices {vertices}: {vertex_own} their own body, {vertex_other} a neighbour \
             (T1-3), {vertex_above} above their own level (at most {vertex_above_max_m:.3} m, \
             {:.3} m mean -- a ring IS the shoreline contour, so a vertex marginally proud of \
             its own level is the 250 m trace's own resolution, not a wrong surface), \
             {vertex_unclaimed} claimed by nothing;\n  \
             collar points {collars}: {collar_own} their own body, {collar_other} another, \
             {collar_dry} not water;\n  \
             reach points {reach_points}: {reach_own} their own reach, {reach_confluence} another \
             reach (Q-11), {reach_in_body} a body, {reach_sea_last} the sea at a mouth, \
             {reach_sea_inland} the sea midstream;\n  \
             leg midpoints {midpoints}: {mid_river} river, {mid_body} a body, {mid_sea} the sea \
             on a last leg;\n  \
             notch points {notch_points}: {notch_river} river, {notch_dry} dry, {notch_sea} sea, \
             {notch_body} a body.",
            record.bodies.len(), record.reaches.len(), record.notches.len(),
            // cast-ok: a count of ring vertices into a float, for a mean
            if vertex_above > 0 { vertex_above_sum_m / vertex_above as f64 } else { 0.0 });

        // Not a vacuous pass: the population really does record all three families, and the
        // interior sample and the between-the-points sample really did run.
        assert!(!record.bodies.is_empty(), "{name}: no bodies to check");
        assert!(!record.reaches.is_empty(), "{name}: no reaches to check");
        assert!(!record.notches.is_empty(), "{name}: no notches to check");
        assert!(members > 0, "{name}: no shore member to check");
        assert!(anchors > 0, "{name}: no anchor to check");
        assert!(midpoints > 0, "{name}: no leg midpoint to check");
        assert!(notch_points > 0, "{name}: no notch point to check");
        // A mouth in the sea is Ruling Q-5 working. A point *midstream* answering the sea is the
        // river running under the datum with no body's extent over it, which is a different
        // thing entirely, and is what makes the mouth count benign rather than alarming.
        assert_eq!(reach_sea_inland, 0,
                   "{name}: {reach_sea_inland} recorded reach points answer Ocean somewhere other \
                    than at the reach's last point");
        ponds_seen += ring_ponds;
        if distinct {
            total.members += members;
            total.member_dry += member_dry;
            total.anchors += anchors;
            total.anchor_own += anchor_own;
            total.anchor_above += anchor_above;
            total.vertices += vertices;
            total.vertex_own += vertex_own;
            total.vertex_above += vertex_above;
            total.vertex_unclaimed += vertex_unclaimed;
            total.vertex_above_sum_m += vertex_above_sum_m;
            if vertex_above_max_m > total.vertex_above_max_m {
                total.vertex_above_max_m = vertex_above_max_m;
            }
            total.collars += collars;
            total.collar_own += collar_own;
            total.reach_points += reach_points;
            total.reach_own += reach_own;
            total.reach_confluence += reach_confluence;
            total.midpoints += midpoints;
            total.notch_points += notch_points;
        }
    }

    // Ruling Q-16's residual, measured rather than shrugged at. A ring vertex is a cell
    // **corner** -- `ponds.rs`'s trace places it at `(index - 0.5) * pond_cell_m`, half a cell
    // back from the centre the search actually sampled -- so it stands half a cell outside the
    // outermost wet sample, on ground no sample ever tested as submerged. It is therefore biased
    // to stand *proud* of its own level by construction, and the measured share is not a coin
    // flip: 141 of 226 is 62.4%, which against a fair coin's 113 +/- 7.52 is about 3.7 standard
    // deviations out. The corner offset is what explains the bias; the excess magnitude is what
    // says the surfaces agree, and it is a metre and change on a 250 m trace.
    let mean_above_m = if total.vertex_above > 0 {
        // cast-ok: a count of ring vertices into a float, for a mean
        total.vertex_above_sum_m / total.vertex_above as f64
    } else {
        0.0
    };
    eprintln!(
        "DISTINCT TOTALS over the three stock populations (the fourth is a re-bake of the second \
         and is excluded; only its pond count is new):\n  \
         shore members {} ({} above their own level), collar points {} ({} their own body);\n  \
         anchors {} ({} their own body, {} above their own level);\n  \
         ring vertices {} ({} their own body, {} above their own level -- at most {:.3} m, \
         {mean_above_m:.3} m mean -- {} claimed by nothing);\n  \
         reach points {} ({} their own reach by reach_id, {} another reach), leg midpoints {}, \
         notch points {}.\n  \
         Ponds of kind `Pond` across every population: {ponds_seen}.",
        total.members, total.member_dry, total.collars, total.collar_own,
        total.anchors, total.anchor_own, total.anchor_above,
        total.vertices, total.vertex_own, total.vertex_above, total.vertex_above_max_m,
        total.vertex_unclaimed,
        total.reach_points, total.reach_own, total.reach_confluence, total.midpoints,
        total.notch_points);

    // The pond clause is answered by a real `Pond` somewhere, which is what the fourth population
    // is for; without it this whole property says nothing about ponds.
    assert!(ponds_seen > 0, "no population recorded a body of kind Pond");

    // **Ruling Q-11's allowance is exercised, and the earlier claim that it could not be was
    // wrong twice over.** The argument was that a reach's own centre line passes through its own
    // recorded point at distance zero, so no other reach can be nearer. Both halves fail: the
    // tie-break at an equal distance goes to the LOWER reach id, so a tributary whose last point
    // coincides with a main-stem point sits at zero too and the lower id takes it; and the old
    // discriminator was "the level differs", which counted a different reach carrying a
    // numerically equal level as this reach's own. Ruling Q-18's `reach_id` settles both, and the
    // count went from a reported 0 to a measured 35 of 4,082 river answers the moment it did.
    assert!(total.reach_confluence > 0,
            "no recorded reach point is answered by another reach: Ruling Q-11's tie-break is \
             asserting nothing on these populations");
}

/// Re-derives the anchors `tests/test_conformance.py`'s water section pins the PyO3 door against.
///
/// The Python door has no reference implementation to compare with -- nothing under
/// `worldbuilder/` bakes water -- so it is pinned against values this crate produced. Those
/// values have to come from a **run**, and until plan 2a's final review the instruction for
/// getting them was "add a temporary `eprintln!`", which is how one anchor ended up being all
/// there was. This prints every one of them, for every kind the record actually holds, from the
/// same `bake_tests::world()`/`params()` population and through the same `water_at` the binding
/// calls. Pond is not among them: this population records none that the query answers as `Pond`,
/// and the parity work already settled that `BodyKind::Pond` never crosses a wire here.
///
/// Ignored because it asserts almost nothing -- it is an instrument. Run it with
/// `cargo test --release -p worldbuilder-engine --lib print_the_python_doors_anchors -- --ignored --nocapture`
/// and copy what it prints into the constants in `tests/test_conformance.py`.
#[test]
#[ignore = "instrument: prints the Python door's anchors; run with --ignored --nocapture"]
fn print_the_python_doors_anchors() {
    let surface = crate::hydrology::bake_tests::world();
    let mut params = HydroParams::earth_like(20_000);
    params.wetness_nodes = 500;
    params.keep_depth_m = 8.0;
    params.keep_area_m2 = 1.0e6;
    params.pond_max_area_m2 = 1.0e6;
    params.stream_flow_m2 = 3.0e10;
    params.river_flow_m2 = 3.0e11;
    params.great_flow_m2 = 3.0e12;
    params.notch_fall_m = 1.0;
    params.evaporation_factor = 1.0;
    params.salt_flat_share = 0.1;
    let record = crate::hydrology::bake(&surface, &params).expect("bake");
    println!("bodies: {}, reaches: {}", record.bodies.len(), record.reaches.len());
    let index = WaterIndex::build(&record, surface.radius_m, DEFAULT_CELL_M);
    let landform = |p: &SpherePoint| surface.structural_m(p);
    let detail = crate::hydrology::ponds::pond_ground(&surface, &params);
    let ground = Ground { landform_m: Landform(&landform), detail_m: Detail(&detail) };

    let name = |kind: WaterKind| match kind {
        WaterKind::None => "none",
        WaterKind::Ocean => "ocean",
        WaterKind::Lake => "lake",
        WaterKind::SaltLake => "salt_lake",
        WaterKind::SaltFlat => "salt_flat",
        WaterKind::Pond => "pond",
        WaterKind::River => "river",
    };
    let report = |what: &str, lat: f64, lon: f64| {
        let answer = water_at(&record, &index, &ground, &SpherePoint::from_latlon(lat, lon));
        println!("{what}: kind={} level_m={:?} depth_m={:?} fresh={} body_id={} reach_id={} \
                  lat={lat:?} lon={lon:?}",
                 name(answer.kind), answer.level_m, answer.depth_m, answer.fresh,
                 answer.body_id, answer.reach_id);
    };

    // One body per recorded kind, the lowest id of each, asked at its own anchor -- Ruling Q-14's
    // point: the one interior point the record itself names.
    for want in [BodyKind::Lake, BodyKind::SaltLake, BodyKind::SaltFlat, BodyKind::Pond] {
        match record.bodies.iter().find(|b| b.kind == want) {
            Some(body) => {
                let fine = body.shore_member_count == 0;
                report(&format!("{want:?} body {} (fine-found: {fine})", body.id),
                       body.anchor.0, body.anchor.1);
            }
            None => println!("{want:?}: this population records none"),
        }
    }
    // The lowest-id body the FINE search found, whose level the query reads off the detail field
    // (Ruling Q-16) rather than the landform -- a different branch of `water_at` from the ones
    // above, whatever kind it wears.
    match record.bodies.iter().find(|b| b.shore_member_count == 0) {
        Some(body) => report(&format!("fine-found body {} ({:?})", body.id, body.kind),
                             body.anchor.0, body.anchor.1),
        None => println!("fine-found: this population records none"),
    }
    // A river: the midpoint-most recorded point of the lowest-id reach, which is as far from a
    // mouth or a confluence as a recorded point gets (Ruling Q-11's tie-break and the sea at the
    // mouth are the two things that make an endpoint answer something other than its own reach).
    match record.reaches.first() {
        Some(reach) => {
            let p = &reach.points[reach.points.len() / 2];
            report(&format!("reach {} midpoint", reach.id), p.lat_deg, p.lon_deg);
        }
        None => println!("river: this population records no reaches"),
    }
    // The two answers that are not a body: the first point of a 5-degree scan, in a stated
    // order, that answers each. Found by scan rather than picked, because the pole -- which the
    // Python door used to assume was dry -- is inside body 0 on this population.
    for want in [WaterKind::None, WaterKind::Ocean] {
        let mut found = false;
        let mut lat = -85.0;
        while lat <= 85.0 && !found {
            let mut lon = -180.0;
            while lon < 180.0 && !found {
                let answer = water_at(&record, &index, &ground, &SpherePoint::from_latlon(lat, lon));
                if answer.kind == want {
                    report(&format!("first {} of the 5-degree scan", name(want)), lat, lon);
                    found = true;
                }
                lon += 5.0;
            }
            lat += 5.0;
        }
        if !found {
            println!("{}: the 5-degree scan found none", name(want));
        }
    }
}
