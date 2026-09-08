"""How places are joined, written down where a tool can read it.

A worldfile could already say two things about getting somewhere: an exit between two rooms,
and a dock with enough water for a ship. The world it describes has neither between the
ranger camp and the bayou - you get across in a kayak - so the reachability law reported
two of three areas unreachable, which was true of the file and false of the game.

**The missing idea is that a boat service is a fact about the world.** "A kayak crosses
between these two banks" and "a ferry runs between these two docks" are exactly as much a
part of the map as a road is, and they lived only as attributes on runtime objects that no
tool reading the worldfile could see. So they are written down here.

**Authored, not derived, and that is the whole point.** Nothing about the terrain implies a
ferry. Somebody decided there is one, decided where it calls and decided how long it takes,
and a generator that inferred boat services from coastline would be inventing them. What
this module does instead is *validate*: every endpoint a route names must be a room that
exists, in an area the file contains, and a route naming a room that is not there is an
error rather than a silently dead connection.

That last check is the one that earns its keep. The ferry's island terminal lives in an
area that was dropped from the published worldfile, so the route is real in the game and
broken in the file - and until now nothing could tell.
"""

#: A route somebody paddles or rows themselves.
BY_HAND = "small craft"

#: A route that runs to a timetable and carries you.
BY_SERVICE = "service"


def _index(world):
    """`room id -> (area name, room)` and `(area, room key) -> id`, for validation."""
    by_id, by_name = {}, {}
    for area in world.get("areas", []):
        for room in area.get("rooms", []):
            by_id[room["id"]] = (area["name"], room)
            by_name[(area["name"], room["key"])] = room["id"]
    return by_id, by_name


def route(kind, name, endpoints, craft=None, seconds=None, distance_m=None, passage=None):
    """
    One boat service, as a record.

    Args:
        kind (str): `BY_HAND` or `BY_SERVICE`.
        name (str): What it is called, for reports.
        endpoints (list): Two or more `{"area": ..., "room": ...}` it calls at.
        craft (str, optional): What makes the crossing.
        seconds (int, optional): How long it takes, one way.
        distance_m (float, optional): How far, measured.
        passage (int, optional): Index into `maritime.passages` for a route that follows
            an authored mark network, or None for one that simply crosses.

    Returns:
        record (dict): The route, ready to go in the worldfile.

    Notes:
        **Endpoints are named by area and room key, not by room id.** Ids are assigned by
        whichever database exported the file and change on every re-import; a route keyed on
        them would break on exactly the operation that already breaks seams. A name survives
        the round trip, and if the room is renamed the validator says so out loud.
    """
    return {"kind": kind, "name": name,
            "endpoints": [{"area": e["area"], "room": e["room"]} for e in endpoints],
            "craft": craft, "seconds": seconds, "distance_m": distance_m,
            "passage": passage}


def landing(area, room, draught_m, note=None):
    """
    A place a small boat can be brought ashore, whether or not a ship could.

    Notes:
        Recorded separately from `maritime.docks` because they answer different questions
        about the same shoreline. The camp's boat ramp is refused as a dock - correctly,
        at the six metres a hull needs - and is a perfectly good slipway at the three
        tenths of a metre a kayak needs. One list would have to pick a draught, and either
        choice makes the other kind of place invisible.
    """
    return {"area": area, "room": room, "draught_m": draught_m, "note": note}


def build(world, routes=(), landings=()):
    """
    The `connections` block for a worldfile, validated against its own rooms.

    Args:
        world (dict): The worldfile the connections belong to.
        routes (iterable, optional): Records from `route`.
        landings (iterable, optional): Records from `landing`.

    Returns:
        block (dict): `routes`, `landings`, `seams` and any `problems` found.

    Notes:
        Seams are derived rather than authored - they are already in the file as exits, and
        writing them twice would create a second thing to keep true. They are recorded so a
        reader can see the land joins without walking every exit of every area, and so a
        re-import that severs one shows up as a seam that has gone rather than as an area
        that quietly went quiet.
    """
    by_id, by_name = _index(world)
    problems = []

    checked_routes = []
    for record in routes:
        missing = [e for e in record["endpoints"] if (e["area"], e["room"]) not in by_name]
        for gap in missing:
            problems.append(
                "route %r calls at %s::%s, which this worldfile does not contain"
                % (record["name"], gap["area"], gap["room"]))
        resolved = dict(record)
        resolved["endpoints"] = [
            dict(e, id=by_name.get((e["area"], e["room"]))) for e in record["endpoints"]]
        resolved["complete"] = not missing
        checked_routes.append(resolved)

    checked_landings = []
    for record in landings:
        if (record["area"], record["room"]) not in by_name:
            problems.append("landing %s::%s is not a room in this worldfile"
                            % (record["area"], record["room"]))
            continue
        checked_landings.append(dict(record, id=by_name[(record["area"], record["room"])]))

    seams = []
    for area in world.get("areas", []):
        for exit_ in area.get("exits", []):
            here = by_id.get(exit_["source"])
            there = by_id.get(exit_["destination"])
            if here is None or there is None or here[0] == there[0]:
                continue
            seams.append({"from": {"area": here[0], "room": here[1]["key"]},
                          "to": {"area": there[0], "room": there[1]["key"]},
                          "name": exit_.get("name")})

    return {"routes": checked_routes, "landings": checked_landings, "seams": seams,
            "problems": problems}
