# Water 1b-4: Body Extents Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every kept body carries the extent `water_at` will decide from — its shore points and the reach of its shore band — so stage 2 can ask "is this point in that lake?" and get the same answer at every zoom.

**Architecture:** Plan 1b-3's Task 1 ruled the representation by measurement: **Candidate B**, an unordered set of shore points with a nearest-point test, because the ring the spec imagined cannot be built on a k-nearest graph and a 250 m shoreline of the great lake alone would cost 3.3 MB. This plan puts that on the wire. The discriminator lands first, in its own task, because filling a coarse body's `outline` before a reader can tell a point set from a curve would make every existing consumer join a lake's shore points into a polygon. Then the extent is computed from `routing.lake_of` and `LandGraph::neighbours` — no walk, no ordering, no tangent plane — and the trim that makes it affordable is put on trial with the test the design note demands.

**Tech Stack:** Rust (`crates/worldbuilder-engine`, detmath only), wasm via `npm run build:wasm`, the viewer's ES modules with `node --test`.

**Spec:** `docs/superpowers/specs/2026-09-10-automatic-water-design.md`. Binding sections: §7 (the record), §8.2 (the index), §8.3 (the query), §14 (properties). The ruling this plan implements is `docs/superpowers/reports/2026-09-12-water-1b3-outlines-design.md` §5.3, §5.6 and §5.7; read all three before Task 2.

**Branch:** `water-extents`, from master at bdbb35a (plan 1b-3 merged).

## The one hard ordering constraint

Spec §8.3 already states the reader's rule: a body with `shore_member_count == 0` carries a traced curve, everything else carries a shore-point set. **That field is not on the wire.** Today the only working discriminator is `outline.length > 0`, which happens to be right only because coarse bodies ship an empty outline — `viewer/public/app/water-preview.js` says so at its branch, naming this plan.

So Task 1 adds `shore_member_count` and `shore_reach_m` to `Body`, to SCHEMA 6 and to all four twins, **while every coarse extent is still empty**. Task 2 fills them. Doing it the other way round ships a record whose lakes are silently readable as polygons, which is exactly what spec §7 forbids.

## Global Constraints

- **No std float maths outside `detmath.rs`.** `tests/no_std_math.rs` scans `src/`, bins and test modules included — and after Task 5 it scans `examples/` too. Use `crate::detmath as m`. There is no `ceil`: write `-m::floor(-x)`. No `.abs()`: write the comparison.
- **Casts:** `as u32`, `as u64`, `as i32`, `as i64` and float→`usize` casts need `// cast-ok: <reason>` on the same line.
- **No `f64::min`, `f64::max` or `.clamp(`.** Write explicit `if`/`else`. Sort floats with `total_cmp`.
- **No panics reachable from `extern "C"`** (`wb_hydro_bake` calls `hydrology::bake`).
- **Determinism:** no HashMap or HashSet order in output. Every sort total, every tie-break stated. Node order is the tie-break of record: ascending node index.
- **`Surface` gains no field.** Commit subjects name no third party. End every commit message with a blank line, then exactly `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`.
- **Every `src/` edit changes the fingerprint.** Rebuild the wasm (`npm run build:wasm` in `viewer/`) in the tasks that say so, and commit it.
- **The fingerprint hashes working-tree bytes.** `.gitattributes` marks the crate's sources `eol=lf`. A file rewritten with CRLF makes CI call the shipped wasm stale in two jobs while the binary is correct. Before any wasm rebuild, run `git ls-files --eol crates/worldbuilder-engine | grep -v "w/lf"` — if it prints anything, delete those files and `git checkout --` them first. Plan 1b-3 lost an hour to this.
- **The record layout has four twins**, and they change together: `src/hydrology/record.rs`, `viewer/public/app/engine.js::hydroSummary`, `viewer/public/app/water-preview.js::decodeHydro`, and `crates/worldbuilder-engine/tests/wasm_exports.rs` (the only header assertion that crosses the `extern "C"` boundary).
- **CI pins** (currently engine 786/786/788/892/894 with 7 ignored; parity 147,553 compared / 0 divergent, seed control 141,765, tectonic control 21,783; Python 565/157) are re-derived once, in the last task, by running them. The crate README mirrors `.github/workflows/gates.yml`.
- **"Everything drains" is binding.** Nothing in this plan changes routing; `drainage_check` and `cargo test --release -p worldbuilder-engine --lib every_small_world_drains -- --ignored` must stay green.
- **Plan 1b-2 and 1b-3's properties stay green:** beds never rise, junctions are shared bit for bit, coarse points are kept, no inland station on sea ground except where its chord is, mouths at or below their water, falls are steps on their own reach, a yielded segment ships on its chord, and the shipped lines cross no more often than the coarse ones.
- **Budget:** the owner's world stands at 7,019,992 of 8,000,000 bytes, so this plan has **about 980 KB**. The design note estimates the trimmed extent at 229 KB on that world and the untrimmed fallback at 890 KB. Task 6 measures it; if the trimmed form is over, the ponds' half of the budget is the first place to look, not the bodies'.
- **Time:** the whole wasm bake stays at most 300 s. It measured 242 s in plan 1b-3.
- **Verification numbers** come from runs, each stated with its population, method and host.

## Rulings made while writing this plan

| # | Ruling | Why | Cost if wrong |
|---|---|---|---|
| E-1 | The extent's points are recorded **shore members first, then collar**, each ascending by node index, and `shore_member_count` says where the split is. No other order is meaningful. | The design note's §5.6 test needs the two sets apart, and ascending node index is the tie-break the rest of the record already uses. | None; the order carries no geometry. |
| E-2 | A **shore member** is a member with at least one neighbour that is not a member of the same body. A **collar node** is a non-member with at least one member neighbour, deduplicated. Interior members are not recorded (the trim). | The measured 32× saving on the great lake: 37,844 members become 1,178 shore members. | Task 3 is the trial; if it fails, Ruling E-5 restores the interior members. |
| E-3 | `shore_reach_m` is the **longest usable** member-to-collar edge, where usable means the collar end's landform stands above the body's level. Edges whose collar end is at or below the level are excluded — the design note measured them at 1.0–2.7% of shore edges, all of them dry ground downhill of a perched rim. | It is what makes the shore band contain the level contour without reaching past the rim any further than one edge. | A shore band up to one graph spacing too wide, on at most 2.7% of a shore. |
| E-4 | An **enclosed** body's level is the datum, so its usable edges are those whose collar stands above 0. No special case: E-3 reads `level_m`, which is already 0 for those bodies. | One rule, and `hollows::find_hollows` already sets `level_m = 0.0` when enclosed. | None. |
| E-5 | If Task 3's interior test fails, the trim is abandoned for **every** body — interior members are recorded too — rather than per body. | The design note's fallback is measured whole (0.890 MB estimated on the painted bake); a per-body mix would need a second flag on the wire and would make the record's cost depend on terrain. | The record grows by an estimated 660 KB on the owner's world, leaving about 110 KB of headroom. |
| E-6 | Ponds keep `shore_member_count = 0` and `shore_reach_m = 0.0` and change in no way. | Ruling S-11 and T1-2 already settled that a pond is a traced curve; this plan does not reopen it. | None. |
| E-7 | `water_at` itself is **not** built here. This plan records the extent and tests the geometry that makes it answerable; the query, the index and the carving are stage 2. | The plan family's stages are the spec's, and stage 2 owns §8.2 and §8.3's implementation. | None. |

## File Structure

| File | Change |
|---|---|
| `src/hydrology/mod.rs` | `Body` gains `shore_member_count: u32` and `shore_reach_m: f64`; `BakeStats` gains two counts (Task 1) |
| `src/hydrology/record.rs` | SCHEMA 6: two body words, two header words (Task 1) |
| `src/hydrology/extent.rs` (new) | The extent: shore members, collar, `shore_reach_m` (Task 2) |
| `src/hydrology/bake.rs` | `record_of` fills each coarse body's extent (Task 2) |
| `src/hydrology/bake_tests.rs` | The world-scale properties, and Task 3's interior trial |
| `src/hydrology/refine.rs` → `src/hydrology/refine/` | Split, and the crossing pass's cost (Task 5) |
| `src/hydrology/ponds.rs` | The refused-candidate dedup anchor (Task 5) |
| `crates/worldbuilder-engine/parity/parity_dump.rs` (or wherever the corpus is built — read it) | A pond body always compared (Task 4) |
| `tests/no_std_math.rs` | Scans `examples/` too (Task 5) |
| `tests/wasm_exports.rs` | The 56-word header (Task 1) |
| `viewer/public/app/engine.js`, `water-preview.js`, `viewer/test/*.mjs` | SCHEMA 6, and the discriminator changing meaning (Tasks 1, 2) |
| `.github/workflows/gates.yml`, `crates/worldbuilder-engine/README.md` | Pins (Task 6) |
| spec §8.2, §9, §14; the carry-forward; a new verification report | Tasks 5 and 6 |

All paths are relative to `crates/worldbuilder-engine/` unless they start with `viewer/`, `docs/` or `.github/`.

---

### Task 1: The discriminator on the wire

**Files:**
- Modify: `src/hydrology/mod.rs`, `src/hydrology/record.rs`, `tests/wasm_exports.rs`, `viewer/public/app/engine.js`, `viewer/public/app/water-preview.js`
- Test: `src/hydrology/record.rs`'s tests, `src/hydrology/bake_tests.rs`, `viewer/test/water-preview.test.mjs`, `viewer/test/hydro.test.mjs`

**Interfaces:**
- Produces: `Body.shore_member_count: u32` and `Body.shore_reach_m: f64`, both zero for every body this task ships.
- Produces: `BakeStats.shore_members: u32` and `BakeStats.collar_points: u32`, both zero for now.
- Produces: SCHEMA `6.0`. The **body** grows from 14 fixed words to 16: `shore_member_count` and `shore_reach_m` go **after `downstream_id` and before `outline_len`**. The **header** grows from 54 to 56: `shore_members` then `collar_points`, appended after `pond_density_area_m2`.

- [ ] **Step 1: Write the failing tests.**

In `record.rs`'s tests:

```rust
    #[test]
    fn the_header_is_fifty_six_words() {
        let record = HydroRecord {
            bodies: Vec::new(), reaches: Vec::new(), notches: Vec::new(), falls: Vec::new(),
            stats: sample_stats(),
        };
        assert_eq!(encode(&record).len(), 56);
        assert_eq!(encode(&record)[0], 6.0);
    }

    #[test]
    fn a_body_carries_its_extent_words() {
        let mut record = HydroRecord {
            bodies: vec![sample_body()], reaches: Vec::new(), notches: Vec::new(),
            falls: Vec::new(), stats: sample_stats(),
        };
        record.bodies[0].shore_member_count = 3;
        record.bodies[0].shore_reach_m = 41_000.5;
        let words = encode(&record);
        assert_eq!(decode(&words).as_ref(), Some(&record));
        // 16 fixed words, then the outline pairs.
        assert_eq!(words[56 + 14], 3.0);
        assert_eq!(words[56 + 15], 41_000.5);
    }

    #[test]
    fn a_schema_five_record_is_refused() {
        let mut words = encode(&HydroRecord {
            bodies: Vec::new(), reaches: Vec::new(), notches: Vec::new(), falls: Vec::new(),
            stats: sample_stats(),
        });
        words[0] = 5.0;
        assert_eq!(decode(&words), None);
    }
```

`sample_body()` and `sample_stats()` are whatever that module's existing fixtures are called — read them and use them; if only one exists, follow its shape.

Run `cargo test -p worldbuilder-engine --lib record`. Expected: FAIL (fields missing, header 54).

- [ ] **Step 2: Add the fields.** In `mod.rs`:

```rust
    /// Ruling E-1: how many of `outline`'s points are the body's own shore members. The rest are
    /// its collar. Zero means the outline is a traced curve, not a shore-point set (Ruling T1-2
    /// and spec §8.3): that is how a pond, and a fine-search lake, are told apart from a coarse
    /// body. Plan 1b-4 fills this; before it, every coarse body shipped an empty outline.
    pub shore_member_count: u32,
    /// Ruling E-3: the longest usable member-to-collar step of this body, in metres — usable
    /// meaning the collar end's landform stands above `level_m`. Spec §8.3's second clause uses
    /// it as the width of the shore band, which is what holds the level contour inside the
    /// extent. Zero for a traced curve.
    pub shore_reach_m: f64,
```

Add `shore_members` and `collar_points` to `BakeStats` with doc comments saying they are the totals across bodies, and that they are the record's own account of what the extent cost.

Fix every `Body { .. }` and `BakeStats { .. }` literal the compiler names — `bake.rs`, `ponds.rs`, `record.rs`'s fixtures, `refine.rs`'s test fixtures and `bake_tests.rs`. Every one of them sets the new fields to `0` / `0.0` in this task.

- [ ] **Step 3: The wire.** In `record.rs`:
- set `SCHEMA` to `6.0` and rewrite its doc comment to say what SCHEMA 6 added and why the discriminator had to precede the data (name this plan's ordering constraint);
- `encode`: push the two header words after `pond_density_area_m2`, and the two body words after `downstream_id`;
- `decode`: read them back in the same places, `shore_member_count` through `r.u32()?`;
- update the module doc's layout description, including the sentence that a body whose `shore_member_count` is 0 carries a traced curve.

Run the record tests. Expected: PASS.

- [ ] **Step 4: The other three twins.**
- `tests/wasm_exports.rs`: the header assertion becomes `len >= 56`, with a comment naming plan 1b-4 and schema 6.
- `viewer/public/app/engine.js::hydroSummary`: refuse any schema but 6; the header is 56 words; expose `shoreMembers` and `collarPoints`.
- `viewer/public/app/water-preview.js::decodeHydro`: schema 6, 56-word header, the two new header fields, and **the two new body fields** — `shoreMemberCount` and `shoreReachM` — read in the same position as the Rust.
- In `water-preview.js`'s `drawPreview`, change the traced-curve branch from `outline.length > 0` to `shoreMemberCount === 0 && outline.length > 0`, and rewrite the comment that names this plan: the discriminator is now on the wire, and a coarse body will have a non-empty outline from Task 2 onward.
- Update `viewer/test/water-preview.test.mjs` and `hydro.test.mjs`: the schema-refusal tests refuse 5 and accept 6, the body offsets move by two words, and the pond-drawing test's bodies carry `shoreMemberCount: 0`. Add a test that a body with `shoreMemberCount > 0` and a non-empty outline is **not** drawn as a polygon.

- [ ] **Step 5: Run everything.** All five gates.yml feature configurations, `no_std_math`, and from `viewer/`: `npm run build:wasm` then `npm test`. Check the CRLF guard from the Global Constraints before the wasm rebuild. Expected: all pass. Do not re-pin CI counts; Task 6 does that.

- [ ] **Step 6: Commit** `Water 1b-4: SCHEMA 6 puts the extent's discriminator on the wire`, including the rebuilt wasm.

---

### Task 2: Compute and record the extent

**Files:**
- Create: `src/hydrology/extent.rs`
- Modify: `src/hydrology/mod.rs` (`pub mod extent;`), `src/hydrology/bake.rs`
- Test: `extent.rs`'s test module, `src/hydrology/bake_tests.rs`

**Interfaces:**
- Consumes: `LandGraph` (`positions`, `height_m`, `neighbours`, `radius_m`), `routing::Routing.lake_of`, `routing::NO_LAKE`, `hollows::Hollow` (`members`, `level_m`), and `SpherePoint::distance_to`.
- Produces:

```rust
pub struct Extent {
    /// Ruling E-1: shore members first, then collar, each ascending by node index.
    pub points: Vec<(f64, f64)>,
    pub shore_member_count: u32,
    /// Ruling E-3: the longest usable member-to-collar step, in metres.
    pub shore_reach_m: f64,
}

pub fn extent_of(graph: &LandGraph, lake_of: &[u32], hollow_index: u32, members: &[u32], level_m: f64) -> Extent;
```

- [ ] **Step 1: Write the failing tests** in `extent.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::hydrology::landgraph::LandGraph;
    use crate::hydrology::routing::NO_LAKE;
    use crate::sphere::SpherePoint;

    const R: f64 = 6_371_000.0;

    /// A chain of `n` nodes along the equator, one degree apart, each neighbouring the next.
    fn chain(heights: &[f64]) -> LandGraph {
        let n = heights.len();
        let positions = (0..n).map(|i| SpherePoint::from_latlon(0.0, i as f64)).collect();
        let directed: Vec<Vec<u32>> = (0..n)
            .map(|i| if i + 1 < n { vec![(i + 1) as u32] } else { Vec::new() }) // cast-ok: node index
            .collect();
        LandGraph::from_parts(R, positions, heights.to_vec(), vec![1.0e6; n], &directed, vec![0.5; n])
    }

    /// Nodes 2, 3 and 4 are a lake at level 10; 1 and 5 are its collar.
    fn lake_in_a_chain() -> (LandGraph, Vec<u32>, Vec<u32>) {
        let graph = chain(&[40.0, 30.0, 5.0, 2.0, 6.0, 30.0, 40.0]);
        let mut lake_of = vec![NO_LAKE; graph.len()];
        for m in [2u32, 3, 4] {
            lake_of[m as usize] = 0;
        }
        (graph, lake_of, vec![2, 3, 4])
    }

    #[test]
    fn the_extent_is_shore_members_then_collar() {
        let (graph, lake_of, members) = lake_in_a_chain();
        let e = extent_of(&graph, &lake_of, 0, &members, 10.0);
        // 2 and 4 touch a non-member; 3 is interior and is trimmed (Ruling E-2).
        assert_eq!(e.shore_member_count, 2);
        assert_eq!(e.points.len(), 4);
        let at = |i: usize| graph.positions[i].to_latlon();
        assert_eq!(e.points[0], at(2));
        assert_eq!(e.points[1], at(4));
        assert_eq!(e.points[2], at(1));
        assert_eq!(e.points[3], at(5));
    }

    #[test]
    fn shore_reach_is_the_longest_usable_step() {
        let (graph, lake_of, members) = lake_in_a_chain();
        let e = extent_of(&graph, &lake_of, 0, &members, 10.0);
        let one_degree = graph.positions[1].distance_to(&graph.positions[2], R);
        let gap = e.shore_reach_m - one_degree;
        assert!(gap < 1.0 && gap > -1.0, "shore_reach_m {} against one degree {}", e.shore_reach_m, one_degree);
    }

    #[test]
    fn a_collar_at_or_below_the_level_is_not_usable() {
        // Node 5 stands at 6 m, below the lake's level of 10, so its step is excluded (E-3);
        // node 1 at 30 m is usable, and it is the only one left.
        let graph = chain(&[40.0, 30.0, 5.0, 2.0, 5.0, 6.0, 40.0]);
        let mut lake_of = vec![NO_LAKE; graph.len()];
        for m in [2u32, 3, 4] {
            lake_of[m as usize] = 0;
        }
        let e = extent_of(&graph, &lake_of, 0, &[2, 3, 4], 10.0);
        assert_eq!(e.shore_member_count, 2, "both shore members are still recorded");
        assert_eq!(e.points.len(), 4, "the collar is still recorded, usable or not");
        let one_degree = graph.positions[1].distance_to(&graph.positions[2], R);
        let gap = e.shore_reach_m - one_degree;
        assert!(gap < 1.0 && gap > -1.0, "only node 1's step counts: {}", e.shore_reach_m);
    }

    #[test]
    fn a_body_with_no_usable_step_reaches_nowhere() {
        // Every collar node stands below the level: no usable edge, so the band is zero and the
        // extent is decided by the nearest-point clause alone.
        let graph = chain(&[1.0, 2.0, 5.0, 2.0, 5.0, 2.0, 1.0]);
        let mut lake_of = vec![NO_LAKE; graph.len()];
        for m in [2u32, 3, 4] {
            lake_of[m as usize] = 0;
        }
        let e = extent_of(&graph, &lake_of, 0, &[2, 3, 4], 10.0);
        assert_eq!(e.shore_reach_m, 0.0);
        assert_eq!(e.shore_member_count, 2);
    }

    #[test]
    fn the_extent_is_the_same_twice() {
        let (graph, lake_of, members) = lake_in_a_chain();
        assert_eq!(extent_of(&graph, &lake_of, 0, &members, 10.0),
                   extent_of(&graph, &lake_of, 0, &members, 10.0));
    }
}
```

`Extent` needs `#[derive(Debug, Clone, PartialEq)]` for the last one. Run `cargo test -p worldbuilder-engine --lib hydrology::extent`. Expected: FAIL.

- [ ] **Step 2: Implement** `extent.rs`:

```rust
//! A body's extent: the shore points spec §8.3 decides "is this point in that lake?" from.
//!
//! Plan 1b-3's Task 1 ruled the shape by measurement (Candidate B, that plan's design note §5.3):
//! an unordered set of recorded points and a nearest-point test, because the ring the spec first
//! imagined cannot be built on a k-nearest graph — both walks leave members outside their own ring
//! — and a 250 m shoreline of the owner's great lake alone would cost 3.3 MB against a 1 MB budget.
//!
//! **What is recorded** (Rulings E-1 and E-2): every *shore* member, then every collar node, each
//! ascending by node index. A shore member is a member with a neighbour that is not a member of
//! the same body; a collar node is a non-member with a member neighbour. Interior members are not
//! recorded — the great lake has 37,844 members and 1,178 shore members — and the claim that
//! dropping them cannot move the extent's boundary is on trial in this plan's Task 3.
//!
//! **The band** (Ruling E-3): `shore_reach_m` is the longest *usable* member-to-collar step, where
//! usable means the collar end's landform stands above the body's level. The level contour crosses
//! each usable step somewhere along it, so a band that wide holds the contour inside the extent.
//! A step whose collar end is at or below the level carries no contour to hold — it is dry ground
//! downhill of a perched rim, measured at 1.0-2.7% of shore steps — and counting it would only
//! widen the band.

use crate::hydrology::landgraph::LandGraph;
use crate::hydrology::routing::NO_LAKE;

#[derive(Debug, Clone, PartialEq)]
pub struct Extent {
    /// Ruling E-1: shore members first, then collar, each ascending by node index. The order
    /// carries no geometry; `shore_member_count` is the only thing that reads it.
    pub points: Vec<(f64, f64)>,
    pub shore_member_count: u32,
    /// Ruling E-3, in metres. Zero when no step is usable.
    pub shore_reach_m: f64,
}

/// The extent of the body whose hollow index is `hollow_index`. `members` is that hollow's member
/// list; only the nodes `lake_of` actually assigns to this body count, which is what excludes a
/// hollow's dry members above an enclosed basin's datum.
pub fn extent_of(graph: &LandGraph, lake_of: &[u32], hollow_index: u32, members: &[u32], level_m: f64) -> Extent {
    let mine = |node: u32| lake_of[node as usize] == hollow_index;
    let mut shore: Vec<u32> = Vec::new();
    let mut collar: Vec<u32> = Vec::new();
    let mut reach_m = 0.0;
    for &member in members {
        if !mine(member) {
            continue;
        }
        let mut is_shore = false;
        for &next in graph.neighbours(member) {
            if mine(next) {
                continue;
            }
            is_shore = true;
            collar.push(next);
            // Ruling E-3: only a step whose collar end stands above the level carries the contour.
            if graph.height_m[next as usize] > level_m {
                let step = graph.positions[member as usize]
                    .distance_to(&graph.positions[next as usize], graph.radius_m);
                if step > reach_m {
                    reach_m = step;
                }
            }
        }
        if is_shore {
            shore.push(member);
        }
    }
    shore.sort_unstable();
    shore.dedup();
    collar.sort_unstable();
    collar.dedup();
    let mut points = Vec::with_capacity(shore.len() + collar.len());
    for &node in shore.iter().chain(collar.iter()) {
        points.push(graph.positions[node as usize].to_latlon());
    }
    Extent {
        points,
        shore_member_count: shore.len() as u32, // cast-ok: at most one shore member per node
        shore_reach_m: reach_m,
    }
}
```

`NO_LAKE` is imported for readers even if the closure does not name it; drop the import if the compiler says it is unused.

Run the extent tests. Expected: PASS.

- [ ] **Step 3: Fill it in `record_of`.** In `bake.rs`, where each coarse `Body` is built, replace the empty outline with the extent:

```rust
        let extent = crate::hydrology::extent::extent_of(graph, &routing.lake_of, i as u32, &hollow.members, hollow.level_m); // cast-ok: hollow index
```

and set `outline: extent.points`, `shore_member_count: extent.shore_member_count`, `shore_reach_m: extent.shore_reach_m`. Total the two stats over the bodies and put them in `BakeStats`. Ponds are untouched (Ruling E-6): `ponds::search` keeps writing 0 and 0.0.

- [ ] **Step 4: World-scale properties.** Add to `bake_tests.rs`:

```rust
/// Rulings E-1, E-2 and E-3 on a real bake: every coarse body carries a shore-point set, every
/// pond carries a traced curve, and the two are told apart by `shore_member_count` alone.
#[test]
fn every_coarse_body_carries_an_extent() {
    for (name, p) in refined_populations() {
        let record = crate::hydrology::bake(&world(), &p).expect("bake");
        let coarse = record_of(&bake_stages(&world(), &p).expect("stages"), &p);
        let mut with_extent = 0usize;
        for body in &record.bodies[..coarse.bodies.len()] {
            assert!(body.shore_member_count > 0, "{name}: coarse body {} has no shore members", body.id);
            assert!(body.outline.len() as u32 > body.shore_member_count,
                    "{name}: body {} has shore members but no collar", body.id);
            assert!(body.shore_reach_m >= 0.0 && body.shore_reach_m.is_finite());
            with_extent += 1;
        }
        for body in &record.bodies[coarse.bodies.len()..] {
            assert_eq!(body.shore_member_count, 0, "{name}: a pond carries a traced curve");
            assert_eq!(body.shore_reach_m, 0.0);
        }
        assert!(with_extent > 0);
        let shore: u32 = record.bodies.iter().map(|b| b.shore_member_count).sum();
        assert_eq!(record.stats.shore_members, shore);
        eprintln!("{name}: {with_extent} coarse bodies, {} shore members, {} collar points",
                  record.stats.shore_members, record.stats.collar_points);
    }
}

/// Ruling E-3: a body's band never counts a step down to ground at or below its own level.
#[test]
fn no_bodys_band_counts_a_step_below_its_level() {
    let p = params();
    let stages = bake_stages(&world(), &p).expect("stages");
    let record = record_of(&stages, &p);
    let graph = &stages.graph;
    for (i, hollow) in stages.hollows.iter().enumerate() {
        if hollow.fate != Fate::Keep {
            continue;
        }
        let body = record.bodies.iter().find(|b| b.anchor == graph.positions[hollow.floor as usize].to_latlon());
        let body = match body { Some(b) => b, None => continue };
        let mut longest_usable = 0.0;
        for &member in &hollow.members {
            if stages.routing.lake_of[member as usize] != i as u32 { // cast-ok: hollow index
                continue;
            }
            for &next in graph.neighbours(member) {
                if stages.routing.lake_of[next as usize] == i as u32 { // cast-ok: hollow index
                    continue;
                }
                if graph.height_m[next as usize] <= hollow.level_m {
                    continue;
                }
                let step = graph.positions[member as usize].distance_to(&graph.positions[next as usize], graph.radius_m);
                if step > longest_usable { longest_usable = step; }
            }
        }
        let gap = body.shore_reach_m - longest_usable;
        assert!(gap < 1e-6 && gap > -1e-6, "body {} band {} against longest usable {}", body.id, body.shore_reach_m, longest_usable);
    }
}
```

`refined_populations()` and `Fate` are already in that file; read how the existing tests reach `stages.hollows` and follow it.

- [ ] **Step 5: Mutation guard.** Temporarily drop the `is_shore` condition so every member is recorded. `every_coarse_body_carries_an_extent`'s collar assertion still passes, so the guard is Task 3's job — instead, temporarily make `extent_of` skip the usability check in E-3 and confirm `no_bodys_band_counts_a_step_below_its_level` FAILS. Restore, and paste both outputs.

- [ ] **Step 6: Run and commit.** All five configurations, `no_std_math`, the drain sweep, the viewer suite, the CRLF guard, then `npm run build:wasm`. Report the record's word count before and after on each test population. Commit `Water 1b-4: every kept body records its shore points and its band`.

---

### Task 3: The trim on trial

**Files:**
- Modify: `src/hydrology/bake_tests.rs` (or a new `src/hydrology/extent_tests.rs` if that file is already over 1,500 lines — say which you chose and why)
- Modify: `src/hydrology/extent.rs` if Ruling E-5 fires

**Interfaces:**
- Consumes: Task 2's `Extent` and the recorded bodies.
- Produces: either evidence that the trim holds, or the untrimmed fallback (Ruling E-5) and the same evidence for it.

The design note's §5.7 is explicit: **sampling member positions cannot see this fail**, because a member is trivially nearest to itself. The failure mode is a point *between* two interior members falling into a collar node's Voronoi cell.

- [ ] **Step 1: Write the test.** Two populations: a running one at 12k–60k nodes, and an `#[ignore]`d sweep at 1,000,000 nodes.

```rust
/// The design note's §5.7 trial of Ruling E-2's trim. For every kept body, sample the interior
/// *between* members — the midpoint of every graph edge whose ends are both members, and the
/// centroid of every member with its member neighbours — and assert each sample is inside its own
/// body's extent by spec §8.3's first clause: the nearest recorded member point is at least as
/// near as the nearest recorded collar point.
///
/// Sampling member positions themselves proves nothing: a member is nearest to itself.
fn interior_samples_stay_inside(p: &HydroParams, world: &Surface) -> (usize, usize, f64) {
    let stages = bake_stages(world, p).expect("stages");
    let record = record_of(&stages, p);
    let graph = &stages.graph;
    let radius = graph.radius_m;
    let mut sampled = 0usize;
    let mut outside = 0usize;
    let mut worst = 0.0;
    for (i, hollow) in stages.hollows.iter().enumerate() {
        if hollow.fate != Fate::Keep { continue; }
        let mine = |n: u32| stages.routing.lake_of[n as usize] == i as u32; // cast-ok: hollow index
        let body = match record.bodies.iter().find(|b| b.anchor == graph.positions[hollow.floor as usize].to_latlon()) {
            Some(b) => b, None => continue,
        };
        let shore = body.shore_member_count as usize;
        if shore == 0 || shore == body.outline.len() { continue; }
        let members: Vec<SpherePoint> = body.outline[..shore].iter()
            .map(|&(lat, lon)| SpherePoint::from_latlon(lat, lon)).collect();
        let collar: Vec<SpherePoint> = body.outline[shore..].iter()
            .map(|&(lat, lon)| SpherePoint::from_latlon(lat, lon)).collect();
        let mut check = |p: SpherePoint| {
            let dm = members.iter().map(|q| q.distance_to(&p, radius)).fold(f64::INFINITY, |a, b| if b < a { b } else { a });
            let dc = collar.iter().map(|q| q.distance_to(&p, radius)).fold(f64::INFINITY, |a, b| if b < a { b } else { a });
            sampled += 1;
            if dm > dc {
                outside += 1;
                let by = dm - dc;
                if by > worst { worst = by; }
            }
        };
        for &member in &hollow.members {
            if !mine(member) { continue; }
            let mut sum = graph.positions[member as usize].vector;
            let mut count = 1.0;
            for &next in graph.neighbours(member) {
                if !mine(next) { continue; }
                if next > member {
                    let mid = SpherePoint::from_vector(&graph.positions[member as usize].vector
                        .add(&graph.positions[next as usize].vector));
                    if let Some(mid) = mid { check(mid); }
                }
                sum = sum.add(&graph.positions[next as usize].vector);
                count += 1.0;
            }
            if count > 1.0 {
                if let Some(centroid) = SpherePoint::from_vector(&sum.scaled(1.0 / count)) { check(centroid); }
            }
        }
    }
    (sampled, outside, worst)
}

#[test]
fn the_trim_holds_on_the_test_worlds() {
    for (name, p) in refined_populations() {
        let (sampled, outside, worst) = interior_samples_stay_inside(&p, &world());
        eprintln!("{name}: {sampled} interior samples, {outside} outside, worst {worst:.1} m");
        assert!(sampled > 0, "{name}: nothing sampled");
        assert_eq!(outside, 0, "{name}: {outside} interior samples fell outside their own extent, worst by {worst:.1} m");
    }
}

/// The population the design note names: a 1,000,000-node bake. Ignored because it takes minutes.
#[test]
#[ignore]
fn the_trim_holds_at_a_million_nodes() {
    let world = Surface::new(20_260_904, 6_371_000.0, 12, 0.29, None, None, None);
    let p = HydroParams::earth_like(1_000_000);
    let (sampled, outside, worst) = interior_samples_stay_inside(&p, &world);
    eprintln!("1M plain: {sampled} interior samples, {outside} outside, worst {worst:.1} m");
    assert_eq!(outside, 0, "{outside} of {sampled} interior samples fell outside, worst by {worst:.1} m");
    let ranges = Surface::new(1, 6_371_000.0, 12, 0.40, None, Some(TectonicParams::ranges()), None);
    let (s2, o2, w2) = interior_samples_stay_inside(&p, &ranges);
    eprintln!("1M ranges: {s2} interior samples, {o2} outside, worst {w2:.1} m");
    assert_eq!(o2, 0, "{o2} of {s2} interior samples fell outside, worst by {w2:.1} m");
}
```

Check `Surface::new`'s real signature and `TectonicParams::ranges()`'s use against `bake_tests.rs`'s existing `ranges_world()` before you write these two worlds; use that helper if it fits. `Vec3::add`/`scaled` are in `vectors.rs` — read the names there. `f64::INFINITY` folds are not `f64::min`; if the no-std scanner objects, write the fold with an explicit `if`.

- [ ] **Step 2: Run both.** The running one with `cargo test -p worldbuilder-engine --lib the_trim_holds_on_the_test_worlds -- --nocapture`, and the sweep with `--release ... -- --ignored --nocapture`. Report both counts and the worst distance, whichever way they come out.

- [ ] **Step 3: If it holds**, say so in the report with the sample counts, and stop. The trim ships.

- [ ] **Step 4: If it fails (Ruling E-5)**, record the interior members too — for every body, not per body. In `extent_of`, push every member rather than only the shore ones, keep the same order and keep `shore_member_count` meaning "how many of `points` are members". Re-run both tests, re-run Task 2's world-scale tests, and report the record's growth on each population. Do not invent a third option.

- [ ] **Step 5: Mutation guard.** Whichever branch you took, prove the test can fail: temporarily record only *half* the shore members (`shore.retain(|n| n % 2 == 0)`) and confirm `the_trim_holds_on_the_test_worlds` FAILS with a non-zero outside count. Restore, and paste both outputs.

- [ ] **Step 6: Commit** `Water 1b-4: the trim is tested between the members, not at them` (or, if E-5 fired, `Water 1b-4: interior members are recorded too, and the trial says why`).

---

### Task 4: Parity always compares a pond body, and a test bakes at the shipped pond params

**Files:**
- Modify: the parity corpus builder (`crates/worldbuilder-engine/parity/parity_dump.rs` — read it and the harness beside it first), `src/hydrology/bake_tests.rs`
- Test: the parity run itself, plus the new bake test

**Interfaces:**
- Consumes: nothing new.
- Produces: a parity corpus in which at least one compared record carries a body with `shore_member_count == 0` (a pond) **and** at least one with `shore_member_count > 0`, and a running test that bakes at `earth_like`'s pond parameters.

Plan 1b-3 left two coverage holes, both routed here by its carry-forward: `hydro/plain` keeps no pond after Ruling S-16's cap, so only `hydro/ranges` exercises a pond body across the native-versus-wasm boundary; and no Rust test bakes at the shipped `pond_search_radius_m` of 1,500 m and `pond_density_area_m2` of 1.6e10 — plan 1b-3 added a value pin on the constants, which is not the same thing.

- [ ] **Step 1: Measure what the corpus holds now.** Run the parity dump and report, per hydro record, how many bodies have `shore_member_count == 0` and how many have more. Do not guess from the counts in the reports.

- [ ] **Step 2: Close the pond hole.** Two ways; pick on the numbers and say why:
  - raise `hydro/plain`'s node count until it keeps at least one pond, or
  - add a third hydro record to the corpus whose parameters keep one.

  Whichever you choose, the corpus must still run inside CI's existing time — report the parity job's wall time before and after.

- [ ] **Step 3: Close the behavioural hole.** Add to `bake_tests.rs`:

```rust
/// Rulings S-16 and S-17 shipped a 1,500 m corridor and a 1.6e10 density, and plan 1b-3's value
/// pin only checks the constants. This bakes at them, so a change in behaviour at the shipped
/// parameters is caught by something other than a parity count.
#[test]
fn a_bake_at_the_shipped_pond_params_keeps_ponds() {
    let mut p = params();
    p.pond_search_radius_m = HydroParams::earth_like(1_000).pond_search_radius_m;
    p.pond_density_area_m2 = HydroParams::earth_like(1_000).pond_density_area_m2;
    let record = crate::hydrology::bake(&ranges_world(), &p).expect("bake");
    let ponds = record.bodies.iter().filter(|b| b.shore_member_count == 0).count();
    eprintln!("shipped pond params on the ranges world: {ponds} ponds of {} found", record.stats.ponds_found);
    assert!(ponds > 0, "the shipped parameters keep no pond on this world");
    for body in record.bodies.iter().filter(|b| b.shore_member_count == 0) {
        assert!(body.outline.len() >= 3);
        assert!(matches!(body.downstream, Downstream::Reach(_)));
    }
}
```

If the ranges world at these parameters keeps no pond, raise its node count until it does — and if that costs more than about 10 s in release, say so and use the smallest population that works. Report the number.

- [ ] **Step 4: Run and commit.** All five configurations, `no_std_math`, parity (0 divergent), the viewer suite, the CRLF guard, `npm run build:wasm`. Do not re-pin; Task 6 does. Commit `Water 1b-4: parity always compares a pond, and a test bakes at the shipped parameters`.

---

### Task 5: The carry-forward batch

**Files:**
- Modify: `src/hydrology/refine.rs` → `src/hydrology/refine/` (a directory), `src/hydrology/ponds.rs`, `tests/no_std_math.rs`, `docs/superpowers/specs/2026-09-10-automatic-water-design.md`
- Test: the existing suites, plus one new test for the dedup fix

Five items, all routed here by plan 1b-3's carry-forward and ledger. They are independent; do them in one task and one commit each if that reads better.

- [ ] **Step 1: Split `refine.rs`.** It is 1,474 lines and grew in each of the last three plans. Split it into a directory module with the same public surface — nothing outside `hydrology` may need to change:
  - `refine/mod.rs`: `Ground`, `Fine`, `Segment`, `Refined`, `Chord`, `terminal_level`, `beds_never_rise`, `refine`, and the re-exports;
  - `refine/trace.rs`: `trace_segment`, `trace_reach`, `step_back`, the bed rule and the shore trim;
  - `refine/falls.rs`: `find_fall` and its insertion;
  - `refine/meander.rs`: the meander;
  - `refine/simplify.rs`: `simplify`, `simplify_mask`;
  - `refine/crossings.rs`: `Crossing`, `segments_cross`, `crossings`, and the pass.

  Move the tests with the code they exercise. **No behaviour changes in this step** — the wasm's artifact sha256 must be unchanged afterwards, and if it is not, stop and find out why before committing.

- [ ] **Step 2: The crossing pass's cost.** `crossings()` runs up to six times a bake, and each call clones every reach's points into `lines`. Take the clone out — pass slices, or index into the shipped `Refined`s — and skip the in-pass call when the previous pass moved nothing. Measure the bake time on a 200k-node ranges world before and after, and report both. Do not change what the pass decides: the crossing counts on all three populations must be identical, and `refinement_adds_no_crossings` and `a_yielded_segment_ships_on_its_chord` must stay green.

- [ ] **Step 3: A refused candidate stops claiming a dedup anchor.** In `ponds.rs::search`, a candidate whose ring is later refused still inserts into `anchors`, so it can suppress a neighbour within `pond_cell_m * 2.0` that would have been recorded. Move the insert after the outline is built and verified. Add a test: two candidates within that distance, the first with a cell set whose ring cannot be verified, and assert the second is recorded. If a pinched cell set is hard to build by hand, drive it through a `Candidate` fixture directly and say so.

- [ ] **Step 4: `no_std_math` scans `examples/`.** `tests/no_std_math.rs` walks `src/` only, and `examples/` now carries a real instrument (`pond_search_survey.rs`). Extend the walk, and fix whatever it finds — the file was inspected clean in plan 1b-3, so expect nothing, but if it finds something, that is the point.

- [ ] **Step 5: The three spec passages plan 1b-3 routed here.**
  - **§8.2:** the index's cell list must include a body whose **shore points come within `shore_reach_m` of the cell** — about one graph spacing, against 50 km cells, so the dilation is not negligible. Say it in the section, and say that a pond's traced curve dilates by nothing.
  - **§9:** it still names `dilateBodyExtents` and "the box-and-level rule" as the studio's drawing path. Those are the old box drawing that stage 3 removes; reword so the section describes drawing from `water_at` and names the removal.
  - **§14:** properties 6 and 9 still use "outline" in the curve sense ("starting on its outline", "every sampled point of every body outline"). Reword both for the shore-point representation — property 9 should sample the extent's own points and the reach lines, and say what a pond's curve contributes.

- [ ] **Step 6: Run and commit.** All five configurations, `no_std_math` (now wider), the drain sweep, parity, the viewer suite, the CRLF guard, `npm run build:wasm`. Report the before-and-after bake time from Step 2 and the artifact sha from Step 1.

---

### Task 6: Survey, pins, and the owner's world

**Files:**
- Modify: `src/bin/hydro_survey.rs`, `.github/workflows/gates.yml`, `crates/worldbuilder-engine/README.md`, `docs/superpowers/reports/2026-09-10-water-1a-carry-forward.md`
- Create: `docs/superpowers/reports/2026-09-12-water-1b4-verification.md`

- [ ] **Step 1: The survey reports the extent.** `hydro_survey` prints, per bake: shore members, collar points, the extent's share of the record in bytes, the largest `shore_reach_m` and the median, and the bodies with no collar (there should be none). Run it on the three 1M stand-ins — seed 562,423,712 at radius 4.5e6 with 28 plates, land 0.16 and `ranges()`; seed 20,260,904 at 6,371,000 with 12 plates, land 0.29; seed 1 at 6,371,000 with 12 plates, land 0.40 and `ranges()` — and put the table in the report.

- [ ] **Step 2: The gates.** Each stand-in's record is at most 8,000,000 bytes and the whole bake at most 120 s natively. If size is over, the ponds' half of the budget is the first lever (`pond_density_area_m2`, ×2 at a time), not the bodies' — the design note says so, and the extent is what this plan exists to add. Report each step.

- [ ] **Step 3: Rebuild, parity, pins.** The CRLF guard, then `npm run build:wasm`, then parity from `crates/worldbuilder-engine/parity/` as gates.yml runs it (0 divergent), then re-derive every pin by running it: the five engine configurations, the parity totals and every control including the native prediction, and the Python pin. Update gates.yml and the README mirror with a dated note (2026-09-12, plan 1b-4) giving old and new values. Run both ignored sweeps.

- [ ] **Step 4 (controller): The owner's world.** The controller bakes `worlds/world-1788998299904.json` at 1M nodes with one forced outlet at 0°N 0°E in the branch studio, and hands back: the time; the record bytes and the extent's share; shore members and collar points; the largest and median `shore_reach_m`; that every coarse body has both a shore member and a collar; that every pond still has `shore_member_count == 0`; and that plan 1b-2 and 1b-3's properties still hold. Gates: at most 8 MB and at most 300 s.

- [ ] **Step 5: The verification report**, `docs/superpowers/reports/2026-09-12-water-1b4-verification.md`, in plan 1b-3's form — population, method and host for every figure. It must state:
  - the extent's measured cost on the owner's world against the design note's 229 KB estimate, and whether the trim held or Ruling E-5 fired;
  - Task 3's sample counts at both populations;
  - the stand-in table and every pin, old and new;
  - what stage 2 now has: the discriminator, the extent, the band, and the tie-break rule already written into §8.3.

- [ ] **Step 6: The carry-forward.** Tick lake outlines — the last of plan 1a's items — and say the representation is Candidate B as measured and now recorded. Move anything this plan did not close into a "Routed to stage 2" list, and add what stage 2 must do first: build the index with the `shore_reach_m` dilation, implement §8.3's test including the tie-break, and carve from the record rather than from a re-bake.

- [ ] **Step 7: Commit** `Water 1b-4: pins re-derived, and the extents verified on the owner's world`.

---

## Self-review

**Spec coverage:**

| Spec item | Where |
|---|---|
| §7 `outline`, `shore_member_count`, `shore_reach_m` | Tasks 1 and 2 |
| §7 the `kind`/`shore_member_count` discriminator | Task 1 (wire), Task 2 (filled) |
| §8.2 the index dilated by `shore_reach_m` | Task 5's spec text; stage 2 implements it |
| §8.3 the nearest-point test, the band, the tie-break | recorded here, implemented in stage 2 (Ruling E-7) |
| §9 the studio's drawing path | Task 5's spec text |
| §14.3, §14.6, §14.9 wording | Task 5 |
| §14.1 determinism | Task 2's test, plus parity in Task 6 |
| §14.2 everything drains | untouched; the sweep runs in Tasks 2, 5 and 6 |
| The design note's §5.7 trial | Task 3 |
| Plan 1b-3's routed items | Tasks 4 and 5 |

**Types:** `Extent`, `extent_of`, `Body.shore_member_count`, `Body.shore_reach_m`, `BakeStats.shore_members`, `BakeStats.collar_points` are used with the same names throughout. The header is 56 words and the body 16 fixed words in Task 1's Rust and in all four twins.

**Known soft spots, named rather than hidden:**
- Task 3 may fire Ruling E-5, which grows the record by an estimated 660 KB on the owner's world and leaves about 110 KB of headroom. Task 6's gate is where that would bite, and the lever there is the ponds' cap.
- Task 5's Step 1 is a pure move of a 1,474-line file. If the artifact sha changes, something moved that should not have.
- The owner-world extent cost is an estimate scaled from 65 unpainted bodies to 348 painted ones. Task 6 measures it; nothing downstream may inherit the estimate.
