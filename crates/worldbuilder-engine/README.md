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
    src/continentality.rs  Continentality: at, calibration, above_shore, base_elevation, gradient
    src/plates.rs    Plate, PlateSet: the bisector table and the nearest-two Voronoi lookup
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
`sqrt`, which IEEE-754 requires to be correctly rounded, and it agreed exactly across 459
origins and 4,131 component comparisons, including both poles. (`frame_origins()` sweeps
`range(-85, 86, 5)` latitudes -- 35 of them, not 34 -- times 13 longitudes, plus 4 named
points: 35 x 13 + 4 = 459 origins, each contributing 9 component comparisons: 459 x 9 =
4,131.) `local_to_sphere` and `sphere_to_local` route through `hypot`, `cos`, `sin`, and
`atan2`, so they are held to the same 4-ULP bound as `sphere.rs`, for the same
libm-versus-libm reason; the worst observed divergence across the sweep is 3 ULP, on
`sphere_to_local` -- while `local_to_sphere` came back exact, 0 ULP across the entire
sweep, even though it is held to the bounded contract too and not the strict one: `cos`,
`sin`, and `hypot` all sit in its path, and it happened to agree with the Python bit for
bit anyway. That is a stronger result than the 3-ULP figure alone suggests, and it is the
point this module makes concrete twice over: a function is bounded because a
platform-dependent transcendental sits in its path, not because it was observed to
diverge. Altogether the frame conformance section makes 15,622 individual comparisons,
counted by instrumenting every `same()`/`close_enough()` call the section's tests make:
4,131 from `at()`, 6 from the pole-stability check, 6,885 from `local_to_sphere`, 4,590
from `sphere_to_local` (including the origin/antipode degenerate branch), and 10 from the
extended round-trip table.

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

**`Continentality` splits one strict function from four bounded ones.** `at` is held
strictly: it is `Noise::fbm` wired straight through and nothing else, so it inherits
`Noise`'s no-transcendental strict contract rather than earning a bound of its own, and it
agrees with the Python exactly, 0 ULP, across the corpus. `calibration`, `above_shore`,
`base_elevation`, and `gradient` are all bounded, because each puts a transcendental in
its path that `at` does not: `calibration` runs a Fibonacci spiral through `cos`, `sin`,
and `sqrt` to place its sample points; `above_shore` reads the stored calibration, so it
inherits that bound; `gradient` reads neither `shore` nor `spread` and is bounded solely
because it walks a `TangentFrame`; `base_elevation` calls
`powf` to shape the curve between shore and each extreme. Measured results: `at` exact at
0 ULP; `above_shore` 0 ULP; `gradient` 0 ULP; `base_elevation` 2 ULP, from `powf`;
calibration 71 of 72 sampled (seed, land_fraction) pairs exact, with one -- `shore` at
seed `2**63 - 1`, land_fraction 0.95 -- at 2 ULP.

**The calibration is close, not exact -- say so plainly.** An earlier report in this slice
described the calibration as an exact match; that was a different claim from what the
conformance test actually asserts, and the measurement above is what settles it: 71 of 72
sampled pairs land at 0 ULP, one lands at 2 ULP, and the module as a whole is bounded, not
strict. This is worth stating without hedging because it is the second time in this
project that a "matched" has been recorded where a number belonged -- the first cost a
session tracking down which of two reviewers' blessed constants was wrong (see the FNV
prime story below). Recording the actual figure here instead of the flattering rounding is
the cheap way to not pay for that mistake a third time.

**The first module with generated-and-stored state, and the first whose output depends on
a sort.** Every earlier ported function computes its result directly from its inputs.
`Continentality::calibration` instead draws `CALIBRATION_SAMPLES` (4,000) points along a
Fibonacci spiral, evaluates `at` on each, sorts the results, and reads off the `shore` and
`spread` percentiles the rest of the module depends on. A few-ULP difference between the
Rust and Python spiral values is expected and harmless on its own -- but sorting means a
value close enough to a neighbour could in principle land on the other side of it, picking
a different array slot entirely, which would be a reordered sort masquerading as arithmetic
drift. `test_continentality_calibration_agreement_is_far_tighter_than_the_sort_gap`
guards against exactly that: it reproduces the spiral independently in Python, measures the
gap to the nearest neighbour at the `shore` and `spread` indices, and asserts the observed
Rust/Python difference is far smaller than that gap -- checked both at the pair that agreed
exactly (seed 12345, land_fraction 0.29) and at the one pair the wider sweep actually found
diverging (seed `2**63 - 1`, land_fraction 0.95, where `shore` differs by 2 ULP). At that
divergent pair the measured difference is on the order of 1e-16 against a neighbour gap of
roughly 2e-4 -- more than eleven orders of magnitude apart. The smallest gap found anywhere
in the full sorted sample, on the default seed, is 4.6e-9 -- itself about nine orders of
magnitude above a ULP at these magnitudes -- which is what protects the sort: a future
divergence anywhere near that size would mean a sample crossed a neighbour and the sort
picked a different index, not that a transcendental rounded differently.

**Calibration's cost.** Because it is computed once and cached rather than on every
lookup, spending more per call than `at` is affordable: the calibration runs in about
2.9 ms in Rust against roughly 30 ms for the Python -- both driven by the same 4,000-point
spiral and sort, so the gap is call-overhead and interpreter cost, not a different
algorithm.

Skips the whole file if `worldbuilder_engine` is not built, so the Python suite still runs
on a machine with no Rust -- except when `WORLDBUILDER_REQUIRE_ENGINE` is set to anything
non-empty, in which case a missing or stale engine fails the session instead of skipping
it. Set that variable in CI, where a silent skip would report green while comparing
nothing.

Run it with:

    python -m pytest tests/test_conformance.py -v

274 Python tests and 72 crate tests pass in the full suite. The harness includes a test
asserting that `same` can distinguish a one-bit difference and a test asserting that
`close_enough` rejects a difference past the ULP bound, because a conformance suite that
cannot fail proves nothing.

**`Plate` and `PlateSet` are entirely strict, and unusually so: this is the first ported
module with no transcendental anywhere in it** (the `sqrt` inside `length()` does route
through `detmath`, but `sqrt` is algebraic, not transcendental, and IEEE-754 requires it
correctly rounded, so it costs no bound). `nearest_two` compares seeds by dot product
rather than by angle, because for unit vectors a larger dot product *is* a smaller angle --
converting to distances would only be undone by the comparison, at the cost of two dozen
transcendental calls per sample to sort numbers that were already in order. Building the
bisector table is a subtraction, a `length()`, and a `normalised()`, all IEEE-754-exact or
correctly-rounded. So there is no ULP bound anywhere in this slice, and none was needed:
this and the noise module are the two ports so far where "strict" needed no defending.

The bisector table is the entire stored geometry of a planet's tectonics. Points equidistant
from seeds A and B satisfy `dot(P, A) == dot(P, B)`, which rearranges to
`dot(P, A - B) == 0`, so the margin between two plates is a great circle whose plane normal
is `normalise(A - B)`. A couple of dozen plates makes a few hundred such vectors, and that
table is what the next slice's margin queries will read.

The Python's duplicated component-triple table -- the same geometry kept twice, because a
Python method call costs more than the three multiplies inside `Vec3.dot`, and profiling
found ninety-nine such calls per terrain sample -- is not ported. In Rust the field access is
free, so the second copy buys nothing and one representation cannot fall out of step with
itself; this is the same call already made when `Noise`'s corner cache was dropped, above.

Two IEEE-754 properties were established by proof during review, not just observed, because
the next person writing a test here will need them. `normalise(B - A)` is the exact
component-wise negation of `normalise(A - B)` for any non-zero component, for any seed pair:
subtraction is exactly negated, `length()` squares away the sign so both directions share one
scale factor, and multiplying an exactly-negated component by that same positive scale is
again exact. But a component where the seeds are equal gives `+0.0` in both directions, never
`-0.0`, because each direction computes its own subtraction rather than negating the other --
asserting a sign flip there would assert something untrue, and a test in this slice did
exactly that before it was corrected.

`nearest_two`'s tie rule matters for the same reason. Both comparisons are strict `>`, so a
tie keeps the earlier plate, which is what makes the answer independent of iteration order
rather than an accident of it. Review confirmed the property holds for second place as well
as first: `best` is always the earliest plate holding the running maximum, every demotion
moves that same `best` into `second` rather than the incoming plate, and the `else if`
installs the current plate only on a strict `>`. This matters because the margin machinery
the next slice adds consumes second place, not just first.

**A limitation of the plate bindings that the next slice must fix.** `bindings.rs` rebuilds a
`PlateSet` from seed components alone, fabricating `pole = seed` and `rate = 0.0` for every
plate. That is provably inert today, because `PlateSet::new` and `nearest_two` read only the
seed. It will not stay inert: `Margin` carries whole `Plate` values, so once `margin_at` and
`margin_normal` are exposed through the same reconstruction they would compare placeholder
against placeholder on the pole and rate fields and pass trivially -- false confidence, not
conformance. The binding contract must change before then to carry real, independently
varying poles and rates from the Python harness.

## Margins

**The binding fix is real.** `plateset_from_parts` now takes three flat lists -- seeds,
Euler poles, and rates -- and builds a `Plate` with real, independently-varying values in
every field, instead of fabricating `pole = seed` and `rate = 0.0` as it did through the
previous slice. That part of the limitation above is fixed, plainly: the fixture data feeding
the tests below actually varies pole and rate per plate, not just seed.

**But this slice's tests do not, and cannot, exercise a fabrication regression, and the
prior report claiming otherwise was wrong.** `margin_at`, `margin_normal`, and `flattened`
never read `euler_pole` or `rate_rad_per_myr` -- only `Plate::angular_velocity()` does, and
no binding in this slice calls it. This was not reasoned out; it was proven by mutation
during review: `plateset_from_parts` was edited back to `pole = seed`, `rate = 0.0`, the
crate rebuilt, and all 44 conformance tests still passed. The doc comment on
`plateset_from_parts` in `bindings.rs` says this plainly now, and this section is written to
match it rather than to repeat the earlier, disproven claim. **The fabrication guard belongs
to the kinematics slice**, where `angular_velocity` genuinely reads `euler_pole` and
`rate_rad_per_myr` and a fabricated value would actually be caught -- it must not be assumed
to exist before then. A claim that carrying real values through a struct field is itself a
regression test was believed and repeated across two slices before this mutation disproved
it; treat "the fields are populated" and "something reads them" as separate facts from now
on, here and in any future binding.

**`margin_at` splits across both conformance contracts, and the split is a property, not
luck.** Neighbour selection -- which plate is "across" -- is a minimum over bisector sines,
each computed from a dot product and an `abs`, with no transcendental anywhere in the
comparison. A discrete choice made on exactly-computed values compares as exact integers
across languages, so the *identity* of the chosen neighbour is held to the strict contract
and agrees exactly. Only the last step, converting that sine to a distance in metres, calls
`asin`, so only the distance is bounded to the 4-ULP contract everything else in this file
built. One function, two contracts, because the split runs per operation, not per function --
the same lesson `TangentFrame` recorded above, one level further in.

**Why the minimum is taken over every bisector, not just the nearest one.** `lookup.py`
records that an earlier version measured only the second-nearest plate's bisector, and the
answer jumped by five hundred kilometres. The numerator of the sine -- the point's distance
from a candidate plane -- is continuous as the point moves, but which bisector is
second-nearest is not: it can hand off from one plane to a completely unrelated one between
two adjacent points, and the distance measured off the new plane owes nothing to the old
one. Taking the minimum over every bisector fixes this because a minimum of continuous
functions is itself continuous, even though the arg-min -- which function attained it -- can
still jump. `lookup.py` attributes four separate bugs in this module to the same root cause:
a hard decision taken on a continuous quantity. This is the second of the four; `margins_within`,
not ported in this slice, carries the other three (the arg-min flip that its own docstring
opens with -- picking one margin is not continuous, even when its distance is, and cost five
hundred metres of cliff; the phantom-bisector test, where a bisector belongs to two plates
that are not actually the nearest pair anywhere near it; and the shadow weight that replaced
a boolean, one bug rather than two, since the fade *is* the fix for the hard decision), which
is exactly why it gets its own slice rather than riding along with this one.

**The minimum sine gap, measured rather than assumed, for the third time in this crate.**
Across the combined corpus used for margin conformance -- the pinned poles and meridian
points, roughly 3,000 pseudo-random points, and 1,500 points deliberately built near a
bisector midpoint and nudged off it, the case most likely to produce a near-tie -- the
smallest observed gap between the two closest bisector sines at any point is
`1.3689896544988311e-05` (about 1.37e-5), at the sphere point `(-0.0162, -0.6887, -0.7248)`.
A ULP at the magnitude these sines take (0.01 to 1.0) is on the order of 1e-16 to 1e-18, so
the measured gap is roughly eleven orders of magnitude wider than rounding error, in the
corpus that specifically goes looking for a close call. The neighbour selection is discrete,
but it is not fragile. This is the third slice in this crate to measure a safety margin
like this instead of assuming one: the sphere-function ULP bound above ("The bound is
measured, not assumed") is the first, the `Continentality::calibration` sort-gap check is
the second, and this is the third.

**A deliberate deviation from the Python, recorded rather than hidden.** The bisector table
is built by loop position on both sides of it in Rust: `PlateSet::new` fills row and column
by position, and both `margin_at` and `margin_normal` address it by position on both axes.
The Python is not internally consistent about which key it uses: `margin_at` addresses the
table's row by `nearest.index` and its column by position (via `zip(self.plates, ...)`),
while `margin_normal` addresses both row and column by `.index`. The two Python functions
only ever agree with each other, and with the Rust, because `generation.py` assigns
`index=index for index in range(count)` -- position and index are the same number for every
plate the corpus builds. For a hand-built `PlateSet` where a plate's `index` does not match
its position in the list, the Python's own two functions would disagree with each other, and
the Rust -- consistently by-position everywhere -- would disagree with both. That is a real
difference in behaviour outside the regime this corpus exercises, not a bug being smoothed
over: it is written down here, in the doc comments on `margin_at` and `margin_normal` in
`plates.rs`, and in the test file's comment warning against ever building a corpus with
index != position, so that nobody "strengthens" the suite later by shuffling indices and
reports a divergence that is really Python's own inconsistency.

Run the margin tests together with the rest of `test_conformance.py` the same way as
before; 44 tests pass in that file (34 from the earlier `Plate`/`PlateSet` sections plus 10
new for margins), 284 in the full Python suite, and 74 crate tests plus the 6-test
`no_std_math` guard in the Rust suite -- all verified by running them, not carried over from
an earlier report.

## `margins_within`: the first membership decision downstream of a transcendental

Every earlier ported function could be asked "is a transcendental in this path?" and get
a clean yes-or-no that settled which conformance contract applied. `margins_within`
(`worldbuilder/plates/lookup.py:212-283`) breaks that pattern: it decides *which margins
it returns* -- not merely how precisely it states a distance -- by comparing an
exactly-reproducible dot product against `limit = sin(min(pi/2, range_m/radius_m))`. A
one-ULP disagreement in `limit` would not shave a low bit off a number; it would change
the *length* of the returned list, which every caller that sums margin contributions
depends on.

**What Task 1 measured, and why that is not the reason membership is safe.** `limit` came
back bit-identical between CPython and this engine across every value tested -- eight
`range_m` values from 1 km to 5,000 km, the saturating case, and zero -- worst distance 0
ULPs. That is a real result, but it is a measurement against one platform's C library
(Windows' UCRT); another libm backing CPython's `sin` could disagree, and nothing here
would catch it if it did. The fact that actually makes membership safe is the **geometric
margin**: across the corpus this measurement actually runs over -- `_margins_corpus(2000)`
(2,000 pseudo-random points, no pinned poles or meridian points) plus 1,000 points
deliberately built near a bisector midpoint and nudged off it -- the closest any
candidate's `offset` comes to `limit` is `7.307968641692697e-08`, about nine orders of
magnitude above a ULP at that scale (~1e-16 to ~1e-18). That gap absorbs any plausible
divergence in `limit`, whichever libm produced it. It is pinned by an asserted floor of
`1e-9` in the permanent conformance suite, with the observed value carried in the failure
message rather than merely printed. A second hard decision in this function, the shadow
sign at the third-plate exclusion, gets the same treatment over the same corpus: smallest
observed `|shadow|` is `5.962450345231574e-06`, floored the same way at `1e-9`.

**Three bugs, three ways of encoding the same lesson.** `lookup.py`'s own comments record
that all three trace back to one root cause: a hard decision taken on a quantity that is
actually continuous.

- **The arg-min flip this function exists to avoid.** `margin_at` returns a distance that
  varies smoothly, but *which* margin that distance belongs to jumps at any point
  equidistant from two bisectors -- Python's own comment prices this at five hundred
  metres of cliff. The fix is not to pick one: `margins_within` returns every bisector
  still in range and lets the caller sum their contributions, because a sum of continuous
  functions is continuous even where the arg-min over them is not.
- **The phantom bisector.** A bisector is the true margin between two plates only where
  those two are genuinely the nearest pair; elsewhere it runs through a third plate's
  territory, imaginary. Summing those unconditionally cost a hundred and seventy
  kilometres of phantom mountain range, and it was discontinuous besides. The fix stands
  at the closest point on the bisector and asks who the neighbours are *there*, one extra
  lookup, paid only for candidates already inside range.
- **The shadow weight that replaced a boolean.** The first fix for the phantom bisector
  rejected a shadowed candidate outright, which switched a margin on and off in one step
  wherever it landed near a triple junction -- a hundred and forty metres of cliff, and
  the Python's own comment calls this the third instance of the same mistake. It fades
  now: `genuine = smoothstep(clamp(shadow / SHADOW_BLEND, 0, 1))`, transcribed exactly.

**One hard exit that is deliberately safe, so it is not mistaken for a fourth instance.**
`if genuine <= 0.0 { continue }` is a boolean skip sitting right next to the fix for the
last boolean skip -- but it does not reintroduce the bug, because the smoothstep is
*exactly* zero at that boundary. A candidate that gets skipped there and a candidate that
gets included with `weight: 0.0` are indistinguishable to any caller that sums weighted
contributions; the `continue` only avoids pushing a no-op entry, it never changes what the
caller sees.

**The fade fix is now known to be guarded, not merely asserted.** Reverting the smoothstep
to the boolean it replaced (`if shadow <= 0.0 { continue } else { genuine = 1.0 }`) makes
`a_shadowed_margin_fades_rather_than_switching_off` fail with a single-step weight change
of `1.0` against the test's `0.25` bound -- both the implementer and the reviewer ran this
mutation independently and saw the same failure. The measured crossing along the test's
sample path sits at 12.25 degrees latitude, with 9 of its 200 samples landing inside the
fade band.

**This function had no dedicated test before this slice.** It was not untouched --
`tests/test_performance.py:215` and `tests/test_tectonics.py:201` both call it -- but
neither exercises it directly; both are aimed at other things and happen to invoke it
along the way. `test_plates.py`, which does target its neighbours directly (`nearest_two`,
`margin_at`, `margin_normal`, `flattened`), carries 27 tests and none of them are
`margins_within`'s. The conformance harness added here is the first test written to
exercise `margins_within` itself, in either language.

**The main corpus exercises the fade and skip paths on its own, not only through the two
hand-built three-plate tests above.** Instrumenting `margins_within` over
`test_plateset_margins_within_agrees_over_a_corpus_of_points`'s own corpus and range
spread (the 806-point `_margins_corpus(800)` against every non-saturating range in
`_range_values_for_margins`, i.e. excluding the range that selects every margin
unconditionally) produces 6,344 margin entries, of which 489 carry a weight strictly
between 0 and 1 -- the fade band, not merely on or off -- and 11,566 candidates that pass
the range test but are shadow-skipped (`genuine <= 0.0`) before ever reaching the returned
list. The corpus already exercises both paths at scale; the triple-junction and
none/some/all tests above pin specific, checkable points within it.

**The deliberate deviation, consistent with slices 1e and 1f.** The Rust addresses the
bisector table, the seed table, and the third-plate exclusion by a plate's **position** in
`self.plates`, on every axis. The Python is not internally consistent about this: within
`margins_within` itself, the candidate loop's `zip(self.plates, self._bisector_xyz[nearest.index])`
walks by position, but the third-plate exclusion compares `third.index == nearest.index or
third.index == other.index`, mixing index-based and position-based logic in the same
function. They coincide only because `generation.py` assigns `index=index for index in
range(count)` -- position and index are the same number for every plate the corpus
builds. This is the same deviation already recorded for `margin_at` and `margin_normal`
above, extended to the one function that has both styles inside itself.

**The scaffolding is gone, and that is intended, not an oversight.** Task 1 added a
throwaway `margins_within_limit` binding (`plates.rs`, `bindings.rs`, `lib.rs`) purely to
measure `limit`'s bit-identity directly, plus `tests/test_limit_ulps.py` to exercise it.
Both are deleted as of this section. Deleting them removes the only direct pin on
`limit`'s bit-identity -- but bit-identity was never the fact holding membership safe; the
geometric margin is, and that margin is pinned permanently, by a floor, in
`test_conformance.py`. A floor firing in the future is exactly the signal that strict
membership comparison needs to be revisited on this platform; bit-identity holding would
not have given that signal, only its absence would have, silently. So nobody should
"restore" `margins_within_limit` believing it was lost by accident -- its job is now done
by a test that can actually fail for the right reason.

Every test count in this section was verified by running the suites, not copied from an
earlier report: 86 crate tests (80 lib + 6 `no_std_math` guard, unchanged by this
deletion -- `margins_within_limit` had no dedicated Rust unit test), 292 in the full Python
suite (294 minus the 2 tests deleted with `test_limit_ulps.py`), and 52 in
`test_conformance.py` (unchanged -- those two tests never lived there).

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
