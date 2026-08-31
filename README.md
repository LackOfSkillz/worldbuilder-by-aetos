# Worldbuilder by Aetos

A generator for whole planets, meant to be the ground a maritime simulation sails on.

The goal is that a game developer describes a planet rather than draws one - how big, how
wet, how warm, how much of it is sea - and gets back a world with continents, oceans,
poles, rivers, prevailing winds and ocean currents that agree with each other. Areas the
game has already built are then placed on it, rather than the world being built around
them.

This repository is groundwork. The eventual product is a standalone Evennia contrib; what
is here now is the design and the generator needed to give the maritime contrib a real
world to demonstrate on.

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

## Decisions taken so far

- **A true sphere.** Not a plane, not a cylinder. Longitude converges at the poles,
  courses are great circles, and sailing east for long enough brings you home.
- **Procedural, not stored.** See the constraint above.
- **Climate is derived, never authored.** Wind, current, temperature and rainfall all
  fall out of latitude, elevation and each other.

## Layout

    docs/design/     the spec, written before any generator is
