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
    native       1.2 s
    wasm         7.2 s

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
