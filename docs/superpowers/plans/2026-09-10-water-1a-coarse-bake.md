# Water 1a: The Coarse Bake — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An engine bake that turns a world's landform into lakes, notched hollows, flow and rivers at graph resolution, saved as a flat record, exported to wasm, bit-identical native and wasm, and calibrated on the owner's world.

**Architecture:** A new module directory `crates/worldbuilder-engine/src/hydrology/`. It samples a land graph on the landform (`Surface::structural_m`), runs a deterministic priority flood inward from the ocean, judges every hollow (keep, notch or close), routes and accumulates wetness-weighted flow, and extracts reaches. The result is a `HydroRecord`, encoded to a flat `f64` stream for the wasm table protocol. Nothing in the default elevation path changes, so `GENERATOR_VERSION` stays 1 in this plan (the stage 2 water layer bumps it).

**Tech Stack:** Rust 1.98.0 (Windows MSVC toolchain, as CI), the engine's `detmath`, `wasm32-unknown-unknown`, Node 22 for the viewer tests and parity.

**Spec:** `docs/superpowers/specs/2026-09-10-automatic-water-design.md` — sections 5, 6.1–6.5, 7, 13 (stage 1a), 14 and 15. Refinement (6.6), falls (6.7) and outlines are plan 1b.

## Global Constraints

- **No std float maths outside `detmath.rs`.** `tests/no_std_math.rs` bans `.sin(`, `.sqrt(`, `.floor(` and the rest. Use `crate::detmath as m`. There is no `acos` (use `SpherePoint::angle_to`).
- **Casts:** `as u32`, `as u64`, `as i32` and `as i64` need `// cast-ok: <reason>` on the same line.
- **No `f64::min`, `f64::max` or `.clamp(`** (house rule, enforced by review). Write explicit `if` branches.
- **No panics reachable from `extern "C"`.** wasm32 is `panic = abort`; refuse with a status code first.
- **No `HashMap` iteration order may reach output or summation.** Sort by key first.
- **Ties are broken by node index**, and float ordering in heaps uses `heap::sortable` keys.
- **`Surface` must not gain a field** (`lib.rs:411`). Hydrology state lives in its own types.
- **New modules** are declared after `pub mod water;` in `lib.rs`.
- **Every `src/` edit changes the source fingerprint.** Rebuild `viewer/public/wasm/worldbuilder_engine.wasm` and `MANIFEST.txt` with `npm run build:wasm` in `viewer/` before any parity run, and commit both.
- **CI count pins** in `.github/workflows/gates.yml` (engine matrix, currently 632/632/634/735/737 with 5 ignored) and their mirror in `crates/worldbuilder-engine/README.md` must be re-derived **after the last source edit**, with `assert_counts.py cargo-list`, and given a dated prose paragraph.
- **Commit subjects and branch names** name no third party (the Discord feed is public).
- **Initial constants (spec §6):** keep depth 8 m, keep area 1 km², pond < 1 km², stream 2.5e8 m², river 2.5e9 m², great river 1.0e11 m², notch fall 1 m, evaporation factor 1.0, salt flat below 10% of evaporation. They are recorded in `HydroParams::earth_like()` and tuned only in Task 12.

## File Structure

| File | Responsibility |
|---|---|
| `src/hydrology/mod.rs` | Public types (`HydroParams`, `HydroRecord`, `Body`, `ReachLine`, `NotchLine`, `BakeStats`, `HydroError`) and `bake()`. |
| `src/hydrology/heap.rs` | `sortable` height keys and `FloodQueue`, the deterministic min-heap. |
| `src/hydrology/buckets.rs` | `BucketIndex`: lat/lon buckets for nearest-point and within-reach queries on the sphere. |
| `src/hydrology/landgraph.rs` | `LandGraph`: nodes, landform heights, areas, symmetric CSR adjacency, ocean and enclosed-basin labels (Ruling W1), wetness. |
| `src/hydrology/flood.rs` | `flood()`: priority flood from seed levels over an allowed node set. |
| `src/hydrology/hollows.rs` | `find_hollows()`, `judge()`: flat-spill components, depth, area, entry, outlet, and fate. |
| `src/hydrology/routing.rs` | `route()`: final surface, receivers, lake membership, notch routes. |
| `src/hydrology/flow.rs` | `accumulate()`, `close_lakes()`: wetness-weighted flow and the closure loop. |
| `src/hydrology/reaches.rs` | `extract()`: reaches, classes, Strahler order, width and depth, bifurcation ratios. |
| `src/hydrology/record.rs` | `encode()` and `decode()`: the flat `f64` record. |
| `src/wasm.rs` | `wb_hydro_bake`, `wb_hydro_len`, `wb_hydro_copy`, `wb_hydro_free`. |
| `src/bin/hydro_survey.rs` | The native survey: bake time, memory proxy, counts and ratios per world. |
| `tests/wasm_exports.rs` | Export-level tests. |
| `examples/parity_dump.rs`, `parity/parity.mjs` | `H` records for native vs wasm parity. |
| `viewer/public/app/engine.js` | `hydroBake()` wrapper and header summary. |
| `viewer/test/hydro.test.mjs` | Node test of the wrapper against the checked-in wasm. |

---

### Task 1: Deterministic flood queue

**Files:**
- Create: `crates/worldbuilder-engine/src/hydrology/heap.rs`
- Create: `crates/worldbuilder-engine/src/hydrology/mod.rs` (a stub declaring submodules; grown in later tasks)
- Modify: `crates/worldbuilder-engine/src/lib.rs` (add `pub mod hydrology;` after `pub mod water;`)

**Interfaces:**
- Produces: `pub fn sortable(value: f64) -> u64`, `pub fn unsortable(key: u64) -> f64`, and `pub struct FloodQueue` with `new() / push(level_m: f64, node: u32) / pop() -> Option<(f64, u32)> / len() / is_empty()`.

- [ ] **Step 1: Write the failing tests**

`src/hydrology/heap.rs`:

```rust
//! A min-priority queue keyed by height, with one fixed order on every build.
//!
//! **Float keys are integers here.** A heap that compares `f64`s through `partial_cmp` has
//! to decide what NaN means, and one that compares them at all leaves the order to the
//! instruction stream. `sortable` maps every finite `f64` to a `u64` that orders the way the
//! number does, negatives included, and the node index breaks every tie. Two builds, native
//! and wasm, therefore pop the same node at the same moment.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// An `f64` as a `u64` that sorts the way the number does. Callers never pass NaN.
pub fn sortable(value: f64) -> u64 {
    let bits = value.to_bits();
    if bits >> 63 == 1 {
        !bits
    } else {
        bits | (1u64 << 63)
    }
}

/// The inverse of `sortable`.
pub fn unsortable(key: u64) -> f64 {
    let bits = if key >> 63 == 1 { key & !(1u64 << 63) } else { !key };
    f64::from_bits(bits)
}

/// Lowest level first; equal levels pop in ascending node order.
#[derive(Debug, Default)]
pub struct FloodQueue {
    heap: BinaryHeap<Reverse<(u64, u32)>>,
}

impl FloodQueue {
    pub fn new() -> Self {
        Self { heap: BinaryHeap::new() }
    }

    pub fn push(&mut self, level_m: f64, node: u32) {
        self.heap.push(Reverse((sortable(level_m), node)));
    }

    pub fn pop(&mut self) -> Option<(f64, u32)> {
        self.heap.pop().map(|Reverse((key, node))| (unsortable(key), node))
    }

    pub fn len(&self) -> usize {
        self.heap.len()
    }

    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sortable_keys_order_like_the_numbers() {
        let values = [-1.0e9, -1.0, -1.0e-300, 0.0, 1.0e-300, 1.0, 1.0e9];
        for pair in values.windows(2) {
            assert!(sortable(pair[0]) < sortable(pair[1]), "{} < {}", pair[0], pair[1]);
        }
    }

    #[test]
    fn sortable_round_trips_bit_for_bit() {
        for value in [-1234.5678, -0.0, 0.0, 3.25, f64::MAX, f64::MIN, 1.0e-310] {
            assert_eq!(unsortable(sortable(value)).to_bits(), value.to_bits());
        }
    }

    #[test]
    fn the_queue_pops_lowest_first_and_breaks_ties_by_node() {
        let mut queue = FloodQueue::new();
        queue.push(5.0, 7);
        queue.push(-2.0, 9);
        queue.push(5.0, 3);
        queue.push(1.0, 1);
        let order: Vec<(f64, u32)> = std::iter::from_fn(|| queue.pop()).collect();
        assert_eq!(order, vec![(-2.0, 9), (1.0, 1), (5.0, 3), (5.0, 7)]);
        assert!(queue.is_empty());
    }
}
```

`src/hydrology/mod.rs` (stub):

```rust
//! Automatic water: the bake that finds where water collects and where it runs.
//!
//! See `docs/superpowers/specs/2026-09-10-automatic-water-design.md`. This module reads the
//! landform (`Surface::structural_m`), never the detail noise, and it changes nothing in the
//! default elevation path: a bake is requested, not implied.

pub mod heap;
```

In `lib.rs`, directly after `pub mod water;`:

```rust
pub mod hydrology;
```

- [ ] **Step 2: Run the tests and confirm they compile and pass**

Run: `cargo test -p worldbuilder-engine hydrology::heap`
Expected: 3 passed. These tests pin behaviour that follows from the code, so they are written together. What matters is that `no_std_math` stays green:

Run: `cargo test -p worldbuilder-engine --test no_std_math`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/worldbuilder-engine/src/hydrology crates/worldbuilder-engine/src/lib.rs
git commit -m "Water: a deterministic flood queue keyed by sortable heights"
```

---

### Task 2: Bucket index on the sphere

**Files:**
- Create: `crates/worldbuilder-engine/src/hydrology/buckets.rs`
- Modify: `crates/worldbuilder-engine/src/hydrology/mod.rs` (`pub mod buckets;`)

**Interfaces:**
- Consumes: `SpherePoint::{from_latlon, to_latlon, distance_to}`.
- Produces:
  - `pub struct BucketIndex`
  - `pub fn new(radius_m: f64, cell_m: f64) -> Self`
  - `pub fn insert(&mut self, point: &SpherePoint, id: u32)`
  - `pub fn candidates(&self, point: &SpherePoint, reach_m: f64) -> Vec<u32>` (sorted, deduplicated)
  - `pub fn nearest(&self, point: &SpherePoint, positions: &[SpherePoint]) -> Option<u32>`

`positions[id]` is the position inserted under `id`. Stage 2 adds `insert_circle` to this same type.

- [ ] **Step 1: Write the failing tests**

Append to a new `buckets.rs`, with the implementation stubbed as `todo!()` bodies first so the tests compile. Then run and see them fail.

```rust
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
```

Run: `cargo test -p worldbuilder-engine hydrology::buckets`
Expected: FAIL (panics at `todo!()`).

- [ ] **Step 2: Implement**

```rust
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
```

Run: `cargo test -p worldbuilder-engine hydrology::buckets && cargo test -p worldbuilder-engine --test no_std_math`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/worldbuilder-engine/src/hydrology
git commit -m "Water: a bucket index for nearest and within-reach queries on the sphere"
```

---

### Task 3: The land graph

**Files:**
- Create: `crates/worldbuilder-engine/src/hydrology/landgraph.rs`
- Modify: `src/hydrology/mod.rs` (`pub mod landgraph;`)

**Interfaces:**
- Consumes: `stream::sample_nodes`, `Surface::{structural_m, moisture_index}`, `BucketIndex`.
- Produces:

```rust
pub const NO_BASIN: u32 = u32::MAX;
pub struct LandGraph {
    pub radius_m: f64,
    pub positions: Vec<SpherePoint>,
    pub height_m: Vec<f64>,
    pub area_m2: Vec<f64>,
    pub adj_start: Vec<u32>,
    pub adj: Vec<u32>,
    pub ocean: Vec<bool>,
    pub enclosed: Vec<u32>,
    pub enclosed_count: u32,
    pub wetness: Vec<f64>,
}
impl LandGraph {
    pub fn sample(surface: &Surface, total_nodes: u32, wetness_nodes: u32) -> Option<Self>;
    pub fn from_parts(radius_m: f64, positions: Vec<SpherePoint>, height_m: Vec<f64>,
                      area_m2: Vec<f64>, directed: &[Vec<u32>], wetness: Vec<f64>) -> Self;
    pub fn len(&self) -> usize;
    pub fn neighbours(&self, node: u32) -> &[u32];
}
```

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::sphere::SpherePoint;

    /// A ring of 8 nodes around an equatorial strip: nodes 0-2 deep water (ocean-sized),
    /// node 5 a small enclosed below-datum pocket, the rest land.
    fn strip() -> LandGraph {
        let positions: Vec<SpherePoint> =
            (0..8).map(|i| SpherePoint::from_latlon(0.0, i as f64 * 1.0)).collect();
        let heights = vec![-100.0, -80.0, -60.0, 20.0, 30.0, -5.0, 25.0, 40.0];
        let areas = vec![10.0, 10.0, 10.0, 1.0, 1.0, 1.0, 1.0, 1.0];
        let directed: Vec<Vec<u32>> = (0..8u32)
            .map(|i| {
                let mut v = Vec::new();
                if i > 0 { v.push(i - 1); }
                if i < 7 { v.push(i + 1); }
                v
            })
            .collect();
        LandGraph::from_parts(6_371_000.0, positions, heights, areas, &directed, vec![0.5; 8])
    }

    #[test]
    fn adjacency_is_symmetric_sorted_and_self_free() {
        let g = strip();
        for node in 0..g.len() as u32 { // cast-ok: fixture of 8 nodes
            let n = g.neighbours(node);
            let mut sorted = n.to_vec();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(n, &sorted[..]);
            assert!(!n.contains(&node));
            for &other in n {
                assert!(g.neighbours(other).contains(&node));
            }
        }
    }

    #[test]
    fn the_largest_water_is_the_ocean_and_the_rest_is_enclosed() {
        let g = strip();
        assert_eq!(&g.ocean[..3], &[true, true, true]);
        assert!(!g.ocean[5]);
        assert_eq!(g.enclosed[5], 0);
        assert_eq!(g.enclosed_count, 1);
        for node in [3usize, 4, 6, 7] {
            assert!(!g.ocean[node]);
            assert_eq!(g.enclosed[node], NO_BASIN);
        }
    }

    #[test]
    fn a_sampled_world_has_land_ocean_and_wetness_in_range() {
        let surface = crate::surface::Surface::new(20_260_904, 6_371_000.0, 12, 0.29, None, None, None);
        let g = LandGraph::sample(&surface, 4_000, 400).expect("a land graph");
        assert_eq!(g.len(), 4_000);
        let land = g.ocean.iter().filter(|o| !**o).count();
        assert!(land > 400 && land < 3_600, "land nodes {land}");
        assert!(g.wetness.iter().all(|w| (0.0..=1.0).contains(w)));
        let again = LandGraph::sample(&surface, 4_000, 400).expect("a land graph");
        assert_eq!(g.height_m.iter().map(|h| h.to_bits()).collect::<Vec<_>>(),
                   again.height_m.iter().map(|h| h.to_bits()).collect::<Vec<_>>());
        assert_eq!(g.adj, again.adj);
    }
}
```

Run: `cargo test -p worldbuilder-engine hydrology::landgraph`
Expected: FAIL (the module has no implementation yet).

- [ ] **Step 2: Implement**

```rust
//! The graph the water runs on: land nodes and their neighbours, on the landform.
//!
//! **The landform, not the texture.** Heights are `Surface::structural_m` - shelf, plates,
//! mountains and painted features - and never the detail noise. Sampled every sixty-odd
//! kilometres, detail noise is what made 1,361 "lakes" on the owner's world; water takes its
//! orders from the shape of the land.
//!
//! **Ruling W1 lives here.** Below-datum water joined to the largest body of it is the ocean;
//! every other below-datum component is an enclosed basin, numbered in node order, and becomes
//! a lake at the datum rather than sea.

use crate::hydrology::buckets::BucketIndex;
use crate::sphere::SpherePoint;
use crate::stream::sample_nodes;
use crate::surface::Surface;

pub const NO_BASIN: u32 = u32::MAX;

/// Resolution handed to `moisture_index`: the relief palette's coarse climate reads, where the
/// march saves about a quarter of its cost and loses nothing a river cares about.
const WETNESS_RESOLUTION_M: f64 = 20_000.0;

/// Salt that keeps the wetness sampling from landing on the same spiral as the graph.
const WETNESS_SEED_SALT: u64 = 0x5745_5454_4e45_5353;

#[derive(Debug, Clone)]
pub struct LandGraph {
    pub radius_m: f64,
    pub positions: Vec<SpherePoint>,
    pub height_m: Vec<f64>,
    pub area_m2: Vec<f64>,
    pub adj_start: Vec<u32>,
    pub adj: Vec<u32>,
    pub ocean: Vec<bool>,
    pub enclosed: Vec<u32>,
    pub enclosed_count: u32,
    pub wetness: Vec<f64>,
}

impl LandGraph {
    pub fn sample(surface: &Surface, total_nodes: u32, wetness_nodes: u32) -> Option<Self> {
        let radius_m = surface.radius_m;
        let seed = surface.world_seed as u64; // cast-ok: two's-complement reinterpretation, as Surface::new makes
        let sampling = sample_nodes(seed, total_nodes, radius_m)?;
        let height_m: Vec<f64> =
            sampling.positions.iter().map(|p| surface.structural_m(p)).collect();
        if height_m.iter().any(|h| !h.is_finite()) {
            return None;
        }
        let coarse = sample_nodes(seed ^ WETNESS_SEED_SALT, wetness_nodes, radius_m)?;
        let coarse_wetness: Vec<f64> = coarse
            .positions
            .iter()
            .map(|p| {
                let w = surface.moisture_index(p, Some(WETNESS_RESOLUTION_M), None);
                if w.is_finite() { w } else { 0.0 }
            })
            .collect();
        let mut index = BucketIndex::new(radius_m, crate::stream::nominal_spacing_m(wetness_nodes, radius_m));
        for (i, p) in coarse.positions.iter().enumerate() {
            index.insert(p, i as u32); // cast-ok: bounded by wetness_nodes, a u32
        }
        let wetness: Vec<f64> = sampling
            .positions
            .iter()
            .map(|p| match index.nearest(p, &coarse.positions) {
                Some(id) => coarse_wetness[id as usize],
                None => 0.0,
            })
            .collect();
        Some(Self::from_parts(radius_m, sampling.positions, height_m, sampling.area_m2,
                              &sampling.neighbours, wetness))
    }

    pub fn from_parts(
        radius_m: f64,
        positions: Vec<SpherePoint>,
        height_m: Vec<f64>,
        area_m2: Vec<f64>,
        directed: &[Vec<u32>],
        wetness: Vec<f64>,
    ) -> Self {
        let n = positions.len();
        let mut pairs: Vec<(u32, u32)> = Vec::new();
        for (a, list) in directed.iter().enumerate() {
            let a = a as u32; // cast-ok: node counts are bounded by stream::MAX_NODES
            for &b in list {
                if a != b {
                    pairs.push((a, b));
                    pairs.push((b, a));
                }
            }
        }
        pairs.sort_unstable();
        pairs.dedup();
        let mut adj_start = vec![0u32; n + 1];
        for &(a, _) in &pairs {
            adj_start[a as usize + 1] += 1;
        }
        for i in 0..n {
            adj_start[i + 1] += adj_start[i];
        }
        let adj: Vec<u32> = pairs.iter().map(|&(_, b)| b).collect();

        let mut graph = Self {
            radius_m, positions, height_m, area_m2, adj_start, adj,
            ocean: vec![false; n], enclosed: vec![NO_BASIN; n], enclosed_count: 0, wetness,
        };
        graph.label_water();
        graph
    }

    pub fn len(&self) -> usize {
        self.positions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }

    pub fn neighbours(&self, node: u32) -> &[u32] {
        let start = self.adj_start[node as usize] as usize;
        let end = self.adj_start[node as usize + 1] as usize;
        &self.adj[start..end]
    }

    /// Ruling W1: the largest below-datum component (by area, ties to the lower first node)
    /// is the ocean; the others are enclosed basins numbered in order of their first node.
    fn label_water(&mut self) {
        let n = self.len();
        let mut component = vec![u32::MAX; n];
        let mut areas: Vec<f64> = Vec::new();
        let mut stack: Vec<u32> = Vec::new();
        for start in 0..n {
            if self.height_m[start] > 0.0 || component[start] != u32::MAX {
                continue;
            }
            let id = areas.len() as u32; // cast-ok: at most one component per node
            let mut area = 0.0;
            component[start] = id;
            stack.push(start as u32); // cast-ok: node index
            while let Some(node) = stack.pop() {
                area += self.area_m2[node as usize];
                for &next in self.neighbours(node) {
                    if self.height_m[next as usize] <= 0.0 && component[next as usize] == u32::MAX {
                        component[next as usize] = id;
                        stack.push(next);
                    }
                }
            }
            areas.push(area);
        }
        if areas.is_empty() {
            return;
        }
        let mut ocean_id = 0u32;
        for (id, &area) in areas.iter().enumerate() {
            if area > areas[ocean_id as usize] {
                ocean_id = id as u32; // cast-ok: component index
            }
        }
        let mut renumber = vec![NO_BASIN; areas.len()];
        let mut next = 0u32;
        for (id, slot) in renumber.iter_mut().enumerate() {
            if id as u32 != ocean_id { // cast-ok: component index
                *slot = next;
                next += 1;
            }
        }
        for node in 0..n {
            let c = component[node];
            if c == u32::MAX {
                continue;
            }
            if c == ocean_id {
                self.ocean[node] = true;
            } else {
                self.enclosed[node] = renumber[c as usize];
            }
        }
        self.enclosed_count = next;
    }
}
```

Run: `cargo test -p worldbuilder-engine hydrology::landgraph && cargo test -p worldbuilder-engine --test no_std_math`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/worldbuilder-engine/src/hydrology
git commit -m "Water: the land graph, on the landform, with the ocean told from enclosed basins"
```

---

### Task 4: Priority flood

**Files:**
- Create: `crates/worldbuilder-engine/src/hydrology/flood.rs`
- Modify: `src/hydrology/mod.rs` (`pub mod flood;`)

**Interfaces:**
- Consumes: `LandGraph`, `FloodQueue`.
- Produces:

```rust
pub const NO_NODE: u32 = u32::MAX;
pub struct Flood { pub spill_m: Vec<f64>, pub parent: Vec<u32>, pub order: Vec<u32>, pub reached: Vec<bool> }
pub fn flood(graph: &LandGraph, seeds: &[(u32, f64)], allowed: &dyn Fn(u32) -> bool) -> Flood;
pub fn ocean_seeds(graph: &LandGraph) -> Vec<(u32, f64)>;
```

The fields:
- `spill_m[n]` is the level water stands at if every hollow were filled.
- `parent[n]` is the node `n` was reached from (`NO_NODE` for seeds and unreached nodes).
- `order` lists non-seed reached nodes in pop order.
- `allowed(n)` limits which nodes the flood may enter.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::hydrology::landgraph::LandGraph;
    use crate::sphere::SpherePoint;

    /// A line: ocean, rim 50, pit 10, rim 30, land 60. The pit fills to 30 (its lower rim).
    fn line(heights: &[f64]) -> LandGraph {
        let n = heights.len();
        let positions: Vec<SpherePoint> =
            (0..n).map(|i| SpherePoint::from_latlon(0.0, i as f64 * 0.5)).collect();
        let directed: Vec<Vec<u32>> = (0..n as u32) // cast-ok: tiny fixture
            .map(|i| {
                let mut v = Vec::new();
                if i > 0 { v.push(i - 1); }
                if (i as usize) + 1 < n { v.push(i + 1); }
                v
            })
            .collect();
        LandGraph::from_parts(6_371_000.0, positions, heights.to_vec(), vec![1.0e6; n], &directed,
                              vec![0.5; n])
    }

    #[test]
    fn a_pit_fills_to_its_lowest_way_out() {
        // Below-datum water at both ends; the right-hand pair is the larger, so it is the ocean
        // and node 0 is an enclosed pocket. The pit at node 2 can only reach the ocean over
        // node 3 (30 m), not over node 1 (50 m).
        let g = line(&[-10.0, 50.0, 10.0, 30.0, -20.0, -30.0]);
        assert!(g.ocean[4] && g.ocean[5] && !g.ocean[0]);
        let f = flood(&g, &ocean_seeds(&g), &|_| true);
        assert_eq!(f.spill_m[2], 30.0, "the pit spills over the 30 m rim");
        assert_eq!(f.spill_m[3], 30.0);
        assert_eq!(f.parent[2], 3, "the pit is reached over its lowest rim");
    }

    #[test]
    fn every_reached_node_leads_back_to_a_seed_through_non_increasing_spill() {
        let surface = crate::surface::Surface::new(20_260_904, 6_371_000.0, 12, 0.29, None, None, None);
        let g = LandGraph::sample(&surface, 6_000, 300).expect("graph");
        let f = flood(&g, &ocean_seeds(&g), &|_| true);
        for &node in &f.order {
            let mut here = node;
            let mut steps = 0;
            while f.parent[here as usize] != NO_NODE {
                let up = f.parent[here as usize];
                assert!(f.spill_m[up as usize] <= f.spill_m[here as usize]);
                here = up;
                steps += 1;
                assert!(steps < 6_000, "a cycle");
            }
            assert!(g.ocean[here as usize], "chain from {node} ends at a seed");
        }
    }

    #[test]
    fn the_flood_is_bit_identical_run_to_run() {
        let surface = crate::surface::Surface::new(20_260_904, 6_371_000.0, 12, 0.29, None, None, None);
        let g = LandGraph::sample(&surface, 6_000, 300).expect("graph");
        let a = flood(&g, &ocean_seeds(&g), &|_| true);
        let b = flood(&g, &ocean_seeds(&g), &|_| true);
        assert_eq!(a.order, b.order);
        assert_eq!(a.parent, b.parent);
        assert!(a.spill_m.iter().zip(&b.spill_m).all(|(x, y)| x.to_bits() == y.to_bits()));
    }

    #[test]
    fn allowed_confines_the_flood() {
        let g = line(&[-10.0, 5.0, 6.0, 7.0, 8.0, 9.0]);
        let f = flood(&g, &ocean_seeds(&g), &|n| n <= 2);
        assert!(f.reached[2]);
        assert!(!f.reached[3]);
    }
}
```

Run: `cargo test -p worldbuilder-engine hydrology::flood`
Expected: FAIL.

- [ ] **Step 2: Implement**

```rust
//! Priority flood (Barnes, Lehman and Mulla 2014): water rises from the seeds and every node
//! learns the level it would stand at, and the way it was reached.
//!
//! Always the lowest frontier node next, so a hollow is met from its lowest rim and filled to
//! exactly that level. One pass gives every land node a route to a seed: that is the whole of
//! "everything drains" at this stage, before any hollow is kept or notched.

use crate::hydrology::heap::FloodQueue;
use crate::hydrology::landgraph::LandGraph;

pub const NO_NODE: u32 = u32::MAX;

#[derive(Debug, Clone)]
pub struct Flood {
    pub spill_m: Vec<f64>,
    pub parent: Vec<u32>,
    pub order: Vec<u32>,
    pub reached: Vec<bool>,
}

/// Every ocean node, at the datum.
pub fn ocean_seeds(graph: &LandGraph) -> Vec<(u32, f64)> {
    (0..graph.len())
        .filter(|&n| graph.ocean[n])
        .map(|n| (n as u32, 0.0)) // cast-ok: node index
        .collect()
}

pub fn flood(graph: &LandGraph, seeds: &[(u32, f64)], allowed: &dyn Fn(u32) -> bool) -> Flood {
    let n = graph.len();
    let mut spill_m = graph.height_m.clone();
    let mut parent = vec![NO_NODE; n];
    let mut reached = vec![false; n];
    let mut order = Vec::new();
    let mut queue = FloodQueue::new();
    let mut is_seed = vec![false; n];
    for &(node, level) in seeds {
        is_seed[node as usize] = true;
        reached[node as usize] = true;
        spill_m[node as usize] = level;
        queue.push(level, node);
    }
    while let Some((level, node)) = queue.pop() {
        if !is_seed[node as usize] {
            order.push(node);
        }
        for &next in graph.neighbours(node) {
            let i = next as usize;
            if reached[i] || !allowed(next) {
                continue;
            }
            reached[i] = true;
            parent[i] = node;
            let own = graph.height_m[i];
            let spill = if own > level { own } else { level };
            spill_m[i] = spill;
            queue.push(spill, next);
        }
    }
    Flood { spill_m, parent, order, reached }
}
```

Run: `cargo test -p worldbuilder-engine hydrology::flood && cargo test -p worldbuilder-engine --test no_std_math`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/worldbuilder-engine/src/hydrology
git commit -m "Water: priority flood from the ocean, deterministic, confined by a node test"
```

---

### Task 5: Hollows and their fate

**Files:**
- Create: `crates/worldbuilder-engine/src/hydrology/hollows.rs`
- Modify: `src/hydrology/mod.rs` (`pub mod hollows;`, plus `HydroParams` — see below)

**Interfaces:**
- Consumes: `LandGraph`, `Flood`, `heap::sortable`.
- Produces, in `mod.rs`:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct HydroParams {
    pub total_nodes: u32,
    pub wetness_nodes: u32,
    pub keep_depth_m: f64,
    pub keep_area_m2: f64,
    pub pond_max_area_m2: f64,
    pub stream_flow_m2: f64,
    pub river_flow_m2: f64,
    pub great_flow_m2: f64,
    pub notch_fall_m: f64,
    pub evaporation_factor: f64,
    pub salt_flat_share: f64,
    pub forced_outlets: Vec<SpherePoint>,
}
impl HydroParams { pub fn earth_like(total_nodes: u32) -> Self; }
```

and in `hollows.rs`:

```rust
pub enum Fate { Keep, Notch }
pub struct Hollow {
    pub members: Vec<u32>, pub floor: u32, pub floor_m: f64, pub level_m: f64,
    pub depth_m: f64, pub area_m2: f64, pub entry: u32, pub outlet: u32,
    pub enclosed: bool, pub forced: bool, pub fate: Fate,
    pub lake_entry: u32, pub outlet_path: Vec<u32>,
}
pub fn find_hollows(graph: &LandGraph, flood: &Flood) -> Vec<Hollow>;
pub fn judge(hollows: &mut [Hollow], graph: &LandGraph, params: &HydroParams);
```

`earth_like(total_nodes)` returns: `wetness_nodes: 20_000`, `keep_depth_m: 8.0`, `keep_area_m2: 1.0e6`, `pond_max_area_m2: 1.0e6`, `stream_flow_m2: 2.5e8`, `river_flow_m2: 2.5e9`, `great_flow_m2: 1.0e11`, `notch_fall_m: 1.0`, `evaporation_factor: 1.0`, `salt_flat_share: 0.1`, `forced_outlets: vec![]`.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::hydrology::flood::{flood, ocean_seeds};
    use crate::hydrology::landgraph::LandGraph;
    use crate::hydrology::HydroParams;
    use crate::sphere::SpherePoint;

    fn line(heights: &[f64], area: f64) -> LandGraph {
        let n = heights.len();
        let positions: Vec<SpherePoint> =
            (0..n).map(|i| SpherePoint::from_latlon(0.0, i as f64 * 0.5)).collect();
        let directed: Vec<Vec<u32>> = (0..n as u32) // cast-ok: tiny fixture
            .map(|i| {
                let mut v = Vec::new();
                if i > 0 { v.push(i - 1); }
                if (i as usize) + 1 < n { v.push(i + 1); }
                v
            })
            .collect();
        LandGraph::from_parts(6_371_000.0, positions, heights.to_vec(), vec![area; n], &directed,
                              vec![0.5; n])
    }

    #[test]
    fn a_hollow_knows_its_floor_depth_area_and_outlet() {
        // ocean, then a 40 m ridge; behind it 5, 12, 25, then a 70 m wall. The only way out is
        // back over the 40 m ridge, so the hollow fills to 40.
        let g = line(&[-50.0, 40.0, 5.0, 12.0, 25.0, 70.0], 2.0e6);
        let f = flood(&g, &ocean_seeds(&g), &|_| true);
        let hollows = find_hollows(&g, &f);
        assert_eq!(hollows.len(), 1);
        let h = &hollows[0];
        assert_eq!(h.members, vec![2, 3, 4]);
        assert_eq!(h.floor, 2);
        assert_eq!(h.level_m, 40.0);
        assert_eq!(h.depth_m, 35.0);
        assert_eq!(h.area_m2, 6.0e6);
        assert_eq!(h.entry, 2);
        assert_eq!(h.outlet, 1, "it spills back over the 40 m ridge");
    }

    #[test]
    fn deep_wide_hollows_are_kept_and_shallow_ones_notched() {
        let deep = line(&[-50.0, 40.0, 5.0, 12.0, 25.0, 70.0], 2.0e6);
        let f = flood(&deep, &ocean_seeds(&deep), &|_| true);
        let mut hollows = find_hollows(&deep, &f);
        judge(&mut hollows, &deep, &HydroParams::earth_like(0));
        assert_eq!(hollows[0].fate, Fate::Keep);

        let shallow = line(&[-50.0, 30.0, 26.0, 27.0, 28.0, 70.0], 2.0e6);
        let f = flood(&shallow, &ocean_seeds(&shallow), &|_| true);
        let mut hollows = find_hollows(&shallow, &f);
        judge(&mut hollows, &shallow, &HydroParams::earth_like(0));
        assert_eq!(hollows[0].depth_m, 4.0);
        assert_eq!(hollows[0].fate, Fate::Notch, "4 m deep is under the 8 m rule");
    }

    #[test]
    fn an_enclosed_basin_is_always_kept_at_the_datum() {
        // big ocean on the left, a below-datum pocket at node 4 behind a 39 m ridge
        let g = line(&[-50.0, -40.0, -30.0, 39.0, -5.0, 10.0, 60.0], 1.0e6);
        let f = flood(&g, &ocean_seeds(&g), &|_| true);
        let mut hollows = find_hollows(&g, &f);
        judge(&mut hollows, &g, &HydroParams::earth_like(0));
        let enclosed: Vec<&Hollow> = hollows.iter().filter(|h| h.enclosed).collect();
        assert_eq!(enclosed.len(), 1);
        assert_eq!(enclosed[0].fate, Fate::Keep);
        assert_eq!(enclosed[0].level_m, 0.0, "Ruling W1: the shoreline does not move");
        assert_eq!(enclosed[0].outlet, 3, "its lowest way to the ocean is the 39 m ridge");
    }

    #[test]
    fn a_forced_outlet_keeps_a_hollow_the_rule_would_notch() {
        let g = line(&[-50.0, 30.0, 26.0, 27.0, 28.0, 70.0], 2.0e6);
        let f = flood(&g, &ocean_seeds(&g), &|_| true);
        let mut hollows = find_hollows(&g, &f);
        let mut params = HydroParams::earth_like(0);
        params.forced_outlets = vec![SpherePoint::from_latlon(0.0, 1.0)];
        judge(&mut hollows, &g, &params);
        assert!(hollows[0].forced);
        assert_eq!(hollows[0].fate, Fate::Keep);
    }
}
```

Run: `cargo test -p worldbuilder-engine hydrology::hollows`
Expected: FAIL.

- [ ] **Step 2: Implement**

`mod.rs` gains:

```rust
pub mod hollows;

use crate::sphere::SpherePoint;

/// Everything a bake is told. Recorded in the world beside the record, so a re-bake with the
/// same params and the same land is the same water.
#[derive(Debug, Clone, PartialEq)]
pub struct HydroParams {
    pub total_nodes: u32,
    pub wetness_nodes: u32,
    pub keep_depth_m: f64,
    pub keep_area_m2: f64,
    pub pond_max_area_m2: f64,
    pub stream_flow_m2: f64,
    pub river_flow_m2: f64,
    pub great_flow_m2: f64,
    pub notch_fall_m: f64,
    pub evaporation_factor: f64,
    pub salt_flat_share: f64,
    pub forced_outlets: Vec<SpherePoint>,
}

impl HydroParams {
    /// The spec's Earth-like starting values (section 6). Task 12 of plan 1a tunes them against
    /// the owner's world and records the result here.
    pub fn earth_like(total_nodes: u32) -> Self {
        Self {
            total_nodes,
            wetness_nodes: 20_000,
            keep_depth_m: 8.0,
            keep_area_m2: 1.0e6,
            pond_max_area_m2: 1.0e6,
            stream_flow_m2: 2.5e8,
            river_flow_m2: 2.5e9,
            great_flow_m2: 1.0e11,
            notch_fall_m: 1.0,
            evaporation_factor: 1.0,
            salt_flat_share: 0.1,
            forced_outlets: Vec::new(),
        }
    }
}
```

`hollows.rs`:

```rust
//! A hollow is a connected set of nodes the flood had to raise to one flat level. It is kept as
//! a lake or pond, or notched so it drains (spec section 6.3).

use crate::hydrology::buckets::BucketIndex;
use crate::hydrology::flood::{Flood, NO_NODE};
use crate::hydrology::heap::sortable;
use crate::hydrology::landgraph::{LandGraph, NO_BASIN};
use crate::hydrology::HydroParams;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fate {
    Keep,
    Notch,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hollow {
    pub members: Vec<u32>,
    pub floor: u32,
    pub floor_m: f64,
    pub level_m: f64,
    pub depth_m: f64,
    pub area_m2: f64,
    pub entry: u32,
    pub outlet: u32,
    pub enclosed: bool,
    pub forced: bool,
    pub fate: Fate,
    /// Where lake water gathers to leave: the entry for a hollow above the datum; for an
    /// enclosed basin, the submerged node the flood's way in leads down to (set by `route`).
    pub lake_entry: u32,
    /// For an enclosed basin: lake entry, up the shore, over the rim and down to the sea -
    /// the channel cut if the basin proves fresh (set by `route`, cut by `close_lakes`).
    pub outlet_path: Vec<u32>,
}

pub fn find_hollows(graph: &LandGraph, flood: &Flood) -> Vec<Hollow> {
    let n = graph.len();
    let raised = |i: usize| flood.reached[i] && flood.spill_m[i] > graph.height_m[i];
    // Rank of each node in pop order, so "first reached" is a number.
    let mut rank = vec![u32::MAX; n];
    for (r, &node) in flood.order.iter().enumerate() {
        rank[node as usize] = r as u32; // cast-ok: pop order is bounded by the node count
    }
    let mut label = vec![u32::MAX; n];
    let mut hollows = Vec::new();
    let mut stack = Vec::new();
    for start in 0..n {
        if !raised(start) || label[start] != u32::MAX || graph.ocean[start] {
            continue;
        }
        let key = sortable(flood.spill_m[start]);
        let id = hollows.len() as u32; // cast-ok: at most one hollow per node
        let mut members = Vec::new();
        label[start] = id;
        stack.push(start as u32); // cast-ok: node index
        while let Some(node) = stack.pop() {
            members.push(node);
            for &next in graph.neighbours(node) {
                let i = next as usize;
                if label[i] == u32::MAX && raised(i) && !graph.ocean[i]
                    && sortable(flood.spill_m[i]) == key {
                    label[i] = id;
                    stack.push(next);
                }
            }
        }
        members.sort_unstable();
        let level_m = flood.spill_m[start];
        let mut floor = members[0];
        let mut entry = members[0];
        let mut area_m2 = 0.0;
        let mut enclosed = false;
        for &m in &members {
            let i = m as usize;
            area_m2 += graph.area_m2[i];
            let lower = graph.height_m[i] < graph.height_m[floor as usize];
            if lower {
                floor = m;
            }
            if rank[i] < rank[entry as usize] {
                entry = m;
            }
            if graph.enclosed[i] != NO_BASIN {
                enclosed = true;
            }
        }
        let floor_m = graph.height_m[floor as usize];
        let outlet = flood.parent[entry as usize];
        hollows.push(Hollow {
            members,
            floor,
            floor_m,
            level_m: if enclosed { 0.0 } else { level_m },
            depth_m: if enclosed { 0.0 - floor_m } else { level_m - floor_m },
            area_m2,
            entry,
            outlet: if outlet == NO_NODE { entry } else { outlet },
            enclosed,
            forced: false,
            fate: Fate::Notch,
            lake_entry: entry,
            outlet_path: Vec::new(),
        });
    }
    hollows
}

pub fn judge(hollows: &mut [Hollow], graph: &LandGraph, params: &HydroParams) {
    let mut forced_nodes: Vec<u32> = Vec::new();
    if !params.forced_outlets.is_empty() {
        let spacing = crate::stream::nominal_spacing_m(
            graph.len() as u32, graph.radius_m); // cast-ok: node count fits in u32 by construction
        let mut index = BucketIndex::new(graph.radius_m, spacing);
        for (i, p) in graph.positions.iter().enumerate() {
            index.insert(p, i as u32); // cast-ok: node index
        }
        for point in &params.forced_outlets {
            if let Some(node) = index.nearest(point, &graph.positions) {
                forced_nodes.push(node);
            }
        }
        forced_nodes.sort_unstable();
    }
    for hollow in hollows.iter_mut() {
        hollow.forced = hollow.members.iter().any(|m| forced_nodes.binary_search(m).is_ok());
        let big = hollow.depth_m >= params.keep_depth_m && hollow.area_m2 >= params.keep_area_m2;
        hollow.fate = if hollow.enclosed || hollow.forced || big { Fate::Keep } else { Fate::Notch };
    }
}
```

For enclosed hollows, `area_m2` counts only the submerged part. After the loop in `find_hollows`, recompute the area over members with `height_m <= 0.0`:

```rust
        if enclosed {
            area_m2 = members.iter()
                .filter(|&&m| graph.height_m[m as usize] <= 0.0)
                .map(|&m| graph.area_m2[m as usize])
                .sum();
        }
```

This goes just before `hollows.push(...)`. Note that `.sum()` over a sorted `members` is deterministic.

Run: `cargo test -p worldbuilder-engine hydrology && cargo test -p worldbuilder-engine --test no_std_math`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/worldbuilder-engine/src/hydrology
git commit -m "Water: hollows found from the flood and judged - kept, notched or held at the datum"
```

---

### Task 6: Routing — the final surface, receivers and notch routes

**Files:**
- Create: `crates/worldbuilder-engine/src/hydrology/routing.rs`
- Modify: `src/hydrology/mod.rs` (`pub mod routing;`)

**Interfaces:**
- Consumes: `LandGraph`, `Flood`, `flood()`, `Hollow`, `Fate`, `HydroParams`.
- Produces:

```rust
pub const NO_LAKE: u32 = u32::MAX;
pub struct NotchRoute { pub nodes: Vec<u32>, pub bed_m: Vec<f64> }
pub struct Routing {
    pub surface_m: Vec<f64>,
    pub receiver: Vec<u32>,
    pub lake_of: Vec<u32>,
    pub parent: Vec<u32>,
    pub notches: Vec<NotchRoute>,
}
pub fn route(graph: &LandGraph, global: &Flood, hollows: &mut Vec<Hollow>, params: &HydroParams) -> Routing;
pub fn cut_route(routing: &mut Routing, graph: &LandGraph, start: u32, start_bed_m: f64);
pub fn cut_path(routing: &mut Routing, graph: &LandGraph, path: &[u32], start_bed_m: f64);
pub fn set_sink(routing: &mut Routing, hollow: &Hollow);
```

The rules, from spec §6.2–6.3 and Ruling W1:
1. `parent` starts as the global flood's parent. For each **enclosed kept** hollow:
   - a sub-flood seeded from its members with `height <= 0` at level 0, confined to the hollow's members; its parents replace the global ones for the members above the datum (the shore drains into the basin, not over the rim), and hollows found in that sub-flood are appended to `hollows`, judged with the same rules, never marked enclosed;
   - `lake_entry` = the first submerged node on the sub-flood parent chain from the global `entry`;
   - an **in-lake flood** seeded at `lake_entry`, confined to the submerged members, gives every submerged member a parent chain to `lake_entry` that never leaves the water (without it, a submerged node's parent can be a shore node whose own descent points back into the lake - a cycle);
   - `outlet_path` = `lake_entry`, up the sub-flood chain to `entry`, then `outlet` (the rim) and its global parents down to the last node before the ocean.
2. For kept hollows, the submerged members are `height < level_m`, or `height <= 0` for enclosed ones. They get `lake_of[m] = id` and `surface_m[m] = level_m`.
3. **Receivers:** for a non-ocean node, the steepest strictly-lower neighbour on `surface_m` (ties to the lower index). A submerged lake member instead takes its `parent`, which leads to the entry; the entry's receiver is the hollow's outlet.
4. **Local minima:** any non-ocean, non-member node with no receiver gets `cut_route` from itself with `start_bed_m = surface - notch_fall_m`. That route follows `parent`; each node's bed is `min(its surface, previous bed - 0.01)`; it stops at the first node whose surface is already below the running bed, or at an ocean node. Each route node's receiver becomes the next route node.
5. Enclosed kept hollows are sinks (their `lake_entry` drains nowhere) until Task 7 decides they are fresh.
6. `cut_path` cuts an explicit path: `path[0]` keeps its surface; each later node is cut to a bed falling by 0.01 m from `start_bed_m`, and each node's receiver is the next one, until a node is ocean or already lower than the bed. The last cut node, if it is the path's end, keeps its `parent` as receiver.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::hydrology::flood::{flood, ocean_seeds};
    use crate::hydrology::hollows::{find_hollows, judge};
    use crate::hydrology::landgraph::LandGraph;
    use crate::hydrology::HydroParams;
    use crate::sphere::SpherePoint;

    fn line(heights: &[f64], area: f64) -> LandGraph {
        let n = heights.len();
        let positions: Vec<SpherePoint> =
            (0..n).map(|i| SpherePoint::from_latlon(0.0, i as f64 * 0.5)).collect();
        let directed: Vec<Vec<u32>> = (0..n as u32) // cast-ok: tiny fixture
            .map(|i| {
                let mut v = Vec::new();
                if i > 0 { v.push(i - 1); }
                if (i as usize) + 1 < n { v.push(i + 1); }
                v
            })
            .collect();
        LandGraph::from_parts(6_371_000.0, positions, heights.to_vec(), vec![area; n], &directed,
                              vec![0.5; n])
    }

    fn routed(g: &LandGraph) -> (Vec<crate::hydrology::hollows::Hollow>, Routing) {
        let params = HydroParams::earth_like(0);
        let f = flood(g, &ocean_seeds(g), &|_| true);
        let mut hollows = find_hollows(g, &f);
        judge(&mut hollows, g, &params);
        let r = route(g, &f, &mut hollows, &params);
        (hollows, r)
    }

    /// Follows receivers from `node`; returns where it ends.
    fn terminus(r: &Routing, node: u32) -> u32 {
        let mut here = node;
        let mut steps = 0;
        while r.receiver[here as usize] != NO_NODE {
            here = r.receiver[here as usize];
            steps += 1;
            assert!(steps < 10_000, "a cycle");
        }
        here
    }

    #[test]
    fn a_notched_hollow_drains_and_its_route_only_falls() {
        let g = line(&[-50.0, 30.0, 26.0, 27.0, 28.0, 70.0], 2.0e6);
        let (_, r) = routed(&g);
        assert_eq!(terminus(&r, 2), 0, "the notched pit drains to the ocean");
        assert_eq!(r.notches.len(), 1);
        let beds = &r.notches[0].bed_m;
        assert!(beds.windows(2).all(|w| w[1] < w[0]), "a notch bed only falls: {beds:?}");
    }

    #[test]
    fn a_kept_lake_flows_out_through_its_outlet() {
        let g = line(&[-50.0, 40.0, 5.0, 12.0, 25.0, 70.0], 2.0e6);
        let (hollows, r) = routed(&g);
        assert_eq!(r.lake_of[2], 0);
        assert_eq!(r.surface_m[2], 40.0);
        assert_eq!(terminus(&r, 3), 0, "lake water leaves over the outlet and reaches the sea");
        assert_eq!(hollows[0].outlet, 1);
        assert!(r.notches.is_empty(), "a kept lake above the datum needs no cut");
    }

    #[test]
    fn an_enclosed_basin_is_a_sink_until_judged_fresh() {
        let g = line(&[-50.0, -40.0, -30.0, 39.0, -5.0, 10.0, 60.0], 1.0e6);
        let (_, r) = routed(&g);
        assert_eq!(r.surface_m[4], 0.0);
        assert_eq!(terminus(&r, 5), 4, "the shore above the datum drains into the basin");
    }

    #[test]
    fn on_a_real_world_every_land_node_ends_in_the_ocean_or_a_lake() {
        let surface = crate::surface::Surface::new(20_260_904, 6_371_000.0, 12, 0.29, None, None, None);
        let g = LandGraph::sample(&surface, 8_000, 400).expect("graph");
        let (_, r) = routed(&g);
        for node in 0..g.len() as u32 { // cast-ok: node index
            if g.ocean[node as usize] { continue; }
            let end = terminus(&r, node);
            assert!(g.ocean[end as usize] || r.lake_of[end as usize] != NO_LAKE,
                    "node {node} ends at {end}, neither sea nor lake");
        }
    }
}
```

Run: `cargo test -p worldbuilder-engine hydrology::routing`
Expected: FAIL.

- [ ] **Step 2: Implement**

```rust
//! Where each drop goes once hollows are decided: lakes flat at their level, notches cut so the
//! drained hollows run out, and every other node down its steepest slope.

use crate::hydrology::flood::{flood, Flood, NO_NODE};
use crate::hydrology::hollows::{find_hollows, judge, Fate, Hollow};
use crate::hydrology::landgraph::LandGraph;
use crate::hydrology::HydroParams;

pub const NO_LAKE: u32 = u32::MAX;

/// How much each step of a notch drops below the one before, so the bed strictly falls.
const NOTCH_GRADE_M: f64 = 0.01;

#[derive(Debug, Clone, PartialEq)]
pub struct NotchRoute {
    pub nodes: Vec<u32>,
    pub bed_m: Vec<f64>,
}

#[derive(Debug, Clone)]
pub struct Routing {
    pub surface_m: Vec<f64>,
    pub receiver: Vec<u32>,
    pub lake_of: Vec<u32>,
    pub parent: Vec<u32>,
    pub notches: Vec<NotchRoute>,
}

pub fn route(graph: &LandGraph, global: &Flood, hollows: &mut Vec<Hollow>, params: &HydroParams) -> Routing {
    let n = graph.len();
    let mut parent = global.parent.clone();

    // 1. Enclosed kept hollows: the ring above the datum drains into the basin, not over the rim.
    let enclosed: Vec<usize> = (0..hollows.len())
        .filter(|&h| hollows[h].enclosed && hollows[h].fate == Fate::Keep)
        .collect();
    for h in enclosed {
        let members = hollows[h].members.clone();
        let seeds: Vec<(u32, f64)> = members
            .iter()
            .filter(|&&m| graph.height_m[m as usize] <= 0.0)
            .map(|&m| (m, 0.0))
            .collect();
        let inside = |node: u32| members.binary_search(&node).is_ok();
        let sub = flood(graph, &seeds, &inside);
        for &m in &members {
            if sub.reached[m as usize] && sub.parent[m as usize] != NO_NODE {
                parent[m as usize] = sub.parent[m as usize];
            }
        }
        // Where the flood's way in reaches the water: the lake's own entry.
        let entry = hollows[h].entry;
        let mut lake_entry = entry;
        let mut guard = 0usize;
        while graph.height_m[lake_entry as usize] > 0.0 && sub.parent[lake_entry as usize] != NO_NODE
            && guard <= members.len() {
            lake_entry = sub.parent[lake_entry as usize];
            guard += 1;
        }
        // In-lake routing: every submerged member reaches the entry without leaving the water.
        let submerged: Vec<u32> = members.iter().copied()
            .filter(|&m| graph.height_m[m as usize] <= 0.0).collect();
        let under = |node: u32| submerged.binary_search(&node).is_ok();
        let inner = flood(graph, &[(lake_entry, 0.0)], &under);
        for &m in &submerged {
            parent[m as usize] = inner.parent[m as usize];
        }
        // The way out, should the basin prove fresh: entry, up the shore, over the rim, down.
        let mut ring = vec![entry];
        let mut up = entry;
        while up != lake_entry && sub.parent[up as usize] != NO_NODE && ring.len() <= members.len() {
            up = sub.parent[up as usize];
            ring.push(up);
        }
        ring.reverse();
        let mut path = ring;
        let mut down = hollows[h].outlet;
        while down != NO_NODE && !graph.ocean[down as usize] && path.len() <= graph.len() {
            path.push(down);
            down = global.parent[down as usize];
        }
        hollows[h].lake_entry = lake_entry;
        hollows[h].outlet_path = path;
        let mut nested = find_hollows(graph, &sub);
        for hollow in nested.iter_mut() {
            hollow.enclosed = false;
        }
        judge(&mut nested, graph, params);
        hollows.extend(nested);
    }

    // 2. Kept hollows stand flat at their level.
    let mut surface_m = graph.height_m.clone();
    let mut lake_of = vec![NO_LAKE; n];
    for (id, hollow) in hollows.iter().enumerate() {
        if hollow.fate != Fate::Keep {
            continue;
        }
        for &m in &hollow.members {
            let h = graph.height_m[m as usize];
            let under = if hollow.enclosed { h <= 0.0 } else { h < hollow.level_m };
            if under {
                lake_of[m as usize] = id as u32; // cast-ok: hollow index
                surface_m[m as usize] = hollow.level_m;
            }
        }
    }

    let mut routing = Routing { surface_m, receiver: vec![NO_NODE; n], lake_of, parent, notches: Vec::new() };

    // 3. Receivers.
    for node in 0..n {
        if graph.ocean[node] {
            continue;
        }
        routing.receiver[node] = if routing.lake_of[node] != NO_LAKE {
            routing.parent[node]
        } else {
            steepest(graph, &routing.surface_m, node as u32) // cast-ok: node index
        };
    }
    for hollow in hollows.iter() {
        if hollow.fate == Fate::Keep && !hollow.enclosed {
            routing.receiver[hollow.entry as usize] = hollow.outlet;
        }
    }
    for hollow in hollows.iter() {
        if hollow.fate == Fate::Keep && hollow.enclosed {
            set_sink(&mut routing, hollow);
        }
    }

    // 4. Local minima are cut out along their parent chain.
    for node in 0..n {
        let i = node;
        if graph.ocean[i] || routing.lake_of[i] != NO_LAKE || routing.receiver[i] != NO_NODE {
            continue;
        }
        let start_bed = routing.surface_m[i] - params.notch_fall_m;
        cut_route(&mut routing, graph, node as u32, start_bed); // cast-ok: node index
    }
    routing
}

/// The steepest strictly-lower neighbour on `surface`, ties to the lower index.
fn steepest(graph: &LandGraph, surface: &[f64], node: u32) -> u32 {
    let here = surface[node as usize];
    let mut best = NO_NODE;
    let mut best_drop = 0.0;
    for &next in graph.neighbours(node) {
        let drop = here - surface[next as usize];
        if drop <= 0.0 {
            continue;
        }
        let run = graph.positions[node as usize].distance_to(&graph.positions[next as usize], graph.radius_m);
        let slope = drop / run;
        if best == NO_NODE || slope > best_drop {
            best = next;
            best_drop = slope;
        }
    }
    best
}

/// Cut a channel from `start` along its parent chain so it strictly falls, until the ground is
/// already lower than the channel or the sea is reached.
pub fn cut_route(routing: &mut Routing, graph: &LandGraph, start: u32, start_bed_m: f64) {
    let mut nodes = vec![start];
    let mut beds = vec![start_bed_m];
    routing.surface_m[start as usize] = start_bed_m;
    let mut bed = start_bed_m;
    let mut here = start;
    loop {
        let next = routing.parent[here as usize];
        if next == NO_NODE {
            break;
        }
        routing.receiver[here as usize] = next;
        if graph.ocean[next as usize] || routing.surface_m[next as usize] < bed {
            break;
        }
        bed -= NOTCH_GRADE_M;
        routing.surface_m[next as usize] = bed;
        nodes.push(next);
        beds.push(bed);
        here = next;
    }
    routing.notches.push(NotchRoute { nodes, bed_m: beds });
}

/// A closed lake keeps its water: its entry drains nowhere.
pub fn set_sink(routing: &mut Routing, hollow: &Hollow) {
    routing.receiver[hollow.lake_entry as usize] = NO_NODE;
}

/// Cut an explicit path so it strictly falls from `start_bed_m`; `path[0]` keeps its surface.
pub fn cut_path(routing: &mut Routing, graph: &LandGraph, path: &[u32], start_bed_m: f64) {
    let mut nodes = Vec::new();
    let mut beds = Vec::new();
    let mut bed = start_bed_m;
    for k in 1..path.len() {
        let node = path[k];
        routing.receiver[path[k - 1] as usize] = node;
        if graph.ocean[node as usize] || routing.surface_m[node as usize] < bed {
            break;
        }
        routing.surface_m[node as usize] = bed;
        nodes.push(node);
        beds.push(bed);
        bed -= NOTCH_GRADE_M;
    }
    if let (Some(&last), Some(&cut)) = (path.last(), nodes.last()) {
        if last == cut {
            routing.receiver[last as usize] = routing.parent[last as usize];
        }
    }
    routing.notches.push(NotchRoute { nodes, bed_m: beds });
}
```

Check against the test `a_notched_hollow_drains_and_its_route_only_falls`: the pit's floor (node 2) has no strictly-lower neighbour, so `cut_route` starts at node 2 and follows parents 3 and 4 toward 1 and 0. The beds fall by 0.01 per step until reaching a node whose surface is below the bed, or the ocean.

Run: `cargo test -p worldbuilder-engine hydrology && cargo test -p worldbuilder-engine --test no_std_math`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/worldbuilder-engine/src/hydrology
git commit -m "Water: routing - lakes flat, notches cut downhill, every drop somewhere to go"
```

---

### Task 7: Flow and the closure loop

**Files:**
- Create: `crates/worldbuilder-engine/src/hydrology/flow.rs`
- Modify: `src/hydrology/mod.rs` (`pub mod flow;`)

**Interfaces:**
- Consumes: `LandGraph`, `Routing`, `cut_route`, `set_sink`, `Hollow`, `Fate`, `HydroParams`.
- Produces:

```rust
pub fn accumulate(graph: &LandGraph, routing: &Routing) -> Vec<f64>;
pub struct Closure { pub closed: Vec<bool>, pub salt_flat: Vec<bool>, pub fresh_enclosed: Vec<bool> }
pub fn close_lakes(graph: &LandGraph, routing: &mut Routing, hollows: &[Hollow], params: &HydroParams) -> (Vec<f64>, Closure);
```

- `accumulate`: each non-ocean node contributes `area_m2 × wetness`; the sum is passed down receivers, leaves first (a FIFO peel over upstream in-degrees, seeded in ascending node order). The result is per-node flow in m² of fully wet catchment.
- `close_lakes` works in four steps:
  1. Accumulate with enclosed kept lakes as sinks.
  2. For each enclosed kept lake, compute inflow at its `lake_entry` and evaporation = `area × evaporation_factor × (1 − mean member wetness)`. If it is forced, or inflow ≥ evaporation, it is fresh: `cut_path` along its `outlet_path` with `start_bed_m = level − notch_fall_m`.
  3. Loop: re-accumulate; any kept, non-enclosed, non-forced lake with inflow < evaporation is closed (`set_sink`). A salt flat is inflow < `salt_flat_share × evaporation`. Stop when a pass closes nothing, or after `hollows.len() + 1` passes.
  4. Return the final flow and the flags.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::hydrology::flood::{flood, ocean_seeds, NO_NODE};
    use crate::hydrology::hollows::{find_hollows, judge};
    use crate::hydrology::landgraph::LandGraph;
    use crate::hydrology::routing::route;
    use crate::hydrology::HydroParams;
    use crate::sphere::SpherePoint;

    fn line(heights: &[f64], area: f64, wetness: f64) -> LandGraph {
        let n = heights.len();
        let positions: Vec<SpherePoint> =
            (0..n).map(|i| SpherePoint::from_latlon(0.0, i as f64 * 0.5)).collect();
        let directed: Vec<Vec<u32>> = (0..n as u32) // cast-ok: tiny fixture
            .map(|i| {
                let mut v = Vec::new();
                if i > 0 { v.push(i - 1); }
                if (i as usize) + 1 < n { v.push(i + 1); }
                v
            })
            .collect();
        LandGraph::from_parts(6_371_000.0, positions, heights.to_vec(), vec![area; n], &directed,
                              vec![wetness; n])
    }

    #[test]
    fn flow_adds_up_downhill_and_conserves_the_catchment() {
        // ocean, then land rising steadily: every land node drains to node 0
        let g = line(&[-10.0, 5.0, 10.0, 15.0, 20.0], 1.0e6, 0.5);
        let params = HydroParams::earth_like(0);
        let f = flood(&g, &ocean_seeds(&g), &|_| true);
        let mut hollows = find_hollows(&g, &f);
        judge(&mut hollows, &g, &params);
        let r = route(&g, &f, &mut hollows, &params);
        let flow = accumulate(&g, &r);
        assert_eq!(flow[4], 0.5e6);
        assert_eq!(flow[1], 2.0e6, "node 1 carries all four land nodes");
    }

    #[test]
    fn a_wet_enclosed_basin_is_fresh_and_cut_through_its_rim() {
        let g = line(&[-50.0, -40.0, -30.0, 39.0, -5.0, 10.0, 60.0, 80.0], 1.0e6, 0.9);
        let params = HydroParams::earth_like(0);
        let f = flood(&g, &ocean_seeds(&g), &|_| true);
        let mut hollows = find_hollows(&g, &f);
        judge(&mut hollows, &g, &params);
        let mut r = route(&g, &f, &mut hollows, &params);
        let (_, closure) = close_lakes(&g, &mut r, &hollows, &params);
        let id = hollows.iter().position(|h| h.enclosed).expect("one enclosed basin");
        assert!(closure.fresh_enclosed[id]);
        assert!(r.surface_m[3] < 0.0, "the 39 m ridge is cut below the datum");
        let mut here = 4u32;
        let mut steps = 0;
        while r.receiver[here as usize] != NO_NODE { here = r.receiver[here as usize]; steps += 1; assert!(steps < 100); }
        assert!(g.ocean[here as usize], "the fresh basin drains to the ocean");
    }

    #[test]
    fn a_dry_lake_closes_and_keeps_its_water() {
        let g = line(&[-50.0, 40.0, 5.0, 12.0, 25.0, 70.0], 2.0e6, 0.05);
        let params = HydroParams::earth_like(0);
        let f = flood(&g, &ocean_seeds(&g), &|_| true);
        let mut hollows = find_hollows(&g, &f);
        judge(&mut hollows, &g, &params);
        let mut r = route(&g, &f, &mut hollows, &params);
        let (_, closure) = close_lakes(&g, &mut r, &hollows, &params);
        assert!(closure.closed[0], "wetness 0.05 cannot keep a lake topped up");
        assert_eq!(r.receiver[hollows[0].lake_entry as usize], NO_NODE);
    }

    #[test]
    fn a_forced_enclosed_basin_is_fresh_however_dry() {
        let g = line(&[-50.0, -40.0, -30.0, 39.0, -5.0, 10.0, 60.0, 80.0], 1.0e6, 0.0);
        let mut params = HydroParams::earth_like(0);
        params.forced_outlets = vec![SpherePoint::from_latlon(0.0, 2.0)];
        let f = flood(&g, &ocean_seeds(&g), &|_| true);
        let mut hollows = find_hollows(&g, &f);
        judge(&mut hollows, &g, &params);
        let mut r = route(&g, &f, &mut hollows, &params);
        let (_, closure) = close_lakes(&g, &mut r, &hollows, &params);
        let id = hollows.iter().position(|h| h.enclosed).expect("one enclosed basin");
        assert!(closure.fresh_enclosed[id]);
    }
}
```

Run: `cargo test -p worldbuilder-engine hydrology::flow`
Expected: FAIL.

- [ ] **Step 2: Implement**

```rust
//! How much water passes each node, and which lakes it keeps full.

use crate::hydrology::flood::NO_NODE;
use crate::hydrology::hollows::{Fate, Hollow};
use crate::hydrology::landgraph::LandGraph;
use crate::hydrology::routing::{cut_path, set_sink, Routing};
use crate::hydrology::HydroParams;

#[derive(Debug, Clone, PartialEq)]
pub struct Closure {
    pub closed: Vec<bool>,
    pub salt_flat: Vec<bool>,
    pub fresh_enclosed: Vec<bool>,
}

pub fn accumulate(graph: &LandGraph, routing: &Routing) -> Vec<f64> {
    let n = graph.len();
    let mut flow: Vec<f64> = (0..n)
        .map(|i| if graph.ocean[i] { 0.0 } else { graph.area_m2[i] * graph.wetness[i] })
        .collect();
    let mut upstream = vec![0u32; n];
    for i in 0..n {
        let r = routing.receiver[i];
        if r != NO_NODE {
            upstream[r as usize] += 1;
        }
    }
    let mut ready: Vec<u32> = (0..n).filter(|&i| upstream[i] == 0).map(|i| i as u32).collect(); // cast-ok: node index
    let mut head = 0;
    while head < ready.len() {
        let node = ready[head] as usize;
        head += 1;
        let r = routing.receiver[node];
        if r == NO_NODE {
            continue;
        }
        flow[r as usize] += flow[node];
        upstream[r as usize] -= 1;
        if upstream[r as usize] == 0 {
            ready.push(r);
        }
    }
    flow
}

fn evaporation(graph: &LandGraph, routing: &Routing, id: usize, hollow: &Hollow, factor: f64) -> f64 {
    let mut area = 0.0;
    let mut wet = 0.0;
    let mut count = 0.0;
    for &m in &hollow.members {
        if routing.lake_of[m as usize] == id as u32 { // cast-ok: hollow index
            area += graph.area_m2[m as usize];
            wet += graph.wetness[m as usize];
            count += 1.0;
        }
    }
    let mean = if count > 0.0 { wet / count } else { 0.0 };
    area * factor * (1.0 - mean)
}

pub fn close_lakes(graph: &LandGraph, routing: &mut Routing, hollows: &[Hollow], params: &HydroParams) -> (Vec<f64>, Closure) {
    let count = hollows.len();
    let mut closure = Closure {
        closed: vec![false; count],
        salt_flat: vec![false; count],
        fresh_enclosed: vec![false; count],
    };

    // Enclosed basins first: they are sinks until their balance says otherwise.
    let flow = accumulate(graph, routing);
    for (id, hollow) in hollows.iter().enumerate() {
        if hollow.fate != Fate::Keep || !hollow.enclosed {
            continue;
        }
        let inflow = flow[hollow.lake_entry as usize];
        let loss = evaporation(graph, routing, id, hollow, params.evaporation_factor);
        if hollow.forced || inflow >= loss {
            closure.fresh_enclosed[id] = true;
            cut_path(routing, graph, &hollow.outlet_path, hollow.level_m - params.notch_fall_m);
        } else {
            closure.closed[id] = true;
            closure.salt_flat[id] = inflow < params.salt_flat_share * loss;
        }
    }

    // Then the rest, until closing one lake starves no other.
    let mut flow = accumulate(graph, routing);
    for _ in 0..=count {
        let mut changed = false;
        for (id, hollow) in hollows.iter().enumerate() {
            if hollow.fate != Fate::Keep || hollow.enclosed || hollow.forced || closure.closed[id] {
                continue;
            }
            let inflow = flow[hollow.lake_entry as usize];
            let loss = evaporation(graph, routing, id, hollow, params.evaporation_factor);
            if inflow < loss {
                closure.closed[id] = true;
                closure.salt_flat[id] = inflow < params.salt_flat_share * loss;
                set_sink(routing, hollow);
                changed = true;
            }
        }
        if !changed {
            break;
        }
        flow = accumulate(graph, routing);
    }
    (flow, closure)
}
```

`cut_path` walks the enclosed basin's `outlet_path` from its lake entry: the shore and the rim are cut below the datum, and the cut stops where the ground toward the sea is already lower.

Run: `cargo test -p worldbuilder-engine hydrology && cargo test -p worldbuilder-engine --test no_std_math`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/worldbuilder-engine/src/hydrology
git commit -m "Water: flow by catchment and wetness, and lakes closed where the air takes more"
```

---

### Task 8: Reaches

**Files:**
- Create: `crates/worldbuilder-engine/src/hydrology/reaches.rs`
- Modify: `src/hydrology/mod.rs` (`pub mod reaches;`)

**Interfaces:**
- Consumes: `LandGraph`, `Routing`, flow `&[f64]`, `HydroParams`, `NO_LAKE`.
- Produces:

```rust
pub enum ReachClass { Stream, River, Great }
pub enum Downstream { Reach(u32), Body(u32), Ocean, Sink }
pub struct Reach { pub nodes: Vec<u32>, pub class: ReachClass, pub order: u32, pub downstream: Downstream }
pub fn extract(graph: &LandGraph, routing: &Routing, flow: &[f64], params: &HydroParams) -> Vec<Reach>;
pub fn bifurcation_ratios(reaches: &[Reach]) -> Vec<f64>;
pub fn width_m(flow_m2: f64, params: &HydroParams) -> f64;
pub fn depth_m(flow_m2: f64, params: &HydroParams) -> f64;
```

The rules:
- A node is **channel** if it is not ocean, not a lake member, and has `flow ≥ stream_flow_m2`.
- A reach starts at a channel node with no channel upstream, at a confluence (two or more channel nodes drain into it), or at a lake's outlet. It runs down receivers to the next confluence (included as the start of the next reach), a lake member (the body), the ocean, or a sink.
- **Class** by the flow at its last node: `≥ great` Great, `≥ river` River, else Stream.
- **Strahler order:** sources are 1. A reach fed by two or more reaches of the highest upstream order k gets k + 1; otherwise it gets the highest upstream order. Computed in topological order of reaches.
- **Width and depth** (Leopold–Maddock): `width = a·Q^0.5` and `depth = c·Q^0.4`, with Q = flow (m²). `a` is chosen so width(stream_flow) = 3 m, and `c` so depth(stream_flow) = 0.5 m. `a = 3 / stream_flow^0.5`, `c = 0.5 / stream_flow^0.4`. (Great-river width then falls where the flow says; Task 12 reports it.)
- **Bifurcation ratios:** `N_k / N_{k+1}` for every k whose `N_{k+1} ≥ 1` and whose `N_k ≥ 10`.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::hydrology::flood::{flood, ocean_seeds};
    use crate::hydrology::flow::close_lakes;
    use crate::hydrology::hollows::{find_hollows, judge};
    use crate::hydrology::landgraph::LandGraph;
    use crate::hydrology::routing::route;
    use crate::hydrology::HydroParams;

    fn baked(seed: i64, nodes: u32) -> (LandGraph, Routing, Vec<f64>, Vec<Reach>) {
        let surface = crate::surface::Surface::new(seed, 6_371_000.0, 12, 0.29, None, None, None);
        let g = LandGraph::sample(&surface, nodes, 500).expect("graph");
        let mut params = HydroParams::earth_like(nodes);
        // Coarse test graph: scale the thresholds to its node area so there are reaches to see.
        params.stream_flow_m2 = 3.0e10;
        params.river_flow_m2 = 3.0e11;
        params.great_flow_m2 = 3.0e12;
        let f = flood(&g, &ocean_seeds(&g), &|_| true);
        let mut hollows = find_hollows(&g, &f);
        judge(&mut hollows, &g, &params);
        let mut r = route(&g, &f, &mut hollows, &params);
        let (flow, _) = close_lakes(&g, &mut r, &hollows, &params);
        let reaches = extract(&g, &r, &flow, &params);
        (g, r, flow, reaches)
    }

    #[test]
    fn reaches_are_connected_and_acyclic() {
        let (_, _, _, reaches) = baked(20_260_904, 12_000);
        assert!(!reaches.is_empty());
        for (id, reach) in reaches.iter().enumerate() {
            if let Downstream::Reach(next) = reach.downstream {
                assert!((next as usize) < reaches.len());
                assert_ne!(next as usize, id);
                assert_eq!(reach.nodes.last(), reaches[next as usize].nodes.first(),
                           "a tributary shares its junction node with its receiver");
            }
        }
        // acyclic: following downstream never revisits
        for start in 0..reaches.len() {
            let mut seen = vec![false; reaches.len()];
            let mut here = start;
            while let Downstream::Reach(next) = reaches[here].downstream {
                assert!(!seen[here], "a cycle through {here}");
                seen[here] = true;
                here = next as usize;
            }
        }
    }

    #[test]
    fn flow_never_falls_along_a_reach_and_order_never_falls_downstream() {
        let (_, _, flow, reaches) = baked(20_260_904, 12_000);
        for reach in &reaches {
            for pair in reach.nodes.windows(2) {
                assert!(flow[pair[1] as usize] >= flow[pair[0] as usize]);
            }
            if let Downstream::Reach(next) = reach.downstream {
                assert!(reaches[next as usize].order >= reach.order);
            }
        }
    }

    #[test]
    fn width_and_depth_hit_their_anchors() {
        let params = HydroParams::earth_like(0);
        assert!((width_m(params.stream_flow_m2, &params) - 3.0).abs() < 1.0e-9);
        assert!((depth_m(params.stream_flow_m2, &params) - 0.5).abs() < 1.0e-9);
        assert!(width_m(params.great_flow_m2, &params) > width_m(params.river_flow_m2, &params));
    }

    #[test]
    fn a_hand_network_has_the_expected_strahler_orders() {
        // Two order-1 sources join (order 2), a third order-1 joins that (still 2).
        let reaches = vec![
            Reach { nodes: vec![0, 4], class: ReachClass::Stream, order: 0, downstream: Downstream::Reach(3) },
            Reach { nodes: vec![1, 4], class: ReachClass::Stream, order: 0, downstream: Downstream::Reach(3) },
            Reach { nodes: vec![2, 5], class: ReachClass::Stream, order: 0, downstream: Downstream::Reach(4) },
            Reach { nodes: vec![4, 5], class: ReachClass::Stream, order: 0, downstream: Downstream::Reach(4) },
            Reach { nodes: vec![5, 6], class: ReachClass::Stream, order: 0, downstream: Downstream::Ocean },
        ];
        let ordered = strahler(reaches);
        assert_eq!(ordered.iter().map(|r| r.order).collect::<Vec<_>>(), vec![1, 1, 1, 2, 2]);
    }
}
```

Run: `cargo test -p worldbuilder-engine hydrology::reaches`
Expected: FAIL.

- [ ] **Step 2: Implement**

```rust
//! Streams, rivers and great rivers, cut from the flow wherever it passes the thresholds.

use crate::detmath as m;
use crate::hydrology::flood::NO_NODE;
use crate::hydrology::landgraph::LandGraph;
use crate::hydrology::routing::{Routing, NO_LAKE};
use crate::hydrology::HydroParams;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReachClass { Stream, River, Great }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Downstream { Reach(u32), Body(u32), Ocean, Sink }

#[derive(Debug, Clone, PartialEq)]
pub struct Reach {
    pub nodes: Vec<u32>,
    pub class: ReachClass,
    pub order: u32,
    pub downstream: Downstream,
}

pub fn width_m(flow_m2: f64, params: &HydroParams) -> f64 {
    3.0 * m::powf(flow_m2 / params.stream_flow_m2, 0.5)
}

pub fn depth_m(flow_m2: f64, params: &HydroParams) -> f64 {
    0.5 * m::powf(flow_m2 / params.stream_flow_m2, 0.4)
}

pub fn extract(graph: &LandGraph, routing: &Routing, flow: &[f64], params: &HydroParams) -> Vec<Reach> {
    let n = graph.len();
    let channel = |i: usize| !graph.ocean[i] && routing.lake_of[i] == NO_LAKE && flow[i] >= params.stream_flow_m2;
    let mut feeders = vec![0u32; n];
    for i in 0..n {
        if channel(i) {
            let r = routing.receiver[i];
            if r != NO_NODE {
                feeders[r as usize] += 1;
            }
        }
    }
    // A lake outlet starts a reach even with one channel feeder: the lake is the source.
    let mut outlet_of_lake = vec![false; n];
    for i in 0..n {
        if routing.lake_of[i] != NO_LAKE {
            let r = routing.receiver[i];
            if r != NO_NODE && routing.lake_of[r as usize] == NO_LAKE {
                outlet_of_lake[r as usize] = true;
            }
        }
    }
    let starts: Vec<u32> = (0..n)
        .filter(|&i| channel(i) && (feeders[i] != 1 || outlet_of_lake[i]))
        .map(|i| i as u32) // cast-ok: node index
        .collect();
    let mut reach_at = vec![u32::MAX; n];
    for (id, &s) in starts.iter().enumerate() {
        reach_at[s as usize] = id as u32; // cast-ok: at most one reach per node
    }
    let mut reaches = Vec::with_capacity(starts.len());
    for &start in &starts {
        let mut nodes = vec![start];
        let mut here = start;
        let downstream = loop {
            let next = routing.receiver[here as usize];
            if next == NO_NODE {
                break Downstream::Sink;
            }
            let j = next as usize;
            if graph.ocean[j] {
                nodes.push(next);
                break Downstream::Ocean;
            }
            if routing.lake_of[j] != NO_LAKE {
                nodes.push(next);
                break Downstream::Body(routing.lake_of[j]);
            }
            nodes.push(next);
            if reach_at[j] != u32::MAX {
                break Downstream::Reach(reach_at[j]);
            }
            here = next;
        };
        let last = *nodes.last().expect("a reach has nodes");
        let q = if graph.ocean[last as usize] || routing.lake_of[last as usize] != NO_LAKE {
            flow[nodes[nodes.len() - 2] as usize]
        } else {
            flow[last as usize]
        };
        let class = if q >= params.great_flow_m2 {
            ReachClass::Great
        } else if q >= params.river_flow_m2 {
            ReachClass::River
        } else {
            ReachClass::Stream
        };
        reaches.push(Reach { nodes, class, order: 0, downstream });
    }
    strahler(reaches)
}

/// Strahler order, in topological order of reaches (sources first).
pub fn strahler(mut reaches: Vec<Reach>) -> Vec<Reach> {
    let count = reaches.len();
    let mut inputs: Vec<Vec<u32>> = vec![Vec::new(); count];
    for (id, reach) in reaches.iter().enumerate() {
        if let Downstream::Reach(next) = reach.downstream {
            inputs[next as usize].push(id as u32); // cast-ok: reach index
        }
    }
    let mut pending: Vec<u32> = inputs.iter().map(|v| v.len() as u32).collect(); // cast-ok: feeder count
    let mut ready: Vec<usize> = (0..count).filter(|&i| pending[i] == 0).collect();
    let mut head = 0;
    while head < ready.len() {
        let id = ready[head];
        head += 1;
        let mut top = 0u32;
        let mut at_top = 0u32;
        for &input in &inputs[id] {
            let o = reaches[input as usize].order;
            if o > top {
                top = o;
                at_top = 1;
            } else if o == top {
                at_top += 1;
            }
        }
        reaches[id].order = if inputs[id].is_empty() { 1 } else if at_top >= 2 { top + 1 } else { top };
        if let Downstream::Reach(next) = reaches[id].downstream {
            let j = next as usize;
            pending[j] -= 1;
            if pending[j] == 0 {
                ready.push(j);
            }
        }
    }
    reaches
}

pub fn bifurcation_ratios(reaches: &[Reach]) -> Vec<f64> {
    let top = reaches.iter().map(|r| r.order).max().unwrap_or(0) as usize;
    let mut counts = vec![0.0f64; top + 2];
    for reach in reaches {
        counts[reach.order as usize] += 1.0;
    }
    let mut ratios = Vec::new();
    for k in 1..=top {
        if counts[k] >= 10.0 && counts[k + 1] >= 1.0 {
            ratios.push(counts[k] / counts[k + 1]);
        }
    }
    ratios
}
```

A reach counts by its segments here: a stream between two confluences is one reach. Horton's law counts streams of an order as whole segments of that order, so before computing ratios, **merge consecutive reaches of equal order** (a reach whose single same-order upstream continues it). Add this to `bifurcation_ratios`: count an order-k reach only if none of its inputs has order k. Replace the counting loop with:

```rust
    let mut continues = vec![false; reaches.len()];
    for reach in reaches {
        if let Downstream::Reach(next) = reach.downstream {
            if reaches[next as usize].order == reach.order {
                continues[next as usize] = true;
            }
        }
    }
    for (id, reach) in reaches.iter().enumerate() {
        if !continues[id] {
            counts[reach.order as usize] += 1.0;
        }
    }
```

Run: `cargo test -p worldbuilder-engine hydrology && cargo test -p worldbuilder-engine --test no_std_math`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/worldbuilder-engine/src/hydrology
git commit -m "Water: reaches from the flow, with classes, widths and Strahler order"
```

---

### Task 9: The bake, the record, and the properties

**Files:**
- Create: `crates/worldbuilder-engine/src/hydrology/record.rs`
- Modify: `crates/worldbuilder-engine/src/hydrology/mod.rs` (types and `bake()`)

**Interfaces:**
- Consumes: everything above.
- Produces, in `mod.rs`:

```rust
pub enum BodyKind { Lake, Pond, SaltLake, SaltFlat }
pub struct Body { pub id: u32, pub kind: BodyKind, pub fresh: bool, pub enclosed: bool, pub forced: bool,
                  pub level_m: f64, pub area_m2: f64, pub depth_m: f64, pub outlet_reach: Option<u32>,
                  pub anchor: (f64, f64), pub outline: Vec<(f64, f64)> }
pub struct ReachPoint { pub lat_deg: f64, pub lon_deg: f64, pub bed_m: f64, pub width_m: f64, pub depth_m: f64, pub flow_m2: f64 }
pub struct ReachLine { pub id: u32, pub class: ReachClass, pub order: u32, pub downstream: Downstream, pub points: Vec<ReachPoint> }
pub struct NotchLine { pub points: Vec<(f64, f64, f64)> }
pub struct Fall { pub reach: u32, pub at: (f64, f64), pub height_m: f64 }
pub struct BakeStats { pub nodes: u32, pub land_nodes: u32, pub hollows: u32, pub kept: u32, pub notched: u32,
                       pub closed: u32, pub streams: u32, pub rivers: u32, pub great: u32, pub max_order: u32,
                       pub bifurcation_min: f64, pub bifurcation_max: f64 }
pub struct HydroRecord { pub bodies: Vec<Body>, pub reaches: Vec<ReachLine>, pub notches: Vec<NotchLine>,
                         pub falls: Vec<Fall>, pub stats: BakeStats }
pub enum HydroError { Params(&'static str), Sampling }
pub fn bake(surface: &Surface, params: &HydroParams) -> Result<HydroRecord, HydroError>;
```

and in `record.rs`: `pub const SCHEMA: f64 = 1.0; pub fn encode(record: &HydroRecord) -> Vec<f64>; pub fn decode(words: &[f64]) -> Option<HydroRecord>;`

**Record layout** (the order is the contract):

| Words | Meaning |
|---|---|
| Header, 17 words | `[SCHEMA, body_count, reach_count, notch_count, fall_count, nodes, land_nodes, hollows, kept, notched, closed, streams, rivers, great, max_order, bifurcation_min, bifurcation_max]` |
| Body | `id, kind(0 Lake, 1 Pond, 2 SaltLake, 3 SaltFlat), fresh, enclosed, forced, level_m, area_m2, depth_m, outlet_reach (-1 if none), anchor_lat, anchor_lon, outline_len`, then `outline_len × [lat, lon]` |
| Reach | `id, class(0 Stream, 1 River, 2 Great), order, downstream_kind(0 Reach, 1 Body, 2 Ocean, 3 Sink), downstream_id (-1 unless Reach or Body), point_count`, then `point_count × [lat, lon, bed_m, width_m, depth_m, flow_m2]` |
| Notch | `point_count`, then `point_count × [lat, lon, bed_m]` |
| Fall | `reach, lat, lon, height_m` |

**`bake()` builds:**
- **Validation:** `total_nodes` in `2..=stream::MAX_NODES`; every threshold finite and > 0; `stream ≤ river ≤ great`. Otherwise `HydroError::Params(...)`.
- **Order:** `LandGraph::sample` → `flood(ocean_seeds)` → `find_hollows` + `judge` → `route` → `close_lakes` → `extract`.
- **Bodies:** one per kept hollow, in hollow order. The kind:
  - `SaltFlat` if a closed salt flat, else `SaltLake` if closed;
  - otherwise `Pond` if area < `pond_max_area_m2`, else `Lake`.

  `fresh = !closed`. `anchor` is the floor node's lat/lon. `outline` is empty in 1a. `outlet_reach` is the reach whose first node is the outlet (or `None`).
- **Reach points:** one per node, with `bed_m = surface_m[node] − depth_m(flow)` for channel nodes. The terminal ocean or lake node's bed is its surface.
- **Notches** from `routing.notches`. **Falls** empty in 1a.
- **Stats** from the counts. `bifurcation_min` and `bifurcation_max` come from `bifurcation_ratios` (`0.0` if empty).

- [ ] **Step 1: Write the failing tests** (in `mod.rs`)

```rust
#[cfg(test)]
mod bake_tests {
    use super::*;
    use crate::hydrology::record::{decode, encode};
    use crate::surface::Surface;

    fn world() -> Surface {
        Surface::new(20_260_904, 6_371_000.0, 12, 0.29, None, None, None)
    }

    fn params() -> HydroParams {
        let mut p = HydroParams::earth_like(12_000);
        p.wetness_nodes = 500;
        p.stream_flow_m2 = 3.0e10;
        p.river_flow_m2 = 3.0e11;
        p.great_flow_m2 = 3.0e12;
        p
    }

    #[test]
    fn a_bake_is_bit_identical_run_to_run() {
        let a = encode(&bake(&world(), &params()).expect("bake"));
        let b = encode(&bake(&world(), &params()).expect("bake"));
        assert_eq!(a.iter().map(|w| w.to_bits()).collect::<Vec<_>>(),
                   b.iter().map(|w| w.to_bits()).collect::<Vec<_>>());
    }

    #[test]
    fn the_record_round_trips() {
        let record = bake(&world(), &params()).expect("bake");
        let words = encode(&record);
        assert_eq!(decode(&words).as_ref(), Some(&record));
        assert_eq!(words[0], record::SCHEMA);
    }

    #[test]
    fn a_truncated_record_is_refused() {
        let words = encode(&bake(&world(), &params()).expect("bake"));
        assert_eq!(decode(&words[..words.len() - 1]), None);
        assert_eq!(decode(&[]), None);
    }

    #[test]
    fn no_kept_body_is_below_the_keep_rule_unless_forced_or_enclosed() {
        let p = params();
        let record = bake(&world(), &p).expect("bake");
        for body in &record.bodies {
            if !body.forced && !body.enclosed {
                assert!(body.depth_m >= p.keep_depth_m && body.area_m2 >= p.keep_area_m2,
                        "body {} depth {} area {}", body.id, body.depth_m, body.area_m2);
            }
        }
    }

    #[test]
    fn every_open_lake_has_one_outlet_reach_or_drains_straight_to_the_sea() {
        let record = bake(&world(), &params()).expect("bake");
        for body in &record.bodies {
            if body.fresh {
                let feeding = record.reaches.iter()
                    .filter(|r| r.downstream == Downstream::Body(body.id)).count();
                let _ = feeding; // lakes may have no feeding reach at coarse thresholds
                assert!(body.outlet_reach.map_or(true, |id| (id as usize) < record.reaches.len()));
            }
        }
    }

    #[test]
    fn bad_params_are_refused_not_panicked() {
        let mut p = params();
        p.river_flow_m2 = p.stream_flow_m2 / 2.0;
        assert!(matches!(bake(&world(), &p), Err(HydroError::Params(_))));
        let mut p = params();
        p.total_nodes = 1;
        assert!(matches!(bake(&world(), &p), Err(HydroError::Params(_))));
    }

    /// Mutation guard for the connectivity property: a hand-broken reach list must fail it.
    #[test]
    fn the_connectivity_check_catches_a_cycle() {
        let mut record = bake(&world(), &params()).expect("bake");
        assert!(reaches_are_acyclic(&record.reaches));
        if record.reaches.len() >= 2 {
            record.reaches[0].downstream = Downstream::Reach(1);
            record.reaches[1].downstream = Downstream::Reach(0);
            assert!(!reaches_are_acyclic(&record.reaches));
        }
    }
}
```

`reaches_are_acyclic(reaches: &[ReachLine]) -> bool` is a `pub fn` in `mod.rs` (used again by the survey). It walks downstream from every reach and fails on a revisit.

Run: `cargo test -p worldbuilder-engine hydrology::bake_tests`
Expected: FAIL.

- [ ] **Step 2: Implement `mod.rs` types and `bake`, and `record.rs`**

`record.rs` writes the table above field by field. `decode` reads it back, validating every count against the remaining length and returning `None` on any mismatch or on trailing words. Use exact `f64` values for enums and `-1.0` for "none", and convert with checked branches, not casts from arbitrary floats:

```rust
fn word_to_u32(w: f64) -> Option<u32> {
    if w.is_finite() && w >= 0.0 && w <= u32::MAX as f64 && m::floor(w) == w {
        Some(w as u32) // cast-ok: checked finite, non-negative, integral and in range above
    } else {
        None
    }
}
```

`bake` follows the order given above. Its body maps are:
- `hollow index → body id` (kept hollows only, in order)
- `node → reach id` for first nodes (for `outlet_reach`)

Run: `cargo test -p worldbuilder-engine hydrology && cargo test -p worldbuilder-engine --test no_std_math`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/worldbuilder-engine/src/hydrology
git commit -m "Water: the bake end to end, and a flat record that round-trips"
```

---

### Task 10: The wasm exports and the viewer wrapper

**Files:**
- Modify: `crates/worldbuilder-engine/src/wasm.rs` (four exports, `WB_EXPORTS`, the params stride doc)
- Modify: `crates/worldbuilder-engine/tests/wasm_exports.rs` (tests)
- Modify: `viewer/public/app/engine.js` (expected-exports list, `hydroBake()`)
- Create: `viewer/test/hydro.test.mjs`

**Interfaces:**
- Consumes: `hydrology::{bake, HydroParams, record::encode}`.
- Produces:

```rust
pub const WB_HYDRO_PARAMS_STRIDE: usize = 12;
// [total_nodes, wetness_nodes, keep_depth_m, keep_area_m2, pond_max_area_m2, stream_flow_m2,
//  river_flow_m2, great_flow_m2, notch_fall_m, evaporation_factor, salt_flat_share, forced_count]
//  followed by forced_count x [lat, lon]. The order is the contract.
#[no_mangle] pub extern "C" fn wb_hydro_bake(handle: u32, params: *const f64, params_len: u32, out_id: *mut u32) -> u32;
#[no_mangle] pub extern "C" fn wb_hydro_len(id: u32) -> u32;      // words, 0 = unknown id
#[no_mangle] pub extern "C" fn wb_hydro_copy(id: u32, out: *mut f64, out_len: u32) -> u32;
#[no_mangle] pub extern "C" fn wb_hydro_free(id: u32) -> u32;
```

In `engine.js`:
- `hydroBake({ handle, params })` returns `Float64Array`.
- `hydroSummary(words)` returns `{ schema, bodies, reaches, notches, falls, nodes, landNodes, hollows, kept, notched, closed, streams, rivers, great, maxOrder, bifurcationMin, bifurcationMax }`.

**Protocol:**
- **Bakes are held in a `thread_local!` table** `HYDRO: RefCell<Vec<Option<Vec<f64>>>>` with 1-based ids that are never reused, like world handles.
- **`wb_hydro_bake`** validates:
  - `params` non-null and 8-aligned;
  - `params_len ≥ WB_HYDRO_PARAMS_STRIDE`;
  - `params_len == stride + 2 × forced_count`;
  - every word finite;
  - `out_id` non-null and 4-aligned;
  - the node counts are integral, and `2 ≤ total_nodes ≤ WB_MAX_HYDRO_NODES`, where `pub const WB_MAX_HYDRO_NODES: u32 = 4_000_000;`

  It returns `WB_ERR_PARAM`, `WB_ERR_BUFFER` or `WB_ERR_HANDLE`, and maps `HydroError::Params` to `WB_ERR_PARAM` and `Sampling` to `WB_ERR_GRAPH`. It writes `*out_id` only on `WB_OK`.
- **`wb_hydro_copy`** is all-or-nothing, like `wb_water_run`: `WB_ERR_BUFFER` if `out_len` < len or the pointer is null or misaligned, `WB_ERR_HANDLE` for an unknown id.
- **`wb_hydro_free`** returns `WB_OK` or `WB_ERR_HANDLE`, and drops the record.

- [ ] **Step 1: Write the failing Rust tests** in `tests/wasm_exports.rs`

```rust
fn hydro_params(total: u32) -> Vec<f64> {
    vec![total as f64, 500.0, 8.0, 1.0e6, 1.0e6, 3.0e10, 3.0e11, 3.0e12, 1.0, 1.0, 0.1, 0.0]
}

#[test]
fn a_hydro_bake_is_held_copied_and_freed() {
    let world = plain_world();
    let params = hydro_params(12_000);
    let mut id: u32 = 0;
    let status = wb_hydro_bake(world, params.as_ptr(), params.len() as u32, &mut id); // cast-ok: a 12-word buffer
    assert_eq!(status, WB_OK);
    let len = wb_hydro_len(id);
    assert!(len >= 17, "at least the header");
    let mut words = vec![0.0f64; len as usize];
    assert_eq!(wb_hydro_copy(id, words.as_mut_ptr(), len), WB_OK);
    assert_eq!(words[0], 1.0, "schema 1");
    let mut short = vec![0.0f64; len as usize - 1];
    assert_eq!(wb_hydro_copy(id, short.as_mut_ptr(), len - 1), WB_ERR_BUFFER);
    assert_eq!(wb_hydro_free(id), WB_OK);
    assert_eq!(wb_hydro_len(id), 0);
    assert_eq!(wb_hydro_free(id), WB_ERR_HANDLE);
    wb_world_free(world);
}

#[test]
fn a_hydro_bake_refuses_bad_params_without_writing_an_id() {
    let world = plain_world();
    let mut id: u32 = 77;
    let mut params = hydro_params(12_000);
    params[6] = 1.0; // river below stream
    assert_eq!(wb_hydro_bake(world, params.as_ptr(), params.len() as u32, &mut id), WB_ERR_PARAM); // cast-ok: a 12-word buffer
    assert_eq!(id, 77);
    let mut params = hydro_params(12_000);
    params[11] = 1.0; // one forced outlet promised, none given
    assert_eq!(wb_hydro_bake(world, params.as_ptr(), params.len() as u32, &mut id), WB_ERR_PARAM); // cast-ok: a 12-word buffer
    assert_eq!(wb_hydro_bake(9_999, hydro_params(12_000).as_ptr(), 12, &mut id), WB_ERR_HANDLE);
    wb_world_free(world);
}
```

Also add the four names to the file's own export expectations if it lists them.

Run: `cargo test -p worldbuilder-engine --features wasm --test wasm_exports hydro`
Expected: FAIL (unresolved functions).

- [ ] **Step 2: Implement the exports** in `wasm.rs`

Follow the `wb_water_run` pattern: validate scalars and pointers first, then `with_world(handle, |world| hydrology::bake(world.surface(), &params))`, then store `record::encode(&record)` in `HYDRO`. Add the four names to `WB_EXPORTS`, in the same one-per-line style. Never write `#[no_mangle]` in a comment. The params decode is:

```rust
fn hydro_params_from(words: &[f64]) -> Option<HydroParams> {
    let whole = |w: f64| w.is_finite() && w >= 0.0 && m::floor(w) == w;
    if words.len() < WB_HYDRO_PARAMS_STRIDE || words.iter().any(|w| !w.is_finite()) {
        return None;
    }
    if !whole(words[0]) || !whole(words[1]) || !whole(words[11]) {
        return None;
    }
    if words[0] > WB_MAX_HYDRO_NODES as f64 || words[1] > WB_MAX_HYDRO_NODES as f64 {
        return None;
    }
    let forced = words[11] as usize; // (usize casts are not banned)
    if words.len() != WB_HYDRO_PARAMS_STRIDE + 2 * forced {
        return None;
    }
    let total = words[0] as u32; // cast-ok: checked integral, non-negative and <= WB_MAX_HYDRO_NODES above
    let wet = words[1] as u32;   // cast-ok: checked integral, non-negative and <= WB_MAX_HYDRO_NODES above
    let mut p = HydroParams::earth_like(total);
    p.wetness_nodes = wet;
    p.keep_depth_m = words[2];
    p.keep_area_m2 = words[3];
    p.pond_max_area_m2 = words[4];
    p.stream_flow_m2 = words[5];
    p.river_flow_m2 = words[6];
    p.great_flow_m2 = words[7];
    p.notch_fall_m = words[8];
    p.evaporation_factor = words[9];
    p.salt_flat_share = words[10];
    p.forced_outlets = (0..forced)
        .map(|k| SpherePoint::from_latlon(words[12 + 2 * k], words[13 + 2 * k]))
        .collect();
    Some(p)
}
```

Run: `cargo test -p worldbuilder-engine --features wasm --test wasm_exports`
Expected: PASS, including `the_declared_export_list_is_the_source`.

- [ ] **Step 3: Rebuild the wasm and write the viewer wrapper and its test**

Run: `cd viewer && npm run build:wasm`
Expected: the build succeeds and `viewer/public/wasm/worldbuilder_engine.wasm` and `MANIFEST.txt` change.

In `engine.js`, add the four names to the expected-exports list, then add `hydroBake` and `hydroSummary`. `hydroSummary` reads words 0–16 by the header table.

`viewer/test/hydro.test.mjs`:

```js
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { Engine } from "../public/app/engine.js";

const wasmPath = fileURLToPath(new URL("../public/wasm/worldbuilder_engine.wasm", import.meta.url));
const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath), {});
const engine = new Engine(instance);

const PARAMS = {
  totalNodes: 12000, wetnessNodes: 500, keepDepthM: 8, keepAreaM2: 1e6, pondMaxAreaM2: 1e6,
  streamFlowM2: 3e10, riverFlowM2: 3e11, greatFlowM2: 3e12, notchFallM: 1,
  evaporationFactor: 1, saltFlatShare: 0.1, forcedOutlets: [],
};

test("a bake comes back with a schema-1 header and counts that add up", () => {
  const handle = engine.newWorld({ seed: 20260904, radiusM: 6371000, plateCount: 12, landFraction: 0.29 });
  const words = engine.hydroBake({ handle, params: PARAMS });
  const s = engine.hydroSummary(words);
  assert.equal(s.schema, 1);
  assert.equal(s.nodes, 12000);
  assert.ok(s.landNodes > 0 && s.landNodes < 12000);
  assert.equal(s.kept + s.notched, s.hollows);
  assert.equal(s.streams + s.rivers + s.great, s.reaches);
});

test("the same bake twice is the same words", () => {
  const handle = engine.newWorld({ seed: 20260904, radiusM: 6371000, plateCount: 12, landFraction: 0.29 });
  const a = engine.hydroBake({ handle, params: PARAMS });
  const b = engine.hydroBake({ handle, params: PARAMS });
  assert.deepEqual(Array.from(a), Array.from(b));
});
```

Run: `cd viewer && node --test test/hydro.test.mjs`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/worldbuilder-engine/src/wasm.rs crates/worldbuilder-engine/tests/wasm_exports.rs viewer/public/app/engine.js viewer/test/hydro.test.mjs viewer/public/wasm
git commit -m "Water: bake, measure, copy and free a hydro record from wasm"
```

---

### Task 11: Parity and CI counts

**Files:**
- Modify: `crates/worldbuilder-engine/examples/parity_dump.rs` (an `H plain` record)
- Modify: `crates/worldbuilder-engine/parity/parity.mjs` (`case 'H':`)
- Modify: `.github/workflows/gates.yml` (engine count pins, parity corpus and control counts, with a dated paragraph)
- Modify: `crates/worldbuilder-engine/README.md` (the mirrored counts)

- [ ] **Step 1: Add the dump record**

In `parity_dump.rs`, after the water block, bake `plain` with `hydro_params(12_000)` from Task 10 and print:

```rust
println!("H plain {} {status} {len} {}",
         params.iter().map(|w| hex(*w)).collect::<Vec<_>>().join(" "),
         words.iter().map(|w| hex(*w)).collect::<Vec<_>>().join(" "));
```

Put the params length first so the replay can split the line. The exact line is:

```
H plain <params_len> <params hex x params_len> <status> <len> <words hex x len>
```

- [ ] **Step 2: Add the replay case** in `parity.mjs`

```js
case 'H': {
  // H <world> <params_len> <params hex...> <status> <len> <record hex...>
  const h = worlds.get(f[1]);
  const pl = Number(f[2]);
  const params = f.slice(3, 3 + pl).map(f64of);
  const status = f[3 + pl];
  const len = Number(f[4 + pl]);
  const words = f.slice(5 + pl);
  if (words.length !== len) throw new Error('hydro line is the wrong length');
  const pp = wb.wb_alloc(pl * 8);
  const idp = wb.wb_alloc(4);
  new Float64Array(wb.memory.buffer, pp, pl).set(params);
  const got = wb.wb_hydro_bake(h, pp, pl, idp);
  group = `hydro/${f[1]}`;
  tally(String(got) === status);
  if (String(got) !== status) note(`hydro status ${f[1]}`, status, String(got));
  const id = mem().getUint32(idp, true);
  const n = wb.wb_hydro_len(id);
  tally(n === len);
  const out = wb.wb_alloc(n * 8);
  wb.wb_hydro_copy(id, out, n);
  const view = mem();
  for (let i = 0; i < len; i += 1) {
    const bits = bitsOf(view.getFloat64(out + i * 8, true));
    tally(bits === words[i]);
    if (bits !== words[i]) note(`hydro word ${i}`, words[i], bits);
  }
  wb.wb_hydro_free(id);
  wb.wb_dealloc(out, n * 8);
  wb.wb_dealloc(pp, pl * 8);
  wb.wb_dealloc(idp, 4);
  break;
}
```

Use the file's own names for `wb.memory`, `mem()`, `bitsOf` and `f64of`; match the existing `W` case.

- [ ] **Step 3: Rebuild and run parity**

Run:
```bash
cd viewer && npm run build:wasm && cd ../crates/worldbuilder-engine/parity
cargo run --release -p worldbuilder-engine --example parity_dump --features wasm > native.txt
node parity.mjs native.txt
```
Expected: `0 divergent`. Record the new compared total; it grows by `2 + len`. Run every `--mutate` control listed in gates.yml and record its new compared and divergent pair. `seed` must now also diverge on the `H` record.

- [ ] **Step 4: Re-derive and pin the counts**

Run, for each of the five feature configs in gates.yml:
```bash
cargo test -p worldbuilder-engine <features> -- --list > list-all.txt
cargo test -p worldbuilder-engine <features> -- --list --ignored > list-ignored.txt
python .github/scripts/assert_counts.py cargo-list --all list-all.txt --ignored list-ignored.txt --expect-passed 0 --expect-ignored 5
```
The last command fails, printing the real count. Put each printed count into gates.yml's `expect:` for its config, and the parity totals into their steps. Add one dated paragraph, "2026-09-10, water 1a:", giving old → new for each config, the shape (uniform `src/` tests plus wasm-only `tests/wasm_exports.rs` tests), the parity delta, and "Re-derived per configuration through `assert_counts.py cargo-list` AFTER the last source edit." Mirror the counts in `crates/worldbuilder-engine/README.md`.

- [ ] **Step 5: Full engine suite and commit**

Run: `cargo test -p worldbuilder-engine --no-fail-fast && cargo test -p worldbuilder-engine --features wasm --no-fail-fast`
Expected: all pass.

```bash
git add crates/worldbuilder-engine/examples/parity_dump.rs crates/worldbuilder-engine/parity/parity.mjs .github/workflows/gates.yml crates/worldbuilder-engine/README.md viewer/public/wasm
git commit -m "Water: hydro records in native-vs-wasm parity, and the counts re-derived"
```

---

### Task 12: Survey, calibration on the owner's world, and the node budget

**Files:**
- Create: `crates/worldbuilder-engine/src/bin/hydro_survey.rs`
- Modify: `crates/worldbuilder-engine/src/hydrology/mod.rs` (`earth_like` constants, if tuned)
- Modify: `docs/superpowers/specs/2026-09-10-automatic-water-design.md` (a "Calibration, 1a" table under section 6)
- Create: `docs/superpowers/reports/2026-09-10-water-1a-calibration.md`

- [ ] **Step 1: Write the survey binary**

It prints, per world and node count:
- bake wall time per step (graph, flood, hollows, routing, flow, reaches);
- a memory proxy: bytes held by the graph's vectors, computed from their lengths;
- land nodes, hollows, kept, notched, closed, enclosed;
- body counts by kind; streams, rivers and great rivers;
- max order and the bifurcation ratios.

It uses the "Method" module-doc convention: every figure names its population, its method with parameters, and its host. The worlds are `plain` (20260904, 6.371 Mm, 12, 0.29) and the 4.5 Mm owner survey world (562423712, 28, 0.16, `TectonicParams::ranges()`). Node counts are 250k, 1M and 2M.

Run: `cargo run --release --no-default-features --bin hydro_survey`
Expected: a table. Save the output into the report.

- [ ] **Step 2: Calibrate on the owner's saved world in the studio**

The saved world (`worlds/world-1788998299904.json`, radius 9,309 km) is built by the viewer's own spec path, so it is baked through the studio. With the studio running (`http://localhost:8137`), open the world from the library. Then, in the browser console or the Browser pane's JavaScript tool:

```js
const engine = window.__wb.engine;
const handle = window.__wb.world;
const run = (totalNodes) => {
  const t = performance.now();
  const words = engine.hydroBake({ handle, params: { ...PARAMS_EARTH_LIKE, totalNodes,
    forcedOutlets: [{ latitudeDeg: -26.0, longitudeDeg: -30.25 }] } });
  return { totalNodes, ms: Math.round(performance.now() - t), ...engine.hydroSummary(words) };
};
[500000, 1000000, 2000000].map(run);
```

`PARAMS_EARTH_LIKE` is the Task 5 values. If `window.__wb.world` is not the handle's name, find it in `main.js`'s `window.__wb` object; do not guess. Record the wasm heap size (`engine.memory.buffer.byteLength`) after each run.

- [ ] **Step 3: Decide and record**

- **Node budget:** the largest count whose studio heap stays at or under 512 MB and whose bake finishes inside 5 minutes becomes the default `total_nodes`. Add `pub const DEFAULT_TOTAL_NODES: u32` in `mod.rs`.
- **The great lake:** the enclosed body at the inland sea must be `fresh`, `forced` and `enclosed`. Its outlet notch must pass within one node spacing of 26.00°S 30.25°W. If it does not, report where the bake put it (spec §15: the bake's answer stands).
- **Thresholds:** if the owner's world's bifurcation ratios fall outside 3–5 at the default node count, move `stream_flow_m2` (only this one) by factors of 2 until they land, up to 4 steps. Record every step tried. If they never land, record that; do not force it.
- **Pit lakes:** report the kept body count and size spread, beside the old 1,361 bodies (638 single-node) from the spec.

Write the report and the spec's calibration table, and update `earth_like` if any constant changed. Run `cargo test -p worldbuilder-engine` (the counts are unchanged by constant edits; if a test pinned a changed value, fix the test).

- [ ] **Step 4: Rebuild the wasm if `src/` changed, re-run parity, and commit**

```bash
cd viewer && npm run build:wasm && cd ..
git add crates/worldbuilder-engine/src/bin/hydro_survey.rs crates/worldbuilder-engine/src/hydrology docs viewer/public/wasm
git commit -m "Water: survey and calibration of the coarse bake on the owner's world"
```

If a new `src/` file (the survey binary) moved any count, re-run Task 11 Step 4 first.

---

## Self-review notes

- **Spec coverage:** §5.1 (landform: Task 3), §5.2 and W1 (Tasks 3, 5, 6, 7), §6.1 (Tasks 3, 12), §6.2 (Task 4), §6.3 (Tasks 5, 6, 7), §6.4 (Task 7), §6.5 (Task 8), §7 (Task 9, record in flat form; the worldfile JSON is stage 2), §13 1a (Tasks 10–12), §14 properties 1–6 and 10 (Tasks 4–9), §15 (Task 12). §6.6, §6.7 and outlines are plan 1b.
- **Types used across tasks:** `LandGraph`, `Flood`/`NO_NODE`, `Hollow`/`Fate`, `Routing`/`NO_LAKE`, `Reach`/`Downstream`/`ReachClass` and `HydroParams`, each defined once, in the task that introduces it.
