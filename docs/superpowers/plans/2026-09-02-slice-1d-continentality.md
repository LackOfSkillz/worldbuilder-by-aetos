# Slice 1d — Continentality — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port `worldbuilder/terrain/continentality.py` to the Rust engine and prove it agrees with the Python.

**Architecture:** Continentality is the broad shape of land and sea — continents and ocean basins and nothing smaller. It is the first module in this port that is not pure arithmetic over its inputs: it performs a **calibration pass** at construction, sampling 4,000 points on a Fibonacci spiral and taking a quantile to decide where sea level falls. That makes it the first module with generated-and-stored state, and the first whose output depends on a sort.

It also takes a seed and nothing else. It cannot consult the plates because it has no way to reach them, which is the point — an architectural claim enforced by the import list rather than by a comment asking people to behave. Preserve that: `continentality.rs` imports `noise`, `sphere`, `tangent` and `detmath`, and nothing from `plates`.

**Tech Stack:** Rust (stable 1.98.0), `libm` 0.2.11, PyO3, maturin, Python 3.11, pytest.

**Spec:** `docs/design/2026-09-02-mark-2-world-studio.md` sections 4.2, 4.3, 4.4.

**Prior slices:** 1a built the crate, `detmath`, `Vec3`, `SpherePoint`, the bindings and the conformance harness. 1b ported `Noise`. 1c ported `TangentFrame` and established that the conformance contract is chosen **per code path, not per module**. Read the conformance section of `crates/worldbuilder-engine/README.md` before starting, especially the rules on transcribing constants.

## Contract classification for this module

Applying 1c's per-code-path principle:

| function | path | contract |
|---|---|---|
| `at` | `Noise::fbm` only — hash, floor, arithmetic | **strict**, bit-for-bit |
| calibration outputs (`shore`, `spread`) | `cos`, `sin`, `sqrt` in the spiral | **bounded**, 4 ULP |
| `above_shore` | `at` minus the calibrated shore | **bounded** (inherits from shore) |
| `base_elevation` | `powf` | **bounded** |
| `gradient` | `TangentFrame` projections | **bounded** |

## A hazard measured rather than assumed

The calibration sorts 4,000 sampled values and selects by index. A discrete selection from a sorted list of continuously-perturbed values *could* pick a different element if two neighbours straddle the quantile and a few-ULP difference reorders them — which would make `shore` differ by far more than a few ULP.

**Measured on seed 12345: the smallest gap anywhere in the sorted list is 4.6e-9**, roughly nine orders of magnitude larger than a ULP at those magnitudes. The sort order cannot flip from transcendental differences. The selection is robust.

Record this in the conformance test rather than leaving it as folklore: if `shore` or `spread` ever diverges by much more than the ULP bound, the cause is a reordered sort, not arithmetic drift, and the response is to investigate rather than widen the bound.

Reference values for seed 12345, default land fraction 0.29:

    shore  = 0.09556581019557257
    spread = 0.1984287160252961
    calibration cost, Python: about 30 ms

## Global Constraints

- Rust is at `~/.cargo/bin`, NOT on PATH in a fresh shell — begin every bash call with `export PATH="$HOME/.cargo/bin:$PATH"`.
- Python is the project venv: `PYTHONPATH=/d/dev/worldbuilder_by_aetos .venv/Scripts/python -m pytest tests/ -q`.
- Run cargo from the repository root with `-p worldbuilder-engine`, dev profile, never `--release`.
- **No std float maths.** Everything through `detmath`. The guard bans method form, `f64::` call form, `mul_add`, and bare integer casts without `// cast-ok: <reason>`.
- **Constants transcribed from the Python are written without underscore separators**, character-identical to their source. This module has more constants than any so far — that rule exists because a mis-grouped one previously survived two reviews.
- **Transcribe, do not rederive.** Operation order is load-bearing under both contracts.
- Nothing under `worldbuilder/` is modified. `worldbuilder/integration/maritime.py` has a pre-existing uncommitted change — leave it unstaged.
- Register each module in `lib.rs` BEFORE running its failing test.

### Python semantics this port must reproduce, verified by measurement

- **`(x) or 1e-6`** — Python treats both `0.0` and `-0.0` as falsy, so either becomes `1e-6`; NaN is truthy and passes through unchanged. The Rust form is `if d == 0.0 { 1e-6 } else { d }`, which is true for `-0.0` and lets NaN through. Do NOT write `if d.abs() < f64::EPSILON` or any tolerance form.
- **`min(1.0, x)`** — Python's two-argument `min` returns `x` if `x < 1.0`, else `1.0`. So NaN gives `1.0` and `-0.0` gives `-0.0`. Write it explicitly as `if x < 1.0 { x } else { 1.0 }` rather than calling `f64::min`, matching how the `to_latlon` clamp was handled in `sphere.rs`.
- **Index truncation** — `int(frac * (n - 1))` truncates toward zero, and the argument is positive here, so a cast is correct. Mark it `// cast-ok:`.

---

## File Structure

    crates/worldbuilder-engine/src/continentality.rs   Gradient, Continentality
    crates/worldbuilder-engine/src/bindings.rs         gains continentality entry points
    crates/worldbuilder-engine/src/lib.rs              gains `pub mod continentality;`
    tests/test_conformance.py                          gains a Continentality section

---

### Task 1: Constants, Gradient, and the raw field

**Files:**
- Create: `crates/worldbuilder-engine/src/continentality.rs`
- Modify: `crates/worldbuilder-engine/src/lib.rs`

**Interfaces:**
- Produces: the module constants; `Gradient { east: f64, north: f64 }` with `magnitude()`; `Continentality` with `new(world_seed, radius_m, land_fraction)` and `at(&self, point: &SpherePoint) -> f64`.

`Continentality::new` must NOT run the calibration yet — Task 2 adds it. For now store the noise field and leave `shore` and `spread` at placeholder values so `at` can be tested on its own.

- [ ] **Step 1: Write the failing test**

Create `crates/worldbuilder-engine/src/continentality.rs` with only this test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::sphere::SpherePoint;

    #[test]
    fn the_constants_match_the_python() {
        assert_eq!(BASE_FREQUENCY.to_bits(), 1.25f64.to_bits());
        assert_eq!(OCTAVES, 4);
        assert_eq!(CONTINENT_M.to_bits(), 700.0f64.to_bits());
        assert_eq!(ABYSS_M.to_bits(), (-4600.0f64).to_bits());
        assert_eq!(LAND_FRACTION.to_bits(), 0.29f64.to_bits());
        assert_eq!(CALIBRATION_SAMPLES, 4000);
        assert_eq!(GRADIENT_STEP_M.to_bits(), 20000.0f64.to_bits());
        assert_eq!(NOISE_SALT, 0x0C0FFEE);
    }

    #[test]
    fn a_gradient_reports_its_magnitude() {
        let g = Gradient { east: 3.0, north: 4.0 };
        assert_eq!(g.magnitude().to_bits(), 5.0f64.to_bits());
    }

    #[test]
    fn the_field_varies_across_the_planet() {
        let c = Continentality::new(12345, EARTH_RADIUS_M, LAND_FRACTION);
        let a = c.at(&SpherePoint::from_latlon(0.0, 0.0));
        let b = c.at(&SpherePoint::from_latlon(45.0, 90.0));
        assert_ne!(a.to_bits(), b.to_bits());
        assert!(a.is_finite() && b.is_finite());
    }

    #[test]
    fn the_field_is_reproducible() {
        let a = Continentality::new(12345, EARTH_RADIUS_M, LAND_FRACTION);
        let b = Continentality::new(12345, EARTH_RADIUS_M, LAND_FRACTION);
        let p = SpherePoint::from_latlon(31.0, 7.0);
        assert_eq!(a.at(&p).to_bits(), b.at(&p).to_bits());
    }
}
```

- [ ] **Step 2: Register the module and run the failing test**

Add `pub mod continentality;` to `lib.rs`, then `cargo test -p worldbuilder-engine continentality`. Expected: FAIL, cannot find the constants or types.

- [ ] **Step 3: Write the implementation**

Insert above the test module:

```rust
//! The broad shape of land and sea on a world.
//!
//! Ported from `worldbuilder/terrain/continentality.py`.
//!
//! This takes a seed and nothing else. It cannot consult the plates because it has no way
//! to reach them, which is the point — an architectural claim enforced by the import list
//! rather than by a comment asking people to behave. Do not add a `plates` import here.

use crate::detmath as m;
use crate::noise::Noise;
use crate::sphere::{SpherePoint, EARTH_RADIUS_M};

/// Cycles per unit of noise space at the first octave. About one and a quarter, which puts
/// the largest features somewhere near five thousand kilometres across — a continent, or
/// an ocean basin, and nothing smaller.
pub const BASE_FREQUENCY: f64 = 1.25;

/// How many octaves. Few, deliberately. Enough to stop the landmasses being simple blobs,
/// not enough to start carving a coast.
pub const OCTAVES: u32 = 4;

/// How high a continental interior stands, and how deep an ocean basin lies, in metres
/// before anything else has its say.
pub const CONTINENT_M: f64 = 700.0;
pub const ABYSS_M: f64 = -4600.0;

/// How much of the surface is dry, unless a world asks otherwise. Earth is about 29 per
/// cent, and it is the single most powerful thing a developer can turn.
pub const LAND_FRACTION: f64 = 0.29;

/// How many points to sample when working out where sea level falls.
pub const CALIBRATION_SAMPLES: usize = 4000;

/// How far apart the probes are when measuring which way the land rises, in metres.
pub const GRADIENT_STEP_M: f64 = 20000.0;

/// Salted so this field is independent of any other on the same world.
pub const NOISE_SALT: u64 = 0x0C0FFEE;

/// Which way continentality increases, here, and how sharply. Change per metre.
#[derive(Debug, Clone, Copy)]
pub struct Gradient {
    pub east: f64,
    pub north: f64,
}

impl Gradient {
    pub fn magnitude(&self) -> f64 {
        m::hypot(self.east, self.north)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Continentality {
    pub radius_m: f64,
    pub land_fraction: f64,
    noise: Noise,
    shore: f64,
    spread: f64,
}

impl Continentality {
    pub fn new(world_seed: u64, radius_m: f64, land_fraction: f64) -> Self {
        // shore and spread are placeholders until the calibration lands in the next task.
        Self {
            radius_m,
            land_fraction,
            noise: Noise::new(world_seed, NOISE_SALT),
            shore: 0.0,
            spread: 1.0,
        }
    }

    /// The raw field, before sea level has been decided.
    pub fn at(&self, point: &SpherePoint) -> f64 {
        let v = point.vector;
        self.noise.fbm(v.x, v.y, v.z, BASE_FREQUENCY, OCTAVES, 0.5, 2.0)
    }
}
```

Note the Python calls `self._noise.fbm(point, BASE_FREQUENCY, OCTAVES)` and lets `gain` and `lacunarity` default to 0.5 and 2.0. The Rust passes them explicitly; confirm those are the Python's defaults before you finish.

- [ ] **Step 4: Run and confirm the tests pass.** Then `cargo test -p worldbuilder-engine` for the whole crate.

- [ ] **Step 5: Commit**

```bash
git add crates/worldbuilder-engine/src/continentality.rs crates/worldbuilder-engine/src/lib.rs
git commit -m "engine: the raw continental field, and a module that cannot see the plates"
```

---

### Task 2: The calibration

**Files:**
- Modify: `crates/worldbuilder-engine/src/continentality.rs`

**Interfaces:**
- Produces: a private `calibrate(noise: &Noise, land_fraction: f64) -> (f64, f64)`, called from `new`.

Summed value noise clusters near the middle of its range rather than filling it, so a fixed threshold produces whatever land fraction the noise happens to feel like — measured at nought, nought and two per cent on three seeds, against Earth's twenty-nine. Sampling the field and taking the quantile that gives the asked-for fraction makes it a control rather than an accident.

Two floats, worked out once per world. Generated-and-stored, and still perfectly deterministic: the sample points are a fixed spiral and the field is a pure function.

- [ ] **Step 1: Write the failing test**

```rust
    #[test]
    fn calibration_reproduces_the_python_reference() {
        // Measured from the Python on seed 12345 at the default land fraction.
        let c = Continentality::new(12345, EARTH_RADIUS_M, LAND_FRACTION);
        assert!((c.shore_for_test() - 0.09556581019557257).abs() < 1e-12,
                "shore was {}", c.shore_for_test());
        assert!((c.spread_for_test() - 0.1984287160252961).abs() < 1e-12,
                "spread was {}", c.spread_for_test());
    }

    #[test]
    fn a_higher_land_fraction_lowers_the_shore() {
        // More land means sea level sits at a lower quantile of the same field.
        let less = Continentality::new(12345, EARTH_RADIUS_M, 0.2);
        let more = Continentality::new(12345, EARTH_RADIUS_M, 0.5);
        assert!(more.shore_for_test() < less.shore_for_test());
    }

    #[test]
    fn the_spread_is_never_zero() {
        let c = Continentality::new(12345, EARTH_RADIUS_M, LAND_FRACTION);
        assert!(c.spread_for_test() != 0.0);
    }
```

Add `#[cfg(test)]` accessors `shore_for_test()` and `spread_for_test()` so the tests can observe the real values without widening the public API. (This is the pattern slice 1b arrived at after a pinning test turned out to be a compiler-folded tautology.)

- [ ] **Step 2: Run and confirm it fails** — the placeholder shore of 0.0 will not match.

- [ ] **Step 3: Write the implementation**

```rust
    /// Where sea level falls, and how varied the field is.
    ///
    /// The sample points are a fixed Fibonacci spiral and the field is a pure function, so
    /// this is generated-and-stored and still perfectly deterministic.
    fn calibrate(noise: &Noise, land_fraction: f64) -> (f64, f64) {
        let golden = core::f64::consts::PI * (3.0 - m::sqrt(5.0));
        let n = CALIBRATION_SAMPLES;
        let mut values: Vec<f64> = Vec::with_capacity(n);

        for index in 0..n {
            let z = 1.0 - 2.0 * (index as f64 + 0.5) / (n as f64); // cast-ok: loop counter to float, no truncation
            let inner = 1.0 - z * z;
            // Python writes max(0.0, 1.0 - z*z); two-argument max returns the second
            // argument when the comparison is false, so a NaN inner would yield 0.0.
            let ring = m::sqrt(if inner > 0.0 { inner } else { 0.0 });
            let angle = golden * index as f64; // cast-ok: loop counter to float, no truncation
            let point = SpherePoint {
                vector: crate::vectors::Vec3::new(m::cos(angle) * ring, m::sin(angle) * ring, z),
            };
            // The sample point is deliberately NOT normalised — the Python builds the
            // vector directly and hands it to SpherePoint, and the spiral already lies on
            // the unit sphere to within rounding.
            let v = point.vector;
            values.push(noise.fbm(v.x, v.y, v.z, BASE_FREQUENCY, OCTAVES, 0.5, 2.0));
        }

        // Python's list.sort() on floats and Rust's stable sort_by agree: no NaN is
        // produced here, and -0.0 compares equal to 0.0 in both, with stability keeping
        // the original order in that case.
        values.sort_by(|a, b| a.partial_cmp(b).expect("the field produces no NaN"));

        let last = (n - 1) as f64; // cast-ok: count to float, exact for n far below 2^53
        let shore_index = ((1.0 - land_fraction) * last) as usize; // cast-ok: truncation, matching Python's int()
        let spread_index = (0.84 * last) as usize; // cast-ok: truncation, matching Python's int()
        let shore = values[shore_index];
        let middle = values[n / 2];
        let difference = values[spread_index] - middle;
        // Python writes `... or 1e-6`, and both 0.0 and -0.0 are falsy there, so either
        // becomes 1e-6. NaN is truthy and passes through unchanged.
        let spread = if difference == 0.0 { 1e-6 } else { difference };
        (shore, spread)
    }
```

Then call it from `new`, replacing the placeholders:

```rust
        let noise = Noise::new(world_seed, NOISE_SALT);
        let (shore, spread) = Self::calibrate(&noise, land_fraction);
```

- [ ] **Step 4: Run and confirm the tests pass.** Report the calibration's wall-clock cost — Python's is about 30 ms and the Rust should be far below that.

- [ ] **Step 5: Commit**

```bash
git add crates/worldbuilder-engine/src/continentality.rs
git commit -m "engine: sea level as a calibrated quantile, not an accident"
```

---

### Task 3: Elevation

**Files:**
- Modify: `crates/worldbuilder-engine/src/continentality.rs`

**Interfaces:**
- Produces: `above_shore(&self, point) -> f64` and `base_elevation(&self, point) -> f64`.

The raw field is *not* zero at the shore — sea level is a calibrated quantile of it, and that threshold is nowhere near zero. Anything wanting to know how far the coast is has to measure from `above_shore`, and the first version of the shelf did not: it estimated the distance to where the raw field crossed zero and reported coastlines fifteen hundred kilometres away.

The elevation curve is deliberately not linear. Earth's surface is bimodal — a great deal of shelf near sea level, a great deal of abyssal plain far below, not much in between — so a straight ramp would put far too much of the planet at the depths a ship sails in. The exponent below one makes the ground climb quickly near the shore and flatten far from it.

The seaward side is **linear**, and that was measured rather than chosen. It began at an exponent of a half, which put the seabed a thousand metres down eighty kilometres offshore, leaving no room for a shelf — the shelf would have been inventing nine hundred metres of ground rather than shaping it. Linear puts that same point at about two hundred and forty metres, which a shelf can plausibly correct.

- [ ] **Step 1: Write the failing test**

```rust
    #[test]
    fn above_shore_is_zero_at_the_calibrated_shoreline() {
        let c = Continentality::new(12345, EARTH_RADIUS_M, LAND_FRACTION);
        // A point whose raw field equals the shore has above_shore exactly zero.
        let p = SpherePoint::from_latlon(17.0, 43.0);
        let expected = c.at(&p) - c.shore_for_test();
        assert_eq!(c.above_shore(&p).to_bits(), expected.to_bits());
    }

    #[test]
    fn elevation_is_bounded_by_the_continent_and_the_abyss() {
        let c = Continentality::new(12345, EARTH_RADIUS_M, LAND_FRACTION);
        for lat in (-80..81).step_by(10) {
            for lon in (-180..181).step_by(20) {
                let e = c.base_elevation(&SpherePoint::from_latlon(lat as f64, lon as f64));
                assert!(e <= CONTINENT_M, "{} at {},{}", e, lat, lon);
                assert!(e >= ABYSS_M, "{} at {},{}", e, lat, lon);
            }
        }
    }

    #[test]
    fn the_seaward_side_is_linear() {
        // Twice as far below the shore is twice as deep, until the abyss clamps it.
        let c = Continentality::new(12345, EARTH_RADIUS_M, LAND_FRACTION);
        let spread = c.spread_for_test();
        let quarter = c.elevation_from_above(-0.25 * spread / spread);
        let half = c.elevation_from_above(-0.5 * spread / spread);
        assert!((half - 2.0 * quarter).abs() < 1e-9, "{} vs {}", half, quarter);
    }
```

The third test needs a small helper `elevation_from_above(&self, above: f64) -> f64` holding the curve itself, with `base_elevation` computing `above` and delegating. That keeps the curve testable without constructing a point that happens to land where you want.

- [ ] **Step 2: Run and confirm it fails**

- [ ] **Step 3: Write the implementation**

```rust
    /// How far above the shoreline this point stands, in field units. Zero exactly at the
    /// coast, positive inland.
    pub fn above_shore(&self, point: &SpherePoint) -> f64 {
        self.at(point) - self.shore
    }

    /// Elevation relative to datum, before tectonics or detail.
    pub fn base_elevation(&self, point: &SpherePoint) -> f64 {
        self.elevation_from_above(self.above_shore(point) / self.spread)
    }

    /// The curve itself, separated so it can be exercised without hunting for a point that
    /// happens to land at a given height.
    pub fn elevation_from_above(&self, above: f64) -> f64 {
        if above >= 0.0 {
            // Python: CONTINENT_M * min(1.0, above) ** 0.75
            let capped = if above < 1.0 { above } else { 1.0 };
            CONTINENT_M * m::powf(capped, 0.75)
        } else {
            // Linear on the seaward side, and that number was measured rather than chosen.
            // Python: ABYSS_M * min(1.0, -above)
            let depth = -above;
            let capped = if depth < 1.0 { depth } else { 1.0 };
            ABYSS_M * capped
        }
    }
```

Note both `min` calls are written explicitly as `if x < 1.0 { x } else { 1.0 }` rather than `f64::min`, matching Python's two-argument `min` semantics exactly — including that NaN yields `1.0`.

- [ ] **Step 4: Run and confirm the tests pass**

- [ ] **Step 5: Commit**

```bash
git add crates/worldbuilder-engine/src/continentality.rs
git commit -m "engine: an elevation curve that leaves room for a shelf"
```

---

### Task 4: Gradient

**Files:**
- Modify: `crates/worldbuilder-engine/src/continentality.rs`

**Interfaces:**
- Produces: `gradient(&self, point: &SpherePoint) -> Gradient`.

**Measured along geodesics, not by nudging the raw coordinates.** A finite difference taken in x, y and z would step *off* the sphere and measure the noise volume rather than the planet's surface, and the error would grow with latitude in a way nobody would notice until the shelves near the poles came out wrong. The tangent frame already knows how to walk a fixed number of metres in a real direction, so it does.

Four samples, and it is not free: this costs five evaluations where the value alone costs one. Which is exactly why it is a separate call — the shelf shaper will want it near a coast; open ocean never will, and should not pay for it.

- [ ] **Step 1: Write the failing test**

```rust
    #[test]
    fn the_gradient_points_uphill() {
        let c = Continentality::new(12345, EARTH_RADIUS_M, LAND_FRACTION);
        let p = SpherePoint::from_latlon(20.0, 30.0);
        let g = c.gradient(&p);
        let frame = crate::tangent::TangentFrame::at(&p, EARTH_RADIUS_M);
        // Stepping a little way along the gradient should raise the field.
        let step = 5000.0;
        let scale = step / g.magnitude();
        let uphill = frame.local_to_sphere(g.east * scale, g.north * scale);
        assert!(c.at(&uphill) > c.at(&p), "gradient did not point uphill");
    }

    #[test]
    fn the_gradient_is_finite_everywhere_including_the_poles() {
        let c = Continentality::new(12345, EARTH_RADIUS_M, LAND_FRACTION);
        for (lat, lon) in [(90.0, 0.0), (-90.0, 0.0), (0.0, 0.0), (45.0, -170.0)] {
            let g = c.gradient(&SpherePoint::from_latlon(lat, lon));
            assert!(g.east.is_finite() && g.north.is_finite(), "at {},{}", lat, lon);
        }
    }
```

- [ ] **Step 2: Run and confirm it fails**

- [ ] **Step 3: Write the implementation**

```rust
    /// Which way continentality rises, measured along the surface.
    pub fn gradient(&self, point: &SpherePoint) -> Gradient {
        let frame = crate::tangent::TangentFrame::at(point, self.radius_m);
        let step = GRADIENT_STEP_M;
        let east = self.at(&frame.local_to_sphere(step, 0.0));
        let west = self.at(&frame.local_to_sphere(-step, 0.0));
        let north = self.at(&frame.local_to_sphere(0.0, step));
        let south = self.at(&frame.local_to_sphere(0.0, -step));
        Gradient {
            east: (east - west) / (2.0 * step),
            north: (north - south) / (2.0 * step),
        }
    }
```

The four probes are taken in the Python's order — east, west, north, south — and each difference divided by `2.0 * step`, not multiplied by a precomputed reciprocal.

- [ ] **Step 4: Run and confirm the tests pass**

- [ ] **Step 5: Commit**

```bash
git add crates/worldbuilder-engine/src/continentality.rs
git commit -m "engine: a gradient walked along geodesics rather than through the noise volume"
```

---

### Task 5: Bindings and conformance

**Files:**
- Modify: `crates/worldbuilder-engine/src/bindings.rs`, `crates/worldbuilder-engine/src/lib.rs`
- Modify: `tests/test_conformance.py`

**Interfaces:**
- `continentality_calibration(seed, land_fraction) -> (shore, spread)`
- `continentality_at(seed, land_fraction, x, y, z) -> f64`
- `continentality_above_shore(seed, land_fraction, x, y, z) -> f64`
- `continentality_base_elevation(seed, land_fraction, x, y, z) -> f64`
- `continentality_gradient(seed, land_fraction, radius_m, x, y, z) -> (east, north)`

**Constructing a `Continentality` per call runs the 4,000-sample calibration each time.** That is far too slow for a corpus loop. Instead, build it once per (seed, land_fraction) and cache it — the simplest correct approach is a `std::sync::OnceLock<Mutex<HashMap<(u64, u64), Continentality>>>` keyed on the seed and the land fraction's bit pattern, or a single-entry memo of the last-used pair. Choose the simplest thing that keeps the bindings stateless from Python's point of view and does not change any value. Say in your report which you chose and why.

Apply the contract table from the top of this plan: `continentality_at` uses `same()`; everything else uses `close_enough`.

- [ ] **Step 1: Add the bindings and register them**

- [ ] **Step 2: Rebuild** with `maturin develop --release` into the project venv.

- [ ] **Step 3: Add the conformance tests**, covering:
  - `at` against the Python over a corpus of sphere points, **strictly**.
  - the calibration pair for several seeds and several land fractions, bounded.
  - `above_shore`, `base_elevation` and `gradient` over the corpus, bounded, reporting ULP distance on failure.
  - the poles explicitly, since `gradient` builds a tangent frame there.
  - a test recording that `shore` and `spread` agree far more tightly than the sort's smallest neighbour gap (measured at 4.6e-9), so a future divergence of that size indicates a reordered sort rather than arithmetic drift.

- [ ] **Step 4: Run the conformance suite and the whole suite**, quoting both.

**If `continentality_at` fails strictly, that is a real defect** — it is `Noise::fbm` and nothing else, and `Noise` already has passing strict conformance. Do not loosen it; report it.

**If the calibration diverges by much more than the ULP bound**, the cause is a reordered sort, not arithmetic. Report which index differs and the neighbouring values. Do not widen the bound.

- [ ] **Step 5: Commit**

---

### Task 6: Record it

**Files:**
- Modify: `crates/worldbuilder-engine/README.md`

- [ ] **Step 1** — add `src/continentality.rs` to the listing; record the contract classification for this module and why each function falls where it does; record the calibration's cost in Rust against Python's 30 ms; record the measured 4.6e-9 minimum gap and what it protects. Verify all test counts by running the suites rather than copying figures from this plan.

- [ ] **Step 2** — commit.

---

## What this slice deliberately does not do

- **No tectonics, shelf, detail or surface.** Each is its own slice.
- **No deletion of `worldbuilder/terrain/continentality.py`.** The Python stays the reference.
- **No caching of the field itself.** Only the calibration is stored, exactly as the Python does.
