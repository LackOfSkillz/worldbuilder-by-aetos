"""
Four maps of the macro world, of which one matters.

The tectonic-contribution map is the diagnostic worth having. Land and final elevation
look plausible almost whatever happens underneath them; the contribution map, showing only
what the plates did and nothing else, is where a mistake has nowhere to hide. Glowing rings
around every cell, uplift on the wrong side of a trench, widths that change for no reason,
a discontinuity at a triple junction - all obvious there and all invisible in a finished
render.

Also reports the branch frequencies, because the cost of an expensive path matters far
less than how often anything takes it.

Run directly:  python -m worldbuilder.debug.macro_map
"""

import math

from ..geometry.sphere import SpherePoint
from ..plates.generation import plates_for
from ..plates.kinematics import CONVERGENT, DIVERGENT, motion_at
from ..terrain.continentality import Continentality
from ..terrain.tectonics import MAX_TECTONIC_RANGE_M, Tectonics

WIDTH, HEIGHT = 720, 360


def _sweep(width, height):
    for row in range(height):
        latitude = 90.0 - 180.0 * (row + 0.5) / height
        for column in range(width):
            longitude = -180.0 + 360.0 * (column + 0.5) / width
            yield row, column, latitude, SpherePoint.from_latlon(latitude, longitude)


def _ground(metres):
    if metres >= 0.0:
        rise = min(1.0, metres / 2200.0)
        return (int(70 + 150 * rise), int(105 + 100 * rise), int(60 + 70 * rise))
    depth = min(1.0, -metres / 6500.0)
    return (
        int(20 + 25 * (1 - depth)),
        int(60 + 95 * (1 - depth)),
        int(90 + 120 * (1 - depth)),
    )


def _contribution(metres):
    """Red where the plates lift the ground, blue where they drop it, black for
    neither."""
    if metres > 0.0:
        weight = min(1.0, metres / 1600.0)
        return (int(30 + 225 * weight), int(30 + 60 * weight), 30)
    if metres < 0.0:
        weight = min(1.0, -metres / 2600.0)
        return (30, int(30 + 60 * weight), int(30 + 225 * weight))
    return (18, 18, 18)


def _write(path, width, height, pixels):
    with open(path, "wb") as handle:
        handle.write(f"P6\n{width} {height}\n255\n".encode("ascii"))
        handle.write(bytes(pixels))


def render(world_seed, width=WIDTH, height=HEIGHT, into="."):
    plates = plates_for(world_seed)
    land = Continentality(world_seed)
    tectonics = Tectonics(plates, land)

    base = bytearray(width * height * 3)
    offset = bytearray(width * height * 3)
    final = bytearray(width * height * 3)

    dry_before = dry_after = area_total = 0.0
    branches = {"interior": 0, "near margin": 0, "active": 0}
    biggest_up = biggest_down = 0.0

    for row, column, latitude, point in _sweep(width, height):
        at = (row * width + column) * 3
        area = math.cos(math.radians(latitude))
        area_total += area

        was = land.base_elevation(point)
        lift = tectonics.offset_m(point)
        now = was + lift

        base[at : at + 3] = bytes(_ground(was))
        offset[at : at + 3] = bytes(_contribution(lift))
        final[at : at + 3] = bytes(_ground(now))

        if was >= 0.0:
            dry_before += area
        if now >= 0.0:
            dry_after += area
        biggest_up = max(biggest_up, lift)
        biggest_down = min(biggest_down, lift)

        margin = plates.margin_at(point)
        if margin.distance_m >= MAX_TECTONIC_RANGE_M:
            branches["interior"] += 1
        else:
            motion = motion_at(point, plates)
            if motion and motion.kind in (CONVERGENT, DIVERGENT):
                branches["active"] += 1
            else:
                branches["near margin"] += 1

    written = []
    for name, pixels in (("base", base), ("tectonic", offset), ("macro", final)):
        path = f"{into}/{name}-{world_seed}.ppm"
        _write(path, width, height, pixels)
        written.append(path)

    samples = sum(branches.values())
    print(f"  world {world_seed}")
    print(f"    land before tectonics   {100.0 * dry_before / area_total:5.1f} %")
    print(f"    land after              {100.0 * dry_after / area_total:5.1f} %")
    print(f"    largest uplift          {biggest_up:+8.0f} m")
    print(f"    deepest trench          {biggest_down:+8.0f} m")
    print("    which branch a sample takes:")
    for name, count in branches.items():
        print(f"      {name:14} {100.0 * count / samples:5.1f} %")
    for path in written:
        print(f"    wrote {path}")
    return 100.0 * dry_after / area_total


if __name__ == "__main__":
    render(20260831)
