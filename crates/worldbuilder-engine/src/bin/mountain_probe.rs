//! Does moving `TectonicParams` actually make a mountain? A throwaway probe, not Task 2.
//!
//! - **Population:** a 0.5-degree global grid (720 x 359 = 258,480 sites), refined at
//!   0.05 degrees in a 2-degree box around the coarse maximum.
//! - **Method:** `Surface::elevation_m(point, None)` and `structural_m`, plus a grade
//!   measured as the steepest single 2 km step on the FLANK -- 12 bearings walked out from
//!   the peak to 250 km. Measuring across the summit measures the one place a mountain is
//!   flat, and the first version of this probe did exactly that.
//! - **World:** the owner's, from their screenshot -- seed 123925603, radius 4,500,000 m,
//!   28 plates, land fraction 0.16.
//! - **Host:** named by whoever runs it; release build.
//!
//! Today's canonical pair is 1,500 m over 400 km, a 0.375% grade. Real ranges run 3-8%.

use worldbuilder_engine::sphere::SpherePoint;
use worldbuilder_engine::surface::Surface;
use worldbuilder_engine::tectonics::TectonicParams;

const SEED: i64 = 123_925_603;
const RADIUS_M: f64 = 4_500_000.0;
const PLATES: usize = 28;
const LAND: f64 = 0.16;

fn peak_of(surface: &Surface) -> (f64, f64, f64) {
    let mut best = (f64::NEG_INFINITY, 0.0, 0.0);
    let mut lat = -89.5;
    while lat <= 89.5 {
        let mut lon = -180.0;
        while lon < 180.0 {
            let e = surface.elevation_m(&SpherePoint::from_latlon(lat, lon), None);
            if e > best.0 {
                best = (e, lat, lon);
            }
            lon += 0.5;
        }
        lat += 0.5;
    }
    let (mut e, clat, clon) = best;
    let (mut blat, mut blon) = (clat, clon);
    let mut lat = clat - 1.0;
    while lat <= clat + 1.0 {
        let mut lon = clon - 1.0;
        while lon <= clon + 1.0 {
            let v = surface.elevation_m(&SpherePoint::from_latlon(lat, lon), None);
            if v > e {
                e = v;
                blat = lat;
                blon = lon;
            }
            lon += 0.05;
        }
        lat += 0.05;
    }
    (e, blat, blon)
}

/// Steepest LOCAL gradient anywhere on the flank, walking outward from the peak.
///
/// **Measuring across the summit is measuring the one place a mountain is flat** -- a
/// bump's derivative is zero at its maximum by definition. The first version of this probe
/// did exactly that and reported that narrowing the range made it *shallower*, which is
/// the opposite of the truth. The steep part is the flank, so this walks out from the peak
/// in 12 bearings to 250 km at 2 km steps and takes the steepest single 2 km step.
fn grade_at(surface: &Surface, lat: f64, lon: f64) -> f64 {
    let step_m = 2_000.0;
    let reach_m = 250_000.0;
    let per_m = 1.0 / (RADIUS_M * core::f64::consts::PI / 180.0);
    let mut steepest: f64 = 0.0;
    let mut bearing = 0.0;
    while bearing < 360.0 {
        let rad = bearing * core::f64::consts::PI / 180.0;
        let dlat = libm::cos(rad) * step_m * per_m;
        let dlon =
            libm::sin(rad) * step_m * per_m / libm::cos(lat * core::f64::consts::PI / 180.0);
        let mut i = 0.0;
        let mut previous =
            surface.elevation_m(&SpherePoint::from_latlon(lat, lon), None);
        while i * step_m < reach_m {
            i += 1.0;
            let here = surface.elevation_m(
                &SpherePoint::from_latlon(lat + dlat * i, lon + dlon * i),
                None,
            );
            let rise = if previous > here { previous - here } else { here - previous };
            let grade = 100.0 * rise / step_m;
            if grade > steepest {
                steepest = grade;
            }
            previous = here;
        }
        bearing += 30.0;
    }
    steepest
}

fn report(label: &str, params: Option<TectonicParams>) {
    let surface = Surface::new(SEED, RADIUS_M, PLATES, LAND, None, None, params);
    let (elevation_m, lat, lon) = peak_of(&surface);
    let structural_m = surface.structural_m(&SpherePoint::from_latlon(lat, lon));
    let grade = grade_at(&surface, lat, lon);
    println!(
        "{label:<34} peak {elevation_m:8.1} m  structural {structural_m:8.1} m  \
         grade {grade:5.3}%  at {lat:.2},{lon:.2}"
    );
}

/// How much of the planet is mountain, at a stated height, over the SAME 0.5-degree grid
/// `peak_of` walks -- 720 x 359 = 258,480 sites, every one of them, no refinement.
///
/// **This is the measurement `continental_blend` needed and the amplitude table could not
/// give.** Amplitude and width decide how high and how steep ONE range is; the blend decides
/// how many margins run the collision profile at all, and a peak height cannot see that. A
/// count of sites over a height can. `land` is the control: `continental_blend` reaches only
/// `from_margin`'s profile mix, never `Continentality`, so a blend that moved the coastline
/// would mean this knob is not the knob it is documented to be.
fn extent_of(surface: &Surface) -> (usize, usize, usize) {
    let (mut land, mut over_1000, mut over_2000) = (0usize, 0usize, 0usize);
    let mut lat = -89.5;
    while lat <= 89.5 {
        let mut lon = -180.0;
        while lon < 180.0 {
            let e = surface.elevation_m(&SpherePoint::from_latlon(lat, lon), None);
            if e > 0.0 {
                land += 1;
            }
            if e > 1000.0 {
                over_1000 += 1;
            }
            if e > 2000.0 {
                over_2000 += 1;
            }
            lon += 0.5;
        }
        lat += 0.5;
    }
    (land, over_1000, over_2000)
}

/// One row of the blend sweep, at a fixed collision pair so the only thing moving is the
/// blend.
fn blend_row(blend: f64, amp: f64, width_km: f64) {
    let params = TectonicParams {
        continent_collision_m: amp,
        continent_collision_width_m: width_km * 1000.0,
        continental_blend: blend,
        ..TectonicParams::canonical()
    };
    let surface = Surface::new(SEED, RADIUS_M, PLATES, LAND, None, None, Some(params));
    let (elevation_m, lat, lon) = peak_of(&surface);
    let (land, over_1000, over_2000) = extent_of(&surface);
    println!(
        "blend {blend:6.3}  peak {elevation_m:8.1} m  land {land:6}  \
         >1000 m {over_1000:5}  >2000 m {over_2000:5}  at {lat:.2},{lon:.2}"
    );
}

/// Where on the `wasm_exports.rs` fixture a tectonic block actually bites.
///
/// **This exists because a test found the six relief probes blind to it.** The relief
/// channel's `RELIEF_PROBES` were chosen to cross deep water, shelf, shoreline and ordinary
/// land -- the five terms `Detail::amplitude_m` blends between -- and reusing them for the
/// tectonic channel looked reasonable and was not: on the default fixture
/// (seed 20260904, radius 6,371,000 m, 12 plates, land 0.29) a collision profile of 6,000 m
/// over 100 km changes **not one bit** at any of the six, because none of them is within
/// 420 km of a convergent continental margin. A test asserting "a chosen block moves the
/// ground" over those probes would have failed for a true reason and, had it been written the
/// other way round, passed while proving nothing.
///
/// Method: the same 0.5-degree global grid, `elevation_m(point, None)` on two worlds that
/// differ in exactly one block, reporting the site of the largest absolute difference.
fn witness_for(label: &str, tectonics: TectonicParams) {
    witness_between(label, None, tectonics);
}

/// The same scan between two *chosen* blocks rather than against canonical.
///
/// Task 6 needs it. The parity corpus samples a tectonic world where the block bites, and
/// its negative control turns **one field** of that block off, so the site the corpus wants
/// is the one where that single field moves the ground the most -- not where the whole
/// preset does. Those are different places, and a corpus placed at the second would be a
/// control that barely moved anything.
fn witness_between(label: &str, base: Option<TectonicParams>, tectonics: TectonicParams) {
    let seed = 20_260_904;
    let radius_m = 6_371_000.0;
    let base = Surface::new(seed, radius_m, 12, 0.29, None, None, base);
    let moved = Surface::new(seed, radius_m, 12, 0.29, None, None, Some(tectonics));
    let mut best = (0.0f64, 0.0, 0.0, 0.0, 0.0);
    let mut lat = -89.5;
    while lat <= 89.5 {
        let mut lon = -180.0;
        while lon < 180.0 {
            let point = SpherePoint::from_latlon(lat, lon);
            let (a, b) = (base.elevation_m(&point, None), moved.elevation_m(&point, None));
            let delta = if a > b { a - b } else { b - a };
            if delta > best.0 {
                best = (delta, lat, lon, a, b);
            }
            lon += 0.5;
        }
        lat += 0.5;
    }
    let (delta, lat, lon, a, b) = best;
    println!(
        "{label:<28} largest move {delta:9.3} m at {lat:6.2},{lon:7.2}  \
         canonical {a:9.3} m -> {b:9.3} m"
    );
}

fn main() {
    println!("world: seed {SEED}, radius {RADIUS_M} m, {PLATES} plates, land {LAND}");
    println!("grade: steepest rise over a 20 km run through the peak, 12 bearings\n");

    report("canonical (1500 m / 400 km)", None);

    for (amp, width_km) in
        [(1500.0, 200.0), (1500.0, 100.0), (3000.0, 400.0), (3000.0, 150.0), (6000.0, 150.0), (6000.0, 100.0)]
    {
        let params = TectonicParams {
            continent_collision_m: amp,
            continent_collision_width_m: width_km * 1000.0,
            ..TectonicParams::canonical()
        };
        report(&format!("{amp:.0} m / {width_km:.0} km"), Some(params));
    }

    // ------------------------------------------------------------------ the blend sweep
    //
    // Task 4's own, because Task 1's probe did not vary this field and the brief says so.
    // Population: the same 0.5-degree global grid, 258,480 sites, counted rather than
    // maximised. Two amplitudes, because a count over 1,000 m is nearly empty at canonical
    // amplitude and would read as "the blend does nothing".
    for (amp, width_km) in [(1500.0, 400.0), (6000.0, 150.0)] {
        println!("\nblend sweep at {amp:.0} m / {width_km:.0} km (canonical blend is 0.45)");
        for blend in [0.01, 0.05, 0.10, 0.20, 0.30, 0.45, 0.60, 0.80, 1.00, 1.50, 2.00, 4.00, 8.00]
        {
            blend_row(blend, amp, width_km);
        }
    }

    // ------------------------------------------- a witness point for the export-side tests
    println!("\nwitness points on the wasm_exports fixture (seed 20260904, 12 plates, land 0.29)");
    witness_for(
        "6000 m / 100 km",
        TectonicParams {
            continent_collision_m: 6000.0,
            continent_collision_width_m: 100_000.0,
            ..TectonicParams::canonical()
        },
    );
    witness_for(
        "blend 1.00 (fewer)",
        TectonicParams { continental_blend: 1.0, ..TectonicParams::canonical() },
    );
    witness_for(
        "blend 0.10 (more)",
        TectonicParams { continental_blend: 0.1, ..TectonicParams::canonical() },
    );

    // ------------------------------------------------ where the parity corpus should sample
    //
    // The same fixture world the parity corpus uses (`examples/parity_dump.rs`'s SEED,
    // RADIUS_M, PLATES, LAND are these four values). Two witnesses, because the corpus and
    // its control want different things: the first says where `ranges()` bites at all, the
    // second says where turning `margin_warp_m` off moves the ground -- which is the site the
    // corpus's concentrated points and its tile are placed on, so that the control has
    // something to move.
    witness_for("ranges() vs canonical", TectonicParams::ranges());
    witness_between(
        "ranges() warp on vs off",
        Some(TectonicParams { margin_warp_m: 0.0, ..TectonicParams::ranges() }),
        TectonicParams::ranges(),
    );
}
