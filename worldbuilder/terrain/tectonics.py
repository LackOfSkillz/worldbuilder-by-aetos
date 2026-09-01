"""
What plate motion does to the ground.

Three rules shape this file, and all three are about what it refuses to do.

**It returns a contribution, never an elevation.** `offset_m` is a number to *add* to the
continental base, so that shelves, erosion, bathymetry and detail can all compose with
tectonics later instead of reverse-engineering what tectonics already overwrote. A
function that set an absolute height would have made every one of those layers harder to
write, and the damage would not have been visible until they were being written.

**It does the expensive work only where it matters.** Most of a planet is nowhere near a
margin, and the cost measurements from earlier phases are unambiguous: kinematics are
6.6 microseconds a sample and continentality gradients 33, against 5 for finding the
margin at all. So a point far from any boundary returns zero having done nothing but the
lookup it had to do anyway. Progressive enrichment - cheap context, then a question, then
expensive context - rather than assembling everything and discovering later what was
needed.

**It has no crust model, and does not pretend to.** Whether a margin is oceanic or
continental is answered by sampling continentality either side of it, which is crude and
sufficient: the same convergent margin can then behave differently along its length,
because the land either side of it differs along its length. When a real crust field
arrives it replaces the two probes and nothing else changes - the shapers never learn
where the answer came from.
"""

import math
from dataclasses import dataclass

from ..geometry.sphere import EARTH_RADIUS_M
from ..geometry.tangent import TangentFrame
from ..plates.kinematics import ACROSS_ENOUGH, motion_between

#: Beyond this, a margin does nothing at all and no kinematics are evaluated. Every
#: profile below must reach exactly zero by here, or the gate itself becomes a cliff.
MAX_TECTONIC_RANGE_M = 420_000.0

#: How far either side of the margin to ask what kind of ground it is. Far enough to be
#: clear of the transition, near enough to describe this stretch of margin rather than
#: the continent behind it.
PROBE_M = 300_000.0

#: Continentality at which a side counts as half continental, and how wide the transition
#: from oceanic to continental is. **A width rather than a threshold, and that matters.**
#:
#: The first version used a hard test - continental if above zero - and the ground jumped
#: five hundred and fifty metres wherever a margin crossed it, because the two sides of the
#: test run entirely different profiles. It is the same mistake M1.2 made: a hard selection
#: on a continuous quantity. The branches are blended now, and this is how far it takes.
CONTINENTAL_ENOUGH = 0.0
CONTINENTAL_BLEND = 0.45

#: How sharply the margin picks a side. The profiles are asymmetric - a trench belongs on
#: the ocean side - so they need to know which side is which, and that answer must also
#: arrive continuously. Where the two sides are equally continental it goes smoothly to
#: nothing, which is correct: a symmetric margin has no side to prefer.
SIDE_SHARPNESS = 6.0

#: Closing speed that counts as a thoroughly active margin, in metres per million years.
#: Two plates at four centimetres a year approaching head-on. Faster than this does not
#: build higher mountains; it just saturates.
FULL_RATE_M_PER_MYR = 80_000.0

#: The profiles. Height in metres, width in metres, and where the feature sits relative
#: to the margin itself - a trench lies out on the oceanic side, an arc a little inboard.
CONTINENT_COLLISION_M = 1500.0
CONTINENT_COLLISION_WIDTH_M = 400_000.0

COASTAL_UPLIFT_M = 900.0
COASTAL_UPLIFT_WIDTH_M = 260_000.0

TRENCH_M = -2600.0
TRENCH_WIDTH_M = 120_000.0
TRENCH_OFFSET_M = 90_000.0

ISLAND_ARC_M = 700.0
ISLAND_ARC_WIDTH_M = 110_000.0
ISLAND_ARC_OFFSET_M = 60_000.0

RIDGE_M = 900.0
RIDGE_WIDTH_M = 380_000.0
RIFT_M = -350.0
RIFT_WIDTH_M = 70_000.0


def _continental(value):
    """
    Args:
        value (float): Continentality on one side of a margin.

    Returns:
        weight (float): Nothing for thoroughly oceanic, one for thoroughly continental,
            and a smooth ramp between.

    """
    fraction = (value - CONTINENTAL_ENOUGH) / CONTINENTAL_BLEND * 0.5 + 0.5
    fraction = max(0.0, min(1.0, fraction))
    return fraction * fraction * (3.0 - 2.0 * fraction)


def _bump(distance_m, width_m):
    """
    A smooth hump: one at the centre, nothing at the edge, and no corner anywhere.

    Args:
        distance_m (float): How far from the middle of the feature.
        width_m (float): Where it reaches zero.

    Returns:
        weight (float): Between zero and one.

    Notes:
        Smoothstep rather than a cosine or a straight taper, because it is flat at both
        ends: the derivative is zero at the centre *and* at the edge. A profile that
        merely reached zero would still leave a crease where it met the untouched ground,
        and a crease in terrain is a cliff somebody sails into.

    """
    if width_m <= 0.0:
        return 0.0
    away = min(1.0, abs(distance_m) / width_m)
    fade = 1.0 - away
    return fade * fade * (3.0 - 2.0 * fade)


@dataclass(frozen=True)
class Setting:
    """
    What kind of ground lies either side of a margin, here.

    Attributes:
        inboard (float): Continentality on the nearest plate's side.
        outboard (float): Continentality on the neighbour's side.

    """

    inboard: float
    outboard: float

    @property
    def inboard_continental(self):
        """How continental the near side is, from nothing to one, smoothly."""
        return _continental(self.inboard)

    @property
    def outboard_continental(self):
        return _continental(self.outboard)

    @property
    def lean(self):
        """
        Which side is the more continental, from -1 to +1, and how decidedly.

        Notes:
            Near zero where the two sides are alike, which is what lets an asymmetric
            profile fade out rather than flip. A hard comparison here would have put a
            trench on one side of a symmetric margin and the other side of it a metre
            away.

        """
        return math.tanh((self.inboard - self.outboard) * SIDE_SHARPNESS)


class Tectonics:
    """
    The tectonic contribution to elevation, worked out where it matters and nowhere else.

    Notes:
        Holds the plates and the continentality field and combines them. It is the first
        thing in the engine that knows about both, which is deliberate - they were built
        in ignorance of each other so that continents would not inherit plate shapes, and
        this is the seam where they are allowed to meet.

    """

    def __init__(self, plates, continentality, radius_m=EARTH_RADIUS_M):
        self.plates = plates
        self.land = continentality
        self.radius_m = radius_m

    def setting_at(self, point, distance_m, normal):
        """
        What lies either side of the margin near this point.

        Args:
            point (SpherePoint): Where.
            distance_m (float): How far the margin is.
            normal (Vec3): Away from the margin, into the nearest plate.

        Returns:
            setting (Setting): Continentality on each side.

        Notes:
            The probes are placed relative to the *margin*, not to the point, so that two
            samples on opposite sides of the same boundary describe the same stretch of it
            and agree about what it is. Probing outward from each point instead would have
            let a margin be a subduction zone from one side and a collision from the other.

        """
        frame = TangentFrame.at(point, self.radius_m)
        east = normal.dot(frame.east)
        north = normal.dot(frame.north)

        # Walk back to the margin, then out to either side of it.
        to_inboard = -distance_m + PROBE_M
        to_outboard = -distance_m - PROBE_M
        return Setting(
            inboard=self.land.at(
                frame.local_to_sphere(east * to_inboard, north * to_inboard)
            ),
            outboard=self.land.at(
                frame.local_to_sphere(east * to_outboard, north * to_outboard)
            ),
        )

    def offset_m(self, point):
        """
        How much the plates raise or lower the ground here.

        Args:
            point (SpherePoint): Anywhere on the planet.

        Returns:
            metres (float): To be *added* to the continental base elevation.

        Notes:
            **Every margin in range, summed - not the nearest one, chosen.**

            Picking the nearest margin is not continuous even though its distance is. The
            identity of the neighbour jumps: at a point equidistant from two of a plate's
            margins the choice flips under a step of a metre, and the relative motion,
            the normal and what lies either side all flip with it. Measured at five
            hundred and sixty metres of cliff, a hundred and thirty kilometres from any
            boundary, where one margin was transform and the other divergent.

            Summing is continuous because each term depends only on its own distance and
            fades to nothing at its own range. It is also the truer answer: near a triple
            junction there really are two margins acting on the ground.

            Costs nothing where nothing is happening. A plate interior fails the distance
            test on every bisector, having done one dot product each - and that is 69 per
            cent of the planet.

        """
        nearest, margins = self.plates.margins_within(
            point, MAX_TECTONIC_RANGE_M, self.radius_m
        )
        if not margins:
            return 0.0

        total = 0.0
        for other, distance_m, bisector, weight in margins:
            normal = self.plates.flattened(point, bisector)
            if normal is None:
                continue
            total += weight * self._from_margin(point, nearest, other, distance_m, normal)
        return total

    def _from_margin(self, point, near, far, distance_m, normal):
        """
        One margin's contribution to the ground here.

        Args:
            point (SpherePoint): Where.
            near (Plate): The plate the point is on.
            far (Plate): The plate across this margin.
            distance_m (float): How far the margin is.
            normal (Vec3): Across it, tangent to the surface, pointing towards `near`.

        Returns:
            metres (float): Which may be zero, and usually is.

        """
        motion = motion_between(near, far, point, normal, self.radius_m)

        # How much of the relative motion is across the margin rather than along it, from
        # -1 (pulling apart) through 0 (pure sliding) to +1 (head on).
        #
        # **Weighed, not classified.** `motion.kind` is a name given by a threshold, and
        # using the name to pick a profile meant a margin drifting from convergent to
        # transform went from a full mountain belt to nothing in one step. The name
        # survives for diagnostics; the terrain uses the number.
        speed = math.hypot(motion.closing_m_per_myr, motion.sliding_m_per_myr)
        if speed <= 0.0:
            return 0.0
        across = motion.closing_m_per_myr / speed

        # A transform margin still leaves no mark. It arrives at no mark smoothly.
        engagement = (abs(across) - ACROSS_ENOUGH) / (1.0 - ACROSS_ENOUGH)
        if engagement <= 0.0:
            return 0.0
        engagement = min(1.0, engagement)
        engagement = engagement * engagement * (3.0 - 2.0 * engagement)

        strength = min(1.0, speed / FULL_RATE_M_PER_MYR) * engagement
        if strength <= 0.0:
            return 0.0

        if across < 0.0:
            # Pulling apart. Symmetric about the axis, so it needs no sense of side.
            return strength * (
                RIDGE_M * _bump(distance_m, RIDGE_WIDTH_M)
                + RIFT_M * _bump(distance_m, RIFT_WIDTH_M)
            )

        setting = self.setting_at(point, distance_m, normal)
        inboard = setting.inboard_continental
        outboard = setting.outboard_continental
        collision = inboard * outboard
        oceanic = (1.0 - inboard) * (1.0 - outboard)
        subduction = max(0.0, 1.0 - collision - oceanic)

        def profile(across_m):
            """The convergent response at a signed distance across the margin."""
            collided = CONTINENT_COLLISION_M * _bump(across_m, CONTINENT_COLLISION_WIDTH_M)
            trench = TRENCH_M * _bump(across_m + TRENCH_OFFSET_M, TRENCH_WIDTH_M)
            arc = ISLAND_ARC_M * _bump(across_m - ISLAND_ARC_OFFSET_M, ISLAND_ARC_WIDTH_M)
            uplift = COASTAL_UPLIFT_M * _bump(across_m - 70_000.0, COASTAL_UPLIFT_WIDTH_M)
            return (
                collision * collided
                + oceanic * (arc + trench)
                + subduction * (uplift + trench)
            )

        # Which side of the margin this point is on, weighed rather than decided.
        #
        # The obvious form is `signed = distance * lean`, and it is wrong in a way that
        # took a diagnostic to see: scaling the axis *compresses distance*, so with a lean
        # of -0.22 a point four hundred and nineteen kilometres out mapped to -90 km,
        # which is exactly where the trench sits. The trench fired at four hundred
        # kilometres and the range gate then cut it off mid-profile.
        #
        # The distance stays true. The profile is evaluated on both sides and blended by
        # the lean, which keeps every feature at its intended range and reaches zero by
        # the gate because each profile does.
        toward = (1.0 + setting.lean) * 0.5
        return strength * (
            toward * profile(distance_m) + (1.0 - toward) * profile(-distance_m)
        )

    def elevation_m(self, point):
        """
        The macro elevation: continental base plus whatever the plates have done to it.

        Args:
            point (SpherePoint): Anywhere on the planet.

        Returns:
            metres (float): Relative to datum, before shelves or detail.

        """
        return self.land.base_elevation(point) + self.offset_m(point)
