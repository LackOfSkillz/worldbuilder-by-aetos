# Slice 0 — native/WASM bit-equality

**Throwaway.** Nothing in `worldbuilder/` or `crates/` may import this. It is not the
beginning of the engine; it is a question with a workbench attached.

## The question

Do a Rust field compiled natively and to `wasm32-unknown-unknown` return bit-identical
`f64` results? Section 4.1 of the Mark 2 spec makes the studio, the provider and the
one-source argument all conditional on the answer.

## The answer

    samples      5,000,000
    verdict      IDENTICAL
    native       1.4 s     rustc 1.98.0, x86_64 Windows
    wasm         7.2 s     wasm32-unknown-unknown, Node 22.17.0 (V8)

Figures are from a from-scratch build (`cargo clean` before rebuilding both targets), not
an incremental one.

## What was tested

Normalisation (`sqrt`), spherical coordinates (`asin`, `atan2`), trigonometry (`sin`,
`cos`), planar distance (`hypot`), saturating blend (`tanh`), fractional power (`powf`),
64-bit lattice hashing, and an order-sensitive 16-term accumulation. That is every
operation class the Python generator uses, established by inventory rather than assumed.

The comparison was proved capable of failing before its passing result was believed: a
negative control routing native `sin` through std while WASM used `libm` was detected.
That control produced 2,441 divergences out of 100,000 samples, differing by a single bit,
first at index 38 — proof the comparison notices a real, subtle difference rather than
passing by construction.

## What was NOT tested, and matters

**Python-to-Rust equality.** This spike compares Rust to Rust. It says nothing about
whether the Rust core reproduces the existing Python generator bit-for-bit — and there is
specific reason to think it may not. The Python lattice hash multiplies arbitrary-precision
signed integers and masks only at the end, so for negative lattice coordinates its
intermediate values are not the same as wrapping `u64` arithmetic. Slice 1 must decide
whether to reproduce Python's exact semantics or to accept that the port is a new generator
version. Under VERSION-001 that is a legitimate choice, but it is a choice, and it must be
made deliberately rather than discovered.

**`powf` sensitivity.** `powf` enters the probe attenuated: it contributes as `p * 1e-6`,
where `p` is on the order of 31-37, so its share of a result of order 1.6 is roughly 3.7e-5.
A 1-ULP divergence in `powf` perturbs the sum by roughly 2.5e-21, against a result ULP of
roughly 2.2e-16 — attenuated by about five orders of magnitude. Such a divergence would
change the rounded result only when a sample straddles a rounding boundary, which is
plausibly on the order of 50 samples in 5,000,000, and could easily show as zero. The
negative control was run on `sin`, which is fully amplified in the accumulation, so it
demonstrates the comparison can catch a 1-bit difference in general — it does not
demonstrate sensitivity for the `powf` term specifically. By contrast, `hypot` was checked
against the same analysis and survives it: its 1-ULP effect propagates through `tanh` at
about 16% of a result ULP, which is detectable. `powf` is the least load-bearing operation
class in this measurement, which narrows the claim rather than voiding it — the Python
generator uses `powf` exactly once, in `worldbuilder/terrain/continentality.py` (`** 0.75`).

## Reproducing it

Run from inside `spikes/0-bit-equality/` — `main.rs` writes to a relative `results/`
directory, so the crate directory is a precondition, not a convenience:

    cargo clean
    cargo build --release --bin native_probe
    cargo build --release --target wasm32-unknown-unknown
    target/release/native_probe.exe 5000000
    node host/run_wasm.mjs 5000000
    python compare.py

`cargo test --release` fails because `[profile.release]` sets `panic = "abort"` and the
test harness needs unwinding; run tests with plain `cargo test` instead.
