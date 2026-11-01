#!/usr/bin/env python3
"""Scan tracked files for secrets and personal email addresses (docs/SECURITY.md §6).

Run from the repository root: python3 tools/check-secrets.py
Test data must use obviously fake values such as TEST-SECRET-1234, which this scan ignores.
A line that must contain a matching pattern on purpose can end with: secret-scan: allow
"""
import os
import re
import subprocess
import sys

PATTERNS = [
    ("private key", re.compile(r"-----BEGIN (?:RSA |EC |DSA |OPENSSH |PGP |ENCRYPTED )?PRIVATE KEY-----")),
    ("GitHub token", re.compile(r"\b(?:gh[pousr]_[A-Za-z0-9]{36,}|github_pat_[A-Za-z0-9_]{60,})")),
    ("AWS access key", re.compile(r"\bAKIA[0-9A-Z]{16}\b")),
    ("Slack token", re.compile(r"\bxox[abprs]-[A-Za-z0-9-]{10,}")),
    ("Stripe key", re.compile(r"\b[sr]k_live_[0-9A-Za-z]{20,}")),
    ("Google API key", re.compile(r"\bAIza[0-9A-Za-z_-]{35}\b")),
    ("Anthropic key", re.compile(r"\bsk-ant-[A-Za-z0-9_-]{20,}")),
    ("OpenAI key", re.compile(r"\bsk-proj-[A-Za-z0-9_-]{20,}")),
    ("Twitch stream key", re.compile(r"\blive_\d{6,}_[A-Za-z0-9]{20,}")),
    ("assigned secret", re.compile(
        r"(?i)\b(?:password|passwd|secret|token|api[_-]?key|stream[_-]?key|client[_-]?secret)\b"
        r"\s*[:=]\s*[\"'][^\"'\s]{16,}[\"']")),
    ("personal email address", re.compile(
        r"\b[\w.+-]+@(?:gmail|googlemail|yahoo|ymail|outlook|hotmail|live|icloud|me|aol|proton|protonmail)"
        r"\.(?:com|me|co\.uk|ch)\b", re.I)),
]
FAKE = re.compile(r"TEST-SECRET-|EXAMPLE|example|xxxxxxxx|<[A-Za-z_ -]+>")
BANNED_FILES = re.compile(r"(^|/)(\.env(\..+)?|id_rsa[^/]*|id_ed25519[^/]*|[^/]+\.(pfx|p12|mobileprovision))$")
BINARY_EXT = {".png", ".jpg", ".jpeg", ".gif", ".ico", ".webp", ".ttf", ".otf", ".woff", ".woff2",
              ".zip", ".db", ".sqlite", ".mp4", ".mov", ".wav", ".mp3", ".pdf", ".exe", ".dll"}
MAX_BYTES = 2 * 1024 * 1024


def tracked_files():
    out = subprocess.run(["git", "ls-files", "-z"], capture_output=True, check=True).stdout
    return [p for p in out.decode("utf-8").split("\0") if p]


def main():
    problems = []
    for path in tracked_files():
        if BANNED_FILES.search(path):
            problems.append((path, 1, "this kind of file holds secrets and must not be committed"))
            continue
        if os.path.splitext(path)[1].lower() in BINARY_EXT or not os.path.isfile(path):
            continue
        if os.path.getsize(path) > MAX_BYTES:
            continue
        try:
            with open(path, encoding="utf-8") as f:
                lines = f.read().splitlines()
        except UnicodeDecodeError:
            continue
        for n, line in enumerate(lines, 1):
            if "secret-scan: allow" in line:
                continue
            for label, rx in PATTERNS:
                m = rx.search(line)
                if m and not FAKE.search(m.group(0)):
                    problems.append((path, n, f"possible {label}"))
    gha = os.environ.get("GITHUB_ACTIONS") == "true"
    for path, line, message in problems:
        if gha:
            print(f"::error file={path},line={line}::{message}")
        print(f"{path}:{line}: {message}", file=sys.stderr)
    if problems:
        print(f"check-secrets: {len(problems)} problem(s). Remove the value; use the OS keychain or a fake.",
              file=sys.stderr)
        return 1
    print("check-secrets: no secrets or personal email addresses found")
    return 0


if __name__ == "__main__":
    sys.exit(main())
