#!/usr/bin/env bash
# Open (or update) the pull request for the current branch, using GitHub's REST API (ADR-025).
# Cloud sessions can't use `gh pr` (it needs GraphQL), but `gh api` works.
#
#   tools/open-pr.sh --title "M1: Foundations (M1.1 to M1.4)" --body-file /tmp/pr.md [--label ci:macos]
#
# The branch must be pushed first. Prints the PR URL. If the API isn't reachable, prints a link
# the user can open to create the PR by hand, and exits 2.
set -euo pipefail

TITLE=""; BODY_FILE=""; LABELS=()
while [ $# -gt 0 ]; do
  case "$1" in
    --title) TITLE="$2"; shift 2 ;;
    --body-file) BODY_FILE="$2"; shift 2 ;;
    --label) LABELS+=("$2"); shift 2 ;;
    *) echo "unknown option: $1" >&2; exit 64 ;;
  esac
done
[ -n "$TITLE" ] && [ -f "$BODY_FILE" ] || { echo "usage: $0 --title T --body-file F [--label L]..." >&2; exit 64; }

repo_slug() {
  if [ -n "${SELAH_REPO:-}" ]; then echo "$SELAH_REPO"; return; fi
  local url; url="$(git remote get-url origin)"
  url="${url%.git}"
  echo "$url" | awk -F'[/:]' '{print $(NF-1) "/" $NF}'
}

REPO="$(repo_slug)"
OWNER="${REPO%%/*}"
BRANCH="$(git rev-parse --abbrev-ref HEAD)"
BASE="main"

if [ "$BRANCH" = "$BASE" ] || [ "$BRANCH" = "HEAD" ]; then
  echo "open-pr: refusing to open a PR from '$BRANCH'. Work on a session branch (ADR-025)." >&2; exit 1
fi
if [ -n "$(git status --porcelain)" ]; then
  echo "open-pr: the working tree has uncommitted changes. Commit them first." >&2; exit 1
fi
REMOTE_SHA="$(git ls-remote origin "refs/heads/$BRANCH" | cut -f1)"
if [ "$REMOTE_SHA" != "$(git rev-parse HEAD)" ]; then
  echo "open-pr: origin/$BRANCH doesn't match HEAD. Push the branch first." >&2; exit 1
fi

fallback() {
  echo "open-pr: couldn't reach the GitHub API. Ask the user to open this link and select 'Create pull request':" >&2
  echo "https://github.com/$REPO/compare/$BASE...$BRANCH?expand=1"
  echo "(PR description: $BODY_FILE)"
  exit 2
}
command -v gh >/dev/null 2>&1 || fallback

NUMBER="$(gh api "repos/$REPO/pulls?head=$OWNER:$BRANCH&state=open" --jq '.[0].number // empty' 2>/dev/null)" || fallback
if [ -n "$NUMBER" ]; then
  gh api -X PATCH "repos/$REPO/pulls/$NUMBER" -f title="$TITLE" -F body=@"$BODY_FILE" >/dev/null || fallback
else
  NUMBER="$(gh api -X POST "repos/$REPO/pulls" -f title="$TITLE" -f head="$BRANCH" -f base="$BASE" \
    -F body=@"$BODY_FILE" --jq .number)" || fallback
fi

for label in "${LABELS[@]+"${LABELS[@]}"}"; do
  enc="$(printf '%s' "$label" | sed 's/:/%3A/g')"
  gh api "repos/$REPO/labels/$enc" >/dev/null 2>&1 \
    || gh api -X POST "repos/$REPO/labels" -f name="$label" -f color="1f6feb" >/dev/null 2>&1 || true
  gh api -X POST "repos/$REPO/issues/$NUMBER/labels" -f "labels[]=$label" >/dev/null \
    || echo "open-pr: couldn't add label $label" >&2
done

echo "https://github.com/$REPO/pull/$NUMBER"
