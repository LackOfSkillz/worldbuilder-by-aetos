# Slice identity: the Python extension gets a name and a provenance

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The pure-Python reference and the Rust extension stop being the same distribution, the reference becomes installed rather than reachable by accident, and a stale extension becomes detectable.

**Architecture:** A second `pyproject.toml` under the engine crate with maturin as its backend; the root keeps setuptools. A source fingerprint embedded in the extension at build time, and a CI gate asserting it against the tree.

**Spec:** `docs/design/2026-09-02-mark-2-world-studio.md` sections 4.3 and 19.

## Why this is a slice

**The project's only oracle is not installed.** Verified from the installed metadata, not inferred:

- `.venv/Lib/site-packages/worldbuilder_by_aetos-0.1.0.dist-info/RECORD` lists
  `worldbuilder_engine/__init__.py`, the compiled `.pyd` and its `.pdb` — **and not one `worldbuilder/` file.**
- The root `pyproject.toml` declares `name = "worldbuilder-by-aetos"` with `include = ["worldbuilder*"]`,
  so the distribution named for the reference is installed carrying only the extension.
- The engine is installed **twice**, as `worldbuilder-by-aetos 0.1.0` and `worldbuilder-engine 0.0.1`.
- From any cwd outside the repo, `.venv/Scripts/python.exe -c "import worldbuilder"` raises
  **`ModuleNotFoundError: No module named 'worldbuilder'`**.

The 390-test suite passes because pytest puts the rootdir on `sys.path`. Move the runner one directory up
and 150 conformance comparisons stop existing. **This is the silent-skip bug one layer further down**, and
the CI gate that now guards against it is standing on something that is not installed.

**And `WORLDBUILDER_REQUIRE_ENGINE=1` asserts only that the module imports.** An extension built from older
source imports fine, so the suite reports `390 passed` and every gate goes green while the comparisons
measure an artifact nobody can place in history. The `.wasm` has a 28-input fingerprint and a manifest for
exactly this reason. The extension has neither.

## Global Constraints

- **`worldbuilder/` is the reference implementation and its BEHAVIOUR must not change.** Packaging metadata
  and `__init__` plumbing may change if a task needs it; no algorithm, constant or output may.
  `worldbuilder/integration/maritime.py` has a pre-existing uncommitted change; leave it unstaged.
  **Never `git commit -a`.**
- **All transcendentals through `detmath`.** No `f64::` method or associated form, no `mul_add`, no bare
  integer cast without a `// cast-ok: <reason>` marker on the same line. `abs` is exempt.
- **`cargo` is not on PATH in bash locally — use `/c/Users/gary/.cargo/bin/cargo.exe`.** It IS on PATH in
  CI; do not commit the local workaround into any script or workflow.
- **Verify by exit status, never by grepping `test result:` lines.**
- `.claude/worktrees/zen-lewin-cd32bf/` is a **full second checkout of this repo** — plain `grep -r` from
  the root double-counts.
- **Every figure names its population, its method with parameters, its host — and, for a ratio, its step.**
- **The conformance suite is the only oracle.** Any change that makes it pass more easily is a defect.

## The ruling that shapes this slice: two digests, gated against each other

The `.wasm` fingerprint is computed in JavaScript by `viewer/scripts/build-wasm.mjs::fingerprintInputs`.
The extension cannot use it: `maturin develop` does not run node, and making the provenance check shell out
to cargo would put a build into a job that currently needs none.

**So there will be two implementations of the same digest, and they will be gated against each other** —
a CI step asserts the node-computed digest equals the Rust-embedded one and fails if they differ. This is
the same shape as the count gates, which read a count stated twice in two formats and fail when the two
disagree. **Duplication that is checked is not duplication that rots.** The alternative — one
implementation, invoked from both — costs a cargo build in the cheapest job in the pipeline.

**Cost if this ruling is wrong:** the two digests drift in a way that agrees, which requires the same
mistake in two languages.

---

### Task 1: Split the distributions

**Files:** Create `crates/worldbuilder-engine/pyproject.toml`; modify root `pyproject.toml`

The engine gets its own `pyproject.toml` with `build-backend = "maturin"` and `name = "worldbuilder-engine"`
— the name its own dist-info already claims, so the tree half-agrees with itself already. The root keeps
setuptools and `worldbuilder-by-aetos` for the reference package.

**The failing test comes first and it is an installation test, not a unit test:** from a cwd outside the
repository, `import worldbuilder` must succeed and `worldbuilder.__file__` must point into the repo. Prove
it fails now (it does — `ModuleNotFoundError`), then make it pass with `pip install -e .`.

**Then prove the collision is gone:** after `maturin develop --release --features python`, the
`worldbuilder-by-aetos` dist-info must still list `worldbuilder/` files and must NOT list
`worldbuilder_engine/` ones. Assert on the RECORD, not on an import succeeding.

- [ ] **Steps:** failing installation test, run, split the metadata, run, re-run the full 390 by exit status, commit.

---

### Task 2: The extension carries its source fingerprint

**Files:** Create `crates/worldbuilder-engine/build.rs`; modify `Cargo.toml`, `src/bindings.rs`

A `build.rs` computes a digest over **the same 28 inputs** `fingerprintInputs` walks, and exposes it so the
module can return it. Add one export — `source_fingerprint()` — returning the digest as a string.

**Mirror the source script's own defence:** every input directory must contribute at least one file, and an
empty or renamed one is an error rather than a silently smaller digest. That check exists in the node script
because this repository keeps finding silent zeroes; the Rust side must not be the version without it.

**Test the failure path as hard as the success path.** A fingerprint that cannot disagree is decoration —
this project has shipped one already and caught it only by breaking it on purpose.

- [ ] **Steps:** failing tests, run, implement, run, commit.

---

### Task 3: The two digests are gated against each other

**Files:** Modify `.github/workflows/gates.yml`; create a comparison script beside `.github/scripts/`

A step that computes the node digest, reads the Rust-embedded one, and fails if they differ **or if either
is absent.** The absent half is the half that gets skipped and it is the half that matters:
`assert_counts.py` is the house pattern — read it before writing this.

**Prove it can go red, on a real run, and record the message and the run URL.** A gate whose failure nobody
has seen is a gate nobody knows the shape of.

- [ ] **Steps:** write it, prove it red on a pushed branch, restore, prove green, commit.

---

### Task 4: `WORLDBUILDER_REQUIRE_ENGINE` stops accepting a stale engine

**Files:** Modify `tests/conftest.py` or wherever the guard lives — find it, do not assume

Today the guard asserts the module imports. It must also assert the extension's fingerprint matches the tree
it is being compared against, and **fail loudly** when it does not — the failure message must say the engine
is stale, not that an import failed, because those need different fixes.

**Do not make this the default for ordinary local runs.** A developer mid-edit has a stale engine by
definition and must not be blocked by it; the strict form is what CI sets.

- [ ] **Steps:** failing test, run, implement, run, whole suite by exit status, commit.

---

### Task 5: Record it

**Files:** `README.md` (the `## CI` section), `crates/worldbuilder-engine/README.md`

**Read every number from the current source and from your own runs, never from a report.** Cover: the two
distributions and why they collided; that the reference was not installed at all and the suite passed on a
cwd accident; the two-digest ruling and what checks it; and the exact failure message each new gate produces.

**Update the README's existing account of these as deferred** — it currently says the extension has no
fingerprint and no manifest, and points at a slice that is this one. That sentence must stop being true.

- [ ] **Steps:** record, verify by running, commit.

---

## What this slice must NOT do

- **No change to the reference implementation's behaviour.** Packaging only.
- **No change to the `.wasm` fingerprint's inputs or algorithm.** This slice mirrors it; it does not move it.
- **No new gate on the viewer's browser checks.** Still out of scope, still needs a browser and a person.
- **No erosion, no climate, no cartography.**
