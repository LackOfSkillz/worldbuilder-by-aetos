//! Measures what a coastal roughening term does to a coastline, and chooses nothing.
//!
//! Task 5 of the photoreal slice
//! (`.superpowers/sdd/2026-09-05-slice-photoreal/task-5-brief.md`). `continentality.rs` now
//! carries an opt-in `CoastParams` block, proven bit-identical to today at `None` and at
//! `Some(CoastParams::canonical())`. This binary sweeps it and reports what moves. **It
//! changes no default and no `canonical()` value.**
//!
//! ```text
//! cargo run --release --bin coastline_survey
//! ```
//!
//! This is a `[[bin]]`, not a `cargo test`-visible fixture, for the reason
//! `relief_survey.rs` and `pond_threshold_survey.rs` already are: a few tens of millions of
//! field evaluations across three worlds and eleven amplitudes is a one-machine measurement
//! question, not a property a suite that runs on every push should re-pay. It contributes
//! **0 tests** to every one of the five feature configurations, exactly as
//! `mountain_survey.rs` does, and it compiles under all of them plus
//! `wasm32-unknown-unknown` -- there is nothing target-specific in it (no `std::time`, no
//! threads, no file IO).
//!
//! # Method -- every figure below names its population, its method with parameters, and its host
//!
//! - **Host:** this developer machine, `cargo run --release`, single-threaded.
//! - **Worlds.** Three, each built through `Surface::with_coast(seed, radius_m, plates,
//!   land_fraction, None, None, None, coast)` so the coastal block reaches `Continentality`,
//!   `Tectonics` and `Shelf` alike:
//!   - `owner`: seed **562423712**, radius **4,500,000 m**, **28** plates, land **0.16** --
//!     the owner's own world, the one the gap analysis was written against.
//!   - `earth-a`: seed **20260904**, `EARTH_RADIUS_M`, `DEFAULT_PLATE_COUNT`,
//!     `LAND_FRACTION` (0.29) -- the seed `relief_survey.rs` measured against.
//!   - `earth-b`: seed **12345**, same radius/plates/land fraction -- the seed
//!     `continentality.rs`'s own calibration test is pinned on.
//! - **Land fraction, two of them, and they are different questions.**
//!   - *field land fraction*: the share of a **200,000-point Fibonacci spiral** (the same
//!     area-uniform construction `calibrate` uses, at fifty times the sample count) whose
//!     `Continentality::above_shore` is `> 0.0`. This is the control the technique is
//!     judged by: it is what `land_fraction` is supposed to mean. Binomial standard error
//!     at `p = 0.16, n = 200,000` is `sqrt(0.16*0.84/200000) = 8.2e-4`, i.e. **+-0.082 pp**
//!     at 1 sigma -- an order below the +-0.58 pp the 4,000-sample calibrator itself carries,
//!     so this estimator can see a shift the calibrator cannot.
//!   - *world land fraction*: the same spiral, `Surface::structural_m(point) > 0.0`. This
//!     is what a viewer draws as land, after tectonics and the shelf have had their say.
//! - **Coastline length.** An equirectangular lat/lon grid whose meridional spacing is the
//!   stated `spacing_m`: `rows = round(PI * radius_m / spacing_m)` latitudes at row centres
//!   (`lat = -PI/2 + (row + 0.5) * PI / rows`), `cols = 2 * rows` longitudes. Each sample is
//!   classified land or sea by the stated predicate. Length is a **Cauchy-Crofton
//!   boundary-edge sum**: every east-west neighbour pair (with longitude wrapping) whose
//!   classification differs contributes the meridional sample spacing `PI * radius_m / rows`
//!   -- the length of the north-south boundary segment separating them -- and every
//!   north-south pair whose classification differs contributes the zonal spacing at the
//!   midpoint latitude, `2 * PI * radius_m * cos(lat_mid) / cols`. The estimator is biased
//!   HIGH against a smooth curve by the usual raster factor (~4/PI); every figure reported
//!   from it is a **ratio between two configurations measured on the identical grid**, where
//!   that bias divides out.
//! - **The fractal signature is the ratio's behaviour ACROSS spacings, not its value at
//!   one.** A measure that reports a longer coast for any added noise is not measuring
//!   fractality, so every length is reported at **four spacings** and beside a **deliberately
//!   smooth control** -- `CoastParams { frequency: 2.0, octaves: 1, .. }`, one octave BELOW
//!   the four the base field already has, at the identical amplitude. The control moves the
//!   coastline just as far as the fractal term does; it adds no structure finer than the
//!   field already had. **If the metric cannot tell those two apart, it is not measuring
//!   what this task exists to produce**, and the run says so in as many words.
//! - **Islands and inland water.** Connected components of the same grid, 4-connectivity,
//!   longitude wrapping, poles not joined (the row-centre construction has no pole sample).
//!   Cell area is `radius_m^2 * dlat * dlon * cos(lat)`. *Islands* are land components at or
//!   above the stated area threshold. *Inland water* is every water component except the
//!   largest -- the ocean -- at or above the same threshold. *Inlet heads* are water samples
//!   with three or more land neighbours out of four, grouped into components: a bay, a fjord
//!   or a strait head one sample wide. A count at spacing `d` cannot see a feature below
//!   about one cell, so counts are only ever compared **at the same spacing**.
//! - **Largest-land share** is the largest land component's area over all land area. It is
//!   how "are these still recognisable landmasses" is asked as a number: a continent that
//!   has been shot into speckle has a high island count and a collapsed share.

use worldbuilder_engine::continentality::{CoastParams, LAND_FRACTION};
use worldbuilder_engine::detmath as m;
use worldbuilder_engine::generation::DEFAULT_PLATE_COUNT;
use worldbuilder_engine::sphere::{SpherePoint, EARTH_RADIUS_M};
use worldbuilder_engine::surface::Surface;
use worldbuilder_engine::vectors::Vec3;

/// The owner's world, verbatim from the slice ledger.
const OWNER_SEED: i64 = 562_423_712;
const OWNER_RADIUS_M: f64 = 4_500_000.0;
const OWNER_PLATES: usize = 28;
const OWNER_LAND: f64 = 0.16;

/// Sample count for the land-fraction spiral. See the module note for the standard error
/// this buys.
const SPIRAL_SAMPLES: usize = 200_000;

/// Grid spacings, in metres, at which every length is reported. Halving each time is what
/// makes the ratio's trend readable as a ruler-length experiment.
const SPACINGS_M: [f64; 4] = [100_000.0, 50_000.0, 25_000.0, 12_500.0];

/// The spacing the component counts and the amplitude sweep are taken at. One spacing, held
/// fixed, because a count is only comparable to another count at the same resolution.
const COMPONENT_SPACING_M: f64 = 25_000.0;

/// Area thresholds for the component counts, in square kilometres. The smaller is about
/// forty cells at `COMPONENT_SPACING_M` on the owner's world; the larger is Sicily-sized.
const SMALL_AREA_KM2: f64 = 25_000.0;
const LARGE_AREA_KM2: f64 = 100_000.0;

/// The amplitudes swept. Zero is the control and must reproduce the canonical world exactly.
const AMPLITUDES: [f64; 11] = [0.0, 0.05, 0.1, 0.15, 0.2, 0.3, 0.35, 0.5, 0.75, 1.0, 1.5];

#[derive(Clone, Copy)]
struct World {
    name: &'static str,
    seed: i64,
    radius_m: f64,
    plates: usize,
    land_fraction: f64,
}

const WORLDS: [World; 3] = [
    World {
        name: "owner",
        seed: OWNER_SEED,
        radius_m: OWNER_RADIUS_M,
        plates: OWNER_PLATES,
        land_fraction: OWNER_LAND,
    },
    World {
        name: "earth-a",
        seed: 20_260_904,
        radius_m: EARTH_RADIUS_M,
        plates: DEFAULT_PLATE_COUNT,
        land_fraction: LAND_FRACTION,
    },
    World {
        name: "earth-b",
        seed: 12_345,
        radius_m: EARTH_RADIUS_M,
        plates: DEFAULT_PLATE_COUNT,
        land_fraction: LAND_FRACTION,
    },
];

fn build(world: &World, coast: Option<CoastParams>) -> Surface {
    Surface::with_coast(
        world.seed,
        world.radius_m,
        world.plates,
        world.land_fraction,
        None,
        None,
        None,
        coast,
    )
}

/// The smooth control: the same amplitude, moved by a term COARSER than the field's own
/// finest octave, so it displaces the coastline without adding any structure to it.
fn smooth_control(amplitude: f64) -> CoastParams {
    CoastParams { amplitude, frequency: 2.0, octaves: 1, ..CoastParams::canonical() }
}

/// One area-uniform Fibonacci-spiral point, built exactly as `calibrate` builds its own.
fn spiral_point(index: usize, count: usize) -> SpherePoint {
    let golden = core::f64::consts::PI * (3.0 - m::sqrt(5.0));
    let n = count as f64; // cast-ok: sample count to float, exact far below 2^53
    let i = index as f64; // cast-ok: loop counter to float, exact far below 2^53
    let z = 1.0 - 2.0 * (i + 0.5) / n;
    let inner = 1.0 - z * z;
    let ring = m::sqrt(if inner > 0.0 { inner } else { 0.0 });
    let angle = golden * i;
    SpherePoint { vector: Vec3::new(m::cos(angle) * ring, m::sin(angle) * ring, z) }
}

/// The share of the spiral that a predicate calls land.
fn spiral_fraction(predicate: &dyn Fn(&SpherePoint) -> bool) -> f64 {
    let mut land = 0usize;
    for index in 0..SPIRAL_SAMPLES {
        if predicate(&spiral_point(index, SPIRAL_SAMPLES)) {
            land += 1;
        }
    }
    let hits = land as f64; // cast-ok: count to float, exact far below 2^53
    let total = SPIRAL_SAMPLES as f64; // cast-ok: count to float, exact far below 2^53
    hits / total
}

/// A land/sea raster at one spacing, plus the grid geometry needed to measure it.
struct Raster {
    rows: usize,
    cols: usize,
    /// Meridional sample spacing, metres.
    dy_m: f64,
    /// Zonal sample spacing at each row's latitude, metres.
    dx_m: Vec<f64>,
    /// Cell area at each row's latitude, square metres.
    area_m2: Vec<f64>,
    land: Vec<bool>,
}

impl Raster {
    fn build(radius_m: f64, spacing_m: f64, predicate: &dyn Fn(&SpherePoint) -> bool) -> Self {
        let pi = core::f64::consts::PI;
        // `round` is banned outside detmath and this is an integer count, not a floor of a
        // physical quantity -- add a half and truncate, which is the same thing for a
        // positive value.
        let rows_f = pi * radius_m / spacing_m + 0.5;
        let mut rows = rows_f as usize; // cast-ok: truncation of a positive count, the +0.5 above makes it a round
        if rows < 4 {
            rows = 4;
        }
        let cols = rows * 2;
        let rows_float = rows as f64; // cast-ok: count to float, exact far below 2^53
        let cols_float = cols as f64; // cast-ok: count to float, exact far below 2^53
        let dlat = pi / rows_float;
        let dlon = 2.0 * pi / cols_float;
        let dy_m = radius_m * dlat;

        let mut dx_m = Vec::with_capacity(rows);
        let mut area_m2 = Vec::with_capacity(rows);
        let mut land = Vec::with_capacity(rows * cols);
        for row in 0..rows {
            let row_float = row as f64; // cast-ok: loop counter to float, exact far below 2^53
            let lat = -pi / 2.0 + (row_float + 0.5) * dlat;
            let cos_lat = m::cos(lat);
            dx_m.push(radius_m * dlon * cos_lat);
            area_m2.push(radius_m * radius_m * dlat * dlon * cos_lat);
            let sin_lat = m::sin(lat);
            for col in 0..cols {
                let col_float = col as f64; // cast-ok: loop counter to float, exact far below 2^53
                let lon = -pi + (col_float + 0.5) * dlon;
                let point = SpherePoint {
                    vector: Vec3::new(m::cos(lon) * cos_lat, m::sin(lon) * cos_lat, sin_lat),
                };
                land.push(predicate(&point));
            }
        }
        Self { rows, cols, dy_m, dx_m, area_m2, land }
    }

    fn at(&self, row: usize, col: usize) -> bool {
        self.land[row * self.cols + col]
    }

    /// Cauchy-Crofton boundary-edge sum. See the module note.
    fn coast_length_m(&self) -> f64 {
        let mut total = 0.0;
        for row in 0..self.rows {
            for col in 0..self.cols {
                let here = self.at(row, col);
                // East neighbour, wrapping: the boundary between them runs north-south, so
                // it is the MERIDIONAL spacing long.
                let east = if col + 1 == self.cols { 0 } else { col + 1 };
                if here != self.at(row, east) {
                    total += self.dy_m;
                }
                // North neighbour: the boundary runs east-west, so it is the ZONAL spacing
                // long. Taken at the northern row's latitude rather than an interpolated
                // midpoint -- the difference is second order in `dlat` and this is a ratio.
                if row + 1 < self.rows && here != self.at(row + 1, col) {
                    total += self.dx_m[row + 1];
                }
            }
        }
        total
    }

    /// Water samples with three or more land neighbours out of four: a bay, fjord or strait
    /// head one sample wide. Returned as a component count, so a two-cell notch is one
    /// inlet rather than two.
    fn inlet_heads(&self) -> usize {
        let mut notch = vec![false; self.rows * self.cols];
        for row in 0..self.rows {
            for col in 0..self.cols {
                if self.at(row, col) {
                    continue;
                }
                let east = if col + 1 == self.cols { 0 } else { col + 1 };
                let west = if col == 0 { self.cols - 1 } else { col - 1 };
                let mut neighbours = 0usize;
                if self.at(row, east) {
                    neighbours += 1;
                }
                if self.at(row, west) {
                    neighbours += 1;
                }
                if row + 1 < self.rows && self.at(row + 1, col) {
                    neighbours += 1;
                }
                if row > 0 && self.at(row - 1, col) {
                    neighbours += 1;
                }
                if neighbours >= 3 {
                    notch[row * self.cols + col] = true;
                }
            }
        }
        components(&notch, self.rows, self.cols, &vec![1.0; self.rows]).len()
    }

    /// Every land component's area, largest first.
    fn island_areas_m2(&self) -> Vec<f64> {
        let mut areas = components(&self.land, self.rows, self.cols, &self.area_m2);
        sort_desc(&mut areas);
        areas
    }

    /// Every water component's area, largest first. The first is the ocean.
    fn water_areas_m2(&self) -> Vec<f64> {
        let water: Vec<bool> = self.land.iter().map(|l| !*l).collect();
        let mut areas = components(&water, self.rows, self.cols, &self.area_m2);
        sort_desc(&mut areas);
        areas
    }
}

fn sort_desc(values: &mut [f64]) {
    values.sort_by(|a, b| b.partial_cmp(a).expect("areas are finite"));
}

/// Union-find connected components of `mask` over a 4-connected, longitude-wrapping grid.
/// Returns one area per component, in no particular order, `row_weight[row]` being the area
/// contributed by one cell in that row.
fn components(mask: &[bool], rows: usize, cols: usize, row_weight: &[f64]) -> Vec<f64> {
    let mut parent: Vec<usize> = (0..rows * cols).collect();

    fn find(parent: &mut Vec<usize>, mut node: usize) -> usize {
        while parent[node] != node {
            parent[node] = parent[parent[node]];
            node = parent[node];
        }
        node
    }

    for row in 0..rows {
        for col in 0..cols {
            let here = row * cols + col;
            if !mask[here] {
                continue;
            }
            let east_col = if col + 1 == cols { 0 } else { col + 1 };
            let east = row * cols + east_col;
            if east != here && mask[east] {
                let (a, b) = (find(&mut parent, here), find(&mut parent, east));
                if a != b {
                    parent[a] = b;
                }
            }
            if row + 1 < rows {
                let north = (row + 1) * cols + col;
                if mask[north] {
                    let (a, b) = (find(&mut parent, here), find(&mut parent, north));
                    if a != b {
                        parent[a] = b;
                    }
                }
            }
        }
    }

    let mut area = vec![0.0f64; rows * cols];
    for row in 0..rows {
        for col in 0..cols {
            let here = row * cols + col;
            if mask[here] {
                let root = find(&mut parent, here);
                area[root] += row_weight[row];
            }
        }
    }
    let mut out = Vec::new();
    for (index, value) in area.iter().enumerate() {
        if *value > 0.0 && find(&mut parent, index) == index {
            out.push(*value);
        }
    }
    out
}

fn count_at_least(areas: &[f64], threshold_km2: f64) -> usize {
    let threshold_m2 = threshold_km2 * 1e6;
    areas.iter().filter(|a| **a >= threshold_m2).count()
}

fn main() {
    println!("# coastline_survey -- Task 5, photoreal slice");
    println!("# host: cargo run --release, single-threaded, this developer machine");
    println!(
        "# spiral: {SPIRAL_SAMPLES} area-uniform Fibonacci points; grid: equirectangular, \
         rows = round(PI*R/spacing), cols = 2*rows"
    );
    println!("# fractal preset under test: {:?}", CoastParams::fractal());
    println!();

    // ---- 1. Land fraction, the control -------------------------------------------------
    println!("## 1. Land fraction before and after, three worlds");
    println!("#    field  = share of the spiral with Continentality::above_shore > 0");
    println!("#    world  = share of the spiral with Surface::structural_m > 0");
    println!("#    1 sigma binomial SE at n=200,000 is +-0.08 pp at p=0.16, +-0.10 pp at p=0.29");
    println!(
        "{:<9} {:>6} {:>10} {:>10} {:>9} {:>10} {:>10} {:>9}",
        "world", "asked", "field_off", "field_on", "delta_pp", "world_off", "world_on", "delta_pp"
    );
    for world in WORLDS.iter() {
        let off = build(world, None);
        let on = build(world, Some(CoastParams::fractal()));
        let field_off = spiral_fraction(&|p| off.land.above_shore(p) > 0.0);
        let field_on = spiral_fraction(&|p| on.land.above_shore(p) > 0.0);
        let world_off = spiral_fraction(&|p| off.structural_m(p) > 0.0);
        let world_on = spiral_fraction(&|p| on.structural_m(p) > 0.0);
        println!(
            "{:<9} {:>6.3} {:>10.5} {:>10.5} {:>+9.3} {:>10.5} {:>10.5} {:>+9.3}",
            world.name,
            world.land_fraction,
            field_off,
            field_on,
            (field_on - field_off) * 100.0,
            world_off,
            world_on,
            (world_on - world_off) * 100.0,
        );
    }
    println!();

    // ---- 2. Coastline length against spacing -------------------------------------------
    println!("## 2. Coastline length, and whether the ratio grows as the ruler shortens");
    println!("#    predicate: Continentality::above_shore > 0 (the field this task changes)");
    println!("#    smooth control: same amplitude at frequency 2.0, one octave -- COARSER");
    println!("#    than the base field's own finest octave, so it moves the coast without");
    println!("#    adding structure to it. A metric that cannot separate these two columns");
    println!("#    is not measuring fractality.");
    println!(
        "{:<9} {:>10} {:>7} {:>14} {:>14} {:>10} {:>14} {:>10}",
        "world", "spacing_km", "rows", "canon_len_km", "fract_len_km", "fract/can", "ctrl_len_km", "ctrl/can"
    );
    for world in WORLDS.iter() {
        let off = build(world, None);
        let on = build(world, Some(CoastParams::fractal()));
        let control = build(world, Some(smooth_control(CoastParams::fractal().amplitude)));
        for spacing_m in SPACINGS_M.iter() {
            let canon = Raster::build(world.radius_m, *spacing_m, &|p| off.land.above_shore(p) > 0.0);
            let fract = Raster::build(world.radius_m, *spacing_m, &|p| on.land.above_shore(p) > 0.0);
            let ctrl =
                Raster::build(world.radius_m, *spacing_m, &|p| control.land.above_shore(p) > 0.0);
            let canon_len = canon.coast_length_m();
            let fract_len = fract.coast_length_m();
            let ctrl_len = ctrl.coast_length_m();
            println!(
                "{:<9} {:>10.1} {:>7} {:>14.0} {:>14.0} {:>10.3} {:>14.0} {:>10.3}",
                world.name,
                spacing_m / 1000.0,
                canon.rows,
                canon_len / 1000.0,
                fract_len / 1000.0,
                fract_len / canon_len,
                ctrl_len / 1000.0,
                ctrl_len / canon_len,
            );
        }
    }
    println!();

    // ---- 3. The same, through the whole pipeline ---------------------------------------
    println!("## 3. The same length ratio, but on Surface::structural_m > 0 -- what a viewer draws");
    println!("#    owner's world only, two spacings: this is the check that the coastline the");
    println!("#    field grew survives tectonics and the shelf rather than being smoothed back out.");
    println!(
        "{:<9} {:>10} {:>14} {:>14} {:>10}",
        "world", "spacing_km", "canon_len_km", "fract_len_km", "fract/can"
    );
    {
        let world = WORLDS[0];
        let off = build(&world, None);
        let on = build(&world, Some(CoastParams::fractal()));
        for spacing_m in [50_000.0, 25_000.0].iter() {
            let canon = Raster::build(world.radius_m, *spacing_m, &|p| off.structural_m(p) > 0.0);
            let fract = Raster::build(world.radius_m, *spacing_m, &|p| on.structural_m(p) > 0.0);
            let canon_len = canon.coast_length_m();
            let fract_len = fract.coast_length_m();
            println!(
                "{:<9} {:>10.1} {:>14.0} {:>14.0} {:>10.3}",
                world.name,
                spacing_m / 1000.0,
                canon_len / 1000.0,
                fract_len / 1000.0,
                fract_len / canon_len,
            );
        }
    }
    println!();

    // ---- 4. The amplitude sweep --------------------------------------------------------
    println!("## 4. Amplitude travel: where it becomes visible, and where it destroys the land");
    println!(
        "#    owner's world, spacing {} km, predicate Continentality::above_shore > 0.",
        COMPONENT_SPACING_M / 1000.0
    );
    println!(
        "#    islands / inland: components at or above {SMALL_AREA_KM2:.0} and {LARGE_AREA_KM2:.0} km2."
    );
    println!("#    largest_share: the biggest landmass's share of all land area.");
    println!(
        "{:>5} {:>10} {:>9} {:>10} {:>9} {:>9} {:>9} {:>9} {:>9} {:>13}",
        "amp",
        "field_land",
        "len_ratio",
        "largest_%",
        "isl>=25k",
        "isl>=100k",
        "inland25k",
        "inlets",
        "ctrl_len",
        "ctrl_isl>=25k"
    );
    let world = WORLDS[0];
    let mut baseline_len = 0.0f64;
    for amplitude in AMPLITUDES.iter() {
        let coast = if *amplitude == 0.0 {
            None
        } else {
            Some(CoastParams { amplitude: *amplitude, ..CoastParams::canonical() })
        };
        let surface = build(&world, coast);
        let raster = Raster::build(world.radius_m, COMPONENT_SPACING_M, &|p| {
            surface.land.above_shore(p) > 0.0
        });
        let length = raster.coast_length_m();
        if *amplitude == 0.0 {
            baseline_len = length;
        }
        let islands = raster.island_areas_m2();
        let total_land: f64 = islands.iter().sum();
        let largest = if islands.is_empty() { 0.0 } else { islands[0] };
        let water = raster.water_areas_m2();
        let inland = if water.is_empty() { Vec::new() } else { water[1..].to_vec() };

        let control_surface = build(&world, Some(smooth_control(*amplitude)));
        let control_raster = Raster::build(world.radius_m, COMPONENT_SPACING_M, &|p| {
            control_surface.land.above_shore(p) > 0.0
        });
        let control_len = control_raster.coast_length_m();
        let control_islands = control_raster.island_areas_m2();

        let field_land = spiral_fraction(&|p| surface.land.above_shore(p) > 0.0);
        println!(
            "{:>5.2} {:>10.5} {:>9.3} {:>10.1} {:>9} {:>9} {:>9} {:>9} {:>9.3} {:>13}",
            amplitude,
            field_land,
            length / baseline_len,
            100.0 * largest / total_land,
            count_at_least(&islands, SMALL_AREA_KM2),
            count_at_least(&islands, LARGE_AREA_KM2),
            count_at_least(&inland, SMALL_AREA_KM2),
            raster.inlet_heads(),
            control_len / baseline_len,
            count_at_least(&control_islands, SMALL_AREA_KM2),
        );
    }
    println!();
    println!("# end. This binary chose nothing; the preset is chosen in the task report.");
}
