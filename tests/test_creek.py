"""
Tests for the tidal creek, whose whole point is that its head is a time rather than a place.

A channel cut inland from a harbour is easy to get wrong in ways the source hides. It can
come out as a canal - below low water everywhere, always navigable, teaching nothing. It can
come out as a ditch, above high water everywhere and never wet. It can come out as a string
of ponds with sills between them, which looks like a creek on any chart and stops a keel
dead. Each of those has perfectly reasonable numbers behind it.

**Nothing here mentions a tide, and that is deliberate.** The tide belongs to the maritime
contrib and this generator must never import it. What a generator can be held to is the
shape of the ground, so these tests hand the bed a water level as a plain number and ask how
far up the water gets. That the level comes from a real tide, in a game, is somebody else's
department - and the separation is exactly why the creek works at all without either side
knowing about the other.
"""

import math
import unittest

from worldbuilder.regions.demo import (
    CREEK_REACHES,
    POND_AT,
    POND_REACH_M,
    POND_SURFACE_M,
    WORLD_SEED,
    demo_region,
)
from worldbuilder.terrain.surface import Surface

#: Water levels to ask the creek about, in metres relative to datum. Chosen to bracket an
#: ordinary coastal tide without this file having any opinion about tides.
LOW_SPRINGS = -2.0
LOW_NEAPS = -0.75
HIGH_NEAPS = 0.75
HIGH_SPRINGS = 2.0

#: How much water a boat that goes up a creek needs under her. A shoal-draught craft; nobody
#: takes a laden hull up a creek.
CREEK_DRAFT_M = 0.8


class CreekTestCase(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.region = demo_region()
        cls.surface = Surface(WORLD_SEED, features=cls.region.features)
        cls.coast = cls.region.coast
        cls.bed = cls.walk()

    @classmethod
    def walk(cls):
        """
        Returns:
            bed (list): `(metres up the creek, elevation)` along its own course, seaward
                first.

        Notes:
            Sampled along the channel rather than at the reaches. A sill between two reaches
            sits precisely where no reach is, so measuring at the reaches is measuring the
            places a gap cannot be.

        """
        points = []
        for index in range(len(CREEK_REACHES) - 1):
            here, there = CREEK_REACHES[index], CREEK_REACHES[index + 1]
            for part in range(20):
                fraction = part / 20.0
                points.append(
                    (
                        here[0] + (there[0] - here[0]) * fraction,
                        here[1] + (there[1] - here[1]) * fraction,
                    )
                )
        points.append(CREEK_REACHES[-1][:2])

        bed, run, previous = [], 0.0, None
        for offshore, along in points:
            if previous is not None:
                run += math.hypot(offshore - previous[0], along - previous[1])
            previous = (offshore, along)
            bed.append((run, cls.surface.elevation_m(cls.coast.at(offshore, along))))
        return bed

    def navigable_to(self, water_level, draft=0.0):
        """
        Args:
            water_level (float): The water surface, in metres relative to datum.
            draft (float, optional): What the boat needs under her.

        Returns:
            distance (float): How far up the creek she can get, in metres, before the bed
                comes up to meet her.

        """
        reached = 0.0
        for run, bed in self.bed:
            if bed > water_level - draft:
                break
            reached = run
        return reached


class TestItIsAChannelAndNotAStringOfPonds(CreekTestCase):
    def test_the_bed_climbs_the_whole_way_up(self):
        """
        A creek that dips back down between two reaches has a pond in it, and the far side
        of that pond is a sill. Both are invisible on a chart and entirely present to a
        keel.

        Allowed a few centimetres of wobble, because the ground has texture on it and a bed
        that was monotonic to the millimetre would mean the texture had been switched off.

        """
        for (_, sooner), (run, later) in zip(self.bed, self.bed[1:]):
            self.assertGreater(
                later, sooner - 0.35, f"the bed falls away again {run / 1000:.2f} km up"
            )

    def test_and_climbs_without_a_step_in_it(self):
        """Consecutive reaches overlap, so no edge of one shows against the next."""
        worst = max(abs(later - sooner) for (_, sooner), (_, later) in zip(self.bed, self.bed[1:]))
        self.assertLess(worst, 1.2, "the bed jumps between samples - the reaches do not overlap")

    def test_it_starts_below_the_water_and_ends_above_it(self):
        self.assertLess(self.bed[0][1], LOW_SPRINGS, "the creek does not reach the sea")
        self.assertGreater(self.bed[-1][1], HIGH_NEAPS, "the creek has no head - it is a canal")

    def test_it_is_long_enough_to_be_worth_going_up(self):
        self.assertGreater(self.bed[-1][0], 4_000.0)


class TestItsHeadIsATimeRatherThanAPlace(CreekTestCase):
    """
    The whole reason for it. Nothing computes where the water stops; the channel and the
    water level decide between them, so the head moves on its own.
    """

    def test_high_water_reaches_further_up_than_low(self):
        self.assertGreater(
            self.navigable_to(HIGH_SPRINGS),
            self.navigable_to(LOW_SPRINGS) + 2_000.0,
            "the tide barely moves the head of the creek",
        )

    def test_springs_reach_further_up_than_neaps(self):
        self.assertGreater(self.navigable_to(HIGH_SPRINGS), self.navigable_to(HIGH_NEAPS) + 400.0)

    def test_the_head_moves_monotonically_with_the_water(self):
        """Higher water can only ever reach further, and a creek where it did not would
        have a sill somewhere above the level that cleared it."""
        levels = (LOW_SPRINGS, LOW_NEAPS, 0.0, HIGH_NEAPS, HIGH_SPRINGS)
        reaches = [self.navigable_to(level) for level in levels]
        self.assertEqual(reaches, sorted(reaches))

    def test_a_boat_can_get_up_on_the_tide_and_be_left_by_it(self):
        """
        The situation the creek exists to create. She goes up on a making tide, and if she
        stays too long the water that carried her is somewhere else.

        """
        went_up = self.navigable_to(HIGH_SPRINGS, CREEK_DRAFT_M)
        came_back = self.navigable_to(LOW_SPRINGS, CREEK_DRAFT_M)
        self.assertGreater(
            went_up - came_back,
            1_500.0,
            "there is nowhere up this creek a boat can be caught out",
        )


class TestItJoinsTheHarbour(CreekTestCase):
    """A creek that does not reach the harbour is a lake nobody can get to."""

    def test_the_seaward_end_is_in_deep_water(self):
        self.assertLess(self.bed[0][1], -4.0)

    def test_and_it_is_navigable_from_there_at_any_state_of_the_tide(self):
        """
        The lower reach has to stay wet. A creek whose mouth dries is a creek nobody enters,
        because the moment it is passable at the top it is impassable at the bottom.

        """
        self.assertGreater(self.navigable_to(LOW_SPRINGS, CREEK_DRAFT_M), 1_500.0)


class TestThePondHoldsWater(CreekTestCase):
    """
    A closed basin, and the only question that matters is whether it is closed.

    Nothing in this generator models water running downhill, so a pond whose rim dips below
    its own surface is not a pond with a stream out of it - it is a picture of water standing
    above a gap. The rim has to hold all the way round, and "all the way round" is a claim
    about the *lowest* point of it, which is found by looking.

    How hard you look is part of the measurement. A sweep at thirty degrees and fifty-metre
    steps reported the first site holding by a comfortable quarter of a metre; a finer sweep
    found a notch two metres below the waterline, cut by the creek's own carve reaching
    further than anybody had checked. So this sweeps finely, on purpose.
    """

    RIM_STEP_DEG = 5
    RIM_STEP_M = 20
    RIM_OUT_M = 1_200

    #: How much rim a pond must have above its water before it counts as holding. Generous,
    #: because the rim is natural ground with texture on it.
    FREEBOARD_M = 1.0

    def ground(self, offshore, along):
        return self.surface.elevation_m(self.coast.at(offshore, along))

    def lowest_rim(self):
        """
        Returns:
            lowest (float): The lowest ground anywhere on the ring outside the pond.

        """
        lowest = math.inf
        for step in range(0, 360, self.RIM_STEP_DEG):
            east = math.sin(math.radians(step))
            north = math.cos(math.radians(step))
            for out in range(
                int(POND_REACH_M), int(POND_REACH_M) + self.RIM_OUT_M, self.RIM_STEP_M
            ):
                lowest = min(lowest, self.ground(POND_AT[0] + east * out, POND_AT[1] + north * out))
        return lowest

    def test_there_is_water_in_it(self):
        floor = self.ground(*POND_AT)
        self.assertLess(floor, POND_SURFACE_M, "the pond's floor stands above its own water")
        self.assertGreater(POND_SURFACE_M - floor, 2.0, "the pond is a puddle")

    def test_and_the_rim_holds_it_all_the_way_round(self):
        rim = self.lowest_rim()
        self.assertGreater(
            rim,
            POND_SURFACE_M + self.FREEBOARD_M,
            f"the rim dips to {rim:.2f} m and the water stands at {POND_SURFACE_M:.2f} - it leaks",
        )

    def test_it_is_big_enough_to_be_worth_putting_a_boat_on(self):
        across = sum(
            1
            for out in range(0, 1500, 10)
            if self.ground(POND_AT[0] + out, POND_AT[1]) < POND_SURFACE_M
        )
        self.assertGreater(
            across * 10 * 2, 300.0, "the pond is smaller than a boat's turning circle"
        )

    def test_it_is_well_clear_of_the_creek(self):
        """
        The two must not meet. They did: the creek's last reach is long enough to overlap
        its neighbour, which put its carve inside the first pond's rim.

        """
        head = CREEK_REACHES[-1]
        away = math.hypot(POND_AT[0] - head[0], POND_AT[1] - head[1])
        self.assertGreater(away, 2_500.0, "the creek reaches the pond and cuts its rim")

    def test_it_sits_well_above_the_sea(self):
        """The whole reason it needs its own water level. A pond at datum is a bay."""
        self.assertGreater(POND_SURFACE_M, 10.0)
