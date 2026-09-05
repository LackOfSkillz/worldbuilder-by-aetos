# Slice 1e — Plates and the Voronoi Lookup — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port `worldbuilder/plates/model.py` and the construction and nearest-neighbour half of `worldbuilder/plates/lookup.py`, and prove they agree with the Python bit-for-bit.

**Architecture:** A plate is a seed point on the sphere, an Euler pole it turns about, and a signed rate. A point belongs to the plate whose seed is nearest — that is the whole of the Voronoi construction. `PlateSet` holds the plates plus one precomputed table: for each ordered pair, the normal of the plane bisecting them. Points equidistant from seeds A and B satisfy `dot(P, A) == dot(P, B)`, which rearranges to `dot(P, A - B) == 0`, so the margin between two plates is a great circle whose plane normal is `normalise(A - B)`. A couple of dozen plates makes a few hundred such vectors, and that is the entire stored geometry of a planet's tectonics.

**This slice is entirely STRICT.** `nearest_two` compares by dot product — multiplies and adds, nothing else. The bisector table is a subtraction, a `length()` and a `normalised()`, and IEEE-754 requires `sqrt` correctly rounded. There is no transcendental anywhere in this slice, so every conformance comparison is exact and no ULP bound is available to excuse a divergence.

**Tech Stack:** Rust (stable 1.98.0), `libm` 0.2.11, PyO3, maturin, Python 3.11, pytest.

**Spec:** `docs/design/2026-09-02-mark-2-world-studio.md` sections 4.2, 4.3, 4.4.

**Prior slices:** 1a built the crate, `detmath`, `Vec3`, `SpherePoint`, the bindings and the conformance harness. 1b ported `Noise`, 1c `TangentFrame`, 1d `Continentality`. Read the conformance section of `crates/worldbuilder-engine/README.md` before starting — especially the rules on transcribing constants and on what the two contracts mean.

## Why this comes before tectonics

`worldbuilder/terrain/tectonics.py` imports `ACROSS_ENOUGH` and `motion_between` from `plates.kinematics`, and `kinematics` works on `Plate` values. Porting tectonics first would leave a module reaching into an unported package. Plates come first, and this slice takes the half that depends only on geometry.

## Global Constraints

- Rust is at `~/.cargo/bin`, NOT on PATH in a fresh shell — begin every bash call with `export PATH="$HOME/.cargo/bin:$PATH"`.
- Python is the project venv: `PYTHONPATH=/d/dev/worldbuilder_by_aetos .venv/Scripts/python -m pytest tests/ -q`.
- Run cargo from the repository root with `-p worldbuilder-engine`, dev profile, never `--release`.
- **No std float maths.** Everything through `detmath`. The guard bans method form, the `f64::` call form, `mul_add`, and bare integer casts without `// cast-ok: <reason>`.
- **Constants transcribed from the Python are written without underscore separators**, character-identical to their source.
- **Transcribe, do not rederive.** This slice is strict throughout, so there is no bound to absorb a reordering.
- Nothing under `worldbuilder/` is modified. `worldbuilder/integration/maritime.py` has a pre-existing uncommitted change — leave it unstaged.
- Register each module in `lib.rs` BEFORE running its failing test.

---

## File Structure

    crates/worldbuilder-engine/src/plates.rs      Plate, Margin, PlateSet
    crates/worldbuilder-engine/src/bindings.rs    gains plate entry points
    crates/worldbuilder-engine/src/lib.rs         gains `pub mod plates;`
    tests/test_conformance.py                     gains a PlateSet section

One module rather than a package: `model.py` is a single dataclass and `lookup.py`'s remaining half arrives next slice. If `plates.rs` grows past a few hundred lines when the margin work lands, splitting it then is easy; splitting it now would be structure without content.

---

### Task 1: The plate

**Files:**
- Create: `crates/worldbuilder-engine/src/plates.rs`
- Modify: `crates/worldbuilder-engine/src/lib.rs`

**Interfaces:**
- Produces: `plates::Plate { index: usize, seed: SpherePoint, euler_pole: SpherePoint, rate_rad_per_myr: f64 }` with `angular_velocity() -> Vec3`.

The rate is **signed**: the sign and the pole together give the sense of rotation, so there is no separate clockwise flag to get wrong. And combining the pole and the rate into one vector is what makes surface velocity a single cross product rather than a special case at the pole itself.

- [ ] **Step 1: Write the failing test**

Create `crates/worldbuilder-engine/src/plates.rs` with only this test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn a_plate(index: usize, rate: f64) -> Plate {
        Plate {
            index,
            seed: SpherePoint::from_latlon(10.0, 20.0),
            euler_pole: SpherePoint::from_latlon(80.0, 5.0),
            rate_rad_per_myr: rate,
        }
    }

    #[test]
    fn angular_velocity_is_the_pole_scaled_by_the_rate() {
        let p = a_plate(0, 0.01);
        let omega = p.angular_velocity();
        let pole = p.euler_pole.vector;
        assert_eq!(omega.x.to_bits(), (pole.x * 0.01).to_bits());
        assert_eq!(omega.y.to_bits(), (pole.y * 0.01).to_bits());
        assert_eq!(omega.z.to_bits(), (pole.z * 0.01).to_bits());
    }

    #[test]
    fn a_negative_rate_reverses_the_rotation() {
        // The sign and the pole together give the sense of rotation; there is no
        // separate clockwise flag to get wrong.
        let forward = a_plate(0, 0.01).angular_velocity();
        let backward = a_plate(0, -0.01).angular_velocity();
        assert_eq!(backward.x.to_bits(), (-forward.x).to_bits());
        assert_eq!(backward.z.to_bits(), (-forward.z).to_bits());
    }

    #[test]
    fn a_zero_rate_is_a_still_plate() {
        let omega = a_plate(0, 0.0).angular_velocity();
        assert_eq!(omega.x.to_bits(), 0.0f64.to_bits());
        assert_eq!(omega.y.to_bits(), 0.0f64.to_bits());
        assert_eq!(omega.z.to_bits(), 0.0f64.to_bits());
    }
}
```

Note the third test: `pole.x * 0.0` is `+0.0` for a positive `pole.x` and `-0.0` for a negative one. `SpherePoint::from_latlon(80.0, 5.0)` has all-positive components, so `+0.0` is right — but if you change the fixture, check the sign.

- [ ] **Step 2: Register the module and run the failing test**

Add `pub mod plates;` to `lib.rs`, then `cargo test -p worldbuilder-engine plates`. Expected: FAIL, cannot find type `Plate`.

- [ ] **Step 3: Write the implementation**

```rust
//! Plates, and the Voronoi lookup that says which one a point is on.
//!
//! Ported from `worldbuilder/plates/model.py` and `worldbuilder/plates/lookup.py`.
//!
//! A point belongs to the plate whose seed is nearest, which is the whole of the Voronoi
//! construction. Everything else here is arithmetic in service of asking that question
//! quickly, and of asking how far the point is from the answer changing.

use crate::sphere::SpherePoint;
use crate::vectors::{Vec3, DEGENERATE};

/// One plate: where it is, what it turns about, and how fast.
#[derive(Debug, Clone, Copy)]
pub struct Plate {
    /// Which plate this is. Stable for a given seed and count.
    pub index: usize,
    /// Its centre. A point belongs to the plate whose seed is nearest.
    pub seed: SpherePoint,
    /// The axis it turns about.
    pub euler_pole: SpherePoint,
    /// Radians per million years. **Signed**: the sign and the pole together give the
    /// sense of rotation, so there is no separate clockwise flag to get wrong.
    pub rate_rad_per_myr: f64,
}

impl Plate {
    /// The rotation vector — the pole, scaled by the rate.
    ///
    /// Combining the two into one vector is what makes surface velocity a single cross
    /// product rather than a special case at the pole itself.
    pub fn angular_velocity(&self) -> Vec3 {
        self.euler_pole.vector.scaled(self.rate_rad_per_myr)
    }
}
```

- [ ] **Step 4: Run and confirm the tests pass**, then the whole crate suite.

- [ ] **Step 5: Commit**

```bash
git add crates/worldbuilder-engine/src/plates.rs crates/worldbuilder-engine/src/lib.rs
git commit -m "engine: a plate, with the sign of its rotation in the rate"
```

---

### Task 2: The set, and its bisector table

**Files:**
- Modify: `crates/worldbuilder-engine/src/plates.rs`

**Interfaces:**
- Produces: `PlateSet` with `new(plates: Vec<Plate>) -> PlateSet`, `len()`, `is_empty()`, `plate(index) -> Plate`, `plates() -> &[Plate]`, and a private `bisector(a: usize, b: usize) -> Option<Vec3>`.

The table is the point of the type. For each ordered pair of plates, it stores the normal of the plane that bisects them: `normalise(A.seed - B.seed)`. That is the entire stored geometry of a planet's tectonics — a few hundred vectors for a couple of dozen plates.

**The entry is absent when a pair cannot define a bisector**: when the plate is itself, or when the two seeds are closer together than `DEGENERATE`, where the difference vector has no trustworthy direction. Python stores `None`; the Rust stores `Option<Vec3>`.

**The Python also keeps a second copy of the same geometry as bare component triples**, because profiling found ninety-nine `Vec3.dot` calls per terrain sample and a Python method call costs more than the three multiplies inside it. **Do not port that duplication.** In Rust a field access is free, and the arithmetic is identical either way — the same reasoning that dropped `Noise`'s corner cache in slice 1b. One representation, no second copy to keep in step.

- [ ] **Step 1: Write the failing test**

```rust
    fn two_plates() -> PlateSet {
        PlateSet::new(vec![
            Plate { index: 0, seed: SpherePoint::from_latlon(0.0, 0.0),
                    euler_pole: SpherePoint::from_latlon(90.0, 0.0), rate_rad_per_myr: 0.01 },
            Plate { index: 1, seed: SpherePoint::from_latlon(0.0, 90.0),
                    euler_pole: SpherePoint::from_latlon(90.0, 0.0), rate_rad_per_myr: 0.01 },
        ])
    }

    #[test]
    fn the_set_reports_its_plates() {
        let set = two_plates();
        assert_eq!(set.len(), 2);
        assert!(!set.is_empty());
        assert_eq!(set.plate(1).index, 1);
    }

    #[test]
    fn a_plate_has_no_bisector_with_itself() {
        assert!(two_plates().bisector(0, 0).is_none());
        assert!(two_plates().bisector(1, 1).is_none());
    }

    #[test]
    fn a_bisector_is_the_normalised_difference_of_the_seeds() {
        let set = two_plates();
        let a = set.plate(0).seed.vector;
        let b = set.plate(1).seed.vector;
        let want = a.sub(&b).normalised().expect("distinct seeds");
        let got = set.bisector(0, 1).expect("distinct seeds");
        assert_eq!(got.x.to_bits(), want.x.to_bits());
        assert_eq!(got.y.to_bits(), want.y.to_bits());
        assert_eq!(got.z.to_bits(), want.z.to_bits());
    }

    #[test]
    fn the_bisector_reverses_with_the_pair() {
        let set = two_plates();
        let forward = set.bisector(0, 1).expect("distinct");
        let backward = set.bisector(1, 0).expect("distinct");
        assert_eq!(backward.x.to_bits(), (-forward.x).to_bits());
        assert_eq!(backward.z.to_bits(), (-forward.z).to_bits());
    }

    #[test]
    fn coincident_seeds_have_no_bisector() {
        // Closer together than DEGENERATE, so the difference has no trustworthy
        // direction and the table records that rather than inventing one.
        let here = SpherePoint::from_latlon(12.0, 34.0);
        let set = PlateSet::new(vec![
            Plate { index: 0, seed: here, euler_pole: here, rate_rad_per_myr: 0.0 },
            Plate { index: 1, seed: here, euler_pole: here, rate_rad_per_myr: 0.0 },
        ]);
        assert!(set.bisector(0, 1).is_none());
    }
```

- [ ] **Step 2: Run and confirm it fails**

- [ ] **Step 3: Write the implementation**

```rust
/// Every plate on a world, with the arithmetic needed to ask questions about them.
///
/// Holds one precomputed table: for each ordered pair, the normal of the plane bisecting
/// them. Points equidistant from seeds A and B satisfy `dot(P, A) == dot(P, B)`, which
/// rearranges to `dot(P, A - B) == 0` — so the margin between two plates is a great circle
/// whose plane normal is `normalise(A - B)`, and the distance from any point to it is an
/// arc sine away.
///
/// The Python keeps a second copy of this geometry as bare component triples, because a
/// Python method call costs more than the three multiplies inside `Vec3.dot` and a chart
/// redraw makes ninety-nine of them per terrain sample. That duplication is deliberately
/// not ported: in Rust the field access is free, the arithmetic is identical, and one
/// representation cannot fall out of step with itself.
pub struct PlateSet {
    plates: Vec<Plate>,
    /// Row-major, `plates.len()` squared. `None` where the pair cannot define a bisector.
    bisectors: Vec<Option<Vec3>>,
}

impl PlateSet {
    pub fn new(plates: Vec<Plate>) -> Self {
        let n = plates.len();
        let mut bisectors = Vec::with_capacity(n * n);
        for plate in &plates {
            for other in &plates {
                let difference = plate.seed.vector.sub(&other.seed.vector);
                // Python tests `other is plate` first, then the length. Index equality is
                // the faithful translation of that identity check, and the length test
                // catches distinct plates whose seeds coincide.
                let entry = if plate.index == other.index || difference.length() <= DEGENERATE {
                    None
                } else {
                    difference.normalised()
                };
                bisectors.push(entry);
            }
        }
        Self { plates, bisectors }
    }

    pub fn len(&self) -> usize {
        self.plates.len()
    }

    pub fn is_empty(&self) -> bool {
        self.plates.is_empty()
    }

    pub fn plate(&self, index: usize) -> Plate {
        self.plates[index]
    }

    pub fn plates(&self) -> &[Plate] {
        &self.plates
    }

    /// The bisector normal for an ordered pair, or `None` where none is defined.
    pub fn bisector(&self, a: usize, b: usize) -> Option<Vec3> {
        self.bisectors[a * self.plates.len() + b]
    }
}
```

**A note on the identity check.** Python writes `other is plate`, an object identity test, which is true exactly when the loop has reached the same element. Comparing `index` is faithful **provided indices are unique within a set**, which the generator guarantees. Comparing positions in the loop would be equally faithful and would not depend on that guarantee — if you prefer that, say so in your report and use it; either is acceptable, but the choice should be deliberate rather than incidental.

- [ ] **Step 4: Run and confirm the tests pass**

- [ ] **Step 5: Commit**

```bash
git add crates/worldbuilder-engine/src/plates.rs
git commit -m "engine: the bisector table, which is the whole stored geometry of tectonics"
```

---

### Task 3: Nearest two

**Files:**
- Modify: `crates/worldbuilder-engine/src/plates.rs`

**Interfaces:**
- Produces: `nearest_two(&self, point: &SpherePoint) -> (Option<Plate>, Option<Plate>)`.

Compared by **dot product rather than by angle**. For unit vectors a larger dot product *is* a smaller angle, so converting to distances would only be undone by the comparison — two dozen transcendental calls a sample, to sort numbers that were already in order.

**The comparison chain is load-bearing and must be transcribed exactly.** Python is:

```python
        if alignment > best_dot:
            second, second_dot = best, best_dot
            best, best_dot = plate, alignment
        elif alignment > second_dot:
            second, second_dot = plate, alignment
```

Both comparisons are strict `>`, so **a tie keeps the earlier plate** — with equal alignments the lower index wins, and that is what makes the answer stable rather than dependent on iteration order. Both accumulators start at `-2.0`, which is below any possible dot product of unit vectors.

- [ ] **Step 1: Write the failing test**

```rust
    #[test]
    fn a_point_on_a_seed_belongs_to_that_plate() {
        let set = two_plates();
        let (best, second) = set.nearest_two(&set.plate(0).seed);
        assert_eq!(best.expect("a nearest").index, 0);
        assert_eq!(second.expect("a second").index, 1);
    }

    #[test]
    fn the_second_is_the_other_one() {
        let set = two_plates();
        let (best, second) = set.nearest_two(&set.plate(1).seed);
        assert_eq!(best.expect("a nearest").index, 1);
        assert_eq!(second.expect("a second").index, 0);
    }

    #[test]
    fn a_tie_keeps_the_earlier_plate() {
        // Equidistant from both seeds. Both comparisons are strict, so the lower index
        // wins and the answer does not depend on iteration order.
        let set = two_plates();
        let midpoint = SpherePoint::from_latlon(0.0, 45.0);
        let (best, _) = set.nearest_two(&midpoint);
        assert_eq!(best.expect("a nearest").index, 0);
    }

    #[test]
    fn an_empty_set_has_no_nearest() {
        let set = PlateSet::new(vec![]);
        let (best, second) = set.nearest_two(&SpherePoint::from_latlon(0.0, 0.0));
        assert!(best.is_none() && second.is_none());
    }

    #[test]
    fn a_single_plate_has_no_second() {
        let only = Plate {
            index: 0,
            seed: SpherePoint::from_latlon(5.0, 5.0),
            euler_pole: SpherePoint::from_latlon(90.0, 0.0),
            rate_rad_per_myr: 0.0,
        };
        let set = PlateSet::new(vec![only]);
        let (best, second) = set.nearest_two(&SpherePoint::from_latlon(0.0, 0.0));
        assert_eq!(best.expect("a nearest").index, 0);
        assert!(second.is_none());
    }
```

The tie test depends on `from_latlon(0.0, 45.0)` being *exactly* equidistant in floating point from the two seeds. It may not be. **Run it and see**: if the alignments differ in the last bits the test will pick whichever is genuinely nearer, which is correct behaviour but does not test the tie. If that happens, construct the tie directly — take the two seed vectors, add them, normalise, and use that — and say in your report what you did.

- [ ] **Step 2: Run and confirm it fails**

- [ ] **Step 3: Write the implementation**

```rust
    /// The two plates whose seeds are closest.
    ///
    /// Compared by dot product rather than by angle: for unit vectors a larger dot product
    /// *is* a smaller angle, so converting to distances would only be undone by the
    /// comparison — two dozen transcendental calls a sample, to sort numbers that were
    /// already in order.
    ///
    /// Both comparisons are strict, so a tie keeps the earlier plate and the answer does
    /// not depend on iteration order.
    pub fn nearest_two(&self, point: &SpherePoint) -> (Option<Plate>, Option<Plate>) {
        let v = point.vector;
        let (px, py, pz) = (v.x, v.y, v.z);
        let mut best: Option<Plate> = None;
        let mut second: Option<Plate> = None;
        let mut best_dot = -2.0f64;
        let mut second_dot = -2.0f64;
        for plate in &self.plates {
            let s = plate.seed.vector;
            let alignment = px * s.x + py * s.y + pz * s.z;
            if alignment > best_dot {
                second = best;
                second_dot = best_dot;
                best = Some(*plate);
                best_dot = alignment;
            } else if alignment > second_dot {
                second = Some(*plate);
                second_dot = alignment;
            }
        }
        (best, second)
    }
```

Note the dot product is written out in the Python's term order — `px * s.x + py * s.y + pz * s.z`, left-associated — rather than calling `Vec3::dot`. `Vec3::dot` computes the same expression in the same order, so either is correct; writing it out matches the Python's unrolled form literally. If you prefer `v.dot(&s)`, confirm from `vectors.rs` that its term order is identical and say so in your report.

- [ ] **Step 4: Run and confirm the tests pass**

- [ ] **Step 5: Commit**

```bash
git add crates/worldbuilder-engine/src/plates.rs
git commit -m "engine: nearest two, by dot product because the angle would be undone"
```

---

### Task 4: Bindings and conformance

**Files:**
- Modify: `crates/worldbuilder-engine/src/bindings.rs`, `crates/worldbuilder-engine/src/lib.rs`
- Modify: `tests/test_conformance.py`

**Interfaces:**
- `plate_angular_velocity(pole_x, pole_y, pole_z, rate) -> (f64, f64, f64)`
- `plateset_bisector(seeds_flat, a, b) -> Option<(f64, f64, f64)>`
- `plateset_nearest_two(seeds_flat, x, y, z) -> (Option<usize>, Option<usize>)`

`seeds_flat` is a flat list of seed components — `[x0, y0, z0, x1, y1, z1, ...]` — so the harness can build an arbitrary set without a class boundary. The bindings reconstruct a `PlateSet` from it, giving each plate its position as `index` and a placeholder pole and rate, since neither affects the two functions under test. Say in your report that you checked that claim rather than assuming it.

**Everything in this slice is held STRICTLY, with `same()`.** There is no transcendental in any path here: `nearest_two` is multiplies and adds, and the bisector table is a subtraction, a `length()` and a `normalised()`, all IEEE-exact or correctly-rounded. **If any comparison fails, that is a real defect** — do not reach for `close_enough`.

- [ ] **Step 1: Add the bindings and register them.** Conversion only, no arithmetic.

- [ ] **Step 2: Rebuild** with `maturin develop --release` into the project venv.

- [ ] **Step 3: Add the conformance tests**, covering:
  - `angular_velocity` over a range of poles and rates including zero and negative.
  - The bisector table for a generated set, every ordered pair, including the `None` entries — **assert the Rust is `None` exactly where the Python is, not merely that the vectors match where both exist.** A table that silently had a vector where Python has `None` would pass a value-only comparison.
  - `nearest_two` over a corpus of sphere points, comparing **both** returned indices.
  - The poles and the meridian explicitly.
  - A set with coincident seeds, so the `DEGENERATE` branch is exercised against the Python rather than only in a unit test.

Build the Python side with `PlateSet` from `worldbuilder.plates.lookup` and `Plate` from `worldbuilder.plates.model`, constructing plates directly rather than through the generator — the generator is not ported until the next slice and is not needed here.

- [ ] **Step 4: Run the conformance suite and the whole suite**, quoting both.

- [ ] **Step 5: Commit**

---

### Task 5: Record it

**Files:**
- Modify: `crates/worldbuilder-engine/README.md`

- [ ] **Step 1** — add `src/plates.rs` to the listing. Record that this slice is **entirely strict**, and why: no transcendental appears in any path, so every comparison is exact and there is no bound to fall back on. Record the decision not to port the Python's duplicated component-triple table and the reasoning (a Rust field access is free; one representation cannot fall out of step with itself), noting it as the same call made for `Noise`'s corner cache. Verify all test counts by running the suites.

- [ ] **Step 2** — commit.

---

## What this slice deliberately does not do

- **No margins.** `margin_at`, `margins_within`, `flattened` and `margin_normal` are the other half of `lookup.py` — about two hundred lines — and become their own slice.
- **No plate generation.** `generation.py` derives plate seeds, poles and rates from `blake2b` over a UTF-8 string built by joining `str(part)` with `|`. That is a byte-level string-construction port feeding a cryptographic hash, and it needs a new engine dependency; it deserves its own slice and its own care.
- **No kinematics, and no tectonics.** Both wait on the above.
- **No deletion of the Python.** It stays the reference.
