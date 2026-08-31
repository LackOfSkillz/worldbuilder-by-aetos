# Brief for a second opinion

Paste this whole file. It is written to be read cold.

---

## What exists already

I have built a **maritime simulation** as a contrib for Evennia, a Python MUD framework.
It is text-first and quite deep: sailing physics with a polar curve per rig, sail plans
from bare poles to full sail, wind and current as separate vectors, crew with quality and
morale, cargo and lading, gunnery and boarding, mooring and berthing.

Two parts matter for what follows.

**Charts model knowledge rather than truth.** The engine knows exactly where a ship is;
the people aboard do not. A vessel carries a *reckoned* position that accumulates error
from current and leeway, and the chart is drawn from that. Charts also record their own
*coverage* - water nobody surveyed comes back as a hole in the paper rather than as open
sea - and they are wrong in the same places every voyage, deliberately, so that taking a
fix and sounding are worth doing.

**The world is a seam.** Maritime does not contain a map. It calls a provider object:

    terrain_z_at(position) -> metres relative to sea-level datum
    bottom_type_at(position) -> sand, rock, mud

`position` is flat metres - x east, y north - inside a named region.

There is also an optional graphical web client with a chart panel that contours the
coastline live out of that provider, at whatever zoom the player chooses.

## The problem

The demonstration world is a placeholder called `ShoalingShelf`, whose depth is a function
of easting **alone**. It is a one-dimensional ramp. At any zoom wider than its single rock
ledge there is nothing to draw but a straight north-south line, because that is all there
is. The chart has been faithfully drawing a ramp.

## What I want to build

A second, standalone contrib: **a planet generator**. A developer clicks generate, gets a
rendered globe, customises it, and then places their *already built* Evennia zones onto
landmasses - which is what gives maritime somewhere real to sail. Beyond the placed
content, the rest of the planet is discoverable.

## The constraint everything answers to

**Measured, not assumed:** one chart redraw calls `terrain_z_at` **9,216 times** and takes
about **45 ms**, for arbitrary points, at arbitrary zoom, wherever a ship happens to be.

So the generator's output cannot be a stored map. A heightmap coarse enough to store for a
planet has no coastline at ship scale; one fine enough for ship scale is far too large to
store. It has to be **a deterministic function of position**.

Determinism is not only about speed. A chart that is wrong in the same places every voyage
is what makes navigation mean anything. A world that answered differently on a second
asking would delete that without anyone noticing.

## What we have decided, and why

**A true sphere**, Earth-sized - 6,371 km radius. A cylinder wrapping east-west was
seriously considered and rejected: it gives equator, poles, trade winds and
circumnavigation for much less work, but the point is a planet. Shrinking the planet to
shorten voyages was also rejected, because radius is the parameter that quietly breaks the
physics - horizon distance, the Coriolis that drives the wind bands, the sense of
roundness. On a quarter-Earth a lighthouse that should loom at 16 nautical miles appears
at four. Playability is bought with time compression instead.

**A maritime region is a tangent plane on the sphere.** This is how a flat simulation
sails on a globe, and it is not a compromise - it is what a chart *is*. Every real chart
projects a curved surface onto flat paper, and navigators use plane sailing over short
distances and great circles only for ocean passages. A region carries a reference latitude
and longitude; local metres project onto the sphere. Distortion grows toward a region's
edges, which is correct: a chart is supposed to be slightly wrong at its edges.

**Terrain by plate tectonics, then noise.** 18-24 plate seeds spread over the sphere by
Fibonacci spiral. A point's plate is its nearest seed; its distance to a boundary is
arithmetic on two great-circle distances - **so this remains a pure function of position**,
with the seeds being a few hundred bytes rather than a map. Each plate has a drift vector
and a kind (oceanic or continental). The relative drift along a boundary decides what
forms there: continent-continent convergence makes a high range, ocean-continent makes a
coastal range plus a trench, ocean-ocean makes an island arc, divergence makes a rift.
Fractal 3D noise on top gives coastline detail - three dimensions because a 2D field
cannot wrap onto a sphere without a seam and pole distortion, and octaves because the
chart asks for the same ground at 22 miles and at 1 mile and needs real detail at both.

**Climate is derived, never authored.** "Earthlike" is not about coastlines, it is about
*correlation*: deserts sit near 30 degrees because that is where Hadley cells return dry
air to the ground; rainforests sit on the equator and on windward coasts; gyres turn
clockwise north of the equator and anticlockwise south. Temperature from latitude and
elevation; wind from latitude bands (trades, westerlies, polar easterlies); rainfall from
band, upwind ocean fetch and orographic lift; currents as wind-driven gyres.

**Five layers, which are also the things to discover:**

| Layer | Derived from | Discovery |
|---|---|---|
| Terrain | position, seed | a landmass |
| Climate | latitude, elevation, wind | - |
| Biome | climate, elevation | a new species |
| Flora | biome | a new spice |
| Settlement | biome, coast, water | a new population |

The payoff is a *skill*: because climate is honest rather than sprinkled, a captain who
has found a spice twice can reason toward the third - right latitude, wet side of the
island - instead of searching at random.

**Discovery** is modelled on Elite Dangerous: tiered fidelity, carried at risk, paid only
on return, permanently attributed. Maritime already has three of the four tiers under
other names - "land ho" from the masthead is the coarse scan, running the coast is the
resolve step, sounding is the detailed survey, and only "report to the admiralty" is new.
Two departures: survey *quality* is real, because a hasty running survey produces a chart
that is genuinely inaccurate and grounds you later; and the survey is paper aboard a ship,
so losing the ship loses it, which is what happened to real expeditions. Everyone is paid
whether or not the ground is already known - avoiding a frontier that goes dead once
charted - and the first keel home with a landmass, population, spice or species is paid
three times over, in shares split as prize money was.

**The demo world**: a stretch of continental coast plus five or six charted islands,
deliberately sited as an *island arc* on a convergent boundary - because an arc continues,
so the known islands are the near end of a chain and following it is the first expedition
a captain can reason his way into. Everything else on the planet is unknown.

## The risk I think this design has

**Pure Python is about three times over the sampling budget.** Measured, not guessed - I
wrote a throwaway probe with 20 plate seeds, a nearest-and-second-nearest search, and four
octaves of 3D value noise, and ran it over a full redraw's worth of points:

    9,216 samples (one chart redraw)      134.7 ms
    per sample                             14.6 microseconds
    budget                                  5.0 microseconds
    over by                                 2.9x

I had guessed ten to thirty times over. It is three. So a redraw would cost roughly 135 ms
of sampling on top of about 45 ms of contouring - noticeable, but this is a debounced
redraw that happens when somebody zooms, drags or resizes, not per frame.

Three routes to closing the gap, and I think one of them is correct regardless of speed:

- **Scale octaves to the zoom.** At 22 miles with a 96-square grid the samples are 421
  metres apart, so noise octaves finer than a few hundred metres are invisible - they are
  costing time to produce detail below the resolution being drawn, and worse, they alias.
  Dropping them at wide zoom is both faster and *more correct*. This is the one I would do
  first even if performance were free.
- **Batch the grid.** Maritime samples a grid rather than scattered points - the
  contouring builds a square array of elevations and marches squares over it - so the
  whole grid could be evaluated at once with numpy. The catch is that the seam is
  point-wise, `terrain_z_at(position)`, so this means adding a batch method to a public
  interface.
- **Cache the plate lookup**, which changes slowly across the surface and is a third of
  the cost.

## Where I would value your input

1. **Voronoi plates.** Does nearest-seed-plus-drift actually produce earthlike continents,
   or are there known failure modes? I am aware the boundaries will be straighter than
   real plate margins - is domain warping the standard fix, and does it survive being a
   pure function?

2. **The performance problem above.** Is zoom-adaptive octave count the standard answer to
   this, and is there a name for it I should be looking up? Is batching the grid worth
   widening a public interface for, given it is only 2.9x and the redraw is debounced?

3. **The climate model.** Is latitude bands plus orographic lift enough to look right, or
   is there a cheap model that is markedly better? Rainfall in particular worries me,
   because upwind fetch seems to need tracing a path rather than evaluating a point, which
   breaks the pure-function property.

4. **The tangent-plane identification.** Are there failure modes I have not thought of -
   particularly for a ship sailing far enough that distortion matters, or crossing between
   regions?

5. **The discovery economy.** Everyone paid, first paid triple. Does that inflate? Does it
   hold up once a world is mostly charted?

6. **What are we missing?** Especially anything that will be expensive to add later and
   cheap to design in now.

Please argue with the decisions rather than improving on them politely. Where you think
something is wrong, say so and say why.
