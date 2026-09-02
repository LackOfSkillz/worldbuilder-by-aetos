# Slice 1g: `margins_within` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port `margins_within` from `worldbuilder/plates/lookup.py` to `crates/worldbuilder-engine/src/plates.rs`, reproducing the three bug-fixes encoded in it and establishing, by measurement, whether its result-list membership is reproducible across implementations.

**Architecture:** One function, 104 lines, a nested loop. It returns every margin of a point's plate that is near enough to matter, each with a weight that fades where a third plate shadows it. It exists because picking *one* margin is not continuous even when its distance is — so it returns all of them and lets the caller sum.

**Tech Stack:** Rust (engine), PyO3 bindings, pytest differential harness against the Python reference.

**Spec:** `docs/design/2026-09-02-mark-2-world-studio.md`, sections 4.1 (bit-equality), 4.2 (DETERMINISM-001), 6 (VERSION-001).

## Global Constraints

- **Two conformance contracts, chosen per code path, not per function.** Strict bit-for-bit where no transcendental is in the path; bounded at `MAX_TRANSCENDENTAL_ULPS = 4` where one is.
- **All float maths through `detmath`** (libm-backed). No `f64::` method or associated-function form, no `mul_add`, no bare integer casts without a `// cast-ok: <reason>` marker. A build-failing guard test enforces this.
- **Constants transcribed character-for-character** from the Python, including how they are written. `SHADOW_BLEND` is `0.02` — a plain decimal literal, no underscores, no exponent form.
- **Never `f64::min` / `f64::max` / `clamp`.** Python's two-argument `min`/`max` are asymmetric and position-dependent under NaN; Rust's are commutative and NaN-avoiding. Every clamp is written as an explicit `if`/`else` reproducing the Python's comparison direction and operand order.
- **The bisector table and the seed table are addressed by POSITION on every axis.** This is a settled ruling from slice 1f, concurred with by review and commented at `plates.rs:161`. See "The indexing rule" below.
- **Nothing under `worldbuilder/` may be modified.** The Python remains the reference. `worldbuilder/integration/maritime.py` has a pre-existing uncommitted change; leave it unstaged.
- **A floating-point sign, zero, or exact-equality assertion without the reason it holds is a latent bug.** Four have occurred in this port. Establish why each holds, or do not assert it.

---

## The hazard that defines this slice

**Read this before Task 1. It changes what the conformance harness is allowed to claim.**

Every earlier slice could say where its strict contract applied by asking "is there a transcendental in this code path?" For `margins_within` the answer is subtler than it first appears, and getting it wrong would produce a conformance suite that passes while proving less than it claims.

Line 217 computes the range threshold:

```python
limit = math.sin(min(math.pi / 2, range_m / radius_m))
```

Line 230 then makes the membership decision:

```python
if offset > limit:
    continue
```

`offset` is algebraic — a dot product and an `abs`, exactly reproducible. But `limit` comes from `math.sin`, and CPython's `sin` is the platform libm while the engine's is the pure-Rust `libm` crate. **The membership decision is therefore a discrete choice tested against a transcendentally-derived threshold.** If the two `sin` implementations disagree by one ULP, a candidate whose `offset` falls between the two values of `limit` is included by one implementation and excluded by the other — and the two result lists differ in *length*, not merely in a low-order bit.

This is the same family as slice 1d's calibration quantile and slice 1e's neighbour selection, but it lands somewhere new: in `margin_at` the selection was genuinely strict because no transcendental was anywhere in its path. Here one is, upstream of every membership decision the function makes.

**Task 1 settles this by measurement before any dependent code is written**, because the answer determines what Task 4's harness may assert. There are two possible outcomes and both are acceptable results:

- `limit` is **bit-identical** between the two implementations for the range values under test. Then membership is exactly reproducible and the harness compares lists strictly, by length and by position. Say so with the evidence.
- `limit` **differs**. Then membership is reproducible only where no candidate sits within that difference of the boundary, and the harness must measure the closest approach and report it. A slice that claims strict membership without checking has claimed something it did not establish.

Do not assume the first outcome. Measure it.

---

## The indexing rule

The Python addresses its tables three different ways inside this one function:

```python
zip(self.plates, self._bisector_xyz[nearest.index])   # line 223: row by .index, column by loop position
here = seeds[nearest.index]                           # line 258: seed row by .index
self._bisectors[nearest.index][index]                 # line 281: row by .index, column by loop position
```

and excludes plates by index equality:

```python
if third.index == nearest.index or third.index == other.index:   # line 262
```

**The Rust uses position consistently everywhere.** `PlateSet::new` addresses its tables by position and does not enforce `index == position`; the two coincide only because `generation.py` assigns `index=index for index in range(count)`. This was ruled on in slice 1f, concurred with by review, and is documented at `plates.rs:161`, in the engine README, and asserted in the conformance fixture.

Apply the same rule here, at all four sites, and do not add a fresh comment block at each one — reference the existing explanation rather than restating it four times.

---

## The three encoded bugs

The port must reproduce the **fixed** behaviour. A port that quietly reverts one would still look correct and still pass a careless review, so each has its own task step and its own test.

**Bug 1 — the arg-min flip (the function's whole reason to exist).** Picking one margin is not continuous even when its distance is: at a point equidistant from two of a plate's margins, which one is "the" margin flips under a step of a metre, and the normal, the relative motion and what lies either side flip with it. Terrain built on that gained five hundred metres of cliff. The fix is to return *all* margins in range and let the caller sum, because each contribution depends on its own distance and fades at its own range.

**Bug 2 — the phantom bisector.** Two seeds always have a bisector, but it is only part of the cell boundary where those two are genuinely the nearest pair; elsewhere it runs through a third plate's territory, imaginary. Summing those cost a hundred and seventy kilometres of phantom mountain range, and was discontinuous: crossing from plate 5 to plate 0 swapped `bisector(5,8)` for `bisector(0,8)` — different planes with no reason to agree — for two hundred and sixty metres of cliff. The fix stands at the closest point on the bisector and asks who the neighbours are there.

**Bug 3 — the shadow weight that replaced a boolean.** The first version rejected shadowed bisectors with a boolean, switching a margin on and off in one step wherever it ended at a triple junction: a hundred and forty metres of cliff. The Python's own comment calls this "the third time the same mistake appeared in this phase: a hard decision taken on a continuous quantity." It fades now, through `SHADOW_BLEND` and a smoothstep.

**One hard exit remains and it is correct — do not report it as a fourth bug.** Line 272's `if genuine <= 0.0: continue` is a discrete test on a continuous quantity, but the smoothstep is exactly zero there, so a skipped candidate and an included candidate of weight zero are indistinguishable to any summing caller. It is an optimisation, not a discontinuity. Note it in the code so a later reviewer does not "fix" it.

---

## File Structure

- **Modify** `crates/worldbuilder-engine/src/plates.rs` — `SHADOW_BLEND`, a `NearbyMargin` struct, and `margins_within`.
- **Modify** `crates/worldbuilder-engine/src/bindings.rs` and `src/lib.rs` — one binding.
- **Modify** `tests/test_conformance.py` — the differential tests.
- **Modify** `crates/worldbuilder-engine/README.md` — the record.

---

### Task 1: Settle the membership contract by measurement

**Files:**
- Create: `tests/test_limit_ulps.py` (throwaway; deleted in Task 5)

**Interfaces:**
- Produces: a recorded answer — whether `limit` is bit-identical across implementations — that Tasks 2 and 4 depend on.

This task writes no engine code. It answers the question in "The hazard that defines this slice" so that everything after it is built on a measured fact.

- [ ] **Step 1: Compute `limit` both ways and compare bits.**

For a spread of `range_m` values spanning the realistic domain (say `1e3`, `1e4`, `5e4`, `1e5`, `5e5`, `1e6`, `2e6`, `5e6`, and the saturating case `range_m > radius_m * pi / 2`), compute in Python:

```python
limit_py = math.sin(min(math.pi / 2, range_m / radius_m))
```

and in Rust, through `detmath`, the identical expression with the identical operand order. Compare with the existing `bits` helper from `tests/test_conformance.py`. Report, for each `range_m`, whether the two are bit-identical and if not, the ULP distance.

- [ ] **Step 2: Measure the closest approach to the boundary.**

Over the existing `corpus()`, for a representative `range_m`, record the minimum of `abs(offset - limit)` across every candidate considered — that is, how close any candidate comes to flipping membership. Report it, and report the ULP magnitude of `limit` at that value, so the two can be compared directly.

- [ ] **Step 3: Record the answer in the ledger, with the numbers.**

State which of the two outcomes holds, with the evidence. If `limit` is bit-identical everywhere tested, Task 4 compares membership strictly. If it is not, Task 4 must compare membership with the measured margin of safety stated, and any candidate closer to the boundary than the `limit` discrepancy is a genuine divergence to report rather than to tolerate.

**Do not proceed to Task 2 until this is recorded.**

---

### Task 2: The constant, the struct, and the candidate loop

**Files:**
- Modify: `crates/worldbuilder-engine/src/plates.rs`

**Interfaces:**
- Produces: `pub const SHADOW_BLEND: f64 = 0.02;`
- Produces: `pub struct NearbyMargin { pub other: Plate, pub distance_m: f64, pub normal: Vec3, pub weight: f64 }`
- Produces: `pub fn margins_within(&self, point: &SpherePoint, range_m: f64, radius_m: f64) -> (Option<Plate>, Vec<NearbyMargin>)`
- Consumes: `nearest_two`, `bisector`, `DEGENERATE`, `Vec3`, `detmath`.

`radius_m` is a required parameter, not an emulated default — matching `margin_at`'s existing Rust signature.

- [ ] **Step 1: Write the failing tests.**

**Do not invent a `Plate` literal.** `plates.rs` already has test constructors from slices 1e and 1f — read them and use them. `Plate` is not known to implement `Default`, and a plan that assumes it would send you down a compile-error path. Add one shared helper alongside the existing ones:

```rust
#[cfg(test)]
fn three_plate_set() -> PlateSet {
    // Two seeds on the equator and one lifted off it. The third seed must NOT lie on
    // the great circle bisecting the other two - see the note below, which cost this
    // plan a rewrite.
    PlateSet::new(vec![
        test_plate(0, 0.0, 0.0),
        test_plate(1, 0.0, 90.0),
        test_plate(2, 60.0, 45.0),
    ])
}
```

**Why the third seed is off the equator, because the obvious arrangement does not work.** The natural choice is seeds at 0, 45 and 90 degrees along the equator. It is useless for testing the fade. The bisector of the outer two is the great circle through 45 degrees, the middle seed sits exactly on it, and the shadow along that circle works out to `-0.293 * cos(phi)` — negative everywhere, reaching zero only at the pole. The weight is therefore zero at every sample, and a fade test on that set records two hundred identical zeros, computes a largest-step change of `0.0`, and passes while testing nothing.

With the third seed at 60N 45E the shadow genuinely changes sign: on the bisector of plates 0 and 1 it is about `+0.207` at the equator and about `-0.214` by 27 degrees north, so the crossing — and the whole fade — lies inside a path the test can walk.

where `test_plate(index, lat, lon)` is whatever the existing helpers already provide — match their name and signature rather than adding a second convention. Poles and rates must be non-degenerate and differ from the seeds, per the binding contract fixed in slice 1f.

```rust
#[test]
fn a_single_plate_set_has_no_margins() {
    let set = PlateSet::new(vec![test_plate(0, 0.0, 0.0)]);
    let (nearest, found) = set.margins_within(
        &SpherePoint::from_latlon(10.0, 10.0), 1.0e6, EARTH_RADIUS_M);
    assert!(nearest.is_some(), "one plate still owns every point");
    assert!(found.is_empty(), "fewer than two plates means no margin can exist");
}

#[test]
fn a_plate_interior_finds_nothing_in_a_short_range() {
    // Standing on seed 0, the nearer bisector is the one with seed 2, roughly
    // 35 degrees away - about 3,900 km. A 1,000 km range must reject both on
    // the range test alone, before any shadow work is done.
    let set = three_plate_set();
    let (_, found) = set.margins_within(
        &SpherePoint::from_latlon(0.0, 0.0), 1.0e6, EARTH_RADIUS_M);
    assert!(found.is_empty(), "every candidate is beyond the range limit");
}

#[test]
fn a_wide_range_admits_every_candidate_bisector() {
    // A range spanning the planet admits both of plate 0's candidate bisectors.
    // This is the pre-shadow count; Task 3 re-verifies it once shadowing exists.
    let set = three_plate_set();
    let (_, found) = set.margins_within(
        &SpherePoint::from_latlon(0.0, 0.0), 2.0e7, EARTH_RADIUS_M);
    assert_eq!(found.len(), 2, "both bisectors are in range before shadowing");
}
```

**The third test changes in Task 3** — that is intended, and Task 3 says so. Update it there rather than weakening it here.

- [ ] **Step 2: Run them and confirm they fail** for the expected reason (the function does not exist), not for a compile error elsewhere.

- [ ] **Step 3: Implement the early exit, the limit, and the candidate loop.**

Transcribe lines 212-231. Three things must be written deliberately:

The **saturating clamp** at line 217 is Python's two-argument `min`, which keeps the first operand unless the second is strictly below it:

```rust
// Python writes `min(math.pi / 2, range_m / radius_m)`; two-argument min keeps
// pi/2 unless the ratio is strictly below it, so a NaN ratio saturates to pi/2
// rather than propagating. Not f64::min, which would return the ratio.
let ratio = range_m / radius_m;
let angle = if ratio < core::f64::consts::FRAC_PI_2 { ratio } else { core::f64::consts::FRAC_PI_2 };
let limit = detmath::sin(angle);
```

Verify `core::f64::consts::FRAC_PI_2` is bit-identical to CPython's `math.pi / 2` before relying on it; if it is not, compute it as `PI / 2.0` exactly as the Python does.

The **`normal is None` skip** at line 225 is the self-bisector — a plate against itself. In Rust this is `bisector(near_pos, position)` returning `None`.

The **range test** is `if offset > limit { continue }` — strictly greater, so a candidate exactly at the limit is kept.

- [ ] **Step 4: Run the tests and confirm they pass.**

- [ ] **Step 5: Commit.**

---

### Task 3: The phantom-bisector test and the shadow weight

**Files:**
- Modify: `crates/worldbuilder-engine/src/plates.rs`

**Interfaces:**
- Consumes: everything from Task 2.

This task encodes bugs 2 and 3. Transcribe lines 233-283.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test]
fn a_bisector_running_through_a_third_plate_is_not_a_margin() {
    // Bug 2. Standing well north on plate 0, the bisector of plates 0 and 1 has
    // its foot in territory that plate 2 owns, so that bisector is imaginary
    // there and must not be returned. Derive the latitude from the set rather
    // than trusting this comment: find a point whose nearest plate is 0 and
    // where the 0-1 shadow is negative, and assert plate 1 is absent from the
    // result while the margin against plate 2 is present.
    let set = three_plate_set();
    let (nearest, found) = set.margins_within(
        &SpherePoint::from_latlon(20.0, 20.0), 2.0e7, EARTH_RADIUS_M);
    assert_eq!(nearest.expect("a plate owns this point").index, 0);
    let others: Vec<usize> = found.iter().map(|m| m.other.index).collect();
    assert!(
        !others.contains(&1),
        "the 0-1 bisector is shadowed by plate 2 here, so it is not a margin; got {others:?}",
    );
}

#[test]
fn a_shadowed_margin_fades_rather_than_switching_off() {
    // Bug 3, and the test that would catch a reversion to the boolean. Walking
    // north along longitude 20, the shadow that plate 2 casts on the 0-1 margin
    // goes from about +0.207 at the equator to about -0.214 by 27 degrees, so
    // the crossing lies inside this path. A boolean would step from 1.0 to
    // absent in a single sample; the fade must not.
    //
    // SHADOW_BLEND is 0.02, so the fade occupies a narrow band: the shadow moves
    // roughly 0.0021 per step here, which puts about ten samples inside it. If
    // your measured numbers differ, add samples rather than relaxing the bound.
    let set = three_plate_set();
    let mut weights = Vec::new();
    for step in 0..200 {
        let lat = (step as f64) * 0.125;  // cast-ok: loop counter to f64
        let (_, found) = set.margins_within(
            &SpherePoint::from_latlon(lat, 20.0), 2.0e7, EARTH_RADIUS_M);
        weights.push(found.iter().find(|m| m.other.index == 1).map_or(0.0, |m| m.weight));
    }
    let biggest = weights.windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0f64, |a, b| if b > a { b } else { a });
    assert!(
        biggest < 0.25,
        "the shadow weight must fade across neighbouring samples, not step; \
         largest single-step change was {biggest}",
    );
    assert!(
        weights.iter().any(|&w| w > 0.0) && weights.iter().any(|&w| w == 0.0),
        "the path must actually cross the shadow boundary, or the test proves nothing",
    );
}

#[test]
fn the_weight_never_leaves_zero_to_one() {
    // The smoothstep of a [0,1] clamp cannot leave [0,1]. Weak on its own, which
    // is why it is not the test that guards bug 3.
    let set = three_plate_set();
    let (_, found) = set.margins_within(
        &SpherePoint::from_latlon(0.0, 20.0), 2.0e7, EARTH_RADIUS_M);
    for margin in &found {
        assert!(margin.weight > 0.0 && margin.weight <= 1.0, "weight {} out of range", margin.weight);
    }
}
```

**The second test is the one that matters, and its second assertion is what stops it being vacuous.** Without it, a path that never crosses the shadow boundary at all would record 200 identical weights, report a largest change of zero, and pass while testing nothing — the same shape as the three vacuous tests this project has already shipped. Verify both the chosen latitude span and the `0.25` bound against what the code actually produces; if the fade is sharper than the bound, tighten the sampling rather than loosening the assertion.

Then **confirm the test can fail**: temporarily replace the smoothstep weight with the boolean it replaced (`1.0` when `shadow > 0.0`, skip otherwise), observe this test fail, and revert. Report what you observed. A test guarding a bug-fix that has never been seen to fail against the bug is not yet known to guard anything.

**Also re-verify `a_wide_range_admits_every_candidate_bisector` from Task 2.** Standing on seed 0, both candidate bisectors are expected to survive shadowing — the 0-1 bisector has its foot near 45 degrees east on the equator, where the shadow works out to roughly `+0.207`, comfortably genuine. Confirm that empirically rather than trusting this paragraph. If the count changes, update the assertion and say why in the commit; do not delete the test, and do not adjust it merely to make it pass.

- [ ] **Step 2: Run them and confirm they fail.**

- [ ] **Step 3: Implement the foot projection and the degeneracy guard** (lines 246-253). `reach` is a `sqrt`, algebraic and correctly rounded, so this path stays strict. `reach <= DEGENERATE` skips; the guard is defensive against dividing by a near-zero length.

- [ ] **Step 4: Implement the shadow loop** (lines 258-264). The running minimum is Python's two-argument `min` with the accumulator as the *first* operand:

```rust
// Python writes `shadow = min(shadow, candidate)`; the accumulator is the first
// operand, so a NaN candidate is ignored and leaves shadow unchanged, while a NaN
// accumulator would stick permanently. Not f64::min, which is commutative.
if candidate < shadow {
    shadow = candidate;
}
```

`shadow` starts at `2.0`. The exclusion at line 262 becomes a position comparison against `near_pos` and the candidate's own position, per the indexing rule.

- [ ] **Step 5: Implement the weight** (lines 271-274). Two nested clamps, `max` first then `min`, each explicit:

```rust
// Python writes `min(1.0, max(0.0, shadow / SHADOW_BLEND))`. max keeps 0.0 unless
// the value is strictly above it, so NaN clamps to 0.0; min then keeps that.
let scaled = shadow / SHADOW_BLEND;
let lifted = if scaled > 0.0 { scaled } else { 0.0 };
let mut genuine = if lifted < 1.0 { lifted } else { 1.0 };
if genuine <= 0.0 {
    // A hard exit on a continuous quantity, and deliberately safe: the smoothstep
    // below is exactly zero here, so a skipped candidate and an included one of
    // weight zero are indistinguishable to any summing caller. Not a fourth
    // instance of this module's recurring bug.
    continue;
}
genuine = genuine * genuine * (3.0 - 2.0 * genuine);
```

The smoothstep's operation order is load-bearing under a bit-for-bit contract. Transcribe it exactly: `genuine * genuine * (3.0 - 2.0 * genuine)`.

- [ ] **Step 6: Implement the push** (lines 278-283). The distance is `detmath::asin(min(1.0, offset)) * radius_m` with the same explicit two-argument-min treatment already used in `margin_at` — read that code and match it rather than writing a fourth variant. The normal is the `Vec3` from the bisector table, addressed by position.

- [ ] **Step 7: Run the tests and confirm they pass.** Report the result of the bug-3 reversion experiment from Step 1.

- [ ] **Step 8: Commit.**

---

### Task 4: Conformance

**Files:**
- Modify: `crates/worldbuilder-engine/src/bindings.rs`, `crates/worldbuilder-engine/src/lib.rs`, `tests/test_conformance.py`

**Interfaces:**
- `plateset_margins_within(seeds, poles, rates, x, y, z, range_m, radius_m) -> (Option<usize>, list[(usize, f64, (f64, f64, f64), f64)])`

**Apply the contract split, and apply Task 1's finding — do not overstate it.**

- **List membership and ordering**: governed by Task 1's measured answer. If `limit` was bit-identical, compare strictly — same length, same plate indices, same order. If it was not, compare with the measured margin of safety stated explicitly in the test, and treat any candidate closer to the boundary than the discrepancy as a divergence to report.
- **The plate indices** in each entry: exact integers.
- **The weight**: strict, `same()`. It is algebraic throughout — dot products, a division, two clamps and a polynomial.
- **The normal**: strict, `same()`, component-wise.
- **The distance**: `close_enough`, the `asin` bound.
- **The `nearest` plate**: exact integer, or `None` positionally.

Build the Python `PlateSet` with `index` equal to position, and reuse the existing fixture that already asserts this — do not write a second one.

Cover: the corpus against a multi-plate set; a single-plate set; ranges that select none, some, and all margins; points near a triple junction where the shadow weight is between 0 and 1; and points deliberately placed near the range boundary.

- [ ] **Step 1: Add the binding and register it.** Conversion only, no arithmetic.
- [ ] **Step 2: Rebuild** with `maturin develop --release` into the project venv.
- [ ] **Step 3: Add the conformance tests.**
- [ ] **Step 4: Measure and assert, do not merely print.** Record the minimum weight gap and the closest approach to the range boundary observed across the corpus, and **assert a floor on each with the observed value in the failure message**. A test whose only assertions are satisfied by degenerate values is the failure mode this project has shipped three times; the sine-gap test in slice 1f had to be fixed for exactly this.
- [ ] **Step 5: Run both suites**, quote them, and report every measured number.
- [ ] **Step 6: Commit.**

---

### Task 5: Record it

**Files:**
- Modify: `crates/worldbuilder-engine/README.md`
- Delete: `tests/test_limit_ulps.py`

- [ ] **Step 1: Record** — that `margins_within` is the first function whose *membership* decision sits downstream of a transcendental, and what Task 1 measured about it; the three bugs and how each is encoded; the one hard exit that is deliberately safe and why; that this function had **no test coverage of any kind** before this slice; and the measured floors from Task 4. Verify every test count by running the suites rather than copying a number from a report.

- [ ] **Step 2: Delete the throwaway** `tests/test_limit_ulps.py`. Its findings live in the README and the ledger; the file itself was a spike.

- [ ] **Step 3: Commit.**

---

## What this slice deliberately does not do

- **No kinematics.** `plates/kinematics.py` is the next slice, and it is where the fabrication guard flagged in 1e and 1f must finally be established — `angular_velocity` is the only function that reads `euler_pole` and `rate_rad_per_myr`, so no slice before it can test that the bindings carry them honestly.
- **No tectonics.** `terrain/tectonics.py` imports `ACROSS_ENOUGH` and `motion_between` from kinematics.
- **No plate generation.** `generation.py` needs `blake2b` over a UTF-8 joined string — a byte-level port feeding a cryptographic hash, and a new engine dependency.
- **No deletion of the Python.** It stays the reference.
