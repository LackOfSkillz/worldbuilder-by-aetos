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
use crate::water::query::{water_at, Ground, WaterKind};

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
fn query_populations() -> Vec<(String, Surface, HydroParams)> {
    let mut out: Vec<(String, Surface, HydroParams)> = refined_populations().into_iter()
        .map(|(name, surface, params)| (name.to_string(), surface, params))
        .collect();
    let (name, surface, mut params) = refined_populations().into_iter().nth(1)
        .expect("junction_params");
    params.pond_max_area_m2 = 5.0e6;
    out.push((format!("{name} at a 5 km² pond threshold"), surface, params));
    out
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
/// # What is asserted, clause by clause
///
/// - **Every shore member** of a shore-point body answers *that* body, by kind and by id. A member
///   is a submerged node of the body by construction, so this holds exactly -- 169 of 169 across
///   the four populations -- except for Ruling Q-12's case, a member the landform stands above its
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
///   magnitude that tells a resolution residual from a wrong surface.
/// - **Every collar point**: how many answer their own body is reported, because the band admits
///   some by design, and no point may answer a body whose level is *below* the landform there.
/// - **Every reach point** answers `River` at that point's own `bed_m + depth_m` within 1e-6, or
///   -- Ruling Q-11, where two reaches cover a point the nearer centre line wins -- a *different*
///   reach at a confluence, or a body where the reach ends in one, or the sea at its mouth. All
///   four are counted, and a point answering the sea anywhere but at the reach's last point is a
///   failure.
/// - **Every leg's midpoint** answers `River` unless a body claims it, or the sea on a last leg.
///   This is the case a point-only property misses: a recorded point is where the record is right
///   by definition.
/// - **Every notch point** is counted four ways. A notch is a cut, not standing water, and §8.3's
///   table has no notch row -- the query never reads `Candidates::notches` -- so a notch point
///   answers whatever the other families say there, `River` or `None` in the ordinary case and
///   the sea where the cut runs under the datum.
///
/// Every count is printed. They are Task 6's verification table, and two of them -- ring vertices
/// standing above their own recorded level, and vertices claimed by nothing -- are what produced
/// Rulings Q-16 and Q-17.
#[test]
fn the_query_agrees_with_the_record_at_every_recorded_point() {
    let mut ponds_seen = 0usize;
    let mut confluences_seen = 0usize;
    for (name, surface, params) in query_populations() {
        let record = crate::hydrology::bake(&surface, &params).expect("bake");
        let index = WaterIndex::build(&record, surface.radius_m, DEFAULT_CELL_M);
        // Ruling Q-3: the LANDFORM for a coarse body, a reach and the ocean -- every level and
        // bed the coarse bake wrote is landform-derived, so the level test must ask the same
        // surface. Ruling Q-16: the DETAIL FIELD at `pond_cell_m` for a body the fine search
        // found, because Ruling S-9 levelled it off exactly that, and `ponds::pond_ground` is
        // the very closure that did it.
        let landform = |p: &SpherePoint| surface.structural_m(p);
        let detail = crate::hydrology::ponds::pond_ground(&surface, &params);
        let ground = Ground { landform_m: &landform, detail_m: &detail };
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
                    let want = rp.bed_m + rp.depth_m;
                    let off =
                        if got.level_m > want { got.level_m - want } else { want - got.level_m };
                    // Ruling Q-11: the nearer centre line wins, so a point recorded on this reach
                    // may legitimately answer another one at a confluence.
                    if off <= 1.0e-6 { reach_own += 1 } else { reach_confluence += 1 }
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

        // A notch is a cut, not standing water: §8.3's table has no notch row, and the query
        // never reads `Candidates::notches`. So a notch point answers whatever the *other*
        // families say there -- the reach that runs through the cut, or nothing. The two the
        // brief did not anticipate are counted rather than refused: a notch cut at or below the
        // datum is sea by Ruling Q-4, and one inside a recorded extent is that body.
        let (mut notch_points, mut notch_river, mut notch_dry) = (0usize, 0usize, 0usize);
        let (mut notch_sea, mut notch_body) = (0usize, 0usize);
        for notch in &record.notches {
            for &(lat, lon, _, _) in &notch.points {
                let got = ask(&SpherePoint::from_latlon(lat, lon));
                notch_points += 1;
                match got.kind {
                    WaterKind::River => notch_river += 1,
                    WaterKind::None => notch_dry += 1,
                    WaterKind::Ocean => notch_sea += 1,
                    _ => notch_body += 1,
                }
            }
        }

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
        confluences_seen += reach_confluence;
    }
    // The pond clause is answered by a real `Pond` somewhere, which is what the fourth population
    // is for; without it this whole property says nothing about ponds. The Ruling Q-11 allowance,
    // by contrast, is *provably* unreachable at a recorded point -- a reach's own centre line
    // passes through its own point at distance zero, so no other reach can be nearer -- and it is
    // reported at zero rather than engineered into existence. See the task report.
    assert!(ponds_seen > 0, "no population recorded a body of kind Pond");
    eprintln!("across every population: {ponds_seen} traced rings of kind Pond, \
               {confluences_seen} recorded reach points answered by another reach (Q-11)");
}
