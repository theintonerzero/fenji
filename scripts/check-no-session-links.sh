#!/usr/bin/env bash
# Refuses text that carries a Claude session link or ID: a session trailer, or a claude.ai session
# or chat URL (CLAUDE.md, "Git, versions, changelog"). Used by .githooks/commit-msg and by CI for
# a PR's description and new commits. Co-Authored-By attribution is fine.
#
#   scripts/check-no-session-links.sh [FILE]      # FILE, or standard input
set -euo pipefail

pattern='claude-session:|claude\.ai/(code/session|chat/)'
rc=0
found=$(LC_ALL=C grep -inE -- "$pattern" "${1:-/dev/stdin}") || rc=$?
case "$rc" in
  0)
    {
      echo "session links don't belong in commit messages or PR descriptions:"
      printf '%s\n' "$found" | sed 's/^/  line /'
      echo "Remove them (Co-Authored-By attribution is fine)."
    } >&2
    exit 1
    ;;
  1) ;;
  *) echo "check-no-session-links: can't read ${1:-standard input}" >&2; exit 2 ;;
esac
