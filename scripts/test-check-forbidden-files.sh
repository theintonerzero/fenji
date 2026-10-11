#!/usr/bin/env bash
# Tests for scripts/check-forbidden-files.sh. Runs anywhere bash runs (macOS bash 3.2, Fedora, CI).
set -euo pipefail

check="$(cd "$(dirname "$0")" && pwd)/check-forbidden-files.sh"
passed=0
ok()   { echo "ok - $1"; passed=$((passed + 1)); }
fail() { echo "FAIL - $1" >&2; exit 1; }

root=""
trap 'rm -rf "$root"' EXIT
root=$(mktemp -d)

# staged PATH FORMAT: a fresh repo with only PATH staged (content from printf FORMAT); prints its dir.
staged() {
  local d
  d=$(mktemp -d "$root/r.XXXXXX")
  git -C "$d" init -q
  mkdir -p "$d/$(dirname "$1")"
  # shellcheck disable=SC2059 # the format carries octal escapes on purpose
  printf "$2" > "$d/$1"
  git -C "$d" add -f -- "$1"
  echo "$d"
}

# rejected PATH FORMAT WHAT: the staged file must fail the check.
rejected() {
  local d
  d=$(staged "$1" "$2")
  if (cd "$d" && "$check" --staged) >/dev/null 2>&1; then fail "$3 ($1) was accepted"; fi
  ok "rejects $3 ($1)"
}

d=$(staged docs/notes.md '# notes\n')
(cd "$d" && "$check" --staged) >/dev/null 2>&1 || fail "an ordinary markdown file was rejected"
ok "accepts an ordinary file"

rejected bin64/gujianol.exe 'x' "an .exe by name"
rejected bin64/oo2core_6_win64.dll 'x' "a DLL by name"
rejected bin64/VisionEnginePlugin.vPlugin 'x' "a Vision engine plugin by name"
rejected tools/blob.bin 'MZ\220\000' "a PE binary by magic"
rejected data/data042 'x' "a data archive by name"
rejected data/_index 'x' "the archive index by name"
rejected out/chunk.bin '\033Lua\123\000' "Lua bytecode by magic"
rejected out/script.luac 'x' "compiled Lua by name"
rejected out/block.bin 'ZFS\000IX' "an archive block by magic"
rejected tables/item.tab 'x' "an extracted table"
rejected media/intro.bk2 'x' "a Bink video"
rejected audio/sfx.bank 'x' "an FMOD bank"
rejected cap/session.pcapng 'x' "a capture by name"
rejected cap/blob.bin '\012\015\015\012' "a pcapng capture by magic"
rejected re/notes.txt 'x' "anything under re/"
rejected ghidra/fenji.gpr 'x' "a Ghidra project"

d=$(staged big.txt 'x')
head -c $((5 * 1024 * 1024 + 1)) /dev/zero > "$d/big.txt"
git -C "$d" add -f big.txt
if (cd "$d" && "$check" --staged) >/dev/null 2>&1; then fail "a file over 5 MiB was accepted"; fi
ok "rejects a file over 5 MiB"

echo "all $passed checks passed"
