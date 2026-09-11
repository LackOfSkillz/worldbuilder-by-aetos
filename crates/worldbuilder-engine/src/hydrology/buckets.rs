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

impl BucketIndex {
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

    pub fn insert(&mut self, point: &SpherePoint, id: u32) {
        let (lat, lon) = point.to_latlon();
        let row = self.row_of(lat);
        let column = self.column_of(row, lon);
        self.buckets[self.first[row] + column].push(id);
    }

    pub fn candidates(&self, point: &SpherePoint, reach_m: f64) -> Vec<u32> {
        let (lat, lon) = point.to_latlon();
        let reach_deg = m::to_degrees(reach_m / self.radius_m);
        let low = self.row_of(if lat - reach_deg < -90.0 { -90.0 } else { lat - reach_deg });
        let high = self.row_of(if lat + reach_deg > 90.0 { 90.0 } else { lat + reach_deg });
        let mut found = Vec::new();
        for row in low..=high {
            let south = -90.0 + row as f64 * 180.0 / self.rows as f64;
            let north = south + 180.0 / self.rows as f64;
            // The widest point of the row decides how far the longitude reach stretches.
            let widest = if south.abs() > north.abs() { south.abs() } else { north.abs() };
            let cos = m::cos(m::to_radians(widest));
            let count = self.columns[row];
            let everything = cos <= 1.0e-9 || reach_deg / cos >= 180.0 || lat.abs() + reach_deg >= 90.0;
            if everything {
                for column in 0..count {
                    found.extend_from_slice(&self.buckets[self.first[row] + column]);
                }
                continue;
            }
            let stretch = reach_deg / cos;
            let west = self.column_of(row, wrap(lon - stretch));
            let east = self.column_of(row, wrap(lon + stretch));
            let mut column = west;
            loop {
                found.extend_from_slice(&self.buckets[self.first[row] + column]);
                if column == east {
                    break;
                }
                column = (column + 1) % count;
            }
        }
        found.sort_unstable();
        found.dedup();
        found
    }

    pub fn nearest(&self, point: &SpherePoint, positions: &[SpherePoint]) -> Option<u32> {
        if positions.is_empty() {
            return None;
        }
        let mut reach = core::f64::consts::PI * self.radius_m / self.rows as f64;
        loop {
            let mut best: Option<(f64, u32)> = None;
            for id in self.candidates(point, reach) {
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

    #[test]
    fn an_empty_index_has_no_nearest() {
        let index = BucketIndex::new(R, 100_000.0);
        assert_eq!(index.nearest(&SpherePoint::from_latlon(0.0, 0.0), &[]), None);
    }
}
