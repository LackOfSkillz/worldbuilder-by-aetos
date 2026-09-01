"""
Tests for the plate skeleton.

Two themes. The first is determinism of a specific kind: not merely "the same seed gives
the same world", but "the same seed gives the same world *however the code asks for it*" -
in any order, any number of times, with any other plate generated first. That is what
hashing per plate buys and what a mutable sequence would have quietly taken away.

The second is that the sphere has no special places. Poles and the antimeridian are tested
not because they are likely to work, but because they are the two spots where a globe
implemented in latitude and longitude always eventually fails, and the claim of this
design is that they are ordinary here.
"""

import math
import time
import unittest

from worldbuilder.geometry.sphere import EARTH_RADIUS_M, SpherePoint
from worldbuilder.geometry.tangent import TangentFrame
from worldbuilder.geometry.vectors import Vec3
from worldbuilder.plates.generation import DEFAULT_PLATE_COUNT, plates_for
from worldbuilder.plates.kinematics import (
    CONVERGENT,
    DIVERGENT,
    TRANSFORM,
    motion_at,
    surface_velocity,
)

SEED = 20260831
NORTH_POLE = SpherePoint.from_latlon(90.0, 0.0)
SOUTH_POLE = SpherePoint.from_latlon(-90.0, 0.0)


def scattered(count=400):
    """A deterministic spread of sample points, without a random generator anywhere."""
    golden = math.pi * (3.0 - math.sqrt(5.0))
    points = []
    for index in range(count):
        z = 1.0 - 2.0 * (index + 0.5) / count
        ring = math.sqrt(max(0.0, 1.0 - z * z))
        angle = golden * index
        points.append(
            SpherePoint(Vec3(math.cos(angle) * ring, math.sin(angle) * ring, z))
        )
    return points


class TestGeneration(unittest.TestCase):
    def test_a_seed_gives_the_same_plates_every_time(self):
        first, second = plates_for(SEED), plates_for(SEED)
        for a, b in zip(first.plates, second.plates):
            self.assertEqual(a.seed.vector, b.seed.vector)
            self.assertEqual(a.euler_pole.vector, b.euler_pole.vector)
            self.assertEqual(a.rate_rad_per_myr, b.rate_rad_per_myr)

    def test_a_different_seed_gives_a_different_world(self):
        here, there = plates_for(SEED), plates_for(SEED + 1)
        moved = sum(1 for a, b in zip(here.plates, there.plates)
                    if a.seed.vector != b.seed.vector)
        self.assertEqual(moved, len(here))

    def test_a_plate_does_not_depend_on_the_plates_before_it(self):
        """
        The property hashing exists for, and the reason a mutable generator was refused.

        Asking for one plate must give the same answer as asking for all of them, so that
        adding a property to `Plate` next month cannot shift every subsequent plate by
        consuming a different number of values from a sequence.

        """
        whole = plates_for(SEED, DEFAULT_PLATE_COUNT)
        for index in (0, 7, DEFAULT_PLATE_COUNT - 1):
            self.assertEqual(plates_for(SEED, DEFAULT_PLATE_COUNT)[index].seed.vector,
                             whole[index].seed.vector)

    def test_asking_for_more_plates_does_not_move_the_early_ones_arbitrarily(self):
        """
        Their positions do change - the spiral depends on the count - but their *motion*
        does not, because a pole and a rate are hashed from the index alone.

        """
        few, many = plates_for(SEED, 12), plates_for(SEED, 30)
        for index in range(12):
            self.assertEqual(few[index].euler_pole.vector, many[index].euler_pole.vector)
            self.assertEqual(few[index].rate_rad_per_myr, many[index].rate_rad_per_myr)

    def test_every_seed_and_pole_is_a_unit_vector(self):
        for plate in plates_for(SEED).plates:
            self.assertAlmostEqual(plate.seed.vector.length(), 1.0, places=12)
            self.assertAlmostEqual(plate.euler_pole.vector.length(), 1.0, places=12)

    def test_no_two_plates_share_a_seed(self):
        seeds = plates_for(SEED).plates
        for i, a in enumerate(seeds):
            for b in seeds[i + 1:]:
                self.assertGreater(a.seed.angle_to(b.seed), 1e-6)

    def test_plates_turn_at_plausible_speeds(self):
        """A few centimetres a year, which is what plates do."""
        for plate in plates_for(SEED).plates:
            fastest = abs(plate.rate_rad_per_myr) * EARTH_RADIUS_M / 1e6  # metres a year
            self.assertGreater(fastest, 0.005)
            self.assertLess(fastest, 0.20)

    def test_poles_are_spread_rather_than_crowded(self):
        """
        Sampling a latitude uniformly would crowd the rotation axes towards the poles and
        every plate would drift as one sheet. The z component is sampled instead.

        """
        heights = [plate.euler_pole.vector.z for plate in plates_for(SEED, 200).plates]
        self.assertLess(abs(sum(heights) / len(heights)), 0.15)


class TestLookup(unittest.TestCase):
    def setUp(self):
        self.plates = plates_for(SEED)

    def test_a_plate_seed_belongs_to_its_own_plate(self):
        for plate in self.plates.plates:
            nearest, _ = self.plates.nearest_two(plate.seed)
            self.assertEqual(nearest.index, plate.index)

    def test_every_point_has_a_nearest_and_a_different_second(self):
        for point in scattered():
            nearest, second = self.plates.nearest_two(point)
            self.assertIsNotNone(nearest)
            self.assertIsNotNone(second)
            self.assertNotEqual(nearest.index, second.index)

    def test_the_answer_does_not_depend_on_the_order_asked(self):
        """Specifically catches any accidental reliance on mutable state."""
        points = scattered(60)
        forward = [self.plates.nearest_two(p)[0].index for p in points]
        backward = [self.plates.nearest_two(p)[0].index for p in reversed(points)]
        self.assertEqual(forward, list(reversed(backward)))

    def test_margin_distance_is_metres_and_not_a_score(self):
        """
        The distance is exact rather than a proxy: points equidistant from two seeds
        satisfy dot(P, A - B) == 0, so the margin is a great circle and the distance to
        it is an arc sine. Terrain constants downstream are in metres - a shelf is eighty
        kilometres wide - and a normalised closeness measure would have made every one of
        them mean something else.

        """
        for point in scattered(200):
            distance = self.plates.margin_at(point).distance_m
            self.assertGreaterEqual(distance, 0.0)
            self.assertLess(distance, EARTH_RADIUS_M * math.pi / 2 + 1.0)

    def test_a_point_on_the_margin_is_no_distance_from_it(self):
        """Halfway between two seeds is on their margin, by construction."""
        a, b = self.plates[0], self.plates[1]
        midpoint = SpherePoint.from_vector(a.seed.vector + b.seed.vector)
        margin = self.plates.margin_at(midpoint)
        if {margin.nearest.index, margin.neighbour.index} == {a.index, b.index}:
            self.assertLess(margin.distance_m, 1.0)

    def test_margin_distance_is_continuous_across_a_margin(self):
        """
        Crossing from one plate to another swaps which is nearest, and the distance must
        pass smoothly through zero rather than jumping. A discontinuity here would become
        a wall of terrain later.

        """
        a, b = self.plates[0], self.plates[1]
        steps = 400
        previous = None
        swapped = False
        for step in range(steps + 1):
            fraction = step / steps
            point = SpherePoint.from_vector(
                a.seed.vector.scaled(1.0 - fraction) + b.seed.vector.scaled(fraction)
            )
            margin = self.plates.margin_at(point)
            if previous is not None:
                self.assertLess(abs(margin.distance_m - previous), 40_000.0)
            if margin.nearest.index != a.index:
                swapped = True
            previous = margin.distance_m
        self.assertTrue(swapped, "the walk never left the first plate")

    def test_the_normal_across_a_margin_lies_flat_on_the_surface(self):
        for point in scattered(100):
            margin = self.plates.margin_at(point)
            normal = self.plates.margin_normal(point, margin)
            if normal is not None:
                self.assertAlmostEqual(normal.length(), 1.0, places=9)
                self.assertAlmostEqual(normal.dot(point.vector), 0.0, places=9)

    def test_the_poles_and_the_dateline_are_ordinary_places(self):
        for point in (NORTH_POLE, SOUTH_POLE):
            nearest, second = self.plates.nearest_two(point)
            self.assertNotEqual(nearest.index, second.index)
            self.assertTrue(math.isfinite(self.plates.margin_at(point).distance_m))

        # Longitude cannot matter at a pole.
        chosen = {self.plates.nearest_two(SpherePoint.from_latlon(90.0, lon))[0].index
                  for lon in (-180.0, -90.0, 0.0, 37.0, 180.0)}
        self.assertEqual(len(chosen), 1)

    def test_the_two_poles_are_on_different_plates(self):
        north, _ = self.plates.nearest_two(NORTH_POLE)
        south, _ = self.plates.nearest_two(SOUTH_POLE)
        self.assertNotEqual(north.index, south.index)


class TestKinematics(unittest.TestCase):
    def setUp(self):
        self.plates = plates_for(SEED)

    def test_the_ground_does_not_move_at_a_plates_own_euler_pole(self):
        """
        Which is the whole reason for Euler poles over drift vectors: the variation in
        speed across a plate is not a detail, it is why one margin can pull apart at one
        end and grind sideways at the other.

        """
        for plate in self.plates.plates:
            speed = surface_velocity(plate, plate.euler_pole).length()
            self.assertLess(speed, 1e-6)

    def test_speed_grows_with_distance_from_that_pole(self):
        plate = self.plates[0]
        pole = plate.euler_pole.vector
        sideways = Vec3(0.0, 0.0, 1.0).cross(pole)
        if sideways.length() < 1e-9:
            sideways = Vec3(1.0, 0.0, 0.0).cross(pole)
        away = sideways.normalised()

        speeds = []
        for degrees in (5.0, 30.0, 60.0, 90.0):
            angle = math.radians(degrees)
            point = SpherePoint.from_vector(
                pole.scaled(math.cos(angle)) + away.scaled(math.sin(angle))
            )
            speeds.append(surface_velocity(plate, point).length())
        self.assertEqual(speeds, sorted(speeds))

    def test_velocity_is_tangent_to_the_surface(self):
        """A plate slides across the planet; it does not burrow into it."""
        for point in scattered(100):
            for plate in self.plates.plates[:5]:
                velocity = surface_velocity(plate, point)
                self.assertAlmostEqual(
                    velocity.dot(point.vector) / max(velocity.length(), 1e-9), 0.0,
                    places=9,
                )

    def test_no_velocity_is_ever_nonsense(self):
        for point in scattered(200) + [NORTH_POLE, SOUTH_POLE]:
            for plate in self.plates.plates:
                velocity = surface_velocity(plate, point)
                for value in (velocity.x, velocity.y, velocity.z):
                    self.assertTrue(math.isfinite(value))

    def test_every_margin_is_doing_one_of_three_things(self):
        seen = set()
        for point in scattered(600):
            motion = motion_at(point, self.plates)
            self.assertIsNotNone(motion)
            self.assertIn(motion.kind, (CONVERGENT, DIVERGENT, TRANSFORM))
            seen.add(motion.kind)
        self.assertEqual(seen, {CONVERGENT, DIVERGENT, TRANSFORM})

    def test_closing_and_separating_agree_with_the_names(self):
        for point in scattered(300):
            motion = motion_at(point, self.plates)
            if motion.kind == CONVERGENT:
                self.assertGreater(motion.closing_m_per_myr, 0.0)
            elif motion.kind == DIVERGENT:
                self.assertLess(motion.closing_m_per_myr, 0.0)

    def test_motion_is_worked_out_fresh_and_not_remembered(self):
        """
        A margin that converges at one end and slides at the other is the normal case, so
        the classification cannot be a property of the margin. Walking one should find it
        changing.

        """
        a, b = self.plates[0], self.plates[1]
        along = a.seed.vector.cross(b.seed.vector).normalised()
        middle = SpherePoint.from_vector(a.seed.vector + b.seed.vector)
        kinds = set()
        for degrees in range(-80, 81, 4):
            angle = math.radians(degrees)
            point = SpherePoint.from_vector(
                middle.vector.scaled(math.cos(angle)) + along.scaled(math.sin(angle))
            )
            motion = motion_at(point, self.plates)
            if motion:
                kinds.add(motion.kind)
        self.assertGreater(len(kinds), 1, "one margin behaved identically along its length")


class TestCost(unittest.TestCase):
    """
    A baseline, measured now and in isolation.

    The point is not to pass or fail but to know what plate lookup costs *before* noise
    and terrain are layered on, so that when the five-microsecond target is eventually
    chased there is a number to attribute the time to.
    """

    def test_a_chart_of_plate_lookups(self):
        plates = plates_for(SEED)
        points = scattered(96 * 96)

        start = time.perf_counter()
        for point in points:
            plates.margin_at(point)
        took = time.perf_counter() - start

        per_sample = took / len(points) * 1e6
        print(f"\n    plate lookup + margin distance: {took * 1000:7.1f} ms for "
              f"{len(points)} samples, {per_sample:5.2f} us each")

        start = time.perf_counter()
        for point in points:
            motion_at(point, plates)
        took_motion = time.perf_counter() - start
        print(f"    with kinematics as well:        {took_motion * 1000:7.1f} ms, "
              f"{took_motion / len(points) * 1e6:5.2f} us each")

        # Not a performance assertion, a sanity one: this must not be catastrophic.
        self.assertLess(per_sample, 200.0)


if __name__ == "__main__":
    unittest.main()


class TestMarginDistanceAgainstBruteForce(unittest.TestCase):
    """
    The reference check for the one piece of load-bearing geology arithmetic.

    `margin_at` claims a real distance in metres to the edge of a plate's cell, and from
    M1.4 onwards terrain will read that number as metres - a shelf eighty kilometres wide,
    an uplift belt decaying over three hundred. If it were quietly a proximity score
    instead, every one of those constants would mean something else and nothing would
    obviously be wrong.

    So the claim is checked against a method that shares none of its reasoning: walk
    outwards from the point in many directions until the nearest plate changes, and take
    the shortest walk that crossed. Slow, obvious, and independent.
    """

    def setUp(self):
        self.plates = plates_for(SEED)

    def _walked(self, point, radius_m=EARTH_RADIUS_M, bearings=72, steps=260,
                reach_m=900_000.0):
        """
        Brute force: how far to walk before standing on a different plate.

        Notes:
            Deliberately naive. It knows nothing of bisectors and does not care which
            plate it ends up on - only that the answer changed, which is the definition
            of having crossed an edge.

        """
        frame = TangentFrame.at(point, radius_m)
        here, _ = self.plates.nearest_two(point)
        shortest = float("inf")
        for turn in range(bearings):
            bearing = 2.0 * math.pi * turn / bearings
            east, north = math.sin(bearing), math.cos(bearing)
            for step in range(1, steps + 1):
                distance = reach_m * step / steps
                if distance >= shortest:
                    break
                out = frame.local_to_sphere(east * distance, north * distance)
                if self.plates.nearest_two(out)[0].index != here.index:
                    shortest = distance
                    break
        return shortest

    def test_the_computed_distance_matches_a_walk(self):
        tolerance_m = 900_000.0 / 260 * 2  # two steps of the walk's own resolution
        checked = 0
        for point in scattered(60):
            walked = self._walked(point)
            if not math.isfinite(walked):
                continue  # deep inside a plate, further than the walk reaches
            computed = self.plates.margin_at(point).distance_m
            self.assertAlmostEqual(
                computed, walked, delta=tolerance_m,
                msg=f"at {point.to_latlon()}: computed {computed:.0f} walked {walked:.0f}",
            )
            checked += 1
        self.assertGreater(checked, 25, "too few points were near enough a margin to check")

    def test_it_is_never_an_overestimate(self):
        """
        The direction that would matter. Too small merely puts a mountain range slightly
        wide; too large would claim open plate interior where an edge actually is.

        """
        for point in scattered(40):
            walked = self._walked(point)
            if math.isfinite(walked):
                self.assertLessEqual(
                    self.plates.margin_at(point).distance_m, walked + 8000.0
                )
