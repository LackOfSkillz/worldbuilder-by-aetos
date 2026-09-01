"""
A picture of where the land is, and a check that it is not where the plates are.

Two renders and one number. The renders are the globe's land and sea, and the plate cells
beside them; the number is how strongly the two correlate, which should be very close to
nothing. That is the architectural claim of this phase - continents are not plate cells -
and a correlation is a far better test of it than looking at two pictures and feeling
reassured.

Run directly:  python -m worldbuilder.debug.land_map
"""

import math

from ..geometry.sphere import SpherePoint
from ..plates.generation import plates_for
from ..terrain.continentality import Continentality

WIDTH, HEIGHT = 720, 360


def _sweep(width, height):
    for row in range(height):
        latitude = 90.0 - 180.0 * (row + 0.5) / height
        for column in range(width):
            longitude = -180.0 + 360.0 * (column + 0.5) / width
            yield row, column, SpherePoint.from_latlon(latitude, longitude)


def _shade(metres):
    """Land in browns, sea in blues, with the shallows picked out."""
    if metres >= 0.0:
        rise = min(1.0, metres / 900.0)
        return (int(70 + 120 * rise), int(105 + 85 * rise), int(60 + 55 * rise))
    depth = min(1.0, -metres / 4800.0)
    return (
        int(20 + 25 * (1 - depth)),
        int(60 + 95 * (1 - depth)),
        int(90 + 120 * (1 - depth)),
    )


def _write(path, width, height, pixels):
    with open(path, "wb") as handle:
        handle.write(f"P6\n{width} {height}\n255\n".encode("ascii"))
        handle.write(bytes(pixels))


def render(world_seed, width=WIDTH, height=HEIGHT, into="."):
    land = Continentality(world_seed)
    plates = plates_for(world_seed)

    picture = bytearray(width * height * 3)
    dry = 0.0
    total = 0.0

    # For the correlation: continentality against distance to the nearest plate edge.
    values, distances = [], []
    for row, column, point in _sweep(width, height):
        at = (row * width + column) * 3
        metres = land.base_elevation(point)
        picture[at : at + 3] = bytes(_shade(metres))

        # Weighted by the cosine of the latitude, because equirectangular pixels are not
        # equal-area: a row beside a pole covers a sliver of the surface that a row at the
        # equator covers a great deal of. Counting pixels flat reported the polar regions
        # as though they were as big as the tropics.
        area = math.cos(math.radians(90.0 - 180.0 * (row + 0.5) / height))
        total += area
        if metres >= 0.0:
            dry += area
        if row % 4 == 0 and column % 4 == 0:
            values.append(land.at(point))
            distances.append(plates.margin_at(point).distance_m)

    path = f"{into}/land-{world_seed}.ppm"
    _write(path, width, height, picture)

    mean_v = sum(values) / len(values)
    mean_d = sum(distances) / len(distances)
    covariance = sum((v - mean_v) * (d - mean_d) for v, d in zip(values, distances))
    spread_v = math.sqrt(sum((v - mean_v) ** 2 for v in values))
    spread_d = math.sqrt(sum((d - mean_d) ** 2 for d in distances))
    correlation = covariance / (spread_v * spread_d) if spread_v and spread_d else 0.0

    print(f"  world {world_seed}")
    print(f"    land            {100.0 * dry / total:5.1f} % of the surface")
    print("    correlation between continentality and distance-to-plate-edge:")
    print(f"                    {correlation:+.4f}   (wanted: near zero)")
    print(f"    wrote {path}")
    return correlation


if __name__ == "__main__":
    for seed in (20260831, 7, 99):
        render(seed)
        print()
