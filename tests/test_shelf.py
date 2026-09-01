"""
Tests for coastal bathymetry.

The nasty ones matter more than the obvious ones here, and they are all the same nasty
one: M1.4 produced four cliffs from four hard decisions taken on continuous quantities,
and this phase is full of tempting equivalents - is this near a coast, is this a continent,
is this an island, is the gradient good enough. Every one is a weight, and most of these
tests exist to prove it.

The rule the phase was built to: **a performance gate must sit outside the support of what
it gates, or fade to nothing before it.** The gate tests below are the general form of
M1.4's worst bug, where a profile was still a thousand metres tall at the range limit and
the optimisation that skipped it became the cliff.
"""

import math
import time
import unittest

from worldbuilder.bathymetry.shelf import COASTAL_WINDOW, SHELF_EDGE_M, Shelf
from worldbuilder.geometry.sphere import SpherePoint
from worldbuilder.geometry.tangent import TangentFrame
from worldbuilder.geometry.vectors import Vec3
from worldbuilder.plates.generation import plates_for
from worldbuilder.terrain.continentality import Continentality
from worldbuilder.terrain.tectonics import Tectonics

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


class ShelfTestCase(unittest.TestCase):
    def setUp(self):
        self.plates = plates_for(SEED)
        self.land = Continentality(SEED)
        self.tectonics = Tectonics(self.plates, self.land)
        self.shelf = Shelf(self.tectonics, self.land)

    def coastal_points(self, limit=60):
        found = []
        for point in scattered(3000):
            if abs(self.land.base_elevation(point)) < 250.0:
                found.append(point)
                if len(found) >= limit:
                    break
        return found


class TestNoCliffs(ShelfTestCase):
    def test_crossing_the_shore_is_smooth(self):
        for point in self.coastal_points(40):
            frame = TangentFrame.at(point)
            here = self.shelf.elevation_m(point)
            for x, y in ((1500.0, 0.0), (-1500.0, 0.0), (0.0, 1500.0), (0.0, -1500.0)):
                there = self.shelf.elevation_m(frame.local_to_sphere(x, y))
                self.assertLess(abs(there - here), 60.0)

    def test_crossing_the_activation_window_is_smooth(self):
        """
        The gate that saves the gradient's cost. If the shelf still had any say where the
        window closes, this gate would be a wall running round every continent at a fixed
        distance offshore - the exact shape of M1.4's trench bug.

        """
        for point in scattered(150):
            frame = TangentFrame.at(point)
            previous = None
            for step in range(80):
                out = frame.local_to_sphere(step * 2_000.0, 0.0)
                value = self.shelf.elevation_m(out)
                if previous is not None:
                    self.assertLess(abs(value - previous), 90.0)
                previous = value

    def test_the_weight_is_nothing_by_the_window_edge(self):
        """Stated directly: at the gate, the shelf must already have no say."""
        for point in scattered(2000):
            above = abs(self.land.above_shore(point))
            if above < COASTAL_WINDOW * 0.94 or above > COASTAL_WINDOW:
                continue
            coastal = self.shelf.coastal(point)
            if coastal is not None:
                self.assertLess(self.shelf.weight(point, coastal), 0.25)

    def test_a_flat_field_cannot_produce_extreme_ground(self):
        """
        Where the gradient approaches nothing the distance estimate divides by nearly
        nothing and reports a shore on the far side of the world. Nothing may come of it.

        """
        for point in scattered(2000):
            metres = self.shelf.elevation_m(point)
            self.assertTrue(math.isfinite(metres))
            self.assertGreater(metres, -7000.0)
            self.assertLess(metres, 2500.0)


class TestItShapesRatherThanReplaces(ShelfTestCase):
    def test_it_leaves_the_interior_alone(self):
        untouched = 0
        for point in scattered(1500):
            # Four hundred metres, not nine hundred: the continental base caps at seven
            # hundred, so the first version of this filter selected nothing at all and
            # passed by testing an empty set.
            if self.land.base_elevation(point) < 400.0:
                continue
            self.assertAlmostEqual(
                self.shelf.elevation_m(point), self.tectonics.elevation_m(point), places=6
            )
            untouched += 1
        self.assertGreater(untouched, 20)

    def test_it_leaves_the_deep_ocean_alone(self):
        untouched = 0
        for point in scattered(1500):
            if self.land.base_elevation(point) > -2500.0:
                continue
            self.assertAlmostEqual(
                self.shelf.elevation_m(point), self.tectonics.elevation_m(point), places=6
            )
            untouched += 1
        self.assertGreater(untouched, 50)

    def test_a_trench_survives_crossing_a_margin(self):
        """
        The composition rule with teeth. A shelf announcing that the water here is about
        a hundred and fifty metres must not fill in deliberate deep structure - and a
        blend, unlike an added offset, is capable of deferring.

        """
        deepest = 0.0
        deepest_at = None
        for point in scattered(2500):
            offset = self.tectonics.offset_m(point)
            if offset < deepest:
                deepest, deepest_at = offset, point
        self.assertLess(deepest, -800.0)

        macro = self.tectonics.elevation_m(deepest_at)
        after = self.shelf.elevation_m(deepest_at)
        self.assertLess(abs(after - macro), abs(macro) * 0.25 + 60.0)

    def test_the_land_fraction_barely_moves(self):
        points = scattered(3000)
        before = sum(1 for p in points if self.tectonics.elevation_m(p) >= 0.0)
        after = sum(1 for p in points if self.shelf.elevation_m(p) >= 0.0)
        self.assertLess(abs(after - before) / len(points), 0.04)


class TestTheProfile(ShelfTestCase):
    def test_a_margin_goes_land_shore_shelf_break_slope_basin(self):
        """
        The whole phase in one assertion. Walk seaward from a passive margin and the
        depths must go in order, reach shelf depth at the break, and carry on down.

        """
        best = None
        for point in scattered(3000):
            score = abs(self.land.base_elevation(point)) + abs(
                self.tectonics.offset_m(point)
            ) * 0.5
            gradient = self.land.gradient(point)
            if gradient.magnitude() <= 0.0:
                continue
            if best is None or score < best[0]:
                best = (score, point, gradient)
        self.assertIsNotNone(best)

        _, start, gradient = best
        frame = TangentFrame.at(start)
        scale = 1.0 / gradient.magnitude()
        east, north = -gradient.east * scale, -gradient.north * scale

        depths = []
        for km in range(0, 160, 10):
            out = frame.local_to_sphere(east * km * 1000.0, north * km * 1000.0)
            depths.append(self.shelf.elevation_m(out))

        self.assertGreater(depths[0], -60.0, "did not start at the shore")
        for earlier, later in zip(depths, depths[1:]):
            self.assertLessEqual(later, earlier + 25.0, "the seabed rose while sailing out")
        at_break = depths[8]  # eighty kilometres
        self.assertLess(at_break, SHELF_EDGE_M * 0.4)
        self.assertGreater(at_break, SHELF_EDGE_M * 3.0)
        self.assertLess(depths[-1], at_break, "no slope beyond the break")


class TestDeterminism(ShelfTestCase):
    def test_the_order_of_asking_changes_nothing(self):
        points = self.coastal_points(40)
        forward = [self.shelf.elevation_m(p) for p in points]
        backward = list(reversed([self.shelf.elevation_m(p) for p in reversed(points)]))
        self.assertEqual(forward, backward)

    def test_the_dateline_and_the_poles_are_ordinary(self):
        east = self.shelf.elevation_m(SpherePoint.from_latlon(15.0, 179.999))
        west = self.shelf.elevation_m(SpherePoint.from_latlon(15.0, -179.999))
        self.assertLess(abs(east - west), 40.0)
        for pole in (90.0, -90.0):
            self.assertTrue(
                math.isfinite(self.shelf.elevation_m(SpherePoint.from_latlon(pole, 0.0)))
            )


class TestCost(ShelfTestCase):
    """
    Two numbers, and the second is the one that matters.

    A gradient costs six times a field evaluation, so what decides whether this layer is
    affordable is not its cost but how rarely anything reaches it.
    """

    def test_how_often_a_sample_pays_for_a_gradient(self):
        points = scattered(96 * 96)

        asked = sum(
            1 for p in points if abs(self.land.above_shore(p)) <= COASTAL_WINDOW
        )

        start = time.perf_counter()
        for point in points:
            self.shelf.elevation_m(point)
        took = time.perf_counter() - start

        share = 100.0 * asked / len(points)
        print(f"\n    full terrain:             {took * 1000:7.1f} ms for {len(points)} "
              f"samples, {took / len(points) * 1e6:5.2f} us each")
        print(f"    samples paying for a gradient: {share:5.1f} %")

        self.assertLess(share, 35.0, "too much of the planet is asking for a gradient")


if __name__ == "__main__":
    unittest.main()
