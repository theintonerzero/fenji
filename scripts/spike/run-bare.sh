#!/usr/bin/env bash
# Runs the unmodified pinned SoLO client once outside the sandbox, as a player starts it: through
# `fenji-launch play`, which checks the client folder against the pinned build, gets the launcher's
# Wine prefix ready with umu-run, signs in to a local fenji-server and starts the client with a
# fresh ticket in its environment (#139; the owner's decision on #114, 2026-10-13). Every run keeps
# all the guards of CLAUDE.md's "Bare-metal client runs":
#   - umu-run in a dedicated prefix (re/bare-metal/compat); the client from a reflink copy, made for
#     the run, of the pinned copy at $FENJI_CLIENT_PATH (never a link, never holding one, never in
#     a Steam library);
#   - Steam not running, checked before the run and every 2 s during it (Steam starting stops the
#     run);
#   - a local fenji-server, which shares the client's network namespace: that namespace has only
#     loopback, and the host's daemon sockets in /run, /tmp and /dev/shm (D-Bus, systemd-resolved,
#     the desktop portal) are out of reach, so the client's network reaches the Fenji server only.
#     Sockets elsewhere in the real filesystem stay reachable, such as IBus's in ~/.cache/ibus,
#     and the audio daemon can load network modules (#148); --desktop adds the session's display
#     (below);
#   - the AGT driver watchdog armed on the namespace's PID namespace before anything in it starts,
#     as in sandboxed runs; a gate event stops the run (exit 3);
#   - never --gf-sdk=mocked: fenji-launch passes no SDK flag at all.
# The namespace is bwrap with the real filesystem (--dev-bind / /), its own /proc, empty tmpfs
# /run, /tmp and /dev/shm (bar the user's audio sockets and the display's X11 socket, bound back),
# loopback only, its own PID namespace, and isolated-run.sh's seccomp filter against key requests
# (a kernel DNS upcall). So home, GPU, audio and display are as on a player's machine, and no host
# daemon socket in /run, /tmp or /dev/shm is. Its own /proc matters: without it, a nested bwrap
# (umu's pressure-vessel) looks its child up in the host's /proc by a namespace PID and hangs. The
# watchdog scans re/bare-metal (the prefix and the run's copy) and the namespace's own /run, /tmp
# and /dev/shm, less what the namespace binds into them (watch_cmd); the rest of the namespace's
# filesystem is the host's, whose own Proton prefixes would trip it. Its process rules (S2), its log
# rules (S3, which follow Proton's per-session logs) and its registry rule (S4, on the prefix named
# directly, not through umu's pfx link) carry the rest, and the run's evidence lists any new *.sys
# file under umu's runtime and /var/tmp.
# Linux on x86_64 only.
#
#   scripts/spike/run-bare.sh setup (--display N | --desktop)
#   scripts/spike/run-bare.sh run NAME --server FILE --server-data DIR --username USER
#       --password-file FILE (--display N | --desktop) [--timeout SECONDS] [--key T:KEY]...
#       [--type T:TEXT]... [--click T:X,Y]...
#
# setup builds the prefix with `umu-run createprefix` inside the guard namespace, with umu's runtime
# update off (the namespace has no network anyway), then primes it: cmd.exe started once through
# Proton's launcher, so Proton's own service is in the watchdog's baseline (prefix_primed). A run
# refuses an unprimed prefix, and setup primes an existing one.
#
# run's --server config must bind 127.0.0.1 and name 127.0.0.1 as its public_host, its [store] path
# must lie in --server-data, a directory under re/bare-metal/, and --password-file holds the
# account's password on its first line. The evidence goes to re/bare-metal/out/NAME; a Fenji ticket
# found there is redacted in place.
#
# The display: a run names it, and has no default. A nested display (--display N) is a dedicated
# rootful Xwayland, Xephyr or Xvfb, never the session's own (bare_display_problem), where
# screenshots and key presses reach the client and nothing else. The real GPU can present there
# only if that Xwayland has glamor on, so start a dedicated one without -shm, for example
#   Xwayland :98 -geometry 1920x1080 -ac -nolisten tcp -noreset -extension MIT-SHM &
# and pass --display 98, and run nothing else on it: with -ac, a terminal there is a shell outside
# the namespace. isolated-run.sh's :99 keeps glamor off (-shm) so that a sandboxed client gets no
# GPU device; a bare run's client has the GPU anyway, so never share the two displays.
# Otherwise the real GPU's first frame ends the client (bm-2: "XIO: fatal IO error").
# --desktop runs the client on the session's own display (DISPLAY), as a player has it. The
# display's X authority file comes in read-only, and there are no screenshots, key presses or
# clicks, which would take or reach the whole desktop. The client then joins the session's X
# server: it can see, read and type into the other X11 windows, read and replace the clipboard the
# compositor bridges into X11, and its XTEST input may reach native windows through the compositor
# (libei), a terminal included. The namespace's network stays loopback-only; the risk is the X
# server and the compositor as a way out of it. The owner accepted that for hands-on test runs
# (2026-10-13). Players run the game on their own desktop anyway; these runs are a developer's.
#
# Exit: 0 the run finished; 3 GATE EVENT (the watchdog stopped the run: stop all runs, report);
# 4 the watchdog failed; 5 the watchdog never armed (the client was not started); 6 the namespace
# ended before the client ran; 7 fenji-server or fenji-launch failed before the client started;
# 1 a guard or the workspace refused the run, or Steam started during it; 2 a usage error.
set -euo pipefail
BARE_HERE=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
# shellcheck source=run-client.sh
. "$BARE_HERE/run-client.sh"
# Everything a bare run writes lives here. The pinned copy is the run's source
# (~/.config/fenji/env.sh sets FENJI_CLIENT_PATH).
WORK="$REPO/re/bare-metal"
PINNED_COPY="${FENJI_CLIENT_PATH:-}"
UMU="${FENJI_UMU_RUN:-$HOME/.local/bin/umu-run}"
GAMEID=umu-1418100
# Where the display checks look: X lock files, the X socket directory, /proc (tests move all three).
X_LOCKS=/tmp
X_SOCKDIR=/tmp/.X11-unix
BARE_PROC=/proc

usage() { awk 'NR > 1 && /^#/ { print; next } NR > 1 { exit }' "$BARE_HERE/run-bare.sh" >&2; exit 2; }
die() { echo "run-bare: $*" >&2; exit 1; }
misuse() { echo "run-bare: $*" >&2; exit 2; }

# The guards that hold for setup and for every run: Linux, Steam not running, no Wine left over.
host_guards() {
  [ "$(uname -s)" = Linux ] || die "bare-metal runs are for the Linux game box only"
  [ "$(uname -m)" = x86_64 ] || die "x86_64 only (the seccomp filter knows x86 syscall numbers only)"
  if pgrep -x steam >/dev/null || pgrep -x steamwebhelper >/dev/null; then
    die "Steam is running; quit it first (CLAUDE.md, \"Bare-metal client runs\")"
  fi
  if pgrep -x wineserver >/dev/null || pgrep -x winedevice.exe >/dev/null; then
    die "Wine processes are already running on the host; stop them first"
  fi
  [ -x "$UMU" ] || die "umu-run not found at $UMU (run scripts/bootstrap.sh)"
  [ -x "$PROTON/proton" ] || die "GE-Proton not found at $PROTON (run scripts/bootstrap.sh)"
}

# The guard namespace's command line for display N: the real filesystem with its own /proc; empty
# /run, /tmp and /dev/shm, so no host daemon socket of those (D-Bus, systemd-resolved, the portal,
# agents) is there, bar the user's audio sockets and the display's X11 socket, bound back; Steam's
# pipe masked; loopback only; its own PID namespace (where the watchdog arms); the seccomp filter
# on fd 7; and nothing left when its parent goes. A second argument is a desktop run's X authority
# file, brought in read-only after the tmpfs mounts, which would hide it.
namespace_cmd() {
  local display=$1 xauth=${2:-} rt=${XDG_RUNTIME_DIR:-/run/user/$(id -u)} s
  printf '%s\n' bwrap --dev-bind / / --proc /proc --tmpfs /run --perms 0700 --dir "$rt"
  for s in pipewire-0 pulse; do
    if [ -e "$rt/$s" ]; then printf '%s\n' --bind "$rt/$s" "$rt/$s"; fi
  done
  printf '%s\n' --tmpfs /tmp --bind "/tmp/.X11-unix/X$display" "/tmp/.X11-unix/X$display" --tmpfs /dev/shm
  if [ -e "$HOME/.steam/steam.pipe" ]; then printf '%s\n' --ro-bind /dev/null "$HOME/.steam/steam.pipe"; fi
  if [ -n "$xauth" ]; then printf '%s\n' --ro-bind "$xauth" "$xauth"; fi
  printf '%s\n' --unshare-net --unshare-pid --die-with-parent --seccomp 7 --
}

# The environment inside: no session bus, no agents, no Wayland (the client draws on X11 display N),
# no password.
namespace_env() {
  printf '%s\n' env -u WAYLAND_DISPLAY -u DBUS_SESSION_BUS_ADDRESS -u SSH_AUTH_SOCK -u FENJI_PASSWORD "DISPLAY=:$1"
  if [ -n "${2:-}" ]; then printf '%s\n' "XAUTHORITY=$2"; fi
}

# --desktop: the session's own display, as a player has it. Its number, from a local DISPLAY.
desktop_display() {
  [[ "${DISPLAY:-}" =~ ^:([0123456789]+)(\.[0123456789]+)?$ ]] ||
    misuse "--desktop needs a local DISPLAY such as :0 (DISPLAY is '${DISPLAY:-}')"
  echo "${BASH_REMATCH[1]}"
}

# The X authority file a desktop run brings into the namespace, read-only: XAUTHORITY's, when it is
# set (GNOME keeps it under /run/user, which the namespace hides). Unset, Xlib reads ~/.Xauthority,
# which the namespace shows anyway.
desktop_xauth() {
  [ -n "${XAUTHORITY:-}" ] || return 0
  case "$XAUTHORITY" in /*) ;; *) misuse "XAUTHORITY must be an absolute path: '$XAUTHORITY'" ;; esac
  if [ ! -f "$XAUTHORITY" ] || [ ! -r "$XAUTHORITY" ]; then
    misuse "XAUTHORITY isn't a readable file: '$XAUTHORITY'"
  fi
  echo "$XAUTHORITY"
}

# --display's number, as isolated-run.sh's --x11 takes it: 0 to 999 without leading zeros.
check_display_number() {
  case "$1" in
    0 | [123456789] | [123456789][0123456789] | [123456789][0123456789][0123456789]) ;;
    *) die "--display takes an X display number, 0 to 999 without leading zeros, not '$1'" ;;
  esac
}

# The display for setup or a run, from --desktop (1 or 0) and whether --display was given (1 or 0):
# the nested one in display, or with --desktop the session's own, whose X authority file goes in
# xauth. Sets display and xauth. A nested display is never the session's own: --display naming
# DISPLAY would skip --desktop's limits on screenshots and input.
choose_display() {
  if [ "$1" = 1 ]; then
    [ "$2" = 0 ] || misuse "--desktop and --display don't go together: --desktop is the session's own display"
    display=$(desktop_display) || exit 2
    xauth=$(desktop_xauth) || exit 2
    return 0
  fi
  # DISPLAY parsed as isolated-run.sh parses it for --x11: [protocol/][host]:D[.S], by number.
  if [ -n "${DISPLAY:-}" ]; then
    local d=${DISPLAY##*:}
    d=${d%%.*}
    if [[ $DISPLAY != *:* || -z $d || $d == *[!0123456789]* ]]; then
      misuse "DISPLAY=$DISPLAY doesn't name a display number, so --display $display can't be checked against the session's display (fix or unset DISPLAY)"
    fi
    while [[ $d == 0?* ]]; do d=${d#0}; done
    [ "$d" != "$display" ] || misuse "--display $display is the session's own display (DISPLAY=$DISPLAY); use --desktop for it"
  fi
}

# Why display N isn't a nested display a bare run may use (nothing if it is). It must be a dedicated
# rootful Xwayland, Xephyr or Xvfb: a compositor locks its session displays under its own PID, and
# a session Xwayland runs -rootless and may pass input on to the compositor (-enable-ei-portal), as
# isolated-run.sh's "Display" explains. These are isolated-run.sh's --x11 checks: the socket
# directory, the socket and the lock (by stat without -L, so a symlink is refused, not followed,
# and a FIFO is never opened), the server's process, and its libXfont, which must have no
# font-server support, or the client could make the server connect anywhere from the host.
# Glamor and MIT-SHM may be on here, unlike in the sandbox: a bare run's client has the GPU and
# the host's IPC namespace anyway.
bare_display_problem() {
  local n=$1 uid st sock lock pid="" comm="" arg path last="" libs=() f r rc
  uid=$(id -u)
  sock="$X_SOCKDIR/X$n" lock="$X_LOCKS/.X$n-lock"
  st=$(stat -c '%u %f' -- "$X_SOCKDIR" 2>/dev/null) || { echo "no X server socket at $sock"; return; }
  (( (16#${st#* } & 8#170000) == 8#040000 )) || { echo "$X_SOCKDIR is not a directory (a symlink is refused too)"; return; }
  if [ "${st% *}" != 0 ] && [ "${st% *}" != "$uid" ]; then
    echo "$X_SOCKDIR belongs to uid ${st% *}, not root or you"
    return
  fi
  (( 16#${st#* } & 8#1000 )) || { echo "$X_SOCKDIR doesn't have the sticky bit, so anyone who can write to it could replace X$n"; return; }
  st=$(stat -c '%u %f' -- "$sock" 2>/dev/null) || { echo "no X server socket at $sock"; return; }
  (( (16#${st#* } & 8#170000) == 8#140000 )) || { echo "$sock is not a socket (a symlink is refused too)"; return; }
  [ "${st% *}" = "$uid" ] || { echo "$sock belongs to uid ${st% *}, not you (another user's X server)"; return; }
  st=$(stat -c '%u %f' -- "$lock" 2>/dev/null) || { echo "display :$n has no lock ($lock)"; return; }
  if (( (16#${st#* } & 8#170000) != 8#100000 )) || [ "${st% *}" != "$uid" ]; then
    echo "$lock is not a regular file of yours"
    return
  fi
  { read -r pid < "$lock"; } 2>/dev/null || true
  case "$pid" in "" | *[!0123456789]*) echo "$lock doesn't hold a process ID"; return ;; esac
  if ! { read -r comm < "$BARE_PROC/$pid/comm"; } 2>/dev/null; then
    echo "process $pid, named by $lock, is not running"
    return
  fi
  case "$comm" in
    Xwayland | Xephyr | Xvfb) ;;
    *) echo "$lock names $comm (PID $pid), not a dedicated Xwayland, Xephyr or Xvfb: a compositor locks its session displays; use --desktop for the session's own"
       return ;;
  esac
  { while IFS= read -r -d '' arg; do
      case "$arg" in
        -rootless) echo "$comm (PID $pid) runs -rootless, as a session Xwayland does: start a dedicated rootful one, or use --desktop"; return ;;
        -enable-ei-portal) echo "$comm (PID $pid) passes input on to the compositor (-enable-ei-portal): start a dedicated one without it"; return ;;
      esac
    done < "$BARE_PROC/$pid/cmdline"; } 2>/dev/null || true
  # No font-server support, checked as isolated-run.sh's x11_server_problem does: every libXfont
  # the server maps, read through its root, refused if it opens sockets or looks up host names.
  { while read -r _ _ _ _ _ path; do
      case "${path##*/}" in libXfont*) [ "$path" = "$last" ] || libs+=("$path"); last=$path ;; esac
    done < "$BARE_PROC/$pid/maps"; } 2>/dev/null || true
  if [ "${#libs[@]}" -eq 0 ]; then
    echo "$comm (PID $pid) maps no libXfont (or its maps can't be read), so its font library can't be checked for font-server support"
    return
  fi
  for f in "${libs[@]}"; do
    case "$f" in
      *" (deleted)") echo "$comm (PID $pid) maps $f, a libXfont replaced since it started, which can't be checked: restart the server"; return ;;
    esac
    r=$BARE_PROC/$pid/root$f
    if [ ! -f "$r" ] || [ ! -r "$r" ]; then
      echo "$comm (PID $pid) maps $f, which is not a readable file here, so it can't be checked for font-server support"
      return
    fi
    rc=0
    LC_ALL=C grep -qaF -e getaddrinfo -e gethostbyname -e socket -e connect -- "$r" || rc=$?
    case "$rc" in
      0) echo "$comm (PID $pid) maps $f, a libXfont with font-server support: the client could make the server connect anywhere"; return ;;
      1) ;;
      *) echo "grep failed (exit $rc) on $f, so it can't be checked for font-server support"; return ;;
    esac
  done
}

# Dies unless a nested display (desktop 0) is a dedicated X server (bare_display_problem).
nested_display_ok() {
  local why
  [ "$1" = 0 ] || return 0
  why=$(bare_display_problem "$2")
  [ -z "$why" ] || die "display :$2 can't be a nested display: $why"
}

# The host-side inputs for a run (run-client.sh's start_inputs), their PIDs in inputs. None on the
# player's own desktop: a screenshot would take the whole screen, and a key press or click would go
# to whatever has focus there.
bare_inputs() {
  local desktop=$1 out=$2 display=$3 tmo=$4
  inputs=()
  [ "$desktop" = 0 ] || return 0
  start_inputs "$out" "$display" "$tmo"
  inputs=("$shots" "$clicker" "$typist")
}

# isolated-run.sh's filter: add_key, request_key and keyctl fail, so no key request can make the
# kernel run a DNS lookup on the host's network.
seccomp_filter() {
  local bpf
  bpf=$(python3 -I "$BARE_HERE/seccomp-deny-keys.py" --printf) || die "couldn't build the seccomp filter"
  [ -n "$bpf" ] || die "couldn't build the seccomp filter"
  printf '%s' "$bpf"
}

# The pinned copy, refused if it could put the Steam library, or a link, under the run.
check_pinned_copy() {
  [ -n "$PINNED_COPY" ] || die "FENJI_CLIENT_PATH is unset (scripts/bootstrap.sh writes it to ~/.config/fenji/env.sh)"
  [ ! -L "$PINNED_COPY" ] || die "the pinned copy is a link: $PINNED_COPY"
  [ -f "$PINNED_COPY/bin64/gujianol.exe" ] || die "no pinned copy at $PINNED_COPY"
  local real
  real=$(realpath -- "$PINNED_COPY")
  case "$real/" in
    */steamapps/* | "$HOME"/.steam/* | "$HOME"/.local/share/Steam/*) die "the pinned copy is in a Steam library: $real" ;;
  esac
  [ -z "$(find "$PINNED_COPY" -type l -print -quit)" ] || die "the pinned copy holds a link; it must hold files only"
}

# The watchdog's command line for run OUT/IO and namespace PID, given the namespace's own command
# line (namespace_cmd's). It names the prefix, not umu's pfx link to it. S1 scans each tmpfs mount
# the namespace makes, through /proc/PID/root, less each path the namespace binds in (the audio and
# X11 sockets, the auth file): the rest of its filesystem is the host's, whose own Proton prefixes
# would trip that scan.
watch_cmd() {
  local out=$1 io=$2 spid=$3 i
  shift 3
  local a=("$@")
  printf '%s\n' python3 -I "$WATCHDOG" watch --root "$WORK" --prefix "$WORK/compat" --out "$out" \
    --sandbox-pid "$spid" --log "$io/steam-1418100.log" --go "$io/.go"
  for ((i = 0; i < ${#a[@]}; i++)); do
    case ${a[i]} in
      --tmpfs) printf '%s\n' --tmpfs "${a[i + 1]}" ;;
      --bind | --ro-bind) printf '%s\n' --bind "${a[i + 2]}" ;;
      --) break ;;
    esac
  done
}

# The watchdog's baseline command line for run OUT, with the prefix named as watch_cmd names it.
baseline_cmd() {
  printf '%s\n' python3 -I "$WATCHDOG" baseline --root "$WORK" --prefix "$WORK/compat" --out "$1"
}

# fenji-launch's arguments for a run: the player's command, with the run's client copy, the
# dedicated prefix, the pinned umu-run and GE-Proton. Never an SDK flag.
launch_args() {
  local api_port=$1 user=$2 pwfile=$3 client=$4
  printf '%s\n' play --server "http://127.0.0.1:$api_port" --username "$user" \
    --password-file "$pwfile" --client "$client" --prefix "$WORK/compat" --umu-run "$UMU" \
    --proton "$PROTON"
}

prefix_ready() { [ -f "$WORK/compat/system.reg" ] && [ -d "$WORK/compat/drive_c/windows" ]; }

# Proton's launch shim (umu.exe, through which Proton starts a program named by its unix path)
# registers GE-Proton's Discord bridge service the first time it starts anything in a prefix, and
# createprefix never runs the shim. Unprimed, the client's own first launch registers the service,
# and the watchdog rightly stops the run: a service that isn't in the baseline (bm-1, 2026-10-13).
# setup primes the prefix by starting cmd.exe once through the shim, then leaves the mark, which
# names the GE-Proton build that primed it. The prefix is primed when the mark names this build (a
# new build upgrades the prefix, and its services would be new too) and, if this GE-Proton ships
# the bridge, the bridge service has reached the registry on disk.
PRIMED_MARK=fenji-primed
proton_build() { cat -- "$PROTON/version" 2>/dev/null || echo unknown; }
prefix_primed() {
  [ -f "$WORK/compat/$PRIMED_MARK" ] || return 1
  [ "$(cat -- "$WORK/compat/$PRIMED_MARK")" = "$(proton_build)" ] || return 1
  [ ! -e "$PROTON/files/lib/wine/discord-rpc-bridge/bridge.exe" ] ||
    grep -q '^\[System\\\\ControlSet00[0-9]\\\\Services\\\\discord-bridge\]' "$WORK/compat/system.reg"
}

# umu-run's arguments for the prime: the prefix's cmd.exe by its unix path, which exits at once.
prime_args() { printf '%s\n' "$WORK/compat/drive_c/windows/system32/cmd.exe" /c exit; }

# umu-run's environment in setup: no runtime update (the namespace has no network anyway), the
# pinned GE-Proton, the dedicated prefix, and no Xalia, as in every run.
setup_env() {
  printf '%s\n' UMU_RUNTIME_UPDATE=0 "PROTONPATH=$PROTON" "WINEPREFIX=$WORK/compat" "GAMEID=$GAMEID" PROTON_USE_XALIA=0
}

cmd_bare_setup() {
  local display="" display_given=0 desktop=0 xauth=""
  while [ $# -gt 0 ]; do
    case "$1" in
      --display) [ $# -ge 2 ] || usage; display=$2; display_given=1; shift 2 ;;
      --desktop) desktop=1; shift ;;
      *) usage ;;
    esac
  done
  [ "$display_given" = 0 ] || check_display_number "$display"
  [ "$desktop" = 1 ] || [ "$display_given" = 1 ] ||
    misuse "setup needs --display N or --desktop: a dedicated nested display, or the session's own (see the header)"
  choose_display "$desktop" "$display_given"
  host_guards
  if prefix_ready && prefix_primed; then
    echo "the prefix is ready: $WORK/compat"
    return 0
  fi
  mkdir -p "$WORK/compat"
  need_display "$display"
  nested_display_ok "$desktop" "$display"
  local ns ne se bpf prime
  mapfile -t ns < <(namespace_cmd "$display" "$xauth")
  mapfile -t ne < <(namespace_env "$display" "$xauth")
  mapfile -t se < <(setup_env)
  bpf=$(seccomp_filter)
  if ! prefix_ready; then
    # umu-launcher 1.4 ends createprefix with exit 1 once the prefix is built: the prefix decides.
    "${ns[@]}" "${ne[@]}" "${se[@]}" "$UMU" createprefix > "$WORK/setup.log" 2>&1 \
      7< <(printf '%b' "$bpf") || true
    prefix_ready || die "createprefix didn't build the prefix; see $WORK/setup.log"
  fi
  # The prime (see prefix_primed). umu-run returns once Wine has ended, so the registry is on disk.
  mapfile -t prime < <(prime_args)
  timeout -k 10 300 "${ns[@]}" "${ne[@]}" "${se[@]}" "$UMU" "${prime[@]}" >> "$WORK/setup.log" 2>&1 \
    7< <(printf '%b' "$bpf") || die "the prime through Proton's launcher failed; see $WORK/setup.log"
  proton_build > "$WORK/compat/$PRIMED_MARK"
  prefix_primed || die "Proton's launcher ran, but its bridge service isn't in the prefix's registry; see $WORK/setup.log"
  echo "prefix ready: $WORK/compat"
}

cmd_bare_run() {
  [ $# -ge 1 ] || usage
  local run=$1
  shift
  local tmo=300 display="" display_given=0 desktop=0 xauth="" user="" pwfile="" keys=() types=() clicks=() presses=()
  local server_cfg="" server_data="" launch="" launch_password="" ports_given=0 stub_args=()
  while [ $# -gt 0 ]; do
    case "$1" in
      --server) [ $# -ge 2 ] || usage; server_cfg=$2; shift 2 ;;
      --server-data) [ $# -ge 2 ] || usage; server_data=$2; shift 2 ;;
      --username) [ $# -ge 2 ] || usage; user=$2; shift 2 ;;
      --password-file) [ $# -ge 2 ] || usage; pwfile=$2; shift 2 ;;
      --timeout) [ $# -ge 2 ] || usage; tmo=$2; shift 2 ;;
      --display) [ $# -ge 2 ] || usage; display=$2; display_given=1; shift 2 ;;
      --desktop) desktop=1; shift ;;
      --key) [ $# -ge 2 ] || usage; keys+=("$2"); presses+=("key:$2"); shift 2 ;;
      --type) [ $# -ge 2 ] || usage; types+=("$2"); presses+=("type:$2"); shift 2 ;;
      --click) [ $# -ge 2 ] || usage; clicks+=("$2"); shift 2 ;;
      *) usage ;;
    esac
  done
  case "$run" in '' | */* | .* | *[!A-Za-z0-9._-]*) die "bad run name: '$run'" ;; esac
  case "$tmo" in '' | *[!0123456789]*) die "--timeout takes whole seconds" ;; esac
  [ "$display_given" = 0 ] || check_display_number "$display"
  if [ "$desktop" = 1 ] && { [ ${#presses[@]} -gt 0 ] || [ ${#clicks[@]} -gt 0 ]; }; then
    misuse "--desktop takes no --key, --type or --click: they would go to whatever has focus on the desktop"
  fi
  [ -n "$server_cfg" ] || misuse "--server is required: the client plays against a local fenji-server"
  [ -n "$server_data" ] || misuse "--server-data is required: the server's store lives there"
  [[ "$user" =~ ^[A-Za-z0-9_.-]{1,32}$ ]] || misuse "--username takes the account's name"
  if [ -z "$pwfile" ] || [ ! -f "$pwfile" ] || [ ! -r "$pwfile" ]; then
    misuse "--password-file: can't read '$pwfile'"
  fi
  pwfile=$(realpath -- "$pwfile")
  local c
  for c in "${clicks[@]}"; do
    [[ "$c" =~ ^[0-9]+:[0-9]+,[0-9]+$ ]] || die "--click takes T:X,Y in whole numbers, not '$c'"
  done
  check_presses
  local login_port="" api_port="" launch_words=()
  check_server
  case "$server_data/" in "$WORK"/runs/* | "$WORK"/compat/*) misuse "--server-data can't be a run's or the prefix's directory" ;; esac
  python3 -I - "$server_cfg" <<'PY' || misuse "--server: [server] public_host must be 127.0.0.1 (fenji-launch looks it up inside the namespace)"
import sys, tomllib
with open(sys.argv[1], "rb") as f:
    sys.exit(0 if tomllib.load(f).get("server", {}).get("public_host") == "127.0.0.1" else 1)
PY
  [ "$desktop" = 1 ] || [ "$display_given" = 1 ] ||
    misuse "a run needs --display N or --desktop: a dedicated nested display, or the session's own (see the header)"
  choose_display "$desktop" "$display_given"
  host_guards
  prefix_ready || die "no prefix; run: $0 setup"
  if [ -f "$WORK/compat/$PRIMED_MARK" ] && [ "$(cat -- "$WORK/compat/$PRIMED_MARK")" != "$(proton_build)" ]; then
    die "the prefix was primed by another GE-Proton build, or before builds were recorded, and this one would upgrade it during the run; run once: $0 setup"
  fi
  prefix_primed || die "the prefix hasn't been through Proton's launcher, so Proton's own service isn't in the watchdog's baseline; run: $0 setup"
  need_display "$display"
  nested_display_ok "$desktop" "$display"
  check_pinned_copy
  [ -x "$STUBBIN/fenji-server" ] || die "build the server first: cargo build --release -p fenji-server"
  [ -x "$STUBBIN/fenji-launch" ] || die "build the launcher first: cargo build --release -p fenji-launcher"
  local OUT="$WORK/out/$run" IO="$WORK/io/$run" RUN="$WORK/runs/$run" PFX="$WORK/compat"
  if [ -e "$OUT" ] || [ -e "$IO" ] || [ -e "$RUN" ]; then die "run $run exists already; pick a new name"; fi
  mkdir -p "$OUT" "$IO/tmp" "$IO/server" "$RUN/client"
  cp -a --reflink=always -- "$PINNED_COPY/." "$RUN/client/" ||
    die "the client copy needs a filesystem with reflinks (btrfs, XFS); nothing was started"
  local server_copy="$IO/server/fenji.toml" channels
  cp -- "$server_cfg" "$server_copy"
  channels=$(watch_channels "$CHANNELS_DEFAULT")

  # --- before: inputs and the AGT baseline
  {
    echo "run $run (bare metal), $(date -Is)"
    echo "guards: Linux x86_64; Steam not running (and watched); no Wine running; reflink copy of $PINNED_COPY; prefix $PFX"
    echo "namespace: $(namespace_cmd "$display" "$xauth" | tr '\n' ' ')$(namespace_env "$display" "$xauth" | tr '\n' ' ')"
    echo "launch: fenji-launch $(launch_args "$api_port" "$user" "<password file>" "$RUN/client" | tr '\n' ' ')"
    echo "server: $server_copy (login 127.0.0.1:$login_port, API 127.0.0.1:$api_port), store in $server_data"
    echo "umu-run: $UMU ($(sha256sum "$UMU" | cut -c1-16)…); GE-Proton: $PROTON"
    echo "wine channels (WINEDEBUG, with PROTON_LOG=1): $channels"
    echo "driver watchdog: watchdog.py sha256 $(sha256sum "$WATCHDOG" | cut -c1-16)…, scanning the namespace's tmpfs mounts"
    if [ "$desktop" = 1 ]; then
      echo "display :$display, the desktop (no screenshots, keys or clicks); timeout $tmo s"
    else
      echo "display :$display; timeout $tmo s; keys ${presses[*]-}; clicks ${clicks[*]-}"
    fi
  } > "$OUT/inputs.txt"
  find "$PFX" -iname '*.sys' -printf '%P\t%y\t%l\t%s\t%T@\n' | sort > "$OUT/sys-before.tsv"
  python3 -I "$WATCHDOG" collect --src "$PFX" --dest "$OUT/reg-before" --name system.reg --name user.reg
  grep -o '^\[System\\\\ControlSet00[0-9]\\\\Services\\\\[^]\\]*' "$OUT/reg-before/system.reg" | sort -u > "$OUT/services-before.txt" || true
  local bc
  mapfile -t bc < <(baseline_cmd "$OUT")
  "${bc[@]}"
  touch "$OUT/.start"

  local shots clicker typist inputs=()
  bare_inputs "$desktop" "$OUT" "$display" "$tmo"
  touch "$OUT/.running"

  # --- the run: fenji-server and fenji-launch in one namespace, started once the watchdog has armed
  local ns ne la bpf
  mapfile -t ns < <(namespace_cmd "$display" "$xauth")
  mapfile -t ne < <(namespace_env "$display" "$xauth")
  mapfile -t la < <(launch_args "$api_port" "$user" "$pwfile" "$RUN/client")
  bpf=$(seccomp_filter)
  set +e
  # shellcheck disable=SC2016 # the bash -c script expands its own positional parameters
  "${ns[@]}" "${ne[@]}" TMPDIR="$IO/tmp" PROTON_LOG=1 \
    PROTON_LOG_DIR="$IO" PROTON_USE_XALIA=0 WINEDEBUG="$channels" UMU_RUNTIME_UPDATE=0 \
    bash -c '
      W=$1 IO=$2 BIN=$3 SERVER=$4 LPORT=$5 APORT=$6 TMO=$7 PROTON=$8
      shift 8
      exec < /dev/null
      for _ in $(seq 200); do [ -e "$IO/.go" ] && break; sleep 0.05; done
      [ -e "$IO/.go" ] || { echo "watchdog never armed; client not started" >&2; exit 98; }
      RUST_LOG=fenji_server=trace "$BIN/fenji-server" --config "$SERVER" run > "$IO/server.log" 2>&1 &
      srv=$!
      up() { (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null; }
      for _ in $(seq 100); do
        up "$LPORT" && up "$APORT" && break
        kill -0 "$srv" 2>/dev/null || break
        sleep 0.1
      done
      up "$LPORT" && up "$APORT" || { echo "fenji-server not ready; client not started" >&2; exit 96; }
      timeout -k 10 "$TMO" "$BIN/fenji-launch" "$@" > "$IO/launch.out" 2> "$IO/launch.err"
      rc=$?
      # fenji-launch says so right before it starts the client; without that line, it never did.
      if grep -q "starting the client" "$IO/launch.err"; then echo "$rc" > "$IO/exit-code"; fi
      WINEPREFIX="$W/compat" "$PROTON/files/bin/wineserver" -k 2>/dev/null
      sleep 2
      kill "$srv" 2>/dev/null
      wait "$srv" 2>/dev/null
      [ -e "$IO/exit-code" ] || exit 95
      exit "$rc"
    ' _ "$WORK" "$IO" "$STUBBIN" "$server_copy" "$login_port" "$api_port" "$tmo" "$PROTON" \
    "${la[@]}" > "$OUT/namespace.out" 2>&1 7< <(printf '%b' "$bpf") &
  local spid=$! wc
  # Steam starting during the run stops it (the watchdog reads the host's /proc too).
  (
    while [ -e "$OUT/.running" ]; do
      if pgrep -x steam >/dev/null || pgrep -x steamwebhelper >/dev/null; then
        touch "$OUT/steam-started"
        kill -KILL "$spid" 2>/dev/null
        break
      fi
      sleep 2
    done
  ) &
  local steamwatch=$!
  mapfile -t wc < <(watch_cmd "$OUT" "$IO" "$spid" "${ns[@]}")
  "${wc[@]}" 2>> "$OUT/watchdog.err" &
  local wdpid=$!
  # Whichever ends first. If it is the watchdog (gate event, failure, or killed), the namespace
  # must not outlive it.
  local first="" frc wrc wdrc
  wait -n -p first "$spid" "$wdpid"
  frc=$?
  if [ "$first" = "$wdpid" ]; then
    wdrc=$frc
    kill -KILL "$spid" 2>/dev/null
    wait "$spid"
    wrc=$?
  else
    wrc=$frc
    wait "$wdpid"
    wdrc=$?
  fi
  set -e
  rm -f "$OUT/.running" "$IO/.go"
  kill "${inputs[@]}" "$steamwatch" 2>/dev/null || true
  wait "${inputs[@]}" "$steamwatch" 2>/dev/null || true
  local ran=0
  if [ -e "$IO/exit-code" ]; then ran=1; fi
  echo "$wrc" > "$OUT/namespace-exit-code"
  echo "$wdrc" > "$OUT/watchdog-exit-code"
  date -Is > "$OUT/ended"
  collect_bare_evidence || echo "run-bare: collecting the evidence failed (exit $?); the verdict stands" >&2
  echo "run $run done: fenji-launch exit $(cat "$IO/exit-code" 2>/dev/null || echo '?'), namespace exit $wrc, watchdog exit $wdrc"
  local v=0
  bare_verdict "$wdrc" "$wrc" "$ran" "$OUT" || v=$?
  exit "$v"
}

# run-client.sh's verdict, in this runner's words where they differ.
bare_verdict() {
  local wdrc=$1 wrc=$2 ran=$3 out=$4
  # A gate event (3) or a watchdog failure (4) wins: exit 3 is the signal to stop all runs.
  if [ -e "$out/steam-started" ]; then
    if [ "$wdrc" = 0 ]; then
      echo "Steam started during the run, so the run was stopped (CLAUDE.md, \"Bare-metal client runs\")" >&2
      return 1
    fi
    echo "Steam also started during the run (CLAUDE.md, \"Bare-metal client runs\")" >&2
  fi
  if [ "$wdrc" = 0 ] && [ "$ran" != 1 ]; then
    case "$wrc" in
      95) echo "fenji-launch didn't start the client (see $out/io/launch.err)" >&2; return 7 ;;
      96) echo "fenji-server wasn't ready (see $out/server.log); the client was not started" >&2; return 7 ;;
      98) echo "the watchdog never armed; the client was not started" >&2; return 5 ;;
      *) echo "the namespace ended before the client ran (exit $wrc); see $out/namespace.out" >&2; return 6 ;;
    esac
  fi
  verdict "$wdrc" "$wrc" "$ran" "$out"
}

# The evidence, copied out without following any link; uses cmd_bare_run's OUT, IO, RUN and PFX.
collect_bare_evidence() {
  local bad=0 r
  python3 -I "$WATCHDOG" collect --src "$IO" --dest "$OUT/io" --exclude .go --exclude tmp || bad=1
  if [ -f "$OUT/io/steam-1418100.log" ]; then mv "$OUT/io/steam-1418100.log" "$OUT/wine.log" || bad=1; fi
  if [ -f "$OUT/io/server.log" ]; then mv "$OUT/io/server.log" "$OUT/server.log" || bad=1; fi
  python3 -I "$WATCHDOG" collect --src "$RUN/client/bin64" --dest "$OUT" \
    --name gujianol.log --name Log_gujianol.htm || bad=1
  find "$PFX" -iname '*.sys' -printf '%P\t%y\t%l\t%s\t%T@\n' | sort > "$OUT/sys-after.tsv" || bad=1
  diff "$OUT/sys-before.tsv" "$OUT/sys-after.tsv" > "$OUT/sys-diff.txt"
  python3 -I "$WATCHDOG" collect --src "$PFX" --dest "$OUT/reg-after" --name system.reg --name user.reg || bad=1
  grep -o '^\[System\\\\ControlSet00[0-9]\\\\Services\\\\[^]\\]*' "$OUT/reg-after/system.reg" | sort -u > "$OUT/services-after.txt"
  diff "$OUT/services-before.txt" "$OUT/services-after.txt" > "$OUT/services-diff.txt"
  for r in system user; do
    diff "$OUT/reg-before/$r.reg" "$OUT/reg-after/$r.reg" > "$OUT/reg-$r.diff"
  done
  find "$RUN/client" "$PFX/drive_c" -newer "$OUT/.start" -type f -not -path '*/mesa_shader_cache/*' \
    -printf '%TT\t%s\t%p\n' 2>/dev/null | sort > "$OUT/new-files.txt"
  # Outside the watched root, where Wine's Z: or umu's container could put a file: new *.sys only.
  find "$HOME/.local/share/umu" /var/tmp -newer "$OUT/.start" -iname '*.sys' -printf '%TT\t%s\t%p\n' \
    2>/dev/null | sort > "$OUT/sys-elsewhere.txt"
  redact_tickets "$OUT" || bad=1
  return "$bad"
}

# Sourced by test-run-bare.sh for its functions; run as a command otherwise.
if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  case "${1:-}" in
    setup) shift; cmd_bare_setup "$@" ;;
    run) shift; cmd_bare_run "$@" ;;
    *) usage ;;
  esac
fi
