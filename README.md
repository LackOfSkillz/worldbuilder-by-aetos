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

## Layout

    docs/design/anchoring-architecture.md    the core: anchors, seams, authority
    docs/design/2026-08-31-planet-design.md  decisions and what they were taken against
    docs/design/2026-08-31-generator-spec.md the generator itself
    docs/design/gpt-brief.md                 the brief sent for outside review
