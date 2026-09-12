# Water 1b-3 — verification on the owner's world

Plan 1b-3 (shores) does three things: it stops the refinement introducing river crossings, it adds the fine pond search of spec §6.6, and it moves the record to SCHEMA 5 (a 54-word header). Lake outlines were spiked, ruled on, and routed to plan 1b-4. This report measures the result on the owner's world and on three native stand-ins, and re-derives every CI pin.

**Two of §6.6's own parameters were changed to meet the size and time gates. Both departures, their measured reasons and their consequences are stated in "Two departures from spec §6.6" below — they are the most important thing in this report.**

## The owner's world

**Population:** the owner's saved world, `worlds/world-1788998299904.json` (radius 9,309 km, land 0.4), opened from the studio library with its 2 painted features.

**Method:** the studio's pool worker, running `wb_hydro_bake` at `PREVIEW_PARAMS`: the `earth_like` values, with 1,000,000 total nodes and 20,000 wetness nodes. One forced outlet at the inland sea's centre (0°N 0°E). The record was decoded by `water-preview.js::decodeHydro`. The wall time runs from the pool dispatch to the words arriving. The pond and refinement params are not wasm params, so the bake takes `earth_like`'s: the crossing pass of Rulings S-2 to S-4 and S-14, and a fine search at 250 m cells, a **1,500 m** corridor and a **1.6e10 m²** density cap (see the departures below).

**Host:** the branch studio on :8138, in wasm, on the owner's Windows laptop. Branch `water-shores` at c13d150. The controller took these measurements in the studio.

### Results against plan 1b-2 (same world, same parameters)

| | 1b-2 (2db6a17) | **1b-3 (c13d150)** |
|---|---|---|
| Wall time, pool dispatch to words | 70.4 s | **242 s** (gate 300 s: met) |
| Record size | 5,958,896 bytes | **7,019,992 bytes** (gate 8,000,000: met) |
| Schema | 4 (43-word header) | **5 (54-word header)** |
| Bodies | 348 (296 fresh, 52 salt) | **4,067** = 348 coarse + **3,719 ponds**, of 55,742 found |
| Pond area p10 / p50 / p90 | — | **0.94 / 1.81 / 3.38 km²** |
| Pond depth p10 / p50 / p90 | — | **2.4 / 5.3 / 14.5 m** |
| Reaches (stream / river / great) | 4,876 (3,420 / 1,319 / 137) | **4,876 (3,420 / 1,319 / 137)** |
| Reach points | 117,169 | **114,941** |
| Crossings, coarse → shipped | not recorded | **43 → 37** |
| Waterfalls | 0 | **0** |
| Capped basins / inner / inner kept | 7 / 60 / 9 | **7 / 60 / 9** |
| Forced outlets matched | 1 of 1 | **1 of 1** |
| Fresh lakes with no way out | 0 | **0** |
| Bed rises along any reach (§14.5) | 0 | **0** |
| Junctions, and those shared bit for bit (§14.4) | 3,268, all shared | **3,268, all shared** |
| Mouths whose bed is above their water (R-4) | 0 | **0** |
| Ponds breaking Ruling S-5, or with a ring under 3 points | — | **0** |

**The great lake** (body 63) still drains 63 → 316 → 317 → 318 → 322 → 319 to the **ocean at 25.938°S 30.106°W**, unchanged from 1b-2. `drainage_check` passes in `bake_stages`; nothing in this plan changes routing.

### How the gates were reached, in order

Every figure below is a studio bake of the same world by the same method. This sequence is the argument for the two departures, so it is recorded rather than summarised.

| params | record | wall time | ponds kept |
|---|---|---|---|
| cap 4.0e9, corridor 3,000 m | 11,146,072 B — **fails** | — | 11,578 of 131,386 found |
| cap 1.6e10, corridor 3,000 m (Ruling S-16) | 8,430,792 B — **fails** | **441 s then 432 s** — fails | 5,184 of 131,386 |
| cap 1.6e10, corridor **1,500 m** (Ruling S-17) | **7,019,992 B** — passes | **242 s** — passes | **3,719 of 55,742** |

An earlier 124 s reading at the middle row did not reproduce on a settled page and is **discarded as an outlier**; 432–441 s is the figure that stands.

## Two departures from spec §6.6

Plan 1b-3 ships **two** of §6.6's parameters at values other than the ones the spec names. They were made for the size and time gates, they are measured, and together they change what a world looks like.

**1. The density cap is 1.6e10 m² against §6.6's 5.0e8 — 32× the spec's number** (one kept body per 16,000 km² of searched corridor, not per 500 km²).

*The measured reason:* at 5.0e8 the `seed1_ranges` stand-in alone recorded 9,020,112 bytes against the 8,000,000 gate. Raising in ×2 steps and measuring at each (5.0e8 → 9,020,112; 1.0e9 → 8,746,368; 2.0e9 → 8,357,824; 4.0e9 → 7,896,992) fitted the stand-ins at 4.0e9 — and then **the owner's world recorded 11,146,072 bytes at that value**, with ponds about 5.2 MB of it. One further doubling landed at 8,430,792, still over. Two doublings, to 1.6e10, is Ruling S-16.

**2. The search corridor is 1,500 m either side of a river against §6.6's 3,000 m.**

*The measured reason:* at 3,000 m with the cap already at 1.6e10, the owner's world took **441 s and then 432 s** against a 300 s gate, and still recorded 8,430,792 bytes. The corridor is the only lever that moves both gates: the search samples a lane `2 × radius` wide, so its cost is **linear** in the radius (measured on the stand-ins as 0.496 / 0.519 / 0.518 for a halving — linear to within 4%), and because a hollow must lie wholly inside the strip to survive Ruling S-10's side clip, a narrower lane drops candidates *faster* than it drops time (measured: to 0.213 / 0.196 / 0.239, about a fifth). This is Ruling S-17.

*Why not the other levers.* `pond_cell_m` stays at the spec's 250 m: it is the trace itself, coarsening it would make every recorded outline coarser, and Ruling S-13's containment result — no ring self-crossing and no cell outside its own ring, over 36,575 rings at 1M — is measured at 250 m and would have to be re-established. The keep rule is not the limiter: the owner world's median pond is 1.81 km² against a 0.05 km² floor.

**The consequence, stated plainly.** The owner's world ships **3,719 ponds**. Under §6.6's own two parameters it would have had **roughly twenty times as many** — the 3 km corridor found 131,386 candidates on that world where the 1.5 km corridor finds 55,742, and the spec's 500 km² cap would have kept a far larger share of them than 1.6e10 does. And **no pond is looked for more than 1.5 km from a river**: ponds away from the drainage network are not thinned, they are never searched for at all. Both are visible changes to how a world reads, made to fit an 8 MB record and a 300 s bake, and neither is a judgement that §6.6's numbers are wrong for their own sake.

## Ruling S-14: crossings are counted on the lines the record ships

Task 7's first survey measured the **shipped** record crossing *more* than the coarse record it came from — 36 against 33 coarse on the `plain` stand-in, 5 against 4, and 57 against 54. Ruling S-4 had put the crossing pass between tracing and the meander, so two stages were free to move a line after the last check had passed: a meander of up to 1.5 channel widths, and Douglas-Peucker at 500 m. Neither was ever looked at again.

**Ruling S-14** moves the pass to the end of the pipeline — trace, meander, simplify, **then** check — and repeats from there; `MAX_CROSSING_PASSES` went 3 → 4, which is itself a measurement (the descent is 2,909 → 1,204 → 118 → 55 → 50 on `seed1_ranges` at 1M; three passes stop at 55 against 54 coarse, over by one, and a fifth round moves nothing). Ruling S-4a still holds inside the loop: a segment that yields is straightened, skips the meander, and is simplified like any other.

**What the pass removed, on the record as it ships:**

| world | coarse crossings | **shipped** |
|---|---|---|
| the owner's world | 43 | **37** |
| `seed1_ranges` at 1M | 54 | **50** |
| `plain` at 1M | 33 | **28** |

By **Ruling S-2** the coarse crossings stay: a coarse crossing is a graph artifact — the two reaches' node paths really do cross — and fixing it means re-routing, which is out of scope. So the guarantee this plan ships is the one S-14 names: *refinement adds no crossing of its own*, and the shipped count is at or under the coarse count on every population measured. `hydrology::bake_tests::refinement_adds_no_crossings` holds it at 12k and 200k, and the `#[ignore]`d `refinement_adds_no_crossings_at_1m` holds it at the population where the failure was found.

## Ruling S-15: the meander's cost stands

At 1,000,000 nodes the crossing pass's **first** round sees **2,909 crossings** on `seed1_ranges`. Every one of those segments yields under Ruling S-4a, which means it **ships straightened and unmeandered** — it loses its valley-following for one coarse step, and it never gets its meander back, because a yielded segment skips that stage.

This is accepted as ruled. Re-tuning `meander_amplitude_widths` to reduce the collision rate **waits for the mountains project's erosion**: the meander's amplitude is currently 1.5 channel widths against terrain that has no erosional valley to follow, and tuning it against that terrain would be tuning against the wrong ground.

## Lake outlines: ruled in Task 1, implemented in plan 1b-4

Task 1 spiked spec §6.6's "lake outlines at 250 m" and **replaced Ruling S-1 with Candidate B**: a body's extent is recorded as an **unordered set of shore points** (the body's shore members, then its collar) plus `shore_member_count`, `shore_reach_m` and its level, and `water_at` decides by a nearest-point test rather than a polygon. Ponds keep the 250 m trace. The full argument is in `2026-09-12-water-1b3-outlines-design.md`.

The measurements that forced it: a ring cannot be built on a k-nearest graph with no faces — a minimum-turn edge walk closes on a 3-cycle covering 0.3–4.1% of the collar on 11 of 12 bodies measured, and the one substantial ring self-crossed 6 times; an angular sort closes but leaves up to 24.5% of a body's own members outside it. Containment is criterion 1 and both walks fail it. A true 250 m contour costs 2.4–8.3 MB a world (the great lake alone 1.5–3.3 MB) against a 1 MB budget. The shore-point set costs 0.073 MB on the owner world (an estimated 0.229 MB on the painted bake).

**Plan 1b-4 implements it.** Nothing in 1b-3 depends on which way Task 1 went.

## Native stand-ins: per-part timing and size

**Population:** three worlds, each `Surface::new(seed, radius, plates, land, None, None, tectonics)` baked once with `HydroParams::earth_like(1_000_000)` and no forced outlets — `plain` (seed 20,260,904, 6,371,000 m, 12 plates, land 0.29, no tectonics); `owner_survey` (seed 562,423,712, 4,500,000 m, 28 plates, land 0.16, `TectonicParams::ranges()`); `seed1_ranges` (seed 1, 6,371,000 m, 12 plates, land 0.40, `ranges()`).

**Method:** `src/bin/hydro_survey.rs` (`cargo run --release --no-default-features --bin hydro_survey`), which now times **four** parts of `hydrology::bake`, called exactly as `bake` calls them: `bake_stages`, `record_of`, `refine::refine`, and `ponds::search` with its own `ponds::pond_ground` inside the timed region. Record bytes are `record::encode(..).len() × 8`. Crossing and pond counts are the record's own `BakeStats`. Each run is a single sample.

**Host:** the owner's Windows laptop, native release build, rustc 1.98.0 x86_64-pc-windows-msvc.

### At the params as shipped (c13d150: cap 1.6e10, corridor 1,500 m)

| | plain | owner_survey | seed1_ranges |
|---|---|---|---|
| `bake_stages` | 8.88 s | 9.92 s | 9.71 s |
| `record_of` | 0.03 s | 0.02 s | 0.06 s |
| `refine` | 1.56 s | 0.75 s | 2.47 s |
| **`ponds::search`** | **29.53 s** | **13.97 s** | **40.90 s** |
| **Bake** (sum of the four) | **39.99 s** | **24.66 s** | **53.14 s** |
| **Record bytes** | **4,241,672** | **2,461,400** | **5,974,160** |
| Ponds found / kept | 1,993 / 273 | 409 / 57 | 3,752 / 504 |
| Crossings, coarse → shipped | 33 → 28 | 4 → 3 | 54 → 50 |
| Reach points, coarse → refined | 36,379 → 81,312 | 19,163 → 47,336 | 56,141 → 113,840 |
| Falls | 0 | 4 | 0 |
| Capped basins / inner / inner kept | 3 / 4 / 0 | 0 / 0 / 0 | 2 / 13 / 4 |
| Notch lines / points | 14 / 40 | 20 / 58 | 43 / 231 |
| Drainage | Ok | Ok | Ok |
| Bodies: lakes / ponds / salt lakes / salt flats | 291 / 1 / 4 / 0 | 105 / 0 / 17 / 6 | 591 / 3 / 64 / 6 |
| Reaches: stream / river / great, top order | 3,436 / 695 / 154, 5 | 2,211 / 576 / 34, 4 | 4,369 / 1,046 / 130, 4 |

The worst record clears 8,000,000 by 25.3%, and the worst pond search is 34% of its 120 s gate.

### The corridor at 3,000 m (the same worlds, one commit earlier)

| | plain | owner_survey | seed1_ranges |
|---|---|---|---|
| `ponds::search` | 59.56 s | 26.91 s | 79.02 s |
| Bake | 70.26 s | 37.73 s | 91.23 s |
| Record bytes | 4,934,968 | 2,650,888 | 6,966,736 |
| Ponds found / kept | 9,358 / 1,496 | 2,083 / 409 | 15,683 / 2,237 |

Reaches, reach points, crossings, notches, capped basins and drainage are **bit-identical** between the two tables: the corridor decides where the search looks and nothing else.

**The pond search is the dominant part of the bake** — 57–77% of native bake time at 1,500 m, and 71–87% at 3,000 m. It is what the 300 s owner-world gate turns on, and what the 242 s figure above is mostly made of.

### Ring properties at 1M (Ruling S-13)

`cargo run --release -p worldbuilder-engine --example pond_search_survey -- --stand-ins-1m`, at the 3,000 m corridor. Over **36,575 rings** across the three worlds: **0 self-crossing, 0 leaking a cell outside their own ring** (0 of 2,960,113 cells), and 11 refused (0.03%: neither the simplified ring nor the raw walk usable, so `search` records nothing for that candidate). No strip was skipped on any world — 0 over budget, 0 degenerate, 0 gated. On the owner's world the studio's own check found **0 ponds shipping a ring under 3 points**.

## CI pins

Every figure was re-derived by **running** it on the host above, in each of the four rounds this task went through, and never by transcribing. Each engine and parity figure was checked through `assert_counts.py`, which printed `count OK`.

| Pin | 1b-2 (2db6a17) | 1b-3 at cap 4.0e9 | at 1.6e10 | **1b-3 shipped (c13d150)** |
|---|---|---|---|---|
| Engine run, `--no-default-features` / default / `python` / `wasm` / `python,wasm` | 751/751/753/857/859 | 783/783/785/889/891 | 783/783/785/889/891 | **783/783/785/889/891** |
| Engine listed | 757/757/759/863/865 | 790/790/792/896/898 | 790/790/792/896/898 | **790/790/792/896/898** |
| Engine ignored | 6 | **7** | 7 | **7** |
| Engine test binaries | 16 | **17** | 17 | **17** |
| Parity, compared / divergent | 146,555 / 0 | 158,733 / 0 | 155,919 / 0 | **147,553 / 0** |
| `--mutate seed` | 140,818 | 152,886 | 150,097 | **141,765** |
| `--mutate erosion-k` | 216 | 216 | 216 | **216** |
| `--mutate water-pond` | 60 | 60 | 60 | **60** |
| `--mutate tectonic-warp` | 20,808 | 31,857 | 29,068 | **21,783** |
| Native TCTL prediction, `hydro/ranges` | 14,622 | 25,671 | 22,882 | **15,597** |
| `--mutate coast-amplitude` | 13,128 | 13,128 | 13,128 | **13,128** |
| `--mutate gully-steer` | 3,752 | 3,752 | 3,752 | **3,752** |
| `--mutate climate-samples` | 648 | 648 | 648 | **648** |
| Python (total / conformance) | 565 / 157 | 565 / 157 | 565 / 157 | **565 / 157** |
| Viewer `npm test` (not a CI pin) | 333 | 337 | 337 | **337** |

- **Engine: +32 on every row**, all from Tasks 1–6 — the crossing pass in `reaches.rs` and `bake_tests.rs`, the fine search in the new `ponds.rs`, and the SCHEMA 5 record words. `expect_ignored` moves 6 → 7 for Ruling S-14's `refinement_adds_no_crossings_at_1m`, and the binary count 16 → 17 because Task 1 restored `src/bin/shore_probe.rs` and a bin is a test target. The three tuning rulings (S-16, S-17) moved no count.
- **Parity: only the two hydro records move**, in every round. `hydro/plain` goes 3,938 → 5,001 → 5,001 → **3,949**, and `hydro/ranges` 14,958 → 26,073 → 23,259 → **15,945**. No H word diverges native against wasm in any round. The seed control's non-hydro groups stay at 122,208 throughout and the tectonic control's belt groups at 6,186; the tectonic control printed *"exactly as the native side predicted"* every time.
- **Wasm at c13d150:** 427,667 bytes, 31 exports, 0 imports; artifact-sha256 `e590b3b9c2fe9273272d140c1b85aab2877748282b58e2350f4563dfc8660454`, source-fingerprint `7669d18aa7962c27e370b2ceabc22c9086efbbe9f01f412f30e232f782a79dea` (58 inputs). `npm run check:wasm` reports it matches its manifest and the source.
- **Both ignored sweeps pass.** `every_small_world_drains` in 65.10 s; `refinement_adds_no_crossings_at_1m` in 114.81 s, printing `ranges 1M: 5545 reaches, coarse 54 shipped 50` and `default 1M: 4285 reaches, coarse 33 shipped 28` — agreeing with `hydro_survey` from a different code path.

### A concern about the corpus: `hydro/plain` now keeps no pond

`hydro/plain` ends this plan at **3,949 words**, which is exactly its pre-pond 3,938 plus SCHEMA 5's eleven new header words. **That parity world keeps zero ponds at a 1,500 m corridor.** So of the corpus's two hydro records, only **`hydro/ranges`** (15,945 words, about 976 of them ponds) exercises a pond **body** across the wasm boundary at all — the pond header words are still compared on both, and the seed control still moves `hydro/plain` (3,859 of 3,949, which is its pre-pond 3,857 of 3,938 plus the two header words a moved seed moves).

This is a real narrowing of coverage and it happened as a side effect of a tuning ruling, not by choice. It is not a defect today — one record with ponds is still one record with ponds, and it is the larger of the two — but a future tuning that also emptied `hydro/ranges` would leave the pond body layout crossing the boundary with **nothing comparing it**, and no gate would say so. Plan 1b-4 should either raise `parity_dump.rs`'s hydro populations, or give the corpus a third record chosen so it keeps ponds at the shipped params.

## The seven fixtures Ruling S-17 adapted

Halving the shipped corridor broke seven tests. None was added, removed, or weakened; each was adapted, and what each now exercises is named here so a reader can check that.

| fixture | what changed | what it exercises now |
|---|---|---|
| `ponds::tests::a_strip_covers_the_search_radius_at_the_cell_size` | via `params()` below | that a strip is `2 × radius / cell + 1` cells across — 25 at 3 km and 250 m |
| `ponds::tests::a_bowl_in_open_ground_is_flagged_neither_way` | via `params()` | that a bowl clear of both the strip's sides and its ends is flagged neither way |
| `ponds::tests::a_bowl_against_the_corridors_side_is_flagged_as_side_clipped` | via `params()` | that a bowl touching a long side sets `touches_side` (Ruling S-10's input) |
| `ponds::tests::a_side_clipped_candidate_is_counted_and_not_recorded` | via `params()` | Ruling S-10: counted in `ponds_found`, absent from the record |
| `ponds::tests::a_candidate_in_dry_ground_is_not_recorded` | via `params()` | Ruling S-6's wetness gate, and that the corridor still passed the strip gate |
| `ponds::tests::a_candidate_inside_a_coarse_lake_is_dropped` | via `params()` | Ruling S-7: a candidate whose nearest node is a coarse body member is dropped |
| `bake_tests::ponds_obey_their_keep_rule_and_name_a_river` | its own local `p.pond_search_radius_m = 3_000.0` | the keep rule, and Rulings S-5, S-7, S-11 and S-13 on a real 12,000-node bake |

The six `ponds::tests` share one change: **`ponds::tests::params()` now sets `pond_search_radius_m = 3_000.0` explicitly** rather than inheriting `earth_like`'s. Their bowls, offsets and expected cell counts were laid out against spec §6.6's corridor, and what they assert is the **search's mechanism** — not which corridor width ships. Inheriting a number that the size and time gates tune would break all six the next time it is tuned, and would say nothing about the mechanism when it did. `bake_tests::ponds_obey_their_keep_rule_and_name_a_river` needed the same for a different reason: at 1.5 km its 12,000-node world finds no hollow passing the keep rule, so every assertion in it would have run over an empty set.

An eighth, in the viewer, needed a different fix: **`viewer/test/water-preview.test.mjs`'s pond test now bakes at 50,000 nodes instead of 12,000.** A wasm bake always takes `earth_like`'s pond params — they are not on the wire — so that side cannot widen the corridor back the way the Rust fixtures do. The same `plain` world at 50,000 nodes keeps 19 ponds in about two seconds; every other test in the file stays on the 12,000-node bake it shares with `hydro.test.mjs`.

## What each carry-forward item became

| Item | Status |
|---|---|
| Reaches cross one another inside their corridors (1b-2 Ruling FF-3) | **Done** (Tasks 2, 3 and Ruling S-14). The pass now runs on the lines the record ships; 43 coarse against 37 shipped on the owner's world. Coarse crossings stay by Ruling S-2. |
| The fine layer: small lakes and ponds | **Done** (Tasks 4–6). 3,719 ponds on the owner's world, each with a traced 250 m ring, each naming a river by Ruling S-5. Two of §6.6's parameters were changed to fit the gates — see the departures above. |
| The fine layer: lake outlines | **Plan 1b-4.** Ruled in Task 1 as **Candidate B** (an unordered shore-point set with `shore_member_count` and `shore_reach_m`, and a nearest-point `water_at`), replacing Ruling S-1. |
| Falls | Still 0 on the owner's world; 4 on the native `owner_survey` stand-in. Unchanged by this plan; awaits the mountains project's relief. |
| The meander on yielded segments (Ruling S-15) | **Deferred as ruled.** 2,909 segments ship straightened and unmeandered at 1M; re-tuning waits for the mountains project's erosion. |
