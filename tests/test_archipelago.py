"""
Tests for the island chain, and for the four things that make it worth sailing to.

A chain of islands is easy to get wrong in ways that all look right in the source. The
numbers can be perfectly reasonable and the result be one long shoal, or six islands nobody
can reach, or six islands with no water between them.

So none of these tests reads the constants back. Every one of them asks the finished ground:

    separate       six landmasses, not one bar with lumps on it
    reachable      a passage from the harbour a player will actually make
    navigable      water in the gaps, and water on the way out
    shelving       ground that comes up out of the sea rather than standing out of it

The last one is the quiet one. An island with no foreshore is a cliff - twenty metres of
water one step and dry sand the next - and a lead line gives no warning at all before she
strikes. It is invisible in a picture of the coast and obvious to anybody who sails it.
"""

import math
import unittest

from worldbuilder.regions.demo import (
    ISLAND_LEG_M,
    ISLAND_START,
    ISLAND_STEP,
    ISLAND_Z,
    ISLANDS,
    WORLD_SEED,
    demo_region,
)
from worldbuilder.terrain.surface import Surface

#: Where a hull *actually* departs from, offshore and alongshore.
#:
#: Not the entrance. The first version of this measured from the gut between the moles,
#: and found land a quarter of the way out to the first island - which was the south mole,
#: because a straight line from inside a harbour crosses the arm that makes it a harbour.
#: No vessel sails that line.
#:
#: This is the fairway: clear of both moles, which reach three and a half kilometres out,
#: and clear of the bar. It is where a passage to the islands genuinely begins.
FAIRWAY = (4_200.0, 0.0)

#: What a working-sail rig makes on this coast, averaged over every point of sail. Measured
#: from the vessel rather than assumed - see `ISLAND_LEG_M`.
WORKING_SAIL_MS = 2.83

#: How long a passage may take before it stops being a demonstration. A world built to be
#: tested has to be reachable inside the patience of somebody testing it.
LONGEST_APPROACH_MIN = 30.0

#: The least water a gap may hold and still be a gap. A laden hull on this coast draws
#: three or four; anything under this is a passage nobody sane takes.
LEAST_NAVIGABLE_M = 8.0


class ArchipelagoTestCase(unittest.TestCase):
    """Shared ground: one region, one surface, and a way to ask what the ground does."""

    @classmethod
    def setUpClass(cls):
        cls.region = demo_region()
        cls.surface = Surface(WORLD_SEED, features=cls.region.features)
        cls.coast = cls.region.coast

    def z(self, offshore, alongshore):
        """
        Returns:
            elevation (float): The finished ground, in metres relative to datum.

        """
        return self.surface.elevation_m(self.coast.at(offshore, alongshore))

    def middle(self, index):
        """
        Returns:
            place (tuple): The offshore and alongshore metres of one island's centre.

        """
        return (
            ISLAND_START[0] + ISLAND_STEP[0] * index,
            ISLAND_START[1] + ISLAND_STEP[1] * index,
        )


class TestTheyAreSixIslands(ArchipelagoTestCase):
    """
    Not one shoal with six lumps on it, which is what happens when they are placed too
    close together or raised out of water too shallow to separate them.
    """

    def test_every_one_of_them_stands_out_of_the_water(self):
        for index, (name, _) in enumerate(ISLANDS):
            offshore, alongshore = self.middle(index)
            self.assertGreater(self.z(offshore, alongshore), 0.0, f"{name} is submerged")

    def test_and_stands_as_high_as_it_was_asked_to(self):
        """
        A RAISE is one-way: asked to lift ground that is already higher, it does nothing.
        An island placed on a headland would silently be no island at all.

        """
        for index, (name, _) in enumerate(ISLANDS):
            offshore, alongshore = self.middle(index)
            self.assertAlmostEqual(self.z(offshore, alongshore), ISLAND_Z, delta=1.0, msg=name)

    def test_they_are_separate_landmasses(self):
        """
        Asked of the ground rather than of the spacing. Two islands whose feet touch are
        one island, however far apart their centres were authored.

        """
        step = 40.0
        west = ISLAND_START[0] - 1_200.0
        east = ISLAND_START[0] + ISLAND_STEP[0] * (len(ISLANDS) - 1) + 1_200.0
        far = ISLAND_START[1] + ISLAND_STEP[1] * (len(ISLANDS) - 1) - 1_200.0
        near = ISLAND_START[1] + 1_200.0

        land = set()
        offshore = west
        while offshore <= east:
            alongshore = far
            while alongshore <= near:
                if self.z(offshore, alongshore) > 0.0:
                    land.add((round(offshore / step), round(alongshore / step)))
                alongshore += step
            offshore += step

        seen, found = set(), 0
        for cell in land:
            if cell in seen:
                continue
            found += 1
            stack = [cell]
            seen.add(cell)
            while stack:
                x, y = stack.pop()
                for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, -1), (1, -1), (-1, 1)):
                    neighbour = (x + dx, y + dy)
                    if neighbour in land and neighbour not in seen:
                        seen.add(neighbour)
                        stack.append(neighbour)
        self.assertEqual(found, len(ISLANDS), f"expected {len(ISLANDS)} islands, found {found}")


class TestTheyCanBeSailedTo(ArchipelagoTestCase):
    """
    The test the first placement failed. Six islands in open water, correctly separated,
    correctly shelving, and forty-five minutes from anywhere - which is a fine coast and a
    useless demonstration.
    """

    def test_the_first_is_a_passage_rather_than_an_expedition(self):
        offshore, alongshore = self.middle(0)
        away = math.hypot(offshore - FAIRWAY[0], alongshore - FAIRWAY[1])
        minutes = away / WORKING_SAIL_MS / 60.0
        self.assertLess(minutes, LONGEST_APPROACH_MIN, f"{minutes:.0f} minutes from the entrance")

    def test_there_is_water_the_whole_way_out_to_it(self):
        """
        A destination behind a drying bank is not a destination.

        The passage runs to an anchorage off the island, not to the middle of it. Measured
        all the way to the centre this failed at ninety-two per cent, in five metres - and
        five metres two hundred metres off a beach is not a fault, it is the beach. A test
        that demands deep water at an island's centre is asking the foreshore not to exist.

        """
        offshore, alongshore = self.middle(0)
        reach = ISLANDS[0][1]
        away = math.hypot(offshore - FAIRWAY[0], alongshore - FAIRWAY[1])
        anchorage = (away - reach) / away

        for part in range(1, 12):
            fraction = anchorage * part / 12.0
            depth = self.z(
                FAIRWAY[0] + (offshore - FAIRWAY[0]) * fraction,
                FAIRWAY[1] + (alongshore - FAIRWAY[1]) * fraction,
            )
            self.assertLess(depth, -LEAST_NAVIGABLE_M, f"only {-depth:.1f} m at {fraction:.0%}")

    def test_each_leg_is_a_fair_sail_from_the_last(self):
        """
        Between five and ten minutes under working sail, which is what the spacing was
        measured to give. Asserted against the *measured* speed rather than against the
        constant, because the constant is the thing that could be wrong.

        """
        for index in range(len(ISLANDS) - 1):
            here, there = self.middle(index), self.middle(index + 1)
            leg = math.hypot(there[0] - here[0], there[1] - here[1])
            minutes = leg / WORKING_SAIL_MS / 60.0
            self.assertGreater(minutes, 5.0, f"leg {index} is {minutes:.1f} minutes - barely a sail")
            self.assertLess(minutes, 10.0, f"leg {index} is {minutes:.1f} minutes - a slog")

    def test_the_declared_leg_matches_the_placement(self):
        """
        `ISLAND_LEG_M` is documentation, and documentation that disagrees with the code it
        describes is worse than none. This is the one test here that reads a constant, and
        it exists to catch a step edited without the note being updated.

        """
        self.assertAlmostEqual(math.hypot(*ISLAND_STEP), ISLAND_LEG_M, delta=25.0)


class TestThereIsWaterBetweenThem(ArchipelagoTestCase):
    def test_the_gaps_are_navigable(self):
        for index in range(len(ISLANDS) - 1):
            here, there = self.middle(index), self.middle(index + 1)
            depth = self.z((here[0] + there[0]) / 2.0, (here[1] + there[1]) / 2.0)
            self.assertLess(
                depth,
                -LEAST_NAVIGABLE_M,
                f"only {-depth:.1f} m between {ISLANDS[index][0]} and {ISLANDS[index + 1][0]}",
            )


class TestTheyShelveRatherThanStand(ArchipelagoTestCase):
    """
    An island with no foreshore is a cliff, and a lead line gives no warning before she
    strikes it. The taper that raises the ground is what provides the shelving, so this is
    really a test that the taper was not defeated by too small a reach.
    """

    def test_the_ground_falls_away_no_faster_than_the_taper_allows(self):
        """
        Measured against the taper, not against a fixed number of metres.

        A fixed threshold asks the wrong question, and asked it: six metres in thirty
        looked like a reasonable definition of a cliff, and Outer Skerry failed it at 6.6.
        The skerry is fine. It reaches only two hundred metres and it rises from
        twenty-five metres of water, so it climbs thirty-seven metres in that distance and
        *must* be steeper than an island four hundred metres across - the arithmetic had
        been done against the crest height alone, ignoring how deep the sea it stands in
        is.

        So the bound is the one the shape itself implies. A smoothstep reaches its steepest
        at the halfway point, at three halves of the average, and anything much beyond that
        is something other than the taper - a discontinuity, or two features fighting.

        """
        for index, (name, reach) in enumerate(ISLANDS):
            offshore, alongshore = self.middle(index)
            for heading in (0.0, 90.0, 180.0, 270.0):
                east = math.sin(math.radians(heading))
                north = math.cos(math.radians(heading))
                sounded = [
                    self.z(offshore + east * out, alongshore + north * out)
                    for out in range(0, int(reach + 600.0), 30)
                ]
                rise = sounded[0] - min(sounded)
                allowed = 1.5 * (rise / reach) * 30.0 * 1.6
                worst = max(sooner - later for sooner, later in zip(sounded, sounded[1:]))
                self.assertLess(
                    worst,
                    allowed,
                    f"{name} drops {worst:.1f} m in 30 m on heading {heading:.0f}, "
                    f"where its own taper over {reach:.0f} m allows {allowed:.1f} - "
                    "something steeper than the taper is at work",
                )

    def test_and_reaches_deep_water_outside_its_own_ground(self):
        for index, (name, reach) in enumerate(ISLANDS):
            offshore, alongshore = self.middle(index)
            self.assertLess(
                self.z(offshore + reach + 500.0, alongshore), -LEAST_NAVIGABLE_M, name
            )


class TestTheDryingRockSurvivedThem(ArchipelagoTestCase):
    """
    The chain was routed deliberately close to the drying rock, because a rock awash in the
    middle of an island passage is the best hazard on this coast. Close is the point and
    swallowed is the failure, and only the ground can tell the two apart - a clearance
    computed from the authored radii said these overlapped by more than a kilometre when
    in fact they never touch, because a feature's footprint is an ellipse and the arithmetic
    treated it as a circle of its long axis.
    """

    ROCK = (4_000.0, -5_000.0)

    def test_it_is_still_there(self):
        self.assertGreater(self.z(*self.ROCK), 0.0, "the drying rock no longer dries")

    def test_and_is_still_alone(self):
        for east, north in ((300.0, 0.0), (-300.0, 0.0), (0.0, 300.0), (0.0, -300.0)):
            depth = self.z(self.ROCK[0] + east, self.ROCK[1] + north)
            self.assertLess(depth, 0.0, "an island has grown out to meet the drying rock")
