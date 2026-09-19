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

1. **Spec §8.2's performance target is MISSED.** The median native `elevation_m` on the owner's
   world rises **30–37%**, against a limit of 20%. The median per-point ratio rises **27–30%**.
   **What dominates is measured, and it is neither of the two ledgered suspects.** It is the fixed
   index lookup that every sample pays, touched or not: about 150 ns of a ~630 ns sample, most of
   it one cache miss. It is not `inside_ring`, which runs on 0.02% of samples. It is not the empty
   `Vec` headers either: a packed table of the same cell count costs within 18 ns of the real
   index.
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

## 1. Spec §8.2's performance target — MISSED

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

**What would plausibly meet the target — an inference, not measured.** A per-cell occupancy
bitmap would let `cut_with` return before touching the index at all. It needs one bit per cell,
set where any reach or notch is listed: 54 KB at this radius, small enough to stay in cache. Read
after an elevation sample, it cost 68–90 ns, against the index's 176–179 ns — a saving of roughly
90–110 ns on the 89% of samples it would reject. That would take the fixed overhead from ~27% to
somewhere near 10% at the median. **This is an estimate from the microbenchmark, not a
measurement of a changed engine.** It is not built here: it changes `water/index.rs` and
`layer.rs`, moves the wasm, and needs its own tests and parity run, so it is not a small, contained
fix inside a report task. **It is the follow-up this finding asks for.**

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

### Is the carve visible in the studio? Yes, in the drawn terrain. Not seen as an image.

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

Of the 34 more than 1 m below, **19** enter a lake whose level is exactly 0 m. Task 4b counted 20
"about 1.0–1.3 m under a lake at 0 m". My filter (more than 1 m below, level exactly 0) gives 19.
The one-body difference is in how the band was drawn and is not reconciled further.

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

The ordinary crossed-below count here is **1,372** by `d > 0`; Task 4b reported 1,371 from its
histogram. The one-body difference is not reconciled. It does not touch the drained count, which
reproduces at 1,311.

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

**No pin moved.** This task changes no source, no test and no parity record: only this report, the
spec and a dated comment in `.github/workflows/gates.yml` saying the rows were re-derived and are
unchanged. The wasm was not rebuilt, because nothing under `src/` changed.

**Where pins live.** Pins live in **three** places, not the two Ruling C-4 named:
- this report is the record;
- `.github/workflows/gates.yml` is the gate;
- `.github/scripts/assert_counts.py` parses what the gate prints. Task 7 found it would not
  recognise a new control's label.

The script was run against every engine count above. The next plan's Global Constraints should say
three.

---

## 9. What this plan did not resolve

- **The performance target (§1).** The median `elevation_m` rises 27–37% against a limit of 20%.
  The fixed index lookup is the cause, measured. An occupancy bitmap is the likely fix; that is an
  estimate, not built. The touched minority pays 16–100 µs per sample, dominated by `body_claim`'s
  full outline scan of coarse bodies. That tail is what a river-in-view tile pays: 2.5–4.5× in
  wasm.
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
- **Not measured here:**
  - the wasm32 size of the index;
  - the painted-feature half of §8.2's target;
  - a visual check of the carved owner's world by a person.
