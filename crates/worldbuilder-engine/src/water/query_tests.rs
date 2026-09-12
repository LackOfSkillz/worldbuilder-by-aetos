//! The index against real bakes, rather than against a hand-written record.
//!
//! `index.rs`'s own tests pin the rules one at a time on a fixture small enough to reason about.
//! This asks the only question a real record can answer: does the index ever *lose* something?
//! Every recorded body, reach and notch must be a candidate at every one of its own recorded
//! points -- the weakest property the query can be built on, and the one a wrong cell, a dropped
//! leg or a mis-swept seam all break.

use crate::hydrology::bake_tests::refined_populations;
use crate::sphere::SpherePoint;
use crate::water::index::{WaterIndex, DEFAULT_CELL_M};

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
