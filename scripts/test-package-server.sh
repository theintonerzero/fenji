#!/usr/bin/env bash
# Tests for scripts/package-server.sh (M4 hosting spec §3.7): each archive's listing, its modes and
# its texts, from a fake binary and with a stand-in for cargo, so there's no build and no network.
# Needs tar, gzip, zip and unzip. Runs on Linux and macOS (bash 3.2).
#   scripts/test-package-server.sh
set -uo pipefail
export LC_ALL=C
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

# A stand-in for cargo, first on PATH. It writes the directory it ran in and its arguments, one a
# line, to $T/cargo-args, and prints fenji-server's dependency tree: with fenji-oodle in it when
# FAKE_OODLE=yes, with only a crate whose name starts the same when FAKE_OODLE=prefix, and failing
# as cargo does when FAKE_OODLE=fail.
mkdir "$T/bin"
cat > "$T/bin/cargo" <<'EOF'
#!/bin/sh
{ pwd -P; printf '%s\n' "$@"; } > "$FAKE_CARGO_ARGS"
case "${FAKE_OODLE:-no}" in
  fail) echo "error: failed to download from \`https://index.crates.io/\`" >&2; exit 101 ;;
esac
echo "fenji-server v0.0.0 (/fenji/crates/fenji-server)"
echo "tokio v1.0.0"
case "${FAKE_OODLE:-no}" in
  yes) echo "fenji-oodle v0.0.0 (/fenji/crates/fenji-oodle)" ;;
  prefix) echo "fenji-oodle-helper v0.0.0 (/fenji/tools/oodle-helper)" ;;
esac
EOF
chmod 0755 "$T/bin/cargo"
PATH="$T/bin:$PATH"
export PATH FAKE_CARGO_ARGS="$T/cargo-args"
unset FAKE_OODLE

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
same "the Linux tarball holds one folder: the server, start-server.sh and the texts" \
  "$(tar -tzf "$T/out/$L.tar.gz")" \
  "$(entries "$L" fenji-server start-server.sh fenji.example.toml README.txt LICENSE SOURCE.txt)"
same "the folder, the server and start-server.sh are 0755, the texts 0644" "$(modes "$T/out/$L.tar.gz")" "$(printf '%s\n' \
  "drwxr-xr-x $L/" "-rwxr-xr-x $L/fenji-server" "-rwxr-xr-x $L/start-server.sh" "-rw-r--r-- $L/fenji.example.toml" \
  "-rw-r--r-- $L/README.txt" "-rw-r--r-- $L/LICENSE" "-rw-r--r-- $L/SOURCE.txt")"
case "$(tar --version 2>/dev/null)" in
  *"GNU tar"*)
    same "every entry is root's, by number" "$(tar --numeric-owner -tvzf "$T/out/$L.tar.gz" | awk '{ print $2 }' | sort -u)" 0/0 ;;
esac
same "cargo tree is asked, in the repository, for the target's linked crates, locked" "$(cat "$T/cargo-args")" "$(printf '%s\n' \
  "$REPO" tree --locked -p fenji-server --target x86_64-unknown-linux-musl -e normal --prefix none)"
mkdir "$T/linux"
tar -xzf "$T/out/$L.tar.gz" -C "$T/linux"
X=$T/linux/$L
holds "the server is the binary given" cmp -s "$X/fenji-server" "$T/in/with space/fake"
holds "start-server.sh is deploy/start's" cmp -s "$X/start-server.sh" "$REPO/deploy/start/start-server.sh"
holds "fenji.example.toml is the server's" cmp -s "$X/fenji.example.toml" "$REPO/crates/fenji-server/fenji.example.toml"
holds "LICENSE is the repository's" cmp -s "$X/LICENSE" "$REPO/LICENSE"
holds "SOURCE.txt names the version's source code" grep -qxF "$src" "$X/SOURCE.txt"
holds "README.txt links the version's hosting guide" grep -qF "$guide" "$X/README.txt"
holds "README.txt keeps no placeholder" lacks "$X/README.txt" @VERSION@
holds "without fenji-oodle, README.txt names no other licence" lacks "$X/README.txt" fenji-oodle

# Windows: a zip with start-server.bat, whose lines end in CR LF.
W=fenji-server-$v-x86_64-pc-windows-msvc
pack "$v" x86_64-pc-windows-msvc
packs "a Windows target packs"
same "it prints the zip's path" "$out" "$T/out/$W.zip"
same "the Windows zip holds one folder: the server, start-server.bat and the texts" \
  "$(unzip -Z1 "$T/out/$W.zip")" \
  "$(entries "$W" fenji-server.exe start-server.bat fenji.example.toml README.txt LICENSE SOURCE.txt)"
unzip -p "$T/out/$W.zip" "$W/fenji-server.exe" > "$T/exe"
holds "fenji-server.exe is the binary given" cmp -s "$T/exe" "$T/in/with space/fake"
unzip -p "$T/out/$W.zip" "$W/start-server.bat" > "$T/bat"
holds "start-server.bat's lines end in CR LF" crlf "$T/bat"
tr -d '\r' < "$T/bat" > "$T/bat-lf"
tr -d '\r' < "$REPO/deploy/start/start-server.bat" > "$T/bat-repo"
holds "start-server.bat is deploy/start's" cmp -s "$T/bat-lf" "$T/bat-repo"
unzip -p "$T/out/$W.zip" "$W/SOURCE.txt" > "$T/source"
holds "the zip's SOURCE.txt names the version's source code" grep -qxF "$src" "$T/source"

# A zip already there is replaced, not added to (zip adds to an archive that's there).
rm -f "$T/out/$W.zip"
printf 'stray\n' > "$T/stray.txt"
(cd "$T" && zip -q "$T/out/$W.zip" stray.txt)
pack "$v" x86_64-pc-windows-msvc
packs "a Windows target packs over an old zip"
same "the old zip is replaced" "$(unzip -Z1 "$T/out/$W.zip")" \
  "$(entries "$W" fenji-server.exe start-server.bat fenji.example.toml README.txt LICENSE SOURCE.txt)"

# macOS: a tarball with start-server.command, which Finder runs only if it's executable.
M=fenji-server-$v-aarch64-apple-darwin
pack "$v" aarch64-apple-darwin
packs "a macOS target packs"
same "the macOS tarball holds one folder: the server, start-server.command and the texts" \
  "$(tar -tzf "$T/out/$M.tar.gz")" \
  "$(entries "$M" fenji-server start-server.command fenji.example.toml README.txt LICENSE SOURCE.txt)"
same "start-server.command is 0755" "$(modes "$T/out/$M.tar.gz" | grep start-server)" "-rwxr-xr-x $M/start-server.command"
mkdir "$T/mac"
tar -xzf "$T/out/$M.tar.gz" -C "$T/mac"
holds "start-server.command is deploy/start's" cmp -s "$T/mac/$M/start-server.command" "$REPO/deploy/start/start-server.command"

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
  "$(entries "$L" fenji-server start-server.sh fenji.example.toml README.txt LICENSE SOURCE.txt \
    fenji-oodle/ fenji-oodle/COPYING fenji-oodle/NOTICE)"
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
  "$(entries "$W" fenji-server.exe start-server.bat fenji.example.toml README.txt LICENSE SOURCE.txt \
    fenji-oodle/ fenji-oodle/COPYING fenji-oodle/NOTICE)"
export FAKE_OODLE=prefix
pack "$v" x86_64-unknown-linux-musl
packs "a build with a crate named like fenji-oodle packs"
same "a crate whose name only starts with fenji-oodle brings no GPL texts" "$(tar -tzf "$T/out/$L.tar.gz")" \
  "$(entries "$L" fenji-server start-server.sh fenji.example.toml README.txt LICENSE SOURCE.txt)"
unset FAKE_OODLE

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
