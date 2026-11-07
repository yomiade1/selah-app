#!/usr/bin/env python3
"""The test ledger (ADR-016, TESTING-AND-QUALITY §3 rule 2): test counts may only go up.

    test-ledger.py --check    fail if any suite has fewer passing tests than tests/ledger.json
    test-ledger.py --update   raise tests/ledger.json to the current counts (never lowers them)
    test-ledger.py --show     print the current counts

Counts come from the logs the gate writes to target/verify-report/counts/, so run the gate
(tools/verify.sh or tools/verify.ps1) first. Skipped or ignored tests don't count.
"""
import argparse
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
LEDGER = ROOT / "tests" / "ledger.json"
COUNTS = ROOT / "target" / "verify-report" / "counts"
SUITES = ["rust", "dotnet", "web", "golden", "ui", "e2e", "kb"]
# Rust integration test files that are end-to-end (real processes), not unit or integration.
E2E_FILES = {"continuity", "launch"}

RUNNING = re.compile(r"Running (?:unittests )?(?P<path>\S+)")
RESULT = re.compile(r"test result: \w+\. (?P<passed>\d+) passed")


def rust_counts(text):
    rust = e2e = 0
    current = ""
    for line in text.splitlines():
        running = RUNNING.search(line)
        if running:
            current = Path(running["path"].replace("\\", "/")).stem.split("-")[0]
            continue
        result = RESULT.search(line)
        if result:
            if current in E2E_FILES:
                e2e += int(result["passed"])
            else:
                rust += int(result["passed"])
    return rust, e2e


def current_counts():
    counts = dict.fromkeys(SUITES, 0)

    def read(name):
        path = COUNTS / name
        if not path.is_file():
            raise SystemExit(f"test-ledger: {path.relative_to(ROOT)} is missing; run the gate first")
        return path.read_text(encoding="utf-8", errors="replace")

    counts["rust"], counts["e2e"] = rust_counts(read("cargo-test.log"))
    counts["dotnet"] = sum(int(n) for n in re.findall(r"Passed:\s+(\d+)", read("dotnet-test.log")))
    counts["web"] = sum(int(n) for n in re.findall(r"^# pass (\d+)", read("web-test.log"), re.M))
    ui_log = COUNTS / "ui-tests.log"
    if ui_log.is_file():
        counts["ui"] = sum(int(n) for n in re.findall(r"Passed:\s+(\d+)", ui_log.read_text(encoding="utf-8", errors="replace")))
    golden_log = COUNTS / "golden-frames.log"
    if golden_log.is_file():
        counts["golden"] = sum(int(m) for m in RESULT.findall(golden_log.read_text(encoding="utf-8")))
    counts["total"] = sum(counts[s] for s in SUITES)
    return counts


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--check", action="store_true")
    group.add_argument("--update", action="store_true")
    group.add_argument("--show", action="store_true")
    args = parser.parse_args()
    counts = current_counts()
    ledger = json.loads(LEDGER.read_text(encoding="utf-8")) if LEDGER.is_file() else {}
    if args.show:
        print(json.dumps(counts, indent=2))
        return 0
    if args.update:
        merged = {suite: max(counts.get(suite, 0), ledger.get(suite, 0)) for suite in SUITES}
        merged["total"] = sum(merged[s] for s in SUITES)
        LEDGER.write_text(json.dumps(merged, indent=2) + "\n", encoding="utf-8", newline="\n")
        print(f"test-ledger: updated {LEDGER.relative_to(ROOT)}: {merged}")
        return 0
    errors = 0
    for suite in SUITES + ["total"]:
        minimum = ledger.get(suite, 0)
        if counts.get(suite, 0) < minimum:
            print(f"::error file=tests/ledger.json::{suite}: {counts.get(suite, 0)} passing tests, "
                  f"the ledger requires at least {minimum} (tests must never be removed or skipped)")
            errors += 1
    if errors:
        return 1
    print(f"test-ledger: OK {counts}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
