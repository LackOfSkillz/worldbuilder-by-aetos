# Slice 1c — TangentFrame — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port `worldbuilder/geometry/tangent.py` to the Rust engine, completing the geometry layer, and prove it agrees with the Python.

**Architecture:** `TangentFrame` is how the planet's curved surface becomes a flat local chart in metres, and back. Maritime works in local metres and never learns the world is a sphere; the shelf shaper walks fixed distances along geodesics rather than nudging raw coordinates. It is also the module that carries the azimuthal-equidistant projection and its measured 200 km region cap.

**It exercises BOTH conformance contracts, which is why it is worth doing before Continentality.** `TangentFrame::at` uses only cross products, dot products and `sqrt` — and IEEE-754 requires `sqrt` to be correctly rounded — so it falls under the **strict** bit-for-bit contract. `local_to_sphere` and `sphere_to_local` use `hypot`, `cos`, `sin` and `atan2`, so they fall under the **4-ULP bounded** contract. The split is per code path, not per module.

**Tech Stack:** Rust (stable 1.98.0), `libm` 0.2.11, PyO3, maturin, Python 3.11, pytest.

**Spec:** `docs/design/2026-09-02-mark-2-world-studio.md` sections 4.2, 4.3, 4.4. The projection's measured error table lives in `docs/design/mark-1-scope.md` and `2026-08-31-planet-design.md`.

**Prior slices:** 1a built the crate, `detmath`, `Vec3`, `SpherePoint`, the bindings and the conformance harness. 1b ported `Noise` and established two rules for transcribing constants — read the "Constants transcribed from Python" section of `crates/worldbuilder-engine/README.md` before writing any literal.

## Global Constraints

- Rust is at `~/.cargo/bin`, NOT on PATH in a fresh shell — begin every bash call with `export PATH="$HOME/.cargo/bin:$PATH"`. Shell state does not persist between tool calls.
- Python is the project venv: `PYTHONPATH=/d/dev/worldbuilder_by_aetos .venv/Scripts/python -m pytest tests/ -q`.
- Run cargo from the repository root with `-p worldbuilder-engine`, dev profile, never `--release`.
- **No std float maths.** Everything through `detmath`. The guard fails the build on method form, `f64::` call form, and bare integer casts without a `// cast-ok: <reason>` marker.
- **Constants transcribed from the Python are written without underscore separators**, character-identical to their source. `EARTH_RADIUS_M` is `6_371_000.0` in the Python and already ported in `sphere.rs`; `DEGENERATE` is `1e-9` and already in `vectors.rs`. This slice should need no new numeric constants — if you find yourself adding one, that is a signal to check you are not reinventing something already ported.
- **Transcribe, do not rederive.** `local_to_sphere` is deliberately written out in components rather than vector algebra, and the Python's own comment records that the rewrite was verified by hashing every value the world produces. Reordering it is a defect.
- Nothing under `worldbuilder/` is modified. `worldbuilder/integration/maritime.py` has a pre-existing uncommitted change — leave it unstaged.
- Register each new module in `lib.rs` BEFORE running its failing test; an unregistered module reports zero tests rather than the error you want to see.

---

## File Structure

    crates/worldbuilder-engine/src/tangent.rs   TangentFrame: at, local_to_sphere, sphere_to_local
    crates/worldbuilder-engine/src/bindings.rs  gains frame entry points
    crates/worldbuilder-engine/src/lib.rs       gains `pub mod tangent;` and registrations
    tests/test_conformance.py                   gains a TangentFrame section

---

### Task 1: The frame, and what east means at a pole

**Files:**
- Create: `crates/worldbuilder-engine/src/tangent.rs`
- Modify: `crates/worldbuilder-engine/src/lib.rs`

**Interfaces:**
- Consumes: `vectors::{Vec3, NORTH_AXIS, POLAR_FALLBACK, DEGENERATE}`, `sphere::{SpherePoint, EARTH_RADIUS_M}`.
- Produces: `tangent::TangentFrame` with fields `origin: SpherePoint`, `east: Vec3`, `north: Vec3`, `up: Vec3`, `radius_m: f64`; constructors `at(origin, radius_m)` and `at_latlon(lat_deg, lon_deg, radius_m)`.

The fallback chain is the substance of this task. East is the direction at right angles to both up and the planet's axis — which is what east means anywhere it means anything. **At a pole it means nothing**, the cross product goes to zero, and a direction must be chosen instead. Which one does not matter. That the *same* one is chosen every time is the whole requirement, because a frame that reshuffled itself between two calls would move every ship it held.

- [ ] **Step 1: Write the failing test**

Create `crates/worldbuilder-engine/src/tangent.rs` with only this test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_basis_is_orthonormal_at_an_ordinary_place() {
        let frame = TangentFrame::at_latlon(51.5, -0.12, EARTH_RADIUS_M);
        assert!((frame.east.length() - 1.0).abs() < 1e-12);
        assert!((frame.north.length() - 1.0).abs() < 1e-12);
        assert!((frame.up.length() - 1.0).abs() < 1e-12);
        assert!(frame.east.dot(&frame.north).abs() < 1e-12);
        assert!(frame.east.dot(&frame.up).abs() < 1e-12);
        assert!(frame.north.dot(&frame.up).abs() < 1e-12);
    }

    #[test]
    fn east_points_east_on_the_equator() {
        // At (0, 0) the up vector is +x, so east must be +y.
        let frame = TangentFrame::at_latlon(0.0, 0.0, EARTH_RADIUS_M);
        assert!((frame.east.y - 1.0).abs() < 1e-12, "east was {:?}", frame.east);
    }

    #[test]
    fn a_pole_still_yields_an_orthonormal_basis() {
        for lat in [90.0, -90.0] {
            let frame = TangentFrame::at_latlon(lat, 0.0, EARTH_RADIUS_M);
            assert!((frame.east.length() - 1.0).abs() < 1e-9, "at {}", lat);
            assert!((frame.north.length() - 1.0).abs() < 1e-9, "at {}", lat);
            assert!(frame.east.dot(&frame.up).abs() < 1e-9, "at {}", lat);
        }
    }

    #[test]
    fn a_pole_yields_the_same_basis_every_time() {
        // The whole requirement. A frame that reshuffled itself between two calls would
        // move every ship it held.
        let a = TangentFrame::at(&SpherePoint { vector: Vec3::new(0.0, 0.0, 1.0) }, EARTH_RADIUS_M);
        let b = TangentFrame::at(&SpherePoint { vector: Vec3::new(0.0, 0.0, 1.0) }, EARTH_RADIUS_M);
        assert_eq!(a.east.x.to_bits(), b.east.x.to_bits());
        assert_eq!(a.east.y.to_bits(), b.east.y.to_bits());
        assert_eq!(a.east.z.to_bits(), b.east.z.to_bits());
    }

    #[test]
    fn up_is_the_origins_own_vector() {
        let origin = SpherePoint::from_latlon(31.0, 7.0);
        let frame = TangentFrame::at(&origin, EARTH_RADIUS_M);
        assert_eq!(frame.up.x.to_bits(), origin.vector.x.to_bits());
        assert_eq!(frame.up.y.to_bits(), origin.vector.y.to_bits());
        assert_eq!(frame.up.z.to_bits(), origin.vector.z.to_bits());
    }
}
```

- [ ] **Step 2: Register the module and run the failing test**

Add `pub mod tangent;` to `lib.rs`, then:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p worldbuilder-engine tangent
```

Expected: FAIL — `cannot find type TangentFrame`.

- [ ] **Step 3: Write the implementation**

Insert above the test module:

```rust
//! A local flat coordinate system, tangent to the planet at one point.
//!
//! Ported from `worldbuilder/geometry/tangent.py`. This is how a curved planet becomes a
//! flat chart in metres and back: maritime works in local metres and never learns the
//! world is a sphere, and the shelf shaper walks fixed distances along geodesics rather
//! than nudging raw coordinates, which would step off the sphere entirely.

use crate::detmath as m;
use crate::sphere::{SpherePoint, EARTH_RADIUS_M};
use crate::vectors::{Vec3, DEGENERATE, NORTH_AXIS, POLAR_FALLBACK};

#[derive(Debug, Clone, Copy)]
pub struct TangentFrame {
    /// Where the chart touches the globe; local (0, 0).
    pub origin: SpherePoint,
    /// Unit vector, increasing local x.
    pub east: Vec3,
    /// Unit vector, increasing local y.
    pub north: Vec3,
    /// Unit vector, away from the centre. Equal to the origin's vector.
    pub up: Vec3,
    pub radius_m: f64,
}

impl TangentFrame {
    /// East is the direction at right angles to both straight up and the planet's axis.
    ///
    /// At a pole east means nothing — every direction from the north pole is south — and
    /// that is a fact about poles rather than a failure of the maths. The cross product
    /// goes to zero there and the basis cannot be derived, so one is chosen instead.
    /// Which direction it is does not matter in the slightest. That it is the *same* one
    /// on every call is the whole requirement.
    pub fn at(origin: &SpherePoint, radius_m: f64) -> Self {
        let up = origin.vector;
        let mut sideways = NORTH_AXIS.cross(&up);
        if sideways.length() <= DEGENERATE {
            // At a pole, or near enough that the arithmetic has lost its nerve.
            sideways = POLAR_FALLBACK.cross(&up);
            if sideways.length() <= DEGENERATE {
                // The fallback was itself parallel to up, which cannot happen for a
                // planet whose axis is z — but a fixed second answer costs one line and
                // removes the only path here that could ever fail.
                sideways = Vec3::new(0.0, 1.0, 0.0).cross(&up);
            }
        }
        // The Python calls .normalised() and would raise on a zero vector; by this point
        // the fallback chain has guaranteed a non-zero result, so the None case is
        // unreachable. Falling back to POLAR_FALLBACK rather than panicking keeps the
        // function total, and the conformance corpus covers the poles.
        let east = sideways.normalised().unwrap_or(POLAR_FALLBACK);
        let north = up.cross(&east);
        Self { origin: *origin, east, north, up, radius_m }
    }

    /// Convenience: a frame centred on a named latitude and longitude.
    pub fn at_latlon(latitude_deg: f64, longitude_deg: f64, radius_m: f64) -> Self {
        Self::at(&SpherePoint::from_latlon(latitude_deg, longitude_deg), radius_m)
    }
}
```

Note `EARTH_RADIUS_M` and `m` are imported for the later tasks; if the compiler warns they are unused at this point, leave them — Task 2 uses both.

- [ ] **Step 4: Run and confirm the tests pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p worldbuilder-engine
```

- [ ] **Step 5: Commit**

```bash
git add crates/worldbuilder-engine/src/tangent.rs crates/worldbuilder-engine/src/lib.rs
git commit -m "engine: a tangent frame, and the same east at a pole every time"
```

---

### Task 2: Local to sphere

**Files:**
- Modify: `crates/worldbuilder-engine/src/tangent.rs`

**Interfaces:**
- Produces: `TangentFrame::local_to_sphere(&self, x_m: f64, y_m: f64) -> SpherePoint`.

The local distance from the origin is taken as an **arc along the surface**, not a straight line across the tangent plane. That is what makes the projection equidistant, and what stops a thousand-mile chart quietly claiming more ocean than the planet has.

**This is written out in components rather than in vector algebra, and that is deliberate.** The Python's own comment records why: profiling put it at a quarter of a chart redraw's cost, it is called six times per terrain sample, and the tidy version built seven intermediate vectors each time — forty-two objects a sample to produce one answer. The comment also records that the rewrite was verified by hashing every value the world produces. Transcribe it in the same operations in the same order.

- [ ] **Step 1: Write the failing test**

Append to the test module:

```rust
    #[test]
    fn the_origin_maps_to_itself() {
        let frame = TangentFrame::at_latlon(51.5, -0.12, EARTH_RADIUS_M);
        let there = frame.local_to_sphere(0.0, 0.0);
        assert_eq!(there.vector.x.to_bits(), frame.origin.vector.x.to_bits());
        assert_eq!(there.vector.y.to_bits(), frame.origin.vector.y.to_bits());
        assert_eq!(there.vector.z.to_bits(), frame.origin.vector.z.to_bits());
    }

    #[test]
    fn a_step_east_lands_east() {
        let frame = TangentFrame::at_latlon(0.0, 0.0, EARTH_RADIUS_M);
        let (_, lon) = frame.local_to_sphere(100_000.0, 0.0).to_latlon();
        assert!(lon > 0.0, "stepping east gave longitude {}", lon);
    }

    #[test]
    fn the_projection_is_equidistant() {
        // The defining property: local distance equals great-circle distance from the
        // origin. If this drifts, a chart is claiming more ocean than the planet has.
        let frame = TangentFrame::at_latlon(20.0, 30.0, EARTH_RADIUS_M);
        for metres in [1_000.0, 25_000.0, 200_000.0, 1_000_000.0] {
            let there = frame.local_to_sphere(metres, 0.0);
            let measured = frame.origin.distance_to(&there, EARTH_RADIUS_M);
            let error = (measured - metres).abs();
            assert!(error < 1e-6, "at {} m the arc measured {} m", metres, measured);
        }
    }

    #[test]
    fn the_result_is_a_unit_vector() {
        let frame = TangentFrame::at_latlon(-40.0, 170.0, EARTH_RADIUS_M);
        let there = frame.local_to_sphere(300_000.0, -200_000.0);
        assert!((there.vector.length() - 1.0).abs() < 1e-12);
    }
```

- [ ] **Step 2: Run and confirm it fails**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p worldbuilder-engine tangent
```

- [ ] **Step 3: Write the implementation**

Add inside `impl TangentFrame`:

```rust
    /// Where a point on the chart actually is.
    ///
    /// The local distance is taken as an arc along the surface rather than a straight
    /// line across the tangent plane, which is what makes the projection equidistant.
    ///
    /// Written out in components rather than in vector algebra, which is not how the rest
    /// of this crate is written and so needs the excuse: the Python's profiling put this
    /// at a quarter of a chart redraw's cost, it is called six times per terrain sample,
    /// and the tidy version built seven intermediate vectors each time. Same operations,
    /// same order.
    pub fn local_to_sphere(&self, x_m: f64, y_m: f64) -> SpherePoint {
        let distance = m::hypot(x_m, y_m);
        if distance == 0.0 {
            return self.origin;
        }

        let angle = distance / self.radius_m;
        let along = x_m / distance;
        let across = y_m / distance;
        let (east, north, up) = (self.east, self.north, self.up);
        let heading_x = east.x * along + north.x * across;
        let heading_y = east.y * along + north.y * across;
        let heading_z = east.z * along + north.z * across;

        let forward = m::cos(angle);
        let outward = m::sin(angle);
        let x = up.x * forward + heading_x * outward;
        let y = up.y * forward + heading_y * outward;
        let z = up.z * forward + heading_z * outward;

        let scale = 1.0 / m::sqrt(x * x + y * y + z * z);
        SpherePoint { vector: Vec3::new(x * scale, y * scale, z * scale) }
    }
```

Note the final normalisation is `1.0 / sqrt(...)` then three multiplies — a reciprocal and multiply, not three divisions. That is what the Python does and the two are not the same in floating point.

- [ ] **Step 4: Run and confirm the tests pass**

- [ ] **Step 5: Commit**

```bash
git add crates/worldbuilder-engine/src/tangent.rs
git commit -m "engine: local to sphere, in components and in the Python's order"
```

---

### Task 3: Sphere to local

**Files:**
- Modify: `crates/worldbuilder-engine/src/tangent.rs`

**Interfaces:**
- Produces: `TangentFrame::sphere_to_local(&self, point: &SpherePoint) -> (f64, f64)`.

The exact inverse of `local_to_sphere`, and tested as one. A point directly opposite the origin has no direction on this chart at all — every bearing reaches it — and returns the origin rather than failing, because a chart of half a planet is a misuse the caller should not have to guard against.

- [ ] **Step 1: Write the failing test**

Append to the test module:

```rust
    #[test]
    fn the_origin_maps_back_to_zero() {
        let frame = TangentFrame::at_latlon(51.5, -0.12, EARTH_RADIUS_M);
        let (x, y) = frame.sphere_to_local(&frame.origin);
        assert_eq!(x.to_bits(), 0.0f64.to_bits());
        assert_eq!(y.to_bits(), 0.0f64.to_bits());
    }

    #[test]
    fn the_antipode_returns_the_origin_rather_than_failing() {
        let frame = TangentFrame::at_latlon(10.0, 20.0, EARTH_RADIUS_M);
        let opposite = SpherePoint { vector: frame.up.scaled(-1.0) };
        let (x, y) = frame.sphere_to_local(&opposite);
        assert_eq!(x.to_bits(), 0.0f64.to_bits());
        assert_eq!(y.to_bits(), 0.0f64.to_bits());
    }

    #[test]
    fn the_round_trip_returns_where_it_started() {
        let frame = TangentFrame::at_latlon(-33.0, 151.0, EARTH_RADIUS_M);
        for (x_m, y_m) in [(1_000.0, 0.0), (0.0, 25_000.0), (120_000.0, -80_000.0)] {
            let there = frame.local_to_sphere(x_m, y_m);
            let (back_x, back_y) = frame.sphere_to_local(&there);
            assert!((back_x - x_m).abs() < 1e-6, "x {} came back {}", x_m, back_x);
            assert!((back_y - y_m).abs() < 1e-6, "y {} came back {}", y_m, back_y);
        }
    }
```

- [ ] **Step 2: Run and confirm it fails**

- [ ] **Step 3: Write the implementation**

Add inside `impl TangentFrame`:

```rust
    /// Where a place on the globe falls on this chart.
    ///
    /// The exact inverse of `local_to_sphere`, and tested as one. A point directly
    /// opposite the origin has no direction on this chart at all — every bearing reaches
    /// it — and returns the origin rather than failing, because a chart of half a planet
    /// is a misuse the caller should not have to guard against.
    pub fn sphere_to_local(&self, point: &SpherePoint) -> (f64, f64) {
        let along = self.up.dot(&point.vector);
        let sideways = point.vector.sub(&self.up.scaled(along));
        let across = sideways.length();
        if across <= DEGENERATE {
            // The origin itself, or its antipode.
            return (0.0, 0.0);
        }

        let heading = sideways.scaled(1.0 / across);
        let distance = m::atan2(across, along) * self.radius_m;
        (heading.dot(&self.east) * distance, heading.dot(&self.north) * distance)
    }
```

- [ ] **Step 4: Run and confirm the tests pass**

- [ ] **Step 5: Commit**

```bash
git add crates/worldbuilder-engine/src/tangent.rs
git commit -m "engine: sphere to local, and an antipode that returns rather than raises"
```

---

### Task 4: Bindings and conformance

**Files:**
- Modify: `crates/worldbuilder-engine/src/bindings.rs`, `crates/worldbuilder-engine/src/lib.rs`
- Modify: `tests/test_conformance.py`

**Interfaces:**
- Produces, on the Python side:
  - `frame_at(x, y, z, radius_m) -> (ex, ey, ez, nx, ny, nz, ux, uy, uz)`
  - `frame_local_to_sphere(x, y, z, radius_m, east_m, north_m) -> (px, py, pz)`
  - `frame_sphere_to_local(x, y, z, radius_m, px, py, pz) -> (east_m, north_m)`

Each takes the origin's components and rebuilds the frame, so the binding stays stateless and the harness can drive it freely.

**This module spans both conformance contracts, and the tests must reflect that:**
- `frame_at` uses only cross products, dot products and `sqrt`. IEEE-754 requires `sqrt` correctly rounded, so **`at` is held STRICTLY, with `same()`**.
- `local_to_sphere` and `sphere_to_local` use `hypot`, `cos`, `sin` and `atan2`, so they are held to **`close_enough` at the existing 4-ULP bound**.

Do not blur the two. If a strict comparison on `frame_at` fails, that is a real defect, not a candidate for loosening.

- [ ] **Step 1: Add the bindings**

Append to `bindings.rs`:

```rust
#[pyfunction]
pub fn frame_at(x: f64, y: f64, z: f64, radius_m: f64) -> (f64, f64, f64, f64, f64, f64, f64, f64, f64) {
    let origin = SpherePoint { vector: Vec3::new(x, y, z) };
    let f = crate::tangent::TangentFrame::at(&origin, radius_m);
    (f.east.x, f.east.y, f.east.z, f.north.x, f.north.y, f.north.z, f.up.x, f.up.y, f.up.z)
}

#[pyfunction]
pub fn frame_local_to_sphere(
    x: f64, y: f64, z: f64, radius_m: f64, east_m: f64, north_m: f64,
) -> (f64, f64, f64) {
    let origin = SpherePoint { vector: Vec3::new(x, y, z) };
    let f = crate::tangent::TangentFrame::at(&origin, radius_m);
    let p = f.local_to_sphere(east_m, north_m);
    (p.vector.x, p.vector.y, p.vector.z)
}

#[pyfunction]
#[allow(clippy::too_many_arguments)]
pub fn frame_sphere_to_local(
    x: f64, y: f64, z: f64, radius_m: f64, px: f64, py: f64, pz: f64,
) -> (f64, f64) {
    let origin = SpherePoint { vector: Vec3::new(x, y, z) };
    let f = crate::tangent::TangentFrame::at(&origin, radius_m);
    f.sphere_to_local(&SpherePoint { vector: Vec3::new(px, py, pz) })
}
```

Register all three in the `#[pymodule]`.

- [ ] **Step 2: Rebuild**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cd crates/worldbuilder-engine && python -m maturin develop --release
```

Use the project venv; confirm it installs there and not into `D:\dev\.venv`.

- [ ] **Step 3: Add the conformance tests**

Append to `tests/test_conformance.py`:

```python
# ---------------------------------------------------------------------------
# TangentFrame
#
# This module spans BOTH contracts, which is why it is a useful one to port early.
#
#   at()               cross products, dot products and sqrt. IEEE-754 requires sqrt
#                      correctly rounded, so this is held STRICTLY, bit-for-bit.
#   local_to_sphere()  hypot, cos, sin, sqrt -- bounded at MAX_TRANSCENDENTAL_ULPS.
#   sphere_to_local()  atan2, sqrt            -- bounded likewise.
#
# The split is per code path, not per module. A strict failure below is a real defect.
# ---------------------------------------------------------------------------

from worldbuilder.geometry.tangent import TangentFrame as PyTangentFrame

FRAME_RADIUS = EARTH_RADIUS_M


def frame_origins():
    """Ordinary places, both poles, and the meridian — where a frame breaks first."""
    yield (0.0, 0.0, 1.0)
    yield (0.0, 0.0, -1.0)
    yield (1.0, 0.0, 0.0)
    yield (-1.0, 0.0, 0.0)
    for lat in range(-85, 86, 5):
        for lon in range(-180, 181, 30):
            v = SpherePoint.from_latlon(float(lat), float(lon)).vector
            yield (v.x, v.y, v.z)


def test_frame_at_agrees_exactly():
    """Strict: at() has no transcendental in its path beyond a correctly-rounded sqrt."""
    for x, y, z in frame_origins():
        py = PyTangentFrame.at(SpherePoint(Vec3(x, y, z)), FRAME_RADIUS)
        got = engine.frame_at(x, y, z, FRAME_RADIUS)
        want = (py.east.x, py.east.y, py.east.z,
                py.north.x, py.north.y, py.north.z,
                py.up.x, py.up.y, py.up.z)
        for i, (w, g) in enumerate(zip(want, got)):
            assert same(w, g), f"frame_at({x},{y},{z}) component {i}: {w!r} vs {g!r}"


def test_frame_at_is_stable_at_the_poles():
    """
    A frame that reshuffled itself between two calls would move every ship it held, so
    the pole fallback must be the same choice every time and the same choice as Python's.
    """
    for pole in [(0.0, 0.0, 1.0), (0.0, 0.0, -1.0)]:
        first = engine.frame_at(*pole, FRAME_RADIUS)
        second = engine.frame_at(*pole, FRAME_RADIUS)
        assert first == second
        py = PyTangentFrame.at(SpherePoint(Vec3(*pole)), FRAME_RADIUS)
        assert same(py.east.x, first[0]) and same(py.east.y, first[1]) and same(py.east.z, first[2])


def test_local_to_sphere_agrees_within_bound():
    for x, y, z in frame_origins():
        py = PyTangentFrame.at(SpherePoint(Vec3(x, y, z)), FRAME_RADIUS)
        for east_m, north_m in [(0.0, 0.0), (1_000.0, 0.0), (0.0, -25_000.0),
                                (200_000.0, 200_000.0), (-1_000_000.0, 500_000.0)]:
            want = py.local_to_sphere(east_m, north_m).vector
            got = engine.frame_local_to_sphere(x, y, z, FRAME_RADIUS, east_m, north_m)
            for w, g in zip((want.x, want.y, want.z), got):
                assert close_enough(w, g), (
                    f"local_to_sphere at ({x},{y},{z}) + ({east_m},{north_m}): "
                    f"{w!r} vs {g!r}, {ulps_apart(w, g)} ULP"
                )


def test_sphere_to_local_agrees_within_bound():
    for x, y, z in frame_origins():
        py = PyTangentFrame.at(SpherePoint(Vec3(x, y, z)), FRAME_RADIUS)
        for east_m, north_m in [(1_000.0, 0.0), (0.0, -25_000.0), (200_000.0, 200_000.0)]:
            there = py.local_to_sphere(east_m, north_m)
            want = py.sphere_to_local(there)
            got = engine.frame_sphere_to_local(
                x, y, z, FRAME_RADIUS, there.vector.x, there.vector.y, there.vector.z
            )
            for w, g in zip(want, got):
                assert close_enough(w, g), (
                    f"sphere_to_local at ({x},{y},{z}): {w!r} vs {g!r}, {ulps_apart(w, g)} ULP"
                )


def test_the_projection_error_table_reproduces():
    """
    The 200 km region cap is a measured engineering limit, not an assertion, and the spec
    quotes the table it came from. If the Rust reproduced the projection but not its error
    profile, the cap would silently stop meaning what it says.

    This asserts the Rust's round-trip error tracks the Python's at the same distances,
    rather than pinning absolute figures that belong to the spec.
    """
    py = PyTangentFrame.at_latlon(45.0, 0.0, FRAME_RADIUS)
    for metres in [25_000.0, 100_000.0, 200_000.0, 500_000.0, 1_000_000.0]:
        there_py = py.local_to_sphere(metres, 0.0)
        back_py = py.sphere_to_local(there_py)
        origin = py.origin.vector
        got_there = engine.frame_local_to_sphere(
            origin.x, origin.y, origin.z, FRAME_RADIUS, metres, 0.0
        )
        back_rs = engine.frame_sphere_to_local(
            origin.x, origin.y, origin.z, FRAME_RADIUS, *got_there
        )
        for w, g in zip(back_py, back_rs):
            assert close_enough(w, g), f"round trip at {metres} m: {w!r} vs {g!r}"
```

- [ ] **Step 4: Run the conformance suite**

```bash
cd /d/dev/worldbuilder_by_aetos
PYTHONPATH=/d/dev/worldbuilder_by_aetos .venv/Scripts/python -m pytest tests/test_conformance.py -v
```

**If `test_frame_at_agrees_exactly` fails, do not switch it to `close_enough`.** `at` has no transcendental in its path; a divergence there means the fallback chain, the cross products or the normalisation differ, and the fix is in the port. Report the failing origin and both bit patterns.

If a bounded test fails, report the ULP distance — if it exceeds 4, that is a finding about the port, not a reason to raise the bound.

- [ ] **Step 5: Run the whole suite**

```bash
PYTHONPATH=/d/dev/worldbuilder_by_aetos .venv/Scripts/python -m pytest tests/ -q
```

- [ ] **Step 6: Commit**

```bash
git add crates/worldbuilder-engine/src/bindings.rs crates/worldbuilder-engine/src/lib.rs tests/test_conformance.py
git commit -m "engine: frame bindings, and a module that spans both contracts"
```

---

### Task 5: Record it

**Files:**
- Modify: `crates/worldbuilder-engine/README.md`

- [ ] **Step 1: Update the README**

Add `src/tangent.rs` to the file listing. Extend the conformance section to record that `TangentFrame` spans both contracts — `at` strictly because its only transcendental is a correctly-rounded `sqrt`, the two projections bounded — and say why that makes the split principled per code path rather than a per-module label. Update the test counts to what the suite actually reports.

Note also that the geometry layer is now complete, and that `Continentality` is unblocked because its `gradient` method needs this frame.

- [ ] **Step 2: Commit**

```bash
git add crates/worldbuilder-engine/README.md
git commit -m "engine: record a module that lands on both sides of the contract line"
```

---

## What this slice deliberately does not do

- **No `Continentality`.** It is next, and it needs this frame for `gradient`.
- **No re-derivation of the projection error table.** Those figures are Mark 1 measurements in the spec; this slice only proves the Rust tracks the Python, which is the property that matters for a port.
- **No deletion of `worldbuilder/geometry/tangent.py`.** The Python stays the reference.
