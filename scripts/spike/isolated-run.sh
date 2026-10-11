#!/usr/bin/env bash
# Runs COMMAND in a bubblewrap sandbox with no network but loopback and no way to reach a host
# daemon, so nothing started inside (Wine, the game client, anti-cheat) can reach the internet or
# the game's official services, look up their names, or ask a host process to do it (CLAUDE.md
# ground rule 4). Linux x86_64 only; needs bwrap, python3 and unprivileged user namespaces.
#
#   scripts/spike/isolated-run.sh [--hosts FILE] [--bind DIR]... [--ro-bind DIR]... [--x11 N]
#                                 [--no-root] [--cpu-topology] -- COMMAND [ARGS...]
#
#   --hosts FILE    FILE's content becomes /etc/hosts inside (after localhost entries), e.g. to
#                   point the client's hostnames at a stub listening on 127.0.0.1 inside.
#   --bind DIR      make DIR visible read-write inside at the same (symlink-resolved) path.
#   --ro-bind DIR   the same, read-only.
#   --x11 N         give COMMAND X display :N, a nested X server you started for the run (see
#                   "Display" below): its socket is the only X11 thing inside, with DISPLAY=:N.
#   --no-root       run COMMAND as your own uid and gid with no capabilities, not as uid 0 (see
#                   "Inside" below), for tools that refuse to run as root, such as umu-run.
#   --cpu-topology  bind the host's /sys/devices/system/cpu (CPU topology, caches, frequencies...)
#                   read-only at the same path; Wine and Proton build their processor information
#                   from it. No other part of /sys is inside, and without it there is no /sys,
#                   unless a --bind/--ro-bind DIR exposes some.
#   Pass extra environment with `env VAR=value COMMAND`; only PATH, HOME, TERM, LANG, LANGUAGE,
#   LC_* and USER are inherited, plus BROWSER=/bin/false (and DISPLAY with --x11).
#
# Inside: in a new user namespace, uid 0 with only CAP_NET_BIND_SERVICE (ports < 1024) and
# CAP_SETFCAP (lets a nested bwrap, e.g. Proton's pressure-vessel, map uid 0), or with --no-root
# your own uid and gid with no capabilities at all (so no ports below 1024; a nested bwrap such as
# pressure-vessel then maps your uid itself); new network, pid, ipc, uts and cgroup namespaces, so
# the only interface is lo and host abstract sockets are out of reach; a new session (no
# controlling tty, so no TIOCSTI typing into the host terminal); all inherited file descriptors
# except stdin/out/err are closed, and a socket on stdin/out/err is replaced with /dev/null (with a
# warning). Other stdin/out/err pass through: start it from a terminal emulator (not a Linux VT) or
# redirect them. Core dumps are off (RLIMIT_CORE 0, soft and hard), so systemd-coredump keeps no
# client memory; keep abrt's autoreporting off on this host.
#
# A seccomp filter (scripts/spike/seccomp-deny-keys.py, built at each run; needs python3) makes
# add_key, request_key and keyctl fail with EPERM for x86_64 and i386 and denies the x32 ABI. A
# key request can make the kernel run /sbin/request-key as root on the host with our arguments
# (dns_resolver does a host DNS lookup), and the host's session and user keyrings are shared with
# the sandbox. Filters stack, so a nested bwrap keeps it. x86_64 hosts only.
#
# The filesystem is an allowlist, not a mask: an empty root plus read-only /usr (and the
# /bin, /lib... links), a private /proc, a minimal /dev, empty tmpfs /tmp, /run, /var, /dev/shm
# and $HOME, a handful of read-only /etc files (linker, certificates, fonts, time zone), and
# generated hosts, nsswitch.conf (files only), passwd and group (root and nobody, and with
# --no-root you: by your names, or as user and users if a name is empty, starts with '-' or has
# anything but letters, digits, '.', '_' and '-'). No resolv.conf. No host display, audio, bus or
# daemon socket (systemd-resolved, D-Bus, systemd --user, PulseAudio/PipeWire, CUPS, sshd, avahi,
# X11, Wayland, ibus, Steam) exists inside unless a --bind/--ro-bind DIR exposes it, bar the one
# nested X server socket --x11 N names (see "Display"). Nor does /sys, unless a --bind/--ro-bind
# DIR exposes part of it, bar the read-only /sys/devices/system/cpu that --cpu-topology adds.
# Bound dirs must belong to the run. One is refused (exit 2) if, at start:
#   - it holds a socket or fifo at any depth (also one mounted over another file), or can't be
#     fully scanned for them;
#   - it is on, or contains a mount of, a FUSE, network or automount filesystem (a host daemon
#     answers its file operations, often over the network): fuse, fuse.*, fuseblk, nfs, nfs4,
#     cifs, smb3, smbfs, ncpfs, 9p, virtiofs, ceph, glusterfs, lustre, gpfs, beegfs, orangefs,
#     gfs2, ocfs2, afs, coda, davfs, sshfs, autofs, rpc_pipefs. (A game copy on an ntfs-3g
#     disk is fuseblk: copy it to a local filesystem.)
#   - it is, or is a parent of, /, /run, /tmp/.X11-unix, /tmp/.ICE-unix, /var, /etc, /dev, /proc,
#     $HOME, the runtime dir or a Steam dir, or lies inside /run, /dev, /proc, the X11/ICE dirs,
#     ~/.steam, ~/.local/share/Steam, Flatpak Steam, ~/.ssh (ControlMaster sockets) or ~/.gnupg.
#     The client must therefore not live in a Steam library (use the DepotDownloader copy).
# These checks run once: a socket created in a bound dir mid-run is visible inside, so never
# bind a dir a host daemon writes to.
#
# Display (--x11 N). The host's own display never goes inside: an X client can read other
# clients' windows and type into them (XTEST), and a session Xwayland, being -rootless, may pass
# XTEST on to the compositor (libei) as input to native windows too, terminals included. Instead,
# start a dedicated rootful Xwayland outside, on a free display number. It is one window on the
# Wayland desktop and keeps input injected into it to itself (Xwayland forwards XTEST only when
# -rootless). For example:
#
#   Xwayland :99 -geometry 1280x720 -ac -nolisten tcp -noreset -extension MIT-SHM -shm &
#   scripts/spike/isolated-run.sh --x11 99 [other options] -- COMMAND [ARGS...]
#
# -ac turns off X authorisation, since no Xauthority goes inside (so any local process may use
# :99, which holds only the sandboxed client); -nolisten tcp keeps it off the network.
# -extension MIT-SHM is required: the server would attach SysV shared memory by the client's ID
# in the host's IPC namespace, i.e. any segment of yours. -shm (no glamor, hence no DRI3) is
# advised but not checked: with DRI3 the server hands the client a GPU device fd.
# The server's libXfont2 must be built without font-server support, or a client could set a
# font path of tcp/HOST:PORT and the server would look HOST up and connect to it, from the host.
# Fedora's is, since 2.0.8-3 (`rpm -q --changelog libXfont2 | grep 'Disable X fontserver'`); the
# wrapper checks the libXfont the server has loaded (below).
#
# Run nothing else on :N: with -ac the client can read and type into any window there, so a
# terminal on :N is a shell on the host. And the server itself runs on the host, with network
# access: a bug in what it does for clients (GLX, running xkbcomp, FreeType parsing fonts from
# paths the client names, including font files it writes into a bound dir) is a way out. That
# is inherent to X: keep the server dedicated to the run and short-lived.
#
# --x11 N is refused (exit 2) unless, at start:
#   - N is 0-999 without leading zeros, and --x11 is given once;
#   - DISPLAY doesn't name display N (:N, :N.S, host:N, unix:N; a DISPLAY that can't be parsed
#     refuses any N);
#   - /tmp/.X11-unix is a directory (not a symlink), root's or yours, with the sticky bit, as X
#     servers make it (otherwise someone else could swap XN after these checks);
#   - /tmp/.X11-unix/XN is a socket of yours, not a symlink;
#   - /tmp/.XN-lock, which X servers write, is a regular file of yours naming a running Xwayland,
#     Xephyr or Xvfb that isn't -rootless and has MIT-SHM off. This refuses session displays
#     whatever DISPLAY says: a compositor locks the displays it serves under its own PID (GNOME's
#     gnome-shell holds :0 and :1, both its session Xwayland), and a session Xwayland is -rootless;
#   - that server maps a libXfont, and every libXfont it maps is still a readable file at its path
#     (one deleted or replaced since the server started means: restart the server) without the
#     names getaddrinfo or gethostbyname in it, the host-name lookups of font-server code.
# Inside, the socket is bound read-only (connect() works; nothing inside can chmod or touch the
# host's socket), alone, at /tmp/.X11-unix/XN: no Xauthority, no other host X socket, and the
# server's abstract socket stays out of reach in the host's network namespace. Like the bind
# checks, these run once.
#
# No GPU: /dev inside has no dri nodes (and -shm keeps DRI3 from passing one in), so render in
# software. For Vulkan (DXVK, VKD3D-Proton), pick Mesa's lavapipe inside the command:
# `env VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json COMMAND` (newer Vulkan loaders
# also read VK_DRIVER_FILES). OpenGL falls back to llvmpipe by itself.
#
# A running host Steam (native or Flatpak) is refused (exit 2); the self-test alone overrides that
# with FENJI_ISOLATED_RUN_ALLOW_STEAM=1 (and adds refused filesystem types for its own test with
# FENJI_ISOLATED_RUN_TEST_EXTRA_FSTYPES; that can only refuse more). Exit 2 also on usage
# errors, a missing --hosts file or bind dir, a refused bind dir or --x11 display, --no-root when
# your uid or gid is 0 or 65534 (root and nobody inside), --cpu-topology unless
# /sys/devices/system/cpu is a directory on sysfs with nothing mounted below it (only the kernel's
# own CPU sysfs goes inside: in LXC containers, for one, lxcfs mounts FUSE files there), a
# non-x86_64 host, or missing bwrap, pgrep or python3. Otherwise the exit status is COMMAND's.
set -euo pipefail

usage() { sed -n '2,23p' "$0" >&2; exit 2; }
die2() { echo "isolated-run: $*" >&2; exit 2; }

hosts="" binds=() x11="" noroot="" cputopo=""
while [ $# -gt 0 ]; do
  case "$1" in
    --hosts)
      [ $# -ge 2 ] || usage
      [ -f "$2" ] || die2 "hosts file not found: $2"
      hosts=$(realpath -- "$2")
      shift 2
      ;;
    --bind|--ro-bind)
      [ $# -ge 2 ] || usage
      [ -d "$2" ] || die2 "bind dir not found: $2"
      binds+=("$1" "$(realpath -- "$2")")
      shift 2
      ;;
    --x11)
      [ $# -ge 2 ] || usage
      [ -z "$x11" ] || die2 "--x11 may be given only once"
      # Digits spelled out: a range like [0-9] can match other characters in some locales.
      case "$2" in
        0|[123456789]|[123456789][0123456789]|[123456789][0123456789][0123456789]) x11=$2 ;;
        *) die2 "--x11 takes an X display number, 0 to 999 without leading zeros, not '$2'" ;;
      esac
      shift 2
      ;;
    --no-root) noroot=1; shift ;;
    --cpu-topology) cputopo=1; shift ;;
    --) shift; break ;;
    *) usage ;;
  esac
done
[ $# -gt 0 ] || usage
[ "$(uname -s)" = Linux ] || die2 "Linux only"
command -v bwrap >/dev/null || die2 "bwrap not found (install bubblewrap)"
command -v pgrep >/dev/null || die2 "pgrep not found, can't check for Steam"
command -v python3 >/dev/null || die2 "python3 not found, can't build the seccomp filter"
[ "$(uname -m)" = x86_64 ] || die2 "x86_64 only (the seccomp filter knows x86 syscall numbers only)"
here=$(cd -- "$(dirname -- "$0")" && pwd -P)

# --no-root: COMMAND runs as your uid and gid, named in the generated passwd and group (below).
# Refused if either is 0 or 65534, which are root and nobody there (and running this as real root
# is out of scope). entry_name NAME FALLBACK prints NAME, or FALLBACK if NAME is empty, starts
# with '-' or has anything but letters, digits, '.', '_' and '-', which could break those files'
# format. Spelled out, as for --x11: a range can match other characters in some locales.
entry_name() {
  case "$1" in
    ""|-*|*[!ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789._-]*) printf '%s\n' "$2" ;;
    *) printf '%s\n' "$1" ;;
  esac
}
if [ -n "$noroot" ]; then
  caller_uid=$(id -u)
  caller_gid=$(id -g)
  for n in "$caller_uid" "$caller_gid"; do
    case "$n" in
      0|65534) die2 "--no-root needs a uid and gid other than 0 and 65534 (root and nobody inside); yours are $caller_uid and $caller_gid" ;;
    esac
  done
  caller_user=$(entry_name "$(id -un 2>/dev/null)" user)
  caller_group=$(entry_name "$(id -gn 2>/dev/null)" users)
fi

if [ "${FENJI_ISOLATED_RUN_ALLOW_STEAM:-}" != 1 ]; then
  if pgrep -x steam >/dev/null || pgrep -x steamwebhelper >/dev/null || pgrep -f com.valvesoftware.Steam >/dev/null; then
    echo "isolated-run: quit Steam first (its IPC would let the client reach the network)" >&2
    exit 2
  fi
fi

home=${HOME:-/root}
case "$home" in /?*) ;; *) home=/root ;; esac
home=${home%/}

# Refused filesystem types: a host daemon serves every file operation, often over the network.
refused_fstype() {
  case "$1" in
    fuse|fuse.*|fuseblk|nfs|nfs4|cifs|smb3|smbfs|ncpfs|9p|virtiofs|ceph|glusterfs|lustre|gpfs|\
    beegfs|orangefs|gfs2|ocfs2|afs|coda|davfs|sshfs|autofs|rpc_pipefs) return 0 ;;
  esac
  local t
  for t in ${FENJI_ISOLATED_RUN_TEST_EXTRA_FSTYPES:-}; do [ "$1" = "$t" ] && return 0; done
  return 1
}
# Mount table: mount points (octal escapes like \040 decoded) and filesystem types.
mnt_points=() mnt_types=()
if [ "${#binds[@]}" -gt 0 ] || [ -n "$cputopo" ]; then
  while IFS= read -r line; do
    read -r _ _ _ _ mp _ <<<"$line"
    printf -v mp '%b' "${mp//\\/\\0}"
    rest=${line#* - }
    mnt_points+=("$mp") mnt_types+=("${rest%% *}")
  done < /proc/self/mountinfo
fi
# mounts_at DIR sets on_type to the type of the filesystem DIR is on (the deepest mount at or above
# it; the last listed wins a tie, being on top), and below_points and below_types to the mount
# point and type of every mount below it.
mounts_at() {
  local m mp t on_len=-1
  on_type="" below_points=() below_types=()
  for ((m = 0; m < ${#mnt_points[@]}; m++)); do
    mp=${mnt_points[$m]} t=${mnt_types[$m]}
    case "$1/" in
      "${mp%/}/"*) if [ "${#mp}" -ge "$on_len" ]; then on_len=${#mp} on_type=$t; fi ;;
    esac
    case "$mp/" in
      "$1/"?*) below_points+=("$mp") below_types+=("$t") ;;
    esac
  done
}

# --cpu-topology binds the host's /sys/devices/system/cpu, and nothing but the kernel's own CPU
# sysfs may go inside with it: the directory must be on sysfs, with nothing mounted below it. (In
# LXC containers, for one, lxcfs mounts FUSE files there, which a host daemon serves.)
if [ -n "$cputopo" ]; then
  [ -d /sys/devices/system/cpu ] ||
    die2 "--cpu-topology: the host has no /sys/devices/system/cpu directory (is sysfs mounted on /sys?)"
  mounts_at /sys/devices/system/cpu
  [ "$on_type" = sysfs ] ||
    die2 "--cpu-topology: /sys/devices/system/cpu is on ${on_type:-an unknown filesystem}, not sysfs; only the kernel's own CPU sysfs is bound"
  [ "${#below_points[@]}" -eq 0 ] ||
    die2 "--cpu-topology: a filesystem (${below_types[0]}) is mounted at ${below_points[0]}, below /sys/devices/system/cpu; only the kernel's own CPU sysfs is bound, with nothing mounted in it"
fi

# Refuse binds that obviously contain host daemon sockets: a dir that is, or is a parent of, one
# of these, or lies inside one marked "under".
resolve() { realpath -m -- "$1"; }
steam_dirs=("$(resolve "$home/.steam")" "$(resolve "$home/.local/share/Steam")"
            "$(resolve "$home/.var/app/com.valvesoftware.Steam")")
contains=(/ /run /tmp/.X11-unix /tmp/.ICE-unix /var /etc /dev /proc "$home" "/run/user/$(id -u)"
          "${steam_dirs[@]}")
under=(/run /dev /proc /tmp/.X11-unix /tmp/.ICE-unix "${steam_dirs[@]}" "$(resolve "$home/.ssh")"
       "$(resolve "$home/.gnupg")")
if [ -n "${XDG_RUNTIME_DIR:-}" ]; then
  contains+=("$XDG_RUNTIME_DIR")
  under+=("$(resolve "$XDG_RUNTIME_DIR")")
fi
for ((i = 1; i < ${#binds[@]}; i += 2)); do
  b=${binds[$i]}
  for c in "${contains[@]}"; do
    c=$(resolve "$c")
    case "$c/" in "${b%/}/"*) die2 "refusing to bind $b: it contains $c (host daemon sockets)" ;; esac
  done
  for u in "${under[@]}"; do
    case "$b/" in "${u%/}/"*) die2 "refusing to bind $b: it is inside $u (host daemon sockets)" ;; esac
  done
  # The filesystem b is on, and every mount below it (see mounts_at).
  mounts_at "$b"
  for ((m = 0; m < ${#below_points[@]}; m++)); do
    ! refused_fstype "${below_types[$m]}" ||
      die2 "refusing to bind $b: it contains a ${below_types[$m]} mount at ${below_points[$m]} (a host daemon serves it)"
  done
  ! refused_fstype "$on_type" || die2 "refusing to bind $b: it is on a $on_type filesystem (a host daemon serves it)"
  # No -xdev: bwrap binds recursively, so everything below b is visible inside. %M makes find
  # stat every entry, as the self-test's class guard does. -type would take the type readdir
  # reports, which is that of the file a mount covers, so a socket bind-mounted over a file would
  # pass as a file; a test that stats placed before it doesn't help either, as findutils 4.9's
  # default -O2 moves -type ahead of such tests.
  found=$(find "$b" -printf '%M %p\n' 2>/dev/null | LC_ALL=C sed -n 's/^[sp]......... //p') ||
    die2 "refusing to bind $b: can't scan all of it for sockets and fifos"
  [ -z "$found" ] ||
    die2 "refusing to bind $b: it contains a socket or fifo (${found%%$'\n'*}); bind only dirs that belong to the run"
done

# --x11 N: a nested X server of the caller's own, never a session one (see "Display" above).
# x11_server_problem LOCK prints why the process LOCK names isn't such a server (nothing if it is).
x11_server_problem() {
  local pid="" comm="" arg prev="" rootless="" shm=on path last="" libs=() f rc
  { read -r pid < "$1"; } 2>/dev/null || true
  case "$pid" in ""|*[!0123456789]*) echo "$1 doesn't hold a process ID"; return ;; esac
  if ! { read -r comm < "/proc/$pid/comm"; } 2>/dev/null; then
    echo "process $pid, named by $1, is not running"
    return
  fi
  case "$comm" in
    Xwayland|Xephyr|Xvfb) ;;
    *) echo "$1 names $comm (PID $pid), not Xwayland, Xephyr or Xvfb (a compositor locks its session displays under its own PID)"
       return ;;
  esac
  # The server reads its options in order and matches extension names ignoring ASCII case
  # (strcasecmp, C locale). Spelled out, not ${arg,,}: that follows the caller's locale, and a
  # Turkish one lowercases I to a dotless i.
  { while IFS= read -r -d '' arg; do
      [ "$arg" != -rootless ] || rootless=1
      case "$prev $arg" in
        "-extension "[Mm][Ii][Tt]-[Ss][Hh][Mm]) shm=off ;;
        "+extension "[Mm][Ii][Tt]-[Ss][Hh][Mm]) shm=on ;;
      esac
      prev=$arg
    done < "/proc/$pid/cmdline"; } 2>/dev/null || true
  if [ -n "$rootless" ]; then
    echo "$comm (PID $pid) runs -rootless, as a session Xwayland does; start a rootful one"
    return
  fi
  if [ "$shm" = on ]; then
    echo "$comm (PID $pid) has MIT-SHM on, so it would attach your SysV shared memory for the client: start it with -extension MIT-SHM"
    return
  fi
  # No font-server support (see "Display"): every libXfont the server maps, each file once. maps
  # shows a file's path as it is now, marked " (deleted)" once that name no longer leads to it.
  # A maps that can't be read lists none, which refuses.
  { while read -r _ _ _ _ _ path; do
      case "${path##*/}" in libXfont*) [ "$path" = "$last" ] || libs+=("$path"); last=$path ;; esac
    done < "/proc/$pid/maps"; } 2>/dev/null || true
  if [ "${#libs[@]}" -eq 0 ]; then
    echo "$comm (PID $pid) maps no libXfont (or /proc/$pid/maps can't be read), so its font library can't be checked for font-server support"
    return
  fi
  for f in "${libs[@]}"; do
    case "$f" in
      *" (deleted)")
        echo "$comm (PID $pid) maps $f, a libXfont deleted or replaced since it started, which can't be checked: restart the server"
        return ;;
    esac
    if [ ! -f "$f" ] || [ ! -r "$f" ]; then
      echo "$comm (PID $pid) maps $f, which is not a readable file here, so it can't be checked for font-server support"
      return
    fi
    rc=0
    LC_ALL=C grep -qaF -e getaddrinfo -e gethostbyname -- "$f" || rc=$?
    case "$rc" in
      0) echo "$comm (PID $pid) maps $f, a libXfont with font-server support (it looks up host names):" \
              "any client could make the server connect anywhere. Use a libXfont2 built without it," \
              "as Fedora's is since 2.0.8-3"
         return ;;
      1) ;;
      *) echo "grep failed (exit $rc) on $f, which $comm (PID $pid) maps, so it can't be checked for font-server support"
         return ;;
    esac
  done
}
if [ -n "$x11" ]; then
  if [ -n "${DISPLAY:-}" ]; then
    d=${DISPLAY##*:}                    # [protocol/][host]:D[.S] -> D
    d=${d%%.*}
    if [[ $DISPLAY != *:* || -z $d || $d == *[!0123456789]* ]]; then
      die2 "--x11: DISPLAY=$DISPLAY doesn't name a display number, so $x11 can't be checked against your session's display (fix or unset DISPLAY)"
    fi
    while [[ $d == 0?* ]]; do d=${d#0}; done
    [ "$d" != "$x11" ] || die2 "--x11 $x11 is your session's display (DISPLAY=$DISPLAY): start a nested X server for the run (see the header)"
  fi
  uid=$(id -u)
  xsock=/tmp/.X11-unix/X$x11 xlock=/tmp/.X$x11-lock
  # stat without -L describes the entry itself, so a symlink is refused rather than followed.
  # The directory first, as X servers want it: a real one, root's or yours, sticky. Otherwise
  # someone else could swap XN after these checks (bwrap's bind follows symlinks).
  st=$(stat -c '%u %f' -- /tmp/.X11-unix 2>/dev/null) || die2 "--x11 $x11: no X server socket at $xsock"
  (( (16#${st#* } & 8#170000) == 8#040000 )) ||
    die2 "--x11 $x11: /tmp/.X11-unix is not a directory (a symlink is refused too)"
  [ "${st% *}" = 0 ] || [ "${st% *}" = "$uid" ] ||
    die2 "--x11 $x11: /tmp/.X11-unix belongs to uid ${st% *}, not root or you"
  (( 16#${st#* } & 8#1000 )) ||
    die2 "--x11 $x11: /tmp/.X11-unix doesn't have the sticky bit (X servers make it 1777), so anyone who can write to it could replace XN"
  st=$(stat -c '%u %f' -- "$xsock" 2>/dev/null) || die2 "--x11 $x11: no X server socket at $xsock"
  (( (16#${st#* } & 8#170000) == 8#140000 )) || die2 "--x11 $x11: $xsock is not a socket (a symlink is refused too)"
  [ "${st% *}" = "$uid" ] || die2 "--x11 $x11: $xsock belongs to uid ${st% *}, not you (another user's X server)"
  st=$(stat -c '%u %f' -- "$xlock" 2>/dev/null) ||
    die2 "--x11 $x11: no lock file $xlock, which names the server (don't start it with -nolock)"
  if (( (16#${st#* } & 8#170000) != 8#100000 )) || [ "${st% *}" != "$uid" ]; then
    die2 "--x11 $x11: $xlock is not a regular file of yours"
  fi
  why=$(x11_server_problem "$xlock")
  [ -z "$why" ] || die2 "--x11 $x11: $why"
fi

args=(
  --unshare-all --unshare-user --unshare-net    # user ns and net ns are mandatory, not "-try"
)
if [ -n "$noroot" ]; then
  args+=(--uid "$caller_uid" --gid "$caller_gid")   # and no --cap-add: no capabilities at all
else
  args+=(--uid 0 --gid 0 --cap-add CAP_NET_BIND_SERVICE --cap-add CAP_SETFCAP)
fi
args+=(
  --die-with-parent --new-session
  --seccomp 7                                   # fd 7: seccomp-deny-keys.py's filter (exec line)
  --hostname isolated-run
  --clearenv
)

# Filesystem: empty root, then only what is listed here.
args+=(--ro-bind /usr /usr)
for l in bin sbin lib lib32 lib64 libx32; do
  if [ -L "/$l" ]; then args+=(--symlink "$(readlink -- "/$l")" "/$l")
  elif [ -d "/$l" ]; then args+=(--ro-bind "/$l" "/$l"); fi
done
args+=(
  --proc /proc --dev /dev
  --perms 1777 --tmpfs /dev/shm
  --perms 1777 --tmpfs /tmp
  --tmpfs /run --tmpfs /var --symlink ../run /var/run --perms 1777 --dir /var/tmp
  --tmpfs "$home"
  --dir /etc
)
for e in ld.so.cache ld.so.conf ld.so.conf.d alternatives ssl pki crypto-policies localtime \
         host.conf services protocols fonts os-release vulkan glvnd egl mime.types; do
  if [ -L "/etc/$e" ]; then args+=(--symlink "$(readlink -- "/etc/$e")" "/etc/$e")
  elif [ -e "/etc/$e" ]; then args+=(--ro-bind "/etc/$e" "/etc/$e"); fi
done

# Caller's binds, at the same path inside, after the tmpfs mounts so they can sit inside them.
for ((i = 0; i < ${#binds[@]}; i += 2)); do
  args+=("${binds[$i]}" "${binds[$((i + 1))]}" "${binds[$((i + 1))]}")
done

# --x11 N: that one socket, alone in a fresh /tmp/.X11-unix, and DISPLAY (set after --clearenv).
# Read-only: connect() works on it, but nothing inside can chmod or touch the host's socket.
if [ -n "$x11" ]; then
  args+=(--dir /tmp/.X11-unix --ro-bind "$xsock" "$xsock" --setenv DISPLAY ":$x11")
fi

# --cpu-topology: the host's /sys/devices/system/cpu (checked above: on sysfs, nothing mounted
# below it), read-only, and nothing else of /sys (Wine and Proton build their processor
# information from it). After the caller's binds, so none can sit on top of it.
if [ -n "$cputopo" ]; then
  args+=(--ro-bind /sys/devices/system/cpu /sys/devices/system/cpu)
fi

# Generated /etc files go last so no bind can shadow them (fds 3-6 are opened on the exec line).
args+=(--perms 0644 --ro-bind-data 3 /etc/hosts --perms 0644 --ro-bind-data 4 /etc/nsswitch.conf
       --perms 0644 --ro-bind-data 5 /etc/passwd --perms 0644 --ro-bind-data 6 /etc/group)

# Working dir: the caller's, if it is inside a bound dir; else /.
cwd=$(pwd -P) chdir=/
for ((i = 1; i < ${#binds[@]}; i += 2)); do
  case "$cwd/" in "${binds[$i]%/}/"*) chdir=$cwd; break ;; esac
done
args+=(--chdir "$chdir")

# Environment allowlist.
args+=(--setenv HOME "$home" --setenv BROWSER /bin/false)
while IFS= read -r v; do
  case "$v" in PATH|TERM|LANG|LANGUAGE|USER|LC_*) args+=(--setenv "$v" "${!v}") ;; esac
done < <(compgen -e)

hosts_data="127.0.0.1 localhost localhost.localdomain isolated-run
::1 localhost localhost.localdomain ip6-localhost ip6-loopback"
[ -n "$hosts" ] && hosts_data+=$'\n'$(cat -- "$hosts")
nss_data="passwd: files
shadow: files
group: files
hosts: files
networks: files
protocols: files
services: files
ethers: files
rpc: files
netgroup: files"
passwd_data="root:x:0:0:root:$home:/bin/bash
nobody:x:65534:65534:Kernel Overflow User:/:/sbin/nologin"
group_data="root:x:0:
nobody:x:65534:"
if [ -n "$noroot" ]; then
  passwd_data+=$'\n'"$caller_user:x:$caller_uid:$caller_gid::$home:/bin/bash"
  group_data+=$'\n'"$caller_group:x:$caller_gid:"
fi

if ! { bpf=$(python3 -I "$here/seccomp-deny-keys.py" --printf) && [ -n "$bpf" ]; }; then
  die2 "couldn't build the seccomp filter"
fi
ulimit -S -c 0
ulimit -H -c 0

# Close every inherited fd above 2 that would survive exec (an open socket handed to us by the
# caller would otherwise be a way out). Close-on-exec fds, like bash's own, go away by themselves.
for f in /proc/self/fd/*; do
  fd=${f##*/}
  [ "$fd" -gt 2 ] 2>/dev/null || continue
  flags=""
  { while read -r k val; do if [ "$k" = flags: ]; then flags=$val; fi; done < "/proc/self/fdinfo/$fd"; } 2>/dev/null || true
  if [ -z "$flags" ] || (( (8#$flags & 8#2000000) == 0 )); then eval "exec $fd>&-"; fi
done

# A socket on stdin/out/err would connect the command to whatever holds its other end.
for n in 0 1 2; do
  if [ -S "/proc/self/fd/$n" ]; then
    name=(stdin stdout stderr)
    echo "isolated-run: ${name[$n]} is a socket; replacing it with /dev/null inside" >&2
    if [ "$n" -eq 0 ]; then exec 0</dev/null; else eval "exec $n>/dev/null"; fi
  fi
done

exec bwrap "${args[@]}" -- "$@" \
  3<<<"$hosts_data" 4<<<"$nss_data" 5<<<"$passwd_data" 6<<<"$group_data" 7< <(printf '%b' "$bpf")
