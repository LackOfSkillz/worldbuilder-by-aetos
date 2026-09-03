"""
Throwaway spike for slice 1o, Task 1. **Deleted in Task 6.**

Four questions, each answered by measurement rather than by argument, each figure carrying
its population, its method (with the method's parameters) and its host.

    Q1  Is `world_seed as u64` faithful for the two `Noise`-backed constructors?
    Q2  Can `shaped == -0.0` ever reach `Detail.offset_m`'s `amplitude_m <= 0.0` guard?
    Q3  What are the four reordering deltas, on a named grid?
    Q4  Do the two exact invariants hold bit-for-bit, and over what?

Run `python tests/test_surface_gates.py` for the full ledger; run under pytest for the
pass/fail gates.
"""

import math
import platform
import random
import struct
import sys

from worldbuilder.bathymetry.features import (
    CARVE,
    RAISE,
    SETTLE_M,
    SHAPE,
    Feature,
    Features,
)
from worldbuilder.geometry.sphere import EARTH_RADIUS_M, SpherePoint
from worldbuilder.plates.generation import plates_for
from worldbuilder.regions.demo import WORLD_SEED, Coast, demo_region
from worldbuilder.terrain.continentality import LAND_FRACTION, Continentality
from worldbuilder.terrain.detail import Detail
from worldbuilder.terrain.noise import MASK, Noise, _lattice
from worldbuilder.terrain.surface import Surface

try:
    import worldbuilder_engine as ENGINE
except ImportError:  # pragma: no cover - the crate may not be built
    ENGINE = None


def host():
    return "%s | %s | %s | CPython %s" % (
        platform.node(),
        platform.platform(),
        platform.processor(),
        sys.version.replace("\n", " "),
    )


def bits(value):
    """The exact IEEE-754 bit pattern, so `-0.0` and `+0.0` are distinguishable."""
    return struct.unpack("<Q", struct.pack("<d", value))[0]


def is_negative_zero(value):
    return value == 0.0 and math.copysign(1.0, value) < 0.0


# --------------------------------------------------------------------------------------
# The corpus. Named once; used by Q2's census, Q3 and Q4.
# --------------------------------------------------------------------------------------

GRID_STEPS = 25          # 25 x 25 = 625 points
GRID_SPAN_M = 45_000.0   # +/- 45 km
GRID_STEP_M = 2.0 * GRID_SPAN_M / (GRID_STEPS - 1)   # 3,750 m


def demo_grid():
    """625 points, +/-45,000 m, 3,750 m per step, centred on the demo-coast anchor."""
    coast = Coast()
    offsets = [-GRID_SPAN_M + index * GRID_STEP_M for index in range(GRID_STEPS)]
    return [coast.at(offshore, along) for offshore in offsets for along in offsets]


def demo_surface():
    return Surface(WORLD_SEED, features=demo_region().features)


def bare_surface():
    return Surface(WORLD_SEED)


# --------------------------------------------------------------------------------------
# Q1. The seed cast.
# --------------------------------------------------------------------------------------

def negative_seed_population():
    """
    Named population for every Q1 figure.

    1,000 dense negatives (-1 .. -1000)
      + 25 structured extremes (two's-complement boundaries, sign-bit patterns, the
        demo seed negated, two lattice multipliers negated)
      + 1,024 pseudo-random draws from `random.Random(20260903).randrange(-2**63, 0)`
    = 2,049 negative seeds.
    """
    dense = [-n for n in range(1, 1001)]
    extremes = [
        -1,
        -2,
        -(2 ** 63),
        -(2 ** 63) + 1,
        -(2 ** 63) + 2,
        -(2 ** 62),
        -(2 ** 32),
        -(2 ** 32) - 1,
        -(2 ** 31),
        -(2 ** 16),
        -(2 ** 53),
        -(2 ** 53) - 1,
        -(0x9E3779B97F4A7C15 % (2 ** 63)) - 1,
        -WORLD_SEED,
        -20260831,
        -0x7FFFFFFFFFFFFFFF,
        -0x0123456789ABCDEF,
        -0x5555555555555555,
        -0xAAAAAAAAAAAAAAA,
        -0xFFFFFFFF,
        -0x100000001B3,
        -(0x27D4EB2F165667C5 % (2 ** 63)) - 1,
        -3,
        -7,
        -1000000007,
    ]
    rng = random.Random(20260903)
    random_draws = [rng.randrange(-(2 ** 63), 0) for _ in range(1024)]
    return dense + extremes + random_draws


LATTICE_COORDS = [
    (0, 0, 0), (1, 0, 0), (0, 1, 0), (0, 0, 1),
    (-1, -1, -1), (-7, 13, -29), (12345, -67890, 424242),
    (2 ** 31, -(2 ** 31), 2 ** 40), (-(2 ** 40), 3, -5),
]

NOISE_PROBES = [
    (0.0, 0.0, 0.0), (0.5, 0.25, 0.125), (-3.75, 12.5, -0.03125),
    (1234.5678, -8765.4321, 0.999), (-0.5, -0.5, -0.5),
]

SALTS = (0, 0x0C0FFEE, 0x5EABED)


def q1_lattice_and_noise():
    """
    Method: for each negative seed `s`, form `u = s & ((1 << 64) - 1)` -- which is exactly
    what `s as u64` produces in Rust -- and compare, bit-for-bit via `struct.pack`:

      (a) `_lattice(ix, iy, iz, s)` vs `_lattice(ix, iy, iz, u)` over 9 coordinate triples
          including negative and > 2^31 components;
      (b) `Noise(s, salt).seed & MASK` vs `Noise(u, salt).seed & MASK` for the three salts
          the engine uses (0, 0x0C0FFEE continentality, 0x5EABED detail);
      (c) `Noise(s, salt).at(x, y, z)` vs `Noise(u, salt).at(x, y, z)` over 5 probes.

    Population: `negative_seed_population()` -- 2,049 negative seeds.
    """
    seeds = negative_seed_population()
    lattice_pairs = seed_pairs = noise_pairs = 0
    mismatches = []
    for s in seeds:
        u = s & MASK
        for coord in LATTICE_COORDS:
            lattice_pairs += 1
            a = _lattice(coord[0], coord[1], coord[2], s)
            b = _lattice(coord[0], coord[1], coord[2], u)
            if bits(a) != bits(b):
                mismatches.append(("lattice", s, coord, a, b))
        for salt in SALTS:
            seed_pairs += 1
            ns, nu = Noise(s, salt=salt), Noise(u, salt=salt)
            if (ns.seed & MASK) != (nu.seed & MASK):
                mismatches.append(("mixed-seed", s, salt, ns.seed, nu.seed))
            for probe in NOISE_PROBES:
                noise_pairs += 1
                a = ns.at(*probe)
                b = nu.at(*probe)
                if bits(a) != bits(b):
                    mismatches.append(("noise.at", s, (salt, probe), a, b))
    return {
        "seeds": len(seeds),
        "lattice_pairs": lattice_pairs,
        "seed_pairs": seed_pairs,
        "noise_pairs": noise_pairs,
        "mismatches": mismatches,
    }


def q1_unmasked_intermediate_is_negative():
    """
    The fact the marker's wording has to rest on, measured rather than asserted: the
    intermediate `Noise.seed` is a *negative, unbounded* Python int for a negative world
    seed -- it is genuinely not a u64 until `_lattice` masks it. Counts how many of the
    population produce a negative `Noise.seed` at each salt.
    """
    seeds = negative_seed_population()
    negative_at_salt = {}
    for salt in SALTS:
        negative_at_salt[salt] = sum(1 for s in seeds if Noise(s, salt=salt).seed < 0)
    return {"seeds": len(seeds), "negative_mixed_seed": negative_at_salt}


def q1_cross_language():
    """
    The same reinterpretation, checked across the language boundary. For each negative seed
    `s` with `u = s & MASK`, Python built on `s` against the Rust crate built on `u`,
    bit-for-bit:

      (a) `Noise(s, salt).at(...)`           vs `engine.noise_at(u, salt, ...)`
      (b) `Detail(s, R).offset_m(p, A, res)` vs `engine.detail_offset_m(u, R, ..., A, res)`
      (c) `Continentality(s).at(p)`          vs `engine.continentality_at(u, lf, ...)`

    Subsamples, named as subsamples: (a) and (b) run on 128 seeds (the first 8 of the dense
    block plus `random.Random(1).sample(population, 120)`); (c) on 16 seeds (first 4 plus
    `random.Random(1).sample(population, 12)`), because each `Continentality` construction
    runs a 4,000-point calibration.
    """
    if ENGINE is None:
        return None
    seeds = negative_seed_population()
    rng = random.Random(1)
    cheap = seeds[:8] + rng.sample(seeds, 120)
    dear = seeds[:4] + rng.sample(seeds, 12)
    radius = EARTH_RADIUS_M
    point = SpherePoint.from_latlon(41.2, -8.7)
    v = point.vector
    mismatches = []
    noise_pairs = detail_pairs = cont_pairs = 0
    for s in cheap:
        u = s & MASK
        for salt in SALTS:
            for probe in NOISE_PROBES:
                noise_pairs += 1
                a = Noise(s, salt=salt).at(*probe)
                b = ENGINE.noise_at(u, salt, *probe)
                if bits(a) != bits(b):
                    mismatches.append(("noise_at", s, salt, probe, a, b))
        for resolution in (None, 500.0, 5000.0):
            detail_pairs += 1
            a = Detail(s, radius).offset_m(point, 25.0, resolution)
            b = ENGINE.detail_offset_m(u, radius, v.x, v.y, v.z, 25.0, resolution)
            if bits(a) != bits(b):
                mismatches.append(("detail_offset_m", s, resolution, a, b))
    for s in dear:
        u = s & MASK
        cont_pairs += 1
        a = Continentality(s, radius, LAND_FRACTION).at(point)
        b = ENGINE.continentality_at(u, LAND_FRACTION, v.x, v.y, v.z)
        if bits(a) != bits(b):
            mismatches.append(("continentality_at", s, a, b))
    return {
        "cheap_seeds": len(cheap),
        "dear_seeds": len(dear),
        "noise_pairs": noise_pairs,
        "detail_pairs": detail_pairs,
        "cont_pairs": cont_pairs,
        "mismatches": mismatches,
    }


def q1_plates_must_not_be_cast():
    """
    The counter-measurement, and the reason the marker's wording matters. `plates_for` does
    NOT go through `_lattice`: `generation._fraction` builds a **decimal string** key
    (`"|".join(str(part) ...)`) and blake2b's it, so `str(-5)` and
    `str(18446744073709551611)` are different keys and the mask is *not* a no-op.

    Method: for 64 negative seeds, compare `plates_for(s, 22)` plate-seed vectors against
    `plates_for(s & MASK, 22)` bit-for-bit; and, when the crate is present, the Rust
    `generation_plates_for(s, 22)` (an `i64` parameter, no cast) against the Python
    `plates_for(s, 22)`.

    The cross-language comparison is **not** bit-for-bit and must not be: the crate uses
    pure-Rust `libm` while CPython uses the platform libm, so `_spread`'s trigonometry is a
    permitted divergence. What is reported instead is the worst component distance -- which
    separates "same blake2b key, different last bits" (~1e-16) from "different key
    entirely" (order 1).

    Population: the first 8 of the dense block plus `random.Random(2).sample(population, 56)`.
    """
    seeds = negative_seed_population()
    rng = random.Random(2)
    sample = seeds[:8] + rng.sample(seeds, 56)
    differed = 0
    rust_checked = 0
    worst_i64 = 0.0
    worst_masked = 0.0

    def spread(rust_rows, python_vectors):
        worst = 0.0
        for (_, rv, _, _), pv in zip(rust_rows, python_vectors):
            worst = max(worst, abs(rv[0] - pv.x), abs(rv[1] - pv.y), abs(rv[2] - pv.z))
        return worst

    for s in sample:
        u = s & MASK
        a = [p.seed.vector for p in plates_for(s, 22).plates]
        b = [p.seed.vector for p in plates_for(u, 22).plates]
        if any(bits(x.x) != bits(y.x) or bits(x.y) != bits(y.y) or bits(x.z) != bits(y.z)
               for x, y in zip(a, b)):
            differed += 1
        if ENGINE is not None and -(2 ** 63) <= s < 2 ** 63:
            rust_checked += 1
            rust = ENGINE.generation_plates_for(s, 22)
            worst_i64 = max(worst_i64, spread(rust, a))
            worst_masked = max(worst_masked, spread(rust, b))
    return {
        "sample": len(sample),
        "masked_differed": differed,
        "rust_checked": rust_checked,
        "worst_rust_i64_vs_python_negative_seed": worst_i64,
        "worst_rust_i64_vs_python_masked_seed": worst_masked,
    }


# --------------------------------------------------------------------------------------
# Q2. The OPEN `-0.0` case.
#
# The guard is bit-observable in `Surface.elevation_m` only when BOTH hold at once:
#   (i)  the guard fires             -- `amplitude * (1.0 - authority) <= 0.0`
#   (ii) `shaped` is exactly `-0.0`  -- because `-0.0 + (+0.0) == +0.0` while
#                                       `-0.0 + (-0.0) == -0.0`.
# Each leg is measured separately, and then their conjunction.
# --------------------------------------------------------------------------------------

def q2_addition_lemma():
    """
    Leg 0: why `shaped == -0.0` is the *only* input that makes the guard bit-observable.

    Method: exhaustive over the 4 signed-zero pairs, plus 200,000 pseudo-random draws from
    `random.Random(7)` of `a` over
    `{10**u for u in uniform(-300, 300)} U {+-that} U {+-0.0} U {integers -9..9}`,
    each paired with both signed zeros. Checks (1) `fl(a + b)` is `-0.0` only when both
    operands are `-0.0`, and (2) `bits(a + b) == bits(a)` whenever `a` is not `-0.0`.
    """
    rng = random.Random(7)
    zeros = (0.0, -0.0)
    checked = 0
    absorbed = 0
    violations = []
    for a in zeros:
        for b in zeros:
            checked += 1
            if is_negative_zero(a + b) and not (is_negative_zero(a) and is_negative_zero(b)):
                violations.append(("zero-pair", a, b))
    for _ in range(200_000):
        magnitude = 10.0 ** rng.uniform(-300.0, 300.0)
        a = rng.choice([magnitude, -magnitude, 0.0, -0.0, float(rng.randint(-9, 9))])
        for b in zeros:
            checked += 1
            total = a + b
            if is_negative_zero(total) and not (is_negative_zero(a) and is_negative_zero(b)):
                violations.append(("random", a, b))
            if not is_negative_zero(a):
                if bits(total) == bits(a):
                    absorbed += 1
                else:
                    violations.append(("not-absorbed", a, b, total))
    return {
        "checked": checked,
        "absorbed_when_a_is_not_negative_zero": absorbed,
        "violations": violations,
    }


def q2_amplitude_floor():
    """
    Leg (i)a: `Detail.amplitude_m` never returns a value `<= 0.0`, so the guard can only be
    reached through `amplitude *= 1.0 - authority` with `authority == 1.0` exactly.

    Method: sweep `Detail(WORLD_SEED, EARTH_RADIUS_M).amplitude_m(p, elevation, weight,
    tectonic)` over the full cross product
        elevation : -12,000 .. +9,000 m in 211 even steps, plus -0.0, +0.0, +-1e-320,
                    +-5e-324, +-3000.0, +-350.0, +-200.0, +-1100.0, 1e300   (226 values)
        weight    : 0.0, 0.25, 0.5, 0.75, 1.0, 1e-18, 1.0 - 2**-52          (7 values)
        tectonic  : -20,000 .. 20,000 m in 41 even steps, plus -0.0, +0.0, +-1200.0
                                                                            (45 values)
    = 226 x 7 x 45 = 71,190 evaluations. `p` is the demo-coast anchor (`amplitude_m` never
    reads `point`, which is itself worth recording).
    """
    detail = Detail(WORLD_SEED, EARTH_RADIUS_M)
    point = Coast().at(0.0, 0.0)
    elevations = [-12000.0 + i * (21000.0 / 210.0) for i in range(211)]
    elevations += [-0.0, 0.0, 1e-320, -1e-320, 3000.0, -3000.0, 350.0, -350.0,
                   200.0, -200.0, 1100.0, -1100.0, 5e-324, -5e-324, 1e300]
    weights = [0.0, 0.25, 0.5, 0.75, 1.0, 1e-18, 1.0 - 2 ** -52]
    tectonics = [-20000.0 + i * (40000.0 / 40.0) for i in range(41)]
    tectonics += [-0.0, 0.0, 1200.0, -1200.0]
    lowest = math.inf
    lowest_at = None
    count = 0
    non_positive = 0
    for elevation in elevations:
        for weight in weights:
            for tectonic in tectonics:
                count += 1
                value = detail.amplitude_m(point, elevation, weight, tectonic)
                if value <= 0.0:
                    non_positive += 1
                if value < lowest:
                    lowest, lowest_at = value, (elevation, weight, tectonic)
    return {
        "evaluations": count,
        "non_positive": non_positive,
        "minimum": lowest,
        "minimum_at": lowest_at,
    }


def q2_authority_and_negative_zero():
    """
    Leg (ii) and the conjunction, measured on the real `Features.apply`.

    Method: exhaustive cross product over an adversarial feature configuration space,
    calling `Features.apply` unmodified, with the sample point placed exactly at the feature
    centre (so `weight` is exactly 1.0 -- confirmed by calling `weight_at`, not assumed) and
    at four offsets that give partial weights:

        pre-feature elevation : -0.0, +0.0, +-5e-324, +-1e-300, +-1e-8, +-0.5, +-3.0,
                                +-9.0, +-100.0, 2.999999999999999      (17 values)
        feature target_m      : the same 17 values
        compose               : RAISE, CARVE, SHAPE                     (3 values)
        offset from centre    : 0, 1, 100, 700, 1400 m                  (5 values)
    = 17 x 17 x 3 x 5 = 4,335 single-feature evaluations.

    Plus a two-feature sweep (17 x 17 x 3 = 867 configurations at the centre) whose first
    feature is a big-lift SHAPE that drives `authority` to 1.0 and whose second tries to
    land the result on `-0.0` -- the only route by which a `-0.0` result could co-occur
    with a fired guard.

    Records, for every evaluation: whether `shaped` is `-0.0`, the `authority` returned, and
    whether `amplitude_m(...) * (1.0 - authority) <= 0.0`.
    """
    detail = Detail(WORLD_SEED, EARTH_RADIUS_M)
    coast = Coast()
    centre = coast.at(0.0, 0.0)
    values = [-0.0, 0.0, 5e-324, -5e-324, 1e-300, -1e-300, 1e-8, -1e-8,
              0.5, -0.5, 3.0, -3.0, 9.0, -9.0, 100.0, -100.0, 2.999999999999999]
    offsets = [0.0, 1.0, 100.0, 700.0, 1400.0]
    composes = [RAISE, CARVE, SHAPE]

    def probe(features_list, pre, point):
        built = Features(features_list, EARTH_RADIUS_M)
        shaped, authority = built.apply(point, pre)
        amplitude = detail.amplitude_m(point, shaped, 0.5, 0.0)
        amplitude *= 1.0 - authority
        return shaped, authority, amplitude, amplitude <= 0.0

    single = 0
    weight_one_seen = 0
    negative_zero_shaped = []
    guard_fires = 0
    both = []
    for pre in values:
        for target in values:
            for compose in composes:
                for offset in offsets:
                    single += 1
                    point = coast.at(offset, 0.0)
                    feature = Feature(
                        kind="probe", at=centre, target_m=target,
                        length_m=3000.0, width_m=3000.0, bearing_deg=0.0,
                        compose=compose,
                    )
                    shaped, authority, amplitude, fired = probe([feature], pre, point)
                    if offset == 0.0:
                        built = Features([feature], EARTH_RADIUS_M)
                        if built.placed[0].weight_at(point) == 1.0:
                            weight_one_seen += 1
                    if fired:
                        guard_fires += 1
                    if is_negative_zero(shaped):
                        negative_zero_shaped.append(
                            (pre, target, compose, offset, authority, amplitude)
                        )
                        if fired:
                            both.append((pre, target, compose, offset, authority, amplitude))

    pairs = 0
    pair_negative_zero = []
    pair_both = []
    pair_guard_fires = 0
    big = Feature(kind="big", at=centre, target_m=-500.0, length_m=3000.0,
                  width_m=3000.0, bearing_deg=0.0, compose=SHAPE)
    for pre in values:
        for target in values:
            for compose in composes:
                pairs += 1
                second = Feature(kind="second", at=centre, target_m=target,
                                 length_m=3000.0, width_m=3000.0, bearing_deg=0.0,
                                 compose=compose)
                shaped, authority, amplitude, fired = probe([big, second], pre, centre)
                if fired:
                    pair_guard_fires += 1
                if is_negative_zero(shaped):
                    pair_negative_zero.append((pre, target, compose, authority, amplitude))
                    if fired:
                        pair_both.append((pre, target, compose, authority, amplitude))
    return {
        "single_evaluations": single,
        "weight_exactly_one_at_centre": weight_one_seen,
        "single_guard_fires": guard_fires,
        "single_negative_zero_shaped": len(negative_zero_shaped),
        "single_negative_zero_examples": negative_zero_shaped[:6],
        "single_both": both,
        "pair_evaluations": pairs,
        "pair_guard_fires": pair_guard_fires,
        "pair_negative_zero_shaped": len(pair_negative_zero),
        "pair_both": pair_both,
    }


def q2_authority_needs_three_metres():
    """
    The structural reason the conjunction is empty, measured: `authority == 1.0` requires
    `weight == 1.0` AND `_smooth(abs(lift) / SETTLE_M) == 1.0`, i.e. `abs(lift) >= 3.0`;
    while `shaped == -0.0` requires every *applying* feature to contribute exactly `-0.0`,
    i.e. `abs(lift) == 0.0`.

    Method: coarse sweep of `abs(lift)` over 100,001 even steps in [0, 6] for the first
    saturating value, then a ULP-resolution sweep starting 2,000 `math.nextafter` steps
    below 3.0 and walking up to 4,000 steps, for the exact first saturating float.
    """
    from worldbuilder.bathymetry.features import _smooth

    smallest = None
    for index in range(100_001):
        lift = index * (6.0 / 100_000.0)
        if _smooth(lift / SETTLE_M) == 1.0:
            smallest = lift
            break
    probe = SETTLE_M
    for _ in range(2000):
        probe = math.nextafter(probe, 0.0)
    ulp_first = None
    for _ in range(4000):
        if _smooth(probe / SETTLE_M) == 1.0:
            ulp_first = probe
            break
        probe = math.nextafter(probe, math.inf)
    return {
        "coarse_sweep_first_saturating_lift": smallest,
        "ulp_sweep_first_saturating_lift": ulp_first,
        "settle_m": SETTLE_M,
    }


def q2_grid_and_centre_census():
    """
    The blindness measurement the brief asks for. Same demo world, two populations:

      - the 625-point demo grid (`demo_grid()`)
      - the 25 demo feature centres (`placed.feature.at`)

    For each point: does the guard fire (`amplitude * (1 - authority) <= 0.0`), is
    `authority` exactly 1.0, and is `shaped` exactly `-0.0`?
    """
    surface = demo_surface()
    detail, shelf, features = surface.detail, surface.shelf, surface.features

    def census(points):
        fired = 0
        negative_zero = 0
        authority_one = 0
        for point in points:
            reading = shelf.evaluate(point)
            shaped, authority = features.apply(point, reading.elevation_m)
            amplitude = detail.amplitude_m(point, shaped, reading.weight, reading.tectonic_m)
            amplitude *= 1.0 - authority
            if amplitude <= 0.0:
                fired += 1
            if authority == 1.0:
                authority_one += 1
            if is_negative_zero(shaped):
                negative_zero += 1
        return {"points": len(points), "guard_fires": fired,
                "authority_exactly_one": authority_one,
                "negative_zero_shaped": negative_zero}

    centres = [placed.feature.at for placed in features.placed]
    return {"grid": census(demo_grid()), "centres": census(centres)}


def q2_shelf_can_produce_signed_zero():
    """
    The remaining route into `shaped == -0.0` on a real world: `shelf.evaluate` itself
    returning `+-0.0` with no feature applying. Census over the 625-point grid plus a
    1,000-point bisection hunt along the sign change of `shelf.elevation_m` across the
    demo shoreline (`Coast.at(offshore, 0.0)`, bisecting offshore in [-4000, 4000] m for
    60 iterations from 25 alongshore lines).
    """
    surface = demo_surface()
    shelf = surface.shelf
    coast = Coast()
    zeros = 0
    closest = math.inf
    closest_value = None
    for point in demo_grid():
        value = shelf.elevation_m(point)
        if value == 0.0:
            zeros += 1
        if abs(value) < closest:
            closest, closest_value = abs(value), value
    for line in range(25):
        along = -45000.0 + line * 3750.0
        low, high = -4000.0, 4000.0
        flow = shelf.elevation_m(coast.at(low, along))
        fhigh = shelf.elevation_m(coast.at(high, along))
        if (flow > 0.0) == (fhigh > 0.0):
            continue
        for _ in range(60):
            mid = 0.5 * (low + high)
            fmid = shelf.elevation_m(coast.at(mid, along))
            if fmid == 0.0:
                zeros += 1
                break
            if abs(fmid) < closest:
                closest, closest_value = abs(fmid), fmid
            if (fmid > 0.0) == (flow > 0.0):
                low, flow = mid, fmid
            else:
                high, fhigh = mid, fmid
    return {"exact_zeros_found": zeros,
            "closest_to_zero_abs": closest,
            "closest_value": closest_value}


def q2_negative_zero_needs_a_negative_zero_input():
    """
    Closes the last route: `Features.apply`'s accumulator can only *stay* at `-0.0`, never
    *arrive* at it. `result += weight * lift` yields `-0.0` only when both operands are
    `-0.0` (the addition lemma), and `weight * lift == -0.0` with `weight > 0` needs
    `lift == -0.0`, which forces `abs(lift) == 0.0` and therefore `authority == 0.0`.

    Method: 400,000 pseudo-random single-feature evaluations from `random.Random(11)`,
    every one with a pre-feature elevation that is **not** `-0.0`:
        pre     : `random.choice` over {+-10**uniform(-320, 3), +0.0, small integers}
        target  : the same distribution, plus `-0.0` and `+0.0`
        compose : RAISE / CARVE / SHAPE
        offset  : uniform(0, 2000) m from the centre, so weights span (0, 1]
    Counts how many produced `shaped == -0.0`.
    """
    coast = Coast()
    centre = coast.at(0.0, 0.0)
    rng = random.Random(11)
    composes = [RAISE, CARVE, SHAPE]

    def draw(allow_negative_zero):
        pick = rng.random()
        if pick < 0.08:
            return -0.0 if allow_negative_zero and pick < 0.04 else 0.0
        if pick < 0.16:
            return float(rng.randint(-9, 9))
        magnitude = 10.0 ** rng.uniform(-320.0, 3.0)
        return magnitude if rng.random() < 0.5 else -magnitude

    arrived = 0
    evaluations = 0
    for _ in range(400_000):
        pre = draw(False)
        if is_negative_zero(pre):
            continue
        evaluations += 1
        feature = Feature(kind="probe", at=centre, target_m=draw(True),
                          length_m=3000.0, width_m=3000.0, bearing_deg=0.0,
                          compose=rng.choice(composes))
        point = coast.at(rng.uniform(0.0, 2000.0), 0.0)
        shaped, _ = Features([feature], EARTH_RADIUS_M).apply(point, pre)
        if is_negative_zero(shaped):
            arrived += 1
    return {"evaluations": evaluations, "arrived_at_negative_zero": arrived}


# --------------------------------------------------------------------------------------
# Q3. The four reordering figures.
# --------------------------------------------------------------------------------------

def q3_features_before_shelf_readings():
    """
    "Features before the shelf" is not one experiment, and the spread between defensible
    readings is the figure worth reporting. Same 625-point grid, same world, canonical
    resolution. Four readings of the same sentence:

      R1  full pipeline, authority taken from the *reordered* `apply` (on `macro`)
      R2  full pipeline, authority taken from the *shipped* order (on `reading.elevation_m`)
      R3  structural only -- `abs(swapped - shaped)`, no detail stage at all
      R4  full pipeline, but detail amplitude still sized off the shipped `shaped`
    """
    surface = demo_surface()
    shelf, features, detail = surface.shelf, surface.features, surface.detail
    worst = {"R1_reordered_authority": 0.0, "R2_shipped_authority": 0.0,
             "R3_structural_only": 0.0, "R4_amplitude_from_shipped_shaped": 0.0}
    for point in demo_grid():
        reference = surface.elevation_m(point)
        reading = shelf.evaluate(point)
        shaped, authority = features.apply(point, reading.elevation_m)
        tectonic = reading.tectonic_m
        macro = shelf.land.base_elevation(point) + tectonic
        coastal = shelf.coastal(point)
        pre_shaped, pre_authority = features.apply(point, macro)
        if coastal is None or reading.weight <= 0.0:
            swapped = pre_shaped
        else:
            swapped = pre_shaped + reading.weight * (
                shelf.target_depth_m(coastal) - pre_shaped
            )

        base = detail.amplitude_m(point, swapped, reading.weight, tectonic)
        worst["R1_reordered_authority"] = max(
            worst["R1_reordered_authority"],
            abs(swapped + detail.offset_m(point, base * (1.0 - pre_authority), None)
                - reference),
        )
        worst["R2_shipped_authority"] = max(
            worst["R2_shipped_authority"],
            abs(swapped + detail.offset_m(point, base * (1.0 - authority), None)
                - reference),
        )
        worst["R3_structural_only"] = max(
            worst["R3_structural_only"], abs(swapped - shaped)
        )
        alt = detail.amplitude_m(point, shaped, reading.weight, tectonic)
        worst["R4_amplitude_from_shipped_shaped"] = max(
            worst["R4_amplitude_from_shipped_shaped"],
            abs(swapped + detail.offset_m(point, alt * (1.0 - pre_authority), None)
                - reference),
        )
    return {"points": 625, "worst": worst}



def q3_reorderings():
    """
    Population: the 625-point demo grid (`demo_grid()`), on the demo world
    `Surface(20260831, features=demo_region().features)` -- 22 plates, land fraction 0.29,
    25 features placed.

    Method: recompose `Surface.elevation_m` by hand from the *same* `Reading` and the same
    sub-objects, so nothing but the stage order differs; `resolution_m=None` throughout
    (canonical). Worst absolute delta against the shipped `Surface.elevation_m(point)`.
    """
    surface = demo_surface()
    shelf, features, detail = surface.shelf, surface.features, surface.detail
    grid = demo_grid()

    worst = {
        "features_before_shelf": 0.0,
        "detail_before_features": 0.0,
        "amplitude_from_pre_feature_ground": 0.0,
        "drop_authority_multiply": 0.0,
    }

    for point in grid:
        reference = surface.elevation_m(point)
        reading = shelf.evaluate(point)
        shaped, authority = features.apply(point, reading.elevation_m)
        tectonic = reading.tectonic_m

        # (a) features before shelf: stamp onto `macro`, then blend towards the shelf
        # target with the same weight (`Shelf.weight` never reads `macro`).
        macro = shelf.land.base_elevation(point) + tectonic
        coastal = shelf.coastal(point)
        pre_shaped, pre_authority = features.apply(point, macro)
        if coastal is None or reading.weight <= 0.0:
            swapped = pre_shaped
        else:
            swapped = pre_shaped + reading.weight * (
                shelf.target_depth_m(coastal) - pre_shaped
            )
        amplitude = detail.amplitude_m(point, swapped, reading.weight, tectonic)
        amplitude *= 1.0 - pre_authority
        worst["features_before_shelf"] = max(
            worst["features_before_shelf"],
            abs(swapped + detail.offset_m(point, amplitude, None) - reference),
        )

        # (b) detail before features: roughen the shelf ground, then stamp features on it.
        amplitude = detail.amplitude_m(point, reading.elevation_m, reading.weight, tectonic)
        rough = reading.elevation_m + detail.offset_m(point, amplitude, None)
        worst["detail_before_features"] = max(
            worst["detail_before_features"],
            abs(features.apply(point, rough)[0] - reference),
        )

        # (c) amplitude sized off pre-feature ground: `shaped` -> `reading.elevation_m`.
        amplitude = detail.amplitude_m(point, reading.elevation_m, reading.weight, tectonic)
        amplitude *= 1.0 - authority
        worst["amplitude_from_pre_feature_ground"] = max(
            worst["amplitude_from_pre_feature_ground"],
            abs(shaped + detail.offset_m(point, amplitude, None) - reference),
        )

        # (d) drop `amplitude *= 1.0 - authority` (surface.py:112).
        amplitude = detail.amplitude_m(point, shaped, reading.weight, tectonic)
        worst["drop_authority_multiply"] = max(
            worst["drop_authority_multiply"],
            abs(shaped + detail.offset_m(point, amplitude, None) - reference),
        )

    return {"points": len(grid), "worst": worst}


def q3_reorderings_at_centres():
    """
    The same four reorderings on the population the grid is blind to: the 25 demo feature
    centres. Same world, same method, `resolution_m=None`.
    """
    surface = demo_surface()
    shelf, features, detail = surface.shelf, surface.features, surface.detail
    centres = [placed.feature.at for placed in features.placed]

    worst = {
        "features_before_shelf": 0.0,
        "detail_before_features": 0.0,
        "amplitude_from_pre_feature_ground": 0.0,
        "drop_authority_multiply": 0.0,
    }
    for point in centres:
        reference = surface.elevation_m(point)
        reading = shelf.evaluate(point)
        shaped, authority = features.apply(point, reading.elevation_m)
        tectonic = reading.tectonic_m
        macro = shelf.land.base_elevation(point) + tectonic
        coastal = shelf.coastal(point)
        pre_shaped, pre_authority = features.apply(point, macro)
        if coastal is None or reading.weight <= 0.0:
            swapped = pre_shaped
        else:
            swapped = pre_shaped + reading.weight * (
                shelf.target_depth_m(coastal) - pre_shaped
            )
        amplitude = detail.amplitude_m(point, swapped, reading.weight, tectonic)
        amplitude *= 1.0 - pre_authority
        worst["features_before_shelf"] = max(
            worst["features_before_shelf"],
            abs(swapped + detail.offset_m(point, amplitude, None) - reference),
        )

        amplitude = detail.amplitude_m(point, reading.elevation_m, reading.weight, tectonic)
        rough = reading.elevation_m + detail.offset_m(point, amplitude, None)
        worst["detail_before_features"] = max(
            worst["detail_before_features"],
            abs(features.apply(point, rough)[0] - reference),
        )

        amplitude = detail.amplitude_m(point, reading.elevation_m, reading.weight, tectonic)
        amplitude *= 1.0 - authority
        worst["amplitude_from_pre_feature_ground"] = max(
            worst["amplitude_from_pre_feature_ground"],
            abs(shaped + detail.offset_m(point, amplitude, None) - reference),
        )

        amplitude = detail.amplitude_m(point, shaped, reading.weight, tectonic)
        worst["drop_authority_multiply"] = max(
            worst["drop_authority_multiply"],
            abs(shaped + detail.offset_m(point, amplitude, None) - reference),
        )
    return {"points": len(centres), "worst": worst}


# --------------------------------------------------------------------------------------
# Q4. The two exact invariants.
# --------------------------------------------------------------------------------------

def q4_invariants():
    """
    Populations, both named:

      A. the 625-point demo grid
      B. the 25 demo feature centres  (the axis the grid is blind on)

    Invariant 1 -- with **no** features, `Surface(20260831).structural_m(p)` is bit-identical
    to `surface.shelf.elevation_m(p)`.

    Invariant 2 -- on the demo world (25 features),
    `elevation_m(p) == structural_m(p) + detail.offset_m(p, amplitude, resolution_m)` with
    `amplitude` recomputed exactly as `surface.py:108-112` does it. Checked at
    `resolution_m` in {None, 500.0, 5000.0}.

    A third column, `shaped_is_structural_m`, pins the substitution the invariant relies on:
    the `shaped` of `elevation_m` line 107 is bit-identical to `structural_m(p)`.

    Everything compared on `struct.pack` bit patterns, not `==`, so signed zeros cannot hide.
    """
    bare = bare_surface()
    surface = demo_surface()
    grid = demo_grid()
    centres = [placed.feature.at for placed in surface.features.placed]

    def check(points):
        one_ok = two_ok = two_total = shaped_ok = 0
        for point in points:
            if bits(bare.structural_m(point)) == bits(bare.shelf.elevation_m(point)):
                one_ok += 1
            reading = surface.shelf.evaluate(point)
            shaped, authority = surface.features.apply(point, reading.elevation_m)
            if bits(shaped) == bits(surface.structural_m(point)):
                shaped_ok += 1
            amplitude_base = surface.detail.amplitude_m(
                point, shaped, reading.weight, reading.tectonic_m
            )
            for resolution in (None, 500.0, 5000.0):
                two_total += 1
                amplitude = amplitude_base * (1.0 - authority)
                recomposed = shaped + surface.detail.offset_m(point, amplitude, resolution)
                if bits(recomposed) == bits(surface.elevation_m(point, resolution)):
                    two_ok += 1
        return {
            "points": len(points),
            "invariant_1_bit_identical": one_ok,
            "shaped_is_structural_m": shaped_ok,
            "invariant_2_checks": two_total,
            "invariant_2_bit_identical": two_ok,
        }

    return {"grid": check(grid), "centres": check(centres)}


# --------------------------------------------------------------------------------------
# pytest gates
# --------------------------------------------------------------------------------------

def test_seed_cast_is_faithful_for_the_noise_backed_constructors():
    result = q1_lattice_and_noise()
    assert result["mismatches"] == []
    assert result["seeds"] == 2049


def test_seed_cast_is_not_faithful_for_plates_for():
    result = q1_plates_must_not_be_cast()
    assert result["masked_differed"] == result["sample"]
    if ENGINE is not None:
        # The i64 path tracks the Python to libm noise; the masked path does not.
        assert result["worst_rust_i64_vs_python_negative_seed"] < 1e-12
        assert result["worst_rust_i64_vs_python_masked_seed"] > 1e-3


def test_amplitude_m_is_strictly_positive():
    result = q2_amplitude_floor()
    assert result["non_positive"] == 0
    assert result["minimum"] > 0.0


def test_negative_zero_shaped_never_coincides_with_a_fired_guard():
    result = q2_authority_and_negative_zero()
    assert result["single_negative_zero_shaped"] > 0, "the -0.0 case must be constructible"
    assert result["single_guard_fires"] > 0, "the guard must actually fire somewhere"
    assert result["single_both"] == []
    assert result["pair_both"] == []


def test_exact_invariants():
    result = q4_invariants()
    for population in ("grid", "centres"):
        block = result[population]
        assert block["invariant_1_bit_identical"] == block["points"]
        assert block["shaped_is_structural_m"] == block["points"]
        assert block["invariant_2_bit_identical"] == block["invariant_2_checks"]


def main():
    print("HOST: " + host())
    print("ENGINE: " + (ENGINE.version() if ENGINE else "not built"))
    print()
    print("== Q1 lattice/noise ==");            print(q1_lattice_and_noise())
    print("== Q1 unmasked intermediate ==");    print(q1_unmasked_intermediate_is_negative())
    print("== Q1 cross-language ==");           print(q1_cross_language())
    print("== Q1 plates_for counter ==");       print(q1_plates_must_not_be_cast())
    print()
    print("== Q2 addition lemma ==");           print(q2_addition_lemma())
    print("== Q2 amplitude floor ==");          print(q2_amplitude_floor())
    print("== Q2 authority saturation ==");     print(q2_authority_needs_three_metres())
    print("== Q2 constructed -0.0 ==");         print(q2_authority_and_negative_zero())
    print("== Q2 grid vs centre census ==");    print(q2_grid_and_centre_census())
    print("== Q2 shelf signed zero hunt ==");   print(q2_shelf_can_produce_signed_zero())
    print("== Q2 -0.0 needs a -0.0 input ==");  print(q2_negative_zero_needs_a_negative_zero_input())
    print()
    print("== Q3 features-before-shelf readings =="); print(q3_features_before_shelf_readings())
    print("== Q3 reorderings, 625 grid ==");    print(q3_reorderings())
    print("== Q3 reorderings, 25 centres =="); print(q3_reorderings_at_centres())
    print()
    print("== Q4 invariants ==");               print(q4_invariants())


if __name__ == "__main__":
    main()
