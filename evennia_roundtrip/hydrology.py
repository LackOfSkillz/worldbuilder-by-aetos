"""Fresh water: where it rises, where it runs, and where it stands.

The planet had exactly one river, because somebody drew one. Every inland settlement was
therefore dry, `human port city` and `valran river city` could never be placed, and the
site scorer's "fresh water at hand" was a term that scored zero everywhere except a few
kilometres of the Landing.

**Rivers are authored polylines that become CARVE features, and that is not a compromise.**
The whole engine rests on being point-evaluable - `f(point) -> metres`, no neighbours, no
traversal order - and flow accumulation over a grid is the opposite of that. But a river is
already expressed as a chain of carve features, which IS point-evaluable, so nothing here
touches the evaluation model. What this adds is the *authoring* step: finding the line a
river would take, once, and writing it down. The engine still never traverses anything.

**Water runs downhill, and that is the only rule.** A source is high ground; the path is
whichever neighbouring direction descends fastest, stepped until it reaches the sea. No
hydrological model, no rainfall, no discharge - those would be a simulation, and what is
wanted is a line on a map that a person would believe.

**Where a descent stops without reaching the sea, that is a lake.** A basin with no outlet
is exactly what a lake is, so the failure case of the river walk is the other feature this
module produces rather than an error it has to handle. Ponds are the same thing smaller,
and streams are the same thing shorter - one walk, three kinds of water, told apart by how
far they got.
"""

import math

from .river import OVERLAP, RiverShape, features_from_points

#: How far apart the steps of a descent are. Small enough to follow a valley, large enough
#: that a continent-long river is a few hundred steps rather than a few hundred thousand.
STEP_M = 2000.0

#: How many directions are tried at each step.
BEARINGS = 16

#: A descent that has not reached the sea after this many steps has found a basin, not an
#: ocean. Generous: a real river crosses a continent.
MAX_STEPS = 1200

#: How far a trapped walk may look for an outlet, as multiples of a step.
#:
#: **A lake fills and spills, and a walk that cannot do the same stops in every hollow.**
#: The first version treated any local minimum as a terminal basin, and produced thirty
#: streams averaging twenty kilometres and not one river - which read as "this planet has
#: no rivers" and was a walk that could not get out of a puddle. Looking further out for a
#: lower point is what an overflowing lake does, and the hollow it leaves behind is
#: recorded as the lake it is.
ESCAPE_REACH_STEPS = 60

#: How much a step must fall to count as still running downhill. Below this the water is
#: standing, which is a lake rather than a very slow river.
FALL_M = 0.05

#: How long a carved segment is, against the two-kilometre steps the walk takes.
#:
#: **The line a map draws and the channel an engine carves want different resolutions, and
#: conflating them cost a factor of twenty-five.** `Features.apply` is a linear scan - the
#: engine's own note says a world wanting thousands of features has wanted a generator
#: rather than a stamp - and one carve per two-kilometre step gave seven thousand features
#: and five milliseconds a sample, against forty-three microseconds for the ninety-seven
#: authored ones. At two hundred samples a second nothing that reads the ground can run.
#:
#: So the walk still steps at two kilometres, because that is what follows a valley, and
#: the polyline it produces is drawn at full detail. Only the carve records are decimated.
#: A river bed is a smooth thing tens of metres wide; sampling its centreline every sixteen
#: kilometres instead of every two loses nothing a player could stand in.
CARVE_EVERY_M = 16_000.0

#: What counts as a river rather than a stream, in metres of run.
RIVER_M = 120_000.0

#: What counts as a lake rather than a pond, in metres of radius.
LAKE_M = 3_000.0

#: How deep standing water is cut, and how wide, per metre of its radius.
LAKE_DEPTH_M = 12.0
POND_DEPTH_M = 4.0


def _offset(latitude_deg, longitude_deg, east_m, north_m, radius_m):
    metres_per_degree = math.pi * radius_m / 180.0
    return (latitude_deg + north_m / metres_per_degree,
            longitude_deg + east_m / (metres_per_degree
                                      * math.cos(math.radians(latitude_deg))))


def descend(at, latitude_deg, longitude_deg, radius_m, step_m=STEP_M,
            bearings=BEARINGS, max_steps=MAX_STEPS):
    """
    Follow the ground downhill from a point.

    Args:
        at (callable): `(lat, lon) -> metres`.
        latitude_deg (float): Where the water rises.
        longitude_deg (float): Where the water rises.
        radius_m (float): The planet's radius.
        step_m (float, optional): Distance per step.
        bearings (int, optional): Directions tried at each step.
        max_steps (int, optional): Give up after this many.

    Returns:
        walk (dict): `points` from source to end, `reached_sea`, and `end_height_m`.

    Notes:
        **Steepest descent, and it is allowed to fail.** A walk that stops on dry land has
        found a basin with no outlet, which is a lake - so the two outcomes are both
        useful and neither is an error. Backtracking to escape a basin would produce
        rivers that flow uphill through a saddle, which is worse than a lake.

        Visited points are remembered so a flat spot cannot trap the walk in a two-step
        cycle - a real hazard on gentle ground, where two neighbours can each be lower
        than the other by a rounding error.
    """
    points = [(round(latitude_deg, 6), round(longitude_deg, 6))]
    basins = []
    here = (latitude_deg, longitude_deg)
    height = at(*here)
    seen = {(round(here[0], 3), round(here[1], 3))}

    for _step in range(max_steps):
        if height <= 0.0:
            return {"points": points, "reached_sea": True, "end_height_m": height,
                    "basins": basins}
        best, best_height = None, height
        for index in range(bearings):
            bearing = math.radians(360.0 * index / bearings)
            candidate = _offset(here[0], here[1], step_m * math.sin(bearing),
                                step_m * math.cos(bearing), radius_m)
            key = (round(candidate[0], 3), round(candidate[1], 3))
            if key in seen:
                continue
            candidate_height = at(*candidate)
            if candidate_height < best_height - FALL_M:
                best, best_height = candidate, candidate_height
        if best is None:
            # Nothing lower within one step: the water is standing. Look further for an
            # outlet, which is what a filling lake finds. The hollow is remembered so the
            # caller can put a lake in it - the water really does stand here, it simply
            # does not stop here.
            best, best_height = _outlet(at, here, height, radius_m, step_m, bearings)
            if best is None:
                return {"points": points, "reached_sea": False,
                        "end_height_m": height, "basins": basins}
            basins.append((round(here[0], 6), round(here[1], 6)))
        here, height = best, best_height
        seen.add((round(here[0], 3), round(here[1], 3)))
        points.append((round(here[0], 6), round(here[1], 6)))

    return {"points": points, "reached_sea": height <= 0.0, "end_height_m": height,
            "basins": basins}


def _outlet(at, here, height, radius_m, step_m, bearings, reach_steps=ESCAPE_REACH_STEPS):
    """The nearest lower ground beyond one step: where a filling lake would spill.

    Returns `(point, height)`, or `(None, height)` if this really is a closed basin.

    Notes:
        **A disc, not a set of rings.** The first version tried four fixed distances - four,
        eight, sixteen and thirty-two kilometres - and the outlet it needed was at twenty.
        It stepped straight over the gap and declared a closed basin, and the whole planet
        came out with no rivers. A ring search that misses is the failure `sea_reach`
        already documents; here it does not merely under-report, it stops the water.

        Nearest wins, so a river takes the first outlet it could actually spill through
        rather than the deepest one anywhere in range - which is what an overflowing lake
        does, and keeps the course where the ground leads it.
    """
    for multiple in range(2, reach_steps + 1):
        distance = step_m * multiple
        best, best_height = None, height
        for index in range(bearings * 2):
            bearing = math.radians(360.0 * index / (bearings * 2))
            candidate = _offset(here[0], here[1], distance * math.sin(bearing),
                                distance * math.cos(bearing), radius_m)
            candidate_height = at(*candidate)
            if candidate_height < best_height - FALL_M:
                best, best_height = candidate, candidate_height
        if best is not None:
            return best, best_height
    return None, height


def _decimate(points, radius_m, every_m):
    """Thin a polyline to roughly one node per `every_m`, keeping both ends.

    Notes:
        Both ends are kept unconditionally. The mouth is where the river meets the sea and
        the source is where it rises; a thinning that could drop either would shorten the
        river by up to one segment at the end that matters most.
    """
    if len(points) < 3:
        return list(points)
    kept = [points[0]]
    carried = 0.0
    for a, b in zip(points, points[1:]):
        carried += _length([a, b], radius_m)
        if carried >= every_m:
            kept.append(b)
            carried = 0.0
    if kept[-1] != points[-1]:
        kept.append(points[-1])
    return kept


def _length(points, radius_m):
    total = 0.0
    for a, b in zip(points, points[1:]):
        la1, lo1, la2, lo2 = map(math.radians, (a[0], a[1], b[0], b[1]))
        h = (math.sin((la2 - la1) / 2) ** 2
             + math.cos(la1) * math.cos(la2) * math.sin((lo2 - lo1) / 2) ** 2)
        total += 2 * math.asin(math.sqrt(h)) * radius_m
    return total


def basin_radius(at, latitude_deg, longitude_deg, radius_m, reach_m=12000.0,
                 rays=12, steps=8):
    """
    How far standing water would spread from a low point before the ground rises.

    Notes:
        Measured as the mean over several bearings, not the maximum. One open direction is
        a valley the water drains along, not a lake shore, and taking the furthest ray
        would turn every valley floor into an inland sea.
    """
    here = at(latitude_deg, longitude_deg)
    reaches = []
    for index in range(rays):
        bearing = math.radians(360.0 * index / rays)
        reach = reach_m
        for step in range(1, steps + 1):
            distance = reach_m * step / steps
            point = _offset(latitude_deg, longitude_deg,
                            distance * math.sin(bearing), distance * math.cos(bearing),
                            radius_m)
            if at(*point) > here + 8.0:
                reach = distance
                break
        reaches.append(reach)
    return sum(reaches) / len(reaches)


def still_water(at, latitude_deg, longitude_deg, radius_m):
    """
    A lake or a pond at a point where a descent stopped, as one round CARVE feature.

    Returns:
        record (dict or None): The feature, or None where the basin is too small to be
        worth a mark on any map.
    """
    spread = basin_radius(at, latitude_deg, longitude_deg, radius_m)
    if spread < 400.0:
        return None
    lake = spread >= LAKE_M
    surface = at(latitude_deg, longitude_deg)
    return {
        "kind": "lake" if lake else "pond",
        "latitude_deg": round(latitude_deg, 6),
        "longitude_deg": round(longitude_deg, 6),
        # Cut BELOW the ground it stands on, so the water surface sits at the old ground
        # level rather than at datum - an upland lake is not at sea level.
        "target_m": round(surface - (LAKE_DEPTH_M if lake else POND_DEPTH_M), 3),
        "length_m": round(spread, 1),
        "width_m": round(spread, 1),
        "bearing_deg": 0.0,
        "compose": "carve",
        "substrate": "derive",
        "marked": True,
        "radius_m": round(spread, 1),
        "surface_m": round(surface, 2),
    }


def draw(at, radius_m, sources, shape=None, step_m=STEP_M,
         carve_every_m=CARVE_EVERY_M):
    """
    Every watercourse and body of standing water these sources produce.

    Args:
        at (callable): The elevation oracle.
        radius_m (float): The planet's radius.
        sources (iterable): `(lat, lon)` high points for water to rise at.
        shape (RiverShape, optional): Depth and width at mouth and head.
        step_m (float, optional): Descent step.

    Returns:
        water (dict): `courses` (each with its polyline, kind and length), `bodies`
        (lakes and ponds), and `features` - every carve record, ready for a worldfile.

    Notes:
        **The polyline is kept alongside the features, and that is the point of this
        return shape.** Features are what the engine reads; a line is what a map draws and
        what a person recognises. Deriving one from the other afterwards means reversing a
        chain of overlapping segments, which is exactly the arithmetic that made the first
        river shoal at every node.
    """
    shape = shape or RiverShape()
    courses, bodies, features = [], [], []
    for latitude, longitude in sources:
        if at(latitude, longitude) <= 0.0:
            continue
        walk = descend(at, latitude, longitude, radius_m, step_m=step_m)
        points = walk["points"]
        if len(points) < 3:
            continue
        run = _length(points, radius_m)
        kind = "river" if run >= RIVER_M else "stream"
        # Mouth first, because `features_from_points` ramps depth and width from node zero
        # and node zero is the deep end.
        mouth_first = list(reversed(points))
        carved = features_from_points(_decimate(mouth_first, radius_m, carve_every_m),
                                      radius_m, shape=shape)
        for record in carved:
            record["kind"] = kind
        features.extend(carved)
        courses.append({"kind": kind, "length_m": round(run),
                        "reached_sea": walk["reached_sea"],
                        "source": list(points[0]), "mouth": list(points[-1]),
                        "points": [list(p) for p in points]})
        if not walk["reached_sea"]:
            body = still_water(at, points[-1][0], points[-1][1], radius_m)
            if body is not None:
                bodies.append(body)
                features.append({k: v for k, v in body.items()
                                 if k not in ("radius_m", "surface_m")})
    return {"courses": courses, "bodies": bodies, "features": features,
            "overlap": OVERLAP}
