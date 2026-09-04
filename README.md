# Worldbuilder by Aetos

**Take the Evennia world you have already spent years building, and put a whole planet
around it.**

Not "generate a planet for a new game". The generator is a subsystem; the product is that
your existing rooms, exits, zones, wilderness grids and builder workflows stay exactly as
they are, and gain a deterministic planet around them - continents, oceans, poles, rivers,
prevailing winds and ocean currents that agree with each other, and a coastline for your
port city to actually stand on.

Nothing is migrated. Nothing is rewritten. An area is *anchored* onto the globe, and a
handful of *seams* - a dock, a gate, a ferry landing - connect generated space to the
rooms you built by hand.

This repository is groundwork. The eventual product is a standalone Evennia contrib; what
is here now is the design, and the world needed to demonstrate the maritime contrib on
something better than a ramp.

## The constraint everything else answers to

Maritime does not read a map. It asks the world how high the ground is at a point, and it
asks a great many times: **one chart redraw calls `terrain_z_at` 9,216 times and takes
about 45 milliseconds**, for arbitrary points, at arbitrary zoom, wherever a ship happens
to be.

So the output of this generator cannot be a stored map. It has to be a **deterministic
function of position** - the same answer for the same point, for ever, computed in
microseconds, at any scale. A heightmap coarse enough to store is too blurry to have a
coastline at ship scale; one fine enough for ship scale is far too large to store for a
planet.

Determinism is not merely a performance trick. A chart in maritime is *wrong in the same
places every voyage*, which is what makes surveying, dead reckoning and taking a fix
mean anything. A world that answered differently on a second asking would take that away.

## What "earthlike" actually requires

Not coastlines. **Correlation.** On Earth the deserts sit near thirty degrees because
that is where Hadley cells return dry air to the ground; rainforests sit on the equator
and on windward coasts; mountains cast rain shadows; ocean gyres turn one way north of
the equator and the other way south; ice sits at the poles.

A generator that produces continents and then sprinkles biomes onto them looks false
immediately. One that derives climate from latitude, elevation and prevailing wind gets
Earth for nothing - and cheaply, because latitude and elevation are already to hand.

## The core, which is not the generator

The architecture is in `docs/design/anchoring-architecture.md`, and the rule at the top of
it is the product:

> Planet coordinates are authoritative for generated planetary space. Existing local
> coordinates remain authoritative inside imported areas.

A `WorldAnchor` gives a zone geographic *context*; a `WorldSeam` gives one room geographic
*precision*. That separation is what lets a fifteen-year-old room graph with no
coordinates at all sit on a planet and still have a dock at an exact latitude.

## Decisions taken so far

- **A true sphere.** Not a plane, not a cylinder. Longitude converges at the poles,
  courses are great circles, and sailing east for long enough brings you home.
- **Procedural, not stored.** See the constraint above.
- **Climate is derived, never authored.** Wind, current, temperature and rainfall all
  fall out of latitude, elevation and each other.
- **Additive to existing games.** Nothing is migrated, no exit is ever rewritten, and
  coordinates are never required of a game that does not have them.
- **The physical field is total.** `terrain_z_at` answers everywhere on the planet -
  under a castle, inside an abstract city - because a handcrafted harbour still needs the
  world to tell a ship how deep its own water is.
- **Determinism, not statelessness.** Generated-once-and-stored is still deterministic.
  Only the functional fields need answering cheaply at arbitrary coordinates.
- **Detail is texture; features are placed.** Noise makes roughness and nothing else. A
  bank, a bar or a reef is a thing somebody put somewhere, so finding one on a chart means
  something.
- **A chart has two channels.** Soundings come from sampling the terrain; isolated dangers
  come from a marks layer. A hundred-and-forty-metre pinnacle is not smoothed away by a
  four-hundred-metre grid, it is *missed* - and missed differently depending on where the
  grid falls, so it would blink as a ship moved.
- **Bottom type is a mixture, not a category.** Sand, mud and rock are three fractions that
  sum to one and vary smoothly; the single word is only ever the largest of them, and
  nothing continuous is ever computed from the word. Anchor holding wants the fractions.

## What is built

    worldbuilder/geometry/      unit vectors, sphere points, tangent frames
    worldbuilder/plates/        plates, Euler poles, margins, relative motion
    worldbuilder/terrain/       continentality, tectonics, band-limited detail, surface
    worldbuilder/bathymetry/    the shelf, placed features, and what the bottom is made of
    worldbuilder/regions/       the demonstration coast
    worldbuilder/integration/   the maritime seam, and the only file that knows both
    worldbuilder/debug/         diagnostics, all of which write PPM and need no libraries

Standard library only. 208 tests.

That describes Mark 1, which is what exists today. Mark 2 moves the generator core to
Rust - one implementation, compiled native for Evennia and to WASM for the browser studio,
so the world a builder sees and the world a game sails on cannot drift apart. It also ends
the zero-dependency stance deliberately: Worldbuilder does not target Evennia's vendored
contrib tree, so contrib conventions do not bind it. See
`docs/design/2026-09-02-mark-2-world-studio.md`, section 4.

## Looking at it

    python -m worldbuilder.debug.harbour            the demonstration coast, twice
    python -m worldbuilder.debug.bottom             sand, mud and rock, as a mixture
    python -m worldbuilder.debug.macro_map          the whole planet
    python -m worldbuilder.debug.lod_shift          what zoom costs a coastline
    python -m worldbuilder.debug.projection_error   the region cap, measured

`harbour` is the one worth running. It draws the same water twice - as the terrain holds
it, and as a chart sampling every four hundred metres would print it - and the difference
between those two renders is the whole argument for how charts have to be built.

## Sailing on it

The maritime contrib asks a world three things, and a generated planet answers all three
without maritime knowing it exists:

```python
from worldbuilder.integration.maritime import maritime_provider
from worldbuilder.regions.demo import WORLD_SEED, demo_region
from worldbuilder.terrain.surface import Surface

region = demo_region()
provider = maritime_provider(Surface(WORLD_SEED, features=region.features), region)
```

Point `MARITIME_MAP_PROVIDER` at a class built that way and a game is sailing on a planet.
The dependency runs one way: maritime never imports this, and the generator imports Evennia
in exactly one function, inside the call.

Handed to maritime's own grounding code, a hull drawing 4.5 m is holed on rock over the
demonstration coast's pinnacle - a hazard 140 m across that sixty-three chart grids in
sixty-four cannot see - while one drawing 2.5 m passes over it, and a 3.5 m hull goes
aground on sand across the harbour bar.

### Somewhere to sail to

The coast carries a harbour, its approaches and six islands - Gannet Isle, Kettle Rock,
Longhope, The Brothers, Sandhaven and Outer Skerry - strung south-east of the fairway. A
coast on its own gives a player one thing to do, which is leave and come back; a chain gives
them a destination, a passage between two of them, and a reason to plot a course rather than
steer one.

The legs are 1,387 m, which is eight minutes under working sail. That figure was measured
from a vessel rather than assumed, and so was the placement: five arrangements were tried
and scored on how long the run out takes, whether six islands come out as six separate
landmasses, how much water lies in the gaps, and how much lies on the way. The chain runs
deliberately close to the drying rock, because a rock awash between two harbours is the best
hazard on this coast.

### A creek that comes and goes

A channel runs inland from the harbour, its bed climbing as it goes, so the sea reaches
further up it at high water than at low. Its head is not a place but a time.

    low water, springs      navigable 3.00 km up
    high water, neaps       navigable 5.13 km up
    high water, springs     navigable 6.13 km up

Nothing works that out. The channel is a shape in the ground and the water level is a number,
and where one meets the other is the head of the creek — so it moves with the tide, and reaches
further at springs, without either the generator or the game being told to make it.

Which is also why nothing here knows what a tide is. The maritime contrib supplies the water
level; this supplies the ground. Neither imports the other.

### And a pond that is not the sea

Above the creek, a closed bowl five metres deep whose water stands twenty-three metres above
sea level. It exists to be the opposite of everything else on the coast: still, fresh, and
entirely indifferent to the tide.

The generator only cuts the bowl. What stands in it is the maritime side's business — its
water model answers by region, so a position in the pond gets one level and the very same
spot asked about as sea gets another. A world that can only have one water level can only
have sea.

## CI

`.github/workflows/gates.yml` runs on every push, on `windows-latest`, pinned to
`rustc 1.98.0`. Seven jobs: the engine suite in five feature configurations
(`--no-default-features`, default, `python`, `wasm`, `python,wasm`), Python conformance, and
one provenance-plus-parity job. Nothing is cached, and no artifact is rebuilt before it is
checked - the provenance job compares the *committed* `.wasm` against the source in the same
commit.

This exists because three things went wrong here and each survived multiple commits, because
nothing ran the check that would have caught it:

1. **The conformance suite skipped silently and reported green while comparing nothing.**
   `tests/test_conformance.py` falls through to `pytest.importorskip` when
   `worldbuilder_engine` cannot be imported. With no engine built and no guard set,
   `pytest tests/` prints `240 passed, 1 skipped` and exits 0 - all 150 comparisons in that
   file vanish into the one `1 skipped` line. Nobody scanning a green log for a suspicious
   number finds one, because there isn't one. **That is the row this slice exists to make
   impossible**, and it is the reason the conformance job sets
   `WORLDBUILDER_REQUIRE_ENGINE=1`: with the guard set, a missing engine is a hard
   `ModuleNotFoundError` at collection, exit 2, not a skip.
2. **The shipped `.wasm` was several commits stale and still passed parity.** Nothing
   compared the artifact's provenance against the source before running comparisons through
   it, so a coastline change could ship with an engine built from an earlier one and parity
   would still report zero divergent - correctly, because the corpus and the artifact agreed
   with each other and with nothing else.
3. **The provenance guard itself was unreproducible from git and failed on every machine but
   its author's.** The fingerprint folds the exact toolchain string (release, commit hash,
   host triple) into the digest, so a check written against one machine's Rust could never
   pass anywhere else without pinning both the toolchain and the runner OS. Gates now run only
   on `windows-latest` at `rustc 1.98.0`, for this reason.

### The five gates, and the message each one produces on a real failure

Verbatim messages below are quoted from prior CI runs recorded in
`.superpowers/sdd/2026-09-04-slice-ci/task-2-report.md`; every count in this section was
re-derived from the current tree, not copied from those runs.

**1. Engine suite (five feature configurations).** `cargo test -p worldbuilder-engine
<features> --no-fail-fast`. A broken constant fails with the ordinary libtest message, e.g.
`assertion left == right failed / left: 3.5 / right: 3.0`. `--no-fail-fast` is load-bearing:
without it, a failing unit test stops cargo before `tests/no_std_math.rs` (gate 2) ever runs,
which is exactly the shape of gate 1 silently hiding gate 2 that this slice exists to
prevent.

**2. The determinism guard**, `crates/worldbuilder-engine/tests/no_std_math.rs`, runs as part
of the engine suite above. It scans `src/` for `f64::`-style float math, `mul_add`, or an
unmarked truncating cast, and fails with: `std float maths (or an unmarked float-truncating
cast) found outside detmath: <file>:<line>: <form> — route it through detmath (or mark with
` `// cast-ok:` ` if this is a genuine integer cast)`.

   **Known limitation, recorded rather than fixed:** the scanner is comment-blind by design -
   `scan_text` treats any line whose trimmed text starts with `//` as a comment to skip
   (`crates/worldbuilder-engine/tests/no_std_math.rs:60`), and `///` and `//!` both start with
   `//`. A banned form written inside a doc comment compiles to nothing and is invisible to
   the guard - confirmed on the current tree: `crates/worldbuilder-engine/src/lib.rs` lines
   91-96 are a `///` doc comment, and a proof-of-failure placed there reported
   `test result: ok. 6 passed` instead of catching anything. The same form on a line of real
   code was caught. This is correct behaviour for an actual comment and a trap for anyone
   trying to prove the guard still works: it must be tested on a code line, not a doc line.

**3. Python conformance**, with `WORLDBUILDER_REQUIRE_ENGINE=1`. Missing engine:
`ModuleNotFoundError: No module named 'worldbuilder_engine'` at collection, exit 2. A
divergent engine: per-test `AssertionError`s comparing measured values against tolerance,
e.g. `assert 0.06100853039628548 <= 2.2e-14`.

   **The row where CI passes is the important one.** With the guard *unset* and no engine
   built - the historical, buggy configuration - `pytest tests/` still exits 0 and prints
   `240 passed, 1 skipped`. Re-derived locally on the current tree,
   `pytest tests/ --collect-only -q` reports **390 tests collected**, of which **150** are in
   `tests/test_conformance.py` (`tests/ --collect-only -q | grep -c
   '^tests/test_conformance.py::'` → 150) and the remaining **240** are spread across the
   other thirteen test files. `240 passed, 1 skipped` is exactly `390 − 150`: the whole
   conformance file collapsing into one line. This is the bug the slice exists to make
   impossible, and it is why the count gate below asserts the per-file total, not just exit
   status.

   **The guard closes the *missing* engine, not the *stale* one.** `WORLDBUILDER_REQUIRE_ENGINE=1`
   asserts only that `worldbuilder_engine` imports. An engine that is present but built from
   older source imports fine, so the suite reports `390 passed` and every gate here goes green
   while 150 comparisons measure an artifact nobody can place in history. The `.wasm` has a
   28-input fingerprint and a manifest for exactly this reason; the Python extension has
   neither. See "What CI does NOT cover" below - it is deferred, not solved.

**4. Provenance**, `npm run check:wasm` in `viewer/`. Source edited without a rebuild:
`STALE ARTIFACT: - the shipped .wasm was NOT built from the source that is here now: source
now: <hash> / artifact built from: <hash> (28 inputs fingerprinted.)`. Re-run locally against
the current tree: `Current: .../viewer/public/wasm/worldbuilder_engine.wasm matches its
manifest and the source that is here now.` - and `viewer/public/wasm/MANIFEST.txt` itself
records `fingerprint-inputs: 28`, confirmed by running the check, not read off a report.

   Two cheaper proofs run alongside it, both re-run locally as part of this task:
   `npm run build:wasm:stale-self-test` (`SELF-TEST PASSED: the fingerprint refuses a source
   tree that has moved.`) and `npm run build:wasm:self-test` (rejects a stripped 327-byte,
   memory-only artifact before rebuilding the real one).

**5. Parity**, `parity_dump` (native) replayed through the committed `.wasm` by `parity.mjs`.
A mutated artifact is refused rather than silently compared: `REFUSING TO REPORT PARITY --
STALE ARTIFACT:` with the source/artifact hashes, because "the corpus and the .wasm agree
with each other and with nothing else" is not evidence. A control run
(`--mutate seed`) proves the harness can fail at all: of 53,251 values compared, 50,778
diverge.

### The two count gates

`.github/scripts/assert_counts.py` is a sixth kind of check: the four gates above ask "did
anything fail", which cannot catch a suite whose tests were deleted or whose corpus quietly
shrank while everything else stayed green. It cross-checks two independently-produced
statements of the same number (never a `test result:` grep) and fails loudly, naming the
mismatch, if they disagree or either is missing:

- **Engine suite**, per feature configuration: `cargo test -- --list` gives one `<name>: test`
  line per test and a `<N> tests, <M> benchmarks` trailer; they must agree. Re-derived
  locally for `--no-default-features`: **414 listed, 5 ignored → 409 run**, matching the
  workflow's asserted `--expect-passed 409 --expect-ignored 5` exactly
  (`python .github/scripts/assert_counts.py cargo-list ...` → `count OK: 409 passed / 0
  failed / 5 ignored`). The `wasm` and `python,wasm` configurations expect 439/5, thirty more
  than the other three because they compile `tests/wasm_exports.rs`. On a deleted test, the
  gate fails with `COUNT GATE FAILED / expected <N> tests to run, found <M>` even though
  `cargo test` itself exits 0.
- **Python suite**: `pytest --collect-only -q`'s per-test lines and its `<N> tests collected`
  trailer, cross-checked against the real run's `<N> passed in <T>s` summary. Asserts
  **390 tests in total, 150 of them in `tests/test_conformance.py`** - both figures re-derived
  above, not copied from an earlier report. On the historical unguarded configuration it
  fails with `the run reports outcomes that are not `passed`: 1 skipped ... expected 390
  tests in total, found 240 / expected 150 tests in tests/test_conformance.py, found 0`.
- **Parity corpus**: the total line (`53,251 values compared`) is cross-checked against the
  nine per-group tallies summing to it, so a shrunk corpus fails with `COUNT GATE FAILED /
  expected 53251 values compared, found <M>` even when provenance and parity both report
  green on their own.

### What CI does NOT cover

**The viewer's browser checks.** `viewer/README.md` documents an in-page harness
(`window.__wb.check()`) exercised through URL parameters and a `?fault=` switch that forces a
known-wrong implementation: **eleven checks pass on the default world**, and **seven**
`?fault=` values (`flip-latitude`, `shift-tile`, `wrong-world`, `stale-worker`, `cache-key`,
`feature-blind`, `feature-everywhere`) each must make specific checks fail. This needs a real
browser with WebGL compositing and a person reading the result - it is not a test file CI can
invoke, several checks are explicitly rendering-dependent (one reports NOT EXERCISED when
`frameState.frameNumber` is 0), and a software-rasterised CI number would be a different
measurement wearing the same name. **It is out of scope for this slice.** A green badge on
this workflow says nothing about the viewer having been watched run.

Two further holes are known and deliberately deferred to their own slice, not this one:

- **The Python extension has no fingerprint and no manifest.** The `.wasm` carries a
  28-input source fingerprint and a manifest that provenance checks on every push; an engine
  present in a developer's `.venv` but built from older source is undetected by anything.
  `WORLDBUILDER_REQUIRE_ENGINE=1` only asserts that the import succeeds, never which source
  built it.
- **A distribution-name collision.** `pyproject.toml` declares `name = "worldbuilder-by-aetos"`
  for both the setuptools-built reference package and the maturin-built extension, so
  `maturin develop` silently uninstalls the pure-Python `worldbuilder/` package that the
  conformance suite compares the engine against. CI works around it by importing the
  reference package from the working tree rather than installing it; the underlying collision
  is untouched.

## Layout

    CHANGELOG.md                             phase by phase, with the measurements
    docs/design/anchoring-architecture.md    the core: anchors, seams, authority
    docs/design/2026-08-31-planet-design.md  decisions and what they were taken against
    docs/design/2026-08-31-generator-spec.md the generator itself
    docs/design/mark-1-scope.md              what Mark 1 is for, and each phase result
    docs/design/2026-09-02-mark-2-world-studio.md
                                             Mark 2: the studio, and the route into Evennia
    docs/design/gpt-brief.md                 the brief sent for outside review
