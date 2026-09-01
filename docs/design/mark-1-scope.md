# Mark 1

The prototype built in a cave, out of scraps. It takes a few bullets, it leaps, and it
survives the landing three miles away. It is not the Mark 5, and every temptation to make
it the Mark 5 is to be refused on sight.

The second half of this document is a request for outside review. Everything above the
line is what we have decided; everything below it is what we are unsure about.

## Why a prototype rather than the real thing

The maritime contrib has a working simulation, a working chart, and a working graphical
client - and nothing to sail on. Its demonstration world returns depth as a function of
easting alone: a ramp, whose coastline is a straight north-south line at every latitude.

The cost of that is not aesthetic. **Every interface decision of the last few hours has
been judged against a ramp** - how dense the printed soundings should be, how fine to
contour, how much contrast a range ring needs, how much sea a chart should show, whether
the coastline reads at all. Some of those judgements are probably wrong and there is no
way to tell.

There is a second reason, and it is the stronger one. The outside review's best point was
that for a maritime simulation **the first hundred metres below sea level matter more than
the continents** - shelves, bars, channels, estuaries, reefs. That was accepted
immediately. But nobody involved actually knows what "good" looks like on that chart yet.
Building a cheap seabed and sailing it is how we find out, *before* specifying the
first-class bathymetry stage we would otherwise design blind.

So Mark 1 is not a detour from the architecture. It is the thing that tells us what to put
in it.

## Scope

**In:**

- A deterministic seabed for one region, from a seed.
- Flat coordinates - the testbed's local metres. No sphere.
- Layered noise with **band-limited octaves**, dropped below the sampling footprint,
  because that is correct regardless of speed and we have already established why.
- A small vocabulary of shaped bathymetric features, enough to sail and to chart:
  a continental coast, an arc of five or six islands, a shelf that shoals from deep water,
  a couple of banks, a channel between them.
- Implements `MaritimeMapProvider` - `terrain_z_at`, `bottom_type_at` - and nothing else.
- Lives in this repository, not in the testbed, because most of it survives.

**Out, deliberately:**

- The sphere. Plates, Euler poles, crust fields, terranes.
- Anchors, seams, footprints, authority policies.
- Climate, biomes, flora, settlement.
- Hydrology and drainage graphs.
- The globe renderer and any user interface at all.
- Generator versioning.
- Discovery, surveys, rewards.

Every one of those is designed in `anchoring-architecture.md` and
`2026-08-31-generator-spec.md`, and every one of them stays there until Mark 1 has taught
us something.

## What survives into Mark 5

| Survives | Gets replaced |
|---|---|
| the noise machinery | flat coordinates become spherical |
| band-limited octaves | hand-placed features become consequences of plates |
| the bathymetric vocabulary | |
| the provider wiring | |
| everything learned about what a chart needs | |

That is a coordinate change and a source change, not a rewrite. The shelf that Mark 1
places by hand is the same shelf Mark 5 derives from a passive continental margin; the
code that turns "there is a shelf here" into elevations is the part worth keeping.

`ShoalingShelf`, the existing ramp, stays exactly where it is so the grounding tests keep
their rock ledge and its apron.

## The budget, unchanged

**9,216 calls per chart redraw.** A throwaway probe with plate seeds and four octaves of
3D value noise measured 14.6 microseconds a sample - 2.9x over the 5 microsecond budget,
against a guess of 10-30x. Mark 1 has no plate search at all, and band-limiting removes
octaves that are invisible anyway, so it should come in comfortably. To be measured.

---

# Request for review

We have deliberately scoped this down. The question is whether we have scoped it down
*correctly* - cheap enough to be a prototype, structured enough not to be thrown away.

**1. Which of our Mark 1 decisions is hardest to reverse?**
This is the one that worries us most. What looks cheap now and is expensive in three
months? We think flat-instead-of-spherical is safe because it is a coordinate change at
the boundary, but we would like that challenged.

**2. Is hand-placing bathymetric features a trap?**
Mark 1 puts a shelf, an island arc and a channel where we want them. Mark 5 derives them
from plate boundaries. Is there a shared representation - some intermediate description of
"a shelf runs along here, of this width and profile" - that both a hand-placed prototype
and a tectonic generator could emit, so the code that turns description into elevation is
written once? Or is trying to unify those two prematurely, and we should accept writing
the shaping twice?

**3. What is the minimum bathymetric vocabulary that teaches us something?**
We listed coast, island arc, shelf, banks, channel. Is that enough to reveal what the
chart needs, or is there something we will regret leaving out - a river mouth, a reef, a
steep-to headland - because its absence hides a whole class of chart problem?

**4. Should Mark 1 carry a generator version from day one?**
It is on the "out" list. The argument for including it is that it costs almost nothing and
the whole design depends on it later. The argument against is that Mark 1 worlds are
disposable by definition, so versioning them is ceremony. We are genuinely unsure.

**5. What are we about to learn the hard way?**
Anything you would expect a first seabed to get wrong, that we could simply not get wrong.

---

# M1.1 result: the projection, and the region cap it earns

**Projection chosen: azimuthal equidistant.** Distance and bearing from the frame's origin
are exact at any range, by construction. The error lives entirely in how two points *away
from* the origin relate to each other.

Measured rather than asserted, which is what the earlier `(d/R)^2 / 2` guess could not be:

| range | radial error | transverse error | as a fraction |
|---|---|---|---|
| 25 km | 0 | 0.01 m | 0.0003 % |
| 50 km | 0 | 0.09 m | 0.0010 % |
| 100 km | 0 | 0.71 m | 0.0041 % |
| 200 km | 0 | 5.68 m | 0.0163 % |
| 500 km | 0 | 88.76 m | 0.1019 % |
| 1000 km | 0 | 709.50 m | 0.4087 % |

**The cap: 200 km, from a stated tolerance.** At 200 km the worst error between two charted
points is under six metres - a third of a cutter's length, and far below the four hundred
metres between printed soundings on a chart. At 500 km it is 89 m, which is several ship
lengths and would show as a bad landfall. The tolerance is therefore *error smaller than a
ship*, and 200 km is where that holds with room to spare.

**The error does not depend on latitude.** Measured at 0, 45 and 80 degrees: identical to
six decimal places. That is a real consequence of working in unit vectors - high latitudes
are not a special case, which a latitude-and-longitude implementation could never have
claimed - and it is why the polar handling is two lines rather than a subsystem.

**At a pole, east is chosen rather than derived.** Every direction from the north pole is
south and none is east, so the cross product that defines east goes to zero. A fixed
reference direction is used instead. Which direction is irrelevant; that it is the same one
on every call is the whole requirement, because a frame that reshuffled itself between two
calls would move every ship it held.

23 tests, covering both poles exactly, the band just off them, the antimeridian from both
sides, and the local-sphere-local round trip at five frames.
