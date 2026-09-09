"""Where the great cities go, and why they are not all on the coast.

**A world of a hundred towns and no cities is a world with no centre.** Areas are sited by
what the ground is good for, one at a time, and that produces a plausible scatter of
villages and holds and nothing a region turns about. A hub is the exception: it is placed
*first*, and the rest of the world is sited afterwards around what is already there.

**On a grid, because scoring picks coasts.** Every site score in this generator rewards
water - a harbour, a river mouth, a landing - so left to its own judgement it strings every
large settlement along a shoreline and leaves the interior to hamlets. Dividing the ground
into cells and taking the best site *inside each cell* keeps the terrain's judgement about
where a city stands while taking away its judgement about how the cities are spread. The
inland cells get a city because they are cells, not because the interior scored well.

**A cell that already has a great place keeps it.** A world being added to already has
somewhere its owner built by hand; putting a generated city beside it is the one outcome
nobody wants. So an occupied cell is skipped, and the hand-built place is that region's hub.
"""

import math

#: One major city per this many areas.
HUB_EVERY = 50

#: How many street rooms a major city has.
#:
#: **Street rooms, not rooms.** Shops are interiors now and hang off the lattice, so a city
#: of this size carries another forty or fifty rooms a player can walk into on top of it.
#:
#: Above canon's ceiling on purpose: law P2 puts an ordinary area at 16-170 rooms and warns
#: that beyond it a place "wants to be two areas joined at a boundary". That is the right
#: advice for an ordinary area and the wrong advice for a capital, which is meant to be the
#: one place on the map you cannot walk across in a minute. Recorded as a departure rather
#: than an oversight.
HUB_ROOMS = 160


def _haversine(lat_a, lon_a, lat_b, lon_b, radius_m):
    phi_a, phi_b = math.radians(lat_a), math.radians(lat_b)
    d_phi = phi_b - phi_a
    d_lambda = math.radians(lon_b - lon_a)
    inner = (math.sin(d_phi / 2) ** 2
             + math.cos(phi_a) * math.cos(phi_b) * math.sin(d_lambda / 2) ** 2)
    return 2 * radius_m * math.asin(min(1.0, math.sqrt(inner)))


def how_many(count, every=HUB_EVERY):
    """
    Args:
        count (int): How many areas the run is making.
        every (int): One hub per this many areas.

    Returns:
        hubs (int): At least one, so even a small run has a centre.
    """
    return max(1, int(math.ceil(count / float(every))))


def cells(region, wanted):
    """
    The ground divided into roughly square cells, one per hub.

    Args:
        region (tuple or None): `(lat_low, lat_high, lon_low, lon_high)`, or None for the
            whole planet.
        wanted (int): How many cells.

    Returns:
        cells (list): `(lat_low, lat_high, lon_low, lon_high)` covering the ground once.

    Notes:
        **Roughly square in degrees, not in metres.** A cell near a pole is narrower on the
        ground than one at the equator, and correcting for it would crowd the poles with
        cities - which is the opposite of what the grid is for. The grid decides how many
        cities and roughly where; the terrain inside each cell decides exactly where.
    """
    lat_low, lat_high, lon_low, lon_high = region or (-60.0, 60.0, -180.0, 180.0)
    # The far edge reaches a hair past the region in every case, including this one: the
    # bounds are half-open, so without it a site exactly on the region's own boundary falls
    # in no cell at all. The single-cell shortcut forgot it and lost the corner.
    edge = 1e-9
    if wanted <= 1:
        return [(lat_low, lat_high + edge, lon_low, lon_high + edge)]
    height = max(1e-6, lat_high - lat_low)
    width = max(1e-6, lon_high - lon_low)
    # Choose the row/column split that comes nearest to square cells.
    best = None
    for rows in range(1, wanted + 1):
        columns = int(math.ceil(wanted / float(rows)))
        shape = (height / rows) / max(1e-6, width / columns)
        cost = abs(math.log(shape)) + 0.05 * (rows * columns - wanted)
        if best is None or cost < best[0]:
            best = (cost, rows, columns)
    _cost, rows, columns = best
    # **Half-open, or a site on a boundary is in two cells and becomes two cities.** Found
    # by a test with one candidate and four cells that produced two capitals from it.
    out = []
    for row in range(rows):
        for column in range(columns):
            out.append((lat_low + height * row / rows,
                        lat_low + height * (row + 1) / rows + (edge if row == rows - 1 else 0),
                        lon_low + width * column / columns,
                        lon_low + width * (column + 1) / columns
                        + (edge if column == columns - 1 else 0)))
    return out[:wanted] if len(out) > wanted else out


def _inside(cell, lat, lon):
    """Half-open: a point belongs to exactly one cell. See the note in `cells`."""
    lat_low, lat_high, lon_low, lon_high = cell
    return lat_low <= lat < lat_high and lon_low <= lon < lon_high


def occupied(cell, existing):
    """
    Args:
        cell (tuple): One grid cell.
        existing (list): Areas the world already has, each with an `anchor`.

    Returns:
        taken (bool): Whether somewhere already stands in this cell.

    Notes:
        **A hand-built place is its region's city.** Dropping a generated capital beside
        somebody's own city is the one result that makes a generated world unusable to the
        person who had a world already.
    """
    for area in existing or ():
        anchor = area.get("anchor") or {}
        lat, lon = anchor.get("latitude_deg"), anchor.get("longitude_deg")
        if lat is not None and _inside(cell, lat, lon):
            return True
    return False


def plan(sites, region, count, existing=(), every=HUB_EVERY, score=None):
    """
    Which sites become the great cities.

    Args:
        sites (list): Candidate sites, each with `latitude_deg` and `longitude_deg`.
        region (tuple or None): The ground being populated.
        count (int): How many areas the run is making.
        existing (list): Areas the world already has.
        every (int): One hub per this many areas.
        score (callable, optional): `site -> number`, higher is better. Defaults to the
            site's own score if it carries one.

    Returns:
        chosen (list): One site per free cell, in the order the cells were laid out.

    Notes:
        A cell with no candidate in it gets no city rather than a city somewhere else: the
        grid decides where cities are *allowed*, and empty ocean is allowed to have none.
    """
    def value(site):
        if score is not None:
            return score(site)
        return site.get("score", 0.0) or 0.0

    chosen, spoken = [], set()
    for cell in cells(region, how_many(count, every)):
        if occupied(cell, existing):
            continue
        inside = [site for site in sites
                  if id(site) not in spoken
                  and _inside(cell, site["latitude_deg"], site["longitude_deg"])]
        if not inside:
            continue
        best = max(inside, key=value)
        spoken.add(id(best))
        chosen.append(best)
    return chosen


def far_enough(site, chosen, radius_m, apart_m):
    """
    Args:
        site (dict): A candidate.
        chosen (list): Sites already picked.
        radius_m (float): The planet's radius.
        apart_m (float): How far two cities must stand from each other.

    Returns:
        clear (bool): Whether the candidate is far enough from every city so far.
    """
    return all(_haversine(site["latitude_deg"], site["longitude_deg"],
                          other["latitude_deg"], other["longitude_deg"], radius_m) >= apart_m
               for other in chosen)
