"""
Every named street is a run of rooms a player can walk (law G1), read from the real exits.

The street plan used to name every row of the lattice a street and every column a cross
street without looking at the exits. The lattice is a tree with a few loops, so a third of
the rooms that sat side by side had no way between them, and a street sixteen rooms long on
paper broke into nine pieces on foot - 475 G1 failures in forty generated areas. These build
towns from the generator's own lattice and walk them.
"""

import random
import unittest

from evennia_roundtrip import areagen, generate, naming


def a_town(size, seed, race="human"):
    """A settlement grown and named exactly as the generator grows and names one."""
    rng = random.Random(seed)
    built, _problems = areagen.build_lattice(size, rng)
    rooms, exits = areagen.rooms_and_exits(built, ["x"], 1000)
    for room in rooms:
        room.update(latitude_deg=0.0, longitude_deg=0.0, elevation_m=5.0)
    area = {"rooms": rooms, "exits": exits}
    naming.name_and_describe(area, race, rng)
    return area


def broken_streets(area):
    """Every street whose rooms are not one connected run, with how many pieces it is in."""
    linked = {}
    for exit_ in area["exits"]:
        linked.setdefault(exit_["source"], set()).add(exit_["destination"])
        linked.setdefault(exit_["destination"], set()).add(exit_["source"])
    members = {}
    for room in area["rooms"]:
        for street in generate.streets_in(room["key"]):
            members.setdefault(street, set()).add(room["id"])
    broken = {}
    for street, rooms in members.items():
        unvisited, pieces = set(rooms), 0
        while unvisited:
            pieces += 1
            queue = [unvisited.pop()]
            while queue:
                here = queue.pop()
                for there in linked.get(here, ()):
                    if there in unvisited:
                        unvisited.discard(there)
                        queue.append(there)
        if pieces > 1:
            broken[street] = pieces
    return broken


class TestStreetsAreWalkable(unittest.TestCase):
    def test_no_street_is_in_pieces_in_a_city(self):
        for seed in range(6):
            area = a_town(160, seed)
            self.assertEqual(broken_streets(area), {}, "seed %d" % seed)

    def test_no_street_is_in_pieces_in_a_village(self):
        for seed in range(12):
            area = a_town(40, seed, race="dwarf")
            self.assertEqual(broken_streets(area), {}, "seed %d" % seed)

    def test_a_street_is_never_named_for_rooms_with_no_way_between_them(self):
        """The old fault exactly: two neighbouring cells, no exit, one street name."""
        rooms = [{"id": 1, "cell": [0, 0, 0]}, {"id": 2, "cell": [1, 0, 0]},
                 {"id": 3, "cell": [2, 0, 0]}]
        exits = [{"source": 1, "name": "east", "destination": 2},
                 {"source": 2, "name": "west", "destination": 1}]
        plan = naming.street_plan(rooms, "human", random.Random(1), exits=exits)
        self.assertEqual(generate.streets_in(plan[1]), generate.streets_in(plan[2]))
        self.assertNotEqual(generate.streets_in(plan[3])[0], generate.streets_in(plan[1])[0])

    def test_a_corner_carries_both_streets(self):
        rooms = [{"id": n, "cell": list(cell)} for n, cell in
                 enumerate([(0, 0, 0), (1, 0, 0), (2, 0, 0), (1, 1, 0), (1, -1, 0)])]
        exits = []
        for one, two, way, back in ((0, 1, "east", "west"), (1, 2, "east", "west"),
                                    (4, 1, "north", "south"), (1, 3, "north", "south")):
            exits += [{"source": one, "name": way, "destination": two},
                      {"source": two, "name": back, "destination": one}]
        plan = naming.street_plan(rooms, "human", random.Random(2), exits=exits)
        self.assertIn(" at ", plan[1])
        self.assertEqual(len(generate.streets_in(plan[1])), 2)

    def test_the_street_word_fits_the_length_of_the_run(self):
        """Law P7: a Row is kept to four rooms, so a long run is never called one."""
        for seed in range(6):
            area = a_town(160, seed)
            members = {}
            for room in area["rooms"]:
                for street in generate.streets_in(room["key"]):
                    members.setdefault(street, set()).add(room["id"])
            spine = max(members, key=lambda s: len(members[s]))
            for street, rooms in members.items():
                band = naming._word_band(street)
                if band and street != spine:
                    self.assertLessEqual(len(rooms), band[1], street)

    def test_no_street_shares_a_word_with_a_door(self):
        doors = set(naming.TRADE_DOORS) | set(naming.WILD_DOORS)
        for seed in range(6):
            for room in a_town(160, seed)["rooms"]:
                if room.get("interior"):
                    continue
                for street in generate.streets_in(room["key"]):
                    for door in doors:
                        self.assertNotIn(door, street.lower(), room["key"])


class TestNamesAreDistinctWithoutNumbers(unittest.TestCase):
    def test_no_two_shops_in_a_city_share_a_name(self):
        for seed in range(6):
            inside = [r["key"] for r in a_town(160, seed)["rooms"] if r.get("interior")]
            self.assertEqual(len(inside), len(set(inside)), "seed %d" % seed)

    def test_no_name_is_numbered(self):
        """Law R7: qualified, never numbered - "a hunter's camp (4)" was the old fallback."""
        import re
        for seed in range(6):
            for room in a_town(160, seed)["rooms"]:
                self.assertIsNone(re.search(r"\(\d+\)|\b\d+\b", room["key"]), room["key"])

    def test_a_repeat_is_told_apart_by_a_word(self):
        used = {"the alchemist on Grey Stair"}
        self.assertEqual(naming._distinct("the alchemist on Grey Stair", used),
                         "the old alchemist on Grey Stair")
        self.assertEqual(naming._distinct("a cache under stones", {"a cache under stones"}),
                         "an old cache under stones")


class TestRoadsAndRamps(unittest.TestCase):
    def test_a_road_is_named_for_both_ends_and_a_path_for_where_it_goes(self):
        self.assertEqual(generate.road_street("Greystair", "Bridgerow"), "Greystair-Bridgerow Road")
        self.assertEqual(generate.road_street("Greystair", "Crookmidden", "path"),
                         "Crookmidden Path")
        self.assertIsNone(generate.road_street(None, None))

    def test_a_ramp_is_a_stretch_of_the_street_it_carries_on(self):
        """And keeps the words the dock finder recognises a ramp by."""
        area = {"rooms": [{"id": 1, "key": "Alder Walk, west of Kiln Lane"},
                          {"id": 2, "key": "Alder Walk at Kiln Lane"}],
                "exits": [{"source": 2, "name": "west", "destination": 1}]}
        name = generate.ramp_name(area, area["rooms"][1], "east")
        self.assertEqual(name, "Alder Walk, boat ramp")
        self.assertIn("boat ramp", name)

    def test_streets_are_read_the_way_the_linter_reads_them(self):
        self.assertEqual(generate.streets_in("Market Row at Kiln Lane"), ["Market Row", "Kiln Lane"])
        self.assertEqual(generate.streets_in("Alder Walk, North End"), ["Alder Walk"])
        self.assertEqual(generate.streets_in("Grey Yard"), ["Grey Yard"])


if __name__ == "__main__":
    unittest.main()
