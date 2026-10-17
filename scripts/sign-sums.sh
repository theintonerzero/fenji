#!/usr/bin/env bash
# Writes SHA256SUMS over every file in DIR, signs it with the release's SSH key as
# SHA256SUMS.sig, and checks that signature against ALLOWED, the repository's allowed-signers line
# (the hosting spec's Amendment D, HD2). The release workflow's sums job runs it, behind the owner's
# approval: a secret that isn't that key's private half, or a repository that holds no key's line
# yet (HD5), fails the release here, before anything is published.
#   SSH_SIGNING_PRIVATE_KEY=<the private key's text> scripts/sign-sums.sh DIR ALLOWED
set -euo pipefail
export LC_ALL=C
umask 077

fail() {
  echo "::error::$1" >&2
  [ -z "${dir:-}" ] || rm -f -- "$dir/SHA256SUMS.sig"
  exit 1
}

[ $# -eq 2 ] || { echo "usage: SSH_SIGNING_PRIVATE_KEY=... $0 DIR ALLOWED" >&2; exit 2; }
# Both as full paths, so that fail's clean-up and the check find them from inside DIR.
dir=$(cd -- "$1" && pwd) || fail "there's no folder $1 to sum"
[ -f "$2" ] || fail "there's no $2 to check the signature against"
allowed=$(cd -- "$(dirname -- "$2")" && pwd)/$(basename -- "$2")
[ -n "${SSH_SIGNING_PRIVATE_KEY:-}" ] ||
  fail "the release environment has no SSH_SIGNING_PRIVATE_KEY, so the sums can't be signed"
grep -q '^fenji-release ' "$allowed" ||
  fail "$2 holds no release key's line: the owner makes the key before the release is tagged (HD5)"

cd -- "$dir"
# A run again finds its own sums: they're made anew, and ssh-keygen would ask before replacing one.
rm -f -- SHA256SUMS SHA256SUMS.sig
files=(*)
sha256sum -- "${files[@]}" > SHA256SUMS
key=$(mktemp "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/release-key.XXXXXX")
trap 'rm -f -- "$key"' EXIT
printf '%s\n' "$SSH_SIGNING_PRIVATE_KEY" > "$key"
unset SSH_SIGNING_PRIVATE_KEY
# Only the key given, never an agent's, and nothing to answer: no prompt can wait on anyone.
env -u SSH_AUTH_SOCK ssh-keygen -q -Y sign -f "$key" -n fenji-release SHA256SUMS < /dev/null ||
  fail "ssh-keygen couldn't sign SHA256SUMS with SSH_SIGNING_PRIVATE_KEY"
env -u SSH_AUTH_SOCK ssh-keygen -q -Y verify -f "$allowed" -I fenji-release -n fenji-release \
  -s SHA256SUMS.sig < SHA256SUMS > /dev/null ||
  fail "SHA256SUMS.sig doesn't verify against $2: SSH_SIGNING_PRIVATE_KEY isn't the release key's private half"
cat SHA256SUMS
echo "SHA256SUMS is signed with the release key."
