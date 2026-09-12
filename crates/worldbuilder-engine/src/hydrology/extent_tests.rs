//! Ruling E-2's trim on trial: the design note's §5.7 test, and what it measured.
//!
//! `extent.rs` records only a body's *shore* members, on the argument that they surround the
//! interior, so an interior point's nearest recorded point is always a member and never a collar
//! node. §5.7 is explicit that **sampling member positions cannot see that argument fail** — a
//! member is trivially nearest to itself. §5.7 expects the failure mode to be a point *between*
//! two interior members landing in a collar node's Voronoi cell, so these tests sample between the
//! members: the midpoint of every member-member graph edge, and the centroid of every member with
//! its member neighbours.
//!
//! **The trim holds** (plan 1b-4 Task 3; full numbers in that task's report). On every sample that
//! is genuinely over its own body's water, §5.6's first clause — nearest recorded member at least
//! as near as nearest recorded collar point — holds, on all six populations from 12,000 nodes to
//! 1,000,000: **0 misses of 74,904 samples.** `outside_clause_1_over_own_water` asserts it.
//!
//! **§5.7's sample set over-reaches, and that accounts for all 284 raw misses.** The midpoint of a
//! member-member edge need not lie over either member's cell: on an irregular k-nearest graph two
//! members can be mutual neighbours with a *non-member* node sitting spatially between them, and
//! the midpoint then falls in that node's cell — dry ground between two lake cells, not interior
//! water. Filtering samples by their nearest node in the whole graph removes exactly the misses and
//! nothing else: per population the raw miss count equals `sampled - over_own_water` to the unit
//! (1 = 57-56, 1 = 57-56, 4 = 321-317, 64 = 9,867-9,803, 73 = 11,787-11,714, 141 = 52,815-52,674).
//! So what §5.7 got wrong is its probe, not §5.6's containment argument — and the failure mode it
//! names, a point between two *interior* members, is not the one the probe actually finds.
//!
//! **Ruling E-5 is therefore not needed, and would not have helped anyway.** Applied and measured
//! on all six populations, recording every interior member leaves every miss count and every worst
//! distance identical while growing the record by up to 120% (`recorded_points` 2,215 to 3,827 on
//! the plain million-node world, 7,132 to 15,649 on the million-node `ranges` world).
//!
//! **What the tests assert is the extent §8.3 actually defines** — clause 1 *or* clause 2,
//! `dm <= dc || dm <= shore_reach_m` — because that disjunction is what ships. It holds on all
//! 74,904 samples, the over-reaching ones included. The raw clause-1 count is printed beside it and
//! guarded non-zero, so this account of why it is non-zero cannot go stale unnoticed.
//!
//! **The 200,000-node population is not decoration**, for the same reason
//! `no_bodys_band_counts_a_step_below_its_level` carries one. At 12,000 nodes the band is so wide
//! against the graph that the extent assertion is vacuous: recording only half of every shore
//! member (Task 3's Step 5 mutation) still leaves 0 samples outside the extent on all three
//! 12,000-node populations. On the seed 1 `ranges` world at 200,000 that same mutation puts 403 of
//! 9,812 samples outside, 278.8 km past the band, so the assertion has teeth.
//!
//! `params` and `junction_params` are the same graph — `min_stream_nodes` changes reach extraction
//! and nothing the extent reads — so the six populations are four distinct ones. Both are kept
//! because `refined_populations` is the suite's standard population set.

use crate::hydrology::bake::{bake_stages, record_of};
use crate::hydrology::buckets::BucketIndex;
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
    /// Every point of `outline` this population records, summed over the bodies below. The witness
    /// that an `extent_of` change actually took: Ruling E-5 roughly triples it.
    recorded_points: usize,
    sampled: usize,
    /// Of `sampled`, those whose nearest node **in the whole graph** is a member of their own body
    /// — the ones that really are over that body's interior water. `BucketIndex` answers this the
    /// way `hollows::nearest_forced_nodes` does; a scan of the body's own points is a different
    /// question and would not do.
    over_own_water: usize,
    /// Samples whose nearest recorded member is further than their nearest recorded collar point:
    /// §8.3's first clause alone.
    outside_clause_1: usize,
    /// Of `outside_clause_1`, those that are also `over_own_water`. This is the count that decides
    /// whether clause 1 fails on *interior water* or only on the probe's over-reach.
    outside_clause_1_over_own_water: usize,
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

    // Which node's cell a sample actually falls in, over the WHOLE graph. Scanning only the body's
    // own recorded points answers a different question, so this is the same `BucketIndex` walk
    // `hollows::nearest_forced_nodes` uses, built once for the bake.
    let spacing = crate::stream::nominal_spacing_m(graph.len() as u32, radius); // cast-ok: node count fits in u32 by construction
    let mut node_index = BucketIndex::new(radius, spacing);
    for (n, position) in graph.positions.iter().enumerate() {
        node_index.insert(position, n as u32); // cast-ok: node index
    }

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
        trial.recorded_points += body.outline.len();

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
            // Is this sample actually over the body's own water, by the same nearest-node reading
            // §8.3 uses for the landform? A midpoint between two members can sit in a non-member
            // node's cell -- dry ground between two lake cells -- which is not interior water.
            let over_own_water = match node_index.nearest(q, &graph.positions) {
                Some(node) => mine(node),
                None => false,
            };
            if over_own_water {
                trial.over_own_water += 1;
            }
            if dm <= dc {
                continue;
            }
            trial.outside_clause_1 += 1;
            if over_own_water {
                trial.outside_clause_1_over_own_water += 1;
            }
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
    eprintln!("{name}: {} bodies, {} trimmed, {} recorded points | {} samples, {} over own water \
               | {} outside clause 1 (worst by {:.1} m), of those {} over own water | {} outside the extent",
              t.bodies, t.trimmed_bodies, t.recorded_points, t.sampled, t.over_own_water,
              t.outside_clause_1, t.worst_clause_1_m, t.outside_clause_1_over_own_water,
              t.outside_extent);
    assert!(t.sampled > 0, "{name}: nothing sampled");
    assert!(t.over_own_water > 0, "{name}: no sample was over its own body's water");

    // The headline finding. §5.6's first clause holds on every sample that is really over the
    // body's water; the misses are all samples the §5.7 probe reaches past that water.
    assert_eq!(t.outside_clause_1_over_own_water, 0,
               "{name}: {} samples over their own body's water failed §8.3's first clause",
               t.outside_clause_1_over_own_water);
    // ...and the probe does over-reach, on every population. Without this the docstring's account
    // of why `outside_clause_1` is non-zero could go stale silently.
    assert!(t.outside_clause_1 > 0,
            "{name}: the probe no longer over-reaches; this module's docstring needs rewriting");
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
/// the seed 1 `ranges` world.
///
/// `#[ignore]`d for the reason the rest of this suite's million-node work is: two bakes of that
/// size are seconds in `--release` (about 21 s together, measured) and minutes in the debug profile
/// CI runs `cargo test` under. Run it with
/// `cargo test --release -p worldbuilder-engine --lib the_trim_holds_at_a_million_nodes --
/// --ignored --nocapture`.
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
