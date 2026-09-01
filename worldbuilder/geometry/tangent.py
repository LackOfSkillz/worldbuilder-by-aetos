"""
A flat chart of a small piece of a round planet.

This is the file that lets a flat simulation sail on a sphere, and it is not a compromise
made to avoid work - it is what a chart *is*. Every real chart projects a curved surface
onto flat paper; every navigator works plane sailing over short distances and reaches for
great circles only on an ocean passage. A `TangentFrame` is that projection, with an
origin somewhere on the globe and east, north and up defined there.

**The projection is azimuthal equidistant**, chosen for one property: distance and bearing
*from the frame's origin* are exact, at any range, by construction. Nothing is
approximated near the middle of a chart and nothing degrades gracefully - it is simply
right there, and the error is entirely in how two points away from the origin relate to
each other. That is the trade a navigator would recognise, and section 11 of the spec
asks for the error to be measured rather than assumed, which `debug/projection_error.py`
does.

**Nothing outside this package needs to know it exists.** Maritime goes on working in flat
metres and asking how deep the water is; the provider holds the frame and does the
conversion. That is the whole of the integration.
"""

import math
from dataclasses import dataclass

from .sphere import EARTH_RADIUS_M, SpherePoint
from .vectors import DEGENERATE, NORTH_AXIS, POLAR_FALLBACK, Vec3


@dataclass(frozen=True)
class TangentFrame:
    """
    A local flat coordinate system, tangent to the planet at one point.

    Attributes:
        origin (SpherePoint): Where the chart touches the globe; local (0, 0).
        east (Vec3): Unit vector, increasing local x.
        north (Vec3): Unit vector, increasing local y.
        up (Vec3): Unit vector, away from the centre. Equal to the origin's vector.
        radius_m (float): The planet's radius.

    """

    origin: SpherePoint
    east: Vec3
    north: Vec3
    up: Vec3
    radius_m: float

    @classmethod
    def at(cls, origin, radius_m=EARTH_RADIUS_M):
        """
        Build a frame at a point on the globe.

        Args:
            origin (SpherePoint): Where the chart is centred.
            radius_m (float, optional): The planet's radius.

        Returns:
            frame (TangentFrame): A frame with an orthonormal basis, everywhere.

        Notes:
            East is the direction at right angles to both straight up and the planet's
            axis, which is what east means anywhere it means anything.

            **At a pole it means nothing**, and that is not a failure of the maths but a
            fact about poles: every direction from the north pole is south, and no
            direction is east. The cross product goes to zero there and the basis cannot
            be derived, so one is chosen instead - a fixed reference direction, used
            every time. Which direction it is does not matter in the slightest. That it
            is the *same* one on every call is the whole requirement, because a frame
            that reshuffled itself between two calls would move every ship it held.

        """
        up = origin.vector
        sideways = NORTH_AXIS.cross(up)
        if sideways.length() <= DEGENERATE:
            # At a pole, or near enough that the arithmetic has lost its nerve.
            sideways = POLAR_FALLBACK.cross(up)
            if sideways.length() <= DEGENERATE:
                # The fallback was itself parallel to up, which cannot happen for a
                # planet whose axis is z - but a fixed second answer costs one line and
                # removes the only path here that could ever raise.
                sideways = Vec3(0.0, 1.0, 0.0).cross(up)
        east = sideways.normalised()
        north = up.cross(east)
        return cls(origin=origin, east=east, north=north, up=up, radius_m=radius_m)

    @classmethod
    def at_latlon(cls, latitude_deg, longitude_deg, radius_m=EARTH_RADIUS_M):
        """Convenience: a frame centred on a named latitude and longitude."""
        return cls.at(SpherePoint.from_latlon(latitude_deg, longitude_deg), radius_m)

    def local_to_sphere(self, x_m, y_m):
        """
        Where a point on the chart actually is.

        Args:
            x_m (float): Metres east of the frame's origin.
            y_m (float): Metres north of the frame's origin.

        Returns:
            point (SpherePoint): The place on the globe.

        Notes:
            The local distance from the origin is taken as an arc along the surface, not
            as a straight line across the tangent plane - which is what makes the
            projection equidistant, and what stops a thousand-mile chart quietly claiming
            more ocean than the planet has.

        """
        distance = math.hypot(x_m, y_m)
        if distance == 0.0:
            return self.origin

        angle = distance / self.radius_m
        heading = self.east.scaled(x_m / distance) + self.north.scaled(y_m / distance)
        return SpherePoint.from_vector(
            self.up.scaled(math.cos(angle)) + heading.scaled(math.sin(angle))
        )

    def sphere_to_local(self, point):
        """
        Where a place on the globe falls on this chart.

        Args:
            point (SpherePoint): Somewhere on the planet.

        Returns:
            local (tuple): Metres east and north of the frame's origin.

        Notes:
            The exact inverse of `local_to_sphere`, and tested as one. A point directly
            opposite the origin has no direction on this chart at all - every bearing
            reaches it - and returns the origin rather than raising, because a chart of
            half a planet is a misuse the caller should not have to guard against.

        """
        along = self.up.dot(point.vector)
        sideways = point.vector - self.up.scaled(along)
        across = sideways.length()
        if across <= DEGENERATE:
            # The origin itself, or its antipode.
            return 0.0, 0.0

        heading = sideways.scaled(1.0 / across)
        distance = math.atan2(across, along) * self.radius_m
        return heading.dot(self.east) * distance, heading.dot(self.north) * distance
