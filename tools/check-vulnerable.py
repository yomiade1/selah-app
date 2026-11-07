#!/usr/bin/env python3
"""Dependency vulnerability audits (SEC-09, gate stage 14).

    check-vulnerable.py dotnet   dotnet list package --vulnerable --include-transitive
    check-vulnerable.py pnpm     pnpm audit --prod

Fails on any known vulnerability. These need the advisory databases, so they run in the gate,
never in tests.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
WINDOWS = os.name == "nt"


def dotnet():
    result = subprocess.run([shutil.which("dotnet") or "dotnet", "list", "app/Selah.sln", "package",
                             "--vulnerable", "--include-transitive", "--format", "json"],
                            cwd=ROOT, capture_output=True, text=True, timeout=600)
    if result.returncode != 0:
        print(result.stdout + result.stderr)
        print("::error title=dotnet audit::dotnet list package --vulnerable failed")
        return 1
    report = json.loads(result.stdout)
    found = []
    for project in report.get("projects", []):
        for framework in project.get("frameworks", []):
            for kind in ("topLevelPackages", "transitivePackages"):
                for package in framework.get(kind, []):
                    for vulnerability in package.get("vulnerabilities", []):
                        found.append(f"{package['id']} {package.get('resolvedVersion', '')}: "
                                     f"{vulnerability.get('severity', '')} {vulnerability.get('advisoryurl', '')}")
    for item in found:
        print(f"::error title=vulnerable NuGet package::{item}")
    print(f"dotnet audit: {len(found)} vulnerable package(s)")
    return 1 if found else 0


def pnpm():
    command = ([shutil.which("corepack.cmd") or "corepack", "pnpm"] if WINDOWS
               else [shutil.which("pnpm") or "pnpm"])
    result = subprocess.run([*command, "audit", "--prod", "--json"], cwd=ROOT,
                            capture_output=True, text=True, timeout=600)
    try:
        report = json.loads(result.stdout or "{}")
    except json.JSONDecodeError:
        print(result.stdout + result.stderr)
        print("::error title=pnpm audit::pnpm audit didn't return a report")
        return 1
    advisories = report.get("advisories", {})
    for advisory in advisories.values():
        print(f"::error title=vulnerable npm package::{advisory.get('module_name')} "
              f"{advisory.get('severity')} {advisory.get('url')}")
    print(f"pnpm audit: {len(advisories)} advisory(ies)")
    return 1 if advisories else 0


if __name__ == "__main__":
    which = sys.argv[1] if len(sys.argv) > 1 else ""
    if which not in {"dotnet", "pnpm"}:
        print(__doc__)
        sys.exit(2)
    sys.exit(dotnet() if which == "dotnet" else pnpm())
