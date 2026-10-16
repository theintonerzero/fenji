#!/usr/bin/env bash
# Keeps unsafe Rust to the two files allowed it. The workspace forbids `unsafe_code`; the server and
# the Oodle helper deny it instead, so that one file each may allow it: crates/fenji-server/src/unix.rs
# (sigaction and getpwuid_r, which have no safe binding) and tools/oodle-helper/src/dll.rs (the
# Oodle DLL's calls). Unlike forbid, deny gives way to an allow anywhere, so this refuses an allow,
# expect or warn of `unsafe_code` in any other Rust file, its parentheses over several lines
# included (the hosting plan's final fix pass's re-review).
#
# Usage:
#   scripts/check-unsafe-allowed.sh [DIR]   # the repository DIR is in, or this one (CI)
#
# It reads the working tree: the tracked files, and the untracked ones git doesn't ignore.
#
# Compatible with macOS's bash 3.2.
set -euo pipefail
export LC_ALL=C
# Anything unexpected must be visibly a crash, not a quiet non-zero exit that looks like a refusal.
trap 'echo "check-unsafe-allowed.sh: failed unexpectedly (line $LINENO); the check did not complete" >&2' ERR

root=$(git -C "${1:-$(dirname "$0")}" rev-parse --show-toplevel)
cd "$root"

# The list goes through a temporary file, not process substitution, so that a failing git trips set -e.
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
git ls-files -z --cached --others --exclude-standard -- '*.rs' > "$tmp/files"

# An attribute that lowers unsafe_code's level: allow(unsafe_code), expect(dead_code, unsafe_code),
# cfg_attr(unix, warn(unsafe_code)) and the like.
pattern='(allow|expect|warn)[[:space:]]*\([^)]*unsafe_code'

found=0
while IFS= read -r -d '' file; do
  case "$file" in
    crates/fenji-server/src/unix.rs | tools/oodle-helper/src/dll.rs) continue ;;
  esac
  # Read as one line, so that an attribute over several lines is seen whole; grep reads it all, so
  # tr never meets a closed pipe. A tracked file deleted from the working tree has nothing to read.
  if [ -f "$file" ] && tr '\n' ' ' < "$file" | grep -E -- "$pattern" > /dev/null; then
    echo "unsafe code allowed: $file" >&2
    found=1
  fi
done < "$tmp/files"

if [ "$found" -ne 0 ]; then
  echo "Only crates/fenji-server/src/unix.rs and tools/oodle-helper/src/dll.rs may allow unsafe_code:" >&2
  echo "move the unsafe code into one of them, or do without it." >&2
  exit 1
fi
