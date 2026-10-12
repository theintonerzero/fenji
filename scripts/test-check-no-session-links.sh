#!/usr/bin/env bash
# Tests for scripts/check-no-session-links.sh and the commit-msg hook that runs it. Runs anywhere
# bash and git run (macOS, Fedora, CI).
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
check=$here/check-no-session-links.sh
hook=$here/../.githooks/commit-msg
tmp=$(mktemp -d)
trap 'rm -rf -- "$tmp"' EXIT
failures=0

result() {  # WANT NAME GOT
  if [ "$3" = "$1" ]; then
    echo "ok - $1: $2"
  else
    echo "FAIL - expected $1, got $3: $2" >&2
    failures=$((failures + 1))
  fi
}

# expect WANT NAME TEXT: the checker, given TEXT on standard input.
expect() {
  local got=reject
  if printf '%s\n' "$3" | "$check" >/dev/null 2>&1; then got=accept; fi
  result "$1" "$2" "$got"
}

# hook WANT NAME MESSAGE: the commit-msg hook, given a message file, run inside this repository.
hook() {
  local got=reject
  printf '%s\n' "$3" > "$tmp/msg"
  if (cd "$here" && "$hook" "$tmp/msg") >/dev/null 2>&1; then got=accept; fi
  result "$1" "hook: $2" "$got"
}

session=https://claude.ai/code/session_01AbCdEfGhIjKlMnOpQrStUv
expect accept "a plain message" "$(printf 'feat(proto): add handshake state machine\n\nBody text.')"
expect accept "co-author attribution" "$(printf 'fix: x\n\nCo-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>')"
expect accept "the generic Claude Code link" "Generated with [Claude Code](https://claude.com/claude-code)"
expect accept "an empty message" ""
expect reject "a session trailer" "$(printf 'fix: x\n\nClaude-Session: %s' "$session")"
expect reject "a session trailer, lower case" "$(printf 'fix: x\n\nclaude-session: anything')"
expect reject "a session URL in the body" "$(printf 'docs: y\n\nSee %s for the run.' "$session")"
expect reject "a chat link" "$(printf 'docs: z\n\nFrom https://claude.ai/chat/0f1e2d3c-4b5a-6978-8091-a2b3c4d5e6f7')"

# A file argument reads as standard input does; a file that can't be read is an error, not a pass.
printf 'Claude-Session: x\n' > "$tmp/file"
got=reject; if "$check" "$tmp/file" >/dev/null 2>&1; then got=accept; fi
result reject "a session trailer in a file argument" "$got"
rc=0; "$check" "$tmp/missing" >/dev/null 2>&1 || rc=$?
result 2 "a missing file argument exits" "$rc"

hook accept "a clean message" "$(printf 'feat(tools): add x\n\nCo-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>')"
hook reject "a session trailer" "$(printf 'feat(tools): add x\n\nClaude-Session: %s' "$session")"
hook reject "a bad subject, still" "added stuff"
# git drops comment lines, and everything below the scissors line `git commit -v` adds (the diff,
# which may well show this very rule's patterns): neither is part of the message.
hook accept "comments and a verbose diff aren't the message" "$(printf 'feat(tools): add x\n\n# Claude-Session: %s\n# ------------------------ >8 ------------------------\n+Claude-Session: %s' "$session" "$session")"

[ "$failures" -eq 0 ] || { echo "$failures failure(s)" >&2; exit 1; }
echo "all session-link checks passed"
