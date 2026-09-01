"""
Tests for the places on a sphere where flat thinking breaks.

The field-level versions of these already exist - `test_detail` and `test_shelf` both cross
the antimeridian and stand on both poles. What is new here is the **assembled** case: a
region, its tangent frame, the features stamped in it and the provider maritime talks to,
all at the coordinates where the geometry is hostile.

That distinction is the whole phase. A continuous elevation field is necessary and nowhere
near sufficient: everything a ship actually uses goes through a projection, and a projection
is exactly the thing that has a pole and a seam. A world can be perfectly smooth and still
put a vessel on the wrong side of the planet when she sails north past eighty-nine degrees.

Three questions, asked at every awkward place:

    does the ground stay continuous       the field, through a frame
    does the frame stay a frame           basis, scale, and round trip
    does a ship get where she is going    the provider, over a track
"""

import math
import unittest

from worldbuilder.bathymetry.features import SHAPE, Feature, Features
from worldbuilder.bathymetry.substrate import ROCK
from worldbuilder.geometry.sphere import EARTH_RADIUS_M, SpherePoint
from worldbuilder.geometry.tangent import TangentFrame
from worldbuilder.geometry.vectors import Vec3
from worldbuilder.integration.maritime import WorldbuilderTerrain
from worldbuilder.regions.demo import WORLD_SEED, Coast, Region
from worldbuilder.terrain.surface import Surface

#: Where a region has to work, and why each one is here.
AWKWARD = (
    ("the equator", 0.0, 0.0),
    ("the antimeridian", 4.0, 180.0),
    ("just west of it", -12.0, -179.98),
    ("the arctic", 78.0, 40.0),
    ("the antarctic", -81.0, -120.0),
    ("all but the north pole", 89.6, 15.0),
    ("all but the south pole", -89.6, -95.0),
    ("the north pole itself", 90.0, 0.0),
    ("the south pole itself", -90.0, 0.0),
)


class Position:
    """What the adapter reads off a maritime position."""

    __slots__ = ("x", "y", "z", "region")

    def __init__(self, x, y, z=0.0, region="default"):
        self.x, self.y, self.z, self.region = float(x), float(y), float(z), region


def scattered(count=400):
    """Points spread evenly over the sphere, as unit vectors rather than degrees."""
    golden = math.pi * (3.0 - math.sqrt(5.0))
    points = []
    for index in range(count):
        z = 1.0 - 2.0 * (index + 0.5) / count
        ring = math.sqrt(max(0.0, 1.0 - z * z))
        angle = golden * index
        points.append(SpherePoint(Vec3(math.cos(angle) * ring, math.sin(angle) * ring, z)))
    return points


def region_at(lat, lon, features=None, reach_m=60_000.0):
    coast = Coast(lat=lat, lon=lon, seaward_deg=0.0)
    return Region(f"{lat},{lon}", coast, reach_m, Features(features or (), EARTH_RADIUS_M))


class GlobalTestCase(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.world = Surface(WORLD_SEED)


class TestAFrameIsAFrameAnywhere(GlobalTestCase):
    def test_local_metres_survive_the_round_trip(self):
        """
        The one property everything else rests on. If a projection does not invert, a
        vessel plotted at a position is not at that position, and no amount of continuous
        terrain underneath will save her.

        """
        for name, lat, lon in AWKWARD:
            frame = TangentFrame.at_latlon(lat, lon)
            for east in (-50_000.0, -1_000.0, 0.0, 1_000.0, 50_000.0):
                for north in (-50_000.0, 0.0, 50_000.0):
                    back = frame.sphere_to_local(frame.local_to_sphere(east, north))
                    self.assertAlmostEqual(back[0], east, places=3, msg=name)
                    self.assertAlmostEqual(back[1], north, places=3, msg=name)

    def test_the_basis_is_orthonormal_everywhere(self):
        """
        At a pole every direction is south and none is east, so the cross product that
        defines east goes to zero and a fixed reference is used instead. Which direction
        it picks does not matter; that it picks *a* unit vector perpendicular to the
        others does.

        """
        for name, lat, lon in AWKWARD:
            frame = TangentFrame.at_latlon(lat, lon)
            for axis in (frame.east, frame.north, frame.up):
                self.assertAlmostEqual(axis.length(), 1.0, places=9, msg=name)
            self.assertAlmostEqual(frame.east.dot(frame.north), 0.0, places=9, msg=name)
            self.assertAlmostEqual(frame.east.dot(frame.up), 0.0, places=9, msg=name)
            self.assertAlmostEqual(frame.north.dot(frame.up), 0.0, places=9, msg=name)

    def test_a_frame_answers_the_same_on_every_call(self):
        """
        A frame that reshuffled its own basis between two calls would move every ship it
        held. The polar fallback is where that could happen, because there is nothing in
        the geometry to prefer one east over another.

        """
        for name, lat, lon in AWKWARD:
            first = TangentFrame.at_latlon(lat, lon)
            second = TangentFrame.at_latlon(lat, lon)
            self.assertEqual(first.east, second.east, name)
            self.assertEqual(first.north, second.north, name)

    def test_a_metre_is_a_metre_at_every_latitude(self):
        """
        Longitude converges at the poles, so a degree is a different distance at eighty
        than at nothing. A frame that leaked that into its metres would make a high
        latitude region silently the wrong size.

        """
        for name, lat, lon in AWKWARD:
            frame = TangentFrame.at_latlon(lat, lon)
            origin = frame.local_to_sphere(0.0, 0.0)
            for east, north in ((30_000.0, 0.0), (0.0, 30_000.0), (21_213.0, 21_213.0)):
                walked = origin.distance_to(frame.local_to_sphere(east, north))
                self.assertAlmostEqual(walked, math.hypot(east, north), delta=1.0, msg=name)


class TestTheGroundStaysContinuous(GlobalTestCase):
    def worst_step(self, frame, span_m, steps):
        worst = 0.0
        for across in (True, False):
            previous = None
            for index in range(steps + 1):
                offset = (2.0 * index / steps - 1.0) * span_m
                point = frame.local_to_sphere(
                    offset if across else 0.0, 0.0 if across else offset
                )
                value = self.world.elevation_m(point)
                if previous is not None:
                    worst = max(worst, abs(value - previous))
                previous = value
        return worst

    def test_no_cliff_at_any_awkward_place(self):
        """
        Refinement, the same test M1.7 settled on. A fixed threshold cannot tell steep
        ground from a seam; halving the step can, because a discontinuity does not care.

        """
        for name, lat, lon in AWKWARD:
            frame = TangentFrame.at_latlon(lat, lon)
            coarse = self.worst_step(frame, 40_000.0, 200)
            fine = self.worst_step(frame, 40_000.0, 400)
            self.assertLess(fine, coarse * 0.8 + 0.5, f"{name} has a seam in it")

    def test_walking_round_a_pole_comes_back_to_the_same_ground(self):
        """
        A circle of constant latitude near a pole crosses every meridian there is. If
        anything in the stack were built on longitude, this would not close.

        """
        for latitude in (89.4, -89.4):
            frame = TangentFrame.at_latlon(latitude, 0.0)
            radius = 20_000.0
            readings = []
            for degrees in range(0, 361, 5):
                radians = math.radians(degrees)
                readings.append(self.world.elevation_m(frame.local_to_sphere(
                    math.sin(radians) * radius, math.cos(radians) * radius
                )))
            self.assertAlmostEqual(readings[0], readings[-1], places=6)
            for first, second in zip(readings, readings[1:]):
                self.assertLess(abs(second - first), 90.0, f"a seam at {latitude}")

    def test_the_antimeridian_is_not_a_place(self):
        """
        Approached from both sides, at several latitudes, the ground has to agree. A seam
        here is the classic failure of anything that stores longitude.

        """
        for latitude in (-70.0, -20.0, 0.0, 35.0, 74.0):
            east = self.world.elevation_m(SpherePoint.from_latlon(latitude, 179.999))
            west = self.world.elevation_m(SpherePoint.from_latlon(latitude, -179.999))
            self.assertLess(abs(east - west), 40.0, f"at {latitude} degrees")

    def test_crossing_a_plate_margin_is_smooth(self):
        """
        The place M1.2 and M1.4 kept producing cliffs, revisited through a region frame
        rather than through the field directly - because that is how a ship meets one.

        """
        closest, at = 9e9, None
        for point in scattered(1500):
            distance = self.world.plates.margin_at(point).distance_m
            if distance < closest:
                closest, at = distance, point
        self.assertLess(closest, 60_000.0, "no margin found to cross")

        frame = TangentFrame.at(at)
        coarse = self.worst_step(frame, 120_000.0, 300)
        fine = self.worst_step(frame, 120_000.0, 600)
        self.assertLess(fine, coarse * 0.8 + 0.5, "a cliff on a plate margin")


class TestARegionWorksAtTheEndsOfTheEarth(GlobalTestCase):
    def test_a_region_can_be_anchored_anywhere(self):
        for name, lat, lon in AWKWARD:
            region = region_at(lat, lon)
            provider = WorldbuilderTerrain(self.world, region, region_name="edge")
            for east, north in ((0.0, 0.0), (30_000.0, -20_000.0), (-45_000.0, 45_000.0)):
                metres = provider.terrain_z_at(Position(east, north))
                self.assertTrue(math.isfinite(metres), name)
                self.assertGreater(metres, -8000.0, name)
                self.assertLess(metres, 3000.0, name)

    def test_projection_error_does_not_depend_on_latitude(self):
        """
        M1.1 measured this on the frame and found it identical to six decimal places. It
        is restated here on a *region* because that is the object a game configures, and
        because a high-latitude region silently being the wrong size is the kind of thing
        nobody would look for.

        """
        worst = {}
        for name, lat, lon in AWKWARD:
            region = region_at(lat, lon)
            frame = region.coast.frame
            found = 0.0
            for bearing in range(0, 360, 20):
                radians = math.radians(bearing)
                east = math.sin(radians) * region.reach_m
                north = math.cos(radians) * region.reach_m
                walked = region.origin.distance_to(frame.local_to_sphere(east, north))
                found = max(found, abs(walked - math.hypot(east, north)))
            worst[name] = found
        for name, found in worst.items():
            self.assertLess(found, 1.0, f"{name}: {found:.3f} m")
        spread = max(worst.values()) - min(worst.values())
        self.assertLess(spread, 0.5, f"latitude changes the error: {worst}")

    def test_the_bottom_answers_at_the_ends_of_the_earth(self):
        for name, lat, lon in AWKWARD:
            bottom = self.world.bottom_at(SpherePoint.from_latlon(lat, lon))
            self.assertAlmostEqual(bottom.sand + bottom.mud + bottom.rock, 1.0, places=9)
            self.assertIn(bottom.dominant, ("sand", "mud", "rock"), name)

    def test_features_can_be_placed_at_a_pole(self):
        """
        The polar fallback again, this time through a feature's own frame. A stamped rock
        at ninety degrees exercises a basis that had to be chosen rather than derived.

        `SHAPE`, not `RAISE`. The north pole of this world is thirty-one metres of dry
        land, so a raise to three metres below datum correctly did nothing at all - which
        is the composition rule working and the test asking the wrong question.

        """
        for latitude in (90.0, -90.0):
            at = SpherePoint.from_latlon(latitude, 0.0)
            natural = self.world.elevation_m(at)
            features = Features([Feature(
                kind="polar cairn", at=at, target_m=natural + 120.0,
                length_m=80.0, width_m=80.0, compose=SHAPE, marked=True, substrate=ROCK,
            )])
            world = Surface(WORLD_SEED, features=features)
            self.assertAlmostEqual(world.elevation_m(at), natural + 120.0, delta=0.6)

            # And it is local: a few hundred metres off, the world is untouched.
            frame = TangentFrame.at(at)
            for degrees in range(0, 360, 30):
                radians = math.radians(degrees)
                away = frame.local_to_sphere(
                    math.sin(radians) * 400.0, math.cos(radians) * 400.0
                )
                self.assertEqual(world.elevation_m(away), self.world.elevation_m(away))


class TestSailingAcrossAPole(GlobalTestCase):
    """
    The case that only exists because the world is a sphere.

    A vessel steering due north does not stop at the pole; she carries on and finds herself
    steering south down the far side, ninety degrees of longitude from where anybody would
    guess. Nothing in the generator has to *do* anything about that - the point is that
    nothing in it may break because of it.
    """

    def setUp(self):
        self.region = region_at(89.4, 0.0)
        self.provider = WorldbuilderTerrain(self.world, self.region, region_name="polar")

    def test_a_track_over_the_pole_is_continuous_ground(self):
        previous = None
        for step in range(-60, 61):
            north = step * 2_000.0
            metres = self.provider.terrain_z_at(Position(0.0, north))
            if previous is not None:
                self.assertLess(abs(metres - previous), 90.0, f"a seam at {north:.0f} m")
            previous = metres

    def test_the_far_side_of_the_pole_is_the_far_side_of_the_pole(self):
        """
        The pole is sixty-seven kilometres due north of eighty-nine point four degrees.
        Two hundred kilometres north of it is a hundred and thirty-three kilometres *past*
        the pole - which means heading south, on the opposite meridian, at a lower latitude
        than she started. If the frame turned back at the top instead, a voyage north would
        quietly become a voyage nowhere.

        The signature that matters is the meridian flip, not the latitude. At a hundred and
        twenty kilometres she is already past the pole and still north of where she began,
        which is what the first version of this test mistook for her never getting there.

        """
        origin_lat, origin_lon = self.region.origin.to_latlon()
        to_pole = self.region.origin.distance_to(SpherePoint.from_latlon(90.0, 0.0))
        self.assertAlmostEqual(to_pole, 66_700.0, delta=500.0)

        just_past = self.provider.point_at(Position(0.0, to_pole + 20_000.0)).to_latlon()
        self.assertGreater(just_past[0], origin_lat, "she should still be north of home")
        self.assertGreater(abs(just_past[1] - origin_lon), 90.0, "same meridian")

        well_past = self.provider.point_at(Position(0.0, 200_000.0)).to_latlon()
        self.assertLess(well_past[0], origin_lat, "she did not get past the pole")
        self.assertGreater(abs(well_past[1] - origin_lon), 90.0, "same meridian")

    def test_the_ground_over_the_pole_is_the_ground_at_the_pole(self):
        """
        Approached across the top, the pole itself must read what it reads. It is one
        point on the planet and every meridian meets there.

        """
        pole = SpherePoint.from_latlon(90.0, 0.0)
        straight = self.world.elevation_m(pole)
        distance = self.region.origin.distance_to(pole)
        crossed = self.provider.terrain_z_at(Position(0.0, distance))
        self.assertAlmostEqual(crossed, straight, places=4)

    def test_she_can_ask_what_the_bottom_is_up_there(self):
        for north in (-80_000.0, 0.0, 66_000.0, 120_000.0):
            bottom = self.provider.bottom_type_at(Position(0.0, north))
            self.assertIn(bottom, ("sand", "mud", "rock"))


if __name__ == "__main__":
    unittest.main()
