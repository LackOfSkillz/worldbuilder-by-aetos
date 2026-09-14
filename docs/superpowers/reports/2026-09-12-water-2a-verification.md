# Water 2a — verification of the query, on the owner's world

Plan 2a (automatic water, the query) does one thing: it makes the baked record **answerable**.
`water_at(point)` returns spec §8.3's kind, level, depth, body id and reach id, behind a spatial
index built from the record, and it is exposed at three boundaries — `wb_water_at`, the per-tile
batch `wb_water_tile`, and a PyO3 `water_at`. **The record itself is unchanged**: no stage was added
to `Surface`, `Surface` gained no field, and no existing output moved. §8.1's carve is deliberately
plan 2b's.

Every figure below names its **population**, its **method with parameters**, and its **host**.
Where a figure exists in an earlier report and in a run, **the run wins** — one number in this
plan's own Task 3 report was transcribed wrong and is corrected here, in public, for that reason.

**The two results worth reading first.** On the owner's world, **all 4,067 bodies answer their own
id at their own anchor** — 0 wrong body, 0 answering `none` or `ocean` — which is the property that
would have caught any hole in the index, and it is clean. And the index that makes it affordable
costs **0.17 to 0.41 candidates per query against a gate of 50**, while occupying **20 MB of
structure to hold 439 KB of entries**. The first is the plan's claim; the second is its bill, and it
is ledgered rather than hidden.

---

## The owner's world

**Population:** the owner's saved world, `worlds/world-1788998299904.json` (radius 9,309 km, land
0.4), opened from the studio library with its 2 painted features.

**Method:** the studio's bake at `PREVIEW_PARAMS` — `earth_like` at 1,000,000 total nodes — with one
forced outlet at the inland sea's centre (0°N 0°E), held on the main thread via `hydroHold`, then
queried through `wb_water_at` and `wb_water_tile`. The index is built on the first query and cached
beside the bake (Ruling Q-2), so the per-query costs below are amortised over a warm index.

**Host:** the branch studio, in wasm, on the owner's Windows laptop. Branch `water-query` at
`4af13ae`. **The controller took these measurements in the studio**; every other measurement in this
report is native, on the developer machine, and is labelled as such.

| | measured |
|---|---|
| Bake | **77 s** (gate 300 s: met) |
| Record | **7,326,056 bytes** (915,757 words; gate 8,000,000: met) |
| Bodies | **4,067** |
| **Every body at its own anchor** | **4,067 of 4,067 answer their own id** — 0 wrong body, 0 answering `none`, 0 answering `ocean` |
| That sweep's cost | 308 ms for 4,067 queries — **76 µs each** |
| A 100×100 global grid | 10,000 points in 172 ms — **17 µs each** |
| That grid's kind histogram | 3,542 `none`, 6,147 `ocean`, 299 `lake`, 12 `saltLake` |
| The great lake (body 63, 41.33M km²) | `lake`, body **63**, level 0.00 m, **depth 4,600 m** |
| A mid-leg point of reach 0 | `river`, `reachId` **0**, level **35.17 m = bed + depth exactly** |
| A 4×4 tile around the great lake | agrees with **16 of 16** point queries on level and body id |

**The anchor sweep is the load-bearing one, and Ruling Q-14 is why.** Every recorded outline point
is on a shore by construction, so sampling the outline would leave an interior hole in the index
invisible. The anchor is the only point that tests the interior — and this is the world where that
mattered: the great lake is **3,627 km across with a 58 km shore band**, so the shore-band index
this plan started with answered `Ocean` over a region 1,700 km wide. Ruling Q-13's bounding circle
is what fixed it, and **4,067 of 4,067** is what says the fix is complete on the world that broke it.

**The record and the bake are unchanged from plan 1b-4**, as they must be: 7,326,056 bytes and 4,067
bodies are 1b-4's own figures to the byte. The 77 s against 1b-4's 80 s is host noise on a machine
that has measured this same bake at 80, 124, 242, 432 and 441 s; **nothing in this plan touches the
bake, and no reading of 80 → 77 is available.**

**The grid found no river and no pond, and that is arithmetic rather than a gap.** A 100×100 grid on
a 9,309 km planet steps about 585 km; a reach is a few hundred metres wide. The reach probe and the
anchor sweep are what cover those branches, and the reach probe is the stronger evidence anyway
because it checks the identity `level_m = bed_m + depth_m` (Ruling Q-6) rather than merely finding
water.

**The tile agrees with the point query, 16 of 16.** Ruling Q-8 says the batch does not interpolate
and does not smooth: every sample is exactly the `wb_water_at` answer at that point. That is a test,
not a claim, and it is also a unit test in `wasm_exports.rs`; the studio measurement is the same
property on the real world.

---

## Task 3: the query agrees with the record (§14.9) — counts taken from a run

**Population:** three stock bake populations built by `water::query_tests` — `params` (8 bodies / 12
reaches / 7 notches), `junction_params` (20 / 165 / 3) and `ranges` (13 / 13 / 14) — plus a fourth
that is a re-bake of `junction_params` at a 5 km² pond threshold, whose only new figure is its pond
count. The distinct totals below are over the **three** stock populations; the fourth is excluded
because double-counting a re-bake would inflate every column.

**Method:** `cargo test -p worldbuilder-engine --lib
the_query_agrees_with_the_record_at_every_recorded_point -- --nocapture`, debug.

**Host:** the developer machine, Windows 11, rustc 1.98.0. **Re-run for this report at `4af13ae`;
27.52 s.**

| population | measured |
|---|---|
| **Shore members** | **142**, of which **0** stand above their own level (Ruling Q-12) |
| **Anchors** | **41**: **41 answer their own body**, 0 a neighbour (Ruling Q-14), 0 stand above their own level |
| **Ring vertices** | **226**: 85 their own body, **141 above their own level** (at most **1.284 m**, **0.420 m** mean), **0 claimed by nothing** (Ruling Q-17) |
| **Collar points** | **378**: 16 their own body, the rest another body or not water |
| **Reach points** | **4,283**: 4,047 their own reach *by `reach_id`*, **35** another reach (Ruling Q-11), **61** a body, **140** the sea at a mouth, **0** the sea midstream |
| **Leg midpoints** | **4,093**: 4,032 river, 39 a body, 22 the sea on a last leg |
| **Notch points** | **94**: 23 river, 66 dry, 5 sea, **0** a body |
| **Bodies of kind `Pond`** | **12**, across every population |

### The transcription that was wrong, and how it is known to be wrong

**Task 3's report states "51 a body" in its aggregate reach-point row. The run says 61.** The
per-population lines the same run printed are 10 (`params`) + 40 (`junction_params`) + 11 (`ranges`)
= **61**, and the report's own per-population lines already said so — only its summed row did not.

It does not need a re-run to be caught, either, and that is the useful part: the row must sum to its
own population.

```
  4,047 + 35 + 51 + 140  =  4,273     the report's row, against a stated 4,283
  4,047 + 35 + 61 + 140  =  4,283     the run
```

**Every figure in the table above is from the run quoted, not from any report.** This is the third
time this plan family has lost a number to transcription; the rule that stops it is not "check the
arithmetic", it is "do not copy a figure out of prose when the command that produced it is cheap".

### What the counts mean

- **41 of 41 anchors answer their own body, and 0 stand above their own level.** This is Ruling
  Q-14's property at bench scale and the owner-world sweep's 4,067 of 4,067 at planetary scale.
- **0 ring vertices are claimed by nothing.** Before Ruling Q-17 made an on-vertex/on-edge point
  explicitly inside, 88 were — the ray test's degeneracy, answered by leaving it to chance.
- **141 of 226 ring vertices stand above their own recorded level** against the landform, at most
  1.284 m and 0.420 m mean. That is not a defect: a ring *is* the shoreline contour, traced at
  250 m, so a vertex marginally proud of its own level is the trace's resolution. The claim that
  the two surfaces agree rests on the **magnitude** (worst 1.284 m) and not on the count.
- **0 reach points answer the sea midstream** in any population, and **0 notch points answer a
  body**. Both are zeros that were predicted before they were measured.
- **35 reach points answer another reach** (Ruling Q-11, a confluence naming one of two touching
  reaches) — neither 0, which would mean the tie-break never fires, nor a large fraction.

---

## The index: what it costs, and what it holds

**Population:** three native stand-in worlds at `HydroParams::earth_like(1_000_000)` — `plain`
(seed 20,260,904, 6.371 Mm, 12 plates, 0.29 land, no tectonics), `owner_survey` (562,423,712,
4.5 Mm, 28 plates, 0.16 land, `TectonicParams::ranges()`) and `seed1_ranges` (1, 6.371 Mm, 12
plates, 0.40 land, `ranges()`). `drainage_check` is `Ok` on all three.

**Method:** `./target/release/hydro_survey.exe`, no flags. The index is
`water::index::WaterIndex::build(&record, radius_m, DEFAULT_CELL_M)` on the record each run just
baked — **after `ponds::search`**, exactly as `wasm::with_water_query` builds it on a bake's first
query — timed with `std::time::Instant` around the `build` call alone. "Candidates" is
`bodies + reaches + notches` in the answering cell.

**The candidate sample is 10,000 area-uniform points**, SplitMix64 seeded at 20,260,912, drawn once
and used on every world: `lat = asin(2u − 1)`, **not** `180u − 90`. That distinction decides whether
the gate means anything — a lat/lon-uniform scatter crowds its points at the poles, where this
index's cells are emptiest, and would flatter the very mean it is being gated on.

**Host:** the developer machine, `cargo run --release --no-default-features`.

| world | cell realised | build | cells | occupied | largest cell | entries (body / reach / notch) | memory | mean candidates | max | gate |
|---|---|---|---|---|---|---|---|---|---|---|
| plain | 50,038 m | 0.091 s | 203,682 | 24,444 (12.00%) | **13** | 6,400 / 39,973 / 49 | 19,998,952 B | **0.2358** | 9 | **OK** |
| owner_survey | 50,132 m | 0.042 s | 101,374 | 7,829 (7.72%) | **20** | 3,223 / 14,362 / 59 | 9,887,576 B | **0.1686** | 20 | **OK** |
| seed1_ranges | 50,038 m | 0.115 s | 203,682 | 44,004 (21.60%) | **20** | 22,445 / 60,852 / 232 | 20,395,096 B | **0.4101** | 11 | **OK** |

**§8.2's gate — a mean candidate count under 50 — is met by more than two orders of magnitude, and
a stronger claim is available.** The **largest single cell in any of the three indexes holds 20
items**, so no query anywhere on any of these planets can test 50 candidates, let alone average it.
Task 1's self-review flagged Ruling Q-13's bounding circle as possibly unaffordable on the owner's
world, whose `shore_reach_m` runs to 62,586 m against 50 km cells so that most bodies dilate into
their neighbours' cells. **Measured, it is not.** `DEFAULT_CELL_M` was not touched and no lever was
pulled.

### 20 MB of structure holding 439 KB of entries

Nobody had measured the index's memory since Ruling Q-13 widened every body to its bounding circle.
The figure is a summed proxy from each `Vec`'s own `capacity()` — not an allocator sample, so
allocator rounding is not in it.

| world | headers | entries | grid | total | entries' share |
|---|---|---|---|---|---|
| plain | 14,665,104 | **439,072** | 4,894,776 | **19,998,952** (19.07 MiB) | **2.20%** |
| owner_survey | 7,298,928 | 151,152 | 2,437,496 | 9,887,576 (9.43 MiB) | **1.53%** |
| seed1_ranges | 14,665,104 | 835,216 | 4,894,776 | 20,395,096 (19.45 MiB) | **4.10%** |

`headers` is `3 × cells × size_of::<Vec<u32>>()` — the three per-cell `Vec`s, **paid in full on an
empty index before a single body is listed**. `grid` is the `BucketIndex`'s own, and nearly all of
it is a **fourth** set of empty per-cell `Vec` headers: `WaterIndex` never calls
`BucketIndex::insert`, so it pays for that grid's `buckets` field purely to address it.

**So 19.56 MB of the 20.0 MB is empty structure — 98% — and 439 KB is data.** Task 1's review
estimated "about 14 MB of empty `Vec` headers at 50 km cells"; the estimate was of the right thing
and low by the grid's own share.

**This is a measurement, not a worry, and it is ledgered for plan 2b.** It is affordable today —
derived state, never on the wire (Ruling Q-2), dropped when the bake is freed, 20 MiB beside a
4.3 MB record — and it is the wrong container: a `Vec<u32>` per cell for a structure that is 88%
unoccupied. An offsets-plus-one-flat-`Vec` layout would cost roughly a tenth of it. **Nothing in
plan 2a changes it**, because `index.rs`'s shape is Task 1's and re-laying it out mid-plan would
have invalidated every measurement in this section.

### A performance fix, and the proof it changed nothing

Task 1's review found `buckets::BucketIndex::sweep` computing `half_extent_deg` — about eight
transcendental calls — for **every row**, before the `everything` short-circuit and before the
`linear >= 180` test could use it. `sweep` is on the bake's hot path through `candidates` and
`nearest`, so a pole-crossing sweep paid for and discarded that work on every row it touched. Both
whole-row tests are decided without the exact term, and the pole test is loop-invariant; both are
now hoisted.

**Nothing observable changed, and that is measured rather than argued:** the parity corpus's
pre-existing 150,830 values are byte-for-byte unmoved, and
`refinement_adds_no_crossings_at_1m` printed **the same two lines it has printed since plan 1b-4** —
`ranges 1M: 5545 reaches, coarse 54 shipped 50` and `default 1M: 4285 reaches, coarse 33 shipped 28`.
**No speed-up figure is claimed**: `sweep` is reached from `LandGraph::sample`, `nearest` and the
pond search, and this host's bake timings wander by tens of percent between runs, so no honest
before/after is available. The argument for the change is that the work was computed and discarded.

### Bake timings wandered; nothing recorded moved

The three stand-in bakes measured **82.47 / 51.12 / 106.83 s** against plan 1b-4's 46.05 / 26.95 /
56.87 s on the same host, essentially all of it in `ponds` (60.63 / 28.58 / 81.11 against 34.37 /
15.25 / 43.69) — a part this plan does not touch. **Every recorded quantity is bit-identical to
1b-4's**: record bytes 4,281,864 / 2,511,928 / 6,098,912; ponds found/kept 1,993/273, 409/57,
3,752/504; coarse bodies 23 / 71 / 160; `shore_reach_m` largest 43,908.3 / 30,129.4 / 43,397.7 m.
Plan 1b-4 recorded the same effect at up to 9%; this is larger, and it is reported rather than
explained away. **Do not read a bake regression into it without a controlled run.**

---

## Parity: the query across the native/WASM boundary

Before plan 2a, `water::query` and `water::index` were **unfalsifiable** native against wasm rather
than merely unverified — the same sense in which `water.rs` was before `wb_water_run` and
`erosion.rs` before `wb_erosion_run`. Three groups now compare them.

**Population and method:** `examples/parity_dump.rs` writes every value natively as its 16-hex-digit
bit pattern; `parity/parity.mjs` replays the same inputs through the **shipped** `.wasm` and
compares bits. No decimal text is parsed anywhere. **Host:** the developer machine, Windows,
rustc 1.98.0 pinned, Node 22.

| group | compared | divergent | export | what it covers |
|---|---|---|---|---|
| `water_at/plain` | **5,121** | **0** | `wb_water_tile` | a fixed 32×32 grid: `none`, `Ocean`, `Lake` |
| `water_point/plain` | **30** | **0** | `wb_water_at` | `Lake`, `SaltLake`, `SaltFlat`, fine-found, `River` |
| `water_point/ranges` | **30** | **0** | `wb_water_at` | the same five, on the tectonic world's bake |

**5,121 = 1 + 32 × 32 × 5**, and the 5 is **Ruling Q-18**, not Ruling Q-8's superseded 4. Any later
note that computes this group at four words a sample is wrong on arithmetic before anything is
measured.

### The grid, and why it is a literal

**The box is 31 N, 5 W to 27 N, 1 W**, on the `plain` world's own `H plain` bake. It was chosen by
scanning that bake's record — every body anchor and every reach midpoint, at seventeen half-widths
from 0.05 to 8 degrees, a 32 × 32 histogram each — and scoring by how many kinds appear and how
large the smallest of them is.

| none | ocean | lake | salt lake | salt flat | pond | river |
|---|---|---|---|---|---|---|
| **692** | **213** | **119** | 0 | 0 | 0 | 0 |

— with **119 samples naming recorded body 2**, and no bucket under 11% of the grid.

It is a **literal** rather than a box derived from the record at run time, deliberately: a derived
box would follow the bake wherever it went, and the guards could never fire. A box entirely inside a
lake, or entirely on dry land, compares 1,024 copies of one answer and reports an agreement it never
tested.

### Ruling Q-21: the kinds a grid cannot reach

A fixed grid reaches only what lies under it. **`River` and the fine-found branch are the two the
drawing path uses most, and neither crossed the boundary at all** — a `reach_id` that is `NO_REACH`
at all 1,024 grid samples is a word compared 1,024 times without ever being a reach. So two point
groups joined the grid, through `wb_water_at` (the scalar export is what a caller asking about one
place uses; the grid already exercises the batch), **chosen from the record and not by hand**:

| chosen for | rule | why that point |
|---|---|---|
| `Lake`, `SaltLake`, `SaltFlat`, `Pond` | lowest-id body of that kind, at its `anchor` | Ruling Q-14: every recorded outline point is on a shore, so only the anchor tests the interior |
| fine-found | lowest-id body with `shore_member_count == 0` (Ruling E-8's discriminator), at its anchor | the branch Ruling Q-16 makes the query read the **detail field** for |
| `River` | **middle** recorded point of the lowest-id reach that answers `River` | the middle, not an end: a mouth sits at a shore where Ruling Q-5 hands the answer to the body, and this point exists to carry a real `reach_id` |

Measured, **both bakes cover Lake, SaltLake, SaltFlat, fine-found and River** — five points each,
5 × 6 × 2 = 60.

### `BodyKind::Pond` is not on the wire, and this is why

**Neither parity bake records a body of kind `Pond`, and that is stated rather than engineered
around.** `Pond` is an **area** classification — `pond_max_surface_area_m2` against a body's summed
surface area — not a statement about where the body was found. At **20,000 nodes on `plain`** (13
bodies) and **60,000 on `ranges`** (51 bodies) every kept body is above the threshold. The
1,000,000-node survey worlds above *do* record one (`plain`: 291 lakes, **1 pond**), but those are
not the parity bakes, and raising a parity node count to manufacture a `Pond` would move the `H`
records — which this plan's own constraints forbid.

What both bakes *do* hold is bodies the **fine pond search** found, and that is the branch worth
comparing: it is the one the query reads the **detail field** for (Ruling Q-16), and the only place
that field crosses the boundary at all. **Its guard is a body id, not a kind** — chosen for a
*branch*, the check that means something is that the query still answers *that body*; a kind check
would not fire, because these bodies are classified `Lake`. If the detail field stops reaching a
body's own anchor, the body-id assertion is what says so.

**So `WaterKind::Pond` is covered by unit tests alone in this corpus**, and
`examples/parity_dump.rs` prints that on stderr at every run rather than leaving it to be noticed:

```
WP coverage: NEITHER bake records a Pond; §8.3's Pond branch is not on the wire in this corpus
and is covered by unit tests alone
```

**Combined coverage, grid plus points:** `none`, `Ocean`, `Lake`, `SaltLake`, `SaltFlat`, `River`
and the fine-found branch — six of §8.3's seven kinds, and the seventh named.

### The guards that refuse the corpus

`examples/parity_dump.rs` refuses to write a corpus at all if the grid holds fewer than two kinds,
no `none` sample or no body sample; if any chosen point stops answering the kind it was chosen for;
if the fine-found point stops answering **its own body id**; if the `River` point's `reach_id` comes
back as the sentinel; if a record offers no point; if neither bake offers a `River` or a fine-found
point; or if the tectonic control's prediction below is 0 or all 30. **That is the part that makes
this stick as the bake moves underneath it**, and it is the same both-ends-refused discipline the
coast and gully controls carry — a guard that has already caught two boxes in this project's
history.

### The seventh `TCTL` field, and why the tectonic control needed one

**The tectonic control caught the new group, and the first run failed exactly as it should:**

```
FAIL: group water_point/ranges moved 2 values; the native side predicted 0
```

`--mutate tectonic-warp` sets `margin_warp_m` to 0 and touches nothing else. It reaches the terrain
the `ranges` bake runs over — that is why `hydro/ranges` moves 16,807 words under it — so a **query**
on that world must move too. `parity.mjs` requires every group's movement to equal a number the
**native** side computed, and an unlisted group's prediction is zero; a group that moves without a
prediction is a control that has stopped being one, so this was a refusal to fix rather than a
tolerance to widen.

The fix is a **seventh `TCTL` field**, computed by `water_points_divergence`: it bakes the **same
params** on the warp-0 world and asks the **same recorded coordinates** — replayed, never re-chosen,
because re-choosing from the warp-0 record would compare two different questions and call the
difference a divergence — then counts differing values by bit. The dump refuses to write a
prediction of 0 or of all 30. It measures **2 of 30**, and the control now prints *"exactly as the
native side predicted"*. `water_point/plain` has no entry, so its prediction is zero: the `plain`
world carries no tectonic block.

### What the controls say about the query

| control | divergent | `water_at/plain` | `water_point/plain` | `water_point/ranges` |
|---|---|---|---|---|
| `--mutate seed` | 147,387 | 2,073 of 5,121 | 20 of 30 | 20 of 30 |
| `--mutate erosion-k` | 216 | **0** | **0** | **0** |
| `--mutate water-pond` | 60 | **0** | **0** | **0** |
| `--mutate tectonic-warp` | 22,995 | **0** | **0** | 2 of 30 (predicted) |
| `--mutate coast-amplitude` | 13,128 | **0** | **0** | **0** |
| `--mutate gully-steer` | 3,752 | **0** | **0** | **0** |
| `--mutate climate-samples` | 648 | **0** | **0** | **0** |

**Those zeros are assertions, not absences.** The water, tectonic, coast, gully and climate controls
each check their own per-group prediction and exit 1 on a mismatch, so "the query is not downstream
of `erodibility_per_yr`, of `pond_max_surface_area_m2`, of `margin_warp_m`, of the coast amplitude,
of the steering lattice or of the upwind budget" is asserted six times over.

**The seed control's shapes are arithmetic rather than luck.** The grid moves **2,073 of 5,121 —
40.5%**, and both 99% and 0% would be findings: a moved seed is a different planet, so every sample
naming water moves, but a sample answering `none` writes the same five words on *both* planets (kind
0, level 0, depth 0, and the two sentinels), so most of the box's 692 dry samples compare equal. The
point groups move **20 of 30** each, which is 5 × 4 of 5 × 6: the **status** word is `WB_OK` on both
planets, and four of the five points answer a body, whose `reach_id` is `NO_REACH` on both.

---

## Every CI pin, old and new

Each re-derived by running it at `4af13ae`, never by hand arithmetic. `.github/workflows/gates.yml`
and the crate README mirror both carry dated notes (2026-09-12, plan 2a) giving old and new values.

### The wasm was stale at `8f8f998`, measured

Task 5 edited `src/` without rebuilding. With the working tree stashed, `npm run check:wasm` on the
unmodified branch reported `source now: b021c52a…` against `artifact built from: fc573f6e…` — the
branch's own provenance gate was red at HEAD and would have refused the parity job. The CRLF guard
printed nothing before each rebuild.

| | was | now |
|---|---|---|
| artifact | 461,474 B, `26284781…` | **461,498 B, `acf822df…`** |
| source fingerprint | `fc573f6e…` | **`6e81ebc6…`** (69 inputs) |

The artifact hash is unchanged across Ruling Q-21's own step, because only `examples/parity_dump.rs`
moved and an example is not compiled into the library; only the fingerprint moved. Both build
self-tests pass, and `check:wasm` reports the artifact matches its manifest and the source that is
here.

### Engine

**801/801/803/907/909 with 8 ignored → 841/841/843/953/955 with 9 ignored.** Listed totals
809/809/811/915/917 → **850/850/852/962/964**. Re-derived per configuration through `cargo test
-p worldbuilder-engine <cfg> -- --list` and the same with `--ignored`, cross-checked through
`assert_counts.py cargo-list` after the last source edit; all five printed `count OK` at 17 binaries.

| configuration | listed | ignored | run | was |
|---|---|---|---|---|
| `--no-default-features` | 850 | 9 | **841** | 801 |
| default | 850 | 9 | **841** | 801 |
| `--features python` | 852 | 9 | **843** | 803 |
| `--features wasm` | 962 | 9 | **953** | 907 |
| `--features python,wasm` | 964 | 9 | **955** | 909 |

All five suites run `--no-fail-fast`, **0 failed**. Per binary: lib **822 / 822 / 824 / 822 / 824**,
`blake2_bytes` 4, `build_fingerprint` 9, **`no_std_math` 6**, and on the wasm rows only
**`wasm_exports` 112** (was 106). So 822 + 4 + 9 + 6 = **841**, and 841 + 112 = **953**.

The +40 that lands uniformly is the query: `src/water/index.rs` and `src/water/query.rs` have their
test modules in `src/`, so every configuration sees them alike. The six extra on the wasm rows are
`wasm_exports.rs`'s tests for `wb_water_at` and `wb_water_tile`. **`expect_ignored` moves 8 → 9**,
the second time in that file's history: the ninth is
`water::query_tests::the_index_costs_what_it_costs_at_a_million_nodes`.

*A note on a figure in circulation:* **822/822/824/822/824 is the lib-only count, not the gate's.**
The gate pins the whole suite per configuration, which is what `assert_counts.py` reads. Both are
above so the two cannot be confused.

### Both ignored sweeps

- `hydrology::flow::tests::every_small_world_drains` — **passed**, 141.45 s then 106.24 s.
- `hydrology::bake_tests::refinement_adds_no_crossings_at_1m` — **passed**, 247.42 s then 179.80 s,
  printing `ranges 1M: 5545 reaches, coarse 54 shipped 50` and `default 1M: 4285 reaches, coarse 33
  shipped 28` both times — **identical to plan 1b-4's**, which is the `sweep` hoist's proof.

The third `#[ignore]`d test in this family, `the_index_costs_what_it_costs_at_a_million_nodes`, is
Task 1's own trial; the survey table above is that measurement on the shipping binary.

### Parity

| | compared | divergent | was |
|---|---|---|---|
| `parity` | **156,011** | **0** | 150,830 / 0 |
| `--mutate seed` | 156,011 | **147,387** | 145,274 |
| `--mutate erosion-k` | 156,011 | 216 | 216 |
| `--mutate water-pond` | 156,011 | 60 | 60 |
| `--mutate tectonic-warp` | 156,011 | **22,995** | 22,993 |
| `--mutate coast-amplitude` | 156,011 | 13,128 | 13,128 |
| `--mutate gully-steer` | 156,011 | 3,752 | 3,752 |
| `--mutate climate-samples` | 156,011 | 648 | 648 |

**The +5,181 is three new groups and nothing else moves by a value or by a word:** `water_at/plain`
5,121, `water_point/plain` 30, `water_point/ranges` 30. `hydro/plain` is still 6,072 and
`hydro/ranges` still 17,099, byte for byte, in the plain run and in all seven controls — which is
what the global constraint *"`elevation_m` must return the same bits; parity's existing groups must
not move"* asks of this plan, measured rather than asserted.

### Python

**565 / 157 → 569 / 161.** All four new tests are Task 5's PyO3 binding tests and all four are in
`tests/test_conformance.py`, so the whole-suite delta and the conformance delta are the same +4.
Re-derived by `pytest --collect-only -q tests` (**569**) and the same over
`tests/test_conformance.py` (**161**), then **run** in a fresh repo-local `.venv` (`pip install -e .
--no-deps`, `maturin develop --release --features python`, `WORLDBUILDER_REQUIRE_ENGINE=1`): **569
passed, 0 failed**.

Two environment properties, recorded rather than hidden:

- Against a **globally**-installed `worldbuilder_engine` older than Task 5, the same suite reports
  five failures — the four `water_at` conformance tests (that extension has no `water_at` binding)
  plus `test_installation.py::test_worldbuilder_importable_outside_the_repo`, the
  two-checkouts-sharing-one-interpreter property plan 1b-4's notes already record.
- **A corpus-only change invalidates the Python extension.** After Ruling Q-21 the suite failed at
  collection with `worldbuilder_engine is STALE`, because `examples/parity_dump.rs` is a
  fingerprinted input and the installed wheel carried the old fingerprint. That is Task 4's
  staleness guard doing its job; CI rebuilds the wheel every run and would never surface it.

### Viewer

`npm test` (Node's test runner, `viewer/`) — **346 pass, 0 fail** (was 340). The +6 is Task 4's, not
Task 6's; the suite was re-run twice as a gate.

---

## The spec corrections this plan made

- **§8.2's "fixed cube-sphere cell grid" is now the crate's `BucketIndex` grid** (Ruling Q-1), with
  Ruling Q-2's derived-state rule, Ruling Q-13's bounding circle, the measured candidate counts and
  the ledgered header cost.
- **§8.3 now states Rulings Q-3, Q-4, Q-5, Q-6, Q-7, Q-10 to Q-13, Q-16, Q-17, Q-18 and Q-20** where
  it describes the test, the signature and the exposure — including that `reach_id` widens the tile
  stride to five, superseding Ruling Q-8's four.
- **The stale `surface_open` sentence is replaced, and what replaces it is a finding.** See below.

### What `surface_open` turned out to be

The spec said *"The PyO3 work adds the missing `surface_open` family the Python oracle already
calls"*, and §2 said the oracle "calls a binding that is missing from this checkout's
`bindings.rs`". Task 5's reviewer found no `surface_open` anywhere in
`crates/worldbuilder-engine/src`, while `evennia_roundtrip/planet.py:196` calls
`engine.surface_open` — and asked what that `engine` is.

**It is the maturin-built extension and nothing else:** `planet.py:29` is
`import worldbuilder_engine as engine`. Counted from the module's own registration block
(`crates/worldbuilder-engine/src/lib.rs`, the `#[pymodule]` at line 218, at `b3e7026`), it binds
**71** names: 69 `wrap_pyfunction!` registrations plus the two exception classes
`UnknownSubstrateError` and `HydroBakeError`. They are the `surface_*`, `substrate_*`,
`plateset_*` and `features_*` families and, as of plan 2a, `water_at`. A `dir()` on a
built wheel returns one more than this; the registration block is the figure to trust, because
it is the list the extension is compiled from. **`planet.py` reaches for five names and the
extension binds none of them:**

| `planet.py` | called at | in the extension? |
|---|---|---|
| `engine.relief_canonical` | `planet.py:138` | **no** |
| `engine.tectonics_canonical` | `planet.py:139` | **no** |
| `engine.coast_canonical` | `planet.py:140` | **no** |
| `engine.surface_open` | `planet.py:196` | **no** |
| `engine.surface_elevation_by_handle` | `planet.py:205` | **no** |

So `planet.blocks()` fails at `relief_canonical` **before execution ever reaches `surface_open`**,
and `planet.elevation_at()` — "the oracle every placement tool has been missing", by its own
docstring — **has never run against this extension.** Confirmed by calling it: `AttributeError:
module 'worldbuilder_engine' has no attribute 'relief_canonical'`. **Nothing in `tests/` calls it**,
which is why a 569-test suite is green over a function that cannot execute.

**So the sentence was stale in the opposite direction from the one the brief assumed.** It is not
that `surface_open` already exists; it is that the whole oracle is dead code against the shipped
extension, and `water_at` is the first and only working Python door onto this world's water.
Building the `surface_open` family is not plan 2a's business and is **not** on plan 2b's list; it is
named in §8.3 so the next reader does not repeat the source-only `grep` and conclude the opposite.

---

## What plan 2a closed, and what it did not

**Closed.** §8.2's index (Ruling Q-1's shape), §8.3's query and all five kinds it can answer,
§8.3's tie-break (Ruling T1-3), the wasm export and the per-tile batch, the PyO3 binding, §14.9's
"the query agrees with the record", and §14.1's determinism — the last of these measured, not
asserted: **156,011 values compare bit-for-bit native against wasm, 0 divergent**, so the query and
its index carry no platform libm and no map iteration order.

**Deliberately not closed, and carried to plan 2b:** §8.1's carve, the detail damping, the
`hydrology` block and its fingerprint, the `GENERATOR_VERSION` decision, and — new from this plan —
**the index's empty-header cost**. The carry-forward has the list.

**Known gaps, named rather than left to be found:**

1. **`WaterKind::Pond` is not on the parity wire.** A property of what the two parity bakes classify
   at their node counts, not of the query; the fine-found branch is covered instead, and the dump
   says so at every run.
2. **§8.2's own performance target — `elevation_m` no more than 20% slower — is untested here**, and
   deliberately: this plan adds no stage to `Surface`, so `elevation_m` is bit-identical and there
   is nothing to slow down. It becomes live when plan 2b's carve lands.
3. **A bake queried against a different world of the same radius answers wrongly rather than
   erroring** (Ruling Q-20), until plan 2b's fingerprint gives content-based keying.
4. **This host's bake timings wander by tens of percent** while every recorded quantity is
   bit-identical. Any timing comparison across plans needs a controlled run.
