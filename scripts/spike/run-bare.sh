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
#     IBus's private sockets in ~/.cache/ibus are hidden too. Other sockets elsewhere in the real
#     filesystem stay reachable, and so does the audio daemon: the accepted residual, as its pulse
#     server takes load-module, so the client could load a network module on the host
#     (module-tunnel-sink, module-native-protocol-tcp; #148). --desktop adds the session's display
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
#       --password-file FILE (--display N | --desktop) [--timeout SECONDS] [--key T:KEY[/MS]]...
#       [--type T:TEXT]... [--click T:X,Y]...
#       [--clients 2 --rehearsed --username2 USER --password-file2 FILE
#        (--display2 M | with --desktop) [--second-after S] [--timeout2 S]]
#       [--launcher-app APPIMAGE] [--print]
#
# --launcher-app APPIMAGE (launcher plan Task 16) runs the desktop launcher in place of fenji-launch,
# under the same guards: its AppImage, unpacked into the run (RUN/launcher), starts with its data,
# config and cache folders inside the run, so with no settings file; PROTONPATH names the pinned
# GE-Proton and umu-run's folder comes first on PATH; the dedicated prefix is bound where the
# launcher keeps its own (RUN/data/fenji-launcher/prefix), so the launcher's Wine prefix is this one,
# primed and watched; and umu's runtime is bound where the launcher's umu-run looks for it with the
# run's XDG_DATA_HOME (RUN/data/umu), as fenji-launch's runs use it in place. GTK's folder dialog lists the run's client copy as a bookmark, "SoLO (this
# run)". The player's steps are --key, --type and --click on --display: --type
# T:@password types --password-file's first line through xkey.py --type-file, so the password is
# never on a command line or in a file of the run, and every input is followed by a screenshot
# (step-Ts-KIND.png). --username is optional there: the launcher's own form takes the name. The
# launcher's log says when the game runs and when it ends; the run ends when the game does, or at
# the timeout. Not with --clients 2.
# --print prints a run's plan (the namespace, its environment, and fenji-launch's arguments or the
# launcher's command) once the arguments have passed their checks, and starts nothing.
#
# --clients 2 (M4 world plan, Amendment B) adds a second client: a second reflink copy
# (RUN/client2), its own fenji-launch on the one prefix with its own account (--username2,
# --password-file2), started S seconds after the first (--second-after, default 30, at least 2),
# with its own timeout (--timeout2, default --timeout) and display (--display2 M, a dedicated nested
# one other than --display's, or with --desktop the session's for both). By Ruling R14, a live
# two-client bare run waits for its own umu-run rehearsal and the owner's go-ahead, so --clients 2
# is a usage error (exit 2) unless --rehearsed says both have happened (Ruling R25, #280); its
# script tests pass it. PROTON_LOG_DIR is $IO/1 and $IO/2, and the watchdog gets
# both logs in that order. --key, --type and --click take 2@T:... for client 2's display. Client 2's
# timeout ends its fenji-launch; if its gujianol.exe is still there 10 s later, the namespace ends
# that process by its own pid. If client 1's fenji-launch has ended when client 2 is due, client 2
# isn't started (io/2/not-started): a fresh wineserver would start Wine's drivers in a new
# winedevice.exe, which the watchdog's S2 takes for a gate event. Whether two umu-run sessions
# share one wineserver is Unknown (world.md 15.5), and a second would boot its own session in the
# prefix: once client 2 has started, the runner checks every 2 s that the namespace holds one
# wineserver at most, and a second stops the run at once (the namespace is killed, so neither saves
# the registry): exit 1, "a second wineserver for the prefix", and the prefix is marked, to be
# rebuilt as after a gate event.
#
# setup builds the prefix with `umu-run createprefix` inside the guard namespace, with umu's runtime
# update off (the namespace has no network anyway), then primes it: cmd.exe started once through
# Proton's launcher, so Proton's own service is in the watchdog's baseline (prefix_primed). A run
# refuses an unprimed prefix, and setup primes an existing one. A run whose watchdog doesn't exit
# 0 (a gate event, or a watchdog failure) marks the prefix (compat/fenji-gate-event), and setup and
# run refuse it until it is rebuilt (remove re/bare-metal/compat, then setup): a driver service that
# run left in the registry would start, unwatched, the next time Wine boots the prefix.
#
# run's --server config must bind 127.0.0.1 and name 127.0.0.1 as its public_host, its [store] path
# must lie in --server-data, a directory under re/bare-metal/, and --password-file holds the
# account's password on its first line. The evidence goes to re/bare-metal/out/NAME; a Fenji ticket
# found there is redacted in place. re/bare-metal, target/release, umu-run, GE-Proton and the
# password file must lie outside /run, /tmp and /dev/shm, links resolved: the namespace replaces
# those with its own empty tmpfs.
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
# 1 a guard or the workspace refused the run, Steam started during it, or a two-client run's
# namespace held a second wineserver; 2 a usage error.
set -euo pipefail
BARE_HERE=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
# shellcheck source=run-client.sh
. "$BARE_HERE/run-client.sh"
# Everything a bare run writes lives here. The pinned copy is the run's source
# (~/.config/fenji/env.sh sets FENJI_CLIENT_PATH).
WORK="$REPO/re/bare-metal"
PINNED_COPY="${FENJI_CLIENT_PATH:-}"
UMU="${FENJI_UMU_RUN:-$HOME/.local/bin/umu-run}"
# umu's runtime, where umu-run keeps it: XDG_DATA_HOME/umu.
UMU_DATA="${XDG_DATA_HOME:-$HOME/.local/share}/umu"
GAMEID=umu-1418100
# Where the display checks look: X lock files, the X socket directory, /proc (tests move all three).
X_LOCKS=/tmp
X_SOCKDIR=/tmp/.X11-unix
BARE_PROC=/proc
# The namespace's own tmpfs mounts (namespace_cmd), where a host path vanishes (tests move them).
NS_TMPFS=(/run /tmp /dev/shm)

usage() { awk 'NR > 1 && /^#/ { print; next } NR > 1 { exit }' "$BARE_HERE/run-bare.sh" >&2; exit 2; }
die() { echo "run-bare: $*" >&2; exit 1; }
misuse() { echo "run-bare: $*" >&2; exit 2; }

# The namespace's own tmpfs mount that hides host PATH (resolved), if one does (#148).
ns_hides() {
  local real m
  real=$(realpath -m -- "$1")
  for m in "${NS_TMPFS[@]}"; do
    case "$real/" in "$m"/*) echo "$m"; return 0 ;; esac
  done
  return 1
}

# Refuses host PATH, which the namespace must see, when one of the namespace's own mounts hides it.
check_seen() {
  local m
  if m=$(ns_hides "$2"); then die "$1 is under $m, which the namespace replaces with an empty tmpfs: $2"; fi
}

# The guards that hold for setup and for every run: Linux, Steam not running, no Wine left over,
# and re/bare-metal, umu-run and GE-Proton where the namespace sees them.
host_guards() {
  [ "$(uname -s)" = Linux ] || die "bare-metal runs are for the Linux game box only"
  [ "$(uname -m)" = x86_64 ] || die "x86_64 only (the seccomp filter knows x86 syscall numbers only)"
  if pgrep -x steam >/dev/null || pgrep -x steamwebhelper >/dev/null; then
    die "Steam is running; quit it first (CLAUDE.md, \"Bare-metal client runs\")"
  fi
  if pgrep -x wineserver >/dev/null || pgrep -x winedevice.exe >/dev/null; then
    die "Wine processes are already running on the host; stop them first"
  fi
  check_seen "re/bare-metal" "$WORK"
  check_seen "umu-run (FENJI_UMU_RUN)" "$UMU"
  check_seen "GE-Proton (FENJI_GE_PROTON)" "$PROTON"
  [ -x "$UMU" ] || die "umu-run not found at $UMU (run scripts/bootstrap.sh)"
  [ -x "$PROTON/proton" ] || die "GE-Proton not found at $PROTON (run scripts/bootstrap.sh)"
}

# The guard namespace's command line for display N: the real filesystem with its own /proc; empty
# /run, /tmp and /dev/shm, so no host daemon socket of those (D-Bus, systemd-resolved, the portal,
# agents) is there, bar the user's audio sockets and the display's X11 socket, bound back; Steam's
# pipe masked; IBus's private sockets hidden behind an empty tmpfs; loopback only; its own PID
# namespace (where the watchdog arms); the seccomp filter on fd 7; and nothing left when its parent
# goes. A second argument is a desktop run's X authority file, brought in read-only after the tmpfs
# mounts, which would hide it; a third, a two-client run's second nested display, whose socket is
# bound back beside the first; a fourth, where the desktop launcher keeps its prefix, which the
# dedicated prefix is bound over, and a fifth, where its umu-run looks for umu's runtime, which the
# host's is bound over (both --launcher-app).
namespace_cmd() {
  local display=$1 xauth=${2:-} display2=${3:-} prefix_at=${4:-} umu_at=${5:-}
  local rt=${XDG_RUNTIME_DIR:-/run/user/$(id -u)} s
  local cache=${XDG_CACHE_HOME:-$HOME/.cache}
  printf '%s\n' bwrap --dev-bind / / --proc /proc --tmpfs /run --perms 0700 --dir "$rt"
  for s in pipewire-0 pulse; do
    if [ -e "$rt/$s" ]; then printf '%s\n' --bind "$rt/$s" "$rt/$s"; fi
  done
  printf '%s\n' --tmpfs /tmp --bind "/tmp/.X11-unix/X$display" "/tmp/.X11-unix/X$display"
  if [ -n "$display2" ]; then printf '%s\n' --bind "/tmp/.X11-unix/X$display2" "/tmp/.X11-unix/X$display2"; fi
  printf '%s\n' --tmpfs /dev/shm
  if [ -e "$HOME/.steam/steam.pipe" ]; then printf '%s\n' --ro-bind /dev/null "$HOME/.steam/steam.pipe"; fi
  if [ -d "$HOME/.cache/ibus" ]; then printf '%s\n' --tmpfs "$HOME/.cache/ibus"; fi
  if [ "$cache" != "$HOME/.cache" ] && [ -d "$cache/ibus" ]; then printf '%s\n' --tmpfs "$cache/ibus"; fi
  if [ -n "$xauth" ]; then printf '%s\n' --ro-bind "$xauth" "$xauth"; fi
  if [ -n "$prefix_at" ]; then printf '%s\n' --bind "$WORK/compat" "$prefix_at"; fi
  if [ -n "$umu_at" ]; then printf '%s\n' --bind "$UMU_DATA" "$umu_at"; fi
  printf '%s\n' --unshare-net --unshare-pid --die-with-parent --seccomp 7 --
}

# Where the desktop launcher keeps its Wine prefix in run folder RUN: XDG_DATA_HOME/fenji-launcher.
app_prefix() { echo "$1/data/fenji-launcher/prefix"; }

# Where the desktop launcher's umu-run looks for umu's runtime in run folder RUN: XDG_DATA_HOME/umu.
# Without the host's there, umu-run wants to download it, and the namespace has no network (r6).
app_umu() { echo "$1/data/umu"; }

# The bookmark that lists run folder RUN's client copy in GTK's folder dialog (--launcher-app), as
# GTK reads it from XDG_CONFIG_HOME/gtk-3.0/bookmarks: a file URI and a label. Choosing the folder is
# then two clicks: a typed path is mangled by the dialog's completion (rehearsal r4).
app_bookmark() {
  python3 -I -c 'import sys, urllib.parse; print("file://" + urllib.parse.quote(sys.argv[1]) + " SoLO (this run)")' \
    "$1/client"
}

# The desktop launcher's command in run folder RUN (--launcher-app): its unpacked AppImage, with its
# data, config and cache folders in the run, the pinned GE-Proton and umu-run's folder first on PATH.
app_cmd() {
  local dir=$1
  printf '%s\n' env "XDG_DATA_HOME=$dir/data" "XDG_CONFIG_HOME=$dir/config" "XDG_CACHE_HOME=$dir/cache" \
    "PROTONPATH=$PROTON" "PATH=$(dirname -- "$UMU"):$PATH" "$dir/launcher/AppRun"
}

# The environment inside: no session bus, no agents, no Wayland (the client draws on X11 display N),
# no password, and no FENJI_LOGIN_BIND or FENJI_API_BIND, which would move fenji-server off the
# 127.0.0.1 binds its --server config was checked for (Ruling HP7).
namespace_env() {
  printf '%s\n' env -u WAYLAND_DISPLAY -u DBUS_SESSION_BUS_ADDRESS -u SSH_AUTH_SOCK -u FENJI_PASSWORD \
    -u FENJI_LOGIN_BIND -u FENJI_API_BIND "DISPLAY=:$1"
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
  local real home
  real=$(realpath -- "$PINNED_COPY")
  home=$(realpath -m -- "$HOME")
  case "$real/" in
    */steamapps/* | "$home"/.steam/* | "$home"/.local/share/Steam/*) die "the pinned copy is in a Steam library: $real" ;;
  esac
  [ -z "$(find "$PINNED_COPY" -type l -print -quit)" ] || die "the pinned copy holds a link; it must hold files only"
}

# The watchdog's command line for run OUT/IO and namespace PID, given the namespace's own command
# line (namespace_cmd's). It names the prefix, not umu's pfx link to it. S1 scans each tmpfs mount
# the namespace makes, through /proc/PID/root, less each path the namespace binds in (the audio and
# X11 sockets, the auth file): the rest of its filesystem is the host's, whose own Proton prefixes
# would trip that scan. A two-client run (the caller's clients=2) gives each client's log, in the
# order they start: $IO/1 and $IO/2.
watch_cmd() {
  local out=$1 io=$2 spid=$3 i
  shift 3
  local a=("$@") logs=(--log "$io/steam-1418100.log")
  if [ "${clients:-1}" = 2 ]; then logs=(--log "$io/1/steam-1418100.log" --log "$io/2/steam-1418100.log"); fi
  printf '%s\n' python3 -I "$WATCHDOG" watch --root "$WORK" --prefix "$WORK/compat" --out "$out" \
    --sandbox-pid "$spid" "${logs[@]}" --go "$io/.go"
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

# The wineserver processes below PID, the namespace's bwrap: a two-client run's backstop. One
# wineserver hosts the prefix; a second would boot its own session in it (Amendment B).
namespace_wineservers() {
  python3 -I - "$1" <<'PY'
import os, sys
kids, names = {}, {}
for d in os.listdir("/proc"):
    if d.isdigit():
        try:
            with open(f"/proc/{d}/stat") as f:
                st = f.read()
            with open(f"/proc/{d}/comm") as f:
                names[int(d)] = f.read().strip()
            kids.setdefault(int(st[st.rfind(")") + 2:].split()[1]), []).append(int(d))
        except (OSError, ValueError, IndexError):
            pass
n, stack = 0, [int(sys.argv[1])]
while stack:
    p = stack.pop()
    n += names.get(p) == "wineserver"
    stack += kids.get(p, [])
print(n)
PY
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
# names the GE-Proton build that primed it (run-client.sh's PRIMED_MARK, proton_build and
# prefix_marked). The prefix is primed when the mark names this build (a new build upgrades the
# prefix, and its services would be new too) and, if this GE-Proton ships the bridge, the bridge
# service has reached the registry on disk.
prefix_primed() {
  prefix_marked || return 1
  [ ! -e "$PROTON/files/lib/wine/discord-rpc-bridge/bridge.exe" ] ||
    grep -q '^\[System\\\\ControlSet00[0-9]\\\\Services\\\\discord-bridge\]' "$WORK/compat/system.reg"
}

# A run whose watchdog didn't end cleanly (a gate event, or a watchdog failure) marks the prefix,
# and setup and run refuse it until it is rebuilt: a driver service that run left in the registry
# would start, unwatched, the next time Wine boots the prefix, as setup's prime does (#148).
GATE_MARK=fenji-gate-event
# Creates FILE holding TEXT, refusing anything already there (a file, a link, a FIFO, a device),
# as watchdog.py's create_exclusive does: the client can write the prefix, so this neither writes
# through a link it planted nor waits on a FIFO.
create_new() {
  python3 -I -c 'import os, sys
fd = os.open(sys.argv[1], os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o644)
os.write(fd, sys.argv[2].encode())
os.close(fd)' "$1" "$2"
}
check_unmarked() {
  local m="$WORK/compat/$GATE_MARK" what=""
  [ -e "$m" ] || [ -L "$m" ] || return 0
  if [ -f "$m" ] && [ ! -L "$m" ]; then what=" ($(head -c 200 -- "$m" | head -n 1))"; fi
  die "the prefix saw a gate event or a watchdog failure$what; rebuild it before anything runs in it again: remove $WORK/compat, then run: $0 setup"
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
  check_unmarked
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

# A two-client bare run's options (Amendment B), after the first client's. Uses and sets
# cmd_bare_run's clients, rehearsed, user, user2, pwfile2, desktop, display, display2, second_after,
# second_after_given, tmo2_given and the 2@ inputs.
check_two_bare() {
  if [ "$clients" = 1 ]; then
    [ "$rehearsed" = 0 ] || misuse "--rehearsed needs --clients 2: it says the two-client mode was rehearsed"
    [ -z "$user2" ] || misuse "--username2 needs --clients 2"
    [ -z "$pwfile2" ] || misuse "--password-file2 needs --clients 2"
    [ -z "$display2" ] || misuse "--display2 needs --clients 2"
    [ "$second_after_given" = 0 ] || misuse "--second-after needs --clients 2"
    [ "$tmo2_given" = 0 ] || misuse "--timeout2 needs --clients 2"
    if [ "$((${#keys2[@]} + ${#types2[@]} + ${#clicks2[@]}))" -gt 0 ]; then
      misuse "2@ inputs are client 2's: they need --clients 2"
    fi
    return 0
  fi
  [ -n "$user2" ] || misuse "--clients 2 needs --username2: client 2 signs in with its own account"
  [[ "$user2" =~ ^[A-Za-z0-9_.-]{1,32}$ ]] || misuse "--username2 takes the account's name"
  [ "$user2" != "$user" ] ||
    misuse "--username2 must be another account than --username: a second login of one account replaces the first"
  [ -n "$pwfile2" ] || misuse "--clients 2 needs --password-file2"
  if [ ! -f "$pwfile2" ] || [ ! -r "$pwfile2" ]; then misuse "--password-file2: can't read '$pwfile2'"; fi
  pwfile2=$(realpath -- "$pwfile2")
  local hidden
  if hidden=$(ns_hides "$pwfile2"); then
    misuse "--password-file2 is under $hidden, which the namespace replaces with an empty tmpfs: $pwfile2"
  fi
  if [ "$desktop" = 1 ]; then
    [ -z "$display2" ] || misuse "--desktop is both clients' display: no --display2"
  else
    [ -n "$display2" ] || misuse "--clients 2 needs --display2 M: client 2's own dedicated nested display"
    check_display_number "$display2"
    [ "$display2" != "$display" ] ||
      misuse "--display2 must be another display than --display (:$display): two client windows on one display stack"
  fi
  [ "$second_after" -ge 2 ] ||
    misuse "--second-after takes at least 2 s: the watchdog tells the clients apart by the order they start"
}

cmd_bare_run() {
  [ $# -ge 1 ] || usage
  local run=$1
  shift
  local tmo=300 display="" display_given=0 desktop=0 xauth="" user="" pwfile="" keys=() types=() clicks=() presses=()
  local server_cfg="" server_data="" launch="" launch_password="" ports_given=0 stub_args=()
  local clients=1 user2="" pwfile2="" display2="" second_after=30 second_after_given=0 tmo2="" tmo2_given=0
  local keys2=() types2=() clicks2=() rehearsed=0
  local app="" print=0 type_secret="" shot_after_inputs="" prefix_at="" umu_at=""
  while [ $# -gt 0 ]; do
    case "$1" in
      --launcher-app) [ $# -ge 2 ] || usage; app=$2; shift 2 ;;
      --print) print=1; shift ;;
      --server) [ $# -ge 2 ] || usage; server_cfg=$2; shift 2 ;;
      --server-data) [ $# -ge 2 ] || usage; server_data=$2; shift 2 ;;
      --username) [ $# -ge 2 ] || usage; user=$2; shift 2 ;;
      --password-file) [ $# -ge 2 ] || usage; pwfile=$2; shift 2 ;;
      --timeout) [ $# -ge 2 ] || usage; tmo=$2; shift 2 ;;
      --display) [ $# -ge 2 ] || usage; display=$2; display_given=1; shift 2 ;;
      --desktop) desktop=1; shift ;;
      --key)
        [ $# -ge 2 ] || usage
        case "$2" in
          2@*) keys2+=("${2#2@}"); presses+=("2:key:${2#2@}") ;;
          *) keys+=("$2"); presses+=("1:key:$2") ;;
        esac
        shift 2 ;;
      --type)
        [ $# -ge 2 ] || usage
        case "$2" in
          2@*) types2+=("${2#2@}"); presses+=("2:type:${2#2@}") ;;
          *) types+=("$2"); presses+=("1:type:$2") ;;
        esac
        shift 2 ;;
      --click)
        [ $# -ge 2 ] || usage
        case "$2" in 2@*) clicks2+=("${2#2@}") ;; *) clicks+=("$2") ;; esac
        shift 2 ;;
      --clients) [ $# -ge 2 ] || usage; clients=$2; shift 2 ;;
      --rehearsed) rehearsed=1; shift ;;
      --username2) [ $# -ge 2 ] || usage; user2=$2; shift 2 ;;
      --password-file2) [ $# -ge 2 ] || usage; pwfile2=$2; shift 2 ;;
      --display2) [ $# -ge 2 ] || usage; display2=$2; shift 2 ;;
      --second-after) [ $# -ge 2 ] || usage; second_after=$2; second_after_given=1; shift 2 ;;
      --timeout2) [ $# -ge 2 ] || usage; tmo2=$2; tmo2_given=1; shift 2 ;;
      *) usage ;;
    esac
  done
  case "$run" in '' | */* | .* | *[!A-Za-z0-9._-]*) die "bad run name: '$run'" ;; esac
  case "$tmo" in '' | *[!0123456789]*) die "--timeout takes whole seconds" ;; esac
  case "$clients" in 1 | 2) ;; *) misuse "--clients takes 1 or 2, not '$clients'" ;; esac
  if [ -n "$app" ]; then
    [ "$clients" = 1 ] || misuse "--launcher-app runs one desktop launcher: no --clients 2"
    { [ -f "$app" ] && [ -x "$app" ]; } ||
      misuse "--launcher-app: no AppImage to run at '$app' (build one: cd apps/launcher && cargo tauri build --bundles appimage)"
    app=$(realpath -- "$app")
    local app_hidden
    if app_hidden=$(ns_hides "$app"); then misuse "--launcher-app is under $app_hidden, which the namespace replaces with an empty tmpfs: $app"; fi
    shot_after_inputs=1
  fi
  # Ruling R14, in code by Ruling R25 (#280): checked first, so nothing else about client 2 matters.
  if [ "$clients" = 2 ] && [ "$rehearsed" = 0 ]; then
    misuse "--clients 2 waits for its own umu-run rehearsal and the owner's go-ahead (Ruling R14); add --rehearsed only once it has both"
  fi
  case "$second_after" in '' | *[!0123456789]*) misuse "--second-after takes whole seconds, not '$second_after'" ;; esac
  if [ "$tmo2_given" = 1 ]; then
    case "$tmo2" in '' | *[!0123456789]*) misuse "--timeout2 takes whole seconds, not '$tmo2'" ;; esac
  else
    tmo2=$tmo
  fi
  [ "$display_given" = 0 ] || check_display_number "$display"
  if [ "$desktop" = 1 ] && { [ ${#presses[@]} -gt 0 ] || [ ${#clicks[@]} -gt 0 ] || [ ${#clicks2[@]} -gt 0 ]; }; then
    misuse "--desktop takes no --key, --type or --click: they would go to whatever has focus on the desktop"
  fi
  [ -n "$server_cfg" ] || misuse "--server is required: the client plays against a local fenji-server"
  [ -n "$server_data" ] || misuse "--server-data is required: the server's store lives there"
  # The desktop launcher's own form takes the name, so a launcher run needs none.
  if [ -z "$app" ] || [ -n "$user" ]; then
    [[ "$user" =~ ^[A-Za-z0-9_.-]{1,32}$ ]] || misuse "--username takes the account's name"
  fi
  if [ -z "$pwfile" ] || [ ! -f "$pwfile" ] || [ ! -r "$pwfile" ]; then
    misuse "--password-file: can't read '$pwfile'"
  fi
  pwfile=$(realpath -- "$pwfile")
  # --type T:@password types this file's first line (start_inputs).
  type_secret=$pwfile
  local hidden
  if hidden=$(ns_hides "$pwfile"); then
    misuse "--password-file is under $hidden, which the namespace replaces with an empty tmpfs: $pwfile"
  fi
  check_seen "target/release" "$STUBBIN"
  local c
  for c in "${clicks[@]}" "${clicks2[@]}"; do
    [[ "$c" =~ ^[0-9]+:[0-9]+,[0-9]+$ ]] || die "--click takes T:X,Y in whole numbers, not '$c'"
  done
  check_presses
  check_two_bare
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
  # Client 2's nested display is checked as the first one is, the session's own display included.
  local d2=""
  if [ "$clients" = 2 ] && [ "$desktop" = 0 ]; then
    d2=$display2
    (display=$display2 choose_display 0 1) || exit $?
  fi
  if [ -n "$app" ]; then prefix_at=$(app_prefix "$WORK/runs/$run") umu_at=$(app_umu "$WORK/runs/$run"); fi
  if [ "$print" = 1 ]; then
    namespace_cmd "$display" "$xauth" "$d2" "$prefix_at" "$umu_at"
    namespace_env "$display" "$xauth"
    if [ -n "$app" ]; then
      app_cmd "$WORK/runs/$run"
    else
      printf '%s\n' "$STUBBIN/fenji-launch"
      launch_args "$api_port" "$user" "$pwfile" "$WORK/runs/$run/client"
    fi
    exit 0
  fi
  host_guards
  check_unmarked
  prefix_ready || die "no prefix; run: $0 setup"
  if [ -f "$WORK/compat/$PRIMED_MARK" ] && [ "$(cat -- "$WORK/compat/$PRIMED_MARK")" != "$(proton_build)" ]; then
    die "the prefix was primed by another GE-Proton build, or before builds were recorded, and this one would upgrade it during the run; run once: $0 setup"
  fi
  prefix_primed || die "the prefix hasn't been through Proton's launcher, so Proton's own service isn't in the watchdog's baseline; run: $0 setup"
  need_display "$display"
  nested_display_ok "$desktop" "$display"
  if [ -n "$d2" ]; then
    need_display "$d2"
    nested_display_ok 0 "$d2"
  fi
  check_pinned_copy
  [ -x "$STUBBIN/fenji-server" ] || die "build the server first: cargo build --release -p fenji-server"
  [ -n "$app" ] || [ -x "$STUBBIN/fenji-launch" ] || die "build the launcher first: cargo build --release -p fenji-launcher"
  local OUT="$WORK/out/$run" IO="$WORK/io/$run" RUN="$WORK/runs/$run" PFX="$WORK/compat"
  if [ -e "$OUT" ] || [ -e "$IO" ] || [ -e "$RUN" ]; then die "run $run exists already; pick a new name"; fi
  mkdir -p "$OUT" "$IO/tmp" "$IO/server" "$RUN/client"
  cp -a --reflink=always -- "$PINNED_COPY/." "$RUN/client/" ||
    die "the client copy needs a filesystem with reflinks (btrfs, XFS); nothing was started"
  if [ -n "$app" ]; then
    # The launcher's own folders start empty, bar GTK's bookmark of the client copy, and its
    # prefix's mount point waits for the bind.
    mkdir -p "$prefix_at" "$RUN/config/gtk-3.0" "$RUN/cache"
    app_bookmark "$RUN" > "$RUN/config/gtk-3.0/bookmarks"
    if ! (cd "$RUN" && "$app" --appimage-extract > "$IO/extract.log" 2>&1) || ! mv -- "$RUN/squashfs-root" "$RUN/launcher"; then
      die "couldn't unpack $app (see $IO/extract.log); nothing was started"
    fi
  fi
  if [ "$clients" = 2 ]; then
    mkdir -p "$IO/1" "$IO/2" "$RUN/client2"
    cp -a --reflink=always -- "$PINNED_COPY/." "$RUN/client2/" ||
      die "the client copy needs a filesystem with reflinks (btrfs, XFS); nothing was started"
  fi
  local server_copy="$IO/server/fenji.toml" channels
  cp -- "$server_cfg" "$server_copy"
  channels=$(watch_channels "$CHANNELS_DEFAULT")

  # --- before: inputs and the AGT baseline
  {
    echo "run $run (bare metal), $(date -Is)"
    echo "guards: Linux x86_64; Steam not running (and watched); no Wine running; reflink copy of $PINNED_COPY; prefix $PFX"
    echo "namespace: $(namespace_cmd "$display" "$xauth" "$d2" "$prefix_at" "$umu_at" | tr '\n' ' ')$(namespace_env "$display" "$xauth" | tr '\n' ' ')"
    if [ -n "$app" ]; then
      echo "launch: the desktop launcher $app, unpacked in $RUN/launcher: $(app_cmd "$RUN" | tr '\n' ' '); the prefix $PFX bound at $prefix_at, umu's runtime $UMU_DATA at $umu_at; the player's client folder $RUN/client"
    else
      echo "launch: fenji-launch $(launch_args "$api_port" "$user" "<password file>" "$RUN/client" | tr '\n' ' ')"
    fi
    if [ "$clients" = 2 ]; then
      echo "client 2: fenji-launch $(launch_args "$api_port" "$user2" "<password file 2>" "$RUN/client2" | tr '\n' ' ')"
      echo "client 2: ${d2:+display :$d2, }started $second_after s after client 1, timeout $tmo2 s; Proton logs in $IO/1 and $IO/2; backstop: one wineserver at most once client 2 has started"
    fi
    echo "server: $server_copy (login 127.0.0.1:$login_port, API 127.0.0.1:$api_port), store in $server_data"
    echo "umu-run: $UMU ($(sha256sum "$UMU" | cut -c1-16)…); GE-Proton: $PROTON"
    echo "wine channels (WINEDEBUG, with PROTON_LOG=1): $channels"
    echo "driver watchdog: watchdog.py sha256 $(sha256sum "$WATCHDOG" | cut -c1-16)…, scanning the namespace's tmpfs mounts"
    if [ "$desktop" = 1 ]; then
      echo "display :$display, the desktop (no screenshots, keys or clicks); timeout $tmo s"
    else
      echo "display :$display; timeout $tmo s; keys ${presses[*]-}; clicks ${clicks[*]-}; client 2 clicks ${clicks2[*]-}"
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

  # --- the run: fenji-server and fenji-launch (one per client) in one namespace, started once the
  # watchdog has armed
  local ns ne la la2=() bpf
  mapfile -t ns < <(namespace_cmd "$display" "$xauth" "$d2" "$prefix_at" "$umu_at")
  mapfile -t ne < <(namespace_env "$display" "$xauth")
  if [ -n "$app" ]; then
    mapfile -t la < <(app_cmd "$RUN")
  else
    mapfile -t la < <(launch_args "$api_port" "$user" "$pwfile" "$RUN/client")
  fi
  if [ "$clients" = 2 ]; then mapfile -t la2 < <(launch_args "$api_port" "$user2" "$pwfile2" "$RUN/client2"); fi
  bpf=$(seccomp_filter)
  set +e
  # shellcheck disable=SC2016 # the bash -c script expands its own positional parameters
  "${ns[@]}" "${ne[@]}" TMPDIR="$IO/tmp" PROTON_LOG=1 \
    PROTON_LOG_DIR="$IO" PROTON_USE_XALIA=0 WINEDEBUG="$channels" UMU_RUNTIME_UPDATE=0 \
    bash -c "$(declare -f end_client)"'
      W=$1 IO=$2 BIN=$3 SERVER=$4 LPORT=$5 APORT=$6 TMO=$7 PROTON=$8 CLIENTS=$9 AFTER=${10}
      TMO2=${11} D2=${12} N1=${13} EXE2=${14} APPLOG=${15}
      shift 15
      LA1=("${@:1:$N1}") LA2=("${@:$((N1 + 1))}")
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
      # Each client is a fenji-launch of its own; in a two-client run each has its own Proton log.
      (
        if [ "$CLIENTS" = 2 ]; then export PROTON_LOG_DIR="$IO/1"; fi
        if [ -n "$APPLOG" ]; then
          # The desktop launcher stays open after the game; its log says when the game runs and
          # when it ends, with its code. The run ends with the game, or at the timeout.
          timeout -k 10 "$TMO" "${LA1[@]}" > "$IO/launcher.out" 2> "$IO/launcher.err" &
          app=$!
          while kill -0 "$app" 2>/dev/null; do
            if grep -qs "the game ended" "$APPLOG"/launcher*.log; then
              sleep 2
              kill "$app" 2>/dev/null
              break
            fi
            sleep 1
          done
          wait "$app"
          rc=$?
          if grep -qs "the game is running" "$APPLOG"/launcher*.log; then
            code=$(grep -hs "the game ended" "$APPLOG"/launcher*.log | sed -n "s/.*code=\(-\{0,1\}[0-9]*\).*/\1/p" | tail -n 1)
            echo "${code:-$rc}" > "$IO/exit-code"
          fi
          exit "$rc"
        fi
        timeout -k 10 "$TMO" "$BIN/fenji-launch" "${LA1[@]}" > "$IO/launch.out" 2> "$IO/launch.err"
        rc=$?
        # fenji-launch says so right before it starts the client; without that line, it never did.
        if grep -q "starting the client" "$IO/launch.err"; then echo "$rc" > "$IO/exit-code"; fi
        exit "$rc"
      ) &
      c1=$!
      c2=""
      if [ "$CLIENTS" = 2 ]; then
        sleep "$AFTER"
        if ! kill -0 "$c1" 2>/dev/null; then
          # Client 1 has ended, and the wineserver may be gone with it: client 2 would boot a fresh
          # one, which starts Wine drivers in a new winedevice.exe, and S2 takes that for a gate
          # event (#244).
          echo "client 1 had ended when client 2 was due; client 2 not started" >&2
          : > "$IO/2/not-started"
        else
          # From here the runner checks that the namespace holds one wineserver at most.
          : > "$IO/2/.started"
          (
            export PROTON_LOG_DIR="$IO/2"
            if [ -n "$D2" ]; then export DISPLAY=":$D2"; fi
            timeout -k 10 "$TMO2" "$BIN/fenji-launch" "${LA2[@]}" > "$IO/2/launch.out" 2> "$IO/2/launch.err"
            rc=$?
            if grep -q "starting the client" "$IO/2/launch.err"; then echo "$rc" > "$IO/exit-code-2"; fi
            # Its timeout ends fenji-launch; a client 2 still there 10 s later is ended by its own pid
            # (wineserver -k would end client 1 too).
            sleep 10
            { date +%s; end_client "$EXE2"; } > "$IO/2/ended-by-pid"
            exit "$rc"
          ) &
          c2=$!
        fi
      fi
      wait "$c1"
      rc=$?
      [ -z "$c2" ] || wait "$c2"
      WINEPREFIX="$W/compat" "$PROTON/files/bin/wineserver" -k 2>/dev/null
      sleep 2
      kill "$srv" 2>/dev/null
      wait "$srv" 2>/dev/null
      [ -e "$IO/exit-code" ] || exit 95
      exit "$rc"
    ' _ "$WORK" "$IO" "$STUBBIN" "$server_copy" "$login_port" "$api_port" "$tmo" "$PROTON" \
    "$clients" "$second_after" "$tmo2" "$d2" "${#la[@]}" "$RUN/client2/bin64/gujianol.exe" \
    "${app:+$RUN/data/fenji-launcher/logs}" \
    "${la[@]}" "${la2[@]}" > "$OUT/namespace.out" 2>&1 7< <(printf '%b' "$bpf") &
  local spid=$! wc
  # Steam starting during the run stops it (the watchdog reads the host's /proc too), and so does a
  # second wineserver in a two-client run's namespace once client 2 has started.
  (
    while [ -e "$OUT/.running" ]; do
      if pgrep -x steam >/dev/null || pgrep -x steamwebhelper >/dev/null; then
        touch "$OUT/steam-started"
        kill -KILL "$spid" 2>/dev/null
        break
      fi
      if [ "$clients" = 2 ] && [ -e "$IO/2/.started" ] && [ "$(namespace_wineservers "$spid")" -gt 1 ]; then
        touch "$OUT/second-wineserver"
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
  # The mark goes where the client could write: create_new refuses anything already there, and
  # anything there keeps the prefix refused anyway. A second wineserver marks it too: it booted its
  # own session in the prefix.
  local why=""
  if [ "$wdrc" != 0 ]; then why="watchdog exit $wdrc"; fi
  if [ -e "$OUT/second-wineserver" ]; then why="${why:+$why, }a second wineserver for the prefix"; fi
  if [ -n "$why" ] &&
    ! create_new "$PFX/$GATE_MARK" "run $run: $why, $(date -Is)"$'\n' 2> /dev/null; then
    echo "run-bare: couldn't write the gate-event mark $PFX/$GATE_MARK; don't use this prefix again: remove $WORK/compat, then run: $0 setup" >&2
  fi
  rm -f "$OUT/.running" "$IO/.go"
  kill "${inputs[@]}" "$steamwatch" 2>/dev/null || true
  wait "${inputs[@]}" "$steamwatch" 2>/dev/null || true
  local ran=0
  if [ -e "$IO/exit-code" ]; then ran=1; fi
  echo "$wrc" > "$OUT/namespace-exit-code"
  echo "$wdrc" > "$OUT/watchdog-exit-code"
  date -Is > "$OUT/ended"
  collect_bare_evidence || echo "run-bare: collecting the evidence failed (exit $?); the verdict stands" >&2
  local two=""
  if [ "$clients" = 2 ] && [ -e "$IO/2/not-started" ]; then
    two=", client 2 not started (client 1 had ended when it was due)"
  elif [ "$clients" = 2 ]; then
    two=", client 2's fenji-launch exit $(cat "$IO/exit-code-2" 2>/dev/null || echo '?')"
  fi
  local what="fenji-launch exit"
  if [ -n "$app" ]; then what="game exit"; fi
  echo "run $run done: $what $(cat "$IO/exit-code" 2>/dev/null || echo '?')$two, namespace exit $wrc, watchdog exit $wdrc"
  local v=0
  bare_verdict "$wdrc" "$wrc" "$ran" "$OUT" || v=$?
  exit "$v"
}

# run-client.sh's verdict, in this runner's words where they differ.
bare_verdict() {
  local wdrc=$1 wrc=$2 ran=$3 out=$4
  # A gate event (3) or a watchdog failure (4) wins: exit 3 is the signal to stop all runs.
  if [ -e "$out/second-wineserver" ]; then
    if [ "$wdrc" = 0 ]; then
      echo "a second wineserver for the prefix: the namespace held two once client 2 had started, so the run was stopped; the prefix is marked, rebuild it (remove $WORK/compat, then: $0 setup)" >&2
      return 1
    fi
    echo "a second wineserver for the prefix too: the namespace held two once client 2 had started" >&2
  fi
  if [ -e "$out/steam-started" ]; then
    if [ "$wdrc" = 0 ]; then
      echo "Steam started during the run, so the run was stopped (CLAUDE.md, \"Bare-metal client runs\")" >&2
      return 1
    fi
    echo "Steam also started during the run (CLAUDE.md, \"Bare-metal client runs\")" >&2
  fi
  if [ "$wdrc" = 0 ] && [ "$ran" != 1 ]; then
    case "$wrc" in
      95)
        if [ -n "${app:-}" ]; then
          echo "the desktop launcher didn't start the game (see its logs in $out/launcher and $out/io/launcher.err)" >&2
        else
          echo "fenji-launch didn't start the client (see $out/io/launch.err)" >&2
        fi
        return 7 ;;
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
  local copies=("$RUN/client")
  if [ "${clients:-1}" = 2 ]; then
    copies+=("$RUN/client2")
    if [ -f "$OUT/io/1/steam-1418100.log" ]; then mv "$OUT/io/1/steam-1418100.log" "$OUT/wine.log" || bad=1; fi
    if [ -f "$OUT/io/2/steam-1418100.log" ]; then mv "$OUT/io/2/steam-1418100.log" "$OUT/wine-2.log" || bad=1; fi
    python3 -I "$WATCHDOG" collect --src "$RUN/client2/bin64" --dest "$OUT/client2" \
      --name gujianol.log --name Log_gujianol.htm || bad=1
  elif [ -f "$OUT/io/steam-1418100.log" ]; then
    mv "$OUT/io/steam-1418100.log" "$OUT/wine.log" || bad=1
  fi
  if [ -f "$OUT/io/server.log" ]; then mv "$OUT/io/server.log" "$OUT/server.log" || bad=1; fi
  # The desktop launcher's own files (--launcher-app): its logs (the game's output in game.log) and
  # its settings, which hold no password, token or ticket.
  if [ -n "${app:-}" ]; then
    mkdir -p "$OUT/launcher" || bad=1
    if [ -d "$RUN/data/fenji-launcher/logs" ]; then cp -r -- "$RUN/data/fenji-launcher/logs" "$OUT/launcher/" || bad=1; fi
    if [ -f "$RUN/config/fenji-launcher/settings.json" ]; then cp -- "$RUN/config/fenji-launcher/settings.json" "$OUT/launcher/" || bad=1; fi
  fi
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
  find "${copies[@]}" "$PFX/drive_c" -newer "$OUT/.start" -type f -not -path '*/mesa_shader_cache/*' \
    -printf '%TT\t%s\t%p\n' 2>/dev/null | sort > "$OUT/new-files.txt"
  # Outside the watched root, where Wine's Z: or umu's container could put a file: new *.sys only.
  find "$UMU_DATA" /var/tmp -newer "$OUT/.start" -iname '*.sys' -printf '%TT\t%s\t%p\n' \
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
