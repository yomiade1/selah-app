#!/usr/bin/env bash
# Re-render the golden reference frames on purpose (TESTING-AND-QUALITY §3, rule 3).
#
#   tools/bless-golden.sh --reason "why the frames changed" [--filter <test name>]
#
# Writes tests/golden/reference/*.png and appends the reason to tests/golden/CHANGES.md.
# Review the new images in the PR. Never bless to hide a bug. Platform-specific images are
# added only through the bless-golden.yml workflow.
set -euo pipefail
cd "$(dirname "$0")/.."

reason=""
filter=""
while [ $# -gt 0 ]; do
  case "$1" in
    --reason) reason="${2:-}"; shift 2 ;;
    --filter) filter="${2:-}"; shift 2 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done
if [ -z "$reason" ]; then
  echo 'Usage: tools/bless-golden.sh --reason "why the frames changed" [--filter <test>]' >&2
  exit 2
fi

export SELAH_BLESS_GOLDEN=1 SELAH_GPU=software
cargo test -p selah-testkit --features golden --test golden_text --test golden_m3 -- ${filter:+"$filter"}

{
  echo
  echo "## $(date -u +%Y-%m-%d) — reference (${filter:-all})"
  echo
  echo "- Reason: $reason"
  echo "- Rendered on: $(uname -s) $(uname -m), adapter chosen with SELAH_GPU=software"
} >> tests/golden/CHANGES.md
git status --short tests/golden
