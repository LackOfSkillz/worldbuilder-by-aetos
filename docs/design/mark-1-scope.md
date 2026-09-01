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

---

# M1.7 result: placed features, and the second channel a chart needs

Everything before this phase decides what ordinary ground looks like. This is the other
channel: a short list of named things, stamped where somebody wants them.

The division only became defensible because M1.6 proved its half of it. Detail makes
texture and demonstrably not landforms - land fraction moves by under two per cent when it
is applied - so a bank on the chart is a bank somebody placed, and finding one means
something. Had noise been allowed to make plausible shoals, this phase would have had
nothing left to do and every hazard in the world would have been an accident of a spectrum.

## Constructed, not found

The obvious move is to search the globe for a coast with a good natural harbour. That needs
a global enumeration pass - sample the planet, score every coastal cell, keep the best -
which is exactly the stored-world machinery Mark 1 exists to avoid.

So the coast was chosen for its *ordinary* virtues, which a few thousand samples can find,
and the interesting parts were placed.

## Choosing it took three attempts, and each failure was a measurement of the wrong thing

**First: the land gradient measured at the wrong place.** A sweep kept coasts at mid
latitude, no tectonic contribution worth speaking of, a monotonic shelf, and land rising
inland. Nineteen survived, the best was taken, and the render showed no harbour at all -
just a slightly deeper patch of an already-submerged plain. The land rose at four tenths of
a metre a kilometre, because "rising inland" had been measured at the *sampled candidate*,
which sat twenty kilometres inland of the actual shore.

**Second: the world does not have the coast I was looking for.** Measured at the waterline
across seventy-two monotonic passive coasts, this planet offers a median of four metres of
land four kilometres inland and a best of twenty-seven. Its margins are gentle everywhere.
That is not a defect - a passive margin *is* gentle, and the shelf phase was built to make
it so - but it means a harbour here is a low-lying one with moles rather than a fjord, and
the demonstration had to accept the world it was placed in.

**Third: the alongshore axis was not parallel to the shore.** The seaward bearing had been
taken from the steepest descent of *continentality*. A shoreline is a contour of the
**finished** field, and the shelf and the tectonics both tilt it, so those are different
directions. A line meant to run parallel to the beach two kilometres inland went from
fourteen metres of land at one end to fourteen metres of water at the other, and a harbour
cut on it had open sea on one flank.

Taken from the structural field instead, the same line holds between 6.3 and 7.5 metres
over sixteen kilometres, which is what parallel to a shore means:

    anchor        21.5841 S, 149.8703 E
    seaward       296.5 degrees true
    inland        +3 m at 1 km, +6 at 2, +13 at 4, +31 at 10
    seaward       -6 m at 2 km, -25 at 8, -50 at 16, -121 at 40

The lesson is one this project keeps relearning: **a diagnostic that measures a proxy tells
you about the proxy.** Land fraction was measured on equirectangular pixels in M1.3, the
coastal distance was measured to the wrong zero in M1.5, and here the coast was measured
with the wrong gradient. Each time, the generator was right.

## Composition is explicit, and it is one-way

A feature says what the ground should be and how it should argue with what is there.
`RAISE` may only make it shallower, `CARVE` only deeper, `SHAPE` either. Adding offsets
would have let a bank inside a channel cancel out into ordinary seabed.

**One-way is not a hard decision in disguise.** A raise whose target is already below the
ground contributes nothing, and at the moment the two are equal it contributes nothing
either - so the switch happens exactly where the effect is zero, and the ground stays
continuous. That is the same argument every tectonic gate in M1.4 had to survive.

The *authority* a feature has over detail needed that argument made a second time and
differently. It would have jumped from nothing to full weight the instant a feature began
to apply, putting a ring of abruptly smooth seabed around every bank. It ramps over three
metres of relief instead - which is also the behaviour worth having, since a feature
reshaping the bed by centimetres has no business taking its texture away.

## Two placement bugs, both found by measuring

Neither was visible in a render, and both are the kind that would have been argued about.

**The approach channel dredged away the harbour bar.** Listed after the bar and long
enough to reach it, so it cut straight through the one feature that makes a harbour
interesting. The bar read -7.4 m where it was stated at -3.2. Order in the list is
composition; the channel now starts outside the bar, which is also the real story, because
a bar is the thing a dredger cannot keep clear.

**Both flanking banks sat on top of the channel they were meant to flank.** Given an
alongshore bearing, their thirteen-kilometre length ran *parallel to the beach* rather than
out to sea, so each of them covered the leading line. A bank flanking a channel runs with
it.

**And the channel itself was a no-op.** Stated at fifteen metres on a shelf already
twenty-five metres down eight kilometres out, a one-way carve could not fill - correctly -
so the feature contributed nothing anywhere along its length. Deepened to thirty and kept
short enough not to reach back over the bar, the approach now reads -30 m on the leading
line with the banks at -13 and -9.5 either side, which is a gut worth staying in.

## The measurement the phase exists for

An isolated pinnacle: a hundred and forty metres across, standing twenty-four metres proud
of a twenty-eight-metre bottom. Sixty-four chart grids were swept across it at each of
three resolutions, because a chart is centred on the ship and its phase relative to a fixed
rock is arbitrary and changes as she moves.

| chart spacing | grids that found it | shoalest sounding printed |
|---|---|---|
| 400 m | 1 in 64 | -21.6 m to -3.5 m |
| 200 m | 5 in 64 | -21.1 m to -3.5 m |
| 100 m | 21 in 64 | -20.9 m to -3.5 m |

Three things follow, and the third is the one that matters.

**It is missed, not smoothed.** Sixty-three grids in sixty-four print twenty metres of
water over a rock with three and a half on it.

**Sampling finer buys hit rate, not certainty.** At a hundred metres - a quarter the cell
area and four times the cost - most grids still miss it.

**Whether it appears depends on where the grid falls**, which makes it a correctness
problem rather than a fidelity one. The same rock reads -21.6 m or -3.5 m depending on
nothing but grid phase, so it would blink in and out as a ship moved. A hazard that blinks
is worse than one never drawn.

Hence the two channels. Soundings come from sampling terrain; isolated dangers come from
`marks_near`, the way real charts give them symbols rather than contours. The terrain still
carries the rock at full height for anything that asks canonically - which is everything
that can run aground on it.

## What gets marked is a measurement, not a judgement

The first rule was a size heuristic - mark anything under five hundred metres across - and
the render showed it was wrong. It marked the pinnacle and the drying rock and stopped.

It left **the moles**, which are two kilometres long and three hundred and forty metres
wide, so a four-hundred-metre grid prints six metres of water over a four-metre breakwater.
It left **the harbour bar**, over whose three-metre crest the same grid prints seven. Both
are large features; both are narrow in one dimension, which is all sampling cares about.

So the rule is stated as the thing it was standing in for: **a feature is marked exactly
when a chart would lie about it.** Measured over every feature in the region, as the
difference between the truth and the shoalest sounding a four-hundred-metre lattice would
print near it - positive meaning the chart claims more water than there is, which is the
only direction that drowns anybody:

| feature | truth | charted | the chart is |
|---|---|---|---|
| pinnacle | -3.5 m | -27.6 m | optimistic by 24.1 m |
| drying rock | +1.0 m | -14.1 m | optimistic by 15.1 m |
| north mole | +4.0 m | -6.6 m | optimistic by 10.6 m |
| harbour bar | -3.2 m | -7.3 m | optimistic by 4.1 m |
| north bank | -6.0 m | -6.1 m | right |
| approach channel | -30.0 m | -30.0 m | right |
| headland | +70.0 m | +69.8 m | right |
| harbour basin | -9.0 m | -6.0 m | pessimistic, which is safe |

The two sets separate cleanly - nothing marked is under-reported by less than four metres,
nothing unmarked by more than nothing - and the test asserts the equivalence rather than
the size, so a feature added later cannot quietly be both dangerous and unmarked.

## Testing steep against discontinuous

A pinnacle rising twenty-four metres in seventy is *supposed* to be abrupt, so the fixed
per-step threshold every earlier phase used would either pass a cliff or fail a rock.
Refinement is the test instead: sample a line across the feature at N points and at 2N, and
a continuous function roughly halves its worst neighbour-to-neighbour change while a step
function cannot. Applied to elevation and to authority alike.

## What is here

    harbour basin       somewhere to lie              -9 m, kept
    entrance            a gut through the shore       -9 m, 800 m wide
    two moles           arms a flat coast cannot make +4 m, either side of the entrance
    bar                 the reason for a tide table   -3.2 m across the mouth
    approach channel    a dredged lane                -30 m, starting outside the bar
    two banks           a gut worth staying in        -6 m and -7.5 m either side
    drying rock         a mark that is land at datum  +1.0 m, 90 m across
    pinnacle            the argument for marks        -3.5 m, 140 m across, in 24 of water
    headland            a landfall                    +70 m
    steep-to water      nowhere to anchor             -32 m within 4 km of the shore

28 new tests, 135 in total. The repository also gained a flake8 configuration, which it had
never had, and the sixty-odd pre-existing violations it immediately found are fixed.

## One thing measured and deliberately not fixed here

Placed features hold their stated shape exactly - the bar reads -3.2 m canonically, both
marks read their targets to a tenth. The **ordinary shelf around them** does not: detail
moves the bottom by twelve to fifteen metres in four to ten kilometres of water, because
M1.6 gives coastal ground thirty-five metres of amplitude and the shelf weight does not
pull it down far enough this close in.

That is within everything M1.6 asserted and measured, so it is not a regression. It is also
more roughness than a demonstration where a player navigates a twenty-metre channel wants.
Retuning it belongs to a phase that can re-measure M1.6's coastline-shift table afterwards,
not to this one - changing an amplitude here would quietly invalidate a published result.
Recorded rather than silently adjusted.
