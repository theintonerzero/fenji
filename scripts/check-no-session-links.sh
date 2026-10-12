#!/usr/bin/env bash
# Refuses text that carries a Claude session link or ID: a session trailer (a line starting with
# Claude-Session:), or a claude.ai session or chat URL anywhere (CLAUDE.md, "Git, versions,
# changelog"). Used by .githooks/commit-msg and by scripts/check-pr-session-links.sh in CI.
# Co-Authored-By attribution is fine. A refusal names lines only: CI logs are public, so it never
# repeats the link.
#
#   scripts/check-no-session-links.sh [FILE]      # FILE, or standard input
set -euo pipefail

pattern='^[[:space:]]*claude-session:|claude\.ai/(code/session|chat/)'
rc=0
lines=$(LC_ALL=C grep -inE -- "$pattern" "${1:-/dev/stdin}" | cut -d: -f1 | paste -sd, -) || rc=$?
case "$rc" in
  0)
    echo "session links don't belong in commit messages or PR descriptions: line ${lines//,/, } carries one." >&2
    echo "Remove it (Co-Authored-By attribution is fine)." >&2
    exit 1
    ;;
  1) ;;
  *) echo "check-no-session-links: can't read ${1:-standard input}" >&2; exit 2 ;;
esac
