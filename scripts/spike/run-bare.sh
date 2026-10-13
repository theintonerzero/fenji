#!/usr/bin/env bash
# Runs the unmodified pinned SoLO client once outside the sandbox, as a player starts it: through
# `fenji-launch play`, which checks the client folder against the pinned build, gets the launcher's
# Wine prefix ready with umu-run, signs in to a local fenji-server and starts the client with a
# fresh ticket in its environment (#139; the owner's decision on #114, 2026-10-13). Every run keeps
# all the guards of CLAUDE.md's "Bare-metal client runs":
#   - umu-run in a dedicated prefix (re/bare-metal/compat); the client from a reflink copy of the
#     pinned copy (re/runs/client, from `run-client.sh setup`) made for the run, never the Steam
#     library;
#   - Steam not running;
#   - a local fenji-server, which shares the client's network namespace: that namespace has only
#     loopback, so the client can reach the Fenji server and nothing else;
#   - the AGT driver watchdog armed on the namespace's PID namespace before anything in it starts,
#     as in sandboxed runs; a gate event stops the run (exit 3);
#   - never --gf-sdk=mocked: fenji-launch passes no SDK flag at all.
# The namespace is `bwrap --dev-bind / / --proc /proc --unshare-net --unshare-pid`: the real
# filesystem, home, GPU, audio and display, as on a player's machine. Its own /proc matters: without
# it, a nested bwrap (umu's pressure-vessel) looks its child up in the host's /proc by a namespace
# PID and hangs. The watchdog's scan of sandbox tmpfs mounts is off (--no-tmpfs-scan): there are
# none, and the host's own Proton prefixes would trip it. Its scan of re/bare-metal, its process
# rules, its log rules and its registry rule work as in sandboxed runs. Linux only.
#
#   scripts/spike/run-bare.sh setup
#   scripts/spike/run-bare.sh run NAME --server FILE --server-data DIR --username USER
#       --password-file FILE [--timeout SECONDS] [--display N] [--key T:KEY]... [--type T:TEXT]...
#       [--click T:X,Y]...
#
# setup builds the prefix with `umu-run createprefix` inside the guard namespace, with umu's runtime
# update off (the namespace has no network anyway). run's --server config must bind 127.0.0.1, its
# [store] path must lie in --server-data, a directory under re/bare-metal/, and --password-file holds
# the account's password on its first line. The display is the nested one (:99 unless --display):
# Xwayland on the real GPU, where screenshots and key presses can reach the client. The evidence
# goes to re/bare-metal/out/NAME; a Fenji ticket found there is redacted in place.
#
# Exit: 0 the run finished; 3 GATE EVENT (the watchdog stopped the run: stop all runs, report); 4 the
# watchdog failed; 5 the watchdog never armed (the client was not started); 6 the namespace ended
# before the client ran; 7 fenji-server or fenji-launch failed before the client started; 1 a guard
# or the workspace refused the run; 2 a usage error.
set -euo pipefail
BARE_HERE=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
# shellcheck source=run-client.sh
. "$BARE_HERE/run-client.sh"
# Everything a bare run writes lives here; the sandboxed runs' copy of the pinned client is its
# source.
WORK="$REPO/re/bare-metal"
PINNED_COPY="$REPO/re/runs/client"
UMU="${FENJI_UMU_RUN:-$HOME/.local/bin/umu-run}"
GAMEID=umu-1418100

usage() { awk 'NR > 1 && /^#/ { print; next } NR > 1 { exit }' "$BARE_HERE/run-bare.sh" >&2; exit 2; }
die() { echo "run-bare: $*" >&2; exit 1; }
misuse() { echo "run-bare: $*" >&2; exit 2; }

# The guards that hold for setup and for every run: Linux, Steam not running, no Wine left over.
host_guards() {
  [ "$(uname -s)" = Linux ] || die "bare-metal runs are for the Linux game box only"
  if pgrep -x steam >/dev/null || pgrep -x steamwebhelper >/dev/null; then
    die "Steam is running; quit it first (CLAUDE.md, \"Bare-metal client runs\")"
  fi
  if pgrep -x wineserver >/dev/null || pgrep -x winedevice.exe >/dev/null; then
    die "Wine processes are already running on the host; stop them first"
  fi
  [ -x "$UMU" ] || die "umu-run not found at $UMU (run scripts/bootstrap.sh)"
  [ -x "$PROTON/proton" ] || die "GE-Proton not found at $PROTON (run scripts/bootstrap.sh)"
}

# The guard namespace's command line: the real filesystem with its own /proc, loopback only, its
# own PID namespace (where the watchdog arms), and nothing left when its parent goes.
namespace_cmd() {
  printf '%s\n' bwrap --dev-bind / / --proc /proc --unshare-net --unshare-pid --die-with-parent --
}

# The watchdog's command line for run OUT/IO and namespace PID.
watch_cmd() {
  local out=$1 io=$2 spid=$3
  printf '%s\n' python3 -I "$WATCHDOG" watch --root "$WORK" --out "$out" --sandbox-pid "$spid" \
    --log "$io/steam-1418100.log" --go "$io/.go" --no-tmpfs-scan
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

cmd_bare_setup() {
  [ $# -eq 0 ] || usage
  host_guards
  if prefix_ready; then
    echo "the prefix exists: $WORK/compat"
    return 0
  fi
  mkdir -p "$WORK/compat"
  local ns
  mapfile -t ns < <(namespace_cmd)
  # umu-launcher 1.4 ends createprefix with exit 1 once the prefix is built: the prefix decides.
  "${ns[@]}" env UMU_RUNTIME_UPDATE=0 PROTONPATH="$PROTON" WINEPREFIX="$WORK/compat" \
    GAMEID="$GAMEID" "$UMU" createprefix > "$WORK/setup.log" 2>&1 || true
  prefix_ready || die "createprefix didn't build the prefix; see $WORK/setup.log"
  echo "prefix ready: $WORK/compat"
}

cmd_bare_run() {
  [ $# -ge 1 ] || usage
  local run=$1
  shift
  local tmo=300 display=99 user="" pwfile="" keys=() types=() clicks=() presses=()
  local server_cfg="" server_data="" launch="" launch_password="" ports_given=0 stub_args=()
  while [ $# -gt 0 ]; do
    case "$1" in
      --server) [ $# -ge 2 ] || usage; server_cfg=$2; shift 2 ;;
      --server-data) [ $# -ge 2 ] || usage; server_data=$2; shift 2 ;;
      --username) [ $# -ge 2 ] || usage; user=$2; shift 2 ;;
      --password-file) [ $# -ge 2 ] || usage; pwfile=$2; shift 2 ;;
      --timeout) [ $# -ge 2 ] || usage; tmo=$2; shift 2 ;;
      --display) [ $# -ge 2 ] || usage; display=$2; shift 2 ;;
      --key) [ $# -ge 2 ] || usage; keys+=("$2"); presses+=("key:$2"); shift 2 ;;
      --type) [ $# -ge 2 ] || usage; types+=("$2"); presses+=("type:$2"); shift 2 ;;
      --click) [ $# -ge 2 ] || usage; clicks+=("$2"); shift 2 ;;
      *) usage ;;
    esac
  done
  case "$run" in '' | */* | .* | *[!A-Za-z0-9._-]*) die "bad run name: '$run'" ;; esac
  case "$tmo" in '' | *[!0123456789]*) die "--timeout takes whole seconds" ;; esac
  case "$display" in '' | *[!0123456789]*) die "--display takes a display number" ;; esac
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
  host_guards
  need_display "$display"
  prefix_ready || die "no prefix; run: $0 setup"
  [ -d "$PINNED_COPY/bin64" ] || die "no pinned copy at $PINNED_COPY; run: scripts/spike/run-client.sh setup"
  [ -x "$STUBBIN/fenji-server" ] || die "build the server first: cargo build --release -p fenji-server"
  [ -x "$STUBBIN/fenji-launch" ] || die "build the launcher first: cargo build --release -p fenji-launcher"
  local OUT="$WORK/out/$run" IO="$WORK/io/$run" RUN="$WORK/runs/$run" PFX="$WORK/compat"
  if [ -e "$OUT" ] || [ -e "$IO" ] || [ -e "$RUN" ]; then die "run $run exists already; pick a new name"; fi
  mkdir -p "$OUT" "$IO/tmp" "$IO/server" "$RUN"
  cp -a --reflink=always -- "$PINNED_COPY" "$RUN/client" ||
    die "the client copy needs a filesystem with reflinks (btrfs, XFS); nothing was started"
  local server_copy="$IO/server/fenji.toml" channels
  cp -- "$server_cfg" "$server_copy"
  channels=$(watch_channels "$CHANNELS_DEFAULT")

  # --- before: inputs and the AGT baseline
  {
    echo "run $run (bare metal), $(date -Is)"
    echo "guards: Linux; Steam not running; no Wine running; reflink copy of $PINNED_COPY; prefix $PFX"
    echo "namespace: $(namespace_cmd | tr '\n' ' ')"
    echo "launch: fenji-launch $(launch_args "$api_port" "$user" "<password file>" "$RUN/client" | tr '\n' ' ')"
    echo "server: $server_copy (login 127.0.0.1:$login_port, API 127.0.0.1:$api_port), store in $server_data"
    echo "umu-run: $UMU ($(sha256sum "$UMU" | cut -c1-16)…); GE-Proton: $PROTON"
    echo "wine channels (WINEDEBUG, with PROTON_LOG=1): $channels"
    echo "driver watchdog: watchdog.py sha256 $(sha256sum "$WATCHDOG" | cut -c1-16)…, --no-tmpfs-scan"
    echo "display :$display; timeout $tmo s; keys ${presses[*]-}; clicks ${clicks[*]-}"
  } > "$OUT/inputs.txt"
  find "$PFX" -iname '*.sys' -printf '%P\t%y\t%l\t%s\t%T@\n' | sort > "$OUT/sys-before.tsv"
  python3 -I "$WATCHDOG" collect --src "$PFX" --dest "$OUT/reg-before" --name system.reg --name user.reg
  grep -o '^\[System\\\\ControlSet00[0-9]\\\\Services\\\\[^]\\]*' "$OUT/reg-before/system.reg" | sort -u > "$OUT/services-before.txt" || true
  python3 -I "$WATCHDOG" baseline --root "$WORK" --out "$OUT"
  touch "$OUT/.start"

  local shots clicker typist
  start_inputs "$OUT" "$display" "$tmo"
  touch "$OUT/.running"

  # --- the run: fenji-server and fenji-launch in one namespace, started once the watchdog has armed
  local ns la
  mapfile -t ns < <(namespace_cmd)
  mapfile -t la < <(launch_args "$api_port" "$user" "$pwfile" "$RUN/client")
  set +e
  # shellcheck disable=SC2016 # the bash -c script expands its own positional parameters
  "${ns[@]}" env -u WAYLAND_DISPLAY DISPLAY=":$display" TMPDIR="$IO/tmp" PROTON_LOG=1 \
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
    "${la[@]}" > "$OUT/namespace.out" 2>&1 &
  local spid=$! wc
  mapfile -t wc < <(watch_cmd "$OUT" "$IO" "$spid")
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
  kill "$shots" "$clicker" "$typist" 2>/dev/null || true
  wait "$shots" "$clicker" "$typist" 2>/dev/null || true
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

# run-client.sh's verdict, with fenji-launch's failure before the start named for this runner.
bare_verdict() {
  local wdrc=$1 wrc=$2 ran=$3 out=$4
  if [ "$wdrc" = 0 ] && [ "$ran" != 1 ] && [ "$wrc" = 95 ]; then
    echo "fenji-launch didn't start the client (see $out/io/launch.err)" >&2
    return 7
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
