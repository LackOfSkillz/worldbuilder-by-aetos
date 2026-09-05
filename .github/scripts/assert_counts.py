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

  pytest:      `pytest --collect-only -q` prints one `<file>::<nodeid>` line per test AND a
               `<N> tests collected in <T>s` trailer; the count of lines must equal the
               trailer, and the trailer must be that exact clean form -- a collection that
               also errored, deselected, or found nothing prints something else and is a
               hard failure rather than a number. The REAL run's terminal summary
               (`<N> passed in <T>s`) is a second, independently-produced statement of the
               same number and must agree. That pair is the pytest analogue of the two
               cargo `--list` formats, and it is why nothing here greps for `passed` in a
               way that could read "no match" as "nothing wrong".

               THE CASE THIS EXISTS FOR: with WORLDBUILDER_REQUIRE_ENGINE unset and no
               engine built, tests/test_conformance.py skips AT IMPORT. All 157 of its
               tests collapse into a single `1 skipped` (150 conformance comparisons plus
               7 guard unit tests -- not all 157 are comparisons; see the identity slice's
               Task 5 report), pytest exits 0, and CI reports `241 passed, 1 skipped` --
               green, having compared nothing. Asserting the per-file count is what
               notices.
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

# One collected test: `tests/test_features.py::TestX::test_y`. Parametrised ids may contain
# spaces and colons, so only the leading path is pinned.
COLLECT_LINE = re.compile(r"^(\S+\.py)::(.+)$")
# The CLEAN collection trailer, and only the clean one. `240 tests collected, 1 error in
# 0.33s` deliberately does not match: a collection that errored is not a count.
COLLECT_TRAILER = re.compile(r"^(\d+) tests? collected in [\d.]+s$")
# `====== 390 passed in 85.31s (0:01:25) ======` -- pytest's terminal summary.
SUMMARY_LINE = re.compile(r"^=+\s+(.*?)\s+in \d+[\d.]*s(?: \(\d+:\d+:\d+\))?\s*=+$")
SUMMARY_PAIR = re.compile(
    r"(\d+) (passed|failed|errors?|skipped|xfailed|xpassed|deselected|warnings?|reruns?)"
)
# Counts that mean "a test ran and ended somehow". `deselected` and `warnings` are not
# outcomes and are excluded on purpose.
OUTCOMES = ("passed", "failed", "error", "errors", "skipped", "xfailed", "xpassed")


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


def cmd_pytest(args):
    """Assert the Python suite is the size it is supposed to be, per file and in total."""
    collect = read(args.collect)
    node_lines = [m for m in (COLLECT_LINE.match(ln) for ln in collect) if m]
    trailers = [m for m in (COLLECT_TRAILER.match(ln) for ln in collect) if m]
    if len(trailers) != 1:
        die(
            f"expected exactly one `<N> tests collected in <T>s` trailer in {args.collect},"
            f" found {len(trailers)}.",
            "That is the line `pytest --collect-only -q` ends with when collection is",
            "clean. Not finding it means collection errored, collected nothing, deselected",
            "something, or the output format moved -- and none of those may be read as",
            "'the tests are all still there'.",
        )
    by_trailer = int(trailers[0].group(1))
    by_line = len(node_lines)
    if by_line != by_trailer:
        die(
            f"the two counts in {args.collect} disagree.",
            f"  `<file>::<nodeid>` lines: {by_line}",
            f"  the collected trailer:    {by_trailer}",
            "They describe the same set; disagreement means the --collect-only format",
            "moved. Fix this script deliberately rather than letting one of the two win.",
        )
    if by_line == 0:
        die("zero tests collected. A suite of no tests passes trivially.")

    per_file = {}
    for m in node_lines:
        per_file[m.group(1)] = per_file.get(m.group(1), 0) + 1

    run = read(args.run)
    summaries = [m for m in (SUMMARY_LINE.match(ln) for ln in run) if m]
    if len(summaries) != 1:
        die(
            f"expected exactly one pytest terminal summary line in {args.run}, found"
            f" {len(summaries)}.",
            "That line is the run's OWN statement of how many tests it executed, and it is",
            "the independent cross-check on the collected count. Its absence means the run",
            "did not finish or the format moved -- never that nothing was wrong.",
        )
    pairs = SUMMARY_PAIR.findall(summaries[0].group(1))
    if not pairs:
        die(
            f"the summary line in {args.run} carries no `<N> <outcome>` counts:",
            f"  {summaries[0].group(0)}",
        )
    tally = {}
    for n, word in pairs:
        tally[word] = tally.get(word, 0) + int(n)
    ran = sum(v for k, v in tally.items() if k in OUTCOMES)
    passed = tally.get("passed", 0)
    print(f"collected: {by_line} tests over {len(per_file)} files "
          f"(line count and trailer agree)")
    print("summary:   " + ", ".join(f"{v} {k}" for k, v in sorted(tally.items())))

    problems = []
    not_passed = ", ".join(f"{v} {k}" for k, v in sorted(tally.items())
                           if k in OUTCOMES and k != "passed")
    if passed != ran:
        problems.append(
            f"the run reports outcomes that are not `passed`: {not_passed} -- a skipped"
            " test is a test that did not run, and this gate exists because 150"
            " comparisons once collapsed into one `1 skipped` and reported green"
        )
    if passed != by_line:
        problems.append(
            f"collected {by_line} tests but the run summarised {passed} passed;"
            " the two independent statements of the same number disagree"
        )
    if by_line != args.expect_total:
        problems.append(f"expected {args.expect_total} tests in total, found {by_line}")
    for spec in args.expect_file:
        path, _, want = spec.partition("=")
        got = per_file.get(path, 0)
        if got != int(want):
            problems.append(f"expected {want} tests in {path}, found {got}")
    if problems:
        die(*problems,
            "A test that disappears does not fail -- it stops existing, and the run stays",
            "green. If this change is intended, update the expected count in the workflow",
            "in the same commit that changes the suite.")
    print(f"count OK: {by_line} collected and {passed} passed, per file as recorded")


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

    t = sub.add_parser("pytest")
    t.add_argument("--collect", required=True,
                   help="output of `pytest --collect-only -q`")
    t.add_argument("--run", required=True,
                   help="output of the real `pytest` run, including its terminal summary")
    t.add_argument("--expect-total", type=int, required=True)
    t.add_argument("--expect-file", action="append", default=[], metavar="PATH=N",
                   help="assert one file's own count; repeatable")
    t.set_defaults(fn=cmd_pytest)

    args = ap.parse_args()
    args.fn(args)


if __name__ == "__main__":
    main()
