"""
Tests for what the bottom is made of.

The phase has one characteristic bug and it is not the obvious one.

**The obvious risk is the category.** Three names are as hard a decision as this engine
contains, and if anything continuous were computed from the word, every boundary between
sand and mud would be a cliff in whatever depended on it. It is not: the field is three
fractions and the word is only ever the largest of them.

**The real risk is the slope probe.** A finite difference cannot see anything narrower than
its own baseline, and at six hundred metres it straddled the pinnacle - reporting flat
bottom a hundred and thirty metres from a rock standing twenty metres proud, and steep
bottom three hundred metres away where one probe happened to land on it. The rock field
came out as *rings*. That is not a rounding error, it is the substrate equivalent of the
moving-grid problem, and it gets the test with teeth below.
"""

import math
import time
import unittest

from worldbuilder.bathymetry.substrate import (
    MUD,
    ROCK,
    SAND,
    SLOPE_BASELINE_M,
    Composition,
)
from worldbuilder.geometry.sphere import SpherePoint
from worldbuilder.geometry.tangent import TangentFrame
from worldbuilder.geometry.vectors import Vec3
from worldbuilder.regions.demo import WORLD_SEED, demo_region
from worldbuilder.terrain.surface import Surface


def scattered(count=400):
    golden = math.pi * (3.0 - math.sqrt(5.0))
    points = []
    for index in range(count):
        z = 1.0 - 2.0 * (index + 0.5) / count
        ring = math.sqrt(max(0.0, 1.0 - z * z))
        angle = golden * index
        points.append(SpherePoint(Vec3(math.cos(angle) * ring, math.sin(angle) * ring, z)))
    return points


class SubstrateTestCase(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.region = demo_region()
        cls.coast = cls.region.coast
        cls.world = Surface(WORLD_SEED, features=cls.region.features)
        cls.bare = Surface(WORLD_SEED)

    def named(self, kind):
        for feature in self.region.features:
            if feature.kind == kind:
                return feature
        raise AssertionError(f"no feature called {kind}")


class TestItIsAlwaysAComposition(SubstrateTestCase):
    def test_the_fractions_sum_to_one_everywhere(self):
        for point in scattered(500) + [
            SpherePoint.from_latlon(90.0, 0.0),
            SpherePoint.from_latlon(-90.0, 0.0),
            SpherePoint.from_latlon(0.0, 180.0),
        ]:
            bottom = self.bare.bottom_at(point)
            self.assertAlmostEqual(bottom.sand + bottom.mud + bottom.rock, 1.0, places=9)
            for share in (bottom.sand, bottom.mud, bottom.rock):
                self.assertGreaterEqual(share, 0.0)
                self.assertLessEqual(share, 1.0)

    def test_the_word_is_only_ever_the_largest_fraction(self):
        for point in scattered(300):
            bottom = self.world.bottom_at(point)
            shares = {SAND: bottom.sand, MUD: bottom.mud, ROCK: bottom.rock}
            self.assertAlmostEqual(shares[bottom.dominant], max(shares.values()), places=9)

    def test_a_degenerate_composition_does_not_divide_by_nothing(self):
        self.assertEqual(Composition(0.0, 0.0, 0.0).dominant, ROCK)

    def test_holding_ranks_the_three_the_way_ground_tackle_does(self):
        mud = Composition(0.0, 1.0, 0.0)
        sand = Composition(1.0, 0.0, 0.0)
        rock = Composition(0.0, 0.0, 1.0)
        self.assertGreater(mud.holding(), sand.holding())
        self.assertGreater(sand.holding(), rock.holding())
        self.assertEqual(rock.holding(), 0.0)

    def test_handing_in_known_values_changes_nothing(self):
        """
        The same bargain the shelf struck. A caller that already has the ground should not
        pay for it twice, and must not get a different answer for having saved the work.

        """
        for point in [self.coast.at(km * 2_000.0) for km in range(-3, 12)]:
            elevation = self.world.structural_m(point)
            tectonic = self.world.tectonics.offset_m(point)
            slope = self.world.substrate.slope_at(point)
            given = self.world.substrate.at(
                point, elevation_m=elevation, slope=slope, tectonic_m=tectonic
            )
            worked_out = self.world.substrate.at(point)
            self.assertAlmostEqual(given.sand, worked_out.sand, places=9)
            self.assertAlmostEqual(given.mud, worked_out.mud, places=9)
            self.assertAlmostEqual(given.rock, worked_out.rock, places=9)


class TestTheSlopeProbeResolvesWhatWasPlaced(SubstrateTestCase):
    """
    The bug this phase produces, and the test that catches it.

    A finite difference is blind to anything narrower than its baseline. Sampled at six
    hundred metres, the bottom beside a hundred-and-forty-metre pinnacle read *flat* -
    both probes missing the rock - while the bottom three hundred metres away read steep,
    because one probe landed on it. Rock in rings, sand against the rock itself.
    """

    def rock_by_distance(self, feature, step_m, count):
        frame = TangentFrame.at(feature.at)
        return [
            self.world.substrate.at(frame.local_to_sphere(index * step_m, 0.0)).rock
            for index in range(count)
        ]

    def test_the_baseline_resolves_the_narrowest_thing_placed(self):
        narrowest = min(
            min(feature.length_m, feature.width_m) * 2.0
            for feature in self.region.features
        )
        self.assertLess(
            SLOPE_BASELINE_M, narrowest,
            "the slope probe is wider than the smallest feature, so it cannot see it",
        )

    def test_rock_does_not_come_in_rings_round_a_hazard(self):
        """
        Walking away from a rock, the bottom may stop being rocky. It may not stop and
        then start again - that is the aliasing signature, and it is what a six-hundred-
        metre baseline produced.

        """
        for kind in ("pinnacle", "drying rock"):
            shares = self.rock_by_distance(self.named(kind), 25.0, 24)
            for nearer, further in zip(shares, shares[1:]):
                self.assertLessEqual(
                    further, nearer + 0.02,
                    f"the bottom gets rockier with distance from the {kind}: {shares}",
                )

    def test_the_bottom_beside_a_rock_is_rock(self):
        pinnacle = self.named("pinnacle")
        frame = TangentFrame.at(pinnacle.at)
        for offset in (-50.0, -30.0, 30.0, 50.0):
            beside = self.world.substrate.at(frame.local_to_sphere(offset, 0.0))
            self.assertEqual(beside.dominant, ROCK, f"{offset:+.0f} m from the pinnacle")

    def test_and_the_bottom_well_clear_of_it_is_not(self):
        pinnacle = self.named("pinnacle")
        frame = TangentFrame.at(pinnacle.at)
        for offset in (-400.0, -250.0, 250.0, 400.0):
            clear = self.world.substrate.at(frame.local_to_sphere(offset, 0.0))
            self.assertNotEqual(clear.dominant, ROCK)


class TestItIsDerivedFromTheGround(SubstrateTestCase):
    def test_deep_water_is_mud_and_shoal_water_is_sand(self):
        """
        Above wave base the sea keeps the bottom swept; below it, the fines settle. Tested
        on ordinary ground well clear of anything placed, so it is the depth answering.

        """
        shoal = self.bare.bottom_at(self.coast.at(2_000.0, -30_000.0))
        deep = self.bare.bottom_at(self.coast.at(200_000.0, -30_000.0))
        self.assertGreater(shoal.sand, 0.7)
        self.assertGreater(deep.mud, 0.7)

    def test_sand_gives_way_to_mud_without_ever_going_back(self):
        shares = [
            self.bare.bottom_at(self.coast.at(km * 4_000.0, -30_000.0)).sand
            for km in range(1, 40)
        ]
        for shallower, deeper in zip(shares, shares[1:]):
            self.assertLessEqual(deeper, shallower + 0.02)

    def test_deliberate_deep_structure_is_rock(self):
        deepest, at = 0.0, None
        for point in scattered(2500):
            offset = self.bare.tectonics.offset_m(point)
            if offset < deepest:
                deepest, at = offset, point
        self.assertLess(deepest, -800.0)
        self.assertEqual(self.bare.bottom_at(at).dominant, ROCK)

    def test_a_gentle_regional_swell_is_not_rock(self):
        """
        The calibration that was wrong first time. A passive margin carries a hundred and
        fifty metres of tectonic contribution, and at the original threshold the whole
        demonstration coast came out a third rock. A broad rise is not a rock face.

        """
        rocky = 0
        for km in range(2, 60, 2):
            if self.bare.bottom_at(self.coast.at(km * 1_000.0, -30_000.0)).rock > 0.2:
                rocky += 1
        self.assertEqual(rocky, 0)


class TestWhatWasPlacedOverrules(SubstrateTestCase):
    def test_every_feature_that_declares_a_substrate_gets_it(self):
        for feature in self.region.features:
            if feature.substrate is None:
                continue
            self.assertEqual(
                self.world.bottom_at(feature.at).dominant, feature.substrate, feature.kind
            )

    def test_a_declaration_does_not_reach_beyond_the_feature(self):
        """
        Asserted against the featureless world rather than against a substrate name. Deep
        water thirty kilometres out is mud whatever anybody declared, so "it is not mud
        out there" proves nothing at all - which is what the first version of this test
        was measuring.

        """
        self.assertEqual(self.world.bottom_at(self.named("harbour basin").at).dominant, MUD)
        for point in scattered(400):
            if self.region.origin.distance_to(point) < 100_000.0:
                continue
            placed, plain = self.world.bottom_at(point), self.bare.bottom_at(point)
            self.assertEqual(placed.sand, plain.sand)
            self.assertEqual(placed.mud, plain.mud)
            self.assertEqual(placed.rock, plain.rock)

    def test_the_slope_a_feature_makes_does_not_overrule_what_it_says_it_is(self):
        """
        A bank declaring sand has flanks of its own making, and those flanks are the
        steepest ground for miles. If the slope term won there, every placed feature would
        be ringed with rock it never asked for and the declaration would hold only at the
        exact centre.

        It does not win: across the whole support of the bank the rock share peaks near
        three tenths and sand stays dominant throughout. Which is also the right answer -
        a scoured bank edge is coarser than its crest, and with three fractions the coarse
        end is what rock means.

        """
        bank = self.named("north bank")
        frame = TangentFrame.at(bank.at)
        peak = 0.0
        for step in range(-6, 7):
            bottom = self.world.substrate.at(frame.local_to_sphere(0.0, step * 250.0))
            self.assertEqual(bottom.dominant, SAND, f"{step * 250} m across the bank")
            peak = max(peak, bottom.rock)
        self.assertGreater(peak, 0.1, "the flank is not coarsening at all")
        self.assertLess(peak, 0.5)

    def test_a_declaration_blends_rather_than_switches(self):
        """
        The same argument as everywhere else. A dredged basin declaring mud must fade into
        the sand around it, not end at a line.

        """
        basin = self.named("harbour basin")
        frame = TangentFrame.at(basin.at)
        span = basin.reach_m() * 1.4
        previous = None
        for index in range(401):
            offset = (2.0 * index / 400 - 1.0) * span
            share = self.world.substrate.at(frame.local_to_sphere(offset, 0.0)).mud
            if previous is not None:
                self.assertLess(abs(share - previous), 0.08)
            previous = share

    def test_a_world_with_no_features_still_answers(self):
        for point in scattered(200):
            self.assertAlmostEqual(sum(
                (lambda b: (b.sand, b.mud, b.rock))(self.bare.bottom_at(point))
            ), 1.0, places=9)


class TestCost(SubstrateTestCase):
    def test_a_bottom_costs_several_soundings_and_that_is_the_bargain(self):
        points = [self.coast.at(i * 300.0, j * 300.0)
                  for i in range(-16, 16) for j in range(-16, 16)]

        start = time.perf_counter()
        for point in points:
            self.world.bottom_at(point)
        bottom = time.perf_counter() - start

        start = time.perf_counter()
        for point in points:
            self.world.elevation_m(point)
        elevation = time.perf_counter() - start

        print(f"\n    bottom    {bottom / len(points) * 1e6:6.1f} us a sample")
        print(f"    elevation {elevation / len(points) * 1e6:6.1f} us a sample"
              f"   ({bottom / elevation:.1f}x)")

        # Four probes and a frame. Much more than that means something is being recomputed.
        self.assertLess(bottom, elevation * 8.0)


if __name__ == "__main__":
    unittest.main()
