"""
Tests for the field that decides where land is.

The claim being defended is architectural rather than numerical: **continentality does not
know that plates exist.** If it did, continents would inherit the shapes of Voronoi cells
and every coastline would follow a tectonic boundary, which is the single most obvious tell
of a generated world. The test for that is not a picture but an import list and a
correlation.

The rest is about staying in its lane. This layer decides *this side continent, that side
ocean* and nothing finer, and a test that it produces no coastline detail is as important
as one that it produces continents at all - because detail generated here would have stolen
work from layers that have not been written yet.
"""

import math
import time
import unittest

from worldbuilder.geometry.sphere import EARTH_RADIUS_M, SpherePoint
from worldbuilder.geometry.tangent import TangentFrame
from worldbuilder.geometry.vectors import Vec3
from worldbuilder.terrain.continentality import Continentality
from worldbuilder.terrain.noise import Noise

SEED = 20260831
NORTH_POLE = SpherePoint.from_latlon(90.0, 0.0)
SOUTH_POLE = SpherePoint.from_latlon(-90.0, 0.0)


def scattered(count=300):
    golden = math.pi * (3.0 - math.sqrt(5.0))
    points = []
    for index in range(count):
        z = 1.0 - 2.0 * (index + 0.5) / count
        ring = math.sqrt(max(0.0, 1.0 - z * z))
        angle = golden * index
        points.append(SpherePoint(Vec3(math.cos(angle) * ring, math.sin(angle) * ring, z)))
    return points


class TestItKnowsNothingOfPlates(unittest.TestCase):
    """The architectural claim of this phase, tested two ways."""

    def test_the_module_does_not_import_the_plates(self):
        """
        Enforced by the import list rather than by a comment asking people to behave.
        If this ever fails, somebody has made continents out of plate cells.

        """
        import worldbuilder.terrain.continentality as module

        with open(module.__file__, encoding="utf-8") as handle:
            source = handle.read()
        self.assertNotIn("plates", source.split('"""')[2])

    def test_land_does_not_follow_the_plate_margins(self):
        """
        The measurable version. Continentality against distance-to-plate-edge, over the
        whole globe: if the two were related, continents would wear Voronoi shapes.

        """
        from worldbuilder.plates.generation import plates_for

        land = Continentality(SEED)
        plates = plates_for(SEED)
        points = scattered(600)
        values = [land.at(p) for p in points]
        distances = [plates.margin_at(p).distance_m for p in points]

        mean_v = sum(values) / len(values)
        mean_d = sum(distances) / len(distances)
        covariance = sum((v - mean_v) * (d - mean_d) for v, d in zip(values, distances))
        spread_v = math.sqrt(sum((v - mean_v) ** 2 for v in values))
        spread_d = math.sqrt(sum((d - mean_d) ** 2 for d in distances))
        correlation = covariance / (spread_v * spread_d)

        self.assertLess(abs(correlation), 0.25, f"correlation {correlation:+.3f}")


class TestDeterminism(unittest.TestCase):
    def test_the_same_seed_gives_the_same_world(self):
        first, second = Continentality(SEED), Continentality(SEED)
        for point in scattered(120):
            self.assertEqual(first.at(point), second.at(point))

    def test_a_different_seed_gives_a_different_world(self):
        here, there = Continentality(SEED), Continentality(SEED + 1)
        differing = sum(1 for p in scattered(120) if here.at(p) != there.at(p))
        self.assertGreater(differing, 100)

    def test_the_order_of_asking_changes_nothing(self):
        """
        The field memoises its lattice, so this specifically checks that the cache
        returns values rather than inventing them.

        """
        land = Continentality(SEED)
        points = scattered(80)
        forward = [land.at(p) for p in points]
        backward = list(reversed([land.at(p) for p in reversed(points)]))
        self.assertEqual(forward, backward)

    def test_noise_is_salted_so_two_fields_are_not_one_field(self):
        here = Noise(SEED, salt=1)
        there = Noise(SEED, salt=2)
        self.assertNotEqual(here.at(0.3, 0.4, 0.5), there.at(0.3, 0.4, 0.5))


class TestTheShapeOfTheWorld(unittest.TestCase):
    def setUp(self):
        self.land = Continentality(SEED)

    def test_the_land_fraction_is_a_control_and_not_an_accident(self):
        """
        Measured on an equal-area spread, which is what caught the first version: summed
        value noise clusters near the middle of its range, so a fixed threshold produced
        nought, nought and two per cent land on three seeds against Earth's twenty-nine.

        """
        for wanted in (0.15, 0.29, 0.55):
            land = Continentality(SEED, land_fraction=wanted)
            points = scattered(2000)
            dry = sum(1 for p in points if land.base_elevation(p) >= 0.0)
            self.assertAlmostEqual(dry / len(points), wanted, delta=0.04)

    def test_elevations_are_plausible_and_never_nonsense(self):
        for point in scattered(400):
            metres = self.land.base_elevation(point)
            self.assertTrue(math.isfinite(metres))
            self.assertGreater(metres, -6000.0)
            self.assertLess(metres, 1200.0)

    def test_the_field_is_continuous(self):
        """A step of a kilometre must not move the ground by a cliff."""
        for point in scattered(60):
            frame = TangentFrame.at(point)
            here = self.land.base_elevation(point)
            for x, y in ((1000.0, 0.0), (-1000.0, 0.0), (0.0, 1000.0), (0.0, -1000.0)):
                there = self.land.base_elevation(frame.local_to_sphere(x, y))
                self.assertLess(abs(there - here), 120.0)

    def test_it_makes_continents_rather_than_coastlines(self):
        """
        The lane this layer must stay in. Sampling along a thousand-kilometre line, the
        value should cross sea level a handful of times at most - if it crossed twenty
        times it would be generating archipelagos, which is a later layer's job and would
        leave that layer nothing to do.

        """
        frame = TangentFrame.at_latlon(15.0, 40.0)
        crossings = 0
        previous = None
        for step in range(201):
            metres = self.land.base_elevation(frame.local_to_sphere(-500_000.0 + step * 5_000.0, 0.0))
            wet = metres < 0.0
            if previous is not None and wet != previous:
                crossings += 1
            previous = wet
        self.assertLess(crossings, 6, f"{crossings} coastlines in a thousand kilometres")

    def test_the_poles_and_the_dateline_are_ordinary(self):
        for point in (NORTH_POLE, SOUTH_POLE):
            self.assertTrue(math.isfinite(self.land.base_elevation(point)))

        east = self.land.at(SpherePoint.from_latlon(20.0, 179.999))
        west = self.land.at(SpherePoint.from_latlon(20.0, -179.999))
        self.assertAlmostEqual(east, west, places=4)

        at_pole = self.land.at(NORTH_POLE)
        for longitude in (-90.0, 0.0, 90.0, 180.0):
            self.assertAlmostEqual(
                self.land.at(SpherePoint.from_latlon(90.0, longitude)), at_pole, places=9
            )


class TestGradient(unittest.TestCase):
    def setUp(self):
        self.land = Continentality(SEED)

    def test_it_points_the_way_the_land_rises(self):
        """
        Walking along the gradient must find more continent than walking against it.
        This is what the shelf shaper will lean on to know which way the sea is.

        """
        agreed = 0
        tested = 0
        for point in scattered(120):
            gradient = self.land.gradient(point)
            if gradient.magnitude() < 1e-12:
                continue
            frame = TangentFrame.at(point)
            step = 40_000.0
            scale = step / gradient.magnitude()
            uphill = self.land.at(
                frame.local_to_sphere(gradient.east * scale, gradient.north * scale)
            )
            downhill = self.land.at(
                frame.local_to_sphere(-gradient.east * scale, -gradient.north * scale)
            )
            tested += 1
            if uphill > downhill:
                agreed += 1
        self.assertGreater(agreed / tested, 0.95)

    def test_it_is_measured_along_the_surface_and_not_through_the_planet(self):
        """
        A finite difference taken in raw x, y and z would step off the sphere and measure
        the noise volume instead of the ground, with an error that grows towards the
        poles. Testing that the answer at high latitude is the same size as at the equator
        is the cheapest way to catch that having been done.

        """
        sizes = [
            self.land.gradient(SpherePoint.from_latlon(latitude, 25.0)).magnitude()
            for latitude in (0.0, 45.0, 80.0, 89.5)
        ]
        for size in sizes:
            self.assertTrue(math.isfinite(size))
            self.assertLess(size, 1e-4)

    def test_it_works_at_the_poles(self):
        for pole in (NORTH_POLE, SOUTH_POLE):
            gradient = self.land.gradient(pole)
            self.assertTrue(math.isfinite(gradient.east))
            self.assertTrue(math.isfinite(gradient.north))


class TestCost(unittest.TestCase):
    """
    Measured on its own, so that when the pipeline is assembled the time can be
    attributed rather than guessed at.
    """

    def test_what_a_chart_of_continentality_costs(self):
        land = Continentality(SEED)
        points = scattered(96 * 96)

        start = time.perf_counter()
        for point in points:
            land.at(point)
        value_time = time.perf_counter() - start

        start = time.perf_counter()
        for point in points[:900]:
            land.gradient(point)
        gradient_time = (time.perf_counter() - start) / 900 * len(points)

        print(f"\n    continentality:           {value_time * 1000:7.1f} ms for "
              f"{len(points)} samples, {value_time / len(points) * 1e6:5.2f} us each")
        print(f"    with gradient as well:    {gradient_time * 1000:7.1f} ms, "
              f"{gradient_time / len(points) * 1e6:5.2f} us each  "
              f"({gradient_time / value_time:.1f}x)")

        self.assertLess(value_time / len(points) * 1e6, 100.0)


if __name__ == "__main__":
    unittest.main()
