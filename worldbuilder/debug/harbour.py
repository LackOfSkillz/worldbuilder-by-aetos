"""
The demonstration coast, drawn the way a chart draws it.

Two renders and a table. The chart uses depth bands rather than a smooth ramp because that
is what makes a bank look like a bank: a continuous blue gradient hides a two-metre shoal
inside a shelf that is already blue, and every one of the placement bugs in this region -
banks lying across the channel they were meant to flank, an approach that dredged away its
own bar - was invisible in a smooth render and obvious in a banded one.

The second render is the same water sampled the way a chart samples it, four hundred metres
apart. Comparing the two is the phase in a picture: every placed feature survives except
the pinnacle, which is missing from the sampled render and present in the truth.

Run directly:  python -m worldbuilder.debug.harbour
"""

from ..geometry.tangent import TangentFrame
from ..regions.demo import WORLD_SEED, demo_region
from ..terrain.surface import Surface

WIDTH, HEIGHT = 760, 760

#: Metres either way from the anchor. Twenty-five kilometres puts the harbour, both
#: banks, both marks, the headland and the steep-to water in one frame at thirty-three
#: metres a pixel - fine enough that a hundred-and-forty-metre rock is four pixels
#: across, which is the point. Eighty kilometres across made it one.
HALF_M = 25_000.0

#: Depth bands, in metres, shoalest first. The first four are the ones a small craft
#: cares about and they are deliberately close together.
BANDS = (0.0, -2.0, -5.0, -10.0, -20.0, -50.0, -100.0, -200.0, -1000.0)


def _sea(metres):
    """Chart blue: pale in the shallows, deepening by band."""
    for index, floor in enumerate(BANDS):
        if metres >= floor:
            shade = index / (len(BANDS) - 1)
            return (
                int(214 - 150 * shade),
                int(232 - 130 * shade),
                int(245 - 80 * shade),
            )
    return (40, 70, 130)


def _land(metres):
    rise = min(1.0, metres / 200.0)
    return (int(226 - 40 * rise), int(214 - 60 * rise), int(170 - 40 * rise))


def _colour(metres):
    return _land(metres) if metres >= 0.0 else _sea(metres)


def _cross(pixels, width, height, column, row, colour, arm=6):
    """A chart symbol, drawn over whatever is underneath it."""
    for step in range(-arm, arm + 1):
        for x, y in ((column + step, row), (column, row + step)):
            if 0 <= x < width and 0 <= y < height:
                pixels[y * width + x] = colour


def _write(path, width, height, pixels):
    with open(path, "wb") as handle:
        handle.write(f"P6\n{width} {height}\n255\n".encode("ascii"))
        handle.write(bytes(value for pixel in pixels for value in pixel))


def render(world_seed=WORLD_SEED, into=".", width=WIDTH, height=HEIGHT, half_m=HALF_M):
    """
    Draw the region twice and report what each render can see.

    Args:
        world_seed (int, optional): Which world.
        into (str, optional): Where to write.
        width (int, optional): Pixels.
        height (int, optional): Pixels.
        half_m (float, optional): Metres either way from the anchor.

    Returns:
        paths (tuple): What was written.

    """
    region = demo_region()
    world = Surface(world_seed, features=region.features)
    frame = TangentFrame.at(region.origin)
    metres_per_pixel = 2.0 * half_m / width

    truth, sampled = [], []
    for row in range(height):
        north = half_m - 2.0 * half_m * (row + 0.5) / height
        for column in range(width):
            east = -half_m + 2.0 * half_m * (column + 0.5) / width
            point = frame.local_to_sphere(east, north)
            truth.append(_colour(world.elevation_m(point)))
            sampled.append(_colour(world.elevation_m(point, 400.0)))

    # Marks go on the sampled render only, because that is the one that needs them.
    for _, feature in region.features.marks_near(region.origin, half_m * 2.0):
        east, north = frame.sphere_to_local(feature.at)
        column = int((east + half_m) / metres_per_pixel)
        row = int((half_m - north) / metres_per_pixel)
        if 0 <= column < width and 0 <= row < height:
            _cross(sampled, width, height, column, row, (200, 30, 40))

    paths = []
    for name, pixels in (("harbour-truth", truth), ("harbour-sampled", sampled)):
        path = f"{into}/{name}-{world_seed}.ppm"
        _write(path, width, height, pixels)
        paths.append(path)

    print(f"  {region.name}, world {world_seed}")
    print(f"    {len(region.features)} features, {metres_per_pixel:.0f} m a pixel")
    print("\n    what the truth holds against what a chart prints, in metres:")
    print(f"      {'feature':18} {'truth':>8} {'charted':>9}  {'':>8}")
    for feature in region.features:
        truth_m = world.elevation_m(feature.at)
        charted_m = charted(world, frame, feature, half_m)
        # The same two metres the tests use, so the render and the suite cannot
        # disagree about which features a chart lies about.
        lied = truth_m - charted_m > 2.0
        note = "  <- a chart lies here" if lied else ""
        if lied != feature.marked:
            note += "   ** AND IS NOT MARKED **" if lied else "   ** MARKED ANYWAY **"
        print(f"      {feature.kind:18} {truth_m:8.1f} {charted_m:9.1f}{note}")
    for path in paths:
        print(f"    wrote {path}")
    return tuple(paths)


def charted(world, frame, feature, half_m, spacing_m=400.0):
    """
    The shoalest sounding a chart would actually print over a feature.

    Notes:
        Sampled on a lattice anchored to the region rather than to the feature, because a
        chart does not get to choose where its grid falls relative to a rock. Asking the
        terrain at the feature's own centre - which is how this table read first - reports
        that a four-hundred-metre chart can see a hundred-and-forty-metre pinnacle, which
        is true only of a grid that was told where it was.

    """
    east, north = frame.sphere_to_local(feature.at)
    base_col = round(east / spacing_m)
    base_row = round(north / spacing_m)
    shoalest = -9e9
    for row in (base_row - 1, base_row, base_row + 1):
        for col in (base_col - 1, base_col, base_col + 1):
            point = frame.local_to_sphere(col * spacing_m, row * spacing_m)
            shoalest = max(shoalest, world.elevation_m(point, spacing_m))
    return shoalest


if __name__ == "__main__":
    render()
