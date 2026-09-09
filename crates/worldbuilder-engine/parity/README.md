# Native-against-WASM parity, as a harness rather than a report figure

The engine's WASM path rests on one correctness claim: **the arithmetic behind the
`extern "C"` exports gives bit-identical answers natively and in a browser.** That claim
was previously carried only in a task report. A number that cannot be re-derived is a
number that cannot be defended, so it lives here instead.

## What it compares

`examples/parity_dump.rs` (native, `--release`) and `parity/parity.mjs` (the committed
`.wasm`, in Node) call **the same shipped exports** — `wb_world_new`, `wb_elevation_m`,
`wb_structural_m`, `wb_bottom_at`, `wb_fill_tile_f32`, `wb_generator_version`, (slice 5a)
`wb_erosion_run`, (slice 5b) `wb_water_run`, and (slice mountains Task 6)
`wb_world_new_tectonic`, `wb_tectonic_preset` and `wb_tectonic_check`, and (photoreal slice
Task 6) `wb_world_new_coast`, `wb_coast_preset` and `wb_coast_check` — never an internal
function, because the exports are what a browser reaches.

The corpus is defined once, in the native dump, and carried to the replaying side as
16-hex-digit bit patterns. Node parses no decimal text and recomputes no input, so a
mismatch is a real disagreement and not a printf.

Population, per run (`SEED = 20260904`, `radius = 6_371_000 m`, `plate_count = 12`,
`land_fraction = 0.29`):

| group | what | values |
|---|---|---|
| scattered, open water | 10,000 uniform lat/lon points, `wb_elevation_m` at `res = 250` + `wb_structural_m` | 20,000 |
| **inside the placed harbour** | 10,000 points within ±0.01° of the extraction's harbour (a 900×260 m `CARVE` to −12 m with a 200×60 m `RAISE` to +4 m inside it), same two exports | 20,000 |
| the resolution sentinel | 200 harbour points × `{0.0, −1.0, +inf, NaN}` | 800 |
| the inspection tap | 500 points per world, status + three fractions each | 4,000 |
| **tiles** | 65×65 `wb_fill_tile_f32` in each world (f32 cells) | 8,450 |
| **erosion** (slice 5a) | one `wb_erosion_run` call, 3,000 nodes, this crate's default test constants, capped at 20 iterations (chosen to hit the cap without converging, so `iterations`/`converged` are themselves part of the comparison) — 3,000 heights plus status/iterations/converged | 3,003 |
| **the relief presets** (relief slice) | `wb_relief_preset` at both selectors — status + ten f64 each | 22 |
| **a non-canonical relief world** (relief slice) | 5,000 scattered points through `wb_world_new_relief` carrying `ReliefParams::hills()`, `wb_elevation_m` + `wb_structural_m` | 10,000 |
| **its tile** | one 65×65 `wb_fill_tile_f32` on the hills world — the path the viewer's tile workers take with a relief block on the spec | 4,225 |
| **water** (slice 5b) | one `wb_water_run` call, 30,000 nodes, datum 0.0, `pond_max_surface_area_m2 = 1.0e5` — 156 bodies × 7 fields plus status/body-count/datum | 1,095 |
| **the tectonic presets** (slice mountains) | `wb_tectonic_preset` at both selectors — status + sixteen f64 each | 34 |
| **the tectonic checker** (slice mountains) | `wb_tectonic_check` on six records, three accepted and three refused — status each | 6 |
| **a non-canonical tectonic world** (slice mountains) | 5,000 scattered points through `wb_world_new_tectonic` carrying `TectonicParams::ranges()`, `wb_elevation_m` + `wb_structural_m` | 10,000 |
| **its belt** | 2,000 points in a 2° box on the range that block builds, same two exports | 4,000 |
| **its tile** | one 65×65 `wb_fill_tile_f32` across the belt — the path the viewer's tile workers take with a tectonic block on the spec | 4,225 |
| **the coast presets** (photoreal slice) | `wb_coast_preset` at both selectors — status + six f64 each | 14 |
| **the coast checker** (photoreal slice) | `wb_coast_check` on six records, three accepted and three refused — status each | 6 |
| **a non-canonical coast world** (photoreal slice) | 5,000 scattered points through `wb_world_new_coast` carrying `CoastParams::fractal()`, `wb_elevation_m` + `wb_structural_m` | 10,000 |
| **its shore box** | 2,000 points in a 20° box on the largest mover, same two exports | 4,000 |
| **its tile** | one 65×65 `wb_fill_tile_f32` across the same box — the path the viewer's tile workers take with a coast block on the spec | 4,225 |
| **the gully presets** (gully slice) | `wb_gully_preset` at both selectors -- status + twelve f64 each (TEN until the merging slice appended `harmonic_weight` and `harmonic_band_m`; +4) | 26 |
| **the gully checker** (gully slice) | `wb_gully_check` on six records, three accepted and three refused -- status each | 6 |
| **a world with a drainage block** (gully slice) | 5,000 scattered points through `wb_world_new_gully` carrying `GullyParams::drainage()`, `wb_elevation_m` + `wb_structural_m` | 10,000 |
| **its flank belt** | 2,000 points in a 20-degree box on the ground the gate opens over, same two exports | 4,000 |
| **its tile** | one 65x65 `wb_fill_tile_f32` across the same box -- and the only block here whose term is FADED BY RESOLUTION, so a scalar corpus at one resolution cannot see the branch a tile takes | 4,225 |
| identity | `wb_generator_version` | 1 |
| | | **126,363** |

The last four rows are slice 5b Task 5's, and each closes a hole rather than adding volume:

- **The relief entries close a gap the relief slice flagged itself.** Its Task 4 changed the
  export surface for the first time in that slice and reported, correctly, that parity had not
  been re-run. Every world in the rows above it is built through `wb_world_new`, which sends
  `None` — so nothing here had ever decoded a relief block at all, and the block travels to the
  tile workers, which is why one of the three new relief groups is a tile.
- **The water entry closes a larger one.** `water.rs` was **unreachable from the export
  surface**: no export touched a `StreamGraph`'s lakes, so slice 5b's whole module had a
  native/WASM claim that was *unfalsifiable*, not merely unverified — exactly the position
  `erosion.rs` was in before `wb_erosion_run`. `wb_water_run` is the export that makes it
  checkable, and what it dumps is the **shipped manifest** (`water_manifest_from_graph`, after
  fill, overflow resolution, Ruling 7's tied-plateau merge and classification), not an
  intermediate. The pre-flight conflict scan named that trap for this pair of tasks.
- **All 156 bodies are lakes and none is a pond, and the dump asserts it.** Slice 5b Task 3
  calibrated `pond_max_surface_area_m2` at 1.0e5 m² on external ground and then measured that
  this generator's smallest body is nearly four orders of magnitude larger. The corpus carries
  the calibrated value; a corpus that moved the threshold until ponds appeared would be hiding
  that finding rather than testing it.

**The gully control's five counts are measurements too.** 1,235 of 5,000 scattered
elevations, 0 of 5,000 scattered structurals, 993 of 2,000 in the flank box, 0 of 2,000 flank
structurals, and **1,524** of 4,225 tile cells -- `--expect-divergent 3752`. **The tile count has
been 962, then 975, then 1,524**, and the two moves have different causes worth keeping apart.
The merging slice's 962 -> 975 was the second harmonic reshaping the drainage world's own ground,
so a moved `steer_lattice_m` changed a different set of cells of the same size. The
local-reference slice's 975 -> 1,524 is a change in what the mutated word DOES: `steer_lattice_m`
now feeds the local elevation reference as well as the fall line -- the reference is the mean of
the four samples the gradient differences -- so bumping it by a ULP moves strictly more ground.
The two scalar counts did not move at all, and that is the same fact seen from the other side:
both are sampled at 250 m, where the term is faded almost out, and the tile is the only group
asked at a resolution fine enough for the reference to bite. In neither slice were the extra
preset words the difference: a preset cannot move under a steering mutation and
`--mutate gully-steer` never touches words 10 and 11. The other four counts are unmoved, and so are all five other controls. The 24.7% on a uniform
global scatter is the right shape: the gate is `smooth((structural - 200) / 900)` and 29.15% of
this world is land whose median is 441 m, so about a quarter of a scatter is gated ground and
the rest is sea and shore where the kernel does not run. **The first cut of the flank box was
four degrees and it moved 2,000 of 2,000** -- a four-degree box on that witness sits entirely
inside the landmass -- and the dump's own both-ends-refused guard caught it and refused to write
the corpus, for the second time in this file's life.

**Two numbers in this corpus are MEASUREMENTS of the world, and their gates must be
RE-DERIVED rather than merely re-run.** Every other row above is a count this harness chose:
10,000 points because the corpus asks for 10,000, 65×65 because a tile is 65×65.

**156 is not.** It is how many lake bodies seed 20260904 happens to produce at 30,000 nodes
with datum 0.0, and it enters the total twice over — `156 × 7 + 3 = 1,095` in the `water/plain`
row, and `60 of 156` in the water control's own gate. Change the mesh, the sampler, the seed,
the node count or the datum and **both pins move**, along with `gates.yml`'s
`--expect-compared 108106` and `--expect-divergent 60`.

**Neither are the tectonic control's five counts.** 132 / 132 / 1,269 / 1,269 / 3,384 are
properties of where this world's convergent continental margins fall relative to a scatter and
a box, not of any choice made here. They do not enter the corpus size — every tectonic row's
*size* is chosen — but they are `--expect-divergent 6186`, and the seed control's own
`--expect-divergent 103931` carries the same dependence.

**Nor are the coast control's five.** 3,185 / 3,185 / 1,643 / 1,643 / 3,472 are properties of
how much of this world lies inside the coastal window, and they are `--expect-divergent 13128`.

The correct response when that happens is to re-derive the corpus arithmetic from its
definition — line by line, the way `gates.yml`'s own inline commentary sets it out — measure
the new body count natively, and *then* check the run against the derivation. Pasting in
whatever number the new run printed turns a gate into a rubber stamp: the pin exists precisely
so that a corpus which quietly shrank cannot pass. This is the most brittle gate here and the
only one whose value nothing in the source constrains, which is why it is called out where the
next person to change the mesh will meet it.

A scattered corpus never lands inside a placed feature, and that gap has survived every
earlier probe in this project — hence the second world and the second tile.

### The five tectonic rows, and the three tasks that flagged the gap without owning it

Mountains Task 4 reported that the tectonic channel had no parity coverage. Task 3 reported it
again, larger. Task 5 reported it a third time, two fields larger still. **The corpus watched
71,596 values and not one of them went through a tectonic export**, while `TectonicParams::ranges()`
is a preset the owner presses on the panel and it drives seven of `TectonicParams`' sixteen
words across the boundary.

**Why this is worth doing when both sides are the same Rust.** They are — over the same
pure-Rust `libm`, which is why native-against-WASM here is *strict bit-for-bit* even where
transcendentals are in the path, a different and stronger contract than the bounded one
Python-against-Rust conformance holds. What is not shared is the **decode**: the block crosses
as sixteen f64 in linear memory, is read back through a raw pointer, is bounds-checked field by
field, and only then becomes a `TectonicParams`. That code exists on this boundary and nowhere
else, and until Task 6 nothing exercised it.

**The belt box is a measurement, not a round number.** `src/bin/mountain_probe.rs`'s
`witness_between` scans this fixture world on a 0.5° global grid for the site where turning
`margin_warp_m` off moves `elevation_m` the most, and answers **−5.00, 66.00 — 821.955 m with
the warp off, 2,432.773 m with it on**. A uniform scatter over a planet does not land on a
100 km belt; that is the same gap the placed harbour's second world exists for. The scattered
tectonic points are kept anyway, and they earn their place under the control: they are the
evidence that the block does **not** reach the rest of the planet.

The two `worldt` records name one configuration under two names on purpose, so the scattered
points and the belt points tally as separate groups. One mixed group would have hidden exactly
what the control is there to show.

### The five coast rows, and the task that owned the gap the same day it opened

The tectonic channel sat unwatched for three tasks before anybody owned it. The coast channel
did not: `wb_world_new_coast`, `wb_coast_preset` and `wb_coast_check` and these five rows were
written in the same commit, because **a new crossing value with no corpus coverage is a
crossing value nothing compares.**

What is not shared between the two sides is again the **decode** — six f64 in linear memory,
read back through a raw pointer, bounds-checked, and only then a `CoastParams`. Two of those
bounds are unlike anything the other channels carry and are the reason this decode is worth
comparing rather than assuming:

- **`octaves` is a per-sample loop bound narrowed from an f64.** `value as u32` saturates in
  Rust, so `1e300` would arrive as `u32::MAX` and `Noise::fbm` would walk four billion octaves
  for **one elevation sample** — measured natively in release at ~1.6 × 10⁻⁸ s an octave, about
  68 s per sample, so a single 65×65 tile is roughly 80 hours. A hung tab, not a slow world.
- **The finest octave's frequency is a PRODUCT**, `frequency × lacunarity^(octaves − 1)`, and
  three fields each inside their own domain can compound past the point where `Noise::at`'s
  `i64` lattice index saturates and `ix + 1` overflows. `CC product` in the corpus is exactly
  that record, and it must be refused on both sides.

**The shore box is 20° and the first cut of it was 2°.** The coastal window is
`|above_shore| ≤ window_spreads × spread`, and that band is wide: an independent 0.5° global scan
of this fixture finds **162,159 of 258,480 sites** moving under `CoastParams::fractal()`. A 2°
box on the largest mover — −71.50, 38.00, where the ground moves 1,267.34 m — is therefore
entirely *inside* the band, and every one of its 2,000 points moved under the control, as did
every one of a 1° tile's 4,225 cells. The dump's own both-ends-refused guard caught that and
refused to write the corpus. That is the guard doing what it exists for, and it is recorded here
rather than quietly fixed.

The two `worldc` records name one configuration under two names for the same reason the `worldt`
pair does.

## Running it

```sh
cargo run --release -p worldbuilder-engine --example parity_dump --features wasm > native.txt
node crates/worldbuilder-engine/parity/parity.mjs native.txt
node crates/worldbuilder-engine/parity/parity.mjs native.txt --mutate seed        # control 1: a different planet
node crates/worldbuilder-engine/parity/parity.mjs native.txt --mutate erosion-k   # control 2 (slice 5a): one ULP of erodibility
node crates/worldbuilder-engine/parity/parity.mjs native.txt --mutate water-pond # control 3 (slice 5b): one field of the manifest
node crates/worldbuilder-engine/parity/parity.mjs native.txt --mutate tectonic-warp # control 4 (mountains): one word of the block
```

`--mutate erosion-k` bumps `erodibility_per_yr` by exactly one ULP before replaying the
erosion record and touches nothing else — `world`/`E`/`S`/`B`/`T`/`version` records are
unaffected, so only the erosion group can move. Its job is different from `--mutate seed`'s:
where the seed control proves the harness can detect a completely different planet, this one
proves it can detect a one-ULP arithmetic change in a single group **without that group's own
iteration count moving**, so a real divergence cannot be explained away as "the two sides
just took a different number of steps."

`--mutate water-pond` replays the `W` (water manifest) record with
`pond_max_surface_area_m2` moved from Task 3's calibrated 1.0e5 m² to 2.0e10 m², and touches
nothing else. Its job is different again: `pond_max_surface_area_m2` reaches **exactly one
field** of the manifest (`classify_lake_kinds` compares it against a body's summed surface area
and writes `LakeKind`), so it proves the harness can notice a one-field change in one export
**while every other field of that same export, and every other group, compares equal**. And
unlike the other two, **its count is predicted before the run rather than reported after it** —
see "the third control" below.

`--mutate tectonic-warp` replays the two `worldt` records with word 14, `margin_warp_m`, set
to 0.0 and touches nothing else. Its job is different again: that word reaches the collision
profile of a world built **from a tectonic block**, and nothing else in this corpus — not
`wb_tectonic_preset`, which hands back `tectonics.rs`' own constants; not `wb_tectonic_check`,
whose records it does not touch; not any world built without a block. So it proves the harness
notices one word of one channel **while 83,675 values stay equal**. Like the water control, its
counts are predicted before the run rather than reported after it — see "the fourth control"
below.

`--mutate gully-steer` replays the two `worldg` records with word 9, `steer_lattice_m`, moved
from the shipped 2,000 m to 500 m -- a value **inside** the domain rather than outside it,
because the point of this control is that a plausible number a host could send moves the
ground, not that a refused one does. It touches nothing else. Its counts are predicted before
the run, and **two of the five predictions are zero on purpose**: the gully term is *detail*,
and `structural_m` is the very signal its steering lattice reads. A structural value that moved
under this control would mean the term had escaped its layer and was feeding its own input --
and that substitution, tried as a source mutation on the Rust side, **does not terminate**.

```sh
node crates/worldbuilder-engine/parity/parity.mjs native.txt --mutate coast-amplitude # control 5 (photoreal)
node crates/worldbuilder-engine/parity/parity.mjs native.txt --mutate gully-steer      # control 6 (gully)
```

`--wasm <path>` overrides the artifact; the default is the committed
`viewer/public/wasm/worldbuilder_engine.wasm`, i.e. the bytes a browser loads.

## The provenance gate: what bit-for-bit agreement does *not* prove

Before a single value is compared, this script asks a question the comparison itself cannot:
**were these bytes built from the source that is here now?**

They were not, for several commits of this project's life. The committed `.wasm` predated
`d0c2eff`, which added a net +28 lines to `wasm.rs`; five bytes inside the artifact — all
panic-location line numbers, which never execute — still named the old lines. It passed
this harness perfectly, because `native.txt` had been recorded from the same stale build.
A corpus and an artifact that are stale *together* agree with each other and with nothing
else, and there is no number in the output above that can tell you so.

So `parity.mjs` now imports `checkFreshness()` from `viewer/scripts/build-wasm.mjs` — the
same function `npm run check:wasm` runs — and **refuses to report parity at all** when it
returns problems. It imports rather than reimplements: two copies of a provenance rule
drift, and the copy that drifts is the one that stops refusing.

| situation | result |
|---|---|
| shipped artifact, current source | `provenance: the shipped .wasm matches its manifest and current source.`, then the run |
| source moved, artifact not rebuilt | `REFUSING TO REPORT PARITY -- STALE ARTIFACT`, exit 1 |
| shipped `.wasm` edited or swapped | `REFUSING … not the one this manifest describes`, exit 1 |
| `--wasm <other path>` | `REFUSING … no manifest describes those bytes`, exit 1 |
| `--wasm <other path> --no-provenance` | runs; every line is labelled `UNVERIFIED` |
| `--no-provenance` on the shipped artifact | refused, exit 2 — there is no such escape hatch |

A guard that cannot run has not passed: if `rustc` cannot be found, `checkFreshness()`
throws rather than guess, and this script turns that into a refusal too.

## Recorded output

Host: Windows 11 (10.0.26200), x86_64-pc-windows-msvc, cargo 1.98.0, Node v22.17.0.
Artifact: the committed `worldbuilder_engine.wasm`, **226,558 bytes (22 exports, 0 imports)** —
grew from 117,146 bytes / 12 exports at slice 5a, then through the relief slice's three
exports, slice 5b's `wb_water_run`, slice mountains' three tectonic exports, and the photoreal
slice's three coast exports. Dump: 79,838 lines. **Re-run in full for photoreal slice Task 6**,
not carried forward: this run is the first that compares a coast preset, a coast checker
answer, or a world built from a coast block at all. Every figure below is a count — a property
of the algorithm, not of the moment — except the byte size, which is a property of this
build.

```
$ node parity.mjs native.txt
provenance: the shipped .wasm matches its manifest and current source.
parity: 108106 values compared through the shipped exports, 0 divergent
artifact: D:\dev\worldbuilder_by_aetos\viewer\public\wasm\worldbuilder_engine.wasm (226558 bytes)
  elevation/plain: 10000 compared, 0 divergent
  structural/plain: 10000 compared, 0 divergent
  elevation/harbour: 10800 compared, 0 divergent
  structural/harbour: 10000 compared, 0 divergent
  bottom/plain: 2000 compared, 0 divergent
  bottom/harbour: 2000 compared, 0 divergent
  tile/plain: 4225 compared, 0 divergent
  tile/harbour: 4225 compared, 0 divergent
  erosion/erosion: 3003 compared, 0 divergent
  preset/0: 11 compared, 0 divergent
  preset/1: 11 compared, 0 divergent
  elevation/hills: 5000 compared, 0 divergent
  structural/hills: 5000 compared, 0 divergent
  tile/hills: 4225 compared, 0 divergent
  water/plain: 1095 compared, 0 divergent
  tpreset/0: 17 compared, 0 divergent
  tpreset/1: 17 compared, 0 divergent
  tcheck: 6 compared, 0 divergent
  elevation/ranges: 5000 compared, 0 divergent
  structural/ranges: 5000 compared, 0 divergent
  elevation/belt: 2000 compared, 0 divergent
  structural/belt: 2000 compared, 0 divergent
  tile/belt: 4225 compared, 0 divergent
  coast-preset/0: 7 compared, 0 divergent
  coast-preset/1: 7 compared, 0 divergent
  coast-check: 6 compared, 0 divergent
  elevation/fractal: 5000 compared, 0 divergent
  structural/fractal: 5000 compared, 0 divergent
  elevation/shore: 2000 compared, 0 divergent
  structural/shore: 2000 compared, 0 divergent
  tile/shore: 4225 compared, 0 divergent
  version: 1 compared, 0 divergent
OK: zero divergent

$ node parity.mjs native.txt --mutate seed
provenance: the shipped .wasm matches its manifest and current source.
CONTROL (--mutate seed): 108106 values compared through the shipped exports, 103931 divergent
artifact: D:\dev\worldbuilder_by_aetos\viewer\public\wasm\worldbuilder_engine.wasm (226558 bytes)
  elevation/plain: 10000 compared, 10000 divergent
  structural/plain: 10000 compared, 9058 divergent
  elevation/harbour: 10800 compared, 10800 divergent
  structural/harbour: 10000 compared, 10000 divergent
  bottom/plain: 2000 compared, 1489 divergent
  bottom/harbour: 2000 compared, 982 divergent
  tile/plain: 4225 compared, 4225 divergent
  tile/harbour: 4225 compared, 4224 divergent
  erosion/erosion: 3003 compared, 3000 divergent
  preset/0: 11 compared, 0 divergent
  preset/1: 11 compared, 0 divergent
  elevation/hills: 5000 compared, 5000 divergent
  structural/hills: 5000 compared, 4516 divergent
  tile/hills: 4225 compared, 4225 divergent
  water/plain: 1095 compared, 938 divergent
  tpreset/0: 17 compared, 0 divergent
  tpreset/1: 17 compared, 0 divergent
  tcheck: 6 compared, 0 divergent
  elevation/ranges: 5000 compared, 5000 divergent
  structural/ranges: 5000 compared, 4508 divergent
  elevation/belt: 2000 compared, 2000 divergent
  structural/belt: 2000 compared, 2000 divergent
  tile/belt: 4225 compared, 4225 divergent
  coast-preset/0: 7 compared, 0 divergent
  coast-preset/1: 7 compared, 0 divergent
  coast-check: 6 compared, 0 divergent
  elevation/fractal: 5000 compared, 5000 divergent
  structural/fractal: 5000 compared, 4516 divergent
  elevation/shore: 2000 compared, 2000 divergent
  structural/shore: 2000 compared, 2000 divergent
  tile/shore: 4225 compared, 4225 divergent
  version: 1 compared, 0 divergent
  e.g. elevation plain 4050f64e53982ff4,c059b998e99bf26c res 406f400000000000: native c064279493d609d0 wasm c0ac07bc33592429
  e.g. structural plain 4050f64e53982ff4,c059b998e99bf26c: native c06604fe81b06af5 wasm c0abd90419cad964
  e.g. elevation plain bff3b8f228d643c0,4058a17ecef7af14 res 406f400000000000: native 406a8086b8e83a3b wasm 4084d7e8f181a562
  e.g. structural plain bff3b8f228d643c0,4058a17ecef7af14: native 406709794b0bbbdc wasm 40850405c6c44540
  e.g. elevation plain c05289328bd0cf3d,c0593753877872b7 res 406f400000000000: native c09361c7e71faf85 wasm c0a6cd1fcce0427c
control OK: the harness can be made to fail

$ node parity.mjs native.txt --mutate erosion-k
provenance: the shipped .wasm matches its manifest and current source.
CONTROL (--mutate erosion-k): 108106 values compared through the shipped exports, 216 divergent
artifact: D:\dev\worldbuilder_by_aetos\viewer\public\wasm\worldbuilder_engine.wasm (226558 bytes)
  elevation/plain: 10000 compared, 0 divergent
  structural/plain: 10000 compared, 0 divergent
  elevation/harbour: 10800 compared, 0 divergent
  structural/harbour: 10000 compared, 0 divergent
  bottom/plain: 2000 compared, 0 divergent
  bottom/harbour: 2000 compared, 0 divergent
  tile/plain: 4225 compared, 0 divergent
  tile/harbour: 4225 compared, 0 divergent
  erosion/erosion: 3003 compared, 216 divergent
  preset/0: 11 compared, 0 divergent
  preset/1: 11 compared, 0 divergent
  elevation/hills: 5000 compared, 0 divergent
  structural/hills: 5000 compared, 0 divergent
  tile/hills: 4225 compared, 0 divergent
  water/plain: 1095 compared, 0 divergent
  tpreset/0: 17 compared, 0 divergent
  tpreset/1: 17 compared, 0 divergent
  tcheck: 6 compared, 0 divergent
  elevation/ranges: 5000 compared, 0 divergent
  structural/ranges: 5000 compared, 0 divergent
  elevation/belt: 2000 compared, 0 divergent
  structural/belt: 2000 compared, 0 divergent
  tile/belt: 4225 compared, 0 divergent
  coast-preset/0: 7 compared, 0 divergent
  coast-preset/1: 7 compared, 0 divergent
  coast-check: 6 compared, 0 divergent
  elevation/fractal: 5000 compared, 0 divergent
  structural/fractal: 5000 compared, 0 divergent
  elevation/shore: 2000 compared, 0 divergent
  structural/shore: 2000 compared, 0 divergent
  tile/shore: 4225 compared, 0 divergent
  version: 1 compared, 0 divergent
  e.g. erosion height erosion[13]: native 407336eb79a24d0d wasm 407336eb79a24d0c
  e.g. erosion height erosion[53]: native 4030732c664ab4a0 wasm 4030732c664ab49b
  e.g. erosion height erosion[54]: native 40015ba507e014fb wasm 40015ba507e014f5
  e.g. erosion height erosion[66]: native c0344f8fc14b3a0f wasm c0344f8fc14b3a12
  e.g. erosion height erosion[73]: native c00cef8df0d9e974 wasm c00cef8df0d9e982
control OK: the harness can be made to fail

$ node parity.mjs native.txt --mutate water-pond
provenance: the shipped .wasm matches its manifest and current source.
CONTROL (--mutate water-pond): 108106 values compared through the shipped exports, 60 divergent
artifact: D:\dev\worldbuilder_by_aetos\viewer\public\wasm\worldbuilder_engine.wasm (226558 bytes)
  elevation/plain: 10000 compared, 0 divergent
  structural/plain: 10000 compared, 0 divergent
  elevation/harbour: 10800 compared, 0 divergent
  structural/harbour: 10000 compared, 0 divergent
  bottom/plain: 2000 compared, 0 divergent
  bottom/harbour: 2000 compared, 0 divergent
  tile/plain: 4225 compared, 0 divergent
  tile/harbour: 4225 compared, 0 divergent
  erosion/erosion: 3003 compared, 0 divergent
  preset/0: 11 compared, 0 divergent
  preset/1: 11 compared, 0 divergent
  elevation/hills: 5000 compared, 0 divergent
  structural/hills: 5000 compared, 0 divergent
  tile/hills: 4225 compared, 0 divergent
  water/plain: 1095 compared, 60 divergent
  tpreset/0: 17 compared, 0 divergent
  tpreset/1: 17 compared, 0 divergent
  tcheck: 6 compared, 0 divergent
  elevation/ranges: 5000 compared, 0 divergent
  structural/ranges: 5000 compared, 0 divergent
  elevation/belt: 2000 compared, 0 divergent
  structural/belt: 2000 compared, 0 divergent
  tile/belt: 4225 compared, 0 divergent
  coast-preset/0: 7 compared, 0 divergent
  coast-preset/1: 7 compared, 0 divergent
  coast-check: 6 compared, 0 divergent
  elevation/fractal: 5000 compared, 0 divergent
  structural/fractal: 5000 compared, 0 divergent
  elevation/shore: 2000 compared, 0 divergent
  structural/shore: 2000 compared, 0 divergent
  tile/shore: 4225 compared, 0 divergent
  version: 1 compared, 0 divergent
  e.g. water plain body[1].kind: native 0000000000000000 wasm 3ff0000000000000
  e.g. water plain body[2].kind: native 0000000000000000 wasm 3ff0000000000000
  e.g. water plain body[3].kind: native 0000000000000000 wasm 3ff0000000000000
  e.g. water plain body[6].kind: native 0000000000000000 wasm 3ff0000000000000
  e.g. water plain body[8].kind: native 0000000000000000 wasm 3ff0000000000000
control OK: 60 of 1095 water values moved, exactly the bodies the native surface-area distribution predicted, and no value outside the water group moved at all

$ node parity.mjs native.txt --mutate tectonic-warp
provenance: the shipped .wasm matches its manifest and current source.
CONTROL (--mutate tectonic-warp): 108106 values compared through the shipped exports, 6186 divergent
artifact: D:\dev\worldbuilder_by_aetos\viewer\public\wasm\worldbuilder_engine.wasm (226558 bytes)
  elevation/plain: 10000 compared, 0 divergent
  structural/plain: 10000 compared, 0 divergent
  elevation/harbour: 10800 compared, 0 divergent
  structural/harbour: 10000 compared, 0 divergent
  bottom/plain: 2000 compared, 0 divergent
  bottom/harbour: 2000 compared, 0 divergent
  tile/plain: 4225 compared, 0 divergent
  tile/harbour: 4225 compared, 0 divergent
  erosion/erosion: 3003 compared, 0 divergent
  preset/0: 11 compared, 0 divergent
  preset/1: 11 compared, 0 divergent
  elevation/hills: 5000 compared, 0 divergent
  structural/hills: 5000 compared, 0 divergent
  tile/hills: 4225 compared, 0 divergent
  water/plain: 1095 compared, 0 divergent
  tpreset/0: 17 compared, 0 divergent
  tpreset/1: 17 compared, 0 divergent
  tcheck: 6 compared, 0 divergent
  elevation/ranges: 5000 compared, 132 divergent
  structural/ranges: 5000 compared, 132 divergent
  elevation/belt: 2000 compared, 1269 divergent
  structural/belt: 2000 compared, 1269 divergent
  tile/belt: 4225 compared, 3384 divergent
  coast-preset/0: 7 compared, 0 divergent
  coast-preset/1: 7 compared, 0 divergent
  coast-check: 6 compared, 0 divergent
  elevation/fractal: 5000 compared, 0 divergent
  structural/fractal: 5000 compared, 0 divergent
  elevation/shore: 2000 compared, 0 divergent
  structural/shore: 2000 compared, 0 divergent
  tile/shore: 4225 compared, 0 divergent
  version: 1 compared, 0 divergent
  e.g. elevation ranges c024ffe9697c6af8,406500d5a186d7be res 406f400000000000: native 404f780c121dbec1 wasm 404f78ac2969b75c
  e.g. structural ranges c024ffe9697c6af8,406500d5a186d7be: native 404d23b23714aaa5 wasm 404d24503fe554f3
  e.g. elevation ranges c023c00d930ebbc0,c050a9e716173668 res 406f400000000000: native c0b1f036f28ddf94 wasm c0b1ef31952e1b8f
  e.g. structural ranges c023c00d930ebbc0,c050a9e716173668: native c0b1f71cb3137dc3 wasm c0b1f61753d26d79
  e.g. elevation ranges c03f6e60156fe3ba,405acc2e9fa9dbd8 res 406f400000000000: native c0b245f22c311c6b wasm c0b245f1fe21c9d5
control OK: elevation/ranges 132/5000, structural/ranges 132/5000, elevation/belt 1269/2000, structural/belt 1269/2000, tile/belt 3384/4225 moved, exactly as the native side predicted, and every other group -- both tectonic presets, the checker, and every world without a tectonic block -- moved nothing at all

$ node parity.mjs native.txt --mutate coast-amplitude
provenance: the shipped .wasm matches its manifest and current source.
CONTROL (--mutate coast-amplitude): 108106 values compared through the shipped exports, 13128 divergent
artifact: D:\dev\worldbuilder_by_aetos\viewer\public\wasm\worldbuilder_engine.wasm (226558 bytes)
  elevation/plain: 10000 compared, 0 divergent
  structural/plain: 10000 compared, 0 divergent
  elevation/harbour: 10800 compared, 0 divergent
  structural/harbour: 10000 compared, 0 divergent
  bottom/plain: 2000 compared, 0 divergent
  bottom/harbour: 2000 compared, 0 divergent
  tile/plain: 4225 compared, 0 divergent
  tile/harbour: 4225 compared, 0 divergent
  erosion/erosion: 3003 compared, 0 divergent
  preset/0: 11 compared, 0 divergent
  preset/1: 11 compared, 0 divergent
  elevation/hills: 5000 compared, 0 divergent
  structural/hills: 5000 compared, 0 divergent
  tile/hills: 4225 compared, 0 divergent
  water/plain: 1095 compared, 0 divergent
  tpreset/0: 17 compared, 0 divergent
  tpreset/1: 17 compared, 0 divergent
  tcheck: 6 compared, 0 divergent
  elevation/ranges: 5000 compared, 0 divergent
  structural/ranges: 5000 compared, 0 divergent
  elevation/belt: 2000 compared, 0 divergent
  structural/belt: 2000 compared, 0 divergent
  tile/belt: 4225 compared, 0 divergent
  coast-preset/0: 7 compared, 0 divergent
  coast-preset/1: 7 compared, 0 divergent
  coast-check: 6 compared, 0 divergent
  elevation/fractal: 5000 compared, 3185 divergent
  structural/fractal: 5000 compared, 3185 divergent
  elevation/shore: 2000 compared, 1643 divergent
  structural/shore: 2000 compared, 1643 divergent
  tile/shore: 4225 compared, 3472 divergent
  version: 1 compared, 0 divergent
  e.g. elevation fractal 402d1e4b93ccc168,40663abac478f8c8 res 406f400000000000: native c08f605fd11306f0 wasm c08e8151153cdd19
  e.g. structural fractal 402d1e4b93ccc168,40663abac478f8c8: native c08f6a2803cefba4 wasm c08e8b21d4f0e9b4
  e.g. elevation fractal 404e309a81231f0c,404439e73e2152f0 res 406f400000000000: native 4080049556a64ab9 wasm 40807d8ad1cb46c3
  e.g. structural fractal 404e309a81231f0c,404439e73e2152f0: native 407dc6f381b8e19b wasm 407eb0026515d51a
  e.g. elevation fractal 40529b2df7f58e5c,c0510a222ac62d95 res 406f400000000000: native 4076dfdbda1e8545 wasm 40775e4f1f579b4c
control OK: elevation/fractal 3185/5000, structural/fractal 3185/5000, elevation/shore 1643/2000, structural/shore 1643/2000, tile/shore 3472/4225 moved, exactly as the native side predicted, and every other group -- both coast presets, the checker, and every world without a coast block -- moved nothing at all
```

(All six exit 0 — the plain run because it is genuinely clean, the five controls because a
control that successfully fails the comparison is itself the passing outcome; see
`parity.mjs`'s own exit-code table.)

**The controls matter more than the headline.** `--mutate seed` changes the world seed by one
and nothing else; **86,190 of 89,861** values diverge, and every group that carries a continuous
height moves. Five groups do not, and all five are informative rather than residual: `version`,
which a seed cannot move; **`preset/0` and `preset/1`** — `wb_relief_preset` hands back
`detail.rs`'s own constants, and a world seed reaches none of them; **`tpreset/0` and
`tpreset/1`**, the same statement about `tectonics.rs`; and **`tcheck`**, which is a decision
about a caller-supplied record and has no world in it at all. The partial groups sit where a value is bounded or discrete: `bottom`'s 4,000 values are
1,000 status codes and 3,000 fractions in [0, 1]; `structural` agrees on deep-ocean floor on
both the `plain` world (942 of 10,000) and the `hills` one (484 of 5,000); and the water group's
157 unmoved values of 1,095 are, measured field by field, the status, the datum, and 155 `kind`
codes. The seed-plus-one planet has **155** bodies against this one's 156, so the body count
diverges and every `root_node`, `level_m` and extent bound diverges — but `kind` is `Lake` on
both planets for every body, because neither mesh makes a pond at the calibrated threshold. A
discrete column that reads the same on two different planets is exactly what one expects, and
naming it is cheaper than leaving 157 unexplained.

**`--mutate erosion-k` isolates arithmetic from iteration count**, and its count is *unchanged
at 216* across two rounds of this corpus's growth — slice 5b's three groups and the mountains
slice's five. That is the expected outcome and worth stating: those groups are downstream of
neither `k` nor erosion, so a control whose count had moved with them would have been reaching
something it does not name. `--mutate water-pond` is unchanged at 60 for the same reason.

### The third control, and why its number is a prediction rather than a report

`--mutate seed` moves 95.6% of everything. `--mutate erosion-k` moves a fraction of one group
that nobody could state in advance. **A control that only ever moves everything is nearly as
uninformative as one that never moves anything**, and neither of those two touches the water
path at all — so before Task 5 there was no control that said anything about `water.rs`.

`--mutate water-pond` moves `pond_max_surface_area_m2` from the corpus's calibrated 1.0e5 m² to
2.0e10 m² and changes nothing else. Stated **before** the run, and then checked against it:

| field | prediction | result |
|---|---|---|
| `Body::kind` | 60 of 156 move | 60 |
| `root_node`, `level_m`, all four extent bounds | 0 move | 0 |
| body count, datum, status | 0 move | 0 |
| every group outside `water/plain` | 0 move | 0 |

That is slice 5a's discipline exactly: its erosion control moved 216 of 56,254 heights *while
iterations and convergence compared equal on both sides*, and this one moves one column of one
table while every other column of that table, and every other table, compares equal.

**The 60 is derived independently of the classifier it predicts.** `examples/parity_dump.rs`
computes it from `water::lake_body_surface_areas_m2` — the summed surface area per physical
body, which is arithmetic over areas, not a call to `classify_lake_kinds` — asserts that the
classifier at that threshold agrees with it, and writes it into the corpus as a `WCTL` record.
`parity.mjs` then checks **every** per-group tally against that prediction and exits 1 if any
group moved by a different amount, water or not. So the number is asserted twice, by two
scripts, from two directions, and a gate read off the control's own output would have been a
rubber stamp.

2.0e10 m² is not a round number picked for looks: slice 5b Task 3's survey put this mesh's body
surface areas over roughly one order of magnitude around a median of ~1.9e10 m², so a threshold
there splits the population. 1.0e5 m² would move nothing (this mesh makes no ponds at all) and
1.0e12 m² would move 155 of 156.

The per-group tally exists so that a divergent count is never a single unexplained number. A run
where only one group moved would be a finding, not a pass.

### The fourth control: one word of the tectonic block, and the half that must stay equal

`--mutate tectonic-warp` sets word 14 of both `worldt` records — `margin_warp_m`, the newest
field on the channel and the one Task 5 added to bend a belt that geometry makes straight — to
0.0, and touches nothing else. Stated **before** the run, per group, and then checked against
it:

| group | prediction | result |
|---|---|---|
| `elevation/ranges`, `structural/ranges` | 132 of 5,000 each | 132, 132 |
| `elevation/belt`, `structural/belt` | 1,269 of 2,000 each | 1,269, 1,269 |
| `tile/belt` | 3,384 of 4,225 | 3,384 |
| `tpreset/0`, `tpreset/1`, `tcheck` | 0 | 0 |
| every group of every world with no tectonic block | 0 | 0 |

**The second half of that table is the interesting half.** 83,675 values must not move, and
they include the two exports that sit next to the one being perturbed: a preset that hands
back constants and a checker that decides about a record this mutation never touches. A field
that reached either of them would be reaching something it does not name.

**2.6% from orbit and 63% on the belt is the shape this control was chosen for.** A uniform
scatter over a planet is mostly nowhere near a convergent continental margin, so the scattered
groups move a small minority; the belt box, placed by `witness_between` at −5.00, 66.00, moves
most of what is in it. A control that moved everything would have proved only that the corpus
noticed a different planet — which `--mutate seed` already does — and one that moved nothing
would have proved less than that.

**The five counts are derived twice, natively, before this script sees them.**
`examples/parity_dump.rs` computes each one through the exports *and* through the library's own
`Surface`, with the two blocks read from `TectonicParams::ranges()` rather than from the sixteen
words that crossed the boundary — so an `encode_tectonic`/`decode_tectonic` disagreement parts
the two counts and fails there rather than being absorbed into a divergent tally here. Beside
them it asserts a structural containment: `margin_warp_m` reaches `elevation_m` only through
the tectonic offset, so every point whose elevation moved must be a point whose structural
moved. Measured, they are the same points — the assertion deliberately does not require that,
because the reverse containment is not true in general. And it refuses to write a corpus where
any group's count is 0 or the whole group.

**This is a weaker independence than the water control's and it should be read as such.** That
one predicts a classifier's output from summed surface areas — a different quantity. This one
predicts an export's output from the library beneath it: a different *call path* over the same
arithmetic, which catches a decode or a boundary defect and would not catch a shared error in
`from_margin`. The containment and the "neither end" refusal are what carry the rest.

### The fifth control: one word of the coast block

`--mutate coast-amplitude` sets word 0 of both `worldc` records — `amplitude`, the one field
`CoastParams::fractal()` moves off canonical — back to canonical's own inert value, read from
`wb_coast_preset` on the native side rather than written down anywhere. Stated **before** the
run, per group, and then checked against it:

| group | prediction | result |
|---|---|---|
| `elevation/fractal`, `structural/fractal` | 3,185 of 5,000 each | 3,185, 3,185 |
| `elevation/shore`, `structural/shore` | 1,643 of 2,000 each | 1,643, 1,643 |
| `tile/shore` | 3,472 of 4,225 | 3,472 |
| `coast-preset/0`, `coast-preset/1`, `coast-check` | 0 | 0 |
| every group of every world with no coast block | 0 | 0 |

**63.7% from orbit is the right shape here, and 2.6% would be wrong.** That is the difference
between this control and the tectonic one above, and it is a fact about the two mechanisms
rather than about the corpus: a tectonic belt is a line on the planet, while the coastal window
is `|above_shore| ≤ window_spreads × spread` and covers the whole shelf. An independent 0.5°
global scan of this fixture finds **162,159 of 258,480 sites** moving under `fractal()`.

**The 94,978 values that must not move are still the informative half**, and they now include
both tectonic worlds as well as both coast presets and the checker.

**The counts are derived twice, natively, before this script sees them** — through the exports
and through the library's own `Surface`, with the block read from `CoastParams::fractal()`
rather than from the six words that crossed the boundary, so an `encode_coast`/`decode_coast`
disagreement parts the two counts and fails there.

**No structural containment is asserted, unlike the tectonic control's, and that is a
deliberate omission rather than a missing check.** `CoastParams` reaches `elevation_m` through
`Continentality::base_elevation` as well as through the shelf, so an elevation that moves
without a structural moving is expected. Claiming the subset the warp satisfies would be
claiming something false.

## Why this is not a `cargo test`

Running the `.wasm` inside the suite needs either a WASM runtime crate as a
dev-dependency — a large new dependency for a crate whose entire dependency list is two
pinned crates — or a test that shells out to `node` and **skips when it is absent**. A
test that can silently do nothing is the exact shape this project has been bitten by, so
neither was taken. This is a script with a documented invocation and its output recorded
above, to be re-run by whoever touches `wasm.rs` or rebuilds the artifact.
