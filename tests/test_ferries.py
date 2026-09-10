"""
The ferry network: who gets a terminal, who is joined to whom, and how long the crossing is.

The properties worth holding are the ones a picture of the finished world would show: one
terminal per shore rather than one per town, a hub with spokes to every shore, a shore-to-
shore line only where going round by the hub would be most of the journey again, and crossing
times that differ from each other because the distances do.
"""

import unittest

from evennia_roundtrip import ferries

RADIUS_M = 6_371_000.0


def area(name, lat, lon, docks=1, rooms=10):
    return {"name": name, "latitude_deg": lat, "longitude_deg": lon,
            "docks": docks, "rooms": [{"id": n} for n in range(rooms)]}


class TestWhoStandsOnWater(unittest.TestCase):
    def test_only_areas_with_a_dock_are_coastal(self):
        found = ferries.coastal([area("port", 0.0, 0.0), area("inland", 1.0, 1.0, docks=0)])
        self.assertEqual([one["name"] for one in found], ["port"])

    def test_a_town_at_the_water_is_coastal_without_a_dock_room(self):
        """When streets stopped counting as docks, 399 of 400 areas had none, the planner
        saw one shore town, and every world after it had no ferries at all."""
        harbour = dict(area("harbour", 0.0, 0.0, docks=0), harbour_m=1200.0, landing_m=None)
        beach = dict(area("beach", 1.0, 0.0, docks=0), harbour_m=None, landing_m=300.0)
        dry = dict(area("dry", 2.0, 0.0, docks=0), harbour_m=None, landing_m=None)
        found = ferries.coastal([harbour, beach, dry])
        self.assertEqual([one["name"] for one in found], ["harbour", "beach"])

    def test_a_town_a_short_walk_from_the_sea_is_coastal(self):
        near = dict(area("near", 0.0, 0.0, docks=0), inland_km=11.1)
        far = dict(area("far", 1.0, 0.0, docks=0), inland_km=25.0)
        self.assertEqual([one["name"] for one in ferries.coastal([near, far])], ["near"])

    def test_a_hunting_ground_is_not_a_ferry_town(self):
        """Nobody runs a ferry service to a stretch of wild country."""
        wild = dict(area("marsh", 0.0, 0.0, docks=0), harbour_m=500.0, purpose="hunting")
        self.assertEqual(ferries.coastal([wild]), [])


class TestOneTerminalPerShore(unittest.TestCase):
    def test_towns_along_one_shore_are_one_cluster(self):
        """A shore is a chain: its two ends may be far apart and it is still one shore."""
        shore = [area("a", 0.0, 0.0), area("b", 0.0, 0.9), area("c", 0.0, 1.8)]
        groups = ferries.clusters(shore, RADIUS_M, within_m=120_000.0)
        self.assertEqual(len(groups), 1)
        self.assertEqual(len(groups[0]), 3)

    def test_opposite_shores_are_different_clusters(self):
        both = [area("north", 2.0, 0.0), area("south", -2.0, 0.0)]
        groups = ferries.clusters(both, RADIUS_M, within_m=120_000.0)
        self.assertEqual(len(groups), 2)

    def test_the_busiest_waterfront_is_the_terminal(self):
        """Not the nearest point: a hamlet on a headland is closer to the water than the
        town behind it and is not where a service would call."""
        group = [area("hamlet", 0.0, 0.0, docks=1, rooms=8),
                 area("town", 0.1, 0.0, docks=3, rooms=40)]
        self.assertEqual(ferries.terminal_of(group)["name"], "town")


class TestTheShapeOfTheNetwork(unittest.TestCase):
    def ring(self):
        """Four shores round a sea with an island in the middle."""
        return [area("island", 0.0, 0.0),
                area("north", 2.0, 0.0), area("south", -2.0, 0.0),
                area("east", 0.0, 2.0), area("west", 0.0, -2.0)]

    def test_the_hub_is_the_most_central_terminal(self):
        self.assertEqual(ferries.hub_of(self.ring(), RADIUS_M)["name"], "island")

    def test_every_shore_has_a_line_to_the_hub(self):
        made = ferries.pairs(self.ring(), RADIUS_M)
        spokes = {tuple(sorted((one["name"], other["name"])))
                  for one, other in made if "island" in (one["name"], other["name"])}
        self.assertEqual(len(spokes), 4)

    def test_a_shore_to_shore_line_only_where_it_pays(self):
        """Two shores on the same side of the sea are worth a direct boat; two on opposite
        sides are not, because the hub is on the way."""
        made = ferries.pairs(self.ring(), RADIUS_M)
        direct = {tuple(sorted((one["name"], other["name"])))
                  for one, other in made if "island" not in (one["name"], other["name"])}
        self.assertNotIn(("north", "south"), direct)
        self.assertNotIn(("east", "west"), direct)

    def test_neighbouring_shores_do_get_a_direct_line(self):
        close = [area("island", 0.0, 0.0), area("east", 0.0, 2.0), area("northeast", 0.4, 2.0)]
        made = ferries.pairs(close, RADIUS_M)
        direct = {tuple(sorted((one["name"], other["name"])))
                  for one, other in made if "island" not in (one["name"], other["name"])}
        self.assertIn(("east", "northeast"), direct)


class TestHowLongACrossingTakes(unittest.TestCase):
    def test_the_shortest_crossing_takes_the_floor_and_the_longest_the_ceiling(self):
        self.assertAlmostEqual(ferries.minutes_for(1000.0, 1000.0, 9000.0), 30.0)
        self.assertAlmostEqual(ferries.minutes_for(9000.0, 1000.0, 9000.0), 50.0)

    def test_a_middling_crossing_lands_in_the_middle(self):
        self.assertAlmostEqual(ferries.minutes_for(5000.0, 1000.0, 9000.0), 40.0)

    def test_nothing_ever_leaves_the_band(self):
        for metres in (0.0, 500.0, 1000.0, 5000.0, 9000.0, 90_000.0):
            minutes = ferries.minutes_for(metres, 1000.0, 9000.0)
            self.assertGreaterEqual(minutes, 30.0)
            self.assertLessEqual(minutes, 50.0)

    def test_when_every_crossing_is_the_same_they_all_take_the_floor(self):
        """The shortest crossing in the world is also the longest; there is nothing to
        spread, and the alternative is a division by zero."""
        self.assertAlmostEqual(ferries.minutes_for(4000.0, 4000.0, 4000.0), 30.0)


class TestTheWholePlan(unittest.TestCase):
    def world(self):
        """Shores at different distances, because a sea is not a circle - and a world where
        every crossing is the same length is the one case that cannot show a spread."""
        return [area("island", 0.0, 0.0), area("north", 1.2, 0.0), area("south", -2.4, 0.0),
                area("east", 0.0, 3.1), area("inland", 5.0, 5.0, docks=0)]

    def test_an_inland_area_gets_no_terminal(self):
        plan = ferries.plan(self.world(), RADIUS_M)
        self.assertNotIn("inland", [one["name"] for one in plan["terminals"]])

    def test_the_times_are_spread_across_the_band(self):
        plan = ferries.plan(self.world(), RADIUS_M)
        minutes = sorted(line["minutes"] for line in plan["lines"])
        self.assertGreaterEqual(minutes[0], 30.0)
        self.assertLessEqual(minutes[-1], 50.0)
        self.assertGreater(minutes[-1] - minutes[0], 0.0,
                           "crossings of different lengths must take different times")

    def test_a_line_with_no_water_route_is_dropped_rather_than_built(self):
        """A berth to a place no boat can reach is a room a player waits in for ever."""
        plan = ferries.plan(self.world(), RADIUS_M, sailed_m=lambda one, other: 0.0)
        self.assertEqual(plan["lines"], [])

    def test_every_line_names_both_its_ends(self):
        plan = ferries.plan(self.world(), RADIUS_M)
        self.assertTrue(plan["lines"])
        for line in plan["lines"]:
            self.assertEqual(len(line["ends"]), 2)
            self.assertNotEqual(line["ends"][0], line["ends"][1])

    def test_a_berth_is_named_for_where_it_goes(self):
        self.assertEqual(ferries.berth_key("Longmire"), "the Longmire berth")

    def test_a_destination_that_carries_its_article_does_not_get_two(self):
        self.assertEqual(ferries.berth_key("the island"), "the island berth")
