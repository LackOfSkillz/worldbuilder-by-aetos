# Slice 1b — Noise — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port `worldbuilder/terrain/noise.py` to the Rust engine and prove it agrees with the Python bit-for-bit.

**Architecture:** `Noise` is the generator's most-used primitive — about forty calls per terrain sample and several million per chart — and everything above it (continentality, tectonics, shelves, detail) is built on it. It is also, unusually, **entirely free of transcendentals**: a 64-bit integer hash, a floor, and pure arithmetic. So unlike the geometry layer it falls wholly under the harness's strict bit-for-bit contract, with no ULP bound anywhere.

**Tech Stack:** Rust (stable 1.98.0), `libm` 0.2.11, PyO3, maturin, Python 3.11, pytest.

**Spec:** `docs/design/2026-09-02-mark-2-world-studio.md` — sections 4.2, 4.3, 4.4.

**Prior slice:** `docs/superpowers/plans/2026-09-02-slice-1a-the-bridge.md`. Slice 1a built the crate, `detmath`, the `Vec3`/`SpherePoint` port, the PyO3 bindings and the conformance harness. Read `crates/worldbuilder-engine/README.md` for what the two conformance contracts mean.

## Global Constraints

- Rust is at `~/.cargo/bin` and is NOT on PATH in a fresh shell — begin every bash call with `export PATH="$HOME/.cargo/bin:$PATH"`. Shell state does not persist between tool calls.
- Python is the project venv. Use `.venv/Scripts/python` with `PYTHONPATH` set to the repo root: `PYTHONPATH=/d/dev/worldbuilder_by_aetos .venv/Scripts/python -m pytest tests/ -q`.
- Run cargo from the repository root with `-p worldbuilder-engine`. Tests use the dev profile — never `--release`.
- **No std float maths.** Everything through `detmath`. A build-failing guard enforces it and covers method form, `f64::` function-call form, and bare integer casts. It walks `src/` recursively.
- **`detmath::floor`, never `as i64`.** Python derives lattice cells with `int(x // 1)`, which floors toward negative infinity; a cast truncates toward zero, and on any negative coordinate — half the sphere — that picks a different cell, silently. If a cast is genuinely needed after flooring, mark the line `// cast-ok: <reason>`.
- **Transcribe, do not rederive.** Floating-point addition is not associative; a reordered sum is a different number. This slice's whole standard is bit-for-bit.
- Nothing under `worldbuilder/` is modified. The Python stays the reference.
- `worldbuilder/integration/maritime.py` has a pre-existing uncommitted change. Leave it unstaged.
- **Python reference values**, read from the source:
  - `MASK = (1 << 64) - 1`, `SCALE = float(1 << 64)`
  - seed mixing: `self.seed = (seed * 0x100000001B3) ^ (salt * 0x9E3779B97F4A7C15)` — unmasked in Python, masked later inside `_lattice`. Verified equivalent to `u64` wrapping arithmetic across seeds including 2^63.
  - `_lattice` constants: `0x9E3779B97F4A7C15`, `0xC2B2AE3D27D4EB4F`, `0x165667B19E3779F9`, `0x27D4EB2F165667C5`, `0xFF51AFD7ED558CCD`, `0xC4CEB9FE1A85EC53`
  - `fbm` defaults: `gain=0.5`, `lacunarity=2.0`

---

## File Structure

    crates/worldbuilder-engine/src/noise.rs   Noise: seed, lattice, at, fbm
    crates/worldbuilder-engine/src/bindings.rs   gains noise entry points
    crates/worldbuilder-engine/src/lib.rs        gains `pub mod noise;` and registrations
    tests/test_conformance.py                    gains a Noise section

---

### Task 1: The lattice hash

**Files:**
- Create: `crates/worldbuilder-engine/src/noise.rs`
- Modify: `crates/worldbuilder-engine/src/lib.rs`

**Interfaces:**
- Produces: `noise::Noise` with `Noise::new(seed: u64, salt: u64)`, and a private `lattice(&self, ix: i64, iy: i64, iz: i64) -> f64`.

Register `pub mod noise;` in `lib.rs` BEFORE running the failing test — an unregistered module is not part of the crate and the run would report zero tests rather than the error you are looking for.

- [ ] **Step 1: Write the failing test**

Create `crates/worldbuilder-engine/src/noise.rs` with only this test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lattice_is_a_pure_function_of_its_coordinates() {
        let n = Noise::new(12345, 0x0C0FFEE);
        assert_eq!(n.lattice(3, -4, 5).to_bits(), n.lattice(3, -4, 5).to_bits());
    }

    #[test]
    fn the_lattice_lands_in_the_unit_interval() {
        let n = Noise::new(12345, 0x0C0FFEE);
        for (ix, iy, iz) in [(0, 0, 0), (1, 2, 3), (-1, -2, -3), (i64::MAX, i64::MIN, 7)] {
            let v = n.lattice(ix, iy, iz);
            assert!((0.0..1.0).contains(&v), "lattice({},{},{}) was {}", ix, iy, iz, v);
        }
    }

    #[test]
    fn neighbouring_cells_differ() {
        let n = Noise::new(12345, 0x0C0FFEE);
        assert_ne!(n.lattice(0, 0, 0).to_bits(), n.lattice(1, 0, 0).to_bits());
        assert_ne!(n.lattice(0, 0, 0).to_bits(), n.lattice(0, 1, 0).to_bits());
        assert_ne!(n.lattice(0, 0, 0).to_bits(), n.lattice(0, 0, 1).to_bits());
    }

    #[test]
    fn salt_separates_two_fields_on_one_world() {
        let a = Noise::new(12345, 0);
        let b = Noise::new(12345, 0x0C0FFEE);
        assert_ne!(a.lattice(2, 2, 2).to_bits(), b.lattice(2, 2, 2).to_bits());
    }
}
```

- [ ] **Step 2: Run it and confirm it fails**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p worldbuilder-engine noise
```

Expected: FAIL — `cannot find type Noise`.

- [ ] **Step 3: Write the implementation**

Insert above the test module:

```rust
//! Deterministic value noise, sampled in three dimensions on the sphere.
//!
//! Ported from `worldbuilder/terrain/noise.py`. Three dimensions rather than two because a
//! two-dimensional field cannot be wrapped onto a sphere without a seam down one meridian
//! and a pinch at each pole; sampling a volume at the point's own position has neither.
//!
//! Every lattice value is an integer hash of its own coordinates and the seed, so it
//! depends on nothing but that point — there is no generator whose position could matter
//! and no order that could change an answer.
//!
//! **The Python memoises the eight corners of each cell; this does not.** That cache exists
//! because a Python-level call costs more than the arithmetic it avoids — the Python's own
//! comment records 2.9 million calls in one chart redraw, where call overhead was twice the
//! cost of the dictionary lookup. Rust has no such overhead. Dropping the cache returns
//! exactly the same values (it memoises a pure function of three integers and a seed), and
//! it buys a `Noise` that is immutable, `Sync`, and free of interior mutability — which the
//! WebAssembly build and any future parallel bake both want.

use crate::detmath as m;

const SCALE: f64 = 18_446_744_073_709_551_616.0; // 2^64, exactly representable

#[derive(Debug, Clone, Copy)]
pub struct Noise {
    seed: u64,
}

impl Noise {
    /// Salted so that two fields on the same world — continentality here, roughness later —
    /// are independent rather than the same shape at different amplitudes.
    ///
    /// The Python leaves this product unmasked and masks inside the lattice hash instead.
    /// Wrapping here is equivalent: multiplication and XOR both commute with truncation
    /// mod 2^64, so masking once at the end is the same as masking throughout. Verified
    /// against the Python across seeds including 2^63.
    pub fn new(seed: u64, salt: u64) -> Self {
        Self {
            seed: seed
                .wrapping_mul(0x0000_0001_0000_01B3)
                ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15),
        }
    }

    /// An integer avalanche rather than a cryptographic digest. A real digest would be just
    /// as deterministic and about thirty times slower, and this is called eight times per
    /// octave per sample.
    fn lattice(&self, ix: i64, iy: i64, iz: i64) -> f64 {
        let h = (ix as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (iy as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
            ^ (iz as u64).wrapping_mul(0x1656_67B1_9E37_79F9);
        let mut h = h ^ self.seed.wrapping_mul(0x27D4_EB2F_1656_67C5);
        h ^= h >> 33;
        h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
        h ^= h >> 33;
        h = h.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
        h ^= h >> 33;
        h as f64 / SCALE
    }
}
```

Note `0x0000_0001_0000_01B3` is `0x100000001B3` written with separators; confirm the value matches the Python before moving on.

- [ ] **Step 4: Run and confirm the tests pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p worldbuilder-engine
```

Expected: PASS, including the existing crate tests and both guard tests.

- [ ] **Step 5: Commit**

```bash
git add crates/worldbuilder-engine/src/noise.rs crates/worldbuilder-engine/src/lib.rs
git commit -m "engine: the lattice hash, without the cache Python needed"
```

---

### Task 2: Trilinear sampling

**Files:**
- Modify: `crates/worldbuilder-engine/src/noise.rs`

**Interfaces:**
- Produces: `Noise::at(&self, x: f64, y: f64, z: f64) -> f64`.

The Python is written flat rather than tidily, because it is called about forty times per terrain sample and several million times per chart. Transcribe that shape exactly — every intermediate, in the same order. A "cleaner" nested-loop interpolation is a different sum and will fail conformance.

- [ ] **Step 1: Write the failing test**

Append to the test module in `noise.rs`:

```rust
    #[test]
    fn sampling_is_continuous_across_a_cell_boundary() {
        let n = Noise::new(12345, 0x0C0FFEE);
        let just_below = n.at(0.999_999_999, 0.3, 0.3);
        let just_above = n.at(1.000_000_001, 0.3, 0.3);
        assert!((just_below - just_above).abs() < 1e-6, "{} vs {}", just_below, just_above);
    }

    #[test]
    fn sampling_at_a_lattice_point_returns_that_corner() {
        let n = Noise::new(12345, 0x0C0FFEE);
        assert_eq!(n.at(2.0, 3.0, 4.0).to_bits(), n.lattice(2, 3, 4).to_bits());
    }

    #[test]
    fn sampling_stays_in_the_unit_interval() {
        let n = Noise::new(12345, 0x0C0FFEE);
        for i in 0..1000 {
            let t = i as f64 * 0.0137;
            let v = n.at(t, -t * 0.5, t * 0.25);
            assert!((0.0..1.0).contains(&v), "at({}) was {}", t, v);
        }
    }

    #[test]
    fn negative_coordinates_floor_rather_than_truncate() {
        // The trap this port exists to avoid. -0.5 lies in cell -1, not cell 0, so a
        // sample just below zero must interpolate from the -1 cell's corners.
        let n = Noise::new(12345, 0x0C0FFEE);
        let below = n.at(-0.000_000_001, 0.5, 0.5);
        let above = n.at(0.000_000_001, 0.5, 0.5);
        assert!((below - above).abs() < 1e-6, "discontinuity at zero: {} vs {}", below, above);
    }
```

- [ ] **Step 2: Run and confirm it fails**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p worldbuilder-engine noise
```

Expected: FAIL — no method `at`.

- [ ] **Step 3: Write the implementation**

Add inside `impl Noise`:

```rust
    /// Trilinear between the eight surrounding lattice values, with each fraction put
    /// through a smoothstep first. Straight linear interpolation would leave visible
    /// creases along every lattice plane — and on terrain a crease is a cliff somebody
    /// sails into.
    ///
    /// Written flat rather than tidily, matching the Python: this is called about forty
    /// times per terrain sample and several million times per chart. It is also transcribed
    /// in exactly the Python's order because floating-point addition is not associative and
    /// this must agree bit-for-bit.
    pub fn at(&self, x: f64, y: f64, z: f64) -> f64 {
        // floor, never a cast: Python uses int(x // 1), which floors toward negative
        // infinity, and every negative coordinate would otherwise land in the wrong cell.
        let fx_floor = m::floor(x);
        let fy_floor = m::floor(y);
        let fz_floor = m::floor(z);
        let ix = fx_floor as i64; // cast-ok: already floored, mirrors Python's int(x // 1)
        let iy = fy_floor as i64; // cast-ok: already floored, mirrors Python's int(y // 1)
        let iz = fz_floor as i64; // cast-ok: already floored, mirrors Python's int(z // 1)

        let fx = x - fx_floor;
        let fy = y - fy_floor;
        let fz = z - fz_floor;

        let ux = fx * fx * (3.0 - 2.0 * fx);
        let uy = fy * fy * (3.0 - 2.0 * fy);
        let uz = fz * fz * (3.0 - 2.0 * fz);

        let (jx, jy, jz) = (ix + 1, iy + 1, iz + 1);
        let c000 = self.lattice(ix, iy, iz);
        let c100 = self.lattice(jx, iy, iz);
        let c010 = self.lattice(ix, jy, iz);
        let c110 = self.lattice(jx, jy, iz);
        let c001 = self.lattice(ix, iy, jz);
        let c101 = self.lattice(jx, iy, jz);
        let c011 = self.lattice(ix, jy, jz);
        let c111 = self.lattice(jx, jy, jz);

        let x00 = c000 + (c100 - c000) * ux;
        let x10 = c010 + (c110 - c010) * ux;
        let x01 = c001 + (c101 - c001) * ux;
        let x11 = c011 + (c111 - c011) * ux;
        let y0 = x00 + (x10 - x00) * uy;
        let y1 = x01 + (x11 - x01) * uy;
        y0 + (y1 - y0) * uz
    }
```

Note: the Python computes `fx = x - ix` where `ix` is the integer. Using `x - fx_floor` is the same value — the float that was floored — and avoids an int-to-float round trip. Confirm this against the conformance harness in Task 4 rather than assuming; if it diverges, use `x - (ix as f64)` instead.

- [ ] **Step 4: Run and confirm the tests pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p worldbuilder-engine
```

- [ ] **Step 5: Commit**

```bash
git add crates/worldbuilder-engine/src/noise.rs
git commit -m "engine: trilinear sampling, flat and in the Python's order"
```

---

### Task 3: Fractal Brownian motion

**Files:**
- Modify: `crates/worldbuilder-engine/src/noise.rs`

**Interfaces:**
- Produces: `Noise::fbm(&self, x: f64, y: f64, z: f64, frequency: f64, octaves: u32, gain: f64, lacunarity: f64) -> f64`.

The Python takes a `SpherePoint` and reads `.vector`; the Rust takes the three components directly, so that `noise.rs` does not depend on `sphere.rs`. The arithmetic is identical.

The accumulation order is load-bearing: `total`, `amplitude` and `frequency` update in a specific sequence inside the loop, and the final expression divides by `loudest`. Transcribe it exactly, including the guard for `loudest == 0` when `octaves` is zero.

- [ ] **Step 1: Write the failing test**

Append to the test module:

```rust
    #[test]
    fn zero_octaves_is_silent() {
        let n = Noise::new(12345, 0x0C0FFEE);
        assert_eq!(n.fbm(0.3, 0.4, 0.5, 1.25, 0, 0.5, 2.0).to_bits(), 0.0f64.to_bits());
    }

    #[test]
    fn one_octave_is_the_sample_recentred() {
        // With a single octave, loudest is 1.0 and the result is 2 * (at(..) - 0.5).
        let n = Noise::new(12345, 0x0C0FFEE);
        let expected = 2.0 * (n.at(0.3 * 1.25, 0.4 * 1.25, 0.5 * 1.25) - 0.5);
        assert_eq!(n.fbm(0.3, 0.4, 0.5, 1.25, 1, 0.5, 2.0).to_bits(), expected.to_bits());
    }

    #[test]
    fn more_octaves_stay_centred_near_zero() {
        let n = Noise::new(12345, 0x0C0FFEE);
        for i in 0..500 {
            let t = i as f64 * 0.021;
            let v = n.fbm(t, -t, t * 0.5, 1.25, 4, 0.5, 2.0);
            assert!((-1.5..1.5).contains(&v), "fbm at {} was {}", t, v);
        }
    }
```

- [ ] **Step 2: Run and confirm it fails**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p worldbuilder-engine noise
```

- [ ] **Step 3: Write the implementation**

Add inside `impl Noise`:

```rust
    /// Several octaves summed, each half the amplitude and twice the frequency of the last.
    ///
    /// The octave count is a parameter rather than a constant because a chart drawn at
    /// twenty-two miles has samples four hundred metres apart, and octaves finer than that
    /// are invisible — they cost time to produce detail below the resolution being drawn,
    /// and they alias while doing it. The caller decides.
    ///
    /// The loop's update order is transcribed from the Python and must not be rearranged:
    /// the sum is order-dependent and this has to agree bit-for-bit.
    pub fn fbm(
        &self,
        x: f64,
        y: f64,
        z: f64,
        frequency: f64,
        octaves: u32,
        gain: f64,
        lacunarity: f64,
    ) -> f64 {
        let mut total = 0.0f64;
        let mut amplitude = 1.0f64;
        let mut loudest = 0.0f64;
        let mut frequency = frequency;
        for _ in 0..octaves {
            total += (self.at(x * frequency, y * frequency, z * frequency) - 0.5) * amplitude;
            loudest += amplitude;
            amplitude *= gain;
            frequency *= lacunarity;
        }
        if loudest == 0.0 {
            0.0
        } else {
            2.0 * total / loudest
        }
    }
```

- [ ] **Step 4: Run and confirm the tests pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p worldbuilder-engine
```

- [ ] **Step 5: Commit**

```bash
git add crates/worldbuilder-engine/src/noise.rs
git commit -m "engine: fbm, with the loop order the sum depends on"
```

---

### Task 4: Bindings and conformance

**Files:**
- Modify: `crates/worldbuilder-engine/src/bindings.rs`
- Modify: `crates/worldbuilder-engine/src/lib.rs`
- Modify: `tests/test_conformance.py`

**Interfaces:**
- Produces, on the Python side: `noise_at(seed, salt, x, y, z) -> f64` and `noise_fbm(seed, salt, x, y, z, frequency, octaves, gain, lacunarity) -> f64`.

Constructing a `Noise` per call is deliberate: it is two integer operations, the harness needs to vary the seed freely, and it keeps the binding stateless.

- [ ] **Step 1: Add the bindings**

Append to `bindings.rs`:

```rust
#[pyfunction]
pub fn noise_at(seed: u64, salt: u64, x: f64, y: f64, z: f64) -> f64 {
    crate::noise::Noise::new(seed, salt).at(x, y, z)
}

#[pyfunction]
#[allow(clippy::too_many_arguments)]
pub fn noise_fbm(
    seed: u64,
    salt: u64,
    x: f64,
    y: f64,
    z: f64,
    frequency: f64,
    octaves: u32,
    gain: f64,
    lacunarity: f64,
) -> f64 {
    crate::noise::Noise::new(seed, salt).fbm(x, y, z, frequency, octaves, gain, lacunarity)
}
```

Register both in the `#[pymodule]` alongside the existing entries.

- [ ] **Step 2: Rebuild the extension**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cd crates/worldbuilder-engine && python -m maturin develop --release
```

Use the project venv. Confirm it installs there and not into `D:\dev\.venv`.

- [ ] **Step 3: Add the conformance tests**

Append to `tests/test_conformance.py`:

```python
# ---------------------------------------------------------------------------
# Noise
#
# Unlike the sphere functions, Noise contains NO transcendentals: a 64-bit integer
# hash, a floor, and pure arithmetic. It therefore falls entirely under the strict
# contract -- every one of these comparisons is bit-for-bit, with no ULP bound
# anywhere. If one of them ever needs loosening, something is wrong with the port,
# not with the standard.
# ---------------------------------------------------------------------------

from worldbuilder.terrain.noise import Noise as PyNoise

NOISE_SEED = 12345
NOISE_SALT = 0x0C0FFEE


def noise_points(count=5000):
    """Hashed sample positions, including negatives so the floor path is exercised."""
    state = 0x9E3779B97F4A7C15
    mask = (1 << 64) - 1
    yield (0.0, 0.0, 0.0)
    yield (-0.0, -0.0, -0.0)
    yield (1.0, 2.0, 3.0)
    yield (-1.0, -2.0, -3.0)
    yield (-0.000000001, 0.5, 0.5)
    for _ in range(count):
        comps = []
        for _ in range(3):
            state = (state * 6364136223846793005 + 1442695040888963407) & mask
            h = state ^ (state >> 29)
            comps.append(((h >> 11) / float(1 << 53)) * 20.0 - 10.0)
        yield tuple(comps)


def test_noise_at_agrees_exactly():
    py = PyNoise(NOISE_SEED, salt=NOISE_SALT)
    for x, y, z in noise_points():
        want = py.at(x, y, z)
        got = engine.noise_at(NOISE_SEED, NOISE_SALT, x, y, z)
        assert same(want, got), f"at({x}, {y}, {z}): {want!r} vs {got!r}"


def test_noise_at_agrees_on_negative_coordinates():
    """
    The floor-versus-truncate trap, exercised deliberately.

    Python derives its lattice cell with int(x // 1), which floors toward negative
    infinity; a Rust `as i64` truncates toward zero. On any negative coordinate those
    select different cells, and the resulting world would differ everywhere south and
    west of the origin with nothing raised and no test failing -- unless this one does.
    """
    py = PyNoise(NOISE_SEED, salt=NOISE_SALT)
    for i in range(2000):
        t = -0.0005 * i
        want = py.at(t, t * 0.5, t * 0.25)
        got = engine.noise_at(NOISE_SEED, NOISE_SALT, t, t * 0.5, t * 0.25)
        assert same(want, got), f"at({t}): {want!r} vs {got!r}"


def test_noise_fbm_agrees_exactly():
    py = PyNoise(NOISE_SEED, salt=NOISE_SALT)

    class _Point:
        """The Python fbm takes a SpherePoint and reads .vector; this is the smallest
        thing that satisfies it without dragging in normalisation."""

        def __init__(self, x, y, z):
            self.vector = Vec3(x, y, z)

    for x, y, z in noise_points(1500):
        for octaves in (0, 1, 4, 8):
            want = py.fbm(_Point(x, y, z), 1.25, octaves)
            got = engine.noise_fbm(NOISE_SEED, NOISE_SALT, x, y, z, 1.25, octaves, 0.5, 2.0)
            assert same(want, got), f"fbm({x},{y},{z},oct={octaves}): {want!r} vs {got!r}"


def test_noise_seed_and_salt_agree():
    """Different worlds and different fields on one world must both track the Python."""
    for seed in (0, 1, 12345, 2**31, 2**63):
        for salt in (0, NOISE_SALT):
            py = PyNoise(seed, salt=salt)
            for x, y, z in noise_points(200):
                want = py.at(x, y, z)
                got = engine.noise_at(seed, salt, x, y, z)
                assert same(want, got), f"seed={seed} salt={salt} at({x},{y},{z})"
```

- [ ] **Step 4: Run the conformance suite**

```bash
cd /d/dev/worldbuilder_by_aetos
PYTHONPATH=/d/dev/worldbuilder_by_aetos .venv/Scripts/python -m pytest tests/test_conformance.py -v
```

**Every one of these must pass under `same()`, exactly.** If any fails, that is a real finding and the likely causes, in order, are: `fx` computed differently from the Python (see Task 2 Step 3's note — try `x - (ix as f64)`), a reordered interpolation, or a wrong hash constant. Report the divergence with its inputs and both bit patterns. Do NOT introduce a ULP bound here — `Noise` has no transcendental in it, so there is nothing for a bound to excuse.

- [ ] **Step 5: Run the whole suite**

```bash
PYTHONPATH=/d/dev/worldbuilder_by_aetos .venv/Scripts/python -m pytest tests/ -q
```

Expected: the pre-existing 240 tests still pass, plus the slice-1a conformance tests, plus these.

- [ ] **Step 6: Commit**

```bash
git add crates/worldbuilder-engine/src/bindings.rs crates/worldbuilder-engine/src/lib.rs tests/test_conformance.py
git commit -m "engine: noise bindings, and conformance with no bound to hide behind"
```

---

### Task 5: Record it

**Files:**
- Modify: `crates/worldbuilder-engine/README.md`

- [ ] **Step 1: Update the README**

Add `src/noise.rs` to the "What is here so far" listing, and add a short paragraph to the conformance section recording that `Noise` is held to the strict contract in full — no ULP bound — because it contains no transcendentals, and stating how many comparisons the noise tests make.

Also record the cache decision: the Python memoises each cell's eight corners because a Python-level call costs more than the arithmetic it avoids; the Rust does not, because it returns identical values without needing to, and dropping it makes `Noise` immutable and `Sync`.

- [ ] **Step 2: Commit**

```bash
git add crates/worldbuilder-engine/README.md
git commit -m "engine: record that Noise is held strictly, and why the cache is gone"
```

---

## What this slice deliberately does not do

- **No `Continentality`.** It sits on `Noise` and adds a calibration pass that samples 4,000 points on a Fibonacci spiral and takes a quantile — worth its own slice and its own conformance work.
- **No deletion of `worldbuilder/terrain/noise.py`.** The Python stays the reference until every module has a passing conformance test.
- **No WASM build.** Still slice 2's.
