"""A generated world as an Evennia batch-command file: the world dug out by hand, in writing.

**The same world as the direct builder, built the long way round.** `export.py` writes a
data file and a script that calls `create_object` - fast, re-runnable, and opaque. This
writes a `.ev` file that Evennia's `batchcommands` processor runs one builder command at a
time - `dig`, `open`, `desc`, `set`, `tag`, `create` - exactly as if a builder were typing
them. Anyone can open it in an editor and read what it will do before it does it. That is
the whole case for it, and it is a real one when the world is being handed to somebody
else's game.

**It pays for that in three ways, and they are written down rather than discovered:**

  * **Slow.** Every line goes through the command handler and echoes to the builder.
  * **Not re-runnable.** Batch commands have no conditionals, so a second run digs a second
    world. The direct builder is the one to use for an update; because every room this file
    makes carries the same `wb_id`, it can refresh a world this file built.
  * **Some names cannot be typed.** Evennia's builder commands split their arguments on
    `,` (another object), `:` (a typeclass), `;` (aliases) and `/` (attributes), and `name`
    shares that parser - so no stock builder command can give a room a name with a comma in
    it, and 16% of the rooms in a generated city have one ("Water Colonnade, West End"). Those
    rooms are dug under the part before the comma and renamed with one line of `py`. That
    needs Developer permission, which `batchcommands` needs already.

**Rooms are found by alias, not by name.** A room has no dbref until it exists, so a file
written beforehand cannot say "#4312"; and names repeat (thousands of road rooms are "the
verge"). Every room gets an alias from its worldfile id, and every exit points at it.
"""

import re

#: The characters Evennia's builder commands treat as syntax inside an object name.
UNSAFE = re.compile(r"[,;:/=]")

ROOM_TYPECLASS = "typeclasses.rooms.Room"
EXIT_TYPECLASS = "typeclasses.exits.Exit"
FOLK_TYPECLASS = "typeclasses.characters.Character"
#: Things to look at (laws F1, F2), from the wb_fixtures.py the export writes beside this file.
THING_TYPECLASS = {"fixture": "world.wb_fixtures.Fixture",
                   "landmark": "world.wb_fixtures.Fixture",
                   "mirror": "world.wb_fixtures.Mirror",
                   "clock": "world.wb_fixtures.Clock"}


def room_alias(room_id):
    """
    The handle a room is found by.

    Notes:
        **Fixed width, because Evennia's search matches prefixes.** `wb_20227` would also
        match `wb_202270`, and a search that finds two answers finds none. Strings of one
        length cannot be prefixes of each other, so none of these can collide however big
        the world is.
    """
    return "wb_%07d" % int(room_id)


def person_alias(room_id, place):
    """
    The handle a person is found by.

    Notes:
        **Not `wb_...`.** A person's alias beginning with its room's would make every search
        for the room also find the people standing in it.
    """
    return "wbp_%07d_%02d" % (int(room_id), int(place))


def thing_alias(room_id, place):
    """
    The handle a thing to look at is found by.

    Notes:
        Its own prefix, so that no search for a room or a person ever finds a clock.
    """
    return "wbt_%07d_%02d" % (int(room_id), int(place))


def _literal(value):
    """A value as `set` will read it back: Python literal syntax, on one line."""
    return repr(value)


def _desc(target, text):
    """
    The command that gives something its description.

    Notes:
        `desc` takes its text verbatim, which keeps the file readable. Text that would not
        survive being a batch line - a newline, which the processor reads as the end of a
        paragraph - goes through `set` as a literal instead, where `\\n` is an escape.
    """
    if "\n" in text:
        return "set %s/desc = %s" % (target, _literal(text))
    return "desc %s = %s" % (target, text)


def commands(world):
    """
    Every command the world takes, in the order they must run.

    Args:
        world (dict): A flattened world, as `export.flatten` makes it.

    Yields:
        command (str): One builder command, on one line.

    Raises:
        ValueError: For an exit whose name the builder commands cannot express. Exits are
            referred to by name when they are made, so there is no rename to fall back on.

    Notes:
        **Two passes, because an exit needs somewhere to go.** Every room is dug first; only
        then does the builder visit each one in turn to describe it, furnish it, people it and
        open its exits.
    """
    rooms = world["rooms"]
    exits_from = {}
    for exit_ in world["exits"]:
        if UNSAFE.search(exit_["name"]):
            raise ValueError("exit %r cannot be written as a batch command: Evennia reads "
                             "%r in it as syntax" % (exit_["name"],
                                                     UNSAFE.search(exit_["name"]).group()))
        exits_from.setdefault(exit_["source"], []).append(exit_)

    for record in rooms:
        name = record["key"]
        typed = UNSAFE.split(name)[0].strip() or room_alias(record["id"])
        yield "dig %s;%s:%s" % (typed, room_alias(record["id"]), ROOM_TYPECLASS)

    for record in rooms:
        alias = room_alias(record["id"])
        yield "tel %s" % alias
        if UNSAFE.search(record["key"]):
            yield "py here.key = %s" % _literal(record["key"])
        yield _desc("here", record.get("desc") or "")
        yield "set here/wb_id = %s" % _literal(record["id"])
        for field, value in (("wb_area", record.get("area")),
                             ("wb_latitude", record.get("latitude_deg")),
                             ("wb_longitude", record.get("longitude_deg")),
                             ("wb_elevation_m", record.get("elevation_m")),
                             ("wb_stock", record.get("stock"))):
            if value is not None:
                yield "set here/%s = %s" % (field, _literal(value))
        if record.get("area"):
            yield "tag here = %s:area" % record["area"]
        if record.get("purpose"):
            yield "tag here = %s:wb_purpose" % record["purpose"]

        for place, person in enumerate(record.get("people") or ()):
            handle = person_alias(record["id"], place)
            typed = UNSAFE.split(person["name"])[0].strip() or handle
            yield "create/drop %s;%s:%s" % (typed, handle, FOLK_TYPECLASS)
            if UNSAFE.search(person["name"]):
                yield ("py [o for o in here.contents if %s in o.aliases.all()][0].key = %s"
                       % (_literal(handle), _literal(person["name"])))
            yield "set %s/wb_id = %s" % (handle, _literal("%s:folk:%s" % (record["id"], place)))
            yield "set %s/wb_role = %s" % (handle, _literal(person.get("role") or "folk"))
            wares = record.get("stock") if person.get("role") == "keeper" else None
            if wares:
                yield "set %s/stock = %s" % (handle, _literal(list(wares)))
                yield _desc(handle, "Goods for sale:" + "".join("\n  " + w for w in wares))
            else:
                yield _desc(handle, "One of the people of this place.")

        for place, thing in enumerate(record.get("fixtures") or ()):
            handle = thing_alias(record["id"], place)
            typed = UNSAFE.split(thing["key"])[0].strip() or handle
            kind = THING_TYPECLASS.get(thing.get("kind"), THING_TYPECLASS["fixture"])
            yield "create/drop %s;%s:%s" % (typed, handle, kind)
            if UNSAFE.search(thing["key"]):
                yield ("py [o for o in here.contents if %s in o.aliases.all()][0].key = %s"
                       % (_literal(handle), _literal(thing["key"])))
            yield "set %s/wb_id = %s" % (handle, _literal("%s:thing:%s" % (record["id"], place)))
            yield _desc(handle, thing.get("desc") or "")

        for exit_ in exits_from.get(record["id"], ()):
            aliases = "".join(";" + a for a in (exit_.get("aliases") or ()))
            yield "open %s%s:%s = %s" % (exit_["name"], aliases, EXIT_TYPECLASS,
                                         room_alias(exit_["destination"]))

    # Leave the builder at home, not in the last shed on the last road. `home` rather than
    # `tel #2`: Limbo is #2 only in a game nobody has touched - the first game this was run
    # against keeps it at #6606 - and a file handed to somebody else cannot know their dbrefs.
    yield "home"


HEADER = """# {name}.ev - a generated world, as Evennia batch commands.
#
# Run it in-game, as a Developer or superuser, with this file in your world/ folder:
#
#     batchcommands {name}
#
# It builds {rooms} rooms, {exits} exits and {people} people with ordinary builder commands,
# one at a time - {count} commands in all. That is slow; the builder watches every one.
#
# RUN IT ONCE. Batch commands cannot check what already exists, so a second run builds a
# second world. To refresh or update a world this file built, use build_{name}.py instead:
# every room here carries the same wb_id, so the direct builder finds and updates them.
#
# Rooms are found by alias - wb_ and the room's id - so `tel wb_0020227` goes straight to
# one. Rooms whose names contain a comma are dug under the part before it and renamed by a
# line of `py`, because Evennia's builder commands read a comma as "another object follows".
#
# Every command ends at the next line beginning with #.
#
"""


def write(world, path, name):
    """
    Write the world as a batch-command file.

    Args:
        world (dict): A flattened world.
        path (str): Where to write it.
        name (str): What to call it in the instructions.

    Returns:
        counts (dict): Commands written, and the rooms that needed a `py` rename.
    """
    lines = list(commands(world))
    renamed = sum(1 for line in lines if line.startswith("py here.key"))
    people = sum(len(r.get("people") or ()) for r in world["rooms"])
    with open(path, "w", encoding="utf-8") as handle:
        handle.write(HEADER.format(name=name, rooms=len(world["rooms"]),
                                   exits=len(world["exits"]), people=people,
                                   count=len(lines)))
        handle.write("\n#\n".join(lines))
        handle.write("\n")
    return {"commands": len(lines), "renamed_by_py": renamed}
