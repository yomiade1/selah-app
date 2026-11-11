#!/usr/bin/env python3
"""Check the cloud Windows type-check still covers the crates with Windows-only code.

Gate stage 5 runs `cargo check --target x86_64-pc-windows-gnu` over the workspace minus the
crates listed in tools/windows-check-exclusions.txt. That list is necessary (Skia and FFmpeg
have no cloud-buildable Windows target), but it is also how Windows-only code can stop being
checked without anyone noticing.

This script fails if a crate holding Windows-only code is excluded without the exclusions file
saying so, in a line that names the crate and the words "HAS Windows-only code". It is the
regression guard for M1-A1.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EXCLUSIONS = ROOT / "tools" / "windows-check-exclusions.txt"
CRATES = ROOT / "crates"

# Only *conditional compilation* matters here: code behind a #[cfg(windows)] attribute is
# never type-checked unless the target is Windows. `cfg!(windows)` is a runtime boolean whose
# branches both compile everywhere, so it is deliberately NOT matched.
WINDOWS_ATTR = re.compile(
    r"#!?\[\s*cfg(?:_attr)?\s*\([^\]]*"
    r'(?:\bwindows\b|target_os\s*=\s*"windows"|target_family\s*=\s*"windows")'
    r"[^\]]*\]",
    re.DOTALL,
)
# Cargo.toml: [target.'cfg(windows)'.dependencies] or [target.x86_64-pc-windows-msvc.…]
WINDOWS_MANIFEST = re.compile(r"^\s*\[target\.[^\]]*windows[^\]]*\]", re.MULTILINE)

# `#![cfg_attr(all(windows, …), windows_subsystem = "windows")]` only hides the console window
# on a Windows release build. It gates no code, so a cross-target check of it proves nothing;
# the `windows` CI job compiles it for real. Ignored so the guard's warnings stay meaningful.
BENIGN_ATTR = re.compile(r"windows_subsystem")


def excluded_crates() -> list[str]:
    names = []
    for line in EXCLUSIONS.read_text(encoding="utf-8").splitlines():
        name = line.split("#", 1)[0].strip()
        if name:
            names.append(name)
    return names


def acknowledged_crates() -> set[str]:
    """Crates the exclusions file explicitly records as losing Windows-only coverage."""
    acknowledged = set()
    text = EXCLUSIONS.read_text(encoding="utf-8")
    for line in text.splitlines():
        if "HAS Windows-only code" not in line:
            continue
        # The marker sits in the indented note under the crate name, so look back for the
        # nearest crate name mentioned in the comment block.
        acknowledged.update(re.findall(r"selah-[a-z-]+", line))
    # A note can wrap onto the next line, so also pair each comment paragraph with its crate.
    paragraphs = re.split(r"\n(?=#   selah-)", text)
    for para in paragraphs:
        if "HAS Windows-only code" in para:
            match = re.search(r"#\s+(selah-[a-z-]+)", para)
            if match:
                acknowledged.add(match.group(1))
    return acknowledged


def has_windows_code(crate_dir: Path) -> list[Path]:
    hits = []
    for path in sorted(crate_dir.rglob("*.rs")):
        if "target" in path.parts:
            continue
        try:
            found = [
                m.group(0)
                for m in WINDOWS_ATTR.finditer(path.read_text(encoding="utf-8", errors="replace"))
                if not BENIGN_ATTR.search(m.group(0))
            ]
            if found:
                hits.append(path)
        except OSError:
            continue
    manifest = crate_dir / "Cargo.toml"
    if manifest.is_file():
        if WINDOWS_MANIFEST.search(manifest.read_text(encoding="utf-8", errors="replace")):
            hits.append(manifest)
    return hits


def main() -> int:
    if not EXCLUSIONS.is_file():
        print(f"check-windows-coverage: missing {EXCLUSIONS.relative_to(ROOT)}", file=sys.stderr)
        return 1

    excluded = excluded_crates()
    acknowledged = acknowledged_crates()

    unknown = [c for c in excluded if not (CRATES / c).is_dir()]
    problems = []
    for crate in unknown:
        problems.append(f"{crate}: listed in the exclusions file but there is no such crate")

    uncovered = []
    for crate in excluded:
        crate_dir = CRATES / crate
        if not crate_dir.is_dir():
            continue
        hits = has_windows_code(crate_dir)
        if hits and crate not in acknowledged:
            rel = ", ".join(str(h.relative_to(ROOT)) for h in hits[:3])
            problems.append(
                f"{crate}: has Windows-only code ({rel}) but is excluded from the Windows "
                f'type-check without a note saying "HAS Windows-only code" in '
                f"{EXCLUSIONS.relative_to(ROOT)}"
            )
        if hits:
            uncovered.append(crate)

    if problems:
        for problem in problems:
            print(f"::error file=tools/windows-check-exclusions.txt::{problem}")
            print(f"check-windows-coverage: {problem}", file=sys.stderr)
        return 1

    covered = sorted(
        c.name
        for c in CRATES.iterdir()
        if c.is_dir() and c.name not in excluded and has_windows_code(c)
    )
    print(
        "check-windows-coverage: OK — Windows-only code is type-checked in "
        f"{', '.join(covered) if covered else '(none)'}; "
        f"recorded as CI-only: {', '.join(sorted(uncovered)) if uncovered else '(none)'}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
