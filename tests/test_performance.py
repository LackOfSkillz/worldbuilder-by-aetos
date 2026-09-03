"""
The performance gate, and the honest position on it.

Maritime does not read a map. It asks how high the ground is at a point, and a chart redraw
asks 9,216 times. That number is the whole reason this generator is a function of position
rather than a stored heightmap, so it is the number the generator has to answer to.

**Where it stands, measured end to end in a real Evennia environment** with maritime's own
`client.cartography.sample`, against the same chart, on the same machine:

    grid            soundings   a hand-written ramp   the generated planet
    96 x 96             9216               18.0 ms               725.4 ms
    48 x 48             2304                4.7 ms               163.9 ms
    32 x 32             1024                2.1 ms                70.5 ms

A canonical sample costs about eighty microseconds. A hand-written seabed costs about two.

**That gap is not going to be closed by making Python faster**, and this file exists partly
to say so with numbers rather than leave it as a surprise. A terrain sample evaluates
thirty-nine octaves of noise plus the plate geometry; even perfectly written, that is tens
of microseconds in this language. What closed half of it already was ordinary care -
caching lattice cells rather than corners, unrolling the vector algebra on the two hottest
paths, and slotting the two hottest types - for a 1.5x speedup and *bit-identical output*,
which was checked by hashing every value the world produces before and after.

Closing the rest means choosing something, and each choice costs something that is not
performance:

    coarser charts       32x32 puts a redraw at 70 ms, and a chart at 600 m a sounding
                         is a real chart. Costs chart detail; costs no accuracy at all,
                         because the marks layer already carries what sampling misses.

    a coarse lattice     continentality's finest structure is 640 km and the gradient of
    for the slow fields  it is a third of the cost. Sampling it on a fixed world-anchored
                         lattice and interpolating with smoothstep - the same trick the
                         noise itself uses, and C1 for the same reason - would be an order
                         of magnitude. Costs a small change to every elevation in the
                         world, which needs measuring before it is chosen.

    an analytic gradient the four probes behind the shelf are sixteen of the thirty-nine
                         noise evaluations. The derivative of a smoothstep-trilinear fbm
                         has a closed form costing about one evaluation. More accurate
                         than what it replaces; still changes every coastline slightly.

    caching in the       an anchored ship redraws the same grid. An exact cache on the
    provider             position pair makes repeats free and changes nothing at all -
                         but does nothing for the first redraw or for a ship under way.

The tests below are a **regression gate**, not a budget assertion. A hard assert on
microseconds would fail on a loaded machine and teach everybody to ignore it; what is
guarded is that the shape of the cost stays right and that nothing doubles overnight.
"""

import time
import unittest

from worldbuilder.geometry.tangent import TangentFrame
from worldbuilder.regions.demo import WORLD_SEED, demo_region
from worldbuilder.terrain import noise as noise_module
from worldbuilder.terrain.surface import Surface

#: One maritime chart redraw, as maritime actually draws it.
CHART_GRID = 96
CHART_SPACING_M = 400.0

#: Where a sample has to stay under. Generous on purpose: this is a ceiling that catches a
#: regression, not a budget that says the generator is fast enough. It is not - see the
#: table above - and pretending otherwise with a tight number that fails on a busy machine
#: would only teach everybody to skip the test.
CEILING_US = 260.0


class ChartTestCase(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.region = demo_region()
        cls.world = Surface(WORLD_SEED, features=cls.region.features)
        frame = TangentFrame.at(cls.region.origin)
        half = CHART_GRID / 2.0
        cls.points = [
            frame.local_to_sphere(
                (col - half) * CHART_SPACING_M, (row - half) * CHART_SPACING_M
            )
            for row in range(CHART_GRID)
            for col in range(CHART_GRID)
        ]
        # The noise lattice fills in as it is used, so a cold first pass would be timing
        # the cache rather than the arithmetic. Every measurement here is of warm code,
        # which is also the state a running game is in.
        for point in cls.points[::7]:
            cls.world.elevation_m(point)

    def timed(self, work):
        start = time.perf_counter()
        for point in self.points:
            work(point)
        return (time.perf_counter() - start) / len(self.points) * 1e6


class TestWhatAChartCosts(ChartTestCase):
    def test_the_table_that_decides_everything(self):
        costs = {
            "continentality": lambda p: self.world.land.at(p),
            "tectonics": lambda p: self.world.tectonics.elevation_m(p),
            "shelf": lambda p: self.world.shelf.elevation_m(p),
            "structural": self.world.structural_m,
            "canonical": self.world.elevation_m,
            "at 400 m": lambda p: self.world.elevation_m(p, CHART_SPACING_M),
            "at 10 km": lambda p: self.world.elevation_m(p, 10_000.0),
        }
        measured = {name: self.timed(work) for name, work in costs.items()}

        print(f"\n    a {CHART_GRID} x {CHART_GRID} chart, {len(self.points)} soundings:")
        for name, each in measured.items():
            print(f"      {name:16} {each:7.2f} us a sample "
                  f"{each * len(self.points) / 1000:8.1f} ms a redraw")

        self.assertLess(measured["canonical"], CEILING_US)

    def test_each_layer_costs_more_than_the_one_under_it(self):
        """
        The shape of the cost, which is what a regression usually breaks first. If the
        shelf ever came out cheaper than the tectonics it sits on, something is being
        skipped rather than computed.

        """
        order = ["continentality", "tectonics", "shelf"]
        works = {
            "continentality": lambda p: self.world.land.at(p),
            "tectonics": lambda p: self.world.tectonics.elevation_m(p),
            "shelf": lambda p: self.world.shelf.elevation_m(p),
        }
        measured = [self.timed(works[name]) for name in order]
        for cheaper, dearer in zip(measured, measured[1:]):
            self.assertGreater(dearer, cheaper * 0.9)

    def _noise_evaluations(self, work):
        """
        How many times the noise is sampled, which is what band-limiting actually skips.

        """
        original = noise_module.Noise.at
        counted = 0

        def counting(inner_self, *args, **keywords):
            nonlocal counted
            counted += 1
            return original(inner_self, *args, **keywords)

        noise_module.Noise.at = counting
        try:
            for point in self.points[::16]:
                work(point)
        finally:
            noise_module.Noise.at = original
        return counted

    def test_a_coarse_chart_does_less_work_than_a_canonical_one(self):
        """
        Band-limiting has to do real work, or `resolution_m` is a parameter that lies.

        This counts noise evaluations rather than timing them, and the reason is
        measured rather than stylistic. The saving is real but small - 22,464
        evaluations against 18,432, so 17.9% fewer - while the wall-clock gap is
        nearer 10%, because what band-limiting skips is the cheap end of the octave
        stack. On this machine the two timing distributions **overlap**: over nine
        samples each, the fastest canonical pass (94.10 us) came in slower than the
        slowest coarse one (114.69 us), and 11 of 81 pairings had the coarse chart
        looking dearer. Taking the minimum of five passes each did not fix it either
        - still 2 of 16, with the ranges still touching.

        So this claim cannot be measured by wall clock on a machine doing anything
        else, and the test said so by failing intermittently for nine slices. The
        count is exact, repeats to the evaluation, and tests the claim the docstring
        actually makes.

        """
        canonical = self._noise_evaluations(self.world.elevation_m)
        coarse = self._noise_evaluations(
            lambda point: self.world.elevation_m(point, 10_000.0)
        )
        self.assertLess(coarse, canonical)

    def test_a_bottom_costs_a_few_soundings_and_no_more(self):
        """
        Four probes and a frame. Much more than that means something is being recomputed
        that a caller already had.

        """
        sample = self.points[::16]
        start = time.perf_counter()
        for point in sample:
            self.world.bottom_at(point)
        bottom = (time.perf_counter() - start) / len(sample) * 1e6

        canonical = self.timed(self.world.elevation_m)
        print(f"\n    a bottom is {bottom / canonical:.1f} soundings")
        self.assertLess(bottom, canonical * 9.0)


class TestTheOptimisationsAreFree(ChartTestCase):
    """
    Everything done for speed so far changed no value. These tests are how that stays true.

    Each of them would have caught one of the three changes going wrong: the lattice cache
    returning corners in the wrong order, the unrolled projection getting an operation out
    of sequence, the unrolled plate sweep comparing against the wrong seed.
    """

    def test_the_lattice_cache_returns_what_the_lattice_says(self):
        from worldbuilder.terrain.noise import Noise, _lattice

        noise = Noise(WORLD_SEED, salt=1234)
        for ix, iy, iz in ((0, 0, 0), (3, -7, 11), (-40, 40, -1)):
            cached = noise._fill((ix, iy, iz))
            direct = (
                _lattice(ix, iy, iz, noise.seed),
                _lattice(ix + 1, iy, iz, noise.seed),
                _lattice(ix, iy + 1, iz, noise.seed),
                _lattice(ix + 1, iy + 1, iz, noise.seed),
                _lattice(ix, iy, iz + 1, noise.seed),
                _lattice(ix + 1, iy, iz + 1, noise.seed),
                _lattice(ix, iy + 1, iz + 1, noise.seed),
                _lattice(ix + 1, iy + 1, iz + 1, noise.seed),
            )
            self.assertEqual(cached, direct)

    def test_the_unrolled_projection_still_inverts(self):
        frame = TangentFrame.at(self.region.origin)
        for east in (-60_000.0, -137.0, 0.0, 137.0, 60_000.0):
            for north in (-60_000.0, 0.0, 60_000.0):
                back = frame.sphere_to_local(frame.local_to_sphere(east, north))
                self.assertAlmostEqual(back[0], east, places=3)
                self.assertAlmostEqual(back[1], north, places=3)

    def test_the_unrolled_plate_sweep_agrees_with_the_vector_algebra(self):
        plates = self.world.plates
        for point in self.points[::311]:
            nearest, second = plates.nearest_two(point)
            by_hand = sorted(
                plates.plates,
                key=lambda plate: -point.vector.dot(plate.seed.vector),
            )
            self.assertIs(nearest, by_hand[0])
            self.assertIs(second, by_hand[1])

    def test_a_margin_normal_is_still_a_vector(self):
        """
        The unrolled sweep works in component triples. If one leaked out of the function,
        every caller taking a bearing off a margin would fail somewhere much less obvious.

        """
        for point in self.points[::311]:
            _, margins = self.world.plates.margins_within(point, 420_000.0)
            for _, _, normal, _ in margins:
                self.assertTrue(hasattr(normal, "x"), "a normal came back as a tuple")
                self.assertAlmostEqual(normal.length(), 1.0, places=9)


if __name__ == "__main__":
    unittest.main()
