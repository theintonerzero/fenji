#!/usr/bin/env bash
# shellcheck disable=SC2016 # the single-quoted bash -c scripts expand their own $1, $2 ...
# Self-test for isolated-run.sh: proves nothing inside can reach the network, directly or through
# a host daemon. Linux only; needs bwrap. Run locally on Fedora and in CI.
# Every check runs; failures are reported as `FAIL -` lines and make the exit status 1.
set -euo pipefail
exec </dev/null   # a socket on stdin makes the wrapper warn on stderr, which several checks capture

run="$(cd "$(dirname "$0")" && pwd)/isolated-run.sh"
passed=0 failed=0
ok()   { echo "ok - $1"; passed=$((passed + 1)); }
fail() { echo "FAIL - $1" >&2; failed=$((failed + 1)); }
check() { local msg=$1; shift; if "$@"; then ok "$msg"; else fail "$msg"; fi; }
uid=$(id -u) gid=$(id -g)

# Cleanup first, so nothing created below can be left behind.
tmp="" marker_tmp="" marker_shm="" listener="" fake_x_pid="" fake_x_made=() x11_dir_made=""
cleanup() {
  [ -z "$listener" ] || kill "$listener" 2>/dev/null || true
  # The fake X server removes its socket and lock file when stopped. If it died first, remove them
  # here, but only while they are still the files it made.
  if [ -n "$fake_x_pid" ]; then kill "$fake_x_pid" 2>/dev/null || true; wait "$fake_x_pid" 2>/dev/null || true; fi
  local f
  for f in "${fake_x_made[@]}"; do
    if [ "$(stat -c %i -- "${f%:*}" 2>/dev/null)" = "${f##*:}" ]; then rm -f -- "${f%:*}" || true; fi
  done
  [ -z "$x11_dir_made" ] || rmdir /tmp/.X11-unix 2>/dev/null || true
  rm -f -- "$marker_tmp" "$marker_shm"
  [ -z "$tmp" ] || rm -rf -- "$tmp"
}
trap cleanup EXIT
tmp=$(realpath -- "$(mktemp -d)")
marker_tmp=$(mktemp -p /tmp fenji-isolation-test.XXXXXX)
marker_shm=$(mktemp -p /dev/shm fenji-isolation-test.XXXXXX)

host_hashes=$(sha256sum /etc/hosts /etc/resolv.conf 2>/dev/null || true)

# A running host Steam makes the wrapper refuse (checked at the end); the other checks override it.
export FENJI_ISOLATED_RUN_ALLOW_STEAM=1

# --- network: only loopback ----------------------------------------------------------------
# These helpers take extra wrapper options last (the --no-root checks reuse them). The negative
# ones print what happened inside, so a wrapper that fails to start can't pass them.
t_links() {
  [ "$("$run" "$@" -- ip -o link show | awk -F': ' '{print $2}')" = lo ] &&
    "$run" "$@" -- ip -o addr show dev lo | grep -q 'inet 127.0.0.1/8' &&
    "$run" "$@" -- ip -o addr show dev lo | grep -q 'inet6 ::1/128' &&
    "$run" "$@" -- ip -o link show dev lo | grep -q ',UP'
}
t_no_tcp() {  # HOST PORT [OPTION...]
  local host=$1 port=$2
  shift 2
  [ "$("$run" "$@" -- bash -c '(exec 3<>"/dev/tcp/$1/$2") 2>/dev/null || echo no-connection' \
       _ "$host" "$port")" = no-connection ]
}
t_no_dns() {  # [OPTION...]
  [ "$("$run" "$@" -- bash -c 'getent hosts example.com >/dev/null 2>&1 || echo unresolved')" = unresolved ]
}
check "only the loopback interface exists, up, with 127.0.0.1 and ::1" t_links
check "cannot open a TCP connection to the internet by IP" t_no_tcp 1.1.1.1 443
check "cannot open a TCP connection to the internet over IPv6" t_no_tcp 2606:4700:4700::1111 53
check "public hostnames don't resolve" t_no_dns

printf '127.0.0.1 login.fenji.test\n' > "$tmp/hosts"
check "--hosts file maps names to 127.0.0.1" \
  bash -c '[ "$("$1" --hosts "$2" -- getent hosts login.fenji.test | awk "{print \$1}")" = 127.0.0.1 ]' _ "$run" "$tmp/hosts"

mkdir "$tmp/d"; printf '127.0.0.1 dash.fenji.test\n' > "$tmp/d/-hosts"
check "--hosts accepts a path beginning with '-'" \
  bash -c 'cd "$2" && [ "$("$1" --hosts -hosts -- getent hosts dash.fenji.test | awk "{print \$1}")" = 127.0.0.1 ]' _ "$run" "$tmp/d"

check "loopback services inside are reachable (stub + client can talk)" "$run" -- bash -c '
  python3 -I -m http.server 18080 --bind 127.0.0.1 >/dev/null 2>&1 &
  pid=$!
  for _ in 1 2 3 4 5 6 7 8 9 10; do curl -sf -o /dev/null http://127.0.0.1:18080/ && { kill "$pid"; exit 0; }; sleep 0.3; done
  kill "$pid"; exit 1'

check "uid 0 inside can bind privileged ports (127.0.0.1:443, [::1]:80)" "$run" -- python3 -I -c '
import socket
a = socket.socket(); a.bind(("127.0.0.1", 443))
b = socket.socket(socket.AF_INET6); b.bind(("::1", 80))'

# --- usage errors exit 2 before running anything -------------------------------------------
t_usage() {
  local rc
  for argv in "--hosts /nonexistent/hosts -- true" "--bind /nonexistent/dir -- true" \
              "--ro-bind /nonexistent/dir -- true" "" "-- " "true" "--hosts" "--x11" "--bogus -- true"; do
    set +e
    # shellcheck disable=SC2086 # word splitting intended
    "$run" $argv >/dev/null 2>&1
    rc=$?
    set -e
    [ "$rc" -eq 2 ] || { echo "  '$argv' exited $rc" >&2; return 1; }
  done
}
check "usage errors, missing --hosts file and missing bind dirs exit 2" t_usage

t_refuse_binds() {
  local rc fh=$tmp/fakehome-parent/home
  mkdir -p "$fh/.steam" "$fh/.ssh" "$fh/.local/share/Steam/steamapps" "$fh/.var/app/com.valvesoftware.Steam/data"
  for argv in "--bind /run" "--ro-bind /" "--bind /tmp" "--ro-bind /etc" "--ro-bind /var" \
              "--bind $tmp/fakehome-parent" "--ro-bind /dev/pts" "--bind $fh/.steam" "--ro-bind $fh/.ssh" \
              "--bind $fh/.local/share/Steam/steamapps" "--ro-bind $fh/.local/share" \
              "--bind $fh/.var/app/com.valvesoftware.Steam/data"; do
    set +e
    # shellcheck disable=SC2086 # word splitting intended
    HOME=$tmp/fakehome-parent/home "$run" $argv -- true >/dev/null 2>&1
    rc=$?
    set -e
    [ "$rc" -eq 2 ] || { echo "  '$argv' exited $rc" >&2; return 1; }
  done
}
check "binding a dir that holds or sits in host socket dirs (/run, /, /tmp, /etc, /var, HOME's parent, /dev, ~/.steam, ~/.ssh, ~/.local/share/Steam, Flatpak Steam) is refused" t_refuse_binds

# Any socket or fifo in a bind dir, at any depth, gets it refused.
mkdir -p "$tmp/sockdir/deep/er" "$tmp/fifodir/sub"
python3 -I -c 'import socket,sys;socket.socket(socket.AF_UNIX).bind(sys.argv[1])' "$tmp/sockdir/deep/er/s"
mkfifo "$tmp/fifodir/sub/f"
t_refuse_socket_binds() {
  local rc err
  for argv in "--bind $tmp/sockdir" "--ro-bind $tmp/sockdir/deep" "--ro-bind $tmp/fifodir"; do
    set +e
    # shellcheck disable=SC2086 # word splitting intended
    err=$("$run" $argv -- true 2>&1 >/dev/null)
    rc=$?
    set -e
    if ! { [ "$rc" -eq 2 ] && [[ $err == *"socket or fifo"* ]]; }; then
      echo "  '$argv' exited $rc: $err" >&2
      return 1
    fi
  done
}
check "binding a dir containing a socket or fifo (at any depth) is refused" t_refuse_socket_binds

ibus=$HOME/.cache/ibus
if [ -n "$(find "$ibus" -type s -print -quit 2>/dev/null)" ]; then
  check "binding ~/.cache/ibus (live host ibus D-Bus sockets) is refused" \
    bash -c '"$1" --ro-bind "$2" -- true >/dev/null 2>&1; [ $? -eq 2 ]' _ "$run" "$ibus"
else
  echo "skip - no ibus sockets in ~/.cache/ibus, can't test refusing it"
fi

# A bind dir that is on, or contains, a FUSE or network filesystem is refused. No such mount can be
# made unprivileged, so this uses a ramfs in a private mount namespace and the self-test-only
# FENJI_ISOLATED_RUN_TEST_EXTRA_FSTYPES (it can only add types to the refused list). The space
# in the path checks that /proc/self/mountinfo escapes are decoded.
mkdir -p "$tmp/net fs/mnt" "$tmp/overmount/game" "$tmp/find-O3"
: > "$tmp/overmount/game/data.bin"
printf '#!/bin/sh\nexec %s -O3 "$@"\n' "$(command -v find)" > "$tmp/find-O3/find"
chmod +x "$tmp/find-O3/find"
if unshare -rm true 2>/dev/null; then
  check "binding a dir on, or containing, a refused filesystem type is refused (on / contains)" \
    unshare -rm bash -c '
      mount -t ramfs fenji-test "$2/mnt" || exit 1
      export FENJI_ISOLATED_RUN_TEST_EXTRA_FSTYPES=ramfs
      err=$("$1" --ro-bind "$2" -- true 2>&1); rc=$?
      [ "$rc" -eq 2 ] && [[ $err == *"ramfs mount at $2/mnt"* ]] || { echo "  contains: rc=$rc $err" >&2; exit 1; }
      err=$("$1" --bind "$2/mnt" -- true 2>&1); rc=$?
      [ "$rc" -eq 2 ] && [[ $err == *"on a ramfs filesystem"* ]] || { echo "  on: rc=$rc $err" >&2; exit 1; }
      unset FENJI_ISOLATED_RUN_TEST_EXTRA_FSTYPES
      "$1" --ro-bind "$2" -- true' _ "$run" "$tmp/net fs"
  # A socket bind-mounted over a file (only root can mount one on a real host): readdir reports
  # the covered file's type, so the scan must stat every entry, whatever its link count, which is
  # 0 once the socket's own name is removed (it still takes connections through the mount). Also
  # with find forced to -O3 (a PATH shim): findutils 4.9 defaults to -O2, which moves -type tests
  # ahead of tests that stat.
  check "binding a dir with a socket mounted over a file in it is refused, also once the socket's own name is gone (find at its default -O and at -O3)" \
    unshare -rm bash -c '
      python3 -I -c "import socket, sys; socket.socket(socket.AF_UNIX).bind(sys.argv[1])" "$2/s" &&
        mount --bind "$2/s" "$2/game/data.bin" || exit 1
      for when in mounted "name removed"; do
        [ "$when" = mounted ] || rm "$2/s" || exit 1
        for o in default -O3; do
          if [ "$o" = -O3 ]; then p=$3:$PATH; else p=$PATH; fi
          err=$(PATH=$p "$1" --bind "$2/game" -- true 2>&1); rc=$?
          [ "$rc" -eq 2 ] && [[ $err == *"socket or fifo"* ]] || { echo "  $when, find $o: rc=$rc $err" >&2; exit 1; }
        done
      done' _ "$run" "$tmp/overmount" "$tmp/find-O3"
else
  echo "skip - unshare -rm not permitted, can't test the filesystem-type refusal or a socket mounted over a file"
fi

# --- host daemons are unreachable (unix sockets cross network namespaces) -------------------
check "host systemd-resolved, system bus and nscd sockets are absent and unconnectable" "$run" -- bash -c '
  for s in /run/systemd/resolve/io.systemd.Resolve /run/dbus/system_bus_socket \
           /var/run/dbus/system_bus_socket /run/nscd/socket; do
    [ ! -e "$s" ] || exit 1
    ! python3 -I -c "import socket,sys;socket.socket(socket.AF_UNIX).connect(sys.argv[1])" "$s" 2>/dev/null || exit 1
  done
  [ ! -e /etc/resolv.conf ]'

if command -v busctl >/dev/null; then
  check "host session bus is unreachable (busctl --user Ping)" bash -c '! "$1" -- env XDG_RUNTIME_DIR=/run/user/$2 \
    DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$2/bus busctl --user call org.freedesktop.systemd1 \
    /org/freedesktop/systemd1 org.freedesktop.DBus.Peer Ping >/dev/null 2>&1' _ "$run" "$uid"
else
  echo "skip - busctl not installed, can't test the session bus"
fi
if command -v systemctl >/dev/null; then
  check "host systemd --user is unreachable" bash -c '! "$1" -- env XDG_RUNTIME_DIR=/run/user/$2 \
    systemctl --user is-system-running >/dev/null 2>&1' _ "$run" "$uid"
else
  echo "skip - systemctl not installed, can't test systemd --user"
fi
if command -v pactl >/dev/null; then
  check "host PulseAudio/PipeWire is unreachable (pactl info)" bash -c '! "$1" -- env XDG_RUNTIME_DIR=/run/user/$2 \
    PULSE_SERVER=unix:/run/user/$2/pulse/native pactl info >/dev/null 2>&1' _ "$run" "$uid"
else
  echo "skip - pactl not installed, can't test pactl info"
fi

check "CUPS, sshd, avahi, X11, ICE and user runtime sockets are absent" "$run" -- bash -c '
  for p in /run/cups/cups.sock /var/run/cups/cups.sock /run/ssh-unix-local /run/ssh-unix-local/socket \
           /run/avahi-daemon/socket /run/user /tmp/.ICE-unix; do
    [ ! -e "$p" ] || { echo "  present: $p" >&2; exit 1; }
  done
  [ -z "$(find /tmp/.X11-unix -mindepth 1 2>/dev/null)" ]'

# Class guard: no socket or fifo anywhere inside except one the command itself makes in its bound
# dir. find without -xdev descends into every mount (each bwrap mount is its own filesystem), and
# the fifo made inside is the positive control that it searched the bound mount; /proc holds none.
# -printf %M makes find stat every entry: readdir reports the file type of what a mount covers, so
# a plain -type s sees a socket bind-mounted over a file as a regular file.
find_ipc='find / -path /proc -prune -o -printf "%M %p\n" 2>/dev/null | sed -n "s/^[sp]......... //p"'
mkdir "$tmp/bound"
t_no_sockets() {  # [OPTION...]
  local found
  found=$("$run" "$@" --bind "$tmp/bound" -- bash -c 'mkfifo "$1/fifo-inside" && eval "$2" | sort' \
    _ "$tmp/bound" "$find_ipc" || true)
  rm -f -- "$tmp/bound/fifo-inside"
  [ "$found" = "$tmp/bound/fifo-inside" ] || { echo "  found: $(echo "$found" | tr '\n' ' ')" >&2; return 1; }
}
check "no socket or fifo is visible anywhere inside except one the command made in its bound dir" t_no_sockets

check "every mount inside is on the allowlist" "$run" --bind "$tmp/bound" -- bash -c '
  while read -r _ _ _ _ mp _; do
    case "$mp" in
      /|/usr|/proc|/proc/*|/dev|/dev/*|/tmp|/run|/var|/etc/*|"$HOME"|"$1") ;;
      *) echo "  unexpected mount: $mp" >&2; exit 1 ;;
    esac
  done < /proc/self/mountinfo' _ "$tmp/bound"

# Host abstract unix sockets are per network namespace: start one on the host and try it.
abstract="fenji-isolation-test-$$"
python3 -I -c '
import socket, sys, time
s = socket.socket(socket.AF_UNIX); s.bind("\0" + sys.argv[1]); s.listen(); time.sleep(60)' "$abstract" &
listener=$!
abs_connect='import socket,sys;socket.socket(socket.AF_UNIX).connect("\0"+sys.argv[1])'
t_abstract() {
  local _i
  for _i in 1 2 3 4 5 6 7 8 9 10; do python3 -I -c "$abs_connect" "$abstract" 2>/dev/null && break; sleep 0.2; done
  python3 -I -c "$abs_connect" "$abstract" || { echo "  host-side control connect failed" >&2; return 1; }
  ! "$run" -- python3 -I -c "$abs_connect" "$abstract" 2>/dev/null
}
check "host abstract unix sockets (e.g. X11's @/tmp/.X11-unix/X0) are unreachable" t_abstract
kill "$listener" 2>/dev/null || true; listener=""

# --- display: --x11 N passes one nested X server's socket and nothing else --------------------
# Display numbers 100-999 that nothing on the host uses (no socket, lock file or abstract socket),
# other than the arguments.
x11_free() {
  local n _i
  for _i in $(seq 200); do
    n=$((RANDOM % 900 + 100))
    case " $* " in *" $n "*) continue ;; esac
    if [ -e "/tmp/.X11-unix/X$n" ] || [ -L "/tmp/.X11-unix/X$n" ] || [ -e "/tmp/.X$n-lock" ] ||
       [ -L "/tmp/.X$n-lock" ] || grep -q "@/tmp/.X11-unix/X$n\$" /proc/net/unix; then
      continue
    fi
    echo "$n"
    return 0
  done
  return 1
}

t_x11_bad() {
  local spec want argv err rc m
  m=$(x11_free) || { echo "  no free display number" >&2; return 1; }
  for spec in "display number|--x11 abc" "display number|--x11 1000" "display number|--x11 042" \
              "display number|--x11 -1" "display number|--x11 --" "display number|--x11 4x2" \
              "only once|--x11 $m --x11 $m" "no X server socket|--x11 $m"; do
    want=${spec%%|*} argv=${spec#*|}
    set +e
    # shellcheck disable=SC2086 # word splitting intended
    err=$(env -u DISPLAY "$run" $argv -- true 2>&1 >/dev/null)
    rc=$?
    set -e
    if ! { [ "$rc" -eq 2 ] && [[ $err == *"$want"* ]]; }; then
      echo "  '$argv' exited $rc: $err" >&2
      return 1
    fi
  done
}
check "--x11 is refused for a bad display number, a second --x11 and a missing socket" t_x11_bad

# A fake nested X server on the host, on an unused display number: python listening on
# /tmp/.X11-unix/X<n> and on its abstract twin (as real servers do), accepting and closing, with
# the lock file X servers write, comm "Xwayland", -extension MIT-SHM on its command line and a
# stand-in libXfont2 without font-server support mapped, as a real server maps its own. It
# removes its files when stopped, or by itself after five minutes.
fake_x_py='
import ctypes, mmap, os, select, signal, socket, sys, time
lock, path, font = sys.argv[1:4]
fd = os.open(font, os.O_RDONLY)
font_map = mmap.mmap(fd, 0, prot=mmap.PROT_READ)
os.close(fd)
made = []
def stop(*_, code=0):
    for p, ino in made:
        try:
            if os.lstat(p).st_ino == ino:
                os.unlink(p)
        except OSError:
            pass
    os._exit(code)
for sig in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
    signal.signal(sig, stop)
ctypes.CDLL(None).prctl(15, b"Xwayland", 0, 0, 0)  # PR_SET_NAME
try:
    listeners = []
    for name in (path, "\0" + path):
        s = socket.socket(socket.AF_UNIX)
        s.bind(name)
        if name == path:
            made.append((path, os.lstat(path).st_ino))
        s.listen(8)
        listeners.append(s)
    fd = os.open(lock, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o444)
    made.append((lock, os.fstat(fd).st_ino))
    os.write(fd, b"%10d\n" % os.getpid())
    os.close(fd)
except OSError as e:
    print(e, file=sys.stderr)
    stop(code=1)
print("ready", *(ino for _, ino in made), flush=True)
end = time.monotonic() + 300
while time.monotonic() < end:
    for s in select.select(listeners, [], [], 1)[0]:
        s.accept()[0].close()
stop()
'
# Connects to a unix socket path, or to an abstract one written @name.
x11_probe='
import socket, sys
p = sys.argv[1]
socket.socket(socket.AF_UNIX).connect("\0" + p[1:] if p.startswith("@") else p)'

xn="" xm="" x11_host=""
if [ ! -e /tmp/.X11-unix ] && [ ! -L /tmp/.X11-unix ] && mkdir -m 1777 /tmp/.X11-unix 2>/dev/null; then
  x11_dir_made=1
fi
mkdir "$tmp/xfont"
echo "a font library without font-server support" > "$tmp/xfont/libXfont2.so.2"
if [ -d /tmp/.X11-unix ] && [ -w /tmp/.X11-unix ] && xn=$(x11_free) && xm=$(x11_free "$xn"); then
  : > "$tmp/fake-x.out"   # exists before the loop below reads it, however late the job starts
  python3 -I -c "$fake_x_py" "/tmp/.X$xn-lock" "/tmp/.X11-unix/X$xn" "$tmp/xfont/libXfont2.so.2" \
    -extension MIT-SHM > "$tmp/fake-x.out" 2>&1 &
  fake_x_pid=$!
  for _ in $(seq 50); do
    line=$(head -n 1 "$tmp/fake-x.out")
    if [[ $line == "ready "* ]]; then
      read -r _ ino_sock ino_lock <<<"$line"
      fake_x_made=("/tmp/.X11-unix/X$xn:$ino_sock" "/tmp/.X$xn-lock:$ino_lock")
      x11_host=1
      break
    fi
    kill -0 "$fake_x_pid" 2>/dev/null || break
    sleep 0.1
  done
fi

# Inside: DISPLAY=:N, no XAUTHORITY even when the caller has one, a working socket, and the
# server's abstract twin out of reach (the host side reaches both).
t_x11_inside() {
  local out
  if ! { python3 -I -c "$x11_probe" "/tmp/.X11-unix/X$xn" && python3 -I -c "$x11_probe" "@/tmp/.X11-unix/X$xn"; }; then
    echo "  host-side control connect failed" >&2
    return 1
  fi
  out=$(DISPLAY=:$xm XAUTHORITY=$tmp/Xauthority "$run" --x11 "$xn" -- bash -c '
    [ "$DISPLAY" = ":$1" ] && echo display=ok
    [ -z "${XAUTHORITY+x}" ] && echo xauthority=unset
    python3 -I -c "$2" "/tmp/.X11-unix/X$1" && echo socket=connects
    python3 -I -c "$2" "@/tmp/.X11-unix/X$1" 2>/dev/null || echo abstract=unreachable' _ "$xn" "$x11_probe" 2>&1) || true
  [ "$out" = "$(printf '%s\n' display=ok xauthority=unset socket=connects abstract=unreachable)" ] ||
    { echo "  got: $(tr '\n' ' ' <<<"$out")" >&2; return 1; }
}

# The socket is bound read-only: connect() still works, but the sandbox can't chmod (or otherwise
# change) the host's socket inode. A changed mode is put back, so the checks after this one run.
t_x11_readonly() {
  local out before after sock=/tmp/.X11-unix/X$xn
  before=$(stat -c %a -- "$sock") || return 1
  out=$(DISPLAY=:$xm "$run" --x11 "$xn" -- bash -c '
    if chmod 000 "/tmp/.X11-unix/X$1" 2>/dev/null; then echo chmod=allowed; else echo chmod=refused; fi
    python3 -I -c "$2" "/tmp/.X11-unix/X$1" && echo socket=connects' _ "$xn" "$x11_probe" 2>&1) || true
  after=$(stat -c %a -- "$sock") || return 1
  if [ "$after" != "$before" ]; then
    chmod "$before" -- "$sock"
    echo "  the host socket's mode changed from $before to $after; got: $(tr '\n' ' ' <<<"$out")" >&2
    return 1
  fi
  [ "$out" = "$(printf '%s\n' chmod=refused socket=connects)" ] || { echo "  got: $(tr '\n' ' ' <<<"$out")" >&2; return 1; }
}

# The class guard with --x11: the nested server's socket is the only socket inside (the fifo the
# command makes in its bound dir is the positive control), and /tmp/.X11-unix holds nothing else.
t_x11_only() {
  local found want
  found=$(DISPLAY=:$xm "$run" --bind "$tmp/bound" --x11 "$xn" -- bash -c '
    mkfifo "$1/fifo-inside" && eval "$2"
    find /tmp/.X11-unix -mindepth 1 -printf "in /tmp/.X11-unix: %p\n"' _ "$tmp/bound" "$find_ipc" | LC_ALL=C sort) || true
  rm -f -- "$tmp/bound/fifo-inside"
  want=$(printf '%s\n' "$tmp/bound/fifo-inside" "/tmp/.X11-unix/X$xn" "in /tmp/.X11-unix: /tmp/.X11-unix/X$xn" |
         LC_ALL=C sort)
  [ "$found" = "$want" ] || { echo "  got: $(tr '\n' ' ' <<<"$found")" >&2; return 1; }
}

t_x11_mounts() {
  local without with added removed
  without=$("$run" -- cut -d' ' -f5 /proc/self/mountinfo | LC_ALL=C sort) || return 1
  with=$(DISPLAY=:$xm "$run" --x11 "$xn" -- cut -d' ' -f5 /proc/self/mountinfo | LC_ALL=C sort) || return 1
  added=$(LC_ALL=C comm -13 <(printf '%s\n' "$without") <(printf '%s\n' "$with"))
  removed=$(LC_ALL=C comm -23 <(printf '%s\n' "$without") <(printf '%s\n' "$with"))
  if [ "$added" != "/tmp/.X11-unix/X$xn" ] || [ -n "$removed" ]; then
    echo "  added: $(tr '\n' ' ' <<<"$added")removed: $(tr '\n' ' ' <<<"$removed")" >&2
    return 1
  fi
}

# The caller's own display is refused in every DISPLAY form; another one, or none, is fine.
t_x11_session() {
  local d err rc want
  for d in ":$xn" ":$xn.0" "unix:$xn" "localhost:$xn.0" "unix/:$xn" ":0$xn" nonsense; do
    set +e
    err=$(DISPLAY=$d "$run" --x11 "$xn" -- true 2>&1 >/dev/null)
    rc=$?
    set -e
    if [ "$d" = nonsense ]; then want="doesn't name a display number"; else want="is your session's display"; fi
    if ! { [ "$rc" -eq 2 ] && [[ $err == *"$want"* ]]; }; then
      echo "  DISPLAY=$d exited $rc: $err" >&2
      return 1
    fi
  done
  DISPLAY=:$xm "$run" --x11 "$xn" -- true || { echo "  DISPLAY=:$xm (another display) was refused" >&2; return 1; }
  env -u DISPLAY "$run" --x11 "$xn" -- true || { echo "  an unset DISPLAY was refused" >&2; return 1; }
}

# With --no-root, as umu needs: the caller's uid inside, DISPLAY=:N and a working socket.
t_x11_noroot() {
  local out
  out=$(DISPLAY=:$xm "$run" --no-root --x11 "$xn" -- bash -c '
    [ "$(id -u)" = "$3" ] && echo uid=caller
    [ "$DISPLAY" = ":$1" ] && echo display=ok
    python3 -I -c "$2" "/tmp/.X11-unix/X$1" && echo socket=connects' _ "$xn" "$x11_probe" "$uid" 2>&1) || true
  [ "$out" = "$(printf '%s\n' uid=caller display=ok socket=connects)" ] ||
    { echo "  got: $(tr '\n' ' ' <<<"$out")" >&2; return 1; }
}

if [ -n "$x11_host" ]; then
  check "--x11 N: DISPLAY=:N and no XAUTHORITY inside; the nested server's socket takes connections, its abstract twin is out of reach" \
    t_x11_inside
  check "--x11 N: the socket is read-only inside (chmod fails, connect works) and its host mode is unchanged" \
    t_x11_readonly
  check "--x11 N: /tmp/.X11-unix holds only XN, the only socket anywhere inside" t_x11_only
  check "--x11 N adds exactly one mount, the socket at /tmp/.X11-unix/XN" t_x11_mounts
  check "--x11 N is refused when DISPLAY names display N (:N, :N.S, unix:N, host:N.S, unix/:N, :0N) or can't be parsed" \
    t_x11_session
  check "--no-root --x11 N: COMMAND runs as the caller's uid with DISPLAY=:N, and the nested server's socket takes connections" \
    t_x11_noroot
elif [ -n "$fake_x_pid" ]; then
  fail "a fake X server starts in /tmp/.X11-unix (the --x11 checks against it need one): $(head -c 300 "$tmp/fake-x.out")"
else
  echo "skip - /tmp/.X11-unix is missing and can't be made, or isn't writable: can't test --x11 against a fake X server"
fi
if [ -n "$fake_x_pid" ]; then kill "$fake_x_pid" 2>/dev/null || true; wait "$fake_x_pid" 2>/dev/null || true; fi
fake_x_pid=""

# The other refusals need X sockets, lock files and servers that must never sit in the host's /tmp,
# so they run in a private user and mount namespace whose /tmp is a fresh tmpfs, gone when it exits.
# The scripts are copied in first (they may live under /tmp). In there, `server COMM ARGS...`
# starts a process with that comm and command line and sets $pid; it maps a stand-in
# /tmp/lib/libXfont2.so.2 without font-server support, or the files fontlibs lists (FILE:FILE...,
# empty for none), before taking the comm. `xsock N` makes a socket at /tmp/.X11-unix/XN;
# `xlock N PID` writes /tmp/.XN-lock as X servers do; `refused N TEXT` checks that --x11 N exits
# 2 saying TEXT.
private_tmp='
  exec 9< "$1"
  mount -t tmpfs fenji-test /tmp && mkdir -m 1777 /tmp/.X11-unix /tmp/scripts /tmp/lib &&
    cp /proc/self/fd/9/isolated-run.sh /proc/self/fd/9/seccomp-deny-keys.py /tmp/scripts/ &&
    echo "a font library without font-server support" > /tmp/lib/libXfont2.so.2 || exit 1
  exec 9<&-
  run=/tmp/scripts/isolated-run.sh pids=()
  unset DISPLAY fontlibs
  trap "kill \${pids[*]} 2>/dev/null" EXIT
  server() {
    python3 -I -c "import ctypes, mmap, os, sys, time; \
      m = [mmap.mmap(os.open(f, os.O_RDONLY), 0, prot=mmap.PROT_READ) for f in sys.argv[1].split(\":\") if f]; \
      ctypes.CDLL(None).prctl(15, sys.argv[2].encode(), 0, 0, 0); time.sleep(60)" \
      "${fontlibs-/tmp/lib/libXfont2.so.2}" "$@" >/dev/null 2>&1 &
    pid=$!
    pids+=("$pid")
    local _i
    for _i in $(seq 50); do [ "$(cat "/proc/$pid/comm" 2>/dev/null)" = "$1" ] && return 0; sleep 0.1; done
    return 1
  }
  xsock() { python3 -I -c "import socket, sys; socket.socket(socket.AF_UNIX).bind(sys.argv[1])" "/tmp/.X11-unix/X$1"; }
  xlock() { printf "%10d\n" "$2" > "/tmp/.X$1-lock"; }
  refused() {
    local err rc
    err=$("$run" --x11 "$1" -- true 2>&1 >/dev/null)
    rc=$?
    if ! { [ "$rc" -eq 2 ] && [[ $err == *"$2"* ]]; }; then echo "  --x11 $1 exited $rc: $err" >&2; return 1; fi
  }
'
# Accepted: a nested Xwayland or Xephyr of yours with MIT-SHM off (the server matches extension
# names in any case). Refused: a regular file, or a symlink to a good socket, where the socket
# should be; a missing, symlinked or garbled lock file, or one naming a dead process, a compositor
# (GNOME's gnome-shell holds the locks of :0 and :1, both its Xwayland), a -rootless Xwayland or
# one with MIT-SHM still on.
t_x11_refused() {
  unshare -rm bash -c "$private_tmp"'
    server Xwayland -extension MIT-SHM || exit 1
    good=$pid
    xsock 5; xlock 5 "$good"
    "$run" --x11 5 -- true || { echo "  control: a nested Xwayland was refused" >&2; exit 1; }
    server Xephyr -extension mit-shm || exit 1
    xsock 6; xlock 6 "$pid"
    "$run" --x11 6 -- true || { echo "  control: a nested Xephyr with -extension mit-shm was refused" >&2; exit 1; }
    : > /tmp/.X11-unix/X7; xlock 7 "$good"; refused 7 "not a socket" || exit 1
    ln -s X5 /tmp/.X11-unix/X8; xlock 8 "$good"; refused 8 "not a socket" || exit 1
    xsock 9; refused 9 "no lock file" || exit 1
    xsock 10; ln -s .X5-lock /tmp/.X10-lock; refused 10 "not a regular file" || exit 1
    xsock 11; echo 1x > /tmp/.X11-lock; refused 11 "process ID" || exit 1
    sleep 0 & dead=$!; wait "$dead"
    xsock 12; xlock 12 "$dead"; refused 12 "not running" || exit 1
    server gnome-shell || exit 1
    xsock 13; xlock 13 "$pid"; refused 13 "not Xwayland, Xephyr or Xvfb" || exit 1
    server Xwayland -rootless -extension MIT-SHM || exit 1
    xsock 14; xlock 14 "$pid"; refused 14 "-rootless" || exit 1
    server Xwayland -extension GLX || exit 1
    xsock 15; xlock 15 "$pid"; refused 15 "MIT-SHM" || exit 1
    server Xwayland -extension MIT-SHM +extension mit-shm || exit 1
    xsock 16; xlock 16 "$pid"; refused 16 "MIT-SHM" || exit 1' _ "$(dirname "$run")"
}

# Other sockets, and a non-socket, in /tmp/.X11-unix must not come along.
t_x11_alone() {
  local out
  out=$(unshare -rm bash -c "$private_tmp"'
    server Xwayland -extension MIT-SHM || exit 1
    xsock 5; xlock 5 "$pid"; xsock 6; xsock 7; : > /tmp/.X11-unix/X8
    "$run" --x11 5 -- find /tmp/.X11-unix -mindepth 1' _ "$(dirname "$run")" 2>&1) || true
  [ "$out" = /tmp/.X11-unix/X5 ] || { echo "  got: $(tr '\n' ' ' <<<"$out")" >&2; return 1; }
}

# Another user's socket: a root-owned host socket bind-mounted over XN shows as nobody's in there.
rootsock=""
for s in /run/systemd/journal/stdout /run/systemd/journal/socket /run/dbus/system_bus_socket /run/udev/control; do
  if [ -S "$s" ] && [ ! -L "$s" ] && [ "$(stat -c %u -- "$s")" = 0 ]; then rootsock=$s; break; fi
done
t_x11_foreign() {
  unshare -rm bash -c "$private_tmp"'
    server Xwayland -extension MIT-SHM || exit 1
    : > /tmp/.X11-unix/X5 && mount --bind "$2" /tmp/.X11-unix/X5 || exit 1
    xlock 5 "$pid"
    refused 5 "belongs to uid"' _ "$(dirname "$run")" "$rootsock"
}

# Font-server support: a libXfont that looks up host names (getaddrinfo, gethostbyname) connects
# wherever a client's font path says, from the host. Refused: either name in a mapped libXfont,
# also next to a clean one (which maps lists first, as it is mapped last, at a lower address);
# no libXfont mapped; one deleted, or hidden by a mount, since it was mapped; one grep fails on
# (a stub exiting 2).
t_x11_fonts() {
  unshare -rm bash -c "$private_tmp"'
    mkdir /tmp/lib/tcp /tmp/lib/old /tmp/lib/gone /tmp/lib/hidden /tmp/stub &&
      printf "\0getaddrinfo\0" > /tmp/lib/tcp/libXfont2.so.2 &&
      printf "\0gethostbyname\0" > /tmp/lib/old/libXfont.so.1 &&
      cp /tmp/lib/libXfont2.so.2 /tmp/lib/gone/ && cp /tmp/lib/libXfont2.so.2 /tmp/lib/hidden/ &&
      printf "#!/bin/sh\nexit 2\n" > /tmp/stub/grep && chmod +x /tmp/stub/grep || exit 1
    server Xwayland -extension MIT-SHM || exit 1
    xsock 5; xlock 5 "$pid"
    "$run" --x11 5 -- true || { echo "  control: a server with a clean libXfont was refused" >&2; exit 1; }
    PATH=/tmp/stub:$PATH refused 5 "grep failed" || exit 1
    fontlibs=/tmp/lib/tcp/libXfont2.so.2 server Xwayland -extension MIT-SHM || exit 1
    xsock 6; xlock 6 "$pid"; refused 6 "with font-server support" || exit 1
    fontlibs=/tmp/lib/old/libXfont.so.1:/tmp/lib/libXfont2.so.2 server Xwayland -extension MIT-SHM || exit 1
    xsock 7; xlock 7 "$pid"; refused 7 "with font-server support" || exit 1
    fontlibs= server Xwayland -extension MIT-SHM || exit 1
    xsock 8; xlock 8 "$pid"; refused 8 "maps no libXfont" || exit 1
    fontlibs=/tmp/lib/gone/libXfont2.so.2 server Xwayland -extension MIT-SHM || exit 1
    rm /tmp/lib/gone/libXfont2.so.2 || exit 1
    xsock 9; xlock 9 "$pid"; refused 9 "restart the server" || exit 1
    fontlibs=/tmp/lib/hidden/libXfont2.so.2 server Xwayland -extension MIT-SHM || exit 1
    mount -t tmpfs fenji-test /tmp/lib/hidden || exit 1
    xsock 10; xlock 10 "$pid"; refused 10 "not a readable file here" || exit 1' _ "$(dirname "$run")"
}

# The server matches extension names ignoring ASCII case, whatever the locale. A locale whose
# lowercase of I is a dotless i (Turkish, Azeri) must change nothing: MIT-SHM re-enabled after a
# lowercase -extension mit-shm is refused, and the recipe in the header is accepted.
turkish=""
for l in tr_TR.UTF-8 az_AZ.UTF-8; do
  if [ "$(LC_ALL=$l bash -c 'x=I; echo "${x,,}"' 2>/dev/null)" != i ]; then turkish=$l; break; fi
done
t_x11_locale() {
  unshare -rm bash -c "$private_tmp"'
    export LC_ALL=$2
    server Xwayland -extension mit-shm +extension MIT-SHM || exit 1
    xsock 5; xlock 5 "$pid"; refused 5 "MIT-SHM" || exit 1
    server Xwayland :99 -geometry 1280x720 -ac -nolisten tcp -noreset -extension MIT-SHM -shm || exit 1
    xsock 6; xlock 6 "$pid"
    "$run" --x11 6 -- true || { echo "  the recipe was refused under LC_ALL=$2" >&2; exit 1; }' _ "$(dirname "$run")" "$turkish"
}

# /tmp/.X11-unix itself, as X servers want it: a real directory, root's or yours, sticky. Otherwise
# someone else could swap XN after the check (bwrap's bind follows symlinks).
t_x11_dir() {
  unshare -rm bash -c "$private_tmp"'
    server Xwayland -extension MIT-SHM || exit 1
    xsock 5; xlock 5 "$pid"
    "$run" --x11 5 -- true || { echo "  control: a good /tmp/.X11-unix was refused" >&2; exit 1; }
    chmod 0777 /tmp/.X11-unix && refused 5 "sticky" && chmod 1777 /tmp/.X11-unix || exit 1
    mv /tmp/.X11-unix /tmp/x11 && ln -s x11 /tmp/.X11-unix || exit 1
    refused 5 "/tmp/.X11-unix is not a directory"' _ "$(dirname "$run")"
}

# Another user's /tmp/.X11-unix: a root-owned sticky host dir, bound read-only over it, shows as
# nobody's in there.
rootdir=""
for d in /var/tmp /dev/shm; do
  if [ -d "$d" ] && [ ! -L "$d" ] && [ -k "$d" ] && [ "$(stat -c %u -- "$d")" = 0 ]; then rootdir=$d; break; fi
done
t_x11_dir_foreign() {
  unshare -rm bash -c "$private_tmp"'
    mount --bind "$2" /tmp/.X11-unix && mount -o remount,bind,ro /tmp/.X11-unix || exit 1
    refused 5 "/tmp/.X11-unix belongs to uid"' _ "$(dirname "$run")" "$rootdir"
}

if unshare -rm true 2>/dev/null; then
  check "--x11 N is refused unless XN is a socket (not a symlink) and /tmp/.XN-lock names a running rootful Xwayland, Xephyr or Xvfb with MIT-SHM off" \
    t_x11_refused
  check "--x11 N: only XN appears inside, whatever else /tmp/.X11-unix holds" t_x11_alone
  if [ -n "$rootsock" ]; then
    check "--x11 N is refused when XN is another user's socket" t_x11_foreign
  else
    echo "skip - no root-owned socket in /run to stand in for another user's X server"
  fi
  check "--x11 N is refused unless every libXfont the server maps is a readable file without font-server support (getaddrinfo, gethostbyname)" \
    t_x11_fonts
  if [ -n "$turkish" ]; then
    check "--x11 N matches MIT-SHM in ASCII under LC_ALL=$turkish: re-enabled is refused, the recipe accepted" t_x11_locale
  else
    echo "skip - no tr_TR.UTF-8 or az_AZ.UTF-8 locale (dotless-i lowercasing), can't test locale-proof MIT-SHM matching"
  fi
  check "--x11 N is refused unless /tmp/.X11-unix is a real directory (not a symlink) with the sticky bit" t_x11_dir
  if [ -n "$rootdir" ]; then
    check "--x11 N is refused when /tmp/.X11-unix belongs to another user" t_x11_dir_foreign
  else
    echo "skip - no root-owned sticky dir (/var/tmp, /dev/shm) to stand in for another user's /tmp/.X11-unix"
  fi
else
  echo "skip - unshare -rm not permitted, can't fake X servers in a private /tmp"
fi

# --- environment, fds, terminal -------------------------------------------------------------
check "display, bus and runtime variables are not passed; BROWSER=/bin/false" \
  env DISPLAY=:0 XAUTHORITY=/x WAYLAND_DISPLAY=wayland-0 XDG_RUNTIME_DIR=/run/user/"$uid" \
    DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/"$uid"/bus DBUS_SYSTEM_BUS_ADDRESS=unix:path=/x \
    PULSE_SERVER=unix:/x SSH_AUTH_SOCK=/x "$run" -- bash -c '
  for v in DISPLAY XAUTHORITY WAYLAND_DISPLAY XDG_RUNTIME_DIR DBUS_SESSION_BUS_ADDRESS \
           DBUS_SYSTEM_BUS_ADDRESS PULSE_SERVER SSH_AUTH_SOCK; do [ -z "${!v+x}" ] || exit 1; done
  [ "$BROWSER" = /bin/false ]'

# An fd the caller left open (no close-on-exec) must not reach the command.
t_fds() { "$run" -- bash -c '[ ! -e /proc/self/fd/7 ]' 7<"$tmp/hosts"; }
check "inherited file descriptors above stderr are closed" t_fds

# The command must have no controlling terminal (TIOCSTI typing into the host terminal needs one).
check "no controlling terminal inside, even when started from one (no TIOCSTI injection)" python3 -I -c '
import os, pty, sys
pid, fd = pty.fork()
if pid == 0:
    os.execv(sys.argv[1], [sys.argv[1], "--", "python3", "-I", "-c",
        "import os,sys\ntry:\n os.open(\"/dev/tty\", os.O_RDWR)\nexcept OSError:\n sys.exit(0)\nsys.exit(1)"])
out = b""
while True:
    try:
        chunk = os.read(fd, 1024)
    except OSError:
        break
    if not chunk:
        break
    out += chunk
_, status = os.waitpid(pid, 0)
sys.exit(0 if os.waitstatus_to_exitcode(status) == 0 else 1)' "$run"

# A socket on stdin/stdout/stderr would be a channel to whatever holds the other end: the wrapper
# swaps it for /dev/null and says so on stderr.
check "a socket on stdin, stdout or stderr is replaced with /dev/null, with a warning" python3 -I -c '
import socket, subprocess, sys
inside = "for n in 0 1 2; do [ ! -S /proc/self/fd/$n ] || exit 1; done; [ -c /proc/self/fd/0 ] && [ -c /proc/self/fd/1 ]"
a, b = socket.socketpair()
r = subprocess.run([sys.argv[1], "--", "bash", "-c", inside], stdin=b, stdout=b, stderr=subprocess.PIPE)
err = r.stderr.decode()
ok1 = r.returncode == 0 and "stdin is a socket" in err and "stdout is a socket" in err
r = subprocess.run([sys.argv[1], "--", "bash", "-c", "[ ! -S /proc/self/fd/2 ] && [ -c /proc/self/fd/2 ]"],
                   stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=b)
a.settimeout(5)
try:
    err2 = a.recv(4096).decode()
except TimeoutError:
    err2 = ""
ok2 = r.returncode == 0 and "stderr is a socket" in err2
if not (ok1 and ok2):
    print("  stdin/stdout:", ok1, repr(err), "stderr:", ok2, repr(err2), file=sys.stderr)
sys.exit(0 if ok1 and ok2 else 1)' "$run"

check "core dumps are off inside (soft and hard RLIMIT_CORE are 0)" bash -c '
  ulimit -S -c "$(ulimit -H -c)" 2>/dev/null || true
  [ "$("$1" -- bash -c "ulimit -S -c; ulimit -H -c" | tr "\n" " ")" = "0 0 " ]' _ "$run"

# --- kernel keys: no request-key upcalls ------------------------------------------------------
# request_key makes the kernel run /sbin/request-key as root on the host (dns_resolver,
# cifs.upcall, nfsidmap ...), so add_key, request_key and keyctl must fail with EPERM, on every
# syscall ABI. First, the filter itself, run through a small classic-BPF interpreter.
check "the seccomp filter denies the key syscalls for x86_64, x32 and i386, allows others, kills unknown arches" \
  python3 -I -c '
import struct, subprocess, sys
prog = subprocess.run([sys.executable, "-I", sys.argv[1]], capture_output=True, check=True).stdout
assert prog and len(prog) % 8 == 0
ins = [struct.unpack_from("=HBBI", prog, i) for i in range(0, len(prog), 8)]
def run(arch, nr):
    data = struct.pack("=II", nr & 0xffffffff, arch).ljust(64, b"\0")
    a = pc = 0
    while True:
        code, jt, jf, k = ins[pc]
        if code == 0x20: a = struct.unpack_from("=I", data, k)[0]; pc += 1
        elif code == 0x15: pc += 1 + (jt if a == k else jf)
        elif code == 0x35: pc += 1 + (jt if a >= k else jf)
        elif code == 0x45: pc += 1 + (jt if a & k else jf)
        elif code == 0x06: return k
        else: sys.exit(f"unknown opcode {code:#x}")
X86_64, I386, EPERM, ALLOW, KILL = 0xc000003e, 0x40000003, 0x00050001, 0x7fff0000, 0x80000000
cases = [(X86_64, n, EPERM) for n in (248, 249, 250)]
cases += [(X86_64, 0x40000000 | n, EPERM) for n in (0, 1, 248, 249, 250)]
cases += [(I386, n, EPERM) for n in (286, 287, 288)]
cases += [(X86_64, n, ALLOW) for n in (0, 1, 59, 247, 251, 286, 287, 288)]
cases += [(I386, n, ALLOW) for n in (1, 11, 20, 248, 249, 250, 285, 289)]
cases += [(0xc00000b7, 219, KILL), (0x40000028, 311, KILL), (0, 250, KILL)]
bad = [(hex(a), n, hex(run(a, n)), hex(w)) for a, n, w in cases if run(a, n) != w]
if bad: print("  arch, nr, got, want:", bad, file=sys.stderr)
sys.exit(1 if bad else 0)' "$(dirname "$run")/seccomp-deny-keys.py"

# Then the real thing inside. The probe joins a fresh anonymous session keyring first, so that if
# the filter were missing nothing it creates could land in the host's shared session keyring.
key_probe='
import ctypes, mmap, os, struct, sys
libc = ctypes.CDLL(None, use_errno=True)
libc.syscall.restype = ctypes.c_long
def arg(x): return ctypes.c_long(x) if isinstance(x, int) else ctypes.c_char_p(x)
def sc(nr, *a):
    ctypes.set_errno(0)
    r = libc.syscall(ctypes.c_long(nr), *map(arg, a))
    return "EPERM" if r == -1 and ctypes.get_errno() == 1 else f"rc={r},errno={ctypes.get_errno()}"
sc(250, 1, 0)                                  # keyctl(KEYCTL_JOIN_SESSION_KEYRING, NULL)
res = {}
if sys.argv[1] == "64":
    res["add_key"] = sc(248, b"user", b"fenji-isolation-test", b"x", 1, -2)
    res["request_key"] = sc(249, b"user", b"debug:fenji-isolation-test", b"upcall", -3)
    res["keyctl"] = sc(250, 0, -3, 0)          # KEYCTL_GET_KEYRING_ID, session keyring
    for nr, name in ((248, "add_key"), (249, "request_key"), (250, "keyctl")):
        res["x32 " + name] = sc(0x40000000 | nr, 0, 0, 0)
else:                                          # i386 ABI: int 0x80 from this 64-bit process
    buf = mmap.mmap(-1, 4096, prot=mmap.PROT_READ | mmap.PROT_WRITE | mmap.PROT_EXEC)
    fn = ctypes.CFUNCTYPE(ctypes.c_int)(ctypes.addressof(ctypes.c_char.from_buffer(buf)))
    def i386(nr, *a):                          # push rbx; mov eax..edi, imm32; int 0x80; pop rbx; ret
        code = b"\x53\xb8" + struct.pack("<I", nr)
        for op, v in zip(b"\xbb\xb9\xba\xbe\xbf", (list(a) + [0] * 5)[:5]):
            code += bytes([op]) + struct.pack("<I", v & 0xffffffff)
        buf.seek(0); buf.write(code + b"\xcd\x80\x5b\xc3")
        return fn()
    pid = os.fork()
    if pid == 0:
        os._exit(0 if i386(20) == os.getpid() else 3)  # getpid: is the i386 ABI there at all?
    if os.waitpid(pid, 0)[1] != 0:
        print("no-ia32"); sys.exit(0)
    for nr, name, a in ((286, "add_key", ()), (287, "request_key", ()), (288, "keyctl", (0, -3))):
        r = i386(nr, *a)
        res["i386 " + name] = "EPERM" if r == -1 else f"rc={r}"
for k, v in res.items(): print(f"{k}={v}")
'
t_keys() {  # WANT ABI [OPTION...]
  local out want=$1 abi=$2
  shift 2
  out=$("$run" "$@" -- python3 -I -c "$key_probe" "$abi" 2>&1) || true
  if [ "$out" = no-ia32 ]; then echo "  (no i386 ABI on this kernel)" >&2; return 2; fi
  if [ "$(grep -c '=EPERM$' <<<"$out")" -ne "$want" ] || grep -qv '=EPERM$' <<<"$out"; then
    echo "  got: $(tr '\n' ' ' <<<"$out")" >&2; return 1
  fi
}
check "add_key, request_key and keyctl fail with EPERM inside (x86_64 and x32 syscall numbers)" t_keys 6 64
set +e; t_keys 3 i386 2>/dev/null; rc=$?; set -e
if [ "$rc" -eq 2 ]; then
  echo "skip - the kernel has no i386 syscall ABI, can't test it at runtime (the BPF check covers it)"
else
  check "add_key, request_key and keyctl fail with EPERM inside (i386 ABI via int 0x80)" t_keys 3 i386
fi

if command -v keyctl >/dev/null; then
  # keyctl session - on the host gives the run a throwaway session keyring, for the same reason.
  t_keyctl() {
    keyctl session - "$run" -- bash -c '! keyctl show >/dev/null 2>&1 &&
      ! keyctl request2 user debug:fenji-isolation-test upcall @s >/dev/null 2>&1' >/dev/null 2>&1
  }
  check "keyctl show and keyctl request2 (a request-key upcall) fail inside" t_keyctl
else
  echo "skip - keyctl not installed, can't test it (the syscall checks above still ran)"
fi

# --- filesystem ------------------------------------------------------------------------------
mkdir "$tmp/rw" "$tmp/ro"; : > "$tmp/ro/file"
check "--bind dir is writable inside, --ro-bind dir is read-only" bash -c '
  "$1" --bind "$2" --ro-bind "$3" -- bash -c "echo hi > \"\$1/new\" && [ -f \"\$2/file\" ] && ! touch \"\$2/file\" 2>/dev/null && ! touch \"\$2/new\" 2>/dev/null" _ "$2" "$3" &&
  [ "$(cat "$2/new")" = hi ] && [ ! -e "$3/new" ]' _ "$run" "$tmp/rw" "$tmp/ro"

mkdir "$tmp/rw/sub"
check "working dir is the caller's inside a bound dir, else /" bash -c '
  [ "$(cd "$2/sub" && "$1" --bind "$2" -- pwd)" = "$2/sub" ] && [ "$(cd "$2/sub" && "$1" -- pwd)" = / ]' \
  _ "$run" "$tmp/rw"

check "host /tmp and /dev/shm are not visible and inside writes don't reach them" bash -c '
  "$1" -- bash -c "[ ! -e \"\$1\" ] && [ ! -e \"\$2\" ] && : > /tmp/fenji-inside-\$\$ && : > /dev/shm/fenji-inside-\$\$" _ "$2" "$3" &&
  ! compgen -G "/tmp/fenji-inside-*" >/dev/null && ! compgen -G "/dev/shm/fenji-inside-*" >/dev/null' \
  _ "$run" "$marker_tmp" "$marker_shm"

# Fake HOME (never the real Steam dir) with a Steam fifo, pid file and symlink; the host recreates
# the fifo mid-run. Inside, HOME is a fresh empty tmpfs, so none of it may appear.
fakehome=$tmp/home
mkdir -p "$fakehome/.steam"
mkfifo "$fakehome/.steam/steam.pipe"
printf 1 > "$fakehome/.steam/steam.pid"
ln -s /nonexistent/steam-root "$fakehome/.steam/root"
: > "$fakehome/host-file"
HOME=$fakehome "$run" --bind "$tmp/rw" -- bash -c '
  sleep 1
  { [ "$HOME" = "$2" ] && echo home=same
    [ -z "$(find "$HOME" -mindepth 1)" ] && echo home=empty
    : > "$HOME/w" && echo home=writable
    [ -e "$HOME/.steam/steam.pipe" ] || [ -L "$HOME/.steam/root" ] && echo steam=visible || echo steam=absent
  } > "$1"
' _ "$tmp/rw/inside.out" "$fakehome" &
bgpid=$!
sleep 0.3
rm -f "$fakehome/.steam/steam.pipe"; mkfifo "$fakehome/.steam/steam.pipe"   # host recreates the fifo
t_home() {
  wait "$bgpid" || return 1
  if ! { [ "$(cat "$tmp/rw/inside.out")" = "$(printf 'home=same\nhome=empty\nhome=writable\nsteam=absent')" ] &&
         [ ! -e "$fakehome/w" ]; }; then
    echo "  got: $(tr '\n' ' ' < "$tmp/rw/inside.out")" >&2
    return 1
  fi
}
check "HOME is a fresh empty tmpfs: host files and Steam IPC (even recreated mid-run) are absent" t_home

# Proton's pressure-vessel runs its own bwrap; it must work inside and still see only lo.
t_nested_bwrap() {  # [OPTION...]
  "$run" "$@" -- bash -c '
    [ "$(bwrap --unshare-all --ro-bind /usr /usr --symlink usr/bin /bin --symlink usr/lib64 /lib64 \
         --symlink usr/lib /lib --proc /proc --dev /dev -- ip -o link show | awk -F": " "{print \$2}")" = lo ]'
}
check "a nested bwrap sandbox works inside and still sees only lo" t_nested_bwrap

# --- --no-root: the caller's uid and gid, no capabilities, isolation unchanged ----------------
# For tools that refuse to run as uid 0 (umu-run). Inside, the generated passwd and group name the
# caller as id does on the host, or user and users when a name is empty, starts with '-' or has
# anything but letters, digits, '.', '_' and '-'.
want_name() {  # NAME FALLBACK
  case "$1" in
    ""|-*|*[!ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789._-]*) printf '%s\n' "$2" ;;
    *) printf '%s\n' "$1" ;;
  esac
}
want_user=$(want_name "$(id -un 2>/dev/null)" user) want_group=$(want_name "$(id -gn 2>/dev/null)" users)
t_noroot_ids() {
  local opts out want
  want=$(printf '%s\n' "$uid" "$gid" "$want_user" "$want_group" \
           "$want_user:x:$uid:$gid::${HOME%/}:/bin/bash" "$want_group:x:$gid:")
  for opts in --no-root "--no-root --no-root"; do
    # shellcheck disable=SC2086 # word splitting intended
    out=$("$run" $opts -- bash -c 'id -u; id -g; id -un; id -gn; getent passwd "$(id -u)"; getent group "$(id -g)"' \
            2>&1) || true
    [ "$out" = "$want" ] || { echo "  $opts: got: $(tr '\n' ' ' <<<"$out")" >&2; return 1; }
  done
}
check "--no-root (also given twice) runs COMMAND as the caller's uid and gid, named in passwd and group as on the host" \
  t_noroot_ids

t_noroot_caps() {
  local out
  out=$("$run" --no-root -- grep -E '^Cap(Prm|Eff|Amb):' /proc/self/status 2>&1) || true
  [ "$out" = "$(printf 'Cap%s:\t0000000000000000\n' Prm Eff Amb)" ] ||
    { echo "  got: $(tr '\n\t' '  ' <<<"$out")" >&2; return 1; }
}
check "--no-root: no capabilities inside (CapPrm, CapEff and CapAmb are all zero)" t_noroot_caps

check "--no-root: binding 127.0.0.1:80 fails, while a listener on a high loopback port takes connections" \
  "$run" --no-root -- python3 -I -c '
import socket, sys
try:
    socket.socket().bind(("127.0.0.1", 80))
    sys.exit("  bound 127.0.0.1:80")
except PermissionError:
    pass
srv = socket.socket(); srv.bind(("127.0.0.1", 18080)); srv.listen()
cli = socket.create_connection(("127.0.0.1", 18080), timeout=5)
srv.accept()[0].sendall(b"ok")
sys.exit(0 if cli.recv(2) == b"ok" else "  no reply from the listener on 127.0.0.1:18080")'

check "--no-root: only the loopback interface exists, up, with 127.0.0.1 and ::1" t_links --no-root
check "--no-root: cannot open a TCP connection to the internet by IP" t_no_tcp 1.1.1.1 443 --no-root
check "--no-root: public hostnames don't resolve" t_no_dns --no-root
check "--no-root: add_key, request_key and keyctl fail with EPERM inside (x86_64 and x32 syscall numbers)" \
  t_keys 6 64 --no-root
check "--no-root: a nested user namespace can be created inside (unshare --user --map-root-user true)" \
  "$run" --no-root -- unshare --user --map-root-user true
check "--no-root: a nested bwrap sandbox works inside and still sees only lo" t_nested_bwrap --no-root

# The generated passwd and group give 0 to root and 65534 to nobody, so --no-root refuses a caller
# with either as uid or gid: run it in user namespaces that give the caller those ids.
t_noroot_refused() {
  local ids err rc
  for ids in "0 $gid" "65534 $gid" "$uid 0" "$uid 65534"; do
    set +e
    err=$(unshare --map-user="${ids% *}" --map-group="${ids#* }" "$run" --no-root -- true 2>&1 >/dev/null)
    rc=$?
    set -e
    if ! { [ "$rc" -eq 2 ] && [[ $err == *"--no-root needs"* ]]; }; then
      echo "  uid ${ids% *}, gid ${ids#* }: exited $rc: $err" >&2
      return 1
    fi
  done
  unshare --map-user="$uid" --map-group="$gid" "$run" --no-root -- true ||
    { echo "  control: the caller's own uid and gid were refused" >&2; return 1; }
}
if unshare --map-user="$uid" --map-group="$gid" true 2>/dev/null; then
  check "--no-root is refused (exit 2) when the caller's uid or gid is 0 or 65534 (root and nobody inside)" \
    t_noroot_refused
else
  echo "skip - unshare --map-user/--map-group unavailable or not permitted, can't test the --no-root refusal of uid or gid 0 and 65534"
fi

# Names that could break the generated passwd and group, or pass for an option, become user and
# users. An id stand-in first on PATH (outside only: its dir isn't inside) prints the contents of
# $tmp/id-shim/user and group for -un and -gn, and fails when they are missing.
mkdir "$tmp/id-shim"
printf '#!/bin/sh\nd=%s\ncase "$1" in -un) f=$d/user ;; -gn) f=$d/group ;; *) exec %s "$@" ;; esac\n[ -e "$f" ] && exec cat -- "$f"\nexit 1\n' \
  "'$tmp/id-shim'" "$(command -v id)" > "$tmp/id-shim/id"
chmod +x "$tmp/id-shim/id"
t_noroot_names() {
  local name out
  for name in "" -x "a:b" "a b" "a/b" $'\xc3\xbc' $'\xff' $'a\nroot2:x:0:0::/:/bin/sh'; do
    printf '%s' "$name" > "$tmp/id-shim/user"
    printf '%s' "$name" > "$tmp/id-shim/group"
    out=$(PATH=$tmp/id-shim:$PATH "$run" --no-root -- bash -c 'id -un; id -gn; grep -c "" /etc/passwd /etc/group' \
            2>&1) || true
    [ "$out" = "$(printf '%s\n' user users /etc/passwd:3 /etc/group:3)" ] ||
      { echo "  name $(printf %q "$name"): got: $(tr '\n' ' ' <<<"$out")" >&2; return 1; }
  done
  rm -f -- "$tmp/id-shim/user" "$tmp/id-shim/group"   # id finds no name at all
  out=$(PATH=$tmp/id-shim:$PATH "$run" --no-root -- bash -c 'id -un; id -gn' 2>&1) || true
  [ "$out" = "$(printf '%s\n' user users)" ] || { echo "  no names: got: $(tr '\n' ' ' <<<"$out")" >&2; return 1; }
  printf 'A.b_c-9' > "$tmp/id-shim/user"
  printf 'g-1.x_y' > "$tmp/id-shim/group"
  out=$(PATH=$tmp/id-shim:$PATH "$run" --no-root -- bash -c 'id -un; id -gn' 2>&1) || true
  [ "$out" = "$(printf '%s\n' A.b_c-9 g-1.x_y)" ] ||
    { echo "  names of letters, digits, '.', '_' and '-': got: $(tr '\n' ' ' <<<"$out")" >&2; return 1; }
}
check "--no-root names the caller user and users when id gives no name, or one that starts with '-' or has characters other than letters, digits, '.', '_' and '-'" \
  t_noroot_names

# --- --cpu-topology: the host's /sys/devices/system/cpu, read-only, nothing else of /sys --------
# Wine and Proton build their processor information from that subtree; without the option there
# is no /sys inside at all. Each check runs as uid 0 and with --no-root. Read-only is read from the
# mount's own options: sysfs refuses most writes anyway, so a write attempt would prove nothing.
t_no_sys() {  # [OPTION...]
  [ "$("$run" "$@" -- bash -c '[ -e /sys ] || [ -L /sys ] || echo absent')" = absent ]
}
t_cpu_tree() {  # [OPTION...]
  local out
  out=$("$run" "$@" -- bash -c 'ls -A /sys; ls -A /sys/devices; ls -A /sys/devices/system' 2>&1) || true
  [ "$out" = "$(printf '%s\n' devices system cpu)" ] || { echo "  got: $(tr '\n' ' ' <<<"$out")" >&2; return 1; }
}
t_cpu_online() {  # [OPTION...]
  local host out
  if ! host=$(cat /sys/devices/system/cpu/online) || [ -z "$host" ]; then
    echo "  can't read the host's /sys/devices/system/cpu/online" >&2
    return 1
  fi
  out=$("$run" "$@" -- cat /sys/devices/system/cpu/online 2>&1) || true
  [ "$out" = "$host" ] || { echo "  host: $host, inside: $(tr '\n' ' ' <<<"$out")" >&2; return 1; }
}
# Every mount in /sys inside, with the mount's own options (mountinfo field 6; the superblock's,
# after " - ", stay rw): just /sys/devices/system/cpu, with ro among them.
t_cpu_ro() {  # [OPTION...]
  local out
  out=$("$run" "$@" -- cut -d' ' -f5,6 /proc/self/mountinfo 2>&1 | LC_ALL=C grep -E '^/sys(/| )') || true
  if [[ $out != "/sys/devices/system/cpu "* || $out == *$'\n'* || ,${out#* }, != *,ro,* ]]; then
    echo "  mounts in /sys, with their options: $(tr '\n' ' ' <<<"$out")" >&2
    return 1
  fi
}
# The option adds exactly one mount, /sys/devices/system/cpu, and removes none; given twice, it
# adds nothing more. The options given go to every run.
t_cpu_mounts() {  # [OPTION...]
  local without with twice added removed again
  without=$("$run" "$@" -- cut -d' ' -f5 /proc/self/mountinfo | LC_ALL=C sort) || return 1
  with=$("$run" "$@" --cpu-topology -- cut -d' ' -f5 /proc/self/mountinfo | LC_ALL=C sort) || return 1
  twice=$("$run" "$@" --cpu-topology --cpu-topology -- cut -d' ' -f5 /proc/self/mountinfo |
          LC_ALL=C sort) || return 1
  added=$(LC_ALL=C comm -13 <(printf '%s\n' "$without") <(printf '%s\n' "$with"))
  removed=$(LC_ALL=C comm -23 <(printf '%s\n' "$without") <(printf '%s\n' "$with"))
  again=$(LC_ALL=C comm -3 <(printf '%s\n' "$with") <(printf '%s\n' "$twice"))
  if [ "$added" != /sys/devices/system/cpu ] || [ -n "$removed" ] || [ -n "$again" ]; then
    echo "  added: $(tr '\n' ' ' <<<"$added")removed: $(tr '\n' ' ' <<<"$removed")" \
         "given twice, differs in: $(tr '\n\t' '  ' <<<"$again")" >&2
    return 1
  fi
}
# shellcheck disable=SC2086 # word splitting intended: $mode is empty or --no-root
for mode in "" --no-root; do
  flag="${mode:+$mode }--cpu-topology"
  check "without --cpu-topology${mode:+ ($mode)}, /sys is absent inside" t_no_sys $mode
  check "$flag: /sys holds only devices, /sys/devices only system, /sys/devices/system only cpu" \
    t_cpu_tree $mode --cpu-topology
  check "$flag: /sys/devices/system/cpu/online inside is the host's" t_cpu_online $mode --cpu-topology
  check "$flag: the only mount in /sys is /sys/devices/system/cpu, with ro among its mount options" \
    t_cpu_ro $mode --cpu-topology
  check "$flag (also given twice) adds exactly one mount, /sys/devices/system/cpu, and removes none" \
    t_cpu_mounts $mode
  check "$flag: no socket or fifo is visible anywhere inside except one the command made in its bound dir" \
    t_no_sockets $mode --cpu-topology
  check "$flag: only the loopback interface exists, up, with 127.0.0.1 and ::1" t_links $mode --cpu-topology
  check "$flag: cannot open a TCP connection to the internet by IP" t_no_tcp 1.1.1.1 443 $mode --cpu-topology
  check "$flag: public hostnames don't resolve" t_no_dns $mode --cpu-topology
  check "$flag: a nested bwrap sandbox works inside and still sees only lo" t_nested_bwrap $mode --cpu-topology
done

# Refused (exit 2) on a host without /sys/devices/system/cpu: an empty tmpfs hides it here, in a
# private mount namespace (no test hook needed).
if unshare -rm true 2>/dev/null; then
  check "--cpu-topology is refused (exit 2) on a host without /sys/devices/system/cpu" \
    unshare -rm bash -c '
      "$1" --cpu-topology -- true || { echo "  control: refused while /sys/devices/system/cpu exists" >&2; exit 1; }
      mount -t tmpfs fenji-test /sys/devices/system || exit 1
      err=$("$1" --cpu-topology -- true 2>&1); rc=$?
      [ "$rc" -eq 2 ] && [[ $err == *"has no /sys/devices/system/cpu"* ]] || { echo "  rc=$rc $err" >&2; exit 1; }' _ "$run"
else
  echo "skip - unshare -rm not permitted, can't test refusing --cpu-topology without /sys/devices/system/cpu"
fi

# --- Steam refusal and host files ------------------------------------------------------------
if pgrep -x steam >/dev/null 2>&1 || pgrep -x steamwebhelper >/dev/null 2>&1 ||
   pgrep -f com.valvesoftware.Steam >/dev/null 2>&1; then
  check "refuses to run while the host Steam client (native or Flatpak) is running" bash -c '
    set +e; env -u FENJI_ISOLATED_RUN_ALLOW_STEAM "$1" -- true 2>/dev/null; [ $? -eq 2 ]' _ "$run"
else
  echo "skip - Steam is not running, can't test the refusal"
fi

check "host /etc/hosts and /etc/resolv.conf are unchanged" \
  bash -c '[ "$(sha256sum /etc/hosts /etc/resolv.conf 2>/dev/null || true)" = "$1" ]' _ "$host_hashes"

if [ "$failed" -gt 0 ]; then
  echo "$failed of $((passed + failed)) checks failed" >&2
  exit 1
fi
echo "all $passed checks passed"
