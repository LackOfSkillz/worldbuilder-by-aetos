"""
Where the great cities go.

Every site score in this generator rewards water, so left alone it strings the large places
along a coast and leaves the interior to hamlets. The grid takes that judgement away from
the scorer without taking away its judgement about where a city stands: the cell decides
that there IS a city, the terrain inside it decides where.
"""

import unittest

from evennia_roundtrip import hubs

REGION = (-20.0, 20.0, -40.0, 40.0)


def site(lat, lon, score=1.0):
    return {"latitude_deg": lat, "longitude_deg": lon, "score": score}


class TestHowManyCities(unittest.TestCase):
    def test_one_city_per_fifty_areas(self):
        self.assertEqual(hubs.how_many(400), 8)
        self.assertEqual(hubs.how_many(130), 3)

    def test_a_small_run_still_has_a_centre(self):
        self.assertEqual(hubs.how_many(12), 1)

    def test_a_part_full_band_still_earns_a_city(self):
        self.assertEqual(hubs.how_many(51), 2)


class TestTheGrid(unittest.TestCase):
    def test_the_cells_cover_the_region_once(self):
        """The far edge reaches a hair past the region on purpose: the bounds are half-open,
        so without it a site exactly on the region's boundary would fall in no cell at all."""
        edge = 1e-6
        made = hubs.cells(REGION, 8)
        self.assertEqual(len(made), 8)
        for lat_low, lat_high, lon_low, lon_high in made:
            self.assertGreaterEqual(lat_low, REGION[0])
            self.assertLessEqual(lat_high, REGION[1] + edge)
            self.assertGreaterEqual(lon_low, REGION[2])
            self.assertLessEqual(lon_high, REGION[3] + edge)

    def test_a_site_on_the_regions_own_edge_still_lands_in_a_cell(self):
        """What the epsilon is for."""
        corner = [site(REGION[1], REGION[3], 4.0)]
        self.assertEqual(len(hubs.plan(corner, REGION, 50)), 1)

    def test_the_cells_do_not_overlap(self):
        made = hubs.cells(REGION, 6)
        for one in range(len(made)):
            for other in range(one + 1, len(made)):
                a, b = made[one], made[other]
                apart = (a[1] <= b[0] or b[1] <= a[0] or a[3] <= b[2] or b[3] <= a[2])
                self.assertTrue(apart, f"{a} overlaps {b}")

    def test_a_whole_planet_is_the_default_ground(self):
        made = hubs.cells(None, 4)
        self.assertEqual(len(made), 4)


class TestWhichSitesBecomeCities(unittest.TestCase):
    def test_one_city_per_cell_and_the_best_site_in_it(self):
        spread = [site(-10, -20, 1.0), site(-9, -19, 9.0), site(10, 20, 5.0)]
        chosen = hubs.plan(spread, REGION, 100, every=50)
        self.assertEqual(len(chosen), 2)
        self.assertIn(9.0, [one["score"] for one in chosen])

    def test_the_interior_gets_a_city_even_though_the_coast_scores_better(self):
        """The whole point of the grid: a cell inland has its own city, and it is the best
        site in that cell rather than the best site in the world."""
        coast = [site(-19, -39, 99.0), site(-18, -38, 98.0)]
        inland = [site(1.0, 1.0, 3.0)]
        chosen = hubs.plan(coast + inland, REGION, 100, every=50)
        self.assertTrue(any(one["score"] == 3.0 for one in chosen),
                        "the inland cell must still get a city")

    def test_a_cell_that_already_holds_a_place_is_left_alone(self):
        """A hand-built city is its region's hub. Dropping a generated capital beside it is
        the one result that makes a generated world unusable to somebody who had a world."""
        theirs = [{"anchor": {"latitude_deg": -10.0, "longitude_deg": -20.0}}]
        spread = [site(-10, -20, 9.0), site(10, 20, 5.0)]
        chosen = hubs.plan(spread, REGION, 100, existing=theirs, every=50)
        self.assertEqual(len(chosen), 1)
        self.assertEqual(chosen[0]["score"], 5.0)

    def test_a_cell_with_no_candidate_gets_no_city(self):
        """Empty ocean is allowed to have none; the grid says where a city MAY be."""
        chosen = hubs.plan([site(10, 20, 5.0)], REGION, 200, every=50)
        self.assertEqual(len(chosen), 1)

    def test_a_city_is_bigger_than_anything_else_the_generator_builds(self):
        from evennia_roundtrip import areagen
        self.assertGreaterEqual(hubs.HUB_ROOMS, 150)
        self.assertGreater(hubs.HUB_ROOMS, max(areagen.TYPE_SIZE.values()))
