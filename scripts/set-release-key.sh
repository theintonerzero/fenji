#!/usr/bin/env bash
# Puts Fenji's release key in, from KEY.pub (its public half, as ssh-keygen wrote it), as its
# allowed-signers line (the hosting spec's Amendment D, HD1, HD5, HD7):
#   scripts/set-release-key.sh KEY.pub         the key: the only line in deploy/allowed_signers,
#                                              which the sums job checks the secret against, and
#                                              the line the update script and section 5 of
#                                              docs/hosting.md trust;
#   scripts/set-release-key.sh --next KEY.pub  a new key, for the one release that brings it: the
#                                              update script and the guide trust it, while
#                                              deploy/allowed_signers keeps the old key's line
#                                              beside it, so that release is still signed with the
#                                              old key. After it, the secret becomes the new key,
#                                              and KEY.pub goes in again without --next.
# scripts/test-update-server.sh pins that the three agree. FENJI_REPO names another tree to write in
# (for its test).
set -euo pipefail
export LC_ALL=C
next=no
if [ "${1-}" = --next ]; then
  next=yes
  shift
fi
[ $# -eq 1 ] || { echo "usage: $0 [--next] KEY.pub" >&2; exit 2; }
root=${FENJI_REPO:-$(cd "$(dirname "$0")/.." && pwd)}
type='' key=''
read -r type key _ < "$1" || true
# The line goes into a shell script and a guide, so only these characters ever reach them; and only
# a key ssh-keygen itself reads.
if [ "$type" != ssh-ed25519 ] || ! [[ $key =~ ^AAAAC3NzaC1lZDI1NTE5[A-Za-z0-9+/]+=*$ ]] ||
  ! printf '%s %s\n' "$type" "$key" | ssh-keygen -l -f - > /dev/null 2>&1; then
  echo "$1 isn't an ssh-ed25519 public key: make one with ssh-keygen -t ed25519" >&2
  exit 1
fi
line="fenji-release namespaces=\"fenji-release\" $type $key"

# rewrite FILE AWK-PROGRAM: FILE through the program, with the line as `line`, in place, its mode kept.
rewrite() {
  local file=$1 out
  out=$(awk -v line="$line" "$2" "$file")
  printf '%s\n' "$out" > "$file"
}
allowed=$root/deploy/allowed_signers
script=$root/deploy/start/update-server.sh
guide=$root/docs/hosting.md
if [ "$next" = yes ]; then
  # The old key's line stays, the new one joins it, once.
  rewrite "$allowed" '$0 != line { print } END { print line }'
else
  rewrite "$allowed" '!/^fenji-release / { print } END { print line }'
fi
rewrite "$script" '/^RELEASE_KEY=/ { print "RELEASE_KEY='"'"'" line "'"'"'"; next } { print }'
rewrite "$guide" 'after { print line; after = 0; next } { print } /^cat > allowed_signers <<.END.$/ { after = 1 }'

for f in "$allowed" "$script" "$guide"; do
  grep -qF -- "$line" "$f" || { echo "the release key's line didn't go into $f" >&2; exit 1; }
done
if [ "$next" = yes ]; then
  echo "The update script and docs/hosting.md trust the next key; deploy/allowed_signers keeps the"
  echo "current key's line beside it, so the next release is still signed with the current key:"
else
  echo "The release key's line is in deploy/allowed_signers, the update script and docs/hosting.md:"
fi
echo "$line"
