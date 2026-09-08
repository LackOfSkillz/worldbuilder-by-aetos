"""Where a settlement would go, scored from the ground itself.

Placing three areas by hand was a matter of the owner naming a coordinate. Placing five
hundred is not, and the difference is not effort - it is that nobody can hold five hundred
coastlines in their head, so the reasons have to be written down and applied evenly.

**Every score here is a measurement, not a taste.** A site is good because there is water a
hull can reach, or fresh water at hand, or ground that does not fall away under a street -
and each of those is a number the oracle can answer. Nothing scores well for being
picturesque, because nothing here can see.

**The cheap gate runs first, and that is the whole performance story.** Forty thousand
candidate points at twenty samples each is eight hundred thousand elevation calls; the same
forty thousand gated on one call and then scored properly is a few tens of thousands. So a
point is asked "are you dry, and are you in the band a town could stand on?" before it is
asked anything that costs a ring search.

**A site that cannot be reached is not a site.** The owner's rule, applied at the moment of
choosing rather than checked afterwards: every settlement must have water a vessel can get
to, or a neighbour close enough to walk to. A scorer that produced beautiful unreachable
towns would be producing work for somebody to throw away.
"""

import math

#: The band of ground a settlement will stand on. Below this it floods; above it, the
#: reasons people build somewhere stop being agricultural and start being military, which
#: is what `CASTLE_M` is for.
LOW_M = 1.0
HIGH_M = 400.0

#: Ground high enough that the reason to build is a view of everything below it.
CASTLE_M = 250.0

#: What a hull needs under it to call a place a harbour, and how far it may lie offshore.
HARBOUR_DEPTH_M = 6.096
HARBOUR_REACH_M = 3000.0

#: What a small boat needs, and how far somebody will carry one.
LANDING_DEPTH_M = 0.3
LANDING_REACH_M = 800.0

#: How far apart two settlements must be. Nearer than this they are one settlement with a
#: gap in the middle, which is a thing to author deliberately rather than to generate.
SEPARATION_M = 25000.0

#: How near two settlements have to be before a road between them is credible. A site with
#: no water access must have a neighbour within this, or it is not reachable and not a site.
LAND_LINK_M = 40000.0

#: How far out the slope is measured. Two hundred metres is a few streets: the question is
#: whether a town can stand here, not whether the region is mountainous.
SLOPE_REACH_M = 200.0

#: How far the cheap coast gate looks. Generous on purpose: it decides only whether a point
#: is worth the two hundred and sixty calls a full score costs, and a gate that is too tight
#: rejects a real harbour, which is a fault the survey can never recover from.
COAST_GATE_M = 8000.0


def _sea_nearby(at, latitude_deg, longitude_deg, radius_m, reach_m, rays=4):
    """Is there any water within reach? Four calls, used as a screen and nothing else."""
    for step in (0.5, 1.0):
        for lat, lon in _ring(latitude_deg, longitude_deg, reach_m * step, radius_m, rays):
            if at(lat, lon) < 0.0:
                return True
    return False


def _offset(latitude_deg, longitude_deg, east_m, north_m, radius_m):
    metres_per_degree = math.pi * radius_m / 180.0
    return (latitude_deg + north_m / metres_per_degree,
            longitude_deg + east_m / (metres_per_degree
                                      * math.cos(math.radians(latitude_deg))))


def _ring(latitude_deg, longitude_deg, distance_m, radius_m, rays):
    for index in range(rays):
        bearing = math.radians(360.0 * index / rays)
        yield _offset(latitude_deg, longitude_deg,
                      distance_m * math.sin(bearing), distance_m * math.cos(bearing),
                      radius_m)


def slope_at(at, latitude_deg, longitude_deg, radius_m, reach_m=SLOPE_REACH_M, rays=8):
    """
    How steeply the ground falls away, as metres of drop over the reach.

    Notes:
        The worst ray, not the average. A town on a shelf with one cliff on its north side
        is a town with a cliff, and averaging that away is how a scorer recommends
        somewhere nobody could build.
    """
    here = at(latitude_deg, longitude_deg)
    return max(abs(at(lat, lon) - here)
               for lat, lon in _ring(latitude_deg, longitude_deg, reach_m, radius_m, rays))


class WaterIndex:
    """Fresh-water points, bucketed by degree, so a site asks about its own neighbourhood.

    Notes:
        **A linear scan over every river node is the same mistake as a linear scan over
        every feature, one layer up.** The planet went from one authored river to seven
        thousand course nodes; checking each against each of twenty-six thousand candidate
        sites is a hundred and eighty million distance calculations to answer a question
        whose reach is three kilometres. Buckets of one degree, and only the nine around a
        site are opened.

        One degree is far larger than the reach on purpose. It is a screen, not the answer:
        the distances inside a bucket are still measured properly, and a bucket too small
        would drop a river running just over its edge.
    """

    def __init__(self, points):
        self.buckets = {}
        for latitude, longitude in points:
            key = (int(math.floor(latitude)), int(math.floor(longitude)))
            self.buckets.setdefault(key, []).append((latitude, longitude))
        self.count = sum(len(v) for v in self.buckets.values())

    def nearest(self, latitude_deg, longitude_deg, radius_m, within_m):
        """Distance to the nearest fresh water, or None past `within_m`."""
        best = None
        base = (int(math.floor(latitude_deg)), int(math.floor(longitude_deg)))
        for dlat in (-1, 0, 1):
            for dlon in (-1, 0, 1):
                for point in self.buckets.get((base[0] + dlat, base[1] + dlon), ()):
                    gap = _haversine(latitude_deg, longitude_deg, point[0], point[1],
                                     radius_m)
                    if gap <= within_m and (best is None or gap < best):
                        best = gap
        return best


def prominence_at(at, latitude_deg, longitude_deg, radius_m, reach_m=5000.0, rays=8):
    """
    How far this stands above the country around it, in metres.

    Notes:
        Measured against the MEAN of a ring five kilometres out, not the minimum. A minimum
        is one gully away from calling every valley shoulder a mountain; the mean is what
        somebody standing there would call the surrounding country.
    """
    here = at(latitude_deg, longitude_deg)
    ring = [at(lat, lon)
            for lat, lon in _ring(latitude_deg, longitude_deg, reach_m, radius_m, rays)]
    return here - (sum(ring) / len(ring))


def water_within(at, latitude_deg, longitude_deg, radius_m, depth_m, reach_m,
                 rays=16, steps=8):
    """
    Distance to the nearest water this deep, or None.

    Notes:
        A ring search, and a ring search can miss - at sixteen rays and three kilometres
        the gap between rays at full reach is over a kilometre, so this finds harbours and
        would miss a creek. That is the documented behaviour of `place.sea_reach` and the
        same trade is taken here for the same reason: it is a screen, and the dock finder
        measures properly once a site is chosen.
    """
    for step in range(1, steps + 1):
        distance = reach_m * step / steps
        for lat, lon in _ring(latitude_deg, longitude_deg, distance, radius_m, rays):
            if -at(lat, lon) >= depth_m:
                return distance
    return None


def sunflower(count):
    """`count` points spread evenly over a sphere, as `(latitude_deg, longitude_deg)`.

    Notes:
        **The stride is coprime with the count, and it has to be.** The first version of
        this in `place` walked a plain Fibonacci spiral in index order and its first four
        anchors all landed between 76 and 78 degrees north - not because the sphere was
        badly covered, but because consecutive indices are neighbours. Taking every
        `stride`-th point visits the same set in an order that jumps.
    """
    golden = math.pi * (3.0 - math.sqrt(5.0))
    stride = max(1, int(count * 0.618))
    while math.gcd(stride, count) != 1:
        stride += 1
    for step in range(count):
        index = (step * stride) % count
        z = 1.0 - (2.0 * index + 1.0) / count
        yield (math.degrees(math.asin(z)), math.degrees((index * golden) % (2 * math.pi)))


def kind_of(site):
    """What sort of place the ground says this is.

    Notes:
        Named from the numbers rather than chosen: a harbour with a river is a port town, a
        river without a harbour is a river town, high ground with a long view is a castle.
        The archetype decides what gets built there, so this is the seam between siting and
        authoring - and keeping it a function of the measurements means a site cannot be
        labelled one thing and scored as another.
    """
    if site["elevation_m"] >= CASTLE_M and site["slope_m"] >= 40.0:
        return "castle"
    if site["harbour_m"] is not None and site["fresh_m"] is not None:
        return "port town"
    if site["harbour_m"] is not None:
        return "harbour village"
    if site["fresh_m"] is not None:
        return "river town"
    if site["landing_m"] is not None:
        return "fishing hamlet"
    return "waystation"


def score_point(at, latitude_deg, longitude_deg, radius_m, rivers=(),
                look_for_water=True, classify=None):
    """
    Everything measurable about one candidate, and what it comes to.

    Args:
        at (callable): `(lat, lon) -> metres`, from `planet.elevation_at`.
        latitude_deg (float): The candidate.
        longitude_deg (float): The candidate.
        radius_m (float): The planet's radius.
        rivers (iterable, optional): `(lat, lon)` of authored river features, for fresh
            water. Empty is honest rather than wrong: a world with no authored rivers has
            no fresh water this can find, and every site scores as if inland water did not
            exist - which it does not, yet.

    Returns:
        site (dict or None): The measurements and a `score`, or None if the ground is
        simply not somewhere anybody would build.
    """
    height = at(latitude_deg, longitude_deg)
    if height < LOW_M or height > HIGH_M + CASTLE_M:
        return None

    slope = slope_at(at, latitude_deg, longitude_deg, radius_m)
    # **Prominence, not steepness, is what high ground is for.** The castle test asked for
    # forty metres of fall over two hundred, and nothing on this gentle planet came within
    # a seventh of it - so "castle" was a branch that could never be taken, and it read as
    # a scoring preference rather than as a threshold borrowed from a steeper world. How
    # far this stands above the country around it is the question a fortress actually asks,
    # and it is meaningful on flat ground and on sharp ground alike.
    prominence = prominence_at(at, latitude_deg, longitude_deg, radius_m)
    # **The two water searches are the whole cost of a score**: sixteen rays by eight steps,
    # twice, is two hundred and fifty-six calls against the eight a slope takes. Somewhere
    # the cheap gate has already said there is no water within eight kilometres, both are
    # certain to return None, and running them anyway is the entire budget spent proving it.
    # That is what made exempting high ground from the gate a thirty-fold slowdown rather
    # than a small one.
    harbour = landing = None
    if look_for_water:
        harbour = water_within(at, latitude_deg, longitude_deg, radius_m,
                               HARBOUR_DEPTH_M, HARBOUR_REACH_M)
        landing = water_within(at, latitude_deg, longitude_deg, radius_m,
                               LANDING_DEPTH_M, LANDING_REACH_M)
    if isinstance(rivers, WaterIndex):
        fresh = rivers.nearest(latitude_deg, longitude_deg, radius_m, HARBOUR_REACH_M)
    else:
        fresh = None
        for river_lat, river_lon in rivers:
            gap = _haversine(latitude_deg, longitude_deg, river_lat, river_lon, radius_m)
            if fresh is None or gap < fresh:
                fresh = gap
        if fresh is not None and fresh > HARBOUR_REACH_M:
            fresh = None

    site = {"latitude_deg": round(latitude_deg, 6),
            "longitude_deg": round(longitude_deg, 6),
            "elevation_m": round(height, 2), "slope_m": round(slope, 2),
            "prominence_m": round(prominence, 2),
            "harbour_m": harbour, "landing_m": landing,
            "fresh_m": None if fresh is None else round(fresh)}

    # Reasons, each with what it is worth, so a site can say why it was chosen.
    reasons = []
    total = 0.0
    if harbour is not None:
        worth = 40.0 * (1.0 - harbour / HARBOUR_REACH_M)
        reasons.append(("a hull can come alongside", round(worth, 1)))
        total += worth
    if landing is not None:
        worth = 10.0 * (1.0 - landing / LANDING_REACH_M)
        reasons.append(("a boat can be pulled ashore", round(worth, 1)))
        total += worth
    if fresh is not None:
        worth = 25.0 * (1.0 - fresh / HARBOUR_REACH_M)
        reasons.append(("fresh water at hand", round(worth, 1)))
        total += worth
    # Flat ground is worth a lot to a town and nothing to a castle, which wants the drop.
    if height >= CASTLE_M:
        worth = min(25.0, slope / 4.0)
        reasons.append(("high ground with a long view", round(worth, 1)))
    else:
        worth = 25.0 * max(0.0, 1.0 - slope / 60.0)
        reasons.append(("ground a street can run on", round(worth, 1)))
    total += worth
    # Standing clear of the water, without being up a mountain.
    worth = 10.0 * max(0.0, 1.0 - abs(height - 25.0) / 200.0)
    reasons.append(("stands clear of the water", round(worth, 1)))
    total += worth

    site["reasons"] = reasons
    site["score"] = round(total, 1)
    if classify is None:
        site["kind"], site["kinds"] = kind_of(site), None
    else:
        # Everything this ground could be, in the table's priority order. The selection
        # picks from it, so a full quota lets a site fall through instead of starving what
        # is listed below it.
        site["kinds"] = classify(site)
        site["kind"] = site["kinds"][0] if site["kinds"] else None
    return site


def _haversine(lat1, lon1, lat2, lon2, radius_m):
    a1, o1, a2, o2 = map(math.radians, (lat1, lon1, lat2, lon2))
    h = (math.sin((a2 - a1) / 2) ** 2
         + math.cos(a1) * math.cos(a2) * math.sin((o2 - o1) / 2) ** 2)
    return 2 * math.asin(math.sqrt(h)) * radius_m


def local_sites(at, radius_m, near, within_m, rivers=(), rings=6, rays=12,
                classify=None):
    """
    Candidates found by looking AROUND a place, not by filtering a global grid.

    Args:
        at (callable): The elevation oracle.
        radius_m (float): The planet's radius.
        near (dict): A chosen site to grow from.
        within_m (float): How far out to look.
        rivers (iterable, optional): Authored river points.
        rings (int, optional): Distance bands.
        rays (int, optional): Bearings per band.

    Returns:
        sites (list): Scored candidates, best first.

    Notes:
        **A global sample set cannot answer a local question, and this is the second time
        that has cost a day.** `place._local_anchors` exists because filtering forty
        thousand globally-spread points to those within three hundred kilometres of
        somewhere left about four candidates. Here it was worse and quieter: at a hundred
        and twenty thousand samples the points stand a hundred and thirty-five kilometres
        apart, `LAND_LINK_M` is forty, so no two candidates in the entire set could ever be
        neighbours - and the survey returned zero castles and zero waystations every time,
        which reads as a scoring preference rather than as a grid that cannot express the
        question.
    """
    found = []
    for ring in range(1, rings + 1):
        distance = within_m * ring / rings
        for lat, lon in _ring(near["latitude_deg"], near["longitude_deg"],
                              distance, radius_m, rays):
            height = at(lat, lon)
            if height < LOW_M or height > HIGH_M + CASTLE_M:
                continue
            site = score_point(at, lat, lon, radius_m, rivers=rivers, classify=classify,
                               look_for_water=_sea_nearby(at, lat, lon, radius_m,
                                                          COAST_GATE_M))
            if site is not None and site["kind"] is not None:
                found.append(site)
    found.sort(key=lambda s: -s["score"])
    return found


def _water_seeds(index, radius_m, separation_m):
    """Points along the fresh water, thinned so two seeds are never the same settlement.

    Notes:
        Walked in bucket order rather than course order, so seeds are spread over the
        planet instead of marching down one river bank and filling every quota from a
        single valley.
    """
    taken = []
    for key in sorted(index.buckets):
        for point in index.buckets[key]:
            if any(_haversine(point[0], point[1], other[0], other[1], radius_m)
                   < separation_m for other in taken):
                continue
            taken.append(point)
            yield point


def sites_at_range(at, radius_m, origin, distance_m, bearings=72, spread=0.15,
                   rivers=(), classify=None):
    """
    Candidates lying about `distance_m` from a point, all the way round.

    Args:
        at (callable): The elevation oracle.
        radius_m (float): The planet's radius.
        origin (tuple): `(lat, lon)` the ring is measured from.
        distance_m (float): How far out the ring lies.
        bearings (int, optional): How many directions are tried.
        spread (float, optional): How much nearer and further to also look, as a fraction
            of the distance, so a ring that lands entirely in the sea still finds ground.
        rivers (iterable, optional): Authored river points.
        classify (callable, optional): Culture matcher.

    Returns:
        sites (list): Scored candidates on that ring, best first.

    Notes:
        **A ring is walked on the sphere, not on the tangent plane.** At a third of the way
        round a planet the flat-earth offset used elsewhere in this module is nonsense - it
        is fine for a settlement's own streets and wrong for a journey - so this steps along
        a great circle from the origin on each bearing, which is exact at every distance.
    """
    lat0, lon0 = math.radians(origin[0]), math.radians(origin[1])
    found = []
    for near in (1.0 - spread, 1.0, 1.0 + spread):
        angular = (distance_m * near) / radius_m
        for index in range(bearings):
            bearing = 2.0 * math.pi * index / bearings
            lat = math.asin(math.sin(lat0) * math.cos(angular)
                            + math.cos(lat0) * math.sin(angular) * math.cos(bearing))
            lon = lon0 + math.atan2(
                math.sin(bearing) * math.sin(angular) * math.cos(lat0),
                math.cos(angular) - math.sin(lat0) * math.sin(lat))
            latitude, longitude = math.degrees(lat), math.degrees(lon)
            height = at(latitude, longitude)
            if height < LOW_M or height > HIGH_M + CASTLE_M:
                continue
            site = score_point(at, latitude, longitude, radius_m, rivers=rivers,
                               classify=classify,
                               look_for_water=_sea_nearby(at, latitude, longitude,
                                                          radius_m, COAST_GATE_M))
            if site is not None and site["kind"] is not None:
                found.append(site)
    found.sort(key=lambda s: -s["score"])
    return found


def survey(at, radius_m, count=12, samples=20000, rivers=(),
           separation_m=SEPARATION_M, land_link_m=LAND_LINK_M,
           coast_reach_m=COAST_GATE_M, quotas=None, classify=None):
    """
    The best places on a planet to put a settlement.

    Args:
        at (callable): The elevation oracle.
        radius_m (float): The planet's radius.
        count (int, optional): How many sites are wanted.
        samples (int, optional): Candidates examined.
        rivers (iterable, optional): `(lat, lon)` of authored river features.
        separation_m (float, optional): How far apart two settlements must stand.
        land_link_m (float, optional): How near a neighbour must be for a site with no
            water access to count as reachable.

    Returns:
        report (dict): `sites` (chosen, best first), `examined`, `on_land`, and `refused`
        - the sites that scored well and were dropped for being unreachable, which is the
        list worth reading when a survey returns fewer than asked for.
    """
    if not isinstance(rivers, WaterIndex):
        rivers = WaterIndex(rivers)
    scored, on_land, near_water = [], 0, 0
    for latitude, longitude in sunflower(samples):
        # **Gate one: is it dry?** One call, and it drops roughly half the planet.
        height = at(latitude, longitude)
        if height < LOW_M or height > HIGH_M + CASTLE_M:
            continue
        on_land += 1
        # **Gate two: is there water anywhere near?** Four calls, and it drops most of
        # what is left - and it is the gate that was missing. Without it the first survey
        # scored 1,797 inland points at 264 calls each and found not one harbour, because
        # six thousand samples on a nine-thousand-kilometre planet stand four hundred
        # kilometres apart and a coast is thinner than that. Cheap coast detection is what
        # lets the sample count go up far enough to hit one.
        # **High ground skips the coast gate, or castles cannot exist.** The gate asks for
        # water within eight kilometres, and a castle's whole reason is to stand above a
        # country rather than beside a sea - so a gate written for towns silently made one
        # of the archetypes unreachable. `kind_of` could return "castle" and never would,
        # which is the kind of dead branch that reads as a taste ("it just never picks
        # castles") instead of as a filter.
        commanding = height >= CASTLE_M
        sea = _sea_nearby(at, latitude, longitude, radius_m, coast_reach_m)
        if not commanding and not sea:
            continue
        near_water += 1 if sea else 0
        site = score_point(at, latitude, longitude, radius_m, rivers=rivers,
                           look_for_water=sea, classify=classify)
        if site is not None and site["kind"] is not None:
            scored.append(site)

    scored.sort(key=lambda s: -s["score"])

    # **Quotas, because one ranking populates a planet with one kind of place.** Harbour
    # access is worth forty and a castle's best case is thirty-five, so a straight
    # best-first walk over twenty-six thousand sites returned ten harbour villages and ten
    # fishing hamlets and nothing else - not because the world has no hills, but because a
    # hill can never outscore a harbour. Asking for so many of each is the only way a
    # single score can serve archetypes with different reasons for existing.
    #
    # No quota given means the old behaviour: best first, whatever they turn out to be.
    wanted = dict(quotas or {})
    taken = {kind: 0 for kind in wanted}

    chosen, refused = [], []

    def vacancy(site):
        """The first culture this ground fits whose quota is not yet full, or None."""
        if not wanted:
            return site["kind"] if len(chosen) < count else None
        for name in (site.get("kinds") or [site["kind"]]):
            if taken.get(name, 0) < wanted.get(name, 0):
                return name
        return None

    def far_enough(site):
        return all(_haversine(site["latitude_deg"], site["longitude_deg"],
                              other["latitude_deg"], other["longitude_deg"],
                              radius_m) >= separation_m for other in chosen)

    def linked(site):
        """Water of its own, or a neighbour already placed within walking reach."""
        if site["harbour_m"] is not None or site["landing_m"] is not None:
            return True
        return any(_haversine(site["latitude_deg"], site["longitude_deg"],
                              other["latitude_deg"], other["longitude_deg"],
                              radius_m) <= land_link_m for other in chosen)

    # **Grown outward from the water, not walked once best-first.** The single pass placed
    # every coastal site first, scattered across a whole planet, and then refused
    # twenty-six thousand inland ones because no chosen neighbour was within forty
    # kilometres - so castles and waystations came out at zero every time. Reading that as
    # "the scorer does not like hills" would have been wrong twice over: the hills scored
    # fine and were rejected by an ordering, and the rule doing the rejecting is correct.
    #
    # An inland settlement is reachable when a chain of settlements leads back to the sea,
    # so the set has to be built in the order that chain forms. Each sweep places what is
    # now linkable, which brings the frontier inland, which makes more sites linkable on
    # the next sweep. It settles when a sweep adds nothing. No new elevation calls: this
    # is arithmetic over sites already scored.
    while True:
        added = 0
        for site in scored:
            if site in chosen or not far_enough(site) or not linked(site):
                continue
            name = vacancy(site)
            if name is None:
                continue
            site["kind"] = name
            chosen.append(site)
            taken[name] = taken.get(name, 0) + 1
            added += 1
        if not added:
            break
        if wanted and all(taken.get(k, 0) >= n for k, n in wanted.items()):
            break

    # **A settlement on a river has to be looked for on the river.** Rivers occupy a few
    # hundred of a planet's sixty-four thousand degree cells, so a globally-spread sample
    # lands on one essentially never: the first survey after the water network went in
    # reported not one site with fresh water, on a planet with seven thousand river nodes.
    # Same shape as the level rings and the inland growth - a grid cannot find a thin thing,
    # so the thin thing is walked instead.
    if wanted and rivers.count:
        for point in _water_seeds(rivers, radius_m, separation_m):
            if all(taken.get(k, 0) >= n for k, n in wanted.items()):
                break
            site = score_point(at, point[0], point[1], radius_m, rivers=rivers,
                               classify=classify,
                               look_for_water=_sea_nearby(at, point[0], point[1],
                                                          radius_m, coast_reach_m))
            if site is None or site["kind"] is None:
                continue
            if not far_enough(site) or not linked(site):
                continue
            name = vacancy(site)
            if name is None:
                continue
            site["kind"] = name
            chosen.append(site)
            taken[name] = taken.get(name, 0) + 1

    # **Inland kinds are grown by looking around what is already placed.** The sweep above
    # can only ever choose from the global sample set, and that set is too coarse to
    # contain a neighbour - so the frontier never moves inland and the quota never fills.
    # Looking around each chosen site asks the question at the scale the answer lives at.
    if wanted:
        frontier = list(chosen)
        while frontier and not all(taken.get(k, 0) >= n for k, n in wanted.items()):
            seed = frontier.pop(0)
            for site in local_sites(at, radius_m, seed, land_link_m, rivers=rivers,
                                    classify=classify):
                if not far_enough(site) or not linked(site):
                    continue
                name = vacancy(site)
                if name is None:
                    continue
                site["kind"] = name
                chosen.append(site)
                taken[name] = taken.get(name, 0) + 1
                frontier.append(site)

    placed = {id(site) for site in chosen}
    for site in scored:
        if id(site) not in placed and not linked(site) and far_enough(site):
            refused.append(dict(site, why="no water access and no neighbour within reach"))

    return {"sites": chosen, "quotas": wanted, "taken": taken, "examined": samples, "on_land": on_land,
            "near_water": near_water, "scored": len(scored), "refused": refused}
