"""The ferry network on an inland sea: who is joined to whom, and how long the crossing is.

**A ferry network is a shape, not a list of crossings.** The generator already knows how to
put a boat between two areas that a road cannot join; what it did not know was that a sea
with settlements all round it wants a *service* - a hub, spokes to each shore, and a boat you
can wait for. Left as pairwise crossings, every town on the water got its own private ferry
to whichever neighbour the road-builder happened to refuse, which is a set of accidents
rather than a network.

**Clusters, not areas.** Twelve towns along one shore do not want twelve terminals; they want
one, with roads to it. So the shore is grouped by proximity and each group sends one terminal
to the water - which is also what stops a fifty-line timetable nobody can read.

**The times are spread across the band, not clamped into it.** Clamping gives a world where
almost every crossing takes the same thirty minutes, because most crossings are short and the
floor swallows them. Scaling maps the shortest crossing in the world onto the floor and the
longest onto the ceiling, so the times differ from each other for the same reason the
distances do.
"""

import math

#: The band a crossing must fall in, in minutes of real time.
#:
#: The floor is what makes a crossing a journey rather than a door; the ceiling is what stops
#: it being a punishment. Both are the host game's call and both are arguments.
SHORTEST_MINUTES = 30.0
LONGEST_MINUTES = 50.0

#: How near two coastal areas must be to share a terminal, as a multiple of the world's own
#: median gap between neighbouring coastal areas.
#:
#: **Measured from the world rather than guessed at.** The first version was a flat 120 km,
#: chosen against an older spacing rule; the areas in a real run sit about 207 km apart, so
#: it grouped nothing at all and every coastal town became its own terminal - sixty-two of
#: them, and seven hundred and seventy-one lines. A world that spaces its settlements
#: differently would have broken it the other way.
#:
#: A quarter again over the typical gap: near neighbours on one shore join, and the next
#: town along a different coast does not.
CLUSTER_SHARE = 1.25

#: How much of the line between two terminals must be water before they count as sharing a
#: sea, and how many points are sampled to find out.
#:
#: **A sea is what a boat can cross, and distance cannot tell you that.** Two towns on
#: opposite coasts of one continent are as near each other as two across a gulf. Sampling the
#: ground between them is cheap - no pathfinding, one oracle call per sample - and it is the
#: difference between an inland sea's own network and a line drawn over a mountain range.
#:
#: Not all of it, because a headland or an islet on the line is not a wall.
WATER_SHARE = 0.85
WATER_SAMPLES = 24

#: How much shorter a direct crossing must be than the same trip through the hub before it
#: is worth a line of its own.
#:
#: **A ring line has to earn its boats.** Two hulls and two berths at each end is a real cost,
#: and a crossing that saves a passenger five minutes over changing at the island is a line
#: nobody will use. Two thirds is a saving worth the timetable.
RING_SHARE = 0.66


def _haversine(lat_a, lon_a, lat_b, lon_b, radius_m):
    """Great-circle metres between two points."""
    phi_a, phi_b = math.radians(lat_a), math.radians(lat_b)
    d_phi = phi_b - phi_a
    d_lambda = math.radians(lon_b - lon_a)
    inner = (math.sin(d_phi / 2) ** 2
             + math.cos(phi_a) * math.cos(phi_b) * math.sin(d_lambda / 2) ** 2)
    return 2 * radius_m * math.asin(min(1.0, math.sqrt(inner)))


#: How far inland a town may stand and still send its people to a ferry.
#:
#: **Measured, not guessed**: on a 400-area world, 10 towns stood within 3 km of the sea -
#: five terminals, too few for a network - and 35 within about 11 km, in 21 clusters, which
#: is the shape a service wants. Twelve takes the second ring of `generate.inland_m` whole,
#: about half a day's walk to the boat.
FERRY_INLAND_KM = 12.0


def coastal(areas):
    """
    The areas that stand on water.

    Args:
        areas (list): Every area in the world.

    Returns:
        coastal (list): Those with somewhere a boat could come alongside.

    Notes:
        **A dock room, or a settlement whose ground reaches water.** This was docks alone,
        and docks were once any street called "Quay" or "Stair" - wrong, but it happened to
        mark most shore towns. When docks became only rooms truly at the water, 399 of 400
        areas had none, the planner saw one shore town, and the world had no ferries. What a
        ferry needs is a town a hull can reach, which the site already measured:
        `harbour_m` (deep water within 3 km) or `landing_m` (a landing within 800 m), or a
        town within `FERRY_INLAND_KM` of the sea. The terminal's own berth is built at the
        shore when the line is laid.
    """
    def reaches_water(area):
        if area.get("purpose") == "hunting":
            return False
        if area.get("harbour_m") is not None or area.get("landing_m") is not None:
            return True
        inland = area.get("inland_km")
        return inland is not None and inland <= FERRY_INLAND_KM

    return [area for area in areas if (area.get("docks") or 0) > 0 or reaches_water(area)]


def spacing(areas, radius_m):
    """
    The world's own median gap between neighbouring coastal areas, in metres.

    Args:
        areas (list): Coastal areas.
        radius_m (float): The planet's radius.

    Returns:
        metres (float): The median nearest-neighbour distance, or 0.0 if there is no pair.

    Notes:
        What every distance threshold here is scaled from, so a world built to a different
        spacing rule gets thresholds that suit it instead of ones that suited the last one.
    """
    if len(areas) < 2:
        return 0.0
    gaps = []
    for one in areas:
        gaps.append(min(_haversine(one["latitude_deg"], one["longitude_deg"],
                                   other["latitude_deg"], other["longitude_deg"], radius_m)
                        for other in areas if other is not one))
    gaps.sort()
    middle = len(gaps) // 2
    return gaps[middle] if len(gaps) % 2 else (gaps[middle - 1] + gaps[middle]) / 2.0


def share_a_sea(at, one, other, radius_m, share=WATER_SHARE, samples=WATER_SAMPLES):
    """
    Whether a boat could cross from one place to the other without meeting land.

    Args:
        at (callable): `(lat, lon) -> metres` above datum.
        one (dict), other (dict): The two areas.
        radius_m (float): The planet's radius.
        share (float): How much of the line must be water.
        samples (int): How many points to look at.

    Returns:
        together (bool): Whether they stand on the same water.

    Notes:
        A straight sample rather than a route: this decides which terminals belong to one
        network, and the network's own crossings are routed properly afterwards. Getting this
        wrong costs a line that is later refused, not a boat sailed over a hill.
    """
    if at is None:
        return True
    wet = 0
    for step in range(1, samples + 1):
        part = step / (samples + 1.0)
        lat = one["latitude_deg"] + (other["latitude_deg"] - one["latitude_deg"]) * part
        lon = one["longitude_deg"] + (other["longitude_deg"] - one["longitude_deg"]) * part
        try:
            if at(lat, lon) < 0.0:
                wet += 1
        except Exception:  # noqa: BLE001 - an oracle that will not answer is not water
            return False
    return wet >= share * samples


def seas(terminals, at, radius_m):
    """
    Terminals grouped into the bodies of water they stand on.

    Args:
        terminals (list): One area per shore.
        at (callable): The elevation oracle.
        radius_m (float): The planet's radius.

    Returns:
        seas (list): Lists of terminals, largest first.

    Notes:
        **One network per sea, because a hub is only central to its own water.** Run over a
        whole planet, "the most central terminal" is a place in the middle of a continent
        that no boat from anywhere can reach.
    """
    remaining = list(terminals)
    found = []
    while remaining:
        group = [remaining.pop()]
        growing = True
        while growing:
            growing = False
            for other in list(remaining):
                if any(share_a_sea(at, one, other, radius_m) for one in group):
                    group.append(other)
                    remaining.remove(other)
                    growing = True
        found.append(group)
    found.sort(key=len, reverse=True)
    return found


def clusters(areas, radius_m, within_m=None):
    """
    Coastal areas grouped into the shores they share.

    Args:
        areas (list): Coastal areas.
        radius_m (float): The planet's radius.
        within_m (float, optional): How near two areas must be to share a terminal. None
            measures the world's own spacing, which is what a caller should normally do.

    Returns:
        clusters (list): Lists of areas, largest first.

    Notes:
        Single-link: a chain of towns strung along one shore is one cluster even when its two
        ends are far apart, which is what a shore IS. The alternative - grouping by distance
        to a centre - cuts a long coast in half at an arbitrary place.
    """
    if within_m is None:
        within_m = spacing(areas, radius_m) * CLUSTER_SHARE
    remaining = list(areas)
    found = []
    while remaining:
        group = [remaining.pop()]
        growing = True
        while growing:
            growing = False
            for other in list(remaining):
                near = any(_haversine(one["latitude_deg"], one["longitude_deg"],
                                      other["latitude_deg"], other["longitude_deg"],
                                      radius_m) <= within_m
                           for one in group)
                if near:
                    group.append(other)
                    remaining.remove(other)
                    growing = True
        found.append(group)
    found.sort(key=len, reverse=True)
    return found


def terminal_of(group):
    """
    Which area in a cluster the boats call at.

    Args:
        group (list): One cluster's areas.

    Returns:
        area (dict): The one with the most dock rooms, then one with a harbour, then the
            most rooms.

    Notes:
        The busiest waterfront rather than the nearest point: a hamlet on a headland is closer
        to the water than the town behind it and is not where a service would call.
    """
    return max(group, key=lambda area: ((area.get("docks") or 0),
                                        area.get("harbour_m") is not None,
                                        len(area.get("rooms") or ())))


def hub_of(terminals, radius_m):
    """
    The terminal every other one is joined to.

    Args:
        terminals (list): One area per cluster.
        radius_m (float): The planet's radius.

    Returns:
        area (dict or None): The most central terminal, or None if there are none.

    Notes:
        **Central, because a hub that is not central makes long spokes out of short ones.**
        On an inland sea with an island in it that picks the island, which is the shape a
        ferry network takes for the reason a wheel does.
    """
    if not terminals:
        return None
    if len(terminals) <= 2:
        return terminals[0]
    return min(terminals,
               key=lambda here: sum(_haversine(here["latitude_deg"], here["longitude_deg"],
                                               there["latitude_deg"], there["longitude_deg"],
                                               radius_m)
                                    for there in terminals))


def pairs(terminals, radius_m, ring_share=RING_SHARE):
    """
    Which terminals are worth joining by boat.

    Args:
        terminals (list): One area per cluster.
        radius_m (float): The planet's radius.
        ring_share (float): How much shorter a direct hop must be to earn its own line.

    Returns:
        pairs (list): `(from_area, to_area)`, hub spokes first.

    Notes:
        Hub and spoke, plus the ring lines that pay for themselves. Every terminal reaches
        every other in at most one change, and a shore-to-shore crossing exists only where
        going round by the hub would be most of the journey again.
    """
    hub = hub_of(terminals, radius_m)
    if hub is None:
        return []
    spokes = [there for there in terminals if there is not hub]
    made = [(hub, there) for there in spokes]

    def gap(one, other):
        return _haversine(one["latitude_deg"], one["longitude_deg"],
                          other["latitude_deg"], other["longitude_deg"], radius_m)

    # **A ring line joins neighbours, and only neighbours.** Every pair that merely beats
    # the trip through the hub is most pairs on a wide sea - sixty-two terminals produced
    # seven hundred and seventy-one lines, which is not a timetable, it is a phone book. A
    # terminal gets at most one ring line, to its nearest neighbour, and only when going
    # round by the hub would be most of the journey again.
    seen = set()
    for one in spokes:
        others = [other for other in spokes if other is not one]
        if not others:
            continue
        nearest = min(others, key=lambda other: gap(one, other))
        through_hub = gap(one, hub) + gap(hub, nearest)
        if gap(one, nearest) > ring_share * through_hub:
            continue
        pair = tuple(sorted((one["name"], nearest["name"])))
        if pair in seen:
            continue
        seen.add(pair)
        made.append((one, nearest))
    return made


def minutes_for(metres, shortest_m, longest_m,
                floor=SHORTEST_MINUTES, ceiling=LONGEST_MINUTES):
    """
    How long one crossing should take, in minutes.

    Args:
        metres (float): This crossing's sailed distance.
        shortest_m (float): The shortest crossing in the world.
        longest_m (float): The longest.
        floor (float): What the shortest crossing takes.
        ceiling (float): What the longest takes.

    Returns:
        minutes (float): Inside the band, and proportional within it.

    Notes:
        **Spread, not clamped.** Clamping into the band gives a world where nearly every
        crossing sits on the floor, because most crossings are short - and a timetable where
        everything takes thirty minutes tells a player nothing about distance.

        When every crossing is the same length there is nothing to spread, and they all take
        the floor: the shortest crossing in the world is also the longest.
    """
    span = longest_m - shortest_m
    if span <= 0.0:
        return floor
    share = (metres - shortest_m) / span
    return floor + max(0.0, min(1.0, share)) * (ceiling - floor)


def berth_key(destination):
    """
    Args:
        destination (str): Where the boats from this berth go.

    Returns:
        key (str): What the room is called.

    Notes:
        **A berth per destination, named for the destination.** One quay serving four lines
        is four ways to board the wrong boat, and a player who has done that once has lost
        forty minutes to a mistake the world let them make.

        A destination that already carries its article does not get a second one: display
        names come both ways and "the the island berth" is what happens when the room name
        is built without asking.
    """
    said = str(destination or "").strip()
    if said.lower().startswith("the "):
        said = said[4:]
    return "the %s berth" % said


def plan(areas, radius_m, at=None, sailed_m=None, within_m=None,
         floor=SHORTEST_MINUTES, ceiling=LONGEST_MINUTES):
    """
    The whole network: which terminals, which lines, and how long each crossing takes.

    Args:
        areas (list): Every area in the world.
        radius_m (float): The planet's radius.
        at (callable, optional): The elevation oracle, used to tell one sea from another.
            None puts every terminal on one water, which is only right for a test.
        sailed_m (callable, optional): `(from_area, to_area) -> metres` by sea, or None to
            use the straight-line distance. The generator passes its own sea router, so a
            line round a headland is timed by the water it actually crosses.
        within_m (float): How near two areas must be to share a terminal.
        floor (float): Minutes for the shortest crossing.
        ceiling (float): Minutes for the longest.

    Returns:
        plan (dict): `terminals`, `lines`, and the `hub` they turn about. Each line carries
            its two ends, its sailed distance and its crossing time in minutes.

    Notes:
        Lines with no sea route are dropped here rather than built and refused later - a berth
        to a place no boat can reach is a room a player waits in for ever.
    """
    groups = clusters(coastal(areas), radius_m, within_m)
    terminals = [terminal_of(group) for group in groups]
    waters = seas(terminals, at, radius_m)
    # Only bodies of water with something on both sides of them run a service.
    waters = [water for water in waters if len(water) >= 2]
    hub = hub_of(waters[0], radius_m) if waters else None
    if not waters:
        return {"terminals": terminals, "lines": [], "hub": hub, "seas": 0}

    wanted = []
    for water in waters:
        wanted.extend(pairs(water, radius_m))

    measured = []
    for one, other in wanted:
        metres = (sailed_m(one, other) if sailed_m
                  else _haversine(one["latitude_deg"], one["longitude_deg"],
                                  other["latitude_deg"], other["longitude_deg"], radius_m))
        if not metres:
            continue
        measured.append((one, other, metres))
    if not measured:
        return {"terminals": terminals, "lines": [], "hub": hub, "seas": len(waters)}

    shortest = min(metres for _one, _other, metres in measured)
    longest = max(metres for _one, _other, metres in measured)
    hubs = {hub_of(water, radius_m)["name"] for water in waters}
    lines = []
    for one, other, metres in measured:
        minutes = minutes_for(metres, shortest, longest, floor, ceiling)
        lines.append({
            "name": "the %s to %s crossing" % (one["name"], other["name"]),
            "ends": [one["name"], other["name"]],
            "berths": [berth_key(other["name"]), berth_key(one["name"])],
            "metres": round(metres),
            "minutes": round(minutes, 1),
            # **What the timetable asks of the hull.** Written down because a crossing time
            # and a distance together are a claim about a boat, and on a big world that
            # claim can be an impossible one. A game can read this and refuse.
            "speed_ms": round(metres / (minutes * 60.0), 2),
            "hub": one["name"] in hubs or other["name"] in hubs,
        })
    return {"terminals": terminals, "lines": lines, "hub": hub, "seas": len(waters)}
