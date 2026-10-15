#!/usr/bin/env bash
# Fails unless the desktop launcher's two versions, apps/launcher/package.json's and
# apps/launcher/src-tauri/Cargo.toml's, equal the workspace's ([workspace.package] in Cargo.toml).
# tauri.conf.json has no version of its own: the shell's Cargo.toml gives it.
# Usage: scripts/check-launcher-version.sh [repository root]
set -euo pipefail

root=${1:-"$(cd "$(dirname "$0")/.." && pwd)"}

# The `version = "…"` line in a TOML table, by the table's header.
toml_version() {
    sed -n "/^\[$2\]/,/^\[/s/^version = \"\(.*\)\"$/\1/p" "$1" | head -n 1
}

workspace=$(toml_version "$root/Cargo.toml" 'workspace\.package')
shell=$(toml_version "$root/apps/launcher/src-tauri/Cargo.toml" 'package')
view=$(python3 -I -c 'import json, sys; print(json.load(open(sys.argv[1]))["version"])' \
    "$root/apps/launcher/package.json")

if [ -z "$workspace" ]; then
    echo "check-launcher-version: no version in Cargo.toml's [workspace.package]" >&2
    exit 1
fi
status=0
for pair in "apps/launcher/package.json=$view" "apps/launcher/src-tauri/Cargo.toml=$shell"; do
    file=${pair%%=*}
    version=${pair#*=}
    if [ "$version" != "$workspace" ]; then
        echo "check-launcher-version: $file says ${version:-nothing}, the workspace $workspace" >&2
        status=1
    fi
done
exit "$status"
