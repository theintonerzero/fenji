#!/usr/bin/env bash
# Writes the launcher updater's latest.json for a release (launcher spec §3.8; plan Task 15).
#
#   scripts/launcher-latest-json.sh <version> <notes-file> <bundle-dir>
#
# Reads <bundle-dir>'s three bundles, each beside its updater signature (<bundle>.sig), and writes
# <bundle-dir>/latest.json, then prints its path:
#   - linux-x86_64: the *.AppImage;
#   - windows-x86_64: the *-setup.exe (NSIS);
#   - darwin-aarch64: the *.app.tar.gz.
# Each platform's url is the release's download address for its file, under the tag v<version>, and
# its signature is the .sig file's text. The version is the tag's without its v (0.5.0, or
# 0.5.0-rc.1); pub_date is now, in UTC (RFC 3339); notes is <notes-file>'s text.
# Exits 1, writing nothing, when a platform has no bundle, more than one, or no signature, or when a
# file's name would need escaping in a URL. Needs jq. Compatible with macOS's bash 3.2.
set -euo pipefail
export LC_ALL=C

RELEASES=https://github.com/theintonerzero/fenji/releases/download

usage() {
  echo "usage: $0 <version> <notes-file> <bundle-dir>" >&2
  exit 2
}

# fail MESSAGE [STATUS]: says MESSAGE, and exits with STATUS, 1 by default.
fail() {
  echo "launcher-latest-json.sh: $1" >&2
  exit "${2:-1}"
}

[ $# -eq 3 ] || usage
version=$1 notes=$2 dir=$3
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] ||
  fail "'$version' isn't a version such as 0.5.0 or 0.5.0-rc.1 (no v)" 2
[ -f "$notes" ] || fail "no notes file at $notes" 2
[ -d "$dir" ] || fail "no bundle folder at $dir" 2
command -v jq > /dev/null || fail "jq is needed"

# bundle PATTERN: the one bundle in the folder whose name matches PATTERN, with its signature.
bundle() {
  local found=() f
  for f in "$dir"/$1; do
    [ -f "$f" ] && found+=("$f")
  done
  [ "${#found[@]}" -eq 1 ] || fail "want one $1 in $dir, found ${#found[@]}"
  f=$(basename -- "${found[0]}")
  [[ "$f" =~ ^[A-Za-z0-9._-]+$ ]] || fail "'$f' would need escaping in a URL; rename it"
  [ -f "${found[0]}.sig" ] || fail "$f has no signature ($f.sig)"
  echo "$f"
}
linux=$(bundle '*.AppImage')
windows=$(bundle '*-setup.exe')
mac=$(bundle '*.app.tar.gz')

out=$dir/latest.json
tmp=$(mktemp "$dir/.latest.json.XXXXXX")
trap 'rm -f -- "$tmp"' EXIT
jq -n \
  --arg version "$version" \
  --arg date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --rawfile notes "$notes" \
  --arg url "$RELEASES/v$version" \
  --arg linux "$linux" --rawfile linux_sig "$dir/$linux.sig" \
  --arg windows "$windows" --rawfile windows_sig "$dir/$windows.sig" \
  --arg mac "$mac" --rawfile mac_sig "$dir/$mac.sig" \
  'def text: sub("\\s+$"; "");
   {
     version: $version,
     notes: ($notes | text),
     pub_date: $date,
     platforms: {
       "linux-x86_64": {url: "\($url)/\($linux)", signature: ($linux_sig | text)},
       "windows-x86_64": {url: "\($url)/\($windows)", signature: ($windows_sig | text)},
       "darwin-aarch64": {url: "\($url)/\($mac)", signature: ($mac_sig | text)}
     }
   }' > "$tmp"
mv -f -- "$tmp" "$out"
trap - EXIT
echo "$out"
