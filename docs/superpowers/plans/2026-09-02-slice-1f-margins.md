# Slice 1f — Margins — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port `margin_at`, `flattened` and `margin_normal` from `worldbuilder/plates/lookup.py`, fix the plate binding contract that slice 1e flagged, and prove the lot against the Python.

**Architecture:** A margin is where a point stands relative to the edge of its plate: which plate it is on, which plate lies across the nearest stretch of that edge, and how far away that edge is. `margin_normal` then answers which way is *across* the margin in the tangent plane — the direction the kinematics need in order to tell whether two plates approach each other across their boundary or slide along it.

**Tech Stack:** Rust (stable 1.98.0), `libm` 0.2.11, PyO3, maturin, Python 3.11, pytest.

**Spec:** `docs/design/2026-09-02-mark-2-world-studio.md` sections 4.2, 4.3, 4.4.

**Prior slices:** 1a-1e built the crate, `detmath`, the geometry layer, `Noise`, `Continentality`, and `Plate`/`PlateSet` with the Voronoi lookup. Read the conformance section of `crates/worldbuilder-engine/README.md` first — particularly the constants rules and the recorded limitation of the plate bindings, which this slice must fix before its conformance results mean anything.

## The theme this module is built around

`lookup.py` records four bugs found by testing, and names the pattern behind them: **a hard decision taken on a continuous quantity**. Each fix replaced a discrete choice with something continuous, and the numbers are in the source:

- Measuring only the second-nearest plate's bisector — nearly always the right one, and a single arc sine — jumped by **five hundred kilometres** as the runner-up changed, because `|A - B|` and `|A - C|` differ so the normalisation is discontinuous even though the numerator is not.
- Picking *one* margin when two are equidistant flipped the neighbour's identity, and everything derived from it, under a step of a metre: **five hundred metres of cliff**.
- Counting bisectors that are not actually margins at that point cost **a hundred and seventy kilometres of phantom mountain range**, and crossing a cell boundary swapped one imaginary plane for another: **two hundred and sixty metres of cliff**.
- Rejecting shadowed bisectors with a boolean rather than fading them: **a hundred and forty metres of cliff**, and the source notes this was the third time the same mistake appeared in one phase.

Only the first belongs to this slice; the rest live in `margins_within`, which is its own. But the pattern is why `margin_at` looks the way it does, and it should survive the port intact. **Do not "simplify" the minimum-over-all-bisectors into a second-nearest shortcut** — that is the exact bug the loop exists to avoid.

## Contract classification

This module splits unusually, and the split is worth understanding before writing the tests:

| function | path | contract |
|---|---|---|
| `margin_at`'s **neighbour selection** | dot products, `abs`, comparison | **strict** — the discrete choice is made on exactly-computed values |
| `margin_at`'s **distance** | `asin` | **bounded**, 4 ULP |
| `flattened` | subtract, dot, scale, `length`, `normalised` | **strict** — `sqrt` only |
| `margin_normal` | same as `flattened` | **strict** |

That the *selection* is strict is a real property, not luck: the sines are dot products of exactly-representable quantities, so which bisector is nearest is decided without a transcendental anywhere. Only the final conversion to metres costs a bound. **The conformance tests must compare the selected neighbour's index with `same()`-grade exactness and the distance with `close_enough` — not both loosely.**

## Global Constraints

- Rust is at `~/.cargo/bin`, NOT on PATH in a fresh shell — begin every bash call with `export PATH="$HOME/.cargo/bin:$PATH"`.
- Python is the project venv: `PYTHONPATH=/d/dev/worldbuilder_by_aetos .venv/Scripts/python -m pytest tests/ -q`.
- Run cargo from the repository root with `-p worldbuilder-engine`, dev profile, never `--release`.
- **No std float maths.** Everything through `detmath`. The guard bans method form, the `f64::` call form, `mul_add`, and bare integer casts without `// cast-ok: <reason>`.
- **Constants transcribed from the Python are written without underscore separators**, character-identical to their source.
- **A floating-point sign or zero assertion must carry the reason it holds.** Three appeared in slice 1e: two were false, one was true but undocumented. If you write one, write why.
- Nothing under `worldbuilder/` is modified. `worldbuilder/integration/maritime.py` has a pre-existing uncommitted change — leave it unstaged.

---

## File Structure

    crates/worldbuilder-engine/src/plates.rs      gains Margin, margin_at, flattened, margin_normal
    crates/worldbuilder-engine/src/bindings.rs    plate bindings gain real poles and rates
    tests/test_conformance.py                     gains a margin section

---

### Task 1: The binding contract, fixed first

**Files:**
- Modify: `crates/worldbuilder-engine/src/bindings.rs`
- Modify: `tests/test_conformance.py`

**Interfaces:**
- Changes `plateset_bisector` and `plateset_nearest_two` to take poles and rates alongside seeds; adds a shared helper that rebuilds a `PlateSet` from all three.

**This task exists because slice 1e's review found the current bindings would produce false conformance results here.** They rebuild a `PlateSet` from seed components alone, fabricating `pole = seed` and `rate = 0.0`. That is provably inert for `bisector` and `nearest_two`, which read only the seed — but `Margin` carries whole `Plate` values, so once `margin_at` is exposed the same way, both sides would compare fabricated poles against identically fabricated poles and pass trivially. False confidence rather than conformance.

Fix the contract before anything depends on it.

- [ ] **Step 1: Extend the binding signatures**

Take three flat lists rather than one — seeds, poles, and rates — so the harness supplies real, independently-varying values:

```rust
fn plateset_from_parts(seeds: Vec<f64>, poles: Vec<f64>, rates: Vec<f64>) -> crate::plates::PlateSet {
    let plates = seeds
        .chunks_exact(3)
        .zip(poles.chunks_exact(3))
        .zip(rates.iter())
        .enumerate()
        .map(|(index, ((seed, pole), rate))| crate::plates::Plate {
            index,
            seed: SpherePoint { vector: Vec3::new(seed[0], seed[1], seed[2]) },
            euler_pole: SpherePoint { vector: Vec3::new(pole[0], pole[1], pole[2]) },
            rate_rad_per_myr: *rate,
        })
        .collect();
    crate::plates::PlateSet::new(plates)
}
```

Update `plateset_bisector` and `plateset_nearest_two` to take the extra arguments and delegate to this helper. Their behaviour must not change — the poles and rates are still unread by those two functions, which is the point: the harness now supplies real ones so that later functions which *do* read them are genuinely tested.

- [ ] **Step 2: Update the existing plate conformance tests** to pass real poles and rates rather than relying on the old signature. Vary them per plate — a pole derived from the seed by some rotation, and a rate that differs per index — so no two plates share either.

- [ ] **Step 3: Rebuild and confirm the existing plate tests still pass** exactly as before. They compare `bisector` and `nearest_two`, which do not read the new fields, so **every one of them must still agree bit-for-bit**. If any changes, the helper is not reconstructing plates faithfully and that is a real finding.

- [ ] **Step 4: Commit**

```bash
git add crates/worldbuilder-engine/src/bindings.rs tests/test_conformance.py
git commit -m "engine: plate bindings carry real poles and rates, before anything reads them"
```

---

### Task 2: Margin, and the distance to the plate's edge

**Files:**
- Modify: `crates/worldbuilder-engine/src/plates.rs`

**Interfaces:**
- Produces: `plates::Margin { nearest: Option<Plate>, neighbour: Option<Plate>, distance_m: f64 }` and `PlateSet::margin_at(&self, point: &SpherePoint, radius_m: f64) -> Margin`.

**The minimum over every bisector of the nearest plate, and it has to be.** The obvious shortcut — measure only the bisector with the second-nearest seed — is nearly always right and is a single arc sine. It is also discontinuous, and the walk-across-a-margin test caught it jumping by five hundred kilometres: the distance to a bisector is `asin(dot(P, normalise(A - B)))`, and when the runner-up changes from B to C the numerator is continuous but the normalisation is not, because `|A - B|` and `|A - C|` differ. Terrain built on that would have grown a wall wherever a third plate happened to become the runner-up.

A minimum of continuous functions is continuous, and it is also the honest answer: the distance to the plate's actual edge rather than to one particular neighbour's bisector.

**The minimum is taken on the sine, not the angle.** Arc sine is monotonic over the range in question, so the smallest sine is the smallest angle, and one transcendental call at the end does for the lot.

- [ ] **Step 1: Write the failing test**

```rust
    #[test]
    fn a_single_plate_has_no_margin() {
        let only = Plate {
            index: 0,
            seed: SpherePoint::from_latlon(5.0, 5.0),
            euler_pole: SpherePoint::from_latlon(90.0, 0.0),
            rate_rad_per_myr: 0.0,
        };
        let set = PlateSet::new(vec![only]);
        let m = set.margin_at(&SpherePoint::from_latlon(0.0, 0.0), EARTH_RADIUS_M);
        assert_eq!(m.nearest.expect("a nearest").index, 0);
        assert!(m.neighbour.is_none());
        assert!(m.distance_m.is_infinite());
    }

    #[test]
    fn a_point_on_the_bisector_is_at_zero_distance() {
        // Equidistant from both seeds, so it stands on their shared edge.
        let set = two_plates();
        let a = set.plate(0).seed.vector;
        let b = set.plate(1).seed.vector;
        let midpoint = SpherePoint {
            vector: a.add(&b).normalised().expect("distinct seeds"),
        };
        let m = set.margin_at(&midpoint, EARTH_RADIUS_M);
        assert!(m.distance_m.abs() < 1e-6, "distance was {}", m.distance_m);
        assert!(m.neighbour.is_some());
    }

    #[test]
    fn a_point_at_a_seed_is_a_quarter_turn_from_the_edge() {
        // With two seeds ninety degrees apart, standing on one puts the shared edge
        // forty-five degrees away.
        let set = two_plates();
        let m = set.margin_at(&set.plate(0).seed, EARTH_RADIUS_M);
        let expected = (std::f64::consts::PI / 4.0) * EARTH_RADIUS_M;
        assert!((m.distance_m - expected).abs() < 1.0, "distance was {}", m.distance_m);
        assert_eq!(m.neighbour.expect("a neighbour").index, 1);
    }

    #[test]
    fn the_distance_never_exceeds_a_quarter_turn() {
        // asin caps at pi/2, and the sine is clamped to 1.0 before it.
        let set = two_plates();
        for lat in (-80..81).step_by(20) {
            for lon in (-180..181).step_by(45) {
                let m = set.margin_at(&SpherePoint::from_latlon(lat as f64, lon as f64), EARTH_RADIUS_M);
                assert!(m.distance_m <= (std::f64::consts::PI / 2.0) * EARTH_RADIUS_M + 1.0);
            }
        }
    }
```

`two_plates()` already exists in the test module from slice 1e.

- [ ] **Step 2: Run and confirm it fails**

- [ ] **Step 3: Write the implementation**

```rust
/// Where a point stands relative to the edge of its plate.
#[derive(Debug, Clone, Copy)]
pub struct Margin {
    /// The plate the point is on.
    pub nearest: Option<Plate>,
    /// The plate across the nearest stretch of that edge.
    pub neighbour: Option<Plate>,
    /// Metres to it, along the surface.
    pub distance_m: f64,
}
```

and inside `impl PlateSet`:

```rust
    /// How far a point is from the edge of the plate it is on.
    ///
    /// **The minimum over every bisector of the nearest plate**, and it has to be. The
    /// obvious shortcut is to measure only the bisector with the second-nearest seed,
    /// which is nearly always the right one and is a single arc sine. It is also
    /// discontinuous, and the walk-across-a-margin test caught it jumping by five hundred
    /// kilometres: the distance to a bisector is `asin(dot(P, normalise(A - B)))`, and
    /// when the runner-up changes from B to C the numerator is continuous but the
    /// normalisation is not, because `|A - B|` and `|A - C|` differ. Terrain built on that
    /// would have grown a wall wherever a third plate became the runner-up.
    ///
    /// The minimum is taken on the sine rather than the angle: arc sine is monotonic over
    /// the range in question, so the smallest sine is the smallest angle, and one
    /// transcendental call at the end does for the lot.
    pub fn margin_at(&self, point: &SpherePoint, radius_m: f64) -> Margin {
        let (nearest, _) = self.nearest_two(point);
        if self.plates.len() < 2 {
            return Margin { nearest, neighbour: None, distance_m: f64::INFINITY };
        }

        let near = nearest.expect("a set with two or more plates has a nearest");
        let v = point.vector;
        let (px, py, pz) = (v.x, v.y, v.z);
        let mut closest_sine = 2.0f64;
        let mut across: Option<Plate> = None;
        for (other_index, other) in self.plates.iter().enumerate() {
            let normal = match self.bisector(near.index, other_index) {
                Some(n) => n,
                None => continue,
            };
            let offset = (px * normal.x + py * normal.y + pz * normal.z).abs();
            if offset < closest_sine {
                closest_sine = offset;
                across = Some(*other);
            }
        }

        // Python writes `min(1.0, closest_sine)`; two-argument min keeps 1.0 unless the
        // value is strictly below it, so a NaN would clamp to 1.0 rather than propagate.
        let clamped = if closest_sine < 1.0 { closest_sine } else { 1.0 };
        Margin { nearest, neighbour: across, distance_m: m::asin(clamped) * radius_m }
    }
```

Note `closest_sine` starts at `2.0`, above any possible `abs` of a dot product of unit vectors, and the comparison is strict `<`, so **a tie keeps the earlier plate** — consistent with `nearest_two`, and for the same reason: it makes the answer independent of iteration order.

Note also the Python indexes `self._bisector_xyz[nearest.index]` by *position in the plates tuple*, pairing it with `zip(self.plates, ...)`. The Rust uses `enumerate()` to get the same position. Confirm that `bisector(near.index, other_index)` indexes the table the same way the Python's `[nearest.index][index]` does — if `Plate::index` ever differed from a plate's position in the set, these would diverge. Say in your report that you checked.

- [ ] **Step 4: Run and confirm the tests pass**

- [ ] **Step 5: Commit**

---

### Task 3: Across the margin, in the tangent plane

**Files:**
- Modify: `crates/worldbuilder-engine/src/plates.rs`

**Interfaces:**
- Produces: `PlateSet::flattened(&self, point: &SpherePoint, normal: &Vec3) -> Option<Vec3>` and `PlateSet::margin_normal(&self, point: &SpherePoint, margin: &Margin) -> Option<Vec3>`.

Wanted by the kinematics, which need to know whether two plates approach each other *across* their margin or slide *along* it. The bisector's plane normal is already perpendicular to the margin; what is returned is its component in the tangent plane, which is what "away from the margin" means to somebody standing there.

Both return `None` where the direction is undefined — at the two points where the normal is parallel to the surface normal, and where there is no neighbour at all.

- [ ] **Step 1: Write the failing test**

```rust
    #[test]
    fn a_flattened_normal_lies_in_the_tangent_plane() {
        let set = two_plates();
        let p = SpherePoint::from_latlon(20.0, 40.0);
        let normal = set.bisector(0, 1).expect("distinct seeds");
        let flat = set.flattened(&p, &normal).expect("not degenerate here");
        // Perpendicular to up, and unit length.
        assert!(flat.dot(&p.vector).abs() < 1e-12, "dot was {}", flat.dot(&p.vector));
        assert!((flat.length() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn a_normal_parallel_to_up_has_no_flat_direction() {
        // Standing exactly where the bisector's normal points straight up leaves no
        // component in the tangent plane, so there is no "across" to report.
        let set = two_plates();
        let normal = set.bisector(0, 1).expect("distinct seeds");
        let straight_up = SpherePoint { vector: normal };
        assert!(set.flattened(&straight_up, &normal).is_none());
    }

    #[test]
    fn a_margin_with_no_neighbour_has_no_normal() {
        let only = Plate {
            index: 0,
            seed: SpherePoint::from_latlon(5.0, 5.0),
            euler_pole: SpherePoint::from_latlon(90.0, 0.0),
            rate_rad_per_myr: 0.0,
        };
        let set = PlateSet::new(vec![only]);
        let p = SpherePoint::from_latlon(0.0, 0.0);
        let margin = set.margin_at(&p, EARTH_RADIUS_M);
        assert!(set.margin_normal(&p, &margin).is_none());
    }

    #[test]
    fn the_margin_normal_points_across_the_margin() {
        let set = two_plates();
        let p = SpherePoint::from_latlon(10.0, 30.0);
        let margin = set.margin_at(&p, EARTH_RADIUS_M);
        let across = set.margin_normal(&p, &margin).expect("a neighbour exists here");
        assert!(across.dot(&p.vector).abs() < 1e-12);
        assert!((across.length() - 1.0).abs() < 1e-12);
    }
```

- [ ] **Step 2: Run and confirm it fails**

- [ ] **Step 3: Write the implementation**

```rust
    /// A bisector normal laid flat on the surface at a point.
    pub fn flattened(&self, point: &SpherePoint, normal: &Vec3) -> Option<Vec3> {
        let v = point.vector;
        let flat = normal.sub(&v.scaled(v.dot(normal)));
        if flat.length() <= DEGENERATE {
            return None;
        }
        flat.normalised()
    }

    /// Which way is across the margin, in the tangent plane at this point.
    ///
    /// Wanted by the kinematics, which need to know whether two plates approach each other
    /// *across* their margin or slide *along* it. The bisector's plane normal is already
    /// perpendicular to the margin; this is its component in the tangent plane, which is
    /// what "away from the margin" means to somebody standing there.
    pub fn margin_normal(&self, point: &SpherePoint, margin: &Margin) -> Option<Vec3> {
        let neighbour = margin.neighbour?;
        let nearest = margin.nearest?;
        let normal = self.bisector(nearest.index, neighbour.index)?;
        let v = point.vector;
        let flattened = normal.sub(&v.scaled(v.dot(&normal)));
        if flattened.length() <= DEGENERATE {
            return None;
        }
        flattened.normalised()
    }
```

Note `margin_normal` repeats `flattened`'s body rather than calling it. **That is what the Python does** — `margin_normal` inlines the same three lines rather than calling `flattened`. Transcribing the duplication keeps the port literal; if you would rather call `flattened` and can show the arithmetic is identical, say so in your report and do that instead. Either is acceptable, but state which and why.

The Python's `margin_normal` also does not guard `margin.nearest` being `None`, because it cannot be when a neighbour exists. The Rust's `?` on both is a total function where the Python would raise; note that difference in your report.

- [ ] **Step 4: Run and confirm the tests pass**

- [ ] **Step 5: Commit**

---

### Task 4: Conformance

**Files:**
- Modify: `crates/worldbuilder-engine/src/bindings.rs`, `crates/worldbuilder-engine/src/lib.rs`
- Modify: `tests/test_conformance.py`

**Interfaces:**
- `plateset_margin_at(seeds, poles, rates, x, y, z, radius_m) -> (Option<usize>, Option<usize>, f64)` — nearest index, neighbour index, distance.
- `plateset_margin_normal(seeds, poles, rates, x, y, z, radius_m) -> Option<(f64, f64, f64)>`.
- `plateset_flattened(seeds, poles, rates, x, y, z, nx, ny, nz) -> Option<(f64, f64, f64)>`.

**Apply the split contract, and do not blur it:**
- The **nearest and neighbour indices** are decided by dot products and `abs` with no transcendental anywhere. Compare them as exact integers. **A mismatch is a real defect, not a rounding artefact.**
- The **distance** goes through `asin`. Compare with `close_enough` at the existing bound.
- `flattened` and `margin_normal` return vectors computed with `sqrt` only. Compare with `same()`, **strictly**.
- The `None` cases must be compared positionally — assert the Rust returns `None` exactly where the Python does, not merely that vectors agree where both exist.

Cover: a corpus of sphere points against a multi-plate set; the poles and meridian; a single-plate set (infinite distance, no neighbour); and points close to a bisector where the neighbour selection is most likely to differ.

**A hazard worth measuring rather than assuming.** The neighbour selection is a discrete choice — the minimum over bisector sines — and although those sines are computed strictly, two of them could be equal or near-equal at a point equidistant from two margins. Slice 1d hit the same shape with the calibration quantile and measured the margin of safety rather than guessing. Do the same here: for the corpus, record how close the two smallest sines get, and report the minimum gap observed. If the gap is comparable to a ULP anywhere, say so — it would mean neighbour selection is genuinely fragile at that point rather than merely discrete.

- [ ] **Step 1: Add the bindings and register them.** Conversion only, no arithmetic.
- [ ] **Step 2: Rebuild** with `maturin develop --release` into the project venv.
- [ ] **Step 3: Add the conformance tests.**
- [ ] **Step 4: Run the conformance suite and the whole suite**, quoting both, and report the minimum sine gap.
- [ ] **Step 5: Commit**

---

### Task 5: Record it

**Files:**
- Modify: `crates/worldbuilder-engine/README.md`

- [ ] **Step 1** — record: that `margin_at` splits across both contracts, with the *selection* strict and only the distance bounded, and why that is a property rather than luck; the minimum-over-all-bisectors rule and the five-hundred-kilometre jump that made it necessary; that the binding limitation flagged in 1e is now **fixed**, with real poles and rates carried from the harness; and the minimum sine gap measured in Task 4. Verify all test counts by running the suites.

- [ ] **Step 2** — commit.

---

## What this slice deliberately does not do

- **No `margins_within`.** It is a hundred lines with a nested loop and three separate bug-fixes encoded in it — the shadow weight, the phantom-bisector test, and the fade that replaced a boolean. It gets its own slice.
- **No kinematics or tectonics.** Both wait on `margins_within`.
- **No plate generation.** `generation.py` needs `blake2b` and a byte-level string port.
- **No deletion of the Python.** It stays the reference.
