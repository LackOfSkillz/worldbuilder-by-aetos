#!/usr/bin/env python3
"""Assert that the two implementations of the source fingerprint agree.

WHY THIS EXISTS. `viewer/scripts/build-wasm.mjs` computes a sha256 over the engine's
fingerprinted inputs to detect a stale `.wasm` (Task 0/1 of this project). `maturin
develop` does not run node, so the Python extension cannot ask that script anything at
build time -- Task 2 gave it the SAME digest by writing a SECOND implementation of the
algorithm, in Rust, baked in at compile time via `build.rs` and exposed as the PyO3
exports `source_fingerprint()` and `source_fingerprint_inputs()`
(`crates/worldbuilder-engine/src/bindings.rs`). That duplication was allowed on one
condition: "duplication that is checked is not duplication that rots." This script is the
check. Until it exists, nothing asserts the two implementations produce the same answer
over the same tree.

WHY IT DOES NOT JUST COMPARE TWO STRINGS FOR EQUALITY. A comparison that reads two empty
strings, two tracebacks-as-text, or two truncated values and finds them "equal" is the
exact defect class this project keeps finding elsewhere: a parser reading a missing
section as "unknown", three checks counting zero work as success, a check comparing a
list against its own source (see `.github/scripts/assert_counts.py`'s docstring for the
pattern this one follows). So every value on both sides is validated on its own terms --
a 64-character lowercase hex sha256 for the digest, a positive decimal integer for the
input count -- BEFORE either side is compared to the other, and the extension failing to
import is treated as the entire Rust side being absent, not as an unrelated crash.

Two independent statements per side (digest AND input count) rather than one, for the
same reason `assert_counts.py` cross-checks two independently-formatted statements of one
number: agreement on a single figure could be coincidence or a shared blind spot, and
that is precisely the shape of bug this project has shipped before.
"""

import argparse
import re
import sys

HEX64 = re.compile(r"^[0-9a-f]{64}$")
FIELD = re.compile(r"^(source-fingerprint|fingerprint-inputs): (.+)$")


def die(*lines):
    print("FINGERPRINT PARITY GATE FAILED", file=sys.stderr)
    for line in lines:
        print(f"  {line}", file=sys.stderr)
    sys.exit(1)


def read_fields(path, label):
    """Parse `source-fingerprint: <..>` / `fingerprint-inputs: <..>` lines from a captured
    output file. Missing lines are a hard failure, not an empty value to compare with."""
    try:
        with open(path, "r", encoding="utf-8", errors="replace") as fh:
            lines = fh.read().splitlines()
    except OSError as e:
        die(f"{label}: could not read {path}: {e}",
            "There is nothing to compare against -- the command that should have produced",
            "this file did not run, or wrote somewhere else.")
        return {}  # unreachable; die() exits, but keeps type-checkers happy

    fields = {}
    for ln in lines:
        m = FIELD.match(ln)
        if m:
            fields[m.group(1)] = m.group(2).strip()

    if "source-fingerprint" not in fields:
        die(
            f"{label}: no `source-fingerprint: <hex>` line in {path}.",
            "That line is this side's entire statement of its digest. Its absence means",
            "the command that should have printed it did not run, crashed before printing",
            "it, or the output format moved -- none of those may be read as 'nothing to",
            "compare', which is the exact failure this gate exists to catch.",
        )
    if "fingerprint-inputs" not in fields:
        die(
            f"{label}: no `fingerprint-inputs: <N>` line in {path}.",
            "The input count is the second, independent statement this gate cross-checks",
            "against the digest; its absence is exactly as much a failure as the digest's.",
        )
    return fields


def validate_digest(value, label):
    if not value:
        die(f"{label}: source-fingerprint is empty.",
            "An empty string is not a digest, and two empty strings are not 'agreement'.")
    if not HEX64.match(value):
        die(
            f"{label}: source-fingerprint {value!r} is not a 64-character lowercase hex",
            "sha256 digest. Comparing this to the other side would be comparing shapes",
            "that merely happen to be equal, not two implementations of one algorithm.",
        )
    return value


def validate_count(value, label):
    if not value or not re.match(r"^\d+$", value):
        die(f"{label}: fingerprint-inputs {value!r} is not a plain non-negative integer.")
    n = int(value)
    if n == 0:
        die(f"{label}: fingerprint-inputs is 0. A fingerprint over no inputs proves nothing"
            " -- this is the silent-zero failure this project keeps finding elsewhere.")
    return n


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                  formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--node-output", required=True,
                     help="captured stdout of `node scripts/build-wasm.mjs digest`, run "
                          "from viewer/")
    args = ap.parse_args()

    node_fields = read_fields(args.node_output, "node")
    node_digest = validate_digest(node_fields["source-fingerprint"], "node")
    node_count = validate_count(node_fields["fingerprint-inputs"], "node")

    try:
        import worldbuilder_engine as engine
    except ImportError as e:
        die(
            f"the compiled extension is not importable: {e}",
            "This is not a missing field inside a comparison -- it is the entire Rust side",
            "of the check absent, and it must fail exactly as loudly as a wrong digest",
            "would. Build it first with `maturin develop --release --features python`.",
        )
        return  # unreachable

    try:
        rust_digest_raw = engine.source_fingerprint()
    except Exception as e:
        die(f"worldbuilder_engine.source_fingerprint() raised: {e!r}")
        return  # unreachable
    try:
        rust_count_raw = engine.source_fingerprint_inputs()
    except Exception as e:
        die(f"worldbuilder_engine.source_fingerprint_inputs() raised: {e!r}")
        return  # unreachable

    rust_digest = validate_digest(str(rust_digest_raw), "rust")
    rust_count = validate_count(str(rust_count_raw), "rust")

    print(f"node: source-fingerprint {node_digest} ({node_count} inputs)")
    print(f"rust: source-fingerprint {rust_digest} ({rust_count} inputs)")

    problems = []
    if node_digest != rust_digest:
        problems.append(
            "the two digests disagree:\n"
            f"      node (build-wasm.mjs):      {node_digest}\n"
            f"      rust (worldbuilder_engine): {rust_digest}"
        )
    if node_count != rust_count:
        problems.append(
            "the two input counts disagree:\n"
            f"      node (build-wasm.mjs):      {node_count}\n"
            f"      rust (worldbuilder_engine): {rust_count}"
        )
    if problems:
        die(*problems,
            "Task 2 built a second implementation of this digest in Rust because",
            "`maturin develop` does not run node. The ruling that allowed that duplication",
            "was 'duplication that is checked is not duplication that rots' -- this is the",
            "check, and it just found the two implementations have drifted apart.")

    print(f"count OK: node and rust fingerprints agree on a real digest over a real "
          f"corpus ({node_count} inputs)")


if __name__ == "__main__":
    main()
