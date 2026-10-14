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
# run-bare.sh runs the client on the Linux game box only, and needs GNU tools such as realpath -m,
# which macOS's realpath lacks.
if [ "$(uname -s)" != Linux ]; then
  echo "skip: run-bare.sh and its tests are for Linux"
  exit 0
fi
# shellcheck source=run-bare.sh
. "$HERE/run-bare.sh"
set +e # run-bare.sh sets -e; these tests check exit statuses themselves
unset DISPLAY XDG_CACHE_HOME # set where a test needs them, so the caller's session can't change a result
TESTROOT=$(mktemp -d) || { echo "FATAL: mktemp failed" >&2; exit 2; }
readonly TESTROOT
[ -d "$TESTROOT" ] || { echo "FATAL: no test root" >&2; exit 2; }
# Only the test shell itself removes the root, and only when it can tell it is the test shell: a
# subshell signalled before it resets its traps runs this one, and its first command can fail.
trap '[ "$BASHPID" = "$$" ] && rm -rf -- "$TESTROOT"' EXIT
WORK=$TESTROOT/work REPO=$TESTROOT/repo PINNED_COPY=$TESTROOT/nonexistent UMU=$TESTROOT/nonexistent
BARE_PROC=$TESTROOT/proc
PROTON=$TESTROOT/nonexistent STUBBIN=$TESTROOT/nonexistent WATCHDOG=$TESTROOT/nonexistent
# The namespace's own tmpfs mounts, as run-bare.sh names them, moved under TESTROOT (itself in /tmp).
NS_TMPFS_REAL=("${NS_TMPFS[@]}")
NS_TMPFS=("$TESTROOT/ns")
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
NESTED=(--display 98)

# --- arguments
says 2 "" "run with no name is a usage error" -- cmd_bare_run
says 2 "" "an unknown option is a usage error" -- cmd_bare_run t --print
says 1 "bad run name" "a run name with a slash is refused" -- cmd_bare_run ../t "${GOOD[@]}"
says 2 "--server is required" "a run needs a local fenji-server" -- cmd_bare_run t --server-data "$WORK/data" --username tester --password-file "$TESTROOT/password"
says 2 "--server-data is required" "a run needs the server's store folder" -- cmd_bare_run t --server "$TESTROOT/fenji.toml" --username tester --password-file "$TESTROOT/password"
says 2 "--username" "a username with a space is refused" -- cmd_bare_run t --server "$TESTROOT/fenji.toml" --server-data "$WORK/data" --username "a b" --password-file "$TESTROOT/password"
says 2 "--password-file" "an unreadable password file is refused" -- cmd_bare_run t --server "$TESTROOT/fenji.toml" --server-data "$WORK/data" --username tester --password-file "$TESTROOT/none"
says 2 "--key" "a key press after the timeout is refused" -- cmd_bare_run t "${GOOD[@]}" --timeout 60 --key 90:Return
says 2 "ends after the --timeout" "a held key that outlasts the timeout is refused" -- cmd_bare_run t "${GOOD[@]}" --timeout 60 --key 59:w/1001
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
says 2 "--display N or --desktop" "a run names its display: no default" -- with_fakes cmd_bare_run t "${GOOD[@]}"
says 1 "umu-run not found" "good arguments reach the guards, which stop at a missing umu-run" -- with_fakes cmd_bare_run t "${GOOD[@]}" "${NESTED[@]}"
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
# A home reached through a link (/home -> /var/home on Fedora Atomic): the copy's resolved path is
# compared with the resolved home (#148).
mkdir -p "$TESTROOT/realhome/.local/share/Steam/library/SoLO/bin64"
touch "$TESTROOT/realhome/.local/share/Steam/library/SoLO/bin64/gujianol.exe"
ln -s "$TESTROOT/realhome" "$TESTROOT/homelink"
# env_pinned_home HOME COPY COMMAND...: COMMAND with that home and that pinned copy.
env_pinned_home() {
  HOME=$1 PINNED_COPY=$2
  shift 2
  "$@"
}
says 1 "in a Steam library" "a Steam library under a home reached through a link is refused" -- \
  env_pinned_home "$TESTROOT/homelink" "$TESTROOT/homelink/.local/share/Steam/library/SoLO" check_pinned_copy

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
mkdir -p "$TESTROOT/rt/pulse" "$TESTROOT/home2/.steam" "$TESTROOT/home2/.cache/ibus"
touch "$TESTROOT/rt/pipewire-0" "$TESTROOT/home2/.steam/steam.pipe"
mapfile -t got < <(XDG_RUNTIME_DIR=$TESTROOT/rt HOME=$TESTROOT/home2 namespace_cmd 99)
case " ${got[*]} " in
  *" --bind $TESTROOT/rt/pipewire-0 $TESTROOT/rt/pipewire-0 --bind $TESTROOT/rt/pulse $TESTROOT/rt/pulse "*" --ro-bind /dev/null $TESTROOT/home2/.steam/steam.pipe --tmpfs $TESTROOT/home2/.cache/ibus "*)
    ok "audio sockets come back, Steam's pipe is masked, and IBus's sockets are hidden" ;;
  *) bad "namespace with audio, a Steam pipe and IBus: ${got[*]}" ;;
esac
mkdir -p "$TESTROOT/cache2/ibus"
mapfile -t got < <(XDG_CACHE_HOME=$TESTROOT/cache2 XDG_RUNTIME_DIR=$TESTROOT/rt HOME=$TESTROOT/home2 namespace_cmd 99)
case " ${got[*]} " in
  *" --tmpfs $TESTROOT/home2/.cache/ibus --tmpfs $TESTROOT/cache2/ibus "*)
    ok "IBus's sockets are hidden under XDG_CACHE_HOME too" ;;
  *) bad "namespace with XDG_CACHE_HOME: ${got[*]}" ;;
esac
mapfile -t got < <(namespace_env 99)
want=(env -u WAYLAND_DISPLAY -u DBUS_SESSION_BUS_ADDRESS -u SSH_AUTH_SOCK -u FENJI_PASSWORD DISPLAY=:99)
if [ "${got[*]}" = "${want[*]}" ]; then ok "inside: no session bus, no agents, no Wayland, no password"; else bad "env: ${got[*]}"; fi
mapfile -t ns < <(XDG_RUNTIME_DIR=$TESTROOT/rt HOME=$TESTROOT/home2 namespace_cmd 99)
mapfile -t got < <(watch_cmd "$TESTROOT/out" "$TESTROOT/io" 4242 "${ns[@]}")
want=(python3 -I "$WATCHDOG" watch --root "$WORK" --prefix "$WORK/compat" --out "$TESTROOT/out" --sandbox-pid 4242
  --log "$TESTROOT/io/steam-1418100.log" --go "$TESTROOT/io/.go" --tmpfs /run --bind "$TESTROOT/rt/pipewire-0"
  --bind "$TESTROOT/rt/pulse" --tmpfs /tmp --bind /tmp/.X11-unix/X99 --tmpfs /dev/shm
  --bind "$TESTROOT/home2/.steam/steam.pipe" --tmpfs "$TESTROOT/home2/.cache/ibus")
if [ "${got[*]}" = "${want[*]}" ]; then
  ok "the watchdog watches the namespace and its prefix, and scans its tmpfs mounts less what it binds in"
else
  bad "watch: ${got[*]}"
fi
mapfile -t got < <(baseline_cmd "$TESTROOT/out")
want=(python3 -I "$WATCHDOG" baseline --root "$WORK" --prefix "$WORK/compat" --out "$TESTROOT/out")
if [ "${got[*]}" = "${want[*]}" ]; then ok "the baseline names the prefix too"; else bad "baseline: ${got[*]}"; fi
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
if locale -a 2> /dev/null | grep -qix 'en_US.utf-\?8'; then wide=en_US.UTF-8; else wide=C.UTF-8; fi
says 2 "needs a local DISPLAY" "--desktop takes ASCII digits only" -- with_env LC_ALL="$wide" with_env DISPLAY=$':\xd9\xa3' desktop_display
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
mapfile -t ns < <(XDG_RUNTIME_DIR=$TESTROOT/rt2 HOME=$TESTROOT/nohome namespace_cmd 0 "$TESTROOT/xauth")
mapfile -t got < <(watch_cmd "$TESTROOT/out" "$TESTROOT/io" 4242 "${ns[@]}")
case " ${got[*]} " in
  *" --tmpfs /run --tmpfs /tmp --bind /tmp/.X11-unix/X0 --tmpfs /dev/shm --bind $TESTROOT/xauth ") ok "a desktop run's auth file is left out of the tmpfs scan" ;;
  *) bad "desktop watch: ${got[*]}" ;;
esac
mapfile -t got < <(namespace_env 0 "$TESTROOT/xauth")
want=(env -u WAYLAND_DISPLAY -u DBUS_SESSION_BUS_ADDRESS -u SSH_AUTH_SOCK -u FENJI_PASSWORD DISPLAY=:0 XAUTHORITY="$TESTROOT/xauth")
if [ "${got[*]}" = "${want[*]}" ]; then ok "inside a desktop run: the session's display and its auth file"; else bad "desktop env: ${got[*]}"; fi
inputs=(stale)
bare_inputs 1 "$TESTROOT/out" 0 60
if [ "${#inputs[@]}" = 0 ]; then ok "a desktop run takes no screenshots and sends no input"; else bad "desktop inputs: ${inputs[*]}"; fi
# start_inputs stands in here, so the test starts no background jobs of its own.
started=$(start_inputs() { echo "$*" > "$TESTROOT/started-with"; shots=11 clicker=12 typist=13; }; inputs=(); bare_inputs 0 "$TESTROOT/out" 98 60; echo "${inputs[*]-}")
if [ "$started" = "11 12 13" ] && [ "$(cat -- "$TESTROOT/started-with")" = "$TESTROOT/out 98 60" ]; then
  ok "a nested-display run starts its screenshots, clicks and keys, on its own display"
else
  bad "nested inputs: $started / $(cat -- "$TESTROOT/started-with" 2> /dev/null)"
fi

# --- a nested display must be a dedicated X server, never the session's own (fake /proc, /tmp)
# xserver PID COMM ARGS...: a process in the fake /proc: its comm, its NUL-separated cmdline, and a
# libXfont in its maps that has no font-server support (readable through its root).
xserver() {
  local pid=$1 comm=$2
  shift 2
  mkdir -p "$BARE_PROC/$pid/root/usr/lib64"
  echo "$comm" > "$BARE_PROC/$pid/comm"
  printf '%s\0' "$@" > "$BARE_PROC/$pid/cmdline"
  echo "7f0000000000-7f0000001000 r--p 00000000 00:00 0    /usr/lib64/libXfont2.so.2.0.0" > "$BARE_PROC/$pid/maps"
  echo "a font library that opens no files over the network" > "$BARE_PROC/$pid/root/usr/lib64/libXfont2.so.2.0.0"
}
# lock N PID: display N's lock file, naming PID; sock N: its socket.
lock() { printf '%10d\n' "$2" > "$X_LOCKS/.X$1-lock"; }
sock() { python3 -I -c 'import socket, sys; socket.socket(socket.AF_UNIX).bind(sys.argv[1])' "$X_SOCKDIR/X$1"; }
X_LOCKS=$TESTROOT/xlocks X_SOCKDIR=$TESTROOT/x11
mkdir -p "$X_LOCKS" "$X_SOCKDIR"
chmod 1777 "$X_SOCKDIR"
xserver 4101 Xwayland Xwayland :98 -geometry 1920x1080 -ac -nolisten tcp -noreset -extension MIT-SHM
xserver 4102 Xwayland Xwayland :97 -geometry 1280x720 -ac -nolisten tcp -noreset -extension MIT-SHM -shm
xserver 4103 Xwayland Xwayland :0 -rootless -noreset -accessx -core -listenfd 4 -enable-ei-portal
xserver 4104 gnome-shell /usr/bin/gnome-shell
xserver 4105 Xwayland Xwayland :96 -ac -enable-ei-portal
xserver 4106 Xwayland Xwayland :88 -ac
echo "calls getaddrinfo and connect" > "$BARE_PROC/4106/root/usr/lib64/libXfont2.so.2.0.0"
xserver 4107 Xwayland Xwayland :87 -ac
: > "$BARE_PROC/4107/maps"
for n in 98 97 0 1 96 95 94 93 90 89 88 87; do sock "$n"; done
lock 98 4101; lock 97 4102; lock 0 4103; lock 1 4104; lock 96 4105; lock 95 999999; lock 88 4106; lock 87 4107
printf 'garbage\n' > "$X_LOCKS/.X94-lock"
touch "$X_SOCKDIR/X91"
lock 91 4101
mkfifo "$X_LOCKS/.X90-lock"
ln -s "$X_LOCKS/.X98-lock" "$X_LOCKS/.X89-lock"
# check_in_child N [CODE]: bare_display_problem N, in good time, in a child shell with the test
# root's paths and, before the check, CODE (stand-ins, other paths). Its standard output is the
# reason, as nested_display_ok reads it, and its status is the check's.
check_in_child() {
  # shellcheck disable=SC2016 # the bash -c script expands its own positional parameters
  timeout 5 bash -c '. "$1"; WORK=$2/work REPO=$2/repo X_LOCKS=$3 X_SOCKDIR=$4 BARE_PROC=$5
    eval "$7"; bare_display_problem "$6"' _ \
    "$HERE/run-bare.sh" "$TESTROOT" "$X_LOCKS" "$X_SOCKDIR" "$BARE_PROC" "$1" "${2-}"
}
# problem N PATTERN WHAT [CODE]: display N is refused, with PATTERN in the reason.
problem() {
  local why
  why=$(check_in_child "$1" "${4-}")
  case "$why" in *"$2"*) ok "$3" ;; *) bad "$3: '$why'" ;; esac
}
# accepted N WHAT [CODE]: display N passes: the check ends with status 0 and gives no reason.
accepted() {
  local why rc
  why=$(check_in_child "$1" "${3-}" 2>&1)
  rc=$?
  if [ "$rc" = 0 ] && [ -z "$why" ]; then ok "$2"; else bad "$2: exit $rc: '$why'"; fi
}
# foreign PATH: CODE for problem that makes stat report PATH as uid 4242's (or ROOT=1: root's).
foreign() {
  # shellcheck disable=SC2016 # expanded in problem's child shell
  printf 'stat() { if [ "${!#}" = %q ]; then local u=4242; [ -z "${ROOT:-}" ] || u=0; printf "%%s %%s\\n" "$u" "$(command stat -c %%f -- "${!#}")"; else command stat "$@"; fi; }\n' "$1"
}
accepted 98 "a dedicated rootful Xwayland with glamor on is a nested display"
accepted 97 "the sandbox's kind of display, glamor off, is one too"
problem 0 rootless "a -rootless session Xwayland is refused"
problem 1 gnome-shell "a display the compositor locks is refused"
problem 96 ei-portal "an Xwayland that passes input to the compositor is refused"
problem 95 "not running" "a lock naming no running process is refused"
problem 94 "process ID" "a lock without a process ID is refused"
problem 93 "no lock" "a display with no lock is refused"
problem 92 "no X server socket" "a display with no socket is refused"
problem 91 "not a socket" "a socket that is a plain file is refused"
problem 90 "not a regular file" "a FIFO as the lock is refused, without blocking"
problem 89 "not a regular file" "a symlink as the lock is refused"
problem 88 "with font-server support" "a server whose libXfont can open sockets is refused"
problem 87 "no libXfont" "a server whose font library can't be checked is refused"
# A second library that can open sockets, after a clean one.
xserver 4110 Xwayland Xwayland :84 -ac
mkdir -p "$BARE_PROC/4110/root/usr/lib"
echo "calls connect" > "$BARE_PROC/4110/root/usr/lib/libXfont.so.1"
echo "7f0000002000-7f0000003000 r--p 00000000 00:00 0    /usr/lib/libXfont.so.1" >> "$BARE_PROC/4110/maps"
sock 84; lock 84 4110
problem 84 "with font-server support" "every libXfont the server maps is checked, not only the first"
# A library replaced since the server started, and one that isn't there to read.
xserver 4108 Xwayland Xwayland :85 -ac
echo "7f0000000000-7f0000001000 r--p 00000000 00:00 0    /usr/lib64/libXfont2.so.2.0.0 (deleted)" > "$BARE_PROC/4108/maps"
sock 85; lock 85 4108
problem 85 "replaced since it started" "a libXfont replaced since the server started is refused"
xserver 4109 Xwayland Xwayland :83 -ac
echo "7f0000000000-7f0000001000 r--p 00000000 00:00 0    /usr/lib64/libXfont2-gone.so.2" > "$BARE_PROC/4109/maps"
sock 83; lock 83 4109
problem 83 "not a readable file" "a libXfont that can't be read here is refused"
problem 98 "grep failed" "a check that can't run refuses" 'grep() { return 2; }'
# A symlink as the socket.
ln -s "$X_SOCKDIR/X98" "$X_SOCKDIR/X86"
lock 86 4101
problem 86 "not a socket" "a symlink as the socket is refused"
# Owners: a socket directory, socket or lock of another user's; a socket directory of root's is fine.
problem 98 "belongs to uid 4242, not root or you" "another user's socket directory is refused" "$(foreign "$X_SOCKDIR")"
problem 98 "belongs to uid 4242, not you" "another user's socket is refused" "$(foreign "$X_SOCKDIR/X98")"
problem 98 "not a regular file of yours" "another user's lock is refused" "$(foreign "$X_LOCKS/.X98-lock")"
accepted 98 "a socket directory of root's is accepted" "ROOT=1; $(foreign "$X_SOCKDIR")"
if [ "$(id -u)" != 0 ]; then
  problem 98 "belongs to uid 0, not you" "a root-owned socket is refused" "ROOT=1; $(foreign "$X_SOCKDIR/X98")"
  problem 98 "not a regular file of yours" "a root-owned lock is refused" "ROOT=1; $(foreign "$X_LOCKS/.X98-lock")"
else
  echo "skip the root-owned socket and lock tests: they need a user other than root"
fi
# The socket directory: sticky, and not a symlink.
mkdir -p "$TESTROOT/x11-plain"
chmod 0777 "$TESTROOT/x11-plain"
python3 -I -c 'import socket, sys; socket.socket(socket.AF_UNIX).bind(sys.argv[1])' "$TESTROOT/x11-plain/X98"
problem 98 "doesn't have the sticky bit" "a socket directory without the sticky bit is refused" "X_SOCKDIR=$(printf %q "$TESTROOT/x11-plain")"
ln -s "$X_SOCKDIR" "$TESTROOT/x11-link"
problem 98 "not a directory" "a socket directory that is a symlink is refused" "X_SOCKDIR=$(printf %q "$TESTROOT/x11-link")"
says 2 "session's own display" "--display naming the session's own display is refused" -- with_env display=98 with_env DISPLAY=:98.0 choose_display 0 1
says 2 "session's own display" "... with DISPLAY as unix:N" -- with_env display=0 with_env DISPLAY=unix:0 choose_display 0 1
says 2 "session's own display" "... with DISPLAY as localhost:N" -- with_env display=0 with_env DISPLAY=localhost:0 choose_display 0 1
says 2 "session's own display" "... with leading zeros" -- with_env display=0 with_env DISPLAY=:00 choose_display 0 1
says 2 "doesn't name a display number" "a DISPLAY that can't be parsed is refused for a nested run" -- with_env display=98 with_env DISPLAY=weird choose_display 0 1
if (with_env display=98 with_env DISPLAY=:5 choose_display 0 1); then ok "another session display doesn't stop a nested one"; else bad "DISPLAY :5 vs 98"; fi
if (nested_display_ok 1 1); then ok "a desktop run skips the nested-display check"; else bad "desktop bypass"; fi
says 2 "needs a local DISPLAY" "setup --desktop with no DISPLAY is refused" -- with_env DISPLAY= cmd_bare_setup --desktop
says 2 "--display N or --desktop" "setup names its display: no default" -- cmd_bare_setup
says 2 "" "setup: an unknown option is a usage error" -- cmd_bare_setup --print
says 1 "display number" "setup: --display takes a number" -- cmd_bare_setup --display x
says 1 "0 to 999" "setup: --display takes no leading zeros" -- cmd_bare_setup --display 098
says 1 "0 to 999" "setup: --display takes 0 to 999" -- cmd_bare_setup --display 1000
says 1 "0 to 999" "a run's --display takes no leading zeros" -- cmd_bare_run t "${GOOD[@]}" --display 098
says 1 "0 to 999" "a run's --display takes 0 to 999" -- cmd_bare_run t "${GOOD[@]}" --display 1000
says 1 "umu-run not found" "setup takes a three-digit --display" -- with_fakes cmd_bare_setup --display 999
says 1 "umu-run not found" "a run takes a three-digit --display" -- with_fakes cmd_bare_run t "${GOOD[@]}" --display 999

# --- host paths the namespace must see: none under its own tmpfs mounts, where they vanish (#148)
mapfile -t ns < <(XDG_RUNTIME_DIR=$TESTROOT/rt HOME=$TESTROOT/nohome namespace_cmd 99)
mounts=()
for ((i = 0; i < ${#ns[@]}; i++)); do [ "${ns[i]}" != --tmpfs ] || mounts+=("${ns[i + 1]}"); done
if [ "${mounts[*]}" = "${NS_TMPFS_REAL[*]:-}" ]; then ok "the hidden places are the namespace's tmpfs mounts"; else bad "NS_TMPFS ${NS_TMPFS_REAL[*]:-} vs ${mounts[*]}"; fi
mkdir -p "$TESTROOT/ns/repo" "$TESTROOT/seen"
ln -s "$TESTROOT/ns/repo" "$TESTROOT/seen/to-ns"
echo "a long password" > "$TESTROOT/ns/password"
if [ "$(uname -s)" = Linux ]; then
  says 1 "re/bare-metal is under $TESTROOT/ns" "a work directory the namespace would hide is refused" -- \
    with_env WORK="$TESTROOT/seen/to-ns/work" with_fakes host_guards
  says 1 "umu-run (FENJI_UMU_RUN) is under" "a umu-run the namespace would hide is refused" -- \
    with_env UMU="$TESTROOT/seen/to-ns/umu-run" with_fakes host_guards
  says 1 "GE-Proton (FENJI_GE_PROTON) is under" "a GE-Proton the namespace would hide is refused" -- \
    with_env PROTON="$TESTROOT/ns/proton" with_fakes host_guards
  says 1 "umu-run not found" "paths the namespace sees pass on to the next guard" -- \
    with_env WORK="$TESTROOT/seen/work" with_fakes host_guards
fi
says 1 "target/release is under $TESTROOT/ns" "built binaries the namespace would hide are refused" -- \
  with_env STUBBIN="$TESTROOT/ns/release" with_fakes cmd_bare_run t "${GOOD[@]}" "${NESTED[@]}"
says 2 "--password-file is under $TESTROOT/ns" "a password file the namespace would hide is refused" -- \
  cmd_bare_run t "${GOOD[@]}" "${NESTED[@]}" --password-file "$TESTROOT/ns/password"

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


# --- two clients: arguments and command lines (Amendment B)
echo "a second long password" > "$TESTROOT/password2"
TWO=(--clients 2 --username2 second --password-file2 "$TESTROOT/password2" --display2 97)
says 2 "--clients takes 1 or 2" "--clients takes 1 or 2" -- cmd_bare_run t "${GOOD[@]}" "${NESTED[@]}" --clients 3
says 2 "needs --username2" "two clients need client 2's account" -- \
  cmd_bare_run t "${GOOD[@]}" "${NESTED[@]}" --clients 2 --password-file2 "$TESTROOT/password2" --display2 97
says 2 "needs --password-file2" "two clients need client 2's password" -- \
  cmd_bare_run t "${GOOD[@]}" "${NESTED[@]}" --clients 2 --username2 second --display2 97
says 2 "another account" "client 2's account is another one" -- \
  cmd_bare_run t "${GOOD[@]}" "${NESTED[@]}" --clients 2 --username2 tester --password-file2 "$TESTROOT/password2" --display2 97
says 2 "--password-file2" "an unreadable second password file is refused" -- \
  cmd_bare_run t "${GOOD[@]}" "${NESTED[@]}" --clients 2 --username2 second --password-file2 "$TESTROOT/none" --display2 97
says 2 "needs --display2" "a nested two-client run needs client 2's display" -- \
  cmd_bare_run t "${GOOD[@]}" "${NESTED[@]}" --clients 2 --username2 second --password-file2 "$TESTROOT/password2"
says 2 "another display" "client 2's display is another one" -- \
  cmd_bare_run t "${GOOD[@]}" --display 97 "${TWO[@]}"
says 1 "0 to 999" "--display2 takes a display number" -- \
  cmd_bare_run t "${GOOD[@]}" "${NESTED[@]}" --clients 2 --username2 second --password-file2 "$TESTROOT/password2" --display2 097
says 2 "both clients" "--desktop is both clients' display: no --display2" -- \
  with_env DISPLAY=:0 cmd_bare_run t "${GOOD[@]}" --desktop "${TWO[@]}"
says 2 "at least 2" "--second-after takes at least 2 s" -- cmd_bare_run t "${GOOD[@]}" "${NESTED[@]}" "${TWO[@]}" --second-after 1
says 2 "--timeout2 takes" "--timeout2 takes whole seconds" -- cmd_bare_run t "${GOOD[@]}" "${NESTED[@]}" "${TWO[@]}" --timeout2 x
says 2 "needs --clients 2" "client 2's account needs --clients 2" -- cmd_bare_run t "${GOOD[@]}" "${NESTED[@]}" --username2 second
says 2 "needs --clients 2" "--display2 needs --clients 2" -- cmd_bare_run t "${GOOD[@]}" "${NESTED[@]}" --display2 97
says 2 "need --clients 2" "2@ inputs need --clients 2" -- cmd_bare_run t "${GOOD[@]}" "${NESTED[@]}" --key 2@5:Return
says 2 "--password-file2 is under $TESTROOT/ns" "a second password file the namespace would hide is refused" -- \
  cmd_bare_run t "${GOOD[@]}" "${NESTED[@]}" --clients 2 --username2 second --password-file2 "$TESTROOT/ns/password" --display2 97
mkdir -p "$TESTROOT/rt3"
mapfile -t got < <(XDG_RUNTIME_DIR=$TESTROOT/rt3 HOME=$TESTROOT/nohome namespace_cmd 99 "" 97)
want=(bwrap --dev-bind / / --proc /proc --tmpfs /run --perms 0700 --dir "$TESTROOT/rt3" --tmpfs /tmp
  --bind /tmp/.X11-unix/X99 /tmp/.X11-unix/X99 --bind /tmp/.X11-unix/X97 /tmp/.X11-unix/X97 --tmpfs /dev/shm
  --unshare-net --unshare-pid --die-with-parent --seccomp 7 --)
if [ "${got[*]}" = "${want[*]}" ]; then ok "a two-client namespace binds both displays' sockets"; else bad "two-client namespace: ${got[*]}"; fi
mapfile -t ns < <(XDG_RUNTIME_DIR=$TESTROOT/rt3 HOME=$TESTROOT/nohome namespace_cmd 99 "" 97)
mapfile -t got < <(clients=2 watch_cmd "$TESTROOT/out" "$TESTROOT/io" 4242 "${ns[@]}")
want=(python3 -I "$WATCHDOG" watch --root "$WORK" --prefix "$WORK/compat" --out "$TESTROOT/out" --sandbox-pid 4242
  --log "$TESTROOT/io/1/steam-1418100.log" --log "$TESTROOT/io/2/steam-1418100.log" --go "$TESTROOT/io/.go"
  --tmpfs /run --tmpfs /tmp --bind /tmp/.X11-unix/X99 --bind /tmp/.X11-unix/X97 --tmpfs /dev/shm)
if [ "${got[*]}" = "${want[*]}" ]; then ok "a two-client watchdog gets each client's log, in order"; else bad "two-client watch: ${got[*]}"; fi

# The backstop's count: the wineserver processes below the namespace's bwrap, and nothing else.
WSD=$TESTROOT/wsd
mkdir -p "$WSD"
ln -s "$(command -v sleep)" "$WSD/wineserver"
(
  "$WSD/wineserver" 30 &
  "$WSD/wineserver" 30 &
  wait
) &
tree=$!
"$WSD/wineserver" 30 &
alone=$!
for _ in $(seq 50); do [ "$(namespace_wineservers "$tree")" = 2 ] && break; sleep 0.1; done
if [ "$(namespace_wineservers "$tree")" = 2 ] && [ "$(namespace_wineservers "$alone")" = 1 ]; then
  ok "the backstop counts the wineservers below the namespace, and none outside it"
else
  bad "the wineserver count: $(namespace_wineservers "$tree") below the tree, $(namespace_wineservers "$alone") alone"
fi
pkill -KILL -P "$tree" 2> /dev/null
kill -KILL "$tree" "$alone" 2> /dev/null
{ wait "$tree" "$alone"; } 2> /dev/null

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
    with_fakes with_proton "$TESTROOT/proton" cmd_bare_run t "${GOOD[@]}" "${NESTED[@]}"
  cp -- "$TESTROOT/proton-nobridge/version" "$WORK/compat/fenji-primed"
  says 1 "another GE-Proton build" "a run names a prefix primed by another GE-Proton build" -- \
    with_fakes with_proton "$TESTROOT/proton" cmd_bare_run t "${GOOD[@]}" "${NESTED[@]}"
  : > "$WORK/compat/fenji-primed"
  says 1 "before builds were recorded" "a run names a prefix primed before builds were recorded" -- \
    with_fakes with_proton "$TESTROOT/proton" cmd_bare_run t "${GOOD[@]}" "${NESTED[@]}"
  # wired COMMAND...: COMMAND with need_display and seccomp_filter standing in, so it gets as far as
  # the nested-display check, with the fakes and the fake GE-Proton.
  # shellcheck disable=SC2317,SC2329 # the stand-ins are called by run-bare.sh's functions, not here
  wired() {
    need_display() { :; }
    seccomp_filter() { echo x; }
    with_fakes with_proton "$TESTROOT/proton" "$@"
  }
  cp -- "$TESTROOT/proton/version" "$WORK/compat/fenji-primed"
  says 1 "can't be a nested display" "a run checks its nested display" -- wired cmd_bare_run t "${GOOD[@]}" --display 1
  rm -f -- "$WORK/compat/fenji-primed"
  says 1 "can't be a nested display" "setup checks its nested display" -- wired cmd_bare_setup --display 1
  says 2 "session's own display" "a run refuses --display naming DISPLAY" -- \
    with_env DISPLAY=:98 wired cmd_bare_run t "${GOOD[@]}" --display 98
  says 2 "session's own display" "setup refuses --display naming DISPLAY" -- with_env DISPLAY=:98 wired cmd_bare_setup --display 98
  rm -f -- "$WORK/compat/fenji-primed"
else
  echo "skip the unprimed-prefix run test: it needs Linux on x86_64"
fi

# --- setup and run end to end (#148). bwrap runs what follows its --, and stand-ins record what
# they were given: umu-run (which plays createprefix and the prime), GE-Proton, fenji-server (which
# listens), fenji-launch, screenshots (import), and the watchdog's watch mode; its baseline and
# collect modes are the real ones. cp drops --reflink=always: the test root has no reflinks.
if [ "$(uname -s)/$(uname -m)" = Linux/x86_64 ]; then
  F=$TESTROOT/flow
  mkdir -p "$F/bin" "$F/proton/files/bin" "$F/proton/files/lib/wine/discord-rpc-bridge" "$F/release" \
    "$F/pinned/bin64" "$F/home" "$F/log" "$F/rt"
  cp -- "$(command -v sleep)" "$F/pinned/bin64/gujianol.exe"
  touch "$F/proton/files/lib/wine/discord-rpc-bridge/bridge.exe" "$F/xauth"
  echo "1789520217 GE-Proton-flow" > "$F/proton/version"
  printf '#!/bin/sh\nexit 0\n' > "$F/proton/proton"
  # wineserver -k: recorded.
  # shellcheck disable=SC2016 # the stand-in expands its own variables
  printf '#!/bin/sh\n[ -z "${FAKE_LOG:-}" ] || echo "wineserver $*" >> "$FAKE_LOG/wineserver"\n' > "$F/proton/files/bin/wineserver"
  cat > "$F/bin/bwrap" <<'EOF'
#!/usr/bin/env bash
# Records its arguments, then runs what follows --.
{ echo "--- bwrap"; printf '%s\n' "$@"; } >> "$FAKE_LOG/bwrap"
while [ $# -gt 0 ] && [ "$1" != -- ]; do shift; done
shift
exec "$@"
EOF
  cat > "$F/bin/cp" <<'EOF'
#!/usr/bin/env bash
# The test root has no reflinks: --reflink=always becomes a plain copy.
here=$(dirname -- "$0")
args=()
for a in "$@"; do [ "$a" = --reflink=always ] || args+=("$a"); done
PATH=${PATH//"$here:"/} exec cp "${args[@]}"
EOF
  cat > "$F/bin/import" <<'EOF'
#!/usr/bin/env bash
echo "import $*" >> "$FAKE_LOG/import"
EOF
  cat > "$F/umu-run" <<'EOF'
#!/usr/bin/env bash
# Records its arguments and the environment it got, then plays umu-run's part: createprefix builds
# a prefix (and exits 1, as umu-launcher 1.4 does); anything else is the prime, which registers
# Proton's bridge service. FAKE_UMU_FAIL names a step that fails instead, changing nothing.
{
  echo "--- umu-run $*"
  env | grep -E '^(UMU_RUNTIME_UPDATE|PROTONPATH|WINEPREFIX|GAMEID|PROTON_USE_XALIA|DISPLAY|XAUTHORITY)=' | LC_ALL=C sort
} >> "$FAKE_LOG/umu"
if [ "$1" = createprefix ]; then
  [ "${FAKE_UMU_FAIL:-}" != createprefix ] || exit 1
  mkdir -p "$WINEPREFIX/drive_c/windows/system32"
  printf 'WINE REGISTRY Version 2\n' > "$WINEPREFIX/system.reg"
  printf 'WINE REGISTRY Version 2\n' > "$WINEPREFIX/user.reg"
  exit 1
fi
[ "${FAKE_UMU_FAIL:-}" != prime ] || exit 1
printf '\n[System\\\\ControlSet001\\\\Services\\\\discord-bridge] 1\n"Type"=dword:00000010\n' >> "$WINEPREFIX/system.reg"
EOF
  cat > "$F/release/fenji-server" <<'EOF'
#!/usr/bin/env python3
"""Listens on its config's login and API addresses until it is stopped (`--config FILE run`), or its
parent ends: run-bare.sh's own stop isn't the only way a run ends."""
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
# Records its arguments, says what fenji-launch says right before it starts the client, and plays
# the client for FAKE_LAUNCH_SECS. A client that misbehaves plants a link at FAKE_PLANT, to
# FAKE_PLANT_TO, or a FIFO at FAKE_FIFO. A rescuer opens the FIFO after 15 s, so that a writer
# stalled on it is let go and the test ends, but only while the path still is a FIFO: the test
# removes it once its flow is over, and opening a path that is gone would create a regular file
# there, where the next flow writes its mark.
printf '%s\n' "$@" > "$FAKE_LOG/launch"
echo "$* | PROTON_LOG_DIR=${PROTON_LOG_DIR:-} DISPLAY=${DISPLAY:-}" >> "$FAKE_LOG/launches"
echo "starting the client" >&2
# FAKE_CLIENT: the copy's gujianol.exe (sleep) in its own session, outliving this launcher as Wine's
# processes may. FAKE_WINESERVER: a wineserver of this session's own, as a second umu-run session
# would start one (Amendment B's backstop).
client=""
prev=""
for a in "$@"; do
  if [ "$prev" = --client ]; then client=$a; fi
  prev=$a
done
if [ -n "${FAKE_CLIENT:-}" ]; then
  setsid "$client/bin64/gujianol.exe" 60 < /dev/null > /dev/null 2>&1 &
  echo "$!" >> "$FAKE_LOG/fake-pids"
fi
if [ -n "${FAKE_WINESERVER:-}" ]; then
  "$FAKE_WS" 60 < /dev/null > /dev/null 2>&1 &
  echo "$!" >> "$FAKE_LOG/fake-pids"
fi
if [ -n "${FAKE_PLANT:-}" ]; then ln -s -- "$FAKE_PLANT_TO" "$FAKE_PLANT"; fi
if [ -n "${FAKE_FIFO:-}" ]; then
  mkfifo -- "$FAKE_FIFO"
  (sleep 15; if [ -p "$FAKE_FIFO" ]; then exec 3<> "$FAKE_FIFO"; fi) > /dev/null 2>&1 &
fi
sleep "${FAKE_LAUNCH_SECS:-0}"
EOF
  cat > "$F/watchdog.py" <<'EOF'
"""The flow tests' watchdog: watch creates the start signal, waits for the namespace to end (or,
with FAKE_WATCH_EARLY, only that many seconds, as a gate event does while the client plays) and
exits FAKE_WATCH_RC (0 by default); every other mode is the real watchdog's. Each call is
recorded."""
import os
import sys
import time

with open(os.path.join(os.environ["FAKE_LOG"], "watchdog"), "a") as f:
    f.write(" ".join(sys.argv[1:]) + "\n")
if sys.argv[1] != "watch":
    real = os.path.join(os.environ["FAKE_REAL_SPIKE"], "watchdog.py")
    os.execv(sys.executable, [sys.executable, "-I", real] + sys.argv[1:])
args = sys.argv[2:]
open(args[args.index("--go") + 1], "x").close()
spid = int(args[args.index("--sandbox-pid") + 1])


def alive(pid):
    try:
        with open(f"/proc/{pid}/stat") as f:
            return f.read().split(") ", 1)[1][0] != "Z"
    except (OSError, IndexError):
        return False


deadline = time.monotonic() + float(os.environ.get("FAKE_WATCH_EARLY", "120"))
while alive(spid) and time.monotonic() < deadline:
    time.sleep(0.05)
sys.exit(int(os.environ.get("FAKE_WATCH_RC", "0")))
EOF
  mkdir -p "$F/here" "$F/ws"
  for tool in xclick xkey; do
    printf 'import os, sys, time\nwith open(os.path.join(os.environ["FAKE_LOG"], "inputs"), "a") as f:\n    f.write("%s " + " ".join(sys.argv[1:]) + "\\n")\n' "$tool" > "$F/here/$tool.py"
  done
  ln -s "$(command -v sleep)" "$F/ws/wineserver"
  chmod +x "$F/bin/"* "$F/proton/proton" "$F/proton/files/bin/wineserver" "$F/umu-run" "$F/release/"*
  fake_pgrep ""
  echo "a long password" > "$F/password"
  read -r lp ap < <(python3 -I -c 'import socket
s = [socket.socket() for _ in range(2)]
for x in s:
    x.bind(("127.0.0.1", 0))
print(*(x.getsockname()[1] for x in s))')
  mkdir -p "$F/work/data"
  printf '[server]\npublic_host = "127.0.0.1"\nlogin_bind = "127.0.0.1:%s"\napi_bind = "127.0.0.1:%s"\n[store]\npath = "%s"\n' \
    "$lp" "$ap" "$F/work/data/fenji.db" > "$F/fenji.toml"

  # flow NAME WORKDIR COMMAND...: COMMAND with the stand-ins, in WORKDIR, its records in $F/log/NAME;
  # sets rc, out (its output) and secs (how long it took). The output goes to a file, not a pipe:
  # a stopped scheduler's sleep outlives it, and would hold a pipe open.
  # shellcheck disable=SC2317,SC2329 # the stand-ins are called by run-bare.sh's functions, not here
  flow() {
    local name=$1 dir=$2 t0
    shift 2
    mkdir -p "$F/log/$name"
    rc=0
    t0=$(date +%s)
    (
      export FAKE_LOG=$F/log/$name FAKE_REAL_SPIKE=$HERE FAKE_WS=$F/ws/wineserver
      HERE=$F/here
      WORK=$dir PROTON=$F/proton UMU=$F/umu-run STUBBIN=$F/release WATCHDOG=$F/watchdog.py
      PINNED_COPY=$F/pinned HOME=$F/home PATH="$F/bin:$TESTROOT/bin:$PATH" XDG_RUNTIME_DIR=$F/rt
      hash -r
      need_display() { :; }
      nested_display_ok() { :; }
      seccomp_filter() { echo x; }
      "$@"
    ) > "$F/log/$name/out" 2>&1 || rc=$?
    secs=$(($(date +%s) - t0))
    out=$(cat -- "$F/log/$name/out")
  }
  # joined FILE: FILE's lines joined by spaces, padded for case patterns.
  joined() { echo " $(paste -sd ' ' -- "$1") "; }
  # umu_blocks NAME: each umu-run call of flow NAME on one line: its arguments, then its environment.
  umu_blocks() { awk '/^--- umu-run / { if (b) print b; b = $0; next } { b = b " " $0 } END { if (b) print b }' "$F/log/$1/umu"; }
  on_desktop=(with_env DISPLAY=:77 with_env XAUTHORITY="$F/xauth")

  # Setup: a new prefix, built and primed through Proton's launcher, with umu-run's environment.
  flow setup-new "$F/work" "${on_desktop[@]}" cmd_bare_setup --desktop
  if [ "$rc" = 0 ] && [ "$(cat "$F/work/compat/fenji-primed" 2> /dev/null)" = "$(cat "$F/proton/version")" ]; then
    ok "setup builds and primes a prefix, and its mark names the GE-Proton build"
  else
    bad "setup: rc $rc, mark '$(cat "$F/work/compat/fenji-primed" 2> /dev/null)': $out"
  fi
  mapfile -t calls < <(umu_blocks setup-new)
  want_env="DISPLAY=:77 GAMEID=umu-1418100 PROTONPATH=$F/proton PROTON_USE_XALIA=0 UMU_RUNTIME_UPDATE=0 WINEPREFIX=$F/work/compat XAUTHORITY=$F/xauth"
  if [ "${#calls[@]}" = 2 ] && [ "${calls[0]}" = "--- umu-run createprefix $want_env" ] &&
    [ "${calls[1]}" = "--- umu-run $F/work/compat/drive_c/windows/system32/cmd.exe /c exit $want_env" ]; then
    ok "setup runs createprefix, then the prime, each with umu-run's environment and the display"
  else
    bad "setup's umu-run calls: $(printf '[%s] ' "${calls[@]}")"
  fi
  case "$(joined "$F/log/setup-new/bwrap")" in
    *" --ro-bind $F/xauth $F/xauth"*" --ro-bind $F/xauth $F/xauth "*)
      ok "setup's namespaces bring in the desktop's auth file" ;;
    *) bad "setup's namespaces: $(joined "$F/log/setup-new/bwrap")" ;;
  esac
  flow setup-again "$F/work" "${on_desktop[@]}" cmd_bare_setup --desktop
  if [ "$rc" = 0 ] && [[ $out == *"the prefix is ready"* ]] && [ ! -e "$F/log/setup-again/umu" ]; then
    ok "setup leaves a primed prefix alone"
  else
    bad "setup on a primed prefix: rc $rc: $out"
  fi

  # Setup on a prefix that exists but was never primed: the prime alone.
  mkdir -p "$F/work2/compat/drive_c/windows"
  echo "WINE REGISTRY Version 2" > "$F/work2/compat/system.reg"
  flow setup-unprimed "$F/work2" "${on_desktop[@]}" cmd_bare_setup --desktop
  mapfile -t calls < <(umu_blocks setup-unprimed)
  if [ "$rc" = 0 ] && [ "${#calls[@]}" = 1 ] && [[ ${calls[0]} == "--- umu-run $F/work2/compat/drive_c/windows/system32/cmd.exe /c exit "* ]] &&
    [ -f "$F/work2/compat/fenji-primed" ]; then
    ok "setup primes an existing prefix without createprefix"
  else
    bad "setup on an unprimed prefix: rc $rc, calls $(printf '[%s] ' "${calls[@]}"): $out"
  fi

  # A failing umu-run: exit 1, and no mark.
  flow setup-nobuild "$F/work3" "${on_desktop[@]}" with_env FAKE_UMU_FAIL=createprefix cmd_bare_setup --desktop
  if [ "$rc" = 1 ] && [[ $out == *"createprefix didn't build the prefix"* ]] && [ ! -e "$F/work3/compat/fenji-primed" ]; then
    ok "a createprefix that builds nothing stops setup, with no mark"
  else
    bad "a failing createprefix: rc $rc: $out"
  fi
  flow setup-noprime "$F/work4" "${on_desktop[@]}" with_env FAKE_UMU_FAIL=prime cmd_bare_setup --desktop
  if [ "$rc" = 1 ] && [[ $out == *"the prime through Proton's launcher failed"* ]] && [ ! -e "$F/work4/compat/fenji-primed" ]; then
    ok "a failing prime stops setup, with no mark"
  else
    bad "a failing prime: rc $rc: $out"
  fi

  RUNARGS=(--server "$F/fenji.toml" --server-data "$F/work/data" --username tester --password-file "$F/password")
  # A desktop run, end to end: the session's display and auth file, the watchdog on the prefix and
  # the namespace's tmpfs mounts, fenji-launch as a player starts it, and no screenshots (the client
  # plays 6 s, past the first screenshot's time).
  flow run-desktop "$F/work" "${on_desktop[@]}" with_env FAKE_LAUNCH_SECS=6 cmd_bare_run d1 "${RUNARGS[@]}" --desktop
  if [ "$rc" = 0 ] && [[ $out == *"run d1 done: fenji-launch exit 0, namespace exit 0, watchdog exit 0"* ]]; then
    ok "a desktop run goes end to end"
  else
    bad "a desktop run: rc $rc: $out"
  fi
  case "$(joined "$F/log/run-desktop/bwrap")" in
    *" --bind /tmp/.X11-unix/X77 /tmp/.X11-unix/X77"*" --ro-bind $F/xauth $F/xauth"*" DISPLAY=:77 XAUTHORITY=$F/xauth "*)
      ok "a desktop run's namespace has the session's display and its auth file" ;;
    *) bad "a desktop run's namespace: $(joined "$F/log/run-desktop/bwrap")" ;;
  esac
  if grep -qxF "baseline --root $F/work --prefix $F/work/compat --out $F/work/out/d1" "$F/log/run-desktop/watchdog"; then
    ok "the run's baseline names the prefix"
  else
    bad "baseline: $(cat "$F/log/run-desktop/watchdog")"
  fi
  case " $(grep '^watch ' "$F/log/run-desktop/watchdog") " in
    *" --prefix $F/work/compat"*" --tmpfs /run"*" --tmpfs /tmp --bind /tmp/.X11-unix/X77 --tmpfs /dev/shm"*" --bind $F/xauth "*)
      ok "the run's watchdog has the prefix, and the started namespace's tmpfs mounts less what it binds in" ;;
    *) bad "watch: $(grep '^watch ' "$F/log/run-desktop/watchdog")" ;;
  esac
  if [ "$(joined "$F/log/run-desktop/launch")" = " play --server http://127.0.0.1:$ap --username tester --password-file $F/password --client $F/work/runs/d1/client --prefix $F/work/compat --umu-run $F/umu-run --proton $F/proton " ]; then
    ok "fenji-launch plays from the run's copy, as a player starts it"
  else
    bad "launch: $(joined "$F/log/run-desktop/launch")"
  fi
  if [ ! -e "$F/log/run-desktop/import" ]; then ok "a desktop run takes no screenshots"; else bad "desktop screenshots: $(cat "$F/log/run-desktop/import")"; fi

  # A nested run: screenshots of its display, and the input schedulers stopped once it ends (a key
  # press is still due at 20 s when the client is done at 6 s).
  flow run-nested "$F/work" with_env FAKE_LAUNCH_SECS=6 cmd_bare_run n1 "${RUNARGS[@]}" --display 78 --timeout 30 --key 20:Return
  if [ "$rc" = 0 ] && grep -qxF "import -display :78 -window root $F/work/out/n1/screen-5s.png" "$F/log/run-nested/import" 2> /dev/null; then
    ok "a nested run screenshots its own display"
  else
    bad "a nested run: rc $rc, screenshots '$(cat "$F/log/run-nested/import" 2> /dev/null)': $out"
  fi
  if [ "$secs" -lt 15 ]; then ok "a run ends with its client: the input schedulers are stopped"; else bad "a nested run took $secs s"; fi

  # A run whose watchdog doesn't end cleanly (here a gate event) marks the prefix, and setup and run
  # refuse it until it is rebuilt: a driver service that run left in the registry would start,
  # unwatched, when Wine next boots the prefix, as setup's prime does (#148).
  if [ ! -e "$F/work/compat/fenji-gate-event" ]; then ok "clean runs leave the prefix unmarked"; else bad "a clean run marked the prefix"; fi
  flow run-failure "$F/work" with_env FAKE_WATCH_RC=4 cmd_bare_run w1 "${RUNARGS[@]}" --display 78
  if [ "$rc" = 4 ] && grep -q '^run w1: watchdog exit 4, ' "$F/work/compat/fenji-gate-event" 2> /dev/null; then
    ok "a watchdog failure marks the prefix too"
  else
    bad "a watchdog failure: rc $rc, mark '$(cat "$F/work/compat/fenji-gate-event" 2> /dev/null)': $out"
  fi
  rm -f -- "$F/work/compat/fenji-gate-event"
  flow run-gate "$F/work" with_env FAKE_WATCH_RC=3 cmd_bare_run g1 "${RUNARGS[@]}" --display 78
  if [ "$rc" = 3 ] && grep -q '^run g1: watchdog exit 3, ' "$F/work/compat/fenji-gate-event" 2> /dev/null; then
    ok "a gate event marks the prefix"
  else
    bad "a gate event: rc $rc, mark '$(cat "$F/work/compat/fenji-gate-event" 2> /dev/null)': $out"
  fi
  flow setup-marked "$F/work" "${on_desktop[@]}" cmd_bare_setup --desktop
  if [ "$rc" = 1 ] && [[ $out == *"saw a gate event or a watchdog failure (run g1: watchdog exit 3, "* ]] &&
    [ ! -e "$F/log/setup-marked/umu" ]; then
    ok "setup refuses a prefix that saw a gate event, and starts nothing in it"
  else
    bad "setup on a marked prefix: rc $rc: $out"
  fi
  flow run-marked "$F/work" cmd_bare_run t2 "${RUNARGS[@]}" --display 78
  if [ "$rc" = 1 ] && [[ $out == *"saw a gate event or a watchdog failure"* ]] && [ ! -e "$F/log/run-marked/bwrap" ]; then
    ok "a run refuses a prefix that saw a gate event"
  else
    bad "a run on a marked prefix: rc $rc: $out"
  fi
  # The client can write the prefix: the mark is never written through a link it planted there,
  # and anything at the mark's place keeps the prefix refused.
  rm -f -- "$F/work/compat/fenji-gate-event"
  echo "the host's own file" > "$F/host-file"
  flow run-planted "$F/work" with_env FAKE_WATCH_RC=3 with_env FAKE_PLANT="$F/work/compat/fenji-gate-event" \
    with_env FAKE_PLANT_TO="$F/host-file" cmd_bare_run p1 "${RUNARGS[@]}" --display 78
  if [ "$rc" = 3 ] && [ "$(cat "$F/host-file")" = "the host's own file" ] && [[ $out == *"couldn't write the gate-event mark"* ]]; then
    ok "the gate-event mark is never written through a link the client planted"
  else
    bad "a planted link at the mark: rc $rc, host file '$(cat "$F/host-file")': $out"
  fi
  flow run-planted-next "$F/work" cmd_bare_run t3 "${RUNARGS[@]}" --display 78
  if [ "$rc" = 1 ] && [[ $out == *"saw a gate event or a watchdog failure;"* ]]; then
    ok "a link at the mark's place keeps the prefix refused"
  else
    bad "after a planted link: rc $rc: $out"
  fi
  # Nor does a FIFO there stall the runner on its way to the verdict and the evidence.
  rm -f -- "$F/work/compat/fenji-gate-event"
  flow run-fifo "$F/work" with_env FAKE_WATCH_RC=3 with_env FAKE_FIFO="$F/work/compat/fenji-gate-event" \
    cmd_bare_run f1 "${RUNARGS[@]}" --display 78
  if [ "$rc" = 3 ] && [ "$secs" -lt 12 ] && [[ $out == *"couldn't write the gate-event mark"* ]] &&
    [ -e "$F/work/out/f1/watchdog-exit-code" ]; then
    ok "a FIFO the client planted at the mark's place doesn't stall the runner"
  else
    bad "a planted FIFO: rc $rc after $secs s: $out"
  fi
  # A gate event while the client still plays (#174): the watchdog exits first, and the run ends
  # the namespace, marks the prefix and reports it, long before the client would have stopped.
  rm -f -- "$F/work/compat/fenji-gate-event"
  flow run-gate-first "$F/work" with_env FAKE_WATCH_RC=3 with_env FAKE_WATCH_EARLY=2 \
    with_env FAKE_LAUNCH_SECS=60 cmd_bare_run e1 "${RUNARGS[@]}" --display 78
  if [ "$rc" = 3 ] && [ "$secs" -lt 30 ] && grep -q '^run e1: watchdog exit 3, ' "$F/work/compat/fenji-gate-event" 2> /dev/null; then
    ok "a watchdog that exits first ends the run: the namespace stops and the prefix is marked"
  else
    bad "the watchdog first: rc $rc after $secs s, mark '$(cat "$F/work/compat/fenji-gate-event" 2> /dev/null)': $out"
  fi

  # --- two clients (M4 world plan, Amendment B; Ruling R14: tested here, not run live yet). Two
  # reflink copies, two fenji-launch on the one prefix with their own Proton logs and displays, both
  # logs to the watchdog, client 2 ended by its own pid if it outlives its launcher, and the backstop
  # that stops a run whose namespace holds a second wineserver.
  echo "a second long password" > "$F/password2"
  rm -f -- "$F/work/compat/fenji-gate-event"
  TWOARGS=(--clients 2 --username2 second --password-file2 "$F/password2" --display2 77 --second-after 2)
  flow run-two "$F/work" with_env FAKE_LAUNCH_SECS=1 with_env FAKE_CLIENT=1 cmd_bare_run two1 "${RUNARGS[@]}" \
    --display 78 "${TWOARGS[@]}" --timeout 25 --timeout2 3 --key 1:space --key 2:w/500 --key 2@3:Return
  BIO=$F/work/io/two1 BOUT=$F/work/out/two1 BRUN=$F/work/runs/two1
  if [ "$rc" = 0 ] && [[ $out == *"run two1 done:"* ]] && [ -f "$BRUN/client/bin64/gujianol.exe" ] &&
    [ -f "$BRUN/client2/bin64/gujianol.exe" ]; then
    ok "a two-client bare run goes end to end, from two copies of the pinned client"
  else
    bad "a two-client bare run: rc $rc: $out"
  fi
  l1=$(sed -n 1p "$F/log/run-two/launches") l2=$(sed -n 2p "$F/log/run-two/launches")
  if [[ $l1 == *"--username tester --password-file $F/password --client $BRUN/client --prefix $F/work/compat "*"| PROTON_LOG_DIR=$BIO/1 DISPLAY=:78" ]] &&
    [[ $l2 == *"--username second --password-file $F/password2 --client $BRUN/client2 --prefix $F/work/compat "*"| PROTON_LOG_DIR=$BIO/2 DISPLAY=:77" ]]; then
    ok "each client is its own fenji-launch on the one prefix, with its own account, copy, Proton log and display"
  else
    bad "the launches: [$l1] [$l2]"
  fi
  case " $(grep '^watch ' "$F/log/run-two/watchdog") " in
    *" --log $BIO/1/steam-1418100.log --log $BIO/2/steam-1418100.log "*" --bind /tmp/.X11-unix/X77 "*)
      ok "the watchdog gets both logs in order, and leaves client 2's display socket out of its tmpfs scan" ;;
    *) bad "two clients' watch: $(grep '^watch ' "$F/log/run-two/watchdog")" ;;
  esac
  case "$(joined "$F/log/run-two/bwrap")" in
    *" --bind /tmp/.X11-unix/X78 /tmp/.X11-unix/X78 --bind /tmp/.X11-unix/X77 /tmp/.X11-unix/X77 "*)
      ok "the namespace has both displays' sockets" ;;
    *) bad "two clients' namespace: $(joined "$F/log/run-two/bwrap")" ;;
  esac
  client2=$(sed -n 2p "$F/log/run-two/fake-pids")
  if [ "$(sed -n 2p "$BOUT/io/2/ended-by-pid" 2> /dev/null)" = "$client2" ] && [ -f "$BOUT/io/exit-code" ] &&
    [ -f "$BOUT/io/exit-code-2" ] && [ "$(grep -c . "$F/log/run-two/wineserver")" = 1 ]; then
    ok "client 2, outliving its fenji-launch, is ended by its own pid; the wineserver stops once, after both"
  else
    bad "client 2's end: '$(cat "$BOUT/io/2/ended-by-pid" 2> /dev/null)' vs $client2; wineserver $(cat "$F/log/run-two/wineserver" 2> /dev/null)"
  fi
  if grep -qx "xkey --display :78 --key=space" "$F/log/run-two/inputs" && grep -qx "xkey --display :77 --key=Return" "$F/log/run-two/inputs" &&
    grep -qx "xkey --display :78 --key=w --hold=500" "$F/log/run-two/inputs"; then
    ok "each client's key presses, held ones too, go to its own display"
  else
    bad "two clients' inputs: $(cat "$F/log/run-two/inputs" 2> /dev/null)"
  fi
  while read -r p; do kill "$p" 2> /dev/null; done < "$F/log/run-two/fake-pids"

  # The backstop: a second wineserver in the namespace once client 2 has started stops the run at
  # once (exit 1), and the prefix is marked, so setup and run refuse it until it is rebuilt.
  flow run-ws "$F/work" with_env FAKE_LAUNCH_SECS=20 with_env FAKE_WINESERVER=1 cmd_bare_run ws1 "${RUNARGS[@]}" \
    --display 78 "${TWOARGS[@]}" --timeout 30 --timeout2 30
  if [ "$rc" = 1 ] && [ "$secs" -lt 15 ] && [[ $out == *"a second wineserver for the prefix"* ]] &&
    grep -q '^run ws1: a second wineserver' "$F/work/compat/fenji-gate-event" 2> /dev/null; then
    ok "a second wineserver in the namespace stops the run at once, exit 1, and marks the prefix"
  else
    bad "a second wineserver: rc $rc after $secs s, mark '$(cat "$F/work/compat/fenji-gate-event" 2> /dev/null)': $out"
  fi
  while read -r p; do kill "$p" 2> /dev/null; done < "$F/log/run-ws/fake-pids"
  flow run-after-ws "$F/work" cmd_bare_run t4 "${RUNARGS[@]}" --display 78
  if [ "$rc" = 1 ] && [[ $out == *"rebuild it before anything runs in it again"* ]]; then
    ok "after a second wineserver, the prefix is refused until it is rebuilt"
  else
    bad "after a second wineserver: rc $rc: $out"
  fi
  rm -f -- "$F/work/compat/fenji-gate-event"
else
  echo "skip the end-to-end flows: they need Linux on x86_64"
fi

# --- the namespace for real, where it can be made: probes run inside it. CI runs them after it
# installs bwrap, with a socket at /tmp/.X11-unix/X99, and sets FENJI_REQUIRE_LIVE_PROBES=1, so
# that a skip there fails instead (#148).
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
elif [ "${FENJI_REQUIRE_LIVE_PROBES:-}" = 1 ]; then
  bad "the live namespace probes couldn't run (FENJI_REQUIRE_LIVE_PROBES=1): they need Linux on x86_64, bwrap, namespaces and /tmp/.X11-unix/X99"
else
  echo "skip the live namespace probes: they need bwrap, namespaces and the nested display :99"
fi

if [ "$fails" -gt 0 ]; then echo "$fails failed"; exit 1; fi
echo "all passed"
