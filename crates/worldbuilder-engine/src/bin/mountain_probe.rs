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
}
