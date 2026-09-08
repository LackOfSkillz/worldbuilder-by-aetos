"""Who settles which ground, and why.

`siting` measures. It answers how high a place is, how far it stands above the country
round it, whether a hull can reach it and whether there is fresh water. Those are facts
about a planet and they are the same facts whoever is looking.

**This is the other half, and it is deliberately not in the library.** A dwarf hold wants
height and ore; an elf village wants forest and quiet water; a barbarian camp wants
somewhere defensible and far from anybody's law. Those are facts about a *game*, not about
a planet - worldbuilder has no races and should not grow any. So this module ships a schema
and a matcher, and the table below is an EXAMPLE, filled from the races and professions of
one game that has them.

**A settlement is a set of requirements, not a name.** The matcher never guesses: a site
either satisfies every requirement of a culture or it does not, so a hold that never gets
placed says so by leaving its quota unfilled rather than by quietly becoming something
else. That is what happened to "castle" when its test asked for a steepness this planet
does not have, and the symptom read as a preference rather than as an impossible gate.

**Level is a radius from where characters start, and the world is built that way round.**
Ground for new characters sits within reach of the starting city; each band outward is a
longer journey than the last, so a character's reach grows with them and the map rewards
going further. The bands are concentric rings, not a ranking - a site does not get a level
because it happened to be the furthest thing found, it gets one because of which ring it
falls in, and every ring is sought out rather than hoped for.

That distinction is the whole of it. Dividing by the furthest ground that turned up gave a
site thirty-three kilometres out the same band as the starting city and one nineteen
thousand kilometres away the top band, with nothing in between - a scale set by an
accident of sampling rather than by a design.
"""

import math

#: What a settlement means for the people who meet it.
FRIENDLY = "friendly"
NEUTRAL = "neutral"
HOSTILE = "hostile"


#: The rings, outward from where characters start: `(low, high, outer edge)`.
#:
#: The edge is a FRACTION of the furthest any two points on the planet can be - half the
#: circumference - so the same rings describe a moon and a gas giant without anybody
#: retyping kilometres. Geometric rather than even: each ring is a longer journey than the
#: last, which is what makes going further feel like progress rather than like more walking.
LEVEL_RINGS = (
    (1, 5, 0.02),
    (6, 10, 0.05),
    (11, 20, 0.12),
    (21, 40, 0.25),
    (41, 60, 0.50),
    (61, 100, 1.00),
)


def ring_at(distance_m, radius_m, rings=LEVEL_RINGS):
    """
    Which level band a place this far from the origin belongs to.

    Args:
        distance_m (float): Great-circle distance from where characters start.
        radius_m (float): The planet's radius.
        rings (tuple, optional): The bands.

    Returns:
        band (tuple): `(low, high)`.
    """
    antipode = math.pi * radius_m
    for low, high, edge in rings:
        if distance_m <= edge * antipode:
            return (low, high)
    return (rings[-1][0], rings[-1][1])


def ring_radii(radius_m, rings=LEVEL_RINGS):
    """The outer edge of each band in metres, for placing rather than labelling."""
    antipode = math.pi * radius_m
    return [(low, high, edge * antipode) for low, high, edge in rings]


class Culture:
    """One kind of place a game wants on its map.

    Attributes:
        name (str): What it is called, and the key a quota names.
        race (str or None): Who lives there.
        profession (str or None): Whose hall or camp it is, if any.
        faction (str): `FRIENDLY`, `NEUTRAL` or `HOSTILE`.
        purpose (str): `home`, `guild`, `hunting`, `trade`, `mine` or `military`.
        size (str): `camp`, `hamlet`, `village`, `town`, `city` or `seat`.
        wants (dict): Terrain requirements. Any of `elevation_m`, `prominence_m`,
            `slope_m` as `(low, high)` pairs, and `needs` / `forbids` as sequences of
            `harbour`, `landing` or `fresh`.
        level_band (tuple or None): For hunting grounds, filled in by distance.
    """

    def __init__(self, name, wants, race=None, profession=None, faction=FRIENDLY,
                 purpose="home", size="village", level_band=None):
        self.name = name
        self.wants = wants
        self.race = race
        self.profession = profession
        self.faction = faction
        self.purpose = purpose
        self.size = size
        self.level_band = level_band

    def fits(self, site):
        """Whether this ground satisfies every requirement. No partial credit."""
        for field in ("elevation_m", "prominence_m", "slope_m"):
            band = self.wants.get(field)
            if band is None:
                continue
            low, high = band
            value = site.get(field)
            if value is None or value < low or value > high:
                return False
        for need in self.wants.get("needs", ()):
            if site.get("%s_m" % need) is None:
                return False
        for forbid in self.wants.get("forbids", ()):
            if site.get("%s_m" % forbid) is not None:
                return False
        return True

    def as_dict(self):
        return {"culture": self.name, "race": self.race, "profession": self.profession,
                "faction": self.faction, "purpose": self.purpose, "size": self.size}


def classifier(table):
    """A `classify(site) -> [name, ...]` for `siting.survey`, from a culture table.

    Notes:
        **Every culture the ground could support, in the table's own order, not just the
        first.** The table's order is its priority - a city and a hamlet may both fit a
        good harbour, and listing the city first means good harbours become cities while
        hamlets take what is left, which is how settlement actually works.

        Returning only the winner looked equivalent and was not. A quota is filled from a
        finite set of sites, so once two felari hamlets are placed every further shoreline
        still classified as felari, was skipped for a full quota, and never fell through to
        the saurathi raider town listed below it. Eight of seventeen cultures came out
        empty and read as "the planet has no ground for them" when the ground was there and
        spoken for. The site offers everything it can be; the selection decides.
    """
    def classify(site):
        return [culture.name for culture in table if culture.fits(site)]
    return classify


def _haversine(lat1, lon1, lat2, lon2, radius_m):
    a1, o1, a2, o2 = map(math.radians, (lat1, lon1, lat2, lon2))
    h = (math.sin((a2 - a1) / 2) ** 2
         + math.cos(a1) * math.cos(a2) * math.sin((o2 - o1) / 2) ** 2)
    return 2 * math.asin(math.sqrt(h)) * radius_m


def describe(sites, table, radius_m, origin=None, bands=LEVEL_RINGS):
    """
    Attach culture, faction and level band to sited places.

    Args:
        sites (list): Chosen sites from `siting.survey`.
        table (list): The `Culture` records those sites were classified against.
        radius_m (float): The planet's radius.
        origin (dict, optional): Where characters start. Defaults to the first site.
        bands (tuple, optional): Level bands, nearest first.

    Returns:
        placed (list): Each site with its culture, and a `level_band` where the culture
        hunts rather than lives.
    """
    by_name = {culture.name: culture for culture in table}
    if origin is None and sites:
        origin = sites[0]
    placed = []
    for site in sites:
        culture = by_name.get(site.get("kind"))
        record = dict(site)
        if culture is not None:
            record.update(culture.as_dict())
            if culture.purpose == "hunting" and origin:
                gap = _haversine(origin["latitude_deg"], origin["longitude_deg"],
                                 site["latitude_deg"], site["longitude_deg"], radius_m)
                record["level_band"] = list(ring_at(gap, radius_m, bands))
                record["km_from_origin"] = round(gap / 1000)
        placed.append(record)
    return placed


# --------------------------------------------------------------------------------------
# The demo world's peoples, from the owner's own affinities.
#
# **These are answers, not guesses.** An earlier version of this table was invented from the
# race names and got at least one badly wrong - the Lunari read as moon-mages when they are
# the wolf people, which would have put forty observatories where packs should have been.
# Every line below was given rather than inferred.
#
# **What is measurable, and what is not yet.** Height, slope, prominence, coast, landing and
# fresh water are all real questions the oracle answers. Forest is not: there is no biome
# layer yet, so "prefers woods" is approximated by temperate inland ground of the height
# trees grow on, and will get sharper when climate lands. That approximation is stated here
# rather than hidden, because an elf village in a grassland is a thing somebody should be
# able to explain.
#
# The order is the priority: a site offers every culture it fits and the first with room
# takes it, so the rarer and more particular peoples are listed above the common ones.
# --------------------------------------------------------------------------------------

#: Swamp: low, flat and wet. The bayou the fish camp sits in is the type example.
SWAMP = {"elevation_m": (0.5, 25.0), "slope_m": (0.0, 4.0), "needs": ("landing",)}

DEMO_TABLE = [
    # --- particular ground, listed first so it is not taken by a generalist -----------
    Culture("saurathi marsh town", race="saurathi", size="town", purpose="home",
            wants=dict(SWAMP)),
    Culture("saurathi marsh village", race="saurathi", size="village", purpose="home",
            wants=dict(SWAMP)),
    Culture("dwarf mountain hold", race="dwarf", size="seat", purpose="home",
            wants={"elevation_m": (250.0, 650.0), "prominence_m": (25.0, 1e9)}),
    Culture("dwarf mining village", race="dwarf", size="village", purpose="mine",
            wants={"elevation_m": (150.0, 650.0)}),
    Culture("gnome workshop village", race="gnome", size="village", purpose="home",
            wants={"elevation_m": (60.0, 300.0), "prominence_m": (5.0, 1e9)}),
    Culture("volgrin steading", race="volgrin", size="town", purpose="home",
            wants={"elevation_m": (20.0, 200.0), "slope_m": (0.0, 6.0)}),
    Culture("aethari coastal city", race="aethari", size="city", purpose="trade",
            wants={"needs": ("harbour",), "elevation_m": (2.0, 80.0)}),
    Culture("aethari retreat", race="aethari", size="camp", purpose="guild",
            wants={"prominence_m": (20.0, 1e9)}),
    Culture("valran highland steading", race="valran", size="village", purpose="home",
            wants={"elevation_m": (120.0, 500.0)}),

    # --- the peoples who live where humans live, in their own places ------------------
    Culture("felari fishing village", race="felari", size="village", purpose="home",
            wants={"needs": ("landing",), "elevation_m": (1.0, 40.0)}),
    Culture("lunari pack holt", race="lunari", size="village", purpose="home",
            wants={"elevation_m": (30.0, 250.0), "forbids": ("harbour",)}),
    Culture("halfling farm hamlet", race="halfling", size="hamlet", purpose="home",
            wants={"elevation_m": (5.0, 120.0), "slope_m": (0.0, 8.0),
                   "needs": ("fresh",)}),
    Culture("elf woodland village", race="elf", size="village", purpose="home",
            wants={"elevation_m": (25.0, 250.0), "slope_m": (0.0, 12.0),
                   "forbids": ("harbour",)}),

    # --- humans everywhere, which is why they are last ---------------------------------
    Culture("human port city", race="human", size="city", purpose="trade",
            wants={"needs": ("harbour", "fresh"), "elevation_m": (2.0, 60.0)}),
    Culture("human harbour town", race="human", size="town", purpose="trade",
            wants={"needs": ("harbour",), "elevation_m": (1.0, 60.0)}),
    Culture("human river town", race="human", size="town", purpose="trade",
            wants={"needs": ("fresh",), "elevation_m": (2.0, 150.0)}),
    Culture("human village", race="human", size="village", purpose="home",
            wants={"elevation_m": (2.0, 200.0)}),

    # --- guild ground -----------------------------------------------------------------
    Culture("ranger lodge", race=None, profession="ranger", size="camp", purpose="guild",
            wants={"elevation_m": (10.0, 200.0), "forbids": ("harbour",)}),
    Culture("barbarian war camp", race=None, profession="barbarian", size="camp",
            purpose="guild", faction=NEUTRAL,
            wants={"elevation_m": (100.0, 650.0)}),

    # --- hostile ground: NPC peoples, and the wild ------------------------------------
    Culture("goblin camp", race=None, size="camp", purpose="home", faction=HOSTILE,
            wants={"elevation_m": (10.0, 400.0)}),
    Culture("marsh hunting ground", size="camp", purpose="hunting", faction=HOSTILE,
            wants=dict(SWAMP)),
    Culture("upland hunting ground", size="camp", purpose="hunting", faction=HOSTILE,
            wants={"elevation_m": (200.0, 650.0)}),
    Culture("wildwood hunting ground", size="camp", purpose="hunting", faction=HOSTILE,
            wants={"elevation_m": (5.0, 200.0), "forbids": ("harbour",)}),
    Culture("shore hunting ground", size="camp", purpose="hunting", faction=HOSTILE,
            wants={"needs": ("landing",), "elevation_m": (1.0, 20.0)}),
]
