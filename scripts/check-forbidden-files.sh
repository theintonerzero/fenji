#!/usr/bin/env bash
# Rejects game content and RE artefacts from the repo (design spec §2 rule 3, §9).
#
# Usage:
#   scripts/check-forbidden-files.sh            # check all tracked files (CI)
#   scripts/check-forbidden-files.sh --staged   # check staged files (pre-commit hook)
#
# It judges what a commit would contain, so content is read from the index and never from the working
# tree. Names are read NUL-delimited: unless asked for -z, git quotes names with non-ASCII bytes, tabs,
# newlines, '"' or '\' ("\351\205\215/x.exe"), and a quoted name is not a file. Everything is compared
# bytewise (LC_ALL=C).
#
# Compatible with macOS's bash 3.2.
set -euo pipefail
export LC_ALL=C
# Anything unexpected must be visibly a crash, not a quiet non-zero exit that looks like a rejection.
trap 'echo "check-forbidden-files.sh: failed unexpectedly (line $LINENO); the check did not complete" >&2' ERR

MAX_BYTES=$((5 * 1024 * 1024))

# git prints names relative to the root in some modes and to the current directory in others.
root=$(git rev-parse --show-toplevel)
cd "$root"

# The list goes through a temporary file, not process substitution, so that a failing git trips set -e.
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

if [ "${1:-}" = "--staged" ]; then
  # T (a symlink replaced by a regular file, or the reverse) changes the content, so it is checked too.
  git diff --cached --name-only -z --diff-filter=ACMRT > "$tmp/list"
else
  git ls-files -z > "$tmp/list"
fi

failed=0
fail() {
  printf 'forbidden: %q (%s)\n' "$1" "$2" >&2
  failed=1
}

# The first 8 bytes of the staged blob as lowercase hex. The final "|| true" is for SIGPIPE: head exits
# early on a large blob, and pipefail would turn that into a failure.
magic() {
  { git cat-file blob ":0:$1" 2>/dev/null | head -c 8 | od -An -tx1 | tr -d ' \n'; } || true
}

# The list is read on fd 3 so that nothing run inside the loop can consume it.
while IFS= read -r -d '' f <&3; do
  lower=$(printf '%s' "$f" | tr '[:upper:]' '[:lower:]')

  # The name rules come first: they need no content, so they cannot be skipped for want of a readable
  # file. The lowercase name is matched because macOS and Windows treat Re/ and re/ alike.
  case "$lower" in
    re/*) fail "$f" "local RE workspace"; continue ;;
  esac

  # Digits spelled out: a range like [0-9] can match other characters in some locales.
  case "${lower##*/}" in
    data[0123456789][0123456789][0123456789]|_index)
      fail "$f" "game data archive"; continue ;;
  esac
  case "$lower" in
    *.exe|*.dll|*.sys|*.pae|*.vplugin|*.luac|*.tab|*.bk2|*.bank|*.fsb|*.pcap|*.pcapng|*.gpr|*.hop|*.i64|*.idb)
      fail "$f" "forbidden file type"; continue ;;
    *.so|*.so.*|*.dylib)
      fail "$f" "native library (Oodle and other middleware ship as .so and .dylib)"; continue ;;
    *.rep/*)
      fail "$f" "Ghidra project data"; continue ;;
  esac

  # A submodule or an unmerged path has no blob at stage 0, so there is nothing to read (the name
  # rules above applied to it already).
  size=$(git cat-file -s ":0:$f" 2>/dev/null) || continue

  m=$(magic "$f")
  case "$m" in
    4d5a*) fail "$f" "Windows PE binary (MZ header)" ;;
    7f454c46*) fail "$f" "ELF binary" ;;
    # cafebabe is a universal (fat) Mach-O header, and also the header of Java class files.
    feedface*|feedfacf*|cefaedfe*|cffaedfe*|cafebabe*) fail "$f" "Mach-O binary" ;;
    1b4c7561*) fail "$f" "Lua bytecode" ;;
    5a465300*) fail "$f" "game archive block (ZFS)" ;;
    d4c3b2a1*|a1b2c3d4*|4d3cb2a1*|a1b23c4d*) fail "$f" "pcap capture" ;;
    0a0d0d0a*) fail "$f" "pcapng capture" ;;
  esac

  if [ "$size" -gt "$MAX_BYTES" ]; then
    fail "$f" "larger than 5 MiB; game data must never be committed"
  fi
done 3< "$tmp/list"

if [ "$failed" -ne 0 ]; then
  echo "Game binaries, archives, scripts, captures and RE artefacts must stay out of the repo (see docs/re/README.md)." >&2
  exit 1
fi
