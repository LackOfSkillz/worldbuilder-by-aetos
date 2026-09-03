# Slice 1l: Continental Shelf Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port `worldbuilder/bathymetry/shelf.py` to `crates/worldbuilder-engine/src/shelf.rs` — the water a ship actually sails in.

**Architecture:** The shelf sets a **target depth** and blends the ground towards it. It does not add an offset, and that distinction is the module's first rule. Everything is weighted rather than classified: not "is this a continent", not "is this an island", not "is this near a coast".

**Tech Stack:** Rust (engine), PyO3 bindings, pytest differential harness against the Python reference.

**Spec:** `docs/design/2026-09-02-mark-2-world-studio.md`, sections 4.1 (bit-equality), 4.2 (DETERMINISM-001), 6 (VERSION-001).

## Global Constraints

- **All transcendentals through `detmath`** (libm-backed). No `f64::` method or associated-function form, no `mul_add`, no bare integer casts without a `// cast-ok: <reason>` marker **on the same line as the cast**. A build-failing guard test enforces this. `abs` is exempt and used bare.
- **Constants transcribed character-for-character**, including underscore separators and exponent form: `COASTAL_WINDOW = 0.055`, `MIN_GRADIENT = 1.0e-8`, `REFERENCE_GRADIENT = 2.0e-7`, `SHELF_BREAK_M = 80_000.0`, `SHELF_EDGE_M = -150.0`, `SLOPE_WIDTH_M = 70_000.0`, `INLAND_REACH_M = 12_000.0`, and the remaining constants below line 60. A mis-grouped constant earlier in this project was "verified" by two agents and caught only by a differential test.
- **Never `f64::min` / `f64::max` / `clamp`.** Python's two-argument forms are asymmetric under NaN. Every clamp is an explicit `if`/`else` in the Python's operand order; `plates.rs::margin_at` has the house form.
- **Nothing under `worldbuilder/` may be modified.** The Python remains the reference. `worldbuilder/integration/maritime.py` has a pre-existing uncommitted change; leave it unstaged.
- **A floating-point sign, zero, or exact-equality assertion without the reason it holds is a latent bug.** Five have occurred in this port. Establish why each holds, or do not assert it.
- **Verify by exit status, not by grepping `test result:` lines.** A defect in slice 1i survived three reviews that way.

---

## What this module returns, and why it is not what the last two returned

`tectonics.py` and `detail.py` both return a **contribution to be added**. **`shelf.py` does not.** It returns an **absolute elevation**, produced by blending the macro ground towards a target depth:

```python
shaped = macro + weight * (self.target_depth_m(coastal) - macro)
```

The docstring is emphatic about why: *"A shelf describes what the coastal profile should tend to, and blending leaves control over what it may override — so a trench crossing a continental margin is not quietly flattened by something announcing that the water here is about a hundred metres."*

Do not "make it consistent" with the neighbouring modules by converting it to an offset. The difference is the design.

---

## The contract, and where the one transcendental actually is

**`shelf.py` itself contains no transcendental call.** No `sin`, `cos`, `sqrt`, `hypot`, `exp` or `log` appears anywhere in the file.

But it reaches one **indirectly**: `self.land.gradient(point).magnitude()` calls `Gradient.magnitude()`, which uses `hypot`. **That matters more than an ordinary transcendental would**, because CPython has not delegated `hypot` to libm since 3.8 — it computes its own Neumaier-summed vector norm, so it and Rust's `libm::hypot` are *different algorithms*, measured diverging by up to 1 ULP in slice 1i.

So: **everything downstream of `slope` is bounded; everything else is strict.** Task 1 establishes exactly which paths those are.

---

## The three gates, and the structural fact that makes them safe

The module's second rule is *"every gate sits outside the support of what it gates"*, and it explains why: *"The general form of M1.4's worst bug, where a trench profile was still a thousand metres tall at the range limit and the optimisation that skipped it became a cliff."*

**That principle is realised structurally, and noticing this is what settles the hazard.** In `evaluate`, both early returns produce the **identical `Reading`**:

```python
coastal = self.coastal(point)
if coastal is None:
    return Reading(elevation_m=macro, weight=0.0, tectonic_m=tectonic)

weight = self.weight(point, coastal, tectonic)
if weight <= 0.0:
    return Reading(elevation_m=macro, weight=0.0, tectonic_m=tectonic)
```

**So a gate flipping is observable only if the alternative branch would have produced a weight above zero.** The gates are not independent hazards; they funnel into the same result.

Now each one:

**`abs(value) > COASTAL_WINDOW` — and note it is NOT downstream of the `hypot`.** `value` comes from `above_shore(point)`, which never touches `gradient()`. Whatever contract `above_shore` earns, this gate inherits; it does not inherit the `hypot` bound. **I measured the margin: the closest approach across 4,000 random points is `1.21e-4`, against a threshold of `0.055` where one ULP is about `7e-18` — roughly fourteen orders of headroom.**

**`slope < MIN_GRADIENT` — this one IS downstream of the `hypot`**, and the extraction flagged it as the decision that matters. **Measured, it is safe by an enormous margin**: the closest approach is `2.64e-9` against a threshold of `1e-8`, where one ULP is about `1.7e-24` — roughly fifteen orders. A 1-ULP `hypot` difference cannot come close to flipping it.

**It is also live code, not dead defence**: the sampling found a point at `0.736 ×` `MIN_GRADIENT`, so the gate genuinely fires. Its constant's comment claims *"the weight has already faded out by here; this only stops the arithmetic"* — **that claim is worth checking rather than repeating**, because `breadth = _smooth(REFERENCE_GRADIENT / slope)` clamps to `1.0` when the slope is tiny, so the fade must come from the enormous `distance_m = value / slope` instead, and a point where *both* `value` and `slope` are tiny would give a small distance and a large weight.

**`offshore <= 0.0` and `offshore >= 0.0` — safe for a stated reason, and it is the same argument three earlier slices turned on.** `offshore = -(value / slope)`, and `slope` is strictly positive because the `MIN_GRADIENT` gate has already returned. **Dividing by a strictly positive quantity cannot change a sign**, so these tests are decided by the sign of `value`, which never passes through the `hypot`. They look exposed and are not.

**`weight <= 0.0`** sits at a true zero of a smoothstep-clamped product, and funnels to the same `Reading` as the `None` case above.

---

## What the module records as scars

Three, and they are requirements rather than colour.

**The blend rather than the offset**, above.

**Gates outside the support of what they gate** — M1.4's trench profile still a thousand metres tall at the range limit, where *"the optimisation that skipped it became a cliff."*

**Nothing classified.** *"Not 'is this a continent', not 'is this an island', not 'is this near a coast'. M1.4 produced four separate cliffs from four hard decisions taken on continuous quantities, and every equivalent temptation here is answered with a weight."*

And two performance facts that explain the shape of the code:

- **The value is checked before the gradient is taken**, and *"that ordering is the whole performance strategy of this file: the gradient costs six times what the value does, and most of a planet is deep interior or deep basin."* Do not reorder those two checks.
- **`evaluate` returns intermediates deliberately.** *"Asking separately cost the gradient twice and the tectonics three times over, which took a whole-pipeline chart from three hundred milliseconds to twelve hundred — a comment claiming the values were 'recovered rather than recomputed where it is free' while they were being recomputed."* The `Reading` carries `weight` and `tectonic_m` because the layer above needs them and they are expensive.

---

## File Structure

- **Create** `crates/worldbuilder-engine/src/shelf.rs`, declared in `src/lib.rs`.
- **Modify** `crates/worldbuilder-engine/src/bindings.rs` and `src/lib.rs` — the bindings.
- **Modify** `tests/test_conformance.py` — the differential tests.
- **Modify** `crates/worldbuilder-engine/README.md` — the record.

Every dependency is already ported: `Tectonics`, `Continentality` (with `above_shore`, `gradient`, `base_elevation`), `SpherePoint`, `EARTH_RADIUS_M`. **Verify `Gradient::magnitude` exists on the Rust side with the same `hypot`-based definition** before relying on it.

---

### Task 1: Measure the two gate margins before anything depends on them

**Files:** Create `tests/test_shelf_gates.py` (throwaway; deleted in Task 7)

This task writes no engine code. It settles which contract each path earns.

- [ ] **Step 1: Establish where the `hypot` actually is.** Confirm `shelf.py` contains no direct transcendental, and that the only one it reaches is `hypot` inside `Gradient.magnitude()`, called from `coastal()`. **Confirm `above_shore` does not reach it** — if it does, gate 1 inherits the bound too and this plan's analysis needs revising. Report either way.

- [ ] **Step 2: Measure both gate margins over the project's real `corpus()`**, not a random sample. For every point, record `abs(abs(above_shore) - COASTAL_WINDOW)` and, for points inside the window, `abs(slope - MIN_GRADIENT)`. Report the minimum of each alongside the ULP magnitude at that threshold.

  **Expected from my own sampling of 4,000 random points, which you should treat as a prior to check rather than a result to confirm:** about `1.21e-4` for the window and `2.64e-9` for the gradient. If the real corpus comes closer, say so — a smaller margin is the finding.

- [ ] **Step 3: Check the `MIN_GRADIENT` comment's claim.** Its constant says *"the weight has already faded out by here"*. Find whether that holds: for points at or below `MIN_GRADIENT`, compute what `weight` would be if the gate did not return early. **A point where both `value` and `slope` are tiny would give a small `distance_m` and a large weight**, which would make the claim false in general even if no such point appears in the corpus. Report what you find, and whether the claim is true universally, true for the corpus, or false.

- [ ] **Step 4: Confirm the gate is reachable.** My sampling found a point at `0.736 ×` `MIN_GRADIENT`. Confirm the gate fires somewhere in the corpus, so it is live code rather than dead defence.

- [ ] **Step 5: Record the answers in the ledger with the numbers.** Do not proceed to Task 2 until they are recorded.

---

### Task 2: Constants, `_smooth`, and the two value types

**Files:** Create `crates/worldbuilder-engine/src/shelf.rs`; modify `src/lib.rs`

**Interfaces:**
- Produces: every module constant, transcribed character-for-character.
- Produces: `fn smooth(fraction: f64) -> f64` — check whether the codebase already has one that matches; `detail.rs` and `tectonics.rs` both do. **If an existing one is identical, use it rather than adding a third copy**, and say which you chose and why.
- Produces: `pub struct Coastal { pub distance_m: f64, pub breadth: f64 }` and `pub struct Reading { pub elevation_m: f64, pub weight: f64, pub tectonic_m: f64 }`.
- Produces: `pub struct Shelf { tectonics: Tectonics, land: Continentality, radius_m: f64 }` with a constructor.

- [ ] **Step 1: Write the failing tests**, pinning each constant against the Python and covering `smooth`'s saturation and midpoint. **Read the constants from the live Python** rather than copying from this plan.
- [ ] **Steps 2-5:** Run and confirm they fail for the expected reason, implement, run again, whole crate suite by exit status, commit.

---

### Task 3: `coastal`

**Files:** Modify `crates/worldbuilder-engine/src/shelf.rs`

**Interfaces:** Produces `pub fn coastal(&self, point: &SpherePoint) -> Option<Coastal>`

Transcribe lines 127-163. **The ordering is load-bearing**: the value is checked before the gradient is taken, because the gradient costs six times what the value does. Do not reorder, and carry that reason into a comment.

Both gates return `None`, and both carry a comment explaining they sit outside the support of what they gate. Carry those too.

- [ ] **Step 1: Write the failing tests** covering: a deep interior point returning `None` on the value gate having taken no gradient; a coastal point returning `Some`; and the `MIN_GRADIENT` gate firing where the field is flat. **Use the point Task 1 found at `0.736 ×` `MIN_GRADIENT` if the corpus offers one**, so the second gate is genuinely exercised rather than assumed unreachable.
- [ ] **Steps 2-5:** Run, implement, run, commit.

---

### Task 4: `target_depth_m` and `weight`

**Files:** Modify `crates/worldbuilder-engine/src/shelf.rs`

**Interfaces:**
- Produces: `pub fn target_depth_m(&self, coastal: &Coastal) -> f64`
- Produces: `pub fn weight(&self, point: &SpherePoint, coastal: &Coastal, tectonic_m: Option<f64>) -> f64`

`max(0.15, coastal.breadth)` appears in both and is a two-argument `max` — explicit `if`/`else` in the Python's operand order.

**`tectonic_m` is an optional the Python defaults to `None` and recomputes**: *"Worked out again if not, which is what makes it worth passing."* Note this is the **safe** optional idiom — an `is None` identity check, not a float-falsy `if x:` — so `Some(0.0)` must behave as a supplied zero, **not** as absent. That is the opposite of the trap in slice 1k, and getting it backwards would be a real divergence.

The four fades in `weight` each replace a decision that could have been a hard test; the docstring names all four. Carry that.

- [ ] **Step 1: Write the failing tests**, deriving expected values from the formula rather than recording implementation output. Cover: offshore beyond the break fading to nothing; inland fading quickly; a large `tectonic_m` suppressing the weight; and `Some(0.0)` for `tectonic_m` behaving as a supplied zero rather than as absent.
- [ ] **Steps 2-5:** Run, implement, run, commit.

---

### Task 5: `evaluate` and `elevation_m`

**Files:** Modify `crates/worldbuilder-engine/src/shelf.rs`

**Interfaces:** Produces `pub fn evaluate(&self, point: &SpherePoint) -> Reading` and `pub fn elevation_m(&self, point: &SpherePoint) -> f64`

**The blend, transcribed exactly:** `shaped = macro + weight * (target_depth_m(coastal) - macro)`. Not an offset, not a lerp written the other way round — under a bit-for-bit contract an algebraically equivalent rearrangement is not equivalent.

**Both early returns produce the identical `Reading`.** That is what makes the gates safe, so write them as the Python does rather than collapsing them.

`evaluate` computes `tectonic` once and passes it to `weight`, which is the performance fix the docstring records. **Do not let it be recomputed.**

- [ ] **Step 1: Write the failing tests** covering: a deep interior returning `macro` with weight exactly `0.0`; a shelf point returning a shaped elevation between `macro` and the target; `elevation_m` agreeing with `evaluate().elevation_m` bit-for-bit; and that `tectonic_m` in the returned `Reading` equals what `Tectonics::offset_m` gives for the same point.
- [ ] **Steps 2-5:** Run, implement, run, commit.

---

### Task 6: Conformance

**Files:** Modify `crates/worldbuilder-engine/src/bindings.rs`, `src/lib.rs`, `tests/test_conformance.py`

**Interfaces:**
- `shelf_coastal(...) -> Option<(f64, f64)>`
- `shelf_target_depth_m(distance_m, breadth) -> f64`
- `shelf_weight(...) -> f64`
- `shelf_evaluate(...) -> (f64, f64, f64)`

**Apply the contract Task 1 established**, and do not blur it. Paths that never touch `slope` are strict — compare with `same()`. Paths downstream of the `hypot` are bounded at `MAX_TRANSCENDENTAL_ULPS`. **State which is which in the test names or comments**, so a later reader can see the split rather than inferring it.

**The `None` cases must be compared positionally** — the Rust returns `None` exactly where the Python does, not merely agreeing where both are `Some`.

**Cover:** the corpus; deep interiors and deep basins that fail the value gate; coastal points that pass it; points near the shelf break; points inland within `INLAND_REACH_M`; a large tectonic offset suppressing the weight; and `tectonic_m` supplied as `Some(0.0)` versus absent.

**Measure and assert, do not print.** Report the worst ULP distance on the bounded paths and assert a floor on the two gate margins with the observed values in the failure messages. Pytest swallows `print` on a passing run, and three tests in this port have shipped asserting nothing.

- [ ] **Steps:** bindings, rebuild with `maturin develop --release`, tests, run both suites quoting them and checking exit status, commit.

---

### Task 7: Record it

**Files:** Modify `crates/worldbuilder-engine/README.md`; delete `tests/test_shelf_gates.py`

- [ ] **Step 1: Record** — that this module returns an **absolute elevation by blending**, unlike its two neighbours, and why; that it contains no direct transcendental but reaches one indirectly through `Gradient.magnitude`'s `hypot`, which is CPython's own algorithm rather than libm's; **the structural reason the gates are safe** — both early returns produce the identical `Reading`, so a gate flip is observable only if the alternative would have given a weight above zero; the two measured gate margins from Task 1 with their ULP context; **which gates are and are not downstream of the `hypot`** (the value gate is not); the sign argument for the `offshore` tests; whether the `MIN_GRADIENT` comment's claim about the weight having faded turned out true, corpus-true, or false; and the three scars the module records.
- [ ] **Step 2: Delete the throwaway** `tests/test_shelf_gates.py`.
- [ ] **Step 3: Verify every count by running the suites and checking exit status.** Do not copy a number from any report — a count in an earlier README was wrong by twelve because it came from an extraction nobody re-ran.
- [ ] **Step 4: Commit.**

---

## What this slice deliberately does not do

- **No `substrate.py` or `features.py`.** Each is its own slice; both have their dependencies ported already.
- **No `surface.py`.** It composes continentality, tectonics, detail, shelf, substrate and features, and is the capstone once all of them land.
- **No deletion of the Python.** It stays the reference.
