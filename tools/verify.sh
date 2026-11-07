#!/usr/bin/env bash
# The quality gate for Linux and macOS (TESTING-AND-QUALITY §1). Exit 0 means green.
#
#   tools/verify.sh --cloud     in a cloud session (adds the Windows type-check)
#   tools/verify.sh --ci        in CI (30-second fuzz smoke runs, annotations)
#   tools/verify.sh --fast      during a work package (skips golden, UI, e2e and web stages)
#   tools/verify.sh --release   release-mode checks (M10)
#
# The stages live in tools/verify.py so Linux, macOS and Windows (tools/verify.ps1) run the
# same gate. A summary and every stage's log go to target/verify-report/.
set -euo pipefail
cd "$(dirname "$0")/.."
if [ -d "$HOME/.dotnet" ] && ! command -v dotnet >/dev/null 2>&1; then
  export DOTNET_ROOT="$HOME/.dotnet" PATH="$HOME/.dotnet:$PATH"
fi
export DOTNET_CLI_TELEMETRY_OPTOUT=1 DOTNET_NOLOGO=1
exec python3 tools/verify.py "$@"
