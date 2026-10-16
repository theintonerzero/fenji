#!/usr/bin/env bash
# Tests for scripts/launcher-latest-json.sh (launcher spec §3.8; plan Task 15): the updater's
# latest.json from three fake bundles and their signatures, and the refusals. Needs jq. Runs on
# Linux and macOS (bash 3.2).
#   scripts/test-launcher-latest-json.sh
set -uo pipefail
export LC_ALL=C
unset CDPATH
HERE=$(cd -- "$(dirname -- "$0")" && pwd -P)
LATEST=$HERE/launcher-latest-json.sh
fails=0
ok() { echo "ok   $1"; }
bad() { echo "FAIL $1"; fails=$((fails + 1)); }

command -v jq > /dev/null || { echo "FATAL: the test needs jq" >&2; exit 2; }

T=$(mktemp -d) || { echo "FATAL: mktemp failed" >&2; exit 2; }
T=$(cd -- "$T" && pwd -P) || exit 2
trap 'rm -rf -- "$T"' EXIT
case "$T" in /?*) ;; *) echo "FATAL: no temporary directory" >&2; exit 2 ;; esac

# bundles DIR: the three bundles a release builds, each with its signature, a line of text.
bundles() {
  mkdir -p "$1"
  for f in Fenji_0.5.1_amd64.AppImage Fenji_0.5.1_x64-setup.exe Fenji.app.tar.gz; do
    printf 'bundle\n' > "$1/$f"
    printf 'sig of %s\n' "$f" > "$1/$f.sig"
  done
}
printf 'Signed self-updates.\n\nAnd more.\n' > "$T/notes.md"

# --- a release's three bundles
bundles "$T/b"
out=$("$LATEST" 0.5.1 "$T/notes.md" "$T/b" 2> "$T/err")
rc=$?
J=$T/b/latest.json
url=https://github.com/theintonerzero/fenji/releases/download/v0.5.1
if [ "$rc" = 0 ] && [ "$out" = "$J" ] && [ -f "$J" ]; then
  ok "it writes latest.json beside the bundles and prints its path"
else
  bad "exit $rc, printed '$out': $(cat "$T/err")"
fi
check() { # check NAME JQ-FILTER: the filter holds for latest.json
  if jq -e "$2" "$J" > /dev/null 2>&1; then ok "$1"; else bad "$1: $(jq -c . "$J" 2> /dev/null)"; fi
}
check "the version, without its v" '.version == "0.5.1"'
check "the notes are the file's text" '.notes == "Signed self-updates.\n\nAnd more."'
check "pub_date is RFC 3339, in UTC" '.pub_date | test("^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$")'
check "Linux gets the AppImage and its signature" \
  ".platforms[\"linux-x86_64\"] == {\"url\": \"$url/Fenji_0.5.1_amd64.AppImage\", \"signature\": \"sig of Fenji_0.5.1_amd64.AppImage\"}"
check "Windows gets the NSIS installer and its signature" \
  ".platforms[\"windows-x86_64\"] == {\"url\": \"$url/Fenji_0.5.1_x64-setup.exe\", \"signature\": \"sig of Fenji_0.5.1_x64-setup.exe\"}"
check "macOS gets the app's tarball and its signature" \
  ".platforms[\"darwin-aarch64\"] == {\"url\": \"$url/Fenji.app.tar.gz\", \"signature\": \"sig of Fenji.app.tar.gz\"}"
check "only those three platforms" '.platforms | keys == ["darwin-aarch64", "linux-x86_64", "windows-x86_64"]'

# --- refusals: nothing is written
refused() { # refused NAME DIR [VERSION]: exit 1 or 2 and no latest.json
  local out rc
  out=$("$LATEST" "${3:-0.5.1}" "$T/notes.md" "$2" 2>&1)
  rc=$?
  if [ "$rc" != 0 ] && [ ! -e "$2/latest.json" ]; then ok "$1"; else bad "$1 (exit $rc): $out"; fi
}
bundles "$T/nosig"
rm "$T/nosig/Fenji_0.5.1_x64-setup.exe.sig"
refused "a bundle without its signature stops it" "$T/nosig"
bundles "$T/nomac"
rm "$T/nomac/Fenji.app.tar.gz" "$T/nomac/Fenji.app.tar.gz.sig"
refused "a missing platform stops it" "$T/nomac"
bundles "$T/two"
printf 'bundle\n' > "$T/two/Fenji_0.5.0_amd64.AppImage"
printf 'sig\n' > "$T/two/Fenji_0.5.0_amd64.AppImage.sig"
refused "two bundles for one platform stop it" "$T/two"
bundles "$T/space"
mv "$T/space/Fenji.app.tar.gz" "$T/space/Fenji app.tar.gz"
mv "$T/space/Fenji.app.tar.gz.sig" "$T/space/Fenji app.tar.gz.sig"
refused "a file name a URL can't carry as it is stops it" "$T/space"
bundles "$T/v"
refused "a version that isn't one stops it" "$T/v" v0.5.1
"$LATEST" 0.5.1 "$T/notes.md" > /dev/null 2>&1
rc=$?
if [ "$rc" = 2 ]; then ok "too few arguments is a usage error"; else bad "too few arguments (exit $rc)"; fi

if [ "$fails" = 0 ]; then echo "all passed"; else echo "$fails failed"; exit 1; fi
