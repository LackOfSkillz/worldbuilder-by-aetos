"""
Which plate a point is on, and how far it is from the edge of that plate.

Both answers are arithmetic on the plate seeds. **Nothing about the margins is stored** -
no boundary graph, no polygons, no spatial index. A margin is not an object here; it is a
fact about a point, worked out when somebody asks about that point.

The one thing precomputed is a table of bisector normals, one per pair of plates, and that
is a few hundred vectors rather than a description of the world.
"""

import math
from dataclasses import dataclass

from ..geometry.sphere import EARTH_RADIUS_M
from ..geometry.vectors import DEGENERATE

#: How wide the fade is when a third plate shadows a bisector, as a difference of dot
#: products. Small: a margin is genuine or it is not, and this only smooths the handover.
SHADOW_BLEND = 0.02


@dataclass(frozen=True)
class Margin:
    """
    Where a point stands relative to the edge of its plate.

    Attributes:
        nearest (Plate): The plate the point is on.
        neighbour (Plate): The plate across the nearest stretch of that edge.
        distance_m (float): Metres to it, along the surface.

    """

    nearest: object
    neighbour: object
    distance_m: float


class PlateSet:
    """
    Every plate on a world, with the arithmetic needed to ask questions about them.

    Notes:
        Holds one precomputed table: for each ordered pair of plates, the normal of the
        plane that bisects them. Points equidistant from seeds A and B satisfy
        `dot(P, A) == dot(P, B)`, which rearranges to `dot(P, A - B) == 0` - so the
        margin between two plates is a great circle whose plane normal is
        `normalise(A - B)`, and the distance from any point to it is an arc sine away.

        A couple of dozen plates makes a few hundred such vectors. That is the entire
        stored geometry of a planet's tectonics.

    """

    def __init__(self, plates):
        self.plates = tuple(plates)
        self._bisectors = tuple(
            tuple(
                None
                if other is plate or (plate.seed.vector - other.seed.vector).length() <= DEGENERATE
                else (plate.seed.vector - other.seed.vector).normalised()
                for other in self.plates
            )
            for plate in self.plates
        )

    def __len__(self):
        return len(self.plates)

    def __iter__(self):
        return iter(self.plates)

    def __getitem__(self, index):
        return self.plates[index]

    def nearest_two(self, point):
        """
        The two plates whose seeds are closest.

        Args:
            point (SpherePoint): Anywhere on the planet.

        Returns:
            pair (tuple): The nearest plate and the second nearest.

        Notes:
            Compared by dot product rather than by angle. For unit vectors a larger dot
            product *is* a smaller angle, so converting to distances would only be undone
            by the comparison - two dozen transcendental calls a sample, to sort numbers
            that were already in order.

        """
        best = second = None
        best_dot = second_dot = -2.0
        for plate in self.plates:
            alignment = point.vector.dot(plate.seed.vector)
            if alignment > best_dot:
                second, second_dot = best, best_dot
                best, best_dot = plate, alignment
            elif alignment > second_dot:
                second, second_dot = plate, alignment
        return best, second

    def margin_at(self, point, radius_m=EARTH_RADIUS_M):
        """
        How far a point is from the edge of the plate it is on.

        Args:
            point (SpherePoint): Anywhere on the planet.
            radius_m (float, optional): The planet's radius.

        Returns:
            margin (Margin): The plate, the one across the nearest edge, and the distance.

        Notes:
            **The minimum over every bisector of the nearest plate**, and it has to be.

            The obvious shortcut is to measure only the bisector with the *second nearest*
            seed, which is nearly always the right one and is a single arc sine. It is
            also discontinuous, and the walk-across-a-margin test caught it jumping by
            five hundred kilometres. The reason is that the distance to a bisector is
            `asin(dot(P, normalise(A - B)))`: when the second-nearest plate changes from B
            to C, the numerator is continuous but the normalisation is not, because
            `|A - B|` and `|A - C|` differ. Terrain built on that would have grown a wall
            wherever a third plate happened to become the runner-up.

            Taking the minimum over all of them is continuous because a minimum of
            continuous functions is continuous, and it is *also* the honest answer: it is
            the distance to the plate's actual edge rather than to one particular
            neighbour's bisector.

            The minimum is taken on the sine rather than the angle. Arc sine is monotonic
            over the range in question, so the smallest sine is the smallest angle, and
            one transcendental call at the end does for the lot.

        """
        nearest, _ = self.nearest_two(point)
        if len(self.plates) < 2:
            return Margin(nearest=nearest, neighbour=None, distance_m=float("inf"))

        closest_sine = 2.0
        across = None
        for other, normal in zip(self.plates, self._bisectors[nearest.index]):
            if normal is None:
                continue
            offset = abs(point.vector.dot(normal))
            if offset < closest_sine:
                closest_sine = offset
                across = other

        return Margin(
            nearest=nearest,
            neighbour=across,
            distance_m=math.asin(min(1.0, closest_sine)) * radius_m,
        )

    def margins_within(self, point, range_m, radius_m=EARTH_RADIUS_M):
        """
        Every margin of this point's plate that is near enough to matter.

        Args:
            point (SpherePoint): Anywhere on the planet.
            range_m (float): How far a margin may be and still count.
            radius_m (float, optional): The planet's radius.

        Returns:
            found (tuple): The plate the point is on, and a list of
                `(other_plate, distance_m, bisector_normal, weight)` for each margin in
                range. The weight fades a margin out where a third plate shadows it.

        Notes:
            **Because picking one margin is not continuous, even when its distance is.**
            `margin_at` returns a distance that varies smoothly, but the *identity* of the
            neighbour it belongs to still jumps: at a point equidistant from two of a
            plate's margins, which one is "the" margin flips under a step of a metre, and
            everything derived from it - the normal, the relative motion, what lies either
            side - flips with it. Terrain built on that gained five hundred metres of cliff
            wherever a plate had two margins the same distance away.

            The honest answer is that both margins are there. A caller that sums their
            effects is continuous, because each contribution depends on its own distance
            and each fades out at its own range.

            Costs nothing extra where nothing is happening: the distance test is a dot
            product and a comparison, and a plate interior fails all of them.

        """
        nearest, _ = self.nearest_two(point)
        if len(self.plates) < 2:
            return nearest, ()

        # Compared as sines, so the arc sine is paid only for the few that are in range.
        limit = math.sin(min(math.pi / 2, range_m / radius_m))
        found = []
        for other, normal in zip(self.plates, self._bisectors[nearest.index]):
            if normal is None:
                continue
            offset = abs(point.vector.dot(normal))
            if offset > limit:
                continue

            # Is this bisector actually a margin here, or is a third plate in the way?
            #
            # Two seeds always have a bisector, but it is only part of the cell boundary
            # where those two are genuinely the nearest pair. Elsewhere it runs through
            # some other plate's territory, imaginary. Summing those cost a hundred and
            # seventy kilometres of phantom mountain range, and worse, it was
            # discontinuous: crossing from plate 5 to plate 0 swapped bisector(5,8) for
            # bisector(0,8) - different planes with no reason to agree - for two hundred
            # and sixty metres of cliff.
            #
            # The test is to stand at the closest point on the bisector and ask who the
            # neighbours are. One extra lookup, paid only for candidates already in range,
            # and none at all in a plate interior.
            foot = point.vector - normal.scaled(point.vector.dot(normal))
            if foot.length() <= DEGENERATE:
                continue
            standing = foot.normalised()

            # How far a third plate would have to be for this to be a real margin, against
            # how far the nearest one actually is. Positive means genuine; negative means
            # somebody else's territory.
            mine = standing.dot(nearest.seed.vector)
            shadow = 2.0
            for third in self.plates:
                if third.index in (nearest.index, other.index):
                    continue
                shadow = min(shadow, mine - standing.dot(third.seed.vector))

            # **A weight, not a test.** The first version rejected shadowed bisectors with
            # a boolean, and that switched a margin on and off in one step wherever it
            # ended at a triple junction - a hundred and forty metres of cliff. The third
            # time the same mistake appeared in this phase: a hard decision taken on a
            # continuous quantity. It fades now.
            genuine = min(1.0, max(0.0, shadow / SHADOW_BLEND))
            if genuine <= 0.0:
                continue
            genuine = genuine * genuine * (3.0 - 2.0 * genuine)

            found.append(
                (other, math.asin(min(1.0, offset)) * radius_m, normal, genuine)
            )
        return nearest, tuple(found)

    def flattened(self, point, normal):
        """
        A bisector normal laid flat on the surface at a point.

        Returns:
            normal (Vec3 or None): Unit vector across the margin, tangent to the sphere.

        """
        flat = normal - point.vector.scaled(point.vector.dot(normal))
        if flat.length() <= DEGENERATE:
            return None
        return flat.normalised()

    def margin_normal(self, point, margin):
        """
        Which way is across the margin, in the tangent plane at this point.

        Args:
            point (SpherePoint): Where to measure.
            margin (Margin): From `margin_at`.

        Returns:
            normal (Vec3 or None): Unit vector pointing from the neighbour's side towards
                the nearest plate's side, lying flat on the surface. None if there is no
                neighbour, or at the two points where the direction is undefined.

        Notes:
            Wanted by the kinematics, which need to know whether two plates approach each
            other *across* their margin or slide *along* it. The bisector's plane normal
            is already perpendicular to the margin; what is returned is its component in
            the tangent plane, which is what "away from the margin" means to somebody
            standing there.

        """
        if margin.neighbour is None:
            return None
        normal = self._bisectors[margin.nearest.index][margin.neighbour.index]
        if normal is None:
            return None
        flattened = normal - point.vector.scaled(point.vector.dot(normal))
        if flattened.length() <= DEGENERATE:
            return None
        return flattened.normalised()
