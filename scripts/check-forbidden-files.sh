#!/usr/bin/env bash
# Rejects game content and RE artefacts from the repo (design spec §2 rule 3, §9).
#
# Usage:
#   scripts/check-forbidden-files.sh            # check all tracked files (CI)
#   scripts/check-forbidden-files.sh --staged   # check staged files (pre-commit hook)
#
# Compatible with macOS's bash 3.2.
set -euo pipefail

MAX_BYTES=$((5 * 1024 * 1024))

if [ "${1:-}" = "--staged" ]; then
  files=$(git diff --cached --name-only --diff-filter=ACMR)
else
  files=$(git ls-files)
fi

failed=0
fail() {
  echo "forbidden: $1 ($2)" >&2
  failed=1
}

# Reads the first 8 bytes of a file as lowercase hex.
magic() {
  head -c 8 "$1" 2>/dev/null | od -An -tx1 | tr -d ' \n'
}

while IFS= read -r f; do
  [ -z "$f" ] && continue
  [ -f "$f" ] || continue

  case "$f" in
    re/*) fail "$f" "local RE workspace"; continue ;;
  esac

  lower=$(printf '%s' "$f" | tr '[:upper:]' '[:lower:]')
  # Digits spelled out: a range like [0-9] can match other characters in some locales.
  case "${lower##*/}" in
    data[0123456789][0123456789][0123456789]|_index)
      fail "$f" "game data archive"; continue ;;
  esac
  case "$lower" in
    *.exe|*.dll|*.sys|*.pae|*.vplugin|*.luac|*.tab|*.bk2|*.bank|*.fsb|*.pcap|*.pcapng|*.gpr|*.hop|*.i64|*.idb)
      fail "$f" "forbidden file type"; continue ;;
    *.rep/*)
      fail "$f" "Ghidra project data"; continue ;;
  esac

  m=$(magic "$f")
  case "$m" in
    4d5a*) fail "$f" "Windows PE binary (MZ header)" ;;
    1b4c7561*) fail "$f" "Lua bytecode" ;;
    5a465300*) fail "$f" "game archive block (ZFS)" ;;
    d4c3b2a1*|a1b2c3d4*|4d3cb2a1*|a1b23c4d*) fail "$f" "pcap capture" ;;
    0a0d0d0a*) fail "$f" "pcapng capture" ;;
  esac

  size=$(wc -c < "$f" | tr -d ' ')
  if [ "$size" -gt "$MAX_BYTES" ]; then
    fail "$f" "larger than 5 MiB; game data must never be committed"
  fi
done <<EOF
$files
EOF

if [ "$failed" -ne 0 ]; then
  echo "Game binaries, archives, scripts, captures and RE artefacts must stay out of the repo (see docs/re/README.md)." >&2
  exit 1
fi
