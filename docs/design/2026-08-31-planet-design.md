# A planet for maritime to sail on

Working design. Nothing here is built. This records what has been decided, what it was
decided *against*, and the reasoning - so that the next session starts from the argument
rather than from the conclusion.

## The problem

Maritime is a simulation of sailing that currently has nowhere to sail. Its demonstration
world, `ShoalingShelf`, returns depth as a function of easting alone: a one-dimensional
ramp. At any zoom wider than its single rock ledge there is nothing on the chart but a
straight north-south line, because that is genuinely all there is. The chart has been
faithful the whole time.

What is needed is a world. What is wanted, eventually, is a generator that gives any
Evennia developer one.

## The constraint everything answers to

**Maritime does not read a map. It asks the world how high the ground is at a point.**

Measured, not assumed: one chart redraw calls `terrain_z_at` **9,216 times** and takes
about **45 ms**, for arbitrary points, at arbitrary zoom, wherever a ship happens to be.

Two things follow.

**The world cannot be stored.** A heightmap coarse enough to hold for a planet has no
coastline at ship scale; one fine enough for ship scale is far too large to hold. The
world has to be a function evaluated on demand.

**The world must be deterministic.** This is not a performance point. A maritime chart is
*wrong in the same places every voyage*, which is what makes surveying, dead reckoning and
taking a fix worth doing. A world that answered differently on a second asking would
delete navigation without anybody noticing.

## Decided

### A true sphere

Not a plane, not a cylinder. Longitude converges at the poles, courses are great circles,
and sailing east for long enough brings you home.

Considered and rejected: a **cylinder** wrapping east-west with flat latitude and
impassable ice at the poles. It gives equator, poles, trade winds, gyres and
circumnavigation while leaving maritime's flat-metre navigation untouched, and it is
markedly less work. Rejected because the point is a planet.

The cost is real and is accepted: great-circle courses, converging meridians, a projection
for the chart, and navigation code that currently assumes a plane.

### Earth-sized

Circumference ~40,000 km. An ocean crossing is therefore about 5,000 km, which at six
knots is **18.8 days of continuous sailing**.

Considered and rejected: shrinking the planet to make voyages shorter. Radius is the one
parameter that quietly breaks the physics - horizon distance, the Coriolis effect that
drives the wind bands, and the sense of roundness all come from it. On a quarter-Earth a
lighthouse that should loom at 16 nautical miles appears at four.

Playability is bought with **time compression** instead, which costs nothing physical.

### Procedural, evaluated on demand

See the constraint. One exception, below.

### Climate is derived, never authored

Earthlike is not a matter of coastlines. It is a matter of **correlation**: deserts sit
near thirty degrees because that is where Hadley cells return dry air to the ground;
rainforests sit on the equator and on windward coasts; mountains cast rain shadows; ocean
gyres turn one way north of the equator and the other way south.

A generator that makes continents and then sprinkles biomes onto them reads as false
immediately. One that derives climate from latitude, elevation and prevailing wind gets
Earth for nothing, and cheaply, because latitude and elevation are already to hand.

Winds and currents are the easiest part of this and the most valuable to maritime, which
already consumes both.

## The four layers

Each layer derives from the one above it, and each is simultaneously a class of thing to
discover. That correspondence is the reason to build it in this order.

| Layer | Derived from | Discovery |
|-------|--------------|-----------|
| Terrain | position, seed | a landmass |
| Climate | latitude, elevation, wind | - |
| Biome | climate, elevation | a new species |
| Flora | biome | a new spice |
| Settlement | biome, coast, water | a new population |

**The payoff is a skill.** Because climate is honest rather than sprinkled, an experienced
captain can *reason* towards a discovery: if nutmeg grows in wet tropical lowlands on
windward coasts, somebody who has found it twice knows where to look the third time. That
only works if the correlation is real.

## Discovery

Modelled on Elite Dangerous, whose loop is: tiered fidelity, carried at risk, monetised
only on return, and permanently attributed. Maritime already has three of the four tiers
under other names.

| Elite | Maritime | Exists? |
|-------|----------|---------|
| Discovery scanner - something is there | **Land ho** from the masthead, at geographic range | yes |
| FSS - resolve what it is | **Running the coast** - outline enters the chart from seaward | coverage is modelled |
| Surface probes - map the detail | **Sounding** - cast the lead, send boats in, find the anchorage | `sound` exists |
| Sell at a station | **Report to the admiralty** | no |

Two departures from Elite, both improvements for this setting:

**Survey quality is real.** Maritime charts are deliberately wrong where they are wrong. A
running survey taken at speed from seaward therefore produces a chart that is *actually
inaccurate*, and the inaccuracy turns up later as a grounding. Sounding properly costs
days and produces paper worth trusting. Elite has no equivalent axis.

**The survey is paper aboard a ship.** Lose the ship, lose the survey - which is not a
game contrivance but what happened to real expeditions. A specimen of a new spice is not
data either; it is cargo, taking up hold space, competing with paying freight.

### Rewards

Everyone is paid for survey work, whether or not the ground is already known - which
avoids Elite's dead frontier, where a charted region stops being worth visiting. The
**first** captain to bring back a landmass, a population, a spice or a species gets
**three times** the reward.

An unreported survey is worth more unreported to a captain who wants a private passage,
and worth money and fame reported. That is a real decision, every time.

### Two units, on purpose

- **Chart squares** are the unit of survey and payment. They compose directly with the
  coverage model the charts already have, and they are trivially computable.
- **Named features** - this island, that cape - are the unit of naming and fame. Nobody
  boasts of having charted square 47-J.

**This is the one place the world stops being a pure function.** Naming a thing requires
knowing it is one thing, which is connected-component analysis over a region rather than a
point query. It needs a coarse enumeration pass over the terrain. Accepted deliberately,
and kept coarse.

## Placement

A developer generates a planet, customises it, and then places **existing Evennia zones**
on a landmass. Placement is coarse and rare - a dozen harbours on a planet, not a
coastline.

- Placing a zone establishes up to **three harbours** for it.
- **Landmarks** may be pinned: a lighthouse, a tall spire.

Landmarks need no new mathematics. Maritime already models geographic range, so a pinned
height at a pinned position appears at the correct distance, and is raised from aloft
before the deck sees it. Measured, from a cutter:

| | from the deck (2 m) | from aloft (18 m) |
|---|---|---|
| a low headland, 20 m | 12.2 nmi | 18.0 nmi |
| a lighthouse, 40 m | 16.0 nmi | 21.9 nmi |
| a tall spire, 70 m | 20.2 nmi | 26.1 nmi |
| a mountain, 400 m | 44.3 nmi | 50.2 nmi |

That is the loom of a light over the horizon, working today.

## Open

- **How large is the known world, and what is in it?** Earth-sized planet, small charted
  bubble, discovery beyond. Its size sets the whole early game.
- Are there blanks *inside* the known world, or only beyond its rim?
- What is "civilisation" for the purpose of reporting a discovery - any port, or named
  ones?
- How are continents actually generated? Plate boundaries as a Voronoi diagram over the
  sphere would stay a pure function while giving genuine ranges, arcs and trenches; layered
  noise is cheaper and blobbier. Not yet decided.
