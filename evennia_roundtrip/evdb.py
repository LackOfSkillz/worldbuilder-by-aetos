"""Read an Evennia game out of its SQLite file, with no Django and no running server.

**Why not import Evennia.** A worldfile is produced once, out of band, by a studio that has
no reason to boot a game. Reading the database directly means the studio needs neither the
game's settings module nor its typeclasses on the path - which matters, because a studio
that can only read a game it can also *run* is a studio nobody can point at another
person's game.

**The area key is a priority, not a name, and that is a measured decision.** The fixture
carries seven attributes that each look like they say which area a room belongs to, and
they disagree with each other:

    key                  rooms   distinct values
    region                1686   2   (1599 of them 'default_region')
    zone                  1564   21
    zone_id               1564   22
    area                   645   31
    area_id                826   9
    canonical_area          19   10
    canonical_area_name     21   4

No single key covers the game. `region` looks like the obvious answer and is the worst one:
95% of its rooms sit in an undifferentiated bucket. So the reader takes the first key that
answers for a given room, and records which key answered, because a builder looking at a
surprising placement needs to know where the name came from.

**Attribute values are base64-encoded pickles.** That is Evennia's own storage format, not
a choice made here. Unpickling arbitrary data executes arbitrary code, so this module
refuses anything but plain data - see `_Restricted`.
"""

import base64
import io
import pickle
import sqlite3
from dataclasses import dataclass, field

#: Tried in order; the first that answers for a room names that room's area.
#:
#: `region` is last despite being the most populated, because it is the least informative:
#: it puts 1,599 of 1,686 rooms in one bucket called `default_region`, which is not an area.
AREA_KEY_PRIORITY = ("area_id", "zone_id", "zone", "area", "region")

#: Never an area name, whichever key it came from. A room tagged this way is untagged.
NOT_AN_AREA = frozenset({"default_region", "", "None"})

#: Exit names that carry a direction. Everything else is a door, a portal or a named way.
COMPASS = {
    "north": (0, 1, 0),
    "south": (0, -1, 0),
    "east": (1, 0, 0),
    "west": (-1, 0, 0),
    "northeast": (1, 1, 0),
    "northwest": (-1, 1, 0),
    "southeast": (1, -1, 0),
    "southwest": (-1, -1, 0),
    "up": (0, 0, 1),
    "down": (0, 0, -1),
}


class _Restricted(pickle.Unpickler):
    """An unpickler that will not build anything but plain data.

    Evennia stores attribute values as pickles, and a worldfile tool is pointed at
    databases it did not write. `find_class` is the hook every pickle exploit goes through,
    so refusing it outright is the whole defence.
    """

    def find_class(self, module, name):
        raise pickle.UnpicklingError("refusing to construct %s.%s" % (module, name))


def _load(blob):
    """Decode one Evennia attribute value, or return None if it is not plain data."""
    try:
        return _Restricted(io.BytesIO(base64.b64decode(blob))).load()
    except Exception:
        return None


@dataclass
class Room:
    """One room, as the database has it."""

    id: int
    key: str
    area: str = ""
    area_key: str = ""
    desc: str = ""


@dataclass
class Exit:
    """One exit, source to destination, by the name a player types."""

    name: str
    source: int
    destination: int


@dataclass
class Area:
    """A named set of rooms and the exits between them."""

    name: str
    #: Which attribute the name came from, so a surprising placement is traceable.
    key: str
    rooms: dict = field(default_factory=dict)
    exits: list = field(default_factory=list)
    #: Exits with exactly one end inside. An area with many of these is not self-contained.
    boundary: list = field(default_factory=list)

    @property
    def compass_fraction(self):
        """How much of the internal graph is laid out by direction rather than by name."""
        if not self.exits:
            return 0.0
        return sum(1 for e in self.exits if e.name in COMPASS) / len(self.exits)


def _attribute(connection, key):
    """Every value of one attribute, keyed by the object it is on."""
    rows = connection.execute(
        "SELECT m.objectdb_id, a.db_value"
        " FROM objects_objectdb_db_attributes m"
        " JOIN typeclasses_attribute a ON a.id = m.attribute_id"
        " WHERE a.db_key = ?",
        (key,),
    )
    found = {}
    for object_id, blob in rows:
        value = _load(blob)
        if value is not None:
            found[object_id] = value
    return found


def read(path, area_keys=AREA_KEY_PRIORITY):
    """
    Read every area out of an Evennia database.

    Args:
        path (str): The `.db3` file. Opened read-only; this never writes.
        area_keys (tuple, optional): Attribute names, in priority order.

    Returns:
        areas (dict): Name to `Area`, largest first.

    """
    connection = sqlite3.connect("file:%s?mode=ro" % path, uri=True)
    try:
        rooms = {
            object_id: Room(object_id, name or "#%d" % object_id)
            for object_id, name in connection.execute(
                "SELECT id, db_key FROM objects_objectdb"
                " WHERE db_typeclass_path LIKE '%room%'"
            )
        }
        for object_id, text in _attribute(connection, "desc").items():
            if object_id in rooms and isinstance(text, str):
                rooms[object_id].desc = text

        tags = {key: _attribute(connection, key) for key in area_keys}
        for object_id, room in rooms.items():
            for key in area_keys:
                value = tags[key].get(object_id)
                if isinstance(value, str) and value not in NOT_AN_AREA:
                    room.area, room.area_key = value, key
                    break

        edges = [
            Exit(name or "", source, destination)
            for name, source, destination in connection.execute(
                "SELECT db_key, db_location_id, db_destination_id FROM objects_objectdb"
                " WHERE db_typeclass_path LIKE '%exits.Exit%'"
                " AND db_location_id IS NOT NULL AND db_destination_id IS NOT NULL"
            )
        ]
    finally:
        connection.close()

    areas = {}
    for room in rooms.values():
        if not room.area:
            continue
        area = areas.setdefault(room.area, Area(room.area, room.area_key))
        area.rooms[room.id] = room

    for edge in edges:
        source = rooms.get(edge.source)
        destination = rooms.get(edge.destination)
        if source is None or destination is None:
            continue
        if source.area and source.area == destination.area:
            areas[source.area].exits.append(edge)
        else:
            for room in (source, destination):
                if room.area:
                    areas[room.area].boundary.append(edge)

    return dict(sorted(areas.items(), key=lambda item: -len(item[1].rooms)))


def census(areas, minimum=1):
    """A readable table of what was read, for a builder deciding what to place."""
    lines = ["%-32s%-10s%6s%6s%7s%8s" % ("area", "key", "rooms", "exits", "bound", "compass")]
    for area in areas.values():
        if len(area.rooms) < minimum:
            continue
        lines.append(
            "%-32s%-10s%6d%6d%7d%7.0f%%"
            % (
                area.name[:31],
                area.key,
                len(area.rooms),
                len(area.exits),
                len(area.boundary),
                area.compass_fraction * 100,
            )
        )
    return "\n".join(lines)
