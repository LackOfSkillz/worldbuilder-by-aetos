"""
A dock is a room at the water that is named as one - never a street that sounds wet.

The detector this replaces matched "bridge", "stair", "steps" and "causeway" anywhere in a
room's name. Those are ordinary street words in a generated town, so three areas once
reported 175 docks, and the dry-ground gate excused any street with such a name for
standing in the sea.
"""

import unittest

from evennia_roundtrip import generate, populate

RADIUS_M = 6371000.0


def sea_east_of(longitude):
    """An oracle: dry land west of `longitude`, five metres of water east of it."""
    return lambda lat, lon: -5.0 if lon > longitude else 10.0


def room(rid, key, lon, **extra):
    return dict({"id": rid, "key": key, "latitude_deg": 0.0, "longitude_deg": lon}, **extra)


class TestWhatIsADock(unittest.TestCase):
    def test_a_quay_on_the_waterfront_is_a_dock(self):
        area = {"rooms": [room(1, "Pale Quay, East End", 0.999)]}
        self.assertEqual(generate.mark_docks(area, sea_east_of(1.0), RADIUS_M), 1)
        self.assertTrue(area["rooms"][0]["dock"])

    def test_a_quay_two_miles_inland_is_a_street_with_a_nautical_name(self):
        area = {"rooms": [room(1, "Pale Quay, East End", 0.97)]}
        self.assertEqual(generate.mark_docks(area, sea_east_of(1.0), RADIUS_M), 0)

    def test_streets_that_merely_sound_wet_are_never_docks(self):
        area = {"rooms": [room(1, "Bridge Prospect at Quiet Stair", 0.999),
                          room(2, "Reed Causeway, West End", 0.999),
                          room(3, "Grey Steps", 0.999)]}
        self.assertEqual(generate.mark_docks(area, sea_east_of(1.0), RADIUS_M), 0)

    def test_a_dock_word_counts_only_as_a_whole_word(self):
        self.assertFalse(generate.names_a_dock("Slipper Lane"))
        self.assertTrue(generate.names_a_dock("Mill Slip"))

    def test_an_interior_is_never_a_dock_even_the_harbour_inn(self):
        area = {"rooms": [room(1, "the Harbour Inn", 0.999, interior=True)]}
        self.assertEqual(generate.mark_docks(area, sea_east_of(1.0), RADIUS_M), 0)

    def test_a_boat_ramp_is_a_dock_by_construction(self):
        area = {"rooms": [room(1, "Alder Walk, boat ramp", 0.5, ramp=True)]}
        self.assertEqual(generate.mark_docks(area, sea_east_of(1.0), RADIUS_M), 1)


class TestOnlyDocksMayBeWet(unittest.TestCase):
    def test_a_street_named_stair_standing_in_the_sea_fails_the_ground_check(self):
        rooms = [room(1, "Quiet Stair, North End", 1.5)]
        report = populate.dry_enough(rooms, sea_east_of(1.0))
        self.assertFalse(report["fits"])
        self.assertEqual(report["wet_unexpected"], ["Quiet Stair, North End"])

    def test_a_flagged_dock_may_stand_in_the_water(self):
        rooms = [room(1, "Pale Quay", 1.5, dock=True), room(2, "a boat ramp", 1.5, ramp=True)]
        self.assertTrue(populate.dry_enough(rooms, sea_east_of(1.0))["fits"])


if __name__ == "__main__":
    unittest.main()
