#!/usr/bin/env python3
"""Generated files must match their sources (gate stage 4).

Regenerates the TypeScript contracts (buf generate) and, once it exists, the design-token
outputs, then fails if git sees any difference.
"""
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
WINDOWS = os.name == "nt"
GENERATED = ["web/contracts/src/gen"]


def pnpm(*args):
    if WINDOWS:
        return [shutil.which("corepack.cmd") or "corepack", "pnpm", *args]
    return [shutil.which("pnpm") or "pnpm", *args]


def main():
    subprocess.run(pnpm("exec", "buf", "generate"), cwd=ROOT, check=True)
    if (ROOT / "app/Selah.UI").is_dir():
        subprocess.run(pnpm("run", "tokens"), cwd=ROOT, check=True)
        GENERATED.extend(["app/Selah.UI/Styles/Tokens.g.axaml", "app/Selah.UI/Styles/Tokens.g.cs",
                          "web/tokens/tokens.css"])
    existing = [p for p in GENERATED if (ROOT / p).exists()]
    diff = subprocess.run(["git", "status", "--porcelain", "--", *existing], cwd=ROOT,
                          capture_output=True, text=True, check=True).stdout
    # Only changes in the working tree count (column 2, or untracked): a regenerated file that's
    # already staged is current.
    diff = "\n".join(line for line in diff.splitlines() if len(line) > 3 and (line[1] != " " or line[:2] == "??"))
    if diff:
        for line in diff.splitlines():
            path = line[3:]
            print(f"::error file={path}::generated file is out of date; run tools/codegen.ps1 "
                  "(or pnpm exec buf generate / pnpm tokens) and commit the result")
        return 1
    print("check-generated: generated files are current")
    return 0


if __name__ == "__main__":
    sys.exit(main())
