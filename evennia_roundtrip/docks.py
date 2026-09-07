"""Which rooms a vessel can tie up at, and where the water off them is.

**Maritime navigates between docks, not between coordinates.** A passage is only useful if
its ends are places a player can stand and board something, and those are rooms. So this
finds them, measures the water off each one, and writes the result where both the game and
the studio can read it.

**A dock is not "a room with dock in its name", and the difference matters.** A room called
`The Dock Road` is a road. A room called `Slakeside, Crane Stand` is a working wharf and
says nothing about docks at all. Naming is evidence and not proof, so a candidate has to
carry BOTH a name that suggests a landing AND navigable water within reach - and the reach
is measured rather than assumed.

**The approach is the useful half.** A dock's own position is at the waterline, where depth
is nearly zero by construction: a passage that starts there is never navigable, which is
exactly how the first attempt at one produced two waypoints and gave up. What a vessel
needs is the point OFFSHORE of the dock where it can float, and the bearing to reach it.
That is what `approach` finds, and it is the thing maritime actually steers to.
"""

import math

#: Words that suggest a place a vessel ties up. Evidence, not proof - each candidate is
#: then measured against the water.
DOCK_WORDS = ("dock", "quay", "wharf", "jetty", "pier", "slip", "landing stage",
              "landing beach", "harbour", "harbor", "staith", "hythe", "boat ramp")

#: Words that look like the above and are not. A road to a dock is a road.
NOT_A_DOCK = ("dock road", "quay road", "wharf road", "dock street", "dockside lane")

#: How far off a dock a vessel may lie and still be alongside.
DEFAULT_APPROACH_M = 600.0

#: What a hull needs under it. Twenty feet, the owner's own figure for the ferry.
DEFAULT_DRAUGHT_M = 6.096


def looks_like_a_dock(key):
    """Whether a room's name suggests a landing place."""
    lowered = key.lower()
    if any(word in lowered for word in NOT_A_DOCK):
        return False
    return any(word in lowered for word in DOCK_WORDS)


def approach(latitude_deg, longitude_deg, elevation_at, radius_m,
             draught_m=DEFAULT_DRAUGHT_M, reach_m=DEFAULT_APPROACH_M, samples=32):
    """
    The nearest water off a dock that a hull can float in.

    Args:
        latitude_deg (float): The dock.
        longitude_deg (float): The dock.
        elevation_at (callable): `(lat, lon) -> metres`.
        radius_m (float): The planet's radius.
        draught_m (float, optional): What the vessel needs under it.
        reach_m (float, optional): How far off the dock to look.
        samples (int, optional): Bearings tried.

    Returns:
        found (dict or None): `latitude_deg`, `longitude_deg`, `bearing_deg`, `distance_m`
        and `depth_m` of the nearest floatable water, or None if there is none.

    Notes:
        Returns the NEAREST such point rather than the deepest. A ferry wants to come
        alongside, not to stand off in the deepest water it can find.
    """
    metres_per_degree = math.pi * radius_m / 180.0
    best = None
    for step_m in range(50, int(reach_m) + 1, 50):
        for index in range(samples):
            bearing = 360.0 * index / samples
            angle = math.radians(bearing)
            degrees = step_m / metres_per_degree
            lat = latitude_deg + degrees * math.cos(angle)
            lon = longitude_deg + degrees * math.sin(angle) / math.cos(
                math.radians(latitude_deg))
            depth = -elevation_at(lat, lon)
            if depth >= draught_m:
                best = {"latitude_deg": round(lat, 6), "longitude_deg": round(lon, 6),
                        "bearing_deg": round(bearing, 1), "distance_m": step_m,
                        "depth_m": round(depth, 2)}
                break
        if best:
            break
    return best


def find(areas, elevation_at, radius_m, draught_m=DEFAULT_DRAUGHT_M,
         reach_m=DEFAULT_APPROACH_M):
    """
    Every room a vessel could tie up at, with its approach.

    Args:
        areas (list): Worldfile areas, each with `name` and placed `rooms`.
        elevation_at (callable): `(lat, lon) -> metres`.
        radius_m (float): The planet's radius.
        draught_m (float, optional): What the vessel needs.
        reach_m (float, optional): How far off to look for water.

    Returns:
        docks (list): One record per room that passed BOTH tests, and a `rejected` count
        for the ones whose name suggested a dock and whose water did not.

    """
    found, rejected = [], []
    for area in areas:
        for room in area.get("rooms", []):
            if not looks_like_a_dock(room["key"]):
                continue
            water = approach(room["latitude_deg"], room["longitude_deg"],
                             elevation_at, radius_m, draught_m, reach_m)
            if water is None:
                # The name promised a landing and the water refused it. Counted rather
                # than dropped: a game with fourteen quays and no navigable water at any
                # of them has a problem worth reporting, not hiding.
                rejected.append({"area": area["name"], "room": room["key"],
                                 "id": room["id"],
                                 "why": "no water %.2f m deep within %.0f m"
                                        % (draught_m, reach_m)})
                continue
            found.append({
                "area": area["name"], "room": room["key"], "id": room["id"],
                "latitude_deg": room["latitude_deg"],
                "longitude_deg": room["longitude_deg"],
                "elevation_m": room.get("elevation_m"),
                "approach": water,
            })
    return {"docks": found, "rejected": rejected,
            "draught_m": draught_m, "approach_reach_m": reach_m}
