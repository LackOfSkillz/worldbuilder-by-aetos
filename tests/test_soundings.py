"""
The generator checking its own work against the water.

Both faults this finds were found by eye, from screenshots, after a run was over - and both
had been in every world before that one. A room below sea level says so in its own record;
a span between two dry rooms that crosses a strait says nothing anywhere, which is why it
lasted longest.
"""

import unittest

from evennia_roundtrip import soundings


def room(number, lat, lon, height):
    return {"id": number, "latitude_deg": lat, "longitude_deg": lon, "elevation_m": height}


class TestARoomInTheWater(unittest.TestCase):
    def test_a_room_below_sea_level_is_found(self):
        place = {"rooms": [room(1, 0, 0, 12.0), room(2, 0, 0.1, -30.0)]}
        self.assertEqual([r["id"] for r in soundings.wet_rooms(place)], [2])

    def test_a_dry_place_reports_nothing(self):
        place = {"rooms": [room(1, 0, 0, 12.0), room(2, 0, 0.1, 8.0)]}
        self.assertEqual(soundings.wet_rooms(place), [])

    def test_an_interior_is_not_sounded(self):
        """It hangs off its street and has no ground of its own."""
        place = {"rooms": [room(1, 0, 0, 5.0),
                           dict(room(2, 0, 0, -100.0), interior=True)]}
        self.assertEqual(soundings.wet_rooms(place), [])


class TestASpanOverWater(unittest.TestCase):
    def dry_then_wet(self):
        """Two rooms on dry land with a channel between them."""
        def at(lat, lon):
            return -50.0 if 0.02 < lon < 0.08 else 10.0
        return at

    def test_a_strait_between_two_dry_rooms_is_found(self):
        """Neither room says anything is wrong, which is why this one lasted longest."""
        place = {"rooms": [room(1, 0, 0.0, 10.0), room(2, 0, 0.1, 10.0)]}
        found = soundings.wet_spans(place, self.dry_then_wet())
        self.assertEqual(len(found), 1)
        self.assertEqual(found[0][:2], (1, 2))
        self.assertLess(found[0][2], 0.0)

    def test_dry_ground_all_the_way_is_no_span(self):
        place = {"rooms": [room(1, 0, 0.0, 10.0), room(2, 0, 0.1, 10.0)]}
        self.assertEqual(soundings.wet_spans(place, lambda lat, lon: 10.0), [])

    def test_the_line_does_not_detour_through_interiors(self):
        """A wayside shrine hanging off a road is not a point the road passes through, and
        walking out to it and back is how a road came to look like it crossed a strait."""
        place = {"rooms": [room(1, 0, 0.0, 10.0),
                           dict(room(9, 5, 5, 10.0), interior=True),
                           room(2, 0, 0.01, 10.0)]}
        def at(lat, lon):
            return -50.0 if abs(lat) > 1 else 10.0
        self.assertEqual(soundings.wet_spans(place, at), [])


class TestTheWholeWorld(unittest.TestCase):
    def test_a_clean_world_reports_nothing(self):
        world = {"areas": [{"name": "a", "rooms": [room(1, 0, 0, 5.0)]}], "roads": []}
        report = soundings.check(world, lambda lat, lon: 5.0)
        self.assertEqual(report["rooms_under_water"], 0)
        self.assertEqual(report["areas"], [])

    def test_a_drowned_area_is_named(self):
        world = {"areas": [{"display_name": "Sunkport",
                            "rooms": [room(1, 0, 0, -12.0)]}], "roads": []}
        report = soundings.check(world, lambda lat, lon: -12.0)
        self.assertEqual(report["rooms_under_water"], 1)
        self.assertEqual(report["areas"][0]["name"], "Sunkport")
        self.assertEqual(report["areas"][0]["deepest_room_m"], -12.0)

    def test_a_road_across_a_strait_is_named(self):
        world = {"areas": [], "roads": [{"display_name": "the coast road",
                                         "rooms": [room(1, 0, 0.0, 10.0),
                                                   room(2, 0, 0.1, 10.0)]}]}
        report = soundings.check(world, lambda lat, lon: -50.0 if 0.02 < lon < 0.08 else 10.0)
        self.assertEqual(report["spans_over_water"], 1)
        self.assertEqual(report["roads"][0]["name"], "the coast road")
