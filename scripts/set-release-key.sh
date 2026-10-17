#!/usr/bin/env bash
# Puts Fenji's release key in, from KEY.pub (its public half, as ssh-keygen wrote it), as its
# allowed-signers line, in the three places that carry it (the hosting spec's Amendment D, HD1, HD5,
# HD7): deploy/allowed_signers, deploy/start/update-server.sh's RELEASE_KEY, and the by-hand check in
# section 5 of docs/hosting.md. Running it again replaces the line; scripts/test-update-server.sh pins
# that the three agree. FENJI_REPO names another tree to write in (for its test).
#   scripts/set-release-key.sh KEY.pub
set -euo pipefail
export LC_ALL=C
[ $# -eq 1 ] || { echo "usage: $0 KEY.pub" >&2; exit 2; }
root=${FENJI_REPO:-$(cd "$(dirname "$0")/.." && pwd)}
type= key=
read -r type key _ < "$1" || true
if [ "$type" != ssh-ed25519 ] || ! [[ $key =~ ^AAAAC3NzaC1lZDI1NTE5[A-Za-z0-9+/]+=*$ ]]; then
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
rewrite "$allowed" '!/^fenji-release / { print } END { print line }'
rewrite "$script" '/^RELEASE_KEY=/ { print "RELEASE_KEY='"'"'" line "'"'"'"; next } { print }'
rewrite "$guide" 'after { print line; after = 0; next } { print } /^cat > allowed_signers <<.END.$/ { after = 1 }'

for f in "$allowed" "$script" "$guide"; do
  grep -qF -- "$line" "$f" || { echo "the release key's line didn't go into $f" >&2; exit 1; }
done
echo "The release key's line is in deploy/allowed_signers, the update script and docs/hosting.md:"
echo "$line"
