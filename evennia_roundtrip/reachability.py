"""Can you get there? Asked of a whole planet, not of one area.

`area_lint` is a law about the inside of an area - its corridors, its dead ends, its loop
density - and it is a pure graph function over one area's own rooms. This is the law one
level up, and it is the one that decides whether a place exists in the world or merely
exists in the database::

    Every settlement is reachable: by land from a neighbouring area, or by water
    from a dock that a vessel can actually get to.

**"Has a dock room" is not the test, and the difference is the whole point.** A room called
`Blackstone Quay` with two feet of water off it is a quay in name only - no hull can come
alongside, so a town whose sole connection is that room is as cut off as a town with no
quay at all, while looking connected to anybody reading the file. So a sea connection means
a dock whose approach was *measured* and found deep enough, which is exactly the
distinction `docks.find` already draws when it separates its `docks` from its `rejected`.

**This catches the failure that has already happened once.** Re-importing a zone deletes
the far side's way back, because the exits into it belonged to the rooms that were
replaced. The area still looks complete - all its own rooms, all its own exits, a clean
`area_lint` pass - and it is now an island nobody can walk to. Nothing inside an area can
detect that; only the graph between areas can.

The check is deliberately cheap and total: it runs over the worldfile, needs no database,
no engine and no server, and answers for every area at once. A law you can only afford to
run sometimes is a law that gets skipped.
"""

#: How a place can be joined to the rest of the world.
BY_LAND = "land"
BY_SEA = "sea"


def _room_owners(world):
    """Which area each room id belongs to."""
    owner = {}
    for area in world.get("areas", []):
        for room in area.get("rooms", []):
            owner[room["id"]] = area["name"]
    return owner


def land_seams(world):
    """
    Every exit that leaves one area and arrives in another.

    Returns:
        seams (dict): `(from_area, to_area) -> [exit records]`.

    Notes:
        Directed, and kept that way rather than symmetrised. A one-way seam is a real
        thing a builder can create by accident - the re-import failure produces exactly
        one - and folding the pair together would hide it behind its own return leg.
    """
    owner = _room_owners(world)
    seams = {}
    for area in world.get("areas", []):
        for exit_ in area.get("exits", []):
            here = owner.get(exit_["source"])
            there = owner.get(exit_["destination"])
            if here is None or there is None or here == there:
                continue
            seams.setdefault((here, there), []).append(exit_)
    return seams


def sea_connections(world):
    """
    Which areas a vessel can reach, and which only look as though it can.

    Returns:
        found (dict): `area -> {"docks": [...], "rejected": [...]}`.

    Notes:
        A rejected dock is reported rather than dropped. An area whose only quay was
        refused for want of water is the interesting case: it is unreachable, it reads as
        reachable, and the reason is a number somebody can go and change.
    """
    maritime = world.get("maritime", {})
    found = {}
    for dock in maritime.get("docks", []):
        found.setdefault(dock["area"], {"docks": [], "rejected": []})["docks"].append(dock)
    for dock in maritime.get("docks_rejected", []):
        found.setdefault(dock["area"], {"docks": [], "rejected": []})["rejected"].append(dock)
    return found


def check(world, origin=None):
    """
    Whether every area in this world can be got to.

    Args:
        world (dict): A loaded worldfile.
        origin (str, optional): The area reachability is measured *from*. Defaults to the
            first area carrying a working dock, else the first area in the file.

    Returns:
        report (dict): `ok`, the `origin` used, a per-area verdict, and `unreachable`.

    Notes:
        **Connected is not the same as reachable, and this asks the second one.** A pair
        of areas joined only to each other is a connected component and is still a place
        no player can get to. So this walks outward from one origin rather than counting
        components, which is the question a player is really asking.
    """
    areas = [area["name"] for area in world.get("areas", [])]
    seams = land_seams(world)
    sea = sea_connections(world)
    afloat = {name for name, record in sea.items() if record["docks"]}

    if origin is None:
        origin = next((name for name in areas if name in afloat), areas[0] if areas else None)
    if origin is None:
        return {"ok": True, "origin": None, "areas": {}, "unreachable": [],
                "note": "this world has no areas"}

    # Every port shares the water, so any area with a working dock reaches any other.
    # That is what the passage network *is*: a statement that these quays are one system.
    neighbours = {name: set() for name in areas}
    for (here, there) in seams:
        neighbours.setdefault(here, set()).add(there)
    for name in afloat:
        neighbours.setdefault(name, set()).update(afloat - {name})

    seen, frontier = {origin}, [origin]
    while frontier:
        here = frontier.pop()
        for there in neighbours.get(here, ()):
            if there not in seen:
                seen.add(there)
                frontier.append(there)

    verdicts = {}
    for name in areas:
        how = []
        if any(there == name for (_here, there) in seams):
            how.append(BY_LAND)
        if name in afloat:
            how.append(BY_SEA)
        record = sea.get(name, {"docks": [], "rejected": []})
        verdicts[name] = {
            "reachable": name in seen,
            "by": how,
            "docks": len(record["docks"]),
            "docks_rejected": len(record["rejected"]),
            "ways_in": sorted({here for (here, there) in seams if there == name}),
            "ways_out": sorted({there for (here, there) in seams if here == name}),
        }
    unreachable = [name for name in areas if name not in seen]
    return {"ok": not unreachable, "origin": origin, "areas": verdicts,
            "unreachable": unreachable}


def report(world, origin=None):
    """The check as lines of text, for a build log."""
    found = check(world, origin=origin)
    lines = ["reachability from %r" % found["origin"]]
    for name, verdict in sorted(found["areas"].items()):
        mark = "ok " if verdict["reachable"] else "OUT"
        how = "+".join(verdict["by"]) or "nothing"
        note = ""
        if verdict["docks_rejected"] and not verdict["docks"]:
            note = "  (%d quay(s) refused for want of water)" % verdict["docks_rejected"]
        lines.append("  %s %-22s by %-9s in:%-2d out:%-2d docks:%d%s"
                     % (mark, name, how, len(verdict["ways_in"]),
                        len(verdict["ways_out"]), verdict["docks"], note))
    if found["unreachable"]:
        lines.append("UNREACHABLE: %s" % ", ".join(found["unreachable"]))
    else:
        lines.append("every area can be got to.")
    return "\n".join(lines)
