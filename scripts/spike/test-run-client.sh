#!/usr/bin/env bash
# Tests for run-client.sh's own logic, with no sandbox, no Wine and no client: the verdict of a run,
# evidence failures that must not change it, and the arguments that would blind the watchdog.
#   scripts/spike/test-run-client.sh
set -uo pipefail
HERE=$(cd -- "$(dirname -- "$0")" && pwd -P)
# shellcheck source=run-client.sh
. "$HERE/run-client.sh"
set +e # run-client.sh sets -e; these tests check exit statuses themselves
# An empty workspace: anything that gets past the argument checks stops at "no workspace", so no test
# can ever start a run.
WORK=$(mktemp -d)
PROTON=/nonexistent
trap 'rm -rf -- "$WORK"' EXIT
fails=0
ok() { echo "ok   $1"; }
bad() { echo "FAIL $1"; fails=$((fails + 1)); }

# exits WANT WHAT -- COMMAND...: COMMAND, in a subshell, exits with status WANT.
exits() {
  local want=$1 what=$2 got
  shift 3
  ("$@") > /dev/null 2>&1
  got=$?
  if [ "$got" = "$want" ]; then ok "$what"; else bad "$what: exit $got, want $want"; fi
}

# refuses WHAT PATTERN -- ARGS...: cmd_run ARGS exits 1 with PATTERN in its message.
refuses() {
  local what=$1 pattern=$2 err rc
  shift 3
  err=$( (cmd_run t "$@") 2>&1 > /dev/null)
  rc=$?
  if [ "$rc" = 1 ] && grep -q -- "$pattern" <<< "$err"; then ok "$what"; else bad "$what: exit $rc: $err"; fi
}

OUT=$WORK/out run=t
exits 3 "a gate event is exit 3" -- verdict 3 0 1 "$OUT"
exits 4 "a watchdog failure is exit 4" -- verdict 4 124 1 "$OUT"
exits 5 "a watchdog that never armed is exit 5" -- verdict 0 98 0 "$OUT"
exits 6 "a sandbox that ended before the client ran is exit 6" -- verdict 0 2 0 "$OUT"
exits 0 "a finished run is exit 0, whatever the client's own status" -- verdict 0 124 1 "$OUT"

collect_evidence() { return 7; }
exits 3 "a failed evidence collection leaves a gate event exit 3" -- finish_run 3 0 1
exits 0 "a failed evidence collection leaves a clean run exit 0" -- finish_run 0 124 1

refuses "--env can't redirect the log the watchdog reads" "PROTON_LOG_DIR" -- --env PROTON_LOG_DIR=/tmp/x
refuses "--env can't replace the channels" "WINEDEBUG" -- --env WINEDEBUG=-all
refuses "--env can't move the prefix the watchdog scans" "STEAM_COMPAT_DATA_PATH" -- --env STEAM_COMPAT_DATA_PATH=/tmp/x
refuses "--env takes VAR=VALUE" "VAR=VALUE" -- --env NOVALUE
refuses "--ports takes numbers" "--ports" -- --ports '7800;x'
refuses "--ports can't be empty" "--ports" -- --ports ''

refuses "valid --ports and --env pass their checks" "no workspace" -- --ports '7800 7801' --env FOO=bar

case "$(watch_channels -all)" in
  -all,*+process*+loaddll*+resource*+service*err+all*) ok "the watchdog's channels follow any --channels" ;;
  *) bad "the watchdog's channels follow any --channels: $(watch_channels -all)" ;;
esac

[ "$fails" = 0 ] || { echo "$fails failed"; exit 1; }
echo "all passed"
