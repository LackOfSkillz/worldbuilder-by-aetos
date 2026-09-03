# Slice 1j: Plate Generation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port `worldbuilder/plates/generation.py` to `crates/worldbuilder-engine/src/generation.rs`, completing the `plates` package, and establish that a Rust BLAKE2 implementation reproduces CPython's `hashlib.blake2b` byte for byte.

**Architecture:** Every value is hashed, never drawn from a sequence. A plate's pole and rate come from `hash(world_seed, "plate", index, what)`, so plate 7 depends on nothing but the seed and the number 7. Seeds are placed on a golden spiral and nudged.

**Tech Stack:** Rust (engine), a pinned BLAKE2 crate, PyO3 bindings, pytest differential harness.

**Spec:** `docs/design/2026-09-02-mark-2-world-studio.md`, sections 4.1 (bit-equality), 4.2 (DETERMINISM-001), 6 (VERSION-001).

## Global Constraints

- **Two conformance contracts, chosen per code path, not per function.** Strict bit-for-bit where no transcendental is in the path; bounded at `MAX_TRANSCENDENTAL_ULPS = 4` where one is.
- **All transcendentals through `detmath`** (libm-backed, pinned `=0.2.11`). No `f64::` method or associated-function form, no `mul_add`, no bare integer casts without a `// cast-ok: <reason>` marker. A build-failing guard test enforces this. `abs` is exempt and used bare.
- **The new BLAKE2 dependency must be pinned exactly**, in the style of `libm = "=0.2.11"`. A floating version would make world generation depend on when the crate was built, which is precisely what DETERMINISM-001 forbids.
- **Constants transcribed character-for-character**: `DEFAULT_PLATE_COUNT = 22`, `JITTER_RAD = 0.18`, `SLOWEST_RAD_PER_MYR = 0.002`, `FASTEST_RAD_PER_MYR = 0.016`.
- **Never `f64::min` / `f64::max` / `clamp`.** Python's two-argument forms are asymmetric under NaN. Every clamp is an explicit `if`/`else` in the Python's operand order; `plates.rs::margin_at` has the house form.
- **Nothing under `worldbuilder/` may be modified.** The Python remains the reference. `worldbuilder/integration/maritime.py` has a pre-existing uncommitted change; leave it unstaged.
- **A floating-point sign, zero, or exact-equality assertion without the reason it holds is a latent bug.** Five have occurred in this port. Establish why each holds, or do not assert it.
- **Verify by exit status, not by grepping for `test result:` lines.** A defect in slice 1i survived three reviews because `cargo test` was failing on its doctest target while every review read its counts.

---

## What is different about this slice

Every earlier slice worried about agreement to within a few ULP. **This one has a step with no tolerance at all.**

```python
key = "|".join(str(part) for part in (world_seed,) + parts).encode("utf-8")
digest = hashlib.blake2b(key, digest_size=8).digest()
return struct.unpack("<Q", digest)[0] / float(1 << 64)
```

If the digest differs by a single bit, the resulting `u64` is unrelated, and so is every plate on the planet. There is no graceful degradation — the port either reproduces the bytes exactly or generates a different world.

**The good news, established by reading the source:** every part joined into that string is an `int` or a `str` literal — `world_seed`, the label `"plate"`, the index, and a label like `"pole-z"`. **No float ever reaches `str()`**, so the notoriously implementation-specific float-repr problem does not arise here. Strings look like `20260831|plate|7|pole-z`.

**Two traps in that snippet, both of which produce a silently different world:**

1. **`key` is the MESSAGE, not BLAKE2's key parameter.** `hashlib.blake2b(key, digest_size=8)` passes it as the first positional argument, which is `data`. The local variable name is misleading. Rust BLAKE2 APIs usually take the key as a separate constructor argument, so passing this as the key is an easy and catastrophic mistake — it produces a valid digest of the right length that is completely unrelated.
2. **`digest_size=8` is a BLAKE2b parameter, not a truncation.** BLAKE2b with an 8-byte output is *not* the first 8 bytes of the 64-byte digest — the digest length is mixed into the initial state. A Rust API offering a fixed-size `Blake2b512` and a `truncate` will give the wrong answer. You need the variable-output form (in the RustCrypto `blake2` crate, `Blake2bVar::new(8)`).

---

## The contract split, which is cleaner than it first appears

- **`_fraction` is bit-exact.** A hash, a little-endian `u64`, and a division by `2^64` — an exact power of two, so the division introduces no rounding beyond the `u64`-to-`f64` conversion, which both languages round to nearest. **No transcendental anywhere.** Compare with `same()`.
- **`_rate` is bit-exact.** `SLOWEST + fraction * (FASTEST - SLOWEST)`, then a sign. Pure arithmetic on an exact fraction.
- **`_spread` and `_pole` are bounded.** Both end in `cos` and `sin`.

---

## The three discrete decisions, and why two are safe

**`turning = _fraction(...) < 0.5` is safe, and for a better reason than usual.** It is a discrete decision on a continuous quantity — the shape that has caused trouble throughout this port — but the quantity is *exactly reproducible*: it comes from a byte-identical digest through integer conversion and a division by a power of two, with no transcendental in the path. Both implementations compare identical values. **State that as the reason.**

**`max(0.0, 1.0 - z * z)` appears in both `_spread` and `_pole.** Two-argument `max`, so NaN clamps to `0.0`. Explicit `if`/`else` in the Python's operand order.

**`if sideways.length() < 1e-9` in `_spread` is UNREACHABLE for any usable plate count, and the derivation is worth having.**

`sideways` is `Vec3(0, 0, 1).cross(point)`, which is `(-point.y, point.x, 0)`, so its length is `sqrt(x² + y²)` — the spiral's ring radius. With `z = 1 - 2(i + 0.5)/count`, write `u = (i + 0.5)/count`, so `z = 1 - 2u` and

```
1 - z² = 1 - (1 - 2u)² = 4u(1 - u),   so   ring = 2*sqrt(u(1 - u))
```

`u` is smallest at `i = 0`, giving `u = 0.5/count` and `ring ≈ sqrt(2/count)`. For `ring < 1e-9` you would need **`count > 2e18`** — more plates than the process could allocate. Since `i` is an integer in `[0, count)`, `u` is strictly inside `(0, 1)` and `z` never reaches `±1`.

**Port the guard anyway** — it is in the reference and removing it would change behaviour for an absurd count — but **comment that it is unreachable and why**, and let Task 3 assert it. This is the same shape as slice 1i's `across < 0.0`: the branch that looks most exposed is safe for a derivable reason.

---

## A transcription trap that would change every pole

`_spread` ends with `SpherePoint.from_vector(...)`, which **normalises**. `_pole` ends with `SpherePoint(Vec3(...))`, which **does not** — its vector is already unit by construction from `cos`, `sin` and `ring`.

Making `_pole` use `from_vector` would look like a tidy-up and would change every pole's bits. Check the ported `SpherePoint`'s constructors and preserve the distinction exactly.

---

## File Structure

- **Create** `crates/worldbuilder-engine/src/generation.rs`, declared in `src/lib.rs`.
- **Modify** `crates/worldbuilder-engine/Cargo.toml` — one pinned BLAKE2 dependency.
- **Modify** `crates/worldbuilder-engine/src/bindings.rs` and `src/lib.rs` — the bindings.
- **Modify** `tests/test_conformance.py` — the differential tests.
- **Modify** `crates/worldbuilder-engine/README.md` — the record.

Everything else this module needs is already ported: `SpherePoint`, `Vec3`, `Plate`, `PlateSet`, `detmath`.

---

### Task 1: Prove a Rust BLAKE2 reproduces CPython, before anything depends on it

**Files:** Modify `Cargo.toml`; create `tests/test_blake2_bytes.py` (throwaway; deleted in Task 7)

This task writes almost no engine code. It answers the one question with no tolerance.

- [ ] **Step 1: Add a pinned BLAKE2 dependency.** The RustCrypto `blake2` crate provides `Blake2bVar`, which is the variable-output form this needs. **Pin it exactly** (`blake2 = "=x.y.z"`), matching how `libm` is pinned, and say in your report which version you pinned and why that form rather than a fixed-size type.

- [ ] **Step 2: Compare digests byte for byte, on the strings this module actually produces.**

Build the exact inputs `_fraction` generates — `"{world_seed}|plate|{index}|{label}"` for each of the six labels `jitter-a`, `jitter-b`, `pole-z`, `pole-angle`, `rate`, `sense` — across several world seeds and indices, plus the edge cases: `world_seed = 0`, a negative seed, a very large seed, and `index = 0`. Compute `hashlib.blake2b(s.encode("utf-8"), digest_size=8).digest()` in Python and the same in Rust, and **compare the raw 8 bytes**, not the derived float.

**Expect exact equality. If any pair differs, stop and report it** — it means the crate or the parameters are wrong, and no amount of downstream work is worth anything until it is fixed. In particular check you have not passed the message as BLAKE2's *key*, and that `digest_size=8` is a real 8-byte BLAKE2b rather than a truncated 64-byte digest.

**One test vector, measured from the live Python rather than reproduced from memory** — use it as the first thing you check, because it fails loudly against both traps:

```
message  "20260831|plate|7|pole-z"   (UTF-8, 23 bytes)
digest   2d729d257c6a1550            (8 bytes, little-endian order as stored)
u64      5770635578984722989
fraction 0.3128267815678692
```

If your Rust reproduces those four lines, the crate, the digest size and the byte order are all right together. **Re-derive it yourself from the Python as well** — it is quoted here from one run, and a plan's numbers have been wrong before in this project.

- [ ] **Step 3: Verify the bytes-to-fraction conversion.** `struct.unpack("<Q", digest)[0] / float(1 << 64)`. Confirm the Rust reads the same little-endian `u64` and that `u64 as f64 / 2^64` is bit-identical to Python's, including for a digest whose `u64` exceeds `2^53` and therefore rounds on conversion. Report whether any tested value rounded.

- [ ] **Step 4: Confirm the unreachable guard.** Compute `ring` at `index = 0` for `count` in `{1, 2, 3, 22, 1000, 100000}` and report the minimum. Confirm it is many orders above `1e-9` and that the derivation `ring ≈ sqrt(2/count)` matches what you measure.

- [ ] **Step 5: Record the answers in the ledger with the numbers.** Do not proceed to Task 2 until they are recorded.

---

### Task 2: The constants and `_fraction`

**Files:** Create `crates/worldbuilder-engine/src/generation.rs`; modify `src/lib.rs`

**Interfaces:**
- Produces: `pub const DEFAULT_PLATE_COUNT: usize = 22;`, `JITTER_RAD`, `SLOWEST_RAD_PER_MYR`, `FASTEST_RAD_PER_MYR`
- Produces: `fn fraction(world_seed: i64, parts: &[Part]) -> f64` or equivalent — the signature is yours, but it must build **exactly** the string the Python builds.

The Python is variadic over mixed `int` and `str` parts. Rust has no such thing, so choose a representation — a small `enum Part { Int(i64), Str(&'static str) }`, or a pre-built `&str`, or a builder — and **say which you chose and why**. What matters is that the joined string is byte-identical, including the `|` separators and the absence of any trailing separator.

**Carry the module's own reason into a doc comment**, because it is a hard requirement rather than a preference: *"A generator that consumes a mutable sequence makes every plate depend on the order in which plates were built... Add a property to a plate six weeks from now and every subsequent plate silently changes, because the sequence shifted under it. Worlds people had sailed would quietly become different worlds."*

- [ ] **Step 1: Write the failing tests.**

```rust
#[test]
fn the_joined_key_is_byte_identical_to_the_python() {
    // "20260831|plate|7|pole-z" -- pipes between every part, none trailing.
    assert_eq!(joined_key(20260831, &[Part::Str("plate"), Part::Int(7), Part::Str("pole-z")]),
               "20260831|plate|7|pole-z");
}

#[test]
fn a_fraction_is_in_the_unit_interval_and_never_reaches_one() {
    // u64 / 2^64 is in [0, 1) by construction: the largest u64 is 2^64 - 1.
    for index in 0..64 {
        let f = fraction(20260831, &[Part::Str("plate"), Part::Int(index), Part::Str("rate")]);
        assert!(f >= 0.0 && f < 1.0, "fraction {f} out of range at index {index}");
    }
}

#[test]
fn the_same_arguments_always_give_the_same_fraction() {
    let a = fraction(20260831, &[Part::Str("plate"), Part::Int(7), Part::Str("pole-z")]);
    let b = fraction(20260831, &[Part::Str("plate"), Part::Int(7), Part::Str("pole-z")]);
    assert_eq!(a.to_bits(), b.to_bits());
}

#[test]
fn different_labels_give_unrelated_fractions() {
    // The whole design rests on this: plate 7's pole does not move when plate 6
    // gains a property. Different labels must not collide.
    let z = fraction(20260831, &[Part::Str("plate"), Part::Int(7), Part::Str("pole-z")]);
    let a = fraction(20260831, &[Part::Str("plate"), Part::Int(7), Part::Str("pole-angle")]);
    assert_ne!(z.to_bits(), a.to_bits());
}
```

**Replace the hard-coded `"20260831|plate|7|pole-z"` only if you verify the Python produces exactly that** for those arguments — run it and check, rather than trusting this plan.

- [ ] **Steps 2-5:** Run and confirm they fail for the expected reason, implement, run again, whole crate suite, commit.

---

### Task 3: `_spread` — the golden spiral

**Files:** Modify `crates/worldbuilder-engine/src/generation.rs`

**Interfaces:** Produces `fn spread(world_seed: i64, index: usize, count: usize) -> SpherePoint`

Transcribe lines 79-96. Points to write deliberately:

**`golden = math.pi * (3.0 - math.sqrt(5.0))` must be computed, not precomputed as a literal.** `sqrt(5.0)` is correctly rounded, `3.0 - sqrt(5.0)` is exact, and one multiplication by `PI` follows. Writing the decimal value instead would introduce a rounding this code does not have. Confirm Rust's `PI` is bit-identical to CPython's `math.pi` before relying on it — slice 1h confirmed `FRAC_PI_2` matched, so this is expected, but check.

**`max(0.0, 1.0 - z * z)`** — explicit `if`/`else`, NaN to `0.0`.

**The degeneracy guard** — port it, and comment that it is unreachable for any usable count, with the `ring ≈ sqrt(2/count)` derivation from Task 1.

**`SpherePoint::from_vector`** at the end — the normalising constructor. Do not substitute the direct one.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test]
fn the_degeneracy_guard_never_fires_for_a_usable_count() {
    // ring = 2*sqrt(u(1-u)) with u = (i + 0.5)/count, smallest at i = 0 where it is
    // about sqrt(2/count). Reaching the 1e-9 guard needs count > 2e18. Assert it
    // rather than believing the algebra: this pins the claim the port's comment makes.
    for &count in &[1usize, 2, 3, 22, 1000, 100_000] {
        let u = 0.5 / (count as f64);  // cast-ok: plate count to f64 for the bound
        let ring = 2.0 * detmath::sqrt(u * (1.0 - u));
        assert!(ring > 1e-6, "ring {ring} at count {count} approaches the guard");
    }
}

#[test]
fn every_seed_is_a_unit_vector() {
    // from_vector normalises, so this holds by construction -- it is here to catch
    // the direct constructor being substituted for it.
    for index in 0..22 {
        let p = spread(20260831, index, 22);
        assert!((p.vector.length() - 1.0).abs() < 1e-12, "seed {index} is not unit");
    }
}

#[test]
fn distinct_indices_give_distinct_seeds() {
    let seeds: Vec<_> = (0..22).map(|i| spread(20260831, i, 22)).collect();
    for i in 0..seeds.len() {
        for j in (i + 1)..seeds.len() {
            let d = seeds[i].vector.sub(&seeds[j].vector).length();
            assert!(d > 1e-6, "seeds {i} and {j} collide");
        }
    }
}
```

**Also write a test that the jitter actually moves the point**, by comparing against the same computation with the nudges forced to zero and requiring a difference. A jitter wired to nothing would otherwise pass every test above. **You will need to expose that comparison somehow — say how you did it**, and do not weaken it to a tolerance that a zero jitter would also satisfy.

- [ ] **Steps 2-5:** Run and confirm they fail for the expected reason, implement, run again, whole crate suite, commit.

---

### Task 4: `_pole` and `_rate`

**Files:** Modify `crates/worldbuilder-engine/src/generation.rs`

**Interfaces:** Produces `fn pole(world_seed: i64, index: usize) -> SpherePoint` and `fn rate(world_seed: i64, index: usize) -> f64`

**`_pole` uses `SpherePoint(Vec3(...))` directly — the NON-normalising constructor** — because the vector is already unit by construction. Preserve that; see "A transcription trap that would change every pole" above.

**`_rate` is bit-exact**: `SLOWEST + fraction * (FASTEST - SLOWEST)`, then `-speed if turning else speed` where `turning = fraction < 0.5`. That comparison is a discrete decision on an *exactly* reproducible value, so it is safe — comment the reason.

The docstring records why the sign lives here: *"The sign is what makes a rotation clockwise or otherwise, so it lives here rather than in a separate flag that could disagree with the pole."*

- [ ] **Step 1: Write the failing tests**, including: poles are unit vectors; z is uniform rather than latitude-uniform (sample many and check the distribution of z is flat while the distribution of latitude is not — the docstring says crowding the poles was the bug this avoids); rates fall within `[SLOWEST, FASTEST]` in magnitude; and both signs occur across a run of indices. **Derive the sample size you need rather than picking one**, and say how.
- [ ] **Steps 2-5:** Run, implement, run, commit.

---

### Task 5: `plates_for`

**Files:** Modify `crates/worldbuilder-engine/src/generation.rs`

**Interfaces:** Produces `pub fn plates_for(world_seed: i64, count: usize) -> PlateSet`

`index` runs over `range(count)` and each `Plate` takes `index=index`. **This is the assignment that makes `index == position` true**, which several earlier slices depend on — the bisector and seed tables are addressed by position, and the two coincide only because of this line. Comment it.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test]
fn every_plate_has_index_equal_to_its_position() {
    // The line several earlier slices rest on. plates.rs addresses its bisector and
    // seed tables by POSITION, and that only agrees with the .index field because
    // this loop assigns them together. If it ever stopped holding, the position-
    // indexing ruling from slice 1f would silently address the wrong rows.
    let set = plates_for(20260831, 22);
    assert_eq!(set.plates().len(), 22);
    for (position, plate) in set.plates().iter().enumerate() {
        assert_eq!(plate.index, position, "index and position must agree");
    }
}

#[test]
fn the_same_seed_always_builds_the_same_world() {
    // Bit-for-bit, not approximately: nothing here is time-, order- or
    // allocation-dependent, so two calls must be indistinguishable.
    let a = plates_for(20260831, 22);
    let b = plates_for(20260831, 22);
    for (x, y) in a.plates().iter().zip(b.plates().iter()) {
        assert_eq!(x.seed.vector.x.to_bits(), y.seed.vector.x.to_bits());
        assert_eq!(x.euler_pole.vector.z.to_bits(), y.euler_pole.vector.z.to_bits());
        assert_eq!(x.rate_rad_per_myr.to_bits(), y.rate_rad_per_myr.to_bits());
    }
}

#[test]
fn a_different_seed_builds_a_different_world() {
    // Weak on its own, but it would catch a world_seed wired to nothing -- which
    // would otherwise pass every other test in this task.
    let a = plates_for(20260831, 22);
    let b = plates_for(20260832, 22);
    let differs = a.plates().iter().zip(b.plates().iter())
        .any(|(x, y)| x.rate_rad_per_myr.to_bits() != y.rate_rad_per_myr.to_bits());
    assert!(differs, "changing the seed must change the world");
}

#[test]
fn a_count_of_one_still_builds_a_world() {
    // The spiral's z = 1 - 2*(0 + 0.5)/1 = 0.0 exactly, so the single seed sits on
    // the equator with ring = 1. Confirm rather than assume; a count of one is the
    // degenerate case most likely to divide by something.
    let set = plates_for(20260831, 1);
    assert_eq!(set.plates().len(), 1);
}
```

Adjust the accessor names to whatever `PlateSet` actually exposes — read it rather than guessing.

- [ ] **Steps 2-5:** Run and confirm they fail for the expected reason, implement, run again, whole crate suite, commit.

---

### Task 6: Conformance

**Files:** Modify `crates/worldbuilder-engine/src/bindings.rs`, `src/lib.rs`, `tests/test_conformance.py`

**Interfaces:**
- `generation_fraction(world_seed, label_parts) -> f64`
- `generation_spread(world_seed, index, count) -> (f64, f64, f64)`
- `generation_pole(world_seed, index) -> (f64, f64, f64)`
- `generation_rate(world_seed, index) -> f64`
- `generation_plates_for(world_seed, count) -> list[(usize, (f64,f64,f64), (f64,f64,f64), f64)]`

**Apply the contract split:**
- **`fraction` and `rate`: strict, `same()`, bit-for-bit.** No transcendental is in either path, so a mismatch is a defect, not rounding.
- **`spread` and `pole`: bounded**, `close_enough` at `MAX_TRANSCENDENTAL_ULPS`.
- **Plate indices: exact integers.**

Cover: several world seeds including `0`, a negative seed and a large one; `count` of 1, 2, 22 and something large; and every index in a full set.

- [ ] **Step 1: Add the bindings, conversion only.**
- [ ] **Step 2: Rebuild** with `maturin develop --release`.
- [ ] **Step 3: Add the conformance tests.**
- [ ] **Step 4: Measure and assert, do not print.** Report the worst ULP distance for `spread` and `pole`, and **assert the strict comparisons genuinely hold for `fraction` and `rate` across every case** — a single differing digest would show here as a wildly different value, not a near miss.
- [ ] **Step 5: Run both suites, quote them, check exit status.**
- [ ] **Step 6: Commit.**

---

### Task 7: Record it

**Files:** Modify `crates/worldbuilder-engine/README.md`; delete `tests/test_blake2_bytes.py`

- [ ] **Step 1: Record** — that this is the only step in the port with **no tolerance at all**, since a one-bit digest difference produces an unrelated world; which BLAKE2 crate and version were pinned and why the variable-output form; the two traps (the message-versus-key argument, and `digest_size=8` not being a truncation); that no float reaches `str()` so the float-repr problem does not arise; the contract split, with `fraction` and `rate` strict and `spread`/`pole` bounded; **why `turning = fraction < 0.5` is safe** — a discrete decision on an exactly reproducible value; **why the degeneracy guard is unreachable**, with the `ring ≈ sqrt(2/count)` derivation; the `from_vector`-versus-direct constructor distinction; and that `plates_for` is what makes `index == position` true, which earlier slices rely on.
- [ ] **Step 2: Delete the throwaway** `tests/test_blake2_bytes.py`.
- [ ] **Step 3: Verify every count by running the suites and checking exit status.** Do not copy a number from any report.
- [ ] **Step 4: Commit.**

---

## What this slice deliberately does not do

- **No shelves, erosion, bathymetry or detail.** Those compose on top of the tectonic contribution.
- **No deletion of the Python.** It stays the reference.
- **With this slice the `plates` package is fully ported** — `model.py`, `lookup.py`, `kinematics.py` and `generation.py` — and so is `terrain/tectonics.py`.
