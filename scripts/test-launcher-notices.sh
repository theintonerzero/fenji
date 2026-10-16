#!/usr/bin/env bash
# Tests for scripts/launcher-notices.sh (#341): the launcher's third-party notices, from stand-ins for
# cargo and pnpm, a fake npm package, and the launcher's real fonts, so there's no build and no
# network. Needs node, as the script does. Runs on Linux and macOS (bash 3.2).
#   scripts/test-launcher-notices.sh
set -uo pipefail
export LC_ALL=C
unset CDPATH
HERE=$(cd -- "$(dirname -- "$0")" && pwd -P)
REPO=$(cd -- "$HERE/.." && pwd -P)
NOTICES=$HERE/launcher-notices.sh
FONTS=$REPO/apps/launcher/src/fonts
fails=0
ok() { echo "ok   $1"; }
bad() { echo "FAIL $1"; fails=$((fails + 1)); }

command -v node > /dev/null || { echo "FATAL: the test needs node" >&2; exit 2; }

T=$(mktemp -d) || { echo "FATAL: mktemp failed" >&2; exit 2; }
T=$(cd -- "$T" && pwd -P) || exit 2
trap 'rm -rf -- "$T"' EXIT
case "$T" in /?*) ;; *) echo "FATAL: no temporary directory" >&2; exit 2 ;; esac

# Stand-ins for cargo and pnpm, first on PATH. Each writes the directory it ran in and its
# arguments, one a line, to $FAKE_ARGS.<name>, with the file after -o as OUT.
# - `cargo about` writes $FAKE_CRATES to the file after -o, or fails as cargo-about does when
#   FAKE_ABOUT=fail.
# - `pnpm licenses list` prints $FAKE_PNPM, or fails as pnpm does when FAKE_PNPM_FAIL=yes.
mkdir "$T/bin"
cat > "$T/bin/cargo" <<'EOF'
#!/bin/sh
out=
prev=
for a in "$@"; do
  if [ "$prev" = -o ]; then out=$a; fi
  prev=$a
done
prev=
{
  pwd -P
  for a in "$@"; do
    if [ "$prev" = -o ]; then echo OUT; else printf '%s\n' "$a"; fi
    prev=$a
  done
} > "$FAKE_ARGS.cargo"
[ "$1" = about ] || { echo "stand-in cargo: no subcommand $1" >&2; exit 101; }
if [ "${FAKE_ABOUT:-}" = fail ]; then
  echo "error: failed to satisfy license requirements" >&2
  exit 1
fi
cat "$FAKE_CRATES" > "$out"
EOF
cat > "$T/bin/pnpm" <<'EOF'
#!/bin/sh
{ pwd -P; printf '%s\n' "$@"; } > "$FAKE_ARGS.pnpm"
if [ "${FAKE_PNPM_FAIL:-}" = yes ]; then
  echo " ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND  No package.json was found" >&2
  exit 1
fi
cat "$FAKE_PNPM"
EOF
chmod 0755 "$T/bin/cargo" "$T/bin/pnpm"
PATH="$T/bin:$PATH"

# The crates' notices, laid out as the launcher's template lays them out.
cat > "$T/crates" <<'EOF'
Third-party notices for Fenji's launcher, from a stand-in for cargo about

==============================================================================
MIT License (MIT), for:
  tauri 2.12.4

Copyright (c) 2017 - Present Tauri Apps Contributors <someone@example.com>
EOF

# A fake npm package: its MIT text, an SPDX document that isn't a licence text, and code. Another
# with a placeholder in its licence, and one with no licence file at all.
pkg=$T/node_modules/.pnpm/@fake+licensed@1.2.3/node_modules/@fake/licensed
mkdir -p "$pkg" "$T/node_modules/placeheld" "$T/node_modules/unlicensed"
printf 'MIT License\n\nCopyright (c) 2026 Fake Authors\n\nPermission is hereby granted.\n' > "$pkg/LICENSE-MIT"
printf 'SPDXVersion: SPDX-2.1\nPackageName: licensed\n' > "$pkg/LICENSE.spdx"
printf 'export {};\n' > "$pkg/index.js"
printf 'MIT License\n\nCopyright (c) <year> <copyright holders>\n' > "$T/node_modules/placeheld/LICENSE"
printf 'export {};\n' > "$T/node_modules/unlicensed/index.js"
# pnpm_json NAME VERSION LICENCE FOLDER: `pnpm licenses list --json` for one package.
pnpm_json() {
  printf '{\n  "%s": [\n    {\n      "name": "%s",\n      "versions": ["%s"],\n      "paths": ["%s"],\n      "license": "%s"\n    }\n  ]\n}\n' \
    "$3" "$1" "$2" "$4" "$3"
}
pnpm_json @fake/licensed 1.2.3 MIT "$pkg" > "$T/pnpm.json"
export PATH FAKE_ARGS="$T/args" FAKE_CRATES="$T/crates" FAKE_PNPM="$T/pnpm.json"
unset FAKE_ABOUT FAKE_PNPM_FAIL

# same WHAT GOT WANT: GOT is exactly WANT.
same() {
  if [ "$2" = "$3" ]; then ok "$1"; else bad "$1"; printf '  want:\n%s\n  got:\n%s\n' "$3" "$2"; fi
}

# holds WHAT COMMAND...: COMMAND succeeds.
holds() {
  local what=$1
  shift
  if "$@"; then ok "$what"; else bad "$what"; fi
}

# body HEADING NOTICES: the text of the section whose heading's first line is HEADING, without the
# empty lines at its end.
body() {
  awk -v h="$1" '
    /^=+$/ { if (in_body) exit; at_head = 1; next }
    at_head { at_head = 0; if ($0 == h) in_head = 1; next }
    in_head && /^$/ { in_head = 0; in_body = 1; next }
    in_head { next }
    in_body { line[n++] = $0 }
    END { while (n > 0 && line[n - 1] == "") n--; for (i = 0; i < n; i++) print line[i] }
  ' "$2"
}

# heading HEADING NOTICES: the rest of the heading whose first line is HEADING, one line each.
heading() {
  awk -v h="$1" '
    /^=+$/ { at_head = 1; next }
    at_head { at_head = 0; if ($0 == h) in_head = 1; next }
    in_head && /^$/ { exit }
    in_head { print }
  ' "$2"
}

# trimmed FILE: FILE without the empty lines at its end.
trimmed() { awk '{ line[n++] = $0 } END { while (n > 0 && line[n - 1] == "") n--; for (i = 0; i < n; i++) print line[i] }' "$1"; }

# first HEADING NOTICES: the line number of the heading whose first line is HEADING, or nothing.
first() { awk -v h="$1" 'prev ~ /^=+$/ && $0 == h { print NR; exit } { prev = $0 }' "$2"; }

# run TARGET OUT...: launcher-notices.sh, run from $T with relative paths, as a workflow runs it.
# Sets rc, out (its standard output) and err (its standard error).
run() {
  out=$(cd "$T" && "$NOTICES" "$@" 2> "$T/err")
  rc=$?
  err=$(cat "$T/err")
}

# The notices: the crates', then each package's licence files, then each font's OFL text.
N=$T/out/THIRD-PARTY-NOTICES.txt
run x86_64-unknown-linux-gnu out/THIRD-PARTY-NOTICES.txt
if [ "$rc" = 0 ]; then ok "the notices are made"; else bad "the notices are made: exit $rc: $err"; fi
same "it prints nothing" "$out" ""
holds "they're written to the out-file, in a folder it makes" test -f "$N"
holds "they're 0644, readable by all" test -n "$(find "$N" -perm 0644)"
same "nothing else is left in its folder" "$(find "$T/out" -mindepth 1)" "$N"
same "cargo about is asked, in the repository, for the shell's crates for the target, locked, failing closed" \
  "$(cat "$T/args.cargo")" "$(printf '%s\n' "$REPO" about generate --locked --fail \
  -m apps/launcher/src-tauri/Cargo.toml --target x86_64-unknown-linux-gnu -c deploy/launcher/about.toml -o OUT \
  deploy/launcher/THIRD-PARTY-NOTICES.hbs)"
same "pnpm is asked, in apps/launcher, for the production packages' licences" "$(cat "$T/args.pnpm")" \
  "$(printf '%s\n' "$REPO/apps/launcher" licenses list --prod --json)"
holds "the notices start with cargo about's" \
  cmp -s "$T/crates" <(head -n "$(wc -l < "$T/crates" | tr -d ' ')" "$N")
mit="LICENSE-MIT, for the npm package:"
same "a package's licence file has a section naming the package, its version and its licence" \
  "$(heading "$mit" "$N")" "  @fake/licensed 1.2.3 (MIT)"
holds "the section holds the file's text" cmp -s <(trimmed "$pkg/LICENSE-MIT") <(body "$mit" "$N")
same "an SPDX document isn't a licence text" "$(grep -c -F 'SPDXVersion' "$N")" 0
same "nor is the package's code" "$(grep -c -F 'export {};' "$N")" 0
# fontcase FAMILY OFL FILE...: the section of the font FAMILY lists the FILEs it covers, and holds
# its OFL text.
fontcase() {
  local family=$1 ofl=$2 h
  shift 2
  h="SIL Open Font License 1.1 (OFL-1.1), for the font $family:"
  same "$family's section lists the font files $ofl covers" "$(heading "$h" "$N")" "$(printf '  %s\n' "$@")"
  holds "$family's section holds $ofl" cmp -s <(trimmed "$FONTS/$ofl") <(body "$h" "$N")
}
fontcase Cinzel OFL-Cinzel.txt Cinzel.woff2
fontcase Geist OFL-Geist.txt Geist.woff2 GeistMono.woff2
fontcase "Noto Serif SC" OFL-NotoSerifSC.txt NotoSerifSC-fenji.woff2
same "each of the fonts' OFL texts has a section" "$(grep -c ', for the font ' "$N")" \
  "$(find "$FONTS" -maxdepth 1 -name 'OFL-*.txt' | wc -l | tr -d ' ')"
crates_end=$(wc -l < "$T/crates" | tr -d ' ')
pkg_at=$(first "$mit" "$N")
font_at=$(first "SIL Open Font License 1.1 (OFL-1.1), for the font Cinzel:" "$N")
if [ -n "$pkg_at" ] && [ -n "$font_at" ] && [ "$pkg_at" -gt "$crates_end" ] && [ "$font_at" -gt "$pkg_at" ]; then
  ok "the crates come first, then the packages, then the fonts"
else
  bad "the crates come first, then the packages, then the fonts: crates end at $crates_end, package at '$pkg_at', font at '$font_at'"
fi
holds "the notices end with a line end" test "$(tail -c 1 "$N" | od -An -tx1 | tr -d ' \n')" = 0a

# Each bundle's own target.
run aarch64-apple-darwin out/THIRD-PARTY-NOTICES.txt
same "cargo about is asked for another target's crates" "$(sed -n '/^--target$/{n;p;}' "$T/args.cargo")" aarch64-apple-darwin

# refused WANT_RC PATTERN WHAT -- ARGS...: launcher-notices.sh ARGS, run from $T, exits WANT_RC with
# PATTERN in its message, and leaves the out-file as it was: $T/out/THIRD-PARTY-NOTICES.txt holds
# "old", and nothing else is in its folder.
refused() {
  local want=$1 pattern=$2 what=$3 got rc
  shift 4
  rm -rf "$T/out"
  mkdir "$T/out"
  printf 'old\n' > "$T/out/THIRD-PARTY-NOTICES.txt"
  got=$(cd "$T" && "$NOTICES" "$@" 2>&1 > /dev/null)
  rc=$?
  if [ "$rc" = "$want" ] && grep -q -- "$pattern" <<< "$got" &&
    [ "$(cat "$T/out/THIRD-PARTY-NOTICES.txt")" = old ] && [ "$(find "$T/out" -mindepth 1)" = "$N" ]; then
    ok "$what"
  else
    bad "$what: exit $rc: $got: $(find "$T/out" -mindepth 1 | tr '\n' ' ')"
  fi
}

refused 2 usage "no arguments are a usage error" --
refused 2 usage "one argument is a usage error" -- x86_64-unknown-linux-gnu
refused 2 usage "three arguments are a usage error" -- x86_64-unknown-linux-gnu out/THIRD-PARTY-NOTICES.txt more
refused 2 usage "an empty out-file is a usage error" -- x86_64-unknown-linux-gnu ""
refused 2 "not a target" "a target with a slash is refused" -- ../x86_64-unknown-linux-gnu out/THIRD-PARTY-NOTICES.txt
refused 2 "not a target" "an empty target is refused" -- "" out/THIRD-PARTY-NOTICES.txt

# A placeholder where a copyright line belongs, in a crate's text or a package's, is refused (#328),
# with the section that has it.
printf '%s\n' "" ============================================================================== \
  "MIT License (MIT), for:" "  objc2 0.6.5" "" "Copyright (c) <year> <copyright holders>" > "$T/placeheld-crate"
cat "$T/crates" "$T/placeheld-crate" > "$T/crates-placeheld"
FAKE_CRATES=$T/crates-placeheld refused 1 "objc2 0.6.5" \
  "a crate's placeholder is refused, naming the crate, and the out-file stays as it was" -- \
  x86_64-unknown-linux-gnu out/THIRD-PARTY-NOTICES.txt
FAKE_CRATES=$T/crates-placeheld refused 1 "deploy/launcher/about.toml" "the refusal says where crates are clarified" -- \
  x86_64-unknown-linux-gnu out/THIRD-PARTY-NOTICES.txt
pnpm_json placeheld 0.1.0 MIT "$T/node_modules/placeheld" > "$T/pnpm-placeheld.json"
FAKE_PNPM=$T/pnpm-placeheld.json refused 1 "placeheld 0.1.0" "a package's placeholder is refused, naming the package" -- \
  x86_64-unknown-linux-gnu out/THIRD-PARTY-NOTICES.txt

# Nothing is made without every part of the notices.
FAKE_ABOUT=fail refused 1 "cargo about failed" "a failing cargo about stops it" -- \
  x86_64-unknown-linux-gnu out/THIRD-PARTY-NOTICES.txt
FAKE_PNPM_FAIL=yes refused 1 "pnpm" "a failing pnpm stops it" -- x86_64-unknown-linux-gnu out/THIRD-PARTY-NOTICES.txt
printf 'not json\n' > "$T/pnpm-bad.json"
FAKE_PNPM=$T/pnpm-bad.json refused 1 "pnpm" "pnpm's report unread stops it" -- x86_64-unknown-linux-gnu out/THIRD-PARTY-NOTICES.txt
pnpm_json unlicensed 0.2.0 MIT "$T/node_modules/unlicensed" > "$T/pnpm-unlicensed.json"
FAKE_PNPM=$T/pnpm-unlicensed.json refused 1 "unlicensed 0.2.0" "a package with no licence file stops it" -- \
  x86_64-unknown-linux-gnu out/THIRD-PARTY-NOTICES.txt
pnpm_json gone 0.3.0 MIT "$T/node_modules/gone" > "$T/pnpm-gone.json"
FAKE_PNPM=$T/pnpm-gone.json refused 1 "gone 0.3.0" "a package that isn't installed stops it" -- \
  x86_64-unknown-linux-gnu out/THIRD-PARTY-NOTICES.txt

# With no production packages, the notices are the crates' and the fonts'.
printf '{}\n' > "$T/pnpm-none.json"
rm -rf "$T/out"
FAKE_PNPM=$T/pnpm-none.json run x86_64-unknown-linux-gnu out/THIRD-PARTY-NOTICES.txt
if [ "$rc" = 0 ] && ! grep -q "for the npm package:" "$N" && grep -q "for the font Geist:" "$N"; then
  ok "with no production packages, it makes the crates' and fonts' notices"
else
  bad "with no production packages, it makes the crates' and fonts' notices: exit $rc: $err"
fi

# A font without its OFL text stops it: in a copy of the repository's scripts, with a fonts folder of
# its own.
F=$T/repo
mkdir -p "$F/scripts" "$F/apps/launcher/src/fonts"
cp "$NOTICES" "$HERE/check-notices.sh" "$F/scripts/"
cp "$FONTS/OFL-Cinzel.txt" "$FONTS/Cinzel.woff2" "$F/apps/launcher/src/fonts/"
: > "$F/apps/launcher/src/fonts/Orphan.woff2"
rm -rf "$T/out"
got=$(cd "$T" && "$F/scripts/launcher-notices.sh" x86_64-unknown-linux-gnu out/THIRD-PARTY-NOTICES.txt 2>&1 > /dev/null)
rc=$?
if [ "$rc" = 1 ] && grep -q "Orphan.woff2" <<< "$got" && [ ! -e "$N" ]; then
  ok "a font without its OFL text stops it"
else
  bad "a font without its OFL text stops it: exit $rc: $got"
fi

# An exported CDPATH doesn't steer its cd: a decoy apps/launcher there doesn't catch it.
mkdir -p "$T/decoy/apps/launcher"
rm -rf "$T/out"
out=$(cd "$T" && CDPATH=$T/decoy "$NOTICES" x86_64-unknown-linux-gnu out/THIRD-PARTY-NOTICES.txt 2> "$T/err")
rc=$?
same "with CDPATH set, pnpm still runs in the repository's apps/launcher" "$rc $(head -n 1 "$T/args.pnpm")" \
  "0 $REPO/apps/launcher"

# toml_list FILE TABLE KEY: the strings in FILE's KEY = [ ... ] array, under [TABLE] (or before any
# table when TABLE is empty), one a line, in order.
toml_list() {
  awk -v table="$2" -v key="$3" '
    /^\[/ { t = $0; gsub(/^\[|\]$/, "", t); next }
    !on && t == table && $0 ~ "^" key " *= *\\[" { on = 1 }
    on {
      s = $0
      sub(/#.*/, "", s)
      while (match(s, /"[^"]*"/)) { print substr(s, RSTART + 1, RLENGTH - 2); s = substr(s, RSTART + RLENGTH) }
      if (s ~ /\]/) on = 0
    }
  ' "$1"
}

# cargo-about accepts exactly the licences the shell's cargo-deny allows, in the same order, so the
# launcher's notices and its dependency check never disagree about what may go into a bundle.
allowed=$(toml_list "$REPO/apps/launcher/src-tauri/deny.toml" licenses allow)
holds "the shell's deny.toml [licenses] allow list is read" test -n "$allowed"
same "deploy/launcher/about.toml accepts exactly the shell's deny.toml licences, in its order" \
  "$(toml_list "$REPO/deploy/launcher/about.toml" "" accepted)" "$allowed"

[ "$fails" = 0 ] || { echo "$fails failed"; exit 1; }
echo "all passed"
