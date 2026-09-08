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

**Hunting grounds are placed by distance, because that is what level means.** A ground for
new characters belongs within reach of where they start; one for veterans belongs where
getting there is itself the journey. So the band is assigned from how far the site is from
the world's origin rather than declared, and a world with one continent cannot accidentally
put its hardest ground next to its capital.
"""

import math

#: What a settlement means for the people who meet it.
FRIENDLY = "friendly"
NEUTRAL = "neutral"
HOSTILE = "hostile"


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


def describe(sites, table, radius_m, origin=None, bands=((1, 10), (10, 25), (25, 50),
                                                         (50, 80), (80, 100))):
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
    hunting = [s for s in sites if by_name.get(s.get("kind")) is not None
               and by_name[s["kind"]].purpose == "hunting"]
    reach = 1.0
    if hunting and origin:
        reach = max(_haversine(origin["latitude_deg"], origin["longitude_deg"],
                               s["latitude_deg"], s["longitude_deg"], radius_m)
                    for s in hunting) or 1.0

    placed = []
    for site in sites:
        culture = by_name.get(site.get("kind"))
        record = dict(site)
        if culture is not None:
            record.update(culture.as_dict())
            if culture.purpose == "hunting" and origin:
                gap = _haversine(origin["latitude_deg"], origin["longitude_deg"],
                                 site["latitude_deg"], site["longitude_deg"], radius_m)
                # **The band comes from the distance, not from the table.** A ground for
                # new characters within reach of where they start, a veterans' ground where
                # getting there is the journey - so a world cannot put its hardest ground
                # next to its capital by accident.
                index = min(len(bands) - 1, int(len(bands) * gap / (reach * 1.001)))
                record["level_band"] = list(bands[index])
                record["km_from_origin"] = round(gap / 1000)
        placed.append(record)
    return placed


# --------------------------------------------------------------------------------------
# An EXAMPLE table, and only an example.
#
# The races and professions are one game's, borrowed to demonstrate the schema on a real
# roster rather than on three invented names. Worldbuilder ships no races: a game supplies
# its own table and this one is deleted.
#
# The order is the priority. Cities before villages, so the best harbours become cities.
# --------------------------------------------------------------------------------------

DEMO_TABLE = [
    # --- seats of power: the best ground of each kind ---------------------------------
    Culture("dwarf mountain kingdom", race="dwarf", size="seat", purpose="home",
            wants={"elevation_m": (250.0, 650.0), "prominence_m": (60.0, 1e9)}),
    Culture("human port city", race="human", size="city", purpose="trade",
            wants={"needs": ("harbour", "fresh"), "elevation_m": (2.0, 60.0)}),
    Culture("valran river city", race="valran", size="city", purpose="trade",
            wants={"needs": ("fresh",), "forbids": ("harbour",),
                   "elevation_m": (2.0, 120.0)}),

    # --- working settlements ----------------------------------------------------------
    Culture("dwarf mining village", race="dwarf", size="village", purpose="mine",
            wants={"elevation_m": (150.0, 650.0), "prominence_m": (20.0, 1e9)}),
    Culture("elf woodland village", race="elf", size="village", purpose="home",
            wants={"elevation_m": (20.0, 250.0), "prominence_m": (-1e9, 20.0),
                   "forbids": ("harbour",)}),
    Culture("human harbour town", race="human", size="town", purpose="trade",
            wants={"needs": ("harbour",), "elevation_m": (1.0, 60.0)}),
    Culture("halfling farm hamlet", race="halfling", size="hamlet", purpose="home",
            wants={"elevation_m": (5.0, 120.0), "slope_m": (0.0, 8.0),
                   "forbids": ("harbour",)}),
    Culture("felari fishing hamlet", race="felari", size="hamlet", purpose="home",
            wants={"needs": ("landing",), "forbids": ("harbour",),
                   "elevation_m": (1.0, 30.0)}),
    Culture("gnome workshop village", race="gnome", size="village", purpose="home",
            wants={"elevation_m": (60.0, 300.0)}),

    # --- guild halls: a profession's own ground ---------------------------------------
    Culture("ranger lodge", race=None, profession="ranger", size="camp", purpose="guild",
            wants={"elevation_m": (10.0, 200.0), "forbids": ("harbour",)}),
    Culture("barbarian war camp", race=None, profession="barbarian", size="camp",
            purpose="guild", faction=NEUTRAL,
            wants={"elevation_m": (100.0, 650.0), "prominence_m": (10.0, 1e9)}),
    Culture("moon mage observatory", race="lunari", profession="moon_mage", size="camp",
            purpose="guild", wants={"prominence_m": (30.0, 1e9)}),

    # --- hostile ground ---------------------------------------------------------------
    Culture("saurathi raider town", race="saurathi", size="town", purpose="home",
            faction=HOSTILE, wants={"needs": ("landing",), "elevation_m": (1.0, 40.0)}),
    Culture("volgrin hold", race="volgrin", size="town", purpose="military",
            faction=HOSTILE,
            wants={"elevation_m": (200.0, 650.0), "prominence_m": (40.0, 1e9)}),

    # --- hunting grounds: banded by distance, not by declaration -----------------------
    Culture("wildwood hunting ground", size="camp", purpose="hunting", faction=HOSTILE,
            wants={"elevation_m": (5.0, 200.0), "forbids": ("harbour",)}),
    Culture("upland hunting ground", size="camp", purpose="hunting", faction=HOSTILE,
            wants={"elevation_m": (200.0, 650.0)}),
    Culture("shore hunting ground", size="camp", purpose="hunting", faction=HOSTILE,
            wants={"needs": ("landing",), "elevation_m": (1.0, 20.0)}),
]
