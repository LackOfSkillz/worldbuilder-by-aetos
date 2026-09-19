# Water 2b — verification of the carve, on the owner's world

Plan 2b (automatic water, the carve) does one thing: it lets a baked record **cut the ground**.
Spec §8.1's water layer is a new, opt-in stage in `Surface` between features and detail. A river
channel and a notch are cut to the record's beds, a lake bed is left alone, and texture defers to a
channel. The layer is reached in two phases. First, bake the bare world. Then build a second world
from the same parameters, plus a water block, plus that record. A ground fingerprint in the
record's header joins the two. Only a record **baked for carving** can carve. That bake drains the
ponds its own channels cut through (SCHEMA 8). An absent block is bit-identical to the world before
the layer existed, so `GENERATOR_VERSION` stays 1.

Every figure below names its **population**, its **method with parameters**, and its **host**, and
every one was obtained by running for this report. Where a figure also appears in an earlier task
report, the run is the source and the earlier one is cited only to say whether it reproduced.
**Where a figure is explained, the explanation is marked as measured or as an inference.**

**Read these three results first.**

1. **Spec §8.2's performance target: missed as first built, and met natively after Ruling C-30.**
   As first built, the median native `elevation_m` on the owner's world rose **30–37%** (the
   median per-point ratio 27–30%), against a limit of 20%. **What dominated was measured, and it
   was neither of the two ledgered suspects.** It was the fixed index lookup that every sample
   pays, touched or not: about 150 ns of a ~630 ns sample, most of it one cache miss. It was not
   `inside_ring`, which runs on 0.02% of samples. It was not the empty `Vec` headers either: a
   packed table of the same cell count costs within 18 ns of the real index. **With a 54 KB
   bitmap of the cells that hold a channel (Ruling C-30), the native median rises 11–12%: a
   PASS.** Every carved elevation is bit-identical to before. **In wasm, the studio's host, the
   evidence is weaker and does not show a pass.** The paired per-sample harness reads 1.22–1.24,
   a miss by 2–4 points. What remains there is the arithmetic of finding the cell (`asin` and
   `atan2` in pure-Rust libm), not memory. See §1's last part.
2. **The inland sea survives the carve untouched.** The query answers it identically, all
   400,000 samples on both worlds, and its ground is bit-identical. It still drains by the same
   chain to the ocean at 25.938°S 30.106°W.
3. **The owner's pond figures reconcile exactly.** The studio's own `pondChange` gives **1,311
   drained and 903 arrived** at the owner's 86,000 wetness nodes, the same as Task 4b. At the
   studio's own `PREVIEW_PARAMS` (20,000 wetness) it gives **1,285 / 886**. That is also what the
   studio itself printed when it carved the owner's world in a browser for this report.

---

## Hosts, and how the owner's world was built

- **Native host:** the developer machine, Windows 11, x86_64, rustc 1.98.0 `x86_64-pc-windows-msvc`,
  release build. Timing uses `rdtsc` fenced with `lfence`. The TSC was calibrated against
  `Instant` at 2.4192 GHz; one timer pair costs 13.6 ns (median) and is included in every
  per-sample figure.
- **Wasm host:** the committed `viewer/public/wasm/worldbuilder_engine.wasm` (483,574 bytes, 39
  exports), under Node v22.17.0 on the same machine. Timing uses `process.hrtime.bigint()`. It
  costs 66.5 ns per call and its readings step by 100 ns on this host, so per-sample wasm figures
  are quantised. The tile figures are the reliable wasm ones.
- **The studio:** the branch studio (`node viewer/scripts/serve.mjs`) in the Chromium pane of the
  developer's desktop app.

**The owner's world was built from its own blocks, never from defaults.** The planet block of
`worlds/world-1788998299904.json` was run through the studio's own `reliefFromParams`,
`tectonicFromParams`, `coastFromParams`, `gullyFromParams` and `peakFromParams`, with engine
canonicals as the fallback, plus `HARBOUR` (2 features, because `harbour=1`). That gives:
- radius 9,309 km, 3 plates, land 0.4;
- relief `mountainM` 600, quieting 0.4, persistence 0.65;
- tectonics collision 3,400 m / 210 km, asymmetry 2.5, structure 0.5 at 80 km, wander 80 km at
  300 km;
- coast amplitude 0.35;
- gully and peaks canonical.

**The native probe did not transcribe any of that.** It wrapped `wb_world_new_water` under Node and
captured the **exact f64 words** the studio's marshaller passes. It then built the native world from
those words, through the same export compiled natively (`--features wasm`).

**Two checks say the two hosts built the same planet.**
- The native bake for carving from those words is **word-for-word identical** to the Node/wasm one
  (892,443 words).
- The native carved world was **accepted by `with_water`'s fingerprint check**. A world differing
  in any block would have been refused.

**Bake parameters:** `PREVIEW_PARAMS` (`earth_like` values, 1,000,000 total nodes) with one forced
outlet at 0°N 0°E. The wetness nodes are **86,000**, the world's own `lakeNodes`, unless a row says
20,000. **20,000 is what the studio's bake actually uses:** `lakeNodes` sets the older water solve's
graph, not the hydro bake. A bake **for carving** is the 13-word params buffer with word 12 = 1.

| Record (owner's world, Node/wasm) | words | bytes | schema | bodies | fine-found kept | bake wall time |
|---|---:|---:|---:|---:|---:|---:|
| ordinary, 86,000 wetness | 912,745 | 7,301,960 | 7 | 4,080 | 3,732 | 117.4 s |
| **for carving, 86,000 wetness** | **892,443** | **7,139,544** | **8** | **3,672** | **3,324** | 133.3 s |
| ordinary, 20,000 wetness (the studio's) | 915,761 | 7,326,088 | 7 | 4,067 | 3,719 | 94.7 s |
| for carving, 20,000 wetness (the studio's) | 895,839 | 7,166,712 | 8 | 3,668 | 3,320 | 113.2 s |

**How these records compare with earlier dumps** (`cmp` on the raw f64 files):
- The 86k ordinary record is **byte-identical** to Task 4b's dump at `7db8dd0`.
- The 86k carving record differs from Task 4b's drained dump at `ddb430d` in **exactly one byte**:
  word 0, 7.0 → 8.0, the carving flag Ruling C-20 added afterwards.
- The 20k ordinary record is plan 2a's 915,757 words plus Task 1's 4 fingerprint words.
- The native bake for carving took 80.2 s and 80.4 s on two runs.

Bake time is host-variable here and carries no claim.

---

## 1. Spec §8.2's performance target — missed as first built; met natively after Ruling C-30

> On the owner's world with its full record, the median cost of `elevation_m` rises by no more than
> 20% over the same world with no water.

**Population:** 200,000 area-uniform points over the whole planet (xorshift, fixed seed; 40.1%
land). All points are sampled at canonical resolution.

**Method.** Each point is evaluated on the bare world and on the carved world. The carved world is
the owner's world with the canonical water block (`bank_widths` 1) over the 86,000-wetness bake for
carving. Each pair is timed back to back, and the order alternates from point to point so that
neither world always runs second. The first 20,000 points are run untimed as a warm-up. Each run
makes two passes over the same points, and the program was run twice (four passes). Both handles
are driven through `wb_elevation_m`, so the handle lookup is common to both.

**Host:** native, as above.

| pass | median absent | median present | **ratio of medians** | **median per-point ratio** | ratio of means | p99 absent / present |
|---|---:|---:|---:|---:|---:|---:|
| run 1, pass 0 | 756 ns | 1,034 ns | **1.367** | **1.271** | 1.629 | 1,947 / 7,524 ns |
| run 1, pass 1 | 608 ns | 819 ns | **1.346** | **1.295** | 1.651 | 2,081 / 6,693 ns |
| run 2, pass 0 | 626 ns | 816 ns | **1.304** | **1.299** | 1.670 | 1,960 / 6,756 ns |
| run 2, pass 1 | 649 ns | 871 ns | **1.341** | **1.290** | 1.659 | 2,006 / 7,139 ns |

**The target is missed by 7 to 17 percentage points, depending on the statistic.** The median
rises 30–37% as a ratio of medians (10–17 points over) or 27–30% as a median of per-point ratios
(7–10 points over), against a limit of 20%. The absolute medians
drift between passes by up to 25% (host noise). The ratios drift by less than 7 points, and no
pass comes near the limit. **In wasm the miss is the same size.** The studio's host is below.

### Which cost dominates — measured

**Where the extra cost falls.** In run 2, the points split by what the index lists at them:

| subset (share of points) | median absent → present | ratio of medians | median per-point ratio |
|---|---|---:|---:|
| **no reach or notch listed in the cell** (89.2%) | 617 → 782 ns; 641 → 819 ns | **1.267; 1.277** | **1.272; 1.265** |
| a reach or notch listed (10.8%) | 686 → 3,022 ns; 718 → 3,179 ns | 4.40; 4.42 | 4.09; 4.11 |
| actually inside some leg's bank footprint (41 points, 0.02%) | 959 → 15,864 ns (run 1) | 16.5 | 17.5 |
| …and a body listed too (24 points) | 959 → 101,281 ns (run 1) | 106 | 119 |

**First finding, measured: the whole 20% budget is spent before the layer does anything.** On the
89.2% of points where the index lists no reach and no notch, the layer can only look up the cell and
return the ground unchanged. Even so, the median rises 27%. The layer's fixed cost is **165–178 ns
per sample** on a median sample of ~630 ns. The 10.8% of points in cells that list a channel then
pay the leg walk (about 2.3 µs each). Those points move the ratio of medians from ~1.27 up to
~1.30–1.37.

**Second finding: the lookup is that fixed cost, and it is a cache miss.** The measurement was
`WaterIndex::candidates`, timed straight after a whole bare `elevation_m` sample, the way the
carved world meets it. 200,000 points, two rounds, medians, timer included:

| what is timed after one elevation sample | median |
|---|---:|
| the empty timer | 27–28 ns |
| `BucketIndex::cell_of` alone (point → cell arithmetic, `to_latlon` included) | 65–67 ns |
| **`WaterIndex::candidates` (the real index)** | **176–179 ns** |
| `cell_of` + one read of a packed 24-byte row from a 434,626-row table (10.4 MB) | 155–158 ns |
| `cell_of` + one `u32` from a 434,626-entry table (1.7 MB) | 133–159 ns |
| `cell_of` + one bit of a 434,626-bit occupancy bitmap (54 KB) | 68–90 ns |

The same call in a **tight loop** tells the same story:
- 68–74 ns over random points;
- **22–29 ns** over points that stay in one 0.2° patch, so the cell stays in cache;
- `to_latlon` alone is 30 ns, and `cell_of` 37–38 ns.

In situ, where a whole elevation sample runs between lookups, the arithmetic costs ~40 ns. Almost
all of the remaining ~110 ns is **a miss into a per-cell table the size of the planet's cell
count**.

**So, of the two costs ledgered for this measurement:**

- **The index's empty `Vec` headers** (31.3 MB of 44.3 MB at this radius): **not what stands
  between the layer and the target.** A single packed offsets row per cell — the "offsets plus a
  flat `Vec`" design that would remove the headers — measured **155–158 ns**, against the real
  index's 176–179 ns. **About 18 ns would come back.** The miss is paid for touching *any*
  per-cell table of 434,626 cells, however it is laid out. The headers remain a **memory** cost:
  44.3 MB native per held bake.
- **`inside_ring`'s allocation and triple walk (Ruling Q-23):** **irrelevant to the median.** It
  runs only when a leg's bank reaches a point *and* a pond is a candidate there. That is **at most
  0.012% of samples** (24 of 200,000 had any body candidate while touched). For the touched
  minority, its measured share is ~3.7–4.1 µs of the ~53–55 µs extra per sample. What dominates
  there is the **coarse body test**, **45.7–49.5 µs**: `body_claim` scans every shore member and
  collar point of every coarse body listed, and the great lake alone records 2,835 points.

**What would plausibly meet the target — estimated here, then built and measured below.** A
per-cell occupancy bitmap would let `cut_with` return before touching the index at all. It needs
one bit per cell, set where any reach or notch is listed: 54 KB at this radius, small enough to
stay in cache. Read after an elevation sample, it cost 68–90 ns, against the index's 176–179 ns —
a saving of roughly 90–110 ns on the 89% of samples it would reject. The estimate was that this
would take the fixed overhead from ~27% to somewhere near 10% at the median. Ruling C-30 had it
built; the measured result is in "Ruling C-30" below.

**Attribution method, and its limit.** The layer's pieces were timed separately on the same
points, through the public API:
- `WaterIndex::candidates`;
- `WaterLayer::cut_m` over the full record;
- the same over copies of the record with no bodies, with coarse bodies only, and with ponds only.

Each copy has its **own** index, so its cache state differs from the real carved world's. That is
why the whole-planet means of those layers disagree with each other by ~100 ns. **Only the touched
subsets are read from that table.** The median finding rests on the in-situ lookup table and the
89.2% subset above, which do not depend on it.

### In the studio's host (wasm)

**Population and method:** the same design under Node through the committed wasm. 200,000
area-uniform points, two passes, over a carved world built with `engine.newWorld({…spec, water,
bake})` from a held 86,000-wetness bake for carving.

- **Per sample:** ratio of medians **1.286** and **1.308** (1,400 → 1,800 ns and 1,300 → 1,700 ns,
  on a 100 ns clock). The quantisation makes the per-point ratios unusable, so they are not
  reported.
- **Tiles**, as the studio fills them: `wb_fill_tile_f32`, 65 × 65 samples, resolution equal to
  the tile's own spacing, 150 tiles per row, order alternating.

| tile span | centred on | median absent → present | ratio of medians | ratio of means |
|---|---|---|---:|---:|
| 4° | area-uniform points | 2.45 → 3.28 ms | 1.338 | 1.402 |
| 4° | random recorded reach points | 3.67 → 9.27 ms | 2.526 | 2.784 |
| 0.5° | area-uniform points | 2.89 → 3.61 ms | 1.251 | 1.437 |
| 0.5° | random recorded reach points | 4.10 → 17.52 ms | 4.278 | 4.497 |
| 0.05° | area-uniform points | 3.20 → 3.79 ms | 1.185 | 1.415 |
| 0.05° | random recorded reach points | 3.99 → 18.11 ms | 4.542 | 4.598 |

**A tile wherever a river is in view costs 2.5–4.5× its bare cost.** Carved tiles are filled on
the main thread (Task 6), so this is the cost the owner feels as they fly along a river.

### Ruling C-30: a table of which cells hold a channel — the target re-measured

**What was built.** `WaterIndex` gains `channels`, one bit per cell (434,626 cells, 54,336 bytes
at the owner's radius). A bit is set **exactly** when that cell lists at least one reach or
notch. It is derived from the finished per-cell lists at the end of `build`, and nothing else
writes it. `WaterIndex::channel_candidates(point)` finds the cell once and returns `None` from
the bit alone when the cell lists no channel; otherwise it returns exactly what `candidates`
returns. `WaterLayer::cut_with` calls it first and returns the ground untouched on `None`. That
return is what the old code reached anyway with no channel candidate to visit (`!touched`), so
the change is a pure acceleration. The query (`water_at`) is untouched.

**Exactness, and the test seen failing.**
`water::index::tests::the_channel_bitmap_is_exactly_the_cells_that_list_a_channel` checks every
cell both ways, over the index fixture and over a real bake (`bake_tests::world()` at
`bake_tests::params()`). It also checks that no bit is set past the last cell, and that both
flagged and clear cells occur. `channel_candidates_is_candidates_where_a_channel_is_listed_and_none_elsewhere`
compares the two lookups at 20,000 points plus four on the fixture's own channels. Two
deliberate breakages were applied to `build` and reverted:
- **one set flag cleared** (the false negative that would leave a river uncut): the exactness
  test failed — `fixture: cell 51113's flag disagrees with its lists (0 reaches, 1 notches)` —
  and so did six layer tests (`a_notch_is_cut_the_same_way_to_its_own_cut_surface`,
  `the_carve_and_the_query_agree_about_where_the_channel_is`,
  `the_querys_water_surface_never_stands_below_the_carved_bed`,
  `no_detail_sample_rises_above_the_water_along_a_channel`,
  `a_carved_world_fingerprints_exactly_as_its_bare_parent`,
  `a_feature_over_a_channel_damps_detail_by_the_product_of_the_two_authorities`);
- **one clear flag set** (a false positive, which costs only speed): **only** the exactness test
  failed, `fixture: cell 0's flag disagrees with its lists (0 reaches, 0 notches)`. That is the
  case the layer tests cannot see, and the reason the exactness test exists.

**Output unchanged, measured three ways.**
- The native parity dump is **byte-identical** to the one taken before the change (`cmp`).
- Parity is **179,086 compared / 0 divergent**, and all eight controls are unmoved: seed
  170,363, erosion-k 216, water-pond 60, tectonic-warp 39,702, coast-amplitude 13,128,
  gully-steer 3,752, climate-samples 648, carve-bank 12. Each passed `assert_counts.py`.
- **On the owner's world** (Node, the old artifact against the new, each with its own held
  86,000-wetness bake for carving), 1,000,000 area-uniform points gave **0 carved elevations
  that differ**. So did **343,374 points** on every recorded reach point and 0.002° either side
  of it, where the cut is.

**§8.2 re-measured natively.** The same population, method and host as the table at the top of
this section: 200,000 points, two runs of two passes.

| pass | median absent | median present | **ratio of medians** | **median per-point ratio** | ratio of means |
|---|---:|---:|---:|---:|---:|
| run 1, pass 0 | 610 ns | 681 ns | **1.117** | **1.113** | 1.510 |
| run 1, pass 1 | 592 ns | 666 ns | **1.124** | **1.111** | 1.499 |
| run 2, pass 0 | 610 ns | 679 ns | **1.113** | **1.110** | 1.504 |
| run 2, pass 1 | 618 ns | 694 ns | **1.122** | **1.110** | 1.498 |

**PASS natively, with 8–9 points to spare.** On the 89.2% of points whose cell lists no channel,
the overhead fell from 27% to **8%** (ratio of medians 1.080–1.081, per-point 1.087–1.090). The
mean is barely changed (1.50 against 1.63–1.67), because it is carried by the 10.8% of points in
channel cells, which still pay the leg walk (ratio ~4.4 there, unchanged). **Two statistics
still fail, and they are reported rather than hidden:**
- **the land-only median** rises 39–51% (per-point 1.15). A land sample is far more often in a
  channel cell, and the 4× leg walk moves the land median;
- **the mean** rises about 50%.

The spec's target is the median over the world, which passes.

**§8.2 in the studio's host (wasm): not shown to pass.** Three measurements, and they do not
agree well enough to call it.
- **The paired per-sample harness** (as above, `process.hrtime.bigint()` around each call,
  100 ns steps), read by the grouped median, which interpolates inside the 100 ns step that holds
  the median:
  - the old artifact read **1.303 and 1.249**;
  - the new artifact read **1.235 and 1.220**.

  That is a miss by 2–4 points. The raw step-floor medians (1,300 → 1,500 ns) give 1.154, but
  with two clock steps between the numbers that is not a measurement.
- **A four-way interleaved harness** (old bare, old carved, new bare and new carved, all per
  point in one process) read the old artifact at **0.99–1.06** and the new at **0.93–0.99**. In
  that harness the other instances' calls evict the cache for the bare world too, so it
  measures a different thing. It is recorded to show how far the harness moves the answer.
- **A clock-free loop inside wasm** (a scratch `cdylib` over the same engine, timing 200,000
  `wb_elevation_m` calls per export call, 12 rounds, the median round), over the points whose
  cell lists no channel, which is the population holding the median sample:
  - the old engine read **1.16 and 1.14**;
  - the new engine read **1.12, 1.10 and 1.14**.

  Round-to-round spread is ±10–15%.

**What dominates in wasm now — measured with the same clock-free loop.** Each mode adds one
operation to a bare `elevation_m` sample; the figures are extra ns per sample, as the median of
16 rounds:

| added operation | extra ns per sample |
|---|---:|
| `to_latlon` alone | +151 |
| `BucketIndex::cell_of` (`to_latlon` + row/column) | +210 |
| `channel_candidates` (the bitmap path) | +256 |
| `candidates` (the old path) | +466 |
| a whole carved sample over bare, all points (includes the leg walk) | +506 |

**In wasm the lookup is arithmetic-bound, not memory-bound.** The bitmap removed ~210 ns of
list access. The ~210 ns left is the point-to-cell arithmetic, and ~150 ns of that is
`to_latlon`'s `asin` and `atan2` in the pure-Rust libm that determinism requires. Natively the
same arithmetic is ~40 ns, which is why the native host passes and wasm is marginal. **What
could still help in wasm (an inference):** a key for the bitmap that avoids `atan2`, for example
row from `z` directly. That is a second grid that must be proved conservative against this one,
so it is not a small change, and it is not built.

---

## 2. The decoded record's size, measured

**Method:** a counting global allocator (live bytes across alloc, dealloc and realloc) around
`hydrology::record::decode`, then around `IndexedRecord::new` at 9,309,000 m and 50 km cells. The
program is Task 5's `memprobe`, run on this report's four records.

**Host:** native, 64-bit.

| record | wire | **decoded `HydroRecord`** | `WaterIndex` | projected points | **held bake: wire + `IndexedRecord`** |
|---|---:|---:|---:|---:|---:|
| **for carving, 86k** | 7,139,544 B | **7,015,808 B** | 44,319,944 B | 2,883,696 B | **61,358,992 B** |
| ordinary, 86k | 7,301,960 B | 7,168,432 B | 44,324,328 B | 2,883,696 B | 61,678,416 B |
| for carving, 20k | 7,166,712 B | 7,042,792 B | 44,322,088 B | 2,896,464 B | 61,428,056 B |
| ordinary, 20k | 7,326,088 B | 7,192,592 B | 44,326,312 B | 2,896,464 B | 61,741,456 B |

The projected points are the `IndexedRecord` total minus the decoded record and the index.

- **The decoded carving record is 7.02 MB.** Task 3's "~7.3 MB" was the wire size of plan 2a's
  ordinary record, copied rather than measured. The number was the right size but described the
  wrong thing.
- **A held carving bake on the owner's world costs 61.4 MB, native, not 49–53 MB.** Task 3's total
  was low for two reasons:
  - its index figure was `memory_bytes`' 41.7 MB, where the allocator measures 44.3 MB;
  - it summed a decoded record and points it had not measured.

  The index's `memory_bytes` splits into headers 31,293,072 B, entries 2,586,496 B and grid
  10,440,376 B.
- **In the browser the index is smaller, by an amount not measured here.** A `Vec` header is
  12 bytes on wasm32 against 24 bytes natively.

---

## 3. The owner's world, baked and carved

### The inland sea, and the reach that drains it

**What "3,627 km" meant (corrected, Ruling C-30 §2).** Plan 2a's report, spec §8.2 and
`water/index.rs` called the great lake "3,627 km across with a 58 km band", and said the band-only
index "answered `Ocean` over a region 1,700 km wide". **3,627 km is a radius, not a width:**
√(4.1326 × 10⁷ km² / π) = 3,626.9 km, the radius of a circle with the lake's area.

It is **not** the index's bounding-circle radius, which is the farthest recorded point
(7,554.8 km from the anchor) plus the band: **7,616.3 km**. The band is **61.6 km** for this
body; 58,083 m was plan 1b-4's median over all 348 coarse bodies.

The claim beside it was re-measured. **Population:** 60,000 points uniform on the bounding cap,
13,977 of them inside the lake by clause 1. **Method:** the distance from each to every recorded
shore member. **Host:** Node, from the 86k carving record.
- The nearest shore member is **1,196 km** from the anchor.
- The deepest interior point is **1,890 km** from any shore member, at (−6.42°, −15.26°).
- **86%** of the clause-1 interior lies farther than a band plus a cell diagonal (132 km) from
  every member.

So a band-only index would have listed most of the lake nowhere. That is a stronger failure than
a "1,700 km region", and I could not re-derive the 1,700 km figure. The reasoning beside it (the
band alone is not enough, so Ruling Q-13's bounding circle) holds, and more strongly. The spec and
the three places in `water/index.rs` now say what each number measures. Plan 2a's report is left
as the historical record.

**The inland sea and the great lake are one body, body 63.** It is the record's only `forced`
body, the one the 0°N 0°E outlet matched. From the carving record:
- `Lake`, fresh, enclosed, level **0.000 m**, depth 4,600 m;
- recorded area **4.1326 × 10⁷ km²**, anchor (6.0975°, 11.5002°);
- **1,422 shore members of 2,835 recorded points**, `shore_reach_m` 61,563.5 m;
- farthest recorded point 7,554.8 km from the anchor.

Its outlet is **reach 3030**, a `Great` river:
- first point at (−18.5913°, −21.9841°), bed −68.944 m + depth 67.944 m = water −1.000 m, one
  metre under the lake;
- 39 points and 241.2 km to body 316, where its mouth is 1,392 m wide.

The chain continues **316 → 317 → 318 → 322 → 319**, every one a `Lake` at 0.00 m, by reaches
3061, 3232, 3286 + 3319, 3325 and 3344, **to the ocean at (−25.938°, −30.106°)**. That is the
chain and the mouth plans 1b-2, 1b-3 and 1b-4 reported. Nothing in plan 2b touches routing, and
nothing moved.

**Does the inland sea survive? Yes.** The survey:
- **Population:** 400,000 points uniform on the spherical cap of radius 7,616.3 km about body 63's
  anchor (its farthest recorded point plus `shore_reach_m`), 1.723 × 10⁸ km².
- **Method:** `wb_water_at` against the carving bake, asked through the bare world and through the
  carved world, plus `elevation_m` on both, at canonical resolution.
- **Host:** native.

| | bare world | carved world |
|---|---:|---:|
| samples the query answers as body 63 | 95,452 | **95,452** |
| area answered as the sea (share × cap area) | 4.1115 × 10⁷ km² | **4.1115 × 10⁷ km²** |
| of those, ground at or under the level | 95,326 | **95,326** |

- **Identical five-word answers, bare against carved: 400,000 of 400,000.**
- **Inside the sea the carved ground differs from the bare ground at 0 samples.** Lake beds are
  not cut (Ruling C-15).
- Outside the sea but inside the cap, the carve changed the ground at 176 samples, the channels.
  The deepest cut is 113.91 m.
- The sampled area agrees with the recorded 4.1326 × 10⁷ km² to 0.5%.

The shoreline stays where it was, and a carved outlet river drains the sea to the ocean. That is
the owner's decision in spec §2.

### Ponds drained and kept, reconciled with Task 4b and Task 6

**Method:** the studio's own `water-params.js::pondChange`, over pairs of records of the owner's
world. It compares the ordinary and the carving bake of the same world and the same params, and
keys fine-found ponds by anchor and level. **Host:** Node, committed wasm.

| wetness nodes | fine-found ponds, ordinary → carving | drained | arrived in freed cells | untouched |
|---|---|---:|---:|---:|
| **86,000** (the world's `lakeNodes`, Task 4b's setting) | 3,732 → 3,324 | **1,311** | **903** | 2,421 |
| **20,000** (`PREVIEW_PARAMS`, what the studio bakes) | 3,719 → 3,320 | **1,285** | **886** | 2,434 |

A second count comes from the rule itself: `ponds::drain_deficit_m`, run natively over the 86k
ordinary record's 3,732 fine-found bodies (step `pond_cell_m` / 4 = 62.5 m, index at 200 km
cells). **1,311 have a deficit over `refine_vertical_m` (1.0 m)**, which is exactly
`is_drained`. So the studio's account, the rule and Task 4b agree to the pond.

**The reconciliation.** Task 6 did not fail to reproduce 1,311 / 903. **It never baked the owner's
world.** Its 75 drained / 8 new / 198 untouched was measured on `DEFAULT_WORLD`, a different
planet. The owner's world differs between Task 4b and the studio only by wetness nodes: 86,000
against the studio's 20,000. The remaining difference, 1,311 / 903 against 1,285 / 886, is that
setting.

**The studio agrees with the 20k row.** It opened the owner's world with `carve=1` and
`forcedOutlet=0,0` for this report (next section). Its own `CarveSession` printed `before 3719,
after 3320, drained 1285, arrived 886, kept 2434` and took 135.5 s for the main-thread bake.

### Is the carve visible in the studio? Yes, in the drawn terrain. Seen as an image later (§9, Ruling C-34).

**Method.** The branch studio was served locally and opened in the desktop app's Chromium pane at
the owner's planet block, with three changes:
- `clouds=0`, so the ground is visible. It is a drawing parameter; the studio's area overlay
  noted it as a different planet for the saved areas;
- `forcedOutlet=0,0`;
- `carve=1`, flown to (−18.62°, −22.2°) at 40 km.

**What was verified, by reading the page:**
- The status line reads `carve=bank 1 widths (tiles on MAIN THREAD)`.
- `window.__wb.world` (handle 4) is the carved world, and `bareWorld` (handle 2) the bare one.
- At reach 3030's first point, the carved world answers **−68.941 m**, the recorded bed, and the
  bare world −3.867 m.
- **Cesium's rendered globe** (`scene.globe.getHeight`) reads **−68.941 m** there: the tile on
  screen carries the cut.
- At (−18.62°, −22.2°), in the same channel, the rendered globe reads −1.008 m, the carved world
  −1.008 m and the bare world +17.29 m.

**What could not be verified:** a screenshot. After the first frame, every capture timed out,
because the page's main thread was filling carved tiles (Task 6's concern 1, visible here). So
**no one has looked at an image of the carved owner's world.** What is established is that the
terrain the studio draws is the carved terrain, at the points read. Whether it *reads* as a river
to the eye is still the owner's call.

*Later:* a picture was taken after this run, on the default world rather than the owner's, and
Ruling C-34 records what it showed. See §9.

---

## 4. Ruling C-18 — detail above a coarse lake's level, inside the lake

**Population:** 2,000,000 area-uniform points over the whole planet (a different seed from §1).

**Method.** `wb_water_at` on the carved world against the carving bake. A sample counts when the
body it names has shore members (a coarse body). For those samples, `elevation_m` was taken at
canonical resolution on the carved and the bare world. **Host:** native.

- **Kinds answered:** none 790,907, ocean 1,119,616, lake 87,296, salt lake 2,043, salt flat 0,
  pond 0, river 138. Ponds and rivers at 0 and 138 are what area-uniform sampling of features
  hundreds of metres across should give.
- **Samples inside a coarse body: 89,331.** Carved differs from bare at **0** of them, so this is
  a property of the bare world and not the carve's (C-18: pre-existing).
- **Detail stands above the body's level at 3,641 (4.08%).** By how much:
  - more than 1 m at 3,423;
  - more than 10 m at 1,927;
  - more than 100 m at 62.
- **Excess when above:** median 10.90 m, p90 46.30 m, p99 119.35 m, max **238.25 m**, in body 185
  at (−27.60°, 22.83°), whose level is 1,119.15 m.
- 300 of the 344 coarse bodies sampled have at least one sample above their level. **The worst are
  highland lakes** (levels 950–1,125 m): bodies 185, 192, 205, 164 and 200, from 238 m down to
  196 m.
- **At or below the datum it is rare:** 42 bodies, 77,085 samples, 125 above.
- **The inland sea:** 89 of 75,809 samples above its level, worst 6.30 m.

**Reading it.** The query judges a coarse lake by the landform and draws its surface at the level.
The ground under it keeps undamped detail, and in highland lakes (mountain relief amplitude) that
detail stands up through the surface as islands of texture. **That is measured.** That the
highland amplitude is the cause is an inference: detail amplitude grows with elevation class
(`Detail::amplitude_m`), and the worst bodies all sit near 1,000 m. **Unresolved; see below.**

---

## 5. Ruling C-21 — rivers arriving far below their lake

**Population:** every reach whose `downstream` is a coarse body.

**Method:** the reach's last recorded point, `bed + depth`, minus the lake's `level_m`. Run on
three records. **Host:** native.

| record | coarse-lake inflows | below by > 0 m | > 1 m | > 10 m | > 50 m | > 100 m |
|---|---:|---:|---:|---:|---:|---:|
| ordinary, 86k | 636 | 35 | 34 | 6 | 4 | 3 |
| for carving, 86k | 636 | 35 | 34 | 6 | 4 | 3 |
| for carving, 20k | 635 | 35 | 34 | 6 | 4 | 3 |

**Confirmed, by id, for the follow-up plan** (86k records):

| reach → body | below the lake by | lake level |
|---|---:|---:|
| **4249 → 280** | **139.75 m** | 307.04 m |
| **3909 → 239** | **127.35 m** | 289.95 m |
| **494 → 23** | **101.29 m** | 100.20 m |
| 4127 → 266 | 57.12 m | 75.02 m |
| 4832 → 313 | 28.03 m | 27.01 m |
| 3857 → 227 | 25.37 m | 24.29 m |

In the studio's 20k records the same six are reaches 4282, 3939, 494, 4164, 4873 and 3885, into the
same bodies by the same metres. **The drain does not touch them**, because it acts on fine-found
ponds, never on coarse lakes or reaches.

Of the 34 more than 1 m below, **19** enter a lake whose level is exactly 0 m.

**Reconciled with Task 4b's 20: 19 is right.** Every one of the 35 inflows arriving below their
lake was listed with its lake's level printed to the bit. **Exactly 19** of them enter a lake
whose level is `0.0` (bits `0x0`). Those 19 arrive 1.01–1.26 m under it: reaches 104, 700, 775,
2597, 2816, 3030, 3061, 3068, 3145, 3157, 3232, 3314, 3319, 3325, 3883, 3896, 4030, 4042 and 4217.
- No other inflow lies in Task 4b's stated band of "about 1.0–1.3 m". The next nearest is reach
  387, 1.51 m under body 21, whose level is 78.23 m, not the datum. After it comes reach 4021,
  1.93 m under a lake at 171.1 m.
- So **every definition consistent with Task 4b's own words gives 19**: datum lakes, the
  1.0–1.3 m band, or both.
- Task 4b's 20 must have counted one more inflow. Reach 387 is the likeliest, because it is the
  only one within 2 m. That is an inference: Task 4b listed its six largest but not these 20.
- The breakdown then stands as 34 = **19** datum mouths + **15** others, not 20 + 14.

---

## 6. Ruling C-22 — the mirror case: pits

**Population:** the fine-found bodies (the last `ponds_kept` in each record).

**Method:** `ponds::drain_deficit_m` at step `pond_cell_m / 4` = 62.5 m over a 200 km-cell index.
The rule is the same as the drain's, and so is its sampling across the channel's full width. A
"pit" is a crossed pond whose largest deficit is below −1 m: every sample of the channel inside its
ring runs more than 1 m **above** the pond's level. **Host:** native.

| record | fine-found | crossed by a channel | channel below level (> 1 m: drained) | **channel > 1 m above level everywhere inside** |
|---|---:|---:|---:|---:|
| ordinary, 86k | 3,732 | 3,543 | 1,372 (1,311) | **2,112** |
| **for carving, 86k — the carved world** | 3,324 | 2,945 | 76 (**0**) | **2,786** |

- **Task 4b's 2,112 is confirmed** on the ordinary record.
- **In the carved world there are 2,786 pits.** That is more than 2,112, because many of the 903
  arrivals are also crossed from above.
- Their excess: median 21.64 m, p90 77.72 m, max 215.17 m (body 770, level 483.55 m). It is apparently
  the same pond as the ordinary record's body 933, renumbered: same level, same excess. That is an
  inference from those two numbers; no anchor was compared.
- **The carving record keeps no dam.** No crossed pond has its channel more than 1 m below its
  level, which is the drain doing its job. The 76 that lie below by less than 1 m are inside
  `refine_vertical_m` and survive by design.

**Reconciled with Task 4b's 1,371: 1,372 is right for the committed rule, and the difference
cannot change a drained pond.** The committed `ponds::drain_deficit_m` was re-run over the
ordinary 86k record (byte-identical to the record Task 4b measured), and the result was put into
Task 4b's own histogram bins:

| deficit bin (m) | (0, 0.01] | (0.01, 0.05] | (0.05, 0.1] | (0.1, 0.2] | **(0.2, 0.5]** | (0.5, 1] | (1, 2] | (2, 5] | > 5 | total |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Task 4b | 0 | 5 | 3 | 8 | **22** | 22 | 61 | 165 | 1,085 | 1,371 |
| this report | 0 | 5 | 3 | 8 | **23** | 22 | 61 | 165 | 1,085 | **1,372** |

- **The whole difference is one pond, in the (0.2, 0.5] bin.** That is under
  `refine_vertical_m` (1.0 m), so the pond is kept whichever count is right.
- Every bin above 1 m agrees, so **both counts give the same 1,311 drained ponds**:
  1,372 − 61 = 1,311 here, and 1,371 − 60 = 1,311 in Task 4b.
- The 1,311 is confirmed three independent ways: the rule run here, the bake's own drop, and
  the studio's `pondChange`.
- No deficit sits on a bin edge. The 23 in (0.2, 0.5] run 0.229–0.497 m, and there is no value
  of exactly 0.
- The smallest deficit is 0.0267 m, as Task 4b said ("from 0.027 m").

So the difference is not a boundary condition or a tolerance. It is the tool. **1,372 comes from
the function the bake calls, over the record the bake produced.** Task 4b's histogram was taken
before the rule was final, by a measurement it did not keep, and I cannot say which pond it
missed.

**The same re-run corrects a second Task 4b figure.** The largest deficit is **299.10 m** (body
2285), not the "1,089 m" Task 4b gave, and `ponds.rs`'s doc had carried that number too. The count
with the channel below the pond's own floor reproduces at 1,080. `ponds.rs`'s doc now says
1,372 / 61 / 299.1 m and notes where Task 4b's figures came from.

---

## 7. Spec corrections

`docs/superpowers/specs/2026-09-10-automatic-water-design.md`:

- **§8.3's Q-20 bullet: resolved.** It said a bake queried against another world of the same
  radius "answers wrongly rather than erroring". Since Task 2 the fingerprint check refuses it
  with `WB_ERR_WRONG_WORLD` / `WrongWorldError`, once per call. The bullet now says so.
- **§8.1 rewritten to what was built:**
  - opt-in, and why `GENERATOR_VERSION` did not move;
  - the two phases (bake on bare ground through `bake_ground_m`, carve by joining), and the
    refusals at the join and at every bake-like entry point;
  - the bed interpolated along the leg (C-13) and banks over `bank_widths` widths, where the spec
    had "one width";
  - the lake-bed rule (C-15);
  - **the drain and SCHEMA 8** (C-16, C-20);
  - multiplicative damping over roughness and gullies (C-14), and the undamped lake floor (C-18).
- **§8.2:**
  - the shared `Arc<IndexedRecord>`, and the index footprint widened to the bank;
  - the measured memory at the owner's radius;
  - **the performance target's measured miss and what dominates it**;
  - that the painted-feature half of the target is still unmeasured (painted features are not in
    the index).
- **§7's versioning note:** the layer did not bump `GENERATOR_VERSION` (Task 7).
- **§5's diagram:** the water layer is a second world joined to the record, and the bake reads the
  bare one.

---

## 8. CI pins — every one re-derived by running

**Engine.** Counted by `cargo test -p worldbuilder-engine <cfg> -- --list` and `-- --list
--ignored`, summing each binary's `N tests` trailer, then passed through
`.github/scripts/assert_counts.py cargo-list`, which printed `count OK` at all five. Each suite was
also run in full with `--no-fail-fast`:

| configuration | listed | ignored | **run (the gate's `expect`)** | full run | binaries |
|---|---:|---:|---:|---|---:|
| `--no-default-features` | 914 | 11 | **903** (was 903) | 903 passed / 0 failed / 11 ignored | 18 |
| default | 914 | 11 | **903** (was 903) | 903 / 0 / 11 | 18 |
| `--features python` | 917 | 11 | **906** (was 906) | 906 / 0 / 11 | 18 |
| `--features wasm` | 1,055 | 11 | **1,044** (was 1,044) | 1,044 / 0 / 11 | 18 |
| `--features python,wasm` | 1,058 | 11 | **1,047** (was 1,047) | 1,047 / 0 / 11 | 18 |

**Other pins:**

| pin | before this task | **now** | how |
|---|---|---|---|
| Python total / conformance | 577 / 168 | **577 / 168** | `pytest tests/ -q --collect-only`, and the same on `tests/test_conformance.py` |
| Python run | — | **577 passed**, 0 failed (`test_installation` passed this run) | `WORLDBUILDER_REQUIRE_ENGINE=1 pytest tests/ -q` under the repo `.venv`, whose extension Task 7's fix round rebuilt from this tree (nothing under `src/` has changed since) |
| Viewer `npm test` (not a CI pin) | 381 | **381 / 381** pass | `npm test` |
| Parity, compared / divergent | 179,086 / 0 | **179,086 / 0** | `parity_dump` then `node parity.mjs native.txt` |
| `--mutate seed` | 170,363 | **170,363** | |
| `--mutate erosion-k` | 216 | **216** | |
| `--mutate water-pond` | 60 | **60** | |
| `--mutate tectonic-warp` | 39,702 | **39,702** (TCTL: hydro/ranges 16,811, carve/ranges 12, hydro_carve/ranges 16,691 — "exactly as the native side predicted") | |
| `--mutate coast-amplitude` | 13,128 | **13,128** | |
| `--mutate gully-steer` | 3,752 | **3,752** | |
| `--mutate climate-samples` | 648 | **648** | |
| `--mutate carve-bank` | 12 | **12** (carve/ranges 6/30, carve/plain 6/26, the bank points) | |
| `check:wasm` | current | **current** ("matches its manifest and the source that is here now") | `npm run check:wasm` |
| EOL guard | clean | **clean** (printed nothing) | `git ls-files --eol crates/worldbuilder-engine viewer/public/app \| awk '$2!="w/lf"'` |

Every parity figure was checked by `assert_counts.py parity`, which printed `count OK` for the corpus and each of the eight controls. The native dump came from `cargo run --release -p worldbuilder-engine --example parity_dump --features wasm`, the replay from the committed `.wasm`.

**At the report's first commit (`872458f`), no pin moved.** That commit changed no source, no
test and no parity record.

**Ruling C-30 moved the engine rows by +2 each.** Two tests in `src/water/index.rs`, ungated. They
were re-derived by `--list` / `--list --ignored`, summing trailers, through
`assert_counts.py cargo-list` (`count OK` at all five), and `gates.yml` was moved with a dated
comment:

| configuration | listed | ignored | **run (`expect`)** |
|---|---:|---:|---:|
| `--no-default-features` | 916 | 11 | **905** (was 903) |
| default | 916 | 11 | **905** (was 903) |
| `--features python` | 919 | 11 | **908** (was 906) |
| `--features wasm` | 1,057 | 11 | **1,046** (was 1,044) |
| `--features python,wasm` | 1,060 | 11 | **1,049** (was 1,047) |

The `python,wasm` suite was run in full at that commit: 1,049 passed, 0 failed, 11 ignored.

**Parity and every control are unmoved by C-30,** and the native dump is byte-identical (§1).

**At the corrections commit that follows C-30**, everything was run in full on the final tree.
That commit changes doc comments in `index.rs`, `ponds.rs` and `parity_dump.rs`, the parity
README, the spec and this report:
- **engine:** 905 / 905 / 908 / 1,046 / 1,049 passed, 0 failed, 11 ignored, `--no-fail-fast`;
  `--list` gave 916 / 916 / 919 / 1,057 / 1,060 across 18 binaries;
- **Python:** 577 collected, 168 of them conformance. Run with
  `WORLDBUILDER_REQUIRE_ENGINE=1`: 577 passed, after `maturin develop --release --features python`
  rebuilt the `.venv` extension. The first run refused it as STALE, which is the guard working;
- **viewer:** 381 / 381;
- **parity:** 179,086 / 0, with every control at its pin (seed 170,363, erosion-k 216,
  water-pond 60, tectonic-warp 39,702, coast-amplitude 13,128, gully-steer 3,752,
  climate-samples 648, carve-bank 12). The native dump is byte-identical to the one at
  `872458f`;
- **wasm:** rebuilt, `check:wasm` current, EOL guard clean.

The artifact's bytes moved again (artifact-sha256 `fcdc42ed…8d97`, still 484,383 bytes; source
fingerprint `e7909761…c4c2`), although only comments changed. The inference is that shifted
source lines move the file:line locations embedded in panic messages. The corpus says no
behaviour moved. The
wasm was rebuilt: artifact-sha256 `3fd8a3bc…5b6f`, 484,383 bytes, source fingerprint
`04c9a6c9…d6ae` over 72 inputs. `check:wasm` reports it current.

**Where pins live.** Pins live in **three** places, not the two Ruling C-4 named:
- this report is the record;
- `.github/workflows/gates.yml` is the gate;
- `.github/scripts/assert_counts.py` parses what the gate prints. Task 7 found it would not
  recognise a new control's label.

The script was run against every engine count above. The next plan's Global Constraints should say
three.

---

## 9. What this plan did not resolve

- **The performance target (§1) is met natively and not shown in wasm.** After Ruling C-30's
  bitmap, the native median rises 11–12% (it was 27–37%). In the studio's host the best
  per-sample reading is 1.22–1.24, a miss by 2–4 points, on a clock too coarse to be sure. What
  remains there is `to_latlon`'s arithmetic in finding the cell. Two statistics still fail
  natively: the land-only median (+39–51%) and the mean (+50%). Both are carried by the 10.8% of
  samples in channel cells, which pay the leg walk (~4×). And the touched minority pays
  16–100 µs per sample, dominated by `body_claim`'s full outline scan of coarse bodies. That
  tail is what a river-in-view tile pays: 2.5–4.5× in wasm.
- **Turning the carve on freezes the studio for minutes.** Task 6 measured 207 s for the whole
  turn-on on the default world. On the owner's world, this report's studio run spent **135.5 s**
  in the main-thread carving bake alone. The ordinary bake for the pond account runs in a worker;
  Ruling C-28 deferred removing it. The whole turn-on was not timed here. Nothing persists the
  record, so a shared `carve=1` link pays this on every load.
- **Carved tiles draw on the main thread** (Task 6, concern 1). No export loads a record into a
  worker. In this report's studio run the page was busy enough that no screenshot could be taken
  after the first frame.
- **C-21's contradictions.** Reaches **4249 → 280** (139.75 m), **3909 → 239** (127.35 m) and **494
  → 23** (101.29 m) arrive over 100 m below their lake, with three more between 25 and 57 m. They
  are record contradictions from the bake's routing, which the carve neither causes nor fixes.
- **C-22's pits.** 2,786 of the carved world's 3,324 fine-found ponds have a channel whose water
  runs more than 1 m above their level everywhere inside the ring, by up to 215 m. Water can flow
  in, so none is a dam. But a pond sitting in a hole beneath a river is not a landform the record
  should describe.
- **C-18's detail above lakes.** 4.08% of coarse-lake samples have ground above the water: 1,927
  of 89,331 by more than 10 m, up to 238 m. They are concentrated in highland lakes. It is
  pre-existing and untouched by the carve, but a carved world that cuts rivers cleanly and leaves
  texture islands in its lakes is inconsistent.
- **The carve reads as a canal, not a river (Ruling C-34). A finding, and a follow-up; not fixed
  in this plan.** Someone has now looked at a picture of the carve. Population and method: the
  studio at :8138 on this branch, the default world (seed 20260904, 12 plates, land 0.29) with
  `?carve=1`; great river 1911 at 20.0338, −132.1332 (bed 131.2 m, width 177 m, depth 13.0 m),
  cross-sectioned bare against carved through `wb_elevation_m`, then a screenshot at 2.6 km and
  −38° pitch with the pane fronted and the side panels hidden. What it showed:
  - **The carve is visible and exact.** Untouched at ±450 m and beyond, bit for bit; the floor
    flat at exactly 131.2 m, `bed_m`, across ±120 m; the banks blending between.
  - **It reads as engineered.** The river crosses a ridge there, so the cut is 233 m deep on a
    177 m channel: walls of about 53°, a flat bottom, and sharp turns where the channel follows
    the simplified polyline from leg to leg. A river crossing a ridge does make a gorge, but not
    this one. The cause is the profile spec §8.1 specifies: one bank width either side whatever
    the depth, and a polyline with corners.
  - **No water is drawn in it by default**, so the channel reads as a dry trench.

  Follow-up: a bank profile that widens with cut depth, and smoothing of the centre line through
  its recorded points. Either moves every carved value, so it is a later plan's decision, not this
  plan's. The dry-looking channel is a drawing question for §9.1's water rendering. (The query
  side of "dry" is now fixed: Ruling C-35, §10, makes the query answer water in a notch.)
- **Below the datum the query and the carve still differ, by design** (`water/layer.rs`, "One
  exception"). The query answers `Ocean` before any reach or notch, while the layer cuts there at
  authority 1. Measured natively on `bake_tests::world()` baked for carving at
  `earth_like(60_000)`, at every reach mid-leg: 3 of 2,169 answered `Ocean`, and 2 of those stand
  more than 1 m lower in the carved world than in the bare one. There `Ocean`'s depth is read off
  the uncut landform. The ocean's precedence was deliberately not changed.
- **Not measured here:**
  - the wasm32 size of the index;
  - the painted-feature half of §8.2's target.

---

## 10. The final fix wave (Rulings C-35 and C-36)

The final whole-branch review (`aa1e806`) found two blockers and seven minors. Both blockers are
fixed, each shown failing before its fix.

### Ruling C-35: the query answers water in a notch

A notch is where a lake spills through its rim. The carve cut notches exactly as it cut reaches,
but the query had no notch clause. So wherever no reach ran through a notch, the query called
the cut channel dry.

**Measured, by running.** Population: every recorded notch of `bake_tests::world()` (seed
20,260,904, 6,371 km, 12 plates, 0.29 land). Method: baked for carving at `earth_like(30_000)`
and `earth_like(60_000)`, carved by its own record at the canonical block, native release build,
this report's host. Results:
- 12 notches in all (5 and 7), and 10 lie 97 km to 1,732 km from any recorded reach point. The
  distance is nearest recorded notch point to nearest recorded reach point. The review said
  1,782 km; that was not reproduced.
- At their recorded points, the carved world stands 11.2 m to 55.8 m below the bare one (the
  deepest point of each notch).

**The fix.** After the reaches, `water_at` asks the notches through the same `line_claim` a
reach uses: the same `leg_foot`, the same Ruling Q-7 width and the same half of it. So a point is
in a notch's water exactly where the layer cuts it at full authority. The answer is:
- `River`, at the notch's cut surface read along the leg by `along_leg`, which is the target the
  layer cuts to;
- `reach_id = NO_REACH` and `body_id = NO_BODY`;
- depth is the level over the landform, or 0 where the cut stands at the surface;
- `fresh = true`.

A reach that also claims the point answers first, and keeps its id.

**`WaterKind` did not change.** Every other `River` names its reach (Ruling Q-18), so a `River`
naming none is a notch's. The kinds and the five-word sample are unchanged.

**Seen failing at `aa1e806`, before the fix:**
- `the_carve_and_the_query_agree_about_where_the_channel_is` now has three notches in its
  fixture (one no reach touches, one crossing a reach, one a single point). It failed: the query
  said river=false where the carve's authority was 1.
- `no_channel_is_dammed_by_a_pond_it_drains` now fails on any dry sample inside a channel, and
  requires that some sample of a notch no reach runs through was judged. It failed: 1,295 (30k)
  and 1,253 (60k) samples inside a cut channel were dry, and no notch sample was judged.
- After the fix: 0 dry inside a channel, and 1,623 (30k) and 1,530 (60k) notch-only samples
  judged.
- Also added or extended: a unit test of the notch answer (`query.rs`);
  `the_query_agrees_with_the_record_at_every_recorded_point` now asserts that no notch point is
  dry; and the channel samples behind the two texture and water-surface tests now include
  notches.

**Parity: no group moved.** The native dump is byte-identical to `aa1e806`'s. No `water_at` grid
sample falls in a notch's footprint, and every `water_point` river probe sits on a recorded reach
point. Native and wasm changed identically, and divergent stays 0.

### Ruling C-36: a carved world answers only through the bake it was carved from

`with_water_query` compared ground fingerprints only, and a carved world fingerprints as its bare
parent. The fix: for a carved world (`Surface::carved_from`), the held bake must be the very
`Arc` the world holds (`Arc::ptr_eq`). Anything else is refused with **`WB_ERR_NOT_CARVED_FROM`
(11)**. The new status is:
- named in `engine.js`'s exports and `STATUS_NAMES`;
- given a sentence in `water-params.js`'s `CARVE_REFUSALS`;
- pinned by `every_status_is_distinct_and_the_viewer_names_every_one`.

Two further points:
- **A bare world is still judged by its ground alone**, so Ruling Q-20 stands.
- **The record cache no longer replaces a slot a carved world shares.** Otherwise a refused carve
  of the same bake at another radius could evict the carved world's own record, and the tie would
  refuse its rightful bake. A test covers this.

**Seen failing at `58e83c5`, with only the status constant declared.** Both refusal tests returned
`WB_OK` where 11 was expected:
- `a_carved_world_refuses_the_water_query_through_an_ordinary_bake_of_its_ground`;
- `a_carved_world_refuses_the_water_query_through_a_second_carving_bake_of_its_ground`, which
  covers both another wetness and a bit-identical re-bake.

The accepted case passes both before and after: `a_carved_world_answers_the_water_query_through_the_bake_it_was_carved_from`.
The viewer's live check through the shipped wasm (`water-params.test.mjs`) failed against the old
artifact and passes against the rebuilt one.

### The seven minors

- **4.** `ground_fingerprint`'s two `expect`s are gone, and a status was not added, because none
  is needed. The hash is now `Blake2b<U16>`, the same BLAKE2b at a 16-byte output: the length
  still enters the parameter block, so the digest is unchanged. Its `finalize` cannot fail, so
  there is no failure left to report. The native dump is byte-identical, fingerprint words
  included.
- **5.** `layer_tests.rs`'s `id as usize` carries a `// cast-ok:`. **The ledger scanner does not
  check `as usize`**, which is why it passed. The scanner was not widened in this wave.
- **6.** `pondChange` returns `null` when the two records' `ground` fingerprints differ. The
  owner is then told the account is not given, rather than shown a wrong one. A test covers it,
  and a mutation removing the check turned it red.
- **7.** `engine.js` and `carve-session.js` now say that an admissible block with no bake costs no
  build: `build_surface` resolves the bake id before any `Surface` is constructed.
- **8.** `parity/README.md`'s `GENERATOR_VERSION` rationale now names both halves:
  - terrain is bit-identical;
  - every ordinary record changed, versioned by `SCHEMA`;
  - the query's answers moved (C-13's levels between recorded points; C-35's notches), and these
    are a reader's behaviour, not the generator's output.
- **9.** `WB_HYDRO_PARAMS_CARVE_STRIDE`'s doc says length parity works exactly once. The next
  layout needs an explicit tag.
- **10.** `layer.rs` no longer claims the query and carve agree "exactly". It names the
  below-datum exception with the figures in §9. The ocean's precedence is unchanged.

### Pins, every one re-derived by running

**Engine.** Counted per configuration by `cargo test -p worldbuilder-engine <cfg> -- --list`, and
the same with `--ignored`, summing each binary's trailer (18 binaries each). Each count was
checked by `assert_counts.py cargo-list` (`count OK` at all five), and each suite was run in full
with `--release --no-fail-fast`:

| configuration | listed | ignored | **run (`expect`)** | full run |
|---|---:|---:|---:|---|
| `--no-default-features` | 917 | 11 | **906** (was 905) | 906 / 0 / 11 |
| default | 917 | 11 | **906** (was 905) | 906 / 0 / 11 |
| `--features python` | 920 | 11 | **909** (was 908) | 909 / 0 / 11 |
| `--features wasm` | 1,061 | 11 | **1,050** (was 1,046) | 1,050 / 0 / 11 |
| `--features python,wasm` | 1,064 | 11 | **1,053** (was 1,049) | 1,053 / 0 / 11 |

**Other pins:**

| pin | before | **now** |
|---|---|---|
| `no_std_math` (the ledger test) | 7 pass | **7 pass** |
| Python total / conformance | 577 / 168 | **577 / 168**; run 577 passed after `maturin develop` rebuilt the `.venv` extension; `assert_counts.py pytest` `count OK` |
| Viewer `npm test` | 381 | **381 / 381** (assertions added to existing tests) |
| Parity | 179,086 / 0 | **179,086 / 0** |
| `--mutate seed` | 170,363 | **170,363** |
| `--mutate erosion-k` | 216 | **216** |
| `--mutate water-pond` | 60 | **60** |
| `--mutate tectonic-warp` | 39,702 | **39,702** ("exactly as the native side predicted") |
| `--mutate coast-amplitude` | 13,128 | **13,128** |
| `--mutate gully-steer` | 3,752 | **3,752** |
| `--mutate climate-samples` | 648 | **648** |
| `--mutate carve-bank` | 12 | **12** |
| `check:wasm` | current | **current** (486,081 bytes, artifact-sha256 `d1c379b9…ce48`, source fingerprint `8d11394d…2fe8` over 72 inputs) |
| EOL guard | clean | **clean** |

- Every parity figure was checked by `assert_counts.py parity`.
- `gates.yml` carries the new engine rows with a dated comment, and dated notes on the unchanged
  Python and corpus pins.
- `assert_counts.py` needed no change: no new label.

