#!/usr/bin/env bash
# Tests scripts/check-release-files.sh and scripts/release-notes.sh (the hosting spec's Amendment D,
# HD2), with made-up artifacts: the check takes a release's files only when each artifact holds
# exactly what its own job reported, and refuses a replaced, added, missing, linked or misreported
# file, leaving nothing; the notes are the changelog's section for the release.
#   scripts/test-release-files.sh
set -uo pipefail
export LC_ALL=C
REPO=$(cd "$(dirname "$0")/.." && pwd)
T=$(mktemp -d "${TMPDIR:-/tmp}/test-release-files.XXXXXX")
trap 'rm -rf -- "$T"' EXIT
failed=0
ok() { echo "ok   $1"; }
bad() { echo "FAIL $1"; failed=1; }
sum256() {
  if command -v sha256sum > /dev/null; then sha256sum < "$1"; else shasum -a 256 < "$1"; fi | cut -d' ' -f1
}

# artifacts DIR: a fresh DIR with a run's artifacts, each in a folder of its name, and BUILD,
# LAUNCHER and SIGNED reporting them, as the jobs' outputs would.
artifacts() {
  local dir=$1 f
  rm -rf -- "$dir"
  mkdir -p "$dir/fenji-server-x86_64-unknown-linux-musl" "$dir/fenji-server-x86_64-pc-windows-msvc" \
    "$dir/fenji-launcher-appimage" "$dir/fenji-launcher-nsis" "$dir/fenji-launcher-app" \
    "$dir/launcher-signatures"
  printf 'linux\n' > "$dir/fenji-server-x86_64-unknown-linux-musl/fenji-server-1.2.3-x86_64-unknown-linux-musl.tar.gz"
  printf 'windows\n' > "$dir/fenji-server-x86_64-pc-windows-msvc/fenji-server-1.2.3-x86_64-pc-windows-msvc.zip"
  printf 'appimage\n' > "$dir/fenji-launcher-appimage/Fenji_1.2.3_amd64.AppImage"
  printf 'nsis\n' > "$dir/fenji-launcher-nsis/Fenji_1.2.3_x64-setup.exe"
  printf 'app\n' > "$dir/fenji-launcher-app/Fenji.app.tar.gz"
  for f in Fenji_1.2.3_amd64.AppImage Fenji_1.2.3_x64-setup.exe Fenji.app.tar.gz; do
    printf 'signature of %s\n' "$f" > "$dir/launcher-signatures/$f.sig"
  done
  report() { printf '"%s": "%s %s"' "$1" "$(sum256 "$2")" "${2##*/}"; }
  BUILD="{$(report x86_64-unknown-linux-musl "$dir/fenji-server-x86_64-unknown-linux-musl/fenji-server-1.2.3-x86_64-unknown-linux-musl.tar.gz"),
    $(report x86_64-pc-windows-msvc "$dir/fenji-server-x86_64-pc-windows-msvc/fenji-server-1.2.3-x86_64-pc-windows-msvc.zip")}"
  LAUNCHER="{$(report appimage "$dir/fenji-launcher-appimage/Fenji_1.2.3_amd64.AppImage"),
    $(report nsis "$dir/fenji-launcher-nsis/Fenji_1.2.3_x64-setup.exe"),
    $(report app "$dir/fenji-launcher-app/Fenji.app.tar.gz")}"
  SIGNED="{$(report appimage "$dir/launcher-signatures/Fenji_1.2.3_amd64.AppImage.sig"),
    $(report nsis "$dir/launcher-signatures/Fenji_1.2.3_x64-setup.exe.sig"),
    $(report app "$dir/launcher-signatures/Fenji.app.tar.gz.sig")}"
  export BUILD LAUNCHER SIGNED
}

# check IN OUT: the script over IN, its output in $T/out, its exit code returned.
check() { "$REPO/scripts/check-release-files.sh" 1.2.3 "$1" "$2" > "$T/out" 2>&1; }

artifacts "$T/in"
if check "$T/in" "$T/dist"; then ok "the files their jobs reported are taken"; else bad "the files their jobs reported are taken: $(cat "$T/out")"; fi
want="Fenji.app.tar.gz Fenji.app.tar.gz.sig Fenji_1.2.3_amd64.AppImage Fenji_1.2.3_amd64.AppImage.sig Fenji_1.2.3_x64-setup.exe Fenji_1.2.3_x64-setup.exe.sig fenji-server-1.2.3-x86_64-pc-windows-msvc.zip fenji-server-1.2.3-x86_64-unknown-linux-musl.tar.gz"
got=$(cd "$T/dist" && printf '%s ' * | sed 's/ $//')
if [ "$got" = "$want" ]; then ok "into one folder, all of them and nothing else"; else bad "into one folder: got $got"; fi
if cmp -s "$T/in/fenji-launcher-app/Fenji.app.tar.gz" "$T/dist/Fenji.app.tar.gz"; then ok "as they were"; else bad "as they were"; fi
rm -rf -- "$T/dist"
if env -u BUILD -u SIGNED "$REPO/scripts/check-release-files.sh" 1.2.3 "$T/launcher-only" "$T/dist" > "$T/out" 2>&1; then
  bad "a folder that isn't there is refused"
else
  ok "a folder that isn't there is refused"
fi
artifacts "$T/in"
rm -rf -- "$T/in"/fenji-server-* "$T/in/launcher-signatures"
if env -u BUILD -u SIGNED "$REPO/scripts/check-release-files.sh" 1.2.3 "$T/in" "$T/dist" > "$T/out" 2>&1 &&
  [ "$(cd "$T/dist" && printf '%s ' * | sed 's/ $//')" = "Fenji.app.tar.gz Fenji_1.2.3_amd64.AppImage Fenji_1.2.3_x64-setup.exe" ]; then
  ok "the launcher's reports alone take its bundles, as the sign job does"
else
  bad "the launcher's reports alone take its bundles: $(cat "$T/out")"
fi
rm -rf -- "$T/dist"

# refused WHAT WORDS [CODE]: after the setup the caller made in $T/in, the check exits CODE (1 by
# default), says WORDS, and leaves no $T/dist.
refused() {
  local code=0
  check "$T/in" "$T/dist" || code=$?
  if [ "$code" != "${3:-1}" ]; then
    bad "$1: exit $code: $(cat "$T/out")"
  elif ! grep -qF -- "$2" "$T/out"; then
    bad "$1: it didn't say \"$2\": $(cat "$T/out")"
  elif [ -e "$T/dist" ]; then
    bad "$1: $T/dist stayed"
  else
    ok "$1"
  fi
}

artifacts "$T/in"
printf 'trojan\n' > "$T/in/fenji-server-x86_64-unknown-linux-musl/fenji-server-1.2.3-x86_64-unknown-linux-musl.tar.gz"
refused "a replaced archive is refused" "not the"
artifacts "$T/in"
printf 'trojan\n' > "$T/in/launcher-signatures/Fenji.app.tar.gz.sig"
refused "and a replaced signature" "not the"
artifacts "$T/in"
mkdir "$T/in/fenji-server-aarch64-unknown-linux-musl"
printf 'more\n' > "$T/in/fenji-server-aarch64-unknown-linux-musl/fenji-server-1.2.3-aarch64-unknown-linux-musl.tar.gz"
refused "an artifact no job reported is refused" "unreported fenji-server-aarch64-unknown-linux-musl"
artifacts "$T/in"
printf 'more\n' > "$T/in/fenji-launcher-nsis/extra.exe"
refused "and a file no job reported, in a reported artifact" "unreported extra.exe"
artifacts "$T/in"
rm -rf -- "$T/in/fenji-launcher-app"
refused "a reported artifact that isn't there is refused" "no fenji-launcher-app"
artifacts "$T/in"
mv "$T/in/fenji-server-x86_64-unknown-linux-musl/fenji-server-1.2.3-x86_64-unknown-linux-musl.tar.gz" "$T/real.tar.gz"
ln -s "$T/real.tar.gz" "$T/in/fenji-server-x86_64-unknown-linux-musl/fenji-server-1.2.3-x86_64-unknown-linux-musl.tar.gz"
refused "a link in place of a file is refused" "isn't a regular file"
artifacts "$T/in"
printf 'odd\n' > "$T/in/fenji-launcher-app/odd name"
refused "a file whose name isn't plain is refused" "isn't a plain file name"
artifacts "$T/in"
BUILD=$(printf '%s' "$BUILD" | sed 's/fenji-server-1.2.3-x86_64-unknown-linux-musl.tar.gz/fenji-server-1.2.3-aarch64-unknown-linux-musl.tar.gz/')
refused "a target that reports another target's archive is refused" "not fenji-server-1.2.3-x86_64-unknown-linux-musl.tar.gz"
artifacts "$T/in"
LAUNCHER=$(printf '%s' "$LAUNCHER" | jq -c '.nsis += "\nfenji-server-1.2.3-x86_64-unknown-linux-musl.tar.gz"')
refused "a report with more than one line in it is refused" "not as a SHA-256 and a file name"
artifacts "$T/in"
LAUNCHER=$(printf '%s' "$LAUNCHER" | jq -c '.deb = .app')
refused "a bundle Fenji doesn't make is refused" "a bundle Fenji doesn't make"
artifacts "$T/in"
LAUNCHER=$(printf '%s' "$LAUNCHER" | jq -c '.app = (.app | sub(" .*"; " Fenji.dmg"))')
refused "a bundle of the wrong kind is refused" "isn't that kind of bundle"
artifacts "$T/in"
SIGNED=$(printf '%s' "$SIGNED" | jq -c 'del(.app)')
refused "signatures for other bundles than were made are refused" "other bundles"
artifacts "$T/in"
SIGNED=$(printf '%s' "$SIGNED" | jq -c '.app = (.app | sub(" .*"; " Fenji_1.2.3_amd64.AppImage.sig"))')
refused "a signature reported under another bundle's name is refused" "not Fenji.app.tar.gz.sig"
artifacts "$T/in"
BUILD='["not", "an object"]'
refused "reports that aren't an object are refused" "is not a JSON object"
artifacts "$T/in"
mkdir "$T/dist"
code=0
check "$T/in" "$T/dist" || code=$?
if [ "$code" = 2 ] && grep -qF "already there" "$T/out" && [ -d "$T/dist" ] && [ -z "$(ls -A "$T/dist")" ]; then
  ok "a folder to fill that's already there is refused, and left as it was"
else
  bad "a folder to fill that's already there: exit $code: $(cat "$T/out")"
fi
rm -rf -- "$T/dist"
artifacts "$T/in"
if env -u BUILD -u LAUNCHER "$REPO/scripts/check-release-files.sh" 1.2.3 "$T/in" "$T/dist" > "$T/out" 2>&1; then
  bad "signatures without the launcher's reports are refused"
elif [ -e "$T/dist" ]; then
  bad "signatures without the launcher's reports: $T/dist stayed"
else
  ok "signatures without the launcher's reports are refused"
fi

# release-notes.sh: the section for a version, its heading included, up to the next one or the footer.
cat > "$T/CHANGELOG.md" <<'EOF'
# Changelog

## [1.2.30] - 2026-10-18

### Fixed

- Not this one

## [1.2.4] - 2026-10-17

### Added

- **deploy:** A thing

## [1.2.3] - 2026-10-16

### Fixed

- **server:** Another

<!-- generated by git-cliff -->
EOF
notes() { "$REPO/scripts/release-notes.sh" "$1" "$T/CHANGELOG.md" 2> "$T/out"; }
if [ "$(notes 1.2.4)" = "$(printf '## [1.2.4] - 2026-10-17\n\n### Added\n\n- **deploy:** A thing')" ]; then
  ok "the notes are the release's section, to the next one"
else
  bad "the notes are the release's section: $(notes 1.2.4)"
fi
if [ "$(notes 1.2.3)" = "$(printf '## [1.2.3] - 2026-10-16\n\n### Fixed\n\n- **server:** Another')" ]; then
  ok "and the last section's end at git-cliff's footer"
else
  bad "and the last section's end at git-cliff's footer: $(notes 1.2.3)"
fi
if [ "$(notes 1.2.30 | head -n 1)" = "## [1.2.30] - 2026-10-18" ]; then
  ok "a version is its own, not another's that starts with it"
else
  bad "a version is its own: $(notes 1.2.30)"
fi
if notes 1.2.5 > /dev/null; then
  bad "a release with no section is refused"
elif grep -qF "git cliff --tag v1.2.5" "$T/out"; then
  ok "a release with no section is refused, saying how to write it"
else
  bad "a release with no section: $(cat "$T/out")"
fi
if notes v1.2.4 > /dev/null; then bad "a version with its v is refused"; else ok "a version with its v is refused"; fi

[ "$failed" = 0 ] && echo "all passed"
exit "$failed"
