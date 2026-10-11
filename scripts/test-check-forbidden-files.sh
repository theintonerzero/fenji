#!/usr/bin/env bash
# Tests for scripts/check-forbidden-files.sh. Runs anywhere bash runs (macOS bash 3.2, Fedora, CI).
#
# Each case builds a scratch repo, stages or commits something and runs the checker in it. A rejection
# has to come from the checker itself: exit 1 and a "forbidden: " line on stderr. A crash that happens
# to exit non-zero (git failing on an option-like file name, say) is not a rejection.
set -euo pipefail

check="$(cd "$(dirname "$0")" && pwd)/check-forbidden-files.sh"
passed=0
failed=0
ok()   { echo "ok - $1"; passed=$((passed + 1)); }
fail() { echo "FAIL - $1" >&2; failed=$((failed + 1)); }

root=""
trap 'rm -rf "$root"' EXIT
root=$(mktemp -d)

# Keep the developer's git setup (signing, hooks path, quotePath, ...) and any hook environment out of
# the scratch repos.
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE
export HOME="$root/home" XDG_CONFIG_HOME="$root/home/.config" GIT_CONFIG_NOSYSTEM=1
mkdir -p "$HOME"

# git_in DIR ARGS...: git in a scratch repo, with an identity and without signing.
git_in() {
  local d=$1
  shift
  git -C "$d" -c user.name=Test -c user.email=test@example.invalid -c commit.gpgsign=false "$@"
}

# newrepo: an empty repo; prints its directory. quotePath is set explicitly although it is git's
# default: quoted names are what trip up a checker that reads git's line-based output.
newrepo() {
  local d
  d=$(mktemp -d "$root/r.XXXXXX")
  git -c init.defaultBranch=main -C "$d" init -q
  git -C "$d" config core.quotePath true
  echo "$d"
}

# put DIR PATH FORMAT: writes PATH under DIR (content from printf FORMAT), creating directories.
put() {
  mkdir -p "$1/$(dirname "./$2")"
  # shellcheck disable=SC2059 # the format carries octal escapes on purpose
  printf "$3" > "$1/$2"
}

# bigfile PATH: writes a file one byte over the 5 MiB limit.
bigfile() { head -c $((5 * 1024 * 1024 + 1)) /dev/zero > "$1"; }

# stage PATH FORMAT: a fresh repo with only PATH staged; prints its directory.
stage() {
  local d
  d=$(newrepo)
  put "$d" "$1" "$2"
  git --literal-pathspecs -C "$d" add -f -- "$1"
  echo "$d"
}

# commit DIR MESSAGE: commits what is staged, skipping hooks.
commit() { git_in "$1" commit -q --no-verify -m "$2"; }

# expect_reject DIR WHAT [CHECKER-ARGS...]: the checker, run inside DIR, must exit 1 and say "forbidden: ".
expect_reject() {
  local d=$1 what=$2 out rc=0
  shift 2
  out=$(cd "$d" && "$check" "$@" 2>&1 >/dev/null </dev/null) || rc=$?
  if [ "$rc" -eq 1 ] && grep -q '^forbidden: ' <<<"$out"; then
    ok "rejects $what"
  else
    fail "rejects $what (exit $rc; stderr: $(head -n 1 <<<"$out"))"
  fi
}

# expect_accept DIR WHAT [CHECKER-ARGS...]: the checker, run inside DIR, must exit 0.
expect_accept() {
  local d=$1 what=$2 out rc=0
  shift 2
  out=$(cd "$d" && "$check" "$@" 2>&1 </dev/null) || rc=$?
  if [ "$rc" -eq 0 ]; then
    ok "accepts $what"
  else
    fail "accepts $what (exit $rc; output: $(head -n 1 <<<"$out"))"
  fi
}

# expect_error DIR WHAT [CHECKER-ARGS...]: a checker that cannot do its job must fail loudly: non-zero,
# no "forbidden: " (nothing was judged) and its own "failed unexpectedly" line.
expect_error() {
  local d=$1 what=$2 out rc=0
  shift 2
  out=$(cd "$d" && "$check" "$@" 2>&1 >/dev/null </dev/null) || rc=$?
  if [ "$rc" -ne 0 ] && grep -q 'failed unexpectedly' <<<"$out" && ! grep -q '^forbidden: ' <<<"$out"; then
    ok "fails loudly on $what"
  else
    fail "fails loudly on $what (exit $rc; stderr: $(head -n 1 <<<"$out"))"
  fi
}

# rejected PATH FORMAT WHAT: PATH, staged alone, must be rejected.
rejected() { expect_reject "$(stage "$1" "$2")" "$3 ($(printf '%q' "$1"))" --staged; }

# ---- ordinary content is accepted

expect_accept "$(stage docs/notes.md '# notes\n')" "an ordinary file" --staged
expect_accept "$(stage 文档/说明.md 'x')" "an ordinary file with a non-ASCII name" --staged
expect_accept "$(stage "$(printf 'a\nb.txt')" 'x')" "an ordinary file with a newline in its name" --staged
expect_accept "$(stage docs/re/README.md 'x')" "a file under docs/re/ (only a top-level re/ is the RE workspace)" --staged
expect_accept "$(stage docs/dylib-notes.md 'x')" "a name that only mentions a library suffix" --staged

# ---- game content, by name and by magic

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

d=$(stage big.txt 'x')
bigfile "$d/big.txt"
git -C "$d" add -f big.txt
expect_reject "$d" "a file over 5 MiB" --staged

# ---- native libraries: Oodle ships as .dll, .so and .dylib, and any ELF or Mach-O file is out too

rejected lib/liboo2core.so 'x' "a Linux shared library by name"
rejected lib/liboo2corelinux64.so.9 'x' "a versioned shared library by name"
rejected lib/liboo2coremac64.dylib 'x' "a macOS dynamic library by name"
rejected tools/blob.bin '\177ELF\002\001\001\000' "an ELF binary by magic"
rejected tools/blob.bin '\376\355\372\316' "a 32-bit Mach-O binary by magic"
rejected tools/blob.bin '\376\355\372\317' "a 64-bit Mach-O binary by magic"
rejected tools/blob.bin '\316\372\355\376' "a byte-swapped 32-bit Mach-O binary by magic"
rejected tools/blob.bin '\317\372\355\376' "a byte-swapped 64-bit Mach-O binary by magic"
rejected tools/blob.bin '\312\376\272\276' "a universal (fat) Mach-O binary by magic"

# .gitignore mirrors the library name rules, so `git add .` does not even pick a stray library up.
d=$(newrepo)
cp "$(dirname "$check")/../.gitignore" "$d/.gitignore"
unignored=""
for name in lib/liboo2core.so lib/liboo2corelinux64.so.9 lib/liboo2coremac64.dylib; do
  git -C "$d" check-ignore -q "$name" || unignored="$unignored $name"
done
if [ -z "$unignored" ]; then ok ".gitignore ignores shared libraries"; else fail ".gitignore ignores shared libraries (not ignored:$unignored)"; fi

# ---- names git quotes: unless asked for -z it prints "\351\205\215/x.exe", which is not a file

rejected 'données/blob.bin' 'MZ\220\000' "a PE binary in a directory with a non-ASCII name"
rejected '配置/gujianol.exe' 'x' "an .exe in a CJK directory"
rejected 'data/古剑奇谭.exe' 'x' "an .exe with a CJK file name"
rejected '资源/data042' 'x' "a data archive in a CJK directory"
rejected "$(printf 'a\tb.exe')" 'x' "an .exe with a tab in its name"
rejected "$(printf 'a\nb.exe')" 'x' "an .exe with a newline in its name"
rejected 'a"b.exe' 'x' "an .exe with a double quote in its name"
rejected 'a\b.exe' 'x' "an .exe with a backslash in its name"
rejected 'x:1^2~3@{u}*?[a].bin' 'MZ\220\000' "a PE binary whose name looks like revision and glob syntax"
rejected --version 'MZ\220\000' "a PE binary named like a long option"
rejected -x 'MZ\220\000' "a PE binary named like a short option"
rejected Re/notes.txt 'x' "a capitalised Re/ directory"

# ---- the index is what a commit contains: read it, never the working tree

d=$(stage payload.bin 'MZ\220\000')
put "$d" payload.bin 'harmless'
expect_reject "$d" "a staged PE blob whose working copy was rewritten" --staged

d=$(stage game.exe 'x')
rm "$d/game.exe"
expect_reject "$d" "a staged .exe that was deleted from the working tree" --staged

d=$(stage big.txt 'x')
bigfile "$d/big.txt"
git -C "$d" add -f big.txt
put "$d" big.txt 'small'
expect_reject "$d" "a staged blob over 5 MiB whose working copy was truncated" --staged

d=$(stage notes.txt 'small')
bigfile "$d/notes.txt"
expect_accept "$d" "a small staged blob whose working copy grew past 5 MiB (the commit is fine)" --staged

d=$(stage notes.txt 'small')
put "$d" notes.txt 'MZ\220\000'
expect_accept "$d" "a harmless staged blob whose working copy became a PE file (the commit is fine)" --staged

# ---- a type change (symlink <-> regular file) changes the content, so it is checked

d=$(newrepo)
ln -s nowhere "$d/payload"
git -C "$d" add payload
commit "$d" "chore: add a link"
rm "$d/payload"
put "$d" payload 'MZ\220\000'
git -C "$d" add payload
expect_reject "$d" "a symlink replaced by a PE binary (type change)" --staged

d=$(newrepo)
ln -s nowhere "$d/loader.exe"
git -C "$d" add -f loader.exe
expect_reject "$d" "a dangling symlink named like an .exe" --staged

# ---- a rename is judged by its new name

d=$(stage notes.md 'x')
commit "$d" "docs: add notes"
git -C "$d" mv notes.md notes.exe
expect_reject "$d" "a harmless file renamed to .exe" --staged

# ---- run from a subdirectory

d=$(stage bin/game.exe 'x')
expect_reject "$d/bin" "a staged .exe when run from a subdirectory" --staged

# ---- default mode (CI): every tracked file

d=$(stage bin/game.exe 'x')
commit "$d" "chore: add an exe"
put "$d" docs/notes.md '# notes\n'
git -C "$d" add docs/notes.md
commit "$d" "docs: add notes"
expect_reject "$d" "a committed .exe (default mode)"
expect_reject "$d/bin" "a committed .exe (default mode, run from its own directory)"
expect_reject "$d/docs" "a committed .exe outside the directory the check ran from (default mode)"
rm "$d/bin/game.exe"
expect_reject "$d" "a committed .exe that is missing from the working tree (default mode)"

d=$(stage '配置/blob.bin' 'MZ\220\000')
commit "$d" "chore: add a blob"
expect_reject "$d" "a committed PE blob in a CJK directory (default mode)"

d=$(stage '配置/gujianol.exe' 'x')
commit "$d" "chore: add an exe"
expect_reject "$d" "a committed .exe in a CJK directory (default mode)"

d=$(stage docs/notes.md '# notes\n')
commit "$d" "docs: add notes"
expect_accept "$d" "a clean tree (default mode)"
expect_accept "$d/docs" "a clean tree (default mode, run from a subdirectory)"
expect_accept "$d/docs" "a clean index (run from a subdirectory)" --staged

# ---- entries with nothing to read: a submodule, and a path in conflict

d=$(newrepo)
git -C "$d" update-index --add --cacheinfo 160000,1111111111111111111111111111111111111111,vendor/sub
expect_accept "$d" "a submodule entry (--staged)" --staged
expect_accept "$d" "a submodule entry (default mode)"
git -C "$d" update-index --add --cacheinfo 160000,1111111111111111111111111111111111111111,vendor/sub.exe
expect_reject "$d" "a submodule entry named like an .exe" --staged

d=$(newrepo)
blob=$(printf x | git -C "$d" hash-object -w --stdin)
printf '100644 %s 1\tconflict.txt\n100644 %s 2\tconflict.txt\n100644 %s 3\tconflict.txt\n' "$blob" "$blob" "$blob" |
  git -C "$d" update-index --index-info
expect_accept "$d" "an unmerged path (default mode)"
printf '100644 %s 1\tconflict.exe\n100644 %s 2\tconflict.exe\n100644 %s 3\tconflict.exe\n' "$blob" "$blob" "$blob" |
  git -C "$d" update-index --index-info
expect_reject "$d" "an unmerged path named like an .exe (default mode)"

# ---- a git that fails must fail the check, not look like an empty list

d=$(stage docs/notes.md 'x')
printf garbage > "$d/.git/index"
expect_error "$d" "a corrupt index (--staged)" --staged
expect_error "$d" "a corrupt index (default mode)"

# ---- end to end through a real pre-commit hook, as .githooks/pre-commit runs it

# hooked: an empty repo wired up like this one, with this repo's own .githooks/pre-commit and checker
# (untracked, so they are not part of what gets committed); prints its directory.
hooked() {
  local d here
  here=$(cd "$(dirname "$check")/.." && pwd)
  d=$(newrepo)
  mkdir "$d/scripts" "$d/.githooks"
  cp "$check" "$d/scripts/check-forbidden-files.sh"
  cp "$here/.githooks/pre-commit" "$d/.githooks/pre-commit"
  git -C "$d" config core.hooksPath .githooks
  echo "$d"
}

# allowed DIR WHAT ARGS...: "git commit ARGS" in DIR must succeed.
allowed() {
  local d=$1 what=$2
  shift 2
  if git_in "$d" commit -q "$@" >/dev/null 2>&1; then ok "the hook lets $what through"; else fail "the hook lets $what through"; fi
}

# blocked DIR WHAT ARGS...: "git commit ARGS" in DIR must fail and leave HEAD where it was.
blocked() {
  local d=$1 what=$2 before after
  shift 2
  before=$(git -C "$d" rev-parse -q --verify HEAD || echo none)
  if git_in "$d" commit -q "$@" >/dev/null 2>&1; then
    fail "the hook blocks $what (the commit went through)"
    return
  fi
  after=$(git -C "$d" rev-parse -q --verify HEAD || echo none)
  if [ "$before" = "$after" ]; then ok "the hook blocks $what"; else fail "the hook blocks $what (HEAD moved)"; fi
}

d=$(hooked)
put "$d" docs/notes.md '# notes\n'
git -C "$d" add docs/notes.md
allowed "$d" "an ordinary commit" -m "docs: add notes"

put "$d" '配置/gujianol.exe' 'x'
git -C "$d" add -f -- 配置/gujianol.exe
blocked "$d" "a commit of an .exe in a CJK directory" -m "chore: add an exe"
git -C "$d" reset -q

put "$d" docs/payload.bin 'harmless'
git -C "$d" add docs/payload.bin
allowed "$d" "a second ordinary commit" -m "chore: add a payload"
put "$d" docs/payload.bin 'MZ\220\000'
blocked "$d" "commit -a of a tracked file rewritten as a PE binary" -a -m "chore: change the payload"
blocked "$d" "commit PATH of a tracked file rewritten as a PE binary" -m "chore: change the payload" docs/payload.bin

# ---- result

if [ "$failed" -ne 0 ]; then
  echo "$failed of $((passed + failed)) checks failed" >&2
  exit 1
fi
echo "all $passed checks passed"
