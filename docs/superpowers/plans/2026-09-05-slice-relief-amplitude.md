# Slice relief-amplitude: mountains, not wheelchair ramps

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make it possible to generate a world with real relief at sub-kilometre scale, **without changing a single existing world by one bit**.

**Architecture:** The five roughness constants and the octave schedule become an optional parameter block on `Surface`. `None` reproduces today exactly. Nothing else changes.

**Spec:** No spec section governs relief amplitude. This is owner-requested, and its acceptance is measured relief.

## The measurement that motivates it

Taken against the running engine, seed 20260904:

- **The highest point on the planet is 1,381 m. Its tectonic component is 1,378 m. All surface detail contributes 2.9 m.**
- Across 113 sites above 300 m, the median magnitude of the detail term is **18.2 m** and the maximum is **65.5 m**.
- Relief over a 2 km transect, across 54 land sites: median **4.6 m**, max **14.1 m**. **The steepest gradient anywhere at that scale is 0.7%.**
- The field converges at ~156 m sampling and is flat below it: peak-to-peak over 2 km reads 9.66, 9.55, 9.56, 9.59, 9.58 m at 78, 39, 19.5, 9.8 and 4.9 m spacing.

## Three causes, and they multiply

**1. The roughness budget is small.** `detail.rs`: `ABYSSAL_M = 55`, `SHELF_M = 15`, `COAST_M = 35`, `INTERIOR_M = 80`, `MOUNTAIN_M = 150`. One hundred and fifty metres of roughness on a 1,400 m peak is a smooth dome.

**2. Roughness is damped hardest exactly where mountains are.**

```rust
let quieted = 1.0 - 0.7 * smooth(tectonic_m.abs() / 1200.0);
rough * quieted
```

So a real mountain gets `150 × 0.3 = 45 m`. The stated reason is honest and worth quoting, because it is a design choice and not an oversight:

> a deep, deliberate piece of structure stays legible instead of being buried under texture that has no idea it is there

**That is right for a navigational chart and wrong for a view.** Real uplifted ground is *rougher*, not smoother — it is where erosion has most to cut into.

**3. Amplitude halves per octave**, so the finest band carries 0.79% of the budget: shares run 50.4, 25.2, 12.6, 6.3, 3.2, 1.6, **0.79**%.

Compounded: the finest octave on a mountain carries about **35 centimetres**.

## CORRECTED, and it changes what this slice measures

An earlier draft said halving is "the smoothest end of the plausible range" and implied the fix was more
octaves, on the strength of Infinity's published 16-octave schedule. **The exponent was right and the
conclusion was wrong.**

**Persistence 0.5 at lacunarity 2 is f⁻³, not 1/f.** Pink noise — constant energy per octave — is persistence
**1.0**; Brownian f⁻² is **0.707**. This schedule is **two full spectral classes smoother than Brownian
motion**.

**And at H = 1.0 characteristic slope is scale-invariant**: slope ∝ L^(H−1) = L⁰. Every octave contributes the
*same* slope, so the series is pinned by the base octave's amplitude-to-wavelength ratio, about **1/500**.
**Adding fine octaves cannot help.** Proof by absurdity: a realistic 10% slope at 2 km from a single H=1
series would require 10% slope at 2,000 km too — **100 km of continental relief.** That impossibility is
what proves H must vary with scale.

**The resolution floor is roughly right.** Perron, Kirchner & Dietrich (2008), from lidar, find real
landscapes genuinely go quiet below hillslope length (β ≈ 4.5-5.2 above the roll-off). **The amplitude above
it is wrong by about 25×.**

**Calibration.** FAO/IIASA slope classes run C1 0-0.5% up to C8 >45%. This planet's median 0.23% gradient is
**C1** and its planetary maximum 0.7% is **C2** — the two flattest of eight, planet-wide, measured at a
baseline *below* this generator's own floor.

**What real terrain measures**, from Gagnon, Lovejoy & Schertzer (2006), four DEMs, >2×10⁸ pixels: β ≈
2.04-2.17, giving **H ≈ 0.6-0.71**; by regime **0.46 bathymetry, 0.66 continents, 0.77 margins.** Rougher
than the repeated "D ≈ 2.1-2.3" folklore, which traces to Burrough (1981) — a paper about heterogeneity,
flattened by retelling into a constant.

**And 0.5 is the universal library default** — libnoise, FastNoiseLite and Quilez's fbm all ship it. It was
inherited, not chosen.

### The schedule to measure

| Band | Wavelengths | H | persistence at l=2 |
|---|---|---|---|
| Continental | > 10 km | **0.50** | **0.71** |
| Relief | 100 m - 10 km | **1.0** | **0.50** (today) |
| Detail | < 100 m | **1.8** | **0.29**, hard-capped in metres |

Anchored at **10 km**, not the base octave. Predicted for median terrain: **~100-140 m over a 2 km transect**
against today's 4.6 m, with planetary relief still Earth-like. A mountain cell yields ~430 m and a 30% slope,
saturating near angle of repose. Use lacunarity **1.98 or 2.03** to avoid octave grid alignment.

Validation targets (Hammond via USGS/MoRAP): flat plains 10-25 m over 2 km, hills 80-160 m, low mountains
300-700 m. **About 50% of Earth's land is steeper than 5.5%** — roughly 40× this generator's median.

### RULING 3: the detail band is a SEPARATE ADDITIVE TERM defaulting to zero

**The shares are normalised, so adding an octave changes the normaliser and therefore every height on the
planet.** There is no "just adding a finer octave" here.

So the sub-100 m band gets its own term and its own budget, **defaulting to zero amplitude**: at default it
adds exactly `0.0`, output is bit-identical, and the normaliser is untouched. The geomorphology agrees —
Perron's above-roll-off exponent is **not reachable by any persistence inside a continuing series** without
wrecking the middle band. **The physics and the version policy point at the same design.**

Verify the no-op path does not change loop counts or summation order, and watch `-0.0` and NaN on the `+ 0.0`.

### Fixed exponent, spatially varying amplitude

Crooks et al. (~10⁹ points) found the scaling exponent **uncorrelated with regional roughness (r = 0.17)** —
plains and mountains obey the same exponent and only the amplitude prefactor varies, over three orders of
magnitude. That is empirical licence for varying amplitude at fixed exponent, which is far cheaper than
spatially varying H and fits the data at least as well.

## RULING 1: THE DEFAULT MUST NOT CHANGE. THIS IS THE WHOLE SAFETY ARGUMENT.

`worldbuilder/terrain/detail.py` carries **the same constants** — `CANONICAL_WAVELENGTH_M = 250.0`,
`ABYSSAL_M = 55.0`, `INTERIOR_M = 80.0`, `MOUNTAIN_M = 150.0` — and **that Python module is the conformance
oracle.** 150 tests compare Rust against it.

So: **new parameters, opt-in, default byte-identical to today.** A changed default is not a tuning decision,
it is a change to the oracle, and it would mean editing the reference implementation this project treats as
ground truth. **No task in this slice may change a default.** If the measurement argues for one, that is a
finding to report and a decision for the owner, not a commit.

The prize for obeying this: the conformance suite, the 56,254-value parity corpus, every stored seed and
`VERSION-001` all stay exactly as they are.

## RULING 2: `Option<ReliefParams>`, following the house pattern

`Surface::new(world_seed, radius_m, plate_count, land_fraction, features: Option<FeatureInput>)` already
takes an optional block. Add one more the same way. `None` takes the canonical path.

**Do not add five loose arguments**, and do not thread a struct through with `Default::default()` — this
codebase deliberately rejects defaults nobody chose (`stream.rs::BuildParams`'s own comment says so). An
explicit `None` meaning "canonical" is different from an implicit default, and it is the pattern already here.

## Global Constraints

- **All transcendentals through `detmath`.** No `f64::` method or associated form, no `mul_add`, no bare
  integer cast without a `// cast-ok: <reason>` marker **on the same line**. `abs` is exempt. The guard
  `tests/no_std_math.rs` fails the build and scans all of `src/`; it skips whole-line comments only.
- **Never `f64::min` / `f64::max` / `.clamp(`** — NaN-asymmetric, and **the guard does not catch them**.
  `plates.rs::margin_at` is the house explicit-branch form.
- **`worldbuilder/` must not be modified.** It is the oracle. `worldbuilder/integration/maritime.py` has a
  pre-existing uncommitted change; leave it unstaged. **Never `git commit -a`.**
- **`fixtures/dragonsire/` is a gitignored real game database.** It must never appear in a commit.
- **`cargo` is not on PATH in bash locally — use `/c/Users/gary/.cargo/bin/cargo.exe`.** Never commit it.
- **Verify by exit status, never by grepping `test result:` lines.**
- **Editing `src/` moves the source fingerprint.** Re-bless with `npm run build:wasm` **after** the last
  source edit, rebuild the Python extension, and confirm `npm run check:wasm`.
- **Re-derive the five engine count pins as tests that RUN — listed minus ignored.**
- **Every figure names its population, its method with parameters, its host, and where parameters combine
  into a governing group, the group.**

---

### Task 1: The parameter block, with a default that changes nothing

**Files:** Modify `crates/worldbuilder-engine/src/detail.rs`, `surface.rs`, `lib.rs`

`ReliefParams` carrying the five roughness amplitudes, the tectonic-quieting strength (currently `0.7`) and
its scale (currently `1200.0`), the octave persistence (currently `0.5`), and the finest wavelength
(currently `CANONICAL_WAVELENGTH_M = 250.0`). `Surface::new` takes `Option<ReliefParams>`.

**The whole test of this task is that nothing moved.** Assert that a `Surface` built with `None` and one
built with `Some(ReliefParams::canonical())` produce **bit-identical** elevations over a large sample —
compare bit patterns, not values — and that the existing conformance suite still passes unchanged.

**Prove the assertion can fail:** perturb one field of `canonical()` by one ULP and confirm the test goes
red. A bit-identity test that cannot fail is the defect this project keeps finding.

- [ ] **Steps:** failing test, run, implement, run, whole suite by exit status, commit.

---

### Task 2: Measure relief across the parameter space

**Files:** Create `crates/worldbuilder-engine/src/bin/relief_survey.rs`

**Do not choose anything yet.** Measure, over a stated population of land sites, for a grid of settings:

- `MOUNTAIN_M` at its current 150 and several multiples
- quieting strength at 0.7 (current), and reduced, and **inverted** so uplift roughens rather than smooths
- persistence at 0.5 (current), 0.65, 0.75

Report, for each: **relief over a 2 km transect** (median, p90, max), the **maximum gradient** anywhere, and
the **detail term's share of total elevation** on high ground. Those are the three numbers that decide
whether it looks like a mountain.

**Keep it out of the test suite** — CI runs those on every push. A binary, as `erosion_convergence_sweep.rs`
and `pond_threshold_survey.rs` already are.

- [ ] **Steps:** write it, measure, record the tables, commit.

---

### Task 3: Choose a named preset from the measurement

**Files:** Modify `src/detail.rs`

Not a new default — **Ruling 1 forbids that.** A named constructor beside `canonical()`, chosen from Task 2's
tables on stated ground, the way Task 3 of slice 5b chose the pond threshold on "a body a person could walk
around in fifteen minutes".

**Say what ground you chose on.** A gradient a person would call a mountainside is one candidate; real
terrain's Hurst exponent is another. **If the measurement shows no setting produces convincing relief
without also breaking something, that is a finding** — report it rather than shipping a preset that only
looks good in one screenshot.

- [ ] **Steps:** choose, test the preset's properties, commit.

---

### Task 4: Reach it from the viewer

**Files:** Modify `src/wasm.rs`, `viewer/public/app/engine.js`, `main.js`, `controls.js`

A new export, or an extension of `wb_world_new`. **`extern "C"` is nounwind** — validate every input and
return a status; this project has shipped two reachable aborts through exports whose bounds looked complete,
and both were bands rather than cliffs, so **sweep, do not spot-check**.

Then a control in the panel. The panel already lists what it cannot drive; this moves relief out of that list.

- [ ] **Steps:** export, validate, prove no abort, wire the panel, commit.

---

### Task 5: Record it

**Files:** `crates/worldbuilder-engine/README.md`

**Read every number from your own runs.** Cover: the three multiplying causes with their measurements; why
the default cannot move while `worldbuilder/terrain/detail.py` is the oracle; the preset and the ground it
was chosen on; and **what still does not look like a mountain**.

- [ ] **Steps:** record, verify by running, commit.

---

## What this slice must NOT do

- **Change any default, or touch `worldbuilder/`.** Ruling 1.
- **Change the octave *fade* behaviour.** Octaves fade in by `smooth((lambda/r - 2)/2)` for a recorded
  reason: dropping one abruptly is a cliff in resolution, and the ground would jump as somebody zoomed.
- **Add erosion-aware or biome-aware roughness.** Tempting, and a different slice.
- **Touch the relief-imagery work** — that is `2026-09-05-slice-relief.md`, viewer-side, and orthogonal.
