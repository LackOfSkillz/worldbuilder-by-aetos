# Water 1b-2: Fine Channels Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every reach in the hydro record is re-traced on the landform at 1.5 km steps, with waterfalls and meanders. Notch lines gain their ends and split where the graph doesn't join them. The record counts what capped basins keep, which closes carry-forward item I3.

**Architecture:** A new module, `hydrology/refine.rs`, runs after `record_of`. It walks each coarse segment (one reach point to the next) in fine stations, takes the lowest ground within a corridor, keeps the bed from ever rising, trims the reach at the shore, finds falls, applies a meander on flat wide rivers, then simplifies. Coarse points are kept exactly, so tributary junctions stay shared. The record moves to SCHEMA 4: eleven more header words (capped-basin counts and the refinement params). The body, reach, notch and fall layouts stay as they are.

**Tech Stack:** Rust (`crates/worldbuilder-engine`, detmath only), wasm via `npm run build:wasm`, the viewer's ES modules with `node --test`.

**Spec:** `docs/superpowers/specs/2026-09-10-automatic-water-design.md`. Binding sections: §6.6 (tracing), §6.7 (falls), §7 (record), §14 (properties). The carry-forward is `docs/superpowers/reports/2026-09-10-water-1a-carry-forward.md`.

**Scope ruling (plan-level):** spec §6.6 also asks for **lake outlines at 250 m** and **small lakes and ponds**. Those go to plan **1b-3 (shores)**, which starts with a spike, because a 250 m fill of the 41.33M km² great lake would be about 6.6×10⁸ cells, and a 250 m outline of it would break the 8 MB record target alone. The outline method must be decided on measurements first. This plan does the channels, which stage 2's carving needs, and fixes the notch geometry stage 2 blocks on. Cost if wrong: none, since outlines were never in this plan's code path.

## Global Constraints

- **No std float maths outside `detmath.rs`.** `tests/no_std_math.rs` scans `src/`, bins included. Use `crate::detmath as m` (`sin`, `cos`, `sqrt`, `hypot`, `atan2`, `powf`, `floor`, …). There is no `ceil`: write `-m::floor(-x)`.
- **Casts:** `as u32`, `as u64`, `as i32` and `as i64` need `// cast-ok: <reason>` on the same line. So does a float→`usize` cast.
- **No `f64::min`, `f64::max` or `.clamp(`.** Write explicit `if`/`else`. Sort floats with `total_cmp`.
- **No panics reachable from `extern "C"`** (`wb_hydro_bake` calls `hydrology::bake`).
- **Determinism:** no HashMap order in output, fixed candidate order, ties to the first candidate in a stated order.
- **`Surface` gains no field.** Commit subjects name no third party. End every commit message with a blank line and `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`.
- **"Everything drains" is binding.** `drainage_check` still passes in `bake_stages`, and the ignored sweep `cargo test --release --lib every_small_world_drains -- --ignored` still passes. Refinement never changes routing.
- **Every `src/` edit changes the fingerprint.** Rebuild the wasm (`npm run build:wasm` in `viewer/`) in the tasks that say so, and commit it.
- **CI pins** (currently engine 710/710/712/816/818 with 6 ignored; parity plain 132,472/0, seed control 126,737, tectonic control 10,013; Python 565) are re-derived once, in Task 8, by running them and never by transcribing. The crate README mirrors `.github/workflows/gates.yml`.
- **The record layout's twins** change together: `src/hydrology/record.rs`, `viewer/public/app/engine.js::hydroSummary`, `viewer/public/app/water-preview.js::decodeHydro`, and their tests.
- **Spec §14.5, Downhill:** along a reach the bed never rises. After this plan that holds with no exceptions, including at a mouth.
- **Spec §14.4, Connected:** every tributary's last point is bit-identical (lat, lon) to its receiver's first point.
- **Size and time targets (spec §7, §15):** the owner world's record is at most 8 MB, and the whole wasm bake takes at most 300 s. Both are measured in Task 8.
- **Verification numbers** come from runs, and each states its population, method and host.

## Rulings made while writing this plan

| # | Ruling | Why | Cost if wrong |
|---|---|---|---|
| R-1 | Coarse reach points are kept exactly; tracing runs between each consecutive pair. | Shared junction vertices (§14.4) come for free, and coarse beds already fall. | The fine line bends at every coarse point, up to a graph spacing apart. |
| R-2 | A fine dip met while tracing is not judged as a lake. The bed holds level across it. | New bodies mid-reach would change routing after `drainage_check`. Ponds are plan 1b-3's fine search, with their own keep rule. | Some fine lakes along rivers wait for 1b-3. |
| R-3 | A non-terminal segment never steps onto ground at or below the datum. The terminal segment ends at the first station whose ground is at or below the water it runs into (the shore trim). | "Rivers end at the coast or at a lake shore" (§6.6), and lowest-ground search must not wander into the sea along a coast. | A river running along a below-datum inland flat keeps to the higher side. |
| R-4 | A mouth's bed is `min(previous bed, water level)`. | It removes the I4 side effect (a mouth's bed rising at the last step), so §14.5 holds everywhere. | Stage 2 carves a mouth slightly below the water, which is under water anyway. |
| R-5 | A fall is recorded as its upper end (`Fall.at`) and height. Both ends are inserted as reach points, protected from simplification. The lower end is the next point. | §6.7 names two ends, and §7's 4-word fall layout stays. Stage 2 then carves a real step, not a 1.5 km ramp. | None: the layout is unchanged. |
| R-6 | Meander only where it can be drawn: wavelength (11 widths) at least 4 steps (6 km), segment slope under 0.2%, no fall in the segment. The amplitude is 1.5 widths, tapered to zero at coarse points, and kept inside the corridor. | A 3 m stream's 33 m wavelength cannot be drawn at 1.5 km steps; it would alias into zigzags. | Only great rivers meander. |
| R-7 | Simplification: Douglas–Peucker with horizontal tolerance `refine_simplify_m` (250 m) and vertical tolerance `refine_vertical_m` (1 m). Coarse points, fall ends and the mouth are always kept. | It is what makes 1.5 km tracing fit the 8 MB target. Task 8 measures it and raises the tolerance if needed. | A bend or bed change smaller than the tolerances is lost. |
| R-8 | The refinement params are not wasm params. A wasm bake takes `earth_like`'s values, like `min_stream_nodes`. | `WB_HYDRO_PARAMS_STRIDE` stays 12, and the studio has no controls for them yet (stage 3). | None. |

## File Structure

| File | Change |
|---|---|
| `src/hydrology/bake.rs` | Task 1 moves the tests out. Task 2 adds notch ends and splits. Task 3 adds the capped counts and params echo to the stats. |
| `src/hydrology/bake_tests.rs` (new) | The bake tests, moved verbatim from `bake.rs` (Task 1). |
| `src/hydrology/hollows.rs` | `judge` takes a precomputed forced list (Task 1). `Hollow.inner_of_capped` (Task 3). |
| `src/hydrology/routing.rs` | Passes the forced list to `judge`, and marks inner hollows (Tasks 1, 3). |
| `src/hydrology/mod.rs` | New `HydroParams` fields and `BakeStats` fields (Task 3). `bake()` calls `refine` (Task 4). `reaches_are_acyclic` becomes O(R) (Task 1). |
| `src/hydrology/record.rs` | SCHEMA 4, with 43 header words (Task 3). |
| `src/hydrology/refine.rs` (new) | Tracing (Task 4), falls (Task 5), meander (Task 6), simplification (Task 7). |
| `src/wasm.rs` | `WB_ERR_DRAINAGE` (Task 1). |
| `src/bin/hydro_survey.rs` | Times the refinement separately (Task 8). |
| `viewer/public/app/engine.js`, `water-preview.js`, and tests | The SCHEMA 4 decode (Task 3). Falls drawn in the preview (Task 8). |
| `viewer/test/parity.mjs` | Checks the bake status before reading `out_id` (Task 1). |
| `.github/workflows/gates.yml`, `crates/worldbuilder-engine/README.md` | Pins (Task 8), plus stale notes (Task 1). |
| spec, carry-forward, a new verification report | Rulings R-1 to R-8 into §6.6, §6.7 and §7; I3's outcome; the 1b-3 scope note (Tasks 4–8). |

All paths below are relative to `crates/worldbuilder-engine/` unless they start with `viewer/`, `docs/` or `.github/`.

---

### Task 1: Carry-forward minors, and the bake tests in their own file

**Files:**
- Create: `src/hydrology/bake_tests.rs`
- Modify: `src/hydrology/bake.rs`, `src/hydrology/mod.rs`, `src/hydrology/hollows.rs`, `src/hydrology/routing.rs`, `src/wasm.rs`, `viewer/test/parity.mjs`, `.github/workflows/gates.yml`, `README.md`, and `docs/superpowers/reports/2026-09-10-water-1a-calibration.md`

**Interfaces:**
- Produces: `pub fn judge(hollows: &mut [Hollow], forced: &[u32], params: &HydroParams)`. `forced` is `forced_nodes(graph, params)`, which the caller computes once.
- Produces: `pub const WB_ERR_DRAINAGE: u32`, the next free `WB_ERR_*` value in `src/wasm.rs`.
- Produces: `bake_tests.rs`, where every later task adds its bake tests.

- [ ] **Step 1: Move the tests.** Cut the whole `#[cfg(test)] mod bake_tests { ... }` block out of `src/hydrology/bake.rs` (it starts at `#[cfg(test)]` near line 469) and paste its contents into the new file `src/hydrology/bake_tests.rs`, dropping the `mod bake_tests {` wrapper and its closing brace. In `src/hydrology/mod.rs`, after `pub mod bake;`, add:

```rust
#[cfg(test)]
mod bake_tests;
```

At the top of `bake_tests.rs`, replace `use super::*;` with the imports the tests use, for example:

```rust
use crate::hydrology::bake::*;
use crate::hydrology::flood::{flood, ocean_seeds, NO_NODE};
use crate::hydrology::flow::{self, close_lakes, drainage_check};
use crate::hydrology::hollows::{self, find_hollows, judge, Fate};
use crate::hydrology::landgraph::LandGraph;
use crate::hydrology::reaches::{depth_m, width_m};
use crate::hydrology::routing::{self, route, NO_LAKE};
use crate::hydrology::{
    Body, BodyKind, BakeStats, Downstream, Fall, HydroError, HydroParams, HydroRecord, NotchLine,
    ReachClass, ReachLine, ReachPoint,
};
```

The tests also reach private helpers in `bake.rs`: `reach_points`, `body_downstream` and `downstream_is_fresh`. Make each one `pub(crate)`. Replace `super::super::bake(` with `crate::hydrology::bake(`. Add or remove imports until `cargo test -p worldbuilder-engine --lib hydrology` compiles with **no unused-import warnings**. The count of hydrology tests must be unchanged: run `cargo test -p worldbuilder-engine --lib hydrology 2>&1 | tail -3` before and after, and record both.

- [ ] **Step 2: Build `judge`'s forced list once.** In `src/hydrology/hollows.rs`, change `judge` to take the list instead of rebuilding it:

```rust
pub fn judge(hollows: &mut [Hollow], forced: &[u32], params: &HydroParams) {
    for hollow in hollows.iter_mut() {
        hollow.forced = hollow.members.iter().any(|m| forced.binary_search(m).is_ok());
        // ... the rest of the body unchanged ...
    }
}
```

Update every caller:
- `bake_stages`: add `let forced = hollows::forced_nodes(&graph, params);` before `judge(&mut hollows, &forced, params);`.
- `routing::route`: it already has `let forced = forced_nodes(graph, params);`, so pass `&forced` to its `judge` call in the capped step.
- Every test that calls `judge` (grep `judge(` under `src/hydrology`).

Run `cargo test -p worldbuilder-engine --lib hydrology`. Expected: PASS, with the same count as Step 1.

- [ ] **Step 3: Refuse absurd flow thresholds.** Write the failing test in `bake_tests.rs`:

```rust
/// Carry-forward (1b-1 final review): a finite but absurd `stream_flow_m2` used to overflow the
/// effective river threshold to infinity, so the record carried non-finite words.
#[test]
fn an_absurd_flow_threshold_is_refused() {
    let mut p = params();
    p.stream_flow_m2 = 1.0e308;
    p.river_flow_m2 = 1.0e308;
    p.great_flow_m2 = 1.0e308;
    assert!(matches!(crate::hydrology::bake(&world(), &p), Err(HydroError::Params(_))));
    let mut q = params();
    q.min_stream_nodes = 1.0e300;
    assert!(matches!(crate::hydrology::bake(&world(), &q), Err(HydroError::Params(_))));
}
```

Run it: `cargo test -p worldbuilder-engine --lib an_absurd_flow_threshold_is_refused`. Expected: FAIL (the bake returns `Ok`).

Then add bounds in `bake_stages`, after the existing `require_finite_positive` block:

```rust
/// Above any catchment a planet can hold (Earth's whole surface is 5.1e14 m^2). Flow params
/// above it are a typo, and `record_of`'s x10 steps would overflow them to infinity.
const MAX_FLOW_M2: f64 = 1.0e20;
/// More than any graph has nodes.
const MAX_MIN_STREAM_NODES: f64 = 1.0e7;
```

```rust
    if params.great_flow_m2 > MAX_FLOW_M2 {
        return Err(HydroError::Params("flow thresholds must be <= 1e20 m^2"));
    }
    if params.min_stream_nodes > MAX_MIN_STREAM_NODES {
        return Err(HydroError::Params("min_stream_nodes must be <= 1e7"));
    }
```

`great >= river >= stream` is already checked, so bounding `great` bounds all three. Run the test again. Expected: PASS.

- [ ] **Step 4: Give a drainage refusal its own wasm code.** In `src/wasm.rs`, read the `WB_ERR_*` constants (from about line 120). Add the next free value as `pub const WB_ERR_DRAINAGE: u32 = <next>;`, with a doc comment: "`hydrology::bake` refused: the routing broke 'everything drains' (`HydroError::Drainage`)". Then find where `wb_hydro_bake` maps `HydroError` (about line 4087 onwards) and map `HydroError::Drainage(_)` to `WB_ERR_DRAINAGE`, leaving `Sampling` on `WB_ERR_GRAPH`.
  - Grep `viewer/public/app/` for `WB_ERR_GRAPH` or the numeric code. If the studio names codes, add the new one with the message "the water bake refused a world whose routing did not drain".
  - Add a wasm-side test next to the existing `wb_hydro_bake` tests if one can provoke `Drainage`. It cannot on a real world, so state in a comment that the mapping is covered by inspection only.

- [ ] **Step 5: `parity.mjs` checks the status.** In `viewer/test/parity.mjs`, find case `H` (grep `wb_hydro_bake`). If the status returned is non-zero, report the case as divergent with the status, and do not read `out_id`. Run `node test/parity.mjs` (from `viewer/`, the way gates.yml does; read gates.yml for the exact command). Expected: the same counts as before.

- [ ] **Step 6: `reaches_are_acyclic` in O(R).** Replace its body in `src/hydrology/mod.rs` with a colour walk:

```rust
pub fn reaches_are_acyclic(reaches: &[ReachLine]) -> bool {
    // 0 unvisited, 1 on the current walk, 2 known to end without a cycle.
    let mut state = vec![0u8; reaches.len()];
    let mut walk: Vec<usize> = Vec::new();
    for start in 0..reaches.len() {
        let mut here = start;
        loop {
            if state[here] == 2 {
                break;
            }
            if state[here] == 1 {
                return false;
            }
            state[here] = 1;
            walk.push(here);
            match reaches[here].downstream {
                Downstream::Reach(next) => {
                    let next = next as usize;
                    if next >= reaches.len() {
                        return false;
                    }
                    here = next;
                }
                _ => break,
            }
        }
        for &id in &walk {
            state[id] = 2;
        }
        walk.clear();
    }
    true
}
```

The existing acyclicity tests must still pass, including the one that proves a cycle is caught (grep `reaches_are_acyclic` in the tests). If no test feeds it a cycle, add one: two reaches whose `downstream` point at each other must return `false`.

- [ ] **Step 7: Stale words.**
  - `bake_tests.rs`: the doc comment on `effective_thresholds_rise_to_the_graph_resolution` says the params sit above the node floor on this world. They don't; the floor binds there too (4.24e11 against 3.0e10). Reword it to say the floor binds on this world as well, and that the test's `>=` assertions hold either way.
  - Fix the matching README note (grep `README.md` for "node floor" or "never binds").
  - `docs/superpowers/reports/2026-09-10-water-1a-calibration.md`: the post-fix parity figures should read 136,086 / seed 130,366 / tectonic-warp 13,590. Add "(superseded by later pins; see gates.yml)".
  - `.github/workflows/gates.yml`: rename the step whose name contains "all on the belt" to say what it runs.
  - `README.md`: "other four TCTL fields" becomes "other five TCTL fields". Count the fields in `parity.mjs` first, and write the real count.

- [ ] **Step 8: Doc comments for the unchecked assumptions.** Add a one-line comment, or a `debug_assert!` where it is cheap, at each of these:
  - `BucketIndex::nearest`'s unchecked index (`buckets.rs`);
  - the third disjunct in `candidates` (`buckets.rs`);
  - seeds versus `allowed` in `flood` (`flood.rs`);
  - the wetness length equal to the node count (`landgraph.rs::from_parts`, as a `debug_assert_eq!`);
  - the `outlet == NO_NODE` fallback (`hollows.rs::find_hollows`);
  - the `outlet_path.len() < 2` fresh sink (grep `outlet_path.len()` in `flow.rs`);
  - `count_fits`'s minimums (`record.rs`).

  Each comment says what the assumption is and why it holds.

- [ ] **Step 9: Run everything.**
  - `cargo test -p worldbuilder-engine` (all binaries);
  - `cargo test -p worldbuilder-engine --test no_std_math`;
  - `cargo test --release -p worldbuilder-engine --lib every_small_world_drains -- --ignored`;
  - from `viewer/`: `npm run build:wasm`, then `node --test test/hydro.test.mjs test/water-preview.test.mjs`.

  Expected: all pass, with test counts noted. Do not re-pin gates.yml counts here.

- [ ] **Step 10: Commit.** Commit with the subject `Water 1b-2: bake tests in their own file, and the small carry-forward fixes`, including the rebuilt wasm.

---

### Task 2: Notch lines get their ends and split where the graph doesn't join them

**Files:**
- Modify: `src/hydrology/bake.rs` (the notch loop in `record_of`, about lines 365–394)
- Test: `src/hydrology/bake_tests.rs`

**Interfaces:**
- Consumes: `Routing { receiver, lake_of, surface_m, notches }`, `LandGraph::neighbours` (sorted ascending; `from_parts` sorts its pairs), and `closure.outlet_notch`.
- Produces: no type changes. `HydroRecord.notches` now obeys two rules:
  - consecutive points of a line are graph neighbours;
  - a line that contains its route's last cut node ends with one extra point, the node the cut stopped at. That point's third word is the water level there: 0.0 for the ocean, the lake's `level_m` for a lake member, and `routing.surface_m` for a node an earlier cut committed.

Ruling F-3 (from the 1b-1 final review): the layout is unchanged.

- [ ] **Step 1: Read how cuts end.** Read `routing.rs::cut_route`, `grade`, `follow_parents` and `cut_path` (about lines 419–525). Confirm that a `NotchRoute`'s last node has its `receiver` set to the node the cut stopped at (the ocean, a lake member, a committed node, or lower ground). If that is not so, write down what the stop node is and use it in Step 3. Do not change routing.

- [ ] **Step 2: Write the failing tests** in `bake_tests.rs`:

```rust
/// Maps a record point back to its graph node by exact position.
fn node_at(graph: &LandGraph) -> std::collections::BTreeMap<(u64, u64), u32> {
    let mut map = std::collections::BTreeMap::new();
    for (i, p) in graph.positions.iter().enumerate() {
        let (lat, lon) = p.to_latlon();
        map.insert((lat.to_bits(), lon.to_bits()), i as u32); // cast-ok: node index
    }
    map
}

/// Ruling F-3: stage 2 carves a notch line segment by segment, so consecutive points must be
/// graph neighbours, never two nodes a gap apart.
#[test]
fn every_notch_segment_joins_graph_neighbours() {
    let p = params();
    let stages = bake_stages(&world(), &p).expect("stages");
    let record = record_of(&stages, &p);
    let lookup = node_at(&stages.graph);
    let mut segments = 0usize;
    for line in &record.notches {
        for pair in line.points.windows(2) {
            let a = lookup[&(pair[0].0.to_bits(), pair[0].1.to_bits())];
            let b = lookup[&(pair[1].0.to_bits(), pair[1].1.to_bits())];
            assert!(stages.graph.neighbours(a).binary_search(&b).is_ok(),
                    "notch points {a} and {b} are not neighbours");
            segments += 1;
        }
    }
    assert!(segments > 0, "the test world must record at least one notch segment");
}

/// Ruling F-3: an outlet cut ends in the water it drains into, not one node short of it.
#[test]
fn every_outlet_cut_ends_in_the_water_it_drains_into() {
    let p = params();
    let stages = bake_stages(&world(), &p).expect("stages");
    let record = record_of(&stages, &p);
    let mut checked = 0usize;
    for idx in stages.closure.outlet_notch.iter().flatten() {
        let route = &stages.routing.notches[*idx];
        let last = *route.nodes.last().expect("an outlet cut has nodes");
        let stop = stages.routing.receiver[last as usize];
        let i = stop as usize;
        assert!(stages.graph.ocean[i] || stages.routing.lake_of[i] != NO_LAKE,
                "an outlet cut stops at the ocean or a lake");
        let level = if stages.graph.ocean[i] {
            0.0
        } else {
            stages.hollows[stages.routing.lake_of[i] as usize].level_m
        };
        let (lat, lon) = stages.graph.positions[i].to_latlon();
        assert!(record.notches.iter().any(|line| {
            let end = line.points.last().expect("a recorded line has points");
            end.0 == lat && end.1 == lon && end.2 == level
        }), "outlet cut {idx} has no recorded line ending at its stop node");
        checked += 1;
    }
    assert!(checked > 0, "the test world must have at least one outlet cut");
}
```

If the test world has no outlet cut (`checked == 0`), switch that test to `params_for_world()` or another world already used in `bake_tests.rs` that has a fresh enclosed pocket, and say which in the report. Run both tests with `cargo test -p worldbuilder-engine --lib notch`. Expected: FAIL. Paste the failure lines into the report.

- [ ] **Step 3: Implement.** Replace the notch loop in `record_of` with a version that:
  1. builds, for each routing notch, the retained `(node, surface_m, width_m)` list, using exactly the existing rules for outlet cuts and other notches;
  2. appends the stop node when the last retained node is the route's last node;
  3. splits the list wherever consecutive nodes are not neighbours.

```rust
    let is_adjacent = |a: u32, b: u32| graph.neighbours(a).binary_search(&b).is_ok();
    // Ruling F-3: the level of the water a cut stops in -- the datum for the ocean, a lake's own
    // level, or the committed bed of an earlier cut.
    let level_at = |node: u32| -> f64 {
        let i = node as usize;
        if graph.ocean[i] {
            0.0
        } else if routing.lake_of[i] != NO_LAKE {
            hollows[routing.lake_of[i] as usize].level_m
        } else {
            routing.surface_m[i]
        }
    };

    let mut notches = Vec::new();
    for (idx, notch) in routing.notches.iter().enumerate() {
        let mut kept: Vec<(u32, f64, f64)> = Vec::with_capacity(notch.nodes.len() + 1);
        for (&node, &surface_m) in notch.nodes.iter().zip(&notch.bed_m) {
            if is_outlet_notch[idx] {
                kept.push((node, surface_m, width_m(flow[node as usize], params)));
                continue;
            }
            if is_river_node[node as usize] {
                continue;
            }
            let cut_depth_m = graph.height_m[node as usize] - surface_m;
            if cut_depth_m < NOTCH_RECORD_MIN_CUT_M {
                continue;
            }
            let q = flow[node as usize];
            let q_for_width = if q > effective_stream_flow_m2 { q } else { effective_stream_flow_m2 };
            kept.push((node, surface_m, width_m(q_for_width, params)));
        }
        // The node the cut stopped at: appended when the route's own last node was kept, so the
        // line reaches the water (or the earlier cut) it drains into.
        if let (Some(&last), Some(&(kept_last, _, kept_width))) = (notch.nodes.last(), kept.last()) {
            let stop = routing.receiver[last as usize];
            if kept_last == last && stop != NO_NODE {
                kept.push((stop, level_at(stop), kept_width));
            }
        }
        // Split wherever two consecutive kept nodes are not graph neighbours.
        let mut line: Vec<(f64, f64, f64, f64)> = Vec::new();
        let mut previous: Option<u32> = None;
        for &(node, third, width) in &kept {
            if let Some(prev) = previous {
                if !is_adjacent(prev, node) && !line.is_empty() {
                    notches.push(NotchLine { points: std::mem::take(&mut line) });
                }
            }
            let (lat_deg, lon_deg) = graph.positions[node as usize].to_latlon();
            line.push((lat_deg, lon_deg, third, width));
            previous = Some(node);
        }
        if !line.is_empty() {
            notches.push(NotchLine { points: line });
        }
    }
```

Update the big comment above the loop to state Ruling F-3: split at non-neighbours; the appended stop point's third word is the water level there, not a cut surface. Put the same sentence in `record.rs`'s module doc, next to the F-2 bullet.

- [ ] **Step 4: Keep the F-2 test honest.** `an_outlet_cut_agrees_with_the_reach_it_runs_along` compares outlet-notch points with the reach points at the same place. The appended stop point is water, not a cut. Exclude points whose node is an ocean node or a lake member, and keep the test's `compared > 0` assertion. Run it. Expected: PASS.

- [ ] **Step 5: Run the hydrology tests.** Run `cargo test -p worldbuilder-engine --lib hydrology`. Expected: all pass, including Step 2's two tests. `the_record_keeps_only_notches_that_matter` may need its counts re-derived, because lines now split and gain an end. Update it only by re-running and reasoning; report old and new values.

- [ ] **Step 6: Commit.** Rebuild the wasm (`npm run build:wasm` in `viewer/`) and commit with the subject `Water 1b-2: notch lines reach the water they drain into, and split at gaps`.

---

### Task 3: SCHEMA 4 — capped-basin counts and the refinement params

**Files:**
- Modify: `src/hydrology/mod.rs`, `src/hydrology/hollows.rs`, `src/hydrology/routing.rs`, `src/hydrology/bake.rs`, `src/hydrology/record.rs`, `viewer/public/app/engine.js`, `viewer/public/app/water-preview.js`
- Test: `src/hydrology/bake_tests.rs`, `src/hydrology/record.rs` tests, `viewer/test/hydro.test.mjs`, `viewer/test/water-preview.test.mjs`

**Interfaces:**
- Produces: `HydroParams` gains these fields, set in `earth_like` to the values shown:

```rust
    /// Spec §6.6: the fine tracer's station spacing along a coarse segment.
    pub refine_step_m: f64,             // 1_500.0
    /// Ruling R-7: Douglas–Peucker horizontal tolerance for refined reaches.
    pub refine_simplify_m: f64,         // 250.0
    /// Ruling R-7: the vertical tolerance, on the bed.
    pub refine_vertical_m: f64,         // 1.0
    /// Spec §6.7: a fall drops at least this much ...
    pub fall_min_drop_m: f64,           // 10.0
    /// ... over at most this much of its length.
    pub fall_max_run_m: f64,            // 150.0
    /// Ruling R-6: meander wavelength, in channel widths.
    pub meander_wavelength_widths: f64, // 11.0
    /// Ruling R-6: meander amplitude, in channel widths.
    pub meander_amplitude_widths: f64,  // 1.5
    /// Ruling R-6: a segment meanders only if its bed falls less steeply than this.
    pub meander_max_slope: f64,         // 0.002
```

- Produces: `Hollow.inner_of_capped: bool`. It is `false` from `find_hollows`, and `route` sets it to `true` on every inner hollow its capped step appends.
- Produces: `BakeStats` gains the following fields, echoed from params in `record_of` exactly as the existing echo fields are:
  - `capped_basins: u32`: hollows with `capped == true`;
  - `capped_inner: u32`: hollows with `inner_of_capped`;
  - `capped_inner_kept: u32`: those with `fate == Keep`;
  - `refine_step_m`, `refine_simplify_m`, `refine_vertical_m`, `fall_min_drop_m`, `fall_max_run_m`, `meander_wavelength_widths`, `meander_amplitude_widths` and `meander_max_slope`, all `f64`.
- Produces: record header words 32–42, in this order: `capped_basins`, `capped_inner`, `capped_inner_kept`, `refine_step_m`, `refine_simplify_m`, `refine_vertical_m`, `fall_min_drop_m`, `fall_max_run_m`, `meander_wavelength_widths`, `meander_amplitude_widths`, `meander_max_slope`. The header is 43 words and `SCHEMA` is `4.0`. Body, reach, notch and fall layouts are unchanged.

- [ ] **Step 1: Write the failing tests** in `bake_tests.rs`:

```rust
/// Runs the coarse pipeline on a line graph (area 1e6 and wetness 0.5 per node), the way
/// `bake_stages` does, for stats that need a hand-built world.
fn line_stages(heights: &[f64], params: &HydroParams) -> BakeStages {
    let n = heights.len();
    let positions = (0..n)
        .map(|i| crate::sphere::SpherePoint::from_latlon(0.0, i as f64 * 0.01))
        .collect();
    let directed: Vec<Vec<u32>> = (0..n)
        .map(|i| if i + 1 < n { vec![(i + 1) as u32] } else { Vec::new() }) // cast-ok: node index
        .collect();
    let graph = LandGraph::from_parts(6_371_000.0, positions, heights.to_vec(), vec![1.0e6; n],
                                      &directed, vec![0.5; n]);
    let global = flood(&graph, &ocean_seeds(&graph), &|_| true);
    let mut hollows = find_hollows(&graph, &global);
    let forced = hollows::forced_nodes(&graph, params);
    judge(&mut hollows, &forced, params);
    let mut routing = route(&graph, &global, &mut hollows, params);
    let (flow, closure) = close_lakes(&graph, &mut routing, &hollows, params);
    drainage_check(&graph, &routing).expect("the fixture drains");
    BakeStages { graph, hollows, routing, flow, closure }
}

/// Carry-forward I3: the record says how many capped basins there were and what they kept, so
/// the owner-world bake can show whether capped basins keep their inner lakes at 1M nodes.
#[test]
fn the_record_counts_what_capped_basins_keep() {
    let mut p = HydroParams::earth_like(1_000);
    p.keep_max_area_m2 = 7.0e6;
    // Task 4 fixture: {3} sits on the basin's way out and is notched; {7} is off it and kept.
    let stages = line_stages(&[-50.0, 40.0, 30.0, 10.0, 25.0, 5.0, 25.0, 12.0, 28.0, 35.0, 70.0], &p);
    let record = record_of(&stages, &p);
    assert_eq!(record.stats.capped_basins, 1);
    assert!(record.stats.capped_inner >= 2, "both inner hollows are counted");
    assert_eq!(record.stats.capped_inner_kept, 1);
}

#[test]
fn the_record_echoes_the_refinement_params() {
    let p = params();
    let record = crate::hydrology::bake(&world(), &p).expect("bake");
    assert_eq!(record.stats.refine_step_m, p.refine_step_m);
    assert_eq!(record.stats.meander_max_slope, p.meander_max_slope);
    let words = crate::hydrology::record::encode(&record);
    assert_eq!(words[0], 4.0);
    assert_eq!(words[32], f64::from(record.stats.capped_basins));
    assert_eq!(words[42], p.meander_max_slope);
}
```

If the fixture's inner hollows number differently from `>= 2` and `== 1`, read `a_capped_basin_keeps_the_lake_off_its_way_out` in `routing.rs`'s tests (it is the same fixture) and assert the numbers that test implies. Run with `cargo test -p worldbuilder-engine --lib the_record_`. Expected: compile errors (fields missing), which count as FAIL.

- [ ] **Step 2: Add the fields.**
  - Add the eight `HydroParams` fields with their `earth_like` values.
  - Add the `BakeStats` fields.
  - Add `inner_of_capped: false` to the `Hollow` literal in `find_hollows`.
  - In `routing.rs`'s capped step, before `hollows.extend(inner);`, add `for hollow in inner.iter_mut() { hollow.inner_of_capped = true; }`.
  - In `bake_stages`, add `require_finite_positive` for each of the eight new params.
  - In `record_of`, count and echo:

```rust
    let capped_basins = hollows.iter().filter(|h| h.capped).count();
    let capped_inner = hollows.iter().filter(|h| h.inner_of_capped).count();
    let capped_inner_kept = hollows.iter().filter(|h| h.inner_of_capped && h.fate == Fate::Keep).count();
```

  Put them in the `BakeStats` literal with `as u32 // cast-ok: bounded by hollow count`, followed by the eight echoed params. Fix every other `BakeStats { .. }` or `HydroParams { .. }` literal the compiler finds (tests, `record.rs` fixtures, `hydro_survey.rs`).

- [ ] **Step 3: The wire.** In `record.rs`:
  - set `SCHEMA` to `4.0`, and rewrite its doc comment to say what SCHEMA 4 added (the eleven header words 32–42, named);
  - in `encode`, push the eleven words after `forced_matched`, in the order above;
  - in `decode`, read them back in the same order (the three counts with `r.u32()?`, the eight params with `r.word()?`).

  Run the record tests with `cargo test -p worldbuilder-engine --lib record`. Any test that builds a header by hand, or asserts a length of 32, must move to 43. Expected: PASS.

- [ ] **Step 4: The viewer twins.**
  - In `viewer/public/app/engine.js::hydroSummary`, change the schema check from 3 to 4 and the header length from 32 to 43. Expose `cappedBasins`, `cappedInner` and `cappedInnerKept`.
  - In `viewer/public/app/water-preview.js::decodeHydro`, make the same schema and header changes, and add `header.cappedBasins`, `header.cappedInner`, `header.cappedInnerKept`, `header.refineStepM`, `header.refineSimplifyM`, `header.refineVerticalM`, `header.fallMinDropM`, `header.fallMaxRunM`, `header.meanderWavelengthWidths`, `header.meanderAmplitudeWidths` and `header.meanderMaxSlope`.
  - Update the layout comments in both files.
  - In `viewer/test/water-preview.test.mjs` and `hydro.test.mjs`, the schema-refusal tests now refuse 3 and accept 4. Add one assertion that `decodeHydro(words).header.cappedBasins === words[32]`.

- [ ] **Step 5: Rebuild and run.** From `viewer/`, run `npm run build:wasm`, then `node --test test/hydro.test.mjs test/water-preview.test.mjs`. Then run `cargo test -p worldbuilder-engine` and `--test no_std_math`. Expected: all pass. `parity.mjs` is header-agnostic, so check that it still runs (`node test/parity.mjs`), but do not re-pin it.

- [ ] **Step 6: Commit.** Include the wasm, with the subject `Water 1b-2: SCHEMA 4 counts what capped basins keep and echoes the refinement params`.

---

### Task 4: Trace each reach on the landform

**Files:**
- Create: `src/hydrology/refine.rs`
- Modify: `src/hydrology/mod.rs` (`pub mod refine;`, and `bake()`), and the spec (§6.6)
- Test: the `#[cfg(test)] mod tests` block in `refine.rs`, and `bake_tests.rs`

**Interfaces:**
- Consumes: `HydroParams.refine_step_m` (Task 3), `ReachLine`, `ReachPoint`, `Body`, `Fall`, `Downstream`, `TangentFrame::{at, local_to_sphere, sphere_to_local}`, `SpherePoint::{from_latlon, to_latlon}`, and `crate::stream::nominal_spacing_m(count: u32, radius_m: f64) -> f64`.
- Produces, for Tasks 5 to 7:
  - `pub struct Ground<'a> { pub height_m: &'a dyn Fn(&SpherePoint) -> f64, pub radius_m: f64, pub corridor_m: f64, pub seed: u64 }`
  - `pub struct Fine { pub along_m: f64, pub lateral_m: f64, pub point: SpherePoint, pub bed_m: f64, pub keep: bool }`
  - `pub struct Segment { pub interior: Vec<Fine>, pub mouth: Option<Fine>, pub falls: Vec<(SpherePoint, f64)> }`
  - `pub struct Refined { pub points: Vec<ReachPoint>, pub protected: Vec<bool>, pub falls: Vec<Fall> }`
  - `pub fn terminal_level(reach: &ReachLine, bodies: &[Body]) -> Option<f64>`
  - `pub fn trace_segment(ground: &Ground, params: &HydroParams, a: &ReachPoint, b: &ReachPoint, shore: Option<f64>) -> Segment`
  - `pub fn refine_reach(reach: &ReachLine, shore: Option<f64>, ground: &Ground, params: &HydroParams) -> Refined`
  - `pub fn refine(record: &mut HydroRecord, ground: &Ground, params: &HydroParams)`
  - `pub fn beds_never_rise(reach: &ReachLine) -> bool`

- [ ] **Step 1: Write `refine.rs` with its tests first.** Create the file with the module doc, the types and function signatures above (bodies `todo!()` for now), and this test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::hydrology::reaches::ReachClass;

    const R: f64 = 6_371_000.0;
    /// Metres per degree on this test radius.
    const M_PER_DEG: f64 = R * std::f64::consts::PI / 180.0;

    fn point(lat_deg: f64, lon_deg: f64, bed_m: f64) -> ReachPoint {
        ReachPoint { lat_deg, lon_deg, bed_m, width_m: 10.0, depth_m: 1.0, flow_m2: 1.0e9 }
    }

    /// A 30 km segment due east along the equator: a at 0 E, b at 30 km east.
    fn ends(bed_a: f64, bed_b: f64) -> (ReachPoint, ReachPoint) {
        (point(0.0, 0.0, bed_a), point(0.0, 30_000.0 / M_PER_DEG, bed_b))
    }

    /// North of the equator in metres, and east of 0 E in metres (small-angle, test only).
    fn north_east(p: &SpherePoint) -> (f64, f64) {
        let (lat, lon) = p.to_latlon();
        (lat * M_PER_DEG, lon * M_PER_DEG)
    }

    fn ground<'a>(height: &'a dyn Fn(&SpherePoint) -> f64) -> Ground<'a> {
        Ground { height_m: height, radius_m: R, corridor_m: 20_000.0, seed: 7 }
    }

    fn params() -> HydroParams {
        HydroParams::earth_like(1_000)
    }

    #[test]
    fn a_trace_settles_into_the_valley_floor() {
        // A straight valley 5 km north of the chord, falling gently east.
        let h = |p: &SpherePoint| {
            let (n, e) = north_east(p);
            let off = if n > 5_000.0 { n - 5_000.0 } else { 5_000.0 - n };
            100.0 - 0.001 * e + 0.02 * off
        };
        let (a, b) = ends(99.0, 69.0);
        let seg = trace_segment(&ground(&h), &params(), &a, &b, None);
        let n = seg.interior.len();
        assert!(n == 19 || n == 20, "30 km at 1.5 km: 20 steps (21 if the chord rounds just over), got {n} interior stations");
        let middle: Vec<&Fine> = seg.interior.iter()
            .filter(|f| f.along_m >= 8_000.0 && f.along_m <= 20_000.0).collect();
        assert!(!middle.is_empty());
        for f in middle {
            let off = f.lateral_m - 5_000.0;
            assert!(off <= 750.0 && off >= -750.0, "station at {} m is {} m off the valley", f.along_m, off);
        }
    }

    #[test]
    fn the_bed_never_rises_and_never_drops_below_the_segment_end() {
        let h = |p: &SpherePoint| {
            let (_, e) = north_east(p);
            50.0 - 0.001 * e + 30.0 * crate::detmath::sin(e / 2_000.0)
        };
        let (a, b) = ends(49.0, 19.0);
        let seg = trace_segment(&ground(&h), &params(), &a, &b, None);
        let mut prev = a.bed_m;
        for f in &seg.interior {
            assert!(f.bed_m <= prev, "bed rose from {prev} to {}", f.bed_m);
            assert!(f.bed_m >= b.bed_m, "bed {} fell below the segment end {}", f.bed_m, b.bed_m);
            prev = f.bed_m;
        }
    }

    #[test]
    fn a_trace_stays_in_its_corridor_and_returns_to_the_next_point() {
        // Ground falling to the north without end: the tracer goes as far as it may, and comes back.
        let h = |p: &SpherePoint| { let (n, _) = north_east(p); 100.0 - 0.01 * n };
        let (a, b) = ends(99.0, 90.0);
        let g = ground(&h);
        let seg = trace_segment(&g, &params(), &a, &b, None);
        let spacing = 30_000.0 / 20.0;
        for f in &seg.interior {
            assert!(f.lateral_m <= g.corridor_m && f.lateral_m >= -g.corridor_m);
            let remaining = 30_000.0 - f.along_m;
            assert!(f.lateral_m <= remaining + 1e-6 && f.lateral_m >= -remaining - 1e-6,
                    "station at {} m cannot get back to the chord", f.along_m);
            let _ = spacing;
        }
        let last = seg.interior.last().expect("stations");
        assert!(last.lateral_m <= spacing + 1e-6 && last.lateral_m >= -spacing - 1e-6);
    }

    #[test]
    fn an_inland_segment_never_steps_into_the_sea() {
        // Sea south of 2 km south; the land just north of it is the lowest land.
        let h = |p: &SpherePoint| { let (n, _) = north_east(p); if n < -2_000.0 { -10.0 } else { 10.0 + 0.001 * n } };
        let (a, b) = ends(9.0, 8.0);
        let seg = trace_segment(&ground(&h), &params(), &a, &b, None);
        for f in &seg.interior {
            assert!(h(&f.point) > 0.0, "station at {} m stepped into the sea", f.along_m);
        }
    }

    #[test]
    fn a_river_ends_at_the_shore() {
        // Land falling east to the sea at 20 km: the last segment stops there, not at its coarse end.
        let h = |p: &SpherePoint| { let (_, e) = north_east(p); 100.0 - 0.005 * e };
        let (a, b) = ends(99.0, 0.0);
        let seg = trace_segment(&ground(&h), &params(), &a, &b, Some(0.0));
        let mouth = seg.mouth.expect("the trace reaches the shore before b");
        assert!(h(&mouth.point) <= 0.0);
        assert!(mouth.along_m >= 19_500.0 && mouth.along_m <= 21_600.0, "mouth at {} m", mouth.along_m);
        for f in &seg.interior {
            assert!(h(&f.point) > 0.0 && f.along_m < mouth.along_m);
        }
        assert!(mouth.bed_m <= 0.0);
    }

    #[test]
    fn coarse_points_are_kept_exactly_and_the_mouth_bed_never_rises() {
        let h = |p: &SpherePoint| { let (_, e) = north_east(p); 100.0 - 0.001 * e };
        let pts = vec![point(0.0, 0.0, 99.0), point(0.0, 30_000.0 / M_PER_DEG, 69.0),
                       point(0.0, 60_000.0 / M_PER_DEG, 5.0)];
        let reach = ReachLine { id: 0, class: ReachClass::Stream, order: 1,
                                downstream: Downstream::Ocean, fresh: true, points: pts.clone() };
        let refined = refine_reach(&reach, Some(20.0), &ground(&h), &params());
        assert_eq!(refined.points.len(), refined.protected.len());
        assert_eq!(refined.points[0], pts[0]);
        assert!(refined.points.iter().any(|p| p == &pts[1]), "the middle coarse point is kept");
        let line = ReachLine { points: refined.points.clone(), ..reach.clone() };
        assert!(beds_never_rise(&line));
    }

    #[test]
    fn beds_never_rise_catches_a_rising_bed() {
        let reach = ReachLine { id: 0, class: ReachClass::Stream, order: 1, downstream: Downstream::Ocean,
                                fresh: true, points: vec![point(0.0, 0.0, 10.0), point(0.0, 0.1, 11.0)] };
        assert!(!beds_never_rise(&reach));
    }

    #[test]
    fn terminal_levels() {
        let reach = |d| ReachLine { id: 0, class: ReachClass::Stream, order: 1, downstream: d,
                                    fresh: true, points: Vec::new() };
        assert_eq!(terminal_level(&reach(Downstream::Ocean), &[]), Some(0.0));
        assert_eq!(terminal_level(&reach(Downstream::Reach(3)), &[]), None);
        assert_eq!(terminal_level(&reach(Downstream::Sink), &[]), None);
    }
}
```

Run `cargo test -p worldbuilder-engine --lib hydrology::refine`. Expected: FAIL (`todo!()` panics).

- [ ] **Step 2: Implement.** Replace the `todo!()` bodies with the code below. `refine.rs` in full:

```rust
//! Refinement (spec §6.6): every coarse reach re-traced on the landform at fine steps.
//!
//! A coarse segment runs from one reach point to the next, about one graph spacing long. Each
//! is walked in `refine_step_m` stations along its chord. At each station the tracer looks a
//! little to either side and takes the lowest ground, so the line settles into the valley floor
//! the coarse graph only saw every few tens of kilometres. It never leaves the corridor (one
//! graph spacing either side of the chord). It never steps onto ground at or below the datum
//! before its mouth (Ruling R-3). It always arrives back on the next coarse point. Coarse points
//! are kept exactly, so a tributary still ends on its receiver's first vertex (Ruling R-1, spec
//! §14.4).
//!
//! The bed never rises (spec §14.5). Inside a segment it follows the ground down, less the
//! channel's depth, but never below the segment's lower end. Where the ground rises, the bed
//! holds, which is a cut. A fine dip met on the way is not judged as a new lake: the bed stays
//! level across it (Ruling R-2), and plan 1b-3's pond search owns fine lakes. The last segment of
//! a reach into the sea or a lake ends at the first station on the shore, and the mouth's bed is
//! the lower of the bed so far and the water level (Rulings R-3, R-4).

use crate::detmath as m;
use crate::hydrology::{Body, Downstream, Fall, HydroParams, HydroRecord, ReachLine, ReachPoint};
use crate::sphere::SpherePoint;
use crate::tangent::TangentFrame;

/// A tracer never plans more stations (or fall windows) than this on one segment, whatever the
/// params ask.
const MAX_STATIONS: f64 = 100_000.0;

/// Lateral candidates at each station, as fractions of the station spacing, in tie-break order:
/// straight on first, then the nearer sides, left before right.
const CANDIDATES: [f64; 5] = [0.0, -0.5, 0.5, -1.0, 1.0];

/// The ground and the geometry a trace needs, apart from the reach itself. A closure rather than
/// a `Surface`, so the tests can trace over ground written by hand.
pub struct Ground<'a> {
    pub height_m: &'a dyn Fn(&SpherePoint) -> f64,
    pub radius_m: f64,
    /// One graph spacing: how far either side of a coarse chord the line may wander.
    pub corridor_m: f64,
    /// The world seed, for the meander's phase.
    pub seed: u64,
}

/// One traced point inside a coarse segment, in the segment's own frame (metres along the chord
/// from its start, and to its left).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fine {
    pub along_m: f64,
    pub lateral_m: f64,
    pub point: SpherePoint,
    pub bed_m: f64,
    /// Survives simplification and is never meandered (a fall's two ends).
    pub keep: bool,
}

/// What one segment traced to: its interior points; the mouth that replaces the coarse end, on a
/// last segment that reached the shore first; and any falls, as (upper end, height).
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub interior: Vec<Fine>,
    pub mouth: Option<Fine>,
    pub falls: Vec<(SpherePoint, f64)>,
}

/// One refined reach: its points, which of them simplification must keep, and its falls.
#[derive(Debug, Clone, PartialEq)]
pub struct Refined {
    pub points: Vec<ReachPoint>,
    pub protected: Vec<bool>,
    pub falls: Vec<Fall>,
}

/// The water level a reach runs into at its end: the datum for the ocean, a lake's own level.
/// `None` for a reach that ends on another reach or nowhere.
pub fn terminal_level(reach: &ReachLine, bodies: &[Body]) -> Option<f64> {
    match reach.downstream {
        Downstream::Ocean => Some(0.0),
        Downstream::Body(id) => bodies.get(id as usize).map(|b| b.level_m),
        Downstream::Reach(_) | Downstream::Sink => None,
    }
}

/// Spec §14.5 on one reach: the bed never rises from one point to the next.
pub fn beds_never_rise(reach: &ReachLine) -> bool {
    reach.points.windows(2).all(|w| w[1].bed_m <= w[0].bed_m)
}

/// Traces the coarse segment `a -> b`. `shore` is the level of the water the reach runs into,
/// given only for its last segment.
pub fn trace_segment(ground: &Ground, params: &HydroParams, a: &ReachPoint, b: &ReachPoint, shore: Option<f64>) -> Segment {
    let mut segment = Segment { interior: Vec::new(), mouth: None, falls: Vec::new() };
    let start = SpherePoint::from_latlon(a.lat_deg, a.lon_deg);
    let end = SpherePoint::from_latlon(b.lat_deg, b.lon_deg);
    let frame = TangentFrame::at(&start, ground.radius_m);
    let (bx, by) = frame.sphere_to_local(&end);
    let len = m::hypot(bx, by);
    if !(len > params.refine_step_m) {
        return segment;
    }
    let wanted = -m::floor(-(len / params.refine_step_m));
    let stations = if wanted > MAX_STATIONS { MAX_STATIONS } else { wanted };
    let k = stations as usize; // cast-ok: a whole number in 2..=MAX_STATIONS
    let spacing = len / stations;
    let (ux, uy) = (bx / len, by / len);
    let (vx, vy) = (-uy, ux);
    let at = |along: f64, lateral: f64| frame.local_to_sphere(ux * along + vx * lateral, uy * along + vy * lateral);

    // The bed may fall to the segment's lower end and no further.
    let floor_m = if b.bed_m < a.bed_m { b.bed_m } else { a.bed_m };
    let mut lateral = 0.0;
    let mut bed = a.bed_m;
    for i in 1..k {
        let along = spacing * i as f64;
        let remaining = spacing * (k - i) as f64;
        let limit = if remaining < ground.corridor_m { remaining } else { ground.corridor_m };
        let mut best: Option<(f64, f64, SpherePoint)> = None;
        for &j in CANDIDATES.iter() {
            let o = lateral + j * spacing;
            if o > limit || o < -limit {
                continue;
            }
            let p = at(along, o);
            let g = (ground.height_m)(&p);
            if !g.is_finite() {
                continue;
            }
            if shore.is_none() && g <= 0.0 {
                continue;
            }
            let better = match best {
                None => true,
                Some((best_g, _, _)) => g < best_g,
            };
            if better {
                best = Some((g, o, p));
            }
        }
        let (g, o, p) = match best {
            Some(found) => found,
            None => {
                // Nothing allowed: hold the line as close to where it was as the limit lets it be.
                let o = if lateral > limit { limit } else if lateral < -limit { -limit } else { lateral };
                let p = at(along, o);
                let g = (ground.height_m)(&p);
                (if g.is_finite() { g } else { bed + a.depth_m }, o, p)
            }
        };
        lateral = o;
        if let Some(level) = shore {
            if g <= level {
                let mouth_bed = if bed < level { bed } else { level };
                segment.mouth = Some(Fine { along_m: along, lateral_m: o, point: p, bed_m: mouth_bed, keep: false });
                return segment;
            }
        }
        let want = g - a.depth_m;
        if want < bed {
            bed = want;
        }
        if bed < floor_m {
            bed = floor_m;
        }
        segment.interior.push(Fine { along_m: along, lateral_m: o, point: p, bed_m: bed, keep: false });
    }
    segment
}

fn fine_point(fine: &Fine, like: &ReachPoint) -> ReachPoint {
    let (lat_deg, lon_deg) = fine.point.to_latlon();
    ReachPoint { lat_deg, lon_deg, bed_m: fine.bed_m, width_m: like.width_m, depth_m: like.depth_m, flow_m2: like.flow_m2 }
}

/// Refines one reach: coarse points kept exactly, fine points between them, trimmed at the shore
/// on its last segment. Interior points carry their segment's upstream width, depth and flow.
/// Flow only steps at a coarse point, where a tributary joins.
pub fn refine_reach(reach: &ReachLine, shore: Option<f64>, ground: &Ground, params: &HydroParams) -> Refined {
    let coarse = &reach.points;
    let mut refined = Refined { points: Vec::new(), protected: Vec::new(), falls: Vec::new() };
    if coarse.is_empty() {
        return refined;
    }
    refined.points.push(coarse[0].clone());
    refined.protected.push(true);
    for s in 0..coarse.len() - 1 {
        let a = &coarse[s];
        let b = &coarse[s + 1];
        let last = s + 2 == coarse.len();
        let here_shore = if last { shore } else { None };
        let segment = trace_segment(ground, params, a, b, here_shore);
        for fine in &segment.interior {
            refined.points.push(fine_point(fine, a));
            refined.protected.push(fine.keep);
        }
        for &(at, height_m) in &segment.falls {
            refined.falls.push(Fall { reach: reach.id, at: at.to_latlon(), height_m });
        }
        if let Some(mouth) = segment.mouth {
            refined.points.push(fine_point(&mouth, b));
            refined.protected.push(true);
            break;
        }
        let mut end = b.clone();
        if last && here_shore.is_some() {
            // Ruling R-4: a mouth's bed never rises above the bed that reaches it.
            let before = refined.points.last().expect("at least the first point").bed_m;
            if before < end.bed_m {
                end.bed_m = before;
            }
        }
        refined.points.push(end);
        refined.protected.push(true);
    }
    refined
}

/// Refines every reach in the record, in reach order, and records the falls in the same order.
pub fn refine(record: &mut HydroRecord, ground: &Ground, params: &HydroParams) {
    let shores: Vec<Option<f64>> = record.reaches.iter().map(|r| terminal_level(r, &record.bodies)).collect();
    let mut falls = Vec::new();
    for (reach, shore) in record.reaches.iter_mut().zip(shores) {
        let refined = refine_reach(reach, shore, ground, params);
        reach.points = refined.points;
        falls.extend(refined.falls);
    }
    record.falls = falls;
}
```

The `for i in 1..k` loop casts `i as f64` and `(k - i) as f64`. These are `usize → f64` casts, not in the `cast-ok` list, but add `// cast-ok: i < k <= MAX_STATIONS` anyway, to match house style.

Run `cargo test -p worldbuilder-engine --lib hydrology::refine`. Expected: PASS. If `a_trace_settles_into_the_valley_floor` fails because the greedy walk settles more slowly than 8 km, print the laterals and report them. Do not loosen the test without a written reason.

- [ ] **Step 3: Wire refinement into `bake()`.** In `src/hydrology/mod.rs`, add `pub mod refine;` and change `bake`:

```rust
/// The bake, end to end: `bake_stages`, then `record_of`, then `refine::refine` (spec §6.6).
pub fn bake(surface: &Surface, params: &HydroParams) -> Result<HydroRecord, HydroError> {
    let stages = bake_stages(surface, params)?;
    let mut record = record_of(&stages, params);
    let height = |p: &SpherePoint| surface.structural_m(p);
    let ground = refine::Ground {
        height_m: &height,
        radius_m: surface.radius_m,
        corridor_m: crate::stream::nominal_spacing_m(params.total_nodes, surface.radius_m),
        seed: surface.world_seed as u64, // cast-ok: two's-complement reinterpretation, as Surface::new makes
    };
    refine::refine(&mut record, &ground, params);
    Ok(record)
}
```

- [ ] **Step 4: Sort out the existing bake tests.** Run `cargo test -p worldbuilder-engine --lib hydrology`. Any `bake_tests.rs` test that now fails because it asserts a graph-node relationship, rather than a property, should build its record with `record_of(&bake_stages(&world(), &p).expect("stages"), &p)` instead of `bake()`. Such tests match reach points to graph nodes by position, compare a reach's last point to a lake node, or use point counts.
  - Tests of properties stay on `bake()`: determinism, round trip, drainage, downstream links, freshness, forced counts, and keep rules.
  - List every test you moved in the report, with one line on why.

- [ ] **Step 5: Property tests on the refined record.** Add these to `bake_tests.rs`:

```rust
/// Spec §14.5 on a real bake: after refinement, no reach's bed rises anywhere, mouths included.
#[test]
fn refined_beds_never_rise() {
    let record = crate::hydrology::bake(&world(), &params()).expect("bake");
    for reach in &record.reaches {
        assert!(crate::hydrology::refine::beds_never_rise(reach), "reach {} has a rising bed", reach.id);
    }
}

/// Spec §14.4: every tributary's last point is its receiver's first point, bit for bit.
#[test]
fn refined_tributaries_share_their_junction_vertex() {
    let record = crate::hydrology::bake(&world(), &params()).expect("bake");
    let mut junctions = 0usize;
    for reach in &record.reaches {
        if let Downstream::Reach(next) = reach.downstream {
            let last = reach.points.last().expect("points");
            let first = &record.reaches[next as usize].points[0];
            assert_eq!((last.lat_deg.to_bits(), last.lon_deg.to_bits()),
                       (first.lat_deg.to_bits(), first.lon_deg.to_bits()));
            junctions += 1;
        }
    }
    assert!(junctions > 0);
}

/// Ruling R-1: refinement adds points and never moves a coarse one (except the water node a
/// trimmed mouth replaces).
#[test]
fn refinement_keeps_every_coarse_point_in_order() {
    let p = params();
    let stages = bake_stages(&world(), &p).expect("stages");
    let coarse = record_of(&stages, &p);
    let refined = crate::hydrology::bake(&world(), &p).expect("bake");
    let mut added = 0usize;
    for (c, r) in coarse.reaches.iter().zip(&refined.reaches) {
        let mut at = 0usize;
        let keep = if matches!(c.downstream, Downstream::Ocean | Downstream::Body(_)) { c.points.len() - 1 } else { c.points.len() };
        for cp in &c.points[..keep] {
            while at < r.points.len()
                && (r.points[at].lat_deg.to_bits(), r.points[at].lon_deg.to_bits()) != (cp.lat_deg.to_bits(), cp.lon_deg.to_bits()) {
                at += 1;
            }
            assert!(at < r.points.len(), "reach {} lost a coarse point", c.id);
        }
        added += r.points.len().saturating_sub(c.points.len());
    }
    assert!(added > 0, "refinement added fine points");
}
```

Run them. Expected: PASS. If `refined_beds_never_rise` fails at a coarse junction (not a mouth), stop and report the reach and point. That would mean coarse beds rise somewhere, which Ruling R-1 assumed they don't.

- [ ] **Step 6: Mutation guard.** In `trace_segment`, temporarily delete the `if want < bed { bed = want; }` guard (so `bed = want` always) and run `refined_beds_never_rise` and `the_bed_never_rises_and_never_drops_below_the_segment_end`. At least one must FAIL. Restore the guard, and paste both outputs into the report.

- [ ] **Step 7: Spec.** In spec §6.6's **Tracing** bullet, add the rulings as sentences, keeping it in the spec's plain style:
  - coarse points are kept (R-1);
  - fine dips are not judged (R-2);
  - no step below the datum inland, and the shore trim (R-3);
  - the mouth bed (R-4).

  Add to §6.6 the plan-level scope note: outlines and ponds are plan 1b-3, and why.

- [ ] **Step 8: Run everything and commit.**
  - Run `cargo test -p worldbuilder-engine`, `--test no_std_math`, and the ignored drain sweep.
  - Time `cargo test --release -p worldbuilder-engine --lib a_bake_is_bit_identical_run_to_run` before and after this task, and report both.
  - Rebuild the wasm.
  - Commit with the subject `Water 1b-2: reaches re-traced on the landform at 1.5 km`.

---

### Task 5: Waterfalls

**Files:**
- Modify: `src/hydrology/refine.rs`, and the spec (§6.7)
- Test: `refine.rs` tests, and `bake_tests.rs`

**Interfaces:**
- Consumes: `trace_segment`, `Fine`, `Segment.falls`, `HydroParams.fall_min_drop_m` and `HydroParams.fall_max_run_m` (Tasks 3, 4).
- Produces: `Segment.falls` filled in; the fall ends are inserted into `Segment.interior` with `keep: true`, and `HydroRecord.falls` is filled in by `refine`. Ruling R-5: `Fall.at` is the upper end, and the lower end is the next reach point.

- [ ] **Step 1: Write the failing tests** in `refine.rs`'s test module:

```rust
    /// A cliff 50 m high and 100 m wide at 15 km along an otherwise gentle segment.
    fn cliff(p: &SpherePoint) -> f64 {
        let (_, e) = north_east(p);
        let base = 200.0 - 0.001 * e;
        if e < 15_000.0 { base } else if e > 15_100.0 { base - 50.0 } else { base - 50.0 * (e - 15_000.0) / 100.0 }
    }

    #[test]
    fn a_cliff_on_the_line_is_a_waterfall() {
        let (a, b) = ends(199.0, 119.0);
        let seg = trace_segment(&ground(&cliff), &params(), &a, &b, None);
        assert_eq!(seg.falls.len(), 1, "one fall");
        let (at, height) = seg.falls[0];
        assert!(height >= 10.0 && height <= 51.0, "height {height} (the cliff plus at most 150 m of the base slope)");
        let (_, e) = north_east(&at);
        assert!(e >= 14_800.0 && e <= 15_100.0, "fall's upper end at {e} m east");
        let kept: Vec<&Fine> = seg.interior.iter().filter(|f| f.keep).collect();
        assert_eq!(kept.len(), 2, "both ends are inserted and protected");
        let d = kept[0].bed_m - kept[1].bed_m - height;
        assert!(d < 1e-9 && d > -1e-9, "the bed drops by the fall's height between them");
        let mut prev = a.bed_m;
        for f in &seg.interior {
            assert!(f.bed_m <= prev);
            prev = f.bed_m;
        }
    }

    #[test]
    fn a_steep_but_even_slope_has_no_waterfall() {
        // 60 m over 30 km: steep, but never 10 m in 150 m.
        let h = |p: &SpherePoint| { let (_, e) = north_east(p); 200.0 - 0.002 * e };
        let (a, b) = ends(199.0, 139.0);
        assert!(trace_segment(&ground(&h), &params(), &a, &b, None).falls.is_empty());
    }

    #[test]
    fn a_step_just_under_ten_metres_is_not_a_waterfall() {
        let h = |p: &SpherePoint| {
            let (_, e) = north_east(p);
            let base = 200.0 - 0.0001 * e;
            if e < 15_000.0 { base } else if e > 15_100.0 { base - 9.0 } else { base - 9.0 * (e - 15_000.0) / 100.0 }
        };
        let (a, b) = ends(199.0, 185.0);
        assert!(trace_segment(&ground(&h), &params(), &a, &b, None).falls.is_empty());
    }
```

Tests follow the house rule too: no `.abs()`, `min`, `max` or `clamp`, even under `#[cfg(test)]`. Run `cargo test -p worldbuilder-engine --lib hydrology::refine::tests::a_`. Expected: the cliff test FAILs (no falls).

- [ ] **Step 2: Implement.** Add to `refine.rs`:

```rust
/// Spec §6.7 on one step `from -> to` of a trace: a fall is where the bed drops at least
/// `fall_min_drop_m` across the step, and the ground drops at least that much inside one window
/// of at most `fall_max_run_m`. Returns the fall's two ends and its height (Ruling R-5). The upper
/// end is at `from`'s bed and the lower that much below; the height is the smaller of the bed's
/// drop and the window's, so the bed after the lower end still never rises.
fn find_fall(ground: &Ground, params: &HydroParams, at: &dyn Fn(f64, f64) -> SpherePoint, from: &Fine, to: &Fine) -> Option<(Fine, Fine, f64)> {
    let bed_drop = from.bed_m - to.bed_m;
    if !(bed_drop >= params.fall_min_drop_m) {
        return None;
    }
    let dx = to.along_m - from.along_m;
    let dy = to.lateral_m - from.lateral_m;
    let run = m::hypot(dx, dy);
    let wanted = -m::floor(-(run / params.fall_max_run_m));
    let windows = if wanted < 1.0 { 1.0 } else if wanted > MAX_STATIONS { MAX_STATIONS } else { wanted };
    let count = windows as usize; // cast-ok: a whole number in 1..=MAX_STATIONS
    let mut best: Option<(usize, f64)> = None;
    let mut upper = (ground.height_m)(&from.point);
    for w in 0..count {
        let t = (w + 1) as f64 / windows;
        let lower = (ground.height_m)(&at(from.along_m + dx * t, from.lateral_m + dy * t));
        let drop = upper - lower;
        if drop.is_finite() {
            let better = match best { None => true, Some((_, d)) => drop > d };
            if better {
                best = Some((w, drop));
            }
        }
        upper = lower;
    }
    let (w, drop) = best?;
    if !(drop >= params.fall_min_drop_m) {
        return None;
    }
    let height = if drop < bed_drop { drop } else { bed_drop };
    let t0 = w as f64 / windows;
    let t1 = (w + 1) as f64 / windows;
    let end = |t: f64, bed_m: f64| {
        let along_m = from.along_m + dx * t;
        let lateral_m = from.lateral_m + dy * t;
        Fine { along_m, lateral_m, point: at(along_m, lateral_m), bed_m, keep: true }
    };
    Some((end(t0, from.bed_m), end(t1, from.bed_m - height), height))
}
```

Wire it into `trace_segment`:
- keep `let mut previous = Fine { along_m: 0.0, lateral_m: 0.0, point: start, bed_m: a.bed_m, keep: true };` before the loop;
- after computing each station's `Fine` (call it `here`) and before pushing it, run:

```rust
        if let Some((upper, lower, height)) = find_fall(ground, params, &at, &previous, &here) {
            segment.falls.push((upper.point, height));
            segment.interior.push(upper);
            segment.interior.push(lower);
        }
        segment.interior.push(here);
        previous = here;
```

After the loop, check the last step into `b`. Only do this when the segment was not trimmed, which is the case whenever the code reaches here, because a trimmed segment has already returned:

```rust
    let (ex, ey) = (len, 0.0);
    let into_end = Fine { along_m: ex, lateral_m: ey, point: end, bed_m: b.bed_m, keep: true };
    if let Some((upper, lower, height)) = find_fall(ground, params, &at, &previous, &into_end) {
        segment.falls.push((upper.point, height));
        segment.interior.push(upper);
        segment.interior.push(lower);
    }
```

Here `at` must take `&dyn Fn(f64, f64) -> SpherePoint`, so bind the closure as `let at = |along: f64, lateral: f64| ...;` and pass `&at`. The shore-trim branch returns before any fall is checked on that step, so a mouth never gets a fall; that is fine.

Run the refine tests. Expected: PASS.

- [ ] **Step 3: Falls reach the record.** `refine_reach` already turns `Segment.falls` into `Fall`s and pushes each `keep: true` interior point with `protected = true`. Add to `bake_tests.rs`:

```rust
/// Spec §6.7: every recorded fall sits on its own reach, at a point of that reach, and the next
/// point is lower by the fall's height.
#[test]
fn every_fall_is_a_step_on_its_own_reach() {
    let record = crate::hydrology::bake(&world(), &params()).expect("bake");
    for fall in &record.falls {
        let reach = &record.reaches[fall.reach as usize];
        let i = reach.points.iter().position(|p| (p.lat_deg, p.lon_deg) == fall.at)
            .expect("a fall's upper end is a point of its reach");
        let drop = reach.points[i].bed_m - reach.points[i + 1].bed_m;
        let d = drop - fall.height_m;
        assert!(d < 1e-6 && d > -1e-6, "fall height {} but the bed drops {}", fall.height_m, drop);
        assert!(fall.height_m >= 10.0);
    }
    eprintln!("falls on the test world: {}", record.falls.len());
}
```

Run it. Expected: PASS. Report the count, which may be 0 on this smooth test world; the analytic tests are the guard.

- [ ] **Step 4: Mutation guard.** Temporarily change `if !(drop >= params.fall_min_drop_m)` in `find_fall` to always `return None`. The cliff test must FAIL. Restore it, and paste the output.

- [ ] **Step 5: Spec.** Add Ruling R-5 to §6.7 and §7 (`falls: at` is the upper end; the lower end is the next reach point).

- [ ] **Step 6: Run everything and commit.** Run the full crate tests and `no_std_math`, then rebuild the wasm. Commit with the subject `Water 1b-2: waterfalls where a reach drops ten metres in a hundred and fifty`.

---

### Task 6: Meanders on flat wide rivers

**Files:**
- Modify: `src/hydrology/refine.rs`, and the spec (§6.6)
- Test: `refine.rs` tests

**Interfaces:**
- Consumes: `trace_segment` and `Ground.seed` (Task 4); `HydroParams.meander_wavelength_widths`, `meander_amplitude_widths` and `meander_max_slope` (Task 3); `crate::noise::Noise::{new(seed: u64, salt: u64), at(&self, x, y, z) -> f64}`.
- Produces: interior `Fine`s of qualifying segments shifted sideways. Nothing else changes.

- [ ] **Step 1: Write the failing tests:**

```rust
    fn wide(lat_deg: f64, lon_deg: f64, bed_m: f64, width_m: f64) -> ReachPoint {
        ReachPoint { lat_deg, lon_deg, bed_m, width_m, depth_m: 5.0, flow_m2: 1.0e12 }
    }

    /// Nearly flat ground falling east: 0.05% slope.
    fn flat(p: &SpherePoint) -> f64 { let (_, e) = north_east(p); 50.0 - 0.0005 * e }

    #[test]
    fn a_flat_wide_river_meanders_inside_its_corridor() {
        let a = wide(0.0, 0.0, 45.0, 1_000.0);
        let b = wide(0.0, 30_000.0 / M_PER_DEG, 30.0, 1_000.0);
        let g = ground(&flat);
        let seg = trace_segment(&g, &params(), &a, &b, None);
        let mut straight = params();
        straight.meander_amplitude_widths = 0.0;
        let plain = trace_segment(&g, &straight, &a, &b, None);
        assert_eq!(seg.interior.len(), plain.interior.len());
        let mut moved = 0usize;
        for (f, q) in seg.interior.iter().zip(&plain.interior) {
            let shift = f.lateral_m - q.lateral_m;
            assert!(shift <= 1_500.0 + 1e-9 && shift >= -1_500.0 - 1e-9, "shift {shift} exceeds 1.5 widths");
            assert!(f.lateral_m <= g.corridor_m && f.lateral_m >= -g.corridor_m);
            assert_eq!(f.bed_m, q.bed_m, "a meander moves the line, not the bed");
            if shift > 1.0 || shift < -1.0 { moved += 1; }
        }
        assert!(moved > seg.interior.len() / 2, "most stations moved: {moved}");
    }

    #[test]
    fn a_narrow_river_does_not_meander_at_this_resolution() {
        let a = wide(0.0, 0.0, 45.0, 100.0);
        let b = wide(0.0, 30_000.0 / M_PER_DEG, 30.0, 100.0);
        let g = ground(&flat);
        let mut straight = params();
        straight.meander_amplitude_widths = 0.0;
        assert_eq!(trace_segment(&g, &params(), &a, &b, None), trace_segment(&g, &straight, &a, &b, None));
    }

    #[test]
    fn a_steep_river_does_not_meander() {
        let steep = |p: &SpherePoint| { let (_, e) = north_east(p); 500.0 - 0.01 * e };
        let a = wide(0.0, 0.0, 495.0, 1_000.0);
        let b = wide(0.0, 30_000.0 / M_PER_DEG, 195.0, 1_000.0);
        let g = ground(&steep);
        let mut straight = params();
        straight.meander_amplitude_widths = 0.0;
        assert_eq!(trace_segment(&g, &params(), &a, &b, None), trace_segment(&g, &straight, &a, &b, None));
    }
```

Run them. Expected: the flat test FAILs (`moved == 0`).

- [ ] **Step 2: Implement.** In `refine.rs`, add:

```rust
use crate::noise::Noise;

/// Salt for the meander's phase, so it is independent of every other noise field on the world.
const MEANDER_SALT: u64 = 0x4d45_414e_4445_5253;
/// Scales a unit vector into the noise lattice, so neighbouring segments get unrelated phases.
const MEANDER_FREQUENCY: f64 = 64.0;
```

At the end of `trace_segment` (after the final-step fall check, before `segment`), add:

```rust
    // Ruling R-6: a meander only where it can be drawn at this step (a wavelength of at least
    // four steps), where the river is flat (bed slope under `meander_max_slope`), and where no
    // fall was found. It is tapered to zero at both coarse points and kept inside the corridor.
    // It moves the line, never the bed.
    let wavelength = params.meander_wavelength_widths * a.width_m;
    let slope = (a.bed_m - floor_m) / len;
    let amplitude = params.meander_amplitude_widths * a.width_m;
    if segment.falls.is_empty() && slope < params.meander_max_slope
        && wavelength >= 4.0 * params.refine_step_m && amplitude > 0.0 {
        let v = start.vector;
        let n = Noise::new(ground.seed, MEANDER_SALT)
            .at(v.x * MEANDER_FREQUENCY, v.y * MEANDER_FREQUENCY, v.z * MEANDER_FREQUENCY);
        if n.is_finite() {
            let pi = std::f64::consts::PI;
            let phase = pi * (1.0 + n);
            for fine in segment.interior.iter_mut() {
                let envelope = m::sin(pi * fine.along_m / len);
                let mut shift = amplitude * envelope * m::sin(2.0 * pi * fine.along_m / wavelength + phase);
                let room_left = ground.corridor_m - fine.lateral_m;
                let room_right = ground.corridor_m + fine.lateral_m;
                if shift > room_left {
                    shift = room_left;
                }
                if shift < -room_right {
                    shift = -room_right;
                }
                fine.lateral_m += shift;
                fine.point = at(fine.along_m, fine.lateral_m);
            }
        }
    }
```

A segment with a fall is never meandered, so no `keep` point moves. Run the refine tests. Expected: PASS.

- [ ] **Step 3: Mutation guard.** Temporarily delete the two corridor `if`s. Then add a test-only check that runs `a_flat_wide_river_meanders_inside_its_corridor` with `g.corridor_m = 500.0`. It must FAIL on the corridor assertion. Restore the `if`s. Keep a permanent variant of the test at `corridor_m = 500.0`, asserting that every lateral is within ±500 m.

- [ ] **Step 4: Spec and commit.**
  - Add Ruling R-6 to §6.6's meander sentence.
  - Run the full crate tests and `no_std_math`, and rebuild the wasm.
  - Commit with the subject `Water 1b-2: flat wide rivers meander inside their corridor`.

---

### Task 7: Simplify refined reaches

**Files:**
- Modify: `src/hydrology/refine.rs`
- Test: `refine.rs` tests, and `bake_tests.rs`

**Interfaces:**
- Consumes: `Refined { points, protected, falls }` from `refine_reach`, plus `HydroParams.refine_simplify_m` and `refine_vertical_m`.
- Produces: `pub fn simplify(points: &[ReachPoint], protected: &[bool], radius_m: f64, params: &HydroParams) -> Vec<ReachPoint>`. `refine` stores `simplify(...)` of each refined reach.

- [ ] **Step 1: Write the failing tests:**

```rust
    fn line_of(beds: &[f64], north_m: &[f64]) -> Vec<ReachPoint> {
        beds.iter().zip(north_m).enumerate()
            .map(|(i, (&bed, &n))| point(n / M_PER_DEG, (i as f64 * 1_500.0) / M_PER_DEG, bed))
            .collect()
    }

    #[test]
    fn a_straight_even_line_keeps_only_its_ends() {
        let pts = line_of(&[10.0, 9.0, 8.0, 7.0, 6.0], &[0.0; 5]);
        let out = simplify(&pts, &[true, false, false, false, true], R, &params());
        assert_eq!(out, vec![pts[0].clone(), pts[4].clone()]);
    }

    #[test]
    fn a_bend_wider_than_the_tolerance_is_kept_and_a_small_one_is_not() {
        let pts = line_of(&[10.0, 9.0, 8.0], &[0.0, 400.0, 0.0]);
        assert_eq!(simplify(&pts, &[true, false, true], R, &params()).len(), 3);
        let small = line_of(&[10.0, 9.0, 8.0], &[0.0, 100.0, 0.0]);
        assert_eq!(simplify(&small, &[true, false, true], R, &params()).len(), 2);
    }

    #[test]
    fn a_bed_step_over_a_metre_is_kept() {
        let pts = line_of(&[10.0, 7.0, 6.5], &[0.0; 3]);
        assert_eq!(simplify(&pts, &[true, false, true], R, &params()).len(), 3);
    }

    #[test]
    fn protected_points_are_always_kept() {
        let pts = line_of(&[10.0, 9.0, 8.0, 7.0, 6.0], &[0.0; 5]);
        let out = simplify(&pts, &[true, false, true, false, true], R, &params());
        assert_eq!(out, vec![pts[0].clone(), pts[2].clone(), pts[4].clone()]);
    }
```

Run them. Expected: FAIL (`simplify` not defined).

- [ ] **Step 2: Implement:**

```rust
/// Ruling R-7: Douglas–Peucker over one refined reach. Between two kept points, the point that
/// strays furthest -- sideways from their chord, in units of `refine_simplify_m`, or off their
/// straight-line bed, in units of `refine_vertical_m`, whichever is worse -- is kept if it strays
/// more than one unit, and the two halves are examined in turn. Protected points (coarse points,
/// fall ends, the mouth) and both ends are always kept. Keeping a subset of a falling bed keeps it
/// falling, so spec §14.5 survives. The outcome does not depend on the order spans are examined.
pub fn simplify(points: &[ReachPoint], protected: &[bool], radius_m: f64, params: &HydroParams) -> Vec<ReachPoint> {
    let n = points.len();
    if n <= 2 {
        return points.to_vec();
    }
    let at = |p: &ReachPoint| SpherePoint::from_latlon(p.lat_deg, p.lon_deg);
    let mut keep: Vec<bool> = protected.to_vec();
    keep[0] = true;
    keep[n - 1] = true;
    let anchors: Vec<usize> = (0..n).filter(|&i| keep[i]).collect();
    let mut spans: Vec<(usize, usize)> = anchors.windows(2).map(|w| (w[0], w[1])).collect();
    while let Some((lo, hi)) = spans.pop() {
        if hi <= lo + 1 {
            continue;
        }
        let frame = TangentFrame::at(&at(&points[lo]), radius_m);
        let (bx, by) = frame.sphere_to_local(&at(&points[hi]));
        let len2 = bx * bx + by * by;
        let mut worst = 0.0;
        let mut worst_at = lo;
        for i in lo + 1..hi {
            let (px, py) = frame.sphere_to_local(&at(&points[i]));
            let raw = if len2 > 0.0 { (px * bx + py * by) / len2 } else { 0.0 };
            let t = if raw < 0.0 { 0.0 } else if raw > 1.0 { 1.0 } else { raw };
            let sideways = m::hypot(px - t * bx, py - t * by) / params.refine_simplify_m;
            let straight_bed = points[lo].bed_m + t * (points[hi].bed_m - points[lo].bed_m);
            let off = points[i].bed_m - straight_bed;
            let vertical = (if off < 0.0 { -off } else { off }) / params.refine_vertical_m;
            let err = if sideways > vertical { sideways } else { vertical };
            if err > worst {
                worst = err;
                worst_at = i;
            }
        }
        if worst > 1.0 {
            keep[worst_at] = true;
            spans.push((lo, worst_at));
            spans.push((worst_at, hi));
        }
    }
    points.iter().zip(&keep).filter(|(_, &k)| k).map(|(p, _)| p.clone()).collect()
}
```

In `refine`, store the simplified line: `reach.points = simplify(&refined.points, &refined.protected, ground.radius_m, params);`. Run the refine tests and all of `bake_tests.rs`. Expected: PASS. The bake tests on beds, junctions and coarse points must still pass, because simplification keeps protected points.

- [ ] **Step 3: Falls survive simplification.** `every_fall_is_a_step_on_its_own_reach` (Task 5) already checks this on the real bake. Add an analytic check: in the `refine.rs` tests, build a two-point `ReachLine` over `cliff`, run `refine_reach` and then `simplify`, and assert that both fall ends are still present.

- [ ] **Step 4: Report the size effect.** In a test with `--nocapture`, print the refined point count before and after simplification on the test world (`record_of` coarse count, refined count, simplified count). Report the three numbers.

- [ ] **Step 5: Commit.** Run the full crate tests and `no_std_math`, and rebuild the wasm. Commit with the subject `Water 1b-2: refined reaches simplified to 250 m and one metre`.

---

### Task 8: Integration, pins, and the owner world

**Files:**
- Modify: `src/bin/hydro_survey.rs`, `viewer/public/app/water-preview.js` (falls drawn), `viewer/public/app/world-panel.js` (summary line), `.github/workflows/gates.yml`, `README.md`, and `docs/superpowers/reports/2026-09-10-water-1a-carry-forward.md`
- Create: `docs/superpowers/reports/2026-09-12-water-1b2-verification.md`

**Interfaces:**
- Consumes: everything above.
- Produces: pins, the verification report, and I3's outcome.

- [ ] **Step 1: The survey times the refinement.** In `src/bin/hydro_survey.rs`, time `refine::refine` separately from `bake_stages` and `record_of`. The survey builds the `Ground` exactly as `hydrology::bake` does. Print:
  - refined point count;
  - falls;
  - `capped_basins`, `capped_inner` and `capped_inner_kept`;
  - record words × 8 as bytes.

  Run it natively at 1,000,000 nodes on the three stand-in worlds the 1b-1 final review used:
  - seed 562,423,712 at radius 4.5e6;
  - seed 20,260,904 at 6,371,000;
  - seed 1 with ranges.

  Read the survey's `--help` or source for its arguments. Report per-part times and sizes.

- [ ] **Step 2: Size and time gates, native first.** If any stand-in's record is over 8,000,000 bytes, raise `refine_simplify_m` in `earth_like` from 250 to 500, then 1000, re-running until all three fit. Record the ruling with the numbers. If refinement alone takes more than 120 s natively on any stand-in, stop and report the numbers (the controller rules).

- [ ] **Step 3: The preview draws falls.** In `viewer/public/app/water-preview.js::drawPreview`, add the falls as points: white, pixelSize 7, with `disableDepthTestDistance: Number.POSITIVE_INFINITY` and a description of `waterfall, N m`. Add `falls` to `counts`. In the world panel's summary line, append `, W waterfalls`. Add a `water-preview.test.mjs` assertion that `decodeHydro(words).falls.length === hydroSummary(words).falls` (or the summary's falls field, whatever `hydroSummary` names it; read it first).

- [ ] **Step 4: Rebuild, parity, pins.** From `viewer/`, run `npm run build:wasm`, then the parity command gates.yml runs. It must give 0 divergent. Then re-derive, by running:
  - the engine counts for every binary and feature set gates.yml pins;
  - the parity totals and every control, including the native prediction;
  - the Python pin.

  Update gates.yml and the README mirror with a dated note (2026-09-12, plan 1b-2), giving old and new values.

- [ ] **Step 5 (controller): Bake the owner world.** Serve this branch on :8138 (`set PORT=8138&& npm run serve` in `viewer/`). Open `worlds/world-1788998299904.json` and bake with the **preview water** button, forced outlet `0,0`, at 1M nodes. From the decoded record, collect:
  - the bake time;
  - the record bytes;
  - bodies, fresh and salt;
  - reaches by class;
  - refined points;
  - falls;
  - `capped_basins`, `capped_inner` and `capped_inner_kept`;
  - fresh dead-ends (must be 0);
  - forced matched;
  - the great lake's outlet path and mouth (1b-1: ocean at 26.13°S 30.13°W);
  - a check, over every reach, that beds never rise and every junction is shared.

  Gates:
  - the record is at most 8 MB, or else apply Step 2's raise and re-bake;
  - the bake takes at most 300 s, or else report and rule.

- [ ] **Step 6: Write the verification report**, `docs/superpowers/reports/2026-09-12-water-1b2-verification.md`, in the form of the 1b-1 report: population, method, host, results against 1b-1, and per-part timing. Also:
  - **I3:** close it if `capped_basins == 0` (nothing to lose) or `capped_inner_kept > 0`. Otherwise keep it open, with `capped_inner` and `capped_inner_kept` stated.
  - **Carry-forward:** tick what this plan closed: notch ends and splits, the flow bound, the forced index, the tests split, the drainage code, the parity status check, acyclicity in O(R), the stale docs, the doc comments, the I4 mouth rise (Ruling R-4) and I3. Add a line pointing to plan 1b-3 for outlines and ponds.

- [ ] **Step 7: Commit.** Commit with the subject `Water 1b-2: pins re-derived, and the fine channels verified on the owner's world`.

---

## Self-review

**Spec coverage:**

| Spec item | Where |
|---|---|
| §6.6 tracing at 1.5 km, every step downhill, inside the corridor | Task 4 |
| §6.6 a hollow met on the way | Ruling R-2, in Task 4 |
| §6.6 meander | Task 6 |
| §6.6 tributaries joining at a shared vertex | Ruling R-1, Task 4 test |
| §6.6 rivers end at the coast or a lake shore | Ruling R-3, Task 4 |
| §6.6 lake outlines and small lakes | plan 1b-3 (scope ruling) |
| §6.7 falls | Task 5 |
| §7 params echo | Task 3 |
| §7 falls layout | Ruling R-5 |
| §7 size target | Task 7 sizes it, Task 8 gates it |
| §14.1 determinism | existing tests plus parity (Task 8) |
| §14.2 everything drains | refinement never touches routing; sweep in Tasks 1 and 4 |
| §14.4 connected | Task 4 |
| §14.5 downhill | Tasks 4, 5 and 7 |
| §14.10 mutation guards | Tasks 4, 5 and 6 |
| Carry-forward notch ends and splits | Task 2 |
| Carry-forward I3 | Tasks 3 and 8 |
| Carry-forward minors | Task 1 |

**Types:** these names are used identically across Tasks 4–7: `Ground`, `Fine`, `Segment`, `Refined`, `trace_segment`, `refine_reach`, `refine`, `simplify`, `find_fall`, `terminal_level` and `beds_never_rise`. `judge(hollows, forced, params)` is the same in Tasks 1 and 3. Header words 32–42 are the same in Task 3's Rust and JS.
