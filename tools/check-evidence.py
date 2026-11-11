#!/usr/bin/env python3
"""Check that every test named as evidence in the docs really exists.

PROGRESS.md and docs/SECURITY.md justify each work package, acceptance criterion and security
control by naming the tests that prove it. That only means something if the names are real.
Five stale names were found at the end of M3 — among them the two tests cited for SAFE-05, the
limiter that stops loud audio reaching a PA system — so this check stops the docs drifting
away from the suite.

It looks at every `backticked_snake_case_name` with four or more words and fails if no Rust
function by that name exists. Shorter names (`max_lines`, `flash_warning`) are field and column
names, not tests, so they are left alone.

Writing about a name that no longer exists (a renamed or removed test) therefore needs plain
quotes rather than backticks, so that only names offered as real evidence are checked.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DOCS = ("PROGRESS.md", "docs/SECURITY.md")
CODE_DIRS = ("crates", "fuzz", "app")

# Four or more underscore-separated lowercase words: the shape of a Rust test name.
CANDIDATE = re.compile(r"`([a-z][a-z0-9]*(?:_[a-z0-9]+){3,})`")
RUST_FN = re.compile(r"\bfn\s+([a-z_][a-z_0-9]*)")

# Names that look like tests but are something else the docs legitimately mention. Empty on
# purpose: add an entry only with a comment saying what the name really is.
ALLOWED: set[str] = set()


def known_function_names() -> set[str]:
    names: set[str] = set()
    for directory in CODE_DIRS:
        base = ROOT / directory
        if not base.is_dir():
            continue
        for path in base.rglob("*.rs"):
            if "target" in path.parts:
                continue
            try:
                names.update(RUST_FN.findall(path.read_text(encoding="utf-8", errors="replace")))
            except OSError:
                continue
    return names


def main() -> int:
    known = known_function_names()
    if len(known) < 100:
        print(
            "check-evidence: found almost no Rust functions; run from the repository root",
            file=sys.stderr,
        )
        return 1

    problems: list[tuple[str, int, str]] = []
    checked = 0
    for doc in DOCS:
        path = ROOT / doc
        if not path.is_file():
            print(f"check-evidence: missing {doc}", file=sys.stderr)
            return 1
        for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
            for match in CANDIDATE.finditer(line):
                name = match.group(1)
                checked += 1
                if name not in known and name not in ALLOWED:
                    problems.append((doc, number, name))

    if problems:
        for doc, number, name in problems:
            message = (
                f"{name} is named as evidence but no such test or function exists "
                f"(rename it to the real test, or remove the claim)"
            )
            print(f"::error file={doc},line={number}::{message}")
            print(f"check-evidence: {doc}:{number}: {message}", file=sys.stderr)
        return 1

    print(f"check-evidence: OK — {checked} test name(s) cited in {', '.join(DOCS)} all exist")
    return 0


if __name__ == "__main__":
    sys.exit(main())
