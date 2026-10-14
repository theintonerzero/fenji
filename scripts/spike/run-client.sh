#!/usr/bin/env bash
# Runs the unmodified pinned SoLO client once inside scripts/spike/isolated-run.sh, with
# fenji-stub listening and the AGT driver watchdog (watchdog.py) armed (design spec §4 B; M1 spec
# §4, W4).
#
#   scripts/spike/run-client.sh setup [--display N] [--clients 2]
#       Makes the git-ignored run workspace re/runs/: client/, a reflink copy of
#       $FENJI_CLIENT_PATH (a full copy is refused); a Wine prefix in compat/, created with Wine's
#       own cmd.exe; and empty home/, steamclient/ and hosts-empty. With --clients 2, also client2/,
#       a second reflink copy for a two-client run's second client; on an existing workspace,
#       client2/ only.
#   scripts/spike/run-client.sh run NAME [--hosts FILE] [--ports "P1 P2"] [--timeout S]
#       [--display N] [--channels WINEDEBUG] [--env VAR=VALUE]... [--client-file NAME=SRC]...
#       [--click T:X,Y]... [--key T:KEY]... [--type T:TEXT]... [--stub-arg ARG]...
#       [--server CONFIG [--server-data DIR] [--launch "ARGS" [--launch-password FILE]]]
#       [--clients 2 --display2 M [--second-after S] [--timeout2 S]
#        [--launch2 "ARGS" --launch-password2 FILE]] -- [client args...]
#       One run: evidence in re/runs/out/NAME/, stub logs in re/stub-logs/NAME/; NAME must be new.
#       The sandbox writes only to the client copy, the prefix, its home, re/runs/io/NAME and, with
#       --server-data, DIR; the evidence directories are never bound into it, and the host takes
#       files out of what the sandbox can write only through `watchdog.py collect`, which never
#       follows a link.
#       --client-file puts SRC into the run's client copy as NAME at the install root, an input
#       the client reads, for this run only. It never replaces a file the copy already has.
#       --click plays a player's left click at X,Y on the nested display, T seconds into the run
#       (xclick.py, through XTEST): input to the client's own UI, never a change to the client.
#       --stub-arg passes one word to fenji-stub, e.g. --stub-arg --speak (M2).
#       --key and --type play a player's key press (an X keysym name such as Return, space, Tab,
#       BackSpace or a letter) or typed printable ASCII, T seconds into the run (xkey.py, XTEST).
#       --server runs target/release/fenji-server with a copy of CONFIG (a fenji.toml) in place of
#       the stub, RUST_LOG=fenji_server=trace, its output in the evidence as server.log; the run
#       waits for its login_bind and api_bind ports, which must be on 127.0.0.1. Its [store] path is
#       relative and stays in the copy's folder (a fresh database each run), or, with --server-data
#       DIR, absolute and inside DIR: a directory under re/runs/, bound read-write, which outlives
#       the run. The sandbox can write DIR, so open it on the host only before the first run, or as
#       a copy made with `watchdog.py collect`; a DIR that holds a link or a special file is refused.
#       --launch runs `target/release/fenji-launch play --print ARGS` once the server is ready, and
#       gives the client only the gj_* pairs it prints (nothing else may be printed); inputs.txt
#       records ARGS, and io/launch.txt the pairs with gj_ticket redacted. The account's password
#       comes from --launch-password FILE (required with --launch), given to the sandbox on its
#       standard input: it is never bound, copied, recorded, or named in an argument or the evidence.
#       --clients 2 runs a second client from client2/ in the same sandbox, Wine prefix and server
#       (M4 world plan, Amendment B): on its own nested display :M (--display2, required, another
#       display than --display's, started as :N is and checked as it is), S seconds after the
#       first (--second-after, default 30, at least 2: the watchdog tells the clients apart by the
#       order they start), with its own timeout (--timeout2, default --timeout) and Proton log
#       (io/NAME/2), and with --launch, its own account (--launch2 and --launch-password2, required
#       then; that password is the second line of the sandbox's standard input). --click, --key and
#       --type take 2@T:... for client 2's display, T counted from the run's start. Client 2's
#       timeout ends its proton run; if its gujianol.exe is still there 10 s later, the sandbox
#       ends that process by its own pid (end_client): wineserver -k would end client 1 too. The
#       wineserver stops once both clients have ended. Client 2's evidence: OUT/client2/ (its
#       gujianol.log, Log_gujianol.htm and install-root files), OUT/wine-2.log,
#       OUT/screen-2-<T>s.png and io/exit-code-2.
#
# Every run uses isolated-run.sh --no-root --x11 N [--x11 M] --cpu-topology (CLAUDE.md "Client runs"), the
# pinned GE-Proton's own `proton run` (umu's runtime container fails in the sandbox), lavapipe, and
# Wine channels that record lookups, sockets, modules and driver activity. The dev-only
# --gf-sdk=mocked (gate #11) and --fs=data:_index:_index are the caller's to pass.
#
# The channels the watchdog reads (CHANNELS_WATCHDOG) follow any --channels, and --env can't set what
# the watchdog depends on (the log's place, the channels, the prefix, HOME).
#
# Exit: 0 the run finished; 3 GATE EVENT (the watchdog stopped the run: stop all runs, report);
# 4 the watchdog failed; 5 the watchdog never armed (the client was not started); 6 the sandbox
# ended before the client ran; 7 fenji-stub or fenji-server wasn't ready, or fenji-launch failed,
# when the client was due (the client was not started); 1 or 2 setup or usage errors. Evidence is
# collected first, best effort: a failure there is reported but never changes the exit status. A
# Fenji ticket found in the evidence afterwards is redacted in place and listed in redactions.txt.
set -euo pipefail
HERE=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
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
# What the watchdog's log rules read: line headers, process creation, module loads, resource lookups
# (the driver's resource type), services, debug strings, and errors (LoadDriver).
CHANNELS_WATCHDOG=+timestamp,+pid,+tid,+process,+loaddll,+resource,+service,+debugstr,err+all
# Environment the runner sets and the watchdog depends on; --env may not override it.
ENV_RESERVED=" WINEDEBUG WINEPREFIX WINESERVER PROTON_LOG PROTON_LOG_DIR STEAM_COMPAT_DATA_PATH STEAM_COMPAT_CLIENT_INSTALL_PATH HOME SteamGameId SteamAppId "
# Environment that could carry the launcher's password or trace the inner shell (and the ticket in
# it) into the evidence; --env may not set it.
ENV_SECRET=" FENJI_PASSWORD SHELLOPTS BASHOPTS BASH_ENV ENV PS4 "

usage() { awk 'NR > 1 && /^#/ { print; next } NR > 1 { exit }' "$0" >&2; exit 2; } # the header above
die() { echo "run-client: $*" >&2; exit 1; }
# A usage error with its reason.
misuse() { echo "run-client: $*" >&2; exit 2; }

# The pairs on stdin, one per line, with the ticket's whole value hidden.
redact_gj_ticket() { sed -E 's/^gj_ticket=.*/gj_ticket=<redacted>/'; }

# fenji-launch play --print's output on stdin, read straight from the pipe: prints its gj_* pairs.
# Every line must be one pair of printable ASCII: a NUL (turned into a control character here), a
# value split by a newline, or any other line fails it, as does output with no pair.
parse_launch_output() {
  local LC_ALL=C line pairs=()
  while IFS= read -r line; do
    [[ "$line" =~ ^gj_[a-z_]+=[\ -~]*$ ]] || return 1
    pairs+=("$line")
  done < <(tr '\000' '\001')
  [ "${#pairs[@]}" -gt 0 ] || return 1
  printf '%s\n' "${pairs[@]}"
}

# The launcher passwords' first lines for the sandbox's standard input, each ending its line:
# client 1's, then with --launch2 client 2's, or nothing. A pipe, not the files: once the inner shell
# has read them, nothing in the sandbox can read them again (bwrap's PID 1 keeps its standard input
# open).
password_line() {
  if [ -n "${launch_password-}" ]; then printf '%s\n' "$(head -n 1 -- "$launch_password")"; fi
  if [ -n "${launch_password2-}" ]; then printf '%s\n' "$(head -n 1 -- "$launch_password2")"; fi
}

# launch_pairs BIN PASSWORD WORDS...: `BIN play --print WORDS` with the password in its
# environment only, its output parsed straight from the pipe (parse_launch_output).
launch_pairs() {
  local bin=$1 pw=$2
  shift 2
  set -o pipefail
  FENJI_PASSWORD="$pw" "$bin" play --print "$@" | parse_launch_output
}

# server_config CONFIG: the login port, the API port and the store path of a fenji.toml, one per
# line, with the server's defaults. Fails when CONFIG isn't TOML.
server_config() {
  python3 -I - "$1" <<'EOF'
import sys
import tomllib

try:
    with open(sys.argv[1], "rb") as f:
        c = tomllib.load(f)
except (OSError, tomllib.TOMLDecodeError) as e:
    sys.exit(f"{sys.argv[1]}: {e}")
s = c.get("server", {})
for key, default in (("login_bind", "127.0.0.1:7800"), ("api_bind", "127.0.0.1:7801")):
    host, _, port = str(s.get(key, default)).rpartition(":")
    if host != "127.0.0.1" or not port.isdigit() or not 1 <= int(port) <= 65535:
        sys.exit(f"[server] {key} must be 127.0.0.1 and a port from 1 to 65535 (the sandbox is loopback only)")
    print(port)
path = str(c.get("store", {}).get("path", "fenji.db"))
if not path or any(ord(ch) < 0x20 or ord(ch) == 0x7F for ch in path):
    sys.exit("[store] path must be a path without control characters")
print(path)
EOF
}
# WINEDEBUG for a run: the caller's channels, then the watchdog's (later entries win).
watch_channels() { echo "$1,$CHANNELS_WATCHDOG"; }

need_display() { [ -S "/tmp/.X11-unix/X$1" ] || die "no nested display :$1 (see isolated-run.sh, \"Display\")"; }

# copy_client DEST: a reflink copy of the pinned client at DEST; a full copy is refused.
copy_client() {
  mkdir -p "$1"
  if ! cp -a --reflink=always "$FENJI_CLIENT_PATH/." "$1/"; then
    rm -rf "$1"
    die "the reflink copy failed (it needs a copy-on-write filesystem such as btrfs); refusing a full copy"
  fi
}

cmd_setup() {
  local display=99 clients=1
  while [ $# -gt 0 ]; do
    case "$1" in
      --display) [ $# -ge 2 ] || usage; display=$2; shift 2 ;;
      --clients) [ $# -ge 2 ] || usage; clients=$2; shift 2 ;;
      *) usage ;;
    esac
  done
  case "$clients" in 1 | 2) ;; *) misuse "--clients takes 1 or 2, not '$clients'" ;; esac
  [ -n "${FENJI_CLIENT_PATH:-}" ] || die "set FENJI_CLIENT_PATH to the pinned client copy (docs/re/pinned-build.md)"
  [ -f "$FENJI_CLIENT_PATH/bin64/gujianol.exe" ] || die "no bin64/gujianol.exe under $FENJI_CLIENT_PATH"
  if [ -e "$WORK/client" ] && [ "$clients" = 2 ]; then
    # An existing workspace gains client 2's copy only: the prefix and its baseline stay.
    [ ! -e "$WORK/client2" ] || die "$WORK/client2 exists already"
    copy_client "$WORK/client2"
    echo "client 2's copy ready: $WORK/client2"
    return 0
  fi
  [ ! -e "$WORK/client" ] || die "$WORK/client exists already; remove re/runs/ to start over"
  [ -x "$PROTON/proton" ] || die "GE-Proton not found at $PROTON (run scripts/bootstrap.sh)"
  need_display "$display"
  mkdir -p "$WORK/compat" "$WORK/home" "$WORK/steamclient" "$WORK/out" "$REPO/re/stub-logs"
  : > "$WORK/hosts-empty"
  copy_client "$WORK/client"
  if [ "$clients" = 2 ]; then copy_client "$WORK/client2"; fi
  # The prefix is created now, through Proton's own launcher (its steam.exe shim, which also
  # registers Proton's Discord bridge service), and wineserver is left to exit cleanly so the
  # registry reaches disk: everything Proton sets up belongs to the watchdog's baseline. A prefix, or
  # a service, appearing during a client run would look like driver activity to the watchdog.
  # shellcheck disable=SC2016 # the bash -c script expands its own positional parameters
  "$WRAP" --no-root --x11 "$display" --cpu-topology --bind "$WORK/compat" --bind "$WORK/home" \
    --bind "$WORK/steamclient" --ro-bind "$PROTON" -- \
    env HOME="$WORK/home" STEAM_COMPAT_DATA_PATH="$WORK/compat" \
    STEAM_COMPAT_CLIENT_INSTALL_PATH="$WORK/steamclient" SteamGameId=1418100 SteamAppId=1418100 \
    PROTON_USE_XALIA=0 VK_ICD_FILENAMES="$LAVAPIPE" \
    timeout -k 10 300 bash -c 'python3 "$1/proton" run cmd.exe /c exit; WINEPREFIX="$2/pfx" "$1/files/bin/wineserver" -w' \
    _ "$PROTON" "$WORK/compat" > "$WORK/prefix-init.log" 2>&1 || true
  [ -f "$WORK/compat/pfx/system.reg" ] || die "the Wine prefix wasn't created; see $WORK/prefix-init.log"
  echo "workspace ready: $WORK"
}

# What defines a run, written to OUT/inputs.txt before it starts. Uses cmd_run's run, ports, tmo,
# display, channels, hosts, envs, client_files, clicks and stub_args; "$@" is the client's arguments.
record_inputs() {
  {
  echo "run: $run"
  echo "date: $(date -Is)"
  echo "client args: $*"
  echo "extra env: ${envs[*]-}"
  echo "client files: ${client_files[*]-}"
  echo "clicks (T:X,Y): ${clicks[*]-}"
  echo "keys (T:KEY): ${keys[*]-}"
  echo "text (T:TEXT): ${types[*]-}"
  if [ -n "${server_cfg-}" ]; then
    echo "server config: ${server_copy-} (a copy of $server_cfg)"
    echo "server data: ${server_data:-none}"
    echo "launch args: ${launch-}"
    echo "launch password: ${launch_password:+given on the sandbox standard input, never recorded}"
  else
    echo "stub ports: $ports"
    echo "stub args: ${stub_args[*]-}"
  fi
  echo "timeout: $tmo"
  echo "display: :$display"
  echo "clients: ${clients:-1}"
  if [ "${clients:-1}" = 2 ]; then
    echo "client 2: display :$display2, started $second_after s after client 1, timeout $tmo2 s, from $WORK/client2"
    echo "client 2 clicks (2@T:X,Y): ${clicks2[*]-}"
    echo "client 2 keys (2@T:KEY): ${keys2[*]-}"
    echo "client 2 text (2@T:TEXT): ${types2[*]-}"
    if [ -n "${server_cfg-}" ]; then
      echo "client 2 launch args: ${launch2-}"
      echo "client 2 launch password: ${launch_password2:+the second line of the sandbox standard input, never recorded}"
    fi
  fi
  echo "wine channels (WINEDEBUG, with PROTON_LOG=1): $channels"
  echo "GE-Proton: $PROTON"
  echo "sandbox: isolated-run.sh --no-root --x11 $display${display2:+ --x11 $display2} --cpu-topology (sha256 $(sha256sum "$WRAP" | cut -c1-16)…)"
  echo "driver watchdog: watchdog.py sha256 $(sha256sum "$WATCHDOG" | cut -c1-16)…"
  echo "hosts file: $hosts"
  echo "--- hosts content"
  cat "$hosts"
  if [ -n "${server_cfg-}" ]; then
    echo "--- server config content"
    cat "${server_copy:-$server_cfg}"
  fi
  } > "$OUT/inputs.txt"
}

# --key and --type: whole seconds, then a keysym name or printable ASCII; client 1's no later than
# --timeout, client 2's (2@) no later than its own end, --second-after plus --timeout2. Uses cmd_run's
# keys, types, clicks, their client-2 twins and presses, whose entries are C:KIND:T:WHAT for client C.
check_presses() {
  local LC_ALL=C k end2=$((${second_after:-0} + ${tmo2:-${tmo:-0}}))
  for k in "${keys[@]}" "${keys2[@]}"; do
    [[ "$k" =~ ^[0-9]+:[A-Za-z][A-Za-z0-9_]*$ ]] || misuse "--key takes T:KEY (whole seconds, an X keysym name), not '$k'"
  done
  for k in "${types[@]}" "${types2[@]}"; do
    [[ "$k" =~ ^[0-9]+:[\ -~]+$ ]] || misuse "--type takes T:TEXT (whole seconds, printable ASCII), not '$k'"
  done
  for k in "${keys[@]}"; do
    [ "${k%%:*}" -le "$tmo" ] || misuse "--key $k comes after the --timeout of $tmo s"
  done
  for k in "${types[@]}"; do
    [ "${k%%:*}" -le "$tmo" ] || misuse "--type at ${k%%:*} s comes after the --timeout of $tmo s"
  done
  for k in "${clicks[@]}"; do
    [ "${k%%:*}" -le "$tmo" ] || misuse "--click $k comes after the --timeout of $tmo s"
  done
  for k in "${keys2[@]}"; do
    [ "${k%%:*}" -le "$end2" ] || misuse "--key 2@$k comes after client 2's --timeout2 (at $end2 s)"
  done
  for k in "${types2[@]}"; do
    [ "${k%%:*}" -le "$end2" ] || misuse "--type 2@ at ${k%%:*} s comes after client 2's --timeout2 (at $end2 s)"
  done
  for k in "${clicks2[@]}"; do
    [ "${k%%:*}" -le "$end2" ] || misuse "--click 2@$k comes after client 2's --timeout2 (at $end2 s)"
  done
  # Played in time order, whatever the command line's order; at one time, in the order given.
  if [ "${#presses[@]}" -gt 0 ]; then
    mapfile -t presses < <(printf '%s\n' "${presses[@]}" | sort -s -t: -k3,3n)
  fi
}

# Both clients' clicks as one schedule in time order (at one time, client 1's first, each in the
# order given): lines "T DISPLAY C X,Y". Uses cmd_run's clicks, clicks2, display and display2.
click_plan() {
  local c
  {
    for c in "${clicks[@]}"; do printf '%s %s 1 %s\n' "${c%%:*}" "$display" "${c#*:}"; done
    for c in "${clicks2[@]}"; do printf '%s %s 2 %s\n' "${c%%:*}" "${display2:-}" "${c#*:}"; done
  } | sort -s -n -k1,1
}

# end_client EXE: SIGKILL to every process whose program is EXE, as Wine shows it in argv[0] (Z: and
# backslashes) or as given, and to nothing else; prints each pid it ended. A two-client run's client
# 2: its timeout ends its `proton run`, not the client, and wineserver -k would end client 1 too.
# argv[0], not the process name: the client's main thread renames itself (run m4-s0: to
# "vx_initialize"), and Wine passes thread names on to Linux.
end_client() {
  local want=$1 d a0
  for d in /proc/[0-9]*; do
    a0=$(head -z -n 1 -- "$d/cmdline" 2> /dev/null | tr -d '\0') || continue
    a0=${a0//\\//}
    case "$a0" in [Zz]:/*) a0=${a0#??} ;; esac
    [ "$a0" = "$want" ] || continue
    if kill -KILL "${d#/proc/}" 2> /dev/null; then echo "${d#/proc/}"; fi
  done
}

# --server, --server-data, --launch and --launch-password. Uses and sets cmd_run's variables; sets
# login_port and api_port from the config, and launch_words.
check_server() {
  # Byte ranges, not the locale's collation, for the printable-ASCII checks below.
  local LC_ALL=C
  if [ -z "$server_cfg" ]; then
    [ -z "$server_data" ] || misuse "--server-data needs --server"
    [ -z "$launch" ] || misuse "--launch needs --server"
    [ -z "$launch_password" ] || misuse "--launch-password needs --launch"
    return 0
  fi
  [ "$ports_given" = 0 ] || misuse "--server and --ports don't mix: the server's ports come from its config"
  [ "${#stub_args[@]}" = 0 ] || misuse "--server and --stub-arg don't mix: no stub runs"
  if [ ! -f "$server_cfg" ] || [ ! -r "$server_cfg" ]; then misuse "--server: can't read '$server_cfg'"; fi
  server_cfg=$(realpath -- "$server_cfg")
  local cfg store
  cfg=$(server_config "$server_cfg" 2>&1) || misuse "--server: '$server_cfg' isn't usable: $cfg"
  { read -r login_port; read -r api_port; read -r store; } <<< "$cfg"
  if [ -n "$server_data" ]; then
    [ -d "$server_data" ] || misuse "--server-data: no directory '$server_data'"
    server_data=$(realpath -- "$server_data")
    case "$server_data/" in
      "$WORK"/client/* | "$WORK"/compat/* | "$WORK"/home/* | "$WORK"/steamclient/* | "$WORK"/out/* | "$WORK"/io/*)
        misuse "--server-data can't be one of the run workspace's own directories" ;;
      "$WORK"/?*/) ;;
      *) misuse "--server-data must be a directory under $WORK/" ;;
    esac
    local odd
    odd=$(find "$server_data" -mindepth 1 \( -type l -o ! \( -type f -o -type d \) \) -printf . | wc -c)
    [ "$odd" = 0 ] || misuse "--server-data: '$server_data' holds a link or a special file"
    case "$store" in
      /*) ;;
      *) misuse "--server-data: the config's [store] path must be absolute, inside $server_data" ;;
    esac
    case "$(realpath -m -- "$store")" in
      "$server_data"/?*) ;;
      *) misuse "--server-data: the config's [store] path must be inside $server_data, not '$store'" ;;
    esac
  else
    # Relative to the config's copy, which the server reads; it must stay in that folder.
    case "/$store/" in
      //* ) misuse "--server: an absolute [store] path needs --server-data (the sandbox can write only there)" ;;
      */../* | */./*) misuse "--server: a relative [store] path can't leave the config's folder: '$store'" ;;
    esac
  fi
  if [ -n "$launch" ]; then
    [ -n "$launch_password" ] || misuse "--launch needs --launch-password: fenji-launch signs in"
    [[ "$launch" =~ ^[\ -~]+$ ]] || misuse "--launch takes one line of printable ASCII"
    local w
    read -r -a launch_words <<< "$launch"
    for w in "${launch_words[@]}"; do
      case "$w" in *[!A-Za-z0-9=_.:/@-]*) misuse "--launch takes words of [A-Za-z0-9=_.:/@-], not '$w'" ;; esac
      case "$w" in --password*) misuse "--launch: the password comes only from --launch-password" ;; esac
    done
  else
    [ -z "$launch_password" ] || misuse "--launch-password needs --launch"
  fi
  if [ -n "$launch_password" ]; then
    if [ ! -f "$launch_password" ] || [ ! -r "$launch_password" ]; then
      misuse "--launch-password: can't read the file"
    fi
  fi
}

# A two-client run's options (Amendment B), after check_server. Uses and sets cmd_run's clients,
# display, display2, second_after, tmo, tmo2, launch, launch2, launch_password2 and the 2@ inputs.
check_two() {
  local LC_ALL=C w words=()
  case "$clients" in 1 | 2) ;; *) misuse "--clients takes 1 or 2, not '$clients'" ;; esac
  if [ "$clients" = 1 ]; then
    [ -z "$display2" ] || misuse "--display2 needs --clients 2"
    [ "$second_after_given" = 0 ] || misuse "--second-after needs --clients 2"
    [ "$tmo2_given" = 0 ] || misuse "--timeout2 needs --clients 2"
    [ -z "$launch2" ] || misuse "--launch2 needs --clients 2"
    [ -z "$launch_password2" ] || misuse "--launch-password2 needs --clients 2"
    if [ "$((${#keys2[@]} + ${#types2[@]} + ${#clicks2[@]}))" -gt 0 ]; then
      misuse "2@ inputs are client 2's: they need --clients 2"
    fi
    return 0
  fi
  [ -n "$display2" ] || misuse "--clients 2 needs --display2 M: client 2's own nested display"
  [ "$display2" != "$display" ] ||
    misuse "--display2 must be another display than --display (:$display): two client windows on one display stack"
  if [ -n "$launch" ]; then
    [ -n "$launch2" ] || misuse "--launch with --clients 2 needs --launch2: client 2 signs in with its own account"
    [ -n "$launch_password2" ] || misuse "--launch2 needs --launch-password2: fenji-launch signs in"
    [[ "$launch2" =~ ^[\ -~]+$ ]] || misuse "--launch2 takes one line of printable ASCII"
    read -r -a words <<< "$launch2"
    for w in "${words[@]}"; do
      case "$w" in *[!A-Za-z0-9=_.:/@-]*) misuse "--launch2 takes words of [A-Za-z0-9=_.:/@-], not '$w'" ;; esac
      case "$w" in --password*) misuse "--launch2: the password comes only from --launch-password2" ;; esac
    done
    if [ ! -f "$launch_password2" ] || [ ! -r "$launch_password2" ]; then
      misuse "--launch-password2: can't read the file"
    fi
  else
    [ -z "$launch2" ] || misuse "--launch2 needs --launch"
    [ -z "$launch_password2" ] || misuse "--launch-password2 needs --launch2"
  fi
}

cmd_run() {
  [ $# -ge 1 ] || usage
  local run=$1
  shift
  local hosts="$WORK/hosts-empty" ports=7804 tmo=300 display=99 channels=$CHANNELS_DEFAULT
  local envs=() client_files=() clicks=() stub_args=() keys=() types=() presses=()
  local server_cfg="" server_data="" launch="" launch_password="" ports_given=0 launch_words=()
  local clients=1 display2="" second_after=30 second_after_given=0 tmo2="" tmo2_given=0
  local launch2="" launch_password2="" clicks2=() keys2=() types2=()
  while [ $# -gt 0 ]; do
    case "$1" in
      --hosts) [ $# -ge 2 ] || usage; hosts=$(realpath -- "$2"); shift 2 ;;
      --ports) [ $# -ge 2 ] || usage; ports=$2; ports_given=1; shift 2 ;;
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
      --server) [ $# -ge 2 ] || usage; server_cfg=$2; shift 2 ;;
      --server-data) [ $# -ge 2 ] || usage; server_data=$2; shift 2 ;;
      --launch) [ $# -ge 2 ] || usage; launch=$2; shift 2 ;;
      --launch-password) [ $# -ge 2 ] || usage; launch_password=$2; shift 2 ;;
      --timeout) [ $# -ge 2 ] || usage; tmo=$2; shift 2 ;;
      --display) [ $# -ge 2 ] || usage; display=$2; shift 2 ;;
      --channels) [ $# -ge 2 ] || usage; channels=$2; shift 2 ;;
      --env) [ $# -ge 2 ] || usage; envs+=("$2"); shift 2 ;;
      --client-file) [ $# -ge 2 ] || usage; client_files+=("$2"); shift 2 ;;
      --click)
        [ $# -ge 2 ] || usage
        case "$2" in 2@*) clicks2+=("${2#2@}") ;; *) clicks+=("$2") ;; esac
        shift 2 ;;
      --stub-arg) [ $# -ge 2 ] || usage; stub_args+=("$2"); shift 2 ;;
      --clients) [ $# -ge 2 ] || usage; clients=$2; shift 2 ;;
      --display2) [ $# -ge 2 ] || usage; display2=$2; shift 2 ;;
      --second-after) [ $# -ge 2 ] || usage; second_after=$2; second_after_given=1; shift 2 ;;
      --timeout2) [ $# -ge 2 ] || usage; tmo2=$2; tmo2_given=1; shift 2 ;;
      --launch2) [ $# -ge 2 ] || usage; launch2=$2; shift 2 ;;
      --launch-password2) [ $# -ge 2 ] || usage; launch_password2=$2; shift 2 ;;
      --) shift; break ;;
      *) usage ;;
    esac
  done
  case "$run" in '' | */* | .*) die "bad run name: '$run'" ;; esac
  case "$tmo" in '' | *[!0123456789]*) die "--timeout takes whole seconds" ;; esac
  case "$display" in '' | *[!0123456789]*) die "--display takes a display number" ;; esac
  case "$ports" in '' | *[!0123456789\ ]*) die "--ports takes port numbers separated by spaces, not '$ports'" ;; esac
  case "$clients" in 1 | 2) ;; *) misuse "--clients takes 1 or 2, not '$clients'" ;; esac
  if [ -n "$display2" ]; then
    case "$display2" in *[!0123456789]*) misuse "--display2 takes a display number, not '$display2'" ;; esac
  fi
  case "$second_after" in '' | *[!0123456789]*) misuse "--second-after takes whole seconds, not '$second_after'" ;; esac
  if [ "$clients" = 2 ] && [ "$second_after" -lt 2 ]; then
    misuse "--second-after takes at least 2 s: the watchdog tells the clients apart by the order they start"
  fi
  if [ "$tmo2_given" = 1 ]; then
    case "$tmo2" in '' | *[!0123456789]*) misuse "--timeout2 takes whole seconds, not '$tmo2'" ;; esac
  else
    tmo2=$tmo
  fi
  local ev
  for ev in "${envs[@]}"; do
    [[ "$ev" =~ ^[A-Za-z_][A-Za-z0-9_]*= ]] || die "--env takes VAR=VALUE with a plain name, not '$ev'"
    case "$ENV_RESERVED" in *" ${ev%%=*} "*) die "--env ${ev%%=*}: the runner sets it, and the watchdog depends on it" ;; esac
    case "$ENV_SECRET" in *" ${ev%%=*} "*) misuse "--env ${ev%%=*}: it could put the password or the ticket in the evidence" ;; esac
  done
  channels=$(watch_channels "$channels")
  local c
  for c in "${clicks[@]}" "${clicks2[@]}"; do
    [[ "$c" =~ ^[0-9]+:[0-9]+,[0-9]+$ ]] || die "--click takes T:X,Y in whole numbers, not '$c'"
  done
  local sa
  for sa in "${stub_args[@]}"; do
    case "$sa" in '' | *[!A-Za-z0-9=_.:,-]*) die "--stub-arg takes one word of [A-Za-z0-9=_.:,-], not '$sa'" ;; esac
  done
  check_presses
  local login_port="" api_port=""
  check_server
  check_two
  local OUT="$WORK/out/$run" IO="$WORK/io/$run" STUB="$REPO/re/stub-logs/$run" PFX="$WORK/compat/pfx"
  if [ ! -d "$WORK/client/bin64" ] || [ ! -f "$PFX/system.reg" ]; then die "no workspace; run: $0 setup"; fi
  if [ "$clients" = 2 ] && [ ! -d "$WORK/client2/bin64" ]; then
    die "no copy for client 2; run: $0 setup --clients 2"
  fi
  if [ -e "$OUT" ] || [ -e "$IO" ] || [ -e "$STUB" ]; then die "run $run exists already; pick a new name"; fi
  # A run whose evidence couldn't be collected leaves its entries in the copies; never start from them.
  local copies=(client) copy left
  if [ "$clients" = 2 ]; then copies+=(client2); fi
  for copy in "${copies[@]}"; do
    for left in "$WORK/$copy"/* "$WORK/$copy"/.[!.]*; do
      if [ -e "$left" ] || [ -L "$left" ]; then
        case "${left##*/}" in
          bin64 | data | tools) ;;
          *) die "leftover ${left##*/} at the install root of $copy/, which should hold only bin64, data and tools; move it first" ;;
        esac
      fi
    done
  done
  need_display "$display"
  if [ "$clients" = 2 ]; then need_display "$display2"; fi
  [ -f "$hosts" ] || die "hosts file missing: $hosts"
  [ -x "$PROTON/proton" ] || die "GE-Proton not found at $PROTON (run scripts/bootstrap.sh)"
  if [ -n "$server_cfg" ]; then
    [ -x "$STUBBIN/fenji-server" ] || die "build the server first: cargo build --release -p fenji-server"
    if [ -n "$launch" ]; then
      [ -x "$STUBBIN/fenji-launch" ] || die "build the launcher first: cargo build --release -p fenji-launcher"
    fi
  else
    [ -x "$STUBBIN/fenji-stub" ] || die "build the stub first: cargo build --release -p fenji-stub"
  fi
  # Exact process names: matching whole command lines would also match any shell or editor that
  # merely mentions them.
  if pgrep -x wineserver >/dev/null || pgrep -x winedevice.exe >/dev/null; then
    die "Wine processes are already running on the host; stop them first"
  fi
  local spec name f
  for spec in "${client_files[@]}"; do
    name=${spec%%=*}
    case "$name" in '' | */* | . | ..) die "--client-file NAME must be a plain file name, not '$name'" ;; esac
    [ -f "${spec#*=}" ] || die "--client-file source missing: ${spec#*=}"
    if [ -e "$WORK/client/$name" ] || [ -L "$WORK/client/$name" ]; then
      die "--client-file $name would replace a file (or a link) the client copy has"
    fi
  done
  for copy in "${copies[@]}"; do
    for f in gujianol.log Log_gujianol.htm; do
      if [ -e "$WORK/$copy/bin64/$f" ] || [ -L "$WORK/$copy/bin64/$f" ]; then
        die "leftover $f in $copy/bin64; move it first"
      fi
    done
  done
  mkdir -p "$OUT" "$IO/stub" "$STUB"
  if [ "$clients" = 2 ]; then mkdir -p "$IO/2"; fi
  local server_copy=""
  if [ -n "$server_cfg" ]; then
    mkdir -p "$IO/server"
    server_copy="$IO/server/fenji.toml"
    cp -- "$server_cfg" "$server_copy"
  fi

  # --- before: inputs, client files, AGT baseline
  record_inputs "$@"
  for spec in "${client_files[@]}"; do
    cp -- "${spec#*=}" "$WORK/client/${spec%%=*}"
  done
  find "$PFX" -iname '*.sys' -printf '%P\t%y\t%l\t%s\t%T@\n' | sort > "$OUT/sys-before.tsv"
  python3 -I "$WATCHDOG" collect --src "$PFX" --dest "$OUT/reg-before" --name system.reg --name user.reg
  grep -o '^\[System\\\\ControlSet00[0-9]\\\\Services\\\\[^]\\]*' "$OUT/reg-before/system.reg" | sort -u > "$OUT/services-before.txt" || true
  python3 -I "$WATCHDOG" baseline --root "$WORK" --out "$OUT"
  touch "$OUT/.start"

  local shots clicker typist
  start_inputs "$OUT" "$display" "$tmo"
  touch "$OUT/.running"

  # --- the sandboxed run: stub + client(s) in one invocation, started once the watchdog has armed.
  # One bind list for the sandbox and the watchdog: the watchdog leaves every bound directory out of
  # its pass over the sandbox's tmpfs mounts (the read-write ones lie under WORK, which it scans on
  # the host side; the read-only ones can't gain a file from inside).
  local binds=(--bind "$WORK/client" --bind "$WORK/compat" --bind "$WORK/home" --bind "$WORK/steamclient"
    --bind "$IO" --ro-bind "$STUBBIN" --ro-bind "$PROTON")
  if [ "$clients" = 2 ]; then binds+=(--bind "$WORK/client2"); fi
  if [ -n "$server_data" ]; then binds+=(--bind "$server_data"); fi
  local wbinds=() i
  for ((i = 1; i < ${#binds[@]}; i += 2)); do wbinds+=(--bind "${binds[i]}"); done
  # Each client's display and log, in the order the clients start: the watchdog ties the n-th to
  # start to the n-th log.
  local x11s=(--x11 "$display") wlogs=(--log "$IO/steam-1418100.log")
  if [ "$clients" = 2 ]; then x11s+=(--x11 "$display2") wlogs+=(--log "$IO/2/steam-1418100.log"); fi
  set +e
  # shellcheck disable=SC2016 # the bash -c script expands its own positional parameters
  "$WRAP" --no-root "${x11s[@]}" --cpu-topology --hosts "$hosts" "${binds[@]}" -- \
    env HOME="$WORK/home" STEAM_COMPAT_DATA_PATH="$WORK/compat" \
    STEAM_COMPAT_CLIENT_INSTALL_PATH="$WORK/steamclient" SteamGameId=1418100 SteamAppId=1418100 \
    PROTON_LOG_DIR="$IO" PROTON_USE_XALIA=0 PROTON_LOG=1 WINEDEBUG="$channels" DXVK_LOG_PATH="$IO" \
    VK_ICD_FILENAMES="$LAVAPIPE" "${envs[@]}" \
    bash -c "$(declare -f parse_launch_output redact_gj_ticket launch_pairs end_client)"'
      W=$1 IO=$2 STUB=$3 STUBBIN=$4 PORTS=$5 TMO=$6 PROTON=$7 SARGS=$8 SERVER=$9 LAUNCH=${10}
      LPORT=${11} APORT=${12} CLIENTS=${13} D2=${14} AFTER=${15} TMO2=${16} LAUNCH2=${17}
      shift 17
      # The launcher passwords, if any, are this shell'"'"'s standard input, client 2'"'"'s on the
      # second line; nothing else reads them.
      PW="" PW2=""
      [ -z "$LAUNCH" ] || IFS= read -r PW || true
      [ -z "$LAUNCH2" ] || IFS= read -r PW2 || true
      exec < /dev/null
      for _ in $(seq 200); do [ -e "$IO/.go" ] && break; sleep 0.05; done
      [ -e "$IO/.go" ] || { echo "watchdog never armed; client not started" >&2; exit 98; }
      if [ -n "$SERVER" ]; then
        RUST_LOG=fenji_server=trace "$STUBBIN/fenji-server" --config "$SERVER" run > "$IO/server.log" 2>&1 &
        stub=$!
        up() { (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null; }
        for _ in $(seq 100); do
          up "$LPORT" && up "$APORT" && break
          kill -0 "$stub" 2>/dev/null || break
          sleep 0.1
        done
        up "$LPORT" && up "$APORT" || { echo "fenji-server not ready; client not started" >&2; exit 96; }
      else
        la=()
        for p in $PORTS; do la+=(--listen "127.0.0.1:$p"); done
        # SARGS holds the validated --stub-arg words, split here on purpose.
        "$STUBBIN/fenji-stub" "${la[@]}" $SARGS --out "$STUB" > "$IO/stub.out" 2>&1 &
        stub=$!
        sleep 1
        kill -0 "$stub" 2>/dev/null || { echo "fenji-stub exited at start; client not started" >&2; exit 97; }
      fi
      pairs=""
      if [ -n "$LAUNCH" ]; then
        # The validated --launch words, split without globbing. The pairs hold the ticket: they
        # stay in this shell and reach a file only redacted.
        read -r -a LW <<< "$LAUNCH"
        pairs=$(launch_pairs "$STUBBIN/fenji-launch" "$PW" "${LW[@]}" 2> "$IO/launch.err") ||
          { echo "fenji-launch failed, or printed something other than gj_* pairs; client not started" >&2; exit 95; }
        redact_gj_ticket <<< "$pairs" > "$IO/launch.txt"
      fi
      unset PW
      # Each client in a subshell of its own, with its own gj_* pairs.
      (
        if [ -n "$pairs" ]; then while IFS= read -r kv; do export "${kv?}"; done <<< "$pairs"; fi
        unset pairs kv
        cd "$W/client/bin64" || exit 99
        timeout -k 10 "$TMO" python3 "$PROTON/proton" run "$W/client/bin64/gujianol.exe" "$@" > "$IO/proton.out" 2>&1
        rc=$?
        echo "$rc" > "$IO/exit-code"
        exit "$rc"
      ) &
      c1=$!
      unset pairs
      c2=""
      if [ "$CLIENTS" = 2 ]; then
        sleep "$AFTER"
        pairs2=""
        if [ -n "$LAUNCH2" ]; then
          read -r -a LW2 <<< "$LAUNCH2"
          pairs2=$(launch_pairs "$STUBBIN/fenji-launch" "$PW2" "${LW2[@]}" 2> "$IO/2/launch.err") ||
            { echo "fenji-launch failed for client 2, or printed something other than gj_* pairs; client 2 not started" >&2
              pairs2="" LAUNCH2=failed; }
          [ -z "$pairs2" ] || redact_gj_ticket <<< "$pairs2" > "$IO/2/launch.txt"
        fi
        unset PW2
        if [ "$LAUNCH2" != failed ]; then
          (
            if [ -n "$pairs2" ]; then while IFS= read -r kv; do export "${kv?}"; done <<< "$pairs2"; fi
            unset pairs2 kv
            export DISPLAY=":$D2" PROTON_LOG_DIR="$IO/2" DXVK_LOG_PATH="$IO/2"
            cd "$W/client2/bin64" || exit 99
            timeout -k 10 "$TMO2" python3 "$PROTON/proton" run "$W/client2/bin64/gujianol.exe" "$@" > "$IO/2/proton.out" 2>&1
            rc=$?
            echo "$rc" > "$IO/exit-code-2"
            # The timeout ends client 2'"'"'s proton run, not the client, and wineserver -k would end
            # client 1 too: a client 2 still there 10 s later is ended by its own pid.
            sleep 10
            { date +%s; end_client "$W/client2/bin64/gujianol.exe"; } > "$IO/2/ended-by-pid"
            exit "$rc"
          ) &
          c2=$!
        fi
        unset pairs2
      fi
      wait "$c1"
      rc=$?
      [ -z "$c2" ] || wait "$c2"
      WINEPREFIX="$W/compat/pfx" "$PROTON/files/bin/wineserver" -k 2>/dev/null
      sleep 2
      kill "$stub" 2>/dev/null
      wait "$stub" 2>/dev/null
      exit "$rc"
    ' _ "$WORK" "$IO" "$IO/stub" "$STUBBIN" "$ports" "$tmo" "$PROTON" "${stub_args[*]}" "$server_copy" \
    "$launch" "$login_port" "$api_port" "$clients" "$display2" "$second_after" "$tmo2" "$launch2" \
    "$@" < <(password_line) > "$OUT/sandbox.out" 2>&1 &
  local spid=$!
  python3 -I "$WATCHDOG" watch --root "$WORK" --out "$OUT" --sandbox-pid "$spid" \
    "${wlogs[@]}" --go "$IO/.go" "${wbinds[@]}" \
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
  rm -f "$OUT/.running" "$IO/.go"
  # The schedulers may still sleep toward a later time: they end with the run.
  kill "$shots" "$clicker" "$typist" 2>/dev/null || true
  wait "$shots" "$clicker" "$typist" 2>/dev/null || true
  local ran=0
  if [ -e "$IO/exit-code" ]; then ran=1; fi # the inner script writes it once the client has run
  echo "$wrc" > "$OUT/sandbox-exit-code"
  echo "$wdrc" > "$OUT/watchdog-exit-code"
  date -Is > "$OUT/ended"
  local v=0
  finish_run "$wdrc" "$wrc" "$ran" || v=$?
  exit "$v"
}

# The host-side inputs while a run is active, each at its time from now: screenshots of the nested
# display (and of client 2's, display2, as screen-2-<T>s.png), the caller's clicks and its key
# presses, client 2's (2@) on its display, each kind in time order across both clients
# (run-client.sh's cmd_run, run-bare.sh). Uses the caller's clicks, clicks2, presses, display2,
# second_after and tmo2. Sets shots, clicker and typist to their PIDs; each stops once OUT/.running
# is gone, or when killed.
start_inputs() {
  local out=$1 display=$2 tmo=$3 end2=$((${second_after:-0} + ${tmo2:-0})) last=$3
  if [ -n "${display2:-}" ] && [ "$end2" -gt "$last" ]; then last=$end2; fi
  # --- host-side screenshots while the run is active
  (
    start=$(date +%s)
    for t in 5 10 30 60 90 120 150 180 210 240 270 295; do
      [ "$t" -le "$last" ] || break
      now=$(date +%s)
      sl=$((start + t - now))
      [ "$sl" -le 0 ] || sleep "$sl"
      [ -e "$out/.running" ] || break
      if [ "$t" -le "$tmo" ]; then
        import -display ":$display" -window root "$out/screen-${t}s.png" 2>>"$out/screenshot.err" || true
      fi
      if [ -n "${display2:-}" ] && [ "$t" -le "$end2" ]; then
        import -display ":$display2" -window root "$out/screen-2-${t}s.png" 2>>"$out/screenshot.err" || true
      fi
    done
  ) &
  shots=$!
  # --- host-side clicks on the nested displays, at their times
  (
    start=$(date +%s)
    while read -r t d n xy; do
      now=$(date +%s)
      sl=$((start + t - now))
      [ "$sl" -le 0 ] || sleep "$sl"
      [ -e "$out/.running" ] || break
      label=""
      [ "$n" = 1 ] || label="2@"
      python3 -I "$HERE/xclick.py" --display ":$d" "${xy%,*}" "${xy#*,}" 2>>"$out/click.err" &&
        echo "$(date -Is) click $label$t:$xy" >> "$out/clicks.log"
    done < <(click_plan)
  ) &
  clicker=$!
  # --- host-side key presses and typing on the nested displays, at their times
  (
    start=$(date +%s)
    for p in "${presses[@]}"; do
      n=${p%%:*} rest=${p#*:}
      kind=${rest%%:*} rest=${rest#*:}
      t=${rest%%:*} what=${rest#*:}
      d=$display label=""
      if [ "$n" = 2 ]; then d=${display2:-} label="2@"; fi
      now=$(date +%s)
      sl=$((start + t - now))
      [ "$sl" -le 0 ] || sleep "$sl"
      [ -e "$out/.running" ] || break
      python3 -I "$HERE/xkey.py" --display ":$d" "--$kind=$what" 2>>"$out/keys.err" &&
        echo "$(date -Is) $kind $label$t:$what" >> "$out/keys.log"
    done
  ) &
  typist=$!
}

# After a run: the evidence, best effort (a failure there is reported but never changes the verdict:
# callers stop all runs on exit 3), then the verdict. Uses cmd_run's run, OUT, IO, STUB and PFX.
finish_run() {
  local wdrc=$1 wrc=$2 ran=$3
  collect_evidence || echo "run-client: collecting the evidence failed (exit $?); the verdict stands" >&2
  local two=""
  if [ "${clients:-1}" = 2 ]; then two=", client 2's proton exit $(cat "$OUT/io/exit-code-2" 2>/dev/null || echo '?')"; fi
  echo "run $run done: proton exit $(cat "$OUT/io/exit-code" 2>/dev/null || echo '?')$two, sandbox exit $wrc, watchdog exit $wdrc"
  verdict "$wdrc" "$wrc" "$ran" "$OUT"
}

# The verdict of a run from the watchdog's and the sandbox's exit statuses and whether the client
# ran: prints it, and returns the runner's exit status (see the header).
verdict() {
  local wdrc=$1 wrc=$2 ran=$3 out=$4
  case "$wdrc" in
    0) ;;
    3) echo "GATE EVENT: the driver watchdog stopped the run; see $out/watchdog-trigger.txt. Stop all runs." >&2; return 3 ;;
    *) echo "WATCHDOG FAILURE (exit $wdrc); see $out/watchdog.log and watchdog.err" >&2; return 4 ;;
  esac
  # A client that ran finished, whatever its own status (it may exit 95 to 98 itself).
  [ "$ran" != 1 ] || return 0
  [ "$wrc" != 98 ] || { echo "the watchdog never armed; the client was not started" >&2; return 5; }
  [ "$wrc" != 97 ] || { echo "fenji-stub wasn't running when the client was due (see $out/io/stub.out); the client was not started" >&2; return 7; }
  [ "$wrc" != 96 ] || { echo "fenji-server wasn't ready when the client was due (see $out/server.log); the client was not started" >&2; return 7; }
  [ "$wrc" != 95 ] || { echo "fenji-launch failed or printed more than gj_* pairs (see $out/io/launch.err and $out/sandbox.out); the client was not started" >&2; return 7; }
  echo "the sandbox ended before the client ran (sandbox exit $wrc); see $out/sandbox.out" >&2
  return 6
}

# The evidence, copied out of what the sandbox could write without following any link. Uses
# cmd_run's run, OUT, IO, STUB and PFX.
collect_evidence() {
  # Called from an || list, where set -e is off: every step records its own failure, and nothing is
  # deleted unless it was copied first.
  local bad=0 e r
  python3 -I "$WATCHDOG" collect --src "$IO" --dest "$OUT/io" --exclude stub --exclude .go || bad=1
  python3 -I "$WATCHDOG" collect --src "$IO/stub" --dest "$STUB" || bad=1
  if [ -f "$OUT/io/steam-1418100.log" ]; then mv "$OUT/io/steam-1418100.log" "$OUT/wine.log" || bad=1; fi
  if [ -f "$OUT/io/server.log" ]; then mv "$OUT/io/server.log" "$OUT/server.log" || bad=1; fi
  if python3 -I "$WATCHDOG" collect --src "$WORK/client/bin64" --dest "$OUT" \
    --name gujianol.log --name Log_gujianol.htm; then
    rm -f -- "$WORK/client/bin64/gujianol.log" "$WORK/client/bin64/Log_gujianol.htm" || bad=1
  else
    bad=1
  fi
  # Entries the client creates at its install root (settings/, ExportFace/, ExportPrefab/) and this
  # run's --client-file inputs go into the evidence and out of the copy, so every run starts from
  # the same copy. If they can't be copied they stay, and the next run refuses to start.
  if python3 -I "$WATCHDOG" collect --src "$WORK/client" --dest "$OUT/client-files" \
    --exclude bin64 --exclude data --exclude tools; then
    for e in "$WORK/client"/* "$WORK/client"/.[!.]*; do
      if [ -e "$e" ] || [ -L "$e" ]; then
        case "${e##*/}" in
          bin64 | data | tools) ;;
          *) rm -rf -- "$e" || bad=1 ;;
        esac
      fi
    done
  else
    bad=1
  fi
  # Client 2's own logs and install-root entries (Amendment B), the same way: into OUT/client2.
  local copies=("$WORK/client")
  if [ "${clients:-1}" = 2 ]; then
    copies+=("$WORK/client2")
    if [ -f "$OUT/io/2/steam-1418100.log" ]; then mv "$OUT/io/2/steam-1418100.log" "$OUT/wine-2.log" || bad=1; fi
    if python3 -I "$WATCHDOG" collect --src "$WORK/client2/bin64" --dest "$OUT/client2" \
      --name gujianol.log --name Log_gujianol.htm; then
      rm -f -- "$WORK/client2/bin64/gujianol.log" "$WORK/client2/bin64/Log_gujianol.htm" || bad=1
    else
      bad=1
    fi
    if python3 -I "$WATCHDOG" collect --src "$WORK/client2" --dest "$OUT/client2/client-files" \
      --exclude bin64 --exclude data --exclude tools; then
      for e in "$WORK/client2"/* "$WORK/client2"/.[!.]*; do
        if [ -e "$e" ] || [ -L "$e" ]; then
          case "${e##*/}" in
            bin64 | data | tools) ;;
            *) rm -rf -- "$e" || bad=1 ;;
          esac
        fi
      done
    else
      bad=1
    fi
  fi
  find "$PFX" -iname '*.sys' -printf '%P\t%y\t%l\t%s\t%T@\n' | sort > "$OUT/sys-after.tsv" || bad=1
  diff "$OUT/sys-before.tsv" "$OUT/sys-after.tsv" > "$OUT/sys-diff.txt"
  python3 -I "$WATCHDOG" collect --src "$PFX" --dest "$OUT/reg-after" --name system.reg --name user.reg || bad=1
  grep -o '^\[System\\\\ControlSet00[0-9]\\\\Services\\\\[^]\\]*' "$OUT/reg-after/system.reg" | sort -u > "$OUT/services-after.txt"
  diff "$OUT/services-before.txt" "$OUT/services-after.txt" > "$OUT/services-diff.txt"
  for r in system user; do
    diff "$OUT/reg-before/$r.reg" "$OUT/reg-after/$r.reg" > "$OUT/reg-$r.diff"
  done
  find "${copies[@]}" "$PFX/drive_c" "$WORK/home" -newer "$OUT/.start" -type f -not -path '*/mesa_shader_cache/*' \
    -printf '%TT\t%s\t%p\n' 2>/dev/null | sort > "$OUT/new-files.txt"
  redact_tickets "$OUT" || bad=1
  return "$bad"
}

# A Fenji ticket that reached the evidence anyway (Wine's +process lines, a client argument) is
# redacted in place; the files are listed in redactions.txt.
redact_tickets() {
  local dir=$1 f
  while IFS= read -r -d '' f; do
    sed -i -E 's/fenji1-[0-9a-fA-F]{64}/fenji1-<redacted>/g' -- "$f" || return 1
    printf '%s\n' "${f#"$dir"/}" >> "$dir/redactions.txt"
  done < <(grep -rlZE 'fenji1-[0-9a-fA-F]{64}' -- "$dir" 2>/dev/null)
}

# Sourced by test-run-client.sh for its functions; run as a command otherwise.
if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  case "${1:-}" in
    setup) shift; cmd_setup "$@" ;;
    run) shift; cmd_run "$@" ;;
    *) usage ;;
  esac
fi
