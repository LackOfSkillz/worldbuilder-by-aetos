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
