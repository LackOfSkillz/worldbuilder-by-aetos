# Slice 2b: the read-only viewer

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rotate and zoom a generated planet in a browser, drawing terrain from the WASM engine. Read-only: no placement, no editing, no Evennia, no live connection.

**Architecture:** The engine compiled to `wasm32-unknown-unknown` behind a `wasm` feature, exposing a **world handle** through raw `extern "C"`. CesiumJS draws it through `CustomHeightmapTerrainProvider`. A Web Worker pool fills tiles; tiles cache and are never recomputed per frame.

**Tech Stack:** Rust → WASM (no `wasm-bindgen`), CesiumJS 1.145 (Apache-2.0), plain ES modules, a static server.

**Spec:** `docs/design/2026-09-02-mark-2-world-studio.md` §7 (the studio, of which this is the read-only ancestor), §20 (why a viewer rides alongside slice 2), §4.1/§4.2 (bit-equality, DETERMINISM-001), §17 (acceptance). Plus `docs/design/2026-09-03-roadmap-additions.md` §2, **as corrected** — see below.

## Global Constraints

- **All transcendentals through `detmath`.** No `f64::` method or associated form, no `mul_add`, no bare integer cast without a `// cast-ok: <reason>` marker **on the same line**. `abs` is exempt. **The guard test scans all of `src/`, and a probe export module failed it with ten offences** — `.round(`, `.cos(`, `.to_radians(` and seven integer casts. Grid-step and index-to-float conversions are exactly where it bites.
- **Never `f64::min` / `f64::max` / `clamp`** — explicit `if`/`else`, house form in `plates.rs::margin_at`.
- **Nothing under `worldbuilder/` may be modified.** `worldbuilder/integration/maritime.py` has a pre-existing uncommitted change; leave it unstaged.
- **No live connection.** No websocket, no browser access to an ORM, no credentials, **no phone-home**. This is a spec requirement, not a preference.
- **Verify by exit status, never by grepping `test result:` lines.** Check all three engine feature configurations.
- **`cargo` is not on PATH in bash — use `/c/Users/gary/.cargo/bin/cargo.exe`.** `cargo clippy`/`cargo fmt` shims exist but the components are not installed.
- **Every figure names its population, its method including that method's parameters, its host — and, for a ratio, its step.** Fifteen figures in this project have needed a population attached.

---

## What is already measured, and must not be re-litigated

**Cesium cleared all three verification questions.** Apache-2.0 at 1.145.0, confirmed against `LICENSE.md` on `main` with every commit touching it listed back to 2015-02-20. `CustomHeightmapTerrainProvider` exists **specifically** for procedural sources — one callback, which may return a promise, so **no provider class need be written at all**. Ion is fully disablable: no telemetry in the bundle, `Ion.js` performs no I/O, and exactly **one** `Viewer` default is network-live.

**The float64 precision problem costs a custom provider nothing.** Terrain uses per-tile relative-to-centre encoding with the camera subtraction done in float64 on the CPU. Return standard `TerrainData` and the mechanism is downstream.

**The WASM path holds in a browser.** Chrome 151, 84,188 bytes, 13 exports plus memory, **zero imports** — no `wasm-bindgen`, no JS glue. 400,000 samples over five entry points, **0 divergent** against native; plus a second **200,000-sample corpus inside a placed harbour**, also 0 divergent, because the scattered corpus never lands in a feature.

**Per-call FFI overhead is ~2% of a sample, so batching buys nothing** — `noop3` costs 0.008–0.013 µs against ~0.9 µs for an elevation, and per-call versus one-call-per-tile over an identical 256×256 grid is indistinguishable. **Output width is free too**, so the boundary can be shaped for Cesium's `Float32Array` at no cost.

**Tile budget**, 65×65, 128 origins globe-wide, 8 repeats: median **3.86 ms**, p90 18.20 ms. **Land tiles mean 9.56 ms against sea 6.88 ms, and a coastal tile costs up to 9× a deep-ocean one — which matters because coasts are what the viewer looks at.** 512×512 is 0.24 s. **Web Workers scale 6.08× at 8.** `Surface::new` is 2.2–3.2 ms, so a parameter change rebuilds a world inside a frame.

---

## Three traps, each already sprung once in this project

**1. A green WASM build can contain nothing.** The first one was **327 bytes exporting only `memory`**, because `cdylib` discarded every module — nothing was `#[no_mangle] extern "C"`. **Inspect the artifact's size and export section, never the build's exit status.** Slice 1p's Task 1 deliberately produced exactly that empty build and confirmed it by hand-parsing section id 7.

**2. `bindings.rs` is not a template.** `surface_elevation_m(...)` **rebuilds the `Surface` on every call** and would pay ~3 ms per sample. The export layer needs a **world-handle model**: construct once, sample many times.

**3. A panic behind `extern "C"` is an ABORT, not a catchable error.** Task 2 found a latent panic — a
negative `land_fraction` indexing past the continentality calibration — and deleting the guard did not
produce a failing test. It produced `thread caused non-unwinding panic. aborting`, taking 27 other tests
with it, **because `extern "C"` is nounwind**. Nothing above that layer can catch anything, and a JS host
gets a dead module with no diagnostic.

**So every `extern "C"` entry point validates its inputs and returns a status.** The ordinary Rust posture
— let it panic, the caller sees a backtrace — is not available here, because there is no caller to see it.

**4. A 100% divergence rate is almost never a divergence.** A feature-local run once reported 200,000 of 200,000 divergent; it was a stale cached `common.js` running the wrong corpus. **Suspect the harness before the engine.**

---

## What zoom actually reveals, corrected twice

The roadmap's original claim was wrong in both directions and is now measured:

- **`CANONICAL_WAVELENGTH_M = 250` is a loop bound**, so the finest generated octave is **312.5 m**.
- **The resolution floor is 78.125 m**, not 250 — octaves fade by `smooth((λ/r − 2)/2)` and reach full strength only at `r ≤ λ/4`. On a 2 km transect, peak-to-peak rises monotonically 0.19 → 5.58 m from `r = 20000` down to `r = 78.125`, then is **bit-identical** for 50, 25 and `None`.
- **Below ~100 m the generated field is a tilted plane** — 4.5 cm of chord deviation over 100 m, 2.5 mm over 25 m.
- **Authored features are genuinely different.** A 900×260 m carve plus a 200×60 m mole give **152.8 m of relief over a 100 m span**, because `Features::apply` is analytic and outside the octave schedule. Features cost under 5% in throughput.

**So: zoom reveals generated ground to ~78 m, and below that reveals authored features.** The demo world has features; a bare world does not, and the viewer should not pretend otherwise.

**`getTileDataAvailable` returning `undefined` makes Cesium refine until it runs out of memory. That is where the zoom cap is enforced** — and it must be, deliberately, at a depth this plan states.

---

## Rulings taken before execution

**1. The export module lives IN the engine crate behind a `wasm` feature — not in its own crate.** DETERMINISM-001's static guard scans all of `src/`, so an in-crate module is covered automatically. Splitting it out would require widening the guard *in the same commit* or it silently stops covering the newest, least-reviewed code in the project. Same-crate is the safe default; the cost is one more feature flag.

**2. No `wasm-bindgen`.** The measured module has **zero imports** and 13 raw exports. Adding a binding generator would add a JS glue layer, a build dependency and a marshalling story, to replace something already measured working.

**3. Tiles cache; nothing recomputes per frame.** A 65×65 tile is 3.86 ms median but 18.2 ms at p90 — over a 16 ms frame. Cache by tile key, fill in workers.

**4. The viewer is read-only, and the line is where slice 3 begins.** Rotate, zoom, inspect, distinguish land from water. **No placement, no editing, no anchor tree, no worldfile, no Evennia.**

---

## File Structure

- **Create** `crates/worldbuilder-engine/src/wasm.rs` — the `extern "C"` surface and world-handle model, behind `feature = "wasm"`.
- **Modify** `crates/worldbuilder-engine/src/lib.rs`, `Cargo.toml` — declare the module and the feature.
- **Create** `viewer/` — `index.html`, an ES module entry, the worker, and a build script. Static; no bundler unless a task proves one is needed.
- **Create** `viewer/README.md` — how to build and serve it offline.

---

### Task 1: Witness the offline claim

**Files:** Create `viewer/` skeleton and `viewer/README.md`

**The ion finding is documentary and source-based, not witnessed** — no browser network trace was taken, because that needed a third-party download. **This is the first task because a phone-home is disqualifying**, and everything else is built on the assumption there is none.

- [ ] **Step 1:** Vendor Cesium locally under `CESIUM_BASE_URL`. **Record its exact version and license file.**
- [ ] **Step 2:** Serve a minimal `Viewer` and **take a browser network trace.** Confirm: no request leaves the origin. Remember the one network-live default, `baseLayer = ImageryLayer.fromWorldImagery()` — replace it.
- [ ] **Step 3:** **Then break it on purpose** — enable the default imagery, confirm the trace *does* show an outbound request, and revert. A trace that shows nothing proves nothing unless it can show something.
- [ ] **Step 4:** Record the recipe in `viewer/README.md`. Commit.

---

### Task 2: The WASM export surface

**Files:** Create `src/wasm.rs`; modify `src/lib.rs`, `Cargo.toml`

**Interfaces:** a world handle — construct from parameters, sample by point, free — plus `alloc`/`dealloc`, and a tile entry point taking a bounding box and a grid size and writing `Float32Array`-shaped heights into linear memory.

**Do not model this on `bindings.rs`**, which rebuilds the `Surface` per call.

**Every cast needs its `// cast-ok:` marker.** This is the module most likely to fail the guard.

- [ ] **Steps:** failing tests, run, implement, run, **inspect the built artifact's exports**, whole suite by exit status in all three configurations, commit.

---

### Task 3: Build and serve

**Files:** `viewer/` build script

Produce the `.wasm`, inspect it, serve it. **Assert the artifact is not the empty build** — size and export count, checked in the script, not by eye.

- [ ] **Steps:** script, run, verify exports, commit.

---

### Task 4: The terrain provider

**Files:** `viewer/` ES modules

`CustomHeightmapTerrainProvider` with one callback. **Do not implement `ready`/`readyPromise`** — removed in 1.107. Use `getEstimatedLevelZeroGeometricErrorForAHeightmap`. **Set the zoom cap explicitly**, since leaving `getTileDataAvailable` undefined refines until out of memory.

`HeightmapTerrainData` with a `Float32Array` and default `structure` takes **metres above the ellipsoid, directly**.

- [ ] **Steps:** implement, verify against known engine values at named points, commit.

---

### Task 5: Workers, the tile cache, and feature-aware availability

**Files:** `viewer/` worker and cache

Eight workers measured at 6.08×. Cache by tile key. **Measure the frame budget in a browser and state it** — median, p90, and the coastal-versus-ocean split, since a coastal tile costs up to 9×.

**And close the gap Task 4 found, which is why this task grew.** Authored features are resolution-independent *point-wise* — the harbour centre reads exactly +4.00 m at every resolution tried — **but they are grid-sampling-limited**. The level-12 tile containing that harbour **tops out at −819 m against a +4 m target** and needs about **level 16** to resolve.

So the cap of 12 is correct for generated ground and **four levels short for features**. A viewer that caps below its own authored relief cannot show the one thing "a field viewer shows the bottom of the harbour" was ever about. **Make availability feature-aware**: refine further where a feature is present, and nowhere else. It needs the cache to be affordable, which is why it lands here rather than as a follow-up.

- [ ] **Steps:** implement, measure, commit.

---

### Task 6: A content-security policy — turn "did not" into "cannot"

**Files:** `viewer/` server config and page head

Task 1 witnessed that nothing leaves the origin, and then proved the trace can show something by
re-enabling the default imagery and watching tens of off-origin requests appear. **But absence of traffic
is not absence of capability** — the trace shows Chrome on Windows *did not* phone home, not that it
*cannot*.

**`default-src 'self'` converts the guarantee.** It lands here rather than in Task 1 because it interacts
with the WASM module and the workers, and retrofitting a policy around them is harder than writing it once
they exist.

**Verify it the same way Task 1 did**: confirm the viewer still works under the policy, then **re-enable
the net probe and confirm the policy now BLOCKS it** — a CSP that has never refused anything is not known
to be doing its job.

- [ ] **Steps:** add the policy, verify the viewer, verify the probe is blocked, commit.

---

### Task 7: Record it

**Files:** `viewer/README.md`, `crates/worldbuilder-engine/README.md`

**Read every number from the current source and from your own runs, never from a report.** Cover: the offline recipe and the trace that witnessed it; the export surface and why it is a handle; the measured tile budget with its populations; the zoom cap and **what zoom actually reveals at each scale**; and that this is read-only, with slice 3 owning placement.

- [ ] **Steps:** record, verify counts by running, delete any throwaway, commit.

---

## What this slice must NOT do

- **No placement, no editing, no anchor tree, no worldfile.** Slice 3.
- **No Evennia, no inventory, no apply.** Slice 2a.
- **No climate or land cover** — designed, not approved.
- **No erosion.** Slice 5.
