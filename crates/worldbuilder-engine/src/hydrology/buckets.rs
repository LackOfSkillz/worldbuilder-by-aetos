//! Points on the sphere, bucketed by latitude row and longitude column, so "what is near here"
//! looks at a handful of buckets rather than at everything.
//!
//! **Rows of equal height, columns of equal width on the ground.** A plain lat/lon grid
//! crowds its columns together at the poles; here each row has as many columns as its own
//! circumference holds, so a bucket is roughly `cell_m` square everywhere. The seam at +/-180
//! and the poles are handled by the query, not by special buckets.

use crate::detmath as m;
use crate::sphere::SpherePoint;

const MAX_ROWS: usize = 4_096;
const MAX_COLUMNS: usize = 8_192;

#[derive(Debug, Clone)]
pub struct BucketIndex {
    radius_m: f64,
    rows: usize,
    columns: Vec<usize>,
    first: Vec<usize>,
    buckets: Vec<Vec<u32>>,
}

/// The finest cell a `BucketIndex` can actually realise on a sphere of `radius_m`.
///
/// [`BucketIndex::new`] clamps at `MAX_ROWS` rows and `MAX_COLUMNS` columns **and says nothing**,
/// so a caller that asks for a finer cell than this silently gets a coarser index than it asked
/// for. That is harmless for a caller that only wants "what is near here" -- the query still
/// finds everything, it just looks at more of it -- and wrong for a caller whose cell *is* the
/// thing it means, such as Ruling S-8's density cap or a survey baseline meant to be no cap at
/// all. On Earth's radius it is 4,886.50 m, so 23.88 km² per cell.
pub fn finest_cell_m(radius_m: f64) -> f64 {
    let by_rows = core::f64::consts::PI * radius_m / MAX_ROWS as f64;
    let by_columns = 2.0 * core::f64::consts::PI * radius_m / MAX_COLUMNS as f64;
    if by_columns > by_rows { by_columns } else { by_rows }
}

impl BucketIndex {
    /// The cell this index **realised**, in metres, which is the cell it was asked for only when
    /// that was at or above [`finest_cell_m`]. See there for why the difference matters.
    pub fn cell_m(&self) -> f64 {
        core::f64::consts::PI * self.radius_m / self.rows as f64
    }

    pub fn new(radius_m: f64, cell_m: f64) -> Self {
        let span = core::f64::consts::PI * radius_m / cell_m;
        let mut rows = if span >= 1.0 { m::floor(span) as usize } else { 1 };
        if rows > MAX_ROWS {
            rows = MAX_ROWS;
        }
        let mut columns = Vec::with_capacity(rows);
        let mut first = Vec::with_capacity(rows + 1);
        let mut total = 0usize;
        for row in 0..rows {
            let middle = -90.0 + (row as f64 + 0.5) * 180.0 / rows as f64;
            let around = 2.0 * core::f64::consts::PI * radius_m * m::cos(m::to_radians(middle));
            let raw = around / cell_m;
            let mut count = if raw >= 1.0 { m::floor(raw) as usize } else { 1 };
            if count > MAX_COLUMNS {
                count = MAX_COLUMNS;
            }
            first.push(total);
            columns.push(count);
            total += count;
        }
        first.push(total);
        Self { radius_m, rows, columns, first, buckets: vec![Vec::new(); total] }
    }

    fn row_of(&self, latitude_deg: f64) -> usize {
        let raw = (latitude_deg + 90.0) / 180.0 * self.rows as f64;
        let row = if raw <= 0.0 { 0 } else { m::floor(raw) as usize };
        if row >= self.rows { self.rows - 1 } else { row }
    }

    fn column_of(&self, row: usize, longitude_deg: f64) -> usize {
        let count = self.columns[row];
        let raw = (longitude_deg + 180.0) / 360.0 * count as f64;
        let column = if raw <= 0.0 { 0 } else { m::floor(raw) as usize };
        if column >= count { count - 1 } else { column }
    }

    /// Which cell a point falls in, as a stable index into this index's own grid.
    ///
    /// For a caller that wants "one per cell" rather than "what is near here": Ruling S-8's
    /// density cap walks candidates deepest-first and keeps the first one it sees in each cell.
    /// The value means nothing beyond "the same cell or not" -- two points with the same index
    /// are in the same roughly-`cell_m`-square bucket, and neighbouring indices are not
    /// neighbouring cells at a row boundary.
    pub fn cell_of(&self, point: &SpherePoint) -> usize {
        let (lat, lon) = point.to_latlon();
        let row = self.row_of(lat);
        self.first[row] + self.column_of(row, lon)
    }

    pub fn insert(&mut self, point: &SpherePoint, id: u32) {
        let (lat, lon) = point.to_latlon();
        let row = self.row_of(lat);
        let column = self.column_of(row, lon);
        self.buckets[self.first[row] + column].push(id);
    }

    /// How many cells this index's grid has. A caller keeping its own payload alongside the
    /// grid -- `water::index::WaterIndex` keeps three -- sizes them by this and addresses them
    /// with [`cell_of`](Self::cell_of) and [`cells_within`](Self::cells_within).
    pub fn cell_count(&self) -> usize {
        self.buckets.len()
    }

    /// For a survey: what this grid occupies in bytes -- the `columns` and `first` row tables,
    /// plus its own per-cell `Vec` headers and whatever those `Vec`s have allocated.
    ///
    /// A caller that only ever asks [`cell_of`](Self::cell_of), [`cell_count`](Self::cell_count)
    /// and [`cells_within`](Self::cells_within) -- `water::index::WaterIndex` is one -- never
    /// calls [`insert`](Self::insert), so for it the `buckets` term is a cell's worth of empty
    /// `Vec` header apiece and nothing else. It is counted here rather than hidden, because a
    /// cost paid for addressing alone is exactly the kind that goes unmeasured.
    ///
    /// A proxy, summed from each `Vec`'s own reported size rather than sampled from an
    /// allocator: allocator rounding and this struct's own fields are not in it.
    pub fn memory_bytes(&self) -> usize {
        self.columns.len() * core::mem::size_of::<usize>()
            + self.first.len() * core::mem::size_of::<usize>()
            + self.buckets.len() * core::mem::size_of::<Vec<u32>>()
            + self.buckets.iter().map(|b| b.capacity() * core::mem::size_of::<u32>()).sum::<usize>()
    }

    /// Every cell whose row/column range the disc of `reach_m` about `point` touches, ascending.
    ///
    /// A **superset** of the cells the disc actually intersects -- the sweep is a latitude band
    /// crossed with the widest longitude stretch the band needs -- so a caller may rely on it
    /// listing every cell that holds any point within `reach_m`, and must not rely on it listing
    /// only those. [`candidates`](Self::candidates) is this sweep with the points in each cell
    /// collected; a caller with its own per-cell payload takes the cells instead.
    pub fn cells_within(&self, point: &SpherePoint, reach_m: f64) -> Vec<usize> {
        let mut cells = Vec::new();
        self.sweep(point, reach_m, |cell| cells.push(cell));
        cells.sort_unstable();
        cells.dedup();
        cells
    }

    pub fn candidates(&self, point: &SpherePoint, reach_m: f64) -> Vec<u32> {
        let mut found = Vec::new();
        self.sweep(point, reach_m, |cell| found.extend_from_slice(&self.buckets[cell]));
        found.sort_unstable();
        found.dedup();
        found
    }

    /// The row/column arithmetic both of the two above are made of: hands `visit` each cell of
    /// the sweep, row by row, without deciding what to do with it.
    fn sweep(&self, point: &SpherePoint, reach_m: f64, mut visit: impl FnMut(usize)) {
        let (lat, lon) = point.to_latlon();
        let reach_deg = m::to_degrees(reach_m / self.radius_m);
        let low = self.row_of(if lat - reach_deg < -90.0 { -90.0 } else { lat - reach_deg });
        let high = self.row_of(if lat + reach_deg > 90.0 { 90.0 } else { lat + reach_deg });
        // Loop-invariant, so it is asked once rather than once per row: a reach that carries the
        // query past a pole (its latitude band running off the top or bottom of the grid) covers
        // every longitude at EVERY row it touches, regardless of any row's own stretch -- the
        // row's whole circle is within reach once the cap itself is, so there is no narrower
        // column range to compute anywhere in the sweep.
        let pole_crossing = lat.abs() + reach_deg >= 90.0;
        for row in low..=high {
            let south = -90.0 + row as f64 * 180.0 / self.rows as f64;
            let north = south + 180.0 / self.rows as f64;
            // The widest point of the row decides how far the LINEAR longitude reach stretches.
            let widest = if south.abs() > north.abs() { south.abs() } else { north.abs() };
            let cos = m::cos(m::to_radians(widest));
            let count = self.columns[row];
            // The linear form alone is not a bound (see `half_extent_deg`), so the stretch is the
            // larger of it and the exact half-extent. Taking the larger rather than replacing the
            // linear form is deliberate: no caller's candidate set can shrink, so nothing already
            // measured against this grid moves, and the exact term alone already makes it a
            // superset.
            let linear = if cos <= 1.0e-9 { 180.0 } else { reach_deg / cos };
            // **Both whole-row tests are decided before `half_extent_deg` runs, and neither
            // needs it.** `stretch` is the LARGER of `linear` and `exact`, so `linear >= 180.0`
            // settles `everything` on its own, and `pole_crossing` does not read the stretch at
            // all. `half_extent_deg` is about eight transcendental calls per row, and `sweep` is
            // on the bake's hot path through `candidates` and `nearest` -- so before this hoist
            // a pole-crossing sweep computed, and then discarded, that work on every row it
            // touched. Nothing observable changes: where `whole_row` holds, the exact term could
            // only have made `stretch` larger, and `everything` was already true either way.
            let whole_row = linear >= 180.0 || pole_crossing;
            let stretch = if whole_row {
                linear
            } else {
                let exact = half_extent_deg(lat, reach_deg, south, north);
                if exact > linear { exact } else { linear }
            };
            let everything = whole_row || stretch >= 180.0;
            if everything {
                for column in 0..count {
                    visit(self.first[row] + column);
                }
                continue;
            }
            let west = self.column_of(row, wrap(lon - stretch));
            let east = self.column_of(row, wrap(lon + stretch));
            let mut column = west;
            loop {
                visit(self.first[row] + column);
                if column == east {
                    break;
                }
                column = (column + 1) % count;
            }
        }
    }

    pub fn nearest(&self, point: &SpherePoint, positions: &[SpherePoint]) -> Option<u32> {
        if positions.is_empty() {
            return None;
        }
        let mut reach = core::f64::consts::PI * self.radius_m / self.rows as f64;
        loop {
            let mut best: Option<(f64, u32)> = None;
            for id in self.candidates(point, reach) {
                // Unchecked: every id `candidates` returns came from `insert`, whose caller is
                // trusted to pass the same id space as `positions` here -- one entry per
                // position, in the same index. `candidates` never invents an id.
                let d = point.distance_to(&positions[id as usize], self.radius_m);
                best = match best {
                    Some((bd, bid)) if bd < d || (bd == d && bid < id) => Some((bd, bid)),
                    _ => Some((d, id)),
                };
            }
            if let Some((d, id)) = best {
                if d <= reach {
                    return Some(id);
                }
            }
            if reach >= core::f64::consts::PI * self.radius_m {
                return best.map(|(_, id)| id);
            }
            reach *= 2.0;
        }
    }
}

/// How far east or west of its own meridian a circle of angular radius `reach_deg` about
/// `centre_lat_deg` reaches, anywhere in the latitude row `[south_deg, north_deg]`, in degrees.
///
/// The exact half-extent at one latitude φ, for a circle of radius `r` about latitude `φ₀`, is
///
/// ```text
/// Δλ(φ) = acos( (cos r − sin φ₀ sin φ) / (cos φ₀ cos φ) )
/// ```
///
/// which is what the linear `r / cos φ` approximates. The approximation is not a bound: it
/// over-covers while `r` is small -- which is why the linear form served every caller correctly
/// until plan 2a's index started asking for megametres -- and under-covers once `r` is a fair
/// fraction of a radian, by 66 km at latitude 70 on a 2,061 km circle.
///
/// `Δλ` is unimodal in φ with its maximum at the tangency latitude `sin φₜ = sin φ₀ / cos r`
/// (where a meridian touches the circle), so the largest value the row can hold is at one of its
/// two edges or at `φₜ` if that falls between them: three evaluations, and no search.
///
/// `acos`, which `detmath` does not carry, is written `atan2(sqrt(1 − x²), x)`.
fn half_extent_deg(centre_lat_deg: f64, reach_deg: f64, south_deg: f64, north_deg: f64) -> f64 {
    let r = m::to_radians(reach_deg);
    let centre = m::to_radians(centre_lat_deg);
    let cos_r = m::cos(r);
    let sin_centre = m::sin(centre);
    let cos_centre = m::cos(centre);
    // A circle about a pole covers every longitude of every row it reaches at all.
    if cos_centre <= 1.0e-12 {
        return 180.0;
    }
    let sin_tangency = sin_centre / cos_r;
    let tangency_deg = if sin_tangency >= 1.0 {
        90.0
    } else if sin_tangency <= -1.0 {
        -90.0
    } else {
        m::to_degrees(m::asin(sin_tangency))
    };
    let inside = if tangency_deg < south_deg {
        south_deg
    } else if tangency_deg > north_deg {
        north_deg
    } else {
        tangency_deg
    };
    let mut widest = 0.0;
    for latitude_deg in [south_deg, north_deg, inside] {
        let latitude = m::to_radians(latitude_deg);
        let cos_lat = m::cos(latitude);
        // A row edge sitting exactly on a pole: a circle that reached the pole took the caller's
        // `everything` branch, so one that did not reaches nothing at this latitude.
        if cos_lat <= 1.0e-12 {
            continue;
        }
        let x = (cos_r - sin_centre * m::sin(latitude)) / (cos_centre * cos_lat);
        let here = if x <= -1.0 {
            180.0
        } else if x >= 1.0 {
            // The circle does not reach this latitude at all.
            0.0
        } else {
            m::to_degrees(m::atan2(m::sqrt(1.0 - x * x), x))
        };
        if here > widest {
            widest = here;
        }
    }
    // A tenth of a millimetre of slack, so a cell edge that the exact value lands on is swept
    // rather than decided by the last bit of an `atan2`.
    widest + 1.0e-9
}

/// A longitude in (-180, 180].
fn wrap(longitude_deg: f64) -> f64 {
    let mut value = longitude_deg;
    while value > 180.0 {
        value -= 360.0;
    }
    while value <= -180.0 {
        value += 360.0;
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sphere::SpherePoint;

    const R: f64 = 6_371_000.0;

    /// `new`'s clamp is silent, so the only way a caller can tell it happened is to compare what
    /// it asked for against [`BucketIndex::cell_m`]. At and above [`finest_cell_m`] the two agree;
    /// below it, they do not, however small the request gets.
    #[test]
    fn the_realised_cell_matches_the_request_only_at_or_above_the_finest() {
        let finest = finest_cell_m(R);
        assert!(finest > 4_886.0 && finest < 4_887.0, "about 4,886.5 m on Earth's radius: {finest}");
        for &asked in &[finest, 10_000.0, 100_000.0] {
            let got = BucketIndex::new(R, asked).cell_m();
            let off = if got > asked { got - asked } else { asked - got };
            assert!(off < asked * 1.0e-3, "asked {asked} m, realised {got} m");
        }
        // The survey's old baseline, and the floor `bake` now refuses: 100 m asked, 23.88 km^2 of
        // cell delivered. Exact equality, not a tolerance, because a correction rests on it --
        // `pond_search_survey`'s old `1.0e4` baseline realised the very grid the honest baseline
        // asks for, so the figures it produced stand and only their label was wrong.
        assert_eq!(BucketIndex::new(R, 100.0).cell_m(), BucketIndex::new(R, finest).cell_m(),
                   "a request below the finest is clamped to exactly the finest, silently");
    }

    fn brute_nearest(p: &SpherePoint, positions: &[SpherePoint]) -> u32 {
        let mut best = 0u32;
        let mut best_d = f64::INFINITY;
        for (i, q) in positions.iter().enumerate() {
            let d = p.distance_to(q, R);
            if d < best_d {
                best_d = d;
                best = i as u32; // cast-ok: test fixture of a few thousand points
            }
        }
        best
    }

    fn scatter(count: u32) -> Vec<SpherePoint> {
        (0..count).map(|i| crate::stream::spiral_point(i, count)).collect()
    }

    #[test]
    fn nearest_agrees_with_brute_force_everywhere_including_poles_and_seam() {
        let positions = scatter(3_000);
        let mut index = BucketIndex::new(R, 200_000.0);
        for (i, p) in positions.iter().enumerate() {
            index.insert(p, i as u32); // cast-ok: test fixture of a few thousand points
        }
        let probes = [
            (89.9, 10.0), (-89.9, -170.0), (0.0, 179.99), (0.0, -179.99),
            (45.0, 0.0), (-33.3, 120.5), (60.0, 180.0), (12.0, -45.0),
        ];
        for (lat, lon) in probes {
            let p = SpherePoint::from_latlon(lat, lon);
            assert_eq!(index.nearest(&p, &positions), Some(brute_nearest(&p, &positions)),
                       "probe {lat},{lon}");
        }
    }

    #[test]
    fn candidates_include_every_point_within_reach() {
        let positions = scatter(3_000);
        let mut index = BucketIndex::new(R, 150_000.0);
        for (i, p) in positions.iter().enumerate() {
            index.insert(p, i as u32); // cast-ok: test fixture
        }
        let centre = SpherePoint::from_latlon(70.0, 179.0);
        let reach = 900_000.0;
        let found = index.candidates(&centre, reach);
        for (i, q) in positions.iter().enumerate() {
            if centre.distance_to(q, R) <= reach {
                assert!(found.contains(&(i as u32)), "missed {i}"); // cast-ok: test fixture
            }
        }
        let mut sorted = found.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(found, sorted, "candidates are sorted and unique");
    }

    /// `cell_of` is the density cap's "same cell or not" test: two points a small fraction of a
    /// cell apart share one, two points several cells apart do not, and it needs no `insert`.
    #[test]
    fn cell_of_agrees_with_itself_and_separates_distant_points() {
        let index = BucketIndex::new(R, 22_360.0); // 500 km^2, Ruling S-8's cell
        let here = SpherePoint::from_latlon(10.0, 20.0);
        assert_eq!(index.cell_of(&here), index.cell_of(&here));
        // 2 km north-east: well inside one 22.4 km cell unless it straddles a boundary, so the
        // assertion that matters is the far one below.
        let far = SpherePoint::from_latlon(10.5, 20.5);
        assert_ne!(index.cell_of(&here), index.cell_of(&far), "55 km apart is not one cell");
        // The poles and the seam are cells like any other, not a panic.
        for (lat, lon) in [(90.0, 0.0), (-90.0, 0.0), (0.0, 180.0), (0.0, -180.0)] {
            let cell = index.cell_of(&SpherePoint::from_latlon(lat, lon));
            assert!(cell < index.buckets.len(), "{lat},{lon} landed outside the grid");
        }
    }

    /// The superset guarantee at the radii `water::index`'s bounding circles actually use.
    ///
    /// `candidates_include_every_point_within_reach` checks 900 km at one place. The longitude
    /// stretch used to be the linear `reach_deg / cos(row's widest latitude)`, which over-covers
    /// while the reach is small -- every caller before plan 2a's index -- and **under**-covers
    /// once the reach is a fair fraction of a radian. On this grid a 2,061 km circle fell short
    /// by 0.5 km at latitude 45, 18.8 km at 60 and 66.1 km at 70, which is more than a body's
    /// whole shore band: real cells on the circle's east and west flanks went unlisted. This is
    /// the brute-force check across the latitudes and radii where that bites.
    #[test]
    fn candidates_include_every_point_within_reach_at_megametre_radii_and_high_latitude() {
        let positions = scatter(20_000);
        let mut index = BucketIndex::new(R, 50_000.0);
        for (i, p) in positions.iter().enumerate() {
            index.insert(p, i as u32); // cast-ok: test fixture
        }
        for lat in [0.0, 30.0, 45.0, 60.0, 70.0, 80.0, -70.0] {
            for lon in [0.0, 179.5, -120.0] {
                let centre = SpherePoint::from_latlon(lat, lon);
                for reach in [500_000.0, 1_054_219.0, 2_060_743.0] {
                    let found = index.candidates(&centre, reach);
                    for (i, q) in positions.iter().enumerate() {
                        let d = centre.distance_to(q, R);
                        if d <= reach {
                            assert!(found.contains(&(i as u32)), // cast-ok: test fixture
                                    "missed point {i} at {d} m from {lat},{lon}, reach {reach}");
                        }
                    }
                }
            }
        }
    }

    /// `candidates` and `cells_within` are one sweep with two endings, and a caller keeping its
    /// own payload per cell (`water::index`) relies on that: whatever `candidates` would have
    /// collected must live in the cells `cells_within` names. Checked at the poles and the seam,
    /// where the column arithmetic wraps.
    #[test]
    fn cells_within_names_exactly_the_cells_candidates_reads() {
        let positions = scatter(3_000);
        let mut index = BucketIndex::new(R, 150_000.0);
        for (i, p) in positions.iter().enumerate() {
            index.insert(p, i as u32); // cast-ok: test fixture
        }
        for (lat, lon) in [(0.0, 0.0), (89.9, 10.0), (-89.9, -170.0), (0.0, 179.99),
                           (0.0, -179.99), (45.0, 90.0)] {
            let p = SpherePoint::from_latlon(lat, lon);
            for reach in [10_000.0, 400_000.0, 3_000_000.0] {
                let cells = index.cells_within(&p, reach);
                assert!(cells.iter().all(|&c| c < index.cell_count()), "{lat},{lon} r{reach}");
                let mut through_cells: Vec<u32> =
                    cells.iter().flat_map(|&c| index.buckets[c].iter().copied()).collect();
                through_cells.sort_unstable();
                through_cells.dedup();
                assert_eq!(index.candidates(&p, reach), through_cells, "{lat},{lon} r{reach}");
            }
        }
    }

    #[test]
    fn an_empty_index_has_no_nearest() {
        let index = BucketIndex::new(R, 100_000.0);
        assert_eq!(index.nearest(&SpherePoint::from_latlon(0.0, 0.0), &[]), None);
    }
}
