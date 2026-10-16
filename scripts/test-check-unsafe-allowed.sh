#!/usr/bin/env bash
# Tests for scripts/check-unsafe-allowed.sh. Runs anywhere bash and git run (macOS bash 3.2,
# Fedora, CI).
#
# Each case builds a scratch repo holding one Rust file and runs the check on it. A refusal has to
# come from the check itself: a non-zero exit that names the file.
set -euo pipefail

check="$(cd "$(dirname "$0")" && pwd)/check-unsafe-allowed.sh"
passed=0
failed=0
ok()   { echo "ok - $1"; passed=$((passed + 1)); }
fail() { echo "FAIL - $1" >&2; failed=$((failed + 1)); }

root=""
trap 'rm -rf "$root"' EXIT
root=$(mktemp -d)

# Keep the developer's git setup and any hook environment out of the scratch repos.
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE
export HOME="$root/home" XDG_CONFIG_HOME="$root/home/.config" GIT_CONFIG_NOSYSTEM=1
mkdir -p "$HOME"

# repo_with PATH TEXT: a fresh repo holding PATH, tracked, its lines TEXT (printf's %b escapes);
# prints its directory.
repo_with() {
  local d
  d=$(mktemp -d "$root/r.XXXXXX")
  git -c init.defaultBranch=main -C "$d" init -q
  mkdir -p "$d/$(dirname "$1")"
  printf '%b\n' "$2" > "$d/$1"
  git -C "$d" add -- "$1"
  echo "$d"
}

# judged DIR NAMED: what the check makes of the repo at DIR: accept, reject (naming NAMED), or
# what else it did.
judged() {
  if "$check" "$1" > "$root/out" 2>&1; then
    echo accept
  elif grep -qF -- "unsafe code allowed: $2" "$root/out"; then
    echo reject
  else
    echo "a failure that doesn't name $2: $(cat "$root/out")"
  fi
}

# expect WANT NAME PATH TEXT: the check, on a repo holding PATH with TEXT, gives WANT.
expect() {
  local got
  got=$(judged "$(repo_with "$3" "$4")" "$3")
  if [ "$got" = "$1" ]; then ok "$2"; else fail "$2: expected $1, got $got"; fi
}

expect accept "no unsafe code" crates/x/src/lib.rs 'fn main() {}'
expect accept "the server's unix.rs" crates/fenji-server/src/unix.rs '#![allow(unsafe_code)]'
expect accept "the Oodle helper's dll.rs" tools/oodle-helper/src/dll.rs '#![allow(unsafe_code)]'
expect accept "deny" crates/x/src/lib.rs '#![deny(unsafe_code)]'
expect accept "another lint allowed, unsafe_code named after it" crates/x/src/lib.rs \
  '#[allow(dead_code)] // unsafe_code stays denied'
expect reject "another of the server's files" crates/fenji-server/src/lock.rs '#![allow(unsafe_code)]'
expect reject "a unix.rs elsewhere" crates/fenji-console/src/unix.rs '#![allow(unsafe_code)]'
expect reject "an item's allow" crates/x/src/lib.rs 'fn a() {}\n#[allow(unsafe_code)]\nfn f() {}'
expect reject "among other lints" crates/x/src/lib.rs '#[allow(dead_code, unsafe_code)]'
expect reject "spaced" crates/x/src/lib.rs '#[allow ( unsafe_code )]'
expect reject "over several lines" crates/x/src/lib.rs '#[allow(\n    clippy::all,\n    unsafe_code,\n)]'
expect reject "expect" crates/x/src/lib.rs '#[expect(unsafe_code, reason = "x")]'
expect reject "warn" crates/x/src/lib.rs '#![warn(unsafe_code)]'
expect reject "under cfg_attr" crates/x/src/lib.rs '#![cfg_attr(unix, allow(unsafe_code))]'

# Untracked files count, unless git ignores them (target/).
d=$(repo_with crates/x/src/lib.rs 'fn main() {}')
printf '#![allow(unsafe_code)]\n' > "$d/new.rs"
got=$(judged "$d" new.rs)
if [ "$got" = reject ]; then ok "an untracked file"; else fail "an untracked file: got $got"; fi
rm "$d/new.rs"
printf 'target/\n' > "$d/.gitignore"
mkdir -p "$d/target"
printf '#![allow(unsafe_code)]\n' > "$d/target/made.rs"
got=$(judged "$d" target/made.rs)
if [ "$got" = accept ]; then ok "an ignored file"; else fail "an ignored file: got $got"; fi

# Run from elsewhere, on a folder inside the repository.
d=$(repo_with crates/x/src/lib.rs '#![allow(unsafe_code)]')
got=$(cd "$root" && judged "$d/crates" crates/x/src/lib.rs)
if [ "$got" = reject ]; then ok "a folder inside the repository"; else fail "a folder inside: got $got"; fi

echo "$passed passed, $failed failed"
[ "$failed" -eq 0 ]
