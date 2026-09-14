# Islands slice 1 (peaks) — calibration and verification

Slice 1 of the islands plan grew a cellular seamount field — `PeakParams` and
`Tectonics::peak_offset_m`, a lattice of jittered candidate nodes that stand islands (or
submerged shoals) out of deep ocean — wired it through `Tectonics` and `Surface`, exposed and
pinned a wasm ABI for it, and gave the studio a panel. Task 7 calibrates it and reports.

**This slice's central claim is a negative one: with the block absent, nothing moved.** The
parity corpus reports **156,011 compared / 0 divergent** and all four controls are unmoved at
**147,387 / 216 / 60 / 22,995**. `GENERATOR_VERSION` is therefore **not** bumped, and the
reasoning is stated in full below rather than assumed.

**The one thing Task 7 changed in `src/` is a single constant.** `VOLCANIC_DENSITY` moved
**0.11 → 0.36**, because 0.11 put every world measured *below* the spec's band. Nothing else in
the field's geometry moved; `height_m`, `reach_m`, `min_depth_m` and `lattice_m` were swept and
left where Tasks 1 and 2 put them.

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
| 0.58 | 0.6700% | 0.5495% | in band |
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
sign is the same on all sixteen rows**, which happens by chance with probability 2⁻¹⁶. So this
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
| 0.32 | 0.3065% | 0.6655% | 0.3660% | +0.0065 pp |
| 0.33 | 0.3165% | 0.6875% | 0.3730% | +0.0165 pp |
| 0.34 | 0.3215% | 0.7065% | 0.3850% | +0.0215 pp |
| 0.35 | 0.3295% | 0.7310% | 0.3890% | +0.0295 pp |
| **0.36** | **0.3345%** | **0.7525%** | **0.4035%** | **+0.0345 pp — the maximin** |
| 0.37 | 0.3470% | 0.7710% | 0.4170% | +0.0290 pp |
| 0.38 | 0.3585% | 0.7905% | 0.4325% | +0.0095 pp |
| 0.40 | 0.3785% | 0.8280% | 0.4545% | **−0.0280 pp** — `owner` ABOVE |
| 0.45 | 0.4150% | 0.9390% | 0.5165% | −0.1390 pp |
| 0.58 | 0.5495% | 1.2135% | 0.6730% | −0.4135 pp |

**A world with less land has more deep ocean for the field to stand an island in, and the effect
is large: at every density the owner's 0.16-land world yields roughly 2.2× the share the
0.40-land fixture does.** So the band is squeezed from *both* sides at once — `island-a` presses
the 0.3% floor while `owner` presses the 0.8% ceiling — and the admissible window is only seven
hundredths wide (0.32 through 0.38). **0.36 is the maximin**: the admissible density whose worst
world sits furthest from a band edge. 0.32 clears the floor by 0.0065 pp, which is half the
estimator's own 1σ error and would not survive a fourth world.

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

| `lattice_m` / `reach_m` | density 0.11 | **density 0.36** | density 0.58 |
|---|---|---|---|
| 30,000 / 21,000 | 0.0970% | **0.3290%** | 0.5460% |
| 45,000 / 31,500 | 0.1070% | **0.3345%** | 0.5495% |
| 67,500 / 47,250 | 0.1065% | **0.3350%** | 0.5420% |
| 90,000 / 63,000 | 0.1080% | **0.3310%** | 0.5200% |

**The islanded share is invariant under the pair.** At the shipped density the spread across an
eightfold change of lattice volume is **0.006 pp**, against the estimator's own 1σ error of
0.013 pp — i.e. unmeasurable. That is what the model predicts (`d(share)` scales with `reach_m`,
so `d³ / lattice_m³` is scale-free) and it is now measured rather than supposed. **So the ratio
is the lever and neither field alone is one**, which is why calibration moved `density` and left
both of these where they were.

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

| configuration | island cells | **distinct islands** | total island area | share of sphere | mean |
|---|---|---|---|---|---|
| **density 0.36, lattice 45,000 (shipped)** | 106,475 | **4,617** | 1,702,268 km² | **0.3337%** | 368.7 km² |
| density 0.11, lattice 45,000 (before) | 32,155 | 1,443 | 524,366 km² | 0.1028% | 363.4 km² |
| density 0.36, lattice 30,000 | 104,689 | **10,155** | 1,694,766 km² | 0.3323% | 166.9 km² |
| density 0.36, lattice 90,000 | 103,380 | **1,213** | 1,643,372 km² | 0.3222% | 1,354.8 km² |

### Area distribution at the shipped constants

| | largest | p90 | median | p10 | smallest |
|---|---|---|---|---|---|
| **density 0.36, lattice 45,000** | **2,632.3 km²** | 714.6 km² | **323.2 km²** | 67.6 km² | 8.2 km² |
| density 0.11, lattice 45,000 | 1,590.5 km² | 727.8 km² | 323.4 km² | 69.9 km² | 10.8 km² |
| density 0.36, lattice 30,000 | 1,264.0 km² | 319.2 km² | 148.8 km² | 35.3 km² | 3.6 km² |
| density 0.36, lattice 90,000 | 7,565.8 km² | 2,684.7 km² | 1,137.5 km² | 195.2 km² | 15.0 km² |

Histogram at the shipped constants, by area: **< 50 km² 387 · 50–200 km² 1,136 · 200–500 km²
1,836 · 500–2,000 km² 1,248 · 2,000–10,000 km² 10 · ≥ 10,000 km² 0.** No island on this world
exceeds 10,000 km², and only ten exceed 2,000 km² — so nothing the field makes is
continent-sized, which is what "islands, not fragments" is supposed to mean at this slice.

Three findings worth separating out.

1. **Two independent estimators agree on the share.** The 200,000-point spiral says 0.3345% and
   the 32-million-point raster says 0.3337% — 0.0008 pp apart, well inside either one's error.
   The spiral figure is the calibration's; the raster corroborates it on a population three
   orders of magnitude larger and by a different construction.
2. **Density moves count, not size. The lattice moves size, not area.** Raising density 0.11 →
   0.36 tripled the count (1,443 → 4,617) and left the mean essentially untouched (363.4 → 368.7
   km²) and the median unmoved to three figures (323.4 → 323.2 km²). Moving the lattice 30 km →
   90 km changed the count eightfold *down* (10,155 → 1,213) and the mean eightfold *up* (166.9 →
   1,354.8 km²) while total area stayed inside 0.32–0.33%. **This is §3's invariance seen from the
   other side**, and it is the measured reason the calibration knob is `density`.
3. **Roughly a tenth of islands are at or below the raster's own resolution floor.** 387 of 4,617
   are under 50 km² — two cells of 25 km² each — so the smallest bin is resolution-limited and
   the true count of very small islands is higher than 387. The count of islands above 200 km²
   (3,094) is the resolution-safe figure. Spec §5's note that "islands smaller than the node
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

The summit stands **4,773.47 m** with a tectonic offset of **7,484.31 m**.

| distance off the island | deepest of 8 bearings | shallowest of 8 |
|---|---|---|
| 0.20 × `reach_m` = 6,300 m | +3,197.93 m | +4,944.38 m |
| 0.40 × `reach_m` = 12,600 m | +948.60 m | +3,597.61 m |
| 0.60 × `reach_m` = 18,900 m | **−1,223.04 m** | +1,422.25 m |
| 0.80 × `reach_m` = 25,200 m | **−2,450.70 m** | −830.23 m |
| **0.95 × `reach_m` = 29,925 m** | **−2,738.70 m** | −926.01 m |
| 1.10 × `reach_m` = 34,650 m | −2,762.89 m | −494.87 m |

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

**At 30 km off an island the water is 2,739 m deep; at 80 km off a continent it is 153 m deep.**
That is a factor of eighteen at a quarter of the distance, and it is the navigational difference
between an oceanic volcano and a continental margin — the thing a bundle's soundings would show.
The mechanism is why peaks go into the tectonic offset at all: `Shelf::weight`'s authority is
`1 - smooth(|tectonic_m| / 250)`, so a 7,484 m offset holds the shelf off entirely (the shelf
weight at this summit reads **0.0000**, measured, in §6 below).

**Two summit figures are in circulation and they are different populations, not a
disagreement.** `an_island_is_steep_to_rather_than_shelved` reports **−2,907.71 m** off a summit
standing **4,010.75 m**, because its `find_a_summit` searches 40,000 points with the
`offset_m > 2000` discriminator; the survey searches 200,000 with `D_added`. They find different
nodes on the same planet. Both are re-derived in this task by running them, and both are past the
1,000 m bar by a wide margin.

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

| | at the summit | 1.1 × `reach_m` off it |
|---|---|---|
| `elevation_m` | **+4,773.47 m** | −2,753.28 m |
| `tectonic_m` | **+7,484.31 m** | **0.00 m** |
| shelf `weight` | **0.0000** | 0.0000 |
| slope | 0.15752 | 0.00239 |
| `natural` → sand / mud / **rock** | 0.000000 / 0.000000 / **1.000000** | 0.000000 / 0.989720 / **0.010280** |
| `by_tectonics` = `smooth(\|tectonic_m\| / ROCK_TECTONIC_M)` | `smooth(7484.31/1200)` = `smooth(6.2369)` = **1.000000** | `smooth(0.0000)` = **0.000000** |
| `by_slope` = `smooth(slope / ROCK_SLOPE)` | `smooth(0.15752/0.04)` = **1.000000** | `smooth(0.00239/0.04)` = **0.010280** |
| `Detail::amplitude_m` | **45.000000 m** | 55.682384 m |
| the same with `tectonic_m = 0` | **150.000000 m** | 55.682384 m |
| quieting = `1 − quieting_strength · smooth(\|tectonic_m\| / quieting_scale_m)` | `1 − 0.70 · smooth(6.2369)` = **0.300000** | `1 − 0.70 · smooth(0)` = **1.000000** |

**Both spec claims hold, and both are saturated with room to spare rather than marginally.**

- **`by_tectonics` saturates.** `ROCK_TECTONIC_M` is 1,200 m and the argument is 6.24 — `smooth`
  clamps at 1.0 above an argument of 1, so the summit is at **6.2× the saturation threshold.**
  Rock is exactly 1.000000 and both loose fractions are exactly zero.
- **The quieting saturates too, and it costs 105 m of roughness.** `quieting_strength` is 0.70
  and `quieting_scale_m` is 1,200 m, so a saturated quieting multiplies roughness by exactly
  0.300000. Measured: **45.000000 m against the 150.000000 m the same point would get at
  `tectonic_m = 0`** — a 70% reduction, exactly the strength constant, because the quieting is at
  its floor. An island's flanks are therefore *smoother* than the seabed a kilometre away
  (55.68 m), which is the intended behaviour: deliberate deep structure keeps its shape.

**One honest caveat, which is why the decomposition is printed and not just the composition.**
`by_slope` *also* saturates at the summit (slope 0.158 against `ROCK_SLOPE` 0.04), and
`natural` takes the **larger** of the two terms. So "100% rock at a summit" is
over-determined: it would be 100% rock from slope alone. The claim that the *tectonic* term
saturates rests on the decomposition — `smooth(6.2369) = 1.000000` — and on the contrast point,
where the slope is 0.00239, `by_slope` is 0.010280, and the composition reads exactly that
0.010280. Asserting the spec's claim from the composition alone would have been an unfalsifiable
measurement.

---

## 7. Requested against achieved land fraction, and the API spec §5 asks for

**Population:** the 200,000-point spiral on each world. **Method:** `island_survey.rs` section 4b,
the share of the spiral with `structural_m > 0`, at the canonical preset and at `volcanic()`.

| world | requested `land_fraction` | achieved, canonical preset | achieved, `volcanic()` | islands add |
|---|---|---|---|---|
| `island-a` | 0.4000 | 0.4021 | **0.4054** | **+0.3345 pp** |
| `owner` | 0.1600 | 0.1630 | **0.1705** | **+0.7525 pp** |
| `earth-a` | 0.2900 | 0.2928 | **0.2969** | **+0.4035 pp** |

Two separate gaps are visible and they have different causes.

- **The canonical preset already misses the requested figure by 0.21 to 0.30 pp**, with the block
  absent entirely. That is the calibrator's own residual: `Continentality::calibrate` hits
  `land_fraction` on its own 4,000-sample estimator, whose 1σ error is about ±0.58 pp, and this
  200,000-point estimator measures the result to ±0.1 pp. This slice neither caused nor changed
  it.
- **`volcanic()` adds land on top, by exactly `D_added`** — 0.3345 / 0.7525 / 0.4035 pp, the same
  three numbers as the islanded share, which is a consistency check rather than a coincidence:
  peaks only raise ground and never lower it, so every square metre they add to land is a square
  metre of island.

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

| Pin | baseline `88f199e` | **after Task 7** | note |
|---|---|---|---|
| Engine lib, `--features wasm` | 843 passed / 11 ignored | **843 / 11** | the new `[[bin]]` contributes 0 tests |
| Engine lib, `--no-default-features` | — | **843 / 11** | |
| Engine lib, default features | — | **843 / 11** | |
| Engine lib, `--features python` | — | **845 / 11** | +2 as always on the python rows |
| Engine lib, `--features python,wasm` | — | **845 / 11** | |
| `tests/blake2_bytes.rs` | 4 | **4** | every configuration |
| `tests/build_fingerprint.rs` | 9 | **9** | every configuration |
| `tests/no_std_math.rs` | 7 | **7** | every configuration |
| `tests/wasm_exports.rs` | 119 | **119** | wasm rows only |
| `tectonics.rs` `.abs()` ledger | 9 | **9** | see the note below |
| Parity, compared / divergent | 156,011 / 0 | **156,011 / 0** | §8 |
| `--mutate seed` | 147,387 | **147,387** | §8 |
| `--mutate erosion-k` | 216 | **216** | §8 |
| `--mutate water-pond` | 60 | **60** | §8 |
| `--mutate tectonic-warp` | 22,995 | **22,995** | §8 |
| Python suite, `tests/` | 575 | **575** | `WORLDBUILDER_REQUIRE_ENGINE=1` |
| Python conformance, `tests/test_conformance.py` | 167 | **167** | |
| Viewer `npm test` | 359 | **359** | not a CI pin |
| Wasm artifact | 465,699 bytes, 36 exports, 0 imports | **465,699 bytes, 36 exports, 0 imports** | |
| Wasm artifact-sha256 | — | **`b13e600356683a83a3731debfb8ec7549e460e808aa3cb0ba43114dbdce020ee`** | moved: a constant changed |
| Wasm source-fingerprint | — | **`83672441ddddcac2e68b2291c041427d80edf2b2683fd27d15a4154a88895511` (70 inputs)** | moved: `tectonics.rs` changed |
| `npm run check:wasm` | matches | **matches its manifest and the source that is here now** | |
| EOL guard | clean | **clean** | |

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

Task 7 was scoped to write no production logic. Four files moved besides the survey and this
report, and each one is a **measured figure being re-derived**, not new behaviour.

1. **`src/tectonics.rs`** — `VOLCANIC_DENSITY` 0.11 → **0.36**, the calibration this task exists
   to perform. Its doc comment now carries the three-world sweep, and
   `VOLCANIC_HEIGHT_M`/`VOLCANIC_REACH_M`/`VOLCANIC_LATTICE_M` lose their "provisional, pending
   Task 7's survey" labels and gain the measurements that settled them. No code changed.
2. **`src/surface.rs`** — two pinned measurements re-derived, no logic.
   - `an_island_stands_above_the_datum_in_open_ocean`: the count moved **34 → 88 of 20,000**
     (0.17% → 0.44%) and the band **`15..70` → `45..140`**. The doc now also records that this
     20,000-point count is a small sample, that the survey's 200,000-point 0.3345% is the figure
     to quote, and that the two are consistent to a shade over 2σ.
   - `an_island_is_steep_to_rather_than_shelved`: the doc figure moved **−2,594.07 m → −2,907.71
     m**, off a summit this task measured at **4,010.75 m** (the old summit's own height was not
     recorded, so no before/after is claimed for it). **The density moved which node
     `find_a_summit` picks, not how steep a flank is** — `peak_of_cell` gates on
     `hash >= density`, so a higher density strictly *adds* candidate cells and can never remove
     one. The assertion itself (deeper than −1,000 m) is untouched.
3. **`viewer/test/peak-params.test.mjs`** — the transcribed literal `"0.11"` → `"0.36"` in "no
   peak number is written down twice", plus a new assertion that the peak density stays distinct
   from the coast channel's 0.35 and a note on why. The file's header also now records that its
   eight witness probes were *derived* at density 0.11 and why monotonicity keeps them valid at
   0.36; the "991 of 64,800" figure is left labelled as the probe set's provenance and **not**
   restated for 0.36, because nothing re-ran that scan.
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

1. **The band is tight and the corpus is three worlds.** The admissible window is seven
   hundredths of density wide (0.32–0.38) because `island-a` presses the 0.3% floor while `owner`
   presses the 0.8% ceiling. 0.36's margin is +0.0345 pp, a little over 2σ of the estimator. **A
   fourth world with a land fraction outside 0.16–0.40 could push one end out of band**, and the
   honest fix then is not to re-tune `density` but to make the preset's density depend on
   `land_fraction` — which is a design change, not a constant. Flagging it as the first thing to
   re-measure when a new reference world appears.
2. **Spec §7 question 1 asked for "about 0.5%" as its example and 0.3–0.8% as the band.** No
   single density puts all three worlds near 0.5%: at 0.36 they read 0.33 / 0.75 / 0.40. The band
   is met on every world; the 0.5% *centre* is met on none of them, and cannot be while the share
   scales with ocean coverage. That is a question for the owner, not a defect.
3. **Roughly a tenth of the islands are at the raster's resolution floor**, so 4,617 is a lower
   bound on the count and the resolution-safe figure is the 3,094 above 200 km². A finer raster
   would raise the count and leave the area unchanged. Nobody should quote 4,617 as exact.
4. **Spec §5's accessor is not built** — a decision, argued in §7, landing with fragments in slice
   3. Recorded here so it is on the record rather than missing.
5. **Hydrology on islands is untested.** Spec §5 predicts that islands below the node spacing get
   no rivers; §4's distribution says the great majority are below it at 1,000,000 nodes. Nothing
   in this slice bakes hydrology on a peaked world, so that prediction is stated and unverified.
