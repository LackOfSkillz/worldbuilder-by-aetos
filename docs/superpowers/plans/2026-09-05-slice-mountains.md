# Tectonic Mountains Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this
> plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the owner the two controls they have asked for twice -- raise and lower mountains, and make
more or fewer of them -- on the tectonic path where mountain height actually lives.

**Architecture:** Mirror `ReliefParams` exactly. A new `TectonicParams` struct carries the uplift constants
that today are module-level `pub const`s in `tectonics.rs`; `Surface::new` takes `Option<TectonicParams>`;
`None` is bit-identical to today and the conformance suite does not move. Measure, then choose presets from
the measurement, then expose through `wasm.rs` and the viewer panel.

**Tech Stack:** Rust (`crates/worldbuilder-engine`), a `src/bin/` survey binary, `wasm-bindgen`-free
`extern "C"` exports, CesiumJS viewer in `viewer/public/app/`.

**Spec:** `docs/design/2026-09-03-roadmap-additions.md` (the roadmap that records the owner's request), and
the ruling history in `.superpowers/sdd/2026-09-05-slice-relief-amplitude/progress.md` -- Ruling 4 of that
slice is what deferred this work to its own slice, and this plan is that slice.

## Why this slice exists, and the measurement that defines it

The owner, twice, in their own words: *"1 to raise and lower mountains and one to make more mountains and
less as desired"*, and later, looking at the finished relief work: *"still no mountains"*.

**Measured on the owner's own world** (seed 123925603, radius 4,500,000 m, plates 28, land 0.16; population
a 0.5-degree global grid refined to 0.05 degrees around the maximum; method `surface_elevation_m` and
`surface_structural_m` through the Python wheel; host this machine):

- **Highest point 1,454.04 m. Structural 1,437.81 m. Detail 16.24 m. The peak is 98.9% TECTONIC.**

And the reason it reads as a ramp rather than a mountain is a constant, not an accident:

```rust
pub const CONTINENT_COLLISION_M: f64 = 1500.0;
pub const CONTINENT_COLLISION_WIDTH_M: f64 = 400_000.0;
```

**1,500 m spread over 400 km is a 0.375% grade.** The Himalaya rise about 5,000 m over 150 km (~3.3%); the
Alps about 4,000 m over 50 km (~8%). **Height alone is not a mountain -- height over a short distance is** --
so this slice moves amplitude and width together, and width is the one nobody has touched.

## REVISED BY RESEARCH, 2026-09-05: AMPLITUDE AND WIDTH ARE NOT THE KNOBS

Two shipped procedural planet generators were read at source level, and both do the same thing this plan
does not:

**They keep a broad smooth envelope that says WHERE mountains are, and MULTIPLY it by a high-frequency
structure field that supplies the local grade.**

We have the envelope. We have no structure field. **We have been tuning the envelope.**

That reframes this slice's own measurement. 1,500 m over 400 km and 6,000 m over 100 km are **the same
landform rescaled** -- the second is steeper and taller and it is still one smooth swell, not a range. The
probe's 7.03% grade is real and it is a grade *of the envelope*. It does not become ridge-and-valley
structure by being steeper, which is exactly what "smooth ridge rather than a RANGE" was pointing at.

**The sliders are not wasted.** Height and width are the right controls for the envelope, they are measured,
and the owner asked for them. They are necessary and they are not sufficient.

### The four techniques worth adopting, in the order they buy structure

1. **Multiply the envelope by a ridged multifractal times a low-frequency segmentation field.** The
   segmentation is what breaks a continuous welt into separate massifs. **Cost: `noise.rs` has no ridged
   variant**, so this needs a new primitive in BOTH the Rust and the Python oracle, under the conformance
   harness. That is the expensive one of the four; the other three need no new primitive.
2. **Warp the SIGNED across-margin distance before taking its absolute value.** Bends the crest line off the
   plate boundary rather than displacing the whole range. Essentially free, and it fixes the tell-tale that
   a range follows a Voronoi edge.
3. **Stack two to four sutures instead of one crest**, at hashed inboard offsets. **This is the sleeper.**
   Pure arithmetic, no new primitive, and it supplies structure ACROSS the range at 50-200 km -- the axis
   the noise techniques do not reach and the one a real mountain range has. Not taken from either project;
   implied by terrane accretion.
4. **Domain-warp the query point.** Cheaper for us than for the projects we read it in, because they walk a
   graph and we evaluate a function. **Honest cost: it doubles every elevation query and it does NOT conserve
   land fraction.** Neither reference project solves that; it stays our problem.

### THE LITERATURE SAYS OUR GRADE IS ALREADY RIGHT AND OUR SHAPE IS NOT

Davis, Suppe & Dahlen (1983), *JGR* 88(B2), Table 1, read verbatim and cross-confirmed in body text:
**Taiwan alpha = 2.9 +- 0.3 deg, Himalaya alpha = 4.0 +- 0.5 deg.** A 4-degree surface slope is a **7.0%
grade**.

**Our probe measured 7.03% at 6,000 m over 100 km. That is the Himalayan surface slope, to three
significant figures.** So the steep setting is not too shallow. It is the right slope on the wrong shape,
which is the sharpest possible confirmation that the missing thing is structure rather than steepness.

### THE SHAPE, WITH PUBLISHED NUMBERS: A DOUBLY-VERGENT ASYMMETRIC WEDGE

Naylor & Sinclair (2008), *Basin Research*, verbatim: **alpha_pro = 1.5 deg, alpha_retro = 2.5 deg**, giving
at H_max = 3 km a **pro-wedge 115 km wide and a retro-wedge 69 km wide**.

The mechanism is Willett, Beaumont & Fullsack (1993): a wedge grown by accretion at the toe takes the
**minimum** taper; one grown by material transported across the singularity takes the **maximum**. So a
collisional range is **asymmetric by construction** -- broad and shallow on the subducting side, short and
steep on the overriding side.

**One parameter, not two: 2.5/1.5 = 1.67, and 115/69 = 1.67.** The widths are the exact inverse of the
tangents at equal height, so the asymmetry ratio determines both.

That paper is deliberately non-numerical and says so. **Anyone citing WBF93 for a taper angle is citing it
wrongly** -- the numbers come from Naylor & Sinclair.

### WIDTH IS A FLUX BALANCE, AND THE OBVIOUS INTUITION IS BACKWARDS

Dahlen (1990), verbatim: **eW = hV**. Taiwan checks at hV/e = 7 x 70 / 5.5 = **89.1 km against 90 km
observed**.

**Barbados converges 35x slower than Taiwan and is 3.3x WIDER**, because nothing erodes it. A model that
widens a range when convergence rises has the physics inverted.

### THE HEIGHT CEILING IS CLIMATE, NOT TECTONICS, AND THIS COUPLES TWO SLICES

Egholm et al. (2009), *Nature* 460, verbatim: **"most summit elevations are confined to altitudes <1,500 m
above the local snowline"**, and differences in range height "mainly reflect variations in local climate
rather than tectonic forces".

**Convergence sets width and uplift rate. Climate sets height.** The mountains slice and the climate slice
are not independent, which nothing in either plan had noticed. Implementable as a clamp the day a snowline
exists.

### Uplift is SPIKY, which is a third argument for stacked sutures

Measured, all verbatim from primary sources: ordinary orogens run **1-3 mm/yr** (Kishtwar ~3, Western Alps
~2.5, Southern Alps 1-8); the hotspots run **9-13 mm/yr** (Nanga Parbat 9-13, Namche Barwa ~9) **and they
are narrow.** A range's uplift field is spiky along the belt, not a smooth dome -- arrived at here from
field measurement, having already been reached from reading shipped code and from terrane accretion.

### A second finding, possibly as large as the first

`MAX_TECTONIC_RANGE_M = 420_000` against a 400 km collision bump means **the collision profile is
essentially always-on wherever a margin is in range.** One reference project's equivalent cutoff is its
chain width times six, with chain width clamped to **4-18 km** -- two orders of magnitude tighter.
**Localisation may matter as much as grade**, and nothing in this slice has looked at it.

### LICENCES BIND HOW WE USE THIS

**World Orogen is GPL-3.0.** It was read for technique, which is fine and is what the study did. **No code,
shader source, or curated parameter bundle may be copied from it into this repository** -- an excerpt would
place copyleft obligations on this entire codebase.

**So do not transcribe its constants.** Ridged multifractal and domain warping are long-published techniques
(Musgrave and others) and reimplementing them is unencumbered. But the specific tuned values -- warp
frequencies, blend weights, octave gains -- are that project's curation. **Sweep for our own on our own
worlds**, which is better practice regardless: our planet is 4,500 km, theirs is not, and this project has a
survey-binary pattern for exactly this.

## Global Constraints

Every task's requirements implicitly include this section.

- **RULING 1 (inherited, and it is the whole safety argument): `worldbuilder/terrain/tectonics.py` HOLDS THE
  SAME CONSTANTS AND IS THE CONFORMANCE ORACLE.** Verified: `CONTINENT_COLLISION_M = 1500.0`,
  `CONTINENT_COLLISION_WIDTH_M = 400_000.0`, `COASTAL_UPLIFT_M = 900.0`, `ISLAND_ARC_M = 700.0` all appear
  there and are exercised by `tests/test_conformance.py`. **No default moves.** New parameters, opt-in,
  `None` byte-identical to today. A changed default is a change to the oracle and therefore the owner's
  decision, not a commit.
- **`worldbuilder/` must not be modified.** `worldbuilder/integration/maritime.py` has a pre-existing
  uncommitted change; **leave it unstaged. Never `git commit -a`.** `.claude/` is untracked; leave it.
- **`fixtures/dragonsire/` is a gitignored real game database.** It must never appear in a commit.
- **All transcendentals through `detmath`.** No `f64::` method or associated form, no `mul_add`, no bare
  integer cast without a `// cast-ok: <reason>` marker **on the same line**. `abs` is exempt.
- **Never `f64::min` / `f64::max` / `.clamp(`** -- NaN-asymmetric, and **the guard does not catch them**.
  `plates.rs::margin_at` is the house explicit-branch form.
- **`extern "C"` is nounwind.** **Three real aborts and one 2,600-second hang have been found in this
  project by SWEEPING export inputs, and zero by spot-checking.** Every one was a band rather than a cliff.
  **Sweep, do not spot-check.**
- **`cargo` is not on PATH in bash locally -- use `/c/Users/gary/.cargo/bin/cargo.exe`.** Never commit it.
- **Verify by exit status, never by grepping `test result:` lines.**
- **Every figure names its population, its method with parameters, and its host.** Counts and checksums are
  properties of the algorithm; milliseconds are properties of the moment. The same quantity has measured
  191.6, 55.7 and 178.1 ms on three hosts in this project, all supporting one conclusion.
- **Bless the manifest AFTER the last source edit**, rebuild, then re-derive all five pins with
  `assert_counts.py cargo-list` -- listed **minus** ignored. They are `517 / 517 / 519 / 566 / 568` with 5
  ignored as of `cd0c111`. Python conformance: **398/398 with `tests/test_conformance.py=157`**.
- **A survey binary must not run in CI.** `relief_survey.rs`, `erosion_convergence_sweep.rs` and
  `pond_threshold_survey.rs` are the house pattern.

---

### Task 1: `TectonicParams`, opt-in, and prove `None` is bit-identical

**Files:**
- Modify: `crates/worldbuilder-engine/src/tectonics.rs`
- Modify: `crates/worldbuilder-engine/src/surface.rs`

**Interfaces:**
- Produces: the struct below, verbatim, and `Surface::new`'s new parameter. Tasks 2-5 consume both.

Follow `ReliefParams` (in `detail.rs`) as the pattern -- it is the same shape, by the same argument, and it
landed cleanly at `eedb39d`.

```rust
pub struct TectonicParams {
    pub continent_collision_m: f64,       // 1500.0
    pub continent_collision_width_m: f64, // 400000.0
    pub coastal_uplift_m: f64,            // 900.0
    pub coastal_uplift_width_m: f64,      // 260000.0
    pub island_arc_m: f64,                // 700.0
    pub island_arc_width_m: f64,          // 110000.0
    pub ridge_m: f64,                     // 900.0
    pub ridge_width_m: f64,               // 380000.0
    pub continental_blend: f64,           // 0.45  how readily a margin counts as continental
}
impl TectonicParams { pub fn canonical() -> Self; }
```

`Surface::new(world_seed, radius_m, plate_count, land_fraction, features, relief, tectonics)`, following
`features: Option<FeatureInput>` and `relief: Option<ReliefParams>` -- **Ruling 2 of the relief slice: an
explicit `None` meaning canonical, never an implicit `Default::default()`.** This codebase rejects defaults
nobody chose.

**Do NOT include the trench or rift constants.** They are negative-going sea-floor features and this slice
is about mountains; adding them widens the surface without a request behind it.

- [ ] **Step 1: Write the failing test.** In `surface.rs`, a test that a world built with `None` and one
  built with `Some(TectonicParams::canonical())` agree bit-for-bit over a sampled population:

```rust
#[test]
fn tectonics_none_matches_tectonics_some_canonical_bit_for_bit() {
    let a = Surface::new(20260904, 6_371_000.0, 12, 0.29, None, None, None);
    let b = Surface::new(20260904, 6_371_000.0, 12, 0.29, None, None,
                         Some(TectonicParams::canonical()));
    for i in 0..2000 {
        let p = sample_point(i);                 // the file's existing sampling helper
        assert_eq!(a.elevation_m(p, None).to_bits(), b.elevation_m(p, None).to_bits());
        assert_eq!(a.structural_m(p).to_bits(), b.structural_m(p).to_bits());
    }
}
```

**`to_bits()` and not `==`** -- the claim is bit-identity, and `==` would pass on two different NaNs while
failing on `-0.0` versus `0.0`.

- [ ] **Step 2: Run it and watch it fail to compile** (`TectonicParams` does not exist).
- [ ] **Step 3: Add the struct and thread it through**, replacing each `pub const` use inside the uplift
  path with the field. **Leave the `pub const`s in place** -- they become `canonical()`'s values and the
  Python oracle still names them.
- [ ] **Step 4: Run the full matrix**, all five feature configurations, exit 0 each.
- [ ] **Step 5: Run Python conformance** under `WORLDBUILDER_REQUIRE_ENGINE=1`. **398/398 with
  `test_conformance.py=157`, unchanged. This is the only proof of Ruling 1 that matters.**
- [ ] **Step 6: Re-bless, rebuild, re-derive the five pins, update `gates.yml` with the reason. Commit.**

---

### Task 2: Measure the grade, do not choose it

**Files:** Create `crates/worldbuilder-engine/src/bin/mountain_survey.rs`

**Choose nothing.** Sweep, over a stated population of margin crossings.

**Interfaces:**
- Consumes: `TectonicParams`'s field names verbatim from Task 1.
- Produces: the tables Task 3 chooses from.

**The grid:**

- **`continent_collision_m`** at 1500 (today) and multiples up to at least 6000.
- **`continent_collision_width_m`** at 400 km (today) and **fractions down to at least 100 km** -- this is
  the axis nobody has swept and the plan's hypothesis is that it matters more than amplitude.
- **`continental_blend`** at 0.45 (today) and both directions -- this is the "more or fewer mountains" knob,
  since it governs how readily a margin counts as a continental collision at all.

**What to measure, per configuration:**

1. **Peak elevation** over the population.
2. **Maximum grade**, as rise over run across the margin -- the number that decides whether it reads as a
   mountain. Today's is **0.375%** by construction (1500 m / 400 km); real ranges are **3-8%**.
3. **Relief over a 2 km transect** at the steepest point, for continuity with the relief slice's tables.
4. **How many distinct uplift centres exceed 1,000 m**, which is the "how many mountains" figure.

**Report the interactions, not just the margins.** Amplitude and width multiply into grade; a one-at-a-time
sweep will mislead, exactly as it would have in the relief slice.

**Validation targets**, so a reader can judge: Hammond via USGS/MoRAP puts low mountains at **300-700 m** of
relief over 2 km and high mountains above that; the relief slice's roughness ceiling was **161 m**, which is
why this slice exists.

- [ ] **Steps:** write it, measure, record the tables, confirm it does not run in CI and builds under every
  feature configuration including wasm32, commit.

---

### Task 3: Choose named presets from Task 2's tables

**Files:** Modify `crates/worldbuilder-engine/src/tectonics.rs`

**Choose from Task 2's numbers. Do not re-measure a different population** -- the pond threshold in slice 5b
was chosen twice for exactly this reason, and the relief slice's Task 3 got this right by choosing against
the table it was handed.

Add named constructors beside `canonical()`. At least:

- **`ranges()`** -- what the owner asked for: mountains that read as mountains. Aim for a grade in the
  **3-8%** band and relief in Hammond's **low mountains** band or better.
- **`dramatic()`** -- if the tables support something sharper without breaking the surface, and only if.

**State the ground for every field you move**, as the relief slice's `hills()` did: it justified persistence
0.65 by the Hurst exponent against real terrain's measured band, the sign flip by a published mechanism, and
the multiplier by Hammond's bands. **A preset with no argument is just taste.**

**Ruling 1 still binds. `canonical()` does not move and no default changes.**

- [ ] **Steps:** choose, add the constructor, add a test that it moves only the named fields, prove the
  discrimination test can fail **by mutation** (and neutralise its siblings first -- **four assertions in
  this project have looked load-bearing and were not**), commit.

---

### Task 4: Reach it from the viewer -- the two sliders the owner asked for

**Files:** Modify `crates/worldbuilder-engine/src/wasm.rs`, `viewer/public/app/engine.js`,
`viewer/public/app/main.js`, `viewer/public/app/controls.js`, `viewer/public/app/panel-fields.js`

A validated block on the world spec plus a preset export, following `wb_relief_preset` and
`wb_world_new_relief` exactly. **Do not hardcode a second copy of a preset's values** -- the panel must
SHOW the numbers, so the preset crosses as fields, not as a name. That is Ruling 7 of the relief slice and
it is enforced there by a test that strips comments from the JS and asserts the numbers appear in neither.

**THE TWO CONTROLS, NAMED AS THE OWNER NAMED THEM:**

- **"mountain height"** -- `continent_collision_m`, and it must be honest: it moves height.
- **"mountain count"** -- `continental_blend`, which governs how many margins become ranges.

A third is worth offering because the measurement says it is the interesting one:

- **"mountain width"** or "steepness" -- `continent_collision_width_m`. **Narrowing this is what turns a ramp
  into a mountain**, and it is the axis the owner's "wheelchair ramps" complaint was actually about.

**Both entries must come OFF the panel's `NOT_WIRED` list**, where they currently sit reading *"tectonic,
not relief: roughness tops out at 161 m over 2 km"* and *"tectonic: plate collisions place them, and no
relief knob reaches that"*.

**Slider calibration is in scope and comes from Task 2's tables, not from taste.** A slider whose useful
range is a tenth of its travel is a slider nobody can aim. **Every new range control must satisfy
`panelFieldFaults()`** -- `panel-fields.js` holds one table the panel reads rather than restates, and it
already catches a default a slider cannot express. That check found a fourth instance of that bug nobody
had reported; do not add a fifth.

**Sweep the export.** `continent_collision_width_m` at or near zero is a division or a bump width of zero;
`continental_blend` outside [0, 1] is a blend nobody defined. **Validate at the boundary and return a
status; nothing clamps** -- a record is admitted as written or refused entire, because a silently-adjusted
parameter is a world nobody asked for.

- [ ] **Steps:** export, validate, sweep for aborts and hangs, wire the panel, prove `panelFieldFaults()`
  still returns empty, screenshot the result, commit.

---

### Task 5: Record it

**Files:** `crates/worldbuilder-engine/README.md`, `viewer/README.md`

**Read every number from the current source or your own runs, never from a report or a ledger.** This
project has caught **seven** transcription defects across four slices doing exactly what that forbids, and
one of them -- a peak quoted as the planet's when it was a sparse probe's maximum -- survived four tasks and
five shipped comment sites.

Cover: the 98.9%-tectonic measurement that motivated the slice; the 0.375% grade and why width mattered more
than amplitude; why the default cannot move while `worldbuilder/terrain/tectonics.py` is the oracle; the
presets and the ground they were chosen on; and **what still does not look right**.

- [ ] **Steps:** record, verify by running every command you document, commit.

---

## What this slice must NOT do

- **No default change and no `canonical()` change** -- Ruling 1, and the oracle governs 157 tests.
- **No trench or rift parameters.** Sea-floor features, no request behind them.
- **No erosion coupling.** Slice 5a shipped with uniform uplift; connecting them is its own work.
- **No climate.** The moisture spike is done and its finding is recorded; the slice is next, not now.
- **No changes to the relief path.** `mountain_m` there stays a roughness budget, and this slice is why it
  never needed to be anything else.
