"""
Shops are rooms you walk into, and the street outside says so.

The area-building laws are explicit and the generator broke three of them in every
settlement it ever made. A shop WAS a street room - `the weaponsmith` reached by walking
north - so:

  * T1 (MUST): a building is entered by its own noun, never by a compass direction.
  * T1a (MUST): you leave by the same noun, with `out` accepted as an alias.
  * T2 (MUST): an interior does not stand on the street lattice.
  * T5 (SHOULD): the noun a player types is the noun in the room description.

Nothing caught it, because the geometry linter only judges links it can read as directions
and there were none to read. These are the checks that would have.
"""

import random
import unittest

from evennia_roundtrip import naming

COMPASS = {"north", "south", "east", "west",
           "northeast", "northwest", "southeast", "southwest", "up", "down"}


def a_town(rooms=12, race="human", seed=5):
    """One settlement, named and described the way the generator does it."""
    area = {"rooms": [{"id": index, "cell": [index % 4, index // 4, 0],
                       "latitude_deg": 1.0 + index * 0.001, "longitude_deg": 2.0,
                       "elevation_m": 10.0}
                      for index in range(rooms)],
            "exits": []}
    naming.name_and_describe(area, race, random.Random(seed))
    return area


def interiors_of(area):
    return [room for room in area["rooms"] if room.get("interior")]


def doors_of(area):
    return [exit_ for exit_ in area["exits"] if exit_.get("door")]


class TestAShopIsARoomYouWalkInto(unittest.TestCase):
    def test_a_settlement_has_interiors_at_all(self):
        self.assertTrue(interiors_of(a_town()), "no shop was built as an interior")

    def test_no_trade_room_is_left_standing_on_the_street(self):
        """The whole fault: every shop found in a walkthrough was a street room."""
        area = a_town()
        outside = [room for room in area["rooms"] if not room.get("interior")]
        for room in outside:
            for trade in naming.TRADE_DOORS:
                self.assertNotIn(trade, room["key"].lower(),
                                 f"{room['key']} is a shop standing in the street")

    def test_an_interior_is_entered_by_its_noun_and_never_by_a_direction(self):
        area = a_town()
        for door in doors_of(area):
            self.assertNotIn(door["name"], COMPASS,
                             "law T1: a door is a noun, not a compass point")

    def test_you_leave_by_the_noun_you_came_in_through(self):
        """Law T1a. Canon: 61% of door-like links come back through the same noun."""
        area = a_town()
        for inside in interiors_of(area):
            went_in = [d for d in doors_of(area) if d["destination"] == inside["id"]]
            came_out = [d for d in doors_of(area) if d["source"] == inside["id"]]
            self.assertEqual(len(went_in), 1)
            self.assertEqual(len(came_out), 1)
            self.assertEqual(went_in[0]["name"], came_out[0]["name"])

    def test_the_way_out_is_the_one_marked_for_an_out_alias(self):
        area = a_town()
        for inside in interiors_of(area):
            leaving = [d for d in doors_of(area)
                       if d["source"] == inside["id"] and d.get("leaves")]
            self.assertEqual(len(leaving), 1, "exactly one door leaves an interior")
        entering = [d for d in doors_of(area) if d.get("leaves") and d.get("door")
                    and not any(room["id"] == d["source"] and room.get("interior")
                                for room in area["rooms"])]
        self.assertEqual(entering, [], "`out` must not be an alias on the way in")

    def test_an_interior_is_entered_from_exactly_one_street_room(self):
        """Law T3. A building with two street doors is a decision, not an accident."""
        area = a_town()
        for inside in interiors_of(area):
            ways = {d["source"] for d in doors_of(area) if d["destination"] == inside["id"]}
            self.assertEqual(len(ways), 1)

    def test_an_interior_does_not_stand_on_the_street_lattice(self):
        """Law T2: it has no cell, so it can never collide with a street or bend one."""
        for inside in interiors_of(a_town()):
            self.assertNotIn("cell", inside)

    def test_the_street_outside_names_the_door(self):
        """Law T5: a door the room does not mention is found by typing every word there is."""
        area = a_town()
        by_id = {room["id"]: room for room in area["rooms"]}
        for inside in interiors_of(area):
            street = by_id[inside["from"]]
            self.assertIn(inside["noun"], street["desc"].lower(),
                          f"{street['key']} does not mention its {inside['noun']}")

    def test_a_wild_trade_is_entered_by_its_noun_too(self):
        """A camp beside a road is as much a place you step into as a shop is."""
        wild = {"rooms": [{"id": n, "latitude_deg": 0.0, "longitude_deg": 0.0}
                          for n in range(9)],
                "exits": []}
        naming.name_and_describe(wild, "road", random.Random(3), settled=False)
        self.assertTrue(interiors_of(wild))
        for door in doors_of(wild):
            self.assertNotIn(door["name"], COMPASS)


class TestTheShopIsStillCountedAndStocked(unittest.TestCase):
    def test_the_trade_name_survives_so_the_tally_can_find_it(self):
        """Shops are counted by the trade word in a room's name; moving the room indoors
        must not make the world's shops vanish from its own summary."""
        from evennia_roundtrip import generate
        area = a_town()
        found = [room for room in area["rooms"]
                 if any(marker in room["key"].lower() for marker in generate.TRADE_MARKERS)]
        self.assertTrue(found)
        for room in found:
            self.assertTrue(room.get("interior"), "a counted shop must be the interior")

    def test_every_trade_has_a_door_including_the_open_air_ones(self):
        """A stall has no door and is still a place you step into. A trade you cannot `go`
        to behaves differently from every other one for no reason a player can see."""
        for marker, _name in naming.TRADES:
            self.assertIn(marker, naming.TRADE_DOORS)
        for marker, _name in naming.WILD_TRADES:
            self.assertIn(marker, naming.WILD_DOORS)
