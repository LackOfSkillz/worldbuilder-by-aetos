//! Measures the STRUCTURE field across `TectonicParams`' new parameter space, and chooses
//! nothing.
//!
//! Task 2 (replaced scope) of the mountains slice
//! (`.superpowers/sdd/2026-09-05-slice-mountains/task-2-brief.md`). The original Task 2 was
//! a sweep of amplitude and width; Task 4 shipped those sliders and the picture settled the
//! question. `shots/wide-steep-6000m-100km.png` against `shots/wide-canonical.png` is the
//! same landform rescaled -- **a blade, not a range** -- and the probe measured that blade
//! at a 7.03% flank grade against the Himalaya's published 7.0%. The grade was already
//! right. This binary measures what the grade cannot see.
//!
//! ```text
//! cargo run --release --bin mountain_survey
//! ```
//!
//! This is a `[[bin]]`, not a `cargo test`-visible fixture, for the reason
//! `relief_survey.rs`, `erosion_convergence_sweep.rs` and `pond_threshold_survey.rs`
//! already are: several dozen full-planet configurations, each rebuilding a `Surface` and
//! scanning a quarter-million sites, is a one-machine measurement question and not a
//! property a suite that runs on every push should re-pay. **It contributes zero tests to
//! all five configuration counts**, and it builds under every feature configuration
//! including `wasm32-unknown-unknown` because it touches nothing feature-gated -- only
//! `Surface`, `SpherePoint`, `TectonicParams` and `Noise`, all of which are in the crate's
//! unconditional surface.
//!
//! # Method -- every figure below names its population, its method with parameters, and its
//! host
//!
//! - **Host:** named by whoever runs it; `cargo run --release`. The counts and ratios here
//!   are properties of the algorithm and transfer; any millisecond figure would not.
//! - **World:** the owner's own, from their screenshot and the same one `mountain_probe.rs`
//!   used, so this slice's tables compose -- **seed 123,925,603, radius 4,500,000 m, 28
//!   plates, land fraction 0.16**. Ours is a 4,500 km planet and nobody else's is, which is
//!   the whole reason the shaping constants are swept here rather than inherited.
//! - **Peak population:** a 0.5-degree global grid (720 x 359 = 258,480 sites), refined at
//!   0.05 degrees in a 2-degree box around the coarse maximum. `Surface::elevation_m(point,
//!   None)` throughout -- physics ground truth, never a viewer's resampled height.
//! - **Steepest flank grade:** the steepest single 2 km step, 12 bearings walked out from
//!   the peak to 250 km. **Reused from `mountain_probe.rs` exactly, including its documented
//!   mistake:** the first version of that probe measured rise across a run CENTRED ON THE
//!   PEAK -- the one point on a mountain where the slope is zero by definition -- and
//!   reported that narrowing a range made it SHALLOWER. Nothing here measures across the
//!   summit.
//! - **Relief over a 2 km transect:** at the site of that steepest step, `max - min` over 41
//!   samples at 50 m along local east, matching the relief slice's tables so the two
//!   compose.
//! - **Summit count:** see [`summits_in_range`]. This is the number that says "range" rather
//!   than "blade", and no existing measurement in this project captures it.
//! - **Asymmetry ratio:** see [`flank_ratio`]. Target 1.67, from Naylor & Sinclair (2008).
//!
//! # What this binary must not do
//!
//! It changes no default and no `canonical()` value. Task 3 chooses a named preset from
//! these numbers; this file only measures.

use worldbuilder_engine::noise::Noise;
use worldbuilder_engine::sphere::SpherePoint;
use worldbuilder_engine::surface::Surface;
use worldbuilder_engine::tectonics::{TectonicParams, MAX_TECTONIC_RANGE_M};

const SEED: i64 = 123_925_603;
const RADIUS_M: f64 = 4_500_000.0;
const PLATES: usize = 28;
const LAND: f64 = 0.16;

/// The envelope this slice's Task 4 shipped as its steep end, and the one the blade
/// screenshot was taken at. Every structure configuration below is measured ON TOP of it,
/// so the tables answer "what does structure add to the shipped steep setting" rather than
/// "what does some other envelope look like".
const STEEP_AMPLITUDE_M: f64 = 6000.0;
const STEEP_WIDTH_M: f64 = 100_000.0;

/// How far from the peak counts as "within one range", in degrees of latitude and
/// longitude. 3 degrees is about 236 km at this radius, so the box is roughly 470 km across
/// -- comfortably wider than the widest profile here reaches (`MAX_TECTONIC_RANGE_M` is
/// 420 km) and therefore a box around ONE range rather than a slice of several.
const RANGE_BOX_DEG: f64 = 3.0;

/// Grid spacing inside that box, in degrees. 0.02 degrees is about 1.57 km at this radius,
/// which resolves a crest but not the noise below it.
const SUMMIT_STEP_DEG: f64 = 0.02;

/// A summit must stand this far above its surroundings to count as its own summit rather
/// than a bump on somebody else's shoulder. 300 m is the widely used P300 rule for a
/// distinct hill, adopted here because it is an external standard and not one this project
/// tuned until it liked the answer.
const SUMMIT_PROMINENCE_M: f64 = 300.0;

/// And it must stand this high absolutely -- the brief's own threshold.
const SUMMIT_HEIGHT_M: f64 = 1000.0;

/// How far to walk when looking for a summit's key col, in grid cells. 25 cells at 0.02
/// degrees is about 39 km: far enough to find the saddle between two crests of one range,
/// near enough not to wander into the next range.
const COL_REACH_CELLS: usize = 25;

/// Vertical exaggeration for the shaded-relief dump. See `hillshade_ppm` for why it is not
/// 1.0 and why the same factor is used for every image in the set.
const SHADE_EXAGGERATION: f64 = 8.0;

fn surface_with(params: TectonicParams) -> Surface {
    Surface::new(SEED, RADIUS_M, PLATES, LAND, None, None, Some(params))
}

/// The highest point on the planet, and where it is. Coarse global scan then a local
/// refinement -- `mountain_probe.rs`'s `peak_of`, unchanged, so the two binaries' peak
/// figures are the same measurement.
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

/// Degrees of latitude per metre at this radius. Longitude also needs a `cos(lat)`.
fn deg_per_m() -> f64 {
    1.0 / (RADIUS_M * core::f64::consts::PI / 180.0)
}

/// The steepest single 2 km step anywhere on the flank, and where that step was, walking
/// outward from the peak in 12 bearings to 250 km.
///
/// **Do not measure across the summit.** A bump's derivative is zero at its maximum by
/// definition; the first version of `mountain_probe.rs` measured a run centred on the peak
/// and concluded that narrowing a range made it shallower, which is the opposite of the
/// truth. The steep part is the flank. This function is that probe's `grade_at` with one
/// addition -- it also returns the SITE of the steepest step, so the transect below can be
/// taken there rather than at the summit.
fn steepest_flank(surface: &Surface, lat: f64, lon: f64) -> (f64, f64, f64) {
    let step_m = 2_000.0;
    let reach_m = 250_000.0;
    let per_m = deg_per_m();
    let mut steepest = (0.0f64, lat, lon);
    let mut bearing = 0.0;
    while bearing < 360.0 {
        let rad = bearing * core::f64::consts::PI / 180.0;
        let dlat = libm::cos(rad) * step_m * per_m;
        let dlon =
            libm::sin(rad) * step_m * per_m / libm::cos(lat * core::f64::consts::PI / 180.0);
        let mut i = 0.0;
        let mut previous = surface.elevation_m(&SpherePoint::from_latlon(lat, lon), None);
        while i * step_m < reach_m {
            i += 1.0;
            let (plat, plon) = (lat + dlat * i, lon + dlon * i);
            let here = surface.elevation_m(&SpherePoint::from_latlon(plat, plon), None);
            let rise = if previous > here { previous - here } else { here - previous };
            let grade = 100.0 * rise / step_m;
            if grade > steepest.0 {
                steepest = (grade, plat - dlat * 0.5, plon - dlon * 0.5);
            }
            previous = here;
        }
        bearing += 30.0;
    }
    steepest
}

/// Relief over a 2 km transect: `max - min` over 41 samples at 50 m along local east.
///
/// The spacing and the length are the relief slice's, deliberately, so this column can be
/// read beside that slice's tables rather than beside nothing.
fn relief_2km_m(surface: &Surface, lat: f64, lon: f64) -> f64 {
    let per_m = deg_per_m();
    let scale = per_m / libm::cos(lat * core::f64::consts::PI / 180.0);
    let mut lowest = f64::INFINITY;
    let mut highest = f64::NEG_INFINITY;
    for i in -20..=20 {
        let offset_m = f64::from(i) * 50.0;
        let e = surface.elevation_m(&SpherePoint::from_latlon(lat, lon + offset_m * scale), None);
        if e < lowest {
            lowest = e;
        }
        if e > highest {
            highest = e;
        }
    }
    highest - lowest
}

/// **The count of distinct summits above 1,000 m within one range -- the number that says
/// "range" rather than "blade".**
///
/// Method, stated in full because no existing measurement in this project captures this and
/// a count without a method is a number about nothing:
///
/// 1. Sample a `2 * RANGE_BOX_DEG` square centred on the peak at `SUMMIT_STEP_DEG`, giving a
///    301 x 301 grid of `Surface::elevation_m(point, None)`.
/// 2. A candidate is a cell strictly higher than all eight of its neighbours and above
///    `SUMMIT_HEIGHT_M`. Strictly, so a flat crest yields none rather than all of them.
/// 3. A candidate becomes a summit only if its **prominence** clears
///    `SUMMIT_PROMINENCE_M`: walk out in the eight compass directions up to
///    `COL_REACH_CELLS`, stopping a direction when the ground rises above the candidate,
///    and take the highest of the eight minima found. Prominence is the candidate minus
///    that col. This is the standard key-col construction restricted to eight rays and a
///    bounded reach, which is an approximation and is stated as one: it can only ever
///    OVERSTATE a col (a ray may miss the true saddle), so it can only ever UNDERSTATE
///    prominence and therefore undercount summits. A method that erred the other way would
///    be the dangerous one for this task, because this task wants the count to be large.
/// 4. Returns the count, and the second-highest summit's height -- one summit is a peak, and
///    the height of the runner-up is what says whether the others are real mountains or
///    ripples on a single crest.
fn summits_in_range(surface: &Surface, lat: f64, lon: f64) -> (usize, f64) {
    let steps = (2.0 * RANGE_BOX_DEG / SUMMIT_STEP_DEG) as usize + 1; // cast-ok: a grid size from stated constants, ~301
    let mut grid = vec![0.0f64; steps * steps];
    for row in 0..steps {
        let plat = lat - RANGE_BOX_DEG + row as f64 * SUMMIT_STEP_DEG; // cast-ok: grid index to float, exact
        for column in 0..steps {
            let plon = lon - RANGE_BOX_DEG + column as f64 * SUMMIT_STEP_DEG; // cast-ok: grid index to float, exact
            grid[row * steps + column] =
                surface.elevation_m(&SpherePoint::from_latlon(plat, plon), None);
        }
    }

    let mut heights: Vec<f64> = Vec::new();
    for row in 1..steps - 1 {
        for column in 1..steps - 1 {
            let here = grid[row * steps + column];
            if here <= SUMMIT_HEIGHT_M {
                continue;
            }
            let mut highest_neighbour = f64::NEG_INFINITY;
            for dr in [-1i64, 0, 1] {
                for dc in [-1i64, 0, 1] {
                    if dr == 0 && dc == 0 {
                        continue;
                    }
                    let r = (row as i64 + dr) as usize; // cast-ok: bounded by the loop, 1..steps-1
                    let c = (column as i64 + dc) as usize; // cast-ok: bounded by the loop, 1..steps-1
                    let v = grid[r * steps + c];
                    if v > highest_neighbour {
                        highest_neighbour = v;
                    }
                }
            }
            if here <= highest_neighbour {
                continue;
            }

            // The key col, approximated by eight rays. See this function's own doc for why
            // the approximation is safe in the direction it errs.
            let mut col = f64::NEG_INFINITY;
            for (dr, dc) in
                [(-1i64, 0i64), (1, 0), (0, -1), (0, 1), (-1, -1), (-1, 1), (1, -1), (1, 1)]
            {
                let mut lowest = here;
                for step in 1..=COL_REACH_CELLS {
                    let r = row as i64 + dr * step as i64; // cast-ok: a bounded walk length to signed
                    let c = column as i64 + dc * step as i64; // cast-ok: a bounded walk length to signed
                    if r < 0 || c < 0 || r as usize >= steps || c as usize >= steps {
                        break;
                    }
                    let v = grid[r as usize * steps + c as usize]; // cast-ok: bounds checked immediately above
                    if v > here {
                        break;
                    }
                    if v < lowest {
                        lowest = v;
                    }
                }
                if lowest > col {
                    col = lowest;
                }
            }
            if here - col >= SUMMIT_PROMINENCE_M {
                heights.push(here);
            }
        }
    }

    heights.sort_by(|a, b| b.partial_cmp(a).expect("elevations are never NaN"));
    let runner_up = if heights.len() > 1 { heights[1] } else { 0.0 };
    (heights.len(), runner_up)
}

/// **Cross-range profile asymmetry: the ratio of the two flank widths at half height.**
/// Target ~1.67 (Naylor & Sinclair 2008: a 115 km pro-wedge against a 69 km retro-wedge at
/// H_max = 3 km, the exact inverse of alpha_retro/alpha_pro = 2.5/1.5).
///
/// Method: walk the same 12 bearings out to 250 km at 2 km. For each bearing, the flank
/// width is the distance at which the ground first falls to half way between the peak and
/// that bearing's own minimum -- its own minimum, because a bearing running out to sea and
/// one running onto a plateau have different bases and a shared base would compare a range
/// against a coastline. Then take the OPPOSITE PAIR whose two widths sum smallest: that is
/// the across-range axis, because a range is by construction narrow across and long along,
/// and picking the largest pair would measure the crest line instead of the profile.
///
/// Returns (ratio of wider flank to narrower, wider width, narrower width, the bearing
/// index of the across-range axis).
fn flank_ratio(surface: &Surface, lat: f64, lon: f64, peak_m: f64) -> (f64, f64, f64, usize) {
    let step_m = 2_000.0;
    let reach_m = 250_000.0;
    let per_m = deg_per_m();
    let mut widths = [0.0f64; 12];
    for (index, width) in widths.iter_mut().enumerate() {
        let bearing = index as f64 * 30.0; // cast-ok: a bearing index to float, exact
        let rad = bearing * core::f64::consts::PI / 180.0;
        let dlat = libm::cos(rad) * step_m * per_m;
        let dlon =
            libm::sin(rad) * step_m * per_m / libm::cos(lat * core::f64::consts::PI / 180.0);

        // Two passes: the base along this bearing, then where half height is crossed.
        let mut base = peak_m;
        let mut i = 1.0;
        while i * step_m <= reach_m {
            let e = surface
                .elevation_m(&SpherePoint::from_latlon(lat + dlat * i, lon + dlon * i), None);
            if e < base {
                base = e;
            }
            i += 1.0;
        }
        let half = base + 0.5 * (peak_m - base);
        let mut crossed = reach_m;
        let mut i = 1.0;
        while i * step_m <= reach_m {
            let e = surface
                .elevation_m(&SpherePoint::from_latlon(lat + dlat * i, lon + dlon * i), None);
            if e <= half {
                crossed = i * step_m;
                break;
            }
            i += 1.0;
        }
        *width = crossed;
    }

    let mut narrowest = (f64::INFINITY, 0.0, 0.0, 0usize);
    for index in 0..6 {
        let (a, b) = (widths[index], widths[index + 6]);
        if a + b < narrowest.0 {
            narrowest = (a + b, a, b, index);
        }
    }
    let (_, a, b, axis) = narrowest;
    let (wide, narrow) = if a >= b { (a, b) } else { (b, a) };
    let ratio = if narrow > 0.0 { wide / narrow } else { f64::INFINITY };
    (ratio, wide, narrow, axis)
}

/// Latitude and longitude steps for one `step_m` move along `bearing_deg` from `lat`.
fn stride(lat: f64, bearing_deg: f64, step_m: f64) -> (f64, f64) {
    let per_m = deg_per_m();
    let rad = bearing_deg * core::f64::consts::PI / 180.0;
    (
        libm::cos(rad) * step_m * per_m,
        libm::sin(rad) * step_m * per_m / libm::cos(lat * core::f64::consts::PI / 180.0),
    )
}

/// **How far the crest wanders off a straight line, in kilometres.**
///
/// Added because the first run of this survey rejected the crest warp on four measurements
/// none of which can see a crest line's shape -- the exact failure this project keeps
/// recording, where a probe exercises a stage and is blind to the thing that stage does. A
/// technique deserves to be judged on its own axis, so this is that axis.
///
/// Method: the along-range bearing is the across-range axis plus 90 degrees. Walk it from
/// -150 km to +150 km at 5 km (61 stations). At each station scan PERPENDICULAR, -80 km to
/// +80 km at 2 km, and record the perpendicular offset of the highest sample -- the crest's
/// position at that station.
///
/// **Then DE-TREND, and that is not a refinement, it is the difference between a
/// measurement and an artefact.** The first version of this function reported 26.3 km RMS
/// wander on a crest with the warp switched OFF, i.e. on a crest lying exactly along a
/// straight plate bisector. The reason: the across-range axis is chosen from twelve
/// bearings at 30-degree spacing, so the along-range walk can be up to 15 degrees off the
/// true crest, and 15 degrees over 150 km is 39 km of drift. A bearing error is exactly a
/// LINEAR trend in the offsets, so a least-squares line is fitted and removed, and what is
/// reported is the residual about it. A straight crest then reads near zero however badly
/// the axis was quantised, which is the property the number was supposed to have.
///
/// Returns the largest absolute residual and the RMS residual, both in kilometres, and the
/// count of stations whose peak landed on the edge of the scan -- those are unresolved and
/// a large count means the scan was too narrow to see the crest at all.
fn crest_wander_km(surface: &Surface, lat: f64, lon: f64, across_axis: usize) -> (f64, f64, usize) {
    let along_deg = across_axis as f64 * 30.0 + 90.0; // cast-ok: a bearing index to float, exact
    let across_deg = across_axis as f64 * 30.0; // cast-ok: a bearing index to float, exact
    let (along_dlat, along_dlon) = stride(lat, along_deg, 5_000.0);
    let (across_dlat, across_dlon) = stride(lat, across_deg, 2_000.0);

    let mut offsets: Vec<(f64, f64)> = Vec::new();
    let mut unresolved = 0usize;
    for station in -30i64..=30 {
        let s = station as f64; // cast-ok: a station index to float, exact
        let (slat, slon) = (lat + along_dlat * s, lon + along_dlon * s);
        let mut best = (f64::NEG_INFINITY, 0.0f64);
        for offset in -40i64..=40 {
            let o = offset as f64; // cast-ok: an offset index to float, exact
            let e = surface.elevation_m(
                &SpherePoint::from_latlon(slat + across_dlat * o, slon + across_dlon * o),
                None,
            );
            if e > best.0 {
                best = (e, o * 2.0);
            }
        }
        if best.1.abs() >= 80.0 {
            unresolved += 1;
        }
        offsets.push((s * 5.0, best.1));
    }

    // Least squares through (along_km, offset_km), then the residual about it.
    let n = offsets.len() as f64; // cast-ok: a station count to float, exact
    let mean_x = offsets.iter().map(|(x, _)| x).sum::<f64>() / n;
    let mean_y = offsets.iter().map(|(_, y)| y).sum::<f64>() / n;
    let covariance: f64 = offsets.iter().map(|(x, y)| (x - mean_x) * (y - mean_y)).sum();
    let variance: f64 = offsets.iter().map(|(x, _)| (x - mean_x) * (x - mean_x)).sum();
    let slope = if variance > 0.0 { covariance / variance } else { 0.0 };

    let mut largest = 0.0f64;
    let mut squares = 0.0f64;
    for (x, y) in &offsets {
        let residual = y - (mean_y + slope * (x - mean_x));
        let magnitude = residual.abs();
        if magnitude > largest {
            largest = magnitude;
        }
        squares += residual * residual;
    }
    (largest, libm::sqrt(squares / n), unresolved)
}

/// **How many separate crests the range has ACROSS its width.**
///
/// Added for the same reason as `crest_wander_km`, and it is the more important of the two:
/// the first run of this survey showed stacked sutures buying no summits at all, and the
/// reason is that [`summits_in_range`] counts POINT maxima on a 2D grid while a suture is a
/// crest LINE. A straight ridge of constant height has no point maxima along it, so a
/// measurement that only counts points is structurally blind to the one axis technique 3
/// exists to supply. Rejecting it on that number would have been rejecting it on a blind
/// probe.
///
/// Method: walk the across-range axis from -250 km to +250 km at 2 km (251 samples), and
/// count strict 1-D local maxima above `SUMMIT_HEIGHT_M` whose prominence along that line
/// clears `SUMMIT_PROMINENCE_M` -- walking each way until the profile rises above the
/// candidate or 40 km is spent, and taking the higher of the two minima as the col.
fn crests_across(surface: &Surface, lat: f64, lon: f64, across_axis: usize) -> usize {
    let across_deg = across_axis as f64 * 30.0; // cast-ok: a bearing index to float, exact
    let (dlat, dlon) = stride(lat, across_deg, 2_000.0);
    let samples: Vec<f64> = (-125i64..=125)
        .map(|i| {
            let f = i as f64; // cast-ok: a sample index to float, exact
            surface.elevation_m(&SpherePoint::from_latlon(lat + dlat * f, lon + dlon * f), None)
        })
        .collect();

    let mut count = 0usize;
    for index in 1..samples.len() - 1 {
        let here = samples[index];
        if here <= SUMMIT_HEIGHT_M || here <= samples[index - 1] || here <= samples[index + 1] {
            continue;
        }
        let mut col = f64::NEG_INFINITY;
        for direction in [-1i64, 1] {
            let mut lowest = here;
            for step in 1..=20i64 {
                let probe = index as i64 + direction * step; // cast-ok: a bounded walk to signed
                if probe < 0 || probe as usize >= samples.len() {
                    break;
                }
                let v = samples[probe as usize]; // cast-ok: bounds checked immediately above
                if v > here {
                    break;
                }
                if v < lowest {
                    lowest = v;
                }
            }
            if lowest > col {
                col = lowest;
            }
        }
        if here - col >= SUMMIT_PROMINENCE_M {
            count += 1;
        }
    }
    count
}

/// One row of the survey. Everything above, on one configuration.
fn row(label: &str, params: TectonicParams) {
    let reach = params.collision_reach_m();
    let surface = surface_with(params);
    let (peak_m, lat, lon) = peak_of(&surface);
    let (grade, slat, slon) = steepest_flank(&surface, lat, lon);
    let relief_m = relief_2km_m(&surface, slat, slon);
    let (summits, runner_up_m) = summits_in_range(&surface, lat, lon);
    let (ratio, wide_m, narrow_m, axis) = flank_ratio(&surface, lat, lon, peak_m);
    let crests = crests_across(&surface, lat, lon, axis);
    let (wander_max_km, wander_rms_km, unresolved) = crest_wander_km(&surface, lat, lon, axis);
    println!(
        "{label:<40} peak {peak_m:8.1}  grade {grade:6.3}%  rel2km {relief_m:6.1}  \
         summits {summits:3}  2nd {runner_up_m:7.1}  crests {crests:2}  \
         wander {wander_max_km:5.1}/{wander_rms_km:5.1} km ({unresolved:2} unres)  \
         flanks {wide_m:6.0}/{narrow_m:6.0}  ratio {ratio:5.2}  reach {reach:7.0}"
    );
}

/// The steep envelope Task 4 shipped, with the structure fields at whatever this row is
/// testing. Everything is measured on top of this so the tables compose.
fn steep() -> TectonicParams {
    TectonicParams {
        continent_collision_m: STEEP_AMPLITUDE_M,
        continent_collision_width_m: STEEP_WIDTH_M,
        ..TectonicParams::canonical()
    }
}

/// **The ridge feedback sweep, at the FIELD level rather than the terrain level, and that
/// distinction is the point.**
///
/// `Noise::RIDGE_FEEDBACK` is a module constant reached through
/// `Noise::ridged_with_feedback`; the terrain path takes the constant. So this measures the
/// field's own statistics over a stated population and chooses the constant from them,
/// rather than pretending to have swept it end-to-end through a planet it cannot reach.
///
/// Population: 40,000 samples of a 200 x 200 lattice at 0.01 spacing on one seed, at the
/// frequency a 120 km structure wavelength produces on a 4,500 km planet. Reported: the mean
/// of the field, the fraction of it above 0.5 (how much of the belt is crest rather than
/// valley), and the largest absolute second difference along a line at 1e-3 spacing, which
/// is how sharp the crease is.
fn feedback_sweep() {
    println!("\nridge feedback sweep -- FIELD level, 40,000 samples on a 200x200 lattice at 0.01");
    println!("frequency {:.3} (a 120 km wavelength on a 4,500 km planet)", RADIUS_M / 120_000.0);
    let noise = Noise::new(123_925_603, 0x7374_7275_6374_7572);
    let frequency = RADIUS_M / 120_000.0;
    for feedback in [0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 4.0] {
        let mut total = 0.0f64;
        let mut above_half = 0usize;
        let mut count = 0usize;
        for i in 0..200 {
            for j in 0..200 {
                let x = f64::from(i) * 0.01;
                let y = f64::from(j) * 0.01;
                let v = noise.ridged_with_feedback(x, y, 0.37, frequency, 4, 0.5, 2.0, feedback);
                total += v;
                if v > 0.5 {
                    above_half += 1;
                }
                count += 1;
            }
        }
        let mut sharpest = 0.0f64;
        let step = 1e-3;
        for i in 1..4000 {
            let t = f64::from(i) * step;
            let at = |u: f64| {
                noise.ridged_with_feedback(u, 0.5, 0.5, frequency, 4, 0.5, 2.0, feedback)
            };
            let d = (at(t - step) - 2.0 * at(t) + at(t + step)).abs();
            if d > sharpest {
                sharpest = d;
            }
        }
        println!(
            "  feedback {feedback:4.1}  mean {mean:.4}  crest fraction {fraction:.4}  \
             sharpest second difference {sharpest:.3e}",
            mean = total / count as f64, // cast-ok: a sample count to float, exact
            fraction = above_half as f64 / count as f64, // cast-ok: a sample count to float, exact
        );
    }
}

/// A shaded-relief raster of the range around the peak, as a binary PPM.
///
/// **This exists because the whole task started from two screenshots and would be
/// dishonest to finish without one.** The structure fields are deliberately NOT exposed
/// through `wasm.rs` -- the brief forbids viewer work and says to stop and report if a new
/// parameter needs exposing -- so the viewer cannot draw them and a viewer screenshot is
/// not available. This renders the same physics the viewer would, straight from
/// `Surface::elevation_m`, at a stated size and framing, and writes it where a converter
/// can pick it up. It is a diagnostic dump, not a renderer.
///
/// Population: a `2 * RANGE_BOX_DEG` box centred on the peak, `size` x `size` samples.
/// Shading: the surface normal from central differences against a light 45 degrees above
/// the horizon from the north-west, multiplied into a hypsometric tint. No smoothing, no
/// gamma, no camera -- so two images differ only where the ground does.
fn hillshade_ppm(path: &str, params: TectonicParams, size: usize) -> std::io::Result<()> {
    use std::io::Write;

    let surface = surface_with(params);
    let (_, lat, lon) = peak_of(&surface);
    let span = 2.0 * RANGE_BOX_DEG;
    let step = span / size as f64; // cast-ok: a raster size to float, exact

    let mut heights = vec![0.0f64; size * size];
    for row in 0..size {
        let plat = lat + RANGE_BOX_DEG - row as f64 * step; // cast-ok: a raster index to float, exact
        for column in 0..size {
            let plon = lon - RANGE_BOX_DEG + column as f64 * step; // cast-ok: a raster index to float, exact
            heights[row * size + column] =
                surface.elevation_m(&SpherePoint::from_latlon(plat, plon), None);
        }
    }

    // Metres per sample, so the shading is in real gradient units rather than in pixels and
    // two images at different framings would still be comparable.
    let ground_m = step / deg_per_m();
    let mut pixels: Vec<u8> = Vec::with_capacity(size * size * 3);
    for row in 0..size {
        for column in 0..size {
            let here = heights[row * size + column];
            let west = heights[row * size + column.saturating_sub(1)];
            let east = heights[row * size + if column + 1 < size { column + 1 } else { column }];
            let north = heights[row.saturating_sub(1) * size + column];
            let south =
                heights[if row + 1 < size { row + 1 } else { row } * size + column];
            // Vertical exaggeration, and it is stated rather than tuned away: at true scale
            // a 7% grade over 750 m samples produces almost no shading contrast and every
            // image in this set read as flat colour bands. 8x is enough to see a ridge and
            // small enough that the same factor works for the canonical 1.8% grade and the
            // structured 17.8% one, so the five images stay comparable to each other.
            let dx = SHADE_EXAGGERATION * (east - west) / (2.0 * ground_m);
            let dy = SHADE_EXAGGERATION * (south - north) / (2.0 * ground_m);

            // Light from the north-west, 45 degrees up. `1/sqrt(3)` each component.
            let l = 0.577_350_269_189_625_7;
            let length = libm::sqrt(dx * dx + dy * dy + 1.0);
            let shade = (-dx * l + dy * l + l) / length;
            let lit = if shade > 0.15 { shade } else { 0.15 };

            let (r, g, b) = if here <= 0.0 {
                (26.0, 62.0, 110.0)
            } else if here < 800.0 {
                (96.0, 132.0, 76.0)
            } else if here < 2200.0 {
                (140.0, 126.0, 90.0)
            } else if here < 3600.0 {
                (150.0, 138.0, 128.0)
            } else {
                (232.0, 232.0, 236.0)
            };
            for channel in [r, g, b] {
                let v = channel * lit;
                let capped = if v < 255.0 { v } else { 255.0 };
                let floored = if capped > 0.0 { capped } else { 0.0 };
                pixels.push(floored as u8); // cast-ok: bounded to [0, 255] by the two branches above
            }
        }
    }

    if let Some(parent) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::File::create(path)?;
    write!(file, "P6\n{size} {size}\n255\n")?;
    file.write_all(&pixels)?;
    println!("wrote {path} -- peak at {lat:.2},{lon:.2}, +/-{RANGE_BOX_DEG} deg, {size}px");
    Ok(())
}

/// The four pictures worth having, written only when asked for -- `cargo run --release
/// --bin mountain_survey -- images`. The default run stays pure stdout, because a survey
/// that writes files as a side effect of being read is a survey nobody can run twice.
fn write_images() -> std::io::Result<()> {
    println!("\n--- shaded relief, 900 px over a +/-3 degree box at the peak");
    let shipped = TectonicParams {
        collision_asymmetry: 1.67,
        suture_count: 3,
        suture_spread_m: 100_000.0,
        structure_depth: 0.7,
        structure_wavelength_m: 80_000.0,
        ..steep()
    };
    hillshade_ppm("target/mountain-survey/1-canonical.ppm", TectonicParams::canonical(), 900)?;
    hillshade_ppm("target/mountain-survey/2-steep-blade.ppm", steep(), 900)?;
    hillshade_ppm(
        "target/mountain-survey/3-steep-asym-1.67.ppm",
        TectonicParams { collision_asymmetry: 1.67, ..steep() },
        900,
    )?;
    hillshade_ppm(
        "target/mountain-survey/4-steep-structure-0.7-80km.ppm",
        TectonicParams { structure_depth: 0.7, structure_wavelength_m: 80_000.0, ..steep() },
        900,
    )?;
    hillshade_ppm("target/mountain-survey/5-steep-all-three.ppm", shipped, 900)?;
    Ok(())
}

fn main() {
    // `images` means IMAGES ONLY. The tables and the rasters each cost a full pass over
    // several dozen planets, and a mode that silently did both would make anyone who wanted
    // one of them pay for the other -- which is how a survey stops being run.
    if std::env::args().any(|a| a == "images") {
        write_images().expect("the raster dump could not be written");
        return;
    }

    println!("world: seed {SEED}, radius {RADIUS_M} m, {PLATES} plates, land {LAND}");
    println!("range gate: {MAX_TECTONIC_RANGE_M} m");
    println!(
        "summits: local maxima above {SUMMIT_HEIGHT_M} m with prominence >= {SUMMIT_PROMINENCE_M} m, \
         in a +/-{RANGE_BOX_DEG} deg box at {SUMMIT_STEP_DEG} deg\n"
    );

    println!("--- the two settings the screenshots were taken at");
    row("canonical (1500 m / 400 km)", TectonicParams::canonical());
    row("steep (6000 m / 100 km), no structure", steep());

    println!("\n--- technique 1: the doubly-vergent asymmetric profile");
    for asymmetry in [1.0, 1.25, 1.67, 2.0, 2.5, 3.0] {
        row(
            &format!("asymmetry {asymmetry:.2}"),
            TectonicParams { collision_asymmetry: asymmetry, ..steep() },
        );
    }

    // Technique 2 -- warping the signed across-margin distance -- was built, swept here, and
    // REJECTED. Its sweep is in task-2-report.md; the parameter is gone, so this binary can
    // no longer reproduce it, and it says so rather than leaving a section that silently
    // measures nothing. The rejection and its numbers are recorded in `TectonicParams`' own
    // doc comment, where the next person to propose a crest warp will read them.

    println!("\n--- technique 3: stacked sutures");
    for count in [1u32, 2, 3, 4] {
        for spread_km in [60.0, 100.0, 150.0, 200.0] {
            let params = TectonicParams {
                suture_count: count,
                suture_spread_m: spread_km * 1000.0,
                ..steep()
            };
            let reach = params.collision_reach_m();
            let flag = if reach > MAX_TECTONIC_RANGE_M { " PAST THE GATE" } else { "" };
            row(&format!("sutures {count} at {spread_km:.0} km{flag}"), params);
        }
    }

    println!("\n--- technique 4: ridged multifractal x segmentation");
    for depth in [0.0, 0.3, 0.5, 0.7, 0.9] {
        for wavelength_km in [40.0, 80.0, 120.0, 250.0] {
            row(
                &format!("structure depth {depth:.1} at {wavelength_km:.0} km"),
                TectonicParams {
                    structure_depth: depth,
                    structure_wavelength_m: wavelength_km * 1000.0,
                    ..steep()
                },
            );
        }
    }

    println!("\n--- combinations, on the steep envelope");
    row(
        "asym 1.67 + sutures 3 at 100 km",
        TectonicParams {
            collision_asymmetry: 1.67,
            suture_count: 3,
            suture_spread_m: 100_000.0,
            ..steep()
        },
    );
    row(
        "sutures 3 at 100 km + structure 0.7/80 km",
        TectonicParams {
            suture_count: 3,
            suture_spread_m: 100_000.0,
            structure_depth: 0.7,
            structure_wavelength_m: 80_000.0,
            ..steep()
        },
    );
    row(
        "all three (asym 1.67, 3x100 km, 0.7/80 km)",
        TectonicParams {
            collision_asymmetry: 1.67,
            suture_count: 3,
            suture_spread_m: 100_000.0,
            structure_depth: 0.7,
            structure_wavelength_m: 80_000.0,
            ..steep()
        },
    );
    row(
        "all three on the CANONICAL envelope",
        TectonicParams {
            collision_asymmetry: 1.67,
            suture_count: 3,
            suture_spread_m: 100_000.0,
            structure_depth: 0.7,
            structure_wavelength_m: 80_000.0,
            ..TectonicParams::canonical()
        },
    );

    feedback_sweep();
}
