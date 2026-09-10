"""
A keepsake says where it came from; a meal does not.

A hundred and two wares cannot fill nineteen thousand shelves without repeating, so items
are made unique the way a real economy made them recognisable: by the mark of the place that
made them. Each settlement works in one colour and one material, so its goods read as a set
rather than as a shelf of unrelated oddments.
"""

import random
import unittest

from evennia_roundtrip import period, stock


class TestWhatCarriesAPlace(unittest.TestCase):
    def setUp(self):
        self.look = {"colour": "green", "material": "leather"}

    def test_a_durable_ware_is_marked_with_its_place(self):
        made = stock.stock_for("the weaponsmith", "town", random.Random(1),
                               look=self.look, place="Longmire")
        self.assertTrue(made)
        for ware in made:
            self.assertTrue(ware.endswith("from Longmire"), ware)

    def test_a_meal_is_not_a_souvenir(self):
        """You carry a dagger home from the badlands; you do not carry home the broth."""
        made = stock.stock_for("the inn", "town", random.Random(1),
                               look=self.look, place="Longmire")
        self.assertTrue(made)
        for ware in made:
            self.assertNotIn("from Longmire", ware)

    def test_without_a_place_nothing_changes(self):
        plain = stock.stock_for("the weaponsmith", "town", random.Random(4))
        for ware in plain:
            self.assertNotIn(" from ", ware)


class TestHowASouvenirReads(unittest.TestCase):
    def setUp(self):
        self.look = {"colour": "green", "material": "leather"}

    def test_the_material_makes_the_part_it_could_actually_make(self):
        """Your leather is the hilt, not the blade. Pushed in front of the whole item it
        writes "a spider silk short sword", which is not a thing."""
        self.assertEqual(stock.souvenir("a short sword", self.look, "Longmire", "hilted"),
                         "a green leather-hilted short sword from Longmire")

    def test_a_ware_that_names_its_own_material_keeps_it(self):
        """"a green leather clay lamp" is two materials arguing."""
        self.assertEqual(stock.souvenir("a clay lamp", self.look, "Longmire", "carved"),
                         "a green clay lamp from Longmire")

    def test_a_trade_with_no_part_takes_only_its_place(self):
        """An inn's beer is not hilted, bound or handled."""
        self.assertEqual(stock.souvenir("a short sword", self.look, "Longmire"),
                         "a short sword from Longmire")

    def test_a_phrase_is_left_alone_and_only_takes_the_place(self):
        """"a green leather bundle of arrows" is what happens when adjectives are pushed in
        front of a collective noun."""
        self.assertEqual(stock.souvenir("a bundle of arrows", self.look, "Longmire",
                                        "hilted"), "a bundle of arrows from Longmire")
        self.assertEqual(
            stock.souvenir("a boar spear with a crossbar", self.look, "Longmire", "hilted"),
            "a boar spear with a crossbar from Longmire")

    def test_the_article_follows_the_word_that_now_comes_first(self):
        """Keeping the ware's own gave "an sky-blue iron cook pot"."""
        sky = {"colour": "sky-blue", "material": "oak"}
        self.assertEqual(stock.souvenir("an iron cook pot", sky, "Longmire", "handled"),
                         "a sky-blue iron cook pot from Longmire")
        ox = {"colour": "oxblood", "material": "oak"}
        self.assertEqual(stock.souvenir("a short sword", ox, "Longmire", "hilted"),
                         "an oxblood oak-hilted short sword from Longmire")

    def test_a_material_is_found_by_whole_word(self):
        """"tin" is inside "hunting": the spear was taken for tinware and lost its haft."""
        sky = {"colour": "sky-blue", "material": "olivewood"}
        self.assertEqual(stock.souvenir("a hunting spear", sky, "Greystair", "hafted"),
                         "a sky-blue olivewood-hafted hunting spear from Greystair")


class TestWhichPartAWareHas(unittest.TestCase):
    """The shelves once sold "an olivewood-hilted spear ferrule" and "a hilted sword blank,
    unhilted": the trade's one part stamped on every ware it sold."""

    def test_only_a_ware_with_the_part_gets_it(self):
        self.assertEqual(stock.part_for("weaponsmith", "a dagger in a plain sheath", "oak"),
                         "hilted")
        self.assertEqual(stock.part_for("weaponsmith", "a hand axe", "oak"), "hafted")
        for ware in ("a spear ferrule", "a sword blank, unhilted", "a whetstone",
                     "a bundle of arrows"):
            self.assertIsNone(stock.part_for("weaponsmith", ware, "oak"), ware)

    def test_the_material_has_to_be_able_to_be_the_part(self):
        self.assertEqual(stock.part_for("stables", "a saddle", "fine leather"), "stitched")
        self.assertIsNone(stock.part_for("stables", "a saddle", "olivewood"))

    def test_a_material_is_matched_by_whole_word(self):
        self.assertIsNone(stock.part_for("weaponsmith", "a dagger", "calabash"))

    def test_no_shelf_the_generator_stocks_carries_a_part_it_cannot_have(self):
        rng = random.Random(7)
        for trade in stock.PARTS:
            for material in ("olivewood", "fine leather", "horn", "bronze"):
                look = {"colour": "grey", "material": material}
                for _ in range(5):
                    for ware in stock.stock_for("the %s" % trade, "town", rng, look=look,
                                                place="Longmire"):
                        self.assertNotIn("unhilted", ware.replace("a sword blank, unhilted",
                                                                  ""), ware)
                        self.assertNotRegex(ware, r"-(hilted|hafted) (spear ferrule|whetstone"
                                                  r"|sword blank|bundle)", ware)


class TestAPlaceHasOneLook(unittest.TestCase):
    def test_a_settlement_works_in_one_colour_and_one_material(self):
        look = stock.signature("dwarf", random.Random(2))
        self.assertIn(look["colour"], stock.LOOKS["dwarf"]["colour"])
        self.assertIn(look["material"], stock.LOOKS["dwarf"]["material"])

    def test_a_people_with_no_look_written_down_still_gets_one(self):
        look = stock.signature("nobody", random.Random(2))
        self.assertIn(look["colour"], stock.PLAIN_LOOK["colour"])

    def test_no_two_places_sell_the_same_keepsake(self):
        """The whole point: the same ware from two towns is two different things."""
        rng = random.Random(7)
        seen = {}
        for place, race in (("Longmire", "human"), ("Elderbough", "elf"),
                            ("Farsteading", "volgrin"), ("Lowscratch", "goblin")):
            look = stock.signature(race, rng)
            for ware in stock.stock_for("the general store", "city", rng,
                                        look=look, place=place):
                self.assertNotIn(ware, seen, f"{ware} is sold in two places")
                seen[ware] = place

    def test_every_material_can_be_a_hilt_or_a_strap(self):
        """A material that cannot be the part is a material that writes nonsense. Checked by
        eye once and pinned here: no stone, no dust, no glass, no felt."""
        cannot = ("granite", "marble", "dust", "glass", "felt", "slate", "flint", "clay",
                  "reed", "wool", "birchbark", "spider silk")
        for race, made in stock.LOOKS.items():
            for word in made["material"]:
                for bad in cannot:
                    self.assertNotIn(bad, word, f"{race} works in {word}")

    def test_every_look_is_period_appropriate(self):
        """The same lint the room descriptions face: a colour or a material that could not
        exist yet is a defect wherever it appears."""
        for race, made in stock.LOOKS.items():
            for field in ("colour", "material"):
                for word in made[field]:
                    self.assertEqual(period.offences(word), [], f"{race} {field}: {word}")
