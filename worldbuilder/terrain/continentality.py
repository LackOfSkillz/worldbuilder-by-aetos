"""
Where the land is, decided without reference to the plates.

**This layer must not know that plates exist**, and the import list is the proof. Earth's
plates carry both kinds of crust - the North American plate holds the continent and half
the Atlantic floor - so deriving continents from plate cells would make every coastline
follow a tectonic boundary and every landmass wear the shape of a Voronoi cell. A world
like that reads as generated within about ten seconds of looking at it.

Two independent fields, laid over each other by two systems that have never met, is what
makes a planet look like it has a history.

**The wavelengths here are enormous, on purpose.** Continentality decides *this side is
continent, that side is ocean* and nothing finer. It has no opinion about coves, beaches,
headlands or islands - those belong to layers that do not exist yet, and a continentality
field that produced a beautiful coastline would have stolen their job and made them
impossible to write.
"""

import math
from dataclasses import dataclass

from ..geometry.sphere import EARTH_RADIUS_M
from ..geometry.tangent import TangentFrame
from .noise import Noise

#: Cycles per unit of noise space at the first octave. About one and a quarter, which puts
#: the largest features somewhere near five thousand kilometres across - a continent, or
#: an ocean basin, and nothing smaller.
BASE_FREQUENCY = 1.25

#: How many octaves. Few, deliberately. Enough to stop the landmasses being simple blobs,
#: not enough to start carving a coast.
OCTAVES = 4

#: How high a continental interior stands, and how deep an ocean basin lies, in metres
#: before anything else has its say.
CONTINENT_M = 700.0
ABYSS_M = -4600.0

#: How much of the surface is dry, unless a world asks otherwise. Earth is about 29 per
#: cent, and it is the single most powerful thing a developer can turn: raising the sea
#: drowns continental margins into archipelagos, lowering it joins islands into landmasses.
LAND_FRACTION = 0.29

#: How many points to sample when working out where sea level falls. A Fibonacci spread,
#: so it is even and deterministic; a few thousand is ample for a threshold and costs a
#: fraction of a second once per world.
CALIBRATION_SAMPLES = 4000

#: How far apart the probes are when measuring which way the land rises, in metres.
#: Twenty kilometres: far enough that the difference is not floating-point dust at these
#: wavelengths, near enough to be a local measurement.
GRADIENT_STEP_M = 20_000.0


@dataclass(frozen=True)
class Gradient:
    """
    Which way continentality increases, here, and how sharply.

    Attributes:
        east (float): Change per metre, eastwards.
        north (float): Change per metre, northwards.

    """

    east: float
    north: float

    def magnitude(self):
        return math.hypot(self.east, self.north)


class Continentality:
    """
    The broad shape of land and sea on a world.

    Notes:
        Takes a seed and nothing else. It cannot consult the plates because it has no way
        to reach them, which is the point: an architectural claim enforced by the import
        list rather than by a comment asking people to behave.

    """

    def __init__(self, world_seed, radius_m=EARTH_RADIUS_M, land_fraction=LAND_FRACTION):
        self.radius_m = radius_m
        self.land_fraction = land_fraction
        self._noise = Noise(world_seed, salt=0x0C0FFEE)
        self._shore, self._spread = self._calibrate()

    def _calibrate(self):
        """
        Where sea level falls, and how varied the field is.

        Returns:
            calibration (tuple): The value that counts as the shoreline, and the spread
                of values around it.

        Notes:
            Summed value noise clusters near the middle of its range rather than filling
            it, so a fixed threshold produces whatever land fraction the noise happens to
            feel like - measured at nought, nought and two per cent on three seeds, against
            Earth's twenty-nine. Sampling the field and taking the quantile that gives the
            asked-for fraction makes it a control rather than an accident.

            Two floats, worked out once per world. Generated-and-stored, and still
            perfectly deterministic: the sample points are a fixed spiral and the field is
            a pure function.

        """
        from ..geometry.vectors import Vec3
        from ..geometry.sphere import SpherePoint

        golden = math.pi * (3.0 - math.sqrt(5.0))
        values = []
        for index in range(CALIBRATION_SAMPLES):
            z = 1.0 - 2.0 * (index + 0.5) / CALIBRATION_SAMPLES
            ring = math.sqrt(max(0.0, 1.0 - z * z))
            angle = golden * index
            values.append(
                self.at(SpherePoint(Vec3(math.cos(angle) * ring, math.sin(angle) * ring, z)))
            )
        values.sort()
        shore = values[int((1.0 - self.land_fraction) * (len(values) - 1))]
        middle = values[len(values) // 2]
        spread = (values[int(0.84 * (len(values) - 1))] - middle) or 1e-6
        return shore, spread

    def at(self, point):
        """
        Args:
            point (SpherePoint): Anywhere on the planet.

        Returns:
            value (float): Roughly -1 (deep ocean) to +1 (continental interior).

        """
        return self._noise.fbm(point, BASE_FREQUENCY, OCTAVES)

    def base_elevation(self, point):
        """
        Args:
            point (SpherePoint): Anywhere on the planet.

        Returns:
            metres (float): Elevation relative to datum, before tectonics or detail.

        Notes:
            Measured from the calibrated shoreline rather than from the middle of the
            field, so sea level means what it was asked to mean.

            The curve is deliberately not linear. Earth's surface is bimodal - a great
            deal of shelf near sea level, a great deal of abyssal plain far below, not
            much in between - so a straight ramp from abyss to summit would put far too
            much of the planet at the depths a ship sails in.

            The exponents are below one, which makes the ground climb and fall *quickly*
            near the shore and flatten out far from it. That leaves a narrow transition on
            purpose. Building a broad shelf into it is M1.5's business; this only has to
            leave room for one, and a gentle ramp here would have used up the space.

        """
        above = (self.at(point) - self._shore) / self._spread
        if above >= 0.0:
            return CONTINENT_M * min(1.0, above) ** 0.75
        return ABYSS_M * min(1.0, -above) ** 0.5

    def gradient(self, point):
        """
        Which way continentality rises, measured along the surface.

        Args:
            point (SpherePoint): Anywhere on the planet.

        Returns:
            gradient (Gradient): Change per metre, east and north.

        Notes:
            **Measured along geodesics, not by nudging the raw coordinates.** A finite
            difference taken in x, y and z would step *off* the sphere and measure the
            noise volume rather than the planet's surface, and the error would grow with
            latitude in a way nobody would notice until the shelves near the poles came
            out wrong. The tangent frame from M1.1 already knows how to walk a fixed
            number of metres in a real direction, so it does.

            Four samples, and it is not free: this costs five evaluations where the value
            alone costs one. Which is exactly why it is a separate call. The shelf shaper
            will want it near a coast; open ocean never will, and should not pay for it.

        """
        frame = TangentFrame.at(point, self.radius_m)
        step = GRADIENT_STEP_M
        east = self.at(frame.local_to_sphere(step, 0.0))
        west = self.at(frame.local_to_sphere(-step, 0.0))
        north = self.at(frame.local_to_sphere(0.0, step))
        south = self.at(frame.local_to_sphere(0.0, -step))
        return Gradient(
            east=(east - west) / (2.0 * step),
            north=(north - south) / (2.0 * step),
        )
