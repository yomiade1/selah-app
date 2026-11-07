#!/usr/bin/env python3
"""Fetch immutable, hash-pinned third-party files. Never runs during tests."""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import platform
import re
import shutil
import tarfile
import tempfile
import sys
import time
import urllib.parse
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]
ALLOWED_HOSTS = {"raw.githubusercontent.com", "github.com", "ffmpeg.org"}
MAX_BYTES = 512 * 1024 * 1024
# Generous for slow connections (a 25 MB file at 15 KB/s), but still bounded.
MAX_SECONDS = 1800
ARCHIVE_SUFFIXES = (".zip", ".tar.xz", ".tar.gz")


def validate(asset):
    url = urllib.parse.urlsplit(asset["url"])
    if url.scheme != "https" or url.hostname not in ALLOWED_HOSTS or url.username or url.password:
        raise ValueError("asset URL must be HTTPS on a trusted upstream host")
    relative = PurePosixPath(asset["path"])
    if (not relative.parts or relative.is_absolute() or ".." in relative.parts
            or "\\" in asset["path"] or ":" in asset["path"]):
        raise ValueError("asset path must stay within third_party")
    if not re.fullmatch(r"[0-9a-f]{64}", asset["sha256"]):
        raise ValueError("asset SHA-256 must be pinned")
    return relative


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def fetch(asset, destination):
    relative = validate(asset)
    base = destination.resolve()
    target = base.joinpath(*relative.parts).resolve()
    if not target.is_relative_to(base):
        raise ValueError("asset destination escapes third_party through a symlink")
    if target.is_file() and digest(target) == asset["sha256"]:
        return
    target.parent.mkdir(parents=True, exist_ok=True)
    temporary = None
    deadline = time.monotonic() + MAX_SECONDS
    try:
        request = urllib.request.Request(asset["url"], headers={"User-Agent": "Selah-setup"})
        with urllib.request.urlopen(request, timeout=60) as response, \
                tempfile.NamedTemporaryFile(dir=target.parent, delete=False) as output:
            temporary = Path(output.name)
            final_url = urllib.parse.urlsplit(response.url)
            if final_url.scheme != "https":
                raise ValueError("insecure download redirect")
            size = 0
            while True:
                if time.monotonic() > deadline:
                    raise TimeoutError("third-party download exceeded its time limit")
                # read1 returns after one buffered/socket read. read(n) can
                # wait indefinitely for n bytes while a peer trickles data.
                chunk = response.read1(64 * 1024)
                if not chunk:
                    break
                size += len(chunk)
                if size > MAX_BYTES:
                    raise ValueError("third-party download exceeds size limit")
                output.write(chunk)
        if digest(temporary) != asset["sha256"]:
            raise ValueError(f"SHA-256 mismatch: {asset['path']}")
        temporary.replace(target)
        temporary = None
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def extract(asset, destination, check_only=False):
    """Stage verified archives before publishing. Links become bounded regular copies."""
    if not asset["path"].endswith(ARCHIVE_SUFFIXES):
        return
    validate(asset)
    root = destination.resolve()
    archive_path = (root / asset["path"]).resolve()
    if not archive_path.is_relative_to(root) or digest(archive_path) != asset["sha256"]:
        raise ValueError("archive must stay in third_party and match its pinned hash")
    name = asset.get("extract_to", "shared")
    if not re.fullmatch(r"[a-z0-9][a-z0-9-]{0,40}", name):
        raise ValueError("extract_to must be a simple folder name")
    target = archive_path.parent / name
    marker = target / ".source-sha256"
    if marker.is_file() and marker.read_text() == asset["sha256"]:
        return
    if check_only:
        raise ValueError(f"archive is not extracted: {asset['path']}")
    with tempfile.TemporaryDirectory(dir=archive_path.parent) as directory:
        staging = Path(directory).resolve()
        total = 0
        seen_paths = set()

        def output_path(name):
            relative = PurePosixPath(name)
            if relative.is_absolute() or ".." in relative.parts or "\\" in name or ":" in name:
                raise ValueError("unsafe archive entry path")
            path = (staging / name).resolve()
            if not path.is_relative_to(staging):
                raise ValueError("archive entry escapes its destination")
            if path in seen_paths:
                raise ValueError("duplicate archive entry")
            seen_paths.add(path)
            return path

        def copy(stream, path, size, mode=0):
            nonlocal total
            total += size
            if size > MAX_BYTES or total > 2 * 1024 * 1024 * 1024:
                raise ValueError("archive expands beyond its size limit")
            path.parent.mkdir(parents=True, exist_ok=True)
            with path.open("wb") as output:
                shutil.copyfileobj(stream, output)
            if mode & 0o111:
                path.chmod(0o755)

        if zipfile.is_zipfile(archive_path):
            with zipfile.ZipFile(archive_path) as archive:
                entries = archive.infolist()
                if len(entries) > 20_000:
                    raise ValueError("too many archive entries")
                for entry in entries:
                    path = output_path(entry.filename)
                    if entry.is_dir():
                        path.mkdir(parents=True, exist_ok=True)
                        continue
                    mode = entry.external_attr >> 16
                    if mode & 0o170000 == 0o120000:
                        raise ValueError("ZIP symlinks are not permitted")
                    with archive.open(entry) as stream:
                        copy(stream, path, entry.file_size, mode)
        else:
            mode = "r:gz" if asset["path"].endswith(".tar.gz") else "r:xz"
            with tarfile.open(archive_path, mode) as archive:
                entries = archive.getmembers()
                if len(entries) > 20_000:
                    raise ValueError("too many archive entries")
                links = []
                for entry in entries:
                    path = output_path(entry.name)
                    if entry.isdir():
                        path.mkdir(parents=True, exist_ok=True)
                    elif entry.isfile():
                        with archive.extractfile(entry) as stream:
                            copy(stream, path, entry.size, entry.mode)
                    elif entry.issym() or entry.islnk():
                        if "\\" in entry.linkname or ":" in entry.linkname:
                            raise ValueError("unsafe archive link")
                        base = path.parent if entry.issym() else staging
                        source = (base / entry.linkname).resolve()
                        if not source.is_relative_to(staging):
                            raise ValueError("archive link escapes its destination")
                        links.append((source, path))
                    else:
                        raise ValueError("archive special files are not permitted")
                for _ in range(8):
                    remaining = []
                    for source, path in links:
                        if source.is_file():
                            with source.open("rb") as stream:
                                copy(stream, path, source.stat().st_size, source.stat().st_mode)
                        else:
                            remaining.append((source, path))
                    links = remaining
                    if not links:
                        break
                if links:
                    raise ValueError("archive contains unresolved or cyclic links")
        (staging / ".source-sha256").write_text(asset["sha256"])
        # Only this tool's exact cache directory is replaced. Resolve both
        # paths and enforce the third_party boundary before removing it.
        resolved_target = target.resolve()
        if not resolved_target.is_relative_to(root) or resolved_target == root:
            raise ValueError("extraction target escapes third_party")
        if target.exists():
            shutil.rmtree(resolved_target)
        staging.rename(target)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="offline validation; never download")
    parser.add_argument("--all-platforms", action="store_true", help="include other platforms' assets")
    parser.add_argument("--extract", action="store_true", help="also extract verified FFmpeg archives")
    args = parser.parse_args()
    manifest = json.loads((ROOT / "tools/third_party.lock.json").read_text(encoding="utf-8"))
    seen = set()
    machine = platform.machine().lower()
    architecture = "arm64" if machine in {"aarch64", "arm64"} else "x64"
    system = {"win32": "windows", "linux": "linux", "darwin": "macos"}.get(sys.platform, sys.platform)
    current_platform = f"{system}-{architecture}"
    for asset in manifest["assets"]:
        validate(asset)
        if asset["path"] in seen:
            raise ValueError("duplicate asset path")
        seen.add(asset["path"])
        if asset.get("platform", current_platform) != current_platform and not args.all_platforms:
            continue
        if args.check:
            path = ROOT / "third_party" / asset["path"]
            if not path.is_file() or digest(path) != asset["sha256"]:
                raise ValueError(f"missing or modified asset: {asset['path']}")
        else:
            fetch(asset, ROOT / "third_party")
        if args.extract:
            extract(asset, ROOT / "third_party", check_only=args.check)
        print(f"verified {asset['path']}", flush=True)


if __name__ == "__main__":
    main()
