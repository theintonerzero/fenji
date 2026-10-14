#!/usr/bin/env bash
# Tests for run-oodle-helper.sh's guards and binds, with no sandbox, no Wine and no client: they keep
# the client's decoded data and the fixtures' inputs and outputs inside the git-ignored re/ (ground
# rule 3) and away from the Wine prefix, and keep each mode's binds to what it needs.
#   scripts/spike/test-run-oodle-helper.sh
#
# Every guard runs before the script creates anything. A refused directory dies at its guard; an
# accepted one dies at the next check, because FENJI_GE_PROTON points nowhere, so no case writes a
# file. The bind cases at the end run a copy of the script in a fake repository under a temporary
# directory, whose isolated-run.sh prints the arguments it is given and starts nothing.
set -uo pipefail
HERE=$(cd -- "$(dirname -- "$0")" && pwd -P)
REPO=$(cd -- "$HERE/../.." && pwd -P)
RUN=$HERE/run-oodle-helper.sh
export FENJI_GE_PROTON=/nonexistent
fails=0
ok() { echo "ok   $1"; }
bad() { echo "FAIL $1"; fails=$((fails + 1)); }

# expect WANT_EXIT PATTERN WHAT -- ARGS...: $RUN ARGS, run from $REPO, exits WANT_EXIT with PATTERN
# in its message.
expect() {
  local want=$1 pattern=$2 what=$3 err rc
  shift 4
  err=$(cd "$REPO" && "$RUN" "$@" 2>&1 > /dev/null)
  rc=$?
  if [ "$rc" = "$want" ] && grep -q -- "$pattern" <<< "$err"; then ok "$what"; else bad "$what: exit $rc: $err"; fi
}

expect 1 "must be inside re/" "re/ itself is refused" -- --spool re
expect 1 "must be inside re/" "a spool outside the repo is refused" -- --spool /tmp/fenji-spool
expect 1 "must be inside re/" "a spool that climbs out of re/ is refused" -- --spool re/../docs/spool
expect 1 "inside the Wine prefix" "the Wine prefix is refused" -- --spool re/oodle-prefix
expect 1 "inside the Wine prefix" "a spool inside the Wine prefix is refused" -- --spool re/oodle-prefix/spool
expect 1 "GE-Proton not found" "the default spool passes the guards" --
expect 1 "GE-Proton not found" "a spool elsewhere inside re/ passes the guards" -- --spool re/m2/spool
expect 2 "usage" "an unknown argument is a usage error" -- --nope

# The probe takes nothing more, and runs in one mode only.
expect 1 "GE-Proton not found" "the probe passes the guards" -- --probe
expect 2 "usage" "the probe takes no directory" -- --probe re/oodle-spool
expect 2 "usage" "the probe and the spool together are a usage error" -- --spool re/m2/spool --probe
expect 2 "usage" "the probe and compress together are a usage error" -- --probe --compress re/in re/out 8 4

# compress: four values, two whole numbers, and two directories inside re/ that stay apart.
expect 1 "GE-Proton not found" "compress inside re/ passes the guards" -- --compress re/oodle-fixtures/in re/oodle-fixtures/out 8 4
expect 1 "GE-Proton not found" "a negative level passes the guards" -- --compress re/in re/out 8 -4
expect 1 "needs IN OUT COMPRESSOR LEVEL" "compress with three values is refused" -- --compress re/in re/out 8
expect 1 "COMPRESSOR must be a whole number" "a compressor that isn't a number is refused" -- --compress re/in re/out kraken 4
expect 1 "COMPRESSOR must be a whole number" "a negative compressor is refused" -- --compress re/in re/out -8 4
expect 1 "LEVEL must be a whole number" "a level that isn't a number is refused" -- --compress re/in re/out 8 4x
expect 1 "LEVEL must be a whole number" "an empty level is refused" -- --compress re/in re/out 8 ""
expect 1 "LEVEL must be a whole number" "a lone minus is refused as a level" -- --compress re/in re/out 8 -
expect 1 "the input must be inside re/" "re/ itself is refused as the input" -- --compress re re/out 8 4
expect 1 "the input must be inside re/" "an input outside the repo is refused" -- --compress /tmp/fenji-in re/out 8 4
expect 1 "the input must be inside re/" "an input that climbs out of re/ is refused" -- --compress re/../docs re/out 8 4
expect 1 "the output must be inside re/" "re/ itself is refused as the output" -- --compress re/in re 8 4
expect 1 "the output must be inside re/" "an output outside the repo is refused" -- --compress re/in /tmp/fenji-out 8 4
expect 1 "the output must be inside re/" "an output that climbs out of re/ is refused" -- --compress re/in re/../crates 8 4
expect 1 "the input can't be inside the Wine prefix" "an input inside the Wine prefix is refused" -- --compress re/oodle-prefix/in re/out 8 4
expect 1 "the output can't be inside the Wine prefix" "the Wine prefix is refused as the output" -- --compress re/in re/oodle-prefix 8 4
expect 1 "can't overlap" "one directory for both is refused" -- --compress re/fx re/fx 8 4
expect 1 "can't overlap" "an output inside the input is refused" -- --compress re/fx re/fx/out 8 4
expect 1 "can't overlap" "an input inside the output is refused" -- --compress re/fx/in re/fx 8 4
expect 1 "GE-Proton not found" "siblings that share a prefix don't overlap" -- --compress re/fx re/fx2 8 4

# A link inside re/ that leads out of it: the guards compare the resolved paths.
link="$REPO/re/.test-oodle-helper-$$"
mkdir -p "$REPO/re"
ln -s /tmp "$link"
expect 1 "must be inside re/" "a spool through a link out of re/ is refused" -- --spool "re/${link##*/}/spool"
expect 1 "the input must be inside re/" "an input through a link out of re/ is refused" -- --compress "re/${link##*/}/in" re/out 8 4
expect 1 "the output must be inside re/" "an output through a link out of re/ is refused" -- --compress re/in "re/${link##*/}/out" 8 4
rm -f -- "$link"

# The binds, through a copy of the script in a fake repository: GE-Proton, the client and the helper
# are empty files there, and its isolated-run.sh prints its arguments, one a line.
FAKE=$(mktemp -d) || { echo "FATAL: mktemp failed" >&2; exit 2; }
FAKE=$(cd -- "$FAKE" && pwd -P) || exit 2
trap 'rm -rf -- "$FAKE"' EXIT
case "$FAKE" in /?*) ;; *) echo "FATAL: no fake repository" >&2; exit 2 ;; esac
BIN=$FAKE/target/x86_64-pc-windows-gnu/release
mkdir -p "$FAKE/scripts/spike" "$FAKE/proton/files/bin" "$FAKE/client/bin64" "$BIN"
cp -- "$RUN" "$FAKE/scripts/spike/run-oodle-helper.sh"
printf 'GE_PROTON_VERSION=fake\n' > "$FAKE/scripts/versions.env"
printf '#!/bin/sh\nprintf "%%s\\n" "$@"\n' > "$FAKE/scripts/spike/isolated-run.sh"
: > "$FAKE/proton/files/bin/wine"
: > "$FAKE/client/bin64/oo2core_6_win64.dll"
: > "$BIN/oodle-helper.exe"
chmod +x "$FAKE/scripts/spike/isolated-run.sh" "$FAKE/proton/files/bin/wine"
REPO=$FAKE RUN=$FAKE/scripts/spike/run-oodle-helper.sh
export FENJI_GE_PROTON=$FAKE/proton FENJI_CLIENT_PATH=$FAKE/client

# binds WHAT -- ARGS...: $RUN ARGS, run from $REPO, hands isolated-run.sh exactly the array want.
binds() {
  local what=$1 got rc
  shift 2
  got=$(cd "$REPO" && "$RUN" "$@" 2>&1)
  rc=$?
  if [ "$rc" = 0 ] && [ "$got" = "$(printf '%s\n' "${want[@]}")" ]; then ok "$what"; else bad "$what: exit $rc: $got"; fi
}

# What every mode binds and runs.
P=$FAKE/re/oodle-prefix
base=(--no-root --ro-bind "$FAKE/proton" --ro-bind "$FAKE/client/bin64" --ro-bind "$BIN" --bind "$P")
wine=(env "WINEPREFIX=$P" WINEDEBUG=-all "WINEDLLOVERRIDES=mscoree,mshtml=" "$FAKE/proton/files/bin/wine"
  "$BIN/oodle-helper.exe" "Z:$FAKE/client/bin64/oo2core_6_win64.dll")

want=("${base[@]}" -- "${wine[@]}" probe)
binds "the probe binds no spool, input or output" -- --probe
if [ -d "$P" ] && [ ! -e "$FAKE/re/oodle-spool" ]; then ok "the probe makes the Wine prefix and no spool"; else bad "the probe makes the Wine prefix and no spool"; fi

S=$FAKE/re/oodle-spool
want=("${base[@]}" --ro-bind "$S/in" --bind "$S/out" -- "${wine[@]}" "Z:$S")
binds "the spool binds SPOOL/in read-only and SPOOL/out read-write" --

expect 1 "no input directory" "compress needs its input to exist" -- --compress re/fx/in re/fx/out 8 4
if [ ! -e "$FAKE/re/fx" ]; then ok "a refused compress makes no output"; else bad "a refused compress makes no output"; fi
mkdir -p "$FAKE/re/fx/in"
want=("${base[@]}" --ro-bind "$FAKE/re/fx/in" --bind "$FAKE/re/fx/out" -- "${wine[@]}"
  compress "Z:$FAKE/re/fx/in" "Z:$FAKE/re/fx/out" 8 -4)
binds "compress binds IN read-only and OUT read-write, and passes the numbers on" -- --compress re/fx/in re/fx/out 8 -4
if [ -d "$FAKE/re/fx/out" ]; then ok "compress makes its output"; else bad "compress makes its output"; fi

[ "$fails" = 0 ] || { echo "$fails failed"; exit 1; }
echo "all passed"
