"""Populate a world: the one entry point that runs everything in order.

Every part of this has been measured on its own - the oracle, the site scorer, the culture
table, the lattice builder, the namer, the period lint, the run history. What this adds is
the order they happen in, the gates between them, and the record of what happened.

**Seeds first, and they are places rather than scores.** Whatever the world already has is
a seed: the fish camp on its island and the city on the north shore are not candidates to
be ranked, they are facts, and everything grows outward from them. Then a landfall on each
shore of the sea, so the first hundred areas ring the water instead of piling up beside
whichever bay happened to score highest.

**Every area is gated before it is kept, and a refusal is recorded rather than repaired.**
Shape against the building laws, prose against the word and sentence bands, ground against
the water, and every word against the period. A hundred areas is more than anybody will
read, so the gate is the only thing that will ever notice - and an area quietly patched to
pass would make the gate a formality.

**One line per area, written as it lands.** The viewer polls that file, so the pins appear
while the run is still going. It also means a run that dies at area sixty leaves sixty
areas somebody can look at and a manifest that says it failed, rather than a silent
half-world.
"""

import json
import math
import os
import random

from . import (areagen, cultures, naming, period, place, planet, populate,
               reachability, runs, siting)

#: How many of each culture a hundred-area world should hold.
#:
#: **Given, not derived.** These are the owner's numbers for this demo world, and the
#: proportions are what make it read as somebody's setting rather than as a uniform sample:
#: humans everywhere, dwarves in the mountains, and a quarter of the map given over to
#: things that will kill you. Scaled to whatever `count` is asked for, so the shape of the
#: world survives asking for twenty areas or two hundred.
QUOTAS = (
    ("human", 25),
    ("dwarf", 15),
    ("hunting", 15),
    ("elf", 10),
    ("gnome", 8),
    ("volgrin", 8),
    ("halfling", 6),
    ("saurathi", 5),
    ("felari", 5),
    ("lunari", 5),
    ("goblin", 5),
    ("aethari", 4),
    ("valran", 4),
)

#: How many NPCs stand in an area of each size, per room.
#:
#: A city is not a village with more streets: it is denser, and the density is what a player
#: feels as "somewhere busy". Hunting grounds are the other way - more things per room than
#: a village and none of them will talk to you.
NPC_DENSITY = {"city": 0.55, "seat": 0.5, "town": 0.45, "village": 0.38,
               "hamlet": 0.3, "camp": 0.7}

#: The trade rooms, by the name `naming` gives them, so shops can be counted rather than
#: guessed. A shop is a room; counting them means counting rooms.
TRADE_MARKERS = ("inn", "weaponsmith", "armourer", "general store", "alchemist",
                 "counting house", "healer", "stables", "shrine", "market stalls")


def voice_race(culture):
    """
    Which vocabulary an area is written in.

    A culture has a race when somebody lives there. A hunting ground has nobody, and a
    goblin camp has goblins whom the table does not call a race because they are not a
    playable one - so both are answered here rather than by bending the table.
    """
    if culture.race:
        return culture.race
    if culture.purpose == "hunting":
        return "wild"
    if "goblin" in culture.name:
        return "goblin"
    return "human"


def _haversine(lat1, lon1, lat2, lon2, radius_m):
    a1, o1, a2, o2 = map(math.radians, (lat1, lon1, lat2, lon2))
    h = (math.sin((a2 - a1) / 2) ** 2
         + math.cos(a1) * math.cos(a2) * math.sin((o2 - o1) / 2) ** 2)
    return 2 * math.asin(math.sqrt(h)) * radius_m


def wanted(count, quotas=QUOTAS):
    """
    How many areas of each kind a run of this size should produce.

    Returns:
        wanted (dict): Key to count, summing to `count`.

    Notes:
        Largest-remainder, so scaling to a count that does not divide evenly does not
        silently drop the smallest cultures - which are exactly the ones a demo is for.
    """
    total = sum(share for _, share in quotas)
    exact = [(key, count * share / total) for key, share in quotas]
    out = {key: int(value) for key, value in exact}
    short = count - sum(out.values())
    for key, value in sorted(exact, key=lambda kv: kv[1] - int(kv[1]), reverse=True):
        if short <= 0:
            break
        out[key] += 1
        short -= 1
    return out


def _key_for(culture):
    """Which quota a culture counts against."""
    if culture.purpose == "hunting":
        return "hunting"
    if "goblin" in culture.name:
        return "goblin"
    return culture.race or "human"


def river_points(document):
    """
    Where the fresh water is, out of a worldfile's own features.

    Notes:
        **Nothing can want a river until somebody hands one over.** `score_point` takes
        `rivers` and defaults it to empty, so a runner that forgets it produces a world
        where `fresh_m` is None everywhere - and every culture that needs fresh water is
        silently unplaceable. That is five halfling hamlets and every river town missing
        from a hundred-area run, with no error and nothing in the refusals to explain it.

        A carve is water. Raises are ground and are not asked about.
    """
    return [(feature["latitude_deg"], feature["longitude_deg"])
            for feature in (document.get("features") or ())
            if feature.get("compose") == "carve"]


#: How far from the water a riverside seed may be moved to find dry ground.
#:
#: Well inside the three kilometres that makes a site count as having fresh water, so a
#: seed stepped onto the bank still qualifies as riverside.
BANK_REACH_M = 2200.0


def _bank_beside(at, point, radius_m, reach_m=BANK_REACH_M, rings=4, rays=12):
    """The nearest dry ground beside a water point, or None if it is all water."""
    for ring in range(1, rings + 1):
        distance = reach_m * ring / rings
        for latitude, longitude in siting._ring(point[0], point[1], distance, radius_m, rays):
            if at(latitude, longitude) > 0.5:
                return (latitude, longitude)
    return None


def lake_points(document):
    """
    Where the standing fresh water is, out of a worldfile's `water` block.

    Notes:
        **The engine solves for lakes and Python cannot ask it.** `wb_water_run` is exported
        to WebAssembly and the viewer draws two hundred and fourteen lakes with it; the pyo3
        surface has no water function at all, so the generator is blind to every one of
        them. A halfling farm wants fresh water and a lake is fresh water - it was simply
        invisible.

        Rather than solve the water a second time in Python and have two answers to one
        question, the viewer writes what the engine told it into the worldfile and this
        reads it back. The worldfile is already the contract between the two halves; this
        is one more thing it carries.

        A painted lake is a carve and arrives through `river_points` instead. Both are
        fresh water and the scorer does not care which is which.
    """
    water = document.get("water") or {}
    points = []
    for body in water.get("bodies") or ():
        latitude = body.get("latitude_deg")
        longitude = body.get("longitude_deg")
        if latitude is not None and longitude is not None:
            points.append((latitude, longitude))
    return points


def grow_sites(at, radius_m, seeds, count, region=None, near_m=siting.SEPARATION_M,
               far_m=siting.LAND_LINK_M, classify=None, rivers=()):
    """
    Grow a settlement network outward from the seeds, ten to thirty miles at a step.

    Args:
        at (callable): The elevation oracle.
        radius_m (float): The planet's radius.
        seeds (list): `(lat, lon)` to start from - what the world already has, plus a
            landfall on each shore.
        count (int): How many sites are wanted.
        region (tuple, optional): The frame sites must fall inside.
        near_m (float): No two settlements closer than this.
        far_m (float): How far out from a settled place the next one may be founded.
        classify (callable, optional): Passed to the scorer.

    Returns:
        sites (list): Scored sites, seeds first, then outward in the order they were founded.

    Notes:
        **A uniform sample of the region cannot answer this, and `local_sites` says so at
        length.** Measured on this world: thirty thousand points over the demo region put
        five hundred and sixty-three of them on land, spread so widely that almost every one
        was refused for having "no water access and no neighbour within reach" - so the
        survey returned SEVEN sites whether it was asked for twelve or three hundred. The
        constraint was never the scoring; it was that no two candidates in the whole set
        could ever be neighbours.

        Growing outward inverts it. Every new site is founded within reach of one that
        already exists, which is both how settlement actually spreads and exactly the
        spacing that was asked for: near enough to walk between, far enough to be its own
        place.
    """
    if not isinstance(rivers, siting.WaterIndex):
        rivers = siting.WaterIndex(rivers)
    chosen = []
    for latitude, longitude in seeds:
        site = siting.score_point(at, latitude, longitude, radius_m, rivers=rivers,
                                  classify=classify, look_for_water=True)
        if site is None:
            # A seed is a fact about the world, not a candidate: if the scorer will not
            # have it, it still anchors the growth, so it goes in with a bare record.
            site = {"latitude_deg": latitude, "longitude_deg": longitude,
                    "elevation_m": at(latitude, longitude), "score": 0.0, "kind": "seed"}
        site["seeded"] = True
        chosen.append(site)

    def far_enough(latitude, longitude):
        return all(_haversine(latitude, longitude, other["latitude_deg"],
                              other["longitude_deg"], radius_m) >= near_m
                   for other in chosen)

    frontier = list(chosen)
    while frontier and len(chosen) < count:
        near = frontier.pop(0)
        for candidate in siting.local_sites(at, radius_m, near, far_m, rivers=rivers,
                                            classify=classify):
            if len(chosen) >= count:
                break
            latitude = candidate["latitude_deg"]
            longitude = candidate["longitude_deg"]
            if not siting.in_region(latitude, longitude, region):
                continue
            if not far_enough(latitude, longitude):
                continue
            candidate["seeded"] = False
            chosen.append(candidate)
            frontier.append(candidate)
    return chosen


#: How far a road may run between two areas before it stops being a walk.
#:
#: Two hundred kilometres. Beyond that the link is a voyage or a road nobody would take on
#: foot, and pretending otherwise would satisfy the reachability check while stranding
#: somebody in practice.
ROAD_REACH_M = 200_000.0


def _where(area):
    """An area's position, however the file happens to record it.

    Areas this run made carry `latitude_deg` at the top level; areas already in the
    worldfile carry it under `anchor`, which is the older shape and still the one on disk.
    A road between a new area and an old one has to read both.
    """
    if area.get("latitude_deg") is not None:
        return (area["latitude_deg"], area["longitude_deg"])
    anchor = area.get("anchor") or {}
    if anchor.get("latitude_deg") is not None:
        return (anchor["latitude_deg"], anchor["longitude_deg"])
    rooms = area.get("rooms") or []
    for room in rooms:
        if room.get("latitude_deg") is not None:
            return (room["latitude_deg"], room["longitude_deg"])
    return None


def _nearest_rooms(here, there, radius_m):
    """The two rooms, one in each area, that a road between them would join."""
    best = None
    for room_a in here["rooms"]:
        for room_b in there["rooms"]:
            gap = _haversine(room_a["latitude_deg"], room_a["longitude_deg"],
                             room_b["latitude_deg"], room_b["longitude_deg"], radius_m)
            if best is None or gap < best[0]:
                best = (gap, room_a, room_b)
    return best


def _bearing(lat1, lon1, lat2, lon2):
    a1, o1, a2, o2 = map(math.radians, (lat1, lon1, lat2, lon2))
    y = math.sin(o2 - o1) * math.cos(a2)
    x = math.cos(a1) * math.sin(a2) - math.sin(a1) * math.cos(a2) * math.cos(o2 - o1)
    return (math.degrees(math.atan2(y, x)) + 360.0) % 360.0


#: The eight compass names in bearing order, so a road runs the way it actually points.
COMPASS = ("north", "northeast", "east", "southeast",
           "south", "southwest", "west", "northwest")
OPPOSITE = {"north": "south", "south": "north", "east": "west", "west": "east",
            "northeast": "southwest", "southwest": "northeast",
            "northwest": "southeast", "southeast": "northwest"}


def _free_name(room, area, direction):
    """A compass exit the room does not already use, or a named road instead."""
    taken = {e["name"] for e in area["exits"] if e["source"] == room["id"]}
    if direction not in taken:
        return direction
    return "the %s road" % direction


def connect_areas(areas, radius_m, reach_m=ROAD_REACH_M):
    """
    Lay roads until every area is joined to the network.

    Args:
        areas (list): Finished areas, each with `rooms` carrying positions.
        radius_m (float): The planet's radius.
        reach_m (float): The longest road worth walking.

    Returns:
        roads (list): One record per road laid, for the manifest.

    Notes:
        **A spanning tree, not a mesh.** Every area is joined to the nearest area already
        on the network - which is Prim's algorithm and is also how roads actually appear:
        somewhere new gets a track to the nearest somewhere old. What comes out is exactly
        what was asked for, a bus with hubs rather than a lattice: the big places end up
        with several roads because several neighbours were nearest to them, and the small
        ones end up with one.

        **The failure this prevents has a name here.** The fish camp is on an island and is
        reachable only by kayak; take the kayak away and it is a place a player can see and
        never visit. An area with no road and no dock is that, and the reachability check
        cannot invent one - it can only report it, which is why the roads are laid before
        the check runs rather than after it complains.
    """
    placed = [area for area in areas if _where(area) is not None and area.get("rooms")]
    if len(placed) < 2:
        return []
    joined = [placed[0]]
    outside = list(placed[1:])
    roads = []
    while outside:
        best = None
        for area in outside:
            here = _where(area)
            for other in joined:
                there = _where(other)
                gap = _haversine(here[0], here[1], there[0], there[1], radius_m)
                if best is None or gap < best[0]:
                    best = (gap, area, other)
        gap, area, other = best
        outside.remove(area)
        joined.append(area)
        # **The road is always laid, and a long one is flagged rather than skipped.** The
        # first version refused links over the reach and left those areas unjoined, which
        # made connectivity a matter of whether some later area happened to bridge them -
        # nine links were skipped in a hundred-area run and the world was connected by luck.
        # A two-hundred-and-fifty kilometre road is a hard walk; an area nobody can reach at
        # all is a place a player can see and never visit, which is the failure that was
        # actually named. So it is joined, and the manifest says which roads are long enough
        # to want a ship instead.
        found = _nearest_rooms(other, area, radius_m)
        if found is None:
            continue
        other.setdefault("exits", [])
        area.setdefault("exits", [])
        room_gap, room_a, room_b = found
        heading = _bearing(room_a["latitude_deg"], room_a["longitude_deg"],
                           room_b["latitude_deg"], room_b["longitude_deg"])
        direction = COMPASS[int((heading + 22.5) % 360 // 45)]
        out_name = _free_name(room_a, other, direction)
        back_name = _free_name(room_b, area, OPPOSITE[direction])
        other["exits"].append({"source": room_a["id"], "name": out_name,
                               "destination": room_b["id"], "road": True})
        area["exits"].append({"source": room_b["id"], "name": back_name,
                              "destination": room_a["id"], "road": True})
        roads.append({"from": other["name"], "to": area["name"],
                      "metres": round(room_gap), "laid": True, "direction": direction,
                      "long": room_gap > reach_m})
    return roads


def gate(area, culture, at, shape):
    """
    Everything that must be true before an area is kept.

    Returns:
        problems (list): Empty when the area may be shipped.

    Notes:
        **Four checks and they fail for different reasons**, so they are reported
        separately: a shape fault is the lattice builder's, a wet room is the site's, a
        short description is the namer's, and an anachronism is the vocabulary's. Rolling
        them into one boolean would make a run's failures unreadable.
    """
    problems = ["shape: " + why for why in areagen.fits_laws(shape)]

    ground = populate.dry_enough(area["rooms"], at)
    if not ground["fits"]:
        problems.append("wet: " + ", ".join(ground["wet_unexpected"][:3]))

    for room in area["rooms"]:
        words = len((room.get("desc") or "").split())
        if not areagen.DESC_WORD_BAND[0] <= words <= areagen.DESC_WORD_BAND[1]:
            problems.append("prose: room %s is %d words" % (room["id"], words))
            break

    for problem in period.check_area(area)[:3]:
        problems.append("period: " + problem)
    return problems


def counts(area, culture):
    """
    The NPCs, shops and docks an area holds, counted from its rooms rather than invented.

    Notes:
        **Counted, because a predicted number is a different claim.** The tally beside the
        globe says what the world contains; if these were the quota's intention rather than
        the rooms' contents, the readout would be describing the plan while the pins
        described the world, and the two would drift apart without anybody noticing.
    """
    rooms = area["rooms"]
    shops = sum(1 for room in rooms
                if any(marker in room["key"].lower() for marker in TRADE_MARKERS))
    docks = sum(1 for room in rooms if place.water_room(room["key"]))
    density = NPC_DENSITY.get(culture.size, 0.4)
    return {"room_count": len(rooms), "shops": shops, "docks": docks,
            "npcs": int(round(len(rooms) * density))}


#: What the progress feed carries: enough to draw a pin and fill a hover card, and none of
#: the room bodies. A hundred areas of sixty rooms is six thousand descriptions, and the
#: viewer polls this file every second.
FEED_FIELDS = ("name", "display_name", "latitude_deg", "longitude_deg", "race", "purpose",
               "faction", "culture", "size", "level_band", "from_origin_km", "npcs",
               "shops", "docks")


def feed_line(area):
    """One area as the viewer wants it: counts, not contents."""
    line = {key: area[key] for key in FEED_FIELDS if key in area}
    line["rooms"] = area["room_count"]
    return line


def build_area(site, culture, at, radius_m, rng, base_id, origin):
    """
    One finished, named, gated area - or None with the reasons it was refused.

    Returns:
        result (dict): `area` and `problems`.
    """
    kind = culture.size
    size = areagen.TYPE_SIZE.get(kind, 47)
    style = areagen.TYPE_STYLE.get(kind, "organic")
    lattice, shape_problems = areagen.build_lattice(size, rng, style=style)
    if lattice is None:
        return {"area": None, "problems": ["shape: " + "; ".join(shape_problems or ["none"])]}

    anchor, rooms, exits, ground = populate._fit_on_land(
        lattice, ["room"] * size, (site["latitude_deg"], site["longitude_deg"]),
        at, radius_m, base_id)

    area = {
        "name": culture.name.replace(" ", "_"),
        "rooms": rooms,
        "exits": exits,
        "anchor": {"latitude_deg": round(anchor[0], 6),
                   "longitude_deg": round(anchor[1], 6),
                   "bearing_deg": 0.0, "spacing_m": populate.ROOM_SPACING_M},
        "layout_quality": lattice["shape"],
    }
    race = voice_race(culture)
    naming.name_and_describe(area, race, rng,
                             settled=culture.purpose not in ("hunting",))
    area["name"] = area["display_name"].lower()

    problems = gate(area, culture, at, lattice["shape"])
    if problems:
        return {"area": None, "problems": problems}

    distance = _haversine(anchor[0], anchor[1], origin[0], origin[1], radius_m)
    band = cultures.ring_at(distance, radius_m)
    area.update({
        "latitude_deg": area["anchor"]["latitude_deg"],
        "longitude_deg": area["anchor"]["longitude_deg"],
        "culture": culture.name,
        "race": race,
        "purpose": culture.purpose,
        "faction": culture.faction,
        "size": culture.size,
        "level_band": [band[0], band[1]],
        "from_origin_km": round(distance / 1000.0, 1),
        "moved_m": round(_haversine(site["latitude_deg"], site["longitude_deg"],
                                    anchor[0], anchor[1], radius_m)),
    })
    area.update(counts(area, culture))
    return {"area": area, "problems": []}


def populate_world(worldfile_path, project_root, count=100, region=None, label="populate",
                   seed=20260908, on_area=None, sea_centre=(0.0, 0.0), samples=24000,
                   announce=None):
    """
    Generate `count` areas around an existing world and record the run.

    Args:
        worldfile_path (str): The world to build on. Its planet, its painted features and
            its existing areas are all read.
        project_root (str): Where `runs/` lives.
        count (int): How many areas to place.
        region (tuple, optional): `(lat_low, lat_high, lon_low, lon_high)`.
        label (str): A short name for the run.
        seed (int): The run's own generator seed, so a run can be replayed exactly.
        on_area (callable, optional): Called with each area as it lands.
        sea_centre (tuple): Where to cast shore rays from.
        samples (int): How many candidate points the survey scores.

    Returns:
        summary (dict): What was made, what was refused, and the run id.
    """
    with open(worldfile_path, encoding="utf-8") as handle:
        document = json.load(handle)

    at = planet.elevation_from_worldfile(document)
    radius_m = planet.scalars(document["planet"])["radius_m"]
    rng = random.Random(seed)
    existing = document.get("areas") or []

    run = runs.begin(project_root, label, {
        "worldfile": os.path.abspath(worldfile_path),
        "count": count, "region": list(region) if region else None,
        "seed": seed, "sea_centre": list(sea_centre), "samples": samples,
        "quotas": dict(QUOTAS),
    })
    # **The id goes out before the work starts.** A caller that spawned this needs it to
    # follow the feed, and a run announced only at the end is a run nobody could watch.
    if announce:
        announce(run.run_id)
    progress = open(run.path("progress.ndjson"), "w", encoding="utf-8", buffering=1)

    try:
        # The origin every level band is measured from: the largest place already here, or
        # the middle of the water if the world is empty.
        anchored = [a for a in existing if a.get("anchor")]
        if anchored:
            biggest = max(anchored, key=lambda a: len(a.get("rooms") or []))
            origin = (biggest["anchor"]["latitude_deg"], biggest["anchor"]["longitude_deg"])
        else:
            origin = sea_centre

        seeds = populate.seeds_for(
            at, radius_m, sea_centre,
            existing=[(a["anchor"]["latitude_deg"], a["anchor"]["longitude_deg"])
                      for a in anchored])

        # **Fresh water has to be seeded, not stumbled upon.** A site counts as having
        # fresh water only within three kilometres of it, and the growth walks in steps of
        # sixteen to forty-eight - so landing that close by chance almost never happens.
        # Measured: with a thousand river features loaded and no water seeds, every
        # halfling hamlet and every river town in a hundred-area run went unplaced, and
        # nothing in the refusals said why, because they were never candidates.
        #
        # `_water_seeds` walks the index in bucket order rather than along each course, so
        # the seeds spread over the map instead of marching down one valley.
        fresh = river_points(document) + lake_points(document)
        if fresh:
            index = siting.WaterIndex(fresh)
            for point in siting._water_seeds(index, radius_m, siting.SEPARATION_M * 6):
                if not siting.in_region(point[0], point[1], region):
                    continue
                # **A river node is IN the river.** Seeding on it and then requiring dry
                # ground threw every water seed away; seeding on it and letting the growth
                # find land put the nearest candidate eight kilometres out, which is past
                # the three-kilometre reach that makes a site count as having fresh water.
                # So the seed is stepped onto the bank: near enough to drink from, dry
                # enough to build on.
                bank = _bank_beside(at, point, radius_m)
                if bank is None:
                    continue
                seeds.append(bank)
                if len(seeds) >= 60:
                    break

        classify = cultures.classifier(cultures.DEMO_TABLE)
        # More candidates than areas, because a site can fail its gate or find every
        # culture it fits already at quota - and a run that stops at ninety because it ran
        # out of ground is worse than one that looked at half as much again.
        sites = grow_sites(at, radius_m, seeds, count * 3, region=region,
                           classify=classify, rivers=fresh)
        by_name = {c.name: c for c in cultures.DEMO_TABLE}
        quota = wanted(count)
        filled = {key: 0 for key in quota}

        made, refused, base_id = [], [], 1000
        for site in sites:
            if len(made) >= count:
                break
            # **Round robin, not table order.** Taking the first culture the ground fits
            # meant whatever sits highest in the table ate the sites: measured, volgrin and
            # lunari filled their quotas off the riverbanks and every halfling hamlet in a
            # hundred-area run went unplaced, because a riverside site fits all three and
            # halflings were listed third. Ordering the candidates by how far behind their
            # own quota they are does one pass down the whole list before anybody gets a
            # second helping, so each people is represented in proportion to what was asked
            # for rather than in proportion to where it happens to sit in the table.
            #
            # The table's order survives as the tie-break, so a good harbour still becomes
            # a city rather than a hamlet when the two are equally short.
            candidates = classify(site)
            rank = {name: index for index, name in enumerate(candidates)}

            def _share(name):
                key = _key_for(by_name[name])
                return (filled.get(key, 0) / max(1, quota.get(key, 1)), rank[name])

            for name in sorted(candidates, key=_share):
                culture = by_name[name]
                key = _key_for(culture)
                if filled.get(key, 0) >= quota.get(key, 0):
                    continue
                built = build_area(site, culture, at, radius_m, rng, base_id, origin)
                if built["area"] is None:
                    refused.append({"culture": name, "site": [site["latitude_deg"],
                                                             site["longitude_deg"]],
                                    "problems": built["problems"]})
                    continue
                area = built["area"]
                filled[key] += 1
                base_id += len(area["rooms"]) + 10
                made.append(area)
                progress.write(json.dumps(feed_line(area)) + "\n")
                if on_area:
                    on_area(area)
                break

        progress.close()
        document["areas"] = existing + made

        # **Roads before the check, because the check can only report.** Every area is
        # joined to the nearest area already on the network; see `connect_areas`.
        roads = connect_areas(document["areas"], radius_m)
        run.write_json("roads.json", roads)
        stranded = reachability.check(document)
        run.write_json("worldfile.json", document)
        run.write_json("refused.json", refused)

        summary = {
            "areas": len(made),
            "rooms": sum(a["room_count"] for a in made),
            "npcs": sum(a["npcs"] for a in made),
            "shops": sum(a["shops"] for a in made),
            "refused": len(refused),
            "roads": sum(1 for road in roads if road["laid"]),
            "long_roads": sum(1 for road in roads if road.get("long")),
            "stranded": len(stranded.get("unreachable", ())),
            "unfilled": {key: quota[key] - filled[key]
                         for key in quota if filled[key] < quota[key]},
        }
        run.finish(summary=summary)
        summary["run_id"] = run.run_id
        return summary
    except Exception as error:                    # noqa: BLE001 - recorded, then re-raised
        try:
            progress.close()
        finally:
            run.fail(str(error))
        raise


def main(argv=None):
    """Run a populate from the command line, or from the server on the viewer's behalf.

    Prints the run id as its FIRST line and flushes it, so a caller that spawned this can
    start following the progress feed while the run is still going - which is the whole
    point of streaming one line per area.
    """
    import argparse

    parser = argparse.ArgumentParser(description="populate a world with areas")
    parser.add_argument("--world", required=True, help="worldfile to build on")
    parser.add_argument("--root", default=".", help="where runs/ lives")
    parser.add_argument("--count", type=int, default=100)
    parser.add_argument("--label", default="populate")
    parser.add_argument("--seed", type=int, default=20260908)
    parser.add_argument("--region", default=None,
                        help="lat_low,lat_high,lon_low,lon_high")
    parser.add_argument("--sea", default="0,0", help="lat,lon in the middle of the water")
    args = parser.parse_args(argv)

    region = tuple(float(v) for v in args.region.split(",")) if args.region else None
    sea = tuple(float(v) for v in args.sea.split(","))

    summary = populate_world(args.world, args.root, count=args.count, region=region,
                             label=args.label, seed=args.seed, sea_centre=sea,
                             announce=lambda run_id: (
                                 print(json.dumps({"run_id": run_id}), flush=True)))
    print(json.dumps(summary), flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
