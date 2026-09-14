//! Measures what the seamount field makes of a world, and calibrates `density` against the
//! spec's band.
//!
//! Task 7 of the islands slice
//! (`.superpowers/sdd/2026-09-14-islands-1-peaks/task-7-brief.md`). Tasks 1 through 6 grew
//! `PeakParams` and `Tectonics::peak_offset_m`, wired the block through `Tectonics` and
//! `Surface`, pinned a wasm ABI for it and gave the studio a slider. Nothing had yet asked a
//! *world* how much of its surface the field turns into islands. This binary asks, sweeps
//! `density` and `lattice_m`, and prints the number the constants in `tectonics.rs` are then
//! set from.
//!
//! ```text
//! cargo run --release --bin island_survey
//! cargo run --release --bin island_survey -- components   # adds the raster pass (slow)
//! ```
//!
//! This is a `[[bin]]`, not a `cargo test`-visible fixture, for the reason
//! `coastline_survey.rs`, `relief_survey.rs` and `pond_threshold_survey.rs` already are: tens
//! of millions of field evaluations across three worlds and a two-dimensional sweep is a
//! one-machine measurement question, not a property a suite that runs on every push should
//! re-pay. It contributes **0 tests** to every one of the five feature configurations, exactly
//! as `coastline_survey.rs` does, and there is nothing target-specific in it beyond the
//! optional `std::env::args` selector `hydro_survey.rs`, `mountain_survey.rs`,
//! `shore_probe.rs` and `streambench.rs` already use.
//!
//! # The trap this binary exists to avoid, stated before any method
//!
//! **Measure through the full `Surface` pipeline, over ocean. Never through `peak_offset_m`
//! against an assumed seabed.** Task 1's own sweep pinned `seabed_m` to `ABYSS_M` at every
//! probe, so `peak_depth_window` returned 1.0 unconditionally and the figure it reported --
//! 0.54% -- is what the field would make *if the whole planet were abyssal ocean, land
//! included*. It is a measurement of the FIELD, on a fiction. The real figure through
//! `Surface` on the same constants is **0.17%**, threefold lower, and the three factors that
//! account for the gap were themselves measured on that fixture: only 59.8% of the sphere is
//! ocean at all, only 67.5% of that ocean is deeper than the 2,500 m window threshold, and
//! only 20.6% of it actually reaches `ABYSS_M`.
//!
//! The volumetric model in `VOLCANIC_REACH_M`'s doc comment reproduced two separate
//! measurements to three significant figures and is still not a result: **it predicts the
//! FIELD, not a world's islanded share.** This binary uses it only to choose where to sample.
//! Every number printed below is read off a live world.
//!
//! # Method -- every figure names its population, its method with parameters, and its host
//!
//! - **Host:** this developer machine, `cargo run --release`, single-threaded. The report
//!   records the host and the rustc version; this file does not restate them, because a
//!   figure transcribed into a comment is a figure nobody re-runs.
//! - **Worlds.** Three, each built twice -- once through `Surface::new` (no peak block at all)
//!   and once through `Surface::with_peaks(.., Some(peaks))`, every other argument identical,
//!   so the difference between the two IS the block:
//!   - `island-a`: seed **9001**, `EARTH_RADIUS_M`, **22** plates, land **0.4**. This is the
//!     fixture `surface.rs`'s own peak tests use (`ISLAND_SEED` / `ISLAND_PLATES` /
//!     `ISLAND_LAND`), so the share printed here is directly comparable with the count
//!     `an_island_stands_above_the_datum_in_open_ocean` pins. It is the world the sweep runs
//!     on and the constants are chosen from.
//!   - `owner`: seed **562423712**, radius **4,500,000 m**, **28** plates, land **0.16** --
//!     the owner's own world, the one `coastline_survey.rs` also measures.
//!   - `earth-a`: seed **20260904**, `EARTH_RADIUS_M`, `DEFAULT_PLATE_COUNT`, `LAND_FRACTION`
//!     (0.29) -- the parity corpus's own seed.
//!   The chosen constants are confirmed on all three; the sweep itself runs on `island-a`
//!   only, because a sweep averaged across three different land fractions would hide the
//!   very dependence the report has to state.
//! - **The point set.** A **Fibonacci spiral**, `z = 1 - (2i + 1) / n`, which is the identical
//!   area-uniform construction `Continentality::calibrate` and `surface.rs`'s own
//!   `fibonacci_point` use. Two sizes are reported for every configuration and they answer
//!   different questions: **n = 20,000** because that is the population spec §1's claim and
//!   the pinned test are both stated over, and **n = 200,000** because that is the one the
//!   calibration decision is made on. Binomial standard error at `p = 0.005, n = 200,000` is
//!   `sqrt(0.005 * 0.995 / 200000)` = 1.58e-4, i.e. **+-0.0158 pp** at 1 sigma -- an order
//!   below the 0.5 pp band the spec asks for, so this estimator can see the band's edges. At
//!   n = 20,000 the same error is **+-0.05 pp**, which is why the decision is not made there.
//! - **Two discriminators, and they are different questions.** Both are printed everywhere.
//!   - *added land* (`D_added`): `peaked.structural_m > 0` where `plain.structural_m <= 0`.
//!     This is exactly "surface the peak block turned into land", it needs no threshold, and
//!     it is the one the calibration is done on. At an inert block it is **zero by
//!     construction** -- the two surfaces are bit-identical -- which is why it cannot be the
//!     measurement spec §1's before-case is stated with.
//!   - *offshore land* (`D_offshore`): `structural_m > 0 && tectonics.offset_m > 2000`, the
//!     discriminator `an_island_stands_above_the_datum_in_open_ocean` asserts on. It is
//!     meaningful at an inert block (it is the island-arc question), and it is slightly
//!     LOOSER than `D_added` on a peaked world, because a continental point whose tectonic
//!     offset exceeds 2,000 m counts too.
//! - **The island-arc before-case.** At the canonical preset, over the same spiral, the
//!   largest `tectonics.offset_m` at any point the continent field calls sea -- spec §1 says
//!   this term is short of the 4,600 m abyss by about a factor of six. Measured, not
//!   transcribed.
//! - **Island count and area distribution.** An equirectangular lat/lon raster, built exactly
//!   as `coastline_survey.rs::Raster` builds its own: `rows = round(PI * radius_m /
//!   spacing_m)` latitudes at row centres, `cols = 2 * rows` longitudes, cell area
//!   `radius_m^2 * dlat * dlon * cos(lat)`. A cell is an island cell when `D_added` holds
//!   there. Components are 4-connected with longitude wrapping and the poles not joined (the
//!   row-centre construction has no pole sample), and the union-find runs over the island
//!   cells ALONE rather than the whole grid, which is what makes a 5 km raster affordable.
//!   **A count at spacing `d` cannot see an island below about one cell**, so the spacing is
//!   stated with every count and counts are only ever compared at the same spacing. This pass
//!   is behind the `components` argument because it is two orders of magnitude more field
//!   evaluations than the spiral.
//! - **Steep-to.** From an island's summit (the tallest point the spiral found on that world),
//!   `TangentFrame::at` and eight compass bearings, reporting the deepest and shallowest
//!   `structural_m` at each of several fractions of `reach_m`. The contrast case walks the
//!   same eight bearings out from a **continental shore** point -- the shallowest land point
//!   the spiral found on the plain world -- to `SHELF_BREAK_M` (80,000 m) and beyond, which is
//!   the distance a continental shelf runs before it breaks.
//! - **Substrate and detail at a peak.** `substrate::natural(elevation_m, slope, tectonic_m)`
//!   and `Detail::amplitude_m(point, elevation_m, shelf_weight, tectonic_m)` are called
//!   directly, on the arguments the elevation path itself hands them:
//!   `surface.shelf.evaluate(point)` supplies `weight` and `tectonic_m`, and
//!   `substrate::slope_at` supplies the slope at the same baseline `Surface` uses. The spec
//!   claims a large tectonic offset saturates `natural`'s `by_tectonics` term (100% rock) and
//!   saturates `amplitude_m`'s quieting; both are printed with the reference values
//!   (`ROCK_TECTONIC_M`, `quieting_scale_m`, `quieting_strength`) beside them so the reader
//!   can see how far past saturation the argument is, rather than being told that it is.
//! - **Requested against achieved land fraction.** `land_fraction` is what the caller asked
//!   for; the achieved figure is the share of the same spiral with `structural_m > 0`,
//!   reported at the canonical preset and at `volcanic()`. Spec §5 asks that `Surface` be able
//!   to report both numbers. **This slice measures them here and adds no API** -- see the
//!   report for why that is a decision rather than an omission.

use worldbuilder_engine::continentality::{LAND_FRACTION};
use worldbuilder_engine::detail::ReliefParams;
use worldbuilder_engine::detmath as m;
use worldbuilder_engine::generation::DEFAULT_PLATE_COUNT;
use worldbuilder_engine::shelf::SHELF_BREAK_M;
use worldbuilder_engine::sphere::{SpherePoint, EARTH_RADIUS_M};
use worldbuilder_engine::substrate::{self, ROCK_SLOPE, ROCK_TECTONIC_M};
use worldbuilder_engine::surface::Surface;
use worldbuilder_engine::tangent::TangentFrame;
use worldbuilder_engine::tectonics::PeakParams;
use worldbuilder_engine::vectors::Vec3;

/// The fixture `surface.rs`'s peak tests use, verbatim, so the share printed here and the
/// count that test pins are figures about the same planet.
const ISLAND_SEED: i64 = 9001;
const ISLAND_PLATES: usize = 22;
const ISLAND_LAND: f64 = 0.4;

/// The owner's world, verbatim from `coastline_survey.rs`.
const OWNER_SEED: i64 = 562_423_712;
const OWNER_RADIUS_M: f64 = 4_500_000.0;
const OWNER_PLATES: usize = 28;
const OWNER_LAND: f64 = 0.16;

/// The two spiral sizes, and they answer different questions -- see the module note.
const SPIRAL_SMALL: usize = 20_000;
const SPIRAL_LARGE: usize = 200_000;

/// The band spec §7 question 1 asks for, as a share of the planet's surface.
const BAND_LOW: f64 = 0.003;
const BAND_HIGH: f64 = 0.008;

/// The discriminator `an_island_stands_above_the_datum_in_open_ocean` uses, in metres of
/// tectonic offset. Named rather than inlined because it appears in three places here.
const OFFSHORE_OFFSET_M: f64 = 2_000.0;

/// The densities swept, at the shipped `reach_m / lattice_m`. Hundredths throughout, because
/// `viewer/public/app/peak-params.js`'s density slider carries an integer position and maps it
/// to a value by dividing by 100 -- a chosen density off that lattice is one the panel cannot
/// reach, which is the defect `panelFieldFaults()` exists for.
const DENSITIES: [f64; 16] = [
    0.11, 0.16, 0.20, 0.24, 0.28, 0.32, 0.36, 0.40, 0.45, 0.50, 0.55, 0.58, 0.60, 0.62, 0.70,
    0.75,
];

/// The `(lattice_m, reach_m)` pairs swept, in metres, each holding the shipped ratio of
/// **0.70** -- see the table this prints for why the ratio rather than either field alone is
/// the lever. `reach_m <= lattice_m` is a real bound `wasm.rs`'s `peak_is_admissible`
/// enforces, and 0.70 holds it at every entry.
///
/// **Both fields are written out rather than derived by multiplying.** `45_000.0 * 0.70` is
/// `31499.999999999996`, not `31500.0`, so a swept pair built that way would differ from the
/// shipped `VOLCANIC_REACH_M` in the last two bits -- a sweep that never actually visits the
/// configuration it is calibrating. The first run of this binary did exactly that, and printed
/// the wrong number in its own header as evidence.
const LATTICES_M: [(f64, f64); 4] =
    [(30_000.0, 21_000.0), (45_000.0, 31_500.0), (67_500.0, 47_250.0), (90_000.0, 63_000.0)];
/// The ratio every pair above holds, for the record only -- nothing multiplies by it.
const REACH_RATIO: f64 = 0.70;
/// The index of the shipped pair in [`LATTICES_M`], so the density sweep runs at the shipped
/// geometry rather than at whatever happens to be first.
const SHIPPED_LATTICE: usize = 1;

/// The candidate densities section 4 measures on **all three** worlds. The choice cannot be
/// made on one world: the islanded share is a share of the whole sphere, and a world with less
/// land has more deep ocean for the field to stand an island in, so the same density yields
/// very different shares. Section 4 prints all three and the choice is the density whose
/// WORST world is still inside the band.
const CANDIDATES: [f64; 11] =
    [0.28, 0.32, 0.33, 0.34, 0.35, 0.36, 0.37, 0.38, 0.40, 0.45, 0.58];

/// The raster spacing the component counts are taken at, in metres. An island at the shipped
/// constants stands about 25 km across, so 5 km puts about five cells across one and about
/// twenty in it -- enough to separate two that do not touch, and stated with every count
/// because a count at one spacing is not comparable with a count at another.
const COMPONENT_SPACING_M: f64 = 5_000.0;

/// Fractions of `reach_m` the steep-to walk samples.
const WALK_FRACTIONS: [f64; 6] = [0.2, 0.4, 0.6, 0.8, 0.95, 1.1];

/// Distances, in metres, the continental-shelf contrast walk samples. `SHELF_BREAK_M` is
/// 80,000, so the last two are past the break.
const SHELF_WALK_M: [f64; 6] = [10_000.0, 20_000.0, 40_000.0, 80_000.0, 120_000.0, 200_000.0];

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
        name: "island-a",
        seed: ISLAND_SEED,
        radius_m: EARTH_RADIUS_M,
        plates: ISLAND_PLATES,
        land_fraction: ISLAND_LAND,
    },
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
];

fn plain(world: &World) -> Surface {
    Surface::new(world.seed, world.radius_m, world.plates, world.land_fraction, None, None, None)
}

/// Every argument identical to `plain` above; the peak block is the only difference, exactly
/// as `surface.rs`'s `plain_surface` / `peaked_surface` pair is, or the difference between the
/// two would be a fact about these two helpers.
fn peaked(world: &World, peaks: PeakParams) -> Surface {
    Surface::with_peaks(
        world.seed,
        world.radius_m,
        world.plates,
        world.land_fraction,
        None,
        None,
        None,
        None,
        None,
        Some(peaks),
    )
}

/// The peak block at one point of the sweep: the shipped trio with `density` and the
/// `lattice_m` / `reach_m` pair replaced.
fn swept(density: f64, pair: (f64, f64)) -> PeakParams {
    PeakParams { density, lattice_m: pair.0, reach_m: pair.1, ..PeakParams::canonical() }
}

/// An area-uniform point, built exactly as `surface.rs`'s `fibonacci_point` and
/// `Continentality::calibrate` build theirs.
fn fibonacci_point(index: usize, count: usize) -> SpherePoint {
    let n = count as f64; // cast-ok: a survey's own probe count, exact far below 2^53
    let i = index as f64; // cast-ok: a loop counter below `count`, exact far below 2^53
    let z = 1.0 - (2.0 * i + 1.0) / n;
    let inner = 1.0 - z * z;
    let radius = m::sqrt(if inner > 0.0 { inner } else { 0.0 });
    let theta = i * core::f64::consts::PI * (3.0 - m::sqrt(5.0));
    SpherePoint { vector: Vec3::new(radius * m::cos(theta), radius * m::sin(theta), z) }
}

/// What one configuration did to one world, over one spiral.
struct Shares {
    count: usize,
    /// `D_added`: land the block made, where the plain world had sea.
    added: usize,
    /// `D_offshore`: the pinned test's own discriminator.
    offshore: usize,
    /// Every point with `structural_m > 0` on the peaked world.
    land: usize,
    /// The same on the plain world, so the requested/achieved pair can be printed together.
    plain_land: usize,
}

impl Shares {
    fn total(&self) -> f64 {
        self.count as f64 // cast-ok: a survey's own probe count, exact far below 2^53
    }
    fn added_share(&self) -> f64 {
        let hits = self.added as f64; // cast-ok: a count below `count`, exact far below 2^53
        hits / self.total()
    }
    fn offshore_share(&self) -> f64 {
        let hits = self.offshore as f64; // cast-ok: a count below `count`, exact far below 2^53
        hits / self.total()
    }
    fn land_share(&self) -> f64 {
        let hits = self.land as f64; // cast-ok: a count below `count`, exact far below 2^53
        hits / self.total()
    }
    fn plain_land_share(&self) -> f64 {
        let hits = self.plain_land as f64; // cast-ok: a count below `count`, exact far below 2^53
        hits / self.total()
    }
}

/// The plain world's answers over one spiral, cached because they do not depend on the peak
/// block and the sweep would otherwise recompute them at every point of it.
struct PlainSpiral {
    count: usize,
    structural_m: Vec<f64>,
    land: usize,
}

fn plain_spiral(surface: &Surface, count: usize) -> PlainSpiral {
    let mut structural_m = Vec::with_capacity(count);
    let mut land = 0usize;
    for i in 0..count {
        let point = fibonacci_point(i, count);
        let height = surface.structural_m(&point);
        if height > 0.0 {
            land += 1;
        }
        structural_m.push(height);
    }
    PlainSpiral { count, structural_m, land }
}

fn shares(peaked: &Surface, base: &PlainSpiral) -> Shares {
    let mut added = 0usize;
    let mut offshore = 0usize;
    let mut land = 0usize;
    for i in 0..base.count {
        let point = fibonacci_point(i, base.count);
        let height = peaked.structural_m(&point);
        if height > 0.0 {
            land += 1;
            if base.structural_m[i] <= 0.0 {
                added += 1;
            }
            if peaked.tectonics.offset_m(&point) > OFFSHORE_OFFSET_M {
                offshore += 1;
            }
        }
    }
    Shares { count: base.count, added, offshore, land, plain_land: base.land }
}

/// The largest tectonic offset at any point the continent field calls sea, and the largest
/// `structural_m` at any such point. Spec §1's before-case: the island-arc term is short of
/// the abyss by about a factor of six, and no open ocean rises above the datum.
fn arc_reach(surface: &Surface, count: usize) -> (f64, f64, usize, usize) {
    let mut tallest_offset = f64::NEG_INFINITY;
    let mut tallest_ground = f64::NEG_INFINITY;
    let mut oceanic = 0usize;
    let mut above_datum = 0usize;
    for i in 0..count {
        let point = fibonacci_point(i, count);
        if surface.land.above_shore(&point) > 0.0 {
            continue;
        }
        oceanic += 1;
        let offset = surface.tectonics.offset_m(&point);
        if offset > tallest_offset {
            tallest_offset = offset;
        }
        let ground = surface.structural_m(&point);
        if ground > tallest_ground {
            tallest_ground = ground;
        }
        if ground > 0.0 {
            above_datum += 1;
        }
    }
    (tallest_offset, tallest_ground, oceanic, above_datum)
}

/// The tallest point the block turned into land, by `D_added`.
fn find_a_summit(peaked: &Surface, base: &PlainSpiral) -> Option<(SpherePoint, f64)> {
    let mut best: Option<(SpherePoint, f64)> = None;
    for i in 0..base.count {
        if base.structural_m[i] > 0.0 {
            continue;
        }
        let point = fibonacci_point(i, base.count);
        let height = peaked.structural_m(&point);
        if height <= 0.0 {
            continue;
        }
        match &best {
            Some((_, tallest)) if *tallest >= height => {}
            _ => best = Some((point, height)),
        }
    }
    best
}

/// The lowest land point the plain world has -- a continental shore, for the shelf contrast.
fn find_a_shore(base: &PlainSpiral) -> Option<(SpherePoint, f64)> {
    let mut best: Option<(SpherePoint, f64)> = None;
    for i in 0..base.count {
        let height = base.structural_m[i];
        if height <= 0.0 {
            continue;
        }
        match &best {
            Some((_, lowest)) if *lowest <= height => {}
            _ => best = Some((fibonacci_point(i, base.count), height)),
        }
    }
    best
}

/// The deepest and shallowest `structural_m` on a ring of eight compass bearings at `walk_m`.
fn ring(surface: &Surface, centre: &SpherePoint, walk_m: f64) -> (f64, f64) {
    let frame = TangentFrame::at(centre, surface.radius_m);
    let mut deepest = f64::INFINITY;
    let mut shallowest = f64::NEG_INFINITY;
    for bearing in 0..8 {
        let angle = m::to_radians(45.0 * f64::from(bearing)); // cast-ok: a loop counter in 0..8
        let out = frame.local_to_sphere(walk_m * m::cos(angle), walk_m * m::sin(angle));
        let height = surface.structural_m(&out);
        if height < deepest {
            deepest = height;
        }
        if height > shallowest {
            shallowest = height;
        }
    }
    (deepest, shallowest)
}

/// Positive magnitude without `.abs()`, which `src/` bans -- see constraints.md.
fn magnitude(x: f64) -> f64 {
    if x < 0.0 {
        -x
    } else {
        x
    }
}

/// `smooth`, re-exported from `detail` at `shelf.rs:83`, is what both saturating terms below
/// are built on; the survey prints the argument and the reference so the reader can see how
/// far past 1.0 the argument is.
fn saturation(argument: f64) -> f64 {
    worldbuilder_engine::shelf::smooth(argument)
}

// --- The island raster: components, counts and an area distribution. ---

/// An island mask at one spacing, held SPARSELY -- only the island cells, because a 5 km
/// raster of a 6,371 km sphere is 32 million cells and the mask is about half a percent of
/// them. `cols` is the row length, `start[row]` indexes `cols_of` for that row, and
/// `cols_of[start[row]..start[row+1]]` is that row's island columns in increasing order, so a
/// neighbour test is a binary search rather than an array probe.
struct IslandMask {
    rows: usize,
    cols: usize,
    start: Vec<usize>,
    cols_of: Vec<u32>,
    /// Cell area at each row's latitude, square metres.
    area_m2: Vec<f64>,
    /// Island cells found, and cells sampled.
    found: usize,
    sampled: usize,
}

impl IslandMask {
    fn build(world: &World, peaked: &Surface, base: &Surface, spacing_m: f64) -> Self {
        let pi = core::f64::consts::PI;
        // `round` is banned outside detmath and this is an integer count, not a floor of a
        // physical quantity -- add a half and truncate, which is the same thing for a
        // positive value. `coastline_survey.rs::Raster::build` does exactly this.
        let rows_f = pi * world.radius_m / spacing_m + 0.5;
        let mut rows = rows_f as usize; // cast-ok: truncation of a positive count; the +0.5 above makes it a round
        if rows < 4 {
            rows = 4;
        }
        let cols = rows * 2;
        let rows_float = rows as f64; // cast-ok: a count to float, exact far below 2^53
        let cols_float = cols as f64; // cast-ok: a count to float, exact far below 2^53
        let dlat = pi / rows_float;
        let dlon = 2.0 * pi / cols_float;

        let mut area_m2 = Vec::with_capacity(rows);
        let mut start = Vec::with_capacity(rows + 1);
        let mut cols_of: Vec<u32> = Vec::new();
        let mut sampled = 0usize;
        for row in 0..rows {
            start.push(cols_of.len());
            let row_float = row as f64; // cast-ok: a loop counter to float, exact far below 2^53
            let lat = -pi / 2.0 + (row_float + 0.5) * dlat;
            let cos_lat = m::cos(lat);
            let sin_lat = m::sin(lat);
            area_m2.push(world.radius_m * world.radius_m * dlat * dlon * cos_lat);
            for col in 0..cols {
                let col_float = col as f64; // cast-ok: a loop counter to float, exact far below 2^53
                let lon = -pi + (col_float + 0.5) * dlon;
                let point = SpherePoint {
                    vector: Vec3::new(m::cos(lon) * cos_lat, m::sin(lon) * cos_lat, sin_lat),
                };
                sampled += 1;
                // The cheap half first: the peaked world is the one that can be land here,
                // and the plain call is only needed to decide whether it ALREADY was.
                if peaked.structural_m(&point) <= 0.0 {
                    continue;
                }
                if base.structural_m(&point) > 0.0 {
                    continue;
                }
                cols_of.push(col as u32); // cast-ok: a column index below `cols`, itself below 2^32 at every spacing this binary uses
            }
        }
        start.push(cols_of.len());
        let found = cols_of.len();
        Self { rows, cols, start, cols_of, area_m2, found, sampled }
    }

    /// The flat index of an island cell, or `None` if that cell is not one.
    fn index_of(&self, row: usize, col: usize) -> Option<usize> {
        let lo = self.start[row];
        let hi = self.start[row + 1];
        let needle = col as u32; // cast-ok: a column index below `cols`
        self.cols_of[lo..hi].binary_search(&needle).ok().map(|offset| lo + offset)
    }

    /// Which row a flat index belongs to. A linear scan over `start` would be O(rows) per
    /// call; this is the same binary search `index_of` uses, against the row offsets.
    fn row_of(&self, index: usize) -> usize {
        match self.start.binary_search(&index) {
            // `start` may repeat (an empty row), so a hit can land on the first of a run --
            // walk forward to the row that actually contains the index.
            Ok(mut row) => {
                while row + 1 < self.start.len() && self.start[row + 1] <= index {
                    row += 1;
                }
                row
            }
            Err(after) => after - 1,
        }
    }

    /// Every island component's area in square metres, largest first, 4-connected with
    /// longitude wrapping and the poles not joined.
    fn island_areas_m2(&self) -> Vec<f64> {
        let mut parent: Vec<usize> = (0..self.found).collect();

        fn find(parent: &mut Vec<usize>, mut node: usize) -> usize {
            while parent[node] != node {
                parent[node] = parent[parent[node]];
                node = parent[node];
            }
            node
        }
        fn union(parent: &mut Vec<usize>, a: usize, b: usize) {
            let ra = find(parent, a);
            let rb = find(parent, b);
            if ra != rb {
                parent[ra] = rb;
            }
        }

        for index in 0..self.found {
            let row = self.row_of(index);
            let col = self.cols_of[index] as usize; // cast-ok: a stored column index, widening
            let east = if col + 1 == self.cols { 0 } else { col + 1 };
            if let Some(other) = self.index_of(row, east) {
                union(&mut parent, index, other);
            }
            if row + 1 < self.rows {
                if let Some(other) = self.index_of(row + 1, col) {
                    union(&mut parent, index, other);
                }
            }
        }

        let mut area = vec![0.0f64; self.found];
        for index in 0..self.found {
            let root = find(&mut parent, index);
            let row = self.row_of(index);
            area[root] += self.area_m2[row];
        }
        let mut areas: Vec<f64> = (0..self.found)
            .filter(|index| find(&mut parent, *index) == *index)
            .map(|index| area[index])
            .collect();
        areas.sort_by(|a, b| b.total_cmp(a));
        areas
    }
}

/// The value at rank `q` of a descending-sorted list, by nearest rank -- `q = 0` is the
/// largest and `q = 1` the smallest, so `quantile_desc(areas, 0.10)` is the **90th percentile
/// by area** and `quantile_desc(areas, 0.90)` is the 10th. The caller's labels say so; the
/// first revision of this binary printed them the other way round and called a 66 km2 island
/// the ninetieth percentile of a distribution whose median was 323 km2.
fn quantile_desc(sorted_desc: &[f64], q: f64) -> f64 {
    if sorted_desc.is_empty() {
        return f64::NAN;
    }
    let last = sorted_desc.len() - 1;
    let last_float = last as f64; // cast-ok: a length below 2^53
    let rank = last_float * q + 0.5;
    let mut index = rank as usize; // cast-ok: truncation of a positive rank; the +0.5 makes it a round
    if index > last {
        index = last;
    }
    sorted_desc[index]
}

fn print_shares(label: &str, s: &Shares) {
    println!(
        "  {label:<34} n {:>7}  D_added {:>6} = {:>7.4}%   D_offshore {:>6} = {:>7.4}%   \
         land {:>7.4}% (plain {:>7.4}%)",
        s.count,
        s.added,
        s.added_share() * 100.0,
        s.offshore,
        s.offshore_share() * 100.0,
        s.land_share() * 100.0,
        s.plain_land_share() * 100.0,
    );
}

fn band_verdict(share: f64) -> &'static str {
    if share < BAND_LOW {
        "BELOW"
    } else if share > BAND_HIGH {
        "ABOVE"
    } else {
        "IN BAND"
    }
}

fn main() {
    let want_components = std::env::args().any(|a| a == "components");

    println!("=== island_survey: the seamount field, measured through `Surface` over ocean ===");
    println!(
        "spec band: {:.1}%-{:.1}% of the planet's surface as islands",
        BAND_LOW * 100.0,
        BAND_HIGH * 100.0
    );
    println!(
        "shipped trio at the time of this run: height_m {} density {} reach_m {} min_depth_m {} \
         lattice_m {}",
        PeakParams::volcanic().height_m,
        PeakParams::volcanic().density,
        PeakParams::volcanic().reach_m,
        PeakParams::volcanic().min_depth_m,
        PeakParams::volcanic().lattice_m,
    );
    println!();

    let sweep_world = WORLDS[0];
    let sweep_plain = plain(&sweep_world);
    let small = plain_spiral(&sweep_plain, SPIRAL_SMALL);
    let large = plain_spiral(&sweep_plain, SPIRAL_LARGE);

    // --- Section 1: spec §1's before-case, at the canonical preset, on every world. ---
    //
    // **Spec §1 states its measurement over 20,000 area-uniform points on the OWNER'S world**,
    // so that world is measured here too rather than only the sweep fixture. All three are
    // reported, because §1's claim is about the generator and not about one planet.
    println!("--- 1. spec §1's before-case: what this generator made before the block ---");
    for world in WORLDS {
        let base = plain(&world);
        let spiral = plain_spiral(&base, SPIRAL_SMALL);
        let inert = peaked(&world, PeakParams::canonical());
        println!(
            "  world {} (seed {}, radius {} m, {} plates, land_fraction {})",
            world.name, world.seed, world.radius_m, world.plates, world.land_fraction
        );
        print_shares("Surface::new (no block)", &shares(&base, &spiral));
        print_shares("PeakParams::canonical()", &shares(&inert, &spiral));
        for count in [SPIRAL_SMALL, SPIRAL_LARGE] {
            let (offset, ground, oceanic, above_datum) = arc_reach(&base, count);
            println!(
                "    island arc, n {count:>7}: {oceanic} points the continent field calls sea; \
                 tallest tectonic offset {offset:.2} m (abyss 4,600 m, short by {:.2}x)",
                4_600.0 / offset
            );
            // **`above_datum` is shoreline wobble, not islands, and the spec says so itself**
            // -- §1 records 65 such points on the owner's world and calls them "shoreline
            // wobble, the deepest by -634 m". They are points the continent field puts just
            // below its own shore threshold that tectonics and the shelf then lift over the
            // datum. The claim §1 actually makes about OPEN ocean is `D_offshore`, printed
            // above, and it is zero here on every world.
            println!(
                "                     {above_datum} of those stand above the datum (shoreline \
                 wobble -- see spec §1's own 65); tallest structural_m among them {ground:.2} m"
            );
        }
    }
    println!();

    // --- Section 2: the density sweep, at the shipped lattice. ---
    println!(
        "--- 2. density sweep, lattice_m {} / reach_m {} (ratio {REACH_RATIO}) ---",
        LATTICES_M[SHIPPED_LATTICE].0, LATTICES_M[SHIPPED_LATTICE].1
    );
    for density in DENSITIES {
        let peaks = swept(density, LATTICES_M[SHIPPED_LATTICE]);
        let world = peaked(&sweep_world, peaks);
        let s_small = shares(&world, &small);
        let s_large = shares(&world, &large);
        println!(
            "  density {density:<5}  n {:>7} {:>7.4}%   n {:>7} {:>7.4}%  [{}]   D_offshore@20k {:>5}",
            SPIRAL_SMALL,
            s_small.added_share() * 100.0,
            SPIRAL_LARGE,
            s_large.added_share() * 100.0,
            band_verdict(s_large.added_share()),
            s_small.offshore,
        );
    }
    println!();

    // --- Section 3: the lattice sweep, at the shipped density and at the chosen one. ---
    println!("--- 3. lattice sweep at ratio {REACH_RATIO}, three densities ---");
    for density in [0.11, PeakParams::volcanic().density, 0.58] {
        for pair in LATTICES_M {
            let peaks = swept(density, pair);
            let world = peaked(&sweep_world, peaks);
            let s = shares(&world, &large);
            println!(
                "  density {density:<5} lattice_m {:>8} reach_m {:>8}  n {} {:>7.4}%  [{}]",
                pair.0,
                pair.1,
                SPIRAL_LARGE,
                s.added_share() * 100.0,
                band_verdict(s.added_share()),
            );
        }
    }
    println!();

    // --- Section 4: the candidate densities, on all three worlds. This is the choice. ---
    println!(
        "--- 4. candidate densities on all three worlds, at the shipped geometry (lattice_m {} / \
         reach_m {}), n {} ---",
        LATTICES_M[SHIPPED_LATTICE].0,
        LATTICES_M[SHIPPED_LATTICE].1,
        SPIRAL_LARGE
    );
    println!(
        "  A density is only admissible if its WORST world is inside the band, because the \
         constant is one number and the spec's band is about a planet."
    );
    let mut per_world: Vec<(World, PlainSpiral)> = Vec::new();
    for world in WORLDS {
        let base = plain(&world);
        let spiral = plain_spiral(&base, SPIRAL_LARGE);
        per_world.push((world, spiral));
    }
    print!("  {:<9}", "density");
    for (world, _) in &per_world {
        print!("   {:>9} (land {:>4})", world.name, world.land_fraction);
    }
    println!("   verdict");
    for density in CANDIDATES {
        print!("  {density:<9}");
        let mut lowest = f64::INFINITY;
        let mut highest = f64::NEG_INFINITY;
        for (world, spiral) in &per_world {
            let peaked_world = peaked(world, swept(density, LATTICES_M[SHIPPED_LATTICE]));
            let share = shares(&peaked_world, spiral).added_share();
            if share < lowest {
                lowest = share;
            }
            if share > highest {
                highest = share;
            }
            print!("   {:>9.4}%       ", share * 100.0);
        }
        let verdict = if lowest < BAND_LOW {
            "one world BELOW"
        } else if highest > BAND_HIGH {
            "one world ABOVE"
        } else {
            "ALL IN BAND"
        };
        // The margin an admissible density has to the nearer band edge, in percentage points.
        // The pick is the MAXIMIN: the admissible density whose worst world sits furthest from
        // an edge. A density admissible only by 0.0065 pp is admissible on this corpus and
        // would not survive a fourth world.
        let margin = {
            let low = (lowest - BAND_LOW) * 100.0;
            let high = (BAND_HIGH - highest) * 100.0;
            if low < high {
                low
            } else {
                high
            }
        };
        println!("   {verdict:<16} margin to the nearer edge {margin:>+8.4} pp");
    }
    println!(
        "  The pick is the MAXIMIN margin above -- the admissible density whose worst world is \
         furthest from an edge."
    );
    println!();

    // --- Section 4b: the shipped block, on all three worlds, in full. ---
    println!("--- 4b. the shipped `volcanic()` block, on all three worlds ---");
    for (world, spiral) in &per_world {
        let peaked_world = peaked(world, PeakParams::volcanic());
        let s = shares(&peaked_world, spiral);
        println!(
            "  {:<9} seed {:>10} radius {:>9} plates {:>3} land {:>4}",
            world.name, world.seed, world.radius_m, world.plates, world.land_fraction
        );
        print_shares("volcanic()", &s);
        println!(
            "    requested land_fraction {:.4} / achieved (plain) {:.4} / achieved (volcanic) \
             {:.4}  -- islands add {:+.4} pp   [{}]",
            world.land_fraction,
            s.plain_land_share(),
            s.land_share(),
            (s.land_share() - s.plain_land_share()) * 100.0,
            band_verdict(s.added_share()),
        );
    }
    println!();

    // --- Section 5: steep-to, against a continental shelf. ---
    println!("--- 5. steep-to: depth off an island, against a continental shelf ---");
    let shipped = PeakParams::volcanic();
    let peaked_sweep = peaked(&sweep_world, shipped);
    match find_a_summit(&peaked_sweep, &large) {
        None => println!("  no island found on {} -- nothing to walk out from", sweep_world.name),
        Some((summit, height)) => {
            println!(
                "  island summit on {}: structural_m {height:.2} m, tectonic offset {:.2} m",
                sweep_world.name,
                peaked_sweep.tectonics.offset_m(&summit)
            );
            for fraction in WALK_FRACTIONS {
                let walk_m = fraction * shipped.reach_m;
                let (deepest, shallowest) = ring(&peaked_sweep, &summit, walk_m);
                println!(
                    "    {:>5.2} x reach_m = {walk_m:>9.0} m out: deepest {deepest:>10.2} m, \
                     shallowest {shallowest:>10.2} m",
                    fraction
                );
            }
            // Substrate and detail, at the summit and one reach off it.
            println!("  --- what a peak does to substrate and to detail roughness ---");
            // `Detail::relief` is private, and the world above was built with `relief: None`,
            // which `Detail::with_gully` turns into exactly `ReliefParams::canonical()` -- so
            // this is the same block the `amplitude_m` calls below are actually using, read
            // from the one public place it is written down rather than transcribed.
            let relief = ReliefParams::canonical();
            for (label, point) in [
                ("summit", summit),
                (
                    "1.1 x reach off it",
                    TangentFrame::at(&summit, peaked_sweep.radius_m)
                        .local_to_sphere(1.1 * shipped.reach_m, 0.0),
                ),
            ] {
                let reading = peaked_sweep.shelf.evaluate(&point);
                let slope = substrate::slope_at(peaked_sweep.radius_m, &point, 1_000.0, &|p| {
                    peaked_sweep.structural_m(p)
                });
                let composition = substrate::natural(reading.elevation_m, slope, reading.tectonic_m);
                let amplitude = peaked_sweep.detail.amplitude_m(
                    &point,
                    reading.elevation_m,
                    reading.weight,
                    reading.tectonic_m,
                );
                let quiet_amplitude = peaked_sweep.detail.amplitude_m(
                    &point,
                    reading.elevation_m,
                    reading.weight,
                    0.0,
                );
                let tectonic_argument = magnitude(reading.tectonic_m) / ROCK_TECTONIC_M;
                let quieting_argument =
                    magnitude(reading.tectonic_m) / relief.quieting_scale_m;
                println!(
                    "    {label:<20} elevation_m {:>10.2}  tectonic_m {:>10.2}  shelf weight \
                     {:>6.4}  slope {:>8.5}",
                    reading.elevation_m, reading.tectonic_m, reading.weight, slope
                );
                println!(
                    "      substrate::natural -> sand {:.6} mud {:.6} rock {:.6}   \
                     by_tectonics = smooth({:.2}/{ROCK_TECTONIC_M}) = smooth({tectonic_argument:.4}) \
                     = {:.6}   by_slope = smooth({:.5}/{ROCK_SLOPE}) = {:.6}",
                    composition.sand,
                    composition.mud,
                    composition.rock,
                    reading.tectonic_m,
                    saturation(tectonic_argument),
                    slope,
                    saturation(slope / ROCK_SLOPE),
                );
                println!(
                    "      Detail::amplitude_m -> {amplitude:.6} m   with tectonic_m 0 it would \
                     be {quiet_amplitude:.6} m   quieting = 1 - {:.2} * smooth({:.2}/{:.2}) = 1 - \
                     {:.2} * smooth({quieting_argument:.4}) = {:.6}",
                    relief.quieting_strength,
                    magnitude(reading.tectonic_m),
                    relief.quieting_scale_m,
                    relief.quieting_strength,
                    1.0 - relief.quieting_strength
                        * saturation(quieting_argument),
                );
            }
        }
    }
    match find_a_shore(&large) {
        None => println!("  no continental shore found -- no shelf contrast"),
        Some((shore, height)) => {
            println!(
                "  continental shore on {} (the plain world's lowest land): structural_m \
                 {height:.2} m. SHELF_BREAK_M is {SHELF_BREAK_M} m.",
                sweep_world.name
            );
            for walk_m in SHELF_WALK_M {
                let (deepest, shallowest) = ring(&sweep_plain, &shore, walk_m);
                println!(
                    "    {walk_m:>9.0} m out: deepest {deepest:>10.2} m, shallowest \
                     {shallowest:>10.2} m"
                );
            }
        }
    }
    println!();

    // --- Section 6: the component raster. ---
    if !want_components {
        println!("--- 6. island count and area distribution: SKIPPED ---");
        println!("  re-run with `-- components` to add it. It is a {COMPONENT_SPACING_M:.0} m \
                  raster of the whole sphere and costs two orders of magnitude more field \
                  evaluations than every section above it together.");
        return;
    }
    println!(
        "--- 6. island count and area distribution, raster spacing {COMPONENT_SPACING_M:.0} m ---"
    );
    for (density, pair) in [
        (PeakParams::volcanic().density, LATTICES_M[SHIPPED_LATTICE]),
        (0.11, LATTICES_M[SHIPPED_LATTICE]),
        (PeakParams::volcanic().density, LATTICES_M[0]),
        (PeakParams::volcanic().density, LATTICES_M[3]),
    ] {
        let lattice_m = pair.0;
        let peaks = swept(density, pair);
        let world = peaked(&sweep_world, peaks);
        let mask = IslandMask::build(&sweep_world, &world, &sweep_plain, COMPONENT_SPACING_M);
        let areas = mask.island_areas_m2();
        let total: f64 = areas.iter().sum();
        let sphere = 4.0 * core::f64::consts::PI * sweep_world.radius_m * sweep_world.radius_m;
        let counted = areas.len() as f64; // cast-ok: a component count, far below 2^53
        println!(
            "  density {density} lattice_m {lattice_m}: rows {} cols {} sampled {} island cells \
             {} ({:.4}% of cells)",
            mask.rows,
            mask.cols,
            mask.sampled,
            mask.found,
            {
                let found = mask.found as f64; // cast-ok: a count below `sampled`
                let sampled = mask.sampled as f64; // cast-ok: a count, far below 2^53
                found / sampled * 100.0
            }
        );
        println!(
            "    distinct islands {}   total island area {:.0} km2 = {:.4}% of the sphere   \
             mean {:.1} km2",
            areas.len(),
            total / 1.0e6,
            total / sphere * 100.0,
            total / counted / 1.0e6,
        );
        if !areas.is_empty() {
            println!(
                "    area distribution, km2: largest {:.1}  p90 {:.1}  median {:.1}  p10 {:.1}  \
                 smallest {:.1}",
                areas[0] / 1.0e6,
                quantile_desc(&areas, 0.10) / 1.0e6,
                quantile_desc(&areas, 0.50) / 1.0e6,
                quantile_desc(&areas, 0.90) / 1.0e6,
                areas[areas.len() - 1] / 1.0e6,
            );
            let mut bins = [0usize; 6];
            for area in &areas {
                let km2 = area / 1.0e6;
                let bin = if km2 < 50.0 {
                    0
                } else if km2 < 200.0 {
                    1
                } else if km2 < 500.0 {
                    2
                } else if km2 < 2_000.0 {
                    3
                } else if km2 < 10_000.0 {
                    4
                } else {
                    5
                };
                bins[bin] += 1;
            }
            println!(
                "    histogram, km2: <50 {}  50-200 {}  200-500 {}  500-2k {}  2k-10k {}  \
                 >=10k {}",
                bins[0], bins[1], bins[2], bins[3], bins[4], bins[5]
            );
        }
    }
}
