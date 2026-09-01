"""
How far the coastline moves when a chart is drawn at a coarser scale.

The measurement that decides whether band-limiting is safe. Dropping zero-mean octaves
smooths the ground rather than displacing it - but the *zero contour* is another matter,
because a point at minus two metres with four metres of detail on it is land at close
range and water at wide range. Some of that is correct: real charts generalise coastlines.
A bay opening and closing as somebody zooms is not.

So this walks a transect at several resolutions and reports how far the shoreline moved,
against the shelf width it must stay well inside.

Run directly:  python -m worldbuilder.debug.lod_shift
"""

import math

from ..geometry.sphere import SpherePoint
from ..geometry.tangent import TangentFrame
from ..geometry.vectors import Vec3
from ..terrain.surface import Surface

RESOLUTIONS_M = (None, 100.0, 500.0, 2_000.0, 10_000.0)


def _spread(count):
    golden = math.pi * (3.0 - math.sqrt(5.0))
    for index in range(count):
        z = 1.0 - 2.0 * (index + 0.5) / count
        ring = math.sqrt(max(0.0, 1.0 - z * z))
        angle = golden * index
        yield SpherePoint(Vec3(math.cos(angle) * ring, math.sin(angle) * ring, z))


def _shoreline_along(surface, frame, east, north, resolution_m, reach_m, step_m):
    """Where the ground first crosses sea level, walking seaward."""
    previous = None
    for step in range(int(reach_m / step_m)):
        along = step * step_m
        here = surface.elevation_m(
            frame.local_to_sphere(east * along, north * along), resolution_m
        )
        if previous is not None and (previous >= 0.0) != (here >= 0.0):
            return along
        previous = here
    return None


def measure(world_seed=20260831, transects=12):
    surface = Surface(world_seed)

    starts = []
    for point in _spread(2500):
        if 20.0 < surface.structural_m(point) < 260.0:
            gradient = surface.land.gradient(point)
            if gradient.magnitude() > 0.0:
                starts.append((point, gradient))
                if len(starts) >= transects:
                    break

    print(f"  world {world_seed}: shoreline position on {len(starts)} transects\n")
    print(f"  {'resolution':>12}  {'mean shift':>11}  {'worst shift':>12}  {'lost':>5}")
    print(f"  {'-'*12}  {'-'*11}  {'-'*12}  {'-'*5}")

    canonical = []
    for point, gradient in starts:
        frame = TangentFrame.at(point)
        scale = 1.0 / gradient.magnitude()
        east, north = -gradient.east * scale, -gradient.north * scale
        canonical.append(
            (frame, east, north,
             _shoreline_along(surface, frame, east, north, None, 200_000.0, 500.0))
        )

    for resolution in RESOLUTIONS_M:
        shifts = []
        lost = 0
        for frame, east, north, truth in canonical:
            if truth is None:
                continue
            found = _shoreline_along(
                surface, frame, east, north, resolution, 200_000.0, 500.0
            )
            if found is None:
                lost += 1
            else:
                shifts.append(abs(found - truth))
        name = "canonical" if resolution is None else f"{resolution/1000:.1f} km"
        if shifts:
            print(f"  {name:>12}  {sum(shifts)/len(shifts)/1000:8.2f} km  "
                  f"{max(shifts)/1000:9.2f} km  {lost:5}")
        else:
            print(f"  {name:>12}  {'-':>11}  {'-':>12}  {lost:5}")


if __name__ == "__main__":
    measure()
