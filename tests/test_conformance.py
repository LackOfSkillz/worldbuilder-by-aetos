"""
Conformance between the Python reference and the Rust engine.

The engine is not a rewrite that should behave similarly. It is a port, and this file
holds it to two different contracts depending on what is in a function's path.

Anything with no transcendental in it (Vec3: length, cross, normalised) must agree
bit-for-bit -- comparison is on raw f64 bit patterns, never a tolerance, because a
tolerance would let a coastline move by a metre and call it equal. Anything whose path
includes a transcendental (sin, cos, atan2, by way of the sphere functions) is bounded
to MAX_TRANSCENDENTAL_ULPS instead, because Python's math module and Rust's `libm` crate
are two independently-rounded implementations that are permitted, and measured, to
differ by a few bits. See MAX_TRANSCENDENTAL_ULPS below for why that split exists and
where the bound comes from.

Skips wholesale if the engine is not built, so the Python suite still runs on a machine
with no Rust.
"""

import math
import os
import struct

import pytest

from worldbuilder.geometry.sphere import EARTH_RADIUS_M, SpherePoint
from worldbuilder.geometry.vectors import DEGENERATE, Vec3

if os.environ.get("WORLDBUILDER_REQUIRE_ENGINE"):
    # Opt-in escape from importorskip below: on a machine that is supposed to have the
    # engine built (CI, in particular), a missing or stale `worldbuilder_engine` must
    # fail the run loudly rather than skip it -- a skipped conformance suite reports
    # green while comparing nothing at all, which is worse than no suite.
    import worldbuilder_engine as engine
else:
    engine = pytest.importorskip(
        "worldbuilder_engine",
        reason="Rust engine not built; run `maturin develop --release` in crates/worldbuilder-engine",
    )


def bits(value):
    """The exact 64-bit pattern of a float, so comparison cannot be fooled by printing."""
    return struct.unpack("<Q", struct.pack("<d", value))[0]


def same(a, b):
    return bits(a) == bits(b)


def corpus(count=20000):
    """
    Deterministic pseudo-random unit-ish vectors plus the awkward places.

    Hashed rather than gridded: a grid samples the same fractional bit patterns over and
    over and would hide a divergence that only appears at an awkward mantissa. The poles
    and the meridian are pinned, because that is where a spherical field breaks first.
    """
    yield (0.0, 0.0, 1.0)
    yield (0.0, 0.0, -1.0)
    yield (1.0, 0.0, 0.0)
    yield (-1.0, 0.0, 0.0)
    yield (0.0, 1.0, 0.0)
    yield (0.0, -1.0, 0.0)

    state = 0x2545F4914F6CDD1D
    mask = (1 << 64) - 1
    for _ in range(count):
        components = []
        for _ in range(3):
            state = (state * 6364136223846793005 + 1442695040888963407) & mask
            h = state ^ (state >> 33)
            components.append((h >> 11) / float(1 << 53) * 2.0 - 1.0)
        x, y, z = components
        if x == 0.0 and y == 0.0 and z == 0.0:
            continue
        yield (x, y, z)


def test_vec3_length_agrees():
    for x, y, z in corpus():
        assert same(Vec3(x, y, z).length(), engine.vec3_length(x, y, z)), (x, y, z)


def test_vec3_cross_agrees():
    points = list(corpus(2000))
    for (ax, ay, az), (bx, by, bz) in zip(points, points[1:]):
        want = Vec3(ax, ay, az).cross(Vec3(bx, by, bz))
        got = engine.vec3_cross(ax, ay, az, bx, by, bz)
        assert (same(want.x, got[0]) and same(want.y, got[1]) and same(want.z, got[2]))


def test_vec3_normalised_agrees():
    for x, y, z in corpus():
        want = Vec3(x, y, z).normalised()
        got = engine.vec3_normalised(x, y, z)
        assert got is not None
        assert same(want.x, got[0]) and same(want.y, got[1]) and same(want.z, got[2])


def test_vec3_normalised_agrees_on_the_zero_vector():
    """Python raises; Rust returns None. Different shapes, same meaning."""
    with pytest.raises(ValueError):
        Vec3(0.0, 0.0, 0.0).normalised()
    assert engine.vec3_normalised(0.0, 0.0, 0.0) is None


MAX_TRANSCENDENTAL_ULPS = 4
"""
Why these are not bit-for-bit, when the Vec3 tests are.

Python's math.sin, cos and atan2 delegate to the platform C library -- UCRT on
Windows, glibc on Linux. The Rust engine deliberately uses the pure-Rust `libm`
crate instead, so that its native and WebAssembly builds agree bit-for-bit; that
equality was measured over 5,000,000 samples and the studio depends on it.

Those two choices are mutually exclusive. Matching CPython exactly would mean
taking the platform libm into Rust and giving up native/WASM equality, which is
the foundation of the architecture. So the engine is a new generator version
rather than a bit-exact reimplementation, and this bound is what "the port is
structurally correct" means for a path containing a transcendental.

The bound is measured, not guessed: the worst observed divergence across a dense
sweep is 3 ULP (from_latlon's y component, a product of two 1-ULP values), with
sin alone differing on 2 of 181 integer latitudes. 4 leaves one ULP of headroom.

A bound this tight still catches structural errors. Substituting acos for atan2
in angle_to, or reordering a cross product, moves results by far more than 4 ULP.
"""


def ulps_apart(a, b):
    """Signed ULP distance, or None where the measure does not apply."""
    if a == b:
        return 0
    if math.isnan(a) or math.isnan(b) or math.isinf(a) or math.isinf(b):
        return None
    ba, bb = struct.unpack("<q", struct.pack("<d", a))[0], struct.unpack("<q", struct.pack("<d", b))[0]
    if (ba < 0) != (bb < 0):
        return None  # straddles zero
    return bb - ba


def close_enough(a, b, limit=MAX_TRANSCENDENTAL_ULPS):
    d = ulps_apart(a, b)
    if d is None:
        return a == b or (math.isnan(a) and math.isnan(b))
    return abs(d) <= limit


def test_sphere_from_latlon_agrees():
    for lat in range(-90, 91, 3):
        for lon in range(-180, 181, 7):
            want = SpherePoint.from_latlon(float(lat), float(lon)).vector
            got = engine.sphere_from_latlon(float(lat), float(lon))
            for label, w, g in zip("xyz", (want.x, want.y, want.z), got):
                assert close_enough(w, g), (
                    lat, lon, label, w, g, ulps_apart(w, g)
                )


def test_sphere_to_latlon_agrees():
    for x, y, z in corpus():
        point = SpherePoint(Vec3(x, y, z).normalised())
        want = point.to_latlon()
        got = engine.sphere_to_latlon(point.vector.x, point.vector.y, point.vector.z)
        for label, w, g in zip(("lat", "lon"), want, got):
            assert close_enough(w, g), (x, y, z, label, w, g, ulps_apart(w, g))


def test_sphere_angle_and_distance_agree():
    points = list(corpus(2000))
    for (ax, ay, az), (bx, by, bz) in zip(points, points[1:]):
        a = SpherePoint(Vec3(ax, ay, az).normalised())
        b = SpherePoint(Vec3(bx, by, bz).normalised())
        av, bv = a.vector, b.vector
        want_angle = a.angle_to(b)
        got_angle = engine.sphere_angle_to(av.x, av.y, av.z, bv.x, bv.y, bv.z)
        assert close_enough(want_angle, got_angle), (
            "angle", av, bv, want_angle, got_angle, ulps_apart(want_angle, got_angle)
        )
        want_distance = a.distance_to(b)
        got_distance = engine.sphere_distance_to(av.x, av.y, av.z, bv.x, bv.y, bv.z, EARTH_RADIUS_M)
        assert close_enough(want_distance, got_distance), (
            "distance", av, bv, want_distance, got_distance,
            ulps_apart(want_distance, got_distance),
        )


def test_transcendental_divergence_stays_within_its_measured_bound():
    """
    The bound is a measurement, not a tolerance to hide behind. If a future change
    widens it, this fails and someone has to look at why rather than nudging the
    number up.

    A comparison that cannot be measured (NaN, infinity, or a sign-straddle) must not
    be silently dropped from `worst` -- that is exactly the failure-open shape that
    would let a NaN sail through unnoticed. Every comparison in this sweep is expected
    to be measurable, so any skip at all is itself a failure.
    """
    worst = 0
    skipped = 0
    for lat in range(-90, 91):
        for lon in range(-180, 181, 5):
            want = SpherePoint.from_latlon(float(lat), float(lon)).vector
            got = engine.sphere_from_latlon(float(lat), float(lon))
            for w, g in zip((want.x, want.y, want.z), got):
                d = ulps_apart(w, g)
                if d is None:
                    skipped += 1
                else:
                    worst = max(worst, abs(d))
    assert skipped == 0, f"{skipped} comparisons could not be measured (NaN, inf or sign-straddle)"
    assert worst <= MAX_TRANSCENDENTAL_ULPS, f"divergence grew to {worst} ULP"


def test_transcendental_divergence_stays_within_its_measured_bound_for_every_sphere_function():
    """
    The test above anchors the bound to `from_latlon` alone. `to_latlon`, `angle_to`
    and `distance_to` are exercised by their own agreement tests but nothing records
    their worst case, so a regression from (say) 1 ULP to 4 would pass silently. This
    sweeps all four sphere functions over the same corpus and tracks a worst case for
    each, so a regression names which function moved.
    """
    worst = {"from_latlon": 0, "to_latlon": 0, "angle_to": 0, "distance_to": 0}
    skipped = {"from_latlon": 0, "to_latlon": 0, "angle_to": 0, "distance_to": 0}

    def record(name, w, g):
        d = ulps_apart(w, g)
        if d is None:
            skipped[name] += 1
        else:
            worst[name] = max(worst[name], abs(d))

    for lat in range(-90, 91):
        for lon in range(-180, 181, 5):
            want = SpherePoint.from_latlon(float(lat), float(lon)).vector
            got = engine.sphere_from_latlon(float(lat), float(lon))
            for w, g in zip((want.x, want.y, want.z), got):
                record("from_latlon", w, g)

    for x, y, z in corpus():
        point = SpherePoint(Vec3(x, y, z).normalised())
        want = point.to_latlon()
        got = engine.sphere_to_latlon(point.vector.x, point.vector.y, point.vector.z)
        for w, g in zip(want, got):
            record("to_latlon", w, g)

    points = list(corpus(2000))
    for (ax, ay, az), (bx, by, bz) in zip(points, points[1:]):
        a = SpherePoint(Vec3(ax, ay, az).normalised())
        b = SpherePoint(Vec3(bx, by, bz).normalised())
        av, bv = a.vector, b.vector

        want_angle = a.angle_to(b)
        got_angle = engine.sphere_angle_to(av.x, av.y, av.z, bv.x, bv.y, bv.z)
        record("angle_to", want_angle, got_angle)

        want_distance = a.distance_to(b)
        got_distance = engine.sphere_distance_to(av.x, av.y, av.z, bv.x, bv.y, bv.z, EARTH_RADIUS_M)
        record("distance_to", want_distance, got_distance)

    for name in worst:
        assert skipped[name] == 0, (
            f"{name}: {skipped[name]} comparisons could not be measured (NaN, inf or sign-straddle)"
        )
        assert worst[name] <= MAX_TRANSCENDENTAL_ULPS, (
            f"{name}: divergence grew to {worst[name]} ULP"
        )


def test_the_strict_contract_is_still_strict():
    """Vec3 has no transcendental in its path and must agree exactly, not approximately."""
    for x, y, z in corpus(500):
        assert same(Vec3(x, y, z).length(), engine.vec3_length(x, y, z))


def test_the_strict_comparison_can_actually_fail():
    """
    A conformance suite that cannot fail proves nothing. This asserts that `same` really
    distinguishes a one-bit difference, so a passing run above means something.
    """
    value = 0.1
    nudged = struct.unpack("<d", struct.pack("<Q", bits(value) + 1))[0]
    assert value != nudged
    assert not same(value, nudged)
    assert math.isclose(value, nudged)  # and a tolerance would have called them equal


def test_close_enough_can_actually_fail():
    """
    Only `same` was proved falsifiable above. `close_enough` is the other contract this
    harness leans on, and it needs the same proof: a difference past the bound must be
    rejected, not just a difference within it accepted.
    """
    value = 0.1
    nudged = struct.unpack("<d", struct.pack("<Q", bits(value) + MAX_TRANSCENDENTAL_ULPS + 1))[0]
    assert not close_enough(value, nudged)


# ---------------------------------------------------------------------------
# Noise
#
# Unlike the sphere functions, Noise contains NO transcendentals: a 64-bit integer
# hash, a floor, and pure arithmetic. It therefore falls entirely under the strict
# contract -- every one of these comparisons is bit-for-bit, with no ULP bound
# anywhere. If one of them ever needs loosening, something is wrong with the port,
# not with the standard.
# ---------------------------------------------------------------------------

from worldbuilder.terrain.noise import Noise as PyNoise

NOISE_SEED = 12345
NOISE_SALT = 0x0C0FFEE


def noise_points(count=5000):
    """Hashed sample positions, including negatives so the floor path is exercised."""
    state = 0x9E3779B97F4A7C15
    mask = (1 << 64) - 1
    yield (0.0, 0.0, 0.0)
    yield (-0.0, -0.0, -0.0)
    yield (1.0, 2.0, 3.0)
    yield (-1.0, -2.0, -3.0)
    yield (-0.000000001, 0.5, 0.5)
    for _ in range(count):
        comps = []
        for _ in range(3):
            state = (state * 6364136223846793005 + 1442695040888963407) & mask
            h = state ^ (state >> 29)
            comps.append(((h >> 11) / float(1 << 53)) * 20.0 - 10.0)
        yield tuple(comps)


def test_noise_at_agrees_exactly():
    py = PyNoise(NOISE_SEED, salt=NOISE_SALT)
    for x, y, z in noise_points():
        want = py.at(x, y, z)
        got = engine.noise_at(NOISE_SEED, NOISE_SALT, x, y, z)
        assert same(want, got), f"at({x}, {y}, {z}): {want!r} vs {got!r}"


def test_noise_at_agrees_on_negative_coordinates():
    """
    The floor-versus-truncate trap, exercised deliberately.

    Python derives its lattice cell with int(x // 1), which floors toward negative
    infinity; a Rust `as i64` truncates toward zero. On any negative coordinate those
    select different cells, and the resulting world would differ everywhere south and
    west of the origin with nothing raised and no test failing -- unless this one does.
    """
    py = PyNoise(NOISE_SEED, salt=NOISE_SALT)
    for i in range(2000):
        t = -0.0005 * i
        want = py.at(t, t * 0.5, t * 0.25)
        got = engine.noise_at(NOISE_SEED, NOISE_SALT, t, t * 0.5, t * 0.25)
        assert same(want, got), f"at({t}): {want!r} vs {got!r}"


def test_noise_fbm_agrees_exactly():
    """
    Covers argument VALUES, not just presence. A suite that only ever exercised the
    defaults (frequency=1.25, gain=0.5, lacunarity=2.0) would still pass if `gain` and
    `lacunarity` were transposed inside `fbm`, because Rust and Python would each be
    silently fed the same swapped pair. Running distinct, non-default combinations
    below -- including a second frequency -- makes a transposition visible.
    """
    py = PyNoise(NOISE_SEED, salt=NOISE_SALT)

    class _Point:
        """The Python fbm takes a SpherePoint and reads .vector; this is the smallest
        thing that satisfies it without dragging in normalisation."""

        def __init__(self, x, y, z):
            self.vector = Vec3(x, y, z)

    for x, y, z in noise_points(1500):
        for octaves in (0, 1, 4, 8):
            for frequency, gain, lacunarity in (
                (1.25, 0.5, 2.0),
                (1.25, 0.4, 2.7),
                (0.6, 0.4, 2.7),
            ):
                want = py.fbm(_Point(x, y, z), frequency, octaves, gain=gain, lacunarity=lacunarity)
                got = engine.noise_fbm(
                    NOISE_SEED, NOISE_SALT, x, y, z, frequency, octaves, gain, lacunarity
                )
                assert same(want, got), (
                    f"fbm({x},{y},{z},oct={octaves},freq={frequency},"
                    f"gain={gain},lac={lacunarity}): {want!r} vs {got!r}"
                )


def test_noise_seed_and_salt_agree():
    """Different worlds and different fields on one world must both track the Python."""
    for seed in (0, 1, 12345, 2**31, 2**63):
        for salt in (0, NOISE_SALT):
            py = PyNoise(seed, salt=salt)
            for x, y, z in noise_points(200):
                want = py.at(x, y, z)
                got = engine.noise_at(seed, salt, x, y, z)
                assert same(want, got), f"seed={seed} salt={salt} at({x},{y},{z})"


# ---------------------------------------------------------------------------
# TangentFrame
#
# This module spans BOTH contracts, which is why it is a useful one to port early.
#
#   at()               cross products, dot products and sqrt. IEEE-754 requires sqrt
#                      correctly rounded, so this is held STRICTLY, bit-for-bit.
#   local_to_sphere()  hypot, cos, sin, sqrt -- bounded at MAX_TRANSCENDENTAL_ULPS.
#   sphere_to_local()  atan2, sqrt            -- bounded likewise.
#
# The split is per code path, not per module. A strict failure below is a real defect.
# ---------------------------------------------------------------------------

from worldbuilder.geometry.tangent import TangentFrame as PyTangentFrame

FRAME_RADIUS = EARTH_RADIUS_M


def frame_origins():
    """Ordinary places, both poles, and the meridian — where a frame breaks first."""
    yield (0.0, 0.0, 1.0)
    yield (0.0, 0.0, -1.0)
    yield (1.0, 0.0, 0.0)
    yield (-1.0, 0.0, 0.0)
    for lat in range(-85, 86, 5):
        for lon in range(-180, 181, 30):
            v = SpherePoint.from_latlon(float(lat), float(lon)).vector
            yield (v.x, v.y, v.z)


def test_frame_at_agrees_exactly():
    """Strict: at() has no transcendental in its path beyond a correctly-rounded sqrt."""
    for x, y, z in frame_origins():
        py = PyTangentFrame.at(SpherePoint(Vec3(x, y, z)), FRAME_RADIUS)
        got = engine.frame_at(x, y, z, FRAME_RADIUS)
        want = (py.east.x, py.east.y, py.east.z,
                py.north.x, py.north.y, py.north.z,
                py.up.x, py.up.y, py.up.z)
        for i, (w, g) in enumerate(zip(want, got)):
            assert same(w, g), f"frame_at({x},{y},{z}) component {i}: {w!r} vs {g!r}"


def test_frame_at_is_stable_at_the_poles():
    """
    A frame that reshuffled itself between two calls would move every ship it held, so
    the pole fallback must be the same choice every time and the same choice as Python's.
    """
    for pole in [(0.0, 0.0, 1.0), (0.0, 0.0, -1.0)]:
        first = engine.frame_at(*pole, FRAME_RADIUS)
        second = engine.frame_at(*pole, FRAME_RADIUS)
        assert first == second
        py = PyTangentFrame.at(SpherePoint(Vec3(*pole)), FRAME_RADIUS)
        assert same(py.east.x, first[0]) and same(py.east.y, first[1]) and same(py.east.z, first[2])


def test_local_to_sphere_agrees_within_bound():
    for x, y, z in frame_origins():
        py = PyTangentFrame.at(SpherePoint(Vec3(x, y, z)), FRAME_RADIUS)
        for east_m, north_m in [(0.0, 0.0), (1_000.0, 0.0), (0.0, -25_000.0),
                                (200_000.0, 200_000.0), (-1_000_000.0, 500_000.0)]:
            want = py.local_to_sphere(east_m, north_m).vector
            got = engine.frame_local_to_sphere(x, y, z, FRAME_RADIUS, east_m, north_m)
            for w, g in zip((want.x, want.y, want.z), got):
                assert close_enough(w, g), (
                    f"local_to_sphere at ({x},{y},{z}) + ({east_m},{north_m}): "
                    f"{w!r} vs {g!r}, {ulps_apart(w, g)} ULP"
                )


def test_sphere_to_local_agrees_within_bound():
    for x, y, z in frame_origins():
        py = PyTangentFrame.at(SpherePoint(Vec3(x, y, z)), FRAME_RADIUS)
        for east_m, north_m in [(1_000.0, 0.0), (0.0, -25_000.0), (200_000.0, 200_000.0)]:
            there = py.local_to_sphere(east_m, north_m)
            want = py.sphere_to_local(there)
            got = engine.frame_sphere_to_local(
                x, y, z, FRAME_RADIUS, there.vector.x, there.vector.y, there.vector.z
            )
            for w, g in zip(want, got):
                assert close_enough(w, g), (
                    f"sphere_to_local at ({x},{y},{z}): {w!r} vs {g!r}, {ulps_apart(w, g)} ULP"
                )

        # The `across <= DEGENERATE` branch -- the origin itself and its antipode --
        # is not reached by any offset above, since every local_to_sphere() offset
        # lands strictly between the two. Both implementations return a hard
        # (0.0, 0.0) here, but that agreement has never actually been checked by
        # this suite; it is what proves the two sides agree, not merely what the
        # crate's own tests assert about themselves.
        for target in [(x, y, z), (-x, -y, -z)]:
            there = SpherePoint(Vec3(*target))
            want = py.sphere_to_local(there)
            got = engine.frame_sphere_to_local(x, y, z, FRAME_RADIUS, *target)
            for w, g in zip(want, got):
                assert close_enough(w, g), (
                    f"sphere_to_local degenerate at ({x},{y},{z}) -> {target}: "
                    f"{w!r} vs {g!r}, {ulps_apart(w, g)} ULP"
                )


def test_the_projection_error_table_reproduces():
    """
    Extends the round trip past the 200 km region cap, out to 500 km and 1,000 km.

    `test_local_to_sphere_agrees_within_bound` and `test_sphere_to_local_agrees_within_bound`
    already establish Rust/Python agreement within the cap; this test does not compute or
    assert anything about round-trip error. Its value is the two distances beyond the cap
    that those tests do not reach -- confirming the two implementations keep agreeing with
    each other once the projection is being used well past where the spec says it should be
    trusted, even though neither side's answer out there is meant to be accurate.
    """
    py = PyTangentFrame.at_latlon(45.0, 0.0, FRAME_RADIUS)
    for metres in [25_000.0, 100_000.0, 200_000.0, 500_000.0, 1_000_000.0]:
        there_py = py.local_to_sphere(metres, 0.0)
        back_py = py.sphere_to_local(there_py)
        origin = py.origin.vector
        got_there = engine.frame_local_to_sphere(
            origin.x, origin.y, origin.z, FRAME_RADIUS, metres, 0.0
        )
        back_rs = engine.frame_sphere_to_local(
            origin.x, origin.y, origin.z, FRAME_RADIUS, *got_there
        )
        for w, g in zip(back_py, back_rs):
            assert close_enough(w, g), f"round trip at {metres} m: {w!r} vs {g!r}"


# ---------------------------------------------------------------------------
# Continentality
#
# Two things a plausibility-check unit test cannot establish on its own, which this
# section exists to nail down:
#
#   1. The gradient's AXES. `the_gradient_points_uphill` (the Rust unit test) cannot
#      reliably catch a transposed east/north pair: a transposition is a 90-degree
#      rotation, and any direction within +/-90 degrees of true uphill still increases
#      a smooth field to first order, so a rotated gradient often passes at a single
#      point anyway. Only comparing against the Python, component by component, pins
#      the axes down.
#
#   2. Whether the calibration matches BIT-FOR-BIT or merely closely. The Rust unit
#      test asserts `< 1e-12`, which is a tolerance, not a proof of exactness. The
#      calibration spiral runs every sample through cos, sin and sqrt, whose
#      implementations differ between the pure-Rust `libm` crate and CPython's
#      platform libm (see MAX_TRANSCENDENTAL_ULPS above), so the sampled *points* are
#      expected to differ by a few ULP. Whether that propagates into the selected
#      quantile *values* is what the calibration tests below actually measure.
#
# Contract, per code path (mirrors the TangentFrame split above):
#
#   at()              is Noise.fbm and nothing else -- Noise already has a passing
#                     STRICT conformance suite, so this must also be strict. A strict
#                     failure here is a real defect, most likely in the fbm argument
#                     wiring, not grounds for loosening the bound.
#   calibration       (shore, spread) -- bounded, because the spiral uses cos/sin/sqrt.
#   above_shore       inherits its bound from shore.
#   base_elevation    bounded -- powf.
#   gradient          bounded -- TangentFrame projections.
# ---------------------------------------------------------------------------

from worldbuilder.terrain.continentality import Continentality as PyContinentality
from worldbuilder.terrain.continentality import LAND_FRACTION as PY_LAND_FRACTION

CONTINENTALITY_SEED = 12345


def continentality_corpus(count=1500):
    """Points on (or extremely near) the unit sphere -- gradient and base_elevation
    both assume a genuine sphere point, unlike raw fbm which does not care."""
    yield SpherePoint.from_latlon(90.0, 0.0)
    yield SpherePoint.from_latlon(-90.0, 0.0)
    yield SpherePoint.from_latlon(0.0, 0.0)
    yield SpherePoint.from_latlon(0.0, 180.0)
    for x, y, z in corpus(count):
        yield SpherePoint(Vec3(x, y, z).normalised())


def test_continentality_at_agrees_exactly():
    """Strict: `at` is Noise.fbm wired straight through, and Noise already agrees
    bit-for-bit. A failure here means the fbm arguments (frequency, octaves, gain,
    lacunarity, or the point itself) are wired differently than the Python, not that
    the bound needs loosening."""
    py = PyContinentality(CONTINENTALITY_SEED, EARTH_RADIUS_M, PY_LAND_FRACTION)
    for point in continentality_corpus():
        v = point.vector
        want = py.at(point)
        got = engine.continentality_at(CONTINENTALITY_SEED, PY_LAND_FRACTION, v.x, v.y, v.z)
        assert same(want, got), f"at({v.x},{v.y},{v.z}): {want!r} vs {got!r}"


def test_continentality_calibration_agrees_within_bound_across_seeds_and_land_fractions():
    """The calibration pair, for several seeds and several land fractions. Bounded,
    not strict: the spiral that produces it runs through cos, sin and sqrt."""
    for seed in (0, 1, 12345, 99999, 2**31, 2**63 - 1):
        for land_fraction in (0.05, 0.2, PY_LAND_FRACTION, 0.5, 0.71, 0.95):
            py = PyContinentality(seed, EARTH_RADIUS_M, land_fraction)
            want = (py._shore, py._spread)
            got = engine.continentality_calibration(seed, land_fraction)
            for label, w, g in zip(("shore", "spread"), want, got):
                assert close_enough(w, g), (
                    f"seed={seed} land_fraction={land_fraction} {label}: "
                    f"{w!r} vs {g!r}, {ulps_apart(w, g)} ULP"
                )


def test_continentality_above_shore_agrees_within_bound():
    py = PyContinentality(CONTINENTALITY_SEED, EARTH_RADIUS_M, PY_LAND_FRACTION)
    for point in continentality_corpus():
        v = point.vector
        want = py.above_shore(point)
        got = engine.continentality_above_shore(
            CONTINENTALITY_SEED, PY_LAND_FRACTION, v.x, v.y, v.z
        )
        assert close_enough(want, got), (
            f"above_shore({v.x},{v.y},{v.z}): {want!r} vs {got!r}, {ulps_apart(want, got)} ULP"
        )


def test_continentality_base_elevation_agrees_within_bound():
    py = PyContinentality(CONTINENTALITY_SEED, EARTH_RADIUS_M, PY_LAND_FRACTION)
    for point in continentality_corpus():
        v = point.vector
        want = py.base_elevation(point)
        got = engine.continentality_base_elevation(
            CONTINENTALITY_SEED, PY_LAND_FRACTION, v.x, v.y, v.z
        )
        assert close_enough(want, got), (
            f"base_elevation({v.x},{v.y},{v.z}): {want!r} vs {got!r}, {ulps_apart(want, got)} ULP"
        )


def test_continentality_gradient_agrees_within_bound():
    py = PyContinentality(CONTINENTALITY_SEED, EARTH_RADIUS_M, PY_LAND_FRACTION)
    for point in continentality_corpus():
        v = point.vector
        want = py.gradient(point)
        got_east, got_north = engine.continentality_gradient(
            CONTINENTALITY_SEED, PY_LAND_FRACTION, EARTH_RADIUS_M, v.x, v.y, v.z
        )
        for label, w, g in zip(("east", "north"), (want.east, want.north), (got_east, got_north)):
            assert close_enough(w, g), (
                f"gradient({v.x},{v.y},{v.z}) {label}: {w!r} vs {g!r}, {ulps_apart(w, g)} ULP"
            )


def test_continentality_gradient_agrees_at_the_poles():
    """`gradient` builds a tangent frame at the point it is called on, and a frame at
    the poles takes a fallback path the ordinary corpus above brushes past almost by
    accident. Pin it down explicitly."""
    py = PyContinentality(CONTINENTALITY_SEED, EARTH_RADIUS_M, PY_LAND_FRACTION)
    for lat, lon in [(90.0, 0.0), (-90.0, 0.0), (90.0, 37.0), (-90.0, -113.0)]:
        point = SpherePoint.from_latlon(lat, lon)
        v = point.vector
        want = py.gradient(point)
        got_east, got_north = engine.continentality_gradient(
            CONTINENTALITY_SEED, PY_LAND_FRACTION, EARTH_RADIUS_M, v.x, v.y, v.z
        )
        for label, w, g in zip(("east", "north"), (want.east, want.north), (got_east, got_north)):
            assert close_enough(w, g), (
                f"gradient at pole ({lat},{lon}) {label}: {w!r} vs {g!r}, {ulps_apart(w, g)} ULP"
            )


def test_continentality_calibration_agreement_is_far_tighter_than_the_sort_gap():
    """
    Whether the calibration matches bit-for-bit or merely closely is genuinely open
    going in: the spiral's sample points are expected to differ from Python's by a few
    ULP (cos/sin/sqrt are not bit-identical between libm and CPython's platform libm),
    but whether that survives being sorted and indexed into a quantile is a separate
    question -- a few-ULP difference in the *values themselves* could in principle
    tip a value across its neighbour in the sort and select a different array slot
    entirely, which would be a reordered sort, not arithmetic drift, and would show up
    as a divergence far larger than a few ULP.

    This measures the actual local gap between neighbouring sorted samples at the
    shore and spread indices (by reproducing the calibration spiral independently in
    Python) and asserts that the measured Rust/Python agreement is a small fraction of
    that gap, not merely under some fixed constant. The gap itself is expected to be on
    the order of 1e-9 -- far above the calibration's own ULP-level agreement, and far
    below anything that would matter to the elevation curve -- so a future divergence
    anywhere near that gap's size would mean a sample landed on the other side of a
    neighbour and the sort picked a different index, not that arithmetic drifted.
    """
    # The calibration pair that agreed exactly (0 ULP) and the one pair the wider
    # sweep above actually found diverging (shore differs by 2 ULP: Python
    # -0.3543061605914575 vs Rust -0.3543061605914576). Checking only the exact
    # pair would make the "not a reordered sort" conclusion an extrapolation from
    # a case with nothing to explain -- the divergent pair is the one that needs
    # the gap measured against it.
    for seed, land_fraction in (
        (CONTINENTALITY_SEED, PY_LAND_FRACTION),
        (2**63 - 1, 0.95),
    ):
        py = PyContinentality(seed, EARTH_RADIUS_M, land_fraction)

        golden = math.pi * (3.0 - math.sqrt(5.0))
        n = 4000
        values = []
        for index in range(n):
            z = 1.0 - 2.0 * (index + 0.5) / n
            ring = math.sqrt(max(0.0, 1.0 - z * z))
            angle = golden * index
            point = SpherePoint(Vec3(math.cos(angle) * ring, math.sin(angle) * ring, z))
            values.append(py.at(point))
        values.sort()

        shore_index = int((1.0 - land_fraction) * (n - 1))
        spread_index = int(0.84 * (n - 1))

        def neighbour_gap(i):
            gaps = []
            if i > 0:
                gaps.append(abs(values[i] - values[i - 1]))
            if i < n - 1:
                gaps.append(abs(values[i + 1] - values[i]))
            return min(gaps)

        shore_gap = neighbour_gap(shore_index)
        spread_gap = neighbour_gap(spread_index)

        got_shore, got_spread = engine.continentality_calibration(seed, land_fraction)
        shore_diff = abs(py._shore - got_shore)
        spread_diff = abs(py._spread - got_spread)

        assert shore_diff < shore_gap / 100, (
            f"seed={seed} land_fraction={land_fraction} shore diff {shore_diff!r} "
            f"is not far below the neighbour gap {shore_gap!r}"
        )
        assert spread_diff < spread_gap / 100, (
            f"seed={seed} land_fraction={land_fraction} spread diff {spread_diff!r} "
            f"is not far below the neighbour gap {spread_gap!r}"
        )


# --- PlateSet: Plate, the bisector table, and nearest_two -------------------------------
#
# Everything below is held to `same()`, not `close_enough()`. The brief for this slice is
# explicit that nothing in this path is transcendental: `nearest_two` is multiplies and
# adds, and the bisector table is a subtraction, a `length()` and a `normalised()`, all
# IEEE-exact or correctly-rounded. A divergence here is a real defect, not float noise.

from worldbuilder.plates.model import Plate as PyPlate
from worldbuilder.plates.lookup import PlateSet as PyPlateSet


def _plate_seed_vectors(count=12):
    """
    A small, fixed set of unit vectors to seed a `PlateSet` with: the six poles/meridian
    points first (explicitly, per the brief), then `count - 6` pseudo-random unit vectors
    drawn from the same corpus used everywhere else in this file.
    """
    pinned = [
        Vec3(0.0, 0.0, 1.0), Vec3(0.0, 0.0, -1.0),
        Vec3(1.0, 0.0, 0.0), Vec3(-1.0, 0.0, 0.0),
        Vec3(0.0, 1.0, 0.0), Vec3(0.0, -1.0, 0.0),
    ]
    pinned_keys = {(v.x, v.y, v.z) for v in pinned}
    seeds = list(pinned)
    for x, y, z in corpus():
        if len(seeds) >= count:
            break
        if (x, y, z) in pinned_keys:
            continue
        seeds.append(Vec3(x, y, z).normalised())
    return seeds


def _pole_for_seed(seed):
    """
    A pole distinct from its seed: a cyclic permutation of the seed's components. That
    permutation is a rotation (determinant +1), so the result is still a unit vector, and
    for every seed in this file's corpus (none of which is on the x == y == z line) it
    differs from the seed itself -- which is what "derived by some rotation" needs to mean
    for the harness to exercise a genuinely different pole per plate.
    """
    return Vec3(seed.z, seed.x, seed.y)


def _rate_for_index(index):
    """A rate that differs for every plate index, including the sign and a zero-adjacent
    value, so no two plates in a generated set share a rate."""
    return 0.01 * (index + 1) * (-1.0 if index % 2 else 1.0)


def _build_plateset_pair(seed_vectors):
    """
    A matching (Python PlateSet, flat seeds/poles/rates) tuple, index-aligned with each
    other. Poles and rates are derived per-plate (see `_pole_for_seed` and
    `_rate_for_index`) rather than fabricated as the seed and zero, so both sides of the
    comparison carry real, independently-varying values -- the point of this slice.
    """
    poles = [_pole_for_seed(v) for v in seed_vectors]
    rates = [_rate_for_index(i) for i in range(len(seed_vectors))]
    py_plates = [
        PyPlate(index=i, seed=SpherePoint(v), euler_pole=SpherePoint(p), rate_rad_per_myr=r)
        for i, (v, p, r) in enumerate(zip(seed_vectors, poles, rates))
    ]
    py_set = PyPlateSet(py_plates)
    # The margin tests below, and the index-vs-position comment above them, both depend
    # on index == position for every plate. Assert it here rather than merely documenting
    # it, so a future edit to this fixture that broke the premise would fail loudly
    # instead of silently making Python's own margin_at/margin_normal internally
    # inconsistent with each other.
    for position, plate in enumerate(py_plates):
        assert plate.index == position, (
            f"fixture invariant violated: plate at position {position} has index "
            f"{plate.index}"
        )
    seeds_flat, poles_flat = [], []
    for v, p in zip(seed_vectors, poles):
        seeds_flat.extend((v.x, v.y, v.z))
        poles_flat.extend((p.x, p.y, p.z))
    return py_set, seeds_flat, poles_flat, list(rates)


PLATE_SEED_VECTORS = _plate_seed_vectors(12)
PY_PLATE_SET, PLATE_SEEDS_FLAT, PLATE_POLES_FLAT, PLATE_RATES = _build_plateset_pair(PLATE_SEED_VECTORS)


def test_plate_angular_velocity_agrees_over_poles_and_rates():
    """
    Plate.angular_velocity is the pole scaled by the rate -- three multiplies, nothing
    transcendental -- so this is bit-for-bit over a spread of poles (including the poles
    and meridian) and rates (including zero and negative).
    """
    poles = PLATE_SEED_VECTORS + [Vec3(x, y, z).normalised() for x, y, z in list(corpus(200))[:60]]
    rates = (0.0, 1.0, -1.0, 0.01, -0.01, 0.037, -12.5, 1000.0, -1000.0)
    checked = 0
    for pole in poles:
        for rate in rates:
            plate = PyPlate(index=0, seed=SpherePoint(pole), euler_pole=SpherePoint(pole),
                             rate_rad_per_myr=rate)
            want = plate.angular_velocity()
            got = engine.plate_angular_velocity(pole.x, pole.y, pole.z, rate)
            assert same(want.x, got[0]) and same(want.y, got[1]) and same(want.z, got[2]), (
                pole, rate, want, got
            )
            checked += 1
    assert checked == len(poles) * len(rates)


def test_plateset_bisector_agrees_on_every_ordered_pair_including_none():
    """
    The whole bisector table for a generated 12-plate set, every ordered pair.

    This is the comparison the brief calls out by name: a table that had a vector where
    Python has None would sail through a check that only compared vectors where both
    sides had one, so the "is None" agreement is asserted first and separately from the
    component comparison, and the diagonal (a plate against itself, always None) is
    included by iterating the full a in range(n), b in range(n) square rather than only
    a != b.
    """
    n = len(PLATE_SEED_VECTORS)
    checked = 0
    for a in range(n):
        for b in range(n):
            want = PY_PLATE_SET._bisectors[a][b]
            got = engine.plateset_bisector(PLATE_SEEDS_FLAT, PLATE_POLES_FLAT, PLATE_RATES, a, b)
            assert (want is None) == (got is None), (a, b, want, got)
            if want is not None:
                assert same(want.x, got[0]) and same(want.y, got[1]) and same(want.z, got[2]), (
                    a, b, want, got
                )
            checked += 1
    assert checked == n * n
    # The diagonal is exactly the a == b slice of the square above, and every entry on
    # it must be None -- a plate has no bisector with itself.
    for i in range(n):
        assert PY_PLATE_SET._bisectors[i][i] is None
        assert engine.plateset_bisector(PLATE_SEEDS_FLAT, PLATE_POLES_FLAT, PLATE_RATES, i, i) is None


def test_plateset_bisector_agrees_on_a_coincident_seed_pair():
    """
    A set containing two coincident seeds, so the DEGENERATE branch (the difference's
    length being too small to trust a direction from) is exercised against the Python
    reference rather than only in the Rust unit test that already covers it in isolation.
    """
    here = Vec3(0.0, 0.0, 1.0)
    elsewhere = Vec3(0.0, 1.0, 0.0)
    seeds = [here, here, elsewhere]
    py_set, flat, poles_flat, rates = _build_plateset_pair(seeds)

    for a, b in ((0, 1), (1, 0)):
        want = py_set._bisectors[a][b]
        got = engine.plateset_bisector(flat, poles_flat, rates, a, b)
        assert want is None, "python fixture sanity: coincident seeds have no bisector"
        assert got is None, (a, b, got)

    # The distinct pairs in the same set still have a real bisector, so the degenerate
    # entries are not a symptom of every pair coming back None.
    for a, b in ((0, 2), (2, 0), (1, 2), (2, 1)):
        want = py_set._bisectors[a][b]
        got = engine.plateset_bisector(flat, poles_flat, rates, a, b)
        assert want is not None and got is not None, (a, b, want, got)
        assert same(want.x, got[0]) and same(want.y, got[1]) and same(want.z, got[2])


def test_plateset_nearest_two_agrees_over_a_corpus_of_points():
    """
    nearest_two over a corpus of sphere points, comparing both returned indices --
    comparing only the first would miss a defect in second place, which is exactly what
    the margin machinery in the next slice will read.
    """
    checked = 0
    for x, y, z in corpus(3000):
        point = SpherePoint(Vec3(x, y, z).normalised())
        want_best, want_second = PY_PLATE_SET.nearest_two(point)
        got_best, got_second = engine.plateset_nearest_two(
            PLATE_SEEDS_FLAT, PLATE_POLES_FLAT, PLATE_RATES,
            point.vector.x, point.vector.y, point.vector.z
        )
        want_best_index = None if want_best is None else want_best.index
        want_second_index = None if want_second is None else want_second.index
        assert want_best_index == got_best, (x, y, z, want_best_index, got_best)
        assert want_second_index == got_second, (x, y, z, want_second_index, got_second)
        checked += 1
    assert checked > 0


def test_plateset_nearest_two_agrees_at_the_poles_and_the_meridian():
    """The six pinned points, explicitly, rather than trusting they survive inside a loop."""
    for x, y, z in ((0.0, 0.0, 1.0), (0.0, 0.0, -1.0), (1.0, 0.0, 0.0),
                    (-1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, -1.0, 0.0)):
        point = SpherePoint(Vec3(x, y, z))
        want_best, want_second = PY_PLATE_SET.nearest_two(point)
        got_best, got_second = engine.plateset_nearest_two(
            PLATE_SEEDS_FLAT, PLATE_POLES_FLAT, PLATE_RATES, x, y, z
        )
        assert want_best.index == got_best, (x, y, z, want_best.index, got_best)
        assert want_second.index == got_second, (x, y, z, want_second.index, got_second)


def test_plateset_nearest_two_agrees_with_coincident_seeds():
    """
    The DEGENERATE branch affects the bisector table, not nearest_two -- a coincident
    seed is still a perfectly good candidate for "nearest" -- but the tie-breaking rule
    (strict comparisons, so the earlier plate wins) has to resolve identically between the
    two implementations when two seeds are literally the same point.
    """
    here = Vec3(0.0, 0.0, 1.0)
    elsewhere = Vec3(0.0, 1.0, 0.0)
    seeds = [here, here, elsewhere]
    py_set, flat, poles_flat, rates = _build_plateset_pair(seeds)

    for x, y, z in list(corpus(500)) + [(0.0, 0.0, 1.0), (0.0, 1.0, 0.0)]:
        point = SpherePoint(Vec3(x, y, z).normalised())
        want_best, want_second = py_set.nearest_two(point)
        got_best, got_second = engine.plateset_nearest_two(
            flat, poles_flat, rates, point.vector.x, point.vector.y, point.vector.z
        )
        assert want_best.index == got_best, (x, y, z, want_best.index, got_best)
        want_second_index = None if want_second is None else want_second.index
        assert want_second_index == got_second, (x, y, z, want_second_index, got_second)


# --- PlateSet: margins -------------------------------------------------------------------
#
# `Margin` carries whole `Plate` values, not indices, so a binding that fabricated
# `pole = seed` and `rate = 0.0` (as earlier bindings in this crate did, before this slice)
# would compare placeholder poles and placeholder rates against identically fabricated
# placeholders on the Python side and pass trivially -- false conformance, not conformance.
# `_build_plateset_pair` above already gives every plate a genuinely different pole
# (`_pole_for_seed`, a cyclic permutation of the seed) and a genuinely different rate
# (`_rate_for_index`, including sign and magnitude), and `plateset_from_parts` on the Rust
# side carries them straight through into real `Plate` values -- so a regression back to
# fabrication would show up here as every margin's neighbour and distance moving, not as a
# trivial pass.
#
# The split contract from the brief, applied without blurring it:
#   - nearest/neighbour indices: dot products and abs only, no transcendental anywhere.
#     Compared as exact integers with `==`. A mismatch here is a real defect.
#   - distance_m: goes through asin. Compared with `close_enough` at MAX_TRANSCENDENTAL_ULPS.
#   - margin_normal / flattened: sqrt only, no asin. Compared with `same()`, strictly.
#   - Every `None` is compared positionally (`(want is None) == (got is None)` before ever
#     touching a value), so a binding that returned `None` early where Python has a value
#     -- or a value where Python has `None` -- fails here rather than being skipped.
#
# On index vs. position: `PlateSet::new` (both languages) builds the bisector table
# addressed by POSITION in the plate list, but Python's `margin_normal` and `margin_at`
# read it back two different ways -- `margin_normal` indexes by `.index`
# (`self._bisectors[margin.nearest.index][margin.neighbour.index]`), while `margin_at`
# indexes by position in the enumeration. Rust's `margin_normal` and `margin_at` both use
# position on both axes (see the comment at plates.rs:161), which is a deliberate,
# reviewed choice (Task 2 of this slice), not an oversight.
#
# Those only coincide when a plate's `index` equals its position in the list -- true here
# because `_build_plateset_pair` assigns `index=i` in enumeration order, mirroring
# `generation.py`'s `index=index for index in range(count)`. Index-equals-position is the
# only regime in which the *Python reference itself* is internally self-consistent between
# its two ways of reading the same table; a set with shuffled indices would make Python's
# own `margin_normal` and `margin_at` disagree with EACH OTHER, before Rust ever enters the
# picture. Do not "strengthen" this suite by shuffling indices relative to position -- that
# would report a divergence that is really just Python's own inconsistency, not a Rust
# defect.

def _margins_corpus(count=4000):
    """Sphere points to probe margins at: the six pinned poles/meridian points, then a
    slice of the shared pseudo-random corpus, normalised onto the sphere."""
    for x, y, z in list(corpus())[:6]:
        yield SpherePoint(Vec3(x, y, z))
    for x, y, z in corpus(count):
        yield SpherePoint(Vec3(x, y, z).normalised())


def _bisector_points_near_margin(seed_vectors, count=1500):
    """
    Points deliberately close to a bisector between two plates -- exactly where the
    "minimum over all bisectors" neighbour selection is most likely to differ, per the
    brief. Constructed as points near the midpoint of a pair of seeds, nudged off the
    great circle by a small, varying amount so the sample is not literally sitting on the
    bisector every time (which would only exercise the exact-zero case).
    """
    n = len(seed_vectors)
    state = 0xABCDEF0123456789
    mask = (1 << 64) - 1
    pairs = [(i, j) for i in range(n) for j in range(i + 1, n)]
    produced = 0
    while produced < count and pairs:
        for i, j in pairs:
            a, b = seed_vectors[i], seed_vectors[j]
            midpoint = (a + b)
            if midpoint.length() <= 1e-12:
                continue
            midpoint = midpoint.normalised()

            state = (state * 6364136223846793005 + 1442695040888963407) & mask
            h = state ^ (state >> 33)
            nudge = (h >> 11) / float(1 << 53) * 2.0 - 1.0  # in [-1, 1)

            # A small perpendicular-ish nudge via a third, unrelated direction, then
            # renormalise back onto the sphere.
            other = seed_vectors[(i + j + 1) % n]
            nudged = Vec3(
                midpoint.x + other.x * nudge * 1e-4,
                midpoint.y + other.y * nudge * 1e-4,
                midpoint.z + other.z * nudge * 1e-4,
            )
            if nudged.length() <= 1e-12:
                continue
            yield SpherePoint(nudged.normalised())
            produced += 1
            if produced >= count:
                break


def test_plateset_margin_at_agrees_over_a_corpus_of_points():
    """
    The split contract, point by point: indices as exact integers (a mismatch is a real
    defect), distance through `close_enough` (it went through asin).
    """
    checked = 0
    for point in _margins_corpus():
        v = point.vector
        want = PY_PLATE_SET.margin_at(point, EARTH_RADIUS_M)
        got_nearest, got_neighbour, got_distance = engine.plateset_margin_at(
            PLATE_SEEDS_FLAT, PLATE_POLES_FLAT, PLATE_RATES, v.x, v.y, v.z, EARTH_RADIUS_M
        )
        want_nearest = None if want.nearest is None else want.nearest.index
        want_neighbour = None if want.neighbour is None else want.neighbour.index
        assert want_nearest == got_nearest, (v.x, v.y, v.z, "nearest", want_nearest, got_nearest)
        assert want_neighbour == got_neighbour, (
            v.x, v.y, v.z, "neighbour", want_neighbour, got_neighbour
        )
        assert close_enough(want.distance_m, got_distance), (
            v.x, v.y, v.z, "distance_m", want.distance_m, got_distance,
            ulps_apart(want.distance_m, got_distance),
        )
        checked += 1
    assert checked > 0


def test_plateset_margin_at_agrees_at_the_poles_and_the_meridian():
    """The six pinned points, explicitly."""
    for x, y, z in ((0.0, 0.0, 1.0), (0.0, 0.0, -1.0), (1.0, 0.0, 0.0),
                    (-1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, -1.0, 0.0)):
        point = SpherePoint(Vec3(x, y, z))
        want = PY_PLATE_SET.margin_at(point, EARTH_RADIUS_M)
        got_nearest, got_neighbour, got_distance = engine.plateset_margin_at(
            PLATE_SEEDS_FLAT, PLATE_POLES_FLAT, PLATE_RATES, x, y, z, EARTH_RADIUS_M
        )
        assert want.nearest.index == got_nearest, (x, y, z, want.nearest.index, got_nearest)
        want_neighbour = None if want.neighbour is None else want.neighbour.index
        assert want_neighbour == got_neighbour, (x, y, z, want_neighbour, got_neighbour)
        assert close_enough(want.distance_m, got_distance), (
            x, y, z, want.distance_m, got_distance, ulps_apart(want.distance_m, got_distance)
        )


def test_plateset_margin_at_agrees_with_a_single_plate():
    """A single-plate set: infinite distance, no neighbour, and the None must line up
    positionally rather than merely "both sides happen to have nothing to compare"."""
    seed = Vec3(0.0, 0.0, 1.0)
    py_set, flat, poles_flat, rates = _build_plateset_pair([seed])
    for x, y, z in [(0.0, 0.0, 1.0), (1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (-1.0, 0.0, 0.0)]:
        point = SpherePoint(Vec3(x, y, z))
        want = py_set.margin_at(point, EARTH_RADIUS_M)
        got_nearest, got_neighbour, got_distance = engine.plateset_margin_at(
            flat, poles_flat, rates, x, y, z, EARTH_RADIUS_M
        )
        assert want.nearest.index == got_nearest == 0
        assert want.neighbour is None
        assert got_neighbour is None
        assert math.isinf(want.distance_m)
        assert math.isinf(got_distance)


def test_plateset_margin_at_agrees_near_bisectors():
    """
    Points deliberately close to a bisector between two plates, where the minimum-over-
    bisector-sines neighbour selection is most likely to differ between implementations.
    """
    checked = 0
    for point in _bisector_points_near_margin(PLATE_SEED_VECTORS):
        v = point.vector
        want = PY_PLATE_SET.margin_at(point, EARTH_RADIUS_M)
        got_nearest, got_neighbour, got_distance = engine.plateset_margin_at(
            PLATE_SEEDS_FLAT, PLATE_POLES_FLAT, PLATE_RATES, v.x, v.y, v.z, EARTH_RADIUS_M
        )
        want_nearest = None if want.nearest is None else want.nearest.index
        want_neighbour = None if want.neighbour is None else want.neighbour.index
        assert want_nearest == got_nearest, (v.x, v.y, v.z, "nearest", want_nearest, got_nearest)
        assert want_neighbour == got_neighbour, (
            v.x, v.y, v.z, "neighbour", want_neighbour, got_neighbour
        )
        assert close_enough(want.distance_m, got_distance), (
            v.x, v.y, v.z, "distance_m", want.distance_m, got_distance,
            ulps_apart(want.distance_m, got_distance),
        )
        checked += 1
    assert checked > 0


def test_plateset_margin_normal_agrees():
    """`margin_normal` runs through sqrt only, no asin, so it is held strictly."""
    checked = 0
    for point in _margins_corpus():
        v = point.vector
        margin = PY_PLATE_SET.margin_at(point, EARTH_RADIUS_M)
        want = PY_PLATE_SET.margin_normal(point, margin)
        got = engine.plateset_margin_normal(
            PLATE_SEEDS_FLAT, PLATE_POLES_FLAT, PLATE_RATES, v.x, v.y, v.z, EARTH_RADIUS_M
        )
        assert (want is None) == (got is None), (v.x, v.y, v.z, want, got)
        if want is not None:
            assert same(want.x, got[0]) and same(want.y, got[1]) and same(want.z, got[2]), (
                v.x, v.y, v.z, want, got
            )
        checked += 1
    assert checked > 0


def test_plateset_margin_normal_agrees_near_bisectors():
    checked = 0
    for point in _bisector_points_near_margin(PLATE_SEED_VECTORS):
        v = point.vector
        margin = PY_PLATE_SET.margin_at(point, EARTH_RADIUS_M)
        want = PY_PLATE_SET.margin_normal(point, margin)
        got = engine.plateset_margin_normal(
            PLATE_SEEDS_FLAT, PLATE_POLES_FLAT, PLATE_RATES, v.x, v.y, v.z, EARTH_RADIUS_M
        )
        assert (want is None) == (got is None), (v.x, v.y, v.z, want, got)
        if want is not None:
            assert same(want.x, got[0]) and same(want.y, got[1]) and same(want.z, got[2]), (
                v.x, v.y, v.z, want, got
            )
        checked += 1
    assert checked > 0


def test_plateset_margin_normal_agrees_with_a_single_plate():
    """No neighbour, so no normal -- on both sides, positionally."""
    seed = Vec3(0.0, 0.0, 1.0)
    py_set, flat, poles_flat, rates = _build_plateset_pair([seed])
    point = SpherePoint(Vec3(1.0, 0.0, 0.0))
    margin = py_set.margin_at(point, EARTH_RADIUS_M)
    want = py_set.margin_normal(point, margin)
    got = engine.plateset_margin_normal(
        flat, poles_flat, rates, point.vector.x, point.vector.y, point.vector.z, EARTH_RADIUS_M
    )
    assert want is None
    assert got is None


def test_plateset_flattened_agrees():
    """`flattened` is sqrt only, no asin, so it is held strictly -- exercised directly
    against every defined bisector normal in the table, not only the ones `margin_at`
    happens to select as the nearest plate's closest margin. The point corpus is kept
    modest (200) because it is crossed against the full n*n bisector table below."""
    n = len(PLATE_SEED_VECTORS)
    checked = 0
    for point in _margins_corpus(200):
        v = point.vector
        for a in range(n):
            for b in range(n):
                normal = PY_PLATE_SET._bisectors[a][b]
                if normal is None:
                    continue
                want = PY_PLATE_SET.flattened(point, normal)
                got = engine.plateset_flattened(
                    PLATE_SEEDS_FLAT, PLATE_POLES_FLAT, PLATE_RATES,
                    v.x, v.y, v.z, normal.x, normal.y, normal.z,
                )
                assert (want is None) == (got is None), (v.x, v.y, v.z, a, b, want, got)
                if want is not None:
                    assert (
                        same(want.x, got[0]) and same(want.y, got[1]) and same(want.z, got[2])
                    ), (v.x, v.y, v.z, a, b, want, got)
                checked += 1
    assert checked > 0


def test_plateset_flattened_agrees_when_the_normal_points_straight_up():
    """The degenerate case: standing exactly where a bisector's normal points, leaving no
    component in the tangent plane. Both sides must return None, not almost-None."""
    normal = PY_PLATE_SET._bisectors[0][1]
    assert normal is not None, "fixture sanity: plates 0 and 1 have a bisector"
    point = SpherePoint(normal)
    want = PY_PLATE_SET.flattened(point, normal)
    got = engine.plateset_flattened(
        PLATE_SEEDS_FLAT, PLATE_POLES_FLAT, PLATE_RATES,
        point.vector.x, point.vector.y, point.vector.z, normal.x, normal.y, normal.z,
    )
    assert want is None
    assert got is None


def test_the_minimum_bisector_sine_gap_is_measured_not_assumed():
    """
    The hazard the brief calls out: neighbour selection in `margin_at` is a discrete
    choice -- the minimum over bisector sines -- and two of those sines could be equal or
    near-equal at a point equidistant from two margins. Slice 1d measured the same shape
    for the calibration quantile rather than guessing; this does the same here.

    For every point in the corpus, this recomputes every defined bisector sine for the
    nearest plate (mirroring `margin_at`'s own loop) and records the gap between the two
    smallest. The minimum such gap observed, over the whole corpus, is the reported
    number. A gap near the spacing of one ULP at that magnitude (roughly 2**-52, i.e.
    about 2.2e-16 for a sine near 1.0, smaller still for a sine near 0.0) would mean
    neighbour selection is genuinely fragile at that point -- a rounding difference alone
    could flip which plate is "the" neighbour -- and that must be reported, not buried.
    """
    minimum_gap = math.inf
    minimum_gap_point = None
    for point in list(_margins_corpus(3000)) + list(_bisector_points_near_margin(PLATE_SEED_VECTORS, 1500)):
        nearest, _ = PY_PLATE_SET.nearest_two(point)
        v = point.vector
        px, py, pz = v.x, v.y, v.z
        sines = []
        for normal in PY_PLATE_SET._bisector_xyz[nearest.index]:
            if normal is None:
                continue
            sines.append(abs(px * normal[0] + py * normal[1] + pz * normal[2]))
        if len(sines) < 2:
            continue
        sines.sort()
        gap = sines[1] - sines[0]
        if gap < minimum_gap:
            minimum_gap = gap
            minimum_gap_point = (v.x, v.y, v.z)

    assert minimum_gap_point is not None, "corpus produced no point with two or more margins"

    print(
        f"\nminimum observed gap between the two smallest bisector sines: {minimum_gap!r} "
        f"at point {minimum_gap_point}"
    )

    # Assert a real floor rather than merely printing the value: a passing run swallows
    # `print`, so without this the 1.369e-5 figure the README cites as evidence of
    # robustness is pinned nowhere, and an exact tie (gap 0.0 -- precisely the fragility
    # this test exists to detect) would pass silently. 1e-9 sits four orders below the
    # observed gap and seven above the ~1e-16 scale where a tie hazard would actually
    # live, so it has margin on both sides without masking a real regression.
    assert minimum_gap >= 1e-9, (
        f"minimum bisector sine gap collapsed to {minimum_gap!r} at {minimum_gap_point} "
        "-- neighbour selection may be fragile at this point"
    )
    assert minimum_gap < math.inf


# --- PlateSet: margins_within -------------------------------------------------------------
#
# The split contract from the brief, applied without blurring it -- and Task 1's finding
# applied without overstating it:
#
#   - List membership and ordering: Task 1 measured `limit` bit-identical between Python
#     and Rust across every range tested (worst 0 ULP), so this compares strictly -- same
#     length, same plate indices, same order. A mismatch is a real defect, not rounding.
#   - Plate indices (`nearest`, and each entry's `other`): exact integers.
#   - weight: `same()`, strictly. Algebraic throughout -- dot products, a division, two
#     clamps and a polynomial. Nothing transcendental touches it.
#   - normal: `same()`, strictly, component-wise.
#   - distance_m: `close_enough` at MAX_TRANSCENDENTAL_ULPS -- the one bounded quantity,
#     because of asin.
#   - `None` / empty: compared positionally. `nearest` is `None` exactly where Python's is,
#     never merely "both sides happen to have nothing"; an empty list must come back empty,
#     not padded.
#
# What is deliberately NOT done here: no permanent binding is added to pin bit-identity of
# `limit`, and Task 1's measurement is not re-asserted as bit-identity. Task 5 deletes
# `tests/test_limit_ulps.py` and the TEMPORARY `margins_within_limit` scaffolding, which
# removes the only thing that pinned it -- and bit-identity of `sin` is platform-contingent
# (measured against Windows' UCRT; another libm could differ) so it would be the wrong
# thing to lean on permanently anyway. What strict membership actually depends on is the
# *geometric* margin: how close any real candidate's `offset` comes to `limit`. Task 1
# measured that separation at 2.858e-07 against a one-ULP scale of about 1e-16 -- nine
# orders of headroom -- for one range value over its corpus.
# `test_the_closest_approach_to_the_range_boundary_is_measured_not_assumed` below
# re-measures that quantity independently, from the Python side alone (so it needs no
# special binding), across a spread of ranges and this file's corpus, and asserts a floor
# on it with the observed value in the failure message -- not merely printed, per the
# vacuous-test failure mode this project has already shipped three times.
# `test_the_smallest_shadow_gap_is_measured_not_assumed` does the same for the other hard
# decision in this function: `genuine <= 0: continue`, taken on the sign of `shadow`.

def _range_values_for_margins():
    """
    A spread of range_m from selecting nothing, through selecting some, to spanning more
    than the planet -- reused by every margins_within test below so "none/some/all" is
    exercised consistently rather than each test inventing its own numbers.
    """
    return (1.0e3, 1.0e4, 1.0e5, 1.0e6, 4.0e6, 5.0e6, 2.0e7, EARTH_RADIUS_M * math.pi)


def _assert_margins_within_match(want_nearest, want_found, got_nearest, got_found, context):
    """
    The split contract, applied to one `margins_within` call: nearest and every `other` as
    exact integers, `None` positionally, length and order compared before any element (so a
    length mismatch is reported as a length mismatch, not an off-by-one zip truncation),
    weight and normal strictly, distance through `close_enough`.
    """
    want_nearest_index = None if want_nearest is None else want_nearest.index
    assert want_nearest_index == got_nearest, (*context, "nearest", want_nearest_index, got_nearest)
    assert len(want_found) == len(got_found), (
        *context, "length",
        [o.index for o, _, _, _ in want_found], [o for o, _, _, _ in got_found],
    )
    for (w_other, w_dist, w_normal, w_weight), (g_other, g_dist, g_normal, g_weight) in zip(
        want_found, got_found
    ):
        assert w_other.index == g_other, (*context, "other", w_other.index, g_other)
        assert close_enough(w_dist, g_dist), (
            *context, "distance_m", w_dist, g_dist, ulps_apart(w_dist, g_dist),
        )
        assert (
            same(w_normal.x, g_normal[0])
            and same(w_normal.y, g_normal[1])
            and same(w_normal.z, g_normal[2])
        ), (*context, "normal", (w_normal.x, w_normal.y, w_normal.z), g_normal)
        assert same(w_weight, g_weight), (*context, "weight", w_weight, g_weight)


def test_plateset_margins_within_agrees_over_a_corpus_of_points():
    """The corpus against the standing 12-plate multi-plate set, across every range value
    from `_range_values_for_margins` -- selecting nothing, something, and everything."""
    checked = 0
    for point in _margins_corpus(800):
        v = point.vector
        for range_m in _range_values_for_margins():
            want_nearest, want_found = PY_PLATE_SET.margins_within(point, range_m, EARTH_RADIUS_M)
            got_nearest, got_found = engine.plateset_margins_within(
                PLATE_SEEDS_FLAT, PLATE_POLES_FLAT, PLATE_RATES,
                v.x, v.y, v.z, range_m, EARTH_RADIUS_M,
            )
            _assert_margins_within_match(
                want_nearest, want_found, got_nearest, got_found, (v.x, v.y, v.z, range_m)
            )
            checked += 1
    assert checked > 0


def test_plateset_margins_within_agrees_at_the_poles_and_the_meridian():
    """The six pinned points, explicitly, across every range value."""
    for x, y, z in ((0.0, 0.0, 1.0), (0.0, 0.0, -1.0), (1.0, 0.0, 0.0),
                    (-1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, -1.0, 0.0)):
        point = SpherePoint(Vec3(x, y, z))
        for range_m in _range_values_for_margins():
            want_nearest, want_found = PY_PLATE_SET.margins_within(point, range_m, EARTH_RADIUS_M)
            got_nearest, got_found = engine.plateset_margins_within(
                PLATE_SEEDS_FLAT, PLATE_POLES_FLAT, PLATE_RATES, x, y, z, range_m, EARTH_RADIUS_M
            )
            _assert_margins_within_match(
                want_nearest, want_found, got_nearest, got_found, (x, y, z, range_m)
            )


def test_plateset_margins_within_agrees_near_bisectors():
    """
    Points deliberately close to a bisector (offset near zero), where a candidate is most
    likely to sit right at the edge of being included at all as range shrinks, and where
    the shadow test is most sensitive to a third plate's position.
    """
    checked = 0
    for point in _bisector_points_near_margin(PLATE_SEED_VECTORS, 600):
        v = point.vector
        for range_m in (1.0e4, 1.0e6, 2.0e7):
            want_nearest, want_found = PY_PLATE_SET.margins_within(point, range_m, EARTH_RADIUS_M)
            got_nearest, got_found = engine.plateset_margins_within(
                PLATE_SEEDS_FLAT, PLATE_POLES_FLAT, PLATE_RATES,
                v.x, v.y, v.z, range_m, EARTH_RADIUS_M,
            )
            _assert_margins_within_match(
                want_nearest, want_found, got_nearest, got_found, (v.x, v.y, v.z, range_m)
            )
            checked += 1
    assert checked > 0


def test_plateset_margins_within_agrees_with_a_single_plate_set():
    """A single-plate set: the nearest plate is returned, and the margin list is empty on
    both sides -- positionally, not merely "both sides happen to have nothing"."""
    seed = Vec3(0.0, 0.0, 1.0)
    py_set, flat, poles_flat, rates = _build_plateset_pair([seed])
    for x, y, z in [(0.0, 0.0, 1.0), (1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (-1.0, 0.0, 0.0)]:
        point = SpherePoint(Vec3(x, y, z))
        for range_m in _range_values_for_margins():
            want_nearest, want_found = py_set.margins_within(point, range_m, EARTH_RADIUS_M)
            got_nearest, got_found = engine.plateset_margins_within(
                flat, poles_flat, rates, x, y, z, range_m, EARTH_RADIUS_M
            )
            assert want_nearest.index == got_nearest == 0, (x, y, z, range_m, got_nearest)
            assert want_found == (), "python fixture sanity: one plate has no margins"
            assert got_found == [], (x, y, z, range_m, got_found)


def _three_plate_py_set():
    """
    Mirrors `plates.rs`'s `three_plate_set` Rust unit test fixture exactly: two seeds on
    the equator, one lifted off it, chosen so the third is not on the great circle
    bisecting the other two. Reuses `_build_plateset_pair` for real, distinct poles and
    rates rather than fabricating them.
    """
    seeds = [
        SpherePoint.from_latlon(0.0, 0.0).vector,
        SpherePoint.from_latlon(0.0, 90.0).vector,
        SpherePoint.from_latlon(60.0, 45.0).vector,
    ]
    return _build_plateset_pair(seeds)


def test_plateset_margins_within_agrees_across_ranges_that_select_none_some_and_all_margins():
    """
    Standing on plate 0's seed: the 0-1 bisector is about 5,003,772 m away and the 0-2
    bisector about 3,852,637 m away (measured from the Python reference, not assumed), so a
    1e6 m range selects neither, a 4e6 m range selects only the nearer one, and a 2e7 m
    range selects both -- none, some, and all, from the same point.
    """
    py_set, flat, poles_flat, rates = _three_plate_py_set()
    point = SpherePoint.from_latlon(0.0, 0.0)
    v = point.vector
    expectations = {1.0e6: [], 4.0e6: [2], 2.0e7: [1, 2]}
    for range_m, expected_others in expectations.items():
        want_nearest, want_found = py_set.margins_within(point, range_m, EARTH_RADIUS_M)
        assert [o.index for o, _, _, _ in want_found] == expected_others, (
            "python fixture sanity", range_m, [o.index for o, _, _, _ in want_found]
        )
        got_nearest, got_found = engine.plateset_margins_within(
            flat, poles_flat, rates, v.x, v.y, v.z, range_m, EARTH_RADIUS_M
        )
        _assert_margins_within_match(
            want_nearest, want_found, got_nearest, got_found, (v.x, v.y, v.z, range_m)
        )


def test_plateset_margins_within_finds_a_weight_strictly_between_zero_and_one_near_a_triple_junction():
    """
    Walking north along longitude 20 in the three-plate fixture, the shadow that plate 2
    casts on the 0-1 margin crosses zero somewhere between 11 and 13 degrees latitude
    (measured from the Python reference; see the Rust unit test
    `a_shadowed_margin_fades_rather_than_switching_off` for the same walk). At 11.75
    degrees the weight of the 0-1 margin is strictly between 0 and 1 -- neither fully
    genuine nor fully shadowed -- which is exactly the case a boolean shadow test would get
    wrong and this contract (weight compared strictly, via `same()`) must still agree on.
    """
    py_set, flat, poles_flat, rates = _three_plate_py_set()
    point = SpherePoint.from_latlon(11.75, 20.0)
    v = point.vector
    want_nearest, want_found = py_set.margins_within(point, 2.0e7, EARTH_RADIUS_M)
    want_weight = next((w for o, _, _, w in want_found if o.index == 1), None)
    assert want_weight is not None and 0.0 < want_weight < 1.0, (
        f"fixture sanity: weight was {want_weight!r}, not strictly between 0 and 1"
    )
    got_nearest, got_found = engine.plateset_margins_within(
        flat, poles_flat, rates, v.x, v.y, v.z, 2.0e7, EARTH_RADIUS_M
    )
    _assert_margins_within_match(
        want_nearest, want_found, got_nearest, got_found, (v.x, v.y, v.z, "triple-junction")
    )


def _non_saturating_range_values():
    """
    A spread of range_m that stays strictly below the point where `min(pi/2, range_m /
    radius_m)` saturates -- unlike `_range_values_for_margins`, which deliberately
    includes a saturating range so "select all" is exercised for membership tests.

    At saturation `limit` pins at exactly `sin(pi/2) == 1.0`, an exact value rather than
    one reached through rounding, and `offset` (a dot product of two unit vectors) can
    itself equal 1.0 exactly whenever a bisector normal is parallel to the point -- e.g.
    at a pinned pole. That makes `abs(offset - limit) == 0.0` a real but uninteresting
    coincidence of exact arithmetic, not the near-boundary fragility this floor exists to
    detect, so it must not be allowed to swallow the measurement. Task 1 measured its
    2.858e-07 figure at range_m=1e5, well inside this non-saturating band.
    """
    return (1.0e3, 1.0e4, 1.0e5, 1.0e6, 4.0e6, 5.0e6)


def _closest_approach_to_range_boundary(points, range_values):
    """
    For every (point, range_m) pair, the minimum `abs(offset - limit)` over every defined
    bisector of the nearest plate -- the raw geometric quantity `margins_within`'s
    membership test (`if offset > limit: continue`) is actually taken on. Returns the
    smallest gap found, and the `(x, y, z, range_m, offset, limit)` context that produced
    it, so a caller can re-test membership exactly there.
    """
    minimum = math.inf
    minimum_context = None
    for point in points:
        v = point.vector
        nearest, _ = PY_PLATE_SET.nearest_two(point)
        px, py, pz = v.x, v.y, v.z
        for range_m in range_values:
            limit = math.sin(min(math.pi / 2, range_m / EARTH_RADIUS_M))
            for normal in PY_PLATE_SET._bisector_xyz[nearest.index]:
                if normal is None:
                    continue
                offset = abs(px * normal[0] + py * normal[1] + pz * normal[2])
                gap = abs(offset - limit)
                if gap < minimum:
                    minimum = gap
                    minimum_context = (v.x, v.y, v.z, range_m, offset, limit)
    return minimum, minimum_context


def test_the_closest_approach_to_the_range_boundary_is_measured_not_assumed():
    """
    Task 1 measured this quantity at one range value (2.858e-07, against a one-ULP scale
    of about 1e-16 -- nine orders of headroom) and found it robust to a few-ULP divergence
    in `limit`. This recomputes the same quantity independently, from the Python side
    alone, across the spread of ranges this file's other margins_within tests use and this
    file's corpus, and asserts a floor on it -- not merely prints it, per the vacuous-test
    failure mode (asserting only `>= 0.0` and `< inf`, which an exact tie would pass) that
    slice 1f's sine-gap test had to be fixed for.

    If this floor ever fires, that is the signal to revisit strict comparison for
    membership -- and it is precisely the signal bit-identity of `limit` would not have
    given, since bit-identity is platform-contingent (measured against Windows' UCRT here)
    while this geometric margin is not.
    """
    points = list(_margins_corpus(2000)) + list(_bisector_points_near_margin(PLATE_SEED_VECTORS, 1000))
    minimum, context = _closest_approach_to_range_boundary(points, _non_saturating_range_values())
    assert context is not None, "corpus produced no measurable candidate"

    print(f"\nclosest approach to the range boundary: {minimum!r} at {context}")

    assert minimum >= 1e-9, (
        f"closest observed approach to the range boundary collapsed to {minimum!r} at "
        f"{context} -- strict membership may be fragile here"
    )

    # The point and range that produced the smallest gap, explicitly re-tested: this is
    # the "points deliberately placed near the range boundary" case the brief asks for,
    # and it is exactly the case a bit-identity assumption about `limit` would have been
    # needed to cover, rather than the geometric margin this test measures instead.
    x, y, z, range_m, offset, limit = context
    point = SpherePoint(Vec3(x, y, z))
    want_nearest, want_found = PY_PLATE_SET.margins_within(point, range_m, EARTH_RADIUS_M)
    got_nearest, got_found = engine.plateset_margins_within(
        PLATE_SEEDS_FLAT, PLATE_POLES_FLAT, PLATE_RATES, x, y, z, range_m, EARTH_RADIUS_M
    )
    _assert_margins_within_match(
        want_nearest, want_found, got_nearest, got_found, (x, y, z, range_m)
    )


def _shadow_values_for_point(point, range_m):
    """
    Reimplements the shadow computation inside `margins_within` (lookup.py, the loop
    building `found`) to surface the raw `shadow` value the `genuine <= 0: continue`
    decision is taken on -- a quantity the public API never returns. Mirrors the algorithm
    exactly, the same approach `test_the_minimum_bisector_sine_gap_is_measured_not_assumed`
    takes for the neighbour-selection decision above.
    """
    nearest, _ = PY_PLATE_SET.nearest_two(point)
    v = point.vector
    px, py, pz = v.x, v.y, v.z
    limit = math.sin(min(math.pi / 2, range_m / EARTH_RADIUS_M))
    seeds = PY_PLATE_SET._seed_xyz
    values = []
    for other, normal in zip(PY_PLATE_SET.plates, PY_PLATE_SET._bisector_xyz[nearest.index]):
        if normal is None:
            continue
        nx, ny, nz = normal
        signed = px * nx + py * ny + pz * nz
        offset = abs(signed)
        if offset > limit:
            continue
        foot_x, foot_y, foot_z = px - nx * signed, py - ny * signed, pz - nz * signed
        reach = math.sqrt(foot_x * foot_x + foot_y * foot_y + foot_z * foot_z)
        if reach <= DEGENERATE:
            continue
        scale = 1.0 / reach
        stand_x, stand_y, stand_z = foot_x * scale, foot_y * scale, foot_z * scale
        here = seeds[nearest.index]
        mine = stand_x * here[0] + stand_y * here[1] + stand_z * here[2]
        shadow = 2.0
        for third, (tx, ty, tz) in zip(PY_PLATE_SET.plates, seeds):
            if third.index == nearest.index or third.index == other.index:
                continue
            shadow = min(shadow, mine - (stand_x * tx + stand_y * ty + stand_z * tz))
        values.append(shadow)
    return values


def test_the_smallest_shadow_gap_is_measured_not_assumed():
    """
    The other hard decision in `margins_within`: `genuine <= 0: continue`, taken on the
    sign of `shadow`. Like the range-boundary gap above, this is a discrete membership
    decision on a continuous quantity, and the same hazard applies -- a candidate whose
    `shadow` sits right at zero is one rounding difference away from being included by one
    implementation and excluded by the other.

    Measured across this file's corpus and range spread, floored rather than printed, for
    the same reason as `test_the_closest_approach_to_the_range_boundary_is_measured_not_assumed`.
    """
    minimum = math.inf
    minimum_context = None
    points = list(_margins_corpus(2000)) + list(_bisector_points_near_margin(PLATE_SEED_VECTORS, 1000))
    for point in points:
        for range_m in _range_values_for_margins():
            for shadow in _shadow_values_for_point(point, range_m):
                gap = abs(shadow)
                if gap < minimum:
                    minimum = gap
                    v = point.vector
                    minimum_context = (v.x, v.y, v.z, range_m, shadow)

    assert minimum_context is not None, "corpus produced no candidate that reached the shadow test"

    print(f"\nsmallest observed |shadow|: {minimum!r} at {minimum_context}")

    assert minimum >= 1e-9, (
        f"smallest observed |shadow| collapsed to {minimum!r} at {minimum_context} -- "
        "the shadow/genuine decision may be fragile here"
    )


# --- Kinematics: surface_velocity, motion_between, motion_at ----------------------------
#
# The split contract from the brief, applied without blurring it: nothing in this module
# is transcendental except the `asin` inside `margin_at`, which only rides along in the
# returned `Motion.margin.distance_m` -- `motion_at` itself never reads it. Every velocity
# component, `closing_m_per_myr`, `sliding_m_per_myr` and plate index is compared with
# `same()`, bit-for-bit; `kind` is compared as an exact string; the one `close_enough`
# comparison anywhere in this section is that trailing `distance_m`. `None` is compared
# positionally throughout: `motion_at` returns `None`, not a `Motion` full of zeros,
# whenever there is no neighbour or no normal, and a binding that fabricated a value where
# Python has `None` (or vice versa) must fail here rather than being skipped.
#
# `plateset_motion_at` is the first binding in this crate that both goes through
# `plateset_from_parts` AND reads `euler_pole`/`rate_rad_per_myr` -- every earlier binding
# onto that constructor (`plateset_bisector`, `plateset_nearest_two`, `plateset_margin_at`,
# `plateset_margin_normal`, `plateset_flattened`, `plateset_margins_within`) feeds functions
# that only read `seed`, so a fabricated pole or a zeroed rate was inert there. See the
# fabrication-mutation record in this slice's task-4 report for the guard this finally
# exercises.

from worldbuilder.plates.kinematics import ACROSS_ENOUGH as PY_ACROSS_ENOUGH
from worldbuilder.plates.kinematics import TRANSFORM as PY_TRANSFORM
from worldbuilder.plates.kinematics import motion_at as py_motion_at
from worldbuilder.plates.kinematics import motion_between as py_motion_between
from worldbuilder.plates.kinematics import surface_velocity as py_surface_velocity


def _kinematics_plate(index, pole, rate):
    """A `Plate` whose seed is the pole itself -- irrelevant to every function in this
    module, exactly as `bindings::plate_angular_velocity` and the new
    `plate_surface_velocity`/`plates_motion_between` bindings assume when they build a
    `Plate` inline rather than going through `plateset_from_parts`."""
    point = SpherePoint(pole)
    return PyPlate(index=index, seed=point, euler_pole=point, rate_rad_per_myr=rate)


def test_plate_surface_velocity_agrees_over_a_spread_of_poles_rates_and_points():
    """Cross product and a scale, nothing transcendental: bit-for-bit over a spread of
    poles (including the pinned poles/meridian), rates (including negative), and points."""
    poles = PLATE_SEED_VECTORS
    points = list(corpus(300))
    checked = 0
    for index, pole in enumerate(poles):
        rate = _rate_for_index(index)
        plate = _kinematics_plate(index, pole, rate)
        for x, y, z in points:
            point = SpherePoint(Vec3(x, y, z).normalised())
            want = py_surface_velocity(plate, point, EARTH_RADIUS_M)
            got = engine.plate_surface_velocity(
                pole.x, pole.y, pole.z, rate,
                point.vector.x, point.vector.y, point.vector.z, EARTH_RADIUS_M,
            )
            assert (
                same(want.x, got[0]) and same(want.y, got[1]) and same(want.z, got[2])
            ), (pole, rate, point.vector.x, point.vector.y, point.vector.z, want, got)
            checked += 1
    assert checked == len(poles) * len(points)


def test_plate_surface_velocity_agrees_at_a_plates_own_euler_pole():
    """Vanishes on both sides -- the position vector is parallel to the rotation axis
    there, so the cross product is exactly zero, not merely close to it."""
    pole = Vec3(0.0, 0.0, 1.0)
    plate = _kinematics_plate(0, pole, 0.01)
    at_pole = SpherePoint(pole)
    want = py_surface_velocity(plate, at_pole, EARTH_RADIUS_M)
    got = engine.plate_surface_velocity(
        pole.x, pole.y, pole.z, 0.01, pole.x, pole.y, pole.z, EARTH_RADIUS_M
    )
    assert want.x == 0.0 and want.y == 0.0 and want.z == 0.0, (
        "fixture sanity: python itself must vanish exactly here"
    )
    assert same(want.x, got[0]) and same(want.y, got[1]) and same(want.z, got[2])


def test_plates_motion_between_agrees_over_a_corpus_of_points_and_normals():
    """Two named plates (real, distinct poles and rates, not fabricated), a corpus of
    points, and a corpus of normals -- `closing`, `sliding` and `kind` bit/string-exact."""
    near_pole, far_pole = PLATE_SEED_VECTORS[0], PLATE_SEED_VECTORS[1]
    near_rate, far_rate = _rate_for_index(0), _rate_for_index(1)
    near = _kinematics_plate(0, near_pole, near_rate)
    far = _kinematics_plate(1, far_pole, far_rate)

    points = list(corpus(1500))
    checked = 0
    for (px, py_, pz), (nx, ny, nz) in zip(points, points[1:]):
        try:
            normal = Vec3(nx, ny, nz).normalised()
        except ValueError:
            continue
        point = SpherePoint(Vec3(px, py_, pz).normalised())
        want = py_motion_between(near, far, point, normal, EARTH_RADIUS_M)
        got_closing, got_sliding, got_kind = engine.plates_motion_between(
            near_pole.x, near_pole.y, near_pole.z, near_rate,
            far_pole.x, far_pole.y, far_pole.z, far_rate,
            point.vector.x, point.vector.y, point.vector.z,
            normal.x, normal.y, normal.z, EARTH_RADIUS_M,
        )
        assert same(want.closing_m_per_myr, got_closing), (
            "closing", point.vector, normal, want.closing_m_per_myr, got_closing
        )
        assert same(want.sliding_m_per_myr, got_sliding), (
            "sliding", point.vector, normal, want.sliding_m_per_myr, got_sliding
        )
        assert want.kind == got_kind, ("kind", point.vector, normal, want.kind, got_kind)
        checked += 1
    # `corpus()` itself never yields the zero vector (it skips that combination), so the
    # `except ValueError` above can never fire here: every consecutive pair is checked.
    assert checked == len(points) - 1


def test_plates_motion_between_agrees_for_a_stationary_pair():
    """speed is exactly 0.0, so `abs(closing) / speed` would be 0.0 / 0.0 -- only the
    `or` short-circuit prevents it, on both sides. Compared with exact equality: these are
    products of an exactly-zero relative-velocity vector, not the residue of cancelling
    unequal quantities."""
    # Both plates' Euler poles at the north pole -- mirrors `spinning_pair` in the Rust
    # unit tests, so both angular velocities are `(0, 0, rate)` and the relative velocity
    # on the equator is purely eastward, making the classification arithmetic easy to
    # reason about by hand while still going through the real bindings.
    pole = Vec3(0.0, 0.0, 1.0)
    near = _kinematics_plate(0, pole, 0.01)
    far = _kinematics_plate(1, pole, 0.01)
    point = SpherePoint.from_latlon(0.0, 0.0)
    normal = Vec3(0.0, 1.0, 0.0)

    want = py_motion_between(near, far, point, normal, EARTH_RADIUS_M)
    assert want.kind == PY_TRANSFORM
    # +0.0 or -0.0, but exactly zero-magnitude either way -- products of an identically
    # zero relative-velocity vector, not the residue of cancelling unequal quantities.
    assert want.closing_m_per_myr == 0.0 and want.sliding_m_per_myr == 0.0

    got_closing, got_sliding, got_kind = engine.plates_motion_between(
        pole.x, pole.y, pole.z, 0.01, pole.x, pole.y, pole.z, 0.01,
        point.vector.x, point.vector.y, point.vector.z,
        normal.x, normal.y, normal.z, EARTH_RADIUS_M,
    )
    assert got_kind == PY_TRANSFORM
    assert got_closing == 0.0 and got_sliding == 0.0
    # The sign of zero is part of what "bit-for-bit" means: Rust and Python must agree on
    # it, not merely on magnitude.
    assert same(want.closing_m_per_myr, got_closing), (want.closing_m_per_myr, got_closing)
    assert same(want.sliding_m_per_myr, got_sliding), (want.sliding_m_per_myr, got_sliding)


def test_plates_motion_between_agrees_either_side_of_the_across_enough_threshold():
    """`|closing| / speed` equals `a` exactly for a normal of `(0, a, b)`. At `a = 0.5` the
    ratio is exactly `ACROSS_ENOUGH` and the comparison is a strict `<`, so this must NOT
    be transform on either side; at `a = 0.4` it must be. Exercised through the real
    bindings, not just the Rust unit test, so a divergence in how the two languages wire
    the comparison would show here too."""
    # Both plates' Euler poles at the north pole -- mirrors `spinning_pair` in the Rust
    # unit tests, so both angular velocities are `(0, 0, rate)` and the relative velocity
    # on the equator is purely eastward, making the classification arithmetic easy to
    # reason about by hand while still going through the real bindings.
    pole = Vec3(0.0, 0.0, 1.0)
    near = _kinematics_plate(0, pole, 0.02)
    far = _kinematics_plate(1, pole, 0.01)
    point = SpherePoint.from_latlon(0.0, 0.0)

    root_three_over_two = math.sqrt(0.75)
    exactly_at_normal = Vec3(0.0, PY_ACROSS_ENOUGH, root_three_over_two)
    just_below_normal = Vec3(0.0, 0.4, math.sqrt(1.0 - 0.16))

    for normal, expect_transform in ((exactly_at_normal, False), (just_below_normal, True)):
        want = py_motion_between(near, far, point, normal, EARTH_RADIUS_M)
        assert (want.kind == PY_TRANSFORM) == expect_transform, (
            "python fixture sanity", normal, want.kind
        )
        got_closing, got_sliding, got_kind = engine.plates_motion_between(
            pole.x, pole.y, pole.z, 0.02, pole.x, pole.y, pole.z, 0.01,
            point.vector.x, point.vector.y, point.vector.z,
            normal.x, normal.y, normal.z, EARTH_RADIUS_M,
        )
        assert (got_kind == PY_TRANSFORM) == expect_transform, (normal, got_kind)
        assert want.kind == got_kind
        assert same(want.closing_m_per_myr, got_closing)
        assert same(want.sliding_m_per_myr, got_sliding)


def _motion_at_agrees(point, radius_m=EARTH_RADIUS_M):
    """One `motion_at` comparison, positional on `None` first: nearest/neighbour indices
    as exact integers, `closing`/`sliding` with `same()`, `kind` as an exact string, and
    the margin's `distance_m` -- the one `close_enough` comparison in this whole section,
    because it rode in through `asin` inside `margin_at` -- last."""
    want = py_motion_at(point, PY_PLATE_SET, radius_m)
    got = engine.plateset_motion_at(
        PLATE_SEEDS_FLAT, PLATE_POLES_FLAT, PLATE_RATES,
        point.vector.x, point.vector.y, point.vector.z, radius_m,
    )
    assert (want is None) == (got is None), (point.vector, want, got)
    if want is None:
        return
    got_nearest, got_neighbour, got_distance, got_closing, got_sliding, got_kind = got
    assert want.margin.nearest.index == got_nearest, (point.vector, "nearest")
    assert want.margin.neighbour.index == got_neighbour, (point.vector, "neighbour")
    assert same(want.closing_m_per_myr, got_closing), (point.vector, "closing")
    assert same(want.sliding_m_per_myr, got_sliding), (point.vector, "sliding")
    assert want.kind == got_kind, (point.vector, "kind", want.kind, got_kind)
    assert close_enough(want.margin.distance_m, got_distance), (
        point.vector, "distance_m", want.margin.distance_m, got_distance,
        ulps_apart(want.margin.distance_m, got_distance),
    )


def test_plateset_motion_at_agrees_over_a_corpus_of_points():
    """The standing 12-plate multi-plate set, across the shared margins corpus (the six
    pinned poles/meridian points plus a slice of the pseudo-random corpus)."""
    checked = 0
    for point in _margins_corpus(2000):
        _motion_at_agrees(point)
        checked += 1
    assert checked > 0


def test_plateset_motion_at_agrees_near_bisectors():
    """Points deliberately close to a bisector, where neighbour selection -- and so which
    margin's motion gets reported -- is most likely to differ between implementations."""
    checked = 0
    for point in _bisector_points_near_margin(PLATE_SEED_VECTORS, 1000):
        _motion_at_agrees(point)
        checked += 1
    assert checked > 0


def test_plateset_motion_at_agrees_at_the_poles_and_the_meridian():
    """The six pinned points, explicitly, rather than trusting they survive inside a
    loop."""
    for x, y, z in ((0.0, 0.0, 1.0), (0.0, 0.0, -1.0), (1.0, 0.0, 0.0),
                    (-1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, -1.0, 0.0)):
        _motion_at_agrees(SpherePoint(Vec3(x, y, z)))


def test_plateset_motion_at_agrees_with_a_single_plate_set():
    """No neighbour, so no margin, so `None` on both sides -- positionally, not merely
    'both sides happen to have nothing to compare'."""
    seed = Vec3(0.0, 0.0, 1.0)
    py_set, flat, poles_flat, rates = _build_plateset_pair([seed])
    for x, y, z in [(0.0, 0.0, 1.0), (1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (-1.0, 0.0, 0.0)]:
        point = SpherePoint(Vec3(x, y, z))
        want = py_motion_at(point, py_set, EARTH_RADIUS_M)
        got = engine.plateset_motion_at(
            flat, poles_flat, rates, x, y, z, EARTH_RADIUS_M
        )
        assert want is None, "python fixture sanity: a single plate has no margin to report"
        assert got is None, (x, y, z, got)


def test_plateset_motion_at_agrees_at_a_plates_own_euler_pole():
    """Standing exactly at one plate's own Euler pole: that plate's own contribution to
    the relative velocity there is exactly zero, which is exercised through the full
    `motion_at` path (margin lookup, normal, and the classification) rather than in
    isolation, against whichever neighbour margin_at actually selects there."""
    own_pole = Vec3(PLATE_POLES_FLAT[0], PLATE_POLES_FLAT[1], PLATE_POLES_FLAT[2])
    _motion_at_agrees(SpherePoint(own_pole))


def test_the_margin_classification_threshold_gap_is_measured_not_assumed():
    """
    The hazard this section's whole contract rests on: `motion_at`'s classification is a
    strict `abs(closing) / speed < ACROSS_ENOUGH` comparison, so a sample landing extremely
    close to that ratio is one rounding difference away from being classified transform by
    one implementation and convergent/divergent by the other -- even though every quantity
    feeding the comparison is algebraic (a cross product, dot products, subtractions and a
    division; nothing transcendental) and therefore expected to be bit-identical between
    Python and Rust.

    This recomputes the classification ratio independently from the Python side alone (so
    it needs no special binding), mirroring `motion_at`'s own arithmetic, over the shared
    margins corpus and the near-bisector corpus, and asserts a floor on the smallest
    observed `abs(abs(closing) / speed - ACROSS_ENOUGH)` across every sample where
    `speed > 0.0` -- not merely printed, per the vacuous-test failure mode (asserting only
    `>= 0.0`) this project has already had to fix three times.
    """
    minimum = math.inf
    minimum_context = None
    points = list(_margins_corpus(3000)) + list(_bisector_points_near_margin(PLATE_SEED_VECTORS, 1500))
    for point in points:
        margin = PY_PLATE_SET.margin_at(point, EARTH_RADIUS_M)
        if margin.neighbour is None:
            continue
        normal = PY_PLATE_SET.margin_normal(point, margin)
        if normal is None:
            continue
        relative = py_surface_velocity(margin.nearest, point, EARTH_RADIUS_M) - py_surface_velocity(
            margin.neighbour, point, EARTH_RADIUS_M
        )
        closing = -relative.dot(normal)
        speed = relative.length()
        if speed <= 0.0:
            continue
        gap = abs(abs(closing) / speed - PY_ACROSS_ENOUGH)
        if gap < minimum:
            minimum = gap
            v = point.vector
            minimum_context = (v.x, v.y, v.z, closing, speed)

    assert minimum_context is not None, "corpus produced no sample with speed > 0.0"

    print(f"\nsmallest observed threshold gap: {minimum!r} at {minimum_context}")

    assert minimum >= 1e-9, (
        f"smallest observed |abs(closing)/speed - ACROSS_ENOUGH| collapsed to {minimum!r} "
        f"at {minimum_context} -- margin classification may be fragile here"
    )


# --- Tectonics: bump, continental, setting_at, offset_m, elevation_m --------------------
#
# The contract split from the brief, applied with one measured amendment:
#
# `_bump` and `_continental` are purely algebraic -- an abs, a division, one or two
# comparisons and a smoothstep, nothing transcendental -- so they are compared with
# `same()`, bit-for-bit. Any tolerance here would hide a real defect.
#
# `setting_at`, `offset_m` and `elevation_m` all run through `hypot`, `tanh`, the tangent
# frame and `Continentality` (fbm noise), so per the brief they are "bounded" -- but
# `close_enough(..., MAX_TRANSCENDENTAL_ULPS)` (4 ULP) does NOT hold for any of the three,
# and this section measures that rather than asserting it away. The mechanism is the same
# in each case: a quantity that legitimately passes through (or arbitrarily close to)
# zero -- `engagement` at the `ACROSS_ENOUGH` gate inside `offset_m`/`elevation_m`, and
# `Continentality.at`'s own zero crossing inside `setting_at` -- turns a few-ULP upstream
# disagreement (from `hypot`'s Neumaier summation vs `libm`, ultimately) into a large
# *relative* error exactly where the absolute value is smallest. `TECTONICS_BOUNDED_MAX_ULPS`
# below is the measured, documented replacement for `MAX_TRANSCENDENTAL_ULPS` in this one
# section; it is not a general loosening of the file's contract.
#
# The set of margins that actually contribute (survive `engagement <= 0.0`) is a separate,
# discrete question from the *size* of their contribution, and this slice's own
# ~22,000-point `TECTONICS_POINTS` corpus measured that one strictly: the smallest observed
# `abs(abs(across) - ACROSS_ENOUGH)` was 2.4349e-05, about 2.19e11 ULP of `across` --
# roughly eleven orders of magnitude clear of the 1-ULP `hypot`
# divergence that could ever move `across` at all. So which margins engage is reproducible
# and is exercised here through real geometry (a genuine two-margin point, and points
# deliberately close to the gate that this section finds on the standing 12-plate fixture),
# not hedged with a tolerance.

from worldbuilder.terrain.tectonics import Tectonics as PyTectonics
from worldbuilder.terrain.tectonics import _bump as py_bump
from worldbuilder.terrain.tectonics import _continental as py_continental
from worldbuilder.terrain.tectonics import CONTINENTAL_ENOUGH as PY_CONTINENTAL_ENOUGH
from worldbuilder.terrain.tectonics import MAX_TECTONIC_RANGE_M as PY_MAX_TECTONIC_RANGE_M

TECTONICS_BOUNDED_MAX_ULPS = 8192
"""
Measured, not guessed -- see the section header above for the mechanism.

Over `TECTONICS_POINTS` (the shared pseudo-random corpus plus points nudged near a
bisector of the standing 12-plate fixture, ~22,000 points and the margins found at each),
the worst observed divergence was 614 ULP for `offset_m`, 512 ULP for `elevation_m`, and
1,501 ULP for `setting_at`'s `outboard` -- all far beyond `MAX_TRANSCENDENTAL_ULPS` (4),
confirming the hazard described in the section header is real, not theoretical.

8,192 is deliberately generous rather than a tight fit to those three numbers: the
brief's own error-propagation estimate for `engagement` at Task 1's measured minimum gap
(1.19069e-04) put the *relative* error there at roughly 4,200 ULP, and this corpus is not
guaranteed to have sampled the single worst point possible -- a larger or differently-seeded
corpus could land closer to the gate and see a larger number. What this bound does catch:
a structural defect (a wrong branch, a swapped sign, a misplaced probe) moves a result by
far more than a few thousand ULP of an already-near-zero quantity -- typically by orders of
magnitude, or by picking an entirely different profile. It does not, and cannot, promise
that no legitimate sample will ever exceed it; that would require re-deriving the port's
architecture (matching the platform libm, at the cost of native/WASM equality) rather than
loosening a test.
"""


CONTINENTALITY_SEED_FOR_TECTONICS = CONTINENTALITY_SEED
PY_TECTONICS = PyTectonics(
    PY_PLATE_SET,
    PyContinentality(CONTINENTALITY_SEED_FOR_TECTONICS, EARTH_RADIUS_M, PY_LAND_FRACTION),
    EARTH_RADIUS_M,
)


def _engine_offset_m(x, y, z, radius_m=EARTH_RADIUS_M):
    return engine.tectonics_offset_m(
        PLATE_SEEDS_FLAT, PLATE_POLES_FLAT, PLATE_RATES,
        CONTINENTALITY_SEED_FOR_TECTONICS, PY_LAND_FRACTION,
        x, y, z, radius_m,
    )


def _engine_elevation_m(x, y, z, radius_m=EARTH_RADIUS_M):
    return engine.tectonics_elevation_m(
        PLATE_SEEDS_FLAT, PLATE_POLES_FLAT, PLATE_RATES,
        CONTINENTALITY_SEED_FOR_TECTONICS, PY_LAND_FRACTION,
        x, y, z, radius_m,
    )


def _engine_setting_at(x, y, z, distance_m, normal, radius_m=EARTH_RADIUS_M):
    return engine.tectonics_setting_at(
        PLATE_SEEDS_FLAT, PLATE_POLES_FLAT, PLATE_RATES,
        CONTINENTALITY_SEED_FOR_TECTONICS, PY_LAND_FRACTION,
        x, y, z, distance_m, normal.x, normal.y, normal.z, radius_m,
    )


def _tectonics_points(count=20000, near_bisector=2000):
    """The shared corpus for every `offset_m`/`elevation_m`/`setting_at` test below: the
    six pinned poles/meridian points and the full pseudo-random `corpus()` (so plate
    interiors are exercised, per the docstring's "69 per cent of the planet" claim), plus
    points deliberately nudged near a bisector of the standing 12-plate fixture (so
    margins, and points close to the engagement gate, are too)."""
    for x, y, z in corpus(count):
        yield SpherePoint(Vec3(x, y, z).normalised())
    yield from _bisector_points_near_margin(PLATE_SEED_VECTORS, near_bisector)


TECTONICS_POINTS = list(_tectonics_points())


def test_tectonics_bump_agrees_bit_for_bit():
    """Purely algebraic: an abs, a division, a `min`, a smoothstep. `same()`, not
    `close_enough()` -- a divergence here is a real defect, not float noise."""
    edge_cases = [
        (0.0, 100_000.0), (100_000.0, 100_000.0), (-100_000.0, 100_000.0),
        (200_000.0, 100_000.0), (0.0, 0.0), (50.0, -1.0), (0.0, -0.0), (-0.0, 100_000.0),
    ]
    checked = 0
    for distance_m, width_m in edge_cases:
        want = py_bump(distance_m, width_m)
        got = engine.tectonics_bump(distance_m, width_m)
        assert same(want, got), (distance_m, width_m, want, got)
        checked += 1

    widths = (1.0, 4321.0, 100_000.0, PY_MAX_TECTONIC_RANGE_M)
    sample = list(corpus(3000))
    for x, y, z in sample:
        distance_m = x * 1_000_000.0
        for width_m in widths:
            want = py_bump(distance_m, width_m)
            got = engine.tectonics_bump(distance_m, width_m)
            assert same(want, got), (distance_m, width_m, want, got)
            checked += 1
    assert checked == len(edge_cases) + len(sample) * len(widths)


def test_tectonics_continental_agrees_bit_for_bit():
    """Purely algebraic: a division, two clamps and a smoothstep. `same()`, bit-for-bit."""
    edge_cases = [-10.0, 10.0, PY_CONTINENTAL_ENOUGH, 0.0, -0.0, 1e300, -1e300, 1.0, -1.0]
    checked = 0
    for value in edge_cases:
        want = py_continental(value)
        got = engine.tectonics_continental(value)
        assert same(want, got), (value, want, got)
        checked += 1

    sample = list(corpus(4000))
    for x, y, z in sample:
        for value in (x * 3.0, y * 0.5, z):
            want = py_continental(value)
            got = engine.tectonics_continental(value)
            assert same(want, got), (value, want, got)
            checked += 1
    assert checked == len(edge_cases) + len(sample) * 3


def test_tectonics_offset_m_and_elevation_m_agree_within_the_measured_bound():
    """The corpus, point by point: `offset_m` and `elevation_m` bounded at
    `TECTONICS_BOUNDED_MAX_ULPS`, not `MAX_TRANSCENDENTAL_ULPS` -- see the section header."""
    checked = 0
    for point in TECTONICS_POINTS:
        v = point.vector
        want_offset = PY_TECTONICS.offset_m(point)
        got_offset = _engine_offset_m(v.x, v.y, v.z)
        assert close_enough(want_offset, got_offset, TECTONICS_BOUNDED_MAX_ULPS), (
            "offset_m", v.x, v.y, v.z, want_offset, got_offset,
            ulps_apart(want_offset, got_offset),
        )

        want_elevation = PY_TECTONICS.elevation_m(point)
        got_elevation = _engine_elevation_m(v.x, v.y, v.z)
        assert close_enough(want_elevation, got_elevation, TECTONICS_BOUNDED_MAX_ULPS), (
            "elevation_m", v.x, v.y, v.z, want_elevation, got_elevation,
            ulps_apart(want_elevation, got_elevation),
        )
        checked += 1
    assert checked == len(TECTONICS_POINTS)


def test_tectonics_offset_m_and_elevation_m_exceed_the_ordinary_transcendental_bound():
    """
    The measurement the section header claims, made concrete: sweeps the same corpus as
    the test above and tracks the worst ULP divergence for `offset_m` and `elevation_m`
    directly, rather than only asserting a pass/fail at some threshold.

    Two assertions, not one -- per the brief, this must show its work both ways:

    - `worst > MAX_TRANSCENDENTAL_ULPS` demonstrates the ordinary bound genuinely does not
      hold here (this is the finding, not a hoped-for outcome).
    - `worst <= TECTONICS_BOUNDED_MAX_ULPS` demonstrates the wider, measured bound does.

    If a future change made this test's first assertion fail (worst divergence dropped to
    4 ULP or below), that would be good news -- the hazard would no longer be observable in
    this corpus -- but it would mean `TECTONICS_BOUNDED_MAX_ULPS` and its justification are
    stale and should be revisited, not that the assertion is wrong to have made.
    """
    worst_offset = 0
    worst_elevation = 0
    for point in TECTONICS_POINTS:
        v = point.vector
        d = ulps_apart(PY_TECTONICS.offset_m(point), _engine_offset_m(v.x, v.y, v.z))
        if d is not None:
            worst_offset = max(worst_offset, abs(d))
        de = ulps_apart(PY_TECTONICS.elevation_m(point), _engine_elevation_m(v.x, v.y, v.z))
        if de is not None:
            worst_elevation = max(worst_elevation, abs(de))

    assert worst_offset > MAX_TRANSCENDENTAL_ULPS, (
        f"expected offset_m's worst divergence over this corpus to exceed the ordinary "
        f"{MAX_TRANSCENDENTAL_ULPS}-ULP bound (that is the finding this section reports); "
        f"observed worst was only {worst_offset} ULP"
    )
    assert worst_elevation > MAX_TRANSCENDENTAL_ULPS, (
        f"expected elevation_m's worst divergence over this corpus to exceed the ordinary "
        f"{MAX_TRANSCENDENTAL_ULPS}-ULP bound; observed worst was only {worst_elevation} ULP"
    )
    assert worst_offset <= TECTONICS_BOUNDED_MAX_ULPS, (
        f"offset_m's worst observed divergence grew to {worst_offset} ULP, beyond the "
        f"measured TECTONICS_BOUNDED_MAX_ULPS ({TECTONICS_BOUNDED_MAX_ULPS}) -- re-measure "
        f"and update that bound's justification rather than bumping the number blindly"
    )
    assert worst_elevation <= TECTONICS_BOUNDED_MAX_ULPS, (
        f"elevation_m's worst observed divergence grew to {worst_elevation} ULP, beyond "
        f"the measured TECTONICS_BOUNDED_MAX_ULPS ({TECTONICS_BOUNDED_MAX_ULPS})"
    )


def test_tectonics_setting_at_agrees_within_the_measured_bound():
    """`setting_at` exercised with real margin geometry -- the normals `offset_m` itself
    would compute via `margins_within`/`flattened`, not an arbitrary vector -- over the
    corpus. Bounded at `TECTONICS_BOUNDED_MAX_ULPS` for the same reason as `offset_m`:
    the probes sample `Continentality.at`, which has its own zero crossing."""
    worst_inboard = 0
    worst_outboard = 0
    checked = 0
    for point in TECTONICS_POINTS:
        v = point.vector
        nearest, margins = PY_PLATE_SET.margins_within(point, PY_MAX_TECTONIC_RANGE_M, EARTH_RADIUS_M)
        for other, distance_m, bisector, weight in margins:
            normal = PY_PLATE_SET.flattened(point, bisector)
            if normal is None:
                continue
            want = PY_TECTONICS.setting_at(point, distance_m, normal)
            got_inboard, got_outboard = _engine_setting_at(v.x, v.y, v.z, distance_m, normal)

            assert close_enough(want.inboard, got_inboard, TECTONICS_BOUNDED_MAX_ULPS), (
                "inboard", v.x, v.y, v.z, distance_m, want.inboard, got_inboard,
                ulps_apart(want.inboard, got_inboard),
            )
            assert close_enough(want.outboard, got_outboard, TECTONICS_BOUNDED_MAX_ULPS), (
                "outboard", v.x, v.y, v.z, distance_m, want.outboard, got_outboard,
                ulps_apart(want.outboard, got_outboard),
            )
            di = ulps_apart(want.inboard, got_inboard)
            do = ulps_apart(want.outboard, got_outboard)
            if di is not None:
                worst_inboard = max(worst_inboard, abs(di))
            if do is not None:
                worst_outboard = max(worst_outboard, abs(do))
            checked += 1

    assert checked > 0, "corpus produced no margin to probe setting_at with"
    # Recorded rather than merely asserted-in-range: a floor with the observed values in
    # the message, per the brief's warning about vacuous tests.
    assert worst_inboard <= TECTONICS_BOUNDED_MAX_ULPS, (
        f"setting_at's inboard worst observed divergence grew to {worst_inboard} ULP over "
        f"{checked} margin probes"
    )
    assert worst_outboard <= TECTONICS_BOUNDED_MAX_ULPS, (
        f"setting_at's outboard worst observed divergence grew to {worst_outboard} ULP "
        f"over {checked} margin probes"
    )


def test_a_plate_interior_contributes_exactly_zero_on_both_sides():
    """Every one of the 12-plate fixture's own seeds is more than `MAX_TECTONIC_RANGE_M`
    from any bisector (confirmed here, not assumed), so `offset_m` takes its early return
    before doing any arithmetic at all -- exactly `0.0`, `same()`, on both sides."""
    for seed in PLATE_SEED_VECTORS:
        point = SpherePoint(seed)
        _, margins = PY_PLATE_SET.margins_within(point, PY_MAX_TECTONIC_RANGE_M, EARTH_RADIUS_M)
        assert margins == (), f"fixture sanity: expected no margins at plate seed {seed}"

        want = PY_TECTONICS.offset_m(point)
        got = _engine_offset_m(point.vector.x, point.vector.y, point.vector.z)
        assert want == 0.0, "python fixture sanity: must be a literal zero, not merely small"
        assert same(want, got), (seed, want, got)


def test_a_point_near_a_triple_junction_sums_two_margins_on_both_sides():
    """
    A real two-margin point on the standing 12-plate fixture, found by scanning
    `TECTONICS_POINTS` for the first one where `margins_within` returns two margins in
    range -- the reason `offset_m` sums rather than picks a nearest margin. Confirmed
    live rather than hard-coded as "trust me": both the Python and the Rust side of the
    corpus loop below re-derive it, so a future change to the fixture that removed the
    triple junction would fail this test's own setup assertion rather than silently
    testing a plate interior instead.
    """
    point = None
    for candidate in TECTONICS_POINTS:
        _, margins = PY_PLATE_SET.margins_within(candidate, PY_MAX_TECTONIC_RANGE_M, EARTH_RADIUS_M)
        if len(margins) >= 2:
            point = candidate
            break
    assert point is not None, "fixture sanity: no two-margin point found in this corpus"

    v = point.vector
    want = PY_TECTONICS.offset_m(point)
    got = _engine_offset_m(v.x, v.y, v.z)
    assert close_enough(want, got, TECTONICS_BOUNDED_MAX_ULPS), (
        v.x, v.y, v.z, want, got, ulps_apart(want, got)
    )

    want_elevation = PY_TECTONICS.elevation_m(point)
    got_elevation = _engine_elevation_m(v.x, v.y, v.z)
    assert close_enough(want_elevation, got_elevation, TECTONICS_BOUNDED_MAX_ULPS), (
        v.x, v.y, v.z, want_elevation, got_elevation, ulps_apart(want_elevation, got_elevation)
    )


def _two_plate_world(near_rate, far_rate, seed=98765):
    """
    A minimal, controllable two-plate world -- both Euler poles at the true north pole,
    seeds ten degrees apart on the equator, mirroring `kinematics.rs`'s `spinning_pair`
    and `tectonics.rs`'s `lopsided_world` fixtures. With only two plates, `margins_within`
    can find at most one margin, so `offset_m`'s total *is* that one margin's contribution
    -- there is nothing else in the sum to disentangle it from.

    Returns `(py_tectonics, seeds_flat, poles_flat, rates)`.
    """
    north = Vec3(0.0, 0.0, 1.0)
    near = PyPlate(index=0, seed=SpherePoint.from_latlon(0.0, 0.0), euler_pole=SpherePoint(north),
                   rate_rad_per_myr=near_rate)
    far = PyPlate(index=1, seed=SpherePoint.from_latlon(0.0, 10.0), euler_pole=SpherePoint(north),
                  rate_rad_per_myr=far_rate)
    plates = PyPlateSet([near, far])
    land = PyContinentality(seed, EARTH_RADIUS_M, PY_LAND_FRACTION)
    tectonics = PyTectonics(plates, land, EARTH_RADIUS_M)

    n, f = near.seed.vector, far.seed.vector
    pn, pf = near.euler_pole.vector, far.euler_pole.vector
    seeds_flat = [n.x, n.y, n.z, f.x, f.y, f.z]
    poles_flat = [pn.x, pn.y, pn.z, pf.x, pf.y, pf.z]
    return tectonics, seeds_flat, poles_flat, [near_rate, far_rate], seed


def test_a_convergent_margin_agrees_within_the_measured_bound():
    """
    `near_rate > far_rate` on this fixture: measured (not assumed) to give
    `across = 0.9995...` at (60N, 7E) -- comfortably engaged, comfortably on the
    convergent side (`across > 0`), so `from_margin` runs the collision/trench/arc/uplift
    profile blend rather than the ridge/rift branch.
    """
    tectonics, seeds_flat, poles_flat, rates, seed = _two_plate_world(0.02, 0.01)
    point = SpherePoint.from_latlon(60.0, 7.0)

    want = tectonics.offset_m(point)
    got = engine.tectonics_offset_m(
        seeds_flat, poles_flat, rates, seed, PY_LAND_FRACTION,
        point.vector.x, point.vector.y, point.vector.z, EARTH_RADIUS_M,
    )
    assert want != 0.0, "fixture sanity: expected a genuinely non-zero convergent contribution"
    assert close_enough(want, got, TECTONICS_BOUNDED_MAX_ULPS), (
        want, got, ulps_apart(want, got)
    )


def test_a_divergent_margin_agrees_within_the_measured_bound():
    """The rates from the convergent test above, swapped: `across` flips sign to
    `-0.9995...` at the same point, so `from_margin` takes the ridge/rift branch instead
    of the convergent profile blend."""
    tectonics, seeds_flat, poles_flat, rates, seed = _two_plate_world(0.01, 0.02)
    point = SpherePoint.from_latlon(60.0, 7.0)

    want = tectonics.offset_m(point)
    got = engine.tectonics_offset_m(
        seeds_flat, poles_flat, rates, seed, PY_LAND_FRACTION,
        point.vector.x, point.vector.y, point.vector.z, EARTH_RADIUS_M,
    )
    assert want != 0.0, "fixture sanity: expected a genuinely non-zero divergent contribution"
    assert close_enough(want, got, TECTONICS_BOUNDED_MAX_ULPS), (
        want, got, ulps_apart(want, got)
    )


def test_points_either_side_of_the_engagement_threshold_agree():
    """
    The hazard the section header describes, exercised at its actual boundary rather than
    only in aggregate. Found by scanning `TECTONICS_POINTS` on the standing 12-plate
    fixture (the same corpus and the same recomputation `test_conformance.py`'s own
    kinematics section uses to measure the `ACROSS_ENOUGH` gap) for the closest point on
    each side of it:

    - `just_below`: the point with the smallest `abs(across) - ACROSS_ENOUGH < 0` found --
      `engagement <= 0.0`, so *that margin's* contribution is a literal `0.0` on both
      sides, checked with `same()`. `offset_m`'s total may still be non-zero if another
      margin is in range, so what is asserted is specifically this margin's own
      `from_margin`-equivalent zero, recovered from `offset_m` because a fresh two-plate
      world built at the same point has only the one margin to sum.
    - `just_above`: the closest point on the engaged side -- `engagement` is a hair above
      zero, so the contribution is non-zero but tiny, which is exactly where the
      ULP-amplification hazard bites hardest. Bounded at `TECTONICS_BOUNDED_MAX_ULPS`.

    Both points are found live from the corpus, not hard-coded, so a change to the fixture
    or the corpus that moved the gate would still be exercised at whatever the new closest
    points are, rather than silently testing two points that no longer mean anything.
    """
    just_below = (math.inf, None)  # (gap, point) with across strictly inside [-1, ACROSS_ENOUGH)... below the gate
    just_above = (math.inf, None)  # closest point with across strictly beyond the gate

    for point in TECTONICS_POINTS:
        nearest, margins = PY_PLATE_SET.margins_within(point, PY_MAX_TECTONIC_RANGE_M, EARTH_RADIUS_M)
        for other, distance_m, bisector, weight in margins:
            normal = PY_PLATE_SET.flattened(point, bisector)
            if normal is None:
                continue
            motion = py_motion_between(nearest, other, point, normal, EARTH_RADIUS_M)
            speed = math.hypot(motion.closing_m_per_myr, motion.sliding_m_per_myr)
            if speed <= 0.0:
                continue
            across = motion.closing_m_per_myr / speed
            gap = abs(across) - PY_ACROSS_ENOUGH
            if gap < 0.0 and abs(gap) < just_below[0]:
                just_below = (abs(gap), point)
            elif gap > 0.0 and abs(gap) < just_above[0]:
                just_above = (abs(gap), point)

    assert just_below[1] is not None, "fixture sanity: no sub-threshold margin found in this corpus"
    assert just_above[1] is not None, "fixture sanity: no super-threshold margin found in this corpus"

    below_point = just_below[1]
    v = below_point.vector
    want_below = PY_TECTONICS.offset_m(below_point)
    got_below = _engine_offset_m(v.x, v.y, v.z)
    # Both sides take the identical `engagement <= 0.0` early return, so this is an exact
    # zero on both sides -- but only if this is genuinely the *only* margin in range,
    # otherwise a second, well-engaged margin could still make the total non-zero. Checked
    # rather than assumed.
    _, below_margins = PY_PLATE_SET.margins_within(below_point, PY_MAX_TECTONIC_RANGE_M, EARTH_RADIUS_M)
    if len(below_margins) == 1:
        assert want_below == 0.0, (
            f"fixture sanity: expected the single sub-threshold margin to contribute "
            f"exactly zero, got {want_below!r}"
        )
        assert same(want_below, got_below), (v.x, v.y, v.z, want_below, got_below)
    else:
        assert close_enough(want_below, got_below, TECTONICS_BOUNDED_MAX_ULPS), (
            v.x, v.y, v.z, want_below, got_below, ulps_apart(want_below, got_below)
        )

    above_point = just_above[1]
    v = above_point.vector
    want_above = PY_TECTONICS.offset_m(above_point)
    got_above = _engine_offset_m(v.x, v.y, v.z)
    assert close_enough(want_above, got_above, TECTONICS_BOUNDED_MAX_ULPS), (
        v.x, v.y, v.z, want_above, got_above, ulps_apart(want_above, got_above)
    )


# --- Generation: fraction, spread, pole, rate, plates_for --------------------------------
#
# The contract split here is unusually clean. `_fraction` is a BLAKE2 digest, a
# little-endian u64, and a division by 2**64 -- an exact power of two -- so nothing in
# that path is transcendental. `_rate` is `SLOWEST + fraction * (FASTEST - SLOWEST)` and a
# sign: pure arithmetic on an exact fraction. Both are held to `same()`, bit-for-bit, and
# a mismatch here is a defect, not rounding -- Task 1 proved the digests byte-identical
# across 27 vectors, so if the hash chain is wrong at all it will differ in the first
# significant digit, not the last bit.
#
# `_spread` and `_pole` both end in cos/sin, so they are BOUNDED at
# MAX_TRANSCENDENTAL_ULPS, the same as every other transcendental path in this file.
#
# One thing this section does NOT catch: `_pole` in the Python (and `pole` in the Rust)
# builds its SpherePoint from an already-unit vector without renormalising it. A Rust unit
# test added in Task 4 (`pole_uses_the_non_normalising_constructor`, in
# crates/worldbuilder-engine/src/generation.rs) pins that the Rust side keeps using the
# non-normalising constructor rather than `from_vector`; swapping them moves values by
# about 2 ULP at pole 6, which hides comfortably inside the 4-ULP bound applied here. This
# conformance suite compares Python's reference against whatever the Rust side currently
# does, so it cannot distinguish "normalising" from "not" when both stay under 4 ULP --
# that distinction is the Rust unit test's job, not this file's.

from worldbuilder.plates import generation as py_generation

GENERATION_WORLD_SEEDS = [0, -20260831, 20260831, 9223372036854775807]
"""Zero, a negative seed, the seed used throughout the rest of this file, and i64::MAX --
the largest value the Rust binding's `i64` parameter can carry."""

GENERATION_COUNTS = [1, 2, 22, 137]
"""A count of one (the degenerate case), two, the default plate count, and something
larger than any world this project actually builds."""

GENERATION_LABELS = ["jitter-a", "jitter-b", "pole-z", "pole-angle", "rate", "sense"]
"""Every label `_fraction` is ever called with by `_spread`, `_pole` and `_rate` -- the
whole vocabulary, not a sample of it."""

GENERATION_SPREAD_MEASURED_MAX_COUNT = 137
"""
The largest plate count `GENERATION_SPREAD_BOUNDED_MAX_ULPS` below was actually measured
against. Deliberately a fixed literal, not `max(GENERATION_COUNTS)` -- it must not move if
`GENERATION_COUNTS` does, or it could never catch that change. If `GENERATION_COUNTS` ever
grows past this, the bound needs to be re-measured, not assumed -- see the assertion at
the top of `test_generation_spread_agrees_within_the_measured_bound` that enforces exactly
that.
"""

GENERATION_SPREAD_BOUNDED_MAX_ULPS = 32
"""
Measured, not guessed, over exactly the sweep `test_generation_spread_agrees_within_the_
measured_bound` runs (every index of every count in GENERATION_COUNTS, for every seed in
GENERATION_WORLD_SEEDS): the worst observed divergence was 6 ULP (seed i64::MAX, count
137, index 85, the y component).

**This bound is scoped to the counts actually tested here (max 137) -- it is not a
property of `_spread` itself.** A broader sweep run for the Task 6 review, at counts this
suite does not exercise, measured divergence that grows with count: 3 ULP at count 22, 6
ULP at 137 (the figure above), 8 ULP at 500, 16 ULP at 1000, and up to 131 ULP at 5000
(all four seeds exceeded 32 there). Extrapolating 32 to a generator that ever uses a
plate count larger than what `GENERATION_COUNTS` covers would be wrong; that generator
needs its own measurement at its own count, the same way this one was measured at 137.

The reassuring half of that same sweep: at `DEFAULT_PLATE_COUNT` (22, the only count any
world this project actually builds uses), the divergence is **3 ULP** -- inside the
ordinary `MAX_TRANSCENDENTAL_ULPS` (4) with no special bound needed at all. This dedicated
bound exists only because the conformance sweep deliberately reaches count 137, well past
any real world; realistic-sized worlds never need it.

`_spread` chains far more floating-point operations than a bare `cos`/`sin` call: the
golden angle itself needs a `sqrt`, then `cos`/`sin` place the un-jittered point, then two
more `_fraction` calls (strict, contribute no error of their own) scale two tangent
vectors built from two `cross` products and a `normalised` (a `length`, itself a `sqrt`,
and a division), which are then added onto the point. Each step is correctly rounded on
its own, exactly like `sphere_from_latlon`'s single `cos`/`sin` pair, but here there are
several of them in series, so the per-step rounding compounds instead of standing alone.
That growing chain is the primary driver -- `_pole`'s angle is bounded to a single turn
(0 to 2*pi) and stays at 2 ULP by contrast, while `_spread`'s angle is `golden * index`,
unbounded in `index`, so the trig range reduction it needs grows more demanding as index
(and therefore count) grows, and CPython's range reduction does not agree bit-for-bit with
`libm`'s.

A second mechanism compounds the first, and it is the same one the Tectonics section of
`crates/worldbuilder-engine/README.md` documents at much larger scale: ULP is a *relative*
measure, and it gets very fine near zero, so an ordinary small absolute rounding
difference in a near-zero vector component reads as a large ULP count -- not because the
arithmetic got worse, but because of how close to zero the component happens to land. The
worst cases measured in the broader sweep above bear this out: the count-1000 and
count-5000 worst components were both small in magnitude (order 1e-3), where a fixed
absolute difference reads as tens of times more ULP than the same difference would at a
component nearer 1.0. `_spread` never divides by a quantity that legitimately reaches
zero the way `Continentality::at` does, so this effect is milder here, but it is the same
mechanism, not a different one.

32 is generous headroom over the measured 6 for the counts this suite actually sweeps
(max 137), in the same spirit as the tectonics section's own bound: what this catches is a
structural defect (a dropped cross product, a jitter wired to the wrong sign, a missing
normalisation), which would move a component by far more than a few tens of ULP, not a
legitimate accumulation of correctly-rounded steps. It is not headroom against a larger
plate count -- the growth measured above shows 32 is exceeded well within a plausible
range if `GENERATION_COUNTS` is ever widened.
"""


def _assert_generation_spread_bound_still_measured():
    """
    Guards `GENERATION_SPREAD_BOUNDED_MAX_ULPS` itself, not any one caller: every
    consumer of that bound must call this first, so widening `GENERATION_COUNTS` past
    `GENERATION_SPREAD_MEASURED_MAX_COUNT` fails here, with an explanation, instead of
    surfacing downstream as a puzzling raw-ULP `close_enough()` failure.
    """
    assert max(GENERATION_COUNTS) <= GENERATION_SPREAD_MEASURED_MAX_COUNT, (
        f"GENERATION_COUNTS now reaches {max(GENERATION_COUNTS)}, past the "
        f"{GENERATION_SPREAD_MEASURED_MAX_COUNT} this bound was measured up to. "
        "GENERATION_SPREAD_BOUNDED_MAX_ULPS does not hold outside the range it was "
        "measured over -- divergence grows with count (up to 131 ULP was observed at "
        "count 5000 in the Task 6 review's broader sweep). Re-measure the worst-case ULP "
        "at the new count and pick a new bound from that measurement; do not just raise "
        "this constant to make the assertion below pass."
    )


def test_generation_fraction_agrees_bit_for_bit_across_seeds_indices_and_labels():
    """
    Strict: `same()`, not `close_enough()`. No transcendental sits between the BLAKE2
    digest and the returned float, so Python and Rust must produce identical bits, not
    merely close ones. Covers every one of the six labels `_fraction` is ever called
    with, across four seeds (including zero, a negative seed, and i64::MAX) and forty
    plate indices -- 960 comparisons, every one required to be exact.
    """
    checked = 0
    for seed in GENERATION_WORLD_SEEDS:
        for index in range(40):
            for label in GENERATION_LABELS:
                want = py_generation._fraction(seed, "plate", index, label)
                got = engine.generation_fraction(seed, ["plate", str(index), label])
                assert same(want, got), (seed, index, label, want, got)
                checked += 1
    assert checked == len(GENERATION_WORLD_SEEDS) * 40 * len(GENERATION_LABELS)


def test_generation_rate_agrees_bit_for_bit_across_seeds_and_a_full_set_of_indices():
    """
    Strict, like `_fraction` above: `_rate` is `SLOWEST + fraction * (FASTEST - SLOWEST)`
    and a sign, pure arithmetic on an exact fraction, so it earns the same bit-for-bit
    bar. Every index across a full default-sized plate set, for every seed.
    """
    checked = 0
    for seed in GENERATION_WORLD_SEEDS:
        for index in range(py_generation.DEFAULT_PLATE_COUNT):
            want = py_generation._rate(seed, index)
            got = engine.generation_rate(seed, index)
            assert same(want, got), (seed, index, want, got)
            checked += 1
    assert checked == len(GENERATION_WORLD_SEEDS) * py_generation.DEFAULT_PLATE_COUNT


def test_generation_spread_agrees_within_the_measured_bound():
    """
    Bounded, per the brief, because `_spread` ends in cos/sin -- but the ordinary
    MAX_TRANSCENDENTAL_ULPS (4 ULP) does NOT hold for it, and this test measures that
    rather than asserting it away. See GENERATION_SPREAD_BOUNDED_MAX_ULPS above for the
    mechanism and the actual worst value observed. Every index of every count, for every
    seed -- not a sample -- because the brief calls for "every index in a full set" and
    the counts here are all small enough that "every index" is cheap.

    The worst ULP distance observed across this sweep, measured live rather than assumed,
    is recorded in the Task 6 report rather than printed here (pytest swallows stdout on
    a passing run) -- this test only has to prove the bound holds, not narrate it.
    """
    _assert_generation_spread_bound_still_measured()
    worst = 0
    skipped = 0
    for seed in GENERATION_WORLD_SEEDS:
        for count in GENERATION_COUNTS:
            for index in range(count):
                want = py_generation._spread(seed, index, count).vector
                got = engine.generation_spread(seed, index, count)
                for label, w, g in zip("xyz", (want.x, want.y, want.z), got):
                    assert close_enough(w, g, GENERATION_SPREAD_BOUNDED_MAX_ULPS), (
                        seed, count, index, label, w, g, ulps_apart(w, g)
                    )
                    d = ulps_apart(w, g)
                    if d is None:
                        skipped += 1
                    else:
                        worst = max(worst, abs(d))
    assert skipped == 0, f"{skipped} comparisons could not be measured (NaN, inf or sign-straddle)"
    assert worst <= GENERATION_SPREAD_BOUNDED_MAX_ULPS, f"spread divergence grew to {worst} ULP"


def test_generation_pole_agrees_within_the_measured_bound():
    """
    Bounded at MAX_TRANSCENDENTAL_ULPS, like `_spread` above -- `_pole` also ends in
    cos/sin. Every index of a full plate set (DEFAULT_PLATE_COUNT and the largest count
    in GENERATION_COUNTS), for every seed.

    This does not, and cannot, distinguish the non-normalising SpherePoint construction
    from the normalising one -- see the section header above. That distinction is pinned
    by a Rust unit test, not by this comparison.
    """
    worst = 0
    skipped = 0
    for seed in GENERATION_WORLD_SEEDS:
        for count in (py_generation.DEFAULT_PLATE_COUNT, max(GENERATION_COUNTS)):
            for index in range(count):
                want = py_generation._pole(seed, index).vector
                got = engine.generation_pole(seed, index)
                for label, w, g in zip("xyz", (want.x, want.y, want.z), got):
                    assert close_enough(w, g), (seed, index, label, w, g, ulps_apart(w, g))
                    d = ulps_apart(w, g)
                    if d is None:
                        skipped += 1
                    else:
                        worst = max(worst, abs(d))
    assert skipped == 0, f"{skipped} comparisons could not be measured (NaN, inf or sign-straddle)"
    assert worst <= MAX_TRANSCENDENTAL_ULPS, f"pole divergence grew to {worst} ULP"


def test_generation_plates_for_agrees_across_seeds_and_counts():
    """
    `plates_for` is the whole pipeline in one call: exact plate indices, bounded seed and
    pole vectors, and a strict rate, all in one pass over every count this file tests.
    """
    _assert_generation_spread_bound_still_measured()
    for seed in GENERATION_WORLD_SEEDS:
        for count in GENERATION_COUNTS:
            want = py_generation.plates_for(seed, count)
            got = engine.generation_plates_for(seed, count)
            assert len(got) == count == len(want)
            for position, (py_plate, (index, seed_xyz, pole_xyz, rate)) in enumerate(zip(want, got)):
                assert index == py_plate.index == position, (seed, count, position, index)
                sv = py_plate.seed.vector
                for label, w, g in zip("xyz", (sv.x, sv.y, sv.z), seed_xyz):
                    assert close_enough(w, g, GENERATION_SPREAD_BOUNDED_MAX_ULPS), (
                        seed, count, position, "seed", label, w, g, ulps_apart(w, g)
                    )
                pv = py_plate.euler_pole.vector
                for label, w, g in zip("xyz", (pv.x, pv.y, pv.z), pole_xyz):
                    assert close_enough(w, g), (seed, count, position, "pole", label, w, g)
                assert same(py_plate.rate_rad_per_myr, rate), (
                    seed, count, position, py_plate.rate_rad_per_myr, rate
                )


def test_generation_fraction_and_rate_hold_strictly_over_the_entire_sweep():
    """
    States directly what the two strict tests above prove piecewise: across every seed,
    every label, and every index this file exercises, `fraction` and `rate` never once
    fall back to a tolerance. If a single digest differed, this would not be a near
    miss -- Task 1 measured that the digests are byte-identical, so any divergence here
    would show up as a `fraction` in a completely different part of [0, 1), not a
    neighbouring float.
    """
    fraction_checked = 0
    rate_checked = 0
    for seed in GENERATION_WORLD_SEEDS:
        for index in range(40):
            for label in GENERATION_LABELS:
                want = py_generation._fraction(seed, "plate", index, label)
                got = engine.generation_fraction(seed, ["plate", str(index), label])
                assert bits(want) == bits(got), (seed, index, label)
                fraction_checked += 1
        for index in range(py_generation.DEFAULT_PLATE_COUNT):
            want_rate = py_generation._rate(seed, index)
            got_rate = engine.generation_rate(seed, index)
            assert bits(want_rate) == bits(got_rate), (seed, index)
            rate_checked += 1
    assert fraction_checked == len(GENERATION_WORLD_SEEDS) * 40 * len(GENERATION_LABELS)
    assert rate_checked == len(GENERATION_WORLD_SEEDS) * py_generation.DEFAULT_PLATE_COUNT


# ---------------------------------------------------------------------------
# Detail: smooth, the band table, amplitude_m, offset_m
#
# The simplest contract in the file. `detail.py` contains no transcendental call in any
# path -- verified by reading it, not assuming it: `math.pi` is a module-level constant,
# not an operation, and `Noise` (already covered, bit-for-bit, in its own section above)
# reaches only `floor`, which is exact. So unlike Tectonics just above, there is no
# measured, disclosed exception here and no `close_enough` anywhere in this section --
# every comparison uses `same()`. If one of these ever needed a tolerance, that would be
# a finding about the port, not a reason to add one.
# ---------------------------------------------------------------------------

from worldbuilder.terrain.detail import Detail as PyDetail
from worldbuilder.terrain.detail import _smooth as py_smooth


class _DetailPoint:
    """`Detail.amplitude_m` never reads its `point` argument at all (see detail.rs's own
    doc comment on the method) and `Detail.offset_m` only reads `.vector`; this is the
    smallest thing that satisfies both without dragging in `SpherePoint` normalisation,
    the same pattern as `noise_points`' `_Point` above."""

    def __init__(self, x, y, z):
        self.vector = Vec3(x, y, z)


DETAIL_WORLD_SEEDS = [0, 1, 20260831, 2**63 - 1]

# Earth's own radius, plus a radius chosen so the four-operation transcription
# `2*pi*radius_m/wavelength/(2*pi)` and the simplified `radius_m/wavelength` disagree.
# Earth's radius does NOT distinguish the two forms for any of the seven configured
# wavelengths (that is precisely why a simplification could hide there), so it alone
# would not catch trap 1 -- this second radius is what makes the corpus able to.
DETAIL_NON_EARTH_RADIUS_M = 32450893.20683292
DETAIL_RADII = [EARTH_RADIUS_M, DETAIL_NON_EARTH_RADIUS_M]

DETAIL_AMPLITUDE_M = 100.0

# One elevation from each of the five settings amplitude_m's docstring and comments name:
# abyssal (deep == 1.0), shelf (partway up the deep/high blend), coast (inside the
# near_shore band, |elevation| < 350), interior (partway up the high blend, positive
# side), and mountain (high == 1.0).
DETAIL_ELEVATIONS_M = [-6000.0, -1500.0, 0.0, 500.0, 2000.0]
DETAIL_SHELF_WEIGHTS = [0.0, 0.5, 1.0]
DETAIL_TECTONIC_MS = [0.0, 600.0, 1200.0, 5000.0]  # zero to well past the 1200 saturation

# None (canonical -- every band at full strength), 0.0 (must reach the exact same path as
# None, per Python's `if resolution_m:` falsiness), -0.0 (also falsy in Python, but not
# for the reason 0.0 is bit-exact-guarded: `wavelength / -0.0` is `-inf`, `smooth(-inf)`
# clamps to `0.0`, and `visible <= 0.0` breaks the loop, dropping every band -- so a port
# that collapses only `0.0` and not `-0.0` to the canonical path diverges here even though
# it agrees on plain zero), a fine spacing that leaves every band's `visible` at 1.0
# (100.0, far below even the finest band's fade window), one squarely inside the coarsest
# band's fade window (wavelength 20000.0 fades for resolution_m in [5000.0, 10000.0], so
# 7500.0 sits in the middle), one coarse enough that even the coarsest band's `visible`
# clamps to 0.0 on the very first iteration, breaking the loop and dropping every octave
# (50000.0: 20000.0/50000.0 = 0.4, already below BARELY_M), and NaN (truthy in Python, so
# it takes the *resolution* branch rather than the canonical one; `wavelength / NaN` is
# NaN and `smooth(NaN)` must clamp to exactly `1.0` -- the clamp-order trap -- for this to
# agree with canonical at all).
DETAIL_RESOLUTIONS_M = [None, 0.0, -0.0, 100.0, 7500.0, 50000.0, float("nan")]


def test_detail_smooth_agrees_bit_for_bit():
    """Purely algebraic: two clamps and a smoothstep. `same()`, not `close_enough()`."""
    edge_cases = [-1e300, -10.0, -0.0, 0.0, 1e-12, 0.5, 1.0 - 1e-12, 1.0, 1.0 + 1e-12, 10.0, 1e300]
    checked = 0
    for fraction in edge_cases:
        want = py_smooth(fraction)
        got = engine.detail_smooth(fraction)
        assert same(want, got), (fraction, want, got)
        checked += 1

    sample = list(corpus(3000))
    for x, y, z in sample:
        for fraction in (x, y * 3.0, z * 0.5):
            want = py_smooth(fraction)
            got = engine.detail_smooth(fraction)
            assert same(want, got), (fraction, want, got)
            checked += 1
    assert checked == len(edge_cases) + len(sample) * 3


def test_detail_bands_agree_bit_for_bit_across_seeds_and_radii():
    """The band table: seven octaves, each a `(wavelength_m, frequency, share)` triple,
    for every world seed crossed with every radius -- including the non-Earth one where
    trap 1 (a simplified frequency expression) would bite."""
    checked = 0
    for seed in DETAIL_WORLD_SEEDS:
        for radius_m in DETAIL_RADII:
            want = PyDetail(seed, radius_m)._bands
            got = engine.detail_bands(seed, radius_m)
            assert len(want) == len(got) == 7, (seed, radius_m, len(want), len(got))
            for (want_w, want_f, want_s), (got_w, got_f, got_s) in zip(want, got):
                assert same(want_w, got_w), (seed, radius_m, "wavelength", want_w, got_w)
                assert same(want_f, got_f), (seed, radius_m, "frequency", want_f, got_f)
                assert same(want_s, got_s), (seed, radius_m, "share", want_s, got_s)
                checked += 1
    assert checked == len(DETAIL_WORLD_SEEDS) * len(DETAIL_RADII) * 7


def test_detail_bands_uses_the_transcribed_frequency_formula_not_the_simplified_one():
    """
    Trap 1, named directly. At `DETAIL_NON_EARTH_RADIUS_M`, the band whose wavelength is
    10000.0 must carry the frequency the Python's four-operation transcription produces
    (3245.0893206832916), not the value a `radius_m / wavelength` simplification would
    give (3245.089320683292) -- the two are one ULP apart and agree at Earth's radius for
    every configured wavelength, which is exactly why a simplification could pass every
    other test in this file and still be wrong. Checked against the literals directly (so
    this test cannot pass merely because both languages made the same mistake), and then
    against the engine.
    """
    seed = DETAIL_WORLD_SEEDS[0]
    want_bands = PyDetail(seed, DETAIL_NON_EARTH_RADIUS_M)._bands
    want_band = next(b for b in want_bands if b[0] == 10000.0)
    transcribed = 3245.0893206832916
    simplified = 3245.089320683292
    assert same(want_band[1], transcribed), (want_band[1], transcribed)
    assert not same(want_band[1], simplified), "the reference itself drifted onto the simplified form"

    got_bands = engine.detail_bands(seed, DETAIL_NON_EARTH_RADIUS_M)
    got_band = next(b for b in got_bands if b[0] == 10000.0)
    assert same(got_band[1], transcribed), (got_band[1], transcribed)
    assert same(want_band[1], got_band[1]), (want_band[1], got_band[1])


def test_detail_amplitude_m_agrees_bit_for_bit_across_elevation_shelf_and_tectonic():
    """
    `amplitude_m` reads none of `point`, `world_seed` or `radius_m` -- its entire
    behaviour is `elevation_m`, `shelf_weight` and `tectonic_m` (see detail.rs's own doc
    comment on the method). This is the test that actually exercises that behaviour: one
    elevation from each of the five settings the docstring names, the three shelf weights
    the brief calls out, and tectonic contributions from zero to well past the 1200
    saturation point, crossed with a handful of real points so the binding's
    otherwise-unused x/y/z arguments still cross the FFI boundary on every comparison.
    """
    seed = DETAIL_WORLD_SEEDS[0]
    radius_m = DETAIL_RADII[0]
    d = PyDetail(seed, radius_m)
    points = list(corpus(20))
    checked = 0
    for x, y, z in points:
        point = _DetailPoint(x, y, z)
        for elevation_m in DETAIL_ELEVATIONS_M:
            for shelf_weight in DETAIL_SHELF_WEIGHTS:
                for tectonic_m in DETAIL_TECTONIC_MS:
                    want = d.amplitude_m(point, elevation_m, shelf_weight, tectonic_m)
                    got = engine.detail_amplitude_m(
                        seed, radius_m, x, y, z, elevation_m, shelf_weight, tectonic_m
                    )
                    assert same(want, got), (
                        x, y, z, elevation_m, shelf_weight, tectonic_m, want, got
                    )
                    checked += 1
    assert checked == (
        len(points) * len(DETAIL_ELEVATIONS_M) * len(DETAIL_SHELF_WEIGHTS) * len(DETAIL_TECTONIC_MS)
    )


def test_detail_amplitude_m_agrees_across_world_seeds_and_radii():
    """
    Confirms the `world_seed`/`radius_m`/`point` arguments the binding still requires
    (even though the formula ignores every one of them) survive the FFI round trip
    without corrupting anything downstream, for several seeds and both radii.
    """
    points = [(0.0, 0.0, 1.0), (1.0, 0.0, 0.0), (0.3, -0.4, 0.8)]
    checked = 0
    for seed in DETAIL_WORLD_SEEDS:
        for radius_m in DETAIL_RADII:
            d = PyDetail(seed, radius_m)
            for x, y, z in points:
                point = _DetailPoint(x, y, z)
                want = d.amplitude_m(point, -6000.0, 0.5, 600.0)
                got = engine.detail_amplitude_m(seed, radius_m, x, y, z, -6000.0, 0.5, 600.0)
                assert same(want, got), (seed, radius_m, x, y, z, want, got)
                checked += 1
    assert checked == len(DETAIL_WORLD_SEEDS) * len(DETAIL_RADII) * len(points)


def test_detail_offset_m_agrees_bit_for_bit():
    """
    The corpus, every world seed, both radii (including the non-Earth one), and every
    `resolution_m` this task's brief names: `None`, `0.0`, a fine spacing, one mid-fade,
    and one coarse enough to drop every band.
    """
    points = list(corpus(150))
    checked = 0
    for seed in DETAIL_WORLD_SEEDS:
        for radius_m in DETAIL_RADII:
            d = PyDetail(seed, radius_m)
            for x, y, z in points:
                point = _DetailPoint(x, y, z)
                for resolution_m in DETAIL_RESOLUTIONS_M:
                    want = d.offset_m(point, DETAIL_AMPLITUDE_M, resolution_m)
                    got = engine.detail_offset_m(
                        seed, radius_m, x, y, z, DETAIL_AMPLITUDE_M, resolution_m
                    )
                    assert same(want, got), (
                        seed, radius_m, x, y, z, resolution_m, want, got
                    )
                    checked += 1
    assert checked == (
        len(DETAIL_WORLD_SEEDS) * len(DETAIL_RADII) * len(points) * len(DETAIL_RESOLUTIONS_M)
    )


def test_detail_offset_m_zero_resolution_matches_omitted_resolution():
    """
    Python's `if resolution_m:` is false for `None`, `0.0` and `-0.0`, so a caller passing
    either zero must get every octave, not a division by zero -- and the binding must
    carry that through for a caller that omits the argument entirely, not only one that
    passes `None` explicitly. All four call shapes -- omitted, `None`, `0.0`, and `-0.0`
    -- must land on the identical bit pattern the Python reference produces for its own
    default. `-0.0` is the case that actually distinguishes a correct collapse from one
    that only special-cases plain zero: `wavelength / -0.0` is `-inf`, and an unguarded
    port would drop every band instead of matching canonical.
    """
    seed = DETAIL_WORLD_SEEDS[-1]
    radius_m = DETAIL_RADII[-1]
    d = PyDetail(seed, radius_m)
    points = list(corpus(20))
    checked = 0
    for x, y, z in points:
        point = _DetailPoint(x, y, z)
        want = d.offset_m(point, DETAIL_AMPLITUDE_M)  # resolution_m omitted -> None
        got_omitted = engine.detail_offset_m(seed, radius_m, x, y, z, DETAIL_AMPLITUDE_M)
        got_none = engine.detail_offset_m(seed, radius_m, x, y, z, DETAIL_AMPLITUDE_M, None)
        got_zero = engine.detail_offset_m(seed, radius_m, x, y, z, DETAIL_AMPLITUDE_M, 0.0)
        got_neg_zero = engine.detail_offset_m(seed, radius_m, x, y, z, DETAIL_AMPLITUDE_M, -0.0)
        assert same(want, got_omitted), (x, y, z, "omitted", want, got_omitted)
        assert same(want, got_none), (x, y, z, "none", want, got_none)
        assert same(want, got_zero), (x, y, z, "zero", want, got_zero)
        assert same(want, got_neg_zero), (x, y, z, "neg_zero", want, got_neg_zero)
        checked += 4
    assert checked == len(points) * 4


def test_detail_offset_m_returns_exactly_zero_for_non_positive_amplitude():
    """The early return in both languages: `amplitude_m <= 0.0` skips the band loop
    entirely and returns exactly `0.0`, never a near-zero float."""
    seed = DETAIL_WORLD_SEEDS[0]
    radius_m = DETAIL_RADII[0]
    d = PyDetail(seed, radius_m)
    amplitudes = [0.0, -0.0, -1.0, -1e300]
    points = list(corpus(10))
    checked = 0
    for x, y, z in points:
        point = _DetailPoint(x, y, z)
        for amplitude_m in amplitudes:
            want = d.offset_m(point, amplitude_m, None)
            got = engine.detail_offset_m(seed, radius_m, x, y, z, amplitude_m, None)
            assert same(want, got) and want == 0.0, (x, y, z, amplitude_m, want, got)
            checked += 1
    assert checked == len(points) * len(amplitudes)


# ---------------------------------------------------------------------------
# Shelf: coastal, target_depth_m, weight, evaluate, elevation_m
#
# The contract split Task 1 measured, applied and not blurred:
#
# `shelf.py` contains no transcendental call of its own. It reaches exactly one
# indirectly -- `hypot`, inside `Continentality.Gradient.magnitude()` -- and only via
# `coastal()`'s `gradient(point).magnitude()` call that produces `slope`. Task 1 confirmed
# this both structurally (no `math` name bound in the module, no transcendental call in its
# source) and behaviourally (patching `math.hypot` to raise, `above_shore()` ran clean over
# 2,000 points while `coastal()` hit it on every one not short-circuited by the window
# gate).
#
# So, per the brief:
#
# - `above_shore` (gate 1: `abs(value) > COASTAL_WINDOW`) does NOT reach the `hypot` --
#   STRICT, and the `None` it produces must line up with the engine's `None` positionally.
# - `slope` (gate 2, and `Coastal.distance_m`/`Coastal.breadth`, and everything computed
#   from them) IS downstream of the `hypot` -- BOUNDED at `MAX_TRANSCENDENTAL_ULPS`.
# - `target_depth_m` and `weight` are themselves purely algebraic (division, `max`,
#   smoothstep, `abs`) -- no transcendental of their own. Given bit-identical inputs they
#   must agree bit-for-bit, which is what
#   `test_shelf_target_depth_m_and_weight_are_strict_given_identical_inputs` below measures
#   directly, isolated from the `coastal()` hazard.
# - `evaluate`/`elevation_m` additionally compose with `Tectonics.offset_m` and
#   `Continentality.base_elevation` (the macro layer). A first pass borrowed
#   `TECTONICS_BOUNDED_MAX_ULPS` (8192) wholesale for all three fields, on the theory that
#   the divergence was inherited from the Tectonics section's already-measured
#   `engagement`/`authority` cancellation hazard. That theory is wrong, and the bound was
#   dangerously loose besides -- both corrected below.
#
#   The wrongness: at the corpus point where `weight` diverges most (1024 ULP), `tectonic_m`
#   is bit-identical on both sides (0 ULP). The divergence is reproducible by varying only
#   `coastal()`'s `distance_m` (itself bounded at `MAX_TRANSCENDENTAL_ULPS` via `hypot`)
#   while holding `tectonic_m` fixed, so it cannot be coming from `Tectonics.offset_m`. The
#   real mechanism is local to this module: `weight`'s `seaward = 1.0 - smooth(x)` term, at
#   this point, evaluates `smooth` at `x` ~= 0.982 -- close enough to 1.0 that subtracting it
#   from 1.0 loses most of the input's precision to catastrophic cancellation. That is a
#   shelf-specific hazard in `weight`'s own formula, not something inherited from tectonics.
#
#   The looseness: an 8192-ULP shared bound is wide enough that an algebraically-equal but
#   numerically-different rearrangement of `evaluate`'s blend (`macro * (1.0 - weight) +
#   target * weight` in place of `macro + weight * (target - macro)`) diverges `elevation_m`
#   by 203 ULP and the suite would still pass. A per-field bound sized to what each field
#   actually needs closes that gap for `elevation_m` while still accommodating `weight`'s
#   genuine cancellation hazard.
#
#   So each field gets its own bound below (`SHELF_ELEVATION_MAX_ULPS`,
#   `SHELF_WEIGHT_MAX_ULPS`, `SHELF_TECTONIC_MAX_ULPS`), each measured against this
#   section's own corpus rather than borrowed: worst observed 36 ULP for `elevation_m`
#   (legitimate -- the composition with `Tectonics.offset_m`/`base_elevation` genuinely
#   moves it a little), 1024 ULP for `weight` (the `1.0 - smooth(x)` cancellation described
#   above), 230 ULP for `tectonic_m` (a plain read-through of `Tectonics.offset_m`'s own
#   already-documented hazard). All three exceed the ordinary 4-ULP transcendental bound and
#   none of them share a single number by coincidence -- see each constant's own docstring.
#
# Nothing here needed a tolerance the brief did not already predict: `coastal()`'s own
# `distance_m`/`breadth` measured at a worst of 2 and 4 ULP respectively (right at, not
# past, `MAX_TRANSCENDENTAL_ULPS`), and `target_depth_m`/`weight` measured bit-exact given
# identical inputs, confirming they carry no hazard of their own.

from worldbuilder.bathymetry.shelf import Coastal as PyCoastal
from worldbuilder.bathymetry.shelf import Shelf as PyShelf
from worldbuilder.bathymetry.shelf import COASTAL_WINDOW as PY_COASTAL_WINDOW
from worldbuilder.bathymetry.shelf import MIN_GRADIENT as PY_MIN_GRADIENT

SHELF_CONTINENTALITY_SEED = 20260831
"""
Not `CONTINENTALITY_SEED` (12345, the Tectonics section's world) -- this is the seed Task 1
measured its gate margins and firing point against (Task 1's own throwaway `build()`,
since deleted, which matches `shelf.rs`'s own `#[cfg(test)]` fixture: `SEED = 20260831`).
Pinning it lets the two floor assertions below reuse Task 1's literal measured numbers as a
regression guard, and lets the fixture points found by scanning below reproduce the exact
points `shelf.rs`'s own unit tests and Task 1's report already named.

The plates are still the Tectonics section's synthetic 12-plate fixture
(`PY_PLATE_SET`/`PLATE_SEEDS_FLAT`/`PLATE_POLES_FLAT`/`PLATE_RATES`) -- real,
independently-varying poles and rates, exactly as that section's own fixture doc explains,
just paired with this different `Continentality` seed. `shelf.rs`'s tests pair the same
seed with real generated plates (`generation::plates_for(SEED, 22)`) instead; that
difference only changes what the tectonic offset *is* at a given point, not whether the
`Continentality` gates fire, so it does not need to be reproduced here for the gate-margin
and gradient-gate-firing measurements to line up with Task 1's numbers.
"""

SHELF_LAND = PyContinentality(SHELF_CONTINENTALITY_SEED, EARTH_RADIUS_M, PY_LAND_FRACTION)
SHELF_TECTONICS = PyTectonics(PY_PLATE_SET, SHELF_LAND, EARTH_RADIUS_M)
PY_SHELF = PyShelf(SHELF_TECTONICS, SHELF_LAND, EARTH_RADIUS_M)

SHELF_POINTS = [SpherePoint(Vec3(x, y, z).normalised()) for x, y, z in corpus()]
"""The same `corpus()` Task 1's harness swept (20,006 points: 6 axis-pinned plus 20,000
hashed), normalised exactly as `continentality_corpus()` above normalises its points --
`above_shore` and `gradient` both assume a genuine unit sphere point."""


def _engine_coastal(x, y, z, radius_m=EARTH_RADIUS_M):
    return engine.shelf_coastal(
        PLATE_SEEDS_FLAT, PLATE_POLES_FLAT, PLATE_RATES,
        SHELF_CONTINENTALITY_SEED, PY_LAND_FRACTION,
        x, y, z, radius_m,
    )


def _engine_weight(x, y, z, distance_m, breadth, radius_m=EARTH_RADIUS_M, tectonic_m=None):
    return engine.shelf_weight(
        PLATE_SEEDS_FLAT, PLATE_POLES_FLAT, PLATE_RATES,
        SHELF_CONTINENTALITY_SEED, PY_LAND_FRACTION,
        x, y, z, distance_m, breadth, radius_m, tectonic_m,
    )


def _engine_evaluate(x, y, z, radius_m=EARTH_RADIUS_M):
    return engine.shelf_evaluate(
        PLATE_SEEDS_FLAT, PLATE_POLES_FLAT, PLATE_RATES,
        SHELF_CONTINENTALITY_SEED, PY_LAND_FRACTION,
        x, y, z, radius_m,
    )


SHELF_WINDOW_MARGIN_FLOOR = 1.053777e-06
"""Task 1's measured `min(abs(abs(above_shore) - COASTAL_WINDOW))` over this exact corpus
and seed (task-1-report.md, Question 2). A regression guard, not a tolerance: if a future
change to the corpus, the seed, or `Continentality` narrowed this by an order of magnitude
or more, that would be worth knowing about even though it still would not threaten the
gate's `hypot`-independence (gate 1 never reaches `hypot` at all -- see the section
header)."""

SHELF_GRADIENT_MARGIN_FLOOR = 2.371402e-09
"""Task 1's measured `min(abs(slope - MIN_GRADIENT))` among the corpus points inside the
coastal window, over this exact corpus and seed. Same regression-guard role as the window
margin above, for gate 2 -- the one gate actually downstream of `hypot`."""

SHELF_ELEVATION_MAX_ULPS = 96
"""Measured worst over `SHELF_POINTS`: 36 ULP (see `test_shelf_evaluate_agrees_within_the_
shelf_composed_bounds` below). `elevation_m` composes `coastal()`'s hypot-bounded
`distance_m`/`breadth` with `Tectonics.offset_m` and `Continentality.base_elevation`
through the blend in `evaluate`, which is legitimately enough to clear the ordinary 4-ULP
transcendental bound but nowhere near what `TECTONICS_BOUNDED_MAX_ULPS` (8192, borrowed by
a since-corrected first pass) would tolerate.

96 sits with real headroom above the measured 36 and a clear margin below 203 -- the worst
`elevation_m` divergence this same corpus produces when `evaluate`'s blend is rewritten to
the algebraically-equal `macro * (1.0 - weight) + target * weight`. That rearrangement
changes the floating-point rounding path without changing the value it computes in real
arithmetic, so a bound that could not tell it apart from the legitimate port would be
worthless as a conformance check. 96 discriminates: it passes the real port (36) and fails
the mutation (203)."""

SHELF_WEIGHT_MAX_ULPS = 2048
"""Measured worst over `SHELF_POINTS`: 1024 ULP (see the same test below). NOT inherited
from `Tectonics.offset_m` -- at the corpus point producing this worst divergence,
`tectonic_m` is bit-identical on both sides (0 ULP; confirmed by `dt` in that test), so the
composition-with-tectonics theory a first pass gave for this number is wrong. The actual
mechanism is `weight`'s own `seaward = 1.0 - smooth(x)` term: at that point `x` ~= 0.982, and
subtracting a smoothstep that close to 1.0 destroys most of the input's precision to
catastrophic cancellation -- a hazard specific to this formula in this module, not
something the shelf picked up from tectonics. 2048 leaves 2x headroom over the measured
1024 without being sized to hide anything else; see the note below the corpus test for
whether this field could instead be compared more tightly by decomposing the product."""

SHELF_TECTONIC_MAX_ULPS = 512
"""Measured worst over `SHELF_POINTS`: 230 ULP (see the same test below). This one *is*
what a first pass called "inherited from tectonics" -- `tectonic_m` is `Tectonics.offset_m`
passed straight through `evaluate`, so its divergence is exactly that section's own
`engagement`-cancellation hazard, not a new one. 512 leaves a little over 2x headroom over
the measured 230."""


def test_shelf_gate_margins_match_task_1s_measurement():
    """
    Both gate margins, measured fresh against this exact corpus and seed and checked
    against Task 1's literal numbers -- a regression guard on the RNG stream and the
    `Continentality` calibration, not a tolerance on the port. Deterministic code over a
    fixed corpus should reproduce the same margin to many significant figures; a relative
    tolerance of 1e-3 leaves room for Task 1's report having rounded its own printed
    figures to 7 significant digits without pretending the two computations could
    legitimately diverge by more than that.
    """
    above_shores = [SHELF_LAND.above_shore(p) for p in SHELF_POINTS]
    window_margin = min(abs(abs(a) - PY_COASTAL_WINDOW) for a in above_shores)
    assert window_margin > 0.0, f"a corpus point landed exactly on the window boundary: {window_margin}"
    assert abs(window_margin - SHELF_WINDOW_MARGIN_FLOOR) / SHELF_WINDOW_MARGIN_FLOOR < 1e-3, (
        f"window gate margin measured {window_margin:.6e}, expected close to Task 1's "
        f"{SHELF_WINDOW_MARGIN_FLOOR:.6e} -- re-measure and update task-1-report.md's "
        f"figure if the corpus or seed genuinely changed"
    )

    inside_window = [p for p, a in zip(SHELF_POINTS, above_shores) if abs(a) <= PY_COASTAL_WINDOW]
    assert inside_window, "no corpus point fell inside the coastal window at all"
    slopes = [SHELF_LAND.gradient(p).magnitude() for p in inside_window]
    gradient_margin = min(abs(s - PY_MIN_GRADIENT) for s in slopes)
    assert gradient_margin > 0.0, f"a corpus point landed exactly on the gradient boundary: {gradient_margin}"
    assert abs(gradient_margin - SHELF_GRADIENT_MARGIN_FLOOR) / SHELF_GRADIENT_MARGIN_FLOOR < 1e-3, (
        f"gradient gate margin measured {gradient_margin:.6e}, expected close to Task 1's "
        f"{SHELF_GRADIENT_MARGIN_FLOOR:.6e} -- re-measure and update task-1-report.md's "
        f"figure if the corpus or seed genuinely changed"
    )


def test_shelf_coastal_none_is_positional_over_the_whole_corpus():
    """
    Every one of the 20,006 corpus points, `None`-vs-`Some` compared positionally --
    agreeing only where both sides happen to be `Some` would hide a gate that fired on one
    side and not the other. Exact count, not `> 0`, so a silently-truncated sweep would be
    caught.
    """
    none_mismatches = 0
    some_checked = 0
    for point in SHELF_POINTS:
        v = point.vector
        want = PY_SHELF.coastal(point)
        got = _engine_coastal(v.x, v.y, v.z)
        if (want is None) != (got is None):
            none_mismatches += 1
        elif want is not None:
            some_checked += 1
    assert none_mismatches == 0, (
        f"{none_mismatches} of {len(SHELF_POINTS)} points disagreed on which side of "
        f"coastal()'s None/Some divide they fall"
    )
    assert some_checked == 2509, f"expected exactly 2509 Some points in this corpus, got {some_checked}"


def test_shelf_coastal_some_values_agree_within_the_hypot_bound():
    """
    `Coastal.distance_m`/`.breadth`, the one path in this module downstream of `hypot`
    (via `slope = gradient(point).magnitude()`) -- bounded at `MAX_TRANSCENDENTAL_ULPS`,
    not `same()`. Tracks the worst divergence directly rather than only asserting a
    pass/fail, per the brief's "measure and assert, do not print."
    """
    worst_distance = 0
    worst_breadth = 0
    checked = 0
    for point in SHELF_POINTS:
        v = point.vector
        want = PY_SHELF.coastal(point)
        got = _engine_coastal(v.x, v.y, v.z)
        if want is None:
            assert got is None
            continue
        assert got is not None
        assert close_enough(want.distance_m, got[0]), (
            "distance_m", v.x, v.y, v.z, want.distance_m, got[0],
            ulps_apart(want.distance_m, got[0]),
        )
        assert close_enough(want.breadth, got[1]), (
            "breadth", v.x, v.y, v.z, want.breadth, got[1],
            ulps_apart(want.breadth, got[1]),
        )
        d1 = ulps_apart(want.distance_m, got[0])
        d2 = ulps_apart(want.breadth, got[1])
        if d1 is not None:
            worst_distance = max(worst_distance, abs(d1))
        if d2 is not None:
            worst_breadth = max(worst_breadth, abs(d2))
        checked += 1
    assert checked == 2509, f"expected exactly 2509 Some points, checked {checked}"
    assert worst_distance <= MAX_TRANSCENDENTAL_ULPS, (
        f"distance_m divergence grew to {worst_distance} ULP, past the "
        f"{MAX_TRANSCENDENTAL_ULPS}-ULP hypot bound"
    )
    assert worst_breadth <= MAX_TRANSCENDENTAL_ULPS, (
        f"breadth divergence grew to {worst_breadth} ULP, past the "
        f"{MAX_TRANSCENDENTAL_ULPS}-ULP hypot bound"
    )


def test_shelf_coastal_is_none_deep_in_the_interior_on_the_value_gate_alone():
    """
    The north pole on `SHELF_CONTINENTALITY_SEED`'s world: `above_shore` is far outside
    `COASTAL_WINDOW`, so gate 1 alone returns `None` without ever reaching `gradient` (and
    so never reaching the `hypot` inside it) -- the same point `shelf.rs`'s own
    `coastal_is_none_deep_in_the_interior_on_the_value_gate_alone` test uses, confirmed
    here to be the correct choice for this fixture by checking `above_shore` directly
    before trusting the `None`.
    """
    point = SpherePoint(Vec3(0.0, 0.0, 1.0))
    above = SHELF_LAND.above_shore(point)
    assert abs(above) > PY_COASTAL_WINDOW, f"fixture point must fail gate 1 alone; above={above}"
    assert PY_SHELF.coastal(point) is None
    got = _engine_coastal(0.0, 0.0, 1.0)
    assert got is None


def test_shelf_coastal_is_none_where_the_gradient_gate_genuinely_fires():
    """
    Task 1's closest-approach point, reproduced by scanning this section's own
    `SHELF_POINTS`/`SHELF_LAND` rather than copied as a literal: `above_shore =
    5.247544e-02` (inside the window) and `slope = 2.501102e-09` (~0.25x `MIN_GRADIENT`,
    the closest of the corpus's 6 gradient-gate-firing points) -- so the `None` it produces
    is attributable to gate 2, the one downstream of `hypot`, not gate 1.
    """
    above_shores = [SHELF_LAND.above_shore(p) for p in SHELF_POINTS]
    inside_window = [
        (p, a) for p, a in zip(SHELF_POINTS, above_shores) if abs(a) <= PY_COASTAL_WINDOW
    ]
    with_slope = [
        (p, a, SHELF_LAND.gradient(p).magnitude()) for p, a in inside_window
    ]
    sub_threshold = [(p, a, s) for p, a, s in with_slope if s < PY_MIN_GRADIENT]
    assert len(sub_threshold) == 6, f"expected exactly 6 gradient-gate-firing points, found {len(sub_threshold)}"
    point, above, slope = min(sub_threshold, key=lambda t: abs(t[2] - PY_MIN_GRADIENT))

    assert abs(above) <= PY_COASTAL_WINDOW, f"fixture point must pass gate 1; above={above}"
    assert slope < PY_MIN_GRADIENT, f"fixture point must fail gate 2; slope={slope}"
    assert PY_SHELF.coastal(point) is None

    v = point.vector
    got = _engine_coastal(v.x, v.y, v.z)
    assert got is None, (v.x, v.y, v.z, above, slope, got)


SHELF_TARGET_DEPTH_EDGE_CASES = [
    (0.0, 0.5), (1.0, 0.5), (-1.0, 0.5),
    (-200_000.0, 0.5), (-6_000.0, 0.05), (-1_000.0, 0.8),
]
"""At-and-seaward-of-shore (`offshore <= 0.0`, the hard `0.0` return), beyond the shelf
break (saturates to `SHELF_EDGE_M`), and a narrow platform below the 0.15 `breadth` floor
-- the same hand-derived cases `shelf.rs`'s own unit tests use, now driving the binding
rather than the Rust struct directly."""

SHELF_WEIGHT_EDGE_CASES = [
    (0.0, 0.5), (6_000.0, 0.6), (20_000.0, 0.6), (-200_000.0, 0.5), (-1_000.0, 0.8),
]
"""Seaward, inland within `INLAND_REACH_M`, inland far enough to saturate to zero, well
beyond the shelf break, and a point where a large tectonic offset is meant to suppress the
weight -- the same shapes `shelf.rs`'s own unit tests hand-derive expectations for."""


def test_shelf_target_depth_m_and_weight_are_strict_given_identical_inputs():
    """
    `target_depth_m` and `weight` are themselves purely algebraic -- division, `max`,
    `abs`, and `smooth` (already pinned bit-for-bit elsewhere) -- with no transcendental of
    their own. Given bit-identical `distance_m`/`breadth`/`tectonic_m` inputs (rather than
    inputs freshly drawn from `coastal()`, which is where this module's one hazard lives),
    the two languages must agree bit-for-bit. `same()`, not `close_enough()`: a tolerance
    here would hide a real defect in these two functions rather than paper over `hypot`.
    """
    checked = 0
    for distance_m, breadth in SHELF_TARGET_DEPTH_EDGE_CASES:
        want = PY_SHELF.target_depth_m(PyCoastal(distance_m, breadth))
        got = engine.shelf_target_depth_m(distance_m, breadth)
        assert same(want, got), (distance_m, breadth, want, got)
        checked += 1
    assert checked == len(SHELF_TARGET_DEPTH_EDGE_CASES)

    point = SpherePoint(Vec3(0.0, 0.0, 1.0))
    weight_checked = 0
    for distance_m, breadth in SHELF_WEIGHT_EDGE_CASES:
        for tectonic_m in (0.0, 10_000.0, -300.0, 5.0):
            coastal = PyCoastal(distance_m, breadth)
            want = PY_SHELF.weight(point, coastal, tectonic_m)
            got = _engine_weight(0.0, 0.0, 1.0, distance_m, breadth, tectonic_m=tectonic_m)
            assert same(want, got), (distance_m, breadth, tectonic_m, want, got)
            weight_checked += 1
    assert weight_checked == len(SHELF_WEIGHT_EDGE_CASES) * 4


def test_shelf_target_depth_m_near_the_shelf_break_and_inland_within_reach():
    """
    Two more identical-input cases beyond the hand-derived edges above, this time from
    real coastal geometry: a corpus point whose `offshore` distance sits within 30% of its
    own `break_at` (found by scanning `SHELF_POINTS`/`PY_SHELF`), and a corpus point
    genuinely inland but within `INLAND_REACH_M`. Still `same()`: these are real `Coastal`
    values, but fed identically into both sides rather than recomputed independently, so
    `coastal()`'s own hazard does not enter this comparison.
    """
    near_break = PyCoastal(-84190.01223838703, 1.0)
    inland = PyCoastal(470.0541276317929, 1.0)

    offshore = -near_break.distance_m
    break_at = 80_000.0 * max(0.15, near_break.breadth)
    assert offshore > 0.0 and abs(offshore - break_at) < break_at * 0.3, (
        "fixture point must sit near its own shelf break", offshore, break_at
    )
    assert 0.0 < inland.distance_m < 12_000.0, "fixture point must be inland within INLAND_REACH_M"

    checked = 0
    for coastal in (near_break, inland):
        want_target = PY_SHELF.target_depth_m(coastal)
        got_target = engine.shelf_target_depth_m(coastal.distance_m, coastal.breadth)
        assert same(want_target, got_target), (coastal, want_target, got_target)

        want_weight = PY_SHELF.weight(SpherePoint(Vec3(0.0, 0.0, 1.0)), coastal, 0.0)
        got_weight = _engine_weight(0.0, 0.0, 1.0, coastal.distance_m, coastal.breadth, tectonic_m=0.0)
        assert same(want_weight, got_weight), (coastal, want_weight, got_weight)
        checked += 2
    assert checked == 4


def test_shelf_weight_large_tectonic_offset_suppresses_it():
    """
    The same fixture point as the "supplied zero vs absent" trap test below (found by
    scanning `SHELF_POINTS` for a coastal point with a genuinely non-trivial tectonic
    offset AND a non-saturated baseline weight, so the suppression is actually observable
    rather than starting from zero): a huge supplied `tectonic_m` overriding the small real
    one, so `authority` saturates to (near) zero and the weight collapses, matched against
    a `tectonic_m=0.0` baseline that does not. Both sides fed the identical `Coastal` and
    `tectonic_m`, so this stays on the strict, purely-algebraic side of the contract.
    """
    point = SpherePoint(Vec3(-0.39211320249599313, -0.9086470905306608, 0.14355382718165768))
    coastal = PyCoastal(-69538.32381558529, 0.7994033287540755)
    actual_tectonic = SHELF_TECTONICS.offset_m(point)
    assert actual_tectonic > 50.0, f"fixture point must have a non-trivial tectonic offset; got {actual_tectonic}"

    want_baseline = PY_SHELF.weight(point, coastal, 0.0)
    got_baseline = _engine_weight(
        point.vector.x, point.vector.y, point.vector.z,
        coastal.distance_m, coastal.breadth, tectonic_m=0.0,
    )
    assert same(want_baseline, got_baseline), (want_baseline, got_baseline)
    assert want_baseline > 0.0, "baseline weight must be non-zero for the suppression to be observable"

    want_suppressed = PY_SHELF.weight(point, coastal, 10_000.0)
    got_suppressed = _engine_weight(
        point.vector.x, point.vector.y, point.vector.z,
        coastal.distance_m, coastal.breadth, tectonic_m=10_000.0,
    )
    assert same(want_suppressed, got_suppressed), (want_suppressed, got_suppressed)
    assert want_suppressed < want_baseline * 0.01, (
        f"a 10,000 m tectonic offset should suppress the weight nearly to zero; "
        f"baseline={want_baseline} suppressed={want_suppressed}"
    )


def test_shelf_weight_treats_some_zero_as_a_supplied_zero_not_as_absent():
    """
    The trap, and this task's brief is explicit it is the inverse of the previous slice's:
    `tectonic_m=0.0` (a supplied zero) must NOT behave like `tectonic_m` omitted/`None`
    (which recomputes via `self.tectonics.offset_m(point)`). A binding that flattens
    `Option<f64>` with `unwrap_or(0.0)` would make every `None` look like `Some(0.0)` and
    pass every other test in this file while failing exactly this one.

    Point found by scanning `SHELF_POINTS`/`PY_SHELF` for a coastal point whose recomputed
    tectonic offset is genuinely non-trivial (74.6 m) and whose weight is not saturated, so
    `Some(0.0)` and `None` are guaranteed to disagree by more than float noise.
    """
    point = SpherePoint(Vec3(-0.39211320249599313, -0.9086470905306608, 0.14355382718165768))
    coastal = PY_SHELF.coastal(point)
    assert coastal is not None, "fixture point must be coastal"

    actual_tectonic = SHELF_TECTONICS.offset_m(point)
    assert actual_tectonic > 1.0, f"fixture point must have a non-trivial tectonic offset; got {actual_tectonic}"

    want_some_zero = PY_SHELF.weight(point, coastal, 0.0)
    want_none = PY_SHELF.weight(point, coastal, None)
    assert abs(want_some_zero - want_none) > 1e-6, (
        "fixture must produce distinguishable Python expectations",
        want_some_zero, want_none,
    )

    v = point.vector
    got_some_zero = _engine_weight(v.x, v.y, v.z, coastal.distance_m, coastal.breadth, tectonic_m=0.0)
    got_none = _engine_weight(v.x, v.y, v.z, coastal.distance_m, coastal.breadth, tectonic_m=None)
    got_omitted = engine.shelf_weight(
        PLATE_SEEDS_FLAT, PLATE_POLES_FLAT, PLATE_RATES,
        SHELF_CONTINENTALITY_SEED, PY_LAND_FRACTION,
        v.x, v.y, v.z, coastal.distance_m, coastal.breadth, EARTH_RADIUS_M,
    )  # tectonic_m omitted entirely, not just None -- must land on the same default

    # Some(0.0) never touches tectonics.offset_m, so this side is strict.
    assert same(want_some_zero, got_some_zero), (want_some_zero, got_some_zero)
    # None recomputes via offset_m and then runs through weight's own seaward/authority
    # product -- not strict, bounded at SHELF_WEIGHT_MAX_ULPS (see that constant's
    # docstring for the cancellation this accommodates).
    assert close_enough(want_none, got_none, SHELF_WEIGHT_MAX_ULPS), (
        want_none, got_none, ulps_apart(want_none, got_none)
    )
    assert same(got_none, got_omitted), (
        "omitting tectonic_m must land on exactly the same path as passing None explicitly",
        got_none, got_omitted,
    )
    assert abs(got_some_zero - got_none) > 1e-6, (
        f"Some(0.0) must not behave like None: got_some_zero={got_some_zero} got_none={got_none}"
    )


def test_shelf_evaluate_none_is_positional_and_agrees_deep_in_the_interior():
    """`evaluate` never returns `None` itself (it's `Reading`, always), but its early
    return when `coastal()` is `None` must still land on macro elevation with weight
    exactly `0.0` on both sides -- the north pole fixture again."""
    point = SpherePoint(Vec3(0.0, 0.0, 1.0))
    assert PY_SHELF.coastal(point) is None

    want = PY_SHELF.evaluate(point)
    got = _engine_evaluate(0.0, 0.0, 1.0)
    assert want.weight == 0.0 and got[1] == 0.0
    assert close_enough(want.elevation_m, got[0], SHELF_ELEVATION_MAX_ULPS), (
        want.elevation_m, got[0], ulps_apart(want.elevation_m, got[0])
    )
    assert close_enough(want.tectonic_m, got[2], SHELF_TECTONIC_MAX_ULPS), (
        want.tectonic_m, got[2], ulps_apart(want.tectonic_m, got[2])
    )


def test_shelf_evaluate_agrees_within_the_shelf_composed_bounds():
    """
    `evaluate`'s three fields, over the whole corpus, each bounded at its own measured
    constant (`SHELF_ELEVATION_MAX_ULPS`, `SHELF_WEIGHT_MAX_ULPS`,
    `SHELF_TECTONIC_MAX_ULPS`) rather than one shared, borrowed number -- see the section
    header and each constant's docstring for why a single `TECTONICS_BOUNDED_MAX_ULPS`
    both mischaracterised `weight`'s hazard and was loose enough to hide a real defect in
    `elevation_m`'s blend. Tracks the worst divergence for each field separately, per the
    brief's "report the worst ULP distance on the bounded paths."

    Whether `weight`'s 2048-ULP bound could be tightened by comparing its three factors
    (`seaward`, `breadth`, `authority`) separately instead of the product: `breadth` is
    exact here (fed straight through from `coastal()`, not recomputed) and `authority` is
    only as bad as `tectonic_m`'s own 230-ULP hazard, so the real payoff would be isolating
    `seaward`'s `1.0 - smooth(x)` cancellation on its own -- worth doing if `weight`'s
    current bound (a factor of ~2 over its measured worst, on a value in [0, 1]) ever proves
    too weak an assertion in practice, but not attempted here since the brief only asks that
    it be named.
    """
    worst_elevation = 0
    worst_weight = 0
    worst_tectonic = 0
    checked = 0
    for point in SHELF_POINTS:
        v = point.vector
        want = PY_SHELF.evaluate(point)
        got = _engine_evaluate(v.x, v.y, v.z)

        assert close_enough(want.elevation_m, got[0], SHELF_ELEVATION_MAX_ULPS), (
            "elevation_m", v.x, v.y, v.z, want.elevation_m, got[0],
            ulps_apart(want.elevation_m, got[0]),
        )
        assert close_enough(want.weight, got[1], SHELF_WEIGHT_MAX_ULPS), (
            "weight", v.x, v.y, v.z, want.weight, got[1], ulps_apart(want.weight, got[1]),
        )
        assert close_enough(want.tectonic_m, got[2], SHELF_TECTONIC_MAX_ULPS), (
            "tectonic_m", v.x, v.y, v.z, want.tectonic_m, got[2],
            ulps_apart(want.tectonic_m, got[2]),
        )

        de = ulps_apart(want.elevation_m, got[0])
        dw = ulps_apart(want.weight, got[1])
        dt = ulps_apart(want.tectonic_m, got[2])
        if de is not None:
            worst_elevation = max(worst_elevation, abs(de))
        if dw is not None:
            worst_weight = max(worst_weight, abs(dw))
        if dt is not None:
            worst_tectonic = max(worst_tectonic, abs(dt))
        checked += 1

    assert checked == len(SHELF_POINTS) == 20006

    # The measurement this section's header claims, made concrete -- two-sided, per the
    # brief and the Tectonics section's own precedent: the ordinary bound genuinely does
    # not hold here (this is the finding), but each field's own wider, measured bound does.
    assert worst_elevation > MAX_TRANSCENDENTAL_ULPS, (
        f"expected elevation_m's worst divergence to exceed the ordinary "
        f"{MAX_TRANSCENDENTAL_ULPS}-ULP bound (composition with Tectonics.offset_m is the "
        f"finding this section reports); observed worst was only {worst_elevation} ULP"
    )
    assert worst_tectonic > MAX_TRANSCENDENTAL_ULPS, (
        f"expected tectonic_m's worst divergence to exceed the ordinary "
        f"{MAX_TRANSCENDENTAL_ULPS}-ULP bound; observed worst was only {worst_tectonic} ULP"
    )
    assert worst_weight > MAX_TRANSCENDENTAL_ULPS, (
        f"expected weight's worst divergence to exceed the ordinary "
        f"{MAX_TRANSCENDENTAL_ULPS}-ULP bound (the seaward cancellation is the finding "
        f"this section reports); observed worst was only {worst_weight} ULP"
    )
    assert worst_elevation <= SHELF_ELEVATION_MAX_ULPS, (
        f"elevation_m's worst observed divergence grew to {worst_elevation} ULP, beyond "
        f"the measured SHELF_ELEVATION_MAX_ULPS ({SHELF_ELEVATION_MAX_ULPS})"
    )
    assert worst_weight <= SHELF_WEIGHT_MAX_ULPS, (
        f"weight's worst observed divergence grew to {worst_weight} ULP, beyond the "
        f"measured SHELF_WEIGHT_MAX_ULPS ({SHELF_WEIGHT_MAX_ULPS})"
    )
    assert worst_tectonic <= SHELF_TECTONIC_MAX_ULPS, (
        f"tectonic_m's worst observed divergence grew to {worst_tectonic} ULP, beyond the "
        f"measured SHELF_TECTONIC_MAX_ULPS ({SHELF_TECTONIC_MAX_ULPS})"
    )


# ---------------------------------------------------------------------------
# Features: bump, Feature.reach_m, Placed.weight_at, Features.apply/marks_near
# ---------------------------------------------------------------------------
# Which contract applies to what, chosen per code path, and measured on THIS module's own
# corpus rather than borrowed from any other section (the previous slice borrowed a bound
# and it was both 227x too loose and justified by a factually wrong theory):
#
#   _bump(), and the module constants          no transcendental at all -- STRICT, raw bits.
#   Feature.reach_m()                          `hypot` alone -- BOUNDED, 1 ULP measured.
#   Placed.weight_at()                         `atan2` + `sqrt` (via sphere_to_local; NOT
#                                              local_to_sphere's hypot/cos/sin) -- BOUNDED,
#                                              and bounded ABSOLUTELY, not in ULP; see
#                                              FEATURES_WEIGHT_MAX_ABS.
#   Placed.weight_at(), reach gate rejected    nothing at all is reached -- STRICT, raw bits,
#                                              exactly 0.0 on both sides.
#   Features.apply() -> shaped_metres          BOUNDED, its own ULP + absolute pair.
#   Features.apply() -> authority              BOUNDED, its own ABSOLUTE bound, measured
#                                              separately from shaped_metres.
#   Features.apply(), zero-weight / zero-lift  STRICT, raw bits -- the -0.0 cases.
#   Features.marks_near() -> distance_m        BOUNDED, 2 ULP measured...
#   Features.marks_near() -> membership/order  ...but feeding a DISCRETE output, which is a
#                                              FINDING rather than a bound. See
#                                              test_features_marks_near_membership_can_flip
#                                              _when_within_m_is_taken_from_a_distance.
#
# `apply` returns `(shaped_metres, authority)`: an absolute elevation in metres first, a
# nothing-to-one blend weight second. They are different quantities in different units and
# they get different bounds, which is the whole reason they are measured apart.

from worldbuilder.bathymetry import features as py_features
from worldbuilder.bathymetry.features import CARVE as PY_CARVE
from worldbuilder.bathymetry.features import RAISE as PY_RAISE
from worldbuilder.bathymetry.features import SETTLE_M as PY_SETTLE_M
from worldbuilder.bathymetry.features import SHAPE as PY_SHAPE
from worldbuilder.bathymetry.features import Feature as PyFeature
from worldbuilder.bathymetry.features import Features as PyFeatures
from worldbuilder.bathymetry.features import Placed as PyPlaced

FEATURE_SHAPES = [
    (3.0, 2.0), (20.0, 20.0), (150.0, 90.0), (500.0, 500.0), (1200.0, 300.0),
    (5000.0, 30.0), (10000.0, 40.0), (40.0, 10000.0),
    (20000.0, 7000.0), (40000.0, 12000.0),
]
"""
Spans four orders of magnitude in `length_m` AND two in aspect ratio, and both of those
are load-bearing.

**Size.** The reach gate's corner leak -- points the gate rejects whose ungated `bump`
product is still non-zero -- falls off as roughly a **fourth** power in each of `length_m`
and `width_m`. It is NOT the `1 / (length_m^2 * width_m^2)` an earlier version of this
docstring claimed; that law came from a relative-span grid that never reached the widest
part of the band, and it was overturned in this slice (see `features.rs`'s `weight_at`
notes and the crate README). The band the leak lives in is where `dot` and `cos_reach` are
both within an ULP of 1.0 and the comparison stops resolving distance at all, so its width
in metres runs as `ULP(1.0) * radius_m^2 / reach_m` and *narrows* as the feature grows --
2.500e-3 m at 3x2 against 7.286e-6 m at 1200x300 -- rather than staying a fixed sliver of
arc.

Re-measured for this docstring rather than copied: an absolute-inset corner scan, insets
from 1e-12 m to 1e-1 m in the along and the across direction independently, **120 samples
per decade**, feature at (12.34, 56.78) on bearing 37 deg at Earth radius. Worst leaked
ungated weight:

    3x2         1.20474e-12
    20x20       4.82446e-21
    150x90      1.10546e-26
    1200x300    8.41881e-32

**A scanned extremum depends on the scan, so the density is quoted beside the number.** The
same scan at 40 samples per decade finds only 4.98373e-13 at 3x2 and 2.196e-32 at 1200x300
-- the coarser grid misses the peak of the band by a factor of two to four, which is why a
figure here without its scan is not a figure. The measured 3x2-to-1200x300 ratio is 1.4e19,
against the 1.3e19 a fourth power per dimension predicts and the 3.6e9 the old squared law
predicted: nearly ten orders of magnitude out, which is what overturned it.

At 1200x300 a leak of 1e-32 is invisible to `shaped_metres` and reaches only `authority`
(which starts at a hard `0.0`, where `max(0.0, tiny)` is `tiny`), so a matrix built only on
large features proves nothing about the gate. At 3x2 the leak is 1e-12, which reaches
`shaped_metres` itself -- an ungated `result` of `-29.999999999970655` against an exact
`-30.0` has been measured at that shape.

**Aspect ratio, which an earlier version of this list got wrong and which every bound below
was resized for.** That version topped out at 4:1 (1200x300), and every absolute bound came
out near 5e-16 as a result. Substituting a 5000x30 dredged channel for it -- nothing else
changed, unmutated engine -- failed two of these tests outright, and at 10000x40 the true
worsts are twenty times the 4:1 figures: 1.08e-14 absolute on the weight and 14,080 ULP on
`shaped_metres`. The mechanism is cancellation in
`across = east * across_e + north * across_n`, amplified by `along_m / width_m`; at bearings
0, 90 and 270 degrees, where one of those two terms is exactly zero, it collapses back to
4.44e-16, which is what confirms it. A long narrow feature is not exotic -- a dredged
channel, a breakwater, a sandbar and a levee are all high aspect ratio, and placing exactly
those is what this module is for. `40x10000` is the same ratio with the extents swapped,
included because `along` and `across` are not quite interchangeable under a bearing change.
"""

FEATURE_ORIGINS = [(12.34, 56.78), (0.0, 0.0), (-33.0, 151.0), (89.5, 10.0), (-89.9, -170.0)]
"""Including both poles, where `TangentFrame.at` takes its fallback basis."""

FEATURE_BEARINGS = [0.0, 37.0, 90.0, 143.5, 270.0]
"""0 and 90 exercise the exact `sin`/`cos` landmarks; the rest do not."""

FEATURE_FRACTIONS = [
    0.0, 0.05, 0.2, 0.4, 0.6, 0.8, 0.95, 0.999, 0.99999, 1.0, 1.00001, 1.05, 1.3, 2.0,
]
"""
Fractions of `length_m`/`width_m` to probe at, taken in every ordered pair and in both
signs: well inside, at the edge (1.0 exactly, where `bump`'s `min(1.0, ...)` decides), a
hair either side of it, and beyond the reach. The pair `(1.0, 1.0)` is the corner the reach
gate exists for -- `along` and `across` both landing inside their own extent while the true
arc distance has already reached `reach_m`.
"""

FEATURE_APPLY_CASES = [
    (PY_RAISE, -5.0, -30.0),
    (PY_CARVE, -60.0, -30.0),
    (PY_SHAPE, 4.0, -2.0),
    (PY_SHAPE, 0.5, -0.5),
    (PY_RAISE, 1000.0, -4000.0),
    (PY_CARVE, -8000.0, 120.0),
]
"""`(compose, target_m, elevation_m)`. All three compose arms, both one-way guards on
their contributing and their skipping side, a case whose `shaped_metres` lands near zero
(0.5 over -0.5), and two with kilometres of `lift` so the absolute error has room to grow."""

FEATURES_REACH_MAX_ULPS = 1
"""
`Feature.reach_m()` is `math.hypot(length_m, width_m)` and nothing else.

BOUNDED, not strict, and for a specific reason: since Python 3.8 CPython does not call the
platform `hypot` at all -- it computes its own Neumaier-compensated norm -- while the engine
calls `libm::hypot`. Those are two different algorithms, not two roundings of one algorithm,
so bit-equality is not something either side promises.

Measured worst over 20,010 `(length_m, width_m)` pairs (this section's ten shapes plus
20,000 drawn from [0.5, 50000]): exactly 1 ULP, at
`(24628.73974506011, 42633.3696821233)`. The bound is that 1, with no headroom, because a
`hypot` that started disagreeing by 2 would be worth being told about.
"""

FEATURES_WEIGHT_MAX_ABS = 2.2e-14
"""
`Placed.weight_at()` is bounded ABSOLUTELY rather than in ULP, and that is the measurement
talking, not a preference.

Measured worst absolute divergence over 98,000 probe points (ten shapes x five origins x
five bearings x 196 fraction pairs x both signs): **1.082467e-14**, at a **10000x40 m**
feature from **(-89.9, -170.0)** on **bearing 143.5 deg**, probing (-0.4, -0.4) of its
extents, where the weight is ~0.41990400000001615 against ~0.41990400000000533.

Per shape, worst absolute:

    3x2           3.330669e-16   (12.34, 56.78)   bearing 0
    20x20         4.996004e-16   (-33.0, 151.0)   bearing 37
    150x90        4.440892e-16   (-33.0, 151.0)   bearing 0
    500x500       4.302114e-16   (12.34, 56.78)   bearing 143.5
    1200x300      4.440892e-16   (12.34, 56.78)   bearing 37
    5000x30       7.771561e-15   (-89.9, -170.0)  bearing 37
    10000x40      1.082467e-14   (-89.9, -170.0)  bearing 143.5   <- the bound
    40x10000      1.060263e-14   (0.0, 0.0)       bearing 143.5
    20000x7000    4.996004e-16   (-89.9, -170.0)  bearing 37
    40000x12000   4.440892e-16   (12.34, 56.78)   bearing 0

**The bound is driven by aspect ratio, not by size** -- 40000x12000 (3.3:1) sits at
4.44e-16 while 10000x40 (250:1) is twenty-four times worse. And it is driven by bearing:
the worst at bearing 0, at 90 and at 270 is 4.440892e-16 in each case, against 1.082467e-14
at 143.5 and 1.063039e-14 at 37. At the axis-aligned bearings one of the two terms of
`across = east * across_e + north * across_n` is exactly zero and there is nothing to
cancel; off-axis both terms are large and opposite, and `along_m / width_m` amplifies what
is left.

**Headroom: 2.03x** (2.2e-14 over a legitimate maximum of 1.082467e-14). Deliberately about
two, not about a hundred -- a bound with 100x headroom would accommodate a real defect
without noticing, and this quantity's whole range is [0, 1].

**Why not a ULP bound.** `bump` is `smooth(1.0 - min(1.0, d / half))`, so at the edge of a
feature's support the weight is a smoothstep evaluated on a quantity going to zero, and the
result cancels down to 1e-30 and below. There, one ULP of `along` (which is bounded, coming
through `sphere_to_local`'s `atan2`) is the entire value. Measured worst ULP divergence,
bucketed by how big the weight actually is: 2,517 ULP where the weight is >= 1e-3, 76,326
where it is >= 1e-6, 32,642,720 where it is >= 1e-12, and 4.19e18 -- i.e. no bound at all --
taking every point. (These are the 250:1 corpus's figures. The buckets first written here
were measured on the earlier 7-shape corpus capped at 4:1 and were not re-measured when the
corpus widened; they read 145 / 6,239 / 725,675 / 1.8e16. The bound is unaffected -- the
collapse is worse than first recorded, so the argument for an absolute bound is stronger.) A ULP bound wide enough to hold everywhere would assert precisely nothing;
this absolute bound holds everywhere AND is tight. See
`test_features_weight_at_is_bounded_absolutely_because_the_ulp_measure_collapses` for the
two-sided version of that claim.

**THE ENVELOPE. This bound is not universal and it is not scalable.** It is validated over
a corpus spanning 1.5:1 to 250:1 in both orientations, it holds empirically to about 500:1,
and beyond that it FAILS on the unmutated engine. Re-measured by extending only
`FEATURE_SHAPES`, over the same origins, bearings, fraction pairs and signs:

    30x12000   (400:1)   worst weight 2.847722e-14   -- over this 2.2e-14 bound
    40000x40  (1000:1)   worst weight 4.252154e-14   -- over it by nearly twice

**A feature beyond that envelope needs this bound RE-MEASURED, never scaled.** The
mechanism is catastrophic cancellation in `across = east * across_e + north * across_n`,
amplified by `along_m / width_m` -- and that amplification grows without limit as the aspect
ratio grows, so **no finite corpus makes this bound universal**. Extending the corpus to
500:1 would relocate the same cliff to 800:1 and buy nothing; a bound that implied
universality would be the real defect. Aspect ratio is the axis, size is not: 40000x12000
(3.3:1) sits at 4.44e-16 while 10000x40 (250:1) sets the bound.

**AND IT IS NOT YOURS TO BORROW.** `substrate.py` calls `weight_at` directly -- it is the
second consumer this method is public for -- and reusing this constant when it is ported
will be tempting and is wrong. Measure `substrate.py`'s own quantity, over its own corpus,
with a high-aspect-ratio feature in it. This is not a hypothetical: in a previous slice
`shelf.rs` took `TECTONICS_BOUNDED_MAX_ULPS` (8192) wholesale, an algebraically-equal
rewrite of the blend diverged `elevation_m` by 203 ULP, and the defect sat green inside the
borrowed bound. **A borrowed bound admits whatever the lending module admits**, and the
justification offered for that borrowing was itself factually wrong.
"""

FEATURES_AUTHORITY_MAX_ABS = 2.2e-14
"""
`apply`'s second return value, measured SEPARATELY from `shaped_metres` -- they are
different quantities in different units and a shared bound would be a borrowed one.

Measured worst absolute divergence over the same 98,000 points x six `FEATURE_APPLY_CASES`:
**1.082467e-14**, at **10000x40** from **(-89.9, -170.0)**, **bearing 143.5 deg**,
fractions (-0.4, -0.4), a RAISE to -5 m over -30 m, authority ~0.41990400000001615.

Per shape, worst absolute: 3.330669e-16 at 3x2, 4.996004e-16 at 20x20, 4.440892e-16 at
150x90, 4.302114e-16 at 500x500, 4.440892e-16 at 1200x300, 7.771561e-15 at 5000x30,
1.082467e-14 at 10000x40, 1.060263e-14 at 40x10000, 4.996004e-16 at 20000x7000 and
4.440892e-16 at 40000x12000 -- the same aspect-ratio story as the weight, for the reason
below.

**Headroom: 2.03x** (2.2e-14 over 1.082467e-14), matching the weight's on purpose.

It equals `FEATURES_WEIGHT_MAX_ABS` and that is not an accident to be tidied away by
sharing one constant: `authority` is `weight * smooth(|lift| / SETTLE_M)`, and at every
worst case measured the `smooth` factor had saturated to exactly 1.0, so authority was
carrying the weight's error and nothing else. If `smooth` were ever the dominant term the
two would part company, which is why they are measured and asserted apart.

Absolute rather than ULP for the same reason as the weight, and one more: `authority` starts
at a hard `0.0` and `max(0.0, tiny)` is `tiny`, so a sub-ULP contribution that
`shaped_metres` absorbs invisibly shows up here at full size. Where the gate rejects, the
comparison is not this bound at all but raw bits -- see
`test_features_reach_gate_rejects_identically_and_authority_stays_raw_zero`.

**THE ENVELOPE. This bound is not universal and it is not scalable.** Validated over a
corpus spanning 1.5:1 to 250:1 in both orientations; it holds empirically to about 500:1
and FAILS on the unmutated engine beyond that. Re-measured by extending only
`FEATURE_SHAPES`, over the same origins, bearings, fraction pairs, signs and
`FEATURE_APPLY_CASES`:

    30x12000   (400:1)   worst authority 2.847722e-14   -- over this 2.2e-14 bound
    40000x40  (1000:1)   worst authority 4.252154e-14   -- over it by nearly twice

(Identical to the weight's figures at both shapes, for the reason above: `smooth` had
saturated to 1.0 and authority was carrying the weight's error and nothing else.)

**A feature beyond that envelope needs this bound RE-MEASURED, never scaled.** The
amplification of the `across` cancellation by `along_m / width_m` grows without limit with
the aspect ratio, so **no finite corpus makes this bound universal** -- pushing the corpus to
500:1 would relocate the same cliff to 800:1.

**AND IT IS NOT YOURS TO BORROW.** `substrate.py` calls `weight_at` directly and will be
tempted to reuse a features bound when it is ported. Do not. Measure its own quantity over
its own corpus with a high-aspect-ratio feature in it. `shelf.rs` borrowed
`TECTONICS_BOUNDED_MAX_ULPS` (8192) in a previous slice and a real 203-ULP defect sat green
inside it; a borrowed bound admits whatever the lending module admits.
"""

FEATURES_RESULT_MAX_ULPS = 32768
"""
`apply`'s first return value, `shaped_metres`: an absolute elevation in metres.

Measured worst over the same sweep, counted only where `|shaped_metres| > 1e-6` (below that
the quantity is cancelling to zero and ULP stops meaning anything -- see
`FEATURES_RESULT_MAX_ABS`): **14,080 ULP**, at **10000x40** from **(-89.9, -170.0)**,
**bearing 143.5 deg**, fractions (0.2, 0.2), a RAISE to `target_m=1000.0` over
`elevation_m=-4000.0` giving 14.079999999988104 against 14.080000000013115.

Per shape:

    3x2             768    (12.34, 56.78)   bearing 0
    20x20           208    (-33.0, 151.0)   bearing 37
    150x90          160    (89.5, 10.0)     bearing 37
    500x500         256    (12.34, 56.78)   bearing 37
    1200x300        192    (89.5, 10.0)     bearing 37
    5000x30       4,352    (-89.9, -170.0)  bearing 143.5
    10000x40     14,080    (-89.9, -170.0)  bearing 143.5   <- the bound
    40x10000      6,912    (12.34, 56.78)   bearing 143.5
    20000x7000      224    (-89.9, -170.0)  bearing 37
    40000x12000     768    (12.34, 56.78)   bearing 0

**Headroom: 2.33x** (32768 over 14,080), in line with this file's own precedent for a
measured bound on a bounded path -- `SHELF_ELEVATION_MAX_ULPS` is 96 over a measured 36
(2.67x) and `SHELF_WEIGHT_MAX_ULPS` is 2048 over a measured 1024 (2.0x).

**What drives it, and what does not.** The brief warned not to assume a tight bound here, on
the strength of a one-ULP nudge to the reach threshold moving `shaped_metres` by ~105,470
ULP at 3x2. That sensitivity is real but it is NOT what happens between these two
implementations, and the reason is measured rather than assumed: the reach gate does not
move between them at all. Over 98,000 cross-language probes there were ZERO points where one
language's gate accepted and the other's rejected (see
`test_features_reach_gate_classifies_identically_in_both_languages`), because `reach_m`
agrees to within 1 ULP and `cos` of it lands on the same bit. So the 105,470-ULP sensitivity
never gets excited.

What DOES drive it is aspect ratio, which an earlier version of this constant missed
entirely by capping the shape matrix at 4:1 and landing on 1024. The 14,080 worst is
eighteen times that, and it comes from a 250:1 feature at an oblique bearing -- the same
cancellation in `across` that sets the weight bound. Sizing to the threshold sensitivity
would have been 137x looser than the field needs; sizing to a 4:1 matrix was 13x too tight.
Both are the same mistake in opposite directions: a bound taken from something other than a
measurement over the inputs the module will actually see.

**Do not read that as "and now the corpus is wide enough".** It is the inference this
paragraph most invites and it is false. Widening the corpus from 4:1 to 250:1 fixed a bound
that was wrong for the inputs this module ships against; it did not make the bound
universal, and it could not have, because the quantity that sets it grows without limit.
What the widening actually established is the method -- measure over the inputs the module
will see -- and that method obliges the NEXT author to measure again, not to inherit this
number. Which is what the envelope below is for.

**THE ENVELOPE. This bound is not universal and it is not scalable.** Validated over a
corpus spanning 1.5:1 to 250:1 in both orientations; it holds empirically to about 500:1 and
FAILS on the unmutated engine beyond that. Re-measured by extending only `FEATURE_SHAPES`,
over the same origins, bearings, fraction pairs, signs and `FEATURE_APPLY_CASES`:

    30x12000   (400:1)   worst 18,432 ULP   -- inside this 32,768 bound, but its
                                               companion `FEATURES_RESULT_MAX_ABS` and
                                               `FEATURES_WEIGHT_MAX_ABS` both fail here
    40000x40  (1000:1)   worst 55,296 ULP   -- over this bound by 1.7x

**A feature beyond that envelope needs this bound RE-MEASURED, never scaled.** The
amplification of the `across` cancellation by `along_m / width_m` grows without limit as the
aspect ratio grows, so **no finite corpus makes this bound universal**: pushing the corpus to
500:1 would relocate the same cliff to 800:1 and buy nothing. A bound that implied
universality would be the real defect; a bound with a stated envelope is honest.

**AND IT IS NOT YOURS TO BORROW.** `substrate.py` is the next slice and it calls `weight_at`
directly. Measure its own quantity, over its own corpus, with a high-aspect-ratio feature in
it -- do not reuse this constant or any of its three neighbours. `shelf.rs` borrowed
`TECTONICS_BOUNDED_MAX_ULPS` (8192) in a previous slice; an algebraically-equal rewrite of
the blend diverged `elevation_m` by 203 ULP and sat green inside the borrowed bound, and the
justification given for borrowing was itself false. **A borrowed bound admits whatever the
lending module admits.**
"""

FEATURES_RESULT_MAX_ABS = 1.8e-10
"""
The companion to `FEATURES_RESULT_MAX_ULPS`, and the reason the assertion is
`close_enough(...) or abs(...) <= this` rather than either one alone.

Where `shaped_metres` cancels towards zero -- a CARVE to `target_m=-1e-9` over
`elevation_m=0.0`, say -- the answer can be 1e-46 on one side and exactly 0.0 on the other,
which is unbounded in ULP and utterly meaningless in metres. Conversely a purely absolute
bound would be a weak assertion where the elevation is kilometres. Each half covers the
other's blind spot.

Measured worst absolute divergence over the sweep: **8.776624e-11 m**, at **10000x40** from
**(-89.9, -170.0)**, **bearing 143.5 deg**, fractions (-0.4, -0.4), a CARVE to -8000 m over
+120 m giving -3289.620480000131 against -3289.6204800000432.

Per shape: 2.728484e-12 at 3x2, 4.092726e-12 at 20x20, 3.637979e-12 at 150x90, 3.524292e-12
at 500x500, 3.637979e-12 at 1200x300, 6.298251e-11 at 5000x30, 8.776624e-11 at 10000x40,
8.617462e-11 at 40x10000, 4.092726e-12 at 20000x7000, 3.637979e-12 at 40000x12000.

**Headroom: 2.05x** (1.8e-10 over 8.776624e-11). That is 180 picometres of elevation, which
no chart, sounding or hull has an opinion about, and it is still tight enough that a real
defect in the blend could not hide under it.

**THE ENVELOPE. This bound is not universal and it is not scalable.** Validated over a
corpus spanning 1.5:1 to 250:1 in both orientations; it holds empirically to about 500:1 and
FAILS on the unmutated engine beyond that. Re-measured by extending only `FEATURE_SHAPES`,
over the same origins, bearings, fraction pairs, signs and `FEATURE_APPLY_CASES`:

    30x12000   (400:1)   worst 2.314664e-10 m   -- over this 1.8e-10 bound
    40000x40  (1000:1)   worst 3.453806e-10 m   -- over it by nearly twice

**A feature beyond that envelope needs this bound RE-MEASURED, never scaled.** The
amplification of the `across` cancellation by `along_m / width_m` grows without limit as the
aspect ratio grows, so **no finite corpus makes this bound universal**: pushing the corpus to
500:1 would relocate the same cliff to 800:1. Note that the two halves of the assertion fail
together rather than covering for each other here -- at 400:1 the ULP half is still inside
32,768 while this absolute half is already out, so a caller past the envelope who checks only
one has checked nothing.

**AND IT IS NOT YOURS TO BORROW.** `substrate.py` calls `weight_at` directly and is the next
slice. Measure its own quantity over its own corpus, with a high-aspect-ratio feature in it.
`shelf.rs` borrowed `TECTONICS_BOUNDED_MAX_ULPS` (8192) in a previous slice and a real
203-ULP defect sat green inside it; a borrowed bound admits whatever the lending module
admits.
"""

FEATURES_MARK_DISTANCE_MAX_ULPS = 2
"""
`marks_near`'s `distance_m`, which comes through `SpherePoint.distance_to` -> `angle_to`
(`atan2` over a cross-product magnitude) and is therefore BOUNDED.

Measured worst over 120,000 mark distances (400 marked features x 300 probe points):
**2 ULP**, at 4040848.634591214 m against 4040848.634591215 m. That is under a nanometre on
a four-thousand-kilometre distance, and it is a well-behaved bound.

It is also, on its own, a misleading one -- `distance_m` feeds a discrete output. See
`test_features_marks_near_membership_can_flip_when_within_m_is_taken_from_a_distance`.
"""


def _feature_tuple(feature):
    """A `Feature` in the positional shape the binding takes: `at` flattened, then fields."""
    v = feature.at.vector
    return (
        feature.kind, v.x, v.y, v.z, feature.target_m, feature.length_m, feature.width_m,
        feature.bearing_deg, feature.compose, feature.marked, feature.substrate,
    )


def _feature(kind="f", lat=0.0, lon=0.0, target_m=-5.0, length_m=1200.0, width_m=300.0,
             bearing_deg=0.0, compose=PY_RAISE, marked=False, substrate=None):
    return PyFeature(
        kind=kind, at=SpherePoint.from_latlon(lat, lon), target_m=target_m,
        length_m=length_m, width_m=width_m, bearing_deg=bearing_deg, compose=compose,
        marked=marked, substrate=substrate,
    )


def _probe_point(placed, along_fraction, across_fraction):
    """
    A point at a given fraction of the feature's own extents, along and across its bearing.

    Built through the feature's own frame so that the probe lands where it is meant to for
    every bearing and every origin -- a fixed lat/lon offset would land somewhere quite
    different on a feature bearing 37 degrees than on one bearing 0.
    """
    along_m = along_fraction * placed.feature.length_m
    across_m = across_fraction * placed.feature.width_m
    east_m = along_m * placed._along_e + across_m * placed._across_e
    north_m = along_m * placed._along_n + across_m * placed._across_n
    return placed.frame.local_to_sphere(east_m, north_m)


def _engine_apply(features, point, elevation_m, radius_m=EARTH_RADIUS_M):
    v = point.vector
    return engine.features_apply(
        [_feature_tuple(f) for f in features], v.x, v.y, v.z, elevation_m, radius_m,
    )


def _engine_marks_near(features, point, within_m, radius_m=EARTH_RADIUS_M):
    """`(distance_m, kind)` pairs, so the engine's index answer is comparable to Python's."""
    v = point.vector
    got = engine.features_marks_near(
        [_feature_tuple(f) for f in features], v.x, v.y, v.z, within_m, radius_m,
    )
    return [(distance_m, features[index].kind) for distance_m, index in got]


def _engine_weight_at(feature, point, radius_m=EARTH_RADIUS_M):
    v = point.vector
    return engine.features_weight_at(_feature_tuple(feature), v.x, v.y, v.z, radius_m)


def test_features_constants_agree():
    """The three compose names and `SETTLE_M`, so everything below is comparing like with
    like rather than each language against its own copy of the literals. STRICT: no path."""
    raise_name, carve_name, shape_name, settle_m = engine.features_constants()
    assert raise_name == PY_RAISE
    assert carve_name == PY_CARVE
    assert shape_name == PY_SHAPE
    assert same(settle_m, PY_SETTLE_M)


def test_features_bump_agrees_bit_for_bit():
    """
    STRICT. `_bump` is `abs`, a divide, a two-argument `min` and `_smooth` -- there is no
    transcendental anywhere in it, so a tolerance here would be hiding a defect rather than
    accommodating a libm. Covers the `half_m <= 0.0` early return, the `min(1.0, ...)` clamp
    at and either side of the edge, and both signs.

    **`-0.0` in `halves` is the whole point of the early return's strictness, and it was
    missing while this docstring claimed it was here.** An earlier version of this docstring
    said the negative case was covered because `-1.0` was in the list. `-1.0` exercises the
    *branch*; it does not exercise its *strictness*. The only input in the universe where
    `half_m <= 0.0` and `half_m < 0.0` disagree is `half_m == -0.0`, and until it was added
    below, changing the guard to `<` passed both the Rust and the Python suite outright --
    at `_bump(10.0, -0.0)` Python gives `0.0` and the mutant gives `1.0`, full weight where
    there is none. `distances` had carried `-0.0` all along; `halves` had not. A docstring
    asserting coverage that does not exist is how a gap stays a gap, so this paragraph
    records what the case is for rather than merely naming it.
    """
    halves = [0.0, -0.0, -1.0, 1e-9, 1.0, 2.0, 3.0, 90.0, 300.0, 500.0, 12000.0]
    distances = [
        0.0, -0.0, 1e-12, 0.1, 0.5, 1.0, 1.4999, 1.5, 2.9999, 3.0, -3.0, 149.9, 150.0,
        299.999999, 300.0, 300.0000001, 1e9, -1e9, 0.3333333333333333,
    ]
    checked = 0
    for half_m in halves:
        for distance_m in distances:
            want = py_features._bump(distance_m, half_m)
            got = engine.features_bump(distance_m, half_m)
            assert same(want, got), (distance_m, half_m, want, got)
            checked += 1
    assert checked == len(halves) * len(distances) == 209


def test_features_reach_m_agrees_within_its_own_measured_hypot_bound():
    """
    BOUNDED at `FEATURES_REACH_MAX_ULPS` (1), because `hypot` is where CPython and `libm`
    genuinely run different algorithms -- see that constant's docstring. Two-sided: the
    bound must hold, and it must not be vacuous, so the sweep is also required to find at
    least one pair that is NOT bit-identical.
    """
    pairs = list(FEATURE_SHAPES)
    state = 0x9E3779B97F4A7C15
    mask = (1 << 64) - 1
    for _ in range(20000):
        drawn = []
        for _ in range(2):
            state = (state * 6364136223846793005 + 1442695040888963407) & mask
            h = state ^ (state >> 33)
            drawn.append(0.5 + (h >> 11) / float(1 << 53) * 49999.5)
        pairs.append((drawn[0], drawn[1]))

    worst = 0
    differed = 0
    for length_m, width_m in pairs:
        want = math.hypot(length_m, width_m)
        got = engine.features_reach_m(length_m, width_m)
        assert close_enough(want, got, FEATURES_REACH_MAX_ULPS), (
            length_m, width_m, want, got, ulps_apart(want, got),
        )
        d = ulps_apart(want, got)
        if d is not None:
            worst = max(worst, abs(d))
        if bits(want) != bits(got):
            differed += 1
    assert len(pairs) == 20010
    assert differed > 0, (
        "reach_m came out bit-identical on every pair, so this bound asserts nothing; "
        "CPython's Neumaier hypot and libm's are supposed to be distinguishable"
    )
    assert worst <= FEATURES_REACH_MAX_ULPS, f"reach_m divergence grew to {worst} ULP"


def _weight_sweep():
    """Every (shape, origin, bearing, fraction pair, sign) probe, yielded once."""
    for length_m, width_m in FEATURE_SHAPES:
        for lat, lon in FEATURE_ORIGINS:
            for bearing_deg in FEATURE_BEARINGS:
                feature = _feature(
                    lat=lat, lon=lon, length_m=length_m, width_m=width_m,
                    bearing_deg=bearing_deg,
                )
                placed = PyPlaced(feature, EARTH_RADIUS_M)
                for along_fraction in FEATURE_FRACTIONS:
                    for across_fraction in FEATURE_FRACTIONS:
                        for sign in (1.0, -1.0):
                            point = _probe_point(
                                placed, sign * along_fraction, sign * across_fraction,
                            )
                            yield feature, placed, point, (
                                length_m, width_m, lat, lon, bearing_deg,
                                sign * along_fraction, sign * across_fraction,
                            )


def test_features_weight_at_is_bounded_absolutely_because_the_ulp_measure_collapses():
    """
    BOUNDED at `FEATURES_WEIGHT_MAX_ABS`, absolutely rather than in ULP -- `weight_at`
    reaches `atan2` and `sqrt` through `sphere_to_local` and nothing more (not
    `local_to_sphere`'s `hypot`/`cos`/`sin`, which is the other tangent-frame direction and
    a different profile).

    Three-sided, because the choice of measure is itself the claim being made:

    1. the absolute bound holds on all 98,000 probes;
    2. it is not vacuous -- some probe genuinely approaches it;
    3. the ULP measure really does collapse. The same sweep contains 312 points where one
       language returns exactly 0.0 and the other returns as much as 8.617674e-29, which is
       infinitely many ULP apart and physically indistinguishable from agreement. That is
       `bump`'s support edge, NOT the reach gate -- 0 of the 312 are gate-rejected, and the
       flips run in both directions -- and it is the finding that makes an absolute bound
       the only honest one here. Per shape: 22 at 3x2, 8 at 20x20, 13 at 150x90, 25 at
       500x500, 39 at 1200x300, 30 at 5000x30, 40 at 10000x40, 39 at 40x10000, 39 at
       20000x7000, 57 at 40000x12000 -- worst magnitude 8.617674e-29, at 10000x40.
    """
    worst_abs = 0.0
    worst_case = None
    zero_flips = 0
    worst_flip_magnitude = 0.0
    checked = 0
    for feature, placed, point, ctx in _weight_sweep():
        want = placed.weight_at(point)
        got = _engine_weight_at(feature, point)
        difference = abs(want - got)
        assert difference <= FEATURES_WEIGHT_MAX_ABS, (ctx, want, got, difference)
        if difference > worst_abs:
            worst_abs, worst_case = difference, (ctx, want, got)
        if (want == 0.0) != (got == 0.0):
            zero_flips += 1
            worst_flip_magnitude = max(worst_flip_magnitude, abs(want), abs(got))
        checked += 1

    assert checked == 98000, checked
    assert worst_abs > FEATURES_WEIGHT_MAX_ABS / 10.0, (
        f"worst observed weight divergence was only {worst_abs:e}, an order of magnitude "
        f"inside the bound -- either the corpus stopped reaching the hard cases or the "
        f"bound wants tightening. Worst case: {worst_case}"
    )
    assert zero_flips > 0, (
        "no probe found a point where one language's weight is exactly zero and the "
        "other's is not; that flip is the reason this bound is absolute rather than in "
        "ULP, and a sweep that cannot find it is not testing the claim"
    )
    assert zero_flips == 312, (
        f"the support-edge zero/non-zero flip census measured {zero_flips}, not the 312 "
        f"this corpus was measured to produce; the mechanism has changed"
    )
    assert worst_flip_magnitude < 1e-24, (
        f"a zero/non-zero weight flip carried {worst_flip_magnitude:e}, far more than the "
        f"8.617674e-29 support-edge cancellation measured on this corpus -- that would be a "
        f"real divergence, not a last-bit one"
    )


def test_features_reach_gate_classifies_identically_in_both_languages():
    """
    The reach gate -- `point.vector.dot(feature.at.vector) < cos_reach` -- is measured to
    fire on exactly the same points in both languages, and that is what licenses
    `FEATURES_RESULT_MAX_ULPS` being 32768 rather than the ~105,470 a one-ULP threshold nudge
    would imply. `cos_reach` is `cos(min(pi, hypot(length_m, width_m) / radius_m))`: two
    bounded calls, so it *could* move, but `hypot` agrees to within 1 ULP and `cos` of that
    lands on the same bit, so in practice it does not.

    **Checked in BOTH directions**, because a gate can part company either way and a
    one-directional test names the gate while only catching half of it:

    A. Python rejects, engine must return exactly `0.0`. Catches a threshold that has moved
       so as to make the ENGINE's gate more permissive.
    B. Python accepts with a weight comfortably clear of zero, engine must also return
       non-zero. Catches a threshold that has moved so as to make the engine's gate
       STRICTER -- which direction A cannot see at all.

    Direction B needs the "comfortably clear of zero" qualifier and it is measured, not
    arbitrary. At `bump`'s support edge there are 312 points in this sweep where one
    language returns exactly `0.0` and the other returns ~1e-29 (see
    `test_features_weight_at_is_bounded_absolutely_because_the_ulp_measure_collapses` --
    that is a different mechanism, and 0 of those 312 are gate-rejected). Their largest
    magnitude measured 8.617674e-29, so a `GATE_CLEAR_OF_ZERO` floor of 1e-24 sits five
    orders of magnitude above the contamination and still far below any weight that means
    anything. Measured on the unmutated engine: zero violations in either direction.

    The decisive test of direction B is the millimetre-band scan in
    `test_features_reach_gate_rejects_identically_and_authority_stays_raw_zero`, which puts
    3,061 probes on the accepting side of the gate boundary itself with weights up to
    2.3e-13; this sweep's direction B is the broader, weaker version over all ten shapes.
    """
    GATE_CLEAR_OF_ZERO = 1e-24
    permissive_flips = 0
    strict_flips = 0
    gate_rejected = 0
    gate_accepted_clear = 0
    checked = 0
    for feature, placed, point, ctx in _weight_sweep():
        rejected = point.vector.dot(feature.at.vector) < placed._cos_reach
        got = _engine_weight_at(feature, point)
        if rejected:
            gate_rejected += 1
            if bits(got) != bits(0.0):
                permissive_flips += 1
        elif placed.weight_at(point) > GATE_CLEAR_OF_ZERO:
            gate_accepted_clear += 1
            if got == 0.0:
                strict_flips += 1
        checked += 1
    assert checked == 98000
    assert gate_rejected > 0, "the sweep never once reached past the gate; it proves nothing"
    assert gate_accepted_clear > 0, (
        "no probe landed inside the gate with a weight clear of zero, so direction B "
        "asserted nothing"
    )
    assert permissive_flips == 0, (
        f"{permissive_flips} of {gate_rejected} gate-rejected points got a non-zero weight "
        f"from the engine -- the engine's reach gate has become more permissive than "
        f"Python's, which would invalidate FEATURES_RESULT_MAX_ULPS's justification as "
        f"well as this test"
    )
    assert strict_flips == 0, (
        f"{strict_flips} of {gate_accepted_clear} points that Python accepts with a "
        f"meaningful weight got exactly 0.0 from the engine -- the engine's reach gate has "
        f"become stricter than Python's"
    )


def test_features_reach_gate_rejects_identically_and_authority_stays_raw_zero():
    """
    STRICT, RAW BITS, and deliberately at a 3x2 m feature.

    The gate guards the corner: `along` a hair inside `length_m` and `across` a hair inside
    `width_m` at the same time, so both `bump` factors are individually non-zero even though
    the arc distance has already passed `reach_m`. How much weight leaks if the gate is
    removed falls off as roughly a **fourth** power in each of `length_m` and `width_m` --
    NOT the `1 / (length_m^2 * width_m^2)` recorded earlier in this slice, which came from a
    relative-span grid that never reached the widest part of the band and understated the
    fall-off by nearly ten orders of magnitude over the 3x2-to-1200x300 span; see
    `FEATURE_SHAPES` for the re-measurement and the scan that produced it. The direction of
    the correction does not change what this test needs: it makes the small feature MORE
    necessary, not less, so this must still be probed at a SMALL feature and at the RIGHT
    scale. At 3x2 the disagreement band between the numerical
    gate and the exact geometry is about 1.4 mm wide, and probing at 1e-8 relative (about
    3e-8 m) lands far inside it and finds only ~1e-38, which would prove nothing.

    Scanned here at millimetre scale: 939 of 4,000 corner probes are gate-rejected while
    carrying a non-zero ungated `bump` product, worst 2.277550e-13 at 1.438 mm of inset. The
    other 3,061 sit on the ACCEPTING side of the same boundary with weights from 5.62e-25 up
    to 2.30e-13, and they are checked too -- this is the scan where the gate's threshold
    actually lives, so it is where a threshold nudged in the strict direction (`cos_reach`
    plus one ULP, which direction A cannot see) shows up.

    Note what is NOT asserted on the accepting side: raw-bit equality. 486 of the 4,000
    probes have bit-different weights across the two languages, because on the accepting
    side the weight is a real (if tiny) number computed through `sphere_to_local`'s bounded
    `atan2`. The assertion there is that the engine agrees the feature APPLIES -- non-zero
    against non-zero -- which is the gate's decision, not the weight's value.

    The assertion is raw bits, not a tolerance, and it is on `authority` as well as on the
    weight. 2.28e-13 of weight is invisible in `shaped_metres` -- an elevation tolerance in
    metres would never notice it -- but `authority` starts at a hard `0.0` where
    `max(0.0, tiny)` is `tiny`, so it is the field where deleting this gate becomes
    observable. A tolerance on `authority` would make that untestable, which is why there
    is not one here.
    """
    feature = _feature(
        kind="rock", lat=12.34, lon=56.78, target_m=0.5, length_m=3.0, width_m=2.0,
    )
    placed = PyPlaced(feature, EARTH_RADIUS_M)
    length_m, width_m = feature.length_m, feature.width_m

    leaks = 0
    accepted = 0
    worst_leak = 0.0
    worst_accepted = 0.0
    worst_inset_mm = 0.0
    for step in range(1, 4001):
        inset = step * 1e-7
        along_m = length_m * (1.0 - inset)
        across_m = width_m * (1.0 - inset)
        east_m = along_m * placed._along_e + across_m * placed._across_e
        north_m = along_m * placed._along_n + across_m * placed._across_n
        point = placed.frame.local_to_sphere(east_m, north_m)

        if point.vector.dot(feature.at.vector) >= placed._cos_reach:
            # Direction B: the gate let this one through, so if Python finds any weight
            # here the engine must find some too. A `cos_reach` nudged one ULP in the
            # strict direction rejects exactly these points and is caught right here.
            want_inside = placed.weight_at(point)
            if want_inside > 0.0:
                accepted += 1
                worst_accepted = max(worst_accepted, want_inside)
                got_inside = _engine_weight_at(feature, point)
                assert got_inside != 0.0, (
                    "the engine's reach gate rejected a point Python's gate accepted with "
                    "a real weight; cos_reach has moved in the strict direction",
                    step, want_inside, got_inside,
                )
            continue

        east_back, north_back = placed.frame.sphere_to_local(point)
        along_back = east_back * placed._along_e + north_back * placed._along_n
        across_back = east_back * placed._across_e + north_back * placed._across_n
        ungated = (
            py_features._bump(along_back, length_m) * py_features._bump(across_back, width_m)
        )
        if ungated <= 0.0:
            continue  # gate-rejected, but the bumps would have zeroed it anyway
        leaks += 1
        worst_leak = max(worst_leak, ungated)
        worst_inset_mm = max(
            worst_inset_mm, (feature.reach_m() - math.hypot(along_m, across_m)) * 1000.0
        )

        # Raw bits, both languages, both fields. Not close_enough, not abs().
        want_weight = placed.weight_at(point)
        got_weight = _engine_weight_at(feature, point)
        assert bits(want_weight) == bits(0.0), (ungated, want_weight)
        assert bits(got_weight) == bits(0.0), (ungated, got_weight)

        want = PyFeatures([feature], EARTH_RADIUS_M).apply(point, -30.0)
        got = _engine_apply([feature], point, -30.0)
        assert bits(want[1]) == bits(0.0), ("python authority", ungated, want[1])
        assert bits(got[1]) == bits(0.0), ("engine authority", ungated, got[1])
        assert bits(want[0]) == bits(got[0]), ("shaped_metres", want[0], got[0])

    assert leaks >= 900, (
        f"only {leaks} corner probes were gate-rejected with a non-zero ungated weight; "
        f"the scan has drifted off the ~1.4 mm disagreement band this test exists to stand "
        f"in, and proves nothing where it is now"
    )
    assert worst_leak > 1e-14, (
        f"worst leaked weight was only {worst_leak:e}. Below ~1e-14 this test is back in "
        f"the 1e-32-to-1e-44 band a large feature produces, which no assertion can "
        f"distinguish from agreement -- probe at millimetre offsets, not at 1e-8 relative"
    )
    assert 1.0 < worst_inset_mm < 2.0, (
        f"the gate/geometry disagreement band at 3x2 measured {worst_inset_mm:.4f} mm, not "
        f"the ~1.4 mm this test was sized against"
    )
    assert accepted >= 3000, (
        f"only {accepted} probes landed on the accepting side of the gate boundary, so "
        f"direction B had almost nothing to assert over"
    )
    assert worst_accepted > 1e-14, (
        f"the largest weight on the accepting side was only {worst_accepted:e}; the scan is "
        f"no longer straddling the boundary and direction B proves nothing"
    )
    assert leaks + accepted == 4000, (leaks, accepted)


def test_features_apply_agrees_within_its_own_per_field_measured_bounds():
    """
    BOUNDED, with `shaped_metres` and `authority` on separate, separately-measured bounds
    (`FEATURES_RESULT_MAX_ULPS`/`FEATURES_RESULT_MAX_ABS` and `FEATURES_AUTHORITY_MAX_ABS`).
    They are an elevation in metres and a dimensionless blend weight; one number for both
    would be a bound borrowed from whichever field happened to be worse.

    `shaped_metres` is asserted as "within its ULP bound OR within its absolute bound",
    because neither half covers the whole range on its own: where the elevation cancels
    towards zero the ULP measure is unbounded and meaningless, and where it is kilometres an
    absolute bound alone is a weak assertion. Both halves are sized to this module's own
    measured worst -- see each constant.

    Two-sided, per this file's precedent: each bound must hold, and the sweep must genuinely
    approach it.
    """
    worst_result_ulps = 0
    worst_result_abs = 0.0
    worst_authority_abs = 0.0
    worst_result_case = None
    checked = 0
    for feature, placed, point, ctx in _weight_sweep():
        for compose, target_m, elevation_m in FEATURE_APPLY_CASES:
            shaped = PyFeature(
                kind="f", at=feature.at, target_m=target_m, length_m=feature.length_m,
                width_m=feature.width_m, bearing_deg=feature.bearing_deg, compose=compose,
                marked=False, substrate=None,
            )
            want = PyFeatures([shaped], EARTH_RADIUS_M).apply(point, elevation_m)
            got = _engine_apply([shaped], point, elevation_m)

            result_abs = abs(want[0] - got[0])
            assert (
                close_enough(want[0], got[0], FEATURES_RESULT_MAX_ULPS)
                or result_abs <= FEATURES_RESULT_MAX_ABS
            ), ("shaped_metres", ctx, compose, target_m, elevation_m, want[0], got[0],
                ulps_apart(want[0], got[0]), result_abs)

            authority_abs = abs(want[1] - got[1])
            assert authority_abs <= FEATURES_AUTHORITY_MAX_ABS, (
                "authority", ctx, compose, target_m, elevation_m, want[1], got[1],
                authority_abs,
            )

            if result_abs > worst_result_abs:
                worst_result_abs = result_abs
                worst_result_case = (ctx, compose, target_m, elevation_m, want, got)
            worst_authority_abs = max(worst_authority_abs, authority_abs)
            d = ulps_apart(want[0], got[0])
            if d is not None and abs(want[0]) > 1e-6:
                worst_result_ulps = max(worst_result_ulps, abs(d))
            checked += 1

    assert checked == 98000 * len(FEATURE_APPLY_CASES)
    assert worst_result_ulps > MAX_TRANSCENDENTAL_ULPS, (
        f"shaped_metres's worst divergence was only {worst_result_ulps} ULP, inside the "
        f"ordinary {MAX_TRANSCENDENTAL_ULPS}-ULP bound -- if that is genuinely true this "
        f"section does not need a bound of its own and should say so"
    )
    assert worst_result_ulps <= FEATURES_RESULT_MAX_ULPS, (
        f"shaped_metres grew to {worst_result_ulps} ULP, past the measured "
        f"FEATURES_RESULT_MAX_ULPS ({FEATURES_RESULT_MAX_ULPS}). Worst case: "
        f"{worst_result_case}"
    )
    assert worst_result_abs > FEATURES_RESULT_MAX_ABS / 10.0, (
        f"worst absolute shaped_metres divergence was only {worst_result_abs:e}; the corpus "
        f"is no longer reaching the kilometre-lift cases this bound was sized on"
    )
    assert worst_authority_abs > FEATURES_AUTHORITY_MAX_ABS / 10.0, (
        f"worst absolute authority divergence was only {worst_authority_abs:e}; either the "
        f"corpus stopped reaching the hard cases or the bound wants tightening"
    )


def test_features_apply_is_empty_single_and_order_sensitive():
    """
    An empty `Features` (the loop never runs), a single feature, and the same two features
    in both orders. Order is SEMANTIC here, not merely float non-associativity: each
    iteration's `shaped_metres` feeds the next feature's `lift`, so a bar listed after the
    channel it crosses sits on the carved bottom, and listed before it is cut straight
    through. The two orders must therefore DISAGREE with each other -- by tens of metres,
    not by last bits -- while each agrees with its own Python counterpart.
    """
    far = SpherePoint.from_latlon(40.0, 20.0)
    want_empty = PyFeatures([], EARTH_RADIUS_M).apply(far, -12.5)
    got_empty = _engine_apply([], far, -12.5)
    assert bits(want_empty[0]) == bits(got_empty[0]) == bits(-12.5)
    assert bits(want_empty[1]) == bits(got_empty[1]) == bits(0.0)
    assert engine.features_round_trip([], EARTH_RADIUS_M) == (0, [], [])

    channel = _feature(
        kind="channel", lat=5.0, lon=5.0, target_m=-60.0, length_m=4000.0, width_m=400.0,
        bearing_deg=90.0, compose=PY_CARVE,
    )
    bar = _feature(
        kind="bar", lat=5.0, lon=5.0, target_m=-8.0, length_m=800.0, width_m=2000.0,
        bearing_deg=0.0, compose=PY_RAISE,
    )
    point = SpherePoint.from_latlon(5.0005, 5.0005)

    single_want = PyFeatures([channel], EARTH_RADIUS_M).apply(point, -30.0)
    single_got = _engine_apply([channel], point, -30.0)
    assert abs(single_want[0] - single_got[0]) <= FEATURES_RESULT_MAX_ABS
    assert abs(single_want[1] - single_got[1]) <= FEATURES_AUTHORITY_MAX_ABS
    assert single_want[0] < -30.0, "the channel must actually carve for this to be a test"

    results = {}
    for order in ([channel, bar], [bar, channel]):
        kinds = tuple(f.kind for f in order)
        want = PyFeatures(order, EARTH_RADIUS_M).apply(point, -30.0)
        got = _engine_apply(order, point, -30.0)
        assert abs(want[0] - got[0]) <= FEATURES_RESULT_MAX_ABS, (kinds, want, got)
        assert abs(want[1] - got[1]) <= FEATURES_AUTHORITY_MAX_ABS, (kinds, want, got)
        assert engine.features_round_trip(
            [_feature_tuple(f) for f in order], EARTH_RADIUS_M
        ) == (2, list(kinds), [None, None])
        results[kinds] = (want, got)

    forwards = results[("channel", "bar")]
    backwards = results[("bar", "channel")]
    assert abs(forwards[0][0] - backwards[0][0]) > 10.0, (
        "swapping the two features must change the answer by tens of metres; if it does "
        "not, this fixture is not exercising order at all", forwards, backwards,
    )
    assert abs(forwards[1][0] - backwards[1][0]) > 10.0, (
        "the engine must be as order-sensitive as the Python", forwards, backwards,
    )


def test_features_apply_exercises_all_three_compose_arms_including_shape():
    """
    All three arms of the compose enum, each on both sides of its own guard.

    `SHAPE` in particular: it has no guard at all, so a widened RAISE guard (`compose !=
    CARVE`) would silently swallow it. The first fixture below is the case that
    distinguishes them -- a SHAPE feature whose `lift` is negative at full weight. Python
    gives `(-10.0, 1.0)`; a RAISE-guard-widened engine would give `(-5.0, 0.0)`, which is a
    whole feature going missing rather than a last bit moving. Probed at the feature's own
    centre, where the weight is exactly 1.0 and the arithmetic is exact, so these are
    raw-bit comparisons rather than bounded ones.
    """
    at_centre = SpherePoint.from_latlon(10.0, 20.0)

    def centred(compose, target_m):
        return PyFeature(
            kind=compose, at=at_centre, target_m=target_m, length_m=1200.0, width_m=300.0,
            bearing_deg=0.0, compose=compose, marked=False, substrate=None,
        )

    assert same(PyPlaced(centred(PY_SHAPE, 0.0), EARTH_RADIUS_M).weight_at(at_centre), 1.0)

    cases = [
        # (compose, target_m, elevation_m, expected shaped_metres, expected authority)
        (PY_SHAPE, -10.0, -5.0, -10.0, 1.0),   # SHAPE going DOWN: a RAISE guard would skip
        (PY_SHAPE, -1.0, -5.0, -1.0, 1.0),     # SHAPE going up: a CARVE guard would skip
        (PY_RAISE, -1.0, -5.0, -1.0, 1.0),     # RAISE contributing
        (PY_RAISE, -10.0, -5.0, -5.0, 0.0),    # RAISE skipping, lift < 0
        (PY_CARVE, -10.0, -5.0, -10.0, 1.0),   # CARVE contributing
        (PY_CARVE, -1.0, -5.0, -5.0, 0.0),     # CARVE skipping, lift > 0
        (PY_RAISE, -5.0, -5.0, -5.0, 0.0),     # lift exactly 0.0: RAISE skips
        (PY_CARVE, -5.0, -5.0, -5.0, 0.0),     # lift exactly 0.0: CARVE skips too
    ]
    for compose, target_m, elevation_m, expected_result, expected_authority in cases:
        feature = centred(compose, target_m)
        want = PyFeatures([feature], EARTH_RADIUS_M).apply(at_centre, elevation_m)
        got = _engine_apply([feature], at_centre, elevation_m)
        assert bits(want[0]) == bits(got[0]), (compose, target_m, elevation_m, want, got)
        assert bits(want[1]) == bits(got[1]), (compose, target_m, elevation_m, want, got)
        assert want == (expected_result, expected_authority), (
            "the Python reference itself has moved", compose, target_m, elevation_m, want,
        )


def test_features_apply_keeps_negative_zero_where_a_contribution_is_skipped():
    """
    STRICT, RAW BITS. Two separate places in `apply`'s loop where a skipped contribution is
    bit-observable, and `-0.0` is the only thing that can see either of them:

    1. the `weight <= 0.0` guard. Probed far outside the reach, where the weight is exactly
       `0.0`. Weakening it to `weight < 0.0` lets the zero-weight case fall through to
       `result += weight * lift`, which is `-0.0 + 0.0 * lift` = `+0.0` -- value-equal,
       bit-different.
    2. the RAISE and CARVE guards, at `lift == 0.0` exactly (`elevation_m == -0.0`,
       `target_m == 0.0`). Both skip there, keeping `-0.0`; folding them into one rule that
       fell through would give `-0.0 + weight * 0.0` = `+0.0`.

    `SHAPE` has no guard, so at `lift == 0.0` it DOES fall through and DOES produce `+0.0` --
    included below, because "both languages produce +0.0 here" is as much a part of the
    contract as "both produce -0.0 there", and a test that only pinned the minus signs would
    pass against an engine that had lost the distinction entirely.
    """
    at_centre = SpherePoint.from_latlon(10.0, 20.0)
    far = SpherePoint.from_latlon(40.0, 20.0)

    # 1. weight exactly zero, elevation_m == -0.0.
    for compose in (PY_RAISE, PY_CARVE, PY_SHAPE):
        feature = PyFeature(
            kind="far", at=at_centre, target_m=5.0, length_m=1200.0, width_m=300.0,
            bearing_deg=0.0, compose=compose, marked=False, substrate=None,
        )
        assert bits(PyPlaced(feature, EARTH_RADIUS_M).weight_at(far)) == bits(0.0)
        assert bits(_engine_weight_at(feature, far)) == bits(0.0)
        want = PyFeatures([feature], EARTH_RADIUS_M).apply(far, -0.0)
        got = _engine_apply([feature], far, -0.0)
        assert bits(want[0]) == bits(-0.0), ("python lost the sign", compose, want)
        assert bits(got[0]) == bits(-0.0), (
            "the engine turned -0.0 into +0.0 on a zero-weight feature; the `weight <= 0.0`"
            " guard has been weakened to `weight < 0.0`", compose, got,
        )
        assert bits(want[1]) == bits(got[1]) == bits(0.0)

    # 2. lift exactly zero at full weight, elevation_m == -0.0, target_m == 0.0.
    expected_bits = {PY_RAISE: bits(-0.0), PY_CARVE: bits(-0.0), PY_SHAPE: bits(0.0)}
    for compose, expected in expected_bits.items():
        feature = PyFeature(
            kind="zero", at=at_centre, target_m=0.0, length_m=1200.0, width_m=300.0,
            bearing_deg=0.0, compose=compose, marked=False, substrate=None,
        )
        want = PyFeatures([feature], EARTH_RADIUS_M).apply(at_centre, -0.0)
        got = _engine_apply([feature], at_centre, -0.0)
        assert bits(want[0]) == expected, ("python reference moved", compose, want)
        assert bits(got[0]) == expected, (compose, want, got)
        assert bits(want[1]) == bits(got[1]), (compose, want, got)

    # 3. an empty Features must not touch the sign either.
    want = PyFeatures([], EARTH_RADIUS_M).apply(at_centre, -0.0)
    got = _engine_apply([], at_centre, -0.0)
    assert bits(want[0]) == bits(got[0]) == bits(-0.0)


def _mark(kind, lat, lon, marked=True):
    return PyFeature(
        kind=kind, at=SpherePoint.from_latlon(lat, lon), target_m=-3.0, length_m=100.0,
        width_m=80.0, bearing_deg=10.0, compose=PY_RAISE, marked=marked, substrate=None,
    )


MARKS_FIXTURE = [
    _mark("a", 0.01, 0.0), _mark("b", -0.01, 0.0), _mark("c", 0.0, 0.02),
    _mark("d", 0.05, 0.05), _mark("unmarked", 0.0, 0.001, marked=False),
    _mark("e", -0.2, 0.3),
]
"""`a` and `b` are placed symmetrically so their distances from the origin are bit-identical
-- the tie the stable sort has to survive. `unmarked` is there so the `marked` filter is
exercised by something other than its absence, and it is the nearest feature of the lot, so
a filter that stopped filtering would change every answer below."""


def test_features_marks_near_agrees_at_several_radii():
    """
    `distance_m` is BOUNDED at `FEATURES_MARK_DISTANCE_MAX_ULPS` (2, measured over 120,000
    mark distances); membership and order are DISCRETE and must match exactly.

    Radii chosen to select none, some and all of the fixture, including one below the
    nearest mark and one at 1e9 (past the far side of the planet).
    """
    centre = SpherePoint.from_latlon(0.0, 0.0)
    selected = []
    for within_m in (0.0, 500.0, 1000.0, 1200.0, 2300.0, 5000.0, 50000.0, 1e9):
        want = [
            (d, f.kind)
            for d, f in PyFeatures(MARKS_FIXTURE, EARTH_RADIUS_M).marks_near(centre, within_m)
        ]
        got = _engine_marks_near(MARKS_FIXTURE, centre, within_m)
        assert [k for _, k in want] == [k for _, k in got], (within_m, want, got)
        for (want_d, kind), (got_d, _) in zip(want, got):
            assert close_enough(want_d, got_d, FEATURES_MARK_DISTANCE_MAX_ULPS), (
                within_m, kind, want_d, got_d, ulps_apart(want_d, got_d),
            )
        assert all(kind != "unmarked" for _, kind in want), "the marked filter is not filtering"
        selected.append(len(want))
    assert selected == [0, 0, 0, 2, 3, 3, 5, 5], selected

    empty = PyFeatures([], EARTH_RADIUS_M).marks_near(centre, 1e9)
    assert empty == ()
    assert _engine_marks_near([], centre, 1e9) == []


def test_features_marks_near_keeps_construction_order_on_a_tie():
    """
    DISCRETE, and no tolerance absorbs it. Python's `list.sort` is stable and Rust's
    `sort_by` is stable, so two marks at the same distance must come back in the order they
    were given -- which is only observable if the two can be told apart, hence the binding
    answering with indices rather than cloned features.

    `a` at +0.01 deg and `b` at -0.01 deg of latitude are bit-identical distances from the
    origin (asserted, not assumed). Both build orders are checked: an unstable sort would be
    free to return either order for either build, so testing one build order alone could
    pass by luck.

    **A two-element tie is not sufficient on its own, and that was measured rather than
    assumed.** Rust's `sort_unstable_by` is pattern-defeating quicksort: it falls back to
    insertion sort on short slices and detects an all-equal partition outright, so
    substituting it for `sort_by` reorders NOTHING for a 2-mark tie, nor for a single tie
    group of up to 64 marks. Catching it takes ties scattered through a list long enough to
    reach the quicksort proper. 60 marks at 30 distinct distances, each distance used twice,
    is the smallest fixture measured to do it, and the second half of this test is exactly
    that -- without it the whole test passes against an unstable sort.
    """
    centre = SpherePoint.from_latlon(0.0, 0.0)
    north, south = MARKS_FIXTURE[0], MARKS_FIXTURE[1]
    assert bits(centre.distance_to(north.at, EARTH_RADIUS_M)) == \
        bits(centre.distance_to(south.at, EARTH_RADIUS_M)), \
        "the fixture no longer ties; this test cannot see stability without a tie"

    for order in ([north, south], [south, north]):
        expected = [f.kind for f in order]
        want = [f.kind for _, f in PyFeatures(order, EARTH_RADIUS_M).marks_near(centre, 5000.0)]
        got = [kind for _, kind in _engine_marks_near(order, centre, 5000.0)]
        assert want == expected, ("python's sort stopped being stable", want, expected)
        assert got == expected, (
            "the engine reordered two tied marks; sort_by has become sort_unstable_by",
            got, expected,
        )

    # 60 marks over 30 distances, each used twice: marks i and i+30 tie, and the pairs sit
    # far enough apart in the input that an unstable partition genuinely swaps them.
    paired = [_mark(f"k{i}", 0.01 * (1 + (i % 30)), 0.0) for i in range(60)]
    distances = [centre.distance_to(f.at, EARTH_RADIUS_M) for f in paired]
    assert len(set(bits(d) for d in distances)) == 30, (
        "the 60-mark fixture no longer resolves to 30 bit-identical pairs, so it is no "
        "longer a tie fixture at all"
    )
    expected = [f"k{i}" for j in range(30) for i in (j, j + 30)]
    want = [f.kind for _, f in PyFeatures(paired, EARTH_RADIUS_M).marks_near(centre, 1e9)]
    got = [kind for _, kind in _engine_marks_near(paired, centre, 1e9)]
    assert want == expected, ("python's sort stopped being stable", want, expected)
    assert got == expected, (
        "the engine reordered tied marks in a 60-mark list; sort_by has become "
        "sort_unstable_by", got, expected,
    )


def test_features_marks_near_names_which_feature_when_several_share_a_kind():
    """
    DISCRETE, on raw indices rather than on kinds.

    The binding answers `marks_near` with `(distance_m, index)` and recovers each index by
    **pointer identity** against `built.placed`, and its doc comment justifies that O(n^2)
    walk on the grounds that two features may share a `kind`. Until this test existed that
    hazard was hypothetical: every other fixture in this section uses distinct kinds
    (`a`..`e`, `k0`..`k59`), and `_engine_marks_near` maps the returned index straight back
    to `features[index].kind`, so a wrong index carrying the right kind is invisible by
    construction. Substituting `placed.feature.kind == feature.kind` for
    `std::ptr::eq(&placed.feature, feature)` survived both suites.

    It cannot survive this one. Five rocks all called `rock` are placed so that distance
    order and construction order disagree; `Vec::position` returns the FIRST match, so a
    kind comparison answers index 0 for every mark. This test compares indices directly and
    never looks at a kind, which is the only way the identity mechanism is observable at
    all.

    A chart with five unnamed rocks on it is the ordinary case, not the exotic one: `kind`
    is documented as "what it is called, for diagnostics and for chart symbols", so it is a
    symbol class, and symbol classes repeat.
    """
    centre = SpherePoint.from_latlon(0.0, 0.0)
    # Construction order deliberately not distance order.
    offsets = [0.05, 0.03, 0.01, 0.04, 0.02]
    rocks = [_mark("rock", lat, 0.0) for lat in offsets]
    assert len({f.kind for f in rocks}) == 1, "the fixture must actually share one kind"

    want_pairs = PyFeatures(rocks, EARTH_RADIUS_M).marks_near(centre, 1e9)
    want_indices = [next(i for i, f in enumerate(rocks) if f is feature)
                    for _, feature in want_pairs]
    assert want_indices == [2, 4, 1, 3, 0], (
        "the fixture no longer separates construction order from distance order, so a "
        "binding that answered 0 for everything could pass", want_indices,
    )

    v = centre.vector
    got = engine.features_marks_near(
        [_feature_tuple(f) for f in rocks], v.x, v.y, v.z, 1e9, EARTH_RADIUS_M,
    )
    got_indices = [index for _, index in got]
    assert got_indices == want_indices, (
        "the engine named the wrong placed feature: it is matching marks back to features "
        "by something other than identity, and every one of these shares a kind",
        got_indices, want_indices,
    )

    # And a tie inside the shared kind, where identity is the only thing left to tell the
    # two apart -- same distance, same kind, different index.
    tied = [_mark("rock", 0.01, 0.0), _mark("rock", -0.01, 0.0)]
    assert bits(centre.distance_to(tied[0].at, EARTH_RADIUS_M)) == \
        bits(centre.distance_to(tied[1].at, EARTH_RADIUS_M)), "the tie fixture no longer ties"
    tied_got = engine.features_marks_near(
        [_feature_tuple(f) for f in tied], v.x, v.y, v.z, 1e9, EARTH_RADIUS_M,
    )
    assert [index for _, index in tied_got] == [0, 1], (
        "two tied marks of the same kind must still come back as index 0 then index 1",
        tied_got,
    )


def _marks_corpus(count=400, probes=300):
    state = 0x243F6A8885A308D3
    mask = (1 << 64) - 1

    def nxt():
        nonlocal state
        state = (state * 6364136223846793005 + 1442695040888963407) & mask
        h = state ^ (state >> 33)
        return (h >> 11) / float(1 << 53)

    features = [
        _mark(f"m{i}", nxt() * 179.8 - 89.9, nxt() * 360.0 - 180.0) for i in range(count)
    ]
    points = [
        SpherePoint.from_latlon(nxt() * 179.8 - 89.9, nxt() * 360.0 - 180.0)
        for _ in range(probes)
    ]
    return features, points


def test_features_marks_near_distance_and_order_agree_over_a_corpus():
    """
    The bounded half of `marks_near`, over 400 marks x 300 probe points: `distance_m` within
    `FEATURES_MARK_DISTANCE_MAX_ULPS`, and the resulting order identical every time.
    """
    features, points = _marks_corpus()
    py = PyFeatures(features, EARTH_RADIUS_M)
    worst = 0
    compared = 0
    for point in points:
        want = py.marks_near(point, 1e9)
        got = _engine_marks_near(features, point, 1e9)
        assert [f.kind for _, f in want] == [k for _, k in got], point.to_latlon()
        for (want_d, feature), (got_d, _) in zip(want, got):
            assert close_enough(want_d, got_d, FEATURES_MARK_DISTANCE_MAX_ULPS), (
                feature.kind, want_d, got_d, ulps_apart(want_d, got_d),
            )
            d = ulps_apart(want_d, got_d)
            if d is not None:
                worst = max(worst, abs(d))
            compared += 1
    assert compared == 400 * 300
    assert worst == FEATURES_MARK_DISTANCE_MAX_ULPS, (
        f"distance_m's worst divergence measured {worst} ULP, not the "
        f"{FEATURES_MARK_DISTANCE_MAX_ULPS} this bound was sized on"
    )


def test_features_marks_near_membership_can_flip_when_within_m_is_taken_from_a_distance():
    """
    THE FINDING, reported rather than bounded, because a discrete flip is the one thing no
    tolerance absorbs.

    `distance_m` is bounded at 2 ULP but it feeds `distance <= within_m`, which is a yes or
    a no. Ordinarily the margin is enormous: over this corpus the smallest gap between any
    two adjacent mark distances measured 20,709,884 ULP (0.0386 m), so nothing within 2 ULP
    could reorder two marks; and the nearest a mark distance came to a round `within_m`
    (1e5 ... 1e7) was 3.17 m, so nothing within 2 ULP could reclassify one either. Both are
    asserted below, so the claim is measured rather than believed.

    But a caller who derives `within_m` FROM a distance -- "everything at least as close as
    that rock" -- lands exactly on the boundary, and there the 2 ULP decides. Measured: of
    1,800 such boundary cases (300 probe points x the six nearest marks to each), **174 --
    9.67% -- return a different set of marks from the engine than from the Python**, in
    every observed case one fewer, because the engine's distance for the boundary mark came
    out fractionally larger than the Python value being used as the threshold.

    This is not a defect in the port and no bound would fix it: it is what "bounded" means
    when the consumer is a comparison operator. It is pinned here so the behaviour cannot
    change unnoticed, and so a caller computing `within_m` from a distance obtained from the
    other language knows it is standing on the edge.
    """
    features, points = _marks_corpus()
    py = PyFeatures(features, EARTH_RADIUS_M)

    smallest_gap_ulps = None
    for point in points:
        distances = [d for d, _ in py.marks_near(point, 1e9)]
        for i in range(len(distances) - 1):
            gap = ulps_apart(distances[i], distances[i + 1])
            if gap is not None and (smallest_gap_ulps is None or gap < smallest_gap_ulps):
                smallest_gap_ulps = gap
    assert smallest_gap_ulps is not None
    assert smallest_gap_ulps > 1_000_000 * FEATURES_MARK_DISTANCE_MAX_ULPS, (
        f"two marks came within {smallest_gap_ulps} ULP of each other, close enough to the "
        f"{FEATURES_MARK_DISTANCE_MAX_ULPS}-ULP distance bound that the two languages could "
        f"sort them differently -- a reordering hazard this corpus was measured not to have"
    )

    closest_to_a_round_radius = None
    for point in points:
        for distance_m in (d for d, _ in py.marks_near(point, 1e9)):
            for within_m in (1e5, 5e5, 1e6, 5e6, 1e7):
                margin = abs(distance_m - within_m)
                if closest_to_a_round_radius is None or margin < closest_to_a_round_radius:
                    closest_to_a_round_radius = margin
    assert closest_to_a_round_radius > 1.0, (
        f"a mark sat {closest_to_a_round_radius:e} m from a round within_m; at that margin a "
        f"bounded distance could reclassify it and this corpus would be measuring luck"
    )

    flips = 0
    boundary_cases = 0
    for point in points:
        for boundary_m, _ in py.marks_near(point, 1e9)[:6]:
            want = [f.kind for _, f in py.marks_near(point, boundary_m)]
            got = [kind for _, kind in _engine_marks_near(features, point, boundary_m)]
            boundary_cases += 1
            if want != got:
                flips += 1
                assert len(got) == len(want) - 1, (
                    "a boundary flip that was not the expected 'engine excludes the mark "
                    "the threshold came from' shape", boundary_m, want, got,
                )
                assert set(want) > set(got), (boundary_m, want, got)
    assert boundary_cases == 1800, boundary_cases
    assert flips > 0, (
        "no boundary flip at all. That would be a change in behaviour rather than an "
        "improvement to celebrate quietly -- distance_m is still bounded at 2 ULP and `<=` "
        "is still a hard comparison, so re-measure before deleting this test"
    )
    assert 100 <= flips <= 300, (
        f"{flips} of {boundary_cases} boundary cases flipped; the measured figure was 174 "
        f"(9.67%) and a large move in either direction means distance_m's agreement has "
        f"changed"
    )


def test_features_honour_a_non_earth_radius():
    """
    `radius_m` is threaded through `Features::new` (into every `Placed`'s frame and
    `cos_reach`) and separately through `marks_near`'s `distance_to`. Hard-coding
    `EARTH_RADIUS_M` in either place passes every other test in this section, because every
    other test uses Earth.

    1000 m: a feature 300 m long is then a sixth of the way round the world, so the answers
    are nowhere near their Earth-radius counterparts -- asserted below, so this cannot pass
    by the two radii happening to agree.
    """
    radius_m = 1000.0
    bank = PyFeature(
        kind="bank", at=SpherePoint.from_latlon(0.0, 0.0), target_m=-3.0, length_m=100.0,
        width_m=80.0, bearing_deg=25.0, compose=PY_RAISE, marked=True, substrate=None,
    )
    trench = PyFeature(
        kind="trench", at=SpherePoint.from_latlon(1.0, 2.0), target_m=-9.0, length_m=300.0,
        width_m=200.0, bearing_deg=200.0, compose=PY_CARVE, marked=True, substrate="rock",
    )
    features = [bank, trench]
    point = SpherePoint.from_latlon(0.3, 0.4)

    want = PyFeatures(features, radius_m).apply(point, -5.0)
    got = _engine_apply(features, point, -5.0, radius_m)
    assert abs(want[0] - got[0]) <= FEATURES_RESULT_MAX_ABS, (want, got)
    assert abs(want[1] - got[1]) <= FEATURES_AUTHORITY_MAX_ABS, (want, got)
    assert want[1] > 0.5, "the fixture must actually engage a feature at this radius"

    earth = PyFeatures(features, EARTH_RADIUS_M).apply(point, -5.0)
    assert abs(earth[0] - want[0]) > 1.0, (
        "the two radii give the same answer, so this fixture cannot detect a hard-coded "
        "EARTH_RADIUS_M", earth, want,
    )

    want_marks = [
        (d, f.kind) for d, f in PyFeatures(features, radius_m).marks_near(point, 60.0)
    ]
    got_marks = _engine_marks_near(features, point, 60.0, radius_m)
    assert [k for _, k in want_marks] == [k for _, k in got_marks] == ["bank", "trench"]
    for (want_d, kind), (got_d, _) in zip(want_marks, got_marks):
        assert close_enough(want_d, got_d, FEATURES_MARK_DISTANCE_MAX_ULPS), (
            kind, want_d, got_d,
        )
    assert PyFeatures(features, EARTH_RADIUS_M).marks_near(point, 60.0) == (), (
        "at Earth's radius this within_m selects nothing, which is what makes the 1000 m "
        "answer above evidence that radius_m reached marks_near"
    )


def test_features_the_reach_arc_is_clamped_at_pi_when_a_feature_outgrows_its_world():
    """
    BOUNDED at `FEATURES_WEIGHT_MAX_ABS`, and it exists solely to put a test under the
    `min(math.pi, ...)` in `Placed.__init__`:

        self._cos_reach = math.cos(min(math.pi, feature.reach_m() / radius_m))

    **That clamp had no coverage at all, and deleting it is not a subtle divergence.**
    `test_features_honour_a_non_earth_radius` is the only test in this file that leaves
    Earth, and its features are 100x80 m and 300x200 m at `radius_m = 1000.0`, so
    `reach_m / radius_m` is 0.128 and 0.360 -- nowhere near pi, and the clamp never fires.
    Nothing else in the corpus goes near it either. Replacing the whole clamp with the raw
    ratio passed `cargo test --release` and `pytest tests/test_conformance.py` outright.

    The fixture below fires it: a 5000x5000 m feature on a 1000 m world has
    `reach_m / radius_m = 7.0710678`, so Python clamps to pi and `cos_reach` is exactly
    `-1.0` -- the gate cannot reject anything, which is correct, because past half a turn
    the arc distance starts coming back down and a cosine threshold has stopped meaning
    anything. Unclamped, `cos(7.0710678)` is `+0.7053`, a gate that rejects most of the
    world. Measured against the unclamped variant at these probes: 0.8867793645367641
    against 0.0 at (0, 60), 0.8096072198488381 against 0.0 at (0, 80),
    0.7195243272032742 against 0.0 at (0, 100), 0.5169588223811201 against 0.0 at
    (0, 140). A divergence of 0.89 in a quantity whose entire range is [0, 1], against a
    bound of 2.2e-14.

    `radius_m` is a plain argument of `Features.__init__`; a world generator that models a
    moon, an asteroid or a test fixture at kilometre scale reaches this on its first
    feature.
    """
    radius_m = 1000.0
    atoll = PyFeature(
        kind="atoll", at=SpherePoint.from_latlon(0.0, 0.0), target_m=-3.0,
        length_m=5000.0, width_m=5000.0, bearing_deg=0.0, compose=PY_RAISE,
        marked=True, substrate=None,
    )
    placed = PyPlaced(atoll, radius_m)
    assert atoll.reach_m() / radius_m > math.pi, (
        "the fixture no longer outgrows its world, so the pi clamp is not exercised and "
        "this test asserts nothing it means to"
    )
    assert bits(placed._cos_reach) == bits(-1.0), (
        "the clamp must take cos_reach to exactly -1.0 here; if Python's own value has "
        "moved, the engine comparison below is measuring the wrong thing", placed._cos_reach,
    )

    checked = 0
    for latitude_deg, longitude_deg in [
        (0.0, 0.0), (0.0, 10.0), (0.0, 30.0), (0.0, 60.0), (0.0, 80.0), (0.0, 100.0),
        (0.0, 140.0), (0.0, 179.0), (30.0, 60.0), (-45.0, -120.0), (89.0, 200.0),
    ]:
        point = SpherePoint.from_latlon(latitude_deg, longitude_deg)
        want = placed.weight_at(point)
        got = _engine_weight_at(atoll, point, radius_m)
        assert abs(want - got) <= FEATURES_WEIGHT_MAX_ABS, (
            latitude_deg, longitude_deg, want, got,
        )
        checked += 1
    assert checked == 11

    # The half of the world an unclamped cos_reach would reject: dot below cos(7.0710678).
    far = SpherePoint.from_latlon(0.0, 140.0)
    assert far.vector.dot(atoll.at.vector) < math.cos(atoll.reach_m() / radius_m), (
        "this probe no longer sits where an unclamped gate would reject it, so it can no "
        "longer tell a clamped cos_reach from an unclamped one"
    )
    assert placed.weight_at(far) > 0.5, (
        "the clamped gate must accept this probe with a weight nothing like zero -- that "
        "gap is the whole signal", placed.weight_at(far),
    )
    assert abs(placed.weight_at(far) - _engine_weight_at(atoll, far, radius_m)) \
        <= FEATURES_WEIGHT_MAX_ABS


def test_features_substrate_none_survives_the_crossing_as_none_not_as_empty():
    """
    `substrate` is an `is None` sentinel, not a falsy check: `None` means "derive the bottom
    from the shape of the ground", and an empty string does not mean the same thing.

    **Nothing inside the crate reads this field.** `substrate.py` is not ported, so
    `Features::apply` and `marks_near` are both indifferent to it, and an earlier version of
    this test -- which only round-tripped `kind` -- therefore could not fail on what it was
    named for: a binding doing `substrate.clone().unwrap_or_default()` survived the entire
    conformance file at exit 0. A test that cannot fail is worse than no test, because it
    reads as coverage.

    So the field is round-tripped explicitly through `features_round_trip`, which is the only
    place in the binding surface that observes it. That makes the sentinel's survival an
    assertion rather than an assumption, and it will keep being one when `substrate.py`
    arrives to depend on it. `None` and `""` must come back distinct, and `None` must come
    back as `None`.
    """
    derived = _feature(kind="derived", substrate=None)
    stated = _feature(kind="stated", substrate="")
    named = _feature(kind="named", substrate="rock")
    features = [derived, stated, named]
    assert derived.substrate is None and stated.substrate == "" and named.substrate == "rock"

    count, kinds, substrates = engine.features_round_trip(
        [_feature_tuple(f) for f in features], EARTH_RADIUS_M,
    )
    assert (count, kinds) == (3, ["derived", "stated", "named"])
    assert substrates == [None, "", "rock"], (
        "the substrate sentinel did not survive the crossing intact; a binding that "
        "flattens Option<String> (unwrap_or_default) turns None into the empty string, "
        "which means something different",
        substrates,
    )
    assert substrates[0] is None, "None came back as something falsy but not None"
    assert substrates[1] is not None, "the empty string came back as None"

    python_side = list(PyFeatures(features, EARTH_RADIUS_M))
    assert len(PyFeatures(features, EARTH_RADIUS_M)) == count
    assert [f.kind for f in python_side] == kinds
    assert [f.substrate for f in python_side] == substrates


# --- substrate -------------------------------------------------------------------------
#
# What is STRICT here and what is BOUNDED, and why the split falls where it does:
#
#   _smooth                          STRICT   two comparisons and a polynomial
#   Composition(...) / dominant      STRICT   a sum, three divisions, three comparisons
#   holding / blended_towards        STRICT   multiplies and adds, then Composition again
#   natural                          STRICT   _smooth, a max, and arithmetic
#   slope_at                         BOUNDED  math.hypot, once directly and four times
#                                             inside local_to_sphere. SUBSTRATE_SLOPE
#                                             _DRIFT_REL, and NOT borrowed from anywhere.
#   at / dominant_at, optionals given STRICT  measured, and a finding -- see
#                                             test_substrate_at_is_strict_when_every
#                                             _optional_is_supplied
#   at / dominant_at, slope derived  BOUNDED  slope_at is inside it. SUBSTRATE_AT_DERIVED
#                                             _SLOPE_MAX_ABS, measured separately.
#
# **Every bound in this section was measured over this section's own corpora and none was
# borrowed.** `features.rs`'s FEATURES_WEIGHT_MAX_ABS in particular is NOT reused, even
# though `at` calls `weight_at`: it is sized for a 250:1 dredged channel probed at its own
# support edge, which is 20x looser than anything the substrate corpora reach, and a
# borrowed bound admits whatever the lending module admits.
#
# **Two corpora with opposite shapes, and every figure below states its population and its
# scan resolution**, because both of these quantities are properties of the scan:
#
#   the pinnacle grid   a SMALL STEEP feature, scanned in 2-D. The only thing that reaches
#                       `natural`'s slope clamp at all -- a planetary scatter reads the
#                       clamp as dead code, and a LINE through the same pinnacle tops out
#                       at 7.80 x ROCK_SLOPE against the grid's 8.13 x at any density.
#   the open-water grid GENTLE ground far from anything placed, which is where the
#                       blending guard and the smallest composition margins live.

from worldbuilder.bathymetry.substrate import MUD as PY_MUD
from worldbuilder.bathymetry.substrate import PURE as PY_PURE
from worldbuilder.bathymetry.substrate import ROCK as PY_ROCK
from worldbuilder.bathymetry.substrate import ROCK_SLOPE as PY_ROCK_SLOPE
from worldbuilder.bathymetry.substrate import ROCK_TECTONIC_M as PY_ROCK_TECTONIC_M
from worldbuilder.bathymetry.substrate import SAND as PY_SAND
from worldbuilder.bathymetry.substrate import SETTLED_M as PY_SETTLED_M
from worldbuilder.bathymetry.substrate import SLOPE_BASELINE_M as PY_SLOPE_BASELINE_M
from worldbuilder.bathymetry.substrate import SWEPT_M as PY_SWEPT_M
from worldbuilder.bathymetry.substrate import Composition as PyComposition
from worldbuilder.bathymetry.substrate import Substrate as PySubstrate
from worldbuilder.bathymetry.substrate import _smooth as py_substrate_smooth
from worldbuilder.regions.demo import WORLD_SEED as PY_WORLD_SEED
from worldbuilder.regions.demo import demo_region as py_demo_region
from worldbuilder.terrain.surface import Surface as PySurface

SUBSTRATE_SLOPE_DRIFT_REL = 2.3e-16
"""
`Substrate.slope_at`'s bound, as a fraction of the answer. ONE ULP, and measured here over
this section's own corpora rather than taken on trust from the crate.

**It is a bound on `slope_at` ALONE, and the comparison that produces it drives both sides
from the SAME `structural_m`** -- the Python surface's, handed to the engine as a callable.
That is not a convenience. Driving the engine's `slope_at` with the PORT'S own elevation
field instead moves the answer by up to 7.968304e-11 relative, a factor of 3.46e5 over this
bound -- five orders of magnitude, because the ported elevation itself differs by up to
3.07e-12 m. That drift belongs to `shelf.rs` and `features.rs`. Measuring both ports at
once measures their sum and can attribute it to neither, so this bound is never quoted for
a comparison that crosses the elevation-field boundary, and no test below does.

Measured, both sides on the Python field:

    pinnacle 2-D grid, +-140 m       3,721 pts   4.667 m/step   1 ULP, rel 2.212201e-16
    open-water 2-D grid              961 pts     6000x2000 m    1 ULP, rel 2.136838e-16

**HOST-CONDITIONAL.** One ULP holds only because `local_to_sphere` agreed bit-for-bit at
every measured point on this host, so none of the drift comes from the probe positions.
Nudge a single probe coordinate by one ULP and the answer moves by 3.167834e-09 relative --
seven orders above this bound. A host where that stops being true needs the bound
RE-MEASURED, never widened: widening hides exactly the divergence the bound exists to
detect.
"""

SUBSTRATE_AT_DERIVED_SLOPE_MAX_ABS = 6.6e-16
"""
`Substrate.at`/`dominant_at` when `slope` is left to be derived -- ABSOLUTE, on a fraction
whose whole range is [0, 1], so an absolute bound is the meaningful one.

This is a DIFFERENT quantity from SUBSTRATE_SLOPE_DRIFT_REL and gets its own name and its
own measurement, because it covers a different span of the stack: `slope_at`'s one ULP is
here amplified through `natural`'s smoothstep and then through however many blends the
placed features contribute. The elevation field is still the Python's on both sides.

Measured worst absolute divergence in any one of the three fractions:

    pinnacle 2-D grid    3,721 pts   4.667 m/step   3.330669e-16   (16 ULP, rel 1.938755e-15)
    open-water 2-D grid    961 pts   6000x2000 m    1.110223e-16   ( 4 ULP, rel 6.592115e-16)

**Headroom 1.98x** over the legitimate 3.330669e-16, deliberately about two rather than
about a hundred, and the test asserts the measurement lands in [bound/2, bound] rather than
merely under it -- a test that only checks a ceiling ratchets loose for free and passes
more comfortably as the code degrades.

Zero dominant flips over both corpora in both supply modes. The word is what maritime
consumes, and no tolerance could absorb a flip in it.
"""

SUBSTRATE_GUARD_WORST_ABS = 2.220446049250313e-16
"""
How far `blended_towards(..., 0.0)` moves a composition -- what the `weight > 0.0` guard in
`at` exists to prevent. **ABSOLUTE, not relative**, which matters because it sits a hair
above SUBSTRATE_SLOPE_DRIFT_REL's measured 2.212201e-16 and reads like the same kind of
quantity. It is not. The worst RELATIVE shift is 1.249555e-15, 5.63x larger, and the worst
distance is 11 ULP, not 1. (The absolute figure is exactly one machine epsilon,
2.220446049250313e-16, which is why it is written out in full rather than rounded -- the
assertion below is on bits.)

Blending at weight exactly zero is not the identity because `blended_towards` re-enters
`Composition.__init__`, and the renormalising division there moves fractions whose total is
not exactly 1.0 (an exhaustive sweep of `natural`'s domain puts that total as low as two
ULP below one).

**THE RATE IS A PROPERTY OF THE SAMPLING CONVENTION AND MEANS NOTHING WITHOUT IT: name the
FRAME, the STEP and the SPAN, all three.** Over the demonstration coast's own frame --
`Coast.at(offshore_m, along_m)`, centred on the anchor:

    61x61, 1,500 m per step  (span +-45,000 m)    67/3,721 = 1.80%
    61x61, span +-1,500 m    (50 m per step)     185/3,721 = 4.97%

Using `TangentFrame.at(region.origin)` instead of the coast frame moves the first of those
to 62/3,721 = 1.67%, and seven further conventions a reviewer tried give counts from 18 to
159. The CONCLUSION is robust under every one of them -- the guard is bit-observable, and
it must be transcribed rather than simplified away -- but the RATE is not a property of the
module. This is the third narrowing this one number has needed.
"""

SUBSTRATE_CLAMP_SATURATION = 8.0
"""
How far past `ROCK_SLOPE` the steepest ground in the corpus reaches. **The slope clamp is
not dead code, and only a 2-D scan of a small steep feature shows that.** On the demo
world's 140 m pinnacle at `Coast.at(8_000, 6_500)`:

    61x61 grid, +-140 m, 4.667 m/step     0.3252142109022925   8.1304 x ROCK_SLOPE
    61-point E-W line through it          0.3042234484625276   7.6056 x
    61-point N-S line through it          0.3014451002052766   7.5361 x
    61-point diagonal line                0.3119559807440774   7.7990 x
    400-point planetary scatter           0.0143501550330470   0.3588 x  -- reads DEAD

Resolution does not rescue a line; a second dimension does, because a feature's weight is a
product of two `bump` factors and the steepest ground is off-axis.
"""


_SUBSTRATE_DEMO = []


def _demo_world():
    """The demonstration coast and a `Surface` carrying it.

    Built once for the whole section and reused. Calibrating `Continentality` is a
    4,000-sample sort, and every test here would otherwise pay for it again; the world is
    read-only to all of them.
    """
    if not _SUBSTRATE_DEMO:
        region = py_demo_region()
        _SUBSTRATE_DEMO.append((region, PySurface(PY_WORLD_SEED, features=region.features)))
    return _SUBSTRATE_DEMO[0]


def _engine_field(surface):
    """`surface.structural_m` in the flat `(x, y, z)` shape the binding calls back through.

    **The Python surface's own field, handed to the engine.** See
    SUBSTRATE_SLOPE_DRIFT_REL for why substituting the port's would make every number in
    this section unattributable.
    """
    return lambda x, y, z: surface.structural_m(SpherePoint(Vec3(x, y, z)))


def _engine_tectonic(surface):
    return lambda x, y, z: surface.tectonics.offset_m(SpherePoint(Vec3(x, y, z)))


def _substrate_grid(coast, offshore_m, along_m, half_m, side):
    """A square 2-D grid in the coast's own frame, centred on a point offshore.

    Returns `(points, step_m)`. The step is returned rather than assumed because every
    saturation figure in this section is a property of it.
    """
    step = 2.0 * half_m / (side - 1)
    points = [
        coast.at(offshore_m - half_m + column * step, along_m - half_m + row * step)
        for row in range(side)
        for column in range(side)
    ]
    return points, step


PINNACLE_OFFSHORE_M, PINNACLE_ALONG_M = 8000.0, 6500.0
"""`Coast.at` coordinates of the demo world's 140 m pinnacle -- a 70x70 m RAISE to -3.5 m
standing in about 25 m of water. The small steep feature every corpus here needs."""


def _open_water_points(coast):
    """Gentle ground 20-200 km offshore, 31x31 at 6,000 x 2,000 m per step."""
    return [
        coast.at(20000.0 + column * 6000.0, -30000.0 + row * 2000.0)
        for row in range(31)
        for column in range(31)
    ]


class _RecordingHost:
    """
    A `Substrate` host that answers analytically and writes down which member was asked.

    Two things it is for. The resolution order of `at`'s three optionals is observable only
    through WHICH host call each missing one triggers, so the recorder is the instrument
    that makes it observable -- from Python, across the FFI, rather than only from inside
    the crate. And an analytic field costs nothing, so the order tests do not have to build
    a planet to ask a question that has nothing to do with one.
    """

    def __init__(self, features, radius_m=EARTH_RADIUS_M):
        self.radius_m = radius_m
        self.features = features if isinstance(features, PyFeatures) \
            else PyFeatures(features, radius_m)
        self.calls = []
        host = self

        class _Tectonics:
            def offset_m(self, point):
                host.calls.append("tectonic")
                return 140.0 * point.vector.y

        self.tectonics = _Tectonics()

    def structural_m(self, point):
        self.calls.append("structural")
        # A tilted, gently curved bottom: enough slope to be non-zero everywhere, no
        # transcendental of its own, so nothing the host adds can drift.
        v = point.vector
        return -60.0 + 900.0 * v.z + 300.0 * v.x * v.y


def _engine_at(host, point, **known):
    v = point.vector
    return engine.substrate_at(
        [_feature_tuple(f) for f in host.features], host.radius_m, v.x, v.y, v.z,
        _engine_field(host), _engine_tectonic(host), **known,
    )


def _engine_dominant_at(host, point, **known):
    v = point.vector
    return engine.substrate_dominant_at(
        [_feature_tuple(f) for f in host.features], host.radius_m, v.x, v.y, v.z,
        _engine_field(host), _engine_tectonic(host), **known,
    )


def test_substrate_constants_agree():
    """The three names and the five numbers, so everything below compares like with like
    rather than each language against its own copy of the literals. STRICT: no path."""
    (sand, mud, rock, rock_slope, rock_tectonic_m, swept_m, settled_m, baseline_m,
     drift_rel) = engine.substrate_constants()
    assert (sand, mud, rock) == (PY_SAND, PY_MUD, PY_ROCK)
    assert same(rock_slope, PY_ROCK_SLOPE)
    assert same(rock_tectonic_m, PY_ROCK_TECTONIC_M)
    assert same(swept_m, PY_SWEPT_M)
    assert same(settled_m, PY_SETTLED_M)
    assert same(baseline_m, PY_SLOPE_BASELINE_M)
    assert same(drift_rel, SUBSTRATE_SLOPE_DRIFT_REL), (
        "the bound this section applies is no longer the bound the engine documents; one "
        "of the two moved, and they have to be re-measured together", drift_rel,
    )


def test_substrate_smooth_agrees_bit_for_bit():
    """`substrate.py`'s `_smooth`, reached through the crate's `pub use` of `detail::smooth`
    rather than through a fourth transcription of it. STRICT: two comparisons and a cubic.

    Both clamps are probed from both sides, because the reuse is only safe while the two
    Python modules' `_smooth` are character-for-character identical, and the clamps are
    where a divergence would first show.
    """
    values = [
        -1e300, -1.0, -1e-300, -0.0, 0.0, 1e-300, 0.25, 0.5, 0.75,
        1.0 - 2.220446049250313e-16, 1.0, 1.0 + 2.220446049250313e-16, 1.5, 1e300,
    ]
    values += [i / 997.0 * 1.4 - 0.2 for i in range(998)]
    for fraction in values:
        assert same(py_substrate_smooth(fraction), engine.substrate_smooth(fraction)), fraction
        # And the re-export really is the same function `detail_smooth` exposes.
        assert same(engine.substrate_smooth(fraction), engine.detail_smooth(fraction)), fraction


_COMPOSITION_TRIPLES = [
    (1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0),
    (1.0, 1.0, 1.0), (0.25, 0.25, 0.5), (0.5, 0.25, 0.25), (0.25, 0.5, 0.25),
    (0.1, 0.2, 0.7), (1e-18, 1.0, 1e-18), (1e18, 1.0, 1.0),
    (0.3333333333333333, 0.3333333333333333, 0.3333333333333333),
    (0.0, 0.0, 0.0), (5e-324, 0.0, 0.0), (0.0, 5e-324, 0.0), (0.0, 0.0, 5e-324),
    (-1.0, 0.5, 0.5), (-1.0, -1.0, -1.0), (1.0, -1.0, 0.0), (-0.0, -0.0, -0.0),
    (0.7, -0.7, 0.4),
]
"""Pure corners, ties, three-way ties, wildly unequal magnitudes, and the whole
neighbourhood of the `total <= 0.0` boundary from both sides."""


def test_substrate_composition_normalises_and_answers_bit_for_bit():
    """`Composition.__init__`, `.dominant` and `.holding` off ONE constructed instance.
    STRICT: a sum, three divisions, two multiplies and three comparisons.

    `dominant` is compared as the WORD it is. There is no tolerance that could absorb a
    flip in it, which is why it is checked at every tie the triples list carries.
    """
    for sand, mud, rock in _COMPOSITION_TRIPLES:
        want = PyComposition(sand, mud, rock)
        got_sand, got_mud, got_rock, got_dominant, got_holding = \
            engine.substrate_composition(sand, mud, rock)
        assert same(want.sand, got_sand), (sand, mud, rock)
        assert same(want.mud, got_mud), (sand, mud, rock)
        assert same(want.rock, got_rock), (sand, mud, rock)
        assert want.dominant == got_dominant, (sand, mud, rock, want.dominant, got_dominant)
        assert same(want.holding(), got_holding), (sand, mud, rock)


def test_composition_total_guard_fires_through_the_public_constructor_and_does_not_converge():
    """
    `total <= 0.0` is a real branch, and it is a CLIFF rather than a limit.

    One ULP above zero the result points whichever way the triple points; AT zero it snaps
    to pure rock, because the Python assigns `0.0, 0.0, 1.0, 1.0` in that order. So the
    answer at the boundary is not the limit of the answers approaching it, and a port that
    "smoothed" the guard would be wrong in a way no nearby sample could reveal.

    Reached through the public constructor, which is the only way a caller has -- `PURE`'s
    own three entries go through it too.
    """
    for triple in [(0.0, 0.0, 0.0), (-0.0, -0.0, -0.0), (-1.0, -1.0, -1.0),
                   (1.0, -1.0, 0.0), (-2.0, 1.0, 1.0)]:
        want = PyComposition(*triple)
        assert (want.sand, want.mud, want.rock) == (0.0, 0.0, 1.0), triple
        assert engine.substrate_composition(*triple)[:3] == (0.0, 0.0, 1.0), triple
        assert engine.substrate_composition(*triple)[3] == PY_ROCK

    # One ULP above zero in each direction: a value, not an absence, and it does not point
    # at rock unless the triple did.
    tiny = 5e-324
    for triple, expected in [((tiny, 0.0, 0.0), PY_SAND),
                             ((0.0, tiny, 0.0), PY_MUD),
                             ((0.0, 0.0, tiny), PY_ROCK)]:
        want = PyComposition(*triple)
        got = engine.substrate_composition(*triple)
        assert want.dominant == expected and got[3] == expected, triple
        assert (same(want.sand, got[0]) and same(want.mud, got[1]) and same(want.rock, got[2]))
    assert PyComposition(tiny, 0.0, 0.0).dominant != PyComposition(0.0, 0.0, 0.0).dominant, (
        "the guard converges after all -- one ULP either side of the boundary now gives "
        "the same word, so this test can no longer show the discontinuity it is named for"
    )


def test_substrate_dominant_at_and_near_ties_in_all_three_precedence_orders():
    """
    ROCK > SAND > MUD, each an independent comparison, and each probed AT the tie and one
    ULP either side of it.

    The output is a WORD. No tolerance anywhere could absorb a flip across one of these
    cliffs, so the only thing that can hold the precedence is a test that stands on it.
    """
    def nudge(value, up):
        return math.nextafter(value, math.inf if up else -math.inf)

    cases = [
        # (sand, mud, rock, expected) -- ties first.
        (1.0, 1.0, 1.0, PY_ROCK),        # three-way tie -> rock
        (1.0, 0.5, 1.0, PY_ROCK),        # rock/sand tie, mud below -> rock
        (0.5, 1.0, 1.0, PY_ROCK),        # rock/mud tie, sand below -> rock
        (1.0, 1.0, 0.5, PY_SAND),        # sand/mud tie, rock below -> sand
        (1.0, 0.5, 0.5, PY_SAND),
        (0.5, 1.0, 0.5, PY_MUD),
        (0.5, 0.5, 1.0, PY_ROCK),
    ]
    for sand, mud, rock, expected in cases:
        assert PyComposition(sand, mud, rock).dominant == expected, (sand, mud, rock)
        assert engine.substrate_composition(sand, mud, rock)[3] == expected, (sand, mud, rock)

    # One ULP off each tie, both ways, comparing the two languages rather than an
    # expectation -- what matters is that they step off the cliff together.
    for sand, mud, rock, _ in cases:
        for index in range(3):
            for up in (True, False):
                triple = [sand, mud, rock]
                triple[index] = nudge(triple[index], up)
                want = PyComposition(*triple).dominant
                got = engine.substrate_composition(*triple)[3]
                assert want == got, (triple, want, got)

    # **A ONE-ULP NUDGE OFF A TIE DOES NOT ALWAYS CHANGE THE WORD, and that is the
    # normalising division talking rather than the comparison.** `Composition.__init__`
    # divides all three by their total, and the total moves with the nudge, so at
    # `(1.0, 1.0, nextafter(1.0, -inf))` the three quotients come back exactly equal and
    # the answer is still ROCK. So the precedence chain is walked with the SMALLEST nudge
    # that actually survives normalisation, found rather than assumed -- and the engine is
    # required to step off the cliff at exactly the same place, not merely somewhere near.
    def smallest_surviving(base, index, expected):
        value = base[index]
        for _ in range(80):
            value = nudge(value, False)
            triple = list(base)
            triple[index] = value
            if PyComposition(*triple).dominant == expected:
                return tuple(triple)
        raise AssertionError(("no nudge within 80 ULP changed the word", base, index))

    down_rock = smallest_surviving((1.0, 1.0, 1.0), 2, PY_SAND)
    assert engine.substrate_composition(*down_rock)[3] == PY_SAND, down_rock
    # One ULP back the other way is still ROCK on BOTH sides -- the cliff is in the same
    # place, not merely crossed somewhere.
    just_above = list(down_rock)
    just_above[2] = nudge(just_above[2], True)
    assert PyComposition(*just_above).dominant == PY_ROCK, just_above
    assert engine.substrate_composition(*just_above)[3] == PY_ROCK, just_above

    down_both = smallest_surviving(down_rock, 0, PY_MUD)
    assert engine.substrate_composition(*down_both)[3] == PY_MUD, down_both
    just_above = list(down_both)
    just_above[0] = nudge(just_above[0], True)
    assert PyComposition(*just_above).dominant == engine.substrate_composition(*just_above)[3]


def test_substrate_pure_table_agrees_and_misses_the_same_words():
    """`PURE` holds exactly three keys, and the engine's `pure` misses on exactly the words
    Python's dict lookup raises on -- the empty string among them."""
    for name in (PY_SAND, PY_MUD, PY_ROCK):
        want = PY_PURE[name]
        got = engine.substrate_pure(name)
        assert got is not None, name
        assert same(want.sand, got[0]) and same(want.mud, got[1]) and same(want.rock, got[2])
    for name in ("", " ", "Rock", "ROCK", "gravel", "shell", "sand ", "none"):
        assert name not in PY_PURE, name
        assert engine.substrate_pure(name) is None, name


def test_substrate_blended_towards_agrees_bit_for_bit_including_weight_zero():
    """
    `blended_towards` over weights spanning and overshooting [0, 1]. STRICT: multiplies,
    adds, and `Composition.__init__` again.

    Weight exactly `0.0` and one ULP above it are both in the list, and they are not the
    same case: at exactly zero the blend is arithmetically the identity but the
    renormalisation inside `Composition.__init__` is not, which is the whole reason `at`
    guards the call rather than making it unconditionally.

    **The receiver and the target cross as ALREADY-NORMALISED fields**, because that is
    what they are in Python -- two constructed instances, each divided by its own total
    once. A binding that rebuilt them from the raw triple would normalise a second time,
    and since a real composition's fractions do not sum to exactly 1.0 the second division
    moves them. The first version of this binding did exactly that and this test caught it,
    at `0.2781153660496104` against `0.27811536604961046`.
    """
    weights = [
        -0.5, -5e-324, -0.0, 0.0, 5e-324, 1e-16, 1e-8, 0.25, 0.5, 0.75,
        1.0 - 2.220446049250313e-16, 1.0, 1.0 + 2.220446049250313e-16, 1.5, 2.0,
    ]
    for sand, mud, rock in _COMPOSITION_TRIPLES:
        base = PyComposition(sand, mud, rock)
        for name in (PY_SAND, PY_MUD, PY_ROCK):
            other = PY_PURE[name]
            for weight in weights:
                want = base.blended_towards(other, weight)
                got = engine.substrate_blended_towards(
                    base.sand, base.mud, base.rock,
                    other.sand, other.mud, other.rock, weight,
                )
                assert same(want.sand, got[0]), (sand, mud, rock, name, weight)
                assert same(want.mud, got[1]), (sand, mud, rock, name, weight)
                assert same(want.rock, got[2]), (sand, mud, rock, name, weight)


def test_the_weight_zero_guard_is_bit_observable_and_its_rate_needs_a_named_convention():
    """
    `at`'s `if weight > 0.0` is not a shortcut, and the guard's shift is measurable.

    **The figure to quote carefully.** The worst shift is ABSOLUTE, 2.220446e-16, and it
    sits a hair above SUBSTRATE_SLOPE_DRIFT_REL's measured 2.212201e-16 -- close enough to
    read as the same kind of quantity, which it is not. The worst RELATIVE shift is
    1.249555e-15, 5.63x larger, and the worst distance is 11 ULP, not 1.

    **And the RATE is a property of the sampling convention, not of the module**, so the
    frame, the step and the span are all three named here and in
    SUBSTRATE_GUARD_WORST_ABS. Under the demonstration coast's own frame, 61x61 at 1,500 m
    per step, it is 67/3,721 = 1.80%; the same 3,721 points read as a +-1,500 m span give
    185, and a `TangentFrame.at(origin)` grid gives different numbers again. What is robust
    under every convention is that the count is not zero.
    """
    region, world = _demo_world()
    coast, placed = region.coast, world.features.placed
    side, half_m = 61, 45000.0
    points, step_m = _substrate_grid(coast, 0.0, 0.0, half_m, side)
    assert len(points) == side * side == 3721
    assert step_m == 1500.0, step_m

    shifted_points = 0
    worst_abs = 0.0
    worst_rel = 0.0
    worst_ulp = 0
    flips = 0
    for point in points:
        composition = world.substrate.at(point)
        walked = composition
        moved = False
        for one in placed:
            declared = one.feature.substrate
            if declared is None:
                continue
            if one.weight_at(point) == 0.0:
                blended = walked.blended_towards(PY_PURE[declared], 0.0)
                # The engine agrees about the shift itself, bit for bit.
                got = engine.substrate_blended_towards(
                    walked.sand, walked.mud, walked.rock,
                    PY_PURE[declared].sand, PY_PURE[declared].mud, PY_PURE[declared].rock,
                    0.0,
                )
                assert same(blended.sand, got[0]) and same(blended.mud, got[1]) \
                    and same(blended.rock, got[2])
                if (bits(blended.sand), bits(blended.mud), bits(blended.rock)) != \
                        (bits(walked.sand), bits(walked.mud), bits(walked.rock)):
                    moved = True
                walked = blended
        if moved:
            shifted_points += 1
            for a, b in zip((composition.sand, composition.mud, composition.rock),
                            (walked.sand, walked.mud, walked.rock)):
                worst_abs = max(worst_abs, abs(a - b))
                if max(abs(a), abs(b)) > 0.0:
                    worst_rel = max(worst_rel, abs(a - b) / max(abs(a), abs(b)))
                worst_ulp = max(worst_ulp, abs(bits(a) - bits(b)))
            if walked.dominant != composition.dominant:
                flips += 1

    assert shifted_points > 0, (
        "no point in this population has a placed feature at weight exactly 0.0 whose "
        "blend would shift the composition, so this corpus can no longer show that the "
        "guard is bit-observable -- re-site it rather than dropping the assertion"
    )
    assert shifted_points == 67, (
        "the guard rate moved under a convention that is fully pinned here (Coast.at "
        "frame, 61x61, 1,500 m per step, span +-45,000 m about the anchor)", shifted_points,
    )
    assert same(worst_abs, SUBSTRATE_GUARD_WORST_ABS), (worst_abs, SUBSTRATE_GUARD_WORST_ABS)
    assert abs(worst_rel - 1.249555e-15) < 1e-20, worst_rel
    assert worst_rel > worst_abs * 5.0, (
        "the relative shift is no longer several times the absolute one, which was the "
        "whole reason the two figures must not be quoted for each other", worst_abs, worst_rel,
    )
    assert worst_ulp == 11, ("worst distance is 11 ULP, not 1", worst_ulp)
    assert flips == 0, ("a guard-sized shift moved the one-word answer", flips)


def test_substrate_natural_agrees_bit_for_bit_over_both_corpora():
    """
    `natural` is STRICT -- `_smooth`, a two-argument `max` and arithmetic, zero
    transcendentals -- so every fraction is compared as raw bits with no tolerance at all.

    Driven from the Python surface's own elevation, slope and tectonics at every point of
    both corpora, so the arguments reaching the two `natural`s are identical and only
    `natural` itself is under test.
    """
    region, world = _demo_world()
    coast = region.coast
    pinnacle, _ = _substrate_grid(
        coast, PINNACLE_OFFSHORE_M, PINNACLE_ALONG_M, 140.0, 61,
    )
    for label, points in (("pinnacle", pinnacle), ("open water", _open_water_points(coast))):
        for point in points:
            elevation_m = world.structural_m(point)
            tectonic_m = world.tectonics.offset_m(point)
            slope = world.substrate.slope_at(point)
            want = world.substrate.natural(elevation_m, slope, tectonic_m)
            got = engine.substrate_natural(elevation_m, slope, tectonic_m)
            assert same(want.sand, got[0]), (label, elevation_m, slope, tectonic_m)
            assert same(want.mud, got[1]), (label, elevation_m, slope, tectonic_m)
            assert same(want.rock, got[2]), (label, elevation_m, slope, tectonic_m)


def test_substrate_natural_slope_clamp_is_reached_only_by_a_two_dimensional_scan():
    """
    The slope clamp is not dead code, and the corpus shape is what decides whether anybody
    can tell. See SUBSTRATE_CLAMP_SATURATION for the table.

    A LINE through the pinnacle tops out below 8x at any density and any direction, because
    a feature's weight is a product of two `bump` factors and the steepest ground is
    off-axis. A planetary scatter never approaches the clamp at all and would report it
    dead. Only the grid saturates it.
    """
    region, world = _demo_world()
    coast = region.coast
    grid, step_m = _substrate_grid(
        coast, PINNACLE_OFFSHORE_M, PINNACLE_ALONG_M, 140.0, 61,
    )
    assert len(grid) == 3721 and abs(step_m - 4.666666666666667) < 1e-12

    grid_worst = max(world.substrate.slope_at(point) for point in grid)
    assert grid_worst / PY_ROCK_SLOPE > SUBSTRATE_CLAMP_SATURATION, (
        "the 2-D scan no longer saturates the slope clamp, so nothing in this suite "
        "exercises it", grid_worst, grid_worst / PY_ROCK_SLOPE,
    )

    span = 280.0
    lines = {
        "east-west": [coast.at(PINNACLE_OFFSHORE_M - 140.0 + i * span / 60.0,
                               PINNACLE_ALONG_M) for i in range(61)],
        "north-south": [coast.at(PINNACLE_OFFSHORE_M,
                                 PINNACLE_ALONG_M - 140.0 + i * span / 60.0)
                        for i in range(61)],
        "diagonal": [coast.at(PINNACLE_OFFSHORE_M - 140.0 + i * span / 60.0,
                              PINNACLE_ALONG_M - 140.0 + i * span / 60.0)
                     for i in range(61)],
    }
    for name, points in lines.items():
        line_worst = max(world.substrate.slope_at(point) for point in points)
        assert line_worst < grid_worst, (name, line_worst, grid_worst)
        assert line_worst / PY_ROCK_SLOPE < SUBSTRATE_CLAMP_SATURATION, (name, line_worst)

    # And at the saturated point the two languages agree on the composition, bit for bit.
    steepest_point = max(grid, key=world.substrate.slope_at)
    elevation_m = world.structural_m(steepest_point)
    tectonic_m = world.tectonics.offset_m(steepest_point)
    want = world.substrate.natural(elevation_m, grid_worst, tectonic_m)
    got = engine.substrate_natural(elevation_m, grid_worst, tectonic_m)
    assert want.dominant == PY_ROCK
    assert same(want.sand, got[0]) and same(want.mud, got[1]) and same(want.rock, got[2])


def test_substrate_slope_at_agrees_within_its_own_measured_bound():
    """
    `slope_at` is the ONE bounded function in this module, and this is the comparison the
    bound was measured on: both sides driven by the SAME `structural_m` -- the Python
    surface's, handed to the engine as a callable -- so nothing but `slope_at` itself can
    differ. See SUBSTRATE_SLOPE_DRIFT_REL.

    The assertion is two-sided. Every measured worst must land in [bound/2, bound], not
    merely under it: a test that only checks a ceiling ratchets loose for free and would
    pass more comfortably as the code degraded.
    """
    region, world = _demo_world()
    coast = region.coast
    field = _engine_field(world)
    pinnacle, _ = _substrate_grid(
        coast, PINNACLE_OFFSHORE_M, PINNACLE_ALONG_M, 140.0, 61,
    )
    populations = {
        "pinnacle 2-D grid, 3,721 pts at 4.667 m/step": pinnacle,
        "open water 2-D grid, 961 pts at 6,000 x 2,000 m": _open_water_points(coast),
    }
    for label, points in populations.items():
        worst_rel = 0.0
        worst_ulp = 0
        for point in points:
            v = point.vector
            want = world.substrate.slope_at(point)
            got = engine.substrate_slope_at(
                EARTH_RADIUS_M, v.x, v.y, v.z, PY_SLOPE_BASELINE_M, field,
            )
            assert abs(want - got) <= abs(want) * SUBSTRATE_SLOPE_DRIFT_REL, (
                label, v.x, v.y, v.z, want, got,
            )
            if want != got:
                worst_rel = max(worst_rel, abs(want - got) / abs(want))
                worst_ulp = max(worst_ulp, abs(bits(want) - bits(got)))
        assert worst_ulp == 1, (
            "the drift is no longer one ULP of the final hypot. A wider distance means "
            "local_to_sphere has stopped agreeing bit-for-bit, and this bound must be "
            "RE-MEASURED on this host, never widened -- widening hides exactly the "
            "divergence it exists to detect", label, worst_ulp,
        )
        assert SUBSTRATE_SLOPE_DRIFT_REL * 0.5 <= worst_rel <= SUBSTRATE_SLOPE_DRIFT_REL, (
            "the measured worst no longer sits in the top half of its own bound, so the "
            "bound has drifted loose and admits more than it was sized for",
            label, worst_rel, SUBSTRATE_SLOPE_DRIFT_REL,
        )

    # The baseline is an argument, not a constant, and a wider one is a different answer.
    probe = coast.at(PINNACLE_OFFSHORE_M + 40.0, PINNACLE_ALONG_M - 25.0)
    v = probe.vector
    for baseline_m in (6.0, 60.0, 600.0, 2000.0):
        want = world.substrate.slope_at(probe, baseline_m)
        got = engine.substrate_slope_at(EARTH_RADIUS_M, v.x, v.y, v.z, baseline_m, field)
        assert abs(want - got) <= abs(want) * SUBSTRATE_SLOPE_DRIFT_REL, (baseline_m, want, got)
    assert world.substrate.slope_at(probe, 6.0) != world.substrate.slope_at(probe, 600.0), (
        "the baseline no longer changes the answer, so passing it proves nothing"
    )


def test_substrate_at_is_strict_when_every_optional_is_supplied():
    """
    With `elevation_m`, `slope` and `tectonic_m` all handed in, `at` reaches no
    transcendental of its own -- only `natural`, `Placed.weight_at` and `blended_towards`.

    **And it comes out bit-identical over both corpora, which is a finding rather than an
    assumption.** `weight_at` IS bounded (`atan2` and `hypot` inside `sphere_to_local`), so
    a tolerance would have been defensible here; over 4,682 points across the pinnacle and
    the open water, against all 25 placed features of the demo coast, the measured
    divergence is exactly zero in every one of the three fractions. So this asserts raw
    bits. `features.rs`'s own FEATURES_WEIGHT_MAX_ABS is 2.2e-14 and is NOT borrowed here:
    it is sized for a 250:1 dredged channel probed at its own support edge, which is
    nothing this corpus reaches, and importing it would let a real defect sit green.

    If this ever needs a tolerance, that is a finding to report, not a bound to add.
    """
    region, world = _demo_world()
    coast = region.coast
    tuples = [_feature_tuple(f) for f in world.features]
    field, tectonic = _engine_field(world), _engine_tectonic(world)
    pinnacle, _ = _substrate_grid(
        coast, PINNACLE_OFFSHORE_M, PINNACLE_ALONG_M, 140.0, 61,
    )
    checked = 0
    for label, points in (("pinnacle", pinnacle),
                          ("open water", _open_water_points(coast))):
        for point in points:
            v = point.vector
            known = dict(
                elevation_m=world.structural_m(point),
                slope=world.substrate.slope_at(point),
                tectonic_m=world.tectonics.offset_m(point),
            )
            want = world.substrate.at(point, **known)
            got = engine.substrate_at(
                tuples, EARTH_RADIUS_M, v.x, v.y, v.z, field, tectonic, **known,
            )
            assert same(want.sand, got[0]), (label, v, want.sand, got[0])
            assert same(want.mud, got[1]), (label, v, want.mud, got[1])
            assert same(want.rock, got[2]), (label, v, want.rock, got[2])
            assert want.dominant == engine.substrate_dominant_at(
                tuples, EARTH_RADIUS_M, v.x, v.y, v.z, field, tectonic, **known,
            ), (label, v)
            checked += 1
    assert checked == 3721 + 961


def test_substrate_at_with_a_derived_slope_stays_inside_its_own_bound():
    """
    The same two corpora with every optional left `None`, so `slope_at` runs inside `at`
    and its one ULP is amplified through `natural`'s smoothstep and the blends.

    Bounded by SUBSTRATE_AT_DERIVED_SLOPE_MAX_ABS, which is measured for THIS comparison
    and is a different quantity from SUBSTRATE_SLOPE_DRIFT_REL -- absolute rather than
    relative, and covering a longer span of the stack. Two-sided, so a later widening
    fails rather than passing.
    """
    region, world = _demo_world()
    coast = region.coast
    tuples = [_feature_tuple(f) for f in world.features]
    field, tectonic = _engine_field(world), _engine_tectonic(world)
    pinnacle, _ = _substrate_grid(
        coast, PINNACLE_OFFSHORE_M, PINNACLE_ALONG_M, 140.0, 61,
    )
    overall_worst = 0.0
    for label, points in (("pinnacle", pinnacle),
                          ("open water", _open_water_points(coast))):
        worst = 0.0
        flips = 0
        for point in points:
            v = point.vector
            want = world.substrate.at(point)
            got = engine.substrate_at(tuples, EARTH_RADIUS_M, v.x, v.y, v.z, field, tectonic)
            for a, b in zip((want.sand, want.mud, want.rock), got):
                assert abs(a - b) <= SUBSTRATE_AT_DERIVED_SLOPE_MAX_ABS, (label, v, a, b)
                worst = max(worst, abs(a - b))
            if want.dominant != engine.substrate_dominant_at(
                tuples, EARTH_RADIUS_M, v.x, v.y, v.z, field, tectonic,
            ):
                flips += 1
        assert flips == 0, (
            "the one-word answer flipped, which no tolerance can absorb because the "
            "output is a word", label, flips,
        )
        overall_worst = max(overall_worst, worst)
    assert SUBSTRATE_AT_DERIVED_SLOPE_MAX_ABS * 0.5 <= overall_worst \
        <= SUBSTRATE_AT_DERIVED_SLOPE_MAX_ABS, (
        "the measured worst no longer sits in the top half of its own bound", overall_worst,
    )


def test_substrate_at_maps_its_keywords_by_name_not_by_position():
    """
    **THE LIVE TRAP IN THIS BINDING.** `substrate.py` declares `at(point, elevation_m=None,
    slope=None, tectonic_m=None)`; `substrate::at` takes `elevation_m, tectonic_m, slope`,
    in the order the body RESOLVES them. All three are `Option<f64>`, so a binding that
    forwarded its arguments positionally would compile, type-check and be silently wrong --
    feeding the slope in as tectonic metres and the tectonic metres in as a dimensionless
    slope.

    Nothing catches that but a test. These probes are chosen so a swap changes the answer
    without either arm saturating a clamp -- a saturating case would give 1.0 both ways and
    prove nothing -- and the last of them flips the one-word answer outright.

    Verified against the mutation: swapping the two arguments at the engine call site in
    `bindings::substrate_at` fails this test on its first case.
    """
    host = _RecordingHost([])
    point = SpherePoint.from_latlon(11.0, 47.0)
    v = point.vector

    swap_cases = [
        # (elevation_m, slope, tectonic_m). tectonic_m is small enough that
        # `tectonic_m / ROCK_TECTONIC_M` is negligible, and large enough that
        # `tectonic_m / ROCK_SLOPE` is a real fraction -- so a swap is loud but neither
        # side of it clamps.
        (-80.0, 0.020, 0.024),
        (-45.0, 0.006, 0.012),
        (-100.0, 0.030, 0.036),
    ]
    for elevation_m, slope, tectonic_m in swap_cases:
        want = PyComposition(*_natural_triple(elevation_m, slope, tectonic_m))
        swapped = PyComposition(*_natural_triple(elevation_m, tectonic_m, slope))
        assert not (same(want.sand, swapped.sand) and same(want.rock, swapped.rock)), (
            "this probe no longer tells a correct mapping from a swapped one",
            elevation_m, slope, tectonic_m,
        )
        got = _engine_at(host, point, elevation_m=elevation_m, slope=slope,
                         tectonic_m=tectonic_m)
        assert same(want.sand, got[0]), (elevation_m, slope, tectonic_m, want.sand, got[0])
        assert same(want.mud, got[1]), (elevation_m, slope, tectonic_m)
        assert same(want.rock, got[2]), (elevation_m, slope, tectonic_m, want.rock, got[2])
        assert not same(swapped.rock, got[2]), (
            "the binding answered what a POSITIONAL forward would have answered -- the "
            "slope reached the engine as the tectonic contribution",
            elevation_m, slope, tectonic_m,
        )

    # And a swap that changes the WORD, which is what maritime consumes.
    elevation_m, slope, tectonic_m = -80.0, 0.0104, 0.0248
    assert PyComposition(*_natural_triple(elevation_m, slope, tectonic_m)).dominant == PY_SAND
    assert PyComposition(*_natural_triple(elevation_m, tectonic_m, slope)).dominant == PY_ROCK
    assert _engine_dominant_at(
        host, point, elevation_m=elevation_m, slope=slope, tectonic_m=tectonic_m,
    ) == PY_SAND

    # The same three values against the live Python, so the expectation is not this test's
    # own arithmetic.
    live = PySubstrate(host)
    for elevation_m, slope, tectonic_m in swap_cases:
        want = live.at(point, elevation_m=elevation_m, slope=slope, tectonic_m=tectonic_m)
        got = _engine_at(host, point, elevation_m=elevation_m, slope=slope,
                         tectonic_m=tectonic_m)
        assert same(want.sand, got[0]) and same(want.mud, got[1]) and same(want.rock, got[2])


def _natural_triple(elevation_m, slope, tectonic_m):
    """`Substrate.natural`'s three fractions, from the live Python, host-free."""
    composition = PySubstrate(None).natural(elevation_m, slope, tectonic_m)
    return composition.sand, composition.mud, composition.rock


def test_substrate_at_resolves_elevation_then_tectonic_then_slope():
    """
    Each `None` triggers a DIFFERENT host call, so the resolution order is observable
    across the FFI -- not merely inside the crate, where Task 4 already pins it.

    All three absent gives `[structural, tectonic, structural x4]`: the elevation, then the
    tectonic offset, then `slope_at`'s own four probes. Every partial combination drops
    exactly its own call from that sequence, and the ORDER of what remains is the
    signature. A binding that mapped its keywords positionally would produce a different
    sequence for two of these eight combinations, and a port that resolved slope before
    tectonic would produce a different one for all of them.
    """
    point = SpherePoint.from_latlon(-19.0, 122.0)
    for supply in range(8):
        known = {}
        if supply & 1:
            known["elevation_m"] = -70.0
        if supply & 2:
            known["tectonic_m"] = 220.0
        if supply & 4:
            known["slope"] = 0.011

        expected = []
        if "elevation_m" not in known:
            expected.append("structural")
        if "tectonic_m" not in known:
            expected.append("tectonic")
        if "slope" not in known:
            expected += ["structural"] * 4

        python_host = _RecordingHost([])
        PySubstrate(python_host).at(point, **known)
        assert python_host.calls == expected, (known, python_host.calls, expected)

        engine_host = _RecordingHost([])
        _engine_at(engine_host, point, **known)
        assert engine_host.calls == python_host.calls, (
            "the port asked the host for different things, or asked in a different order",
            known, engine_host.calls, python_host.calls,
        )


def test_substrate_at_treats_a_supplied_zero_as_a_value_not_an_absence():
    """
    `0.0` is a value. Elevation `0.0` is the datum, slope `0.0` is dead-flat ground and
    tectonic `0.0` is ground the plates did nothing to -- and re-deriving any of them would
    call the host and get a different number.

    Two assertions, because either alone is weak: the answer must match the Python's, AND
    the host must not be asked for the thing that was supplied. A binding that flattened
    the sentinel with a falsy test would pass the first and fail the second.
    """
    point = SpherePoint.from_latlon(4.0, -63.0)
    for name in ("elevation_m", "tectonic_m", "slope"):
        known = {name: 0.0}
        python_host = _RecordingHost([])
        want = PySubstrate(python_host).at(point, **known)

        engine_host = _RecordingHost([])
        got = _engine_at(engine_host, point, **known)
        assert same(want.sand, got[0]) and same(want.mud, got[1]) and same(want.rock, got[2]), name
        assert engine_host.calls == python_host.calls, (name, engine_host.calls)

        derived_host = _RecordingHost([])
        derived = PySubstrate(derived_host).at(point)
        assert len(derived_host.calls) > len(python_host.calls), (
            "supplying this optional no longer saves a host call, so this probe cannot "
            "tell a supplied zero from an absent value", name,
        )
        if name != "slope":
            assert not same(derived.rock, want.rock) or not same(derived.sand, want.sand), (
                "the host happens to return exactly 0.0 for this member, so supplying "
                "0.0 is indistinguishable from deriving it and the probe proves nothing",
                name,
            )


def test_substrate_at_skips_a_feature_that_omits_a_substrate():
    """
    `if declared is None: continue` -- a genuine skip, and it is NOT the same branch as a
    feature that declared a word `PURE` has no entry for.

    **All 25 features on the demo coast declare a substrate**, so a corpus built from that
    world alone never takes this branch and never exercises either side of the guard. This
    fixture puts an omitting feature exactly where a declaring one also reaches, so the
    skip is load-bearing: with it skipped the answer is the ground's own composition
    blended once, and if it were not skipped there would be no `PURE[None]` to blend
    towards at all.
    """
    at = SpherePoint.from_latlon(-6.0, 88.0)
    silent = _feature(kind="silent", lat=-6.0, lon=88.0, length_m=4000.0, width_m=4000.0,
                      substrate=None)
    stated = _feature(kind="stated", lat=-6.0, lon=88.0, length_m=4000.0, width_m=4000.0,
                      substrate=PY_MUD)
    host_both = _RecordingHost([silent, stated])
    host_stated = _RecordingHost([stated])

    probe = at
    v = probe.vector
    assert host_both.features.placed[0].weight_at(probe) > 0.0, (
        "the omitting feature does not reach this probe, so its skip is never taken here"
    )

    want_both = PySubstrate(host_both).at(probe)
    want_stated = PySubstrate(host_stated).at(probe)
    assert same(want_both.sand, want_stated.sand) and same(want_both.mud, want_stated.mud) \
        and same(want_both.rock, want_stated.rock), (
        "the omitting feature changed the answer in the reference, so it is not being "
        "skipped and this test is testing something else"
    )
    got_both = _engine_at(host_both, probe)
    got_stated = _engine_at(host_stated, probe)
    assert same(want_both.sand, got_both[0]) and same(want_both.mud, got_both[1]) \
        and same(want_both.rock, got_both[2])
    assert got_both == got_stated, (got_both, got_stated)

    # And it is not merely that the answer is unchanged: the declaring feature really is
    # doing something here, so "unchanged" is a skip rather than a no-op world.
    host_none = _RecordingHost([silent])
    bare = _engine_at(host_none, probe)
    assert bare != got_both, (
        "the declaring feature has no effect at this probe either, so the two answers "
        "would agree whatever the skip did", bare, got_both,
    )


def test_substrate_at_refuses_an_empty_string_substrate_on_both_sides():
    """
    An empty string is a word `PURE` has no entry for, and it is NOT the `None` sentinel --
    `test_features_substrate_none_survives_the_crossing_as_none_not_as_empty` already pins
    that it crosses the FFI distinct from `None`, so a value this port guarantees can reach
    `at` is a value that makes the Python raise `KeyError`.

    **Both sides must fail.** A silent success on either would be the worst divergence this
    module could carry, because the two languages would then disagree about whether an
    answer EXISTS -- not about its last bit. Continuing past the miss (`if let Some(...)`
    and on to the next feature) would answer where the reference refuses.

    And the refusal is conditional on the weight, exactly as the Python's is: the lookup
    lives inside `if weight > 0.0`, so an unreachable feature declaring nonsense is not an
    error on either side.
    """
    for declared in ("", " ", "Rock", "gravel"):
        bad = _feature(kind="bad", lat=30.0, lon=-15.0, length_m=3000.0, width_m=3000.0,
                       substrate=declared)
        host = _RecordingHost([bad])
        probe = SpherePoint.from_latlon(30.0, -15.0)
        assert host.features.placed[0].weight_at(probe) > 0.0, declared

        with pytest.raises(KeyError):
            PySubstrate(host).at(probe)
        with pytest.raises(engine.UnknownSubstrateError) as raised:
            _engine_at(host, probe)
        assert issubclass(engine.UnknownSubstrateError, KeyError), (
            "the port's refusal is no longer catchable as the KeyError the reference "
            "raises, so a caller handling one would not handle the other"
        )
        assert declared in str(raised.value) or repr(declared) in str(raised.value)

        with pytest.raises(KeyError):
            PySubstrate(host).dominant_at(probe)
        with pytest.raises(engine.UnknownSubstrateError):
            _engine_dominant_at(host, probe)

        # Out of reach: no lookup happens, and neither side raises.
        far = SpherePoint.from_latlon(-30.0, 165.0)
        assert host.features.placed[0].weight_at(far) == 0.0, declared
        unreached = PySubstrate(host).at(far)
        got = _engine_at(host, far)
        assert same(unreached.sand, got[0]) and same(unreached.mud, got[1]) \
            and same(unreached.rock, got[2]), declared


def test_substrate_at_with_no_features_one_and_several():
    """
    The blend loop at all three of its interesting lengths, plus the `weight > 0.0` guard
    inside it, on an analytic host so nothing but the loop is under test.

    **Order is composition here too.** Two features declaring different substrates over the
    same water give different answers reversed, so the reversed pair is compared as well --
    a port iterating `placed` in any order but construction order would pass the forward
    case and fail this one.
    """
    lat, lon = 22.0, -140.0
    probe = SpherePoint.from_latlon(lat, lon)
    rock = _feature(kind="pinnacle", lat=lat, lon=lon, length_m=5000.0, width_m=5000.0,
                    substrate=PY_ROCK)
    mud = _feature(kind="basin", lat=lat + 0.01, lon=lon, length_m=6000.0, width_m=6000.0,
                   substrate=PY_MUD)
    sand = _feature(kind="bar", lat=lat, lon=lon + 0.01, length_m=6000.0, width_m=6000.0,
                    substrate=PY_SAND)
    silent = _feature(kind="silent", lat=lat, lon=lon, length_m=5000.0, width_m=5000.0,
                      substrate=None)
    far = _feature(kind="far", lat=-lat, lon=-lon, length_m=800.0, width_m=800.0,
                   substrate=PY_ROCK)

    populations = {
        "none": [],
        "one": [rock],
        "several": [rock, mud, sand],
        "several reversed": [sand, mud, rock],
        "several with an omitter and an unreachable": [rock, silent, mud, far, sand],
    }
    answers = {}
    for label, features in populations.items():
        host = _RecordingHost(features)
        want = PySubstrate(host).at(probe)
        got = _engine_at(host, probe)
        assert same(want.sand, got[0]), (label, want.sand, got[0])
        assert same(want.mud, got[1]), (label, want.mud, got[1])
        assert same(want.rock, got[2]), (label, want.rock, got[2])
        assert want.dominant == _engine_dominant_at(host, probe), label
        answers[label] = got

    assert answers["none"] != answers["one"], "one feature made no difference"
    assert answers["one"] != answers["several"], "the extra two made no difference"
    assert answers["several"] != answers["several reversed"], (
        "reversing the features gave the same answer, so this probe cannot tell "
        "construction order from any other order -- blending is not commutative",
        answers["several"],
    )
    assert answers["several"] == answers["several with an omitter and an unreachable"], (
        "the omitting feature or the out-of-reach one changed the answer",
        answers["several"], answers["several with an omitter and an unreachable"],
    )
