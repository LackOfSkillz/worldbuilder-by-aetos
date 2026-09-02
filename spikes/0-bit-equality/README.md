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
whether the Rust core reproduces the existing Python generator bit-for-bit, and slice 1 owns
that question. Two things about it were checked afterwards and are recorded here because one
of them corrects an earlier draft of this section.

*The lattice hash is not the problem.* An earlier draft warned that Python's hash multiplies
arbitrary-precision signed integers and masks only at the end, so negative lattice
coordinates might not match wrapping `u64` arithmetic. That is wrong, and measurably so:
200,000 random cases, 174,958 of them carrying at least one negative coordinate, produced
zero divergence between the Python hash and a faithful `u64`-wrapping emulation of it. The
reason is algebraic rather than lucky — multiplication and XOR both commute with truncation
mod 2^64, so masking once at the end is equivalent to masking at every step. The hash ports
exactly.

*Floor versus truncate is the problem.* `worldbuilder/terrain/noise.py` derives its lattice
cell as `int(x // 1)`, which floors toward negative infinity. Rust's `as i64` truncates
toward zero. For any negative coordinate — half the sphere — those pick a *different lattice
cell*: `-2.3` floors to `-3` and truncates to `-2`, and `-1e-9` floors to `-1` and truncates
to `0`. A port that writes the obvious `as i64` produces a subtly different world, with no
error raised and no test failing. Rust's `f64::floor` is the correct translation, routed
through `detmath` like everything else.

Neither point makes the port bit-exact by itself. They are the two traps found by looking;
slice 1 must still establish equality by measurement, using the existing 208 Python tests as
the conformance suite. But the choice that earlier draft posed — reproduce Python's semantics
or accept a new generator version — is not forced by the hash, and should not be taken as
settled in that direction.

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
