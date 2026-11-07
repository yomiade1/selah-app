#!/usr/bin/env python3
"""Check prerequisite versions without downloading or modifying the machine."""
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

def run(name, *args):
    program = shutil.which(name)
    if not program:
        raise RuntimeError(f"{name} is missing")
    result = subprocess.run([program, *args], capture_output=True, text=True,
                            timeout=30, check=False)
    if result.returncode:
        raise RuntimeError(f"{name} could not run (exit {result.returncode})")
    return result.stdout.strip()


def rust_ok():
    # Calling a rustup cargo shim can otherwise install a missing toolchain.
    # Listing installed toolchains first keeps --check read-only.
    installed = run("rustup", "toolchain", "list")
    if not any(line.startswith("1.91.0-") for line in installed.splitlines()):
        return False
    return run("cargo", "--version").startswith("cargo 1.91.0 ")


def check():
    checks = [
        ("Rust 1.91.0", rust_ok),
        (".NET SDK 10 LTS", lambda: any(re.match(r"10\.0\.\d+\s", line)
                                         for line in run("dotnet", "--list-sdks").splitlines())),
        ("Node 22 LTS", lambda: run("node", "--version").startswith("v22.")),
        ("pnpm 10.20.0", lambda: run("pnpm", "--version") == "10.20.0"),
        ("Python 3.11+", lambda: sys.version_info >= (3, 11)),
        ("LLVM / libclang", lambda: bool(run("clang", "--version"))),
    ]
    errors = []
    for label, probe in checks:
        try:
            if not probe():
                raise RuntimeError("version does not match the repository pin")
            print(f"PASS {label}")
        except (RuntimeError, OSError, subprocess.TimeoutExpired) as exc:
            errors.append(f"{label}: {exc}")
    if os.name == "nt":
        vswhere = Path(os.environ.get("ProgramFiles(x86)", "C:/Program Files (x86)")) / \
            "Microsoft Visual Studio/Installer/vswhere.exe"
        try:
            result = subprocess.run([str(vswhere), "-products", "*", "-requires",
                                     "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
                                     "-property", "installationPath"], capture_output=True,
                                    text=True, timeout=30, check=True)
            if not result.stdout.strip():
                raise RuntimeError("C++ workload not installed")
        except (OSError, RuntimeError, subprocess.SubprocessError) as exc:
            errors.append(f"MSVC C++ Build Tools: {exc}")
    for error in errors:
        print(f"::error file=tools/setup.ps1::{error}", file=sys.stderr)
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(check())
