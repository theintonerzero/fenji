#!/usr/bin/env bash
# Tests for scripts/package-server.sh (M4 hosting spec §3.7): each archive's listing, its modes and
# its texts, from a fake binary and with a stand-in for cargo, so there's no build and no network.
# Needs tar, gzip, zip and unzip. Runs on Linux and macOS (bash 3.2).
#   scripts/test-package-server.sh
set -uo pipefail
export LC_ALL=C
unset CDPATH
HERE=$(cd -- "$(dirname -- "$0")" && pwd -P)
REPO=$(cd -- "$HERE/.." && pwd -P)
PACK=$HERE/package-server.sh
fails=0
ok() { echo "ok   $1"; }
bad() { echo "FAIL $1"; fails=$((fails + 1)); }

for tool in tar gzip zip unzip; do
  command -v "$tool" > /dev/null || { echo "FATAL: the test needs $tool" >&2; exit 2; }
done

T=$(mktemp -d) || { echo "FATAL: mktemp failed" >&2; exit 2; }
T=$(cd -- "$T" && pwd -P) || exit 2
trap 'rm -rf -- "$T"' EXIT
case "$T" in /?*) ;; *) echo "FATAL: no temporary directory" >&2; exit 2 ;; esac

# A stand-in for cargo, first on PATH, for its two subcommands here. It writes the directory it ran
# in and its arguments, one a line, to $T/cargo-args.<subcommand>, with the file after -o as OUT.
# - `cargo tree` prints fenji-server's dependency tree: with fenji-oodle in it when FAKE_OODLE=yes,
#   with only a crate whose name starts the same when FAKE_OODLE=prefix, and failing as cargo does
#   when FAKE_OODLE=fail.
# - `cargo about` writes $T/notices to the file after -o, or fails as cargo-about does when
#   FAKE_ABOUT=fail.
mkdir "$T/bin"
cat > "$T/bin/cargo" <<'EOF'
#!/bin/sh
out=
prev=
for a in "$@"; do
  if [ "$prev" = -o ]; then out=$a; fi
  prev=$a
done
prev=
{
  pwd -P
  for a in "$@"; do
    if [ "$prev" = -o ]; then echo OUT; else printf '%s\n' "$a"; fi
    prev=$a
  done
} > "$FAKE_CARGO_ARGS.$1"
case "$1" in
  tree)
    case "${FAKE_OODLE:-no}" in
      fail) echo "error: failed to download from \`https://index.crates.io/\`" >&2; exit 101 ;;
    esac
    echo "fenji-server v0.0.0 (/fenji/crates/fenji-server)"
    echo "tokio v1.0.0"
    case "${FAKE_OODLE:-no}" in
      yes) echo "fenji-oodle v0.0.0 (/fenji/crates/fenji-oodle)" ;;
      prefix) echo "fenji-oodle-helper v0.0.0 (/fenji/tools/oodle-helper)" ;;
    esac ;;
  about)
    if [ "${FAKE_ABOUT:-}" = fail ]; then
      echo "error: failed to satisfy license requirements" >&2
      exit 1
    fi
    cat "$FAKE_NOTICES" > "$out" ;;
  *) echo "stand-in cargo: no subcommand $1" >&2; exit 101 ;;
esac
EOF
chmod 0755 "$T/bin/cargo"
# The stand-in's notices, laid out as the archive's template lays them out. What a crate's own
# licence text holds in brackets (an email address, Rust code from a source file's header, the
# Apache License's appendix) isn't a placeholder, so every archive below packs with it.
cat > "$T/notices" <<'EOF'
Third-party notices, from a stand-in for cargo about

==============================================================================
MIT License (MIT), for:
  rcgen 0.14.10

Copyright (c) 2019-2026 Someone <someone@example.com> and contributors
Copyright (c) 2020 Someone Else <someone at example dot com>

==============================================================================
Apache License 2.0 (Apache-2.0), for:
  ring 0.17.14

   Copyright [yyyy] [name of copyright owner]
   impl<T: ?Sized> Owner<T> for Year<T> {}
EOF
PATH="$T/bin:$PATH"
export PATH FAKE_CARGO_ARGS="$T/cargo-args" FAKE_NOTICES="$T/notices"
unset FAKE_OODLE FAKE_ABOUT

# The fake binary, in a folder with a space in its name, and 0644, as a new file is: the archive's
# copy must be 0755.
mkdir -p "$T/in/with space"
printf '#!/bin/sh\n' > "$T/in/with space/fake"
chmod 0644 "$T/in/with space/fake"

# pack VERSION TARGET: package-server.sh, run from $T with relative paths as the release workflow
# runs it, into $T/out. Sets rc, out (its standard output) and err (its standard error).
pack() {
  out=$(cd "$T" && "$PACK" "$1" "$2" "in/with space/fake" out 2> "$T/err")
  rc=$?
  err=$(cat "$T/err")
}

# entries FOLDER NAME...: the folder, then each NAME in it, one a line, as tar -t and unzip -Z1
# list them.
entries() {
  local folder=$1 n
  shift
  printf '%s/\n' "$folder"
  for n in "$@"; do printf '%s/%s\n' "$folder" "$n"; done
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

# lacks FILE PATTERN: FILE is there, and has no line with PATTERN.
lacks() { [ -f "$1" ] && ! grep -q -- "$2" "$1"; }

# packs WHAT: the last pack succeeded.
packs() {
  if [ "$rc" = 0 ]; then ok "$1"; else bad "$1: exit $rc: $err"; fi
}

# modes TARBALL: each entry's mode and name, as tar -tv shows them (GNU tar and bsdtar alike).
modes() { tar -tvzf "$1" | awk '{ print $1, $NF }'; }

# crlf FILE: every line of FILE ends in CR LF, with no CR doubled.
crlf() {
  [ -s "$1" ] && ! grep -q "$(printf '\r\r')" "$1" &&
    awk '!/\r$/ { bare = 1 } END { exit bare }' "$1" &&
    [ "$(tail -c 2 "$1" | od -An -tx1 | tr -d ' \n')" = 0d0a ]
}

v=1.2.3
src=https://github.com/theintonerzero/fenji/tree/v$v
guide=https://github.com/theintonerzero/fenji/blob/v$v/docs/hosting.md

# Linux: a tarball with start-server.sh.
L=fenji-server-$v-x86_64-unknown-linux-musl
pack "$v" x86_64-unknown-linux-musl
packs "a Linux target packs"
same "it prints the archive's path" "$out" "$T/out/$L.tar.gz"
# A Linux archive also carries deploy/systemd's units and deploy/caddy's Caddyfile, so a VPS needs no
# second download (#359).
# Every file deploy/systemd holds, in byte order: the archive is compared with that listing (#367).
UNITS=()
for path in "$REPO"/deploy/systemd/*; do UNITS+=("${path##*/}"); done
same "deploy/systemd holds the service, the backup and its timer" "$(printf '%s\n' "${UNITS[@]}")" \
  "$(printf '%s\n' fenji-backup.service fenji-backup.timer fenji.service)"
DEPLOY=(systemd/ "${UNITS[@]/#/systemd/}" caddy/ caddy/Caddyfile)
UNIT_MODES=()
for u in "${UNITS[@]}"; do UNIT_MODES+=("-rw-r--r-- $L/systemd/$u"); done
same "the Linux tarball holds one folder: the server, start-server.sh, update-server.sh, the texts and the VPS's units" \
  "$(tar -tzf "$T/out/$L.tar.gz")" \
  "$(entries "$L" fenji-server start-server.sh update-server.sh fenji.example.toml README.txt LICENSE SOURCE.txt \
    THIRD-PARTY-NOTICES.txt "${DEPLOY[@]}")"
same "the folders, the server and the two scripts are 0755, the texts and units 0644" "$(modes "$T/out/$L.tar.gz")" "$(printf '%s\n' \
  "drwxr-xr-x $L/" "-rwxr-xr-x $L/fenji-server" "-rwxr-xr-x $L/start-server.sh" "-rwxr-xr-x $L/update-server.sh" \
  "-rw-r--r-- $L/fenji.example.toml" \
  "-rw-r--r-- $L/README.txt" "-rw-r--r-- $L/LICENSE" "-rw-r--r-- $L/SOURCE.txt" "-rw-r--r-- $L/THIRD-PARTY-NOTICES.txt" \
  "drwxr-xr-x $L/systemd/" "${UNIT_MODES[@]}" "drwxr-xr-x $L/caddy/" "-rw-r--r-- $L/caddy/Caddyfile")"
case "$(tar --version 2>/dev/null)" in
  *"GNU tar"*)
    same "every entry is root's, by number" "$(tar --numeric-owner -tvzf "$T/out/$L.tar.gz" | awk '{ print $2 }' | sort -u)" 0/0 ;;
esac
same "cargo tree is asked, in the repository, for the target's linked crates, locked" "$(cat "$T/cargo-args.tree")" "$(printf '%s\n' \
  "$REPO" tree --locked -p fenji-server --target x86_64-unknown-linux-musl -e normal --prefix none)"
same "cargo about is asked, in the repository, for the notices of the target's linked crates, locked, failing closed" \
  "$(cat "$T/cargo-args.about")" "$(printf '%s\n' "$REPO" about generate --locked --fail -m crates/fenji-server/Cargo.toml \
  --target x86_64-unknown-linux-musl -c deploy/archive/about.toml -o OUT deploy/archive/THIRD-PARTY-NOTICES.hbs)"
mkdir "$T/linux"
tar -xzf "$T/out/$L.tar.gz" -C "$T/linux"
X=$T/linux/$L
holds "the server is the binary given" cmp -s "$X/fenji-server" "$T/in/with space/fake"
holds "start-server.sh is deploy/start's" cmp -s "$X/start-server.sh" "$REPO/deploy/start/start-server.sh"
# The update script (the hosting spec's Amendment B, HB1), in the Linux and macOS archives.
holds "update-server.sh is deploy/start's" cmp -s "$X/update-server.sh" "$REPO/deploy/start/update-server.sh"
holds "fenji.example.toml is the server's" cmp -s "$X/fenji.example.toml" "$REPO/crates/fenji-server/fenji.example.toml"
holds "LICENSE is the repository's" cmp -s "$X/LICENSE" "$REPO/LICENSE"
for unit in "${UNITS[@]}"; do
  holds "systemd/$unit is deploy/systemd's" cmp -s "$X/systemd/$unit" "$REPO/deploy/systemd/$unit"
done
holds "caddy/Caddyfile is deploy/caddy's" cmp -s "$X/caddy/Caddyfile" "$REPO/deploy/caddy/Caddyfile"
holds "SOURCE.txt names the version's source code" grep -qxF "$src" "$X/SOURCE.txt"
holds "README.txt links the version's hosting guide" grep -qF "$guide" "$X/README.txt"
holds "README.txt keeps no placeholder" lacks "$X/README.txt" @VERSION@
holds "without fenji-oodle, README.txt names no other licence" lacks "$X/README.txt" fenji-oodle
holds "README.txt names the third-party notices" grep -qF THIRD-PARTY-NOTICES.txt "$X/README.txt"
# A musl build's notices: cargo about's, then musl's COPYRIGHT, whole, at the end.
musl=$REPO/deploy/archive/musl-COPYRIGHT
holds "the notices start with cargo about's" \
  cmp -s "$T/notices" <(head -n "$(wc -l < "$T/notices" | tr -d ' ')" "$X/THIRD-PARTY-NOTICES.txt")
holds "a musl build's notices end with musl's COPYRIGHT" \
  cmp -s "$musl" <(tail -c "$(wc -c < "$musl" | tr -d ' ')" "$X/THIRD-PARTY-NOTICES.txt")
holds "musl's notice says which musl" grep -qxF "musl 1.2.5, the C library the static Linux builds link in:" \
  "$X/THIRD-PARTY-NOTICES.txt"


# Windows: a zip with start-server.bat, whose lines end in CR LF.
W=fenji-server-$v-x86_64-pc-windows-msvc
pack "$v" x86_64-pc-windows-msvc
packs "a Windows target packs"
same "it prints the zip's path" "$out" "$T/out/$W.zip"
same "the Windows zip holds one folder: the server, start-server.bat and the texts" \
  "$(unzip -Z1 "$T/out/$W.zip")" \
  "$(entries "$W" fenji-server.exe start-server.bat fenji.example.toml README.txt LICENSE SOURCE.txt THIRD-PARTY-NOTICES.txt)"
same "the Windows zip has no update-server.sh: Windows hosts update by hand (Amendment B, HB1)" \
  "$(unzip -Z1 "$T/out/$W.zip" | grep -c update-server)" 0
unzip -p "$T/out/$W.zip" "$W/fenji-server.exe" > "$T/exe"
holds "fenji-server.exe is the binary given" cmp -s "$T/exe" "$T/in/with space/fake"
unzip -p "$T/out/$W.zip" "$W/start-server.bat" > "$T/bat"
holds "start-server.bat's lines end in CR LF" crlf "$T/bat"
tr -d '\r' < "$T/bat" > "$T/bat-lf"
tr -d '\r' < "$REPO/deploy/start/start-server.bat" > "$T/bat-repo"
holds "start-server.bat is deploy/start's" cmp -s "$T/bat-lf" "$T/bat-repo"
# A crash ends with a negative NTSTATUS code (0xC0000005, 0xC00000FD), which `if errorlevel 1`
# misses, so the window would close on the error.
same "start-server.bat's commands: its folder, a network share's too; the server; a pause on any failure" \
  "$(grep -v -i -E '^(@echo off|rem( |$))' "$T/bat-lf")" "$(printf '%s\n' 'pushd "%~dp0"' 'fenji-server.exe %*' \
  'if %errorlevel% neq 0 pause')"
unzip -p "$T/out/$W.zip" "$W/SOURCE.txt" > "$T/source"
holds "the zip's SOURCE.txt names the version's source code" grep -qxF "$src" "$T/source"
unzip -p "$T/out/$W.zip" "$W/THIRD-PARTY-NOTICES.txt" > "$T/win-notices"
holds "a Windows build's notices are cargo about's, without musl's" cmp -s "$T/win-notices" "$T/notices"

# A zip already there is replaced, not added to (zip adds to an archive that's there).
rm -f "$T/out/$W.zip"
printf 'stray\n' > "$T/stray.txt"
(cd "$T" && zip -q "$T/out/$W.zip" stray.txt)
pack "$v" x86_64-pc-windows-msvc
packs "a Windows target packs over an old zip"
same "the old zip is replaced" "$(unzip -Z1 "$T/out/$W.zip")" \
  "$(entries "$W" fenji-server.exe start-server.bat fenji.example.toml README.txt LICENSE SOURCE.txt THIRD-PARTY-NOTICES.txt)"

# macOS: a tarball with start-server.command, which Finder runs only if it's executable.
M=fenji-server-$v-aarch64-apple-darwin
pack "$v" aarch64-apple-darwin
packs "a macOS target packs"
same "the macOS tarball holds one folder: the server, start-server.command, update-server.sh and the texts" \
  "$(tar -tzf "$T/out/$M.tar.gz")" \
  "$(entries "$M" fenji-server start-server.command update-server.sh fenji.example.toml README.txt LICENSE SOURCE.txt \
    THIRD-PARTY-NOTICES.txt)"
same "start-server.command and update-server.sh are 0755" "$(modes "$T/out/$M.tar.gz" | grep -e start-server -e update-server)" \
  "$(printf '%s\n' "-rwxr-xr-x $M/start-server.command" "-rwxr-xr-x $M/update-server.sh")"
mkdir "$T/mac"
tar -xzf "$T/out/$M.tar.gz" -C "$T/mac"
holds "start-server.command is deploy/start's" cmp -s "$T/mac/$M/start-server.command" "$REPO/deploy/start/start-server.command"
holds "the macOS update-server.sh is deploy/start's" cmp -s "$T/mac/$M/update-server.sh" "$REPO/deploy/start/update-server.sh"
holds "a macOS build's notices are cargo about's, without musl's" cmp -s "$T/mac/$M/THIRD-PARTY-NOTICES.txt" "$T/notices"
same "cargo about is asked for the macOS target's crates" "$(sed -n '/^--target$/{n;p;}' "$T/cargo-args.about")" aarch64-apple-darwin

# The start scripts, with a stand-in server that reports its open-file limits, its folder and its
# arguments. They raise the soft limit to 4,096, or as far as the hard limit allows, and never
# lower either (Ruling HP11).
# shellcheck disable=SC2016 # the stand-in's $(...) and $* are for it, not for this script
reporter='#!/bin/sh
echo "soft $(ulimit -Sn) hard $(ulimit -Hn) in $(pwd -P) with $# args: $*"'
# started SCRIPT SOFT HARD ARGS...: SCRIPT's report, run from / in a shell whose soft open-file
# limit is SOFT and whose hard limit is HARD (or as it is, when HARD is empty).
started() {
  local script=$1 soft=$2 hard=$3
  shift 3
  (ulimit -Sn "$soft" && { [ -z "$hard" ] || ulimit -Hn "$hard"; } && cd / && "$script" "$@") 2>&1
}
hard=$(ulimit -Hn)
printf '%s\n' "$reporter" > "$X/fenji-server"
for s in "$X/start-server.sh" "$T/mac/$M/start-server.command"; do
  [ -e "$s" ] || continue
  d=${s%/*}
  [ "$d" = "$X" ] || printf '%s\n' "$reporter" > "$d/fenji-server"
  n=${s##*/}
  same "$n raises a soft limit of 256 to 4096, and leaves the hard limit" \
    "$(started "$s" 256 "")" "soft 4096 hard $hard in $d with 0 args: "
  same "$n raises the soft limit only as far as a hard limit of 1024" \
    "$(started "$s" 256 1024)" "soft 1024 hard 1024 in $d with 0 args: "
  if [ "$hard" = unlimited ] || [ "$hard" -ge 8192 ]; then
    same "$n leaves a soft limit above 4096 as it is" \
      "$(started "$s" 8192 "")" "soft 8192 hard $hard in $d with 0 args: "
  else
    echo "skip $n leaves a soft limit above 4096 as it is: the hard limit is $hard"
  fi
  same "$n starts the server in its own folder, with the arguments given" \
    "$(started "$s" 4096 "" a "b c")" "soft 4096 hard $hard in $d with 2 args: a b c"
  # A decoy folder of the same name, where an exported CDPATH points, doesn't catch its cd.
  mkdir -p "$T/decoy-start/${d##*/}"
  printf '#!/bin/sh\necho decoy\n' > "$T/decoy-start/${d##*/}/fenji-server"
  chmod 0755 "$T/decoy-start/${d##*/}/fenji-server"
  same "$n, run by a relative path with CDPATH set, starts its own server" \
    "$(ulimit -Sn 4096 && cd "${d%/*}" && CDPATH=$T/decoy-start "${d##*/}/$n" 2>&1)" \
    "soft 4096 hard $hard in $d with 0 args: "
done

# A release candidate's version.
R=fenji-server-1.2.3-rc.1-x86_64-unknown-linux-musl
pack 1.2.3-rc.1 x86_64-unknown-linux-musl
packs "a release candidate packs"
holds "its tarball is named for it" test -f "$T/out/$R.tar.gz"
mkdir "$T/rc"
tar -xzf "$T/out/$R.tar.gz" -C "$T/rc"
holds "its SOURCE.txt names its tag" grep -qxF https://github.com/theintonerzero/fenji/tree/v1.2.3-rc.1 "$T/rc/$R/SOURCE.txt"

# A build that links fenji-oodle (GPL-3.0-or-later, Oodle Ruling O16) carries its COPYING and NOTICE.
export FAKE_OODLE=yes
rm -rf "$T/out"
pack "$v" x86_64-unknown-linux-musl
packs "a Linux target that links fenji-oodle packs"
same "its tarball adds fenji-oodle's COPYING and NOTICE" "$(tar -tzf "$T/out/$L.tar.gz")" \
  "$(entries "$L" fenji-server start-server.sh update-server.sh fenji.example.toml README.txt LICENSE SOURCE.txt \
    THIRD-PARTY-NOTICES.txt "${DEPLOY[@]}" fenji-oodle/ fenji-oodle/COPYING fenji-oodle/NOTICE)"
same "fenji-oodle's folder is 0755 and its texts 0644" "$(modes "$T/out/$L.tar.gz" | grep fenji-oodle)" "$(printf '%s\n' \
  "drwxr-xr-x $L/fenji-oodle/" "-rw-r--r-- $L/fenji-oodle/COPYING" "-rw-r--r-- $L/fenji-oodle/NOTICE")"
mkdir "$T/gpl"
tar -xzf "$T/out/$L.tar.gz" -C "$T/gpl"
holds "COPYING is fenji-oodle's" cmp -s "$T/gpl/$L/fenji-oodle/COPYING" "$REPO/crates/fenji-oodle/COPYING"
holds "NOTICE is fenji-oodle's" cmp -s "$T/gpl/$L/fenji-oodle/NOTICE" "$REPO/crates/fenji-oodle/NOTICE"
holds "README.txt names fenji-oodle's licence" grep -qF fenji-oodle/COPYING "$T/gpl/$L/README.txt"
pack "$v" x86_64-pc-windows-msvc
packs "a Windows target that links fenji-oodle packs"
same "its zip adds fenji-oodle's COPYING and NOTICE" "$(unzip -Z1 "$T/out/$W.zip")" \
  "$(entries "$W" fenji-server.exe start-server.bat fenji.example.toml README.txt LICENSE SOURCE.txt THIRD-PARTY-NOTICES.txt \
    fenji-oodle/ fenji-oodle/COPYING fenji-oodle/NOTICE)"
export FAKE_OODLE=prefix
pack "$v" x86_64-unknown-linux-musl
packs "a build with a crate named like fenji-oodle packs"
same "a crate whose name only starts with fenji-oodle brings no GPL texts" "$(tar -tzf "$T/out/$L.tar.gz")" \
  "$(entries "$L" fenji-server start-server.sh update-server.sh fenji.example.toml README.txt LICENSE SOURCE.txt \
    THIRD-PARTY-NOTICES.txt "${DEPLOY[@]}")"
unset FAKE_OODLE

# The archive is staged in a folder beside it, so that its last move is a rename on one file
# system: a pack needs no temporary directory elsewhere, and leaves no staging folder behind.
rm -rf "$T/out"
got=$(cd "$T" && TMPDIR=$T/nowhere "$PACK" "$v" x86_64-unknown-linux-musl "in/with space/fake" out 2>&1)
rc=$?
same "with no usable TMPDIR, it packs into the out-dir and prints the path" "$rc $got" "0 $T/out/$L.tar.gz"
same "it leaves no staging folder behind" "$(find "$T/out" -mindepth 1 -name '.stage.*' 2>&1)" ""

# An exported CDPATH doesn't steer its cd: decoy scripts/ and out/ folders there don't catch it.
mkdir -p "$T/decoy/scripts" "$T/decoy/out"
rm -rf "$T/out"
got=$(cd "$T" && CDPATH=$T/decoy "$PACK" "$v" x86_64-unknown-linux-musl "in/with space/fake" out 2> "$T/err")
rc=$?
same "with CDPATH set, a relative out-dir is the working directory's" "$rc $got" "0 $T/out/$L.tar.gz"
same "nothing goes to CDPATH's out/" "$(find "$T/decoy/out" -mindepth 1)" ""
got=$(cd "$REPO" && CDPATH=$T/decoy scripts/package-server.sh "$v" x86_64-unknown-linux-musl \
  "$T/in/with space/fake" "$T/out" 2> "$T/err")
rc=$?
same "run by a relative path with CDPATH set, it finds its own repository" "$rc $got" "0 $T/out/$L.tar.gz"

# When the notices can't be made, nothing is packed: no archive, and no staging folder left.
export FAKE_ABOUT=fail
rm -rf "$T/out"
got=$(cd "$T" && "$PACK" "$v" x86_64-unknown-linux-musl "in/with space/fake" out 2>&1 > /dev/null)
rc=$?
if [ "$rc" = 1 ] && grep -q "cargo about failed" <<< "$got" && [ -z "$(find "$T/out" -mindepth 1 2>/dev/null)" ]; then
  ok "a failing cargo about stops it: no archive goes out without its notices"
else
  bad "a failing cargo about stops it: exit $rc: $got: $(find "$T/out" -mindepth 1 2>&1)"
fi
unset FAKE_ABOUT

# For a crate whose own licence text it can't find or read, cargo about puts in the licence's
# standard text, and says so only in its debug log (#328). MIT's and BSD's leave the copyright
# line a template ("Copyright (c) <year> <copyright holders>"), which must never ship.
# placeheld WHAT TARGET HEADING LINE CRATE...: package-server.sh, when cargo about's notices end
# with a section for the CRATEs that has HEADING and holds LINE, exits 1, shows LINE, names those
# crates and none of the other sections', and leaves no archive and no staging folder.
placeheld() {
  local what=$1 target=$2 heading=$3 line=$4 got rc c wrong=
  shift 4
  {
    cat "$T/notices"
    printf '\n%s\n%s\n' ============================================================================== "$heading"
    for c in "$@"; do printf '  %s\n' "$c"; done
    printf '\n%s\n' "$line" "Permission is hereby granted, free of charge, to any person obtaining a copy"
  } > "$T/placeheld"
  rm -rf "$T/out"
  got=$(cd "$T" && FAKE_NOTICES=$T/placeheld "$PACK" "$v" "$target" "in/with space/fake" out 2>&1 > /dev/null)
  rc=$?
  for c in "$@"; do grep -qxF -- "  $c" <<< "$got" || wrong="$wrong, $c not named"; done
  for c in "rcgen 0.14.10" "ring 0.17.14"; do grep -qF -- "$c" <<< "$got" && wrong="$wrong, $c named"; done
  grep -qF -- "$line" <<< "$got" || wrong="$wrong, the line not shown"
  grep -q placeholder <<< "$got" || wrong="$wrong, no word of a placeholder"
  if [ "$rc" = 1 ] && [ -z "$wrong" ] && [ -z "$(find "$T/out" -mindepth 1 2>/dev/null)" ]; then
    ok "$what"
  else
    bad "$what: exit $rc$wrong: $got: $(find "$T/out" -mindepth 1 2>&1)"
  fi
}
mit="MIT License (MIT), for:"
placeheld "MIT's standard text, with <year> and <copyright holders>, stops a Linux pack, naming its crates" \
  x86_64-unknown-linux-musl "$mit" "Copyright (c) <year> <copyright holders>" "mutants 0.0.3" "sqlx-core 0.9.0"
placeheld "a <year> on its own stops a Windows pack" \
  x86_64-pc-windows-msvc "$mit" "Copyright (c) <year> Someone" "windows-sys 0.61.2"
placeheld "a <copyright holders> on its own stops it" \
  x86_64-unknown-linux-musl "$mit" "Copyright (c) 2020 <copyright holders>" "some-crate 1.0.0"
placeheld "BSD's <owner> stops a macOS pack" \
  aarch64-apple-darwin 'BSD 3-Clause "New" or "Revised" License (BSD-3-Clause), for:' \
  "Copyright (c) 2016 <owner>. All rights reserved." "some-crate 1.0.0"
placeheld "a field in capitals and in the singular, as MIT-0 writes it, stops it" \
  x86_64-unknown-linux-musl "MIT No Attribution (MIT-0), for:" "Copyright 2020 <COPYRIGHT HOLDER>" "some-crate 1.0.0"

# toml_list FILE TABLE KEY: the strings in FILE's KEY = [ ... ] array, under [TABLE] (or before any
# table when TABLE is empty), one a line, in order.
toml_list() {
  awk -v table="$2" -v key="$3" '
    /^\[/ { t = $0; gsub(/^\[|\]$/, "", t); next }
    !on && t == table && $0 ~ "^" key " *= *\\[" { on = 1 }
    on {
      s = $0
      sub(/#.*/, "", s)
      while (match(s, /"[^"]*"/)) { print substr(s, RSTART + 1, RLENGTH - 2); s = substr(s, RSTART + RLENGTH) }
      if (s ~ /\]/) on = 0
    }
  ' "$1"
}

# cargo-about accepts exactly the licences cargo-deny allows, in the same order, so the notices and
# the dependency check never disagree about what may go into a build.
allowed=$(toml_list "$REPO/deny.toml" licenses allow)
holds "deny.toml's [licenses] allow list is read" test -n "$allowed"
same "deploy/archive/about.toml accepts exactly deny.toml's licences, in its order" \
  "$(toml_list "$REPO/deploy/archive/about.toml" "" accepted)" "$allowed"

# refused WANT_RC PATTERN WHAT -- ARGS...: package-server.sh ARGS, run from $T, exits WANT_RC with
# PATTERN in its message, and makes no out-dir.
refused() {
  local want=$1 pattern=$2 what=$3 got rc
  shift 4
  rm -rf "$T/out"
  got=$(cd "$T" && "$PACK" "$@" 2>&1 > /dev/null)
  rc=$?
  if [ "$rc" = "$want" ] && grep -q -- "$pattern" <<< "$got" && [ ! -e "$T/out" ]; then
    ok "$what"
  else
    bad "$what: exit $rc: $got"
  fi
}

refused 2 usage "no arguments are a usage error" --
refused 2 usage "three arguments are a usage error" -- "$v" x86_64-unknown-linux-musl "in/with space/fake"
refused 2 usage "five arguments are a usage error" -- "$v" x86_64-unknown-linux-musl "in/with space/fake" out more
refused 2 usage "an empty out-dir is a usage error" -- "$v" x86_64-unknown-linux-musl "in/with space/fake" ""
refused 2 "without the tag's v" "a version with the tag's v is refused" -- "v$v" x86_64-unknown-linux-musl "in/with space/fake" out
refused 2 "without the tag's v" "a version of two numbers is refused" -- 1.2 x86_64-unknown-linux-musl "in/with space/fake" out
refused 2 "without the tag's v" "a version with a slash is refused" -- 1.2.3-a/b x86_64-unknown-linux-musl "in/with space/fake" out
refused 2 "without the tag's v" "an empty version is refused" -- "" x86_64-unknown-linux-musl "in/with space/fake" out
refused 2 "not a target" "a target with a slash is refused" -- "$v" ../x86_64-unknown-linux-musl "in/with space/fake" out
refused 2 "not a target" "an empty target is refused" -- "$v" "" "in/with space/fake" out
refused 2 "no start script" "a system without a start script is refused" -- "$v" x86_64-unknown-freebsd "in/with space/fake" out
refused 2 "no binary" "a missing binary is refused" -- "$v" x86_64-unknown-linux-musl in/nothing out
refused 2 "no binary" "a folder as the binary is refused" -- "$v" x86_64-unknown-linux-musl in out
export FAKE_OODLE=fail
refused 1 "cargo tree failed" "a failing cargo tree stops it: GPL code must not go out without its licence" -- \
  "$v" x86_64-unknown-linux-musl "in/with space/fake" out
unset FAKE_OODLE

[ "$fails" = 0 ] || { echo "$fails failed"; exit 1; }
echo "all passed"
