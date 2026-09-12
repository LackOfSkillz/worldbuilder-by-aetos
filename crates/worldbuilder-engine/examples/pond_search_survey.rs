//! The instrument behind the fine pond search's numbers, committed so they can be re-derived.
//!
//! Every figure in `.superpowers/sdd/2026-09-12-water-1b3-shores/task-4-report.md` -- the
//! candidate counts, the depth and area quantiles, the share the corridor clips, and what
//! survives Ruling S-10 -- comes out of this. Run it, do not transcribe it:
//!
//! ```text
//! cargo run --release -p worldbuilder-engine --example pond_search_survey
//! cargo run --release -p worldbuilder-engine --example pond_search_survey -- --stand-ins-1m
//! ```
//!
//! Release matters: a debug bake of the 200k populations takes minutes.
//!
//! `--stand-ins-1m` swaps the populations for `src/bin/hydro_survey.rs`'s three worlds at
//! 1,000,000 nodes, the population plan 1b-3's Task 7 judges the size and time gates at. It is
//! opt-in because it is about twenty minutes of bakes, and because the default run above is the
//! one Tasks 4 and 5 reported at.
//!
//! It is an example rather than a test because it asserts nothing. The numbers move when the
//! generator moves, which is the point: a survey that could fail would have to be pinned, and a
//! pinned survey stops being a measurement.

use worldbuilder_engine::hydrology::refine::Ground;
use worldbuilder_engine::hydrology::{self, buckets, ponds, HydroParams};
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

    let (mut strips_made, mut over, mut degen, mut gated) = (0u64, 0u64, 0u64, 0u64);
    let (mut with_one, mut side, mut end) = (0u64, 0u64, 0u64);
    // Ruling S-13's measurement: every S-10 survivor's ring, held to the two properties
    // simplification can break. `rings` is the population; `crossing` is how many cross
    // themselves; `leaking` is how many leave at least one of their own cells outside, and
    // `cells_out` of `cells_in` is how many cells that is.
    let (mut rings, mut crossing, mut leaking, mut refused) = (0u64, 0u64, 0u64, 0u64);
    let (mut cells_in, mut cells_out) = (0u64, 0u64);
    let mut ring_points = 0u64;
    let mut searched_m2 = 0.0f64;
    let (mut depths, mut areas) = (Vec::new(), Vec::new());
    let (mut kept_depths, mut kept_areas) = (Vec::new(), Vec::new());

    for reach in &record.reaches {
        let (made, skips) = ponds::strips_with_skips(reach, &ground, &detail, p);
        strips_made += made.len() as u64; // cast-ok: a count of strips, already an integer
        over += skips.over_budget as u64; // cast-ok: widening a u32 count
        degen += skips.degenerate as u64; // cast-ok: widening a u32 count
        gated += skips.gated as u64; // cast-ok: widening a u32 count
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
                    let ring = ponds::outline(s, &c.cells, p);
                    if ring.is_empty() {
                        // Neither the simplified ring nor the raw walk was usable, so `search`
                        // records nothing for this candidate.
                        refused += 1;
                        continue;
                    }
                    rings += 1;
                    ring_points += ring.len() as u64; // cast-ok: a count of ring points, already an integer
                    if !ponds::ring_is_simple(&ring, ground.radius_m) {
                        crossing += 1;
                    }
                    let outside = c.cells.iter().filter(|&&(row, column)| {
                        !ponds::ring_contains(&ring, ground.radius_m,
                                              &s.point_at(row, column, p.pond_cell_m))
                    }).count() as u64; // cast-ok: a count of cells, already an integer
                    cells_in += c.cells.len() as u64; // cast-ok: a count of cells, already an integer
                    cells_out += outside;
                    if outside > 0 {
                        leaking += 1;
                    }
                }
            }
        }
    }

    depths.sort_by(|a, b| a.total_cmp(b));
    areas.sort_by(|a, b| a.total_cmp(b));
    kept_depths.sort_by(|a, b| a.total_cmp(b));
    kept_areas.sort_by(|a, b| a.total_cmp(b));
    let total = depths.len() as u64; // cast-ok: a count of candidates, already an integer

    println!("{label}: nodes {}, reaches {}, strips {strips_made} (skipped {over} over budget, \
              {degen} degenerate, {gated} gated), searched {:.0} km2",
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
        println!("{label}: RINGS {rings} ({:.1} points each), self-crossing {crossing}, leaking {leaking}, refused {refused}; cells outside their own ring {cells_out} of {cells_in}",
                 ring_points as f64 / rings as f64);
    }
    println!("{label}: Ruling S-8 ceiling, one per {:.0} km2 of searched area = {:.0} bodies",
             p.pond_density_area_m2 / 1.0e6, searched_m2 / p.pond_density_area_m2);
}

/// What `ponds::search` actually put in the record, and what Ruling S-8's density cap cost: the
/// same world baked twice, once at the caller's cap and once at the loosest baseline the cap's
/// own grid can represent. The caller's cap is `earth_like`'s, which plan 1b-3's Task 7 size gate
/// raised from spec §6.6's 500 km² to 16,000 km² (Ruling S-16); the printed line names whatever
/// it is.
///
/// **The baseline is `buckets::finest_cell_m` squared, and that is a correction.** Task 5's
/// version asked for `1.0e4` m² -- a 100 m cell, finer than the search's own 250 m -- and called
/// it uncapped. It was not: `BucketIndex::new` clamps at 4,096 rows by 8,192 columns and says
/// nothing, so the realised cell was **4,886.50 m — 23.88 km², not 0.01**.
///
/// What this changes is the *label*, not the numbers. The clamp lands on exactly the cell
/// `finest_cell_m` names -- `buckets`'s own test asserts that equality exactly, not to a
/// tolerance -- so every figure the old baseline produced stands as a measurement against a
/// 23.88 km² baseline. What it is not is a measurement against no cap at all: this index cannot
/// represent one, so "the cap removes N" is a **lower bound**, and saying so is the whole of the
/// fix. The assertion below is the part that would have caught the mislabelling; `bake` now
/// refuses a request under one search cell outright.
fn recorded(label: &str, surface: &Surface, p: &HydroParams) {
    let started = std::time::Instant::now();
    let record = hydrology::bake(surface, p).expect("bake");
    let bake_s = started.elapsed().as_secs_f64();
    let mut uncapped = p.clone();
    let baseline_cell_m = buckets::finest_cell_m(surface.radius_m);
    uncapped.pond_density_area_m2 = baseline_cell_m * baseline_cell_m;
    let realised = buckets::BucketIndex::new(surface.radius_m, baseline_cell_m).cell_m();
    let off = if realised > baseline_cell_m { realised - baseline_cell_m } else { baseline_cell_m - realised };
    assert!(off < baseline_cell_m * 1.0e-3,
            "{label}: the baseline asked for a {baseline_cell_m} m cell and the index realised \
             {realised} m -- it is not a baseline, it is another cap");
    let without = hydrology::bake(surface, &uncapped).expect("bake");
    let kept = record.stats.ponds_kept as usize;
    let ponds = &record.bodies[record.bodies.len() - kept..];
    // What the ponds cost the record: the same record with them removed, encoded, against this
    // one. Nine of the difference are the header words Task 5 added, and the rest is the bodies.
    let words = hydrology::record::encode(&record).len();
    let mut without_ponds = record.clone();
    without_ponds.bodies.truncate(record.bodies.len() - kept);
    let bare = hydrology::record::encode(&without_ponds).len();
    println!("{label}: found {} kept {kept}; baseline ({:.1} km2 cell) {} -- the {:.0} km2 cap \
              removes {}; coarse bodies {}; record {words} words ({} of them ponds, {} bytes); \
              bake {bake_s:.2} s",
             record.stats.ponds_found,
             uncapped.pond_density_area_m2 / 1.0e6, without.stats.ponds_kept,
             p.pond_density_area_m2 / 1.0e6,
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

/// Plan 1b-3, Task 7: `src/bin/hydro_survey.rs`'s three stand-ins at its 1,000,000 nodes, so the
/// corridor and ring measurements can be read at the population the gates are judged at. Opt-in,
/// because it is about twenty minutes of bakes -- `survey` walks every strip a second time and
/// `recorded` bakes each world twice more -- and because the default run above is the population
/// Tasks 4 and 5 reported at, which must not move under it.
fn stand_ins_1m() {
    let p = HydroParams::earth_like(1_000_000);
    let worlds = [
        ("plain 1M", Surface::new(20_260_904, 6_371_000.0, 12, 0.29, None, None, None)),
        ("owner_survey 1M",
         Surface::new(562_423_712, 4_500_000.0, 28, 0.16, None, None, Some(TectonicParams::ranges()))),
        ("seed1_ranges 1M",
         Surface::new(1, 6_371_000.0, 12, 0.40, None, None, Some(TectonicParams::ranges()))),
    ];
    for (label, surface) in &worlds {
        survey(label, surface, &p);
    }
    for (label, surface) in &worlds {
        recorded(&format!("RECORD {label}"), surface, &p);
    }
}

fn main() {
    if std::env::args().any(|a| a == "--stand-ins-1m") {
        stand_ins_1m();
        return;
    }

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
    recorded("RECORD ranges 12k", &ranges_world, &small);
    recorded("RECORD default 200k", &default_world, &large);
    recorded("RECORD ranges 200k", &ranges_world, &large);
}
