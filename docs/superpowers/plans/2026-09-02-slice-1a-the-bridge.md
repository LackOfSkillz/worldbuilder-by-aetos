# Slice 1a — The Bridge — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up the Rust engine crate, make Python able to call it, and prove by measurement that the Rust implementation of the geometry layer agrees with the existing Python one bit-for-bit.

**Architecture:** A `crates/worldbuilder-engine` Rust library, built as a Python extension module with PyO3 and maturin, living in the same repository as the Python it will eventually replace. All transcendental maths routes through one `detmath` module backed by the pure-Rust `libm` crate — the rule slice 0 established by measurement. The first module ported is `geometry`, the smallest real one, and a differential harness samples both implementations over a corpus and compares raw `f64` bits. Nothing is deleted from Python in this slice.

**Tech Stack:** Rust (stable 1.98.0, already installed), `libm` 0.2.11, PyO3, maturin, Python 3.11, pytest.

**Spec:** `docs/design/2026-09-02-mark-2-world-studio.md` — sections 4.2 (DETERMINISM-001), 4.3 (dependency policy), 4.4 (where the code lives), 20 (build order).

**Prior slice:** `docs/superpowers/plans/2026-09-02-slice-0-bit-equality-spike.md` and its result in `spikes/0-bit-equality/README.md`. Slice 0 measured that native and WASM agree bit-for-bit over 5,000,000 samples, with a negative control proving the comparison could detect a one-bit difference. That is why this slice may assume the two-target architecture works. **The spike crate is throwaway and must not be imported, extended, or moved into `crates/`.** Write the engine fresh.

## Why this is 1a and not slice 1

Slice 1 as specified — the whole field, the parameter surface, Python bindings and the planet-scale provider — is roughly 5,000 lines of Python across eight modules with 240 tests behind it. That is not one plan. This is the first of several sub-slices, and it is deliberately the *bridge* rather than a module port: when it is done, every subsequent module can be ported and immediately checked against its Python original. Getting that check working is worth more than getting any particular module ported.

## Global Constraints

- **Rust edition 2021, stable toolchain.** No nightly. Rust is already installed at `~/.cargo/bin` and is NOT on PATH in a fresh shell — every bash invocation must begin with `export PATH="$HOME/.cargo/bin:$PATH"`.
- **No `std` transcendental functions in the engine crate.** Not `f64::sin`, `cos`, `sqrt`, `hypot`, `atan2`, `asin`, `tanh`, `powf`, `floor`. All route through `detmath`, backed by `libm`. This is DETERMINISM-001 and slice 0 measured why it matters: std `sin` against `libm::sin` diverged on 2,441 of 100,000 samples by a single bit.
- **`floor`, not `as i64`.** `worldbuilder/terrain/noise.py` derives lattice cells with `int(x // 1)`, which floors toward negative infinity. Rust's `as i64` truncates toward zero. For any negative coordinate — half the sphere — they select different cells: `-2.3` floors to `-3`, truncates to `-2`. This is recorded because it was found the hard way; see `spikes/0-bit-equality/README.md`.
- **Engine dependencies stay minimal and deterministic:** `libm` (pinned `=0.2.11`) and `pyo3`. Nothing that reaches platform libm behind our back, no GPU, no SIMD intrinsics, no fast-math flags, no `-C target-feature=+simd128`.
- **`[profile.release]` in the engine crate:** `opt-level = 2`, `lto = false`, `codegen-units = 1`, matching slice 0. Do NOT set `panic = "abort"` here — that broke `cargo test --release` in the spike, and this crate is a library that will be tested.
- **Nothing in `worldbuilder/` is deleted or rewritten in this slice.** The Python remains the reference implementation and keeps passing its own tests untouched.
- **Comparison is on raw bits** — `f64::to_bits()` against Python's `struct.pack('<d', ...)` — never on decimal text and never with a tolerance.
- **Python reference values, established by reading the source:**
  - `EARTH_RADIUS_M = 6_371_000.0`
  - `NORTH_AXIS = Vec3(0.0, 0.0, 1.0)`, `POLAR_FALLBACK = Vec3(1.0, 0.0, 0.0)`, `DEGENERATE = 1e-9`
  - `Vec3.length()` is `math.sqrt(self.dot(self))`
  - `Vec3.normalised()` raises `ValueError` on a zero vector
  - `SpherePoint.to_latlon()` clamps z to [-1, 1] before `asin`, and returns longitude from `atan2(y, x)`
  - `SpherePoint.angle_to()` is `atan2(cross.length(), dot)` — NOT `acos(dot)`

---

## File Structure

    crates/worldbuilder-engine/
      Cargo.toml            the engine crate; libm and pyo3 pinned
      src/lib.rs            the PyO3 module; re-exports, no logic
      src/detmath.rs        the only place a transcendental is called
      src/vectors.rs        Vec3
      src/sphere.rs         SpherePoint
      src/bindings.rs       PyO3 wrapper types; conversion only, no maths
    Cargo.toml              workspace manifest
    pyproject.toml          gains a maturin build path for the extension
    tests/test_conformance.py   the differential harness

`detmath.rs` is a separate file so a future CI lint can police a path rather than a symbol. `bindings.rs` is separate so that the maths files contain no PyO3 types — the engine must remain usable from a plain Rust or WASM build with no Python anywhere.

---

### Task 1: The workspace, the crate, and an import that works

**Files:**
- Create: `Cargo.toml` (workspace root)
- Create: `crates/worldbuilder-engine/Cargo.toml`
- Create: `crates/worldbuilder-engine/src/lib.rs`
- Modify: `.gitignore`

**Interfaces:**
- Produces: a Python-importable module `worldbuilder_engine` exposing `version() -> str`.

- [ ] **Step 1: Write the workspace manifest**

Create `Cargo.toml` at the repository root:

```toml
[workspace]
members = ["crates/worldbuilder-engine"]
resolver = "2"

# The spike under spikes/0-bit-equality is deliberately NOT a workspace member.
# It is throwaway, it is frozen, and it must not be built or tested with the engine.
exclude = ["spikes/0-bit-equality"]
```

- [ ] **Step 2: Write the engine crate manifest**

Create `crates/worldbuilder-engine/Cargo.toml`:

```toml
[package]
name = "worldbuilder-engine"
version = "0.0.1"
edition = "2021"
publish = false

[lib]
name = "worldbuilder_engine"
crate-type = ["cdylib", "rlib"]

[dependencies]
libm = "=0.2.11"
pyo3 = { version = "0.22", features = ["extension-module"] }

[profile.release]
# Determinism first. Matches the slice 0 spike, minus panic=abort, which this
# crate must not carry because it breaks `cargo test --release` on a library.
opt-level = 2
lto = false
codegen-units = 1
```

- [ ] **Step 3: Write a minimal PyO3 module**

Create `crates/worldbuilder-engine/src/lib.rs`:

```rust
//! The Worldbuilder generator core.
//!
//! One implementation, compiled twice: natively for Evennia and maritime through Python
//! bindings, and to WebAssembly for the browser studio. Slice 0 measured that those two
//! targets agree bit-for-bit, which is the only reason a studio and a game can be trusted
//! to be looking at the same world.

use pyo3::prelude::*;

/// The engine's own version, so a caller can tell which core answered.
#[pyfunction]
fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[pymodule]
fn worldbuilder_engine(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(version, m)?)?;
    Ok(())
}
```

- [ ] **Step 4: Ignore Rust build output**

Append to `.gitignore`:

```
/target/
crates/*/target/
```

- [ ] **Step 5: Confirm it compiles**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo build --release -p worldbuilder-engine
```

Expected: compiles. A linker error mentioning Python symbols at this stage means the `extension-module` feature is doing its job; if the build fails for that reason, it will still succeed under maturin in Step 7.

- [ ] **Step 6: Install maturin**

```bash
python -m pip install maturin
python -m maturin --version
```

Expected: a version string.

- [ ] **Step 7: Build and install the extension into the current Python**

```bash
cd crates/worldbuilder-engine
python -m maturin develop --release
```

Expected: it builds and installs `worldbuilder_engine` into the active environment.

- [ ] **Step 8: Prove Python can call Rust**

```bash
python -c "import worldbuilder_engine as e; print(e.version())"
```

Expected: `0.0.1`

- [ ] **Step 9: Commit**

```bash
git add Cargo.toml crates .gitignore
git commit -m "engine: a Rust crate Python can call, and nothing else yet"
```

---

### Task 2: detmath, and the guard that keeps it honest

**Files:**
- Create: `crates/worldbuilder-engine/src/detmath.rs`
- Modify: `crates/worldbuilder-engine/src/lib.rs` (add `pub mod detmath;`)

**Interfaces:**
- Produces: `detmath::{sin, cos, sqrt, hypot, atan2, asin, tanh, powf, floor, to_radians, to_degrees}`.

- [ ] **Step 1: Write the failing test**

Create `crates/worldbuilder-engine/src/detmath.rs` containing only this test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_op_is_routed_and_finite() {
        assert!(sin(0.7).is_finite());
        assert!(cos(0.7).is_finite());
        assert!(sqrt(2.0).is_finite());
        assert!(hypot(3.0, 4.0).is_finite());
        assert!(atan2(1.0, 2.0).is_finite());
        assert!(asin(0.5).is_finite());
        assert!(tanh(0.5).is_finite());
        assert!(powf(2.0, 0.5).is_finite());
        assert!(floor(-2.3).is_finite());
    }

    #[test]
    fn floor_goes_down_not_towards_zero() {
        // The trap this module exists to close. Python's int(x // 1) floors;
        // Rust's `as i64` truncates. For negative coordinates they disagree.
        assert_eq!(floor(-2.3), -3.0);
        assert_eq!(floor(-1e-9), -1.0);
        assert_eq!(floor(-1.0), -1.0);
        assert_eq!(floor(2.3), 2.0);
    }

    #[test]
    fn degrees_and_radians_round_trip_exactly_at_the_landmarks() {
        assert_eq!(to_radians(180.0).to_bits(), std::f64::consts::PI.to_bits());
        assert_eq!(to_degrees(std::f64::consts::PI).to_bits(), 180.0f64.to_bits());
    }
}
```

- [ ] **Step 2: Run it and confirm it fails**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p worldbuilder-engine detmath
```

Expected: FAIL — `cannot find function` for each name.

- [ ] **Step 3: Write the implementation**

Insert above the test module in `crates/worldbuilder-engine/src/detmath.rs`:

```rust
//! The only place in this crate that may call a transcendental function.
//!
//! std's maths dispatches to the platform's libm, and the platform differs between a
//! native host and a WASM runtime. Slice 0 measured the consequence: native `f64::sin`
//! against `libm::sin` in WASM diverged on 2,441 of 100,000 samples, each by a single
//! bit. Coastlines are decided by last bits, so every call goes through here.
//!
//! `sqrt` is routed even though IEEE-754 requires it to be correctly rounded, so that the
//! rule is "no std maths, ever" rather than "no std maths except the ones somebody judged
//! safe" — a rule with an exception list is a rule that erodes.

/// Radians per degree, and degrees per radian, as explicit constants rather than std's
/// `to_radians`/`to_degrees`, so the conversion is visible and identical on both targets.
const RAD_PER_DEG: f64 = std::f64::consts::PI / 180.0;
const DEG_PER_RAD: f64 = 180.0 / std::f64::consts::PI;

pub fn sin(x: f64) -> f64 {
    libm::sin(x)
}

pub fn cos(x: f64) -> f64 {
    libm::cos(x)
}

pub fn sqrt(x: f64) -> f64 {
    libm::sqrt(x)
}

pub fn hypot(x: f64, y: f64) -> f64 {
    libm::hypot(x, y)
}

pub fn atan2(y: f64, x: f64) -> f64 {
    libm::atan2(y, x)
}

pub fn asin(x: f64) -> f64 {
    libm::asin(x)
}

pub fn tanh(x: f64) -> f64 {
    libm::tanh(x)
}

pub fn powf(x: f64, y: f64) -> f64 {
    libm::pow(x, y)
}

/// Floors toward negative infinity, which is what Python's `int(x // 1)` does and what
/// `as i64` does NOT do. Never derive a lattice coordinate with a cast.
pub fn floor(x: f64) -> f64 {
    libm::floor(x)
}

pub fn to_radians(degrees: f64) -> f64 {
    degrees * RAD_PER_DEG
}

pub fn to_degrees(radians: f64) -> f64 {
    radians * DEG_PER_RAD
}
```

- [ ] **Step 4: Register the module**

In `crates/worldbuilder-engine/src/lib.rs`, add below the doc comment:

```rust
pub mod detmath;
```

- [ ] **Step 5: Run the tests and confirm they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p worldbuilder-engine detmath
```

Expected: PASS, 3 tests.

- [ ] **Step 6: Add the static guard**

Create `crates/worldbuilder-engine/tests/no_std_math.rs`:

```rust
//! DETERMINISM-001's static guard. A rule in a document holds for a year and then quietly
//! stops; this fails the build instead.

use std::fs;
use std::path::Path;

const BANNED: &[&str] = &[
    ".sin()", ".cos()", ".sqrt()", ".hypot(", ".atan2(", ".asin(", ".tanh(",
    ".powf(", ".powi(", ".floor()", ".ceil()", ".round()", ".exp()", ".ln()",
    ".to_radians()", ".to_degrees()",
];

#[test]
fn no_std_float_maths_outside_detmath() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offences = Vec::new();

    for entry in fs::read_dir(&src).expect("read src/") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        if path.file_name().and_then(|n| n.to_str()) == Some("detmath.rs") {
            continue;
        }
        let text = fs::read_to_string(&path).expect("read source file");
        for (lineno, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            for needle in BANNED {
                if line.contains(needle) {
                    offences.push(format!(
                        "{}:{}: {} — route it through detmath",
                        path.display(),
                        lineno + 1,
                        needle
                    ));
                }
            }
        }
    }

    assert!(
        offences.is_empty(),
        "std float maths found outside detmath:\n{}",
        offences.join("\n")
    );
}
```

- [ ] **Step 7: Confirm the guard passes, and that it can fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p worldbuilder-engine --test no_std_math
```

Expected: PASS.

Now prove the guard works. Temporarily add this line inside `lib.rs`'s `version` function:

```rust
    let _ = (2.0f64).sqrt();
```

Re-run the guard. Expected: FAIL, naming `lib.rs` and `.sqrt()`. Then remove the line and confirm it passes again. **A guard that has never been seen to fail is not a guard.**

- [ ] **Step 8: Commit**

```bash
git add crates/worldbuilder-engine/src/detmath.rs crates/worldbuilder-engine/src/lib.rs crates/worldbuilder-engine/tests/no_std_math.rs
git commit -m "engine: one door for maths, a floor that floors, and a guard that fails"
```

---

### Task 3: Vec3

**Files:**
- Create: `crates/worldbuilder-engine/src/vectors.rs`
- Modify: `crates/worldbuilder-engine/src/lib.rs` (add `pub mod vectors;`)

**Interfaces:**
- Consumes: `detmath::sqrt`.
- Produces: `vectors::Vec3` with public fields `x`, `y`, `z` (f64) and methods `add`, `sub`, `scaled`, `dot`, `cross`, `length`, `normalised`; plus consts `NORTH_AXIS`, `POLAR_FALLBACK`, `DEGENERATE`.

The Python original is `worldbuilder/geometry/vectors.py`. Port the arithmetic exactly — same operand order in `cross`, same `sqrt(dot(self))` for length — because floating-point addition is not associative and a reordered cross product is a different number.

- [ ] **Step 1: Write the failing test**

Create `crates/worldbuilder-engine/src/vectors.rs` with only this test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cross_follows_the_right_hand_rule() {
        let x = Vec3::new(1.0, 0.0, 0.0);
        let y = Vec3::new(0.0, 1.0, 0.0);
        let z = x.cross(&y);
        assert_eq!(z.x.to_bits(), 0.0f64.to_bits());
        assert_eq!(z.y.to_bits(), 0.0f64.to_bits());
        assert_eq!(z.z.to_bits(), 1.0f64.to_bits());
    }

    #[test]
    fn length_of_three_four_zero_is_exactly_five() {
        assert_eq!(Vec3::new(3.0, 4.0, 0.0).length().to_bits(), 5.0f64.to_bits());
    }

    #[test]
    fn normalised_preserves_direction_and_sets_length_one() {
        let unit = Vec3::new(0.0, 0.0, 7.0).normalised().expect("non-zero");
        assert_eq!(unit.z.to_bits(), 1.0f64.to_bits());
    }

    #[test]
    fn a_zero_vector_has_no_direction() {
        assert!(Vec3::new(0.0, 0.0, 0.0).normalised().is_none());
    }

    #[test]
    fn add_and_sub_are_componentwise() {
        let a = Vec3::new(1.0, 2.0, 3.0);
        let b = Vec3::new(0.5, 0.5, 0.5);
        assert_eq!(a.add(&b).x.to_bits(), 1.5f64.to_bits());
        assert_eq!(a.sub(&b).z.to_bits(), 2.5f64.to_bits());
    }
}
```

- [ ] **Step 2: Run and confirm it fails**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p worldbuilder-engine vectors
```

Expected: FAIL — `cannot find type Vec3`.

- [ ] **Step 3: Write the implementation**

Insert above the test module:

```rust
//! Vectors in the frame the whole planet is expressed in.
//!
//! Ported from `worldbuilder/geometry/vectors.py`. The arithmetic is transcribed rather
//! than rederived: floating-point addition is not associative, so a cross product written
//! in a different order is a different number, and this must agree with the Python it
//! replaces bit-for-bit.

use crate::detmath as m;

/// x towards longitude zero on the equator, y towards ninety east, z towards the pole.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub fn add(&self, other: &Vec3) -> Vec3 {
        Vec3::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }

    pub fn sub(&self, other: &Vec3) -> Vec3 {
        Vec3::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }

    pub fn scaled(&self, factor: f64) -> Vec3 {
        Vec3::new(self.x * factor, self.y * factor, self.z * factor)
    }

    pub fn dot(&self, other: &Vec3) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    pub fn cross(&self, other: &Vec3) -> Vec3 {
        Vec3::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    pub fn length(&self) -> f64 {
        m::sqrt(self.dot(self))
    }

    /// `None` where Python raises `ValueError`: a zero vector has no direction to keep.
    pub fn normalised(&self) -> Option<Vec3> {
        let magnitude = self.length();
        if magnitude == 0.0 {
            return None;
        }
        Some(self.scaled(1.0 / magnitude))
    }
}

/// The axis the planet turns about, and so the direction of the north pole.
pub const NORTH_AXIS: Vec3 = Vec3::new(0.0, 0.0, 1.0);

/// What to build a frame from at a pole, where east has no meaning. Which direction is
/// chosen does not matter; that the same one is chosen every time does.
pub const POLAR_FALLBACK: Vec3 = Vec3::new(1.0, 0.0, 0.0);

/// How nearly parallel two vectors may be before their cross product stops being a
/// trustworthy direction.
pub const DEGENERATE: f64 = 1e-9;
```

- [ ] **Step 4: Register the module**

In `lib.rs`, add `pub mod vectors;` beside `pub mod detmath;`.

- [ ] **Step 5: Run and confirm the tests pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p worldbuilder-engine
```

Expected: PASS — 5 vector tests plus the earlier detmath and guard tests.

- [ ] **Step 6: Commit**

```bash
git add crates/worldbuilder-engine/src/vectors.rs crates/worldbuilder-engine/src/lib.rs
git commit -m "engine: Vec3, transcribed rather than rederived"
```

---

### Task 4: SpherePoint

**Files:**
- Create: `crates/worldbuilder-engine/src/sphere.rs`
- Modify: `crates/worldbuilder-engine/src/lib.rs` (add `pub mod sphere;`)

**Interfaces:**
- Consumes: `vectors::Vec3`, `detmath::{sin, cos, asin, atan2, to_radians, to_degrees}`.
- Produces: `sphere::SpherePoint` with field `vector: Vec3`, constructors `from_vector`, `from_latlon`, and methods `to_latlon`, `angle_to`, `distance_to`; plus `EARTH_RADIUS_M`.

The Python original is `worldbuilder/geometry/sphere.py`. Two details carry weight and must not be "improved":
- `angle_to` uses `atan2(cross.length(), dot)`, never `acos(dot)`. The simpler form loses precision for points close together, which is where a ship spends its whole life.
- `to_latlon` clamps z into [-1, 1] before `asin`, because a unit vector that is a hair over one by rounding would otherwise produce NaN.

- [ ] **Step 1: Write the failing test**

Create `crates/worldbuilder-engine/src/sphere.rs` with only this test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::vectors::Vec3;

    #[test]
    fn latlon_round_trips_at_an_ordinary_place() {
        let point = SpherePoint::from_latlon(51.5, -0.12);
        let (lat, lon) = point.to_latlon();
        assert!((lat - 51.5).abs() < 1e-12, "lat was {}", lat);
        assert!((lon + 0.12).abs() < 1e-12, "lon was {}", lon);
    }

    #[test]
    fn a_pole_reports_zero_longitude_by_convention() {
        let (lat, lon) = SpherePoint::from_latlon(90.0, 137.0).to_latlon();
        assert!((lat - 90.0).abs() < 1e-9, "lat was {}", lat);
        assert_eq!(lon.to_bits(), 0.0f64.to_bits());
    }

    #[test]
    fn longitude_is_periodic_without_normalising_it_first() {
        let a = SpherePoint::from_latlon(10.0, 180.0);
        let b = SpherePoint::from_latlon(10.0, 540.0);
        assert!(a.angle_to(&b) < 1e-12);
    }

    #[test]
    fn a_quarter_turn_subtends_a_right_angle() {
        let equator = SpherePoint::from_latlon(0.0, 0.0);
        let pole = SpherePoint::from_latlon(90.0, 0.0);
        let expected = std::f64::consts::FRAC_PI_2;
        assert!((equator.angle_to(&pole) - expected).abs() < 1e-12);
    }

    #[test]
    fn distance_scales_the_angle_by_the_radius() {
        let a = SpherePoint::from_latlon(0.0, 0.0);
        let b = SpherePoint::from_latlon(0.0, 1.0);
        let expected = a.angle_to(&b) * EARTH_RADIUS_M;
        assert_eq!(a.distance_to(&b, EARTH_RADIUS_M).to_bits(), expected.to_bits());
    }

    #[test]
    fn from_vector_normalises() {
        let point = SpherePoint::from_vector(&Vec3::new(0.0, 0.0, 9.0)).expect("non-zero");
        assert_eq!(point.vector.z.to_bits(), 1.0f64.to_bits());
    }
}
```

- [ ] **Step 2: Run and confirm it fails**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p worldbuilder-engine sphere
```

Expected: FAIL — `cannot find type SpherePoint`.

- [ ] **Step 3: Write the implementation**

Insert above the test module:

```rust
//! A place on the planet, as a unit vector from its centre.
//!
//! Ported from `worldbuilder/geometry/sphere.py`. The radius is deliberately not stored:
//! a point is a *direction*, and how big the planet is belongs to the world rather than
//! to each of the billions of places on it.

use crate::detmath as m;
use crate::vectors::Vec3;

pub const EARTH_RADIUS_M: f64 = 6_371_000.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpherePoint {
    pub vector: Vec3,
}

impl SpherePoint {
    /// `None` where Python raises `ValueError`, for a vector with no direction.
    pub fn from_vector(vector: &Vec3) -> Option<Self> {
        vector.normalised().map(|v| Self { vector: v })
    }

    /// Longitude is not normalised first and does not need to be: sine and cosine are
    /// periodic, so -180, +180 and +540 give the same vector by arithmetic rather than by
    /// a rule somebody has to remember.
    pub fn from_latlon(latitude_deg: f64, longitude_deg: f64) -> Self {
        let latitude = m::to_radians(latitude_deg);
        let longitude = m::to_radians(longitude_deg);
        let cos_lat = m::cos(latitude);
        Self {
            vector: Vec3::new(
                cos_lat * m::cos(longitude),
                cos_lat * m::sin(longitude),
                m::sin(latitude),
            ),
        }
    }

    /// At a pole the longitude returned is zero, which is a convention rather than a
    /// fact: every meridian meets there and none of them is the answer.
    pub fn to_latlon(&self) -> (f64, f64) {
        let clamped = if self.vector.z < -1.0 {
            -1.0
        } else if self.vector.z > 1.0 {
            1.0
        } else {
            self.vector.z
        };
        let latitude = m::to_degrees(m::asin(clamped));
        let longitude = m::to_degrees(m::atan2(self.vector.y, self.vector.x));
        (latitude, longitude)
    }

    /// By arc tangent of the cross and dot products rather than the arc cosine of the dot
    /// alone. The simpler form loses precision for points close together — exactly the
    /// case a ship spends its whole life in.
    pub fn angle_to(&self, other: &SpherePoint) -> f64 {
        let across = self.vector.cross(&other.vector).length();
        let along = self.vector.dot(&other.vector);
        m::atan2(across, along)
    }

    pub fn distance_to(&self, other: &SpherePoint, radius_m: f64) -> f64 {
        self.angle_to(other) * radius_m
    }
}
```

- [ ] **Step 4: Register the module**

In `lib.rs`, add `pub mod sphere;`.

- [ ] **Step 5: Run and confirm the tests pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p worldbuilder-engine
```

Expected: PASS, all tests.

- [ ] **Step 6: Commit**

```bash
git add crates/worldbuilder-engine/src/sphere.rs crates/worldbuilder-engine/src/lib.rs
git commit -m "engine: SpherePoint, with the arc tangent the arc cosine cannot replace"
```

---

### Task 5: The bindings

**Files:**
- Create: `crates/worldbuilder-engine/src/bindings.rs`
- Modify: `crates/worldbuilder-engine/src/lib.rs`

**Interfaces:**
- Produces, on the Python side: `worldbuilder_engine.vec3_length(x, y, z)`, `vec3_cross(ax, ay, az, bx, by, bz) -> (f64, f64, f64)`, `vec3_normalised(x, y, z) -> Optional[tuple]`, `sphere_from_latlon(lat, lon) -> (f64, f64, f64)`, `sphere_to_latlon(x, y, z) -> (f64, f64)`, `sphere_angle_to(ax, ay, az, bx, by, bz) -> f64`, `sphere_distance_to(ax, ay, az, bx, by, bz, radius_m) -> f64`.

Plain functions over floats, not wrapper classes. The harness needs to compare numbers, and every object boundary is somewhere a conversion could quietly round. Classes can come later if a caller wants them.

- [ ] **Step 1: Write the bindings**

Create `crates/worldbuilder-engine/src/bindings.rs`:

```rust
//! PyO3 surface. Conversion only — no arithmetic lives here, so the maths modules stay
//! usable from a plain Rust or WASM build with no Python anywhere in the picture.

use pyo3::prelude::*;

use crate::sphere::SpherePoint;
use crate::vectors::Vec3;

#[pyfunction]
pub fn vec3_length(x: f64, y: f64, z: f64) -> f64 {
    Vec3::new(x, y, z).length()
}

#[pyfunction]
pub fn vec3_cross(ax: f64, ay: f64, az: f64, bx: f64, by: f64, bz: f64) -> (f64, f64, f64) {
    let c = Vec3::new(ax, ay, az).cross(&Vec3::new(bx, by, bz));
    (c.x, c.y, c.z)
}

#[pyfunction]
pub fn vec3_normalised(x: f64, y: f64, z: f64) -> Option<(f64, f64, f64)> {
    Vec3::new(x, y, z).normalised().map(|v| (v.x, v.y, v.z))
}

#[pyfunction]
pub fn sphere_from_latlon(latitude_deg: f64, longitude_deg: f64) -> (f64, f64, f64) {
    let v = SpherePoint::from_latlon(latitude_deg, longitude_deg).vector;
    (v.x, v.y, v.z)
}

#[pyfunction]
pub fn sphere_to_latlon(x: f64, y: f64, z: f64) -> (f64, f64) {
    SpherePoint { vector: Vec3::new(x, y, z) }.to_latlon()
}

#[pyfunction]
pub fn sphere_angle_to(ax: f64, ay: f64, az: f64, bx: f64, by: f64, bz: f64) -> f64 {
    let a = SpherePoint { vector: Vec3::new(ax, ay, az) };
    let b = SpherePoint { vector: Vec3::new(bx, by, bz) };
    a.angle_to(&b)
}

#[pyfunction]
pub fn sphere_distance_to(
    ax: f64, ay: f64, az: f64,
    bx: f64, by: f64, bz: f64,
    radius_m: f64,
) -> f64 {
    let a = SpherePoint { vector: Vec3::new(ax, ay, az) };
    let b = SpherePoint { vector: Vec3::new(bx, by, bz) };
    a.distance_to(&b, radius_m)
}
```

- [ ] **Step 2: Register them in the module**

Replace the `#[pymodule]` block in `lib.rs` with:

```rust
#[pymodule]
fn worldbuilder_engine(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::vec3_length, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::vec3_cross, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::vec3_normalised, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::sphere_from_latlon, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::sphere_to_latlon, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::sphere_angle_to, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::sphere_distance_to, m)?)?;
    Ok(())
}
```

and add `pub mod bindings;` with the other module declarations.

- [ ] **Step 3: Rebuild and confirm the functions are callable**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cd crates/worldbuilder-engine && python -m maturin develop --release
python -c "import worldbuilder_engine as e; print(e.vec3_length(3.0,4.0,0.0)); print(e.sphere_from_latlon(51.5,-0.12))"
```

Expected: `5.0` and a three-tuple of floats.

- [ ] **Step 4: Commit**

```bash
git add crates/worldbuilder-engine/src/bindings.rs crates/worldbuilder-engine/src/lib.rs
git commit -m "engine: a binding surface of plain floats, so nothing rounds at a boundary"
```

---

### Task 6: The differential harness

**Files:**
- Create: `tests/test_conformance.py`

**Interfaces:**
- Consumes: `worldbuilder_engine` (Rust) and `worldbuilder.geometry` (Python).
- Produces: pytest tests asserting bit-for-bit agreement across a corpus.

This is the deliverable the whole sub-slice exists for. Every later module port is checked by adding cases here.

- [ ] **Step 1: Write the harness**

Create `tests/test_conformance.py`:

```python
"""
Bit-for-bit conformance between the Python reference and the Rust engine.

The engine is not a rewrite that should behave similarly. It is a port that must agree
exactly, because a chart is wrong in the same places every voyage and that is what makes
surveying mean anything. Comparison is therefore on raw f64 bit patterns, never with a
tolerance -- a tolerance would let a coastline move by a metre and call it equal.

Skips wholesale if the engine is not built, so the Python suite still runs on a machine
with no Rust.
"""

import math
import struct

import pytest

from worldbuilder.geometry.sphere import EARTH_RADIUS_M, SpherePoint
from worldbuilder.geometry.vectors import Vec3

engine = pytest.importorskip(
    "worldbuilder_engine",
    reason="Rust engine not built; run `maturin develop --release` in crates/worldbuilder-engine",
)


def bits(value):
    """The exact 64-bit pattern of a float, so comparison cannot be fooled by printing."""
    return struct.unpack("<Q", struct.pack("<d", value))[0]


def same(a, b):
    return bits(a) == bits(b)


def corpus(count=20000):
    """
    Deterministic pseudo-random unit-ish vectors plus the awkward places.

    Hashed rather than gridded: a grid samples the same fractional bit patterns over and
    over and would hide a divergence that only appears at an awkward mantissa. The poles
    and the meridian are pinned, because that is where a spherical field breaks first.
    """
    yield (0.0, 0.0, 1.0)
    yield (0.0, 0.0, -1.0)
    yield (1.0, 0.0, 0.0)
    yield (-1.0, 0.0, 0.0)
    yield (0.0, 1.0, 0.0)
    yield (0.0, -1.0, 0.0)

    state = 0x2545F4914F6CDD1D
    mask = (1 << 64) - 1
    for _ in range(count):
        components = []
        for _ in range(3):
            state = (state * 6364136223846793005 + 1442695040888963407) & mask
            h = state ^ (state >> 33)
            components.append((h >> 11) / float(1 << 53) * 2.0 - 1.0)
        x, y, z = components
        if x == 0.0 and y == 0.0 and z == 0.0:
            continue
        yield (x, y, z)


def test_vec3_length_agrees():
    for x, y, z in corpus():
        assert same(Vec3(x, y, z).length(), engine.vec3_length(x, y, z)), (x, y, z)


def test_vec3_cross_agrees():
    points = list(corpus(2000))
    for (ax, ay, az), (bx, by, bz) in zip(points, points[1:]):
        want = Vec3(ax, ay, az).cross(Vec3(bx, by, bz))
        got = engine.vec3_cross(ax, ay, az, bx, by, bz)
        assert (same(want.x, got[0]) and same(want.y, got[1]) and same(want.z, got[2]))


def test_vec3_normalised_agrees():
    for x, y, z in corpus():
        want = Vec3(x, y, z).normalised()
        got = engine.vec3_normalised(x, y, z)
        assert got is not None
        assert same(want.x, got[0]) and same(want.y, got[1]) and same(want.z, got[2])


def test_vec3_normalised_agrees_on_the_zero_vector():
    """Python raises; Rust returns None. Different shapes, same meaning."""
    with pytest.raises(ValueError):
        Vec3(0.0, 0.0, 0.0).normalised()
    assert engine.vec3_normalised(0.0, 0.0, 0.0) is None


def test_sphere_from_latlon_agrees():
    for lat in range(-90, 91, 3):
        for lon in range(-180, 181, 7):
            want = SpherePoint.from_latlon(float(lat), float(lon)).vector
            got = engine.sphere_from_latlon(float(lat), float(lon))
            assert same(want.x, got[0]) and same(want.y, got[1]) and same(want.z, got[2]), (lat, lon)


def test_sphere_to_latlon_agrees():
    for x, y, z in corpus():
        point = SpherePoint(Vec3(x, y, z).normalised())
        want = point.to_latlon()
        got = engine.sphere_to_latlon(point.vector.x, point.vector.y, point.vector.z)
        assert same(want[0], got[0]) and same(want[1], got[1]), (x, y, z)


def test_sphere_angle_and_distance_agree():
    points = list(corpus(2000))
    for (ax, ay, az), (bx, by, bz) in zip(points, points[1:]):
        a = SpherePoint(Vec3(ax, ay, az).normalised())
        b = SpherePoint(Vec3(bx, by, bz).normalised())
        av, bv = a.vector, b.vector
        assert same(
            a.angle_to(b),
            engine.sphere_angle_to(av.x, av.y, av.z, bv.x, bv.y, bv.z),
        )
        assert same(
            a.distance_to(b),
            engine.sphere_distance_to(av.x, av.y, av.z, bv.x, bv.y, bv.z, EARTH_RADIUS_M),
        )


def test_the_harness_can_actually_fail():
    """
    A conformance suite that cannot fail proves nothing. This asserts that `same` really
    distinguishes a one-bit difference, so a passing run above means something.
    """
    value = 0.1
    nudged = struct.unpack("<d", struct.pack("<Q", bits(value) + 1))[0]
    assert value != nudged
    assert not same(value, nudged)
    assert math.isclose(value, nudged)  # and a tolerance would have called them equal
```

- [ ] **Step 2: Run it**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
python -m pytest tests/test_conformance.py -v
```

Expected: all tests pass, including `test_the_harness_can_actually_fail`.

**If any conformance test fails, that is a finding, not a defect to paper over.** Record which function diverged, at which inputs, and by how many bits. Do not add a tolerance. Do not adjust the Rust to match by trial and error without understanding why. The likely culprits, in order: a reordered arithmetic expression, a std maths call that slipped past the guard, or `to_radians`/`to_degrees` differing from Python's `math.radians`/`math.degrees`.

- [ ] **Step 3: Confirm the existing Python suite still passes untouched**

```bash
python -m pytest tests/ -q
```

Expected: the pre-existing 240 tests still pass, plus the new conformance tests. Nothing in `worldbuilder/` was changed in this slice, so any failure there is a real regression and must be investigated rather than accepted.

- [ ] **Step 4: Commit**

```bash
git add tests/test_conformance.py
git commit -m "engine: the differential harness, and proof it can fail"
```

---

### Task 7: Record what was built and what it showed

**Files:**
- Create: `crates/worldbuilder-engine/README.md`
- Modify: `docs/design/2026-09-02-mark-2-world-studio.md` (section 4.4)

- [ ] **Step 1: Write the engine README**

Create `crates/worldbuilder-engine/README.md`, filling the bracketed figures from your own run:

```markdown
# worldbuilder-engine

The generator core. One implementation, compiled twice: natively for Evennia and maritime
through Python bindings, and to WebAssembly for the browser studio.

Slice 0 measured that those two targets agree bit-for-bit over 5,000,000 samples, with a
negative control proving the comparison could detect a one-bit difference. That is the
foundation this crate is built on; see `spikes/0-bit-equality/README.md`.

## What is here so far

    src/detmath.rs   the only place a transcendental is called
    src/vectors.rs   Vec3
    src/sphere.rs    SpherePoint
    src/bindings.rs  the PyO3 surface, conversion only

The Python in `worldbuilder/` is still the reference implementation and is unchanged.
Nothing has been deleted, and the engine is additive until conformance is established for
every module.

## Two rules that are not style

**No std maths.** Everything transcendental routes through `detmath`, backed by the
pure-Rust `libm`. `tests/no_std_math.rs` fails the build if a std float method appears
outside that file, and the guard has been observed to fail, not merely to pass.

**Floor, never cast.** `worldbuilder/terrain/noise.py` derives lattice cells with
`int(x // 1)`, which floors toward negative infinity; Rust's `as i64` truncates toward
zero. For any negative coordinate they select a different cell, silently. Use
`detmath::floor`.

## Building it

    cd crates/worldbuilder-engine
    python -m maturin develop --release

## Conformance

    python -m pytest tests/test_conformance.py -v

[N] cases compared bit-for-bit across Vec3 length, cross and normalise, and SpherePoint
from_latlon, to_latlon, angle_to and distance_to. Result: [identical / diverged, with
detail]. The harness includes a test asserting that it can distinguish a one-bit
difference, because a conformance suite that cannot fail proves nothing.
```

- [ ] **Step 2: Record it in the spec**

In `docs/design/2026-09-02-mark-2-world-studio.md`, section 4.4, append a short paragraph
after the existing text:

```
**Slice 1a, 2026-09-02.** The crate exists at `crates/worldbuilder-engine`, Python calls it
through PyO3, and the geometry layer is ported and checked against its Python original
bit-for-bit by `tests/test_conformance.py`. Two rules are now mechanised rather than
written down: a build-failing guard forbids std float maths outside `detmath`, and
`detmath::floor` exists because Python's `int(x // 1)` floors where Rust's `as i64`
truncates — a difference that would have moved every lattice cell in the southern half of
the sphere without raising anything.
```

- [ ] **Step 3: Commit**

```bash
git add crates/worldbuilder-engine/README.md docs/design/2026-09-02-mark-2-world-studio.md
git commit -m "engine: record the bridge, and the two rules that are now mechanical"
```

---

## What this slice deliberately does not do

- **No WASM build of the engine.** Slice 0 established the technique; wiring it into this
  crate belongs with the viewer in slice 2, when something needs it.
- **No parameter surface.** It is spec section 5 and its own sub-slice.
- **No `TangentFrame`.** It is geometry, but it brings the azimuthal-equidistant
  projection and its 200 km region cap with it, which deserves its own conformance work.
- **No deletion of Python.** Nothing is removed until every module has a passing
  conformance test, and even then removal is its own decision.
