"""
Laws F1 and F2: things to look at, in every town room and along every road.

Gary's decision, 2026-09-10. A SHOULD in the area-building laws, so hand-built zones are
warned; a MUST for a generated town, because a generator that can furnish every room has no
excuse not to.
"""

import random
import unittest

from evennia_roundtrip import areagen, fixtures, naming


def a_town(size=60, seed=3, race="human"):
    rng = random.Random(seed)
    built, _problems = areagen.build_lattice(size, rng)
    rooms, exits = areagen.rooms_and_exits(built, ["x"], 1000)
    for room in rooms:
        room.update(latitude_deg=0.0, longitude_deg=0.0, elevation_m=5.0)
    area = {"rooms": rooms, "exits": exits}
    naming.name_and_describe(area, race, rng)
    fixtures.furnish_town(area, race, rng)
    return area


class TestF1(unittest.TestCase):
    def test_every_town_room_has_one_to_three_things_to_look_at(self):
        for seed in range(8):
            for race in ("human", "dwarf", "elf", "goblin"):
                area = a_town(seed=seed, race=race)
                self.assertEqual(fixtures.unfurnished(area), [], (seed, race))

    def test_no_room_has_the_same_thing_twice(self):
        for room in a_town()["rooms"]:
            keys = [thing["key"] for thing in room["fixtures"]]
            self.assertEqual(len(keys), len(set(keys)), keys)

    def test_a_dwarf_hold_is_furnished_like_a_dwarf_hold(self):
        dwarf_things = {key for key, _desc in fixtures.STREET["dwarf"]}
        streets = [r for r in a_town(race="dwarf")["rooms"] if not r.get("interior")]
        for room in streets:
            for thing in room["fixtures"]:
                self.assertIn(thing["key"], dwarf_things)

    def test_a_shop_is_furnished_for_its_trade(self):
        inns = [r for r in a_town(size=120)["rooms"] if r.get("trade") == "inn"]
        self.assertTrue(inns, "the fixture town needs an inn")
        allowed = {k for k, _ in fixtures.INTERIOR["inn"]} | {"mirror", "clock"}
        for inn in inns:
            for thing in inn["fixtures"]:
                self.assertIn(thing["key"], allowed)

    def test_the_mirror_and_clock_say_what_they_are(self):
        """So the export can build them as the typeclasses that reflect and tell time."""
        kinds = {thing["kind"] for r in a_town(size=160, seed=5)["rooms"]
                 for thing in r["fixtures"]}
        self.assertTrue({"mirror", "clock"} & kinds, kinds)

    def test_keys_are_bare_nouns_because_evennia_adds_the_article(self):
        every = ([k for pool in fixtures.STREET.values() for k, _ in pool]
                 + [k for pool in fixtures.INTERIOR.values() for k, _ in pool]
                 + [k for k, _ in fixtures.ROAD] + [k for k, _ in fixtures.LANDMARKS]
                 + [fixtures.SPECIAL[s][0] for s in fixtures.SPECIAL] + [fixtures.RAMP[0]])
        for key in every:
            self.assertFalse(key.lower().startswith(("a ", "an ", "the ")), key)

    def test_descriptions_are_permanently_true(self):
        """The rule a room's own description is held to."""
        import re
        texts = ([d for pool in fixtures.STREET.values() for _, d in pool]
                 + [d for pool in fixtures.INTERIOR.values() for _, d in pool]
                 + [d for _, d in fixtures.ROAD] + [d for _, d in fixtures.LANDMARKS])
        for text in texts:
            self.assertIsNone(re.search(r"\b(you|your|today|tonight|morning|evening|rain)\b",
                                        text, re.I), text)


class TestF2(unittest.TestCase):
    def road(self, length):
        return {"rooms": [{"id": n, "key": "Mill-Kiln Road"} for n in range(length)]}

    def test_one_room_in_three_has_something_to_look_at(self):
        for length in (3, 10, 25, 40):
            road = self.road(length)
            fixtures.furnish_road(road, random.Random(length))
            furnished = sum(1 for r in road["rooms"] if r.get("fixtures"))
            self.assertGreaterEqual(furnished * 3, length, length)

    def test_a_landmark_every_eight_to_twelve_rooms_and_never_at_the_ends(self):
        for length in (5, 12, 30, 44):
            road = self.road(length)
            fixtures.furnish_road(road, random.Random(1))
            marks = [n for n, r in enumerate(road["rooms"]) if r.get("landmark")]
            self.assertGreaterEqual(len(marks), max(1, length // 12), length)
            self.assertNotIn(0, marks)
            self.assertNotIn(length - 1, marks)


if __name__ == "__main__":
    unittest.main()
