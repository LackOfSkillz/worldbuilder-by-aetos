"""Populate a region: choose sites, build areas, check the laws, and stream the result.

This is the orchestration. Everything it uses exists already and has been measured on its
own - the oracle, the site scorer, the culture table, the lattice builder, the content pack,
the run history. What this adds is the order they happen in and the gates between them.

**Seeds come first and they are places, not scores.** The shores of the sea are settled
before anything inland is considered, and any area that already exists is a seed too - the
fish camp on its island and the city on the north shore are where this world was already
inhabited, so the rest grows outward from them rather than from a fresh guess.

**Every area is gated before it is kept.** Shape against the building laws, prose against
the word and sentence bands, ground against the water. A failed area is discarded and the
site is left empty rather than shipped and repaired later, because a hundred areas is more
than anybody will read and the gate is the only thing that will notice.

**Only piers may stand in water.** A room named as a dock, quay, jetty, pier, bridge or
slipway is allowed to be wet; every other room must be on dry land. An area with a wet
room that is not one of those is refused outright - which is exactly the fault that put
eleven of eleven ferry-shore rooms under water while its anchor sat happily on the beach.
"""

import json
import math
import random

from . import areagen, cultures, pack as packmod, place, planet, runs, siting

#: Ten to thirty miles, in metres. A floor and a reach, not a target and a tolerance.
NEAR_M = 16093.0
FAR_M = 48280.0

#: How far apart the rooms of one area stand.
ROOM_SPACING_M = 40.0


def _haversine(lat1, lon1, lat2, lon2, radius_m):
    a1, o1, a2, o2 = map(math.radians, (lat1, lon1, lat2, lon2))
    h = (math.sin((a2 - a1) / 2) ** 2
         + math.cos(a1) * math.cos(a2) * math.sin((o2 - o1) / 2) ** 2)
    return 2 * math.asin(math.sqrt(h)) * radius_m


def seeds_for(at, radius_m, centre, existing=(), count=7, separation_m=200_000.0):
    """
    Where settlement starts: what is already there, then a landfall on every shore.

    Args:
        at (callable): The elevation oracle.
        radius_m (float): The planet's radius.
        centre (tuple): `(lat, lon)` in the middle of the water this region is built around.
        existing (iterable, optional): `(lat, lon)` of areas the world already has.
        count (int, optional): How many shore seeds to add.
        separation_m (float, optional): How far a new seed must be from an existing one.

    Returns:
        seeds (list): `(lat, lon)`, existing places first.

    Notes:
        **A world that already has places starts from them.** The fish camp is on the
        island in the middle and the city is on the north shore; those are not candidates
        to be scored, they are facts, and everything else grows around them. Generating a
        fresh seed on top of one would put two settlements in one bay and waste a bearing.
    """
    seeds = [tuple(p) for p in existing]
    for candidate in siting.shore_seeds(at, radius_m, centre, count=count):
        if all(_haversine(candidate[0], candidate[1], s[0], s[1], radius_m) >= separation_m
               for s in seeds):
            seeds.append(candidate)
    return seeds


def place_rooms(built, names, anchor, radius_m, spacing_m=ROOM_SPACING_M, base_id=1):
    """Give every room in a lattice a position on the globe."""
    rooms, exits = areagen.rooms_and_exits(built, names, base_id)
    metres_per_degree = math.pi * radius_m / 180.0
    coslat = math.cos(math.radians(anchor[0]))
    for room in rooms:
        cx, cy = room["cell"][0], room["cell"][1]
        room["latitude_deg"] = round(anchor[0] + (cy * spacing_m) / metres_per_degree, 6)
        room["longitude_deg"] = round(
            anchor[1] + (cx * spacing_m) / (metres_per_degree * coslat), 6)
    return rooms, exits


def dry_enough(rooms, at):
    """
    Whether every room stands where it should.

    Returns:
        report (dict): `wet_unexpected` rooms and whether the area `fits`.

    Notes:
        **Only piers may be wet.** `place.water_room` names the exceptions - dock, quay,
        jetty, pier, bridge, slipway, steps - and everything else must be on dry ground.
        The anchor being on land is not the test and never was: ferry_shore's anchor sat on
        a beach while all eleven of its rooms stood in an eleven-metre dredged channel.
    """
    wet = []
    for room in rooms:
        height = at(room["latitude_deg"], room["longitude_deg"])
        room["elevation_m"] = round(height, 3)
        room["submerged"] = height < 0.0
        if height < 0.0 and not place.water_room(room["key"]):
            wet.append(room["key"])
    return {"wet_unexpected": wet, "fits": not wet}


def area_from_site(site, culture, names, at, radius_m, rng, base_id, size=None):
    """
    One finished area, or None with the reason it was refused.

    Returns:
        built (dict): `area`, `shape`, `problems`.
    """
    kind = culture.size if culture else "village"
    size = size or areagen.TYPE_SIZE.get(kind, 47)
    lattice, problems = areagen.build_lattice(size, rng)
    if problems:
        return {"area": None, "problems": ["shape: " + "; ".join(problems)]}

    anchor = (site["latitude_deg"], site["longitude_deg"])
    rooms, exits = place_rooms(lattice, names, anchor, radius_m, base_id=base_id)
    ground = dry_enough(rooms, at)
    if not ground["fits"]:
        return {"area": None,
                "problems": ["in water: " + ", ".join(ground["wet_unexpected"][:4])]}

    area = {
        "name": site.get("name") or (culture.name.replace(" ", "_") if culture else "area"),
        "rooms": rooms,
        "exits": exits,
        "anchor": {"latitude_deg": anchor[0], "longitude_deg": anchor[1],
                   "bearing_deg": 0.0, "spacing_m": ROOM_SPACING_M},
        "layout_quality": lattice["shape"],
    }
    return {"area": area, "shape": lattice["shape"], "problems": []}
