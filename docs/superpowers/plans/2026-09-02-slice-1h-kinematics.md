# Slice 1h: Plate Kinematics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port `worldbuilder/plates/kinematics.py` to the Rust engine — how fast the ground is moving, and what that does where two plates meet — and establish the fabrication guard that three earlier slices have flagged and none could test.

**Architecture:** Three functions and a small value type. A plate turning about an Euler pole has a different surface velocity everywhere: fast at the equator of its own rotation, nothing at all at its pole. That variation is why one margin can pull apart at one end and grind sideways at the other. **Nothing is stored** — a margin is not classified once and remembered, it is worked out at the point somebody asks about.

**Tech Stack:** Rust (engine), PyO3 bindings, pytest differential harness against the Python reference.

**Spec:** `docs/design/2026-09-02-mark-2-world-studio.md`, sections 4.1 (bit-equality), 4.2 (DETERMINISM-001), 6 (VERSION-001).

## Global Constraints

- **Two conformance contracts, chosen per code path, not per function.** Strict bit-for-bit where no transcendental is in the path; bounded at `MAX_TRANSCENDENTAL_ULPS = 4` where one is. **This module is almost entirely strict — see "The contract" below.**
- **All float maths through `detmath`** (libm-backed). No `f64::` method or associated-function form, no `mul_add`, no bare integer casts without a `// cast-ok: <reason>` marker. A build-failing guard test enforces this.
- **Constants transcribed character-for-character.** `ACROSS_ENOUGH` is `0.5` — a bare literal, no underscores, no exponent, no trailing zeros.
- **Never `f64::min` / `f64::max` / `clamp`.** Python's two-argument forms are asymmetric under NaN. This module has none, but if you reach for one, you are diverging from the Python.
- **The bisector and seed tables are addressed by POSITION on every axis.** Settled in slice 1f, concurred by review, documented at `plates.rs:161`.
- **Nothing under `worldbuilder/` may be modified.** The Python remains the reference. `worldbuilder/integration/maritime.py` has a pre-existing uncommitted change; leave it unstaged.
- **A floating-point sign, zero, or exact-equality assertion without the reason it holds is a latent bug.** Five have occurred in this port. Establish why each holds, or do not assert it.

---

## The fabrication guard: what this slice owes, and a correction to how it was described

Slices 1e and 1f flagged that the PyO3 bindings once fabricated `euler_pole = seed` and `rate_rad_per_myr = 0.0`. Slice 1f's review proved by mutation that the margin functions never read those fields, so the fabrication was inert there and the READMEs record that honestly.

Those records also say kinematics is where the guard "must finally be established, because `angular_velocity` is the only function that reads those fields." **That wording is imprecise and this plan corrects it.** `Plate::angular_velocity` is already ported, already bound as `plate_angular_velocity`, and already conformance-tested across nine rates and about seventy poles. But that binding builds a `Plate` **inline** (`bindings.rs:189-195`) and never calls `plateset_from_parts` — it even sets `seed = euler_pole` deliberately, since the seed is irrelevant to it.

So what exists today guards `angular_velocity`'s **arithmetic**. What does *not* exist is a guard on the **`plateset_from_parts` contract** — that the constructor carries poles and rates honestly into something that consumes them. Every binding that currently goes through that constructor feeds only functions which ignore both fields.

`motion_at` is the first function that needs the `PlateSet` (for `margin_at` and `margin_normal`) **and** reads poles and rates (through `surface_velocity`). **That makes this slice's obligation concrete and testable, which the earlier wording was not:**

> **Task 4 must include a mutation test.** Fabricate `euler_pole = seed` and `rate_rad_per_myr = 0.0` inside `plateset_from_parts`, rebuild, and confirm the `motion_at` conformance tests **fail**. Then revert and confirm they pass. A fabricated rate of zero makes `angular_velocity` return the zero vector, so every velocity, closing and sliding speed collapses to zero — a full-magnitude discrepancy, not a subtle one. If that mutation does *not* fail the suite, the guard still does not exist and that is a Critical finding, not a curiosity.

---

## The contract, and why this module is the cleanest in the port

**`kinematics.py` contains no transcendental call at all.** The only non-arithmetic operation anywhere is the `sqrt` inside `Vec3::length()`, which is algebraic and IEEE-754-mandated correctly rounded. Everything this module computes — velocities, closing, sliding, and the classification — is therefore on the **strict bit-for-bit contract**, with no ULP bound available or needed.

That extends to the discrete decision. `kind` is chosen by comparing `abs(closing) / speed` against `ACROSS_ENOUGH`, which is exactly the shape that has bitten this project repeatedly — but here every input to that comparison is computed algebraically, so both implementations compare identical values and the choice cannot diverge. **State that as the reason, not as a hope.**

The one bounded quantity is imported rather than computed: `motion_at` returns the `Margin` it looked up, and that margin's `distance_m` came through `asin` in `margin_at`. `motion_at` never reads it — it uses only `margin.nearest`, `margin.neighbour` and the normal. So the returned `distance_m` is compared with `close_enough` and everything else strictly.

---

## The short-circuit that prevents a division by zero

```python
if speed <= 0.0 or abs(closing) / speed < ACROSS_ENOUGH:
    kind = TRANSFORM
elif closing > 0.0:
    kind = CONVERGENT
else:
    kind = DIVERGENT
```

The `or` is load-bearing. If the operands were reordered, or the condition computed into a variable before branching, `abs(closing) / 0.0` would evaluate — giving infinity or NaN — and the classification would change. Rust's `||` short-circuits identically, **so write it as a single `if` with `||`, never as a precomputed boolean.**

Work through the NaN case rather than assuming it: if `speed` is NaN, `speed <= 0.0` is false; `abs(closing) / NaN` is NaN; `NaN < 0.5` is false; so the first branch is not taken and the code falls to `closing > 0.0`, which is false for a NaN `closing`, giving `DIVERGENT`. Reproduce that exactly with `if` / `else if` / `else`.

---

## The duplication, and the precedent for what to do about it

`motion_between` and `motion_at` contain the **same twelve lines verbatim** — `relative`, `closing`, `along`, `sliding`, `speed`, and the classification chain — differing only in where the two plates come from. `motion_between`'s docstring says it was "Split out from `motion_at`", yet `motion_at` does not call it.

Slice 1f met this exact situation with `flattened` and `margin_normal`: the Python inlined a duplicate, and the ruling — concurred by review — was that calling the shared function is *more* faithful than transcribing the duplication, **provided the two bodies are verified byte-identical first**, because operation order is what a bit-for-bit contract actually constrains.

**Apply that precedent: `motion_at` calls `motion_between`.** Task 3 must verify the two Python bodies are identical before doing so, and say so in its report. The only difference is the returned `Motion`'s `margin` field, which `motion_at` fills in afterwards.

---

## File Structure

- **Create** `crates/worldbuilder-engine/src/kinematics.rs`, and declare it in `src/lib.rs`. This is decided, not left open: `plates.rs` is already 780 lines and this module would push it toward 930, and kinematics is a distinct responsibility from the plate lookup — velocities and margin classification, not Voronoi geometry. The Python draws the same line, in its own `kinematics.py`.
- `Vec3::cross` already exists (`src/vectors.rs:39`, with a right-hand-rule test). Use it; do not add a second one.
- **Modify** `crates/worldbuilder-engine/src/bindings.rs` and `src/lib.rs` — three bindings.
- **Modify** `tests/test_conformance.py` — the differential tests and the fabrication mutation.
- **Modify** `crates/worldbuilder-engine/README.md` — the record.

---

### Task 1: The value type, the constants, and `surface_velocity`

**Files:**
- Modify: the engine module chosen above

**Interfaces:**
- Produces: `pub const ACROSS_ENOUGH: f64 = 0.5;`
- Produces: `pub enum MarginKind { Convergent, Divergent, Transform }` with a `fn as_str(&self) -> &'static str` returning exactly `"convergent"`, `"divergent"`, `"transform"`.
- Produces: `pub struct Motion { pub margin: Option<Margin>, pub closing_m_per_myr: f64, pub sliding_m_per_myr: f64, pub kind: MarginKind }`
- Produces: `pub fn surface_velocity(plate: &Plate, point: &SpherePoint, radius_m: f64) -> Vec3`
- Consumes: `Plate::angular_velocity`, `Vec3::cross`, `Vec3::scaled`.

`radius_m` is a required parameter, not an emulated default — matching every other ported signature.

**`Vec3::cross` already exists** at `src/vectors.rs:39` with a right-hand-rule test -- verified, so do not add a second one. Confirm its component order matches the Python `Vec3.cross` before relying on it.

The Python is one line:

```python
return plate.angular_velocity().cross(point.vector).scaled(radius_m)
```

Its docstring records why that form was chosen, and the reason is worth a comment: the cross product is **automatically tangent to the sphere, and automatically zero at the plate's own Euler pole, without either being a special case anybody had to write.**

- [ ] **Step 1: Write the failing tests.**

```rust
#[test]
fn a_plate_is_motionless_at_its_own_euler_pole() {
    // Not a special case in the code -- it falls out of the cross product, because the
    // position vector is parallel to the rotation axis there.
    let plate = test_plate_with_pole(0, 0.0, 0.0, 90.0, 0.0, 0.01);
    let at_pole = SpherePoint::from_latlon(90.0, 0.0);
    let v = surface_velocity(&plate, &at_pole, EARTH_RADIUS_M);
    assert!(
        v.length() < 1e-9,
        "velocity at the plate's own Euler pole should vanish, got {}",
        v.length(),
    );
}

#[test]
fn surface_velocity_is_tangent_to_the_sphere() {
    // Also not a special case: a cross product with the position is perpendicular to it.
    let plate = test_plate_with_pole(0, 0.0, 0.0, 90.0, 0.0, 0.01);
    let point = SpherePoint::from_latlon(17.0, 43.0);
    let v = surface_velocity(&plate, &point, EARTH_RADIUS_M);
    assert!(
        v.dot(&point.vector).abs() < 1e-6,
        "velocity must be tangent, dot with position was {}",
        v.dot(&point.vector),
    );
}

#[test]
fn doubling_the_rate_exactly_doubles_the_velocity() {
    // Exact, not approximate, and the reason is worth stating: angular_velocity is
    // linear in the rate; doubling every component multiplies the sum of squares by
    // four; and sqrt(4x) is exactly 2*sqrt(x) in IEEE-754. Scaling by a power of two
    // is exact throughout, so there is no rounding anywhere in this chain.
    let slow = test_plate_with_pole(0, 0.0, 0.0, 90.0, 0.0, 0.01);
    let fast = test_plate_with_pole(0, 0.0, 0.0, 90.0, 0.0, 0.02);
    let point = SpherePoint::from_latlon(17.0, 43.0);
    let a = surface_velocity(&slow, &point, EARTH_RADIUS_M).length();
    let b = surface_velocity(&fast, &point, EARTH_RADIUS_M).length();
    assert_eq!(b.to_bits(), (2.0 * a).to_bits(), "expected exact doubling, got {b} vs {}", 2.0 * a);
}
```

**Where the two tolerances come from — check the arithmetic rather than taking these numbers.**

`from_latlon(90.0, 0.0)` is not exactly `(0, 0, 1)`, because `cos(90°)` is not exactly zero in floating point but about `6.1e-17`. So the velocity at the plate's own pole does not vanish exactly; it leaves a residue of roughly `radius * rate * 6.1e-17`, which for `rate = 0.01` and Earth's radius is about `3.9e-12`. The `1e-9` bound therefore has about three orders of headroom — comfortable but not vacuous.

For tangency, the cross product is perpendicular to the position by construction, so the dot product is zero in exact arithmetic and only rounding remains: roughly `|v| * eps`, or about `6e-12` for these magnitudes. **`1e-6` is far looser than that**; tighten it to `1e-9` unless your measurement says otherwise.

**Measure both while developing and report the actual residues.** If either exceeds what is derived here, that is a finding about the port, not a reason to widen the bound. A tolerance chosen to make a test pass is how a test stops testing.

**Do not invent a `Plate` literal.** Use or extend the existing test constructors in the engine (`test_plate(index, lat, lon)` from slice 1g, and whatever earlier slices added). If you need to vary the pole and rate independently, add one helper and say so.

- [ ] **Step 2: Run them and confirm they fail** for the expected reason — the function does not exist — not an unrelated compile error.

- [ ] **Step 3: Implement the constants, the enum, `Motion`, and `surface_velocity`.**

- [ ] **Step 4: Run the tests and confirm they pass.**

- [ ] **Step 5: Commit.**

---

### Task 2: `motion_between`

**Files:**
- Modify: the engine module chosen in Task 1

**Interfaces:**
- Produces: `pub fn motion_between(near: &Plate, far: &Plate, point: &SpherePoint, normal: &Vec3, radius_m: f64) -> Motion`
- Consumes: `surface_velocity`, `ACROSS_ENOUGH`, `MarginKind`, `Motion`.

Transcribe the Python's `motion_between` exactly. Its `Motion` has `margin: None` — in Rust, `margin: None`.

The `normal` argument points **across the margin, tangent to the surface, towards `near`**. That is why `closing` is the *negative* of the relative velocity's component along it: the nearest plate moving along the normal is moving *away* from the neighbour.

- [ ] **Step 1: Write the failing tests.**

**The construction these tests use, and why it makes them exact.** Put both plates' Euler poles at the north pole, so each angular velocity is `(0, 0, rate)`. At the equatorial point `(1, 0, 0)` the cross product gives a surface velocity of `(0, rate * radius, 0)` — due east — so the relative velocity is `(0, (near_rate - far_rate) * radius, 0)`.

At that point, any `(0, a, b)` with `a² + b² = 1` is both tangent and unit, which is what `motion_between` requires of the normal. For such a normal, `|closing| / speed` is exactly `|a|`: the numerator is `|a · y|` and the denominator `|y|`, and that division is exact in IEEE for the values used below. **So the threshold can be hit precisely rather than approached**, which is what makes the strictness of `<` testable.

```rust
#[cfg(test)]
fn spinning_pair(near_rate: f64, far_rate: f64) -> (Plate, Plate) {
    // Both poles at the north pole, so both angular velocities are (0, 0, rate).
    (
        test_plate_with_pole(0, 0.0, 0.0, 90.0, 0.0, near_rate),
        test_plate_with_pole(1, 0.0, 10.0, 90.0, 0.0, far_rate),
    )
}

#[cfg(test)]
const ON_THE_EQUATOR: fn() -> SpherePoint = || SpherePoint::from_latlon(0.0, 0.0);

#[test]
fn two_plates_driving_into_each_other_are_convergent() {
    // Relative velocity is due east; the normal points due west, into `near`. The
    // nearest plate is moving against it, so they are closing.
    let (near, far) = spinning_pair(0.01, 0.02);
    let motion = motion_between(
        &near, &far, &ON_THE_EQUATOR(), &Vec3::new(0.0, 1.0, 0.0), EARTH_RADIUS_M);
    assert_eq!(motion.kind, MarginKind::Convergent);
    assert!(motion.closing_m_per_myr > 0.0);
}

#[test]
fn two_plates_pulling_apart_are_divergent() {
    let (near, far) = spinning_pair(0.02, 0.01);
    let motion = motion_between(
        &near, &far, &ON_THE_EQUATOR(), &Vec3::new(0.0, 1.0, 0.0), EARTH_RADIUS_M);
    assert_eq!(motion.kind, MarginKind::Divergent);
    assert!(motion.closing_m_per_myr < 0.0);
}

#[test]
fn plates_sliding_past_one_another_are_transform() {
    // The normal points due north, perpendicular to the eastward relative motion,
    // so nothing is crossing the margin at all.
    let (near, far) = spinning_pair(0.02, 0.01);
    let motion = motion_between(
        &near, &far, &ON_THE_EQUATOR(), &Vec3::new(0.0, 0.0, 1.0), EARTH_RADIUS_M);
    assert_eq!(motion.kind, MarginKind::Transform);
}

#[test]
fn the_across_enough_threshold_is_hit_exactly_and_is_not_inclusive() {
    // |closing| / speed equals `a` exactly for a normal of (0, a, b). At a = 0.5 the
    // ratio is exactly ACROSS_ENOUGH, and the Python's test is a strict `<`, so this
    // must NOT be transform. At 0.4 it must be. This fails if ACROSS_ENOUGH is
    // mistyped, and it fails if the comparison is loosened to `<=`.
    let (near, far) = spinning_pair(0.02, 0.01);
    let root_three_over_two = detmath::sqrt(0.75);
    let exactly_at = motion_between(
        &near, &far, &ON_THE_EQUATOR(),
        &Vec3::new(0.0, 0.5, root_three_over_two), EARTH_RADIUS_M);
    assert_ne!(
        exactly_at.kind, MarginKind::Transform,
        "a ratio of exactly ACROSS_ENOUGH is not below it, so the strict `<` must not fire",
    );
    let just_below = motion_between(
        &near, &far, &ON_THE_EQUATOR(),
        &Vec3::new(0.0, 0.4, detmath::sqrt(1.0 - 0.16)), EARTH_RADIUS_M);
    assert_eq!(just_below.kind, MarginKind::Transform);
}

#[test]
fn a_stationary_pair_is_transform_rather_than_dividing_by_zero() {
    // speed is exactly 0.0, so `abs(closing) / speed` would be 0.0 / 0.0. Only the
    // short-circuit prevents it.
    let (near, far) = spinning_pair(0.01, 0.01);
    let motion = motion_between(
        &near, &far, &ON_THE_EQUATOR(), &Vec3::new(0.0, 1.0, 0.0), EARTH_RADIUS_M);
    assert_eq!(motion.kind, MarginKind::Transform);
    // Exact equality is legitimate here, and the reason is worth stating: these are
    // products of an exactly-zero vector, not the residue of cancellation between
    // unequal quantities.
    assert_eq!(motion.closing_m_per_myr, 0.0);
    assert_eq!(motion.sliding_m_per_myr, 0.0);
}
```

**A detail worth knowing before you write the stationary test**, because it is the shape that has produced five bugs in this port: `closing` there is **negative zero**, not positive zero. It is `-(0.0)`, since the dot product of a zero vector is `+0.0`. `-0.0 == 0.0` is true, so the assertion above holds — but `-0.0` is *not* bit-identical to `0.0`, so a `bits()`-style comparison would distinguish them. The Python produces `-0.0` too, by the same expression, so conformance agrees; just do not assert a sign here, and do not "tidy" the negation away.

**Verify the derivations rather than trusting this plan.** Confirm the surface velocity really is `(0, rate * radius, 0)` at that point and that the ratio really lands exactly on `0.5` — print the values once while developing. If any of it is wrong, fix the test to be genuinely exact and say so in your report; do not adjust an assertion until it passes.

`ON_THE_EQUATOR` is written as a `fn()` constant only to avoid repeating the constructor; use whatever plain local the codebase's style prefers.

- [ ] **Step 2: Run them and confirm they fail.**

- [ ] **Step 3: Implement `motion_between`.** The classification must be a single `if` with `||`, preserving the short-circuit:

```rust
// Python writes `if speed <= 0.0 or abs(closing) / speed < ACROSS_ENOUGH`. The `or`
// short-circuits, which is the only thing preventing a division by zero when the two
// plates are moving identically. Do not precompute this condition.
let kind = if speed <= 0.0 || detmath::abs(closing) / speed < ACROSS_ENOUGH {
    MarginKind::Transform
} else if closing > 0.0 {
    MarginKind::Convergent
} else {
    MarginKind::Divergent
};
```

Use whatever form of `abs` the codebase already routes through `detmath` — `margin_at` uses one; match it.

- [ ] **Step 4: Run the tests and confirm they pass.**

- [ ] **Step 5: Commit.**

---

### Task 3: `motion_at`

**Files:**
- Modify: the engine module chosen in Task 1

**Interfaces:**
- Produces: `pub fn motion_at(point: &SpherePoint, plates: &PlateSet, radius_m: f64) -> Option<Motion>`
- Consumes: `PlateSet::margin_at`, `PlateSet::margin_normal`, `motion_between`.

**`motion_at` calls `motion_between` rather than transcribing the Python's duplicated block.** Before doing so, **verify the two Python bodies are byte-identical** — `relative`, `closing`, `along`, `sliding`, `speed` and the classification chain — and say so in your report. This follows the ruling made in slice 1f for `flattened` and `margin_normal`: calling is more faithful than duplicating *provided the bodies match*, because operation order is what the bit-for-bit contract constrains.

Two early exits, both returning `None`: when `margin.neighbour` is `None`, and when `margin_normal` returns `None`.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test]
fn a_single_plate_world_has_no_motion_to_report() {
    // No neighbour, so no margin, so None -- not a Motion full of zeros.
    let set = PlateSet::new(vec![test_plate(0, 0.0, 0.0)]);
    assert!(motion_at(&SpherePoint::from_latlon(10.0, 10.0), &set, EARTH_RADIUS_M).is_none());
}

#[test]
fn motion_at_agrees_bit_for_bit_with_motion_between_on_the_same_margin() {
    // This is the point of calling rather than duplicating. Look the margin up by
    // hand, call motion_between with its parts, and require exact agreement -- not
    // approximate, because both paths run identical arithmetic in identical order.
    let set = three_plate_set();
    let point = SpherePoint::from_latlon(12.0, 20.0);
    let whole = motion_at(&point, &set, EARTH_RADIUS_M).expect("this point has a margin");

    let margin = set.margin_at(&point, EARTH_RADIUS_M);
    let normal = set.margin_normal(&point, &margin).expect("and a normal");
    let by_hand = motion_between(
        &margin.nearest.expect("a nearest plate"),
        &margin.neighbour.expect("a neighbour"),
        &point,
        &normal,
        EARTH_RADIUS_M,
    );

    assert_eq!(whole.closing_m_per_myr.to_bits(), by_hand.closing_m_per_myr.to_bits());
    assert_eq!(whole.sliding_m_per_myr.to_bits(), by_hand.sliding_m_per_myr.to_bits());
    assert_eq!(whole.kind, by_hand.kind);
}

#[test]
fn the_returned_motion_carries_the_margin_it_used() {
    // motion_between returns margin: None; motion_at fills it in. Without this the
    // caller cannot tell which plates the numbers describe.
    let set = three_plate_set();
    let motion = motion_at(&SpherePoint::from_latlon(12.0, 20.0), &set, EARTH_RADIUS_M)
        .expect("this point has a margin");
    let margin = motion.margin.expect("motion_at must attach the margin it used");
    assert!(margin.nearest.is_some() && margin.neighbour.is_some());
}
```

`three_plate_set()` and `test_plate()` already exist from slice 1g — reuse them rather than adding a third convention. If the latitude used above turns out to sit in a region with no margin, pick one that does and say which; do not weaken the test to an `if let`.

The `to_bits()` comparison is deliberate: both paths run identical arithmetic in identical order, so anything less than exact agreement means the refactor changed something.

- [ ] **Step 2: Run them and confirm they fail.**

- [ ] **Step 3: Implement `motion_at`.**

- [ ] **Step 4: Run the tests and confirm they pass, then run the whole crate suite.**

- [ ] **Step 5: Commit.**

---

### Task 4: Conformance, and the fabrication guard three slices have owed

**Files:**
- Modify: `crates/worldbuilder-engine/src/bindings.rs`, `crates/worldbuilder-engine/src/lib.rs`, `tests/test_conformance.py`

**Interfaces:**
- `plate_surface_velocity(pole_x, pole_y, pole_z, rate, x, y, z, radius_m) -> (f64, f64, f64)`
- `plates_motion_between(near_pole, near_rate, far_pole, far_rate, x, y, z, nx, ny, nz, radius_m) -> (f64, f64, str)`
- `plateset_motion_at(seeds, poles, rates, x, y, z, radius_m) -> Option<(usize, usize, f64, f64, f64, str)>` — nearest index, neighbour index, margin distance, closing, sliding, kind.

**Everything this module computes is on the strict contract** — no transcendental touches any of it. Compare `closing`, `sliding` and every velocity component with `same()`, bit-for-bit. Compare `kind` as an exact string. Compare plate indices as exact integers. **The only `close_enough` comparison is the margin's `distance_m`**, which came through `asin` inside `margin_at` and merely rides along in the returned `Motion`.

Compare the `None` cases **positionally** — the Rust must return nothing exactly where the Python does.

Cover: the corpus against a multi-plate set; a single-plate set; points either side of the `ACROSS_ENOUGH` threshold; a stationary pair where `speed == 0.0` exactly; and a point at a plate's own Euler pole.

- [ ] **Step 1: Add the three bindings and register them.** Conversion only, no arithmetic.
- [ ] **Step 2: Rebuild** with `maturin develop --release` into the project venv.
- [ ] **Step 3: Add the conformance tests.**
- [ ] **Step 4: THE FABRICATION MUTATION — this is the task's headline deliverable, not a formality.**

Fabricate `euler_pole = seed` and `rate_rad_per_myr = 0.0` inside `plateset_from_parts` in `bindings.rs`, rebuild, and run the conformance suite. **Record exactly which tests fail and how many.** A zero rate makes `angular_velocity` return the zero vector, so every velocity, closing and sliding speed collapses to zero — this should be a loud, full-magnitude failure.

Then **revert, rebuild, and confirm the suite passes**, verifying with `git status` and `git diff` that nothing is left behind.

**If the mutation does not fail the suite, stop and report it as a Critical finding.** It would mean the guard three slices have been waiting for still does not exist, and the tests written above do not exercise the constructor. Do not paper over it by adding a test afterwards without saying that is what happened.

- [ ] **Step 5: Measure and assert, do not print.** Record the smallest margin by which any classification decision clears the `ACROSS_ENOUGH` threshold across the corpus — that is, the minimum of `abs(abs(closing) / speed - ACROSS_ENOUGH)` over samples where `speed > 0.0` — and **assert a floor on it with the observed value in the failure message.** Everything feeding that comparison is algebraic and therefore identical in both implementations, so no divergence is expected; the floor documents how much room there actually is, and fires if a future change erodes it. A `print` is swallowed by pytest on a passing run, and three vacuous tests have shipped in this port.

- [ ] **Step 6: Run both suites**, quote them, and report every measured number.
- [ ] **Step 7: Commit.**

---

### Task 5: Record it

**Files:**
- Modify: `crates/worldbuilder-engine/README.md`

- [ ] **Step 1: Record** —
  - that this is **the cleanest module in the port**: no transcendental anywhere, so everything it computes is strict, *including* the `ACROSS_ENOUGH` classification, because every input to that comparison is algebraic;
  - that the only bounded quantity is the `distance_m` riding inside the returned margin, which `motion_at` never reads;
  - **the fabrication guard, and the correction to how earlier slices described it.** Say plainly that the READMEs for 1f and 1g asserted kinematics would be where the guard was established "because `angular_velocity` is the only function that reads those fields", that this was imprecise — `angular_velocity` was already ported, bound and conformance-tested through a binding that bypasses `plateset_from_parts` — and that the guard which was actually missing was on the **constructor contract**, now established by `motion_at` and proven by the Task 4 mutation. Quote the mutation's result.
  - the short-circuit that prevents a division by zero, and that its operand order is load-bearing;
  - that `motion_at` calls `motion_between` rather than duplicating it, and the byte-identity that licenses it;
  - the measured threshold margin from Task 4.

**Verify every test count by running the suites yourself.** Do not copy a number from any report — a count in an earlier README was wrong by twelve because it came from an extraction nobody re-ran.

- [ ] **Step 2: Commit.**

---

## What this slice deliberately does not do

- **No tectonics.** `terrain/tectonics.py` imports `ACROSS_ENOUGH` and `motion_between` from here and is the next slice; it can now be ported without reaching into an unported package.
- **No plate generation.** `generation.py` needs `blake2b` over a UTF-8 joined string — a byte-level port feeding a cryptographic hash, and a new engine dependency.
- **No deletion of the Python.** It stays the reference.
