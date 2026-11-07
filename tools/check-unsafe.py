#!/usr/bin/env python3
"""Memory-safety policy check (SEC-11). Runs in gate stage 14.

Rules:
1. Rust crate roots (src/lib.rs, src/main.rs, src/bin/*.rs, build.rs, fuzz targets) start with
   `#![forbid(unsafe_code)]`. A crate that owns allowlisted files uses `#![deny(unsafe_code)]`
   instead, so only those files can opt out.
2. `unsafe` and `allow(unsafe_code)` appear only in files listed in tools/unsafe-allowlist.txt.
3. Every `unsafe` use in an allowlisted file has a `// SAFETY:` comment on the same line or in
   the 3 lines above it.
4. Every allowlisted file exists and declares `#![allow(unsafe_code)]`.
5. C# has no `unsafe` code, and no project turns on AllowUnsafeBlocks.

Exit code 1 on any violation, printed as GitHub annotations.
"""
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
ALLOWLIST = ROOT / "tools/unsafe-allowlist.txt"
UNSAFE = re.compile(r"\bunsafe\b")
STRING = re.compile(r'b?"(?:\\.|[^"\\])*"')
CHAR = re.compile(r"'(?:\\.|[^'\\])'")


def annotate(path, line, message):
    rel = path.relative_to(ROOT).as_posix()
    print(f"::error file={rel},line={line}::{message}")


def code_only(line):
    """The line without string literals and comments (good enough for policy checks)."""
    line = STRING.sub('""', line)
    line = CHAR.sub("''", line)
    return line.split("//", 1)[0]


def rust_files():
    for base in ("crates", "fuzz"):
        for path in sorted((ROOT / base).rglob("*.rs")):
            if "target" in path.relative_to(ROOT).parts:
                continue
            yield path


def is_crate_root(path):
    rel = path.relative_to(ROOT).as_posix()
    return (rel.endswith("/src/lib.rs") or rel.endswith("/src/main.rs") or "/src/bin/" in rel
            or rel.endswith("/build.rs") or rel.startswith("fuzz/fuzz_targets/"))


def crate_dir(path):
    for parent in path.parents:
        if (parent / "Cargo.toml").is_file():
            return parent
    return ROOT


def main():
    errors = 0
    allowed = set()
    for raw in ALLOWLIST.read_text(encoding="utf-8").splitlines():
        entry = raw.split("#", 1)[0].strip()
        if entry:
            allowed.add(entry)

    crates_with_allowlisted = set()
    for entry in sorted(allowed):
        path = ROOT / entry
        if not path.is_file():
            annotate(ALLOWLIST, 1, f"allowlisted file doesn't exist: {entry}")
            errors += 1
            continue
        crates_with_allowlisted.add(crate_dir(path))
        if "#![allow(unsafe_code)]" not in path.read_text(encoding="utf-8"):
            annotate(path, 1, "allowlisted file must declare #![allow(unsafe_code)]")
            errors += 1

    for path in rust_files():
        rel = path.relative_to(ROOT).as_posix()
        lines = path.read_text(encoding="utf-8").splitlines()
        text = "\n".join(lines)
        if is_crate_root(path):
            wants_deny = crate_dir(path) in crates_with_allowlisted and rel.endswith("/src/lib.rs")
            attribute = "#![deny(unsafe_code)]" if wants_deny else "#![forbid(unsafe_code)]"
            if attribute not in text:
                annotate(path, 1, f"crate root must declare {attribute} (SEC-11)")
                errors += 1
        listed = rel in allowed
        for number, line in enumerate(lines, start=1):
            code = code_only(line)
            if not listed and "allow(unsafe_code)" in code:
                annotate(path, number, "allow(unsafe_code) outside tools/unsafe-allowlist.txt (SEC-11)")
                errors += 1
            if not UNSAFE.search(code):
                continue
            if not listed:
                annotate(path, number, "unsafe code outside tools/unsafe-allowlist.txt (SEC-11)")
                errors += 1
                continue
            window = lines[max(0, number - 4):number]
            if not any("// SAFETY:" in candidate for candidate in window):
                annotate(path, number, "unsafe without a // SAFETY: comment just above it (SEC-11)")
                errors += 1

    for path in sorted((ROOT / "app").rglob("*")):
        parts = path.relative_to(ROOT).parts
        if not path.is_file() or "bin" in parts or "obj" in parts:
            continue
        if path.suffix == ".cs":
            for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
                if UNSAFE.search(code_only(line)):
                    annotate(path, number, "C# unsafe code is not allowed (SEC-11)")
                    errors += 1
        elif path.suffix in {".csproj", ".props", ".targets"}:
            if re.search(r"<AllowUnsafeBlocks>\s*true", path.read_text(encoding="utf-8"), re.I):
                annotate(path, 1, "AllowUnsafeBlocks must stay false (SEC-11)")
                errors += 1
    props = ROOT / "Directory.Build.props"
    if "<AllowUnsafeBlocks>false</AllowUnsafeBlocks>" not in props.read_text(encoding="utf-8"):
        annotate(props, 1, "Directory.Build.props must set AllowUnsafeBlocks to false (SEC-11)")
        errors += 1

    if errors:
        print(f"check-unsafe: {errors} violation(s)")
        return 1
    print(f"check-unsafe: OK ({len(allowed)} allowlisted file(s))")
    return 0


if __name__ == "__main__":
    sys.exit(main())
