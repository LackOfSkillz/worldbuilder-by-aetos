//! Measures relief across `ReliefParams`' parameter space, and chooses nothing.
//!
//! Task 2 of the relief-amplitude slice
//! (`.superpowers/sdd/2026-09-05-slice-relief-amplitude/task-2-brief.md`). Task 1 turned
//! nine constants baked into `detail.rs` into an opt-in `ReliefParams` block, proven
//! bit-identical to today at `Some(ReliefParams::canonical())` and at `None`. This binary
//! sweeps three of those fields and reports what moves. **It changes no default and no
//! `canonical()` value** -- Task 3 chooses a named preset from these numbers; this file
//! only measures.
//!
//! ```text
//! cargo run --release --bin relief_survey
//! ```
//!
//! This is a `[[bin]]`, not a `cargo test`-visible fixture, for the reason
//! `erosion_convergence_sweep.rs` and `pond_threshold_survey.rs` already are: 80
//! full-planet configurations, each rebuilding a `Surface` and walking a transect at
//! several dozen land sites, is a one-machine measurement question, not a property a
//! suite that runs on every push should re-pay.
//!
//! # Method (every figure below names its population, method with parameters, and host)
//!
//! - **Host:** this developer machine, `cargo run --release`.
//! - **World:** `Surface::new(SEED, EARTH_RADIUS_M, generation::DEFAULT_PLATE_COUNT,
//!   continentality::LAND_FRACTION, None, relief)` -- `SEED` is `20_260_904`, the same
//!   seed the brief's own baseline measurement (median 4.6 m / max 14.1 m / 0.7% steepest
//!   gradient) was taken against.
//! - **Land-site population:** a fixed 9x12 lat/lon grid (`LAT_STEP_DEG = 20`, latitudes
//!   -80..=80; `LON_STEP_DEG = 30`, longitudes -180..150), 108 candidates, filtered to
//!   land by `structural_m(point) > 0.0` against a **canonical** (`relief: None`)
//!   reference `Surface` -- structural elevation does not depend on `ReliefParams` (see
//!   `surface.rs::structural_m`'s doc: "Detail does not appear here at all"), so the same
//!   site list is valid across every swept setting. The exact count of land sites the
//!   grid resolves to is printed at the top of the run, not assumed.
//! - **High-ground population:** the subset of the land sites above `structural_m >
//!   300.0` -- the same threshold the plan's own baseline used for its "113 sites above
//!   300 m" figure -- against the same canonical reference, so which sites count as
//!   "high ground" also does not move as the swept parameters do.
//! - **Peak population, and why it exists separately:** the 108-candidate land grid is
//!   20 deg x 30 deg, coarse enough that a first run of this survey (kept in
//!   task-2-report.md) landed almost entirely on ordinary terrain and showed
//!   `quieting_strength` changing almost nothing -- because the quieting term only bites
//!   where `tectonic_m` is large, i.e. near real uplift, and the coarse grid barely
//!   samples that. So a second, denser search -- `PEAK_LAT_STEPS x PEAK_LON_STEPS = 35 x
//!   72 = 2,520` candidates at 5 deg spacing, ranked by `structural_m` against the same
//!   canonical reference, top `PEAK_SITE_COUNT = 20` kept -- stands in for "near a real
//!   mountain" the way the land grid stands in for "typical land". Both populations are
//!   measured at every configuration below; the peak table is where `quieting_strength`
//!   should show up if it does anything on the ground that actually has tectonic offset.
//! - **Relief over a 2 km transect:** at each land site, a `TangentFrame::at_latlon`
//!   centred there, sampled along local east from -1000 m to +1000 m at 50 m spacing (41
//!   points, matching the plan's own 50 m-spacing baseline measurement), each point's
//!   elevation from `Surface::elevation_m(point, None)` (canonical resolution -- physics
//!   ground truth, not a viewer's resampled one). Relief is `max - min` over that
//!   transect; gradient is the largest `|delta elevation| / 50.0` between adjacent
//!   samples, in the same transect.
//! - **Population summary statistics:** median and p90 (linear-interpolation percentile,
//!   `rank = p * (n - 1)`, matching the common convention) of per-site relief over all
//!   land sites; the single largest gradient found at ANY land site's transect anywhere
//!   in the population.
//! - **Detail term's share on high ground:** at each high-ground site's own point (not a
//!   transect), `detail_m = elevation_m(point, None) - structural_m(point)`, `share =
//!   |detail_m| / elevation_m(point, None)`. Reported as median and max over the
//!   high-ground population.
//! - **The governing group:** `H = ln(1 / persistence) / ln(lacunarity)`, `lacunarity =
//!   2.0` -- the octave schedule's own wavelength ratio (`detail.rs::plan`: "each octave
//!   is half the wavelength ... of the one before"). Printed once per persistence value
//!   swept, not re-derived per row.
//!
//! # The grid
//!
//! - `mountain_m`: 150 (today), 300, 450, 600 -- `MOUNTAIN_MULTIPLIERS` times today's
//!   `ReliefParams::canonical().mountain_m`.
//! - `quieting_strength`: 0.7 (today), 0.35 (reduced), 0.0, -0.35, -0.7 (inverted -- real
//!   uplifted ground is rougher, per the brief).
//! - `octave_persistence`: 0.5 (today), 0.65, 0.71, 0.75 -- exactly the brief's own list.
//!
//! 4 x 5 x 4 = 80 configurations, each measured over the same fixed site population, so
//! the table below reports every interaction rather than one axis at a time.

use worldbuilder_engine::continentality::LAND_FRACTION;
use worldbuilder_engine::detail::ReliefParams;
use worldbuilder_engine::detmath as m;
use worldbuilder_engine::generation::DEFAULT_PLATE_COUNT;
use worldbuilder_engine::sphere::{SpherePoint, EARTH_RADIUS_M};
use worldbuilder_engine::surface::Surface;
use worldbuilder_engine::tangent::TangentFrame;

const SEED: i64 = 20_260_904;

const LAT_STEPS: i32 = 9; // -80, -60, ..., 80
const LAT_STEP_DEG: f64 = 20.0;
const LAT_START_DEG: f64 = -80.0;
const LON_STEPS: i32 = 12; // -180, -150, ..., 150
const LON_STEP_DEG: f64 = 30.0;
const LON_START_DEG: f64 = -180.0;

/// The dense search used only to find peaks -- see the module doc's "Peak population"
/// bullet. -85..85 at 5 deg (poles excluded, `TangentFrame` degenerates there) and
/// -180..175 at 5 deg, no antimeridian duplicate.
const PEAK_LAT_STEPS: i32 = 35;
const PEAK_LAT_STEP_DEG: f64 = 5.0;
const PEAK_LAT_START_DEG: f64 = -85.0;
const PEAK_LON_STEPS: i32 = 72;
const PEAK_LON_STEP_DEG: f64 = 5.0;
const PEAK_LON_START_DEG: f64 = -180.0;
const PEAK_SITE_COUNT: usize = 20;

const HIGH_GROUND_STRUCTURAL_M: f64 = 300.0;

const TRANSECT_HALF_SPAN_M: f64 = 1000.0; // 2 km transect, centred on the site
const TRANSECT_STEP_M: f64 = 50.0;

const LACUNARITY: f64 = 2.0;

const MOUNTAIN_MULTIPLIERS: &[f64] = &[1.0, 2.0, 3.0, 4.0];
const QUIETING_STRENGTHS: &[f64] = &[0.7, 0.35, 0.0, -0.35, -0.7];
const PERSISTENCES: &[f64] = &[0.5, 0.65, 0.71, 0.75];

/// House explicit-branch form for `min`/`max`/percentile-style folds -- `f64::min`,
/// `f64::max` and `.clamp(` are banned by this slice's own brief (not caught by the build
/// guard, which is exactly why they must not appear here), matching `plates.rs::margin_at`
/// and `pond_threshold_survey.rs`'s own `min_of`/`max_of`.
fn min_of(values: &[f64]) -> f64 {
    let mut m = values[0];
    for &v in &values[1..] {
        if v < m {
            m = v;
        }
    }
    m
}

fn max_of(values: &[f64]) -> f64 {
    let mut m = values[0];
    for &v in &values[1..] {
        if v > m {
            m = v;
        }
    }
    m
}

/// Sorted-array median; `values` must already be sorted ascending.
fn median_of_sorted(values: &[f64]) -> f64 {
    let n = values.len();
    if n % 2 == 1 {
        values[n / 2]
    } else {
        let a = values[n / 2 - 1];
        let b = values[n / 2];
        (a + b) / 2.0
    }
}

/// Linear-interpolation percentile; `values` must already be sorted ascending. `p` in
/// `[0.0, 1.0]`. `rank = p * (n - 1)` is the common convention (numpy's default), not a
/// nearest-rank pick, so a single outlier does not move p90 by a whole sample step.
fn percentile_of_sorted(values: &[f64], p: f64) -> f64 {
    let n = values.len();
    if n == 1 {
        return values[0];
    }
    let last = (n - 1) as f64; // cast-ok: a count, widened to interpolate a fractional rank, not truncated back
    let rank = p * last;
    let lower_f = m::floor(rank);
    // `rank` is in [0, last] and `last >= 0`, so `lower_f` is already a non-negative
    // integer value stored as f64 -- this is the position, not a magnitude being
    // truncated toward zero, so the floor/truncate trap `no_std_math.rs` guards against
    // does not apply here (see that file's own doc on why `as i64`/`as u64` are banned).
    let lower_idx = lower_f as usize;
    let frac = rank - lower_f;
    if lower_idx + 1 >= n {
        return values[lower_idx];
    }
    values[lower_idx] + frac * (values[lower_idx + 1] - values[lower_idx])
}

/// The Hurst exponent this schedule's persistence implies at a fixed lacunarity. See the
/// module doc's "governing group" bullet.
fn hurst_exponent(persistence: f64, lacunarity: f64) -> f64 {
    m::ln(1.0 / persistence) / m::ln(lacunarity)
}

/// A land site: its lat/lon (for building a fresh `TangentFrame` per configuration) and
/// its point, kept together so the point need not be rebuilt from lat/lon on every call.
struct Site {
    lat_deg: f64,
    lon_deg: f64,
    point: SpherePoint,
}

fn candidate_grid() -> Vec<(f64, f64)> {
    let mut out = Vec::with_capacity((LAT_STEPS * LON_STEPS) as usize);
    for i in 0..LAT_STEPS {
        let lat = LAT_START_DEG + (i as f64) * LAT_STEP_DEG; // cast-ok: a grid index widened to a coordinate, not a coordinate truncated to an index
        for j in 0..LON_STEPS {
            let lon = LON_START_DEG + (j as f64) * LON_STEP_DEG; // cast-ok: a grid index widened to a coordinate, not a coordinate truncated to an index
            out.push((lat, lon));
        }
    }
    out
}

fn peak_search_grid() -> Vec<(f64, f64)> {
    let mut out = Vec::with_capacity((PEAK_LAT_STEPS * PEAK_LON_STEPS) as usize);
    for i in 0..PEAK_LAT_STEPS {
        let lat = PEAK_LAT_START_DEG + (i as f64) * PEAK_LAT_STEP_DEG; // cast-ok: a grid index widened to a coordinate, not a coordinate truncated to an index
        for j in 0..PEAK_LON_STEPS {
            let lon = PEAK_LON_START_DEG + (j as f64) * PEAK_LON_STEP_DEG; // cast-ok: a grid index widened to a coordinate, not a coordinate truncated to an index
            out.push((lat, lon));
        }
    }
    out
}

/// The `n` candidates with the highest STRUCTURAL elevation on the reference surface --
/// relief-independent, so this ranking (and therefore the peak population) does not move
/// as the swept parameters do. Sorted descending by `partial_cmp` (no NaN expected from a
/// deterministic elevation field) and truncated to `n` -- 2,520 candidates is cheap to
/// sort outright, so there is no reason to hand-roll a partial selection here.
fn top_n_by_structural(reference: &Surface, candidates: &[(f64, f64)], n: usize) -> Vec<Site> {
    let mut scored: Vec<(f64, f64, f64)> = candidates
        .iter()
        .map(|&(lat, lon)| {
            let point = SpherePoint::from_latlon(lat, lon);
            (reference.structural_m(&point), lat, lon)
        })
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).expect("no NaN structural elevation"));
    scored
        .into_iter()
        .take(n)
        .map(|(_, lat, lon)| Site { lat_deg: lat, lon_deg: lon, point: SpherePoint::from_latlon(lat, lon) })
        .collect()
}

/// Every candidate whose STRUCTURAL elevation (relief-independent) is above `threshold_m`
/// on the given reference surface.
fn sites_above(reference: &Surface, candidates: &[(f64, f64)], threshold_m: f64) -> Vec<Site> {
    let mut out = Vec::new();
    for &(lat, lon) in candidates {
        let point = SpherePoint::from_latlon(lat, lon);
        if reference.structural_m(&point) > threshold_m {
            out.push(Site { lat_deg: lat, lon_deg: lon, point });
        }
    }
    out
}

/// `(relief_m, max_gradient)` for one site's 2 km transect on the given (already
/// relief-configured) surface.
fn transect_relief_and_gradient(surface: &Surface, site: &Site) -> (f64, f64) {
    let frame = TangentFrame::at_latlon(site.lat_deg, site.lon_deg, EARTH_RADIUS_M);
    let mut elevations = Vec::new();
    let mut x = -TRANSECT_HALF_SPAN_M;
    while x <= TRANSECT_HALF_SPAN_M {
        let p = frame.local_to_sphere(x, 0.0);
        elevations.push(surface.elevation_m(&p, None));
        x += TRANSECT_STEP_M;
    }
    let relief_m = max_of(&elevations) - min_of(&elevations);
    let mut max_gradient = 0.0_f64;
    for pair in elevations.windows(2) {
        let step = (pair[1] - pair[0]).abs() / TRANSECT_STEP_M;
        if step > max_gradient {
            max_gradient = step;
        }
    }
    (relief_m, max_gradient)
}

/// `|detail_m| / elevation_m` at one high-ground site's own point (not a transect).
fn detail_share(surface: &Surface, site: &Site) -> f64 {
    let elevation_m = surface.elevation_m(&site.point, None);
    let structural_m = surface.structural_m(&site.point);
    let detail_m = elevation_m - structural_m;
    detail_m.abs() / elevation_m
}

/// The three reported numbers -- relief distribution, worst gradient, detail's share --
/// for one already-relief-configured `Surface` over one population.
struct Row {
    relief_median_m: f64,
    relief_p90_m: f64,
    relief_max_m: f64,
    max_gradient_pct: f64,
    share_median_pct: f64,
    share_max_pct: f64,
}

/// `transect_sites` feeds relief/gradient (walked as a 2 km transect each);
/// `share_sites` feeds the detail-term share (read at the site's own point). The two
/// callers below pass the same slice for both (peaks) or different ones (land vs.
/// high-ground), so this stays one function rather than two near-duplicates.
fn measure(surface: &Surface, transect_sites: &[Site], share_sites: &[Site]) -> Row {
    let mut reliefs_m = Vec::with_capacity(transect_sites.len());
    let mut overall_max_gradient = 0.0_f64;
    for site in transect_sites {
        let (relief_m, max_gradient) = transect_relief_and_gradient(surface, site);
        reliefs_m.push(relief_m);
        if max_gradient > overall_max_gradient {
            overall_max_gradient = max_gradient;
        }
    }
    reliefs_m.sort_by(|a, b| a.partial_cmp(b).expect("no NaN relief"));

    let mut shares = Vec::with_capacity(share_sites.len());
    for site in share_sites {
        shares.push(detail_share(surface, site));
    }
    let (share_median_pct, share_max_pct) = if shares.is_empty() {
        (f64::NAN, f64::NAN)
    } else {
        shares.sort_by(|a, b| a.partial_cmp(b).expect("no NaN share"));
        (median_of_sorted(&shares) * 100.0, max_of(&shares) * 100.0)
    };

    Row {
        relief_median_m: median_of_sorted(&reliefs_m),
        relief_p90_m: percentile_of_sorted(&reliefs_m, 0.9),
        relief_max_m: max_of(&reliefs_m),
        max_gradient_pct: overall_max_gradient * 100.0,
        share_median_pct,
        share_max_pct,
    }
}

fn print_row(mountain_m: f64, quieting_strength: f64, persistence: f64, row: &Row) {
    println!(
        "  {:>10.1}  {:>8.2}  {:>11.2}  {:>6.4}  {:>15.2}  {:>12.2}  {:>13.2}  \
         {:>16.3}  {:>23.2}  {:>21.2}",
        mountain_m,
        quieting_strength,
        persistence,
        hurst_exponent(persistence, LACUNARITY),
        row.relief_median_m,
        row.relief_p90_m,
        row.relief_max_m,
        row.max_gradient_pct,
        row.share_median_pct,
        row.share_max_pct,
    );
}

const TABLE_HEADER: &str = "  mountain_m  quieting  persistence  H       relief_median_m  \
     relief_p90_m  relief_max_m  max_gradient_pct  detail_share_median_pct  \
     detail_share_max_pct";

fn main() {
    println!("relief_survey: seed {SEED}, radius {EARTH_RADIUS_M} m, plates {DEFAULT_PLATE_COUNT}, land_fraction {LAND_FRACTION}");

    let candidates = candidate_grid();
    let reference = Surface::new(SEED, EARTH_RADIUS_M, DEFAULT_PLATE_COUNT, LAND_FRACTION, None, None);
    let land_sites = sites_above(&reference, &candidates, 0.0);
    let high_ground_sites = sites_above(&reference, &candidates, HIGH_GROUND_STRUCTURAL_M);
    let peak_candidates = peak_search_grid();
    let peak_sites = top_n_by_structural(&reference, &peak_candidates, PEAK_SITE_COUNT);

    println!(
        "  {} land candidates ({} lat x {} lon), {} land sites (structural_m > 0), {} \
         high-ground sites (structural_m > {HIGH_GROUND_STRUCTURAL_M} m)",
        candidates.len(),
        LAT_STEPS,
        LON_STEPS,
        land_sites.len(),
        high_ground_sites.len(),
    );
    println!(
        "  {} peak-search candidates ({} lat x {} lon), top {} by structural_m kept as the \
         peak population (highest {:.1} m .. {:.1} m)",
        peak_candidates.len(),
        PEAK_LAT_STEPS,
        PEAK_LON_STEPS,
        peak_sites.len(),
        peak_sites.first().map(|s| reference.structural_m(&s.point)).unwrap_or(f64::NAN),
        peak_sites.last().map(|s| reference.structural_m(&s.point)).unwrap_or(f64::NAN),
    );
    println!(
        "  every site list above is fixed against the CANONICAL reference surface and reused \
         unchanged at every configuration below"
    );
    if land_sites.is_empty() || peak_sites.is_empty() {
        println!("  a required population came back empty -- nothing to measure");
        return;
    }

    println!();
    for &persistence in PERSISTENCES {
        println!(
            "  persistence {persistence:.2} -> H = ln(1/persistence)/ln({LACUNARITY}) = {:.4}",
            hurst_exponent(persistence, LACUNARITY)
        );
    }

    let canonical = ReliefParams::canonical();

    println!();
    println!(
        "LAND POPULATION ({} sites, transect + high-ground detail share over {} sites):",
        land_sites.len(),
        high_ground_sites.len()
    );
    println!("{TABLE_HEADER}");
    for &mountain_mult in MOUNTAIN_MULTIPLIERS {
        for &quieting_strength in QUIETING_STRENGTHS {
            for &persistence in PERSISTENCES {
                let relief = ReliefParams {
                    mountain_m: canonical.mountain_m * mountain_mult,
                    quieting_strength,
                    octave_persistence: persistence,
                    ..canonical
                };
                let surface =
                    Surface::new(SEED, EARTH_RADIUS_M, DEFAULT_PLATE_COUNT, LAND_FRACTION, None, Some(relief));
                let row = measure(&surface, &land_sites, &high_ground_sites);
                print_row(relief.mountain_m, quieting_strength, persistence, &row);
            }
        }
    }

    println!();
    println!(
        "PEAK POPULATION ({} highest-structural-elevation sites, transect + detail share \
         both over the same {} sites -- every one is already high ground):",
        peak_sites.len(),
        peak_sites.len()
    );
    println!("{TABLE_HEADER}");
    for &mountain_mult in MOUNTAIN_MULTIPLIERS {
        for &quieting_strength in QUIETING_STRENGTHS {
            for &persistence in PERSISTENCES {
                let relief = ReliefParams {
                    mountain_m: canonical.mountain_m * mountain_mult,
                    quieting_strength,
                    octave_persistence: persistence,
                    ..canonical
                };
                let surface =
                    Surface::new(SEED, EARTH_RADIUS_M, DEFAULT_PLATE_COUNT, LAND_FRACTION, None, Some(relief));
                let row = measure(&surface, &peak_sites, &peak_sites);
                print_row(relief.mountain_m, quieting_strength, persistence, &row);
            }
        }
    }
}
