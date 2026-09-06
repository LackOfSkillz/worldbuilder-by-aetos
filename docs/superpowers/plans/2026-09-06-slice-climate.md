# Climate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this
> plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the engine a temperature and a moisture at any point, so the viewer's biome colours stop
being an approximation and the game gets real climate metadata.

**Architecture:** Point-evaluable throughout. Temperature is closed form from latitude and elevation.
Moisture is a **bounded upwind ray-march** at query time -- the design the roadmap chose over baking a
raster, because the spec forbids the studio drawing from a separately baked approximation that could
disagree with the game. Both land as **opt-in parameters with byte-identical defaults**, the pattern three
slices have now used cleanly.

**Tech Stack:** Rust (`crates/worldbuilder-engine`), `extern "C"` exports, CesiumJS viewer.

**Spec:** `docs/design/2026-09-03-roadmap-additions.md` section 3, whose decisions were taken and recorded as
owner-approved. Section 8.3 places this slice **after water, before the studio** -- water exists now, so
rainfall has somewhere to go.

## Why this slice, now

**The land-colour task reported the gap itself:** *the engine exposes no temperature, no moisture and no
distance-to-coast*, so the viewer's `biome.js` approximates "coastal" as the bottom quartile of land
elevation and derives moisture from noise. It reaches 30 of 33 colours on the owner's world and looks good.
**It is a stand-in for exactly what this slice computes.**

Closing it also moves two of the gap analysis's twelve differences: **snow and ice** from partly-closed to
closed (a real snow line rather than a global elevation threshold), and **land colour** from approximated to
derived.

## THE SPIKE IS ALREADY ANSWERED. READ IT BEFORE DESIGNING ANYTHING.

`.superpowers/sdd/notes/climate-march-spike-report.md`, measured, not estimated:

- **Cost is AFFINE in sample count with NO KNEE**: about `0.48 + 0.48*N` microseconds native and
  `2.0 + 1.5*N` in WASM, straight from 1 to 320 samples. **So the sample budget is a physics decision, not
  a performance one -- there is no bend to cite.**
- **Native-to-WASM is a stable ~3x** across the whole sweep.
- **Texels are quadratic and samples are linear, so THE RASTER IS THE LEVER.** A 40-sample march at
  relief's own raster costs **7.2 s per tile and ~55 s to settle -- 35-45x the entire relief layer.** At a
  **32^2** moisture raster it is 125 ms/tile and ~0.95 s: parity with relief.
- **Coarse sampling buys only 27% native / 33% WASM and that is its ceiling** -- `res=20km` already fades
  every configured octave. The hoped-for large win did not survive.
- **The roadmap's own `1-2 us` and `40-80 us` estimates are wrong natively by 2-4x and RIGHT in WASM** --
  correct on the host they never measured. Every figure in this slice names its host.

**Two hazards the spike found, both binding:**
- **`count`/`samples` are UNBOUNDED LOOP BOUNDS in a nounwind export.** `count = 2^20` took 0.64 s, so
  `u32::MAX` extrapolates to ~2,600 s inside one uninterruptible call. **Bound the budget in the contract.**
- **A NaN step silently returned full moisture rather than NaN.** Decide deliberately; do not inherit it.
  Note this project has since fixed three NaN entrants that converged on one function and produced a
  **plausible world rather than an error** -- one of them bit-identical to a legitimate all-land world.

## Global Constraints

Every task's requirements implicitly include this section.

- **RULING 1: `worldbuilder/` is the conformance oracle for 157 tests. NO DEFAULT MOVES.** New parameters,
  opt-in, `None` byte-identical. Conformance stays **398/398 with `test_conformance.py`=157**.
  **Note: pytest is not purely 398** -- `test_performance.py` asserts a 260 us/sample wall-clock ceiling and
  returns 397+1 under load. **One of the 398 is a millisecond wearing a count's clothes**; do not chase it.
- **`worldbuilder/integration/maritime.py` has a pre-existing uncommitted change; leave it unstaged. Never
  `git commit -a`.** `.claude/` and `crates/worldbuilder-engine/parity/native.txt` are untracked; leave both.
- **`fixtures/dragonsire/` is a gitignored real game database.** Never in a commit.
- **All transcendentals through `detmath`.** No `f64::` method form, no `mul_add`, no bare integer cast
  without a `// cast-ok: <reason>` marker on the same line. `abs` is exempt.
- **Never `f64::min`/`f64::max`/`.clamp(`** -- NaN-asymmetric, and **the build guard does not catch them.**
- **`extern "C"` is nounwind.** Four aborts and two hangs have been found here by **sweeping** export inputs
  and **zero by spot-checking**; every one was a **band, not a cliff**. **One abort was reachable only
  through the CROSS PRODUCT of three individually-admissible fields** -- sweep combinations, not just axes.
- **Verify by exit status.** Piping into `grep`/`tail` gives you the PIPE's status.
- **Re-bless the manifest AFTER the last source edit**, rebuild, re-derive all five pins with
  `assert_counts.py cargo-list` -- listed **minus** ignored. **Re-derive rather than trusting any number
  written down**; `gates.yml` has been found wrong once and README counts four times.
- Parity is **108,106 compared / 0 divergent**. A new crossing value needs corpus coverage **with a negative
  control that diverges for a stated reason** -- a control that moves everything is as uninformative as one
  that moves nothing.
- **Prove every new assertion red by mutation, neutralising siblings first.** **Thirteen assertions or
  metrics in this project have looked load-bearing and were not.**
- **Every figure names its population, its method with parameters, and its host.**

---

### Task 1: Temperature, closed form

**Files:** Create `crates/worldbuilder-engine/src/climate.rs`; modify `surface.rs`, `lib.rs`

Latitude plus an elevation lapse rate. **Closed form, no march, no state.**

**Elevation feeding temperature gives the snow line for free** -- the same tropical latitude is rainforest at
sea level and snowcap at altitude, and polar caps fall out of temperature alone with no special case.

**Do NOT quantile temperature.** The land-colour task tried it and found it put the owner world's desert
latitudes in the median band and made the brightest palette entry unreachable. **Temperature has a unit;
moisture does not.** That finding is in `task-1-report.md` of the photoreal slice.

- [ ] **Steps:** failing test, run, implement, prove `None` bit-identical, five configs, conformance, commit.

---

### Task 2: The upwind moisture march

**Files:** Modify `crates/worldbuilder-engine/src/climate.rs`

From the query point, walk **upwind** a bounded number of steps sampling elevation and accumulating
rain-out. Latitude bands set the prevailing wind; the march produces the rain shadow.

**The sample budget is a physics decision** -- the spike proved there is no performance knee to hide behind.
Choose it on what a rain shadow needs at this planet's scale and say so.

**Bound the loop in the type, not only in the export.** A loop bound reachable from outside a nounwind
boundary is a hang.

- [ ] **Steps:** failing test, run, implement, sweep for hangs and NaN, measure cost against the spike's
  curve, commit.

---

### Task 3: Bands, and the non-uniform spacing that beats even spacing

**Files:** Modify `crates/worldbuilder-engine/src/climate.rs`

Band edges as **per-world quantiles**, using the same Fibonacci order statistic
`continentality.rs::calibrate` already uses -- **that routine is the grid-free equivalent of the array
reduction other generators use, and this project owns it already.**

**Use non-uniform spacing.** A reference implementation's own comment: *"originally evenly spaced at 12.5%
each but changing them to a bell curve produced better results."* **Evenly-spaced moisture bands give too
much middling terrain and no real desert**, and the roadmap's four evenly-spaced bands should be revised on
that evidence.

**Keep three axes** -- landform x temperature x moisture. Two-axis models have no landform axis and
therefore no "coastal", and for a MUD "tropical coastal" and "boreal coastal" are different places to stand.

- [ ] **Steps:** failing test, run, implement, prove every band is reached on at least two worlds, commit.

---

### Task 4: Export, and replace the viewer's approximation

**Files:** `crates/worldbuilder-engine/src/wasm.rs`, `viewer/public/app/engine.js`,
`viewer/public/app/biome.js`

**`biome.js` already exists and already works.** This task replaces its *inputs*, not its palette: real
temperature and moisture instead of noise plus latitude, and a real coastal term instead of the bottom
quartile of land elevation.

**Ship the moisture raster at a resolution chosen from the spike**, which says 32^2 is parity with relief
and relief's own raster is 35-45x too expensive. **Measure the combined pool cost with all consumers** --
there are three today and this makes four.

**`?climate=0` must be byte-identical to today**, proven by SHA-256 at a pinned viewport, camera and
**explicit frame time**. `Scene.render()` with no argument defaults to `JulianDate.now()`; the harness's
`digest` path is correct and its plain `shoot` is not.

- [ ] **Steps:** export, sweep including cross products, wire, measure, prove the off-path identical,
  screenshot, commit.

---

### Task 5: The snow line, and the coupling nobody had noticed

**Files:** Modify `crates/worldbuilder-engine/src/climate.rs`, and the relief snow band

Egholm et al. (2009), *Nature* 460, verbatim: **"most summit elevations are confined to altitudes <1,500 m
above the local snowline"**, and differences in range height "mainly reflect variations in local climate
rather than tectonic forces".

**So convergence sets a mountain's width and uplift rate; CLIMATE sets its height.** The mountains slice and
this one are coupled in a direction neither plan had noticed.

**Implement the snow line first and the clamp only if it measures well.** The clamp changes terrain and is
the more invasive of the two; the snow line alone closes a gap analysis item.

- [ ] **Steps:** snow line, measure against the relief slice's snow band, decide on the clamp with numbers,
  commit.

---

### Task 6: Record it

**Files:** `crates/worldbuilder-engine/README.md`, `viewer/README.md`, and the gap analysis

**Read every number from current source or your own runs, never from a report.** **Eight transcription
defects across six slices**; one survived four tasks and five shipped comment sites, and an artifact count
was found stale four times.

- [ ] **Steps:** record, run every command you document, commit.

---

## What this slice must NOT do

- **No baked global moisture raster.** The spec forbids the studio drawing from a separately baked
  approximation that could disagree with the game, and editing terrain would then do nothing until a rebake.
  **Per-tile caching is the design; a global bake is not.**
- **No rivers.** The schema carries reaches and they are deliberately unpopulated.
- **No default change to any engine parameter.**
- **No erosion coupling.** Rainfall is erosion's input and that connection is real, but it is its own slice.
