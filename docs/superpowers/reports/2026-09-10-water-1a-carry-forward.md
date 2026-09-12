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

## Status after plan 1b-2 (2026-09-12)

Plan 1b-2 re-traces every reach on the landform at 1.5 km, adds falls and meanders, and simplifies each refined line. See `2026-09-12-water-1b2-verification.md`. On the owner's world at 1M nodes, forced outlet 0°N 0°E, measured in the branch studio (wasm) on the owner's laptop:

- the record is 5,957,984 bytes and the bake takes 64.4 s;
- 0 bed rises along any reach;
- all 3,268 junctions are shared bit for bit;
- 0 mouths sit above their water;
- 0 fresh lakes dead-end.

The size gate failed at the planned 250 m tolerance (8,659,856 bytes), so `refine_simplify_m` rose to 500 m.

It closed:

- **I3**, on the owner-world count: 7 capped basins keep 9 of their 60 inner hollows;
- the notch ends and splits;
- the flow bound;
- the forced index;
- the tests split;
- the drainage status code;
- the parity status check;
- acyclicity in O(R);
- the stale docs;
- the doc comments;
- the I4 mouth rise (Ruling R-4).

Two parts of item 6 remain:

- [x] ~~**Lake outlines, and small lakes and ponds**, go to **plan 1b-3 (shores)**, which starts with a spike.~~ **Superseded:** small lakes and ponds were done in 1b-3; **lake outlines go to plan 1b-4**, ruled in 1b-3's Task 1 as Candidate B.
- [x] **Done in 1b-3** (Tasks 2 and 3, and Ruling S-14). **Reaches still cross one another** inside their corridors (Ruling FF-3 of the 1b-2 final review). The pass now runs on the lines the record **ships** — trace, meander, simplify, then check, repeating up to four times — so the shipped count is at or under the coarse count everywhere measured: **43 coarse against 37 shipped** on the owner's world, 54 → 50 and 33 → 28 on the 1M stand-ins. **Coarse crossings stay** (Ruling S-2): a coarse crossing is a graph artifact and fixing it means re-routing, which is out of scope.
  - **These figures are not the 61 and 95 published above, and the counting changed.** 1b-2 counted a crossing once per intersecting pair of *node paths* over the whole coarse graph; 1b-3's `crossings_coarse` counts intersecting *coarse segment* pairs among the reaches the record keeps, which is the population the shipped count has to be judged against. Same worlds, same populations (`plain` and `seed 1 ranges` at 1M) — 61 and 95 under the old counting, 33 and 54 under the new. Neither is wrong; they are different questions, and only the second can be compared with a shipped count.
- **Falls** work (4 on a native `ranges()` stand-in, and analytic cliff tests), but there are **0 on the owner world**. Its landform has no 10 m drop within 150 m along any reach at these resolutions. Falls await the mountains project's steeper relief.

## Status after plan 1b-3 (2026-09-12)

Plan 1b-3 (shores) stops the refinement introducing river crossings, adds spec §6.6's fine pond search, and moves the record to SCHEMA 5 (a 54-word header). See `2026-09-12-water-1b3-verification.md`. On the owner's world at 1M nodes, forced outlet 0°N 0°E, measured in the branch studio (wasm) on the owner's laptop at c13d150:

- the record is **7,019,992 bytes** (gate 8,000,000) and the bake takes **242 s** (gate 300 s);
- **4,067 bodies** = 348 coarse + **3,719 ponds**, of 55,742 found;
- crossings **43 coarse → 37 shipped**;
- 0 bed rises, 3,268 junctions all shared, 0 mouths above their water, 0 fresh dead-ends, 0 ponds breaking Ruling S-5 or shipping a ring under 3 points;
- the great lake still runs 63 → 316 → 317 → 318 → 322 → 319 to the ocean at 25.938°S 30.106°W.

**Two departures from spec §6.6 ship together, both for the gates and both measured.** The density cap is **1.6e10 m² against the spec's 5.0e8** (32×), because the same world recorded 11,146,072 bytes at 4.0e9; and the search corridor is **1,500 m against the spec's 3,000 m**, because at 3 km it took 432–441 s against a 300 s gate. The consequence: **about ten times fewer ponds** than §6.6's own parameters would place — 3,719 shipped against roughly 39,000, which is an **extrapolation**, resting on one measured bake (5,184 ponds at the spec's 3 km corridor with the shipped cap) and one extrapolated ratio (×0.669 a cap doubling, measured over two doublings, applied over five). Treat it as an order of magnitude; a single bake at 5.0e8 and 3 km would replace it with a count. The other half is not an extrapolation: **no pond is looked for beyond 1.5 km of a river** at all. `pond_cell_m` stays at the spec's 250 m, so no recorded geometry is coarser.

**Ruling S-15 is deferred as ruled:** at 1M the crossing pass's first round sees **2,909 crossings**, so that many segments ship straightened and unmeandered under Ruling S-4a. Re-tuning `meander_amplitude_widths` waits for the mountains project's erosion.

### Routed to plan 1b-4 by plan 1b-3

Task 1 ruled **Candidate B** for a body's extent (see item 6 above) and rewrote spec §6.6, §7 and §8.3 to match. Three places in the spec still describe the ring that ruling replaced, and 1b-4 owns all three; two coverage gaps that Rulings S-16 and S-17 opened go with them.

- [ ] **Spec §8.2's spatial index must list a body whose shore points come within `shore_reach_m` of a cell.** A nearest-point test is only as good as the candidate set the index hands it; an index built for polygon containment will not return the right bodies.
- [ ] **Spec §9 still names `dilateBodyExtents` and the box-and-level rule.** Both are polygon-era; §9 has to be rewritten against the shore-point set.
- [ ] **Spec §14's two uses of "outline" are still curve-sense.** They read as a closed curve and must be restated for an unordered point set, or scoped explicitly to ponds, which do keep the 250 m trace.
- [ ] **The parity corpus must always compare a pond body.** After Ruling S-17 the `hydro/plain` record keeps **no pond at all** (3,949 words = its pre-pond 3,938 plus SCHEMA 5's eleven header words), so of the corpus's two hydro records only `hydro/ranges` carries pond bodies across the wasm boundary. Not a defect today, but a future tuning that also emptied `hydro/ranges` would leave the pond body layout crossing the boundary with nothing comparing it, and **no gate would say so**. Either raise `examples/parity_dump.rs`'s hydro populations, or add a third record chosen so it keeps ponds at whatever params ship.
- [ ] **No Rust test bakes at the shipped pond parameters.** All six `ponds::tests` cases and `bake_tests::ponds_obey_their_keep_rule_and_name_a_river` pin spec §6.6's 3 km corridor deliberately, because what they assert is the search's mechanism; each is right on its own, and together they leave `earth_like`'s 1,500 m and 1.6e10 unexercised in Rust. 1b-3's fix wave added the cheap half — `bake_tests::earth_like_ships_the_tuned_pond_corridor_and_density`, a value pin naming S-16 and S-17 — but that pins the constants, not what they do. The viewer's `water-preview.test.mjs` pond test (50,000 nodes) is the only behavioural test at the shipped values, and it is on the wasm side.

## Plan 1b must fix (load-bearing for stage 2)

1. **Done in 1b-1.** **The residual outlet-cut cycle.** `cut_path` stops on "ground already lower" (`routing.rs:~316`) even when that ground's receiver chain leads back into the source lake. A minima cut that descends to below −1 m next to a pocket can then close a cycle.
   - Fixtures: line `[-50,-40,39,10,0.02,0.1,0.3,0.5,-5,60]`, and `[-50,-40,39,30,0.05,20,14,12,10,8,6,4,2,-5,60]` (via C1-a), both at area 1e6 and wetness 0.5.
   - Real worlds: 0 of 96 bakes were refused.
   - Fix: do not stop on lower ground that drains back into the source lake, or lower that ground to the cut's bed.
2. **Done in 1b-1.** **I2:** notch and outlet paths inside flat-filled hollows follow node-index tie order, which is raster-like because spiral indices follow latitude. Make them terrain-following or least-cost before stage 2 carves them (this moves the parity pins).
3. **Closed in 1b-2.** Mechanism done in 1b-1; on the owner world at 1M, 7 capped basins keep 9 of their 60 inner hollows (SCHEMA 4 header words 32–34). **I3:** 12b-5's capped giant basins lose their inner lake-worthy sub-basins. Give them nested judging.
4. **Done in 1b-1.** **I5:** fresh lakes with no downstream link (4–34 per 200k world). Add `Body.downstream`, or always start a reach at a fresh lake's outlet.
5. **Done in 1b-1.** **Record size:** 23.6 MB against the spec's 8 MB target, where notches are 74–94% of the words. About half of the retained notch points duplicate reach points (whose bed already carries the cut). Record only outlet cuts, plus off-reach cut segments above a depth threshold.
6. **Tracing and waterfalls done in 1b-2; small lakes and ponds done in 1b-3; outlines are plan 1b-4.** **The fine layer itself:** tracing at 1.5 km, lake outlines, small lakes and ponds (which have their own keep rule), and waterfalls.
   - [x] **Tracing and waterfalls — done in 1b-2.**
   - [x] **Small lakes and ponds — done in 1b-3.** 3,719 ponds on the owner's world, each with a traced 250 m ring and each naming a river by Ruling S-5. **Two of spec §6.6's parameters ship changed** to meet the gates: the density cap is 1.6e10 m² against the spec's 5.0e8 (32×), and the search corridor is 1,500 m against the spec's 3,000 m. See `2026-09-12-water-1b3-verification.md`.
   - [ ] **Lake outlines — plan 1b-4.** Ruled in 1b-3's Task 1 as **Candidate B**, replacing Ruling S-1: a body's extent is an unordered set of shore points (its shore members then its collar) plus `shore_member_count`, `shore_reach_m` and its level, and `water_at` decides by a nearest-point test rather than a polygon. Ponds keep the 250 m trace. The argument is in `2026-09-12-water-1b3-outlines-design.md`.
   - Falls are 0 on the owner world and await steeper relief (see the 1b-2 status above).

## Smaller items (plan 1b or stage 2)

- **Done in 1b-1.** Spec §7 fields missing from the record: notch `width_m`, reach `fresh`, body `override`, a params echo.
- A width-anchor ruling. Spec §6.5 over-determines it: 3 m at the stream threshold and 1,000 m at the great threshold cannot both hold with a fixed exponent. The code anchors the stream end.
- **Done in 1b-1.** Report silent forced-outlet misses. A forced point whose nearest node is land, ocean or shore is ignored. C1-a can also drain a forced nested hollow on an outlet path.
- **Done in 1b-2** (Ruling R-4: a mouth's bed is `min(previous bed, water level)`). The I4 side effect: a mouth's bed can rise at the last step (the neighbour at about −1 m minus depth, the mouth at 0). Stage 2 carving should expect it.
- **Done in 1b-1.** `cut_path` pushes an empty `NotchRoute` when it stops at k=1, and `outlet_notch` then points at it.
- C1-b never takes back a fresh verdict when a later cut reduces a pocket's inflow (monotone by design).
- **Done in 1b-2** (`WB_ERR_DRAINAGE = 7`). `HydroError::Drainage` shares `WB_ERR_GRAPH` with sampling failures.
- **Done in 1b-2** (a non-zero status is divergent, and `out_id` is not read). If a control ever makes `wb_hydro_bake` fail, `parity.mjs` case `H` reads an unwritten `out_id`.
- **Acyclicity done in 1b-2** (a colour walk, O(R)). `reaches_are_acyclic` is O(R²). `mod.rs` was split in 1b-1 (227 lines, with `bake.rs` at 730, both at 049d9c1; `bake.rs` has grown since with its tests); the hydro half of `wasm.rs` still waits for stage 2.
- **Done in 1b-2.** Stale docs:
  - the calibration report's post-fix parity figures should be 136,086 / seed 130,366 / tectonic-warp 13,590;
  - the gates.yml step name "all on the belt";
  - the README says "other four TCTL fields", and there are five.
- **Done in 1b-2, except the reaches.rs invariants** (not in the plan; still open). debug_asserts or doc comments for: `BucketIndex::nearest`'s unchecked index, `candidates`' third disjunct, seeds versus `allowed`, the wetness length, the `outlet == NO_NODE` fallback, the `outlet_path.len() < 2` fresh sink, the reaches.rs invariants, and the `count_fits` minimums.

## Added by the plan 1b-1 final review (2026-09-11)

**Plan 1b-2 must fix (stage 2 blocks on it):**

- [x] **Done in 1b-2 (Task 2, Ruling F-3a).** **Notch lines have no ends, and some join nodes that are not neighbours.**
  - 475 of 478 outlet cuts stop one node short of the water they drain into.
  - Split a `NotchLine` wherever two consecutive points are not graph neighbours.
  - Append the node the cut stopped at as a final point. Its surface is the datum for the ocean, or the lake's level otherwise (Ruling I4).
  - The layout stays as it is.
  - Code: `routing.rs` around 435, and the notch emission in `bake.rs`.

**Plan 1b-2 carry-forward:**

- [x] **Done in 1b-2** (flow thresholds above 1e20 m² are refused). A finite `stream_flow_m2` of 1e308 overflows the effective river threshold to infinity, so the record carries non-finite words. Refuse or bound such params. Code: the effective-threshold code in `bake.rs`.
- [x] **Done in 1b-2.** `judge` rebuilds the forced-outlet index for every enclosed or capped basin. This is a speed cost only.
- [x] **Done in 1b-2** (`bake_tests.rs`). Split `bake.rs`'s tests into their own file. The file is 1,306 lines, about 840 of them tests.
- [x] **Done in 1b-2.** The comment on `effective_thresholds_rise_to_the_graph_resolution`, and the matching README note, say the test world's thresholds sit above the node floor. They don't: the floor binds there too.

**Stage 2:**

- [ ] `decode` accepts dangling links. Add a validation pass before stage 2 reads records from disk.

**Rulings from the final review:**

- **F-1:** notch widths use the caller's params, the same as reaches.
- **F-2:** notch point word 3 is the cut surface; reach point word 3 is the bed (surface − depth). A test holds the two consistent wherever they meet.
