"""
Pictures of the plate model, for looking at rather than for shipping.

Three equirectangular images: which plate each point belongs to, how far it is from a
margin, and what the two plates there are doing to each other. None of this is product
output and none of it is on any runtime path.

Written as binary PPM, which every image viewer opens and which needs no library at all.
A diagnostic that dragged in a dependency would be a diagnostic somebody eventually could
not run.

Run directly:  python -m worldbuilder.debug.plate_map
"""

import math

from ..geometry.sphere import SpherePoint
from ..plates.generation import DEFAULT_PLATE_COUNT, plates_for
from ..plates.kinematics import CONVERGENT, DIVERGENT, motion_at

#: Big enough to see the cells and the poles, small enough to write in a moment.
WIDTH, HEIGHT = 720, 360

#: How far from a margin still counts as near it, for the greyscale image.
MARGIN_SCALE_M = 600_000.0


def _sweep(width, height):
    """Every pixel of an equirectangular image, as points on the globe."""
    for row in range(height):
        latitude = 90.0 - 180.0 * (row + 0.5) / height
        for column in range(width):
            longitude = -180.0 + 360.0 * (column + 0.5) / width
            yield row, column, SpherePoint.from_latlon(latitude, longitude)


def _colour_for(index):
    """A distinct enough colour per plate. Not a palette anybody has to like."""
    hue = (index * 0.618033988749895) % 1.0
    sector = int(hue * 6) % 6
    fade = hue * 6 - int(hue * 6)
    bright, dim, rising = 235, 60, int(60 + 175 * fade)
    falling = int(235 - 175 * fade)
    return [
        (bright, rising, dim), (falling, bright, dim), (dim, bright, rising),
        (dim, falling, bright), (rising, dim, bright), (bright, dim, falling),
    ][sector]


def _write(path, width, height, pixels):
    with open(path, "wb") as handle:
        handle.write(f"P6\n{width} {height}\n255\n".encode("ascii"))
        handle.write(bytes(pixels))


def render(world_seed, count=DEFAULT_PLATE_COUNT, width=WIDTH, height=HEIGHT, into="."):
    """
    Args:
        world_seed (int): Which world.
        count (int, optional): How many plates.
        width (int, optional): Image width.
        height (int, optional): Image height.
        into (str, optional): Where to write.

    Returns:
        written (list): The paths written.

    """
    plates = plates_for(world_seed, count)
    cells = bytearray(width * height * 3)
    margins = bytearray(width * height * 3)
    kinds = bytearray(width * height * 3)

    for row, column, point in _sweep(width, height):
        at = (row * width + column) * 3

        nearest, _ = plates.nearest_two(point)
        cells[at : at + 3] = bytes(_colour_for(nearest.index))

        distance = plates.margin_at(point).distance_m
        near = max(0.0, 1.0 - min(1.0, distance / MARGIN_SCALE_M))
        shade = int(255 * near)
        margins[at : at + 3] = bytes((shade, shade, shade))

        motion = motion_at(point, plates)
        if motion is None:
            colour = (20, 20, 20)
        elif motion.kind == CONVERGENT:
            colour = (int(60 + 195 * near), 40, 40)      # red where plates collide
        elif motion.kind == DIVERGENT:
            colour = (40, 40, int(60 + 195 * near))      # blue where they part
        else:
            colour = (int(40 + 120 * near),) * 3         # grey where they slide
        kinds[at : at + 3] = bytes(colour)

    written = []
    for name, pixels in (("plates", cells), ("margins", margins), ("motion", kinds)):
        path = f"{into}/{name}-{world_seed}.ppm"
        _write(path, width, height, pixels)
        written.append(path)
    return written


def summarise(world_seed, count=DEFAULT_PLATE_COUNT):
    """A few numbers about a world's plates, for the terminal."""
    plates = plates_for(world_seed, count)
    print(f"  world {world_seed}: {len(plates)} plates\n")
    print(f"  {'plate':>5}  {'seed lat':>9} {'seed lon':>9}  "
          f"{'pole lat':>9} {'pole lon':>9}  {'rad/Myr':>9}")
    for plate in plates[:8]:
        seed_lat, seed_lon = plate.seed.to_latlon()
        pole_lat, pole_lon = plate.euler_pole.to_latlon()
        print(f"  {plate.index:5}  {seed_lat:9.2f} {seed_lon:9.2f}  "
              f"{pole_lat:9.2f} {pole_lon:9.2f}  {plate.rate_rad_per_myr:9.5f}")
    if len(plates) > 8:
        print(f"  {'...':>5}")

    tally = {}
    for _, _, point in _sweep(180, 90):
        motion = motion_at(point, plates)
        if motion:
            tally[motion.kind] = tally.get(motion.kind, 0) + 1
    total = sum(tally.values())
    print("\n  margins, by what they are doing:")
    for kind, number in sorted(tally.items()):
        print(f"    {kind:12} {100.0 * number / total:5.1f} %")


if __name__ == "__main__":
    summarise(20260831)
    print()
    for path in render(20260831):
        print(f"  wrote {path}")
