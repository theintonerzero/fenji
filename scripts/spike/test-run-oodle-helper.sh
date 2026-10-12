#!/usr/bin/env bash
# Tests for run-oodle-helper.sh's spool guards, with no sandbox, no Wine and no client: they keep
# the client's decoded data inside the git-ignored re/ (ground rule 3) and away from the Wine prefix.
#   scripts/spike/test-run-oodle-helper.sh
#
# Every guard runs before the script creates anything. A refused spool dies at its guard; an accepted
# one dies at the next check, because FENJI_GE_PROTON points nowhere, so no case writes a file.
set -uo pipefail
HERE=$(cd -- "$(dirname -- "$0")" && pwd -P)
REPO=$(cd -- "$HERE/../.." && pwd -P)
RUN=$HERE/run-oodle-helper.sh
export FENJI_GE_PROTON=/nonexistent
fails=0
ok() { echo "ok   $1"; }
bad() { echo "FAIL $1"; fails=$((fails + 1)); }

# expect WANT_EXIT PATTERN WHAT -- ARGS...: run-oodle-helper.sh ARGS, run from the repo root,
# exits WANT_EXIT with PATTERN in its message.
expect() {
  local want=$1 pattern=$2 what=$3 err rc
  shift 4
  err=$(cd "$REPO" && "$RUN" "$@" 2>&1 > /dev/null)
  rc=$?
  if [ "$rc" = "$want" ] && grep -q -- "$pattern" <<< "$err"; then ok "$what"; else bad "$what: exit $rc: $err"; fi
}

expect 1 "must be inside re/" "re/ itself is refused" -- --spool re
expect 1 "must be inside re/" "a spool outside the repo is refused" -- --spool /tmp/fenji-spool
expect 1 "must be inside re/" "a spool that climbs out of re/ is refused" -- --spool re/../docs/spool
expect 1 "inside the Wine prefix" "the Wine prefix is refused" -- --spool re/oodle-prefix
expect 1 "inside the Wine prefix" "a spool inside the Wine prefix is refused" -- --spool re/oodle-prefix/spool
expect 1 "GE-Proton not found" "the default spool passes the guards" --
expect 1 "GE-Proton not found" "a spool elsewhere inside re/ passes the guards" -- --spool re/m2/spool
expect 2 "usage" "an unknown argument is a usage error" -- --nope

# A link inside re/ that leads out of it: the guard compares the resolved path.
link="$REPO/re/.test-oodle-helper-$$"
mkdir -p "$REPO/re"
ln -s /tmp "$link"
expect 1 "must be inside re/" "a spool through a link out of re/ is refused" -- --spool "re/${link##*/}/spool"
rm -f -- "$link"

[ "$fails" = 0 ] || { echo "$fails failed"; exit 1; }
echo "all passed"
