#!/usr/bin/env bash
# Takes a release's files from the release workflow's artifacts, each only when the job that made it
# reported it, and puts them in one folder to sum and sign (the hosting spec's Amendment D, HD2).
#
#   BUILD=JSON LAUNCHER=JSON [SIGNED=JSON] scripts/check-release-files.sh VERSION IN OUT
#
# Any job of a run can replace another job's artifact, or add one of its own, but only a job sets
# its own outputs. So each job that makes a release file reports it in its own output, as
# "SHA-256 NAME" (a job for each target and bundle: a matrix's legs would share their outputs), and
# the workflow passes the reports here as JSON objects, keyed by what each job makes:
#   - BUILD: each server job's, under its target T, for the archive fenji-server-VERSION-T.tar.gz
#     (.zip on Windows), in the artifact fenji-server-T;
#   - LAUNCHER: each launcher job's, under appimage, nsis and app, for the *.AppImage, the
#     *-setup.exe and the *.app.tar.gz, each in the artifact fenji-launcher-KEY;
#   - SIGNED: the sign job's outputs, under the same keys as LAUNCHER, for each bundle's updater
#     signature, BUNDLE.sig, all in the artifact launcher-signatures.
# Those that are set say what IN must hold. IN holds each artifact in a folder of its name, as
# download-artifact leaves them without merge-multiple; it must hold exactly the folders reported,
# and each folder exactly the regular files reported in it, with those names and sums. Then every
# file goes into OUT, which must not exist yet, and no two files may share a name.
# Exits 1 when anything differs, leaving no OUT; 2 on a usage error. Needs jq.
set -euo pipefail
export LC_ALL=C
shopt -s nullglob dotglob

usage() {
  echo "usage: BUILD=JSON LAUNCHER=JSON [SIGNED=JSON] $0 VERSION IN OUT" >&2
  exit 2
}

# fail MESSAGE: says MESSAGE, removes OUT if this run made it, and exits 1.
fail() {
  echo "::error::check-release-files.sh: $1" >&2
  [ -z "${made:-}" ] || rm -rf -- "$out"
  exit 1
}

[ $# -eq 3 ] || usage
version=$1 in=$2 out=$3
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || usage
[ -n "${BUILD:-}${LAUNCHER:-}" ] || usage
[ -z "${SIGNED:-}" ] || [ -n "${LAUNCHER:-}" ] || usage
[ -d "$in" ] || { echo "$0: there's no folder $in" >&2; exit 2; }
if [ -e "$out" ] || [ -L "$out" ]; then
  echo "$0: $out is already there" >&2
  exit 2
fi
command -v jq > /dev/null || { echo "$0: jq is needed" >&2; exit 2; }

work=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/check-release-files.XXXXXX")
trap 'rm -rf -- "$work"' EXIT

# reported WHAT JSON: JSON's keys and values, as "KEY SUM NAME" lines, once each is checked: an
# object whose keys are plain words and whose values are each a SHA-256 and a plain file name.
reported() {
  printf '%s' "$2" | jq -r -s --arg what "$1" '
    if length != 1 or (.[0] | type) != "object" then error("\($what) is not one JSON object") else .[0] end
    | to_entries[]
    | if (.key | test("\\A[a-z0-9][a-z0-9_-]*\\z"))
        and (.value | type == "string")
        and (.value | test("\\A[0-9a-f]{64} [A-Za-z0-9][A-Za-z0-9._-]*\\z"))
      then "\(.key) \(.value)"
      else error("\($what) reports \(.key | tojson) as \(.value | tojson), not as a SHA-256 and a file name") end
  ' 2> "$work/jq.err" || fail "$(cat "$work/jq.err")"
}

# sum256 FILE: FILE's SHA-256.
sum256() {
  if command -v sha256sum > /dev/null; then sha256sum < "$1"; else shasum -a 256 < "$1"; fi | cut -d' ' -f1
}

# names DIR: DIR's entries, one a line, sorted; refuses one whose name isn't a plain file name.
names() {
  local entry name
  for entry in "$1"/*; do
    name=${entry##*/}
    [[ "$name" =~ ^[A-Za-z0-9._-]+$ ]] || fail "$1 holds $(printf '%q' "$name"), which isn't a plain file name"
    printf '%s\n' "$name"
  done > "$work/names"
  sort "$work/names"
}

# same WHAT WANT GOT: WANT and GOT, sorted lists, are the same; otherwise says what's missing or extra.
same() {
  local missing extra
  missing=$(comm -23 "$2" "$3" | tr '\n' ' ')
  extra=$(comm -13 "$2" "$3" | tr '\n' ' ')
  [ -z "$missing$extra" ] || fail "$1 isn't what the jobs reported:${missing:+ no ${missing% };}${extra:+ unreported ${extra% }}"
}

# The manifest: "ARTIFACT NAME SUM" for each file a job reported.
: > "$work/manifest"
if [ -n "${BUILD:-}" ]; then
  reported BUILD "$BUILD" > "$work/build"
  while read -r target sum name; do
    case $target in *-windows-*) ext=zip ;; *) ext=tar.gz ;; esac
    [ "$name" = "fenji-server-$version-$target.$ext" ] ||
      fail "the $target server job reports $name, not fenji-server-$version-$target.$ext"
    echo "fenji-server-$target $name $sum" >> "$work/manifest"
  done < "$work/build"
fi
if [ -n "${LAUNCHER:-}" ]; then
  reported LAUNCHER "$LAUNCHER" > "$work/launcher"
  while read -r bundle sum name; do
    case $bundle:$name in
      appimage:*.AppImage | nsis:*-setup.exe | app:*.app.tar.gz) ;;
      appimage:* | nsis:* | app:*) fail "the $bundle launcher job reports $name, which isn't that kind of bundle" ;;
      *) fail "LAUNCHER reports $bundle, a bundle Fenji doesn't make" ;;
    esac
    echo "fenji-launcher-$bundle $name $sum" >> "$work/manifest"
  done < "$work/launcher"
fi
if [ -n "${SIGNED:-}" ]; then
  reported SIGNED "$SIGNED" > "$work/signed"
  [ "$(cut -d' ' -f1 "$work/signed" | sort)" = "$(cut -d' ' -f1 "$work/launcher" | sort)" ] ||
    fail "the sign job reports signatures for other bundles than the launcher jobs made"
  while read -r bundle sum name; do
    bundled=$(awk -v b="$bundle" '$1 == b { print $3 }' "$work/launcher")
    [ "$name" = "$bundled.sig" ] || fail "the sign job's $bundle reports $name, not $bundled.sig"
    echo "launcher-signatures $name $sum" >> "$work/manifest"
  done < "$work/signed"
fi
[ -s "$work/manifest" ] || fail "no job reports a file"
twice=$(cut -d' ' -f2 "$work/manifest" | sort | uniq -d | tr '\n' ' ')
[ -z "$twice" ] || fail "more than one file is named ${twice% }"

# IN holds exactly the artifacts reported, and each exactly its files.
cut -d' ' -f1 "$work/manifest" | sort -u > "$work/artifacts"
names "$in" > "$work/got"
same "$in" "$work/artifacts" "$work/got"
while read -r artifact; do
  if [ ! -d "$in/$artifact" ] || [ -L "$in/$artifact" ]; then
    fail "$in/$artifact isn't an artifact's folder"
  fi
  awk -v a="$artifact" '$1 == a { print $2 }' "$work/manifest" | sort > "$work/want"
  names "$in/$artifact" > "$work/got"
  same "the artifact $artifact" "$work/want" "$work/got"
done < "$work/artifacts"

# Each file into OUT, then its sum, from the copy, against its job's report.
mkdir -- "$out"
made=yes
while read -r artifact name sum; do
  if [ ! -f "$in/$artifact/$name" ] || [ -L "$in/$artifact/$name" ]; then
    fail "$artifact/$name isn't a regular file"
  fi
  cp -- "$in/$artifact/$name" "$out/$name"
  got=$(sum256 "$out/$name")
  [ "$got" = "$sum" ] || fail "$artifact/$name's SHA-256 is $got, not the $sum its job reported"
  echo "$name: as its job reported ($sum)"
done < "$work/manifest"
