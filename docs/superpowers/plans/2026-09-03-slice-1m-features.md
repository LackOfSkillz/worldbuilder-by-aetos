# Slice 1m: Placed Features Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port `worldbuilder/bathymetry/features.py` to `crates/worldbuilder-engine/src/features.rs` — the things somebody put there on purpose.

**Architecture:** A `Feature` is a declaration; a `Placed` is one bound to a world radius; `Features` is all of them in order. `apply` walks them in sequence, each arguing with the ground it finds — *"a bank on the chart is a bank somebody placed, and finding one means something."*

**Tech Stack:** Rust (engine), PyO3 bindings, pytest differential harness against the Python reference.

**Spec:** `docs/design/2026-09-02-mark-2-world-studio.md`, sections 4.1 (bit-equality), 4.2 (DETERMINISM-001), 6 (VERSION-001).

## Global Constraints

- **All transcendentals through `detmath`** (libm-backed). No `f64::` method or associated-function form, no `mul_add`, no bare integer casts without a `// cast-ok: <reason>` marker **on the same line as the cast**. A build-failing guard test enforces this. `abs` is exempt and used bare.
- **Constants transcribed character-for-character**, including underscore separators, exponent form and trailing zeros. A mis-grouped constant earlier in this project was "verified" by two agents and caught only by a differential test.
- **Never `f64::min` / `f64::max` / `clamp`.** Python's two-argument forms are asymmetric under NaN. Every clamp and `max` is an explicit `if`/`else` in the Python's operand order; `plates.rs::margin_at` has the house form. **`authority = max(authority, ...)` in `apply` is one of these.**
- **Nothing under `worldbuilder/` may be modified.** The Python remains the reference. `worldbuilder/integration/maritime.py` has a pre-existing uncommitted change; leave it unstaged.
- **A floating-point sign, zero, or exact-equality assertion without the reason it holds is a latent bug.** Five have occurred in this port. Establish why each holds, or do not assert it.
- **Verify by exit status, not by grepping `test result:` lines.** A defect in slice 1i survived three reviews that way.

---

## Why this module comes before `substrate.py`

The original order had substrate first. **Reading `substrate.py` rather than its import list corrected that.** `Substrate` imports only `TangentFrame`, but at runtime it reaches `self.surface.features.placed` and calls `placed.weight_at(point)` and `placed.feature.substrate` — **bypassing `Features.apply` entirely.**

So `Placed` has **two independent consumers**: `Features.apply` inside this module, and `Substrate.at` from outside. **Both `weight_at` and the `feature.substrate` field must be public on the Rust side**, not private helpers of `apply`.

---

## The contract

**Direct transcendentals**: `hypot` at line 88 (feeding a `cos`), and `sin`/`cos` at lines 105-107.

**`hypot` deserves separate treatment.** CPython has not delegated it to libm since 3.8 — it computes its own Neumaier-summed vector norm, so it and Rust's `libm::hypot` are *different algorithms*, measured diverging by up to 1 ULP in slice 1i. `sqrt`, by contrast, is IEEE-754-mandated correctly rounded and costs no bound.

**Indirect transcendentals, and the profiles differ by function** — this is worth getting right, because an earlier instruction of mine got it wrong:
- `weight_at` (line 128) calls **`sphere_to_local`**, whose profile is **`sqrt` + `atan2` only**.
- `marks_near` (line 225) calls `distance_to` → `angle_to`, also **`sqrt` + `atan2`**.
- **Neither calls `local_to_sphere`**, which is the one with `hypot`/`cos`/`sin`. Do not assume the two frame methods have the same profile.

Task 1 confirms all of this against the ported `tangent.rs` rather than taking it from here.

---

## The two discrete decisions — one safe, one load-bearing

**The RAISE/CARVE one-way switch is the one the docstring defends, and its continuity is derivable rather than assumed:**

```python
lift = placed.feature.target_m - result
if placed.feature.compose == RAISE and lift <= 0.0:
    continue
if placed.feature.compose == CARVE and lift >= 0.0:
    continue
result += weight * lift
authority = max(authority, weight * _smooth(abs(lift) / SETTLE_M))
```

At the boundary `lift == 0.0`, **both paths converge on both outputs**: skipping leaves `result` untouched, and taking the branch adds `weight * 0.0`, which is also untouched; the authority term is `weight * _smooth(0.0)`, which is `0.0`, so the `max` is unchanged either way. **A flip at the boundary is unobservable.** Confirm this in Task 1 rather than repeating it.

**The reach gate in `weight_at` is LOAD-BEARING. Measured, not argued.** The extraction claimed both branches give approximately zero weight, so the gate only skips a no-op. **That claim is false.** A ring scan around `reach_m` finds nothing — 30,240 rejected points, zero leaks — which is presumably how the claim was made. Probing the real corner, where `along` and `across` land a hair inside `length` and `width` **simultaneously**, 15,417 of 146,359 gate-rejected points return a non-zero ungated weight (10.53%); largest `1.57e-44`, worst over all offset scales `1.74e-32`. It leaves `result` untouched but **moves `authority` off a hard `0.0`**. And `_cos_reach` is `cos(hypot(L, W) / R)` — two bounded calls — so a one-ULP nudge reclassifies 21.6% of probe points.

**Transcribe the gate and its threshold exactly.** No simplification, and no comment claiming it is a no-op.

**A `-0.0` asymmetry in the RAISE/CARVE guards, which the convergence argument above does not cover.** With `elevation_m == target_m == -0.0`, skipping keeps `-0.0` while taking the branch gives `+0.0`, because `-0.0 + weight * 0.0` is `+0.0`. Bit-different, value-equal. **So these guards are transcribed too, not algebraically simplified.** A simplification that "cannot change the value" changes the sign bit — and this project has been bitten by exactly that before.

**The measured transcendental map** (obtained by bombing one `math` name at a time, which catches indirect reaches through `Vec3.length()` that grep cannot): `reach_m` → `hypot` alone. `Placed::new` → `radians, sin, cos, hypot, sqrt`. `weight_at`, `apply` and `marks_near` → `atan2, sqrt` **and nothing more**. A gate-rejected `weight_at` → none at all. `tangent.rs` matches operation for operation; drift at feature scale is 3 ULP through `sphere_to_local`, 1 ULP through `local_to_sphere`.

---

## What the module records, which are requirements

**Order is semantic, not just numeric.** *"Order is meaning here. A bar listed after the channel it lies across sits on the carved bottom, which is the right story; listed before, the channel would cut straight through it."* The Rust must preserve `placed` order exactly — no sorting, no reordering, no parallel accumulation. This is a stronger requirement than float non-associativity: the *answer* changes, not merely its last bits.

**The one-way switch exists on purpose**, and the docstring defends it. Carry that reasoning.

**Features sit at a specific phase.** `surface.py` records: *"Features come after the shelf and before detail, and that ordering is the phase."*

**A feature reshaping by centimetres should not take the texture away** — the sentence just before the `apply` loop, explaining the `authority` term.

---

## File Structure

- **Create** `crates/worldbuilder-engine/src/features.rs`, declared in `src/lib.rs`.
- **Modify** `crates/worldbuilder-engine/src/bindings.rs` and `src/lib.rs` — the bindings.
- **Modify** `tests/test_conformance.py` — the differential tests.
- **Modify** `crates/worldbuilder-engine/README.md` — the record.

`EARTH_RADIUS_M` and `TangentFrame` are already ported. **Read `tangent.rs` for the actual method names and signatures** — in particular confirm `sphere_to_local` exists and what it returns.

---

### Task 1: Settle the contract and both convergence claims

**Files:** Create `tests/test_features_gates.py` (throwaway; deleted in Task 6)

This task writes no engine code. It answers three questions the rest of the slice depends on.

- [ ] **Step 1: Confirm the transcendental map.** For each of `weight_at`, `apply`, `marks_near` and `reach_m`, say which transcendentals the path actually reaches, directly and indirectly. **Verify `sphere_to_local`'s profile is `sqrt` + `atan2` and that `local_to_sphere`'s is different** — check both in the Python and in the ported `tangent.rs`. Report any mismatch between them.

- [ ] **Step 2: Verify the RAISE/CARVE convergence exactly.** Construct a case where `lift` is exactly `0.0` and confirm that skipping and not-skipping give **bit-identical** `result` and `authority`. Then construct cases where `lift` is one ULP either side of zero and report how much the outputs differ — that is the real measure of whether a boundary flip could matter.

- [ ] **Step 3: Test the reach-gate convergence rather than assuming it.** The claim is that both branches give approximately zero weight. **Find out whether it is exact.** Construct a point where the gate is at its boundary, compute what `weight_at` would return on each side, and report the difference bit-for-bit. **If the two branches are not identical, that guard is load-bearing and the port must reproduce it exactly** — which is what a similar claim turned out to be in the previous module.

- [ ] **Step 4: Record the answers in the ledger with the numbers.** Do not proceed to Task 2 until they are recorded.

---

### Task 2: Constants, `_smooth`, `_bump`, and `Feature`

**Files:** Create `crates/worldbuilder-engine/src/features.rs`; modify `src/lib.rs`

**Interfaces:** every module constant; `smooth`; `bump`; and the `Feature` type with its `reach_m`.

**`smooth` almost certainly already exists** — `detail.rs` has one, and `shelf.rs` reuses it. **Read both before adding a third.** If `features.py`'s `_smooth` is identical, reuse; if it differs, say how. `_bump` may be new — compare it against `detail.rs`'s if one exists there.

`Feature.substrate` is an **`Option`** used by `substrate.py` as an `is None` sentinel. Note this is a *third* optional idiom in this codebase: `radius_m=EARTH_RADIUS_M` is plain default substitution, `Feature.substrate is None` is a sentinel, and `detail.py` used a falsy-check where `0.0` meant absent. **They are not interchangeable.**

- [ ] **Steps:** failing tests pinning each constant against the Python, run, implement, run, whole crate suite by exit status, commit.

---

### Task 3: `Placed` and `weight_at`

**Files:** Modify `crates/worldbuilder-engine/src/features.rs`

**Interfaces:** `Placed` with a constructor taking `(feature, radius_m)`, and `pub fn weight_at(&self, point) -> f64`.

**`weight_at` must be public** — `Substrate` calls it directly, bypassing `Features::apply`.

**Transcribe the reach gate and its threshold exactly** — Task 1 measured it LOAD-BEARING, contradicting the extraction. Record the measured leak rate in a comment (10.53% of gate-rejected corner points carry non-zero ungated weight, largest `1.74e-32`) so nobody later "simplifies" it back. `_cos_reach` is `cos(hypot(L, W) / R)`: two bounded calls, and a one-ULP nudge reclassifies 21.6% of probe points.

- [ ] **Steps:** failing tests, run, implement, run, commit.

---

### Task 4: `Features` and `apply`

**Files:** Modify `crates/worldbuilder-engine/src/features.rs`

**Interfaces:** `Features` with `placed`, `len`, iteration, `apply` returning `(f64, f64)`, and `marks_near`.

**`apply` returns `(shaped_metres, authority)`** — the first an **absolute elevation**, the second a **blend weight**, not a height. Do not let the two be confused in the binding.

**Iteration order is semantic.** Preserve it exactly and comment why.

**`authority = max(authority, ...)`** is a two-argument `max` — explicit `if`/`else` in the Python's operand order.

**Transcribe the RAISE/CARVE guards; do not simplify them.** They converge at `lift == 0.0` on both outputs, but with `elevation_m == target_m == -0.0` skipping keeps `-0.0` and taking the branch gives `+0.0`. Value-equal, bit-different.

- [ ] **Step 1: Write the failing tests**, including one that **proves order matters**: the same two features in opposite orders must give different results, matching the docstring's bar-and-channel story. **Derive the expected values from the formula and say how**, and pick features where the difference is genuinely visible rather than a rounding artefact.
- [ ] **Steps 2-5:** Run, implement, run, commit.

---

### Task 5: Conformance

**Files:** Modify `crates/worldbuilder-engine/src/bindings.rs`, `src/lib.rs`, `tests/test_conformance.py`

**Apply the contract Task 1 established, and state which paths are strict and which bounded** in the test names or comments. **If a comparison you expect to be strict needs a tolerance, that is a finding to report, not a bound to add quietly.**

**Do not borrow a bound from another module.** Slice 1l did that and it went wrong twice over: the borrowed ceiling was 227× looser than the field needed and hid a real defect, and the justification for borrowing it was factually wrong. **Measure this module's own worst case per field and size each bound to it**, with the legitimate maximum stated in the comment.

**`authority` gets its own bound, measured separately from `result`, and its reach-gate test compares RAW BITS rather than a tolerance.** `result` absorbs sub-ULP contributions; `authority` amplifies them off zero, so a tolerance would make Task 1's reach-gate finding untestable. Measure before asserting: if the bounded path cannot support a strict bit comparison, report that rather than widening it quietly.

**`marks_near` is a bounded quantity feeding a DISCRETE output** — chart membership via `distance <= within_m`, and the sort order. Task 1 did not measure it. **Measure the margin between the nearest included and the nearest excluded feature, and report whether a bounded drift could reorder or reclassify.** A discrete flip is the one thing no tolerance absorbs.

**Cover:** an empty `Features`; a single feature; several in both orders; features whose `lift` sits at and near zero for both RAISE and CARVE; points inside, at and beyond the reach; and `marks_near` at several radii.

- [ ] **Steps:** bindings, rebuild with `maturin develop --release`, tests, run both suites quoting them and checking exit status, commit.

---

### Task 6: Record it

**Files:** Modify `crates/worldbuilder-engine/README.md`; delete `tests/test_features_gates.py`

- [ ] **Step 1: Record** — that `Placed` has two independent consumers and why `weight_at` is public; the transcendental map, including that `sphere_to_local` and `local_to_sphere` have **different** profiles; both convergence findings from Task 1 with their measured numbers, stated as measured rather than argued; that **iteration order is semantic** and what breaks if it is disturbed; the per-field bounds with their legitimate maxima; and the phase note that features come after the shelf and before detail.
- [ ] **Step 2: Delete the throwaway.**
- [ ] **Step 3: Verify every count by running the suites and checking exit status.** Do not copy a number from any report.
- [ ] **Step 4: Commit.**

---

## What this slice deliberately does not do

- **No `substrate.py`.** It needs this module, and a host trait for the surface back-reference. Next slice.
- **No `surface.py`.** The capstone, once substrate lands.
- **No deletion of the Python.** It stays the reference.
