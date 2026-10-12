#!/usr/bin/env bash
# CI's session-link check for a pull request: its title and description (TITLE and BODY in the
# environment), and each of its commits that isn't in DEV yet. A release PR brings dev's history,
# whose squash commits predate the rule, so those aren't checked. Any git failure fails the check.
#
#   TITLE=... BODY=... scripts/check-pr-session-links.sh BASE_SHA HEAD_SHA [DEV]   # DEV: origin/dev
set -euo pipefail
[ $# -ge 2 ] || { echo "usage: check-pr-session-links.sh BASE_SHA HEAD_SHA [DEV]" >&2; exit 2; }
check="$(cd "$(dirname "$0")" && pwd)/check-no-session-links.sh"
base=$1 head=$2 dev=${3:-origin/dev}
failed=0
if ! printf '%s\n' "${TITLE:-}" | "$check"; then echo "  (in the PR title)" >&2; failed=1; fi
if ! printf '%s\n' "${BODY:-}" | "$check"; then echo "  (in the PR description)" >&2; failed=1; fi
commits=$(git rev-list "$head" --not "$base" "$dev" --)
for c in $commits; do
  msg=$(git log -1 --format=%B "$c")
  if ! printf '%s\n' "$msg" | "$check"; then echo "  (in commit $(git rev-parse --short "$c"))" >&2; failed=1; fi
done
exit "$failed"
