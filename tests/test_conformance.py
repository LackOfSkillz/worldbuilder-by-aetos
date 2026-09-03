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
# discrete question from the *size* of their contribution, and Task 1 measured that one
# strictly: the smallest observed `abs(abs(across) - ACROSS_ENOUGH)` was 1.19069e-04, about
# 1.07e12 ULP of `across` -- twelve orders of magnitude clear of the 1-ULP `hypot`
# divergence that could ever move `across` at all. So which margins engage is reproducible
# and is exercised here through real geometry (a genuine two-margin point, and points
# deliberately close to the gate that this section finds on the standing 12-plate fixture),
# not hedged with a tolerance.

from worldbuilder.terrain.tectonics import Tectonics as PyTectonics
from worldbuilder.terrain.tectonics import Setting as PySetting
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
        CONTINENTALITY_SEED_FOR_TECTONICS, PY_LAND_FRACTION, radius_m,
        x, y, z, distance_m, normal.x, normal.y, normal.z,
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
