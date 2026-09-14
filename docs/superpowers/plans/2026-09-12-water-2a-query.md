# Water 2a: The Query Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Anything that can name a point on the planet can ask what water is there — `water_at(point) -> { kind, level_m, depth_m, fresh, body_id }` — from the record the bake already produces, in Rust, in the browser and in Python.

**Architecture:** Two new modules under `src/water/`. An index buckets every recorded reach segment, notch segment and body extent into cells so a sample tests a handful of candidates instead of the whole record; a query answers spec §8.3's table against those candidates. Nothing carves, nothing is saved, and `elevation_m` returns the same bits it does today — that is plan 2b. The wasm side caches one index per held bake, and exposes a single-point export and a per-tile batch for the relief workers.

**Tech Stack:** Rust (`crates/worldbuilder-engine`, detmath only), wasm via `npm run build:wasm`, PyO3 through `bindings.rs`, the viewer's ES modules with `node --test`.

**Spec:** `docs/superpowers/specs/2026-09-10-automatic-water-design.md`. Binding sections: §8.2 (the index), §8.3 (the query), §7 (what the record means), §14.9 (the property this plan exists to satisfy). Stage 2's other half — the carve, the worldfile block and the version bump — is plan 2b and this plan must not start it.

**Branch:** `water-query`, from master at 8961b9f (plan 1b-4 merged).

## What the record already guarantees, and what this plan may assume

Plans 1a to 1b-4 measured and recorded all of this; the query may rely on it and must not re-derive it.

- A body's `outline` is either a **shore-point set** (`shore_member_count > 0`: that many shore members first, then the collar, each ascending by node index, no edges) or a **traced ring** (`shore_member_count == 0`: joined in order, closing implicitly, at least 3 points). `shore_member_count` is the discriminator, and a decoder already refuses a record where it exceeds the outline's length.
- `shore_reach_m` is the longest member-to-collar step whose collar end stands above the body's level. It is finite and at least zero, enforced at decode. On the owner's world its median is 58,083 m and its maximum 62,586 m; **26 of 1,042 bodies measured across 42 stand-in bakes carry a band of exactly zero**, and those bodies are answered by the nearest-point clause alone.
- Ruling T1-3's tie-break is not optional: the final review of plan 1b-4 measured a real point that two bodies' extents both admit, resolved correctly by the smaller `dm` with a 332 km margin.
- Reach points carry `bed_m`, `width_m`, `depth_m` and `flow_m2`; a reach's `fresh` means its chain reaches the ocean, a body's means it is not closed.
- The record's levels and beds are all derived from the **landform** (`Surface::structural_m`), never from the detail field. Ponds are the one thing found in the detail field, and they are recorded as rings, so the query never has to know that.

## Global Constraints

- **No std float maths outside `detmath.rs`.** `tests/no_std_math.rs` scans `src/`, `examples/`, bins and test modules. Use `crate::detmath as m`. There is no `ceil`: write `-m::floor(-x)`. No `.abs()`: write the comparison.
- **Casts:** `as u32`, `as u64`, `as i32`, `as i64` and float→`usize` casts need `// cast-ok: <reason>` on the same line.
- **No `f64::min`, `f64::max` or `.clamp(`.** Write explicit `if`/`else`. Sort floats with `total_cmp`.
- **No panics reachable from `extern "C"`.** Every new export validates its arguments and returns a status; a hostile or stale handle is refused, never indexed.
- **Determinism:** no HashMap or HashSet order in output; ties broken by a stated rule (lower index, then lower id).
- **`elevation_m` must return the same bits.** This plan adds no stage to `Surface` and changes no existing output. Parity's existing groups must not move; the new ones are additions.
- **`Surface` gains no field.** That is plan 2b's business.
- **Every `src/` edit changes the fingerprint.** Rebuild the wasm (`npm run build:wasm` in `viewer/`) in the tasks that say so, and commit it.
- **The fingerprint hashes working-tree bytes.** Before any wasm rebuild run `git ls-files --eol crates/worldbuilder-engine | grep -v "w/lf"`; if it prints a file, delete it and `git checkout --` it first. Plan 1b-3 lost an hour to this.
- **The record layout's four twins** stay in step if any of them changes: `src/hydrology/record.rs`, `viewer/public/app/engine.js`, `viewer/public/app/water-preview.js`, `crates/worldbuilder-engine/tests/wasm_exports.rs`. This plan should not need to change the layout at all; if it does, that is a finding, not a step.
- **CI pins** (currently engine 801/801/803/907/909 with 8 ignored; parity 150,830 compared / 0 divergent, seed control 145,274, tectonic control 22,993; Python 565/157) are re-derived once, in the last task, by running them. The crate README mirrors `.github/workflows/gates.yml`.
- **Verification numbers** come from runs, each stated with its population, method and host. The owner's world is `worlds/world-1788998299904.json` at 1M nodes, baked in the studio.
- Only the wb-clean test database may ever be written to; nothing here touches a database.

## Rulings made while writing this plan

| # | Ruling | Why | Cost if wrong |
|---|---|---|---|
| Q-1 | The index is the crate's own `BucketIndex` shape — latitude rows, longitude columns sized so a cell is about `cell_m` square everywhere — not spec §8.2's "cube-sphere cell grid". Task 6 corrects the spec text. | `BucketIndex` already exists, is deterministic, is tested against brute force including the poles and the ±180 seam, and is what every other spatial question in this module uses. A second grid is a second thing to get wrong. | None functionally; the spec's word changes. |
| Q-2 | The index is **derived state, built from a decoded record**, and is never recorded or transmitted. In wasm it is built on first query for a held bake id and cached beside it; freeing the bake frees the index. | The record is the contract; an index on the wire would be a second copy to keep honest. | A first query pays the build; measured in Task 6. |
| Q-3 | The query reads the ground through a closure the caller supplies, exactly as `refine::Ground` does, and the callers pass `Surface::structural_m`. | The record's levels are landform-derived, so the level test must ask the same surface. It also keeps the query testable on hand-written ground. | A caller could pass `elevation_m` and get a shoreline that moves with the texture. The doc comment says so in as many words. |
| Q-4 | **Ocean** is decided as: the landform at the point is at or below the datum **and** the point is inside no recorded body's extent. Recorded bodies are what carve lakes out of the below-datum set. | Ruling W1 already says the record is authoritative — "Recorded body ids are what `water_at` answers from, not a re-run of connectivity" — and the query has no graph to re-run connectivity on. | A below-datum point in a basin the bake did not record answers `ocean`. That is what the record says it is. |
| Q-5 | Precedence is the spec's table order: **ocean, then a body, then a river, then none.** Within bodies, Ruling T1-3 decides: smaller `dm` wins, ties to the lower body id. | It is what §8.3 already states, and reaches end at a shore, so a river inside a lake is not a case that arises. | A river mouth inside a lake's extent answers as the lake; the reach's own last point is at the lake's level anyway. |
| Q-6 | Depth: for a body, the level minus the landform, and never below zero; for a river, the reach's own `depth_m` at the nearest recorded point. `level_m` for a river is that point's `bed_m + depth_m`. | §8.3 says exactly this for the river; the body case is the only reading that varies across a lake rather than reporting one number for the whole thing. | A body's depth is a landform depth, not a bathymetric one. |
| Q-7 | A reach's influence is a half-width band around each recorded segment, and the width used is the **larger of its two endpoints'** `width_m`. | A segment's width tapers between recorded points; taking the larger cannot answer `none` inside a channel the carve will cut. | A strip up to half the width difference is called river where the carve tapers it narrower. |
| Q-8 | The tile batch answers a rectangle of samples in one call, writing four `f64`s per sample: kind, level, depth and body id. It does not interpolate and it does not smooth. | The relief workers already take tiles this way (`wb_fill_tile_f32`), and any smoothing belongs to drawing, not to the query. | None. |

## File Structure

| File | Responsibility |
|---|---|
| `src/water/mod.rs` (new) | `WaterAt`, `WaterKind`, `Ground`-style ground closure, the module doc that states Q-3 to Q-7 |
| `src/water/index.rs` (new) | `WaterIndex`: build from a `HydroRecord`, and `candidates(point)` |
| `src/water/query.rs` (new) | `water_at`, and the clause functions it is made of |
| `src/water/query_tests.rs` (new, `#[cfg(test)]`) | the hand fixtures and the world-scale properties |
| `src/lib.rs` | `pub mod water;` |
| `src/wasm.rs` | `wb_water_at`, `wb_water_tile`, the per-bake index cache, and freeing it with the bake |
| `src/bindings.rs` | the PyO3 `water_at` |
| `crates/worldbuilder-engine/parity/parity_dump.rs`, `parity/parity.mjs` | a `water_at` group |
| `tests/wasm_exports.rs` | the two new exports' argument validation |
| `viewer/public/app/engine.js` | `waterAt` and `waterTile` wrappers |
| `.github/workflows/gates.yml`, `crates/worldbuilder-engine/README.md` | pins (Task 6) |
| the spec, a new verification report, the carry-forward | Task 6 |

All paths are relative to `crates/worldbuilder-engine/` unless they start with `viewer/`, `docs/` or `.github/`.

---

### Task 1: The index

**Files:**
- Create: `src/water/mod.rs`, `src/water/index.rs`
- Modify: `src/lib.rs`
- Test: `index.rs`'s test module

**Interfaces:**
- Consumes: `hydrology::{HydroRecord, Body, ReachLine, NotchLine}`, `hydrology::buckets::BucketIndex`, `sphere::SpherePoint`.
- Produces:

```rust
/// About 50 km, spec §8.2. A cell holds every item whose influence reaches it.
pub const DEFAULT_CELL_M: f64 = 50_000.0;

pub struct WaterIndex { /* private */ }

pub struct Candidates<'a> {
    pub bodies: &'a [u32],
    pub reaches: &'a [u32],
    pub notches: &'a [u32],
}

impl WaterIndex {
    pub fn build(record: &HydroRecord, radius_m: f64, cell_m: f64) -> WaterIndex;
    /// Every item whose influence may reach `point`, ascending by id, deduplicated.
    pub fn candidates(&self, point: &SpherePoint) -> Candidates<'_>;
    pub fn cell_m(&self) -> f64;
    /// For the survey: cells occupied, the largest cell's item count, and the totals.
    pub fn stats(&self) -> (usize, usize, usize, usize, usize);
}
```

- [ ] **Step 1: Write the failing tests** in `index.rs`. Build a small record by hand — one lake with a shore-point extent, one pond with a ring, one reach of three points, one notch of two — and assert:
  - a point on a shore member's own position lists that body;
  - a point `shore_reach_m * 0.9` away from the nearest shore member, in a cell the body has no point in, **still lists that body** (the dilation of Ruling Q-1 and spec §8.2);
  - a point two cells beyond `shore_reach_m` does not list it;
  - a body whose `shore_reach_m` is 0.0 is still listed in the cells its own points fall in (26 of 1,042 real bodies are like this);
  - a point on a reach's segment lists that reach, and one a whole cell away does not;
  - `candidates` is ascending and deduplicated, and two builds of the same record give the same lists.

  Run `cargo test -p worldbuilder-engine --lib water::index`. Expected: FAIL, nothing defined.

- [ ] **Step 2: Implement.** For each body: insert every outline point's cell, and — when `shore_member_count > 0` and `shore_reach_m > 0.0` — every cell within `shore_reach_m` of each **shore member** point (the collar's band is not dilated; the band is measured from members). For each reach and notch: insert every point's cell and every cell the segment between consecutive points passes through, plus half the segment's width. Store the ids per cell as sorted deduplicated `Vec<u32>`, built by sorting once at the end — never a HashSet.

  `BucketIndex` indexes points, not areas, so use it for the row/column arithmetic and keep your own `Vec<Vec<u32>>` payloads; read `buckets.rs` first and reuse `candidates(point, reach_m)` where it fits rather than re-deriving cell geometry.

- [ ] **Step 3: A world-scale test** in `query_tests.rs` (create it in this task, `#[cfg(test)] mod` from `water/mod.rs`): on a 12k-node bake, every recorded body, reach and notch appears in the candidates of every one of its own recorded points. Assert on all three counts and print the index's stats.

- [ ] **Step 4: Mutation guard.** Delete the dilation (index only the outline points' own cells). The dilation test must FAIL. Restore, and paste both outputs.

- [ ] **Step 5: Commit** `Water 2a: an index over the record, dilated by each body's own band`.

---

### Task 2: The query

**Files:**
- Create: `src/water/query.rs`
- Modify: `src/water/mod.rs`
- Test: `query.rs`'s test module

**Interfaces:**
- Consumes: Task 1's `WaterIndex`, the record, and a ground closure.
- Produces:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaterKind { None, Ocean, Lake, SaltLake, SaltFlat, Pond, River }

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaterAt {
    pub kind: WaterKind,
    pub level_m: f64,
    pub depth_m: f64,
    pub fresh: bool,
    /// The body this answer belongs to, or `u32::MAX` for ocean, river and none.
    pub body_id: u32,
}

pub const NO_BODY: u32 = u32::MAX;

/// Spec §8.3. `ground_m` is the **landform** at a point (Ruling Q-3): pass
/// `Surface::structural_m`, never `elevation_m`, or the shoreline moves with the texture.
pub fn water_at(
    record: &HydroRecord,
    index: &WaterIndex,
    ground_m: &dyn Fn(&SpherePoint) -> f64,
    point: &SpherePoint,
) -> WaterAt;
```

- [ ] **Step 1: Write the failing tests.** One hand-built record, and one case per branch of §8.3's table:
  1. **A lake's interior** — a point whose nearest recorded point is a shore member and whose ground is below the level: `Lake`, `level_m` the body's, `depth_m` the level minus the ground, `fresh` the body's, `body_id` the body's.
  2. **Beyond the shore** — ground above the level, inside the extent: `None`.
  3. **Outside the extent** — nearer a collar point than any member, and further than `shore_reach_m`: `None`, even with ground below the level.
  4. **The band** — nearer a collar than a member but within `shore_reach_m`, ground below the level: the body.
  5. **A zero band** — `shore_reach_m == 0.0`, so only clause 1 can admit: assert both sides of it.
  6. **Two bodies claiming one point** (Ruling T1-3) — the smaller `dm` wins; with equal `dm`, the lower id.
  7. **A pond** — inside its ring and below its level: `Pond`; outside the ring: `None`. Use a ring of at least 4 points with a concave notch, so an even-odd test that is subtly wrong fails.
  8. **A river** — within half the wider endpoint's width of a segment: `River`, `level_m` the bed plus depth, `depth_m` the reach's depth, `fresh` the reach's, `body_id` `NO_BODY`. Just outside: `None`.
  9. **Ocean** — ground at or below the datum, in no body's extent: `Ocean`, level 0, depth the ground's depth below the datum, `fresh` false.
  10. **Ocean loses to a recorded body** (Ruling Q-4) — an enclosed body at level 0 whose extent contains a below-datum point answers that body, not `Ocean`.
  11. **A salt body** answers `SaltLake` or `SaltFlat` by its kind, with `fresh` false.

  Write each one out in full. Run `cargo test -p worldbuilder-engine --lib water::query`. Expected: FAIL.

- [ ] **Step 2: Implement** in the order Ruling Q-5 fixes: gather candidates; test bodies (each by its own branch, keeping the best by `dm` then id); if none claims the point, test the ocean; then the reaches; else `None`. Note the one subtlety: the **ocean test must run after the body test** even though it is first in the table, because a body's claim beats it — write it as "a body claimed it, else if the ground is at or below the datum, ocean".

  Keep each clause a named function — `body_claim`, `pond_claim`, `river_claim` — so the tests can drive them directly and a future reader can find §8.3's clauses one for one.

- [ ] **Step 3: Mutation guard.** Break the tie-break to "lower id wins" regardless of `dm`. Test 6 must FAIL. Then break the band to `dm < dc` only. Tests 4 and 5 must FAIL. Restore both and paste the outputs.

- [ ] **Step 4: Commit** `Water 2a: water_at answers the record, clause by clause`.

---

### Task 3: The query agrees with the record

**Files:**
- Modify: `src/water/query_tests.rs`
- Test: the same

This is spec §14.9, and it is the property the whole plan exists to satisfy: *the query agrees with the record at every sampled point of every body outline and reach.*

**Interfaces:** consumes Tasks 1 and 2 plus `hydrology::bake_stages`/`record_of`/`bake`.

- [ ] **Step 1: Write the property.** On each of `bake_tests.rs`'s populations — `params()`, `junction_params()`, `ranges_world()` — bake, build the index, and assert:
  - **every shore member point** of every coarse body answers that body's kind with that body's id. A shore member is a submerged node of that body, so this must hold exactly;
  - **every pond's ring vertex** answers that pond, or a neighbour that claims it by Ruling T1-3 — assert the answer is *some* body and record how many are the pond's own;
  - **every reach point** answers `River` with `level_m` equal to `bed_m + depth_m` within 1e-6, **or** a body where the reach ends in one (its last point is inside a lake by construction). Count and report both;
  - **every notch point** answers `River` or `None` — a notch is a cut, not standing water — and report the split;
  - a sample of collar points: report how many answer their own body (the band admits some, by design) and assert none answers a body whose level is below the ground there.

  Print every count. These numbers are Task 6's verification table.

- [ ] **Step 2: Sample between the points too.** For every reach, sample the midpoint of each segment and assert it answers `River` unless a body claims it. This is the case a point-only property misses — plan 1b-4's Task 3 was caught by exactly that gap.

- [ ] **Step 3: Mutation guard.** Make `body_claim` ignore `shore_reach_m` (clause 1 only). Step 1's shore-member assertion must still pass — it is clause 1 — and Step 2's midpoint assertion must FAIL somewhere. If it does not, say so and find a mutation that the property does catch; a property that cannot fail proves nothing.

- [ ] **Step 4: Commit** `Water 2a: the query agrees with the record on every recorded point`.

---

### Task 4: The wasm exports

**Files:**
- Modify: `src/wasm.rs`, `tests/wasm_exports.rs`, `viewer/public/app/engine.js`
- Test: `tests/wasm_exports.rs`, `viewer/test/hydro.test.mjs`

**Interfaces:**
- Produces:

```rust
/// Answers one point against a held bake. Writes four words — kind, level_m, depth_m, body id —
/// and returns WB_OK, or a status and nothing written.
pub extern "C" fn wb_water_at(world: u32, bake: u32, latitude_deg: f64, longitude_deg: f64, out: *mut f64, out_len: u32) -> u32;

/// Answers a rectangle of samples in one call (Ruling Q-8): `rows * columns` samples spanning
/// [lat0, lat1] x [lon0, lon1] inclusive, row-major from the north-west, four words each.
pub extern "C" fn wb_water_tile(world: u32, bake: u32, lat0: f64, lon0: f64, lat1: f64, lon1: f64, rows: u32, columns: u32, out: *mut f64, out_len: u32) -> u32;
```

- `WaterKind` crosses the boundary as `0 None, 1 Ocean, 2 Lake, 3 SaltLake, 4 SaltFlat, 5 Pond, 6 River`, and `fresh` is folded into the kind's row rather than a fifth word: the caller reads `fresh` from the body id's record entry when it needs it. **Say that in the export's doc comment.**

- [ ] **Step 1: The index cache.** Beside `HYDRO`, hold `Option<(HydroRecord, WaterIndex)>` per bake id, built on first query and dropped by `wb_hydro_free`. Decode failure is a status, never a panic. Read how `HYDRO` is declared and freed and follow it exactly.

- [ ] **Step 2: Write the failing tests** in `tests/wasm_exports.rs`, in that file's style:
  - a stale or zero world handle, a stale or zero bake id, a null `out`, a short `out_len`, `rows` or `columns` of zero, and a `rows * columns` that overflows — each returns the right status and writes nothing;
  - a real bake on a small world: `wb_water_at` at a shore member's own latitude and longitude answers that body's kind and id;
  - `wb_water_tile` over a 4×4 rectangle agrees, sample for sample, with sixteen `wb_water_at` calls at the same points;
  - the second `wb_water_at` on the same bake is served from the cache — assert by behaviour, not by timing: free the bake and confirm the next call is refused rather than answering from a stale index.

- [ ] **Step 3: Implement**, then wire `engine.js`: `waterAt(handle, bakeId, lat, lon)` returning a decoded object, and `waterTile(handle, bakeId, box, rows, columns)` returning a `Float64Array`. Add a `hydro.test.mjs` case that bakes the test world through the wasm, queries a body's own anchor and gets that body back.

- [ ] **Step 4: Run and commit.** All five gates.yml feature configurations, `no_std_math`, the CRLF guard, `npm run build:wasm`, the viewer suite. Commit `Water 2a: the browser can ask what water is at a point`.

---

### Task 5: The Python binding

**Files:**
- Modify: `src/bindings.rs`, and the Python conformance suite where it lives
- Test: the Python suite

**Interfaces:** a `water_at` PyO3 function taking the same arguments as the wasm export, in the style of the module's existing functions, returning a tuple or a dict — match what the suite's neighbours do.

Note for the implementer: spec §8.3 says "The PyO3 work adds the missing `surface_open` family the Python oracle already calls." **That is stale — `surface_open` exists and `evennia_roundtrip/planet.py:196` already calls it.** Say so in the report; Task 6 corrects the spec.

- [ ] **Step 1:** read `bindings.rs`'s existing functions and the conformance suite's shape. Report what the suite's neighbours look like before writing anything.
- [ ] **Step 2:** write a failing Python test that asks for water at a known body's anchor on a small bake and expects that body's kind and id.
- [ ] **Step 3:** implement the binding, run the test, and run the whole Python suite.
- [ ] **Step 4:** commit `Water 2a: Python can ask what water is at a point`. Do not re-pin; Task 6 does.

---

### Task 6: Parity, pins, and the owner's world

**Files:**
- Modify: `parity/parity_dump.rs`, `parity/parity.mjs`, `src/bin/hydro_survey.rs`, `.github/workflows/gates.yml`, `README.md`, the spec, the carry-forward
- Create: `docs/superpowers/reports/2026-09-12-water-2a-verification.md`

- [ ] **Step 1: Parity.** Add a `water_at` group: on the existing `plain` world's hydro bake, sample a fixed grid — 32 by 32 over a box that contains land, water and at least one recorded body — and dump kind, level, depth and body id per sample. Both sides must agree exactly. Report the group's word count and the corpus's new total.
- [ ] **Step 2: The survey.** `hydro_survey` reports the index: cells occupied, the largest cell's item count, the build time, and the mean candidates per query over a fixed sample of 10,000 points. Run it on the three 1M stand-ins and put the table in the report. **Gate:** a query's mean candidate count must be under 50; if it is not, the cell size is wrong and the lever is `DEFAULT_CELL_M`.
- [ ] **Step 3: Pins.** Re-derive every one by running it, update gates.yml and the README mirror with a dated note (2026-09-12, plan 2a), and run both ignored sweeps.
- [ ] **Step 4 (controller):** bake the owner's world in the studio, then sample the query against it: every body's anchor answers that body; a grid of 10,000 points reports its kind histogram; and the great lake's own anchor answers `Lake` with its level. Report the query's wall time for 10,000 points.
- [ ] **Step 5: The verification report**, in plan 1b-4's form, with population, method and host for every figure: Task 3's agreement counts, the index stats, the parity group, the owner-world histogram, and the pins.
- [ ] **Step 6: The spec.** Correct §8.2's "cube-sphere cell grid" to the crate's bucket grid (Ruling Q-1); state Rulings Q-3, Q-4, Q-6 and Q-7 where §8.3 describes the test; and strike the stale sentence about `surface_open` being missing.
- [ ] **Step 7: The carry-forward.** Tick what this plan closed and leave plan 2b's list: the carve, the detail damping, the `hydrology` block and fingerprint, and the `GENERATOR_VERSION` decision.
- [ ] **Step 8: Commit** `Water 2a: pins re-derived, and the query verified on the owner's world`.

---

## Self-review

**Spec coverage:**

| Spec item | Where |
|---|---|
| §8.2 the index, 50 km cells, what a cell lists | Task 1 (Ruling Q-1 on its shape) |
| §8.2's performance target (`elevation_m` no more than 20% slower) | **plan 2b** — this plan does not touch `elevation_m` |
| §8.3 the query's table, all five kinds | Task 2 |
| §8.3's tie-break (Ruling T1-3) | Task 2, test 6 |
| §8.3 exposed as a wasm export and a per-tile batch | Task 4 |
| §8.3 exposed as a PyO3 binding | Task 5 |
| §8.3's stale `surface_open` sentence | Task 5 reports it, Task 6 fixes it |
| §14.9 the query agrees with the record | Task 3 |
| §14.1 determinism | Tasks 1 and 2's tests, parity in Task 6 |
| §8.1 the carve, §7's block and fingerprint, the version bump | **plan 2b**, deliberately |

**Types:** `WaterIndex`, `Candidates`, `WaterKind`, `WaterAt`, `NO_BODY`, `water_at`, `DEFAULT_CELL_M`, `wb_water_at`, `wb_water_tile` are used with the same names throughout.

**Known soft spots, named rather than hidden:**
- Task 1's dilation is the only place the index can be wrong in a way the query cannot detect: a body missing from a cell answers `None` silently. Task 3's world-scale property is what catches it, and Task 1's own mutation guard is what proves the property bites.
- Ruling Q-4 makes the record authoritative for the ocean. If a future bake stops recording a basin it used to, that basin becomes ocean with no error.
- The owner world's `shore_reach_m` runs to 62,586 m against 50 km cells, so most bodies dilate into their neighbours' cells. Task 6's candidate-count gate is what says whether that is affordable.
