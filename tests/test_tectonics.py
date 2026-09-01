"""
Tests for what plate motion does to the ground.

Three things are being defended, and none of them is "the mountains look nice".

**It contributes rather than decides.** The output is a number to add to the continental
base, so a test that tectonics has not become the primary author of continents is as
important as any test that it does anything at all.

**It is continuous, including where three plates meet.** M1.2 found a five-hundred-
kilometre discontinuity in margin distance before any terrain existed to reveal it; this
is the phase where such a thing would become a visible wall, so triple junctions get their
own test.

**It costs nothing where nothing is happening.** Most of a planet is far from any margin
and must pay only for the lookup it needed anyway.
"""

import math
import time
import unittest

from worldbuilder.geometry.sphere import SpherePoint
from worldbuilder.geometry.tangent import TangentFrame
from worldbuilder.geometry.vectors import Vec3
from worldbuilder.plates.generation import plates_for
from worldbuilder.plates.kinematics import CONVERGENT, DIVERGENT, TRANSFORM, motion_at
from worldbuilder.terrain.continentality import Continentality
from worldbuilder.terrain.tectonics import MAX_TECTONIC_RANGE_M, Tectonics

SEED = 20260831


def scattered(count=400):
    golden = math.pi * (3.0 - math.sqrt(5.0))
    points = []
    for index in range(count):
        z = 1.0 - 2.0 * (index + 0.5) / count
        ring = math.sqrt(max(0.0, 1.0 - z * z))
        angle = golden * index
        points.append(SpherePoint(Vec3(math.cos(angle) * ring, math.sin(angle) * ring, z)))
    return points


class TectonicsTestCase(unittest.TestCase):
    def setUp(self):
        self.plates = plates_for(SEED)
        self.land = Continentality(SEED)
        self.tectonics = Tectonics(self.plates, self.land)


class TestItContributesRatherThanDecides(TectonicsTestCase):
    def test_elevation_is_the_base_plus_the_contribution(self):
        """
        The composition rule, stated as arithmetic. Shelves, erosion and detail all have
        to compose with tectonics later rather than work out what it overwrote.

        """
        for point in scattered(120):
            self.assertAlmostEqual(
                self.tectonics.elevation_m(point),
                self.land.base_elevation(point) + self.tectonics.offset_m(point),
                places=9,
            )

    def test_tectonics_does_not_take_over_the_continents(self):
        """
        The regression that matters most. Tectonics should build mountains and trenches
        and lift the odd island above water - it should not turn a world that was 29 per
        cent land into one that is 45 per cent.

        """
        points = scattered(3000)
        before = sum(1 for p in points if self.land.base_elevation(p) >= 0.0)
        after = sum(1 for p in points if self.tectonics.elevation_m(p) >= 0.0)
        self.assertGreater(after / len(points), 0.25)
        self.assertLess(after / len(points), 0.35)
        self.assertLess(abs(after - before) / len(points), 0.05)

    def test_the_contribution_stays_within_its_stated_amplitudes(self):
        biggest = deepest = 0.0
        for point in scattered(2000):
            offset = self.tectonics.offset_m(point)
            biggest = max(biggest, offset)
            deepest = min(deepest, offset)
        self.assertLess(biggest, 1800.0)
        self.assertGreater(deepest, -3200.0)
        self.assertGreater(biggest, 400.0, "no uplift anywhere")
        self.assertLess(deepest, -400.0, "no trench anywhere")


class TestContinuity(TectonicsTestCase):
    def test_nothing_happens_far_from_a_margin(self):
        checked = 0
        for point in scattered(600):
            if self.plates.margin_at(point).distance_m > MAX_TECTONIC_RANGE_M + 1000.0:
                self.assertEqual(self.tectonics.offset_m(point), 0.0)
                checked += 1
        self.assertGreater(checked, 50)

    def test_the_gate_itself_is_not_a_cliff(self):
        """
        Every profile has to reach exactly zero by the range limit. If one did not, the
        `if distance >= MAX` that saves most of the work would become a wall of terrain
        running around every plate - an optimisation carving canyons.

        """
        for point in scattered(200):
            frame = TangentFrame.at(point)
            for bearing in (0.0, 90.0, 180.0, 270.0):
                radians = math.radians(bearing)
                previous = None
                for step in range(60):
                    distance = MAX_TECTONIC_RANGE_M - 30_000.0 + step * 1_000.0
                    out = frame.local_to_sphere(
                        math.sin(radians) * distance, math.cos(radians) * distance
                    )
                    offset = self.tectonics.offset_m(out)
                    if previous is not None:
                        self.assertLess(abs(offset - previous), 60.0)
                    previous = offset

    def test_the_ground_is_continuous_across_a_margin(self):
        """A kilometre of sailing must not find a hundred metres of cliff."""
        for point in scattered(80):
            frame = TangentFrame.at(point)
            here = self.tectonics.elevation_m(point)
            for x, y in ((2000.0, 0.0), (-2000.0, 0.0), (0.0, 2000.0), (0.0, -2000.0)):
                there = self.tectonics.elevation_m(frame.local_to_sphere(x, y))
                self.assertLess(abs(there - here), 150.0)

    def test_triple_junctions_do_not_hide_a_wall(self):
        """
        Where three plates meet, which is second-nearest changes constantly and the
        nearest-plate identity can change under a small step. M1.2 found a five-hundred-
        kilometre discontinuity of exactly this kind before terrain existed; this is the
        phase where one would become a visible wall.

        Found by looking for points where the second and third nearest seeds are nearly
        equidistant, then sampling densely all the way round.

        """
        junctions = []
        for point in scattered(1500):
            dots = sorted(
                (point.vector.dot(plate.seed.vector) for plate in self.plates), reverse=True
            )
            if abs(dots[1] - dots[2]) < 0.0015:
                junctions.append(point)
        self.assertGreater(len(junctions), 3, "no triple junctions found to test")

        for junction in junctions[:6]:
            frame = TangentFrame.at(junction)
            for bearing in range(0, 360, 15):
                radians = math.radians(bearing)
                previous = None
                for step in range(40):
                    distance = step * 3_000.0
                    out = frame.local_to_sphere(
                        math.sin(radians) * distance, math.cos(radians) * distance
                    )
                    offset = self.tectonics.offset_m(out)
                    if previous is not None:
                        self.assertLess(
                            abs(offset - previous), 120.0,
                            msg=f"jump near a triple junction at {junction.to_latlon()}",
                        )
                    previous = offset


class TestWhatMarginsDo(TectonicsTestCase):
    def _sample_by_kind(self, wanted, limit=40):
        found = []
        for point in scattered(2500):
            margin = self.plates.margin_at(point)
            if margin.distance_m > 120_000.0:
                continue
            motion = motion_at(point, self.plates)
            if motion and motion.kind == wanted:
                found.append(point)
                if len(found) >= limit:
                    break
        return found

    def test_a_transform_margin_leaves_no_mark(self):
        """
        Deliberate. A boundary does not have to change the ground merely because it can
        be classified, and inventing terrain to make every relationship visible would be
        a decision about looks rather than causes.

        Tested on points whose *only* margin in range is the transform one. The original
        version tested the nearest margin alone and started failing once contributions
        were summed - correctly, because a second margin two hundred kilometres off has
        every right to be doing something.

        """
        from worldbuilder.terrain.tectonics import MAX_TECTONIC_RANGE_M

        checked = 0
        for point in self._sample_by_kind(TRANSFORM, limit=200):
            _, margins = self.plates.margins_within(point, MAX_TECTONIC_RANGE_M)
            if len(margins) != 1:
                continue
            self.assertAlmostEqual(self.tectonics.offset_m(point), 0.0, places=6)
            checked += 1
        self.assertGreater(checked, 3, "no purely transform neighbourhoods found")

    def test_convergence_makes_relief_rather_than_only_mountains(self):
        """
        The first version of this test asserted that convergence raises the ground on
        average, and it failed at -0.6 metres. The code was right and the test was wrong:
        a subduction zone pairs nine hundred metres of mountains with a trench of two and
        a half thousand, so the average across a margin is *supposed* to come out slightly
        negative. What convergence actually makes is relief.

        """
        points = self._sample_by_kind(CONVERGENT, limit=120)
        self.assertGreater(len(points), 5)
        offsets = [self.tectonics.offset_m(p) for p in points]
        self.assertGreater(max(offsets), 200.0, "convergence raised nothing anywhere")
        self.assertLess(min(offsets), -200.0, "convergence dropped nothing anywhere")

    def test_divergence_builds_a_ridge(self):
        points = self._sample_by_kind(DIVERGENT)
        self.assertGreater(len(points), 5)
        average = sum(self.tectonics.offset_m(p) for p in points) / len(points)
        self.assertGreater(average, 0.0)

    def test_trenches_exist_and_are_not_on_the_continental_side(self):
        """
        The bug the contribution map caught. `margin.distance_m` is unsigned - a point is
        always a positive distance into its *own* plate - so a trench meant to sit ninety
        kilometres out on the ocean side was centred at a distance no sample could have.
        Only its tail appeared, and the world came out with mountains everywhere and
        almost no trenches.

        """
        deepest = 0.0
        deepest_at = None
        for point in scattered(2500):
            offset = self.tectonics.offset_m(point)
            if offset < deepest:
                deepest, deepest_at = offset, point
        self.assertLess(deepest, -1000.0, "no real trench anywhere on the planet")
        self.assertLess(
            self.land.at(deepest_at), 0.2, "the deepest trench is inside a continent"
        )


class TestDeterminism(TectonicsTestCase):
    def test_the_same_world_twice(self):
        again = Tectonics(plates_for(SEED), Continentality(SEED))
        for point in scattered(120):
            self.assertEqual(self.tectonics.offset_m(point), again.offset_m(point))

    def test_the_order_of_asking_changes_nothing(self):
        points = scattered(80)
        forward = [self.tectonics.offset_m(p) for p in points]
        backward = list(reversed([self.tectonics.offset_m(p) for p in reversed(points)]))
        self.assertEqual(forward, backward)


class TestCost(TectonicsTestCase):
    """
    Both halves of the question: what a path costs, and how often anything takes it.

    An expensive branch nothing reaches is free. A cheap branch everything reaches is not.
    """

    def test_what_a_chart_of_macro_terrain_costs(self):
        points = scattered(96 * 96)

        branches = {"interior": 0, "quiet margin": 0, "active": 0}
        for point in points:
            margin = self.plates.margin_at(point)
            if margin.distance_m >= MAX_TECTONIC_RANGE_M:
                branches["interior"] += 1
            else:
                motion = motion_at(point, self.plates)
                if motion and motion.kind in (CONVERGENT, DIVERGENT):
                    branches["active"] += 1
                else:
                    branches["quiet margin"] += 1

        start = time.perf_counter()
        for point in points:
            self.tectonics.elevation_m(point)
        took = time.perf_counter() - start

        print(f"\n    macro elevation:          {took * 1000:7.1f} ms for {len(points)} "
              f"samples, {took / len(points) * 1e6:5.2f} us each")
        print("    branch taken:")
        for name, count in branches.items():
            print(f"      {name:14} {100.0 * count / len(points):5.1f} %")

        self.assertLess(took / len(points) * 1e6, 120.0)


if __name__ == "__main__":
    unittest.main()
