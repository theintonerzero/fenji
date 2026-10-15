#!/usr/bin/env bash
# Tests scripts/check-launcher-version.sh against a temporary copy of the three files.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
repo="$here/.."
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

mkdir -p "$tmp/apps/launcher/src-tauri"
cp "$repo/Cargo.toml" "$tmp/Cargo.toml"
cp "$repo/apps/launcher/package.json" "$tmp/apps/launcher/package.json"
cp "$repo/apps/launcher/src-tauri/Cargo.toml" "$tmp/apps/launcher/src-tauri/Cargo.toml"

"$here/check-launcher-version.sh" "$tmp"
echo "ok: the repository's versions agree"

sed -i 's/"version": "[^"]*"/"version": "9.9.9"/' "$tmp/apps/launcher/package.json"
if "$here/check-launcher-version.sh" "$tmp" 2>/dev/null; then
    echo "FAIL: a package.json with another version passed" >&2
    exit 1
fi
echo "ok: a package.json with another version is refused"
