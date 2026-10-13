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

# --- --desktop: the session's own display, as a player has it
# with_env NAME=VALUE COMMAND...: COMMAND with NAME set (an empty VALUE unsets it).
with_env() {
  local kv=$1
  shift
  if [ -n "${kv#*=}" ]; then export "${kv?}"; else unset "${kv%%=*}"; fi
  "$@"
}
says 2 "needs a local DISPLAY" "--desktop refuses a remote DISPLAY" -- with_env DISPLAY=otherhost:0 desktop_display
says 2 "needs a local DISPLAY" "--desktop refuses an unset DISPLAY" -- with_env DISPLAY= desktop_display
if [ "$(with_env DISPLAY=:1.0 desktop_display)" = 1 ]; then ok "--desktop takes the display number from DISPLAY"; else bad "desktop display :1.0"; fi
touch "$TESTROOT/xauth"
if [ -z "$(with_env XAUTHORITY= desktop_xauth)" ]; then ok "no XAUTHORITY: nothing to bring in"; else bad "xauth when unset"; fi
if [ "$(with_env XAUTHORITY="$TESTROOT/xauth" desktop_xauth)" = "$TESTROOT/xauth" ]; then ok "the display's auth file is brought in"; else bad "xauth file"; fi
says 2 "XAUTHORITY" "an XAUTHORITY that isn't a readable file is refused" -- with_env XAUTHORITY="$TESTROOT/none" desktop_xauth
says 2 "--key, --type or --click" "--desktop refuses key presses and clicks" -- with_env DISPLAY=:0 cmd_bare_run t "${GOOD[@]}" --desktop --key 5:Return
says 2 "--key, --type or --click" "--desktop refuses a click given before it" -- with_env DISPLAY=:0 cmd_bare_run t "${GOOD[@]}" --click 5:10,10 --desktop
says 2 "needs a local DISPLAY" "--desktop takes ASCII digits only" -- with_env DISPLAY=$':\u0663' desktop_display
mkdir -p "$TESTROOT/xauthdir"
mkfifo "$TESTROOT/xauthfifo"
says 2 "XAUTHORITY" "an XAUTHORITY that is a directory is refused" -- with_env XAUTHORITY="$TESTROOT/xauthdir" desktop_xauth
says 2 "XAUTHORITY" "an XAUTHORITY that is a FIFO is refused" -- with_env XAUTHORITY="$TESTROOT/xauthfifo" desktop_xauth
# in_dir DIR COMMAND...: COMMAND run from DIR.
in_dir() {
  cd -- "$1" || return 1
  shift
  "$@"
}
says 2 "absolute" "a relative XAUTHORITY is refused" -- in_dir "$TESTROOT" with_env XAUTHORITY=xauth desktop_xauth
says 2 "don't go together" "setup: --desktop and --display don't go together" -- with_env DISPLAY=:0 cmd_bare_setup --desktop --display 98
chose=$(display=99 xauth=""; with_env DISPLAY=:0 with_env XAUTHORITY="$TESTROOT/xauth" choose_display 1 0; echo "$display $xauth")
if [ "$chose" = "0 $TESTROOT/xauth" ]; then ok "--desktop chooses the session's display and its auth file"; else bad "choose desktop: $chose"; fi
chose=$(display=98 xauth=""; choose_display 0 1; echo "$display ${xauth:-none}")
if [ "$chose" = "98 none" ]; then ok "without --desktop the nested display stands, with no auth file"; else bad "choose nested: $chose"; fi
says 2 "don't go together" "--desktop and --display don't go together" -- with_env DISPLAY=:0 cmd_bare_run t "${GOOD[@]}" --desktop --display 0
mkdir -p "$TESTROOT/rt2"
mapfile -t got < <(XDG_RUNTIME_DIR=$TESTROOT/rt2 HOME=$TESTROOT/nohome namespace_cmd 0 "$TESTROOT/xauth")
want=(bwrap --dev-bind / / --proc /proc --tmpfs /run --perms 0700 --dir "$TESTROOT/rt2" --tmpfs /tmp
  --bind /tmp/.X11-unix/X0 /tmp/.X11-unix/X0 --tmpfs /dev/shm --ro-bind "$TESTROOT/xauth" "$TESTROOT/xauth"
  --unshare-net --unshare-pid --die-with-parent --seccomp 7 --)
if [ "${got[*]}" = "${want[*]}" ]; then ok "a desktop run brings in the display's auth file, read-only, after the tmpfs mounts"; else bad "desktop namespace: ${got[*]}"; fi
mapfile -t got < <(namespace_env 0 "$TESTROOT/xauth")
want=(env -u WAYLAND_DISPLAY -u DBUS_SESSION_BUS_ADDRESS -u SSH_AUTH_SOCK -u FENJI_PASSWORD DISPLAY=:0 XAUTHORITY="$TESTROOT/xauth")
if [ "${got[*]}" = "${want[*]}" ]; then ok "inside a desktop run: the session's display and its auth file"; else bad "desktop env: ${got[*]}"; fi
inputs=(stale)
bare_inputs 1 "$TESTROOT/out" 0 60
if [ "${#inputs[@]}" = 0 ]; then ok "a desktop run takes no screenshots and sends no input"; else bad "desktop inputs: ${inputs[*]}"; fi
inputs=()
bare_inputs 0 "$TESTROOT/out" 99 60
if [ "${#inputs[@]}" = 3 ]; then ok "a nested-display run starts its screenshots, clicks and keys"; else bad "nested inputs: ${inputs[*]-}"; fi
kill "${inputs[@]}" 2> /dev/null
wait "${inputs[@]}" 2> /dev/null

# --- the verdict
says 7 "didn't start the client" "fenji-launch stopping before the start is exit 7" -- bare_verdict 0 95 0 "$TESTROOT/out"
says 3 "GATE EVENT" "a watchdog trigger is a gate event" -- bare_verdict 3 137 1 "$TESTROOT/out"
says 5 "never armed" "a watchdog that never armed is exit 5" -- bare_verdict 0 98 0 "$TESTROOT/out"
guard
if (bare_verdict 0 0 1 "$TESTROOT/out") > /dev/null 2>&1; then ok "a client that ran is a finished run"; else bad "a finished run"; fi
mkdir -p "$TESTROOT/out"
touch "$TESTROOT/out/steam-started"
says 3 "GATE EVENT" "a gate event wins over Steam starting" -- bare_verdict 3 137 1 "$TESTROOT/out"
says 4 "WATCHDOG FAILURE" "a watchdog failure wins over Steam starting" -- bare_verdict 4 137 1 "$TESTROOT/out"
says 1 "Steam started" "Steam starting stops an otherwise clean run" -- bare_verdict 0 137 0 "$TESTROOT/out"
says 3 "Steam also started" "a gate event still reports that Steam started" -- bare_verdict 3 137 1 "$TESTROOT/out"
rm -f -- "$TESTROOT/out/steam-started"

# --- the prefix: primed through Proton's launcher, so Proton's own service is in the watchdog's baseline
mkdir -p "$TESTROOT/proton/files/lib/wine/discord-rpc-bridge" "$TESTROOT/proton-nobridge" "$WORK/compat/drive_c/windows"
touch "$TESTROOT/proton/files/lib/wine/discord-rpc-bridge/bridge.exe"
echo "1789520217 GE-Proton-test" > "$TESTROOT/proton/version"
echo "1789000000 GE-Proton-nobridge" > "$TESTROOT/proton-nobridge/version"
printf '#!/bin/sh\n' > "$TESTROOT/proton/proton"
printf '#!/bin/sh\n' > "$TESTROOT/umu-run"
chmod +x "$TESTROOT/proton/proton" "$TESTROOT/umu-run"
printf 'WINE REGISTRY Version 2\n[System\\\\ControlSet001\\\\Services\\\\BITS] 1789520096\n' > "$WORK/compat/system.reg"
# with_proton DIR COMMAND...: COMMAND with DIR as GE-Proton and a umu-run that does nothing.
with_proton() {
  PROTON=$1 UMU=$TESTROOT/umu-run
  shift
  "$@"
}
primed() { (with_proton "$1" prefix_primed); }
if primed "$TESTROOT/proton"; then bad "a prefix never primed counts as primed"; else ok "a prefix that Proton's launcher never ran in isn't primed"; fi
cp -- "$TESTROOT/proton/version" "$WORK/compat/fenji-primed"
if primed "$TESTROOT/proton"; then bad "a mark alone counts as primed"; else ok "the mark without Proton's bridge service on disk isn't primed"; fi
cp -- "$TESTROOT/proton-nobridge/version" "$WORK/compat/fenji-primed"
if primed "$TESTROOT/proton-nobridge"; then ok "a GE-Proton with no bridge needs only its mark"; else bad "the mark with no bridge shipped"; fi
printf '[System\\\\ControlSet001\\\\Services\\\\discord-bridge] 1791858207\n' >> "$WORK/compat/system.reg"
if primed "$TESTROOT/proton"; then bad "a prefix primed by another GE-Proton build counts as primed"; else ok "a prefix primed by another GE-Proton build isn't primed"; fi
: > "$WORK/compat/fenji-primed"
if primed "$TESTROOT/proton"; then bad "a mark naming no build counts as primed"; else ok "a mark that names no GE-Proton build isn't primed"; fi
cp -- "$TESTROOT/proton/version" "$WORK/compat/fenji-primed"
if primed "$TESTROOT/proton"; then ok "this GE-Proton build's mark and the bridge service on disk: primed"; else bad "a primed prefix"; fi
rm -f -- "$WORK/compat/fenji-primed"
mapfile -t got < <(PROTON=$TESTROOT/proton setup_env)
want=(UMU_RUNTIME_UPDATE=0 "PROTONPATH=$TESTROOT/proton" "WINEPREFIX=$WORK/compat" GAMEID=umu-1418100 PROTON_USE_XALIA=0)
if [ "${got[*]}" = "${want[*]}" ]; then ok "setup's umu-run: no runtime update, the pinned GE-Proton, the prefix, no Xalia (as in every run)"; else bad "setup env: ${got[*]}"; fi
mapfile -t got < <(prime_args)
want=("$WORK/compat/drive_c/windows/system32/cmd.exe" /c exit)
if [ "${got[*]}" = "${want[*]}" ]; then
  ok "the prime starts the prefix's cmd.exe by its unix path, which Proton starts through its launcher"
else
  bad "prime: ${got[*]}"
fi
if [ "$(uname -s)/$(uname -m)" = Linux/x86_64 ]; then
  fake_pgrep ""
  says 1 "hasn't been through Proton's launcher" "a run refuses a prefix Proton's launcher never ran in" -- \
    with_fakes with_proton "$TESTROOT/proton" cmd_bare_run t "${GOOD[@]}"
  cp -- "$TESTROOT/proton-nobridge/version" "$WORK/compat/fenji-primed"
  says 1 "another GE-Proton build" "a run names a prefix primed by another GE-Proton build" -- \
    with_fakes with_proton "$TESTROOT/proton" cmd_bare_run t "${GOOD[@]}"
  rm -f -- "$WORK/compat/fenji-primed"
else
  echo "skip the unprimed-prefix run test: it needs Linux on x86_64"
fi

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
