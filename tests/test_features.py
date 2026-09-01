"""
Tests for placed features, and for the claim that placing them changes nothing else.

Three species of bug are possible here and each gets its own class.

**A stamp is a cliff waiting to happen.** Every previous phase produced discontinuities by
deciding something hard about a continuous quantity, and a feature is nothing but a hard
decision with an edge. The difference between steep and discontinuous is not a threshold,
so it is not tested with one: halve the step and a smooth function halves its worst jump,
while a cliff does not care.

**Composition is order, and order is easy to get backwards.** A bar listed before the
channel it lies across is dredged away. That happened, and the test that would have caught
it is now here.

**The marks layer has to earn its existence.** If a chart sampling terrain could find the
pinnacle, the second channel would be complexity for its own sake. So the test asserts the
opposite of what a test usually asserts: that sampling *misses* it, and misses it
differently depending on where the grid falls.
"""

import math
import unittest

from worldbuilder.bathymetry.features import CARVE, RAISE, Feature, Features
from worldbuilder.geometry.sphere import SpherePoint
from worldbuilder.geometry.tangent import TangentFrame
from worldbuilder.geometry.vectors import Vec3
from worldbuilder.regions.demo import WORLD_SEED, demo_region
from worldbuilder.terrain.surface import Surface


def scattered(count=400):
    golden = math.pi * (3.0 - math.sqrt(5.0))
    points = []
    for index in range(count):
        z = 1.0 - 2.0 * (index + 0.5) / count
        ring = math.sqrt(max(0.0, 1.0 - z * z))
        angle = golden * index
        points.append(SpherePoint(Vec3(math.cos(angle) * ring, math.sin(angle) * ring, z)))
    return points


class RegionTestCase(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.region = demo_region()
        cls.coast = cls.region.coast
        cls.world = Surface(WORLD_SEED, features=cls.region.features)
        cls.bare = Surface(WORLD_SEED)

    def named(self, kind):
        for feature in self.region.features:
            if feature.kind == kind:
                return feature
        raise AssertionError(f"no feature called {kind}")

    def worst_step(self, feature, steps):
        """The largest change between neighbouring samples on a line across a feature."""
        frame = TangentFrame.at(feature.at)
        span = feature.reach_m() * 1.5
        worst = 0.0
        for east_axis in (True, False):
            previous = None
            for index in range(steps + 1):
                offset = (2.0 * index / steps - 1.0) * span
                point = frame.local_to_sphere(
                    offset if east_axis else 0.0, 0.0 if east_axis else offset
                )
                value = self.world.structural_m(point)
                if previous is not None:
                    worst = max(worst, abs(value - previous))
                previous = value
        return worst


class TestNoCliffs(RegionTestCase):
    def test_halving_the_step_halves_the_worst_jump(self):
        """
        The only honest way to tell steep from discontinuous. A pinnacle rising
        twenty-five metres in seventy is *supposed* to be abrupt, so a fixed threshold
        would either pass a cliff or fail a rock. Refinement is the test: sample a line
        across the feature at N points and at 2N, and a continuous function must roughly
        halve its worst neighbour-to-neighbour change while a step function cannot.

        """
        for feature in self.region.features:
            coarse = self.worst_step(feature, 400)
            fine = self.worst_step(feature, 800)
            self.assertLess(
                fine, coarse * 0.75 + 0.02,
                f"{feature.kind} did not smooth out under refinement "
                f"({coarse:.3f} -> {fine:.3f} m)",
            )

    def test_the_cheap_rejection_sits_outside_the_support(self):
        """
        `weight_at` skips the projection past `reach_m`. If the bump were still worth
        anything there, that saving would be a wall round every feature - which is M1.4's
        trench bug and M1.5's window bug, for the third time.

        """
        for feature in self.region.features:
            frame = TangentFrame.at(feature.at)
            reach = feature.reach_m()
            for bearing in range(0, 360, 15):
                radians = math.radians(bearing)
                inside = frame.local_to_sphere(
                    math.sin(radians) * reach * 0.999, math.cos(radians) * reach * 0.999
                )
                outside = frame.local_to_sphere(
                    math.sin(radians) * reach * 1.001, math.cos(radians) * reach * 1.001
                )
                self.assertLess(
                    abs(self.world.structural_m(inside) - self.world.structural_m(outside)),
                    0.5,
                )

    def worst_authority_step(self, feature, steps):
        frame = TangentFrame.at(feature.at)
        span = feature.reach_m() * 1.4
        worst = 0.0
        previous = None
        for index in range(steps + 1):
            offset = (2.0 * index / steps - 1.0) * span
            point = frame.local_to_sphere(offset, offset * 0.3)
            _, authority = self.region.features.apply(
                point, self.bare.shelf.elevation_m(point)
            )
            if previous is not None:
                worst = max(worst, abs(authority - previous))
            previous = authority
        return worst

    def test_authority_does_not_jump_when_a_feature_starts_applying(self):
        """
        The second argument the phase needed. A one-way feature contributes nothing at the
        moment its target meets the ground - but its *authority* over detail would have
        snapped from nothing to its full weight there, putting a ring of abruptly smooth
        seabed round every bank.

        Tested by refinement for the same reason the elevation is: authority climbs from
        nothing to one across a bank edge, so it is *supposed* to move quickly, and a
        fixed per-step threshold ends up measuring the step size rather than the function.

        """
        for kind in ("north bank", "south bank", "harbour bar"):
            feature = self.named(kind)
            coarse = self.worst_authority_step(feature, 600)
            fine = self.worst_authority_step(feature, 1200)
            self.assertLess(fine, coarse * 0.75 + 0.002, kind)


class TestComposition(RegionTestCase):
    def test_a_raise_never_deepens_and_a_carve_never_fills(self):
        points = [
            self.coast.at(off * 1000.0, along * 1000.0)
            for off in range(-6, 31, 2)
            for along in range(-25, 26, 2)
        ]
        for feature in self.region.features:
            only = Features([feature])
            for point in points:
                before = self.bare.structural_m(point)
                after, _ = only.apply(point, before)
                if feature.compose == RAISE:
                    self.assertGreaterEqual(after, before - 1e-9, feature.kind)
                elif feature.compose == CARVE:
                    self.assertLessEqual(after, before + 1e-9, feature.kind)

    def test_the_bar_survives_the_channel_that_reaches_past_it(self):
        """
        The bug this class exists for. The approach channel was listed after the bar and
        long enough to reach it, so it dredged away the one feature the harbour needs to
        be interesting. Both facts are asserted: the bar is shoal, and the channel outside
        it is not.

        """
        bar = self.named("harbour bar")
        self.assertLess(self.world.structural_m(bar.at), -2.0)
        self.assertGreater(self.world.structural_m(bar.at), -4.5)

        outside = self.coast.at(12_000.0, 0.0)
        self.assertLess(self.world.structural_m(outside), -12.0)

    def test_composition_order_is_what_makes_the_bar_possible(self):
        """Stated directly: reverse the list and the bar goes."""
        forwards = Features(list(self.region.features))
        backwards = Features(list(self.region.features)[::-1])
        bar = self.named("harbour bar")
        base = self.bare.shelf.elevation_m(bar.at)
        self.assertGreater(
            forwards.apply(bar.at, base)[0], backwards.apply(bar.at, base)[0]
        )


class TestTheFeaturesAreWhatTheyAreCalled(RegionTestCase):
    def test_the_harbour_is_water_and_the_land_around_it_is_not(self):
        basin = self.named("harbour basin")
        self.assertLess(self.world.structural_m(basin.at), -6.0)
        for along in (-4_000.0, 4_000.0):
            self.assertGreater(self.world.structural_m(self.coast.at(-2_000.0, along)), 0.0)

    def test_the_entrance_is_the_only_way_in(self):
        """
        A gut with arms. On a coast rising two metres a kilometre the shore cannot make
        the arms itself, so the moles do - and they have to be land, not merely shoal.

        """
        self.assertLess(self.world.structural_m(self.coast.at(1_600.0, 0.0)), -5.0)
        for along in (-620.0, 620.0):
            self.assertGreater(self.world.structural_m(self.coast.at(1_600.0, along)), 2.0)

    def test_the_approach_is_a_gut_between_two_banks(self):
        """The reason a chart is worth reading: staying in the deep bit is a decision."""
        middle = self.world.structural_m(self.coast.at(8_000.0, 0.0))
        for along in (-3_000.0, 3_000.0):
            flank = self.world.structural_m(self.coast.at(8_000.0, along))
            self.assertGreater(flank, middle + 8.0)
            self.assertLess(flank, 0.0, "a bank that dries is an island")

    def test_the_headland_is_high_and_the_water_off_it_is_deep(self):
        headland = self.named("headland")
        self.assertGreater(self.world.structural_m(headland.at), 50.0)
        steep = self.world.structural_m(self.coast.at(3_500.0, 15_000.0))
        self.assertLess(steep, -25.0)
        # Steep-to means the depth is there because the shore is, not because of distance.
        ordinary = self.world.structural_m(self.coast.at(3_500.0, 0.0))
        self.assertGreater(ordinary, steep + 15.0)

    def test_a_laden_hull_can_reach_the_harbour_but_not_over_the_bar(self):
        """
        Two draughts, one voyage. The whole point of the arrangement is that the answer
        depends on the ship.

        Sampled every two hundred metres, not every kilometre. A bar twelve hundred metres
        across is perfectly capable of hiding between kilometre marks, which is the same
        sampling argument the marks layer is built on, arriving in a test that thought it
        was about draught.

        """
        leading_line = [self.coast.at(step * 200.0) for step in range(0, 100)]
        depths = [-self.world.structural_m(point) for point in leading_line]
        self.assertGreater(min(depths), 2.0, "the fairway dries somewhere")
        self.assertLess(
            min(depths), 4.0, "nothing on the fairway is shoal enough to matter"
        )


class TestDetailDefersToWhatWasPlaced(RegionTestCase):
    def test_a_stated_depth_survives_canonical_evaluation(self):
        """
        Coastal roughness runs to thirty-five metres. A bar stated three metres proud of
        the bottom does not survive that unless detail is told to get out of the way, and
        a bar nobody can find is not a bar.

        """
        for kind in ("harbour bar", "drying rock", "pinnacle"):
            feature = self.named(kind)
            self.assertAlmostEqual(
                self.world.elevation_m(feature.at),
                feature.target_m,
                delta=0.6,
                msg=kind,
            )

    def test_detail_still_works_where_nothing_was_placed(self):
        """The deference is local. Silencing texture everywhere would be a cure worse."""
        moved = 0
        alongshore = range(14, 34)
        for km in alongshore:
            point = self.coast.at(12_000.0, km * 1000.0)
            self.assertEqual(
                self.region.features.apply(point, 0.0)[1], 0.0, "chose a line with features"
            )
            if abs(self.world.elevation_m(point) - self.world.structural_m(point)) > 1.0:
                moved += 1
        self.assertGreater(moved, len(alongshore) * 0.6)


class TestTheMarksLayerEarnsItsExistence(RegionTestCase):
    """
    The argument for the second channel, made as a measurement rather than an assertion.

    If a chart that samples terrain could find an isolated pinnacle, `marks_near` would be
    ornamental. These tests show it cannot, and - worse - that whether it finds one depends
    on where the sample grid happens to fall, so the danger would appear and disappear as
    a ship moved.
    """

    def soundings(self, spacing_m, phases=8, reach_m=1_200.0):
        """
        The shallowest sounding a chart would print near the rock, over many grid phases.

        Notes:
            A chart is centred on the ship, so the grid phase relative to a fixed rock is
            arbitrary and changes as she moves. Sweeping it is the measurement; one grid
            says only where that one grid happened to fall - and centred on the rock,
            which is how this was first written, it finds it every time.

            The box is kept to a mile so the answer is about the pinnacle. Widened to six,
            the shallowest sounding in it was a bank four kilometres away, and the test
            passed while measuring nothing whatsoever.

        """
        frame = TangentFrame.at(self.named("pinnacle").at)
        half = int(reach_m // spacing_m)
        readings = []
        for row_phase in range(phases):
            for col_phase in range(phases):
                east = col_phase * spacing_m / phases
                north = row_phase * spacing_m / phases
                shallowest = -9e9
                for row in range(-half, half + 1):
                    for col in range(-half, half + 1):
                        point = frame.local_to_sphere(
                            col * spacing_m + east, row * spacing_m + north
                        )
                        shallowest = max(
                            shallowest, self.world.elevation_m(point, spacing_m)
                        )
                readings.append(shallowest)
        return readings

    def test_a_sampled_chart_misses_the_pinnacle(self):
        """
        Sixty-three grids in sixty-four print twenty metres of water over a rock with
        three and a half on it. That is the whole argument for the marks layer, and it is
        a measurement rather than a claim.

        """
        readings = self.soundings(400.0)
        found = [reading for reading in readings if reading > -12.0]
        self.assertLess(
            len(found), len(readings) * 0.1,
            "a four-hundred-metre grid keeps finding a hundred-and-forty-metre rock, so "
            "either the rock is too big to be the argument or the box is too wide to be "
            "measuring the rock",
        )
        # Stated against the rock rather than against a depth, so it keeps meaning what it
        # means if the region is ever moved to a different piece of coast.
        self.assertLess(min(readings), self.named("pinnacle").target_m - 10.0)

    def test_physics_never_misses_it(self):
        pinnacle = self.named("pinnacle")
        self.assertAlmostEqual(self.world.elevation_m(pinnacle.at), -3.5, delta=0.6)

    def test_whether_sampling_finds_it_depends_on_where_the_grid_falls(self):
        """
        The part that makes it a correctness problem rather than a fidelity one. A chart
        centred on the ship moves with her, so a hazard found by one grid and missed by
        the next would blink - and a hazard that blinks is worse than one never drawn.

        """
        readings = self.soundings(200.0)
        self.assertGreater(max(readings) - min(readings), 10.0)

    def test_sampling_finer_finds_it_more_often_but_never_reliably(self):
        """
        The tempting fix, measured and rejected. Chart resolution buys hit rate, not
        certainty: even at a hundred metres - a quarter the cell area and four times the
        cost - most grids still print deep water over the rock.

        """
        rates = []
        for spacing in (400.0, 200.0, 100.0):
            readings = self.soundings(spacing)
            rates.append(sum(1 for r in readings if r > -12.0) / len(readings))
        for coarser, finer in zip(rates, rates[1:]):
            self.assertGreater(finer, coarser)
        self.assertLess(max(rates), 0.6)

    def test_the_marks_layer_finds_it_from_anywhere_in_the_region(self):
        for km in (2, 8, 15, 25, 40):
            marks = self.region.features.marks_near(self.coast.at(km * 1000.0), 60_000.0)
            kinds = [feature.kind for _, feature in marks]
            self.assertIn("pinnacle", kinds)
            self.assertIn("drying rock", kinds)

    def test_marks_come_back_nearest_first_and_only_when_near(self):
        near = self.region.features.marks_near(self.named("pinnacle").at, 60_000.0)
        self.assertEqual(near[0][1].kind, "pinnacle")
        distances = [distance for distance, _ in near]
        self.assertEqual(distances, sorted(distances))
        self.assertEqual(self.region.features.marks_near(self.coast.at(0.0), 100.0), ())

    def under_reported_m(self, feature, spacing_m=400.0):
        """
        How much shallower the ground is than the chart says, over one feature.

        Returns:
            metres (float): Positive means the chart is *optimistic* - it prints more
                water than there is, which is the only direction that drowns anybody.

        Notes:
            Sampled on a lattice anchored to the region, because a chart does not get to
            choose where its grid falls relative to what it is drawing.

        """
        frame = TangentFrame.at(self.region.origin)
        east, north = frame.sphere_to_local(feature.at)
        base_col, base_row = round(east / spacing_m), round(north / spacing_m)
        charted = -9e9
        for row in (base_row - 1, base_row, base_row + 1):
            for col in (base_col - 1, base_col, base_col + 1):
                point = frame.local_to_sphere(col * spacing_m, row * spacing_m)
                charted = max(charted, self.world.elevation_m(point, spacing_m))
        return self.world.elevation_m(feature.at) - charted

    def test_a_feature_is_marked_exactly_when_a_chart_would_lie_about_it(self):
        """
        The rule, derived rather than declared - and the version of this test that came
        before it was a size heuristic that got the answer wrong.

        "Mark anything under five hundred metres across" marked the pinnacle and the
        drying rock and stopped. It left the moles, which are two kilometres long and
        three hundred and forty wide, so a four-hundred-metre grid prints six metres of
        water over a four-metre breakwater. It left the harbour bar, over whose
        three-metre crest the same grid prints seven.

        What matters is not how big a feature is but whether sampling it tells the truth.
        Measured for every feature in the region, the two sets separate cleanly - nothing
        marked is under-reported by less than four metres, nothing unmarked by more than
        nothing at all - so the rule can be stated as a measurement and checked.

        """
        for feature in self.region.features:
            lie = self.under_reported_m(feature)
            if feature.marked:
                self.assertGreater(
                    lie, 2.0,
                    f"{feature.kind} is marked but a chart describes it perfectly well",
                )
            else:
                self.assertLess(
                    lie, 2.0,
                    f"{feature.kind} is not marked and a chart prints {lie:.1f} m more "
                    f"water over it than there is",
                )


class TestTheRestOfTheWorldIsUntouched(RegionTestCase):
    def test_a_featureless_world_is_bit_identical(self):
        points = scattered(300)
        plain = Surface(WORLD_SEED, features=None)
        for point in points:
            self.assertEqual(plain.elevation_m(point), self.bare.elevation_m(point))

    def test_features_change_nothing_outside_their_reach(self):
        changed = 0
        for point in scattered(2000):
            if self.region.origin.distance_to(point) < 100_000.0:
                continue
            if self.world.elevation_m(point) != self.bare.elevation_m(point):
                changed += 1
        self.assertEqual(changed, 0)

    def test_the_region_sits_where_it_was_put(self):
        self.assertTrue(self.region.covers(self.coast.at(0.0)))
        self.assertTrue(self.region.covers(self.coast.at(40_000.0)))
        self.assertFalse(self.region.covers(self.coast.at(200_000.0)))

    def test_the_order_of_asking_changes_nothing(self):
        points = [self.coast.at(km * 500.0, km * 137.0) for km in range(-40, 41)]
        forward = [self.world.elevation_m(p) for p in points]
        backward = list(reversed([self.world.elevation_m(p) for p in reversed(points)]))
        self.assertEqual(forward, backward)


class TestFeaturesInGeneral(unittest.TestCase):
    def test_an_empty_set_does_nothing_at_all(self):
        empty = Features()
        self.assertEqual(len(empty), 0)
        at = SpherePoint.from_latlon(0.0, 0.0)
        self.assertEqual(empty.apply(at, -12.5), (-12.5, 0.0))

    def test_a_feature_at_a_pole_is_ordinary(self):
        """No basis is defined at a pole, so a feature there exercises the fallback."""
        for latitude in (90.0, -90.0):
            at = SpherePoint.from_latlon(latitude, 0.0)
            only = Features([Feature("cap", at, 500.0, 40_000.0, 40_000.0)])
            self.assertAlmostEqual(only.apply(at, 0.0)[0], 500.0, places=6)
            for longitude in (0.0, 90.0, -170.0):
                near = SpherePoint.from_latlon(latitude - math.copysign(0.2, latitude),
                                               longitude)
                self.assertTrue(math.isfinite(only.apply(near, 0.0)[0]))

    def test_a_feature_across_the_dateline_has_no_seam(self):
        at = SpherePoint.from_latlon(10.0, 180.0)
        only = Features([Feature("shoal", at, -4.0, 30_000.0, 30_000.0, compose=RAISE)])
        east = only.apply(SpherePoint.from_latlon(10.0, 179.95), -40.0)[0]
        west = only.apply(SpherePoint.from_latlon(10.0, -179.95), -40.0)[0]
        self.assertAlmostEqual(east, west, places=3)

    def test_a_feature_with_no_extent_does_nothing(self):
        at = SpherePoint.from_latlon(0.0, 0.0)
        only = Features([Feature("nothing", at, 900.0, 0.0, 0.0)])
        self.assertEqual(only.apply(at, -20.0), (-20.0, 0.0))


if __name__ == "__main__":
    unittest.main()
