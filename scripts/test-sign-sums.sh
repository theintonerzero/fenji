#!/usr/bin/env bash
# Tests scripts/sign-sums.sh (the hosting spec's Amendment D, HD2) with keys made for the run: it
# writes SHA256SUMS over a folder's files and a signature that verifies against the key's
# allowed-signers line; without the key, with another key, or against a file that holds no key's
# line, it fails, says why, and leaves no signature.
#   scripts/test-sign-sums.sh
set -uo pipefail
export LC_ALL=C
REPO=$(cd "$(dirname "$0")/.." && pwd)
T=$(mktemp -d "${TMPDIR:-/tmp}/test-sign-sums.XXXXXX")
trap 'rm -rf -- "$T"' EXIT
failed=0
ok() { echo "ok   $1"; }
bad() { echo "FAIL $1"; failed=1; }
# holds WHAT COMMAND...: COMMAND succeeds.
holds() { local what=$1; shift; if "$@"; then ok "$what"; else bad "$what"; fi; }

ssh-keygen -q -t ed25519 -N '' -C release -f "$T/key"
ssh-keygen -q -t ed25519 -N '' -C other -f "$T/other"
printf 'fenji-release namespaces="fenji-release" %s\n' "$(cut -d' ' -f1,2 "$T/key.pub")" > "$T/allowed"
printf '# no key yet\n' > "$T/none"

# sign DIR ALLOWED [KEY]: the script over a fresh DIR holding two files, with KEY's private half as
# the secret (none without KEY); its output in $T/out, its exit code returned.
sign() {
  local dir=$1 allowed=$2 key=${3:-}
  rm -rf -- "$dir"
  mkdir -p "$dir"
  printf 'an archive\n' > "$dir/fenji-server-1.2.3-x86_64-unknown-linux-musl.tar.gz"
  printf 'a launcher\n' > "$dir/Fenji_1.2.3_amd64.AppImage"
  if [ -n "$key" ]; then
    SSH_SIGNING_PRIVATE_KEY=$(cat "$key") "$REPO/scripts/sign-sums.sh" "$dir" "$allowed" > "$T/out" 2>&1
  else
    env -u SSH_SIGNING_PRIVATE_KEY "$REPO/scripts/sign-sums.sh" "$dir" "$allowed" > "$T/out" 2>&1
  fi
}

holds "the release key signs" sign "$T/good" "$T/allowed" "$T/key"
holds "SHA256SUMS holds every file's sum" diff <(cd "$T/good" && sha256sum -- Fenji_1.2.3_amd64.AppImage \
  fenji-server-1.2.3-x86_64-unknown-linux-musl.tar.gz) "$T/good/SHA256SUMS"
holds "its signature verifies against the line" ssh-keygen -q -Y verify -f "$T/allowed" -I fenji-release \
  -n fenji-release -s "$T/good/SHA256SUMS.sig" < "$T/good/SHA256SUMS"
holds "a relative line's path is read from where the script started" \
  bash -c "cd '$T' && SSH_SIGNING_PRIVATE_KEY=\$(cat key) '$REPO/scripts/sign-sums.sh' good allowed > /dev/null 2>&1"

refused() {
  local what=$1 words=$2
  shift 2
  if sign "$@"; then
    bad "$what: it succeeded"
  elif ! grep -qF -- "$words" "$T/out"; then
    bad "$what: it didn't say \"$words\": $(cat "$T/out")"
  elif [ -e "$1/SHA256SUMS.sig" ]; then
    bad "$what: a signature stayed"
  else
    ok "$what"
  fi
}
refused "another key is refused" "isn't the release key's private half" "$T/wrong" "$T/allowed" "$T/other"
refused "no secret is refused" "no SSH_SIGNING_PRIVATE_KEY" "$T/nokey" "$T/allowed"
refused "a file with no key's line is refused" "holds no release key's line" "$T/noline" "$T/none" "$T/key"

if [ "$failed" = 0 ]; then echo "all passed"; else exit 1; fi
