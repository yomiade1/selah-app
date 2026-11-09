#!/usr/bin/env python3
"""The quality gate (TESTING-AND-QUALITY §1). `tools/verify.sh` and `tools/verify.ps1` call it.

    verify.py [--cloud] [--ci] [--fast] [--release] [--only STAGE...]

Stages run in order and stop at the first failure. Exit 0 means green.
--cloud    cloud session: adds `cargo check --target x86_64-pc-windows-gnu`
--ci       CI: 30-second fuzz smoke runs (failures are always printed as annotations)
--fast     skip golden frames, UI tests, end-to-end and web stages (during a work package)
--release  release-mode knowledge base lint and stricter checks (M10)

Each stage's output goes to target/verify-report/<stage>.log; a summary goes to
target/verify-report/summary.md. Failures print GitHub annotations (::error file=…,line=…::).
"""
import argparse
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
REPORT = ROOT / "target" / "verify-report"
WINDOWS = os.name == "nt"
PY = sys.executable

# rustc / clippy:   --> crates\x\src\y.rs:12:5      dotnet: path\File.cs(12,5): error CS1234
RUST_LOCATION = re.compile(r"-->\s+(?P<file>[^\s:]+(?::[^\s:]+)?):(?P<line>\d+):\d+")
DOTNET_LOCATION = re.compile(r"(?P<file>[\w./\\:-]+\.(?:cs|axaml|csproj))\((?P<line>\d+),\d+\): (?:error|warning) (?P<msg>.*)")
ANNOTATION = re.compile(r"^::(error|warning)")


def tool(name):
    """Resolve a program (pnpm and corepack are .cmd shims on Windows)."""
    for candidate in ([f"{name}.cmd", f"{name}.exe", name] if WINDOWS else [name]):
        found = shutil.which(candidate)
        if found:
            return found
    return name


def pnpm(*args):
    return [tool("corepack"), "pnpm", *args] if WINDOWS else [tool("pnpm"), *args]


def cargo_deny():
    base = ROOT / "third_party" / "tools" / "cargo-deny"
    for path in sorted(base.rglob("cargo-deny.exe" if WINDOWS else "cargo-deny")):
        return str(path)
    return "cargo-deny"


class Failed(Exception):
    pass


def relative(path):
    path = path.replace("\\", "/")
    root = str(ROOT).replace("\\", "/")
    return path[len(root) + 1:] if path.lower().startswith(root.lower()) else path


def annotate(stage, output):
    """Print annotations for a failed stage: one per located compiler error, else a summary."""
    found = 0
    lines = output.splitlines()
    for index, line in enumerate(lines):
        if ANNOTATION.match(line):
            print(line)
            found += 1
            continue
        match = RUST_LOCATION.search(line)
        if match and index > 0:
            message = next((l.strip() for l in reversed(lines[max(0, index - 3):index])
                            if l.strip().startswith(("error", "warning"))), lines[index - 1].strip())
            print(f"::error file={relative(match['file'])},line={match['line']}::{message}")
            found += 1
            continue
        match = DOTNET_LOCATION.search(line)
        if match:
            print(f"::error file={relative(match['file'])},line={match['line']}::{match['msg']}")
            found += 1
        if found >= 30:
            break
    if not found:
        tail = " / ".join(l.strip() for l in lines[-12:] if l.strip())[:900]
        print(f"::error title=verify stage {stage}::{tail}")


def run(stage, label, command, env=None, timeout=1800):
    started = time.monotonic()
    print(f"--- [{stage}] {label}: {' '.join(Path(c).name if i == 0 else c for i, c in enumerate(command))}", flush=True)
    env = dict(env or {})
    if env.pop("@ffmpeg", None):
        # Resolved now: on a fresh runner stage 1 has only just extracted (or built) FFmpeg.
        env.update(ffmpeg_env())
    # Avalonia's build tasks report anonymous usage unless told not to (SEC-13: no telemetry).
    merged = dict(os.environ, AVALONIA_TELEMETRY_OPTOUT="1", **env)
    if command[0] == "@cargo-deny":
        # Resolved now: on a fresh runner stage 1 has only just fetched the pinned binary.
        command = [cargo_deny(), *command[1:]]
    try:
        # One stream, so lines keep their order (cargo prints "Running" on stderr and results
        # on stdout; the ledger needs both in order).
        result = subprocess.run(command, cwd=ROOT, env=merged, stdout=subprocess.PIPE,
                                stderr=subprocess.STDOUT, text=True, encoding="utf-8",
                                errors="replace", timeout=timeout)
        output = result.stdout
        code = result.returncode
    except FileNotFoundError as error:
        output, code = f"{error}", 127
    except subprocess.TimeoutExpired as error:
        output = f"timed out after {timeout} s\n{(error.stdout or '')}{(error.stderr or '')}"
        code = 124
    REPORT.mkdir(parents=True, exist_ok=True)
    log = REPORT / f"{stage}-{re.sub(r'[^a-z0-9]+', '-', label.lower()).strip('-')}.log"
    log.write_text(output, encoding="utf-8")
    elapsed = time.monotonic() - started
    if code != 0:
        annotate(stage, output)
        raise Failed(f"[{stage}] {label} failed (exit {code}, {elapsed:.0f} s); log: {relative(str(log))}")
    print(f"    ok ({elapsed:.0f} s)", flush=True)
    return output


def skia_binaries_url():
    """skia-bindings reads the pinned, hash-checked Skia build (tools/third_party.lock.json)."""
    machine = platform.machine().lower()
    arch = "arm64" if machine in {"arm64", "aarch64"} else "x64"
    system = {"win32": "windows", "darwin": "macos"}.get(sys.platform, "linux")
    folder = (ROOT / "third_party" / "skia" / f"{system}-{arch}").as_posix()
    return f"file://{folder}/skia-binaries-{{key}}.tar.gz"


def platform_key():
    machine = platform.machine().lower()
    arch = "arm64" if machine in {"arm64", "aarch64"} else "x64"
    return f"{ {'win32': 'windows', 'darwin': 'macos'}.get(sys.platform, 'linux')}-{arch}"


def ffmpeg_env():
    """FFMPEG_DIR for the FFmpeg bindings (selah-media) and the library path to run them.

    The pinned build is extracted (or, on macOS, built by tools/build-ffmpeg-macos.sh) into
    third_party/ffmpeg/<platform>/shared/ffmpeg-*/ with bin/, include/ and lib/.
    """
    found = sorted((ROOT / "third_party" / "ffmpeg" / platform_key() / "shared").glob("ffmpeg-*/include"))
    if not found:
        return {}
    home = found[0].parent
    env = {"FFMPEG_DIR": str(home)}
    if WINDOWS:
        env["PATH"] = f"{home / 'bin'}{os.pathsep}{os.environ.get('PATH', '')}"
    elif sys.platform != "darwin":
        env["LD_LIBRARY_PATH"] = f"{home / 'lib'}{os.pathsep}{os.environ.get('LD_LIBRARY_PATH', '')}"
    return env


def previous_milestone_tag():
    """Most recent milestone tag reachable from HEAD (m1, m2, ...) for buf breaking."""
    result = subprocess.run(
        ["git", "describe", "--tags", "--abbrev=0", "--match", "m[0-9]*"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=False,
    )
    return result.stdout.strip() or "m1"


def stages(args):
    rust_env = {"CARGO_TERM_COLOR": "never", "SELAH_GPU": "software",
                "SKIA_BINARIES_URL": skia_binaries_url(), "@ffmpeg": "1"}
    fuzz_seconds = "30" if args.ci else "5"
    plan = []

    def stage(number, label, command, env=None, skip=False, timeout=1800):
        plan.append((number, label, command, env, skip, timeout))

    fresh = args.ci or args.cloud
    # A fresh runner or cloud session has rustup but not yet the pinned toolchain: install the
    # one rust-toolchain.toml names, so the check below tests the pin rather than the image.
    channel = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text()).group(1)
    stage("1", "rust toolchain", [tool("rustup"), "toolchain", "install", channel, "--profile", "minimal",
                                  "--component", "clippy", "--component", "rustfmt"], skip=not fresh)
    stage("1", "toolchain", [PY, "tools/check-toolchain.py"])
    # Lockfile enforced (SEC-09). Locally this is quick when node_modules is current.
    stage("1", "web dependencies", pnpm("install", "--frozen-lockfile"))
    # CI runners and cloud sessions start without the pinned third-party files.
    stage("1", "third-party files", [PY, "tools/third-party.py", "--extract"],
          skip=not (args.ci or args.cloud) and (ROOT / "third_party" / "fonts").is_dir())
    # No LGPL macOS FFmpeg binary is published: build the decode-only one from the pinned source.
    stage("1", "ffmpeg for macOS", ["bash", "tools/build-ffmpeg-macos.sh", "--if-missing"],
          skip=sys.platform != "darwin", timeout=1200)
    stage("2", "rustfmt", [tool("cargo"), "fmt", "--all", "--check"])
    stage("2", "dotnet format", [tool("dotnet"), "format", "app/Selah.sln", "--verify-no-changes"], timeout=900)
    stage("2", "prettier and buf format", pnpm("run", "format:check"))
    stage("2", "web format", pnpm("-r", "run", "format:check"))
    stage("3", "clippy", [tool("cargo"), "clippy", "--workspace", "--all-targets", "--locked", "--", "-D", "warnings"], rust_env)
    stage("3", "web lint", pnpm("-r", "run", "lint"))
    stage("4", "buf lint", pnpm("exec", "buf", "lint"))
    stage("4", "buf breaking", pnpm("exec", "buf", "breaking", "--against", f".git#tag={previous_milestone_tag()}"))
    stage("4", "generated contracts are current", [PY, "tools/check-generated.py"])
    stage("5", "cargo build", [tool("cargo"), "build", "--workspace", "--all-targets", "--locked"], rust_env)
    # The FFmpeg-linked crates are left out: their bindings need the Windows FFmpeg headers, and
    # the windows CI job builds them natively.
    stage("5", "windows type-check", [tool("cargo"), "check", "--workspace", "--locked", "--target", "x86_64-pc-windows-gnu",
                                      "--exclude", "selah-media", "--exclude", "selah-bench", "--exclude", "selah-testkit"],
          rust_env, skip=not args.cloud)
    stage("5", "dotnet build", [tool("dotnet"), "build", "app/Selah.sln", "-c", "Release"], timeout=900)
    stage("5", "web build", pnpm("-r", "run", "build"))
    stage("6", "cargo test", [tool("cargo"), "test", "--workspace", "--locked"],
          dict(rust_env, SELAH_FUZZ_SECONDS=fuzz_seconds))
    stage("6", "dotnet test", [tool("dotnet"), "test", "app/Selah.Client.Tests/Selah.Client.Tests.csproj", "-c", "Release",
                               "--no-build", "--blame-hang-timeout", "120s"], timeout=900)
    stage("6", "web test", pnpm("-r", "run", "test"))
    stage("6", "python tool tests", [PY, "-m", "unittest", "discover", "-s", "tests/tools"])
    stage("6", "ipc benchmark", [tool("cargo"), "run", "--release", "--locked", "-q", "-p", "selah-bench", "--",
                                 "ipc", "--check", "--json", "target/verify-report/bench-ipc.json"], rust_env)
    stage("6", "search benchmark", [tool("cargo"), "run", "--release", "--locked", "-q", "-p", "selah-bench", "--",
                                    "search", "--check", "--json", "target/verify-report/bench-search.json"], rust_env)
    stage("7", "golden frames", [tool("cargo"), "test", "--locked", "-p", "selah-testkit", "--features", "golden",
                                 "--test", "golden_text"], rust_env, skip=args.fast)
    # Stage 8: headless UI tests at 100/150/200%, Light and Booth, en and en-XA (M1-A9).
    stage("8", "ui tests", [tool("dotnet"), "test", "app/Selah.UI.Tests/Selah.UI.Tests.csproj", "-c", "Release",
                            "--no-build", "--blame-hang-timeout", "120s"], timeout=900, skip=args.fast)
    stage("8", "lint strings", [PY, "tools/lint-strings.py"])
    # Stage 9: the knowledge base lint (KNOWLEDGE-BASE §7.1) and kb.db (target/kb/kb.db).
    kb_flags = ["--release"] if args.release else []
    stage("9", "kb lint", [tool("cargo"), "run", "-q", "--locked", "-p", "selah-kb-lint", "--", *kb_flags], rust_env)
    stage("9", "kb build", [tool("cargo"), "run", "-q", "--locked", "-p", "selah-kb", "--bin", "selah-kb", "--", "build", *kb_flags], rust_env)
    stage("13", "test ledger", [PY, "tools/test-ledger.py", "--check"])
    stage("14", "cargo deny", ["@cargo-deny", "check"])
    stage("14", "dotnet vulnerable packages", [PY, "tools/check-vulnerable.py", "dotnet"])
    stage("14", "pnpm audit", [PY, "tools/check-vulnerable.py", "pnpm"])
    stage("14", "secret scan", [PY, "tools/check-secrets.py"])
    stage("14", "workflow policy", [PY, "tools/check-workflows.py"])
    stage("14", "unsafe policy", [PY, "tools/check-unsafe.py"])
    return plan


def main():
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--cloud", action="store_true")
    parser.add_argument("--ci", action="store_true")
    parser.add_argument("--fast", action="store_true")
    parser.add_argument("--release", action="store_true")
    parser.add_argument("--only", nargs="*", help="run only these stage numbers")
    args = parser.parse_args()
    if os.environ.get("SELAH_CI") == "1":
        args.ci = True
    if REPORT.exists():
        shutil.rmtree(REPORT, ignore_errors=True)
    REPORT.mkdir(parents=True, exist_ok=True)
    started = time.monotonic()
    results = []
    status = 0
    for number, label, command, env, skip, timeout in stages(args):
        if args.only and number not in args.only:
            continue
        if skip:
            results.append((number, label, "skipped"))
            continue
        try:
            output = run(number, label, command, env, timeout)
            results.append((number, label, "passed"))
            if label in {"cargo test", "dotnet test", "web test", "golden frames", "ui tests", "kb lint"}:
                (REPORT / "counts").mkdir(exist_ok=True)
                (REPORT / "counts" / f"{label.replace(' ', '-')}.log").write_text(output, encoding="utf-8")
        except Failed as failure:
            print(f"::error title=verify::{failure}")
            results.append((number, label, "FAILED"))
            status = 1
            break
    summary = ["# Gate summary", "", f"- Platform: {platform.system()} {platform.machine()}",
               f"- Flags: {' '.join(a for a in ('--cloud', '--ci', '--fast', '--release') if getattr(args, a[2:]))}",
               f"- Result: {'green' if status == 0 else 'red'} in {time.monotonic() - started:.0f} s", "",
               "| Stage | Check | Result |", "|---|---|---|"]
    summary += [f"| {n} | {l} | {r} |" for n, l, r in results]
    (REPORT / "summary.md").write_text("\n".join(summary) + "\n", encoding="utf-8")
    (REPORT / "summary.json").write_text(json.dumps(results, indent=1), encoding="utf-8")
    print("\n".join(summary[-len(results) - 2:]))
    print("verify: GREEN" if status == 0 else "verify: RED")
    return status


if __name__ == "__main__":
    sys.exit(main())
