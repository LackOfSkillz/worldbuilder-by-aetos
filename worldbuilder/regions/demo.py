"""
The demonstration coast: one harbour, and everything a chart needs to be worth reading.

**Constructed, not found.** The obvious move is to search the globe for a coast with a
good natural harbour and a bar across its mouth. That would need a global enumeration pass
- sample the whole planet, score every coastal cell, keep the best - which is exactly the
stored-world machinery Mark 1 exists to avoid. So the coast is chosen for its ordinary
virtues, which a few thousand samples can find, and the interesting parts are placed.

The anchor was measured rather than picked, and the first measurement was of the wrong
thing. A sweep for mid latitude, no tectonic contribution worth speaking of, a monotonic
shelf and *land rising inland* produced a coast whose land rose at four tenths of a metre a
kilometre - so a harbour cut two kilometres inland had nothing round it but tidal flat, and
the render showed a slightly deeper patch of an already-submerged plain.

The gradient had been measured at the sampled candidate, which sat twenty kilometres inland
of the actual shore. Measured *at the waterline*, across seventy-two monotonic passive
coasts, this world offers a median of four metres of land four kilometres inland and a best
of twenty-seven. Its margins are gentle everywhere, which is correct - a passive margin is
gentle - and it means a harbour here is a low-lying one with moles rather than a fjord.
This coast is the best combination available: land rising at a little over three metres a
kilometre, and a shelf that reaches sixty metres at twenty kilometres and keeps going.

What is here, and what each is for:

    harbour basin       somewhere to lie              a dredged depth, kept
    entrance            a gut through the shore       narrow, and the only way in
    two moles           arms, because the coast is low the harbour that a flat shore needs
    bar                 the reason for a tide table   shoal water across the entrance
    approach channel    a dredged lane                deep enough for a laden hull
    two banks           a gut worth staying in        shoal either side of the approach
    drying rock         a mark that is land at datum  visible at low water, awash at high
    pinnacle            the whole argument for marks  a hundred metres across, unmissable
                                                      by physics and invisible to sampling
    headland            a landfall                    high ground, seen from far off
    steep-to water      nowhere to anchor             deep water hard against the shore
    six islands         somewhere to go               a chain south-east of the approach

The islands are what make it a world rather than a harbour. A coast alone gives a player
one thing to do - leave, and come back - and every question a chart answers is a question
about the water directly ahead. A chain gives them a destination, a passage between two
destinations, and a reason to plot a course rather than steer one.

The pinnacle is the load-bearing one. Everything else could be found by a chart sampling
the terrain; that cannot, and a generator whose charts silently omit isolated dangers is
worse than one with no charts at all.
"""

import math
from dataclasses import dataclass

from ..bathymetry.features import CARVE, RAISE, SHAPE, Feature, Features
from ..bathymetry.substrate import MUD, ROCK, SAND
from ..geometry.sphere import EARTH_RADIUS_M, SpherePoint
from ..geometry.tangent import TangentFrame

#: The waterline, found by bisecting from the best-scoring candidate coast.
ANCHOR_LAT = -21.5841
ANCHOR_LON = 149.8703

#: Out to sea, degrees true.
#:
#: Taken from the steepest descent of the **finished ground**, not of continentality. Those
#: are different directions, because a shoreline is a contour of the finished field and the
#: shelf and the tectonics both tilt it. Using the continentality gradient put the
#: alongshore axis at an angle to the actual beach, so a line meant to run parallel to the
#: shore two kilometres inland went from fourteen metres of land at one end to fourteen of
#: water at the other, and a harbour cut on it had sea on one flank. Taken from the
#: structural field, the same line holds between 6.3 and 7.5 metres over sixteen
#: kilometres, which is what parallel to a shore means.
SEAWARD_DEG = 296.49

#: How much sea and shore the region covers, either way from the anchor. Well inside the
#: two hundred kilometres a tangent plane holds a chart to.
REACH_M = 60_000.0

# --- the archipelago --------------------------------------------------------

#: How far apart the islands stand, in metres.
#:
#: **Measured, not chosen.** The hand-authored world this coast replaces recorded the
#: exact mistake worth not repeating: its islands were spaced using a guessed four metres
#: a second when the craft actually made two point two, so every leg was nearly twice as
#: long as intended - and the test that checked the spacing passed, because it was
#: checking against the same guess.
#:
#: So the figure comes from a vessel. A working-sail rig on this coast makes 4.05 m/s at
#: best and 2.83 m/s averaged over every point of sail, which is the honest number for a
#: passage that has to beat as well as run. At that speed this is a leg of about eight
#: minutes: long enough to be a passage, short enough that a player sails the whole chain
#: in an evening.
ISLAND_LEG_M = 1_390.0

#: How high an island stands at its middle, and how far its ground reaches.
#:
#: The reach is a half-extent, and the ground tapers smoothly to nothing across it - so an
#: island gets its shoaling foreshore from the same arithmetic that raises it, rather than
#: from a second authored shape. Without that an island is a cliff: twenty metres of water
#: one step and dry sand the next, and a lead line shows nothing at all until she strikes.
ISLAND_Z = 12.0

#: Where the chain begins and which way it runs, as offshore and alongshore metres.
#:
#: South-east, starting just outside the bar. **Chosen by measuring five candidate
#: placements, not by eye**, against four things at once: how long the run out from the
#: entrance takes, whether the six come out as six separate islands, how much water lies
#: in the gaps between them, and how much lies on the way there.
#:
#: The first placement tried put the chain in open water seven and a half kilometres out,
#: which measured beautifully on every count except the one that matters - it was a
#: forty-five minute passage before a player reached anything. This is twenty minutes at
#: the average and fourteen with the wind fair, over twenty metres of water, into gaps
#: carrying eighteen.
#:
#: It passes close by the drying rock, and that is deliberate. A rock awash in the middle
#: of an island passage is the best hazard on this coast: it is on the chart, it is out of
#: sight at high water, and it sits exactly where somebody in a hurry between two harbours
#: would like to cut the corner. Checked rather than hoped for - the rock still stands
#: alone in eighteen metres, and no island has swallowed it.
ISLAND_START = (3_400.0, -2_600.0)
ISLAND_STEP = (600.0, -1_250.0)

#: Each island's name and how far its ground reaches from the middle.
#:
#: Named, because a destination without a name is a shape. Sized unevenly, because a chain
#: of six identical discs reads as a generated thing rather than as a place - and because
#: the smallest of them is a genuinely different navigational problem from the largest.
ISLANDS = (
    ("Gannet Isle", 430.0),
    ("Kettle Rock", 250.0),
    ("Longhope", 520.0),
    ("The Brothers", 300.0),
    ("Sandhaven", 470.0),
    ("Outer Skerry", 210.0),
)

#: The world these coordinates were measured on. Placing them on another seed puts a
#: harbour in whatever happens to be there, which may be the middle of an ocean.
WORLD_SEED = 20260831


class Coast:
    """
    Somewhere to stand, so features can be placed the way a chart describes them.

    Notes:
        Nobody sites a harbour bar at a latitude and longitude. They site it a mile and a
        half outside the entrance, and that is what this converts: distances offshore and
        along the shore into points on a sphere.

    """

    def __init__(
        self, lat=ANCHOR_LAT, lon=ANCHOR_LON, seaward_deg=SEAWARD_DEG, radius_m=EARTH_RADIUS_M
    ):
        self.origin = SpherePoint.from_latlon(lat, lon)
        self.frame = TangentFrame.at(self.origin, radius_m)
        self.seaward_deg = seaward_deg
        self.alongshore_deg = (seaward_deg - 90.0) % 360.0
        radians = math.radians(seaward_deg)
        self._out_e, self._out_n = math.sin(radians), math.cos(radians)

    def at(self, offshore_m, along_m=0.0):
        """
        A point so far out to sea and so far along the shore.

        Args:
            offshore_m (float): Seaward is positive; negative goes inland.
            along_m (float): Positive runs to the left as you look out to sea.

        Returns:
            point (SpherePoint): Where that is.

        """
        east = self._out_e * offshore_m - self._out_n * along_m
        north = self._out_n * offshore_m + self._out_e * along_m
        return self.frame.local_to_sphere(east, north)


@dataclass(frozen=True)
class Region:
    """
    A named piece of a world, and the things somebody put in it.

    Attributes:
        name (str): What to call it.
        coast (Coast): Its anchor and orientation.
        reach_m (float): How far from the anchor it is meant to be used.
        features (Features): What was placed.

    """

    name: str
    coast: object
    reach_m: float
    features: object

    @property
    def origin(self):
        return self.coast.origin

    def covers(self, point):
        """Whether a point is inside the region, for diagnostics and for tests."""
        return self.origin.distance_to(point) <= self.reach_m


def demo_region(radius_m=EARTH_RADIUS_M):
    """
    The demonstration coast, assembled.

    Args:
        radius_m (float, optional): The planet.

    Returns:
        region (Region): Anchor, orientation and features.

    Notes:
        **Order is composition.** The basin and entrance are cut first, then the bar is
        raised across the entrance so it sits on the carved bottom - which is where a bar
        is. Listed the other way round the channel would cut straight through it.

    """
    coast = Coast(radius_m=radius_m)
    seaward, alongshore = coast.seaward_deg, coast.alongshore_deg

    placed = [
        # Cut the harbour first, so everything after it can argue with a real basin.
        Feature(
            kind="harbour basin",
            at=coast.at(-2_000.0, 0.0),
            target_m=-9.0,
            length_m=3_000.0,
            width_m=1_400.0,
            bearing_deg=seaward,
            compose=SHAPE,
            substrate=MUD,
        ),
        Feature(
            kind="entrance",
            at=coast.at(1_200.0, 0.0),
            target_m=-9.0,
            length_m=3_600.0,
            width_m=400.0,
            bearing_deg=seaward,
            compose=CARVE,
            substrate=SAND,
        ),
        # Arms, because this coast rises three metres a kilometre and a harbour on a
        # shore that flat is a pair of moles or it is nothing. Raised after the entrance
        # is cut, so they stand on the channel edge rather than being dredged out of it.
        #
        # Three hundred and forty metres across and two kilometres long, so a chart
        # sampling every four hundred prints six metres of water over a four-metre
        # wall. Marked - which is why the rule is a feature's *narrowest* dimension
        # and not its size.
        Feature(
            kind="north mole",
            at=coast.at(1_600.0, 620.0),
            target_m=4.0,
            length_m=2_000.0,
            width_m=170.0,
            bearing_deg=seaward,
            compose=RAISE,
            marked=True,
            substrate=ROCK,
        ),
        Feature(
            kind="south mole",
            at=coast.at(1_600.0, -620.0),
            target_m=4.0,
            length_m=2_000.0,
            width_m=170.0,
            bearing_deg=seaward,
            compose=RAISE,
            marked=True,
            substrate=ROCK,
        ),
        # And then shoal the bottom back up across the mouth, which is what a bar is.
        # Marked, because a four-hundred-metre grid prints seven metres of water over
        # its three-metre crest - a bar is charted by its controlling depth, not by
        # whatever sounding happened to land near it.
        Feature(
            kind="harbour bar",
            at=coast.at(3_400.0, 0.0),
            target_m=-3.2,
            length_m=1_200.0,
            width_m=550.0,
            bearing_deg=alongshore,
            compose=RAISE,
            marked=True,
            substrate=SAND,
        ),
        # Starting outside the bar, because a bar is the thing a dredger cannot keep
        # clear. Reaching over it, the channel cut straight through it and the harbour
        # lost the only reason it needs a tide table.
        #
        # Thirty metres, not fifteen. At fifteen the channel was a no-op: this shelf is
        # already twenty-five metres down eight kilometres out, and a one-way carve cannot
        # fill - correctly - so the feature contributed nothing anywhere along its length.
        # It is also kept short enough not to reach back over the bar.
        Feature(
            kind="approach channel",
            at=coast.at(8_000.0, 0.0),
            target_m=-30.0,
            length_m=4_000.0,
            width_m=900.0,
            bearing_deg=seaward,
            compose=CARVE,
            substrate=MUD,
        ),
        # Running seaward alongside the channel, not across it. Given the alongshore
        # bearing these were thirteen kilometres long *parallel to the beach*, which put
        # both of them on top of the channel they were supposed to flank.
        Feature(
            kind="north bank",
            at=coast.at(8_000.0, 3_000.0),
            target_m=-6.0,
            length_m=4_500.0,
            width_m=1_600.0,
            bearing_deg=seaward,
            compose=RAISE,
            substrate=SAND,
        ),
        Feature(
            kind="south bank",
            at=coast.at(8_000.0, -3_000.0),
            target_m=-7.5,
            length_m=4_500.0,
            width_m=1_600.0,
            bearing_deg=seaward,
            compose=RAISE,
            substrate=SAND,
        ),
        Feature(
            kind="drying rock",
            at=coast.at(4_000.0, -5_000.0),
            target_m=1.0,
            length_m=45.0,
            width_m=45.0,
            compose=RAISE,
            marked=True,
            substrate=ROCK,
        ),
        # A hundred and forty metres across, in twenty-five of water, and placed clear of
        # the banks so that what it sits on is ordinary seabed rather than another
        # feature. No chart that samples terrain will ever see it, and every hull that
        # touches it will.
        Feature(
            kind="pinnacle",
            at=coast.at(8_000.0, 6_500.0),
            target_m=-3.5,
            length_m=70.0,
            width_m=70.0,
            compose=RAISE,
            marked=True,
            substrate=ROCK,
        ),
        Feature(
            kind="headland",
            at=coast.at(-800.0, 14_000.0),
            target_m=70.0,
            length_m=4_000.0,
            width_m=3_000.0,
            bearing_deg=seaward,
            compose=RAISE,
            substrate=ROCK,
        ),
        Feature(
            kind="steep-to water",
            at=coast.at(3_500.0, 15_000.0),
            target_m=-32.0,
            length_m=5_000.0,
            width_m=3_000.0,
            bearing_deg=alongshore,
            compose=CARVE,
            substrate=ROCK,
        ),
    ]
    placed.extend(_archipelago(coast))
    return Region("demonstration coast", coast, REACH_M, Features(placed, radius_m))


def _archipelago(coast):
    """
    The island chain, strung south-east of the approach.

    Args:
        coast (Coast): Somewhere to place them from.

    Returns:
        islands (list): One `Feature` each, in order out from the land.

    Notes:
        Round, so they have no bearing worth giving: `length_m` and `width_m` are the same
        reach, and the bump that raises them falls to nothing in every direction alike.
        That taper is the foreshore - the ground comes up out of the water rather than
        standing out of it, so a lead line warns before a hull finds anything.

        **Marked, every one.** Not because an island is subtle, but because a chart drawn
        at a wide scale samples the seabed every few kilometres and one eight hundred
        metres across falls between the soundings - so it would appear as she zoomed in
        and vanish as she zoomed out. That is precisely the case `marked` exists for, and
        it is the same rule that put a symbol on the moles.

        The outermost is rock and the rest are sand, which is what a chain like this
        usually is: the far end of it is what the sea has not yet finished with.

    """
    offshore, along = ISLAND_START
    ahead, aside = ISLAND_STEP
    islands = []
    for index, (name, reach_m) in enumerate(ISLANDS):
        islands.append(
            Feature(
                kind=name,
                at=coast.at(offshore + ahead * index, along + aside * index),
                target_m=ISLAND_Z,
                length_m=reach_m,
                width_m=reach_m,
                compose=RAISE,
                marked=True,
                substrate=ROCK if index == len(ISLANDS) - 1 else SAND,
            )
        )
    return islands
