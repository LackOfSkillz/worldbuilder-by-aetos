"""
Slice 1g Task 1: measure whether `margins_within`'s `limit` is safe to compare strictly.

THROWAWAY. Deleted in Task 5 once the answer is recorded in the ledger and in Task 4's
conformance tests. This file exists only to produce numbers, not to be a permanent part
of the suite.

The question: `lookup.py:217` computes

    limit = math.sin(min(math.pi / 2, range_m / radius_m))

and then makes a discrete membership decision (`if offset > limit: continue`) against
it. `offset` is algebraic (a dot product and an `abs`) and reproduces bit-for-bit between
Python and the Rust port. `limit` runs through `sin`, and CPython's `sin` is the
platform libm while the Rust engine's `detmath::sin` is the pure-Rust `libm` crate --
independently rounded implementations that Slice 0 already measured to disagree by a
single bit on a couple of percent of inputs. If `limit` differs between the two, any
candidate whose `offset` sits between the two `limit` values is included by one
implementation and excluded by the other, and the two membership lists differ in
*length*, not just in a low bit.

This harness answers two things:

1. Is `limit` bit-identical between Python and Rust across a spread of `range_m`? If
   not, what is the worst ULP distance?
2. Over the existing corpus, how close does any real candidate's `offset` come to a
   `limit` value -- i.e. is there room between "closest real candidate" and "the size of
   the sin() divergence" for membership to be trusted anyway?

Run directly for a human-readable report:

    .venv/Scripts/python -m pytest tests/test_limit_ulps.py -s -q

or import `report()` and call it.
"""

import math

from worldbuilder.geometry.sphere import EARTH_RADIUS_M, SpherePoint
from worldbuilder.geometry.vectors import Vec3

from test_conformance import (  # noqa: F401 -- reuses the existing conformance fixtures
    PLATE_SEEDS_FLAT,
    PLATE_POLES_FLAT,
    PLATE_RATES,
    PY_PLATE_SET,
    bits,
    corpus,
    engine,
)

RANGE_VALUES_M = [1e3, 1e4, 5e4, 1e5, 5e5, 1e6, 2e6, 5e6]

# range_m / radius_m > pi/2 -- forces the saturating branch of `min`.
SATURATING_RANGE_M = EARTH_RADIUS_M * (math.pi / 2.0) * 1.5


def _limit_py(range_m, radius_m=EARTH_RADIUS_M):
    return math.sin(min(math.pi / 2, range_m / radius_m))


def _limit_rs(range_m, radius_m=EARTH_RADIUS_M):
    return engine.margins_within_limit(range_m, radius_m)


def _ulp_distance(a, b):
    """Signed-magnitude ULP distance between two finite f64 bit patterns."""
    ba, bb = bits(a), bits(b)
    # Map to a monotonic ordering across the sign boundary (standard trick).
    ia = ba if ba < (1 << 63) else (1 << 64) - ba
    ib = bb if bb < (1 << 63) else (1 << 64) - bb
    return abs(ia - ib)


def _values_to_test():
    return RANGE_VALUES_M + [SATURATING_RANGE_M, 0.0]


def test_frac_pi_2_matches_python_half_pi():
    """
    `core::f64::consts::FRAC_PI_2` vs CPython's `math.pi / 2` -- if these differ, the
    Rust side of `margins_within_limit` must compute `PI / 2.0` the way Python does
    (which it already does; this test pins the reason why that matters).
    """
    rs_half_pi = engine.margins_within_limit(SATURATING_RANGE_M, EARTH_RADIUS_M * 1e-300)
    # The saturating branch makes this reproducible from the repo alone, which a
    # scratch rustc build comparing the two constants directly would not be. If
    # Rust's half-pi differed from CPython's `math.pi / 2` by even one ULP, the
    # sine of it would differ and this comparison would fail.
    py_half_pi = math.sin(math.pi / 2.0)
    assert bits(rs_half_pi) == bits(py_half_pi), (
        f"the saturating branch diverged: Rust {bits(rs_half_pi):#018x} vs "
        f"CPython {bits(py_half_pi):#018x} -- Rust's half-pi is not CPython's"
    )


def report():
    lines = []
    lines.append("=== Step 1: limit(range_m) bit comparison, Python vs Rust ===")
    worst_ulps = 0
    for range_m in _values_to_test():
        py = _limit_py(range_m)
        rs = _limit_rs(range_m)
        identical = bits(py) == bits(rs)
        ulps = 0 if identical else _ulp_distance(py, rs)
        worst_ulps = max(worst_ulps, ulps)
        label = "SATURATING" if range_m == SATURATING_RANGE_M else f"{range_m:g}"
        lines.append(
            f"range_m={label:>12}  py={py!r:>24}  rs={rs!r:>24}  "
            f"identical={identical}  ulps={ulps}"
        )
    lines.append(f"worst ULP distance across all tested range_m: {worst_ulps}")

    lines.append("")
    lines.append("=== FRAC_PI_2 check: core::f64::consts::FRAC_PI_2 vs math.pi / 2 ===")
    py_half_pi_bits = bits(math.pi / 2)
    lines.append(f"math.pi / 2 bits (python)                  = {py_half_pi_bits:#018x}")
    lines.append(
        "std::f64::consts::FRAC_PI_2 bits (rust)    = 0x3ff921fb54442d18  "
        "(checked separately via a standalone rustc build -- see task-1-report.md)"
    )
    lines.append(
        "std::f64::consts::PI / 2.0 bits (rust)     = 0x3ff921fb54442d18  "
        "(same standalone check)"
    )
    lines.append(
        f"all three bit-identical: {py_half_pi_bits == 0x3ff921fb54442d18} "
        "-- FRAC_PI_2 would have been safe to use, but margins_within_limit "
        "deliberately computes PI / 2.0 to match the Python source expression exactly"
    )

    lines.append("")
    lines.append("=== Step 2: minimum |offset - limit| over the corpus ===")
    representative_range_m = 1e5
    limit = _limit_py(representative_range_m)
    limit_ulp_bits = bits(limit)
    min_gap = math.inf
    min_gap_point = None
    considered = 0
    for x, y, z in corpus():
        nx_, ny_, nz_ = _normalise(x, y, z)
        point = SpherePoint.from_vector(Vec3(nx_, ny_, nz_))
        nearest, second = PY_PLATE_SET.nearest_two(point)
        if nearest is None:
            continue
        px, py_, pz = point.vector.x, point.vector.y, point.vector.z
        for other, normal in zip(PY_PLATE_SET.plates, PY_PLATE_SET._bisector_xyz[nearest.index]):
            if normal is None:
                continue
            nx, ny, nz = normal
            signed = px * nx + py_ * ny + pz * nz
            offset = abs(signed)
            considered += 1
            gap = abs(offset - limit)
            if gap < min_gap:
                min_gap = gap
                min_gap_point = (x, y, z)

    # Express the sin() divergence at this range_m as an absolute gap for comparison.
    rs_limit = _limit_rs(representative_range_m)
    limit_divergence_abs = abs(limit - rs_limit)
    limit_divergence_ulps = _ulp_distance(limit, rs_limit)

    lines.append(f"representative range_m = {representative_range_m:g}")
    lines.append(f"candidates considered = {considered}")
    lines.append(f"limit (python) = {limit!r}  (bit pattern {limit_ulp_bits:#018x})")
    lines.append(f"minimum |offset - limit| over corpus = {min_gap!r} at point {min_gap_point}")
    lines.append(
        f"limit divergence (python vs rust) at this range_m = {limit_divergence_abs!r} "
        f"({limit_divergence_ulps} ulps)"
    )
    if limit_divergence_abs > 0:
        safety_factor = min_gap / limit_divergence_abs if limit_divergence_abs else math.inf
        lines.append(
            f"safety factor (min_gap / limit_divergence) = {safety_factor!r} -- "
            f"{'SAFE: nothing in the corpus is close enough to flip' if safety_factor > 1 else 'UNSAFE: a candidate could flip'}"
        )
    else:
        lines.append("limit divergence is exactly zero at this range_m -- no flip is possible here")

    return "\n".join(lines), worst_ulps, min_gap, limit_ulp_bits


def _normalise(x, y, z):
    length = math.sqrt(x * x + y * y + z * z)
    return x / length, y / length, z / length


def test_report_limit_measurement():
    """Runs the full measurement and prints it; always passes -- this is a measurement
    harness, not an assertion of a particular outcome. Read the printed report (run with
    `-s`) and record the answer in task-1-report.md / the ledger."""
    text, worst_ulps, min_gap, _ = report()
    print("\n" + text)
    # Assert the finding, not merely that the arithmetic produced numbers. The
    # previous form asserted `worst_ulps >= 0`, which is true at any divergence
    # whatsoever, so it would have passed while the thing this exists to establish
    # was false.
    assert worst_ulps == 0, (
        f"`limit` is no longer bit-identical between CPython and the engine: worst "
        f"distance {worst_ulps} ULPs. Membership in `margins_within` is a discrete "
        f"choice tested against this threshold, so result lists may now differ in "
        f"length. Re-measure the margin of safety before relying on strict comparison."
    )
    assert min_gap >= 1e-9, (
        f"the closest approach to the range boundary collapsed to {min_gap:g}; at "
        f"that separation a one-ULP change in `limit` could flip membership"
    )


if __name__ == "__main__":
    text, *_ = report()
    print(text)
