#!/usr/bin/env bash
# Tests for scripts/check-no-session-links.sh, the commit-msg hook that runs it, and
# scripts/check-pr-session-links.sh, CI's check for a pull request. Runs anywhere bash and git run
# (macOS, Fedora, CI).
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
check=$here/check-no-session-links.sh
prcheck=$here/check-pr-session-links.sh
hooks="$(cd "$here/../.githooks" && pwd)"
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

# hook WANT NAME MESSAGE: the commit-msg hook on a message file, run from outside any repository,
# so it must find its scripts beside itself.
hook() {
  local got=reject
  printf '%s\n' "$3" > "$tmp/msg"
  if (cd "$tmp" && "$hooks/commit-msg" "$tmp/msg") </dev/null >/dev/null 2>&1; then got=accept; fi
  result "$1" "hook: $2" "$got"
}

id=01AbCdEfGhIjKlMnOpQrStUv
session=https://claude.ai/code/session_$id
expect accept "a plain message" "$(printf 'feat(proto): add handshake state machine\n\nBody text.')"
expect accept "co-author attribution" "$(printf 'fix: x\n\nCo-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>')"
expect accept "the generic Claude Code link" "Generated with [Claude Code](https://claude.com/claude-code)"
expect accept "prose naming the trailer" "The Claude-Session: trailer is refused by the hook."
expect accept "an empty message" ""
expect reject "a session trailer" "$(printf 'fix: x\n\nClaude-Session: %s' "$session")"
expect reject "a session trailer, indented" "$(printf 'fix: x\n\n  Claude-Session: anything')"
expect reject "a session trailer, upper case" "$(printf 'fix: x\n\nCLAUDE-SESSION: anything')"
expect reject "a session URL mid-line" "$(printf 'docs: y\n\nSee %s for the run.' "$session")"
expect reject "a chat link" "$(printf 'docs: z\n\nFrom https://claude.ai/chat/0f1e2d3c-4b5a-6978-8091-a2b3c4d5e6f7')"

# The refusal names lines, never the link: CI logs are public.
err=$(printf 'fix: x\n\nClaude-Session: %s\n' "$session" | "$check" 2>&1 >/dev/null) || true
got=no; if [[ $err == *"line 3"* ]] && [[ $err != *"$id"* ]]; then got=yes; fi
result yes "the refusal gives the line and doesn't repeat the link" "$got"

# Matching is ASCII whatever the caller's locale: Turkish lowercases I to a dotless i.
if [ "$(LC_ALL=tr_TR.UTF-8 bash -c 'x=I; echo "${x,,}"' 2>/dev/null)" != i ]; then
  got=reject; if printf 'CLAUDE-SESSION: x\n' | LC_ALL=tr_TR.UTF-8 "$check" >/dev/null 2>&1; then got=accept; fi
  result reject "an upper-case trailer under LC_ALL=tr_TR.UTF-8" "$got"
else
  echo "skip - no tr_TR.UTF-8 locale, can't test locale-proof matching"
fi

# A file argument reads as standard input does; a file that can't be read is an error, not a pass.
printf 'Claude-Session: x\n' > "$tmp/file"
got=reject; if "$check" "$tmp/file" </dev/null >/dev/null 2>&1; then got=accept; fi
result reject "a session trailer in a file argument" "$got"
rc=0; "$check" "$tmp/missing" </dev/null >/dev/null 2>&1 || rc=$?
result 2 "a missing file argument exits" "$rc"

hook accept "a clean message" "$(printf 'feat(tools): add x\n\nCo-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>')"
hook reject "a session trailer" "$(printf 'feat(tools): add x\n\nClaude-Session: %s' "$session")"
hook reject "a bad subject, still" "added stuff"
# git commit -m and -F keep lines starting with #, so they are checked.
hook reject "a session link on a # line" "$(printf 'feat(tools): add x\n\n# see %s' "$session")"
# Below git's own scissors block (git commit -v: the line, then "Do not modify..."), nothing is
# part of the message, and the diff there may well show this rule's patterns.
hook accept "the diff below git's scissors block" "$(printf 'feat(tools): add x\n\n# ------------------------ >8 ------------------------\n# Do not modify or remove the line above.\n# Everything below it will be ignored.\n+Claude-Session: %s' "$session")"
hook reject "a scissors line without git's next line cuts nothing" "$(printf 'feat(tools): add x\n# ------------------------ >8 ------------------------\nClaude-Session: %s' "$session")"

# A real git commit, through core.hooksPath.
git init -q "$tmp/repo"
gitc() { git -C "$tmp/repo" -c core.hooksPath="$hooks" -c user.name=t -c user.email=t@example.invalid "$@"; }
got=reject; if gitc commit -q --allow-empty -m "feat(tools): x" -m "Claude-Session: $session" </dev/null >/dev/null 2>&1; then got=accept; fi
result reject "git commit with a session trailer" "$got"
got=reject; if gitc commit -q --allow-empty -m "feat(tools): x" -m "Co-Authored-By: Claude <noreply@anthropic.com>" </dev/null >/dev/null 2>&1; then got=accept; fi
result accept "git commit with only attribution" "$got"

# CI's PR check, against a small history: dev, one old commit that carries a link (as dev's
# squash commits before the rule do), then a PR branch.
prc() {  # WANT NAME BASE HEAD [ENV...]
  local want=$1 name=$2 base=$3 head=$4 got=reject
  shift 4
  if (cd "$tmp/pr" && env "$@" "$prcheck" "$base" "$head" dev) </dev/null >/dev/null 2>&1; then got=accept; fi
  result "$want" "pr: $name" "$got"
}
git init -q -b dev "$tmp/pr"
gp() { git -C "$tmp/pr" -c user.name=t -c user.email=t@example.invalid "$@"; }
gp commit -q --allow-empty -m "feat: root"
gp commit -q --allow-empty -m "feat: old" -m "Claude-Session: $session"
base=$(gp rev-parse dev)
gp switch -q -c clean && gp commit -q --allow-empty -m "fix: clean"
clean=$(gp rev-parse HEAD)
gp switch -q -c dirty dev && gp commit -q --allow-empty -m "fix: dirty" -m "Claude-Session: $session"
dirty=$(gp rev-parse HEAD)
prc accept "clean new commits; dev's older ones aren't checked" "$base" "$clean" TITLE="fix: clean" BODY="Closes #1"
prc reject "a new commit with a trailer" "$base" "$dirty" TITLE="fix: dirty" BODY=""
prc reject "a session link in the title" "$base" "$clean" TITLE="fix: $session" BODY=""
prc reject "a chat link in the description" "$base" "$clean" TITLE="fix: clean" BODY="see https://claude.ai/chat/0f1e"
prc reject "a head that doesn't exist" "$base" 0123456789abcdef0123456789abcdef01234567 TITLE="fix: x" BODY=""
err=$( (cd "$tmp/pr" && TITLE=t BODY='' "$prcheck" "$base" "$dirty" dev) 2>&1 >/dev/null) || true
got=no; if [[ $err == *"$(gp rev-parse --short "$dirty")"* ]] && [[ $err != *"$id"* ]]; then got=yes; fi
result yes "pr: a refusal names the commit, not the link" "$got"

[ "$failures" -eq 0 ] || { echo "$failures failure(s)" >&2; exit 1; }
echo "all session-link checks passed"
