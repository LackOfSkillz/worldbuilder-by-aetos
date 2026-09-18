# Islands slice 1 (peaks) — calibration and verification

> **THIS DOCUMENT WAS WRITTEN FOR TASK 7 AND HAS BEEN AMENDED TWICE SINCE.** Read §§12–14 before
> quoting anything from §§1–11. The shipped density is **0.14**, not the 0.36 this report was
> written at; §13 records a defect that made every pre-§13 share a share of part of the planet;
> §14 carries the re-run calibration and a table of every figure that moved. Sections that still
> hold pre-fix figures on purpose carry a banner saying so.

Slice 1 of the islands plan grew a cellular seamount field — `PeakParams` and
`Tectonics::peak_offset_m`, a lattice of jittered candidate nodes that stand islands (or
submerged shoals) out of deep ocean — wired it through `Tectonics` and `Surface`, exposed and
pinned a wasm ABI for it, and gave the studio a panel. Task 7 calibrates it and reports.

**This slice's central claim is a negative one: with the block absent, nothing moved.** The
parity corpus reports **156,011 compared / 0 divergent** and all four controls are unmoved at
**147,387 / 216 / 60 / 22,995**. `GENERATOR_VERSION` is therefore **not** bumped, and the
reasoning is stated in full below rather than assumed.

**The one thing Task 7 changed in `src/` was a single constant.** `VOLCANIC_DENSITY` moved
**0.11 → 0.36** then, because 0.11 put every world measured *below* the spec's band; **the second
fix wave re-surveyed it to 0.14** after §13, and also restructured `Tectonics::offset_m` — so
"one constant, no code" describes Task 7 and not this branch's final state. Nothing in the
field's geometry moved at any point: `height_m`, `reach_m`, `min_depth_m` and `lattice_m` were
swept twice and left where Tasks 1 and 2 put them.

**The most interesting result is not the density.** It is that **the islanded share is
invariant under `(lattice_m, reach_m)` at a fixed ratio and strongly dependent on the world's
`land_fraction`** — so the pair sets island *size and count* rather than islanded *area*, and a
density calibrated on one world is not calibrated at all. Both are measured below.

---

## Host, and what "measured" means here

**Host:** K2SO, Windows 11, `rustc 1.98.0 (88d9e12ae 2026-08-18)`, `cargo … --release`,
single-threaded. Node for the viewer suite and the parity replay. Python 3.11.0 in the
repository `.venv` for the Python suites. Branch `islands-peaks`, baseline `88f199e`.

**Every figure below was obtained by running the thing and reading the output.** Nothing is
transcribed from an earlier task's report, nothing is scaled, and nothing is inferred from the
analytic model. Where a figure from an earlier task appears it is labelled as that task's and as
a *different population*. The one number in this report that is neither run here nor labelled as
another task's is the spec's own 65, which is quoted as a claim under test.

---

## 1. Spec §1's before-case: this generator made no islands, and still makes none without the block

Spec §1 states its measurement over **20,000 area-uniform points on the owner's world**. All
three survey worlds are measured, because §1's claim is about the generator rather than about
one planet.

**Population:** a Fibonacci spiral, `z = 1 - (2i + 1) / n`, the same area-uniform construction
`Continentality::calibrate` and `surface.rs`'s `fibonacci_point` use, at `n = 20,000` and
`n = 200,000`. **Method:** `island_survey.rs` section 1, each world built twice — once through
`Surface::new` (no peak block at all) and once through `Surface::with_peaks(… Some(canonical()))`
with every other argument identical. **Host:** as above.

| world | seed | radius | plates | `land_fraction` |
|---|---|---|---|---|
| `island-a` | 9001 | 6,371,000 m | 22 | 0.40 |
| `owner` | 562,423,712 | 4,500,000 m | 28 | 0.16 |
| `earth-a` | 20,260,904 | 6,371,000 m | 22 | 0.29 |

`island-a` is the fixture `surface.rs`'s own peak tests use, so the survey's share and the
pinned test's count are figures about the same planet. `owner` is spec §1's own world and the one
`coastline_survey.rs` also measures. `earth-a` is the parity corpus's seed.

### The before-case, at the canonical preset

Two discriminators, and they are different questions. **`D_added`** is `peaked.structural_m > 0`
where `plain.structural_m <= 0` — ground the block turned into land, which needs no threshold.
**`D_offshore`** is `structural_m > 0 && tectonics.offset_m > 2000`, the discriminator
`an_island_stands_above_the_datum_in_open_ocean` asserts on, and the only one of the two that is
meaningful when the block is inert (`D_added` is zero by construction then, because the two
surfaces are bit-identical).

| world | n | `Surface::new` `D_offshore` | `canonical()` `D_offshore` | achieved land, both |
|---|---|---|---|---|
| `island-a` | 20,000 | **0** | **0** | 40.1550% / 40.1550% |
| `owner` | 20,000 | **0** | **0** | 16.3150% / 16.3150% |
| `earth-a` | 20,000 | **0** | **0** | 29.2350% / 29.2350% |

**Still zero, on every world, and the block is inert.** `PeakParams::canonical()` has
`density: 0.0`, which `Tectonics::offset_m` gates on before touching the peak lattices at all;
`no_peak_block_means_a_bit_identical_surface` pins that bit-for-bit over 8,000 probes on both
`structural_m` and `elevation_m`.

### The island-arc term, measured rather than quoted

Spec §1 says the arc term is short of the abyss by about a factor of six — 700 m of
`ISLAND_ARC_M` against a −4,600 m `ABYSS_M`. What the live field actually reaches, over the same
spiral, at any point the continent field calls sea:

| world | n | points called sea | tallest `tectonics.offset_m` | short of 4,600 m by |
|---|---|---|---|---|
| `island-a` | 20,000 | 11,992 | **843.74 m** | 5.45× |
| `island-a` | 200,000 | 119,912 | **865.89 m** | 5.31× |
| `owner` | 20,000 | 16,794 | **892.85 m** | 5.15× |
| `owner` | 200,000 | 168,037 | **955.63 m** | 4.81× |
| `earth-a` | 20,000 | 14,207 | **824.57 m** | 5.58× |
| `earth-a` | 200,000 | 141,975 | **824.90 m** | 5.58× |

**Spec §1's factor of six is right in substance and slightly pessimistic in size**: the largest
offset the whole term reaches anywhere on these three planets is 955.63 m, so the real shortfall
is between 4.8× and 5.6×, not 6.6× (`4600 / 700`). The gap is because `offset_m` is the sum of
several terms and the arc constant is only one of them; nothing here contradicts the claim that
the arc alone cannot clear the abyss.

### The 65 points spec §1 calls shoreline wobble

Spec §1 records 65 points of 20,000 on the owner's world that "the finer layers lift out of
water", and calls them shoreline wobble rather than shoals. Measured here, counting points the
continent field calls sea whose `structural_m` is nevertheless above the datum:

| world | n = 20,000 | n = 200,000 | tallest among them (n = 200,000) |
|---|---|---|---|
| `island-a` | 26 | 346 | 737.48 m |
| **`owner`** | **57** | 638 | 757.02 m |
| `earth-a` | 55 | 543 | 798.19 m |

**57 against the spec's 65 on the same world at the same sample size**, the residual being that
the two runs draw the ocean/land line at different places (this one at
`Continentality::above_shore <= 0`). The phenomenon is the same one and its magnitude is
confirmed. These are not islands and must not be counted as any: they are shoreline points, and
`D_offshore` — which requires 2,000 m of tectonic offset — is zero on all three worlds.

---

## 2. The calibration, and the trap it was built to avoid

> **SUPERSEDED — measured against a suppressed field.** Every share in this section was measured
> before §13's defect was found: `Tectonics::offset_m` never reached the seamount term on
> **77.16%** of the planet, so each figure below is a share of the quarter of the world where the
> field was evaluated. The defect is fixed and the whole sweep was re-run; **§14 carries the
> corrected tables and the density they chose.** This section is kept because it is a correct
> measurement of what the code then did, and because it is why 0.36 was picked.

### The trap, stated first

**Measure through the full `Surface` pipeline, over ocean. Never through `peak_offset_m` against
an assumed seabed.** Task 1's sweep pinned `seabed_m` to `ABYSS_M` at every probe, so
`peak_depth_window` returned 1.0 unconditionally and the 0.54% it reported is what the field
would make *if the whole planet were abyssal ocean, land included*. It is a measurement of the
field, on a fiction. Task 3 re-measured the same constants through `Surface` and found **0.17%**
over 20,000 points — threefold lower — and measured the three factors that account for the gap
on that fixture: only 59.8% of the sphere is ocean at all, only 67.5% of that ocean is deeper
than the 2,500 m window threshold, and only 20.6% of it actually reaches `ABYSS_M`.

`island_survey.rs` measures `D_added` through `Surface::structural_m` on two worlds built
identically but for the block. The analytic model in `VOLCANIC_REACH_M`'s doc chose where to
sample and nothing else; **no figure in this report is the model's output.**

### The density sweep on one world, and why one world is not enough

**Population:** the 200,000-point spiral on `island-a`. **Method:** `island_survey.rs` section 2,
`density` swept at the shipped geometry (`lattice_m` 45,000 / `reach_m` 31,500, ratio 0.70).
Binomial standard error at `p = 0.005, n = 200,000` is `sqrt(0.005·0.995/200000)` = **±0.0158 pp**
at 1σ — an order below the 0.5 pp band, so this estimator can see the band's edges. At
`n = 20,000` it is ±0.05 pp, which is why the decision was not made there.

| density | n = 20,000 | **n = 200,000** | band |
|---|---|---|---|
| 0.11 *(shipped before Task 7)* | 0.1700% | **0.1070%** | BELOW |
| 0.16 | 0.2050% | 0.1410% | BELOW |
| 0.20 | 0.2200% | 0.1835% | BELOW |
| 0.24 | 0.2650% | 0.2240% | BELOW |
| 0.28 | 0.3300% | 0.2625% | BELOW |
| 0.32 | 0.3950% | 0.3065% | in band |
| **0.36** | 0.4400% | **0.3345%** | in band |
| 0.40 | 0.4900% | 0.3785% | in band |
| 0.45 | 0.5300% | 0.4150% | in band |
| 0.50 | 0.6050% | 0.4675% | in band |
| 0.55 | 0.6550% | 0.5140% | in band |
| 0.58 | 0.6700% | 0.5495% | in band |
| 0.60 | 0.6800% | 0.5680% | in band |
| 0.62 | 0.7050% | 0.5840% | in band |
| 0.70 | 0.7750% | 0.6670% | in band |
| 0.75 | 0.8100% | 0.7165% | in band |

Two things to read from this table beyond the numbers.

**The brief's expectation was close but not right, and measurement is why that is known.** The
task brief reasoned that density is linear in the result and that ~0.32 should therefore land
mid-band. Density *is* very nearly linear here — 0.1070% at 0.11 and 0.7165% at 0.75 is 0.951
%/unit against the two-point slope of 0.996 %/unit between 0.50 and 0.75 — but 0.32 lands at
**0.3065%**, which is the band's *floor*, not its middle. Extrapolating from 0.11's 0.17% (the
20,000-point figure) rather than from its 0.107% (the 200,000-point one) is what produces the
error, and it is exactly the kind of error the constraint about re-deriving figures exists to
catch.

**The n = 20,000 column runs high at every single density, and that is not noise.** It reads
0.04 to 0.14 pp above the 200,000-point figure — 0.17% against 0.107% at density 0.11, 0.44%
against 0.3345% at 0.36 — with the gap growing roughly in proportion to the share. Each
individual gap is only 1 to 2.5σ of the smaller sample, so no one row is remarkable; but **the
sign is the same on all sixteen rows**, which happens by chance with probability 2⁻¹⁶. (The
final whole-branch review's minor 5 found this claim computed over sixteen rows while the table
above published only fourteen -- 0.55 and 0.60 were missing. They are published now, from a
re-run of `cargo run --release --bin island_survey` on the host named at the top of this report:
0.6550% against 0.5140% at 0.55, and 0.6800% against 0.5680% at 0.60. Both run high like the
other fourteen, so the population the 2⁻¹⁶ is computed over is the population the table shows.)
So this
is a property of *this particular 20,000-point lattice* — it evidently samples more of the deep
ocean these islands live in than an average sample of that size would — and not of the field.

**Consequence: the 20,000-point count must not be quoted as the islanded share.** The number to
quote is the survey's 200,000-point figure, which the 32-million-point raster in §4 independently
corroborates to 0.0008 pp. The pinned test's doc comment now says so in as many words, and the
test keeps its 20,000-point count only as a cheap band check.

### Why the choice cannot be made on one world

**Population:** the 200,000-point spiral on each of the three worlds. **Method:**
`island_survey.rs` section 4, same geometry, `D_added`. A density is admissible only if its
**worst** world is inside the band, because the constant is one number and the spec's band is
about a planet.

| density | `island-a` (land 0.40) | `owner` (land 0.16) | `earth-a` (land 0.29) | margin to the nearer band edge |
|---|---|---|---|---|
| 0.28 | 0.2625% | 0.5850% | 0.3290% | **−0.0375 pp** — `island-a` BELOW |
| 0.31 | 0.2955% | 0.6455% | 0.3545% | **−0.0045 pp** — `island-a` BELOW |
| 0.32 | 0.3065% | 0.6655% | 0.3660% | +0.0065 pp |
| 0.33 | 0.3165% | 0.6875% | 0.3730% | +0.0165 pp |
| 0.34 | 0.3215% | 0.7065% | 0.3850% | +0.0215 pp |
| 0.35 | 0.3295% | 0.7310% | 0.3890% | +0.0295 pp |
| **0.36** | **0.3345%** | **0.7525%** | **0.4035%** | **+0.0345 pp — the maximin** |
| 0.37 | 0.3470% | 0.7710% | 0.4170% | +0.0290 pp |
| 0.38 | 0.3585% | 0.7905% | 0.4325% | +0.0095 pp |
| 0.39 | 0.3675% | 0.8120% | 0.4405% | **−0.0120 pp** — `owner` ABOVE |
| 0.40 | 0.3785% | 0.8280% | 0.4545% | **−0.0280 pp** — `owner` ABOVE |
| 0.45 | 0.4150% | 0.9390% | 0.5165% | −0.1390 pp |
| 0.58 | 0.5495% | 1.2135% | 0.6730% | −0.4135 pp |

**A world with less land has more deep ocean for the field to stand an island in, and the effect
is large: at every density the owner's 0.16-land world yields roughly 2.2× the share the
0.40-land fixture does.** So the band is squeezed from *both* sides at once — `island-a` presses
the 0.3% floor while `owner` presses the 0.8% ceiling — and the admissible window is only
**seven admissible hundredths, 0.32 through 0.38, so six hundredths of span**. **0.36 is the
maximin**: the admissible density whose worst world sits furthest from a band edge. 0.32 clears
the floor by 0.0065 pp, which is half the estimator's own 1σ error and would not survive a
fourth world.

**Minor 6 of the final whole-branch review found this sentence and `VOLCANIC_DENSITY`'s doc
disagreeing** — "seven hundredths wide" here against "six hundredths wide" there. Neither number
was wrong; each was the other quantity, written as if it were this one. Both now say both. And
the window's two edges are measured rather than inferred from the rows four and two hundredths
outside it: `island_survey.rs`'s `CANDIDATES` gained 0.31 and 0.39 and was re-run on the host
named at the top of this report, giving the two new rows above. 0.31 misses the floor by
0.0045 pp and 0.39 clears the ceiling by 0.0120 pp, so the admissible set is exactly the seven
hundredths 0.32–0.38 and nothing hides in the gaps.

### The chosen constants

| field | before Task 7 | **after** | how chosen |
|---|---|---|---|
| `VOLCANIC_HEIGHT_M` | 8,000.0 | **8,000.0** | Task 2's; confirmed by the sweep, which reaches the band at this height |
| `VOLCANIC_DENSITY` | 0.11 | **0.36** | `island_survey.rs` section 4, maximin over three worlds |
| `VOLCANIC_REACH_M` | 31,500.0 | **31,500.0** | swept; the share is invariant under the pair (§3 below) |
| `VOLCANIC_MIN_DEPTH_M` | 2,500.0 | **2,500.0** | untouched |
| `VOLCANIC_LATTICE_M` | 45,000.0 | **45,000.0** | swept; sets island size and count, not area (§3 below) |

**Which run each figure came from:** every share in the two tables above is from a single
`cargo run --release --bin island_survey` on the host named at the top, sections 2 and 4, at
`VOLCANIC_DENSITY = 0.36`. Sections 2 and 4 rebuild each configuration from scratch and do not
read the shipped constant except to print it, so the tables are the same whatever the constant
is set to; the run that chose 0.36 and the run that confirms it are the same binary on the same
data.

**Hundredths, and not 0.35.** `viewer/public/app/peak-params.js`'s density slider carries an
integer position and maps it to a value by dividing by 100, so a density off that lattice is one
the panel cannot reach — the defect `panelFieldFaults()` exists for. 0.36 is position 36. It is
also deliberately not `CoastParams::fractal()`'s 0.35: `viewer/test/peak-params.test.mjs`'s "no
peak number is written down twice" test scans the viewer's sources for the preset's own
distinctive literal, and at 0.35 that scan would have been indistinguishable from the coast
channel's identical one — a scan that passes only because another channel's guard already holds
is a scan that tests nothing of its own. 0.35 and 0.36 have the same maximin to within 0.005 pp,
so this cost nothing.

---

## 3. What `lattice_m` and `reach_m` actually control

**Population:** the 200,000-point spiral on `island-a`. **Method:** `island_survey.rs` section 3,
holding `reach_m / lattice_m` at 0.70 and moving the **pair**. Both fields are written out as
exact decimals rather than derived by multiplying, because `45_000.0 * 0.70` is
`31499.999999999996` — the first revision of this binary did derive them and printed the wrong
`reach_m` in its own header as evidence.

**RE-RUN after §13's fix and §14's re-survey.** The pre-fix table is underneath.

| `lattice_m` / `reach_m` | density 0.11 | **density 0.14 (shipped)** | density 0.58 |
|---|---|---|---|
| 30,000 / 21,000 | 0.3385% | **0.4305%** | 1.8170% |
| 45,000 / 31,500 | 0.3650% | **0.4480%** | 1.8410% |
| 67,500 / 47,250 | 0.3530% | **0.4365%** | 1.8380% |
| 90,000 / 63,000 | 0.3460% | **0.4475%** | 1.8030% |

*(Pre-fix, at density 0.11 / 0.36 / 0.58: 0.0970 / 0.3290 / 0.5460, 0.1070 / 0.3345 / 0.5495,
0.1065 / 0.3350 / 0.5420, 0.1080 / 0.3310 / 0.5200.)*

**The islanded share is invariant under the pair.** At the shipped density the spread across an
eightfold change of lattice volume is **0.0175 pp**, against the estimator's own 1σ error of
about **0.0150 pp** at this share — so still about one sigma, and still fairly called
unmeasurable. (Pre-fix the spread was 0.006 pp against 0.013 pp. It grew roughly with the share,
as a binomial spread does, and not relative to it.) That is what the model predicts (`d(share)`
scales with `reach_m`, so `d³ / lattice_m³` is scale-free) and it is measured rather than
supposed. **So the ratio is the lever and neither field alone is one**, which is why both
calibrations moved `density` and left these where they were.

What the pair *does* control is measured in §4.

---

## 4. Island count and the distribution of island areas

**Population:** an equirectangular lat/lon raster of the whole sphere at **5,000 m** spacing on
`island-a` — 4,003 rows × 8,006 columns, **32,048,018 sample points**, each evaluated on both the
peaked and the plain surface. **Method:** `island_survey.rs` section 6 (`-- components`), built
exactly as `coastline_survey.rs::Raster` builds its own: rows at row centres, cell area
`radius_m² · dlat · dlon · cos(lat)`. A cell is an island cell when `D_added` holds there.
Components are 4-connected with longitude wrapping and the poles not joined; the union-find runs
over the island cells alone rather than the whole grid, which is what makes a 5 km raster
affordable (about 111 s per configuration on this host). **A count at spacing `d` cannot see an
island below about one cell**, so the spacing is stated with every count and counts are only ever
compared at the same spacing.

**RE-RUN after §13's fix and §14's re-survey.** The figures are the current ones; the pre-fix
run, at `density: 0.36` against a field suppressed over 77% of the planet, is given underneath
each for comparison.

| configuration | island cells | **distinct islands** | total island area | share of sphere | mean |
|---|---|---|---|---|---|
| **density 0.14, lattice 45,000 (shipped)** | 147,121 | **6,137** | 2,231,588 km² | **0.4375%** | 363.6 km² |
| density 0.11, lattice 45,000 | 118,143 | 4,831 | 1,770,676 km² | 0.3471% | 366.5 km² |
| density 0.14, lattice 30,000 | 147,350 | **13,506** | 2,253,736 km² | 0.4419% | 166.9 km² |
| density 0.14, lattice 90,000 | 140,120 | **1,558** | 2,206,039 km² | 0.4325% | 1,415.9 km² |
| *(pre-fix)* density 0.36, lattice 45,000 | 106,475 | 4,617 | 1,702,268 km² | 0.3337% | 368.7 km² |
| *(pre-fix)* density 0.36, lattice 30,000 | 104,689 | 10,155 | 1,694,766 km² | 0.3323% | 166.9 km² |
| *(pre-fix)* density 0.36, lattice 90,000 | 103,380 | 1,213 | 1,643,372 km² | 0.3222% | 1,354.8 km² |

### Area distribution at the shipped constants

| | largest | p90 | median | p10 | smallest |
|---|---|---|---|---|---|
| **density 0.14, lattice 45,000** | **1,732.5 km²** | 705.0 km² | **327.6 km²** | 73.5 km² | 7.0 km² |
| density 0.11, lattice 45,000 | 1,732.5 km² | 713.3 km² | 334.1 km² | 74.4 km² | 7.0 km² |
| density 0.14, lattice 30,000 | 1,199.9 km² | 320.9 km² | 149.5 km² | 37.2 km² | 2.4 km² |
| density 0.14, lattice 90,000 | 6,351.4 km² | 2,839.1 km² | 1,268.9 km² | 252.6 km² | 21.1 km² |

Histogram at the shipped constants, by area: **< 50 km² 461 · 50–200 km² 1,479 · 200–500 km²
2,570 · 500–2,000 km² 1,627 · 2,000–10,000 km² 0 · ≥ 10,000 km² 0.** No island on this world
exceeds 2,000 km² at all now (the pre-fix run had ten) — so nothing the field makes is
continent-sized, which is what "islands, not fragments" is supposed to mean at this slice.

Three findings worth separating out.

1. **Two independent estimators agree on the share.** The 200,000-point spiral says 0.4480% and
   the 32-million-point raster says 0.4375% — 0.0105 pp apart, inside the spiral's own ±0.015 pp
   1σ. The spiral figure is the calibration's; the raster corroborates it on a population three
   orders of magnitude larger and by a different construction. (Pre-fix the two read 0.3345% and
   0.3337%, 0.0008 pp apart. The agreement is looser now and honestly so — one run of each, and
   0.7σ of the smaller estimator is not a worse agreement than 0.05σ, only a differently lucky
   one.)
2. **Density moves count, not size. The lattice moves size, not area.** Raising density 0.11 →
   0.14 raised the count by a quarter (4,831 → 6,137) and left the mean essentially untouched
   (366.5 → 363.6 km²) and the median within 2% (334.1 → 327.6 km²). Moving the lattice 30 km →
   90 km changed the count eightfold *down* (13,506 → 1,558) and the mean eightfold *up* (166.9 →
   1,415.9 km²) while total area stayed inside 0.432–0.442%. **This is §3's invariance seen from
   the other side**, and it is the measured reason the calibration knob is `density`. The three
   mean sizes are within 1% of the pre-fix run's at the same pitches: §13's fix changed how MANY
   islands there are, not how big one is.
3. **Roughly a fourteenth of islands are at or below the raster's own resolution floor.** 461 of
   6,137 are under 50 km² — two cells of 25 km² each — so the smallest bin is resolution-limited
   and the true count of very small islands is higher than 461. The count of islands above
   200 km² (**4,197**) is the resolution-safe figure. Spec §5's note that "islands smaller than the node
   spacing have no hydrology" bites here: at 1,000,000 hydrology nodes the spacing is about 33 km,
   so anything below roughly 1,000 km² gets zero or one node — which, on this distribution, is
   the great majority of them.

---

## 5. Steep-to: deep water a short way off an island, against a continental shelf

**Population:** one island summit on `island-a` — the tallest point the 200,000-point spiral
found where `D_added` holds — and one continental shore point, the *lowest* land point the same
spiral found on the plain world. **Method:** `island_survey.rs` section 5. `TangentFrame::at`
and eight compass bearings at each distance, reporting the deepest and shallowest
`structural_m` on each ring. **Host:** as above.

**RE-RUN after §13's fix and §14's re-survey.** The summit stands **4,291.15 m** with a tectonic
offset of **6,962.95 m**. (Pre-fix, at `density: 0.36`, it was 4,773.47 m / 7,484.31 m — a
different node: `peak_of_cell` gates on `hash >= density`, so lowering the density removes
candidate cells and the tallest one the survey finds moves. No geometry changed.)

| distance off the island | deepest of 8 bearings | shallowest of 8 |
|---|---|---|
| 0.20 × `reach_m` = 6,300 m | +2,737.76 m | +4,572.47 m |
| 0.40 × `reach_m` = 12,600 m | +603.91 m | +3,396.46 m |
| 0.60 × `reach_m` = 18,900 m | **−1,383.05 m** | +1,351.37 m |
| 0.80 × `reach_m` = 25,200 m | **−2,494.53 m** | −838.33 m |
| **0.95 × `reach_m` = 29,925 m** | **−2,695.04 m** | −2,137.37 m |
| 1.10 × `reach_m` = 34,650 m | −2,800.63 m | −2,494.37 m |

The contrast case, from a continental shore on the *same world* with no peak block involved.
**`SHELF_BREAK_M` is 80,000 m**, so the shelf is expected to run about that far before it breaks:

| distance off the continental shore | deepest of 8 bearings | shallowest of 8 |
|---|---|---|
| 10,000 m | −6.52 m | +13.80 m |
| 20,000 m | −24.89 m | +25.53 m |
| 40,000 m | −84.36 m | +44.16 m |
| **80,000 m (`SHELF_BREAK_M`)** | **−153.49 m** | +78.64 m |
| 120,000 m | −248.39 m | +111.21 m |
| 200,000 m | −354.54 m | +174.20 m |

**At 30 km off an island the water is 2,695 m deep; at 80 km off a continent it is 153 m deep.**
That is a factor of eighteen at a quarter of the distance, and it is the navigational difference
between an oceanic volcano and a continental margin — the thing a bundle's soundings would show.
The continental column is unchanged to the centimetre, which it must be: no peak block is
involved in it, and §13's bit comparison says the plate-only path did not move.
The mechanism is why peaks go into the tectonic offset at all: `Shelf::weight`'s authority is
`1 - smooth(|tectonic_m| / 250)`, so a 6,963 m offset holds the shelf off entirely (the shelf
weight at this summit reads **0.0000**, measured, in §6 below).

**Two summit figures are in circulation and they are different populations, not a
disagreement.** `an_island_is_steep_to_rather_than_shelved` reports **−2,695.04 m** off a summit
standing **4,291.15 m**; its `find_a_summit` searches 40,000 points with the `offset_m > 2000`
discriminator and the survey searches 200,000 with `D_added`, and at this density both happen to
land on the same node (at 0.36 they landed on different ones, −2,907.71 m off 4,010.75 m against
the survey's −2,738.70 m off 4,773.47 m). Both are re-derived in this task by running them, and
both are past the 1,000 m bar by a wide margin.

---

## 6. What a peak does to substrate and to detail roughness — measured, not asserted

Spec §4's table claims a large tectonic offset **saturates** `substrate::natural`'s
`by_tectonics` term (100% rock) and **saturates** `Detail::amplitude_m`'s quieting. Both claims
are checked rather than restated.

**Population:** two points on `island-a` — the island summit above, and a point 1.1 × `reach_m`
away from it on the same bearing. **Method:** `island_survey.rs` section 5. `substrate::natural`
and `Detail::amplitude_m` are called directly on the arguments the elevation path itself hands
them: `surface.shelf.evaluate(point)` supplies `weight` and `tectonic_m`, and
`substrate::slope_at(radius_m, point, 1000.0, structural_m)` supplies the slope. The relief block
is `ReliefParams::canonical()`, which is what `Detail::with_gully` builds from the `relief: None`
these worlds are constructed with.

**RE-RUN after §13's fix and §14's re-survey**, off the summit §5 now finds.

| | at the summit | 1.1 × `reach_m` off it |
|---|---|---|
| `elevation_m` | **+4,291.15 m** | −2,494.37 m |
| `tectonic_m` | **+6,962.95 m** | **16.94 m** |
| shelf `weight` | **0.0000** | 0.0000 |
| slope | 0.16267 | 0.02071 |
| `natural` → sand / mud / **rock** | 0.000000 / 0.000000 / **1.000000** | 0.000000 / 0.473372 / **0.526628** |
| `by_tectonics` = `smooth(\|tectonic_m\| / ROCK_TECTONIC_M)` | `smooth(6962.95/1200)` = `smooth(5.8025)` = **1.000000** | `smooth(0.0141)` = **0.000592** |
| `by_slope` = `smooth(slope / ROCK_SLOPE)` | `smooth(0.16267/0.04)` = **1.000000** | `smooth(0.02071/0.04)` = **0.526628** |
| `Detail::amplitude_m` | **45.000000 m** | 57.630360 m |
| the same with `tectonic_m = 0` | **150.000000 m** | 57.654260 m |
| quieting = `1 − quieting_strength · smooth(\|tectonic_m\| / quieting_scale_m)` | `1 − 0.70 · smooth(5.8025)` = **0.300000** | `1 − 0.70 · smooth(0.0141)` = **0.999585** |

**Both spec claims hold, and both are saturated with room to spare rather than marginally.**

- **`by_tectonics` saturates.** `ROCK_TECTONIC_M` is 1,200 m and the argument is 5.80 — `smooth`
  clamps at 1.0 above an argument of 1, so the summit is at **5.8× the saturation threshold.**
  Rock is exactly 1.000000 and both loose fractions are exactly zero.
- **The quieting saturates too, and it costs 105 m of roughness.** `quieting_strength` is 0.70
  and `quieting_scale_m` is 1,200 m, so a saturated quieting multiplies roughness by exactly
  0.300000. Measured: **45.000000 m against the 150.000000 m the same point would get at
  `tectonic_m = 0`** — a 70% reduction, exactly the strength constant, because the quieting is at
  its floor. An island's flanks are therefore *smoother* than the seabed a kilometre away
  (57.63 m), which is the intended behaviour: deliberate deep structure keeps its shape.

**One honest caveat, which is why the decomposition is printed and not just the composition.**
`by_slope` *also* saturates at the summit (slope 0.158 against `ROCK_SLOPE` 0.04), and
`natural` takes the **larger** of the two terms. So "100% rock at a summit" is
over-determined: it would be 100% rock from slope alone. The claim that the *tectonic* term
saturates rests on the decomposition — `smooth(5.8025) = 1.000000` — and on the contrast point,
where the slope is 0.02071, `by_slope` is 0.526628, and the composition reads exactly that
0.526628 while `by_tectonics` reads 0.000592. Asserting the spec's claim from the composition
alone would have been an unfalsifiable measurement.

**The contrast point is a weaker contrast than it was and the reason is §13's fix, not a
regression.** Pre-fix it read `tectonic_m` exactly **0.00 m** — because it sat outside margin
range, where `offset_m` returned zero *before* the seamount term, so the plate field and the
seamount field were both silent there. It now reads **16.94 m**: a real, small tectonic offset,
which is what a point 35 km off a seamount should have. The decomposition is still unambiguous
(0.000592 against 0.526628, three orders apart), and the composition still reads `by_slope`
exactly.

---

## 7. Requested against achieved land fraction, and the API spec §5 asks for

**Population:** the 200,000-point spiral on each world. **Method:** `island_survey.rs` section 4b,
the share of the spiral with `structural_m > 0`, at the canonical preset and at `volcanic()`.

**RE-RUN after §13's fix and §14's re-survey.** The canonical-preset column is unchanged to the
digit, which it must be — no block is involved in it.

| world | requested `land_fraction` | achieved, canonical preset | achieved, `volcanic()` | islands add |
|---|---|---|---|---|
| `island-a` | 0.4000 | 0.4021 | **0.4065** | **+0.4480 pp** |
| `owner` | 0.1600 | 0.1630 | **0.1694** | **+0.6380 pp** |
| `earth-a` | 0.2900 | 0.2928 | **0.2980** | **+0.5210 pp** |

*(Pre-fix, at `density: 0.36`: 0.4054 / 0.1705 / 0.2969, adding 0.3345 / 0.7525 / 0.4035 pp.)*

Two separate gaps are visible and they have different causes.

- **The canonical preset already misses the requested figure by 0.21 to 0.30 pp**, with the block
  absent entirely. That is the calibrator's own residual: `Continentality::calibrate` hits
  `land_fraction` on its own 4,000-sample estimator, whose 1σ error is about ±0.58 pp, and this
  200,000-point estimator measures the result to ±0.1 pp. This slice neither caused nor changed
  it.
- **`volcanic()` adds land on top, by exactly `D_added`** — 0.4480 / 0.6380 / 0.5210 pp, the same
  three numbers as the islanded share, which is a consistency check rather than a coincidence:
  peaks only raise ground and never lower it, so every square metre they add to land is a square
  metre of island. It reproduces at the new density as it did at the old, which is the check
  doing its job across a recalibration.

### Spec §5's accessor is deliberately not added, and that is the record

Spec §5 says `Surface` "should be able to report both numbers: requested fraction, and achieved
fraction including islands." **This slice measures both in the survey and adds no API.** The
reasoning, stated so the gap is a decision rather than an omission:

- **Peaks are added *after* calibration.** `land_fraction` stays true of the continents exactly as
  documented, and islands sit on top of it. The gap between requested and achieved is therefore
  additive, bounded by the islanded share, and reportable — 0.33 to 0.75 pp on these three
  worlds, against the calibrator's own 0.21–0.30 pp residual. Nobody is misled by a figure that
  is still true of the thing it describes.
- **Fragments are what make the requested number actually wrong**, because they sit *inside*
  calibration: switching them on moves the threshold and slightly shrinks the continents to keep
  the total at the requested value, so `land_fraction` stops meaning "continental land" without
  anything in the signature changing. That is the asymmetry spec §5 says must be documented
  rather than smoothed over, and it is the point at which an accessor stops being a convenience
  and starts being necessary.
- **So the accessor lands with fragments, in slice 3.** Adding it now would also collide with a
  hard constraint of this plan: `lib.rs:416-443` reads `surface.rs`'s source text and asserts
  exactly eight public fields by name plus `steer` as the named ninth, and `constraints.md`
  forbids adding a field to `Surface`. An accessor computing an achieved fraction on demand would
  need a sample count and a point construction as parameters — i.e. it would need to be the
  survey — or it would need cached state, which is a field.

---

## 8. Parity, the four controls, and why `GENERATOR_VERSION` is not bumped

**This is the slice's central claim.** An absent block must produce bit-identical worlds — not
close, identical.

**Population:** `examples/parity_dump.rs`'s whole corpus, **156,011 values** across 47 groups
(scattered ocean, the placed harbour, tiles, climate, erosion, water, hydro, and the five
non-canonical block worlds with their belts and tiles). **Method:** the native dump
(`cargo run --release --example parity_dump --features wasm`) carried to
`parity/parity.mjs` as 16-hex-digit bit patterns and replayed through **the committed
`.wasm`** in Node. Node parses no decimal text and recomputes no input, so a mismatch is a real
disagreement. **Host:** as above.

| | baseline `88f199e` | **after Task 7** |
|---|---|---|
| Provenance | the shipped `.wasm` matches its manifest and current source | **same** |
| Parity, compared / divergent | 156,011 / **0** | **156,011 / 0** |
| `--mutate seed` | 147,387 | **147,387** |
| `--mutate erosion-k` | 216 | **216** |
| `--mutate water-pond` | 60 | **60** |
| `--mutate tectonic-warp` | 22,995 | **22,995** |

All four controls also printed their own explanatory lines unchanged — `water-pond`'s *"60 of
1095 water values moved, exactly the bodies the native surface-area distribution predicted, and
no value outside the water group moved at all"*, and `tectonic-warp`'s per-group breakdown
(`elevation/ranges` 132, `structural/ranges` 132, `elevation/belt` 1,269, `structural/belt`
1,269, `tile/belt` 3,384, `hydro/ranges` 16,807, `water_point/ranges` 2) with *"exactly as the
native side predicted, and every other group … moved nothing at all."*

**Nothing moved, and the reason is structural rather than lucky.** The corpus exercises the
canonical path, where the peak block is **absent** — every world in it is built through
`wb_world_new`, `wb_world_new_relief`, `wb_world_new_tectonic`, `wb_world_new_coast` or
`wb_world_new_gully`, none of which carries a `PeakParams`. `Tectonics::offset_m` gates on
`density` before touching the peak lattices at all, and `VOLCANIC_DENSITY` is only ever read by
`PeakParams::volcanic()`, which nothing on the canonical path constructs. **Changing a default
constant cannot move a corpus that never reads it.** Had anything moved, that would have been a
defect in the gating rather than a figure to re-pin, and this report would say BLOCKED.

### Why `GENERATOR_VERSION` is not bumped, stated explicitly

`GENERATOR_VERSION`'s bump test is: **would the same seed *and the same parameters* through the
new code produce a different world?**

- **With the block absent — every existing caller, and the whole parity corpus — no.** Bit-identical, and the run
  above is the evidence: 156,011 values, 0 divergent, through the shipped exports.
- **With the block present**, the answer depends on `density`, which is a *parameter*. A caller
  who passes an explicit `PeakParams { density: 0.11, .. }` gets exactly the world they got
  before. A caller who asks for `PeakParams::volcanic()` gets a different world — but they have
  asked for *a different parameter*, because `volcanic()` is a named preset whose value is a
  published default and not part of the generator's identity. That is the same distinction
  `ReliefParams::hills()`, `TectonicParams::ranges()`, `CoastParams::fractal()` and
  `GullyParams::drainage()` already live under; recalibrating a preset has never bumped the
  version and must not, or the version would track editorial defaults rather than the arithmetic.
- **Nothing on the canonical path reads `VOLCANIC_DENSITY` at all.** `PeakParams::canonical()`
  has `density: 0.0` and does not inherit from it; only `volcanic()` does.

So the bump test is not met. `GENERATOR_VERSION` is unmoved, and the parity run is the proof
rather than the assertion.

---

## 9. Every pin, old and new

Re-derived by **running** each one on the host named at the top. Nothing in this table is
transcribed.

| Pin | baseline `88f199e` | after Task 7 | **after both final fix waves** | note |
|---|---|---|---|---|
| Engine lib, `--features wasm` | 843 passed / 11 ignored | 843 / 11 | **847 / 11** | +4 tests, ignored back to 11 — see below |
| Engine lib, `--no-default-features` | — | 843 / 11 | **845 / 11** | |
| Engine lib, default features | — | 843 / 11 | **845 / 11** | |
| Engine lib, `--features python` | — | 845 / 11 | **847 / 11** | +2 as always on the python rows |
| Engine lib, `--features python,wasm` | — | 845 / 11 | **849 / 11** | |
| `tests/blake2_bytes.rs` | 4 | 4 | **4** | every configuration |
| `tests/build_fingerprint.rs` | 9 | 9 | **9** | every configuration |
| `tests/no_std_math.rs` | 7 | 7 | **7** | every configuration |
| `tests/wasm_exports.rs` | 119 | 119 | **120** | +1: the swapped-slot sampling test |
| `tectonics.rs` `.abs()` ledger | 9 | 9 | **9** | exact, by the ledger test; see the note below |
| Parity, compared / divergent | 156,011 / 0 | 156,011 / 0 | **156,011 / 0** | §8 |
| `--mutate seed` | 147,387 | 147,387 | **147,387** | §8 |
| `--mutate erosion-k` | 216 | 216 | **216** | §8 |
| `--mutate water-pond` | 60 | 60 | **60** | §8 |
| `--mutate tectonic-warp` | 22,995 | 22,995 | **22,995** | §8 |
| Python suite, `tests/` | 575 | 575 | **575** | `WORLDBUILDER_REQUIRE_ENGINE=1` |
| Python conformance, `tests/test_conformance.py` | 167 | 167 | **167** | |
| Viewer `npm test` | 359 | 359 | **362** | 362 was already the count at `3f473f4`; this round added none |
| Wasm artifact | 465,699 bytes, 36 exports, 0 imports | 465,699 / 36 / 0 | **465,840 bytes, 36 exports, 0 imports** | moved: `src/` changed |
| Wasm artifact-sha256 | — | `b13e6003…0ee` | **`0362020b1859200f12c9e71af38533c5122f89dbeef97e0bd972bf7fa6d98312`** | moved: `tectonics.rs`, `surface.rs` and `wasm.rs` changed |
| Wasm source-fingerprint | — | `83672441…511` (70 inputs) | **`ce29b131a9131dd860fa63de31aa8441cba168b7d72c434da8c2a3aae52a3c8d` (70 inputs)** | moved with the source |
| `npm run check:wasm` | matches | matches | **matches its manifest and the source that is here now** | |
| EOL guard | clean | clean | **clean** | |

**The four new engine tests.** `wasm_exports.rs` gains
`an_island_the_probes_can_actually_see_moves_the_ground_a_swapped_slot_would_not` (the review's
blocker: the only test in that file that fails if `decode_peak` swaps two same-domain slots).
`src/wasm.rs` gains a `peak_wire_format_tests` module of two — a distinct-sentinel-per-slot
decode assertion and an encode/decode round trip — which are the wasm-gated pair, hence 847 with
the feature and 845 without. `src/tectonics.rs` gains
`no_composed_step_exceeds_the_geometric_and_window_bounds_together` (minor 3) in every
configuration, and `the_seamount_term_is_reachable_everywhere_no_matter_where_the_margins_fall`
(§13), which the second wave un-ignored once the defect it records was fixed — which is why the
ignored count is back to its baseline 11.

**Every one of the four was verified to fail against the defect it pins.** The two wire-format
tests and the sampling test against a temporary slots-0-and-3 swap in `decode_peak` (reverted).
The reachability test and the composed-continuity sweep against the pre-fix `offset_m` shape,
temporarily restored for the check and reverted: 35,862 of 200,000 points suppressed, and a
65.79 m step against an 8.26 m bound at the first frontier crossing the sweep reaches. And the
widened inert-path test against the pre-fix `density`-only gate.

### The `.abs()` ledger, and why the ledger test rather than a grep

`tests/no_std_math.rs:86` ledgers `src/tectonics.rs` at **9**, and
`abs_stays_within_its_legacy_ledger` passes — which is an exact check, not an upper bound: the
test fails on `found < n` too, and tells you to lower the entry. **A naive `grep -o '\.abs(' | wc
-l` on that file reports 10**, verified here, because the tenth hit is prose inside a comment and
`count_abs` skips wholly-commented lines. The ledger test is the instrument; the grep is not.
Task 7's edits to `tectonics.rs` are a constant and doc comments, so the count could not move,
and the test was run to confirm rather than reasoned about.

### Suites and gates that were run

- `cargo test --release -p worldbuilder-engine` in **all five** feature configurations
  (`--no-default-features`, default, `python`, `wasm`, `python,wasm`) — every one green, counts
  above.
- `cargo test --release -p worldbuilder-engine --test no_std_math` — 7/7, ledger exact.
- `maturin develop --release --features python` then
  `WORLDBUILDER_REQUIRE_ENGINE=1 pytest tests/` — 575 passed; `pytest tests/test_conformance.py`
  — 167 passed. **The engine had to be rebuilt to run these at all**: the installed
  `worldbuilder_engine` was stale against the baseline tree and `test_conformance.py`'s freshness
  guard refused collection, which is the guard working. The baseline figures above were taken
  after that rebuild and before any edit.
- `npm run build:wasm` then `npm run check:wasm` in `viewer/` — rebuilt because changing a
  constant in `tectonics.rs` moves the source fingerprint, exactly as expected.
- `git ls-files --eol crates/worldbuilder-engine viewer/public/app | awk '$2!="w/lf"'` — printed
  nothing, with `island_survey.rs` staged so the guard could see it.
- `cargo run --release --bin island_survey` and `… -- components`, twice each; the component pass
  reproduced byte-for-byte on the second run.

---

## 10. What moved outside `src/bin/` and the report, and why

> **THIS SECTION DESCRIBES TASK 7 ONLY, and the two fix waves after it moved more.** Task 7 was
> scoped to write no production logic and this is its inventory. The first fix wave added tests
> and corrected claims; the second **restructured `Tectonics::offset_m`** and re-surveyed the
> density, so this section is no longer an inventory of the branch. §13 and §14 are. The figures
> below are Task 7's and are left as its record; each one that later moved is listed in §14's
> table.

Four files moved besides the survey and this report, and each one is a **measured figure being
re-derived**, not new behaviour.

1. **`src/tectonics.rs`** — `VOLCANIC_DENSITY` 0.11 → **0.36** (since re-surveyed to **0.14**;
   §14), the calibration this task exists to perform. Its doc comment now carries the three-world sweep, and
   `VOLCANIC_HEIGHT_M`/`VOLCANIC_REACH_M`/`VOLCANIC_LATTICE_M` lose their "provisional, pending
   Task 7's survey" labels and gain the measurements that settled them. No code changed.
2. **`src/surface.rs`** — two pinned measurements re-derived, no logic.
   - `an_island_stands_above_the_datum_in_open_ocean`: the count moved **34 → 88 of 20,000**
     (0.17% → 0.44%) and the band **`15..70` → `45..140`**. (The second wave moved it again, to
     **103 of 20,000** and `55..165`; §14.) The doc also records that this 20,000-point count is a
     small sample and that the survey's 200,000-point figure is the one to quote.
   - `an_island_is_steep_to_rather_than_shelved`: the doc figure moved **−2,594.07 m → −2,907.71
     m**, off a summit this task measured at **4,010.75 m** (the old summit's own height was not
     recorded, so no before/after is claimed for it). (The second wave moved it again, to
     **−2,695.04 m** off **4,291.15 m**; §14.) **The density moved which node
     `find_a_summit` picks, not how steep a flank is** — `peak_of_cell` gates on
     `hash >= density`, so a higher density strictly *adds* candidate cells and can never remove
     one. The assertion itself (deeper than −1,000 m) is untouched.
3. **`viewer/test/peak-params.test.mjs`** — the transcribed literal `"0.11"` → `"0.36"` in "no
   peak number is written down twice", plus a new assertion that the peak density stays distinct
   from the coast channel's 0.35 and a note on why. **The first fix wave then retired that
   transcription entirely**: the literal is now read live from `VOLCANIC_DENSITY`'s own
   declaration in `tectonics.rs`, so the 0.36 → 0.14 move needed no edit to it at all — which is
   the whole point of reading it rather than writing it down. The file's header records that its
   eight witness probes were *derived* at density 0.11 and why monotonicity keeps them valid at
   every density this preset has shipped; the "991 of 64,800" figure is left labelled as the
   probe set's provenance and **not** restated, because nothing re-ran that scan.
4. **`viewer/public/app/peak-params.js`** and **`viewer/public/app/engine.js`** — prose only. Two
   comments listing the preset's numbers said `0.11`. One of them argued that the slider divides
   rather than multiplies because `0.01 * 11` and `11 / 100` "happen to equal" 0.11; I first
   rewrote that to claim the two forms *disagree* at 0.36, then checked it in Node and found
   `0.01 * 36 === 36 / 100` is **true**. The comment now says they agree, that this was checked
   rather than assumed, and that division remains the house form for a reason that outlives any
   one preset's value. Recording the near-miss because it is precisely the failure mode this
   plan's constraint is written against — a plausible arithmetic claim, in prose, that nobody
   ran.

---

## 11. Concerns and what is left open

1. **~~The band is tight~~ — LARGELY RETIRED BY §14, and the corpus is still three worlds.** As
   written, for Task 7's 0.36: the admissible window was seven hundredths (0.32–0.38, six of
   span) because `island-a` pressed the 0.3% floor while `owner` pressed the 0.8% ceiling, and
   0.36's margin was +0.0345 pp, a little over 2σ of the estimator. **After §13's fix and §14's
   re-survey the window is nine hundredths (0.09–0.17) and 0.14's margin is +0.1480 pp, close to
   10σ, with the binding edge crossing inside the window rather than sitting on one end.** What
   survives is the corpus: **a fourth world with a land fraction outside 0.16–0.40 could still
   push an end out of band**, and the
   honest fix then is not to re-tune `density` but to make the preset's density depend on
   `land_fraction` — which is a design change, not a constant. Flagging it as the first thing to
   re-measure when a new reference world appears.
2. **Spec §7 question 1 asked for "about 0.5%" as its example and 0.3–0.8% as the band.** At the
   re-surveyed 0.14 the three worlds read **0.45 / 0.64 / 0.52%** — so `earth-a` now sits almost
   exactly on the 0.5% example and the other two straddle it, which is a better answer than the
   0.33 / 0.75 / 0.40 the suppressed field gave at 0.36. The *centre* still is not hit on every
   world simultaneously and cannot be while the share scales with ocean coverage. A question for
   the owner, not a defect — and a smaller question than it was.
3. **Roughly a fourteenth of the islands are at the raster's resolution floor**, so **6,137** is
   a lower bound on the count and the resolution-safe figure is the **4,197** above 200 km². A
   finer raster would raise the count and leave the area unchanged. Nobody should quote 6,137 as
   exact.
4. **Spec §5's accessor is not built** — a decision, argued in §7, landing with fragments in slice
   3. Recorded here so it is on the record rather than missing.
5. **Hydrology on islands is untested.** Spec §5 predicts that islands below the node spacing get
   no rivers; §4's distribution says the great majority are below it at 1,000,000 nodes. Nothing
   in this slice bakes hydrology on a peaked world, so that prediction is stated and unverified.
6. **An island casts a very large orographic rain shadow, and it reaches pre-existing land.**
   Measured in §12: the moisture index changes at **45.40%** of a 1-degree global grid, at
   **22.09%** of the land that existed without the block, and by as much as **0.425** there.
   Nothing in this slice is wrong because of it and nothing in `climate.rs` was changed — but a
   later slice that pins biomes must pin them on a world with this block on, and the coarse 20 km
   march step is the first thing to look at when that slice arrives. **This is now the first
   open item on this branch.**
7. **The seamount term was unreachable on 77.16% of the planet. Fixed, and the calibration
   re-run.** §13 records the defect and §14 the re-survey: `VOLCANIC_DENSITY` moved 0.36 → 0.14
   and the maximin margin went from a little over 2σ to close to 10σ, which retires concern 1
   above rather than adding to it. Kept in this list because the *shape* of the miss is worth
   remembering: a term added "last, as an addition to a finished offset" was added after an early
   return, and no test on the branch could see it until continuity was measured on the composed
   function rather than on the term alone.

---

## 12. The orographic rain shadow an island casts — measured, added by the final fix wave

**Minor 4 of the final whole-branch review: `climate.rs`'s orographic march is the one downstream
consumer of `elevation_m` that this branch neither measured nor named**, while §6 measured the
three the spec happened to list. The arithmetic the review did by hand is right, and it is larger
than "possibly negligible". **Nothing in `climate.rs` was changed** — this section is the
measurement and the disclosure the review asked for.

**Population:** every point of a 1-degree global grid over latitudes −80…80 — 57,960 sites — plus
nine downwind stations off each of the three derived island probes.
**Method:** `Surface::moisture_index(point, None, None)` (so `MoistureParams::canonical()`:
`MARCH_STEP_M` 20,000 m, `LIFT_SCALE_M` 1,000 m) on two worlds built identically but for the
block — `Surface::new(20260904, 6_371_000, 12, 0.29, …)` against
`Surface::with_peaks(…, Some(PeakParams::volcanic()))`. Downwind is the negation of
`climate::upwind_east(latitude)`.
**Host:** the one named at the top of this report, `rustc 1.98.0`, `--release`.

### What one island does to the air behind it

**Re-measured after §13's fix and the §14 re-survey.** The figures below are the current ones;
the pre-fix pair is given after each for comparison, because the direction of the change is the
interesting part.

| downwind of the island at 13.5, −91.5 | plain | with the block | (pre-fix, `density: 0.36`) |
|---|---|---|---|
| 0 km (the island itself, +4,327 m of ground) | 1.000000 | **0.013124** | 0.004187 |
| 20 km | 1.000000 | **0.076775** | 0.068402 |
| 60 km | 1.000000 | **0.192052** | 0.184676 |
| 100 km | 1.000000 | **0.044070** | 0.042808 |
| 200 km | 1.000000 | **0.317150** | 0.316119 |
| 300 km | 1.000000 | **0.515459** | 0.514397 |
| 600 km | 1.000000 | **0.908301** | 0.141050 |
| 1,000 km | 1.000000 | **0.993705** | 0.976111 |
| 2,000 km | 0.967729 | **0.926968** | 0.958795 |

The other two derived probes behave the same way: at the island itself the index falls to 0.046233
(−43.75, 46.0) and 0.022853 (−55.0, −52.5), and the air is back within 1% of its plain value
somewhere between 300 and 1,000 km downwind. **The march's 20 km step is what makes the shadow
this deep in one sample** — a 4.3 km island crossed in a single step multiplies moisture by
`exp(−4.3)`, and the recovery term's 300 km scale needs hundreds of kilometres to undo it. The
non-monotone column (0.19 at 60 km, 0.044 at 100 km) is the march crossing *different* islands at
different offsets, not noise.

### How much of the planet notices

| population | sites | changed | share | (pre-fix) |
|---|---|---|---|---|
| all 1-degree sites, −80…80 | 57,960 | **26,314** | **45.40%** | 15,740 / 27.16% |
| …of those, losing more than 0.10 of index | 57,960 | **5,430** | 9.37% | 3,043 / 5.25% |
| sites that are already land on the plain world | 18,637 | **4,117** | **22.09%** | 2,944 / 15.80% |

Worst drop anywhere: **0.986832**, at (−32, 21) — air that was saturated arriving parched. Worst
drop on *pre-existing* land: **0.425249**. Mean signed drop over the 26,314 changed sites: 0.0761.

**The shadow reaches more of the planet now at 40% of the density, which is §13's fix showing up
in a second place.** 45.40% against 27.16% of sites, and 22.09% against 15.80% of pre-existing
land, at `density: 0.14` rather than 0.36: the islands are fewer but they are no longer confined
to the quarter of the world near a plate margin, so far more of the ocean sits downwind of one.
The previous section said these were lower bounds; they were, by about 1.7x.

### The judgement, stated rather than implied

**This is a known consequence, and it is bigger than a curiosity: it is the first thing to
re-measure in the slice that touches biomes.** Three things are true at once and all three belong
on the record.

1. **It is physically the right sign and roughly the right magnitude.** Real oceanic islands do
   cast rain shadows, and a 4 km volcano wringing out the air crossing it is not an artefact.
2. **The 20 km march step makes it coarser than the geography deserves.** An island 25 km across
   is one or two samples of the march, so the shadow it casts is quantised to the march's own
   grid — which is why the downwind column is not monotone. A finer step, or a lift term that
   integrated over the step rather than differencing its ends, would spread the same total
   rain-out over a plausible distance instead of dumping it in one sample.
3. **It reaches ground that already existed.** 15.80% of the plain world's land sites see a
   different moisture index, up to 0.427 lower. Moisture is quantiled into bands, so a shift that
   size can move a biome. **No biome output in this slice is pinned against a peaked world**, so
   nothing here regressed — but a later slice that pins biomes must build those pins on a world
   with this block on, or it will pin them against a planet the studio can no longer make.

One note on reading the two columns together. The pre-fix figures were measured through wiring
that never reached the seamount term on 77% of the planet (§13). The prediction made at the time
was that fixing it would make the shadow *larger*, roughly in proportion to the suppressed
fraction; the re-measurement above confirms that, and the density coming down by more than half
did not offset it.

---

## 13. A defect the minor-3 measurement found: the seamount term was unreachable on most of the planet

**Found by the final fix wave while building the composed-continuity measurement minor 3 asked
for. It is not one of the review's nine findings, it was larger than the blocker was, and it is
now FIXED — the second fix wave lifted the term out of the margin sum and re-ran the whole
calibration. This section records what was wrong and what it cost; §14 records the re-survey.**

`Tectonics::offset_m` returns `0.0` before it reaches the seamount term whenever
`PlateSet::margins_within` comes back empty or `nearest` is `None` (`src/tectonics.rs:1213-1220`).
That early return predates this branch — its own comment calls a plate interior "69 per cent of
the planet" — and Task 2 added the seamount term at the **end** of the function, after it.

**Population:** a 200,000-point area-uniform Fibonacci spiral, plus 600 transects × 2 bearings ×
3,000 steps of 20 m. **Method:** `Tectonics::offset_m` against `Tectonics::peak_offset_m` on
`plates_for(20_260_904, 12)` / `Continentality::new(20_260_904, 6_371_000, 0.29)` at
`PeakParams::volcanic()` — the same world `tests/wasm_exports.rs` uses. **Host:** as above.

- **77.16%** of the planet (154,314 of 200,000 points) has an empty margin set, so `offset_m`
  never evaluates the term there.
- At those suppressed points the field *would* stand up to **7,824.3 m**, and **28,942** of the
  200,000 (14.5%) suppress more than 100 m.
- Worst single-step jump in `offset_m` at a margin-range frontier: **3,460.23 m in one 20 m
  step** (lat −25.81, bearing 45°, step 560) — **454×** the 7.62 m analytic bound, and larger
  than the 1,466 m cliff whose discovery is why `tectonics.rs` has a continuity test at all.
- With the term live on both sides of a step, the worst step is **6.13 m**, inside the bound.
  **The field is continuous; the wiring is not.** On the three-plate test fixture the same
  measurement reads 90.24% suppressed and a 2,749.07 m worst cliff.

### The fix, and how the plate part was held still

`Tectonics::offset_m` now wraps a private `Tectonics::margin_offset_m` instead of ending it. The
margin summation moved **verbatim** — both early returns and the load-bearing iteration order
untouched — and the gated seamount term is applied outside it, to whatever that sum returned,
including nothing.

**The plate part is bit-identical, verified rather than argued.** 60,000 `offset_m` values on the
no-peak path (a 20,000-point area-uniform spiral on each of the three survey fixtures) were dumped
as raw `f64` bits before and after the change and compared with `cmp`: **no difference**. That is
the direct check; three indirect ones agree — the parity corpus reports 156,011 compared and 0
divergent with all four controls exactly unmoved, `WITNESSED_ELEVATION_M`
(682.3921701573904, pinned three ways at extraction) still holds, and the Python conformance
suite's 167 recorded-reference comparisons pass.

**The pin is now live, and it was written red against the old shape.**
`the_seamount_term_is_reachable_everywhere_no_matter_where_the_margins_fall` (`src/tectonics.rs`)
replaced the `#[ignore]`d placeholder. Against the old shape it reported **35,862 of 200,000
points where the field wanted a seamount and `offset_m` did not carry it**, suppressing up to
7,824.32 m, and the 3,460.23 m frontier step. Both are zero now. The test also asserts that its
own fixture is mostly plate interior and that the field does want seamounts there, so it cannot
pass on a world that simply has no interiors to get wrong.

The composed continuity sweep no longer exempts anything either. It used to skip steps that
crossed the frontier; there is no frontier, so it now bounds **1,439,520 steps per arm, none
skipped** — worst composed step **7.0073 m** against a worst derived bound of **12.3159 m**, with
77,016 steps strictly inside the depth window's ramp.

`GENERATOR_VERSION` is untouched: with no peak block the canonical path returns the same `0.0`,
which the bit comparison above is the proof of.

### The bit dump is weak on its own, and nobody should reuse it as a general proof

The re-review's critique of the method is fair and worth writing down, because the dump reads
like a stronger instrument than it is.

**Most of what it compares is a constant.** `offset_m` returns exactly `0.0` at **85.99% /
81.57% / 67.63%** of the 20,000 points on the three dumped fixtures (the ABI world, `island-a`,
`owner`) — measured on the same predicate, at 200,000 points. So around four fifths of the 60,000
compared values are the early return's literal zero on both sides, and they would agree under
almost any edit to the loop.

**The subpopulation that actually tests the thing at risk is small and was not counted at the
time.** The iteration order matters only where two or more margins are summed, since
floating-point addition is non-associative. That is **1.20% / 2.45% / 6.18%** of points on those
three fixtures — so roughly 2,000 of the 60,000 dumped values, present but thin, and no
breakdown was reported with the original dump. Points exactly on a margin, on the antimeridian
and at the poles are measure-zero and unsampled by a Fibonacci spiral at all.

**Why it is nonetheless enough here, and only here.** The argument is *structural*, not
statistical: the loop body, its two early returns and its accumulation order were moved into
`margin_offset_m` verbatim, and `git diff` shows no changed line inside the loop. The dump is a
check that the move was actually verbatim — a guard against a typo in a mechanical edit — not
evidence that a *rewritten* summation would agree. **A future change that reorders, sorts,
parallelises or re-associates the margin sum must not lean on this method.** For that, the
population to build is the two-or-more-margin one, sampled deliberately and reported with its own
count.

---

## 14. The re-survey: the calibration, run again against a field that reaches the whole planet

**§13's defect means the first calibration measured the wrong thing.** Every share in §2 was a
share of the roughly quarter of the planet where `Tectonics::offset_m` actually evaluated the
seamount term. With the term lifted out of the margin sum, the field stands islands anywhere the
seabed allows, and `density` had to come down.

**Population, method and host are §2's, unchanged:** the 200,000-point Fibonacci spiral per world;
`island_survey.rs` sections 2 and 4, `D_added` measured through `Surface::structural_m` over ocean
on two worlds built identically but for the block; this host, `rustc 1.98.0`, `--release`. Only
the code under measurement changed. `island_survey.rs`'s `DENSITIES` and `CANDIDATES` were
re-pointed at the new range and the binary re-run; nothing was scaled.

### The correction on each world is that world's plate-interior area factor

**Population:** the same 200,000-point area-uniform Fibonacci spiral, per world. **Method:** a
point is plate interior when `PlateSet::margins_within(point, MAX_TECTONIC_RANGE_M, radius)`
returns an empty margin set or no nearest plate — the exact predicate `margin_offset_m`'s early
returns use. The area factor is `1 / (1 − interior)`: the reciprocal of the fraction of the
planet where the seamount term used to be evaluated at all. **Host:** as above. Re-derived here,
not taken from the re-review.

| world | plates | plate interior | area factor | share before the fix | share after, same 0.36 | **observed correction** |
|---|---|---|---|---|---|---|
| `island-a` | 22 | 139,388 / 200,000 = 69.6940% | **3.2997×** | 0.3345% | 1.1400% | **3.4081×** |
| `owner` | 28 | 106,539 / 200,000 = 53.2695% | **2.1399×** | 0.7525% | 1.6180% | **2.1502×** |
| `earth-a` | 22 | 139,435 / 200,000 = 69.7175% | **3.3022×** | 0.4035% | 1.3945% | **3.4560×** |

**Three worlds, three matches — 3.3% / 0.5% / 4.7% apart. The correction needed no further
mechanism than the area, and there is no shortfall for bathymetry to account for.** The small
residual even has the opposite sign from a shortfall: the observed correction slightly *exceeds*
the area factor on all three, because the field's peak-wanting rate is marginally **higher** in
the interior than near a margin — `island-a` 0.082439 against 0.082079, `owner` 0.134101 against
0.128321, `earth-a` 0.103303 against 0.097994, over the same spiral and predicate. Interior
seabed is very slightly the *better* place to stand an island here.

> **What the previous version of this section got wrong, recorded rather than quietly replaced.**
> It said the correction was "3.41×, not the 4.4× the area suggests", and explained the gap by
> claiming margin-adjacent seabed is shallower than abyssal plain. Three things were wrong with
> that. **The 4.4× was a different fixture's**: it is `1/(1−0.771570)` for the **12-plate** ABI
> world `plates_for(20_260_904, 12)` that §13's suppression was measured on, while the 3.41× was
> measured on the **22-plate** `island-a`. Fewer plates means fewer margins means more interior,
> so the two have genuinely different factors and comparing one against the other's share is a
> category error. **The mechanism was backwards**: a below-average margin belt would make the
> correction *larger* than the area factor, not smaller. And **the claim was untested** — the
> peak-wanting rates above say the interior is slightly better, not worse. The number never
> needed a story; it is the area factor, and now it is measured as one.

### The one-world sweep, re-run

§2's sixteen-row table is superseded by this one. `island_survey.rs`'s `DENSITIES` was
re-pointed at the range the corrected field needs — the old array started at 0.11, which is now
mid-band — and re-run on `island-a`. Both columns are the same two estimators §2 used.

| density | n = 20,000 | **n = 200,000** | band |
|---|---|---|---|
| 0.02 | 0.0600% | 0.0765% | BELOW |
| 0.04 | 0.1700% | 0.1395% | BELOW |
| 0.06 | 0.2450% | 0.2080% | BELOW |
| 0.08 | 0.3000% | 0.2665% | BELOW |
| 0.09 | 0.3400% | 0.3030% | in band |
| 0.10 | 0.3950% | 0.3350% | in band |
| 0.11 *(the pre-Task-7 value)* | 0.4250% | 0.3650% | in band |
| 0.12 | 0.4700% | 0.3935% | in band |
| **0.14 (shipped)** | 0.5150% | **0.4480%** | in band |
| 0.16 | 0.5650% | 0.5050% | in band |
| 0.20 | 0.6750% | 0.6355% | in band |
| 0.24 | 0.7950% | 0.7615% | in band |
| 0.32 | 1.1200% | 1.0290% | ABOVE |
| 0.36 *(the suppressed-field pick)* | 1.2650% | 1.1400% | ABOVE |
| 0.50 | 1.7200% | 1.5785% | ABOVE |
| 0.75 | 2.4350% | 2.3555% | ABOVE |

**§2's 2⁻¹⁶ observation survives the re-run and is now stronger.** The n = 20,000 column runs
high at **15 of these 16 rows** — the exception is 0.02, where the smaller sample reads 0.0600%
against 0.0765%, and at 12 island points in 20,000 that column is counting single digits. So the
lattice bias §2 identified is a property of that 20,000-point lattice and not of the field, on a
second, independent population. **The figure to quote is still the 200,000-point one.**

### The three-world sweep, re-run

A density is admissible only if its **worst** world is inside the spec's 0.3%–0.8% band.

| density | `island-a` (land 0.40) | `owner` (land 0.16) | `earth-a` (land 0.29) | margin to the nearer band edge |
|---|---|---|---|---|
| 0.08 | 0.2665% | 0.3640% | 0.3090% | **−0.0335 pp** — `island-a` BELOW |
| 0.09 | 0.3030% | 0.4105% | 0.3430% | +0.0030 pp |
| 0.10 | 0.3350% | 0.4590% | 0.3810% | +0.0350 pp |
| 0.11 | 0.3650% | 0.5000% | 0.4140% | +0.0650 pp |
| 0.12 | 0.3935% | 0.5465% | 0.4495% | +0.0935 pp |
| 0.13 | 0.4180% | 0.5915% | 0.4785% | +0.1180 pp |
| **0.14** | **0.4480%** | **0.6380%** | **0.5210%** | **+0.1480 pp — the maximin** |
| 0.15 | 0.4795% | 0.6755% | 0.5575% | +0.1245 pp |
| 0.16 | 0.5050% | 0.7175% | 0.5940% | +0.0825 pp |
| 0.17 | 0.5355% | 0.7625% | 0.6380% | +0.0375 pp |
| 0.18 | 0.5640% | 0.8055% | 0.6810% | **−0.0055 pp** — `owner` ABOVE |
| 0.19 | 0.6045% | 0.8540% | 0.7235% | −0.0540 pp |
| 0.20 | 0.6355% | 0.8990% | 0.7615% | −0.0990 pp |
| 0.36 *(the suppressed-field pick)* | 1.1400% | 1.6180% | 1.3945% | −0.8180 pp |

**`VOLCANIC_DENSITY` = 0.14**, and the three worlds land at **0.4480% / 0.6380% / 0.5210%** — all
three in band, `island-a` clearing the floor by 0.1480 pp and `owner` clearing the ceiling by
0.1620 pp. The admissible window is **nine hundredths, 0.09 through 0.17, eight hundredths of
span**, with both edges measured rather than inferred: 0.08 misses the floor by 0.0335 pp and 0.18
clears the ceiling by 0.0055 pp.

### The fix widened the safety margin, which is the part worth keeping

| | at 0.36, suppressed field | **at 0.14, whole planet** |
|---|---|---|
| maximin margin to a band edge | +0.0345 pp | **+0.1480 pp** |
| 1σ of the estimator at that share | ±0.0158 pp | ±0.0150 pp |
| margin in σ | **2.2σ** | **≈10σ** |
| admissible hundredths | 7 (0.32–0.38) | **9 (0.09–0.17)** |
| which world binds | the floor, on `island-a`, at every admissible density | the floor below 0.15, the ceiling above it — the two cross *inside* the window |

§11's first concern was that the band was tight and 0.36's margin was "a little over 2σ", with
only a `land_fraction`-dependent density able to buy more room. **That concern is largely
retired**: the margin is now near ten sigma, and the binding edge crosses inside the window rather
than sitting on one end of it, so the maximin is a genuine interior optimum rather than the least
bad corner. A fourth world is no longer likely to push an end out of band.

### Also that 0.14 is a legal slider position, and distinct

Hundredths only, for the reason `VOLCANIC_DENSITY`'s doc gives: the panel's density slider carries
an integer position and divides by 100, so a density off that lattice is one the panel cannot
reach. **0.14 is position 14.** It is not `CoastParams::fractal()`'s 0.35, so the
anti-transcription scan still asks a live question rather than passing on another channel's
guard.

**And the scan's blind spot was closed rather than described around.** That scan reads
`peak-params.js`, `controls.js`, `main.js` and `engine.js` **with whole-line comments stripped**,
which is why a retired `11%` (§10.4's minor) and a hand-maintained `0.36` both survived in prose
across two calibrations — and why the density in three of those comments had to be hand-edited
from 0.36 to 0.14 in the second wave, a transcription with extra steps. The third wave removed
the preset's values from the prose of all four modules instead, so **`0.14` appears in none of
them with or without the strip**, checked by grep. The strip is still the right rule — a comment
cannot move a pinned value — but nothing in `viewer/` now needs editing when this constant
moves.

### Every figure that moved, and where

| figure | was | **is** | where |
|---|---|---|---|
| `VOLCANIC_DENSITY` | 0.36 | **0.14** | `tectonics.rs` |
| admissible window | 0.32–0.38 (7 values) | **0.09–0.17 (9 values)** | §14, `VOLCANIC_DENSITY` doc |
| maximin margin | +0.0345 pp (2.2σ) | **+0.1480 pp (≈10σ)** | §14 |
| islanded share, three worlds | 0.3345 / 0.7525 / 0.4035% | **0.4480 / 0.6380 / 0.5210%** | §§4b, 7, 14 |
| one-world density sweep, 16 rows | §2's table, 0.11–0.75 | **§14's table, 0.02–0.75** | §2 banner, §14 |
| ratio-invariance spread | 0.006 pp at 0.36 | **0.0175 pp at 0.14** (still ≈1σ) | §3 (re-run in place), `VOLCANIC_REACH_M` doc |
| distinct islands at 45 km | 4,617 | **6,137** | §4, `VOLCANIC_LATTICE_M` doc |
| island area share (raster) | 0.3337% | **0.4375%** | §4 |
| island count at 30 / 90 km | 10,155 / 1,213 | **13,506 / 1,558** | §4, `VOLCANIC_REACH_M` doc |
| largest island | 2,632.3 km² | **1,732.5 km²** | §4 |
| islands over 2,000 km² | 10 | **0** | §4 |
| resolution-safe count (>200 km²) | 3,094 | **4,197** | §4 |
| survey summit | 4,773.47 m | **4,291.15 m** | §§5, 6 |
| depth at 0.95 × `reach_m` | −2,738.70 m | **−2,695.04 m** | §5 |
| summit `tectonic_m` | 7,484.31 m | **6,962.95 m** | §6 |
| contrast-point `tectonic_m` | 0.00 m | **16.94 m** | §6 |
| achieved land, `volcanic()` | 0.4054 / 0.1705 / 0.2969 | **0.4065 / 0.1694 / 0.2980** | §7 |
| test pin: offshore points above datum | 88 of 20,000 | **103 of 20,000** | `surface.rs` |
| ABI probe 3 | −45.5, −147.25 | **−55.0, −52.5** | `tests/wasm_exports.rs` |
| moisture sites changed | 27.16% | **45.40%** | §12 |
| moisture: pre-existing land changed | 15.80% | **22.09%** | §12 |
| suppressed share of the planet | 77.16% | **0** | §13 |
| worst frontier step | 3,460.23 m | **none — no frontier** | §13 |

**What did NOT move, and was checked rather than assumed:** the plate-only tectonic offset
(60,000 values, bit for bit — §13); the canonical-preset land fractions in §7; the continental
shelf column in §5; `GENERATOR_VERSION`; the parity corpus and all four of its controls;
`VOLCANIC_HEIGHT_M`, `VOLCANIC_REACH_M`, `VOLCANIC_MIN_DEPTH_M` and `VOLCANIC_LATTICE_M`, all four
of which the re-run sweep left where they were for the same reasons §3 gives.
