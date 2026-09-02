"""
Bit-for-bit conformance between the Python reference and the Rust engine.

The engine is not a rewrite that should behave similarly. It is a port that must agree
exactly, because a chart is wrong in the same places every voyage and that is what makes
surveying mean anything. Comparison is therefore on raw f64 bit patterns, never with a
tolerance -- a tolerance would let a coastline move by a metre and call it equal.

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


def test_sphere_from_latlon_agrees():
    for lat in range(-90, 91, 3):
        for lon in range(-180, 181, 7):
            want = SpherePoint.from_latlon(float(lat), float(lon)).vector
            got = engine.sphere_from_latlon(float(lat), float(lon))
            assert same(want.x, got[0]) and same(want.y, got[1]) and same(want.z, got[2]), (lat, lon)


def test_sphere_to_latlon_agrees():
    for x, y, z in corpus():
        point = SpherePoint(Vec3(x, y, z).normalised())
        want = point.to_latlon()
        got = engine.sphere_to_latlon(point.vector.x, point.vector.y, point.vector.z)
        assert same(want[0], got[0]) and same(want[1], got[1]), (x, y, z)


def test_sphere_angle_and_distance_agree():
    points = list(corpus(2000))
    for (ax, ay, az), (bx, by, bz) in zip(points, points[1:]):
        a = SpherePoint(Vec3(ax, ay, az).normalised())
        b = SpherePoint(Vec3(bx, by, bz).normalised())
        av, bv = a.vector, b.vector
        assert same(
            a.angle_to(b),
            engine.sphere_angle_to(av.x, av.y, av.z, bv.x, bv.y, bv.z),
        )
        assert same(
            a.distance_to(b),
            engine.sphere_distance_to(av.x, av.y, av.z, bv.x, bv.y, bv.z, EARTH_RADIUS_M),
        )


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
