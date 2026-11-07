#!/usr/bin/env bash
# Toolchain check and pinned third-party fetch for Linux and macOS.
#
#   tools/setup.sh --check   read-only toolchain check (the gate's stage 1)
#   tools/setup.sh --fetch   download pinned files into third_party/ (SHA-256 checked against
#                            tools/third_party.lock.json), extract them, build FFmpeg on macOS,
#                            and generate the synthetic test media
#
# Installing system toolchains is explicit and separate: cloud sessions use tools/cloud-setup.sh,
# local machines follow docs/specs/foundations.md. Verification never downloads anything except
# through --fetch, and tests never touch the network.
set -euo pipefail
cd "$(dirname "$0")/.."

fetch() {
  python3 tools/third-party.py --extract
  if [ "$(uname -s)" = "Darwin" ]; then
    # No LGPL macOS binary build is published, so build a decode-only FFmpeg from the pinned
    # source. It has no H.264/HEVC encoders; tests use the committed clip in tests/fixtures/media/.
    bash tools/build-ffmpeg-macos.sh
  else
    python3 tools/generate-media.py
  fi
}

case "${1:---check}" in
  --check) exec python3 tools/check-toolchain.py ;;
  --fetch) fetch ;;
  *) echo 'Usage: bash tools/setup.sh --check | --fetch (cloud provisioning: tools/cloud-setup.sh)' >&2; exit 2 ;;
esac
