# Letting a game change the world to fit its areas

The owner, looking at a 177-room river town placed on ground with no river in it:

> "The landing is actually a river town with a deep river that ships can sail up to. How do
> we make this happen in worldbuilder? We need a way for game devs to actually alter the
> world to fit their areas."

**The mechanism already exists, it is the one the demonstration harbour is built from, and
it does not cost the property this project refuses to trade.**

## Why this is not a threat to point-evaluability

The engine's whole architecture rests on `f(SpherePoint) -> value`: no state, no
neighbours, no grid, no traversal order. The obvious way to carve a river - flood-fill a
channel across a heightfield - would destroy that, because it needs a raster and an order.

`bathymetry/features.py` solves it a different way and has since Mark 1. A feature is a
**shape stated at a place**, and asking whether a point is inside it is a pure function of
that point. The file's own header:

> "a short list of named features, stamped at chosen places, because a test region needs a
> channel *here* and a bar across *that* harbour mouth, and no amount of noise will oblige."

So an authored change is not an exception to the design. It is the second half of it.

## What a feature is

    kind         what it is called, for diagnostics and chart symbols
    at           its middle, a SpherePoint
    target_m     the elevation it wants the ground to be at its middle
    length_m     how far it reaches along its bearing
    width_m      how far it reaches either side
    bearing_deg  which way it runs
    compose      RAISE, CARVE or SHAPE
    marked       whether a chart carries a symbol regardless of the soundings

**`CARVE` only ever deepens and `RAISE` only ever shallows**, and that is not a hedge: a
raise whose target is already below the ground contributes nothing, and at the moment the
two are equal it contributes nothing either - so the switch happens exactly where the effect
is zero and the ground stays continuous.

**Order is meaning.** A bar listed after the channel it crosses sits on the carved bottom;
listed before, the channel cuts through it. An authored world therefore has an ordered
feature list, not a set.

## A river, measured

A river is a chain of `CARVE` segments along a polyline - the same primitive as the demo
harbour's approach channel, repeated.

**It reaches its target exactly.** One segment over ground at 33.70 m, target -8 m, length
300 m, width 90 m:

    shaped -8.00 m, weight 1.000, authority 1.00

A straight chain of eight segments reads -8.00 m on the centreline at along-reach factors
of 0.5, 1, 2 and 3 - overlap neither deepens nor cancels it.

**A first measurement said otherwise and was wrong.** A test path that bent while its
segments were shortened to 62% of their span put the sample point off every centreline, and
reported a 7.85 m cut against a 37 m request. That is a lesson about laying segments along
a curve, not a limit of the system: **a bending river needs its segments to overlap, or the
channel has gaps at the bends.**

## What it costs: nothing measurable

Features are a linear scan per elevation query, so the worry is a river of hundreds of
segments. Measured on the owner's world, 400 samples per figure, Python oracle:

    features    us/sample
       0          184.7
       5          199.1
      22          172.1
      60          175.5
     150          169.0

**There is no trend.** The spread is measurement noise on a per-sample cost dominated by
the terrain evaluation itself; the feature scan does not appear above it at 150 segments.
A river with a segment every hundred metres can run twenty kilometres before anybody would
need to look at this again, and the answer if they do is a bounding-box reject before the
weight is computed - which preserves point-evaluability exactly.

## What is missing, and it is not the hard part

1. **The worldfile does not carry features.** It carries a planet, areas, rooms and exits.
   An authored river has to travel with the world or the game and the studio will draw
   different ground. This is the schema change the contrib decision already warned would be
   the expensive part - and it is the same promise, one field larger.

2. **The authoring surface exists and is not wired to this.** The route tool added for
   drawing paths is exactly the right shape: click a line, and each leg becomes a segment.
   What a river needs beyond a path is **width and depth**, either per route or per node -
   a river that narrows upstream is two numbers per node, not a new tool.

3. ~~**The WASM exports take a harbour flag, not a feature list.**~~ **WRONG, and checked
   after the claim was written rather than before.** `Engine.newWorld` has taken a
   `features` array all along - `WB_FEATURE_STRIDE = 8`, `COMPOSE = {raise, carve, shape}`,
   packed into a `Float64Array` and handed to `wb_world_new_gully`. The viewer only ever
   passed `HARBOUR`, which is a different thing from the boundary not existing. **It is
   wired now**: `?river=<route>` reads a saved route, turns each leg into a carve segment,
   and hands them to the constructor before the world is built - so the tiles, the water
   solve, the biome colours and every elevation query see one ground with the river in it.

4. ~~**Nothing checks that an authored change is navigable.**~~ **Built, and it immediately
   failed the first river.** A game saying "ships sail up to
   this town" is asserting a depth along a path, and that assertion can be tested exactly
   the way the port mapping tests a harbour: walk the channel and confirm the depth holds.
   **An unchecked river is a river that is eight metres deep except in the one place a hull
   would find.**

## The shape of the answer to give a game developer

    draw the line          the route tool, already built
    say what it is         a river 90 m wide, 8 m deep, narrowing to 40 m and 3 m upstream
    it becomes features    one CARVE segment per leg, overlapping at the bends
    they travel in the worldfile   so the game and the studio agree
    and they are checked   the channel is walked and its depth confirmed

**None of that requires the generator to become editable.** The planet stays a pure function
of its seed and parameters; the authored world is a list of stated shapes applied on top. A
world is then reproducible as "this seed, these parameters, these features" - which is still
a few hundred bytes, and still bit-for-bit.

## The honest limit

**Features are stamps, not hydrology.** A carved river does not flow, does not have a
catchment, and will happily run uphill if somebody draws it that way. The engine will not
object, and neither will anything downstream of it.

That is the right trade for a game - a river town needs a navigable channel where the map
says one is, not a simulation of erosion. But it should be **stated rather than discovered**,
and a check that a drawn river runs downhill is cheap and worth having next to the depth
check.


---

## What building it actually taught, 2026-09-07

**The first river looked perfect and was impassable.** It had banks, a bed and a clean
cross-section at the town - and the sounding walk found **34 of 90 soundings shoal**, with
the bottom rising above the waterline at every node.

The cause is the shape of a feature's weight: one at its own middle, nothing at its stated
reach. So a chain of segments carves **deepest at the midpoints and shallowest at the nodes
between them**, and a river is a row of dredged pools with bars across it. Nothing about the
cross-section shows this, because a cross-section is taken at a midpoint.

Sounded along a 3.51 km channel of 25 segments at 2.5 m draught, 90 soundings:

    overlap   min depth   shoaling soundings
      1.35     -21.30 m        65
      2.0       -9.34 m        19
      3.0       -0.53 m         1
      4.5       +5.99 m         0   <- navigable
      6.0       +8.26 m         0

**`OVERLAP = 4.5`**, measured. Below it a hull finds the gaps; above it nothing improves
that a deeper target would not do better, and every extra metre of reach widens the
disturbance either side of the river.

Finished, through the town, banks to bed and back:

    4.89  5.11  5.29  5.42  -3.78  -6.38  -3.47  5.55  5.42  5.19  4.84

    90 soundings, min depth 4.91 m, max 9.5 m, 0 shoalings, navigable

**The check is the part worth keeping.** The river that failed was indistinguishable from
the river that worked by every means except walking it. A game asserting that ships reach a
town is asserting a depth along a path, and only a walk along that path tests the assertion.
