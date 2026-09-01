"""
What the bottom is made of.

Maritime asks two things of a world: how deep the water is, and what is under it. This is
the second. An anchor bites in mud and drags on rock; a hull that touches sand is aground
and one that touches rock is holed; a dredger can move one and not the other.

**A category is the wrong shape for the answer, and the right shape for the question.**
Everything in this engine is continuous because hard decisions on continuous quantities
make cliffs, and "sand" is about as hard a decision as exists. So the field is a
*composition* - three fractions summing to one, each varying smoothly - and the single-word
answer is whichever is largest. Nothing continuous is ever computed from the word.

That is not a dodge. Composition is what the physics actually wants: holding ground is a
matter of how much mud is in it, and a bottom that is three-quarters sand over rock behaves
like neither.

Derived from three things, none of which is stored:

    slope       fines do not stay on a steep bottom, so steep means rock
    depth       above wave base the sea winnows the fines out; below, they settle
    tectonics   ground the plates lifted is rock whatever its slope

And overridden, smoothly, by anything placed. A pinnacle is rock because somebody said so,
and a dredged basin is mud because that is what settles in still water behind a mole.
"""

import math

from ..geometry.tangent import TangentFrame

#: The three, and there are only three on purpose. Gravel, shell, weed, coral and clay are
#: all real and all wanted eventually, and every one of them is a fourth fraction rather
#: than a change of shape - which is the point of getting the shape right first.
SAND = "sand"
MUD = "mud"
ROCK = "rock"

#: How steep a bottom has to be before the fines are gone from it. Four per cent is a
#: steep seabed - four metres in a hundred - and a slope twice that is bare.
ROCK_SLOPE = 0.04

#: How much tectonic contribution makes ground rock regardless of how flat it is.
#:
#: Twelve hundred metres, which is the scale of real tectonic structure - a trench
#: wall, a ridge crest. At four hundred the demonstration coast came out a third rock
#: everywhere, because a passive margin carries a hundred and fifty metres of gentle
#: regional swell and a broad rise is not a rock face. What makes rock is steep
#: tectonic structure, and the slope term is already there to catch it.
ROCK_TECTONIC_M = 1200.0

#: Wave base, and how far below it the fines have finished settling. Above the first
#: figure the sea keeps the bottom swept and sandy; below the second it is mud.
SWEPT_M = -40.0
SETTLED_M = -120.0

#: How far apart the two probes are that measure the slope.
#:
#: **Small enough to resolve the narrowest thing anybody placed**, which is the only
#: constraint that binds. At six hundred metres a finite difference straddles a
#: hundred-and-forty-metre pinnacle: the bottom a hundred and thirty metres from a rock
#: standing twenty metres proud read perfectly flat, because both probes missed it,
#: while the bottom three hundred metres away read steep because one probe landed on
#: it. That is an aliased rock field, and a chart drawn from it would scatter rock
#: patches in rings round every hazard.
#:
#: There is no opposing constraint, because this probes `structural_m`, which carries
#: no detail. Measured across the planet, the structural slope distribution is
#: identical at three hundred, six hundred and two thousand metres - structure is
#: smooth at every scale - so a short baseline costs nothing and buys resolution.
SLOPE_BASELINE_M = 60.0


def _smooth(fraction):
    clamped = max(0.0, min(1.0, fraction))
    return clamped * clamped * (3.0 - 2.0 * clamped)


class Composition:
    """
    What a piece of bottom is made of, as fractions that sum to one.

    Attributes:
        sand (float): Nothing to one.
        mud (float): Nothing to one.
        rock (float): Nothing to one.

    """

    __slots__ = ("sand", "mud", "rock")

    def __init__(self, sand, mud, rock):
        total = sand + mud + rock
        if total <= 0.0:
            sand, mud, rock, total = 0.0, 0.0, 1.0, 1.0
        self.sand, self.mud, self.rock = sand / total, mud / total, rock / total

    @property
    def dominant(self):
        """The one-word answer, for callers that want one."""
        if self.rock >= self.sand and self.rock >= self.mud:
            return ROCK
        return SAND if self.sand >= self.mud else MUD

    def holding(self):
        """
        How well an anchor holds here, nothing to one.

        Notes:
            Mud holds best of the three, sand holds moderately, and rock does not hold at
            all - an anchor on rock either finds a crevice or drags. Expressed from the
            fractions rather than from `dominant`, because a bottom that is half rock is
            genuinely half as good and a word cannot say that.

        """
        return self.mud * 1.0 + self.sand * 0.6

    def blended_towards(self, other, weight):
        """This composition moved some of the way towards another one."""
        keep = 1.0 - weight
        return Composition(
            self.sand * keep + other.sand * weight,
            self.mud * keep + other.mud * weight,
            self.rock * keep + other.rock * weight,
        )

    def __repr__(self):
        return (f"Composition(sand={self.sand:.2f}, mud={self.mud:.2f}, "
                f"rock={self.rock:.2f})")


PURE = {
    SAND: Composition(1.0, 0.0, 0.0),
    MUD: Composition(0.0, 1.0, 0.0),
    ROCK: Composition(0.0, 0.0, 1.0),
}


class Substrate:
    """
    The bottom composition of a world, derived from its shape.

    Notes:
        Holds nothing. Every answer is computed from the surface it was given, which is
        the same rule the rest of the engine follows and for the same reason: a stored
        substrate map fine enough to matter is far too large for a planet.

    """

    def __init__(self, surface):
        self.surface = surface

    def slope_at(self, point, baseline_m=SLOPE_BASELINE_M):
        """
        How steep the ground is, as a rise over a run.

        Args:
            point (SpherePoint): Where.
            baseline_m (float, optional): How far apart the probes are.

        Returns:
            slope (float): Dimensionless. Nothing on a flat bottom.

        Notes:
            Four probes and a frame, which is the expensive part of this module by a wide
            margin. It is affordable only because bottom type is asked far less often than
            depth - a ship anchors once and sounds continuously.

        """
        frame = TangentFrame.at(point, self.surface.radius_m)
        half = baseline_m * 0.5
        east = (
            self.surface.structural_m(frame.local_to_sphere(half, 0.0))
            - self.surface.structural_m(frame.local_to_sphere(-half, 0.0))
        ) / baseline_m
        north = (
            self.surface.structural_m(frame.local_to_sphere(0.0, half))
            - self.surface.structural_m(frame.local_to_sphere(0.0, -half))
        ) / baseline_m
        return math.hypot(east, north)

    def natural(self, elevation_m, slope, tectonic_m):
        """
        What ordinary ground here would be made of, before anything placed says otherwise.

        Args:
            elevation_m (float): The ground.
            slope (float): From `slope_at`.
            tectonic_m (float): The tectonic contribution.

        Returns:
            composition (Composition): Fractions summing to one.

        Notes:
            Rock is claimed first, because steepness and uplift both overrule deposition -
            fines cannot stay on a slope whatever the water is doing above it. What is
            left divides between sand and mud on depth alone.

        """
        rock = max(
            _smooth(slope / ROCK_SLOPE),
            _smooth(abs(tectonic_m) / ROCK_TECTONIC_M),
        )
        swept = _smooth((elevation_m - SETTLED_M) / (SWEPT_M - SETTLED_M))
        loose = 1.0 - rock
        return Composition(loose * swept, loose * (1.0 - swept), rock)

    def at(self, point, elevation_m=None, slope=None, tectonic_m=None):
        """
        What the bottom is made of here.

        Args:
            point (SpherePoint): Where.
            elevation_m (float, optional): The structural ground, if already to hand.
            slope (float, optional): The slope, if already to hand.
            tectonic_m (float, optional): The tectonic contribution, if already to hand.

        Returns:
            composition (Composition): Fractions summing to one.

        Notes:
            The optional arguments are the same bargain `Shelf.evaluate` struck. A caller
            that has already evaluated the ground should not pay for it twice, and a
            caller that has not should not have to know what this needs.

        """
        if elevation_m is None:
            elevation_m = self.surface.structural_m(point)
        if tectonic_m is None:
            tectonic_m = self.surface.tectonics.offset_m(point)
        if slope is None:
            slope = self.slope_at(point)

        composition = self.natural(elevation_m, slope, tectonic_m)
        for placed in self.surface.features.placed:
            declared = placed.feature.substrate
            if declared is None:
                continue
            weight = placed.weight_at(point)
            if weight > 0.0:
                composition = composition.blended_towards(PURE[declared], weight)
        return composition

    def dominant_at(self, point, **known):
        """The one-word answer, which is what the maritime interface asks for."""
        return self.at(point, **known).dominant
