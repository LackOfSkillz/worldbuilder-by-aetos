# Islands, slice 1: peaks — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Sparse volcanic peaks rising out of deep ocean, as an opt-in parameter block that leaves every existing world bit-identical when absent.

**Architecture:** A new `PeakParams` block on `Tectonics`. The term is a cellular (Worley-style) field over `Noise::lattice_at` — one candidate peak per lattice node, existence and height hashed per node, position jittered within the cell — windowed to deep seabed and added inside `Tectonics::offset_m`. Because `Shelf::evaluate` puts that offset into both `macro_elevation` and `Reading.tectonic_m`, the peak lands in `structural_m` and `elevation_m` at once, and three downstream consumers key off `tectonic_m` in the way a volcanic island wants: the shelf is held off (`authority` → 0), detail roughness defers to it (quieting saturates), and the substrate reads rock.

**Tech Stack:** Rust (`crates/worldbuilder-engine`), the wasm C ABI in `wasm.rs`, and the studio's plain-JS param modules under `viewer/public/app/`.

**Spec:** `docs/superpowers/specs/2026-09-14-islands-design.md` — read §1, §2.1, §2.2, §3, §4.1, §5 and §6.

## Global Constraints

- **An absent block must produce bit-identical worlds.** Not close — identical. The parity corpus must report 0 divergent values and every control must be unmoved. If that cannot be made to pass, the design is wrong, not the test.
- **`GENERATOR_VERSION` must NOT be bumped.** The bump test in `lib.rs:65-71` is "the same seed *and the same parameters* through the new code would produce a different world." With the block absent it would not.
- No `std` float maths outside `detmath.rs`. Route everything through `crate::detmath as m`.
- **No `ceil`** — write `-m::floor(-x)`.
- **No `.abs()` anywhere in this work.** `tests/no_std_math.rs` holds a per-file ledger (`tectonics.rs`: 9, `continentality.rs`: 9, `shelf.rs`: 15) and "the count may only ever go down". A new `.abs()` in any ledgered file fails the build at once, and there is no escape marker for it. Write the comparison out.
- **No `f64::max`, `f64::min`, or `.clamp()`.** These are NaN-asymmetric and the static guard does NOT catch them. Use the house forms: `if a > b { a } else { b }` keeping the first operand unless the second is strictly beyond it; `detail::smooth` (re-exported by `shelf.rs:83`) for clamp-and-smoothstep; `continentality::coast_window`'s explicit three-arm shape when a NaN must **close** a window rather than open it. Note `smooth(NaN)` returns **1.0** and is therefore not a NaN barrier.
- `// cast-ok: <reason>` on every `as u32/u64/i32/i64` and every float-to-`usize` cast, and the reason must say what makes it safe.
- **Lattice coordinates are never derived by a bare cast.** Copy `noise.rs:166-168` exactly: `m::floor`, then bound against `LATTICE_LIMIT` with the negated `>=`/`<=` pair so a NaN takes the branch, *then* `as i64` with the marker.
- No panic reachable from an `extern "C"` boundary — wasm exports return status codes.
- Never iterate a `HashMap`/`HashSet` into output; `total_cmp` for float sorts.
- **Do not add a field to `Surface`.** `lib.rs:416-443` reads `surface.rs`'s source text and asserts `Surface` has exactly eight public fields by name, plus `steer` as the named ninth. Thread the block through `Tectonics` instead.
- `bindings.rs` is untouched by this plan. It exposes no parameter block at all, by design — the Python side is the conformance oracle and asks only canonical questions.
- Every figure in a report names its population, its method with parameters, and its host.

---

### Task 1: The peak field, standalone

**Files:**
- Modify: `crates/worldbuilder-engine/src/tectonics.rs`
- Test: in-file `#[cfg(test)]` module of `tectonics.rs`

**Interfaces:**
- Consumes: `Noise::lattice_at(ix, iy, iz) -> f64` in `[0,1)` (`noise.rs:94`, `pub(crate)`); `Noise::new(seed, salt)` (`noise.rs:53`); `detail::smooth` (`detail.rs:449`); `crate::detmath as m`.
- Produces: `pub struct PeakParams` with `canonical()` and `volcanic()`; `const PEAK_SALT`, `PEAK_JITTER_SALT`, `PEAK_HEIGHT_SALT`; `fn peak_offset_m(&self, point: &SpherePoint, seabed_m: f64) -> f64` on `Tectonics`.

- [ ] **Step 1: Write the failing tests**

Add to `tectonics.rs`'s test module:

```rust
#[test]
fn a_peak_field_with_no_density_is_exactly_zero() {
    // The inert arm must be an EARLY RETURN, not `+ 0.0`. Adding an exactly-zero
    // offset to a -0.0 seabed yields +0.0 and flips the sign bit, which is how a
    // block that is supposed to change nothing changes something.
    let params = PeakParams { density: 0.0, ..PeakParams::volcanic() };
    let tectonics = peaked(params);
    for (lat, lon) in [(0.0, 0.0), (12.5, -47.5), (-63.25, 128.75), (89.0, 180.0)] {
        let point = SpherePoint::from_latlon(lat, lon);
        let got = tectonics.peak_offset_m(&point, -4600.0);
        assert_eq!(got.to_bits(), 0.0f64.to_bits(), "at {lat},{lon}");
    }
}

#[test]
fn a_peak_needs_deep_water_under_it() {
    // The window is a depth, so it can be stated as one. Shallow seabed gets nothing
    // however dense the field, which is what stops an island erupting on a shelf.
    let tectonics = peaked(PeakParams { density: 1.0, ..PeakParams::volcanic() });
    let mut shallow = 0usize;
    for i in 0..400 {
        let point = SpherePoint::from_latlon(-80.0 + 0.4 * f64::from(i), 17.0); // cast-ok: loop counter, 0..400
        if tectonics.peak_offset_m(&point, -100.0) != 0.0 {
            shallow += 1;
        }
    }
    assert_eq!(shallow, 0, "a 100 m seabed is not deep enough for any peak");
}

#[test]
fn a_dense_field_puts_peaks_in_deep_water_and_a_sparse_one_puts_fewer() {
    // Density and height are independent knobs. This is the property a thresholded
    // fbm cannot give, and the reason this term is built on the lattice.
    let dense = peaked(PeakParams { density: 0.50, ..PeakParams::volcanic() });
    let sparse = peaked(PeakParams { density: 0.05, ..PeakParams::volcanic() });
    let (mut d, mut s) = (0usize, 0usize);
    for i in 0..2_000 {
        let lat = -70.0 + 0.07 * f64::from(i); // cast-ok: loop counter, 0..2000
        let point = SpherePoint::from_latlon(lat, 0.37 * f64::from(i) - 180.0); // cast-ok: loop counter
        if dense.peak_offset_m(&point, -4600.0) > 0.0 { d += 1; }
        if sparse.peak_offset_m(&point, -4600.0) > 0.0 { s += 1; }
    }
    assert!(d > 0, "a half-dense field found no peaks in 2,000 deep probes");
    assert!(d > s, "denser must mean more: dense {d}, sparse {s}");
}

#[test]
fn a_peak_is_tall_enough_to_break_the_surface() {
    // The number that defeats the island arc. 700 m of arc against 4,600 m of abyss
    // surfaces nothing; this term exists to clear that, so assert it does.
    let tectonics = peaked(PeakParams { density: 1.0, ..PeakParams::volcanic() });
    let mut tallest = 0.0f64;
    for i in 0..5_000 {
        let point = SpherePoint::from_latlon(
            -60.0 + 0.024 * f64::from(i), // cast-ok: loop counter, 0..5000
            0.29 * f64::from(i) - 180.0,  // cast-ok: loop counter, 0..5000
        );
        let got = tectonics.peak_offset_m(&point, -4600.0);
        if got > tallest { tallest = got; }
    }
    assert!(tallest > 4_600.0, "tallest peak {tallest} m does not clear the abyss");
}

#[test]
fn the_peak_field_never_answers_a_nan_or_an_infinity() {
    let tectonics = peaked(PeakParams { density: 1.0, ..PeakParams::volcanic() });
    for i in 0..3_000 {
        let point = SpherePoint::from_latlon(-89.0 + 0.06 * f64::from(i), 0.0); // cast-ok: loop counter
        for seabed in [-4600.0, -150.0, 0.0, 700.0, f64::NAN] {
            let got = tectonics.peak_offset_m(&point, seabed);
            assert!(got.is_finite(), "peak_offset_m({seabed}) = {got}");
            assert!(got >= 0.0, "a peak may only ever raise ground, got {got}");
        }
    }
}
```

Add the helper beside them:

```rust
fn peaked(params: PeakParams) -> Tectonics {
    let land = crate::continentality::Continentality::new(
        7788, crate::sphere::EARTH_RADIUS_M, 0.4,
    );
    Tectonics::with_peaks(7788, crate::sphere::EARTH_RADIUS_M, &land, 22, Some(params))
}
```

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo test --release -p worldbuilder-engine --lib tectonics::tests -- peak`
Expected: compile failure — `PeakParams` and `with_peaks` do not exist.

- [ ] **Step 3: Write `PeakParams` and its salts**

Three fresh salts. Read `tectonics.rs:343-345` first and match the ASCII-literal style; the existing three are `"structur"`, `"segments"` and `"wanderin"`. State in the doc comment *which* salts these must differ from and why, imitating `continentality.rs:40-47`.

```rust
/// Where a seamount stands, hashed per lattice node.
///
/// Distinct from `STRUCTURE_SALT`, `SEGMENTATION_SALT` and `MARGIN_WARP_SALT`, and it has to
/// be: a shared salt would put every island on a margin crest, which is the one place this
/// term is not meant to put them. Three salts rather than one because existence, position
/// and height must be independent -- drawn from a single field, a tall peak would always sit
/// in the same corner of its cell.
const PEAK_SALT: u64 = 0x7365_616D_6F75_6E74; // "seamount"
const PEAK_JITTER_SALT: u64 = 0x6A69_7474_6572_6564; // "jittered"
const PEAK_HEIGHT_SALT: u64 = 0x7374_616E_6469_6E67; // "standing"

/// How tall a full-height peak stands, in metres above the seabed it sits on.
const VOLCANIC_HEIGHT_M: f64 = 5_200.0;
/// What share of lattice cells hold a peak at the named preset.
const VOLCANIC_DENSITY: f64 = 0.06;
/// How far a cone reaches from its centre, in metres.
const VOLCANIC_REACH_M: f64 = 14_000.0;
/// How deep the seabed must be under a peak, in metres below datum.
const VOLCANIC_MIN_DEPTH_M: f64 = 2_500.0;
/// How far apart the candidate nodes are, in metres.
const VOLCANIC_LATTICE_M: f64 = 220_000.0;

/// Sparse volcanic peaks rising out of deep ocean.
///
/// Field order is the ABI -- `wasm.rs` encodes and decodes this struct in this order, and
/// `viewer/public/app/peak-params.js` mirrors it. Adding a field means editing both.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PeakParams {
    /// How tall a full-height peak stands above its seabed, in metres. Must clear the
    /// abyss -- `ABYSS_M` is -4,600, and a 700 m term surfaces nothing, which is exactly
    /// why the island arc never made an island.
    pub height_m: f64,
    /// What share of candidate nodes hold a peak, 0 to 1. **This is the opt-in field:** at
    /// zero the term returns before it touches the lattice.
    pub density: f64,
    /// How far a cone reaches from its centre, in metres.
    pub reach_m: f64,
    /// How deep the seabed must be for a peak to stand on it, in metres below datum and
    /// stated positive. Keeps islands off the continental shelf.
    pub min_depth_m: f64,
    /// How far apart the candidate nodes are, in metres. With `density`, this sets how many
    /// islands a world gets; `height_m` sets how tall they are. The two are independent,
    /// which is the whole reason this term is built on a lattice rather than a threshold.
    pub lattice_m: f64,
}

impl PeakParams {
    /// Inert. Opting in is one field -- `density` -- rather than five.
    pub fn canonical() -> Self {
        Self {
            height_m: VOLCANIC_HEIGHT_M,
            density: 0.0,
            reach_m: VOLCANIC_REACH_M,
            min_depth_m: VOLCANIC_MIN_DEPTH_M,
            lattice_m: VOLCANIC_LATTICE_M,
        }
    }

    /// Islands, at the density the survey settled on.
    pub fn volcanic() -> Self {
        Self { density: VOLCANIC_DENSITY, ..Self::canonical() }
    }
}
```

- [ ] **Step 4: Write the window helper**

A NaN must **close** this window, so it takes `coast_window`'s explicit three-arm shape rather than `smooth`, which returns 1.0 for NaN. Read `continentality.rs:163-183` before writing it.

```rust
/// How much of a peak stands, given the seabed under it.
///
/// One at the stated depth and below, ramping to zero a quarter again shallower, so the
/// term has no cliff in it. **A NaN closes this window rather than opening it**, which is
/// why the branches are written out: `smooth(NaN)` is 1.0, and a floored metre is
/// indistinguishable from a real one once it is added to an elevation.
fn peak_depth_window(depth_m: f64, min_depth_m: f64) -> f64 {
    let onset = min_depth_m * 0.8;
    let span = min_depth_m - onset;
    if !(span > 0.0) {
        // A zero or negative span, or a NaN threshold. No window.
        return 0.0;
    }
    if depth_m >= min_depth_m {
        1.0
    } else if depth_m > onset {
        let x = (depth_m - onset) / span;
        x * x * (3.0 - 2.0 * x)
    } else {
        // Shallower than the onset, or unanswerable.
        0.0
    }
}
```

- [ ] **Step 5: Write the cellular field**

Add `peak_noise`, `peak_jitter`, `peak_height` `Noise` fields and a `peaks: Option<PeakParams>` field to `Tectonics`, built unconditionally (a `Noise` is one `u64`) and read only when `peaks` is `Some` with non-zero density. Add `Tectonics::with_peaks(...)` and make the existing `Tectonics::new(...)` delegate to it with `None` — a new constructor, not a widened one, exactly as `Continentality::new` delegates to `with_coast` (`continentality.rs:236-249`).

```rust
/// How high the seamount field stands at a point, in metres, never negative.
///
/// One candidate per lattice node, its existence hashed against `density`, its position
/// jittered inside its own cell and its height drawn from a third salt. The 27 cells
/// around the sample are examined because a jittered centre can fall in any neighbour.
///
/// **The window is evaluated before the lattice is touched**, so a term that is inert, or
/// a point over shallow water, costs one comparison and no hashing. That is the same
/// ordering `coast_offset` uses (`continentality.rs:409-411`) and for the same reason.
pub fn peak_offset_m(&self, point: &SpherePoint, seabed_m: f64) -> f64 {
    let params = match self.peaks {
        None => return 0.0,
        Some(params) if params.density == 0.0 => return 0.0,
        Some(params) => params,
    };
    if !(params.lattice_m > 0.0) || !(params.reach_m > 0.0) {
        return 0.0;
    }
    let window = peak_depth_window(-seabed_m, params.min_depth_m);
    if !(window > 0.0) {
        return 0.0;
    }

    // Lattice cells about `lattice_m` across on this planet's surface.
    let frequency = self.radius_m / params.lattice_m;
    let v = point.vector;
    let (sx, sy, sz) = (v.x * frequency, v.y * frequency, v.z * frequency);

    // Lattice coordinates are never derived by a bare cast. `noise.rs:166-168` is the
    // pattern: floor, bound with a negated pair so a NaN takes the branch, then cast.
    let (fx, fy, fz) = (m::floor(sx), m::floor(sy), m::floor(sz));
    if !(fx >= -LATTICE_LIMIT && fx <= LATTICE_LIMIT)
        || !(fy >= -LATTICE_LIMIT && fy <= LATTICE_LIMIT)
        || !(fz >= -LATTICE_LIMIT && fz <= LATTICE_LIMIT)
    {
        return 0.0;
    }
    let (bx, by, bz) = (
        fx as i64, // cast-ok: floored and bounded against LATTICE_LIMIT on the lines above
        fy as i64, // cast-ok: floored and bounded against LATTICE_LIMIT on the lines above
        fz as i64, // cast-ok: floored and bounded against LATTICE_LIMIT on the lines above
    );

    let mut tallest = 0.0f64;
    for dz in -1..=1 {
        for dy in -1..=1 {
            for dx in -1..=1 {
                let (cx, cy, cz) = (bx + dx, by + dy, bz + dz);
                if self.peak_noise.lattice_at(cx, cy, cz) >= params.density {
                    continue;
                }
                // Jitter inside the cell, from a salt of its own so height and position
                // are uncorrelated.
                let jx = self.peak_jitter.lattice_at(cx, cy, cz);
                let jy = self.peak_jitter.lattice_at(cy, cz, cx);
                let jz = self.peak_jitter.lattice_at(cz, cx, cy);
                let centre = Vec3 {
                    x: (cx as f64) + jx, // cast-ok: a lattice coordinate, already bounded
                    y: (cy as f64) + jy, // cast-ok: a lattice coordinate, already bounded
                    z: (cz as f64) + jz, // cast-ok: a lattice coordinate, already bounded
                };
                // Back to the unit sphere, so the distance below is a real ground distance.
                let length = m::sqrt(
                    centre.x * centre.x + centre.y * centre.y + centre.z * centre.z,
                );
                if !(length > 0.0) {
                    continue;
                }
                let (ux, uy, uz) = (centre.x / length, centre.y / length, centre.z / length);
                let (ox, oy, oz) = (v.x - ux, v.y - uy, v.z - uz);
                let chord = m::sqrt(ox * ox + oy * oy + oz * oz) * self.radius_m;
                let fraction = chord / params.reach_m;
                if !(fraction < 1.0) {
                    continue;
                }
                // A cone with a smoothed flank, so nothing downstream differences a corner.
                let share = self.peak_height.lattice_at(cx, cy, cz);
                let standing = params.height_m * (0.45 + 0.55 * share)
                    * smooth(1.0 - fraction)
                    * window;
                if standing > tallest {
                    tallest = standing;
                }
            }
        }
    }
    tallest
}
```

Note: `LATTICE_LIMIT` is `pub(crate)` in `noise.rs:38` — import it, do not restate the number. `Vec3` and `smooth` likewise come from their existing homes. If `lattice_at` is not visible from `tectonics.rs`, widen its `pub(crate)` scope rather than copying it.

- [ ] **Step 6: Run the tests until green**

Run: `cargo test --release -p worldbuilder-engine --lib tectonics::tests -- peak`
Expected: all five pass. Then run the guards: `cargo test --release -p worldbuilder-engine --test no_std_math`. Expected: 7 pass, and `tectonics.rs`'s `.abs()` ledger entry still reads 9.

- [ ] **Step 7: Commit**

```bash
git add crates/worldbuilder-engine/src/tectonics.rs
git commit -m "A field of seamounts, standing on deep water only"
```

---

### Task 2: Wire it into the tectonic offset, and prove nothing moved

**Files:**
- Modify: `crates/worldbuilder-engine/src/tectonics.rs` (`offset_m`, around `:843`)
- Test: in-file `#[cfg(test)]` module

**Interfaces:**
- Consumes: Task 1's `peak_offset_m`, `PeakParams`, `Tectonics::with_peaks`.
- Produces: peaks visible through `Tectonics::offset_m` and therefore through `Shelf::evaluate`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn the_tectonic_offset_is_bit_identical_without_a_peak_block() {
    // The plan's first global constraint. Not close -- identical. If this fails, the
    // design is wrong and not the test.
    let land = crate::continentality::Continentality::new(4242, crate::sphere::EARTH_RADIUS_M, 0.4);
    let bare = Tectonics::new(4242, crate::sphere::EARTH_RADIUS_M, &land, 22);
    let with_none = Tectonics::with_peaks(4242, crate::sphere::EARTH_RADIUS_M, &land, 22, None);
    let inert = Tectonics::with_peaks(
        4242, crate::sphere::EARTH_RADIUS_M, &land, 22,
        Some(PeakParams::canonical()),
    );
    for i in 0..4_000 {
        let point = SpherePoint::from_latlon(
            -89.0 + 0.0445 * f64::from(i), // cast-ok: loop counter, 0..4000
            0.19 * f64::from(i) - 180.0,   // cast-ok: loop counter, 0..4000
        );
        let want = bare.offset_m(&point);
        assert_eq!(with_none.offset_m(&point).to_bits(), want.to_bits(), "None at probe {i}");
        assert_eq!(inert.offset_m(&point).to_bits(), want.to_bits(), "canonical at probe {i}");
    }
}

#[test]
fn a_peak_block_raises_the_offset_where_the_water_is_deep() {
    let land = crate::continentality::Continentality::new(4242, crate::sphere::EARTH_RADIUS_M, 0.4);
    let bare = Tectonics::new(4242, crate::sphere::EARTH_RADIUS_M, &land, 22);
    let peaked = Tectonics::with_peaks(
        4242, crate::sphere::EARTH_RADIUS_M, &land, 22,
        Some(PeakParams { density: 0.35, ..PeakParams::volcanic() }),
    );
    let mut raised = 0usize;
    let mut lowered = 0usize;
    for i in 0..6_000 {
        let point = SpherePoint::from_latlon(
            -89.0 + 0.0297 * f64::from(i), // cast-ok: loop counter, 0..6000
            0.41 * f64::from(i) - 180.0,   // cast-ok: loop counter, 0..6000
        );
        let before = bare.offset_m(&point);
        let after = peaked.offset_m(&point);
        if after > before { raised += 1; }
        if after < before { lowered += 1; }
    }
    assert!(raised > 0, "a peak block raised nothing over 6,000 probes");
    assert_eq!(lowered, 0, "a peak may only ever raise ground; {lowered} probes fell");
}
```

- [ ] **Step 2: Run them and watch the second fail**

Run: `cargo test --release -p worldbuilder-engine --lib tectonics::tests -- offset`
Expected: the bit-identity test passes already (nothing is wired), the raising test fails with "raised nothing".

- [ ] **Step 3: Add the term to `offset_m`**

Read `Tectonics::offset_m` in full first. It needs the seabed under the point to evaluate the window — `Tectonics` already holds the continentality it was built from, so use `base_elevation`. Add the peak term last, so it reads as an addition to a finished tectonic offset rather than something the margin terms then reshape.

The early-return in `peak_offset_m` is what preserves bit-identity: `offset_m` must not add an unconditional `+ 0.0`, because `-0.0 + 0.0` is `+0.0` and that flips a sign bit. Follow `continentality.rs:382-387`.

- [ ] **Step 4: Run the tests until green**

Run: `cargo test --release -p worldbuilder-engine --lib`
Expected: both new tests pass and the whole lib suite stays green.

- [ ] **Step 5: Prove the corpus did not move**

```bash
cargo run --release -p worldbuilder-engine --example parity_dump --features wasm > /tmp/native.txt
cd crates/worldbuilder-engine/parity && node parity.mjs /tmp/native.txt
```
Expected: `156011 values compared, 0 divergent`. Then each control, expecting its exact existing figure: seed 147,387; erosion-k 216; water-pond 60; tectonic-warp 22,995.

If any value moved, stop. The term is reaching the canonical path and that is a defect, not a new pin.

- [ ] **Step 6: Commit**

```bash
git add crates/worldbuilder-engine/src/tectonics.rs
git commit -m "Let the tectonic offset carry a seamount, and nothing else change"
```

---

### Task 3: Thread it to `Surface`, and see an island

**Files:**
- Modify: `crates/worldbuilder-engine/src/surface.rs`, `crates/worldbuilder-engine/src/shelf.rs` (only if `Shelf::new` must pass the block through)
- Test: in-file `#[cfg(test)]` module of `surface.rs`

**Interfaces:**
- Consumes: Task 2's wired `Tectonics`.
- Produces: `Surface::with_peaks(world_seed, radius_m, plate_count, land_fraction, features, relief, tectonics, coast, gully, peaks) -> Surface` — read the real signature of `Surface::with_gully` and mirror it, adding `peaks: Option<PeakParams>` as the last argument. `Surface::new`, `with_coast` and `with_gully` all delegate with `None`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn an_island_stands_above_the_datum_in_open_ocean() {
    // The spec's §1: measured over 20,000 points, this generator puts NO open ocean
    // above the datum. That is the thing this slice exists to change.
    let peaked = Surface::with_peaks_for_test(PeakParams { density: 0.25, ..PeakParams::volcanic() });
    let mut land_offshore = 0usize;
    for i in 0..20_000 {
        let point = fibonacci_point(i, 20_000);
        if peaked.structural_m(&point) > 0.0 && peaked.tectonics.offset_m(&point) > 2_000.0 {
            land_offshore += 1;
        }
    }
    assert!(land_offshore > 0, "still no islands");
}

#[test]
fn the_island_is_in_both_answers_and_they_still_agree() {
    // `elevation_m` == `structural_m` + the detail offset, bit for bit
    // (`surface.rs:389-394`). A term that lands in one and not the other breaks the
    // localisation property the whole surface stack rests on.
    let peaked = Surface::with_peaks_for_test(PeakParams { density: 0.25, ..PeakParams::volcanic() });
    let mut checked = 0usize;
    for i in 0..20_000 {
        let point = fibonacci_point(i, 20_000);
        let structural = peaked.structural_m(&point);
        if structural > 0.0 && peaked.tectonics.offset_m(&point) > 2_000.0 {
            let elevation = peaked.elevation_m(&point, None);
            assert!(elevation > 0.0, "an island that is land structurally must be land");
            checked += 1;
        }
    }
    assert!(checked > 0, "found no island to check");
}

#[test]
fn an_island_is_steep_to_rather_than_shelved() {
    // The reason peaks go in the tectonic offset: `Shelf::weight`'s authority is
    // `1 - smooth(|tectonic_m| / 250)`, so a large offset holds the shelf off entirely.
    // Deep water a short way off the beach is the navigational difference between
    // Hawaii and Britain, and it is the thing a bundle's soundings will show.
    let peaked = peaked_surface(PeakParams { density: 0.25, ..PeakParams::volcanic() });
    let summit = find_a_summit(&peaked).expect("no island to walk out from");
    let frame = TangentFrame::at(&summit, peaked.radius_m);
    let mut deepest_close_in = 0.0f64;
    for bearing in 0..8 {
        let angle = m::to_radians(45.0 * f64::from(bearing)); // cast-ok: loop counter, 0..8
        let out = frame.local_to_sphere(30_000.0 * m::cos(angle), 30_000.0 * m::sin(angle));
        let depth = peaked.structural_m(&out);
        if depth < deepest_close_in { deepest_close_in = depth; }
    }
    assert!(
        deepest_close_in < -1_000.0,
        "30 km off an island the water is only {deepest_close_in} m; that is a shelf, \
         and a peak in the tectonic offset is supposed to hold the shelf off",
    );
}

#[test]
fn no_peak_block_means_a_bit_identical_surface() {
    let bare = plain_surface();
    let inert = peaked_surface(PeakParams::canonical());
    for i in 0..8_000 {
        let point = fibonacci_point(i, 8_000);
        assert_eq!(
            inert.structural_m(&point).to_bits(),
            bare.structural_m(&point).to_bits(),
            "structural at probe {i}",
        );
        assert_eq!(
            inert.elevation_m(&point, None).to_bits(),
            bare.elevation_m(&point, None).to_bits(),
            "elevation at probe {i}",
        );
    }
}
```

Three helpers go beside them. `plain_surface` must build **exactly** what `peaked_surface` builds minus the block, or the bit-identity test proves nothing about the block and something about the two helpers:

```rust
const ISLAND_SEED: u64 = 9001;
const ISLAND_PLATES: usize = 22;
const ISLAND_LAND: f64 = 0.4;

fn plain_surface() -> Surface {
    Surface::new(ISLAND_SEED, crate::sphere::EARTH_RADIUS_M, ISLAND_PLATES, ISLAND_LAND, None, None, None)
}

fn peaked_surface(peaks: PeakParams) -> Surface {
    // Every argument identical to `plain_surface` above; the block is the only difference.
    Surface::with_peaks(
        ISLAND_SEED, crate::sphere::EARTH_RADIUS_M, ISLAND_PLATES, ISLAND_LAND,
        None, None, None, None, None, Some(peaks),
    )
}

/// An area-uniform point, so a share of probes is a share of the planet.
fn fibonacci_point(index: usize, count: usize) -> SpherePoint {
    let n = count as f64; // cast-ok: a test's own probe count
    let i = index as f64; // cast-ok: a test's own loop counter, below `count`
    let z = 1.0 - (2.0 * i + 1.0) / n;
    let radius = m::sqrt(if 1.0 - z * z > 0.0 { 1.0 - z * z } else { 0.0 });
    let theta = i * core::f64::consts::PI * (3.0 - m::sqrt(5.0));
    SpherePoint::from_vector(Vec3 {
        x: radius * m::cos(theta),
        y: radius * m::sin(theta),
        z,
    })
}

/// The highest offshore point the field makes, or `None` if it made none.
fn find_a_summit(surface: &Surface) -> Option<SpherePoint> {
    let mut best: Option<(f64, SpherePoint)> = None;
    for i in 0..40_000 {
        let point = fibonacci_point(i, 40_000);
        // Offshore: the tectonic offset is what raised it, not the continent field.
        if surface.tectonics.offset_m(&point) < 2_000.0 {
            continue;
        }
        let height = surface.structural_m(&point);
        if height <= 0.0 {
            continue;
        }
        match &best {
            Some((tallest, _)) if *tallest >= height => {}
            _ => best = Some((height, point)),
        }
    }
    best.map(|(_, point)| point)
}
```

Two notes for the implementer. `Surface::new`'s and `with_peaks`' real argument lists must be read from `surface.rs` and the calls above corrected to match — the shapes here are the plan's intent, not a transcription, and `with_gully`'s signature is the one to mirror. And if `SpherePoint::from_vector` is spelled differently, use whatever `surface.rs`'s own tests use; do not add a constructor for a test's convenience.

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo test --release -p worldbuilder-engine --lib surface::tests -- island`
Expected: compile failure — `Surface::with_peaks` does not exist.

- [ ] **Step 3: Add the constructor and thread the block**

Add `Surface::with_peaks` as a **new** constructor. Do not widen `with_gully` — `surface.rs:236-246` records that this is why `Continentality` has two constructors: `Surface::new` has about seventy call sites.

Thread `peaks` into the `Tectonics::with_peaks` call in the private builder (around `surface.rs:249`). **Do not add a `Surface` field** — `lib.rs:416-443` asserts there are exactly eight, by name.

Check `tests/wasm_exports.rs:914`, which counts occurrences of `Surface::with_coast` and `Surface::new`; it has already moved twice and may need its count edited.

- [ ] **Step 4: Run until green, then the whole suite**

Run: `cargo test --release -p worldbuilder-engine`
Expected: every test passes. Report the counts.

- [ ] **Step 5: Commit**

```bash
git add crates/worldbuilder-engine/src/surface.rs crates/worldbuilder-engine/tests/wasm_exports.rs
git commit -m "A world can be asked for islands"
```

---

### Task 4: The wasm ABI

**Files:**
- Modify: `crates/worldbuilder-engine/src/wasm.rs`

**Interfaces:**
- Consumes: `PeakParams`, `Surface::with_peaks`.
- Produces: `WB_PEAK_STRIDE = 5`; `WB_PEAK_CANONICAL = 0`, `WB_PEAK_VOLCANIC = 1`; exports `wb_world_new_peak`, `wb_peak_preset`, `wb_peak_check`.

**The `coast` block is the template throughout.** Mirror it item for item; every line number below is the coast equivalent to copy.

- [ ] **Step 1: Write the ABI**

Work through this checklist, reading the coast equivalent before writing each piece:

| piece | coast model |
|---|---|
| stride + field-order doc | `wasm.rs:683-696` |
| preset selectors | `:701`, `:708` |
| per-field `WB_MIN_*` / `WB_MAX_*` | `:718-866` |
| `peak_is_admissible`, using `within()` | `:1835-1886` |
| `decode_peak` | `:1918-1934` |
| `encode_peak` — the one inverse, in one place | `:1937-1949` |
| `enum PeakArg { Canonical, Chosen(PeakParams), Refused(u32) }` | `:1954-1963` |
| `read_peak` — null + len 0 is `Canonical`; non-null + len 0 is `Refused(WB_ERR_BUFFER)` | `:1970-1992` |
| `peak_preset_by_selector` — the only place the preset values are read | `:2000-2008` |
| `wb_world_new_peak` | `:2469-2516` |
| `wb_peak_preset` — so no host ever transcribes a default | `:2531-2553` |
| `wb_peak_check` — answers *why* a record was refused, without building a world | `:2570-2576` |
| three entries in `WB_EXPORTS` | `:1142-1144` |
| one more `None`-frozen argument on every earlier door, and one more on `build_world` | `:2853` |

Domains to set, with a sentence each on where the bound comes from — not round numbers for their own sake:
- `height_m`: min 0, max something that cannot overflow an elevation. It must be *possible* to state a value below `|ABYSS_M|`, because a caller is allowed to ask for seamounts that never surface.
- `density`: 0 to 1 inclusive.
- `reach_m`: above zero, and bounded below the planet.
- `min_depth_m`: 0 up to a value beyond any abyss.
- `lattice_m`: above zero. **A lower bound matters here** — a tiny lattice makes the 27-cell scan run over a huge number of candidate peaks per sample and is a performance trap, not just a silly world.

- [ ] **Step 2: No panic may cross the boundary**

Every refusal is a status code. `wb_peak_check` returns `WB_OK` / `WB_ERR_BUFFER` / `WB_ERR_PARAM`. `wb_world_new_peak` answers a refusal with handle 0 — which is exactly why the checker exists, per `wasm.rs:2570-2576`.

- [ ] **Step 3: Build and run**

Run: `cargo test --release -p worldbuilder-engine` and `cargo build --release --target wasm32-unknown-unknown -p worldbuilder-engine --features wasm`
Expected: green.

- [ ] **Step 4: Commit**

```bash
git add crates/worldbuilder-engine/src/wasm.rs
git commit -m "Three more doors: ask for islands across the boundary"
```

---

### Task 5: Pin the ABI

**Files:**
- Modify: `crates/worldbuilder-engine/tests/wasm_exports.rs`

- [ ] **Step 1: Write the tests**

Mirror the coast block at `wasm_exports.rs:3233-3400+`:

- `peak_preset_record` and `canonical_peak_record` helpers.
- `world_with_peak`, and a `sample_peak` that probes **both `elevation_m` and `structural_m`** — `:3323` is the coast model and the reason is the same, that a term in one and not the other is a seam.
- Per-field domain sweeps: for each of the five fields, the value just inside each bound is accepted and the value just outside is refused.
- `the_peak_checker_and_the_constructor_agree_on_every_swept_record` — for every record in the sweep, `wb_peak_check`'s verdict and whether `wb_world_new_peak` returns a non-zero handle must agree. A checker that says yes where the constructor says no is worse than no checker.
- A test that `wb_peak_preset` fills all five words for both selectors, so the studio never transcribes a default.
- A test that a canonical record produces a world whose samples are **bit-identical** to `wb_world_new`'s, which is the ABI-level statement of the plan's first global constraint.

- [ ] **Step 2: Run, then commit**

Run: `cargo test --release -p worldbuilder-engine --test wasm_exports`

```bash
git add crates/worldbuilder-engine/tests/wasm_exports.rs
git commit -m "Pin the island door, both sides of it"
```

---

### Task 6: The studio

**Files:**
- Create: `viewer/public/app/peak-params.js`, `viewer/test/peak-params.test.mjs`
- Modify: `viewer/public/app/engine.js`, `viewer/public/app/controls.js`, `viewer/public/app/main.js`

**`coast-params.js` is the model** — a pure module with no DOM, no Cesium and no engine, so it tests headless.

- [ ] **Step 1: Write `peak-params.js` and its test**

Mirror `coast-params.js`: `PEAK_STRIDE`, `PEAK_FIELDS` (ABI order), `PEAK_PRESET`, `PEAK_CONTROLS`, `PEAK_SLIDERS`, `peakReadoutFields()`, `PEAK_PARAM_NAMES`, `peakPanelFields()`, `peakToRecord()` / `peakFromRecord()`, `peakFromParams()` / `peakToParams()`.

The test must assert `PEAK_FIELDS` is in the same order as the Rust struct, and that `peakToRecord` and `peakFromRecord` round-trip.

- [ ] **Step 2: Wire the engine**

`engine.js`: import, add the three exports to the required-export list (`:173`), `peakPreset()`, `checkPeak()`, and allocate/deallocate the record in the world-build path, passing `peakPtr, peaks ? PEAK_STRIDE : 0` (the coast model is `:465`).

- [ ] **Step 3: Wire the panel**

`controls.js`: an islands section, `wirePeaks(presets)`, and `peakToParams(...)` spread into **both** query-string sites (`:1138`, `:1161`). `main.js`: `peaks: null` in the spec, `peakFromParams`, the readout, and the boot-time preset read.

**An untouched panel must write no peak parameter at all**, so a reload takes the `None` path. That is what keeps a saved world bit-identical.

- [ ] **Step 4: Run the viewer suite, rebuild the wasm**

```bash
cd viewer && npm test && npm run build:wasm && npm run check:wasm
git ls-files --eol crates/worldbuilder-engine viewer/public/app | awk '$2!="w/lf"'
```
Expected: all viewer tests pass; `check:wasm` matches; the EOL guard prints nothing. A new export means the manifest moves — that is expected and is the only reason it should.

- [ ] **Step 5: Commit**

```bash
git add viewer/
git commit -m "A slider for islands, and a panel that stays quiet without one"
```

---

### Task 7: Calibrate, verify, report

**Files:**
- Create: `crates/worldbuilder-engine/src/bin/island_survey.rs`, `docs/superpowers/reports/2026-09-14-islands-1-peaks-verification.md`

**Interfaces:**
- Consumes: everything above.

- [ ] **Step 1: Write the survey binary**

`src/bin/coastline_survey.rs` is the model — `CoastParams::fractal()`'s 0.35 came out of it, and the named constant and the survey must not drift.

Sweep `density` and `lattice_m`, and for each combination report, over an area-uniform sample of at least 20,000 points: the share of the planet's surface standing above the datum that is *not* continental, the number of distinct islands, and the distribution of island areas.

**Spec §7 question 1 asks for 0.3-0.8% of surface as islands.** Choose `VOLCANIC_DENSITY` and `VOLCANIC_LATTICE_M` from the survey's output to land inside that band, and say which run the figures came from. If the band cannot be reached, say so and report what is reachable rather than quietly missing it.

- [ ] **Step 2: Re-derive every pin by running it**

Never transcribe a figure — this project has shipped one transcribed number already. Run and read:
- `cargo test --release -p worldbuilder-engine` — the five per-configuration counts.
- `cargo test --release -p worldbuilder-engine --test no_std_math` — and confirm `tectonics.rs`'s `.abs()` ledger entry is unchanged at 9.
- The Python suite and conformance.
- `cd viewer && npm test`.
- The parity corpus and all four controls, which **must be unmoved**.
- `npm run check:wasm`, and the EOL guard.

- [ ] **Step 3: Write the verification report**

In the form of `docs/superpowers/reports/2026-09-12-water-1b4-verification.md`. Population, method-with-parameters and host on every figure. It must carry:
- the before/after for spec §1's measurement: islands found over 20,000 area-uniform points, at the canonical preset (expected: zero, unchanged) and at `volcanic()`;
- the survey's chosen density and lattice, and the island-area band achieved;
- proof that the parity corpus and all four controls are unmoved, which is this slice's central claim;
- the steep-to measurement — depth 30 km off an island, against a continental coast for contrast;
- what a peak does to substrate and to detail roughness, measured rather than asserted from the spec's table;
- the achieved land fraction against the requested one, at the canonical preset and at
  `volcanic()`. **Spec §5 asks that `Surface` be able to report both numbers; this slice
  measures them in the survey and does not add the API.** Peaks are added after
  calibration, so the requested figure stays true of the continents and islands sit on top
  of it — the gap is reportable and small. Fragments are what make the requested number
  actually wrong, because they sit *inside* calibration, so the accessor lands with them in
  slice 3. Say this in the report so the gap is a decision on the record, not an omission;
- every pin, old and new.

- [ ] **Step 4: Commit**

```bash
git add crates/worldbuilder-engine/src/bin/island_survey.rs docs/superpowers/reports/
git commit -m "Survey the islands, and report what the first slice made"
```
