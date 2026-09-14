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

- [x] ~~**Lake outlines, and small lakes and ponds**, go to **plan 1b-3 (shores)**, which starts with a spike.~~ **Superseded, and now closed:** small lakes and ponds were done in 1b-3; **lake outlines were done in 1b-4**, ruled in 1b-3's Task 1 as Candidate B and measured on the owner's world in 1b-4. **Item 6 is complete, and it was the last open entry in "Plan 1b must fix (load-bearing for stage 2)" below — the six items plan 1a routed there are all closed.** What remains in this file is the smaller items and the stage 2 list, gathered under "Routed to stage 2" at the end.
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

### Routed to plan 1b-4 by plan 1b-3 (all five closed in 1b-4 — see "Status after plan 1b-4")

Task 1 ruled **Candidate B** for a body's extent (see item 6 above) and rewrote spec §6.6, §7 and §8.3 to match. Three places in the spec still describe the ring that ruling replaced, and 1b-4 owns all three; two coverage gaps that Rulings S-16 and S-17 opened go with them.

- [x] **Done in 1b-4 (Task 5), as spec text; the implementation is stage 2's.** **Spec §8.2's spatial index must list a body whose shore points come within `shore_reach_m` of a cell.** §8.2 now states the dilation, its measured size (58,083 m median and 62,586 m max on the owner's world, against 50 km cells — a whole ring of cells, not a rounding allowance), and that a pond's traced curve dilates by nothing. A nearest-point test is only as good as the candidate set the index hands it; an index built for polygon containment will not return the right bodies. **Building it is item 1 of "Routed to stage 2" below** (Ruling E-7).
- [x] **Done in 1b-4 (Task 5).** **Spec §9 named `dilateBodyExtents` and the box-and-level rule**, both polygon-era. §9's drawing path is rewritten against the shore-point set.
- [x] **Done in 1b-4 (Task 5).** **Spec §14's uses of "outline" were curve-sense.** §14.3, §14.6 and §14.9 are restated for an unordered point set, or scoped explicitly to ponds, which do keep the 250 m trace.
- [x] **Done in 1b-4 (Task 4).** **The parity corpus must always compare a pond body.** After Ruling S-17 the `hydro/plain` record kept **no pond at all**, so of the corpus's two hydro records only `hydro/ranges` carried pond bodies across the wasm boundary — and a future tuning that also emptied `hydro/ranges` would have left the pond body layout crossing the boundary with nothing comparing it, and **no gate would say so**. Task 4 raised `examples/parity_dump.rs`'s `HYDRO_PARAMS[0]` from 12,000 to **20,000 nodes** (`examples/parity_dump.rs:1949`), chosen as the smallest of {20,000; 30,000; 50,000} that keeps ponds and closest to the original; `hydro/plain` now bakes 13 bodies — **4 ponds and 9 coarse** — so it carries a `shore_member_count == 0` body across the boundary. The corpus grew by **1,849 words** in that record and the parity job's wall time did not measurably move.
- [x] **Done in 1b-4 (Task 4).** **No Rust test baked at the shipped pond parameters.** All six `ponds::tests` cases and `bake_tests::ponds_obey_their_keep_rule_and_name_a_river` pin spec §6.6's 3 km corridor deliberately, because what they assert is the search's mechanism; each is right on its own, and together they left `earth_like`'s 1,500 m and 1.6e10 unexercised in Rust, with 1b-3's `earth_like_ships_the_tuned_pond_corridor_and_density` pinning the constants but not what they do. Task 4 added the behavioural half: **`bake_tests::a_bake_at_the_shipped_pond_params_keeps_ponds`** (`src/hydrology/bake_tests.rs:1799`), which derives both parameters from `earth_like` rather than transcribing them, bakes `ranges_world()`, and asserts a pond survives with a ring of at least 3 points and a `Downstream::Reach`. It keeps 2 ponds of 14 found and runs in about 0.5 s, so it is an ordinary test rather than a sweep.

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
   - [x] **Lake outlines — DONE in 1b-4.** Ruled in 1b-3's Task 1 as **Candidate B**, replacing Ruling S-1, and **now measured and recorded**: a body's extent is an unordered set of shore points (its shore members then its collar) plus `shore_member_count`, `shore_reach_m` and its level, and `water_at` will decide by a nearest-point test rather than a polygon. Ponds keep the 250 m trace. On the owner's world all **348 coarse bodies carry an extent and none is missing a collar** — 5,800 shore members and 9,261 collar points, **240,976 bytes** against the design note's 228,976 estimate. The trim ships; **Ruling E-5 did not fire**. The argument is in `2026-09-12-water-1b3-outlines-design.md`, the measurement in `2026-09-12-water-1b4-verification.md`. **`water_at` itself is stage 2's (Ruling E-7)** — see "Routed to stage 2" below.
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

## Status after plan 1b-4 (2026-09-12)

Plan 1b-4 (body extents) puts a **body extent** on the wire and moves the record to **SCHEMA 6** (a 56-word header, a 16-word body prefix). See `2026-09-12-water-1b4-verification.md`. On the owner's world at 1M nodes, forced outlet 0°N 0°E, measured in the branch studio (wasm) on the owner's laptop at `27e0420`:

- the record is **7,326,056 bytes** (gate 8,000,000) and the bake takes **80 s** (gate 300 s);
- **348 of 348 coarse bodies carry an extent, and none is missing a collar** — 5,800 shore members and 9,261 collar points;
- the extent costs **240,976 bytes** of points, **306,064 bytes** with SCHEMA 6's fixed words, against the design note's 228,976-byte estimate — **5.2% low, and an estimate of the right thing**;
- `shore_reach_m` is **58,083 m median, 62,586 m max** — about one to two graph spacings;
- 4,067 bodies, 3,719 ponds of 55,742 found, crossings 43 → 37, capped 7 / 60 / 9, forced outlets 1 of 1, falls 0 — **every one identical to 1b-3**;
- 0 bed rises, 3,268 junctions all shared bit for bit, 0 mouths above their water, 0 fresh dead-ends;
- the great lake still runs 63 → 316 → 317 → 318 → 322 → 319 to the ocean at 25.938°S 30.106°W.

**The 80 s is not a speedup and must not be reported as one.** This host has measured the same world's wasm bake at 124 s, 242 s, 432 s and 441 s across plans 1b-2 and 1b-3. Bake time here is host-variable; the only thing to read from the 80 s is that the gate holds. The extent's own cost is under 0.05 s, measured natively where the readings are comparable.

**Ruling E-5 did not fire: the trim ships.** Task 3's trial held Ruling E-2's interior trim to account and it passed, so a body records only its shore members and its collar and no interior member at all. The untrimmed fallback would have cost roughly 890 KB against the 306 KB that shipped. **Task 3's result is the most interesting in the plan and is recorded in full in the verification report**: the trial first read **284 clause-1 misses of 74,904 samples**, and the re-run — filtered to samples genuinely over their own body's water — read **0**. The correction went to the design note's **§5.7 sample set**, not to §5.6's containment argument and not to Ruling E-2 (Ruling E-9 as revised).

**Ruling E-10:** `shore_reach_m` may not be narrowed to a percentile of the usable edges without re-running Task 3's trial first — not because the band holds interior points (clause 1 does) but because the trial samples the continuum only at graph-derived points, so a narrower band's effect on the shore contour between them is unmeasured.

### What plan 1b-3 routed here — all five closed

| Item | Status |
|---|---|
| Spec §8.2's index must list a body whose shore points come within `shore_reach_m` of a cell | **Spec written in 1b-4 (Task 5).** The dilation, its measured size against 50 km cells and why it is not a rounding allowance are all stated. **Building the index is stage 2's** (Ruling E-7) — item 1 below. |
| Spec §9 named `dilateBodyExtents` and the box-and-level rule | **Done in 1b-4 (Task 5).** §9's drawing path is rewritten against the shore-point set. |
| Spec §14's uses of "outline" were curve-sense | **Done in 1b-4 (Task 5).** §14.3, §14.6 and §14.9 restated for an unordered point set, or scoped explicitly to ponds. |
| The parity corpus must always compare a pond body | **Done in 1b-4 (Task 4).** `examples/parity_dump.rs:1949` — `HYDRO_PARAMS[0]` raised 12,000 → **20,000 nodes**, the smallest of three tried that keeps ponds. `hydro/plain` now bakes 13 bodies (**4 ponds, 9 coarse**) and carries a `shore_member_count == 0` body across the boundary; the record grew 4,223 → 6,072 words. |
| No Rust test bakes at the shipped pond parameters | **Done in 1b-4 (Task 4).** `src/hydrology/bake_tests.rs:1799` — `a_bake_at_the_shipped_pond_params_keeps_ponds`, deriving both parameters from `earth_like` and asserting a pond survives with a ring of at least 3 points and a `Downstream::Reach`. 2 ponds of 14 found, about 0.5 s. |

## Status after plan 2a (2026-09-12)

Plan 2a (the query) makes the baked record **answerable**. `water_at(point)` returns spec §8.3's kind, level, depth, body id and reach id, behind a spatial index built from the record, and it is exposed as `wb_water_at`, the per-tile batch `wb_water_tile` and a PyO3 `water_at`. **The record is unchanged** — no stage was added to `Surface`, `Surface` gained no field, and no existing output moved. See `2026-09-12-water-2a-verification.md`.

**On the owner's world at 1M nodes, forced outlet 0°N 0°E, measured in the branch studio (wasm) on the owner's laptop at `4af13ae`:**

- bake **77 s**, record **7,326,056 bytes**, **4,067 bodies** — every one identical to 1b-4;
- **every body answers its own id at its own anchor: 4,067 of 4,067**, with 0 wrong body, 0 answering `none` and 0 answering `ocean`;
- that sweep costs **308 ms for 4,067 queries — 76 µs each**; a 100×100 global grid is **10,000 points in 172 ms, 17 µs each** (3,542 `none`, 6,147 `ocean`, 299 `lake`, 12 `saltLake`);
- the great lake (body 63, 41.33M km²) answers **`lake`, body 63, level 0.00 m, depth 4,600 m**;
- a mid-leg point of reach 0 answers **`river`, `reachId` 0, level 35.17 m = `bed_m + depth_m` exactly** (Ruling Q-6);
- a 4×4 tile around the great lake agrees with **16 of 16** point queries on level and body id (Ruling Q-8: the batch does not interpolate and does not smooth).

**The anchor sweep is the load-bearing result, and Ruling Q-14 is why.** Every recorded outline point is on a shore by construction, so sampling the outline leaves an interior hole in the index invisible; the anchor is the only point that tests the interior. This is the world where that mattered — the great lake is **3,627 km across with a 58 km band**, and the shore-band index this plan started with answered `Ocean` over a region 1,700 km wide. **Ruling Q-13's bounding circle** (greatest anchor-to-recorded-point distance plus `shore_reach_m`, stated once in `index::body_circle_m`) is the fix, and 4,067 of 4,067 is what says it is complete.

**The 77 s is not a speedup.** This host has measured the same bake at 80, 124, 242, 432 and 441 s. Nothing in this plan touches the bake.

**Ruling Q-13's cost was feared and is measured.** Task 1's self-review flagged the circle as possibly unaffordable, because the owner world's `shore_reach_m` runs to 62,586 m against 50 km cells so most bodies dilate into their neighbours' cells. Over three 1,000,000-node stand-ins and a fixed 10,000-point area-uniform sample: **mean candidates per query 0.17 / 0.24 / 0.41 against §8.2's gate of 50**, and the **largest single cell in any of the three indexes holds 20 items** — so no query on those planets tests 50 candidates, let alone averages it. `DEFAULT_CELL_M` was not moved.

**Determinism (§14.1) is measured, not asserted:** **156,011 parity values compare bit-for-bit native against wasm, 0 divergent**, across a 32×32 grid group and two explicit point groups. The query and its index carry no platform libm and no map iteration order.

### Rulings made during plan 2a

The full table with "cost if wrong" is in the plan's `constraints.md`; these are the ones a later reader needs.

| Ruling | Decision |
|---|---|
| Q-1 | The index is the crate's own `BucketIndex` grid, **not** spec §8.2's "cube-sphere cell grid". §8.2's text corrected in Task 6. |
| Q-2 | The index is **derived state**, built from a decoded record, never recorded or transmitted; built on first query and cached beside the bake, freed with it. |
| Q-3 | The query reads the ground through a caller-supplied closure, and the callers pass **`Surface::structural_m`** — the landform the record's levels were written against. |
| Q-4 | **Ocean** is: the landform is at or below the datum **and** the point is inside no recorded body's extent. A below-datum basin the bake did not record answers `ocean`. |
| Q-5 | Precedence is §8.3's table order — ocean, body, river, none — with Ruling T1-3 deciding among bodies. |
| Q-6 | Depth: for a body, the level minus the landform, never below zero; for a river, the reach's own `depth_m` at the nearest recorded point, with `level_m = bed_m + depth_m`. |
| Q-7 | A reach's influence is a half-width band around each recorded segment, at the **larger** of the segment's two endpoints' `width_m`. |
| Q-10 / Q-12 | A body **claims** a point only when it is inside the extent **and** at or below the level; the **extent** suppresses the ocean and the **claim** decides which body answers. Two questions, two tests. |
| Q-11 | Where two reaches cover a point, the nearer centre line wins; ties to the lower reach id. |
| Q-13 | The index lists a body in every cell within its **bounding circle**; `index::body_circle_m` is the single statement of it. |
| Q-14 | The agreement property samples each body's **anchor**, not only its recorded points. |
| Q-15 | `decode` refuses a body with `shore_member_count > 0` and no collar (its `dc` is infinite and clause 1 would admit the planet). |
| Q-16 | The query reads the **landform** for coarse bodies, reaches and the ocean, and the **detail field** (`elevation_m` at `pond_cell_m`) for a fine-found body. Measured: 254 of 394 ring vertices stand above their own level against the landform. |
| Q-17 | A point exactly on a ring's vertex or edge is **inside** that ring, decided explicitly. Measured before it: 88 ring vertices were claimed by nothing. |
| Q-18 | `WaterAt` carries `reach_id` with a `NO_REACH` sentinel. **Supersedes Ruling Q-8's stride: the tile batch writes FIVE `f64` per sample**, not four. |
| Q-19 | A wrapper allocating a wasm buffer computes its size and its length word the same way the engine will, and refuses anything that would not survive the u32 boundary. |
| Q-20 | A bake is **not** tied to the world handle it was made from, and a mismatch is not refused; content, not identity, is the right key, and that is plan 2b's fingerprint. `hydroHold` returns the id and the handle together. |
| Q-21 | The parity group must put every kind the record holds on the wire, with a non-sentinel `reach_id`. Explicit points beside the grid, chosen from the record. |

### What stage 2's own list said, and what plan 2a closed

The three items "stage 2 must do first, in this order":

| Item | Status |
|---|---|
| **1. Build §8.2's index with the `shore_reach_m` dilation** | **Closed by plan 2a (Task 1)**, and the rule changed on the way: a band around the shore satisfies §8.3's clause 2 and nothing else, so **Ruling Q-13 widened it to the body's bounding circle**. Ruling Q-1 settled the grid's shape as the crate's `BucketIndex`. Measured affordable (above); §8.2 rewritten in Task 6. |
| **2. Implement §8.3's test, including the tie-break** | **Closed by plan 2a (Task 2)**, all five kinds, both clauses, the `shore_member_count` discriminator (Ruling E-8) and Ruling T1-3's tie-break. §14.9's "the query agrees with the record" is Task 3's property, re-run for the verification report. |
| **3. Carve from the record, not from a re-bake** | **Open — deliberately plan 2b's.** §8.1's carve is the one part of stage 2's water this plan did not touch. |

**The tie-break question 1b-4 left open — should a clause-1 claim beat a band claim? — was not pre-decided by plan 2a either.** Ruling T1-3 still decides on `dm` alone, and Rulings Q-10 and Q-12 answer the case that actually bit: a body's extent suppresses the ocean, but only a *claim* (inside the extent **and** at or below the level) picks the body. Dry ground below a perched lake's surface is therefore excluded by the level test, not by the tie-break. Whether the tie-break itself should prefer a clause-1 claim remains unanswered and unmeasured.

## Routed to plan 2b

Everything below is open. The first four were carried into plan 2a and deliberately left; the fifth is plan 2a's own.

1. **§8.1's carve.** A trapezoid cut to `bed_m` along each refined reach, `width_m` wide at the bank with banks blended over one width either side; notches cut the same way; lake beds **not** cut. **Carve from the record, not from a re-bake** — the record is the artefact and a second bake is a second answer. Expect a mouth's bed to be able to rise at the last step (Ruling R-4's I4 side effect).
2. **The detail damping.** The layer's authority — 1 inside a channel, falling to 0 at the blended bank — multiplies detail amplitude by `1 − authority`, exactly as `Features::apply` does, so texture cannot dam a river or raise an island in mid-channel. It ships with the carve or not at all: a carved channel with undamped detail is the failure the damping exists to prevent.
3. **The `hydrology` block and its fingerprint.** §7's block on the wire, and the content fingerprint that **Ruling Q-20 is waiting for**. Until it exists, a bake queried against a genuinely different world of the same radius answers that world's ground against this record's levels — a wrong answer, not an error — and nothing can check it, because nothing on the wire ties a record to a world. Both wasm exports and `engine.js` say so in as many words. This is the highest-value item on the list: it closes a correctness hole, not a cosmetic one.
4. **The `GENERATOR_VERSION` decision.** Untaken. §8.1's carve changes `elevation_m`, which is the thing the version exists to describe; the decision is whether that is a version bump and what it invalidates.
5. **The index's empty-header cost (new, from plan 2a Task 6).** The index occupies about **20 MB** at 50 km cells, of which **439 KB** is listed entries — **98% is empty per-cell `Vec` headers**: three families in `WaterIndex`, plus a fourth in the `BucketIndex` it never inserts into (`WaterIndex` never calls `BucketIndex::insert`; it pays for that grid's `buckets` field purely to address it). Measured per world: 14,665,104 header + 439,072 entry + 4,894,776 grid bytes on `plain`. **It is affordable today and it is the wrong container** — a `Vec<u32>` per cell for a structure that is 88% unoccupied; an offsets-plus-one-flat-`Vec` layout would cost roughly a tenth. Plan 2a did not touch it because re-laying out `index.rs` mid-plan would have invalidated every measurement in its verification report.

**Also open, carried forward unchanged from stage 2's list:**

- **`decode` accepts dangling links.** A validation pass is still owed before records are read from disk. (Plan 2a added two decode guards of its own — Ruling Q-15's missing collar, and 1b-4's `shore_member_count`/`shore_reach_m` pair — but not this.)
- **`reaches.rs`'s invariants** still want debug_asserts or doc comments.
- **A width-anchor ruling.** Spec §6.5 over-determines it: 3 m at the stream threshold and 1,000 m at the great threshold cannot both hold with a fixed exponent. The code anchors the stream end.
- **C1-b never takes back a fresh verdict** when a later cut reduces a pocket's inflow (monotone by design).

**Named by plan 2a, and NOT on plan 2b's list:**

- **`WaterKind::Pond` is not on the parity wire.** Neither parity bake records a body of kind `Pond` — it is an *area* classification, and at 20,000 nodes on `plain` and 60,000 on `ranges` every kept body is above the threshold. The **fine-found** branch (`shore_member_count == 0`, the one Ruling Q-16 reads the detail field for) is covered instead, guarded by body id rather than by kind, and `parity_dump.rs` prints the gap on stderr at every run. Closing it needs a parity bake at a node count that keeps a sub-threshold body, which would move the `H` records — so it is a deliberate non-item, not an oversight.
- **§8.2's performance target (`elevation_m` no more than 20% slower) is untested**, because plan 2a adds no stage to `Surface` and `elevation_m` is bit-identical. **It becomes live the moment plan 2b's carve lands**, and plan 2b owns measuring it.
- **`planet.py`'s oracle is dead code, and the spec said the opposite.** `evennia_roundtrip/planet.py` reaches for five `engine.*` names — `relief_canonical`, `tectonics_canonical`, `coast_canonical`, `surface_open`, `surface_elevation_by_handle` — and the built extension binds **none** of them, so `planet.elevation_at()` fails at `relief_canonical` before it ever reaches `surface_open`. Nothing in `tests/` calls it, which is why a 569-test suite is green over a function that cannot execute. `water_at` is now the first and only working Python door onto this world's water. **Building the `surface_open` family is not plan 2b's**; it is named in §8.3 so the next reader does not repeat the source-only `grep` and conclude that the binding merely needs re-exporting.
- **This host's bake timings wander by tens of percent** while every recorded quantity stays bit-identical (three stand-ins at 82.47 / 51.12 / 106.83 s against 1b-4's 46.05 / 26.95 / 56.87 s, essentially all of it in `ponds`). No cross-plan timing comparison is available without a controlled run.

**Still deferred to other projects, not to plan 2b:** Ruling S-15's meander on yielded segments, waterfalls (still 0 on the owner's world), and spec §6.6's two departures (the density cap at 1.6e10 against 5.0e8, the corridor at 1,500 m against 3,000 m) — all unchanged by plan 2a.

## Routed to stage 2 (superseded — see "Routed to plan 2b" above for the live list)

**Kept as history.** Plan 2a closed items 1 and 2 of the three below; item 3, the carve, is plan 2b's. The current open list is the "Routed to plan 2b" section above.


All six of plan 1a's "Plan 1b must fix (load-bearing for stage 2)" items are now closed, and plan 1b's four sub-plans are complete. What follows is everything still open anywhere in this file, gathered into one list so stage 2 has one place to read; the sections above are kept as the history of how each item was closed.

**Stage 2 must do these three first, in this order — nothing else in stage 2's water work can be built on top of a missing one:**

1. **Build spec §8.2's index with the `shore_reach_m` dilation.** A body must be listed in every cell its shore points come within `shore_reach_m` **of**, not only the cells they fall in. Measured, that distance is **58,083 m median and 62,586 m max** on the owner's world against 50 km cells — a whole ring of cells, not a rounding allowance. A cell that skips the dilation answers `none` over the shore band, which is exactly the water the band exists to hold. A pond's traced curve dilates by nothing (`shore_member_count == 0`, `shore_reach_m == 0`).
2. **Implement spec §8.3's test, including the tie-break.** Both clauses — `dm <= dc` (the interior, which Task 3 measured at 0 misses of 74,904 samples over their own water) **or** `dm <= shore_reach_m` (the shore band) — with the branch decided by `shore_member_count` and by nothing else (Ruling E-8), and **Ruling T1-3's tie-break: where more than one body claims a point the smaller `dm` wins, ties to the lower body id.** Without T1-3 the answer depends on the order §8.2's cell happens to list bodies in, because `shore_reach_m` is a per-body maximum and a ridge narrower than it can put a point inside two extents at once. `ocean` is decided first. **`water_at` itself is stage 2's to build (Ruling E-7)** — 1b-4 recorded the extent and tested the geometry that makes it answerable, and stopped there.
3. **Carve from the record, not from a re-bake.** The record is the artefact; a second bake is a second answer. Expect a mouth's bed to be able to rise at the last step (Ruling R-4's I4 side effect).

**Also routed to stage 2:**

- **A tie-break question Task 3 raised and 1b-4 deliberately did not pre-decide.** A point inside body A only via the band, and inside body B via clause 1, goes to whichever has the smaller `dm`; T1-3 does not distinguish the two clauses. Some band-only cases are dry ground below a perched lake's surface, which §8.3 already excludes by also requiring the point to be at or below the level — the extent is load-bearing, not a permissive gate around a level test. Whether the tie-break should prefer a clause-1 claim is stage 2's call.
- **`decode` accepts dangling links.** Add a validation pass before stage 2 reads records from disk.
- **The hydro half of `wasm.rs`** still waits for stage 2; `reaches.rs`'s invariants still want debug_asserts or doc comments.
- **A width-anchor ruling.** Spec §6.5 over-determines it: 3 m at the stream threshold and 1,000 m at the great threshold cannot both hold with a fixed exponent. The code anchors the stream end.
- **C1-b never takes back a fresh verdict** when a later cut reduces a pocket's inflow (monotone by design).

**Deferred to other projects, not to stage 2:**

- **Ruling S-15, the meander on yielded segments.** At 1M the crossing pass's first round sees 2,909 crossings, so that many segments ship straightened and unmeandered. Re-tuning `meander_amplitude_widths` waits for the mountains project's erosion — tuning an amplitude against terrain with no erosional valley to follow is tuning against the wrong ground.
- **Waterfalls.** Still 0 on the owner's world; its landform has no 10 m drop within 150 m along any reach at these resolutions. Falls await the mountains project's steeper relief. They work: 4 on the native `owner_survey` stand-in, plus analytic cliff tests.
- **Spec §6.6's two departures.** The density cap ships at 1.6e10 m² against the spec's 5.0e8, and the search corridor at 1,500 m against 3,000 m, both for the 1b-3 gates and both measured; the consequence is about ten times fewer ponds than §6.6's own parameters would place. A single bake at 5.0e8 and 3 km would replace that extrapolation with a count. 1b-4 did not revisit either, and its own gates left 673,944 bytes of the 8 MB record unspent.
