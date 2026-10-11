#!/usr/bin/env bash
# Runs the unmodified pinned SoLO client once inside scripts/spike/isolated-run.sh, with
# fenji-stub listening and the AGT driver watchdog (watchdog.py) armed (design spec §4 B; M1 spec
# §4, W4).
#
#   scripts/spike/run-client.sh setup [--display N]
#       Makes the git-ignored run workspace re/runs/: client/, a reflink copy of
#       $FENJI_CLIENT_PATH (a full copy is refused); a Wine prefix in compat/, created with Wine's
#       own cmd.exe; and empty home/, steamclient/ and hosts-empty.
#   scripts/spike/run-client.sh run NAME [--hosts FILE] [--ports "P1 P2"] [--timeout S]
#       [--display N] [--channels WINEDEBUG] [--env VAR=VALUE]... [--client-file NAME=SRC]...
#       -- [client args...]
#       One run: evidence in re/runs/out/NAME/, stub logs in re/stub-logs/NAME/; NAME must be new.
#       --client-file puts SRC into the run's client copy as NAME at the install root, an input
#       the client reads, for this run only. It never replaces a file the copy already has.
#
# Every run uses isolated-run.sh --no-root --x11 N --cpu-topology (CLAUDE.md "Client runs"), the
# pinned GE-Proton's own `proton run` (umu's runtime container fails in the sandbox), lavapipe, and
# Wine channels that record lookups, sockets, modules and driver activity. The dev-only
# --gf-sdk=mocked (gate #11) and --fs=data:_index:_index are the caller's to pass.
#
# Exit: 0 the run finished; 3 GATE EVENT (the watchdog stopped the run: stop all runs, report);
# 4 the watchdog failed; 5 the watchdog never armed (the client was not started); 1 or 2 setup or
# usage errors.
set -euo pipefail
HERE=$(cd -- "$(dirname -- "$0")" && pwd -P)
REPO=$(cd -- "$HERE/../.." && pwd -P)
# shellcheck source=../versions.env
. "$REPO/scripts/versions.env"
WORK="$REPO/re/runs"
WRAP="$HERE/isolated-run.sh"
WATCHDOG="$HERE/watchdog.py"
PROTON="${FENJI_GE_PROTON:-$HOME/.local/share/fenji/tools/$GE_PROTON_VERSION}"
STUBBIN="$REPO/target/release"
LAVAPIPE=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json
CHANNELS_DEFAULT=+timestamp,+pid,+tid,+threadname,+seh,+debugstr,+loaddll,+winsock,+module,+process,+service,+profile,warn+file

usage() { sed -n '2,24p' "$0" >&2; exit 2; }
die() { echo "run-client: $*" >&2; exit 1; }
need_display() { [ -S "/tmp/.X11-unix/X$1" ] || die "no nested display :$1 (see isolated-run.sh, \"Display\")"; }

cmd_setup() {
  local display=99
  while [ $# -gt 0 ]; do
    case "$1" in
      --display) [ $# -ge 2 ] || usage; display=$2; shift 2 ;;
      *) usage ;;
    esac
  done
  [ -n "${FENJI_CLIENT_PATH:-}" ] || die "set FENJI_CLIENT_PATH to the pinned client copy (docs/re/pinned-build.md)"
  [ -f "$FENJI_CLIENT_PATH/bin64/gujianol.exe" ] || die "no bin64/gujianol.exe under $FENJI_CLIENT_PATH"
  [ ! -e "$WORK/client" ] || die "$WORK/client exists already; remove re/runs/ to start over"
  [ -x "$PROTON/proton" ] || die "GE-Proton not found at $PROTON (run scripts/bootstrap.sh)"
  need_display "$display"
  mkdir -p "$WORK/client" "$WORK/compat" "$WORK/home" "$WORK/steamclient" "$WORK/out" "$REPO/re/stub-logs"
  : > "$WORK/hosts-empty"
  if ! cp -a --reflink=always "$FENJI_CLIENT_PATH/." "$WORK/client/"; then
    rm -rf "$WORK/client"
    die "the reflink copy failed (it needs a copy-on-write filesystem such as btrfs); refusing a full copy"
  fi
  # The prefix is created now, by Wine's own cmd.exe. A prefix appearing during a client run would
  # look like new driver files to the watchdog.
  "$WRAP" --no-root --x11 "$display" --cpu-topology --bind "$WORK" --ro-bind "$PROTON" -- \
    env HOME="$WORK/home" STEAM_COMPAT_DATA_PATH="$WORK/compat" \
    STEAM_COMPAT_CLIENT_INSTALL_PATH="$WORK/steamclient" SteamGameId=1418100 SteamAppId=1418100 \
    PROTON_USE_XALIA=0 VK_ICD_FILENAMES="$LAVAPIPE" \
    timeout -k 10 300 python3 "$PROTON/proton" run cmd.exe /c exit > "$WORK/prefix-init.log" 2>&1 || true
  [ -f "$WORK/compat/pfx/system.reg" ] || die "the Wine prefix wasn't created; see $WORK/prefix-init.log"
  echo "workspace ready: $WORK"
}

cmd_run() {
  [ $# -ge 1 ] || usage
  local run=$1
  shift
  local hosts="$WORK/hosts-empty" ports=7804 tmo=300 display=99 channels=$CHANNELS_DEFAULT
  local envs=() client_files=()
  while [ $# -gt 0 ]; do
    case "$1" in
      --hosts) [ $# -ge 2 ] || usage; hosts=$(realpath -- "$2"); shift 2 ;;
      --ports) [ $# -ge 2 ] || usage; ports=$2; shift 2 ;;
      --timeout) [ $# -ge 2 ] || usage; tmo=$2; shift 2 ;;
      --display) [ $# -ge 2 ] || usage; display=$2; shift 2 ;;
      --channels) [ $# -ge 2 ] || usage; channels=$2; shift 2 ;;
      --env) [ $# -ge 2 ] || usage; envs+=("$2"); shift 2 ;;
      --client-file) [ $# -ge 2 ] || usage; client_files+=("$2"); shift 2 ;;
      --) shift; break ;;
      *) usage ;;
    esac
  done
  case "$run" in '' | */* | .*) die "bad run name: '$run'" ;; esac
  case "$tmo" in '' | *[!0123456789]*) die "--timeout takes whole seconds" ;; esac
  case "$display" in '' | *[!0123456789]*) die "--display takes a display number" ;; esac
  local OUT="$WORK/out/$run" STUB="$REPO/re/stub-logs/$run" PFX="$WORK/compat/pfx"
  if [ ! -d "$WORK/client/bin64" ] || [ ! -f "$PFX/system.reg" ]; then die "no workspace; run: $0 setup"; fi
  if [ -e "$OUT" ] || [ -e "$STUB" ]; then die "run $run exists already; pick a new name"; fi
  need_display "$display"
  [ -f "$hosts" ] || die "hosts file missing: $hosts"
  [ -x "$PROTON/proton" ] || die "GE-Proton not found at $PROTON (run scripts/bootstrap.sh)"
  [ -x "$STUBBIN/fenji-stub" ] || die "build the stub first: cargo build --release -p fenji-stub"
  if pgrep -f 'winedevice\.exe|wineserver' >/dev/null; then
    die "Wine processes are already running on the host; stop them first"
  fi
  local spec name f
  for spec in "${client_files[@]}"; do
    name=${spec%%=*}
    case "$name" in '' | */* | . | ..) die "--client-file NAME must be a plain file name, not '$name'" ;; esac
    [ -f "${spec#*=}" ] || die "--client-file source missing: ${spec#*=}"
    [ ! -e "$WORK/client/$name" ] || die "--client-file $name would replace a file the client copy has"
  done
  for f in gujianol.log Log_gujianol.htm; do
    [ ! -e "$WORK/client/bin64/$f" ] || die "leftover $f in the client copy's bin64; move it first"
  done
  mkdir -p "$OUT" "$STUB"

  # --- before: inputs, client files, AGT baseline
  {
    echo "run: $run"
    echo "date: $(date -Is)"
    echo "client args: $*"
    echo "extra env: ${envs[*]-}"
    echo "client files: ${client_files[*]-}"
    echo "stub ports: $ports"
    echo "timeout: $tmo"
    echo "display: :$display"
    echo "wine channels (WINEDEBUG, with PROTON_LOG=1): $channels"
    echo "GE-Proton: $PROTON"
    echo "sandbox: isolated-run.sh --no-root --x11 $display --cpu-topology (sha256 $(sha256sum "$WRAP" | cut -c1-16)…)"
    echo "driver watchdog: watchdog.py sha256 $(sha256sum "$WATCHDOG" | cut -c1-16)…"
    echo "hosts file: $hosts"
    echo "--- hosts content"
    cat "$hosts"
  } > "$OUT/inputs.txt"
  for spec in "${client_files[@]}"; do
    cp -- "${spec#*=}" "$WORK/client/${spec%%=*}"
  done
  find "$PFX" -iname '*.sys' -printf '%P\t%y\t%l\t%s\t%T@\n' | sort > "$OUT/sys-before.tsv"
  grep -o '^\[System\\\\ControlSet00[0-9]\\\\Services\\\\[^]\\]*' "$PFX/system.reg" | sort -u > "$OUT/services-before.txt" || true
  mkdir -p "$OUT/reg-before" && cp -p "$PFX/system.reg" "$PFX/user.reg" "$OUT/reg-before/"
  python3 -I "$WATCHDOG" baseline --root "$WORK" --out "$OUT"
  touch "$OUT/.start"

  # --- host-side screenshots while the run is active
  (
    start=$(date +%s)
    for t in 5 10 30 60 90 120 150 180 210 240 270 295; do
      [ "$t" -le "$tmo" ] || break
      now=$(date +%s)
      sl=$((start + t - now))
      [ "$sl" -le 0 ] || sleep "$sl"
      [ -e "$OUT/.running" ] || break
      import -display ":$display" -window root "$OUT/screen-${t}s.png" 2>>"$OUT/screenshot.err" || true
    done
  ) &
  local shots=$!
  touch "$OUT/.running"

  # --- the sandboxed run: stub + client in one invocation, started once the watchdog has armed
  set +e
  # shellcheck disable=SC2016 # the bash -c script expands its own positional parameters
  "$WRAP" --no-root --x11 "$display" --cpu-topology --hosts "$hosts" --bind "$WORK" \
    --bind "$REPO/re/stub-logs" --ro-bind "$STUBBIN" --ro-bind "$PROTON" -- \
    env HOME="$WORK/home" STEAM_COMPAT_DATA_PATH="$WORK/compat" \
    STEAM_COMPAT_CLIENT_INSTALL_PATH="$WORK/steamclient" SteamGameId=1418100 SteamAppId=1418100 \
    PROTON_LOG_DIR="$OUT" PROTON_USE_XALIA=0 PROTON_LOG=1 WINEDEBUG="$channels" DXVK_LOG_PATH="$OUT" \
    VK_ICD_FILENAMES="$LAVAPIPE" "${envs[@]}" \
    bash -c '
      W=$1 OUT=$2 STUB=$3 STUBBIN=$4 PORTS=$5 TMO=$6 PROTON=$7
      shift 7
      for _ in $(seq 200); do [ -e "$OUT/.go" ] && break; sleep 0.05; done
      [ -e "$OUT/.go" ] || { echo "watchdog never armed; client not started" >&2; exit 98; }
      la=()
      for p in $PORTS; do la+=(--listen "127.0.0.1:$p"); done
      "$STUBBIN/fenji-stub" "${la[@]}" --out "$STUB" > "$OUT/stub.out" 2>&1 &
      stub=$!
      sleep 1
      cd "$W/client/bin64" || exit 99
      timeout -k 10 "$TMO" python3 "$PROTON/proton" run "$W/client/bin64/gujianol.exe" "$@" > "$OUT/proton.out" 2>&1
      rc=$?
      echo "$rc" > "$OUT/exit-code"
      WINEPREFIX="$W/compat/pfx" "$PROTON/files/bin/wineserver" -k 2>/dev/null
      sleep 2
      kill "$stub" 2>/dev/null
      wait "$stub" 2>/dev/null
      exit "$rc"
    ' _ "$WORK" "$OUT" "$STUB" "$STUBBIN" "$ports" "$tmo" "$PROTON" "$@" > "$OUT/sandbox.out" 2>&1 &
  local spid=$!
  python3 -I "$WATCHDOG" watch --root "$WORK" --out "$OUT" --sandbox-pid "$spid" \
    --log "$OUT/steam-1418100.log" --bind "$REPO/re/stub-logs" --bind "$STUBBIN" \
    2>> "$OUT/watchdog.err" &
  local wdpid=$!
  # Whichever ends first. If it is the watchdog (gate event, failure, or killed), the sandbox must
  # not outlive it.
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
  rm -f "$OUT/.running" "$OUT/.go"
  wait "$shots" 2>/dev/null || true
  echo "$wrc" > "$OUT/sandbox-exit-code"
  echo "$wdrc" > "$OUT/watchdog-exit-code"
  date -Is > "$OUT/ended"

  # --- after: evidence
  [ ! -f "$OUT/steam-1418100.log" ] || mv "$OUT/steam-1418100.log" "$OUT/wine.log"
  for f in gujianol.log Log_gujianol.htm; do
    [ ! -e "$WORK/client/bin64/$f" ] || mv "$WORK/client/bin64/$f" "$OUT/$f"
  done
  # Entries the client creates at its install root (settings/, ExportFace/, ExportPrefab/) and this
  # run's --client-file inputs move into the evidence, so every run starts from the same copy.
  local e
  for e in "$WORK/client"/* "$WORK/client"/.[!.]*; do
    [ -e "$e" ] || continue
    case "${e##*/}" in
      bin64 | data | tools) ;;
      *) mkdir -p "$OUT/client-files" && mv "$e" "$OUT/client-files/" ;;
    esac
  done
  find "$PFX" -iname '*.sys' -printf '%P\t%y\t%l\t%s\t%T@\n' | sort > "$OUT/sys-after.tsv"
  diff "$OUT/sys-before.tsv" "$OUT/sys-after.tsv" > "$OUT/sys-diff.txt" || true
  grep -o '^\[System\\\\ControlSet00[0-9]\\\\Services\\\\[^]\\]*' "$PFX/system.reg" | sort -u > "$OUT/services-after.txt" || true
  diff "$OUT/services-before.txt" "$OUT/services-after.txt" > "$OUT/services-diff.txt" || true
  mkdir -p "$OUT/reg-after" && cp -p "$PFX/system.reg" "$PFX/user.reg" "$OUT/reg-after/"
  local r
  for r in system user; do
    diff "$OUT/reg-before/$r.reg" "$OUT/reg-after/$r.reg" > "$OUT/reg-$r.diff" || true
  done
  find "$WORK/client" "$PFX/drive_c" "$WORK/home" -newer "$OUT/.start" -type f -not -path '*/mesa_shader_cache/*' \
    -printf '%TT\t%s\t%p\n' 2>/dev/null | sort > "$OUT/new-files.txt" || true
  echo "run $run done: proton exit $(cat "$OUT/exit-code" 2>/dev/null || echo '?'), sandbox exit $wrc, watchdog exit $wdrc"
  case "$wdrc" in
    0) ;;
    3) echo "GATE EVENT: the driver watchdog stopped the run; see $OUT/watchdog-trigger.txt. Stop all runs." >&2; exit 3 ;;
    *) echo "WATCHDOG FAILURE (exit $wdrc); see $OUT/watchdog.log and watchdog.err" >&2; exit 4 ;;
  esac
  [ "$wrc" != 98 ] || { echo "the watchdog never armed; the client was not started" >&2; exit 5; }
}

case "${1:-}" in
  setup) shift; cmd_setup "$@" ;;
  run) shift; cmd_run "$@" ;;
  *) usage ;;
esac
