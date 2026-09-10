"""
The curator's second job, the goods on the shelves, held to what the generator got right.

"The items have to make some logical sense" - the owner, on the first souvenirs. A dagger
stays a dagger, a keepsake still says where it was made, and nothing invents a name.
"""

import http.server
import io
import json
import os
import re
import shutil
import tempfile
import threading
import unittest

from evennia_roundtrip import curate, wares

PLACE = "Farsteading"
SHELF = ["a dust-red rawhide-hilted dagger in a plain sheath from Farsteading",
         "a dust-red rawhide-hilted short sword from Farsteading",
         "a bundle of arrows from Farsteading"]
GOOD = [{"name": "a dust-red dagger with a rawhide grip, in a plain sheath from Farsteading",
         "desc": "The blade is short and leaf-shaped, the grip bound in rawhide dyed the red "
                 "of the dust roads."},
        {"name": "a rawhide-hilted short sword from Farsteading",
         "desc": "A plain soldier's blade, balanced for the hand, its hilt wrapped tight."},
        {"name": "a sheaf of goose-fletched arrows from Farsteading",
         "desc": "Twelve shafts of ash, fletched grey and tied with red thread."}]


def fixed(index, **change):
    shelf = [dict(item) for item in GOOD]
    shelf[index].update(change)
    return shelf


class TestWhatAWareIs(unittest.TestCase):
    def test_the_head_noun_is_the_thing_not_its_container(self):
        cases = {"a bundle of arrows": "arrows", "a dagger in a plain sheath": "dagger",
                 "a sword blank, unhilted": "blank", "a pair of leather bracers": "bracers",
                 "a pot of wound salve": "salve", "a token cut from bone": "token",
                 "stabling for one horse": "stabling", "an iron cook pot": "pot",
                 "a phial of something the label does not name": "something"}
        for entry, noun in cases.items():
            self.assertEqual(wares.head_noun(entry), noun, entry)

    def test_a_keepsake_is_traced_back_to_its_table_entry(self):
        self.assertEqual(wares.base_ware(SHELF[0], "weaponsmith"), "a dagger in a plain sheath")
        self.assertEqual(wares.base_ware(SHELF[2]), "a bundle of arrows")


class TestTheGate(unittest.TestCase):
    def test_a_good_shelf_passes(self):
        self.assertIsNone(wares.judge(SHELF, GOOD, PLACE, "weaponsmith"))

    def test_a_dagger_that_becomes_something_else_is_refused(self):
        fault = wares.judge(SHELF, fixed(0, name="a rawhide-bound cudgel from Farsteading"),
                            PLACE)
        self.assertIn("no longer says what it is (dagger)", fault)

    def test_arrows_that_become_bolts_are_refused(self):
        self.assertIn("no longer says what it is (arrows)",
                      wares.judge(SHELF, fixed(2, name="a bundle of crossbow bolts from "
                                                       "Farsteading"), PLACE))

    def test_a_keepsake_that_forgets_its_town_is_refused(self):
        fault = wares.judge(SHELF, fixed(1, name="a rawhide-hilted short sword"), PLACE)
        self.assertIn("lost where it came from", fault)

    def test_an_invented_name_is_refused(self):
        fault = wares.judge(SHELF, fixed(1, desc="Forged by Master Aldric of the Ember "
                                                 "Guild, balanced for the hand."), PLACE)
        self.assertIn("invented a name", fault)

    def test_the_place_itself_is_the_one_name_allowed(self):
        shelf = fixed(1, desc="Every smith in Farsteading can make one and none will sell "
                              "one cheap.")
        self.assertIsNone(wares.judge(SHELF, shelf, PLACE))

    def test_a_short_shelf_is_refused(self):
        self.assertIn("2 wares for 3", wares.judge(SHELF, GOOD[:2], PLACE))

    def test_descriptions_are_short_and_never_address_the_reader(self):
        self.assertIn("words", wares.judge(SHELF, fixed(0, desc="Sharp."), PLACE))
        self.assertIn("reader", wares.judge(SHELF, fixed(0, desc="You could shave with "
                                                                 "this well-kept little blade."),
                                            PLACE))

    def test_the_town_colour_goes_on_what_it_makes_never_on_its_oats(self):
        staples = ["a bag of oats", "a brace of rabbits"]
        dyed = [{"name": "a bag of sky-blue oats", "desc": "Rolled oats in a stout sack."},
                {"name": "a brace of rabbits", "desc": "Two rabbits, fresh from the snare."}]
        self.assertIn("dyed", wares.judge(staples, dyed, PLACE, look={"colour": "sky-blue"}))
        made = ["a clay lamp from Farsteading"]
        lamp = [{"name": "a sky-blue clay lamp from Farsteading",
                 "desc": "A small glazed lamp with a pinched spout."}]
        self.assertIsNone(wares.judge(made, lamp, PLACE, look={"colour": "sky-blue"}))

    def test_the_towns_wood_is_never_made_into_what_wood_cannot_be(self):
        """The model sold "a bridle with an olivewood bit" after being told not to."""
        look = {"colour": "sky-blue", "material": "olivewood"}
        bridle = [{"name": "a bridle with an olivewood bit from Farsteading",
                   "desc": "A plain bridle of dark leather, well oiled and supple."}]
        self.assertIn("olivewood cannot make that (olivewood bit)",
                      wares.judge(["a bridle from Farsteading"], bridle, PLACE, look=look))
        grip = [{"name": "a dagger with an olivewood grip from Farsteading",
                 "desc": "A plain leaf blade, its grip worn smooth by an earlier hand."}]
        self.assertIsNone(wares.judge(["a dagger from Farsteading"], grip, PLACE, look=look))

    def test_the_towns_colour_is_never_put_on_bare_metal(self):
        look = {"colour": "sky-blue", "material": "fine leather"}
        blade = [{"name": "a short sword with a sky-blue blade from Farsteading",
                  "desc": "A soldier's sword, balanced for the hand and plainly hilted."}]
        self.assertIn("coloured bare metal",
                      wares.judge(["a short sword from Farsteading"], blade, PLACE, look=look))

    def test_what_the_generator_already_said_is_not_held_against_the_model(self):
        """The generator's own "a sky-blue iron cook pot" is its fault to fix, not a reason
        to throw away the model's whole shelf."""
        look = {"colour": "sky-blue", "material": "oak"}
        pot = [{"name": "a sky-blue iron cook pot from Farsteading",
                "desc": "A squat pot on three short legs, its lid ringed with a lifting loop."}]
        self.assertIsNone(wares.judge(["a sky-blue iron cook pot from Farsteading"], pot,
                                      PLACE, look=look))

    def test_nothing_out_of_period(self):
        fault = wares.judge(SHELF, fixed(1, desc="The pommel is weighted with a lead "
                                                 "bullet to balance the blade."), PLACE)
        self.assertIn("out of period", fault)

    def test_clockwork_is_period_here_as_the_world_says(self):
        """The world's own rule: gnomes are discovering springs and steam."""
        shelf = fixed(1, desc="A clockwork catch in the scabbard throat holds the blade "
                              "until it is drawn.")
        self.assertIsNone(wares.judge(SHELF, shelf, PLACE))

    def test_every_ware_the_generator_sells_passes_the_period_check(self):
        """The generator's own goods must not fail the gate built to protect them."""
        from evennia_roundtrip import period, stock
        for pool in stock.WARES.values():
            for ware in pool:
                self.assertEqual(period.offences(ware), [], ware)


class Backend(http.server.BaseHTTPRequestHandler):
    """Answers a room as a room and a shelf as a shelf, as a real model would."""

    def log_message(self, *_args):
        pass

    def _send(self, value):
        body = json.dumps(value).encode("utf-8")
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        self._send({"data": [{"id": "fake"}]})

    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers["content-length"])))
        system, user = (m["content"] for m in request["messages"])
        if "goods for sale" in system:
            # Each line is the ware, then " - " and what to do with it.
            items = [line.split(" - ")[0]
                     for line in re.findall(r"^\d+\. (.+)$", user, re.M)]
            reply = {"wares": [{"name": item.replace("a bowl of", "a deep bowl of"),
                                "desc": "Made the way it has always been made here, and "
                                        "none the worse for it."} for item in items]}
        else:
            reply = {"name": "", "desc": ("Worn flagstones run between low walls of fitted "
                                          "grey stone. Moss has taken every joint. The wall "
                                          "is older than the houses built against it.")}
        self._send({"choices": [{"message": {"content": json.dumps(reply)}}]})


class TestShelvesEndToEnd(unittest.TestCase):
    def setUp(self):
        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Backend)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.url = "http://127.0.0.1:%d/v1" % self.server.server_address[1]
        self.run = tempfile.mkdtemp()
        world = {"areas": [{"name": "millby", "display_name": "Millby", "rooms": [
            {"id": 1, "key": "Mill Lane", "desc": "A lane.", "cell": [0, 0, 0]},
            {"id": 2, "key": "the Mill Inn", "desc": "An inn.", "interior": True,
             "trade": "inn", "stock": ["a bowl of barley broth", "brown bread"]}],
            "exits": []}], "roads": []}
        with io.open(os.path.join(self.run, "worldfile.json"), "w", encoding="utf-8") as h:
            json.dump(world, h)

    def tearDown(self):
        self.server.shutdown()
        shutil.rmtree(self.run)

    def curated(self):
        with io.open(os.path.join(self.run, "curate", "curated.json"), encoding="utf-8") as h:
            return json.load(h)

    def test_a_shelf_is_curated_after_the_rooms_and_kept_beside_the_original(self):
        self.assertEqual(curate.main(["--run", self.run, "--base-url", self.url,
                                      "--model", "fake", "--quiet"]), 0)
        inn = self.curated()["areas"][0]["rooms"][1]
        self.assertEqual(inn["stock"], ["a bowl of barley broth", "brown bread"])
        self.assertEqual([w["name"] for w in inn["stock_ai"]],
                         ["a deep bowl of barley broth", "brown bread"])

    def test_a_run_curated_before_wares_existed_gains_them_without_redoing_rooms(self):
        curate.main(["--run", self.run, "--base-url", self.url, "--model", "fake", "--quiet",
                     "--no-wares"])
        jobs_path = os.path.join(self.run, "curate", "job.json")
        with io.open(jobs_path, encoding="utf-8") as h:
            stored = json.load(h)
        stored["jobs"] = [j for j in stored["jobs"] if j.get("what") != "wares"]
        with io.open(jobs_path, "w", encoding="utf-8") as h:
            json.dump(stored, h)
        curate.main(["--run", self.run, "--base-url", self.url, "--model", "fake", "--quiet"])
        with io.open(os.path.join(self.run, "curate", "status.json"), encoding="utf-8") as h:
            status = json.load(h)
        self.assertEqual((status["done"], status["total"]), (3, 3))
        self.assertEqual(status["asked"], 1, "only the shelf was asked about")
        self.assertTrue(self.curated()["areas"][0]["rooms"][1].get("stock_ai"))


if __name__ == "__main__":
    unittest.main()
