#!/usr/bin/env bash
# Validates one Conventional Commits 1.0.0 subject line: a commit subject or a PR title.
# Used by .githooks/commit-msg and by CI for PR titles.
#
#   scripts/check-commit-subject.sh "feat(proto): add handshake state machine"
set -euo pipefail

subject=${1:-}

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

if [ "${#subject}" -gt 100 ]; then
  echo "subject is ${#subject} characters; keep it at 100 or fewer" >&2
  exit 1
fi
