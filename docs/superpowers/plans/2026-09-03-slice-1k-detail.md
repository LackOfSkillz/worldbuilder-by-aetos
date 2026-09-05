# Slice 1k: Terrain Detail Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port `worldbuilder/terrain/detail.py` to `crates/worldbuilder-engine/src/detail.rs` — texture, and only texture.

**Architecture:** Detail roughens ground that structure has already decided. It decides nothing itself: no coves, no shoals, no bars, no islands. Two bands — meso (1–20 km) and micro (250 m–1 km) — with amplitude blended from where you are, and octaves that fade rather than switch off as resolution coarsens.

**Tech Stack:** Rust (engine), PyO3 bindings, pytest differential harness against the Python reference.

**Spec:** `docs/design/2026-09-02-mark-2-world-studio.md`, sections 4.1 (bit-equality), 4.2 (DETERMINISM-001), 6 (VERSION-001).

## Global Constraints

- **All maths through `detmath`** where a routed function exists. No `f64::` method or associated-function form, no `mul_add`, no bare integer casts without a `// cast-ok: <reason>` marker on the **same line** as the cast. A build-failing guard test enforces this. `abs` is exempt and used bare.
- **Constants transcribed character-for-character**, including underscore separators and trailing zeros: `CANONICAL_WAVELENGTH_M = 250.0`, `COARSEST_WAVELENGTH_M = 20_000.0`, `BARELY_M = 2.0`, `CLEARLY_M = 4.0`, `ABYSSAL_M = 55.0`, `SHELF_M = 15.0`, `COAST_M = 35.0`, `INTERIOR_M = 80.0`, `MOUNTAIN_M = 150.0`. A mis-grouped constant earlier in this project was "verified" by two agents and caught only by a differential test.
- **Never `f64::min` / `f64::max` / `clamp`.** Python's two-argument forms are asymmetric under NaN. Every clamp is an explicit `if`/`else` in the Python's operand order; `plates.rs::margin_at` has the house form.
- **Nothing under `worldbuilder/` may be modified.** The Python remains the reference. `worldbuilder/integration/maritime.py` has a pre-existing uncommitted change; leave it unstaged.
- **A floating-point sign, zero, or exact-equality assertion without the reason it holds is a latent bug.** Five have occurred in this port. Establish why each holds, or do not assert it.
- **Verify by exit status, not by grepping `test result:` lines.** A defect in slice 1i survived three reviews that way.

---

## The contract: this is the cleanest module in the port

**`detail.py` contains no transcendental function call anywhere, in any path.** `math.pi` appears once, but it is a *constant*, not an operation — and slice 1h confirmed Rust's `PI` is bit-identical to CPython's `math.pi`. `Noise` reaches only `floor`, which is exact. There is no `sqrt`, no `hypot`, no trigonometry.

**So everything this module computes is on the strict bit-for-bit contract, and there is no bounded quantity at all.** Kinematics came close — it had a `sqrt`, which is at least correctly rounded — but this module has nothing. Every conformance comparison uses `same()`.

That also settles the discrete decisions cheaply: every comparison here is on an exactly reproducible value, so no branch can diverge. **State that as the reason rather than checking each one anxiously.**

---

## Four transcription traps

**1. The frequency expression must be transcribed, not simplified — and I checked how much that matters.**

```python
frequency = 2.0 * math.pi * self.radius_m / wavelength / (2.0 * math.pi)
```

That is algebraically `radius_m / wavelength`, and the temptation to write the short form is obvious. **At Earth's radius, for all seven configured wavelengths, the two forms give bit-identical results** — I measured all seven, so simplifying would not break the default world.

**But they are not equal in general.** Sampling 200,000 random radii found divergences, for example `R = 32450893.20683292` with `wavelength = 10000.0`, where the written form gives `3245.0893206832916` and the simplification gives `3245.089320683292`. Since `radius_m` is a constructor parameter, a non-Earth world would hit it.

So transcribe the four operations in order. The requirement is prophylactic for Earth and load-bearing for anything else.

**2. `or` on a possibly-zero float.**

```python
total = sum(band[2] for band in bands) or 1.0
```

`0.0` and `-0.0` are falsy in Python, `NaN` is truthy. So this returns `1.0` when the sum is either zero, and returns `NaN` unchanged. In Rust: `if total == 0.0 { 1.0 } else { total }` — which gives `1.0` for `-0.0` (since `-0.0 == 0.0`) and `NaN` for `NaN` (since `NaN == 0.0` is false). Both correct. Do not write `if total != 0.0`-style inversions without checking the NaN case again.

**3. `if resolution_m:` is falsy for BOTH `None` and `0.0`.**

```python
if resolution_m:
    visible = _smooth(...)
    ...
else:
    visible = 1.0
```

A Rust `Option<f64>` port diverges on `Some(0.0)`: `is_some()` is true, so it would take the resolution branch and divide by zero. **`Some(0.0)` must behave exactly as `None` does.** Whatever representation you choose, that equivalence is the requirement, and Task 3 must test it explicitly.

**4. `break`, not `continue`.** When an octave becomes invisible the loop stops entirely: *"Everything finer is finer still, so nothing below can be visible."* Bands are ordered coarsest-first, so this is correct and is not an optimisation you may reorder.

---

## The recorded reasons, which are requirements

**Why octaves fade rather than switch off:** *"Dropping one the instant it becomes unrepresentable would be a cliff in resolution rather than in position — the ground would jump as somebody zoomed, which is the same bug M1.4 kept producing, in a different axis."* The fade runs between `BARELY_M` and `CLEARLY_M` multiples of the sample spacing.

**Why sub-sample frequencies are skipped rather than merely wasted:** *"They alias: an octave shorter than the spacing lands somewhere different in every grid, so a chart would shimmer as a ship moved rather than showing generalised ground."*

**Why the shares are normalised:** *"otherwise adding an octave would quietly make every world rougher."*

**Why canonical is a defined thing:** *"`resolution_m=None` evaluates every configured octave down to `CANONICAL_WAVELENGTH_M` and no further. Without that written down, somebody adds a fifty-metre octave in three months and every coastline, reef and chart in every world silently changes."*

**Why the trench term exists in `amplitude_m`:** *"a deep, deliberate piece of structure stays legible instead of being buried under texture that has no idea it is there."*

**And the quantitative rule the whole module serves:** *"detail amplitude stays well below structural relief. A five-kilometre octave may not turn twenty metres of shelf water into an island."*

---

## File Structure

- **Create** `crates/worldbuilder-engine/src/detail.rs`, declared in `src/lib.rs`.
- **Modify** `crates/worldbuilder-engine/src/bindings.rs` and `src/lib.rs` — the bindings.
- **Modify** `tests/test_conformance.py` — the differential tests.
- **Modify** `crates/worldbuilder-engine/README.md` — the record.

`Noise` is the only dependency and is already ported: `Noise::new(seed, salt)` and `.at(x, y, z)` match `detail.py`'s use line for line. `detail.py` never calls `fbm`, so the known Python/Rust `fbm` signature difference is irrelevant here — **do not "fix" it as part of this slice.**

---

### Task 1: Constants, `_smooth`, and the band table

**Files:** Create `crates/worldbuilder-engine/src/detail.rs`; modify `src/lib.rs`

**Interfaces:**
- Produces: every module constant.
- Produces: `fn smooth(fraction: f64) -> f64`
- Produces: `pub struct Detail { radius_m: f64, noise: Noise, bands: Vec<Band> }` with `Band { wavelength_m, frequency, share }`, and a constructor taking `(world_seed, radius_m)`.

`Noise` is constructed as `Noise(world_seed, salt=0x5EABED)` — transcribe that salt exactly.

`_smooth` is `max(0.0, min(1.0, fraction))` then the smoothstep `x * x * (3.0 - 2.0 * x)` — the same shape slices 1g and 1i ported. Explicit `if`/`else` clamps in the Python's operand order.

- [ ] **Step 1: Write the failing tests.**

**The band table is measured from the live Python at `EARTH_RADIUS_M`, not derived by hand:**

```rust
#[test]
fn the_band_table_is_seven_octaves_from_twenty_kilometres_down() {
    // Measured from the Python, not computed here: the loop halves the wavelength
    // from COARSEST_WAVELENGTH_M while it stays at or above CANONICAL_WAVELENGTH_M,
    // and 312.5 is the last that qualifies -- 156.25 is below 250.
    let d = Detail::new(20260831, EARTH_RADIUS_M);
    let want: [(f64, f64); 7] = [
        (20000.0, 318.55),
        (10000.0, 637.1),
        (5000.0, 1274.2),
        (2500.0, 2548.4),
        (1250.0, 5096.8),
        (625.0, 10193.6),
        (312.5, 20387.2),
    ];
    assert_eq!(d.bands().len(), 7);
    for (i, (w, f)) in want.iter().enumerate() {
        assert_eq!(d.bands()[i].wavelength_m.to_bits(), w.to_bits(), "band {i} wavelength");
        assert_eq!(d.bands()[i].frequency.to_bits(), f.to_bits(), "band {i} frequency");
    }
}

#[test]
fn the_shares_are_normalised_to_exactly_one() {
    // "otherwise adding an octave would quietly make every world rougher". The raw
    // shares halve from 1.0, so they sum to 2 - 0.5^6; dividing through gives 1.0,
    // and it lands exactly on 1.0 for this table -- measured, not assumed.
    let d = Detail::new(20260831, EARTH_RADIUS_M);
    let total: f64 = d.bands().iter().map(|b| b.share).sum();
    assert_eq!(total, 1.0, "shares must normalise to exactly one, got {total}");
}

#[test]
fn smooth_saturates_at_both_ends() {
    assert_eq!(smooth(-10.0), 0.0);
    assert_eq!(smooth(10.0), 1.0);
    assert_eq!(smooth(0.5), 0.5);
}
```

**Verify the seven frequency values against the live Python before trusting them** — they were measured in one run and this project's plans have carried wrong numbers before. If any differs, report what you measured rather than adjusting the test to pass.

**`smooth(0.5) == 0.5` exactly** because `0.5 * 0.5 * (3.0 - 2.0 * 0.5)` is `0.25 * 2.0`; every step is exact. Confirm that reasoning holds before relying on it.

- [ ] **Step 2: Run them and confirm they fail** for the expected reason, not an unrelated compile error.
- [ ] **Step 3: Implement.** Transcribe the frequency expression in its four-operation form — see trap 1. Reproduce `sum(...) or 1.0` per trap 2.
- [ ] **Step 4: Run again, then the whole crate suite, checking exit status.**
- [ ] **Step 5: Commit.**

---

### Task 2: `amplitude_m`

**Files:** Modify `crates/worldbuilder-engine/src/detail.rs`

**Interfaces:** Produces `pub fn amplitude_m(&self, point: &SpherePoint, elevation_m: f64, shelf_weight: f64, tectonic_m: f64) -> f64`

Transcribe lines 100-135. Note `point` is accepted but **not used** in the Python — keep the parameter for signature fidelity and say so in a comment, rather than dropping it and diverging from the reference API.

The blend is a chain of weighted terms, and **its operation order is load-bearing** under a bit-for-bit contract:

```python
deep = 1.0 - _smooth((elevation_m + 3000.0) / 2500.0)
high = _smooth((elevation_m - 200.0) / 900.0)
near_shore = _smooth(1.0 - abs(elevation_m) / 350.0)

rough = deep * ABYSSAL_M + (1.0 - deep) * (1.0 - high) * INTERIOR_M + high * MOUNTAIN_M
rough = rough * (1.0 - near_shore) + COAST_M * near_shore
rough = rough * (1.0 - shelf_weight) + SHELF_M * shelf_weight

quieted = 1.0 - 0.7 * _smooth(abs(tectonic_m) / 1200.0)
return rough * quieted
```

Every magic number here — `3000.0`, `2500.0`, `200.0`, `900.0`, `350.0`, `0.7`, `1200.0` — is an inline literal in the Python. **Transcribe them as literals; do not promote them to named constants**, which would look tidier and put this slice's constants out of step with the reference.

- [ ] **Step 1: Write the failing tests**, covering: deep abyssal ground gives roughly `ABYSSAL_M`; a mountain gives roughly `MOUNTAIN_M`; full shelf weight pulls the answer to `SHELF_M`; and a large `tectonic_m` quiets the result toward 30% of what it would otherwise be. **Derive the expected values from the formula and say how**, rather than recording whatever the implementation returns — a test written from the output cannot detect a wrong implementation.
- [ ] **Steps 2-5:** Run, implement, run, commit.

---

### Task 3: `offset_m`, and the falsy-zero trap

**Files:** Modify `crates/worldbuilder-engine/src/detail.rs`

**Interfaces:** Produces `pub fn offset_m(&self, point: &SpherePoint, amplitude_m: f64, resolution_m: Option<f64>) -> f64`

Transcribe lines 137-185. **`Some(0.0)` must behave exactly as `None`** — see trap 3. That is the single most likely divergence in this task.

Other points:
- `if amplitude_m <= 0.0: return 0.0` — an early exit on an exactly reproducible value.
- The accumulation is a running `total +=` in band order; **floating-point addition is not associative**, so the order is load-bearing. Do not sum in parallel or reorder.
- The per-band term is `(noise.at(x*f, y*f, z*f) - 0.5) * 2.0 * share * visible`, in that order.
- `break`, not `continue`, when `visible <= 0.0` — see trap 4.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test]
fn a_resolution_of_zero_behaves_exactly_like_canonical() {
    // Python's `if resolution_m:` is false for BOTH None and 0.0, so a caller
    // passing zero gets every octave, not a division by zero. A Rust Option port
    // diverges here unless Some(0.0) is special-cased -- this is the test that
    // catches it, and it must be bit-exact rather than approximate.
    let d = Detail::new(20260831, EARTH_RADIUS_M);
    let p = SpherePoint::from_latlon(17.0, 43.0);
    let canonical = d.offset_m(&p, 100.0, None);
    let zero = d.offset_m(&p, 100.0, Some(0.0));
    assert_eq!(zero.to_bits(), canonical.to_bits(), "Some(0.0) must equal None");
}

#[test]
fn zero_amplitude_returns_exactly_zero() {
    let d = Detail::new(20260831, EARTH_RADIUS_M);
    let p = SpherePoint::from_latlon(17.0, 43.0);
    assert_eq!(d.offset_m(&p, 0.0, None), 0.0);
    assert_eq!(d.offset_m(&p, -1.0, None), 0.0);
}

#[test]
fn a_coarse_resolution_drops_the_fine_octaves() {
    // At a sample spacing of 5 km, an octave of 312.5 m is far below Nyquist and
    // must contribute nothing, so the coarse answer differs from the canonical one.
    let d = Detail::new(20260831, EARTH_RADIUS_M);
    let p = SpherePoint::from_latlon(17.0, 43.0);
    let canonical = d.offset_m(&p, 100.0, None);
    let coarse = d.offset_m(&p, 100.0, Some(5000.0));
    assert!(canonical != coarse, "a coarse resolution must drop fine octaves");
}
```

**Also write a test that the fade is gradual rather than a step** — sample `resolution_m` across the range where an octave dims, and require that consecutive samples differ by less than a bound you derive. That is the property the docstring calls out (*"a cliff in resolution rather than in position"*), and without it a port that dropped octaves abruptly would pass everything above. **Say how you derived the bound**, and make sure the sampled range actually crosses a fade — a range that does not would make the test vacuous, which has happened three times in this port.

- [ ] **Steps 2-5:** Run, implement, run, commit.

---

### Task 4: Conformance

**Files:** Modify `crates/worldbuilder-engine/src/bindings.rs`, `src/lib.rs`, `tests/test_conformance.py`

**Interfaces:**
- `detail_smooth(fraction) -> f64`
- `detail_bands(world_seed, radius_m) -> list[(f64, f64, f64)]`
- `detail_amplitude_m(world_seed, radius_m, x, y, z, elevation_m, shelf_weight, tectonic_m) -> f64`
- `detail_offset_m(world_seed, radius_m, x, y, z, amplitude_m, resolution_m) -> f64` — with `resolution_m` optional, and **`0.0` reaching the same path as absent**.

**Everything here is STRICT.** There is no transcendental in any path, so **every comparison uses `same()`, bit-for-bit** — the band table, the amplitudes, the offsets, all of it. **If any comparison needs a tolerance, that is a finding to report, not a bound to add**: it would mean something in the port is not what this plan claims.

**Cover:** the existing `corpus()` of sphere points; several world seeds; a non-Earth `radius_m` (**which is where trap 1 would bite**); elevations spanning abyssal, shelf, coast, interior and mountain; shelf weights of 0, 0.5 and 1; tectonic contributions from zero to well past 1200; and `resolution_m` of `None`, `0.0`, a fine value, a value mid-fade, and one coarse enough to drop every band.

- [ ] **Steps:** bindings, rebuild with `maturin develop --release`, tests, run both suites quoting them and checking exit status, commit.

---

### Task 5: Record it

**Files:** Modify `crates/worldbuilder-engine/README.md`

- [ ] **Step 1: Record** — that this is **the first module in the port with no transcendental in any path at all**, so it is entirely strict with no bounded quantity, and why (`math.pi` is a constant, `Noise` reaches only `floor`); that every discrete decision is therefore safe for that single reason rather than each needing its own argument; the four traps, with **trap 1 stated honestly** — the simplification is bit-identical at Earth's radius for all seven wavelengths, but diverges at other radii, so the four-operation form is prophylactic for the default world and load-bearing for any other; the seven-band table and that the shares normalise to exactly 1.0; and the recorded reasons the module gives for fading octaves rather than dropping them, and for skipping sub-sample frequencies.
- [ ] **Step 2: Verify every count by running the suites and checking exit status.** Do not copy a number from any report — a count in an earlier README was wrong by twelve because it came from an extraction nobody re-ran.
- [ ] **Step 3: Commit.**

---

## What this slice deliberately does not do

- **No `surface.py`.** It composes detail with continentality, tectonics, the shelf, substrate and features, and must come after all of them.
- **No bathymetry.** `shelf.py`, `substrate.py` and `features.py` are each their own slice; all three have their dependencies ported already.
- **No `fbm` signature reconciliation.** The Python and Rust `Noise.fbm` take different argument types, but `detail.py` never calls it. Out of scope.
- **No deletion of the Python.** It stays the reference.
