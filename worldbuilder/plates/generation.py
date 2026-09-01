"""
Where the plates are, and how fast they turn.

**Every value here is hashed, never drawn from a sequence.** A plate's pole and rate come
from `hash(world_seed, "plate", index, what)` rather than from successive calls to a
random number generator, and that is a hard requirement rather than a preference.

A generator that consumes a mutable sequence makes every plate depend on the order in
which plates were built and on how many values each one happened to take. Add a property
to a plate six weeks from now - a crust thickness, a colour, anything - and every
*subsequent* plate silently changes, because the sequence shifted under it. Worlds people
had sailed would quietly become different worlds.

Hashing removes the possibility rather than the temptation. Plate 7's pole depends on
nothing but the seed and the number 7.
"""

import hashlib
import math
import struct

from ..geometry.sphere import SpherePoint
from ..geometry.vectors import Vec3
from .lookup import PlateSet
from .model import Plate

#: How many plates, unless a world asks otherwise. Earth has seven or eight major ones and
#: a good many minor; a couple of dozen gives enough boundary to make varied geography
#: without cells so small that every coast is a margin.
DEFAULT_PLATE_COUNT = 22

#: How far a seed may be nudged off the even spiral, in radians. Enough to break the
#: regularity of the pattern, not enough to let two seeds collide and produce a sliver of
#: a plate that nothing sensible can be done with.
JITTER_RAD = 0.18

#: Plausible plate speeds, in radians per million years. At Earth's radius the upper end
#: is about ten centimetres a year, which is roughly the fastest real plates manage.
SLOWEST_RAD_PER_MYR = 0.002
FASTEST_RAD_PER_MYR = 0.016


def _fraction(world_seed, *parts):
    """
    A number in [0, 1) from a seed and a label, with no sequence anywhere.

    Args:
        world_seed (int): The world's seed.
        *parts: Anything identifying what is being asked for.

    Returns:
        fraction (float): Deterministic, and dependent only on the arguments.

    """
    key = "|".join(str(part) for part in (world_seed,) + parts).encode("utf-8")
    digest = hashlib.blake2b(key, digest_size=8).digest()
    return struct.unpack("<Q", digest)[0] / float(1 << 64)


def _spread(world_seed, index, count):
    """
    One seed position on the Fibonacci spiral, nudged.

    Args:
        world_seed (int): The world's seed.
        index (int): Which plate.
        count (int): How many there are.

    Returns:
        point (SpherePoint): Where the plate's seed sits.

    Notes:
        The spiral distributes points over a sphere about as evenly as anything simple
        manages, and - unlike scattering them at random - it cannot produce two seeds on
        top of each other, which would make a plate of no area.

        The jitter is then deterministic and small, because a perfectly even spiral is
        visibly a spiral: the cells line up in arcs that read as machinery.

    """
    golden = math.pi * (3.0 - math.sqrt(5.0))
    z = 1.0 - 2.0 * (index + 0.5) / count
    ring = math.sqrt(max(0.0, 1.0 - z * z))
    angle = golden * index

    point = Vec3(math.cos(angle) * ring, math.sin(angle) * ring, z)

    # Nudge along two arbitrary but deterministic tangent directions.
    nudge_a = (2.0 * _fraction(world_seed, "plate", index, "jitter-a") - 1.0) * JITTER_RAD
    nudge_b = (2.0 * _fraction(world_seed, "plate", index, "jitter-b") - 1.0) * JITTER_RAD
    sideways = Vec3(0.0, 0.0, 1.0).cross(point)
    if sideways.length() < 1e-9:
        sideways = Vec3(1.0, 0.0, 0.0).cross(point)
    east = sideways.normalised()
    north = point.cross(east)
    return SpherePoint.from_vector(point + east.scaled(nudge_a) + north.scaled(nudge_b))


def _pole(world_seed, index):
    """
    Args:
        world_seed (int): The world's seed.
        index (int): Which plate.

    Returns:
        pole (SpherePoint): An evenly distributed axis of rotation.

    Notes:
        Even over the sphere, which needs the z component to be uniform rather than the
        latitude - sampling latitude uniformly would crowd the poles, and a set of plates
        all turning about nearly the same axis would drift as one sheet.

    """
    z = 2.0 * _fraction(world_seed, "plate", index, "pole-z") - 1.0
    angle = 2.0 * math.pi * _fraction(world_seed, "plate", index, "pole-angle")
    ring = math.sqrt(max(0.0, 1.0 - z * z))
    return SpherePoint(Vec3(math.cos(angle) * ring, math.sin(angle) * ring, z))


def _rate(world_seed, index):
    """
    Returns:
        rate (float): Radians per million years, signed.

    Notes:
        The sign is what makes a rotation clockwise or otherwise, so it lives here rather
        than in a separate flag that could disagree with the pole.

    """
    speed = SLOWEST_RAD_PER_MYR + _fraction(world_seed, "plate", index, "rate") * (
        FASTEST_RAD_PER_MYR - SLOWEST_RAD_PER_MYR
    )
    turning = _fraction(world_seed, "plate", index, "sense") < 0.5
    return -speed if turning else speed


def plates_for(world_seed, count=DEFAULT_PLATE_COUNT):
    """
    Every plate on a world.

    Args:
        world_seed (int): The world's seed.
        count (int, optional): How many plates.

    Returns:
        plates (PlateSet): In index order, and identical on every call.

    Notes:
        A tuple of a couple of dozen small records - a few kilobytes, and the only thing
        about the planet's geology that is stored at all. Everything else is worked out
        from these when somebody asks.

    """
    return PlateSet(
        Plate(
            index=index,
            seed=_spread(world_seed, index, count),
            euler_pole=_pole(world_seed, index),
            rate_rad_per_myr=_rate(world_seed, index),
        )
        for index in range(count)
    )
