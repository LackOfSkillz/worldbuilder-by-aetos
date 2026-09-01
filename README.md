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

## Layout

    CHANGELOG.md                             phase by phase, with the measurements
    docs/design/anchoring-architecture.md    the core: anchors, seams, authority
    docs/design/2026-08-31-planet-design.md  decisions and what they were taken against
    docs/design/2026-08-31-generator-spec.md the generator itself
    docs/design/mark-1-scope.md              what Mark 1 is for, and each phase result
    docs/design/gpt-brief.md                 the brief sent for outside review
