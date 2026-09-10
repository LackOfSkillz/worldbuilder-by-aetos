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
#: Things to look at (laws F1 and F2), from wb_fixtures.py beside this file. A fixture cannot
#: be picked up; the mirror shows whoever looks into it; the clock reads the game's time.
THING_TYPECLASS = {{"fixture": "world.wb_fixtures.Fixture",
                   "landmark": "world.wb_fixtures.Fixture",
                   "mirror": "world.wb_fixtures.Mirror",
                   "clock": "world.wb_fixtures.Clock"}}
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
        # The same handle the batch-command file digs every room under, so a world built
        # either way answers to `tel wb_0020227`, and the two formats build one world.
        room.aliases.add("wb_%07d" % int(record["id"]))
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
                # Home is their own room, as `create/drop` makes it in the batch-command
                # file: a keeper sent home should go back behind the counter, not to Limbo.
                body = create_object(FOLK_TYPECLASS, key=person["name"], location=room,
                                     home=room)
                body.attributes.add(WB_ID, mark)
                built[mark] = body
                folk += 1
            body.key = person["name"]
            body.aliases.add("wbp_%07d_%02d" % (int(record["id"]), place))
            body.attributes.add("wb_role", person.get("role") or "folk")
            wares = wares_of.get(record["id"]) if person.get("role") == "keeper" else None
            if wares:
                body.db.stock = wares
                # What each ware looks like, name to description, when the curator wrote it.
                if record.get("stock_notes"):
                    body.db.stock_notes = dict(record["stock_notes"])
                body.db.desc = ("Goods for sale:"
                                + "".join(chr(10) + "  " + ware for ware in wares))
                counters += 1
            elif not body.db.desc:
                body.db.desc = "One of the people of this place."

    # **Things to look at** - laws F1 and F2. Keyed like the people, by the room and their
    # place in it, so a second import moves nothing and doubles nothing.
    things = 0
    for record in world["rooms"]:
        room = built.get(record["id"])
        if room is None:
            continue
        for place, thing in enumerate(record.get("fixtures") or ()):
            mark = "%s:thing:%s" % (record["id"], place)
            body = built.get(mark)
            if body is None:
                body = create_object(THING_TYPECLASS.get(thing.get("kind"),
                                                         THING_TYPECLASS["fixture"]),
                                     key=thing["key"], location=room, home=room)
                body.attributes.add(WB_ID, mark)
                built[mark] = body
                things += 1
            body.key = thing["key"]
            body.aliases.add("wbt_%07d_%02d" % (int(record["id"]), place))
            body.db.desc = thing.get("desc") or ""

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
        "%s people, %s things to look at, %s counters retired"
        % (made, reused, exits_made, counters, folk, things, retired))
    return {{"built": made, "updated": reused, "exits": exits_made,
            "shops": counters, "people": folk, "things": things, "retired": retired}}
'''


#: The typeclasses the things to look at are built as. Shipped as `world/wb_fixtures.py`.
#:
#: **Behaviour, not just words.** "A mirror that shows the character in the mirror" was the
#: requirement, and a mirror that is only a description cannot do it. So the mirror adds the
#: looker's own reflection to what it says, and the clock tells the game's time in words.
FIXTURES_MODULE = '''"""Things to look at in a generated world (area-building laws F1 and F2).

Fixture - cannot be picked up.
Mirror  - a fixture that shows whoever looks into it.
Clock   - a fixture that tells the game's time.

Written by the world exporter. Nothing here depends on it.
"""

import datetime

from evennia import DefaultObject
from evennia.utils import gametime

_HOURS = ("twelve", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
          "ten", "eleven")


def clock_words(hour, minute):
    """"half past seven", "quarter to nine", "a little after three"."""
    minute = int(round(minute / 5.0)) * 5
    if minute == 60:
        hour, minute = hour + 1, 0
    this, next_ = _HOURS[hour % 12], _HOURS[(hour + 1) % 12]
    if minute == 0:
        return "%s o'clock" % this
    if minute == 15:
        return "a quarter past %s" % this
    if minute == 30:
        return "half past %s" % this
    if minute == 45:
        return "a quarter to %s" % next_
    if minute < 30:
        return "%d minutes past %s" % (minute, this)
    return "%d minutes to %s" % (60 - minute, next_)


class Fixture(DefaultObject):
    """Something fixed in a room, to be looked at and left where it is."""

    def at_object_creation(self):
        super().at_object_creation()
        self.locks.add("get:false()")
        self.db.get_err_msg = "It is fixed where it stands."


class Mirror(Fixture):
    """A mirror that shows whoever looks into it."""

    def return_appearance(self, looker, **kwargs):
        text = super().return_appearance(looker, **kwargs)
        if looker is None:
            return text
        face = looker.get_display_name(looker)
        own = (looker.db.desc or "").strip()
        reflection = "In the glass, %s looks back." % face
        if own:
            reflection += " " + own
        return "%s\\n%s" % (text, reflection)


class Clock(Fixture):
    """A clock that tells the game's time."""

    def return_appearance(self, looker, **kwargs):
        text = super().return_appearance(looker, **kwargs)
        try:
            now = datetime.datetime.fromtimestamp(gametime.gametime(absolute=True))
        except Exception:
            return text
        return "%s\\nThe hands stand at %s." % (text, clock_words(now.hour, now.minute))
'''


#: The batch-code wrapper: the direct builder, reachable from Evennia's `batchcode` command.
#:
#: **One builder, two doors.** Some games run world scripts with `@py`, some with the batch-
#: code processor, and a dev receiving this should not have to translate between them. This
#: adds no logic of its own - it calls the same `build` - so the two cannot drift apart.
BATCHCODE = '''"""Build this world with Evennia's batch-code processor.

    batchcode batch_{name}

The same builder as `@py from world.build_{name} import build; build(self)`, run through the
batch-code processor instead. Safe to run again: rooms are found by their wb_id and updated,
never doubled.
"""

#HEADER

from world.build_{name} import build

#CODE

build(caller)
'''

#: Instructions for whoever receives the files. Written for another Evennia developer, who
#: has none of this project's context and should not need any.
README = """# {name} - a generated world for Evennia

{rooms} rooms, {exits} exits and {people} people across {areas} areas and roads.

## Two ways to build it

| | `{name}.ev` - batch commands | `build_{name}.py` - direct |
|---|---|---|
| How | ordinary builder commands, one at a time | writes the database directly |
| Run | `batchcommands {name}` | `batchcode batch_{name}` or `@py from world.build_{name} import build; build(self)` |
| Speed | slow - {commands} commands, each echoed | fast |
| Run twice | **builds a second world** | updates in place; never doubles |
| Readable | yes - open it and see every command | the data is `{name}_world.json` |

**Use the direct builder** unless you specifically want to read or edit the commands before
they run. The batch-command file is there for the dev who wants to see exactly what goes into
their game; it builds the same world.

## Install

Copy these files into your game's `world/` folder:

{files}

## Requirements

- A stock Evennia game (built and tested on Evennia 6.1). The files use the default
  typeclasses: `typeclasses.rooms.Room`, `typeclasses.exits.Exit` and
  `typeclasses.characters.Character` for the people. A game with its own typeclasses changes
  the constants at the top of `build_{name}.py`, or the paths in `{name}.ev`.
- Permission, which differs by door:
  - `batchcommands {name}` - **Developer** is enough.
  - `@py from world.build_{name} import build; build(self)` - **Developer** is enough.
  - `batchcode batch_{name}` - **superuser only**. Evennia locks `batchcode` to superusers
    because it runs arbitrary Python; a Developer does not even see the command. Use `@py`
    instead if you are not the superuser - it runs the same builder.
- Nothing from the generator. These files are the whole world.

## What it builds

- Every room, with its description; tagged with its area (category `area`) and purpose
  (category `wb_purpose`), and carrying `wb_id` plus its latitude, longitude and elevation.
- Every exit. Shops are rooms entered by a word (`go tavern`), left by the same word or `out`.
- The people: a keeper in every shop, holding what it sells as `stock`; townsfolk and quarry.
- An alias on every room, `wb_` and its id: `tel wb_{sample}` goes straight there.
- Things to look at (area-building laws F1 and F2): one to three in every town room, fitted
  to the room and its people, and landmarks and fixtures along the roads. They are objects,
  built from `wb_fixtures.py` - which must be in `world/` too. A fixture cannot be picked up;
  the **mirror** shows whoever looks into it; the **clock** tells the game's time.

## What it does not do

- **Join to your existing rooms.** It builds a separate world. Link it with `open` from
  wherever you want players to arrive.
- **Set locks beyond Evennia's defaults.** Anything built by `batchcommands` is owned by the
  builder who ran it, as dug rooms always are; the direct builder uses the typeclass defaults.

## Removing it

Everything this builds carries `wb_id`. To take it all out again:

    py from evennia.objects.models import ObjectDB; [o.delete() for o in list(ObjectDB.objects.get_by_attribute(key="wb_id"))]

Deleting a room deletes the exits in and out of it.
"""


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
                "stock_notes": room.get("stock_notes") or None,
                "people": room.get("people") or None,
                "fixtures": room.get("fixtures") or None,
                "landmark": room.get("landmark") or None,
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
    counts = {"curated": 0, "template": 0, "renamed": 0, "refused": {},
              "shelves_curated": 0, "shelves_template": 0, "shelves_refused": {}}
    for place in list(adopted.get("areas") or ()) + list(adopted.get("roads") or ()):
        for room in place.get("rooms") or ():
            _adopt_wares(place, room, counts)
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


def _adopt_wares(place, room, counts):
    """
    One shop's curated goods in place of the generator's, if they still pass the gate.

    Notes:
        **The names replace `stock`; the descriptions travel beside it.** `stock` stays a
        list of plain names because everything that reads it - the keeper, the maritime
        client's trade marker - expects names. What each ware looks like goes in
        `stock_notes`, name to description, for a game that wants to show it.

        Re-judged here like the room text, and all or nothing: a shelf half the model's and
        half the generator's could sell the same dagger twice under two names.
    """
    from evennia_roundtrip import wares

    reworked = room.get("stock_ai")
    if not room.get("stock"):
        return
    if not reworked:
        counts["shelves_template"] += 1
        return
    name = place.get("display_name") or place.get("name") or ""
    fault = wares.judge(room["stock"], reworked, name, room.get("trade"), place.get("look"))
    if fault:
        counts["shelves_template"] += 1
        # The fault names the ware; the tally wants the kind of fault.
        kind = fault.split(" (")[0] if "(" in fault else fault
        counts["shelves_refused"][kind] = counts["shelves_refused"].get(kind, 0) + 1
        return
    room["stock"] = [ware["name"].strip() for ware in reworked]
    room["stock_notes"] = {ware["name"].strip(): ware["desc"].strip() for ware in reworked}
    counts["shelves_curated"] += 1


#: Every output this can write. `direct` is the data file and its builder; `batchcode` wraps
#: that builder for Evennia's batch-code processor; `ev` is the batch-command file.
FORMATS = ("direct", "batchcode", "ev")


def write(document, directory, name="aetosia", curated=False, formats=FORMATS, bundle=False):
    """
    Write the world in every format asked for, with instructions, and optionally a zip.

    Args:
        document (dict): A worldfile.
        directory (str): Where to write - a game's `world/` directory, or a folder to hand on.
        name (str): The base name for every file.
        curated (bool): Take the curator's text where it passes the laws (`adopt_curated`).
        formats (iterable): Any of `FORMATS`. `batchcode` brings `direct` with it, since it
            calls the same builder.
        bundle (bool): Also write `<name>_export.zip` holding every file, to hand to someone.

    Returns:
        report (dict): Paths, counts, and any `problems` found.

    Notes:
        Nothing is written when the check fails, because a game asked to build a broken
        world builds most of it and then stops in the middle.
    """
    formats = set(formats)
    unknown = formats - set(FORMATS)
    if unknown:
        raise ValueError("unknown format(s): %s; choose from %s"
                         % (", ".join(sorted(unknown)), ", ".join(FORMATS)))
    if "batchcode" in formats:
        formats.add("direct")
    adoption = None
    if curated:
        document, adoption = adopt_curated(document)
    world = flatten(document)
    problems = check(world)
    if problems:
        return {"written": False, "problems": problems,
                "rooms": len(world["rooms"]), "exits": len(world["exits"])}

    from evennia_roundtrip import batchfile

    os.makedirs(directory, exist_ok=True)
    written = []

    def put(filename, text):
        path = os.path.join(directory, filename)
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(text)
        written.append(path)
        return path

    report = {"written": True, "problems": [], "files": written,
              "rooms": len(world["rooms"]), "exits": len(world["exits"]),
              "areas": len(world["areas"]), "commands": {}}

    # The typeclasses the things to look at are built as, needed by both formats. One module
    # for every world exported into a game, so it is named for what it is, not for a world.
    report["fixtures"] = put("wb_fixtures.py", FIXTURES_MODULE)

    if "direct" in formats:
        data_name = "%s_world.json" % name
        report["data"] = put(data_name, json.dumps(world, separators=(",", ":")))
        report["code"] = put("build_%s.py" % name,
                             BUILDER.format(module="build_%s" % name, data=data_name,
                                            wb_id=WB_ID))
        report["commands"]["direct"] = "@py from world.build_%s import build; build(self)" % name
        report["command"] = report["commands"]["direct"]
    if "batchcode" in formats:
        report["batchcode"] = put("batch_%s.py" % name, BATCHCODE.format(name=name))
        report["commands"]["batchcode"] = "batchcode batch_%s" % name
    ev_counts = {}
    if "ev" in formats:
        ev_path = os.path.join(directory, "%s.ev" % name)
        ev_counts = batchfile.write(world, ev_path, name)
        written.append(ev_path)
        report["ev"] = ev_path
        report["ev_commands"] = ev_counts["commands"]
        report["ev_renamed_by_py"] = ev_counts["renamed_by_py"]
        report["commands"]["ev"] = "batchcommands %s" % name

    people = sum(len(r.get("people") or ()) for r in world["rooms"])
    sample = world["rooms"][0]["id"] if world["rooms"] else 0
    report["readme"] = put("%s_README.md" % name, README.format(
        name=name, rooms=len(world["rooms"]), exits=len(world["exits"]), people=people,
        areas=len(world["areas"]), commands=ev_counts.get("commands", "tens of thousands of"),
        sample="%07d" % int(sample),
        files="\n".join("- `%s`" % os.path.basename(p) for p in written)))

    if bundle:
        import zipfile
        zip_path = os.path.join(directory, "%s_export.zip" % name)
        with zipfile.ZipFile(zip_path, "w", zipfile.ZIP_DEFLATED) as archive:
            for path in written:
                archive.write(path, os.path.basename(path))
        report["zip"] = zip_path

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
    parser.add_argument("--formats", default=",".join(FORMATS),
                        help="comma-separated, from: %s (default: all)" % ", ".join(FORMATS))
    parser.add_argument("--zip", action="store_true",
                        help="also write <name>_export.zip holding every file, to hand on")
    args = parser.parse_args(argv)

    with open(args.worldfile, encoding="utf-8") as handle:
        document = json.load(handle)
    report = write(document, args.into, args.name, curated=args.curated,
                   formats=[f.strip() for f in args.formats.split(",") if f.strip()],
                   bundle=args.zip)
    print(json.dumps(report, indent=2))
    return 0 if report["written"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
