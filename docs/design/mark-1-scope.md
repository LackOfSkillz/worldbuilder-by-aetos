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

---

# M1.8 result: what the bottom is made of

Maritime asks two things of a world: how deep the water is, and what is under it. M1.5
through M1.7 answered the first. This answers the second.

## A category is the wrong shape for the answer, and the right shape for the question

Everything in this engine is continuous because hard decisions on continuous quantities
make cliffs, and "sand" is about as hard a decision as exists. Three named bottom types
with boundaries between them would put a discontinuity in anchor holding, in grounding
damage, and in anything else that read the name.

So the field is a **composition** - three fractions summing to one, each varying smoothly -
and the single-word answer is whichever fraction is largest. Nothing continuous is ever
computed from the word.

That is not a dodge. Composition is what the physics actually wants. Holding ground is a
matter of how much mud is in it; a bottom three-quarters sand over rock behaves like
neither; and `holding()` is expressed from the fractions for exactly that reason, because a
bottom that is half rock is genuinely half as good and a word cannot say that.

There are three fractions rather than eight on purpose. Gravel, shell, weed, coral and clay
are all real and all wanted eventually, and every one of them is a *fourth fraction* rather
than a change of shape - which is the point of getting the shape right first.

## Derived from three things, none stored

    slope       fines do not stay on a steep bottom, so steep means rock
    depth       above wave base the sea winnows the fines out; below, they settle
    tectonics   ground the plates lifted is rock whatever its slope

And overridden, smoothly and by weight, by anything placed. The dredged basin is mud
because that is what settles in still water behind a mole; the moles are rock because
somebody built them out of stone; the bar is sand because a bar is what the ebb dropped.

## The bug this phase produces

**A finite difference cannot see anything narrower than its own baseline.** At six hundred
metres, the slope probe straddled the pinnacle. The bottom a hundred and thirty metres from
a rock standing twenty metres proud read *perfectly flat*, because both probes missed it;
the bottom three hundred metres away read steep, because one probe landed on it.

The render showed it immediately: rock in **rings**, with sand at the centre of every
hazard. It is the substrate equivalent of the moving-grid problem, and it earned the test
with teeth - walking away from a rock the bottom may stop being rocky, but it may not stop
and then start again.

The baseline is sixty metres now, smaller than the narrowest thing anybody placed. There is
no opposing constraint, because the probe reads `structural_m`, which carries no detail:
measured across the planet, the structural slope distribution is identical at three hundred,
six hundred and two thousand metres. Structure is smooth at every scale. Only features and
detail are not, and features are what this needs to see.

## A calibration that was wrong, and how it showed

At a four-hundred-metre threshold on tectonic contribution the whole demonstration coast
came out a third rock. A passive margin carries about a hundred and fifty metres of broad
tectonic rise, and a broad rise is not a rock face. Twelve hundred metres is the scale of
real tectonic structure - a trench wall, a ridge crest - and the slope term was already
there to catch steepness. A trench of eleven hundred and twenty-six metres now reads
ninety-nine per cent rock; the shelf reads under six.

## What a placed feature does to its own flanks

A bank declaring sand has flanks of its own making, and those flanks are the steepest ground
for miles. There was a real question about whether the slope term would win there and ring
every placed feature with rock it never asked for.

Measured across the north bank, the rock share peaks at 0.29 on the flank and sand stays
dominant over the whole support. So the render shows the bank rimmed with a coarse *tint*
rather than a rock bottom - which is also the right answer, because a scoured bank edge is
coarser than its crest, and with three fractions the coarse end is what rock means. Asserted
in the tests, so a later change to either term cannot quietly reverse it.

## What it costs, and the shape of the bargain

    a bottom      661 us a sample
    a sounding    150 us a sample     4.4x

Four probes and a frame. Affordable only because bottom type is asked far less often than
depth - a ship sounds continuously and anchors once - and because the same intermediates
can be handed in when a caller already has them, the way the shelf takes them.

## One thing worth knowing about Mark 1

The steepest **structural** ground anywhere on this planet is one and a half per cent, at
any baseline. Mark 1 makes no cliffs: relief is tens of metres over wavelengths of hundreds
to thousands. So on a Mark 1 world the slope term fires almost entirely on placed features,
where slopes reach twenty-seven per cent, and hardly at all on ordinary ground.

That is not a defect to fix here. It is a measured statement about what this prototype does
and does not contain, and it is the sort of thing that would otherwise be discovered by a
Mark 5 seabed suddenly turning to rock everywhere.

19 new tests, 154 in total.

---

# M1.9 result: the seam, and a hull holed on a generated rock

The generated planet, presented as a map provider the maritime contrib can sail on.

## The dependency runs one way

Maritime never imports worldbuilder and never will. A contrib that needed a planet
generator installed to work would be a contrib nobody could adopt. So the adapter lives on
the generator's side, in the one file that knows both, and fits itself to maritime.

**Nothing in maritime changed to accept it.** The interface is the one its base provider
already declares - `terrain_z_at`, `bottom_type_at`, `hazards_touching` - and maritime tests
none of its providers with `isinstance`, so a duck answers. Swapping a hand-written seabed
for a planet is one settings line.

The generator keeps no dependency either. `WorldbuilderTerrain` carries all the behaviour
and imports nothing from Evennia, which is what lets it be tested without an Evennia install;
`maritime_provider` is the single function that needs the contrib present, and it imports
inside the call. Asserted in the tests by reading the module's own source for a top-level
Evennia import.

## A region is a tangent plane, which is what a chart is

Maritime works in flat metres east and north within a named region; the planet works in unit
vectors. One `TangentFrame` per region converts between them. The two-hundred-kilometre cap
M1.1 measured is a cap on how large a region may be, and the demonstration coast is sixty:
measured round the whole compass at its own reach, the worst projection error is under a
metre.

## Depth is truth; error is the chart's job

`terrain_z_at` answers canonically. Maritime already models a chart's ignorance separately -
`charted_terrain_z_at` adds a deterministic sounding error on top of whatever the world says
- so a provider that answered approximately would put a second, unmodelled error underneath
the first, and a ship taking a fix would be wrong in a way nothing had accounted for.

## A circle is a bad fit for a breakwater, so a breakwater becomes several

Maritime's hazard is a circle with a radius and a shallowest point. That is right for a
pinnacle and wrong for a mole, and a single circle round a mole would either miss most of it
or declare two square kilometres of harbour approach foul. Long features become overlapping
circles of their own width, laid along their length, and a test steers a hull across every
gap between neighbours to prove nothing passes through.

**And a circle is only kept where a chart would lie about it** - the same rule that decided
the feature was marked in the first place, applied one level down. A feature is marked
because a chart lies about it *somewhere*; a circle is a hazard where a chart lies about it
*there*. Without that rule the tapering ends of a mole, which fade into ordinary seabed
seventeen metres down, came back as dangers: fifty-seven circles, of which thirty were
open water. Twenty-seven now, and every one of them is ground a chart gets wrong.

Each circle takes its elevation from the terrain at its own centre rather than from the
feature's stated target, so a hazard and the water under a hull cannot disagree.

## Live, in a real Evennia environment

The generator's own suite cannot import maritime - its package pulls in Twisted - so the
tests here run against a stand-in with the three attributes the adapter reads, and the seam
was then verified where it actually runs. Built as a provider in the maritime testbed and
handed to maritime's own grounding code:

| what | draught | outcome |
|---|---|---|
| over the pinnacle, 3.5 m on it | 2.5 m | clears |
| | 3.4 m | clears |
| | 4.5 m | **holed on rock**, 1.00 m short |
| | 6.0 m | **holed on rock**, 2.50 m short |
| the same track, 900 m off | any | clears |
| across the bar, 3.2 m on it | 2.0 m | crosses |
| | 3.5 m | **aground on sand**, 0.30 m short |
| | 5.0 m | **aground on sand**, 1.80 m short |

Three phases arriving at once. The rock is there because M1.7 placed it and the marks layer
carried it past a chart that cannot see it; the difference between *holed* and *aground* is
M1.8's substrate reaching maritime's damage model; and none of it required a line of change
in the contrib.

One thing the run caught, and it was the probe rather than the code: maritime's
`GroundingResult` is falsy when it represents a failure, so a first pass testing the result
for truth reported that a six-metre draught sailed over a rock with three and a half metres
of water on it. Which is the argument for live verification in one sentence - the unit tests
were green throughout.

24 new tests, 178 in total.

---

# M1.10 result: the places where flat thinking breaks

The field-level versions of these already existed - M1.6 and M1.5 both cross the
antimeridian and stand on both poles. What M1.10 adds is the **assembled** case: a region,
its tangent frame, the features stamped in it and the provider maritime talks to, all at
the coordinates where the geometry is hostile.

That distinction is the phase. A continuous elevation field is necessary and nowhere near
sufficient, because everything a ship actually uses goes through a projection - and a
projection is exactly the thing that has a pole and a seam. A world can be perfectly smooth
and still put a vessel on the wrong side of the planet when she sails north past
eighty-nine degrees.

Nine places, three questions each:

    the equator, the antimeridian from both sides, the arctic, the antarctic,
    a tenth of a degree short of each pole, and each pole exactly

    does the ground stay continuous       the field, through a frame
    does the frame stay a frame           basis, scale, and round trip
    does a ship get where she is going    the provider, over a track

Nothing failed. Which is the expected result, because M1.1 built the geometry on unit
vectors precisely so that these would not be special cases - but "we designed it not to
break" and "we sailed a hull over the north pole and it did not break" are different
statements, and only one of them is a test.

## Two failures, both in the tests

**A rock placed at the north pole did nothing.** The pole of this world is thirty-one
metres of dry land, and a `RAISE` to three metres below datum correctly declines to dig.
The composition rule working exactly as designed, and the test asking the wrong question.

**"She did not get past the pole."** From 89.4 degrees the pole is sixty-seven kilometres
north; a hundred and twenty kilometres puts her fifty-three past it, at 89.52 degrees -
*higher* than she started, and heading south on the opposite meridian. The test asserted
that her latitude would drop, which only happens beyond a hundred and thirty-three
kilometres. The signature that actually distinguishes going over a pole from turning back
at it is the meridian flip, and that is what it checks now.

Both are the same mistake in different clothes: asserting a proxy for the property rather
than the property. It is the fourth time on this project.

16 new tests, 194 in total.

---

# M1.11 result: the performance gate, and the honest position on it

Maritime does not read a map. It asks how high the ground is at a point, and a chart redraw
asks 9,216 times. That number is the whole reason this generator is a function of position
rather than a stored heightmap, so it is the number the generator has to answer to.

## Where it stands

Measured end to end in a real Evennia environment, through maritime's own
`client.cartography.sample`, against the same chart on the same machine:

| grid | soundings | a hand-written ramp | the generated planet |
|---|---|---|---|
| 96 x 96 | 9,216 | 18.0 ms | 725.4 ms |
| 48 x 48 | 2,304 | 4.7 ms | 163.9 ms |
| 32 x 32 | 1,024 | 2.1 ms | 70.5 ms |

A canonical sample costs about eighty microseconds; a hand-written seabed costs about two.

## Half of it was ordinary care, and cost nothing

Three changes, none of which altered a single value:

| what | why it was slow |
|---|---|
| noise caches whole lattice cells | 2.9 million method calls a redraw, where the *call* cost twice the dictionary lookup inside it |
| the projection is unrolled into components | called six times a sample, building seven throwaway vectors each time - forty-two objects for one answer |
| the plate sweep is unrolled, both types slotted | ninety-nine `Vec3.dot` calls a sample: three multiplies wrapped in a Python method call |

**129.5 to 86.6 microseconds a sample, 1.50x, bit-identical.** Proved rather than asserted:
every elevation, structural height, band-limited height and bottom composition the world
produces at fifteen hundred scattered points and a hundred and sixty coastal ones was hashed
before and after, and the digest is unchanged. The test suite went from 37 seconds to 23 on
the way past.

## The rest is not a Python problem

A terrain sample evaluates **thirty-nine octaves of noise** plus the plate geometry. Even
perfectly written that is tens of microseconds in this language, so the remaining gap cannot
be optimised away - it has to be *chosen* away, and every choice costs something that is not
performance:

**Coarser charts.** 32 x 32 puts a redraw at 70 ms, and a chart at six hundred metres a
sounding is a real chart. Costs detail; costs no *safety* at all, because the marks layer
already carries everything sampling misses - which is the argument M1.7 measured and the
reason a coarser chart is not a more dangerous one.

**A coarse lattice for the slow fields.** Continentality's finest structure is six hundred
and forty kilometres, and the gradient of it is a third of the whole cost. Sampling it on a
fixed world-anchored lattice and interpolating with smoothstep - the same trick the noise
itself uses, and C1 for the same reason - would be an order of magnitude. Costs a small
change to every elevation in the world, which wants measuring before it is chosen.

**An analytic gradient.** The four probes behind the shelf are sixteen of the thirty-nine
noise evaluations, plus a tangent frame and four projections. The derivative of a
smoothstep-trilinear fbm has a closed form costing about one evaluation. It is *more*
accurate than the finite difference it replaces, and it still moves every coastline
slightly.

**Caching in the provider.** An anchored ship redraws the same grid. An exact cache keyed on
the position pair makes repeats free and changes nothing whatsoever - but does nothing for
the first redraw or for a ship under way.

The first is a maritime decision, the second and third change the world's values, and the
fourth is free but partial. None of them is a decision this phase should take on its own,
which is why the phase ends with the measurement rather than with a fix.

## One thing the table says that was not expected

`resolution_m` barely helps. At four hundred metres a sample costs 81.3 microseconds against
80.5 canonical - *slower*, because the fade arithmetic costs more than the two octaves it
drops. Only at ten kilometres does it win, and only to 74.6.

That is not a defect in band-limiting; it is a statement about where the cost is. Detail is
fifteen microseconds of eighty. The expensive part of this world is its **structure**, and
structure is the thing that must not thin out with zoom.

## The gate itself

A regression gate, not a budget assertion. A hard assert on microseconds would fail on a
loaded machine and teach everybody to ignore it, so what is guarded is that the shape of the
cost stays right - each layer dearer than the one under it, a coarse chart cheaper than a
canonical one, a bottom still about five soundings - under a ceiling generous enough to
catch a doubling and nothing tighter.

Four of the eight tests exist only to keep the optimisations free: the lattice cache against
the lattice function, the unrolled projection against its own inverse, the unrolled plate
sweep against the vector algebra it replaced, and a check that a margin normal is still a
vector rather than one of the component triples that now live beside it.

8 new tests, 202 in total.
