# Water 1a calibration report: the coarse-bake survey (Task 12)

**Scope of this document.** Task 12 has a native half (this document's "Native survey"
section, done here) and an owner-world half done in the browser studio against the owner's
saved world (`worlds/world-1788998299904.json`), per Controller Ruling E in the task-12
ledger. That half is marked "to be filled by the controller" below and is not attempted here.
No `earth_like` constant was changed in this half.

## Native survey

**Binary:** `crates/worldbuilder-engine/src/bin/hydro_survey.rs`. Run:

```
cargo run --release --no-default-features --bin hydro_survey
```

See the binary's own module doc for the exact method (population, method-with-parameters,
host) behind every figure below. In short: two worlds --

- `plain`: `Surface::new(20_260_904, 6_371_000.0 m, 12 plates, 0.29 land, None, None, None)`.
- `owner_survey`: `Surface::new(562_423_712, 4_500_000.0 m, 28 plates, 0.16 land, None, None,
  Some(TectonicParams::ranges()))` -- a tectonics-shaped stand-in used only to see how the
  bake's own cost and thresholds move on a tectonics world natively. **This is not the
  owner's saved studio world** and does not substitute for Step 2's numbers.

-- each baked with `HydroParams::earth_like(n)` (no forced outlets) at `n` = 250,000 /
1,000,000 / 2,000,000, on this developer machine, `cargo run --release
--no-default-features`.

### Output table

```
== plain ==
  n =    250000  total    9.19 s  (graph   9.03  flood   0.06  hollows   0.00  routing   0.04  flow   0.04  reaches   0.02)
    memory proxy     21.456 MiB  land nodes     72965  median land area    2.037e9 m^2
    hollows      65  kept     48  notched     17  closed     30  enclosed      1
    bodies: lakes    18  ponds     0  salt lakes    29  salt flats     1
    reaches: streams    3900  rivers  13613  great  684  max order   6  bifurcation ratio [4.43, 8.11]
    (wall clock for this run, including setup: 9.20 s)
  n =   1000000  total   29.95 s  (graph  29.41  flood   0.26  hollows   0.01  routing   0.15  flow   0.07  reaches   0.05)
    memory proxy     85.600 MiB  land nodes    291868  median land area    5.100e8 m^2
    hollows      76  kept     26  notched     50  closed      6  enclosed      1
    bodies: lakes    20  ponds     0  salt lakes     6  salt flats     0
    reaches: streams   11322  rivers  22638  great 1026  max order   6  bifurcation ratio [4.33, 10.00]
    (wall clock for this run, including setup: 30.01 s)
  n =   2000000  total   60.48 s  (graph  58.79  flood   0.72  hollows   0.03  routing   0.57  flow   0.28  reaches   0.08)
    memory proxy    172.039 MiB  land nodes    583675  median land area    2.547e8 m^2
    hollows      74  kept     26  notched     48  closed      7  enclosed      1
    bodies: lakes    19  ponds     0  salt lakes     6  salt flats     1
    reaches: streams   17938  rivers  27405  great 1240  max order   7  bifurcation ratio [4.17, 8.75]
    (wall clock for this run, including setup: 60.58 s)

== owner_survey ==
  n =    250000  total   12.70 s  (graph  12.58  flood   0.05  hollows   0.00  routing   0.03  flow   0.02  reaches   0.01)
    memory proxy     21.456 MiB  land nodes     40680  median land area    1.017e9 m^2
    hollows      78  kept     44  notched     34  closed     12  enclosed      2
    bodies: lakes    32  ponds     0  salt lakes    11  salt flats     1
    reaches: streams    3837  rivers   7419  great  217  max order   5  bifurcation ratio [4.88, 7.80]
    (wall clock for this run, including setup: 12.71 s)
  n =   1000000  total   32.20 s  (graph  31.59  flood   0.30  hollows   0.01  routing   0.15  flow   0.11  reaches   0.04)
    memory proxy     85.600 MiB  land nodes    162733  median land area    2.541e8 m^2
    hollows     142  kept     71  notched     71  closed     26  enclosed      3
    bodies: lakes    45  ponds     0  salt lakes    20  salt flats     6
    reaches: streams    8737  rivers   9576  great  322  max order   5  bifurcation ratio [4.73, 9.00]
    (wall clock for this run, including setup: 32.23 s)
  n =   2000000  total   58.59 s  (graph  57.48  flood   0.56  hollows   0.02  routing   0.23  flow   0.24  reaches   0.06)
    memory proxy    172.043 MiB  land nodes    325418  median land area    1.271e8 m^2
    hollows     159  kept     81  notched     78  closed     24  enclosed      4
    bodies: lakes    57  ponds     0  salt lakes    14  salt flats    10
    reaches: streams   12516  rivers  10323  great  473  max order   6  bifurcation ratio [4.45, 13.00]
    (wall clock for this run, including setup: 58.65 s)
```

Total run time for all six bakes on this host: 3 min 24 s. No run approached the 15-minute
per-run stop-and-report threshold from the brief; nothing was cut short.

### Per-step time, at a glance

(Final review I8: `hydro_survey` no longer re-stages the pipeline by hand. It now times the two
halves `hydrology::bake` itself is made of, `bake_stages` and `record_of`, and prints
`drainage_check` for every run. The per-step figures in this section come from the earlier
hand-staged survey, before the drainage fix, and describe that code path, not the current one.)

| world | n | graph | flood | hollows | routing | flow | reaches | total |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| plain | 250k | 9.03 s | 0.06 s | 0.00 s | 0.04 s | 0.04 s | 0.02 s | 9.19 s |
| plain | 1M | 29.41 s | 0.26 s | 0.01 s | 0.15 s | 0.07 s | 0.05 s | 29.95 s |
| plain | 2M | 58.79 s | 0.72 s | 0.03 s | 0.57 s | 0.28 s | 0.08 s | 60.48 s |
| owner_survey | 250k | 12.58 s | 0.05 s | 0.00 s | 0.03 s | 0.02 s | 0.01 s | 12.70 s |
| owner_survey | 1M | 31.59 s | 0.30 s | 0.01 s | 0.15 s | 0.11 s | 0.04 s | 32.20 s |
| owner_survey | 2M | 57.48 s | 0.56 s | 0.02 s | 0.23 s | 0.24 s | 0.06 s | 58.59 s |

`LandGraph::sample` (the "graph" step -- node sampling plus the two `Surface::structural_m` /
`moisture_index` fields) dominates every run by roughly two orders of magnitude over the rest
of the pipeline combined; the hydrology-specific work (flood/hollows/routing/flow/reaches)
stays under a second even at 2,000,000 nodes on both worlds.

### Memory proxy, at a glance

| n | memory proxy |
|---:|---:|
| 250,000 | 21.456 MiB |
| 1,000,000 | 85.600 MiB |
| 2,000,000 | 172.039-172.043 MiB |

The proxy (the summed byte length of every `Vec` `LandGraph` holds, from lengths and element
sizes -- see the binary's module doc) scales linearly with node count and is identical
between the two worlds to three decimal places, as expected: it depends on node/edge counts,
not on the elevation field. **This is the bake's native working set only** -- it does not
include the wasm runtime or the studio's own state, which the owner-world heap reading in
Step 2 will carry on top of it.

### What the native evidence says about the node budget

The brief's node-budget rule (`largest count whose studio heap stays <= 512 MB and whose bake
finishes inside 5 minutes`) is stated in terms of the **studio's wasm heap** and **owner-world**
wall time, both of which are Step 2's numbers and out of scope here. Native evidence only
bounds the question from below:

- **Time is not the native constraint at any of the three counts.** Even 2,000,000 nodes
  finished in about a minute, native, twenty times inside the 5-minute owner-world figure the
  brief allows headroom for wasm being slower than native.
- **The memory proxy is not the native constraint either.** 172 MiB of graph vectors at
  2,000,000 nodes is well under the 512 MB studio-heap ceiling, even allowing generously for
  wasm's own overhead and the rest of the studio's state sharing that heap.

So native evidence rules out neither 250k, 1M nor 2M on time or memory grounds -- the decision
among them has to come from the owner-world heap reading and wall clock, which only the
controller's Step 2 run produces. `DEFAULT_TOTAL_NODES` is **not** added to `mod.rs` in this
half, per the task instructions.

### Bodies, thresholds, and land-node area -- reported as they fall

`ponds: 0` in every single run, on both worlds, at all three node counts. `HydroParams::
earth_like`'s `pond_max_area_m2 = 1.0e6` is several orders of magnitude below the median
land-node area at any of these counts (2.5e8 to 2.0e9 m^2) -- a single coarse node's own
tributary area is already larger than the pond/lake split, so no kept hollow can land below
it. This is the calibration finding the brief asked this survey to surface, not a bug: at
these node counts the pond/lake split as written cannot produce a pond.

Median land-node area drops roughly in proportion to node count (about 8x from 250k to
2,000,000, close to the expected ~1/n scaling since land area is fixed and node count is not),
and is consistently lower on `owner_survey` than on `plain` at a given `n` (that world samples
fewer land nodes at the same total-node budget, since its land fraction is 0.16 vs 0.29).

Bifurcation ratios landed inside 3-5 at every node count on `plain` and mostly inside it on
`owner_survey`, with the max edging over 5 at 1M and 2M nodes (9.00 and 13.00 respectively) --
`HydroParams::earth_like`'s stream/river/great flow thresholds were written for a much finer
graph, so this scatter at coarse counts is expected rather than a fault to chase. Per the
brief, this is reported as it falls; no threshold was moved, since the brief's tuning rule
("move `stream_flow_m2` ... at the default node count") is scoped to the owner's world at the
controller-chosen default, neither of which exists yet in this half.

**Pit lakes / kept-body count, for context (not a substitute for the owner-world figure):**
kept body counts on the two survey worlds ranged 26-81 across the six runs (`plain`: 48/26/26
at 250k/1M/2M; `owner_survey`: 44/71/81), nowhere near the spec's old figure of 1,361 bodies
(638 single-node) measured on a fine, undetrended mesh. That old figure came from detail-noise
lakes on a much finer graph; the coarse `LandGraph` here samples the landform only (never
detail noise, per the module doc), which is exactly why the body count collapses by more than
an order of magnitude. The controller's Step 2 run against the owner's actual saved world is
the number that belongs beside the old 1,361 in the spec's calibration table.

## Owner world (studio)

Filled by the controller (Task 12b), from a browser-studio run (http://localhost:8138, wasm)
against the owner's saved world `worlds/world-1788998299904.json` (radius 9,309 km, land 0.4,
forced outlet at the inland sea's centre, 0°N 0°E).

| Run (500,000 nodes) | Reaches | Streams / rivers / great | Top order | Bifurcation | Record | Time |
|---|---|---|---|---|---|---|
| earth_like | 69,544 | 16,160 / 48,081 / 5,303 | 6 | 4.66-52 | 31 MB | 30 s |
| stream x2 | 54,181 | 7,902 / 41,621 / 4,658 | 5 | 4.71-8 | 28 MB | 32 s |
| stream = 10 x node area (2.2e10 / 2.2e11 / 2.2e12) | 2,126 | 1,695 / 424 / 7 | 4 | 5.28-43 | 17 MB | 41 s |
| stream = 30 x node area | 606 | 524 / 77 / 5 | 3 | 7.61-22 | 16 MB | 42 s |
| **1,000,000 nodes**, stream = 10 x node area (1.1e10 / 1.1e11 / 1.1e12) | 4,697 | 3,398 / 1,219 / 80 | 4 | 4.68-12.1 | 39 MB | 80 s |

At 1M nodes: hollows 1,190 (352 kept, 778 notched, 60 closed), notch routes 84,145, **heap
372 MB** (under the 512 MB ceiling from section 6.1). At 500k nodes: hollows 722 (286 kept, 436
notched, 42 closed), **notch routes 41,202** (which set the record size before Ruling 12b-2),
heap 244 MB.

**The great lake (500k):** the forced body is fresh, enclosed and level 0, area 41.24M km²
(measured earlier at 41.7M), an outlet reach, anchor at 6.05°N 11.76°E. Body kinds: 244 lakes,
39 salt lakes, 3 salt flats, 0 ponds (ponds cannot exist at graph resolution -- unchanged from
the native survey's own finding above; addressed in plan 1b, not here). The median kept-lake
area is 8,816 km².

**Findings that drove the rulings (see the spec's section 6.5a and the ruling table there for
the resulting changes):**

1. **The Earth thresholds cannot apply to a graph that cannot see below its own node area.** At
   500k nodes, a land node stands for about 2,200 km² of catchment, above the 250 km² stream
   threshold from section 6.5, so every land node is a channel. The plan's calibration lever
   (stream x2, up to 4 steps) cannot land: x16 would put the stream threshold above the river
   one, and even x2 leaves 54k reaches. Ruling 12b-1 replaces the lever with resolution-aware
   floors; Ruling 12b-4 records the deviation.
2. **Notches, not reaches, set the record size.** Every sub-resolution dip on the landform graph
   is drained by a notch route, and all 41,202 of them were recorded before this task. Ruling
   12b-2 records only a notch that a recorded river runs through, or a fresh enclosed pocket's
   outlet.
3. **The owner's bake also kept open lakes several times the Caspian's size** (2.02M, 1.24M,
   0.95M, 0.78M, 0.76M, 0.64M and 0.61M km², levels 290-629 m, depths 71-273 m) -- broad
   landform basins filled to their rims, not real lakes. Ruling 12b-5 notches an open hollow
   above `keep_max_area_m2 = 4.0e11` regardless of depth, exempting enclosed basins (the
   coastline lock already keeps the owner's 41.2M km² great lake, which is correctly exempt).

**Node budget (Ruling 12b-3):** `DEFAULT_TOTAL_NODES = 1_000_000` in `mod.rs` -- the 1M row's
372 MB studio heap is under the 512 MB ceiling with margin, and its 80 s wasm bake is well
inside the brief's time allowance. Bifurcation ratios (4.68-12.1 at 1M) are reported, not
forced toward the 3-5 target from section 6.5 -- the gap between what a graph this coarse can
resolve and that target is a known property of the resolution, not something this task's
calibration closes; plan 1b's finer tracing is where it narrows.

**Pit-lake comparison:** 352 kept bodies at 1M nodes (286 at 500k) is consistent with the
native survey's own finding above -- nowhere near the spec's old figure of 1,361 bodies (638
single-node) measured on a fine, undetrended mesh, for the same reason given there: the coarse
`LandGraph` samples the landform only, never detail noise.

**Post-12b verification (the controller's, browser studio, wasm, the same owner world, after
all five 12b rulings landed):** at 1,000,000 nodes with `earth_like` thresholds, the bake took
**83 s**, heap **355 MB**, record **23.6 MB**. **345 bodies** (286 lakes, 59 salt), **4,931
reaches** (3,451 streams / 1,359 rivers / 121 great), top order **5**, bifurcation **4.58-14**,
**30,361 notches** recorded. The great lake is fresh, forced and enclosed at **41.33M km²**;
the largest lake above the datum is **344,503 km²** -- under `keep_max_area_m2` (4.0e11 m²), so
Ruling 12b-5 is binding as intended.

**These owner-world numbers will be re-measured after the final review's fix wave** (the
drainage-cycle fix C1, and I1/I4/I7). C1 notches nested shore lakes that sit on an enclosed
pocket's outlet path and re-judges pockets fed by pockets, so the body and notch counts above
can move; none of the figures in this section has been re-measured since.

## Rebuild, parity, and count check (Step 4, native parts)

- **Wasm rebuild:** `cd viewer && npm run build:wasm`. `worldbuilder_engine.wasm` came out
  **byte-identical** (317,804 bytes, sha256
  `5b95831c9fca50b21a4b5f640cce23b3c442065dfaf0343436ce540049a06d8c` -- same hash as before
  this task). Only `MANIFEST.txt`'s source fingerprint and inputs count moved (51 -> 52
  fingerprinted inputs, since `hydro_survey.rs` is a new file under `src/`), which is expected
  and matches Task 11's own note about a fingerprinted-input-only change.
- **Parity re-run:**
  ```
  cd crates/worldbuilder-engine/parity
  cargo run --release -p worldbuilder-engine --example parity_dump --features wasm > native.txt
  node parity.mjs native.txt
  ```
  Result: `parity: 148707 values compared through the shipped exports, 0 divergent` --
  unchanged from Task 11's own figure, as expected (this task adds a `[[bin]]`, not a new
  export or record type).

  **Correction (final review, 2026-09-10).** 148,707 was the corpus at Task 12, before Task
  12b. It is not the current figure and must not be read as one: Task 12b's Ruling 12b-2 (record
  only the notches on a recorded river or an outlet cut) shrank the `H plain` record from
  21,048 to 688 compared values, so the corpus is **128,347 compared, 0 divergent**, and the
  `--mutate seed` control **122,825** divergent at 12b. After the final review's fix wave the
  corpus is still **128,347 / 0** and the seed control is **122,830** (the seed-moved world's
  bake changed; see `.superpowers/sdd/2026-09-10-water-1a-coarse-bake/final-fix-report.md`).
  The other six controls are unchanged at 216 / 60 / 6,186 / 13,128 / 3,752 / 648. Confirmed against the gate's own assertion:
  `python .github/scripts/assert_counts.py parity --output parity.out --expect-label parity
  --expect-compared 148707 --expect-divergent 0` printed `count OK: the corpus is the size the
  record says it is`.
- **Count-pin check (default `--no-default-features` config, per the brief's instruction to
  check "at least the default config"):**
  ```
  cargo test -p worldbuilder-engine --no-default-features -- --list > list-all.txt
  cargo test -p worldbuilder-engine --no-default-features -- --list --ignored > list-ignored.txt
  python .github/scripts/assert_counts.py cargo-list --all list-all.txt --ignored list-ignored.txt \
    --expect-passed 681 --expect-ignored 5
  ```
  Result: `listed: 686 tests over 16 binaries, 5 ignored -> 681 run` / `count OK: 681 passed /
  0 failed / 5 ignored, as recorded`. **The pin did not move** (681, unchanged from Task 11) --
  `hydro_survey.rs` carries zero `#[test]`s, so it adds a sixteenth test binary to the listing
  without adding a test. `gates.yml` and the README mirror were left untouched, per the
  brief's own conditional ("if anything moved, update ... "; nothing moved).
- **Full suite:** `cargo test -p worldbuilder-engine --no-fail-fast` -- all passed, including
  `tests/no_std_math.rs`'s six tests (confirming `hydro_survey.rs` itself carries no banned
  float call and every `as u32`/`as u64` cast in it is marked `// cast-ok:`).

## Concerns

- The 15-minute per-run stop-and-report contingency in the brief was never exercised: every
  run finished well under a minute. If the controller's owner-world (wasm) run is much slower
  per node than the native figures above (plausible -- wasm plus the studio's own overhead,
  and a different, possibly larger world), the 5-minute owner-world ceiling could bind well
  before native evidence would have predicted, which is exactly why Step 2 is a separate,
  necessary measurement rather than something native numbers alone could settle.
- `ponds: 0` at every node count means the pond/lake split, as calibrated, is currently
  unreachable at coarse resolution -- worth the controller's attention when the owner-world
  numbers come in, in case the same holds there and the split needs its own calibration pass
  (out of this task's scope, which only asked to report what falls out).
- The `owner_survey` world used here (seed 562,423,712, radius 4.5 Mm, `TectonicParams::
  ranges()`) is a stand-in for exercising a tectonics-shaped landform natively; it is not the
  owner's actual saved world and none of its body/reach counts should be read as predictions
  for Step 2's numbers, which come from a different radius (9,309 km) and a different,
  hand-authored landform.

## Files changed

- `crates/worldbuilder-engine/src/bin/hydro_survey.rs` (new): the survey binary.
- `viewer/public/wasm/MANIFEST.txt`: re-blessed after `npm run build:wasm` (fingerprint/inputs
  moved; artifact bytes unchanged).
- `docs/superpowers/reports/2026-09-10-water-1a-calibration.md` (new, this file).

No change to `crates/worldbuilder-engine/src/hydrology/mod.rs`, `.github/workflows/gates.yml`,
`crates/worldbuilder-engine/README.md`, or
`docs/superpowers/specs/2026-09-10-automatic-water-design.md` in this half -- the calibration
table in the spec and `DEFAULT_TOTAL_NODES` are the controller's to add once the owner-world
numbers exist.

## Commit

```
git add crates/worldbuilder-engine/src/bin/hydro_survey.rs \
        viewer/public/wasm/MANIFEST.txt \
        docs/superpowers/reports/2026-09-10-water-1a-calibration.md
git commit -m "Water: a survey of the coarse bake, native half of the calibration"
```
Not pushed, per instructions.
