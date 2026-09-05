# Photoreal Slice Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this
> plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the visible gap between this viewer and the owner's north-star reference, in the order that
buys the most picture per day.

**Architecture:** Viewer-first. Tasks 1-4 touch only `viewer/public/app/` and cannot move the conformance
oracle at all. Task 5 is the one engine change, and it lands under the same opt-in, byte-identical-default
argument as `ReliefParams` and `TectonicParams` before it.

**Tech Stack:** CesiumJS 1.145 in `viewer/public/app/`, the existing worker pool and imagery-provider
machinery from the relief slice, and `crates/worldbuilder-engine` for Task 5 only.

**Spec:** `docs/design/2026-09-05-north-star-gap.md` -- the element-by-element analysis of the two images,
with all twelve differences ranked. **Read it before any task.**

## Why this slice exists

The owner, on seeing the current viewer beside their reference: *"lets stop all other tasks and make this
our only priority. otherwise out app looks like a kindergarden toy."*

Everything else on the roadmap is parked: climate, the Evennia export, the studio, and the fuller mountain
survey. None of them make the picture better.

## Global Constraints

Every task's requirements implicitly include this section.

- **VIEWER ONLY for Tasks 1-4.** Do not modify `crates/`, do not rebuild the wasm, do not touch
  `worldbuilder/`. If a task believes it needs an engine change, that is a finding to report.
- **`worldbuilder/` must not be modified. It is the conformance oracle for 157 tests.**
  `worldbuilder/integration/maritime.py` has a pre-existing uncommitted change; **leave it unstaged. Never
  `git commit -a`.** `.claude/` is untracked; leave it.
- **`fixtures/dragonsire/` is a gitignored real game database.** It must never appear in a commit.
- **Verify by exit status, never by grepping output.** **Piping a command into `grep` or `tail` gives you
  the PIPE's exit status** -- a four-error build was read as passing that way in this project.
- **Every figure names its population, its method with parameters, and its host.** Counts are properties of
  the algorithm; milliseconds are properties of the moment. The same quantity measured 191.6, 55.7 and
  178.1 ms on three hosts here, all supporting one conclusion.
- **Every new range control must satisfy `panelFieldFaults()`.** `panel-fields.js` holds one table the panel
  reads rather than restates. That check has caught **four** mis-stepped sliders, the fourth found by the
  check rather than by a person. Do not add a fifth.
- **Prove any "unchanged" claim by SHA-256 of the rendered PNG** at a pinned viewport, camera and frame
  time, with a positive control showing the digest moves when the thing under test changes.
  **`Scene.render()` with no argument defaults its frame time to `JulianDate.now()`** -- pinning
  `viewer.clock` alone does nothing.
- **A check that cannot fail is worse than no check.** Prove every new assertion red by mutation, and
  **neutralise its siblings first** -- six assertions in this project have looked load-bearing and were not.
  **Byte-identity proves the picture, never the path**: a provider that ignored the worker pool passed a
  byte-identity test because it drew the same pixels.
- **Dead code looks like a feature.** Three colour blends shipped in `relief.js` having never once been
  selected against this terrain. If a branch exists, prove it is reached.
- The viewer's npm suite passes **57/57**; it must still pass.

---

### Task 1: Clouds

**Files:** Create `viewer/public/app/clouds.js`, `viewer/public/app/cloud-provider.js`; modify
`viewer/public/app/main.js`, `viewer/public/app/controls.js`

**The single largest difference between the two images, and it touches no engine code.** The reference
carries white cirrus and cumulus over roughly 40% of the disc. It is what the eye reads first as
"photograph of a planet" rather than "diagram of a planet".

An imagery layer over the relief one, built the same way: an `ImageryProvider` wrapping a tile rasteriser,
rasterised in the existing worker pool. **`relief-provider.js` and `relief.js` are the template** -- they
solved the provider-interface traps already, and Task 4 of that slice moved rasterisation off the main
thread for a measured reason (**55.7 ms -> 0.219 ms per tile**, 53 long tasks -> 0).

**Cesium version traps, recorded and still true:** `ready`/`readyPromise` were **removed from
`ImageryProvider` in 1.107**, and `getTileDataAvailable` returning `undefined` **refines until the tab
dies**. Return a definite boolean.

Design notes, not prescriptions -- state what you chose and why:

- **Cloud cover must be a control.** The reference is roughly 40%; a planet should be able to be clearer or
  stormier. Calibrate its travel from what you measure, not from taste.
- **Clouds are not painted on the ground.** They sit above it and should read as a separate layer -- a
  slight parallax or a soft edge sells this more than detail does.
- **Bands, not uniform noise.** Real planets have latitude structure: an equatorial belt, mid-latitude
  storm tracks, clear subtropics. Uniform fBm reads as static. Cite what you are matching.
- **`?clouds=0` must turn it off**, and with it off the picture must be byte-identical to today's.

- [ ] **Steps:** failing test, run, rasteriser, provider, worker pool, panel control, prove the off-path
  identical, screenshot, commit.

---

### Task 2: Ocean tone and the two colour systems

**Files:** Modify `viewer/public/app/panel-fields.js`, `viewer/public/app/relief.js`,
`viewer/public/app/main.js`

**Measure before you tune. There are TWO colour systems and it is not documented which wins where:** the
`ElevationRamp` material on the globe, and the relief imagery layer painted over it. **Establish which one
the ocean's colour actually comes from**, and whether the imagery layer is flattening the ramp's contrast.
Report that before changing a value.

**A correction that is part of this task's brief.** The gap analysis first said the ocean was flat blue and
the bathymetry unused. **That was wrong** -- `RAMP_STOPS` carries six ocean stops from `#020a14` at the
abyssal plain to `#7ec5df` under the strand, and the pale rim around each landmass is that gradient working.
**The gap is tone, not data.** The reference's deeps go nearly black and its shelves are brighter: more
contrast across the same depth range.

So: measure the **actual depth distribution this generator produces** (there is a global fill in
`panel-fields.js`'s comments giving the sea floor's minimum at -6,345 m), and place the stops against that
distribution rather than against a guessed range. A stop below the deepest water is a stop that never draws.

- [ ] **Steps:** measure which system wins, measure the depth distribution, re-place the stops, prove the
  new digest stable, screenshot before and after, commit.

---

### Task 3: Atmosphere, limb, and grade

**Files:** Modify `viewer/public/app/main.js`

The reference has a **thick blue haze wrapping the limb**; ours is a thin hard white ring. It also has deep
blacks and bright highlights where ours is uniformly mid-tone.

**Ground atmosphere is currently OFF for a measured reason**, recorded earlier in this project: it washed
the ocean to `(122,172,137)` -- the same colour as land 500 m up. **That measurement was taken against the
ocean as it was before Task 2.** Re-measure rather than inheriting the conclusion, and if it still holds,
say so with the new numbers.

Sky atmosphere and lighting are already on. What remains is their tuning, plus whatever grading Cesium
exposes. **Name every setting you change and what it cost**, because this is the task most able to make
things worse while looking like an improvement.

- [ ] **Steps:** re-measure the ground-atmosphere finding, tune, screenshot each change, commit.

---

### Task 4: Draw the lakes that already exist

**Files:** Modify `viewer/public/app/main.js` and whichever colour path Task 2 identifies

**Slice 5b built lakes and nothing draws them.** The panel says so itself: *"lakes + water -- slice 5b, in
progress: no export yet"*. The reference's most distinctive single feature is a large circular inland sea.

`wb_water_run` **already ships in the wasm** and returns the manifest; the parity harness exercises it. This
task is about consuming what exists, not building anything new.

**Known and carried:** the pond threshold produces **zero ponds** on the current mesh -- the smallest body
is 7.9e8 m², four orders larger than the threshold. That is a recorded finding about the mesh's resolution,
**not a bug to tune away**, and it means every body you draw is a large one.

- [ ] **Steps:** read the manifest, draw the bodies, prove they land where the engine says, screenshot,
  commit.

---

### Task 5: Fractal coastlines -- THE ONE ENGINE CHANGE

**Files:** Modify `crates/worldbuilder-engine/src/continentality.rs`, `surface.rs`; then the viewer

**This was not on the roadmap anywhere.** The relief slice fixed the *height* field's spectrum, and nobody
asked the same question of the *land/sea* field. A coastline is the highest-contrast edge in the whole
image and the eye goes straight to it -- ours are smooth blobs because continentality is low-frequency.

**Same safety argument as `ReliefParams` and `TectonicParams`, which both landed cleanly:** new opt-in
parameters, `None` byte-identical to today, `canonical()` untouched, the conformance suite unmoved at
**398/398 with `test_conformance.py=157`**. `worldbuilder/terrain/` holds the continentality reference too.

**The trap this slice must not walk into**, learned from the relief slice: adding octaves to a normalised
sum **redistributes** amplitude rather than adding it. Changing the land/sea threshold's roughness will
change **how much land there is**, not just its edge shape, unless the land fraction is re-normalised. **Measure
the land fraction before and after and hold it constant**, or the owner's `land=0.16` will silently stop
meaning 0.16.

- [ ] **Steps:** parameters, prove `None` bit-identical, measure land fraction is held, sweep for the edge
  shape, expose, screenshot, commit.

---

### Task 6: Record it

**Files:** `viewer/README.md`, `docs/design/2026-09-05-north-star-gap.md`

**Read every number from the current source or your own runs, never from a report or a ledger.** This
project has caught **seven** transcription defects across five slices doing exactly what that forbids, and
one -- a peak quoted as the planet's when it was a sparse probe's maximum -- survived four tasks and five
shipped comment sites.

Update the gap analysis to say which of its twelve elements are now closed, which remain, and **what the
remaining ones actually cost** now that four of them have been attempted. Cover what still differs from the
reference and why.

- [ ] **Steps:** record, verify by running every command you document, commit.

---

## What this slice must NOT do

- **No climate, no biomes, no rivers, no studio, no Evennia export.** All parked by the owner.
- **No default change to any engine parameter.** Ruling 1 across three slices now.
- **No chasing the last 5%.** The reference is a rendered illustration, lit and graded like a film still.
  Some of its look is a post-processing pass and some is painted suggestion rather than derived detail.
  **Ours is computed, which is why it stays consistent at every zoom and can be walked around in a MUD.**
  That trade is the point of the project.
