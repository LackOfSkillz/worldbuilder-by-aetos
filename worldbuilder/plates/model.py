"""
What a plate is, which is less than people expect.

**A plate is a piece of the surface that moves as one rigid body.** It is not a continent
and it does not have a crust type. Earth's plates carry both kinds - the North American
plate holds the continent *and* half the Atlantic floor - and building the two ideas into
one object is the mistake that makes generated worlds look generated: continents inherit
the shapes of the plate cells, and every coastline correlates with a tectonic boundary.

So a plate here carries only motion. Where land is is a separate question, answered by a
separate field, and the two are deliberately uncorrelated.

Motion is an **Euler pole and a rate**, not a drift direction. On a sphere, rigid motion
*is* rotation about an axis through the centre; a plate given a single drift vector would
be moving correctly near its seed and increasingly wrongly thousands of kilometres away,
and the boundaries are exactly where that error would show.
"""

from dataclasses import dataclass

from ..geometry.sphere import SpherePoint
from ..geometry.vectors import Vec3


@dataclass(frozen=True)
class Plate:
    """
    Attributes:
        index (int): Which plate this is. Stable for a given seed and count.
        seed (SpherePoint): Its centre. A point belongs to the plate whose seed is
            nearest, which is the whole of the Voronoi construction.
        euler_pole (SpherePoint): The axis it turns about.
        rate_rad_per_myr (float): How fast, in radians per million years. Signed: the
            sign and the pole together give the sense of rotation, so there is no
            separate clockwise flag to get wrong.

    """

    index: int
    seed: SpherePoint
    euler_pole: SpherePoint
    rate_rad_per_myr: float

    def angular_velocity(self):
        """
        Returns:
            omega (Vec3): The rotation vector - the pole, scaled by the rate.

        Notes:
            Combining the two into one vector is what makes surface velocity a single
            cross product rather than a special case at the pole itself.

        """
        return self.euler_pole.vector.scaled(self.rate_rad_per_myr)
