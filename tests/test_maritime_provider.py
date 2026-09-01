"""
Tests for the maritime adapter, written against a stand-in for maritime.

The contrib cannot be imported here - its package pulls in Twisted - so these tests use a
position object with the three attributes the adapter actually reads. That is a real risk
and worth naming: **a stand-in can drift from the thing it stands in for.** It is mitigated
two ways. The adapter reads only `x`, `y` and `region`, which is a small enough surface to
keep honest by inspection; and the protocol test below asserts that every method maritime's
base provider declares is present with the right signature, so a rename on the other side
shows up here rather than in a live session.

What these tests are really checking is the seam: that a sphere can present itself as a flat
region without the flatness lying, and that the marks layer arrives at the far end as
hazards a hull cannot pass through.
"""

import inspect
import math
import unittest

from worldbuilder.integration.maritime import (
    BOTTOM_NAMES,
    Danger,
    WorldbuilderTerrain,
    _distance_to_track,
)
from worldbuilder.regions.demo import WORLD_SEED, demo_region
from worldbuilder.terrain.surface import Surface


class Position:
    """What the adapter reads off a maritime position, and nothing else."""

    __slots__ = ("x", "y", "z", "region")

    def __init__(self, x, y, z=0.0, region="default"):
        self.x, self.y, self.z, self.region = float(x), float(y), float(z), region


class ProviderTestCase(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.region = demo_region()
        cls.coast = cls.region.coast
        cls.world = Surface(WORLD_SEED, features=cls.region.features)
        cls.provider = WorldbuilderTerrain(cls.world, cls.region, region_name="demo")

    def local(self, point):
        east, north = self.provider.frame.sphere_to_local(point)
        return Position(east, north, region="demo")

    def named(self, kind):
        for feature in self.region.features:
            if feature.kind == kind:
                return feature
        raise AssertionError(f"no feature called {kind}")


class TestTheSeam(ProviderTestCase):
    def test_flat_metres_land_where_the_sphere_says(self):
        for east in (-40_000.0, -5_000.0, 0.0, 5_000.0, 40_000.0):
            for north in (-40_000.0, 0.0, 40_000.0):
                position = Position(east, north)
                back = self.provider.frame.sphere_to_local(
                    self.provider.point_at(position)
                )
                self.assertAlmostEqual(back[0], east, places=3)
                self.assertAlmostEqual(back[1], north, places=3)

    def test_the_ground_maritime_sees_is_the_ground_the_planet_has(self):
        for km in range(-8, 30, 2):
            point = self.coast.at(km * 1_000.0)
            self.assertAlmostEqual(
                self.provider.terrain_z_at(self.local(point)),
                self.world.elevation_m(point),
                places=6,
            )

    def test_it_answers_canonically_rather_than_approximately(self):
        """
        Maritime models a chart's ignorance itself, adding a deterministic sounding error
        on top of whatever the world says. A provider that answered approximately would
        put a second, unmodelled error underneath the first.

        Asserted on the point the adapter itself resolves. Comparing against a point
        projected out and back instead measures the round trip, which differs in the
        thirteenth decimal place and has nothing to do with the claim.

        """
        for step in range(0, 20):
            position = Position(step * 1_500.0 - 6_000.0, step * 900.0)
            self.assertEqual(
                self.provider.terrain_z_at(position),
                self.world.elevation_m(self.provider.point_at(position)),
            )

    def test_a_region_is_flat_enough_at_its_own_reach(self):
        """
        M1.1 measured the cap at two hundred kilometres, where two charted points disagree
        by under six metres. The demonstration region is sixty, so the flatness maritime
        assumes has to be far inside that.

        """
        worst = 0.0
        for bearing in range(0, 360, 15):
            radians = math.radians(bearing)
            reach = self.region.reach_m
            east, north = math.sin(radians) * reach, math.cos(radians) * reach
            point = self.provider.point_at(Position(east, north))
            true_m = self.region.origin.distance_to(point, self.world.radius_m)
            worst = max(worst, abs(true_m - math.hypot(east, north)))
        self.assertLess(worst, 1.0, f"{worst:.2f} m of projection error at the reach")

    def test_the_bottom_arrives_in_maritime_vocabulary(self):
        for km in (2, 6, 10, 30, 60):
            bottom = self.provider.bottom_type_at(self.local(self.coast.at(km * 1_000.0)))
            self.assertIn(bottom, ("sand", "mud", "rock", "gravel", "weed", "reef"))

    def test_every_bottom_the_generator_makes_has_a_maritime_name(self):
        from worldbuilder.bathymetry.substrate import MUD, ROCK, SAND

        self.assertEqual(set(BOTTOM_NAMES), {SAND, MUD, ROCK})


class TestTheMarksArriveAsHazards(ProviderTestCase):
    def test_the_pinnacle_is_a_hazard(self):
        pinnacle = self.named("pinnacle")
        here = self.local(pinnacle.at)
        touched = self.provider.hazards_touching(
            Position(here.x - 400.0, here.y), Position(here.x + 400.0, here.y), width=6.0
        )
        self.assertIn("pinnacle", [danger.key for danger in touched])

    def test_a_hull_that_passes_clear_touches_nothing(self):
        pinnacle = self.named("pinnacle")
        here = self.local(pinnacle.at)
        clear = self.provider.hazards_touching(
            Position(here.x - 400.0, here.y + 900.0),
            Position(here.x + 400.0, here.y + 900.0),
            width=6.0,
        )
        self.assertEqual([d.key for d in clear if d.key.startswith("pinnacle")], [])

    def test_the_worst_news_comes_first(self):
        pinnacle = self.named("pinnacle")
        here = self.local(pinnacle.at)
        touched = self.provider.hazards_touching(
            Position(here.x - 30_000.0, here.y - 20_000.0),
            Position(here.x + 10_000.0, here.y + 10_000.0),
            width=200.0,
        )
        tops = [danger.top_z for danger in touched]
        self.assertEqual(tops, sorted(tops, reverse=True))

    def test_a_hazard_stands_where_the_terrain_says_it_does(self):
        """
        `top_z` is taken from the ground at each circle's own centre, not from what the
        feature was told to be, so a hazard and the terrain under a hull cannot disagree.

        """
        for danger in self.provider.dangers:
            position = Position(danger.x, danger.y)
            self.assertAlmostEqual(
                danger.top_z, self.provider.terrain_z_at(position), places=6
            )

    def test_a_breakwater_is_more_than_one_circle(self):
        """
        A mole two kilometres long and three hundred and forty wide is a hazard, and one
        circle round it would either miss most of it or declare two square kilometres of
        harbour approach foul.

        """
        circles = [d for d in self.provider.dangers if d.key.startswith("north mole")]
        self.assertGreater(len(circles), 3)
        for danger in circles:
            self.assertLess(danger.radius, 400.0)

    def test_nothing_passes_between_the_circles_of_a_breakwater(self):
        """
        The reason they overlap. A hull crossing a mole must touch it wherever she meets
        it, not only where a circle happens to be centred.

        Crossed at right angles to the structure, worked out from the circles themselves.
        The first version of this swept tracks along the region's east axis, which the
        mole very nearly runs along too - so it was sailing *beside* a breakwater and
        finding, correctly, that it never hit one.

        """
        circles = [d for d in self.provider.dangers if d.key.startswith("north mole")]
        self.assertGreater(len(circles), 3)

        run_x = circles[-1].x - circles[0].x
        run_y = circles[-1].y - circles[0].y
        length = math.hypot(run_x, run_y)
        across_x, across_y = -run_y / length, run_x / length

        for first, second in zip(circles, circles[1:]):
            # Between two neighbours is where a gap would be if there were one.
            gap_x, gap_y = (first.x + second.x) * 0.5, (first.y + second.y) * 0.5
            touched = self.provider.hazards_touching(
                Position(gap_x - across_x * 500.0, gap_y - across_y * 500.0),
                Position(gap_x + across_x * 500.0, gap_y + across_y * 500.0),
                width=6.0,
            )
            self.assertTrue(
                any(danger.key.startswith("north mole") for danger in touched),
                f"a hull crossed the mole at {gap_x:.0f}, {gap_y:.0f} and touched nothing",
            )

    def test_a_round_mark_stays_one_circle(self):
        for kind in ("pinnacle", "drying rock"):
            circles = [d for d in self.provider.dangers if d.key.startswith(kind)]
            self.assertEqual(len(circles), 1, kind)

    def test_only_marked_features_become_hazards(self):
        keys = " ".join(danger.key for danger in self.provider.dangers)
        for feature in self.region.features:
            if feature.marked:
                self.assertIn(feature.kind, keys)
            else:
                self.assertNotIn(feature.kind, keys)

    def test_a_hazard_knows_what_it_is_made_of(self):
        for danger in self.provider.dangers:
            self.assertIn(danger.bottom, ("sand", "mud", "rock"))
        pinnacle = [d for d in self.provider.dangers if d.key == "pinnacle"][0]
        self.assertEqual(pinnacle.bottom, "rock")

    def test_every_hazard_is_ground_a_chart_lies_about(self):
        """
        The defining property, and the same one that decided the feature was marked. A
        hazard whose depth the soundings already print is not a hazard, it is a bottom.

        """
        for danger in self.provider.dangers:
            charted = self.provider._charted(
                self.provider.frame.local_to_sphere(danger.x, danger.y)
            )
            self.assertGreater(
                danger.top_z - charted, 2.0,
                f"{danger.key} is charted perfectly well and should not be a hazard",
            )

    def test_the_tapering_ends_of_a_breakwater_are_not_hazards(self):
        """
        The bug this rule fixed. A mole's support runs two kilometres either side of its
        centre, but the far ends taper into ordinary seabed seventeen metres down. Kept
        unconditionally they came back as dangers, and two kilometres of harbour approach
        either side of the structure were foul ground for a hull that could not have
        touched anything.

        """
        for danger in self.provider.dangers:
            if not danger.key.startswith(("north mole", "south mole")):
                continue
            self.assertGreater(danger.top_z, -9.0, danger.key)

    def test_surveying_happens_once(self):
        first = self.provider.dangers
        self.provider.hazards_touching(Position(0.0, 0.0), Position(1_000.0, 0.0))
        self.assertIs(self.provider.dangers, first)


class TestTheArithmetic(unittest.TestCase):
    def test_distance_to_a_track(self):
        self.assertAlmostEqual(_distance_to_track(0.0, 5.0, -10.0, 0.0, 10.0, 0.0), 5.0)
        self.assertAlmostEqual(_distance_to_track(20.0, 0.0, -10.0, 0.0, 10.0, 0.0), 10.0)
        self.assertAlmostEqual(_distance_to_track(-20.0, 0.0, -10.0, 0.0, 10.0, 0.0), 10.0)

    def test_a_track_of_no_length_is_a_point(self):
        self.assertAlmostEqual(_distance_to_track(3.0, 4.0, 0.0, 0.0, 0.0, 0.0), 5.0)

    def test_a_danger_says_what_it_is(self):
        danger = Danger("rock", 1.0, 2.0, 30.0, -3.5, "rock", "demo")
        self.assertIn("rock", repr(danger))


class TestItFitsTheInterfaceItClaimsTo(ProviderTestCase):
    """
    The stand-in's weak point, addressed directly.

    These tests use a position object rather than maritime's own, so a rename on the other
    side would not fail them - it would fail a live session instead. What can be checked
    without importing maritime is that the adapter offers the methods maritime's base
    provider declares, spelled the same way and taking the same arguments.
    """

    EXPECTED = {
        "terrain_z_at": ("self", "position"),
        "bottom_type_at": ("self", "position"),
        "hazards_touching": ("self", "before", "after", "width"),
    }

    def test_the_methods_are_spelled_the_way_maritime_declares_them(self):
        for name, parameters in self.EXPECTED.items():
            method = getattr(WorldbuilderTerrain, name, None)
            self.assertIsNotNone(method, f"the adapter does not offer {name}")
            actual = tuple(inspect.signature(method).parameters)
            self.assertEqual(actual, parameters, name)

    def test_it_does_not_import_maritime_to_do_any_of_it(self):
        """
        The generator must not depend on the thing it adapts to. `maritime_provider` is
        the single exception and imports inside the call.

        """
        import worldbuilder.integration.maritime as adapter

        source = inspect.getsource(adapter)
        top_level = [
            line for line in source.splitlines()
            if line.startswith(("import ", "from ")) and "evennia" in line
        ]
        self.assertEqual(top_level, [])

    def test_a_region_is_required_to_sit_somewhere(self):
        with self.assertRaises(ValueError):
            WorldbuilderTerrain(self.world, None)


if __name__ == "__main__":
    unittest.main()


class TestTheRocksReachThePaper(ProviderTestCase):
    """
    The other end of the marks layer, and the one a captain actually gets to use.

    `hazards_touching` answers what a hull would hit; this answers what the paper should
    show. M1.7 measured why the second cannot be derived from the first by sampling: a
    four-hundred-metre grid finds this pinnacle in one chart in sixty-four. The rock has
    to arrive as a symbol or it does not arrive at all.
    """

    def test_the_pinnacle_belongs_on_the_paper(self):
        keys = [d.key for d in self.provider.charted_dangers(Position(0.0, 0.0), 60_000.0)]
        self.assertIn("pinnacle", keys)
        self.assertIn("drying rock", keys)

    def test_a_sheet_that_does_not_reach_it_does_not_show_it(self):
        near = self.local(self.named("pinnacle").at)
        far = Position(near.x + 40_000.0, near.y + 40_000.0)
        keys = [d.key for d in self.provider.charted_dangers(far, 2_000.0)]
        self.assertNotIn("pinnacle", keys)

    def test_the_worst_news_is_first(self):
        found = self.provider.charted_dangers(Position(0.0, 0.0), 60_000.0)
        tops = [danger.top_z for danger in found]
        self.assertEqual(tops, sorted(tops, reverse=True))

    def test_the_paper_and_the_physics_are_one_list(self):
        """
        The property that makes this worth having. A chart showing one set of rocks while
        the hull was measured against another would be a chart that lies in a new and
        more interesting way.

        """
        here = self.local(self.named("pinnacle").at)
        drawn = self.provider.charted_dangers(here, 3_000.0)
        struck = self.provider.hazards_touching(
            Position(here.x - 400.0, here.y), Position(here.x + 400.0, here.y), width=8.0
        )
        for danger in struck:
            self.assertIn(danger.key, [shown.key for shown in drawn])

    def test_the_box_is_square_because_the_paper_is(self):
        """A rock off the corner of the sheet is still on the sheet."""
        here = self.local(self.named("pinnacle").at)
        corner = Position(here.x - 900.0, here.y - 900.0)
        keys = [d.key for d in self.provider.charted_dangers(corner, 1_000.0)]
        self.assertIn("pinnacle", keys)

    def test_a_region_with_nothing_marked_shows_nothing(self):
        from worldbuilder.bathymetry.features import Features

        bare = WorldbuilderTerrain(
            self.world, self.region, region_name="demo", features=Features()
        )
        self.assertEqual(bare.charted_dangers(Position(0.0, 0.0), 60_000.0), ())
