# Water 1b-1 — verification on the owner's world

**Population:** the owner's saved world, `worlds/world-1788998299904.json` (radius 9,309 km, land 0.4).

**Method:** the studio's pool worker, running `wb_hydro_bake` at `PREVIEW_PARAMS`: the `earth_like` values, with 1,000,000 total nodes and 20,000 wetness nodes. One forced outlet at the inland sea's centre (0°N 0°E), decoded by `water-preview.js::decodeHydro`.

**Host:** the branch studio on :8138, in wasm, on the owner's Windows laptop. Branch `water-refine` at c8cc993.

## Results against plan 1a (same world, same parameters)

| | 1a (after its final fix) | 1b-1 |
|---|---|---|
| Record size | 23.6 MB | **2.2 MB** (spec target 8 MB: met) |
| Bake time | 87 s | **43 s** |
| Schema | 2 | **3** |
| Hollows judged | 1,130 | 1,190 |
| Kept bodies | 343 (285 fresh, 58 salt) | **348** (296 fresh, 52 salt) |
| Drained | 787 | 842 |
| Recorded notches | 30,365 | **221** |
| Reaches | 4,933 (3,449 / 1,359 / 125) | 4,876 (3,420 / 1,319 / 137) |
| Top order, bifurcation | 5, 4.58–14 | 4, 4.74–15 |
| Largest lake above the datum | 344,503 km² | 344,503 km² |
| Fresh lakes with no way out (`downstream == Sink`) | not recorded | **0** |
| Forced outlets matched | not recorded | **1 of 1** |

**The great lake** (body 63, 41.33M km², fresh, forced, enclosed) drains in 7 reaches through five further fresh lakes: 1,146 km², 156,073 km², 26,288 km², 2,404 km² and 55,198 km². It reaches the **ocean at 26.13°S 30.13°W**, the same mouth as plan 1a. The path now picks up two small lakes 1a's raster-ordered paths passed by, because notch and outlet paths follow valley floors.

`drainage_check` passes in `bake_stages`, so the bake would otherwise have been refused.

## What each carry-forward item became

| Item | Status |
|---|---|
| Residual outlet-cut cycle | **Fixed** (Task 3). Cuts follow the flood tree. Evidence: 4.4M fuzz cases and 122 real bakes at Task 3; 44M fuzz cases and 122 real bakes at Task 4; 0 failures. The same fuzzer finds failures on the pre-fix code. |
| I2, raster-shaped paths | **Fixed** (Task 2). The flood breaks ties by ground height. |
| I3, capped basins lose inner lakes | **Fixed in mechanism** (Task 4): 79 inner lakes kept over 122 real 15k–40k worlds. **Not measurable on the owner world from the record**, because the record carries no "capped" flag. Hollows judged rose by 60 and kept bodies by 5, which is consistent with inner basins being judged, but not a count. Plan 1b-2 can add the statistic if it matters. |
| I5, lakes with no downstream link | **Fixed** (Task 5): every body carries `downstream`, and 0 fresh lakes dead-end. |
| Record size | **Fixed** (Task 6): 2.2 MB. |
| Spec §7 fields | **Fixed** (Task 6): body `downstream`, reach `fresh`, notch `width_m`, a params echo, and `forced_requested` / `forced_matched`. The body `override` is expressed by `forced`. |
| Silent forced-outlet misses | **Fixed** (Task 6): `forced_matched`. |
| `mod.rs` size | **Fixed** (Task 1): `mod.rs` 227 lines and `bake.rs` 730, both at 049d9c1 (the split); `bake.rs` has grown since with its tests. |

## CI and parity (Task 7)

- Engine pins: 707/707/709/813/815 at Task 7; 710/710/712/816/818 after the final review's fix wave (ad6f206). Both have 6 ignored (the new drainage sweep is `#[ignore]`d; run it with `cargo test --release --lib every_small_world_drains -- --ignored`, about 70 s).
- Parity: plain 0 divergent, and every control re-pinned.
- Python pin: 565.
