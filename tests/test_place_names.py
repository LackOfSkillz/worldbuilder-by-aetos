"""
No two places in one world share a name.

A culture has six to eight name-heads and as many tails - thirty-six to sixty-four possible
names - and two words drawn independently is the birthday problem wearing a cloak. A run of
a hundred and thirty areas came out with a hundred and six distinct names: three Farcamps,
three Broadsteadings, and two Winterfolds a player could stand between and see both labels.

The same fault was found in the street names and fixed there; nobody looked here.
"""

import random
import unittest

from evennia_roundtrip import naming


class TestAWorldsPlaceNames(unittest.TestCase):
    def test_a_register_stops_a_name_being_used_twice(self):
        taken = set()
        rng = random.Random(4)
        made = [naming.place_name("human", rng, taken) for _ in range(40)]
        self.assertEqual(len(made), len(set(made)))

    def test_without_a_register_the_old_behaviour_is_unchanged(self):
        """Callers that do not care - an inn's name, a test - keep the simple form."""
        rng = random.Random(4)
        self.assertIsInstance(naming.place_name("human", rng), str)

    def test_a_name_already_in_the_world_is_never_taken(self):
        """A generated town called The Landing is worse than two called Farcamp."""
        taken = {"Millfield"}
        rng = random.Random(9)
        made = [naming.place_name("human", rng, taken) for _ in range(30)]
        self.assertNotIn("Millfield", made)

    def test_more_towns_than_names_qualifies_rather_than_numbers(self):
        """Law R7's rule for streets, applied to towns: `Upper Farcamp`, never `Farcamp 2`."""
        taken = set()
        rng = random.Random(2)
        voice = naming.voice_for("volgrin")
        room = len(voice["head"]) * len(voice["tail"])
        made = [naming.place_name("volgrin", rng, taken) for _ in range(room + 12)]
        self.assertEqual(len(made), len(set(made)))
        spare = [name for name in made if " " in name]
        self.assertTrue(spare, "the vocabulary should have run out and been qualified")
        for name in spare:
            self.assertIn(name.split()[0], naming.QUALIFIERS)
            self.assertFalse(any(ch.isdigit() for ch in name))

    def test_the_names_are_still_the_culture_s_own(self):
        taken = set()
        rng = random.Random(6)
        voice = naming.voice_for("elf")
        for _ in range(20):
            name = naming.place_name("elf", rng, taken)
            self.assertTrue(any(name.startswith(head) for head in voice["head"]),
                            f"{name} is not an elf name")
