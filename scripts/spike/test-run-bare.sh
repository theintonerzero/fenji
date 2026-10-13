#!/usr/bin/env bash
# Tests for run-bare.sh's own logic, with no namespace, no Wine and no client: its argument checks,
# its guards (Steam and Wine running), the command lines that carry the guards (the namespace, the
# watchdog, fenji-launch), and its verdict.
#   scripts/spike/test-run-bare.sh
#
# Everything the tests touch lives under TESTROOT. run-bare.sh is sourced once, here, and every path
# it set is pointed into TESTROOT before any test runs; every test checks that WORK is still there.
set -uo pipefail
HERE=$(cd -- "$(dirname -- "$0")" && pwd -P)
# shellcheck source=run-bare.sh
. "$HERE/run-bare.sh"
set +e # run-bare.sh sets -e; these tests check exit statuses themselves
TESTROOT=$(mktemp -d) || { echo "FATAL: mktemp failed" >&2; exit 2; }
readonly TESTROOT
[ -d "$TESTROOT" ] || { echo "FATAL: no test root" >&2; exit 2; }
trap 'rm -rf -- "$TESTROOT"' EXIT
WORK=$TESTROOT/work REPO=$TESTROOT/repo PINNED_COPY=$TESTROOT/nonexistent UMU=$TESTROOT/nonexistent
PROTON=$TESTROOT/nonexistent STUBBIN=$TESTROOT/nonexistent WATCHDOG=$TESTROOT/nonexistent
mkdir -p "$WORK/data" "$WORK/runs/old" "$TESTROOT/bin"
fails=0
ok() { echo "ok   $1"; }
bad() { echo "FAIL $1"; fails=$((fails + 1)); }

guard() {
  case "$TESTROOT:$WORK:$REPO" in
    /?*:"$TESTROOT"/*:"$TESTROOT"/*) ;;
    *) echo "FATAL: WORK or REPO left the test root: $WORK $REPO" >&2; exit 2 ;;
  esac
}

# says WANT PATTERN WHAT -- COMMAND...: COMMAND, in a subshell, exits WANT with PATTERN on stderr.
says() {
  local want=$1 pattern=$2 what=$3 err rc
  shift 4
  guard
  err=$( ("$@") 2>&1 > /dev/null)
  rc=$?
  if [ "$rc" = "$want" ] && grep -q -- "$pattern" <<< "$err"; then ok "$what"; else bad "$what: exit $rc: $err"; fi
}

# A fake pgrep on PATH that finds exactly the process named $1 ("" finds nothing).
fake_pgrep() {
  # shellcheck disable=SC2016 # the fake's own $2, written literally
  printf '#!/bin/sh\n[ "$2" = "%s" ]\n' "$1" > "$TESTROOT/bin/pgrep"
  chmod +x "$TESTROOT/bin/pgrep"
}

# with_fakes COMMAND...: COMMAND with the fakes first on PATH (and bash's command cache cleared).
with_fakes() {
  PATH="$TESTROOT/bin:$PATH"
  hash -r
  "$@"
}

cfg() { printf '[server]\nlogin_bind = "%s"\napi_bind = "127.0.0.1:7801"\n[store]\npath = "%s"\n' "$1" "$2" > "$TESTROOT/fenji.toml"; }
echo "a long password" > "$TESTROOT/password"
cfg 127.0.0.1:7800 "$WORK/data/fenji.db"
GOOD=(--server "$TESTROOT/fenji.toml" --server-data "$WORK/data" --username tester --password-file "$TESTROOT/password")

# --- arguments
says 2 "" "run with no name is a usage error" -- cmd_bare_run
says 2 "" "an unknown option is a usage error" -- cmd_bare_run t --print
says 1 "bad run name" "a run name with a slash is refused" -- cmd_bare_run ../t "${GOOD[@]}"
says 2 "--server is required" "a run needs a local fenji-server" -- cmd_bare_run t --server-data "$WORK/data" --username tester --password-file "$TESTROOT/password"
says 2 "--server-data is required" "a run needs the server's store folder" -- cmd_bare_run t --server "$TESTROOT/fenji.toml" --username tester --password-file "$TESTROOT/password"
says 2 "--username" "a username with a space is refused" -- cmd_bare_run t --server "$TESTROOT/fenji.toml" --server-data "$WORK/data" --username "a b" --password-file "$TESTROOT/password"
says 2 "--password-file" "an unreadable password file is refused" -- cmd_bare_run t --server "$TESTROOT/fenji.toml" --server-data "$WORK/data" --username tester --password-file "$TESTROOT/none"
says 2 "--key" "a key press after the timeout is refused" -- cmd_bare_run t "${GOOD[@]}" --timeout 60 --key 90:Return
cfg 0.0.0.0:7800 "$WORK/data/fenji.db"
says 2 "must be 127.0.0.1" "a server that listens beyond loopback is refused" -- cmd_bare_run t "${GOOD[@]}"
cfg 127.0.0.1:7800 "$TESTROOT/fenji.db"
says 2 "inside" "a store outside --server-data is refused" -- cmd_bare_run t "${GOOD[@]}"
cfg 127.0.0.1:7800 "$WORK/runs/old/fenji.db"
says 2 "a run's or the prefix's" "a store in a run's folder is refused" -- cmd_bare_run t --server "$TESTROOT/fenji.toml" --server-data "$WORK/runs/old" --username tester --password-file "$TESTROOT/password"
cfg 127.0.0.1:7800 "$WORK/data/fenji.db"

# --- guards, after the arguments
fake_pgrep steam
says 1 "Steam is running" "Steam running refuses the run" -- with_fakes host_guards
fake_pgrep steamwebhelper
says 1 "Steam is running" "Steam's web helper running refuses the run" -- with_fakes host_guards
fake_pgrep wineserver
says 1 "Wine processes" "Wine running refuses the run" -- with_fakes host_guards
fake_pgrep ""
says 1 "umu-run not found" "good arguments reach the guards, which stop at a missing umu-run" -- with_fakes cmd_bare_run t "${GOOD[@]}"

# --- the command lines that carry the guards
mapfile -t got < <(namespace_cmd)
want=(bwrap --dev-bind / / --proc /proc --unshare-net --unshare-pid --die-with-parent --)
if [ "${got[*]}" = "${want[*]}" ]; then ok "the namespace: real files, its own /proc, loopback only, its own PIDs"; else bad "namespace: ${got[*]}"; fi
mapfile -t got < <(watch_cmd "$TESTROOT/out" "$TESTROOT/io" 4242)
case " ${got[*]} " in
  *" watch --root $WORK --out $TESTROOT/out --sandbox-pid 4242 "*" --no-tmpfs-scan "*) ok "the watchdog watches the namespace, without the tmpfs scan" ;;
  *) bad "watch: ${got[*]}" ;;
esac
mapfile -t got < <(launch_args 7801 tester "$TESTROOT/password" "$WORK/runs/t/client")
case " ${got[*]} " in
  *gf-sdk* | *" --print "*) bad "fenji-launch gets an SDK flag or --print: ${got[*]}" ;;
  " play --server http://127.0.0.1:7801 --username tester --password-file $TESTROOT/password --client $WORK/runs/t/client --prefix $WORK/compat "*)
    ok "fenji-launch plays from the run's copy in the dedicated prefix, with no SDK flag" ;;
  *) bad "launch: ${got[*]}" ;;
esac

# --- the verdict
says 7 "didn't start the client" "fenji-launch stopping before the start is exit 7" -- bare_verdict 0 95 0 "$TESTROOT/out"
says 3 "GATE EVENT" "a watchdog trigger is a gate event" -- bare_verdict 3 137 1 "$TESTROOT/out"
says 5 "never armed" "a watchdog that never armed is exit 5" -- bare_verdict 0 98 0 "$TESTROOT/out"
guard
if (bare_verdict 0 0 1 "$TESTROOT/out") > /dev/null 2>&1; then ok "a client that ran is a finished run"; else bad "a finished run"; fi

if [ "$fails" -gt 0 ]; then echo "$fails failed"; exit 1; fi
echo "all passed"
