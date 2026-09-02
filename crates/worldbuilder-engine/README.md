# worldbuilder-engine

The generator core. One implementation, compiled twice: natively for Evennia and maritime
through Python bindings, and to WebAssembly for the browser studio.

Slice 0 measured that those two targets agree bit-for-bit over 5,000,000 samples, with a
negative control proving the comparison could detect a one-bit difference. That is the
foundation this crate is built on; see `spikes/0-bit-equality/README.md`.

## What is here so far

    src/detmath.rs   the only place a transcendental is called
    src/vectors.rs   Vec3
    src/sphere.rs    SpherePoint
    src/noise.rs     Noise: 64-bit lattice hash, trilinear sample, fBm
    src/tangent.rs   TangentFrame: at, local_to_sphere, sphere_to_local
    src/bindings.rs  the PyO3 surface, conversion only

The Python in `worldbuilder/` is still the reference implementation and is unchanged.
Nothing has been deleted, and the engine is additive until conformance is established for
every module.

## Two rules that are not style

**No std maths.** Everything transcendental routes through `detmath`, backed by the
pure-Rust `libm`. `tests/no_std_math.rs` fails the build if a std float method appears
outside that file, and the guard has been observed to fail, not merely to pass.

**Floor, never cast.** `worldbuilder/terrain/noise.py` derives lattice cells with
`int(x // 1)`, which floors toward negative infinity; Rust's `as i64` truncates toward
zero. For any negative coordinate they select a different cell, silently. Use
`detmath::floor`. `tests/no_std_math.rs` bans `as i64`/`as i32`/`as u64`/`as u32` outright,
with a `// cast-ok: <reason>` escape hatch for casts that are genuinely integer-to-integer
and not a float truncation -- so this is mechanised the same way the no-std-maths rule is,
not merely documented.

## Building it

    cd crates/worldbuilder-engine
    python -m maturin develop --release

## Conformance

The suite at `tests/test_conformance.py` compares this crate against the Python
implementation it ports, and holds it to two different contracts depending on what is in
a function's path.

**Strict, bit-for-bit, where no transcendental is involved.** All of `Vec3` -- length,
cross, and normalised, including the zero-vector case, where Rust returns `None` and
Python raises `ValueError` -- agrees exactly across a 20,000-sample corpus (hashed, not
gridded, plus the poles and the axes pinned in by hand). No tolerance is used, because a
tolerance would let a coastline move by a metre and call it equal.

**Bounded to 4 ULP, where a transcendental is in the path.** `sphere_from_latlon`,
`sphere_to_latlon`, `sphere_angle_to`, and `sphere_distance_to` all route through `sin`,
`cos`, or `atan2`. Bit-identity with the Python here is not achievable and is not being
pursued: CPython's `math.sin`, `cos`, and `atan2` delegate to the platform C library --
UCRT on Windows, glibc on Linux -- while this engine deliberately uses the pure-Rust
`libm` crate instead, so that its native and WebAssembly builds agree bit-for-bit with
each other. That native/WASM agreement is what slice 0 measured over 5,000,000 samples,
and the whole studio architecture depends on it. The two goals are mutually exclusive:
matching CPython exactly would mean taking the platform libm into Rust and giving up
native/WASM equality.

The bound is measured, not assumed. Across a dense sweep the worst observed divergence
is 3 ULP, at lat=-53 lon=-45: `angle_to` worst 1 ULP, `from_latlon.z` 1, `from_latlon.x`
2, `to_latlon` lat and lon 2, `from_latlon.y` 3 (a product of two 1-ULP values).
Isolating `sin` alone, only 2 of 181 integer latitudes differ at all. 4 ULP leaves one
ULP of headroom over the worst case observed. A bound this tight still catches
structural errors -- substituting `acos` for `atan2` in `angle_to`, or reordering a
cross product, would diverge by orders of magnitude more, not by one or two extra ULP.

Every per-function figure quoted above is defended by a test, not just quoted in this
file: `test_transcendental_divergence_stays_within_its_measured_bound_for_every_sphere_function`
sweeps `from_latlon`, `to_latlon`, `angle_to`, and `distance_to` independently, tracks a
worst-ULP-per-function, asserts zero unmeasurable comparisons (NaN, infinity, or a
sign-straddle) for each, and names the function in its failure message so a regression
says which one moved.

**Strict, bit-for-bit, for `Noise` too -- no ULP bound anywhere in it.** `noise_at` and
`noise_fbm` are held to the strict contract in full, the same one `Vec3` gets, not the
4-ULP bound `sphere.rs` needs. That is not an oversight: `Noise` contains no
transcendentals at all. Its lattice hash is 64-bit integer arithmetic, cell selection is
a floor, and interpolation is a smoothstep built from multiplies and adds -- nothing that
routes through `sin`, `cos`, or any other platform-dependent function. There is no
libm-versus-libm discrepancy for it to absorb, so no tolerance is warranted, and if a
future change to this module ever seems to need one, that is a sign something in the port
is wrong, not a sign the standard is wrong. The noise conformance tests make roughly
15,075 individual bit-for-bit comparisons.

**The cache is gone, deliberately.** `worldbuilder/terrain/noise.py` memoises each cell's
eight lattice corners in a dict, because its own comment records 2.9 million such calls in
a single chart redraw -- at that volume, the cost of a Python-level function call exceeds
the cost of the arithmetic it would otherwise avoid. `noise.rs` does not carry that cache
forward. The memoised value is a pure function of three integers and a seed; recomputing
it returns exactly what the cache would have returned, and Rust's per-call cost does not
carry the same penalty that motivated the Python's dict in the first place. Dropping it
also makes `Noise` immutable and `Sync`, which both the WebAssembly build and any future
parallel bake want and which a cache would have complicated.

**What this means for generator identity.** The Rust core is not a bit-exact
reimplementation of the Python; it is a new generator version under VERSION-001. Mark
1's measured world figures describe the Python generator and will need re-measuring once
the port completes.

**Open question, not yet measured.** Because CPython inherits the platform's libm, the
existing Python generator very likely already produces subtly different worlds on
Windows versus Linux. This has not been measured -- it would require running the Python
suite on Linux and comparing against a Windows run -- so treat it as a hypothesis, not a
finding.

**`TangentFrame` is the first module held to both contracts at once.** `at` -- the frame
constructor, with its pole fallback chain -- is held strictly: its only transcendental is
`sqrt`, which IEEE-754 requires to be correctly rounded, and it agreed exactly across 446
origins and 4,014 component comparisons, including both poles. `local_to_sphere` and
`sphere_to_local` route through `hypot`, `cos`, `sin`, and `atan2`, so they are held to the
same 4-ULP bound as `sphere.rs`, for the same libm-versus-libm reason; the worst observed
divergence across the sweep is 3 ULP, on `sphere_to_local`. Altogether the frame
conformance section makes roughly 13,390 individual comparisons.

The point this module makes concrete: **the contract split is per code path, not per
module.** A module is not itself "strict" or "bounded" -- a function is, depending on
whether a platform-dependent transcendental sits in its path. `TangentFrame` has one
strict function and two bounded ones side by side in the same file. Later modules should
be classified the same way, function by function, rather than assigned a single label for
the whole file.

**What a passing strict test does and does not prove.** It is strong empirical evidence,
not a structural proof. IEEE-754 correctly-rounds each individual `+`, `-`, `*`, `/`, and
`sqrt`, but bit-identity also requires the port to perform the same sequence of elementary
operations in the same order, because floating-point addition is not associative --
`(a*b) + (c*d)` and `(c*d) + (a*b)` can round differently even though both are individually
correct. A second risk is FMA contraction: fusing a multiply and an add into one rounding
step on one side but not the other. Both risks are already covered here rather than left
for a reader to wonder about: the transcribe-don't-rederive rule (below) fixes operation
order to match the Python's, and the determinism guard bans `.mul_add(` and
`f64::mul_add(` outright, so explicit fusion cannot enter the codebase; rustc does not
auto-contract without an explicit `mul_add` call or a target-feature flag. The practical
consequence for future ports: strictness holds only as long as a function's operation
order stays a literal transcription, so "simplifying" the arithmetic in a strict-contract
function is exactly how it would silently stop being bit-exact.

**The geometry layer is now complete.** `Vec3`, `SpherePoint`, and `TangentFrame` are all
ported, which unblocks `Continentality`, whose `gradient` method walks geodesics through a
tangent frame.

Skips the whole file if `worldbuilder_engine` is not built, so the Python suite still runs
on a machine with no Rust -- except when `WORLDBUILDER_REQUIRE_ENGINE` is set to anything
non-empty, in which case a missing or stale engine fails the session instead of skipping
it. Set that variable in CI, where a silent skip would report green while comparing
nothing.

Run it with:

    python -m pytest tests/test_conformance.py -v

261 Python tests and 46 crate tests pass in the full suite. The harness includes a test
asserting that `same` can distinguish a one-bit difference and a test asserting that
`close_enough` rejects a difference past the ULP bound, because a conformance suite that
cannot fail proves nothing.

## Constants transcribed from Python: a rule learned the hard way

The noise port's seed multiplier -- the FNV-1a 64-bit prime, `0x100000001B3` in the
Python -- was transcribed into the plan as `0x0000_0001_0000_01B3`. Grouping the hex
digits into underscored nibbles moved a digit and silently produced a different number:
4,294,967,731 instead of 1,099,511,628,211. An implementer checked that constant against
the plan and confirmed it was right. A reviewer checked it independently and confirmed it
was right. Both were looking at the wrong number and agreed with each other about it.
Only running the conformance suite against the Python -- comparing actual output, not the
literal -- caught the discrepancy.

Two rules follow from this, for every future module port:

1. **Transcribe constants without underscore separators, character-identical to their
   Python source.** `0x100000001B3`, not `0x0000_0001_0000_01B3`. A separator that groups
   digits differently than the source is itself a transcription error waiting to happen,
   and a literal that matches the Python character-for-character can be compared by eye
   without doing arithmetic in your head.
2. **Constants are verified by conformance, never by review -- but only for the path the
   corpus actually reaches.** Do not ask a reviewer to certify a hex or decimal literal by
   reading it next to another one -- two people did exactly that here and both blessed the
   wrong value, because eyeballing a long constant is a task human review is bad at, not a
   matter of carelessness. The conformance harness compares computed output against the
   Python end to end, which exercises every constant in the path whether or not anyone
   thought to check it by hand. That is what caught this one after two reviews had already
   passed it. `Noise`'s eight constants all sit on its one unconditional hot path, so any
   corpus that calls `at` or `fbm` at all reaches every one of them -- that is what makes
   conformance a complete substitute for review here. That guarantee does not carry over to
   a constant reached only conditionally -- a per-biome coefficient, a threshold crossed
   only above some latitude, one row of a lookup table -- because conformance only verifies
   what the corpus happens to hit. For a constant like that, either show that the corpus
   exercises the branch it lives on, or give it its own test pinning it against the
   Python's value, the way `the_seed_multiplier_is_the_fnv_prime` now pins this module's
   multiplier by observing `Noise::new`'s effect rather than restating the literal. This
   matters most for the modules still to be ported -- continentality, tectonics, and the
   shelf all carry far more constants than this one, and far more of them sit behind a
   branch.
