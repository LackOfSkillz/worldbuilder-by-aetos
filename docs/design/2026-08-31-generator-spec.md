# Generator specification

What to build, and why each part is shaped the way it is. Written before any code. The
reasoning behind the decisions is in `2026-08-31-planet-design.md`; this is the
buildable form of it.

## 1. The sphere, and how a flat simulation sails on it

The planet is a sphere of Earth's radius, 6,371 km, circumference 40,030 km.

Maritime navigates in flat metres: `WorldPosition` is x east, y north, inside a named
`region`. That looks incompatible with a globe and is not, because of one identification:

> **A maritime region is a tangent plane on the sphere.**

This is not a compromise made to avoid work. It is what a chart *is* - every real chart is
a projection of a curved surface onto flat paper, and every real navigator works in plane
sailing over short distances and switches to great circles only for ocean passages. A
region carries a reference latitude and longitude; local metres convert to a point on the
sphere by the standard tangent-plane projection; the generator answers there.

Consequences, all of them wanted:

- Maritime's existing flat mathematics stays correct everywhere a ship actually sails.
- Distortion grows with distance from the reference point, exactly as it does on a real
  chart, which is a feature: a chart is *supposed* to be slightly wrong at its edges.
- Great-circle sailing becomes a later addition for ocean crossings rather than a
  precondition for anything.

## 2. Layer one: terrain

Elevation in metres relative to sea-level datum, as a pure function of a point on the
sphere and the planet's seed.

### Plates

Between 18 and 24 plate seeds are distributed over the sphere by the Fibonacci spiral,
which gives near-even spacing deterministically, then jittered by the seed. Each plate
carries:

- a **drift** unit vector tangent to the sphere at its seed
- a **kind**, oceanic or continental, about a third continental
- a **base elevation**: roughly +200 m continental, -4,000 m oceanic

A point's plate is its nearest seed. The distance to the boundary between its plate and
the next nearest is arithmetic on two great-circle distances. **Both are pure functions of
position**, which is what lets the tectonic structure exist without any of it being
stored - the seeds themselves are a few hundred bytes, the seed expanded rather than a map.

### Boundaries

Where two plates meet, the component of their relative drift along the line between them
says what kind of boundary it is:

| Relative motion | Result | Earth's example |
|---|---|---|
| Converging, continent + continent | high range, no trench | the Himalaya |
| Converging, ocean + continent | coastal range, trench offshore | the Andes |
| Converging, ocean + ocean | **island arc**, trench | Japan, the Aleutians |
| Diverging | rift, deep water, mid-ocean ridge | the Mid-Atlantic Ridge |
| Sliding | little vertical effect | the San Andreas |

The effect is strongest at the boundary and decays over a width of roughly 300 km.

### Detail

Fractal noise sampled in three dimensions at the point's position on the unit sphere, in
several octaves, added to the tectonic base. Three dimensions rather than two because a
2D noise field cannot be wrapped onto a sphere without a seam and severe distortion at the
poles.

**Octaves are what make the chart work at every zoom.** The chart is asked for the same
ground at 22 miles and at 1 mile; fractal noise has coastline detail waiting at both, so
zooming in reveals real structure rather than a smooth interpolation of something coarse.

### Sea level

A single global datum. Adjusting it is the cheapest and most powerful customisation a
developer has: raising it drowns continental margins into archipelagos, lowering it joins
islands into landmasses. It changes nothing else and costs nothing.

## 3. Layer two: climate

Derived, never authored.

**Temperature** from latitude, less a lapse rate against elevation, plus a seasonal term
about the planet's axial tilt.

**Wind** from latitude bands, which is where Earth's weather comes from: trade winds
easterly from the equator to about 30 degrees, westerlies from 30 to 60, polar easterlies
beyond, with the doldrums at the equator and the horse latitudes at the band edges. The
bands shift with season.

**Rainfall** from three things multiplied: the band (rising air at the equator and at 60
degrees is wet, descending air at 30 and at the poles is dry), how much ocean the wind
crossed to get here, and orographic lift - windward slopes wet, leeward slopes in rain
shadow.

**Currents** as gyres driven by the wind bands and turned by Coriolis: clockwise in the
northern hemisphere, anticlockwise in the southern, with an equatorial counter-current and
a circumpolar current in the far south where no land interrupts it.

Maritime already consumes wind and current, so this layer is immediately load-bearing
rather than decorative.

## 4. Layers three to five: what lives there

**Biome** from temperature and rainfall by the standard Whittaker classification, with
elevation modifying it - tundra above the treeline at any latitude.

**Flora**, including the spices that are worth carrying home, from biome plus a
per-region seeded variation so that not every tropical island grows the same thing.

**Settlement** where a biome will support people *and* there is water access - a river
mouth, a sheltered bay, a natural harbour.

The correspondence with discovery is the reason for this ordering: a landmass, a species,
a spice, a population are one per layer.

## 5. Features, and the one exception

Everything above is a pure function of position. Naming is not: calling something "this
island" means knowing it is one island, which is connected-component analysis over a
region rather than a point query.

So a planet is enumerated **once**, at creation, by a coarse pass - land sampled at about
10 km and flood-filled into connected masses, each recorded with its centroid, extent,
area and kind. That is a few million samples and a handful of seconds, run once, stored
as a list of features rather than as a map.

Kept deliberately coarse. It is enough to know that an island exists and roughly where;
its actual coastline still comes from the function, at whatever zoom is asked for.

## 6. The demo world

An Earth-sized planet with a small charted bubble and everything else to find.

### Known

- A stretch of **continental coast**, on the order of 400 km, with the ports the game
  already has placed along it.
- **Five or six islands** offshore, charted, within a day or two's sail.

The islands are deliberately an **island arc** - the near end of an ocean-ocean convergent
boundary. This is worth more than a scattering of islands would be, because an arc
*continues*. The known islands are the beginning of a chain, and the first real expedition
a captain can reason his way into is following it further. The plate model gives that for
free; scattered noise could not.

### Unknown

Everything else: the rest of the arc, the far side of the continent, other continents,
open ocean, and the cities and peoples on them.

Charted coverage is set for the known region only. Outside it the paper is blank, which
maritime already draws as *absence* rather than as empty sea.

## 7. Placement

A developer generates a planet, adjusts it, and places existing Evennia zones on it.
Placement is coarse and rare: a dozen harbours on a planet, not a coastline.

- Placing a zone establishes its reference latitude and longitude - which is exactly the
  tangent-plane origin from section 1 - and up to **three harbours** for it.
- **Landmarks** may be pinned, each with a position and a height.

Landmarks need no new mathematics. Maritime already models geographic range: a 40 m
lighthouse is raised at 16 nautical miles from a deck and 21.9 from aloft, so a pinned
height at a pinned position appears at the correct distance and is seen from the masthead
before the deck.

## 8. Discovery

Four tiers, of which maritime already has three under other names: **land ho** from the
masthead, **running the coast** to get its outline, **sounding** to find the anchorage,
and **reporting** to the admiralty, which is the only new one.

Two units:

- **Chart squares** for survey and payment, composing directly with the coverage the
  charts already model.
- **Named features** for naming and fame, from the enumeration in section 5.

**Survey quality is real.** These charts are deliberately wrong where they are wrong, so a
running survey taken at speed produces paper that will ground somebody later, and a proper
boat survey with soundings costs days and produces paper worth trusting.

**The survey is paper aboard a ship.** Lose the ship, lose the survey. A specimen is
cargo, taking hold space, competing with paying freight.

### Reward

Everyone is paid for survey work whether or not the ground is already known, which avoids
a frontier going dead once charted. The first keel home with a landmass, a population, a
spice or a species is paid **three times over**.

Paid in **shares**, as prize money was: the captain took two eighths, the lieutenants and
master one between them, the warrant officers one, the petty officers one, and the whole
crew shared two. A seaman's share was small and real, and it is why crews cheered a sail
on the horizon. A crew with a share in the find has a reason to care about the voyage, and
maritime already models morale as a band that such a thing could move.

**This layer computes shares and never currency.** Money is the host game's, as it is
everywhere else in maritime; the discovery layer says who is owed what fraction and hands
that over.

## 9. The seam with maritime

One class implementing `MaritimeMapProvider`, holding a planet and a region's
tangent-plane origin, answering `terrain_z_at` and `bottom_type_at` by projecting local
metres onto the sphere and evaluating the layers.

Maritime learns nothing about planets. It goes on asking how high the ground is at a
point, exactly as it does of the one-dimensional ramp it has now.

### Budget

The number this design answers to: **9,216 calls to `terrain_z_at` per chart redraw, in
about 45 ms**. That is roughly **5 microseconds a sample**, and it is the acceptance test
for the terrain layer. Plate lookup is a nearest-of-24 search and a few great-circle
distances; noise is several octaves. Both are within budget, and both must be measured
rather than assumed - as the 9,216 was.

## 10. Still open

- Whether biome and settlement are evaluated per sample or only at feature scale. Terrain
  is the only layer the chart asks for at ship scale; the rest may be far coarser.
- What counts as "civilisation" for reporting a discovery - any port, or named ones.
- How a developer adjusts a generated planet beyond sea level and reroll.
- Whether the known region's charted coverage is authored or derived from where the
  placed zones are.
