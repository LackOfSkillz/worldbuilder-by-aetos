# Slice 1o: Surface Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port `worldbuilder/terrain/surface.py` to `crates/worldbuilder-engine/src/surface.rs` — the capstone that composes every field already ported. **This closes the engine core.**

**Architecture:** One class, four methods, an eight-field constructor, and a single multiply. Its content is not arithmetic but *ordering*: continentality, tectonics, shelf, features, detail — with substrate hanging off the side and re-entering at `structural_m`.

**Tech Stack:** Rust (engine), PyO3 bindings, pytest differential harness against the Python reference.

**Spec:** `docs/design/2026-09-02-mark-2-world-studio.md`, sections 4.1 (bit-equality), 4.2 (DETERMINISM-001), 6 (VERSION-001).

## Global Constraints

- **All transcendentals through `detmath`** (libm-backed). No `f64::` method or associated form, no `mul_add`, no bare integer cast without a `// cast-ok: <reason>` marker **on the same line as the cast**. A build-failing guard test enforces this. `abs` is exempt and used bare.
- **Constants transcribed character-for-character.** (This module has none — see below.)
- **Never `f64::min` / `f64::max` / `clamp`.** Python's two-argument forms are asymmetric under NaN. Every clamp is an explicit `if`/`else` in the Python's operand order; `plates.rs::margin_at` has the house form.
- **Nothing under `worldbuilder/` may be modified.** `worldbuilder/integration/maritime.py` has a pre-existing uncommitted change; leave it unstaged.
- **Verify by exit status, not by grepping `test result:` lines.**
- **`cargo` is not on PATH in bash — use `/c/Users/gary/.cargo/bin/cargo.exe`.** `cargo clippy` and `cargo fmt` shims exist but the components are not installed for this toolchain.
- **Every figure names its population, its method including the method's parameters, and its host.** Thirteen numbers were overturned across the two previous slices; one was narrowed seven times, the last two narrowings being an unnamed bisection parameter and an unnamed machine.

---

## This slice's failure mode is STRUCTURAL, not numeric

**Nothing in this file reorders into a last-bits difference.** Every physically possible reordering was measured on a 625-point demo-coast grid (±45 km, 3,750 m/step, 25 features, seed 20260831, CPython 3.11.0 MSC v.1933 on K2SO, identical under both system and venv interpreters).

**The reduction and the grid's orientation, which this table did not name and which are the whole
disagreement.** The figure is the **maximum** of `abs(truth - mutant)` over the 625 points — not the mean,
which is three orders smaller (0.189 / 0.0099 / 0.0666 / 0.00038 m), and not the RMS. And the 625 points
are `Coast.at(offshore, along)`, so the square is **rotated by the demo coast's `SEAWARD_DEG = 296.49`**
about the anchor; it is *not* `TangentFrame.at(origin).local_to_sphere(east, north)`. Both were tried in
the fix round. On the unrotated frame the same four maxima are **15.969 / 9.347 / 12.994 / 0.0841 m** — a
different set of 625 points, so a different set of extrema, from one unnamed parameter. Re-derived and
reproducing to the last digit with the reduction and the orientation named:

| Mutation | Moves the answer by | Reading |
|---|---|---|
| Features before shelf (**SWAP**) | **30.89228988262422 m** | full pipeline, max over the `Coast.at` grid |
| Detail before features | **5.463671791248579 m** | full pipeline, max over the `Coast.at` grid |
| Dropping the authority multiply | **11.744069415078535 m** | full pipeline, max over the `Coast.at` grid |
| Sizing detail off pre-feature ground | **0.04541089914697238 m** | full pipeline, max over the `Coast.at` grid |

**"Features before shelf" names TWO mutations, and this row is the SWAP.** *SWAP*: both stages present,
order exchanged — the features run over the macro elevation and the shelf's lerp then runs over that
result. *DROP*: the shelf deleted, the features composing onto `land.base_elevation + tectonics.offset_m`
and that being the answer. This row is the SWAP, which is what `surface.py` describes; the DROP over the
same grid is **60.53077243225693 m** full-pipeline and **59.70936673990978 m** structure-only. Four agents
measured this correctly and reported four different numbers because that phrase was left to carry both.

**The extraction's table was two experiments read as one.** Its features-before-shelf figure,
`30.913586988571197`, is reproduced exactly by a **structure-only** reading — `abs(swapped - shaped)`,
with no detail stage — while its other three rows are full-pipeline. **The conformance target is the
full-pipeline reading**, because that is what a wrongly-ordered `surface.rs` would actually compute.

**These mutations do not all live in the same method.** `structural_m` is
`features.apply(point, shelf.elevation_m(point))[0]` — it has no detail stage and **no authority
multiply** — so only *features before shelf* is a mutation of it. The other three belong to `elevation_m`
and are **Task 4's** to catch.

So a conformance suite built only on ULP bounds would be aiming at the wrong target. **Get the order right and the numbers follow; get it wrong and no tolerance hides it.** Test the structure.

## Two exact invariants to pin, rather than only testing end-to-end

Both measured 625/625 bit-identical:

1. **With no features, `structural_m` is bit-identical to `shelf.elevation_m`.**
2. **`elevation_m(p) == structural_m(p) + detail.offset_m(p, amplitude, resolution_m)`**, bit-for-bit.

These are far stronger than a bounded end-to-end comparison, and they localise a defect to a stage instead of reporting that the total is wrong.

## The measured transcendental map

All three public methods reach exactly `{asin, atan2, cos, hypot, sin, sqrt, tanh}`, measured by bombing one `math` name at a time.

- **`atan2` enters only via `Placed::weight_at → sphere_to_local`** — proven by a bare-world control where it vanishes entirely.
- **`bottom_at` carries five `hypot` calls that a `weight_at`-shaped assumption would miss**, because `slope_at` uses `local_to_sphere` (`hypot`+`cos`+`sin`+`sqrt`), not the cheap direction. Assuming the two frame methods matched was an error in an earlier slice; do not repeat it.

`sqrt` is IEEE-754 correctly rounded and costs no bound. `hypot` is the opposite — CPython has computed its own Neumaier-summed norm rather than calling libm since 3.8.

---

## The hardest problem: one seed, three consumers, and a banned token

`surface.py:57-61` hands a single `world_seed` to `plates_for(i64)`, `Continentality::new(u64)` and `Detail::new(u64)`. **Every existing binding dodges this by taking two seed parameters. `Surface::new` cannot.**

**The cast is faithful for TWO of the three consumers and WRONG for the third.** Measured over 2,049
negative seeds — 1,000 dense, 25 structured extremes, 1,024 random draws — `s & (2^64-1)` is bit-identical
to `s` through `_lattice` (18,441 pairs), the mixed `Noise.seed` (6,147) and `Noise.at` (30,735), with
zero mismatches, and matches the Rust crate across the FFI boundary too. Not a tautology: all 2,049 give a
negative, unbounded `Noise.seed` *before* the mask.

**But `plates_for` keys a decimal string via `_fraction`, so masking changes the plates — in 64 of 64
seeds tested.** Worst distance Rust-`i64` against Python on the negative seed is `2.2e-16`, ordinary libm
noise; on the *masked* seed it is `0.387`.

**So `Surface::new` holds `i64` and casts once, at the two `Noise`-backed sites ONLY.** Casting at
`plates_for` would produce a different world while looking like consistency. `tests/no_std_math.rs` bans
the literal token `" as u64"`, so this needs a `// cast-ok:` marker, and its two
load-bearing words are **"AFTER"** (the mask follows the mixing) and **"reinterpretation, not a float
truncation"**.

**Correction, measured against source: this is NOT "the crate's first substantive `// cast-ok:` marker",
as an earlier draft of this plan claimed.** There are **31** `cast-ok:` marker lines in
`crates/worldbuilder-engine/src/`, and `noise.rs` already performs the same signed-to-unsigned
reinterpretation for the same hash. What is actually distinctive about this one is narrower and more
useful: it is the only marker in the crate whose justification rests on a **measured population** (2,049
negative seeds) rather than on the shape of the expression, and the only one where the identical cast one
function further along — `plates_for` — would have been **wrong**. "Signed to unsigned is safe here" is a
claim about a call site, not about a cast.

**Do NOT take the alternative of a `u64` signature.** It silently drops the negative-seed domain that
`generation.rs` is already conformance-tested over.

**An unstated domain narrowing, recorded rather than hidden:** an `i64` signature limits Python's
unbounded `int` seed to `[-2^63, 2^63)`, and **no 64-bit type can do better** — `plates_for(2**64+7)`,
`plates_for(10**30)` and `plates_for(-(2^63)-1)` all differ from their masked forms.

---

## Slice 1n's prediction: two-thirds held, and the third must be named

Slice 1n predicted `surface.rs` would pay "three forwarding methods, no new field, and one indirect call per structural probe."

- **"No new field" HELD, and is now provable** — every callback closes over `&self`, and nothing in the crate needs `&mut self` because both noise caches were deliberately dropped.
- **"One indirect call per structural probe" HELD but UNDER-COUNTED.** `substrate::at` takes *two* callbacks, and `bottom_at` pays **6 indirect calls, 5 of them structural** — four in `slope_at`, one for the elevation resolution, one for tectonics. The README's census is already right; it is the slice-1n task-1 report that says "four".
- **"Three forwarding methods" DOES NOT HOLD as a transcription.** `surface.py` exposes exactly **one** substrate-facing method: `bottom_at`. Three forwarders is defensible as an API decision — Python callers reach through `world.substrate.at` / `slope_at`, and Rust has no object to reach through — but it is a **deliberate widening beyond what `surface.py` declares**. If it is done, it must be labelled as an API choice in the source, or a later reader will file it as a transcription error.

---

## Three things not to transcribe, and one not to "fix"

**1. The class docstring is false of the port.** It says `Surface` holds "two noise lattices that fill themselves in as they are used". True of the Python; **false of the Rust**, since the README records both caches deliberately dropped. Rewrite it; do not carry it over.

**2. The slice-1n task-1 report's Rust sketch contradicts its own prose.** The sketch orders `substrate_at`'s optionals `(elevation_m, slope, tectonic_m)`; its prose two paragraphs later rules for **resolution order** `(elevation_m, tectonic_m, slope)`. The shipped `substrate.rs` follows the prose and is correct. **Nobody should "fix" the code to match the sketch.**

**3. This module has zero constants and zero module-level functions.** There is nothing to transcribe character-for-character here; if a task finds itself adding constants, it has misread the file.

---

## Two discrete decisions found in new material

**1. The `isinstance(features, Features)` branch adopts a pre-built `Features` VERBATIM — including its own `radius_m`.** Measured: a `Features` built at 1,234,567 m keeps every frame and every `_cos_reach` at that radius **inside a 6,371,000 m world**. **The branches do not converge.** Transcribe the branch exactly; do not normalise the radius.

**2. `Detail::offset_m`'s `amplitude_m <= 0.0` guard is armed by the authority multiply, and only on features.** `authority` reaches exactly `1.0` at **24 of the 25 demo feature centres**, driving `amplitude` to exactly `0.0`. The guard is bit-observable one level down (`+0.0` versus `-0.0`) at all 24.

---

## The corpus axis this slice introduces, and one case that is OPEN

The previous slice's blind corpora were **steep versus gentle**. This slice's are **on-feature versus between-features**, and that is a different axis entirely:

- The `amplitude_m <= 0.0` guard fires at **24 of 25 feature centres**.
- It fires at **0 of 625 grid points**.
- **No grid, however dense, lands on a feature's exact centre.**

So a corpus must include feature centres *explicitly*, not by sampling more finely.

**The open case is now CLOSED, by measurement rather than argument.** `amplitude_m` has a strictly
positive floor — minimum `4.500000000000001` over 71,190 evaluations — so the guard fires only when
`authority` is exactly `1.0`, which needs `abs(lift) >= 2.999999988824129`. Meanwhile `shaped == -0.0`
requires every applying feature to contribute exactly `-0.0`. Both legs were constructed separately —
`-0.0` in 95 of 4,335, the guard fired in 348, and 867 of 867 in a two-feature co-occurrence hunt — and
**the intersection is empty in both sweeps**. A further 400,000 draws never *arrived* at `-0.0` from a
non-`-0.0` input, and the shelf never returns an exact signed zero.

**So `surface.rs` needs no guard of its own, and `detail.rs` must keep its.** The closure has **three**
named dependencies, and any change reopens it: every roughness constant in `detail.py` staying strictly
positive; `Features.apply` initialising `result = elevation_m` and `continue`-ing before the authority
update; and **`shelf_weight` staying within `[0,1]`** — outside that range `amplitude_m` goes non-positive
in 66,594 of 200,000 draws. The third was found by review, not by the closure's author.

**Neither population is a superset of the other.** The grid shows 0 of 625 guard fires; the centres show
24 of 25 — and the centres collapse the budget's smallest row, sizing detail off pre-feature ground, from
`4.5 cm` to `6.5e-4 m`. **The conformance corpus needs both.**

---

## File Structure

- **Create** `crates/worldbuilder-engine/src/surface.rs`, declared in `src/lib.rs`.
- **Modify** `crates/worldbuilder-engine/src/bindings.rs` and `src/lib.rs`.
- **Modify** `tests/test_conformance.py`.
- **Modify** `crates/worldbuilder-engine/README.md`.

**Read every existing module from the shell** with `cat`/`grep` — the `Read` tool returned stale content for `features.rs` twice in an earlier slice. Note also that `.claude/worktrees/zen-lewin-cd32bf/` is a **full second checkout of this repo**, so a plain `grep -r` from the root double-counts; and the `git grep` pathspec artefact reproduces here pointing the *unquoted* way. **Cross-check any search with a second tool.**

---

### Task 1: Settle the seed, the open `-0.0` case, and the ordering budget

**Files:** Create `tests/test_surface_gates.py` (throwaway; deleted in Task 6)

No engine code.

- [x] **Step 1: Prove the seed cast.** Establish by measurement that Python's `_lattice` masks after mixing, so a negative `world_seed`'s masked result equals the wrapping `u64` result. Give the exact wording for the `// cast-ok:` marker.
- [x] **Step 2: Construct the open `-0.0` case or close it.** Find an input where `shaped == -0.0` reaches `Detail::offset_m`'s guard, or **measure** that none exists. An argument is not acceptable here.
- [x] **Step 3: Re-derive the four reordering figures** on a named population, method and host. They are the budget the conformance suite is aiming at, and they are the reason this slice tests structure.
- [x] **Step 4: Confirm both exact invariants** — no-features `structural_m` against `shelf.elevation_m`, and `elevation_m == structural_m + detail.offset_m` — bit-for-bit, and say over what population.
- [x] **Step 5: Record all four answers in the ledger with their populations, methods and host.**

---

### Task 2: `Surface::new` and the field layout

**Files:** Create `crates/worldbuilder-engine/src/surface.rs`; modify `src/lib.rs`

**Interfaces:** the constructor and its eight fields.

**EIGHT, not nine.** Python's ninth attribute is `self.substrate = Substrate(self)`, and `substrate.rs` deliberately has no `Substrate` type. `bottom_at` calls the free `substrate::at` with `&self` callbacks instead.

**The seed cast lands here**, with the `// cast-ok:` marker Task 1 worded. **Keep the `i64` signature.**

**Transcribe the `isinstance(features, Features)` branch exactly** — a pre-built `Features` is adopted with its own `radius_m`, and the branches do not converge.

**Rewrite the class docstring.** Its noise-lattice claim is false of the port.

- [ ] **Steps:** failing tests against the live Python, run, implement, run, whole crate suite by exit status, commit.

---

### Task 3: `structural_m` and the composition order

**Files:** Modify `crates/worldbuilder-engine/src/surface.rs`

**Order is the content of this module.** Continentality, tectonics, shelf, features — and the authority multiply. Reordering costs metres, not bits.

**Pin the exact invariant**: with no features, `structural_m` is bit-identical to `shelf.elevation_m`.

- [ ] **Steps:** failing tests, run, implement, run, commit.

---

### Task 4: `elevation_m`, `bottom_at`, and the callbacks

**Files:** Modify `crates/worldbuilder-engine/src/surface.rs`

**Pin the second exact invariant**: `elevation_m == structural_m + detail.offset_m`, bit-for-bit.

`bottom_at` pays **6 indirect calls, 5 structural**. `substrate::at` takes **two** callbacks. `slope_at` uses `local_to_sphere` — `hypot`+`cos`+`sin`+`sqrt`, not the cheap direction.

**If forwarding methods are added beyond `bottom_at`, label them in the source as an API decision**, not a transcription.

- [ ] **Steps:** failing tests, run, implement, run, commit.

---

### Task 5: Bindings and conformance

**Files:** Modify `crates/worldbuilder-engine/src/bindings.rs`, `src/lib.rs`, `tests/test_conformance.py`

**Aim at structure.** The two exact invariants are the strongest assertions available and must both be present. Bounded end-to-end comparisons are secondary and must be **measured, never borrowed** — a previous slice borrowed a bound that was 227x looser than its field needed and hid a real defect.

**The corpus must include feature centres explicitly.** The `amplitude_m <= 0.0` guard fires at 24 of 25 of them and at 0 of 625 grid points, and no grid however dense lands on a centre.

**Cover:** a bare world with no features; a world adopting a pre-built `Features` at a *different radius*; feature centres; the `authority == 1.0` case; a negative `world_seed`; and whatever Task 1 established about `shaped == -0.0`.

- [ ] **Steps:** bindings, `maturin develop --release`, tests, run both suites by exit status, commit.

---

### Task 6: Record it, and close the engine core

**Files:** Modify `crates/worldbuilder-engine/README.md`; delete `tests/test_surface_gates.py`

- [ ] **Step 1: Record**, reading every number **from the current source, never from a report** — this rule caught two wrong figures in one previous slice and a seventh narrowing in the next. Cover: the composition order and what each reordering costs; the two exact invariants; the seed cast and its derivation; the indirect-call census; the `radius_m` adoption branch; the on-feature versus between-features corpus axis; and whatever became of the open `-0.0` case.
- [ ] **Step 2: Note that this closes the engine core** — every module `surface.py` composes is now ported.
- [ ] **Step 3: Delete the throwaway.**
- [ ] **Step 4: Verify every count by running the suites and checking exit status.**
- [ ] **Step 5: Commit.**

---

## What this slice deliberately does not do

- **No climate or land-cover layer.** That is designed but unapproved; see `docs/design/2026-09-03-roadmap-additions.md`.
- **No viewer, no studio.** Slices 2 and 3.
- **No deletion of the Python.** It stays the reference.
