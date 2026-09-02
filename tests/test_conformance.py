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
from worldbuilder.geometry.vectors import Vec3

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
