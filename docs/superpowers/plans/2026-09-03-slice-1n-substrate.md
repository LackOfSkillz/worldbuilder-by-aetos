# Slice 1n: Substrate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port `worldbuilder/bathymetry/substrate.py` to `crates/worldbuilder-engine/src/substrate.rs` — what the seabed is made of.

**Architecture:** A `Composition` is three normalised fractions; `natural` derives one from elevation, slope and tectonics; `at` blends it toward each placed feature's declared substrate. The module holds no state — it reads its host.

**Tech Stack:** Rust (engine), PyO3 bindings, pytest differential harness against the Python reference.

**Spec:** `docs/design/2026-09-02-mark-2-world-studio.md`, sections 4.1 (bit-equality), 4.2 (DETERMINISM-001), 6 (VERSION-001).

## Global Constraints

- **All transcendentals through `detmath`** (libm-backed). No `f64::` method or associated form, no `mul_add`, no bare integer cast without a `// cast-ok: <reason>` marker **on the same line as the cast**. A build-failing guard test enforces this. `abs` is exempt and used bare.
- **Constants transcribed character-for-character**, including underscore separators, exponent form and trailing zeros.
- **Never `f64::min` / `f64::max` / `clamp`.** Python's two-argument forms are asymmetric under NaN. Every clamp is an explicit `if`/`else` in the Python's operand order; `plates.rs::margin_at` has the house form.
- **Nothing under `worldbuilder/` may be modified.** `worldbuilder/integration/maritime.py` has a pre-existing uncommitted change; leave it unstaged.
- **Verify by exit status, not by grepping `test result:` lines.**
- **Re-derive every number; never transcribe one.** Six numbers were overturned in the previous slice, each after reading as solid. State the population every figure was measured over and the resolution of any scan.

---

## What the extraction established, measured rather than argued

**This module is almost entirely strict.** `natural`, `Composition::new`, `blended_towards`, `dominant`, `holding` and `smooth` reach **zero transcendentals** — measured by bombing 22 `math` names one at a time, which catches indirect reaches grep cannot see. They transcribe **strictly, raw bits, and owe no bound**.

**Exactly one thing owes a bound:** `slope_at`, which owns a `math.hypot` on top of four `local_to_sphere` calls that each reach `hypot` again. `hypot` is the one call where CPython and Rust run genuinely different algorithms — CPython has computed its own Neumaier-summed norm rather than calling libm since 3.8.

**`at()` pays no `hypot` at all.** With all three optionals supplied it reaches `atan2` + `sqrt`; strip the features and it reaches nothing. `Feature::reach_m`'s `hypot` is folded into `_cos_reach` at `Placed` construction. This confirms the features README's warning from a second direction: `sphere_to_local` (`atan2`+`sqrt`) and `local_to_sphere` (`hypot`+`cos`+`sin`+`sqrt`) have **different profiles**.

**Do not borrow the features bounds.** They are aspect-ratio-driven, validated 1.5:1 to 250:1, and `weight_at`/`authority` are bounded *absolutely* rather than in ULP because at `bump`'s support edge the value cancels to ~1e-31 where the ULP measure reaches 4.19e18. This module calls `weight_at` directly and **needs its own measurement**.

---

## The banked 19% was right about the wrong population — including in my own brief

The claim that `blended_towards(x, 0.0)` is not the identity, and therefore that `at()`'s `weight > 0.0` guard is bit-observable, **is true**. The 19% figure attached to it is not what `at()` feeds:

| Population | Rate |
|---|---|
| Uniform-random triples (what was banked) | 19.09% (3,818/20,000, seed-dependent) |
| Real `natural()` outputs | **0.15%** |
| Coast-local grid, 61x61 at 1,500 m **per step** | **1.80%** (67/3,721) |
| The same grid read as a +/-1,500 m **span** | 4.49% (167/3,721) |
| The same two under `TangentFrame.at(region.origin)` instead | 1.67% (62) and 3.82% (142) |
| The span reading re-measured under the frame that reproduces reading A exactly | 4.97% (185/3,721) |

**Only the ABSOLUTE worst shift is a property of the module.** The relative figure `1.249555e-15` and the
distance of 11 ULP are properties of *reading A specifically* — under reading B the worst relative shift is
`1.887498e-15`. The absolute `2.220446e-16` survives every convention tried; nothing else does.

**A rate here needs three fields, not one: the frame, the grid step, and the span.** Seven further
sampling conventions give counts from 18 to 159. The conclusion is robust — the guard is bit-observable
under all nine — but the *rate* is a property of the convention. This number has now been narrowed three
times, and each earlier reading was true without being sufficient.

**Worst shift `2.220446e-16` is ABSOLUTE, not relative** — and that distinction is a trap, because the figure sits a hair under `SLOPE_DRIFT_REL` (`2.212201e-16`) and so reads as if it were the same kind of quantity. It is not. The worst *relative* shift on that population is `1.249555e-15`, and the worst distance is **11 ULP, not 1**. Read as relative, the figure understates the guard by 5.6x. Zero `dominant` flips. **The conclusion holds and the guard must be transcribed.** But the number that justified it was measured over a population the code never produces — the same error that cost the previous slice four of its six overturned figures, this time in a figure I supplied.

---

## Three discrete decisions that do not converge

**1. `Composition::new`'s `total <= 0.0` guard.** One ULP above zero you get whatever direction the triple points; at zero you get pure ROCK. `natural` can never trip it (argued, not measured — **Task 1 must settle which**) and `blended_towards` cannot, but the public constructor can, and `test_substrate.py:83` pins it.

**2. `dominant` returns a WORD**, so no tolerance can absorb a flip. Tie precedence is ROCK > SAND > MUD: rock wins when it is at least sand and at least mud, otherwise sand wins when it is at least mud. A real `mud`-to-`sand` boundary bisects to `2.27e-13 m` with margins around `1e-13`, so a flip is **genuinely reachable** — but 20,000 area-uniform planetary probes found **zero** within `1e-6` of a tie. Structurally this is the same recorded-not-tolerated divergence as `marks_near` membership in the previous slice: record the condition, do not pretend a bound fixes it.

**3. `PURE[""]` raises `KeyError`** — measured. `test_conformance.py` deliberately pins that an empty-string substrate survives the FFI crossing distinct from `None`, so a value the port *guarantees can arrive here* makes `Substrate.at` raise. Nothing constructs one today. **The port must decide this deliberately rather than discover it in production.**

**Ruling, to implement rather than re-open:** the Rust reproduces the failure. It must not silently succeed where Python raises — a silent divergence is worse than a loud one, and this is the only place in the module where the two could disagree about whether an answer exists at all. Surface it as a typed error at the binding rather than a panic across FFI, and pin with a conformance test that **both** sides fail on the empty string.

---

## Two corpora, opposite shapes — and my first statement of this rule was backwards

**For the clamps and any saturation question: a small steep feature, scanned in 2-D.** A 3,000-point
planetary scatter reports `natural`'s slope clamp as never reached — it reads as dead code. Through the
demo pinnacle a 2-D grid at 0.9333 m/step over 90,601 points reaches **8.1410x ROCK_SLOPE**; for drying
rock, 5.5970x.

**A line through the feature is not enough, and no line direction rescues it.** The steepest ground is
off-axis, because `weight_at` is a product of two `bump` factors. Offshore reaches 7.6275, alongshore
7.5556, diagonal 7.8208 — **none of the three reaches the grid's figure**. With extent matched, a +/-70 m
grid still hits 8.1417, at an offset of (31.73, 15.40) m. Nor does density help: the 401- and 1,601-point
lines both give 7.6275, unchanged at 4x resolution.

**For `dominant`'s tie margins: gentle open water, and the pinnacle is the wrong corpus.** The smallest
margin measured is **2.109424e-15**, offshore, with the bracketing sides `3.638e-12 m` apart — **and that
figure is a property of the SEARCH METHOD, not just the corpus.** It comes from a bisection onto the
crossover contour. A *grid* over the same gentle water finds `7.485921e-04`, eleven orders coarser, because
a grid samples where its nodes fall rather than where the boundary is. The ordering survives either way —
open water is 3.4x tighter than the pinnacle by grid, and four orders tighter by bisection — but a figure
like this must name its search, not only its population. The pinnacle
bisection bottoms out four orders *higher*, at 1.29e-11. **And resolution does not rescue the steep case
either** — a radial found the same `6.646421e-03` at both 0.750 and 0.188 m/step, while a 2-D grid closed
to `1.7e-4`.

**The counter-intuitive half is verified, not assumed.** Steep ground makes a crossing *easier* to
resolve because the residual at exhaustion is the local composition gradient: 8.2e-3 and 5.7e-3 per metre
at the two steep crossings, against 1.2e-7 and 2.4e-10 per metre at the gentle ones — monotone with the
residuals. The offshore ray has the **coarsest** ULP step of any of them, so it is steepness, not
floating-point resolution, that sets the floor.

**So there is no single "good corpus".** Each question needs the corpus that can express its answer:
saturation wants steep and two-dimensional, tie margins want gentle and flat. Say which corpus answered
which question, and state its resolution — a figure without both is not yet a measurement.

---

## Do not bank "the composition sums to one"

The extraction argued `natural`'s pre-normalisation total is exactly `1.0`. It is not: an exhaustive sweep
of the argument domain — both `rock` and `swept` are `smooth` outputs, so `[0,1]` unconditionally,
1,002,001 pairs — gives a minimum of `0.9999999999999998`, 2 ULP low.

The answer to "can `natural` trip the `total <= 0.0` guard" is still a firm **no**. But the invariant that
justified it is false by 2 ULP, and **the port must not encode "sums to one" anywhere** — not as an
assertion, not as a simplification, not as a reason to skip the normalising division.

---

## File Structure

- **Create** `crates/worldbuilder-engine/src/substrate.rs`, declared in `src/lib.rs`.
- **Modify** `crates/worldbuilder-engine/src/bindings.rs` and `src/lib.rs`.
- **Modify** `tests/test_conformance.py`.
- **Modify** `crates/worldbuilder-engine/README.md`.

`features.rs`, `tangent.rs` and `sphere.rs` are ported. **Read `features.rs` from the shell** — the `Read` tool returned stale content for it twice in the previous slice.

---

### Task 1: Settle the host trait and the three non-convergent decisions

**Files:** Create `tests/test_substrate_gates.py` (throwaway; deleted in Task 6)

No engine code. This task decides the module's shape.

- [ ] **Step 1: Enumerate the host surface from evidence.** `Substrate` reaches `surface.radius_m`, `surface.structural_m(point)`, `surface.tectonics.offset_m(point)` and `surface.features.placed`. **Search the whole file, not just `at()`**, and confirm `shelf`, `detail`, `land`, `plates`, `elevation_m` and `bottom_at` are genuinely unreached. Give each reached member's exact signature and return type.
- [x] **Steps 2-5 are settled. Task 1 is complete; its answers are below and bind the later tasks.**

**The construction shape: free functions plus an on-demand borrow — no trait, and no stored `Substrate` field.** `slope_at` takes a `structural_m` callback; `at` takes `&Features` concretely, which is already ported and keeps the pinned reach gate; the `**known` optionals become three `Option<f64>` in `at`'s **resolution order — elevation, tectonic, slope**, which is observable because each `None` triggers a different host call. Cost to `surface.rs`: no new field, three forwarding methods, and one indirect call per structural probe in `bottom_at` — a cost that method's own docstring already prices, and removable later by making `slope_at` generic over `F: Fn`.

**The host surface is four members**, confirmed by a runtime attribute-recording proxy — stronger than grep, being immune to `getattr`, helpers and comprehensions — cross-checked against `grep` and both `git grep` forms, which agreed: `radius_m` (`float`), `structural_m(point) -> float`, `tectonics.offset_m(point) -> float`, `features.placed -> tuple[Placed, ...]`. `shelf`, `detail`, `land`, `plates`, `elevation_m` and `bottom_at` are genuinely unreached; their nonzero grep counts are docstring prose and a local parameter name.

---

### Task 2: `Composition`, `smooth`, and the constants

**Files:** Create `crates/worldbuilder-engine/src/substrate.rs`; modify `src/lib.rs`

**Interfaces:** the three substrate names, `ROCK_SLOPE`, `ROCK_TECTONIC_M`, `SWEPT_M`, `SETTLED_M`, `SLOPE_BASELINE_M`, the `PURE` table; `Composition` with its normalising constructor, `blended_towards`, `dominant`, `holding`.

**All of this is STRICT — zero transcendentals, raw-bit comparison, no bound.** If any test here needs a tolerance, that is a finding to report, not a bound to add.

**`smooth` may already exist** — `detail.rs` has one and `shelf.rs` reuses it; `features.rs` has a separate `bump`. **Read them before adding another.** If this module's differs, say how.

**Transcribe the `total <= 0.0` guard exactly** — it does not converge, and `test_substrate.py:83` pins it.

**`dominant`'s tie precedence is ROCK > SAND > MUD** and its output is a word. Transcribe both comparisons with their exact directions.

- [ ] **Steps:** failing tests pinning each constant and function against the live Python, run, implement, run, whole crate suite by exit status, commit.

---

### Task 3: `natural` and `slope_at`

**Files:** Modify `crates/worldbuilder-engine/src/substrate.rs`

`natural` is **strict**. `slope_at` is **the module's only bounded function** — one `math.hypot` over four `local_to_sphere` calls that each reach `hypot` again. **Measure its drift over a corpus including a small steep feature and size a bound to the measurement**, stating the case that produced it. Do not borrow a bound from `features.rs` or anywhere else.

**`natural`'s slope clamp reads as dead code under a planetary scatter and saturates 8.14x over through a 140 m pinnacle.** Cover it with the pinnacle, not the scatter.

`SLOPE_BASELINE_M`'s two docstring claims were re-measured and **both hold**, including the 600 m pinnacle aliasing — flat at 130 m, steep at 300 m. Reproduce them as tests.

- [ ] **Steps:** failing tests, run, implement, run, commit.

---

### Task 4: `Substrate::at` and the host trait

**Files:** Modify `crates/worldbuilder-engine/src/substrate.rs`

Implement the trait and construction shape Task 1 settled.

**The parameter order is RESOLUTION order — elevation, tectonic, slope — not Python's keyword order (elevation, slope, tectonic).** All three are `Option<f64>`, so **a binding that maps keyword order positionally will compile and be silently wrong.**

**The three optionals use `is None` sentinels**, so a supplied `0.0` is a value and must not be treated as absent. This codebase contains three non-interchangeable idioms — an `is None` sentinel, plain default substitution, and a falsy check where `0.0` counts as absent. Match each exactly.

**Transcribe the `weight > 0.0` guard.** It is bit-observable: on the real `at()` path, guarded and ungated differ on 1.80% of points (67 of 3,721, 61x61 grid at 1,500 m on the demo coast), worst shift 2.22e-16, zero `dominant` flips.

**Iteration over `placed` preserves order** — the same reasoning as `features.rs`.

**`PURE` lookup on an empty string raises.** Implement the ruling above: reproduce the failure as a typed error, never a silent success.

- [ ] **Steps:** failing tests, run, implement, run, commit.

---

### Task 5: Bindings and conformance

**Files:** Modify `crates/worldbuilder-engine/src/bindings.rs`, `src/lib.rs`, `tests/test_conformance.py`

**Most of this module is STRICT — assert raw bits.** Only `slope_at` and anything downstream of it takes a bound, sized to its own measurement with the producing case named. **Do not borrow the features bounds**; they are aspect-ratio-driven and absolutely-bounded for reasons specific to `bump`'s support edge.

**Every corpus must include a small steep feature.** A planetary scatter reports the slope clamp as dead code.

**A feature that omits `substrate` must be in the corpus.** All 25 demo features declare one, so `if declared is None: continue` is never taken on the demo world — a corpus built from it exercises neither branch of that guard.

**Cover:** each optional supplied and omitted, including a supplied `0.0`; `total <= 0.0` through the public constructor; `dominant` at and near ties in all three precedence orders; `blended_towards` at weight exactly `0.0` and just above; an empty-string substrate failing on both sides; and `at()` with no features, one, and several.

- [ ] **Steps:** bindings, `maturin develop --release`, tests, run both suites by exit status, commit.

---

### Task 6: Record it

**Files:** Modify `crates/worldbuilder-engine/README.md`; delete `tests/test_substrate_gates.py`

- [ ] **Step 1: Record**, reading every number **from the current source, never from a report** — this caught two wrong figures in the previous slice, one inside a shipped bound's docstring. Cover: the host trait and why the cycle needed breaking; that the module is strict but for `slope_at`; the `weight > 0.0` guard with its **1.80% on the real path** and the note that the banked 19% was a different population; `dominant` returning a word with its measured tie margins; the empty-string `PURE` behaviour and the ruling; and that the slope clamp reads as dead code under a planetary scatter.
- [ ] **Step 2: Delete the throwaway.**
- [ ] **Step 3: Verify every count by running the suites and checking exit status.**
- [ ] **Step 4: Commit.**

---

## What this slice deliberately does not do

- **No `surface.py`.** The capstone, once this lands; Task 1's construction ruling decides what it inherits.
- **No deletion of the Python.** It stays the reference.
