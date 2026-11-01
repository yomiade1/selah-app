#!/usr/bin/env bash
# Read and start CI from a cloud session, using GitHub's REST API (ADR-024).
#
#   tools/ci.sh status [--wait]   latest CI run for HEAD: jobs, results, and annotations of failed jobs
#   tools/ci.sh run [--macos]     start ci.yml on the current branch (at most once per session)
#   tools/ci.sh link              the run page, where the installer artifact can be downloaded
#
# Failures are reported as annotations (file, line, message) because job logs are stored on hosts
# that cloud sessions may not be able to reach.
set -euo pipefail

CMD="${1:-status}"; shift || true

repo_slug() {
  if [ -n "${SELAH_REPO:-}" ]; then echo "$SELAH_REPO"; return; fi
  local url; url="$(git remote get-url origin)"
  url="${url%.git}"
  echo "$url" | awk -F'[/:]' '{print $(NF-1) "/" $NF}'
}
REPO="$(repo_slug)"
BRANCH="$(git rev-parse --abbrev-ref HEAD)"
SHA="$(git rev-parse HEAD)"

latest_run() {
  # PR runs test the merge commit, but they're listed under the head commit's SHA.
  gh api "repos/$REPO/actions/workflows/ci.yml/runs?head_sha=$SHA&per_page=1" \
    --jq '.workflow_runs[0] | select(.) | "\(.id) \(.status) \(.conclusion // "-") \(.html_url)"'
}

print_run() {
  local id="$1"
  gh api "repos/$REPO/actions/runs/$id/jobs?per_page=50" \
    --jq '.jobs[] | "\(.id)\t\(.name)\t\(.status)\t\(.conclusion // "-")"' |
  while IFS=$'\t' read -r job name status conclusion; do
    printf '  %-10s %-12s %s\n' "$name" "$status" "$conclusion"
    if [ "$conclusion" = "failure" ]; then
      gh api "repos/$REPO/check-runs/$job/annotations?per_page=50" \
        --jq '.[] | "    \(.annotation_level): \(.path):\(.start_line) \(.message | gsub("\n"; " "))"' || true
    fi
  done
}

case "$CMD" in
  status)
    WAIT="no"; [ "${1:-}" = "--wait" ] && WAIT="yes"
    deadline=$(( $(date +%s) + 50 * 60 ))
    while :; do
      run="$(latest_run || true)"
      if [ -z "$run" ]; then
        echo "ci: no CI run for $SHA yet."
        [ "$WAIT" = "yes" ] && [ "$(date +%s)" -lt "$deadline" ] && { sleep 60; continue; }
        exit 3
      fi
      read -r id status conclusion url <<<"$run"
      if [ "$status" = "completed" ] || [ "$WAIT" = "no" ] || [ "$(date +%s)" -ge "$deadline" ]; then
        echo "ci: run $id is $status ($conclusion): $url"
        print_run "$id"
        [ "$status" = "completed" ] || exit 3
        [ "$conclusion" = "success" ] && exit 0 || exit 1
      fi
      sleep 60
    done
    ;;
  run)
    MACOS="false"; [ "${1:-}" = "--macos" ] && MACOS="true"
    active="$(gh api "repos/$REPO/actions/workflows/ci.yml/runs?branch=$BRANCH&per_page=5" \
      --jq '[.workflow_runs[] | select(.status != "completed")] | length')"
    if [ "$active" != "0" ]; then echo "ci: a run is already going for $BRANCH; use 'tools/ci.sh status --wait'." >&2; exit 1; fi
    gh api -X POST "repos/$REPO/actions/workflows/ci.yml/dispatches" -f ref="$BRANCH" -f "inputs[macos]=$MACOS" >/dev/null
    echo "ci: started ci.yml on $BRANCH (macos=$MACOS). Check with: tools/ci.sh status --wait"
    ;;
  link)
    run="$(latest_run || true)"
    [ -n "$run" ] || { echo "ci: no CI run for $SHA yet." >&2; exit 3; }
    echo "${run##* }"
    ;;
  *) echo "usage: $0 status [--wait] | run [--macos] | link" >&2; exit 64 ;;
esac
