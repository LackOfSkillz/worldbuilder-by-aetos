# CI

`.github/workflows/gates.yml` runs on every push, on `windows-latest`, pinned to
`rustc 1.98.0`. Seven jobs: the engine suite in five feature configurations
(`--no-default-features`, default, `python`, `wasm`, `python,wasm`), Python conformance, and
one provenance-plus-parity job. Nothing is cached, and no artifact is rebuilt before it is
checked - the provenance job compares the *committed* `.wasm` against the source in the same
commit.

This exists because four things went wrong here and each survived multiple commits, because
nothing ran the check that would have caught it:

1. **The conformance suite skipped silently and reported green while comparing nothing.**
   `tests/test_conformance.py` falls through to `pytest.importorskip` when
   `worldbuilder_engine` cannot be imported. With no engine built and no guard set,
   `pytest tests/` exits 0 having compared nothing - all 157 tests in that file collapse into
   one `1 skipped` line. Nobody scanning a green log for a suspicious number finds one,
   because there isn't one. **That is the row the CI slice exists to make impossible**, and
   it is the reason the conformance job sets `WORLDBUILDER_REQUIRE_ENGINE=1`: with the guard
   set, a missing engine is a hard `ModuleNotFoundError` at collection, exit 2, not a skip.
2. **The shipped `.wasm` was several commits stale and still passed parity.** Nothing
   compared the artifact's provenance against the source before running comparisons through
   it, so a coastline change could ship with an engine built from an earlier one and parity
   would still report zero divergent - correctly, because the corpus and the artifact agreed
   with each other and with nothing else.
3. **The provenance guard itself was unreproducible from git and failed on every machine but
   its author's.** The fingerprint folds the exact toolchain string (release, commit hash,
   host triple) into the digest, so a check written against one machine's Rust could never
   pass anywhere else without pinning both the toolchain and the runner OS. Gates now run only
   on `windows-latest` at `rustc 1.98.0`, for this reason.
4. **Two Python distributions shared one name, so building either evicted the other's files
   - and nothing noticed, because `pytest` papered over it with an accident of its own.**
   Before the identity slice, both `pyproject.toml` (root, setuptools) and the not-yet-split
   engine crate declared `name = "worldbuilder-by-aetos"`. `pip install -e .` (the pure-Python
   reference) and `maturin develop` (the Rust extension) therefore targeted the *same*
   dist-info, and whichever ran last evicted the other's `RECORD` entries. The installed
   metadata proved it directly: `worldbuilder_by_aetos-0.1.0.dist-info/RECORD` listed
   `worldbuilder_engine/` files and not one `worldbuilder/` file, and a fresh
   `import worldbuilder` raised `ModuleNotFoundError` from any cwd outside the repository.
   **The conformance suite passed anyway, and for no other reason than pytest putting the
   repository root on `sys.path`** - every test in the suite happened to run with the repo
   root as its working directory, so the reference package resolved via that path accident
   regardless of whether it was actually installed. `tests/test_installation.py` is the
   regression test: it imports `worldbuilder` from a subprocess whose cwd is a temp directory
   outside the repository, where the accident cannot reach.

   **The fix is a real split, not a workaround.** `crates/worldbuilder-engine/pyproject.toml`
   now declares its own distribution, `name = "worldbuilder-engine"` (maturin backend); the
   root `pyproject.toml` keeps `worldbuilder-by-aetos` and restricts
   `[tool.setuptools.packages.find]` to `include = ["worldbuilder*"]`,
   `exclude = ["worldbuilder_engine*"]`. The two installs no longer share a dist-info, so
   `pip install -e .` and `maturin develop` no longer evict each other. CI's install order
   (`pip install -e .` before `maturin develop`) is unchanged from before the split and is
   *not known to still be load-bearing* - the reviewer's structural argument (pip keys on
   project name; the RECORDs are name-isolated; the engine's `pyproject.toml` declares no
   `python-source` so maturin cannot see `worldbuilder/`) was not verified by reversing the
   order, because doing so needs a `pip uninstall` nobody has run. The order is kept
   deliberately, not because it is confirmed necessary.

## The six gates, and the message each one produces on a real failure

Every message and count below was read from the gate's own source or reproduced by running
it on the current tree (HEAD `1670b32` on `slice-identity`), not copied from an earlier
report.

**1. Engine suite (five feature configurations).** `cargo test -p worldbuilder-engine
<features> --no-fail-fast`. A broken constant fails with the ordinary libtest message, e.g.
`assertion left == right failed / left: 3.5 / right: 3.0`. `--no-fail-fast` is load-bearing:
without it, a failing unit test stops cargo before `tests/no_std_math.rs` (gate 2) ever runs,
which is exactly the shape of gate 1 silently hiding gate 2 that the CI slice exists to
prevent.

**2. The determinism guard**, `crates/worldbuilder-engine/tests/no_std_math.rs`, runs as part
of the engine suite above. It scans `src/` for `f64::`-style float math, `mul_add`, or an
unmarked truncating cast, and fails with: `std float maths (or an unmarked float-truncating
cast) found outside detmath: <file>:<line>: <form> — route it through detmath (or mark with
` `// cast-ok:` ` if this is a genuine integer cast)`.

   **Known limitation, recorded rather than fixed:** the scanner is comment-blind by design -
   `scan_text` treats any line whose trimmed text starts with `//` as a comment to skip
   (`crates/worldbuilder-engine/tests/no_std_math.rs:60`), and `///` and `//!` both start with
   `//`. A banned form written inside a doc comment compiles to nothing and is invisible to
   the guard. This is correct behaviour for an actual comment and a trap for anyone trying to
   prove the guard still works: it must be tested on a code line, not a doc line. Not a
   defect; recorded so nobody "fixes" it into false positives on real doc comments.

**3. Python conformance, missing-or-stale engine.** `tests/test_conformance.py`, gated by
`WORLDBUILDER_REQUIRE_ENGINE=1` in CI, now rejects two different failures with two different
messages instead of one:

   - **Missing engine** (the row the original CI slice closed): `worldbuilder_engine` fails
     to import, and the module-level guard calls `pytest.fail` with
     `WORLDBUILDER_REQUIRE_ENGINE is set but 'worldbuilder_engine' did not import (<the
     ImportError>). Build it: 'maturin develop --release --features python' in
     crates/worldbuilder-engine/.`
   - **Stale engine** (the row the identity slice closes): the engine imports fine but was
     built from different source than the tree checked out now. `_require_current_engine`
     (`tests/test_conformance.py:99`) raises `EngineFingerprintStale`, and the module-level
     guard fails the run with its exact text, verbatim from the source:

         worldbuilder_engine is STALE: it was built from a different tree than the
         one currently checked out.
           engine's embedded fingerprint : <hex>
           manifest (MANIFEST.txt) fingerprint: <hex>
         Rebuild it from the current tree: `maturin develop --release --features
         python` in crates/worldbuilder-engine/. If the manifest fingerprint looks
         wrong instead, the manifest itself may be stale -- `npm run build:wasm` in
         viewer/ regenerates it, and `npm run check:wasm` tells you if it already was.

     A third failure mode, the oracle itself being unreadable (missing or malformed
     `MANIFEST.txt`), raises `EngineFingerprintUnavailable` instead and is never read as
     agreement - there is no fall-through from "could not check" to "fingerprints matched".

   **The composition this relies on, stated explicitly.** `_require_current_engine` compares
   the engine's *embedded* fingerprint against the one written in
   `viewer/public/wasm/MANIFEST.txt`. That comparison answers "is the installed engine
   current" only because a *separate* gate (gate 5, `npm run check:wasm`) independently
   asserts the manifest itself is current with the tree. Neither check alone proves the
   installed engine matches the tree it is being compared against: read the manifest's digest
   without gate 5 and you are trusting a number nothing has verified; run gate 5 without this
   guard and a locally-stale `.venv` still imports and reports green. **This project has
   already been bitten by exactly this shape once** - the `.wasm` itself passed parity for
   several commits while stale, because parity checked the artifact against a corpus that was
   stale in the same way, and nothing checked either against the *tree*. Saying the
   composition out loud is the only difference between a guard that closes the gap and one
   that quietly reopens it the day either half is skipped.

   With the guard *unset* and no engine built - the historical, buggy configuration -
   `pytest tests/` still exits 0. Reproduced locally on the current tree by hiding the
   installed extension: `pytest tests/` prints **`241 passed, 1 skipped`**, all 157 tests in
   `tests/test_conformance.py` collapsing into that one line (`241` is the other thirteen
   pre-existing files' 240 tests plus the 1 in `tests/test_installation.py`, added by this
   slice's Task 1). This is the bug the CI slice exists to make impossible, and it is why the
   count gate below asserts the per-file total, not just exit status.

**4. Fingerprint parity (identity slice, CI-only).**
`.github/scripts/assert_fingerprint_parity.py`, run once per CI push in the `python
conformance` job right after the engine wheel is built. It asserts that two *independent*
implementations of the same source-fingerprint algorithm agree: `viewer/scripts/build-wasm.mjs
digest` (Node, the ground truth the `.wasm` provenance already trusted) against
`worldbuilder_engine.source_fingerprint()` / `source_fingerprint_inputs()` (Rust, computed by
`crates/worldbuilder-engine/build.rs` + `build_fingerprint.rs` and exposed as PyO3 exports
added by this slice's Task 2/3).

   **The ruling that allows this duplication, stated in the gate's own docstring:**
   *"duplication that is checked is not duplication that rots."* A second implementation of
   one digest algorithm is exactly the kind of drift-prone duplication this project avoids
   elsewhere - permitted here only because this gate exists to catch the two implementations
   disagreeing, and it runs on every push.

   **The agreement is not assumed - it was demonstrated, twice, independently.** The Rust
   implementation reproduced Node's digest
   (`64d7e7c3044165bcfb898efa2b9f79c9bd9208b2e53e6b7495784803ebab8b60` over 28 inputs, before
   this slice's later work added a 29th) **exactly, on the pristine tree, on first attempt** -
   none of the four traps a hand-rolled walk-and-hash routinely hits (joiner character,
   trailing newline, sort key, two-space separator) bit. A reviewer independently re-derived
   that same agreement from scratch: checked out the pre-Task-2 commit into a separate
   worktree, compiled the shipped `build_fingerprint.rs` into a scratch crate outside this
   repository's own build, and got the identical digest over the identical 28 inputs.

   Re-run on the current tree (HEAD, both sides): node reports `source-fingerprint:
   029396ea75bc6eb10e1006a6063578829399e88b7525eb5b4abc44b2fef839b2` /
   `fingerprint-inputs: 29`; the just-built extension's `source_fingerprint()` /
   `source_fingerprint_inputs()` report the same digest and count; the gate exits 0 with
   `count OK: node and rust fingerprints agree on a real digest over a real corpus (29
   inputs)`. On a real disagreement it fails loudly rather than comparing shapes that merely
   happen to be equal - every value is validated as 64-hex-lowercase / positive-integer before
   either side is compared to the other - with:

       FINGERPRINT PARITY GATE FAILED
         the two digests disagree:
               node (build-wasm.mjs):      <hex>
               rust (worldbuilder_engine): <hex>

**5. Provenance**, `npm run check:wasm` in `viewer/`. Source edited without a rebuild:
`STALE ARTIFACT: - the shipped .wasm was NOT built from the source that is here now: source
now: <hash> / artifact built from: <hash> (29 inputs fingerprinted.)`. Re-run locally against
the current tree: `Current: .../viewer/public/wasm/worldbuilder_engine.wasm matches its
manifest and the source that is here now.` - confirmed by running the check, not read off a
report. The input count moved from 28 to 29 in this slice: `tests/build_fingerprint.rs`, a
new file under one of the three walked directories, is itself a fingerprinted input, and a
file *appearing* moves the digest exactly as a file *changing* does.

   Two cheaper proofs run alongside it, both re-run locally as part of this task:
   `npm run build:wasm:stale-self-test` (`SELF-TEST PASSED: the fingerprint refuses a source
   tree that has moved.`) and `npm run build:wasm:self-test` (rejects a stripped 327-byte,
   memory-only artifact before rebuilding the real one).

**6. Parity**, `parity_dump` (native) replayed through the committed `.wasm` by `parity.mjs`.
A mutated artifact is refused rather than silently compared: `REFUSING TO REPORT PARITY --
STALE ARTIFACT:` with the source/artifact hashes, because "the corpus and the .wasm agree
with each other and with nothing else" is not evidence. Re-run on the current tree
(slice 5a added the `erosion/erosion` group through `wb_erosion_run`, which the count below
now includes -- re-derived, not carried forward from before that export existed):
`parity: 56254 values compared through the shipped exports, 0 divergent`, exit 0. A control
run (`--mutate seed`) proves the harness can fail at all: of the same 56,254 values, 53,778
diverge. A second control, `--mutate erosion-k` (bumps `erodibility_per_yr` by one ULP before
replaying the erosion record and touches nothing else), isolates that group specifically:
216 of the erosion group's 3,000 heights diverge and its status/iteration/converged fields do
not, so this control shows the arithmetic is sensitive to `k` without the divergence being an
artifact of a different iteration count on the two sides.

   **A correction this project must not re-introduce.** Task 2 of this slice changed the
   `.wasm`'s bytes (`60244aec…` → `dcbed115…`) as a side effect of adding a Cargo
   build-dependency, and the change is cosmetic - but the evidence first offered for that
   (a symbol-name diff) was wrong: the element section (id 9) and the code section (id 10)
   both moved, not just symbol names. **The conclusion survives on better evidence: the same
   53,251-compared / 0-divergent parity run above, plus the export section (id 7) being
   byte-identical** - the eleven exports and their signatures are unchanged, which is the
   claim that actually matters for a consumer calling through this ABI. Cite parity and the
   export section for this claim; never the symbol diff, and treat no other part of the
   `.wasm` as byte-identical across that change.

## The two count gates

`.github/scripts/assert_counts.py` is a seventh kind of check: the six gates above ask "did
anything fail", which cannot catch a suite whose tests were deleted or whose corpus quietly
shrank while everything else stayed green. It cross-checks two independently-produced
statements of the same number (never a `test result:` grep) and fails loudly, naming the
mismatch, if they disagree or either is missing:

- **Engine suite**, per feature configuration: `cargo test -- --list` gives one `<name>: test`
  line per test and a `<N> tests, <M> benchmarks` trailer per binary; the two must agree.
  Re-derived on the current tree for all five configurations (`cargo test -p
  worldbuilder-engine <features> -- --list`, cross-checked with `assert_counts.py
  cargo-list`):

  | configuration | listed | ignored | run (pinned in `gates.yml`) |
  |---|---|---|---|
  | `--no-default-features` | 458 | 5 | **453** |
  | default | 458 | 5 | **453** |
  | `--features python` | 460 | 5 | **455** |
  | `--features wasm` | 493 | 5 | **488** |
  | `--features python,wasm` | 495 | 5 | **490** |

  These moved up from 409/409/409/439/439 in the identity slice (`tests/build_fingerprint.rs`,
  new, adding 9 tests to all five configurations, plus 2 more in `lib` for `--features python`
  because `source_fingerprint()` / `source_fingerprint_inputs()` are PyO3 exports whose
  binding tests only compile with that feature), and again in slice 5a: `erosion.rs` (new,
  compiles unconditionally) added 35 tests to `lib` across all five rows over that slice's
  three tasks, and `tests/wasm_exports.rs` gained 5 more for `wb_erosion_run`'s parameter
  refusals, moving only the two `wasm`-feature rows. Re-derived directly from `cargo test -p
  worldbuilder-engine <features> -- --list` (and `--list --ignored`) on the current tree, not
  carried forward from either earlier slice. On a deleted test, the gate fails with
  `COUNT GATE FAILED / expected <N> tests to run, found <M>` even though `cargo test` itself
  exits 0.
- **Python suite**: `pytest --collect-only -q`'s per-test lines and its `<N> tests collected`
  trailer, cross-checked against the real run's `<N> passed in <T>s` summary. Asserts
  **398 tests in total, 157 of them in `tests/test_conformance.py`** - both re-derived on the
  current tree (`pytest tests/ --collect-only -q`, `WORLDBUILDER_REQUIRE_ENGINE=1`), not
  copied from an earlier report. Of the 157, **150 are conformance comparisons and 7 are
  guard unit tests** this slice's Task 4 added for `_require_current_engine` /
  `_manifest_source_fingerprint` - calling all 157 "comparisons" overstates what the file
  does, which is why the wording above says "tests", not "comparisons" (the gate itself still
  asserts the file-level total of 157; only the prose describing it changed). On the
  historical unguarded configuration it fails with `the run reports outcomes that are not
  `passed`: 1 skipped ... expected 398 tests in total, found 241 / expected 157 tests in
  tests/test_conformance.py, found 0`.
  (**157**, not 150: the gate pins the file's total, and seven of those are the guard unit
  tests named just above. An earlier draft of this line said 150 and contradicted its own
  preceding bullet.)
- **Parity corpus**: the total line (`56,254 values compared` -- 53,251 plus the 3,003-value
  `erosion/erosion` group slice 5a added) is cross-checked against the ten per-group tallies
  summing to it, so a shrunk corpus fails with `COUNT GATE FAILED / expected 56254 values
  compared, found <M>` even when provenance and parity both report green on their own.

## What CI does NOT cover

**The viewer's browser checks.** `viewer/README.md` documents an in-page harness
(`window.__wb.check()`) exercised through URL parameters and a `?fault=` switch that forces a
known-wrong implementation: eleven checks pass on the default world, and seven `?fault=`
values (`flip-latitude`, `shift-tile`, `wrong-world`, `stale-worker`, `cache-key`,
`feature-blind`, `feature-everywhere`) each must make specific checks fail. This needs a real
browser with WebGL compositing and a person reading the result - it is not a test file CI can
invoke, several checks are explicitly rendering-dependent (one reports NOT EXERCISED when
`frameState.frameNumber` is 0), and a software-rasterised CI number would be a different
measurement wearing the same name. **It is out of scope for CI**, and the identity slice does
not change that. A green badge on this workflow says nothing about the viewer having been
watched run.

Both holes the CI slice deferred to "a future slice" are now closed by the identity slice:
the Python extension carries a source fingerprint and gate 4 above checks it against an
independent implementation, and the distribution-name collision above is a real split, not a
workaround. What is still genuinely open, so this table does not read as full coverage:

- **The stale-engine guard (gate 3) only runs where `WORLDBUILDER_REQUIRE_ENGINE=1` is set.**
  A developer's local `pytest tests/` with no engine installed still silently skips
  (`pytest.importorskip`), by design - the guard is opt-in for exactly the reason described
  above, so a machine with no Rust toolchain can still run the suite. Nothing forces a local
  run to opt in.
- **The stale-engine guard's soundness depends on gate 5 having already run and passed on
  the same tree.** See gate 3's composition note above: if `npm run check:wasm` is ever
  dropped from CI, or a developer runs the pytest guard locally against a manifest they
  never re-checked with `check:wasm`, the comparison silently goes back to answering nothing.
- **The install-order dependency between `pip install -e .` and `maturin develop` is asserted
  fixed, not proven fixed.** The distribution split should make the order irrelevant by
  construction; nobody has verified this by reversing the order and running a `pip
  uninstall`, so treat "the order no longer matters" as the current best understanding, not
  a settled fact.
