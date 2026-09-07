"""Put an area's lattice on the globe, and ask the planet what is underneath it.

The lattice from `layout` is in room-steps from a root. Placing it needs three numbers a
builder chooses and nothing derives: **where the root sits, which way the map's north
points, and how far apart two adjacent rooms are.** Everything else follows.

**The projection is the one the engine already owns.** `TangentFrame` is azimuthal
equidistant about the anchor, so distance and bearing *from the anchor* are exact at any
range. An area is small, so the frame is doing almost nothing - but using it rather than
adding degrees to a latitude means a placement near a pole behaves, and means this file
does not invent a second answer to a question the engine already answers.

**Elevation is read, not chosen.** Once a room has a latitude and longitude, the generated
planet says how high it is and whether it is under water. That is the point of the whole
exercise: the game's map stops being a floating diagram and acquires ground.
"""

import math
from dataclasses import dataclass

from worldbuilder.geometry.sphere import EARTH_RADIUS_M, SpherePoint
from worldbuilder.geometry.tangent import TangentFrame
from worldbuilder.geometry.vectors import Vec3

#: How far apart two rooms one exit apart are, unless a builder says otherwise.
#:
#: A room is a place you stand and look around, so a street of them is a street, not a
#: county. Sixty metres puts a twenty-room guild inside a city block and a three-hundred
#: room city across two kilometres, which is the scale a MUD's prose implies.
DEFAULT_ROOM_SPACING_M = 60.0

#: A room is a port if navigable sea lies within this of it, unless a builder says otherwise.
DEFAULT_PORT_REACH_M = 2000.0

#: Shallower than this and a laden hull does not come in, so the water is not a port.
#:
#: A cove that dries at low water is near the sea and is not a harbour, and the difference
#: is the entire reason this field is not simply "is it wet".
DEFAULT_PORT_DEPTH_M = 4.0


@dataclass(frozen=True)
class Anchor:
    """Where a builder decided an area sits, and how it is turned.

    Attributes:
        latitude_deg (float): The root room's latitude.
        longitude_deg (float): The root room's longitude.
        bearing_deg (float): Which compass bearing the lattice's +y points along. Zero
            means the map's north is the planet's north.
        room_spacing_m (float): Metres per lattice step.

    """

    latitude_deg: float
    longitude_deg: float
    bearing_deg: float = 0.0
    room_spacing_m: float = DEFAULT_ROOM_SPACING_M


@dataclass
class PlacedRoom:
    """One room, once it knows where on the planet it is."""

    id: int
    key: str
    cell: tuple
    latitude_deg: float
    longitude_deg: float
    elevation_m: float
    #: True when the generated planet puts this room under the sea.
    submerged: bool


def _rotate(x, y, bearing_deg):
    """Turn lattice coordinates by a bearing, clockwise from north as a compass reads."""
    angle = math.radians(bearing_deg)
    sin, cos = math.sin(angle), math.cos(angle)
    # A compass bearing runs clockwise from north while the frame's axes run
    # counter-clockwise from east, which is why this is not the textbook rotation matrix.
    return (x * cos + y * sin, -x * sin + y * cos)


def anchor_for_room(layout, room_id, latitude_deg, longitude_deg, bearing_deg,
                    room_spacing_m, radius_m=EARTH_RADIUS_M):
    """
    The anchor that puts ONE NAMED ROOM at a chosen place.

    Args:
        layout (Layout): From `layout.build`.
        room_id (int): The room the builder is placing.
        latitude_deg (float): Where that room goes.
        longitude_deg (float): Where that room goes.
        bearing_deg (float): Which way the lattice's +y points.
        room_spacing_m (float): Metres per lattice step.
        radius_m (float, optional): The planet's radius.

    Returns:
        anchor (Anchor): Anchored on the layout's root, such that `room_id` lands where asked.

    Notes:
        **A builder points at a dock, not at a root.** The anchor positions the layout's root
        room, which is whichever room the graph walk started from - an implementation detail
        the builder has no reason to know or care about. Asking somebody to work out where
        the root must go so that the slip ends up on the coast is asking them to run this
        function in their head.

        It is exact rather than iterative: the room's offset from the root is known in local
        metres, so the root is that offset applied backwards from the requested point in a
        frame built there.
    """
    cell = layout.cells.get(room_id)
    if cell is None:
        raise KeyError("room %r is not in this layout" % room_id)
    x_m, y_m = _rotate(cell[0] * room_spacing_m, cell[1] * room_spacing_m, bearing_deg)
    frame = TangentFrame.at_latlon(latitude_deg, longitude_deg, radius_m)
    root = frame.local_to_sphere(-x_m, -y_m)
    root_latitude, root_longitude = root.to_latlon()
    return Anchor(root_latitude, root_longitude, bearing_deg, room_spacing_m)


def place(layout, area, anchor, surface):
    """
    Give every room in an area a latitude, a longitude and a ground height.

    Args:
        layout (Layout): From `layout.build`.
        area (Area): From `evdb.read`.
        anchor (Anchor): The builder's three numbers.
        surface (Surface): The generated planet.

    Returns:
        rooms (list): `PlacedRoom`, in room-id order.

    """
    frame = TangentFrame.at_latlon(
        anchor.latitude_deg, anchor.longitude_deg, surface.radius_m
    )
    placed = []
    for room_id in sorted(layout.cells):
        cell = layout.cells[room_id]
        x_m, y_m = _rotate(
            cell[0] * anchor.room_spacing_m, cell[1] * anchor.room_spacing_m,
            anchor.bearing_deg,
        )
        point = frame.local_to_sphere(x_m, y_m)
        latitude, longitude = point.to_latlon()
        elevation = surface.elevation_m(point)
        room = area.rooms.get(room_id)
        placed.append(
            PlacedRoom(
                id=room_id,
                key=room.key if room else "#%d" % room_id,
                cell=cell,
                latitude_deg=latitude,
                longitude_deg=longitude,
                elevation_m=elevation,
                submerged=elevation < 0.0,
            )
        )
    return placed


#: Room keys that are SUPPOSED to be at or below the waterline.
#:
#: A boat ramp runs into the water by definition, and a landing stage stands over it. Marking
#: them wet is not an exception to the land check - it is the check knowing what it is looking
#: at. Anything not named here that comes out submerged is a placement fault.
WATER_ROOM_WORDS = ("ramp", "landing stage", "slip", "dock", "quay", "jetty", "wharf", "pier")


def water_room(key):
    """Whether a room is one the builder meant to be in the water."""
    lowered = key.lower()
    return any(word in lowered for word in WATER_ROOM_WORDS)


def land_fit(rooms):
    """
    Which rooms are drawn on water that should not be.

    Args:
        rooms (list): `PlacedRoom`, from `place`.

    Returns:
        report (dict): `dry`, `wet_expected`, `wet_unexpected` and the offending rooms.

    Notes:
        **An area drawn half in the sea looks like a rendering bug and is a placement fault.**
        The submerged flag has been computed since the first version of this pipeline and
        nothing ever read it, so a camp whose bunkhouse sat two hundred metres offshore
        exported clean, applied clean, and read back clean. Every check passed because none
        of them was this one.
    """
    dry, expected, unexpected = [], [], []
    for room in rooms:
        if not room.submerged:
            dry.append(room)
        elif water_room(room.key):
            expected.append(room)
        else:
            unexpected.append(room)
    return {
        "dry": len(dry),
        "wet_expected": len(expected),
        "wet_unexpected": len(unexpected),
        "offenders": [(r.key, round(r.elevation_m, 2)) for r in unexpected],
        "fits": not unexpected,
    }


def fit_spacing(layout, area, latitude_deg, longitude_deg, bearing_deg, surface,
                candidates=(200.0, 150.0, 120.0, 90.0, 70.0, 55.0, 45.0, 35.0, 25.0,
                            18.0, 12.0, 8.0)):
    """
    The largest room spacing at which an area sits on land.

    Args:
        layout (Layout): From `layout.build`.
        area (Area): From `evdb.read`.
        latitude_deg (float): The anchor.
        longitude_deg (float): The anchor.
        bearing_deg (float): The anchor's bearing.
        surface (Surface): The generated planet.
        candidates (tuple, optional): Spacings tried, largest first.

    Returns:
        result (tuple): `(spacing_m or None, report)` for the first spacing that fits.

    Notes:
        **Largest first, because a smaller area is a worse area.** Rooms crammed to eight
        metres apart put a whole camp inside one building's footprint, so the search takes
        the biggest spacing that still lands on ground rather than the smallest that
        certainly will. If nothing fits, the honest answer is that the island is too small
        for this area - which is a fact about the world, not a number to force.
    """
    best = None
    for spacing in candidates:
        anchor = Anchor(latitude_deg, longitude_deg, bearing_deg, spacing)
        report = land_fit(place(layout, area, anchor, surface))
        if report["fits"]:
            return spacing, report
        if best is None or report["wet_unexpected"] < best[1]["wet_unexpected"]:
            best = (spacing, report)
    return None, best[1] if best else {}


def sea_reach(point, surface, reach_m=DEFAULT_PORT_REACH_M, depth_m=DEFAULT_PORT_DEPTH_M,
              samples=16):
    """
    Whether navigable sea lies within reach of a point, and how far off it is.

    Args:
        point (SpherePoint): Where to look from.
        surface (Surface): The generated planet.
        reach_m (float, optional): How far inland a port may serve.
        depth_m (float, optional): How much water a hull needs.
        samples (int, optional): Bearings tried, evenly spaced.

    Returns:
        distance_m (float or None): Distance to the nearest water deep enough, or None.

    Notes:
        **This is a ring search, and a ring search can miss.** It walks outward along a
        fixed set of bearings, so a channel narrower than the gap between two rays at full
        reach is invisible to it. At sixteen rays and two kilometres that gap is 785 m, so
        it finds harbours and would miss a creek. Widening it is a parameter, not a
        redesign - but a builder overriding `has_port` is the designed answer, because the
        game must win an argument about its own coast.
    """
    frame = TangentFrame.at(point, surface.radius_m)
    # Rings are a FRACTION of the reach, never a fixed hundred metres. Tying the step to
    # an absolute distance made the cost grow with the reach: a 25 km search ran 250 rings
    # against a 2 km search's 20, so widening the reach by twelve made the search twelve
    # times slower per candidate and turned a globe sweep into an hour. Twenty rings
    # resolves a coastline at any reach the caller asks for.
    steps = 20
    for step in range(1, steps + 1):
        distance = reach_m * step / steps
        for ray in range(samples):
            angle = 2.0 * math.pi * ray / samples
            probe = frame.local_to_sphere(
                distance * math.cos(angle), distance * math.sin(angle)
            )
            if surface.elevation_m(probe) <= -depth_m:
                return distance
    return None


#: How near the sea a *settlement* is placed. Deliberately larger than the port reach.
#:
#: An area twenty kilometres inland is still a coastal place to put a town, and whether it
#: has its own harbour is a separate question answered separately. Conflating the two is
#: what made the first search find one anchor in twenty thousand samples: it was hunting
#: for the thin ribbon of land that is *both* dry and within two kilometres of navigable
#: water, which on a real coastline is a few pixels wide.
COASTAL_SEARCH_REACH_M = 25000.0


def _local_anchors(surface, count, near, within_m, samples, low_m, high_m, reach_m,
                   separation_m, require_port):
    """Search a disc around a point, at a spacing the disc can actually resolve.

    **Filtering a global grid by distance is not a local search, and this is the second
    time that confusion cost a run.** A forty-thousand-point sweep of an Earth-sized globe
    puts a sample every 160 km or so. Ask it for anchors within 300 km of a chosen harbour
    and roughly four of its samples fall inside the circle at all - so the search returned
    one anchor for five areas and looked like a world with no coastline.

    A disc of a few hundred kilometres needs its own sampling, not a subset of somebody
    else's. The spiral below puts `samples` points inside the requested radius, which at
    300 km and 40,000 points is a sample every 2.7 km - fine enough to find a harbour.
    """
    frame = TangentFrame.at(near, surface.radius_m)
    golden = math.pi * (3.0 - math.sqrt(5.0))
    found = []
    for index in range(samples):
        # A sunflower spiral: radius as the square root of the index keeps the points
        # evenly dense rather than crowded at the middle.
        radius = within_m * math.sqrt((index + 0.5) / samples)
        angle = golden * index
        point = frame.local_to_sphere(radius * math.cos(angle), radius * math.sin(angle))
        elevation = surface.elevation_m(point)
        if not (low_m <= elevation <= high_m):
            continue
        if sea_reach(point, surface, reach_m, DEFAULT_PORT_DEPTH_M, samples=8) is None:
            continue
        if require_port and sea_reach(point, surface, DEFAULT_PORT_REACH_M,
                                      DEFAULT_PORT_DEPTH_M) is None:
            continue
        if any(point.distance_to(other, surface.radius_m) < separation_m
               for other in found):
            continue
        found.append(point)
        if len(found) >= count:
            break
    return found


def _coprime_stride(samples):
    """A stride that visits every index of a cycle exactly once."""
    stride = max(1, int(samples * 0.618))
    while math.gcd(stride, samples) != 1:
        stride += 1
    return stride


def coastal_anchors(surface, count, samples=40000, low_m=2.0, high_m=400.0,
                    reach_m=COASTAL_SEARCH_REACH_M, separation_m=80000.0,
                    require_port=False, near=None, within_m=None):
    """
    Find places on a generated planet worth putting an area.

    Args:
        surface (Surface): The generated planet.
        count (int): How many anchors are wanted.
        samples (int, optional): Points examined, spread evenly over the globe.
        low_m (float, optional): Lowest ground a settlement will accept.
        high_m (float, optional): Highest.
        reach_m (float, optional): How near the sea has to be to call it coastal.
        separation_m (float, optional): How far apart two anchors must be. This is a
            MINIMUM and cannot pull anchors together - lowering it from 80 km to 12 km
            changed nothing, because qualifying coasts are rarer than either figure.
            Clustering needs `near` and `within_m`, which is a different constraint.
        near (SpherePoint, optional): Keep anchors within `within_m` of this point.
        within_m (float, optional): The radius that goes with `near`.

    Returns:
        anchors (list): `SpherePoint`, coastal land, spread out.

    Notes:
        **This exists because assuming a good anchor produced a demonstration in four
        kilometres of water.** The first run of this pipeline reused a latitude and
        longitude from another part of the repository - a coast that was measured, on a
        different planet. On this seed the same point is deep ocean, so 226 rooms were
        placed correctly and every one of them was on the seabed. The lesson is the one
        this project keeps relearning: a number is only good on the population it was
        measured against.

        **The spread is a Fibonacci sphere**, the same grid-free construction
        `continentality.calibrate` uses to find a quantile without a raster. Twenty
        thousand points on an Earth-sized globe put a sample every 160 km or so, which
        finds coasts and would miss an island smaller than that.
    """
    if near is not None and within_m is not None:
        return _local_anchors(surface, count, near, within_m, samples, low_m, high_m,
                              reach_m, separation_m, require_port)

    golden = math.pi * (3.0 - math.sqrt(5.0))
    found = []
    # Walk the sequence with a stride rather than in order. A Fibonacci sphere runs pole
    # to pole, so taking the first points that qualify returns the first coast it meets
    # going south - the first version of this put all four areas between 76 and 78 degrees
    # north, which looked like a decision and was an artefact of the loop. A stride that
    # is coprime with the sample count visits every index exactly once, in an order that
    # crosses every latitude, and is still fully deterministic.
    stride = _coprime_stride(samples)
    for step in range(samples):
        index = (step * stride) % samples
        z = 1.0 - 2.0 * (index + 0.5) / samples
        radius = math.sqrt(max(0.0, 1.0 - z * z))
        angle = golden * index
        point = SpherePoint.from_vector(
            Vec3(radius * math.cos(angle), radius * math.sin(angle), z)
        )
        if near is not None and within_m is not None:
            if point.distance_to(near, surface.radius_m) > within_m:
                continue
        elevation = surface.elevation_m(point)
        if not (low_m <= elevation <= high_m):
            continue
        if sea_reach(point, surface, reach_m, DEFAULT_PORT_DEPTH_M, samples=8) is None:
            continue
        # A place near the sea and a place a ship can reach are different places, and the
        # difference is the whole reason `has_port` exists. Asking for one explicitly means
        # a demonstration can put an area on a harbour and another one up a valley, and
        # have the port mapping do real work rather than report nothing twice.
        if require_port and sea_reach(point, surface, DEFAULT_PORT_REACH_M,
                                      DEFAULT_PORT_DEPTH_M) is None:
            continue
        if any(point.distance_to(other, surface.radius_m) < separation_m for other in found):
            continue
        found.append(point)
        if len(found) >= count:
            break
    return found


def port_mapping(placements, surface, reach_m=DEFAULT_PORT_REACH_M,
                 depth_m=DEFAULT_PORT_DEPTH_M):
    """
    Which areas have a port, and which port serves the ones that do not.

    Args:
        placements (dict): Area name to (Anchor, list of PlacedRoom).
        surface (Surface): The generated planet.
        reach_m (float, optional): How far inland a port may serve.
        depth_m (float, optional): How much water a hull needs.

    Returns:
        mapping (dict): Area name to a dict of `has_port`, `port_area`,
        `port_distance_m` and `port_metric`.

    Notes:
        **`port_distance_m` is a great-circle distance between anchors and says so.** A
        port forty kilometres away across a four-thousand-metre range is not nearer than
        one ninety kilometres along a valley. The honest answer needs a slope-weighted
        cost surface, which is real work and legitimately allowed to be a bake here
        because it runs once over a finite set of areas. Until it exists, the field names
        its metric so nobody reads it as travel time.
    """
    anchors = {}
    ports = {}
    for name, (anchor, rooms) in placements.items():
        anchors[name] = SpherePoint.from_latlon(anchor.latitude_deg, anchor.longitude_deg)
        nearest = None
        for room in rooms:
            point = SpherePoint.from_latlon(room.latitude_deg, room.longitude_deg)
            distance = sea_reach(point, surface, reach_m, depth_m)
            if distance is not None and (nearest is None or distance < nearest):
                nearest = distance
        ports[name] = nearest

    mapping = {}
    for name in placements:
        if ports[name] is not None:
            mapping[name] = {
                "has_port": True,
                "port_area": name,
                "port_distance_m": 0.0,
                "port_metric": "self",
            }
            continue
        best, best_distance = None, None
        for other, distance_to_sea in ports.items():
            if distance_to_sea is None or other == name:
                continue
            separation = anchors[name].distance_to(anchors[other], surface.radius_m)
            if best_distance is None or separation < best_distance:
                best, best_distance = other, separation
        mapping[name] = {
            "has_port": False,
            "port_area": best,
            "port_distance_m": best_distance,
            "port_metric": "great-circle between anchors",
        }
    return mapping
