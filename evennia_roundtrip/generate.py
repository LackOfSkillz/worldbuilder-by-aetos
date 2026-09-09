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

from . import (areagen, cultures, ferries, hubs, naming, people, period, place,
               planet, populate, reachability, runs, siting, stock)

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
    ("hunting", 11),
    ("game", 4),
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

#: The trade rooms, by the name `naming` gives them, so shops can be counted rather than
#: guessed. A shop is a room; counting them means counting rooms.
TRADE_MARKERS = ("inn", "weaponsmith", "armourer", "general store", "alchemist",
                 "counting house", "healer", "stables", "shrine", "market stalls")


#: Which vocabulary a hunting ground speaks in, by the country it stands in.
#:
#: **A ram is not going to love the swamp and an alligator is not going to love a
#: mountain.** One "wild" voice and one "game" voice put grouse butts in a fen and sliding
#: reptiles on a crag, and no word-count band or graph measurement can see it - the prose is
#: the right length and completely wrong. So the animals follow the ground the culture asked
#: for, which the culture table already states.
HUNTING_VOICE = {
    "woodland game covert": "game_wood",
    "river fowl marsh": "game_marsh",
    "upland game moor": "game_moor",
    "marsh hunting ground": "wild_marsh",
    "upland hunting ground": "wild_upland",
    "wildwood hunting ground": "wild",
    "shore hunting ground": "wild_shore",
}


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
        # Game country and hostile country do not read alike, and neither does a fen read
        # like a crag. See `HUNTING_VOICE`.
        return HUNTING_VOICE.get(culture.name,
                                 "wild" if culture.faction == cultures.HOSTILE else "game")
    if "goblin" in culture.name:
        return "goblin"
    return "human"


def people_of(culture):
    """
    Whose place this is, for the map key and the palette.

    **Not the same as the voice it is written in.** `voice_race` answers which vocabulary
    the prose uses and now returns things like `wild_upland` and `game_wood`, because a fen
    and a crag do not read alike. Storing that in `race` put `game_moor` in the legend
    beside `dwarf` and left the palette without a colour for it. The voice is a fact about
    the writing; this is a fact about who lives there.
    """
    if culture.race:
        return culture.race
    if culture.purpose == "hunting":
        return "wild" if culture.faction == cultures.HOSTILE else "game"
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
        # **Game country and hostile country hold separate quotas.** Sharing one meant
        # whichever sat higher in the table took the lot: adding three game cultures turned
        # all thirteen hunting grounds neutral overnight, which is the same fault as having
        # them all hostile, wearing the other coat. Eleven dangerous, four for the pot.
        return "hunting" if culture.faction == cultures.HOSTILE else "game"
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


#: How far apart two settlements stand, and how far a new one may be founded from an old.
#:
#: **A hundred and twelve miles apart, reaching two hundred and fifty for the next.**
#: The floor is what the spacing actually settles at - measured, the median nearest
#: neighbour lands within a few miles of it, because once the map is full every site has
#: somebody at arm's length. So the floor is set for the median that was wanted, not at the
#: bottom of the range: a floor of a hundred gave a median of a hundred and three, and a
#: floor of a hundred and seventeen gave a hundred and twenty-five.
#:
#: A hundred to two hundred miles is several days' travel, not a morning's walk.
#: The first version used `siting`'s own ten and thirty, and a hundred areas at that spacing
#: filled one corner of the region and read as a jumble - impressive for the first dozen
#: pins and then a smear. These are the numbers for a world somebody travels across.
#:
#: `siting.SEPARATION_M` and `LAND_LINK_M` keep their own values: they answer a different
#: question - whether a site is REACHABLE from its neighbours - and shortening a journey is
#: not the same as deciding two places are one.
NEAR_M = 180_246.0
FAR_M = 402_336.0


def _inland(site):
    """Whether a site has no sea within reach - which is what makes it inland."""
    return site.get("harbour_m") is None and site.get("landing_m") is None


def _nearest_gap(site, chosen, radius_m):
    """How far a candidate stands from the nearest settlement already placed."""
    best = None
    for other in chosen:
        gap = _haversine(site["latitude_deg"], site["longitude_deg"],
                         other["latitude_deg"], other["longitude_deg"], radius_m)
        if best is None or gap < best:
            best = gap
    return best if best is not None else 0.0


#: How many coastal settlements are founded before the frontier is pushed inland.
#:
#: **A ring round the water first, then away from it.** People settle the shore and then
#: move up the rivers, so the first dozen places belong on the coast - and after that a
#: generator that keeps taking the highest-scoring site keeps taking the coast, because the
#: scorer pays forty points for a harbour. Past this count an inland candidate is preferred
#: outright, and only if there is no inland candidate does the shore get another one.
COASTAL_FIRST = 10


def grow_sites(at, radius_m, seeds, count, region=None, near_m=NEAR_M,
               far_m=FAR_M, classify=None, rivers=(), coastal_first=COASTAL_FIRST):
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
        found = siting.local_sites(at, radius_m, near, far_m, rivers=rivers,
                                   classify=classify)
        # **Furthest from anything already built, not highest scoring.** `local_sites`
        # answers best-first, and the scorer pays forty points for a harbour and ten for a
        # landing - so taking its favourite every time walks the coastline and never turns
        # inland, which is exactly what a hundred pins strung along one shore looked like.
        # Ordering by how far a candidate stands from the nearest existing settlement pushes
        # the frontier outward instead, and score decides between equals.
        # Seeds are facts about the world, not settlements this run founded, so the coastal
        # allowance counts what was actually placed.
        founded = sum(1 for site in chosen if not site.get("seeded"))
        inland_now = founded >= coastal_first
        found.sort(key=lambda site: (
            # Inland first once the shore has had its share. `harbour_m` and `landing_m`
            # are None exactly when no water is in reach, which is what inland means here.
            (0 if _inland(site) else 1) if inland_now else 0,
            -_nearest_gap(site, chosen, radius_m),
            -site["score"]))
        for candidate in found:
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


#: How finely a proposed road is sounded before it is believed to be dry.
#:
#: Five kilometres. A strait narrower than that is a ford or a bridge, not a crossing a road
#: has to go round.
ROUTE_STEP_M = 5000.0

#: How far off the straight line a road may be bent to keep out of the water, as a fraction
#: of the distance being spanned, and how many bends are tried.
DETOUR_REACH = (0.25, 0.5, 0.8, 1.2)


def _dry_between(at, a, b, radius_m, step_m=ROUTE_STEP_M):
    """Whether every point on the great circle from `a` to `b` stands above the water."""
    span = _haversine(a[0], a[1], b[0], b[1], radius_m)
    steps = max(1, int(span / step_m))
    for index in range(steps + 1):
        point = _point_between(a, b, index / steps, radius_m)
        if at(point[0], point[1]) <= 0.0:
            return False
    return True


#: How steep a road may climb before it stops being a road, as metres risen per metre run.
#:
#: Twelve per cent. A cart road tops out around ten; a pack trail will take fifteen. Above
#: this the pathfinder goes round, which is what a road actually does - the pass, not the
#: peak.
MAX_GRADE = 0.12

#: What a climb costs, against the flat distance. Ten means a hundred metres of ascent is
#: worth a kilometre of level going, which is roughly how a road behaves: it will happily
#: run a long way sideways to avoid a hill.
CLIMB_COST = 10.0

#: The grid the search runs on. Five kilometres, and a box a third larger than the gap being
#: spanned so there is room to go round something.
GRID_M = 5000.0
GRID_MARGIN = 0.35

#: The most nodes one search may open. A road that needs more than this is a road round an
#: ocean, and the answer there is a boat.
GRID_BUDGET = 20000


def route_between(at, a, b, radius_m, passable, step_cost, grid_m=GRID_M,
                  budget=GRID_BUDGET):
    """
    A way from `a` to `b` across whatever surface `passable` accepts.

    Args:
        passable (callable): `(height_m) -> bool`. Land for a road, water for a boat.
        step_cost (callable): `(here_m, there_m, run_m) -> cost or None`. None refuses the
            step, which is how a grade limit is expressed.

    Returns:
        route (list): `[(lat, lon), ...]` from `a` to `b`, or None if there is no way -
            which for a road is the answer that means "this one needs a boat".

    Notes:
        **A road across the sea is not a road, and eighteen per cent of them were.** The
        spanning tree joins each area to the nearest one already on the network, and nearest
        was measured through the ground rather than over it - so a settlement across a bay
        was joined by a road running along the sea floor, in one case two thousand eight
        hundred metres below the water.

        **And a road over a mountain is not one either.** Water is impassable and a slope
        steeper than `MAX_GRADE` is too, so the search finds the pass rather than the peak;
        climbing is merely expensive, which is what makes it prefer the long way round a
        hill and take the short way over a rise.

        A* on a five-kilometre grid, eight-connected, with great-circle distance as the
        heuristic. The grid is a box around the two ends with a third again of margin, and
        the search is budgeted: something that needs more than twenty thousand nodes is
        going round an ocean.
    """
    import heapq

    metres_per_degree = math.pi * radius_m / 180.0
    lat0 = (a[0] + b[0]) / 2.0
    coslat = max(0.15, math.cos(math.radians(lat0)))
    d_lat = grid_m / metres_per_degree
    d_lon = grid_m / (metres_per_degree * coslat)

    span = _haversine(a[0], a[1], b[0], b[1], radius_m)
    margin = span * GRID_MARGIN
    lo_lat = min(a[0], b[0]) - margin / metres_per_degree
    hi_lat = max(a[0], b[0]) + margin / metres_per_degree
    lo_lon = min(a[1], b[1]) - margin / (metres_per_degree * coslat)
    hi_lon = max(a[1], b[1]) + margin / (metres_per_degree * coslat)

    def cell(point):
        return (int(round((point[0] - lo_lat) / d_lat)),
                int(round((point[1] - lo_lon) / d_lon)))

    def where(node):
        return (lo_lat + node[0] * d_lat, lo_lon + node[1] * d_lon)

    rows = int((hi_lat - lo_lat) / d_lat) + 1
    columns = int((hi_lon - lo_lon) / d_lon) + 1
    if rows * columns > budget:
        # **Coarsen rather than give up.** A fixed five-kilometre grid over a two-thousand
        # kilometre span is seventy thousand cells, and the guard turned that into an
        # instant None - which the caller read as "no way over land" and answered with a
        # straight road across the sea. A long road is allowed to be surveyed coarsely; it
        # is not allowed to be surveyed not at all.
        # The grid is coarsened until the whole box fits inside the node budget, so the
        # search can actually cross it rather than run out of nodes halfway. A four-
        # thousand-kilometre crossing ends up surveyed at about forty kilometres, which is
        # coarse for a road and exactly right for deciding whether one is possible at all.
        coarser = grid_m * math.sqrt(rows * columns / float(budget)) * 1.1
        if coarser > grid_m * 40.0:
            return None
        return route_between(at, a, b, radius_m, passable, step_cost,
                             grid_m=coarser, budget=budget)

    start, goal = cell(a), cell(b)
    goal_at = where(goal)

    heights = {}
    def height(node):
        if node not in heights:
            point = where(node)
            heights[node] = at(point[0], point[1])
        return heights[node]

    def guess(node):
        point = where(node)
        return _haversine(point[0], point[1], goal_at[0], goal_at[1], radius_m)

    open_set = [(guess(start), 0.0, start)]
    came, best = {}, {start: 0.0}
    opened = 0
    while open_set:
        _, cost, node = heapq.heappop(open_set)
        if cost > best.get(node, float("inf")):
            continue
        if node == goal:
            route = [where(node)]
            while node in came:
                node = came[node]
                route.append(where(node))
            route.reverse()
            # The real ends, not the grid cells nearest to them.
            route[0], route[-1] = a, b
            return route
        opened += 1
        if opened > budget:
            return None
        here = height(node)
        for dr in (-1, 0, 1):
            for dc in (-1, 0, 1):
                if dr == 0 and dc == 0:
                    continue
                neighbour = (node[0] + dr, node[1] + dc)
                if not (0 <= neighbour[0] < rows and 0 <= neighbour[1] < columns):
                    continue
                there = height(neighbour)
                if not passable(there):
                    continue
                run = grid_m * (1.414 if dr and dc else 1.0)
                step = step_cost(here, there, run)
                if step is None:
                    continue
                through = cost + step
                if through < best.get(neighbour, float("inf")):
                    best[neighbour] = through
                    came[neighbour] = node
                    heapq.heappush(open_set, (through + guess(neighbour), through, neighbour))
    return None


#: How deep the water must be for a hull to pass. See `place.DEFAULT_PORT_DEPTH_M` for the
#: same idea at a quay; out in the fairway the requirement is only that it is not a beach.
SEA_DEPTH_M = -3.0


def overland_route(at, a, b, radius_m, **kwargs):
    """A way over land that never enters the water and never climbs what it can go round."""
    def passable(height):
        return height > 0.0

    def step_cost(here, there, run):
        climb = there - here
        if abs(climb) / run > MAX_GRADE:
            return None
        return run + CLIMB_COST * max(0.0, climb)

    return route_between(at, a, b, radius_m, passable, step_cost, **kwargs)


def sea_route(at, a, b, radius_m, **kwargs):
    """
    A way over the water that never crosses land.

    Notes:
        **Passengers will hate the portage.** A ferry line drawn straight from one ramp to
        the other looks fine on a globe and runs over whatever headland lies between - so
        the same search runs again with the surfaces swapped: water is passable, land is
        not, and there is nothing to climb. What comes out is a line a hull could actually
        follow, which is also the only honest thing to draw.
    """
    def passable(height):
        return height < SEA_DEPTH_M

    def step_cost(_here, _there, run):
        return run

    return route_between(at, a, b, radius_m, passable, step_cost, **kwargs)


def _along(latitude_deg, longitude_deg, bearing_deg, distance_m, radius_m):
    """Walk `distance_m` from a point along a bearing."""
    lat, lon = math.radians(latitude_deg), math.radians(longitude_deg)
    brg = math.radians(bearing_deg)
    d = distance_m / radius_m
    lat2 = math.asin(math.sin(lat) * math.cos(d)
                     + math.cos(lat) * math.sin(d) * math.cos(brg))
    lon2 = lon + math.atan2(math.sin(brg) * math.sin(d) * math.cos(lat),
                            math.cos(d) - math.sin(lat) * math.sin(lat2))
    return (math.degrees(lat2), (math.degrees(lon2) + 540) % 360 - 180)


def shoreline_toward(at, start, toward, radius_m, step_m=2000.0, reach_m=60000.0):
    """
    The last dry ground between `start` and the water in the direction of `toward`.

    This is where a boat ramp goes: on land, at the edge of the water somebody would launch
    into. Returns `start` unchanged when the walk never reaches water.
    """
    heading = _bearing(start[0], start[1], toward[0], toward[1])
    last_dry = start
    steps = max(1, int(reach_m / step_m))
    for index in range(1, steps + 1):
        point = _along(start[0], start[1], heading, step_m * index, radius_m)
        if at(point[0], point[1]) <= 0.0:
            return last_dry
        last_dry = point
    return last_dry


def add_boat_ramp(area, toward, at, radius_m, rng, room_id):
    """
    Give an area a ramp at its own waterside, joined to the room nearest the water.

    Returns:
        ramp (dict): The room that was added.

    Notes:
        **A ramp is a room, like a shop is a room.** It is somewhere a player stands to
        board, which is what makes a sea crossing something they do rather than something
        that happens to them - and it is what the maritime side already expects to find.
    """
    rooms = area.get("rooms") or []
    here = _where(area)
    anchor_room = min(rooms, key=lambda room: _haversine(
        room["latitude_deg"], room["longitude_deg"], toward[0], toward[1], radius_m))
    edge = shoreline_toward(at, (anchor_room["latitude_deg"], anchor_room["longitude_deg"]),
                            toward, radius_m)
    ramp = {
        "id": room_id,
        "key": "a boat ramp",
        "latitude_deg": round(edge[0], 6),
        "longitude_deg": round(edge[1], 6),
        "cell": [0, 0, 0],
        "elevation_m": round(at(edge[0], edge[1]), 3),
        "ramp": True,
    }
    ramp["desc"] = naming.describe([], "human", rng)
    rooms.append(ramp)
    heading = _bearing(anchor_room["latitude_deg"], anchor_room["longitude_deg"],
                       edge[0], edge[1])
    direction = COMPASS[int((heading + 22.5) % 360 // 45)]
    area.setdefault("exits", []).append({
        "source": anchor_room["id"], "name": _free_name(anchor_room, area, direction),
        "destination": ramp["id"], "ramp": True})
    area["exits"].append({
        "source": ramp["id"], "name": OPPOSITE[direction],
        "destination": anchor_room["id"], "ramp": True})
    return ramp


#: How far a road may run between two areas before it stops being a walk.
#:
#: Three hundred and forty kilometres - a little past the far end of the settlement spacing,
#: so an ordinary link to a neighbour is never flagged and only a genuine outlier is. Beyond
#: it the link is a voyage rather than a road, which the manifest says rather than hides.
ROAD_REACH_M = 340_000.0


def _is_wild(area):
    """Whether an area is somewhere people go out to rather than travel between."""
    return area.get("purpose") == "hunting" or area.get("faction") == cultures.HOSTILE


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


#: How far apart the rooms of a road stand.
#:
#: **Five miles, because a road is a place you travel, not a door you step through.** An
#: exit joining two towns a hundred miles apart teleports somebody across a hundred miles;
#: twenty rooms at five-mile intervals is a journey with somewhere to be ambushed, somewhere
#: to camp, and somewhere to meet a caravan. It is also what makes the distance between two
#: settlements mean anything at all in play.
ROAD_ROOM_M = 8046.7

#: The most rooms one road may spend.
#:
#: A road is capped rather than a distance refused: the spanning tree occasionally has to
#: reach a long way to join an outlying place, and three thousand kilometres at five-mile
#: spacing is four hundred rooms nobody will ever walk. Past the cap the rooms simply stand
#: further apart, and the manifest says which roads those are.
ROAD_ROOM_CAP = 40


def _point_between(a, b, fraction, radius_m):
    """A point along the great circle from `a` to `b`."""
    lat1, lon1 = math.radians(a[0]), math.radians(a[1])
    lat2, lon2 = math.radians(b[0]), math.radians(b[1])
    d = _haversine(a[0], a[1], b[0], b[1], radius_m) / radius_m
    if d == 0.0:
        return (a[0], a[1])
    sin_d = math.sin(d)
    p = math.sin((1 - fraction) * d) / sin_d
    q = math.sin(fraction * d) / sin_d
    x = p * math.cos(lat1) * math.cos(lon1) + q * math.cos(lat2) * math.cos(lon2)
    y = p * math.cos(lat1) * math.sin(lon1) + q * math.cos(lat2) * math.sin(lon2)
    z = p * math.sin(lat1) + q * math.sin(lat2)
    return (math.degrees(math.atan2(z, math.hypot(x, y))), math.degrees(math.atan2(y, x)))


def _line_length(line, radius_m):
    """How long a polyline is, in metres."""
    return sum(_haversine(line[i][0], line[i][1], line[i + 1][0], line[i + 1][1], radius_m)
               for i in range(len(line) - 1))


def _point_on_line(line, fraction, radius_m):
    """The point a given fraction of the way along a polyline."""
    total = _line_length(line, radius_m)
    if total <= 0.0:
        return line[0]
    target = total * fraction
    walked = 0.0
    for index in range(len(line) - 1):
        a, b = line[index], line[index + 1]
        leg = _haversine(a[0], a[1], b[0], b[1], radius_m)
        if leg <= 0.0:
            continue
        if walked + leg >= target:
            return _point_between(a, b, (target - walked) / leg, radius_m)
        walked += leg
    return line[-1]


def road_between(from_area, to_area, room_a, room_b, gap_m, radius_m, rng, base_id,
                 at=None, route=None):
    """
    The road itself: an area of its own, with a room every five miles.

    Args:
        from_area, to_area (dict): The two places being joined.
        room_a, room_b (dict): The rooms at each end that the road meets.
        gap_m (float): How far apart those rooms are.
        radius_m (float): The planet's radius.
        rng (random.Random): The world's own generator.
        base_id (int): The first free room id.
        at (callable, optional): The oracle, so a road room knows its own height.
        route (list, optional): `[(lat, lon), ...]` the way round the obstacles,
            from `overland_route`. Without it the rooms fall on the straight line.

    Returns:
        road (dict or None): An area with `purpose` "road", or None if the two rooms are
            close enough to join directly.

    Notes:
        **A road is an area because it is a place.** Putting its rooms inside one of the
        settlements it joins would tack twenty rooms onto a village that does not contain
        them and would break that area's own shape measurements; giving the road its own
        record keeps every area honest about what it is. It carries `purpose` "road" so a
        tally can count settlements without counting the ways between them.
    """
    walked = _line_length(list(route), radius_m) if route else gap_m
    rooms_wanted = int(walked // ROAD_ROOM_M)
    if rooms_wanted < 1:
        return None
    rooms_wanted = min(rooms_wanted, ROAD_ROOM_CAP)

    ends = ((room_a["latitude_deg"], room_a["longitude_deg"]),
            (room_b["latitude_deg"], room_b["longitude_deg"]))
    # **The rooms follow the route, not the crow.** A straight line between two rooms is
    # what put roads on the sea floor and over four-thousand-metre peaks; the route handed
    # in has already gone round both. Without one the line is used, which is right for the
    # short links where a search would find nothing to avoid.
    line = list(route) if route else [ends[0], ends[1]]
    rooms = []
    for index in range(rooms_wanted):
        fraction = (index + 1) / (rooms_wanted + 1)
        latitude, longitude = _point_on_line(line, fraction, radius_m)
        rooms.append({
            "id": base_id + index,
            "latitude_deg": round(latitude, 6),
            "longitude_deg": round(longitude, 6),
            "cell": [index, 0, 0],
            "key": "the road",
            "elevation_m": round(at(latitude, longitude), 3) if at else None,
        })

    # **Checked at room resolution, not at grid resolution.** The route is searched on a
    # grid that coarsens for long spans, so a strait narrower than one cell reads as land
    # and the rooms interpolated between two dry cells land in the water. Forty road rooms
    # were under the sea this way. A road with a wet room is refused here and the caller
    # asks for a boat instead.
    if at is not None:
        for room in rooms:
            if at(room["latitude_deg"], room["longitude_deg"]) <= 0.0:
                return None

    name = "the road from %s to %s" % (from_area.get("display_name") or from_area["name"],
                                       to_area.get("display_name") or to_area["name"])
    road = {
        "name": name.lower().replace(" ", "-"),
        "display_name": name,
        "rooms": rooms,
        "exits": [],
        "purpose": "road",
        "race": "road",
        "faction": cultures.NEUTRAL,
        "size": "road",
        "latitude_deg": rooms[len(rooms) // 2]["latitude_deg"],
        "longitude_deg": rooms[len(rooms) // 2]["longitude_deg"],
        "anchor": {"latitude_deg": rooms[0]["latitude_deg"],
                   "longitude_deg": rooms[0]["longitude_deg"],
                   "bearing_deg": 0.0, "spacing_m": ROAD_ROOM_M},
        "spacing_m": round(gap_m / (rooms_wanted + 1)),
    }
    # The road's own rooms run one after another; the ends are joined to the settlements by
    # the caller, which is what makes the whole thing one walkable chain.
    for index in range(len(rooms) - 1):
        here, there = rooms[index], rooms[index + 1]
        heading = _bearing(here["latitude_deg"], here["longitude_deg"],
                           there["latitude_deg"], there["longitude_deg"])
        direction = COMPASS[int((heading + 22.5) % 360 // 45)]
        road["exits"].append({"source": here["id"], "name": direction,
                              "destination": there["id"], "road": True})
        road["exits"].append({"source": there["id"], "name": OPPOSITE[direction],
                              "destination": here["id"], "road": True})
    # **Not described here.** The prose names every way out of a room, and the road's two
    # END rooms are joined to the settlements by the caller - after this returns. Describing
    # it now produced a first and last room each claiming one fewer way out than it has, on
    # top of the earlier fault where the whole road claimed to be a dead end. The caller
    # calls `finish_road` once every exit is in place.
    return road


def finish_road(road, rng, from_name=None, to_name=None):
    """
    Name and describe a road, once every one of its exits exists.

    Args:
        road (dict): The road area.
        rng (random.Random): The world's own generator.
        from_name (str, optional): The place at the near end, as a player would call it.
        to_name (str, optional): The place at the far end.

    Notes:
        **The end rooms name the places they join, which is law B2.** "A boundary room names
        its neighbour area in the exit or the description. A player should know they are
        leaving." Roads were the seam between every pair of areas in the world and said
        nothing at either end, so walking out of a town and into the next was a change of
        area a player could only detect by the scenery changing.

        Said in the description rather than the exit, because the exit is a compass point
        and law L4 says a compass exit is generated from the coordinates, never typed.
    """
    naming.name_and_describe(road, "road", rng, settled=False)
    road.setdefault("size", "road")
    people.populate(road, "road", rng)

    rooms = road.get("rooms") or []
    if rooms and (from_name or to_name):
        ends = ((rooms[0], from_name, to_name), (rooms[-1], to_name, from_name))
        for room, here, there in ends:
            if not (here or there):
                continue
            said = []
            if here:
                said.append("%s lies back the way you came" % here)
            if there and there != here:
                said.append("the road runs on to %s" % there)
            room["desc"] = "%s %s." % (room.get("desc", "").rstrip(),
                                       "; ".join(said).capitalize())
    return road


def ferry_between(from_area, to_area, room_a, room_b, at, radius_m, rng, base_id):
    """
    A boat crossing: a ramp on each shore, and the water track between them.

    Returns:
        crossing (dict or None): A record with `from`, `to`, the two ramp rooms and the
            `track` the hull follows, or None when even the water has no way through.

    Notes:
        **A ferry is not a road drawn in another colour.** It needs somewhere to board at
        each end, which is a room; it needs a line a hull could actually follow, which is
        the sea route rather than the straight one - passengers hate the portage; and it
        needs to be visible to the reachability check as a sea connection, or an island
        reads as stranded while the boat is sitting there.
    """
    from_point = (room_a["latitude_deg"], room_a["longitude_deg"])
    to_point = (room_b["latitude_deg"], room_b["longitude_deg"])

    # **The crossing is found before anything is built.** Adding the ramps first and then
    # discovering there is no water route left two areas each carrying a boat ramp to
    # nowhere - a room a player can walk to, stand on, and never leave by.
    start = _water_off(at, from_point, to_point, radius_m)
    end = _water_off(at, to_point, from_point, radius_m)
    if start is None or end is None:
        return None
    track = sea_route(at, start, end, radius_m)
    if track is None:
        return None

    near_ramp = add_boat_ramp(from_area, start, at, radius_m, rng, base_id)
    far_ramp = add_boat_ramp(to_area, end, at, radius_m, rng, base_id + 1)
    return {
        "from": from_area["name"], "to": to_area["name"],
        "from_room": near_ramp["id"], "to_room": far_ramp["id"],
        "from_ramp": [near_ramp["latitude_deg"], near_ramp["longitude_deg"]],
        "to_ramp": [far_ramp["latitude_deg"], far_ramp["longitude_deg"]],
        "track": [[round(p[0], 6), round(p[1], 6)] for p in track],
        "metres": round(_line_length(track, radius_m)),
    }


def _water_off(at, start, toward, radius_m, step_m=2000.0, reach_m=150000.0):
    """
    The nearest navigable water out from a place, preferring the far shore's direction.

    Notes:
        **A fan, not a line.** Walking straight at the far shore found nothing whenever the
        coast ran the other way - a town a hundred metres up with the sea round a headland
        answered "no water" and the crossing was refused. The bearings are tried nearest the
        target first, so the answer is still the sensible side of the town when there is
        one.
    """
    straight = _bearing(start[0], start[1], toward[0], toward[1])
    steps = max(1, int(reach_m / step_m))
    for spread in (0, 20, 40, 60, 90, 120, 150, 180):
        for side in ((0,) if spread == 0 else (1, -1)):
            heading = straight + side * spread
            for index in range(1, steps + 1):
                point = _along(start[0], start[1], heading, step_m * index, radius_m)
                if at(point[0], point[1]) < SEA_DEPTH_M:
                    return point
    return None


#: How near two road rooms must be to count as the same crossing.
#:
#: Four kilometres - comfortably under the five-mile room spacing, so two ways that merely
#: run beside each other are not welded together, and two that actually cross are.
CROSSING_M = 4000.0


def _segments_cross(a1, a2, b1, b2, lat0):
    """
    Where two short segments cross, in degrees, or None.

    Flat geometry on a local scale factor: over the few kilometres a road segment spans,
    the curvature of the planet is far smaller than the four-kilometre tolerance this is
    deciding, so the standard planar test is exact enough and enormously cheaper than a
    spherical one.
    """
    k = max(0.15, math.cos(math.radians(lat0)))
    ax, ay = a1[1] * k, a1[0]
    bx, by = a2[1] * k, a2[0]
    cx, cy = b1[1] * k, b1[0]
    dx, dy = b2[1] * k, b2[0]
    r = (bx - ax, by - ay)
    sdir = (dx - cx, dy - cy)
    denominator = r[0] * sdir[1] - r[1] * sdir[0]
    if abs(denominator) < 1e-12:
        return None
    t = ((cx - ax) * sdir[1] - (cy - ay) * sdir[0]) / denominator
    u = ((cx - ax) * r[1] - (cy - ay) * r[0]) / denominator
    if not (0.0 <= t <= 1.0 and 0.0 <= u <= 1.0):
        return None
    return (ay + t * r[1], (ax + t * r[0]) / k)


def join_crossings(roads, radius_m, rng, base_id, at=None):
    """
    Put a room where two ways cross, and join it to both.

    Args:
        roads (list): The road and path areas, each with `rooms` and `exits`.
        radius_m (float): The planet's radius.
        rng (random.Random): The world's own generator.
        base_id (int): The first free room id.
        at (callable, optional): The oracle, for the crossing's own height.

    Returns:
        crossings (list): One record per junction made.

    Notes:
        **Two roads that cross and do not meet are two roads a player cannot change
        between.** Drawn on a globe it looks like a junction; walked, it is a flyover with
        no slip road - you can see the other way and you cannot take it. Every crossing is
        given a room of its own, joined to the nearest room on each way.

        **Segment intersection, not proximity.** The first version asked whether two rooms
        on different ways stood within four kilometres of each other, and found nothing: the
        rooms are five miles apart, so two ways can cross cleanly with the nearest room on
        each a full four kilometres from the crossing and from one another. Where the lines
        actually cross is a question with an exact answer, so it is asked exactly.
    """
    made = []
    next_id = base_id
    for index, road in enumerate(roads):
        here_rooms = road.get("rooms") or []
        if len(here_rooms) < 2:
            continue
        for other in roads[index + 1:]:
            there_rooms = other.get("rooms") or []
            if len(there_rooms) < 2:
                continue
            hit = None
            for i in range(len(here_rooms) - 1):
                a1 = (here_rooms[i]["latitude_deg"], here_rooms[i]["longitude_deg"])
                a2 = (here_rooms[i + 1]["latitude_deg"], here_rooms[i + 1]["longitude_deg"])
                for j in range(len(there_rooms) - 1):
                    b1 = (there_rooms[j]["latitude_deg"], there_rooms[j]["longitude_deg"])
                    b2 = (there_rooms[j + 1]["latitude_deg"],
                          there_rooms[j + 1]["longitude_deg"])
                    point = _segments_cross(a1, a2, b1, b2, a1[0])
                    if point is not None:
                        hit = (point, here_rooms[i], there_rooms[j])
                        break
                if hit:
                    break
            if not hit:
                continue
            point, here, there = hit
            junction = {
                "id": next_id,
                "key": "a crossroads",
                "latitude_deg": round(point[0], 6),
                "longitude_deg": round(point[1], 6),
                "cell": [0, 0, 0],
                "elevation_m": round(at(point[0], point[1]), 3) if at else None,
                "crossing": True,
            }
            next_id += 1
            junction["desc"] = naming.describe([], "road", rng)
            road.setdefault("rooms", []).append(junction)
            for host, room in ((road, here), (other, there)):
                heading = _bearing(junction["latitude_deg"], junction["longitude_deg"],
                                   room["latitude_deg"], room["longitude_deg"])
                direction = COMPASS[int((heading + 22.5) % 360 // 45)]
                road.setdefault("exits", []).append({
                    "source": junction["id"], "name": direction,
                    "destination": room["id"], "road": True})
                host.setdefault("exits", []).append({
                    "source": room["id"],
                    "name": _free_name(room, host, OPPOSITE[direction]),
                    "destination": junction["id"], "road": True})
            made.append({"ways": [road.get("display_name"), other.get("display_name")],
                         "room": junction["id"],
                         "at": [junction["latitude_deg"], junction["longitude_deg"]]})
    return made


def connect_areas(areas, radius_m, rng, base_id, at=None, reach_m=ROAD_REACH_M):
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
    # **The trunk is settlements; the wild hangs off it.** A hunting ground is not a place
    # people travel between, it is somewhere they go OUT to - so joining it into the
    # spanning tree makes it a link in the chain between two towns, and a road through a
    # goblin camp is not a road anybody uses. These are set aside and attached afterwards
    # by a side path to the nearest road.
    aside = [area for area in placed if _is_wild(area)]
    placed = [area for area in placed if not _is_wild(area)]
    if len(placed) < 2:
        placed = placed + aside
        aside = []
    if len(placed) < 2:
        return [], []
    joined = [placed[0]]
    outside = list(placed[1:])
    roads = []
    built_roads = []
    ferries = []
    next_id = base_id
    #: Pairs already tried and found impossible, so the search does not retry them forever.
    refused_pairs = set()
    while outside:
        # **The nearest pair that can actually be joined, not simply the nearest pair.**
        # A link over open sea can be neither a road nor a ferry, and the first version
        # still marked that area joined - so its whole subtree hung off nothing and
        # thirty-nine areas came out unreachable while the manifest said two links were
        # missing. Taking the next-best partner instead is what a spanning tree is for.
        candidates = []
        for area in outside:
            here = _where(area)
            for other in joined:
                if (id(area), id(other)) in refused_pairs:
                    continue
                there = _where(other)
                candidates.append((_haversine(here[0], here[1], there[0], there[1],
                                              radius_m), area, other))
        if not candidates:
            for area in outside:
                roads.append({"from": "(nothing reachable)", "to": area["name"],
                              "metres": 0, "laid": False,
                              "why": "no partner could be joined by road or by boat"})
            break
        candidates.sort(key=lambda row: row[0])
        gap, area, other = candidates[0]
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

        # **Overland if there is an overland way, and a boat if there is not.** The route
        # is searched before anything is built, because whether these two places are joined
        # by a road or by a ferry is a fact about the ground between them, not a style.
        route = overland_route(at, (room_a["latitude_deg"], room_a["longitude_deg"]),
                               (room_b["latitude_deg"], room_b["longitude_deg"]),
                               radius_m) if at else None
        if at and route is None:
            crossing = ferry_between(other, area, room_a, room_b, at, radius_m, rng, next_id)
            if crossing:
                next_id += 10
                ferries.append(crossing)
                roads.append({"from": other["name"], "to": area["name"],
                              "metres": crossing["metres"], "laid": True, "sea": True,
                              "rooms": 0, "long": False})
                outside.remove(area)
                joined.append(area)
                continue
            # **No land route and no water route means no link, and that is the answer.**
            # Falling through to the straight line is what put roads on the sea floor: six
            # of thirty of them, one two thousand eight hundred metres under. An unjoined
            # pair is a fact the manifest can carry and somebody can act on; a road across
            # a sea is a lie the map tells every time it is looked at.
            refused_pairs.add((id(area), id(other)))
            continue
        road = road_between(other, area, room_a, room_b, room_gap, radius_m, rng, next_id,
                            at=at, route=route)
        if road is None and route is not None and room_gap > ROAD_ROOM_M:
            # The route looked dry on the grid and was not once the rooms were placed on it.
            crossing = ferry_between(other, area, room_a, room_b, at, radius_m, rng, next_id)
            if crossing:
                next_id += 10
                ferries.append(crossing)
                roads.append({"from": other["name"], "to": area["name"],
                              "metres": crossing["metres"], "laid": True, "sea": True,
                              "rooms": 0, "long": False})
                outside.remove(area)
                joined.append(area)
                continue
            refused_pairs.add((id(area), id(other)))
            continue
        if road is None:
            out_name = _free_name(room_a, other, direction)
            back_name = _free_name(room_b, area, OPPOSITE[direction])
            other["exits"].append({"source": room_a["id"], "name": out_name,
                                   "destination": room_b["id"], "road": True})
            area["exits"].append({"source": room_b["id"], "name": back_name,
                                  "destination": room_a["id"], "road": True})
            rooms_on_it = 0
        else:
            next_id += len(road["rooms"]) + 10
            built_roads.append(road)
            first, last = road["rooms"][0], road["rooms"][-1]
            other["exits"].append({"source": room_a["id"],
                                   "name": _free_name(room_a, other, direction),
                                   "destination": first["id"], "road": True})
            road["exits"].append({"source": first["id"], "name": OPPOSITE[direction],
                                  "destination": room_a["id"], "road": True})
            back = COMPASS[int((_bearing(last["latitude_deg"], last["longitude_deg"],
                                         room_b["latitude_deg"],
                                         room_b["longitude_deg"]) + 22.5) % 360 // 45)]
            road["exits"].append({"source": last["id"], "name": back,
                                  "destination": room_b["id"], "road": True})
            area["exits"].append({"source": room_b["id"],
                                  "name": _free_name(room_b, area, OPPOSITE[back]),
                                  "destination": last["id"], "road": True})
            finish_road(road, rng,
                        from_name=other.get("display_name") or other.get("name"),
                        to_name=area.get("display_name") or area.get("name"))
            rooms_on_it = len(road["rooms"])
        roads.append({"from": other["name"], "to": area["name"],
                      "metres": round(room_gap), "laid": True, "direction": direction,
                      "rooms": rooms_on_it, "long": room_gap > reach_m})
        outside.remove(area)
        joined.append(area)
    # Every wild place now gets its own path to the nearest road room, or to the nearest
    # settlement if the roads are all too far. A path is a road by another name - same
    # five-mile rooms, different word - so it is built by the same function.
    for area in aside:
        here = _where(area)
        best = None
        for road in built_roads:
            for room in road["rooms"]:
                gap = _haversine(here[0], here[1], room["latitude_deg"],
                                 room["longitude_deg"], radius_m)
                if best is None or gap < best[0]:
                    best = (gap, road, room)
        for other in joined:
            found = _nearest_rooms(other, area, radius_m)
            if found and (best is None or found[0] < best[0]):
                best = (found[0], other, found[1])
        if best is None:
            continue
        gap, host, host_room = best
        found = _nearest_rooms(area, {"rooms": [host_room]}, radius_m)
        near_room = found[1] if found else area["rooms"][0]
        area.setdefault("exits", [])
        host.setdefault("exits", [])
        heading = _bearing(host_room["latitude_deg"], host_room["longitude_deg"],
                           near_room["latitude_deg"], near_room["longitude_deg"])
        direction = COMPASS[int((heading + 22.5) % 360 // 45)]
        trail = road_between(host, area, host_room, near_room, gap, radius_m, rng, next_id,
                             at=at)
        if trail is None:
            host["exits"].append({"source": host_room["id"],
                                  "name": _free_name(host_room, host, direction),
                                  "destination": near_room["id"], "road": True})
            area["exits"].append({"source": near_room["id"],
                                  "name": _free_name(near_room, area, OPPOSITE[direction]),
                                  "destination": host_room["id"], "road": True})
            roads.append({"from": host["name"], "to": area["name"], "metres": round(gap),
                          "laid": True, "rooms": 0, "path": True, "long": False})
            continue
        trail["display_name"] = "the path to %s" % (area.get("display_name")
                                                    or area["name"])
        trail["name"] = trail["display_name"].lower().replace(" ", "-")
        trail["purpose"] = "path"
        next_id += len(trail["rooms"]) + 10
        built_roads.append(trail)
        first, last = trail["rooms"][0], trail["rooms"][-1]
        host["exits"].append({"source": host_room["id"],
                              "name": _free_name(host_room, host, direction),
                              "destination": first["id"], "road": True})
        trail["exits"].append({"source": first["id"], "name": OPPOSITE[direction],
                               "destination": host_room["id"], "road": True})
        back = COMPASS[int((_bearing(last["latitude_deg"], last["longitude_deg"],
                                     near_room["latitude_deg"],
                                     near_room["longitude_deg"]) + 22.5) % 360 // 45)]
        trail["exits"].append({"source": last["id"], "name": back,
                               "destination": near_room["id"], "road": True})
        area["exits"].append({"source": near_room["id"],
                              "name": _free_name(near_room, area, OPPOSITE[back]),
                              "destination": last["id"], "road": True})
        display = trail["display_name"]
        finish_road(trail, rng,
                    from_name=host.get("display_name") or host.get("name"),
                    to_name=area.get("display_name") or area.get("name"))
        trail["display_name"] = display
        roads.append({"from": host["name"], "to": area["name"], "metres": round(gap),
                      "laid": True, "rooms": len(trail["rooms"]), "path": True,
                      "long": gap > reach_m})

    return roads, built_roads, ferries


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
    items = sum(len(room.get("stock") or ()) for room in rooms)
    docks = sum(1 for room in rooms if place.water_room(room["key"]))
    return {"room_count": len(rooms), "shops": shops, "docks": docks, "items": items,
            "npcs": people.count(area)}


#: What the progress feed carries: enough to draw a pin and fill a hover card, and none of
#: the room bodies. A hundred areas of sixty rooms is six thousand descriptions, and the
#: viewer polls this file every second.
FEED_FIELDS = ("name", "display_name", "latitude_deg", "longitude_deg", "race", "purpose",
               "faction", "culture", "size", "level_band", "from_origin_km", "npcs",
               "shops", "docks", "items")


def feed_line(area):
    """One area as the viewer wants it: counts, not contents."""
    line = {key: area[key] for key in FEED_FIELDS if key in area}
    line["rooms"] = area["room_count"]
    return line


def build_area(site, culture, at, radius_m, rng, base_id, origin, taken=None, size=None):
    """
    One finished, named, gated area - or None with the reasons it was refused.

    Returns:
        result (dict): `area` and `problems`.
    """
    kind = culture.size
    # A hub is given its size rather than taking the one its culture usually builds: the
    # site was chosen to be a city before anybody asked what kind of city it would be.
    size = size or areagen.TYPE_SIZE.get(kind, 47)
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
    voice = voice_race(culture)
    # `taken` is the world's register of place names, so no two towns share one. See
    # `naming.place_name`.
    naming.name_and_describe(area, voice, rng,
                             settled=culture.purpose not in ("hunting",), taken=taken)
    # **Stocked here, so the count is of what is actually on the shelves.** A shop with no
    # wares is a room with a sign on it, and "eight shops" in a tally means nothing until
    # there is something in them to buy.
    for room in area["rooms"]:
        wares = stock.stock_for(room["key"], culture.size, rng)
        if wares:
            room["stock"] = wares
    # **And peopled here, for the same reason.** The tally reported `rooms x density` and
    # the worldfile held nobody, so every population figure this generator has ever printed
    # described people who did not exist. See `people.populate`.
    people.populate(area, voice, rng)
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
        "race": people_of(culture),
        "voice": voice,
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

    def stage(named, note=""):
        """
        Say which part of the run is happening now.

        **A feed that only carries areas goes silent for the longest part of the work.** The
        pins stop at the requested count and then nothing happens for minutes while the
        roads are laid, the crossings joined and a six-megabyte worldfile written - which
        looks exactly like a generator that has hung. Stages travel down the same feed the
        areas do, so a watcher needs nothing new to read them.
        """
        progress.write(json.dumps({"stage": named, "note": note}) + chr(10))

    stage("choosing sites", "scoring the ground for every culture")

    try:
        # The origin every level band is measured from: the largest place already here, or
        # the middle of the water if the world is empty.
        # **Every place name used in this world, so no two share one.** Seeded with what is
        # already here, because a generated town called The Landing is worse than two
        # generated towns called Farcamp.
        named = {str(a.get("display_name") or a.get("name") or "") for a in existing}
        stage("building areas", "%d wanted" % count)
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
            for point in siting._water_seeds(index, radius_m, NEAR_M):
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
                # **Checked against the seeds already collected, not only against each
                # other.** `_water_seeds` thins the river nodes among themselves and knows
                # nothing about the shore seeds or the areas the world already has - so a
                # riverside seed landed three hundred and seventy metres from the Landing
                # and the run founded two towns on top of each other.
                if any(_haversine(bank[0], bank[1], seed[0], seed[1], radius_m) < NEAR_M
                       for seed in seeds):
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

        # **Room ids must start above every id already in the file.** The generator counted
        # from a thousand and the world it was adding to used 1023 to 1863, so generated
        # rooms were handed ids that existing rooms already had. Nothing complains: exits
        # still point at "a room", the reachability check attributes seams to whichever area
        # it found first, and two of the four original areas were reported unreachable while
        # sitting on a road. An import would have been worse - one id, two rooms.
        used = [room["id"] for area in existing for room in (area.get("rooms") or [])
                if isinstance(room.get("id"), int)]
        made, refused = [], []
        base_id = max(used) + 1000 if used else 1000

        # **The great cities are placed first, on a grid, and the rest of the world is
        # sited around them.** Every site score here rewards water, so left to itself the
        # generator strings every large settlement along a coast; a cell decides that there
        # IS a city and the ground inside it decides where. A cell that already holds one of
        # the world's own places is skipped - that place is its region's city.
        capitals = hubs.plan(sites, region, count, existing=existing)
        stage("founding cities", "%d on a grid of %d"
              % (len(capitals), hubs.how_many(count)))
        for site in capitals:
            if len(made) >= count:
                break
            candidates = classify(site)
            if not candidates:
                continue
            # The biggest thing the ground will carry, since this is going to be a city
            # whatever the table would have made of the site on its own.
            culture = by_name[max(candidates,
                                  key=lambda name: areagen.TYPE_SIZE.get(
                                      by_name[name].size, 0))]
            built = build_area(site, culture, at, radius_m, rng, base_id, origin,
                               taken=named, size=hubs.HUB_ROOMS)
            if built["area"] is None:
                refused.append({"culture": culture.name,
                                "site": [site["latitude_deg"], site["longitude_deg"]],
                                "problems": built["problems"]})
                continue
            area = built["area"]
            area["hub"] = True
            key = _key_for(culture)
            filled[key] = filled.get(key, 0) + 1
            base_id += len(area["rooms"]) + 10
            made.append(area)
            sites = [one for one in sites if one is not site]
            progress.write(json.dumps(feed_line(area)) + chr(10))
            if on_area:
                on_area(area)
        stage("building areas", "%d wanted, %d cities founded" % (count, len(made)))

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
                built = build_area(site, culture, at, radius_m, rng, base_id, origin,
                                   taken=named)
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

        # **A second pass, because the number asked for is the number wanted.** The first
        # pass respects the quotas, and quotas are a shape rather than a target: when the
        # ground runs out of swamp there are no more saurathi towns, and the run used to
        # simply stop short - a hundred asked for and eighty-six delivered, with the
        # shortfall explained but not made up. Somebody who types a hundred and twenty-three
        # wants a hundred and twenty-three.
        #
        # So whatever is still missing is filled from the sites that are left with whichever
        # culture actually fits them, quota ignored. The manifest records how many were
        # placed this way, because a world whose last twenty areas are all human villages is
        # a fact worth being able to see rather than one to discover by reading it.
        over_quota = 0
        if len(made) < count:
            for site in sites:
                if len(made) >= count:
                    break
                if any(_haversine(site["latitude_deg"], site["longitude_deg"],
                                  area["latitude_deg"], area["longitude_deg"],
                                  radius_m) < NEAR_M for area in made):
                    continue
                for name in classify(site):
                    culture = by_name[name]
                    built = build_area(site, culture, at, radius_m, rng, base_id, origin,
                                   taken=named)
                    if built["area"] is None:
                        continue
                    area = built["area"]
                    filled[_key_for(culture)] = filled.get(_key_for(culture), 0) + 1
                    over_quota += 1
                    base_id += len(area["rooms"]) + 10
                    made.append(area)
                    progress.write(json.dumps(feed_line(area)) + "\n")
                    if on_area:
                        on_area(area)
                    break

        # The feed stays open through the rest of the run: everything after this point is
        # work a watcher used to sit through with no news at all.
        document["areas"] = existing + made
        stage("laying roads", "%d areas to join" % len(document["areas"]))

        # **Roads before the check, because the check can only report.** Every area is
        # joined to the nearest area already on the network; see `connect_areas`.
        roads, road_areas, ferries = connect_areas(document["areas"], radius_m, rng,
                                                   base_id + 10000, at=at)
        # Ferries are neither areas nor roads: they are the water between two ramps.
        document["ferries"] = ferries
        # Every ramp is a dock as far as reachability is concerned, which is what
        # stops an island reading as stranded when a boat serves it.
        maritime = document.setdefault("maritime", {})
        docks = maritime.setdefault("docks", [])
        for crossing in ferries:
            docks.append({"area": crossing["from"], "room": crossing["from_room"],
                          "ferry": crossing["to"]})
            docks.append({"area": crossing["to"], "room": crossing["to_room"],
                          "ferry": crossing["from"]})
        # **Roads go in their own list, not among the areas.** Asking for a hundred areas
        # should give a hundred places, not a hundred places plus the eighty-six ways
        # between them - a road connects areas, it is not one. Everything that walks rooms
        # reads `reachability.places`, which sees both.
        document["roads"] = list(document.get("roads") or ()) + road_areas
        stage("joining crossings", "%d roads laid" % len(road_areas))
        # Where two ways cross, they now meet. See `join_crossings`.
        crossings = join_crossings(document["roads"], radius_m, rng, base_id + 90000, at=at)
        run.write_json("crossings.json", crossings)
        run.write_json("roads.json", roads)
        stage("checking every place can be reached")
        stranded = reachability.check(document)
        stage("writing the world", "%d rooms" % sum(
            len(place.get("rooms") or ()) for place in
            list(document["areas"]) + list(document.get("roads") or ())))
        run.write_json("worldfile.json", document)
        run.write_json("refused.json", refused)

        summary = {
            "areas": len(made),
            "rooms": sum(a["room_count"] for a in made),
            "npcs": sum(a["npcs"] for a in made),
            "shops": sum(a["shops"] for a in made),
            "items": sum(a.get("items", 0) for a in made),
            "refused": len(refused),
            "over_quota": over_quota,
            "short_of": max(0, count - len(made)),
            "roads": sum(1 for road in roads if road["laid"]),
            "road_rooms": sum(len(road["rooms"]) for road in road_areas),
            "paths": sum(1 for road in roads if road.get("path")),
            "ferries": len(ferries),
            "crossings": len(crossings),
            "unjoined": sum(1 for road in roads if not road["laid"]),
            "long_roads": sum(1 for road in roads if road.get("long")),
            "stranded": len(stranded.get("unreachable", ())),
            "unfilled": {key: quota[key] - filled[key]
                         for key in quota if filled[key] < quota[key]},
        }
        run.finish(summary=summary)
        stage("complete", "%d areas, %d rooms" % (summary["areas"], summary["rooms"]))
        progress.close()
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
