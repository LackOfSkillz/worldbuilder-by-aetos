"""
What the bottom of the demonstration coast is made of.

Drawn as a mixture rather than as three flat colours, because the field is a mixture and a
three-colour render would hide the only thing worth checking: whether the boundaries are
gradients or edges. Sand is yellow, mud is green, rock is grey, and every pixel is however
much of each there is - so a bank fading into the deep water beyond it reads as a fade, and
anything that reads as a line is a bug.

The diagnostic that found the phase's real bug was this render. At a six-hundred-metre
slope baseline the pinnacle and the drying rock came out as grey *rings* with sand at the
centre, because a finite difference cannot see anything narrower than itself.

Run directly:  python -m worldbuilder.debug.bottom
"""

from ..geometry.tangent import TangentFrame
from ..regions.demo import WORLD_SEED, demo_region
from ..terrain.surface import Surface

WIDTH, HEIGHT = 420, 420

#: Metres either way from the anchor. Smaller than the chart render, because a bottom
#: costs four soundings and a frame, and because the interesting part is the harbour.
HALF_M = 14_000.0

SAND_RGB = (226, 202, 128)
MUD_RGB = (96, 122, 84)
ROCK_RGB = (128, 128, 132)
DRY_RGB = (236, 228, 206)


def _mix(bottom):
    return tuple(
        int(
            bottom.sand * SAND_RGB[channel]
            + bottom.mud * MUD_RGB[channel]
            + bottom.rock * ROCK_RGB[channel]
        )
        for channel in range(3)
    )


def _write(path, width, height, pixels):
    with open(path, "wb") as handle:
        handle.write(f"P6\n{width} {height}\n255\n".encode("ascii"))
        handle.write(bytes(value for pixel in pixels for value in pixel))


def render(world_seed=WORLD_SEED, into=".", width=WIDTH, height=HEIGHT, half_m=HALF_M):
    """
    Draw the bottom composition, and report where each kind of ground is.

    Args:
        world_seed (int, optional): Which world.
        into (str, optional): Where to write.
        width (int, optional): Pixels.
        height (int, optional): Pixels.
        half_m (float, optional): Metres either way from the anchor.

    Returns:
        path (str): What was written.

    """
    region = demo_region()
    world = Surface(world_seed, features=region.features)
    frame = TangentFrame.at(region.origin)

    pixels = []
    shares = {"sand": 0.0, "mud": 0.0, "rock": 0.0}
    dry = 0
    for row in range(height):
        north = half_m - 2.0 * half_m * (row + 0.5) / height
        for column in range(width):
            east = -half_m + 2.0 * half_m * (column + 0.5) / width
            point = frame.local_to_sphere(east, north)
            # Handed in, so the bottom does not work out ground the render already has.
            elevation = world.structural_m(point)
            if elevation >= 0.0:
                pixels.append(DRY_RGB)
                dry += 1
                continue
            bottom = world.substrate.at(point, elevation_m=elevation)
            pixels.append(_mix(bottom))
            shares["sand"] += bottom.sand
            shares["mud"] += bottom.mud
            shares["rock"] += bottom.rock

    path = f"{into}/bottom-{world_seed}.ppm"
    _write(path, width, height, pixels)

    wet = width * height - dry
    print(f"  {region.name}, world {world_seed}")
    print(f"    {2.0 * half_m / width:.0f} m a pixel, {100.0 * dry / (width * height):.1f} "
          f"% of the frame dry")
    print("    what the water in this frame lies on:")
    for name, total in shares.items():
        print(f"      {name:5} {100.0 * total / max(wet, 1):5.1f} %")
    print("    at each placed feature:")
    for feature in region.features:
        bottom = world.bottom_at(feature.at)
        declared = feature.substrate or "-"
        print(f"      {feature.kind:18} declared {declared:5} -> {bottom.dominant:5} "
              f"(sand {bottom.sand:.2f} mud {bottom.mud:.2f} rock {bottom.rock:.2f})"
              f"  holding {bottom.holding():.2f}")
    print(f"    wrote {path}")
    return path


if __name__ == "__main__":
    render()
