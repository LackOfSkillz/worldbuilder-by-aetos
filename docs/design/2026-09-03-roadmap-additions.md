# Roadmap additions — climate, land cover, the studio, and cartography

Recorded 2026-09-03. Decisions taken in conversation; **not yet a spec amendment**. The
brainstorm was paused before the design sections, so nothing here is approved for
implementation. Resume at "propose the design in sections".

---

## 1. Correction to how the roadmap was being reported

The Rust port is **slice 1 of six**, not the project. The build order in
`2026-09-02-mark-2-world-studio.md` §20 already runs:

    0  bit-equality spike        DONE
    1  Rust core                 IN PROGRESS - 14 of 15 modules; surface.py remains
    2  inventory + apply, and a read-only viewer alongside
    3  studio                    placement, anchor tree, seams, worldfiles
    4  greenfield stub           creation through Evennia's prototype system
    5  erosion bake              rivers, lakes, water manifest

The viewer sits at slice 2 deliberately, because "a stretch of months with nothing to look
at is its own risk".

---

## 2. The studio north star

**PlanetMaker** (planetmaker.apoapsys.com) is the target shape: a browser 3D globe with
live parameter panels either side, immediate feedback, a saved-state gallery.

Take its **interaction** model. Do not take its rendering — the spec is explicit that the
studio is "an instrument, not a renderer: no photorealism, no atmospherics, no vegetation",
and those cost exactly the frame budget zoom depth needs.

**PlanetMaker is texture-limited and Worldbuilder is not.** Its Scene panel carries a
`2048x1024` texture dropdown; its zoom bottoms out where the image does. Worldbuilder
evaluates a continuous field at a point, so zoom is bounded by what the detail octaves
resolve and by frame budget rather than by an image.

**CORRECTED 2026-09-04, twice — this sentence was wrong in both directions before it was
measured.** The generated field has a floor, and it is **78.125 m**, not the 250 m a first
correction claimed: `CANONICAL_WAVELENGTH_M = 250` is a *loop bound*, so the finest octave is
**312.5 m**, and octaves fade by `smooth((lambda/r - 2)/2)`, reaching full strength only at
`r <= lambda/4`. Measured on a 2 km transect, peak-to-peak rises monotonically 0.19 m to 5.58 m
as `r` goes 20,000 down to 78.125, then is **bit-identical** for 50, 25 and `None`.

Below about 100 m the generated field is a **tilted plane** — over a 100 m span it departs from
the straight chord by 4.5 cm, over 25 m by 2.5 mm. So the honest claim is: **zoom reveals
generated ground down to ~78 m, and below that it reveals authored features.**

And features are genuinely different, measured for the first time on 2026-09-04 — every earlier
probe passed `features=None`. A 900x260 m carve plus a 200x60 m mole give **152.8 m of relief
over a 100 m span** and 2.76 m of chord deviation over 25 m, because `Features::apply` is
analytic, sits outside the octave schedule, and its `authority` term damps texture rather than
being buried by it. Features cost under 5% in throughput.

**So a field viewer does show the bottom of the harbour — but because somebody placed the
harbour, not because the noise goes that deep.**

### 2.1 Zoom is understated in the spec

§7 says "rotate, zoom" as though zoom were one verb. The requirement is a zoom deep enough
that a developer can descend from planet to a place worth building and place an area there.
That is a level-of-detail problem, not a camera problem.

**When slice 2 is planned, sharpen this into a stated depth target and a frame budget.**
Three things already exist that feed it: `debug/lod_shift` measures what changes between
detail levels, `detail.rs`'s resolution table defines the octave schedule zoom drives, and
`debug/projection_error` renders where the tangent-frame approximation costs — which is the
budget a deep zoom spends.

### 2.2 Borrowing, and the one hard problem

A globe is ~6.4e6 m across; GPUs are float32. Zoom to metre scale and geometry visibly
swims, because a float32 cannot hold a planet-scale position to metre precision. The known
fix is relative-to-eye rendering: keep positions double on the CPU, subtract the camera,
hand the GPU the small offset.

**DECIDED 2026-09-04: CesiumJS.** Planet-to-metre zoom with correct precision and LOD is
precisely what it exists for, and its custom `TerrainProvider` lets tiles be generated on demand
from the WASM field, satisfying the rule that the studio draws from the generator and never from
a baked approximation.

**All three verification questions came back clear:**

- **Licence: Apache-2.0**, `cesium@1.145.0`, confirmed against `LICENSE.md` on `main` and npm
  metadata, with every commit touching that file listed back to 2015-02-20 — no relicensing.
  Cesium **ion** is a separate product with its own ToS; "CesiumJS is Apache 2.0" and "Cesium is
  free offline" are two different sentences, and we rely only on the first.
- **Custom terrain: fully supported.** `TerrainProvider` is duck-typed, not a sealed class, and
  `CustomHeightmapTerrainProvider` exists *specifically* for procedural terrain — one callback,
  which may return a promise, so no provider class need be written at all.
  `HeightmapTerrainData` with a `Float32Array` takes heights as metres above the ellipsoid
  directly. **Two traps:** `ready`/`readyPromise` were removed in 1.107, so do not implement
  them; and `getTileDataAvailable` returning `undefined` makes Cesium refine until it runs out
  of memory — that is where the zoom cap gets enforced.
- **Ion is fully disablable and NOT disqualifying.** Cesium ships an official offline guide;
  the bundle contains no telemetry; `Ion.js` performs no I/O. Exactly **one** `Viewer` default is
  network-live — `baseLayer = ImageryLayer.fromWorldImagery()`. Terrain's default is offline.
  **Caveat: this is a documentary and source case, not a witnessed one** — a browser network
  trace was not taken. Five minutes with DevTools converts it, and that is the slice's first task.

**And the float64 precision problem — the reason Cesium was chosen — costs a custom provider
nothing.** Terrain uses per-tile relative-to-centre encoding, with the camera subtraction done in
float64 on the CPU. Return standard `TerrainData` and the whole mechanism is downstream.

The alternative worth weighing is **Rust-native**: `bevy` or `wgpu` plus `egui`, all one
WASM module, no JS boundary to marshal across. Costs implementing precision and LOD.

**Verify before committing** (none of this is confirmed): Cesium's current licence; whether
`TerrainProvider` still supports a fully custom non-tiled source; whether Cesium's asset/ion
dependencies can be fully disabled, since the spec forbids live connections and any
phone-home is disqualifying.

Nothing to borrow on the Evennia side — placement, anchor tree, seams and worldfiles are
bespoke.

---

## 3. Climate and land cover — a new field layer

**This reverses a stated non-goal.** §Non-goals currently lists "Production climate
simulation. Economies. Flora and ...". The reversal is deliberate and owner-approved. What
is added is a *bounded* climate model, not production climate simulation.

### 3.1 Decisions taken

| Question | Decision |
|---|---|
| Where do biomes come from? | **Computed baseline + authored overrides** — mirrors terrain exactly: `natural()` computes, `Features` override in order |
| How much moisture model? | **Latitude bands + rain shadow** — deserts land behind mountains, and a CRUD'd mountain casts its own shadow |
| Tag drift on regeneration? | **Derive, stamp, and pin expectations** — tags queried, written to the worldfile, and the area records what it expected; reconciliation reports loudly when reality no longer matches |

The third matches VERSION-001's existing philosophy: fail closed, no silent substitution.
Evennia receives concrete tags and never needs the engine at runtime.

### 3.2 Tags are composed, not enumerated

"Coastal" is not a biome — palm versus pine on the same coastline is the difference between
tropical-coastal and boreal-coastal. Three axes:

| Axis | Source | Values |
|---|---|---|
| Landform | terrain field | coastal, mountain, valley, plain, plateau |
| Temperature | latitude + elevation lapse rate | tropical, temperate, boreal, polar |
| Moisture | bands + rain shadow | arid, dry, moist, wet |

A place is the **intersection**. A flat enum explodes combinatorially; three small
vocabularies give every combination free. Whittaker's temperature-by-precipitation grid is
a well-trodden classification to borrow rather than invent.

**Elevation feeding the temperature axis gives the snow line for free** — the same tropical
latitude is rainforest at sea level and snowcap at 5,000 m. Kilimanjaro is correct with no
special case. Polar caps likewise fall out of temperature alone.

### 3.3 Affordances — what the game engine actually consumes

A fourth layer, derived from the intersection by **lookup table, not simulation**: forage,
timber, stone, game animals, fish, hazards, seasons.

A useful amount is already derivable from what is ported:

- **latitude** gives day length and seasonal swing, free from `SpherePoint`
- **slope** gives travel difficulty, from `slope_at`
- **substrate** gives stone and clay
- **bathymetry** gives sea access and anchorage quality — `debug/harbour.py` already
  measures it

The genuinely new part is the flora/fauna half, which needs the climate axes.

### 3.4 The architectural problem, and the rule that resolves it

Everything in this engine is **point-evaluable**: `f(SpherePoint) -> value`, no state, no
neighbours. That property is why it is deterministic, WASM-able, arbitrarily zoomable and
conformance-testable.

**Rain shadow is not naturally point-evaluable** — knowing how much moisture remains at a
point requires integrating along the wind path from the ocean.

**Chosen: ray-march at query time.** From the query point, walk upwind a bounded number of
steps sampling elevation, accumulating rain-out. Stays point-evaluable, deterministic and
zoom-independent, and a CRUD'd mountain casts its shadow immediately with nothing to
rebuild.

Rejected: **baking a global moisture raster**, because the spec forbids the studio drawing
from "a separately baked approximation that could disagree with the game", and because
editing terrain would then do nothing until a rebake.

### 3.5 Measured cost, so the budget is stated rather than discovered

Measured on this machine through the release build, including Python FFI overhead (likely
most of it; native and WASM will be faster, and this was not isolated):

    continentality_at    0.32 us/call
    detail_offset_m      0.53 us/call

A full elevation query is therefore roughly 1-2 us, and a ~40-sample upwind march roughly
**40-80 us per moisture query**.

| Job | Points | Estimate |
|---|---|---|
| Whole planet at 2048x1024 | 2.1 M | ~1.5 min via Python; ~10 s native; seconds across cores |
| Whole planet at 4096x2048 | 8.4 M | ~6 min via Python; under a minute native |
| One viewer tile, 512x512 | 262 k | ~10 s via Python; ~1 s native |

**Generation is not the constraint. Interactivity is.** At 60 fps there is 16 ms per frame
and a screenful cannot be marched in it.

**Design rule: moisture is cached per tile, never computed per frame.** Biomes do not change
frame to frame; compute a tile once and recompute only when the camera reaches new tiles or
the user edits terrain nearby. Two further mitigations: the march can sample **coarse**
low-octave elevation, since rain shadow is a tens-of-kilometres phenomenon, and results
cache naturally by tile.

**Before the spec commits to a sample budget, benchmark the actual march in Rust and in
WASM.** The figures above are two of three elevation components measured through FFI. That
is a cheap spike and it is the difference between a stated budget and a guess.

### 3.6 A build-order consequence

**Rainfall is exactly what erosion needs.** §20 places the erosion bake last as the one
thing nothing else depends on. Building moisture for biomes builds slice 5's input, so the
two now share a dependency. Whether that reorders the build is an open question for the
spec amendment.

---

## 4. Terrain CRUD — mostly already built

`Feature` is already the record an editor needs, and it was ported and conformance-tested
this week:

    kind        what it is called; also drives chart symbols
    at          where
    target_m    the elevation it wants the ground to be
    length_m, width_m, bearing_deg     extent and orientation
    compose     RAISE (only up) | CARVE (only down) | SHAPE (either)
    substrate   what it is made of, or None to derive

A mountain is `compose=RAISE, target_m=2400`. A valley is `CARVE`. A caldera is a RAISE with
a CARVE after it — and **order is already load-bearing and tested**.

Two caveats: it lives in `bathymetry/` with marine docstrings, and `substrate` speaks
sand/mud/rock. The mechanism is elevation-agnostic; the vocabulary is marine. That is
renaming and a wider substrate table, not new architecture.

**Cover overrides need their own placed type**, not a field on `Feature` — a rainforest is
not a landform and must be placeable on flat ground without raising it.

### 4.1 The rule that keeps CRUD coherent

The architecture rests on *seed + parameters + authored declarations -> deterministic world*,
regenerable and diffable. CRUD is safe **exactly as long as every edit is expressible as a
declaration**.

**Edits are declarations, not paint.** A brush that emits Features is fine. A brush that
emits a heightmap ends the model — regeneration, versioning and the one-source argument all
break together.

---

## 4.2 Variation — three decisions, taken 2026-09-04

Owner-approved: **modest planet size variation**, **terrain features varying within a kind**, and
**biome regions varying** rather than stamping uniformly.

### Planet size is nearly free, and one consequence sets the range

`radius_m` is already threaded through every module, and slice 1o proved it genuinely wired — including
finding four substrate-facing paths where it was accepted but unwitnessed.

**But two layers scale differently, and that is what bounds "modest".**

- **Continentality** uses `BASE_FREQUENCY = 1.25` in *cycles per sphere*. Continents are always the same
  **fraction** of the planet, so they shrink with it.
- **Detail** uses a fixed wavelength table — `COARSEST_WAVELENGTH_M = 20_000.0` down to
  `CANONICAL_WAVELENGTH_M = 250.0`, in **absolute metres** — with frequency derived from the radius
  (`detail.py:94`). A 250 m dune is a 250 m dune on any planet.

So **as the planet shrinks, detail grows relative to landform**, and a smaller world reads as rougher.
That is arguably desirable, and it gives a principled way to choose the range rather than an arbitrary
one: **the limit is where detail stops being detail relative to the continents it sits on.** Pick the
range by measuring that ratio, not by taste.

**The second constraint is measured, not theoretical.** Slice 1o's last fix round found a 3,000,000 m
world **genuinely exceeded** `SURFACE_BOTTOM_GRID_MAX_ABS` — 1.75e-13 against 1.2e-13 — and correctly gave
it its own bound rather than widening the shared one. **A size range therefore means bounds re-measured
across it**, which is tractable precisely because the variation is modest. Do not scale a bound to a new
radius; measure it there.

### Terrain feature variation is already expressible; what is missing is the generator

`Feature` already carries `length_m`, `width_m`, `bearing_deg` and `target_m` independently of `kind`, so
"a reef" versus "this particular reef" is already just data. **Nothing in the engine changes.**

What is missing is what *produces* varied instances: a kind template with ranges, sampled from the seed.

**Ruling on where the variation lands: the studio samples at placement time and stores concrete
`Feature`s**, rather than generating from `(kind, seed, index)` at read time. A stored declaration stays
editable, and a builder must be able to nudge one reef without regenerating the other twenty-four. This is
the same rule as everywhere else — **edits are declarations, not paint** — applied to generated variation.

### Biome variation is free, because the layer does not exist yet

Two kinds fall out naturally. The **computed** baseline is already varied, being driven by continuous
fields. **Authored** cover regions get the same extent-and-orientation treatment as features, plus an
**edge softness** so a rainforest is not a uniform stamp with a hard rim.


## 5. Cartography export — a new subsystem

Artistic maps at every scale: planet, region, down to town and city, exported as images.

Entirely new; nowhere in the current spec. It does **not** contradict "an instrument, not a
renderer" — that rule governs the live interactive surface, and exported maps are a separate
artifact. But it is a substantial subsystem rather than a button, and it needs its own
scoping: which scales, what styles, what projections, raster or vector.

---

## 6. Open questions for the spec amendment

1. Does climate land before slice 5, given rainfall is erosion's input? (§3.6)
2. What is the stated zoom depth target and frame budget? (§2.1)
3. What is the moisture march's sample budget, measured in Rust and WASM rather than
   estimated through FFI? (§3.5)
4. Cesium versus Rust-native, once the three verification questions are answered? (§2.2)
5. Where does cartography sit in the build order, and at what scope? (§5)
6. Does the `bathymetry` package get renamed now that `Feature` is doing terrain work, or
   does a landform package wrap it? (§4)

---

## 7. Status

**Paused before the design sections.** Three decisions are taken (§3.1) and the
architectural constraint is identified (§3.4). Nothing is approved for implementation.

Resume with: propose the design in sections, get approval per section, then write the spec
amendment to `docs/superpowers/specs/`, then `writing-plans`.
