"""Sounding the finished world: is anything standing in water that should not be?

**A generator that cannot check its own work leaves the checking to whoever looks at the
picture.** Rooms on the sea floor and roads across straits were both found by eye, from
screenshots, after the run was over - and both had been in every world before that one.

Two different faults, and only one of them is visible in a room's own record.

  * A ROOM below sea level. Cheap to find: the room carries its height.
  * A SPAN between two rooms that crosses water while both ends stand on dry land. Rooms
    are five miles apart and a channel two miles wide fits between them unnoticed, so
    nothing about either room says anything is wrong. This is the one that put a road
    across eighteen hundred metres of open sea.

Both are reported per place, so a run says what is wrong with which area rather than only
how many faults it has.
"""

#: How many points are looked at between one room and the next.
#:
#: Rooms are about eight kilometres apart, so this is a reading every four hundred metres -
#: fine enough to find a channel a boat would use, coarse enough to cost nothing.
SPAN_SAMPLES = 20

#: How much of a span must be under water before the span counts as crossing it.
#:
#: One sample is enough. A road that dips below the waterline anywhere is a road with a
#: paddle in the middle of it, and the tolerance exists only so a rounding error at a
#: shoreline does not condemn a causeway.
SPAN_SHARE = 0.1


def _rooms_on_the_ground(place):
    """
    The rooms that are places on the map.

    An interior hangs off its street and has no position of its own worth sounding - and
    including it walks the line out to the shrine beside the road and back, which is how a
    road came to look like it crossed a strait it never went near.
    """
    return [room for room in (place.get("rooms") or ())
            if not room.get("interior") and room.get("latitude_deg") is not None]


def wet_rooms(place):
    """
    Args:
        place (dict): An area or a road.

    Returns:
        rooms (list): Every room of it standing below sea level.
    """
    return [room for room in _rooms_on_the_ground(place)
            if (room.get("elevation_m") or 0.0) < 0.0]


def wet_spans(place, at, samples=SPAN_SAMPLES, share=SPAN_SHARE):
    """
    Args:
        place (dict): An area or a road.
        at (callable): `(lat, lon) -> metres` above datum.
        samples (int): Readings between one room and the next.
        share (float): How much of a span must be wet to count.

    Returns:
        spans (list): `(from_id, to_id, deepest)` for every span that crosses water.

    Notes:
        Walked in stored order, which for a road is the order it runs. An area's rooms are
        a lattice rather than a line, so this finds the ones that happen to be listed
        consecutively - useful, and not the whole story for an area. `wet_rooms` is the
        check that matters there.
    """
    found = []
    rooms = _rooms_on_the_ground(place)
    for one, two in zip(rooms, rooms[1:]):
        under, deepest = 0, 0.0
        for step in range(1, samples + 1):
            part = step / (samples + 1.0)
            lat = one["latitude_deg"] + (two["latitude_deg"] - one["latitude_deg"]) * part
            lon = one["longitude_deg"] + (two["longitude_deg"] - one["longitude_deg"]) * part
            height = at(lat, lon)
            if height < 0.0:
                under += 1
                deepest = min(deepest, height)
        if under >= max(1, share * samples):
            found.append((one["id"], two["id"], round(deepest, 1)))
    return found


def check(document, at, roads=True):
    """
    Sound every place in a world.

    Args:
        document (dict): The worldfile.
        at (callable): The elevation oracle.
        roads (bool): Whether to sound the roads as well as the areas.

    Returns:
        report (dict): `areas` and `roads`, each a list of what is wrong with which place,
            plus the totals a run summary carries.

    Notes:
        **Reported, not refused.** By the time this runs the world is built; the value of
        it is that a run says so in its own summary instead of somebody finding it in a
        screenshot a day later. The refusing is done where the road is laid.
    """
    out = {"areas": [], "roads": [], "rooms_under_water": 0, "spans_over_water": 0}
    kinds = [("areas", document.get("areas") or ())]
    if roads:
        kinds.append(("roads", document.get("roads") or ()))
    for key, places in kinds:
        for place in places:
            drowned = wet_rooms(place)
            crossings = wet_spans(place, at) if key == "roads" else []
            if not drowned and not crossings:
                continue
            out[key].append({
                "name": place.get("display_name") or place.get("name"),
                "rooms_under_water": len(drowned),
                "deepest_room_m": min((r.get("elevation_m") or 0.0) for r in drowned)
                if drowned else None,
                "spans_over_water": len(crossings),
                "deepest_span_m": min((deep for _a, _b, deep in crossings), default=None),
            })
            out["rooms_under_water"] += len(drowned)
            out["spans_over_water"] += len(crossings)
    return out
