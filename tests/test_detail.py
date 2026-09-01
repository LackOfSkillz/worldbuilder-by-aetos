"""
Tests for roughness, and for the fact that it is only roughness.

Two dangers in this phase, and both are about a layer exceeding its remit.

**Detail must not become structure.** It is very tempting to let noise make coves, shoals
and little islands, and the result would be navigational hazards that are accidents of the
noise spectrum rather than things somebody put there. M1.7 makes features; this makes
texture.

**Band-limiting must not become a cliff in resolution.** Dropping an octave the moment it
stops being representable would make the ground jump as a player zoomed - the same bug
M1.4 kept producing, moved into a different axis.
"""

import math
import time
import unittest

from worldbuilder.geometry.sphere import SpherePoint
from worldbuilder.geometry.tangent import TangentFrame
from worldbuilder.geometry.vectors import Vec3
from worldbuilder.terrain.detail import CANONICAL_WAVELENGTH_M, COARSEST_WAVELENGTH_M
from worldbuilder.terrain.surface import Surface

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


class SurfaceTestCase(unittest.TestCase):
    def setUp(self):
        self.world = Surface(SEED)


class TestCanonicalIsFixed(SurfaceTestCase):
    def test_canonical_is_the_same_before_and_after_any_other_query(self):
        """
        Physics asks canonically, so a rock must be where it is regardless of how anybody
        happens to be looking at the water around it.

        """
        points = scattered(80)
        before = [self.world.elevation_m(p) for p in points]
        for resolution in (50.0, 500.0, 5_000.0, 50_000.0):
            for point in points:
                self.world.elevation_m(point, resolution)
        after = [self.world.elevation_m(p) for p in points]
        self.assertEqual(before, after)

    def test_the_order_of_asking_changes_nothing(self):
        points = scattered(60)
        forward = [self.world.elevation_m(p) for p in points]
        backward = list(reversed([self.world.elevation_m(p) for p in reversed(points)]))
        self.assertEqual(forward, backward)

    def test_a_fine_enough_resolution_is_the_canonical_answer(self):
        """
        Below the canonical wavelength there is nothing left to reveal, so asking more
        finely than the world is defined must change nothing.

        """
        for point in scattered(60):
            self.assertAlmostEqual(
                self.world.elevation_m(point, CANONICAL_WAVELENGTH_M / 8.0),
                self.world.elevation_m(point),
                places=6,
            )


class TestBandLimiting(SurfaceTestCase):
    def test_coarser_sampling_removes_fine_detail_monotonically(self):
        """
        Each step coarser must be at least as smooth as the last - never rougher, which
        would mean an octave coming *back* as resolution dropped.

        """
        points = scattered(200)
        structural = [self.world.structural_m(p) for p in points]

        roughness = []
        for resolution in (None, 300.0, 1_000.0, 4_000.0, 20_000.0, 80_000.0):
            departures = [
                abs(self.world.elevation_m(p, resolution) - s)
                for p, s in zip(points, structural)
            ]
            roughness.append(sum(departures) / len(departures))

        for finer, coarser in zip(roughness, roughness[1:]):
            self.assertLessEqual(coarser, finer + 1e-9)

    def test_very_coarse_sampling_leaves_structure_alone(self):
        for point in scattered(150):
            self.assertAlmostEqual(
                self.world.elevation_m(point, COARSEST_WAVELENGTH_M * 4.0),
                self.world.structural_m(point),
                delta=1.0,
            )

    def test_octaves_fade_rather_than_switch_off(self):
        """
        The cliff this phase could have produced, in resolution rather than position.
        Sweeping the resolution smoothly must move the ground smoothly: if an octave
        vanished the instant it became unrepresentable, the terrain would jump as a
        player zoomed.

        """
        for point in scattered(40):
            previous = None
            resolution = 60.0
            while resolution < 60_000.0:
                value = self.world.elevation_m(point, resolution)
                if previous is not None:
                    self.assertLess(abs(value - previous), 12.0)
                previous = value
                resolution *= 1.12

    def test_structure_never_thins_out(self):
        """
        Only detail is resolution-aware. If the shelf or the tectonics faded with zoom, a
        wide chart would show a different world rather than a generalised one.

        """
        for point in scattered(120):
            structural = self.world.structural_m(point)
            for resolution in (None, 1_000.0, 100_000.0):
                self.assertEqual(self.world.structural_m(point), structural)


class TestDetailStaysInItsPlace(SurfaceTestCase):
    def test_it_does_not_make_land(self):
        """
        The remit. Detail may roughen a shelf; it may not turn twenty metres of water into
        an island, because then every hazard on the chart is an accident of a noise
        spectrum rather than something somebody put there.

        """
        points = scattered(3000)
        structural_land = sum(1 for p in points if self.world.structural_m(p) >= 0.0)
        final_land = sum(1 for p in points if self.world.elevation_m(p) >= 0.0)
        self.assertLess(abs(final_land - structural_land) / len(points), 0.02)

    def test_it_is_quiet_on_the_shelf(self):
        """Where a ship sails, roughness is texture and never topography."""
        checked = 0
        for point in scattered(2500):
            structural = self.world.structural_m(point)
            if not (-140.0 < structural < -20.0):
                continue
            departure = abs(self.world.elevation_m(point) - structural)
            self.assertLess(departure, 45.0)
            checked += 1
        self.assertGreater(checked, 20)

    def test_a_trench_stays_legible(self):
        deepest = 0.0
        at = None
        for point in scattered(2500):
            offset = self.world.tectonics.offset_m(point)
            if offset < deepest:
                deepest, at = offset, point
        self.assertLess(deepest, -800.0)
        structural = self.world.structural_m(at)
        self.assertLess(abs(self.world.elevation_m(at) - structural), abs(structural) * 0.2)

    def test_nothing_is_ever_nonsense(self):
        for point in scattered(1500) + [
            SpherePoint.from_latlon(90.0, 0.0),
            SpherePoint.from_latlon(-90.0, 0.0),
        ]:
            for resolution in (None, 400.0, 9_000.0):
                metres = self.world.elevation_m(point, resolution)
                self.assertTrue(math.isfinite(metres))
                self.assertGreater(metres, -7500.0)
                self.assertLess(metres, 2500.0)


class TestSeams(SurfaceTestCase):
    def test_the_dateline(self):
        east = self.world.elevation_m(SpherePoint.from_latlon(22.0, 179.999))
        west = self.world.elevation_m(SpherePoint.from_latlon(22.0, -179.999))
        self.assertLess(abs(east - west), 40.0)

    def test_the_poles(self):
        for latitude in (90.0, -90.0):
            at_pole = self.world.elevation_m(SpherePoint.from_latlon(latitude, 0.0))
            for longitude in (-90.0, 37.0, 180.0):
                self.assertAlmostEqual(
                    self.world.elevation_m(SpherePoint.from_latlon(latitude, longitude)),
                    at_pole,
                    places=6,
                )

    def test_detail_does_not_stretch_with_latitude(self):
        """
        Sampled in three dimensions on the sphere, so there is no projection to stretch.
        A two-dimensional field would show visibly coarser texture near the poles.

        """
        sizes = []
        for latitude in (0.0, 40.0, 75.0):
            frame = TangentFrame.at_latlon(latitude, 30.0)
            departures = []
            for step in range(60):
                point = frame.local_to_sphere(step * 900.0, 0.0)
                departures.append(
                    abs(self.world.elevation_m(point) - self.world.structural_m(point))
                )
            sizes.append(sum(departures) / len(departures))
        self.assertLess(max(sizes), min(sizes) * 4.0 + 5.0)


class TestAMovingChartIsStable(SurfaceTestCase):
    def test_shifting_the_grid_half_a_cell_does_not_remake_the_coast(self):
        """
        A chart's grid is centred on the ship, so it moves as she sails. Two grids offset
        by half a cell must describe the same coast - otherwise the chart would redraw
        itself differently every few seconds, which is the moving-grid problem that puts
        isolated hazards in the marks layer rather than in the terrain.

        """
        frame = TangentFrame.at_latlon(-58.5, 117.9)
        spacing = 2_000.0
        for offset in (0.0, spacing * 0.5):
            pass
        first = [
            self.world.elevation_m(frame.local_to_sphere(i * spacing, 0.0), spacing)
            for i in range(60)
        ]
        second = [
            self.world.elevation_m(
                frame.local_to_sphere(i * spacing + spacing * 0.5, 0.0), spacing
            )
            for i in range(60)
        ]
        for a, b in zip(first, second):
            self.assertLess(abs(a - b), 60.0)


class TestCost(SurfaceTestCase):
    def test_coarse_charts_are_cheaper_than_canonical(self):
        points = scattered(96 * 96)
        timings = {}
        for name, resolution in (
            ("canonical", None), ("100 m", 100.0), ("500 m", 500.0),
            ("2 km", 2_000.0), ("10 km", 10_000.0),
        ):
            start = time.perf_counter()
            for point in points:
                self.world.elevation_m(point, resolution)
            timings[name] = time.perf_counter() - start

        print("\n    a 96 x 96 chart, whole pipeline:")
        for name, took in timings.items():
            print(f"      {name:10} {took * 1000:7.1f} ms   "
                  f"{took / len(points) * 1e6:5.2f} us a sample")

        self.assertLess(timings["10 km"], timings["canonical"])


if __name__ == "__main__":
    unittest.main()
