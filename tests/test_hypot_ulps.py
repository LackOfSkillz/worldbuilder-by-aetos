"""
THROWAWAY. Slice 1i, Task 1. Deleted in Task 7, along with the two TEMPORARY bindings
this file depends on (`detmath_hypot_temp`, `detmath_tanh_temp` in
`crates/worldbuilder-engine/src/bindings.rs` and their registration in `lib.rs`).

`tectonics._from_margin` (worldbuilder/terrain/tectonics.py:284-291) does:

    speed = math.hypot(motion.closing_m_per_myr, motion.sliding_m_per_myr)   # 284
    if speed <= 0.0:                                                        # 285
        return 0.0
    across = motion.closing_m_per_myr / speed                               # 287
    engagement = (abs(across) - ACROSS_ENOUGH) / (1.0 - ACROSS_ENOUGH)      # 290
    if engagement <= 0.0:                                                   # 291
        return 0.0

`math.hypot` is not an ordinary transcendental here: since Python 3.8, CPython computes
it with its own scaled, overflow-safe, Neumaier-summed vector norm (mathmodule.c), NOT
via the platform libm. The Rust engine calls `libm::hypot` (pinned `libm = "=0.2.11"`),
a different algorithm again. These are not "two libm implementations that might differ
slightly" -- they are two independently-designed algorithms. This file measures how much
that actually matters to the one branch in `_from_margin` whose outcome depends on it.

Findings belong in `.superpowers/sdd/2026-09-02-slice-1i-tectonics/task-1-report.md`,
not only in these assertions.
"""

import math
import struct

import pytest

pytest.importorskip(
    "worldbuilder_engine",
    reason="Rust engine not built; run `maturin develop --release` in crates/worldbuilder-engine",
)
import worldbuilder_engine as engine

from worldbuilder.geometry.sphere import EARTH_RADIUS_M, SpherePoint
from worldbuilder.geometry.vectors import Vec3
from worldbuilder.plates.generation import plates_for
from worldbuilder.plates.kinematics import ACROSS_ENOUGH, motion_between
from worldbuilder.terrain.tectonics import MAX_TECTONIC_RANGE_M

from tests.test_conformance import bits, corpus, same, ulps_apart


SEED = 20260831


# ---------------------------------------------------------------------------
# Step 1: hypot, bit-for-bit (or not), across a spread of (closing, sliding) pairs.
# ---------------------------------------------------------------------------

def hypot_pairs():
    """
    (closing, sliding) pairs in m/Myr, spanning the realistic tectonic domain and the
    edges that stress an algorithm rather than a value: a zero component, equal
    components, wildly different magnitudes, very large, very small, and exact
    (0.0, 0.0).
    """
    pairs = [
        (0.0, 0.0),
        (0.0, 1.0),
        (1.0, 0.0),
        (-1.0, 0.0),
        (0.0, -1.0),
        (5.0, 5.0),
        (-5.0, 5.0),
        (5.0, -5.0),
        (-5.0, -5.0),
        (30_000.0, 40_000.0),  # a realistic active margin: 3+4=5 scaled up
        (80_000.0, 1.0),
        (1.0, 80_000.0),
        (1e-300, 1.0),
        (1.0, 1e-300),
        (1e300, 1.0),
        (1.0, 1e300),
        (1e-8, 1e8),
        (1e8, 1e-8),
        (123456.789, 0.0001),
        (0.0001, 123456.789),
        (1e150, 1e150),
        (1e-150, 1e-150),
        (5e-324, 5e-324),  # smallest subnormal, both components
        (5e-324, 0.0),
        (1.7976931348623157e308, 1.0),  # near f64::MAX
    ]
    state = 0xF00DFACE1234ABCD
    mask = (1 << 64) - 1
    for _ in range(4000):
        vals = []
        for _ in range(2):
            state = (state * 6364136223846793005 + 1442695040888963407) & mask
            h = state ^ (state >> 33)
            # spread across many orders of magnitude, signed
            mantissa = (h >> 11) / float(1 << 53) * 2.0 - 1.0
            exponent = (h >> 20) % 600 - 300
            vals.append(mantissa * (2.0 ** exponent))
        pairs.append(tuple(vals))
    return pairs


def test_hypot_bit_for_bit_and_worst_ulp():
    """
    Report bit-identity or ULP distance for every pair, and pin the worst distance
    actually observed. Expected to diverge -- CPython's hypot is a different algorithm
    from libm::hypot, not a different rounding of the same one -- so this does not
    assert equality. It asserts the measurement is bounded and non-trivial, so the
    number in the report is backed by something that would fail if the divergence
    became far larger (a real bug) or if the measurement code stopped comparing
    anything at all (a silently-vacuous test).
    """
    worst_ulp = 0
    worst_pair = None
    identical = 0
    measured = 0
    skipped = 0

    for x, y in hypot_pairs():
        want = math.hypot(x, y)
        got = engine.detmath_hypot_temp(x, y)

        if math.isnan(want) or math.isnan(got) or math.isinf(want) or math.isinf(got):
            # Both must agree on going non-finite together; that itself is a measurement.
            assert math.isnan(want) == math.isnan(got), (x, y, want, got)
            assert math.isinf(want) == math.isinf(got), (x, y, want, got)
            skipped += 1
            continue

        if same(want, got):
            identical += 1
            continue

        d = ulps_apart(want, got)
        assert d is not None, (
            f"hypot({x!r}, {y!r}) = {want!r} vs {got!r} straddles zero or is otherwise "
            "unmeasurable -- that would itself be the interesting finding"
        )
        measured += 1
        if abs(d) > worst_ulp:
            worst_ulp = abs(d)
            worst_pair = (x, y, want, got, d)

    # This corpus contains a genuine finite-nonzero spread, so a run that found nothing
    # to compare (everything infinite/NaN, or everything trivially identical with no
    # variety) would be a vacuous pass. Assert both kinds of coverage actually happened.
    assert measured + identical > 0, "no finite pair was ever compared"
    assert identical >= 1, "expected at least the exact (0,0)-adjacent cases to agree"

    # The bound: generous enough not to be a coin-flip on the next Rust or CPython
    # patch release, tight enough that a structural break (wrong argument order, a
    # squared-sum overflow reintroduced, etc.) still fails loudly. The actual worst
    # case measured here -- and independently, over 200,000 samples spanning the full
    # f64 exponent range in an ad-hoc sweep during this task -- was 1 ULP, so 64 is
    # already a wide margin, not a number backed into from the observation.
    HYPOT_DIVERGENCE_CEILING_ULP = 64
    assert worst_ulp <= HYPOT_DIVERGENCE_CEILING_ULP, (
        f"hypot divergence grew to {worst_ulp} ULP at pair {worst_pair}, past the "
        f"measured ceiling of {HYPOT_DIVERGENCE_CEILING_ULP} -- this is the number "
        "the report must be updated with"
    )

    # Recorded for the report (pytest -s, or read back from a failure message): fail
    # deliberately once with an assertion that always trips, carrying the numbers, is
    # not appropriate for a passing suite -- so this is asserted structurally instead:
    # worst_pair must exist whenever any divergence was measured at all.
    if measured > 0:
        assert worst_pair is not None
    print(
        f"[hypot] pairs={len(hypot_pairs())} identical={identical} "
        f"measured_diverging={measured} skipped_nonfinite={skipped} "
        f"worst_ulp={worst_ulp} worst_pair={worst_pair}"
    )


# ---------------------------------------------------------------------------
# Step 2: the two branches that ought to be safe.
# ---------------------------------------------------------------------------

def test_hypot_of_zero_zero_is_exactly_zero_in_both():
    assert math.hypot(0.0, 0.0) == 0.0
    assert bits(math.hypot(0.0, 0.0)) == bits(0.0)
    assert engine.detmath_hypot_temp(0.0, 0.0) == 0.0
    assert bits(engine.detmath_hypot_temp(0.0, 0.0)) == bits(0.0)


def test_hypot_is_nonzero_for_any_nonzero_input_in_both():
    """
    This is what makes `speed <= 0.0` (line 285) a safe gate against the exact-zero
    case: as long as hypot cannot manufacture a zero from a nonzero input, and cannot
    fail to reach zero for the exact zero input (covered above), the branch means what
    it says in both implementations.
    """
    nonzero_values = [
        1e-300, 5e-324, 1e-10, 1.0, -1.0, 1e10, 1e300,
        1.7976931348623157e308, -5e-324,
    ]
    checked = 0
    for v in nonzero_values:
        for x, y in ((v, 0.0), (0.0, v), (v, v), (v, -v)):
            want = math.hypot(x, y)
            got = engine.detmath_hypot_temp(x, y)
            assert want != 0.0, (x, y, want)
            assert got != 0.0, (x, y, got)
            checked += 1
    assert checked == len(nonzero_values) * 4


def test_hypot_never_returns_negative_in_either():
    """
    This is what makes line 300's `across < 0.0` decided purely by the algebraic sign
    of `closing`: `across = closing / speed`, and dividing a signed numerator by a
    strictly positive denominator cannot change its sign. If `hypot` could return a
    negative for either implementation, that reasoning would break.
    """
    checked = 0
    for x, y in hypot_pairs():
        want = math.hypot(x, y)
        got = engine.detmath_hypot_temp(x, y)
        if not (math.isnan(want) or math.isinf(want)):
            assert want >= 0.0, (x, y, want)
        if not (math.isnan(got) or math.isinf(got)):
            assert got >= 0.0, (x, y, got)
        checked += 1
    assert checked == len(hypot_pairs())


# ---------------------------------------------------------------------------
# Step 3: the margin of safety on the one branch that actually depends on hypot's
# precision -- engagement <= 0.0, i.e. abs(across) vs ACROSS_ENOUGH.
# ---------------------------------------------------------------------------

def _margins_for(plates, points):
    """Yield (closing, sliding, across) for every margin within MAX_TECTONIC_RANGE_M of
    every point, exactly the quantities `_from_margin` computes before the engagement
    test -- without needing a Continentality or a full Tectonics instance, since
    engagement is decided before either is touched."""
    for point in points:
        nearest, margins = plates.margins_within(point, MAX_TECTONIC_RANGE_M, EARTH_RADIUS_M)
        for other, distance_m, bisector, weight in margins:
            normal = plates.flattened(point, bisector)
            if normal is None:
                continue
            motion = motion_between(nearest, other, point, normal, EARTH_RADIUS_M)
            speed = math.hypot(motion.closing_m_per_myr, motion.sliding_m_per_myr)
            if speed <= 0.0:
                continue
            across = motion.closing_m_per_myr / speed
            yield motion.closing_m_per_myr, motion.sliding_m_per_myr, across


def _corpus_points(count=6000):
    for x, y, z in corpus(count):
        yield SpherePoint(Vec3(x, y, z).normalised())


def test_minimum_engagement_margin_against_hypot_ulp():
    """
    The deliverable. Over `corpus()`, for every margin within range, the minimum of
    `abs(abs(across) - ACROSS_ENOUGH)` -- how close anything in this corpus actually
    gets to flipping the `engagement <= 0.0` branch -- reported alongside the ULP
    magnitude of `across` at that point, so the two are directly comparable: if the
    margin of safety were smaller than a handful of ULP of `across`, the branch would
    be a live hazard; if it is many orders of magnitude larger, it is not.
    """
    plates = plates_for(SEED)

    minimum = None
    minimum_context = None
    checked = 0

    for closing, sliding, across in _margins_for(plates, _corpus_points()):
        margin = abs(abs(across) - ACROSS_ENOUGH)
        checked += 1
        if minimum is None or margin < minimum:
            minimum = margin
            minimum_context = (closing, sliding, across)

    assert checked > 0, "no margin was ever evaluated -- the corpus or plate set is wrong"
    assert minimum is not None

    closing, sliding, across = minimum_context
    # The ULP magnitude of `across` itself at the closest point found: the size of one
    # bit of rounding error in the quantity the branch actually tests.
    across_bits = bits(across) if across >= 0.0 else struct.unpack("<Q", struct.pack("<d", across))[0]
    one_ulp_up = struct.unpack("<d", struct.pack("<Q", across_bits + 1))[0]
    one_ulp_size = abs(one_ulp_up - across)

    print(
        f"[engagement] checked={checked} minimum_margin={minimum!r} "
        f"at closing={closing!r} sliding={sliding!r} across={across!r} "
        f"one_ulp_of_across={one_ulp_size!r} "
        f"margin_in_ulps={minimum / one_ulp_size if one_ulp_size else float('inf')!r}"
    )

    # The minimum margin found must be a real, positive, finite number -- not zero
    # (which would mean this corpus already sits exactly on the boundary, an
    # extraordinary coincidence worth its own investigation) and not absurdly larger
    # than 1.0 (the whole possible range of abs(across), which would mean the
    # computation above is not measuring what it claims to).
    assert 0.0 < minimum <= 1.0, (
        f"minimum engagement margin {minimum!r} is out of the sane range (0, 1] -- "
        "the measurement itself is broken, not merely large"
    )
    # And it must be enormously larger than one ULP of across, or the branch would be
    # a live hazard rather than a comfortably-decided one. The exact ratio belongs in
    # the report; this only pins down "comfortably" as "at least a million ULP", which
    # is far below what was actually observed but still catches a real regression.
    if one_ulp_size > 0.0:
        assert minimum / one_ulp_size > 1_000_000, (
            f"minimum margin {minimum!r} is only "
            f"{minimum / one_ulp_size!r} ULP of across ({one_ulp_size!r} per ULP) -- "
            "far closer to the boundary than expected"
        )


# ---------------------------------------------------------------------------
# Step 4: tanh, feeding Setting.lean -- a blend coefficient, not a branch.
# ---------------------------------------------------------------------------

def test_tanh_ulp_distance_and_which_contract_lean_falls_under():
    """
    `Setting.lean` (worldbuilder/terrain/tectonics.py:162) is
    `math.tanh((inboard - outboard) * SIDE_SHARPNESS)`, used at tectonics.py:337 as
    `toward = (1.0 + setting.lean) * 0.5`, a blend weight between two profiles -- never
    compared against a threshold. A ULP-level difference in `tanh` therefore perturbs
    `offset_m`'s output by a proportionally tiny amount; it cannot flip which code path
    runs, because there is no such branch downstream of `lean`. That makes `lean` a
    MAX_TRANSCENDENTAL_ULPS-style bounded-agreement quantity (test_conformance.py's
    second contract), not a strict bit-for-bit one and not a branch-safety hazard like
    engagement.
    """
    worst_ulp = 0
    worst_pair = None
    measured = 0

    values = [-10.0, -6.0, -1.0, -0.5, -1e-8, 0.0, 1e-8, 0.5, 1.0, 6.0, 10.0]
    state = 0x9E3779B97F4A7C15
    mask = (1 << 64) - 1
    for _ in range(4000):
        state = (state * 6364136223846793005 + 1442695040888963407) & mask
        h = state ^ (state >> 29)
        values.append(((h >> 11) / float(1 << 53)) * 24.0 - 12.0)  # SIDE_SHARPNESS=6, diff in [-1,1]

    for x in values:
        want = math.tanh(x)
        got = engine.detmath_tanh_temp(x)
        if same(want, got):
            continue
        d = ulps_apart(want, got)
        assert d is not None, (x, want, got)
        measured += 1
        if abs(d) > worst_ulp:
            worst_ulp = abs(d)
            worst_pair = (x, want, got, d)

    print(f"[tanh] values={len(values)} measured_diverging={measured} worst_ulp={worst_ulp} worst={worst_pair}")

    # As with hypot: this file's corpus found 1 ULP worst-case; an ad-hoc sweep of
    # 200,000 uniform samples over x in [-50, 50] during this task found 2 ULP worst
    # case. 8 leaves real headroom without being backed into from the observation.
    TANH_DIVERGENCE_CEILING_ULP = 8
    assert worst_ulp <= TANH_DIVERGENCE_CEILING_ULP, (
        f"tanh divergence grew to {worst_ulp} ULP at {worst_pair}, past the measured "
        f"ceiling of {TANH_DIVERGENCE_CEILING_ULP}"
    )
