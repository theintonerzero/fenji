#!/usr/bin/env bash
# Tests for scripts/check-commit-subject.sh. Runs anywhere bash runs (macOS, Fedora, CI).
set -euo pipefail

check="$(cd "$(dirname "$0")" && pwd)/check-commit-subject.sh"
failures=0

expect() {
  local want=$1 subject=$2 got
  if "$check" "$subject" >/dev/null 2>&1; then got=accept; else got=reject; fi
  if [ "$got" = "$want" ]; then
    echo "ok - $want: $subject"
  else
    echo "FAIL - expected $want, got $got: $subject" >&2
    failures=$((failures + 1))
  fi
}

expect accept "feat(proto): add handshake state machine"
expect accept "docs: add design spec"
expect accept "feat!: change config format"
expect accept "fix(tools/stub): log zero-byte connections"
expect accept "chore(release): v0.1.0"
expect accept "Merge branch 'dev' into main"
expect reject "added stuff"
expect reject "Feat: capitalised type"
expect reject "feat:missing space"
expect reject "feat:  double space"
expect reject "feat(Proto): uppercase scope"
expect reject "feature: unknown type"
expect reject "feat: $(printf 'x%.0s' $(seq 1 100))"
expect reject ""

[ "$failures" -eq 0 ] || { echo "$failures failure(s)" >&2; exit 1; }
echo "all commit-subject checks passed"
