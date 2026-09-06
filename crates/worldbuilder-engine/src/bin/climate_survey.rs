//! Measures what temperature and what moisture this engine's worlds actually reach, and
//! chooses nothing.
//!
//! Task 1 of the climate slice
//! (`docs/superpowers/plans/2026-09-06-slice-climate.md`). `climate.rs` gives every point
//! a mean annual temperature in degrees C from latitude and an elevation lapse rate. The
//! axis is **absolute, not quantiled** -- the photoreal slice measured that quantiling it
//! puts the owner world's desert latitudes in the median band and makes the brightest
//! palette entry unreachable -- and the honest cost of an absolute axis is that **a band
//! can go unvisited on a world whose land is all one climate**.
//!
//! That cost has to be a number, not a shrug. This binary is where the number comes from.
//!
//! ```text
//! cargo run --release --bin climate_survey
//! ```
//!
//! It is a `[[bin]]` rather than a `cargo test` fixture for the reason `relief_survey.rs`,
//! `pond_threshold_survey.rs` and `erosion_convergence_sweep.rs` already are: four
//! full-planet builds, each sampled at 200,000 points, is a one-machine measurement
//! question, not a property a suite that runs on every push should re-pay. **It changes no
//! default, no `canonical()` value and no engine parameter.**
//!
//! # Method -- every figure names its population, its method with parameters, and its host
//!
//! - **Host:** named on the run's own first line, from `cargo run --release`.
//! - **Population:** `SAMPLES = 200_000` points of a Fibonacci spiral over the whole
//!   sphere -- the same construction `continentality.rs::calibrate` and
//!   `viewer/public/app/biome.js::fibonacciPoint` use, and the same 200,000 count the
//!   photoreal slice's own land scan used, so the two are comparable. **Land** is
//!   `elevation_m(point, None) > 0.0`, and the land count is printed rather than assumed.
//! - **Method:** `Surface::temperature_c(point, None, None)` at every land point.
//!   `resolution_m = None` is the physics ground truth rather than a viewer's resampled
//!   one, matching every other survey in this directory.
//! - **Worlds:** the four the photoreal slice's own reachability table used, so a reader
//!   can put the two tables side by side. The owner's world is first.
//!
//! # What it prints, and what each column is for
//!
//! - **land / samples** -- how much of the population survived the land test. A world with
//!   very little land is a world whose temperature span means less.
//! - **min / max / span** -- the answer to "what temperature does this world produce, and
//!   at what latitudes". Both extremes carry the latitude and elevation they were found at,
//!   because "coldest" on a world with polar land and "coldest" on a world whose coldest
//!   place is a tropical summit are different facts.
//! - **sea-level min / max** -- the same two over land at or below 20 m, which isolates the
//!   latitude profile from the lapse term. If the two tables agree, elevation is not
//!   reaching temperature on this world and the snow line has nothing to stand on.
//! - **band occupancy** -- how many land points fall in each of the photoreal slice's five
//!   absolute bands (`TEMP_BAND_EDGES_C = [0, 8, 18, 24]`). **A band with zero points is
//!   the cost of an absolute axis being paid, and this is where it becomes visible.**
//! - **freezing contour** -- the elevation at which this world's temperature crosses zero,
//!   at five latitudes, and the count of land points above it. That is the snow line Task 5
//!   consumes, reported here so Task 5 starts from a measurement rather than a claim.
//!
//! # The moisture half -- Task 2, and it RE-DERIVES A SHIPPED CONSTANT
//!
//! `climate::MARCH_SAMPLES` is 160 rather than the 40 the spike benchmarked, and the reason
//! is not a cost curve -- the spike proved the cost is affine with no knee, so the budget is
//! a physics decision. **This section is that physics, re-derivable on demand rather than
//! transcribed from a report**, which is the standing test for whether a `[[bin]]` earns its
//! place next to `relief_survey.rs` and `pond_threshold_survey.rs`.
//!
//! Two measurements, with their own smaller population (`MARCH_SAMPLES_SURVEYED`, because
//! each moisture query is 161 elevation queries against temperature's one):
//!
//! - **upwind fetch to open water** -- from each land point, walk upwind in `MARCH_STEP_M`
//!   steps until `elevation_m <= 0`. The percentiles say how far a march must reach before
//!   it measures a fetch at all, and the coverage table says what fraction of a world's
//!   land a given budget reaches water from. **A budget that never leaves the continent
//!   measures nothing.**
//! - **the moisture the march actually produces** -- deciles over land, so Task 3 can cut
//!   its bands from a distribution it has seen rather than from an assumed one, and so a
//!   world whose land is all one moisture is visible as such.

use worldbuilder_engine::climate::{self, ClimateParams};
use worldbuilder_engine::continentality::LAND_FRACTION;
use worldbuilder_engine::detmath as m;
use worldbuilder_engine::sphere::{SpherePoint, EARTH_RADIUS_M};
use worldbuilder_engine::surface::Surface;
use worldbuilder_engine::tangent::TangentFrame;
use worldbuilder_engine::tectonics::TectonicParams;

const SAMPLES: usize = 200_000;

/// The moisture half's population. Smaller than `SAMPLES` for a stated reason rather than a
/// convenient one: **one moisture query is `MARCH_SAMPLES + 1` elevation queries**, so the
/// same 200,000 points would be 161 times the work of the temperature half.
const MARCH_SAMPLES_SURVEYED: usize = 20_000;

/// How far the fetch probe is willing to walk before calling a point landlocked, in march
/// steps. 300 steps at `MARCH_STEP_M` is 6,000 km -- comfortably past the widest continent
/// any of these four worlds has, so "never reached water" is a fact about the world and not
/// about this constant.
const FETCH_PROBE_STEPS: usize = 300;

/// The photoreal slice's absolute temperature band edges
/// (`viewer/public/app/biome.js::TEMP_BAND_EDGES_C`), transcribed here for occupancy only.
/// **Nothing in the engine reads them** -- the engine returns degrees and the viewer bands
/// them, which is the whole point of the axis having a unit.
const BAND_EDGES_C: &[f64] = &[0.0, 8.0, 18.0, 24.0];
const BAND_NAMES: &[&str] = &["polar", "boreal", "temperate", "subtropical", "tropical"];

/// Land at or below this height stands in for "sea level", isolating the latitude profile
/// from the lapse term.
const SEA_LEVEL_BAND_M: f64 = 20.0;

struct World {
    label: &'static str,
    seed: i64,
    radius_m: f64,
    plate_count: usize,
    land_fraction: f64,
    tectonics: Option<TectonicParams>,
}

/// The `i`-th of `n` points of a Fibonacci spiral, as a `SpherePoint`. The same
/// construction `continentality.rs::calibrate` uses.
fn fibonacci_point(index: usize, count: usize) -> SpherePoint {
    let golden = core::f64::consts::PI * (3.0 - m::sqrt(5.0));
    let z = 1.0 - 2.0 * (index as f64 + 0.5) / (count as f64); // cast-ok: loop counter to float, exact far below 2^53
    let inner = 1.0 - z * z;
    let ring = m::sqrt(if inner > 0.0 { inner } else { 0.0 });
    let angle = golden * index as f64; // cast-ok: loop counter to float, exact far below 2^53
    SpherePoint {
        vector: worldbuilder_engine::vectors::Vec3::new(
            m::cos(angle) * ring,
            m::sin(angle) * ring,
            z,
        ),
    }
}

/// The band a temperature falls in, by the edges above. Explicit branches: `f64::min`,
/// `f64::max` and `.clamp(` are NaN-asymmetric and banned in this crate.
fn band_of(temperature_c: f64) -> usize {
    let mut band = 0;
    for edge in BAND_EDGES_C {
        if temperature_c >= *edge {
            band += 1;
        }
    }
    band
}

/// The elevation at which the closed form crosses zero at a latitude -- the freezing
/// contour, which is the snow line Task 5 consumes. Negative means the sea surface itself
/// is below freezing there and no elevation is needed.
fn freezing_contour_m(latitude_deg: f64, params: &ClimateParams) -> f64 {
    1000.0 * (params.pole_c + (params.equator_c - params.pole_c) * m::cos(m::to_radians(latitude_deg)))
        / params.lapse_c_per_km
}

/// The decile boundaries of a sorted sample, as a small table. Explicit indexing rather
/// than interpolation: this is a distribution to look at, not a statistic to publish.
fn deciles(sorted: &[f64]) -> Vec<f64> {
    if sorted.is_empty() {
        return Vec::new();
    }
    (0..=10)
        .map(|tenth| {
            let index = (sorted.len() - 1) * tenth / 10;
            sorted[index]
        })
        .collect()
}

/// How many march steps upwind of `point` open water lies, or `None` if it is further than
/// `FETCH_PROBE_STEPS`. **This is the measurement that sets `MARCH_SAMPLES`.**
fn upwind_fetch_steps(surface: &Surface, radius_m: f64, point: &SpherePoint) -> Option<usize> {
    let (latitude, _) = point.to_latlon();
    let east = climate::upwind_east(latitude);
    let frame = TangentFrame::at(point, radius_m);
    for step in 1..=FETCH_PROBE_STEPS {
        let offset = east * climate::MARCH_STEP_M * step as f64; // cast-ok: loop counter to float
        let probe = frame.local_to_sphere(offset, 0.0);
        if !(surface.elevation_m(&probe, None) > 0.0) {
            return Some(step);
        }
    }
    None
}

fn main() {
    let worlds = [
        World {
            label: "owner   (562423712, R 4.5 Mm, 28 plates, land 0.16, ranges)",
            seed: 562_423_712,
            radius_m: 4_500_000.0,
            plate_count: 28,
            land_fraction: 0.16,
            tectonics: Some(TectonicParams::ranges()),
        },
        World {
            label: "default (20260904,  R 6.371 Mm, 12 plates, land 0.29)",
            seed: 20_260_904,
            radius_m: EARTH_RADIUS_M,
            plate_count: 12,
            land_fraction: LAND_FRACTION,
            tectonics: None,
        },
        World {
            label: "seed 7  (7,         R 6.371 Mm, 12 plates, land 0.29)",
            seed: 7,
            radius_m: EARTH_RADIUS_M,
            plate_count: 12,
            land_fraction: LAND_FRACTION,
            tectonics: None,
        },
        World {
            label: "seed 424242 (       R 6.371 Mm, 18 plates, land 0.40)",
            seed: 424_242,
            radius_m: EARTH_RADIUS_M,
            plate_count: 18,
            land_fraction: 0.40,
            tectonics: None,
        },
    ];

    println!("climate_survey -- temperature (Task 1) and the upwind moisture march (Task 2)");
    println!("engine {}, {SAMPLES} Fibonacci samples per world for temperature and {MARCH_SAMPLES_SURVEYED} for moisture, resolution_m = None", env!("CARGO_PKG_VERSION"));
    println!("canonical temperature: equator {} C, pole {} C, lapse {} C/km",
        climate::EQUATOR_C, climate::POLE_C, climate::LAPSE_C_PER_KM);
    println!("canonical march: {} steps of {} m ({} km of reach), lift {} m, fetch {} m, recharge {} m",
        climate::MARCH_SAMPLES, climate::MARCH_STEP_M,
        climate::MARCH_STEP_M * f64::from(climate::MARCH_SAMPLES) / 1000.0,
        climate::LIFT_SCALE_M, climate::FETCH_SCALE_M, climate::RECHARGE_SCALE_M);
    println!();

    let params = ClimateParams::canonical();

    for world in &worlds {
        let surface = Surface::new(
            world.seed,
            world.radius_m,
            world.plate_count,
            world.land_fraction,
            None,
            None,
            world.tectonics,
        );

        let mut land = 0usize;
        let mut coldest = f64::INFINITY;
        let mut coldest_at = (0.0, 0.0);
        let mut hottest = f64::NEG_INFINITY;
        let mut hottest_at = (0.0, 0.0);
        let mut sea_coldest = f64::INFINITY;
        let mut sea_coldest_lat = 0.0;
        let mut sea_hottest = f64::NEG_INFINITY;
        let mut sea_hottest_lat = 0.0;
        let mut bands = [0usize; 5];
        let mut above_freezing_contour = 0usize;
        let mut highest = f64::NEG_INFINITY;

        for index in 0..SAMPLES {
            let point = fibonacci_point(index, SAMPLES);
            let height = surface.elevation_m(&point, None);
            if !(height > 0.0) {
                continue;
            }
            land += 1;
            let (latitude, _longitude) = point.to_latlon();
            let temperature = surface.temperature_c(&point, None, None);
            if height > highest {
                highest = height;
            }
            if temperature < coldest {
                coldest = temperature;
                coldest_at = (latitude, height);
            }
            if temperature > hottest {
                hottest = temperature;
                hottest_at = (latitude, height);
            }
            if height <= SEA_LEVEL_BAND_M {
                if temperature < sea_coldest {
                    sea_coldest = temperature;
                    sea_coldest_lat = latitude;
                }
                if temperature > sea_hottest {
                    sea_hottest = temperature;
                    sea_hottest_lat = latitude;
                }
            }
            bands[band_of(temperature)] += 1;
            if temperature < 0.0 {
                above_freezing_contour += 1;
            }
        }

        println!("{}", world.label);
        println!("  land {land} of {SAMPLES} ({:.4}), highest land {highest:.1} m",
            land as f64 / SAMPLES as f64); // cast-ok: counts to float, exact far below 2^53
        println!("  temperature  {coldest:.2} C at lat {:.1} / {:.0} m  ..  {hottest:.2} C at lat {:.1} / {:.0} m   span {:.2} C",
            coldest_at.0, coldest_at.1, hottest_at.0, hottest_at.1, hottest - coldest);
        println!("  at sea level {sea_coldest:.2} C at lat {sea_coldest_lat:.1}  ..  {sea_hottest:.2} C at lat {sea_hottest_lat:.1}   span {:.2} C",
            sea_hottest - sea_coldest);
        print!("  bands       ");
        for (name, count) in BAND_NAMES.iter().zip(bands.iter()) {
            print!(" {name} {count}");
        }
        println!();
        let empty: Vec<&&str> = BAND_NAMES
            .iter()
            .zip(bands.iter())
            .filter(|(_, count)| **count == 0)
            .map(|(name, _)| name)
            .collect();
        if empty.is_empty() {
            println!("  every band reached");
        } else {
            println!("  UNREACHED: {empty:?}  -- the cost of an absolute axis, on this world");
        }
        println!("  land below freezing: {above_freezing_contour} points");
        print!("  freezing contour   ");
        for latitude in [0.0, 20.0, 45.0, 60.0, 70.0] {
            print!(" {latitude:.0}deg {:.0}m", freezing_contour_m(latitude, &params));
        }
        println!();

        // ---- Moisture. Its own smaller population; see MARCH_SAMPLES_SURVEYED. ----
        let mut fetches: Vec<usize> = Vec::new();
        let mut landlocked = 0usize;
        let mut wetness: Vec<f64> = Vec::new();
        let mut march_land = 0usize;
        for index in 0..MARCH_SAMPLES_SURVEYED {
            let point = fibonacci_point(index, MARCH_SAMPLES_SURVEYED);
            if !(surface.elevation_m(&point, None) > 0.0) {
                continue;
            }
            march_land += 1;
            match upwind_fetch_steps(&surface, world.radius_m, &point) {
                Some(steps) => fetches.push(steps),
                None => landlocked += 1,
            }
            wetness.push(surface.moisture_index(&point, None, None));
        }
        fetches.sort_unstable();
        wetness.sort_by(|a, b| a.partial_cmp(b).expect("the march produces no NaN on real land"));

        println!("  --- moisture, {march_land} land points of {MARCH_SAMPLES_SURVEYED} ---");
        let km = |steps: usize| steps as f64 * climate::MARCH_STEP_M / 1000.0; // cast-ok: count to float
        if fetches.is_empty() {
            println!("  upwind fetch: no land point reached water within {FETCH_PROBE_STEPS} steps");
        } else {
            let at = |fraction: f64| km(fetches[((fetches.len() - 1) as f64 * fraction) as usize]); // cast-ok: quantile index
            println!("  upwind fetch km   p50 {:.0}  p75 {:.0}  p90 {:.0}  p95 {:.0}  max {:.0}   landlocked {landlocked} ({:.2}%)",
                at(0.50), at(0.75), at(0.90), at(0.95), at(1.0),
                100.0 * landlocked as f64 / march_land as f64); // cast-ok: counts to float
            print!("  land reaching water by budget:");
            for budget in [40usize, 80, 160, 240] {
                let within = fetches.iter().filter(|steps| **steps <= budget).count();
                print!("  {budget} ({:.0} km) {:.0}%", km(budget),
                    100.0 * within as f64 / march_land as f64); // cast-ok: counts to float
            }
            println!();
        }
        let d = deciles(&wetness);
        if d.is_empty() {
            println!("  moisture: no land");
        } else {
            print!("  moisture deciles  ");
            for value in &d {
                print!(" {value:.3}");
            }
            println!();
            println!("  moisture span {:.3} .. {:.3}", d[0], d[d.len() - 1]);
        }
        println!();
    }
}
