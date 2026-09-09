"""Rivers and other authored changes, as data the worldfile carries.

**A world is not just a planet and some rooms.** It is a planet, the changes somebody made
to it, and the rooms placed on the result - and until now the worldfile carried the first
and the third. A game reading it got a river town with no river.

The engine has always accepted authored features: a shape stated at a place, evaluated as a
pure function of the point, which is why an authored change costs nothing of the
point-evaluable design. What was missing was carrying them across the boundary. This module
is that, and it is deliberately a mirror of `viewer/public/app/river.js` rather than a
second opinion: the same overlap, the same taper, the same field names.

**The order of a river's nodes is its direction, and it decides which end is deep.** Node
zero is the mouth. Reversing the list makes a river that shoals toward the sea, which is
what a lake outlet does and not what a navigable river does.
"""

import math
from dataclasses import asdict, dataclass

#: How far each segment reaches along its bearing, as a multiple of half the leg it covers.
#:
#: **4.5, measured, and the same number the viewer uses.** A feature's weight is one at its
#: middle and nothing at its stated reach, so a chain carves deepest at the midpoints and
#: shallowest at the nodes. Sounded over a 3.51 km channel of 25 segments at 2.5 m draught,
#: the shoaling count runs 65, 19, 1, 0, 0 at overlaps of 1.35, 2, 3, 4.5 and 6.
#:
#: If this number and the viewer's ever disagree, the game and the studio draw different
#: ground - which is the whole failure this file exists to prevent.
OVERLAP = 4.5


@dataclass(frozen=True)
class RiverShape:
    """What a river is, at each end. Both are stated because a taper needs two."""

    mouth_depth_m: float = -14.0
    head_depth_m: float = -11.0
    #: Half-widths. `width_m` reaches this far EITHER SIDE, so the channel is twice this.
    #:
    #: Getting that wrong drowned 87 of a 177-room town: a river asked for at "80 m" was
    #: 160 m across and swallowed the quay street it was meant to run past.
    mouth_width_m: float = 22.0
    head_width_m: float = 15.0


def _bearing(a, b):
    la1, lo1, la2, lo2 = map(math.radians, (a[0], a[1], b[0], b[1]))
    y = math.sin(lo2 - lo1) * math.cos(la2)
    x = math.cos(la1) * math.sin(la2) - math.sin(la1) * math.cos(la2) * math.cos(lo2 - lo1)
    return (math.degrees(math.atan2(y, x)) + 360.0) % 360.0


def _metres(a, b, radius_m):
    la1, lo1, la2, lo2 = map(math.radians, (a[0], a[1], b[0], b[1]))
    h = (math.sin((la2 - la1) / 2) ** 2
         + math.cos(la1) * math.cos(la2) * math.sin((lo2 - lo1) / 2) ** 2)
    return 2 * math.asin(math.sqrt(h)) * radius_m


def features_from_points(points, radius_m, shape=RiverShape()):
    """
    Carve records for one river, mouth first.

    Args:
        points (list): `[(lat, lon), ...]`, node zero at the mouth.
        radius_m (float): The planet's radius.
        shape (RiverShape, optional): Depth and half-width at each end.

    Returns:
        features (list): Dicts ready for the worldfile and for the engine.

    """
    out = []
    for index in range(len(points) - 1):
        a, b = points[index], points[index + 1]
        leg = _metres(a, b, radius_m)
        if leg <= 0.0:
            continue
        along = index / (len(points) - 2) if len(points) > 2 else 0.0
        out.append({
            "kind": "river",
            "latitude_deg": (a[0] + b[0]) / 2.0,
            "longitude_deg": (a[1] + b[1]) / 2.0,
            "target_m": shape.mouth_depth_m
            + (shape.head_depth_m - shape.mouth_depth_m) * along,
            "length_m": (leg / 2.0) * OVERLAP,
            "width_m": shape.mouth_width_m
            + (shape.head_width_m - shape.mouth_width_m) * along,
            "bearing_deg": _bearing(a, b),
            "compose": "carve",
            "substrate": "derive",
            "marked": False,
        })
    return out


def features_from_route(route, shape=RiverShape()):
    """Read a saved route file and return `(features, points)`."""
    radius_m = float(route["planet"]["radius"])
    points = [(node["latitude_deg"], node["longitude_deg"])
              for node in sorted(route["nodes"], key=lambda n: n.get("order", 0))]
    if len(points) < 2:
        raise ValueError("a river needs at least two nodes")
    return features_from_points(points, radius_m, shape), points


def to_engine(features):
    """The same records in the shape `worldbuilder.bathymetry.features.Feature` wants.

    Kept separate from the worldfile shape on purpose. The file is a promise to strangers
    and uses long, unambiguous names; the engine's constructor is positional and internal.
    Letting one drive the other would make a rename of an internal field a breaking change
    to the format.
    """
    from worldbuilder.bathymetry.features import CARVE, RAISE, SHAPE, Feature
    from worldbuilder.geometry.sphere import SpherePoint

    compose = {"carve": CARVE, "raise": RAISE, "shape": SHAPE}
    built = []
    for f in features:
        built.append(Feature(
            kind=f.get("kind", "feature"),
            at=SpherePoint.from_latlon(f["latitude_deg"], f["longitude_deg"]),
            target_m=f["target_m"],
            length_m=f["length_m"],
            width_m=f["width_m"],
            bearing_deg=f["bearing_deg"],
            compose=compose[f.get("compose", "carve")],
            marked=bool(f.get("marked", False)),
        ))
    return built


def sound(points, elevation_at, radius_m, draught_m=6.096, step_m=100.0):
    """
    Walk a channel and report whether it holds its depth.

    Notes:
        **An unchecked river is one that is fourteen metres deep except where a hull would
        find.** The first river built this way had banks, a bed and a clean cross-section,
        and 34 of its 90 soundings shoaled - because a cross-section is taken at a midpoint
        and the gaps are at the nodes.
    """
    readings = []
    for index in range(len(points) - 1):
        a, b = points[index], points[index + 1]
        steps = max(1, int(round(_metres(a, b, radius_m) / step_m)))
        for s in range(steps):
            f = s / steps
            lat = a[0] + (b[0] - a[0]) * f
            lon = a[1] + (b[1] - a[1]) * f
            readings.append((lat, lon, -elevation_at(lat, lon)))
    shoal = [r for r in readings if r[2] < draught_m]
    depths = [r[2] for r in readings]
    return {
        "soundings": len(readings),
        "draught_m": draught_m,
        "min_depth_m": round(min(depths), 2) if depths else None,
        "max_depth_m": round(max(depths), 2) if depths else None,
        "shoalings": len(shoal),
        "navigable": not shoal,
        "worst": [{"latitude_deg": round(r[0], 5), "longitude_deg": round(r[1], 5),
                   "depth_m": round(r[2], 2)} for r in sorted(shoal, key=lambda r: r[2])[:5]],
    }
