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

# env_pinned DIR COMMAND...: COMMAND with DIR as the pinned copy and HOME in the test root.
env_pinned() {
  PINNED_COPY=$1 HOME=$TESTROOT/home
  shift
  "$@"
}

# with_fakes COMMAND...: COMMAND with the fakes first on PATH (and bash's command cache cleared).
with_fakes() {
  PATH="$TESTROOT/bin:$PATH"
  hash -r
  "$@"
}

# cfg LOGIN_BIND STORE [PUBLIC_HOST]: a fenji.toml for the tests.
cfg() { printf '[server]\npublic_host = "%s"\nlogin_bind = "%s"\napi_bind = "127.0.0.1:7801"\n[store]\npath = "%s"\n' "${3:-127.0.0.1}" "$1" "$2" > "$TESTROOT/fenji.toml"; }
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
cfg 127.0.0.1:7800 "$WORK/data/fenji.db" play.example.net
says 2 "public_host must be 127.0.0.1" "a server named by anything but 127.0.0.1 is refused" -- cmd_bare_run t "${GOOD[@]}"
cfg 127.0.0.1:7800 "$WORK/data/fenji.db"

# --- guards, after the arguments (they read uname, so Linux only)
if [ "$(uname -s)" = Linux ]; then
fake_pgrep steam
says 1 "Steam is running" "Steam running refuses the run" -- with_fakes host_guards
fake_pgrep steamwebhelper
says 1 "Steam is running" "Steam's web helper running refuses the run" -- with_fakes host_guards
fake_pgrep wineserver
says 1 "Wine processes" "Wine running refuses the run" -- with_fakes host_guards
fake_pgrep ""
says 1 "umu-run not found" "good arguments reach the guards, which stop at a missing umu-run" -- with_fakes cmd_bare_run t "${GOOD[@]}"
else
  echo "skip the guard tests: they need Linux"
fi

# --- the pinned copy: never a link, never holding one, never in a Steam library
mkdir -p "$TESTROOT/pinned/bin64" "$TESTROOT/home/.steam/steam/steamapps/common/SoLO/bin64"
touch "$TESTROOT/pinned/bin64/gujianol.exe" "$TESTROOT/home/.steam/steam/steamapps/common/SoLO/bin64/gujianol.exe"
ln -s "$TESTROOT/pinned" "$TESTROOT/pinned-link"
says 1 "FENJI_CLIENT_PATH is unset" "no pinned copy named is refused" -- env_pinned "" check_pinned_copy
says 1 "is a link" "a pinned copy that is a link is refused" -- env_pinned "$TESTROOT/pinned-link" check_pinned_copy
says 1 "in a Steam library" "a pinned copy in a Steam library is refused" -- env_pinned "$TESTROOT/home/.steam/steam/steamapps/common/SoLO" check_pinned_copy
ln -s /etc/hostname "$TESTROOT/pinned/bin64/extra"
says 1 "holds a link" "a pinned copy holding a link is refused" -- env_pinned "$TESTROOT/pinned" check_pinned_copy
rm "$TESTROOT/pinned/bin64/extra"
if (env_pinned "$TESTROOT/pinned" check_pinned_copy) > /dev/null 2>&1; then ok "a plain pinned copy is accepted"; else bad "a plain pinned copy"; fi

# --- the command lines that carry the guards
mkdir -p "$TESTROOT/rt" "$TESTROOT/nohome"
mapfile -t got < <(XDG_RUNTIME_DIR=$TESTROOT/rt HOME=$TESTROOT/nohome namespace_cmd 99)
want=(bwrap --dev-bind / / --proc /proc --tmpfs /run --perms 0700 --dir "$TESTROOT/rt" --tmpfs /tmp
  --bind /tmp/.X11-unix/X99 /tmp/.X11-unix/X99 --tmpfs /dev/shm --unshare-net --unshare-pid --die-with-parent
  --seccomp 7 --)
if [ "${got[*]}" = "${want[*]}" ]; then
  ok "the namespace: real files, its own /proc, no host /run, /tmp or /dev/shm, loopback only, its own PIDs, the seccomp filter"
else
  bad "namespace: ${got[*]}"
fi
mkdir -p "$TESTROOT/rt/pulse" "$TESTROOT/home2/.steam"
touch "$TESTROOT/rt/pipewire-0" "$TESTROOT/home2/.steam/steam.pipe"
mapfile -t got < <(XDG_RUNTIME_DIR=$TESTROOT/rt HOME=$TESTROOT/home2 namespace_cmd 99)
case " ${got[*]} " in
  *" --bind $TESTROOT/rt/pipewire-0 $TESTROOT/rt/pipewire-0 --bind $TESTROOT/rt/pulse $TESTROOT/rt/pulse "*" --ro-bind /dev/null $TESTROOT/home2/.steam/steam.pipe "*)
    ok "audio sockets come back, and Steam's pipe is masked" ;;
  *) bad "namespace with audio and a Steam pipe: ${got[*]}" ;;
esac
mapfile -t got < <(namespace_env 99)
want=(env -u WAYLAND_DISPLAY -u DBUS_SESSION_BUS_ADDRESS -u SSH_AUTH_SOCK -u FENJI_PASSWORD DISPLAY=:99)
if [ "${got[*]}" = "${want[*]}" ]; then ok "inside: no session bus, no agents, no Wayland, no password"; else bad "env: ${got[*]}"; fi
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

# --- the namespace for real, where it can be made: probes run inside it
if [ "$(uname -s)/$(uname -m)" = Linux/x86_64 ] && command -v bwrap > /dev/null && [ -S /tmp/.X11-unix/X99 ] &&
  bwrap --dev-bind / / --unshare-net --unshare-pid true 2> /dev/null; then
  mapfile -t ns < <(namespace_cmd 99)
  mapfile -t ne < <(namespace_env 99)
  bpf=$(seccomp_filter)
  # shellcheck disable=SC2016 # the probe expands its own variables, inside
  probe=$("${ns[@]}" "${ne[@]}" sh -c '
    [ -e /run/dbus/system_bus_socket ] || echo "no system bus"
    [ -e /run/systemd/resolve/io.systemd.Resolve ] || echo "no resolver socket"
    [ -e "$XDG_RUNTIME_DIR/bus" ] || echo "no session bus"
    [ -z "$(ls -A /dev/shm)" ] && echo "empty /dev/shm"
    [ -S /tmp/.X11-unix/X99 ] && echo "the display"
    [ "$(tail -n +3 /proc/net/dev | cut -d: -f1 | tr -d " ")" = lo ] && echo "loopback only"
    getent hosts example.org > /dev/null || echo "no name lookups"
    python3 -I -c "import ctypes,sys; c=ctypes.CDLL(None, use_errno=True); r=c.syscall(248, None, None, None, 0, -3); sys.exit(0 if r == -1 and ctypes.get_errno() == 1 else 1)" && echo "no key requests"
  ' 7< <(printf '%b' "$bpf") 2>&1)
  for want in "no system bus" "no resolver socket" "no session bus" "empty /dev/shm" "the display" "loopback only" "no name lookups" "no key requests"; do
    if grep -qx "$want" <<< "$probe"; then ok "inside the namespace: $want"; else bad "inside the namespace: $want missing ($probe)"; fi
  done
else
  echo "skip the live namespace probes: they need bwrap, namespaces and the nested display :99"
fi

if [ "$fails" -gt 0 ]; then echo "$fails failed"; exit 1; fi
echo "all passed"
