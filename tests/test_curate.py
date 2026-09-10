"""
The curator: what it keeps, what it refuses, where it asks, and what it leaves on disk.

It had no tests of its own until it was about to be driven by a button. The end-to-end cases
run the real command against a small fake backend that speaks the same protocol as vLLM and
LM Studio, on this machine, so they need no model and no network.
"""

import http.server
import io
import json
import os
import shutil
import socket
import tempfile
import threading
import unittest

from evennia_roundtrip import curate

GOOD = ("Worn flagstones run between low walls of fitted grey stone, and a trough cut from "
        "one block stands by the gate. Moss has taken the north side of every joint. "
        "The wall is older than the houses built against it.")


class FakeBackend(http.server.BaseHTTPRequestHandler):
    """Speaks just enough of the OpenAI chat protocol: a model list and a completion."""
    replies = []

    def log_message(self, *_args):
        pass

    def _send(self, value, status=200):
        body = json.dumps(value).encode("utf-8")
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        self._send({"data": [{"id": "fake-model"}]})

    def do_POST(self):
        length = int(self.headers.get("content-length", 0))
        self.rfile.read(length)
        text = self.replies.pop(0) if self.replies else json.dumps(
            {"name": "", "desc": GOOD})
        self._send({"choices": [{"message": {"content": text}}]})


def serve(handler=FakeBackend):
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server, "http://127.0.0.1:%d/v1" % server.server_address[1]


def closed_url():
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        port = probe.getsockname()[1]
    return "http://127.0.0.1:%d/v1" % port


def a_world():
    rooms = [{"id": n, "key": "Mill Lane", "desc": "A lane. A wall.", "cell": [n, 0, 0],
              "elevation_m": 10.0} for n in (1, 2, 3)]
    road = [{"id": n, "key": "Mill-Kiln Road", "desc": "A road. A verge.", "cell": [n, 0, 0],
             "elevation_m": 20.0} for n in (50, 51)]
    return {"areas": [{"name": "millby", "rooms": rooms, "exits": []}],
            "roads": [{"name": "the-road", "rooms": road, "exits": []}]}


class TestJudging(unittest.TestCase):
    def test_a_description_inside_every_band_passes(self):
        self.assertIsNone(curate.judge(GOOD))

    def test_the_bands_and_the_reader_are_enforced(self):
        self.assertIn("thin", curate.judge("A lane. A wall."))
        self.assertEqual(curate.judge(GOOD + " A gate stands open. A path runs on."),
                         "5 sentences")
        self.assertEqual(curate.judge(GOOD.replace("Moss", "You see moss")), "second person")
        self.assertEqual(curate.judge(GOOD.replace("stands by", "beckons by")),
                         "addresses the reader")

    def test_a_door_word_must_be_there_as_a_whole_word(self):
        """"inn" is not satisfied by "inner"; the player types the whole word."""
        inner = GOOD.replace("low walls", "inner walls")
        self.assertEqual(curate.judge(inner, must_name=["inn"]), "door not named (inn)")
        self.assertIsNone(curate.judge(GOOD.replace("the gate", "the inn"), must_name=["inn"]))

    def test_json_is_found_inside_a_fence_or_a_sentence(self):
        self.assertEqual(curate.unwrap('```json\n{"desc": "x"}\n```')["desc"], "x")
        self.assertEqual(curate.unwrap('Here you go: {"desc": "y"} done')["desc"], "y")
        self.assertIsNone(curate.unwrap("no json here"))


class TestTheWorkList(unittest.TestCase):
    def test_roads_come_after_the_areas_and_say_they_are_roads(self):
        jobs = curate.work_list(a_world())
        self.assertEqual([j.get("kind", "areas") for j in jobs],
                         ["areas"] * 3 + ["roads"] * 2)

    def test_a_road_room_and_an_area_room_with_one_index_are_different_rooms(self):
        self.assertNotEqual(curate.token({"area": 0, "room": 1}),
                            curate.token({"kind": "roads", "area": 0, "room": 1}))

    def test_an_old_journal_record_still_means_what_it_meant(self):
        self.assertEqual(curate.token({"area": 3, "room": 7}), "3/7")


class TestReachingTheModel(unittest.TestCase):
    def setUp(self):
        self.server, self.url = serve()

    def tearDown(self):
        self.server.shutdown()

    def test_away_from_home_the_second_address_is_used(self):
        model = curate.Model([closed_url(), self.url], "fake-model")
        self.assertEqual(model.pick(), self.url)
        self.assertEqual(model.base_url, self.url)

    def test_losing_the_address_in_use_mid_run_moves_to_one_that_answers(self):
        model = curate.Model([closed_url(), self.url], "fake-model")
        model.current = 0  # as if the house LAN had been answering until a moment ago
        self.assertIn("desc", model.ask("s", "u"))
        self.assertEqual(model.base_url, self.url)

    def test_nothing_answering_is_none_not_a_hang(self):
        self.assertIsNone(curate.Model([closed_url(), closed_url()], "m").pick())

    def test_a_refusal_from_the_model_is_one_rooms_problem(self):
        class Refuses(FakeBackend):
            def do_POST(self):
                self.rfile.read(int(self.headers.get("content-length", 0)))
                self._send({"error": "bad"}, status=400)
        server, url = serve(Refuses)
        try:
            with self.assertRaises(ValueError):
                curate.Model(url, "m").ask("s", "u")
        finally:
            server.shutdown()


class TestARunEndToEnd(unittest.TestCase):
    def setUp(self):
        self.server, self.url = serve()
        self.run = tempfile.mkdtemp()
        with io.open(os.path.join(self.run, "worldfile.json"), "w", encoding="utf-8") as h:
            json.dump(a_world(), h)

    def tearDown(self):
        self.server.shutdown()
        shutil.rmtree(self.run)

    def curate(self, *extra):
        return curate.main(["--run", self.run, "--base-url", "%s,%s" % (closed_url(), self.url),
                            "--model", "fake-model", "--quiet", "--at-once", "2"] + list(extra))

    def read(self, name):
        with io.open(os.path.join(self.run, "curate", name), encoding="utf-8") as handle:
            return json.load(handle)

    def test_areas_and_roads_are_curated_and_the_status_ends_done(self):
        self.assertEqual(self.curate(), 0)
        status = self.read("status.json")
        self.assertEqual(status["state"], "done")
        self.assertEqual((status["done"], status["total"]), (5, 5))
        self.assertEqual(status["url"], self.url, "the working address, not the dead one")
        world = self.read("curated.json")
        self.assertTrue(all(r.get("desc_ai") for r in world["roads"][0]["rooms"]))
        self.assertTrue(all(r.get("desc_ai") for r in world["areas"][0]["rooms"]))

    def test_a_second_run_asks_nothing_and_says_the_same(self):
        self.curate()
        FakeBackend.replies = ["not json at all"] * 10  # any question now would be refused
        self.assertEqual(self.curate(), 0)
        status = self.read("status.json")
        self.assertEqual(status["done"], 5)
        # The run's totals, not this process's: a resumed run once said "kept 21 of 209".
        self.assertEqual(status["kept"], 5)
        FakeBackend.replies = []

    def test_a_run_over_part_of_the_world_still_writes_all_of_it(self):
        """`--areas 1` once wrote a curated.json holding one area's curation and none of the
        rest, though the journal still had every answer."""
        self.curate()
        self.curate("--areas", "1")  # the slice leaves the road out
        world = self.read("curated.json")
        self.assertTrue(all(r.get("desc_ai") for r in world["roads"][0]["rooms"]))

    def test_no_address_answering_stops_cleanly_and_says_why(self):
        code = curate.main(["--run", self.run, "--base-url", closed_url(), "--quiet"])
        self.assertEqual(code, 2)
        status = self.read("status.json")
        self.assertEqual(status["state"], "stopped")
        self.assertIn("no address answered", status["stopped"])


if __name__ == "__main__":
    unittest.main()
