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
# that version (HB4). The database is backed up first (HB6), and the old program stays as
# fenji-server.previous (HB7). It never touches fenji.toml, the data folder, systemd's drop-ins or
# Caddy's settings (HB8).
#
# Releases come from https://github.com/theintonerzero/fenji/releases, or from FENJI_RELEASES, for a
# fork. It needs curl, tar, and sha256sum or macOS's shasum; for a service, systemctl, sudo and
# journalctl too.
set -eu
# A cd to a relative path goes where it says, not to a folder an exported CDPATH names.
unset CDPATH
export LC_ALL=C

me=${0##*/}

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

# version_of PROGRAM: the version that PROGRAM --version reports, which says "fenji-server 0.5.3".
version_of() {
  said=$("$1" --version 2> /dev/null) || return 1
  said=${said##* }
  is_version "$said" || return 1
  printf '%s\n' "$said"
}

# resolved PATH: PATH, its folder's links resolved as this script's own folder's are; or nothing.
resolved() {
  (cd -P -- "$(dirname -- "$1")" 2> /dev/null && printf '%s/%s\n' "$(pwd -P)" "${1##*/}") || :
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

# fetch URL FILE: URL into FILE, over HTTPS alone, redirects too, failing on an HTTP error.
fetch() { curl -fsSL --proto '=https' --proto-redir '=https' -o "$2" "$1"; }

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

# staged PATH: where the new version of PATH waits beside it, under a temporary name, until the
# swap renames it over PATH.
staged() { printf '%s/.%s.update\n' "${1%/*}" "${1##*/}"; }

# stage FILE PATH: FILE, copied beside PATH under PATH's temporary name, and kept track of, so that
# a failure takes it away again.
stage() {
  to=$(staged "$2")
  printf '%s\n' "$to" >> "$tmp/staged"
  cp -p -- "$1" "$to" || fail "can't copy the new ${2##*/} into ${2%/*}, so nothing has changed"
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

# as_server ARG...: the service's program with ARGs, as the service's user and with its --config,
# from /, as systemd runs the server.
as_server() {
  if [ -n "$config" ]; then set -- --config "$config" "$@"; fi
  if [ -n "$user" ] && [ "$user" != root ]; then
    (cd / && sudo -u "$user" -- "$prog" "$@")
  else
    (cd / && "$prog" "$@")
  fi
}

# gone: the service didn't come up. Its journal's last lines, and how to go back (HB9).
gone() {
  {
    printf "%s: fenji didn't come up within 30 seconds. Its journal's last lines:\n" "$me"
    journalctl -u fenji -n 20 --no-pager 2>&1 || :
    printf 'To go back to %s, stop it, and put the previous program back:\n' "$current"
    printf '  sudo systemctl stop fenji\n'
    printf '  sudo mv %s.previous %s\n' "$prog" "$prog"
    printf 'Then restore the backup made before this update, as docs/hosting.md section 6 says:\n'
    printf '  %s\n' "${backup:-(none was made)}"
    printf 'and start it again: sudo systemctl start fenji\n'
  } >&2 2> /dev/null || :
  exit 1
}

# cleanup: on every exit, takes away whatever is still under a temporary name, the folders made for
# it, and the download.
tmp=
cleanup() {
  [ -n "$tmp" ] || return 0
  if [ -f "$tmp/staged" ]; then
    while IFS= read -r f; do rm -f -- "$f"; done < "$tmp/staged"
  fi
  if [ -f "$tmp/made" ]; then
    sort -r -- "$tmp/made" | while IFS= read -r d; do rmdir -- "$d" 2> /dev/null || :; done
  fi
  rm -rf -- "$tmp"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

case ${1-} in
  -h | --help) usage; exit 0 ;;
  -*) usage >&2; exit 2 ;;
esac
[ $# -le 1 ] || { usage >&2; exit 2; }
wanted=
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

# The fenji service, if it runs the program beside this script (HB2), from what systemd says of it:
# its ExecStart's program, as the service's User, with its --config, and its units' folder.
service=no user='' config='' state='' unitdir=/etc/systemd/system shown=''
if command -v systemctl > /dev/null 2>&1; then
  shown=$(systemctl show --property=LoadState,ActiveState,User,ExecStart,FragmentPath fenji 2> /dev/null) || shown=''
fi
prop() { printf '%s\n' "$shown" | sed -n "s/^$1=//p" | head -n 1; }
if [ "$(prop LoadState)" = loaded ]; then
  # ExecStart={ path=/usr/local/bin/fenji-server ; argv[]=/usr/local/bin/fenji-server --config … run ; … }
  line=$(prop ExecStart)
  path=$(printf '%s\n' "$line" | sed -n 's/^{ path=\(.*\) ; argv\[\]=.*$/\1/p')
  if [ -n "$path" ] && [ "$(resolved "$path")" = "$prog" ]; then
    service=yes
    user=$(prop User)
    state=$(prop ActiveState)
    argv=$(printf '%s\n' "$line" | sed -n 's/^.* ; argv\[\]=\(.*\) ; ignore_errors=.*$/\1/p')
    # The words as systemd shows them, split at spaces: the reference unit's have none of their own.
    set -f
    # shellcheck disable=SC2086
    config=$(config_of $argv)
    set +f
    fragment=$(prop FragmentPath)
    [ -z "$fragment" ] || unitdir=${fragment%/*}
  fi
fi

# Who runs it (HB8): root for a service, which writes in /usr/local/bin and the units' folder; the
# folder's owner for a folder, so that the files it puts there stay theirs.
uid=$(id -u)
running=no
if [ "$service" = yes ]; then
  [ "$uid" = 0 ] || fail "the fenji service runs $prog, and updating it takes root: run sudo $self"
  case $state in
    active) running=yes ;;
    inactive | failed) running=no ;;
    *) fail "the fenji service is $state: run this again once it's active, or stopped" ;;
  esac
else
  # A VPS's fenji-update, run without sudo, may find no service: systemd answers only root where
  # there's no D-Bus.
  if [ ! -f "$here/$start" ] && [ "$uid" != 0 ]; then
    fail "$here has no $start, so it isn't a server's folder: to update the fenji service on a VPS, run it as root: sudo $self; on your own computer, run the update script in your server's folder (docs/hosting.md, section 5)"
  elif [ ! -f "$here/$start" ]; then
    fail "$here has no $start, so it isn't a server's folder, and no fenji service runs its fenji-server: update it by hand (docs/hosting.md, section 5)"
  fi
  # ls shows a folder's owner on Linux and macOS alike, where stat's options differ.
  # shellcheck disable=SC2012
  owner=$(ls -ldn -- "$here" | awk '{ print $3 }')
  if [ "$owner" != "$uid" ]; then
    # shellcheck disable=SC2012
    name=$(ls -ld -- "$here" | awk '{ print $3 }')
    if [ "$uid" = 0 ]; then
      fail "$here is $name's, and this runs as root: run it as $name, without sudo, so that the files it puts there stay $name's"
    fi
    fail "$here is $name's: run it as $name"
  fi
fi

# Which version (HB3): the latest release, from where releases/latest leads, or the one named.
current=$(version_of "$prog") ||
  fail "$prog doesn't run, or doesn't say its version: update it by hand (docs/hosting.md, section 5)"
if [ -z "$wanted" ]; then
  landed=$(curl -fsSL --proto '=https' --proto-redir '=https' -o /dev/null -w '%{url_effective}' "$releases/latest") ||
    fail "can't reach $releases/latest, to find the latest release"
  wanted=${landed##*/tag/v}
  if [ "$wanted" = "$landed" ] || ! is_version "$wanted"; then
    fail "can't tell the latest release from where $releases/latest leads: ${landed:-nowhere}"
  fi
  say "The latest release is $wanted."
fi
case $(compare "$wanted" "$current") in
  0)
    if [ $# -eq 0 ]; then
      say "This server runs $current, the latest release: there's nothing to update."
    else
      say "This server runs $current already: there's nothing to update."
    fi
    exit 0 ;;
  -1) fail "$wanted is older than $current, which this server runs, and an update never goes back, as the database may have moved on since: to go back to $wanted, restore a backup made with $wanted, with that version's program (docs/hosting.md, section 6)" ;;
esac

# A server running from a folder runs in a terminal or in tmux, where it can't be started again
# from here (HB5).
if [ "$service" = no ] && (cd "$here" && ./fenji-server console --run online) > /dev/null 2>&1; then
  fail "a server is running from $here: stop it first: type stop in its console, then run $me again"
fi

if [ "$service" = yes ]; then
  say "Updating the fenji service, which runs $prog, from $current to $wanted."
else
  say "Updating the server in $here from $current to $wanted."
fi

# The download, checked (HB4): nothing changes until it matches SHA256SUMS, and its program runs and
# reports its version.
tmp=$(mktemp -d "${TMPDIR:-/tmp}/fenji-update.XXXXXX") || fail "can't make a temporary folder for the download"
: > "$tmp/staged"
: > "$tmp/made"
archive=fenji-server-$wanted-$target.tar.gz
from=$releases/download/v$wanted
say "Downloading $archive, with its release's SHA256SUMS."
fetch "$from/$archive" "$tmp/$archive" ||
  fail "can't download $from/$archive: is $wanted a release, with a server for $target? Nothing has changed"
fetch "$from/SHA256SUMS" "$tmp/SHA256SUMS" || fail "can't download $from/SHA256SUMS, so nothing has changed"
expected=$(awk -v f="$archive" '$2 == f || $2 == "*" f { n++; s = $1 } END { if (n == 1) print s }' "$tmp/SHA256SUMS")
[ -n "$expected" ] || fail "SHA256SUMS has no line for $archive, so it can't be checked, and nothing has changed"
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

# The new program waits beside the old one, where it's run to check it: the temporary folder may
# not let programs run.
next=$(staged "$prog")
stage "$new/fenji-server" "$prog"
said=$("$next" --version 2> /dev/null) || fail "the new fenji-server doesn't run on this computer, so nothing has changed"
said=${said##* }
[ "$said" = "$wanted" ] || fail "the new fenji-server says it's ${said:-no version}, not $wanted, so nothing has changed"
say "The new fenji-server runs, and reports $wanted."
# The old program, as fenji-server.previous will keep it (HB7).
kept=$(staged "$prog.previous")
printf '%s\n' "$kept" >> "$tmp/staged"
rm -f -- "$kept"
ln -- "$prog" "$kept" 2> /dev/null || cp -p -- "$prog" "$kept" ||
  fail "can't keep the old program beside the new one, in $here, so nothing has changed"

# The rest of what the update replaces waits beside it too: in a folder, the archive's other files;
# in a service, the units and this script.
if [ "$service" = no ]; then
  (cd "$new" && find . -type d -print) > "$tmp/dirs"
  (cd "$new" && find . -type f -print) | LC_ALL=C sort > "$tmp/files"
  while IFS= read -r d; do
    d=${d#./}
    installs "$d" || continue
    if [ ! -d "$here/$d" ]; then
      printf '%s\n' "$here/$d" >> "$tmp/made"
      mkdir -- "$here/$d" || fail "can't make $here/$d, so nothing has changed"
    fi
  done < "$tmp/dirs"
  while IFS= read -r f; do
    f=${f#./}
    installs "$f" || continue
    stage "$new/$f" "$here/$f"
  done < "$tmp/files"
else
  for u in "$new"/systemd/*; do
    [ -f "$u" ] || continue
    stage "$u" "$unitdir/${u##*/}"
  done
  if [ -f "$new/update-server.sh" ]; then stage "$new/update-server.sh" "$self"; fi
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
  if ! { mv -f -- "$kept" "$prog.previous" && mv -f -- "$next" "$prog"; }; then
    fail "can't put the new program in place in $here, so nothing has changed"
  fi
  say "Installed fenji-server $wanted, and kept $current as fenji-server.previous."
  left=''
  while IFS= read -r f; do
    f=${f#./}
    installs "$f" || continue
    mv -f -- "$(staged "$here/$f")" "$here/$f" || left="$left $f"
  done < "$tmp/files"
  [ -z "$left" ] || fail "fenji-server $wanted is in place, but these files of its archive couldn't replace the old ones:$left"
  say "Replaced the archive's other files: $(listed)."
  say "Updated from $current to $wanted. Start the server with ./$start."
  exit 0
fi

# A service is stopped for the swap, and started again (HB5).
if [ "$running" = yes ]; then
  say "Stopping fenji."
  if ! systemctl stop fenji; then
    systemctl start fenji 2> /dev/null || :
    fail "can't stop the fenji service, so nothing has changed"
  fi
fi
if ! { mv -f -- "$kept" "$prog.previous" && mv -f -- "$next" "$prog"; }; then
  if [ "$running" = yes ]; then systemctl start fenji || :; fi
  fail "can't put the new program in place in $here, so nothing has changed"
fi
say "Installed fenji-server $wanted in $here, and kept $current as fenji-server.previous."
units=''
for u in "$new"/systemd/*; do
  [ -f "$u" ] || continue
  if mv -f -- "$(staged "$unitdir/${u##*/}")" "$unitdir/${u##*/}"; then
    units=yes
  else
    say "Couldn't install ${u##*/} in $unitdir: the old one stays."
  fi
done
if [ -n "$units" ]; then
  if systemctl daemon-reload; then
    say "Installed the units in $unitdir, and reloaded systemd."
  else
    say "Installed the units in $unitdir, but systemctl daemon-reload failed, above."
  fi
fi
if [ -f "$new/update-server.sh" ]; then
  if mv -f -- "$(staged "$self")" "$self"; then
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
# its ports listen (HB9).
n=1
until systemctl is-active --quiet fenji && as_server console --run online > /dev/null 2>&1; do
  [ "$n" -lt 30 ] || gone
  n=$((n + 1))
  sleep 1
done
say "fenji is running $wanted."
say "Updated from $current to $wanted."
