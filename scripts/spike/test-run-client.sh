#!/usr/bin/env bash
# shellcheck disable=SC2030,SC2031 # evidence_case sets WORK and the rest only in its subshell, on purpose
# Tests for run-client.sh's own logic, with no sandbox, no Wine and no client: the verdict of a run,
# evidence collection and its failures, and the arguments that would blind the watchdog.
#   scripts/spike/test-run-client.sh
#
# Everything the tests touch lives under TESTROOT, which nothing reassigns. run-client.sh is sourced
# once, here: sourcing it again would reset WORK to the real run workspace. Stub functions are
# defined only inside subshells, and every test checks that WORK is still under TESTROOT.
set -uo pipefail
HERE=$(cd -- "$(dirname -- "$0")" && pwd -P)
# shellcheck source=run-client.sh
. "$HERE/run-client.sh"
set +e # run-client.sh sets -e; these tests check exit statuses themselves
TESTROOT=$(mktemp -d) || { echo "FATAL: mktemp failed" >&2; exit 2; }
readonly TESTROOT
[ -d "$TESTROOT" ] || { echo "FATAL: no test root" >&2; exit 2; }
trap 'rm -rf -- "$TESTROOT"' EXIT
# An empty workspace: anything that gets past the argument checks stops at "no workspace", so no test
# can ever start a run.
WORK=$TESTROOT/work REPO=$TESTROOT/repo PROTON=/nonexistent STUBBIN=/nonexistent WATCHDOG=/nonexistent
mkdir -p "$WORK"
fails=0
ok() { echo "ok   $1"; }
bad() { echo "FAIL $1"; fails=$((fails + 1)); }

# Stops every test at once if WORK has left TESTROOT.
guard() {
  case "$TESTROOT:$WORK:$REPO" in
    /?*:"$TESTROOT"/*:"$TESTROOT"/*) ;;
    *) echo "FATAL: WORK or REPO left the test root: $WORK $REPO" >&2; exit 2 ;;
  esac
}

# exits WANT WHAT -- COMMAND...: COMMAND, in a subshell, exits with status WANT.
exits() {
  local want=$1 what=$2 got
  shift 3
  guard
  ("$@") > /dev/null 2>&1
  got=$?
  if [ "$got" = "$want" ]; then ok "$what"; else bad "$what: exit $got, want $want"; fi
}

# refuses WHAT PATTERN -- ARGS...: cmd_run ARGS exits 1 with PATTERN in its message.
refuses() {
  local what=$1 pattern=$2 err rc
  shift 3
  guard
  err=$( (cmd_run t "$@") 2>&1 > /dev/null)
  rc=$?
  if [ "$rc" = 1 ] && grep -q -- "$pattern" <<< "$err"; then ok "$what"; else bad "$what: exit $rc: $err"; fi
}

# with_failing_collect COMMAND...: COMMAND with collect_evidence failing. Only ever run inside the
# subshell that `exits` makes, so the stub never outlives it.
with_failing_collect() {
  # shellcheck disable=SC2317,SC2329 # called by finish_run, which "$@" runs
  collect_evidence() { return 7; }
  "$@"
}

# evidence_case EXIT: the real collect_evidence over a fake run whose watchdog collect exits EXIT.
evidence_case() {
  local c=$TESTROOT/case
  guard
  rm -rf -- "$c"
  mkdir -p "$c/client/bin64" "$c/client/settings" "$c/io" "$c/out" "$c/compat/pfx" "$c/home"
  echo log > "$c/client/bin64/gujianol.log"
  touch "$c/out/.start"
  printf 'import sys\nsys.exit(%s)\n' "$1" > "$c/collect.py"
  (
    WORK=$c IO=$c/io OUT=$c/out STUB=$c/stub PFX=$c/compat/pfx WATCHDOG=$c/collect.py
    collect_evidence
  ) > /dev/null 2>&1
}

OUT=$WORK/out run=t
exits 3 "a gate event is exit 3" -- verdict 3 0 1 "$OUT"
exits 4 "a watchdog failure is exit 4" -- verdict 4 124 1 "$OUT"
exits 5 "a watchdog that never armed is exit 5" -- verdict 0 98 0 "$OUT"
exits 6 "a sandbox that ended before the client ran is exit 6" -- verdict 0 2 0 "$OUT"
exits 7 "a stub that wasn't running when the client was due is exit 7" -- verdict 0 97 0 "$OUT"
exits 0 "a finished run is exit 0, whatever the client's own status" -- verdict 0 124 1 "$OUT"

exits 3 "a failed evidence collection leaves a gate event exit 3" -- with_failing_collect finish_run 3 0 1
exits 0 "a failed evidence collection leaves a clean run exit 0" -- with_failing_collect finish_run 0 124 1

evidence_case 1
rc=$?
if [ "$rc" != 0 ] && [ -f "$TESTROOT/case/client/bin64/gujianol.log" ] && [ -d "$TESTROOT/case/client/settings" ]; then
  ok "a failed collect keeps what it couldn't copy, and is reported"
else
  bad "a failed collect keeps what it couldn't copy, and is reported: exit $rc"
fi
evidence_case 0
rc=$?
if [ "$rc" = 0 ] && [ ! -e "$TESTROOT/case/client/bin64/gujianol.log" ] && [ ! -e "$TESTROOT/case/client/settings" ]; then
  ok "a successful collect clears the copy for the next run"
else
  bad "a successful collect clears the copy for the next run: exit $rc"
fi

mkdir -p "$TESTROOT/work/client/bin64" "$TESTROOT/work/client/settings" "$TESTROOT/work/compat/pfx"
touch "$TESTROOT/work/compat/pfx/system.reg"
refuses "a run refuses a client copy with another run's leftovers" "leftover" --
rm -rf -- "$TESTROOT/work/client" "$TESTROOT/work/compat"

refuses "--env can't redirect the log the watchdog reads" "PROTON_LOG_DIR" -- --env PROTON_LOG_DIR=/tmp/x
refuses "--env can't rename the log the watchdog reads" "SteamGameId" -- --env SteamGameId=1
refuses "--env can't replace the channels" "WINEDEBUG" -- --env WINEDEBUG=-all
refuses "--env can't move the prefix the watchdog scans" "STEAM_COMPAT_DATA_PATH" -- --env STEAM_COMPAT_DATA_PATH=/tmp/x
refuses "--env takes VAR=VALUE" "VAR=VALUE" -- --env NOVALUE
refuses "--ports takes numbers" "--ports" -- --ports '7800;x'
refuses "--stub-arg takes one word" "--stub-arg" -- --stub-arg 'a b'
refuses "--stub-arg refuses shell characters" "--stub-arg" -- --stub-arg '--x;rm'
refuses "--ports can't be empty" "--ports" -- --ports ''
refuses "--click takes T:X,Y in whole numbers" "--click" -- --click 5:abc
refuses "valid --ports, --env and --click pass their checks" "no workspace" -- --ports '7800 7801' --env FOO=bar --click 30:640,360

# record_inputs writes what defines a run, the stub's arguments included.
if (
  guard
  OUT=$TESTROOT/inputs run=t ports=7800 tmo=9 display=99 channels=-all hosts=$TESTROOT/hosts
  WRAP=$TESTROOT/hosts WATCHDOG=$TESTROOT/hosts PROTON=/nonexistent
  envs=() client_files=() clicks=() stub_args=(--speak --auth-reply=reject)
  mkdir -p "$OUT" && echo "127.0.0.1 localhost" > "$hosts"
  record_inputs -x 1 > /dev/null 2>&1
  grep -qx 'stub args: --speak --auth-reply=reject' "$OUT/inputs.txt"
); then
  ok "inputs.txt records the stub's arguments"
else
  bad "inputs.txt records the stub's arguments"
fi

# --- M3: fenji-server, the launcher's ticket, key presses (M3 plan Task 8)
exits 7 "a server that never got ready is exit 7" -- verdict 0 96 0 "$OUT"
exits 7 "a launcher that failed is exit 7" -- verdict 0 95 0 "$OUT"

# usage WHAT -- ARGS...: cmd_run ARGS exits 2 (a usage error).
usage_error() {
  local what=$1 rc
  shift 2
  guard
  (cmd_run t "$@") > /dev/null 2>&1
  rc=$?
  if [ "$rc" = 2 ]; then ok "$what"; else bad "$what: exit $rc, want 2"; fi
}
printf '[server]\nname = "Fenji"\n' > "$TESTROOT/fenji.toml"
usage_error "--launch needs --server" -- --launch "--server http://127.0.0.1:7801 --username tester"
usage_error "--launch-password needs --launch" -- --server "$TESTROOT/fenji.toml" --launch-password "$TESTROOT/fenji.toml"
usage_error "--server needs a readable file" -- --server "$TESTROOT/missing.toml"
printf 'not = [toml\n' > "$TESTROOT/not.toml"
usage_error "--server takes a config the server can read" -- --server "$TESTROOT/not.toml"
mkdir -p "$TESTROOT/elsewhere"
usage_error "--server-data must be under re/runs/" -- --server "$TESTROOT/fenji.toml" --server-data "$TESTROOT/elsewhere"
usage_error "--server-data needs --server" -- --server-data "$WORK"
usage_error "--server and --ports don't mix" -- --server "$TESTROOT/fenji.toml" --ports 7800
usage_error "--key needs a key name" -- --key 5:
usage_error "--key needs a time" -- --key Return
usage_error "--type needs a time" -- --type x
usage_error "--type takes printable ASCII" -- --type "5:$(printf 'a\tb')"
usage_error "--launch takes words of [A-Za-z0-9=_.:/@-]" -- --server "$TESTROOT/fenji.toml" --launch '--username x;echo'
mkdir -p "$WORK/data"
printf '[server]\nname = "Fenji"\n[store]\npath = "/somewhere/else.db"\n' > "$TESTROOT/away.toml"
usage_error "--server-data needs the config's store path inside it" -- --server "$TESTROOT/away.toml" --server-data "$WORK/data"
printf '[server]\nname = "Fenji"\n[store]\npath = "%s/data/fenji.db"\n' "$WORK" > "$TESTROOT/data.toml"
refuses "valid --server, --server-data, --launch, --key and --type pass their checks" "no workspace" -- \
  --server "$TESTROOT/data.toml" --server-data "$WORK/data" --launch "--server http://127.0.0.1:7801 --username tester" \
  --key 40:Return --key 41:space --type "45:Lin Two" --click 30:640,360

if [ "$(redact_gj_ticket <<< 'gj_ticket=fenji1-abc gj_gates=1.2.3.4:7800')" = 'gj_ticket=<redacted> gj_gates=1.2.3.4:7800' ]; then
  ok "redact_gj_ticket hides only the ticket"
else
  bad "redact_gj_ticket hides only the ticket: $(redact_gj_ticket <<< 'gj_ticket=fenji1-abc gj_gates=1.2.3.4:7800')"
fi
got=$(printf 'gj_uinfo=1:90:Fenji:0:tester:5\nLD_PRELOAD=/x.so\nnoise\ngj_gates=127.0.0.1:7800\ngj_ticket=fenji1-ab\n' | parse_launch_output)
want=$(printf 'gj_uinfo=1:90:Fenji:0:tester:5\ngj_gates=127.0.0.1:7800\ngj_ticket=fenji1-ab')
if [ "$got" = "$want" ]; then ok "parse_launch_output keeps only the gj_* pairs"; else bad "parse_launch_output keeps only the gj_* pairs: $got"; fi
if printf 'gj_ticket=fenji1\000ab\n' | parse_launch_output > /dev/null 2>&1; then
  bad "parse_launch_output refuses a NUL in a value"
else
  ok "parse_launch_output refuses a NUL in a value"
fi
if printf 'gj_ticket=fenji1\rab\n' | parse_launch_output > /dev/null 2>&1; then
  bad "parse_launch_output refuses a control character in a value"
else
  ok "parse_launch_output refuses a control character in a value"
fi
if printf 'warning: nothing\n' | parse_launch_output > /dev/null 2>&1; then
  bad "parse_launch_output refuses output with no pairs"
else
  ok "parse_launch_output refuses output with no pairs"
fi

# The launcher's password file is never copied, recorded or named in inputs.txt.
if (
  guard
  OUT=$TESTROOT/inputs2 run=t ports=7800 tmo=9 display=99 channels=-all hosts=$TESTROOT/hosts
  WRAP=$TESTROOT/hosts WATCHDOG=$TESTROOT/hosts PROTON=/nonexistent
  envs=() client_files=() clicks=() stub_args=() keys=(40:Return) types=("45:Lin")
  server_cfg=$TESTROOT/data.toml server_data=$WORK/data launch="--username tester"
  launch_password=$TESTROOT/secret-password-file
  echo 'hunter2-not-recorded' > "$launch_password"
  mkdir -p "$OUT" && echo "127.0.0.1 localhost" > "$hosts"
  record_inputs > /dev/null 2>&1
  ! grep -q -e hunter2 -e secret-password-file "$OUT/inputs.txt" &&
    grep -qx 'launch args: --username tester' "$OUT/inputs.txt" &&
    grep -qx 'keys (T:KEY): 40:Return' "$OUT/inputs.txt" &&
    grep -q 'server config:' "$OUT/inputs.txt"
); then
  ok "inputs.txt records the launch but never its password"
else
  bad "inputs.txt records the launch but never its password"
fi

case "$(watch_channels -all)" in
  -all,*+process*+loaddll*+resource*+service*err+all*) ok "the watchdog's channels follow any --channels" ;;
  *) bad "the watchdog's channels follow any --channels: $(watch_channels -all)" ;;
esac

guard
[ "$fails" = 0 ] || { echo "$fails failed"; exit 1; }
echo "all passed"
