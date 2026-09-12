# worldbuilder-engine

The generator core. One implementation, compiled twice: natively for Evennia and maritime
through Python bindings, and to WebAssembly for the browser studio.

Slice 0 measured that those two targets agree bit-for-bit over 5,000,000 samples, with a
negative control proving the comparison could detect a one-bit difference. That is the
foundation this crate is built on; see `spikes/0-bit-equality/README.md`.

## What is here so far

Counted from `src/`, not from memory (re-counted for slice 5a Task 6, which found it stale by
two modules and one binary; re-counted again for slice 5b Task 6 / relief Task 5, which found
it stale by one module and two binaries -- `water.rs`, `pond_threshold_survey.rs` and
`relief_survey.rs` had landed without this count moving, exactly as `erosion.rs` and `wasm.rs`
had before them; **re-counted a third time for the photoreal slice's record, which found it
stale by three binaries** -- `mountain_probe.rs`, `mountain_survey.rs` and
`coastline_survey.rs` had landed without it moving, so **this count has now been wrong three
times running and always in the same direction**): **twenty-one modules plus the crate root,
and SEVEN binaries** (`crates/worldbuilder-engine/src/*.rs` is 22 files, one of which is the
crate root; `src/bin/*.rs` is 7). Sixteen of the modules are ported from a named Python module and held
to it by `tests/test_conformance.py`; the last five -- `stream.rs`, `streamfmt.rs`,
`erosion.rs`, `water.rs` and `wasm.rs` -- are **new in this crate and have no Python to be
conformant with**, so every claim they make is a property test, a measurement, or (for
`wasm.rs`) the parity harness instead.

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
    src/stream.rs      the node sampler and StreamGraph: the second representation
    src/streamfmt.rs   the stream graph's on-disk format: fail closed, sliceable by region
    src/erosion.rs     the Cordonnier stream-power bake over a StreamGraph: slice 5a
    src/water.rs       lakes, overflow, the tied-plateau merge and the water manifest: slice 5b
    src/bindings.rs  the PyO3 surface, conversion only
    src/wasm.rs        the hand-written C ABI the browser studio calls, --features wasm
    src/bin/streambench.rs  what a graph costs, at sizes up to a whole planet
    src/bin/erosion_convergence_sweep.rs  whether §14.3's iteration count holds, and against which parameter
    src/bin/pond_threshold_survey.rs  the body-surface-area distribution the pond threshold is calibrated against
    src/bin/relief_survey.rs  relief across ReliefParams' parameter space, over two site populations
    src/bin/mountain_survey.rs  the STRUCTURE field across TectonicParams' space; chooses nothing
    src/bin/mountain_probe.rs   a throwaway probe: does moving TectonicParams make a mountain?
    src/bin/coastline_survey.rs  land fraction, coastline length against ruler, islands and inlets

The first seventeen entries are the engine core, and it is closed -- see **This closes the
engine core** below. `stream.rs`, `streamfmt.rs`, `erosion.rs` and `water.rs` are not part of
it: they are the *second* representation CORE-001 adds beside it, the bake that runs over it,
and the water that falls out of the bake, and none of the four add anything to `Surface`.
`wasm.rs` is not part of it either -- it is the export surface both representations are
reached through, not a third representation.
Keep it in step with `src/` when a module lands: it went seven modules stale once already,
two more (`erosion.rs`, `wasm.rs`) had landed by slice 5a Task 6 before this count caught up
with them again, and a third (`water.rs`) plus two binaries by slice 5b Task 6 -- each time on
the same page that claims the core is complete.

The Python in `worldbuilder/` is still the reference implementation and is unchanged.
Nothing has been deleted, and the engine is additive until conformance is established for
every module.

## Three rules that are not style

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

**Sweep an export's parameter space; do not spot-check it.** This is the newest of the three
and the only one no build guard can enforce, so it is written here rather than left in a task
report. `extern "C"` is **nounwind**: a panic behind an export is not an exception a caller
can catch, it is `abort()` -- exit `0xc0000409` natively, and a dead module plus a blank
viewer in a browser. This project has now found **three reachable aborts by sweeping an
export's inputs and zero by spot-checking them**:

- slice 5a, two: a negative `erodibility_per_yr` turning `implicit_receiver_update`'s
  `1 / (1 + c)` from a contraction into an amplifying map (see **An abort was reachable
  through `wb_erosion_run`** below), and a `radius_m` large enough to overflow
  `stream::node_areas_m2`'s area calculation to `+inf`, found in the whole-branch review's
  fix round.
- relief Task 4, one: `Detail::plan` walks `while wavelength >= relief.canonical_wavelength_m
  { wavelength *= 0.5 }`. At a canonical wavelength of exactly `0.0` **halving never reaches
  zero and the loop never terminates** -- the observed symptom was
  `memory allocation of 103079215104 bytes failed`. Closed by
  `WB_MIN_RELIEF_WAVELENGTH_M = 1.0e-3` at the door (and `WB_MAX_RELIEF_WAVELENGTH_M = 1.0e9`,
  because `inf * 0.5` is `inf` and hangs it from the other end).
- slice 5b Task 5, one: at a `sea_level_m` below the world's own lowest sampled point there is
  no boundary node anywhere, so no mouth, so every basin is a lake and their union has no rim
  -- `merge_tied_plateaus` panics through the boundary. Closed by an exact-precondition check
  (`every_component_has_an_outlet`), and re-confirmed by neutralising the new guard rather
  than by trusting that it exists.

**Every one of the three was a BAND, not a cliff** -- fine on both sides of a bad interior
value. `canonical_wavelength_m` is fine at 250.0 and fine at 1e-3 and fatal at 0.0. The water
datum is fine at 0.0 and fine at -5,698.0 and fatal at -5,699.0, the transition bisected to
-5,698.763334509833 m on that world. A spot check at sensible values passes all three. That is
why `tests/wasm_exports.rs` brackets each of these from **both** sides on a ladder rather than
asserting one value, and why an export's bound is not considered proven until a sweep has been
run across it. "The bounds look complete" is what was believed before each of the three.

    cd crates/worldbuilder-engine
    python -m maturin develop --release

That is the Python wheel. The browser build is a different target, a different feature and
a different script -- see the next section.

## The WebAssembly surface, and the browser that consumes it

`src/wasm.rs`, gated behind `--features wasm`, is the only door the browser has into this
crate: a hand-written C ABI over `wasm32-unknown-unknown`, no `wasm-bindgen`, no glue, no
bundler. The consumer is `viewer/`, whose own README carries everything about the page --
the offline guarantee, the terrain provider, the worker pool and the frame budget. What
belongs *here* is the shape of the door and the evidence that both sides of it agree.

    cd viewer
    npm run build:wasm      # builds, verifies the shape, fingerprints, copies into public/wasm/
    npm run check:wasm      # is the SHIPPED artifact built from the source that is here now?

**The artifact, read from `public/wasm/MANIFEST.txt` and from the file (re-derived for
slice mountains Task 6. This figure has now been found stale THREE times by the task that
re-derived it -- slice 5a Task 6 found it a whole export behind after `wb_erosion_run` landed,
it was four behind again by 5b Task 5 with the relief slice's three exports, and it was three
behind and 4,825 bytes light again by mountains Task 6, this slice's own tectonic exports
having landed the same way. A number nobody's gate reads is a number that goes stale; treat
this paragraph as one to re-derive rather than to trust. **It went stale a FOURTH time**, by the
photoreal slice's three coast exports, and was re-derived here from the file and the manifest
together):** 226,673 bytes, **22 exports**
(`memory` plus the twenty-one functions below), **0 imports**. Zero imports is the design, not an accident:
`WebAssembly.instantiate(bytes, {})` is the entire loader, there is no JS runtime to keep in
step, and a worker gets its own instance and therefore its own linear memory for free.

    wb_generator_version   wb_alloc         wb_dealloc       wb_world_new   wb_world_free
    wb_world_count         wb_elevation_m   wb_structural_m  wb_bottom_at   wb_fill_tile_f32
    wb_erosion_run         wb_world_new_relief   wb_relief_preset   wb_relief_check
    wb_water_run           wb_world_new_tectonic wb_tectonic_preset wb_tectonic_check
    wb_world_new_coast     wb_coast_preset       wb_coast_check

`WB_EXPORTS` in `wasm.rs` is that list, declared. A test holds this crate's source to it and
the build script holds the built module's export section (id 7) to it, because a forgotten
`#[no_mangle]` does not produce a compile error -- it produces a **327-byte artifact
exporting only `memory`**, from a `cargo build` that exits 0. That failure mode is
reproduced on demand by `npm run build:wasm:self-test`, which builds without the feature,
confirms the assertion rejects the result, and rebuilds the real artifact.

### Why the surface is a handle and not a function

`bindings.rs` is the shape a conformance shim wants: pass every world parameter with every
sample, and `Surface` is rebuilt on each call. That is right for a test corpus and wrong for
a viewer, because the rebuild costs about **10^3x** a sample (`wasm.rs`'s own module docs
carry the three-host table). So the browser surface is a handle: `wb_world_new` once,
`wb_elevation_m` / `wb_fill_tile_f32` as often as you like, `wb_world_free` at the end.

That is not a concession to cost. `Surface::new` is **milliseconds** -- 3.5-5.9 ms per
worker measured in the browser, 5.1-6.4 ms median across the four pool sizes in this task's
own run -- so a *parameter* change still rebuilds a world inside one animation frame, which
is what will make slice 3's controls feel live.

Three properties of the handle table are worth knowing before you build on it:

* **Slots are never reused.** A freed handle stays freed and no later world takes its
  number, so a worker that outlived a parameter change gets `WB_ERR_HANDLE` rather than a
  silently different planet.
* **The table is thread-local.** On wasm32 that is simply a static; natively it means
  `wb_world_count()` is per-thread, which is a convenience in tests and not a bug.
* **Panics are fatal, and not only on wasm.** `extern "C"` is nounwind, so a panic reaching
  one of these boundaries aborts the process rather than unwinding -- measured, by deleting
  a bound and watching a test binary die with `STATUS_STACK_BUFFER_OVERRUN` instead of
  reporting a failure. Every representable bad input is answered with a status code
  (`WB_ERR_HANDLE`, `WB_ERR_BUFFER`, `WB_ERR_GRID`, `WB_ERR_SUBSTRATE`) for that reason.

`wb_fill_tile_f32` is **ergonomics, not throughput**, and must not be defended as an
optimisation: a boundary crossing was measured at about 2% of the elevation it carries, and
per-call sampling against a single fill over an identical grid measured indistinguishable.
It exists because it hands a worker one `Float32Array` it can transfer, which is exactly the
buffer `Cesium.HeightmapTerrainData` wants, in metres above the ellipsoid, with no
conversion at all.

### Parity: the shipped bytes, not a rebuild of them

The whole studio architecture rests on native and wasm32 agreeing bit-for-bit, so that claim
is a harness (`parity/`, with its own README) rather than a figure in a task report. Both
sides call **the same shipped exports**, never an internal function, and the corpus crosses
as 16-hex-digit bit patterns so a mismatch cannot be a `printf`.

Re-run in this task, on the committed artifact:

    cargo run --release -p worldbuilder-engine --example parity_dump --features wasm > native.txt
    node crates/worldbuilder-engine/parity/parity.mjs native.txt
    node crates/worldbuilder-engine/parity/parity.mjs native.txt --mutate seed
    node crates/worldbuilder-engine/parity/parity.mjs native.txt --mutate erosion-k
    node crates/worldbuilder-engine/parity/parity.mjs native.txt --mutate water-pond
    node crates/worldbuilder-engine/parity/parity.mjs native.txt --mutate tectonic-warp

**89,861 values compared, 0 divergent** (slice mountains Task 6's re-run: the corpus now
carries the tectonic channel as well -- both presets field by field, six checker answers, a
world built from `TectonicParams::ranges()`, 2,000 points on the belt it builds, and a tile
across it. Three tasks flagged that gap in a row and none owned it). **The controls are the
half that matters** -- a harness that has never reported a disagreement is not known to be
able to -- and there are four, deliberately at four different scales:

| control | what it perturbs | divergent |
|---|---|---:|
| `--mutate seed` | the world seed, by one | 86,190 of 89,861 |
| `--mutate erosion-k` | `erodibility_per_yr`, by one ULP | **216**, all in `erosion/erosion` |
| `--mutate water-pond` | `pond_max_surface_area_m2`, one threshold | **60**, all of them `Body::kind` |
| `--mutate tectonic-warp` | `margin_warp_m`, one word of the block | **6,186**, all on the tectonic world |

A control that moves everything is nearly as uninformative as one that moves nothing, which is
why the last three exist. The last two have their counts **predicted natively before the run**
-- the water control from an independently summed surface-area distribution, the tectonic one
per group and from the library beneath the exports -- and `parity.mjs` fails if any group moves
by a different amount. The tectonic control's most useful number is the one that does *not*
move: 83,675 values, including both tectonic presets and the tectonic checker, must compare
equal. See `parity/README.md`.

**Parity alone cannot tell you the artifact is current, and for several commits of this
project it did not.** The committed `.wasm` predated a change to `wasm.rs`; the two differed
by five bytes, all of them panic-location line numbers, which never execute. It passed
parity perfectly, because `native.txt` had been recorded from the same stale build -- a
corpus and an artifact that are stale *together* agree with each other and with nothing else.
`parity.mjs` now **imports** `checkFreshness()` from `viewer/scripts/build-wasm.mjs` and
refuses to report at all when it returns problems. It imports rather than reimplements
because two copies of a provenance rule drift, and the copy that drifts is the one that
stops refusing.

The fingerprint covers **36 inputs** (re-derived at slice mountains Task 6 by running
`node viewer/scripts/build-wasm.mjs digest`, which prints `fingerprint-inputs: 36`; this
paragraph said 29 and had been left behind by two slices' worth of new files under the walked
directories -- the same shape of staleness the artifact-size paragraph above records): every
file under `src/`, `examples/` and `tests/`
recursively, this crate's `Cargo.toml`, the workspace `Cargo.toml`, `Cargo.lock`, the
`rustc -vV` release, commit hash and host, and the literal cargo argument list. It is
deliberately over-inclusive -- `bindings.rs` cannot affect a `--features wasm` build and will
still trip it -- because a false *rebuild it* is cheap and a false *it is current* is the one
that costs.

**`examples/` and `tests/` were added on 2026-09-04, taking the count from 24 to 28**, after
CI demonstrated the hole: run
[33916847441](https://github.com/LackOfSkillz/worldbuilder-by-aetos/actions/runs/33916847441)
cut the parity corpus by a sixth by editing `examples/parity_dump.rs`, and `check:wasm` still
reported the artifact current while parity reported zero divergent. Neither directory changes
a byte of the `.wasm`, which is precisely why they were missing and precisely why they now
count: the fingerprint is a claim about the tree the artifact was blessed in, and the corpus
and the integration tests are part of what makes the blessing mean anything.

**28 became 29 in the identity slice**, when `tests/build_fingerprint.rs` landed as a new
file under one of the three walked directories -- adding a fingerprinted input is exactly as
much a digest-moving event as editing one of the existing 28, which is why this number is
re-derived here rather than left at 28. **29 became 36 over the slices since**, every one of
them a new file under `src/bin/`, `examples/` or `tests/` rather than any change to how the
walk works, and this paragraph did not notice until Task 6 ran the command. Re-run: `node
viewer/scripts/build-wasm.mjs digest` (from `viewer/`) prints `fingerprint-inputs: 36`
alongside the digest, and `worldbuilder_engine.source_fingerprint_inputs()` -- the PyO3 export
the identity slice's Task 2 added -- returns the same `"36"` from the just-built extension. The two are meant to
agree: see the top-level README's CI section for the gate that checks it and the ruling that
allows this crate to compute the same digest twice, once in the Node build script and once
in `build.rs`.

**A consequence to plan for: touching a doc comment in `src/` invalidates the shipped
artifact.** The fingerprint is over file content, not over anything semantic, so a one-line
comment fix in `wasm.rs` makes `npm run check:wasm` and the parity harness both refuse until
`npm run build:wasm` is re-run and the new bytes are committed. That is the guard working as
designed, and it means a documentation-only change to this crate is not always a
documentation-only commit.

**What none of this claims.** Parity says the shipped bytes reproduce native source on this
corpus. It says nothing about CPython: this crate routes every transcendental through
pure-Rust `libm` precisely so that native and wasm agree, which is mutually exclusive with
matching the platform libm CPython calls. See **Conformance** above for the 4-ULP contract
that governs that boundary instead.

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

## `stream.rs` and `streamfmt.rs`: the second representation, and the first thing in this project measured at planet scale

CORE-001's whole claim is that a core holding **two** representations from the start lets
slice 5 *populate* rather than *restructure*. `Surface` answers "how high is it here" from a
seed alone, statelessly, for ever. A drainage network cannot be answered that way -- whether
water leaving a place reaches the sea depends on every other place -- so it has to be built
over a node set and then held. `World` is the type that holds both, and **`Surface` is not
modified**: the same eight fields, checked by `the_surface_is_not_modified_by_this_slice`,
which reads the struct's own source and fails if a ninth appears or a graph word gets in.

Under VERSION-001 that is not a stylistic preference. Retrofitting a graph into an engine
built only for scalar fields changes what a fixed seed evaluates to, which is a
`GENERATOR_VERSION` bump, which is every existing worldfile through a migration it did not
ask for. The field list is therefore fixed **now**, while nothing has declared a version.

**Slice 5 owns the erosion.** No stream power equation, no implicit solver, no thermal
correction, no lake overflow, no mutation of `height_m` after construction, no spherical
Voronoi. `Lake::outflow_lake` is reserved at its sentinel and `reaches` is reserved empty --
the *records* exist so that filling them later is not a schema break, and two tests assert
that this slice populates neither.

### The four asserted properties, and the population each was measured on

`StreamGraph::validate` returns *every* defect it finds rather than the first, because a
partition failure and a cycle have different causes and a reader wants both. `build` calls it
and refuses rather than returning a graph that fails it.

| # | Property | How it is enforced | Measured on |
|---|---|---|---|
| 1 | The downhill relation is a **forest** -- no cycles | `peel` removes leaves until nothing is ready; `peeled != node_count` is `Cycle` | complete peels at 3,200 (lattice), and at 10,000 / 100,000 / 200,000 / 1,000,000 / 5,000,000 / 20,000,000 / **50,000,000** over a real `Surface` |
| 2 | Every root is **exactly one** of MOUTH or LAKE | `validate` reports both the "neither" and the "both" arm; `streamfmt` re-checks it across sections | 633 roots at 3,200; 597,687 at 20,000,000; 1,203,699 at 50,000,000 |
| 3 | A rebuild is **bit-identical** | `bit_identical_to` compares `to_bits()`, never `==` | a negative control per **column**, plus a signed-zero and a NaN pair per **float** column (see below) |
| 4 | The sentinel is **never a valid index** | `MAX_NODES = u32::MAX - 1`, and `sentinel_is_a_valid_index` is exercised *with a sentinel that is in range* | a guard only ever called with the good value proves nothing about itself |

**Property 3's negative control is per column, because one bit-flip is not a control for
eleven comparisons.** The first version of this slice proved bit identity with a single test
that flipped one bit of `drainage_area_m2[3]`. The final review mutated `bit_identical_to`
column by column and found **six surviving mutants**: deleting the `area_m2`, `height_m`,
`flags` or `header.world_seed` comparison, or downgrading `height_m` or `drainage_area_m2`
from `to_bits()` to `==`, left the whole suite green. Only `drainage_area_m2`'s bits were
actually pinned -- and `area_m2` is the field §3.2 argues hardest to carry, on the grounds
that adding it later silently changes what every stored `drainage_area_m2` means. A property
test that cannot fail is worse than no test, because it is counted.

Three tests now carry it:

- `bit_identity_notices_a_change_in_every_column` walks **all 22 perturbations** -- every
  header field, `downhill`, `flags`, all three float columns and their lengths, every `Lake`
  field, every `Reach` field -- and asserts the comparison is symmetric in each.
- `bit_identity_notices_a_changed_reach_endpoint` plants a `Reach` by hand, because slice 1p
  ships `reaches` reserved empty, so no built graph can witness those columns otherwise.
- `bit_identity_compares_bits_and_not_values_in_every_float_column` applies the two
  perturbations that separate `to_bits()` from `==` to **each of the seven float columns**:
  `0.0` against `-0.0` (equal by `==`, different bits, so the graphs must differ) and two
  identical NaNs (unequal by `==`, identical bits, so the graphs must match).

All six mutants were re-run after the fix and all six die. `bit_identical_to` also now
length-checks all three float columns rather than only `height_m`, which it indexed all three
by; a legal graph can never be ragged, but the function is public and the check was one line.

**VERSION-001 recognises no ordering, for both version fields.** The generator version was
swept at 0, 2, 7 and `u32::MAX`; the format version was tested only at 2, so rewriting its
guard as `format_version != 2` or `format_version <= FORMAT_VERSION` survived the whole
suite -- a regressed reader would have accepted a file declaring 0, 7 or `u32::MAX`.
`refuses_every_format_version_that_is_not_this_one` now mirrors the generator-version sweep,
and both mutants die.

**The root count is invariant across SEA LEVELS at a fixed node count. It is not invariant
across node counts, and it is not a small number.** This is the figure that has already been
got wrong twice in this slice, in both directions, so it is stated here with its population
every time it appears:

| population | nodes | roots | as a fraction | mouths / lakes |
|---|---|---|---|---|
| `stream.rs`'s 40x80 lattice fixture, datum -1400 m | 3,200 | **633** | 19.8% | 64 / 569 |
| the same fixture, datum 0 m | 3,200 | **633** | 19.8% | 554 / 79 |
| the same fixture, datum +2900 m | 3,200 | **633** | 19.8% | 633 / 0 |
| the extraction's probe field (§8.3) | 10,000 | 300 | 3% | -- |
| the extraction's probe field (§8.3) | 20,000,000 | 5,647 | 0.03% | -- |
| this crate's own `Surface`, seed 20260904, datum 0 m | 10,000 | 489 | 4.890% | 425 / 64 |
| " | 100,000 | 4,992 | 4.992% | 3,906 / 1,086 |
| " | 200,000 | 10,247 | 5.123% | 7,671 / 2,576 |
| " | 1,000,000 | 56,901 | 5.690% | 37,312 / 19,589 |
| " | 5,000,000 | 216,850 | 4.337% | 135,038 / 81,812 |
| " | 20,000,000 | **597,687** | 2.988% | 371,866 / **225,821** |
| " | 50,000,000 | 1,203,699 | 2.407% | 751,974 / 451,725 |

The invariance across datums has a one-line reason: a root is a node with no strictly-lower
neighbour, and the datum appears nowhere in that test. The datum decides only how a root is
*labelled*, so it moves the mouth/lake split -- 64 to 554 to 633 across the three rows above
-- and never the total. `the_root_count_is_invariant_across_datums_at_a_fixed_node_count`
pins all four numbers.

The variation *across* node counts has a different reason, and §8.3 states it
correctly: at coarse spacing nearly every node is a local extremum, and as spacing tightens
flow organises into chains, so the *fraction* falls. What §8.3's figures do not survive
is the absolute count. **`lake_at`'s docstring said roots "grow sub-linearly -- a 19x rise for
a 2,000x rise in nodes", which is a property of the extraction's probe field and not of this
code.** Over a real `Surface` the same 2,000x rise in nodes gives a **1,222x** rise in roots,
which is very nearly linear, because a real elevation field has relief at every scale the
spacing can resolve and the probe field did not. That docstring is corrected, and the linear
scan it justified is flagged for slice 5: 225,821 comparisons per `lake_at`, not 5,647.

### The sampler: why "Poisson-sampled" was read as naming the property, not the algorithm

§14.1 asks for "Poisson-sampled points". **What ships is a Fibonacci spiral with
hashed jitter, and the substitution is deliberate.** Three reasons, none of them a preference:

1. **Bridson's algorithm is definitionally sequential** -- sample *n* depends on samples
   `1..n-1`. That is precisely the shape `generation.rs` forbids in terms: "Every value here
   is hashed, never drawn from a sequence." A sequential sampler inside CORE-001 would be
   self-defeating: the slice whose entire purpose is that adding a field later costs nothing
   would introduce the one sampler where adding a *draw* later moves every node.
2. **The spiral is measurably the more regular point set.** Un-jittered, its minimum
   neighbour separation is **0.872 x nominal spacing** -- measured at **0.8723 to four
   figures at every one of 3,200 / 20,000 / 200,000 / 1,000,000 / 20,000,000 nodes**, which
   is why it can be used as a constant in a bound rather than as an observation. Bridson
   guarantees only its own radius *r*, and *r* for a given point count sits well below
   nominal spacing because disc packing is loose.
3. **§14.1's own stated reason is a property, not an algorithm.** It wants points and
   areas that "transfer to a sphere directly -- no projection, no grid seam, no pole
   singularity". Those are properties of the spiral. The property is what matters, and the
   spiral has it.

Nothing in Bridson needs a banned API. **The blocker is architecture, not the constraint
list**, and that distinction is recorded in the source so it is not rediscovered as a
constraint problem and "solved" by relaxing a constraint.

The hash is an integer avalanche, not a BLAKE2b digest, and that is also a measurement:
`generation::fraction` formats a `String` and digests it at **332.1 ns per call**, so two
draws per node at 20,000,000 nodes is 40,000,000 digests and roughly **13 seconds of a
19-second sampling stage**. The avalanche is the one `noise.rs` already uses, is equally
index-addressed and equally free of any sequence, and costs about a nanosecond. It is a
*different* hash from `generation::spread`, on purpose: nodes and plates jitter by different
numbers on the same seed, because they are different point sets and nothing should be able
to confuse them.

### The jitter is 0.15, and the bound is a proof rather than a sample

The jitter adds `a*east + b*north` with `a` and `b` each uniform on `[-J, +J]`, where
`J = NODE_JITTER_FRACTION * nominal_spacing_rad(count)`. The largest tangent displacement is
at the corners of that square, `J*sqrt(2)`, and the arc displacement `atan(J*sqrt(2))` is
smaller still. **Two nodes can approach each other by at most twice that**, so the box bounds
the separation from below at a loss of `2*sqrt(2)*J`:

    min_separation >= (0.8723 - 2*sqrt(2)*J) x nominal
    at J = 0.15:     0.8723 - 0.42426 = 0.448

That inequality is the reason 0.15 is defensible and 0.20 is not, and it does not depend on
any size having been tried:

| jitter | 3,200 | 20,000 | 200,000 | 1,000,000 | 20,000,000 | **proved floor** |
|---|---|---|---|---|---|---|
| 0.00 | 0.8723 | 0.8723 | 0.8723 | 0.8723 | 0.8723 | 0.872 |
| 0.10 | 0.7215 | 0.7053 | 0.6907 | 0.6694 | -- | 0.589 |
| **0.15** | 0.5988 | 0.5783 | 0.5618 | 0.5312 | 0.5322 | **0.448** |
| 0.20 | 0.4768 | 0.4517 | 0.4329 | **0.3930** | -- | 0.307 |
| 0.30 | 0.2394 | 0.2013 | 0.1754 | 0.1165 | -- | 0.024 |
| 0.45 | 0.0805 | 0.0332 | 0.0054 | 0.0026 | -- | -0.401 |

`MIN_SEPARATION_FRACTION` is 0.40. At jitter 0.20 the *measured* ratio is already 0.3930 at a
million nodes -- below the assertion -- and its proved floor of 0.307 is below it too, so it
fails by sample and by proof. At 0.15 the measurement clears the assertion by a third at
every size and the *proof* clears it as well. **The measurements are the check on the bound;
the bound is what licenses sizes nobody measured.** Quoting the 0.15 row without the proved
floor would leave the constant resting on five samples, which is exactly the argument the
next paragraph demolishes.

**The failure does not announce itself at small sizes.** At 3,200 nodes even jitter 0.45
keeps 0.08 x nominal and a graph builds; by a million the ratio has fallen a further factor
of thirty, and at 0.45 two nodes land **59.6 m apart on a 22,584.6 m lattice**. `build`
returns `CoincidentNodes` rather than resolving such a pair, so this constant is the
difference between a graph and a refusal. A jitter validated at a few thousand nodes and
shipped for twenty million is precisely the shape of this bug, and the reason every figure
above names its node count.

Smaller jitter is not free: at 0.0 the point set is a visible spiral, its cells are
near-identical (CV 0.005), and the drainage network inherits its arms. Larger jitter widens
the cell-area spread -- 0.737x to 1.260x the ideal cell at 0.15 against 0.524x to 1.546x at
0.30 -- and stream power goes as area^0.5, so that spread is an erosion-rate error at exactly
the headwater nodes that are hardest to stabilise.

### `area_m2`: the method, and its error against the constant it replaces

**No spherical Voronoi.** An exact spherical Voronoi diagram is a degeneracy-prone
exact-predicate problem -- cocircular sites, collinear sites, antipodes -- and under
DETERMINISM-001 a predicate that is *nearly* right is a different planet, not a slightly
wrong one. It deserves its own slice if it is ever wanted, and slice 5 can add it **without
changing a single type**, because the field is already here.

What ships is a normalised local-density estimate: with `d_i` the mean great-circle angle
from node `i` to its nearest `AREA_NEIGHBOUR_COUNT` neighbours,

    area_i = 4*pi*R^2 * d_i^2 / sum_j d_j^2

so the total is the sphere's area by construction -- a drainage area accumulated over a whole
continent cannot drift away from the geography, and a per-cell estimate that was locally
better but summed to 1.03 spheres would be worse where it matters most.

**Its error, stated.** Against Monte-Carlo cell areas -- n = 20,000 at seed 20260904, jitter
0.15, 8,000,000 probe points drawn from a much denser spiral at seed 777000001 and jitter
0.30 so the two sets are not aligned:

| | RMS relative error | worst single node | correlation with truth |
|---|---|---|---|
| the shipped estimator | **2.46%** | **13.7%** | **0.9479** |
| the shared constant it replaces | **7.5%** | -- | **undefined** (see below) |

The constant's 7.5% is not a separate measurement: it is *by definition* the true spread's
own CV, because a constant is the mean and its error is the deviation. Its correlation with
the truth is **undefined**, not zero: Pearson's r divides by the predictor's standard
deviation, and a constant's is exactly 0. "Zero correlation" is the right *intuition* -- it
tracks nothing -- but the statistic does not exist, and this file does not round an
undefined quantity to a number. So this is roughly a **threefold reduction in area
error, for one extra pass over a neighbour list that had to be built anyway** -- and, more
importantly, it *tracks* the variation rather than flattening it: it recovers 0.756x to
1.270x against a true 0.737x to 1.260x, both at **n = 5,000 nodes**; the CVs of 0.0731 and
0.0753 are from the estimator-error measurement at **n = 20,000**, and are quoted here only
as the same quantity at a different population, never as this one's. (The extraction's §8.4
quotes the true spread as 0.735x to 1.285x; that is its probe field, not this one, and the two
are quoted separately rather than blended.)

**k = 5 was swept, not assumed:** RMS 7.79% at k=2, 2.83% at k=4, **2.46% at k=5**, 3.37% at
k=6, 4.90% at k=8, with the same ordering at n = 5,000. The far neighbours of a stretched cell
sit *across* it rather than around it, so including them pulls every estimate back towards
the mean and throws away the variation the field exists to record.

It is an approximation and is documented as one. What it must not be is an *unstated*
approximation, because `drainage_area_m2` is a sum of these and a reader of the file has no
way to tell which method produced it.

### The three fields that look optional and are not

- **`area_m2`, per node, never a shared constant.** If it were added later, every already
  stored `drainage_area_m2` would silently change meaning from "cell count x a constant" to
  "sum of areas" -- **same bytes, different semantics, undetectable from the file**. And the
  constant is not even approximately right: the true cell area runs **0.737x to 1.260x**
  the ideal at the recommended jitter, measured over **this crate's own sampler** by
  `measure_cell_area_spread` (n = 5,000 nodes at seed 20260904, jitter 0.15, 4,000,000
  Monte-Carlo probes, CV 0.0754). The extraction's §8.4 figure of 0.735x to 1.285x is a
  different population -- its probe field -- as §"Its error, stated" above already says.
- **`flags`, a bitset.** Adding a boundary tag later as a *value* is fine; adding the
  *field* later means every existing graph has no boundary tag, so mouths cannot be
  identified, so a water manifest cannot be produced from an old graph at all. Four of the
  eight bits are still spare, and `validate` refuses a graph that sets one.
- **`sea_level_m`, in the header.** Mouth-versus-lake is a function of it, so **a graph built
  at one datum and read at another is wrong without being malformed** -- the failure mode
  with no symptom. `sea_level_is_load_bearing_for_the_classification` proves the datum
  changes the answer, so the field is not decoration.

**`pond_max_drainage_area_m2` is deliberately required-with-no-default, and deliberately
unmeasured.** It cannot be measured *here*, and the reason is structural rather than a
shortage of time: §13.2's distinction is between a pond and a lake -- a *water body* --
and the size of a water body is the area its surface covers at the level it fills to.
**Slice 1p has no fill algorithm.** `Lake::level_m` is the root's own elevation, an empty
basin, and `LAKE_MEMBER` is set on the root alone. The only quantity this slice can offer is
the drainage area *arriving at* a root, which is the catchment feeding the basin and not the
basin: a small tarn at the head of a large steep catchment and a broad shallow lake at the
head of a small one sit on opposite sides of §13.2 with the same root drainage area. A
number derived from the stand-in noise field would be **worse than no number** -- it would be
a measurement of the stand-in, it would look measured, and that is exactly how an unmeasured
number becomes a permanent one. The refusal is pinned:
`build_params_has_no_default_so_the_pond_threshold_must_be_stated` fails if `Default` is ever
implemented for `BuildParams`, and it carries its own positive control so it cannot pass
vacuously. **Nobody may supply a plausible default. Slice 5, which owns the fill, calibrates
it.**

### The format: fail closed, and sliceable by region

Two things force the shape, and everything else follows from them.

**1. Fail closed on an unsupported generator version.** VERSION-001 is strictly binary:
supported means evaluate, unsupported means refuse. No negotiation, no compatibility matrix,
no partial read. That is why the version is a bare `u32` and why every refusal is a hard
`Err`. A *lower* version is refused exactly as hard as a higher one, because the invariant
recognises no ordering.

**2. Read a region without parsing the whole file.** This is forced, not chosen -- see the
measurements below, which put a 20,000,000-node build at 3.26 GiB of peak resident memory.
Nothing of that shape fits a 32-bit wasm32 heap under any field arrangement, so the browser
must load a *region* and never the planet. Retrofitting that later is exactly the change
VERSION-001 makes expensive.

The layout: an 8-byte magic `WBSTRMG\0`, a 64-byte header, a seven-entry section table of
32-byte rows, and a **288-byte prefix** that is all a client needs to compute every byte range
in the file. `GraphReader::open` takes that prefix and a file length and never requires the
payload. Five sections are per-node (`height_m`, `area_m2`, `downhill`, `drainage_area_m2`,
`flags`); two are tables (`lakes` at 24 bytes a record, `reaches` at 16, written empty).
Element `i` lives at `offset + i * elem_width` and nowhere else.

**What region-sliceability costs, all of it:**

- **29 bytes per node** across the five columns (8 + 8 + 4 + 8 + 1), stored raw. **No
  compression and no delta coding are possible at all** -- either makes element `i`'s
  position depend on elements `0..i`, which is exactly what a region read must not need.
  That is the whole price and it is paid on every byte of the file.
- **Five range requests per region, not one.** Struct-of-arrays means a reader that wants
  only heights pays for heights alone; a reader that wants all five columns pays five seeks.
- **Up to 49 bytes of alignment padding per file** -- seven sections, at most seven bytes
  each, a fixed cost at any node count. Bought to keep a future zero-copy `&[f64]`/`&[u32]`
  view possible without a realignment copy.
- **A fixed 288-byte prefix and a section table that cannot grow within this format
  version.** Appending a section is a *format*-version bump and **not** a generator-version
  bump; conflating the two would force every existing worldfile through a generator migration
  every time a column was added.

**Serialise the flags, never the rule that derived them.** `build` classifies a root by a
sea-level test, and that test is a *default classifier*, not part of the model: it is
physically wrong in two named cases -- a submarine local minimum becomes a "mouth", and a
land depression below the datum (the Dead Sea, Death Valley, the Qattara Depression) is a
lake the test calls a mouth. Slice 5 must be able to replace the classifier **without a
format or generator version bump**, so the format stores the flags and enforces only
§14.2's actual claim: every root is exactly one of MOUTH or LAKE.
`the_format_never_applies_the_datum_classifier` scans the module's own source to keep it that
way, and has its own positive control.

**Forty-one guards, forty-one mutants, forty-one catches.** Every fail-closed check is
a single `ensure(...)` line carrying a `// MUT-nn` marker so that a mutation campaign can
disable exactly one at a time. Task 5 ran that campaign at 39 guards and reported **39 of 39
caught by a named test**, including three that survived its *first* pass and were reported as
corrections to its own work. This task adds two guards, MUT-40 and MUT-41, and mutated each:
both are caught, and MUT-41 by two independent tests. **41 of 41.**

### The two loose ends Task 5 named honestly, now closed

**The FNV is public, and there is still exactly one of it.** The header carries a
`position_checksum` -- FNV-1a over the node positions' IEEE-754 bit patterns -- and stores no
positions, which is 8 bytes instead of 24 per node. Task 5 dropped its `positions_match`
because `stream.rs`'s hash was private, and **explicitly refused to write a second copy**: two
copies would agree until one was touched, and the disagreement would surface as every existing
worldfile failing its own checksum. That was the right call and the wrong end state -- it left
eight header bytes nothing could read. `stream::position_checksum` is now `pub`,
`GraphReader::positions_match` is restored, and
`the_checksum_is_the_one_in_stream_rs_and_not_a_copy_of_it` scans `streamfmt.rs` for the FNV
offset basis and prime in seven spellings (assembled at run time from halves, or the needle
list would match itself) and has been observed to fail on a planted constant.

**`SamplingKind` is verifiable now, not merely recorded.** The header names a sampler, a seed
and a node count; all three are inputs to `stream::node_positions`, so
`GraphReader::verify_sampling` regenerates the node set the file *claims* and checks it
against the checksum. A file that lies about any of the three is refused --
`a_file_that_lies_about_its_sampler_is_refused` takes the four-node fixture, whose positions
are hand-placed lat/lons, patches the header byte to say `Spiral`, and watches the reader
decline. `SamplingKind::Supplied` **declines to verify rather than passing**: this crate
cannot reproduce a node set it was handed, and answering "verified" for one would be the
opposite of failing closed. There is no length check inside `verify_sampling` and no
unreachable error arm for one, on the same reasoning that made Task 5 delete `MissingSection`.
Verification costs what the node set costs -- 480 MB and the sampler's own runtime at
20,000,000 nodes -- so it is a separate call and is never folded into `open`, which must stay
a 288-byte operation.

**The `drop_m > 0.0` filter has its own test.** Task 3 found that relaxing it to `>= 0.0`
fails no test, because strictness is enforced twice -- the filter, and `gradient >
best_gradient` initialised at `0.0`, which rejects the zero gradient a zero drop produces.
That is defence in depth and not a hole, but a guard with no test of its own is a guard
nobody can remove *deliberately*. The filter has exactly one behaviour the second guard
cannot supply: **it runs before the coincidence check**, so a neighbour that is not strictly
below is never measured and therefore never reported as coincident. Two nodes at the same
place and the same height skip each other and the graph builds; the same pair at *different*
heights is refused with `CoincidentNodes`. `the_drop_filter_runs_before_the_coincidence_check`
pins both arms, and relaxing the filter to `>= 0.0` now turns exactly that one test red --
verified by running the mutant.

### What a build actually costs

**This is the first thing in the slice to run at planet scale.** Task 4 measured the
sampler's geometry but never its clock and never `node_neighbours` at 20,000,000; Task 5's
file sizes were arithmetic on the layout, honestly labelled as such; the extraction's 1.45 GB
and 2.16 GB came from a standalone crate, not from this code. `src/bin/streambench.rs` is the
harness -- a `[[bin]]` rather than an `#[ignore]`d test, because a test binary holds every
fixture of every test in one process and the question here is peak resident memory for *one*
node count.

**Host:** 13th Gen Intel Core i9-13900HX, 64 GiB, Windows 11 (10.0.26200), cargo 1.98.0,
`--release`, `--no-default-features`. Seed 20260904, Earth radius, datum 0 m, one process per
size; peak working set measured by the parent around the child, so it includes everything the
byte columns leave out.

| n | positions | neighbours | areas | heights | build | write | **total** | **peak RSS** |
|---|---|---|---|---|---|---|---|---|
| 100,000 | 0.004 s | 0.528 s | 0.006 s | 0.055 s | 0.013 s | 0.001 s | **0.61 s** | -- |
| 1,000,000 | 0.097 s | 7.652 s | 0.057 s | 0.562 s | 0.142 s | 0.007 s | **8.52 s** | 151.4 MiB |
| 5,000,000 | 0.852 s | 48.549 s | 0.340 s | 2.919 s | 0.798 s | 0.054 s | **53.51 s** | 840.4 MiB |
| 20,000,000 | 3.780 s | 241.291 s | 1.409 s | 14.759 s | 3.651 s | 0.190 s | **265.08 s** | **3,340.0 MiB (3.26 GiB)** |
| 50,000,000 | 11.453 s | 660.592 s | 3.287 s | 33.598 s | 9.018 s | 0.478 s | **718.43 s** | **8,336.4 MiB (8.14 GiB)** |

**A whole planet is four and a half minutes and 3.26 GiB, and 91% of the time is one
function.** `node_neighbours` is 241.3 of the 265.1 seconds at 20,000,000 and 660.6 of the
718.4 at 50,000,000 -- 91% and 92% -- and it is superlinear, at a **local exponent alpha of
about 1.15**, where `time_ratio = size_ratio ^ alpha`.

**A growth ratio means nothing without the step it is over**, and this table's four steps are
10x, 5x, 4x and 2.5x, so the raw ratios cannot be compared to each other at all:

| step | size ratio | `node_neighbours` ratio | alpha |
|---|---|---|---|
| 100,000 -> 1,000,000 | 10x | 14.49x | **1.161** |
| 1,000,000 -> 5,000,000 | 5x | 6.35x | **1.148** |
| 5,000,000 -> 20,000,000 | 4x | 4.97x | **1.157** |
| 20,000,000 -> 50,000,000 | 2.5x | 2.74x | **1.099** |

(Population and method: the host named above, seed 20260904, `--release
--no-default-features`, `streambench <n>`, wall clock around the `node_neighbours` stage,
**one run per size**. Ratios are the times in the table divided, so 7.652 / 0.528 = 14.49.)

An earlier revision of this section quoted "13.4x, then 6.3x, then 5.0x, then 2.7x" and read
the falling sequence as a *weakening* exponent. The first figure was simply wrong -- the
table gives 14.49x -- and the reading was a category error, because those are ratios over
shrinking steps. Normalised, the exponent is **flat**: 1.161 / 1.148 / 1.157 / 1.099 over a
500x range of sizes. The one real feature is the mild dip at the last step, and **with a
single unreplicated run per size this data cannot distinguish a genuine asymptotic softening
from run-to-run noise**: an independent sweep at clean 2x steps from 125,000 to 4,000,000 on
the same host class gave alphas of 1.055 / 1.122 / 1.118 / 1.184 / 1.100, scatter of the same
size at node counts where nothing special is happening. Read it as a stable n^1.15 until
somebody replicates. Nothing downstream turns on the difference -- at alpha 1.15, 20 M -> 100 M
is about 6.2x rather than 5x -- and the conclusions below (a flat `Vec<u32>`, and wasm32 as
the wall) are unaffected either way.

`Surface::elevation_m` -- the entire existing engine, run once per node -- is 14.8
seconds at 20 M and 33.6 at 50 M, 0.67 to 0.74 us a call, and is **not** the bottleneck.
`StreamGraph::build` itself, which includes the peel and the whole drainage accumulation, is
3.65 seconds at 20 M and 9.02 at 50 M -- **linear in node count, to within a few percent**,
which is the property slice 5 will lean on. `write_graph` serialises 585 MB in 0.19 s and
1.46 GB in 0.48 s.

Peak resident memory is likewise very nearly linear -- 3.26 GiB at 20 M, 8.14 GiB at 50 M,
about 175 bytes a node -- so nothing here has a hidden quadratic term in *space*. The
quadratic-looking cost is `node_neighbours`' time alone, and it is a candidate set that grows
with node count rather than an algorithmic surprise.

**`node_neighbours`' shape does not survive, and that is a finding for slice 5 rather than a
fix for this slice.** Its `Vec<Vec<u32>>` is **1,120,000,000 bytes (1.043 GiB) at 20,000,000
nodes** and 2,800,000,000 bytes (2.607 GiB) at 50,000,000, and 43% of it -- 480,000,000 and
1,200,000,000 bytes -- is `Vec` headers rather than neighbour indices: 24 bytes of pointer,
length and capacity to own 32 bytes of payload, once per node. The measurement confirms Task
4's warning exactly. **The alternative is a flat `Vec<u32>` of `count * k` at the fixed
`NEIGHBOUR_COUNT`**, which is 640,000,000 bytes at 20 M -- a **1.75x saving at every size**,
no per-node allocation, and contiguous access instead of twenty million pointer chases,
which is very likely most of the 241 seconds as well. It is not done here because
`StreamGraph::build` takes `&[Vec<u32>]` in its signature, and changing that signature is a
change to the type this slice exists to freeze; a `k`-strided view is an additive change
slice 5 can make behind the same validation.

**Task 5's arithmetic holds exactly, and one estimate around it does not.** A real encode at
20,000,000 nodes writes **585,419,992 bytes**, and `encoded_len` predicts the same number
before writing a byte. Task 5's **580,000,288** is `288 + 480,000,000 + 80,000,000 +
20,000,000` -- the prefix and the five per-node columns, its stated zero-lake floor -- and the
difference is precisely the lake table it said to add: 225,821 lakes x 24 bytes =
**5,419,704**. Both figures are right as stated. What was wrong is the *sizing* Task 5 offered
for that table: "at most 135,528 bytes if every one of §8.3's 5,647 roots were a lake",
which is **forty times low**, because 5,647 is the extraction's probe field and this crate's
`Surface` produces 225,821 lakes at that size. The lake section is still only 0.93% of the
file, so reading it whole is still right; but 5.4 MB is a fetch a browser notices, and
`read_lakes`' docstring now says so. The **100,000-node region at 2,900,000 bytes (2.77
MiB)** in five range requests reproduces exactly, and that is the number that decides whether
the browser can work at all. The prediction holds at the next size up too: `encoded_len` says
**1,460,841,688 bytes** for 50,000,000 nodes and 451,725 lakes, and `write_graph` writes
exactly that. So the layout arithmetic is now confirmed by encode at two planet-scale sizes,
and a caller can size a file without producing one.

**Where it stops, and why.** Not on this host, at any size tried. 50,000,000 nodes -- two and
a half times the planet size the design calls for -- completes in twelve minutes at 8.14 GiB,
and `MAX_NODES` (`u32::MAX - 1`) is another 86x further out. The limit on this machine is
patience rather than memory: at the measured superlinear rate, the `u32` ceiling would be
weeks in `node_neighbours` alone, which is the argument for the flat layout restated as a
clock. **It stops on wasm32, and
by a wide margin.** A 32-bit heap tops out at 4 GiB; the positions (480 MB), the neighbour
structure (1.12 GB) and the encoded file (585 MB) already total 2.2 GB before the graph's own
580 MB of columns, and peak measured RSS is 3.26 GiB with a 64-bit allocator that is not
paying 32-bit fragmentation. **A whole planet cannot be built or held in the browser under
any rearrangement of these fields**, which is the premise the format was designed on, now
measured rather than assumed. A 100,000-node region at 2.77 MiB is three orders of magnitude
inside that budget, so the region path is not merely the better option -- it is the only one.
Native builds of a whole planet are fine, and a studio that wants one runs it server-side and
ships regions.

### Test counts, with their environment

**Read from a run, never from a report.** There are **five** configurations to run, not
three: the two feature flags are independent, and `wasm` adds a whole test file that the
other three configurations never compile. Every number below is re-derived for this report
from `cargo test -p worldbuilder-engine <flags> -- --list` (listed, and separately
`--list --ignored`), the same two-format cross-check `.github/scripts/assert_counts.py`
applies in CI -- never from a `test result:` line, and never from an earlier report.

| configuration | lib | `blake2_bytes.rs` | `build_fingerprint.rs` | `no_std_math.rs` | `wasm_exports.rs` | listed | ignored | run |
|---|---|---|---|---|---|---|---|---|
| `--no-default-features` | 503 | 4 | 9 | 6 | -- | 522 | 5 | **517** |
| default (the same set -- the crate declares no default features) | 503 | 4 | 9 | 6 | -- | 522 | 5 | **517** |
| `--features python` | 505 | 4 | 9 | 6 | -- | 524 | 5 | **519** |
| `--features wasm` | 503 | 4 | 9 | 6 | 49 | 571 | 5 | **566** |
| `--features python,wasm` | 505 | 4 | 9 | 6 | 49 | 573 | 5 | **568** |

0 from any of the four `[[bin]]` targets (`streambench`, `erosion_convergence_sweep`,
`pond_threshold_survey`, `relief_survey`) and 0 doc-tests in every configuration. All five
`ignored` are the same five, and all five are in `lib`: `stream::measurements::*` (the four
sampling measurements plus `neighbours_match_brute_force`).

**Re-derived twice now: for slice 5a Task 6, and again for the whole-branch review's fix
round that followed it.** Task 6 found this table stale by 35 `lib` tests and 5
`wasm_exports.rs` tests -- every erosion test this slice added (Task 3's convergence loop,
Task 4's thermal cap, Task 5's `wb_erosion_run` refusals) had landed in the source without
this table moving. It last read 404/404/406/404/406 in `lib`, 30 in `wasm_exports.rs` (the
counts the identity slice's own commentary below still describes); Task 6 corrected it to
439/439/441/439/441 and 35. The fix round for the review's HIGH finding (a second abort
band, `radius_m` overflowing `stream::node_areas_m2`'s area calculation to `+inf`) then
added one more `stream.rs` test (`build_refuses_an_infinite_or_nan_area`), uniformly across
all five configurations since `stream.rs` compiles unconditionally, moving `lib` to
440/440/442/440/442; `wasm_exports.rs` went 35 -> 36 net -- not net zero, since a first
version of the fix round's own erosion-side test was itself superseded once
`WB_MAX_WORLD_RADIUS_M` closed the door that test assumed stayed open, and its replacement
is the one test that survived (`gates.yml`'s own inline commentary tells that story in
full, including the miscount an early draft of it made by arithmetic instead of by running
`--list`). Both deltas match `.github/workflows/gates.yml`'s own inline commentary
for the engine job matrix, which **was, at that point in the branch's history,** pinned to
the same run figures (454/454/456/490/492) and was independently re-run for this record
rather than trusted because it agreed. **That pin has since moved again** -- see the next
correction below and slice 5b Task 1's entry further down this section for the current
figures.

**The `wasm_exports.rs` column above read 35 in the row this table has carried since the
review fix round, and that was already stale then: the fix round's own prose two paragraphs
up says it moved to 36, but the table cell was never edited to match.** Re-deriving it now
(slice 5b Task 1, `stream --list` per configuration rather than the arithmetic that produced
the mismatch) confirms 36 is what actually runs today, independent of anything this task
added -- `wasm_exports.rs` has no lake-fill tests, this task added none there. **This task
(slice 5b Task 1, lake basin filling) adds `src/water.rs` unconditionally** -- eleven tests
over `basins_of`, `symmetric_adjacency` and `fill_lakes`/`fill_basins` -- compiled into `lib`
in every configuration since `water` carries no feature gate, same as `stream.rs`'s own
precedent. 440/440/442/440/442 -> 451/451/453/451/453 in `lib`, moving every row's `listed`
and `run` by +11 uniformly; `expect_ignored` stays 5 (this task added no `#[ignore]`d test).
Combined with the `wasm_exports.rs` correction above, the table's run figures move
454/454/456/490/492 -> 465/465/467/501/503. Re-derived the same way as every prior entry in
this section: `cargo test -p worldbuilder-engine <cfg> -- --list` and the same with
`--ignored`, per configuration, counted rather than assumed.

**Task 1's review fix round adds four more tests, uniformly again**: two in `stream.rs`
(`set_lake_level_m_moves_only_the_named_lakes_level`,
`set_lake_level_m_reports_false_for_a_node_with_no_lake`, pinning the new
`StreamGraph::set_lake_level_m` write-back the review's MEDIUM finding asked for) and two in
`water.rs` (`apply_levels_writes_the_filled_value_onto_the_graphs_own_lake_table`,
`fill_basins_and_apply_moves_the_graphs_own_lake_levels`, exercising the new `apply_levels`/
`fill_basins_and_apply` entry points). Both files compile unconditionally, so `lib` moves
451/451/453/451/453 -> 455/455/457/455/457 and every row's `listed`/`run` by +4 again:
465/465/467/501/503 -> **469/469/471/505/507**. `expect_ignored` stays 5. Re-derived the same
way as every entry above, and cross-checked through `.github/scripts/assert_counts.py
cargo-list` itself (the same script `gates.yml` runs), which reported `count OK` at all five
configurations against these figures.

**Slice 5b Task 2 (the lake super-graph and its overflow edges) adds fourteen tests to
`water.rs` and two to `stream.rs`, sixteen uniformly across every configuration** (both files
compile unconditionally): `water.rs` gains the ordering-disagreement fixture and its four
tests (`ordering_disagreement_fixture_has_the_roots_and_mouth_this_test_relies_on`,
`outflow_follows_level_not_root_height`,
`outflow_direction_follows_level_not_root_height_regression_guard`,
`resolve_outflow_edges_is_bit_identical_across_two_runs`), the write-back and terminal-lake
tests (`apply_outflows_writes_outflow_lake_onto_the_graphs_own_lake_table`,
`a_terminal_lake_keeps_the_sentinel`), the cycle-handling tests
(`mutual_overflow_between_two_lakes_is_a_tie_broken_deterministically`,
`a_three_lake_cycle_is_also_caught`, `a_lake_chain_terminating_at_the_sentinel_peels_cleanly`,
`an_outflow_lake_naming_no_real_lake_root_is_refused`), the real-graph property suite
(`resolve_outflows_over_a_real_graph_satisfies_every_property`,
`resolve_outflows_is_bit_identical_across_two_runs_on_a_real_graph`,
`lake_count_is_measured_at_a_stated_node_count`) and a documentation-anchor test
(`worldbuilder_directory_is_not_touched_by_this_module`); `stream.rs` gains
`set_lake_outflow_lake_moves_only_the_named_lakes_outflow` and
`set_lake_outflow_lake_reports_false_for_a_node_with_no_lake`, pinning the new
`StreamGraph::set_lake_outflow_lake` write-back that mirrors Task 1's
`set_lake_level_m`. `lib` moves 455/455/457/455/457 -> 471/471/473/471/473 and every row's
`listed`/`run` by +16: **469/469/471/505/507 -> 485/485/487/521/523**. `expect_ignored` stays
5 (this task added no `#[ignore]`d test). Re-derived the same way as every entry above and
cross-checked through `.github/scripts/assert_counts.py cargo-list` itself, which reported
`count OK` at all five configurations against these figures.

**Task 2's review fix round adds seven more tests, uniformly again**: five in `water.rs`
(`merge_fixture_has_the_two_tied_roots_this_test_relies_on`,
`a_tied_plateau_is_merged_into_one_body_at_its_true_level`,
`merge_leaves_an_untied_lakes_level_untouched`,
`touching_lakes_fixture_is_a_closed_system_and_merge_refuses_it`,
`two_candidate_ordering_fixture_has_the_roots_this_test_relies_on` -- net of removing the
review's Finding 8 "cannot fail" documentation-anchor test,
`worldbuilder_directory_is_not_touched_by_this_module`, and the two-candidate ordering
fixture's own two property/mutation tests already counted as replacements for the prior
round's non-discriminating pair) and two in `streamfmt.rs`
(`refuses_an_outflow_lake_naming_a_node_that_is_not_a_lake_root`,
`a_resolved_graph_round_trips_its_outflow_lake_values`, pinning Finding 1's fix). Both files
compile unconditionally, so `lib` moves 471/471/473/471/473 -> 478/478/480/478/480 and every
row's `listed`/`run` by +7: **485/485/487/521/523 -> 492/492/494/528/530**. `expect_ignored`
stays 5. Re-derived the same way as every entry above, and cross-checked through
`.github/scripts/assert_counts.py cargo-list` itself, which reported `count OK` at all five
configurations against these figures.

**A second Task 2 fix round adds two more tests, uniformly again**: `water.rs` gains
`chained_merge_fixture_has_the_three_roots_this_test_relies_on` and
`a_chained_merge_carries_the_first_passs_full_membership_into_the_second` (review Finding 3:
a merge that needs a second pass, whose correctness depends on the first pass's full
accumulated membership surviving into the second -- verified by mutation to fail, `left: j is
live`, when that accumulation is dropped). `lib` moves 478/478/480/478/480 ->
480/480/482/480/482 and every row's `listed`/`run` by +2: **492/492/494/528/530 ->
494/494/496/530/532**. `expect_ignored` stays 5. This same round also generalised
`merge_tied_plateaus` from cycles to arbitrary ties (Finding 4) and added a pass-count
termination guard (Finding 2), neither of which added a test by itself -- the chained-merge
fixture above is what exercises both. Re-derived the same way as every entry above, and
cross-checked through `.github/scripts/assert_counts.py cargo-list` itself, which reported
`count OK` at all five configurations against these figures.

**The narrative chain above stops at 494/494/496/530/532, and the table no longer does.**
Slice 5b Tasks 3-5 and the whole relief-amplitude slice landed after that paragraph was
written, and this section did not move with them -- which is the failure mode this section
has now suffered three times and warns about in its own next paragraph. The table above was
therefore **re-derived wholesale for slice 5b Task 6 / relief Task 5**, not extended by
arithmetic: `cargo test -p worldbuilder-engine <cfg> -- --list` and the same with
`--list --ignored`, per configuration, plus `--lib` and `--test <name>` runs to attribute the
columns, all on this host at `1004f4d`. The net movement since that last narrative entry is
**+23 in `lib` across all five rows** (`water.rs`'s threshold, manifest and merge work, plus
`detail.rs`'s `ReliefParams` and `hills()`) and **+13 in `wasm_exports.rs`**, which only the
two `wasm` rows see (`wb_relief_preset`, `wb_relief_check`, `wb_world_new_relief` and
`wb_water_run`, with their refusal ladders): 494/494/496/530/532 ->
**517/517/519/566/568**. `expect_ignored` stays 5. The per-task attribution of those deltas
is in `.github/workflows/gates.yml`'s own inline commentary, task by task, which is where it
belongs -- and the five figures re-derived here match that file's pinned `expect:` values
exactly, having been counted before it was read rather than after.

**`build_fingerprint.rs` is new in the identity slice** (Task 2): 9 tests over the shared
walking/hashing logic `build.rs` calls, none of them present when this table last read
409/409/409/439/439. **The `python` rows are two higher in `lib` than their non-python
neighbours** (441 vs 439) because `source_fingerprint()` and `source_fingerprint_inputs()`
are PyO3 exports whose binding tests only compile with `--features python`. These five
numbers are pinned verbatim in `.github/workflows/gates.yml`'s `engine` job matrix and
asserted there by the same `assert_counts.py cargo-list` check, not merely quoted here.

**Running only the first three is how a stale count survives.** This section said **405**
(395 lib) for several commits and named only three configurations, so the 30 (now 35) tests
that hold `wasm.rs` to its declared export list were outside the number the record quoted.
If you quote a test count here, quote all five rows.

The `#[ignore]`d measurements take minutes and
allocate half a gigabyte; they are tests rather than a throwaway script so they cannot rot
silently while the constants they justify stay in the source, and they run with

    cargo test --release --lib --no-default-features -- --ignored --nocapture

`streambench` is not a test and is not counted:

    cargo run --release --no-default-features --bin streambench -- 20000000

## `erosion.rs`: the Cordonnier stream-power bake, and the number nobody had named

Slice 5a implements one implicit step of `dh/dt = u - k * A^0.5 * s` over an existing
`StreamGraph`, iterated to convergence, with a thermal-erosion slope cap wired in per
iteration -- `docs/design/2026-09-02-mark-2-world-studio.md` §14.1's Cordonnier method,
`§14.3`'s bake-not-query argument, `§14.4`'s CORE-001. No lakes, no overflow, no water
manifest, no feature-kernel blend back into terrain -- that is slice 5b, listed at the end
of this section. Every number below was re-run for this record rather than carried forward
from an earlier task's report, because this slice's own history is why that distinction
matters: **Task 4's brief quoted a digest Task 3 had already moved, and Task 5 reported five
correct count pins as "stale by +5" because it counted `--list` lines without subtracting
the five ignored -- on both sides of a `git stash`.** A control wrapped around an
incorrectly defined measurement reads as convincing precisely because the control itself
passes. Where a number below disagrees with an earlier report's, this section's own
measurement is what is recorded, and the disagreement is stated rather than smoothed over
(see "Figures found stale" at the end).

### 1. §14.3's convergence claim holds -- at a governing group nobody had named

§14.3 claims Cordonnier's implicit solver converges in **100-300 iterations**, independent
of resolution. Run at this crate's own default test constants (`u = 1.0e-3 m/yr`,
`k = 1.0e-6 /yr`, `dt = 1000 yr`), the claim looks false: thousands of iterations, and the
count keeps climbing as the convergence threshold tightens. It is not false. It is a
function of one dimensionless group, derived in `erode_step`'s own doc from the closed-form
update:

    c = k * dt * sqrt(A_drainage) / d

(`A_drainage` the node's drainage area in m², `d` the great-circle distance to its
receiver). The closed form moves a node a fraction `c / (1 + c)` of the remaining distance
toward its new steady state each step, so the iteration count to reach a fixed per-step
threshold scales as `ln(1/threshold) / c` -- **the count is a function of `c`, not of node
count, not of "this implementation" as a monolith.** `sqrt(A)/d` is itself close to
resolution-invariant (both terms scale as `1/sqrt(n)` as the mesh refines), so `c ≈ k * dt`
in practice: it is set by the two constants a caller chooses, not by the mesh.

Re-run for this record, `cargo run --release --bin erosion_convergence_sweep` (Windows 11,
`x86_64-pc-windows-msvc`, `cargo 1.98.0`, release only; `SamplingKind::Spiral` nodes over
`Surface::new(20260904, EARTH_RADIUS_M, 22, 0.29, None)` terrain, `u = 1.0e-3 m/yr` and
`dt = 1000 yr` fixed throughout, six node counts from 300 to 100,000 -- just over two
decades):

**Table 1 -- `k` fixed at `1.0e-6 /yr` (`c` median ≈ `1.0e-3` at every size), threshold
varied.** A tighter threshold demanding more work at a fixed `c` is the sanity check that
this sweep is measuring something rather than reporting one number by construction:

| nodes | threshold (m) | `c` median | `c` max | iterations |
|---:|---:|---:|---:|---:|
| 300 | 1.0 | 1.046e-3 | 4.394e-3 | 2,370 |
| 300 | 1.0e-3 | 1.046e-3 | 4.394e-3 | 14,350 |
| 300 | 1.0e-4 | 1.046e-3 | 4.394e-3 | 18,790 |
| 100,000 | 1.0 | 1.171e-3 | 3.364e-2 | 4,581 |
| 100,000 | 1.0e-3 | 1.171e-3 | 3.364e-2 | 19,858 |
| 100,000 | 1.0e-4 | 1.171e-3 | 3.364e-2 | 24,332 |

Thousands of iterations at every size, at this crate's own constants -- §14.3's band does
not appear here, and the `c` column shows why: `c` sits near `1.0e-3` regardless of node
count, essentially flat across a 333x span of `n` (1.046e-3 to 1.171e-3), which is the
resolution-invariance of `sqrt(A)/d` made visible rather than asserted.

**Table 2 -- threshold fixed at `1.0e-3` m, `k` varied instead (the parameter `c` actually
depends on).** This is the decisive comparison: the same unmodified solver, at a `c` an
order of magnitude closer to what a caller wanting §14.3's own timescale would choose.

| nodes | `k` (/yr) | `c` median | `c` max | iterations |
|---:|---:|---:|---:|---:|
| 300 | 1.0e-6 | 1.046e-3 | 4.394e-3 | 14,350 |
| 300 | 1.0e-5 | 1.046e-2 | 4.394e-2 | 2,015 |
| 300 | 1.0e-4 | 1.046e-1 | 4.394e-1 | **253** |
| 300 | 1.0e-3 | 1.046e0 | 4.394e0 | 37 |
| 100,000 | 1.0e-6 | 1.171e-3 | 3.364e-2 | 19,858 |
| 100,000 | 1.0e-5 | 1.171e-2 | 3.364e-1 | 2,521 |
| 100,000 | 1.0e-4 | 1.171e-1 | 3.364e0 | **301** |
| 100,000 | 1.0e-3 | 1.171e0 | 3.364e1 | 43 |

At `c` median ≈ `0.1` (`k = 1.0e-4`), every one of the six node counts this sweep runs
(300 through 100,000, a 333x span) converges in **253 to 301 iterations -- inside or
one iteration outside §14.3's 100-300 band, on the same solver Table 1 shows converging in
the thousands at a different `c`.** One order of magnitude higher (`c` median ≈ `1`) and the
same solver converges in 37-43 iterations, below the band; one order lower (`c` median ≈
`1.0e-2`) and it takes ~2,000-2,500, above it. **The band is a property of `c`, and this
crate's own default test constants (`c` ≈ `1.0e-3`) simply are not in it.** Record the
group, this project's figure rule now says explicitly, where parameters combine into one --
`u`, `k`, `dt` named separately would suggest three independent knobs when the count that
actually matters depends on one ratio of them (with the mesh's own `sqrt(A)/d` folded in,
which is why `c` and not `k*dt` alone is the exact quantity).

### 2. The thermal cap is inert at every resolution this crate measures, and the reason is geometric

Both tables above print `clamped: 0 edges over 0 iterations` on **every one of their 42
rows**, without exception. That is not a coincidence of these particular parameter choices;
it is geometric. A 30-degree slope needs a rise of `tan(30°) * d` between two adjacent
nodes, and `d` is the mesh spacing -- large at the node counts a test suite can afford,
small only near the planetary target. Re-derived directly (not copied from `erosion.rs`'s
own doc comment, though it agrees), taking `d` as the square root of the mean cell area over
a sphere of Earth's radius (6,371,000 m) and the required rise as `tan(30°) * d`:

| nodes | spacing `d` (m) | rise needed for 30° (m) |
|---:|---:|---:|
| 300 | 1,303,923 | 752,820 |
| 10,000 | 225,846 | 130,392 |
| 100,000 | 71,419 | 41,234 |
| 20,000,000 | 5,050 | 2,916 |

Earth's entire relief, Everest to Challenger Deep, is about 19,700 m. At 100,000 nodes --
this sweep's largest size -- the cap would need **more than twice the planet's whole relief
concentrated between two neighbouring nodes** before it could fire; it cannot bind at any
`k` this crate has run. Only near the 20,000,000-node, ~5 km spacing target §14.3 names does
the required rise (2,916 m) fall inside the range of ordinary steep mountain terrain, which
is where the cap stops being decoration.

Two consequences follow, stated rather than left to be inferred from an unchanged number:

- **The sweep exercises `cap_slopes`'s clamp branch not at all.** Every claim that the cap
  corrects a real spike rests on `erosion.rs`'s own synthetic pathology tests (a
  deliberately low-drainage-area node driven past the cap by hand), not on anything measured
  above. A zero that looks like "the cap works, see, nothing needed clamping" and a zero that
  means "the cap was never exercised" are indistinguishable from the outside, which is
  exactly why `ClampStats` reports the count explicitly rather than leaving it to be read off
  an unmoved iteration total.
- **Task 3's convergence figures above are unchanged, not superseded, by Task 4's cap.** The
  cap runs per-iteration inside the same loop these tables measure; had it fired even once on
  any row, the iteration counts above would be Task 4's numbers, not Task 3's. They are
  identical, because the cap did nothing over this corpus.

### 3. Native/WASM parity exists for erosion, and what it does and does not cover

`wb_erosion_run` (slice 5a Task 5) is a real, shipped export -- re-checked here rather than
reported from Task 5's own record. Re-run for this task (`cargo run --release -p
worldbuilder-engine --example parity_dump --features wasm`, replayed by `parity/parity.mjs`
against the committed `viewer/public/wasm/worldbuilder_engine.wasm`; full corpus and method in
`parity/README.md`, which this task also found stale by one whole group and re-derived -- see
"Figures found stale"). **Figures below re-run for slice 5b Task 5**, against the current
220,452-byte artifact and the corpus that task grew to 71,596 values:

| run | values compared | divergent |
|---|---:|---:|
| parity | 71,596 | **0** |
| `--mutate seed` (a different planet) | 71,596 | 68,457 |
| `--mutate erosion-k` (`erodibility_per_yr` bumped one ULP) | 71,596 | **216** |
| `--mutate water-pond` (`pond_max_surface_area_m2`, one threshold) | 71,596 | **60** |

The plain run is bit-for-bit across all 71,596 values, 3,003 of which are the erosion
group's own corpus (3,000 heights from one `wb_erosion_run` call, 3,000 nodes, this crate's
default test constants, capped at 20 iterations so it exercises the not-yet-converged path
deliberately, plus status/iterations/converged). The seed control shows the harness can
detect an entirely different world: 3,000 of the erosion group's 3,003 values move (every
height, not the run's own status/iterations/converged, which a seed alone cannot change).

**The `--mutate erosion-k` control isolates arithmetic from iteration count, which is the
precise thing a seed control cannot show.** Bumping `erodibility_per_yr` by exactly one ULP
moves 216 of the 3,000 heights and *nothing else in the record* -- both `iterations` and
`converged` compare equal on both sides. That is the control doing its one job: proving the
harness can catch a one-ULP divergence in the implicit update's own arithmetic, with the
possibility that "the two sides just ran a different number of steps" excluded by the same
comparison. Confirmed by re-running it for this task, not assumed from Task 5's report.
**And its count did not move when slice 5b Task 5 grew the corpus by 15,342 values** -- which
is the right outcome, since none of the three added groups is downstream of `k`.

**Slice 5b added a third control on the same principle, one level narrower.**
`--mutate water-pond` moves `pond_max_surface_area_m2` alone; that parameter reaches exactly
one field of the water manifest, so 60 of 156 `Body::kind` codes move while every
`root_node`, `level_m`, extent bound, the body count and the datum compare equal -- the same
shape as `iterations` and `converged` comparing equal here. Its 60 is predicted from
`water::lake_body_surface_areas_m2` before the replay runs, and checked against it.

**What this parity claim does not cover.** `cap_slopes`'s own clamp branch is called by
`wb_erosion_run` (it always runs the capped path) but, per section 2 above, is inert at this
export's 3,000-node parity fixture -- so the parity corpus is a genuine bit-for-bit test of
`sqrt`, `atan2` (via `receiver_distances_m`) and the implicit update, and it is **not** a
test of the cap's own clamp arithmetic agreeing between native and WASM. That remains
unverified by parity until a corpus reaches a node density where the cap can fire at all.

### 4. An abort was reachable through `wb_erosion_run`, closed, and not exhaustively searched

`erodibility_per_yr` was originally bounded only by magnitude, so a negative `k` was
in-domain. A negative `k` makes `c` negative; `implicit_receiver_update`'s `1 / (1 + c)`
stops being a contraction and becomes an amplifying map for `c` in `(-2, 0)`, heights
overflow to `±inf` within roughly a hundred iterations at this crate's own `dt`, and the
following iteration's `inf - inf` trips `erode_to_convergence`'s release-time
`assert!(!change.is_nan())` -- correct as a Rust-internal invariant, but `extern "C"` is
nounwind, so the panic aborts the whole module rather than returning a status, both natively
and in the shipped `.wasm`. `tests/wasm_exports.rs::a_negative_erodibility_is_refused_...`
(re-run for this task, `cargo test -p worldbuilder-engine --features wasm`, exit 0, 35
passed / 0 failed) confirms `-9.0e-4`, `-1.0e-3`, `-1.0e-2`, `-1.0`, `-1.0e-6` and
`-infinity` are now all refused with `WB_ERR_PARAM` before the solver runs at all, and that
flipping the same value positive still succeeds and returns only finite heights.

**It was a band, not a cliff.** Only some of the negative values above actually reach the
amplifying overflow within a bounded iteration count; others stay finite over the same
budget. A single spot-check at one negative `k` would have found "that value is fine" and
missed the reachable ones. Closed by requiring `erodibility_per_yr >= 0.0` in the guard
(which also refuses NaN, since `NaN >= 0.0` is `false`), rather than by bounding magnitude
alone or by fixing one specific value.

**No exhaustive search of the remaining in-domain parameter space was done, and this record
does not claim otherwise.** The one path into the assertion that this task found was
reasoning about the sign of `1 + c` for negative `k`; the guard closes exactly that path.
Adversarial combinations of a very large `A_drainage`, a near-ceiling `k` and `dt`, and a
very small receiver distance `d` were not fuzzed against `f64`'s own overflow boundary.
Finding one reachable band is a reason to raise the prior that others exist, not to lower
it -- the numeric-domain ceilings on `wb_erosion_run`'s parameters
(`WB_MAX_EROSION_RATE_PER_YR`, `WB_MAX_EROSION_TIMESTEP_YR`) are chosen to keep `c`'s
multiplication chain far from `f64::MAX` at every value this crate's own fixtures use, which
is a defensive margin, not a proof.

### What this implementation actually costs, measured, not extrapolated past what ran

§14.3's own figure is explicitly arithmetic, not measurement, and is quoted here from the
design doc directly rather than from an intermediate report: **160,000 nodes over a
50 x 50 km domain, about 200 steps, 252 seconds on a 2016 desktop**, extrapolated (their
per-iteration cost scaling worse than linearly, 1.8x the nodes costing 3.2x the time) to
"plausibly many hours" for a 20,000,000-node planet at ~128x their largest run.

This implementation was timed directly for this record -- a scratch timing binary over
`erode_to_convergence_with_clamp_counts` at a fixed 200 iterations (never reaching
convergence, so every size does the same number of steps), release build, this same host,
graph build and the 200-iteration run timed separately:

| nodes | graph build | 200 iterations | per-iteration |
|---:|---:|---:|---:|
| 3,000 | 0.019 s | 0.016 s | 0.081 ms |
| 30,000 | 0.198 s | 0.227 s | 1.137 ms |
| 100,000 | 0.822 s | 0.824 s | 4.120 ms |
| 200,000 | 1.877 s | 2.373 s | 11.864 ms |

Per-iteration cost is not linear in node count over this range either (100,000 -> 200,000 is
2x the nodes for 2.88x the time), the same superlinear shape `streambench.rs`'s own
`node_neighbours` measurement already reports for graph construction and consistent with
`wasm.rs`'s own doc's independent timing (~8.5 ms/iteration at 200,000 nodes on whatever
host that figure came from -- this host's 11.864 ms differs by about 40%, which is a
different-machine spread, not a contradiction; neither figure claims to be host-independent).

**200,000 nodes is the largest size run for this record, and it is 100x short of the
20,000,000-node planetary target -- that gap is not bridged here.** Naively multiplying
11.864 ms by a plausible iteration count and a 100x larger node count would repeat exactly
the extrapolation error this section's own quoted paragraph warns against (per-iteration
cost is already measured as superlinear over a 66x span in this table alone, so a linear
projection two more decades out is not defensible from this data). What can be said from
what actually ran: at the `c ≈ 0.1` regime section 1 identifies as the one landing inside
§14.3's iteration band, and at this host's measured 4-12 ms/iteration in the 100,000-200,000
node range, a bake in the hundreds-of-iterations count is a matter of seconds to low tens of
seconds at these sizes -- not hours. Whether that holds at 20,000,000 nodes is not
established by anything measured here.

### Figures found stale while re-deriving this section

Re-deriving every number above, rather than trusting an earlier report, surfaced several
figures elsewhere in this crate's own documentation that had drifted:

- **This section's own "What is here so far" module count** (top of this file) still read
  eighteen modules and one binary after `erosion.rs`, `wasm.rs` and
  `src/bin/erosion_convergence_sweep.rs` had all landed -- corrected above to twenty modules,
  two binaries.
- **The WASM artifact's own byte count and export count**, quoted twice in this file (the
  WebAssembly surface section, and the module list), still read 84,856 bytes / 11 exports
  after `wb_erosion_run` shipped, which is a twelfth export at 117,146 bytes -- both
  corrected.
- **The "Test counts, with their environment" table** read 404/404/406/404/406 in `lib` and
  30 in `wasm_exports.rs`, from before any of this slice's three tasks landed a test --
  corrected to 439/439/441/439/441 and 35, re-derived directly from `cargo test -p
  worldbuilder-engine <features> -- --list` (and `--list --ignored`) for all five
  configurations on this host, matching `.github/workflows/gates.yml`'s own pinned figures
  exactly (453/453/455/488/490 run, `expect_ignored: 5` throughout) rather than being copied
  from that file.
- **`parity/README.md`'s entire "Recorded output" section** predated the erosion group and
  the `--mutate erosion-k` control: it still quoted 53,251 values / 84,856 bytes with no
  erosion row at all, despite `examples/parity_dump.rs` and `parity.mjs` both already
  containing the erosion-group code that produces the 56,254-value corpus measured above.
  Corrected, with both controls now recorded.
- **`docs/ci.md`'s "current tree" claims** (the parity re-run line, the engine-suite test
  table, and the parity-corpus count-gate description) carried the same pre-erosion figures;
  corrected to match. Its historical paragraph about the identity slice's `.wasm`-byte-diff
  correction was left untouched, since that describes a specific past commit's evidence and
  updating it to today's byte count would misrepresent what was true at the time it
  describes.

None of these were found by disagreeing with a number in a task brief -- every one was
found by running the actual command a stale sentence claimed to summarize and comparing the
output. That is the check this slice's own history says is worth repeating: a correct
control wrapped around an incorrectly defined (or simply un-rerun) measurement is exactly
what let a prior task's report call five correct pins "stale by +5."

### What slice 5b still owed, and what it delivered

**This subsection was written before slice 5b ran. It is kept as written, and answered
below**, because what a slice expected to owe and what it turned out to owe are two different
records and the second is only legible beside the first. The delivered work is the
**`water.rs`** section further down.

Lakes and the overflow super-graph, the water manifest as a byproduct of building them, and
the feature-kernel blend that lets `terrain_z_at` stay a function after erosion has run
(§14.3's own argument for why the query can remain analytic even though the structure is
baked). Also carried forward as a note for whoever picks that slice up: **`sea_level_m` is
hardcoded inside `wb_erosion_run`** (`WB_EROSION_SEA_LEVEL_M = 0.0`) and is **not inert** --
it is what decides which nodes the graph classifies as roots, and therefore which nodes this
slice's solver holds fixed as local base levels. Lakes and water will need to revisit that
constant, not merely add parameters alongside it.

**Slice 5b Task 5 answered the datum half of that note, at a second door rather than at this
one.** `wb_water_run` takes `sea_level_m` as a caller parameter, bounded by the world's own
radius, and moving it moves every body in the manifest. `wb_erosion_run`'s own constant is
untouched -- changing it would change what that export computes, which is not a parity task's
business -- so the two exports still build their graphs at different datums unless a caller
passes 0.0. That is stated on `WB_EROSION_SEA_LEVEL_M` and remains open.

**What was delivered, and what is left.** Lakes, the overflow super-graph, the tied-plateau
merge, the pond classification and the manifest all landed -- see the **`water.rs`** section
below for each with its measurement. Two of the three items above are closed and one is not:

- *Lakes and the overflow super-graph, and the manifest as a byproduct of building them*:
  **done**, and the byproduct claim held -- no separate flood-fill discovery pass exists.
- *`sea_level_m` hardcoded in `wb_erosion_run`*: **still open**, exactly as the paragraph
  above says. `wb_water_run` takes the datum as a parameter; `wb_erosion_run` does not.
- *The feature-kernel blend that lets `terrain_z_at` stay a function after erosion has run*
  (§14.3's own argument for an analytic query over a baked structure): **not started.** It
  was never a Task in slice 5b's plan and no work in this branch approaches it.

## `water.rs`: lakes as a byproduct, a threshold with nothing under it, and a manifest whose sea is a miss

Slice 5b. Every figure in this section was re-derived for this write-up, on this host, at
`1004f4d`, from the current source or from a run performed while writing it -- never from a
task report. Where a ledger paragraph disagreed with a run, the run is what is recorded here
and the disagreement is named.

**Host and toolchain for every measurement below**: this developer machine, Windows 11,
`rustc 1.98.0 (88d9e12ae 2026-08-18) x86_64-pc-windows-msvc`, Node v22.17.0, `cargo run
--release`.

### Fill-versus-breach was DISSOLVED, not decided -- and the difference matters

Section 13.4 of `docs/design/2026-09-02-mark-2-world-studio.md` poses the question in its own
words: a depression "may be **filled**, which makes a lake, or **breached**, which cuts an
outlet and makes a stream. That single choice decides how many lakes a planet has." Read as a
fork, that is a design decision this slice would have had to take.

It never came up, and the reason is section 14.2's rather than this slice's: **Cordonnier does
neither.** A root node that is not on the boundary *is* a lake -- lakes are a property of the
downhill forest the bake already builds, not the output of a discovery pass run over it. There
is no depression-removal step in which one could choose to fill or to breach, because there is
no step that removes depressions at all. Both branches of the fork describe things this
implementation does not do.

The distinction between *dissolved* and *decided* is not pedantry, which is why it is written
down rather than left implicit:

- A decided question has a losing branch that stays reachable. Someone eventually asks "should
  we breach instead?", and the answer is a tuning argument that can be re-litigated forever.
- A dissolved question has no branches. Asking it here gets the same answer that asking a
  raster method "which way does water flow across this flat?" gets: the question presumes a
  representation that is not the one in use. Section 14.2 makes exactly that point and credits
  it to the raster library's own authors -- RichDEM's documentation is candid that once a
  depression is filled no local gradient information survives, and every reconstructed
  drainage direction is equally arbitrary.

What this slice actually had to decide were the questions the graph *does* pose, and they are
different questions: where a basin's water surface sits (Task 1's spill formula, `min` over
boundary edges of `max(h_inside, h_outside)`), which basin it overflows into (Task 2), what
counts as one body when two basins tie (Ruling 7: a tied plateau is **one** body, because
section 13.2's unit is a body with *a* surface level and a tied pair has exactly one between
them), and which bodies are named at all (Task 4). None of those is fill-versus-breach wearing
a different hat.

Do not re-open it. If a future slice wants breaching, that is a change of *method* -- a
different paper -- and not a parameter.

### What lake resolution costs, measured against section 14.2's `O(N + M log M)`

Section 14.2 claims "a super-graph of lakes handles overflow between them in O(N + M log M),
where the number of lakes M is far below the number of nodes N". That is two claims: a
complexity bound, and a size claim about M. Both are checkable, and both were checked here
rather than cited.

**Population and method.** `cargo run --release --no-default-features --bin
pond_threshold_survey`, the binary that already builds exactly this pipeline. Seed
`20260905`, radius 6,371,000 m, `SamplingKind::Spiral`, heights from `Surface::new(SEED,
EARTH_RADIUS_M, 22, 0.29, None, None)::elevation_m`, datum `sea_level_m = 0.0`. The timed
region is `water::fill_basins_and_apply` followed by `water::resolve_outflows_and_apply` --
the whole of lake resolution and nothing else; the graph build (`StreamGraph::build`,
including node sampling and the downhill forest) is timed separately beside it. One run, this
host, wall clock.

| N | graph build | lake resolution | pre-merge `Lake` rows | physical bodies (M) | merge satellites | M/N |
|---|---|---|---|---|---|---|
| 30,000 | 0.19 s | **0.35 s** | 203 | 171 | 32 | 0.570% |
| 100,000 | 0.69 s | **1.24 s** | 1,024 | 799 | 225 | 0.799% |
| 500,000 | 3.94 s | **7.81 s** | 8,467 | 6,368 | 2,099 | 1.274% |

Three things that table says, and one it does not:

- **M is far below N, and the claim survives -- but the ratio CLIMBS with N rather than
  falling.** 0.570% -> 0.799% -> 1.274% of nodes are bodies as the mesh refines by 16.7x.
  That is the opposite of the direction a reader might assume from "far below", and it is
  worth watching rather than filing away: the argument is still safe here, because even a
  5%-of-N lake count at 20,000,000 nodes puts `M log2 M` at about `1e6 * 20 = 2e7`, the same
  order as N itself.
- **The `M log M` term is not what costs anything today.** At N = 500,000, `M log2 M` is
  `6,368 * 12.64 ~= 80,500` -- about **16% of N** -- and that is the bound, not the work:
  there is no sort in the resolve path at all. The ranking is a running minimum in the house
  explicit-branch form (`water.rs::lowest_crossing`). What is measured is an N cost.
- **The measured scaling is superlinear in N, at about `N^1.10`.** 0.35 s -> 7.81 s over a
  16.67x growth in N is a 22.3x growth in time; `ln(22.31) / ln(16.67) = 1.104`. That is not
  section 14.2's term misbehaving -- it is the neighbour relation. A rim scan needs
  `stream::nearest_neighbours`, `StreamGraph` does not store it, so lake resolution
  regenerates it, and its cost is the superlinear part.
- **What the table does NOT license is an extrapolation to 20,000,000 nodes.** The largest run
  here is 500,000. Slice 5a refused to extrapolate its erosion cost past what actually ran and
  the same refusal applies: an `N^1.10` fit measured over one and a quarter decades is not
  evidence about a point two decades further out, and the memory profile changes character
  long before the time does, since the neighbour relation is held live and in full while the
  rim scan runs.

The honest summary is the ratio rather than the seconds: **lake resolution costs about twice
the graph build** at every size measured (1.84x / 1.80x / 1.98x), and the graph build is
itself far cheaper than the erosion bake it feeds. Section 14.2's "lakes are part of the
algorithm, not a separate pass" is a claim about cost as much as about structure, and at these
sizes it holds.

### The pond threshold, the distribution it was chosen from, and the finding underneath it

`BuildParams::pond_max_surface_area_m2 = 1.0e5` m^2 (10 hectares).

**How the quantity was chosen: it was drainage area first, and a measurement changed it.**
Over the same bodies, the set in the bottom decile *by drainage area* and the set in the
bottom decile *by surface area* overlap by only **0.24 / 0.18 / 0.17** at 30,000 / 100,000 /
500,000 nodes (my run, same binary and population as the cost table above). So roughly four in
five of the smallest bodies were being named by a quantity a player cannot see: a small pond
fed by a wide valley classified as a lake, a broad shallow lake in a flat basin classified as
a pond. Section 13.2 gives a body "a surface level" and never makes `pond` a hydrological
category, so visible extent is the right discriminator. Drainage area was **not** deleted --
it is still the right quantity for flow, flooding and foraging, and it is still carried, as
`water::lake_body_drainage_totals_m2`. It is simply no longer the classifier.

**The distribution the threshold was chosen against** -- surface area per *physical body*,
after Ruling 7's merge, so a tied plateau contributes its union's footprint once rather than
its members' footprints severally:

| N | bodies | min m^2 | median m^2 | mean m^2 | max m^2 |
|---|---|---|---|---|---|
| 30,000 | 171 | 1.345e10 | 1.921e10 | 5.064e10 | 9.597e11 |
| 100,000 | 799 | 4.046e9 | 5.583e9 | 1.556e10 | 1.054e12 |
| 500,000 | 6,368 | 7.904e8 | 1.191e9 | 3.163e9 | 1.101e12 |

**There is no natural break.** Across a log-spaced ladder from 1.0e4 to 1.0e12 m^2 the pond
fraction climbs smoothly from 0 to 1 over about three orders of magnitude, at every node
count. That was a permitted outcome and it is reported as one, rather than papered over by
picking the flattest-looking spot on a smooth curve.

**So the threshold stands on external ground rather than on a feature of the curve.** 1.0e5
m^2 is a body whose shoreline a person could walk in about a quarter of an hour: a 1 km
perimeter encloses `1e6 / (4 * pi) ~= 7.96e4` m^2, rounded to 1.0e5. That is a statement about
people, not about the mesh -- which matters, because the candidate ground it replaced ("the
smallest body the finest tested resolution resolves") was a property of the mesh and would
have moved every time the mesh did.

**And here is the finding, which is neither a defect nor a success.** At 1.0e5 m^2 this
generator produces **zero ponds at every resolution measured**. The smallest body it makes
anywhere in the table above is `7.904e8` m^2 -- **7,904 times the threshold**, nearly four
orders of magnitude. `LakeKind::Pond` is a reachable variant of an unreachable case.

Read that as a measurement of the mesh, because that is what it is. At 500,000 nodes over an
Earth-sized sphere the mean cell is on the order of `4 * pi * R^2 / N ~= 1.0e9` m^2; a body
cannot be much smaller than one cell, so a 1.0e5 m^2 pond sits about four orders of magnitude
below what this sampling can represent at all. The threshold is not wrong and the classifier
is not broken. **This project has simply never run the generator fine enough to need the
category.** The alternative -- moving the number until something fell on each side -- would
have produced a tidier table and hidden the only thing the exercise actually discovered.

One consequence survives the resolution question and in fact answers it. At a fixed *high*
threshold of 3.0e10 m^2 the large-body count is **82 / 67 / 54** across that 16.7x growth in
N, while the small-body count balloons **89 / 732 / 6,314**. So refining the mesh does not
destabilise an absolute threshold in m^2 -- it **adds small bodies to the population**. The
threshold is stable; the population is what changed. That is a much weaker claim than "the
threshold swings with resolution", and it is the one the numbers support.

### What the manifest carries, and what it deliberately leaves empty

`WaterManifest` is three fields, and two of them are deliberately thin.

    WaterManifest { sea_level_m: f64, bodies: Vec<Body>, rivers: Vec<River> }
    Body          { root_node: u32, kind: BodyKind, level_m: f64, extent: Extent }
    BodyKind      { Lake, Pond }
    Extent        { min/max_latitude_deg, min/max_longitude_deg }

**The sea is the FALLBACK, not a row in the mapping.** This is the shape decision a reader is
most likely to get backwards, so it is stated flatly: **ocean bodies are not enumerated at
all.** `BodyKind` has no `Ocean` variant. The datum appears exactly once, as the scalar
`sea_level_m`, and it does two jobs at once -- it is what the mapping's miss answers with, and
it is the datum at which mouth-versus-lake was decided for every body that *is* in `bodies`.
A manifest that did not name its own datum could not be checked against the world it
describes, since the same graph yields a different manifest at a different one.

Section 13.2 defines two complementary mechanisms: "a mapping of named waters", and a fallback
for the unnamed. The sea is the mapping's **miss**. An earlier draft emitted one `Ocean` body
per boundary root, and measurement is what settled it: at n = 30,000 that produced 1,232 rows
of which 1,061 were ocean, **96.3% of the ocean boxes overlapped another ocean box** -- so a
sea position selected an ambiguous set of entries with identical levels and no rule to
disambiguate them -- and **61 of 171 lakes had their boxes hit by an ocean box**, manufacturing
exactly the false hits an extent exists to prevent. Removing them cost 86% of the rows and no
information whatever. The counter-argument that a single merged sea body would have a bounding
box spanning the globe turned out to be the proof rather than the objection: a `Body` here is
a thing with a *meaningful extent*, and the sea has none.

**`rivers` is always empty at Mark 2, and the type exists anyway.** Section 13.2 asks a river
to be "an ordered set of reaches"; `stream::Reach` already carries `gradient`, which is what
would let a later slice hang a section 13.3 waterfall off a reach's upstream end. Carrying the
empty `Vec<River>` now costs nothing, and retrofitting the *sequence* later would be a schema
break. That is the spec's own argument, and it is why an always-empty field is not dead weight.

**`kind` has exactly one reachable value on this mesh**, for the reason the section above
measured. A manifest whose enum has two variants and whose generator can only produce one is
worth saying out loud here, rather than leaving a future reader to infer it from a filter that
always comes back empty.

**`root_node` is beyond section 13.2's literal three fields**, and it earns its place as the
mapping's *key*: the spec asks for a mapping of named waters, a mapping needs a key, `Body`
carries no name field, and `graph.lakes()`'s own root-node identity is the only stable
candidate. It does leak this crate's node indexing. Correlating a body across two manifests of
the same world, or back to the graph it came from, is a real need that key exists to serve.

**`extent` is a lat/lon bounding box over the body's UNDERWATER FOOTPRINT** -- basin members
at or below its own filled `level_m`, not its catchment -- chosen for maritime's
point-in-region lookup, which is what the manifest is for. It is index-independent and cheaper
than a node set, and unlike a centroid-plus-radius it does not repeat the summary-number
failure the pond survey had already found once, where a single scalar stood badly for a shape.
The antimeridian *is* reachable and is handled rather than documented away: the wrap convention
(`min_longitude_deg` greater than `max_longitude_deg` denotes an arc through +/-180) exists
because a claim that a globe-spanning box was unreachable was falsified by measurement -- 6 of
171 bodies at n = 30,000 and 8 of 799 at n = 100,000 were already producing 356-360 degree
boxes, one of them a single-node body at 358.7 degrees. That is the second "unreachable" claim
in this project measurement has overturned, which is why they get tested now rather than
reasoned about.

### Parity over the manifest, and a control that moves exactly one field

Everything here is from a parity run performed while writing this section, not from a CI log
and not from a task report. Five commands, all green, exit 0:

    cd crates/worldbuilder-engine/parity
    cargo run --release -p worldbuilder-engine --example parity_dump --features wasm > native.txt
    node parity.mjs native.txt                        # 71,596 compared, 0 divergent
    node parity.mjs native.txt --mutate seed          # 68,457 of 71,596 divergent
    node parity.mjs native.txt --mutate erosion-k     # 216 of 71,596 divergent
    node parity.mjs native.txt --mutate water-pond    # 60 of 71,596 divergent

The clean run also reports the artifact it replayed through: `220452 bytes`, and
`provenance: the shipped .wasm matches its manifest and current source` before it compares
anything.

**What was compared.** `water.rs` was **unreachable from the export surface** before slice 5b
Task 5. No export touched a `StreamGraph`'s lakes, so the module's native-versus-WASM claim
was not merely unverified, it was *unfalsifiable* -- the same position `erosion.rs` was in
before `wb_erosion_run` existed. `wb_water_run` is the export that makes it checkable, and
what it dumps is the **shipped** manifest (`water_manifest_from_graph`, after filling, overflow
resolution, Ruling 7's merge and classification), not an intermediate a caller never sees. The
`water/plain` group is 1,095 values: one status, one body count, one datum, and 156 bodies x 7
fields, at seed `20260904`, 30,000 nodes, datum 0.0, `pond_max_surface_area_m2 = 1.0e5`. All
156 are lakes and none is a pond, and `examples/parity_dump.rs` asserts that directly rather
than leaving it to be noticed.

**What the negative controls moved, and why one of them is the interesting one.**

| control | what it perturbs | divergent | where |
|---|---|---|---|
| `--mutate seed` | the world seed, by one | 68,457 of 71,596 (95.6%) | every group, gross |
| `--mutate erosion-k` | `erodibility_per_yr`, by one ULP | 216 of 71,596 | `erosion/erosion` only, 216 of its 3,000 heights |
| `--mutate water-pond` | `pond_max_surface_area_m2`, 1.0e5 -> 2.0e10 | 60 of 71,596 | `water/plain` only |

**A control that moves everything is as uninformative as one that moves nothing.** That is the
whole reason the second and third exist. `--mutate seed` proves only that the harness notices
a different planet; it says nothing about whether the harness is sensitive to the arithmetic
of any particular module, because a corpus that compared nothing but a seed-derived value
would light up just as brightly. The two narrow controls are the ones carrying a claim:

- `erosion-k` moves 216 of the erosion group's 3,000 heights (7.2%) and **nothing else** --
  not status, not the iteration count, not the convergence flag, and not one value in any
  other group. The corpus is built so both runs hit `max_iterations` without converging, so
  the step counts are identical by construction and a divergent height is evidence about
  `k`'s arithmetic rather than about a different number of steps. Its count is **unchanged at
  216** across slice 5b's corpus growth, which is the right outcome: the groups Task 5 added
  are downstream of neither `k` nor erosion, and a control whose count had moved with them
  would have been reaching something it does not name.
- `water-pond` is the sharpest of the three, because **its count is predicted before the run
  and checked from two directions**. `pond_max_surface_area_m2` reaches exactly one field --
  `Body::kind` -- so `root_node`, `level_m`, all four extent bounds, the body count and the
  datum must all compare equal, and they do. How many `kind` fields flip is predicted
  *natively*, from `water::lake_body_surface_areas_m2`, which is a different quantity from the
  classifier being perturbed: a body flips exactly when its summed surface area is at or below
  the new threshold, which is **60 of 156**. `examples/parity_dump.rs` asserts that the
  classifier and the area distribution agree before writing that prediction into the corpus,
  and `parity.mjs` checks its own per-group tallies against it and exits 1 if any group -- water
  or not -- moved by a different amount. 60 of 156 is deliberately neither none nor all;
  2.0e10 m^2 sits near the median of this mesh's measured body-surface distribution for
  exactly that reason.

**One gate here depends on a MEASURED property of the world, and must be RE-DERIVED rather
than merely re-run.** The corpus total of 71,596 contains `156 bodies x 7 fields + 3`, and
**156 is a measurement** of seed 20260904 at 30,000 nodes with datum 0.0 -- not a constant. If
the mesh, the sampler, the seed, the node count or the datum moves, that pin moves with it,
and the correct response is to re-derive the corpus arithmetic from its definition (the way
`gates.yml`'s own inline commentary does it, line by line) and then check the run against the
derivation -- **not** to paste in whatever number the new run printed. The same applies to the
water control's 60. A pin edited to match the run it was supposed to constrain is not a gate.
This paragraph exists because the next person to change the mesh will hit it.

### The artifact roughly doubled, and the export is deliberately not feature-gated

Measured, not copied. Both artifacts built here with the same command
(`cargo build -p worldbuilder-engine --release --target wasm32-unknown-unknown
--no-default-features --features wasm`, rustc 1.98.0 x86_64-pc-windows-msvc): the "before"
from a clean worktree checked out at `3afa5f5`, the "after" the committed artifact whose
provenance gate passed in the same session. Sizes from the files, export and import counts
from `WebAssembly.Module.exports` / `.imports` in Node.

| | bytes | exports | imports |
|---|---|---|---|
| before `wb_water_run` (`3afa5f5`) | 118,964 | 15 | 0 |
| with it (`1004f4d`) | **220,452** | **16** | 0 |

**+101,488 bytes, a factor of 1.853, from one export.** The export itself is small; what it
costs is `water.rs` -- 3,352 lines of basin partitioning, rim scanning, union-find merging,
classification and manifest assembly, every line of which dead-code elimination had been
discarding, because nothing `#[no_mangle]` reached it.

**Feature-gating it would be worse than the cost.** The entire value of this harness is that
it compares **the artifact that actually ships**. Gate the export and the parity run proves
bit-equality for a build nobody loads, which is a longer-winded form of the exact failure the
harness exists to prevent: a comparison over nothing also reports zero divergent. 220 KB is
small against a single imagery tile, and `wb_erosion_run` set the same precedent already. Cost
if this is wrong: about a hundred kilobytes on a browser's first load, recoverable at any time
by gating the export and accepting a weaker parity claim in exchange. That trade is available
and has not been taken.

The viewer does not call `wb_water_run` yet. That is the honest state: the browser pays for an
export it does not use, so that the export it will one day use is the one that was tested.

## `detail.rs`, revisited: relief amplitude, three multiplying causes, and what still does not look like a mountain

The relief-amplitude slice. Same discipline as the section above: every number here was
re-derived on this host at `1004f4d` by running `cargo run --release --bin relief_survey`
while writing this, and nothing was copied from a report. Where surviving ledger prose
disagrees with the tables, the tables are what is recorded.

**Population, method and parameters, once, for the whole section.** Seed `20260904`, radius
6,371,000 m, `generation::DEFAULT_PLATE_COUNT = 22` plates, `continentality::LAND_FRACTION =
0.29`. Two site populations, both fixed against a **canonical** (`relief: None`) reference
surface and reused unchanged at every configuration, so which sites count never moves as the
swept parameters do:

- **land**, 42 sites: a 9 x 12 lat/lon grid (20 deg x 30 deg), 108 candidates, kept where
  `structural_m > 0`; 37 of them are "high ground" at `structural_m > 300` m.
- **peaks**, 20 sites: a 35 x 72 candidate search at 5 deg spacing, ranked by `structural_m`,
  top 20 kept. Their structural elevations run 1,690.7 m down to 913.5 m.

**Relief** at a site is `max - min` over a 2 km transect: a `TangentFrame::at_latlon` centred
there, sampled along local east from -1000 m to +1000 m at 50 m spacing, 41 points, each from
`Surface::elevation_m(point, None)`. **Max gradient** is the largest `|delta elevation| / 50 m`
between adjacent samples anywhere in the population. **Detail share** is
`|elevation_m - structural_m| / elevation_m` at a site's own point.

**The governing group is the Hurst exponent**, `H = ln(1 / persistence) / ln(lacunarity)` with
`lacunarity = 2.0`, the octave schedule's own wavelength ratio. The swept fields do not act
independently and quoting `octave_persistence` alone would name a parameter rather than the
thing it governs: 0.50 -> **H = 1.0000**, 0.65 -> **0.6215**, 0.71 -> **0.4941**, 0.75 ->
**0.4150**.

### The flatness had three causes, and they multiply

Baseline, `canonical()` -- `mountain_m = 150`, `quieting_strength = +0.70`,
`octave_persistence = 0.50`, H = 1.0:

| population | relief median | p90 | **max** | max gradient | detail share median / max |
|---|---|---|---|---|---|
| land (42) | 3.42 m | 7.49 m | **14.11 m** | 1.194% | 3.62% / 10.72% |
| peaks (20) | 4.02 m | 7.33 m | **10.32 m** | 1.128% | 1.15% / 4.05% |

Four metres of median relief over a two-kilometre run, and *less* of it on the peaks than on
ordinary land. Three separate things were producing that. Each is measured below by moving one
field and holding the other two at canonical, reported on the peak population's relief maximum:

1. **The amplitude was too small.** `mountain_m` 150 -> 600 (x4): peak relief max
   **10.32 -> 39.90 m**, a factor of **3.87**. This is the only one of the three that is
   simply a magnitude.
2. **The quieting term was suppressing roughness exactly where the ground was interesting.**
   `quieting_strength` +0.70 -> -0.70: peak relief max **10.32 -> 19.67 m**, a factor of
   **1.91**. The term is `1 - quieting_strength * smooth(|tectonic_m| / quieting_scale_m)`, so
   a positive strength makes ground *smoother* the larger its tectonic offset is. **On
   ordinary land it is inert**: in the survey's land table at `mountain_m = 150`, every
   quieting value from +0.70 to -0.70 gives relief median 3.42 m, max 14.11 m and max gradient
   1.194%, identical to three decimals. A land-only sweep would have concluded the parameter
   did nothing at all. The peak population is where it shows, because the term can only bite
   where `tectonic_m` is large.
3. **The spectrum was starved at the fine end.** `octave_persistence` 0.50 -> 0.65: peak
   relief max **10.32 -> 16.51 m**, a factor of **1.60**. The schedule is seven octaves,
   20,000 m down to 312.5 m, normalised so total amplitude is whatever the caller asked for.
   Computed from `Detail::plan`'s own `share *= octave_persistence` and that normalisation:

   | persistence | 20 km | 10 km | 5 km | 2.5 km | 1250 m | 625 m | **312.5 m** |
   |---|---|---|---|---|---|---|---|
   | 0.50 (H = 1.0000) | 50.39% | 25.20% | 12.60% | 6.30% | 3.15% | 1.575% | **0.787%** |
   | 0.65 (H = 0.6215) | 36.80% | 23.92% | 15.55% | 10.11% | 6.57% | 4.27% | **2.776%** |
   | 0.75 (H = 0.4150) | 28.85% | 21.64% | 16.23% | 12.17% | 9.13% | 6.85% | **5.135%** |

   The finest octave -- the only one a walking observer resolves -- carried **under one
   percent** of the amplitude. **This is also where an a priori argument was wrong and the
   measurement corrected it.** Slope is scale-invariant at H = 1, so persistence "should" have
   redistributed relief across scales without moving the worst gradient much. It moves it a
   lot: land max gradient 1.194% -> 5.196% across persistence 0.50 -> 0.75. The argument
   assumed an *infinite* self-similar series and this schedule is a *finite* seven octaves, so
   changing persistence directly reweights the finest band -- 0.787% to 5.135% -- and that
   shows up as gradient. Persistence is not the scale-neutral knob it looks like.

**And they multiply.** 3.87 x 1.91 x 1.60 = **11.79**. All three together, which is what
`hills()` is, takes the peak relief max **10.32 -> 124.25 m**: a factor of **12.04**. The
three causes compose to within 2% of the product of their separate effects, which is why the
flatness could not be fixed by turning up any one of them -- at x4 amplitude alone the ground
is still under 40 m of relief over 2 km.

### Why the default cannot move

`worldbuilder/terrain/detail.py` carries the same constants this module does --
`CANONICAL_WAVELENGTH_M = 250.0`, `ABYSSAL_M = 55.0`, `INTERIOR_M = 80.0`, `MOUNTAIN_M =
150.0` -- and **that module is the conformance oracle**. `tests/test_conformance.py` compares
this crate against it, so a changed default is not a change to the engine, it is a change to
the standard the engine is measured against.

Checked here rather than repeated from anywhere: `pytest tests/` with
`WORLDBUILDER_REQUIRE_ENGINE=1`, cross-checked through `.github/scripts/assert_counts.py
pytest` against a `--collect-only -q` collection, reports **398 passed, 157 of them in
`tests/test_conformance.py`** -- `count OK`, exit 0, `398 tests collected` from the collection
half. Unchanged by everything in this section, which is the entire safety claim and the only
proof of it that matters.

So the relief work is **opt-in and nothing else**. `ReliefParams` is carried as an `Option` on
`Surface::new`, following `features: Option<FeatureInput>`, and an explicit `None` means
canonical -- not `Default::default()`, because this codebase rejects defaults nobody chose.
`ReliefParams::canonical()` is byte-for-byte what it was, and `None` and
`Some(ReliefParams::canonical())` produce bit-identical worlds. Moving the default is an
owner's decision about the oracle, not an implementer's commit.

### `hills()`, and the ground each of its three fields stands on

    ReliefParams::hills() = ReliefParams {
        mountain_m: MOUNTAIN_M * 4.0,   // 600.0
        quieting_strength: -0.7,
        octave_persistence: 0.65,
        ..ReliefParams::canonical()
    }

A preset with no argument is taste. Each field has one:

- **`octave_persistence: 0.65`** is chosen for **H = 0.6215**, the only swept value that lands
  inside real terrain's measured Hurst band of roughly **0.60-0.71** (Gagnon, Lovejoy &
  Schertzer, over four DEMs). 0.50 gives H = 1.0000 and 0.71 / 0.75 give 0.4941 / 0.4150, all
  three outside it. Note what this ground is *not*: it is not "the value that produced the most
  relief". 0.75 produces considerably more -- see the corner below -- and is rejected.
- **`quieting_strength: -0.7`** is the exact negation of canonical's `+0.7`, not a magnitude
  fished out of the sweep's extreme corner. Two grounds. First, the measurement in cause 2
  above: inverting the sign roughly doubles relief where tectonics are already large and does
  nothing whatever on generic land, so the flip *chooses where* rather than manufacturing
  relief everywhere. Second, it mirrors a shipped mechanism instead of inventing one --
  Outerra's developer describes amplitude that *rises* with slope ("the amplitude of noise is
  modulated by slope -- flat areas have less noise, while the steeper get more") and with
  positive curvature, to fix flat mountaintops. **It is an APPROXIMATION of that mechanism and
  is flagged as one in the source.** This engine keys the term on **tectonic magnitude**;
  Outerra keys it on **slope and curvature**. Those correlate -- a plate boundary is where the
  steep ground tends to be -- but they are not the same input, and they disagree in real
  places: an old, eroded, tectonically quiet range is steep with small `tectonic_m`; a young
  margin with large `tectonic_m` and little uplift yet is the reverse. Matching Outerra
  properly is a mechanism change and is deliberately not attempted here.
- **`mountain_m: MOUNTAIN_M * 4.0`** is chosen against **Hammond's landform classification**,
  in which hills carry 80-160 m of local relief over a 2 km run and low mountains start at
  300 m. With the other two fields as above, x4 gives the peak population a relief max of
  **124.25 m** -- inside the hills band, with headroom below its 160 m ceiling and nowhere near
  the 300 m mountain floor. x3 (450.0) also lands in the band, at **93.37 m**, but fills less
  of it. x4 was chosen for best filling a published target band, not for being the largest
  multiplier swept.

The whole `hills()` row, from my run, both populations:

| population | relief median | p90 | max | max gradient | detail share median / max |
|---|---|---|---|---|---|
| land (42) | 13.75 m | 25.74 m | 57.98 m | 9.298% | 5.83% / 19.04% |
| peaks (20) | 43.47 m | 88.50 m | **124.25 m** | 20.241% | 8.13% / 30.39% |

The preset crosses the WASM boundary **as fields, not as a name**: `wb_relief_preset` hands
back the numbers and `wb_world_new_relief` takes a validated block. The panel must *show* what
it is building with, and a build-by-name export cannot do that. A test strips comments from
`controls.js` and `main.js` and asserts that 600, 0.65 and -0.7 appear in neither, so the
second copy of the preset a panel would otherwise grow is a failing test rather than a
remembered rule.

### What still does not look like a mountain

This is the most important paragraph in this section, and the slice does **not** end with the
problem solved.

**The roughness spectrum has a measured ceiling, and it is nowhere near a mountain.** The most
extreme corner of the 80-configuration sweep -- `mountain_m` x4, `quieting_strength = -0.7`,
`octave_persistence = 0.75` -- tops out at **161.34 m of relief on the peak population and
82.10 m on land**. Hammond's low mountains begin at **300 m** over the same 2 km run. The
corner is not close, and there is no corner further out: the sweep was built to bracket the
usable range, and the ceiling is inside it. (The relief slice's own ledger prose has those two
figures the opposite way round, "161 m on land and 120 m on peaks"; the tables say otherwise,
and the tables are what a re-run reproduces. The *conclusion* is unchanged, and slightly
stronger for peaks.)

Three things make that ceiling real rather than an artefact of where the sweep stopped:

1. **The corner is already outside the physical band.** It reaches 161 m only at persistence
   0.75, H = 0.4150, well below real terrain's measured 0.60-0.71 -- and it buys the relief by
   making the finest octave 5.1% of the amplitude instead of 2.8%, at a max gradient of
   **36.62%**. That is not a landform, it is a rougher texture. Inside the Hurst band, at
   `hills()`, the ceiling is 124.25 m.
2. **Mountain height is structural, and the detail term is a rounding error against it.** At
   `hills()` the detail term is **8.13% of elevation at the median peak site and 30.39% at its
   maximum**; the other ninety-odd percent is `structural_m`, which is tectonics and does not
   depend on any `ReliefParams` field at all. The peak population's own structural elevations
   run **1,690.7 m down to 913.5 m** -- a kilometre and a half of relief the roughness spectrum
   neither produced nor can move. **A mountain is not reachable from these parameters at any
   setting, because these parameters are not what makes mountains.**
3. **The slopes are not mountain slopes either.** `hills()`'s worst gradient anywhere in the
   peak population is 20.241%, about **11.5 degrees**. The thermal-erosion correction in
   `erosion.rs` caps slopes at 30 degrees and never engages here. Nothing in this parameter
   space produces ground the erosion model would consider steep.

So: **`hills()` is the best hills this roughness spectrum can produce, and it is not a
mountain-height feature.** It does not close the owner's mountain-height requirement and it
must not be read as progress towards it -- the two live in different terms of the same sum.
Mountain height, and mountain *count*, are tectonic-side work: `tectonics.rs`, `generation.rs`
and the plate model, a separate slice with its own oracle problem. The measurement above is
what promotes that from an assumption to a finding. What this slice established is the
negative result that makes that slice necessary, which is worth more than a knob that looked
like it might have been enough.

Also open, and named here so it is not rediscovered: `quieting_strength` remains keyed on
tectonic magnitude rather than on slope and curvature, so the Outerra mirroring stays an
approximation rather than a match; and the amplitude ceiling has only been measured against
`mountain_m` up to x4 (600 m). Larger values were not swept, and there is no evidence here
about whether they degrade into noise before they reach a landform band.

## `tectonics.rs`, revisited: a range is an envelope times a structure field, and a margin is a great circle

The mountains slice, six tasks. Same discipline as the two sections above: **every number here
was re-derived on this host while writing it**, from current source or from a run performed for
this write-up -- `cargo run --release --bin mountain_probe` and `cargo run --release --bin
mountain_survey`, whose full output is kept beside the slice's reports as
`mountain-survey-task6.txt`. Nothing is copied from a task report, and where a report or the
ledger disagrees with a run, **the run is what is recorded and the disagreement is named**.

**Population, method and host, once, for the whole section.** The owner's own world, from their
screenshot: seed **123925603**, radius **4,500,000 m**, **28** plates, land fraction **0.16**.
Peak over a 0.5-degree global grid (720 x 359 = 258,480 sites) refined at 0.05 degrees in a
2-degree box around the coarse maximum. **Grade** is the steepest single 2 km step on the
*flank*, twelve bearings walked out from the peak to 250 km -- never across the summit, which
is the one place a mountain is flat. **Summits** are local maxima above 1,000 m with at least
300 m of prominence (the external P300 rule) in a +/-3-degree box at 0.02 degrees. **Crest
sinuosity** is the crest's path length over the chord between its endpoints, at a 5 km
along-step and a 2 km across-step; a path length on a rough line grows as the step shrinks, so
these figures are comparable only to each other. Native release build, this developer machine
(Windows 11 10.0.26200, x86_64-pc-windows-msvc, cargo 1.98.0).

### The measurement that started it, and it is not about roughness

The owner asked twice, and the second time was after the relief slice had finished: *"still no
mountains."* The reason is one number.

**The peak on their world is 1,454.0 m, of which 1,437.8 m is `structural_m`. 16.2 m is detail:
the peak is 98.9% tectonic.** No relief parameter could ever have moved it -- which is what the
relief slice's own closing section concluded from the other direction, and what the two
`NOT_WIRED` entries on the viewer's panel had been saying in words.

And the ramp was a constant, not an accident. `CONTINENT_COLLISION_M = 1500.0` over
`CONTINENT_COLLISION_WIDTH_M = 400_000.0` is a **0.375% grade** at the profile's own scale, and
**1.787%** measured as a steepest 2 km step on that world's flank. "Wheelchair ramps", months
of work earlier, was an accurate reading of two constants.

### The finding that reframed the slice: WE ALREADY HAD A REAL OROGEN'S GRADE

Davis, Suppe & Dahlen (1983), *JGR* 88(B2), Table 1, read in the source text: **Himalaya
alpha = 4.0 +- 0.5 degrees** -- a **7.0% surface slope**. And the probe, driven to 6,000 m over
100 km, measures **7.030%**:

| collision x width | peak | structural | grade |
| --- | --- | --- | --- |
| **1,500 m / 400 km -- canonical** | 1,454.0 m | 1,437.8 m | **1.787%** |
| 1,500 m / 200 km | 1,423.3 m | 1,408.2 m | 1.809% |
| 1,500 m / 100 km | 1,377.6 m | 1,366.8 m | 2.575% |
| 3,000 m / 400 km | 2,500.0 m | 2,485.1 m | 2.677% |
| 3,000 m / 150 km | 2,441.1 m | 2,430.6 m | 2.852% |
| 6,000 m / 150 km | 4,551.5 m | 4,547.7 m | 4.936% |
| **6,000 m / 100 km** | 4,540.5 m | 4,542.9 m | **7.030%** |

**Amplitude alone buys height and almost no steepness** -- 3,000 m at the canonical 400 km is
2.677%, barely above canonical -- **and amplitude with width buys both.** That much was the
plan's hypothesis and it survived.

**What did not survive is the assumption underneath it.** 7.030% is the Himalayan surface slope
on the nose, and the thing it produced still did not read as a range: it is one smooth swell,
4.5 km high, with **two** summits on it. **Steepness was never the missing quantity.** A range
is a broad envelope saying *where*, times a structure field supplying the *shape*, and this
generator had only the first. The summit count is the shortest statement of it:

**canonical 0 summits -> the 6,000 m / 100 km blade 2 -> the shipped preset 10.**

### The three techniques that shipped, and the one that was rejected

Every row below sits on the 6,000 m / 100 km envelope, so each table answers "what does this add
to what we already ship". `canonical()` is untouched throughout and the `None` path is
bit-identical to it; Python conformance is 398/398 with `tests/test_conformance.py` = 157 on
both sides of the whole slice.

**1. The doubly-vergent asymmetric wedge -- `collision_asymmetry`, canonical 1.0. SHIPPED.**

Naylor & Sinclair (2008), read in the source text: a **115 km pro-wedge against a 69 km
retro-wedge** at `H_max = 3 km`, ratio **1.67**, with surface angles `alpha_pro = 1.5` and
`alpha_retro = 2.5` degrees. A real orogen is two wedges of different taper meeting at a crest,
not one symmetric bump, and because the widths are the exact inverse of the angles the profile
needs **one** parameter rather than two.

| asymmetry | peak | grade | summits | measured flank ratio |
| --- | --- | --- | --- | --- |
| 1.00 (canonical) | 4,540.5 m | 7.030% | **2** | 1.08 |
| 1.25 | 4,536.1 | 7.910% | 5 | 1.30 |
| **1.67 (published)** | 4,532.2 | 9.898% | 7 | **1.50** |
| **2.00** | 4,531.1 | 11.363% | 10 | **1.64** |
| 2.50 | 4,529.5 | 13.925% | 12 | 2.09 |
| 3.00 | 4,527.5 | 16.538% | 16 | 2.56 |

Monotone in every column over six settings, and it costs 8 m of peak across the whole sweep and
no reach at all, because only the overriding flank is divided -- so no setting of this field can
push a profile past `MAX_TECTONIC_RANGE_M`. **On the profile alone the ratio is exactly the
published 1.67**, proved by bisecting the shipped `asymmetric_bump` for its half-height crossing
rather than by rearranging algebra. **On a planet the same setting reads 1.50**, and 2.00 is
what reads 1.64. A profile is not a planet, and the preset takes the measured column.

**2. Stacked sutures -- `suture_count` and `suture_spread_m`. SHIPPED, with a measured hazard.**

Over 70% of the North American Cordillera is accreted terranes; the Himalaya carries at least
two sutures of different ages. A range is not one crest.

| count x spread | peak | across-range crests | summits | reach | vs the 420 km gate |
| --- | --- | --- | --- | --- | --- |
| 1 (canonical) | 4,540.5 m | **1** | 2 | 100 km | inside |
| 2 x 60 km | 5,238.7 | 1 | 0 | 181 km | inside |
| **2 x 100 km** | **4,540.5** | **2** | 2 | 235 km | inside |
| **2 x 150 km** | **4,540.5** | **2** | 2 | 302.5 km | inside |
| 3 x 100 km | 4,509.0 | **2** | 1 | 370 km | inside |
| 4 x 60 km | **7,457.6** | 1 | 1 | 343 km | inside |
| 4 x 150 km | 5,137.1 | 1 | 1 | **707.5 km** | **past** |

**Two sutures 100-150 km apart doubles the across-range crest count with the peak unmoved.**
Both sides of that band are measured failures. Tight spreads *inflate* the peak, because
overlapping bumps add and the sum is deliberately un-normalised: `4 x 60 km` reads **7,457.6 m**
against the envelope's 4,540.5, a 64% overshoot that would make the height slider mean something
different at every count. Wide ones drive the profile past the range gate, where it is
**truncated rather than faded**: `4 x 150 km` reaches 707.5 km against a 420 km gate and measures
**41.345% of grade and 827.2 m of relief over 2 km**. `collision_reach_m()` exists for that call
site, and is tested against where the profile actually stops rather than against the formula that
produced it.

**3. Ridged multifractal x segmentation -- `structure_depth` and `structure_wavelength_m`.
SHIPPED, and the biggest single win.**

| depth x wavelength | peak | grade | summits | crest sinuosity |
| --- | --- | --- | --- | --- |
| 0.0 (canonical, inert) | 4,540.5 m | 7.030% | 2 | 1.0261 |
| 0.3 x 40 km | 4,106.4 | 8.638% | 4 | 1.3940 |
| 0.5 x 40 km | 3,817.1 | 12.706% | 7 | 1.6169 |
| 0.5 x 80 km | 3,644.6 | 7.052% | 7 | 1.3805 |
| **0.7 x 40 km** | 3,527.7 | **17.783%** | **12** | 1.5324 |
| **0.7 x 80 km** | 3,323.8 | 8.420% | 6 | 1.6481 |
| 0.9 x 40 km | 3,238.3 | 22.859% | **15** | 2.0023 |
| 0.7 x 250 km | 2,955.8 | 4.740% | 1 | 1.1011 |
| 0.9 x 250 km | 2,519.4 | 4.419% | 1 | 1.1644 |

**Wavelength decides whether the knob works at all: 40-80 km bites, 120-250 km does nothing**
(summit counts fall back to 0-4 at every depth -- a control that is switched on and visibly
idle). And the cost is real and not independent: `structure_at` returns a multiplier of at most
1, so **depth can only ever lower the peak** -- 4,540.5 m to 3,323.8 m at depth 0.7, a loss of
22%. `RIDGE_FEEDBACK = 2.0` was swept on our own field rather than transcribed: 40,000 samples
of a 200x200 lattice, crest fraction 0.1692 -> 0.4331 from feedback 0.5 to 2.0 and only 0.4929
by 4.0, with sharpness flat from 2.0 up. **2.0 is the knee.**

**4. The crest warp -- `margin_warp_m` and `margin_warp_wavelength_m`. REJECTED, DELETED, AND
THEN REVIVED ONE TASK LATER BECAUSE THE REJECTION WAS MEASURED WITH THE WRONG INSTRUMENT.**
That is the most valuable thing in this section, and it has its own headings below.

### The straight line, and why it was geometry rather than tuning

With the structure field on, the owner looked at a fresh world and said: *"how do we make them
more random? they look like they were drawn with a straight line tool."*

**They are drawn with a straight line tool, and the line is in `plates.rs`.** `margin_at`
computes a margin's distance as `asin(|point . bisector_normal|) * radius_m`, and a bisector
normal is the normal of a plane **through the origin** -- so the set of points at zero distance
is that plane's intersection with the sphere, which is a **great circle**. Every margin in this
engine is a perfect great-circle arc, and every belt built on one is dead straight by
construction. No amount of amplitude, width, asymmetry, suture stacking or ridge noise can bend
it, because none of them touches the distance field: they all decorate a profile evaluated
*across* a line that is exactly straight.

The fix moves the belt off the line. `margin_warp_m_at` samples an fbm field at a point on the
margin's **own** great circle -- `along = normalise(p - (p . n) n)`, constant across the belt and
varying only along it -- and subtracts the result from the across-margin distance the collision
profile is asked about. One expression, one belt, translated sideways. Three octaves at gain 0.5
and lacunarity 2.0, carrying 4/7, 2/7 and 1/7 of the amplitude.

**Crest AND envelope sinuosity, before and after, on the bare 6,000 m / 100 km envelope** -- a
great circle with nothing else happening on it. Every row is anchored on the *un-warped*
configuration's peak and axis, so before and after describe the same ground:

| | elevation at the anchor | grade | **crest sin** | **max dev (of belt)** | **envelope sin** |
| --- | --- | --- | --- | --- | --- |
| **no warp -- calibration** | 4,540.5 m | 7.030% | **1.0261** | **3.6 km (0.010)** | **1.0172 / 1.0139** |
| + 20 km | 4,482.0 | 7.031% | 1.0293 | 5.2 (0.015) | 1.0163 / 1.0128 |
| + 40 km | 4,362.7 | 6.875% | 1.0243 | 6.8 (0.020) | 1.0175 / 1.0130 |
| **+ 80 km (shipped)** | 3,971.9 | 6.318% | **1.0470** | **15.5 (0.044)** | **1.0250 / 1.0223** |
| + 120 km | 3,428.1 | 5.852% | **1.0648** | **34.1 (0.097)** | **1.0395 / 1.0420** |

Three things this table says that an argument could not:

- **The belt moved, not only the crest inside it.** Crest 1.0261 -> 1.0648 and envelope
  1.0172/1.0139 -> 1.0395/1.0420: they rise together and by comparable fractions. The envelope
  did not stay at 1.000 while the crest wandered.
- **The elevation at a fixed anchor falls from 4,540.5 m to 3,428.1 m.** That is the belt leaving
  the ground it used to stand on -- a translation, not a roughening.
- **The grade goes DOWN, not up.** 7.030% -> 5.852%. The warp moves a belt; it does not steepen
  one.

**A bend longer than the belt is a tilt.** At an 80 km amplitude the crest sinuosity is 1.0470 at
a 300 km wavelength, 1.0337 at 600 km and 1.0312 at 900 km on a 350 km belt: the endpoint chord
absorbs the displacement. That is the same dead band `structure_wavelength_m` has above 120 km,
and it is why the wavelength ceiling on the WASM channel is documented as a domain statement
rather than as a useful setting.

### The preset, and what it actually delivers

`TectonicParams::ranges()`, read from source:

| field | value | the ground for it |
| --- | --- | --- |
| `continent_collision_m` | 6,000 m | the top of the calibrated height travel |
| `continent_collision_width_m` | 100 km | with the amplitude, the 7.030% pair -- the Himalayan surface slope, measured |
| `collision_asymmetry` | 2.0 | Naylor & Sinclair's 1.67 **from the measured column**: 1.67 reads 1.50 on the ground, 2.00 reads 1.64 |
| `suture_count` | 2 | the sutures table's one useful setting: crests 1 -> 2 with the peak unmoved |
| `suture_spread_m` | 100 km | the same row; 235 km of reach, the largest margin under the 420 km gate in the useful band |
| `structure_depth` | 0.7 | the biggest single effect; 0.9 buys three more summits for another 8% of the peak |
| `structure_wavelength_m` | 80 km | **the one place the preset does not take the biggest number available**: 40 km measures 17.783% of grade, which no published surface slope supports, and 120-250 km does nothing at any depth |
| `margin_warp_m` | 80 km | 0.124 of belt length against 0.204 at 120 km, for 9.293% of grade against 10.711% and 315 km of reach against 355 |
| `margin_warp_wavelength_m` | 300 km | roughly the belt's own length: an orocline-scale bend plus two scales of kink |

**Requested 6,000 m. DELIVERED 3,034.6 m**, at a 9.293% grade, with **10 summits and 2
across-range crests**, a crest sinuosity of **1.3204** (44.6 km of lateral deviation, 0.124 of
its belt) and a reach of **315 km** of the 420 km gate. That gap matters because the owner reads
*6,000 m* on a panel slider and gets three kilometres of mountain: `structure_depth` costs 22%
and the warp costs a further 9%, and both are multiplicative on the amplitude the slider names.
The **delivered** peak is what has to stay inside the calibrated 1,500-6,000 m band, and it does.

**A ledger figure that disagrees with the run, named rather than carried forward.** The slice
ledger and three task briefs quote the preset as delivering **3,323.8 m**. That was true before
the warp shipped and is not true now: 3,323.8 m is `ranges()` **with the warp switched off**,
which this survey still prints as its own row, and the shipped preset delivers **3,034.6 m**.
Every "preset with ..." row in the older tables carries the same offset, because they were
measured on a preset that had no warp in it.

**Three of the preset's columns did not compose, and that is why it is measured as a preset.**
The techniques were each measured alone on the steep envelope; stacked, the flank ratio reads
1.38 where the asymmetry sweep read 1.64 at the same setting, and the grade reads 9.293% where
the 80 km wavelength row read 8.420%. What the sutures still buy is measured rather than assumed:
dropping to one suture takes the summit count **10 -> 6**. A preset composed from three tables on
paper would have been three columns wrong.

### THE THINGS THIS SLICE GOT WRONG. THEY ARE WORTH MORE THAN WHAT IT GOT RIGHT

**1. A technique was rejected on four metrics that could not see it.** The crest warp was built,
swept, measured displacing the crest **49.2 -> 78.8 km** de-trended, and **deleted** -- because
it moved no summit, no across-range crest and no flank ratio, and another technique produced a
similar displacement as a side effect. Every one of those four metrics measures structure
**across** a range. Straightness is a property **along** one, and nothing in the set looked
along. One task later the owner reported the exact defect the deleted technique existed to fix,
and it was rebuilt and shipped. **The gap was in the specification, not in the implementation**:
the implementer noticed its probes were blind twice over and added two measurements before ruling
on anything, and still ruled correctly against a brief whose metric set had a hole in it.

**2. A sinuosity metric scored a perfect great circle at 2.13 before it was rebuilt.** Its first
version re-found the crest independently at every station, as the highest sample in a +/-200 km
scan, so on a preset world the global maximum jumped tens of kilometres between adjacent stations
and a path length added every jump; and it walked off the end of the belt, where three stations
turned 1.03 into 1.34. A measure that calls a straight line bendy would have validated anything.
Rebuilt to **follow** the crest (each station within 40 km of the previous) and to **stop where
the belt does**, it reads **1.0261** on the bare blade.

**And it is still not trustworthy everywhere, which this write-up measured and the task reports
did not.** On the **canonical** world -- a smooth symmetric swell on a great circle, the
straightest thing this engine can draw -- the shipped metric reads **1.5250, with 66.4 km of
deviation over a 310 km belt.** The canonical crest barely clears the 1,000 m line the walk stops
at, so the tracker wanders on almost-flat ground. **The metric is calibrated on a belt and is
only meaningful where there is one**: every sinuosity figure in this section is on the 6,000 m
envelope or on the preset for that reason, and a canonical-world sinuosity means nothing.

**3. The warp's first signed-side derivation cut a 913 m cliff down every bisector.** The side was
taken from the ordered plate pair, which is stable across the margin it belongs to and **not**
across a third plate's: crossing from plate A into plate C replaces the whole `(A, *)` margin
set, and the index comparison can come out the other way and flip the displacement from `+w` to
`-w` in one step. Measured at **913.52 m over a single 100 m step**, against 11.58 m for the same
configuration unwarped. **Every other column looked plausible and all the sinuosity numbers went
up.** The first hypothesis -- shear -- was wrong, and an octave sweep refuted it in one run. The
fix takes the sign from the axis `from_margin` already has, so the profile is `P(x - w)` on both
sides of one margin. `seam_probe` is now a permanent survey row, because **nothing else in this
survey looks for a discontinuity**; today it reads 9.79 m for the shipped preset against 8.25 m
with the warp off, and 7.23 m for the bare steep envelope.

**4. `gates.yml` was found already wrong at a commit before this slice touched it**, by 2 on
every row, because nothing had re-derived the pins since the commit that moved them. The count
gate was red and green at the same time. Re-derive, never trust -- and every task since has
re-derived the *baseline* before its first edit as well as the result after its last.

**5. An agent died mid-task without reporting**, leaving an uncommitted, non-compiling tree. It
was found by reconciling live children against the roster rather than by waiting for a
notification that was never coming.

**6. `cargo build ... | tail` reported exit 0 on a build with four errors**, because the exit
status belonged to `tail`. This project's rule is "verify by exit status, never by grepping test
output", and **a pipe is how that rule gets violated while appearing to be followed.**

**7. A bit-identity test that could not fail was believed for a whole task.** `Tectonics::new`
resolves `None` through `unwrap_or_else(TectonicParams::canonical)`, so the `None` arm and the
`Some(canonical())` arm call the same function and agree no matter what the uplift path ignores;
mutating `canonical()` cannot separate them. What proves the fields are read is a different test
entirely -- one-ULP perturbation fixtures sweeping `from_margin` out to the range gate -- and the
same fixtures prove that **two of the nine fields are not read at all.** See the open items.

**8. The tectonic channel shipped to the owner's panel with no native-against-WASM parity
coverage, and three tasks in a row said so without closing it.** A preset the owner presses
crossed a boundary a 71,596-value corpus did not watch, for three commits. It is closed now --
see the parity section above, and `parity/README.md` -- and the shape of the failure is worth
more than the fix: **each of the three reports named the gap accurately, sized it correctly, and
declined it for a good local reason** (the corpus size is a pinned count gate, and moving a gate
is not a task's business unless the task owns the gate). Nobody was wrong; the work simply had no
owner until a task was written whose subject it was.

### What still does not look right, and what is still open

- **`island_arc_m` and `island_arc_width_m` have no evidence the uplift path reads them.** The
  one-ULP fixtures prove the other seven fields move the answer and **assert the arc pair's
  blindness explicitly** rather than dropping the case: the arc term is multiplied by an oceanic
  weight that no synthetic fixture, and no run of the survey, has produced -- the peak the survey
  tracks is on a continental collision margin every time. **No slider binds to either**, and the
  panel declares the omission, which is why this is a recorded gap and not a defect.
- **The warp cannot be turned by the owner.** It is on the preset and in the query string and has
  no widget, because it is jointly constrained with the steepness slider through
  `collision_reach_m()`: at the panel's own widest steepness (canonical's 400 km) the 420 km gate
  leaves 20 km of room, so a slider anchored there would offer one live position and then refuse
  everything after it. A travel that depends on another slider's position is a real feature and
  its own task.
- **The delivered grade, 9.293%, is above the 3-8% band the panel's own note quotes**, and the
  note still quotes it. They are different quantities -- a wedge's *mean surface taper* against
  the steepest single 2 km step on a flank the structure field has deliberately carved into ridge
  and valley -- and the 2 km step on a carved flank must be the larger. Stated rather than tuned
  away; the alternative at a 40 km wavelength reads 12.233% on the shipped preset.
- **The preset's envelope sinuosity is not a usable number** and no claim rests on it. It reads
  **3.2724 / 1.1213** on the shipped preset and **1.9064 / 1.0773** with the warp off -- and a
  belt straight by construction cannot have a 1.9 envelope, so on a structure-carved flank the
  half-height contour is tracking the structure field rather than the belt's edge. The belt-moved
  claim rests on the bare-envelope rows, where the base reads 1.0172.
- **A snowline clamp is a real coupling and a later slice.** Egholm et al. (2009), *Nature* 460,
  read in the source text: *"most summit elevations are confined to altitudes <1,500 m above the
  local snowline"*, and range height *"mainly reflect[s] variations in local climate rather than
  tectonic forces"*. Convergence sets width and uplift rate; **climate sets height.** Nothing here
  implements it, and nothing here should be read as having tried.
- **`MARGIN_WARP_OCTAVES`' doc comment names the Himalayan arc and the Bolivian orocline as the
  long octave's motivation.** Neither is in the verified column of this project's literature note
  -- both came back only as search paraphrases -- and **no number in this section was tuned to
  either.** The octave schedule was chosen by sweeping 1, 2 and 3 octaves while diagnosing the
  seam cliff. Read that comment as an intuition pump, not as a citation.
- **`tectonics::continental` is dead code in a `--features wasm` build**, and the compiler says so
  on every build of this crate. Pre-existing, untouched here, named so it is not rediscovered as
  new.

## `continentality.rs`, revisited: a coast term with its own amplitude, a fourth door, and three NaNs that looked like worlds

The photoreal slice's whole purpose was viewer work -- the owner's complaint was that the picture
*"looks like a kindergarden toy"* beside a reference render, and his framing of the job was *"I
think our engine is amazing, now we need a good paint job."* Two things nevertheless landed in the
engine, and the second of them was not on anybody's plan.

**The first is coastline fractality.** The relief slice fixed the *height* field's spectrum and
nobody had asked the same question of the *land/sea* field: ours are smooth because continentality
is low-frequency, and a coastline is the highest-contrast edge in the whole image.

**The second is a family of three NaNs that produced plausible worlds rather than errors**, found
by the export sweep that the first one owed.

### The brief was wrong in both directions, and that is the finding

The plan's original Task 5 warned about one trap and missed another, and the miss was the
dangerous one.

- **The trap it warned about is solved by construction.** "Roughening the land/sea threshold will
  change how much land there is." `Continentality::new` calibrates `shore` and `spread` as
  quantiles of the same fBm **before the struct exists**, so land fraction is held whatever the
  field's roughness. That warning was written about a problem this codebase fixed years of commits
  ago.
- **The risk it missed would have made the task invisible.** Its prescription was "add a fifth
  octave". Adding an octave to a **gain-0.5 normalised sum gives the new term 3.23% of total
  amplitude** -- a sub-pixel wobble, not a fjord. **The task could have passed every check the
  brief named and produced no visible change**, which is the worst possible outcome for a task
  whose entire purpose is visible. There is a second, quieter cost: `CALIBRATION_SAMPLES = 4000`
  already gives only about 1.8 samples per finest-octave wavelength, and a fifth octave takes that
  below one, degrading the land-fraction estimator from quasi-Monte-Carlo to plain Monte-Carlo.

**What shipped instead is a separate term with its own amplitude, windowed by `|above_shore|`:**

```
above_shore(p) = at(p) - shore
               + amplitude * spread * W(|at(p) - shore| / (window_spreads * spread))
                 * fbm(p, ...)                                  [its own lattice salt]
```

Coast-localised roughening **by amplitude rather than by octave count**. It puts the detail where
the eye looks, leaves plate interiors and abyssal plains untouched, and preserves land fraction to
first order for free because the window is symmetric about the shore. **The term is added after
calibration, never inside it**, so `shore()` and `spread()` -- both part of the conformance
surface -- cannot move on any path and the Nyquist degradation never arises.

`W` is the house clamped smoothstep written as three explicit branches: **no `min`, no `max`, no
`.clamp`**, so a NaN falls to the final arm and closes the window, and that arm is asserted rather
than described. `canonical()` sets `amplitude: 0.0` and `above_shore` branches on exactly that
**before** touching the noise -- an early return rather than `+ 0.0`, deliberately, because
`-0.0 + 0.0` is `+0.0` and an exactly-zero offset would flip the sign bit of a point sitting on
the shore, so `Some(canonical())` would not be *bit*-identical to `None`.

**One deviation from the `ReliefParams` / `TectonicParams` pattern, stated plainly.** Both of those
widened the constructor they attach to. This one adds a delegating `Continentality::with_coast` /
`Surface::with_coast` instead, because `Surface::new` has seventy call sites and a mechanical
`, None` at ninety sites inside a commit about coastlines is diff nobody can review. The precedent
is this crate's own C ABI, which ships `wb_world_new` / `_relief` / `_tectonic` as separate doors.
Everything Ruling 1 requires is unchanged and asserted.

### What the coast term delivers, and the control that makes it a measurement

Re-derived for this section by `cargo run --release --bin coastline_survey` (about four minutes,
single-threaded, this machine). Length is a Cauchy-Crofton boundary-edge sum on an equirectangular
grid, quoted **only as a ratio** against the same grid at amplitude 0 so the estimator's raster
bias divides out. Owner's world (seed 562423712, radius 4,500,000 m, 28 plates, land 0.16),
predicate `above_shore > 0`:

| ruler | 100 km | 50 km | 25 km | 12.5 km |
|---|---|---|---|---|
| **fractal / canonical** | 1.358 | 1.472 | **1.591** | **1.639** |
| **smooth control / canonical** | 1.023 | 1.027 | 1.032 | 1.030 |
| canonical length, km | 101,323 | 100,780 | 100,749 | 100,981 |

**The ratio grows as the ruler halves on all three worlds surveyed. That is the fractal
signature** -- and the control row is what makes it a discrimination rather than a sensitivity.
The control moves the coast by the **same amplitude** at a frequency *coarser* than the base
field's own finest octave: the same displacement, no added structure, and it sits flat at
1.02-1.03 across four ruler lengths. The canonical column is flat across an eightfold change of
ruler, which is the other half of the same statement: today's coastline is not fractal and the
estimator says so.

The land fraction is the control that would condemn the technique if it moved. Over a fixed
200,000-point area-uniform Fibonacci spiral, `field_on - field_off` is **-0.017 pp** on the owner's
world, +0.022 pp and +0.146 pp on two Earth-sized ones. The largest is a real second-order residual
-- the window is symmetric about the shore but the *density* of points at a given `above_shore` is
not -- and it is a quarter of the +-0.58 pp the 4,000-sample calibrator itself carries.

The amplitude travel, same world, 25 km grid: visible from about **0.10** (below that the coast
lengthens by under 3% and inlet heads are single digits -- the sub-pixel-wobble failure reproduced
deliberately, so the floor is measured rather than assumed), and fragmenting above about **0.75**,
where the small-island count runs away while the large one does not. `fractal()` takes **0.35**:
1.591x at 25 km and still rising at 12.5 km, **inlet heads 2 -> 175**, four new landmasses over
100,000 km2, and no sign of speckle.

**A topology change the owner has to decide about.** Above a threshold amplitude a strait opens
through this world's supercontinent and the largest landmass's share falls from 87.8% to about
48%. That is not fragmentation -- the count of islands over 100,000 km2 rises 5 -> 9 at the same
step, so what happened is that large pieces separated -- but it is a visible difference, and it is
photographed at five amplitudes rather than explained afterwards. **The shipped preset is above
it, and the slider reaches every amplitude in the table.**

**And the threshold is a property of the ruler.** The 25 km survey puts the drop between 0.10 and
0.15; a 0.5-degree (about 39 km) component pass puts it between 0.15 and 0.20, at the same neck. A
39 km grid bridges an isthmus a 25 km grid has already cut. Neither is wrong. **Any landmass-count
claim in this project must name its grid spacing**, and these two reports disagree by exactly one
amplitude step for that reason.

### The fourth door, and the sweep it owed

`wb_world_new_coast`, `wb_coast_preset` and `wb_coast_check` -- the shape `wb_world_new_relief` and
`wb_world_new_tectonic` already established: a flat f64 record in a documented order, a preset
export so no host transcribes a number, a checker that answers *why* rather than only *that*, and
a constructor that refuses a record entire rather than admitting it with one field adjusted.
**Nothing clamps; every bound is a refusal, and every comparison is written so a NaN fails it.**

The sweep is **two bases x six fields x about 57 values = 748 records**, 392 accepted and 356
refused, both pinned exactly rather than left as a threshold. **Two bases, and the reason is
sharper here than on the tectonic channel:** `canonical()` carries `amplitude = 0.0` and
`above_shore` branches on exactly that before touching the noise, so around canonical four of the
six fields -- including both halves of the loop bound and the frequency product -- would be swept
with the code that reads them switched off.

It found four things.

1. **`octaves` is a per-sample loop bound and `value as u32` saturates.** `1e300` arrives as
   `u32::MAX`; at a measured 1.6e-8 s per octave that is about **68 seconds for one elevation
   sample** and roughly 80 hours for one 65x65 tile. Bounded at 16 in the contract, stated twice --
   before the cast, and again for a caller who built the struct in Rust.
2. **AN ABORT THAT NO PER-FIELD CEILING CAN SEE.** The finest band a record asks for is
   `frequency * lacunarity^(octaves - 1)`. `frequency = 1e6`, `lacunarity = 16` and `octaves = 16`
   are **each individually admissible** and together ask for about 1.15e24; `Noise::at` floors that
   and casts to `i64`, the cast saturates, and the next line computes `ix + 1`, which overflows --
   an abort across a nounwind `extern "C"`. **With the product check removed, the cross-product
   test aborts and the one-field-at-a-time sweep stays green.** That is why it is a separate test
   and not a comment. **Add cross products to every future export sweep.**
3. **A NaN band whose edge moves with another field.** Above `gain = 1` the fBm amplitude overflows
   to `+inf`, `loudest` with it, and `2*total/loudest` is `inf/inf`. At four octaves the NaN
   appears near 1e103; at sixteen, near 1e21. A probe at one gain finds nothing that a probe at
   another finds. **A band, not a cliff -- as every hazard this project has found has been.**
4. **And the one that outgrew the channel: a NaN did not surface as a NaN.**

### The silent abyss, and the two siblings behind it

`Continentality::elevation_from_above` read `if above >= 0.0` and then `if depth < 1.0`. **A NaN is
false for both**, so it fell through to `ABYSS_M`: every affected point silently became the
deepest ocean on the planet **and every `is_finite` assertion in the crate stayed green**. A
drowned planet that passes its own health checks is worse than a refusal and worse than a NaN.

**The coastal term was not the only entrant and was not the shipped one.** Three reach that
function, and two of them need no opt-in block at all: a non-finite `latitude_deg` or
`longitude_deg` through the C ABI (which takes two bare `f64` and validates neither --
`from_latlon` turns a NaN *or an infinity* into an all-NaN vector), a non-finite vector component
through the Python bindings, and the coastal term. All three were measured returning about
-4,600 m.

The contract chosen is **propagate**, and the reasoning is worth keeping because two more sites
inherited it:

- **Refusing at the boundary cannot cover the reach.** The coastal entrant was already refused and
  was the *least* reachable of the three; the other two arrive as bare scalars on exports whose
  whole design is one `f64` in, one `f64` out.
- **Loud-as-a-panic is unavailable.** `extern "C"` is nounwind here: a panic reached through
  `wb_elevation_m` is an abort, and this project has found three aborts and a ~2,600-second hang
  behind that boundary already. **Trading a wrong number for a dead process is not an improvement.**
- **Propagation is the loud option this ABI already speaks**, and it is what makes the finiteness
  assertions the crate already has actually load-bearing rather than only looking it.

The guard is an explicit `if above.is_nan() { return f64::NAN; }` placed **first**. It is
`plates.rs::margin_at`'s house form used the other way round, and the source says why: there a NaN
is floored on purpose because the value is a **weight**, and a floored weight is visible in the
product it enters; here the value is a **metre**, and a floored metre is indistinguishable from a
real one.

**Two siblings were reported by that work and closed next.**

- **A NaN `land_fraction` made the world entirely land.** `calibrate` picked sea level with
  `values[((1.0 - land_fraction) * last) as usize]`, and **`as usize` saturates a NaN to 0** --
  so `shore` became the sorted sample's **global minimum** and every point on the planet stood
  above it. Measured at seed 12345: shore -0.6889 against a canonical 0.0956, 2000 of 2000 spiral
  points land against 578. **And the world it produced is BIT-IDENTICAL to the world
  `land_fraction = 1.0` legitimately produces.** That is the whole difficulty: not a wrong-looking
  number but a real world, from a real input, arrived at by accident. (`land_fraction = 1.5` gives
  the same answer and is left alone -- it is the monotone continuation of the curve, and it is
  documented at the cast.)
- **A lattice coordinate the index could not name aborted, and in release it was worse.**
  `Noise::at` floors, casts to `i64` -- which saturates -- and asks for `ix + 1`. Debug:
  `attempt to add with overflow`. Behind `extern "C"`: an abort. **Release: the overflow wraps to
  `i64::MIN` and the function returns an ordinary-looking height from a cell chosen by
  wrap-around** -- the plausible-value failure again, on the same line as the loud one. **The
  single-infinite-field entrant and the coastal cross product are the SAME site**, reproduced here
  panicking at the identical line, and one guard closes both. The new test **asserts the
  one-field blindness** as well: each of the three fields alone at its own ceiling still returns a
  finite answer, because the finest bands those ask for are twelve or more orders below saturation.

Both take the same **propagate** contract, and unlike the abyss guard **neither is a divergence
from the Python oracle**: `int()` raises on a NaN `land_fraction` and on an out-of-range
coordinate, so there is no CPython answer these contradict. They are the closest a nounwind
boundary can get to the oracle's own refusal.

### The sweep for the same shape elsewhere: one claim confirmed, one REFUTED

The abyss note's claim was: `elevation_from_above` was the only site where a NaN's fall-through
value is a *metre*; everywhere else it falls to **a weight in [0, 1]**, silent but oracle-required.

- **Confirmed: no other production site turns a NaN into a finite height.** The three that deal in
  metres all propagate -- two fall through to the NaN itself, and `erosion.rs`'s largest-change
  test is explicitly NaN-aware.
- **REFUTED, and the refutation is the transferable part.** "A weight in [0, 1]" is not what the
  rest fall to. **Two sites fall to ANGLES** -- pi/2 and pi radians, both documented CPython
  `min` transcriptions -- and one falls to a **categorical flag** (unreachable: a non-finite height
  is refused seventeen lines earlier). The conclusion survives, because every one of these is
  oracle-required or already refused. **But a later sweep that reused "a weight in [0,1]" as its
  search pattern would have missed three sites**, and that is exactly how a characterisation
  becomes a defect.
- **And the axis the first sweep did not have at all**: defect A is a *cast*, not a comparison.
  Every float-to-int cast in the crate was enumerated separately. **In production code there were
  exactly three, and all three are now closed** -- the shore index, the three lattice indices, and
  one whose operand is a literal.

### Ruling 1, and the pins -- re-derived on this host, not read from `gates.yml`

**Host:** Windows 11 (10.0.26200), `rustc 1.98.0 x86_64-pc-windows-msvc`, Python 3.11 in the
repo's own `.venv`, node v22.17.0. **Method:** `cargo test -p worldbuilder-engine <cfg>
--no-fail-fast` (exit status), then `-- --list` and `-- --list --ignored` through
`.github/scripts/assert_counts.py cargo-list`; **listed minus ignored**. **Every exit status read
from `$?` directly, never through a pipe.**

| configuration | listed | ignored | **run** |
|---|---|---|---|
| `--no-default-features` | 686 | 5 | **681** |
| default | 686 | 5 | **681** |
| `--features python` | 688 | 5 | **683** |
| `--features wasm` | 792 | 5 | **787** |
| `--features python,wasm` | 794 | 5 | **789** |

**2026-09-10, water 1a:** re-derived again, the same way, after the hydro-bake work
(`src/hydrology`'s bake and its bake/measure/copy/free WASM exports) landed on this branch --
632/632/634/735/737 -> 681/681/683/787/789, 5 ignored, unchanged, over the same 15 binaries. The
movement is +49 uniformly on the three non-wasm rows and +52 on the two wasm rows: the shape this
table has always had (uniform `src/` tests plus wasm-only `tests/wasm_exports.rs` tests), not a new
one. Task 11 itself adds no test -- it adds the `H` parity record below and re-derives this table
against a suite that had already moved out from under it. Re-derived through `assert_counts.py
cargo-list` AFTER the last source edit.

**2026-09-10, water 1a Task 12b (the calibration rulings):** 681/681/683/787/789 ->
684/684/686/790/792, 5 ignored, unchanged. `assert_counts.py cargo-list` reports 16 test
binaries now (up from 15 at Task 11); this task added no `[[bin]]`, so that binary was already
in the tree, just not yet re-counted here. **+3 uniformly on every row**:
`a_hollow_larger_than_the_caspian_drains` (Ruling 12b-5, `hollows.rs`),
`effective_thresholds_rise_to_the_graph_resolution` and
`only_notches_on_rivers_or_outlets_are_recorded` (Rulings 12b-1/12b-2, `mod.rs`) -- all three
in `src/`, so every configuration sees them alike; `tests/wasm_exports.rs` gained no new test,
only an in-place move of its one hydro pin (the header-length floor, 17 -> 20 words at the new
SCHEMA 2.0). Re-derived through `assert_counts.py cargo-list` AFTER the last source edit.

**2026-09-10, Task 12b fix round 1 (the threshold test must see the floor bind):**
684/684/686/790/792 -> 685/685/687/791/793, 5 ignored, unchanged. **+1 uniformly on every row**:
`the_node_floor_binds_on_a_coarse_graph` (`mod.rs`, `src/`), added because
`effective_thresholds_rise_to_the_graph_resolution` runs on the bake test world's overridden
thresholds (3.0e10/3.0e11/3.0e12); at the time this was believed to sit above
`min_stream_nodes * median`, so the node-based branch would never bind and the test would
still pass with the floor removed. That belief was itself wrong -- the floor binds on this
world too (about 4.24e11 against the 3.0e10 asked for), corrected in water 1b-2 Task 1 -- but
the new test added here stands on its own regardless: it uses `HydroParams::earth_like(12_000)`'s
stock thresholds (2.5e8/2.5e9/1.0e11), further below that graph's own median land-node area, so
the floor must bind; RED was shown by temporarily
replacing the effective stream threshold with `params.stream_flow_m2` (unconditionally), which
failed the new assertions (`250000000.0 != 424166660158.80225`), then restoring the floor logic
for GREEN. No wasm rebuild was needed -- this is test-only code. Re-derived per configuration
through `assert_counts.py cargo-list` AFTER the last source edit.

**2026-09-10, water 1a final review fix wave:** 685/685/687/791/793 -> **692/692/694/798/800**,
5 ignored, unchanged, over the same 16 binaries (`listed` 697/697/699/803/805). **+7 uniformly on
every row**, all in `src/`: the drainage-cycle repro, the pocket re-judging test and the
drainage-check mutation guard (`flow.rs`), the shore-lake-on-an-outlet-path test (`routing.rs`),
and the real-world full-bake drainage test, the closed-lake outlet test and the river-mouth bed
test (`mod.rs`). The 1.3M node-ceiling check went into an existing `tests/wasm_exports.rs` test in
place, so the two wasm rows move by the same +7 and no more. Re-derived per configuration through
`assert_counts.py cargo-list` AFTER the last source edit; all five printed `count OK`.

All five exited 0 and `assert_counts.py` reported `count OK` at all five, over **15 test
binaries**. The movement decomposes cleanly and the shape is the check: the coast term was **+9 on
every row** (it widened no C ABI), the coast channel **+18 on the two WASM rows only**
(`tests/wasm_exports.rs` is `#![cfg(feature = "wasm")]` in its entirety), the three NaN guards
**+1 then +2 on every row**, the climate slice's temperature task **+18 on every row**, its
moisture march **+21 on every row**, the merging slice **+3 on every row and +4 on the two WASM
rows**, the local-reference slice **+2 on every row**, the climate slice's band task **+13 on
every row**, and its snow-line task **+4 on every row** -- each time because the tests live in
`src/` and compile unconditionally, and each time because no export was added. The one exception
is the climate slice's export task, **+7 on the two WASM rows alone**, which is the complementary
shape and is the check that its tests all landed in `tests/wasm_exports.rs`.

**The binary count moved from 13 to 14 at the temperature task and from 14 to 15 at the merging
slice's `src/bin/gully_merging_survey.rs`.** A `[[bin]]` carries zero tests, so it is invisible to
`--expect-passed` and visible only here; both the moisture march and the band task **extended
`src/bin/climate_survey.rs` rather than adding another survey**, which is why the climate slice
accounts for one binary across three tasks.

**Conformance, re-derived:** `WORLDBUILDER_REQUIRE_ENGINE=1 pytest tests/` -- **398 passed, exit
0** -- and `pytest tests/test_conformance.py` -- **157 passed, exit 0** -- against the extension
built by `maturin develop --release --features python`. `worldbuilder/` was not modified.

**One honest note about that 398, because it is not purely a property of the algorithm.** On a run
taken while this machine was also compiling and driving a browser, the suite came back **397
passed, 1 failed**: `test_performance.py`'s `test_the_table_that_decides_everything` asserts a
wall-clock ceiling of 260 microseconds a sample and measured 487.8. Re-run on a quiet host it
passes with the rest. **A count is a property of the algorithm; a millisecond is a property of the
moment**, and one of the 398 is a millisecond wearing a count's clothes.

**Parity, re-derived, and this is where Ruling 1 is actually proved:**

| | compared | divergent |
|---|---|---|
| `parity` | **148,707** | **0** |
| `--mutate seed` | 148,707 | 142,630 |
| `--mutate erosion-k` | 148,707 | 216 |
| `--mutate water-pond` | 148,707 | 60 |
| `--mutate tectonic-warp` | 148,707 | 6,186 |
| `--mutate coast-amplitude` | 148,707 | 13,128 |
| `--mutate gully-steer` | 148,707 | 3,752 |
| `--mutate climate-samples` | 148,707 | 648 |

**2026-09-10, water 1a:** the `H plain` record adds the hydrology bake (`wb_hydro_bake` /
`wb_hydro_len` / `wb_hydro_copy` / `wb_hydro_free`, `total_nodes = 12,000`) to the corpus:
127,659 -> 148,707 compared (+21,048 = 2 + a 21,046-word record), 0 divergent. `--mutate seed`
is the only control the record moves under -- 122,208 -> 142,630, +20,422 of the record's own
21,048 words -- because the bake runs on the `plain` world and that world's own `world` line
already rebuilds under the seed mutation; every other control (erosion-k, water-pond,
tectonic-warp, coast-amplitude, gully-steer, climate-samples) is byte-for-byte unmoved, which is
what says the hydro channel reaches nothing those controls perturb.

All seven exited 0 and **every control matched its recorded figure exactly**, which is the statement
that nothing in this slice moved a crossing value. `node scripts/build-wasm.mjs check` reports the
committed artifact matches its manifest and the source that is here now.

**2026-09-10, water 1a Task 12b (the calibration rulings):**

| | compared | divergent |
|---|---|---|
| `parity` | **128,347** | **0** |
| `--mutate seed` | 128,347 | 122,825 |
| `--mutate erosion-k` | 128,347 | 216 |
| `--mutate water-pond` | 128,347 | 60 |
| `--mutate tectonic-warp` | 128,347 | 6,186 |
| `--mutate coast-amplitude` | 128,347 | 13,128 |
| `--mutate gully-steer` | 128,347 | 3,752 |
| `--mutate climate-samples` | 128,347 | 648 |

The `H plain` record's header moved from 17 to 20 words at SCHEMA 2.0 (Ruling 12b-1: the three
effective thresholds), but the corpus **shrinks**, 148,707 -> 128,347 (-20,360), because Ruling
12b-2 (record only the notches on a recorded river or an outlet cut) drops the test world's
graph-scale notches from 21,046 to 686 recorded words -- the params buffer is still the same
12-word `HYDRO_PARAMS` fixture, unaffected by `min_stream_nodes`/`keep_max_area_m2`, which are
not wasm params in 1a. `--mutate seed` is still the only control that moves the hydro group
(617 of the group's 688 words, down from 20,422 of 21,048, the same fraction of a much smaller
group); every other control is byte-for-byte unmoved at its Task 11 figure, which is what says
Ruling 12b-1/12b-2's filtering logic is confined to the channel it was written in. All eight
`node parity.mjs ... [--mutate ...]` runs were re-verified against `assert_counts.py parity`
with the exact `--expect-compared`/`--expect-divergent` pairs now committed in `gates.yml`, and
all eight printed `count OK`. `node scripts/build-wasm.mjs check` reports the rebuilt artifact
(326,137 bytes) matches its manifest and the source that is here now.

**2026-09-10, water 1a final review fix wave:**

| | compared | divergent |
|---|---|---|
| `parity` | **128,347** | **0** |
| `--mutate seed` | 128,347 | **122,830** (was 122,825) |
| `--mutate erosion-k` | 128,347 | 216 |
| `--mutate water-pond` | 128,347 | 60 |
| `--mutate tectonic-warp` | 128,347 | 6,186 |
| `--mutate coast-amplitude` | 128,347 | 13,128 |
| `--mutate gully-steer` | 128,347 | 3,752 |
| `--mutate climate-samples` | 128,347 | 648 |

The wasm was rebuilt for the drainage fix, the outlet-reach and mouth-bed rules and the 1.3M
ceiling (329,197 bytes). The `H plain` record is still 686 words; against the pre-fix dump exactly
8 of them moved, the beds of its 8 ocean mouths (each from the seabed under it to the datum), and
every non-`H` line of the dump is byte-identical. The seed control moves by 5, all in `hydro/plain`
(617 -> 622 of 688). **Caution:** on the seed-moved world the bake is 594 words, not 686, and the
harness still compares 686, so the last 92 are read past the copied buffer. The review's length
check (I6) is what closes that. It is not in this change; see the fix-wave report. The other six
controls are byte-for-byte unmoved. All eight runs were checked against `assert_counts.py parity`
with the pairs committed in `gates.yml`; all eight printed `count OK`.

**The coast control was written in the same commit as the export**, deliberately: the tectonic
channel sat unwatched by parity for three tasks, and each of the three reports named the gap
accurately, sized it correctly, and declined it for a good local reason. Nobody was wrong; the work
simply had no owner until a task was written whose subject it was.

**63.7% of a uniform global scatter moving under the coast control is the right shape, and 2.6%
would be wrong** -- a tectonic belt is a line on the planet, while the coastal window covers the
whole shelf. The first corpus cut was refused by the dump's own both-ends-refused guard, because a
2-degree box on the largest mover is entirely *inside* the coastal band and moved 100%.

**2026-09-10, water 1a I6 (the ruling implemented, closing the final review's one blocked item):**

| | compared | divergent |
|---|---|---|
| `parity` | **136,086** | **0** |
| `--mutate seed` | 136,086 | **130,366** (was 122,830) |
| `--mutate erosion-k` | 136,086 | 216 |
| `--mutate water-pond` | 136,086 | 60 |
| `--mutate tectonic-warp` | 136,086 | **13,590** (was 6,186) |
| `--mutate coast-amplitude` | 136,086 | 13,128 |
| `--mutate gully-steer` | 136,086 | 3,752 |
| `--mutate climate-samples` | 136,086 | 648 |

A second `H` record, `H ranges`, is added on the tectonic `ranges` world (as `parity_dump.rs`
already builds it for the tectonic control) with `earth_like` thresholds at 60,000 nodes, so the
12b-1 node-area floor binds, and one forced outlet: an unforced probe bake finds the world's first
enclosed body and its anchor (measured: 74.42 deg N, 38.72 deg E), and the forced bake on that
anchor gives a 7,737-word record -- both figures exactly as the ruling predicted. Corpus:
128,347 -> 136,086 (+7,739 = 2 + 7,737), 0 divergent on the plain run.

`parity.mjs` case `H` now implements the ruling's rule (a): a plain run throws if `wb_hydro_len`
disagrees with the recorded length, before reading a single word; a control run instead tallies
the length match (as before) and, for any recorded word at an index the fresh bake's buffer does
not reach, counts it divergent WITHOUT reading past the copy. `compared` for a hydro group is
still `2 + len` either way. This retires the seed control's old caution: previously the last 92 of
`hydro/plain`'s 686 words were read past a 594-word copy (whatever the wasm heap held there); now
they are counted divergent by construction, for the reason the ruling gives rather than by reading
stack garbage that happened to differ.

`--mutate seed` moves `hydro/ranges` by 7,536 of its 7,739 words (the `ranges` world rebuilds with
`world_seed + 1`, same as `plain` does) and leaves `hydro/plain` at its already-pinned 622 of 688 --
same number as before, now for the sound reason above. `--mutate tectonic-warp` is the other
control that reaches `hydro/ranges`: turning `margin_warp_m` off changes the terrain under the
forced-outlet bake, and the native side predicts the resulting divergence the same way the other
five `TCTL` fields are predicted -- baking the warp-0 world natively with the identical
forced-outlet params and comparing under rule (a). Measured: 7,404 of 7,739. `parity.mjs`'s
tectonic-control check now holds `hydro/ranges` to that count exactly, the same discipline it
already held the other five groups to. The other five controls (erosion-k, water-pond,
coast-amplitude, gully-steer, climate-samples) leave both hydro groups at 0, which each control's
own per-group check (where one exists) now enforces for `hydro/ranges` too.

All eight `node parity.mjs ... [--mutate ...]` runs were re-verified against `assert_counts.py
parity` with the exact `--expect-compared`/`--expect-divergent` pairs now committed in `gates.yml`,
and all eight printed `count OK`. The wasm was rebuilt (329,197 bytes, byte-identical to the
pre-I6 artifact -- only `examples/parity_dump.rs`, a fingerprinted input, moved the source
fingerprint) and `node scripts/build-wasm.mjs check` reports it matches its manifest and the
source that is here now. No `src/` file changed, so the five CI count pins are unmoved.

### What is still open here

- **`CoastParams` is reachable and off.** `canonical()` is amplitude 0 and Ruling 1 keeps it
  there, so the picture does not change until the owner drags the slider.
- **`gradient()` still reads the smooth field.** Deliberate and documented -- the gradient is a
  broad "which way to the sea" direction and `shelf.rs` uses its magnitude as a slope proxy -- but
  it means the shelf's *slope* term follows the old coastline while its *coastal weight* term
  follows the new one. Nothing measured shows a problem; it is an asymmetry a later task should
  either justify or close.
- **`WB_MAX_COAST_AMPLITUDE` and `WB_MAX_COAST_WINDOW_SPREADS` are domain statements, not measured
  edges.** The survey stops at 1.5 and the panel at 0.75. What is asserted is that every accepted
  record in the sweep produces finite elevations at eleven probes.
- **The guards are at the convergence points, not at the entrants.** A host that passes a NaN
  latitude now gets a NaN back instead of a plausible depth, which is the win -- but it still gets
  no *reason*. If the scalar exports ever grow a status channel, validating at the door would be
  strictly better information and the guards would remain as the backstop for the term nobody has
  written yet.
- **`elevation_from_above`'s guard is a deliberate divergence from the Python oracle in a region
  the oracle is never asked about.** `min(1.0, NaN)` is `1.0` in CPython, so
  `worldbuilder/terrain/continentality.py` still drowns a NaN. Nothing tests that today. If
  somebody widens `continentality_corpus()` to hostile vectors the two languages will disagree,
  and the right resolution is to change the Python.
- **An infinite vector component through the Python bindings** reaches the lattice guard now, but
  the bindings still validate nothing themselves.

### Reproducing every figure in this section

```
cargo test -p worldbuilder-engine <cfg> --no-fail-fast            # five configurations
cargo test -p worldbuilder-engine <cfg> -- --list                 # and --list --ignored
python .github/scripts/assert_counts.py cargo-list --all list-all.txt --ignored list-ignored.txt \
    --expect-passed <598|598|600|685|687> --expect-ignored 5

maturin develop --release --features python -m crates/worldbuilder-engine/Cargo.toml
WORLDBUILDER_REQUIRE_ENGINE=1 pytest tests/                       # 398 passed on a quiet host
WORLDBUILDER_REQUIRE_ENGINE=1 pytest tests/test_conformance.py    # 157 passed

node viewer/scripts/build-wasm.mjs check
cargo run --release -p worldbuilder-engine --example parity_dump --features wasm > \
    crates/worldbuilder-engine/parity/native.txt
cd crates/worldbuilder-engine/parity && node parity.mjs native.txt
node parity.mjs native.txt --mutate {seed|erosion-k|water-pond|tectonic-warp|coast-amplitude}

cargo run --release --bin coastline_survey                        # ~4 minutes, chooses nothing
```

**Every one of these was run for this section at `b6862d2`**, and every exit status was read
directly. `native.txt` is regenerated by the dump and is deliberately untracked.

## 2026-09-11, water 1b-1 Task 7: parity and CI pins re-derived after the coarse-fixes slice

The wasm was rebuilt against the coarse-fixes slice's Tasks 1-6 (the `bake.rs` split, the
flood tie-break, the cut-path committed-node deviation, the capped-basin escape chain,
`Body.downstream`, and the record schema 3 layout) -- 335,764 bytes, 31 exports, 0 imports;
artifact-sha256 5649c6219bbcada36f1cf007f74a738a3a1c26285bc33775fa11bfa627d94377,
source-fingerprint a76d6588c1f4c435dd0a1565ffacccf2e530450a09be233898c3072c47bf7f98 (53
inputs). `node scripts/build-wasm.mjs check` reports it matches its manifest and the source
that is here now.

**Engine, re-derived per configuration through `cargo test -p worldbuilder-engine <cfg> --
--list` (and `--ignored`), through `assert_counts.py cargo-list` itself, which reported
`count OK` at all five:**

| configuration | listed | ignored | **run** |
|---|---|---|---|
| `--no-default-features` | 713 | 6 | **707** |
| default | 713 | 6 | **707** |
| `--features python` | 715 | 6 | **709** |
| `--features wasm` | 819 | 6 | **813** |
| `--features python,wasm` | 821 | 6 | **815** |

692/692/694/798/800 with 5 ignored -> **707/707/709/813/815 with 6 ignored**, over 16 test
binaries. `expect_ignored` moves from 5 to 6 for the reason Task 3's own ledger note names:
the every-small-world sweep its Ruling added is `#[ignore]`d (12k nodes per world, run on
request via `cargo test -- --ignored`), the sixth ignored test alongside the pre-existing
five. `cargo test -p worldbuilder-engine --no-fail-fast` and `--features wasm --no-fail-fast`
both ran clean: 707 passed / 0 failed / 6 ignored, and 813 passed / 0 failed / 6 ignored.

**Python, confirmed via `--collect-only -q`:** 565 tests collected, unchanged; 157 of them in
`tests/test_conformance.py`, unchanged. (A full `pytest tests/` run against this host's
`.venv` -- which resolves `worldbuilder.__file__` to the sibling `worldbuilder_by_aetos`
checkout rather than this worktree -- fails one unrelated test,
`test_worldbuilder_importable_outside_the_repo`, on that installation-path assertion; 564 of
565 pass. This is an environment property of running two checkouts against one shared venv,
not a regression in this worktree, and it does not touch the 565/157 collection pins.)

**Viewer, the whole suite:** `npm test` in `viewer/` -- **330 passed, 0 failed** (`node --test`,
17.9 s).

**Parity, re-derived, all eight runs `count OK` against `assert_counts.py parity`:**

| | compared | divergent |
|---|---|---|
| `parity` | **132,472** (was 136,086) | **0** |
| `--mutate seed` | 132,472 | **126,735** (was 130,366) |
| `--mutate erosion-k` | 132,472 | 216 |
| `--mutate water-pond` | 132,472 | 60 |
| `--mutate tectonic-warp` | 132,472 | **10,008** (was 13,590) |
| `--mutate coast-amplitude` | 132,472 | 13,128 |
| `--mutate gully-steer` | 132,472 | 3,752 |
| `--mutate climate-samples` | 132,472 | 648 |

**No H word diverges native against wasm on the plain run.** The corpus shrinks by 3,614
(136,086 -> 132,472): `hydro/plain` re-bakes to 647 words (2 + a 645-word record, down from
688) and `hydro/ranges` re-bakes to 4,166 words (2 + a 4,164-word record, down from 7,739) --
both properties of the coarse-fixes slice's flood/notch/capped-basin changes re-shaping which
notches and forced outlets each bake records, not of any change to what is compared.

**The seed control** drops by 3,631, both hydro groups together -- `hydro/plain` 622 -> 567
(-55) and `hydro/ranges` 7,536 -> 3,960 (-3,576) -- landing at 126,735 of 132,472 (was 130,366
of 136,086); every non-hydro group's movement is unchanged at 122,208. (Corrected in the final
fix wave below: this paragraph first said the drop was all `hydro/plain`, which `hydro/plain`'s
647 words cannot hold. The split was re-derived by replaying b49dd5a's own dump through
b49dd5a's own wasm.)

**The tectonic-warp control is the one whose shape actually changed, and it was checked against
the native TCTL prediction rather than merely re-run:** `hydro/ranges`'s own divergent count
under this control moved from 7,404 of 7,739 to **3,822 of 4,166** -- the re-baked `H ranges`
record is smaller, and `margin_warp_m` moves a slightly smaller fraction of it (95.7% before,
91.7% after; corrected in the final fix wave below) -- while `elevation/ranges` (132/5,000), `structural/ranges` (132/5,000),
`elevation/belt` (1,269/2,000), `structural/belt` (1,269/2,000) and `tile/belt` (3,384/4,225) are
byte-for-byte unchanged. The control's own printed line confirms it: `hydro/ranges 3822/4166
moved, exactly as the native side predicted`. **The tectonic-warp control still matches the
native TCTL prediction.**

The erosion-k, water-pond, coast-amplitude, gully-steer and climate-samples controls all move
by the corpus-shrink amount and nothing else -- their own divergent counts (216, 60, 13,128,
3,752, 648) are byte-for-byte unchanged, which is what says none of Tasks 1-6's changes reached
erosion, the water classifier, the coast channel, the gully channel or the climate march.

**Reproducing this section:**

```
cd viewer && npm run build:wasm

cargo test -p worldbuilder-engine <cfg> --no-fail-fast
cargo test -p worldbuilder-engine <cfg> -- --list                 # and --list --ignored
python .github/scripts/assert_counts.py cargo-list --all list-all.txt --ignored list-ignored.txt \
    --expect-passed <707|707|709|813|815> --expect-ignored 6

"D:\dev\worldbuilder_by_aetos\.venv\Scripts\python.exe" -m pytest --collect-only -q tests
                                                                    # 565 collected, 157 conformance

cd viewer && npm test                                              # 330 passed

cargo run --release -p worldbuilder-engine --example parity_dump --features wasm > \
    crates/worldbuilder-engine/parity/native.txt
cd crates/worldbuilder-engine/parity && node parity.mjs native.txt
node parity.mjs native.txt --mutate {seed|erosion-k|water-pond|tectonic-warp|coast-amplitude|gully-steer|climate-samples}
```

## 2026-09-11, water 1b-1 final review fix wave: pins re-derived

Notch widths now use the caller's params, as reach widths do (Ruling F-1), so `src/` moved and
the wasm was rebuilt: 335,764 bytes, 31 exports, 0 imports; artifact-sha256
cdb971154cc8d3edb36f0171bafcad0500a8279474c26df2d0d092288c954aea, source-fingerprint
bf7f1565496a5114aedfb5479a1bc76c5042163da384d7d49a360b982278a380 (53 inputs). `npm run
check:wasm` reports it matches its manifest and the source here.

**Engine, re-derived per configuration through `cargo test -p worldbuilder-engine <cfg> --
--list` (and `--ignored`) and `assert_counts.py cargo-list`, which printed `count OK` at all
five:**

| configuration | listed | ignored | **run** |
|---|---|---|---|
| `--no-default-features` | 716 | 6 | **710** |
| default | 716 | 6 | **710** |
| `--features python` | 718 | 6 | **712** |
| `--features wasm` | 822 | 6 | **816** |
| `--features python,wasm` | 824 | 6 | **818** |

707/707/709/813/815 -> **710/710/712/816/818**, 6 ignored, unchanged, over the same 16 test
binaries (16 since water 1a Task 12b; the Task 7 note in `gates.yml` that called this "FIFTEEN
at 16" is corrected there). **+3 uniformly on every row**, all in `src/hydrology/bake.rs`:
`an_outlet_cut_agrees_with_the_reach_it_runs_along`, `every_reach_that_reaches_the_ocean_is_fresh`
and `an_open_lake_may_drain_into_a_closed_lake`. `cargo test -p worldbuilder-engine
--no-fail-fast` ran 710 passed / 0 failed / 6 ignored, and `--features wasm --no-fail-fast` 816 /
0 / 6. The ignored sweep, `cargo test --release --lib every_small_world_drains -- --ignored`,
passed (73.8 s).

**Python:** 565 collected, 157 of them conformance -- unchanged (`pytest --collect-only -q`).

**Viewer:** `npm test` in `viewer/` -- **332 passed, 0 failed** (330 + the two new hydro tests).

**Parity, all eight runs `count OK` against `assert_counts.py parity`:**

| | compared | divergent |
|---|---|---|
| `parity` | **132,472** | **0** |
| `--mutate seed` | 132,472 | **126,737** (was 126,735) |
| `--mutate erosion-k` | 132,472 | 216 |
| `--mutate water-pond` | 132,472 | 60 |
| `--mutate tectonic-warp` | 132,472 | **10,013** (was 10,008) |
| `--mutate coast-amplitude` | 132,472 | 13,128 |
| `--mutate gully-steer` | 132,472 | 3,752 |
| `--mutate climate-samples` | 132,472 | 648 |

Against a dump taken at b49dd5a, 11 `hydro/plain` words and 31 `hydro/ranges` words moved --
every recorded notch width, each scaled by sqrt(effective / caller stream threshold): 3.760171x
on the plain bake, 18.432198x on the ranges bake. No count and no other word moved, and both
sides moved together, so the plain run stays at 0 divergent. The seed control moves by 2, all in
`hydro/plain` (567 -> 569 of 647). The tectonic control moves by 5, all in `hydro/ranges` (3,822
-> 3,827 of 4,166), and the native TCTL prediction moved with it, so the control still prints
"exactly as the native side predicted". The other five controls are unchanged.

The reproduction commands are the ones above, with `--expect-passed <710|710|712|816|818>`.

## 2026-09-12, water 1b-2 Task 8 (plan 1b-2): pins re-derived

Plan 1b-2 re-traces every reach at 1.5 km, adds falls and meanders, simplifies to 250 m and 1 m,
and moves the record to SCHEMA 4 (a 43-word header). `src/` moved in every task, and Task 8's
survey edit moved the fingerprint again, so the wasm was rebuilt (byte-identical to Task 7's
build; only the fingerprint moved): 351,072 bytes, 31 exports, 0 imports; artifact-sha256
978ba9a6e6dd860e9b5d86b6025c0884af61aa1f2a939e4a7bc2867cadcb7281, source-fingerprint
7fd5a28b86695a4e2c8fadbb354801c85250db3bafd42cf986d127ae10ef3ffb (55 inputs, up from 53 with
`bake_tests.rs` and `refine.rs`). `npm run check:wasm` reports it matches its manifest and the
source here.

**Engine, re-derived per configuration through `cargo test -p worldbuilder-engine <cfg> --
--list` (and `--ignored`) and `assert_counts.py cargo-list`, which printed `count OK` at all
five:**

| configuration | listed | ignored | **run** |
|---|---|---|---|
| `--no-default-features` | 749 | 6 | **743** (was 710) |
| default | 749 | 6 | **743** (was 710) |
| `--features python` | 751 | 6 | **745** (was 712) |
| `--features wasm` | 855 | 6 | **849** (was 816) |
| `--features python,wasm` | 857 | 6 | **851** (was 818) |

710/710/712/816/818 -> **743/743/745/849/851**, 6 ignored, unchanged, over the same 16 test
binaries. **+33 uniformly on every row**, all in `src/hydrology/`: the bake tests moved from
`bake.rs` (23) to `bake_tests.rs` (35, so 12 new), 20 new in `refine.rs`, and 1 new in
`routing.rs`. `cargo test -p worldbuilder-engine --no-default-features --no-fail-fast` ran 743
passed / 0 failed / 6 ignored, and `--features wasm --no-fail-fast` 849 / 0 / 6. The ignored
sweep, `cargo test --release -p worldbuilder-engine --lib every_small_world_drains --
--ignored`, passed (83.8 s).

**Python:** 565 collected, 157 of them conformance -- unchanged (`pytest --collect-only -q`).

**Viewer:** `npm test` in `viewer/` -- **333 passed, 0 failed** (332 + the preview's falls test).

**Parity, all eight runs `count OK` against `assert_counts.py parity`:**

| | compared | divergent |
|---|---|---|
| `parity` | **164,501** (was 132,472) | **0** |
| `--mutate seed` | 164,501 | **158,765** (was 126,737) |
| `--mutate erosion-k` | 164,501 | 216 |
| `--mutate water-pond` | 164,501 | 60 |
| `--mutate tectonic-warp` | 164,501 | **34,909** (was 10,013) |
| `--mutate coast-amplitude` | 164,501 | 13,128 |
| `--mutate gully-steer` | 164,501 | 3,752 |
| `--mutate climate-samples` | 164,501 | 648 |

The whole +32,029 compared is the two hydro records growing with refinement: `hydro/plain` 647
-> 7,784 words and `hydro/ranges` 4,166 -> 29,058. No H word diverges native against wasm. The
seed control's hydro groups move to 7,703 of 7,784 and 28,854 of 29,058; every non-hydro group
is unmoved at 122,208. The tectonic control's `hydro/ranges` moves to 28,723 of 29,058, and the
native TCTL prediction's sixth field moved with it (3827 -> 28723), so the control still prints
"exactly as the native side predicted"; the five belt groups are unchanged at 6,186. The other
five controls are unchanged.

The reproduction commands are the ones above, with `--expect-passed <743|743|745|849|851>`.

## 2026-09-12, water 1b-2 Task 8, the 500 m ruling: pins re-derived again

The owner's world (`worlds/world-1788998299904.json`, forced outlet 0,0, 1,000,000 nodes, baked
in the studio) produced an 8,659,856-byte record at `refine_simplify_m` 250 m, over the 8 MB
target, so the plan's Step 2 rule applied: `earth_like`'s `refine_simplify_m` rose from 250 to
**500 m** (Ruling R-7's tolerance). At 500 m the native stand-ins at 1M nodes are 4,227,936 /
2,471,408 / 5,966,824 bytes (plain / owner_survey / seed1_ranges). The wasm was rebuilt: 351,072
bytes, 31 exports, 0 imports; artifact-sha256
3e0a305014e7e38b4d8bd710804cf12cc9b4aad596be36fd5af4677aa73e8c98, source-fingerprint
72fa850d7fdaef7cdd13114327bcf5d975bcfb4799cbbfffd2bdf866fe252fc3 (55 inputs).

**Engine:** 743/743/745/849/851, 6 ignored (listed 749/749/751/855/857), **unchanged**; all five
printed `count OK`. The one test that named the tolerance now scales its offsets with it.
`--no-default-features --no-fail-fast` ran 743 / 0 / 6, `--features wasm --no-fail-fast` 849 /
0 / 6, and the ignored `every_small_world_drains` sweep passed (81.9 s).

**Python:** 565 collected, 157 conformance, unchanged. **Viewer:** 333 passed, 0 failed.

**Parity, all eight `count OK`:**

| | compared | divergent |
|---|---|---|
| `parity` | **146,555** (was 164,501) | **0** |
| `--mutate seed` | 146,555 | **140,818** (was 158,765) |
| `--mutate erosion-k` | 146,555 | 216 |
| `--mutate water-pond` | 146,555 | 60 |
| `--mutate tectonic-warp` | 146,555 | **20,811** (was 34,909) |
| `--mutate coast-amplitude` | 146,555 | 13,128 |
| `--mutate gully-steer` | 146,555 | 3,752 |
| `--mutate climate-samples` | 146,555 | 648 |

`hydro/plain` shrinks from 7,784 to 3,938 words and `hydro/ranges` from 29,058 to 14,958 (the
whole -17,946). The seed control's hydro groups are 3,857 of 3,938 and 14,753 of 14,958, with
the non-hydro groups unmoved at 122,208. The tectonic control's `hydro/ranges` is 14,625 of
14,958, matching the native TCTL prediction's sixth field (28723 -> 14625); the belt groups
are unmoved at 6,186.

The reproduction commands are the ones above, with `--expect-passed <743|743|745|849|851>`.

## 2026-09-12, water 1b-2 final review fix wave: pins re-derived

The fix wave records a fall's ends as points of its reach rather than re-derived coordinates
(Rulings FF-1 and FF-4), steps a blocked station back toward its chord (Ruling FF-2 / R-3a), and
floors the refinement params (Ruling FF-5). `src/` moved, so the wasm was rebuilt: 351,584 bytes,
31 exports, 0 imports; artifact-sha256
5eb54e0a5ef5a69158e675a951190715da3e038a4c18edaf3b89cf49becaccb6, source-fingerprint
c31e095278b3930413431e782b6c629ca9ed6387c079b3328ccca259cfef54d2 (55 inputs). `npm run
check:wasm` reports it matches its manifest and the source here.

**Engine, re-derived per configuration through `cargo test -p worldbuilder-engine <cfg> --
--list` (and `--ignored`) and `assert_counts.py cargo-list`, which printed `count OK` at all
five:**

| configuration | listed | ignored | **run** |
|---|---|---|---|
| `--no-default-features` | 757 | 6 | **751** (was 743) |
| default | 757 | 6 | **751** (was 743) |
| `--features python` | 759 | 6 | **753** (was 745) |
| `--features wasm` | 863 | 6 | **857** (was 849) |
| `--features python,wasm` | 865 | 6 | **859** (was 851) |

743/743/745/849/851 -> **751/751/753/857/859**, 6 ignored, unchanged, over the same 16 test
binaries. **+8 uniformly on every row**, all in `src/hydrology/`: 5 new in `refine.rs` (a fall at
a coarse start, a cliff in a step's last window, the blocked station's step back, the chord-point
exception R-3a, a short `protected` slice) and 3 new in `bake_tests.rs` (no inland station on sea
ground, every refined mouth at or below its water, the refinement params' floors). Every
configuration was also run, not only listed: 751 / 0 / 6, 751 / 0 / 6, 753 / 0 / 6, 857 / 0 / 6
and 859 / 0 / 6 passed / failed / ignored. The ignored sweep, `cargo test --release -p
worldbuilder-engine --lib every_small_world_drains -- --ignored`, passed (67.0 s).

**Python:** 565 collected, 157 of them conformance -- unchanged (`pytest --collect-only -q`).

**Viewer:** `npm test` in `viewer/` -- **333 passed, 0 failed**, unchanged.

**Parity, all eight runs `count OK` against `assert_counts.py parity`:**

| | compared | divergent |
|---|---|---|
| `parity` | 146,555 | **0** |
| `--mutate seed` | 146,555 | 140,818 |
| `--mutate erosion-k` | 146,555 | 216 |
| `--mutate water-pond` | 146,555 | 60 |
| `--mutate tectonic-warp` | 146,555 | **20,808** (was 20,811) |
| `--mutate coast-amplitude` | 146,555 | 13,128 |
| `--mutate gully-steer` | 146,555 | 3,752 |
| `--mutate climate-samples` | 146,555 | 648 |

Both hydro records keep their word counts (`hydro/plain` 3,938, `hydro/ranges` 14,958): these
fixes move where a station stands and which point a fall names, not how many words a record has.
Native and wasm moved together, so the plain run stays at 0 divergent, and the seed control is
unmoved in every group (3,857 of 3,938, 14,753 of 14,958, non-hydro 122,208). The tectonic
control's `hydro/ranges` moves by 3, to 14,622 of 14,958, and the native TCTL prediction's sixth
field moved with it (14625 -> 14622), so the control still prints "exactly as the native side
predicted"; the five belt groups are unchanged at 6,186.

**`hydro_survey` at 1,000,000 nodes** (native, release, this host; the owner-world figures stay
the controller's):

| world | bake | (stages / record_of / refine) | record | falls |
|---|---|---|---|---|
| plain | 11.22 s | 10.02 / 0.03 / 1.18 | 4,227,936 bytes (was 4,227,936) | 0 |
| owner_survey | 10.94 s | 10.39 / 0.02 / 0.54 | 2,471,360 bytes (was 2,471,408) | 4 |
| seed1_ranges | 12.05 s | 10.21 / 0.05 / 1.79 | 5,967,064 bytes (was 5,966,824) | 0 |

All three are under the 8 MB target, and `drainage_check` is `Ok` on all three.

The reproduction commands are the ones above, with `--expect-passed <751|751|753|857|859>`.

## 2026-09-12, water 1b-3 Task 7 (plan 1b-3): pins re-derived

Plan 1b-3 added the crossing pass (Rulings S-2 to S-4) and the fine pond search (`ponds.rs`,
Rulings S-5 to S-13), taking the record to **SCHEMA 5, a 54-word header**. Task 7 measured the
size and time gates on the three 1,000,000-node stand-ins and re-derived every pin by running it.

**The size gate moved a param.** `HydroParams::earth_like`'s `pond_density_area_m2` was spec
§6.6's 500 km² (5.0e8) and the `seed1_ranges` stand-in's record came out at 9,020,112 bytes,
over the 8,000,000-byte target. Raised in ×2 steps, measured at each
(`hydro_survey --pond-density D 1000000`, native release, this host):

| `pond_density_area_m2` | plain | owner_survey | seed1_ranges | ponds kept (seed1) |
|---|---|---|---|---|
| 5.0e8 (spec §6.6) | 6,148,920 | 2,909,656 | **9,020,112** over | 6,952 |
| 1.0e9 | 5,992,328 | 2,877,416 | **8,746,368** over | 6,280 |
| 2.0e9 | 5,785,912 | 2,841,000 | **8,357,824** over | 5,355 |
| **4.0e9 (shipped)** | 5,520,664 | 2,780,984 | **7,896,992** fits | 4,270 |
| 8.0e9 (measured, not shipped) | 5,222,632 | 2,717,464 | 7,407,872 | 3,179 |

The cap is a weak lever — each doubling removes about a tenth of the kept bodies, because the
3 km corridors rarely put two candidates in one cell — so four steps were needed and the fifth
was measured too, in case the owner world needs the headroom.

That sweep was measured before Ruling S-14 and has not been re-run step by step; **the decision it
reached was re-checked on S-14's pipeline and still holds** -- at 4.0e9 the three stand-ins record
5,521,624 / 2,781,080 / 7,898,992 bytes, all under 8,000,000. The `seed1_ranges` margin is 101,008
bytes, 1.26%, essentially what it was.

**The time gate binds nothing.** The pond search's own part, at the shipped 4.0e9 on S-14's
pipeline: plain 60.43 s, owner_survey 27.01 s, seed1_ranges 83.30 s, against a 120 s gate (before
S-14: 59.26 / 25.59 / 77.45 s). `pond_search_radius_m` stays at
3,000 m and `pond_cell_m` stays at the spec's 250 m; neither was moved.

`src/` moved, so the wasm was rebuilt: **427,667 bytes**, 31 exports, 0 imports; artifact-sha256
`fcc0439e920044277e79e9dd2194600cc7a05f9213b1ac1e81011571c971b726`, source-fingerprint
`cc4742703c0fcf86074902f20b34d2e6bb66b32a9216a3d8f389de779255c301` (58 inputs). `npm run
check:wasm` reports it matches its manifest and the source here. (The figures first recorded here
were 426,828 bytes / `8112f5e1...` / `63ee7748...`; Ruling S-14 superseded them.)

**Engine, re-derived per configuration through `cargo test -p worldbuilder-engine <cfg> --
--list` (and `--ignored`) and `assert_counts.py cargo-list`, which printed `count OK` at all
five:**

| configuration | listed | ignored | **run** |
|---|---|---|---|
| `--no-default-features` | 790 | 7 | **783** (was 751) |
| default | 790 | 7 | **783** (was 751) |
| `--features python` | 792 | 7 | **785** (was 753) |
| `--features wasm` | 896 | 7 | **889** (was 857) |
| `--features python,wasm` | 898 | 7 | **891** (was 859) |

751/751/753/857/859 -> **783/783/785/889/891**, and `expect_ignored` **6 -> 7**, over **seventeen** test
binaries rather than sixteen: `src/bin/shore_probe.rs` came back in Task 1 (commit d72312a), and a
bin is a test target. **+32 uniformly on every row**, all from this plan's Tasks 1-6 — the
crossing pass in `reaches.rs` and `bake_tests.rs`, the fine pond search in the new `ponds.rs`, and
the SCHEMA 5 record words in `record.rs`/`bake_tests.rs`. Task 7's own source edits
(`src/bin/hydro_survey.rs`, `examples/pond_search_survey.rs` and `earth_like`'s
`pond_density_area_m2`) add no test. The seventh ignored test is Ruling S-14's own:
`hydrology::bake_tests::refinement_adds_no_crossings_at_1m`, the 1,000,000-node sweep that
reproduces what Task 7 measured. Both ignored sweeps were run: `every_small_world_drains` passed
in 71.12 s, and `refinement_adds_no_crossings_at_1m` passed in 191.58 s, printing `ranges 1M:
5545 reaches, coarse 54 shipped 50` and `default 1M: 4285 reaches, coarse 33 shipped 28`.

**Python:** 565 collected, 157 of them conformance -- unchanged (`pytest --collect-only -q`).

**Viewer:** `npm test` in `viewer/` -- **337 passed, 0 failed** (was 333; Task 6 added four
`water-preview` tests).

**Parity, all eight runs from `crates/worldbuilder-engine/parity/` as `gates.yml` runs them.**
Re-derived again after Ruling S-14 and **unmoved**: S-14 changes where the shipped points sit, not
how many words a record has, and native and wasm move together, so every row below is what both
derivations produced.


| | compared | divergent |
|---|---|---|
| `parity` | **158,733** (was 146,555) | **0** |
| `--mutate seed` | 158,733 | **152,886** (was 140,818) |
| `--mutate erosion-k` | 158,733 | 216 |
| `--mutate water-pond` | 158,733 | 60 |
| `--mutate tectonic-warp` | 158,733 | **31,857** (was 20,808) |
| `--mutate coast-amplitude` | 158,733 | 13,128 |
| `--mutate gully-steer` | 158,733 | 3,752 |
| `--mutate climate-samples` | 158,733 | 648 |

The whole +12,178 is the two hydro records: `hydro/plain` 3,938 -> 5,001 words (+1,063) and
`hydro/ranges` 14,958 -> 26,073 (+11,115) — SCHEMA 5's eleven extra header words plus the bodies
the fine search adds. Every non-H group is unmoved, in the plain run and in all seven controls.
The seed control moves with them (`hydro/plain` 4,900 of 5,001, `hydro/ranges` 25,778 of 26,073,
non-hydro 122,208 unmoved). The tectonic control's `hydro/ranges` goes 14,622 -> 25,671 of 26,073
and the native TCTL prediction's sixth field moved with it, so the control still prints "exactly
as the native side predicted"; the five belt groups are unchanged at 6,186.

**`hydro_survey` at 1,000,000 nodes**, at the shipped params (native, release, this host; the
owner-world figures stay the controller's):

| world | bake | (stages / record_of / refine / ponds) | record | crossings coarse / shipped | ponds found / kept |
|---|---|---|---|---|---|
| plain | 71.40 s | 9.28 / 0.02 / 1.68 / 60.43 | 5,521,624 bytes (was 4,227,936) | 33 / **28** | 9,358 / 2,780 |
| owner_survey | 37.74 s | 9.93 / 0.01 / 0.79 / 27.01 | 2,781,080 bytes (was 2,471,360) | 4 / **3** | 2,083 / 691 |
| seed1_ranges | 96.24 s | 10.27 / 0.04 / 2.63 / 83.30 | 7,898,992 bytes (was 5,967,064) | 54 / **50** | 15,683 / 4,270 |

All three are under the 8 MB target, the pond search is under its 120 s gate on all three, and
`drainage_check` is `Ok` on all three.

**These figures supersede the first ones recorded in this section** (5,520,664 / 2,780,984 /
7,896,992 bytes, and crossings 36 / 5 / 57 against 33 / 4 / 54 coarse). Task 7's first run
measured the shipped record crossing MORE than the coarse record it came from, because Ruling S-4
put the crossing pass between tracing and the meander, and neither the meander nor
Douglas-Peucker was looked at again. **Ruling S-14** (commit 2ce614e) moved the pass to the end of
the pipeline -- trace, meander, simplify, then check -- and raised `MAX_CROSSING_PASSES` 3 -> 4.
Shipped is now at or under coarse on every stand-in, and the record grew by 960 / 96 / 2,000
bytes, which is the straightened segments' own points surviving simplification differently.

The reproduction commands are the ones above, with `--expect-passed <783|783|785|889|891>`.

## 2026-09-12, water 1b-3 Ruling S-16: the density cap again, and the pins with it

The section above took `pond_density_area_m2` to 4.0e9 because the three 1,000,000-node stand-ins
fit at that value. **The owner's world did not.** Baked in the branch studio through the wasm
pool at 1,000,000 nodes, with its two painted features and one forced outlet at 0°N 0°E, it
recorded **11,146,072 bytes against the 8,000,000-byte gate** — 11,926 bodies, of which 348 are
coarse and **11,578 are traced ponds** out of 131,386 found, about **5.2 MB of the record**. Time
was never the problem: 124 s against the 300 s gate, pond search included. Everything else on that
bake was clean (4,876 reaches over 114,941 points; crossings 43 coarse → 37 shipped; 0 bed rises;
3,268 junctions all shared; 0 mouths above their water; 0 ponds breaking Ruling S-5).

**Ruling S-16: `pond_density_area_m2` 4.0e9 → 1.6e10** — one kept body per 16,000 km² of searched
corridor, **32× spec §6.6's 500 km²**. Two doublings rather than one, because one lands near
8.6 MB with no margin and the owner's world is the world that must fit. The other levers were
refused on measured grounds: Ruling S-13 shows a coarser outline breaks containment, and the keep
rule is not the limiter (that bake's median pond is 4.38 km² against a 0.05 km² floor — on the
bake that finally ships, at Ruling S-17's 1.5 km corridor, the median is 1.81 km²; its
area p10/p50/p90 are 1.69 / 4.38 / 10.19 km² and its depths 2.7 / 7.0 / 22.6 m). The consequence,
stated plainly: fewer ponds on the owner's world, and at the time this section was written the
prediction was roughly 2,900 against 11,578. **The measured answer, once Ruling S-17 also halved
the corridor, is 3,719** — see the S-17 section below.

### The three stand-ins at 1.6e10

`./target/release/hydro_survey.exe 1000000`, no flags. `drainage_check` is `Ok` on all three.

| world | bake | (stages / record_of / refine / ponds) | record | crossings coarse / shipped | ponds found / kept |
|---|---|---|---|---|---|
| plain | 70.26 s | 9.11 / 0.03 / 1.57 / **59.56** | **4,934,968** (was 5,521,624) | 33 / 28 | 9,358 / **1,496** (was 2,780) |
| owner_survey | 37.73 s | 10.06 / 0.01 / 0.75 / **26.91** | **2,650,888** (was 2,781,080) | 4 / 3 | 2,083 / **409** (was 691) |
| seed1_ranges | 91.23 s | 9.54 / 0.04 / 2.63 / **79.02** | **6,966,736** (was 7,898,992) | 54 / 50 | 15,683 / **2,237** (was 4,270) |

**Both gates hold, and the size margin is no longer thin.** The worst record clears 8,000,000 by
**1,033,264 bytes, 12.9%**, where at 4.0e9 it cleared by 101,008 bytes and 1.26%. The pond search
is 59.56 / 26.91 / 79.02 s against its 120 s gate; the reach geometry, crossings and drainage are
untouched, because the cap changes only which found candidates are kept.

### Pins, re-derived by running them

**Wasm rebuilt:** 427,667 bytes, 31 exports, 0 imports; artifact-sha256
`3c6cb37bfba267966ec13f1a3c8977bf7d5b4b7d92e1fc890c98298b9c646e99`, source-fingerprint
`2f327438c6fb9b83ab044e0c191e50d8b58cf7e29aaba621fd9dc0bbab6c93db` (58 inputs). `npm run
check:wasm` reports it matches its manifest and the source here. (Superseded: 427,667 bytes /
`fcc0439e...` / `cc474270...`.)

**Engine — unchanged.** 783 / 783 / 785 / 889 / 891 run, 7 ignored, `listed`
790 / 790 / 792 / 896 / 898, over seventeen binaries. A tuning value is not a test. Re-derived per
configuration anyway through `cargo test -p worldbuilder-engine <cfg> -- --list` (and
`--ignored`) and `assert_counts.py cargo-list`; all five printed `count OK`, and the suite was
run: 764 + 4 + 9 + 6 = **783 passed, 0 failed, 7 ignored**.

**Both ignored sweeps pass.** `every_small_world_drains` in 64.61 s;
`refinement_adds_no_crossings_at_1m` in 183.02 s, printing `ranges 1M: 5545 reaches, coarse 54
shipped 50` and `default 1M: 4285 reaches, coarse 33 shipped 28` — unchanged, as expected, since
the cap touches no reach.

**Python:** 565 collected, 157 conformance — unchanged. **Viewer:** `npm test` 337 pass, 0 fail —
unchanged.

**Parity — moved:**

| | compared | divergent |
|---|---|---|
| `parity` | **155,919** (was 158,733) | **0** |
| `--mutate seed` | 155,919 | **150,097** (was 152,886) |
| `--mutate erosion-k` | 155,919 | 216 |
| `--mutate water-pond` | 155,919 | 60 |
| `--mutate tectonic-warp` | 155,919 | **29,068** (was 31,857) |
| `--mutate coast-amplitude` | 155,919 | 13,128 |
| `--mutate gully-steer` | 155,919 | 3,752 |
| `--mutate climate-samples` | 155,919 | 648 |

The whole **−2,814** is `hydro/ranges`, 26,073 → 23,259 words: two doublings of the cap keep fewer
ponds, and a pond that is not kept is not recorded. `hydro/plain` stays at **5,001** — that
world's kept ponds were already inside the wider cell. Every non-H group is byte-for-byte unmoved
in the plain run and in all seven controls. Under the seed control `hydro/ranges` goes 25,778 of
26,073 → **22,988 of 23,259** and `hydro/plain` 4,900 → **4,901 of 5,001** (one more word of an
unchanged record now differs under a moved seed); non-hydro is 122,208, unmoved. The tectonic
control's `hydro/ranges` goes 25,671 → **22,882 of 23,259** and the native TCTL prediction's sixth
field moved with it, so the control still printed *"exactly as the native side predicted"*; the
five belt groups are unchanged at 6,186.

The reproduction commands are the ones above, with `--expect-passed <783|783|785|889|891>` and
`--expect-ignored 7`.

## 2026-09-12, water 1b-3 Ruling S-17: the corridor halves, and the pins with it

At 3 km with the cap already at 1.6e10, the owner's world **missed both gates**: 8,430,792 bytes
against 8,000,000, and **441 s then 432 s** (two runs, the second on a settled page) against 300 s.
The earlier 124 s reading did not reproduce and is discarded as an outlier. Ponds were 5,184 kept
of 131,386 found. Everything else on that bake was clean and unchanged.

**Ruling S-17: `pond_search_radius_m` 3,000 → 1,500 m.** It is the only lever that moves both
gates at once. The search samples a lane `2 × radius` wide, so its cost is **linear** in this
number; and because a hollow must sit wholly inside the strip to survive Ruling S-10's side clip,
a narrower lane drops candidates **faster** than it drops time. It was preferred over
`pond_cell_m`, which is the spec's 250 m trace — coarsening it would make every recorded outline
coarser and put Ruling S-13's containment result back in question — and over another density
doubling, because thinning what the search already found is worse than not looking as far.
`pond_density_area_m2` stays at 1.6e10.

### The three stand-ins at 1,500 m

`./target/release/hydro_survey.exe 1000000`, no flags. `drainage_check` is `Ok` on all three.

| world | bake | (stages / record_of / refine / **ponds**) | record | ponds found / kept |
|---|---|---|---|---|
| plain | **39.99 s** (was 70.26) | 8.88 / 0.03 / 1.56 / **29.53** (was 59.56) | **4,241,672** (was 4,934,968) | **1,993 / 273** (was 9,358 / 1,496) |
| owner_survey | **24.66 s** (was 37.73) | 9.92 / 0.02 / 0.75 / **13.97** (was 26.91) | **2,461,400** (was 2,650,888) | **409 / 57** (was 2,083 / 409) |
| seed1_ranges | **53.14 s** (was 91.23) | 9.71 / 0.06 / 2.47 / **40.90** (was 79.02) | **5,974,160** (was 6,966,736) | **3,752 / 504** (was 15,683 / 2,237) |

**The lever behaves exactly as predicted on time and better than predicted on count.** The pond
part scales 0.496 / 0.519 / 0.518 — linear in the radius, to within 4%. The candidate count
scales 0.213 / 0.196 / 0.239 — about a **fifth**, not a half, because the narrower lane side-clips
far more hollows. Reaches, reach points, crossings (33/28, 4/3, 54/50), notches, capped basins and
drainage are bit-identical to the 3 km run: the corridor decides where the search looks, and
nothing else.

Both gates hold with room: the worst record clears 8,000,000 by **2,025,840 bytes (25.3%)** and
the worst pond search is **40.90 s of 120 s (34%)**.

### Pins, re-derived by running them

**Wasm rebuilt:** 427,667 bytes, 31 exports, 0 imports; artifact-sha256
`e590b3b9c2fe9273272d140c1b85aab2877748282b58e2350f4563dfc8660454`, source-fingerprint
`7669d18aa7962c27e370b2ceabc22c9086efbbe9f01f412f30e232f782a79dea` (58 inputs). `npm run
check:wasm` reports it matches. (Superseded: `3c6cb37b…` / `2f327438…`.)

**Engine — unchanged.** 783 / 783 / 785 / 889 / 891 run, 7 ignored, `listed`
790 / 790 / 792 / 896 / 898. Seven fixtures had to be *adapted*, none added or removed:
`ponds::tests::params` and `bake_tests::ponds_obey_their_keep_rule_and_name_a_river` now pin spec
§6.6's 3 km corridor explicitly — their bowls, offsets and expected cell counts were laid out
against it, and what they assert is the search's mechanism rather than which width ships — and
`viewer/test/water-preview.test.mjs` bakes its pond test at 50,000 nodes instead of 12,000,
because a wasm bake always takes `earth_like`'s pond params and at 1.5 km the 12,000-node plain
world keeps none. All five printed `count OK`; the suite ran 783 passed / 0 failed / 7 ignored.

**Both ignored sweeps pass.** `every_small_world_drains` in 65.10 s;
`refinement_adds_no_crossings_at_1m` in 114.81 s, printing the same `ranges 1M: … coarse 54
shipped 50` and `default 1M: … coarse 33 shipped 28`.

**Python:** 565 / 157 — unchanged. **Viewer:** 337 pass, 0 fail — unchanged.

**Parity — moved:**

| | compared | divergent |
|---|---|---|
| `parity` | **147,553** (was 155,919) | **0** |
| `--mutate seed` | 147,553 | **141,765** (was 150,097) |
| `--mutate erosion-k` | 147,553 | 216 |
| `--mutate water-pond` | 147,553 | 60 |
| `--mutate tectonic-warp` | 147,553 | **21,783** (was 29,068) |
| `--mutate coast-amplitude` | 147,553 | 13,128 |
| `--mutate gully-steer` | 147,553 | 3,752 |
| `--mutate climate-samples` | 147,553 | 648 |

The −8,366 is both hydro records: `hydro/ranges` 23,259 → **15,945** and `hydro/plain`
5,001 → **3,949**. **3,949 is 3,938 + 11** — the pre-pond record plus SCHEMA 5's eleven header
words — so that world now keeps **no pond at all** at 1.5 km. Every non-H group is byte-for-byte
unmoved in the plain run and in all seven controls. Under the seed control `hydro/plain` is
3,859 of 3,949, which is exactly the 3,857-of-3,938 this control reported before ponds existed
plus the two header words a moved seed moves. The tectonic control's `hydro/ranges` goes
22,882 → **15,597 of 15,945**, its native TCTL prediction moved with it, and it again printed
*"exactly as the native side predicted"*; the five belt groups are unchanged at 6,186.

### What these numbers predict for the owner's world

Two fitted models, both stated with their assumptions so they can be checked rather than trusted.

**Size — a linear fit on the owner world's own two bakes.** It recorded 11,146,072 bytes with
11,578 ponds (cap 4.0e9) and 8,430,792 with 5,184 (cap 1.6e10). Two points give **424.7 bytes a
pond** and a **6,228,495-byte base**; that base plus 5,184 × 424.7 reproduces the second bake to
within 652 bytes, so the fit is sound. The pond budget under an 8,000,000 gate is therefore
1,771,505 bytes, or **about 4,170 ponds**. Applying the stand-ins' measured kept ratio for this
same 3,000 → 1,500 m step (0.18 / 0.14 / 0.23) to 5,184 gives **730–1,190 ponds** and a record of
**6.54–6.73 MB**. **Size should pass with roughly 16–18% of margin.**

**Time — a share model on the split the nearest stand-in shows.** `seed1_ranges` is the closest
stand-in by reach points (113,840 against the owner world's 114,941) and its bake was 86.6% pond
search at 3 km. Only the pond part halves. At 432 s that gives 187 s + 58 s = **245 s**; at an
80% share, 259 s; at 90%, 237 s. **Time should pass, but only by 14–21%**, and that is the gate
to watch.

**If the re-bake still misses 300 s, the next lever is `pond_search_radius_m` 1,500 → 1,000 m**,
not `pond_cell_m` and not the density. It is linear again (×0.667 on the pond part), it keeps the
250 m trace and Ruling S-13's result intact, and it would take the prediction to about 183 s. The
cost to name: at 1,000 m a strip is 9 cells across, side-clipping bites harder still, and ponds
would get noticeably sparse — on these stand-ins the kept counts would likely fall into the low
hundreds. `pond_cell_m` should stay the last resort.

The reproduction commands are the ones above, with `--expect-passed <783|783|785|889|891>` and
`--expect-ignored 7`.

## 2026-09-12, water 1b-3 Task 7 review fix wave: one value pin, and the pins with it

The review of Task 7 found no defect in the pins or the fixtures — it checked both by arithmetic
and by reading — and three things wrong in prose, plus one missing test. The test is the only
behaviour change.

**`bake_tests::earth_like_ships_the_tuned_pond_corridor_and_density`.** After Ruling S-17, every
other pond test in the tree pins spec §6.6's 3 km corridor on purpose — six `ponds::tests` cases
through their own `params()`, and `ponds_obey_their_keep_rule_and_name_a_river` locally — because
their bowls and cell counts were laid out against it and what they assert is the search's
mechanism. Each is right on its own; in aggregate they meant **no Rust test baked at the values
that ship**, and a drift in `pond_search_radius_m` or `pond_density_area_m2` would have been
caught only by a parity count moving. The new test pins all three constants (1,500 m, 1.6e10 and
the untuned 250 m cell) and names S-17 and S-16 in its messages. It pins the constants, not what
they do; the behavioural gap is recorded in the carry-forward for plan 1b-4.

**Engine, +1 uniformly** — re-derived per configuration through `cargo test -p worldbuilder-engine
<cfg> -- --list` (and `--ignored`) and `assert_counts.py cargo-list`; all five printed `count OK`:

| configuration | listed | ignored | **run** |
|---|---|---|---|
| `--no-default-features` | 791 | 7 | **784** (was 783) |
| default | 791 | 7 | **784** (was 783) |
| `--features python` | 793 | 7 | **786** (was 785) |
| `--features wasm` | 897 | 7 | **890** (was 889) |
| `--features python,wasm` | 899 | 7 | **892** (was 891) |

The suite was run: 765 + 4 + 9 + 6 = **784 passed, 0 failed, 7 ignored**. The drain sweep passed
in 66.31 s.

**Parity — unmoved, and re-derived rather than assumed:** 147,553 compared / 0 divergent; seed
141,765; erosion-k 216; water-pond 60; tectonic-warp 21,783 (again *"exactly as the native side
predicted"*); coast 13,128; gully 3,752; climate 648. A test changes no record. **Python** 565 /
157 and the **viewer's** 337 are unmoved.

**Wasm rebuilt:** 427,667 bytes; artifact-sha256
`e590b3b9c2fe9273272d140c1b85aab2877748282b58e2350f4563dfc8660454` — **identical**, since a test
is not compiled into it — and source-fingerprint
`f9e351a4e94c53bbdda5f1e811382b1b60238434f2114550165422ef2670392d` (58 inputs). `check:wasm`
reports it matches.

**Prose corrected in the same wave.** The "roughly twenty times as many ponds" figure was not
measured and is replaced, in both the verification report and the carry-forward, by **about ten
times (roughly 39,000 against 3,719)**, stated as an extrapolation and showing the two numbers it
rests on: one measured bake (5,184 ponds at the spec's 3 km corridor with the shipped cap) and one
extrapolated ratio (×0.669 a cap doubling, measured over two doublings and applied over five).
The median pond area now says which bake it came from — **4.38 km² at the 3 km corridor, 1.81 km²
at the 1.5 km corridor that ships** — in `mod.rs`, here, and in the verification report.

The reproduction commands are the ones above, with `--expect-passed <784|784|786|890|892>` and
`--expect-ignored 7`.

## 2026-09-12, water 1b-3 whole-branch review fix wave: an order pin, a params floor, and the pins with them

The whole-branch review ran 34 real bakes and found **no correctness defect in the shipped
geometry**. What it found was three gaps in what the branch *pins* and *says*, and five smaller
things. Two of the fixes are behaviour or test-count changes; the rest is prose.

**`bake_tests::a_yielded_segment_ships_on_its_chord` — the order pin Ruling S-14 never had.**
`refinement_adds_no_crossings` compares an outcome, and Task 7 measured that at 200,000 nodes that
outcome reads 8 against 9 **under the old, wrong pipeline order too**, so it cannot tell the two
apart; the one that can, `refinement_adds_no_crossings_at_1m`, is `#[ignore]`d for its cost. The
branch's headline property therefore shipped with no gate that runs. The new test asserts the one
thing true only of the new order: a segment the pass made yield is still on its chord in
`record.reaches`, i.e. *after* `ship`'s meander and Douglas–Peucker. It re-derives the yielding
segments rather than reading them out of `refine` — the first round's lines are `refine_reach`
plus `simplify`, which is exactly `ship` with nothing yielded yet — and the nine it names on the
`junction_params` world are the nine `refine`'s own first round straightens.

*Shown RED and GREEN.* With the pass moved back before the meander and the simplification (a
temporary patch to `refine::refine`, reverted), it fails: `reach 2 segment 0 yielded, yet its
shipped point 1 stands 3.5730507806474634 m off its chord`. On the shipped order it passes with
**14 shipped interior points, worst 1.28e-9 m off chord**, against a 1 mm bar. Two meander params
are widened for this population and **they are the discriminator**: `earth_like`'s meander needs a
channel over about 545 m wide, and nothing that yields on a 12,000-node world is a river that
large — measured, not assumed, because the first version of this test read green on both
pipelines. One 12k bake, about 2 s in release.

**`bake` refuses `pond_density_area_m2 < pond_cell_m²`.** Ruling S-8's cap is applied on a
`BucketIndex` of `sqrt(pond_density_area_m2)`, and `bake` required only finite-positive: `1.0e4`
asks for about **800 MB of buckets**, which the reviewer reproduced. `BucketIndex::new` clamps at
4,096 rows by 8,192 columns **and says nothing**, so the request also silently realises a
**4,886.50 m** cell — 23.88 km², 2,388 times the area asked for. RED first
(`pond_params_below_their_floors_are_refused` gained a `pond_density_area_m2` row and failed with
*"pond_density_area_m2 outside its floor is refused"*), then GREEN. `buckets` gained
`finest_cell_m(radius_m)` and `BucketIndex::cell_m()` so a caller whose cell *is* the thing it
means can tell.

**The Task 5 density-cap figures, corrected.** `pond_search_survey`'s "uncapped" baseline was that
same `1.0e4`, so every *"the cap removes N%"* figure was measured against a 23.88 km² baseline,
not against no cap. The numbers themselves stand — the clamp lands on **exactly** the cell
`finest_cell_m` names, which the new `buckets` test asserts as an equality, so the honest request
realises the identical grid — but the reading changes: **8–12% is a lower bound on what the cap
removes, not the figure.** Re-run at Task 5's own shipped values with the explicit baseline, as
found/kept/baseline/removes: 41/14/14/**0**; 644/286/314/**28 (8.9%)**; 79/33/34/**1 (2.9%)**;
1,401/603/683/**80 (11.7%)**; 2,783/1,249/1,402/**153 (10.9%)** — the *kept* and *removes* columns
reproduce Task 5's table, and the survey now asserts its own baseline is realised.

**Engine, +2 uniformly** — re-derived per configuration through `cargo test -p worldbuilder-engine
<cfg> -- --list` (and `--ignored`) and `assert_counts.py cargo-list`; all five printed `count OK`:

| configuration | listed | ignored | **run** |
|---|---|---|---|
| `--no-default-features` | 793 | 7 | **786** (was 784) |
| default | 793 | 7 | **786** (was 784) |
| `--features python` | 795 | 7 | **788** (was 786) |
| `--features wasm` | 899 | 7 | **892** (was 890) |
| `--features python,wasm` | 901 | 7 | **894** (was 892) |

The suite was run: 767 + 4 + 9 + 6 = **786 passed, 0 failed, 7 ignored** (769 + 4 + 9 + 6 = 788
with `python`; + 106 `wasm_exports` for the wasm rows). `no_std_math` 6/6. The drain sweep passed
in 66.91 s. The **viewer's 337** are unmoved.

**Parity — unmoved, and not re-derived, with the reason stated:** a test changes no record, and
this wave's one behaviour change is a *rejected-params* floor that no shipped population comes
near (`earth_like` ships 1.6e10 against a 62,500 m² floor). The wasm's 12-word param buffer sets
no pond param at all.

**Wasm rebuilt:** 427,770 bytes; artifact-sha256
`a8f3114803c632cb4efc0bbb40a1e1909b764bfad7eb384cbe56a45b5250baf6`; source-fingerprint
`972836ed8ba672668fab057293b528301f522c907562bb5a138992bccf601792` (58 inputs). `check:wasm`
reports it matches its manifest and the source that is here now.

**The prose, corrected in the same wave.** Spec §6.6 now states both departures where it states
the parameters — the 3 km corridor (`earth_like` ships 1,500 m, Ruling S-17) and the 500 km²
density cap (`earth_like` ships 1.6e10 m², 16,000 km², Ruling S-16) — each pointing at the
verification report's departures section; and §6.6's Ruling S-12 cost sentence now says the
pre-check answers **both** the wetness and the coarse-body question from one midpoint, so its
granularity is a whole coarse segment. `tests/wasm_exports.rs` asserted `len >= 45` with a comment
naming schema 4; the header has been **54** words since Task 5, so the assertion pinned nothing —
and it is the only header assertion that crosses the `extern "C"` boundary, which is why the
constraints file now lists **four** record-layout twins, not three. `water-preview.js` and
`engine.js` now say that `ponds_found` counts hollows in the corridors the search sampled, a 65%
step under Ruling S-12. `ponds::ring_is_simple`'s doc now says **transversal**: a vertex on a
segment, or a collinear overlap, passes it.

The reproduction commands are the ones above, with `--expect-passed <786|786|788|892|894>` and
`--expect-ignored 7`.

## 2026-09-12, water 1b-4 Task 6: the extent surveyed, and every pin re-derived by running it

Plan 1b-4 (automatic water, body extents) puts a **body extent** on the wire: every kept coarse
body now records its shore members and its collar, plus `shore_reach_m`. Ponds are unchanged
(Ruling E-6). The record went to **SCHEMA 6** — a 56-word header and a 16-word body prefix. This
section is the measurement half; the verification report has the argument.

### The extent's share, and how it is counted

Two figures, because they answer two questions. **Points** is `2 × (shore_members +
collar_points) × 8` bytes — the `(lat, lon)` pair every recorded extent point costs, which is
exactly the measured growth of `params`, `junction_params` and `ranges` in Tasks 1 and 2.
**Total** adds SCHEMA 6's fixed words: the two header words and the two per-body words, i.e.
`(2 + 2 × bodies) × 8` more. A pond's traced outline is **not** counted — it predates this plan
and is not the extent. `shore_reach_m`'s largest and median are over the **coarse** bodies alone,
those with `shore_member_count > 0`; ponds are excluded because Ruling E-6 fixes theirs at 0.0, so
including them would only measure how many ponds a bake found.

### The three stand-ins at 1,000,000 nodes

`./target/release/hydro_survey.exe`, no flags, `HydroParams::earth_like(1_000_000)`.
`drainage_check` is `Ok` on all three. **No body on any stand-in is missing a collar, and no kept hollow recorded no extent at all** — the second figure is the one the first is blind to (a kept hollow with no extent has `shore_member_count == 0`, never enters the coarse population, and would leave the no-collar count printing a reassuring 0), so `BakeStats::kept` minus the coarse bodies is printed beside it.

| world | bake | (stages / record_of / refine / ponds) | record | coarse bodies | shore members | collar points | extent: points / total | share | `shore_reach_m` largest / median | no collar | kept with no extent |
|---|---|---|---|---|---|---|---|---|---|---|---|
| plain | 46.05 s | 9.82 / 0.03 / 1.83 / 34.37 | **4,281,864** (was 4,241,672) | 23 | 968 | 1,247 | 35,440 / **40,192** | 0.94% | 43,908.3 / 39,402.6 m | **0** | **0** |
| owner_survey | 26.95 s | 10.87 / 0.02 / 0.80 / 15.25 | **2,511,928** (was 2,461,400) | 71 | 1,159 | 1,870 | 48,464 / **50,528** | 2.01% | 30,129.4 / 27,186.9 m | **0** | **0** |
| seed1_ranges | 56.87 s | 10.30 / 0.04 / 2.84 / 43.69 | **6,098,912** (was 5,974,160) | 160 | 2,764 | 4,368 | 114,112 / **124,752** | 2.05% | 43,397.7 / 39,549.3 m | **0** | **0** |

**Each record grew by exactly its own extent total** — +40,192, +50,528, +124,752 against the
1b-3 figures — so the accounting above is not a model fitted to the growth, it *is* the growth.

**The extent's points half reproduces the design note's own per-world estimate to three decimal
places**: 0.035 / 0.048 / 0.114 MB predicted in §5.1's "B trimmed" column, 0.0354 / 0.0485 /
0.1141 MB measured here. The note's probe and this survey agree, which is the strongest available
check on the 229 KB owner-world figure Task 4 measures — that figure is a scaling of the same
quantity from 65 bodies to 348.

**Both gates hold with room and no lever was pulled.** The worst record clears 8,000,000 by
**1,901,088 bytes (23.8%)** and the worst bake is **56.87 s of 120 s (47%)**.
`pond_density_area_m2` stays at 1.6e10.

**The extent costs no measurable time.** `extent_of` runs inside `record_of`, which is
0.03 / 0.02 / 0.04 s — unmoved from 1b-3's 0.03 / 0.02 / 0.06. The whole-bake times are 6.1 / 2.3
/ 3.7 s above 1b-3's, and the movement is in `ponds` and `bake_stages`, which this plan does not
touch (the pond part alone moved +4.84 s on `plain`); it is host variance, not the extent. A
second run of the same binary on the same worlds, minutes later, measured **49.34 / 28.80 /
61.95 s** — up to 9% above the first run — with every recorded quantity **bit-identical**. On this
host the timings wander and the record does not.

**Task 5's pond dedup fix moved no pond count on the stand-ins.** Found/kept are 1,993 / 273,
409 / 57 and 3,752 / 504 — byte-for-byte the 1b-3 figures. The owner's world may still move; Task
4 is where that would show.

### Pins, re-derived by running them

**Wasm rebuilt:** 431,385 bytes, 31 exports, 0 imports; artifact-sha256
`ab858cd2dfe0627f2725f3f0bc6bf302b79e15769c6fc23ea5741bdb56dac93a` — **identical**, because a
`[[bin]]` is not compiled into the artifact — and source-fingerprint
`3eb47b9b74f1df1f1d905f49aaddaddff6e398df39161e6df74d5f5b66202d6d` (66 inputs, was
`5efa9073…`, then `11e0aba0…` before the review fix wave's two survey edits). The CRLF guard (`git ls-files --eol crates/worldbuilder-engine | grep -v "w/lf"`)
printed nothing before the rebuild. `npm run check:wasm` reports it matches its manifest and the
source that is here now.

**Engine — moved, +13 run and +1 ignored uniformly.** Re-derived per configuration through
`cargo test -p worldbuilder-engine <cfg> -- --list` (and `--ignored`) and `assert_counts.py
cargo-list`; all five printed `count OK`:

| configuration | listed | ignored | **run** |
|---|---|---|---|
| `--no-default-features` | 807 | 8 | **799** (was 786) |
| default | 807 | 8 | **799** (was 786) |
| `--features python` | 809 | 8 | **801** (was 788) |
| `--features wasm` | 913 | 8 | **905** (was 892) |
| `--features python,wasm` | 915 | 8 | **907** (was 894) |

The suites were run, all five, `--no-fail-fast`: **780 + 4 + 9 + 6 = 799** passed / 0 failed / 8
ignored under `--no-default-features` and default; **782 + 4 + 9 + 6 = 801** with `python`; and
**+106 `wasm_exports`** on the two wasm rows, for 905 and 907. `no_std_math` is **6/6** in every
configuration.

Nothing is wasm-gated: `tests/wasm_exports.rs` gained no test — its only edit is the header
assertion SCHEMA 6 moves from 54 to 56 words — so the two wasm rows move by the same +13 as the
other three. **`expect_ignored` moves 7 → 8**, the first time in this file's history: the eighth
is `hydrology::extent_tests::the_trim_holds_at_a_million_nodes`, the design note's §5.7 trial,
`#[ignore]`d for the reason the other two sweeps are. `assert_counts.py` still reports
**seventeen** test binaries — this plan added no `[[bin]]`, no `tests/` file and no example.

**Both ignored sweeps pass.** `every_small_world_drains` in **76.31 s**;
`refinement_adds_no_crossings_at_1m` in **128.18 s**, printing the same `ranges 1M: 5545 reaches,
coarse 54 shipped 50` and `default 1M: 4285 reaches, coarse 33 shipped 28` — so the extent moved
no line. The third `#[ignore]`d test, `extent_tests::the_trim_holds_at_a_million_nodes`, is Task
3's own trial and its evidence is Task 3's; it is named here rather than silently skipped.

**Python:** 565 collected, 157 of them conformance — **unchanged** (`pytest --collect-only -q
tests` and the same over `tests/test_conformance.py`; the plan touched no Python).

**Viewer:** `npm test` (Node's test runner, `viewer/`, this host) — **338 pass, 0 fail**, run
here as a gate. The 337 → 338 is **not this task's**: Task 1 added it at `2b579aa`, when SCHEMA 6
put the discriminator on the wire, and `68b9a50` later fixed two of the file's assertions that
Task 2's extent fill had made stale. Task 6 only re-ran the suite to confirm it is green at the
pins above.

**Parity — moved:**

| | compared | divergent |
|---|---|---|
| `parity` | **150,830** (was 147,553) | **0** |
| `--mutate seed` | 150,830 | **145,274** (was 141,765) |
| `--mutate erosion-k` | 150,830 | 216 |
| `--mutate water-pond` | 150,830 | 60 |
| `--mutate tectonic-warp` | 150,830 | **22,993** (was 21,783) |
| `--mutate coast-amplitude` | 150,830 | 13,128 |
| `--mutate gully-steer` | 150,830 | 3,752 |
| `--mutate climate-samples` | 150,830 | 648 |

**The +3,277 is two separate changes and must not be read as one.** Both land in the hydro
records and nothing else moves, which is what made it easy to run them together:

- **+1,428 is SCHEMA 6's extent** (Tasks 1 and 2), taking the branch 147,553 → **148,981**:
  `hydro/ranges` 15,945 → **17,099** (+1,154) and `hydro/plain` 3,949 → **4,223** (+274).
- **+1,849 is Task 4's corpus change**, all in `hydro/plain`: 4,223 → **6,072**, from raising
  `parity_dump.rs`'s `HYDRO_PARAMS[0]` from 12,000 to 20,000 nodes so that record keeps ponds
  again. A bigger bake, not a bigger body layout.

**The `2 + 2 × bodies + 2 × (shore members + collar points)` identity applies to `hydro/ranges`
alone**, the record Task 4 does not touch. It is always even, and `hydro/plain`'s total delta is
**+2,123, odd** — because it is +274 of extent plus +1,849 of extra bake. Any reading that applies
the identity to `hydro/plain`'s total is wrong on parity before anything is measured.

Every non-H group is byte-for-byte unmoved in the plain run and in all seven controls. **0 divergent is the claim that matters here:** the
extent is computed identically natively and in the browser, so `extent_of` carries no platform
libm and no map iteration order — which is what §14.1's determinism requirement asks of it.

Under the seed control `hydro/ranges` is 17,047 of 17,099 and `hydro/plain` 6,019 of 6,072, and
the shortfall in each is the params echo a moved seed cannot move. The control's own +3,509 spans
both causes above, not the extent alone; what it says about the extent is that its new words are
seed-sensitive, as they must be — a different seed puts a different lake in a different place, so
every shore point moves. The tectonic control's `hydro/ranges` goes 15,597 →
**16,807 of 17,099**, its native `TCTL` prediction moved with it, and it again printed *"exactly
as the native side predicted"*; the five belt groups are unchanged at 6,186, which says the extent
adds no coupling of its own.

The reproduction commands are the ones above, with `--expect-passed <799|799|801|905|907>` and
`--expect-ignored 8`.

## 2026-09-12, water 1b-4 whole-branch review fix wave: two decode guards, and the pins with them

The whole-branch review re-derived every extent independently across 42 bakes and found the
geometry correct. It asked for two Importants and four Minors before merge, and **this wave
changes no geometry**: `extent.rs`, the crossing pass and `ponds.rs` are untouched, and no record
this branch writes is different by a byte.

**What moved.** The spec's §7 still said, in bold, that `kind` is the discriminator and
`shore_member_count` is never it — the exact sentence Ruling E-8 overturned, and the one a stage 2
implementer reading §7 top-down would hit first. §7 now leads with `shore_member_count` and keeps
`kind` as what the water *is*, matching §8.3. The other Important is a trust-boundary hole:
`decode` validated `shore_member_count` as a u32 and never against the `outline_len` it reads two
words later, so a record claiming one more shore member than it has points decoded happily and
then panicked the first reader on `body.outline[..shore_member_count]` — and wrapped
`outline.len() - shore_member_count` to 4,294,967,295 in release. `shore_reach_m`, the one float
on the wire used as a containment radius, took NaN, ±∞ and any negative just as happily; an
infinite band makes §8.3's clause 2 admit the whole planet as inside that lake. Both are refused
now, in `record.rs::decode` and in its `water-preview.js::decodeHydro` twin, with a test each side.

**One finding of its own, from Minor 6.** Adding the asked-for `assert_eq!` on `collar_points`
beside the existing one on `shore_members` went red: 101 against 269 on the `params` population.
The stat is right and the naive sum is wrong. `bake.rs` totals both halves **before**
`ponds::search` appends a pond, so they count the coarse bodies only — and "outline length less
the members" on a pond is its whole traced 250 m ring, which is not a collar. The Rust assertion
totals the coarse prefix; the viewer's existing all-body collar identity, which held only because
its 12,000-node bake keeps no pond, now asserts `pondsKept === 0` beside it rather than relying on
it silently. The two rewritten docstrings say which bodies the totals count.

### Pins, re-derived by running them

**Wasm rebuilt, and the artifact is byte-identical.** artifact-sha256
`ab858cd2dfe0627f2725f3f0bc6bf302b79e15769c6fc23ea5741bdb56dac93a` — **unchanged**, because the
only `src/` edits that reach the compiled library are two `decode` guards on paths the browser's
bake never takes and a doc comment. Only the source fingerprint moved,
`3eb47b9b74f1df1f1d905f49aaddaddff6e398df39161e6df74d5f5b66202d6d` →
`54f8b7577a20cd9b7cbb749e480d41809b1756540ad3ded4eb50c7dd502574a9` (66 inputs), because the
fingerprint hashes the crate's sources and `bake_tests.rs` is one of them. The CRLF guard
(`git ls-files --eol crates/worldbuilder-engine | grep -v "w/lf"`) printed nothing before each of
the two rebuilds. `npm run check:wasm` reports it matches its manifest and the source that is here
now.

**Engine — moved, +2 run uniformly, ignored unchanged.** Re-derived per configuration through
`cargo test -p worldbuilder-engine <cfg> -- --list` (and `--ignored`) and `assert_counts.py
cargo-list` after the last source edit; all five printed `count OK`:

| configuration | listed | ignored | **run** |
|---|---|---|---|
| `--no-default-features` | 809 | 8 | **801** (was 799) |
| default | 809 | 8 | **801** (was 799) |
| `--features python` | 811 | 8 | **803** (was 801) |
| `--features wasm` | 915 | 8 | **907** (was 905) |
| `--features python,wasm` | 917 | 8 | **909** (was 907) |

The suites were run, all five, `--no-fail-fast`, and all five are green. **Both new tests are
`record.rs` unit tests** — `decode_refuses_a_body_claiming_more_shore_members_than_it_has_outline_points`
and `decode_refuses_a_non_finite_or_negative_shore_reach` — which is why the +2 lands uniformly
and the two wasm rows move by the same +2 as the other three. `assert_counts.py` still reports
**seventeen** test binaries. `no_std_math` is **6/6**.

**Both ignored sweeps pass.** `every_small_world_drains` in **71.33 s**;
`refinement_adds_no_crossings_at_1m` in **121.17 s**, printing the same `ranges 1M: 5545 reaches,
coarse 54 shipped 50` and `default 1M: 4285 reaches, coarse 33 shipped 28` as Task 6 — so nothing
in this wave moved a line.

**Python:** 565 collected, 157 of them conformance — **unchanged** (`pytest --collect-only -q
tests` and the same over `tests/test_conformance.py`; this wave touched no Python).

**Viewer:** `npm test` (Node's test runner, `viewer/`, this host) — **340 pass, 0 fail** (was
338). The +2 is this wave's: the JS twin of each decode guard, in
`viewer/test/water-preview.test.mjs`.

**Parity — unmoved, which is the claim.** `parity: 150830 values compared through the shipped
exports, 0 divergent`; `--mutate seed` **145,274 of 150,830**, both exactly Task 6's numbers. A
decode guard changes no record, so the corpus is identical group for group — `hydro/ranges` 17,099
and `hydro/plain` 6,072, unchanged.

The reproduction commands are Task 6's, with `--expect-passed <801|801|803|907|909>` and
`--expect-ignored 8`.
