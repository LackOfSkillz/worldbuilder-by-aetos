"""
Throwaway spike for slice 1n, Task 1. Deleted in Task 6.

Four questions, each answered by measurement rather than by argument, and each reporting
the population it was measured over. The trap this file exists to avoid is a clean number
from the wrong corpus: an area-uniform planetary scatter reports `natural`'s slope clamp
as unreachable, and the same clamp saturates eight times over inside the demo world's
hundred-and-forty-metre pinnacle. So every scan below that touches a clamp, a gate or a
tie is a feature-local scan run at three resolutions, and every extremum is quoted with
the resolution that found it.

Run with `-s` to print the measured numbers rather than only the assertions.
"""

import math
import unittest

from worldbuilder.bathymetry.substrate import (
    MUD,
    ROCK,
    ROCK_SLOPE,
    ROCK_TECTONIC_M,
    SAND,
    SETTLED_M,
    SWEPT_M,
    Composition,
    Substrate,
    _smooth,
)
from worldbuilder.geometry.sphere import SpherePoint
from worldbuilder.regions.demo import WORLD_SEED, demo_region
from worldbuilder.terrain.surface import Surface

#: Where the two small steep features stand, as the demo region places them. Offshore and
#: alongshore metres from the coast anchor, and the larger of the two dimensions.
PINNACLE = (8_000.0, 6_500.0, 70.0)
DRYING_ROCK = (4_000.0, -5_000.0, 45.0)


def scattered(count):
    """Area-uniform points over the whole sphere, by the golden-angle spiral."""
    golden = math.pi * (3.0 - math.sqrt(5.0))
    points = []
    for index in range(count):
        z = 1.0 - 2.0 * (index + 0.5) / count
        theta = golden * index
        points.append(
            SpherePoint.from_latlon(
                math.degrees(math.asin(z)),
                (math.degrees(theta) + 180.0) % 360.0 - 180.0,
            )
        )
    return points


class Recorder:
    """
    A proxy that records every attribute read through it.

    Grep finds textual reaches. This finds them at runtime, which is the check that would
    catch a member reached through a helper, a comprehension or a `getattr`. The two are
    run against each other below, because an undercount reads exactly like an absence.
    """

    def __init__(self, wrapped, seen, prefix=""):
        object.__setattr__(self, "_wrapped", wrapped)
        object.__setattr__(self, "_seen", seen)
        object.__setattr__(self, "_prefix", prefix)

    def __getattr__(self, name):
        wrapped = object.__getattribute__(self, "_wrapped")
        seen = object.__getattribute__(self, "_seen")
        prefix = object.__getattribute__(self, "_prefix")
        seen.add(prefix + name)
        value = getattr(wrapped, name)
        if name in ("tectonics", "features"):
            return Recorder(value, seen, prefix + name + ".")
        return value


class World(unittest.TestCase):
    """The demo world, built once. Every measurement below shares it."""

    @classmethod
    def setUpClass(cls):
        cls.region = demo_region()
        cls.surface = Surface(WORLD_SEED, features=cls.region.features)
        cls.substrate = cls.surface.substrate
        cls.coast = cls.region.coast


class HostSurface(World):
    """Step 1: enumerate the host surface from evidence, at runtime."""

    def exercise(self, substrate, point):
        """Every entry point and every combination of the optionals."""
        substrate.at(point)
        substrate.at(point, elevation_m=-9.0)
        substrate.at(point, slope=0.01)
        substrate.at(point, tectonic_m=100.0)
        substrate.at(point, elevation_m=-9.0, slope=0.01)
        substrate.at(point, elevation_m=-9.0, tectonic_m=100.0)
        substrate.at(point, slope=0.01, tectonic_m=100.0)
        substrate.at(point, elevation_m=-9.0, slope=0.01, tectonic_m=100.0)
        substrate.dominant_at(point)
        substrate.dominant_at(point, elevation_m=-9.0)
        substrate.slope_at(point)
        substrate.slope_at(point, baseline_m=600.0)

    def test_host_surface_is_exactly_four_members_wide(self):
        seen = set()
        substrate = Substrate(Recorder(self.surface, seen))
        # Probed at three places, so no branch that only a particular ground reaches is
        # left unexercised: inside the basin, on the pinnacle, and in open ocean.
        self.exercise(substrate, self.coast.at(-2_000.0, 0.0))
        self.exercise(substrate, self.coast.at(*PINNACLE[:2]))
        self.exercise(substrate, SpherePoint.from_latlon(0.0, 0.0))
        self.assertEqual(
            seen,
            {"radius_m", "structural_m", "tectonics", "tectonics.offset_m",
             "features", "features.placed"},
        )

    def test_the_six_named_members_are_unreached_and_do_exist(self):
        seen = set()
        substrate = Substrate(Recorder(self.surface, seen))
        self.exercise(substrate, self.coast.at(-2_000.0, 0.0))
        self.exercise(substrate, self.coast.at(*PINNACLE[:2]))
        for member in ("shelf", "detail", "land", "plates", "elevation_m", "bottom_at"):
            # `hasattr` first, so the assertion below is "unreached", not "misspelled".
            self.assertTrue(hasattr(self.surface, member), member)
            self.assertNotIn(member, seen, member)


class NaturalCannotTripTheGuard(World):
    """Step 3: whether `natural` can produce a total that trips `total <= 0.0`."""

    @staticmethod
    def total_of(elevation_m, slope, tectonic_m):
        """The pre-normalisation total, rebuilt exactly as `natural` builds it."""
        rock = max(
            _smooth(slope / ROCK_SLOPE),
            _smooth(abs(tectonic_m) / ROCK_TECTONIC_M),
        )
        swept = _smooth((elevation_m - SETTLED_M) / (SWEPT_M - SETTLED_M))
        loose = 1.0 - rock
        return loose * swept + loose * (1.0 - swept) + rock

    def test_the_argument_domain_itself_never_reaches_zero(self):
        """
        Population: the reachable argument domain, swept directly rather than sampled.

        `rock` and `swept` are both `_smooth` outputs, so both lie in [0, 1] whatever
        the arguments were -- the clamps make that unconditional. The total is therefore
        a function of those two alone, and a grid over the unit square is the domain
        rather than a sample of it. 1,001 x 1,001 = 1,002,001 pairs.
        """
        worst = math.inf
        steps = 1_000
        for i in range(steps + 1):
            rock = i / steps
            loose = 1.0 - rock
            for j in range(steps + 1):
                swept = j / steps
                worst = min(worst, loose * swept + loose * (1.0 - swept) + rock)
        print(f"\n[natural] argument-domain sweep, 1,002,001 (rock, swept) "
              f"pairs: min total = {worst!r}")
        # Two ULP below 1.0, not exactly 1.0: `loose * swept + loose * (1 - swept)` does
        # not re-sum to `loose`. Nowhere near the guard -- but it is not identically one,
        # and a port that assumed it was would be assuming something false.
        self.assertGreater(worst, 0.999_999_999_999_999)
        self.assertLess(worst, 1.0)

    def test_planetary_scatter_never_reaches_zero(self):
        """
        Population: 3,000 area-uniform planetary probes over the demo world. This is the
        population `bottom_at` is asked about at a random ship position, and it is the
        WRONG population for any clamp -- recorded here only to state the contrast.
        """
        worst = math.inf
        for point in scattered(3_000):
            worst = min(
                worst,
                self.total_of(
                    self.surface.structural_m(point),
                    self.substrate.slope_at(point),
                    self.surface.tectonics.offset_m(point),
                ),
            )
        print(f"[natural] planetary scatter n=3,000: min total = {worst!r}")
        self.assertGreater(worst, 0.99)

    def test_small_steep_feature_scan_never_reaches_zero(self):
        """
        Population: the two small steep features, scanned both as a line through the
        centre and as a 2-D grid, at three resolutions each. This is the population that
        saturates the slope clamp, and the planetary scatter above misses it entirely --
        the assertion at the end is what makes that visible.

        **The line scan and the grid are not interchangeable.** A `Placed` weight is a
        product of two `bump` factors, so the steepest ground is off the feature's axis;
        a line through the centre undercuts the grid's peak by about six per cent on the
        pinnacle and twenty on the drying rock. Both are recorded so the gap is on the
        record rather than the cheaper one being mistaken for the answer.
        """
        worst = math.inf
        peaks = {}
        for name, (offshore, along, size) in (
            ("pinnacle", PINNACLE),
            ("drying rock", DRYING_ROCK),
        ):
            span = 2.0 * size
            for steps in (40, 120, 400):
                peak = 0.0
                for i in range(steps + 1):
                    offset = -span * 0.5 + span * i / steps
                    point = self.coast.at(offshore + offset, along)
                    slope = self.substrate.slope_at(point)
                    peak = max(peak, slope / ROCK_SLOPE)
                    worst = min(
                        worst,
                        self.total_of(
                            self.surface.structural_m(point),
                            slope,
                            self.surface.tectonics.offset_m(point),
                        ),
                    )
                peaks[(name, "line", steps, span / steps)] = peak
            for steps in (40, 120, 300):
                peak = 0.0
                for i in range(steps + 1):
                    for j in range(steps + 1):
                        offset = -span + 2.0 * span * i / steps
                        sideways = -span + 2.0 * span * j / steps
                        point = self.coast.at(offshore + offset, along + sideways)
                        slope = self.substrate.slope_at(point)
                        peak = max(peak, slope / ROCK_SLOPE)
                        worst = min(
                            worst,
                            self.total_of(
                                self.surface.structural_m(point),
                                slope,
                                self.surface.tectonics.offset_m(point),
                            ),
                        )
                peaks[(name, "grid", steps, 2.0 * span / steps)] = peak
        for key, value in sorted(peaks.items()):
            print(f"[natural] {key[0]:12} {key[1]:4} steps={key[2]:4} "
                  f"{key[3]:7.3f} m/step  max slope/ROCK_SLOPE = {value:.4f}")
        print(f"[natural] small-steep-feature scans: min total = {worst!r}")
        self.assertGreater(worst, 0.99)
        # The corpus is only the right one if it really does saturate the clamp.
        self.assertGreater(max(peaks.values()), 8.0)
        # And a line through the centre is genuinely weaker than the grid.
        self.assertGreater(
            peaks[("pinnacle", "grid", 300, 2.0 * 140.0 / 300)],
            peaks[("pinnacle", "line", 400, 140.0 / 400)],
        )

    def test_direct_construction_is_the_only_reachable_path_to_the_guard(self):
        """The guard is live -- but only from a caller building a `Composition` itself."""
        self.assertEqual(Composition(0.0, 0.0, 0.0).dominant, ROCK)
        self.assertEqual(Composition(-1.0, 0.5, 0.4).dominant, ROCK)
        self.assertEqual(Composition(1e-300, -1e-300, 0.0).dominant, ROCK)
        # One ULP above zero and the fallback does not fire, which is the cliff.
        self.assertEqual(Composition(5e-324, 0.0, 0.0).dominant, SAND)


class DominantTieMargins(World):
    """Step 4: how close to a tie the field gets, over what, and at what resolution."""

    @staticmethod
    def margin(composition):
        ordered = sorted(
            (composition.sand, composition.mud, composition.rock), reverse=True
        )
        return ordered[0] - ordered[1]

    def test_tie_precedence_is_rock_then_sand(self):
        third = 1.0 / 3.0
        self.assertEqual(Composition(third, third, third).dominant, ROCK)
        self.assertEqual(Composition(0.5, 0.5, 0.0).dominant, SAND)
        self.assertEqual(Composition(0.0, 0.5, 0.5).dominant, ROCK)
        self.assertEqual(Composition(0.5, 0.0, 0.5).dominant, ROCK)
        self.assertEqual(Composition(0.0, 1.0, 0.0).dominant, MUD)

    def test_planetary_scatter_margin_is_a_property_of_the_scan(self):
        """
        Population: area-uniform planetary scatter, at two densities. Running two is the
        whole point -- the minimum falls as the scan grows, which is the ~1/n behaviour
        of a codimension-1 boundary curve, so no single figure here is a module property.
        """
        minima = {}
        for count in (2_000, 8_000):
            worst = math.inf
            for point in scattered(count):
                worst = min(worst, self.margin(self.substrate.at(point)))
            minima[count] = worst
            print(f"\n[dominant] planetary scatter n={count}: min margin = {worst:.6e}")
        self.assertLess(minima[8_000], minima[2_000])
        self.assertGreater(minima[8_000], 1e-6)

    def test_a_line_through_a_feature_does_not_beat_the_planetary_scatter(self):
        """
        Population: a 600 m line scan across the pinnacle, at three resolutions.

        **This is the negative result, and it is why the grid below exists.** The line
        bottoms out at 6.6e-3 -- indistinguishable from the 8,000-point planetary
        scatter, and no better at 3,200 steps than at 800. A tie surface is a curve on
        the sphere; a line crosses it transversally at isolated points and the nearest
        probe is a matter of luck. The clamp's corpus and the tie's corpus are not the
        same corpus, and reusing the one for the other reads as a clean result.
        """
        offshore, along, _ = PINNACLE
        minima = {}
        for steps in (200, 800, 3_200):
            worst = math.inf
            for i in range(steps + 1):
                offset = -300.0 + 600.0 * i / steps
                worst = min(
                    worst,
                    self.margin(self.substrate.at(self.coast.at(offshore + offset, along))),
                )
            minima[steps] = worst
            print(f"[dominant] pinnacle LINE 600 m, steps={steps:5} "
                  f"({600.0 / steps:6.3f} m/step, n={steps + 1:5}): "
                  f"min margin = {worst:.6e}")
        self.assertGreater(minima[3_200], 1e-3)

    def test_feature_local_grids_beat_the_planet_by_two_orders(self):
        """
        Population: 2-D grids over the pinnacle, the drying rock and the harbour basin,
        each at three resolutions. A grid samples a 2-D neighbourhood of the tie curve
        instead of puncturing it, so it closes on the curve as the step shrinks -- which
        the line above does not.
        """
        minima = {}
        for name, offshore, along, half in (
            ("pinnacle", PINNACLE[0], PINNACLE[1], 140.0),
            ("drying rock", DRYING_ROCK[0], DRYING_ROCK[1], 90.0),
            ("harbour basin", -2_000.0, 0.0, 3_000.0),
        ):
            for steps in (40, 120, 240):
                worst = math.inf
                for i in range(steps + 1):
                    for j in range(steps + 1):
                        offset = -half + 2.0 * half * i / steps
                        sideways = -half + 2.0 * half * j / steps
                        worst = min(
                            worst,
                            self.margin(
                                self.substrate.at(
                                    self.coast.at(offshore + offset, along + sideways)
                                )
                            ),
                        )
                minima[(name, steps)] = worst
                print(f"[dominant] {name:14} GRID +-{half:6.0f} m, steps={steps:4} "
                      f"({2.0 * half / steps:7.3f} m/step, n={(steps + 1) ** 2:6}): "
                      f"min margin = {worst:.6e}")
        # Every grid closes on the tie curve as it refines, and the finest beats the
        # 8,000-point planetary scatter by more than an order of magnitude.
        for name in ("pinnacle", "drying rock", "harbour basin"):
            self.assertLess(minima[(name, 240)], minima[(name, 40)], name)
            self.assertLess(minima[(name, 240)], 1e-3, name)

    def test_the_smallest_margin_lives_on_a_shallow_gradient_not_a_steep_one(self):
        """
        Population: five bisected crossings, chosen to span the gradient range, each run
        to float exhaustion in the offshore coordinate.

        **The steep features are the wrong corpus for this question, and that is the
        opposite of the clamp.** Bisection bottoms out when the offshore coordinate runs
        out of ULPs, so the residual margin is the *composition gradient* times that last
        step in metres. On the pinnacle the composition swings a full word in tens of
        metres, so the last resolvable step still leaves ~1e-11 of margin; out in open
        water the gradient is a thousand times gentler and the same exhaustion leaves
        ~2e-15. Report the shallow number, and say which crossing produced it.
        """
        results = {}
        for label, low, high, along in (
            ("harbour basin radial", -2_000.0, -500.0, 0.0),
            ("pinnacle radial", 7_860.0, 8_000.0, 6_500.0),
            ("drying rock radial", 3_930.0, 4_000.0, -5_000.0),
            ("offshore 20-200 km", 20_000.0, 200_000.0, 0.0),
        ):
            low_word = self.substrate.dominant_at(self.coast.at(low, along))
            high_word = self.substrate.dominant_at(self.coast.at(high, along))
            self.assertNotEqual(low_word, high_word, label)
            for _ in range(400):
                middle = (low + high) * 0.5
                if middle == low or middle == high:
                    break
                if self.substrate.dominant_at(self.coast.at(middle, along)) == low_word:
                    low = middle
                else:
                    high = middle
            margin = min(
                self.margin(self.substrate.at(self.coast.at(low, along))),
                self.margin(self.substrate.at(self.coast.at(high, along))),
            )
            results[label] = margin
            print(f"[dominant] {label:22} {low_word:4}->{high_word:4} "
                  f"sides {high - low:.4e} m apart: min margin = {margin:.6e}")
        self.assertLess(results["offshore 20-200 km"], 1e-14)
        self.assertGreater(results["pinnacle radial"], 1e-12)
        self.assertLess(results["offshore 20-200 km"], results["pinnacle radial"])


if __name__ == "__main__":
    unittest.main()
