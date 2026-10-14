#!/usr/bin/env bash
# Validates one Conventional Commits 1.0.0 subject line: a commit subject or a PR title.
# Used by .githooks/commit-msg and by CI for PR titles.
#
#   scripts/check-commit-subject.sh "feat(proto): add handshake state machine"
#   scripts/check-commit-subject.sh "feat(proto): add handshake state machine" 246
#
# With a PR number, the subject is a PR title: squash-merging adds " (#246)" to it, and the
# result must still fit in 100 characters (#174).
set -euo pipefail

subject=${1:-}
pr=${2:-}

# Let git's own generated messages through.
case "$subject" in
  Merge\ *|Revert\ *|fixup!\ *|squash!\ *) exit 0 ;;
esac

types='feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert'
pattern="^(${types})(\([a-z0-9][a-z0-9._/-]*\))?!?: [^ ].*"

if ! printf '%s' "$subject" | grep -Eq "$pattern"; then
  cat >&2 <<MSG
not a Conventional Commit subject:
  $subject

Expected: <type>(<optional scope>)<optional !>: <description>
Types:    ${types//|/, }
Scopes:   proto, data, lua, auth, store, server, launcher, tools, content, deploy, docs, re, deps, release
Example:  feat(proto): add handshake state machine
MSG
  exit 1
fi

max=100
if [ -n "$pr" ]; then
  case "$pr" in
    *[!0-9]*) echo "not a PR number: $pr" >&2; exit 1 ;;
  esac
  suffix=" (#$pr)"
  max=$((100 - ${#suffix}))
fi

if [ "${#subject}" -gt "$max" ]; then
  if [ -n "$pr" ]; then
    echo "PR title is ${#subject} characters; with \"$suffix\" its squash subject needs it at $max or fewer" >&2
  else
    echo "subject is ${#subject} characters; keep it at 100 or fewer" >&2
  fi
  exit 1
fi
