//! The index against real bakes, rather than against a hand-written record.
//!
//! `index.rs`'s own tests pin the rules one at a time on a fixture small enough to reason about.
//! This asks the only question a real record can answer: does the index ever *lose* something?
//! Every recorded body, reach and notch must be a candidate at every one of its own recorded
//! points -- the weakest property the query can be built on, and the one a wrong cell, a dropped
//! leg or a mis-swept seam all break.

use crate::hydrology::bake_tests::refined_populations;
use crate::hydrology::buckets::BucketIndex;
use crate::sphere::SpherePoint;
use crate::water::index::{WaterIndex, DEFAULT_CELL_M};

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
            let mut span_m = 0.0;
            for &(lat, lon) in &body.outline {
                let d = anchor.distance_to(&SpherePoint::from_latlon(lat, lon), surface.radius_m);
                if d > span_m {
                    span_m = d;
                }
            }
            let circle_m = span_m + body.shore_reach_m;
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
