//! Ruling E-2's trim on trial: the design note's §5.7 test, and what it measured.
//!
//! `extent.rs` records only a body's *shore* members, on the argument that they surround the
//! interior, so an interior point's nearest recorded point is always a member and never a collar
//! node. §5.7 is explicit that **sampling member positions cannot see that argument fail** — a
//! member is trivially nearest to itself. §5.7 expects the failure mode to be a point *between*
//! two interior members landing in a collar node's Voronoi cell, so these tests sample between the
//! members: the midpoint of every member-member graph edge, and the centroid of every member with
//! its member neighbours. (The misses turned out to be between *shore* members instead — see
//! below.)
//!
//! **What the trial found** (plan 1b-4 Task 3; full numbers in that task's report). §5.6's first
//! clause on its own — nearest recorded member at least as near as nearest recorded collar point —
//! does **not** hold. Its misses, per population: 1 of 57 samples on the bake test world at 12,000
//! nodes under both `params` and `junction_params`, 4 of 321 on the seed 1 `ranges` world at
//! 12,000, 64 of 9,867 on that world at 200,000, 73 of 11,787 on the plain world at 1,000,000, and
//! 141 of 52,815 on the `ranges` world at 1,000,000.
//!
//! **The trim is not the cause, and Ruling E-5 is not the cure.** Recording every interior member
//! as well — E-5's fallback, applied and measured on all six — leaves every count and every worst
//! distance *identical*. It cannot help: an extra interior point only shortens the distance to
//! samples near that interior point, and every failing sample is near the shore, where a collar
//! node happens to sit inside the ring of shore members on an irregular k-nearest graph. Two of
//! the six populations trim nothing at all — every member is already a shore member — and fail
//! anyway, which no amount of extra recording can touch.
//!
//! **What does hold, on every sample of every population, is the extent §8.3 actually defines** —
//! clause 1 *or* clause 2, `dm <= dc || dm <= shore_reach_m`. All 284 clause-1 misses across the
//! six populations are inside the band. That disjunction is what these tests assert; the clause-1
//! count is printed beside it so the trial's finding stays on the record rather than being quietly
//! absorbed. The ruling this leaves open is in the task report: §5.6's containment *argument*
//! credits clause 1 with the interior, and the measurement says the band is carrying part of it.
//!
//! **The 200,000-node population is not decoration**, for the same reason
//! `no_bodys_band_counts_a_step_below_its_level` carries one. At 12,000 nodes the band is so wide
//! against the graph that the extent assertion is vacuous: recording only half of every shore
//! member (Task 3's Step 5 mutation) still leaves 0 samples outside the extent on all three
//! 12,000-node populations. On the seed 1 `ranges` world at 200,000 that same mutation puts 403 of
//! 9,812 samples outside, 278.8 km past the band, so the assertion has teeth.

use crate::hydrology::bake::{bake_stages, record_of};
use crate::hydrology::hollows::Fate;
use crate::hydrology::HydroParams;
use crate::sphere::SpherePoint;
use crate::surface::Surface;

/// What one population's trial saw.
#[derive(Default)]
struct Trial {
    /// Bodies with both a member half and a collar half recorded — the ones clause 1 can be
    /// tested on at all.
    bodies: usize,
    /// Bodies with at least one member the trim dropped. Only these can show the *trim* fail; on a
    /// body where every member is already a shore member, recording the interior changes nothing.
    trimmed_bodies: usize,
    sampled: usize,
    /// Samples whose nearest recorded member is further than their nearest recorded collar point:
    /// §8.3's first clause alone, which the trial found does not hold.
    outside_clause_1: usize,
    /// Samples outside the extent itself — outside clause 1 *and* beyond the band. This is the
    /// count that must be zero.
    outside_extent: usize,
    /// How far the worst clause-1 miss missed by, in metres.
    worst_clause_1_m: f64,
    /// How far the worst extent miss missed by (`dm - shore_reach_m`), in metres.
    worst_extent_m: f64,
}

/// The design note's §5.7 trial. For every kept body, sample the interior *between* its members
/// and test each sample against §8.3's extent.
///
/// Bodies are matched to hollows the way `no_bodys_band_counts_a_step_below_its_level` does — body
/// ids are one per kept hollow, numbered in hollow order — and the anchor is asserted rather than
/// searched, so a change to that numbering is caught here too.
fn interior_samples_stay_inside(surface: &Surface, p: &HydroParams) -> Trial {
    let stages = bake_stages(surface, p).expect("stages");
    let record = record_of(&stages, p);
    let graph = &stages.graph;
    let radius = graph.radius_m;
    let mut trial = Trial::default();
    let mut next_body_id = 0usize;
    for (i, hollow) in stages.hollows.iter().enumerate() {
        if hollow.fate != Fate::Keep {
            continue;
        }
        let body = &record.bodies[next_body_id];
        next_body_id += 1;
        assert_eq!(body.anchor, graph.positions[hollow.floor as usize].to_latlon(),
                   "body {} is the body of hollow {i}", body.id);
        let shore = body.shore_member_count as usize;
        if shore == 0 || shore == body.outline.len() {
            // No collar recorded: every clause-1 test is vacuously satisfied, so there is nothing
            // for this trial to see. `every_coarse_body_carries_an_extent` already forbids it.
            continue;
        }
        trial.bodies += 1;

        // The samples: between the members, never at them (§5.7).
        let mine = |node: u32| stages.routing.lake_of[node as usize] == i as u32; // cast-ok: hollow index
        let mut samples: Vec<SpherePoint> = Vec::new();
        let mut own_members = 0usize;
        for &member in &hollow.members {
            if !mine(member) {
                continue;
            }
            own_members += 1;
            let mut sum = graph.positions[member as usize].vector;
            let mut count = 1.0f64;
            for &next in graph.neighbours(member) {
                if !mine(next) {
                    continue;
                }
                if next > member {
                    // One midpoint per undirected member-member edge.
                    let midpoint = SpherePoint::from_vector(
                        &graph.positions[member as usize].vector.add(&graph.positions[next as usize].vector));
                    if let Some(midpoint) = midpoint {
                        samples.push(midpoint);
                    }
                }
                sum = sum.add(&graph.positions[next as usize].vector);
                count += 1.0;
            }
            if count > 1.0 {
                if let Some(centroid) = SpherePoint::from_vector(&sum.scaled(1.0 / count)) {
                    samples.push(centroid);
                }
            }
        }
        if own_members > shore {
            trial.trimmed_bodies += 1;
        }

        let members: Vec<SpherePoint> =
            body.outline[..shore].iter().map(|&(lat, lon)| SpherePoint::from_latlon(lat, lon)).collect();
        let collar: Vec<SpherePoint> =
            body.outline[shore..].iter().map(|&(lat, lon)| SpherePoint::from_latlon(lat, lon)).collect();
        let nearest = |set: &[SpherePoint], q: &SpherePoint| {
            set.iter().fold(f64::INFINITY, |best, s| {
                let d = s.distance_to(q, radius);
                if d < best { d } else { best }
            })
        };
        for q in &samples {
            let dm = nearest(&members, q);
            let dc = nearest(&collar, q);
            trial.sampled += 1;
            if dm <= dc {
                continue;
            }
            trial.outside_clause_1 += 1;
            let by = dm - dc;
            if by > trial.worst_clause_1_m {
                trial.worst_clause_1_m = by;
            }
            if dm <= body.shore_reach_m {
                continue;
            }
            trial.outside_extent += 1;
            let past = dm - body.shore_reach_m;
            if past > trial.worst_extent_m {
                trial.worst_extent_m = past;
            }
        }
    }
    assert_eq!(next_body_id, record.bodies.len(), "every body is a kept hollow's");
    trial
}

fn check(name: &str, t: &Trial) {
    eprintln!("{name}: {} bodies ({} with interior members trimmed), {} interior samples, \
               {} outside clause 1 (worst by {:.1} m), {} outside the extent",
              t.bodies, t.trimmed_bodies, t.sampled,
              t.outside_clause_1, t.worst_clause_1_m, t.outside_extent);
    assert!(t.sampled > 0, "{name}: nothing sampled");
    assert_eq!(t.outside_extent, 0,
               "{name}: {} of {} interior samples fell outside their own body's extent, worst {:.1} m past the band",
               t.outside_extent, t.sampled, t.worst_extent_m);
}

/// The running population: the three refined worlds at 12,000 nodes, and the seed 1 `ranges` world
/// at 200,000 — which is the only one of the four where the assertion is not vacuous, and the only
/// one where the trim drops anything worth speaking of. See this module's docstring.
#[test]
fn the_trim_holds_on_the_test_worlds() {
    for (name, surface, p) in super::bake_tests::refined_populations() {
        check(name, &interior_samples_stay_inside(&surface, &p));
    }
    let t = interior_samples_stay_inside(&super::bake_tests::ranges_world(),
                                         &HydroParams::earth_like(200_000));
    check("ranges 200k", &t);
    assert!(t.trimmed_bodies > 0, "ranges 200k: no body had an interior member trimmed");
}

/// The population the design note's §5.7 names: a 1,000,000-node bake, on the plain world and on
/// the seed 1 `ranges` world. Ignored because it takes minutes.
#[test]
#[ignore]
fn the_trim_holds_at_a_million_nodes() {
    let p = HydroParams::earth_like(1_000_000);
    for (name, surface) in [("1M plain", super::bake_tests::world()),
                            ("1M ranges", super::bake_tests::ranges_world())] {
        let t = interior_samples_stay_inside(&surface, &p);
        check(name, &t);
        assert!(t.trimmed_bodies > 0,
                "{name}: no body had an interior member trimmed, so this population cannot see the trim fail");
    }
}
