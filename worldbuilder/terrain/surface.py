"""
The whole world, assembled.

One object holding every layer, and one question worth asking of it: how high is the
ground at this point. Everything else in the engine exists to answer that.

    continentality      where land is                    structural
    tectonics           what the plates did to it        structural
    shelf               what the coast does to the water structural
    detail              roughness                        resolution-aware

**Only the last of those thins out with zoom.** The first three are geography and answer
the same at every scale; a chart drawn at twenty miles shows the same world as one drawn
at one, generalised rather than replaced. If structure faded with sampling, zooming out
would not simplify the coastline - it would move it.
"""

from ..bathymetry.shelf import Shelf
from ..geometry.sphere import EARTH_RADIUS_M
from ..plates.generation import DEFAULT_PLATE_COUNT, plates_for
from .continentality import LAND_FRACTION, Continentality
from .detail import Detail
from .tectonics import Tectonics


class Surface:
    """
    A generated planet, ready to be asked about.

    Notes:
        Built from a seed and a handful of parameters, and holding a few kilobytes: the
        plate records, the continentality calibration, and two noise lattices that fill
        themselves in as they are used. Nothing resembling a map is stored anywhere.

    """

    def __init__(
        self,
        world_seed,
        radius_m=EARTH_RADIUS_M,
        plate_count=DEFAULT_PLATE_COUNT,
        land_fraction=LAND_FRACTION,
    ):
        self.world_seed = world_seed
        self.radius_m = radius_m
        self.plates = plates_for(world_seed, plate_count)
        self.land = Continentality(world_seed, radius_m, land_fraction)
        self.tectonics = Tectonics(self.plates, self.land, radius_m)
        self.shelf = Shelf(self.tectonics, self.land, radius_m)
        self.detail = Detail(world_seed, radius_m)

    def structural_m(self, point):
        """
        The ground before any roughness, which is the same at every scale.

        Args:
            point (SpherePoint): Anywhere on the planet.

        Returns:
            metres (float): Relative to datum.

        """
        return self.shelf.elevation_m(point)

    def elevation_m(self, point, resolution_m=None):
        """
        How high the ground is.

        Args:
            point (SpherePoint): Anywhere on the planet.
            resolution_m (float, optional): How far apart the samples being taken are.
                None asks for canonical ground truth, which is what physics uses; a
                number lets detail finer than the sampling drop out, which is both faster
                and less prone to shimmer.

        Returns:
            metres (float): Relative to datum.

        Notes:
            **Canonical is a defined thing.** `None` evaluates every configured octave
            down to the canonical minimum wavelength - not infinite detail. Physics always
            asks canonically, so a rock is where it is regardless of how anybody happens
            to be looking at the sea around it.

        """
        # One pass, and the intermediates come back with the answer. Asking the shelf
        # for its weight and the tectonics for their offset separately recomputed the
        # gradient twice and the plate work three times, for four times the cost.
        reading = self.shelf.evaluate(point)
        amplitude = self.detail.amplitude_m(
            point, reading.elevation_m, reading.weight, reading.tectonic_m
        )
        return reading.elevation_m + self.detail.offset_m(point, amplitude, resolution_m)
