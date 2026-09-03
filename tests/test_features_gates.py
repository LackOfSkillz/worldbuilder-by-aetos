"""
Throwaway spike for slice 1m, Task 1. Deleted in Task 6.

Three questions the rest of the slice depends on, answered by measurement rather than
by reading:

1. Which transcendentals each of `weight_at`, `apply`, `marks_near` and `reach_m`
   actually reaches, directly and indirectly -- established by removing each candidate
   from `math` and seeing which paths still run, not by grepping for call sites.
2. Whether the RAISE/CARVE switch in `Features.apply` is bit-continuous at `lift == 0.0`,
   and what one ULP either side of zero costs.
3. Whether the reach gate in `Placed.weight_at` skips a genuine no-op, bit-for-bit, or
   whether it is load-bearing.

Every assertion here is paired with a print, because the artifact of this task is the
numbers, not the green bar. Run with `-s`.
"""

import math
import struct
import sys

import pytest

from worldbuilder.bathymetry.features import (
    CARVE,
    RAISE,
    SETTLE_M,
    Feature,
    Features,
    Placed,
    _bump,
    _smooth,
)
from worldbuilder.geometry import vectors as vectors_module
from worldbuilder.geometry.sphere import EARTH_RADIUS_M, SpherePoint
from worldbuilder.geometry.tangent import TangentFrame

try:
    import worldbuilder_engine as engine
except ImportError:  # pragma: no cover - the Python suite still runs without Rust
    engine = None


def bits(value):
    """The exact 64-bit pattern, so nothing is decided by how a float prints."""
    return struct.unpack("<Q", struct.pack("<d", value))[0]


def ulps_between(a, b):
    """Signed-magnitude bit distance, the same measure test_conformance.py uses."""
    if a == b:
        return 0
    ia, ib = bits(a), bits(b)
    if ia >> 63:
        ia = (1 << 64) - ia
    if ib >> 63:
        ib = (1 << 64) - ib
    return abs(ia - ib)


# ---------------------------------------------------------------------------
# Step 1: what each path actually reaches.
#
# Method: replace one function in the `math` module with a bomb and run the path. The
# modules under test all call `math.foo(...)` by attribute lookup at call time, so a
# swap is seen. A path that runs clean did not reach that function, directly or
# indirectly, on that input -- which is the claim, made behaviourally.
# ---------------------------------------------------------------------------

CANDIDATES = ("sin", "cos", "atan2", "asin", "hypot", "sqrt", "radians", "degrees")


class _Bomb:
    def __init__(self, name):
        self.name = name

    def __call__(self, *args, **kwargs):
        raise AssertionError(f"reached math.{self.name}")


def reached(run):
    """Which of CANDIDATES the callable touches. Returns a sorted tuple of names."""
    found = []
    for name in CANDIDATES:
        original = getattr(math, name)
        setattr(math, name, _Bomb(name))
        try:
            run()
        except AssertionError as exploded:
            if str(exploded).startswith("reached math."):
                found.append(name)
            else:
                raise
        finally:
            setattr(math, name, original)
    return tuple(found)


AT = SpherePoint.from_latlon(12.34, -56.78)
NEARBY = SpherePoint.from_latlon(12.3405, -56.7795)
FAR = SpherePoint.from_latlon(-40.0, 130.0)


def a_feature(**overrides):
    kwargs = dict(
        kind="bar",
        at=AT,
        target_m=-4.0,
        length_m=1200.0,
        width_m=300.0,
        bearing_deg=37.0,
        compose=RAISE,
        marked=True,
    )
    kwargs.update(overrides)
    return Feature(**kwargs)


def test_step1_transcendental_map_of_the_python():
    feature = a_feature()

    # reach_m on its own.
    print("\nreach_m                 ->", reached(feature.reach_m))

    # Construction, which is where the frame and the cosine gate are built.
    print("Placed.__init__         ->", reached(lambda: Placed(feature, EARTH_RADIUS_M)))

    placed = Placed(feature, EARTH_RADIUS_M)

    # weight_at, on a point inside the support (so the projection is reached) and on one
    # outside it (so the gate short-circuits before the projection).
    inside = reached(lambda: placed.weight_at(NEARBY))
    outside = reached(lambda: placed.weight_at(FAR))
    print("weight_at (inside)      ->", inside)
    print("weight_at (gated out)   ->", outside)

    world = Features([feature], EARTH_RADIUS_M)
    print("apply (inside)          ->", reached(lambda: world.apply(NEARBY, -30.0)))
    print("apply (gated out)       ->", reached(lambda: world.apply(FAR, -30.0)))
    print("marks_near              ->", reached(lambda: world.marks_near(NEARBY, 5000.0)))

    frame = placed.frame
    s2l = reached(lambda: frame.sphere_to_local(NEARBY))
    l2s = reached(lambda: frame.local_to_sphere(1000.0, -400.0))
    print("sphere_to_local         ->", s2l)
    print("local_to_sphere         ->", l2s)

    # The brief's specific question.
    assert set(s2l) == {"atan2", "sqrt"}, s2l
    assert set(l2s) == {"sin", "cos", "hypot", "sqrt"}, l2s
    assert s2l != l2s
    print("PROFILES DIFFER: sphere_to_local", s2l, "vs local_to_sphere", l2s)

    assert set(reached(feature.reach_m)) == {"hypot"}
    assert "hypot" in reached(lambda: Placed(feature, EARTH_RADIUS_M))
    # weight_at itself adds nothing beyond sphere_to_local.
    assert set(inside) == {"atan2", "sqrt"}, inside
    assert outside == (), outside


def test_step1_hypot_is_not_sqrt_in_cpython():
    """`reach_m`'s hypot is a different algorithm from a sqrt of a sum of squares."""
    state = 0x2545F4914F6CDD1D
    mask = (1 << 64) - 1
    differing = 0
    worst = 0
    total = 0
    for _ in range(200000):
        pair = []
        for _ in range(2):
            state = (state * 6364136223846793005 + 1442695040888963407) & mask
            h = state ^ (state >> 33)
            pair.append((h >> 11) / float(1 << 53) * 100000.0)
        x, y = pair
        total += 1
        a = math.hypot(x, y)
        b = math.sqrt(x * x + y * y)
        gap = ulps_between(a, b)
        if gap:
            differing += 1
            worst = max(worst, gap)
    print(
        f"\nmath.hypot vs math.sqrt(x*x+y*y): {differing}/{total} differ "
        f"({100.0 * differing / total:.2f}%), worst {worst} ULP"
    )
    assert differing > 0, "hypot and sqrt-of-sum would then be interchangeable"


def test_step1_engine_sources_use_the_same_operations():
    """The Rust side of the same claim, read out of tangent.rs rather than assumed."""
    source = (
        "crates/worldbuilder-engine/src/tangent.rs"
    )
    text = open(source, encoding="utf-8").read()
    body = text.split("pub fn sphere_to_local")[1].split("#[cfg(test)]")[0]
    to_local = text.split("pub fn sphere_to_local")[1].split("\n    }")[0]
    to_sphere = text.split("pub fn local_to_sphere")[1].split("\n    }")[0]
    used_local = tuple(sorted({o for o in ("sin", "cos", "atan2", "hypot", "sqrt") if f"m::{o}(" in to_local}))
    used_sphere = tuple(sorted({o for o in ("sin", "cos", "atan2", "hypot", "sqrt") if f"m::{o}(" in to_sphere}))
    # length() is m::sqrt; sphere_to_local calls .length(), so sqrt is indirect there.
    indirect = ".length()" in to_local
    print("\ntangent.rs sphere_to_local direct  ->", used_local, "| .length() (=m::sqrt):", indirect)
    print("tangent.rs local_to_sphere direct  ->", used_sphere)
    assert used_local == ("atan2",) and indirect, (used_local, indirect)
    assert used_sphere == ("cos", "hypot", "sin", "sqrt"), used_sphere
    # Same operation sets as the Python, function for function.
    assert body  # the split found a body at all


def frame_corpus(count=4000):
    yield (0.0, 0.0, 1.0)
    yield (0.0, 0.0, -1.0)
    yield (1.0, 0.0, 0.0)
    state = 0x9E3779B97F4A7C15
    mask = (1 << 64) - 1
    for _ in range(count):
        parts = []
        for _ in range(3):
            state = (state * 6364136223846793005 + 1442695040888963407) & mask
            h = state ^ (state >> 33)
            parts.append((h >> 11) / float(1 << 53) * 2.0 - 1.0)
        x, y, z = parts
        if x == y == z == 0.0:
            continue
        yield (x, y, z)


@pytest.mark.skipif(engine is None, reason="Rust engine not built")
def test_step1_measure_the_two_profiles_against_the_engine():
    """How far the two profiles actually drift, which is what sets each one's contract."""
    origin_vec = AT.vector
    worst_s2l = 0
    worst_l2s = 0
    exact_s2l = 0
    exact_l2s = 0
    total = 0
    for x, y, z in frame_corpus():
        point = SpherePoint.from_vector(vectors_module.Vec3(x, y, z))
        frame = TangentFrame.at(AT, EARTH_RADIUS_M)
        want = frame.sphere_to_local(point)
        got = engine.frame_sphere_to_local(
            origin_vec.x, origin_vec.y, origin_vec.z, EARTH_RADIUS_M,
            point.vector.x, point.vector.y, point.vector.z,
        )
        gap = max(ulps_between(want[0], got[0]), ulps_between(want[1], got[1]))
        worst_s2l = max(worst_s2l, gap)
        exact_s2l += gap == 0

        east_m, north_m = want
        there = frame.local_to_sphere(east_m, north_m)
        back = engine.frame_local_to_sphere(
            origin_vec.x, origin_vec.y, origin_vec.z, EARTH_RADIUS_M, east_m, north_m,
        )
        gap2 = max(
            ulps_between(there.vector.x, back[0]),
            ulps_between(there.vector.y, back[1]),
            ulps_between(there.vector.z, back[2]),
        )
        worst_l2s = max(worst_l2s, gap2)
        exact_l2s += gap2 == 0
        total += 1
    print(
        f"\nsphere_to_local (atan2+sqrt): worst {worst_s2l} ULP over {total} points, "
        f"{exact_s2l} bit-exact ({100.0 * exact_s2l / total:.1f}%)"
    )
    print(
        f"local_to_sphere (hypot+cos+sin+sqrt): worst {worst_l2s} ULP over {total} points, "
        f"{exact_l2s} bit-exact ({100.0 * exact_l2s / total:.1f}%)"
    )
    print(
        "  (that corpus spans the whole globe, including near-antipodal offsets where a "
        "component of the result passes through zero and one bit of input error becomes "
        "many ULP of output. Features never look further than reach_m, so the scale that "
        "matters to this slice is measured separately below.)"
    )

    # The scale a placed feature actually works at: a few kilometres, never more than
    # reach_m from the feature's middle.
    frame = TangentFrame.at(AT, EARTH_RADIUS_M)
    worst_near_s2l = 0
    worst_near_l2s = 0
    near_total = 0
    for k in range(2000):
        angle = 2.0 * math.pi * k / 2000.0
        for distance in (1.0, 137.0, 1200.0, 1236.9316876852981, 5000.0, 50000.0):
            east_m = distance * math.sin(angle)
            north_m = distance * math.cos(angle)
            there = frame.local_to_sphere(east_m, north_m)
            back = engine.frame_local_to_sphere(
                origin_vec.x, origin_vec.y, origin_vec.z, EARTH_RADIUS_M, east_m, north_m,
            )
            worst_near_l2s = max(
                worst_near_l2s,
                ulps_between(there.vector.x, back[0]),
                ulps_between(there.vector.y, back[1]),
                ulps_between(there.vector.z, back[2]),
            )
            want = frame.sphere_to_local(there)
            got = engine.frame_sphere_to_local(
                origin_vec.x, origin_vec.y, origin_vec.z, EARTH_RADIUS_M,
                there.vector.x, there.vector.y, there.vector.z,
            )
            worst_near_s2l = max(
                worst_near_s2l,
                ulps_between(want[0], got[0]),
                ulps_between(want[1], got[1]),
            )
            near_total += 1
    print(
        f"at feature scale (<= 50 km, {near_total} points): sphere_to_local worst "
        f"{worst_near_s2l} ULP, local_to_sphere worst {worst_near_l2s} ULP"
    )
    assert worst_s2l > 0 or worst_l2s > 0, "then neither would need a bounded contract"


# ---------------------------------------------------------------------------
# Step 2: the RAISE/CARVE switch at lift == 0.0.
# ---------------------------------------------------------------------------


def apply_variant(world, point, elevation_m, honour_switch=True):
    """`Features.apply`, transcribed, with the one-way switch made optional."""
    result = elevation_m
    authority = 0.0
    for placed in world.placed:
        weight = placed.weight_at(point)
        if weight <= 0.0:
            continue
        lift = placed.feature.target_m - result
        if honour_switch:
            if placed.feature.compose == RAISE and lift <= 0.0:
                continue
            if placed.feature.compose == CARVE and lift >= 0.0:
                continue
        result += weight * lift
        authority = max(authority, weight * _smooth(abs(lift) / SETTLE_M))
    return result, authority


def test_step2_the_variant_reproduces_apply_exactly():
    """The measurement is only worth anything if the transcription is faithful."""
    world = Features([a_feature(target_m=-4.0), a_feature(compose=CARVE, target_m=-40.0)])
    for elevation in (-30.0, -4.0, 0.0, 12.0, -1e-9):
        want = world.apply(NEARBY, elevation)
        got = apply_variant(world, NEARBY, elevation, honour_switch=True)
        assert bits(want[0]) == bits(got[0]) and bits(want[1]) == bits(got[1]), elevation
    print("\napply_variant(honour_switch=True) is bit-identical to Features.apply")


def test_step2_raise_and_carve_converge_bit_for_bit_at_zero_lift():
    for compose in (RAISE, CARVE):
        world = Features([a_feature(compose=compose, target_m=-4.0)])
        weight = world.placed[0].weight_at(NEARBY)
        assert weight > 0.0
        # lift is exactly zero: the ground is already at the target.
        skipped = apply_variant(world, NEARBY, -4.0, honour_switch=True)
        taken = apply_variant(world, NEARBY, -4.0, honour_switch=False)
        lift = -4.0 - -4.0
        print(
            f"\n{compose}: weight={weight!r} lift={lift!r}\n"
            f"  skipped   result bits {bits(skipped[0]):#018x} authority bits {bits(skipped[1]):#018x}\n"
            f"  not taken result bits {bits(taken[0]):#018x} authority bits {bits(taken[1]):#018x}"
        )
        assert bits(skipped[0]) == bits(taken[0])
        assert bits(skipped[1]) == bits(taken[1])
        # And the two terms the ruling rests on, printed.
        print(f"  weight * lift            = {weight * lift!r}")
        print(f"  weight * _smooth(0.0/{SETTLE_M}) = {weight * _smooth(abs(lift) / SETTLE_M)!r}")


def test_step2_the_only_zero_lift_divergence_is_a_signed_zero():
    """
    The one input where skipping and not-skipping are NOT bit-identical.

    `result += weight * 0.0` is the identity on every float except negative zero, where
    -0.0 + 0.0 is +0.0. A seabed elevation of exactly -0.0 with a target of exactly -0.0
    therefore comes out with a different bit pattern depending on the branch.
    """
    world = Features([a_feature(compose=RAISE, target_m=-0.0)])
    skipped = apply_variant(world, NEARBY, -0.0, honour_switch=True)
    taken = apply_variant(world, NEARBY, -0.0, honour_switch=False)
    print(
        f"\nsigned zero: skipped result {skipped[0]!r} bits {bits(skipped[0]):#018x}; "
        f"not-skipped result {taken[0]!r} bits {bits(taken[0]):#018x}"
    )
    assert skipped[0] == taken[0] == 0.0
    assert bits(skipped[0]) != bits(taken[0])
    assert bits(skipped[1]) == bits(taken[1])
    print("  values compare equal; bit patterns do not. Real, and confined to -0.0.")


def test_step2_one_ulp_either_side_of_zero_lift():
    weight = Placed(a_feature(), EARTH_RADIUS_M).weight_at(NEARBY)
    ground = -4.0
    tick = math.ulp(ground)
    print(f"\nweight at the probe point = {weight!r}; ulp({ground}) = {tick!r}")
    for compose in (RAISE, CARVE):
        for direction, label in ((+1, "+1 ULP"), (-1, "-1 ULP")):
            target = ground + direction * tick
            world = Features([a_feature(compose=compose, target_m=target)])
            got = world.apply(NEARBY, ground)
            zero = Features([a_feature(compose=compose, target_m=ground)]).apply(NEARBY, ground)
            lift = target - ground
            took_branch = not (
                (compose == RAISE and lift <= 0.0) or (compose == CARVE and lift >= 0.0)
            )
            print(
                f"  {compose} {label}: lift={lift!r} branch_taken={took_branch} "
                f"result={got[0]!r} (delta from lift==0: {ulps_between(got[0], zero[0])} ULP) "
                f"authority={got[1]!r} (delta: {ulps_between(got[1], zero[1])} ULP, "
                f"bits {bits(got[1]):#018x} vs {bits(zero[1]):#018x})"
            )
            if took_branch:
                # `result` moves by at most a couple of ULP -- the weight here is close
                # to one, so the contribution is close to a whole ULP of the ground.
                assert ulps_between(got[0], zero[0]) <= 2, (compose, label)
                # Authority moves from exactly zero to a small positive number, which is
                # the larger of the two effects and the one a boundary flip really costs.
                assert got[1] > 0.0
                assert bits(zero[1]) == bits(0.0)
            else:
                assert bits(got[0]) == bits(zero[0])
                assert bits(got[1]) == bits(zero[1])


# ---------------------------------------------------------------------------
# Step 3: is the reach gate a no-op, or is it load-bearing?
# ---------------------------------------------------------------------------


def weight_ungated(placed, point):
    """`Placed.weight_at` with the cosine rejection removed. Nothing else changed."""
    east, north = placed.frame.sphere_to_local(point)
    along = east * placed._along_e + north * placed._along_n
    across = east * placed._across_e + north * placed._across_n
    return _bump(along, placed.feature.length_m) * _bump(across, placed.feature.width_m)


def test_step3_ungated_matches_inside_the_gate():
    """Inside the gate the two are the same code, which the numbers must show."""
    placed = Placed(a_feature(), EARTH_RADIUS_M)
    checked = 0
    for k in range(500):
        angle = 2.0 * math.pi * k / 500.0
        for distance in (10.0, 200.0, 900.0, 1200.0, 1236.0):
            point = placed.frame.local_to_sphere(
                distance * math.sin(angle), distance * math.cos(angle)
            )
            if point.vector.dot(placed.feature.at.vector) < placed._cos_reach:
                continue
            assert bits(placed.weight_at(point)) == bits(weight_ungated(placed, point))
            checked += 1
    print(f"\ngate-accepted points where gated == ungated bit-for-bit: {checked}/{checked}")


SHAPES = ((1200.0, 300.0), (4000.0, 4000.0), (150.0, 90.0), (50000.0, 12000.0))
BEARINGS = (0.0, 37.0, 90.0, 213.5)


def corner_probes(placed, span, steps):
    """
    Points aimed at the one place the gate could possibly be doing work.

    Past `reach_m` the bump is zero because `along**2 + across**2` then exceeds
    `length**2 + width**2`, which forces `|along| >= length` or `|across| >= width`.
    The only way a rejected point could carry weight is if rounding lands `along` a
    hair under `length` and `across` a hair under `width` at the same time -- so probe
    exactly that corner rather than sampling a ring and hoping to hit it.
    """
    feature = placed.feature
    for ia in range(-steps, steps + 1):
        for ib in range(-steps, steps + 1):
            along = feature.length_m * (1.0 - ia * span)
            across = feature.width_m * (1.0 - ib * span)
            for sign_a in (1.0, -1.0):
                for sign_b in (1.0, -1.0):
                    a, c = along * sign_a, across * sign_b
                    east = a * placed._along_e + c * placed._across_e
                    north = a * placed._along_n + c * placed._across_n
                    yield placed.frame.local_to_sphere(east, north)


def test_step3_a_ring_scan_finds_nothing_which_is_why_the_probe_is_a_corner():
    """
    The negative result that justifies the shape of the next test.

    Sampling rings at `reach_m * (1 +/- small)` across many azimuths never lands on the
    one corner where both bumps are simultaneously a hair inside their edges, so it
    reports the gate as a clean no-op. It is not one. Recorded so nobody re-runs this
    scan later and concludes the gate is free.
    """
    leaks = 0
    total = 0
    for length_m, width_m in SHAPES:
        for bearing_deg in BEARINGS:
            placed = Placed(
                a_feature(length_m=length_m, width_m=width_m, bearing_deg=bearing_deg),
                EARTH_RADIUS_M,
            )
            reach = placed.feature.reach_m()
            for k in range(2000):
                angle = 2.0 * math.pi * k / 2000.0
                for scale in (1.0, 1.0 - 1e-16, 1.0 + 1e-16, 1.0 - 1e-13,
                              1.0 + 1e-13, 1.0 - 1e-10, 1.0 + 1e-10):
                    distance = reach * scale
                    point = placed.frame.local_to_sphere(
                        distance * math.sin(angle), distance * math.cos(angle)
                    )
                    if point.vector.dot(placed.feature.at.vector) >= placed._cos_reach:
                        continue
                    total += 1
                    if weight_ungated(placed, point) != 0.0:
                        leaks += 1
    print(f"\nring scan: {leaks} leaks in {total} gate-rejected points")
    assert leaks == 0, "a ring scan happening to hit the corner would be luck, not method"


def test_step3_the_reach_gate_is_load_bearing():
    """
    THE VERDICT. The two branches are NOT bit-identical.

    `weight_at` returns a hard `0.0` for a gate-rejected point. The same point run
    through the projection returns a small but genuinely non-zero weight, because the
    gate's threshold (a cosine of a hypot) and the bump's edge (a projected distance)
    are rounded independently and do not land on the same points.
    """
    worst = 0.0
    worst_where = None
    nonzero = 0
    total = 0
    for length_m, width_m in SHAPES:
        for bearing_deg in BEARINGS:
            placed = Placed(
                a_feature(length_m=length_m, width_m=width_m, bearing_deg=bearing_deg),
                EARTH_RADIUS_M,
            )
            for point in corner_probes(placed, 1e-13, 60):
                if point.vector.dot(placed.feature.at.vector) >= placed._cos_reach:
                    continue
                total += 1
                gated = placed.weight_at(point)
                ungated = weight_ungated(placed, point)
                assert bits(gated) == bits(0.0)
                if bits(ungated) != bits(0.0):
                    nonzero += 1
                    if ungated > worst:
                        worst = ungated
                        worst_where = (length_m, width_m, bearing_deg, point)
    print(
        f"\nREACH GATE: {nonzero} of {total} gate-rejected probe points return a "
        f"NON-ZERO ungated weight ({100.0 * nonzero / total:.2f}%)"
    )
    print(f"  largest such weight: {worst!r}  (bits {bits(worst):#018x} vs 0x0)")
    print(f"  at length={worst_where[0]} width={worst_where[1]} bearing={worst_where[2]}")
    assert nonzero > 0, "the gate would then be skipping a genuine no-op"


def test_step3_how_large_the_leaked_weight_can_get():
    """A bound on the disagreement, scanned across offset scales rather than one guess."""
    placed = Placed(a_feature(), EARTH_RADIUS_M)
    print("\nleak magnitude by probe offset scale (length=1200 width=300 bearing=37):")
    overall = 0.0
    for exponent in range(-16, -7):
        span = 10.0 ** exponent
        worst = 0.0
        rejected = 0
        leaked = 0
        for point in corner_probes(placed, span, 25):
            if point.vector.dot(placed.feature.at.vector) >= placed._cos_reach:
                continue
            rejected += 1
            value = weight_ungated(placed, point)
            if value != 0.0:
                leaked += 1
            worst = max(worst, value)
        overall = max(overall, worst)
        print(f"  span 1e{exponent:>3}: {leaked}/{rejected} leak, worst {worst!r}")
    print(f"  worst leaked weight over all scales: {overall!r}")
    assert overall > 0.0
    assert overall < 1e-30, "a leak this large would move a seabed, not just a bit"


def test_step3_what_the_leak_does_to_apply():
    """
    What the gate is actually protecting, in the returned values.

    `result` is unmoved: a weight around 1e-44 times any plausible lift is far below the
    resolution of a seabed elevation. `authority` is not: it starts at exactly 0.0, and
    `max(0.0, tiny)` is `tiny`. That is the bit that would differ if the port dropped
    the gate.
    """
    placed = Placed(a_feature(), EARTH_RADIUS_M)
    found = None
    for point in corner_probes(placed, 1e-13, 60):
        if point.vector.dot(placed.feature.at.vector) >= placed._cos_reach:
            continue
        if weight_ungated(placed, point) != 0.0:
            found = point
            break
    assert found is not None
    world = Features([a_feature()], EARTH_RADIUS_M)
    with_gate = world.apply(found, -30.0)

    leaked = weight_ungated(placed, found)
    lift = placed.feature.target_m - -30.0
    without_gate_result = -30.0 + leaked * lift
    without_gate_authority = max(0.0, leaked * _smooth(abs(lift) / SETTLE_M))
    print(
        f"\napply at a gate-rejected leak point (weight {leaked!r}):\n"
        f"  with gate:    result {with_gate[0]!r} bits {bits(with_gate[0]):#018x}, "
        f"authority {with_gate[1]!r} bits {bits(with_gate[1]):#018x}\n"
        f"  without gate: result {without_gate_result!r} bits {bits(without_gate_result):#018x}, "
        f"authority {without_gate_authority!r} bits {bits(without_gate_authority):#018x}"
    )
    print(
        f"  result differs: {bits(with_gate[0]) != bits(without_gate_result)}; "
        f"authority differs: {bits(with_gate[1]) != bits(without_gate_authority)}"
    )
    assert bits(with_gate[0]) == bits(without_gate_result)
    assert bits(with_gate[1]) != bits(without_gate_authority)


def test_step3_the_gate_threshold_is_itself_transcendental():
    """
    Why the gate cannot be re-derived in Rust and must be transcribed operation for
    operation: `_cos_reach` is `cos(hypot(length, width) / radius)`, two bounded calls,
    so a Rust `_cos_reach` that differs by one ULP rejects a different set of points.
    """
    placed = Placed(a_feature(), EARTH_RADIUS_M)
    reach = placed.feature.reach_m()
    naive = math.sqrt(1200.0 * 1200.0 + 300.0 * 300.0)
    print(
        f"\nreach_m via hypot = {reach!r} bits {bits(reach):#018x}\n"
        f"reach via sqrt-of-sum = {naive!r} bits {bits(naive):#018x} "
        f"({ulps_between(reach, naive)} ULP apart)"
    )
    print(f"_cos_reach = {placed._cos_reach!r} bits {bits(placed._cos_reach):#018x}")
    nudged = math.nextafter(placed._cos_reach, 1.0)
    moved = 0
    total = 0
    for point in corner_probes(placed, 1e-13, 60):
        dot = point.vector.dot(placed.feature.at.vector)
        total += 1
        if (dot < placed._cos_reach) != (dot < nudged):
            moved += 1
    print(
        f"moving _cos_reach by ONE ULP reclassifies {moved} of {total} probe points "
        f"({100.0 * moved / total:.3f}%)"
    )
    assert sys.float_info.epsilon > 0  # keeps the import honest
