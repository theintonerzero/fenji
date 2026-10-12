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
exits 0 "a client that ran and exited 95 itself finished" -- verdict 0 95 1 "$OUT"

# usage_error WHAT PATTERN -- ARGS...: cmd_run ARGS exits 2 with PATTERN in its message.
usage_error() {
  local what=$1 pattern=$2 err rc
  shift 3
  guard
  err=$( (cmd_run t "$@") 2>&1 > /dev/null)
  rc=$?
  if [ "$rc" = 2 ] && grep -q -- "$pattern" <<< "$err"; then ok "$what"; else bad "$what: exit $rc: $err"; fi
}
M3=$TESTROOT/m3
mkdir -p "$M3/elsewhere" "$WORK/data"
printf 'secret-for-tests\n' > "$M3/password"
printf '[server]\nname = "Fenji"\n' > "$M3/fenji.toml"
printf 'not = [toml\n' > "$M3/not.toml"
printf '[server]\nname = "Fenji"\n[store]\npath = "%s/data/fenji.db"\n' "$WORK" > "$M3/data.toml"
printf '[server]\nname = "Fenji"\n[store]\npath = "%s/elsewhere/fenji.db"\n' "$M3" > "$M3/away.toml"
printf '[server]\nname = "Fenji"\n[store]\npath = "fenji.db"\n' > "$M3/relative.toml"
printf '[server]\nname = "Fenji"\n[store]\npath = "../../../compat/pfx/fenji.db"\n' > "$M3/escape.toml"
printf '[server]\nname = "Fenji"\n[store]\npath = "/tmp/fenji.db"\n' > "$M3/absolute.toml"
printf '[server]\nname = "Fenji"\n[store]\npath = "fenji\\n.db"\n' > "$M3/control.toml"
printf '[server]\nname = "Fenji"\nlogin_bind = "0.0.0.0:7800"\n' > "$M3/wide.toml"
printf '[server]\nname = "Fenji"\napi_bind = "127.0.0.1:0"\n' > "$M3/port0.toml"
L=(--launch "--server http://127.0.0.1:7801 --username tester")
usage_error "--launch needs --server" "needs --server" -- "${L[@]}"
usage_error "--launch needs --launch-password" "needs --launch-password" -- --server "$M3/relative.toml" "${L[@]}"
usage_error "--launch-password needs --launch" "needs --launch" -- --server "$M3/relative.toml" --launch-password "$M3/password"
usage_error "--launch words can't carry a password file" "only from --launch-password" -- \
  --server "$M3/relative.toml" --launch "--username x --password-file $M3/password" --launch-password "$M3/password"
usage_error "--launch words can't carry a password" "only from --launch-password" -- \
  --server "$M3/relative.toml" --launch "--username x --password=y" --launch-password "$M3/password"
usage_error "--launch takes words of [A-Za-z0-9=_.:/@-]" "takes words" -- \
  --server "$M3/relative.toml" --launch '--username x;echo' --launch-password "$M3/password"
usage_error "--launch words aren't globbed" "takes words" -- \
  --server "$M3/relative.toml" --launch '--username /bi?' --launch-password "$M3/password"
usage_error "--env can't give the launcher its password" "password or the ticket" -- --env FENJI_PASSWORD=x
refuses "--env takes a plain variable name" "plain name" -- --env 'BASH_FUNC_export%%=() { :; }'
usage_error "--launch takes one line of printable ASCII" "one line of printable ASCII" -- \
  --server "$M3/relative.toml" --launch "$(printf -- '--username tester\n--password-file /x')" --launch-password "$M3/password"
usage_error "--env can't turn on the inner shell's tracing" "password or the ticket" -- --env SHELLOPTS=xtrace
usage_error "--server needs a readable file" "can't read" -- --server "$M3/missing.toml"
usage_error "--server takes a config the server can read" "isn't usable" -- --server "$M3/not.toml"
usage_error "--server's binds must be loopback" "must be 127.0.0.1" -- --server "$M3/wide.toml"
usage_error "--server's ports can't be 0" "must be 127.0.0.1" -- --server "$M3/port0.toml"
usage_error "--server's store path has no control characters" "control characters" -- --server "$M3/control.toml"
usage_error "--server and --ports don't mix" "don't mix" -- --server "$M3/relative.toml" --ports 7800
usage_error "--server and --stub-arg don't mix" "no stub runs" -- --server "$M3/relative.toml" --stub-arg --speak
usage_error "--server-data needs --server" "needs --server" -- --server-data "$WORK/data"
usage_error "--server-data must be under re/runs/" "must be a directory under" -- \
  --server "$M3/away.toml" --server-data "$M3/elsewhere"
for own in out io/x client; do
  mkdir -p "$WORK/$own"
  usage_error "--server-data can't be the workspace's $own" "own directories" -- \
    --server "$M3/data.toml" --server-data "$WORK/$own"
done
rmdir "$WORK/out" "$WORK/io/x" "$WORK/io" "$WORK/client"
usage_error "--server-data needs the store path inside it" "must be inside" -- \
  --server "$M3/away.toml" --server-data "$WORK/data"
usage_error "--server-data needs an absolute store path" "must be absolute" -- \
  --server "$M3/relative.toml" --server-data "$WORK/data"
usage_error "a relative store path can't leave the config's folder" "can't leave" -- --server "$M3/escape.toml"
usage_error "an absolute store path needs --server-data" "needs --server-data" -- --server "$M3/absolute.toml"
ln -s /etc/hostname "$WORK/data/planted"
usage_error "--server-data can't hold a link" "a link or a special file" -- --server "$M3/data.toml" --server-data "$WORK/data"
rm -f -- "$WORK/data/planted"
usage_error "--key needs a key name" "takes T:KEY" -- --key 5:
usage_error "--key needs a time" "takes T:KEY" -- --key Return
usage_error "--type needs a time" "takes T:TEXT" -- --type x
usage_error "--type takes printable ASCII" "takes T:TEXT" -- --type "5:$(printf 'a\tb')"
usage_error "a key press can't come after the timeout" "after the --timeout" -- --timeout 100 --key 101:Return
usage_error "a click can't come after the timeout" "after the --timeout" -- --timeout 100 --click 101:1,1
refuses "valid server-mode options pass their checks" "no workspace" -- \
  --server "$M3/data.toml" --server-data "$WORK/data" "${L[@]}" --launch-password "$M3/password" \
  --key 40:Return --key 41:space --type "45:Lin Two" --click 30:640,360
refuses "a relative store path passes without --server-data" "no workspace" -- --server "$M3/relative.toml"

if [ "$(printf 'gj_uinfo=1:90:F:0:t:1\ngj_ticket=fenji1-abc def\n' | redact_gj_ticket)" = "$(printf 'gj_uinfo=1:90:F:0:t:1\ngj_ticket=<redacted>')" ]; then
  ok "redact_gj_ticket hides the ticket's whole value"
else
  bad "redact_gj_ticket hides the ticket's whole value: $(printf 'gj_ticket=fenji1-abc def\n' | redact_gj_ticket)"
fi
got=$(printf 'gj_uinfo=1:90:Fenji:0:tester:5\ngj_gates=127.0.0.1:7800\ngj_ticket=fenji1-ab\n' | parse_launch_output)
want=$(printf 'gj_uinfo=1:90:Fenji:0:tester:5\ngj_gates=127.0.0.1:7800\ngj_ticket=fenji1-ab')
if [ "$got" = "$want" ]; then ok "parse_launch_output passes the gj_* pairs"; else bad "parse_launch_output passes the gj_* pairs: $got"; fi
for case in 'gj_ticket=fenji1\000ab' 'gj_ticket=fenji1\rab' 'gj_ticket=fenji1\nab' 'LD_PRELOAD=/x.so\ngj_ticket=a' 'warning: nothing'; do
  if printf '%b\n' "$case" | parse_launch_output > /dev/null 2>&1; then
    bad "parse_launch_output refuses $case"
  else
    ok "parse_launch_output refuses $case"
  fi
done

# launch_pairs with fake launchers: the password reaches only them, and every bad output fails.
fake() {
  printf '#!/bin/sh\n%s\n' "$1" > "$M3/fenji-launch"
  chmod +x "$M3/fenji-launch"
}
# shellcheck disable=SC2016 # the fake launcher expands its own variable
fake 'printf "gj_uinfo=1:90:Fenji:0:tester:1\ngj_pw=%s\n" "$FENJI_PASSWORD"'
if [ "$( (launch_pairs "$M3/fenji-launch" "p4ss" --username tester) )" = "$(printf 'gj_uinfo=1:90:Fenji:0:tester:1\ngj_pw=p4ss')" ]; then
  ok "launch_pairs gives the launcher the password in its environment"
else
  bad "launch_pairs gives the launcher the password in its environment"
fi
for out in 'printf "gj_ticket=fenji1-aa\000bb\n"' 'printf "gj_ticket=fenji1-aa\nbb\n"' 'printf "gj_ticket=a\nextra\n"' 'printf "gj_ticket=a\n"; exit 1'; do
  fake "$out"
  # In a fresh shell without pipefail, as the sandbox's inner shell runs it.
  if bash -c "$(declare -f parse_launch_output launch_pairs); launch_pairs \"\$1\" x" _ "$M3/fenji-launch" > /dev/null 2>&1; then
    bad "launch_pairs refuses: $out"
  else
    ok "launch_pairs refuses: $out"
  fi
done

# xkey.py refuses bad input before it opens any display; a display that can't exist proves it never types.
XK=$HERE/xkey.py
for a in "--type=$(printf 'a\tb')" "--type=é" "--key=NoSuchKeysym"; do
  python3 -I "$XK" --display :9999 "$a" > /dev/null 2>&1
  rc=$?
  if [ "$rc" = 2 ]; then ok "xkey.py refuses $a"; else bad "xkey.py refuses $a: exit $rc"; fi
done
python3 -I "$XK" --display :9999 --type=-x > /dev/null 2>&1
rc=$?
if [ "$rc" = 1 ]; then ok "xkey.py takes text that starts with a dash"; else bad "xkey.py takes text that starts with a dash: exit $rc"; fi

# The password reaches the sandbox as one line through a pipe, and nothing without --launch.
if [ "$(launch_password=$M3/password password_line)" = "secret-for-tests" ] && [ -z "$(launch_password='' password_line)" ]; then
  ok "the password goes to the sandbox as one line, through a pipe"
else
  bad "the password goes to the sandbox as one line, through a pipe"
fi

# A ticket that reached the evidence is redacted in place and listed.
mkdir -p "$M3/out/io"
printf 'trace: Command: x gj_ticket=fenji1-%s\n' "$(printf 'ab%.0s' $(seq 32))" > "$M3/out/io/wine.log"
redact_tickets "$M3/out"
if ! grep -qE 'fenji1-[0-9a-f]{64}' "$M3/out/io/wine.log" && grep -qx 'io/wine.log' "$M3/out/redactions.txt"; then
  ok "a ticket in the evidence is redacted and listed"
else
  bad "a ticket in the evidence is redacted and listed"
fi

# inputs.txt records the launch, but never the password or its file.
if (
  guard
  OUT=$TESTROOT/inputs2 run=t ports=7800 tmo=9 display=99 channels=-all hosts=$TESTROOT/hosts
  WRAP=$TESTROOT/hosts WATCHDOG=$TESTROOT/hosts PROTON=/nonexistent
  envs=() client_files=() clicks=() stub_args=() keys=(40:Return) types=("45:Lin")
  server_cfg=$M3/data.toml server_copy=$M3/data.toml server_data=$WORK/data launch="--username tester"
  launch_password=$M3/password
  mkdir -p "$OUT" && echo "127.0.0.1 localhost" > "$hosts"
  record_inputs > /dev/null 2>&1
  ! grep -q -e secret-for-tests -e "$M3/password" "$OUT/inputs.txt" &&
    grep -qx 'launch args: --username tester' "$OUT/inputs.txt" &&
    grep -qx 'keys (T:KEY): 40:Return' "$OUT/inputs.txt" &&
    grep -q '^launch password: given on the sandbox standard input' "$OUT/inputs.txt" &&
    ! grep -q '^stub ports' "$OUT/inputs.txt"
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
