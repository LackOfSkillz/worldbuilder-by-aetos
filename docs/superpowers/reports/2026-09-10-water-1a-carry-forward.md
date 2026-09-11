# Water 1a — what carries forward to plan 1b and stage 2

Plan 1a (the coarse bake) is complete: 12 tasks plus a calibration task (12b) and a final fix wave, every one reviewed, and the final whole-branch review's findings addressed. This file is the durable record of what was deliberately left, and of the rulings made on the owner's behalf. The execution ledger was scratch; this replaces it.

## Owner-world verification (after the final fix)

Measured in the branch studio (wasm), on `worlds/world-1788998299904.json` at the default 1,000,000 nodes.

**Cost:** 87 s, heap 355 MB, record 23.6 MB. `drainage_check` passes.

**Bodies:** 343, of which 285 are fresh lakes and 58 are salt. No closed lake reports an outlet. The largest lake above the datum is 344,503 km², under the Caspian cap.

**Reaches:** 4,933 (3,449 streams, 1,359 rivers, 125 great rivers), up to order 5, with bifurcation ratios of 4.58–14. All 957 ocean mouths have their bed at the datum.

**The great lake** is fresh, forced and enclosed, 41.33M km² at level 0. Its outlet path runs through three smaller fresh lakes and reaches the **ocean at 26.13°S 30.13°W**; the lowest ridge measured by hand before the design was at 26.00°S 30.25°W. In full: body 63 → reach 3002 → 3096 → body 318 (156k km²) → 3225 → body 319 (26k km²) → 3277 → 3299 → body 320 (55k km²) → reach 3370 → ocean.

**Parity:** plain 136,086 compared, 0 divergent; all seven mutation controls pass.

**CI pins:** engine 692/692/694/798/800 (5 ignored); Python 565.

## Rulings made during execution (each with what it costs if wrong)

| Ruling | Decision | Cost if wrong |
|---|---|---|
| A | `mod.rs` re-exports `Downstream`, `ReachClass` | none |
| B | record types derive Debug/Clone/PartialEq | none |
| C | `parity_dump.rs` keeps its own copy of the test params (twin named in a comment) | the two copies can drift |
| D | Task 1 tests accepted as written (not fail-first) | none material |
| E | owner-world calibration done by the controller in the studio | one extra dispatch |
| Count pins | re-derived once, in Task 11 and again later, not per task | none |
| Task 6-1 | seed-pocket nested hollows are dropped | none |
| Task 6-2 | an enclosed hollow splits into one lake per below-datum pocket (spec W1) | a pocket that should have merged with a neighbour stays its own small lake |
| Task 6-3 | notches stop at lake members and never cut a lake | none |
| Forced outlets | specified by a point inside the water (the owner world uses the inland sea's centre, 0°N 0°E) | none |
| Task 8 | the bifurcation coverage gap is closed in Task 9 | none |
| 12b-1 | resolution-aware thresholds: stream ≥ 10 × median land-node area; river ≥ 10 × stream; great ≥ 10 × river; SCHEMA 2 carries them | the smallest coarse stream drains about 10 nodes; plan 1b must add the smaller streams |
| 12b-2 | the record keeps only notches on recorded rivers or lake outlets | graph pits stay until plan 1b; **ineffective in practice** (see below) |
| 12b-3 | `DEFAULT_TOTAL_NODES = 1,000,000` | none |
| 12b-4 | the plan's "stream ×2 up to 4 steps" lever is superseded | none |
| 12b-5 | no lake larger than the Caspian: `keep_max_area_m2 = 4e11`, larger non-enclosed, non-forced hollows are notched | very large basins become drained lowlands with deep graph-scale notches, losing their inner lakes (I3) |
| C1-a | nested kept hollows on any pocket's outlet path are notched before routing | a shore lake on an inland sea's way out becomes drained ground even if that sea later closes; see the residual below |
| C1-b | enclosed pockets are re-judged in a loop until stable | none |
| C1-c | `drainage_check` in `bake()`; `HydroError::Drainage` maps to `WB_ERR_GRAPH` | a world that cannot drain is refused rather than baked wrong |
| I6 | a parity length mismatch throws in the plain run and counts as divergence under controls; a second `H` record (tectonic world, forced outlet, floor binding) with a TCTL prediction | none |
| Residual (final re-review) | parked to plan 1b, see below | a rare world is refused with `WB_ERR_GRAPH` until plan 1b fixes the cut |

## Status after plan 1b-1 (2026-09-11)

Plan 1b-1 closed items 1, 2, 4 and 5 below, the §7 fields, the forced-miss report and the `mod.rs` split (see `2026-09-11-water-1b1-verification.md`: record 2.2 MB, 0 fresh lakes dead-end, forced outlets reported). **Item 3 (I3) stays open.** Its mechanism is fixed and keeps 79 inner lakes over 122 real 15k–40k worlds, but it is not counted on the owner world at 1M, because the record carries no "capped" flag. The ruling says it closes only on that count, so plan 1b-2 adds a capped-inner-lakes statistic and measures it. Item 6 (the fine layer) is plan 1b-2.

## Plan 1b must fix (load-bearing for stage 2)

1. **Done in 1b-1.** **The residual outlet-cut cycle.** `cut_path` stops on "ground already lower" (`routing.rs:~316`) even when that ground's receiver chain leads back into the source lake. A minima cut that descends to below −1 m next to a pocket can then close a cycle.
   - Fixtures: line `[-50,-40,39,10,0.02,0.1,0.3,0.5,-5,60]`, and `[-50,-40,39,30,0.05,20,14,12,10,8,6,4,2,-5,60]` (via C1-a), both at area 1e6 and wetness 0.5.
   - Real worlds: 0 of 96 bakes were refused.
   - Fix: do not stop on lower ground that drains back into the source lake, or lower that ground to the cut's bed.
2. **Done in 1b-1.** **I2:** notch and outlet paths inside flat-filled hollows follow node-index tie order, which is raster-like because spiral indices follow latitude. Make them terrain-following or least-cost before stage 2 carves them (this moves the parity pins).
3. **Open: mechanism done in 1b-1, owner-world count pending (1b-2).** **I3:** 12b-5's capped giant basins lose their inner lake-worthy sub-basins. Give them nested judging.
4. **Done in 1b-1.** **I5:** fresh lakes with no downstream link (4–34 per 200k world). Add `Body.downstream`, or always start a reach at a fresh lake's outlet.
5. **Done in 1b-1.** **Record size:** 23.6 MB against the spec's 8 MB target, where notches are 74–94% of the words. About half of the retained notch points duplicate reach points (whose bed already carries the cut). Record only outlet cuts, plus off-reach cut segments above a depth threshold.
6. **The fine layer itself:** tracing at 1.5 km, lake outlines, small lakes and ponds (which have their own keep rule), and waterfalls. There are 0 ponds at graph resolution.

## Smaller items (plan 1b or stage 2)

- **Done in 1b-1.** Spec §7 fields missing from the record: notch `width_m`, reach `fresh`, body `override`, a params echo.
- A width-anchor ruling. Spec §6.5 over-determines it: 3 m at the stream threshold and 1,000 m at the great threshold cannot both hold with a fixed exponent. The code anchors the stream end.
- **Done in 1b-1.** Report silent forced-outlet misses. A forced point whose nearest node is land, ocean or shore is ignored. C1-a can also drain a forced nested hollow on an outlet path.
- The I4 side effect: a mouth's bed can rise at the last step (the neighbour at about −1 m minus depth, the mouth at 0). Stage 2 carving should expect it.
- **Done in 1b-1.** `cut_path` pushes an empty `NotchRoute` when it stops at k=1, and `outlet_notch` then points at it.
- C1-b never takes back a fresh verdict when a later cut reduces a pocket's inflow (monotone by design).
- `HydroError::Drainage` shares `WB_ERR_GRAPH` with sampling failures.
- If a control ever makes `wb_hydro_bake` fail, `parity.mjs` case `H` reads an unwritten `out_id`.
- `reaches_are_acyclic` is O(R²). `mod.rs` was split in 1b-1 (227 lines, with `bake.rs` at 730, both at 049d9c1; `bake.rs` has grown since with its tests); the hydro half of `wasm.rs` still waits for stage 2.
- Stale docs:
  - the calibration report's post-fix parity figures should be 136,086 / seed 130,366 / tectonic-warp 13,590;
  - the gates.yml step name "all on the belt";
  - the README says "other four TCTL fields", and there are five.
- debug_asserts or doc comments for: `BucketIndex::nearest`'s unchecked index, `candidates`' third disjunct, seeds versus `allowed`, the wetness length, the `outlet == NO_NODE` fallback, the `outlet_path.len() < 2` fresh sink, the reaches.rs invariants, and the `count_fits` minimums.
