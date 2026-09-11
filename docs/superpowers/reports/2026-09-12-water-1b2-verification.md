# Water 1b-2 — verification on the owner's world

Plan 1b-2 re-traces every coarse reach on the landform at 1.5 km, adds waterfalls and meanders, simplifies each refined line, and moves the record to SCHEMA 4. This report measures the result on the owner's world, and on three native stand-ins.

## The owner's world

**Population:** the owner's saved world, `worlds/world-1788998299904.json` (radius 9,309 km, land 0.4), opened from the studio library with its 2 painted features.

**Method:** the studio's pool worker, running `wb_hydro_bake` at `PREVIEW_PARAMS`: the `earth_like` values, with 1,000,000 total nodes and 20,000 wetness nodes. There is one forced outlet at the inland sea's centre (0°N 0°E). The record was decoded by `water-preview.js::decodeHydro`. The wall time runs from the pool dispatch to the words arriving. The refinement params are not wasm params (Ruling R-8), so the bake takes `earth_like`'s: 1.5 km steps, a 1 m vertical tolerance, falls of 10 m within 150 m, and meanders by Ruling R-6. The horizontal tolerance is **500 m** (see the size ruling below).

**Host:** the branch studio on :8138, in wasm, on the owner's Windows laptop. Branch `water-channels` at f34229f (and at dcb920b for the 250 m bake).

The controller took these measurements in the studio.

### Results against plan 1b-1 (same world, same parameters)

| | 1b-1 | 1b-2 at 250 m | **1b-2 at 500 m (final)** |
|---|---|---|---|
| Wall time, pool dispatch to words | 43 s | 65.3 s | **64.4 s** (spec target 300 s: met) |
| Record size | 2.2 MB | 8,659,856 bytes (**fails** the 8 MB gate) | **5,957,984 bytes** (spec target 8 MB: met) |
| Schema | 3 | 4 | **4** |
| Reach points | coarse (one per graph node) | 173,439 refined | **117,150** refined |
| Kept bodies | 348 (296 fresh, 52 salt) | 348 (296 / 52) | **348** (296 fresh, 52 salt) |
| Reaches (stream / river / great) | 4,876 (3,420 / 1,319 / 137) | same | **4,876** (3,420 / 1,319 / 137) |
| Waterfalls | not in the record | 0 | **0** |
| Capped basins / inner hollows / inner kept | not recorded | 7 / 60 / 9 | **7 / 60 / 9** |
| Fresh lakes with no way out (`downstream == Sink`) | 0 | 0 | **0** |
| Forced outlets matched | 1 of 1 | 1 of 1 | **1 of 1** |
| Bed rises along any reach (spec §14.5) | not checked | 0 | **0** |
| Junctions, and those shared bit for bit (spec §14.4) | not checked | 3,268, all shared | **3,268, all shared** |
| Mouths whose bed is above their water (Ruling R-4) | not checked | 0 | **0** |

At 250 m, reaches took 1,074,766 of the record's 1,082,482 words. The bake's routing is unchanged by refinement, so bodies, reach counts and classes, and the capped counts are the same at both tolerances. Only the reach geometry differs.

**The great lake** (body 63: 41.33M km², fresh, forced, enclosed) drains through the reaches and bodies 63 → 316 → 317 → 318 → 322 → 319 to the **ocean at 25.938°S 30.106°W**. In 1b-1 the coarse mouth was the ocean node at 26.13°S 30.13°W. The refined reach now ends at the first station on the shore (Ruling R-3: a terminal segment ends where the ground meets the water it runs into), short of that node.

`drainage_check` passes in `bake_stages`, which would otherwise have refused the bake. Refinement never changes routing.

## Rulings

- **The size gate failed at 250 m, so `refine_simplify_m` rose to 500 m.** The owner world baked 8,659,856 bytes at the planned 250 m, over the 8 MB target, although all three native stand-ins had fitted (the largest at 7.28 MB). The plan's rule (Task 8 Step 2) raises the tolerance from 250 to 500, then 1000, until the record fits. It fits at 500 m: 5,957,984 bytes, with 32% fewer refined points (173,439 → 117,150). The bake time barely moved (65.3 → 64.4 s), because simplification is a small part of it. `earth_like` now sets 500 m. The test that named a 400 m bend now scales its offsets with the tolerance. The plan's R-7 row names both values. Commit f34229f.
- **I3 closes.** Capped basins exist on the owner world (7), and their sub-floods keep inner lakes (9 of the 60 inner hollows they reveal). Carry-forward I3 said a capped giant basin lost every lake-worthy sub-basin inside it. Its ruling was to close on this count, from SCHEMA 4's header words 32–34.
- **Falls are 0 on the owner world.** Refinement traces the landform (`structural_m`, with no detail noise). At these resolutions, no reach on it drops 10 m within 150 m. Falls will come with the mountains project's steeper relief. The mechanism works on real relief: the native `owner_survey` stand-in (a 4.5 Mm `ranges()` world) bakes 4 falls. The analytic cliff tests in `refine.rs` guard the rest, with a mutation check at Task 5.

## Native stand-ins: per-part timing and size

**Population:** three worlds, each `Surface::new(seed, radius, plates, land, None, None, tectonics)` baked once with `HydroParams::earth_like(1_000_000)` and no forced outlets:

- `plain`: seed 20,260,904, 6,371,000 m, 12 plates, land 0.29, no tectonics;
- `owner_survey`: seed 562,423,712, 4,500,000 m, 28 plates, land 0.16, `TectonicParams::ranges()`;
- `seed1_ranges`: seed 1, 6,371,000 m, 12 plates, land 0.40, `ranges()`.

**Method:** `src/bin/hydro_survey.rs` (`cargo run --release --no-default-features --bin hydro_survey`). It times the three parts of `hydrology::bake`, called exactly as `bake` calls them: `bake_stages`, `record_of`, and `refine::refine` on a `Ground` built exactly as `bake` builds it. Record bytes are `record::encode(..).len() × 8`. A duplicate notch point (Ruling R-9) is a `(lat, lon)`, compared bit for bit, that appears in more than one notch line; each extra appearance costs 32 bytes. Each run is a single sample.

**Host:** the owner's Windows laptop, native release build, rustc 1.98.0 x86_64-pc-windows-msvc.

### At 500 m (`earth_like` as shipped, f34229f)

| | plain | owner_survey | seed1_ranges |
|---|---|---|---|
| `bake_stages` | 11.66 s | 12.34 s | 12.12 s |
| `record_of` | 0.04 s | 0.02 s | 0.06 s |
| `refine` | 1.35 s | 0.65 s | 2.07 s |
| **Bake** (sum of the three) | **13.05 s** | **13.02 s** | **14.24 s** |
| **Record bytes** | **4,227,936** | **2,471,408** | **5,966,824** |
| Reach points, coarse → refined | 36,379 → 82,993 | 19,163 → 47,979 | 56,141 → 117,298 |
| Falls | 0 | 4 | 0 |
| Capped basins / inner / inner kept | 3 / 4 / 0 | 0 / 0 / 0 | 2 / 13 / 4 |
| Notch lines / points | 14 / 40 | 20 / 58 | 43 / 231 |
| Duplicate notch points, keys / extra (bytes) | 0 / 0 (0) | 0 / 0 (0) | 4 / 4 (128) |
| Drainage | Ok | Ok | Ok |
| Hollows / kept / notched / closed | 80 / 23 / 57 / 4 | 142 / 71 / 71 / 23 | 292 / 160 / 132 / 70 |
| Bodies: lakes / salt lakes / salt flats | 19 / 4 / 0 | 48 / 17 / 6 | 90 / 64 / 6 |
| Reaches: stream / river / great, top order | 3,436 / 695 / 154, 5 | 2,211 / 576 / 34, 4 | 4,369 / 1,046 / 130, 4 |

### At 250 m (as planned, dcb920b)

| | plain | owner_survey | seed1_ranges |
|---|---|---|---|
| `bake_stages` / `record_of` / `refine` | 12.00 / 0.03 / 1.38 s | 12.85 / 0.02 / 0.66 s | 12.65 / 0.06 / 2.16 s |
| Bake | 13.41 s | 13.53 s | 14.88 s |
| Record bytes | 5,265,936 | 2,862,896 | 7,283,512 |
| Refined reach points | 104,618 | 56,135 | 144,729 |

Everything else in the 500 m table is the same at 250 m, because refinement does not change routing.

Refinement costs 5–15% of the native bake (at most 2.16 s, against a 120 s stop line), and the coarse stages dominate. The native bake takes 13–15 s here; the owner world's 64 s in the studio is wasm, on a larger world.

## What each carry-forward item became

| Item | Status |
|---|---|
| Notch lines without ends, joining non-neighbours | **Fixed** (Task 2, Ruling F-3a). A line splits where two consecutive points are not graph neighbours, and ends with the node its last lowered node drains into. |
| A finite 1e308 `stream_flow_m2` overflowing to infinity | **Fixed** (Task 1): `bake_stages` refuses a flow threshold above 1e20 m², or a `min_stream_nodes` above 1e7 (`HydroError::Params`). |
| `judge` rebuilding the forced index per basin | **Fixed** (Task 1): built once, passed in. |
| `bake.rs`'s tests in the same file | **Fixed** (Task 1): `bake_tests.rs`. |
| `HydroError::Drainage` sharing `WB_ERR_GRAPH` | **Fixed** (Task 1): `WB_ERR_DRAINAGE = 7`. |
| `parity.mjs` case `H` reading an unwritten `out_id` | **Fixed** (Task 1): a non-zero status is divergent, and `out_id` is not read. |
| `reaches_are_acyclic` in O(R²) | **Fixed** (Task 1): a colour walk, O(R). |
| Stale docs (calibration parity figures, the "all on the belt" step name, the TCTL field count, the threshold-floor comment) | **Fixed** (Task 1). |
| Doc comments for the unchecked assumptions | **Fixed** (Task 1) for the seven the plan named. The reaches.rs invariants were not in the plan and stay open. |
| I4: a mouth's bed rising at the last step | **Fixed** (Ruling R-4): a mouth's bed is `min(previous bed, water level)`. 0 mouths above their water on the owner world. |
| I3: capped basins lose their inner lakes | **Closed** (Tasks 3 and 8): 7 capped basins on the owner world keep 9 of 60 inner hollows. |
| The fine layer: tracing at 1.5 km, and waterfalls | **Done** (Tasks 4–7). 0 bed rises and 3,268 shared junctions on the owner world. Falls wait for steeper relief (see the rulings). |
| The fine layer: lake outlines, small lakes and ponds | **Plan 1b-3** (shores), which starts with a spike. |

## CI and parity (Task 8)

All figures were re-derived by running them on the host above, not by transcribing. Each engine and parity figure was checked through `assert_counts.py`, which printed `count OK`.

| Pin | 1b-1 (ad6f206) | 1b-2 at 250 m (dcb920b) | **1b-2 at 500 m (f34229f)** |
|---|---|---|---|
| Engine run, `--no-default-features` / default / `python` / `wasm` / `python,wasm` | 710/710/712/816/818 | 743/743/745/849/851 | **743/743/745/849/851** |
| Engine ignored | 6 | 6 | **6** |
| Parity, compared / divergent | 132,472 / 0 | 164,501 / 0 | **146,555 / 0** |
| `--mutate seed` | 126,737 | 158,765 | **140,818** |
| `--mutate erosion-k` | 216 | 216 | **216** |
| `--mutate water-pond` | 60 | 60 | **60** |
| `--mutate tectonic-warp` | 10,013 | 34,909 | **20,811** |
| Native TCTL prediction, `hydro/ranges` | 3,827 | 28,723 | **14,625** |
| `--mutate coast-amplitude` | 13,128 | 13,128 | **13,128** |
| `--mutate gully-steer` | 3,752 | 3,752 | **3,752** |
| `--mutate climate-samples` | 648 | 648 | **648** |
| Python (total / conformance) | 565 / 157 | 565 / 157 | **565 / 157** |
| Viewer `npm test` (not a CI pin) | 332 | 333 | **333** |

- **Engine: +33 on every row**, all in `src/hydrology/`. There are 12 new bake tests (in `bake_tests.rs`, which now holds all 35), 20 in `refine.rs`, and 1 in `routing.rs`. The 500 m ruling added and removed nothing.
- **Parity:** only the two hydro records move. `hydro/plain` grows from 647 words to 7,784 at 250 m, then 3,938 at 500 m. `hydro/ranges` grows from 4,166 to 29,058, then 14,958. No H word diverges native against wasm. The seed control's non-hydro groups stay at 122,208, and the tectonic control's belt groups stay at 6,186.
- **Wasm at f34229f:** 351,072 bytes, artifact-sha256 3e0a305014e7e38b4d8bd710804cf12cc9b4aad596be36fd5af4677aa73e8c98, source-fingerprint 72fa850d7fdaef7cdd13114327bcf5d975bcfb4799cbbfffd2bdf866fe252fc3 (55 inputs).
- **The ignored drainage sweep** (`cargo test --release -p worldbuilder-engine --lib every_small_world_drains -- --ignored`) passes: 83.8 s at 250 m, 81.9 s at 500 m.

## Re-measured after the final review's fix wave (2db6a17)

**Population:** the owner's world again, same method and host as above (the branch studio on :8138, the wasm pool worker, the world opened from the library with its two painted features, `PREVIEW_PARAMS` at 1M nodes, one forced outlet at 0°N 0°E). Measured by the controller.

| | at f34229f | at 2db6a17 |
|---|---|---|
| Wall time, dispatch to words | 64.4 s | **70.4 s** |
| Record bytes | 5,957,984 | **5,958,896** |
| Refined reach points | 117,150 | **117,169** |

Everything else is unchanged: 348 bodies (296 fresh, 52 salt); 4,876 reaches (3,420 / 1,319 / 137); 0 falls; capped basins 7, inner hollows 60, inner kept 9; forced 1 of 1; 0 fresh dead-ends; 0 rising beds; 3,268 junctions all shared bit for bit; 0 mouths above their water; and the great lake (body 63) still drains 63 → 316 → 317 → 318 → 322 → 319 to the ocean at 25.938°S 30.106°W. Every recorded fall's `at` is a point of its own reach with a lower point after it (0 falls here, so the check is vacuous on this world; `ranges_world` in the tests carries three).

The nineteen extra points and the 912 extra bytes are the fix wave's step-back rule (R-3a) moving a few blocked stations back toward their chord.
