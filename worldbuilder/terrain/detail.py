"""
Texture, and only texture.

Detail roughens ground that structure has already decided. It does not decide anything
itself: no coves, no shoals, no bars, no islands. Those are M1.7's job, and a detail layer
that produced approximations of them everywhere would make every navigational hazard an
accident of the noise spectrum rather than a thing somebody put there.

The rule, and it is quantitative: **detail amplitude stays well below structural relief.**
A five-kilometre octave may not turn twenty metres of shelf water into an island.

Two bands, and Mark 1 needs no more:

    meso    one to twenty kilometres    coastal roughness, seabed undulation
    micro   250 metres to one kilometre local texture

**Canonical means a defined thing, not infinite detail.** `resolution_m=None` evaluates
every configured octave down to `CANONICAL_WAVELENGTH_M` and no further. Without that
written down, somebody adds a fifty-metre octave in three months and every coastline,
reef and chart in every world silently changes.
"""

import math

from .noise import Noise

#: The finest ground truth this generator has. Physics sees detail down to here and no
#: further; there is no finer octave to add without changing what canonical means.
CANONICAL_WAVELENGTH_M = 250.0

#: The coarsest detail band. Above this, structure has the say.
COARSEST_WAVELENGTH_M = 20_000.0

#: How many multiples of the sample spacing an octave's wavelength must be before it is
#: worth drawing, and where it has faded out entirely.
#:
#: Nyquist puts the floor at two, but barely representable is not usefully representable -
#: an octave at twice the sample spacing is four points a cycle, which reads as noise
#: rather than as landform and aliases while doing it. It fades between two and four.
BARELY_M = 2.0
CLEARLY_M = 4.0

#: How rough the ground is, in metres, in each setting. Every one of these is far below the
#: structural relief it decorates: a shelf falls a hundred and fifty metres over eighty
#: kilometres, so fifteen metres of roughness on it is texture and not topography.
ABYSSAL_M = 55.0
SHELF_M = 15.0
COAST_M = 35.0
INTERIOR_M = 80.0
MOUNTAIN_M = 150.0


def _smooth(fraction):
    clamped = max(0.0, min(1.0, fraction))
    return clamped * clamped * (3.0 - 2.0 * clamped)


class Detail:
    """
    Roughness, scaled to what is being roughened and to what can be seen.

    Notes:
        The only layer in the engine that is resolution-aware. Continentality, tectonics
        and the shelf are structural geography and answer the same at every scale; if they
        thinned out with zoom, a chart would show a different *world* rather than a
        generalised one.

    """

    def __init__(self, world_seed, radius_m):
        self.radius_m = radius_m
        self._noise = Noise(world_seed, salt=0x5EABED)
        self._bands = self._plan()

    def _plan(self):
        """
        The octaves, as wavelengths in metres with the share of amplitude each carries.

        Returns:
            bands (tuple): `(wavelength_m, frequency, share)`, coarsest first.

        Notes:
            Worked out once. Each octave is half the wavelength and half the amplitude of
            the one before, and the shares are normalised so that the total amplitude is
            what the caller asked for however many bands there happen to be - otherwise
            adding an octave would quietly make every world rougher.

        """
        bands = []
        wavelength = COARSEST_WAVELENGTH_M
        share = 1.0
        while wavelength >= CANONICAL_WAVELENGTH_M:
            # Wavelength in metres to cycles per unit of noise space on the unit sphere.
            frequency = 2.0 * math.pi * self.radius_m / wavelength / (2.0 * math.pi)
            bands.append([wavelength, frequency, share])
            wavelength *= 0.5
            share *= 0.5
        total = sum(band[2] for band in bands) or 1.0
        return tuple((w, f, s / total) for w, f, s in bands)

    def amplitude_m(self, point, elevation_m, shelf_weight, tectonic_m):
        """
        How rough the ground should be here.

        Args:
            point (SpherePoint): Where.
            elevation_m (float): The structural elevation.
            shelf_weight (float): How much say the shelf had, nothing to one.
            tectonic_m (float): The tectonic contribution.

        Returns:
            metres (float): The amplitude detail may use.

        Notes:
            Blended from smooth weights rather than chosen from a category, for the same
            reason as everything else in this engine. And the trench term is deliberate:
            a deep, deliberate piece of structure stays legible instead of being buried
            under texture that has no idea it is there.

        """
        # How high, from deep water through the shelf to the tops.
        deep = 1.0 - _smooth((elevation_m + 3000.0) / 2500.0)
        high = _smooth((elevation_m - 200.0) / 900.0)
        near_shore = _smooth(1.0 - abs(elevation_m) / 350.0)

        rough = (
            deep * ABYSSAL_M
            + (1.0 - deep) * (1.0 - high) * INTERIOR_M
            + high * MOUNTAIN_M
        )
        rough = rough * (1.0 - near_shore) + COAST_M * near_shore
        rough = rough * (1.0 - shelf_weight) + SHELF_M * shelf_weight

        # Deliberate deep structure keeps its shape.
        quieted = 1.0 - 0.7 * _smooth(abs(tectonic_m) / 1200.0)
        return rough * quieted

    def offset_m(self, point, amplitude_m, resolution_m=None):
        """
        The roughness itself.

        Args:
            point (SpherePoint): Where.
            amplitude_m (float): From `amplitude_m`.
            resolution_m (float, optional): How far apart the samples are. None for
                canonical ground truth - every configured octave, down to
                `CANONICAL_WAVELENGTH_M`.

        Returns:
            metres (float): To be added to the structural elevation.

        Notes:
            **Octaves fade rather than switch off.** Dropping one the instant it becomes
            unrepresentable would be a cliff in *resolution* rather than in position - the
            ground would jump as somebody zoomed, which is the same bug M1.4 kept
            producing, in a different axis. Each octave dims between twice and four times
            the sample spacing and is gone by the far end.

            Sub-sample frequencies are not merely wasted work. They alias: an octave
            shorter than the spacing lands somewhere different in every grid, so a chart
            would shimmer as a ship moved rather than showing generalised ground.

        """
        if amplitude_m <= 0.0:
            return 0.0

        vector = point.vector
        total = 0.0
        for wavelength, frequency, share in self._bands:
            if resolution_m:
                visible = _smooth(
                    (wavelength / resolution_m - BARELY_M) / (CLEARLY_M - BARELY_M)
                )
                if visible <= 0.0:
                    # Everything finer is finer still, so nothing below can be visible.
                    break
            else:
                visible = 1.0
            total += (
                self._noise.at(
                    vector.x * frequency, vector.y * frequency, vector.z * frequency
                )
                - 0.5
            ) * 2.0 * share * visible
        return total * amplitude_m
