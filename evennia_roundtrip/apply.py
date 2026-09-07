"""Write a worldfile's answers back into an Evennia game.

**This is the half a contrib would ship**, and it is deliberately the dullest file here. It
reads a file and sets attributes. No terrain generator, no engine, no Rust: roadmap section
3.1's rule is that *Evennia receives concrete values and never needs the engine at
runtime*, and this is what that rule looks like when it is obeyed.

**It writes to a copy by default and says so.** A tool whose first act on somebody's game
is an UPDATE has to earn that, and a demonstration has not. `apply` takes the database path
it is given and nothing more clever; the caller decides whether that is the real game.

**The attributes it sets are namespaced and few.** Anything a game already had is left
alone; the six `wb_` keys below are the entire footprint, so removing them removes the
tool.
"""

import base64
import os
import pickle
import sqlite3

#: The whole footprint. A game can drop these six and be exactly as it was.
WB_LATITUDE = "wb_latitude"
WB_LONGITUDE = "wb_longitude"
WB_ELEVATION = "wb_elevation_m"
WB_AREA = "wb_area"
WB_PORT_AREA = "wb_port_area"
WB_HAS_PORT = "wb_has_port"

ATTRIBUTE_KEYS = (
    WB_LATITUDE,
    WB_LONGITUDE,
    WB_ELEVATION,
    WB_AREA,
    WB_PORT_AREA,
    WB_HAS_PORT,
)


def _dump(value):
    """Encode a value the way Evennia stores one: a base64 pickle, protocol 4."""
    return base64.b64encode(pickle.dumps(value, protocol=4)).decode("ascii")


def _attribute_id(connection, key, value):
    """Find or make the attribute row holding this key and value.

    Evennia shares one `typeclasses_attribute` row between every object carrying the same
    key and value, and joins objects to it through a through-table. Writing per-object
    rows would work and would also double the table for a coordinate every room has a
    different one of - so this reuses a row when the value matches and makes one when it
    does not, which is what Evennia's own attribute handler does.
    """
    encoded = _dump(value)
    row = connection.execute(
        "SELECT id FROM typeclasses_attribute"
        " WHERE db_key = ? AND db_value = ? AND db_attrtype IS NULL",
        (key, encoded),
    ).fetchone()
    if row:
        return row[0]
    cursor = connection.execute(
        "INSERT INTO typeclasses_attribute"
        " (db_key, db_value, db_category, db_lock_storage, db_model, db_attrtype, db_date_created)"
        " VALUES (?, ?, NULL, '', 'objectdb', NULL, datetime('now'))",
        (key, encoded),
    )
    return cursor.lastrowid


def _set(connection, object_id, key, value):
    """Set one attribute on one object, replacing whatever was there."""
    connection.execute(
        "DELETE FROM objects_objectdb_db_attributes"
        " WHERE objectdb_id = ? AND attribute_id IN"
        " (SELECT id FROM typeclasses_attribute WHERE db_key = ? AND db_model = 'objectdb')",
        (object_id, key),
    )
    attribute_id = _attribute_id(connection, key, value)
    connection.execute(
        "INSERT INTO objects_objectdb_db_attributes (objectdb_id, attribute_id)"
        " VALUES (?, ?)",
        (object_id, attribute_id),
    )


def copy_database(source, destination):
    """
    Make a working copy of an Evennia database that SQLite will actually open.

    Args:
        source (str): The game's `.db3`.
        destination (str): Where to put the copy.

    Returns:
        destination (str): The copy.

    Notes:
        **`shutil.copyfile` is the wrong tool and this project found out the hard way.**
        A byte copy of a SQLite file is a copy of whatever state the pages happened to be
        in, and any journal or write-ahead log beside it is left behind. The first run of
        this pipeline copied the fixture that way and the first write into the copy raised
        `database disk image is malformed`. SQLite's own backup API copies a *consistent*
        database, taking the necessary lock, which is exactly the problem it exists for.
    """
    for leftover in (destination, destination + "-journal", destination + "-wal",
                     destination + "-shm"):
        if os.path.exists(leftover):
            os.remove(leftover)
    origin = sqlite3.connect("file:%s?mode=ro" % source, uri=True)
    copy = sqlite3.connect(destination)
    try:
        origin.backup(copy)
    finally:
        copy.close()
        origin.close()
    return destination


def apply(document, path):
    """
    Set every room's global position from a worldfile.

    Args:
        document (dict): A worldfile, already version-checked by `worldfile.read`.
        path (str): The Evennia database to write. This IS written to.

    Returns:
        report (dict): `rooms`, `areas`, and `missing` - room ids in the file that the
        database does not have, which is how a stale worldfile announces itself.

    """
    connection = sqlite3.connect(path)
    known = {row[0] for row in connection.execute("SELECT id FROM objects_objectdb")}
    written, missing = 0, []
    try:
        for area in document["areas"]:
            port = area.get("port") or {}
            for room in area["rooms"]:
                if room["id"] not in known:
                    missing.append(room["id"])
                    continue
                _set(connection, room["id"], WB_LATITUDE, room["latitude_deg"])
                _set(connection, room["id"], WB_LONGITUDE, room["longitude_deg"])
                _set(connection, room["id"], WB_ELEVATION, room["elevation_m"])
                _set(connection, room["id"], WB_AREA, area["name"])
                _set(connection, room["id"], WB_HAS_PORT, bool(port.get("has_port")))
                _set(connection, room["id"], WB_PORT_AREA, port.get("port_area"))
                written += 1
        connection.commit()
    finally:
        connection.close()
    return {"rooms": written, "areas": len(document["areas"]), "missing": missing}


def verify(path, room_ids):
    """
    Read back what `apply` wrote, straight from the database.

    Args:
        path (str): The Evennia database.
        room_ids (iterable): Rooms to check.

    Returns:
        found (dict): Room id to a dict of the `wb_` attributes present on it.

    Notes:
        This exists because "the write returned without an exception" is not evidence the
        game can see the values. Reading them back through a separate connection is.

    """
    connection = sqlite3.connect("file:%s?mode=ro" % path, uri=True)
    try:
        found = {}
        for room_id in room_ids:
            rows = connection.execute(
                "SELECT a.db_key, a.db_value FROM typeclasses_attribute a"
                " JOIN objects_objectdb_db_attributes m ON m.attribute_id = a.id"
                " WHERE m.objectdb_id = ? AND a.db_key IN (%s)"
                % ",".join("?" * len(ATTRIBUTE_KEYS)),
                (room_id,) + ATTRIBUTE_KEYS,
            )
            found[room_id] = {
                key: pickle.loads(base64.b64decode(value)) for key, value in rows
            }
        return found
    finally:
        connection.close()
