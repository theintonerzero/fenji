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

refuses "--no-gpu is one of run's options" "no workspace" -- --no-gpu
usage_setup_gpu=$( (FENJI_CLIENT_PATH="" cmd_setup --no-gpu) 2>&1 > /dev/null)
if [[ $usage_setup_gpu == *"set FENJI_CLIENT_PATH"* ]]; then ok "--no-gpu is one of setup's options"; else bad "setup --no-gpu: $usage_setup_gpu"; fi

# renderer gives the sandbox --gpu and Mesa's CPU-copy presentation, or lavapipe's ICD and no
# --gpu (#266).
if (
  guard
  gpu=1; renderer
  [ "${wgpu[*]}" = --gpu ] && [ "${vkenv[*]}" = MESA_VK_WSI_DEBUG=sw ] || exit 1
  gpu=""; renderer
  [ "${#wgpu[@]}" = 0 ] && [ "${vkenv[*]}" = "VK_ICD_FILENAMES=$LAVAPIPE" ]
); then
  ok "renderer: --gpu with MESA_VK_WSI_DEBUG=sw, or lavapipe's ICD without --gpu"
else
  bad "renderer: --gpu with MESA_VK_WSI_DEBUG=sw, or lavapipe's ICD without --gpu"
fi

# inputs.txt says how the run renders (#266): the GPU through isolated-run.sh --gpu by default,
# lavapipe with --no-gpu.
for g in 1 ""; do
  if (
    guard
    OUT=$TESTROOT/inputs-gpu-${g:-no} run=t ports=7800 tmo=9 display=99 channels=-all hosts=$TESTROOT/hosts
    WRAP=$TESTROOT/hosts WATCHDOG=$TESTROOT/hosts PROTON=/nonexistent gpu=$g
    envs=() client_files=() clicks=() stub_args=()
    mkdir -p "$OUT" && echo "127.0.0.1 localhost" > "$hosts"
    record_inputs -x 1 > /dev/null 2>&1
    if [ -n "$g" ]; then
      grep -q '^sandbox: isolated-run.sh .* --cpu-topology --gpu ' "$OUT/inputs.txt" &&
        grep -qx 'rendering: the host GPU, through its render node (DXVK picks it), frames copied to the display (MESA_VK_WSI_DEBUG=sw)' "$OUT/inputs.txt"
    else
      ! grep -q -- '--gpu' "$OUT/inputs.txt" &&
        grep -qx 'rendering: lavapipe, in software (--no-gpu)' "$OUT/inputs.txt"
    fi
  ); then
    ok "inputs.txt records the renderer (${g:+GPU}${g:-lavapipe})"
  else
    bad "inputs.txt records the renderer (${g:+GPU}${g:-lavapipe})"
  fi
done

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
usage_error "a held key needs a hold of at least 1 ms" "takes T:KEY" -- --key 5:w/0
usage_error "a held key holds a minute at most" "takes T:KEY" -- --key 5:w/60001
usage_error "a held key needs its hold in ms" "takes T:KEY" -- --key 5:w/
usage_error "a held key can't outlast the timeout" "after the --timeout" -- --timeout 100 --key 99:w/1001
refuses "a held key may end at the timeout" "no workspace" -- --timeout 100 --key 99:w/1000
refuses "valid server-mode options pass their checks" "no workspace" -- \
  --server "$M3/data.toml" --server-data "$WORK/data" "${L[@]}" --launch-password "$M3/password" \
  --key 40:Return --key 41:space --type "45:Lin Two" --click 30:640,360 --key 50:w/1500 --key 99:a/1000
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
# --type-file types a file's first line, so a password never sits on a command line (launcher
# Task 16); its line is checked as --type's text is, before any display is opened.
KF=$(mktemp -d)
printf 'a long password\nsecond line\n' > "$KF/good"
printf 'a\tb\n' > "$KF/tab"
printf '\n' > "$KF/empty"
printf '\303\251\n' > "$KF/accent"
for a in "--type-file=$KF/none" "--type-file=$KF/tab" "--type-file=$KF/empty" "--type-file=$KF/accent"; do
  python3 -I "$XK" --display :9999 "$a" > /dev/null 2>&1
  rc=$?
  if [ "$rc" = 2 ]; then ok "xkey.py refuses ${a#--type-file="$KF"/}"; else bad "xkey.py refuses $a: exit $rc"; fi
done
python3 -I "$XK" --display :9999 "--type-file=$KF/good" > /dev/null 2>&1
rc=$?
if [ "$rc" = 1 ]; then ok "xkey.py takes a file's first line as text to type"; else bad "xkey.py --type-file: exit $rc"; fi
python3 -I "$XK" --display :9999 "--type-file=$KF/good" --hold=100 > /dev/null 2>&1
rc=$?
if [ "$rc" = 2 ]; then ok "xkey.py holds keys, not a file's text"; else bad "xkey.py --type-file --hold: exit $rc"; fi
python3 -I "$XK" --display :9999 "--type-file=$KF/good" --type=x > /dev/null 2>&1
rc=$?
if [ "$rc" = 2 ]; then ok "xkey.py types a file or text, not both"; else bad "xkey.py --type-file --type: exit $rc"; fi
rm -rf -- "$KF"
# A hold is a key's alone, from 1 ms to a minute; a good one gets as far as opening the display.
for a in "--hold=0" "--hold=60001" "--hold=x" "--hold=-5"; do
  python3 -I "$XK" --display :9999 --key=w "$a" > /dev/null 2>&1
  rc=$?
  if [ "$rc" = 2 ]; then ok "xkey.py refuses $a"; else bad "xkey.py refuses $a: exit $rc"; fi
done
python3 -I "$XK" --display :9999 --type=hi --hold=100 > /dev/null 2>&1
rc=$?
if [ "$rc" = 2 ]; then ok "xkey.py holds keys, not typing"; else bad "xkey.py holds keys, not typing: exit $rc"; fi
python3 -I "$XK" --display :9999 --key=w --hold=60000 > /dev/null 2>&1
rc=$?
if [ "$rc" = 1 ]; then ok "xkey.py takes a hold of a minute"; else bad "xkey.py takes a hold of a minute: exit $rc"; fi

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

# --- M4 Task 13: two clients in one namespace (world plan, Amendment B)
TWO=(--clients 2 --display2 98)
L2=(--launch2 "--server http://127.0.0.1:7801 --username second")
printf 'second-secret\n' > "$M3/password2"
usage_error "--clients takes 1 or 2" "--clients takes 1 or 2" -- --clients 3
usage_error "--clients 2 needs --display2" "needs --display2" -- --clients 2
usage_error "--display2 must be another display than --display" "another display" -- --clients 2 --display2 99
usage_error "--display2 takes a display number" "--display2 takes" -- --clients 2 --display2 9x
usage_error "--display2 needs --clients 2" "needs --clients 2" -- --display2 98
usage_error "--second-after takes at least 2 s" "at least 2" -- "${TWO[@]}" --second-after 1
usage_error "--second-after takes whole seconds" "--second-after takes" -- "${TWO[@]}" --second-after 2.5
usage_error "--second-after needs --clients 2" "needs --clients 2" -- --second-after 30
usage_error "--timeout2 takes whole seconds" "--timeout2 takes" -- "${TWO[@]}" --timeout2 x
usage_error "--timeout2 needs --clients 2" "needs --clients 2" -- --timeout2 60
usage_error "a 2@ key press needs --clients 2" "need --clients 2" -- --key 2@40:Return
usage_error "a 2@ click needs --clients 2" "need --clients 2" -- --click 2@40:1,1
usage_error "--launch with two clients needs --launch2" "needs --launch2" -- \
  --server "$M3/relative.toml" "${L[@]}" --launch-password "$M3/password" "${TWO[@]}"
usage_error "--launch2 needs --launch-password2" "needs --launch-password2" -- \
  --server "$M3/relative.toml" "${L[@]}" --launch-password "$M3/password" "${TWO[@]}" "${L2[@]}"
usage_error "--launch2 needs --launch" "--launch2 needs --launch" -- --server "$M3/relative.toml" "${TWO[@]}" "${L2[@]}"
usage_error "--launch-password2 needs --launch2" "--launch-password2 needs --launch2" -- \
  --server "$M3/relative.toml" "${TWO[@]}" --launch-password2 "$M3/password2"
usage_error "--launch2 words can't carry a password" "only from --launch-password2" -- \
  --server "$M3/relative.toml" "${L[@]}" --launch-password "$M3/password" "${TWO[@]}" \
  --launch2 "--username b --password=x" --launch-password2 "$M3/password2"
usage_error "--launch2 takes words of [A-Za-z0-9=_.:/@-]" "takes words" -- \
  --server "$M3/relative.toml" "${L[@]}" --launch-password "$M3/password" "${TWO[@]}" \
  --launch2 '--username b;echo' --launch-password2 "$M3/password2"
usage_error "--launch-password2 must be readable" "--launch-password2: can't read" -- \
  --server "$M3/relative.toml" "${L[@]}" --launch-password "$M3/password" "${TWO[@]}" "${L2[@]}" \
  --launch-password2 "$M3/none"
usage_error "a 2@ press can't come after client 2's timeout" "after client 2's" -- \
  "${TWO[@]}" --timeout 100 --second-after 30 --timeout2 20 --key 2@51:Return
usage_error "a 2@ click can't come after client 2's timeout" "after client 2's" -- \
  "${TWO[@]}" --timeout 100 --second-after 30 --timeout2 20 --click 2@51:1,1
usage_error "client 2's held key can't outlast its timeout" "after client 2's" -- \
  "${TWO[@]}" --timeout 100 --second-after 30 --timeout2 20 --key 2@49:w/1001
refuses "a 2@ click takes T:X,Y in whole numbers" "--click" -- "${TWO[@]}" --click 2@5:abc
refuses "valid two-client options pass their checks" "no workspace" -- \
  --server "$M3/data.toml" --server-data "$WORK/data" "${L[@]}" --launch-password "$M3/password" \
  "${TWO[@]}" "${L2[@]}" --launch-password2 "$M3/password2" --second-after 30 --timeout2 200 \
  --key 40:Return --key 2@75:Return --type "2@80:Mai" --click 2@70:640,360

# Two passwords go to the sandbox as two lines, the second client's second, each ending its line.
printf 'no-newline' > "$M3/password-nonl"
got=$(launch_password=$M3/password launch_password2=$M3/password2 password_line)
got_nonl=$(launch_password=$M3/password-nonl launch_password2=$M3/password2 password_line | sed -n 2p)
if [ "$got" = "$(printf 'secret-for-tests\nsecond-secret')" ] && [ "$got_nonl" = second-secret ]; then
  ok "two passwords go to the sandbox as two lines, client 2's second"
else
  bad "two passwords: '$got' / '$got_nonl'"
fi

# 2@ inputs go to client 2's display, in time order with client 1's: a stable sort, so at one time
# they keep the order given.
got=$(
  guard
  tmo=100 tmo2=60 second_after=30 display=99 display2=98
  keys=(40:Return) types=() clicks=("70:1,1" "30:5,5") keys2=(35:Return 30:space) types2=(45:Mai) clicks2=("40:2,2" "30:7,7")
  presses=("1:key:40:Return" "2:key:35:Return" "2:key:30:space" "2:type:45:Mai")
  check_presses > /dev/null 2>&1 || exit 1
  printf '%s\n' "${presses[@]}" -- && click_plan
)
want=$(printf '%s\n' 2:key:30:space 2:key:35:Return 1:key:40:Return 2:type:45:Mai -- \
  "30 99 1 5,5" "30 98 2 7,7" "40 98 2 2,2" "70 99 1 1,1")
if [ "$got" = "$want" ]; then ok "2@ inputs are client 2's, played in time order with client 1's"; else bad "input plans: $got"; fi

# end_client ends the gujianol.exe whose program is the given copy's, as Wine shows it (Z: and
# backslashes) or as given, and nothing else: not client 1's.
EC=$TESTROOT/ec
mkdir -p "$EC/client/bin64" "$EC/client2/bin64"
ln -s "$(command -v python3)" "$EC/client/bin64/gujianol.exe"
ln -s "$(command -v python3)" "$EC/client2/bin64/gujianol.exe"
"$EC/client/bin64/gujianol.exe" -c 'import time; time.sleep(30)' &
one=$!
"$EC/client2/bin64/gujianol.exe" -c 'import time; time.sleep(30)' &
two=$!
winpath="Z:${EC//\//\\}\\client2\\bin64\\gujianol.exe"
(exec -a "$winpath" "$EC/client2/bin64/gujianol.exe" -c 'import time; time.sleep(30)') &
three=$!
# The client's main thread renames itself (run m4-s0: "vx_initialize"), and Wine passes the name to
# Linux: its comm is no longer gujianol.exe, but its argv[0] still names the program.
cp -- "$(command -v sleep)" "$EC/vx_initialize"
(exec -a "$winpath" "$EC/vx_initialize" 30) &
four=$!
for _ in $(seq 50); do
  [ "$(cat "/proc/$three/comm" 2> /dev/null)" = gujianol.exe ] && [ "$(cat "/proc/$two/comm" 2> /dev/null)" = gujianol.exe ] &&
    [ "$(cat "/proc/$four/comm" 2> /dev/null)" = vx_initialize ] && break
  sleep 0.1
done
ended=$(end_client "$EC/client2/bin64/gujianol.exe" | sort -n | tr '\n' ' ')
{ wait "$two"; } 2> /dev/null
{ wait "$three"; } 2> /dev/null
{ wait "$four"; } 2> /dev/null
if [ "$ended" = "$(printf '%s\n' "$two" "$three" "$four" | sort -n | tr '\n' ' ')" ] && kill -0 "$one" 2> /dev/null &&
  ! kill -0 "$two" 2> /dev/null && ! kill -0 "$three" 2> /dev/null && ! kill -0 "$four" 2> /dev/null; then
  ok "end_client ends client 2's gujianol.exe by its pid, in either form of its path, renamed or not, and not client 1's"
else
  bad "end_client: ended '$ended' of $one $two $three $four"
fi
kill "$one" 2> /dev/null
wait "$one" 2> /dev/null

# --- runs end to end (Amendment B): one client, then two. isolated-run.sh stands in as a wrapper that
# records its arguments and runs what follows --, with DISPLAY from its first --x11. GE-Proton is a
# script that records what each client got and starts the copy's gujianol.exe (Python behind a link)
# in its own session, so that it outlives `proton run` as Wine's processes do, and writes a client's
# logs; wineserver -k records which clients are left and ends them. fenji-server listens, fenji-launch
# prints pairs for its --username and records the password it got, and the watchdog's watch mode
# stands in (its baseline and collect modes are the real ones). xclick.py, xkey.py and import record
# what they would play. pgrep finds no Wine.
F=$TESTROOT/flow
mkdir -p "$F/bin" "$F/here" "$F/proton/files/bin" "$F/release" "$F/log"
cat > "$F/bin/pgrep" <<'EOF'
#!/bin/sh
exit 1
EOF
cat > "$F/bin/import" <<'EOF'
#!/bin/sh
echo "import $*" >> "$FAKE_LOG/import"
EOF
cat > "$F/here/xclick.py" <<'EOF'
import os, sys, time
with open(os.path.join(os.environ["FAKE_LOG"], "inputs"), "a") as f:
    f.write(f"{time.time():.2f} click {' '.join(sys.argv[1:])}\n")
EOF
cat > "$F/here/xkey.py" <<'EOF'
import os, sys, time
with open(os.path.join(os.environ["FAKE_LOG"], "inputs"), "a") as f:
    f.write(f"{time.time():.2f} press {' '.join(sys.argv[1:])}\n")
# A held key takes as long as its hold, as the real one does.
for a in sys.argv[1:]:
    if a.startswith("--hold="):
        time.sleep(int(a.split("=", 1)[1]) / 1000)
EOF
cat > "$F/wrap" <<'EOF'
#!/usr/bin/env bash
# Records its arguments, then runs what follows --, with DISPLAY from the first --x11.
echo "$*" >> "$FAKE_LOG/wrap"
d=""
while [ $# -gt 0 ] && [ "$1" != -- ]; do
  if [ "$1" = --x11 ] && [ -z "$d" ]; then d=$2; fi
  shift
done
shift
export DISPLAY=":$d"
exec "$@"
EOF
cat > "$F/proton/proton" <<'EOF'
#!/usr/bin/env python3
import json, os, subprocess, sys, time
assert sys.argv[1] == "run"
exe, args, log = sys.argv[2], sys.argv[3:], os.environ["FAKE_LOG"]
# explorer.exe /desktop=NAME,SIZE PROGRAM ARGS: PROGRAM in a desktop of its own, its argv[0] the
# Windows path, as Wine shows it.
program, argv0 = exe, exe
if exe == "explorer.exe":
    assert args[0].startswith("/desktop="), args
    argv0 = args[1]
    program = argv0[2:].replace("\\", "/") if argv0[:2].upper() == "Z:" else argv0
here = os.path.dirname(program)
client = subprocess.Popen([argv0, "-c", "import time; time.sleep(120)"], executable=program, start_new_session=True,
                          stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
record = {"t": time.time(), "exe": exe, "args": args, "program": program, "cwd": os.getcwd(), "pid": client.pid,
          "display": os.environ.get("DISPLAY"), "log_dir": os.environ.get("PROTON_LOG_DIR"),
          "pairs": sorted(f"{k}={v}" for k, v in os.environ.items() if k.startswith("gj_"))}
with open(os.path.join(log, "proton"), "a") as f:
    f.write(json.dumps(record) + "\n")
with open(os.path.join(log, "clients"), "a") as f:
    f.write(f"{client.pid}\n")
with open(os.path.join(os.environ["PROTON_LOG_DIR"], "steam-1418100.log"), "a") as f:
    f.write(f"1.000:0020:0024:trace:process:a fake client {program}\n")
for name in ("gujianol.log", "fake-cache.bin"):
    with open(os.path.join(here, name), "a") as f:
        f.write("written by the client\n")
time.sleep(600)
EOF
cat > "$F/proton/files/bin/wineserver" <<'EOF'
#!/usr/bin/env python3
import json, os, signal, sys, time
log = os.environ["FAKE_LOG"]
try:
    pids = [int(x) for x in open(os.path.join(log, "clients")).read().split()]
except FileNotFoundError:
    pids = []
def alive(p):
    try:
        return open(f"/proc/{p}/stat").read().split(") ", 1)[1][0] != "Z"
    except (OSError, IndexError):
        return False
left = [p for p in pids if alive(p)]
with open(os.path.join(log, "wineserver"), "a") as f:
    f.write(json.dumps({"t": time.time(), "args": sys.argv[1:], "alive": left}) + "\n")
for p in left:
    os.kill(p, signal.SIGKILL)
EOF
cat > "$F/release/fenji-server" <<'EOF'
#!/usr/bin/env python3
import os, socket, sys, time, tomllib
with open(sys.argv[2], "rb") as f:
    server = tomllib.load(f)["server"]
held = []
for key in ("login_bind", "api_bind"):
    host, port = server[key].rsplit(":", 1)
    s = socket.socket()
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.bind((host, int(port)))
    s.listen()
    held.append(s)
parent = os.getppid()
while os.getppid() == parent:
    time.sleep(0.2)
EOF
cat > "$F/release/fenji-launch" <<'EOF'
#!/usr/bin/env bash
# play --print ... --username NAME: the pairs for NAME, and a record of the password it got.
user=""
while [ $# -gt 0 ]; do [ "$1" = --username ] && user=$2; shift; done
echo "$user $FENJI_PASSWORD" >> "$FAKE_LOG/launch"
ticket=$(printf '%s' "$user" | sha256sum | cut -c1-64)
printf 'gj_uinfo=1:90:Fenji:0:%s:1\ngj_gates=127.0.0.1:7800\ngj_ticket=fenji1-%s\n' "$user" "$ticket"
EOF
cat > "$F/watchdog.py" <<'EOF'
"""The flows' watchdog: watch records its arguments, creates the start signal and waits for the
sandbox to end; every other mode is the real watchdog's."""
import os, sys, time
if sys.argv[1] != "watch":
    os.execv(sys.executable, [sys.executable, "-I", os.path.join(os.environ["FAKE_REAL_SPIKE"], "watchdog.py")] + sys.argv[1:])
with open(os.path.join(os.environ["FAKE_LOG"], "watchdog"), "a") as f:
    f.write(" ".join(sys.argv[1:]) + "\n")
args = sys.argv[2:]
open(args[args.index("--go") + 1], "x").close()
spid = int(args[args.index("--sandbox-pid") + 1])
def alive(pid):
    try:
        return open(f"/proc/{pid}/stat").read().split(") ", 1)[1][0] != "Z"
    except (OSError, IndexError):
        return False
while alive(spid):
    time.sleep(0.05)
EOF
chmod +x "$F/bin/"* "$F/wrap" "$F/proton/proton" "$F/proton/files/bin/wineserver" "$F/release/"*
read -r lp ap < <(python3 -I -c 'import socket
s = [socket.socket() for _ in range(2)]
for x in s:
    x.bind(("127.0.0.1", 0))
print(*(x.getsockname()[1] for x in s))')
printf '[server]\nname = "Fenji"\nlogin_bind = "127.0.0.1:%s"\napi_bind = "127.0.0.1:%s"\n' "$lp" "$ap" > "$F/fenji.toml"
printf 'first-secret\n' > "$F/pw1"
printf 'second-secret\n' > "$F/pw2"

# workspace DIR: a run workspace as setup makes it, with each copy's gujianol.exe a link to Python.
workspace() {
  local c
  mkdir -p "$1/compat/pfx" "$1/home" "$1/steamclient" "$1/out" "$1/stub-logs"
  printf 'WINE REGISTRY Version 2\n' > "$1/compat/pfx/system.reg"
  : > "$1/hosts-empty"
  for c in client client2; do
    mkdir -p "$1/$c/bin64" "$1/$c/data"
    ln -s "$(command -v python3)" "$1/$c/bin64/gujianol.exe"
  done
}

# flow NAME WORK ARGS...: cmd_run NAME ARGS with the stand-ins and WORK as the workspace, its records
# in $F/log/NAME; sets rc and out.
flow() {
  local name=$1 dir=$2
  shift 2
  mkdir -p "$F/log/$name"
  rc=0
  (
    export FAKE_LOG=$F/log/$name FAKE_REAL_SPIKE=$HERE
    WORK=$dir REPO=$dir PROTON=$F/proton STUBBIN=$F/release WATCHDOG=$F/watchdog.py WRAP=$F/wrap HERE=$F/here
    PATH="$F/bin:$PATH"
    hash -r
    need_display() { :; }
    cmd_run "$name" "$@"
  ) > "$F/log/$name/out" 2>&1 || rc=$?
  out=$(cat -- "$F/log/$name/out")
}
# field NAME N KEY: the KEY of flow NAME's N-th proton record (1 or 2), as JSON.
field() { sed -n "${2}p" "$F/log/$1/proton" | python3 -I -c 'import json, sys; print(json.dumps(json.load(sys.stdin)[sys.argv[1]]))' "$3"; }

W1=$TESTROOT/w1
workspace "$W1"
flow one "$W1" --server "$F/fenji.toml" --launch "--server http://127.0.0.1:$ap --username tester" \
  --launch-password "$F/pw1" --timeout 3 -- --fs=data:_index:_index
if [ "$rc" = 0 ] && [ "$(wc -l < "$F/log/one/proton")" = 1 ] && [ "$(field one 1 display)" = '":99"' ] &&
  [ "$(field one 1 log_dir)" = "\"$W1/io/one\"" ] && [ "$(field one 1 exe)" = "\"$W1/client/bin64/gujianol.exe\"" ] &&
  [ -f "$W1/out/one/io/exit-code" ] && [ ! -e "$W1/out/one/io/exit-code-2" ]; then
  ok "a one-client run is as before: one client, from client/, on --display, its log in the run's io"
else
  bad "one client: rc $rc: $out"
fi
case " $(cat "$F/log/one/watchdog") " in
  *" --log $W1/io/one/steam-1418100.log --go "*) ok "a one-client run gives the watchdog one log" ;;
  *) bad "one client's watchdog: $(cat "$F/log/one/watchdog")" ;;
esac
case " $(cat "$F/log/one/wrap") " in
  *" --bind $W1/client2 "* | *"--x11 98"*) bad "one client's sandbox: $(cat "$F/log/one/wrap")" ;;
  *" --x11 99 --cpu-topology "*) ok "a one-client run binds no client2/ and takes one display" ;;
  *) bad "one client's sandbox: $(cat "$F/log/one/wrap")" ;;
esac

W2=$TESTROOT/w2
workspace "$W2"
flow two "$W2" --server "$F/fenji.toml" --launch "--server http://127.0.0.1:$ap --username tester" \
  --launch-password "$F/pw1" --clients 2 --display2 98 --launch2 "--server http://127.0.0.1:$ap --username second" \
  --launch-password2 "$F/pw2" --second-after 2 --timeout 20 --timeout2 3 \
  --click 1:10,10 --key 1:w/3000 --key 2@3:Return --type 2@4:hi --click 2@4:20,20 -- --fs=data:_index:_index
IO2=$W2/io/two OUT2=$W2/out/two
if [ "$rc" = 0 ] && [[ $out == *"run two done:"* ]]; then ok "a two-client run goes end to end"; else bad "two clients: rc $rc: $out"; fi
case " $(cat "$F/log/two/wrap") " in
  *" --x11 99 --x11 98 --cpu-topology "*" --bind $W2/client2 "*) ok "client 2 has its own display, and client2/ is bound read-write" ;;
  *) bad "two clients' sandbox: $(cat "$F/log/two/wrap")" ;;
esac
case " $(cat "$F/log/two/watchdog") " in
  *" --log $IO2/steam-1418100.log --log $IO2/2/steam-1418100.log "*" --bind $W2/client2 "*)
    ok "the watchdog gets each client's log, in order, and leaves client2/ out of its tmpfs scan" ;;
  *) bad "two clients' watchdog: $(cat "$F/log/two/watchdog")" ;;
esac
p1=$(field two 1 pid) p2=$(field two 2 pid)
if [ "$(field two 1 exe)" = "\"$W2/client/bin64/gujianol.exe\"" ] && [ "$(field two 1 cwd)" = "\"$W2/client/bin64\"" ] &&
  [ "$(field two 1 display)" = '":99"' ] && [ "$(field two 1 log_dir)" = "\"$IO2\"" ] &&
  [ "$(field two 2 program)" = "\"$W2/client2/bin64/gujianol.exe\"" ] && [ "$(field two 2 cwd)" = "\"$W2/client2/bin64\"" ] &&
  [ "$(field two 2 display)" = '":98"' ] && [ "$(field two 2 log_dir)" = "\"$IO2/2\"" ]; then
  ok "each client runs from its own copy, on its own display, with its own Proton log"
else
  bad "the clients' places: $(cat "$F/log/two/proton")"
fi
# Client 2 runs in a Wine desktop of its own (run m4-s2): on one desktop, each client's input went
# to whichever window that desktop picked, whatever display it came from.
win2="Z:${W2//\//\\}\\client2\\bin64\\gujianol.exe"
if [ "$(field two 1 exe)" = "\"$W2/client/bin64/gujianol.exe\"" ] && [ "$(field two 2 exe)" = '"explorer.exe"' ] &&
  [ "$(field two 2 args)" = "$(python3 -I -c 'import json, sys; print(json.dumps(["/desktop=fenji-client2,1280x720", sys.argv[1], "--fs=data:_index:_index"]))' "$win2")" ]; then
  ok "client 2 runs in a Wine desktop of its own, its program by its Windows path; client 1 as before"
else
  bad "client 2's command: $(field two 2 exe) $(field two 2 args)"
fi
pairs1=$(field two 1 pairs) pairs2=$(field two 2 pairs)
if [[ $pairs1 == *":tester:1"* && $pairs1 != *second* && $pairs2 == *":second:1"* && $pairs2 != *tester* ]] &&
  [ "$(cat "$F/log/two/launch")" = "$(printf 'tester first-secret\nsecond second-secret')" ]; then
  ok "each client signs in with its own account and password, and gets only its own pairs"
else
  bad "the clients' accounts: $pairs1 / $pairs2 / $(cat "$F/log/two/launch")"
fi
t2=$(field two 2 t)
ended=$(sed -n 2p "$OUT2/io/2/ended-by-pid" 2> /dev/null)
ended_at=$(sed -n 1p "$OUT2/io/2/ended-by-pid" 2> /dev/null)
if [ "$ended" = "$p2" ] && python3 -I -c 'import sys; sys.exit(0 if float(sys.argv[2]) >= float(sys.argv[1]) + 3 + 10 - 1 else 1)' "$t2" "$ended_at" &&
  [ "$(python3 -I -c 'import json, sys; print(json.loads(sys.stdin.readline())["alive"])' < "$F/log/two/wineserver")" = "[$p1]" ]; then
  ok "client 2, outliving its proton run, is ended by its own pid 10 s after --timeout2, while client 1 runs on"
else
  bad "client 2's end: ended '$ended' at '$ended_at' (started $t2); wineserver $(cat "$F/log/two/wineserver" 2> /dev/null)"
fi
if [ "$(cat "$OUT2/io/exit-code-2" 2> /dev/null)" = 124 ] && [ -f "$OUT2/io/exit-code" ] &&
  [ "$(grep -c . "$F/log/two/wineserver")" = 1 ]; then
  ok "the sandbox writes exit-code and exit-code-2, and stops the wineserver once, when both have ended"
else
  bad "exit codes: '$(cat "$OUT2/io/exit-code-2" 2> /dev/null)', wineserver: $(cat "$F/log/two/wineserver" 2> /dev/null)"
fi
if [ -f "$OUT2/wine.log" ] && [ -f "$OUT2/wine-2.log" ] && grep -q client2 "$OUT2/wine-2.log" &&
  [ -f "$OUT2/gujianol.log" ] && [ -f "$OUT2/client2/gujianol.log" ] && [ ! -e "$W2/client2/bin64/gujianol.log" ] &&
  grep -q "$W2/client/bin64/fake-cache.bin" "$OUT2/new-files.txt" && grep -q "$W2/client2/bin64/fake-cache.bin" "$OUT2/new-files.txt"; then
  ok "client 2's evidence: its own logs out of its copy, wine-2.log, and new-files lists both copies"
else
  bad "client 2's evidence: $(cd "$OUT2" && find . -maxdepth 2 | sort | tr '\n' ' ')"
fi
if grep -qxF "import -display :98 -window root $OUT2/screen-2-5s.png" "$F/log/two/import" &&
  grep -qxF "import -display :99 -window root $OUT2/screen-5s.png" "$F/log/two/import"; then
  ok "client 2's display has its own screenshots"
else
  bad "screenshots: $(cat "$F/log/two/import")"
fi
got=$(cut -d' ' -f2- "$F/log/two/inputs")
want=$(printf '%s\n' "click --display :99 10 10" "press --display :99 --key=w --hold=3000" "press --display :98 --key=Return" \
  "press --display :98 --type=hi" "click --display :98 20 20")
got_sorted=$(sort -k1,1n "$F/log/two/inputs" | cut -d' ' -f2-)
if [ "$(sort <<< "$got")" = "$(sort <<< "$want")" ] && [ "$(sed -n 1p <<< "$got_sorted")" = "click --display :99 10 10" ]; then
  ok "client 2's inputs go to its display, in time with client 1's"
else
  bad "inputs: $got"
fi
# Client 1 holds w for 3 s from 1 s; client 2's Return at 3 s goes when it is due, not after the hold.
hold_at=$(awk '/ press --display :99 --key=w --hold=3000$/ { print $1 }' "$F/log/two/inputs")
return_at=$(awk '/ press --display :98 --key=Return$/ { print $1 }' "$F/log/two/inputs")
if [ -n "$hold_at" ] && [ -n "$return_at" ] &&
  python3 -I -c 'import sys; sys.exit(0 if 1.5 < float(sys.argv[2]) - float(sys.argv[1]) < 2.6 else 1)' "$hold_at" "$return_at"; then
  ok "a held key holds up no press after it"
else
  bad "a held key and the press after it: hold at '$hold_at', Return at '$return_at'"
fi
mkdir "$W2/client2/settings"
flow left "$W2" --clients 2 --display2 98 -- --fs=data:_index:_index
if [ "$rc" = 1 ] && [[ $out == *"leftover settings"*client2* ]]; then
  ok "leftovers in client2/ refuse the next two-client run"
else
  bad "client2 leftovers: rc $rc: $out"
fi
rm -rf -- "$W2/client2/settings"

# setup --clients 2 on an existing workspace makes client2/ only, as a reflink copy.
SW=$TESTROOT/sw
mkdir -p "$SW/client/bin64" "$TESTROOT/pinned/bin64"
touch "$TESTROOT/pinned/bin64/gujianol.exe"
cat > "$F/bin/cp" <<'EOF'
#!/usr/bin/env bash
# The test root has no reflinks: --reflink=always becomes a plain copy, and the call is recorded.
echo "cp $*" >> "$FAKE_LOG/cp"
here=$(dirname -- "$0")
args=()
for a in "$@"; do [ "$a" = --reflink=always ] || args+=("$a"); done
PATH=${PATH//"$here:"/} exec cp "${args[@]}"
EOF
chmod +x "$F/bin/cp"
mkdir -p "$F/log/setup"
rc=0
out=$(
  export FAKE_LOG=$F/log/setup
  WORK=$SW FENJI_CLIENT_PATH=$TESTROOT/pinned PATH="$F/bin:$PATH" WRAP=/nonexistent
  hash -r
  cmd_setup --clients 2 2>&1
) || rc=$?
if [ "$rc" = 0 ] && [ -f "$SW/client2/bin64/gujianol.exe" ] && grep -q -- '--reflink=always' "$F/log/setup/cp"; then
  ok "setup --clients 2 on an existing workspace makes client2/, a reflink copy, and nothing else"
else
  bad "setup --clients 2: rc $rc: $out"
fi
rc=0
out=$(WORK=$SW FENJI_CLIENT_PATH=$TESTROOT/pinned cmd_setup --clients 2 2>&1) || rc=$?
if [ "$rc" = 1 ] && [[ $out == *"client2 exists already"* ]]; then ok "setup refuses a second client2/"; else bad "setup again: rc $rc: $out"; fi
usage_setup=$( (cmd_setup --clients 3) 2>&1 > /dev/null)
if [[ $usage_setup == *"--clients takes 1 or 2"* ]]; then ok "setup's --clients takes 1 or 2"; else bad "setup --clients 3: $usage_setup"; fi

guard
[ "$fails" = 0 ] || { echo "$fails failed"; exit 1; }
echo "all passed"
