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

/// How far along the belt the sinuosity walk goes, each way, in metres, and its step.
///
/// 250 km either way is 500 km of belt -- long enough for the warp's longest octave (a
/// 600 km wavelength) to show a bend, and short enough to stay on one margin on a 4,500 km
/// planet, where 28 plates puts a plate at roughly 1,500 km across.
///
/// **The step is part of the number.** Sinuosity is a path length over a chord, and a path
/// length measured on a rough line grows as the step shrinks. Every figure this function
/// produces is at THIS step and comparable only to others at it.
const SINUOSITY_REACH_M: f64 = 250_000.0;
const SINUOSITY_STEP_M: f64 = 5_000.0;

/// How far to scan PERPENDICULAR at the seed station, and at what spacing.
///
/// **Wider than `crest_wander_km`'s +/-80 km, and it has to be.** That scan was built for a
/// crest that stays near its bisector; a warp of 80 km can put the crest at the very edge of
/// it, and a tracker that latches to the edge of its own scan reports a straight line at
/// exactly the settings that were supposed to bend it.
const SINUOSITY_SCAN_M: f64 = 200_000.0;
const SINUOSITY_SCAN_STEP_M: f64 = 2_000.0;

/// How far the crest may move between one station and the next before the tracker gives up
/// and calls that station unresolved.
///
/// **This is the fix for a measurement that read 2.01 on a line it had itself established
/// was straight.** The first version of this function took the highest sample in the whole
/// +/-200 km scan at each station independently. On a preset world that scan contains the
/// second suture belt, the far flank and whatever the neighbouring margin is doing, and the
/// global maximum jumps tens of kilometres between adjacent stations as one ridge of a
/// structure-carved flank overtakes another. Sinuosity is a path length, so per-station
/// jitter is added to it directly and without limit -- the un-warped preset read **2.0133**
/// and the canonical world, which is a smooth symmetric swell on a great circle and is as
/// straight as this engine can produce, read **2.1212**. **A metric that scores a great
/// circle at 2.12 cannot say anything about a warp**, and shipping the warp on it would have
/// been the mirror image of Task 2 rejecting the warp on four metrics that could not see it.
///
/// So the crest is FOLLOWED rather than re-found: each station searches only within this
/// distance of the previous station's crest, seeded at the station where the peak itself is
/// and walked outward in both directions from there. A crest line is a continuous curve and
/// tracking it continuously is what makes the path length a property of the curve rather
/// than of the noise around it.
///
/// 40 km per 5 km step is an 8:1 lateral-to-along rate -- far more than any belt this engine
/// draws can bend at, and far less than the ~100 km jumps between separate features that
/// were the problem. A station whose maximum sits on the edge of this window is counted
/// unresolved and printed, not silently accepted.
const CREST_TRACK_WINDOW_M: f64 = 40_000.0;

/// **The walk stops where the belt does, and this is the second half of the same fix.**
///
/// Tracking continuously stopped the crest jumping between features; it did not stop the
/// walk running off the END of the belt, and a fixed 250 km reach does exactly that. Traced
/// station by station on the bare steep envelope, the crest offset runs 26 km -> -6 km over
/// 350 km -- a straight line with the bearing quantisation's linear trend on it -- and then
/// the range simply stops: the next station's highest sample is 500 m, 136 km off axis, and
/// the two after it sit on the edge of the scan. **Three stations of that turn a sinuosity
/// of 1.0 into 1.34**, and on the canonical world into 2.13, because a path length adds
/// every excursion it is handed and cannot tell a belt from the ocean beyond it.
///
/// So a station is on the belt while its tracked crest stands above this height, and the
/// walk stops on that side the first time it does not. The value is [`SUMMIT_HEIGHT_M`] --
/// this survey's own established "this is mountain" line, already the floor
/// `summits_in_range` and `crests_across` both use -- rather than a threshold invented here
/// and tuned until the answer looked right. The resolved belt length is PRINTED in every
/// row, so a sinuosity measured over 120 km of belt cannot be read as one measured over 500.
const BELT_END_M: f64 = SUMMIT_HEIGHT_M;

/// The fraction of a station's own local relief at which the belt's outer EDGE is taken.
///
/// Half height, matching [`flank_ratio`]'s definition so the two compose, and taken against
/// each station's own crest and its own local base rather than against the planet's peak --
/// the crest height varies along a belt the structure field has carved, and a fixed absolute
/// contour would measure where the belt is TALL rather than where it ENDS.
const ENVELOPE_FRACTION: f64 = 0.5;

/// How far the local base for that contour is looked for, either side of the crest.
const ENVELOPE_BASE_REACH_M: f64 = 150_000.0;

/// How wide a notch in a flank the envelope walk steps over before calling it the end of the
/// belt.
///
/// The structure field deliberately carves ridge-and-valley relief into both flanks, so the
/// profile crosses the half-height contour many times on the way out. Stopping at the first
/// crossing would measure the innermost notch and call it the belt's edge, which on a carved
/// flank is a measurement of the structure field rather than of the envelope. 30 km is
/// narrower than the 40-80 km working band of `structure_wavelength_m`, so a real massif
/// boundary still stops the walk.
const ENVELOPE_GAP_M: f64 = 30_000.0;

/// **CREST SINUOSITY AND ENVELOPE SINUOSITY -- the acceptance metric for the along-margin
/// warp, and the measurement whose absence is the whole reason that technique was rejected
/// once already.**
///
/// Task 2 built the crest warp, measured the crest moving 49.2 -> 78.8 km, and it was
/// rejected for "moving nothing else". All four metrics it was judged on -- summit count,
/// across-range crest count, flank ratio, grade -- measure structure ACROSS a range.
/// **Straightness is a property ALONG it, and nothing in the set ever looked along.** So the
/// displacement that was the entire point of the technique scored as a null result. This
/// function is the axis that was missing.
///
/// # Method
///
/// The along-range bearing is [`flank_ratio`]'s across-range axis plus 90 degrees.
///
/// 1. **Seed.** At station 0 -- the peak's own station -- scan perpendicular over
///    `+/-SINUOSITY_SCAN_M` at `SINUOSITY_SCAN_STEP_M` and take the highest sample. The peak
///    is there by construction, so the seed is on the belt by construction.
/// 2. **Follow.** Walk outward to `+/-SINUOSITY_REACH_M` at `SINUOSITY_STEP_M`, in both
///    directions from the seed, taking at each station the highest sample within
///    `CREST_TRACK_WINDOW_M` of the previous station's crest. A station whose maximum sits on
///    the edge of that window is counted **unresolved**.
/// 3. **Envelope.** At each station, `base` is the lowest sample within
///    `ENVELOPE_BASE_REACH_M` of that station's crest and the contour is
///    `base + ENVELOPE_FRACTION * (crest - base)`. Walk outward from the crest on each flank
///    separately, keeping the furthest offset still above the contour and stopping once the
///    profile has been continuously below it for `ENVELOPE_GAP_M`.
///
/// Each of the three offset series -- crest, lower edge, upper edge -- is then reduced by
/// [`sinuosity_of`]:
///
/// **Sinuosity = path length / straight-line distance between the endpoints.** In the local
/// (along, across) frame, path length is the sum of `hypot(step, offset_i - offset_(i-1))`
/// and the straight-line distance is `hypot(total_along, offset_last - offset_first)`.
/// **A perfect great circle scores exactly 1.000**, which is the property the number is
/// named for, and `sinuosity_reads_one_on_a_straight_line` in `main`'s calibration row is
/// the check rather than the claim.
///
/// # Two things this measurement is deliberately immune to, and one it is not
///
/// **The 30-degree bearing quantisation cannot fake it.** `crest_wander_km` had to de-trend
/// by least squares because an across-range axis up to 15 degrees off the true crest puts a
/// LINEAR trend in the offsets -- and its first version read 26.3 km of wander on a crest
/// lying exactly along a straight bisector. A linear trend is a straight, tilted line, and a
/// straight tilted line has a path length exactly equal to its endpoint distance. Sinuosity
/// is 1.000 for it, with no de-trending and no fitted parameter. That is not a lucky
/// property; it is why this ratio was chosen over an RMS.
///
/// **The lateral deviation is reported against the endpoint CHORD, not against the offset-0
/// line**, for the same reason: offset 0 is where the quantised axis thinks the bisector is,
/// and the chord is where the crest itself actually starts and ends.
///
/// **What it is NOT immune to is a badly-off axis foreshortening the wiggle.** Walking a
/// wandering crest at an angle to it compresses the along-axis and can only make the
/// measured sinuosity SMALLER than the truth. The number is therefore a lower bound, which
/// is the safe direction for a task claiming a belt bends.
///
/// Returns, in order: crest sinuosity; the crest's maximum lateral deviation from its own
/// endpoint chord, in km; that deviation as a fraction of the chord length; the two flanks'
/// envelope sinuosities; the count of unresolved stations; and the resolved belt length in
/// km, **which every other figure here is conditional on**.
fn sinuosity(
    surface: &Surface,
    lat: f64,
    lon: f64,
    across_axis: usize,
) -> (f64, f64, f64, f64, f64, usize, f64) {
    let along_deg = across_axis as f64 * 30.0 + 90.0; // cast-ok: a bearing index to float, exact
    let across_deg = across_axis as f64 * 30.0; // cast-ok: a bearing index to float, exact
    let (along_dlat, along_dlon) = stride(lat, along_deg, SINUOSITY_STEP_M);
    let (across_dlat, across_dlon) = stride(lat, across_deg, SINUOSITY_SCAN_STEP_M);

    let stations = (SINUOSITY_REACH_M / SINUOSITY_STEP_M) as i64; // cast-ok: an exact ratio of two constants
    let half_scan = (SINUOSITY_SCAN_M / SINUOSITY_SCAN_STEP_M) as i64; // cast-ok: an exact ratio of two constants
    let window = (CREST_TRACK_WINDOW_M / SINUOSITY_SCAN_STEP_M) as i64; // cast-ok: an exact ratio of two constants
    let base_reach = (ENVELOPE_BASE_REACH_M / SINUOSITY_SCAN_STEP_M) as i64; // cast-ok: an exact ratio of two constants
    let gap = (ENVELOPE_GAP_M / SINUOSITY_SCAN_STEP_M) as i64; // cast-ok: an exact ratio of two constants
    let step_km = SINUOSITY_SCAN_STEP_M / 1000.0;

    // One station's profile, indexed 0..=2*half_scan, with index `half_scan` on the axis.
    let profile_at = |station: i64| -> Vec<f64> {
        let s = station as f64; // cast-ok: a station index to float, exact
        let (slat, slon) = (lat + along_dlat * s, lon + along_dlon * s);
        (-half_scan..=half_scan)
            .map(|offset| {
                let o = offset as f64; // cast-ok: an offset index to float, exact
                surface.elevation_m(
                    &SpherePoint::from_latlon(slat + across_dlat * o, slon + across_dlon * o),
                    None,
                )
            })
            .collect()
    };
    let width = 2 * half_scan + 1;

    // The highest sample within `radius` indices of `centre`, and whether it sat on the edge
    // of that window. `centre` and the return are indices into the profile above.
    let track = |samples: &[f64], centre: i64, radius: i64| -> (i64, bool) {
        let low = if centre - radius > 0 { centre - radius } else { 0 };
        let high = if centre + radius < width - 1 { centre + radius } else { width - 1 };
        let mut best = (f64::NEG_INFINITY, low);
        let mut index = low;
        while index <= high {
            let value = samples[index as usize]; // cast-ok: bounded by `low`/`high` above
            if value > best.0 {
                best = (value, index);
            }
            index += 1;
        }
        let at_edge = best.1 == centre - radius || best.1 == centre + radius;
        (best.1, at_edge)
    };

    // One station reduced to (crest index, lower edge index, upper edge index).
    let measure = |samples: &[f64], crest_index: i64| -> (i64, i64) {
        let low = if crest_index - base_reach > 0 { crest_index - base_reach } else { 0 };
        let high =
            if crest_index + base_reach < width - 1 { crest_index + base_reach } else { width - 1 };
        let mut base = f64::INFINITY;
        let mut index = low;
        while index <= high {
            let value = samples[index as usize]; // cast-ok: bounded by `low`/`high` above
            if value < base {
                base = value;
            }
            index += 1;
        }
        let crest = samples[crest_index as usize]; // cast-ok: an index into the profile it came from
        let contour = base + ENVELOPE_FRACTION * (crest - base);

        // Outward on each flank, keeping the furthest offset still above the contour and
        // stepping over notches narrower than `ENVELOPE_GAP_M`.
        let mut lower = crest_index;
        let mut below = 0i64;
        let mut index = crest_index;
        while index > 0 {
            index -= 1;
            if samples[index as usize] >= contour {
                // cast-ok: bounded by the loop
                lower = index;
                below = 0;
            } else {
                below += 1;
                if below > gap {
                    break;
                }
            }
        }
        let mut upper = crest_index;
        let mut below = 0i64;
        let mut index = crest_index;
        while index < width - 1 {
            index += 1;
            if samples[index as usize] >= contour {
                // cast-ok: bounded by the loop
                upper = index;
                below = 0;
            } else {
                below += 1;
                if below > gap {
                    break;
                }
            }
        }
        (lower, upper)
    };

    let to_km = |index: i64| -> f64 {
        (index - half_scan) as f64 * step_km // cast-ok: a sample index to float, exact
    };

    // The seed: the peak's own station, found over the WHOLE scan because the peak is on the
    // belt by construction and nothing has to be followed to get there.
    let seed_samples = profile_at(0);
    let (seed_index, _) = track(&seed_samples, half_scan, half_scan);
    let mut unresolved = 0usize;
    let (seed_lower, seed_upper) = measure(&seed_samples, seed_index);

    // Outward in each direction from the seed, so the tracker never has to cross the peak
    // and never inherits a drift from the far end of the belt.
    let mut crest = vec![to_km(seed_index)];
    let mut lower_edge = vec![to_km(seed_lower)];
    let mut upper_edge = vec![to_km(seed_upper)];
    let mut back_crest: Vec<f64> = Vec::new();
    let mut back_lower: Vec<f64> = Vec::new();
    let mut back_upper: Vec<f64> = Vec::new();

    for direction in [1i64, -1] {
        let mut previous = seed_index;
        for step in 1..=stations {
            let samples = profile_at(direction * step);
            let (index, at_edge) = track(&samples, previous, window);
            // The belt has ended on this side. Stop rather than tracking whatever is beyond
            // it -- see `BELT_END_M`, which is the whole reason this branch exists.
            if samples[index as usize] <= BELT_END_M {
                // cast-ok: an index into the profile it came from
                break;
            }
            if at_edge {
                unresolved += 1;
            }
            let (lower, upper) = measure(&samples, index);
            if direction > 0 {
                crest.push(to_km(index));
                lower_edge.push(to_km(lower));
                upper_edge.push(to_km(upper));
            } else {
                back_crest.push(to_km(index));
                back_lower.push(to_km(lower));
                back_upper.push(to_km(upper));
            }
            previous = index;
        }
    }

    // The backward half runs from the seed outward, so reverse it and put the forward half
    // after it: one series ordered from -reach to +reach.
    back_crest.reverse();
    back_lower.reverse();
    back_upper.reverse();
    back_crest.extend(crest);
    back_lower.extend(lower_edge);
    back_upper.extend(upper_edge);

    let belt_km = SINUOSITY_STEP_M / 1000.0 * (back_crest.len() - 1) as f64; // cast-ok: a station count to float, exact
    let (crest_sinuosity, deviation_km, deviation_fraction) = sinuosity_of(&back_crest);
    let (lower_sinuosity, _, _) = sinuosity_of(&back_lower);
    let (upper_sinuosity, _, _) = sinuosity_of(&back_upper);
    (
        crest_sinuosity,
        deviation_km,
        deviation_fraction,
        lower_sinuosity,
        upper_sinuosity,
        unresolved,
        belt_km,
    )
}

/// One offset series reduced to a sinuosity, a maximum lateral deviation in km, and that
/// deviation as a fraction of the endpoint chord.
///
/// Kept separate from [`sinuosity`] so the crest series and the two envelope series are
/// reduced by the SAME code rather than by three copies of it -- the crest number and the
/// envelope number are about to be compared against each other, and a difference between
/// them has to be a difference in the belt, not in two reductions that drifted.
///
/// Planar in the local (along, across) frame. At 500 km on a 4,500 km planet the along-axis
/// is a 6.4-degree arc, and the error in treating it as flat is well below the 2 km scan
/// resolution the offsets are quantised to anyway. Stated rather than assumed.
fn sinuosity_of(offsets_km: &[f64]) -> (f64, f64, f64) {
    if offsets_km.len() < 2 {
        return (1.0, 0.0, 0.0);
    }
    let step_km = SINUOSITY_STEP_M / 1000.0;
    let mut path_km = 0.0f64;
    for index in 1..offsets_km.len() {
        let rise = offsets_km[index] - offsets_km[index - 1];
        path_km += libm::hypot(step_km, rise);
    }
    let along_km = step_km * (offsets_km.len() - 1) as f64; // cast-ok: a station count to float, exact
    let last = offsets_km[offsets_km.len() - 1];
    let first = offsets_km[0];
    let chord_km = libm::hypot(along_km, last - first);
    if !(chord_km > 0.0) {
        return (1.0, 0.0, 0.0);
    }

    // The largest perpendicular distance from the endpoint chord, by the standard
    // point-line formula written out rather than through a cross product, so the units stay
    // visible: the chord runs from (0, first) to (along_km, last).
    let (dx, dy) = (along_km, last - first);
    let mut deviation_km = 0.0f64;
    for (index, offset) in offsets_km.iter().enumerate() {
        let x = step_km * index as f64; // cast-ok: a station index to float, exact
        let y = offset - first;
        let distance = (dx * y - dy * x).abs() / chord_km;
        if distance > deviation_km {
            deviation_km = distance;
        }
    }
    (path_km / chord_km, deviation_km, deviation_km / chord_km)
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
    let (
        crest_sin,
        deviation_km,
        deviation_fraction,
        lower_sin,
        upper_sin,
        sin_unresolved,
        belt_km,
    ) = sinuosity(&surface, lat, lon, axis);
    println!(
        "{label:<40} peak {peak_m:8.1}  grade {grade:6.3}%  rel2km {relief_m:6.1}  \
         summits {summits:3}  2nd {runner_up_m:7.1}  crests {crests:2}  \
         wander {wander_max_km:5.1}/{wander_rms_km:5.1} km ({unresolved:2} unres)  \
         flanks {wide_m:6.0}/{narrow_m:6.0}  ratio {ratio:5.2}  reach {reach:7.0}  \
         SIN belt {belt_km:5.0} km crest {crest_sin:6.4} dev {deviation_km:6.1} km \
         ({deviation_fraction:5.3}) env {lower_sin:6.4}/{upper_sin:6.4} ({sin_unresolved:3} unres)"
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
    let surface = surface_with(params);
    let (_, lat, lon) = peak_of(&surface);
    hillshade_ppm_at(path, params, size, lat, lon, 1.0)
}

/// The same raster at a caller-chosen centre and box.
///
/// **A before/after pair of a belt that MOVES cannot be framed on the belt.** `hillshade_ppm`
/// centres on each configuration's own peak, which is right for showing what one setting
/// produces and wrong for showing that a setting moved something: the warp moves the peak,
/// so the two images end up being two different pieces of ground photographed by two
/// different cameras. Task 5's pair is framed on the UN-warped configuration's peak, both
/// times, so the belt is seen to leave.
fn hillshade_ppm_at(
    path: &str,
    params: TectonicParams,
    size: usize,
    lat: f64,
    lon: f64,
    zoom: f64,
) -> std::io::Result<()> {
    use std::io::Write;

    let surface = surface_with(params);
    let box_deg = RANGE_BOX_DEG * zoom;
    let span = 2.0 * box_deg;
    let step = span / size as f64; // cast-ok: a raster size to float, exact

    let mut heights = vec![0.0f64; size * size];
    for row in 0..size {
        let plat = lat + box_deg - row as f64 * step; // cast-ok: a raster index to float, exact
        for column in 0..size {
            let plon = lon - box_deg + column as f64 * step; // cast-ok: a raster index to float, exact
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
    println!("wrote {path} -- centre {lat:.2},{lon:.2}, +/-{box_deg} deg, {size}px");
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

/// One row measured at a CALLER-CHOSEN anchor rather than at this configuration's own peak.
///
/// **A before/after of a belt that moves cannot be framed on the belt.** `row` finds the
/// global maximum and measures there, which is right for "what does this configuration
/// deliver" and wrong for "did this configuration move the belt": the warp moves the peak,
/// so two rows end up measuring two different pieces of ground. Traced directly -- the
/// un-warped preset peaks at 19.250,48.550 and the same preset with 80 km of warp peaks at
/// 18.850,48.100, a different belt on a different bearing -- and the erratic grade and
/// summit columns that produced were an artefact of that, not a property of the warp.
///
/// So every warp row below is anchored on the UN-warped configuration's peak and axis, and
/// `here` is the elevation at that fixed point. It falls as the amplitude rises, which is
/// the most direct evidence in this file that the belt has translated off it.
fn sin_row(label: &str, params: TectonicParams, lat: f64, lon: f64, axis: usize) {
    let surface = surface_with(params);
    let (grade, slat, slon) = steepest_flank(&surface, lat, lon);
    let relief_m = relief_2km_m(&surface, slat, slon);
    let (summits, _) = summits_in_range(&surface, lat, lon);
    let here = surface.elevation_m(&SpherePoint::from_latlon(lat, lon), None);
    let (crest, deviation_km, fraction, lower, upper, unresolved, belt_km) =
        sinuosity(&surface, lat, lon, axis);
    println!(
        "{label:<36} here {here:7.1} m  grade {grade:6.3}%  rel2km {relief_m:7.1}  \
         summits {summits:3}  belt {belt_km:4.0} km  crest {crest:6.4}  \
         dev {deviation_km:5.1} km ({fraction:5.3})  env {lower:6.4}/{upper:6.4}  \
         ({unresolved:2} unres)  reach {:6.0}",
        params.collision_reach_m()
    );
}

/// **The largest single 100 m step on a transect across the belt.**
///
/// A cliff detector, and it is here because it found one. The first version of the
/// along-margin warp took its signed side from the ordered plate pair, the way
/// `pair_fraction` takes its suture offsets. That is stable across the margin it belongs to
/// and NOT across a third plate's boundary, where the whole `(near, *)` margin set is
/// replaced and the index comparison can come out the other way -- flipping the displacement
/// from `+w` to `-w` in one step. Measured here at **913.52 m over 100 m** on
/// `asymmetry 2.0 + warp`, against 11.58 m for the same configuration unwarped, and visible
/// in the raster as a hairline crack running the length of the bisector.
///
/// It is kept as a permanent row rather than deleted along with the bug, because **every
/// other column looked plausible while that cliff was present** -- the sinuosity numbers
/// this file exists to produce all went UP. Nothing else in this survey looks for a
/// discontinuity.
fn seam_probe(label: &str, params: TectonicParams) {
    let surface = surface_with(params);
    let (peak_m, lat, lon) = peak_of(&surface);
    let (_, _, _, axis) = flank_ratio(&surface, lat, lon, peak_m);
    let across_deg = axis as f64 * 30.0; // cast-ok: a bearing index to float, exact
    let (dlat, dlon) = stride(lat, across_deg, 100.0);
    let mut worst = (0.0f64, 0.0f64);
    let mut previous = surface
        .elevation_m(&SpherePoint::from_latlon(lat - dlat * 2500.0, lon - dlon * 2500.0), None);
    let mut index = -2499i64;
    while index <= 2500 {
        let f = index as f64; // cast-ok: a sample index to float, exact
        let here =
            surface.elevation_m(&SpherePoint::from_latlon(lat + dlat * f, lon + dlon * f), None);
        let jump = (here - previous).abs();
        if jump > worst.0 {
            worst = (jump, f * 0.1);
        }
        previous = here;
        index += 1;
    }
    println!(
        "{label:<36} largest 100 m step over +/-250 km: {:8.2} m at {:7.1} km",
        worst.0, worst.1
    );
}

/// **Technique 2, rebuilt on the axis it exists for, and measured on it.**
///
/// Task 2 warped the signed across-margin distance by an `fbm` of the 3-D POINT, measured
/// the crest moving 49.2 -> 78.8 km, and had it rejected for moving nothing else -- on four
/// metrics none of which can see a crest line's shape. Task 5 rebuilds it perturbing by a
/// function of position ALONG the margin only, so the whole belt translates coherently
/// rather than the edge being roughened, and judges it on [`sinuosity`].
///
/// **Measured in combination as well as alone, and that is deliberate.** Task 3 found three
/// of `ranges()`'s columns did not compose. The two bases disagree here too: the bare
/// envelope's crest sinuosity starts at 1.0261 and the preset's at 1.2786, because the
/// structure field already displaces the CREST as a side effect -- which is the argument
/// Task 2 rejected the warp on, and which is true of the crest and false of the BELT. The
/// envelope columns are where that distinction is visible.
fn warp_sweep() {
    let warped = |amplitude_km: f64, wavelength_km: f64, base: TectonicParams| TectonicParams {
        margin_warp_m: amplitude_km * 1000.0,
        margin_warp_wavelength_m: wavelength_km * 1000.0,
        ..base
    };

    // The bare steep envelope: a great circle with nothing else happening on it, which is
    // what makes it the calibration. A straight line must score 1.000, and this row is where
    // that is checked rather than claimed.
    let bare = steep();
    let surface = surface_with(bare);
    let (peak_m, lat, lon) = peak_of(&surface);
    let (_, _, _, axis) = flank_ratio(&surface, lat, lon, peak_m);
    println!(
        "\n--- Task 5: the along-margin warp on the BARE envelope -- THE CALIBRATION BASE.\n\
         Anchored on its un-warped peak, {peak_m:.1} m at {lat:.3},{lon:.3}, across axis {axis}"
    );
    sin_row("steep, no warp  <- CALIBRATION", bare, lat, lon, axis);
    for wavelength_km in [300.0f64, 600.0, 900.0] {
        for amplitude_km in [20.0f64, 40.0, 80.0, 120.0] {
            sin_row(
                &format!("steep + warp {amplitude_km:.0} km @ {wavelength_km:.0} km"),
                warped(amplitude_km, wavelength_km, bare),
                lat,
                lon,
                axis,
            );
        }
    }

    // The preset: the combination, on the un-warped preset's own peak, so the before and the
    // after are the same belt.
    let base = TectonicParams { margin_warp_m: 0.0, ..TectonicParams::ranges() };
    let surface = surface_with(base);
    let (peak_m, lat, lon) = peak_of(&surface);
    let (_, _, _, axis) = flank_ratio(&surface, lat, lon, peak_m);
    println!(
        "\n--- Task 5: the same warp IN COMBINATION, on the preset.\n\
         Anchored on the un-warped preset's peak, {peak_m:.1} m at {lat:.3},{lon:.3}, \
         across axis {axis}"
    );
    sin_row("preset with the warp switched off", base, lat, lon, axis);
    for amplitude_km in [20.0f64, 40.0, 80.0, 120.0, 160.0] {
        sin_row(
            &format!("preset + warp {amplitude_km:.0} km @ 300 km"),
            warped(amplitude_km, 300.0, base),
            lat,
            lon,
            axis,
        );
    }

    println!("\n--- Task 5: what the shipped preset DELIVERS, each row on its own peak");
    row("ranges() with the warp switched off", base);
    row("THE PRESET  ranges(), warp 80 @ 300 km", TectonicParams::ranges());
    row("  preset with warp 120 km", warped(120.0, 300.0, base));
    row("  preset with warp wavelength 900 km", warped(80.0, 900.0, base));

    println!("\n--- Task 5: THE CLIFF CHECK, which is how the first version was caught");
    for (label, params) in [
        ("steep", steep()),
        ("steep + warp 80 @ 300", warped(80.0, 300.0, steep())),
        ("asymmetry 2.0", TectonicParams { collision_asymmetry: 2.0, ..steep() }),
        (
            "asymmetry 2.0 + warp 80 @ 300",
            warped(80.0, 300.0, TectonicParams { collision_asymmetry: 2.0, ..steep() }),
        ),
        ("ranges() with the warp off", base),
        ("THE PRESET", TectonicParams::ranges()),
    ] {
        seam_probe(label, params);
    }
}

fn main() {
    // `images` means IMAGES ONLY. The tables and the rasters each cost a full pass over
    // several dozen planets, and a mode that silently did both would make anyone who wanted
    // one of them pay for the other -- which is how a survey stops being run.
    if std::env::args().any(|a| a == "images") {
        write_images().expect("the raster dump could not be written");
        return;
    }

    // `warp` means the Task 5 section only, for the same reason `images` means images only.
    if std::env::args().any(|a| a == "warp") {
        println!("world: seed {SEED}, radius {RADIUS_M} m, {PLATES} plates, land {LAND}");
        warp_sweep();
        return;
    }

    // `warpimages` renders the before/after pair the owner's complaint is about, BOTH framed
    // on the un-warped configuration's peak so the pictures show a belt that moved rather
    // than a camera that followed it.
    if std::env::args().any(|a| a == "warpimages") {
        let bare = steep();
        let bare_surface = surface_with(bare);
        let (_, bare_lat, bare_lon) = peak_of(&bare_surface);
        let base = TectonicParams { margin_warp_m: 0.0, ..TectonicParams::ranges() };
        let surface = surface_with(base);
        let (_, lat, lon) = peak_of(&surface);
        println!("bare framing {bare_lat:.3},{bare_lon:.3}; preset framing {lat:.3},{lon:.3}");
        for (name, params, plat, plon) in [
            ("task5-1-blade-straight", bare, bare_lat, bare_lon),
            (
                "task5-2-blade-warped",
                TectonicParams { margin_warp_m: 120_000.0, ..bare },
                bare_lat,
                bare_lon,
            ),
            ("task5-3-preset-straight", base, lat, lon),
            ("task5-4-preset-warped", TectonicParams::ranges(), lat, lon),
        ] {
            hillshade_ppm_at(
                &format!("target/mountain-survey/{name}.ppm"),
                params,
                900,
                plat,
                plon,
                2.0,
            )
            .expect("the raster dump could not be written");
        }
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

    // Technique 2 -- warping the signed across-margin distance -- was built here by Task 2,
    // swept, and REJECTED on four metrics none of which could see what it did. Task 5
    // rebuilt it perturbing by a function of position ALONG the margin and added the metric
    // that can: see `warp_sweep`, called at the end of this function.

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

    // ------------------------------------------------------------------ Task 3: the preset
    //
    // **The DELIVERED peak, which is the only calibration that means anything now.** Task 2
    // measured that `structure_depth` costs 22% of the peak and that a tight
    // `suture_spread_m` can add 64%, so `continent_collision_m` is a REQUEST and this row is
    // the ANSWER. The panel's height slider was calibrated 1,500-6,000 m against a smooth
    // envelope; this row is what says whether the preset lands inside that band.
    //
    // The three rows after it are the alternatives the preset was chosen over, measured on
    // the same population and host rather than argued about: the wavelength decision (40 km
    // buys summits and a grade no published orogen reaches), the depth decision, and the
    // asymmetry decision (1.67 is the published ratio; 2.0 is what MEASURES it).
    println!("\n--- Task 3: THE PRESET, and the alternatives it was chosen over");
    row("THE PRESET  TectonicParams::ranges()", TectonicParams::ranges());
    row(
        "  preset with wavelength 40 km",
        TectonicParams { structure_wavelength_m: 40_000.0, ..TectonicParams::ranges() },
    );
    row(
        "  preset with depth 0.9",
        TectonicParams { structure_depth: 0.9, ..TectonicParams::ranges() },
    );
    row(
        "  preset with asymmetry 1.67 (the published ratio)",
        TectonicParams { collision_asymmetry: 1.67, ..TectonicParams::ranges() },
    );
    row(
        "  preset with one suture (no stacking)",
        TectonicParams { suture_count: 1, suture_spread_m: 0.0, ..TectonicParams::ranges() },
    );

    warp_sweep();

    feedback_sweep();
}
