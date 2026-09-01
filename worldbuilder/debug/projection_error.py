"""
How wrong a flat chart of a round planet is, measured rather than asserted.

The spec asked for a maximum region radius and the first answer offered was a guess with
a formula attached. This produces the evidence instead: the actual error of the actual
projection, at ranges a region might plausibly be, so the cap can be chosen from a
tolerance somebody has agreed to rather than from an algebraic gesture.

Two quantities, because an azimuthal equidistant projection is exact in one of them and
not the other, and reporting a single "error" would hide that.

**Radial** - the distance from the frame's origin to a point, on the chart, against the
true great-circle distance. This should be zero to the limits of floating point. It is
the defining property of the projection and the reason it was chosen; the table exists
partly to prove it holds.

**Transverse** - the distance between two points that are both that far from the origin,
on the chart, against the truth. This is where the error actually lives: near the origin
the chart is very nearly perfect, and far from it two points drift apart faster on paper
than they do on the water.

Instrumentation, not product. Run it directly.
"""

import math

from ..geometry.sphere import EARTH_RADIUS_M, SpherePoint
from ..geometry.tangent import TangentFrame

#: Ranges to report, in metres. Chosen to bracket every plausible region size, from a
#: harbour approach to something nobody should attempt.
RANGES_M = (25_000.0, 50_000.0, 100_000.0, 200_000.0, 500_000.0, 1_000_000.0)

#: How far apart, in bearing, the two transverse probes are placed. Ten degrees is small
#: enough to be a local measurement and large enough that the error is not lost in
#: floating point.
PROBE_SEPARATION_DEG = 10.0


def _probe(frame, distance_m, bearing_deg):
    """
    Args:
        frame (TangentFrame): The chart.
        distance_m (float): How far from the origin to place the probe.
        bearing_deg (float): Which way from the origin.

    Returns:
        probe (tuple): The point on the globe, and its local x and y.

    """
    bearing = math.radians(bearing_deg)
    x = distance_m * math.sin(bearing)
    y = distance_m * math.cos(bearing)
    return frame.local_to_sphere(x, y), x, y


def measure(latitude_deg=0.0, radius_m=EARTH_RADIUS_M):
    """
    Args:
        latitude_deg (float, optional): Where to centre the frame. Error does not depend
            on it for this projection, which the table is worth checking.
        radius_m (float, optional): The planet's radius.

    Returns:
        rows (list): One dict per range, with both errors in metres and as a fraction.

    """
    frame = TangentFrame.at_latlon(latitude_deg, 0.0, radius_m)
    rows = []
    for distance_m in RANGES_M:
        here, x, y = _probe(frame, distance_m, 0.0)
        there, _, _ = _probe(frame, distance_m, PROBE_SEPARATION_DEG)

        # Radial: what the chart says the origin-to-probe distance is, against the truth.
        charted_radial = math.hypot(x, y)
        true_radial = frame.origin.distance_to(here, radius_m)

        # Transverse: the same question between two probes neither of which is the
        # origin, which is where an equidistant projection stops being exact.
        far_x, far_y = frame.sphere_to_local(there)
        charted_across = math.hypot(far_x - x, far_y - y)
        true_across = here.distance_to(there, radius_m)

        rows.append(
            {
                "range_m": distance_m,
                "radial_error_m": charted_radial - true_radial,
                "transverse_error_m": charted_across - true_across,
                "transverse_error_fraction": (charted_across - true_across) / true_across,
            }
        )
    return rows


def report(latitude_deg=0.0):
    """Print the table. The whole point of the exercise."""
    print(f"  azimuthal equidistant, frame centred at {latitude_deg:.0f} degrees latitude\n")
    print(f"  {'range':>10}  {'radial error':>16}  {'transverse error':>18}  {'as a fraction':>14}")
    print(f"  {'-' * 10}  {'-' * 16}  {'-' * 18}  {'-' * 14}")
    for row in measure(latitude_deg):
        print(
            f"  {row['range_m'] / 1000:7.0f} km  "
            f"{row['radial_error_m']:13.6f} m  "
            f"{row['transverse_error_m']:15.2f} m  "
            f"{row['transverse_error_fraction'] * 100:12.4f} %"
        )


if __name__ == "__main__":
    for latitude in (0.0, 45.0, 80.0):
        report(latitude)
        print()
