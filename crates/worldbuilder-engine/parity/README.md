# Native-against-WASM parity, as a harness rather than a report figure

The engine's WASM path rests on one correctness claim: **the arithmetic behind the
`extern "C"` exports gives bit-identical answers natively and in a browser.** That claim
was previously carried only in a task report. A number that cannot be re-derived is a
number that cannot be defended, so it lives here instead.

## What it compares

`examples/parity_dump.rs` (native, `--release`) and `parity/parity.mjs` (the committed
`.wasm`, in Node) call **the same shipped exports** — `wb_world_new`, `wb_elevation_m`,
`wb_structural_m`, `wb_bottom_at`, `wb_fill_tile_f32`, `wb_generator_version`, and (slice 5a)
`wb_erosion_run` — never an internal function, because the exports are what a browser
reaches.

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
| identity | `wb_generator_version` | 1 |
| | | **56,254** |

A scattered corpus never lands inside a placed feature, and that gap has survived every
earlier probe in this project — hence the second world and the second tile.

## Running it

```sh
cargo run --release -p worldbuilder-engine --example parity_dump --features wasm > native.txt
node crates/worldbuilder-engine/parity/parity.mjs native.txt
node crates/worldbuilder-engine/parity/parity.mjs native.txt --mutate seed        # control 1: a different planet
node crates/worldbuilder-engine/parity/parity.mjs native.txt --mutate erosion-k   # control 2 (slice 5a): one ULP of erodibility
```

`--mutate erosion-k` bumps `erodibility_per_yr` by exactly one ULP before replaying the
erosion record and touches nothing else — `world`/`E`/`S`/`B`/`T`/`version` records are
unaffected, so only the erosion group can move. Its job is different from `--mutate seed`'s:
where the seed control proves the harness can detect a completely different planet, this one
proves it can detect a one-ULP arithmetic change in a single group **without that group's own
iteration count moving**, so a real divergence cannot be explained away as "the two sides
just took a different number of steps."

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
Artifact: the committed `worldbuilder_engine.wasm`, 117,146 bytes (12 exports, 0 imports —
grew from 84,856 bytes / 11 exports when slice 5a added `wb_erosion_run`). Dump: 41,806
lines. **Re-run for slice 5a Task 6**, not carried forward from before the erosion group or
the second control existed.

```
$ node parity.mjs native.txt
provenance: the shipped .wasm matches its manifest and current source.
parity: 56254 values compared through the shipped exports, 0 divergent
artifact: .../viewer/public/wasm/worldbuilder_engine.wasm (117146 bytes)
  elevation/plain: 10000 compared, 0 divergent
  structural/plain: 10000 compared, 0 divergent
  elevation/harbour: 10800 compared, 0 divergent
  structural/harbour: 10000 compared, 0 divergent
  bottom/plain: 2000 compared, 0 divergent
  bottom/harbour: 2000 compared, 0 divergent
  tile/plain: 4225 compared, 0 divergent
  tile/harbour: 4225 compared, 0 divergent
  erosion/erosion: 3003 compared, 0 divergent
  version: 1 compared, 0 divergent
OK: zero divergent

$ node parity.mjs native.txt --mutate seed
provenance: the shipped .wasm matches its manifest and current source.
CONTROL (--mutate seed): 56254 values compared through the shipped exports, 53778 divergent
  elevation/plain: 10000 compared, 10000 divergent
  structural/plain: 10000 compared, 9058 divergent
  elevation/harbour: 10800 compared, 10800 divergent
  structural/harbour: 10000 compared, 10000 divergent
  bottom/plain: 2000 compared, 1489 divergent
  bottom/harbour: 2000 compared, 982 divergent
  tile/plain: 4225 compared, 4225 divergent
  tile/harbour: 4225 compared, 4224 divergent
  erosion/erosion: 3003 compared, 3000 divergent
  version: 1 compared, 0 divergent
  e.g. elevation plain ...: native ... wasm ...
control OK: the harness can be made to fail

$ node parity.mjs native.txt --mutate erosion-k
provenance: the shipped .wasm matches its manifest and current source.
CONTROL (--mutate erosion-k): 56254 values compared through the shipped exports, 216 divergent
  elevation/plain: 10000 compared, 0 divergent
  structural/plain: 10000 compared, 0 divergent
  elevation/harbour: 10800 compared, 0 divergent
  structural/harbour: 10000 compared, 0 divergent
  bottom/plain: 2000 compared, 0 divergent
  bottom/harbour: 2000 compared, 0 divergent
  tile/plain: 4225 compared, 0 divergent
  tile/harbour: 4225 compared, 0 divergent
  erosion/erosion: 3003 compared, 216 divergent
  version: 1 compared, 0 divergent
  e.g. erosion height erosion[13]: native 407336eb79a24d0d wasm 407336eb79a24d0c
control OK: the harness can be made to fail
```

(All three exit 0 — the plain run because it is genuinely clean, both controls because a
control that successfully fails the comparison is itself the passing outcome; see
`parity.mjs`'s own exit-code table.)

**The control matters more than the headline.** `--mutate seed` changes the world seed by
one and nothing else; **53,778 of 56,254** values diverge, and **every sampled group moves
except `version`**, which a seed cannot move — including, now, all 3,000 erosion heights
(3,000 of the group's 3,003, not all 3,003: the run's own status/iterations/converged do not
move under a seed change, only the heights do). The residual agreements sit almost entirely
in `bottom`, whose 4,000 values are 1,000 discrete status codes and 3,000 fractions bounded
into [0, 1]. Not chased further: the control's job is to show that this harness can notice a
wrong answer, and it does, in every group that carries a continuous height.

**`--mutate erosion-k` isolates arithmetic from iteration count.** Of the erosion group's
3,003 values, exactly **216 of the 3,000 heights** diverge and the run's status, iteration
count and converged flag do not — both sides still ran the same number of steps and reached
the same (non-)convergence verdict, so the 216-height divergence is a real difference in the
implicit update's output, not an artifact of one side stopping earlier or later than the
other. This is a narrower, more targeted claim than the seed control's "everything moved":
it shows the harness can catch a one-ULP change in one physical parameter of one export,
without also being fooled by (or confused with) a difference in how many times the loop ran.

The per-group tally exists so that a divergent count is never a single unexplained number.
A run where only one group moved would be a finding, not a pass.

## Why this is not a `cargo test`

Running the `.wasm` inside the suite needs either a WASM runtime crate as a
dev-dependency — a large new dependency for a crate whose entire dependency list is two
pinned crates — or a test that shells out to `node` and **skips when it is absent**. A
test that can silently do nothing is the exact shape this project has been bitten by, so
neither was taken. This is a script with a documented invocation and its output recorded
above, to be re-run by whoever touches `wasm.rs` or rebuilds the artifact.
