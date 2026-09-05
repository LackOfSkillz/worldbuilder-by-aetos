# Slice 5a: the erosion solver — stream power, and a cost that is measured

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Erode a `StreamGraph` by the Cordonnier stream-power equation with a thermal correction, deterministically, and **measure what it costs** instead of extrapolating.

**Architecture:** An implicit solver walking the existing receiver forest root-to-leaves, mutating a height array in place. No lakes, no water manifest, no feature kernels — those are slice 5b.

**Spec:** `docs/design/2026-09-02-mark-2-world-studio.md` §14 (especially §14.1 and §14.3), §4.1/§4.2, §6.

## Ruling: slice 5 is split, and this is the first half

The spec's build order names one slice. It carries six separable deliverables — the solver, the thermal
correction, the lake super-graph, the water manifest, the feature-kernel blend back to terrain, and the
measurement. **Splitting at the water line:**

- **5a (this plan)** — the solver and thermal correction over the existing forest, plus a real cost figure.
- **5b** — lakes and overflow, the water manifest as a byproduct, and the kernel blend that keeps
  `terrain_z_at` a function.

The boundary is chosen because 5a produces **an eroded height array and nothing else**, which is testable on
its own and is what 5b consumes. Cost if this ruling is wrong: 5b discovers the solver needs a lake-aware
inner loop and 5a's interface changes. §14.2 says lakes are handled by a **super-graph over roots**, not by
changing the per-node update, which is why that risk is judged low — but it is the risk.

## THE FINDING THAT SHAPES THIS SLICE: the solver needs no transcendental

The equation is `dh/dt = u - k * A^m * s^n` **with n = 1 and m = 0.5**.

- `A^0.5` is `sqrt`, which IEEE-754 requires to be **correctly rounded**. It is not a transcendental and
  costs no tolerance.
- `s^1` is `s`. No `pow` at all.

**So the whole solver can hold the STRICT bit-for-bit contract, not the bounded one.** That is the rarer and
stronger of this project's two conformance contracts, and it is available here for free. `hypot` is *not*
`sqrt` and must not be substituted — CPython has computed its own Neumaier-summed norm since 3.8, so the two
are different algorithms.

**If any task finds itself reaching for `pow`, `exp`, `log` or a trig function in the solver's inner loop,
stop and say so** — it means the formulation drifted from the spec's exponents, and the contract changes with
it.

The thermal correction caps slopes at 30 degrees. `tan(30°)` is **one compile-time constant**, not a per-node
call. Compute it once and record the value; do not call a trig function per node.

## Global Constraints

- **All transcendentals through `detmath`.** No `f64::` method or associated form, no `mul_add`, no bare
  integer cast without a `// cast-ok: <reason>` marker **on the same line**. `abs` is exempt. The
  build-failing guard `crates/worldbuilder-engine/tests/no_std_math.rs` enforces this and **scans all of
  `src/`**; note it is comment-blind (it skips lines whose trim starts with `//`).
- **Never `f64::min` / `f64::max` / `clamp`** — Python's two-argument forms are asymmetric under NaN;
  `plates.rs::margin_at` has the house form. **A slope cap is exactly where a naive `min` gets written.**
- **`worldbuilder/` must not be modified.** `worldbuilder/integration/maritime.py` has a pre-existing
  uncommitted change; leave it unstaged. **Never `git commit -a`.**
- **`cargo` is not on PATH in bash locally — use `/c/Users/gary/.cargo/bin/cargo.exe`.** Never commit it.
- **Verify by exit status, never by grepping `test result:` lines.**
- `.claude/worktrees/zen-lewin-cd32bf/` is a **full second checkout of this repo** — plain `grep -r` from
  the root double-counts.
- **Every figure names its population, its method with parameters, its host — and, for a ratio, its step.**
- **Any edit under `crates/worldbuilder-engine/src`, `examples` or `tests` moves the source fingerprint.**
  Re-bless with `npm run build:wasm` in `viewer/`, and the branch is not done until `npm run check:wasm` is
  green. Rebuild the Python extension too, or the suite's stale-engine guard fails.
- **The engine test counts are pinned in five matrix rows** of `.github/workflows/gates.yml` and the Python
  suite in one. **Re-derive and update any you move**, by `cargo test -p worldbuilder-engine <cfg> -- --list`,
  counting `: test$` lines and subtracting the ignored.

## THERE IS NO PYTHON ORACLE. ASSERT PROPERTIES, NOT EQUALITY.

Like slice 1p, and for the same reason: `worldbuilder/` has no erosion. **Do not add one** — a reference
written alongside the port, by the same hand, on the same day, tests nothing.

The properties that matter, and each must be asserted rather than assumed:

1. **Determinism.** Same seed, same parameters, same iteration count → **bit-identical** height arrays.
   `StreamGraph::bit_identical_to` exists; use the same discipline.
2. **Native and WASM agree**, bit-for-bit, with a **negative control** that diverges when one constant is
   perturbed. A parity harness that cannot show divergence proves nothing — this project has shipped several.
3. **Convergence.** §14.3 says 100-300 iterations and that **the count does not depend on resolution.**
   That is a testable claim: measure iterations-to-convergence at several node counts and report whether it
   holds. **If it does not hold, say so** — the spec's figure comes from a paper, on a different domain.
4. **Mass behaviour.** Erosion without uplift must not raise the maximum height, and must not produce NaN or
   infinities anywhere. Assert over the whole array, not a sample.
5. **The thermal cap binds.** After correction, no edge exceeds 30 degrees — assert the invariant the
   correction exists to establish, not merely that the code ran.

## The cost figure must be MEASURED, and the spec says so in terms

§14.3's numbers are explicitly labelled arithmetic, not measurement: 160,000 nodes at ~200 steps took
**252 seconds on a 2016 desktop**, extrapolated to ~64 nodes/km², a 20 M-node planet at 5 km spacing, and
"plausibly many hours" single-threaded.

**Replace the extrapolation with a measurement.** Report wall-clock at several node counts on this host,
naming the population, the method with its parameters, the host, and — since this is a scaling claim — the
step between sizes. §14.3 states their per-iteration cost scaled **worse than linearly (1.8x the nodes cost
3.2x the time)**; measure this implementation's exponent and compare. Do not assume it matches.

**A 20 M-node run does not fit a 32-bit WASM heap** (1.45 GB of arrays, 2.16 GB peak RSS, from slice 1p).
Do not attempt one in WASM. Native only, and if the largest size you can run on this host is smaller than
planetary, **say what you ran and do not extrapolate past it in the same breath.**

---

### Task 1: Uplift and erodibility as explicit inputs

**Files:** Create `crates/worldbuilder-engine/src/erosion.rs`; modify `src/lib.rs`

The equation needs `u` (uplift) and `k` (erodibility) per node. Define how they are supplied and **make them
part of the recorded parameters**, because two runs that differ in either are not comparable and a stored
graph must say which it was baked with.

§14.4's "safe to add later" list names uplift and erodibility as **recomputable**, so they need not be stored
per node — but the *parameters that generate them* must be.

- [ ] **Steps:** failing tests, run, implement, run, commit.

---

### Task 2: The implicit solver, one iteration

**Files:** Modify `src/erosion.rs`

One step of `dh/dt = u - k * A^0.5 * s`, implicit, walking the forest **root to leaves** so each node's
receiver is already updated. `StreamGraph::peel()` gives the order; do not write a second traversal.

**Assert the strict contract here**: no transcendental in the path, so the test compares bit patterns, never
a tolerance. A tolerance in this file is a defect.

- [ ] **Steps:** failing tests, run, implement, run, commit.

---

### Task 3: Iterate to convergence, and measure whether the count is resolution-independent

**Files:** Modify `src/erosion.rs`

Define convergence explicitly — a threshold on maximum height change, recorded with its units — and iterate.
**Then test §14.3's claim** at several node counts and report the answer either way.

- [ ] **Steps:** failing tests, run, implement, run, measure, commit.

---

### Task 4: The thermal correction

**Files:** Modify `src/erosion.rs`

Cap slopes at 30 degrees. **One constant, computed once.** Assert the invariant holds afterwards over every
edge, and assert it can fail — construct a graph that violates it and confirm the uncorrected solver produces
the spikes §14.1 says it produces, so the correction is demonstrably doing something.

- [ ] **Steps:** failing tests, run, implement, run, commit.

---

### Task 5: Parity, with a negative control

**Files:** Modify `crates/worldbuilder-engine/examples/parity_dump.rs`, `viewer/scripts/parity.mjs` as needed

Native against WASM over an erosion corpus, zero divergent — **and a negative control that diverges** when
one constant is perturbed, reporting how many values moved. A parity run that cannot go red is decoration.

**This moves the corpus size**, which two count gates pin. Re-derive and update them.

- [ ] **Steps:** extend the corpus, run, prove the control diverges, commit.

---

### Task 6: Record it

**Files:** `crates/worldbuilder-engine/README.md`, and `docs/ci.md` if a gate moved

**Read every number from the current source and from your own runs, never from a report.** Cover: the strict
contract and why it is available here (m = 0.5 is `sqrt`, n = 1 is nothing); the measured cost and scaling
exponent against §14.3's extrapolation; whether the resolution-independent iteration count held; and what
5b still owes — lakes, the water manifest, and the kernel blend.

**Do not quote a figure from this plan.** Every number here is a snapshot and this project has already
carried a stale digest from one brief into the next.

- [ ] **Steps:** record, verify by running, commit.

---

## What this slice must NOT do

- **No lakes, no overflow super-graph, no water manifest.** Slice 5b. The `Lake` table and `outflow_lake`
  sentinel already exist from CORE-001; leave them at their sentinels.
- **No feature-kernel blend back to terrain**, and **no `Surface` changes.** Slice 5b.
- **No Python reference implementation of erosion.**
- **No planetary-scale bake in CI.** Measure locally; CI runs the small cases.
- No climate, no cartography.
