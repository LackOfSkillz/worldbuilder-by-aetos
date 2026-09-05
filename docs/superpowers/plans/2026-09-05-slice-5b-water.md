# Slice 5b: lakes, and the water manifest as a byproduct

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fill the basins the eroded graph already contains, resolve overflow between them, and emit the water manifest §13.2 promised maritime — deterministically, with the pond threshold **calibrated by measurement rather than chosen**.

**Architecture:** A lake super-graph over the forest's non-boundary roots, and a manifest emitted from it. `Surface` is not touched. The erosion solver from 5a is not modified.

**Spec:** `docs/design/2026-09-02-mark-2-world-studio.md` §13.2, §13.3, §13.4, §14.2, §14.4; §4.1/§4.2, §6.

## What 5a left, and what CORE-001 reserved

Slice 5a delivers an eroded height array over a `StreamGraph`. CORE-001 reserved everything this slice
populates and left it deliberately empty:

- `Lake { root_node, level_m, kind, outflow_lake }` — `level_m` is currently **the root's own elevation, an
  empty basin**, and `outflow_lake` sits at the `NO_LAKE` sentinel. The doc comments say in terms that
  slice 5 raises one and fills the other.
- `Reach { from_node, to_node, gradient }` — an **empty container**.
- `Flags::LAKE_MEMBER`, and `GraphDefect::LakeAtNonRoot` / `RootIsBothMouthAndLake` already validated.
- `BuildParams::pond_max_drainage_area_m2`, whose own comment says it is **"Not measured"** and is
  **"slice 5's to calibrate"**.

## Three rulings, taken before implementation begins

### 1. §13.4's fill-versus-breach question is DISSOLVED, not answered. Do not re-open it.

§13.4 says the choice between filling depressions and breaching them "decides how many lakes a planet has"
and waits on a measurement. **§14.2 supersedes it**: Cordonnier does neither, and a graph with explicit
receivers does not suffer the flats problem that forces raster methods to fabricate a drainage direction.

**A non-boundary root *is* a lake.** That is the whole algorithm. Any task that finds itself writing a
flood-fill pass over the terrain field has left the plan — stop and say so.

### 2. Mark 2 populates ocean, lake and pond. Rivers are SCHEMA ONLY.

§13.2 is explicit: "Rivers ship with reaches from the start, even though **Mark 2 populates only ocean, lake
and pond**." So the manifest must *carry* rivers and reaches, and this slice must **not** populate them.

§14.4's safe-to-add-later list includes reach geometry, which is the same ruling from the other side. **Leave
the `Reach` container empty**, and make the manifest's river arm expressible rather than filled.

**Waterfalls are the test of whether that schema is right.** §13.3: a fall is not a body, it is a property of
a reach — derived from bed gradient, not authored as a kind — and to maritime it is a *limit*, the upstream
end of navigability, belonging on the marks channel rather than in soundings. Mark 2 does not produce one.
**The manifest must simply not preclude one**, which is why `Reach` already carries `gradient`.

### 3. `sea_level_m` is a recorded input, and it is NOT inert

It decides which roots are mouths and which are lakes, so **the same graph yields a different manifest at a
different datum**. 5a hardcoded it inside `wb_erosion_run` as a scoping shortcut and flagged it. This slice
must thread it properly and record it in the manifest, because a manifest that does not name its datum
cannot be checked against the world it describes.

## Global Constraints

- **All transcendentals through `detmath`.** No `f64::` method or associated form, no `mul_add`, no bare
  integer cast without a `// cast-ok: <reason>` marker **on the same line**. `abs` is exempt. The guard
  `tests/no_std_math.rs` fails the build and scans all of `src/`; **it skips whole-line comments only.**
- **Never `f64::min` / `f64::max` / `clamp`** — NaN-asymmetric; `plates.rs::margin_at` is the house form.
  **The guard does NOT catch these** — they are not in its ban list, so this is enforced by review. A lake
  level is a max-over-basin and a spill point is a min-over-rim: **both are exactly where a naive one gets
  written.**
- **`extern "C"` is nounwind.** Any new export validates its inputs and returns a status. Slice 5a shipped
  two reachable aborts through exports whose bounds looked complete; both were bands rather than cliffs.
- **`worldbuilder/` must not be modified.** `worldbuilder/integration/maritime.py` has a pre-existing
  uncommitted change; leave it unstaged. **Never `git commit -a`.**
- **`cargo` is not on PATH in bash locally — use `/c/Users/gary/.cargo/bin/cargo.exe`.** Never commit it.
- **Verify by exit status, never by grepping `test result:` lines.**
- **Any edit under `src`, `examples` or `tests` moves the source fingerprint.** Re-bless with
  `npm run build:wasm`, rebuild the Python extension, and confirm `npm run check:wasm` before committing.
- **Re-derive the five engine count pins as tests that RUN — listed minus ignored.** A task in 5a pinned the
  *listed* count and would have turned every engine job red.
- **Every figure names its population, its method with parameters, its host, for a ratio its step, and where
  parameters combine into a governing group, the group.**

## There is no Python oracle. Assert properties.

`worldbuilder/` has no lakes and **must not gain one.** A reference written alongside the port, by the same
hand, tests that two expressions of one misunderstanding agree.

The properties, each of which must be *asserted* rather than assumed:

1. **Every lake's level is at or above its root's elevation, and below its spill point.** A lake filled past
   its own rim is a bug that looks like a big lake.
2. **Water flows downhill through the super-graph**: an overflow edge never points to a lake at a higher
   level. **Assert there are no cycles** — a cycle is two lakes each draining into the other.
3. **Ocean is the datum.** Every boundary root is a mouth, not a lake, at the recorded `sea_level_m`.
4. **Determinism** — same graph, same parameters, twice, **bit-identical** manifest.
5. **The pond/lake split is a threshold, not a guess** — see below.
6. **Nothing is both.** A body is ocean, lake or pond, never two. `GraphDefect::RootIsBothMouthAndLake`
   already exists for the mouth/lake half; extend the discipline.

## The pond threshold must be MEASURED, and this is the slice's own §13.4

`BuildParams::pond_max_drainage_area_m2` exists as a required parameter specifically so **"nobody inherits a
number nobody chose."** It is currently uncalibrated.

**Calibrate it by measurement and record the distribution**: how many bodies fall each side at several
thresholds, on a stated population. A threshold that puts every body on one side is not a classification.
Report what you measured, and if the distribution has no natural break, **say that** rather than inventing a
round number — that is a finding about the terrain, and it is worth more than a tidy constant.

## File Structure

- **Create** `crates/worldbuilder-engine/src/water.rs` — lake filling, the super-graph, the manifest type.
- **Modify** `crates/worldbuilder-engine/src/stream.rs` — only to populate what CORE-001 reserved.
- **Modify** `crates/worldbuilder-engine/src/lib.rs`, `wasm.rs` — declaration and any export.
- **`Surface` is not touched. `erosion.rs`'s solver arithmetic is not touched.**

---

### Task 1: Fill each basin to its spill point

**Files:** Create `src/water.rs`; modify `src/lib.rs`

Raise each lake's `level_m` from the root's own elevation to the basin's spill point — the lowest rim node
over which water leaves. **The spill point is a min-over-rim and the level is a max-over-basin**, and the
guard will not catch a naive `min`/`max`; use the house form.

- [ ] **Steps:** failing tests, run, implement, run, commit.

---

### Task 2: The lake super-graph and its overflow edges

**Files:** Modify `src/water.rs`

Fill `outflow_lake`. §14.2 puts this at **O(N + M log M)** with M far below N; if your implementation is not,
say so and give the measured cost rather than the intended one.

**Assert acyclicity by construction or by test** — two lakes each draining into the other is the failure
mode, and it is silent.

- [ ] **Steps:** failing tests, run, implement, run, commit.

---

### Task 3: Calibrate the pond threshold

**Files:** Modify `src/water.rs`; a measurement binary or example

Measure the distribution, choose the threshold from it, and record both. See above — a distribution with no
natural break is a finding, not a problem to paper over.

- [ ] **Steps:** measure, record the distribution, choose, test, commit.

---

### Task 4: The water manifest

**Files:** Modify `src/water.rs`

Ocean, lake and pond populated; **rivers and reaches expressible and empty.** Each body carries extent,
surface level and kind, and the manifest carries the `sea_level_m` it was made at.

**A manifest that cannot represent a waterfall has failed** even though Mark 2 produces none — that is §13.3's
requirement and the reason `Reach` already carries `gradient`.

- [ ] **Steps:** failing tests, run, implement, run, commit.

---

### Task 5: Parity, and a negative control

**Files:** `examples/parity_dump.rs`, `viewer/scripts/parity.mjs`, `src/wasm.rs` as needed

Native against WASM over the manifest, zero divergent, **with a control that diverges** — and one that
diverges for the right reason. 5a's erosion control moved 216 of 56,254 values while iterations and
convergence compared equal on both sides; match that discipline.

**Moves both parity count gates.** Re-derive rather than adjusting to whatever your run produced.

- [ ] **Steps:** extend the corpus, run, prove the control diverges, commit.

---

### Task 6: Record it

**Files:** `crates/worldbuilder-engine/README.md`, `parity/README.md`, `docs/ci.md` if a gate moved

**Read every number from the current source and your own runs, never from a report.** Cover: that
fill-versus-breach was dissolved rather than decided; the measured pond distribution and the threshold chosen
from it; what the manifest carries and what it deliberately leaves empty; and the measured cost of lake
resolution against §14.2's O(N + M log M).

- [ ] **Steps:** record, verify by running, commit.

---

## What this slice must NOT do

- **No flood-fill over the terrain field.** Ruling 1.
- **No river population, no reach geometry, no waterfalls.** Ruling 2 — schema only.
- **No `Surface` changes, no changes to 5a's solver arithmetic, no Python water reference.**
- **No climate, no cartography.**
