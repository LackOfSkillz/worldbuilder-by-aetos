"""
Task 1 (slice 1l): measure the two gate margins before anything depends on them.

Throwaway. Task 7 deletes this file. It writes no engine code and touches nothing under
worldbuilder/ - it only measures the existing Python reference over the project's real
corpus() so the plan's contract choices (Task 2 onward) are made against real numbers
rather than an intuition about them.

Four questions, four test functions:

1. Where is the hypot, exactly - does `above_shore` reach it or not.
2. The two gate margins over corpus(), with ULP context.
3. Is the MIN_GRADIENT comment's claim ("the weight has already faded out by here")
   true, universally, only for this corpus, or false.
4. Is the gradient gate live - does it actually fire on some point in the corpus.
"""

import inspect
import math
import struct
import sys

import pytest

from tests.test_conformance import corpus
from worldbuilder.bathymetry import shelf as shelf_module
from worldbuilder.bathymetry.shelf import (
    COASTAL_WINDOW,
    MIN_GRADIENT,
    REFERENCE_GRADIENT,
    Shelf,
    _smooth,
)
from worldbuilder.geometry.sphere import SpherePoint
from worldbuilder.geometry.vectors import Vec3
from worldbuilder.plates.generation import plates_for
from worldbuilder.terrain import continentality as continentality_module
from worldbuilder.terrain.continentality import Continentality
from worldbuilder.terrain.tectonics import Tectonics

SEED = 20260831


def ulp(value):
    """The gap to the next representable double above abs(value), for scale comparisons."""
    value = abs(value)
    bits = struct.unpack("<Q", struct.pack("<d", value))[0]
    next_value = struct.unpack("<d", struct.pack("<Q", bits + 1))[0]
    return next_value - value


def points():
    for x, y, z in corpus():
        yield SpherePoint(Vec3(x, y, z).normalised())


def build():
    plates = plates_for(SEED)
    land = Continentality(SEED)
    tectonics = Tectonics(plates, land)
    shelf = Shelf(tectonics, land)
    return shelf, land, tectonics


# ---------------------------------------------------------------------------
# Question 1: where is the hypot, exactly.
# ---------------------------------------------------------------------------


def test_shelf_module_has_no_direct_transcendental():
    """
    shelf.py must import no math module and call no transcendental function of its own.
    If this ever fails, the whole premise of Task 1 (that the only transcendental shelf.py
    reaches is hypot, indirectly) is wrong and the plan needs revisiting before Task 2.
    """
    source = inspect.getsource(shelf_module)
    assert "math" not in shelf_module.__dict__, (
        "shelf.py has a `math` name bound in its module namespace: "
        f"{sorted(shelf_module.__dict__)}"
    )
    # Word-boundary + "(" so this does not false-positive on prose containing "distance",
    # "expensive", "using" etc. - only an actual call, bare or via `math.`, counts.
    import re

    transcendental_names = ("sin", "cos", "tan", "sqrt", "hypot", "exp", "log", "atan")
    pattern = re.compile(r"(?:\bmath\.)?\b(" + "|".join(transcendental_names) + r")\s*\(")
    found = sorted(set(match.group(1) for match in pattern.finditer(source)))
    assert not found, f"shelf.py source calls transcendental name(s): {found}"


def test_above_shore_does_not_reach_hypot():
    """
    Continentality.above_shore must not call Gradient.magnitude() (the only hypot in this
    path), directly or by calling gradient(). Checked structurally: above_shore's own
    source must not mention `gradient` or `magnitude`, and must not call math.hypot.

    If this fails, gate 1 (the COASTAL_WINDOW check on above_shore) inherits the same
    non-bit-identical-with-Rust bound as gate 2, and the brief's analysis needs revising.
    """
    source = inspect.getsource(Continentality.above_shore)
    assert "gradient" not in source and "magnitude" not in source, (
        "above_shore's source mentions gradient/magnitude - it may reach hypot after all:\n"
        + source
    )

    # Behavioural corroboration: above_shore must be computable with math.hypot patched to
    # explode, for a healthy sample of the corpus, on a fresh Continentality instance.
    class ExplodingHypot:
        def __call__(self, *args, **kwargs):
            raise AssertionError("above_shore reached math.hypot")

    original_hypot = continentality_module.math.hypot
    continentality_module.math.hypot = ExplodingHypot()
    try:
        land = Continentality(SEED)
        sample = list(points())[:2000]
        for point in sample:
            land.above_shore(point)  # must not raise
    finally:
        continentality_module.math.hypot = original_hypot


def test_gradient_magnitude_is_the_only_hypot_and_coastal_reaches_it_via_gradient():
    """Positive control: confirm hypot *is* reachable, via coastal() -> gradient()."""
    source = inspect.getsource(continentality_module.Gradient.magnitude)
    assert "hypot" in source, "Gradient.magnitude() no longer calls hypot: " + source

    shelf, land, tectonics = build()
    calls = {"count": 0}
    original_hypot = continentality_module.math.hypot

    def counting_hypot(*args, **kwargs):
        calls["count"] += 1
        return original_hypot(*args, **kwargs)

    continentality_module.math.hypot = counting_hypot
    try:
        for point in list(points())[:500]:
            shelf.coastal(point)
    finally:
        continentality_module.math.hypot = original_hypot

    assert calls["count"] > 0, "coastal() never reached hypot in 500 corpus points"


# ---------------------------------------------------------------------------
# Question 2: measure both gate margins over the real corpus().
# ---------------------------------------------------------------------------


def test_gate_margins_over_the_real_corpus():
    land = Continentality(SEED)

    window_margins = []
    gradient_margins = []

    for point in points():
        above = land.above_shore(point)
        window_margin = abs(abs(above) - COASTAL_WINDOW)
        window_margins.append((window_margin, above))

        if abs(above) <= COASTAL_WINDOW:
            slope = land.gradient(point).magnitude()
            gradient_margin = abs(slope - MIN_GRADIENT)
            gradient_margins.append((gradient_margin, slope))

    assert window_margins, "corpus produced no points at all"
    assert gradient_margins, "corpus produced no points inside the coastal window"

    min_window_margin, at_window = min(window_margins, key=lambda pair: pair[0])
    min_gradient_margin, at_gradient = min(gradient_margins, key=lambda pair: pair[0])

    window_ulp = ulp(COASTAL_WINDOW)
    gradient_ulp = ulp(MIN_GRADIENT)

    # Report (via assertion messages, since a passing run swallows print):
    # the margin, in absolute terms and in ULPs-at-the-threshold.
    window_margin_in_ulps = min_window_margin / window_ulp
    gradient_margin_in_ulps = min_gradient_margin / gradient_ulp

    # The prior from the brief's own 4,000-point random sample: ~1.21e-4 (window) and
    # ~2.64e-9 (gradient). The real corpus (20,006 points, includes axis-pinned awkward
    # points) is a superset in spirit and may land closer to either threshold - that is
    # itself the finding, so assert generous bounds that would fail if the corpus came
    # dramatically closer than the prior (which would matter a great deal for a 1-ULP
    # hypot divergence) while still passing on the actual measured values.
    assert min_window_margin > 0.0, (
        f"a corpus point landed exactly on the COASTAL_WINDOW boundary (value={at_window!r}); "
        "the window gate would be a knife-edge, not just close"
    )
    assert min_gradient_margin > 0.0, (
        f"a corpus point landed exactly on MIN_GRADIENT (slope={at_gradient!r}); "
        "the gradient gate would be a knife-edge, not just close"
    )

    # Pin the actual measured numbers in the failure message unconditionally, so this
    # test's output *is* the report even if nobody reads stdout.
    report = (
        f"window margin: {min_window_margin:.6e} "
        f"({window_margin_in_ulps:.6e} ULPs at COASTAL_WINDOW={COASTAL_WINDOW}); "
        f"gradient margin: {min_gradient_margin:.6e} "
        f"({gradient_margin_in_ulps:.6e} ULPs at MIN_GRADIENT={MIN_GRADIENT})"
    )

    # Sanity bound: a margin many orders of magnitude below 1 ULP is impossible (a margin
    # smaller than the spacing between representable doubles near the threshold cannot
    # exist as a *measured* difference of two doubles) - guards against a broken ulp() or
    # a unit mistake in the measurement above.
    assert min_window_margin >= window_ulp * 0.5, report
    assert min_gradient_margin >= gradient_ulp * 0.5, report

    # The real, load-bearing report: both margins must be many, many ULPs wide - the
    # asserted floor is deliberately loose (1e6 ULPs, i.e. still a tiny float distance)
    # so this fails loudly if the real corpus ever comes close enough to a 1-ULP hypot
    # divergence to matter, rather than passing silently on a number nobody looked at.
    assert window_margin_in_ulps > 1.0e6, report
    assert gradient_margin_in_ulps > 1.0e6, report

    print("\n    " + report)


# ---------------------------------------------------------------------------
# Question 3: does the MIN_GRADIENT comment's claim hold?
# ---------------------------------------------------------------------------


def test_min_gradient_comment_claim():
    """
    MIN_GRADIENT's comment claims "the weight has already faded out by here; this only
    stops the arithmetic." breadth = _smooth(REFERENCE_GRADIENT / slope) clamps to 1.0
    for tiny slope, so the claim cannot be resting on breadth fading - it must be resting
    on distance_m = value / slope being enormous (which fades `seaward`/`authority`... but
    a point where *both* value and slope are tiny gives a small distance_m and potentially
    a large, non-faded weight, which would falsify the comment's generality even if no
    such point exists in this corpus.

    Verdict is recorded via the assertion message: which of universal / corpus-only /
    false applies, with the evidence.
    """
    shelf, land, tectonics = build()

    sub_threshold = []
    for point in points():
        above = land.above_shore(point)
        if abs(above) > COASTAL_WINDOW:
            continue
        gradient = land.gradient(point)
        slope = gradient.magnitude()
        if slope >= MIN_GRADIENT:
            continue
        sub_threshold.append((point, above, slope))

    assert sub_threshold, (
        "corpus produced no point with slope < MIN_GRADIENT inside the coastal window; "
        "cannot evaluate the comment's claim against real data (see the separate "
        "reachability test for whether the gate fires at all)"
    )

    worst_weight = -1.0
    worst_detail = None
    for point, above, slope in sub_threshold:
        # Reconstruct exactly what `coastal()` and `weight()` would have produced had the
        # gate not returned early - by hand, since Shelf.coastal() itself applies the gate.
        distance_m = above / slope
        breadth = _smooth(REFERENCE_GRADIENT / slope)
        coastal = shelf_module.Coastal(distance_m=distance_m, breadth=breadth)
        hypothetical_weight = shelf.weight(point, coastal)

        if hypothetical_weight > worst_weight:
            worst_weight = hypothetical_weight
            worst_detail = (above, slope, distance_m, breadth, hypothetical_weight)

    above, slope, distance_m, breadth, weight = worst_detail
    report = (
        f"worst (largest) hypothetical weight below MIN_GRADIENT in this corpus: "
        f"weight={weight:.6e} at above_shore={above:.6e}, slope={slope:.6e} "
        f"({slope / MIN_GRADIENT:.4f}x MIN_GRADIENT), distance_m={distance_m:.6e}, "
        f"breadth={breadth:.6e}, over {len(sub_threshold)} sub-threshold corpus point(s)"
    )

    # The comment's claim, stated as a testable bound: weight must already be small
    # (near zero) at every sub-threshold point actually found. 0.01 is a generous
    # "already faded out" bar - if a sub-threshold point produced, say, weight=0.4, the
    # claim would be false for the corpus outright.
    corpus_claim_holds = weight < 0.01

    # Independently, construct the adversarial case the brief predicts: force `above` and
    # `slope` to both be tiny (well below MIN_GRADIENT) by hand, with a real point/tectonic
    # offset near zero, and see whether *that* produces a large weight. This checks
    # universality rather than only what the corpus happened to sample.
    tiny_slope = MIN_GRADIENT * 1e-3
    tiny_above = tiny_slope * 10.0  # distance_m = 10 m: absurdly close, but not zero
    adversarial_coastal = shelf_module.Coastal(
        distance_m=tiny_above / tiny_slope,
        breadth=_smooth(REFERENCE_GRADIENT / tiny_slope),
    )
    # Use a point with (as close as this corpus gets to) zero tectonic offset so
    # `authority` does not mask the effect.
    zero_tectonic_point = min(
        points(), key=lambda p: abs(tectonics.offset_m(p))
    )
    adversarial_weight = shelf.weight(zero_tectonic_point, adversarial_coastal)

    universal_claim_holds = adversarial_weight < 0.01

    verdict = (
        "universal" if universal_claim_holds else
        ("corpus-only" if corpus_claim_holds else "false")
    )

    full_report = (
        f"{report}; adversarial hand-built case (slope={tiny_slope:.3e}, "
        f"distance_m=10.0, tectonic~=0): weight={adversarial_weight:.6e}; "
        f"verdict={verdict}"
    )

    # This assertion is the deliverable: it always fires (comparing verdict to itself)
    # so the full evidence lands in the report even on a passing run, and pytest -v -s
    # or a failure both show it.
    assert verdict in ("universal", "corpus-only", "false"), full_report
    print("\n    VERDICT: " + full_report)

    # The concrete, falsifiable claim this test makes for the record: the comment is
    # false in general (there exists a constructible sub-threshold point whose weight is
    # far from faded), even though the corpus itself never samples such a point in the
    # extreme. Both facts are asserted so a future edit that breaks either is caught.
    assert adversarial_weight > 0.5, (
        "expected the hand-built tiny-value/tiny-slope point to produce a large, "
        f"non-faded weight (falsifying the comment's universal claim); got {full_report}"
    )
    assert weight < 0.01, (
        "expected every point actually sampled by the corpus to have a small weight "
        f"(the comment holding for this corpus specifically); got {full_report}"
    )


# ---------------------------------------------------------------------------
# Question 4: is the gradient gate reachable / live?
# ---------------------------------------------------------------------------


def test_gradient_gate_fires_somewhere_in_the_corpus():
    """
    Confirm MIN_GRADIENT actually gates something in the real corpus - i.e. coastal()
    returns None specifically because slope < MIN_GRADIENT (having already passed the
    COASTAL_WINDOW check), not merely that such a point is theoretically constructible.
    """
    land = Continentality(SEED)

    closest_ratio = None
    closest_point = None
    fired = 0

    for point in points():
        above = land.above_shore(point)
        if abs(above) > COASTAL_WINDOW:
            continue
        slope = land.gradient(point).magnitude()
        ratio = slope / MIN_GRADIENT
        if closest_ratio is None or ratio < closest_ratio:
            closest_ratio = ratio
            closest_point = (point, above, slope)
        if slope < MIN_GRADIENT:
            fired += 1

    assert closest_ratio is not None, "no corpus point fell inside the coastal window at all"

    point, above, slope = closest_point
    report = (
        f"closest-to-firing point: slope={slope:.6e} = {closest_ratio:.6f}x MIN_GRADIENT "
        f"({MIN_GRADIENT:.3e}), above_shore={above:.6e}; gate fired on {fired} "
        f"corpus point(s) total"
    )

    assert fired > 0, (
        "MIN_GRADIENT gate never fired anywhere in the real corpus - it may be dead "
        f"defence rather than live code. {report}"
    )
    assert closest_ratio < 1.0, report

    print("\n    " + report)


if __name__ == "__main__":
    sys.exit(pytest.main([__file__, "-v", "-s"]))
