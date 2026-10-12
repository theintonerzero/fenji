#!/usr/bin/env bash
# Runs tools/oodle-helper under GE-Proton's Wine inside scripts/spike/isolated-run.sh, to decode the
# Oodle blocks fenji-data spooled (M2 spec §6; gate #39 route c, dev analysis on the owner's machines
# only). The client's own oo2core_6_win64.dll is loaded read-only from $FENJI_CLIENT_PATH/bin64;
# gujianol.exe is never started. No network but loopback; non-root.
#
#   scripts/spike/run-oodle-helper.sh [--spool DIR]     (default DIR: re/oodle-spool)
#
# Exit: the helper's status (0 all decoded, 1 some failed, 2 usage); 1 or 2 for setup errors here.
set -euo pipefail
HERE=$(cd -- "$(dirname -- "$0")" && pwd -P)
REPO=$(cd -- "$HERE/../.." && pwd -P)
# shellcheck source=../versions.env
. "$REPO/scripts/versions.env"
PROTON="${FENJI_GE_PROTON:-$HOME/.local/share/fenji/tools/$GE_PROTON_VERSION}"
CLIENT="${FENJI_CLIENT_PATH:-$HOME/fenji-client}"
HELPER="$REPO/target/x86_64-pc-windows-gnu/release/oodle-helper.exe"
PREFIX="$REPO/re/oodle-prefix"
SPOOL="$REPO/re/oodle-spool"
die() { echo "run-oodle-helper: $*" >&2; exit 1; }
while [ $# -gt 0 ]; do
  case "$1" in
    --spool) [ $# -ge 2 ] || die "--spool needs a directory"; SPOOL=$(realpath -m -- "$2"); shift 2 ;;
    *) echo "usage: $0 [--spool DIR]" >&2; exit 2 ;;
  esac
done
case "$SPOOL/" in "$REPO/re/"*) ;; *) die "the spool must be under re/: $SPOOL" ;; esac
[ -x "$PROTON/files/bin/wine" ] || die "GE-Proton not found at $PROTON (scripts/bootstrap.sh)"
[ -f "$HELPER" ] || die "build it first: cargo build --release -p fenji-oodle-helper --target x86_64-pc-windows-gnu"
DLL="$CLIENT/bin64/oo2core_6_win64.dll"
[ -f "$DLL" ] || die "no $DLL"
mkdir -p "$PREFIX" "$SPOOL/in" "$SPOOL/out"
exec "$HERE/isolated-run.sh" --no-root \
  --ro-bind "$PROTON" --ro-bind "$CLIENT/bin64" --ro-bind "$(dirname -- "$HELPER")" \
  --bind "$PREFIX" --bind "$SPOOL" -- \
  env WINEPREFIX="$PREFIX" WINEDEBUG=-all WINEDLLOVERRIDES="mscoree,mshtml=" \
  "$PROTON/files/bin/wine" "$HELPER" "Z:$DLL" "Z:$SPOOL"
