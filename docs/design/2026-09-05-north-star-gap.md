# The north-star image: a complete gap analysis

The owner supplied a reference render and said, plainly: *"it needs to go from the first image to the second
image. analyze them completely second image is the north star."*

This document enumerates **every visual element** that separates the two, says which system owns each, what
it costs, and in what order they should land. It is a ranking, not a wish list: several of these are cheap
and were simply never done, and two of them are worth more than everything else combined.

**One honest framing first.** The reference is a rendered illustration, not a photograph and not the output
of a simulation. That does not make it unreachable -- every element in it is a real thing this engine can
produce -- but it means the target is a *look*, and some of that look comes from post-processing rather than
from terrain. Saying so up front is what stops us chasing the last 5% through the terrain generator, which
is where a project like this burns a month.

---

## What the two images actually differ by

Read off the images directly, most visually dominant first.

> **The verdict on all twelve is at the end of this file**, in *THE VERDICT, 2026-09-06*. Six are
> closed, three are partly closed, three are untouched, and one of the "partly" verdicts is
> partly *by ruling* rather than by difficulty. **The table below is the question, not the
> answer**, and it is left exactly as it was written so that what it got wrong stays legible.

| # | Element | Ours | North star | Owner |
|---|---|---|---|---|
| 1 | **Clouds** | none | heavy white cirrus and cumulus over perhaps 40% of the disc, with a soft terminator | **viewer only** |
| 2 | **Land colour variation** | two bands, green to tan | tan desert, dark forest, pale rock, olive scrub, white snow -- distinct regions | climate/biomes |
| 3 | **Snow and ice** | none | white ridgelines along every mountain chain, plus polar white | climate + mountains |
| 4 | **Mountains** | none visible | linear ranges with visible ridge structure, casting shade | **in flight** |
| 5 | **Coastline shape** | smooth blobs, low-frequency | deeply fractal -- bays, fjords, offshore islands, peninsulas | engine (continentality) |
| 6 | **Ocean depth** | shelf gradient present but low-contrast | pale shelf grading to near-black abyssal | **viewer only, tuning** |
| 7 | **Rivers and drainage** | none | fine dark dendritic lines over the whole land surface | engine (rivers) |
| 8 | **Inland water** | none | a large circular lake, several smaller | engine (water, built) |
| 9 | **Night side + city lights** | none, fully lit disc | terminator across the right third, orange settlement lights | viewer + studio |
| 10 | **Atmospheric limb** | thin hard white ring | thick blue haze, wraps the limb, softens the terminator | **viewer only** |
| 11 | **Specular sea** | none | subtle sun glint on water | viewer only |
| 12 | **Overall contrast** | flat, washed | deep blacks, bright highlights | viewer only |

---

## The ranking, by impact over cost

### Tier 1 -- cheap, viewer-only, and enormous

**1. Clouds.** The single largest difference between the two images and it touches no engine code. A
procedural cloud layer over the globe -- an imagery layer like the relief one, driven by the same noise the
engine already has. **Nothing about the terrain has to change for this to land**, and it is the element a
viewer's eye reads first as "photograph of a planet" rather than "diagram of a planet".

**2. Ocean depth colour -- CORRECTED, AND THE CORRECTION MATTERS.** My first reading of our image was that
the sea was flat blue and the bathymetry unused. **That is wrong.** `RAMP_STOPS` already carries six ocean
stops from `#020a14` at the abyssal plain to `#7ec5df` just under the strand, and the pale rim around each
landmass in our image is that shelf gradient working. The data is used.

The real gap is **tone, not data**: the reference's deeps go nearly black and its shelves are brighter, so
its ocean carries far more contrast across the same depth range. That is a ramp-tuning and grading job
measured against the depth distribution this generator actually produces -- much cheaper than the plumbing
job I first described, and worth doing for that reason.

**A thing to check while doing it:** there are now TWO colour systems -- the `ElevationRamp` material on the
globe and the relief imagery layer painted over it. Which one wins where, and whether the imagery layer is
flattening the ramp's ocean contrast, should be measured before either is tuned.

**3. Atmosphere and limb.** Cesium ships ground atmosphere and sky atmosphere. Ground atmosphere is
currently **off**, for a measured reason recorded earlier: it washed the ocean to the same colour as land
500 m up. That measurement was taken against a *flat blue* ocean. **With item 2 done, the reason may no
longer hold**, and it is worth re-measuring rather than inheriting.

**4. Contrast and tone.** The reference has deep blacks and bright highlights; ours is uniformly mid-tone.
Partly a consequence of items 1-3, partly a final grade.

### Tier 2 -- engine work already scoped

**5. Mountains.** In flight now. Measured to reach 4,540 m at a 7.03% grade on the owner's own world.

**6. Coastline fractality.** Ours are smooth because continentality is a low-frequency field; the reference's
are fractal at every scale. This is the same *kind* of fix as the relief slice -- add finer octaves where the
eye is looking -- but applied to the land/sea boundary rather than to elevation. **Medium cost, high payoff**,
because a coastline is the highest-contrast edge in the whole image and the eye goes straight to it.

**7. Inland water.** Slice 5b built lakes and they are not drawn. The reference's most distinctive single
feature is a large circular inland sea. **We have the data and no export** -- the panel says so itself.

### Tier 3 -- larger, and correctly later

**8. Land cover from climate.** The reference's colour variation is biome variation: desert, forest, scrub,
rock. This is exactly what the climate slice is designed to produce, it is approved, and its blocking spike
is already answered.

**9. Snow and ice.** Falls out of climate's temperature axis for free, and the relief slice already carries
a snow term waiting for a real snow line rather than a global elevation threshold.

**10. Rivers.** Schema exists, population does not. The finest visible texture in the reference.

**11. Night side and city lights.** Needs placed settlements, which is the studio slice. **Correctly last** --
and worth noting it is the one element that is genuinely about the game rather than about the planet.

---

## What this changes about the roadmap

Nothing is deleted. Two things move:

- **Tier 1 becomes its own slice and runs next**, because four items of large visual payoff cost days rather
  than weeks, and three of them are pure viewer work that cannot break the conformance oracle.
- **Coastline fractality is added**, and it was not on the roadmap at all. The relief slice fixed the *height*
  field's spectrum and nobody asked the same question of the *land/sea* field.

The order is then: mountains (in flight) -> Tier 1 -> lakes drawn -> coastlines -> climate -> rivers -> studio.

## What we should not pretend

Even with all of the above, two differences will remain:

- **The reference is lit and graded like a film still.** Ours is lit like a map. Some of that gap is a
  post-processing pass, not a terrain feature.
- **The reference's land has a painted quality** -- its detail is suggestive rather than derived. Ours is
  computed, which means it is consistent at every zoom and can be walked around in a MUD. That is the whole
  point of the project, and it is a trade worth naming rather than quietly regretting.

---

# THE VERDICT, 2026-09-06

The slice ran as eight tasks plus two engine follow-ups, on `slice-5b-water`, from `3008f89` to
`b6862d2`. **Every figure below was re-derived for this section on this host**, not carried
across from a task report; where a report's number and this section's disagree, the disagreement
is stated and the re-derivation wins. Population, method and host are given with each.

## The scoreboard

| # | Element | Verdict | Where it lives now |
|---|---|---|---|
| 1 | Clouds | **CLOSED** | `viewer/public/app/clouds.js`, `cloud-provider.js`; `?clouds=`, default **0.40** |
| 2 | Land colour variation | **CLOSED** | `biome.js`, 33 colours on three axes; `?biome=0` restores the ramp |
| 3 | Snow and ice | **PARTLY** | polar and ice entries exist in the biome palette and are reached; there is still no snow *line* from climate, and `relief.js`'s snow blend is a latitude-scaled elevation contour |
| 4 | Mountains | **CLOSED, by the slice before this one** | `TectonicParams::ranges()`; every camera in this slice carries it |
| 5 | Coastline shape | **PARTLY -- built, measured, and OFF BY DEFAULT** | `continentality.rs::CoastParams`, three exports, one panel slider; canonical is amplitude 0 and Ruling 1 keeps it there |
| 6 | Ocean depth | **CLOSED** | `OCEAN_STOPS` in `panel-fields.js`, twelve stops, one table for both colour systems |
| 7 | Rivers and drainage | **OPEN** | untouched; `reaches` carried and unpopulated, still on the panel's not-wired list |
| 8 | Inland water | **CLOSED** | `water.js` + `relief.js`; **55 of 55 bodies drawn** on the owner's world, with `LAKE_STOPS` of their own |
| 9 | Night side + city lights | **OPEN** | needs placed settlements: the studio slice, correctly last |
| 10 | Atmospheric limb | **PARTLY** | ground haze is ON by re-measurement; the *thick blue* limb is `?limb=24000` and is an owner decision |
| 11 | Specular sea | **OPEN** | nothing was attempted |
| 12 | Overall contrast | **PARTLY** | the sea gained 2.4x of spread and the land 9x of luminance sd; no tone map ships, and the best-measuring one is an owner decision |

**Six closed, three partly, three open.** Two of the three "partly" verdicts are **decisions
waiting on the owner rather than work waiting on an engineer** -- the limb and the tone map -- and
one is partly-by-ruling: the coastline is finished and switched off because no default may move.

## What each of the closed ones cost, and the measurement that decided it

### 1. Clouds -- and the slider is the coverage, not a threshold

`?clouds=` is the fraction of the sphere at or above half opacity, and it is delivered by
inverting the field's own measured distribution at world load. **Re-derived here:** calibration on
a 20,000-point equal-area Fibonacci spiral, re-measured on a **fresh 200,000-point** spiral --
because a function that measured its answer with the array it had just sorted would agree with
itself by construction. Owner's world (seed 562423712, radius 4,500,000 m, 28 plates, land 0.16),
node v22.17.0, this repository's checked-in `.wasm`.

| asked | 0.05 | 0.10 | 0.20 | 0.30 | **0.40** | 0.50 | 0.70 | 0.90 |
|---|---|---|---|---|---|---|---|---|
| **delivered** | 0.0495 | 0.1007 | 0.2006 | 0.3005 | **0.3996** | 0.5025 | 0.6984 | 0.8994 |

*(Task 2 recorded 0.4000 at the default against this section's 0.3996; the difference is the
re-sample, and both are the same claim.)*

**Cost:** the pool gained a third consumer -- roughly a fifth more worker CPU and a fifth more
time to settle at the orbital camera -- and the layer is capped at level 3, so on a descent the
clouds stop sharpening while the ground keeps going. **`?clouds=0` builds no layer at all**, which
is what let every land measurement in this slice be taken with the weather off.

### 2. Land colour -- and the ordering in this document was wrong

This document parked land colour in Tier 3 behind climate. **That premise was false** and the
plan's own REVISED section says why: the reference implementation gets desert, forest and scrub
from noise plus latitude, and its band edges are quantiles of a global array -- which
`continentality.rs::calibrate` already computes grid-free. Land colour ran **first**, and it had
to: clouds at 40% coverage hide exactly the land defects the slice existed to fix.

**Re-derived here.** Population: the six 30-degree tiles of a 12x6 global scan whose corner and
centre probes are at least three-fifths land on the owner's world, rasterised through
`reliefTile` at 64 texels a side, **land texels only -- 16,743 of them**. Method: the same tiles
and the same build with `biome: null` and with `biome: calibrate(...)`. Host: node v22.17.0, the
checked-in `.wasm`. Rec. 709 luminance over 0..255:

| | mean | **sd** | min | p01 | p99 | max | **p99/p01** |
|---|---|---|---|---|---|---|---|
| **height ramp** | 96.3 | **9.8** | 83 | 88 | 143 | 191 | **1.6:1** |
| **biome** | 63.6 | **42.3** | 5 | 12 | 184 | 200 | **14.8:1** |

**Luminance sd 9.8 -> 42.3, and p99/p01 1.6:1 -> 14.8:1.** That is the difference between a
diagram and a photograph and it is the number the task was defined by. (Task 1 reported 9.5 ->
42.1 and 14.6:1 over 16,740 texels; this section's tile-selection rule is its own, the populations
differ by three texels, and the conclusion is identical.)

The mean *falls*, because this world's land is mostly wet and tropical and a closed canopy really
is near-black in the visible band. That is the same physics the palette is derived from.

### 6. Ocean tone -- the fix was BRIGHTER, and only the distribution could have said so

The correction this document already carries -- "the gap is tone, not data" -- was right, and the
tone problem turned out to be **where the stops are, not how dark they are**.

**Re-derived here.** A 2,880 x 1,440 edge-inclusive global fill (4,147,200 samples) at canonical
resolution on the owner's world, area-weighted by cos(latitude), through `wb_fill_tile_f32`. Node
v22.17.0, the checked-in `.wasm`.

- **48.61% of the sea's area lies between -4,620 m and -4,560 m.** Sixty metres. The old table
  interpolated straight through that band, so half the ocean received about half a luminance unit
  of variation. **The sea did not read as flat because its deeps were not dark enough; it read as
  flat because half of it was one colour**, and darkening the abyss could not have touched that.
- Only **1.29%** of the sea is shallower than 120 m, and **0.1137%** is at or below -6,000 m. The
  deepest sample in the fill is **-6,558.5 m**, which is why the old -6,800 m stop was
  unreachable: it anchored an interpolation and was never itself painted.

The stops moved *up*: twelve of them, the shelf and surf much brighter, the abyssal plain given an
internal gradient. Our table still sits darker than the reference at every depth, and that
remaining lift belongs to the grade.

### 8. Inland water -- and lakes are the ocean's mirror image

The reference's most distinctive feature is a large inland sea; slice 5b had built the data and
nothing drew it. It is drawn now, and **all 55 bodies** on the owner's world are, not 17 -- the
manifest's box bounds node *centres*, so growing it by the one node cell that construction
licenses gives a single-node body the cell it stands for.

**Re-derived here.** All 55 bodies at `node_count = 30,000` on the owner's world, each dilated box
sampled at 0.02 degrees through `wb_fill_tile_f32`, a sample counted where `0 < h <= level`,
area-weighted by cos(latitude): **270,320 lake samples**. Node v22.17.0, checked-in `.wasm`.

| p25 | **p50** | p75 | max |
|---|---|---|---|
| 7.0 m | **15.4 m** | 27.1 m | 127.9 m |

**61.4% of lake area lies in the first twenty metres** (21.4% in the first six, 79.2% in the first
thirty). **That is the exact mirror of the ocean**, which is a slab at one depth: half a lake is
at its shore, half the sea is at about -4,590 m, and **one table cannot serve both**. It does not:
there is a `LAKE_STOPS` now, thirteen stops over 0..390 m, nine of them inside the first 48 m.

*(Task 8 reported 21.5 / 34.5 / 49.0 / 61.4 / 79.3 / 93.0% at 6 / 10 / 15 / 20 / 30 / 48 m and a
maximum of 127.5 m. This re-derivation gives 21.4 / 34.4 / 49.0 / 61.4 / 79.2 / **94.4**% and
127.9 m. The 48 m row differs by 1.4 points and the maximum by 0.4 m -- a sampling difference, and
it moves no conclusion, but it is recorded rather than smoothed over.)*

### 5. Coastlines -- built, and the ratio GROWS with the ruler

This document added coastline fractality to the roadmap and it was the right addition. The term
ships as an opt-in `CoastParams` block windowed by `|above_shore|`, **not** as extra octaves in
the normalised sum -- see the mistakes section below for why that distinction was the whole task.

**Re-derived here** by running the engine's own survey binary, `cargo run --release --bin
coastline_survey` (about four minutes, single-threaded, this machine). Coastline length is a
Cauchy-Crofton boundary-edge sum on an equirectangular grid, reported only as a **ratio** against
the same grid at amplitude 0 so the estimator's raster bias divides out. Owner's world,
`above_shore > 0`:

| ruler | 100 km | 50 km | 25 km | 12.5 km |
|---|---|---|---|---|
| **fractal / canonical** | 1.358 | 1.472 | **1.591** | **1.639** |
| smooth control / canonical | 1.023 | 1.027 | 1.032 | 1.030 |

**The ratio rises as the ruler halves, and that IS the fractal signature.** The control is the
half of the table that matters: the same amplitude at a frequency *coarser* than the base field's
own finest octave displaces the coastline just as far and returns **1.02-1.03, flat across four
ruler lengths**. A metric that reported "longer" for any perturbation would have put the two
columns together. It does not, so the measure separates displacement from structure.

At the shipped preset (amplitude 0.35), on the same 25 km grid: inlet heads **2 -> 175**, islands
over 100,000 km2 **5 -> 9**, and the field's land fraction moves **-0.017 pp** -- a quarter of the
uncertainty the 4,000-sample calibrator itself carries.

**And it is off.** `CoastParams::canonical()` is amplitude 0, Ruling 1 keeps it there, and the
owner reaches the preset with one slider. The picture does not change until they do.

## What is still not right

The most valuable section, and it is longer than the closed list.

1. **A fifth of every lake's boundary is a straight cut.** The manifest carries a bounding box and
   no footprint, so the drawn set is the box intersected with the level, and wherever sub-level
   ground crosses the box, the box is what stops it. Dilating by one node cell halved the exposure
   and cannot remove it. **The fix is engine-side**: a per-body member list, a polygon, or a
   basin-id field sampled like `wb_fill_tile_f32` -- which would also serve the lagoon tint the
   ocean task declined to invent.
2. **The lakes' shallow rim is a strong halo**, and whether it is right is taste.
3. **Clouds are draped, not floating.** An `ImageryLayer` is painted on the terrain: there is no
   parallax, and at the limb a real deck would overhang the silhouette where this one stops at it.
   The only fix in this stack is a second textured ellipsoid with its own tiling and level of
   detail.
4. **The limb is still thin, warm and hard-edged**, and that is difference #10 substantially
   unclosed. `?limb=24000` produces the reference's blue-cyan halo and opens a dark gap at the
   silhouette. **Ours is thin because it is physically right** -- Cesium's scale heights are
   Earth's, over an Earth-sized drawn ellipsoid -- and the reference's is an illustrator's
   convention. **This is a taste decision and it is the owner's.**
5. **No tone map ships, and the best-measuring one is also an owner decision.** `PBR_NEUTRAL`
   improved ocean spread, ocean span and the land's dynamic ratio -- every measured axis -- against
   shipping nothing. It is not the default because **it swallows the limb almost entirely**: it
   buys difference #12 by spending difference #10, and no measurement in this slice can price that
   trade. `?hdr=1&tonemap=PBR_NEUTRAL` is the switch. (`ACES` was measured and rejected outright:
   it crushes the land's first percentile onto the floor.)
6. **The ground haze cost the land's darkest tone.** Turning it on was decided by re-measurement
   and it is right, but the rendered land's p99/p01 fell about a fifth. `?atmosphere=0` reverts it
   byte-for-byte.
7. **Rivers, settlement lights and sea glint were not attempted.** Rivers are the finest visible
   texture in the reference and the schema exists with no population behind it.
8. **Snow is a contour, not a snow line.** The palette has polar and ice entries and reaches them;
   what it does not have is a climate-driven snow line, so white ridgelines are still an elevation
   threshold scaled by latitude.
9. **`?relief=0` shows no lakes and a coarser ocean.** The `ElevationRamp` fallback is a 256x1
   gradient indexed by height alone, and a lake is a level attached to a *place*. That path is now
   meaningfully less truthful than the default one.
10. **The reference is still a rendered illustration.** The two things this document said would
    remain, remain: it is lit and graded like a film still, and its land detail is suggestive
    rather than derived. **Ours is computed, which is why it is consistent at every zoom and can
    be walked around in a MUD.** That trade is the point of the project.

## The mistakes, which are the transferable part

Recorded as carefully as the successes, because they are what a later slice can actually use.

1. **This document's own ordering was wrong, and research corrected it.** Land colour was parked
   behind a climate slice on a false premise. And the sequencing argument is the sharper half:
   **clouds at 40% coverage hide the land and coastline defects the slice exists to fix**, so
   every land measurement here is taken at `?clouds=0` and every future one must say so.
2. **The coastline brief was wrong in BOTH directions.** It warned about a land-fraction trap the
   codebase already solves by construction -- `Continentality::new` calibrates `shore` and
   `spread` as quantiles of the same field before the struct exists -- and it missed that adding
   a fifth octave to a gain-0.5 normalised sum gives the new term **3.23% of total amplitude**.
   **The task could have passed every check the brief named and produced nothing visible.** The
   replacement is a separate term with its own amplitude, windowed by `|above_shore|`.
3. **A noise field's real standard deviation was a fifth of its nominal one.** A trilinear
   value-noise fBm normalised by its own amplitudes has sd about 0.105 against a nominal +-0.5 --
   averaging eight lattice corners costs that much variance. Every weight built on the nominal
   range was **five times too weak**, moisture collapsed onto latitude, and **nine of
   thirty-three colours were unreachable**. Weights are now in standard deviations of their own
   field, and the same mistake was caught a second time before it shipped in the cloud layer,
   where it would have failed *invisibly*: a layer that is transparent everywhere looks exactly
   like light cloud.
4. **A banding check appeared to test five terms and tested two.** Zeroing only the equatorial
   cloud term turned one test red -- and not the banding one -- because **the two subtropical
   minima either side of the equator manufacture an apparent equatorial maximum out of nothing**.
   Replaced by a per-zone removal test, not loosened.
5. **An ocean stop was reached by zero of 8,294,400 samples**, and the check that was supposed to
   notice asserted a *box* whose floor was a global extreme -- so it could never have failed. A
   check that cannot fail is worse than no check.
6. **Three lake mutations in a row left the centrepiece assertion green**, because the test framed
   each body's tile on the box it was about to draw with: a point box gives a zero-size rectangle,
   every texel lands on the point, and the raster paints regardless. Two more were green because
   an identity check compared the two colour tables' *depths* and never their colours -- which is
   exactly how the two ocean tables had drifted in the first place.
7. **An abort was reachable only through the CROSS PRODUCT of three individually admissible
   fields.** The finest band a record asks for is frequency times lacunarity to the (octaves-1),
   which compounds past an `i64` cast's saturation, and the next line overflows. **A
   one-field-at-a-time sweep stays green**; only the cross product reaches it. Add cross products
   to every export sweep.
8. **A NaN silently returned an abyssal elevation while every finiteness assertion passed.**
   `elevation_from_above` failed both its comparisons for a NaN and fell through to `ABYSS_M`, so
   the planet drowned quietly. One function up, a NaN `land_fraction` saturated a float-to-int
   cast to index 0 and made sea level the field's global minimum -- and **the world that produced
   was bit-identical to the legitimate `land_fraction = 1.0` world**, which is why nobody saw it.
   Not a wrong-looking number: a real world, arrived at by accident.
9. **"Everywhere else a NaN falls to a weight in [0,1]" was REFUTED.** Two sites fall to *angles*
   and one to a categorical flag. The conclusion survived -- all three are oracle-required or
   already refused -- but a sweep that reused that characterisation as its search pattern would
   have missed three sites. The axis the first sweep did not have at all was float-to-int
   saturating casts.
10. **`pool.js` silently dropped the two counters it was not told about.** It rebuilt each raster
    reply from five named fields; the globe drew lakes correctly and reported zero. No unit test
    caught it, and the reason is instructive: **the fake pool in the provider's test was faithful
    and the real dispatcher was not** -- a fake that mirrors an interface hides a defect in the
    implementation of that interface. It took a live run against the real browser, real workers
    and real pool.

## Two warnings for whoever reads the pictures

- **`shoot.mjs`'s plain `shoot` command renders at `JulianDate.now()`.** Pinning `viewer.clock`
  does nothing, because `Scene.render()` with no argument defaults its frame time. **Screenshots
  taken with `shoot` in the earlier tasks of this slice may therefore be lit differently on the
  two sides of a before/after pair, and some of those have already been shown to the owner.** The
  `digest` path drives twenty frames by hand at an explicit `JulianDate` and is correct; the later
  tasks used `digest --out` for that reason. The fix -- a `--pin` flag on `shoot` -- is one line
  and has no owner.
- **A body count, an island count and a peak height are all properties of the ruler.** The lake
  count is quoted beside its node count *and* its seed; the landmass count beside its grid
  spacing; and the highest point on the owner's world measures **4,776.2 m** on a 181-post scan
  and **5,011.1 m** on a 361-post one (32 tiles of 45 degrees, `wb_fill_tile_f32` at canonical
  resolution, 4,170,272 samples, node v22.17.0). Neither is "the planet's peak", and a figure of
  4,978 m appears in one task report while 4,540 m appears in this document's own Tier 2 line.
  **Quote the grid or do not quote the number.**
