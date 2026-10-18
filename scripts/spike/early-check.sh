#!/usr/bin/env bash
# The retranslation's early check (retranslation spec §6.5, step 4; RT16), sandbox only: new
# English into the run workspace's copy, one sandboxed run, and the copy put back.
#
#   scripts/spike/early-check.sh ENGLISH.tsv NAME [run-client.sh run options...] -- [client args...]
#
# `translate early apply` loads ENGLISH.tsv (fingerprint, tab, English, as fenji-content's files
# hold it) into re/runs/client the way the engine reads files (docs/re/retranslation-locale.md
# §4): a patch part with the rebuilt locale cache, and one more line in data/_index. Then
# run-client.sh runs the client once as NAME with --early-load, every guard on (the sandbox, the
# stub or server, the AGT driver watchdog), with the options and client arguments given;
# --fs=data:_index:_index is the caller's to pass, as for any run. The run takes the patch part
# into its evidence, and whatever its end, `translate early revert` puts the copy's own _index
# back. The pinned copy is never touched.
#
# Needs a release build of translate (cargo build --release -p fenji-translate) and a run
# workspace (scripts/spike/run-client.sh setup). Exit: run-client.sh's.
set -euo pipefail
cd "$(dirname "$0")/../.."

if [ $# -lt 2 ]; then
  sed -n '2,15p' "$0" >&2
  exit 2
fi
english=$1
name=$2
shift 2
translate=target/release/translate
[ -x "$translate" ] || { echo "early-check: build it first: cargo build --release -p fenji-translate" >&2; exit 2; }
[ -d re/runs/client ] || { echo "early-check: no run workspace: scripts/spike/run-client.sh setup" >&2; exit 2; }

"$translate" early apply --copy re/runs/client --english "$english"
trap '"$translate" early revert --copy re/runs/client' EXIT
scripts/spike/run-client.sh run "$name" --early-load "$@"
