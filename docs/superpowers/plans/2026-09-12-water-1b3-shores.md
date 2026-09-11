# Water 1b-3: Shores and Crossings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refined reaches stop crossing one another, small lakes and ponds are found near the rivers, and the record says how a body's extent is to be decided — the last thing stage 2 needs before it can carve.

**Architecture:** Three pieces, each on its own. A crossing pass in `refine.rs` finds refined segments of unrelated reaches that intersect and straightens the yielding one back to its chord. A new `ponds.rs` samples 250 m strips along the refined rivers, finds fine hollows, keeps the ones the pond rule allows, and appends them to the record as bodies with traced outlines. A measured spike decides how a big body's extent is recorded, because a 250 m outline of the owner's great lake cannot fit the record; its outcome is a design note and a spec ruling that plan 1b-4 implements.

**Tech Stack:** Rust (`crates/worldbuilder-engine`, detmath only), wasm via `npm run build:wasm`, the viewer's ES modules with `node --test`.

**Spec:** `docs/superpowers/specs/2026-09-10-automatic-water-design.md`. Binding sections: §6.6 (small lakes and ponds, the corridor), §7 (record), §8.3 (the query that reads a body's outline), §14 (properties). The carry-forward is `docs/superpowers/reports/2026-09-10-water-1a-carry-forward.md`; plan 1b-2's verification is `docs/superpowers/reports/2026-09-12-water-1b2-verification.md`.

**Branch:** `water-shores`, from master at 562bc1d (plan 1b-2 merged).

## Why the outlines are a spike and not a task

Spec §6.6 says each kept lake "is filled at 250 m resolution from its lowest point up to its level, within its coarse basin", and its outline "traced and then simplified to 250 m tolerance". On the owner's world that cannot be done as written:

- The great lake is 41.33M km². A 250 m fill is about 6.6×10⁸ cells.
- Its shoreline, taken as a circle of that area, is about 23,000 km, which is 92,000 points at 250 m, or 1.5 MB of record for one body before any fractal detail. The whole record already stands at 5.96 MB against an 8 MB target.
- The node graph is **k-nearest, not a triangulation** (`stream.rs::node_neighbours`, k = 8). There are no faces to walk, so a body's boundary cannot simply be read off the graph as a ring.

So Task 1 measures the alternatives on the owner's world and rules between them. The recommendation, and the default if the measurements do not overturn it, is **the collar ruling**:

> **Ruling S-1 (proposed, Task 1 confirms or replaces it).** A body's recorded extent is a **containment ring**, not a shoreline: the ring of nodes adjacent to the body's members (its collar), ordered and simplified. The true shoreline is decided per sample by `water_at` as "inside the ring **and** the landform below the body's level". The record carries containment; the level carries the truth.
>
> Two things fall out of it. The shoreline is exact at every zoom, at no record cost. And an island inside a lake — the owner's island in the great lake — needs no second ring: its ground stands above the level, so it is dry by the same test.

Ponds do not wait on that. They are found at 250 m and are at most a few hundred metres across, so their outline is traced at 250 m as §6.6 asks, whatever Task 1 rules for the big bodies.

## Global Constraints

- **No std float maths outside `detmath.rs`.** `tests/no_std_math.rs` scans `src/`, bins and test modules included. Use `crate::detmath as m` (`sin`, `cos`, `sqrt`, `hypot`, `atan2`, `powf`, `floor`, …). There is no `ceil`: write `-m::floor(-x)`. No `.abs()`: write the comparison.
- **Casts:** `as u32`, `as u64`, `as i32`, `as i64` and float→`usize` casts need `// cast-ok: <reason>` on the same line.
- **No `f64::min`, `f64::max` or `.clamp(`.** Write explicit `if`/`else`. Sort floats with `total_cmp`.
- **No panics reachable from `extern "C"`** (`wb_hydro_bake` calls `hydrology::bake`).
- **Determinism:** no HashMap or HashSet order in output. Fixed candidate order, ties to the first in a stated order, and every sort total and explicit.
- **`Surface` gains no field.** Commit subjects name no third party. End every commit message with a blank line, then exactly `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`.
- **Every `src/` edit changes the fingerprint.** Rebuild the wasm (`npm run build:wasm` in `viewer/`) in the tasks that say so, and commit it.
- **CI pins** (currently engine 751/751/753/857/859 with 6 ignored; parity plain 146,555/0, seed control 140,818, tectonic control 20,808 with the native `hydro/ranges` prediction at 14,622; Python 565/157) are re-derived once, in the last task, by running them and never by transcribing. The crate README mirrors `.github/workflows/gates.yml`.
- **The record layout's twins** change together: `src/hydrology/record.rs`, `viewer/public/app/engine.js::hydroSummary`, `viewer/public/app/water-preview.js::decodeHydro`, and their tests.
- **"Everything drains" is binding.** `drainage_check` still passes in `bake_stages`, and `cargo test --release -p worldbuilder-engine --lib every_small_world_drains -- --ignored` still passes. Nothing in this plan changes routing.
- **Spec §14.5 (the bed never rises), §14.4 (junctions are shared), R-3a (no inland station on sea ground except where its chord is), and Ruling R-1 (coarse points kept exactly)** must all still hold after the crossing pass. Plan 1b-2's tests hold them; keep them green.
- **Size and time targets:** the owner's world record is at most 8 MB and its whole wasm bake at most 300 s. Both are measured in Task 7. The record stands at 5,958,896 bytes, so this plan's budget is **about 2 MB**.
- **Verification numbers** come from runs, and each states its population, method and host.
- Only the wb-clean test database may ever be written to; nothing in this plan touches a database.

## Rulings made while writing this plan

| # | Ruling | Why | Cost if wrong |
|---|---|---|---|
| S-1 | A body's extent is a containment ring plus its level, not a traced shoreline (see above). Task 1 confirms or replaces it. | A 250 m shoreline of the great lake costs 1.5 MB and the budget is 2 MB. Containment plus level is exact at every zoom and gives the island for free. | Stage 2 and the studio draw from a coarser ring than the spec imagined; the shoreline itself is unaffected. |
| S-2 | The **coarse** crossings this plan does not fix (61 on a 1M stand-in) stay. Only the crossings refinement introduced are removed. | A coarse crossing is a graph artifact: the two reaches' node paths really do cross. Fixing that means re-routing, which is out of scope and would move every pin. | Stage 2 carves a handful of crossing channels per world. The record reports the count. |
| S-3 | Where two reaches cross, the one with the **smaller flow at the crossing** yields, and its whole coarse segment is straightened to its chord (every interior station's lateral set to 0). Ties go to the larger reach id. | Straightening a whole segment is deterministic and cannot make a new bend. The bigger river keeps the valley it found. | A yielded segment loses its valley-following for one coarse step (about one graph spacing). |
| S-4 | The pass runs **before** meander and simplification, and repeats until no crossing is left or three passes are done. What remains is counted in the record. | A straightened segment can cross something new; three passes bound the work. Meanders are cosmetic and must not be straightened away when they are not the cause. | A rare crossing survives and is reported rather than fixed. |
| S-5 | A fine pond's `downstream` is `Reach(r)`, where r is the reach whose refined line is nearest its anchor, and its `outlet_reach` is `None`. It is fresh. | Spec §6.6 calls these texture and traces no channel for them. A pond beside a river drains to that river. | Stage 2 draws no outflow stream from a pond. |
| S-6 | The fine search reads **wetness and slope from the coarse graph** (`LandGraph::wetness` at the nearest node, and the landform's gradient over one pond cell), not from a new climate sampling. | The climate march is the expensive part of a bake, and §6.6's gates are coarse gates on where to look. | A pond can be kept in terrain the fine truth would have gated out. |
| S-7 | Ponds are appended to `HydroRecord.bodies` after the coarse bodies, keeping coarse body ids stable. A candidate is dropped if its anchor's nearest graph node is a member of a coarse body, or if the landform there is at or below the datum. | Ids are already the wire's index and stage 2 will key on them. Ponds inside the sea or inside a bigger lake are not ponds. | A pond is lost on a coarse lake's shore, within one graph spacing. |
| S-8 | The density cap of §6.6 ("at most one per 500 km²") is applied on a `BucketIndex` of about 22.4 km cells (500 km²), deepest candidate first, ties by latitude then longitude bits. | It is the spec's rule, made deterministic and cheap. | Two ponds can sit in neighbouring cells and so closer than the rule suggests. |

## File Structure

| File | Change |
|---|---|
| `src/hydrology/refine.rs` | The crossing pass: a segment index, the intersection test, the straightening (Tasks 2, 3) |
| `src/hydrology/ponds.rs` (new) | The fine search: strips, the fine flood, the keep rule, the density cap, outlines (Tasks 4, 5) |
| `src/hydrology/mod.rs` | New `HydroParams` and `BakeStats` fields; `pub mod ponds;`; `bake()` calls the pond search (Tasks 3, 4, 5) |
| `src/hydrology/record.rs` | SCHEMA 5 (Task 3 for the crossing counts, Task 5 for the pond params) |
| `src/hydrology/bake_tests.rs` | World-scale properties for both features |
| `src/bin/hydro_survey.rs` | Prints crossings, ponds, and the parts' times (Tasks 3, 5, 7) |
| `src/bin/shore_probe.rs` (new, Task 1) | The spike's measurements. It is thrown away at the end of Task 1, and its numbers live in the design note. |
| `viewer/public/app/engine.js`, `water-preview.js`, `world-panel.js` | SCHEMA 5, ponds drawn, the summary line |
| `.github/workflows/gates.yml`, `crates/worldbuilder-engine/README.md` | Pins (Task 7) |
| `docs/superpowers/reports/2026-09-12-water-1b3-outlines-design.md` (new) | Task 1's design note: the measurements, the ruling, and what plan 1b-4 implements |
| spec, carry-forward, a new verification report | The rulings, and what is left |

All paths are relative to `crates/worldbuilder-engine/` unless they start with `viewer/`, `docs/` or `.github/`.

---

### Task 1: The outline spike, and the ruling it produces

**Files:**
- Create: `src/bin/shore_probe.rs` (deleted again at the end of this task)
- Create: `docs/superpowers/reports/2026-09-12-water-1b3-outlines-design.md`
- Modify: `docs/superpowers/specs/2026-09-10-automatic-water-design.md` (§6.6's lake-outline bullet, §7's `outline` field, §8.3's `lake`/`pond` row)

**Interfaces:**
- Consumes: `hydrology::bake_stages`, `record_of`, `LandGraph`, `routing::Routing.lake_of`, `hollows::Hollow`.
- Produces: the design note, and the spec text plan 1b-4 will implement. No engine behaviour changes.

This task answers one question with numbers: **how is a body's extent recorded, so that `water_at` can answer §8.3 and the record still fits 2 MB?**

- [ ] **Step 1: Write the probe.** `src/bin/shore_probe.rs` takes a seed, radius, plate count, land fraction, a node count and an optional `--ranges`, bakes the stages, and prints, for every kept body, in body-id order:

```
id, area_km2, level_m, enclosed, members, collar, collar_span_km, perimeter_est_km, contour_pts_250m_est
```

where:
- `members` is the count of nodes with `lake_of == this hollow`;
- `collar` is the count of distinct non-member nodes adjacent to a member (`graph.neighbours`), sorted and deduplicated;
- `collar_span_km` is the greatest distance between any two collar nodes (sample at most 2,000 collar nodes, evenly by index, and say so in the output if it sampled);
- `perimeter_est_km` is the sum over collar nodes of the mean distance to their adjacent members, as a rough shoreline length, plus a plainly labelled second estimate `2 * sqrt(pi * area)`;
- `contour_pts_250m_est` is `perimeter_est_km * 4`.

It then prints totals: bodies, all members, all collars, the record words the collars would cost (`2 * collar`), and the same for a 250 m contour.

- [ ] **Step 2: Measure.** Run it on:
  - the owner's world's parameters, from `worlds/world-1788998299904.json`'s `planet` block (seed 949766019, radius 9,309,000, 3 plates, land 0.4), at 1,000,000 nodes. The painted features are not in the probe, so say so: the probe measures the unpainted landform, and the owner-world figure that matters (the great lake's area) is already known from plan 1b-2's report.
  - the three 1b-2 stand-ins at 1,000,000 nodes: seed 562,423,712 at radius 4.5e6 with 28 plates and land 0.16 and `ranges()`; seed 20,260,904 at 6,371,000 with 12 plates and land 0.29; seed 1 at 6,371,000 with 12 plates, land 0.40 and `ranges()`.

  Put every table in the design note.

- [ ] **Step 3: Rule, by this rule.** Choose the cheapest representation that satisfies all three of:
  1. **Containment:** every point the level test would call wet lies inside the recorded extent, at every zoom.
  2. **Budget:** at most 1 MB of record added on the owner's world, leaving 1 MB for ponds.
  3. **Buildable without a triangulation:** the graph is k-nearest (`stream.rs::node_neighbours`, k = 8).

  The candidates, to be judged on the measured numbers:
  - **A, the collar ring (Ruling S-1's default):** the collar, ordered into a ring and simplified. Containment holds because collar nodes stand above the body's level, so the level contour lies between the members and the collar. The ordering is the risk: with no triangulation, the ring has to be walked by an angular rule in the tangent plane, and a long, snaking body can defeat a naive walk. **Measure the risk:** implement the walk for the three largest bodies of each world and report, for each, whether the ring closes, whether consecutive ring steps are adjacent in the graph, and whether any two ring edges cross.
  - **B, members plus collar, unordered:** the extent test becomes "the nearest node among this body's members and collar is a member". It needs no ordering and is exactly the coarse graph's own region, but it costs `2 * (members + collar)` words.
  - **C, the 250 m contour of §6.6 as written:** report its estimated cost against the budget, so the note says plainly why it was refused if it was.

  The note must state the choice, the numbers behind it, what it costs if wrong, and — for whichever is chosen — the exact record shape plan 1b-4 will write: what `Body.outline` holds, in what order, and the test `water_at` performs.

- [ ] **Step 4: Write the spec text.** Edit the spec so §6.6's lake-outline bullet, §7's `outline` field and §8.3's `lake`/`pond` row all describe the chosen representation, naming the ruling. Keep the 250 m trace for ponds. If the choice is not A, also correct this plan's Ruling S-1 row and say so in the note.

- [ ] **Step 5: Delete the probe.** `git rm` the probe binary; its numbers live in the note. Confirm the crate still builds with no default features.

- [ ] **Step 6: Commit** `Water 1b-3: how a body's extent is recorded, measured and ruled`.

---

### Task 2: Find where refined reaches cross

**Files:**
- Modify: `src/hydrology/refine.rs`
- Test: `refine.rs`'s test module, and `bake_tests.rs`

**Interfaces:**
- Consumes: `Refined { points, protected, falls }`, `ReachLine`, `Downstream`, `crate::hydrology::buckets::BucketIndex` (`new(radius_m, cell_m)`, `insert(&SpherePoint, u32)`, `candidates(&SpherePoint, reach_m) -> Vec<u32>`), `TangentFrame`.
- Produces, for Task 3:
  - `pub struct Crossing { pub reach_a: u32, pub index_a: usize, pub reach_b: u32, pub index_b: usize }` — `index_*` is the first point of the crossing segment in that reach's point list, and `reach_a < reach_b`.
  - `pub fn crossings(lines: &[Vec<ReachPoint>], downstream: &[Downstream], radius_m: f64) -> Vec<Crossing>` — every pair of segments from **different** reaches that intersect, excluding confluence pairs, sorted by `(reach_a, index_a, reach_b, index_b)`.

- [ ] **Step 1: Write the failing tests** in `refine.rs`'s test module:

```rust
    fn line(points: &[(f64, f64)]) -> Vec<ReachPoint> {
        points.iter().map(|&(lat, lon)| point(lat, lon, 0.0)).collect()
    }

    #[test]
    fn two_lines_that_cross_are_found() {
        // An X: one line west to east, one south to north, crossing near (0, 0.1).
        let a = line(&[(-0.2, 0.0), (0.2, 0.2)]);
        let b = line(&[(0.2, 0.0), (-0.2, 0.2)]);
        let found = crossings(&[a, b], &[Downstream::Ocean, Downstream::Ocean], R);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].reach_a, found[0].index_a, found[0].reach_b, found[0].index_b), (0, 0, 1, 0));
    }

    #[test]
    fn lines_that_only_come_close_are_not_a_crossing() {
        let a = line(&[(0.0, 0.0), (0.0, 0.2)]);
        let b = line(&[(0.001, 0.0), (0.001, 0.2)]);
        assert!(crossings(&[a, b], &[Downstream::Ocean, Downstream::Ocean], R).is_empty());
    }

    #[test]
    fn a_tributary_meeting_its_receiver_is_not_a_crossing() {
        // b ends on a's first point, which is what a junction is (Ruling R-1).
        let a = line(&[(0.0, 0.0), (0.0, 0.2)]);
        let b = line(&[(-0.2, -0.2), (0.0, 0.0)]);
        let found = crossings(&[a, b], &[Downstream::Ocean, Downstream::Reach(0)], R);
        assert!(found.is_empty());
    }

    #[test]
    fn two_tributaries_of_one_receiver_are_not_a_crossing_at_their_junction() {
        let a = line(&[(0.0, 0.0), (0.0, 0.2)]);
        let b = line(&[(-0.2, -0.2), (0.0, 0.0)]);
        let c = line(&[(0.2, -0.2), (0.0, 0.0)]);
        let down = [Downstream::Ocean, Downstream::Reach(0), Downstream::Reach(0)];
        assert!(crossings(&[a, b, c], &down, R).is_empty());
    }

    #[test]
    fn a_reach_crossing_itself_is_not_reported_here() {
        // Self-crossings are a separate question; this pass is about unrelated reaches.
        let a = line(&[(-0.2, 0.0), (0.2, 0.1), (-0.2, 0.1), (0.2, 0.2)]);
        assert!(crossings(&[a], &[Downstream::Ocean], R).is_empty());
    }
```

Run `cargo test -p worldbuilder-engine --lib hydrology::refine::tests::` and expect FAIL (`crossings` is not defined).

- [ ] **Step 2: Implement.** Add to `refine.rs`:

```rust
use crate::hydrology::buckets::BucketIndex;

/// Two refined segments of different reaches that intersect. `index_a` and `index_b` are the
/// first point of each crossing segment in its own reach's list, and `reach_a < reach_b`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Crossing {
    pub reach_a: u32,
    pub index_a: usize,
    pub reach_b: u32,
    pub index_b: usize,
}

/// Do the segments `p0 -> p1` and `q0 -> q1` cross, measured on a tangent plane at `p0`? Shared
/// endpoints and touching ends count as no crossing: a junction is a shared vertex by Ruling R-1,
/// and two lines that merely meet do not need straightening.
fn segments_cross(radius_m: f64, p0: &SpherePoint, p1: &SpherePoint, q0: &SpherePoint, q1: &SpherePoint) -> bool {
    let frame = TangentFrame::at(p0, radius_m);
    let (ax, ay) = (0.0, 0.0);
    let (bx, by) = frame.sphere_to_local(p1);
    let (cx, cy) = frame.sphere_to_local(q0);
    let (dx, dy) = frame.sphere_to_local(q1);
    let side = |x0: f64, y0: f64, x1: f64, y1: f64, x: f64, y: f64| {
        (x1 - x0) * (y - y0) - (y1 - y0) * (x - x0)
    };
    let d1 = side(ax, ay, bx, by, cx, cy);
    let d2 = side(ax, ay, bx, by, dx, dy);
    let d3 = side(cx, cy, dx, dy, ax, ay);
    let d4 = side(cx, cy, dx, dy, bx, by);
    // Strictly opposite sides on both tests. A zero is a touch, not a crossing.
    ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
}

/// Every crossing between segments of different reaches, in a deterministic order. A confluence
/// pair is skipped: a reach and its receiver, or two reaches with the same receiver, share a
/// vertex by design (spec §14.4), and nothing there needs straightening.
///
/// The index is a `BucketIndex` over segment midpoints, at a cell of one refinement step, so the
/// work is proportional to the segments, not to their square.
pub fn crossings(lines: &[Vec<ReachPoint>], downstream: &[Downstream], radius_m: f64) -> Vec<Crossing> {
    // Segment id -> (reach index, point index), and the midpoint that indexes it.
    let mut owner: Vec<(u32, usize)> = Vec::new();
    let mut mid: Vec<SpherePoint> = Vec::new();
    let mut ends: Vec<(SpherePoint, SpherePoint)> = Vec::new();
    let mut longest_m = 0.0;
    for (r, points) in lines.iter().enumerate() {
        for i in 0..points.len().saturating_sub(1) {
            let a = SpherePoint::from_latlon(points[i].lat_deg, points[i].lon_deg);
            let b = SpherePoint::from_latlon(points[i + 1].lat_deg, points[i + 1].lon_deg);
            let span = a.distance_to(&b, radius_m);
            if span > longest_m {
                longest_m = span;
            }
            let frame = TangentFrame::at(&a, radius_m);
            let (bx, by) = frame.sphere_to_local(&b);
            owner.push((r as u32, i)); // cast-ok: reach index, bounded by the reach count
            mid.push(frame.local_to_sphere(bx * 0.5, by * 0.5));
            ends.push((a, b));
        }
    }
    if mid.is_empty() {
        return Vec::new();
    }
    let cell_m = if longest_m > 1.0 { longest_m } else { 1.0 };
    let mut index = BucketIndex::new(radius_m, cell_m);
    for (id, point) in mid.iter().enumerate() {
        index.insert(point, id as u32); // cast-ok: segment index, bounded by the point count
    }
    let related = |a: usize, b: usize| -> bool {
        let (ra, rb) = (owner[a].0 as usize, owner[b].0 as usize);
        matches!(downstream[ra], Downstream::Reach(next) if next as usize == rb)
            || matches!(downstream[rb], Downstream::Reach(next) if next as usize == ra)
            || match (downstream[ra], downstream[rb]) {
                (Downstream::Reach(x), Downstream::Reach(y)) => x == y,
                _ => false,
            }
    };
    let mut found = Vec::new();
    for a in 0..mid.len() {
        for b in index.candidates(&mid[a], cell_m) {
            let b = b as usize;
            if b <= a {
                continue;
            }
            if owner[a].0 == owner[b].0 || related(a, b) {
                continue;
            }
            if !segments_cross(radius_m, &ends[a].0, &ends[a].1, &ends[b].0, &ends[b].1) {
                continue;
            }
            let (first, second) = if owner[a].0 < owner[b].0 { (a, b) } else { (b, a) };
            found.push(Crossing {
                reach_a: owner[first].0,
                index_a: owner[first].1,
                reach_b: owner[second].0,
                index_b: owner[second].1,
            });
        }
    }
    found.sort_unstable_by_key(|c| (c.reach_a, c.index_a, c.reach_b, c.index_b));
    found.dedup();
    found
}
```

Run the tests. Expected: PASS. If `candidates(&mid[a], cell_m)` misses a crossing in the X test because the two midpoints fall more than one cell apart, widen the query to `cell_m * 2.0` and say so in the report — never widen past that without measuring the cost.

- [ ] **Step 3: Count them on real worlds.** Add to `bake_tests.rs`:

```rust
/// Ruling S-2: the coarse record crosses itself a little, and that stays. This test reports both
/// counts, so the crossing pass (Task 3) can be judged against the coarse number rather than
/// against zero.
#[test]
fn crossings_are_counted_coarse_and_refined() {
    for (name, p) in [("params", params()), ("junction", junction_params())] {
        let stages = bake_stages(&world(), &p).expect("stages");
        let coarse = record_of(&stages, &p);
        let coarse_lines: Vec<Vec<ReachPoint>> = coarse.reaches.iter().map(|r| r.points.clone()).collect();
        let down: Vec<Downstream> = coarse.reaches.iter().map(|r| r.downstream).collect();
        let before = crate::hydrology::refine::crossings(&coarse_lines, &down, world().radius_m).len();
        let refined = crate::hydrology::bake(&world(), &p).expect("bake");
        let lines: Vec<Vec<ReachPoint>> = refined.reaches.iter().map(|r| r.points.clone()).collect();
        let after = crate::hydrology::refine::crossings(&lines, &down, world().radius_m).len();
        eprintln!("{name}: coarse {before} refined {after}");
    }
}
```

Run it with `--nocapture` and put both numbers for both worlds in the report. The final review of plan 1b-2 measured 61 coarse against 1,957 refined on a 1M stand-in, so expect the refined count to be much the larger.

- [ ] **Step 4: Commit** `Water 1b-3: find where refined reaches cross`. Rebuild the wasm.

---

### Task 3: Straighten the yielding reach, and record what is left

**Files:**
- Modify: `src/hydrology/refine.rs`, `src/hydrology/mod.rs`, `src/hydrology/record.rs`, `viewer/public/app/engine.js`, `viewer/public/app/water-preview.js`
- Test: `refine.rs`'s test module, `bake_tests.rs`, `record.rs`'s tests, `viewer/test/water-preview.test.mjs`, `viewer/test/hydro.test.mjs`

**Interfaces:**
- Consumes: `crossings`, `Crossing`, and `Refined` (Task 2 and plan 1b-2).
- Produces:
  - `BakeStats.crossings_coarse: u32` and `BakeStats.crossings_left: u32`;
  - record header words 43 and 44, in that order, with `SCHEMA` 5;
  - `refine()` runs the pass, per Rulings S-3 and S-4.

- [ ] **Step 1: Write the failing test** in `refine.rs`'s test module:

```rust
    /// Rulings S-3 and S-4: where two reaches cross, the smaller flow yields its whole coarse
    /// segment back to its chord, and the pass repeats until nothing crosses or three passes are
    /// done.
    #[test]
    fn the_smaller_river_yields_its_segment() {
        // Ground with a valley that pulls both reaches north of their chords, so their traced
        // lines cross even though their chords do not.
        let h = |p: &SpherePoint| {
            let (n, e) = north_east(p);
            let off = if n > 6_000.0 { n - 6_000.0 } else { 6_000.0 - n };
            200.0 - 0.001 * e + 0.02 * off
        };
        let ground = Ground { height_m: &h, radius_m: R, corridor_m: 20_000.0, seed: 3 };
        let big = ReachLine {
            id: 0, class: ReachClass::Great, order: 3, downstream: Downstream::Ocean, fresh: true,
            points: vec![wide(0.0, 0.0, 199.0, 900.0), wide(0.0, 30_000.0 / M_PER_DEG, 169.0, 900.0)],
        };
        let small = ReachLine {
            id: 1, class: ReachClass::Stream, order: 1, downstream: Downstream::Ocean, fresh: true,
            points: vec![point(0.1, 0.0, 199.0), point(-0.1, 30_000.0 / M_PER_DEG, 169.0)],
        };
        let mut record = HydroRecord {
            bodies: Vec::new(),
            reaches: vec![big.clone(), small.clone()],
            notches: Vec::new(),
            falls: Vec::new(),
            stats: stats_for(&params()),
        };
        refine(&mut record, &ground, &params());
        let lines: Vec<Vec<ReachPoint>> = record.reaches.iter().map(|r| r.points.clone()).collect();
        let down: Vec<Downstream> = record.reaches.iter().map(|r| r.downstream).collect();
        assert!(crossings(&lines, &down, R).is_empty(), "the pass left a crossing");
        assert_eq!(record.stats.crossings_left, 0);
        assert!(record.stats.crossings_coarse >= 0);
        // The great river kept its valley; the stream was straightened to its chord.
        let great_offsets = record.reaches[0].points.iter().skip(1).take(3)
            .map(|p| p.lat_deg).any(|lat| lat > 0.01);
        assert!(great_offsets, "the larger river keeps its valley");
    }
```

`stats_for(&params())` is a small test helper that builds a `BakeStats` with every count zero and the params echoed; write it beside the test if `refine.rs`'s tests do not already have one. If the fixture does not actually produce a crossing, adjust the two chords (not the assertions) until it does, and record what you used.

Run it. Expected: FAIL, because nothing straightens.

- [ ] **Step 2: Implement the pass.** In `refine.rs`, give `refine_reach` a way to re-trace one coarse segment with its stations straightened, then add the pass:

```rust
/// Rulings S-3 and S-4: where two reaches cross, the one carrying less flow at the crossing
/// yields — its whole coarse segment goes back to its chord, so it cannot bend into anything new
/// — and the pass repeats until nothing crosses or `MAX_CROSSING_PASSES` is done. Ties go to the
/// larger reach id. Ruling S-2: a crossing the coarse record already had cannot be straightened
/// away, which is why the count that is left is recorded rather than asserted to be zero.
const MAX_CROSSING_PASSES: usize = 3;
```

The pass belongs inside `refine`, between tracing and simplification (Ruling S-4). Shape it like this:

1. Trace every reach as now, but keep each reach's `Refined` **and** the index of the coarse segment each point came from. Add `segment: u32` to `Fine`, and a parallel `Vec<u32>` to `Refined` (`segment_of`), so a crossing's point index names a coarse segment.
2. Loop up to `MAX_CROSSING_PASSES`:
   - run `crossings` over the current lines;
   - if empty, stop;
   - for each crossing, decide the yielder by the flow at the two crossing points (`points[index].flow_m2`), smaller yields, ties to the larger reach id;
   - collect `(reach, coarse segment)` pairs to straighten, deduplicated and sorted;
   - re-trace those segments with every interior station's lateral forced to 0, by calling a new `trace_segment_straight(ground, params, a, b, shore)` that is `trace_segment` with the candidate search skipped (lateral always 0). Keep the bed rule, the shore trim, the falls and the meander gate exactly as they are.
3. Record `crossings_coarse` (the count over the coarse lines, before tracing) and `crossings_left` (after the last pass).
4. Then meander and simplify, as now.

Write `trace_segment_straight` by giving `trace_segment` a private `straight: bool` and keeping the public signature, or by a small shared helper — whichever keeps the function readable. Do not duplicate the bed, fall or meander code.

Run the Step 1 test. Expected: PASS.

- [ ] **Step 3: SCHEMA 5.** Add `crossings_coarse` and `crossings_left` to `BakeStats`, encode them as header words 43 and 44, decode them, and set `SCHEMA` to `5.0` with a doc comment saying what SCHEMA 5 added. Update the header length everywhere it appears (45 words) in `record.rs`, `engine.js::hydroSummary` (expose both as `crossingsCoarse` and `crossingsLeft`), `water-preview.js::decodeHydro`, and their tests, including the schema-refusal tests.

- [ ] **Step 4: World-scale test.** In `bake_tests.rs`, extend `crossings_are_counted_coarse_and_refined` into a property:

```rust
/// Rulings S-2 and S-3: after the pass, the refined lines cross no more often than the coarse
/// ones they came from, and the record says so.
#[test]
fn refinement_adds_no_crossings() {
    for (name, p) in [("params", params()), ("junction", junction_params())] {
        let stages = bake_stages(&world(), &p).expect("stages");
        let coarse = record_of(&stages, &p);
        let coarse_lines: Vec<Vec<ReachPoint>> = coarse.reaches.iter().map(|r| r.points.clone()).collect();
        let down: Vec<Downstream> = coarse.reaches.iter().map(|r| r.downstream).collect();
        let before = crate::hydrology::refine::crossings(&coarse_lines, &down, world().radius_m).len();
        let refined = crate::hydrology::bake(&world(), &p).expect("bake");
        let lines: Vec<Vec<ReachPoint>> = refined.reaches.iter().map(|r| r.points.clone()).collect();
        let after = crate::hydrology::refine::crossings(&lines, &down, world().radius_m).len();
        eprintln!("{name}: coarse {before} refined {after}");
        assert!(after <= before, "{name}: refinement left {after} crossings against {before} coarse");
        assert_eq!(refined.stats.crossings_coarse as usize, before);
        assert_eq!(refined.stats.crossings_left as usize, after);
    }
}
```

Also add a `ranges_world()` population to that loop, the same world `every_fall_is_a_step_on_its_own_reach` uses.

Run it. If `after <= before` does not hold within three passes, do not loosen it: report the numbers and the worst pair, and stop for a ruling.

- [ ] **Step 5: Keep plan 1b-2's properties green.** Run the whole hydrology suite. `refined_beds_never_rise`, `refined_tributaries_share_their_junction_vertex`, `refinement_keeps_every_coarse_point_in_order`, `no_inland_station_stands_on_sea_ground`, `every_refined_mouth_is_at_or_below_its_water` and `every_fall_is_a_step_on_its_own_reach` must all still pass. A straightened segment is still traced, so they should.

- [ ] **Step 6: Mutation guard.** Temporarily make the yielder always the larger flow instead of the smaller. `the_smaller_river_yields_its_segment` must FAIL. Restore it and paste both outputs into the report.

- [ ] **Step 7: Commit** `Water 1b-3: the smaller river yields where two cross`. Rebuild the wasm, and run all five gates.yml feature configurations plus `no_std_math`. Do not re-pin CI counts; Task 7 does that.

---

### Task 4: The fine search finds hollows along the rivers

**Files:**
- Create: `src/hydrology/ponds.rs`
- Modify: `src/hydrology/mod.rs` (`pub mod ponds;`)
- Test: `ponds.rs`'s test module

**Interfaces:**
- Consumes: `Ground` (`refine.rs`, with `Ground::for_surface`), `ReachLine`, `ReachPoint`, `LandGraph`, `TangentFrame`, `BucketIndex`, `HydroParams`.
- Produces, for Task 5:
  - `pub struct Strip { pub frame: TangentFrame, pub along_m: f64, pub cells_across: usize, pub steps: usize, pub ground_m: Vec<f64> }` — a lane along one refined reach, `steps` long and `cells_across` wide, row-major from the upstream end, with lateral 0 at the middle column.
  - `pub fn strips(reach: &ReachLine, ground: &Ground, params: &HydroParams) -> Vec<Strip>` — one strip per refined segment of the reach, sampled at `pond_cell_m`, `pond_search_radius_m` either side.
  - `pub struct Candidate { pub anchor: SpherePoint, pub level_m: f64, pub floor_m: f64, pub area_m2: f64, pub cells: Vec<(usize, usize)>, pub strip: usize }`
  - `pub fn hollows_in(strip: &Strip, strip_index: usize, params: &HydroParams) -> Vec<Candidate>` — every fine hollow in one strip whose depth and area pass the pond keep rule, deepest first, ties by row then column.

- [ ] **Step 1: Write the failing tests** in a new `#[cfg(test)] mod tests` in `ponds.rs`:

```rust
    const R: f64 = 6_371_000.0;
    const M_PER_DEG: f64 = R * std::f64::consts::PI / 180.0;

    fn params() -> HydroParams {
        HydroParams::earth_like(1_000)
    }

    fn reach_along_the_equator(km: f64) -> ReachLine {
        let end = (km * 1_000.0) / M_PER_DEG;
        ReachLine {
            id: 0, class: ReachClass::Stream, order: 1, downstream: Downstream::Ocean, fresh: true,
            points: vec![
                ReachPoint { lat_deg: 0.0, lon_deg: 0.0, bed_m: 100.0, width_m: 5.0, depth_m: 1.0, flow_m2: 1.0e9 },
                ReachPoint { lat_deg: 0.0, lon_deg: end, bed_m: 90.0, width_m: 5.0, depth_m: 1.0, flow_m2: 1.0e9 },
            ],
        }
    }

    #[test]
    fn a_strip_covers_the_search_radius_at_the_cell_size() {
        let h = |_: &SpherePoint| 100.0;
        let ground = Ground { height_m: &h, radius_m: R, corridor_m: 20_000.0, seed: 1 };
        let p = params();
        let strip = &strips(&reach_along_the_equator(10.0), &ground, &p)[0];
        // 3 km either side at 250 m: 12 cells each way plus the middle.
        assert_eq!(strip.cells_across, 25);
        assert_eq!(strip.steps, 40, "10 km at 250 m");
        assert_eq!(strip.ground_m.len(), 25 * 40);
    }

    #[test]
    fn a_dip_beside_the_river_is_a_candidate() {
        // A bowl 4 m deep and 1 km across, centred 1 km north of the line at 5 km along.
        let h = |p: &SpherePoint| {
            let (lat, lon) = p.to_latlon();
            let (n, e) = (lat * M_PER_DEG, lon * M_PER_DEG);
            let dn = n - 1_000.0;
            let de = e - 5_000.0;
            let r2 = dn * dn + de * de;
            if r2 < 500.0 * 500.0 { 100.0 - 4.0 * (1.0 - r2 / (500.0 * 500.0)) } else { 100.0 }
        };
        let ground = Ground { height_m: &h, radius_m: R, corridor_m: 20_000.0, seed: 1 };
        let p = params();
        let found: Vec<Candidate> = strips(&reach_along_the_equator(10.0), &ground, &p)
            .iter().enumerate().flat_map(|(i, s)| hollows_in(s, i, &p)).collect();
        assert_eq!(found.len(), 1, "one bowl, one candidate");
        let c = &found[0];
        assert!(c.level_m - c.floor_m >= p.pond_keep_depth_m, "depth {}", c.level_m - c.floor_m);
        assert!(c.area_m2 >= p.pond_keep_area_m2, "area {}", c.area_m2);
        let (lat, lon) = c.anchor.to_latlon();
        let (n, e) = (lat * M_PER_DEG, lon * M_PER_DEG);
        assert!(n > 500.0 && n < 1_500.0 && e > 4_500.0 && e < 5_500.0, "anchor at {n}, {e}");
    }

    #[test]
    fn a_dip_under_two_metres_is_not_a_candidate() {
        let h = |p: &SpherePoint| {
            let (lat, lon) = p.to_latlon();
            let (n, e) = (lat * M_PER_DEG, lon * M_PER_DEG);
            let dn = n - 1_000.0;
            let de = e - 5_000.0;
            let r2 = dn * dn + de * de;
            if r2 < 500.0 * 500.0 { 100.0 - 1.5 * (1.0 - r2 / (500.0 * 500.0)) } else { 100.0 }
        };
        let ground = Ground { height_m: &h, radius_m: R, corridor_m: 20_000.0, seed: 1 };
        let p = params();
        let found: usize = strips(&reach_along_the_equator(10.0), &ground, &p)
            .iter().enumerate().map(|(i, s)| hollows_in(s, i, &p).len()).sum();
        assert_eq!(found, 0);
    }

    #[test]
    fn a_slope_with_no_dip_has_no_candidate() {
        let h = |p: &SpherePoint| { let (_, lon) = p.to_latlon(); 100.0 - 0.002 * lon * M_PER_DEG };
        let ground = Ground { height_m: &h, radius_m: R, corridor_m: 20_000.0, seed: 1 };
        let p = params();
        let found: usize = strips(&reach_along_the_equator(10.0), &ground, &p)
            .iter().enumerate().map(|(i, s)| hollows_in(s, i, &p).len()).sum();
        assert_eq!(found, 0);
    }
```

Run `cargo test -p worldbuilder-engine --lib hydrology::ponds`. Expected: FAIL, nothing defined.

- [ ] **Step 2: Add the params** to `HydroParams`, with these `earth_like` values (spec §6.6):

```rust
    /// Spec §6.6: the fine search's cell.
    pub pond_cell_m: f64,              // 250.0
    /// How far either side of a refined reach the fine search looks.
    pub pond_search_radius_m: f64,     // 3_000.0
    /// The pond keep rule's depth.
    pub pond_keep_depth_m: f64,        // 2.0
    /// The pond keep rule's area.
    pub pond_keep_area_m2: f64,        // 50_000.0
    /// Ruling S-6: a candidate's terrain must be wetter than this share of the graph's land
    /// nodes, measured at the nearest node.
    pub pond_wetness_share: f64,       // 0.6
    /// Ruling S-6: and flatter than this, over one pond cell.
    pub pond_max_slope: f64,           // 0.03
    /// Ruling S-8: at most one kept body per this much searched area.
    pub pond_density_area_m2: f64,     // 5.0e8
```

Validate each in `bake_stages` with the existing `require_finite_positive`, and add floors: `pond_cell_m >= 10.0`, `pond_search_radius_m >= pond_cell_m`, `pond_wetness_share <= 1.0`. Test the floors. Fix every `HydroParams` literal the compiler names.

- [ ] **Step 3: Implement `strips`.** One strip per refined segment. For a segment `a -> b`:
- the frame is `TangentFrame::at(a, radius_m)`, `u` along the chord and `v` to its left, as `refine.rs` does;
- `steps` is `-m::floor(-(len / params.pond_cell_m))` as a `usize`, bounded by a `MAX_STRIP_CELLS` constant (use 4,000,000 cells per strip, and skip a segment that would exceed it, counting the skip in the returned stats or a `Vec` the caller can read);
- `cells_across` is `2 * k + 1` where `k = -m::floor(-(params.pond_search_radius_m / params.pond_cell_m))`;
- `ground_m[row * cells_across + column]` is the landform at along `row * cell`, lateral `(column as f64 - k as f64) * cell`.

- [ ] **Step 4: Implement `hollows_in`.** Inside one strip, on its own 4-neighbour grid:
1. a priority flood from every edge cell of the strip, exactly as `flood.rs` does on the graph but on the grid, giving each cell a spill level (reuse `heap::FloodQueue` with `sortable`);
2. a hollow is a connected run of cells whose spill exceeds their own ground, at one spill level, found in row-major order;
3. its `floor_m` is the lowest ground, `level_m` the spill, `area_m2` the cell count times `pond_cell_m * pond_cell_m`, and its `anchor` the lowest cell (ties to the lower row, then column);
4. keep it only if `level_m - floor_m >= pond_keep_depth_m` **and** `area_m2 >= pond_keep_area_m2`;
5. sort the kept ones by depth descending with `total_cmp`, ties by row then column.

A hollow touching the strip's edge is kept: the strip is a window, and Task 5's dedup handles a pond that two strips both see.

Run the Step 1 tests. Expected: PASS.

- [ ] **Step 5: A determinism test.** Add: the same strip sampled twice gives bit-identical `ground_m`, and `hollows_in` gives the same candidates in the same order. Run it.

- [ ] **Step 6: Commit** `Water 1b-3: the fine search samples strips along the rivers`. Run all five gates.yml configurations and `no_std_math`. Rebuild the wasm.

---

### Task 5: Ponds in the record

**Files:**
- Modify: `src/hydrology/ponds.rs`, `src/hydrology/mod.rs`, `src/hydrology/record.rs`
- Test: `ponds.rs`'s test module, `bake_tests.rs`, `record.rs`'s tests, `viewer/test/*.mjs`

**Interfaces:**
- Consumes: Task 4's `strips`, `hollows_in` and `Candidate`; the record's `Body`, `BodyKind` and `Downstream`; `LandGraph.wetness`; `Routing.lake_of`.
- Produces:
  - `pub fn search(record: &mut HydroRecord, graph: &LandGraph, lake_of: &[u32], ground: &Ground, params: &HydroParams)` — appends kept ponds to `record.bodies` and fills in the pond stats;
  - `BakeStats.ponds_found: u32`, `ponds_kept: u32`, and the seven pond params echoed;
  - record header words 45 to 53: `ponds_found`, `ponds_kept`, `pond_cell_m`, `pond_search_radius_m`, `pond_keep_depth_m`, `pond_keep_area_m2`, `pond_wetness_share`, `pond_max_slope`, `pond_density_area_m2`. The header becomes 54 words and stays SCHEMA 5 (Task 3 bumped it).

- [ ] **Step 1: Write the failing tests.**

In `ponds.rs`:

```rust
    /// Ruling S-8: the density cap keeps the deepest first, one per cell of about 22.4 km.
    #[test]
    fn the_density_cap_keeps_the_deepest() {
        // Two bowls 2 km apart, 5 m and 3 m deep: the same 500 km cell, so only the deeper stays.
        // (Build them with the bowl closure from Task 4's tests, at two centres.)
        // ... assert one kept body, and that its depth is about 5 m.
    }

    /// Ruling S-7: a candidate inside a coarse lake, or on ground at or below the datum, is dropped.
    #[test]
    fn a_candidate_inside_a_coarse_lake_is_dropped() {
        // ... one candidate whose anchor's nearest graph node is a lake member: 0 kept.
    }
```

Write both out in full, using Task 4's bowl closure and a small hand `LandGraph` (the `line_stages` pattern in `bake_tests.rs` shows one).

In `bake_tests.rs`:

```rust
/// Spec §6.6 on a real bake: every pond obeys its own keep rule, sits on its own ground, and
/// names the river it drains to (Ruling S-5).
#[test]
fn ponds_obey_their_keep_rule_and_name_a_river() {
    let p = params();
    let record = crate::hydrology::bake(&world(), &p).expect("bake");
    let coarse = record_of(&bake_stages(&world(), &p).expect("stages"), &p);
    assert!(record.bodies.len() >= coarse.bodies.len(), "ponds are appended, never inserted");
    for (id, body) in record.bodies.iter().enumerate() {
        assert_eq!(body.id as usize, id, "body ids stay the wire index");
    }
    for body in &record.bodies[coarse.bodies.len()..] {
        assert!(body.depth_m >= p.pond_keep_depth_m, "pond {} is {} m deep", body.id, body.depth_m);
        assert!(body.area_m2 >= p.pond_keep_area_m2);
        assert!(body.fresh);
        assert!(matches!(body.downstream, Downstream::Reach(_)), "Ruling S-5");
        assert!(body.outlet_reach.is_none(), "Ruling S-5");
        assert!(body.outline.len() >= 3, "a pond has a traced outline");
        assert_eq!(body.kind, if body.area_m2 < p.pond_max_area_m2 { BodyKind::Pond } else { BodyKind::Lake });
    }
    eprintln!("coarse bodies {} ponds {}", coarse.bodies.len(), record.bodies.len() - coarse.bodies.len());
}
```

Run both. Expected: FAIL.

- [ ] **Step 2: Implement `search`.** In order:
1. **The wetness gate (Ruling S-6):** collect the land nodes' `graph.wetness`, sort with `total_cmp`, and take the `pond_wetness_share` quantile (index `floor(share * (len - 1))`). A candidate whose anchor's nearest graph node (`BucketIndex::nearest` over the graph positions) has wetness below that is dropped.
2. **Strips and candidates:** for every reach, in id order, run `strips` then `hollows_in`, tagging each candidate with its reach id.
3. **The slope gate (Ruling S-6):** drop a candidate whose ground rises more than `pond_max_slope` over one cell in either direction at its anchor, measured from the strip.
4. **Ruling S-7:** drop a candidate whose anchor ground is at or below 0, or whose nearest graph node has `lake_of != NO_LAKE`.
5. **Dedup:** two strips can see one pond. Drop a candidate whose anchor is within `pond_cell_m * 2.0` of an already kept one.
6. **The density cap (Ruling S-8):** sort all surviving candidates by depth descending (`total_cmp`), ties by latitude then longitude bits; walk them, keeping one per `BucketIndex` cell of `sqrt(pond_density_area_m2)`.
7. **The outline:** trace each kept candidate's cells at `pond_cell_m` — the boundary of the cell set, walked in a fixed direction (a marching-squares walk on the strip grid, starting from the lowest-row, lowest-column boundary cell, turning the same way each time), converted through the strip's frame, then simplified with `refine::simplify`-style Douglas–Peucker at `pond_cell_m` tolerance. It must be a closed ring of at least 3 points, in a fixed winding.
8. **The body:** `id` continues from the coarse bodies; `kind` by `pond_max_area_m2`; `fresh` true; `enclosed` false; `forced` false; `level_m`, `area_m2` and `depth_m` from the candidate; `outlet_reach` `None`; `anchor` the candidate's anchor; `downstream` `Reach(r)` for the nearest refined reach line (Ruling S-5), measured from the anchor over the reaches' points with a `BucketIndex`.
9. **The stats:** `ponds_found` is every candidate that passed the keep rule before the gates and the cap, and `ponds_kept` is what reached the record.

Then call it from `bake()` in `mod.rs`, after `refine::refine`, passing `stages.routing.lake_of`. Extend `BakeStages` or pass what is needed; do not clone the graph.

- [ ] **Step 3: SCHEMA 5's remaining words.** Add the nine header words above, in that order, to `encode`, `decode`, `engine.js::hydroSummary` (expose `pondsFound` and `pondsKept`), `water-preview.js::decodeHydro` (all nine), and their tests. The header is now 54 words.

- [ ] **Step 4: Run and measure.** Run the Step 1 tests, then the whole hydrology suite, then all five gates.yml configurations. Report:
- ponds found and kept on `params()`, `junction_params()` and `ranges_world()`;
- the bake time before and after this task on the test world, in release;
- the record's word count before and after.

- [ ] **Step 5: Mutation guard.** Temporarily drop the density cap (keep every candidate). `the_density_cap_keeps_the_deepest` must FAIL. Restore it, and paste both outputs.

- [ ] **Step 6: Commit** `Water 1b-3: small lakes and ponds along the rivers`. Rebuild the wasm.

---

### Task 6: The studio draws them

**Files:**
- Modify: `viewer/public/app/water-preview.js`, `viewer/public/app/world-panel.js`
- Test: `viewer/test/water-preview.test.mjs`

**Interfaces:**
- Consumes: SCHEMA 5's decoded record, including `bodies[].outline` and the new header words.
- Produces: ponds drawn as outlines rather than points, and a summary line that names them.

- [ ] **Step 1: Write the failing test** in `viewer/test/water-preview.test.mjs`, in the style of the falls test added in plan 1b-2 (a Cesium stand-in, two hand-made bodies):

```javascript
test("drawPreview draws a pond's outline and counts it", () => {
  const decoded = {
    header: { schema: 5, pondsKept: 1, crossingsLeft: 0 },
    bodies: [
      { id: 0, kind: "lake", fresh: true, levelM: 100, areaM2: 4e6, depthM: 9, anchor: [1, 1], outline: [], downstream: { kind: "ocean" }, outletReach: null },
      { id: 1, kind: "pond", fresh: true, levelM: 90, areaM2: 6e4, depthM: 3, anchor: [2, 2], outline: [[2, 2], [2.001, 2], [2.001, 2.001], [2, 2.001]], downstream: { kind: "reach", id: 0 }, outletReach: null },
    ],
    reaches: [], notches: 0, falls: [],
  };
  const layer = drawPreview(stubViewer(), stubCesium(), decoded);
  assert.equal(layer.counts.ponds, 1);
  assert.equal(layer.counts.pondOutlines, 1);
  layer.remove();
});
```

Match the existing stubs' shape; read the falls test first. Run `node --test test/water-preview.test.mjs`. Expected: FAIL.

- [ ] **Step 2: Implement.** In `drawPreview`:
- a body with a non-empty `outline` is drawn as a polygon (`clampToGround: true`), filled in the fresh or salt colour at about 60% opacity, with its own outline in the same hue;
- a body with an empty outline keeps today's point;
- `counts` gains `ponds` and `pondOutlines`;
- each pond's description names its depth, its area in hectares, and the reach it drains to.

In `world-panel.js`, extend the summary line with `, P ponds` and `, X crossings left`, reading `header.pondsKept` and `header.crossingsLeft`.

- [ ] **Step 3: Run** `npm test` in `viewer/` and report the count. Expected: no new failures.

- [ ] **Step 4: Commit** `Water 1b-3: the studio draws ponds and says how many crossings are left`.

---

### Task 7: Survey, pins, and the owner's world

**Files:**
- Modify: `src/bin/hydro_survey.rs`, `.github/workflows/gates.yml`, `crates/worldbuilder-engine/README.md`, `docs/superpowers/reports/2026-09-10-water-1a-carry-forward.md`
- Create: `docs/superpowers/reports/2026-09-12-water-1b3-verification.md`

- [ ] **Step 1: The survey.** `hydro_survey` times the pond search as its own part, beside `bake_stages`, `record_of` and `refine`, and prints crossings coarse and left, ponds found and kept, the record bytes, and the reach-point count. Run it on the three 1M stand-ins from Task 1's Step 2 and put the table in the report.

- [ ] **Step 2: The gates.**
  - **Size:** each stand-in's record is at most 8,000,000 bytes. If one is over, raise `pond_density_area_m2` (fewer ponds) in steps of ×2 until it fits, and record the value with the numbers.
  - **Time:** the pond search takes at most 120 s natively on each stand-in. If it is over, raise `pond_cell_m` to 500, re-measure, and record it. If it is still over, STOP and report the numbers.

- [ ] **Step 3: Rebuild, parity, pins.** Rebuild the wasm, run parity from `crates/worldbuilder-engine/parity/` as gates.yml does (0 divergent), then re-derive every pin by running it: the five engine configurations, the parity totals and every control including the native prediction, and the Python pin. Update gates.yml and the README mirror with a dated note (2026-09-12, plan 1b-3) giving old and new values. Run the ignored drain sweep.

- [ ] **Step 4 (controller): The owner's world.** The controller bakes `worlds/world-1788998299904.json` at 1M nodes with one forced outlet at 0°N 0°E, in the branch studio on :8138, and hands back: the time; the record bytes; bodies, coarse and ponds; reaches; crossings coarse and left; ponds found and kept; and the checks that beds never rise, junctions are shared, mouths are at or below their water, and every pond obeys Rulings S-5 and S-7. Gates: at most 8 MB, at most 300 s.

- [ ] **Step 5: Write the verification report**, `docs/superpowers/reports/2026-09-12-water-1b3-verification.md`, in the form of plan 1b-2's: population, method and host for every figure, the stand-in table, the owner-world table against 1b-2's, and the pins. State plainly:
- how many crossings the pass removed, and how many the coarse record still has (Ruling S-2);
- how many ponds the owner's world gained;
- Task 1's outline ruling, and that plan 1b-4 implements it.

  Update the carry-forward: tick the crossings item, tick small lakes and ponds, and leave lake outlines pointing at plan 1b-4 with Task 1's ruling named.

- [ ] **Step 6: Commit** `Water 1b-3: pins re-derived, and the shores verified on the owner's world`.

---

## Self-review

**Spec coverage:**

| Spec item | Where |
|---|---|
| §6.6 small lakes and ponds: 250 m cells, within 3 km of refined lines, wetness and slope gates, the pond keep rule, one per 500 km² deepest first | Tasks 4 and 5 |
| §6.6 the corridor "cannot cross into another basin" | Tasks 2 and 3, with Ruling S-2 on what is left |
| §6.6 lake outlines at 250 m | Task 1's spike and ruling; plan 1b-4 implements it |
| §7 `outline`, params echo, body fields | Tasks 1, 3 and 5 |
| §7 size target | Task 7's gate |
| §8.3 the `lake` / `pond` row of the query | Task 1's spec text |
| §14.1 determinism | Task 4's Step 5, plus parity in Task 7 |
| §14.2 everything drains | nothing here changes routing; the sweep runs in Tasks 3, 4 and 7 |
| §14.3 no pit lakes | the pond keep rule is checked in Task 5's world test |
| §14.4, §14.5 | plan 1b-2's tests, kept green in Task 3's Step 5 |
| §14.6 lakes drain | ponds name a river by Ruling S-5, asserted in Task 5 |
| §14.10 mutation guards | Tasks 3 and 5 |

**Types:** `Crossing`, `crossings`, `segments_cross`, `trace_segment_straight`, `Strip`, `strips`, `Candidate`, `hollows_in` and `search` are used with the same names and signatures wherever they appear. Header words are 43–44 in Task 3 and 45–53 in Task 5, giving a 54-word SCHEMA 5 header in both the Rust and the JS twins.

**Known soft spots, named rather than hidden:**
- Task 1 may replace Ruling S-1. Tasks 2 to 7 do not depend on which way it goes; only plan 1b-4 does.
- Task 3's `after <= before` may not hold in three passes on some world. The plan says to stop and report rather than loosen it.
- Task 4's strip sampling is the expensive part of this plan. Task 7's time gate is what decides whether `pond_cell_m` stays at 250 m.
