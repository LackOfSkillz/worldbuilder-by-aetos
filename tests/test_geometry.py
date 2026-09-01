"""
Tests for the spherical geometry everything else stands on.

Most of these are about the two places a globe goes wrong: the antimeridian, where the
numbers wrap, and the poles, where the numbers stop meaning anything. Both are handled by
representing a position as a unit vector rather than as a latitude and longitude, and the
point of testing them is to prove that claim rather than to repeat it.

The last test is not a pass-or-fail check but a measurement: how wrong a flat chart is at
increasing range, which is where the maximum region size comes from.
"""

import math
import unittest

from worldbuilder.debug.projection_error import measure
from worldbuilder.geometry.sphere import EARTH_RADIUS_M, SpherePoint
from worldbuilder.geometry.tangent import TangentFrame
from worldbuilder.geometry.vectors import Vec3

NORTH_POLE = SpherePoint.from_latlon(90.0, 0.0)
SOUTH_POLE = SpherePoint.from_latlon(-90.0, 0.0)


class TestVectors(unittest.TestCase):
    def test_a_normalised_vector_has_unit_length(self):
        self.assertAlmostEqual(Vec3(3.0, -4.0, 12.0).normalised().length(), 1.0, places=12)

    def test_a_vector_of_no_length_has_no_direction_to_keep(self):
        with self.assertRaises(ValueError):
            Vec3(0.0, 0.0, 0.0).normalised()

    def test_a_cross_product_is_at_right_angles_to_both(self):
        first, second = Vec3(1.0, 2.0, 3.0), Vec3(-2.0, 0.5, 1.0)
        product = first.cross(second)
        self.assertAlmostEqual(product.dot(first), 0.0, places=12)
        self.assertAlmostEqual(product.dot(second), 0.0, places=12)


class TestPositions(unittest.TestCase):
    def test_every_point_is_a_unit_vector(self):
        for lat in (-90.0, -63.4, 0.0, 12.5, 90.0):
            for lon in (-180.0, -73.2, 0.0, 61.3, 180.0):
                point = SpherePoint.from_latlon(lat, lon)
                self.assertAlmostEqual(point.vector.length(), 1.0, places=12)

    def test_latitude_and_longitude_survive_the_round_trip(self):
        for lat, lon in ((0.0, 0.0), (51.5, -0.13), (-33.9, 151.2), (78.2, 15.6)):
            back_lat, back_lon = SpherePoint.from_latlon(lat, lon).to_latlon()
            self.assertAlmostEqual(back_lat, lat, places=9)
            self.assertAlmostEqual(back_lon, lon, places=9)

    def test_the_antimeridian_is_one_place_and_not_two(self):
        """
        The bug this design exists to prevent. 180 east and 180 west name the same
        meridian, and so does 540; a representation that made them different numbers
        would need every caller to remember that, and one of them would forget.

        """
        east = SpherePoint.from_latlon(12.0, 180.0)
        west = SpherePoint.from_latlon(12.0, -180.0)
        again = SpherePoint.from_latlon(12.0, 540.0)
        for other in (west, again):
            self.assertAlmostEqual(east.angle_to(other), 0.0, places=12)

    def test_either_side_of_the_dateline_is_a_short_step(self):
        just_east = SpherePoint.from_latlon(5.0, 179.999)
        just_west = SpherePoint.from_latlon(5.0, -179.999)
        self.assertLess(just_east.distance_to(just_west), 500.0)

    def test_the_poles_are_ordinary_points(self):
        self.assertAlmostEqual(NORTH_POLE.vector.z, 1.0, places=12)
        self.assertAlmostEqual(SOUTH_POLE.vector.z, -1.0, places=12)
        self.assertAlmostEqual(NORTH_POLE.to_latlon()[0], 90.0, places=9)
        self.assertAlmostEqual(SOUTH_POLE.to_latlon()[0], -90.0, places=9)

    def test_longitude_does_not_matter_at_a_pole(self):
        """Every meridian meets there, so naming one cannot change the place."""
        for lon in (-180.0, -90.0, 0.0, 37.0, 180.0):
            self.assertAlmostEqual(NORTH_POLE.angle_to(SpherePoint.from_latlon(90.0, lon)),
                                   0.0, places=9)

    def test_distances_are_the_ones_a_navigator_would_recognise(self):
        # A quarter of the way round the planet, pole to equator.
        self.assertAlmostEqual(
            NORTH_POLE.distance_to(SpherePoint.from_latlon(0.0, 0.0)),
            EARTH_RADIUS_M * math.pi / 2,
            delta=1.0,
        )
        # And all the way round, pole to pole.
        self.assertAlmostEqual(
            NORTH_POLE.distance_to(SOUTH_POLE), EARTH_RADIUS_M * math.pi, delta=1.0
        )

    def test_a_point_is_no_distance_from_itself(self):
        """
        Guards the precision trap this deliberately avoids. Taking the arc cosine of a
        dot product loses its accuracy for points close together - which is where a ship
        spends its entire life - because the cosine of a small angle is very nearly one.

        """
        here = SpherePoint.from_latlon(23.4, -61.2)
        self.assertEqual(here.angle_to(here), 0.0)
        nearby = SpherePoint.from_latlon(23.400001, -61.2)
        self.assertGreater(here.distance_to(nearby), 0.0)
        self.assertLess(here.distance_to(nearby), 1.0)


class TestFrames(unittest.TestCase):
    def frames(self):
        """Frames worth checking: ordinary, high, and exactly at each pole."""
        return {
            "equator": TangentFrame.at_latlon(0.0, 0.0),
            "temperate": TangentFrame.at_latlon(45.0, -30.0),
            "high": TangentFrame.at_latlon(84.0, 120.0),
            "north pole": TangentFrame.at(NORTH_POLE),
            "south pole": TangentFrame.at(SOUTH_POLE),
        }

    def test_every_basis_is_orthonormal(self):
        for name, frame in self.frames().items():
            for axis in (frame.east, frame.north, frame.up):
                self.assertAlmostEqual(axis.length(), 1.0, places=12, msg=name)
            self.assertAlmostEqual(frame.east.dot(frame.north), 0.0, places=12, msg=name)
            self.assertAlmostEqual(frame.east.dot(frame.up), 0.0, places=12, msg=name)
            self.assertAlmostEqual(frame.north.dot(frame.up), 0.0, places=12, msg=name)

    def test_a_frame_at_a_pole_is_finite_and_the_same_every_time(self):
        """
        East means nothing at a pole - every direction from the north pole is south - so
        a direction is chosen rather than derived. Which one is chosen does not matter.
        That the same one is chosen on every call does: a frame that reshuffled itself
        between two calls would move every ship it held.

        """
        for pole in (NORTH_POLE, SOUTH_POLE):
            first = TangentFrame.at(pole)
            second = TangentFrame.at(pole)
            for axis in ("east", "north", "up"):
                a, b = getattr(first, axis), getattr(second, axis)
                self.assertEqual((a.x, a.y, a.z), (b.x, b.y, b.z))
                for component in (a.x, a.y, a.z):
                    self.assertTrue(math.isfinite(component))

    def test_a_frame_just_off_a_pole_is_still_orthonormal(self):
        """The band where the cross product is losing its nerve but has not yet failed."""
        for latitude in (89.9, 89.999, 89.99999999):
            frame = TangentFrame.at_latlon(latitude, 0.0)
            self.assertAlmostEqual(frame.east.length(), 1.0, places=10)
            self.assertAlmostEqual(frame.east.dot(frame.up), 0.0, places=10)

    def test_the_origin_is_the_middle_of_its_own_chart(self):
        for frame in self.frames().values():
            self.assertEqual(frame.sphere_to_local(frame.origin), (0.0, 0.0))

    def test_local_to_sphere_and_back_returns_where_it_started(self):
        offsets = (0.0, 1.0, -250.0, 12_000.0, -80_000.0, 150_000.0)
        for name, frame in self.frames().items():
            for x in offsets:
                for y in offsets:
                    back_x, back_y = frame.sphere_to_local(frame.local_to_sphere(x, y))
                    self.assertAlmostEqual(back_x, x, places=6, msg=f"{name} x={x} y={y}")
                    self.assertAlmostEqual(back_y, y, places=6, msg=f"{name} x={x} y={y}")

    def test_north_on_the_chart_is_north_on_the_planet(self):
        frame = TangentFrame.at_latlon(10.0, 20.0)
        further_north = frame.local_to_sphere(0.0, 100_000.0)
        self.assertGreater(further_north.to_latlon()[0], 10.0)

    def test_distance_from_the_origin_is_exact(self):
        """
        The defining property of this projection and the reason it was chosen: range and
        bearing from the middle of the chart are right, at any range, by construction.

        """
        frame = TangentFrame.at_latlon(30.0, 45.0)
        for distance in (1_000.0, 50_000.0, 500_000.0):
            point = frame.local_to_sphere(0.0, distance)
            self.assertAlmostEqual(frame.origin.distance_to(point), distance, delta=1e-6)

    def test_a_frame_spanning_the_dateline_notices_nothing(self):
        """A chart centred on the antimeridian is an ordinary chart."""
        frame = TangentFrame.at_latlon(0.0, 179.99)
        east = frame.local_to_sphere(50_000.0, 0.0)
        self.assertAlmostEqual(frame.origin.distance_to(east), 50_000.0, delta=1e-6)
        self.assertLess(east.to_latlon()[1], 0.0)  # it has crossed into the west


class TestHowWrongAFlatChartIs(unittest.TestCase):
    """
    Not a pass or fail so much as the evidence the region size is chosen from.

    The first draft of the spec asserted a maximum region radius with a formula attached.
    This measures it instead.
    """

    def test_range_from_the_origin_never_drifts(self):
        for row in measure():
            self.assertAlmostEqual(row["radial_error_m"], 0.0, places=6)

    def test_error_between_two_points_grows_with_range(self):
        errors = [abs(row["transverse_error_m"]) for row in measure()]
        self.assertEqual(errors, sorted(errors))

    def test_a_two_hundred_kilometre_chart_is_wrong_by_less_than_a_ship(self):
        """
        The tolerance that sets the cap. At 200 km the worst error between two charted
        points is under six metres - a third of a cutter's length, and far below the
        four hundred metres between printed soundings. At 500 km it is 89 m, which is
        several ship-lengths and would show as a bad landfall.

        """
        by_range = {row["range_m"]: abs(row["transverse_error_m"]) for row in measure()}
        self.assertLess(by_range[200_000.0], 20.0)
        self.assertGreater(by_range[500_000.0], 50.0)

    def test_the_projection_does_not_care_about_latitude(self):
        """
        Worth proving rather than assuming: the error at eighty degrees is the error at
        the equator. Doing this in unit vectors means high latitudes are not a special
        case, which is exactly what a latitude-and-longitude implementation could not
        have claimed.

        """
        equator = {r["range_m"]: r["transverse_error_m"] for r in measure(0.0)}
        polar = {r["range_m"]: r["transverse_error_m"] for r in measure(80.0)}
        for distance, error in equator.items():
            self.assertAlmostEqual(polar[distance], error, places=6)


if __name__ == "__main__":
    unittest.main()
