#!/usr/bin/env python3
"""Assert that the suites CI runs are the size they are supposed to be.

WHY THIS EXISTS. Every other gate in this repository is asserted by exit status, which is
the right way to ask "did anything fail". It is not a way to ask "did anything RUN". A
suite whose tests are deleted, whose `#[cfg(...)]` stops matching, or whose corpus quietly
shrinks exits 0 and reports green -- which is the exact shape of the three failures that
motivated the CI slice: a conformance suite that skipped silently and compared nothing, and
three separate checks that counted zero work as success.

WHY IT DOES NOT GREP `test result:`. Grepping the human-readable summary line is banned
here, for the same reason: a pattern that stops matching matches nothing, and "no match"
reads as "no failures". Every parse below is therefore CROSS-CHECKED against a second,
independently-formatted statement of the same number, and DISAGREEMENT OR ABSENCE IS A HARD
FAILURE. If libtest or the parity harness changes its output, this script goes red saying
the format moved. It cannot go green by matching nothing.

  cargo-list:  `cargo test -- --list` prints one `<name>: test` line per test AND a
               `<N> tests, <M> benchmarks` trailer per test binary. The count of lines must
               equal the sum of the trailers. `--list --ignored` gives the ignored subset
               the same way, and passed = total - ignored (sound only because the run's own
               exit status already established that nothing failed).

  parity:      parity.mjs prints one total line and one per-group line per group. The
               per-group tallies must sum to the total.
"""

import argparse
import re
import sys

TEST_LINE = re.compile(r"^\S.*: test$")
TRAILER = re.compile(r"^(\d+) tests, (\d+) benchmarks$")
PARITY_TOTAL = re.compile(
    r"^(parity|CONTROL \(--mutate seed\)): (\d+) values compared through the shipped "
    r"exports, (\d+) divergent$"
)
PARITY_GROUP = re.compile(r"^\s+(\S+): (\d+) compared, (\d+) divergent$")


def die(*lines):
    print("COUNT GATE FAILED", file=sys.stderr)
    for line in lines:
        print(f"  {line}", file=sys.stderr)
    sys.exit(1)


def read(path):
    with open(path, "r", encoding="utf-8", errors="replace") as fh:
        return fh.read().splitlines()


def listed(path, what):
    """Count tests in one `cargo test -- --list` capture, two independent ways."""
    lines = read(path)
    by_line = sum(1 for ln in lines if TEST_LINE.match(ln))
    trailers = [TRAILER.match(ln) for ln in lines]
    trailers = [m for m in trailers if m]
    if not trailers:
        die(
            f"{what}: no `<N> tests, <M> benchmarks` trailer in {path}.",
            "cargo test -- --list prints one per test binary. Zero of them means the",
            "output format changed, the command did not run, or nothing was built --",
            "and none of those may be read as 'the tests are all still there'.",
        )
    by_trailer = sum(int(m.group(1)) for m in trailers)
    if by_line != by_trailer:
        die(
            f"{what}: the two counts in {path} disagree.",
            f"  `<name>: test` lines:        {by_line}",
            f"  sum of the trailers:         {by_trailer} (over {len(trailers)} binaries)",
            "They describe the same set. Disagreement means the --list format moved;",
            "fix this script deliberately rather than letting one of the two win.",
        )
    return by_line, len(trailers)


def cmd_cargo_list(args):
    total, binaries = listed(args.all, "total")
    ignored, _ = listed(args.ignored, "ignored")
    if total == 0:
        die("total: zero tests listed. A suite of no tests passes trivially.")
    passed = total - ignored
    print(f"listed: {total} tests over {binaries} binaries, {ignored} ignored "
          f"-> {passed} run")
    problems = []
    if passed != args.expect_passed:
        problems.append(f"expected {args.expect_passed} tests to run, found {passed}")
    if ignored != args.expect_ignored:
        problems.append(f"expected {args.expect_ignored} ignored, found {ignored}")
    if problems:
        die(*problems,
            "A test that disappears does not fail -- it stops existing, and the run stays",
            "green. If this change is intended, update the expected count in the workflow",
            "in the same commit that changes the suite.")
    print(f"count OK: {passed} passed / 0 failed / {ignored} ignored, as recorded")


def cmd_parity(args):
    lines = read(args.output)
    totals = [m for m in (PARITY_TOTAL.match(ln) for ln in lines) if m]
    if len(totals) != 1:
        die(
            f"expected exactly one parity total line in {args.output}, found {len(totals)}.",
            "parity.mjs prints `<label>: <N> values compared through the shipped exports,",
            "<D> divergent`. Not finding it means the harness did not get that far or its",
            "output format changed -- neither is a parity result.",
        )
    label, compared, divergent = totals[0].group(1), int(totals[0].group(2)), int(totals[0].group(3))
    groups = [m for m in (PARITY_GROUP.match(ln) for ln in lines) if m]
    if not groups:
        die(f"no per-group tallies in {args.output}; the total has nothing to check it.")
    g_compared = sum(int(m.group(2)) for m in groups)
    g_divergent = sum(int(m.group(3)) for m in groups)
    if (g_compared, g_divergent) != (compared, divergent):
        die(
            "the per-group tallies do not sum to the total.",
            f"  total:  {compared} compared, {divergent} divergent",
            f"  groups: {g_compared} compared, {g_divergent} divergent (over {len(groups)} groups)",
        )
    print(f"parity line: {label}: {compared} compared, {divergent} divergent "
          f"({len(groups)} groups, tallies agree)")
    problems = []
    if label != args.expect_label:
        problems.append(f"expected the `{args.expect_label}` line, found `{label}`")
    if compared != args.expect_compared:
        problems.append(f"expected {args.expect_compared} values compared, found {compared}")
    if divergent != args.expect_divergent:
        problems.append(f"expected {args.expect_divergent} divergent, found {divergent}")
    if problems:
        die(*problems,
            "The corpus size is part of the claim. A parity run over a smaller corpus is a",
            "weaker statement wearing the same words, and 'zero divergent' over nothing is",
            "the failure this project has already shipped once.")
    print("count OK: the corpus is the size the record says it is")


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    sub = ap.add_subparsers(dest="cmd", required=True)

    a = sub.add_parser("cargo-list")
    a.add_argument("--all", required=True)
    a.add_argument("--ignored", required=True)
    a.add_argument("--expect-passed", type=int, required=True)
    a.add_argument("--expect-ignored", type=int, required=True)
    a.set_defaults(fn=cmd_cargo_list)

    p = sub.add_parser("parity")
    p.add_argument("--output", required=True)
    p.add_argument("--expect-label", required=True)
    p.add_argument("--expect-compared", type=int, required=True)
    p.add_argument("--expect-divergent", type=int, required=True)
    p.set_defaults(fn=cmd_parity)

    args = ap.parse_args()
    args.fn(args)


if __name__ == "__main__":
    main()
