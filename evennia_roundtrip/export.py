"""Turn a generated world into something an Evennia game can actually build.

**Evennia receives concrete values and never needs the engine at runtime.** That is the
roadmap's rule and this file obeys it: what comes out is a plain data file and a small
batchcode script, and neither of them imports the terrain engine, the pathfinder or
anything else from this project. A game builds the world without knowing the world was
generated.

**Data and code are separate on purpose.** A hundred and thirty areas is nearly six
thousand rooms; writing them as Python statements makes a megabyte of source that no
reviewer can read and no diff can show. The rooms go in a JSON file and the batchcode is
sixty lines that reads it - which is also what makes a re-import a data change rather than
a code change.

**A room's identity is its `wb_id`, and that is what makes an import repeatable.** Running
the same batchcode twice must not double the world, so every room carries the worldfile's
own id as an attribute and the builder looks for it before creating anything. The same
handle is what lets a later run update descriptions in place.
"""

import json
import os

#: The attribute every built room carries, and the one the builder searches on.
WB_ID = "wb_id"

#: The builder that is written beside the data and reads it by name.
#:
#: **Written in the target game's own idiom.** Evennia will run either a batchcode file or
#: a module with a `build(caller)` function, and the game this was first pointed at uses the
#: second - `world/build_harbours.py` is called with `@py from world.build_harbours import
#: build; build(self)`. Matching the house style of the game being built into beats being
#: generically correct.
#:
#: Deliberately small, deliberately dull, and deliberately free of anything from this
#: package: a game that has this file and its JSON needs nothing else installed.
BUILDER = '''"""Build a generated world into this game.

Rooms and exits come from {data}, which sits beside this file.

    @py from world.{module} import build; build(self)

Running it twice does not double the world: every room carries its worldfile id as
`{wb_id}` and is found by it, so a second run updates what is there instead of adding to
it. That is also what lets a later export refresh descriptions in place.

Nothing here imports the generator. A game with these two files needs nothing else.
"""

import json
import os

from evennia import create_object
from evennia.objects.models import ObjectDB

HERE = os.path.dirname(os.path.abspath(__file__))
DATA = os.path.join(HERE, "{data}")
ROOM_TYPECLASS = "typeclasses.rooms.Room"
EXIT_TYPECLASS = "typeclasses.exits.Exit"
WB_ID = "{wb_id}"


def _existing():
    """Every room this tool has built before, by its worldfile id.

    `get_by_attribute` is the manager Evennia actually ships; there is no
    module-level attribute search, and a builder that assumed one imported
    cleanly under a stub and failed against a real game.
    """
    return {{room.attributes.get(WB_ID): room
            for room in ObjectDB.objects.get_by_attribute(key=WB_ID)}}


def build(caller=None, data=DATA):
    """Create or update every room and exit in the exported world."""
    def say(message):
        if caller is not None:
            caller.msg(message)
        else:
            print(message)

    with open(data, encoding="utf-8") as handle:
        world = json.load(handle)

    built = _existing()
    made = reused = 0
    for record in world["rooms"]:
        room = built.get(record["id"])
        if room is None:
            room = create_object(ROOM_TYPECLASS, key=record["key"])
            room.attributes.add(WB_ID, record["id"])
            built[record["id"]] = room
            made += 1
        else:
            room.key = record["key"]
            reused += 1
        room.db.desc = record.get("desc") or ""
        for field, value in (("wb_area", record.get("area")),
                             ("wb_latitude", record.get("latitude_deg")),
                             ("wb_longitude", record.get("longitude_deg")),
                             ("wb_elevation_m", record.get("elevation_m")),
                             ("wb_stock", record.get("stock"))):
            if value is not None:
                room.attributes.add(field, value)
        if record.get("area"):
            room.tags.add(record["area"], category="area")
        if record.get("purpose"):
            room.tags.add(record["purpose"], category="wb_purpose")

    exits_made = 0
    for record in world["exits"]:
        source = built.get(record["source"])
        destination = built.get(record["destination"])
        if source is None or destination is None:
            continue
        if any(existing.key == record["name"] and existing.destination == destination
               for existing in source.exits):
            continue
        create_object(EXIT_TYPECLASS, key=record["name"], location=source,
                      destination=destination)
        exits_made += 1

    say("worldbuilder: %s rooms built, %s updated, %s exits made"
        % (made, reused, exits_made))
    return {{"built": made, "updated": reused, "exits": exits_made}}
'''


def flatten(document):
    """
    A worldfile as the builder wants it: one flat list of rooms and one of exits.

    Args:
        document (dict): A worldfile, with `areas` and optionally `roads`.

    Returns:
        world (dict): `rooms`, `exits`, and the `areas` they came from.

    Notes:
        **Roads come too.** They are places with rooms in them, and a world imported
        without them is a set of towns nobody can walk between - which is the one property
        the generator works hardest to guarantee.

        Every room carries the name of the area it belongs to, because Evennia has no
        notion of an area and a tag is how a game gets one back.
    """
    rooms, exits = [], []
    places = list(document.get("areas") or ()) + list(document.get("roads") or ())
    for area in places:
        name = area.get("name")
        purpose = area.get("purpose")
        for room in area.get("rooms") or ():
            rooms.append({
                "id": room["id"],
                "key": room.get("key") or "a room",
                "desc": room.get("desc") or "",
                "area": name,
                "purpose": purpose,
                "latitude_deg": room.get("latitude_deg"),
                "longitude_deg": room.get("longitude_deg"),
                "elevation_m": room.get("elevation_m"),
                "stock": room.get("stock") or None,
            })
        for exit_ in area.get("exits") or ():
            exits.append({"source": exit_["source"], "name": exit_["name"],
                          "destination": exit_["destination"]})
    return {"rooms": rooms, "exits": exits,
            "areas": [{"name": a.get("name"), "display_name": a.get("display_name"),
                       "purpose": a.get("purpose"), "race": a.get("race"),
                       "level_band": a.get("level_band")} for a in places]}


def check(world):
    """
    Everything wrong with a flattened world, before a game is asked to build it.

    Returns:
        problems (list): Readable lines. Empty means it is safe to import.

    Notes:
        **An import that half works is worse than one that refuses.** A duplicate id makes
        two rooms one, and an exit to an id nothing owns makes a door to nowhere - both are
        silent in Evennia and both are found here in a second.
    """
    problems = []
    seen = set()
    for room in world["rooms"]:
        if room["id"] in seen:
            problems.append("room id %s appears twice" % room["id"])
        seen.add(room["id"])
        if not room.get("key"):
            problems.append("room %s has no name" % room["id"])
    for exit_ in world["exits"]:
        if exit_["source"] not in seen:
            problems.append("exit %r leaves room %s, which is not in the file"
                            % (exit_["name"], exit_["source"]))
        if exit_["destination"] not in seen:
            problems.append("exit %r arrives at room %s, which is not in the file"
                            % (exit_["name"], exit_["destination"]))
    return problems


def write(document, directory, name="aetosia"):
    """
    Write the data file and the batchcode that builds it.

    Args:
        document (dict): A worldfile.
        directory (str): Where to write - a game's `world/` directory in practice.
        name (str): The base name for both files.

    Returns:
        report (dict): The two paths, the counts, and any `problems` found.

    Notes:
        Nothing is written when the check fails, because a game asked to build a broken
        world builds most of it and then stops in the middle.
    """
    world = flatten(document)
    problems = check(world)
    if problems:
        return {"written": False, "problems": problems,
                "rooms": len(world["rooms"]), "exits": len(world["exits"])}

    os.makedirs(directory, exist_ok=True)
    data_name = "%s_world.json" % name
    code_name = "build_%s.py" % name
    data_path = os.path.join(directory, data_name)
    code_path = os.path.join(directory, code_name)

    with open(data_path, "w", encoding="utf-8") as handle:
        json.dump(world, handle, separators=(",", ":"))
    with open(code_path, "w", encoding="utf-8") as handle:
        handle.write(BUILDER.format(module="build_%s" % name, data=data_name,
                                    wb_id=WB_ID))
    return {"written": True, "problems": [], "data": data_path, "code": code_path,
            "rooms": len(world["rooms"]), "exits": len(world["exits"]),
            "areas": len(world["areas"]),
            "command": "@py from world.build_%s import build; build(self)" % name}


def main(argv=None):
    """Export a run's worldfile into a game's `world/` directory."""
    import argparse

    parser = argparse.ArgumentParser(description="export a world for Evennia")
    parser.add_argument("--worldfile", required=True)
    parser.add_argument("--into", required=True, help="the game's world/ directory")
    parser.add_argument("--name", default="aetosia")
    args = parser.parse_args(argv)

    with open(args.worldfile, encoding="utf-8") as handle:
        document = json.load(handle)
    report = write(document, args.into, args.name)
    print(json.dumps(report, indent=2))
    return 0 if report["written"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
