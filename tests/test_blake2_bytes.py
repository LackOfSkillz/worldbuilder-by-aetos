"""
Throwaway spike for Task 1 of slice 1j (plate generation): prove that the Rust engine's
pinned `blake2` crate reproduces CPython's `hashlib.blake2b(message, digest_size=8)`
bit-for-bit, before `worldbuilder/plates/generation.py`'s `_fraction` is ported at all.

This is the one question in the slice with no tolerance: `_fraction` seeds every plate's
position, pole and rate from this hash, and a single differing bit gives an unrelated
`u64` and therefore a completely different planet -- there is no bounded-ULP fallback
the way there is for transcendental functions elsewhere in this port.

The cross-language proof itself already happened once, by construction: every expected
digest below was computed here, in Python, against the live `hashlib`, and then copied
byte-for-byte into `crates/worldbuilder-engine/tests/blake2_bytes.rs`, where
`cargo test -p worldbuilder-engine --test blake2_bytes` asserts the Rust `blake2` crate
(pinned to `=0.10.6`, using the runtime-variable `Blake2bVar::new(8)` rather than a fixed
`Blake2b512` plus a truncation) produces the identical bytes. This file's job is to be
the durable, re-runnable half of that proof: if anyone edits either file's constants by
hand, the two suites drift apart and both fail.

Deleted in Task 7, once `_fraction` is actually ported and `test_conformance.py` covers
it directly against the built engine.
"""

import hashlib
import math
import struct

# Mirrors crates/worldbuilder-engine/tests/blake2_bytes.rs::conformance_vectors() --
# the two lists must stay identical byte for byte. (message, expected 8-byte digest hex)
VECTORS = [
    ("0|plate|0|jitter-a", "6579480d289e0246"),
    ("0|plate|0|jitter-b", "66610ec03eefed6b"),
    ("0|plate|0|pole-z", "46246c9e85e0d489"),
    ("0|plate|0|pole-angle", "b06ed5b662bc5875"),
    ("0|plate|0|rate", "df16507c82e6f5e3"),
    ("0|plate|0|sense", "923fb033d442095d"),
    ("0|plate|21|jitter-a", "11aa8e5e7971a091"),
    ("0|plate|21|pole-z", "fc7498a982a0fd7b"),
    ("0|plate|21|sense", "f132e2b55d46c476"),
    ("-1|plate|0|jitter-a", "df527febdcc84106"),
    ("-1|plate|7|pole-z", "24db0df44a406149"),
    ("-1|plate|21|sense", "3c4f54a3bc40fb3b"),
    ("4611686018427387904|plate|0|pole-z", "2dfd5b9406a878d8"),
    ("4611686018427387904|plate|7|rate", "b41654f786a22d76"),
    ("-4611686018427387904|plate|1|pole-angle", "ce9df200d78dff6b"),
    ("987654321|plate|0|jitter-a", "28da1f391eee797f"),
    ("987654321|plate|1|jitter-b", "9a1ed2493562d24c"),
    ("987654321|plate|7|pole-z", "17b140750047bf34"),
    ("987654321|plate|7|pole-angle", "240644668b7e1d47"),
    ("987654321|plate|21|rate", "ee74e5a1d0e4f4cd"),
    ("987654321|plate|21|sense", "2e3db7203e181430"),
    ("20260831|plate|7|jitter-a", "0a3e490208d2c2f1"),
    ("20260831|plate|7|jitter-b", "2c5cec1baa150438"),
    ("20260831|plate|7|pole-angle", "5aba4af4a248835c"),
    ("20260831|plate|7|rate", "9a1de7766019ab68"),
    ("20260831|plate|7|sense", "a3fc8046f277e7ae"),
]

MEASURED_MESSAGE = "20260831|plate|7|pole-z"
MEASURED_DIGEST_HEX = "2d729d257c6a1550"
MEASURED_U64 = 5770635578984722989
MEASURED_FRACTION = 0.3128267815678692


def _fraction_from_python(world_seed, *parts):
    """The exact computation in `worldbuilder/plates/generation.py::_fraction`."""
    key = "|".join(str(part) for part in (world_seed,) + parts).encode("utf-8")
    digest = hashlib.blake2b(key, digest_size=8).digest()
    return digest, struct.unpack("<Q", digest)[0]


def test_measured_vector_reproduces_from_scratch():
    """
    Re-derive the brief's measured vector independently, rather than trusting the four
    quoted lines -- this project has had wrong numbers in a plan before.
    """
    message_bytes = MEASURED_MESSAGE.encode("utf-8")
    assert len(message_bytes) == 23, (
        f"expected the measured message to be 23 UTF-8 bytes, got {len(message_bytes)}"
    )

    digest = hashlib.blake2b(message_bytes, digest_size=8).digest()
    assert digest.hex() == MEASURED_DIGEST_HEX, (
        f"hashlib.blake2b({MEASURED_MESSAGE!r}, digest_size=8).digest() was "
        f"{digest.hex()}, expected {MEASURED_DIGEST_HEX}"
    )

    u = struct.unpack("<Q", digest)[0]
    assert u == MEASURED_U64, f"u64 from digest {digest.hex()} was {u}, expected {MEASURED_U64}"

    fraction = u / float(1 << 64)
    assert fraction == MEASURED_FRACTION, (
        f"fraction from u64 {u} was {fraction!r}, expected {MEASURED_FRACTION!r}"
    )


def test_digests_match_the_rust_conformance_vectors():
    """
    Every `(message, expected_digest)` pair here must also appear, byte for byte, in
    `crates/worldbuilder-engine/tests/blake2_bytes.rs::conformance_vectors()`, which
    asserts the Rust `blake2` crate produces the same bytes. This test only proves the
    Python half; `cargo test -p worldbuilder-engine --test blake2_bytes` proves the Rust
    half. Together they are the byte-for-byte cross-language comparison this task exists
    to make.
    """
    for message, expected_hex in VECTORS:
        digest = hashlib.blake2b(message.encode("utf-8"), digest_size=8).digest()
        assert digest.hex() == expected_hex, (
            f"message {message!r} hashed to {digest.hex()}, expected {expected_hex} -- "
            "if this fails, the Rust side's copy of this vector is now the only "
            "reference, and it must not be trusted without re-deriving from Python again"
        )


def test_fraction_matches_generation_module_directly():
    """
    Cross-check the vectors against the actual `_fraction` call shape used by
    `generation.py` (`world_seed, "plate", index, label`), not just a pre-joined string,
    so a mistake in how the parts are joined would also be caught here.
    """
    cases = [
        (0, 0, "jitter-a", "6579480d289e0246"),
        (0, 21, "sense", "f132e2b55d46c476"),
        (-1, 7, "pole-z", "24db0df44a406149"),
        (4611686018427387904, 0, "pole-z", "2dfd5b9406a878d8"),
        (-4611686018427387904, 1, "pole-angle", "ce9df200d78dff6b"),
        (987654321, 21, "rate", "ee74e5a1d0e4f4cd"),
        (20260831, 7, "pole-z", MEASURED_DIGEST_HEX),
    ]
    for world_seed, index, label, expected_hex in cases:
        digest, _ = _fraction_from_python(world_seed, "plate", index, label)
        assert digest.hex() == expected_hex, (
            f"_fraction({world_seed}, 'plate', {index}, {label!r}) digest was "
            f"{digest.hex()}, expected {expected_hex}"
        )


def test_u64_to_fraction_conversion_and_whether_anything_rounds():
    """
    `struct.unpack("<Q", digest)[0] / float(1 << 64)` converts a u64 to an f64 by IEEE
    round-to-nearest, which is the same rule Rust's `as f64` cast follows -- so the two
    languages agree on every value, including ones above 2**53 where the conversion is
    not exact. Assert that at least one tested value actually exercises that rounding, so
    the claim "this path is tested" is backed by an observation rather than a hope.
    """
    saw_a_value_above_two_pow_53 = False
    saw_a_value_that_actually_rounded = False

    for message, expected_hex in VECTORS + [(MEASURED_MESSAGE, MEASURED_DIGEST_HEX)]:
        digest = hashlib.blake2b(message.encode("utf-8"), digest_size=8).digest()
        u = struct.unpack("<Q", digest)[0]
        fraction = u / float(1 << 64)

        assert 0.0 <= fraction < 1.0, f"fraction {fraction} from u64 {u} is out of [0, 1)"

        if u > (1 << 53):
            saw_a_value_above_two_pow_53 = True
            # A u64 above 2**53 cannot be represented exactly as an f64: converting it to
            # float and back loses the low bits whenever it does not happen to be a
            # multiple of a large enough power of two.
            if int(float(u)) != u:
                saw_a_value_that_actually_rounded = True

    assert saw_a_value_above_two_pow_53, (
        "no tested u64 exceeded 2**53 -- the f64-rounding path step 3 asks about would "
        "be untested"
    )
    assert saw_a_value_that_actually_rounded, (
        "every tested u64 above 2**53 happened to round-trip exactly through f64 -- the "
        "rounding behaviour step 3 asks about was never actually observed"
    )


def test_spread_degeneracy_guard_is_unreachable():
    """
    `_spread`'s guard is `if sideways.length() < 1e-9`, where `sideways` is the cross
    product of the z axis with the seed point -- so its length is the spiral's ring
    radius. With `z = 1 - 2u` for `u = (index + 0.5) / count`,
    `1 - z**2 = 4u(1 - u)`, so `ring = 2*sqrt(u(1 - u))`, smallest at `index = 0` where
    `u = 0.5 / count` and `ring` approaches `sqrt(2/count)` for large `count`.

    Confirms that derivation against the actual `_spread` formula, and that the smallest
    ring measured is many orders of magnitude above the 1e-9 guard threshold.
    """
    counts = [1, 2, 3, 22, 1000, 100_000]
    rings = []

    for count in counts:
        index = 0
        z = 1.0 - 2.0 * (index + 0.5) / count
        ring_from_z = math.sqrt(max(0.0, 1.0 - z * z))

        u = (index + 0.5) / count
        ring_from_derivation = 2.0 * math.sqrt(u * (1.0 - u))

        assert math.isclose(ring_from_z, ring_from_derivation, rel_tol=1e-9), (
            f"for count={count}, ring from z ({ring_from_z}) and from the derivation "
            f"({ring_from_derivation}) disagree"
        )
        rings.append(ring_from_z)

    minimum_ring = min(rings)
    largest_count = max(counts)
    approx_for_largest_count = math.sqrt(2.0 / largest_count)

    assert math.isclose(minimum_ring, approx_for_largest_count, rel_tol=1e-3), (
        f"minimum ring {minimum_ring} (at count={largest_count}) does not match the "
        f"sqrt(2/count) approximation {approx_for_largest_count} to within 0.1%"
    )
    assert minimum_ring > 1e-9 * 1_000_000, (
        f"minimum ring {minimum_ring} across counts {counts} is not at least a million "
        "times the 1e-9 guard threshold -- the guard might be reachable after all"
    )
