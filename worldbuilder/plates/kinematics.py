"""
How fast the ground is moving, and what that does where two plates meet.

This is where Euler poles earn their place over drift vectors. A plate turning about an
axis has a different surface velocity everywhere - fast at the equator of its own
rotation, nothing at all at its pole - and that variation is not a detail. It is why one
margin can be pulling apart at one end and grinding sideways at the other, which is what
makes a generated world's geology look like it has reasons.

**Nothing here is stored.** A margin is not classified once and remembered; it is worked
out at the point somebody asks about, from the two plates' motion there.
"""

from dataclasses import dataclass

from ..geometry.sphere import EARTH_RADIUS_M

#: How much of the relative motion must be across the margin rather than along it before
#: the margin is called convergent or divergent rather than transform. Sine of thirty
#: degrees: below that, the plates are mostly sliding past one another.
ACROSS_ENOUGH = 0.5

CONVERGENT = "convergent"
DIVERGENT = "divergent"
TRANSFORM = "transform"


@dataclass(frozen=True)
class Motion:
    """
    What the two plates at a point are doing to each other.

    Attributes:
        margin (Margin): Which plates, and how far away their margin is.
        closing_m_per_myr (float): How fast they approach across the margin. Negative
            means they are separating.
        sliding_m_per_myr (float): How fast they move along it, unsigned.
        kind (str): `convergent`, `divergent` or `transform`.

    """

    margin: object
    closing_m_per_myr: float
    sliding_m_per_myr: float
    kind: str


def surface_velocity(plate, point, radius_m=EARTH_RADIUS_M):
    """
    How fast the ground of one plate is moving at a point.

    Args:
        plate (Plate): The plate.
        point (SpherePoint): Where on the planet.
        radius_m (float, optional): The planet's radius.

    Returns:
        velocity (Vec3): Metres per million years, tangent to the surface.

    Notes:
        The cross product of the rotation vector with the position, scaled by the radius.
        It is automatically tangent to the sphere, and automatically zero at the plate's
        own Euler pole, without either being a special case anybody had to write.

    """
    return plate.angular_velocity().cross(point.vector).scaled(radius_m)


def motion_between(near, far, point, normal, radius_m=EARTH_RADIUS_M):
    """
    What two named plates are doing to each other at a point.

    Args:
        near (Plate): The plate the point is on.
        far (Plate): The plate across the margin.
        point (SpherePoint): Where.
        normal (Vec3): Across the margin, tangent to the surface, pointing towards `near`.
        radius_m (float, optional): The planet's radius.

    Returns:
        motion (Motion): Closing and sliding speeds, and a name for them.

    Notes:
        Split out from `motion_at` so a caller can ask about a margin it has chosen rather
        than the one that happens to be nearest - which is what lets several margins be
        summed instead of one being picked.

    """
    relative = surface_velocity(near, point, radius_m) - surface_velocity(
        far, point, radius_m
    )
    closing = -relative.dot(normal)
    along = relative - normal.scaled(relative.dot(normal))
    sliding = along.length()

    speed = relative.length()
    if speed <= 0.0 or abs(closing) / speed < ACROSS_ENOUGH:
        kind = TRANSFORM
    elif closing > 0.0:
        kind = CONVERGENT
    else:
        kind = DIVERGENT
    return Motion(
        margin=None, closing_m_per_myr=closing, sliding_m_per_myr=sliding, kind=kind
    )


def motion_at(point, plates, radius_m=EARTH_RADIUS_M):
    """
    What is happening across the nearest plate edge, here.

    Args:
        point (SpherePoint): Anywhere on the planet.
        plates (PlateSet): Every plate on the world.
        radius_m (float, optional): The planet's radius.

    Returns:
        motion (Motion or None): None only if there are not two plates to compare.

    Notes:
        The classification is derived and thrown away. Storing it would mean deciding
        once, for a whole margin, what is only true at a point - and a margin that
        converges at one end and slides at the other is the normal case, not an edge one.

    """
    margin = plates.margin_at(point, radius_m)
    if margin.neighbour is None:
        return None

    normal = plates.margin_normal(point, margin)
    if normal is None:
        return None

    relative = surface_velocity(margin.nearest, point, radius_m) - surface_velocity(
        margin.neighbour, point, radius_m
    )

    # The normal points away from the margin, into the nearest plate. So the nearest
    # plate moving *along* it is moving away from the neighbour, and closing is the
    # negative of that.
    closing = -relative.dot(normal)

    along = relative - normal.scaled(relative.dot(normal))
    sliding = along.length()

    speed = relative.length()
    if speed <= 0.0 or abs(closing) / speed < ACROSS_ENOUGH:
        kind = TRANSFORM
    elif closing > 0.0:
        kind = CONVERGENT
    else:
        kind = DIVERGENT

    return Motion(
        margin=margin,
        closing_m_per_myr=closing,
        sliding_m_per_myr=sliding,
        kind=kind,
    )
