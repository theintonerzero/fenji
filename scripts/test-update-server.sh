#!/usr/bin/env bash
# Tests for deploy/start/update-server.sh (the hosting spec's Amendment B, HB10). It updates a
# server's folder, and the reference VPS's service, from a fake release that a stand-in for curl
# serves, with stand-ins for systemctl, sudo, journalctl, sleep, timeout, id, uname, find and mv,
# and a stand-in fenji-server that reports its version and records what it's asked: no network, no
# root and no systemd. Where the script runs as root, the stand-in for id says so, and the one for
# find lets the test's own user play root (FAKE_ROOT_UID). Needs tar, gzip, cksum, and sha256sum or
# shasum. Runs on Linux and macOS (bash 3.2).
#   scripts/test-update-server.sh
# With TEST_SH set (TEST_SH=dash, say), the script runs in that shell rather than its #!'s.
#
# What the harness can't model: one OS user plays every part. The stand-in sudo keeps the test's own
# uid, and only id -u says another; staging folders are always fresh; and there's no second uid, nor
# anyone racing the script. So three of the review's mutants survive it, each checked by hand
# instead: a umask of 022 for 077, a copy into a path that's there, and a staging folder that isn't
# 0711 (which a real sudo -u fenji needs, as the systemd container's run showed).
set -uo pipefail
export LC_ALL=C
unset CDPATH
# The folders the tests make are writable by their owner alone, as root's checks want them.
umask 022
HERE=$(cd -- "$(dirname -- "$0")" && pwd -P)
REPO=$(cd -- "$HERE/.." && pwd -P)
SCRIPT=$REPO/deploy/start/update-server.sh
fails=0
ok() { echo "ok   $1"; }
bad() { echo "FAIL $1"; fails=$((fails + 1)); }

for tool in tar gzip cksum; do
  command -v "$tool" > /dev/null || { echo "FATAL: the test needs $tool" >&2; exit 2; }
done
if command -v sha256sum > /dev/null; then
  sums() { sha256sum -- "$@"; }
elif command -v shasum > /dev/null; then
  sums() { shasum -a 256 -- "$@"; }
else
  echo "FATAL: the test needs sha256sum or shasum" >&2
  exit 2
fi

T=$(mktemp -d) || { echo "FATAL: mktemp failed" >&2; exit 2; }
T=$(cd -- "$T" && pwd -P) || exit 2
# A test leaves a folder read-only: it's made writable again before it goes.
trap 'chmod -R u+w -- "$T" 2> /dev/null; rm -rf -- "$T"' EXIT
case "$T" in /?*) ;; *) echo "FATAL: no temporary directory" >&2; exit 2 ;; esac
mkdir "$T/bin" "$T/state" "$T/elsewhere" "$T/tmp" "$T/stage"
REAL_ID=$(command -v id)
REAL_UNAME=$(command -v uname)
REAL_FIND=$(command -v find)
REAL_MV=$(command -v mv)
uid=$("$REAL_ID" -u)
me=$("$REAL_ID" -un)

# The stand-ins, first on PATH. Each records what it's asked, a line each, in $FAKE_CALLS.
#
# curl serves $FAKE_RELEASE_DIR as $FAKE_RELEASES: <releases>/latest leads to the tag of
# $FAKE_LATEST, or to the releases' own page when that's empty, as GitHub's does without a release;
# <releases>/download/<tag>/<file> is that file, or a 404. It records the URL, its first argument,
# then its options.
cat > "$T/bin/curl" <<'EOF'
#!/bin/sh
first=${1-} out= format= url= opts=
while [ $# -gt 0 ]; do
  case $1 in
    -o) out=$2; shift 2 ;;
    -w) format=$2; shift 2 ;;
    --proto | --proto-redir) opts="$opts $1 $2"; shift 2 ;;
    -*) opts="$opts $1"; shift ;;
    *) url=$1; shift ;;
  esac
done
printf 'curl %s first=%s%s\n' "$url" "$first" "$opts" >> "$FAKE_CALLS"
case $url in
  "$FAKE_RELEASES/latest")
    landed=$FAKE_RELEASES${FAKE_LATEST:+/tag/v$FAKE_LATEST}
    [ -z "$out" ] || printf '<html></html>\n' > "$out"
    [ "$format" != '%{url_effective}' ] || printf '%s' "$landed" ;;
  "$FAKE_RELEASES/download/"*)
    file=$FAKE_RELEASE_DIR/${url#"$FAKE_RELEASES/download/"}
    if [ ! -f "$file" ]; then
      echo "curl: (22) The requested URL returned error: 404" >&2
      exit 22
    fi
    cp "$file" "$out" ;;
  *) echo "curl: (6) Could not resolve host: $url" >&2; exit 6 ;;
esac
EOF
# systemctl, for one unit, fenji: there's none while FAKE_UNIT is empty, and no systemd at all when
# it's "nosystemd". Otherwise its ExecStart runs the program FAKE_UNIT, with --config
# FAKE_UNIT_CONFIG if that's set, as FAKE_UNIT_USER, from the unit file FAKE_UNIT_FRAGMENT and in
# FAKE_UNIT_WORKDIR, and its state is in $FAKE_STATE/active. A started server answers its console
# (as $FAKE_RUNNING says), unless FAKE_START=fail, which leaves it activating, as a server that
# fails as it starts does, or FAKE_START=noconsole, which leaves it active, with no console. A stop
# fails with FAKE_STOP=fail, and with FAKE_STOP=signals sends the script TERM, INT and HUP first.
# stop and start record the version of the program the unit runs, and daemon-reload the unit's
# first line.
cat > "$T/bin/systemctl" <<'EOF'
#!/bin/sh
case $1 in
  stop | start) note=" (fenji-server $(sed -n 's/^version=//p' "$FAKE_UNIT" 2> /dev/null))" ;;
  daemon-reload) note=" ($(head -n 1 "$FAKE_UNIT_FRAGMENT" 2> /dev/null))" ;;
  *) note= ;;
esac
printf 'systemctl %s%s\n' "$*" "$note" >> "$FAKE_CALLS"
if [ "${FAKE_UNIT:-}" = nosystemd ]; then
  echo "System has not been booted with systemd as init system (PID 1). Can't operate." >&2
  exit 1
fi
state=$(cat "$FAKE_STATE/active")
case $1 in
  show)
    if [ -z "${FAKE_UNIT:-}" ]; then
      printf '%s\n' LoadState=not-found ActiveState=inactive User= ExecStart= FragmentPath= WorkingDirectory=
    else
      args="$FAKE_UNIT${FAKE_UNIT_CONFIG:+ --config $FAKE_UNIT_CONFIG} run"
      printf '%s\n' Type=exec \
        "ExecStart={ path=$FAKE_UNIT ; argv[]=$args ; ignore_errors=no ; start_time=[n/a] ; stop_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }" \
        "User=$FAKE_UNIT_USER" "FragmentPath=$FAKE_UNIT_FRAGMENT" LoadState=loaded "ActiveState=$state" \
        "WorkingDirectory=$FAKE_UNIT_WORKDIR"
    fi ;;
  is-active)
    [ "${2-}" = --quiet ] || echo "$state"
    [ "$state" = active ] ;;
  stop)
    case ${FAKE_STOP:-} in
      fail) echo "Job for fenji.service canceled." >&2; exit 1 ;;
      signals) kill -TERM "$PPID"; kill -INT "$PPID"; kill -HUP "$PPID" ;;
    esac
    echo inactive > "$FAKE_STATE/active"
    rm -f "$FAKE_RUNNING" ;;
  start)
    case ${FAKE_START:-} in
      fail) echo activating > "$FAKE_STATE/active" ;;
      noconsole) echo active > "$FAKE_STATE/active" ;;
      *) echo active > "$FAKE_STATE/active"; : > "$FAKE_RUNNING" ;;
    esac ;;
  daemon-reload) ;;
  *) echo "stand-in systemctl: no $1 here" >&2; exit 1 ;;
esac
EOF
# sudo -u USER [--] COMMAND...: runs COMMAND, as this user, but with USER's uid as id -u says it:
# fenji's is 990, nobody's 65534, and anyone else's 1001.
cat > "$T/bin/sudo" <<'EOF'
#!/bin/sh
printf 'sudo %s\n' "$*" >> "$FAKE_CALLS"
if [ "${1-}" != -u ] || [ -z "${2-}" ]; then
  echo "stand-in sudo: no -u <user>" >&2
  exit 1
fi
case $2 in
  fenji) FAKE_UID=990 ;;
  nobody) FAKE_UID=65534 ;;
  root) FAKE_UID=0 ;;
  *) FAKE_UID=1001 ;;
esac
export FAKE_UID
shift 2
[ "${1-}" != -- ] || shift
exec "$@"
EOF
cat > "$T/bin/journalctl" <<'EOF'
#!/bin/sh
printf 'journalctl %s\n' "$*" >> "$FAKE_CALLS"
echo "fenji-server[4242]: Error: the stand-in server won't start"
EOF
# sleep returns at once, so that waiting for a service costs no time.
cat > "$T/bin/sleep" <<'EOF'
#!/bin/sh
printf 'sleep %s\n' "$*" >> "$FAKE_CALLS"
EOF
# timeout SECONDS COMMAND...: records the bound, and runs COMMAND, without one.
cat > "$T/bin/timeout" <<'EOF'
#!/bin/sh
printf 'timeout %s\n' "$*" >> "$FAKE_CALLS"
shift
exec "$@"
EOF
# id -u says FAKE_UID, when it's set.
cat > "$T/bin/id" <<'EOF'
#!/bin/sh
if [ "$*" = -u ] && [ -n "${FAKE_UID:-}" ]; then
  echo "$FAKE_UID"
  exit 0
fi
exec "$REAL_ID" "$@"
EOF
# uname -s and -m say FAKE_UNAME_S and FAKE_UNAME_M.
cat > "$T/bin/uname" <<'EOF'
#!/bin/sh
case "$*" in
  -s) echo "$FAKE_UNAME_S" ;;
  -m) echo "$FAKE_UNAME_M" ;;
  *) exec "$REAL_UNAME" "$@" ;;
esac
EOF
# find, but where FAKE_ROOT_UID is set, root's files are both root's own and that uid's: the test's
# own user plays root, whose files the script running "as root" finds to be its own, while the real
# root's folders above the test's, / and /tmp, stay root's. A folder FAKE_NOT_ROOTS names, a start
# path of find's, is another user's: its owner doesn't play root.
cat > "$T/bin/find" <<'EOF'
#!/bin/sh
if [ -n "${FAKE_ROOT_UID:-}" ] && [ "${1-}" != "${FAKE_NOT_ROOTS:-}" ]; then
  n=$#
  user=
  for a in "$@"; do
    if [ -n "$user" ]; then
      user=
      if [ "$a" = 0 ]; then set -- "$@" '(' -user 0 -o -user "$FAKE_ROOT_UID" ')'; else set -- "$@" -user "$a"; fi
    elif [ "$a" = -user ]; then
      user=yes
    else
      set -- "$@" "$a"
    fi
  done
  shift "$n"
fi
exec "$REAL_FIND" "$@"
EOF
# mv, failing as a rename can when its last argument ends in /FAKE_MV_FAIL.
cat > "$T/bin/mv" <<'EOF'
#!/bin/sh
if [ -n "${FAKE_MV_FAIL:-}" ]; then
  for last in "$@"; do :; done
  case $last in */"$FAKE_MV_FAIL") echo "mv: cannot move to '$last': Input/output error" >&2; exit 1 ;; esac
fi
exec "$REAL_MV" "$@"
EOF
chmod 0755 "$T/bin/"*
PATH="$T/bin:$PATH"
export PATH REAL_ID REAL_UNAME REAL_FIND REAL_MV FAKE_CALLS=$T/calls FAKE_STATE=$T/state FAKE_RUNNING=$T/state/running
export FAKE_RELEASES=https://releases.example/fenji/releases FAKE_RELEASE_DIR=$T/release
export FENJI_RELEASES=$FAKE_RELEASES TMPDIR=$T/tmp

# program VERSION [REPORTS]: a stand-in fenji-server of VERSION, whose --version reports REPORTS,
# VERSION unless given. It records each call with its version, the uid it runs as and its working
# directory. Its console answers while $FAKE_RUNNING is there, as a running server's does, and its
# backups say FAKE_BACKUP_TO, or fail when FAKE_BACKUP=fail. It writes nothing anywhere else.
program() {
  printf '#!/bin/sh\nversion=%s\nreports=%s\n' "$1" "${2:-$1}"
  cat <<'EOF'
printf 'fenji-server %s as %s in %s: %s\n' "$version" "$(id -u)" "$(pwd -P)" "$*" >> "$FAKE_CALLS"
if [ "${1-}" = --config ]; then shift 2; fi
case "$*" in
  --version) echo "fenji-server $reports" ;;
  "console --run online")
    if [ -e "$FAKE_RUNNING" ]; then echo "nobody is online"; exit 0; fi
    echo "Error: no server is running with this data folder" >&2
    exit 1 ;;
  "console --run backup")
    if [ -e "$FAKE_RUNNING" ] && [ "${FAKE_BACKUP:-}" != fail ]; then echo "backed up to $FAKE_BACKUP_TO"; exit 0; fi
    echo "Error: the backup failed" >&2
    exit 1 ;;
  backup)
    if [ "${FAKE_BACKUP:-}" = fail ]; then echo "Error: the backup failed" >&2; exit 1; fi
    echo "$FAKE_BACKUP_TO" ;;
  *) echo "stand-in fenji-server: unexpected: $*" >&2; exit 1 ;;
esac
EOF
}

TARGETS="x86_64-unknown-linux-musl aarch64-unknown-linux-musl aarch64-apple-darwin"
TEXTS="README.txt LICENSE SOURCE.txt THIRD-PARTY-NOTICES.txt fenji.example.toml"
UNITS="fenji.service fenji-backup.service fenji-backup.timer"
S=$T/svc
B=$S/usr/local/bin
L=x86_64-unknown-linux-musl

# unit NAME WHOSE: the unit file NAME, of WHOSE (a release, or an installed version), as Fenji's own
# unit is on the test's VPS: run as fenji, with the program and settings under $S.
unit() {
  printf '# %s of %s\n[Service]\nUser=fenji\nExecStart=%s --config %s run\n' "$1" "$2" "$B/fenji-server" \
    "$S/etc/fenji/fenji.toml"
}

# release VERSION [PROGRAM]: a fake release of VERSION, in $T/release/vVERSION: an archive for each
# target, laid out as scripts/package-server.sh lays it out, and SHA256SUMS over all its files, a
# launcher's among them, as the release workflow attaches them. Its fenji-server is a stand-in of
# VERSION, or a copy of the file PROGRAM. Its update script is this one, with a line of its own,
# and each text says its release. The archives' files stay in $T/stage, to compare. With
# ODD_UNITS set, a Linux archive's systemd/ also holds a unit that isn't Fenji's, extra.service,
# its fenji.service is writable by everyone and its program by anyone. With HOSTILE set, an
# archive also holds a fenji.toml and a data folder.
release() {
  local v=$1 from=${2:-} dir=$T/release/v$1 t name s start f u
  mkdir -p "$dir"
  for t in $TARGETS; do
    name=fenji-server-$v-$t
    s=$T/stage/$name
    rm -rf -- "$s"
    mkdir -p "$s"
    if [ -n "$from" ]; then cp "$from" "$s/fenji-server"; else program "$v" > "$s/fenji-server"; fi
    case $t in *-apple-darwin) start='start-server.command' ;; *) start='start-server.sh' ;; esac
    printf '#!/bin/sh\necho "the start script of release %s"\n' "$v" > "$s/$start"
    { cat "$SCRIPT" 2> /dev/null; printf '# the update script of release %s\n' "$v"; } > "$s/update-server.sh"
    chmod 0755 "$s" "$s/fenji-server" "$s/$start" "$s/update-server.sh"
    for f in $TEXTS; do printf '%s of release %s\n' "$f" "$v" > "$s/$f"; done
    case $t in
      *-linux-*)
        mkdir "$s/systemd" "$s/caddy"
        for u in $UNITS; do unit "$u" "release $v" > "$s/systemd/$u"; done
        printf '# the Caddyfile of release %s\n' "$v" > "$s/caddy/Caddyfile"
        if [ -n "${ODD_UNITS:-}" ]; then
          printf '[Service]\nExecStart=/bin/sh -c "echo not Fenji"\n' > "$s/systemd/extra.service"
          chmod 0666 "$s/systemd/fenji.service"
          chmod 0777 "$s/fenji-server"
        fi ;;
    esac
    if [ -n "${HOSTILE:-}" ]; then
      printf '[server]\nname = "the archive'"'"'s"\n' > "$s/fenji.toml"
      mkdir "$s/data"
      printf 'the archive'"'"'s database\n' > "$s/data/fenji.db"
    fi
    (cd "$T/stage" && tar -czf "$dir/$name.tar.gz" "$name")
  done
  printf 'a launcher\n' > "$dir/Fenji_${v}_amd64.AppImage"
  (cd "$dir" && sums ./*.tar.gz ./*.AppImage | sed 's|  \./|  |' > SHA256SUMS)
}

# age DIR: everything in DIR made as old as 2020, so that anything an update writes, or a folder it
# adds to or takes from, is newer than $T/ref (2021).
age() { find "$1" -exec touch -t 202001010000 {} +; }
touch -t 202101010000 "$T/ref"

# folder DIR VERSION [START]: a server's folder, as an archive of VERSION unpacked there and started
# once: the program, the start script (START, start-server.sh unless given), this update script and
# the texts, and on Linux systemd/ and caddy/; then the host's own fenji.toml and data folder.
folder() {
  local d=$1 v=$2 start=${3:-start-server.sh} f u
  chmod -R u+w -- "$d" 2> /dev/null
  rm -rf -- "$d"
  mkdir -p "$d"
  program "$v" > "$d/fenji-server"
  printf '#!/bin/sh\necho "the start script of %s"\n' "$v" > "$d/$start"
  cp "$SCRIPT" "$d/update-server.sh" 2> /dev/null
  chmod 0755 "$d/fenji-server" "$d/$start" "$d/update-server.sh" 2> /dev/null
  for f in $TEXTS; do printf '%s of %s\n' "$f" "$v" > "$d/$f"; done
  if [ "$start" = start-server.sh ]; then
    mkdir "$d/systemd" "$d/caddy"
    for u in $UNITS; do printf '# %s of %s\n' "$u" "$v" > "$d/systemd/$u"; done
    printf '# the Caddyfile of %s\n' "$v" > "$d/caddy/Caddyfile"
  fi
  printf '[server]\nname = "Moonward"\n' > "$d/fenji.toml"
  mkdir -p "$d/data/backups" "$d/data/tls"
  printf 'the database\n' > "$d/data/fenji.db"
  printf 'the key\n' > "$d/data/tls/key.pem"
  chmod 0700 "$d/data" "$d/data/tls"
  age "$d"
}

# service VERSION: the reference VPS (docs/hosting.md, section 4) under $S, with VERSION's program
# and this script as fenji-update in usr/local/bin, Fenji's units in etc/systemd/system with a
# drop-in and another service's unit beside them, Caddy's file, fenji.toml and the data folder; and
# the fenji service running that program, as the fenji user, with that fenji.toml. The script runs
# as root, and the test's folders are root's.
service() {
  local u
  rm -rf -- "$S"
  mkdir -p "$B" "$S/etc/systemd/system/fenji.service.d" "$S/etc/fenji" "$S/etc/caddy" "$S/var/lib/fenji/backups"
  program "$1" > "$B/fenji-server"
  cp "$SCRIPT" "$B/fenji-update" 2> /dev/null
  chmod 0755 "$B/fenji-server" "$B/fenji-update" 2> /dev/null
  for u in $UNITS; do unit "$u" "$1" > "$S/etc/systemd/system/$u"; done
  printf '[Service]\nLockPersonality=no\n' > "$S/etc/systemd/system/fenji.service.d/container.conf"
  printf '[Unit]\nDescription=Another service\n' > "$S/etc/systemd/system/other.service"
  printf 'play.example.net {\n\treverse_proxy 127.0.0.1:7801\n}\n' > "$S/etc/caddy/Caddyfile"
  printf '[server]\nname = "Moonward"\n' > "$S/etc/fenji/fenji.toml"
  printf 'the database\n' > "$S/var/lib/fenji/fenji.db"
  age "$S"
  echo active > "$T/state/active"
  : > "$FAKE_RUNNING"
  export FAKE_UNIT=$B/fenji-server FAKE_UNIT_USER=fenji FAKE_UNIT_CONFIG=$S/etc/fenji/fenji.toml
  export FAKE_UNIT_FRAGMENT=$S/etc/systemd/system/fenji.service FAKE_UID=0 FAKE_ROOT_UID=$uid
  export FAKE_BACKUP_TO=/var/lib/fenji/backups/fenji-20261017-101500.db
}

# fresh: each stand-in as a case starts with it: the latest release is 1.3.0, there's no fenji
# service, no server runs, this is Linux on x86_64, and the script runs as its own user, who isn't
# root.
fresh() {
  export FAKE_LATEST=1.3.0 FAKE_UNIT='' FAKE_UNIT_USER=fenji FAKE_UNIT_CONFIG='' FAKE_UNIT_FRAGMENT=''
  export FAKE_UNIT_WORKDIR='' FAKE_START='' FAKE_STOP='' FAKE_BACKUP='' FAKE_UID='' FAKE_ROOT_UID=''
  export FAKE_MV_FAIL='' FAKE_NOT_ROOTS='' FAKE_UNAME_S=Linux FAKE_UNAME_M=x86_64
  export FAKE_BACKUP_TO=data/backups/fenji-20261017-101500.db
  echo inactive > "$T/state/active"
  rm -f -- "$FAKE_RUNNING"
}

# run SCRIPT [ARG...]: SCRIPT, from another folder (FROM, or $T/elsewhere), with CDPATH=RUN_CDPATH
# when that's set, and the umask RUN_UMASK when that is. Sets rc, out, err and calls, what the
# stand-ins were asked, in order. A run that leaves anything in the temporary folder is named in
# $dirty.
dirty=
run() {
  local script=$1 from=${FROM:-$T/elsewhere} mask
  shift
  local cmd=("$script")
  [ -z "${TEST_SH:-}" ] || cmd=("$TEST_SH" "$script")
  mask=${RUN_UMASK:-$(umask)}
  : > "$T/calls"
  rm -rf -- "$T/tmp"
  mkdir "$T/tmp"
  (umask "$mask" && cd "$from" && env ${RUN_CDPATH:+"CDPATH=$RUN_CDPATH"} "${cmd[@]}" "$@") > "$T/out" 2> "$T/err"
  rc=$?
  out=$(cat "$T/out")
  err=$(cat "$T/err")
  calls=$(cat "$T/calls")
  if [ -n "$(find "$T/tmp" -mindepth 1 -print)" ]; then dirty="$dirty, $script ${*:-(latest)}"; fi
}

# exits WHAT STATUS: the last run exited with STATUS.
exits() {
  if [ "$rc" = "$2" ]; then ok "$1"; else bad "$1: exit $rc, want $2: $out $err"; fi
}

# same WHAT GOT WANT: GOT is exactly WANT.
same() {
  if [ "$2" = "$3" ]; then ok "$1"; else bad "$1"; printf '  want:\n%s\n  got:\n%s\n' "$3" "$2"; fi
}

# holds WHAT COMMAND...: COMMAND succeeds.
holds() {
  local what=$1
  shift
  if "$@"; then ok "$what"; else bad "$what"; fi
}

# says WHAT TEXT IN: IN has TEXT, as it is. unsaid WHAT TEXT IN: IN hasn't.
says() {
  if grep -qF -- "$2" <<< "$3"; then ok "$1"; else bad "$1: no \"$2\" in:"; printf '%s\n' "$3" | sed 's/^/  /'; fi
}
unsaid() {
  if grep -qF -- "$2" <<< "$3"; then bad "$1: \"$2\" in:"; printf '%s\n' "$3" | sed 's/^/  /'; else ok "$1"; fi
}

# asked WHAT TEXT: a stand-in was asked TEXT. never WHAT TEXT: none was.
asked() { says "$1" "$2" "$calls"; }
never() { unsaid "$1" "$2" "$calls"; }

# before WHAT A B: in what the stand-ins were asked, a line with A comes before the first with B.
before() {
  local a b
  a=$(grep -nF -- "$2" <<< "$calls" | head -n 1 | cut -d: -f1)
  b=$(grep -nF -- "$3" <<< "$calls" | head -n 1 | cut -d: -f1)
  if [ -n "$a" ] && [ -n "$b" ] && [ "$a" -lt "$b" ]; then
    ok "$1"
  else
    bad "$1: \"$2\" at line ${a:-none}, \"$3\" at line ${b:-none}:"
    printf '%s\n' "$calls" | sed 's/^/  /'
  fi
}

# snapshot DIR: each path in DIR, with its mode, its owner and, for a file, its checksum. ls -ln
# shows the mode and owner on Linux and macOS alike, where stat's options differ.
# shellcheck disable=SC2012
snapshot() {
  (cd "$1" && find . -print | LC_ALL=C sort | while IFS= read -r p; do
    if [ -d "$p" ]; then
      printf '%s %s\n' "$(ls -ldn -- "$p" | awk '{ print $1, $3 }')" "$p"
    else
      printf '%s %s %s\n' "$(ls -ldn -- "$p" | awk '{ print $1, $3 }')" "$(cksum < "$p")" "$p"
    fi
  done)
}

# as_was WHAT DIR SNAPSHOT: DIR is as SNAPSHOT shows it, every file and mode.
as_was() {
  local now
  now=$(snapshot "$2")
  if [ "$now" = "$3" ]; then
    ok "$1"
  else
    bad "$1"
    diff <(printf '%s\n' "$3") <(printf '%s\n' "$now") | sed 's/^/  /'
  fi
}

# untouched WHAT PATH...: nothing in PATHs was written, added or taken away since they were made.
untouched() {
  local what=$1 got
  shift
  got=$(find "$@" -newer "$T/ref" -print 2>&1)
  if [ -z "$got" ]; then ok "$what"; else bad "$what: changed: $got"; fi
}

# entries DIR: the names in DIR, sorted.
entries() { find "$1" -mindepth 1 -maxdepth 1 -print | sed 's|.*/||' | LC_ALL=C sort; }

# mode FILE: FILE's mode, as ls shows it.
# shellcheck disable=SC2012
mode() { ls -ln -- "$1" | cut -c1-10; }

# version FILE: what the program FILE reports.
version() { "$1" --version 2> /dev/null; }

# fetched: the URLs curl was asked for, in order.
fetched() { awk '$1 == "curl" { print $2 }' <<< "$calls"; }

# The releases. 1.2.5's program reports 1.2.6, and 1.2.6's doesn't run. 1.2.7's x86_64 Linux
# archive is whole, but SHA256SUMS gives it another sum; 1.2.8's SHA256SUMS has no line for it, and
# 1.2.11's has two. 1.2.12's archives hold a fenji.toml and a data folder. 1.3.1's Linux archives
# hold a unit that isn't Fenji's, a fenji.service everyone may write and a program anyone may.
release 1.3.0
release 1.2.4
release 1.2.10
program 1.2.5 1.2.6 > "$T/liar"
release 1.2.5 "$T/liar"
printf '#!/nonexistent/sh\n' > "$T/broken"
release 1.2.6 "$T/broken"
release 1.2.7
awk -v f="fenji-server-1.2.7-$L.tar.gz" '$2 == f { $1 = "00000000000000000000000000000000000000000000000000000000000000ff" } { print $1 "  " $2 }' \
  "$T/release/v1.2.7/SHA256SUMS" > "$T/sums" && mv "$T/sums" "$T/release/v1.2.7/SHA256SUMS"
release 1.2.8
grep -v "$L" "$T/release/v1.2.8/SHA256SUMS" > "$T/sums" && mv "$T/sums" "$T/release/v1.2.8/SHA256SUMS"
release 1.2.11
grep "fenji-server-1.2.11-$L.tar.gz" "$T/release/v1.2.11/SHA256SUMS" > "$T/line" && cat "$T/line" >> "$T/release/v1.2.11/SHA256SUMS"
HOSTILE=yes release 1.2.12
ODD_UNITS=yes release 1.3.1
A=$T/stage/fenji-server-1.3.0-$L

# A server's folder, with a space in its path, as a home folder may have.
D="$T/home with space/fenji-server"

# The folder: the latest release, on a computer whose fenji service runs another program.
fresh
folder "$D" 1.2.3
export FAKE_UNIT=/usr/local/bin/fenji-server FAKE_UNIT_CONFIG=/etc/fenji/fenji.toml
export FAKE_UNIT_FRAGMENT=/etc/systemd/system/fenji.service
run "$D/update-server.sh"
exits "a server's folder updates to the latest release" 0
same "its program is the latest release's" "$(version "$D/fenji-server")" "fenji-server 1.3.0"
same "the old program stays, as fenji-server.previous" "$(version "$D/fenji-server.previous")" "fenji-server 1.2.3"
for f in start-server.sh update-server.sh $TEXTS systemd/fenji.service systemd/fenji-backup.service \
  systemd/fenji-backup.timer caddy/Caddyfile; do
  holds "the archive's $f replaces the folder's" cmp -s "$D/$f" "$A/$f"
done
holds "the start script and the update script can still run" test -x "$D/start-server.sh" -a -x "$D/update-server.sh"
untouched "fenji.toml and the data folder are untouched" "$D/fenji.toml" "$D/data"
same "nothing is left under a temporary name, nor a lock" "$(find "$D" -name '.*' -print)" ""
same "nothing else is added to the folder" "$(entries "$D")" "$(printf '%s\n' LICENSE README.txt SOURCE.txt \
  THIRD-PARTY-NOTICES.txt caddy data fenji-server fenji-server.previous fenji.example.toml fenji.toml start-server.sh \
  systemd update-server.sh)"
same "it fetches where the latest release leads, then its archive and SHA256SUMS" "$(fetched)" "$(printf '%s\n' \
  "$FAKE_RELEASES/latest" "$FAKE_RELEASES/download/v1.3.0/fenji-server-1.3.0-$L.tar.gz" \
  "$FAKE_RELEASES/download/v1.3.0/SHA256SUMS")"
same "each over HTTPS alone, redirects too, failing on an HTTP error" \
  "$(awk '$1 == "curl" && !(/ --proto =https/ && / --proto-redir =https/ && / -[a-zA-Z]*f/)' <<< "$calls")" ""
same "each with -q first, so that no .curlrc applies" "$(awk '$1 == "curl" && $3 != "first=-q"' <<< "$calls")" ""
asked "it asks the folder's own server whether it's running, in the folder" \
  "fenji-server 1.2.3 as $uid in $D: console --run online"
before "before it downloads anything" "console --run online" "curl $FAKE_RELEASES/download/"
asked "the backup is made by the old program, in the folder" "fenji-server 1.2.3 as $uid in $D: backup"
asked "the new program is checked as the folder's owner" "fenji-server 1.3.0 as $uid in "
before "once the download has checked out" "fenji-server 1.3.0" "fenji-server 1.2.3 as $uid in $D: backup"
says "it says where it updates, from and to which version" "Updating the server in $D from 1.2.3 to 1.3.0." "$out"
says "it says the latest release" "The latest release is 1.3.0." "$out"
says "it says the backup's name" "Backed up the database: data/backups/fenji-20261017-101500.db" "$out"
says "it ends with the versions, and how to start the server" "Updated from 1.2.3 to 1.3.0." "$out"
says "its start script" "start-server.sh" "$(tail -n 1 <<< "$out")"
same "it says nothing on stderr" "$err" ""

# A named version, with the tag's v, by a relative path with CDPATH set, without systemd.
fresh
folder "$D" 1.2.3
export FAKE_UNIT=nosystemd
mkdir -p "$T/decoy/home with space/fenji-server"
cp "$D/fenji-server" "$D/start-server.sh" "$T/decoy/home with space/fenji-server/"
FROM=$T RUN_CDPATH=$T/decoy run "home with space/fenji-server/update-server.sh" v1.2.4
exits "a named version updates the folder, the one it's in, not one CDPATH names" 0
same "its program is that version's" "$(version "$D/fenji-server")" "fenji-server 1.2.4"
same "the latest release isn't asked for" "$(fetched)" "$(printf '%s\n' \
  "$FAKE_RELEASES/download/v1.2.4/fenji-server-1.2.4-$L.tar.gz" "$FAKE_RELEASES/download/v1.2.4/SHA256SUMS")"
same "CDPATH's folder is untouched" "$(entries "$T/decoy/home with space/fenji-server")" "$(printf '%s\n' fenji-server start-server.sh)"

# Updated again, by the update script the last update brought: one previous program, the last.
run "$D/update-server.sh"
exits "an update after an update works, with the update script it brought" 0
same "its program is the latest's" "$(version "$D/fenji-server")" "fenji-server 1.3.0"
same "fenji-server.previous is the one it replaced" "$(version "$D/fenji-server.previous")" "fenji-server 1.2.4"
same "and there's no other copy" "$(find "$D" -mindepth 1 -name 'fenji-server*' | sed 's|.*/||' | LC_ALL=C sort)" \
  "$(printf '%s\n' fenji-server fenji-server.previous)"

# fenji-server.previous is already fenji-server's own file, as when a host went back with ln: the
# update still goes through, and this one keeps the program it replaces.
fresh
folder "$D" 1.2.3
ln -f "$D/fenji-server" "$D/fenji-server.previous"
run "$D/update-server.sh"
exits "a fenji-server.previous that's fenji-server's own file is no wedge" 0
same "its program is the latest's" "$(version "$D/fenji-server")" "fenji-server 1.3.0"
same "and fenji-server.previous the one it replaced" "$(version "$D/fenji-server.previous")" "fenji-server 1.2.3"
same "with nothing left under a temporary name" "$(find "$D" -name '.*' -print)" ""

# The same version, or an older one: nothing changes.
fresh
folder "$D" 1.2.3
was=$(snapshot "$D")
run "$D/update-server.sh" 1.2.3
exits "the version it runs already is no update" 0
says "it says so" "nothing to update" "$out"
as_was "and the folder is as it was" "$D" "$was"
same "with nothing downloaded" "$(fetched)" ""
export FAKE_LATEST=1.2.3
run "$D/update-server.sh"
exits "the latest release, when it runs that already, is no update" 0
says "it says so" "nothing to update" "$out"
as_was "and the folder is as it was" "$D" "$was"
same "with only where the latest leads fetched" "$(fetched)" "$FAKE_RELEASES/latest"
fresh
run "$D/update-server.sh" 1.2.2
exits "an older version is refused" 1
says "saying it's older" "1.2.2 is older than 1.2.3" "$err"
says "and that a backup made with it is the way back" "backup made with 1.2.2" "$err"
as_was "the folder is as it was" "$D" "$was"
same "with nothing downloaded" "$(fetched)" ""

# Versions are compared as SemVer says, not as text.
for pair in "1.2.10 1.2.9" "1.3.0 1.3.0-rc.1" "1.3.0-rc.10 1.3.0-rc.2" "1.3.0-rc.1 1.3.0-beta"; do
  fresh
  folder "$D" "${pair% *}"
  run "$D/update-server.sh" "${pair#* }"
  exits "${pair#* } is older than ${pair% *}, and refused" 1
  says "saying so" "is older than" "$err"
done
fresh
folder "$D" 1.2.9
run "$D/update-server.sh" 1.2.10
exits "1.2.10 is newer than 1.2.9" 0
same "and updates the folder" "$(version "$D/fenji-server")" "fenji-server 1.2.10"
fresh
folder "$D" 1.3.0-rc.1
run "$D/update-server.sh"
exits "1.3.0 is newer than its release candidate" 0
same "and updates the folder" "$(version "$D/fenji-server")" "fenji-server 1.3.0"

# Nothing changes until the download checks out, and the backup is made only then.
# refused WHAT PATTERN: the last run exited 1 with PATTERN on stderr, left the folder as $was says,
# and made no backup.
refused() {
  exits "$1" 1
  says "saying why" "$2" "$err"
  as_was "the folder is as it was" "$D" "$was"
  never "and nothing was backed up" "backup"
}
fresh
folder "$D" 1.2.3
was=$(snapshot "$D")
run "$D/update-server.sh" 1.2.7
refused "a whole archive SHA256SUMS gives another sum is refused" "doesn't match SHA256SUMS"
run "$D/update-server.sh" 1.2.8
refused "an archive SHA256SUMS has no line for is refused" "SHA256SUMS has no line for fenji-server-1.2.8-$L.tar.gz"
run "$D/update-server.sh" 1.2.11
refused "an archive SHA256SUMS has two lines for is refused" "SHA256SUMS has 2 lines for fenji-server-1.2.11-$L.tar.gz"
run "$D/update-server.sh" 9.9.9
refused "a version with no release is refused" "can't download"
run "$D/update-server.sh" 1.2.5
refused "a program that reports another version is refused" "1.2.6, not 1.2.5"
run "$D/update-server.sh" 1.2.6
refused "a program that doesn't run is refused" "doesn't run"
export FAKE_LATEST=
run "$D/update-server.sh"
refused "a latest release it can't tell is refused" "can't tell the latest release"
fresh
export FAKE_BACKUP=fail
run "$D/update-server.sh"
exits "a backup that fails stops the update" 1
says "saying so" "the backup failed" "$err"
as_was "the folder is as it was" "$D" "$was"

# A server running from the folder: the script can't stop it, so it refuses.
fresh
: > "$FAKE_RUNNING"
run "$D/update-server.sh"
refused "a folder whose server runs is refused" "type stop in its console"
never "before anything is downloaded" "$FAKE_RELEASES/download/"
fresh

# Another update running in the folder, or one cut short: its lock stops this one.
mkdir "$D/.fenji-update.lock"
was=$(snapshot "$D")
run "$D/update-server.sh"
refused "a folder another update holds is refused" "another update of this server is running"
says "saying how to lift a lock left by one cut short, the path quoted" "rmdir '$D/.fenji-update.lock'" "$err"
never "before anything is downloaded" "$FAKE_RELEASES/download/"
rmdir "$D/.fenji-update.lock"
was=$(snapshot "$D")

# A file of the archive where the folder has a folder: nothing changes.
rm -- "$D/LICENSE"
mkdir "$D/LICENSE"
printf 'mine\n' > "$D/LICENSE/notes"
was=$(snapshot "$D")
run "$D/update-server.sh"
refused "a folder where the archive has a file is refused" "$D/LICENSE is a folder, where the new version has a file"

# A rename that fails after the program's: the update says so, loudly, and how to go back.
fresh
folder "$D" 1.2.3
export FAKE_MV_FAIL=README.txt
run "$D/update-server.sh"
exits "a file that can't replace its old one fails the update" 1
says "naming it" "couldn't replace the old ones: README.txt" "$err"
says "and how to go back, the paths quoted" "Go back to 1.2.3 (mv '$D/fenji-server.previous' '$D/fenji-server')" "$err"
same "the new program is in place" "$(version "$D/fenji-server")" "fenji-server 1.3.0"
same "with nothing left under a temporary name" "$(find "$D" -name '.*' -print)" ""

# A folder that has never started its server has no settings, and no database to back up.
fresh
folder "$D" 1.2.3
rm -- "$D/fenji.toml"
run "$D/update-server.sh"
exits "a folder without fenji.toml updates" 0
never "with no backup" "backup"
says "saying there's no database yet" "no database to back up" "$out"
same "and no fenji.toml made" "$(ls "$D/fenji.toml" 2> /dev/null)" ""

# An archive with a fenji.toml and a data folder of its own: neither is installed (HB8).
fresh
folder "$D" 1.2.3
cp "$D/fenji.toml" "$T/fenji.toml.host"
run "$D/update-server.sh" 1.2.12
exits "an archive with a fenji.toml and a data folder updates" 0
same "its program is that version's" "$(version "$D/fenji-server")" "fenji-server 1.2.12"
untouched "the folder's fenji.toml and data folder are untouched" "$D/fenji.toml" "$D/data"
holds "fenji.toml is the host's own" cmp -s "$D/fenji.toml" "$T/fenji.toml.host"
same "and the data folder holds what it held" "$(cd "$D/data" && find . | LC_ALL=C sort)" \
  "$(printf '%s\n' . ./backups ./fenji.db ./tls ./tls/key.pem)"

# Who runs it, and where (HB8).
fresh
folder "$D" 1.2.3
was=$(snapshot "$D")
export FAKE_UID=0
run "$D/update-server.sh"
refused "root is refused in a folder it doesn't own" "run it as $me"
same "before anything is fetched" "$(fetched)" ""
export FAKE_UID=4242
run "$D/update-server.sh"
refused "another user is refused" "run it as $me"
fresh
if [ "$uid" = 0 ]; then
  echo "skip a folder it can't write in is refused: root writes in a read-only folder"
else
  chmod a-w "$D/systemd"
  was=$(snapshot "$D")
  run "$D/update-server.sh"
  refused "a folder it can't write in is refused" "can't write in $D/systemd"
  chmod u+w "$D/systemd"
  chmod a-w "$D"
  was=$(snapshot "$D")
  run "$D/update-server.sh"
  refused "and a server's folder it can't write in, saying so rather than that another update runs" "can't write in $D,"
  chmod u+w "$D"
fi
folder "$D" 1.2.3
rm -- "$D/start-server.sh"
was=$(snapshot "$D")
run "$D/update-server.sh"
refused "a folder without its start script, and no service, is refused" "has no start-server.sh"
same "before anything is fetched" "$(fetched)" ""
# As fenji-update on a VPS, run without sudo where systemd won't answer this user (no D-Bus).
says "saying that a VPS's service takes sudo, the path quoted" "run it as root: sudo '$D/update-server.sh'" "$err"

# Root in a folder that's root's: allowed, if only root may write in it and its subfolders.
fresh
folder "$D" 1.2.3
export FAKE_UID=0 FAKE_ROOT_UID=$uid
run "$D/update-server.sh"
exits "root updates a folder that's root's" 0
same "its program is the latest's" "$(version "$D/fenji-server")" "fenji-server 1.3.0"
fresh
folder "$D" 1.2.3
chmod g+w "$D"
was=$(snapshot "$D")
export FAKE_UID=0 FAKE_ROOT_UID=$uid
run "$D/update-server.sh"
refused "root is refused in a folder of root's that its group may write in" "won't write as root in $D,"
says "saying how to make it safe, the path quoted" "sudo chown root '$D' && sudo chmod go-w '$D'" "$err"
# A path with a single quote in it is quoted so that a pasted command stays on it.
fresh
Q="$T/it's here/fenji-server"
folder "$Q" 1.2.3
chmod g+w "$Q"
export FAKE_UID=0 FAKE_ROOT_UID=$uid
run "$Q/update-server.sh"
exits "root is refused in a folder with a quote in its path that its group may write in" 1
says "with the quote escaped in the advice" \
  "sudo chown root '$T/it'\\''s here/fenji-server' && sudo chmod go-w '$T/it'\\''s here/fenji-server'" "$err"
fresh
folder "$D" 1.2.3
chmod o+w "$D/caddy"
was=$(snapshot "$D")
export FAKE_UID=0 FAKE_ROOT_UID=$uid
run "$D/update-server.sh"
refused "and in one with a subfolder everyone may write in" "won't write as root in $D/caddy,"
fresh
folder "$D" 1.2.3
chmod o+w "$T/home with space"
was=$(snapshot "$D")
export FAKE_UID=0 FAKE_ROOT_UID=$uid
run "$D/update-server.sh"
refused "and in one whose parent everyone may write in" "won't write as root in $D: $T/home with space, above it,"
says "saying to make that parent, which is root's, writable by root alone" "sudo chmod go-w '$T/home with space'" "$err"
unsaid "and not to give it to root, which it is already" "sudo chown root '$T/home with space'" "$err"
chmod o-w "$T/home with space"
# A parent that isn't root's, as a home folder isn't: giving it to root would break it, so the
# advice is to move the server's folder, or give it to that folder's user and run this as them.
fresh
folder "$D" 1.2.3
was=$(snapshot "$D")
export FAKE_UID=0 FAKE_ROOT_UID=$uid FAKE_NOT_ROOTS="$T/home with space"
run "$D/update-server.sh"
refused "and in one whose parent isn't root's" "won't write as root in $D: $T/home with space, above it, is $me's"
says "saying to move it, or give it to the parent's user" \
  "give it to $me (sudo chown -R '$me' '$D') and run this as $me, without sudo" "$err"
unsaid "and never to give that parent to root" "sudo chown root '$T/home with space'" "$err"

# Each system's archive.
fresh
export FAKE_UNAME_S=Darwin FAKE_UNAME_M=arm64
folder "$D" 1.2.3 start-server.command
run "$D/update-server.sh"
exits "macOS on Apple silicon updates" 0
says "from the macOS archive" "fenji-server-1.3.0-aarch64-apple-darwin.tar.gz" "$(fetched)"
holds "its start-server.command is replaced" cmp -s "$D/start-server.command" "$T/stage/fenji-server-1.3.0-aarch64-apple-darwin/start-server.command"
same "and no folders are added" "$(find "$D" -type d | sed "s|^$D||" | LC_ALL=C sort)" "$(printf '%s\n' "" /data /data/backups /data/tls)"
says "it says how to start the server" "start-server.command" "$(tail -n 1 <<< "$out")"
fresh
export FAKE_UNAME_M=aarch64
folder "$D" 1.2.3
run "$D/update-server.sh"
exits "Linux on 64-bit ARM updates" 0
says "from its archive" "fenji-server-1.3.0-aarch64-unknown-linux-musl.tar.gz" "$(fetched)"
for system in "FreeBSD amd64" "Linux armv7l" "Linux i686" "Darwin x86_64"; do
  fresh
  export FAKE_UNAME_S=${system% *} FAKE_UNAME_M=${system#* }
  was=$(snapshot "$D")
  run "$D/update-server.sh"
  refused "$system is refused" "no Fenji server for ${system% *} on ${system#* }"
  same "before anything is fetched" "$(fetched)" ""
done

# Its arguments.
fresh
was=$(snapshot "$D")
run "$D/update-server.sh" 1.2.4 1.3.0
exits "two versions are a usage error" 2
run "$D/update-server.sh" banana
exits "a word that isn't a version is a usage error" 2
says "saying so" "not a version" "$err"
run "$D/update-server.sh" 1.2
exits "a version of two numbers is a usage error" 2
run "$D/update-server.sh" -x
exits "an option it doesn't know is a usage error" 2
run "$D/update-server.sh" --help
exits "--help is no error" 0
says "and says how to use it" "usage:" "$out"
FENJI_RELEASES=http://releases.example/fenji/releases run "$D/update-server.sh"
exits "releases over plain http are refused" 2
says "saying they must be https" "https://" "$err"
as_was "and the folder is as it was" "$D" "$was"
same "with nothing fetched" "$(fetched)" ""

# The service: the reference VPS, where section 4 installs the script as fenji-update.
fresh
service 1.2.3
run "$B/fenji-update"
exits "the service updates to the latest release" 0
same "its program is the latest release's" "$(version "$B/fenji-server")" "fenji-server 1.3.0"
same "the old program stays, as fenji-server.previous" "$(version "$B/fenji-server.previous")" "fenji-server 1.2.3"
for u in $UNITS; do
  holds "the archive's $u replaces the installed one" cmp -s "$S/etc/systemd/system/$u" "$A/systemd/$u"
  holds "which stays as $u.previous" cmp -s "$S/etc/systemd/system/$u.previous" <(unit "$u" 1.2.3)
done
holds "the archive's update script replaces fenji-update" cmp -s "$B/fenji-update" "$A/update-server.sh"
holds "which can still run" test -x "$B/fenji-update"
untouched "fenji.toml, the data folder, the drop-in, another unit and Caddy's file are untouched" "$S/etc/fenji" \
  "$S/var/lib/fenji" "$S/etc/systemd/system/fenji.service.d" "$S/etc/systemd/system/other.service" "$S/etc/caddy"
same "nothing else goes beside the program" "$(entries "$B")" "$(printf '%s\n' fenji-server fenji-server.previous fenji-update)"
same "nothing else goes beside the units" "$(entries "$S/etc/systemd/system")" "$(printf '%s\n' fenji-backup.service \
  fenji-backup.service.previous fenji-backup.timer fenji-backup.timer.previous fenji.service fenji.service.d \
  fenji.service.previous other.service)"
same "nothing is left under a temporary name, nor a lock" "$(find "$S" -name '.*' -print)" ""
asked "the backup is the console's, as the service's user, with its --config" \
  "sudo -u fenji -- $B/fenji-server --config $S/etc/fenji/fenji.toml console --run backup"
asked "run from /, as systemd runs the server" \
  "fenji-server 1.2.3 as 990 in /: --config $S/etc/fenji/fenji.toml console --run backup"
asked "the new program is checked as the service's user, never root" "fenji-server 1.3.0 as 990 in /: --version"
asked "and so is the old one" "fenji-server 1.2.3 as 990 in /: --version"
never "no fenji-server runs as root" " as 0 in "
before "made while the server runs" "console --run backup" "systemctl stop fenji"
before "once the download has checked out" "fenji-server 1.3.0" "console --run backup"
asked "the service is stopped, running the old program" "systemctl stop fenji (fenji-server 1.2.3)"
asked "systemd reloads the new units" "systemctl daemon-reload (# fenji.service of release 1.3.0)"
asked "the service starts again, with the new program" "systemctl start fenji (fenji-server 1.3.0)"
before "the units are reloaded before it starts" "systemctl daemon-reload" "systemctl start fenji"
before "and it waits for the new server's console to answer" "systemctl start fenji" "console --run online"
asked "each look at its console taking 5 seconds at most" \
  "timeout 5 sudo -u fenji -- $B/fenji-server --config $S/etc/fenji/fenji.toml console --run online"
says "it says what it updates, from and to which version" "Updating the fenji service, which runs $B/fenji-server, from 1.2.3 to 1.3.0." "$out"
says "the backup's name" "Backed up the database: /var/lib/fenji/backups/fenji-20261017-101500.db" "$out"
says "the old units' names" "kept the old ones as <unit>.previous" "$out"
says "that the server runs the new version" "fenji is running 1.3.0." "$out"
says "and ends with the versions" "Updated from 1.2.3 to 1.3.0." "$(tail -n 1 <<< "$out")"
same "it says nothing on stderr" "$err" ""

# A named version, for a service whose User is unset: its server runs as root, and so does its
# console's backup, without sudo; the programs' versions are still checked as nobody. The units,
# whose User is fenji, aren't this service's, and stay.
fresh
service 1.2.3
export FAKE_UNIT_USER=''
run "$B/fenji-update" 1.2.4
exits "the service updates to a named version" 0
same "its program is that version's" "$(version "$B/fenji-server")" "fenji-server 1.2.4"
asked "the backup is the console's, as root, from /" \
  "fenji-server 1.2.3 as 0 in /: --config $S/etc/fenji/fenji.toml console --run backup"
never "with no sudo" "sudo -u root"
asked "the new program is checked as nobody" "fenji-server 1.2.4 as 65534 in /: --version"
# Its console's socket is root's alone, so the new server's console is asked as root, but only once
# systemd has started that server, as root.
before "the new program runs as root only once systemd has started it so" "systemctl start fenji" "fenji-server 1.2.4 as 0 "
same "and then only to ask its console whether it's up" \
  "$(grep 'fenji-server 1.2.4 as 0 ' <<< "$calls" | grep -vc ': --config [^ ]* console --run online$')" 0

# A fenji service that runs the program from a link to it is still the service it updates.
fresh
service 1.2.3
mkdir -p "$S/opt"
ln -s "$B/fenji-server" "$S/opt/fenji-server"
export FAKE_UNIT=$S/opt/fenji-server
run "$B/fenji-update"
exits "a service whose ExecStart runs a link to the program updates" 0
says "as the service" "Updating the fenji service" "$out"
same "its program is the latest's" "$(version "$B/fenji-server")" "fenji-server 1.3.0"

# A host's own fenji service, not Fenji's unit: the program is updated, and the units stay.
fresh
service 1.2.3
printf '[Service]\nUser=wen\nExecStart=%s --config %s run\n' "$B/fenji-server" "$S/etc/fenji/fenji.toml" \
  > "$S/etc/systemd/system/fenji.service"
age "$S"
export FAKE_UNIT_USER=wen
run "$B/fenji-update"
exits "a host's own fenji service updates" 0
same "its program is the latest's" "$(version "$B/fenji-server")" "fenji-server 1.3.0"
untouched "its units' folder is untouched" "$S/etc/systemd/system"
says "saying the units stay" "Left the units in $S/etc/systemd/system as they are" "$out"
never "with no reload" "daemon-reload"
asked "and the programs are checked as its user" "sudo -u wen -- $B/.fenji-update."
# The same with Fenji's user but another command line than Fenji's unit's: its settings elsewhere.
fresh
service 1.2.3
export FAKE_UNIT_CONFIG=/srv/moonward/fenji.toml
run "$B/fenji-update"
exits "a fenji service of the fenji user's, with its settings elsewhere, updates" 0
untouched "its units' folder is untouched" "$S/etc/systemd/system"
says "saying the units stay" "Left the units in $S/etc/systemd/system as they are: the fenji service runs $B/fenji-server --config /srv/moonward/fenji.toml run as fenji" "$out"

# A service that doesn't come up shows its journal, and how to go back (HB9).
fresh
service 1.2.3
export FAKE_START=fail
run "$B/fenji-update"
exits "a service that doesn't come up fails the update" 1
says "saying so" "didn't come up" "$err"
says "with the journal's last lines" "the stand-in server won't start" "$err"
asked "for the fenji service" "journalctl -u fenji"
says "and how to go back: the previous program, the paths quoted" "sudo mv '$B/fenji-server.previous' '$B/fenji-server'" "$err"
for u in $UNITS; do
  says "the previous $u" "sudo mv '$S/etc/systemd/system/$u.previous' '$S/etc/systemd/system/$u'" "$err"
done
says "systemd reloaded" "sudo systemctl daemon-reload" "$err"
says "and the backup" "/var/lib/fenji/backups/fenji-20261017-101500.db" "$err"
same "it waits up to 30 seconds" "$(grep -c '^sleep 1$' <<< "$calls")" 29
same "the new program stays in place" "$(version "$B/fenji-server")" "fenji-server 1.3.0"
same "beside the previous one" "$(version "$B/fenji-server.previous")" "fenji-server 1.2.3"

# A service that systemd has active all along, but whose console never answers, may be up: the
# update says so, rather than that it didn't come up.
fresh
service 1.2.3
export FAKE_START=noconsole
run "$B/fenji-update"
exits "a service active all along whose console never answers isn't called a success" 1
says "saying it's active, and its console didn't answer" "fenji is active, but its console didn't answer within 30 seconds" "$err"
unsaid "not that it didn't come up" "didn't come up within" "$err"
says "and how to go back, if it isn't working" "If they can't:" "$err"

# A stopped service is backed up by the program itself, and stays stopped.
fresh
service 1.2.3
echo inactive > "$T/state/active"
rm -f -- "$FAKE_RUNNING"
run "$B/fenji-update"
exits "a stopped service updates" 0
asked "backed up by the program, as the service's user" "sudo -u fenji -- $B/fenji-server --config $S/etc/fenji/fenji.toml backup"
never "not stopped again" "systemctl stop"
never "nor started" "systemctl start"
says "saying it stays stopped" "stays stopped" "$out"
same "its program is the new one" "$(version "$B/fenji-server")" "fenji-server 1.3.0"

# TERM, INT and HUP while the service stops: the swap goes on to its end.
fresh
service 1.2.3
export FAKE_STOP=signals
run "$B/fenji-update"
exits "signals during the swap don't stop it" 0
same "its program is the latest's" "$(version "$B/fenji-server")" "fenji-server 1.3.0"
says "and it ends with the versions" "Updated from 1.2.3 to 1.3.0." "$(tail -n 1 <<< "$out")"

# A --config that isn't a full path is read where systemd runs the server: its WorkingDirectory.
fresh
service 1.2.3
export FAKE_UNIT_CONFIG=fenji.toml FAKE_UNIT_WORKDIR="!$S/etc/fenji"
run "$B/fenji-update"
exits "a service with a --config in its WorkingDirectory updates" 0
asked "its backup run there" "fenji-server 1.2.3 as 990 in $S/etc/fenji: --config fenji.toml console --run backup"

# The service, unchanged: anything that fails before the swap leaves it running as it was.
# kept WHAT PATTERN: the last run exited 1 with PATTERN on stderr, left the VPS as $was says, and
# never stopped the service.
kept() {
  exits "$1" 1
  says "saying why" "$2" "$err"
  as_was "everything is as it was" "$S" "$was"
  never "and the service wasn't stopped" "systemctl stop"
}
fresh
service 1.2.3
was=$(snapshot "$S")
export FAKE_UID=1000
run "$B/fenji-update"
kept "the service needs root" "sudo"
same "before anything is fetched" "$(fetched)" ""
export FAKE_UID=0
run "$B/fenji-update" 1.2.7
kept "an archive SHA256SUMS gives another sum is refused" "doesn't match SHA256SUMS"
never "with no backup" "backup"
export FAKE_BACKUP=fail
run "$B/fenji-update"
kept "a backup that fails stops the update" "backup"
export FAKE_BACKUP='' FAKE_LATEST=1.2.3
run "$B/fenji-update"
exits "the version it runs already is no update" 0
says "saying so" "nothing to update" "$out"
as_was "everything is as it was" "$S" "$was"
never "and the service wasn't stopped" "systemctl stop"
export FAKE_LATEST=1.3.0
echo activating > "$T/state/active"
run "$B/fenji-update"
kept "a service that's starting is refused" "activating"
echo active > "$T/state/active"
export FAKE_UNIT_CONFIG=fenji.toml FAKE_UNIT_WORKDIR='~'
run "$B/fenji-update"
kept "a --config in a WorkingDirectory it can't follow is refused" "WorkingDirectory, ~"
export FAKE_UNIT_CONFIG=$S/etc/fenji/fenji.toml FAKE_UNIT_WORKDIR=''
mkdir "$B/.fenji-update.lock"
was=$(snapshot "$S")
run "$B/fenji-update"
kept "a service another update holds is refused" "another update of this server is running"
rmdir "$B/.fenji-update.lock"

# As root, it writes only in folders that are root's, and that only root may write in.
fresh
service 1.2.3
chmod g+w "$B"
was=$(snapshot "$S")
run "$B/fenji-update"
kept "a program's folder its group may write in is refused" "won't write as root in $B,"
same "before anything is fetched" "$(fetched)" ""
fresh
service 1.2.3
export FAKE_ROOT_UID=''
was=$(snapshot "$S")
run "$B/fenji-update"
kept "a program's folder that isn't root's is refused" "won't write as root in $B,"
fresh
service 1.2.3
chmod o+w "$S/etc/systemd/system"
was=$(snapshot "$S")
run "$B/fenji-update"
kept "a units' folder everyone may write in is refused" "won't write as root in $S/etc/systemd/system,"
never "with no backup" "backup"

# And every folder above one root writes in must be root's, and either writable by root alone or
# sticky, as /tmp is: otherwise someone else could move the folder aside, and put theirs in its place.
fresh
service 1.2.3
chmod o+w "$S/usr/local"
was=$(snapshot "$S")
run "$B/fenji-update"
kept "a program's folder whose parent everyone may write in is refused" "won't write as root in $B: $S/usr/local, above it,"
says "saying to make that parent, which is root's, writable by root alone" "sudo chmod go-w '$S/usr/local'" "$err"
unsaid "and not to give it to root, which it is already" "sudo chown root '$S/usr/local'" "$err"
same "before anything is fetched" "$(fetched)" ""
fresh
service 1.2.3
chmod o+w "$S/usr"
was=$(snapshot "$S")
run "$B/fenji-update"
kept "a program's folder with a folder two above it everyone may write in is refused" "won't write as root in $B: $S/usr, above it,"
fresh
service 1.2.3
chmod g+w "$S/etc/systemd"
was=$(snapshot "$S")
run "$B/fenji-update"
kept "a units' folder whose parent its group may write in is refused" \
  "won't write as root in $S/etc/systemd/system: $S/etc/systemd, above it,"
never "with no backup" "backup"
fresh
service 1.2.3
chmod 1777 "$S/usr/local"
run "$B/fenji-update"
exits "a program's folder whose parent is root's and sticky, as /tmp is, is fine" 0
same "and it updates" "$(version "$B/fenji-server")" "fenji-server 1.3.0"
fresh
service 1.2.3
chmod 1777 "$S/usr/local"
export FAKE_NOT_ROOTS=$S/usr/local
was=$(snapshot "$S")
run "$B/fenji-update"
kept "but a sticky parent that's another user's is refused" "won't write as root in $B: $S/usr/local, above it,"
says "saying to keep the service's files in folders that are root's all the way up" \
  "Keep the service's program and units in folders that are root's all the way up to /" "$err"
unsaid "and never to give that parent to root" "sudo chown root '$S/usr/local'" "$err"
export FAKE_NOT_ROOTS=''
# A units' folder that's gone, where systemd loaded the unit from, is no folder to write in.
fresh
service 1.2.3
export FAKE_UNIT_FRAGMENT=$S/etc/gone/fenji.service
was=$(snapshot "$S")
run "$B/fenji-update"
kept "a units' folder that's gone is refused" "won't write as root in $S/etc/gone, which is missing"

# A stop that fails: the service starts again, nothing has changed, and the update says so.
fresh
service 1.2.3
was=$(snapshot "$S")
export FAKE_STOP=fail
run "$B/fenji-update"
exits "a stop that fails fails the update" 1
says "saying so" "can't stop the fenji service, so nothing has changed, and it runs on as it was" "$err"
before "starting the service again" "systemctl stop fenji" "systemctl start fenji"
as_was "everything is as it was" "$S" "$was"

# Only Fenji's three units are installed, by name, and with root's modes, whatever the archive holds.
# A umask of 000 keeps the archive's modes as it's unpacked, as root's tar does.
fresh
service 1.2.3
RUN_UMASK=000 run "$B/fenji-update" 1.3.1
exits "an archive with a unit that isn't Fenji's updates" 0
same "only Fenji's three units are installed" "$(entries "$S/etc/systemd/system")" "$(printf '%s\n' fenji-backup.service \
  fenji-backup.service.previous fenji-backup.timer fenji-backup.timer.previous fenji.service fenji.service.d \
  fenji.service.previous other.service)"
says "saying what it left out" "Left out extra.service" "$out"
holds "fenji.service is 1.3.1's" cmp -s "$S/etc/systemd/system/fenji.service" "$T/stage/fenji-server-1.3.1-$L/systemd/fenji.service"
same "a unit everyone could write in the archive is installed 0644" "$(mode "$S/etc/systemd/system/fenji.service")" "-rw-r--r--"
same "a program anyone could write in the archive is installed 0755" "$(mode "$B/fenji-server")" "-rwxr-xr-x"
same "and the update script 0755" "$(mode "$B/fenji-update")" "-rwxr-xr-x"

if [ -z "$dirty" ]; then ok "every run cleaned up its temporary folder"; else bad "these runs left temporary files$dirty"; fi

[ "$fails" = 0 ] || { echo "$fails failed"; exit 1; }
echo "all passed"
