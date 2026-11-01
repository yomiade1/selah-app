#!/usr/bin/env python3
"""Check GitHub Actions workflows against the CI policy (ADR-024, SEC-10).

Run from the repository root: python3 tools/check-workflows.py
Exit code 0 means every rule passes. In GitHub Actions, problems are also printed as annotations.
"""
import os
import re
import sys
from pathlib import Path

WORKFLOWS = Path(".github/workflows")
ALLOWED_OWNERS = ("actions/", "github/")  # GitHub-owned actions only
PINNED = re.compile(r"^[\w.-]+/[\w.-]+(/[\w./-]+)?@[0-9a-f]{40}$")
BANNED_TRIGGERS = re.compile(r"^\s*(pull_request_target|workflow_run|schedule)\s*:", re.M)
UNTRUSTED = re.compile(
    r"\$\{\{\s*github\.(head_ref|event\.(pull_request\.(title|body|head\.ref|head\.label)"
    r"|issue\.(title|body)|comment\.body|review\.body|review_comment\.body|label\.name"
    r"|commits|head_commit\.message|inputs))"
)
SECRET_REF = re.compile(r"\$\{\{[^}]*?(?<![\w.])secrets\.([A-Za-z_][A-Za-z0-9_]*)")
ENV_ASSIGNMENT = re.compile(r"^\s*[A-Z][A-Z0-9_]*:\s*\$\{\{")
CONDITION = re.compile(r"^\s*(if|group):")

problems = []


def report(path, line, message):
    problems.append((str(path), line, message))


def jobs_section(lines):
    """Yield (job_name, start, end) for each job under the top-level `jobs:` key."""
    try:
        start = next(i for i, l in enumerate(lines) if l.rstrip() == "jobs:")
    except StopIteration:
        return
    end_of_jobs = next(
        (i for i in range(start + 1, len(lines)) if lines[i] and not lines[i].startswith((" ", "#"))),
        len(lines),
    )
    job_starts = []
    for i in range(start + 1, end_of_jobs):
        m = re.match(r"^  ([\w-]+):\s*$", lines[i])
        if m:
            job_starts.append((m.group(1), i))
    for n, (name, s) in enumerate(job_starts):
        e = job_starts[n + 1][1] if n + 1 < len(job_starts) else end_of_jobs
        yield name, s, e


def check(path):
    text = path.read_text(encoding="utf-8")
    lines = text.splitlines()

    if not re.search(r"^permissions:", text, re.M):
        report(path, 1, "add a top-level 'permissions: {}' and grant permissions per job")
    for m in BANNED_TRIGGERS.finditer(text):
        report(path, text[: m.start()].count("\n") + 1, f"trigger '{m.group(1)}' isn't allowed")

    block_indent = None  # inside a multi-line `if: >-` expression
    for i, l in enumerate(lines, 1):
        stripped = l.split(" #", 1)[0]
        indent = len(l) - len(l.lstrip())
        if block_indent is not None and (not l.strip() or indent > block_indent):
            continue
        block_indent = None
        if re.match(r"^\s*if:\s*[>|]-?\s*$", stripped):
            block_indent = indent
            continue
        m = re.search(r"\buses:\s*([^\s]+)", stripped)
        if m:
            ref = m.group(1).strip("'\"")
            if ref.startswith("./"):
                pass
            elif not ref.startswith(ALLOWED_OWNERS):
                report(path, i, f"'{ref}': only GitHub-owned actions are allowed")
            elif not PINNED.match(ref):
                report(path, i, f"'{ref}': pin the action to a full 40-character commit SHA")
            if ref.startswith("actions/checkout@"):
                block = "\n".join(lines[i : i + 6])
                if "persist-credentials: false" not in block:
                    report(path, i, "actions/checkout needs 'persist-credentials: false'")
        for s in SECRET_REF.findall(stripped):
            if s != "GITHUB_TOKEN":
                report(path, i, f"secret '{s}': no secrets while the repository is public")
        if UNTRUSTED.search(stripped) and not (ENV_ASSIGNMENT.match(stripped) or CONDITION.match(stripped)):
            report(path, i, "untrusted event text must reach scripts through 'env:', not '${{ }}' in place")
        if re.search(r"runs-on:.*self-hosted", stripped):
            report(path, i, "self-hosted runners aren't allowed")

    for name, s, e in jobs_section(lines):
        body = "\n".join(lines[s:e])
        m = re.search(r"timeout-minutes:\s*(\d+)", body)
        if not m:
            report(path, s + 1, f"job '{name}' needs 'timeout-minutes'")
        elif int(m.group(1)) > 60:
            report(path, s + 1, f"job '{name}': timeout over 60 minutes (no long-running CI)")


def main():
    if not WORKFLOWS.is_dir():
        print("check-workflows: no workflows found")
        return 0
    files = sorted(list(WORKFLOWS.glob("*.yml")) + list(WORKFLOWS.glob("*.yaml")))
    for f in files:
        check(f)
    gha = os.environ.get("GITHUB_ACTIONS") == "true"
    for path, line, message in problems:
        if gha:
            print(f"::error file={path},line={line}::{message}")
        print(f"{path}:{line}: {message}", file=sys.stderr)
    if problems:
        print(f"check-workflows: {len(problems)} problem(s) in {len(files)} workflow(s)", file=sys.stderr)
        return 1
    print(f"check-workflows: {len(files)} workflow(s) follow the CI policy")
    return 0


if __name__ == "__main__":
    sys.exit(main())
