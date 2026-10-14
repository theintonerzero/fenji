#!/usr/bin/env bash
# Runs tools/oodle-helper under GE-Proton's Wine inside scripts/spike/isolated-run.sh, with the
# client's own oo2core_6_win64.dll (gate #39 route c: dev analysis on the owner's machines only).
# The DLL is loaded read-only from $FENJI_CLIENT_PATH/bin64; gujianol.exe is never started. No
# network but loopback; non-root.
#
#   scripts/spike/run-oodle-helper.sh [--spool DIR]
#       decodes the Oodle blocks fenji-data spooled (M2 spec §6). DIR defaults to re/oodle-spool.
#   scripts/spike/run-oodle-helper.sh --probe
#       says whether the DLL exports OodleLZ_Compress.
#   scripts/spike/run-oodle-helper.sh --compress IN OUT COMPRESSOR LEVEL
#       compresses each IN/*.raw, Fenji's own synthetic bytes, into OUT/<stem>.c<COMPRESSOR>.l<LEVEL>.oz,
#       for fenji-oodle's test fixtures (the Oodle decoder spec §4.2, approved by the owner 2026-10-14).
#
# Exit: the helper's status (0 all done, 1 some failed, 2 usage); 1 or 2 for setup errors here.
# Inside the sandbox the helper reads SPOOL/in and writes SPOOL/out only, or reads IN and writes OUT
# only; the probe reads the DLL alone. Each of those directories must be inside re/ and apart from
# the Wine prefix, and IN and OUT apart from each other. Wine there prints "Failed to open
# /etc/machine-id" and "XDG_RUNTIME_DIR is invalid or not set": both harmless.
set -euo pipefail
HERE=$(cd -- "$(dirname -- "$0")" && pwd -P)
REPO=$(cd -- "$HERE/../.." && pwd -P)
# shellcheck source=../versions.env
. "$REPO/scripts/versions.env"
PROTON="${FENJI_GE_PROTON:-$HOME/.local/share/fenji/tools/$GE_PROTON_VERSION}"
CLIENT="${FENJI_CLIENT_PATH:-$HOME/fenji-client}"
HELPER="$REPO/target/x86_64-pc-windows-gnu/release/oodle-helper.exe"
PREFIX="$REPO/re/oodle-prefix"
SPOOL=$(realpath -m -- "$REPO/re/oodle-spool")
MODE=""
die() { echo "run-oodle-helper: $*" >&2; exit 1; }
usage() { echo "usage: $0 [--spool DIR] | --probe | --compress IN OUT COMPRESSOR LEVEL" >&2; exit 2; }
# One mode a run: naming another is a usage error.
mode() { [ -z "$MODE" ] || [ "$MODE" = "$1" ] || usage; MODE=$1; }
while [ $# -gt 0 ]; do
  case "$1" in
    --spool) [ $# -ge 2 ] || die "--spool needs a directory"; mode spool; SPOOL=$(realpath -m -- "$2"); shift 2 ;;
    --probe) mode probe; shift ;;
    --compress)
      [ $# -ge 5 ] || die "--compress needs IN OUT COMPRESSOR LEVEL"
      mode compress
      IN=$(realpath -m -- "$2") OUT=$(realpath -m -- "$3") COMPRESSOR=$4 LEVEL=$5
      shift 5 ;;
    *) usage ;;
  esac
done
MODE=${MODE:-spool}
# inside_re WHAT DIR: DIR is inside re/, and apart from the Wine prefix.
inside_re() {
  case "$2/" in "$REPO/re/"?*) ;; *) die "$1 must be inside re/: $2" ;; esac
  case "$2/" in "$PREFIX/"*) die "$1 can't be inside the Wine prefix: $2" ;; esac
  case "$PREFIX/" in "$2/"*) die "$1 can't hold the Wine prefix: $2" ;; esac
}
case "$MODE" in
  spool) inside_re "the spool" "$SPOOL" ;;
  compress)
    # Digits spelled out: a range like [0-9] can match other characters in some locales.
    case "$COMPRESSOR" in ''|*[!0123456789]*) die "COMPRESSOR must be a whole number: $COMPRESSOR" ;; esac
    case "${LEVEL#-}" in ''|*[!0123456789]*) die "LEVEL must be a whole number: $LEVEL" ;; esac
    inside_re "the input" "$IN"
    inside_re "the output" "$OUT"
    # IN is bound read-only before OUT read-write: an IN inside OUT (or the same) would be writable,
    # and an OUT inside IN would put the helper's writes in its input.
    case "$OUT/" in "$IN/"*) die "the input and the output can't overlap: $IN, $OUT" ;; esac
    case "$IN/" in "$OUT/"*) die "the input and the output can't overlap: $IN, $OUT" ;; esac
    ;;
esac
[ -x "$PROTON/files/bin/wine" ] || die "GE-Proton not found at $PROTON (scripts/bootstrap.sh)"
[ -f "$HELPER" ] || die "build it first: cargo build --release -p fenji-oodle-helper --target x86_64-pc-windows-gnu"
DLL="$CLIENT/bin64/oo2core_6_win64.dll"
[ -f "$DLL" ] || die "no $DLL"
# Each mode's directories, and the helper's arguments after the DLL.
case "$MODE" in
  spool)
    mkdir -p "$SPOOL/in" "$SPOOL/out"
    DIRS=(--ro-bind "$SPOOL/in" --bind "$SPOOL/out") ARGS=("Z:$SPOOL") ;;
  probe) DIRS=() ARGS=(probe) ;;
  compress)
    [ -d "$IN" ] || die "no input directory: $IN"
    mkdir -p "$OUT"
    DIRS=(--ro-bind "$IN" --bind "$OUT") ARGS=(compress "Z:$IN" "Z:$OUT" "$COMPRESSOR" "$LEVEL") ;;
esac
mkdir -p "$PREFIX"
exec "$HERE/isolated-run.sh" --no-root \
  --ro-bind "$PROTON" --ro-bind "$CLIENT/bin64" --ro-bind "$(dirname -- "$HELPER")" \
  --bind "$PREFIX" "${DIRS[@]}" -- \
  env WINEPREFIX="$PREFIX" WINEDEBUG=-all WINEDLLOVERRIDES="mscoree,mshtml=" \
  "$PROTON/files/bin/wine" "$HELPER" "Z:$DLL" "${ARGS[@]}"
