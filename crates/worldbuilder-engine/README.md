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
to the kinematics slice, once it reads poles and rates through `plateset_from_parts`
itself -- not merely once `angular_velocity` exists.** (This section originally said the
guard would arrive "because `angular_velocity` is the only function that reads those
fields"; the kinematics section below corrects that -- `angular_velocity` was ported and
bound before the guard existed, through a binding that never goes near
`plateset_from_parts`, so what was missing was narrower than this section claimed.) A
claim that carrying real values through a struct field is itself a
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

## `kinematics.rs`: the cleanest module in the port

`worldbuilder/plates/kinematics.py` contains no transcendental call at all. The only
non-arithmetic operation anywhere in it is the `sqrt` inside `length()`, and `sqrt` is
algebraic, not transcendental -- IEEE-754 requires it correctly rounded, the same fact
`plates.rs` already leaned on. So everything this module computes sits on the strict
bit-for-bit contract, including `ACROSS_ENOUGH`'s convergent/divergent/transform
classification -- the same shape of decision, a discrete choice on a continuous quantity,
that has bitten this project repeatedly (`lookup.py`'s three bugs, recorded above). Here it
does not bite, and that is stated as the reason rather than as a hope: every input the
comparison sees -- `closing`, `speed`, and their ratio -- is built entirely from dot
products, cross products, subtraction, and `length()`, so both languages compare identical
values and the classification cannot diverge between them. The one bounded quantity in the
neighbourhood is imported rather than computed here: `margin.distance_m`, which rides
inside the `Margin` a `Motion` carries, came through `asin` back in `margin_at`, and
`motion_at` never reads it -- it only forwards the `Margin` it was handed.

**The short-circuit is load-bearing.** `if speed <= 0.0 || closing.abs() / speed <
ACROSS_ENOUGH` -- transcribed operand order and all. The `or` is the only thing standing
between this line and a division by zero whenever two plates move identically, so it must
never be precomputed into a bool before the branch runs; doing that would evaluate the
division unconditionally and turn a defined `Transform` result into a NaN.

**The fabrication guard, and a correction to how this file described it.** The section
above ("But this slice's tests do not, and cannot, exercise a fabrication regression...")
said the guard would finally arrive "because `angular_velocity` is the only function that
reads those fields." That was imprecise, and it is corrected in place above rather than
merely re-argued here. `Plate::angular_velocity` was already ported, bound, and
conformance-tested by the time this slice started -- through `plate_angular_velocity`,
which builds its own `Plate` inline at `bindings.rs:189-195` and never calls
`plateset_from_parts` at all. So a guard on that function's *arithmetic* already existed.
What was actually missing, and what this slice supplies, is a guard on the *constructor
contract*: proof that `plateset_from_parts` carries a caller's poles and rates honestly
into something downstream that consumes them. `motion_at` is the first function that both
needs a `PlateSet` (rather than two bare `Plate`s built by hand) and reads poles and rates
through it, so this is the first slice where that guard becomes possible at all.

Task 4 proved it by mutation, not by argument: with `plateset_from_parts` edited back to
fabricate `pole = seed`, `rate = 0.0`, the crate rebuilt and `test_conformance.py` run
again, exactly the four `plateset_motion_at` tests failed -- closing speeds like
`-294560.96645866026` collapsing to exactly `-0.0`, because a zero rate zeroes
`angular_velocity()` for every plate -- while the other 59 tests, including every
`plate_surface_velocity` and `plates_motion_between` test (which build their `Plate`s
inline and never touch `plateset_from_parts`), kept passing, correctly, since none of them
read the fabricated fields. This was reproduced independently during review, not merely
reported once and taken on trust.

**The measured threshold margin.** Across the combined corpus, the smallest observed
`abs(abs(closing) / speed - ACROSS_ENOUGH)` at any point with `speed > 0.0` is
`6.4886e-04`. It is pinned by an asserted floor of `1e-9` in
`test_the_margin_classification_threshold_gap_is_measured_not_assumed` -- six orders of
magnitude below the observation, deliberately. The floor's job is to fire if this margin
ever collapses toward tie territory, not to pin today's value; a floor set just under
`6.4886e-04` would trip on any ordinary corpus change and get relaxed reflexively, which
would teach nobody anything the next time it fired for a real reason.

**One limit worth recording honestly.** `the_across_enough_threshold_is_hit_exactly_and_is_not_inclusive`
brackets `ACROSS_ENOUGH` with probes at `0.4` and `0.5` -- one just below the threshold,
one exactly on it. That catches `ACROSS_ENOUGH` being moved *outside* the bracket, but not
moved *inside* it: retyping the constant to `0.45` leaves both probes on the same side of
the (now different) threshold, so the Rust unit test alone stays green. Only the
conformance suite, comparing against the Python's actual `0.5`, catches that move. The
combination -- unit test plus conformance -- is sound; the unit test by itself is narrower
than a reader might assume from its name.

**`motion_at` calls `motion_between`, rather than duplicating the twelve lines Python
repeats between them.** Task 3 compared both Python bodies line by line and found them
byte-identical past the first two lines -- same operations, same order, same intermediate
names -- with the only difference being where the two `Plate`s come from (parameters versus
`margin.nearest`/`margin.neighbour`, which is exactly what `motion_at` passes as those
parameters). That is the same byte-identity ruling slice 1f already applied to `flattened`
and `margin_normal`: calling the already-ported function is strictly more faithful than
re-transcribing its body, because a transcription can drift from its original one line at a
time while a call cannot drift at all. `motion_at_agrees_bit_for_bit_with_motion_between_on_the_same_margin`
checks the consequence directly, `to_bits()` and all.

Every test count above was verified by running the suites, not copied from an earlier
report: 91 Rust lib tests plus the 6-test `no_std_math` guard, 63 in
`test_conformance.py`, and 303 in the full Python suite -- all unchanged from Task 4's own
numbers, since this task added no tests of its own.

## `tectonics.rs`: the module that finally breaks the 4-ULP contract

`bump` and `continental` are purely algebraic -- an `abs`, a division, a comparison or two,
and the same smoothstep already used by `Continentality::calibration`'s shadow weight --
so they carry no transcendental anywhere in their path and are compared with `same()`,
bit-for-bit, with zero divergence over the corpus. `setting_at`, `offset_m`, and
`elevation_m` are different: all three route through `hypot`, `tanh`, a tangent frame, and
`Continentality::at`, and none of them holds at the file's usual 4-ULP bound
(`MAX_TRANSCENDENTAL_ULPS`). This module needs its own, wider, and separately justified
bound: `TECTONICS_BOUNDED_MAX_ULPS = 8192`, for those three functions only.

**The mechanism, measured rather than assumed.** All three of these quantities can
legitimately pass through, or come arbitrarily close to, zero -- `engagement` at the
`ACROSS_ENOUGH` gate inside `offset_m`/`elevation_m`, and `Continentality::at`'s own
zero-crossing inside `setting_at`. ULP is a *relative* measure, and near zero it becomes
very fine, so an ordinary, small absolute rounding difference reads as an enormous ULP
count -- this is not amplification of the error itself, just of how the count reports it.
`setting_at` settles this cleanly: in one call, `inboard` (value −0.0194) came back
bit-exact while `outboard` (value −0.000635) showed 1,501 ULP. A ULP at that magnitude is
about 1.084e-19, so 1,501 ULP is an absolute difference of roughly 1.63e-16 -- ordinary
rounding scale. The same absolute error measured against the inboard value would read as
only 47 ULP. `offset_m` was checked the same way rather than assumed to match: at the
point producing its worst observed divergence (614 ULP), the value itself is
`0.016465184604870464` -- a few centimetres -- with an absolute difference of
`2.130240428499519e-15`. That is the same near-zero measurement artefact as `setting_at`,
not a genuine metre-scale relative divergence; `offset_m` sums margin contributions that
are built to reach exactly zero at the range gate and at `engagement`'s own gate, so the
corpus finds points where the total sits a few centimetres from that zero, and the ULP
count there is dominated by how close to zero the corpus happens to land, not by anything
wrong in the arithmetic. The honest scale to measure that absolute difference against is
not the near-zero result but the profile amplitudes the arithmetic actually runs at --
`TRENCH_M` alone reaches 2,600 m -- and `ULP(2600.0)` is `4.547e-13`, so
`2.130240428499519e-15` there is about `0.005` ULP: consistent with a single rounding at
the scale the arithmetic was performed, not with error growing anywhere in the sum.

**8,192 is an empirical ceiling over this corpus, not a derived guarantee.** Because the
quantity passes through zero, the ULP count is a function of how close the corpus happens
to sample to that zero -- a different corpus, or a larger one, could land closer to a gate
and see a wider divergence without anything in the port being wrong. So the bound is not a
proof; it is a number this corpus was observed to stay under, deliberately set well above
the worst figures actually seen (614 for `offset_m`, 512 for `elevation_m`, 1,501 for
`setting_at`'s `outboard`), because the brief's own error-propagation estimate for
`engagement` at the smallest measured engagement-gate gap put the relative error there at
roughly 4,200 ULP. The suite does not take this on faith either way:
`test_tectonics_offset_m_and_elevation_m_exceed_the_ordinary_transcendental_bound` asserts
*both* that the ordinary 4-ULP bound genuinely fails on this corpus (`worst >
MAX_TRANSCENDENTAL_ULPS`) and that the wider bound holds (`worst <=
TECTONICS_BOUNDED_MAX_ULPS`) -- so 8,192 was not a blind widening applied to make a test
pass; it replaces a bound that was measured to fail with one that was measured to hold.

**`math.hypot` is not a libm call in CPython.** Since Python 3.8 it is a Neumaier-summed
vector norm implemented in `mathmodule.c`, not a call into the platform C library the way
`sin`, `cos`, and `atan2` are. That makes this the first slice in the crate where the two
sides of a comparison are *known* to run different algorithms for the same function,
rather than merely permitted to diverge because they might happen to use different
libms. Measured divergence: up to 1 ULP, on 44 of the corpus's 4,025 `hypot` pairs; the
other 3,981 are bit-identical.

**Which of the three downstream branches in `from_margin` are safe, and why -- this is
the most useful thing in this section.** `from_margin` makes three decisions on the way to
a contribution, and only one of them actually depends on `hypot`'s precision:

- `if speed <= 0.0 { return 0.0 }` is safe because `hypot` is exactly zero only when both
  of its arguments are exactly zero, regardless of which algorithm computed it -- a
  1-ULP disagreement between `math.hypot` and `libm::hypot` cannot manufacture or erase an
  exact zero.
- `if across < 0.0` is safe even though it looks like the most dangerous decision in the
  file, because `hypot` is never negative, and the zero case has already returned by the
  time this branch runs -- so `speed` here is strictly positive, and dividing
  `motion.closing_m_per_myr` by a strictly positive number cannot change its sign. The
  branch is decided by the sign of `closing`, which is algebraic (a dot product and a
  subtraction), not by anything `hypot` contributes.
- `if engagement <= 0.0 { return 0.0 }` is the one branch that genuinely depends on
  `hypot`'s precision, because `across` is built directly from `speed`. The measured
  margin here is `abs(abs(across) - ACROSS_ENOUGH)` = 2.4349e-05 at its smallest observed
  point, over this slice's own ~22,000-point `TECTONICS_POINTS` corpus -- about 2.19e11
  ULP of `across` at that magnitude, roughly eleven orders of magnitude clear of where a
  1-ULP `hypot` disagreement could ever flip the comparison.

**The two bugs this port must preserve, and how each is encoded.** `lookup.py`'s and
`tectonics.py`'s own comments record two: a 550-metre cliff, and a 419-kilometre
mismapping.

- **The 550-metre cliff.** The first version of `continental`'s weighting used a hard
  test -- continental if above zero, oceanic otherwise -- and the ground jumped five
  hundred and fifty metres wherever a margin crossed that threshold, because the two sides
  of the test ran entirely different profiles. `CONTINENTAL_BLEND` (0.45) fixes it by
  turning the threshold into a width: `continental` is a smoothstep across that width
  rather than a step at a point, so a margin's classification moves continuously instead
  of jumping.
- **The 419-kilometre mismapping.** The obvious way to place a point on one side or the
  other of a margin is `signed = distance * lean`, and it is wrong in a way that took a
  diagnostic to find: scaling the axis by `lean` *compresses* distance, so with a lean of
  −0.22 a point 419 km out mapped to −90 km -- exactly where the trench sits. The trench
  fired at 400 km out, and the range gate then cut the mismapped profile off mid-feature.
  The fix keeps distance true and blends the *profile*, evaluating it on both sides of the
  margin and mixing by `lean`, so every feature stays at its intended range and every
  profile reaches zero by the gate on its own. The regression test for this is **exactly
  derivable**, not approximate: at 419 km every bump argument in the profile is outside its
  own width, on both sides of the blend, so the sum is exactly zero, not merely small --
  `assert_eq!(contribution, 0.0, ...)` rather than a tolerance. Substituting the buggy
  `signed = distance * lean` form back in was observed to make this test fail by 220
  metres, a large, unambiguous miss rather than a rounding-scale one.

**`motion.kind` is deliberately unused.** `motion.kind` names a margin
convergent/divergent/transform by a threshold on the same continuous quantity `from_margin`
already has as a number (`across`). Picking a terrain profile by that name, rather than by
the number, meant a margin drifting continuously from convergent to transform could lose an
entire mountain belt in a single step at the threshold crossing -- the same hard-decision
mistake `lookup.py`'s three bugs and the 550-metre cliff above both trace back to. The name
survives on `Motion` for diagnostics; the terrain only ever reads the number.

**`offset_m` sums every margin in range rather than choosing the nearest one, and the
summation order is load-bearing.** Choosing a single nearest margin was worth 560 metres of
cliff at any point where two margins' ranges overlap, for the same reason `margins_within`
sums rather than picks (recorded above): the *set* of margins in range is discrete and can
change discontinuously, but a sum of their continuous contributions stays continuous even
where an arg-min over them would not. Because floating-point addition is not associative,
`offset_m`'s loop must accumulate margins in the same order `margins_within` returns them
(plate-position order) -- sorting, reversing, or parallelising that accumulation would
still be "correct" in the sense of adding up the same numbers, but could round to a
different bit pattern, which is exactly the kind of divergence this crate's conformance
suite exists to catch.

**The scaffolding is gone.** Task 1's throwaway `tests/test_hypot_ulps.py`, and the
`detmath_hypot_temp`/`detmath_tanh_temp` bindings it exercised
(`crates/worldbuilder-engine/src/bindings.rs`, registered in `src/lib.rs`), are deleted as
of this section. Their job -- measuring `hypot` and `tanh` bit-identity directly, before
anything in the port depended on the answer -- is done; the findings live here and in
`test_conformance.py`'s permanent measurements instead.

Every count in this section was verified by running the suites, not copied from an earlier
report: 103 Rust lib tests plus the 6-test `no_std_math` guard (both unchanged --
`detmath_hypot_temp`/`detmath_tanh_temp` had no dedicated Rust unit test), 313 in the full
Python suite (319 minus the 6 tests deleted with `test_hypot_ulps.py`), and 73 in
`test_conformance.py` (unchanged -- those 6 tests never lived there). `cargo test -p
worldbuilder-engine`, run unfiltered, exits 0.

## `generation.rs`: the one step with no tolerance at all

Every module so far in this port has asked how far Rust and Python may drift before the
divergence stops being rounding and starts being a bug. `generation.rs` is the first place
that question does not apply. `_fraction` seeds a plate's position, pole and rate from a
BLAKE2 digest, and a digest is either identical or it is not -- there is no bounded-ULP
fallback for a hash. One differing bit does not nudge a coastline; it produces a `u64` from
a completely unrelated part of the digest space, and therefore an unrelated planet. So the
crate pins `blake2 = "=0.10.6"` exactly, in the same style as `libm`: a floating version
requirement would make world generation depend on which day the crate happened to be
built, since a routine dependency bump could silently reseed every world that has ever been
generated.

**Two traps, with the measurement that shows why each one matters.** Python's
`hashlib.blake2b(key, digest_size=8)` names its first argument `key`, but that argument is
BLAKE2's **message**, not its key parameter -- passing it to `Blake2bVar`'s actual keying
API would hash something else entirely while looking identical at every call site. And
`digest_size=8` is a real, freestanding 8-byte BLAKE2b, not the first 8 bytes of the
ordinary 64-byte digest truncated down, because BLAKE2 mixes the requested output length
into its initial state before the first block is compressed. The measurement makes this
concrete rather than asserted: the first 8 bytes of the full 64-byte BLAKE2b digest of
`"20260831|plate|7|pole-z"` are `fe33b7b6e9e16221`; the genuine 8-byte digest of the same
message is `2d729d257c6a1550`. Those two hex strings share no structure at all, which is
exactly the point -- `Blake2bVar::new(8)` is not a truncation with a different name, and
substituting one for the other would not fail loudly, it would just generate a different
universe.

**The hazard that did not materialise.** The obvious worry about hashing a joined string is
Python's and Rust's `str()`/`Display` disagreeing on some float's decimal representation --
the trap slice 1h and others spent real effort guarding against. It does not arise here,
because no float ever reaches `joined_key`: every part passed to `_fraction` is an `i64` or
a short string label (`"plate"`, `"pole-z"`, `"sense"`, and so on), and integers format
identically in both languages. Worth recording precisely because it is the first thing
anyone familiar with this port's history would fear, and precisely why it never comes up.

**The contract split, and it is unusually clean for this crate.** `fraction` and `rate` are
**strict, bit-for-bit** -- a digest, a little-endian `u64`, a division by `2**64` (an exact
power of two), and pure arithmetic on the result, with no transcendental anywhere in
either path. `test_conformance.py` holds both to `same()` rather than `close_enough()` and
they held across all four seeds (`0`, a negative seed, `20260831`, and `i64::MAX`), 40
plate indices, and all six labels `_fraction` is ever called with, with zero exceptions.
`spread` and `pole`, by contrast, are bounded: both end their computation in `cos`/`sin`.

**Why `turning = fraction < 0.5` is safe.** This is a discrete decision on a continuous
quantity -- the exact shape that has caused trouble everywhere else in this port, from the
550-metre cliff in `tectonics.rs` to `ACROSS_ENOUGH`'s classification threshold. Here it is
safe, but for a better reason than "the corpus happens not to land on the boundary": the
quantity being thresholded is *exactly* reproducible. It comes from a byte-identical BLAKE2
digest through an integer-to-float conversion and a division by an exact power of two, with
no transcendental anywhere in the path, so Rust and Python are comparing identical bit
patterns against `0.5`, not two independently-rounded approximations that could land on
opposite sides of it.

**Why the degeneracy guard is unreachable, derived rather than asserted.** Python's `_spread`
falls back to a second cross product if `sideways.length() < 1e-9`. `sideways` is
`(0, 0, 1).cross(point)`, so its length is exactly the spiral's ring radius. With
`z = 1 - 2u` for `u = (index + 0.5) / count`, `1 - z^2 = 4u(1-u)`, so
`ring = 2*sqrt(u(1-u))`, smallest at `index = 0`, where it approaches `sqrt(2/count)` for
large `count`. Firing the `1e-9` guard needs `count > 2e18` -- not a realistic plate count
by any margin. Measured, not just derived: the minimum ring across counts up to 100,000 is
`0.004472`, about 4.5 million times the guard threshold. The guard is ported anyway, because
removing it would change behaviour for an absurd count and a future reader should not have
to re-derive why it never fires in practice.

**The constructor distinction, and it is now guarded.** `spread_impl` ends with the
normalising `SpherePoint::from_vector`, because its nudged point is not unit by
construction. `pole` ends with the direct, non-normalising `SpherePoint { vector }`
constructor, because its vector -- built from `cos`/`sin` of an angle and a ring computed to
make the whole thing unit -- already is unit, and normalising it would look like a tidy-up
while quietly moving every pole's bits. **The conformance suite cannot catch a swap of the
two.** Swapping in `from_vector` for `pole` moves values by about 2 ULP (measured at pole
6), which hides inside the 4 ULP bound `pole` already earns for going through `cos`/`sin` --
`test_conformance.py` compares Python's reference against whatever the Rust side currently
does, so both constructors pass. The guard is a Rust unit test instead:
`pole_uses_the_non_normalising_constructor` rebuilds the vector by hand, without
normalising, and requires bit equality against what `pole` actually returns. It was observed
to fail when the swap was made deliberately, which is the only way to trust that a test like
this actually tests anything.

**`plates_for` is what makes `index == position` true.** Its loop assigns
`index: index` for `index in 0..count`, both together, in the same iteration. Slices 1e,
1f and 1g all address the bisector table and the seed/pole tables by *position*, and that
only agrees with a plate's `.index` field because this one line assigns them together. If
this line ever assigned anything else to `index` -- a shuffled order, a filtered subset --
those earlier slices would silently address the wrong rows. No error, just a different
planet.

**The `spread` bound, stated the honest way round.** Lead with the reassuring number: at
`DEFAULT_PLATE_COUNT` (22, the only count any world this project actually builds uses),
`spread`'s divergence from Python is **3 ULP** -- inside the ordinary 4-ULP
`MAX_TRANSCENDENTAL_ULPS` bound with no special allowance needed at all.
`GENERATION_SPREAD_BOUNDED_MAX_ULPS = 32` exists only because `test_conformance.py`'s sweep
deliberately reaches count 137, far past any real world.

32 is scoped to the counts the sweep actually tests, not a property of `spread` itself, and
that has to be said plainly: measured divergence grows with count -- 3 ULP at 22, 6 ULP at
137, 8 ULP at 500, 16 ULP at 1000, and up to **131 ULP at 5000**. A larger plate count needs
its own measurement, not an extrapolation of this one. Two mechanisms compound to produce
that growth. First, `angle = golden * index` grows without bound as `index` grows, so the
trig range reduction `cos`/`sin` need becomes more demanding, and CPython's range reduction
does not agree bit-for-bit with `libm`'s -- `pole`'s angle, by contrast, is bounded to a
single turn (0 to 2*pi), needs no such reduction, and shows only 2 ULP. Second, ULP is a
*relative* measure that gets very fine near zero, so an ordinary small absolute rounding
difference in a near-zero vector component reads as a large ULP count on its own, with
nothing wrong in the arithmetic -- this is the same effect the Tectonics section above
documents at much larger scale, so it is described consistently with that section here
rather than in new words.

`test_generation_spread_agrees_within_the_measured_bound` ties the bound to the range it
was measured over: an assertion checks `GENERATION_COUNTS` has not grown past
`GENERATION_SPREAD_MEASURED_MAX_COUNT` (137) before it trusts the bound at all, and fires
first, with a message explaining why, if the sweep is ever widened without a fresh
measurement.

**The scaffolding is gone.** Task 1's throwaway cross-language harness,
`tests/test_blake2_bytes.py`, is deleted as of this section -- its job was proving the
`blake2` crate matched CPython before anything in the port depended on the answer, and
`test_conformance.py` now covers `_fraction` and friends directly against the built engine.
`crates/worldbuilder-engine/tests/blake2_bytes.rs` **stays**, permanently, even though
`test_conformance.py`'s `_fraction` comparison would itself catch a future `blake2` crate
version bump: the Python side of that comparison is `hashlib`, not the Rust `blake2`
crate, so the two are already independent, and a digest change on the Rust side would
break the 960-case bit-for-bit `same()` assertion loudly. `blake2_bytes.rs` earns its
keep for other reasons -- it is Rust-only, so it fails without the Python extension
needing to be built at all; it pins specific vectors sourced independently against CPython
rather than comparing two live computations; and it localises a failure to the dependency
itself, instead of surfacing as a whole-generation-chain mismatch that someone would have
to diagnose back to its root.

Every count in this section was verified by running the suites, not copied from an earlier
report: 133 Rust tests (123 lib plus the 4-test `blake2_bytes.rs` plus the 6-test
`no_std_math` guard -- unchanged from before this task, since `generation.rs` and its tests
already existed going in), 319 in the full Python suite (324 minus the 5 tests deleted with
`tests/test_blake2_bytes.py`), and 79 in `test_conformance.py` (unchanged -- those 5 tests
never lived there). `cargo test -p worldbuilder-engine`, run unfiltered, exits 0.

## `detail.rs`: the first module with no transcendental anywhere, and two traps a value
## test would have missed

`worldbuilder/terrain/detail.py` contains no transcendental call in any path at all --
not "none that matters," none. `math.pi` appears in `_plan`'s frequency expression, but a
module-level constant is not an operation; `Noise`, which `Detail` wires straight through
for its band sampling, reaches only `floor`, already established strict above. So the
whole module sits on the strict, bit-for-bit contract, every comparison in its
conformance section uses `same()`, and there is no `close_enough()` anywhere in it -- a
claim tested by running the suite with that contract, not assumed because the source
looked simple. It also settles every discrete decision in the module in one stroke:
`if resolution_m:`, `if visible <= 0.0: break`, and the two clamps inside `smooth` all
compare exactly-reproducible values on both sides, so none of them can diverge between
languages and none needed its own argument the way `ACROSS_ENOUGH` or the shadow gate
did in earlier sections.

**The frequency expression, stated honestly.** `_plan` writes
`2.0 * math.pi * radius_m / wavelength / (2.0 * math.pi)`, which is algebraically
`radius_m / wavelength` -- the `2.0 * math.pi` introduced and then divided back out again.
At Earth's radius, for all seven configured wavelengths, both forms are bit-identical, so
simplifying the expression would break nothing in the default world and a reviewer
skimming the diff would have no reason to object. They diverge at other radii:
`test_detail_bands_uses_the_transcribed_frequency_formula_not_the_simplified_one` pins
`DETAIL_NON_EARTH_RADIUS_M = 32450893.20683292` with `wavelength = 10000.0`, where the
four-operation transcription gives `3245.0893206832916` against `3245.089320683292` from
the simplified form -- one ULP apart, and the test asserts the reference itself lands on
the transcribed literal and *not* the simplified one, so it cannot pass merely because
both languages made the same mistake. Since `radius_m` is a constructor parameter here,
not a fixed constant, the four-operation form is prophylactic for Earth and load-bearing
for anything else -- `detail.rs`'s `plan` keeps it in the Python's order for exactly this
reason.

**The band table.** Seven octaves, halving from `COARSEST_WAVELENGTH_M` (20,000 m) down
to the last that still qualifies at `CANONICAL_WAVELENGTH_M` (250 m, since 156.25 falls
below it): 20000, 10000, 5000, 2500, 1250, 625, 312.5. At Earth's radius those map to
frequencies 318.55 through 20387.2, and the raw shares -- halving from 1.0 alongside the
wavelength -- are normalised so they sum to exactly 1.0 regardless of how many bands the
loop happens to produce, "otherwise adding an octave would quietly make every world
rougher." `the_shares_are_normalised_to_exactly_one` checks the sum lands on `1.0`
exactly, not merely close to it.

**The falsy-zero trap, and the intuition it defeats.** Python's `if resolution_m:` is
false for `None`, `0.0`, *and* `-0.0` -- all three take the canonical every-octave path.
A Rust `Option<f64>` port has to collapse `Some(0.0)` and `Some(-0.0)` to `None` itself;
`f64` has no truthiness of its own to inherit that from.

The natural next question is which of the two zeros actually needs the guard, and the
answer runs backwards from intuition. Removing the collapse and rebuilding leaves
`wavelength / 0.0` as `+inf` inside the loop; `smooth(+inf)` clamps to `1.0`, the same
value the canonical arm's literal `1.0` gives, bit for bit -- `+0.0` does not diverge even
with no guard at all. It is `-0.0` that breaks: `wavelength / -0.0` is `-inf`,
`smooth(-inf)` clamps to `0.0`, and `if visible <= 0.0: break` fires on the very first,
coarsest band, dropping every octave where Python's falsy `-0.0` gives full detail. So the
guard is load-bearing, just not for the value one would naturally reach for first. This
was proven by mutation, not read off the source: with the `r != 0.0` collapse in
`offset_m` removed and the crate rebuilt, `test_detail_offset_m_agrees_bit_for_bit` and
`test_detail_offset_m_zero_resolution_matches_omitted_resolution` both failed on the
`-0.0` case (`want=-41.65428342343554, got=0.0`, at point `(0.0, 0.0, 1.0)`) while every
`+0.0` case in the same sweep stayed silently green -- exactly the asymmetry the analysis
predicts. `DETAIL_RESOLUTIONS_M` now carries both `0.0` and `-0.0` so every parametrised
sweep in the section exercises the distinction, not just the two tests that motivated it.

**`smooth`'s clamp order is observable only under NaN.** `max(0.0, min(1.0, fraction))`
and the swapped order `min(1.0, max(0.0, fraction))` agree for every finite input and for
both infinities -- they differ only when `fraction` is NaN, where Python's order gives
`1.0` (the outer `max` against a NaN inner result) and the swap gives `0.0`. The suite
reaches that case through a NaN `resolution_m`: Python's `if resolution_m:` is true for
NaN (NaN is truthy), so it takes the *resolution* branch, not the canonical one, and
`wavelength / NaN` is NaN going into `smooth`. Swapping the clamp order and rebuilding
made `test_detail_offset_m_agrees_bit_for_bit` fail on the NaN case
(`want=-42.98861825522871, got=0.0`, same seed and point as above) -- confirming the
order matters exactly where the analysis says it should and nowhere else. `float("nan")`
now sits in `DETAIL_RESOLUTIONS_M` alongside `-0.0` for the same reason: a differential
suite that never manufactures a NaN cannot tell two clamp orders apart, however carefully
its comments explain why they'd agree.

**The fade bound, and how the test guarding it was first got wrong.** Octaves fade
between `BARELY_M` and `CLEARLY_M` multiples of the sample spacing rather than switching
off, because "dropping one the instant it becomes unrepresentable would be a cliff in
resolution rather than in position -- the ground would jump as somebody zoomed."
`the_fade_is_gradual_rather_than_a_step` guards that smoothness, and its first bound was
derived from an upper bound on a single band's legitimate per-sample swing. That bound was
real, but an upper bound on the *legitimate* signal necessarily also admits the
*illegitimate* one -- a hard cutoff's step is smaller than "anything could happen," so it
passed a test built only to rule out the impossible. The bound now comes from `smooth`'s
own peak slope instead (1.5, the maximum of the smoothstep's derivative `6x - 6x^2`),
which gives a ceiling that actually discriminates. The analytic gradual ceiling that falls
out of that peak slope is roughly 3.02; the test bound is set with headroom above it, at
`0.2 * share * amplitude` ~= 10.08, so a real fade never trips it while a hard cutoff still
does. Measured against the real implementation, the actual, unmutated fade comes in at
about 0.956 -- comfortably under both the analytic ceiling and the test bound -- while a
hard step at the same crossing measures roughly 25.75, well past the bound. Mutating
`visible`'s computation to a hard step and rerunning confirmed the failure; reverting confirmed the pass. The
general lesson, not just this test's: **a derived bound is not automatically a
discriminating one** -- deriving it from the size of the thing being measured proves
nothing about telling it apart from the thing it must reject; the right derivation
compares the two values the test actually needs to distinguish.

**Why sub-sample frequencies are skipped rather than merely wasted, and why `break` is
correct.** An octave shorter than the sample spacing does not just cost cycles for no
visible benefit -- it aliases: it "lands somewhere different in every grid, so a chart
would shimmer as a ship moved rather than showing generalised ground." The loop walks
bands coarsest-first, so once one band's `visible` clamps to `0.0` every band after it is
finer still and equally invisible; `break` throws away no work `continue` would have kept,
and it says so in the code rather than leaving a reader to wonder why the loop doesn't
just skip the dead band and keep going.

Every count in this section was verified by running the suites, not copied from an
earlier report: 146 Rust tests (136 lib plus the 4-test `blake2_bytes.rs` plus the 6-test
`no_std_math` guard -- unchanged from before this task, since `detail.rs`'s function
bodies already existed going in and this task's only change was two conformance test
cases), 327 in the full Python suite, and 87 in `test_conformance.py` (up from 79). The
eight new test *functions* added when this task bound `Detail` account for that whole
delta. The later fix that grew `DETAIL_RESOLUTIONS_M` from five entries to seven, guarding
the `-0.0` and NaN traps described above, added no new test functions -- it deepened the
parametrised sweeps inside tests that already existed, so the case-count delta the
mutations above depended on shows up inside the existing 87, not as a further rise in
the count.
`cargo test -p worldbuilder-engine`, run unfiltered, exits 0.

## `shelf.rs`: the first module that blends instead of adding

`worldbuilder/bathymetry/shelf.py`'s own docstring opens with three rules, and two of them
are scars from M1.4. The first is the one that makes this module different from every
earlier port: **it returns an absolute elevation by blending, not a contribution to add.**
`tectonics.py` and `detail.py` both return offsets that something else sums in; `shelf.py`
returns the ground itself, computed as `macro + weight * (target - macro)`. The docstring
is emphatic about why, and the reason is worth carrying forward exactly as written rather
than paraphrased: *"A shelf describes what the coastal profile should tend to, and
blending leaves control over what it may override -- so a trench crossing a continental
margin is not quietly flattened by something announcing that the water here is about a
hundred metres."* An offset cannot express "defer to whatever is already here"; a blend
weighted toward zero can, and that is the whole reason `weight` exists as a first-class
output of `evaluate` rather than an internal detail.

**The contract split, measured rather than assumed.** `shelf.py` contains no
transcendental call of its own -- no `math` name is bound in the module, and no
transcendental function appears in its source. It reaches exactly one, indirectly:
`hypot`, inside `Continentality`'s `Gradient::magnitude()`, and only by way of
`coastal()`'s `gradient(point).magnitude()` call that produces `slope`. **`above_shore`
does not reach it**, and that was checked behaviourally, not just by reading imports: with
`math.hypot` patched to raise, `above_shore()` ran clean over 2,000 corpus points, while
the same patch made `coastal()` hit the raise on effectively every point not already
short-circuited by the window gate. Structural evidence (no mention of `gradient` or
`magnitude` in `above_shore`'s source) and behavioural evidence (the corpus ran with the
function exploding on contact) agree, and the behavioural check is the stronger of the two
-- it is evidence about what the code actually does, not about what its source happens to
mention. So `above_shore` (gate 1, `abs(value) > COASTAL_WINDOW`) is strict, and only what
is downstream of `slope` -- `Coastal.distance_m`, `Coastal.breadth`, gate 2 -- is bounded.
`target_depth_m` and `weight` are themselves purely algebraic (division, `max`, a
smoothstep, `abs`), and given bit-identical inputs they measured **bit-exact**, confirming
they carry no hazard of their own and that the split runs exactly where the source says it
does.

The sign arguments in `target_depth_m` and `weight` follow directly from that split.
`offshore = -coastal.distance_m`, and `distance_m = value / slope` with `slope` strictly
positive by the time either function runs (the `MIN_GRADIENT` gate in `coastal()` has
already returned otherwise) -- so every branch on the sign of `offshore` is decided by the
sign of `value`, which never touches `hypot`, even though it looks exposed to the same
mixed-sign hazard `slope` carries. `shelf.rs` states this in its own comments rather than
leaving it to be re-derived by a future reader.

**Why the two gates in `evaluate` are safe, structurally rather than numerically.** Both
early returns in `evaluate` -- the `coastal()` gate and the `weight <= 0.0` gate --
produce the *identical* `Reading { elevation_m: macro, weight: 0.0, tectonic_m: tectonic }`.
That means a gate flipping incorrectly is observable only if the branch not taken would
have produced a `weight` above zero; the two gates are not independent hazards, they funnel
into one result. This is the module's own rule -- *"every gate sits outside the support of
what it gates"* -- realised in the control flow rather than merely stated in the docstring.
Reversing the two `return` statements was mutation-tested and, correctly, changed nothing:
there is nothing for the swap to disturb when both branches already agree on what they
hand back.

The measured margins back that up with numbers rather than leaving it as a structural
argument alone. Over the corpus, the closest any point comes to `COASTAL_WINDOW` is
`1.053777e-06` -- about 1.5e11 ULPs at that threshold -- and the closest any point comes to
`MIN_GRADIENT` is `2.371402e-09` -- about 1.4e15 ULPs. A 1-ULP disagreement in `hypot`
cannot move either margin by anything close to enough to flip a gate. And the gradient gate
is not dead code being carried out of caution: it is **live**, firing on 6 of the corpus's
20,006 points, with the closest approach to firing at `0.2501 x MIN_GRADIENT`.

**A claim in the reference Python that is corpus-true, not universal -- recorded as an
observation, since nothing under `worldbuilder/` changed.** `MIN_GRADIENT`'s own comment
says *"the weight has already faded out by here; this only stops the arithmetic."* Every
sub-threshold point the corpus actually produces does give a weight near zero, consistent
with the comment. But a hand-built point with a tiny `value` *and* a tiny `slope` --
tiny enough to fail `MIN_GRADIENT`, but not zero -- gives a weight of **0.9999979**, not
faded at all. The comment describes what this corpus happens to sample, not what the
formula guarantees; the gate is load-bearing in a way its own wording understates.

**The composed bounds, and the mistake they replaced.** A first pass on `evaluate`'s three
returned fields borrowed `TECTONICS_BOUNDED_MAX_ULPS` (8192) wholesale, on the theory that
the divergence was inherited from the Tectonics section's own cancellation hazard. Mutation
testing found two things wrong with that, not one:

- **It was loose enough to hide a real defect.** Rewriting `evaluate`'s blend to the
  algebraically-equal `macro * (1.0 - weight) + target * weight` diverges `elevation_m` by
  203 ULP -- comfortably inside 8192, so the conformance suite would have stayed green on a
  genuine bug in the port.
- **The attribution was factually wrong.** At the point where `weight` diverges most
  (1024 ULP), `tectonic_m` is bit-identical on both sides -- 0 ULP, not the inherited hazard
  a first pass assumed. The real mechanism is local to this module's own formula:
  `seaward = 1.0 - smooth(x)` at that point evaluates `smooth` at `x ~= 0.98197`, where
  `smooth(x) ~= 0.999037` -- close enough to 1.0 that subtracting it from 1.0 loses most of
  the input's precision to catastrophic cancellation. That is a hazard `shelf.py`'s own
  formula introduces, not one it picked up from `tectonics.rs`.

Each field now gets its own bound, sized to what it actually needs rather than shared by
assumption: **`SHELF_ELEVATION_MAX_ULPS = 96`** (measured worst 36; the composition with
`Tectonics.offset_m` and `Continentality.base_elevation` genuinely moves it a little), and
96 is proven tight by the mutation itself -- it passes the real port at 36 and fails the
blend-rewrite mutation at 203. **`SHELF_WEIGHT_MAX_ULPS = 2048`** (measured worst 1024, the
`seaward` cancellation above -- not inherited from tectonics). **`SHELF_TECTONIC_MAX_ULPS
= 512`** (measured worst 230, and this one genuinely *is* inherited, since `tectonic_m` is
a literal passthrough of `Tectonics.offset_m`). The headroom each bound carries over its
own measurement -- 2.67x for elevation, 2.0x for weight, 2.2x for tectonic -- sits in the
same proportionate range across all three, against the discredited 8192, which sat 8.0x
(weight: 8192/1024), 35.6x (tectonic: 8192/230), and 227.5x (elevation: 8192/36) above its
own legitimate per-field values -- an 8x-to-228x spread, not the tight one previously
claimed. Put more precisely than a bare range can: 8192 was 227x too loose for
`elevation_m` specifically, which is exactly why the 203-ULP blend-rewrite defect above
passed through it unnoticed. **A borrowed bound admits whatever the lending module
admits, whether or not that is what is actually being measured.**

**One limitation, stated honestly rather than left implicit.** A 2048-ULP bound on a
`weight` confined to `[0, 1]` is a weak assertion. Decomposing `weight` into `seaward`,
`breadth`, and `authority` and bounding each separately would likely tighten it, since
`breadth` is exact here (carried straight through from `coastal()`, not recomputed) and
`authority` is only as bad as `tectonic_m`'s own 230-ULP hazard -- so the real payoff is
isolating `seaward`'s cancellation on its own. That decomposition was not done in this
slice. The tight elevation bound partially backstops the weak weight bound, because
`weight` only reaches `elevation_m` through the blend -- but that backstop scales with
`(target - macro)`, so it weakens wherever those two are close. At the actual point where
`weight` diverges worst, `(target - macro)` is `112.301` -- not small -- and `elevation_m`
there diverges by only 1 ULP, so the backstop holds at the point measured. That does not
rule out some other point combining a near-maximal `weight` divergence with a small
`(target - macro)`, where the backstop would do little. **`surface.py`, the module that
consumes `weight` directly once it composes every terrain layer, is where this should be
revisited** -- the limitation has a named successor rather than being left open-ended.

**Two properties no test covers, recorded plainly rather than implied as tested.** The
value is checked before the gradient is taken in `coastal()`, and `Tectonics.offset_m` is
computed exactly once per call to `evaluate` rather than once per place that wants it.
Both are *cost* properties, not correctness ones: an implementation that recomputed the
gradient eagerly, or called `offset_m` two or three times over, would produce identical
values and a fully green suite while quietly paying for it. They are verified by reading
`shelf.rs`, not by an assertion that could catch a regression -- there is no cheap way to
observe call counts against `Continentality` and `Tectonics`, both concrete types with no
counting seam. `shelf.py`'s own docstring records this exact failure having happened
before: asking for the gradient, the tectonic offset, and the macro elevation separately
rather than threading them through `evaluate`'s `Reading` cost the gradient twice and the
tectonics three times over, and took a whole-pipeline chart from three hundred
milliseconds to twelve hundred -- while a comment at the time claimed the values were
"recovered rather than recomputed where it is free." `shelf.rs`'s `evaluate` computes
`tectonic` once and threads it through as `Some(tectonic)` so `weight` never asks
`self.tectonics.offset_m` again; nothing currently proves that stays true under a future
edit.

**The throwaway is gone.** Task 1's `tests/test_shelf_gates.py`, which measured the two
gate margins and the `MIN_GRADIENT` comment's claim against the live Python before
anything in the port depended on the answer, is deleted as of this section. Its four
findings -- where the `hypot` is and is not reached, the two measured margins, the
corpus-true-not-universal verdict on the comment, and the gradient gate's liveness -- live
here and in `test_conformance.py`'s permanent measurements instead.

Every count in this section was verified by running the suites, not copied from an earlier
report: 164 Rust tests before this task (154 lib plus the 4-test `blake2_bytes.rs` plus the
6-test `no_std_math` guard), unchanged by this task -- `shelf.rs`'s tests already existed
going in and this task added no new ones. The full Python suite drops from 344 to **338**
(the 6 tests deleted with `tests/test_shelf_gates.py`), and `test_conformance.py` stays at
**98** -- those 6 tests never lived there. `cargo test -p worldbuilder-engine`, run
unfiltered, exits 0.

## `features.rs`: the module with two consumers, and bounds that belong to a shape corpus

`worldbuilder/bathymetry/features.py` is the second channel. Everything before it decides
what ordinary ground looks like; this is where somebody says a channel goes *here* and a
bar goes across *that* harbour mouth. It runs **after the shelf and before the detail**:
`terrain/surface.py` computes `shelf.evaluate(point)`, hands `reading.elevation_m` to
`features.apply`, and only then asks `detail` for its offset -- with `apply`'s second
return value, `authority`, telling the detail how far to get out of the way.

**`Placed` has two independent consumers, which is why `weight_at` is `pub`.** The obvious
one is `Features::apply`. The other is `substrate.py`, which is not ported yet and which
bypasses `apply` entirely: it walks `surface.features.placed`, reads
`placed.feature.substrate` off each one, and calls `placed.weight_at(point)` itself to
blend a stated composition in. So `weight_at` is not a private helper that happens to be
visible -- it is a first-class entry point with a caller that never touches `apply`, and
narrowing it would break a module that has not arrived yet.

### The transcendental map, and two calls that do not have the same profile

`bump`, `smooth`, and every constant in the module are plain arithmetic -- `abs`, a divide,
a two-argument `min`, a smoothstep -- and are transcribed **strictly**, raw bits. Exactly
three things reach `detmath`:

- **`Feature::reach_m` -> `hypot`.** Bounded at **1 ULP**, and the reason is not rounding
  but algorithm: since 3.8 CPython does not call the platform `hypot` at all, it computes
  its own Neumaier-compensated norm, while the engine calls `libm::hypot`. Two different
  algorithms, so bit-equality is not something either side ever promised.
- **`Placed::weight_at` -> `sphere_to_local` -> `atan2` + `sqrt`.**
- **`marks_near` -> `SpherePoint::distance_to` -> `angle_to` -> `atan2` (+ `sqrt`).**

**`sphere_to_local` and `local_to_sphere` do not have the same profile, and an assumption
earlier in this slice that they did was wrong.** `sphere_to_local` reaches `atan2` and a
`sqrt` (through `Vec3::length`) and nothing else. `local_to_sphere` reaches `hypot`, `cos`,
`sin` **and** `sqrt`. They are inverses of each other and they are tested as one, but they
are not interchangeable when the question is what a value costs in tolerance: `weight_at`
goes through the cheaper direction only, and a bound argued from `local_to_sphere`'s
`hypot` would be an inherited bound, not a measured one.

`sqrt` costs nothing anywhere above. It is the one operation IEEE-754 requires to be
correctly rounded, so both languages produce the same bits from the same input by
specification. `hypot` is the exact opposite case, for the CPython reason given above --
the two facts sit next to each other because reading "both are square-root-ish" as "both
are free" is precisely the mistake to avoid.

### The reach gate is load-bearing, and a ring scan proves nothing about it

`weight_at` opens with `if point.vector.dot(&self.feature.at.vector) < self.cos_reach {
return 0.0; }`. An earlier extraction claimed both branches -- gated and ungated -- give
approximately zero everywhere, and treated the gate as an optimisation to simplify away.
That claim came out of a **ring scan**: 30,240 gate-rejected points sampled around
`reach_m` across 16 shapes, **zero leaks**. Reproduced independently while this section was
written -- 2,000 azimuths x 8 radial offsets at 3x2, 150x90 and 1200x300, giving 29,712
gate-rejected ring points and **zero leaks** again. A ring cannot find it, because the leak
is not on the ring.

**The leak lives in the corner**, where `along` lands a hair inside `length_m` and `across`
lands a hair inside `width_m` *at the same time*, so both `bump` factors are individually
non-zero even though the true arc distance has already passed `reach_m`. The band exists
because near the origin `dot` and `cos_reach` are both within an ULP of `1.0` and the
comparison stops resolving distance at all; the band's width in metres runs as
`ULP(1.0) * radius_m^2 / reach_m`, so it *narrows* as the feature grows.

**Numbers, each with the shape that produced it -- there is no module-wide figure here.**
Scanned in absolute insets at the corner, worst leaked ungated weight:

    3x2         1.2047e-12   (at ~1.2 mm along, ~1.8 mm across)
    150x90      1.1055e-26
    1200x300    8.4188e-32

That is a fall-off of roughly a **fourth** power in each of `length_m` and `width_m`. An
earlier `1 / (length_m^2 * width_m^2)` in this slice came from a *relative*-span grid that
never reached the widest part of the band, and understates how fast the leak dies. Either
way the point is the same, and it is the point that matters: **quote a shape beside the
number.** At 1200x300 a leak of 1e-32 is invisible to `shaped_metres` and shows up only in
`authority`, which starts at a hard `0.0` where `max(0.0, tiny)` is `tiny`. At 3x2 a leak
of 1e-12 reaches `shaped_metres` itself -- an ungated `result` of `-29.999999999970655`
against an exact `-30.0` has been measured there. The gate is transcribed exactly at every
size, and `features.rs`'s own corner test scans the small shape for that reason. Its
assertion is exact `0.0`, not a tolerance, so deleting the gate kills it outright.

**The gate is pinned in both directions, and the floor that makes the second direction
work is derived rather than fitted.** Direction A -- Python rejects, so the engine must
return exactly `0.0` -- catches a more permissive engine gate; direction B -- Python accepts
with a weight clear of zero, so the engine must also return non-zero -- catches a stricter
one. Both were mutation-verified: `cos_reach + 1 ULP` and `cos_reach - 1 ULP` are each
caught by the test named for the gate, at 39 of 41,104 and 23 of 29,269 probes respectively.
Direction B needs a "clear of zero" floor and it sits at `1e-24`, mid-window: the
support-edge contamination is gone by `1e-28`, and the mutant signal is detectable anywhere
from `1e-28` to `1e-20`.

### `apply` has two bit-observable zero-weight paths, not one

Both can turn a `-0.0` elevation into `+0.0`, and neither may be algebraically simplified:

- **`if weight <= 0.0 { continue; }`.** Written `<=`, not `<`. With `weight == -0.0` this
  skips, `result` is untouched, and an `elevation_m` of `-0.0` comes back as `-0.0`.
  Rewritten to `<`, the loop falls through to `result += weight * lift`, computing
  `-0.0 + (-0.0 * lift)` -- value-equal, bit-different. The mutation was run and caught.
- **The RAISE/CARVE pair**, `compose == RAISE && lift <= 0.0` and `compose == CARVE &&
  lift >= 0.0`, transcribed as two separate `if`s rather than folded into one. Both
  converge at `lift == 0.0`, but that is a fact about where each one's effect is zero,
  discovered independently -- not a shared reason to merge them. With `result == -0.0` and
  `target_m == 0.0`, the guard skips and `result` stays `-0.0`; a "simplified" version that
  let `lift == 0.0` fall through would compute `-0.0 + weight * 0.0`, which is `+0.0`.

Two paths, not one, and a conformance suite that only exercised the weight guard would
leave the other free to be tidied away.

`authority = max(authority, candidate)` is CPython's two-argument `max`, which returns its
**first** argument unless the second compares strictly greater -- so it is written
`if candidate > authority { candidate } else { authority }`, in that operand order, not
`f64::max`.

### Iteration order is semantic, not merely float-non-associative

`features.py` says it plainly, and the Rust carries the same words: *"Order is meaning
here. A bar listed after the channel it lies across sits on the carved bottom, which is the
right story; listed before, the channel would cut straight through it."* Each iteration's
`result` feeds the **next** feature's `lift`, not the original `elevation_m`. So
`self.placed` is walked in construction order in a plain `for` loop -- never sorted, never
accumulated in parallel and combined afterwards. Both of those would still be
deterministic; neither would tell the same story. The conformance suite asserts the two
orders of the same feature list differ by more than 10 m *in both languages*, so this is
pinned rather than trusted.

### The four bounds, and the corpus that owns them

Every figure below was measured over `test_conformance.py`'s own corpus: **10 shapes x 5
origins x 5 bearings x 196 fraction pairs x both signs = 98,000 probes**, with the shapes
spanning **1.5:1 to 250:1 aspect ratio in both orientations**.

| Constant | Value | Worst measured | Shape | Origin | Bearing | Headroom |
|---|---|---|---|---|---|---|
| `FEATURES_REACH_MAX_ULPS` | 1 ULP | exactly 1 ULP, at `(24628.73974506011, 42633.3696821233)` | -- | -- | -- | none, deliberately |
| `FEATURES_WEIGHT_MAX_ABS` | 2.2e-14 | 1.082467e-14 | 10000x40 | (-89.9, -170.0) | 143.5 deg | 2.03x |
| `FEATURES_AUTHORITY_MAX_ABS` | 2.2e-14 | 1.082467e-14 | 10000x40 | (-89.9, -170.0) | 143.5 deg | 2.03x |
| `FEATURES_RESULT_MAX_ULPS` | 32768 | 14,080 ULP | 10000x40 | (-89.9, -170.0) | 143.5 deg | 2.33x |
| `FEATURES_RESULT_MAX_ABS` | 1.8e-10 m | 8.776624e-11 m | 10000x40 | (-89.9, -170.0) | 143.5 deg | 2.05x |
| `FEATURES_MARK_DISTANCE_MAX_ULPS` | 2 ULP | exactly 2 ULP, over 120,000 mark distances | -- | -- | -- | none, deliberately |

`shaped_metres` is asserted as `close_enough(..., 32768) or abs(...) <= 1.8e-10` because
neither half covers the range alone: where the result cancels to zero the ULP measure is
meaningless, and where the elevation is kilometres an absolute bound is weak.
`FEATURES_AUTHORITY_MAX_ABS` equals `FEATURES_WEIGHT_MAX_ABS` and is deliberately **not**
the same constant -- at every worst case the `smooth(|lift| / SETTLE_M)` factor had
saturated to exactly 1.0, so authority was carrying the weight's error and nothing else. If
`smooth` were ever dominant they would part company, which is only visible because they are
asserted apart.

**These bounds have a measured envelope, and this is what the next slice most needs. They
are validated over a corpus spanning 1.5:1 to 250:1 in both orientations, they hold
empirically to about 500:1, and beyond that they fail on the unmutated engine.** Measured
by extending only `FEATURE_SHAPES`:

    30x12000    (400:1)   weight 2.847722e-14, result abs 2.314664e-10   -- over two bounds
    40000x40   (1000:1)   weight 4.252154e-14, 55,296 ULP, abs 3.453806e-10
                                                             -- over all three

**A feature beyond that envelope needs these bounds RE-MEASURED, never scaled.** The
mechanism is catastrophic cancellation in `across = east * across_e + north * across_n`,
amplified by `along_m / width_m` -- and that amplification grows without limit as the aspect
ratio grows, so **no finite corpus makes these bounds universal.** Extending the corpus to
500:1 would relocate the same cliff to 800:1 and buy nothing. A bound that implied
universality would be the real defect; a bound with a stated envelope is honest, which is
why the envelope is stated here rather than the corpus enlarged again.

The mechanism is confirmed by bearing rather than argued: at 0, 90 and 270 degrees one of
the two terms of `across` is exactly zero and there is nothing to cancel, and the worst
weight divergence is 4.440892e-16 at each; off-axis it is 1.082467e-14 at 143.5 degrees,
twenty-four times worse. It is confirmed by shape too -- 40000x12000 (3.3:1) sits at
4.44e-16 while 10000x40 (250:1) sets the bound. **Aspect ratio is the axis; size is not.**
An earlier version of these constants capped the corpus at 4:1, landed on `1e-15` and
`1024`, and an ordinary 5 km x 30 m dredged approach channel -- one shape substituted,
engine unmutated -- failed two of the sixteen tests outright. The current bounds were
genuinely re-measured rather than scaled off those: the ratios are 22x, 22x, 32x and 40x,
and the headroom moved independently per bound.

### A known divergence: `marks_near` membership can reclassify across languages

`distance_m` is bounded at 2 ULP and feeds `distance <= within_m`, which is a **discrete**
output. No bound fixes a discrete output, so this is recorded rather than tolerated away.

**The condition matters more than the rate.** It happens only when `within_m` is derived
*from* a computed distance -- "everything at least as close as that rock" -- so the caller
is standing exactly on the comparison's edge. Measured over the conformance corpus (400
marked features, 300 probe points):

- **174 of 1,800 (9.67%)** boundary cases -- 300 points x the six nearest marks to each --
  return a different set from the engine than from the Python, in every observed case one
  fewer, because the engine's distance for the boundary mark came out fractionally larger
  than the Python value being used as the threshold.
- **0 of 3,900** round radii and **0 of 3,900** random radii reclassify. A caller passing a
  radius it chose is unaffected.

Ordering never diverges: the smallest gap between two adjacent mark distances measured
**20,709,884 ULP (0.0386 m)**, ten million times the 2-ULP bound, and the nearest a mark
came to a round `within_m` was **3.17 m**. Both are asserted, so the margin is measured
rather than believed. The rule for callers is one line: **do not compute `within_m` from a
`distance_m` obtained from the other language.**

### A warning for `substrate.py`: these bounds are not yours to borrow

`substrate.py` calls `weight_at` directly, and it will be tempting to reuse
`FEATURES_WEIGHT_MAX_ABS` when it is ported. **Do not.** `weight_at` and `authority` are
bounded **absolutely** rather than in ULP, and that is a measurement, not a preference: at
`bump`'s support edge the weight is a smoothstep evaluated on a quantity going to zero, so
it cancels to 1e-31 and below, and there a single ULP of `along` is the entire value. Worst
ULP divergence over the same 98,000 probes, bucketed by how large the weight actually is:

    weight >= 1e-3              2,517 ULP
    weight >= 1e-6             76,326 ULP
    weight >= 1e-12        32,642,720 ULP
    every point               4.19e18 ULP  -- i.e. no bound at all

(Those are this corpus's numbers, re-measured for this section. The smaller figures of
145 / 6,239 / 725,675 / 1.8e16 in `FEATURES_WEIGHT_MAX_ABS`'s docstring were measured on
the earlier 4:1-capped corpus and were not re-measured when it was widened to 250:1; the
conclusion they support is unchanged and only strengthened.) The same edge produces **312**
points where one language returns exactly `0.0` and the other returns up to
**8.617674e-29** -- infinitely many ULP apart and physically indistinguishable from
agreement. That census is pinned to 312, so a real divergence could not hide among them,
and none of the 312 are gate-rejected: this is `bump`'s support edge, not the reach gate.

A borrowed bound is exactly how a real defect survived in a previous slice. `shelf.rs`'s
first pass took `TECTONICS_BOUNDED_MAX_ULPS` (8192) wholesale; an algebraically-equal
rewrite of the blend diverged `elevation_m` by **203 ULP** and sat comfortably inside it,
green. **A borrowed bound admits whatever the lending module admits.** Measure
`substrate.py`'s own quantity, over its own corpus, with a high-aspect-ratio feature in it.

### `substrate` is still unread inside the crate

Nothing in `crates/worldbuilder-engine/src/` reads `Feature::substrate`: the struct field,
the binding's `substrate: substrate.clone()`, and some `None` initialisers are all of it.
So no engine behaviour can observe a flattened sentinel, and `features_round_trip` --
which hands each feature's `kind` and `substrate` back to Python -- is its **only**
observer. It exists for that reason alone. Without it, a binding writing
`substrate.clone().unwrap_or_default()` would compile, pass every test, and turn `None`
("derive the bottom from the shape of the ground") into `""` at the exact moment
`substrate.py` arrived to depend on the difference. Both mutations -- flattening the
sentinel, and dropping the field -- were confirmed caught.

### The throwaway is gone

Task 1's `tests/test_features_gates.py`, which measured the gate behaviour against the live
Python before anything in the port depended on the answer, is deleted as of this section.
Its 14 tests are gone; what they found lives here and in `test_conformance.py`'s permanent
measurements instead.

Every count in this section was verified by running the suites and checking exit status, not
copied from a report. `cargo test --release` exits 0 at **187** tests (177 lib, 4
`blake2_bytes.rs`, 6 `no_std_math` guard), unchanged by this task, which added no Rust test.
`pytest tests/test_conformance.py` exits 0 at **114**, also unchanged -- the deleted spike
never lived there. The full Python suite drops from 368 to **354** (346 outside
`tests/test_performance.py`, plus that file's 8), the fall being exactly the 14 tests deleted
with the spike. `tests/test_performance.py` is separately known to be unreliable: it compares
two wall-clock chart timings on about a 3% margin, it has been observed to fail at parent
commits and in isolation, and it is out of scope here. It happened to pass on the runs above;
a failure there is pre-existing and is not this section going red.
