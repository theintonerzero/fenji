#!/bin/sh
# Updates the Fenji server beside this script to the latest release, or to the version given,
# keeping its settings and data (docs/hosting.md, section 5; the hosting spec's Amendment B):
#
#   ./update-server.sh [<version>]   in a server's folder on your own computer, once it has stopped
#   sudo fenji-update [<version>]    on the reference VPS, where section 4 installs it under that name
#
# It updates where it is (HB2): the systemd service named fenji, if that service runs the
# fenji-server beside this script, as on the reference VPS; otherwise this folder. Nothing changes
# until the new version's archive matches the release's SHA256SUMS and its program runs and reports
# that version (HB4). The database is backed up first (HB6), and what it replaces in a service stays
# as <name>.previous: the program, and the units (HB7). It never touches fenji.toml, the data
# folder, systemd's drop-ins or Caddy's own settings (HB8); in a folder it replaces the archive's
# other files, as the steps by hand do.
#
# A service's units are replaced only when the service is Fenji's own unit, which runs the
# archive's fenji.service's ExecStart as its User, and then only Fenji's three units, by name, as
# root's files with root's modes. As root, it writes only in folders that are root's and that only
# root may write in, below folders that are root's too, all the way up, each writable by root alone
# or sticky. It runs no fenji-server as root to check it: in a service, as the service's
# user, or as nobody for a service that runs as root, whose new server's console it does ask as
# root, once systemd has started that server as root; in a folder, as the folder's owner.
#
# It trusts a release only once its SHA256SUMS is signed with Fenji's release key, whose public half
# it carries (the hosting spec's Amendment D, HD1, HD3): it downloads over HTTPS from the
# repository's GitHub release, checks SHA256SUMS.sig with ssh-keygen -Y verify, then checks the
# archive against SHA256SUMS. FENJI_RELEASE_KEY names a file holding a fork's allowed-signers line
# (HD4).
#
# Releases come from https://github.com/theintonerzero/fenji/releases, or from FENJI_RELEASES, for a
# fork. It needs curl, tar, ssh-keygen from OpenSSH 8 or later, and sha256sum or macOS's shasum;
# for a service, systemctl, sudo and journalctl too, and timeout where there is one.
set -eu
# A cd to a relative path goes where it says, not to a folder an exported CDPATH names.
unset CDPATH
export LC_ALL=C
# What it makes starts as this user's alone, and gets its final mode only once it's whole.
umask 077

me=${0##*/}
# The release key's public half, as an OpenSSH allowed-signers line (HD1): deploy/allowed_signers
# and section 5 of docs/hosting.md hold the same line, and a test pins that they agree. None until
# the owner makes the key, before 0.5.4 is tagged (HD5); scripts/set-release-key.sh puts it in.
RELEASE_KEY=''
# Fenji's own units, the only ones a service's update installs.
UNITS='fenji.service fenji-backup.service fenji-backup.timer'

# say LINE: one step's line. It never fails: a terminal gone with its SSH session mustn't stop an
# update halfway.
say() { printf '%s\n' "$*" 2> /dev/null || :; }

# fail MESSAGE [STATUS]: says MESSAGE, and exits with STATUS, 1 by default.
fail() {
  printf '%s: %s\n' "$me" "$1" >&2 2> /dev/null || :
  exit "${2:-1}"
}

usage() {
  cat <<EOF
usage: $me [<version>]
Updates the Fenji server beside this script to the latest release, or to <version>, such as 0.5.3,
keeping its settings and data. In a server's folder, stop the server first. For the fenji service on
a VPS, run it as root, with sudo. docs/hosting.md, section 5, says more.
EOF
}

# is_version WORD: whether WORD is a version, such as 0.5.3 or 0.6.0-rc.1, without the tag's v.
is_version() {
  case $1 in '' | *[!0-9A-Za-z.-]*) return 1 ;; esac
  printf '%s\n' "$1" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$'
}

# compare A B: -1, 0 or 1 as version A comes before B, is B, or comes after it, in SemVer 2.0.0's
# order: numbers as numbers, and a pre-release such as 1.3.0-rc.1 before its release.
compare() {
  awk -v a="$1" -v b="$2" '
    # Two identifiers of pre-releases: numbers as numbers, before words, which go as text.
    function id(x, y,    xn, yn) {
      xn = x ~ /^[0-9]+$/
      yn = y ~ /^[0-9]+$/
      if (xn && yn) return ((x + 0 < y + 0) ? -1 : (x + 0 > y + 0))
      if (xn != yn) return (xn ? -1 : 1)
      return ((x < y) ? -1 : (x > y))
    }
    BEGIN {
      pa = ""
      pb = ""
      if ((i = index(a, "-")) > 0) { pa = substr(a, i + 1); a = substr(a, 1, i - 1) }
      if ((i = index(b, "-")) > 0) { pb = substr(b, i + 1); b = substr(b, 1, i - 1) }
      split(a, x, ".")
      split(b, y, ".")
      for (i = 1; i <= 3; i++) if (x[i] + 0 != y[i] + 0) { print ((x[i] + 0 < y[i] + 0) ? -1 : 1); exit }
      if (pa == pb) { print 0; exit }
      if (pa == "" || pb == "") { print ((pa == "") ? 1 : -1); exit }
      n = split(pa, x, ".")
      m = split(pb, y, ".")
      for (i = 1; i <= n && i <= m; i++) if ((c = id(x[i], y[i])) != 0) { print c; exit }
      print ((n < m) ? -1 : (n > m))
    }'
}

# said_version PROGRAM: what PROGRAM --version prints. In a service it runs as $checker, never as
# root; in a folder, as the folder's owner, who runs this script.
said_version() {
  if [ "$service" = yes ]; then
    (cd / && sudo -u "$checker" -- "$1" --version) 2> /dev/null
  else
    "$1" --version 2> /dev/null
  fi
}

# version_of PROGRAM: the version that PROGRAM --version reports, which says "fenji-server 0.5.3".
version_of() {
  said=$(said_version "$1") || return 1
  said=${said##* }
  is_version "$said" || return 1
  printf '%s\n' "$said"
}

# config_of WORD...: the --config of the server's command line, ExecStart's words; or nothing.
config_of() {
  while [ $# -gt 0 ]; do
    case $1 in
      --config) [ $# -lt 2 ] || printf '%s\n' "$2"; return 0 ;;
      --config=*) printf '%s\n' "${1#--config=}"; return 0 ;;
    esac
    shift
  done
}

# squeezed TEXT: TEXT with each run of spaces and tabs one space, and none at either end.
squeezed() { printf '%s\n' "$1" | tr -s ' \t' '  ' | sed 's/^ //; s/ $//'; }

# unit_value KEY FILE: the value FILE's last KEY= line gives, squeezed.
unit_value() { squeezed "$(sed -n "s/^$1=//p" "$2" | tail -n 1)"; }

# fetch URL FILE: URL into FILE, over HTTPS alone, redirects too, failing on an HTTP error, and with
# no ~/.curlrc of anyone's (-q) to say otherwise.
fetch() { curl -q -fsSL --proto '=https' --proto-redir '=https' -o "$2" "$1"; }

# sum256: the SHA-256 of stdin, by sha256sum, or by shasum on macOS.
sum256() {
  if command -v sha256sum > /dev/null 2>&1; then
    sha256sum
  elif command -v shasum > /dev/null 2>&1; then
    shasum -a 256
  else
    return 1
  fi
}

# quoted TEXT: TEXT as one shell word in single quotes, any single quote in it escaped, so that a
# command a host pastes stays on the path it names.
quoted() { printf "'%s'" "$(printf '%s\n' "$1" | sed "s/'/'\\\\''/g")"; }

# unsafe_for_root DIR: the first folder that makes DIR unsafe for root to write in, from DIR, a full
# path with its links resolved, up to /; nothing if there's none. DIR must be root's, and only root
# may write in it. Each folder above it must be root's, and either only root may write in it or
# it's sticky, as /tmp is: anyone else who may write in a folder could rename DIR, or a folder on
# the way to it, aside, and put their own in its place, between the checks and the writes.
unsafe_for_root() {
  # The walk up ends at /, so it needs a full path: taking the last part off a relative one would
  # never get there. Every caller passes a path pwd -P made, or one below it; any other is unsafe.
  case $1 in /*) ;; *) printf '%s\n' "$1"; return 0 ;; esac
  if [ ! -d "$1" ] || [ -n "$(find "$1" -prune \( ! -user 0 -o -perm -020 -o -perm -002 \) -print)" ]; then
    printf '%s\n' "$1"
    return 0
  fi
  up=$1
  while [ "$up" != / ]; do
    up=${up%/*}
    [ -n "$up" ] || up=/
    if [ -n "$(find "$up" -prune \( ! -user 0 -o \( \( -perm -020 -o -perm -002 \) ! -perm -1000 \) \) -print)" ]; then
      printf '%s\n' "$up"
      return 0
    fi
  done
}

# guard DIR: as root, refuses to write in DIR unless it, and every folder above it, is safe for
# that (unsafe_for_root), so that nobody else can put a link, or another program, where root writes.
# The advice fits what's wrong: DIR itself, or a folder above it that's root's, can be made safe
# where it is; a folder above that isn't root's, such as a home folder, must stay its user's, so
# the server's files move, or a folder's server goes to that user.
guard() {
  [ "$uid" = 0 ] || return 0
  culprit=$(unsafe_for_root "$1")
  [ -n "$culprit" ] || return 0
  # ls shows owner and mode on Linux and macOS alike, the mode with an SELinux or ACL mark at its end.
  # shellcheck disable=SC2012
  is=$(ls -ld -- "$culprit" 2> /dev/null | awk '{ m = $1; sub(/[.+@]$/, "", m); print $3 "'"'"'s, with mode " m }')
  if [ "$culprit" = "$1" ]; then
    fail "won't write as root in $1, which is ${is:-missing}: a folder root writes in must be root's, and only root may write in it. Make it so (sudo chown root $(quoted "$1") && sudo chmod go-w $(quoted "$1")), or update by hand (docs/hosting.md, section 5)"
  fi
  why="won't write as root in $1: $culprit, above it, is ${is:-missing}, so someone else could move $1 aside and put their own in its place. Each folder above one root writes in must be root's, and either only root may write in it or it's sticky, as /tmp is."
  if [ -z "$(find "$culprit" -prune ! -user 0 -print)" ]; then
    fail "$why Make it writable by root alone (sudo chmod go-w $(quoted "$culprit")), or update by hand (docs/hosting.md, section 5)"
  fi
  if [ "$service" = yes ]; then
    fail "$why Keep the service's program and units in folders that are root's all the way up to /, as /usr/local/bin and /etc/systemd/system are, or update by hand (docs/hosting.md, section 5)"
  fi
  # shellcheck disable=SC2012
  whose=$(ls -ld -- "$culprit" | awk '{ print $3 }')
  fail "$why Move the server's folder into one that's root's all the way up to /, or give it to $whose (sudo chown -R $(quoted "$whose") $(quoted "$here")) and run this as $whose, without sudo"
}

# writes DIR: refuses a folder DIR this user can't write in, before anything changes.
writes() { [ -w "$1" ] || fail "can't write in $1, so nothing has changed"; }

# into PATH: refuses a PATH the swap couldn't rename a file over: a folder.
into() {
  if [ -d "$1" ]; then fail "$1 is a folder, where the new version has a file, so nothing has changed"; fi
}

# stage_in FOLDER: sets $sd to a new staging folder in FOLDER, this user's alone, with a name nobody
# can guess, for what goes into FOLDER: the swap's renames then stay on one file system. Cleanup
# takes it away.
stage_in() {
  sd=$(mktemp -d "$1/.fenji-update.XXXXXX") || fail "can't make a staging folder in $1, so nothing has changed"
  printf '%s\n' "$sd" >> "$tmp/stages"
}

# put FILE STAGED MODE: FILE, copied to STAGED, a new path in a staging folder, with MODE. Nothing is
# ever copied into a path that's there already.
put() {
  if [ -e "$2" ] || [ -L "$2" ]; then fail "$2 is there already, so nothing has changed"; fi
  { cp -- "$1" "$2" && chmod "$3" "$2"; } || fail "can't copy the new ${1##*/} into place, so nothing has changed"
}

# keep PATH STAGED: PATH, as it is, at STAGED, a new path in a staging folder: a hard link where it
# can be, a copy where it can't.
keep() {
  ln -- "$1" "$2" 2> /dev/null || cp -p -- "$1" "$2" || fail "can't keep the old ${1##*/}, so nothing has changed"
}

# swap STAGED PATH: renames STAGED over PATH. Where STAGED is already PATH's file, as a kept program
# is when its .previous is a link to it, STAGED just goes.
swap() {
  # shellcheck disable=SC3013 # -ef: dash's, bash's and busybox's test all have it
  if [ "$1" -ef "$2" ]; then rm -f -- "$1"; else mv -f -- "$1" "$2"; fi
}

# installs PATH: whether the archive's PATH is among what a folder's update replaces: all but the
# program, which goes in on its own, and never fenji.toml or the data folder (HB8).
installs() {
  case $1 in . | fenji-server | fenji.toml | data | data/*) return 1 ;; esac
}

# listed: what a folder's update replaced, the archive's top-level names, as a list.
listed() {
  while IFS= read -r f; do
    f=${f#./}
    installs "$f" || continue
    case $f in */*) printf '%s/\n' "${f%%/*}" ;; *) printf '%s\n' "$f" ;; esac
  done < "$tmp/files" | uniq | awk '{ printf "%s%s", (NR > 1 ? ", " : ""), $0 }'
}

# as_server [-t] ARG...: the service's program with ARGs, as the service's user and with its
# --config, where systemd runs the server; with -t, for 5 seconds at most.
as_server() {
  bound=''
  if [ "$1" = -t ]; then bound=yes; shift; fi
  if [ -n "$config" ]; then set -- --config "$config" "$@"; fi
  if [ -n "$user" ] && [ "$user" != root ]; then
    set -- sudo -u "$user" -- "$prog" "$@"
  else
    set -- "$prog" "$@"
  fi
  if [ -n "$bound" ] && command -v timeout > /dev/null 2>&1; then set -- timeout 5 "$@"; fi
  (cd "$rundir" && "$@")
}

# go_back: how to go back to the version this replaced: the previous program and units, and the
# backup made first.
go_back() {
  printf 'To go back to %s, stop it, and put the previous program back' "$current"
  if [ -n "$replaced" ]; then printf ', and the previous units'; fi
  printf ':\n  sudo systemctl stop fenji\n  sudo mv %s %s\n' "$(quoted "$prog.previous")" "$(quoted "$prog")"
  for u in $replaced; do
    printf '  sudo mv %s %s\n' "$(quoted "$unitdir/$u.previous")" "$(quoted "$unitdir/$u")"
  done
  if [ -n "$replaced" ]; then printf '  sudo systemctl daemon-reload\n'; fi
  printf 'Then restore the backup made before this update, as docs/hosting.md section 6 says:\n'
  printf '  %s\n' "${backup:-(none was made)}"
  printf 'and start it again: sudo systemctl start fenji\n'
}

# gone UP: the service didn't come up (HB9): its journal's last lines and how to go back. UP is yes
# when systemd had it active at every look, so that it runs, but its console never answered.
gone() {
  {
    if [ "$1" = yes ]; then
      printf "%s: fenji is active, but its console didn't answer within 30 seconds, so this can't tell whether the server came up. Its journal's last lines:\n" "$me"
    else
      printf "%s: fenji didn't come up within 30 seconds. Its journal's last lines:\n" "$me"
    fi
    journalctl -u fenji -n 20 --no-pager 2>&1 || :
    if [ "$1" = yes ]; then
      printf 'If players can connect, it works, and its console may have no socket, as its log says. If they can'\''t:\n'
    fi
    go_back
  } >&2 2> /dev/null || :
  exit 1
}

# cleanup: on every exit, takes away the staging folders, with whatever is still in them, the
# folders made for the swap if they're empty, the lock and the download.
tmp='' locked=''
cleanup() {
  if [ -n "$tmp" ] && [ -f "$tmp/stages" ]; then
    while IFS= read -r d; do
      case $d in */.fenji-update.?*) rm -rf -- "$d" ;; esac
    done < "$tmp/stages"
  fi
  if [ -n "$tmp" ] && [ -f "$tmp/made" ]; then
    sort -r -- "$tmp/made" | while IFS= read -r d; do rmdir -- "$d" 2> /dev/null || :; done
  fi
  if [ -n "$locked" ]; then rmdir -- "$lock" 2> /dev/null || :; fi
  if [ -n "$tmp" ]; then rm -rf -- "$tmp"; fi
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

case ${1-} in
  -h | --help) usage; exit 0 ;;
  -*) usage >&2; exit 2 ;;
esac
[ $# -le 1 ] || { usage >&2; exit 2; }
wanted=''
if [ $# -eq 1 ]; then
  wanted=${1#v}
  is_version "$wanted" || fail "not a version: $1 (a version is like 0.5.3)" 2
fi

releases=${FENJI_RELEASES:-https://github.com/theintonerzero/fenji/releases}
releases=${releases%/}
case $releases in
  https://?*) ;;
  *) fail "FENJI_RELEASES is $releases, but releases come over HTTPS alone: give an https:// address, such as https://github.com/theintonerzero/fenji/releases" 2 ;;
esac

here=$(cd -P -- "$(dirname -- "$0")" && pwd -P) || fail "can't find the folder this script is in"
self=$here/${0##*/}
prog=$here/fenji-server
if ! { [ -f "$prog" ] && [ -x "$prog" ]; }; then
  fail "there's no fenji-server beside this script, in $here: run the update script that came with your server, in its folder"
fi

# The archive for this computer, and the start script it holds.
system=$(uname -s)
machine=$(uname -m)
case $system:$machine in
  Linux:x86_64 | Linux:amd64) target='x86_64-unknown-linux-musl' start='start-server.sh' ;;
  Linux:aarch64 | Linux:arm64) target='aarch64-unknown-linux-musl' start='start-server.sh' ;;
  Darwin:arm64 | Darwin:aarch64) target='aarch64-apple-darwin' start='start-server.command' ;;
  *) fail "there's no Fenji server for $system on $machine: the releases have one for Linux on x86_64 or 64-bit ARM, one for macOS on Apple silicon, and one for Windows, which updates by hand (docs/hosting.md, section 5)" ;;
esac

# The fenji service, if it runs the program beside this script, or a link to it (HB2), from what
# systemd says of it: its ExecStart's command line, its User, its working folder and its units'.
service=no user='' argv='' config='' state='' unitdir=/etc/systemd/system workdir='' shown=''
if command -v systemctl > /dev/null 2>&1; then
  shown=$(systemctl show --property=LoadState,ActiveState,User,ExecStart,FragmentPath,WorkingDirectory fenji 2> /dev/null) ||
    shown=''
fi
prop() { printf '%s\n' "$shown" | sed -n "s/^$1=//p" | head -n 1; }
if [ "$(prop LoadState)" = loaded ]; then
  # ExecStart={ path=/usr/local/bin/fenji-server ; argv[]=/usr/local/bin/fenji-server --config … run ; … }
  line=$(prop ExecStart)
  path=$(printf '%s\n' "$line" | sed -n 's/^{ path=\(.*\) ; argv\[\]=.*$/\1/p')
  # shellcheck disable=SC3013 # -ef: dash's, bash's and busybox's test all have it
  if [ -n "$path" ] && [ "$path" -ef "$prog" ]; then
    service=yes
    user=$(prop User)
    state=$(prop ActiveState)
    argv=$(squeezed "$(printf '%s\n' "$line" | sed -n 's/^.* ; argv\[\]=\(.*\) ; ignore_errors=.*$/\1/p')")
    # The words as systemd shows them, split at spaces: the reference unit's have none of their own.
    set -f
    # shellcheck disable=SC2086
    config=$(config_of $argv)
    set +f
    fragment=$(prop FragmentPath)
    [ -z "$fragment" ] || unitdir=${fragment%/*}
    unitdir=$(cd -P -- "$unitdir" 2> /dev/null && pwd -P) || unitdir=${fragment%/*}
    workdir=$(prop WorkingDirectory)
    workdir=${workdir#!}
  fi
fi

# Who runs it (HB8): root for a service, which writes in the program's folder and the units'; the
# folder's owner for a folder, so that the files it puts there stay theirs. Root writes only in
# folders that are root's and that only root may write in.
uid=$(id -u)
running=no checker='' rundir=/
if [ "$service" = yes ]; then
  [ "$uid" = 0 ] || fail "the fenji service runs $prog, and updating it takes root: run sudo $(quoted "$self")"
  guard "$here"
  # Who runs the programs' --version, never root: the service's user, or nobody for a service
  # that runs as root.
  checker=$user
  if [ -z "$checker" ] || [ "$checker" = root ]; then checker=nobody; fi
  command -v sudo > /dev/null 2>&1 ||
    fail "updating the fenji service takes sudo, to run its programs as $checker rather than root: install it (apt install sudo)"
  case $state in
    active) running=yes ;;
    inactive | failed) running=no ;;
    *) fail "the fenji service is $state: run this again once it's active, or stopped" ;;
  esac
  # The service's commands run where systemd runs the server, which matters for a --config that
  # isn't a full path: its WorkingDirectory, or /.
  case $config in
    /*) ;;
    *)
      case $workdir in
        '') ;;
        /*) rundir=$workdir ;;
        *) fail "the fenji service reads ${config:-fenji.toml} in its WorkingDirectory, $workdir, which this can't follow: give its ExecStart a --config with a full path" ;;
      esac ;;
  esac
else
  # A VPS's fenji-update, run without sudo, may find no service: systemd answers only root where
  # there's no D-Bus.
  if [ ! -f "$here/$start" ] && [ "$uid" != 0 ]; then
    fail "$here has no $start, so it isn't a server's folder: to update the fenji service on a VPS, run it as root: sudo $(quoted "$self"); on your own computer, run the update script in your server's folder (docs/hosting.md, section 5)"
  elif [ ! -f "$here/$start" ]; then
    fail "$here has no $start, so it isn't a server's folder, and no fenji service runs its fenji-server: update it by hand (docs/hosting.md, section 5)"
  fi
  if [ -z "$(find "$here" -prune -user "$uid" -print)" ]; then
    # shellcheck disable=SC2012 # ls shows the owner's name on Linux and macOS alike
    name=$(ls -ld -- "$here" | awk '{ print $3 }')
    if [ "$uid" = 0 ]; then
      fail "$here is $name's, and this runs as root: run it as $name, without sudo, so that the files it puts there stay $name's"
    fi
    fail "$here is $name's: run it as $name"
  fi
  guard "$here"
fi

# Which version (HB3): the latest release, from where releases/latest leads, or the one named.
current=$(version_of "$prog") ||
  fail "$prog doesn't run, or doesn't say its version: update it by hand (docs/hosting.md, section 5)"
if [ -z "$wanted" ]; then
  landed=$(curl -q -fsSL --proto '=https' --proto-redir '=https' -o /dev/null -w '%{url_effective}' "$releases/latest") ||
    fail "can't reach $releases/latest, to find the latest release"
  wanted=${landed##*/tag/v}
  if [ "$wanted" = "$landed" ] || ! is_version "$wanted"; then
    fail "can't tell the latest release from where $releases/latest leads: ${landed:-nowhere}"
  fi
  say "The latest release is $wanted."
fi
order=$(compare "$wanted" "$current")
case $order in
  1) ;;
  0)
    if [ $# -eq 0 ]; then
      say "This server runs $current, the latest release: there's nothing to update."
    else
      say "This server runs $current already: there's nothing to update."
    fi
    exit 0 ;;
  -1) fail "$wanted is older than $current, which this server runs, and an update never goes back, as the database may have moved on since: to go back to $wanted, restore a backup made with $wanted, with that version's program (docs/hosting.md, section 6)" ;;
  *) fail "can't tell whether $wanted comes after $current, so nothing has changed" ;;
esac

# A server running from a folder runs in a terminal or in tmux, where it can't be started again
# from here (HB5).
if [ "$service" = no ] && (cd "$here" && ./fenji-server console --run online) > /dev/null 2>&1; then
  fail "a server is running from $here: stop it first: type stop in its console, then run $me again"
fi

# One update at a time: a second's swap would cross the first's.
writes "$here"
lock=$here/.fenji-update.lock
if ! mkdir -- "$lock" 2> /dev/null; then
  if [ -d "$lock" ]; then
    fail "another update of this server is running, or one was cut short: if none is running, remove its lock (rmdir $(quoted "$lock")), and run this again"
  fi
  fail "can't make $lock, so nothing has changed"
fi
locked=yes

if [ "$service" = yes ]; then
  say "Updating the fenji service, which runs $prog, from $current to $wanted."
else
  say "Updating the server in $here from $current to $wanted."
fi

# The download, checked (HB4): nothing changes until it matches SHA256SUMS, and its program runs and
# reports its version.
tmp=$(mktemp -d "${TMPDIR:-/tmp}/fenji-update.XXXXXX") || fail "can't make a temporary folder for the download"
: > "$tmp/stages"
: > "$tmp/made"
archive=fenji-server-$wanted-$target.tar.gz
from=$releases/download/v$wanted
say "Downloading $archive, with its release's SHA256SUMS."
fetch "$from/$archive" "$tmp/$archive" ||
  fail "can't download $from/$archive: is $wanted a release, with a server for $target? Nothing has changed"
fetch "$from/SHA256SUMS" "$tmp/SHA256SUMS" || fail "can't download $from/SHA256SUMS, so nothing has changed"
# The sums count only once their signature is the release key's (HD3).
fetch "$from/SHA256SUMS.sig" "$tmp/SHA256SUMS.sig" ||
  fail "can't download $from/SHA256SUMS.sig: the release isn't signed, so it can't be trusted, and nothing has changed"
if [ -n "${FENJI_RELEASE_KEY:-}" ]; then
  allowed=$FENJI_RELEASE_KEY
else
  [ -n "$RELEASE_KEY" ] ||
    fail "this update script holds no release key, so it can't check a release; update by hand (docs/hosting.md, section 5). Nothing has changed"
  allowed=$tmp/allowed_signers
  printf '%s\n' "$RELEASE_KEY" > "$allowed"
fi
# Only the line given, never an agent's key, and nothing to answer.
if why=$(env -u SSH_AUTH_SOCK ssh-keygen -Y verify -f "$allowed" -I fenji-release -n fenji-release \
  -s "$tmp/SHA256SUMS.sig" < "$tmp/SHA256SUMS" 2>&1); then
  say "Its SHA256SUMS is signed with the release key."
else
  case $?:$why in
    127:*) fail "checking the release's signature needs ssh-keygen, from OpenSSH 8 or later: install OpenSSH, then try again. Nothing has changed" ;;
    *"unknown option"* | *"illegal option"* | *"invalid option"*)
      fail "this ssh-keygen can't check signatures: it needs OpenSSH 8 or later. Nothing has changed" ;;
    *) fail "SHA256SUMS isn't signed with the release key, so this release can't be trusted, and nothing has changed" ;;
  esac
fi
found=$(awk -v f="$archive" '$2 == f || $2 == "*" f { n++; s = $1 } END { print n + 0, s }' "$tmp/SHA256SUMS")
case ${found%% *} in
  1) expected=${found#* } ;;
  0) fail "SHA256SUMS has no line for $archive, so it can't be checked, and nothing has changed" ;;
  *) fail "SHA256SUMS has ${found%% *} lines for $archive, so it can't be checked, and nothing has changed" ;;
esac
actual=$(sum256 < "$tmp/$archive") || fail "can't take the download's checksum: this needs sha256sum, or shasum on macOS"
actual=${actual%% *}
if [ "$actual" != "$expected" ]; then
  fail "$archive doesn't match SHA256SUMS: the download is damaged or incomplete, so nothing has changed; try again"
fi
say "It matches SHA256SUMS."
mkdir -- "$tmp/new"
tar -xzf "$tmp/$archive" -C "$tmp/new" || fail "can't unpack $archive, so nothing has changed"
new=$tmp/new/fenji-server-$wanted-$target
[ -f "$new/fenji-server" ] || fail "$archive has no fenji-server-$wanted-$target/fenji-server, so nothing has changed"

# The new program waits beside the old one, in a staging folder, where it's run to check it: the
# temporary folder may not let programs run, nor a service's user reach them. That user may pass
# through the staging folder, but not see into it or write there.
stage_in "$here"
pstage=$sd
if [ "$service" = yes ]; then chmod 0711 "$pstage"; fi
next=$pstage/fenji-server
into "$prog"
put "$new/fenji-server" "$next" 0755
said=$(said_version "$next") || fail "the new fenji-server doesn't run on this computer, so nothing has changed"
said=${said##* }
[ "$said" = "$wanted" ] || fail "the new fenji-server says it's ${said:-no version}, not $wanted, so nothing has changed"
say "The new fenji-server runs, and reports $wanted."
# The old program, as fenji-server.previous will keep it (HB7).
kept=$pstage/fenji-server.previous
into "$prog.previous"
keep "$prog" "$kept"

# The rest of what the update replaces waits beside it too: in a folder, the archive's other files;
# in a service, Fenji's units, if the service is Fenji's own unit, and this script.
replaced='' units=''
if [ "$service" = no ]; then
  (cd "$new" && find . -type d -print) > "$tmp/dirs"
  (cd "$new" && find . -type f -print) | LC_ALL=C sort > "$tmp/files"
  while IFS= read -r d; do
    d=${d#./}
    installs "$d" || continue
    if [ -d "$here/$d" ]; then
      guard "$here/$d"
      writes "$here/$d"
    elif [ -e "$here/$d" ] || [ -L "$here/$d" ]; then
      fail "$here/$d is a file, where the new version has a folder, so nothing has changed"
    else
      printf '%s\n' "$here/$d" >> "$tmp/made"
      { mkdir -- "$here/$d" && chmod 0755 "$here/$d"; } || fail "can't make $here/$d, so nothing has changed"
    fi
    mkdir -p -- "$pstage/$d"
  done < "$tmp/dirs"
  while IFS= read -r f; do
    f=${f#./}
    installs "$f" || continue
    into "$here/$f"
    if [ -x "$new/$f" ]; then mode=0755; else mode=0644; fi
    put "$new/$f" "$pstage/$f" "$mode"
  done < "$tmp/files"
else
  # The archive's units replace the installed ones only if the service is Fenji's own unit: it runs
  # the archive's fenji.service's ExecStart, as its User. A host's own unit stays as it is. This
  # compares systemd's rendering of ExecStart, the argv[] systemctl show joins with spaces, with the
  # archive's unit's text, which holds because systemd shows the reference unit's ExecStart, one
  # line of plain words, word for word. A fenji.service whose ExecStart systemd shows otherwise (with
  # quotes, specifiers or a line continued) would quietly stop getting its units updated: safe, but
  # this would need to follow.
  ours=no
  if [ -f "$new/systemd/fenji.service" ]; then
    want_exec=$(unit_value ExecStart "$new/systemd/fenji.service")
    want_user=$(unit_value User "$new/systemd/fenji.service")
    if [ -n "$want_exec" ] && [ "$want_exec" = "$argv" ] && [ "$want_user" = "$user" ]; then ours=yes; fi
  fi
  if [ "$ours" = yes ]; then
    guard "$unitdir"
    stage_in "$unitdir"
    ustage=$sd
    for u in $UNITS; do
      [ -f "$new/systemd/$u" ] || continue
      into "$unitdir/$u"
      put "$new/systemd/$u" "$ustage/$u" 0644
      if [ -e "$unitdir/$u" ]; then
        into "$unitdir/$u.previous"
        keep "$unitdir/$u" "$ustage/$u.previous"
      fi
      units="$units $u"
    done
    odd=''
    for u in "$new"/systemd/*; do
      [ -e "$u" ] || continue
      case " $UNITS " in *" ${u##*/} "*) ;; *) odd="$odd${odd:+, }${u##*/}" ;; esac
    done
    [ -z "$odd" ] || say "Left out $odd, from the archive's systemd/: only Fenji's own three units are installed."
  elif [ -d "$new/systemd" ]; then
    say "Left the units in $unitdir as they are: the fenji service runs $argv as ${user:-root}, not as Fenji's own unit does, so only the program is updated."
  fi
  if [ -f "$new/update-server.sh" ]; then
    into "$self"
    put "$new/update-server.sh" "$pstage/update-server.sh" 0755
  fi
fi

# A backup first (HB6): the console's while a service's server runs, or the program's own.
backup=''
if [ "$service" = yes ] && [ "$running" = yes ]; then
  said=$(as_server console --run backup) ||
    fail "the server didn't make its backup, above, so nothing has changed, and it runs on as it was"
elif [ "$service" = yes ]; then
  said=$(as_server backup) || fail "the backup failed, above, so nothing has changed"
elif [ -f "$here/fenji.toml" ]; then
  said=$(cd "$here" && ./fenji-server backup) || fail "the backup failed, above, so nothing has changed"
else
  said=''
  say "There's no fenji.toml here, so the server has never started, and there's no database to back up."
fi
if [ -n "$said" ]; then
  backup=$(printf '%s\n' "$said" | tail -n 1)
  backup=${backup#backed up to }
  say "Backed up the database: $backup"
fi

# The swap (HB7): each new file renamed over the old one. Signals wait until it's done, so that no
# update stops halfway.
trap '' HUP INT TERM PIPE
if [ "$service" = no ]; then
  if ! { swap "$kept" "$prog.previous" && swap "$next" "$prog"; }; then
    fail "can't put the new program in place in $here, so nothing has changed"
  fi
  say "Installed fenji-server $wanted, and kept $current as fenji-server.previous."
  left=''
  while IFS= read -r f; do
    f=${f#./}
    installs "$f" || continue
    swap "$pstage/$f" "$here/$f" || left="$left $f"
  done < "$tmp/files"
  if [ -n "$left" ]; then
    fail "fenji-server $wanted is in place, but these files of its archive couldn't replace the old ones:$left. Go back to $current (mv $(quoted "$prog.previous") $(quoted "$prog")) and run this again"
  fi
  say "Replaced the archive's other files: $(listed)."
  say "Updated from $current to $wanted. Start the server with ./$start."
  exit 0
fi

# A service is stopped for the swap, and started again (HB5).
if [ "$running" = yes ]; then
  say "Stopping fenji."
  if ! systemctl stop fenji; then
    if systemctl start fenji 2> /dev/null && systemctl is-active --quiet fenji; then
      fail "can't stop the fenji service, so nothing has changed, and it runs on as it was"
    fi
    fail "can't stop the fenji service, so nothing has changed, but it isn't running now: sudo systemctl start fenji starts it, and sudo journalctl -u fenji says why it didn't stop"
  fi
fi
if ! { swap "$kept" "$prog.previous" && swap "$next" "$prog"; }; then
  if [ "$running" = yes ]; then systemctl start fenji || :; fi
  fail "can't put the new program in place in $here, so nothing has changed"
fi
say "Installed fenji-server $wanted in $here, and kept $current as fenji-server.previous."
if [ -n "$units" ]; then
  installed=''
  for u in $units; do
    if [ -e "$ustage/$u.previous" ]; then
      if swap "$ustage/$u.previous" "$unitdir/$u.previous"; then replaced="$replaced $u"; fi
    fi
    if swap "$ustage/$u" "$unitdir/$u"; then
      installed=yes
    else
      say "Couldn't install $u in $unitdir: the old one stays."
    fi
  done
  if [ -n "$installed" ]; then
    if systemctl daemon-reload; then
      say "Installed the units in $unitdir, kept the old ones as <unit>.previous, and reloaded systemd."
    else
      say "Installed the units in $unitdir, kept the old ones as <unit>.previous, but systemctl daemon-reload failed, above."
    fi
  fi
fi
if [ -f "$pstage/update-server.sh" ]; then
  if swap "$pstage/update-server.sh" "$self"; then
    say "Replaced $self."
  else
    say "Couldn't replace $self: the old one stays."
  fi
fi
if [ "$running" = no ]; then
  say "fenji wasn't running, so it stays stopped: sudo systemctl start fenji starts it."
  say "Updated from $current to $wanted."
  exit 0
fi
say "Starting fenji."
systemctl start fenji || :
trap 'exit 1' HUP INT TERM
trap - PIPE
# Up once systemd has it active and its console answers, which it does once the store is open and
# its ports listen (HB9). Each look takes 5 seconds at most. Active at every look, it runs, even if
# its console never answers.
n=1 up=yes
while :; do
  if systemctl is-active --quiet fenji; then
    if as_server -t console --run online > /dev/null 2>&1; then break; fi
  else
    up=no
  fi
  [ "$n" -lt 30 ] || gone "$up"
  n=$((n + 1))
  sleep 1
done
say "fenji is running $wanted."
say "Updated from $current to $wanted."
