//! The instrument behind the fine pond search's numbers, committed so they can be re-derived.
//!
//! Every figure in `.superpowers/sdd/2026-09-12-water-1b3-shores/task-4-report.md` -- the
//! candidate counts, the depth and area quantiles, the share the corridor clips, and what
//! survives Ruling S-10 -- comes out of this. Run it, do not transcribe it:
//!
//! ```text
//! cargo run --release -p worldbuilder-engine --example pond_search_survey
//! ```
//!
//! Release matters: a debug bake of the 200k populations takes minutes.
//!
//! It is an example rather than a test because it asserts nothing. The numbers move when the
//! generator moves, which is the point: a survey that could fail would have to be pinned, and a
//! pinned survey stops being a measurement.

use worldbuilder_engine::hydrology::refine::Ground;
use worldbuilder_engine::hydrology::{self, ponds, HydroParams};
use worldbuilder_engine::sphere::SpherePoint;
use worldbuilder_engine::surface::Surface;
use worldbuilder_engine::tectonics::TectonicParams;

/// min, median, max of an already-sorted, non-empty slice.
fn quantiles(sorted: &[f64]) -> (f64, f64, f64) {
    (sorted[0], sorted[sorted.len() / 2], sorted[sorted.len() - 1])
}

fn survey(label: &str, surface: &Surface, p: &HydroParams) {
    let record = hydrology::bake(surface, p).expect("bake");
    // Two grounds, deliberately (Ruling S-9): the landform for the geometry the tracer used, the
    // detail field for what the fine search reads.
    let height = |q: &SpherePoint| surface.structural_m(q);
    let ground = Ground::for_surface(surface, &height, p);
    let detail = ponds::pond_ground(surface, p);

    let (mut strips_made, mut over, mut degen) = (0u64, 0u64, 0u64);
    let (mut with_one, mut side, mut end) = (0u64, 0u64, 0u64);
    let mut searched_m2 = 0.0f64;
    let (mut depths, mut areas) = (Vec::new(), Vec::new());
    let (mut kept_depths, mut kept_areas) = (Vec::new(), Vec::new());

    for reach in &record.reaches {
        let (made, skips) = ponds::strips_with_skips(reach, &ground, &detail, p);
        strips_made += made.len() as u64;
        over += skips.over_budget as u64;
        degen += skips.degenerate as u64;
        for (i, s) in made.iter().enumerate() {
            searched_m2 += (s.steps * s.cells_across) as f64 * p.pond_cell_m * p.pond_cell_m;
            let found = ponds::hollows_in(s, i, p);
            if !found.is_empty() {
                with_one += 1;
            }
            for c in found {
                let depth = c.level_m - c.floor_m;
                depths.push(depth);
                areas.push(c.area_m2);
                if c.touches_side { side += 1; }
                if c.touches_end { end += 1; }
                // Ruling S-10: a side-clipped candidate is not recorded; an end-clipped one is,
                // and Task 5's dedup resolves it. This is the population Task 5 plans for.
                if !c.touches_side {
                    kept_depths.push(depth);
                    kept_areas.push(c.area_m2);
                }
            }
        }
    }

    depths.sort_by(|a, b| a.total_cmp(b));
    areas.sort_by(|a, b| a.total_cmp(b));
    kept_depths.sort_by(|a, b| a.total_cmp(b));
    kept_areas.sort_by(|a, b| a.total_cmp(b));
    let total = depths.len() as u64;

    println!("{label}: nodes {}, reaches {}, strips {strips_made} (skipped {over} over budget, {degen} degenerate), searched {:.0} km2",
             p.total_nodes, record.reaches.len(), searched_m2 / 1.0e6);
    if total == 0 {
        println!("{label}: NO CANDIDATES");
        return;
    }
    let (dlo, dmid, dhi) = quantiles(&depths);
    let (alo, amid, ahi) = quantiles(&areas);
    println!("{label}: candidates {total}, in {with_one} of {strips_made} strips ({:.1}%); one per {:.0} km2 searched",
             100.0 * with_one as f64 / strips_made as f64, searched_m2 / 1.0e6 / total as f64);
    println!("{label}:   depth min {dlo:.2} median {dmid:.2} max {dhi:.2} m; area min {alo:.0} median {amid:.0} max {ahi:.0} m2");
    println!("{label}: clipped by a long side {side} ({:.1}%), by an end {end} ({:.1}%)",
             100.0 * side as f64 / total as f64, 100.0 * end as f64 / total as f64);
    if kept_depths.is_empty() {
        println!("{label}: S-10 SURVIVORS none");
    } else {
        let (dlo, dmid, dhi) = quantiles(&kept_depths);
        let (alo, amid, ahi) = quantiles(&kept_areas);
        println!("{label}: S-10 survivors {} ({:.1}% of candidates)",
                 kept_depths.len(), 100.0 * kept_depths.len() as f64 / total as f64);
        println!("{label}:   depth min {dlo:.2} median {dmid:.2} max {dhi:.2} m; area min {alo:.0} median {amid:.0} max {ahi:.0} m2");
    }
    println!("{label}: Ruling S-8 ceiling, one per {:.0} km2 of searched area = {:.0} bodies",
             p.pond_density_area_m2 / 1.0e6, searched_m2 / p.pond_density_area_m2);
}

/// What Task 5's `ponds::search` actually put in the record, and what Ruling S-8's density cap
/// cost: the same world baked twice, once at the spec's 500 km² cap and once with the cap
/// effectively removed (one cell per 10,000 m², finer than the search's own 250 m cell, so no two
/// candidates can share one).
fn recorded(label: &str, surface: &Surface, p: &HydroParams) {
    let record = hydrology::bake(surface, p).expect("bake");
    let mut uncapped = p.clone();
    uncapped.pond_density_area_m2 = 1.0e4;
    let without = hydrology::bake(surface, &uncapped).expect("bake");
    let kept = record.stats.ponds_kept as usize;
    let ponds = &record.bodies[record.bodies.len() - kept..];
    // What the ponds cost the record: the same record with them removed, encoded, against this
    // one. Nine of the difference are the header words Task 5 added, and the rest is the bodies.
    let words = hydrology::record::encode(&record).len();
    let mut without_ponds = record.clone();
    without_ponds.bodies.truncate(record.bodies.len() - kept);
    let bare = hydrology::record::encode(&without_ponds).len();
    println!("{label}: found {} kept {kept}; uncapped {} -- the 500 km2 cap removes {}; \
              coarse bodies {}; record {words} words ({} of them ponds, {} bytes)",
             record.stats.ponds_found, without.stats.ponds_kept,
             without.stats.ponds_kept as usize - kept, record.bodies.len() - kept,
             words - bare, (words - bare) * 8);
    if kept > 0 {
        let ring: usize = ponds.iter().map(|b| b.outline.len()).sum();
        let lakes = ponds.iter().filter(|b| b.area_m2 >= p.pond_max_area_m2).count();
        let widest = ponds.iter().map(|b| b.outline.len()).max().unwrap_or(0);
        println!("{label}:   Ruling S-11 kinds: {lakes} Lake, {} Pond; outline points {ring} \
                  total, {} mean, {widest} most",
                 kept - lakes, ring / kept);
    }
}

fn main() {
    // `bake_tests`'s two worlds and its two populations, so the survey and the suite argue about
    // the same terrain.
    let default_world = Surface::new(20_260_904, 6_371_000.0, 12, 0.29, None, None, None);
    let ranges_world = Surface::new(1, 6.371e6, 12, 0.40, None, None,
                                    Some(TectonicParams::ranges()));
    let mut small = HydroParams::earth_like(12_000);
    small.wetness_nodes = 500;
    small.stream_flow_m2 = 3.0e10;
    small.river_flow_m2 = 3.0e11;
    small.great_flow_m2 = 3.0e12;
    let large = HydroParams::earth_like(200_000);
    // `bake_tests::junction_params()`: the same world and population with the stream floor
    // lowered, which is a different reach set and so a different set of strips.
    let mut junction = small.clone();
    junction.min_stream_nodes = 2.0;

    survey("default 12k", &default_world, &small);
    survey("default 200k", &default_world, &large);
    survey("ranges 12k", &ranges_world, &small);
    survey("ranges 200k", &ranges_world, &large);

    recorded("RECORD default 12k (params)", &default_world, &small);
    recorded("RECORD default 12k (junction_params)", &default_world, &junction);
    recorded("RECORD ranges 12k", &ranges_world, &HydroParams::earth_like(12_000));
    recorded("RECORD default 200k", &default_world, &large);
    recorded("RECORD ranges 200k", &ranges_world, &large);
}
