# Slice 0 — Native/WASM Bit-Equality Spike — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Determine, by measurement, whether a Rust terrain field compiled natively and to WebAssembly returns bit-identical `f64` results — because the entire Mark 2 architecture rests on the answer being yes.

**Architecture:** A small Rust crate that is *not* the generator. It implements a probe field exercising every arithmetic and transcendental operation the real generator uses, routed through a single `detmath` module backed by the pure-Rust `libm` crate. The crate builds twice — a native binary and a `wasm32-unknown-unknown` cdylib. Each build evaluates the same corpus of inputs and writes each result as its raw 64-bit pattern. The two files are compared byte-for-byte. A deliberate negative control proves the comparison can detect a difference when one exists.

**Tech Stack:** Rust (stable, via rustup), the `libm` crate, `wasm32-unknown-unknown` target, Node 22 as the WASM host (raw `WebAssembly.instantiate`, no bindgen, no WASI), Python 3 for comparison and reporting.

**Spec:** `docs/design/2026-09-02-mark-2-world-studio.md` — sections 4.1 (the precondition), 4.2 (DETERMINISM-001), 20 (slice order).

**This is a spike. Its output is an answer, not code we keep.** It lives in `spikes/0-bit-equality/`, nothing in `worldbuilder/` or `crates/` may ever import it, and it is not the beginning of the engine. If it tempts anyone to "just extend it into slice 1", that is the failure mode this paragraph exists to prevent.

## Global Constraints

- **Rust edition 2021, stable toolchain.** No nightly features.
- **No `std` transcendental functions anywhere in probe code.** Not `f64::sin`, `f64::cos`, `f64::sqrt`, `f64::hypot`, `f64::atan2`, `f64::asin`, `f64::tanh`, `f64::powf`. All must go through `detmath`. `f64::sqrt` is the tempting exception and is still banned — see Task 3.
- **`libm` is the only dependency.** Version pinned exactly, not a caret range.
- **No GPU, no SIMD intrinsics, no `-ffast-math`-equivalent flags.** Do not add `-C target-feature=+simd128` to the WASM build.
- **Comparison is on raw bits**, via `f64::to_bits()`, never on the decimal rendering of a float and never with a tolerance. A tolerance-based comparison would pass this spike while the architecture was broken, which is the exact failure we are paying to avoid.
- **The operations the real generator uses**, established by inventory of `worldbuilder/`: `sin` (15 call sites), `cos` (17), `sqrt` (14), `hypot` (10), `atan2` (4), `asin` (3), `tanh` (1), plus 64-bit integer hashing and ordinary `f64` arithmetic. `powf` is included ahead of erosion (section 14).
- **Environment as found:** no Rust toolchain is installed. Node is v22.17.0. Python is 3.11.0. Task 1 installs Rust.

---

## File Structure

    spikes/0-bit-equality/
      README.md              what this is, that it is throwaway, and the answer it produced
      Cargo.toml             the crate; libm pinned; two build targets
      src/detmath.rs         the only place a transcendental is called
      src/probe.rs           the probe field: every operation class, one f64 out
      src/corpus.rs          deterministic input generation, shared by both builds
      src/main.rs            native binary: evaluate corpus, write bits to a file
      src/lib.rs             cdylib: export probe_at(index) -> f64 for the WASM host
      host/run_wasm.mjs      Node harness: instantiate the .wasm, write bits to a file
      compare.py             byte comparison, first-divergence report, verdict
      results/               native.bits, wasm.bits, verdict.txt  (git-ignored)

`detmath.rs` exists as a separate file rather than a module inside `probe.rs` for one
reason: it is the file DETERMINISM-001's future CI lint will police, and a lint is easier to
write against a directory rule than a symbol rule.

---

### Task 1: Toolchain and a crate that builds twice

**Files:**
- Create: `spikes/0-bit-equality/Cargo.toml`
- Create: `spikes/0-bit-equality/src/lib.rs`
- Create: `spikes/0-bit-equality/src/main.rs`
- Create: `spikes/0-bit-equality/.gitignore`

**Interfaces:**
- Consumes: nothing.
- Produces: a crate that builds as both a native binary and a `wasm32-unknown-unknown` cdylib exporting `probe_at(u64) -> f64`.

- [ ] **Step 1: Install the Rust toolchain**

Rust is not present on this machine. Download and run the installer, accepting defaults:

```bash
curl -sSf -o /tmp/rustup-init.exe https://win.rustup.rs/x86_64
/tmp/rustup-init.exe -y --default-toolchain stable --profile minimal
```

Then open a new shell so `PATH` picks up `~/.cargo/bin`, or export it for this session:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

- [ ] **Step 2: Verify the toolchain and add the WASM target**

```bash
rustc --version && cargo --version
rustup target add wasm32-unknown-unknown
rustup target list --installed
```

Expected: a stable version string for both, and `wasm32-unknown-unknown` listed among installed targets.

- [ ] **Step 3: Write the crate manifest**

Create `spikes/0-bit-equality/Cargo.toml`:

```toml
[package]
name = "bit_equality_spike"
version = "0.0.0"
edition = "2021"
publish = false

[dependencies]
libm = "=0.2.11"

[lib]
crate-type = ["cdylib", "rlib"]
path = "src/lib.rs"

[[bin]]
name = "native_probe"
path = "src/main.rs"

[profile.release]
# Determinism first. No fast-math equivalents, no aggressive float reassociation.
opt-level = 2
lto = false
codegen-units = 1
panic = "abort"
```

`codegen-units = 1` is deliberate: multiple codegen units can make optimisation decisions
differ between builds, and we are trying to isolate target differences, not codegen noise.

- [ ] **Step 4: Write a placeholder library and binary**

Create `spikes/0-bit-equality/src/lib.rs`:

```rust
//! Throwaway spike. Nothing in this crate is the engine.

pub mod corpus;
pub mod detmath;
pub mod probe;

/// Exported to the WASM host. Returns the probe value for one corpus index.
#[no_mangle]
pub extern "C" fn probe_at(index: u64) -> f64 {
    let input = corpus::input_at(index);
    probe::evaluate(&input)
}
```

Create `spikes/0-bit-equality/src/main.rs`:

```rust
fn main() {
    println!("{}", bit_equality_spike::probe_at(0));
}
```

Create `spikes/0-bit-equality/.gitignore`:

```
target/
results/
```

- [ ] **Step 5: Create the three modules as empty stubs so the crate compiles**

Create `spikes/0-bit-equality/src/detmath.rs`:

```rust
//! The only place in this crate that may call a transcendental function.
```

Create `spikes/0-bit-equality/src/corpus.rs`:

```rust
/// One corpus sample: the inputs a single probe evaluation takes.
pub struct Input {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub seed: u64,
}

pub fn input_at(_index: u64) -> Input {
    Input { x: 0.0, y: 0.0, z: 1.0, seed: 1 }
}
```

Create `spikes/0-bit-equality/src/probe.rs`:

```rust
use crate::corpus::Input;

pub fn evaluate(_input: &Input) -> f64 {
    0.0
}
```

- [ ] **Step 6: Verify both targets build**

```bash
cd spikes/0-bit-equality
cargo build --release
cargo build --release --target wasm32-unknown-unknown
ls -l target/release/native_probe.exe target/wasm32-unknown-unknown/release/bit_equality_spike.wasm
```

Expected: both build with no errors, and both artifacts exist.

- [ ] **Step 7: Commit**

```bash
git add spikes/0-bit-equality
git commit -m "spike: a crate that builds native and wasm, and nothing else yet"
```

---

### Task 2: detmath, the single door to transcendentals

**Files:**
- Modify: `spikes/0-bit-equality/src/detmath.rs`
- Test: `spikes/0-bit-equality/src/detmath.rs` (inline `#[cfg(test)]` module)

**Interfaces:**
- Consumes: the `libm` crate.
- Produces: `detmath::{sin, cos, sqrt, hypot, atan2, asin, tanh, powf}`, each `fn(f64[, f64]) -> f64`.

- [ ] **Step 1: Write the failing test**

Append to `spikes/0-bit-equality/src/detmath.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_op_is_routed_and_returns_a_finite_value() {
        assert!(sin(0.7).is_finite());
        assert!(cos(0.7).is_finite());
        assert!(sqrt(2.0).is_finite());
        assert!(hypot(3.0, 4.0).is_finite());
        assert!(atan2(1.0, 2.0).is_finite());
        assert!(asin(0.5).is_finite());
        assert!(tanh(0.5).is_finite());
        assert!(powf(2.0, 0.5).is_finite());
    }

    #[test]
    fn hypot_of_three_and_four_is_exactly_five() {
        assert_eq!(hypot(3.0, 4.0).to_bits(), 5.0f64.to_bits());
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cd spikes/0-bit-equality && cargo test
```

Expected: FAIL — `cannot find function 'sin' in this scope` and the same for the others.

- [ ] **Step 3: Write the implementation**

Insert above the test module in `spikes/0-bit-equality/src/detmath.rs`:

```rust
//! The only place in this crate that may call a transcendental function.
//!
//! std's trigonometry dispatches to the platform's libm, and the platform differs between a
//! native host and a WASM runtime. The differences are in the last bits, which is precisely
//! where a coastline is decided. Routing every call through the pure-Rust `libm` crate means
//! both targets execute the same instructions over the same values.
//!
//! `sqrt` is included even though IEEE-754 requires it to be correctly rounded, and is
//! therefore safe. It is routed anyway so that the rule is "no std math, ever" rather than
//! "no std math except the ones somebody judged safe" - a rule with an exception list is a
//! rule that erodes.

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
```

- [ ] **Step 4: Run the test to verify it passes**

```bash
cd spikes/0-bit-equality && cargo test
```

Expected: PASS, 2 tests.

- [ ] **Step 5: Commit**

```bash
git add spikes/0-bit-equality/src/detmath.rs
git commit -m "spike: one door for transcendentals, and no exception list"
```

---

### Task 3: The corpus — deterministic, adversarial inputs

**Files:**
- Modify: `spikes/0-bit-equality/src/corpus.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `corpus::input_at(index: u64) -> Input`, and `corpus::COUNT: u64`. `Input` has public fields `x`, `y`, `z` (an approximately unit vector) and `seed`.

The corpus must not be a tidy grid. A grid samples the same fractional bit patterns
repeatedly and would hide a difference that only appears at awkward mantissas. It also must
be generated identically on both targets, so it uses integer hashing and no transcendentals.

- [ ] **Step 1: Write the failing test**

Append to `spikes/0-bit-equality/src/corpus.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inputs_are_reproducible() {
        let a = input_at(12345);
        let b = input_at(12345);
        assert_eq!(a.x.to_bits(), b.x.to_bits());
        assert_eq!(a.y.to_bits(), b.y.to_bits());
        assert_eq!(a.z.to_bits(), b.z.to_bits());
        assert_eq!(a.seed, b.seed);
    }

    #[test]
    fn inputs_differ_between_indices() {
        let a = input_at(1);
        let b = input_at(2);
        assert_ne!(a.x.to_bits(), b.x.to_bits());
    }

    #[test]
    fn includes_the_awkward_places() {
        // Poles, the meridian, and the equator are where a spherical field is most likely
        // to be wrong, so the first few indices are pinned to them rather than hashed.
        let pole = input_at(0);
        assert_eq!(pole.z.to_bits(), 1.0f64.to_bits());
        let equator = input_at(2);
        assert_eq!(equator.z.to_bits(), 0.0f64.to_bits());
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cd spikes/0-bit-equality && cargo test corpus
```

Expected: FAIL — `includes_the_awkward_places` and `inputs_differ_between_indices` fail, because the stub returns a constant.

- [ ] **Step 3: Write the implementation**

Replace the contents of `spikes/0-bit-equality/src/corpus.rs` above the test module:

```rust
//! Corpus generation. Integer hashing only - no transcendentals, so the corpus itself
//! cannot be the thing that differs between targets.

/// One corpus sample.
pub struct Input {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub seed: u64,
}

/// How many samples a full run evaluates.
pub const COUNT: u64 = 5_000_000;

/// A 64-bit avalanche. Same shape as the generator's lattice hash, in wrapping u64
/// arithmetic so the semantics are stated rather than inherited.
fn mix(mut h: u64) -> u64 {
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51AFD7ED558CCD);
    h ^= h >> 33;
    h = h.wrapping_mul(0xC4CEB9FE1A85EC53);
    h ^= h >> 33;
    h
}

/// A float in [-1, 1), from the top 53 bits so the mantissa is fully exercised.
fn unit(h: u64) -> f64 {
    let f = (h >> 11) as f64 / (1u64 << 53) as f64;
    f * 2.0 - 1.0
}

pub fn input_at(index: u64) -> Input {
    // The first four indices are pinned to the places a sphere is most likely to break.
    match index {
        0 => return Input { x: 0.0, y: 0.0, z: 1.0, seed: 1 },
        1 => return Input { x: 0.0, y: 0.0, z: -1.0, seed: 1 },
        2 => return Input { x: 1.0, y: 0.0, z: 0.0, seed: 1 },
        3 => return Input { x: -1.0, y: 0.0, z: 0.0, seed: 1 },
        _ => {}
    }

    let hx = mix(index.wrapping_mul(0x9E3779B97F4A7C15));
    let hy = mix(index.wrapping_mul(0xC2B2AE3D27D4EB4F) ^ 0xA5A5A5A5A5A5A5A5);
    let hz = mix(index.wrapping_mul(0x165667B19E3779F9) ^ 0x5A5A5A5A5A5A5A5A);
    let hs = mix(index ^ 0x27D4EB2F165667C5);

    Input {
        x: unit(hx),
        y: unit(hy),
        z: unit(hz),
        // A handful of seeds rather than one, so a seed-dependent divergence is visible.
        seed: (hs % 7) + 1,
    }
}
```

Note these are *not* normalised to the unit sphere here. Normalisation is part of the probe
(Task 4), because normalisation itself uses `sqrt` and is therefore something the spike is
supposed to be testing rather than assuming.

- [ ] **Step 4: Run the test to verify it passes**

```bash
cd spikes/0-bit-equality && cargo test corpus
```

Expected: PASS, 3 tests.

- [ ] **Step 5: Commit**

```bash
git add spikes/0-bit-equality/src/corpus.rs
git commit -m "spike: a corpus that is hashed rather than tidy, and pinned at the poles"
```

---

### Task 4: The probe field

**Files:**
- Modify: `spikes/0-bit-equality/src/probe.rs`

**Interfaces:**
- Consumes: `detmath::*`, `corpus::Input`.
- Produces: `probe::evaluate(input: &Input) -> f64`.

The probe is not the generator and does not try to be. Its job is to execute, in one
expression chain, every *class* of operation the real generator performs — so that if any
class differs between targets, this catches it.

- [ ] **Step 1: Write the failing test**

Append to `spikes/0-bit-equality/src/probe.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus;

    #[test]
    fn evaluation_is_finite_across_the_corpus_head() {
        for i in 0..1000u64 {
            let v = evaluate(&corpus::input_at(i));
            assert!(v.is_finite(), "index {} produced {}", i, v);
        }
    }

    #[test]
    fn evaluation_is_reproducible() {
        let a = evaluate(&corpus::input_at(4242));
        let b = evaluate(&corpus::input_at(4242));
        assert_eq!(a.to_bits(), b.to_bits());
    }

    #[test]
    fn different_seeds_give_different_answers() {
        let a = evaluate(&Input { x: 0.3, y: 0.4, z: 0.5, seed: 1 });
        let b = evaluate(&Input { x: 0.3, y: 0.4, z: 0.5, seed: 2 });
        assert_ne!(a.to_bits(), b.to_bits());
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cd spikes/0-bit-equality && cargo test probe
```

Expected: FAIL — `different_seeds_give_different_answers` fails, because the stub returns `0.0` regardless.

- [ ] **Step 3: Write the implementation**

Replace the contents of `spikes/0-bit-equality/src/probe.rs` above the test module:

```rust
//! The probe field. Not the generator - a deliberately small function that touches every
//! class of operation the generator uses, so that a divergence in any of them shows up.

use crate::corpus::Input;
use crate::detmath as m;

/// Lattice hash, in the same shape the Python generator uses, but stated in wrapping u64
/// arithmetic. See the note in the spike README about why this is not automatically the
/// same as the Python original.
fn lattice(ix: i64, iy: i64, iz: i64, seed: u64) -> f64 {
    let h = (ix as u64).wrapping_mul(0x9E3779B97F4A7C15)
        ^ (iy as u64).wrapping_mul(0xC2B2AE3D27D4EB4F)
        ^ (iz as u64).wrapping_mul(0x165667B19E3779F9);
    let mut h = h ^ seed.wrapping_mul(0x27D4EB2F165667C5);
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51AFD7ED558CCD);
    h ^= h >> 33;
    h = h.wrapping_mul(0xC4CEB9FE1A85EC53);
    h ^= h >> 33;
    h as f64 / (u64::MAX as f64 + 1.0)
}

pub fn evaluate(input: &Input) -> f64 {
    // 1. Normalisation - sqrt, and the operation every sphere point begins with.
    let len = m::sqrt(input.x * input.x + input.y * input.y + input.z * input.z);
    let len = if len == 0.0 { 1.0 } else { len };
    let (x, y, z) = (input.x / len, input.y / len, input.z / len);

    // 2. Spherical coordinates - asin and atan2, as the geometry layer does.
    let lat = m::asin(z.clamp(-1.0, 1.0));
    let lon = m::atan2(y, x);

    // 3. Trigonometry back out, as tangent frames and Euler-pole rotation do.
    let a = m::sin(lat * 3.0) * m::cos(lon * 2.0);

    // 4. Planar distance - hypot, as margin and feature distance do.
    let d = m::hypot(x * 1000.0, y * 1000.0);

    // 5. A saturating blend - tanh, as the shelf and slope shaping do.
    let s = m::tanh(d / 700.0 + a);

    // 6. A fractional power - powf, ahead of the stream power equation of section 14.
    let p = m::powf(d.abs() + 1.0, 0.5);

    // 7. Integer hashing into a float, as the noise lattice does.
    let ix = (x * 4096.0) as i64;
    let iy = (y * 4096.0) as i64;
    let iz = (z * 4096.0) as i64;
    let n = lattice(ix, iy, iz, input.seed);

    // 8. An order-sensitive accumulation. Summed low-to-high deliberately: if any build
    //    reassociates this, the result changes, which is exactly what we want to detect.
    let mut acc = 0.0f64;
    for k in 1..=16u32 {
        let w = 1.0 / (k as f64);
        acc += w * m::sin(a * k as f64 + n);
    }

    a + s + p * 1e-6 + n + acc
}
```

- [ ] **Step 4: Run the test to verify it passes**

```bash
cd spikes/0-bit-equality && cargo test probe
```

Expected: PASS, 3 tests.

- [ ] **Step 5: Commit**

```bash
git add spikes/0-bit-equality/src/probe.rs
git commit -m "spike: a probe that touches every operation class the generator uses"
```

---

### Task 5: The native run

**Files:**
- Modify: `spikes/0-bit-equality/src/main.rs`

**Interfaces:**
- Consumes: `corpus::COUNT`, `probe_at`.
- Produces: `results/native.bits` — `COUNT` lines, each the 16-character lowercase hex of one result's `to_bits()`.

Text hex rather than raw binary, because the first thing anyone does when the files differ
is look at them, and a hex line with an index is far easier to reason about than an offset
into a binary blob.

- [ ] **Step 1: Write the implementation**

Replace `spikes/0-bit-equality/src/main.rs`:

```rust
use std::env;
use std::fs::{create_dir_all, File};
use std::io::{BufWriter, Write};

use bit_equality_spike::{corpus, probe_at};

fn main() {
    let count: u64 = env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(corpus::COUNT);

    create_dir_all("results").expect("create results/");
    let file = File::create("results/native.bits").expect("create results/native.bits");
    let mut out = BufWriter::new(file);

    for i in 0..count {
        writeln!(out, "{:016x}", probe_at(i).to_bits()).expect("write");
    }
    out.flush().expect("flush");
    eprintln!("wrote {} samples to results/native.bits", count);
}
```

- [ ] **Step 2: Run a small batch and inspect it**

```bash
cd spikes/0-bit-equality
cargo run --release --bin native_probe -- 10
cat results/native.bits
```

Expected: ten 16-character hex lines, not all identical, none reading `0000000000000000`.

- [ ] **Step 3: Commit**

```bash
git add spikes/0-bit-equality/src/main.rs
git commit -m "spike: the native run writes raw bits as readable hex"
```

---

### Task 6: The WASM run

**Files:**
- Create: `spikes/0-bit-equality/host/run_wasm.mjs`

**Interfaces:**
- Consumes: `target/wasm32-unknown-unknown/release/bit_equality_spike.wasm`, exporting `probe_at(index: BigInt) -> number`.
- Produces: `results/wasm.bits`, byte-identical in format to `results/native.bits`.

No bindgen and no WASI. The export takes a `u64` and returns an `f64`, which crosses the
boundary as a BigInt and a Number respectively — nothing else is needed, and every layer we
do not add is a layer that cannot introduce a difference of its own.

- [ ] **Step 1: Write the harness**

Create `spikes/0-bit-equality/host/run_wasm.mjs`:

```javascript
// Runs the WASM build of the probe over the same corpus as the native build, and writes
// the results in the same format. Deliberately dependency-free.

import { readFile, writeFile, mkdir } from "node:fs/promises";

const count = BigInt(process.argv[2] ?? 5_000_000);
const wasmPath = "target/wasm32-unknown-unknown/release/bit_equality_spike.wasm";

const bytes = await readFile(wasmPath);
const { instance } = await WebAssembly.instantiate(bytes, {});
const probeAt = instance.exports.probe_at;
if (typeof probeAt !== "function") {
  throw new Error("probe_at is not exported from the wasm module");
}

// One f64 scratch buffer, reused, so the bit pattern is read without allocation per sample.
const scratch = new DataView(new ArrayBuffer(8));
const lines = [];

for (let i = 0n; i < count; i++) {
  scratch.setFloat64(0, probeAt(i));
  const hi = scratch.getUint32(0).toString(16).padStart(8, "0");
  const lo = scratch.getUint32(4).toString(16).padStart(8, "0");
  lines.push(hi + lo);
}

await mkdir("results", { recursive: true });
await writeFile("results/wasm.bits", lines.join("\n") + "\n");
console.error(`wrote ${count} samples to results/wasm.bits`);
```

- [ ] **Step 2: Build the WASM and run ten samples**

```bash
cd spikes/0-bit-equality
cargo build --release --target wasm32-unknown-unknown
node host/run_wasm.mjs 10
cat results/wasm.bits
```

Expected: ten hex lines in the same format as `native.bits`.

- [ ] **Step 3: Eyeball the two files against each other**

```bash
cd spikes/0-bit-equality
cargo run --release --bin native_probe -- 10
diff results/native.bits results/wasm.bits && echo "IDENTICAL at n=10"
```

Expected: either `IDENTICAL at n=10`, or a diff. **Both are valid outcomes of a spike.** Do
not adjust the probe to make them match — record what happened.

- [ ] **Step 4: Commit**

```bash
git add spikes/0-bit-equality/host/run_wasm.mjs
git commit -m "spike: a dependency-free wasm host, no bindgen and no WASI"
```

---

### Task 7: The comparison, and the control that proves it works

**Files:**
- Create: `spikes/0-bit-equality/compare.py`

**Interfaces:**
- Consumes: `results/native.bits`, `results/wasm.bits`.
- Produces: `results/verdict.txt`, and exit status 0 for identical, 1 for divergent.

A spike that always reports "identical" because its comparison is broken is worse than no
spike, so this task builds the negative control before trusting the verdict.

- [ ] **Step 1: Write the comparison tool**

Create `spikes/0-bit-equality/compare.py`:

```python
"""Compare two bit files. Reports the first divergence and the total count."""

import sys


def read(path):
    with open(path, encoding="ascii") as handle:
        return [line.strip() for line in handle if line.strip()]


def main():
    native = read("results/native.bits")
    wasm = read("results/wasm.bits")

    lines = []
    if len(native) != len(wasm):
        lines.append(f"LENGTH MISMATCH: native={len(native)} wasm={len(wasm)}")

    n = min(len(native), len(wasm))
    diffs = [i for i in range(n) if native[i] != wasm[i]]

    lines.append(f"samples compared: {n}")
    lines.append(f"divergent:        {len(diffs)}")

    if diffs:
        first = diffs[0]
        lines.append(f"first divergence at index {first}")
        lines.append(f"  native {native[first]}")
        lines.append(f"  wasm   {wasm[first]}")
        a = int(native[first], 16)
        b = int(wasm[first], 16)
        lines.append(f"  differing bits: {bin(a ^ b).count('1')}")
        lines.append("VERDICT: DIVERGENT")
    else:
        lines.append("VERDICT: IDENTICAL")

    text = "\n".join(lines) + "\n"
    with open("results/verdict.txt", "w", encoding="ascii") as handle:
        handle.write(text)
    print(text, end="")
    return 1 if diffs or len(native) != len(wasm) else 0


if __name__ == "__main__":
    sys.exit(main())
```

- [ ] **Step 2: Run the comparison on the ten-sample files**

```bash
cd spikes/0-bit-equality && python compare.py
```

Expected: a verdict, either way. Record it.

- [ ] **Step 3: Build the negative control**

Temporarily replace the body of `detmath::sin` so the native build uses std and the WASM
build uses libm — a difference the harness *must* catch:

```rust
pub fn sin(x: f64) -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        libm::sin(x)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        #[allow(clippy::disallowed_methods)]
        x.sin()
    }
}
```

- [ ] **Step 4: Verify the control is detected**

```bash
cd spikes/0-bit-equality
cargo build --release --target wasm32-unknown-unknown
cargo run --release --bin native_probe -- 100000
node host/run_wasm.mjs 100000
python compare.py
```

Expected: `VERDICT: DIVERGENT`, with a non-zero count. If this reports IDENTICAL, the
harness is broken and **nothing downstream can be believed** — stop and fix it before
proceeding. (It is possible that std and libm agree on this platform for these inputs; if
the count is zero, make the control blatant instead — return `x.sin() + 1e-15` on native —
and confirm the harness catches that.)

- [ ] **Step 5: Revert the control and commit**

```bash
cd spikes/0-bit-equality
git checkout src/detmath.rs
git add compare.py
git commit -m "spike: compare bits, and prove the comparison can fail"
```

---

### Task 8: The full run, and the answer

**Files:**
- Create: `spikes/0-bit-equality/README.md`
- Modify: `docs/design/2026-09-02-mark-2-world-studio.md` (section 4.1)

**Interfaces:**
- Consumes: everything above.
- Produces: a recorded answer to the question the spike exists to ask.

- [ ] **Step 1: Run the full corpus on both targets**

```bash
cd spikes/0-bit-equality
cargo build --release
cargo build --release --target wasm32-unknown-unknown
time cargo run --release --bin native_probe -- 5000000
time node host/run_wasm.mjs 5000000
python compare.py
```

Record the two wall-clock times as well as the verdict — they are the first real data on
how fast the Rust field is, and slice 1 will want them.

- [ ] **Step 2: Write the spike README**

Create `spikes/0-bit-equality/README.md`, filling in the bracketed values from the run:

```markdown
# Slice 0 — native/WASM bit-equality

**Throwaway.** Nothing in `worldbuilder/` or `crates/` may import this. It is not the
beginning of the engine; it is a question with a workbench attached.

## The question

Do a Rust field compiled natively and to `wasm32-unknown-unknown` return bit-identical
`f64` results? Section 4.1 of the Mark 2 spec makes the studio, the provider and the
one-source argument all conditional on the answer.

## The answer

    samples      5,000,000
    verdict      [IDENTICAL | DIVERGENT]
    native       [X.X] s
    wasm         [X.X] s

[If divergent: first divergence at index N, differing in K bits, in operation class ...]

## What was tested

Normalisation (`sqrt`), spherical coordinates (`asin`, `atan2`), trigonometry (`sin`,
`cos`), planar distance (`hypot`), saturating blend (`tanh`), fractional power (`powf`),
64-bit lattice hashing, and an order-sensitive 16-term accumulation. That is every
operation class the Python generator uses, established by inventory rather than assumed.

The comparison was proved capable of failing before its passing result was believed: a
negative control routing native `sin` through std while WASM used `libm` was detected.

## What was NOT tested, and matters

**Python-to-Rust equality.** This spike compares Rust to Rust. It says nothing about
whether the Rust core reproduces the existing Python generator bit-for-bit — and there is
specific reason to think it may not. The Python lattice hash multiplies arbitrary-precision
signed integers and masks only at the end, so for negative lattice coordinates its
intermediate values are not the same as wrapping `u64` arithmetic. Slice 1 must decide
whether to reproduce Python's exact semantics or to accept that the port is a new generator
version. Under VERSION-001 that is a legitimate choice, but it is a choice, and it must be
made deliberately rather than discovered.
```

- [ ] **Step 3: Record the verdict in the spec**

In `docs/design/2026-09-02-mark-2-world-studio.md`, section 4.1, replace the closing
sentence ("If it does not hold, the architecture is wrong, and we need to know in a day
rather than a month.") with the measured result, keeping the sentence and adding the
outcome beneath it, in the form:

```
**Measured, 2026-09-XX.** 5,000,000 samples across every operation class the generator
uses: [identical / divergent]. [One sentence of consequence.]
```

- [ ] **Step 4: Commit**

```bash
git add spikes/0-bit-equality/README.md docs/design/2026-09-02-mark-2-world-studio.md
git commit -m "spike: the answer, recorded where the claim was made"
```

---

## If the answer is DIVERGENT

Do not patch the probe until it passes. The plan's value is the honest answer, and a
divergence is a finding rather than a failure. The next steps in that case are, in order:

1. Identify which operation class diverges, by evaluating each stage of `probe::evaluate`
   separately over the corpus and comparing per-stage.
2. If it is a `libm` call, check whether the WASM build is using a different `libm` code
   path (`libm` has target-specific branches); pin it or force the generic implementation.
3. If it is the order-sensitive accumulation, the cause is codegen reassociation, and the
   fix is a compiler-flag question rather than a maths one.
4. If nothing resolves it, section 4.1's precondition has failed, and the Mark 2
   architecture needs revisiting before the studio is built on it — which is exactly what
   this slice was for, and why it costs days rather than months.
