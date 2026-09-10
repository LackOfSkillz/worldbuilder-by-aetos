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
WARES_TYPECLASS = "typeclasses.objects.Object"
#: NPCs are characters, because a shopkeeper a player can talk to is a character and not a
#: prop. A game with its own NPC typeclass changes this one line.
FOLK_TYPECLASS = "typeclasses.characters.Character"
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

    # **The wares belong to the keeper, because a shop is a person with goods.**
    #
    # They were first written onto a counter standing in the room, which put them where a
    # player could read them and nowhere they could be asked for. Clicking the shopkeeper
    # and being shown what they sell is the whole of the interaction, and it wants one
    # object holding both the goods and the conversation rather than two holding half each.
    #
    # The maritime client's land map marks a room as trade by looking through its contents
    # for anything carrying `stock`, so the keeper lights the map up as the counter did.
    wares_of = {{}}
    for record in world["rooms"]:
        if record.get("stock"):
            wares_of[record["id"]] = list(record["stock"])

    # **The people the generator placed.** Keyed by the room and their place in it, so a
    # second import moves nobody and doubles nobody.
    folk = 0
    counters = 0
    for record in world["rooms"]:
        room = built.get(record["id"])
        if room is None:
            continue
        for place, person in enumerate(record.get("people") or ()):
            mark = "%s:folk:%s" % (record["id"], place)
            body = built.get(mark)
            if body is None:
                body = create_object(FOLK_TYPECLASS, key=person["name"], location=room)
                body.attributes.add(WB_ID, mark)
                built[mark] = body
                folk += 1
            body.key = person["name"]
            body.attributes.add("wb_role", person.get("role") or "folk")
            wares = wares_of.get(record["id"]) if person.get("role") == "keeper" else None
            if wares:
                body.db.stock = wares
                body.db.desc = ("Goods for sale:"
                                + "".join(chr(10) + "  " + ware for ware in wares))
                counters += 1
            elif not body.db.desc:
                body.db.desc = "One of the people of this place."

    # A counter built by an earlier version of this file has nothing to do now that the
    # keeper carries the goods. Left standing it is a prop that duplicates a person.
    retired = 0
    for mark, thing in list(built.items()):
        if isinstance(mark, str) and mark.endswith(":wares"):
            thing.delete()
            del built[mark]
            retired += 1

    exits_made = 0
    for record in world["exits"]:
        source = built.get(record["source"])
        destination = built.get(record["destination"])
        if source is None or destination is None:
            continue
        if any(existing.key == record["name"] and existing.destination == destination
               for existing in source.exits):
            continue
        made_exit = create_object(EXIT_TYPECLASS, key=record["name"], location=source,
                                  destination=destination)
        if record.get("aliases"):
            made_exit.aliases.add(record["aliases"])
        exits_made += 1

    say("worldbuilder: %s rooms built, %s updated, %s exits made, %s keepers stocked, "
        "%s people, %s counters retired"
        % (made, reused, exits_made, counters, folk, retired))
    return {{"built": made, "updated": reused, "exits": exits_made,
            "shops": counters, "people": folk, "retired": retired}}
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
                "people": room.get("people") or None,
            })
        for exit_ in area.get("exits") or ():
            exits.append({"source": exit_["source"], "name": exit_["name"],
                          "destination": exit_["destination"],
                          # A door is entered and left by its own noun (law T1a); `out` is
                          # an alias on the way out only, because `out` from a street means
                          # nothing.
                          "aliases": ["out"] if exit_.get("leaves") else None})
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


def adopt_curated(document):
    """
    A copy of a worldfile with the curator's text taken wherever it is safe to take.

    Args:
        document (dict): A worldfile the curator has been over (`desc_ai`, `key_ai`).

    Returns:
        adopted (dict): The copy, with `desc` and interior `key` replaced.
        counts (dict): How many rooms took the new text and how many kept the old, and why.

    Notes:
        **Room by room, and re-checked here rather than trusted.** The curator gates what it
        keeps, but its gates have changed as its faults were found - a journal written before
        the door rule existed holds text that passed every check then and hides a shop now.
        So every description is judged again at the moment it is adopted, against today's
        rules, and a room that fails keeps the template's text. Shipping a street that does
        not name its door costs a player a shop; keeping the older prose costs nothing.

        **Only interiors take a new name.** A street room's name is shared by its whole
        street and must stay a connected run (law G1); a shop's name is its own.
    """
    from evennia_roundtrip import curate

    adopted = json.loads(json.dumps(document))
    counts = {"curated": 0, "template": 0, "renamed": 0, "refused": {}}
    for place in list(adopted.get("areas") or ()) + list(adopted.get("roads") or ()):
        for room in place.get("rooms") or ():
            text = room.get("desc_ai")
            if not text:
                counts["template"] += 1
                continue
            nouns = [noun for noun, _what in curate.doors_of(place, room)]
            fault = curate.judge(text, must_name=nouns)
            if fault:
                counts["template"] += 1
                counts["refused"][fault] = counts["refused"].get(fault, 0) + 1
                continue
            room["desc"] = text
            counts["curated"] += 1
            if room.get("interior") and (room.get("key_ai") or "").strip():
                if room["key_ai"].strip() != room.get("key"):
                    counts["renamed"] += 1
                room["key"] = room["key_ai"].strip()
    return adopted, counts


def write(document, directory, name="aetosia", curated=False):
    """
    Write the data file and the batchcode that builds it.

    Args:
        document (dict): A worldfile.
        directory (str): Where to write - a game's `world/` directory in practice.
        name (str): The base name for both files.
        curated (bool): Take the curator's text where it passes the laws (`adopt_curated`).

    Returns:
        report (dict): The two paths, the counts, and any `problems` found.

    Notes:
        Nothing is written when the check fails, because a game asked to build a broken
        world builds most of it and then stops in the middle.
    """
    adoption = None
    if curated:
        document, adoption = adopt_curated(document)
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
    report = {"written": True, "problems": [], "data": data_path, "code": code_path,
              "rooms": len(world["rooms"]), "exits": len(world["exits"]),
              "areas": len(world["areas"]),
              "command": "@py from world.build_%s import build; build(self)" % name}
    if adoption is not None:
        report["curated"] = adoption
    return report


def main(argv=None):
    """Export a run's worldfile into a game's `world/` directory."""
    import argparse

    parser = argparse.ArgumentParser(description="export a world for Evennia")
    parser.add_argument("--worldfile", required=True)
    parser.add_argument("--into", required=True, help="the game's world/ directory")
    parser.add_argument("--name", default="aetosia")
    parser.add_argument("--curated", action="store_true",
                        help="take the curator's descriptions (desc_ai) where they pass the "
                             "laws; any room that fails keeps its template text")
    args = parser.parse_args(argv)

    with open(args.worldfile, encoding="utf-8") as handle:
        document = json.load(handle)
    report = write(document, args.into, args.name, curated=args.curated)
    print(json.dumps(report, indent=2))
    return 0 if report["written"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
