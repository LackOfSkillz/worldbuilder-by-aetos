"""
A generated world written two ways for Evennia, and both ways say the same thing.

The batch-command file is parsed here with the same rule Evennia's processor uses - a line
beginning with `#` ends a command - so a room description that broke the batch syntax fails
in this suite rather than halfway through somebody else's import.
"""

import io
import json
import os
import re
import shutil
import tempfile
import unittest
import zipfile

from evennia_roundtrip import batchfile, export

#: Copied from evennia/utils/batchprocessors.py. The processor splits on exactly this.
EVENNIA_CMD_SPLIT = re.compile(r"^\#.*?$", re.MULTILINE)


def a_world():
    """Three rooms, one with a comma in its name, a shop with a keeper, and a door."""
    return {
        "areas": [{
            "name": "testmoor", "display_name": "Testmoor", "purpose": "home",
            "rooms": [
                {"id": 20227, "key": "Water Colonnade, West End",
                 "desc": "Swept marble runs underfoot. A plinth stands against the wall.",
                 "latitude_deg": 1.5, "longitude_deg": 2.5, "elevation_m": 12.0,
                 "fixtures": [{"key": "bronze dial", "desc": "A sun dial.", "kind": "fixture"}]},
                {"id": 202270, "key": "Market Row",
                 "desc": "Carts have worn two ruts in the stone. A tavern door stands open.",
                 "latitude_deg": 1.6, "longitude_deg": 2.5, "elevation_m": 11.0,
                 "people": [{"name": "a carter", "role": "folk"}]},
                {"id": 202271, "key": "the Saltmarket Inn", "interior": True,
                 "desc": "A low taproom with a slate counter. Stairs climb to rooms above.",
                 "stock": ["a pewter tankard", "a loaf of barley bread"],
                 "people": [{"name": "an innkeeper", "role": "keeper"}],
                 "fixtures": [{"key": "hearth", "desc": "A wide hearth.", "kind": "fixture"},
                              {"key": "mirror", "desc": "A tall mirror.", "kind": "mirror"}]},
            ],
            "exits": [
                {"source": 20227, "name": "east", "destination": 202270},
                {"source": 202270, "name": "west", "destination": 20227},
                {"source": 202270, "name": "tavern", "destination": 202271, "door": True},
                {"source": 202271, "name": "tavern", "destination": 202270, "door": True,
                 "leaves": True},
            ],
        }],
        "roads": [],
    }


def parsed(path):
    """The commands Evennia's batch-command processor would run, from a written file."""
    text = io.open(path, encoding="utf-8").read()
    return [c.strip("\r\n") for c in EVENNIA_CMD_SPLIT.split(text) if c.strip("\r\n")]


class TestTheBatchCommandFile(unittest.TestCase):
    def setUp(self):
        self.world = export.flatten(a_world())
        self.lines = list(batchfile.commands(self.world))

    def test_every_room_is_dug_before_any_is_visited(self):
        """An exit needs somewhere to go, so no room may be furnished before all exist."""
        first_tel = next(i for i, line in enumerate(self.lines) if line.startswith("tel "))
        digs = [i for i, line in enumerate(self.lines) if line.startswith("dig ")]
        self.assertEqual(len(digs), 3)
        self.assertLess(max(digs), first_tel)

    def test_a_name_with_a_comma_is_dug_short_and_renamed_whole(self):
        """Evennia's builder commands read a comma as 'another object follows'."""
        self.assertIn("dig Water Colonnade;wb_0020227:typeclasses.rooms.Room", self.lines)
        self.assertIn("py here.key = 'Water Colonnade, West End'", self.lines)
        for line in self.lines:
            if line.startswith("dig "):
                self.assertNotIn(",", line)

    def test_room_handles_are_fixed_width_so_none_is_a_prefix_of_another(self):
        """Evennia's search matches prefixes: wb_20227 would also find wb_202270."""
        handles = [batchfile.room_alias(r["id"]) for r in self.world["rooms"]]
        for one in handles:
            for other in handles:
                if one != other:
                    self.assertFalse(other.startswith(one), (one, other))

    def test_a_persons_handle_does_not_start_with_its_rooms(self):
        """Or every search for the room would find the people standing in it too."""
        room = batchfile.room_alias(202270)
        person = batchfile.person_alias(202270, 0)
        self.assertFalse(person.startswith(room))

    def test_every_exit_is_opened_to_its_destinations_handle(self):
        opens = [line for line in self.lines if line.startswith("open ")]
        self.assertEqual(len(opens), 4)
        self.assertIn("open tavern;out:typeclasses.exits.Exit = wb_0202270", opens)
        self.assertIn("open east:typeclasses.exits.Exit = wb_0202270", opens)

    def test_the_keeper_carries_the_goods(self):
        handle = batchfile.person_alias(202271, 0)
        self.assertIn("set %s/stock = ['a pewter tankard', 'a loaf of barley bread']"
                      % handle, self.lines)
        self.assertIn("set %s/wb_role = 'keeper'" % handle, self.lines)

    def test_a_multiline_description_goes_through_set_on_one_line(self):
        """A keeper's goods are a list; a newline in a batch line reads as a paragraph."""
        keeper_desc = [line for line in self.lines
                       if line.startswith("set wbp_0202271_00/desc")]
        self.assertEqual(len(keeper_desc), 1)
        self.assertIn("\\n  a pewter tankard", keeper_desc[0])

    def test_every_thing_to_look_at_is_made_with_its_own_typeclass(self):
        """Laws F1/F2: the mirror has to be a Mirror, or it cannot reflect anybody."""
        made = [line for line in self.lines if line.startswith("create/drop")
                and "wb_fixtures" in line]
        self.assertIn("create/drop bronze dial;wbt_0020227_00:world.wb_fixtures.Fixture", made)
        self.assertIn("create/drop mirror;wbt_0202271_01:world.wb_fixtures.Mirror", made)
        self.assertIn("set wbt_0202271_01/wb_id = '202271:thing:1'", self.lines)

    def test_handles_for_rooms_people_and_things_never_find_each_other(self):
        room, person, thing = (batchfile.room_alias(202271), batchfile.person_alias(202271, 0),
                               batchfile.thing_alias(202271, 0))
        for one in (room, person, thing):
            for other in (room, person, thing):
                if one != other:
                    self.assertFalse(other.startswith(one), (one, other))

    def test_no_command_names_a_dbref(self):
        """A file handed to another game cannot know its dbrefs; Limbo is not always #2."""
        for line in self.lines:
            self.assertIsNone(re.search(r"#\d", line), line)

    def test_an_exit_name_the_commands_cannot_express_is_refused_not_mangled(self):
        world = a_world()
        world["areas"][0]["exits"][0]["name"] = "east, then north"
        with self.assertRaises(ValueError):
            list(batchfile.commands(export.flatten(world)))

    def test_evennia_reads_back_exactly_the_commands_that_were_written(self):
        """The whole file, parsed the way the processor parses it."""
        where = tempfile.mkdtemp()
        try:
            path = os.path.join(where, "t.ev")
            counts = batchfile.write(self.world, path, "t")
            self.assertEqual(parsed(path), self.lines)
            self.assertEqual(counts["commands"], len(self.lines))
            self.assertEqual(counts["renamed_by_py"], 1)
        finally:
            shutil.rmtree(where)


class TestTheExport(unittest.TestCase):
    def setUp(self):
        self.where = tempfile.mkdtemp()

    def tearDown(self):
        shutil.rmtree(self.where)

    def test_every_format_is_written_with_its_instructions(self):
        report = export.write(a_world(), self.where, "t", bundle=True)
        self.assertTrue(report["written"], report.get("problems"))
        names = sorted(os.listdir(self.where))
        self.assertEqual(names, sorted(["t_world.json", "build_t.py", "batch_t.py", "t.ev",
                                        "wb_fixtures.py", "t_README.md", "t_export.zip"]))
        self.assertEqual(set(report["commands"]), {"direct", "batchcode", "ev"})

    def test_the_readme_does_not_overwrite_the_games_own(self):
        """A stock game already has world/README.md."""
        export.write(a_world(), self.where, "t")
        self.assertNotIn("README.md", os.listdir(self.where))

    def test_the_zip_holds_every_file_it_lists(self):
        report = export.write(a_world(), self.where, "t", bundle=True)
        with zipfile.ZipFile(report["zip"]) as archive:
            inside = set(archive.namelist())
        self.assertEqual(inside, {os.path.basename(p) for p in report["files"]})

    def test_batchcode_brings_the_builder_it_calls(self):
        report = export.write(a_world(), self.where, "t", formats=["batchcode"])
        self.assertIn("build_t.py", os.listdir(self.where))
        self.assertIn("from world.build_t import build",
                      io.open(report["batchcode"], encoding="utf-8").read())

    def test_an_unknown_format_is_refused(self):
        with self.assertRaises(ValueError):
            export.write(a_world(), self.where, "t", formats=["dig"])

    def test_both_builders_give_every_room_the_same_handle(self):
        """The direct builder adds the alias the batch file digs under: one world."""
        export.write(a_world(), self.where, "t")
        code = io.open(os.path.join(self.where, "build_t.py"), encoding="utf-8").read()
        self.assertIn('"wb_%07d" % int(record["id"])', code)
        self.assertIn('"wbp_%07d_%02d"', code)
        self.assertEqual(batchfile.room_alias(20227), "wb_%07d" % 20227)

    def test_the_builder_is_valid_python(self):
        export.write(a_world(), self.where, "t")
        for name in ("build_t.py", "batch_t.py", "wb_fixtures.py"):
            source = io.open(os.path.join(self.where, name), encoding="utf-8").read()
            compile(source, name, "exec")


class TestAdoptingCuratedWares(unittest.TestCase):
    """The curator's goods replace the generator's names on the way out, re-judged."""

    GOOD = [{"name": "a dented pewter tankard", "desc": "A heavy tankard, its lid hinged "
                                                        "and its handle worn smooth."},
            {"name": "a loaf of barley bread", "desc": "A round loaf, dark and dense, baked "
                                                      "that morning in the inn's own oven."}]

    def world(self, shelf):
        world = a_world()
        inn = world["areas"][0]["rooms"][2]
        inn["trade"] = "inn"
        inn["stock_ai"] = shelf
        return world, inn["id"]

    def test_a_shelf_that_passes_replaces_the_names_and_carries_its_notes(self):
        world, inn = self.world(self.GOOD)
        adopted, counts = export.adopt_curated(world)
        room = adopted["areas"][0]["rooms"][2]
        self.assertEqual(room["stock"], ["a dented pewter tankard", "a loaf of barley bread"])
        self.assertIn("hinged", room["stock_notes"]["a dented pewter tankard"])
        self.assertEqual(counts["shelves_curated"], 1)
        flat = {r["id"]: r for r in export.flatten(adopted)["rooms"]}
        self.assertEqual(flat[inn]["stock_notes"], room["stock_notes"])

    def test_a_shelf_that_fails_today_keeps_the_generators_goods(self):
        bad = [dict(self.GOOD[0], name="a dented pewter cup"), self.GOOD[1]]
        world, _inn = self.world(bad)
        adopted, counts = export.adopt_curated(world)
        room = adopted["areas"][0]["rooms"][2]
        self.assertEqual(room["stock"], ["a pewter tankard", "a loaf of barley bread"])
        self.assertNotIn("stock_notes", room)
        self.assertEqual(counts["shelves_template"], 1)
        self.assertEqual(list(counts["shelves_refused"]), ["'a dented pewter cup' no longer "
                                                            "says what it is"])

    def test_the_batch_file_gives_the_keeper_the_notes(self):
        world, _inn = self.world(self.GOOD)
        adopted, _counts = export.adopt_curated(world)
        lines = list(batchfile.commands(export.flatten(adopted)))
        notes = [line for line in lines if "/stock_notes = " in line]
        self.assertEqual(len(notes), 1)
        import ast
        value = ast.literal_eval(notes[0].split(" = ", 1)[1])
        self.assertIn("a dented pewter tankard", value)


if __name__ == "__main__":
    unittest.main()
