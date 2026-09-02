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
import struct

import pytest

from worldbuilder.geometry.sphere import EARTH_RADIUS_M, SpherePoint
from worldbuilder.geometry.vectors import Vec3

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
    """
    worst = 0
    for lat in range(-90, 91):
        for lon in range(-180, 181, 5):
            want = SpherePoint.from_latlon(float(lat), float(lon)).vector
            got = engine.sphere_from_latlon(float(lat), float(lon))
            for w, g in zip((want.x, want.y, want.z), got):
                d = ulps_apart(w, g)
                if d is not None:
                    worst = max(worst, abs(d))
    assert worst <= MAX_TRANSCENDENTAL_ULPS, f"divergence grew to {worst} ULP"


def test_the_strict_contract_is_still_strict():
    """Vec3 has no transcendental in its path and must agree exactly, not approximately."""
    for x, y, z in corpus(500):
        assert same(Vec3(x, y, z).length(), engine.vec3_length(x, y, z))


def test_the_harness_can_actually_fail():
    """
    A conformance suite that cannot fail proves nothing. This asserts that `same` really
    distinguishes a one-bit difference, so a passing run above means something.
    """
    value = 0.1
    nudged = struct.unpack("<d", struct.pack("<Q", bits(value) + 1))[0]
    assert value != nudged
    assert not same(value, nudged)
    assert math.isclose(value, nudged)  # and a tolerance would have called them equal
