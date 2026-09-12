# Water 1b-4 — verification on the owner's world

Plan 1b-4 (automatic water, body extents) does one thing: it puts a **body extent** on the wire. Every kept coarse body now records its shore members and its collar, plus `shore_reach_m`, and `shore_member_count` says where the split is — which is also the discriminator spec §8.3 has needed since plan 1b-3's Task 1 replaced Ruling S-1 with Candidate B. Ponds are unchanged. The record moves to **SCHEMA 6** (a 56-word header, a 16-word body prefix). This report measures the result on the owner's world and on three native stand-ins, records Task 3's trial in full, and re-derives every CI pin.

**The most interesting result in this plan is Task 3's, and it is not the size figure.** The trial that was built to falsify the trim first appeared to falsify the design note's containment argument instead, and then — re-run against a corrected sample set — reported **zero** misses. Both numbers are below, with which is which.

## The owner's world

**Population:** the owner's saved world, `worlds/world-1788998299904.json` (radius 9,309 km, land 0.4), opened from the studio library with its 2 painted features.

**Method:** the studio's pool worker, running `wb_hydro_bake` at `PREVIEW_PARAMS` — the `earth_like` values with 1,000,000 total nodes and 20,000 wetness nodes — with one forced outlet at the inland sea's centre (0°N 0°E). The record was decoded by `water-preview.js::decodeHydro`. Wall time runs from the pool dispatch to the words arriving. The pond and refinement params are not wasm params, so the bake takes `earth_like`'s, unchanged from 1b-3.

**Host:** the branch studio, in wasm, on the owner's Windows laptop. Branch `water-extents` at `27e0420`. The controller took these measurements in the studio.

### Results against plan 1b-3 (same world, same parameters)

| | 1b-3 (c13d150) | **1b-4 (27e0420)** |
|---|---|---|
| Record size | 7,019,992 bytes | **7,326,056 bytes** (gate 8,000,000: met) |
| Wall time, pool dispatch to words | 242 s | **80 s** (gate 300 s: met — **but see the note below**) |
| Schema | 5 (54-word header) | **6 (56-word header, 16-word body prefix)** |
| Bodies | 4,067 = 348 coarse + 3,719 ponds | **4,067 = 348 coarse + 3,719 ponds** |
| **Coarse bodies with an extent** | — | **348 of 348, none missing a collar** |
| **Shore members / collar points** | — | **5,800 / 9,261** |
| **`shore_reach_m`, median / max** | — | **58,083 m / 62,586 m** |
| Ponds kept, of found | 3,719 of 55,742 | **3,719 of 55,742** |
| Crossings, coarse → shipped | 43 → 37 | **43 → 37** |
| Capped basins / inner / inner kept | 7 / 60 / 9 | **7 / 60 / 9** |
| Forced outlets matched | 1 of 1 | **1 of 1** |
| Waterfalls | 0 | **0** |
| Fresh lakes with no way out | 0 | **0** |
| Bed rises along any reach (§14.5) | 0 | **0** |
| Junctions, and those shared bit for bit (§14.4) | 3,268, all shared | **3,268, all shared** |
| Mouths whose bed is above their water (R-4) | 0 | **0** |

**The great lake** (body 63) still drains 63 → 316 → 317 → 318 → 322 → 319 to the **ocean at 25.938°S 30.106°W**, unchanged from 1b-2 and 1b-3. Nothing in this plan changes routing.

### The 80 s is not a speedup, and must not be read as one

This host has measured **the same world's wasm bake at 124 s, 242 s, 432 s and 441 s** across plans 1b-2 and 1b-3, on configurations that differ from each other by less than the spread between those readings does. Bake time here is **host-variable** — the browser, the page's settledness and whatever else the laptop is doing move it by a factor of three or more. This plan adds one pass over each body's members and removes nothing.

**The only thing to read from the 80 s is that the gate holds.** Any narrative built on 242 → 80 would be a narrative about the laptop.

The native stand-in timings below are the ones that carry information about the extent's cost, because they were taken by the same binary on the same host within minutes of each other — and they say the extent costs **under 0.05 s**: `record_of`, the stage `extent_of` runs inside, is unmoved at 0.03 / 0.02 / 0.04 s.

### The extent's cost, measured, against the 229 KB estimate

The design note's §5.2 estimated the trimmed extent at **14,311 points** on the painted bake, by scaling its 65-body probe's tail to 348 bodies. The measurement:

| | design note's estimate | **measured on the painted bake** |
|---|---|---|
| Recorded extent points | 14,311 | **15,061** (5,800 shore members + 9,261 collar) |
| Words (2 per point) | 28,622 | **30,122** |
| **Bytes** | **228,976** | **240,976** |

**The estimate was 5.2% low, and it was an estimate of the right thing.** Nothing downstream should now inherit 228,976; the figure is 240,976 bytes.

**A units note, because two figures are in circulation.** 240,976 bytes is **235.3 KiB** and **241.0 KB** decimal. The design note's "0.229 MB" is decimal, so the honest comparison is **241 KB measured against 229 KB estimated**. A "235 against 229" reading compares KiB with KB and understates the gap; both figures are given in bytes above so the comparison cannot slip.

**The whole record grew by exactly the extent, and that is checkable arithmetic rather than a fit:**

```
  7,326,056 - 7,019,992  =  306,064 bytes of growth
  extent points          =  2 x (5,800 + 9,261) x 8  =  240,976
  SCHEMA 6 fixed words   =  (2 + 2 x 4,067) x 8      =   65,088
                                                       ---------
                                                         306,064
```

The two new header words (`shore_members`, `collar_points`) and the two new per-body words (`shore_member_count`, `shore_reach_m`) account for the rest, to the byte, over all 4,067 bodies. **The record's size moved by exactly the extent** — which is what this identity shows and all it shows. A byte total is necessary but not sufficient for "nothing else changed"; the claim that no other content moved rests on the property checks and the parity run above, not on this arithmetic.

**Budget:** the plan had about 980 KB (7,019,992 of 8,000,000). It spent **306,064 bytes**, leaving **673,944 bytes** — 8.4% of the gate — for whatever comes next.

### Ruling E-5 did not fire: the trim ships

Ruling E-5 said that if Task 3's interior trial failed, the trim would be abandoned for **every** body and interior members recorded too — an estimated 890 KB on this world, leaving about 110 KB of headroom. **The trial passed. E-5 did not fire.** What ships is Ruling E-2's trimmed form: a body records its shore members (a member with at least one neighbour outside the body) and its collar (a non-member with at least one member neighbour, deduplicated), and no interior member at all.

The saving is the reason the plan fits: on this world the great lake alone has **37,844 members and 1,178 shore members**, a 32× reduction, and across the note's probe 39,469 members became 1,945. The untrimmed record would have cost roughly 890 KB against the 306 KB that shipped.

## Task 3: the trial, both numbers, and what was corrected

This is the plan's central result and it went two ways, so both are recorded.

**What the trial is.** The design note's §5.7 asked for a falsifiable test of the trim's premise. §8.3's inside test is a disjunction — a point is inside a body when `dm <= dc` (**clause 1**: nearer to a member of this body than to any of its collar) **or** `dm <= shore_reach_m` (**clause 2**, the band). §5.6's argument for the trim was that **clause 1 alone** holds a body's interior, because an interior point is nearer to some shore member than to any collar node — "the shore members surround it". If that were false, dropping interior members could open a hole inside a lake.

**Method:** `hydrology::extent_tests::the_trim_holds_at_a_million_nodes` and its smaller siblings, six populations, sampling *between* members and never at them (sampling at a member is trivially satisfied by that member being a recorded shore point). Bodies with no recorded collar are skipped — clause 1 is vacuous there.

### First reading: 284 clause-1 misses of 74,904 samples

| population | samples | outside clause 1 | worst clause-1 miss | **outside the extent** |
|---|---|---|---|---|
| `params` 12k | 57 | 1 | 441.2 m | **0** |
| `junction_params` 12k | 57 | 1 | 441.2 m | **0** |
| `ranges` 12k | 321 | 4 | 31,932.4 m | **0** |
| `ranges` 200k | 9,867 | 64 | 15,529.5 m | **0** |
| `1M plain` | 11,787 | 73 | 8,417.9 m | **0** |
| `1M ranges` | 52,815 | 141 | 16,286.1 m | **0** |
| **totals** | **74,904** | **284** (0.38%) | 31,932.4 m | **0** |

The last column was already decisive for the *record*: **the extent that §8.3 actually defines — clause 1 **or** clause 2 — held on every one of the 74,904 samples, at every node count, on every world.** The trim was never in danger. What appeared to be refuted was §5.6's *reasoning*: 284 samples were inside only via the band, and §5.6 credits clause 1 with the whole interior.

### Second reading, after the sample set was corrected: 0

The reviewer predicted the 284 were not interior water at all but samples the probe had placed **past their own body's shoreline** — over ground, not over the lake. The trial was re-run with every sample classified by its nearest node in the **whole graph** (the `BucketIndex` walk `hollows::nearest_forced_nodes` uses, built once per bake — a scan of the body's own recorded points would have answered a different question):

| population | samples | over own water | raw clause-1 misses | **misses over own water** | `samples − over own water` |
|---|---|---|---|---|---|
| `params` 12k | 57 | 56 | 1 | **0** | 1 |
| `junction_params` 12k | 57 | 56 | 1 | **0** | 1 |
| `ranges` 12k | 321 | 317 | 4 | **0** | 4 |
| `ranges` 200k | 9,867 | 9,803 | 64 | **0** | 64 |
| `1M plain` | 11,787 | 11,714 | 73 | **0** | 73 |
| `1M ranges` | 52,815 | 52,674 | 141 | **0** | 141 |
| **totals** | **74,904** | **74,620** | **284** | **0** | **284** |

**On samples that are genuinely over their own body's water, clause 1 misses 0 of 74,904.** And the last two columns are equal in **every** row — the set of clause-1 misses and the set of samples not over their own water are not merely nested, they coincide exactly. That is stronger than the prediction required.

### So which number is which, and what was corrected

- **284 of 74,904** is the count of **clause-1 misses over the probe's original sample set**, which includes samples that fall outside the body's own water. It is a fact about **§5.7's probe**.
- **0 of 74,904** is the count of clause-1 misses **over samples genuinely above their own body's water**. It is the fact about **the geometry**, and it is the one §5.6's argument makes a claim about.
- **0 outside the extent**, in both readings, on all six populations.

**The correction went to the design note's §5.7 sample set — to the probe — and not to §5.6's argument and not to Ruling E-2.** This is Ruling E-9 as revised: the trim ships **and** §5.6's containment argument stands; clause 1 alone does hold the interior. What §5.7 got wrong is which points it sampled, and its stated failure mode ("a point between two interior members") is not the one the numbers show. Task 5 edited §5.7 and nothing else; §5.6 and Ruling E-2 are provably untouched, which the Task 5 review verified hunk by hunk.

**Ruling E-5's own measurement has a witness, because "identical" and "the rebuild didn't take" look alike.** The first round's table reported only columns invariant under E-5. The trial now prints `trimmed_bodies` (bodies the trim actually dropped a member from — necessarily 0 under E-5) and `recorded_points`. Re-applying E-5 and re-running all six: `trimmed` went to **0 everywhere** and `recorded_points` grew by up to **119.4%** (`1M ranges` 7,132 → 15,649; `1M plain` 2,215 → 3,827; `ranges` 200k 2,733 → 4,045). The edit was demonstrably live, and the clause-1 miss counts did not move under it — the trim is not what those misses were about.

## `shore_reach_m`: its measured spread, and Ruling E-10

**What it is** (Ruling E-3): the **longest usable** member-to-collar edge of a body, where *usable* means the collar end's landform stands **above** the body's level. Edges whose collar end sits at or below the level are excluded — the design note measured them at 1.0–2.7% of shore edges, all of them dry ground downhill of a perched rim. For an **enclosed** body the level is the datum, so its usable edges are those whose collar stands above 0; that is not a special case (Ruling E-4) — E-3 reads `level_m`, which `hollows::find_hollows` already sets to 0.0 when enclosed.

**Measured spread**, over the coarse bodies of each bake (bodies with `shore_member_count > 0`; ponds are excluded because Ruling E-6 fixes theirs at 0.0):

| population | median | largest |
|---|---|---|
| **the owner's world** (radius 9,309 km, 348 coarse bodies) | **58,083 m** | **62,586 m** |
| `seed1_ranges` at 1M (radius 6,371 km, 160 coarse bodies) | 39,549 m | 43,398 m |
| `plain` at 1M (radius 6,371 km, 23 coarse bodies) | 39,403 m | 43,908 m |
| `owner_survey` at 1M (radius 4,500 km, 71 coarse bodies) | 27,187 m | 30,129 m |

**The band is about one graph spacing, and it scales with the planet, not with the lake.** Median and maximum sit within about 12% of each other on every population, and both track the world radius at a fixed 1,000,000 nodes — which is what a longest-edge statistic on a k-nearest graph should do. On the owner's world 62,586 m is roughly two graph spacings. This matters for §8.2: it is **tens of kilometres against 50 km cells**, so the index's dilation can add a whole ring of cells around a body and is not a rounding allowance.

**Ruling E-10: `shore_reach_m` may not be narrowed without re-running Task 3's trial first.** The design note's §5.4 floated taking a percentile of the usable edges instead of the maximum, which would shrink the band. That is now forbidden as a free change.

The *reason* is worth stating precisely, because it was revised once. It is **not** that the band holds interior points — it does not; clause 1 does, on 74,904 of 74,904 samples. It is that **Task 3's trial samples the continuum only at graph-derived points**, so what a narrower band would do to the **shore contour between them** is simply unmeasured. The band's job is to contain the level contour, and the argument that it does rests on `shore_reach_m` being the longest usable step. A percentile breaks that argument and no measurement replaces it. The cost of keeping the ruling is §5.4's bounded over-claim: the band may reach up to one graph spacing past the rim on at most 2.7% of a shore. That is written into spec §8.3 alongside the test.

## Native stand-ins: per-part timing, size, and the extent

**Population:** three worlds, each `Surface::new(seed, radius, plates, land, None, None, tectonics)` baked once with `HydroParams::earth_like(1_000_000)` and no forced outlets — `plain` (seed 20,260,904, 6,371,000 m, 12 plates, land 0.29, no tectonics); `owner_survey` (seed 562,423,712, 4,500,000 m, 28 plates, land 0.16, `TectonicParams::ranges()`); `seed1_ranges` (seed 1, 6,371,000 m, 12 plates, land 0.40, `ranges()`).

**Method:** `src/bin/hydro_survey.rs` (`cargo run --release --no-default-features --bin hydro_survey`), which times the four parts of `hydrology::bake` called exactly as `bake` calls them, and which this plan extended to print the extent. Record bytes are `record::encode(..).len() × 8`. Shore members and collar points are the record's own `BakeStats`. Each run is a single sample, except the timings, where two runs are reported because they disagree (below). **Gates: record ≤ 8,000,000 bytes, whole bake ≤ 120 s.**

The survey prints two failure figures rather than one, because the obvious one is blind. **Coarse bodies with no collar** counts coarse bodies whose `outline.len() == shore_member_count`. **Kept hollows with no extent at all** is `BakeStats::kept` minus the coarse bodies, and it exists because a kept hollow that recorded nothing has `shore_member_count == 0`, never enters the coarse population, and would leave the first figure printing a reassuring 0 for exactly the failure it is there to catch. It is signed, so a surprise in either direction shows instead of wrapping.

**Host:** the owner's Windows laptop, native release build, rustc 1.98.0 x86_64-pc-windows-msvc.

| | plain | owner_survey | seed1_ranges |
|---|---|---|---|
| `bake_stages` | 9.82 s | 10.87 s | 10.30 s |
| **`record_of`** (where `extent_of` runs) | **0.03 s** | **0.02 s** | **0.04 s** |
| `refine` | 1.83 s | 0.80 s | 2.84 s |
| `ponds::search` | 34.37 s | 15.25 s | 43.69 s |
| **Bake** (sum of the four) | **46.05 s** | **26.95 s** | **56.87 s** |
| **Record bytes** | **4,281,864** | **2,511,928** | **6,098,912** |
| — same, at 1b-3 | 4,241,672 | 2,461,400 | 5,974,160 |
| **Growth** | **+40,192** | **+50,528** | **+124,752** |
| Coarse bodies (all with a collar) | 23 | 71 | 160 |
| **Shore members / collar points** | **968 / 1,247** | **1,159 / 1,870** | **2,764 / 4,368** |
| Extent, points only | 35,440 B | 48,464 B | 114,112 B |
| **Extent, with SCHEMA 6's fixed words** | **40,192 B** | **50,528 B** | **124,752 B** |
| Extent as a share of the record | 0.94% | 2.01% | 2.05% |
| `shore_reach_m`, largest / median | 43,908.3 / 39,402.6 m | 30,129.4 / 27,186.9 m | 43,397.7 / 39,549.3 m |
| **Coarse bodies with no collar** | **0** | **0** | **0** |
| **Kept hollows with no extent at all** | **0** | **0** | **0** |
| Ponds found / kept | 1,993 / 273 | 409 / 57 | 3,752 / 504 |
| Crossings, coarse → shipped | 33 → 28 | 4 → 3 | 54 → 50 |
| Reach points, coarse → refined | 36,379 → 81,312 | 19,163 → 47,336 | 56,141 → 113,840 |
| Falls | 0 | 4 | 0 |
| Capped basins / inner / inner kept | 3 / 4 / 0 | 0 / 0 / 0 | 2 / 13 / 4 |
| Drainage | Ok | Ok | Ok |

**Both gates hold with room and no lever was pulled.** The worst record clears 8,000,000 by **1,901,088 bytes (23.8%)** and the worst bake is **56.87 s of 120 s**. `pond_density_area_m2` stays at 1.6e10 and `pond_search_radius_m` at 1,500 m; the plan's first size lever was never needed.

**Each record grew by exactly its own extent total** (+40,192 / +50,528 / +124,752, to the byte), which is the same identity the owner world's +306,064 satisfies. The accounting is not a model fitted to the growth; it is the growth.

**The extent's points half reproduces the design note's own per-world estimate to three decimal places.** §5.1's "B trimmed" column predicted 0.035 / 0.048 / 0.114 MB; measured, 0.0354 / 0.0485 / 0.1141 MB. That agreement on three worlds is what made the 5.2% miss on the owner world unsurprising rather than lucky.

**The extent costs no measurable time.** `record_of` is unmoved from 1b-3's 0.03 / 0.02 / 0.06 s. Whole-bake times are 6.1 / 2.3 / 3.7 s above 1b-3's, and the movement is in `ponds` and `bake_stages`, which this plan does not touch — on `plain` the pond part alone moved +4.84 s. Host variance, stated rather than absorbed.

**A second run of the same binary on the same three worlds puts a number on that variance.** Re-run minutes later with no change but an added printed figure, the bakes took **49.34 / 28.80 / 61.95 s** against the first run's 46.05 / 26.95 / 56.87 — **+3.3 / +1.9 / +5.1 s, up to 9%** — while every recorded quantity was **bit-identical**: the same record bytes, the same shore members and collar points, the same `shore_reach_m` to a tenth of a metre. That is the native version of the owner-world point above: on this host the timings wander and the record does not.

**Kept hollows with no extent at all is the figure that could have been non-zero**, and it is printed rather than argued. The no-collar count is blind to it — a kept hollow that recorded no extent has `shore_member_count == 0`, so it never enters the coarse population and the no-collar count would print a reassuring 0 for exactly the failure it exists to catch. `BakeStats::kept` minus the coarse bodies is the difference that sees it: 23 − 23, 71 − 71, 160 − 160.

**Plan 1b-3's Task 5 dedup fix moved no pond count anywhere.** Found/kept are 1,993/273, 409/57 and 3,752/504 on the stand-ins and **3,719 of 55,742 on the owner's world** — every one identical to 1b-3. The fix changes which candidate a refused neighbour suppresses; on these six populations it changed nothing that reaches the record.

## CI pins

Every figure was re-derived by **running** it on the host above, never by transcribing or by arithmetic. Each engine and parity figure was checked through `.github/scripts/assert_counts.py`, which printed `count OK`. The 1b-3 baseline was re-derived **before** the first edit of this task: the count gate at 786 reported `found 799`, so `gates.yml` was correct about the tree it was written against and the tree had moved under it.

| Pin | 1b-3 (c13d150) | **1b-4 (27e0420)** |
|---|---|---|
| Engine run, `--no-default-features` / default / `python` / `wasm` / `python,wasm` | 786/786/788/892/894 | **799/799/801/905/907** |
| Engine listed | 793/793/795/899/901 | **807/807/809/913/915** |
| Engine ignored | 7 | **8** |
| Engine test binaries | 17 | **17** |
| Parity, compared / divergent | 147,553 / 0 | **150,830 / 0** |
| `--mutate seed` | 141,765 | **145,274** |
| `--mutate erosion-k` | 216 | **216** |
| `--mutate water-pond` | 60 | **60** |
| `--mutate tectonic-warp` | 21,783 | **22,993** |
| Native TCTL prediction, `hydro/ranges` | 15,597 | **16,807** |
| `--mutate coast-amplitude` | 13,128 | **13,128** |
| `--mutate gully-steer` | 3,752 | **3,752** |
| `--mutate climate-samples` | 648 | **648** |
| Python (total / conformance) | 565 / 157 | **565 / 157** |
| Viewer `npm test` (not a CI pin) | 337 | **338** |

- **Engine: +13 run and +1 ignored, uniformly on every row.** Every one of the fourteen is in the crate library. **Thirteen are the extent** — the new `src/hydrology/extent.rs` (`Extent`, `extent_of`, Rulings E-1 to E-4) with its own unit tests, the SCHEMA 6 round trip and the `kind`/`shore_member_count` discriminator in `record.rs` and `bake_tests.rs`, the band property, Ruling E-6's pond invariant, and `src/hydrology/extent_tests.rs`. **The fourteenth is a pond test, not an extent test**: `bake_tests::a_bake_at_the_shipped_pond_params_keeps_ponds` (Task 4, `bake_tests.rs:1799`), which closes one of the two gaps plan 1b-3 routed here — see "What plan 1b-3 routed here" below. `tests/wasm_exports.rs` gained **no** test: its only edit is the header assertion, which SCHEMA 6 moves from 54 to 56 words. That is why the two wasm rows move by the same +13 as the other three rather than more.
- **`expect_ignored` moves 7 → 8, the first time in that file's history.** The eighth is `hydrology::extent_tests::the_trim_holds_at_a_million_nodes` — Task 3's trial, the measurement that decides Ruling E-5. It bakes at a million nodes and is `#[ignore]`d for exactly the reason `refinement_adds_no_crossings_at_1m` and `every_small_world_drains` are.
- **Parity: only the two hydro records move, and 0 divergent is the claim that matters.** The +3,277 is **two separate changes**, and running them together is how an earlier draft of this report got the arithmetic wrong:
  - **+1,428 is SCHEMA 6's extent** (Tasks 1 and 2), taking the branch 147,553 → **148,981**: `hydro/ranges` 15,945 → **17,099** (+1,154) and `hydro/plain` 3,949 → **4,223** (+274).
  - **+1,849 is Task 4's corpus change**, all in `hydro/plain`: 4,223 → **6,072**, from raising `examples/parity_dump.rs`'s `HYDRO_PARAMS[0]` from 12,000 to 20,000 nodes so that record keeps ponds again. A bigger bake, not a bigger body layout.

  **The `2 + 2 × bodies + 2 × (shore members + collar points)` identity applies to `hydro/ranges` alone**, the record Task 4 does not touch. It is always **even**, and `hydro/plain`'s total delta is **+2,123, odd** — precisely because it is +274 of extent plus +1,849 of extra bake. The other 46 groups are byte-for-byte unmoved in the plain run and under all seven controls. That the extent is bit-identical native and in the browser is what says `extent_of` carries no platform libm and no map iteration order — §14.1's determinism requirement, on the one path a native-only survey cannot reach.
- **The two controls that moved, moved for stated reasons.** The seed control gains +3,509 divergent against +3,277 compared; that span covers **both** causes above, not the extent alone, so it is not evidence about the extent's share on its own. What it does say about the extent is that its new words are seed-sensitive, as they must be — a different seed puts a different lake in a different place and every shore point moves. `hydro/ranges` is 17,047 of 17,099 and `hydro/plain` 6,019 of 6,072, the shortfall in each being the params echo a moved seed cannot move. The tectonic control's whole +1,210 is `hydro/ranges` (15,597 → 16,807 of 17,099) — the record Task 4 does not touch, so **this one is the extent's alone** — **its own native `TCTL` prediction moved with it**, and it again printed *"exactly as the native side predicted"*; the five belt groups are unchanged at 6,186, which says the extent adds no coupling of its own.
- **Wasm as shipped:** 431,385 bytes, 31 exports, 0 imports; artifact-sha256 `ab858cd2dfe0627f2725f3f0bc6bf302b79e15769c6fc23ea5741bdb56dac93a`, source-fingerprint `3eb47b9b74f1df1f1d905f49aaddaddff6e398df39161e6df74d5f5b66202d6d` (66 inputs). The artifact is byte-identical across Task 6's rebuild because a `[[bin]]` is not compiled into it; only the fingerprint moved. `npm run check:wasm` reports it matches its manifest and the source. The CRLF guard printed nothing before the rebuild.
- **The suites were run, all five:** 780 + 4 + 9 + 6 = **799** passed / 0 failed / 8 ignored under `--no-default-features` and default; 782 + 4 + 9 + 6 = **801** with `python`; +106 `wasm_exports` on the two wasm rows for **905** and **907**. `no_std_math` is **6/6** in every configuration — the determinism guard, which after Task 5 also scans `examples/`.
- **Both ignored sweeps pass.** `every_small_world_drains` in **76.31 s**; `refinement_adds_no_crossings_at_1m` in **128.18 s**, printing the same `ranges 1M: 5545 reaches, coarse 54 shipped 50` and `default 1M: 4285 reaches, coarse 33 shipped 28` as 1b-3 — so the extent moved no line.

## What plan 1b-3 routed here, and what closed it

Plan 1b-3 routed five items to this plan: three spec passages still describing the ring Candidate B replaced, and two coverage gaps that Rulings S-16 and S-17 opened. **All five closed in 1b-4.**

| routed item | closed by |
|---|---|
| Spec §8.2's index must list a body whose shore points come within `shore_reach_m` of a cell | **Task 5**, as spec text. §8.2 now states the dilation, its measured size against 50 km cells, and that a pond's curve dilates by nothing. **Building the index is stage 2's** (Ruling E-7). |
| Spec §9 named `dilateBodyExtents` and the box-and-level rule | **Task 5.** §9's drawing path is rewritten against the shore-point set. |
| Spec §14's uses of "outline" were curve-sense | **Task 5.** §14.3, §14.6 and §14.9 restated for an unordered point set, or scoped explicitly to ponds. |
| **The parity corpus must always compare a pond body** | **Task 4.** `examples/parity_dump.rs:1949` raises `HYDRO_PARAMS[0]` from 12,000 to **20,000 nodes** — the smallest of {20,000; 30,000; 50,000} tried that keeps ponds, and the closest to the original. `hydro/plain` now bakes 13 bodies (**4 ponds, 9 coarse**), so it carries a `shore_member_count == 0` body across the wasm boundary again. That record grew 4,223 → 6,072 words, which is the **+1,849** separated out in the pins above, and the parity job's wall time did not measurably move. |
| **No Rust test bakes at the shipped pond parameters** | **Task 4.** `src/hydrology/bake_tests.rs:1799`, `a_bake_at_the_shipped_pond_params_keeps_ponds`: it **derives** `pond_search_radius_m` and `pond_density_area_m2` from `earth_like` rather than transcribing them, bakes `ranges_world()`, and asserts a pond survives with a ring of at least 3 points and a `Downstream::Reach`. 2 ponds of 14 found, about 0.5 s — an ordinary test, not a sweep. It is the fourteenth of the engine suite's new tests and the only one that is not the extent's. |

**Why these two mattered.** Before Task 4, a drift in the shipped pond parameters would have been caught only by a parity count moving — which says *a* number changed, not which one or whether anyone meant it to — and the pond body layout crossed the wasm boundary compared on one record only. A later tuning that emptied `hydro/ranges` too would have left it compared on none, with no gate saying so.

## What stage 2 now has

Ruling E-7 draws the line: **this plan records the extent and tests the geometry that makes it answerable; `water_at` itself is stage 2's to build.** The query, §8.2's index and the carving are stage 2's, and the plan family's stages are the spec's. What stage 2 inherits, and can rely on:

**1. The discriminator, on the wire.** `Body.shore_member_count` is in SCHEMA 6 and in all four record twins (`record.rs`, `engine.js::hydroSummary`, `water-preview.js::decodeHydro`, `tests/wasm_exports.rs`). Before this plan the only working discriminator was `outline.length > 0`, which happened to be right only because coarse bodies shipped an empty outline. **Which branch §8.3 takes is decided by `shore_member_count` and by nothing else** (Ruling E-8): zero means a traced curve, any other value a shore-point set. `kind` describes the water — fresh or salt, pond-sized or lake-sized — not how its extent is written down, and Ruling S-11 is why the two do not always agree: a `lake` the §6.6 fine search recorded carries a traced curve like a pond. A coarse body of kind `Pond` correctly carries an extent.

**2. The extent itself.** Every kept coarse body records its shore members, then its collar, **each ascending by node index** (Ruling E-1), with `shore_member_count` marking the split. The order carries no geometry; it exists so §8.3's two clauses can read the two sets apart, and ascending node index is the tie-break the rest of the record already uses. Interior members are not recorded (Ruling E-2, the trim, which Task 3 held to account and E-5 did not overturn). Ponds keep `shore_member_count = 0` and `shore_reach_m = 0.0` and are unchanged (Ruling E-6).

**3. The band.** `shore_reach_m`, per body, the longest usable member-to-collar edge (Rulings E-3 and E-4), measured at 58,083 m median and 62,586 m max on the owner's world — about one to two graph spacings. It may not be narrowed without re-running Task 3's trial (Ruling E-10).

**4. §8.3's test, written down, including the tie-break.** For a body with shore members: let `dm` be the great-circle distance to the nearest **member** point and `dc` to the nearest **collar** point (ties to the lower outline index; no collar means `dc` is infinite). The point is inside if `dm <= dc` **or** `dm <= shore_reach_m` — clause 1 holds the interior (0 misses of 74,904 samples over their own water), clause 2 the shore band the level contour crosses. For a body with `shore_member_count == 0`: its outline is a traced curve, joined in order, **closing implicitly**, and `shore_reach_m` is not consulted. **The tie-break (Ruling T1-3): where more than one body claims the point, the smaller `dm` wins; ties go to the lower body id** — and a pond's curve test yields to a lake through the same rule, taking the pond's distance to its own nearest outline point as its `dm`. Without it the answer would depend on the order §8.2's cell happens to list bodies in, because `shore_reach_m` is a per-body maximum and a ridge narrower than it really can put a point inside two extents at once. `ocean` is decided first.

**Three things stage 2 must do before it can answer a query**, in this order — see the carry-forward for the full routed list:

1. **Build §8.2's index with the `shore_reach_m` dilation.** A body must be listed in every cell its shore points come within `shore_reach_m` **of**, not only the cells they fall in. At 58–63 km against 50 km cells that is a whole ring of cells, not a rounding allowance; a cell that skips it answers `none` over the shore band, which is exactly the water the band exists to hold. A pond's curve dilates by nothing.
2. **Implement §8.3's test including the tie-break.** Both clauses, the discriminator, and Ruling T1-3. A nearest-point test is only as good as the candidate set the index hands it, which is why (1) comes first.
3. **Carve from the record, not from a re-bake.** The record is the artefact; a second bake is a second answer.

**One thing to know before writing the query** (raised by Task 3 and left open deliberately): a point that is inside body A only via the band and inside body B via clause 1 goes to whichever has the smaller `dm`, and Ruling T1-3 does not distinguish the two clauses when it decides. Some of the band-only cases are dry ground below a perched lake's surface, which §8.3 already excludes by requiring the point to be *at or below the level* as well as inside the extent — the extent is not a permissive gate around a level test, it is load-bearing. Whether the tie-break should prefer a clause-1 claim over a band claim is a stage 2 question; it is named here rather than pre-decided.
