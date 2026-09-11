# Water 1b-1: The Coarse Bake, Fixed and Trimmed — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the load-bearing carry-forward items from plan 1a:
- the residual outlet-cut cycle;
- raster-shaped notch paths;
- inner lakes lost in capped basins;
- lakes with no downstream link;
- a record three times its size target.

Then ship SCHEMA 3 with the fields spec §7 names.

**Architecture:** Five small, general changes inside `src/hydrology/`, then one record revision:
1. The flood breaks ties by ground height, so parent chains follow valley floors.
2. Cuts follow the flood tree until they reach the ocean, a lake, or an earlier cut. They never stop on "lower ground", which removes the cycle.
3. Capped basins get nested judging, seeded from their drain.
4. Bodies carry a downstream link.
5. The record keeps only notch points that matter, and it echoes its params.

`mod.rs` is split first, so the later tasks each touch focused files.

**Tech Stack:** Rust 1.98.0 (Windows MSVC, matching CI), the engine's `detmath`, wasm32, and Node 22 for the viewer.

**Spec:** `docs/superpowers/specs/2026-09-10-automatic-water-design.md` (§6.2, §6.3, §7, §14). Carry-forward: `docs/superpowers/reports/2026-09-10-water-1a-carry-forward.md`. Plan 1a's rulings stand unless restated here.

## Global Constraints

- **No std float maths outside `detmath.rs`** (`tests/no_std_math.rs` scans `src/`, bins included).
- **Casts:** `as u32`, `as u64`, `as i32` and `as i64` need `// cast-ok: <reason>` on the same line.
- **No `f64::min`, `f64::max` or `.clamp(`.** Use `total_cmp` for float sorts.
- **No panics reachable from `extern "C"`.**
- **Determinism:** no HashMap order in output, ties to the lower node index, heap keys via `heap::sortable`.
- **`Surface` gains no field.** Commit subjects name no third party.
- **"Everything drains" is binding.** `drainage_check` must pass at the end of `bake_stages` on every test world, and the existing seeds-1/4242 test must keep passing.
- **Every `src/` edit changes the fingerprint.** Rebuild the wasm (`npm run build:wasm` in `viewer/`) before any parity run, and commit it.
- **CI pins** (currently engine 692/692/694/798/800 with 5 ignored; parity plain 136,086/0; Python 565) are re-derived once, in the last task, with a dated note. The README mirrors them.
- **The record layout's twins** must change with it: `record.rs`, `viewer/public/app/engine.js::hydroSummary`, `viewer/public/app/water-preview.js::decodeHydro`, their tests, and `parity.mjs` (header-agnostic, so check only).
- **Verification numbers** come from runs, stated with their population, method and host (the owner world is `worlds/world-1788998299904.json` at 1M nodes, in the studio).

## File Structure

| File | Change |
|---|---|
| `src/hydrology/bake.rs` (new) | `BakeStages`, `bake_stages`, `record_of`, `reach_points`, and the bake tests, moved from `mod.rs` |
| `src/hydrology/mod.rs` | public types, `HydroParams`, `bake()`, re-exports (about 300 lines after the move) |
| `src/hydrology/heap.rs` | a secondary tie key |
| `src/hydrology/flood.rs` | ties by ground height |
| `src/hydrology/routing.rs` | the cut rule, and nested judging for capped basins |
| `src/hydrology/hollows.rs` | a `capped` flag |
| `src/hydrology/record.rs` | SCHEMA 3 |
| `viewer/public/app/engine.js`, `water-preview.js` | the SCHEMA 3 decode |

---

### Task 1: Split `mod.rs` (no behaviour change)

**Files:**
- Create: `src/hydrology/bake.rs`
- Modify: `src/hydrology/mod.rs`

**Interfaces:**
- Produces: `hydrology::bake::{BakeStages, bake_stages, record_of}`. `bake_stages` and `record_of` are also re-exported from `mod.rs` under their current names, so every existing caller (wasm.rs, hydro_survey.rs, parity_dump.rs, tests) compiles unchanged.

- [ ] **Step 1: Move the code.** Move `BakeStages`, `bake_stages`, `record_of`, `reach_points`, their private helpers, and the `bake_tests` module into `bake.rs`. Keep the types, `HydroParams`, `earth_like`, `DEFAULT_TOTAL_NODES`, `HydroError`, `reaches_are_acyclic` and `bake()` in `mod.rs`. Add `pub mod bake;` and `pub use bake::{BakeStages, bake_stages, record_of};`.
- [ ] **Step 2: Prove nothing moved.**
  - Run `cargo test -p worldbuilder-engine hydrology` and `--test no_std_math`: same pass count as before (60 hydrology tests at 22be4a6 — state the number you see before and after).
  - Rebuild the wasm and run parity plain: 136,086 compared, 0 divergent, with **the same H words** (the move must be bit-identical).
- [ ] **Step 3: Commit** `Water: the bake moves to its own file` (plus the wasm and MANIFEST if the fingerprint moved).

---

### Task 2: The flood breaks ties by ground height (carry-forward I2)

**Files:**
- Modify: `src/hydrology/heap.rs`, `src/hydrology/flood.rs`

**Interfaces:**
- Produces: `FloodQueue::push_tied(level_m: f64, tie_m: f64, node: u32)`. `pop()` returns `(level, node)` as now. `push(level, node)` becomes `push_tied(level, level, node)`.

Why this change: inside a filled hollow every node has the same spill, so ties break by node index, and node index follows latitude along the Fibonacci spiral. Notch and outlet paths therefore come out raster-shaped. With the node's own ground height as the second key, the flood reaches the lowest ground first, so the parent chains follow valley floors. The primary key is still the spill level, so "fills to its lowest rim" is untouched.

- [ ] **Step 1: Failing tests.**
  - In `heap.rs`: `ties_break_by_ground_then_node`. Push `(5.0, tie 3.0, 7)`, `(5.0, tie 1.0, 9)` and `(5.0, tie 1.0, 2)`; the pops must come out as 2, 9, 7.
  - In `flood.rs`: `inside_a_flat_the_flood_follows_the_low_ground`. Use a hand grid (`LandGraph::from_parts`, a 5×5 lattice with 4-neighbour directed lists, positions on a small lat/lon patch). The ocean is one corner. The rest is one 30 m flat basin with a 1 m-deep "valley" of nodes running diagonally from the far corner to the rim, and the rim node sits by the ocean corner. Assert that the parent chain from the far corner visits only valley nodes before the rim. It must fail on the current code (index ties).
- [ ] **Step 2: Implement.** The key becomes `(sortable(level), sortable(tie), node)`. Flood pushes seeds with `tie = level` and neighbours with `tie = own height`.
- [ ] **Step 3:** Run `cargo test -p worldbuilder-engine hydrology`. Fixture tests whose expected parents change must be re-derived by hand, not weakened; list each one. Then run `--test no_std_math`. Parity is expected to move, and is re-pinned in Task 7.
- [ ] **Step 4: Commit** `Water: the flood reaches low ground first, so paths follow valleys`.

---

### Task 3: Cuts follow the flood tree (the residual cycle)

**Files:**
- Modify: `src/hydrology/routing.rs` (`cut_route`, `cut_path`), `src/hydrology/flow.rs` (tests)

**Interfaces:**
- `cut_route` and `cut_path` keep their signatures.
- `Routing` gains `pub committed: Vec<bool>`, marking nodes whose receiver was set by a cut.

**The rule** (the proof is in the doc comment):
- A cut walks its path (`cut_route`: the parent chain; `cut_path`: the given path, then the parent chain from its last node).
- It sets each step's receiver to the next node, and lowers the surface to `min(surface, bed)`. The bed grades down from `start_bed_m` by `NOTCH_GRADE_M`. If the ground is already lower, the bed follows the ground: `bed = surface` there, then it keeps grading.
- It **stops only at**:
  - an ocean node;
  - a lake member (receiver set to it, surface untouched);
  - a node already `committed` by an earlier cut;
  - the end of a parent chain (`NO_NODE`).
- It **never stops** because the ground is lower.
- The `NotchRoute` records only nodes whose surface it actually lowered, with their beds.

**Proof sketch (for the doc comment):** receiver edges are either steepest-descent edges, where the surface strictly falls, or cut edges along the flood tree. Flood-tree parents have non-increasing spill, and the cut lowered every surface on them to a graded bed, so no cut edge rises. A cycle would need every edge in it flat; a cycle of flat steepest edges is impossible; and a cycle of parent edges is impossible because the flood tree is a tree. Stopping at `committed` nodes keeps the total work O(n).

- [ ] **Step 1: Failing tests** in `flow.rs`, the review's two fixtures:
  - `a_cut_never_stops_on_ground_that_drains_back`: line `[-50,-40,39,10,0.02,0.1,0.3,0.5,-5,60]`, area 1e6, wetness 0.5, `earth_like(0)`. After `close_lakes`, `drainage_check` is `Ok`.
  - `a_notched_shore_lake_cannot_close_a_cycle`: line `[-50,-40,39,30,0.05,20,14,12,10,8,6,4,2,-5,60]`, same settings. `drainage_check` is `Ok`.

  Both must fail today with `Err(3)`.
- [ ] **Step 2: Implement** the rule above in both functions, and add `committed`.
- [ ] **Step 3: The property sweep.** Add a test, `every_small_world_drains`, that bakes seeds 1 to 24 at 12,000 nodes, with and without `TectonicParams::ranges()`, through `bake_stages`. Every one must be `Ok`. That is 48 bakes; keep them in release-style sizes so the debug suite stays under about 60 s extra, and state the time. Also keep the existing seeds-1/4242 test.
- [ ] **Step 4:** `cargo test -p worldbuilder-engine hydrology` and `--test no_std_math`. Commit `Water: cuts follow the flood tree to the sea, a lake or an earlier cut`.

---

### Task 4: Capped basins keep their inner lakes (carry-forward I3)

**Files:**
- Modify: `src/hydrology/hollows.rs` (`capped: bool` on `Hollow`, set in `judge` when `too_large` decided Notch), `src/hydrology/routing.rs`

**The rule:**
- For each hollow with `capped == true`, `route()` runs a sub-flood **seeded at its floor node, at its floor height**, confined to its members.
- Members the sub-flood reaches take its parents. Water inside the basin runs to the drain, and the drain is then cut to the sea by the minima pass along global parents.
- `find_hollows` on that sub-flood gives the inner basins. Judge them with the same rules (they are not enclosed; each may itself be capped, and then it is simply notched), and append them.
- Kept inner lakes drain over their rims, down the sub-flood parents, to the drain.
- This runs after the enclosed-pocket step and before C1-a.

- [ ] **Step 1: Failing test** `a_capped_basin_keeps_a_deep_inner_lake`. Use a hand line fixture: an ocean, then a 40 m rim, then a wide basin (enough members to pass a test-set `keep_max_area_m2`) whose floor is 5 m. Inside it sits a separate 15 m-deep pocket with its own 25 m inner rim. With `keep_max_area_m2` set in the test so the outer basin is capped:
  - the outer basin is `Notch`;
  - the inner pocket is a kept lake;
  - `drainage_check` is `Ok`.

  It must fail today, because the inner pocket is simply cut.
- [ ] **Step 2: Implement.** Keep the existing C1-a and pocket logic untouched.
- [ ] **Step 3:** Run the tests and commit `Water: a drained giant basin keeps the lakes inside it`.

---

### Task 5: Every open lake says where it drains (carry-forward I5)

**Files:**
- Modify: `src/hydrology/mod.rs` (`Body` gains `downstream: Downstream`), `src/hydrology/bake.rs` (computed in `record_of`)

**The rule:**
- A closed lake has `Downstream::Sink`.
- For an open lake, follow `routing.receiver` from `receiver[lake_entry]`, step by step (bounded by `graph.len()`):
  - the first node where a reach starts gives `Reach(id)`;
  - the first lake member of a **different** lake gives `Body(that body's id)`;
  - an ocean node gives `Ocean`;
  - `NO_NODE` gives `Sink`.
- `outlet_reach` stays as it is. It is `Some(id)` exactly when `downstream == Reach(id)` at the first step; assert this in a test.

- [ ] **Step 1: Failing test** `every_open_lake_says_where_it_drains`. On the bake test world and seed 1 at 12k nodes: every fresh body's `downstream` is not `Sink`; every closed body's is `Sink`; and following `Body` links from any fresh body never loops and ends at `Ocean` (via reaches and bodies). Also add a hand fixture where a lake drains straight into another lake with no reach between them: `Body(id)`.
- [ ] **Step 2: Implement.** `Body` derives stay as they are.
- [ ] **Step 3:** Run the tests and commit `Water: every open lake names where its water goes`.

---

### Task 6: SCHEMA 3 — the §7 fields and a record that keeps only what matters

**Files:**
- Modify: `src/hydrology/record.rs`, `src/hydrology/bake.rs`, `src/hydrology/mod.rs`
- Modify: `viewer/public/app/engine.js` (`hydroSummary`), `viewer/public/app/water-preview.js` (`decodeHydro`), `viewer/test/hydro.test.mjs`, `viewer/test/water-preview.test.mjs`

**The layout (the order is the contract):**

| Section | Words |
|---|---|
| Header | 32 words: words 0–19 as SCHEMA 2 (word 0 = 3.0), then 20 `total_nodes`, 21 `wetness_nodes`, 22 `keep_depth_m`, 23 `keep_area_m2`, 24 `pond_max_area_m2`, 25 `keep_max_area_m2`, 26 `min_stream_nodes`, 27 `notch_fall_m`, 28 `evaporation_factor`, 29 `salt_flat_share`, 30 `forced_requested`, 31 `forced_matched` |
| Body | 14 words plus the outline: SCHEMA 2's 12 (id, kind, fresh, enclosed, forced, level, area, depth, outlet_reach, anchor lat, anchor lon, outline_len), then `downstream_kind` (0 Reach, 1 Body, 2 Ocean, 3 Sink) and `downstream_id` (-1 unless Reach or Body), inserted **before** `outline_len`. The outline pairs follow `outline_len` as before, so outline_len is word 13. |
| Reach | 7 words plus the points: id, class, order, downstream_kind, downstream_id, `fresh` (0 if its downstream chain ends in a closed lake's sink, else 1), point_count; then the points (6 words each, unchanged) |
| Notch | point_count, then per point `lat, lon, bed_m, width_m` (4 words) |
| Fall | unchanged |

**What `forced_matched` counts:** forced outlets whose nearest node is a submerged member of a kept lake. A forced point that misses (it lands on land, on the ocean, or on a shore) is counted in `forced_requested` but not in `forced_matched`. The studio can then say "1 of 2 forced outlets matched".

**Which notches are recorded** (this replaces 12b-2):
- (a) Every outlet cut from `close_lakes` (`closure.outlet_notch`), recorded whole.
- (b) Any other notch, reduced to the points that are **not** channel nodes of a recorded reach (a reach's bed already carries the cut) **and** whose cut depth `graph.height_m − bed ≥ NOTCH_RECORD_MIN_CUT_M = 2.0`. A notch with no such points is dropped.
- A point's width: for outlet cuts, `width_m(flow at the node)`; otherwise `width_m(max(flow, effective stream threshold))`. That is at least the 3 m stream anchor.

- [ ] **Step 1: Failing tests.**
  - The round-trip test at SCHEMA 3.
  - A test that SCHEMA 2 input is refused by `decode`.
  - `the_record_keeps_only_notches_that_matter`: every recorded notch point is either on an outlet cut, or off every reach node with a cut depth of at least 2 m.
  - `forced_outlets_report_how_many_matched`: one point inside a pocket and one on dry land give 2 requested, 1 matched.
  - `a_reach_into_a_closed_lake_is_not_fresh`.
  - Node tests: `decodeHydro` consumes a real SCHEMA 3 bake exactly, `hydroSummary` reads `forcedRequested`, `forcedMatched` and the params echo, and the viewer refuses SCHEMA 2 with a clear message.
- [ ] **Step 2: Implement.**
- [ ] **Step 3:** Run `cargo test -p worldbuilder-engine hydrology`, `--test no_std_math`, and `node --test test/hydro.test.mjs test/water-preview.test.mjs` from `viewer/`. Commit `Water: record schema 3 - where lakes drain, what was forced, and only the cuts that matter`.

---

### Task 7: Rebuild, parity, pins, and the owner world

**Files:**
- Modify: `viewer/public/wasm/*`, `.github/workflows/gates.yml`, `crates/worldbuilder-engine/README.md`, `docs/superpowers/reports/2026-09-10-water-1a-carry-forward.md`
- Create: `docs/superpowers/reports/2026-09-11-water-1b1-verification.md`

- [ ] **Step 1:** Rebuild the wasm and re-run parity plain plus all seven controls. Plain must be 0 divergent; if any H word diverges, STOP. Update every pin pair and the `TCTL` prediction's field if it is recomputed natively, with a dated note.
- [ ] **Step 2:** Re-derive the five engine count pins with `--list` and `assert_counts.py`, confirm Python is 565, and mirror the README.
- [ ] **Step 3:** Run the full engine suites (default and wasm) and the whole viewer suite; report the counts.
- [ ] **Step 4 (controller):** bake the owner world in the studio at 1M nodes, using the studio's **preview water** button once the branch is served. Record:
  - time, heap and record MB (target ≤ 8 MB; report it honestly if not met);
  - body and reach counts;
  - how many fresh lakes have `downstream == Sink` (must be 0);
  - `forced_matched`;
  - the great lake's full path to the ocean;
  - how many inner lakes the capped basins kept.

  Then write the verification report.
- [ ] **Step 5:** Update the carry-forward doc: strike what is done, and keep what moves to 1b-2 or stage 2.
- [ ] **Step 6: Commit** `Water 1b-1: parity and pins re-derived, verified on the owner's world`.

## Self-review notes

- **Carry-forward coverage:**
  - the residual cycle is Task 3;
  - I2 is Task 2;
  - I3 is Task 4;
  - I5 is Task 5;
  - the record size and §7 fields are Task 6, which also covers the forced-miss report;
  - the `mod.rs` split is Task 1.
- **Deferred to 1b-2:** refinement tracing, lake outlines, small lakes and ponds, falls.
- **Deferred to stage 2:** the width-anchor ruling, the `wasm.rs` hydro split, and the `reaches_are_acyclic` O(R²) cost. The rest of the carry-forward's minor items ride along only where a task touches their code.
- **Type consistency:**
  - `Body.downstream` reuses `Downstream`;
  - `Routing.committed` is added in Task 3 and used only by the cuts;
  - `Hollow.capped` is added in Task 4;
  - `FloodQueue::push_tied` is added in Task 2.
