"""
A point on the planet, and the distances between points.

**Canonical position is a unit vector from the centre, not a latitude and longitude.**
That is the decision the rest of the engine rests on, and it is worth stating why, because
latitude and longitude are the obvious choice and they are the wrong one.

A unit vector has no seam. There is no value of it that means the same place as another
value, so no code anywhere has to remember that 180 east and 180 west are the same
meridian, and no test has to guard the antimeridian. A unit vector has no pole
singularity either: the north pole is (0, 0, 1), an ordinary point that no arithmetic
treats specially. And it is already the coordinate that three-dimensional noise wants and
that a nearest-plate test wants, so the representation that avoids the bugs is also the
one the work is done in.

Latitude and longitude remain, as conversions, for the two things they are good for:
talking to people and reading a configuration file.
"""

import math
from dataclasses import dataclass

from .vectors import Vec3

#: Earth's mean radius, in metres. The default because the whole design is calibrated on
#: it - horizon distances, how long an ocean takes, how far a light looms.
EARTH_RADIUS_M = 6_371_000.0


@dataclass(frozen=True)
class SpherePoint:
    """
    A place on the planet, as a unit vector from its centre.

    Attributes:
        vector (Vec3): Unit length, from the centre of the sphere.

    Notes:
        The radius is not stored. A point is a *direction* from the centre; how big the
        planet is belongs to the world, not to each of the billions of places on it, and
        keeping them apart means a point cannot be quietly attached to the wrong planet.

    """

    vector: Vec3

    @classmethod
    def from_vector(cls, vector):
        """
        Args:
            vector (Vec3): Any non-zero vector; its direction is what is kept.

        Returns:
            point (SpherePoint): The place that direction points at.

        """
        return cls(vector.normalised())

    @classmethod
    def from_latlon(cls, latitude_deg, longitude_deg):
        """
        Args:
            latitude_deg (float): Degrees north of the equator, negative for south.
            longitude_deg (float): Degrees east of the prime meridian.

        Returns:
            point (SpherePoint): The place named.

        Notes:
            Longitude is not normalised first and does not need to be. Sine and cosine
            are periodic, so -180, +180 and +540 produce the same vector by arithmetic
            rather than by a rule somebody has to remember to apply.

        """
        latitude = math.radians(latitude_deg)
        longitude = math.radians(longitude_deg)
        cos_lat = math.cos(latitude)
        return cls(
            Vec3(
                cos_lat * math.cos(longitude),
                cos_lat * math.sin(longitude),
                math.sin(latitude),
            )
        )

    def to_latlon(self):
        """
        Returns:
            latlon (tuple): Latitude and longitude in degrees, longitude in -180..180.

        Notes:
            At a pole the longitude returned is zero, which is a convention rather than a
            fact: every meridian meets there and none of them is the answer. Converting
            back gives the same pole, which is the only property that matters.

        """
        latitude = math.degrees(math.asin(max(-1.0, min(1.0, self.vector.z))))
        longitude = math.degrees(math.atan2(self.vector.y, self.vector.x))
        return latitude, longitude

    def angle_to(self, other):
        """
        Args:
            other (SpherePoint): The far place.

        Returns:
            radians (float): The angle subtended at the planet's centre.

        Notes:
            By arc tangent of the cross and dot products rather than by the arc cosine of
            the dot alone. The simpler form loses its precision for points close
            together - exactly the case a ship spends its whole life in - because the
            cosine of a small angle is very nearly one, and the difference between "very
            nearly one" and "one" is where the answer lives.

        """
        across = self.vector.cross(other.vector).length()
        along = self.vector.dot(other.vector)
        return math.atan2(across, along)

    def distance_to(self, other, radius_m=EARTH_RADIUS_M):
        """
        Args:
            other (SpherePoint): The far place.
            radius_m (float, optional): The planet's radius.

        Returns:
            metres (float): Great-circle distance along the surface.

        """
        return self.angle_to(other) * radius_m
