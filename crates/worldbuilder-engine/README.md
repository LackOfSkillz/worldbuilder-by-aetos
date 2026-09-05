# worldbuilder-engine

The generator core. One implementation, compiled twice: natively for Evennia and maritime
through Python bindings, and to WebAssembly for the browser studio.

Slice 0 measured that those two targets agree bit-for-bit over 5,000,000 samples, with a
negative control proving the comparison could detect a one-bit difference. That is the
foundation this crate is built on; see `spikes/0-bit-equality/README.md`.

## What is here so far

Counted from `src/`, not from memory: **sixteen modules plus the crate root**, every one of
them ported from a named Python module and held to that module by `tests/test_conformance.py`.

    src/lib.rs       the crate root: the module tree and the PyO3 module registration
    src/detmath.rs   the only place a transcendental is called
    src/vectors.rs   Vec3
    src/sphere.rs    SpherePoint
    src/noise.rs     Noise: 64-bit lattice hash, trilinear sample, fBm
    src/tangent.rs   TangentFrame: at, local_to_sphere, sphere_to_local
    src/continentality.rs  Continentality: at, calibration, above_shore, base_elevation, gradient
    src/plates.rs    Plate, PlateSet: the bisector table and the nearest-two Voronoi lookup
    src/generation.rs  plates_for: every pole, rate and centre hashed, never drawn
    src/kinematics.rs  surface_velocity, motion_at, motion_between: the boundary regimes
    src/tectonics.rs   Tectonics.offset_m: what plate motion does to the ground
    src/detail.rs      Detail: amplitude_m and offset_m -- texture, and only texture
    src/shelf.rs       Shelf, Coastal: the water a ship actually sails in
    src/features.rs    Feature, Features, Placed: RAISE / CARVE / SHAPE, in list order
    src/substrate.rs   what the bottom is made of, and the Composition it returns
    src/surface.rs     the whole world assembled: structural_m, elevation_m, bottom_at
    src/bindings.rs  the PyO3 surface, conversion only

That list is the engine core, and it is closed -- see **This closes the engine core** below.
Keep it in step with `src/` when a module lands: it went seven modules stale across the
slices that added them, on the same page that claims the core is complete.

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

**This envelope is deliberately duplicated into all four constants' docstrings, and the
duplication is the point.** A README is a file somebody may not open; the docstring is what
is on screen when the number is read and when it is tempting to reuse. Keep the two in
sync -- if the envelope is ever re-measured, it is five edits, not one.

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

(Those are this corpus's numbers, re-measured for this section, and
`FEATURES_WEIGHT_MAX_ABS`'s docstring now agrees with them figure for figure. It briefly did
not: an earlier draft of this paragraph recorded that the docstring still carried
145 / 6,239 / 725,675 / 1.8e16 from the 4:1-capped corpus, and that was true when it was
written and false one commit later, when the docstring was re-measured. The old figures
survive there only as a parenthetical history note, which is where they belong -- the
collapse is worse than first recorded, so the case for bounding this absolutely is
strengthened rather than weakened.) The same edge produces **312**
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

## `substrate.rs`: the module with no type, one bound, and a rate that has been narrowed five times

`worldbuilder/bathymetry/substrate.py` answers the second thing maritime asks of a world.
Depth is the first; what the bottom is made of is this. An anchor bites in mud and drags on
rock, a hull that touches sand is aground and one that touches rock is holed, and a dredger
can move one and not the other. The field is a **composition** -- three fractions that vary
smoothly -- and the single-word answer is whichever is largest.

### There is no `Substrate` type in the crate, and no host trait either

Python builds `Surface`'s five layers and hands `self` to `Substrate` on the **last line**
of `__init__` (`surface.py:67`); `Surface.bottom_at` then calls back in at `:132`. A Rust
`Substrate<'a>` holding `&'a Surface` could not be a field of that `Surface` -- that is a
self-referential struct, and every way out of it (`Rc`/`Weak`, `Pin`, raw pointers,
`ouroboros`) buys lifetime machinery for a type that **holds no state at all**. The Python's
own docstring says so: *"Holds nothing. Every answer is computed from the surface it was
given."*

So the port is **free functions plus an on-demand borrow: no trait, no stored field.** A
`HostSurface` trait was considered and rejected -- `Surface` still could not store the view,
so `surface.rs` would build one per call anyway, paying all of the same cost plus a
four-method trait with exactly one implementor.

**The host surface is exactly four members wide, not `Surface` wide.** Task 1 measured it at
runtime with an attribute-recording proxy, over every entry point crossed with all eight
combinations of `at`'s three optionals, then cross-checked the census against `grep`:

    surface.radius_m                  slope_at, once
    surface.structural_m(point)       slope_at, four probes; at, a fifth when elevation is None
    surface.tectonics.offset_m(point) at, when tectonic_m is None
    surface.features.placed           at

**Six members that look reachable are genuinely unreached**: `shelf`, `detail`, `land`,
`plates`, `elevation_m` and `bottom_at`. Confirmed three ways -- absent from the runtime
census, absent from `self.surface.` in the source, and the spike asserted
`hasattr(surface, member)` *before* asserting non-reach, so a typo could not masquerade as
an absence. This is load-bearing rather than trivia: substrate's elevation channel is
`structural_m`, the feature-shaped ground, and it therefore never sees `detail` -- which is
exactly what `SLOPE_BASELINE_M` relies on when it says a 60 m baseline costs nothing.

So `slope_at` takes a `&dyn Fn(&SpherePoint) -> f64` for the host field, `at` takes
`&Features` concretely (`Placed` and its reach gate are already ported and nothing should
stand between this module and them), and `dominant_at`'s `**known` becomes three
`Option<f64>` in the order `at` resolves them -- `elevation_m`, then `tectonic_m`, then
`slope`. That order is observable, because each `None` triggers a different host call.

### The module is STRICT everywhere but `slope_at`

`natural`, `Composition::new`, `blended_towards`, `dominant`, `holding` and `smooth` reach
**zero transcendentals** -- clamps, a smoothstep, a sum, three divisions, three comparisons
-- so every test on them compares raw bits with no tolerance at all.

`slope_at` alone reaches `hypot`, five times: once itself and once inside each of four
`local_to_sphere` calls. That is the one call where the two languages run genuinely
*different algorithms* -- since 3.8 CPython does not call the platform `hypot`, it computes
its own Neumaier-compensated norm -- so bit-equality was never something either side
promised. It carries the module's one bound.

### A strict test caught a real defect -- and it was not the test that got the credit

The first `substrate_blended_towards` binding passed its receiver and its target through
`Composition::new` before blending. Python does not: both are **already-constructed
instances**, each divided by its own total once, and `blended_towards` reads them as they
stand. Rebuilding divides a second time, and a real composition's fractions do not sum to
exactly 1.0. The divergence was **`0.2781153660496104` against `0.27811536604961046`** --
one ULP, in a comparison with nothing in it to absorb one.

**Any tolerance at all on `blended_towards` would have left that green.** The fix is that
both triples now cross the FFI verbatim as fields; `substrate_composition` is the one
binding that exposes the normalising constructor. Those are the only two `Composition`
construction sites in `bindings.rs`, and each uses the form it needs.

**The credit was misattributed in three docstrings, and re-mutation is what found it.**
The catching test was
`test_the_weight_zero_guard_is_bit_observable_and_its_rate_needs_a_named_convention`,
whose population is the demonstration coast's own grid -- which is where those two
quoted values come from.
`test_substrate_blended_towards_agrees_bit_for_bit_including_weight_zero`, which claimed
the catch, **could not have made it**: restoring the defect left it green, because 20 of
its 21 corpus triples normalised to fractions summing to exactly 1.0, on which a second
normalisation is the identity. A census put the sensitive fraction of that corpus at
**0 of 20**.

The corpus now carries `(3.0, 2.0, 1.0)`, whose normalised total is `0.9999999999999999`,
added for exactly this reason. With it in the list the same mutation moves **42 of the 45**
target-and-weight combinations that triple is swept against, and the test goes red. **A
strict comparison is only as strong as a corpus that can express the defect** -- strictness
without a sensitive input is a tolerance by another name.

### The bounds, both asserted two-sided

    SUBSTRATE_SLOPE_DRIFT_REL           2.3e-16   slope_at alone, RELATIVE
    SUBSTRATE_AT_DERIVED_SLOPE_MAX_ABS  6.6e-16   at/dominant_at with slope derived, ABSOLUTE

Both are asserted `bound/2 <= measured <= bound`, not merely `<= bound`. A test that checks
only a ceiling ratchets loose for free and passes more comfortably as the code degrades; a
two-sided assertion means a later *widening* fails rather than passing quietly.

**`SUBSTRATE_SLOPE_DRIFT_REL` is HOST-CONDITIONAL and that fragility is the point.** One ULP
holds only because `local_to_sphere` agreed bit-for-bit at every measured point on this
host, so none of the drift comes from the probe positions -- and probe positions are one
`cos`, `sin` or `sqrt` away from a host whose libm differs from CPython's. Nudging one
component of the query point by a single ULP moves the answer by up to **2.433094e-09
relative** over the pinnacle grid, seven orders above the bound. **A differing host requires
the bound RE-MEASURED, never widened.** Widening hides exactly the divergence the bound
exists to detect.

**Do not carry `SUBSTRATE_SLOPE_DRIFT_REL` across the elevation-field boundary.** Every
comparison that produces it drives *both* sides from the same `structural_m` -- the Python
surface's, handed to the engine as a callable. Driving the engine's `slope_at` with the
port's own elevation field instead moves the answer by up to **2.217618e-12** relative,
**9,642x** the bound, because the ported elevation itself differs by up to **1.847411e-13
m**. That drift belongs to `shelf.rs` and `features.rs`. Measuring both ports at once
measures their sum and can attribute it to neither. It is the wrong bound for that
comparison, not a defect in either.

**Those two numbers have now been wrong twice, and these are re-derived rather than
copied.** Method, so the next reader can check rather than trust: the port's field
reconstructed exactly as `Surface.structural_m` defines it --
`features_apply(tuples, x, y, z, shelf_evaluate(...)[0], radius_m)[0]` on the demo world,
22 plates, `WORLD_SEED`, `land_fraction` 0.29 -- confirmed against the Python's to the bit
at the first grid point (`-23.087591258514475`), then swept over both this section's
corpora. Pinnacle grid: 2.217618e-12 relative. Open water: 1.175566e-12 relative, and the
larger elevation difference, 1.847411e-13 m. The earlier record of 7.968304e-11 and
3.07e-12 m is 36x and 17x too large and does not reproduce by any method recoverable from
what was written down. **The conclusion is untouched** -- four orders of magnitude is still
emphatically the wrong bound for a comparison crossing the elevation-field boundary --
which is why the figure survived two wrong values without anyone noticing, and why a figure
that no test asserts still has to name its method.

And for the same reason `features.rs`'s `FEATURES_WEIGHT_MAX_ABS` is **not** borrowed here,
even though `at` calls `weight_at` -- see that module's own warning, which this section
honoured.

### `at` with all three optionals supplied is STRICT -- and fragile in one direction

With `elevation_m`, `slope` and `tectonic_m` all handed in, `at` reaches no transcendental
of its own. A tolerance would still have been defensible, because `weight_at` *is* bounded
(`atan2` and `hypot` inside `sphere_to_local`). Measured over **4,682 points** -- the 3,721
of the pinnacle grid plus the 961 of the open water -- against all **25** placed features of
the demo coast, the divergence is **exactly zero in every one of the three fractions**. So
that test asserts raw bits, and the strictness is a finding rather than an assumption.

**The caveat is no longer a caveat: it was measured, and STRICT is a property of THIS
CORPUS rather than of `at`.** The demo coast has no 250:1 dredged channel probed at its own
support edge -- the shape that sized `FEATURES_WEIGHT_MAX_ABS` in the previous slice -- and
against one the strict assertion does not hold. Four high-aspect shapes (10000x40, 40x10000,
5000x30, 30x5000 m), each from the five `FEATURE_ORIGINS` on the five `FEATURE_BEARINGS`,
every optional still supplied:

    39,200 probes   14 FEATURE_FRACTIONS as ordered pairs, both signs
        1.0824674490095276e-14   10000x40 m, (-89.9, -170.0), bearing 143.5, (-0.4, -0.4)
    372,100 probes  61x61 fraction grid over [-1.3, 1.3], same shapes and frames
        2.020605904817785e-14    10000x40 m, (-33.0, 151.0), bearing 143.5,
                                 (0.4333333333333334, 0.4766666666666667)

Both exhaustive grids, no bisection, and **the figure moves 1.9x with the grid alone** over
the same shapes -- which is why the search is named beside each one. Zero `dominant` flips
in either sweep. All of it is `weight_at`'s: the coarse figure is the very probe that sized
`FEATURES_WEIGHT_MAX_ABS` (1.082467e-14), and the fine one is 0.918x that bound. The port
is not wrong; the *claim* was too wide.

**The right response is a bounded case of its own, never a tolerance on this test, and the
reason is specific.** Mutating `at`'s `weight > 0.0` guard to `weight >= 0.0` shifts the
answer by ~2e-19 absolute -- inside `SUBSTRATE_AT_DERIVED_SLOPE_MAX_ABS` (6.6e-16) and
inside `FEATURES_WEIGHT_MAX_ABS` (2.2e-14) alike. The strict test is the **only**
corpus-scale detector that catches it; the bounded derived-slope test passes under that
mutation. Widening this assertion to admit a dredged channel would buy coverage of the
channel at the price of the guard's only detector. So the section header's claim is scoped
to the corpus it holds over, and the channel keeps its own bound.

### `dominant` returns a WORD, so nothing can absorb a flip

Tie precedence is **ROCK > SAND > MUD**, each an independent comparison in the Python's exact
directions: rock wins when it is at least sand *and* at least mud, so a three-way tie is
rock; otherwise sand wins when it is at least mud, so a sand/mud tie is sand. Re-measured
live: `(1/3,1/3,1/3)` -> rock, `(0.5,0.5,0.0)` -> sand, `(0.0,0.5,0.5)` -> rock,
`(0.5,0.0,0.5)` -> rock.

**A one-ULP nudge off a tie does not always change the word**, which is the trap that failed
the first tie test written here. `Composition::new` divides all three fractions by their
total, and *the total moves with the nudge*: at `(1.0, 1.0, nextafter(1.0, -inf))` the three
quotients come back exactly equal and the answer is still rock. So "one ULP off a tie" is
not a usable probe of `dominant` without going through the constructor first. The tests now
*search* for the smallest nudge that survives normalisation, require the engine to step off
the cliff at exactly that point, and require one ULP back the other way to stay rock on both
sides. Both languages agree throughout -- a property of the algorithm, not a divergence.

### The `PURE` lookup raises, on a value the port guarantees can arrive

Python's `PURE[declared]` is a `dict` lookup that raises `KeyError` on any word that is not
`sand`, `mud` or `rock` -- **and the empty string is such a word.** The conformance suite
already pins that `substrate=""` survives the FFI crossing *distinct from `None`*, so this
is not hypothetical: it is a value the port guarantees can reach `at`.

**Ruling: the Rust surfaces a typed error and both sides fail.** `UnknownSubstrate` crosses
as `UnknownSubstrateError`, a `KeyError` subclass, so a caller handling one handles the
other. Silently continuing past the miss -- `if let Some(c) = pure(declared)` and on to the
next feature -- would answer where the Python refuses to, and disagreeing about whether an
answer *exists* is the worst divergence this module could carry. `" "`, `"Rock"` and
`"gravel"` are pinned the same way, and the refusal is conditional on the weight on both
sides: an out-of-reach feature declaring nonsense raises on neither.

`declared is None` is **not** the same case. That is a genuine skip -- the feature has
nothing to say about substrate and the Python `continue`s before it even asks for a weight.
Note that **all 25 demo features declare a substrate**, so any corpus meaning to cover the
skip needs a feature that omits `substrate` deliberately; the suite uses a fixture host for
exactly that.

### "The composition sums to one" is FALSE and banned, and the ban is executable

The pre-normalisation total is **`0.9999999999999998` at its minimum -- two ULP below one**.
Re-measured live over the 1,001 x 1,001 `(rock, swept)` grid, and reachable from `natural`'s
own arguments at elevation -119.8 m, slope 0.0025666666666666667. `loose*swept +
loose*(1-swept)` does not re-sum to `loose` in floating point.

An earlier extraction argued the total is exactly 1.0 and was wrong. The consequence is
observable: the normalising division in `Composition::new` is **not** a no-op, and must
never be skipped, simplified away, or asserted around.

**This ban is executable rather than editorial.**
`blended_towards_cannot_skip_compositions_normalising_division` blends
`natural(-119.8, 0.0025666666666666667, 0.0)` towards `Composition::new(0.7, 0.1, 0.2)` at
weight 0.1 -- a pair whose raw total is `1.0000000000000002`, so every field moves by exactly
one bit under normalisation. Replacing the constructor with a raw struct literal was proven
by mutation to turn the suite red.

### The `weight > 0.0` guard is bit-observable, and its rate is a property of the sampling

`at`'s `if weight > 0.0` is not a shortcut. Blending at weight exactly zero is not the
identity, because `blended_towards` re-enters the normalising constructor and fractions whose
total is not exactly one move under it. The guard must be transcribed, not simplified away.

**The rate needs its full sampling convention -- the FRAME, the STEP and the SPAN, all
three.** It has now been narrowed five separate times and each narrowing was correct. Under
the demonstration coast's own frame, `Coast.at(offshore_m, along_m)` centred on the anchor,
every reading below re-measured live for this section:

    61x61, 1,500 m per step (span +-45,000 m)    67/3,721  = 1.80%  abs 2.220446e-16  rel 1.249555e-15  11 ULP
    61x61, span +-1,500 m (50 m per step)       185/3,721  = 4.97%  abs 2.220446e-16  rel 1.887498e-15  13 ULP
    121x121, 750 m per step                     313/14,641 = 2.14%  abs 2.220446e-16  rel 1.264549e-15  11 ULP
    61x61, 300 m per step                        21/3,721  = 0.56%  abs 1.110223e-16  rel 3.915580e-16   2 ULP
    41x41, span +-250,000 m                       5/1,681  = 0.30%  abs 1.110223e-16  rel 2.171242e-16   1 ULP
    5,000-point jittered scatter, +-45,000 m    101/5,000  = 2.02%  abs 2.220446e-16  rel 1.141701e-15   8 ULP
    open water 31x31                             11/961    = 1.14%  abs 2.220446e-16  rel 1.166550e-15   7 ULP
    pinnacle 61x61, +-140 m                       0/3,721  = 0.00%  -- NOTHING SHIFTS AT ALL

Only the first is asserted, because it is the only reading whose convention the suite fully
pins. A `TangentFrame.at(region.origin)` grid gives different counts again, and seven further
conventions a reviewer tried gave counts from 18 to 159. **The rate is not a property of the
module.** Zero dominant flips under any of them.

**The relative figure and the ULP distance belong to ONE reading and must never be quoted
bare**: 1.249555e-15 / 11 ULP on the first, 1.887498e-15 / 13 ULP on the second, down to
2.171242e-16 / 1 ULP on the widest span.

**And the absolute figure is a CEILING, not an invariant -- this is the fifth narrowing, and
it is new in this section.** One machine epsilon, `2.220446049250313e-16`, is the worst
absolute shift under every convention tried and none has exceeded it; but three of the eight
above bottom out at *half* of it, and the pinnacle grid observes no shift at all. So the
honest statement is "nothing tried exceeds one epsilon", not "one epsilon is what you will
measure" -- and a corpus that is a small steep feature cannot show this guard is live in the
first place. The worst-absolute figure is still the only one worth carrying between corpora,
and even it is an upper bound rather than a reading.

### The slope clamp is not dead code, and only a 2-D scan of a small steep feature shows it

On the demo world's 140 m pinnacle at `Coast.at(8_000, 6_500)`, against `ROCK_SLOPE = 0.04`:

    61x61 grid, +-140 m, 4.667 m/step    0.3252142109022925   8.1304 x ROCK_SLOPE
    61-point diagonal line               0.3119559807440774   7.7990 x
    61-point east-west line              0.3042234484625276   7.6056 x
    61-point north-south line            0.3014451002052766   7.5361 x
    400-point planetary scatter          0.0143501550330470   0.3588 x -- reads DEAD

The test asserts the grid clears 8x **and that all three line directions do not**, so
"resolution does not rescue a line, a second dimension does" stands on measurement rather
than on a docstring. The steepest ground is off the feature's axis, because a feature's
weight is a product of two `bump` factors.

### Two rules this slice earned, which the next slice should inherit

**1. A measurement is a property of its corpus AND its method.** Neither alone.

*Saturation questions need a small steep feature scanned in 2-D.* A planetary scatter never
reaches `natural`'s slope clamp -- 0.36x, reading as dead code -- and no line direction and
no density closes the gap to a grid: the same line gave 6.646421e-03 at 0.750 m/step and the
identical value at 0.188 m/step.

*Boundary-margin questions need gentle open water, which is the opposite corpus* -- and the
margin then differs by **eleven orders of magnitude** depending on whether it is found by
grid or by bisection. Both figures are correct, and both were re-measured live for this
section:

    open water, 31x31 grid                    smallest dominant margin  7.485921e-04
    open water, bisected onto the crossover   smallest dominant margin  2.109424e-15

A grid samples where its nodes fall; a bisection samples where the boundary is. The 20-200 km
offshore ray crosses sand -> mud between 25,760 m and 25,850 m and bisects to sides
`3.637979e-12 m` apart still returning different words. The pinnacle grid's smallest margin is
2.557239e-03, and its bisection FLOOR -- over all four line directions through the pinnacle
(E-W, N-S and both diagonals), 601 samples over +-140 m, every one of the eight crossings
bisected on its own `Coast.at` coordinate to that coordinate's ULP, 9.094947e-13 m -- is
3.655076e-12. That is three orders *higher* than the open water's 2.109424e-15, a ratio of
1,733x, because exhaustion leaves roughly `gradient x last resolvable step`. **So the rule
"measure through a small steep feature" is right for the clamps and backwards for
`dominant`.**

A figure like this must name its search, not only its population -- and the figure it names
must be the FLOOR over that search, because the eight crossings spread over an order of
magnitude (3.655076e-12 to 3.516387e-11):

    east-west     sand->rock  1.285039e-11    rock->sand  3.655076e-12   <- the floor
    north-south   sand->rock  1.132805e-11    rock->sand  2.056244e-11
    diagonal+     sand->rock  2.922662e-11    rock->sand  3.516387e-11
    diagonal-     sand->rock  2.790879e-11    rock->sand  2.423395e-11

An earlier draft of this section quoted 1.29e-11 and called it four orders. That is the E-W
line's FIRST crossing -- one crossing of one direction, named as neither -- and the same line
bottoms out 3.5x lower on its second. The conclusion is unchanged at three orders; the number
was the sixth narrowing this slice has had to make to a figure quoted without its search.

**2. A rate needs its full sampling convention, and only what survives every convention may
be quoted bare.** The `weight > 0.0` rate is the worked example above: narrowed four times
before this section and a fifth time in it, with the relative figure, the ULP distance and
even the absolute ceiling each turning out to belong to a smaller claim than it first
appeared to.

### The throwaway is gone

Task 1's `tests/test_substrate_gates.py` -- 11 tests that measured the host census, the
`natural` guard and the tie margins against the live Python before anything in the port
depended on the answers -- is deleted as of this section. It carried one known unasserted
claim, a *printed* "no better at 3,200 points than at 800" for the pinnacle line scan, which
was deliberately left unfixed because the file dies here; the surviving form of that claim is
asserted in `test_conformance.py`, where all three line directions are required to fall short
of the grid's 8x saturation.

Every count in this section was verified by running the suites and checking exit status, not
copied from a report -- and one report figure did not survive that check. `cargo test
--release` exits 0 at **232** tests (222 lib, 4 `blake2_bytes.rs`, 6 `no_std_math` guard, 0
doc-tests), unchanged by this task, which added no Rust test; Task 5's report recorded 236 for
the same command, and 236 is not what the tree runs. `pytest tests/` exits 0 at **376**: 386
less the 11 tests deleted with the spike, plus the one added by the final fix round
(`test_substrate_at_over_high_aspect_features_is_bounded_and_search_dependent`, which pins
the high-aspect `at` divergence that until then lived in a docstring). Of those,
`test_conformance.py` is **136** and `test_performance.py` is **8**. That performance file is separately known to be
load-sensitive -- it compares two wall-clock chart timings on a narrow margin and has been
observed to fail on a busy machine and pass on an idle one. It passed on the run above, taken
on an otherwise idle machine; a failure there is a statement about the machine, not about this
branch.

## `surface.rs`: the module whose content is an ORDER, and the figure that took four agents to name

`surface.py` has no constants, no free functions and no arithmetic of its own. Every number
it uses is imported from the layer that owns it, and what it contributes is the sequence:
shelf, then features, then detail, with the detail amplitude sized off the shaped ground and
damped by the authority the features claimed. **So this slice tested structure rather than
tolerances.** Nothing here reorders into a last-bits difference; every physically possible
reordering moves the answer by METRES, and the two things worth asserting are exact.

Every figure below was re-derived on this host for this section, from the current Python and
the current crate, by running the code rather than by reading a report. Host **K2SO |
Windows-10-10.0.26200 | CPython 3.11.0 MSC v.1933**.

### The settled figure, and the four-way disagreement that produced it

**Applying the features before the shelf is worth `30.89228988262422 m`.** Method, in full,
because the figure is meaningless without it: the maximum over the **625-point `Coast.at`
demo grid** (+-45,000 m, 3,750 m step, seed 20260831, 22 plates, land fraction 0.29, the 25
demo features), of the **full-pipeline `elevation_m`** difference produced by running **the
shelf's lerp over the featured macro instead of the features over the shelf's output**. Host
K2SO, CPython 3.11.0. Re-derived here; `surface.rs`'s `structural_m` doc comment carries it
and is correct.

**The number was never the defect -- the sentence around it was, and that sentence is now
fixed.** The doc comment used to read "Handing `land.base_elevation + tectonics.offset_m`
here instead ... moves the answer by 11.4 m, and over a demo-coast grid by 30.89 m", with
nothing after it. That phrase describes a DROP, and it carried a DROP figure (11.43 m at the
probe) and a SWAP figure (30.892 m over the grid) in the *same clause*. Two independent
readers measured the DROP, found no statistic matching 30.892, and each concluded the doc
comment was wrong; it was not. `structural_m`'s doc comment now names both mutations
separately, attaches each figure to the one it measures, and states the SWAP figure's frame,
stage and mutation next to it. **This is the root cause of the whole four-way episode: one
English phrase naming two experiments.**

**The disagreement is the lesson, and it is sharper than the number.** Four agents measured
this independently, every one of them correctly, and reported four different values between
15.9 and 60.6 m. Nothing failed to reproduce -- all seven readings below re-derive to the last
digit. The reports differed because **two axes were unnamed**, and the second one had gone
unnoticed entirely:

- **The frame.** The grid is `Coast.at(offshore, along)`, so the square is **rotated by the
  demo coast's `SEAWARD_DEG = 296.49`** about the anchor.
  `TangentFrame.at(region.origin).local_to_sphere(east, north)` is the same span and the same
  step over a **different** 625 points, and a different 625 points is a different set of
  extrema.
- **Which mutation "features before shelf" names.** This is the half nobody had noticed, and
  it is worth twice as much as the frame:
  - **SWAP** -- the shelf lerps the *featured macro*. The two stages exchange places, both
    still run, and the same `weight` is used (`Shelf.weight` never reads the ground). **This
    is the mutation `surface.py` actually describes.**
  - **DROP** -- the shelf is *deleted*. The features compose onto
    `land.base_elevation + tectonics.offset_m` and that is the answer. A different
    experiment: a deletion wearing a reordering's name.

With both variables named, every measurement is correct and they stop disagreeing:

    30.89228988262422   Coast.at frame,  SWAP, full-pipeline   <- doc comment, plan, Task 1
    30.913586988571197  Coast.at frame,  SWAP, structure-only  <- the extraction
    15.968867104622605  TangentFrame,    SWAP, full-pipeline   <- the fix round
    59.87820565940812   TangentFrame,    DROP, full-pipeline   <- the Tasks 3-4 review
    59.70936673990978   Coast.at frame,  DROP, structure-only  <- Task 5
    60.53077243225693   Coast.at frame,  DROP, full-pipeline   <- Task 5
    29.4588 / 29.4619   Coast.at frame,  additive splice       <- a fifth reading

(The first six were re-derived for this section. The additive-splice pair is carried from the
settling review and is *not* re-derived here, so it is quoted to the digits that review gave.)

**So the rule this project already had is not enough.** A figure needs its population, its
method's parameters and its host -- **and which mutation it measures**. Four correct
measurements produced four different numbers because one English phrase named two
experiments. Naming the corpus would not have caught it; naming the mutation does.

Two consequences worth keeping:

- **Task 5's concern that `30.892` should be corrected was a false alarm.** It had measured
  DROP. Acting on it would have replaced a correct figure with one for a mutation `surface.py`
  does not describe -- the exact failure mode the "re-derive, never transcribe" rule exists to
  prevent, arriving from the direction the rule does not cover.
- The earlier note calling the grid-orientation recovery premature is **half-withdrawn**: that
  recovery was right about the frame and incomplete about the mutation, rather than wrong.

The `11.4 m` in the same doc comment is a *different* corpus **and a different mutation** --
it is the **DROP**, at `base_sensitive_probe` with this module's own two test features, not
the SWAP the 30.892 measures. That mismatch inside one sentence is exactly what broke, and
both halves are now labelled at the source. It reproduces: the test
`the_base_is_the_shelf_not_the_macro_elevation` pins the wrong base at `-88.90851322503084`
against a shaped answer 11.43 m away, and the macro elevation there (`-54.97828011320581`) is
32.7 m from the shelf's, which the feature weights damp to 11.43 m in the answer.

The centres are a fourth population and reverse the ranking again: at the 25 feature centres
SWAP is 45.39640578663347 m while DROP is 16.352444190481663 m structure-only and
9.340489258888558 m full-pipeline. A figure quoted without its mutation, its frame, its
reading *and* its population is a figure about nothing.

### The other three reorderings reproduce exactly

Same world, same rotated 625-point grid, canonical resolution, worst `abs` against the
shipped `Surface.elevation_m`. Re-derived rather than transcribed, and unchanged to the last
digit:

    dropping the authority multiply             11.744069415078535 m
    detail added under the features              5.463671791248579 m
    the amplitude sized off pre-feature ground   0.04541089914697238 m

- *The authority multiply.* `amplitude` is damped by `1 - authority` **after** `amplitude_m`
  sized it and **before** `offset_m` spends it. A harbour dredged flat that still carries
  thirty metres of texture is not dredged.
- *Detail under the features.* Detail is added to `shaped`, so features compose against clean
  structure. Rough first feeds noise into the composition gates and lets a `RAISE` argue with
  a texture peak.
- *The amplitude's base.* `amplitude_m` is handed `shaped`, not `reading.elevation_m`. The
  smallest of the three and the easiest to write by accident, because `reading.elevation_m`
  is right there in scope.

On the unrotated frame the same three are 12.99419703036759, 9.34686947538938 and
0.08414582170664175 m; at the 25 centres they are 12.466044684569352, 7.482166393576634 and
**0.0006499904740300266** m. That last pair is the two-orders disagreement between the
populations, and it is why both are carried: the centres collapse the smallest reordering by
seventy times, because `authority` is exactly 1.0 at 24 of 25 of them.

### The two exact invariants, both raw-bit, both checked in both languages

1. **With nothing placed, `structural_m` IS `shelf.elevation_m`** -- re-derived at
   **650 of 650** points (the 625-point grid plus the 25 centres), bit for bit.
2. **`elevation_m(p) == structural_m(p) + detail.offset_m(p, amplitude, resolution_m)`**,
   where `amplitude` is `detail.amplitude_m(p, SHAPED, weight, tectonic)` damped by
   `1 - authority` -- re-derived at **9,750 of 9,750** checks (5 worlds x 650 points x 3
   resolutions), bit for bit.

Neither is cosmetic, and neither is a method checked against its own sibling. **Both are
checked against a pipeline reassembled from the separately bound stages** -- `shelf_evaluate`,
then `features_apply`, then `detail_amplitude_m`, then `detail_offset_m`, none of which knows
a `Surface` exists. A `Surface` that ran those stages in a different order, or handed one of
them a different argument, fails on bits rather than on a tolerance.

They are also the thing a perturbation has the hardest time slipping past. Multiplying the
answer by `(1 + 2.220446049250313e-16)` -- one machine epsilon, the smallest change that is
not a no-op -- is bit-visible at **650 of 650** points for the first invariant and **9,744
of 9,750** checks for the second (the six survivors are values where the scaled result
rounds straight back). Its worst absolute size is **2.842170943040401e-14 m** and
**2.2737367544323206e-13 m** respectively, and roughly four orders inside the 1e-9 relative
bound the Rust unit tests use for transcendental-carrying paths.

**Which bounded cross-language gates it sits inside, enumerated -- because an earlier version
of this paragraph claimed *every* one of them, listed three of the seven, and was wrong about
one of the three.** Each row is the perturbation's own worst on THAT gate's own population,
re-derived against the live Python at this commit:

    gate                                 value    perturbation there      inside?
    SURFACE_GRID_MAX_ABS_M               5.0e-13  2.2737367544323206e-13  yes
    SURFACE_CENTRE_MAX_ABS_M             1.0e-14  2.2737367544323206e-13  NO -- 22.7x over
    SURFACE_SCATTER_MAX_ABS_M            8.0e-13  9.094947017729282e-13   NO -- 1.14x over
    SURFACE_BOTTOM_GRID_MAX_ABS          1.2e-13  1.5681900222830336e-15  yes
    SURFACE_BOTTOM_CENTRE_MAX_ABS        3.0e-16  0.0                     yes
    SURFACE_BOTTOM_SMALL_GRID_MAX_ABS    3.0e-13  8.104628079763643e-15   yes
    SURFACE_BOTTOM_SMALL_CENTRE_MAX_ABS  4.0e-16  0.0                     yes

The four `bottom_at` gates sit on **fractions** rather than on metres, and a machine epsilon
of elevation barely moves a fraction, so the perturbation is comfortably inside them even at
3.0e-16 -- at both centre populations it does not move a single bit. The two it crosses are
elevation gates the earlier list omitted or mis-sized: the centres bound is the tightest in
the section and **22x smaller than the perturbation's own stated worst**, and the scatter
reaches deep-ocean and high-interior elevations an order larger than the demo coast's, so
eps x elevation crosses 8.0e-13 there.

**So "a tolerance test would not notice it" was false**, and here is what it should have
said. Measured by applying the perturbation to `surface.rs`, forcing `maturin develop
--release`, and running both suites by exit status:

    perturbation             cargo                   pytest
    structural_m x (1+eps)   101, 4 lib tests        1, 4 red
    elevation_m x (1+eps)    101, exactly 1 lib test 1, 3 red

The four are `test_surface_structural_is_the_shelf_bit_for_bit_with_nothing_placed`,
`test_surface_elevation_is_structure_plus_detail_bit_for_bit`,
`test_surface_structural_and_elevation_agree_at_the_feature_centres` and
`test_surface_honours_the_scalars_it_is_given_rather_than_their_defaults`; the three are the
same list without the first. The last two of them are **tolerance tests**, not raw-bit ones.

The raw-bit invariants do localise, and on the RUST side `elevation_m`'s perturbation kills
exactly one test -- that half of the original claim holds and is worth keeping. What did not
reproduce was the cross-language half. **The error was in the SAFE direction** -- the suite
is stronger than the record said it was -- which is exactly why it survived three readings,
and is the reason to state gates as a table with a measurement in every row rather than as a
sentence with three examples in it.

### The seed: ONE `world_seed`, an `i64`, cast at exactly TWO of three sites

The seed reaches three constructors and they do not agree on what it is.

- `Continentality::new` and `Detail::new` go through `Noise::new`, which **mixes first and
  masks second** (`noise.py:38`, `h = (h ^ (seed * K)) & MASK`). Only the low 64 bits of the
  mixed value survive, so a negative seed's masked result is exactly the wrapping `u64`
  result and `world_seed as u64` is exact. Measured over **2,049 negative seeds** through
  `_lattice`, `Noise.seed` and `Noise.at` -- 18,441 + 6,147 + 30,735 pairs, **0** bit
  mismatches -- and not a tautology: all 2,049 give a negative unbounded `Noise.seed` before
  the mask.
- `plates_for` does not mask. It keys a **decimal string** through `_fraction`:
  `generation.py:55` builds `"|".join(str(part) ...)` and `generation.rs`'s `joined_key`
  builds `world_seed.to_string()` the same way. `-5` and `18446744073709551611` are different
  keys and a different planet. Masking changed the plates in **64 of 64** sampled seeds; on
  the demo corpus the masked seed moves the ground by 824.7939561944431 m at worst and
  **267.7842618613704 m at its closest approach**, at 625 of 625 grid points.

So the signature stays `i64`, `plates_for` receives it unaltered, and the cast is bound once
to `noise_seed` and used at those two sites only. Casting it in one more place would build a
different world while looking like consistency.

The marker reads:

    let noise_seed = world_seed as u64; // cast-ok: two's-complement reinterpretation, not
    // a float truncation -- the mask comes AFTER the mixing, so nothing is rounded and
    // nothing is lost

It is **not** the crate's first `// cast-ok:` -- counted from source, there are **31**
marker lines in `src/` (12 in `generation.rs`, 6 in `noise.rs`, 5 in `continentality.rs`, 2
each in `features.rs`, `substrate.rs` and `surface.rs`, 1 each in `plates.rs` and
`shelf.rs`), and `noise.rs` already reinterprets a signed lattice coordinate as unsigned for
the same hash. What is new
is the derivation. It is the only marker in the crate whose reason rests on a *measured
population* rather than on an argument from the shape of the expression, and the only one
where the same cast applied one function further along would have been wrong. That is the
transferable part: "signed to unsigned is safe here" is a claim about a call site, not about
a cast.

**The domain narrows, and no 64-bit type avoids it.** Python `int` is unbounded and
`Surface(10**30)` is legal today; an `i64` represents exactly `[-2^63, 2^63)`. `u64` does not
help, because `plates_for` keys the decimal string: `plates_for(2**64 + 7)`,
`plates_for(10**30)` and `plates_for(-(2**63) - 1)` all differ from their masked forms, so no
64-bit representation reproduces any of them. A seed outside the range is a world this port
cannot build, and that is a stated limitation rather than a rounding. The binding raises
`OverflowError` at the boundary rather than masking silently, and `surface_fields` returns
`world_seed` so the domain is visible from Python.

### The indirect-call census: six, five of them structural

`bottom_at` costs **6** indirect calls. `structural_m` is called once for the elevation and
four more times inside `slope_at`'s finite difference; `tectonics.offset_m` is called once.
The count is pinned by a test with counting closures whose result must return the same bits
as `bottom_at`'s own, so it is counted rather than read off.

**An earlier census said four, and it was wrong in an instructive way**: it counted from
`slope_at` alone and forgot that `at` resolves the elevation *before* it asks for the slope.
Reading one function is not a census of what a function costs.

`slope_at` probes through `local_to_sphere`, the expensive frame direction, so the method
carries five `hypot` calls in all -- four in the probes and one in the rise-over-run. A
`weight_at`-shaped assumption about the tangent frame misses every one of them.

### Eight fields, not nine, and no `substrate` to reach through

Python's `__init__` ends with `self.substrate = Substrate(self)` and `bottom_at` is
`self.substrate.at(point)`. `substrate.rs` deliberately has no `Substrate` type -- a
`Substrate<'a>` borrowing its host cannot be a field of that host, and the Python's own
docstring says the thing holds nothing -- so `bottom_at` composes the free `substrate::at`
with callbacks over `&self`. **Eight fields here answer for Python's nine**, and a reviewer
counting fields against the reference should expect the gap rather than file it.

Nothing in the type needs `&mut self`. `noise.rs` dropped the Python's per-cell memo
deliberately, so both lattices are empty at rest and empty forever; the callbacks borrow
immutably and one `Surface` can be asked from several places at once.

`plates`, `land` and `tectonics` are **clones**, where Python shares one object -- the plate
table exists three times over. Every one is immutable and never written after construction,
so the copies cannot drift and no observation can tell them from Python's shared references.
Tens of kilobytes and a handful of memcpys, once per world, against a constructor that
already runs a 4,000-sample calibration.

### `bottom_at` returns a `Result`

Python's `bottom_at` returns a bare `Composition` because `PURE[declared]` raises a
`KeyError` and the raise propagates. Rust has no propagating raise, so the refusal is in the
type: `Result<Composition, UnknownSubstrate>`. At the binding it is mapped back to
`UnknownSubstrateError`, which subclasses `KeyError` -- the same refusal at the same place,
distinguishable from an unrelated dict miss.

### The `-0.0` case is closed by measurement, and it has THREE dependencies

The open question was whether `shaped == -0.0` can ever reach `Detail.offset_m`'s
`amplitude_m <= 0.0` guard. It cannot, and the closure is a measurement rather than an
argument: over 4,335 constructed single-feature evaluations, 95 produce a `-0.0` `shaped` and
348 fire the guard, and the intersection is **empty**; over 867 paired evaluations the guard
fires 867 times and `-0.0` never appears at all. On the real demo world the guard fires at
**0 of 625** grid points and **24 of 25** centres, and `-0.0` appears at neither. A
bisection hunt along `shelf.elevation_m`'s sign change never reached an exact zero.

**That third leg died with the spike and is re-derived here, with its method, because a
number without one is not a record.** Host K2SO, CPython 3.11.0, against the live Python at
this commit: over the demo world's 625-point `Coast.at` grid, `shelf.elevation_m` is exactly
zero at **0** points; then, from **25** alongshore lines at `Coast.at(offshore, along)` with
`along = -45,000 + line * 3,750` m, the sign of `shelf.elevation_m` is bracketed on the
offshore interval `[-4,000, +4,000]` m -- **19** of the 25 lines actually bracket it -- and
each bracketing line is bisected **60** times, for **1,140** probes. **No probe returned an
exact zero.** The closest approach is `-1.7763568394002505e-14` m, and its sign is negative,
which is the relevant detail: the hunt got within 1.8e-14 m of zero from *below* and still
never landed on `-0.0`. (Earlier prose called this a "1,000-point" hunt; 1,140 is the
measured probe count, and the 1,000 was a round number rather than a measurement.)

**Three things hold it shut, and any one of them reopens it.** This is the part worth
carrying forward: a closure is only as good as the list of things that would undo it.

1. **Every roughness constant in `detail.py` is strictly positive** -- `BARELY_M` 2.0,
   `CLEARLY_M` 4.0, `SHELF_M` 15.0, `COAST_M` 35.0, `ABYSSAL_M` 55.0, `INTERIOR_M` 80.0,
   `MOUNTAIN_M` 150.0. Over 71,190 evaluations spanning elevation, weight and tectonic,
   `amplitude_m` never returned `<= 0.0`; its minimum was **4.500000000000001**.
2. **`Features.apply` initialises `result = elevation_m` and `continue`s before the authority
   update.** The `weight <= 0.0` gate and the one-way `RAISE`/`CARVE` gates all `continue`
   above `result += weight * lift`, so a feature that contributes nothing writes nothing --
   and `result` therefore carries the caller's sign rather than a fresh product's.
3. **`shelf_weight` is confined to `[0, 1]`.** `rough * (1 - w) + SHELF_M * w` is a convex
   blend only on that interval; outside it the blend extrapolates and the floor is gone --
   measured, 66,594 of 200,000 draws with `w` in `{1 + 2**-52, 1.0000001, 1.5, 10.0, -1e-18,
   -0.5}` return `<= 0.0`, worst `-1200.0`. It holds because `Shelf.weight` is
   `seaward * coastal.breadth * authority` and all three factors are `_smooth` or
   `1 - _smooth` outputs (`shelf.py:164`, `shelf.py:227-235`).

### `resolution_m = 25.0` returns the same bits as `None`

`None` is not infinite detail: it evaluates every configured octave down to the canonical
minimum wavelength, `CANONICAL_WAVELENGTH_M = 250.0`. A resolution finer than that floor is
therefore not finer than canonical, it **is** canonical -- measured, in both languages, over
both populations, 650 of 650. `25.0` is carried in `SURFACE_RESOLUTIONS_M` for exactly that
reason, and `7500.0` is carried because it is coarse enough to drop octaves.

### The `isinstance(features, Features)` branch adopts a `Features` verbatim, radius and all

Python's parameter is one name carrying three cases. Rust has no runtime `isinstance`, so
they become `None` and `FeatureInput::{Loose, Built}`, which puts the branch at the call site
instead of leaving it to be discovered inside the constructor.

**The branches do not converge, and the difference is visible in the world.** The `elif` arm
re-places loose features at the *world's* radius; the `isinstance` arm adopts what it is
given and does not normalise it, so a `Features` built at 1,234,567 m keeps every tangent
frame and every `_cos_reach` at that radius inside a 6,371,000 m world. Measured: the same 25
features adopted from a 1,234,567 m `Features` differ from the same 25 placed at 6,371,000 m
by up to **82.39849253588422 m**, at **179 of 650** points. Making the branches converge
would be a fix to a bug the reference implementation does not have.

### The insensitive-argument trap, SIX times, and the rule that answers it

**A probe can be sensitive to a stage and still be flat in one of that stage's own
arguments.** This is the slice's most transferable lesson, and it turned up six times -- five
inside the slice, and a sixth that the final whole-branch review found and this branch closes
before the PR. The sixth is the one that generalises the other five, so it is worth reading
even if the first five look familiar.

1. & 2. **Two probes were constants.** At `deep_ocean` the shelf returns exactly `ABYSS_M`
   (weight 0.0, tectonic 0.0, elevation -4600.0 exactly) and the bottom composition is
   exactly `(0.0, 1.0, 0.0)`. A `Surface` wired to nothing at all reproduces both by accident.
   A stage contributing zero cannot show that stage is wired.
3. **A mutation corrupting `Continentality`'s seed passed the entire suite.** At
   `shelf_water`, `Tectonics::offset_m` returns `150.3860222420496` whether its
   `Continentality` was seeded from this world or from `noise_seed ^ 1`. The stage contributes
   +151 m there and is *constant in that argument*. "Every stage contributes at this probe"
   was the wrong question.
4. **A corpus with no variation in an argument.** Substituting the module's own
   `LAND_FRACTION` for the caller's `land_fraction` inside the constructor was invisible to
   all 12 surface tests then present and all 3,250 of their points, because every demo world
   passes exactly that constant. The same hole covered `radius_m`. A 120-point global scatter
   that *varies* both now closes it: a defaulted `land_fraction` is worth
   **1580.563522850889 m at 104 of 120** points, a defaulted `radius_m`
   **482.09015395480674 m at 59 of 120**. That corpus exists because the mutation survived
   everything else, and `test_conformance.py`'s surface section carried 13
   `test_surface_*` tests from that point rather than 12.
5. **`plate_count` was a THIRD argument the corpus could not see. It is now closed the same
   way.** Defaulting `plate_count` to `DEFAULT_PLATE_COUNT` inside `cached_surface` and
   deselecting one test -- `test_surface_fields_round_trip_including_the_adopted_radius`,
   whose witness is a *structural* echo of `surface.plates.len()`, not a value comparison --
   left every remaining surface test green, because no value corpus varied it: all five demo
   worlds are built at 22 plates. It was a straight repeat of instance 4 on the one
   constructor argument that instance 4's scatter did not vary.

   The fix is the same fix: a third row in
   `test_surface_honours_the_scalars_it_is_given_rather_than_their_defaults`, a **seven-plate
   world** over the same 120-point global scatter. A defaulted `plate_count` is worth
   **905.6679021350784 m at 37 of 120** points, and the seven-plate world's own
   cross-language worst is **1.136868e-13 m**, comfortably inside `SURFACE_SCATTER_MAX_ABS_M`
   (8.0e-13). Seven was chosen by measurement rather than taste: plate counts of 17 and 29
   reach **2.2737e-12** and **7.9581e-13** against the Python and would each need their own
   bound; 7, 11 and 23 all sit at 1.1369e-13. Proved by mutation -- with the defaulting
   applied, this test goes red on its own.

   It also settles the case for `surface_fields`: `plate_count` comes back from that binding,
   which is the only *structural* witness a caller has that the argument arrived at all.

6. **The same argument again, on paths nobody had crossed it with: `radius_m` reaching
   `Features::new` and `substrate::*`.** Instance 4 closed `radius_m` at the constructor with
   a 120-point global scatter, and the section then read as though the argument were done.
   It was not. **Six** substitutions of the literal `crate::sphere::EARTH_RADIUS_M` for the
   surface's own `radius_m` each passed `cargo test` AND the whole pytest suite: both live
   arms of `Features::new`, and `substrate::at`, `substrate::dominant_at` and
   `substrate::slope_at` at each of the four sites `surface.rs` calls them from.

   Two `surface::tests` stayed green under the mutation that makes **their own names false**
   -- `no_features_is_an_empty_features_at_the_world_radius` and
   `loose_features_are_placed_at_the_world_radius` both asserted
   `features.radius_m == EARTH_RADIUS_M` on a world built at Earth's radius, where "the
   world's radius" and "Earth's radius" are the same number and the assertion cannot tell
   them apart. The loose arm is not inert: the same feature placed at 1,234,567 m instead of
   6,371,000 m is worth **82.39849253588422 m**.

   **The cause is not an unvaried argument. It is an empty CROSS.** The scatter varies the
   radius but calls only `surface_structural_m` and `surface_elevation_m`, so nothing that
   varied it ever reached the feature placement or the substrate stage; and every world that
   HAS features, in either language, was built at `EARTH_RADIUS_M`. (radius varied) and
   (substrate path exercised) were each true somewhere and never true together.

   Closed by filling that cell in both languages: `test_conformance.py` gains
   `test_surface_bottom_at_agrees_at_a_world_radius_that_is_not_earths`, a 3,000,000 m world
   with loose features over the same 650 points and its own two measured bounds, and
   `surface.rs` gains a `small_world` fixture that `the_scalars_are_kept_exactly_as_given`,
   both feature tests and the forwarder test now use. All six mutations were re-applied
   afterwards: every one goes red, and a control mutation proved the mutant wheel was the one
   loaded. `Shelf::new`'s radius still survives and is still **inert** rather than
   unwitnessed -- `Shelf::radius_m` is stored and never read, and `shelf.rs:277` says so.

**The answer is to mutate each ARGUMENT, and to mutate it ON EACH PATH.** Instance 6 is the
final form of the rule, and it is the half the first five did not state: **an argument is
witnessed on a PATH, not in a codebase.** Closing it at one entry point reads as closing it
everywhere and does not. What has to be checked is the CROSS of (argument varied x path
exercised), one cell at a time; a census that lists arguments down one axis has only done
half the table. Sixteen mutations were written
into the source on purpose and run. Fifteen fail, each named in the test that catches it.
**One survives, and it survives by design rather than by a flat probe**: handing
`detail.amplitude_m` the point of somewhere else changes nothing, because `amplitude_m` never
reads its `point` -- the parameter is vestigial in the Python too (`detail.py:101`, and
`detail.rs` carries `#[allow(unused_variables)]` and says so). No probe anywhere on the
planet catches that one, so it is recorded rather than chased.

### Asserting a blindness makes a probe pairing load-bearing

A corpus that catches three mutations out of one world and nothing out of another has half
the coverage it appears to, and dropping the redundant-looking half then costs nothing
visible. So the blindnesses are asserted alongside the catches.

- **`bottom_at` needs both worlds, and neither probe alone suffices.** In the bare world at
  `base_sensitive_probe` the rock fraction comes from the TECTONIC term
  (`smooth(151/1200) = 0.0436`, strictly above the slope term), so zeroing the slope changes
  **zero bits** while zeroing tectonics moves the answer by 0.030. In the shaped world at the
  same point the slope is 0.0275 and the slope term dominates (0.768 against 0.043) --
  exactly reversed. The test asserts both catches **and** both blindnesses, and both hold at
  world granularity rather than at one lucky probe.
- **`the_tectonics_hold_this_worlds_continentality`** asserts the catch at
  `land_sensitive_probe` (-980.1204136079549 against the mutant's 767.8614639553075, over
  1,000 m apart and of opposite sign) **and** the bit-exact blindness at `shelf_water`. The
  probe was chosen so the sensitivity is a region rather than a knife edge: over the 25 points
  within +-1 degree at a half-degree step the two fields never come closer than 943.27 m.
- At the **world** level the same holds. The bare world is exactly blind to all three of
  `elevation_m`'s orderings, to the bit, at every one of its 650 points -- with nothing
  placed, `authority` is 0 and `shaped` is the shelf's own elevation, so all three collapse
  onto the identity. **A no-features corpus proves nothing whatever about the order of that
  method.** And the negative-seed world is exactly blind to the macro-base splice at every one
  of its 650 points, because there `land.base_elevation + tectonics.offset_m` IS the shelf's
  answer. Neither world is redundant; dropping either silently halves what the section sees.

### `surface_fields` and the three forwarders are API DECISIONS, not transcriptions

**`surface_fields` is a fourth entry point with no counterpart in `surface.py`.** It returns
`(world_seed, radius_m, plate_count, feature_count, features_radius_m)` and exists because
THREE of those are otherwise unwitnessed: `world_seed` makes the `i64` domain visible from
Python; `features_radius_m` is the only cheap observation of the adopted-`Features` branch,
which is otherwise visible only through metres of elevation; and `plate_count` -- the
strongest of the three -- is the one argument that survived a defaulting mutation against
every value comparison in the section (instance 5 of the trap, above). All three are the kind
of thing a constructor gets wrong by dropping, and a dropped one has no other witness.

That reasoning now lives in `bindings.rs`'s own doc comment on `surface_fields`, not only
here. A report is not a record: the label has to sit next to the code that will outlive it,
or the next reader sees an unexplained fourth method on a type whose reference class has
three.

Likewise `substrate_at`, `substrate_dominant_at` and `substrate_slope_at`. `surface.py`
exposes exactly ONE substrate-facing method; Python callers reach through the `substrate`
attribute -- `world.substrate.at(point, **known)` -- and `tests/test_conformance.py` does that
in a dozen places. This port has no object to reach through, so without the forwarders no
caller could supply known intermediates or choose a baseline. They add no behaviour: each is
one call to the free function of the same name with the callbacks `bottom_at` builds.

All four are labelled as decisions **in the source**, because an unlabelled fifth and sixth
method on a type whose reference class has three reads later as a transcription error.

### This closes the engine core

Every module `surface.py` composes is now ported, tested against the live Python, and bound:
`detmath`, `vectors`, `sphere`, `noise`, `tangent`, `continentality`, `plates`, `generation`,
`kinematics`, `tectonics`, `detail`, `shelf`, `features`, `substrate`, `surface`, and
`bindings` over all of them. Nothing in `worldbuilder/` has been deleted; the Python remains
the reference implementation, and every conformance figure in this file is measured against it.

What is *not* here is stated so the boundary is not mistaken for an omission: no climate or
land-cover layer (designed but unapproved -- see `docs/design/2026-09-03-roadmap-additions.md`),
and no viewer and no studio (slices 2 and 3).

### The throwaway is gone

Task 1's `tests/test_surface_gates.py` -- 5 pytest gates over four measured questions (the
seed cast, the `-0.0` case, the four reordering deltas, the two invariants), all answered
before anything in the port depended on the answers -- is deleted as of this section. Every
claim it carried that survives is asserted elsewhere: the seed cast in
`test_surface_keys_the_plates_on_the_signed_seed_not_the_masked_one` and in `surface.rs`'s own
two seed tests, the invariants in
`test_surface_structural_is_the_shelf_bit_for_bit_with_nothing_placed` and
`test_surface_elevation_is_structure_plus_detail_bit_for_bit`, and the reordering budget in
`test_surface_the_worlds_and_populations_catch_different_reorderings`.

**Every count here was verified by running the suites and checking exit status, not copied
from a report -- including the test counts.** `cargo test -p worldbuilder-engine` exits 0 at
**259** tests (249 lib, of which 27 are `surface::tests`; 4 `blake2_bytes.rs`; 6 `no_std_math`
guard; 0 doc-tests), and `--release` gives the same 259. `pytest tests/` exits 0 at **390** --
with the engine *required* via `WORLDBUILDER_REQUIRE_ENGINE=1` rather than skipped, since a
skipped conformance suite reports green while comparing nothing at all. Of those,
`test_conformance.py` is **150** (14 of them `test_surface_*`) and `test_performance.py` is
**8**. The 390th is
`test_surface_bottom_at_agrees_at_a_world_radius_that_is_not_earths`, added for instance 6 of
the trap above; the count was 389 for the length of the slice proper.

**A TEST COUNT MUST NAME ITS ENVIRONMENT, the same way a figure must name its population,
its method's parameters, its host and -- as this slice learned the hard way -- which mutation
it measured.** Every pytest count quoted during this slice was given without saying whether
`WORLDBUILDER_REQUIRE_ENGINE` was set, which makes those counts ambiguous in exactly the way
the reordering figure was: "390 passed" with the guard set and "390 passed" without it are
different claims, and only one of them is evidence that anything was compared. The skip
mechanism itself is correct and says so in its own comment; what was missing was the habit of
naming it. Quote counts as *"390 passed, `WORLDBUILDER_REQUIRE_ENGINE=1`"* or do not quote
them.

**And 14 `test_surface_*` is not 14 conformance tests.** One of them --
`test_surface_the_worlds_and_populations_catch_different_reorderings` -- makes **zero engine
calls**: it asserts what the Python reference's own corpus can and cannot see, so no defect
in the port can make it fail. That is deliberate and it earns its place, but the conformance
surface of this section is **13**, not 14. The test says so in its own docstring now.

`test_performance.py` was repaired earlier in this slice and is **no longer load-sensitive**:
it counts noise evaluations instead of timing them, because the two timing distributions
genuinely overlap. It passed here on a machine that was not idle. A failure there is now news
about the branch rather than a statement about the machine.
