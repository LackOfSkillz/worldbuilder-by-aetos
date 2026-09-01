"""
The whole world, assembled.

One object holding every layer, and one question worth asking of it: how high is the
ground at this point. Everything else in the engine exists to answer that.

    continentality      where land is                    structural
    tectonics           what the plates did to it        structural
    shelf               what the coast does to the water structural
    features            what somebody put there          structural
    detail              roughness                        resolution-aware

**Only the last of those thins out with zoom.** The rest are geography and answer the same
at every scale; a chart drawn at twenty miles shows the same world as one drawn at one,
generalised rather than replaced. If structure faded with sampling, zooming out would not
simplify the coastline - it would move it.

**Features come after the shelf and before detail, and that ordering is the phase.** After
the shelf, because a harbour is cut into real bathymetry rather than instead of it. Before
detail, because detail then knows to get out of their way: thirty-five metres of coastal
roughness would erase a bar standing four metres proud of the bottom, and a bar nobody can
find is not a bar.
"""

from ..bathymetry.features import Features
from ..bathymetry.shelf import Shelf
from ..bathymetry.substrate import Substrate
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
        plate records, the continentality calibration, two noise lattices that fill
        themselves in as they are used, and however many features somebody placed. Nothing
        resembling a map is stored anywhere.

    """

    def __init__(
        self,
        world_seed,
        radius_m=EARTH_RADIUS_M,
        plate_count=DEFAULT_PLATE_COUNT,
        land_fraction=LAND_FRACTION,
        features=None,
    ):
        self.world_seed = world_seed
        self.radius_m = radius_m
        self.plates = plates_for(world_seed, plate_count)
        self.land = Continentality(world_seed, radius_m, land_fraction)
        self.tectonics = Tectonics(self.plates, self.land, radius_m)
        self.shelf = Shelf(self.tectonics, self.land, radius_m)
        self.detail = Detail(world_seed, radius_m)
        if features is None:
            features = Features((), radius_m)
        elif not isinstance(features, Features):
            features = Features(features, radius_m)
        self.features = features
        self.substrate = Substrate(self)

    def structural_m(self, point):
        """
        The ground before any roughness, which is the same at every scale.

        Args:
            point (SpherePoint): Anywhere on the planet.

        Returns:
            metres (float): Relative to datum.

        """
        return self.features.apply(point, self.shelf.elevation_m(point))[0]

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
        shaped, authority = self.features.apply(point, reading.elevation_m)
        amplitude = self.detail.amplitude_m(
            point, shaped, reading.weight, reading.tectonic_m
        )
        # Where somebody stated a shape, roughness defers to it.
        amplitude *= 1.0 - authority
        return shaped + self.detail.offset_m(point, amplitude, resolution_m)

    def bottom_at(self, point):
        """
        What the bottom is made of, as fractions of sand, mud and rock.

        Args:
            point (SpherePoint): Anywhere on the planet.

        Returns:
            composition (Composition): Fractions summing to one.

        Notes:
            Costs several times an elevation, because it needs the local slope and a slope
            is four probes. Affordable because a ship sounds continuously and anchors
            once - and the same intermediates can be handed in when a caller already has
            them, the way the shelf takes them.

        """
        return self.substrate.at(point)
