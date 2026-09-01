"""
A walk from dry land into deep water, printed as numbers.

The most useful diagnostic in the engine so far, and it is text. A picture of a coast
tells you it looks plausible; a column of depths every kilometre tells you whether there
is a shore, a shelf, a break, a slope and a basin, in that order, at sensible distances.

Also prints the coastal-distance estimate beside the depth, because that coordinate is
what the whole shelf is built on and it is easier to debug before the shelf than after.

Run directly:  python -m worldbuilder.debug.coast_walk
"""

import math

from ..bathymetry.shelf import Shelf
from ..geometry.sphere import SpherePoint
from ..geometry.tangent import TangentFrame
from ..plates.generation import plates_for
from ..terrain.continentality import Continentality
from ..terrain.tectonics import Tectonics


def _find_coast(land, seed_points, tectonics=None):
    """
    A point actually at the water's edge, and the direction of the open sea.

    Notes:
        The first version of this took any dry point and walked downhill, which started
        four hundred kilometres inland on a broad continent and never reached the sea in
        three hundred kilometres of walking. On a field whose features are thousands of
        kilometres across, "on land" and "near the coast" are not remotely the same
        question - so this asks the second one.

    """
    best = None
    for point in seed_points:
        height = abs(land.base_elevation(point))
        if tectonics is not None:
            # Prefer a passive margin. The first version took the nearest shore of any
            # kind and landed on a coastal uplift belt, where the interesting thing on
            # show was the shelf arguing with a mountain range rather than the shelf.
            height += abs(tectonics.offset_m(point)) * 0.5
        if best is None or height < best[0]:
            gradient = land.gradient(point)
            if gradient.magnitude() > 0.0:
                best = (height, point, gradient)
    if best is None:
        return None, None, None
    _, point, gradient = best
    scale = 1.0 / gradient.magnitude()
    return point, -gradient.east * scale, -gradient.north * scale


def walk(world_seed=20260831, out_to_m=260_000.0, step_m=10_000.0):
    plates = plates_for(world_seed)
    land = Continentality(world_seed)
    tectonics = Tectonics(plates, land)
    shelf = Shelf(tectonics, land)

    golden = math.pi * (3.0 - math.sqrt(5.0))
    candidates = []
    for index in range(400):
        z = 1.0 - 2.0 * (index + 0.5) / 400
        ring = math.sqrt(max(0.0, 1.0 - z * z))
        angle = golden * index
        from ..geometry.vectors import Vec3

        candidates.append(
            SpherePoint(Vec3(math.cos(angle) * ring, math.sin(angle) * ring, z))
        )

    start, east, north = _find_coast(land, candidates, tectonics)
    if start is None:
        print("  no coast found")
        return

    frame = TangentFrame.at(start)
    print(f"  world {world_seed}, walking seaward from {start.to_latlon()}\n")
    print(f"  {'along':>9}  {'macro':>9}  {'final':>9}  {'coast est':>11}  "
          f"{'weight':>7}  {'breadth':>8}")
    print(f"  {'-'*9}  {'-'*9}  {'-'*9}  {'-'*11}  {'-'*7}  {'-'*8}")

    steps = int(out_to_m / step_m)
    for step in range(-3, steps):
        along = step * step_m
        point = frame.local_to_sphere(east * along, north * along)
        macro = tectonics.elevation_m(point)
        final = shelf.elevation_m(point)
        coastal = shelf.coastal(point)
        if coastal is None:
            print(f"  {along/1000:7.0f} km  {macro:8.0f} m  {final:8.0f} m  "
                  f"{'-':>11}  {'-':>7}  {'-':>8}")
        else:
            print(f"  {along/1000:7.0f} km  {macro:8.0f} m  {final:8.0f} m  "
                  f"{coastal.distance_m/1000:8.1f} km  "
                  f"{shelf.weight(point, coastal):7.3f}  {coastal.breadth:8.3f}")


if __name__ == "__main__":
    walk()
