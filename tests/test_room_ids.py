"""
Every room in a world has an id no other room has.

A 130-area world came out with 98 rooms sharing an id with another, and nothing noticed
until the exporter refused to write it. The cause was arithmetic: id budgets advanced by
how MANY rooms a place had, while a thinned road's surviving ids have gaps and its wayside
interiors are hung off it at `max(id) + 1` - past the end the count reached. The next road
started inside the last one.
"""

import unittest

from evennia_roundtrip import generate


def a_thinned_road(first_id, laid=50, kept_every=3, interiors=3):
    """A road laid with `laid` rooms, thinned, then given wayside interiors."""
    rooms = [{"id": first_id + n} for n in range(0, laid, kept_every)]
    top = max(room["id"] for room in rooms)
    rooms += [{"id": top + 1 + n, "interior": True} for n in range(interiors)]
    return rooms


class TestIdBudgets(unittest.TestCase):
    def test_the_old_count_based_budget_does_collide(self):
        """Pinned so the test proves something: the formula that was replaced is wrong."""
        road = a_thinned_road(1000)
        next_by_count = 1000 + len(road) + 10
        self.assertLessEqual(next_by_count, max(room["id"] for room in road),
                             "the fixture no longer reproduces the fault it guards")

    def test_the_next_place_starts_past_every_id_already_used(self):
        road = a_thinned_road(1000)
        start = generate._past(road)
        self.assertGreater(start, max(room["id"] for room in road))

    def test_two_places_budgeted_in_turn_never_share_an_id(self):
        first = a_thinned_road(1000)
        second = a_thinned_road(generate._past(first))
        ids = [room["id"] for room in first + second]
        self.assertEqual(len(ids), len(set(ids)))

    def test_an_empty_place_leaves_the_budget_at_the_gap(self):
        self.assertEqual(generate._past([], gap=10), 10)


if __name__ == "__main__":
    unittest.main()
