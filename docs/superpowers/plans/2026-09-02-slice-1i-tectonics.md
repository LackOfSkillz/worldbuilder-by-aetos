# Slice 1i: Tectonics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port `worldbuilder/terrain/tectonics.py` — what plate motion does to the ground — to `crates/worldbuilder-engine/src/tectonics.rs`, and establish by measurement whether its one hazardous branch is reproducible.

**Architecture:** A contribution, never an elevation. `offset_m` returns a number to *add* to the continental base, so shelves, erosion, bathymetry and detail can compose with tectonics later instead of reverse-engineering what it overwrote. It sums **every** margin in range rather than choosing the nearest, because choosing is not continuous even when the distance is.

**Tech Stack:** Rust (engine), PyO3 bindings, pytest differential harness against the Python reference.

**Spec:** `docs/design/2026-09-02-mark-2-world-studio.md`, sections 4.1 (bit-equality), 4.2 (DETERMINISM-001), 6 (VERSION-001).

## Global Constraints

- **Two conformance contracts, chosen per code path, not per function.** Strict bit-for-bit where no transcendental is in the path; bounded at `MAX_TRANSCENDENTAL_ULPS = 4` where one is.
- **All transcendentals through `detmath`** (libm-backed, `libm` pinned at `=0.2.11`). `detmath::hypot` and `detmath::tanh` already exist — use them. No `f64::` method or associated-function form, no `mul_add`, no bare integer casts without a `// cast-ok: <reason>` marker. A build-failing guard test enforces this.
- **`abs` is NOT routed through `detmath`.** It has no `detmath` entry, the guard test `the_guard_does_not_ban_abs` whitelists it, and `plates.rs:293` uses bare `.abs()`. It only clears the sign bit, so it is exact.
- **Constants transcribed character-for-character**, including underscore separators and trailing zeros. Rust accepts the same `420_000.0` grouping the Python uses, so transcribe literally. **A mis-grouped constant in an earlier slice was "verified" by two agents and caught only by a differential test.**
- **Never `f64::min` / `f64::max` / `clamp`.** Python's two-argument forms are asymmetric under NaN. Every clamp is an explicit `if`/`else` reproducing the Python's comparison direction and operand order.
- **Nothing under `worldbuilder/` may be modified.** The Python remains the reference. `worldbuilder/integration/maritime.py` has a pre-existing uncommitted change; leave it unstaged.
- **A floating-point sign, zero, or exact-equality assertion without the reason it holds is a latent bug.** Five have occurred in this port. Establish why each holds, or do not assert it.

---

## The hazard that defines this slice

**Read this before Task 1.** The module has exactly two transcendental calls — one `tanh` (line 162) and one `hypot` (line 284) — and no `sqrt` at all. The question is what discrete decisions sit downstream of them.

```python
speed = math.hypot(motion.closing_m_per_myr, motion.sliding_m_per_myr)   # 284
if speed <= 0.0:                                                          # 285
    return 0.0
across = motion.closing_m_per_myr / speed                                 # 287

engagement = (abs(across) - ACROSS_ENOUGH) / (1.0 - ACROSS_ENOUGH)        # 290
if engagement <= 0.0:                                                     # 291
    return 0.0
...
if across < 0.0:                                                          # 300
```

**`math.hypot` is not a normal transcendental for this port's purposes.** Since Python 3.8 CPython does *not* delegate it to the platform libm — it implements a scaled, overflow-safe vector norm with Neumaier summation in `mathmodule.c`. The engine uses `libm::hypot`. So these are **definitely different algorithms**, not merely two libm implementations that might differ. Neither is required to be correctly rounded the way `sqrt` is.

Three decisions sit downstream. **Two of them are safe, and the reasons are worth stating rather than assuming:**

- **Line 285, `speed <= 0.0`: safe.** `hypot` returns exactly zero if and only if both arguments are exactly zero, in any sane implementation, and both arguments are computed algebraically by `motion_between`. The branch fires on an exact condition that both implementations agree on. *Confirm this in Task 1 rather than trusting it.*
- **Line 300, `across < 0.0`: safe, and this one is easy to get wrong.** It looks like the most consequential decision in the file — it picks between the ridge/rift profile and the whole collision/subduction/trench/arc machinery. But `across = closing / speed`, `speed` is `hypot(...)` which is always non-negative, and line 285 has already returned if it were zero. **Dividing by a strictly positive quantity cannot change a sign**, so the sign of `across` is exactly the sign of `closing`, which `motion_between` computes algebraically. `hypot` is upstream of this branch but cannot influence it.
- **Line 291, `engagement <= 0.0`: THIS is the hazard.** It depends on the *magnitude* of `across`, not its sign. A one-ULP difference in `speed` moves `across` by about one ULP and `engagement` by about two, and the branch fires when `abs(across)` is at or below `ACROSS_ENOUGH`. A margin whose `abs(across)` sits within a few ULP of `0.5` could contribute in one implementation and not the other — and the difference is not a low-order bit in the output, it is a whole margin's profile appearing or vanishing.

**Task 1 measures this before any dependent code is written**, exactly as slice 1g did for its membership threshold. Both outcomes are acceptable results; what is not acceptable is assuming one.

**One more, nearly unreachable but not dead.** Line 297's `strength <= 0.0` can only fire if `engagement`'s smoothstep underflows to zero, which needs `engagement` below roughly `1e-162`. It is not dead code and must be ported; note in a comment why it is nearly unreachable so nobody deletes it.

---

## The bugs this module records, which are requirements

The file explains itself, and those explanations are the specification.

**The 550-metre cliff.** `CONTINENTAL_ENOUGH`/`CONTINENTAL_BLEND` exist because the first version used a hard test — continental if above zero — and *"the ground jumped five hundred and fifty metres wherever a margin crossed it, because the two sides of the test run entirely different profiles. It is the same mistake M1.2 made: a hard selection on a continuous quantity."* The blend is the fix; `_continental` is a smoothstep, not a threshold.

**The 419-kilometre mismapping, and it is the subtlest thing in the file.** The obvious way to place a point on one side of an asymmetric margin is `signed = distance * lean`. That is wrong: *"scaling the axis compresses distance, so with a lean of -0.22 a point four hundred and nineteen kilometres out mapped to -90 km, which is exactly where the trench sits. The trench fired at four hundred kilometres and the range gate then cut it off mid-profile."*

The fix keeps the distance true and blends two evaluations instead:

```python
toward = (1.0 + setting.lean) * 0.5
return strength * (
    toward * profile(distance_m) + (1.0 - toward) * profile(-distance_m)
)
```

**A port that "simplified" this back to a scaled axis would look tidier and reintroduce a measured bug.** Task 4 must carry a comment saying so.

**The 560-metre cliff that `offset_m` exists to avoid.** Summing every margin rather than choosing the nearest, because *"at a point equidistant from two of a plate's margins the choice flips under a step of a metre... Measured at five hundred and sixty metres of cliff, a hundred and thirty kilometres from any boundary, where one margin was transform and the other divergent."*

**The threshold that is deliberately not used.** `_from_margin` ignores `motion.kind` and recomputes a continuous `across` instead: *"`motion.kind` is a name given by a threshold, and using the name to pick a profile meant a margin drifting from convergent to transform went from a full mountain belt to nothing in one step. The name survives for diagnostics; the terrain uses the number."* Do not "simplify" by branching on `kind`.

---

## Two transcription traps

**The inline `70_000.0`.** The coastal-uplift term is `_bump(across_m - 70_000.0, COASTAL_UPLIFT_WIDTH_M)` — a bare literal, not a named constant, and it happens to equal `RIFT_WIDTH_M`. **Do not "tidy" it into `RIFT_WIDTH_M`**: they are unrelated quantities that coincide, and binding them would couple two profiles that must be free to change independently. Transcribe the literal.

**`profile` is a nested closure** capturing `collision`, `oceanic` and `subduction` from its enclosing scope. In Rust either a closure or a private function taking those three as parameters is fine — say which you chose and why, and keep the operation order of the returned sum exactly as written.

---

## File Structure

- **Create** `crates/worldbuilder-engine/src/tectonics.rs`, declared in `src/lib.rs`. The Python draws this line too, and `plates.rs` is already 780 lines.
- **Modify** `crates/worldbuilder-engine/src/bindings.rs` and `src/lib.rs` — the bindings.
- **Modify** `tests/test_conformance.py` — the differential tests.
- **Modify** `crates/worldbuilder-engine/README.md` — the record.

**Every dependency is already ported** — `TangentFrame::at` and `local_to_sphere`, `Continentality::at` and `base_elevation`, `PlateSet::margins_within` and `flattened`, `motion_between`, `ACROSS_ENOUGH`, `EARTH_RADIUS_M`. Verified by reading the crate. This slice adds no upstream gaps.

---

### Task 1: Measure the hazard before anything depends on it

**Files:**
- Create: `tests/test_hypot_ulps.py` (throwaway; deleted in Task 7)

This task writes no engine code. It answers whether the `engagement <= 0.0` branch is reproducible, and the answer decides what Task 6's harness may assert.

- [ ] **Step 1: Compare `hypot` bit-for-bit.**

For a spread of `(closing, sliding)` pairs spanning the realistic domain — including pairs where one component is zero, where they are equal, where they differ by many orders of magnitude, and the exactly-`(0.0, 0.0)` case — compute `math.hypot(x, y)` in Python and `detmath::hypot(x, y)` in Rust and compare with the existing `bits` helper from `tests/test_conformance.py`. Report bit-identity or the ULP distance for each.

**Expect them to differ**, and treat that as the interesting result rather than a failure. CPython implements its own vector norm; `libm` implements a different algorithm.

- [ ] **Step 2: Confirm the two branches that should be safe actually are.**

- `hypot(0.0, 0.0)` must be exactly `0.0` in both, and `hypot(x, y)` must be non-zero for any non-zero input in both. That is what makes line 285 safe.
- `hypot` must never return a negative in either. That is what makes line 300's sign test safe.

Report both as measurements, not assertions of the obvious.

- [ ] **Step 3: Measure how close anything gets to the engagement boundary.**

Over the existing `corpus()`, build a plate set and, for every margin within `MAX_TECTONIC_RANGE_M`, compute `abs(abs(across) - ACROSS_ENOUGH)` and record the **minimum**. Report it, and report the ULP magnitude of `across` at that point so the two can be compared directly. This is the margin of safety for the one hazardous branch.

- [ ] **Step 4: Do the same for `tanh`.** It feeds `lean`, which is a blend coefficient rather than a branch today — so a ULP difference perturbs an output rather than flipping a decision. Measure the ULP distance anyway and say which contract `lean` therefore falls under.

- [ ] **Step 5: Record the answer in the ledger with the numbers**, and state plainly what Task 6 may assert: strict comparison of the margin *set*, or a measured margin of safety. **Do not proceed to Task 2 until this is recorded.**

---

### Task 2: Constants, `_continental`, `_bump`, `Setting`

**Files:**
- Create: `crates/worldbuilder-engine/src/tectonics.rs`; modify `src/lib.rs`

**Interfaces:**
- Produces: every module constant, transcribed character-for-character.
- Produces: `fn continental(value: f64) -> f64`, `fn bump(distance_m: f64, width_m: f64) -> f64`
- Produces: `pub struct Setting { pub inboard: f64, pub outboard: f64 }` with `inboard_continental()`, `outboard_continental()` and `lean()`.

Both helpers end in the **same smoothstep** as slice 1g's shadow weight: `x * x * (3.0 - 2.0 * x)`. Transcribe the operation order exactly.

`_bump`'s docstring records why it is a smoothstep rather than a cosine or a taper: *"it is flat at both ends... A profile that merely reached zero would still leave a crease where it met the untouched ground, and a crease in terrain is a cliff somebody sails into."* Carry that reason.

Both contain two-argument clamps — `max(0.0, min(1.0, fraction))` and `min(1.0, abs(distance_m) / width_m)` — which must be explicit `if`/`else` in the Python's operand order, not `f64::min`/`max`.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test]
fn continental_saturates_at_both_ends_and_is_smooth_between() {
    // Thoroughly oceanic and thoroughly continental clamp to exactly 0 and 1;
    // the midpoint is exactly 0.5 because the smoothstep is symmetric about it.
    assert_eq!(continental(-10.0), 0.0);
    assert_eq!(continental(10.0), 1.0);
    assert_eq!(continental(CONTINENTAL_ENOUGH), 0.5);
}

#[test]
fn bump_is_one_at_the_centre_and_zero_at_the_edge() {
    assert_eq!(bump(0.0, 100_000.0), 1.0);
    assert_eq!(bump(100_000.0, 100_000.0), 0.0);
    assert_eq!(bump(-100_000.0, 100_000.0), 0.0);
    assert_eq!(bump(200_000.0, 100_000.0), 0.0);
}

#[test]
fn bump_has_zero_derivative_at_both_ends() {
    // The reason it is a smoothstep and not a taper: no crease where it meets
    // untouched ground. Sample either side of centre and edge; the change per
    // step must shrink towards both, not stay linear.
    let w = 100_000.0;
    let near_centre = bump(0.0, w) - bump(1_000.0, w);
    let mid_slope = bump(40_000.0, w) - bump(41_000.0, w);
    let near_edge = bump(99_000.0, w) - bump(100_000.0, w);
    assert!(near_centre < mid_slope, "must flatten towards the centre");
    assert!(near_edge < mid_slope, "must flatten towards the edge");
}

#[test]
fn a_zero_width_bump_is_nothing_rather_than_a_division_by_zero() {
    assert_eq!(bump(0.0, 0.0), 0.0);
    assert_eq!(bump(50.0, -1.0), 0.0);
}

#[test]
fn lean_is_zero_for_a_symmetric_margin_and_saturates_when_lopsided() {
    // Exactly zero when the two sides are alike -- tanh(0) is exactly 0.0 -- which
    // is what lets an asymmetric profile fade out rather than flip.
    assert_eq!(Setting { inboard: 0.3, outboard: 0.3 }.lean(), 0.0);
    assert!(Setting { inboard: 1.0, outboard: -1.0 }.lean() > 0.99);
    assert!(Setting { inboard: -1.0, outboard: 1.0 }.lean() < -0.99);
}
```

**The three exact assertions in the first two tests need their reasons checked, not assumed.** `continental(CONTINENTAL_ENOUGH)` should be exactly `0.5` because the fraction is exactly `0.5` and the smoothstep of `0.5` is `0.5 * 0.5 * (3.0 - 1.0)` — verify that evaluates exactly. `bump` at the edge should be exactly `0.0` because `away` clamps to exactly `1.0` and `fade` is then exactly `0.0`. **If any is not exact, report the measured value rather than loosening the assertion** — an exact assertion that turns out inexact is a finding about the arithmetic, and this project has twice discovered something interesting that way.

- [ ] **Step 2: Run them and confirm they fail** for the expected reason.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run and confirm they pass, then the whole crate suite.**
- [ ] **Step 5: Commit.**

---

### Task 3: `Tectonics` and `setting_at`

**Files:** Modify `crates/worldbuilder-engine/src/tectonics.rs`

**Interfaces:**
- Produces: `pub struct Tectonics { plates: PlateSet, land: Continentality, radius_m: f64 }` with a constructor.
- Produces: `pub fn setting_at(&self, point: &SpherePoint, distance_m: f64, normal: &Vec3) -> Setting`

The probes are placed relative to the **margin**, not the point — `to_inboard = -distance_m + PROBE_M`, `to_outboard = -distance_m - PROBE_M` — and the docstring says why: *"so that two samples on opposite sides of the same boundary describe the same stretch of it and agree about what it is. Probing outward from each point instead would have let a margin be a subduction zone from one side and a collision from the other."* Carry that reason.

- [ ] **Step 1: Write the failing tests.** Assert that two points on opposite sides of the same margin, at the same distance, produce settings describing the same stretch — that is the property the design exists for. Derive the geometry from `three_plate_set()` and state how you chose the points.
- [ ] **Step 2-5:** Run, implement, run, commit.

---

### Task 4: `_from_margin` — the core, the hazard, and the blend

**Files:** Modify `crates/worldbuilder-engine/src/tectonics.rs`

**Interfaces:**
- Produces: `fn from_margin(&self, point: &SpherePoint, near: &Plate, far: &Plate, distance_m: f64, normal: &Vec3) -> f64`

Transcribe lines 259-340. Three things must be written deliberately:

**The three early exits**, in order, with comments recording which are safe and why (see "The hazard that defines this slice"): `speed <= 0.0` is safe because `hypot` is zero only for exactly-zero inputs; `across < 0.0` is safe because dividing by a positive cannot change a sign; `engagement <= 0.0` is the one that depends on `hypot`'s precision, and Task 1's measurement says how much room it has. `strength <= 0.0` is nearly unreachable — reachable only via smoothstep underflow — and must be kept with a comment saying so.

**`motion.kind` must not be used.** The file recomputes a continuous `across` precisely to avoid it. A comment should say so, because branching on the classification is the obvious simplification and it is a documented bug.

**The two-sided blend must not be "simplified" to a scaled axis.** `toward * profile(distance_m) + (1.0 - toward) * profile(-distance_m)`, with a comment recording the 419-kilometre mismapping that `signed = distance * lean` caused.

- [ ] **Step 1: Write the failing tests.**

The regression test for the 419-kilometre bug is **exactly derivable**, and it is the most valuable test in this slice.

At a distance of 419 km every profile term is already outside its own width, so each `bump` clamps to zero and the contribution is **exactly `0.0`** — whatever the lean. Check the arithmetic: collision width 400 km; trench width 120 km centred at −90 km; arc width 110 km at +60 km; uplift width 260 km at +70 km; ridge width 380 km; rift width 70 km. Every one of those is exceeded at ±419 km, on both `profile(+d)` and `profile(-d)`.

Under the buggy `signed = distance * lean` form with a lean of −0.22, 419 km maps to about −92 km — within 2 km of the trench centre — giving roughly `-2597` metres. So this test does not merely fail under the bug, it fails by thousands of metres.

```rust
#[test]
fn every_profile_reaches_zero_before_the_range_gate() {
    // MAX_TECTONIC_RANGE_M's own docstring: "Every profile below must reach exactly
    // zero by here, or the gate itself becomes a cliff." At 419 km every bump
    // argument is outside its width, on both sides of the blend, so the sum is
    // exactly zero -- not merely small.
    //
    // This is the regression test for the 419 km mismapping. The obvious form,
    // `signed = distance * lean`, compresses the axis: with a lean of -0.22 this
    // same point maps to about -92 km, which is the trench centre, and returns
    // roughly -2597 m instead of zero.
    let world = lopsided_world();
    let contribution = world.from_margin_for_test(/* distance_m */ 419_000.0);
    assert_eq!(contribution, 0.0, "a margin 419 km away must contribute exactly nothing");
}
```

**`lopsided_world()` and the test accessor are yours to build** — `from_margin` is private and takes a point, two plates and a normal, so expose it to the test module rather than making it public, and construct a set whose two sides differ in continentality enough to give a non-trivial lean. **Confirm the lean is actually non-zero** and report it; a symmetric margin would make this test pass for the wrong reason, which is precisely the failure mode that made an earlier fade test in this port vacuous.

Also test that a genuinely close margin returns something non-zero, so the test above cannot pass merely because everything returns zero.

- [ ] **Step 2: Run them and confirm they fail** for the expected reason.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them, then prove the regression test earns its place** — temporarily substitute the scaled-axis form `profile(distance_m * setting.lean())` for the two-sided blend, observe the test fail and by how much, then revert and confirm it passes. **Report the number you saw.** A regression test that has never been seen to fail against the bug it names is not yet known to guard anything.
- [ ] **Step 5: Commit.**

---

### Task 5: `offset_m` and `elevation_m`

**Files:** Modify `crates/worldbuilder-engine/src/tectonics.rs`

**Interfaces:**
- Produces: `pub fn offset_m(&self, point: &SpherePoint) -> f64` and `pub fn elevation_m(&self, point: &SpherePoint) -> f64`

`offset_m` calls `margins_within(point, MAX_TECTONIC_RANGE_M, radius_m)`, returns `0.0` immediately if there are none, then for each margin calls `flattened(point, bisector)`, **skips it if that is `None`**, and accumulates `weight * from_margin(...)`.

**The summation order is load-bearing** under any conformance contract: floating-point addition is not associative, so the total depends on the order the margins arrive in. `margins_within` returns them in plate-position order and the Rust must iterate the same way. Say so in a comment.

`elevation_m` is `base_elevation(point) + offset_m(point)`, in that order.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test]
fn a_plate_interior_contributes_exactly_nothing() {
    // three_plate_set's nearest bisector to seed 0 is about 3,900 km away, an order
    // of magnitude beyond MAX_TECTONIC_RANGE_M, so margins_within returns an empty
    // list and offset_m exits before doing any arithmetic at all. Exactly zero, not
    // approximately -- it is a literal early return.
    let world = test_world();
    assert_eq!(world.offset_m(&SpherePoint::from_latlon(0.0, 0.0)), 0.0);
}

#[test]
fn elevation_is_the_base_plus_the_offset_bit_for_bit() {
    // elevation_m is defined as exactly that sum, in that order. Anything less than
    // bit-equality means the composition was rewritten.
    let world = test_world();
    let point = SpherePoint::from_latlon(12.0, 20.0);
    let expected = world.land_base_elevation_for_test(&point) + world.offset_m(&point);
    assert_eq!(world.elevation_m(&point).to_bits(), expected.to_bits());
}

#[test]
fn a_point_near_two_margins_sums_both_contributions() {
    // The reason offset_m exists. Find a point with more than one margin in range and
    // assert the total differs from either margin's contribution alone -- that is what
    // distinguishes summing from choosing, and choosing was worth 560 m of cliff.
}
```

**The third test is a specification, not the test — fill it in with real code**, choosing a point you have verified has at least two margins within `MAX_TECTONIC_RANGE_M` and saying how you found it. If no such point exists in `three_plate_set()`, build a set that has one and say why. **Do not weaken it to a conditional skip**; a test that silently does nothing when its premise fails is how three vacuous tests shipped in this port.

The first test's exactness is worth confirming rather than assuming: verify that `margins_within` really does return empty there, so the assertion is testing the early return rather than a sum that happens to cancel.

- [ ] **Step 2: Run them and confirm they fail** for the expected reason.
- [ ] **Step 3: Implement.** The iteration order over margins is load-bearing — floating-point addition is not associative, so the total depends on it.
- [ ] **Step 4: Run them, then the whole crate suite.**
- [ ] **Step 5: Commit.**

---

### Task 6: Conformance

**Files:** Modify `crates/worldbuilder-engine/src/bindings.rs`, `src/lib.rs`, `tests/test_conformance.py`

**Interfaces:**
- `tectonics_offset_m(seeds, poles, rates, continentality_params, x, y, z, radius_m) -> f64`
- `tectonics_elevation_m(...) -> f64`
- `tectonics_setting_at(...) -> (f64, f64)`
- `tectonics_bump(distance_m, width_m) -> f64` and `tectonics_continental(value) -> f64` — the pure helpers, comparable strictly.

**Apply the contract split, and apply Task 1's finding.**
- `bump` and `continental` are **purely algebraic** — compare with `same()`, bit-for-bit.
- `offset_m`, `elevation_m` and `setting_at` run through `hypot`, `tanh`, the tangent frame and continentality, so they are **bounded** — `close_enough` at `MAX_TRANSCENDENTAL_ULPS`.
- **The set of contributing margins is a discrete outcome.** Whether it can be compared strictly depends on Task 1's measured margin at the `engagement` boundary. State the finding in the test and assert accordingly.

**Measure and assert, do not print.** Record the minimum `abs(abs(across) - ACROSS_ENOUGH)` observed across the corpus and **assert a floor with the observed value in the failure message.** Three vacuous tests have shipped in this port; one asserted only `>= 0.0` and had to be fixed after review.

**Cover:** the corpus against a multi-plate set; plate interiors returning exactly zero; points near triple junctions where two margins contribute; ridge/rift and convergent margins; and points either side of the engagement threshold.

- [ ] **Steps:** bindings, rebuild with `maturin develop --release`, tests, run both suites and quote them, commit.

---

### Task 7: Record it

**Files:** Modify `crates/worldbuilder-engine/README.md`; delete `tests/test_hypot_ulps.py`

- [ ] **Step 1: Record** — that `math.hypot` is not a libm call in CPython but its own Neumaier-summed vector norm, so this is the first slice where the two implementations are *known* to use different algorithms rather than merely permitted to; Task 1's measured ULP distance; **which of the three downstream branches are safe and why** (the sign argument for line 300 especially, since it is the one that looks most dangerous and is not); the measured margin at the `engagement` boundary and its asserted floor; the two documented bugs this port must preserve (the 550-metre cliff and the 419-kilometre mismapping) and how each is encoded; and that `motion.kind` is deliberately unused.
- [ ] **Step 2: Delete the throwaway** `tests/test_hypot_ulps.py`. Its findings live in the README and the ledger.
- [ ] **Step 3:** Verify every test count by running the suites yourself — **do not copy a number from any report.** A count in an earlier README was wrong by twelve because it came from an extraction nobody re-ran.
- [ ] **Step 4: Commit.**

---

## What this slice deliberately does not do

- **No plate generation.** `generation.py` needs `blake2b` over a UTF-8 joined string — a byte-level port feeding a cryptographic hash, and a new engine dependency.
- **No shelves, erosion, bathymetry or detail.** They compose on top of this contribution, which is why `offset_m` returns an offset rather than an elevation.
- **No deletion of the Python.** It stays the reference.
