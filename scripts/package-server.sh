#!/usr/bin/env bash
# Packs a fenji-server binary into its release archive (M4 hosting spec §3.7, H3).
#
#   scripts/package-server.sh <version> <target> <binary> <out-dir>
#
# Writes <out-dir>/fenji-server-<version>-<target>.tar.gz, or .zip for a Windows target, and prints
# its path. The version is the tag's without its v: 0.5.0, or 0.5.0-rc.1. The archive holds one
# folder of the same name, with:
#   - the binary, as fenji-server or fenji-server.exe;
#   - its system's start script from deploy/start: start-server.sh for Linux, start-server.command
#     for macOS, or start-server.bat, with CR LF line ends, for Windows;
#   - for Linux and macOS, deploy/start's update-server.sh, which updates the server it's beside (the
#     hosting spec's Amendment B): Windows hosts update by hand;
#   - fenji.example.toml, README.txt (deploy/archive's, for this version), LICENSE, and SOURCE.txt,
#     which says where the version's source code is (the AGPL's offer);
#   - THIRD-PARTY-NOTICES.txt: the licences of the crates the target's build links, by cargo about,
#     and musl's notice for a musl target, with no placeholder where a copyright line belongs;
#   - for Linux, deploy/systemd's units in systemd/ and deploy/caddy's Caddyfile in caddy/, which the
#     hosting guide's VPS setup installs, so a VPS needs no second download (#359);
#   - fenji-oodle/COPYING and fenji-oodle/NOTICE when the target's build links fenji-oodle, which is
#     GPL-3.0-or-later (Oodle Ruling O16), as `cargo tree` says.
# The folders, the program and the scripts are 0755 and the rest 0644, and a tarball's entries are
# root's.
# The archive is made in a staging folder inside the out-dir and renamed into place, so no archive
# appears there until it's whole.
#
# Needs cargo and cargo-about, tar and gzip, and zip or 7-Zip for a Windows target (GitHub's Windows
# runners have 7-Zip and no zip). Compatible with macOS's bash 3.2.
set -euo pipefail
export LC_ALL=C
# A cd to a relative path goes where it says, not to a folder an exported CDPATH names.
unset CDPATH

usage() {
  echo "usage: $0 <version> <target> <binary> <out-dir>" >&2
  exit 2
}

# fail MESSAGE [STATUS]: says MESSAGE, and exits with STATUS, 1 by default.
fail() {
  echo "package-server.sh: $1" >&2
  exit "${2:-1}"
}

{ [ $# -eq 4 ] && [ -n "$4" ]; } || usage
version=$1 target=$2 binary=$3 out=$4

# Both name files, and the version a URL.
semver='^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$'
[[ $version =~ $semver ]] || fail "the version is like 0.5.0 or 0.5.0-rc.1, without the tag's v: '$version'" 2
triple='^[a-z0-9_]+(-[a-z0-9_.]+)+$'
[[ $target =~ $triple ]] || fail "not a target triple: '$target'" 2
case $target in
  *-windows-*) exe=fenji-server.exe start=start-server.bat ext=zip update= ;;
  *-apple-darwin) exe=fenji-server start=start-server.command ext=tar.gz update=update-server.sh ;;
  *-linux-*) exe=fenji-server start=start-server.sh ext=tar.gz update=update-server.sh ;;
  *) fail "no start script for $target: there are ones for Linux, macOS and Windows" 2 ;;
esac
[ -f "$binary" ] || fail "no binary at '$binary'" 2
zipper=
if [ "$ext" = zip ]; then
  if command -v zip > /dev/null; then
    zipper=zip
  elif command -v 7z > /dev/null; then
    zipper=7z
  else
    fail "a Windows target's zip needs zip or 7z"
  fi
fi

root=$(cd -- "$(dirname -- "$0")/.." && pwd -P)

# fenji-oodle is GPL-3.0-or-later (Oodle Ruling O16), so a binary that links it carries its licence
# and notice. The build has the answer: the target's linked (normal) dependencies, locked as the
# release builds them. When cargo can't give it, nothing is packed.
deps=$(cd -- "$root" && cargo tree --locked -p fenji-server --target "$target" -e normal --prefix none) ||
  fail "cargo tree failed, so it isn't known whether fenji-server links fenji-oodle (GPL-3.0-or-later)"
gpl=no
if grep -q '^fenji-oodle v' <<< "$deps"; then gpl=yes; fi

name=fenji-server-$version-$target
# Staged on the out-dir's own file system, so that the archive's last move is a rename.
mkdir -p -- "$out"
out=$(cd -- "$out" && pwd -P)
stage=$(mktemp -d "$out/.stage.XXXXXX")
trap 'rm -rf -- "$stage"' EXIT
dir=$stage/$name
mkdir -- "$dir"

# The third-party notices: every crate the target's build links, with its licence's text, from the
# locked dependency graph. cargo-about makes them (pinned in scripts/versions.env), and
# deploy/archive/about.toml accepts exactly deny.toml's licences. When it can't, nothing is packed.
(cd -- "$root" && cargo about generate --locked --fail -m crates/fenji-server/Cargo.toml \
  --target "$target" -c deploy/archive/about.toml -o "$dir/THIRD-PARTY-NOTICES.txt" \
  deploy/archive/THIRD-PARTY-NOTICES.hbs) ||
  fail "cargo about failed (scripts/bootstrap.sh installs cargo-about), so the third-party notices can't be made"
# A musl target's build also links musl, the C library. Rust 1.97's musl targets link musl 1.2.5
# (rust-lang/rust, src/ci/docker/scripts/musl.sh), and deploy/archive/musl-COPYRIGHT is its
# COPYRIGHT, verbatim: replace both when the toolchain's musl changes.
case $target in
  *-linux-musl*)
    {
      printf '\n%s\n' ==============================================================================
      printf '%s\n\n' "musl 1.2.5, the C library the static Linux builds link in:"
      cat -- "$root/deploy/archive/musl-COPYRIGHT"
    } >> "$dir/THIRD-PARTY-NOTICES.txt" ;;
esac
# No placeholder where a copyright line belongs (#328): scripts/check-notices.sh shows each section
# that has one, and nothing is packed.
if ! "$root/scripts/check-notices.sh" "$dir/THIRD-PARTY-NOTICES.txt"; then
  fail "the third-party notices have a placeholder where a copyright line belongs, above, so give the crates listed with it a clarification in deploy/archive/about.toml"
fi

cp -- "$binary" "$dir/$exe"
# A start script gets its system's line ends, whatever the checkout's: CR LF for cmd.exe, LF for sh.
case $start in
  *.bat) awk '{ sub(/\r$/, ""); printf "%s\r\n", $0 }' "$root/deploy/start/$start" > "$dir/$start" ;;
  *) tr -d '\r' < "$root/deploy/start/$start" > "$dir/$start" ;;
esac
if [ -n "$update" ]; then tr -d '\r' < "$root/deploy/start/$update" > "$dir/$update"; fi
cp -- "$root/crates/fenji-server/fenji.example.toml" "$root/LICENSE" "$dir/"
sed "s/@VERSION@/$version/g" "$root/deploy/archive/README.txt" > "$dir/README.txt"
cat > "$dir/SOURCE.txt" <<EOF
Fenji $version is free software: you can redistribute it and/or modify it under
the terms of the GNU Affero General Public License, version 3 or (at your
option) any later version (LICENSE). This version's source code is here:

https://github.com/theintonerzero/fenji/tree/v$version

If you change Fenji and host it for other people, the AGPL requires you to
offer them your version's source code: [server] source_url in fenji.toml
tells their launchers where it is.
EOF
chmod 0755 "$dir" "$dir/$exe" "$dir/$start" ${update:+"$dir/$update"}
chmod 0644 "$dir/fenji.example.toml" "$dir/README.txt" "$dir/LICENSE" "$dir/SOURCE.txt" "$dir/THIRD-PARTY-NOTICES.txt"
files=("$exe" "$start")
if [ -n "$update" ]; then files+=("$update"); fi
files+=(fenji.example.toml README.txt LICENSE SOURCE.txt THIRD-PARTY-NOTICES.txt)
case $target in
  *-linux-*)
    mkdir -- "$dir/systemd" "$dir/caddy"
    chmod 0755 "$dir/systemd" "$dir/caddy"
    files+=(systemd)
    for unit in fenji.service fenji-backup.service fenji-backup.timer; do
      cp -- "$root/deploy/systemd/$unit" "$dir/systemd/"
      chmod 0644 "$dir/systemd/$unit"
      files+=("systemd/$unit")
    done
    cp -- "$root/deploy/caddy/Caddyfile" "$dir/caddy/"
    chmod 0644 "$dir/caddy/Caddyfile"
    files+=(caddy caddy/Caddyfile) ;;
esac
if [ "$gpl" = yes ]; then
  mkdir -- "$dir/fenji-oodle"
  cp -- "$root/crates/fenji-oodle/COPYING" "$root/crates/fenji-oodle/NOTICE" "$dir/fenji-oodle/"
  chmod 0755 "$dir/fenji-oodle"
  chmod 0644 "$dir/fenji-oodle/COPYING" "$dir/fenji-oodle/NOTICE"
  cat >> "$dir/README.txt" <<'EOF'

This build includes fenji-oodle, a port of ooz's Oodle decoder, which is under
the GNU General Public License, version 3 or later: fenji-oodle/NOTICE says
more, and fenji-oodle/COPYING is that licence.
EOF
  files+=(fenji-oodle fenji-oodle/COPYING fenji-oodle/NOTICE)
fi

# The folder, then its files in the order above.
members=("$name")
for f in "${files[@]}"; do members+=("$name/$f"); done

case $ext in
  tar.gz)
    # Every entry is root's, so that root unpacking the tarball (on a VPS, say) doesn't give its files
    # to whoever has the build machine's user id there. GNU tar and bsdtar (macOS) say it differently;
    # bsdtar also leaves out extended attributes (macOS's quarantine and provenance) and, with
    # COPYFILE_DISABLE, the ._ files that would carry them.
    case $(tar --version 2> /dev/null) in
      *"GNU tar"*) owner=(--owner=0 --group=0 --numeric-owner) ;;
      *) owner=(--uid 0 --gid 0 --numeric-owner --no-xattrs --no-acls) ;;
    esac
    (cd -- "$stage" && COPYFILE_DISABLE=1 tar "${owner[@]}" --no-recursion -czf "$name.tar.gz" "${members[@]}") ;;
  zip)
    if [ "$zipper" = zip ]; then
      (cd -- "$stage" && zip -q -X "$name.zip" "${members[@]}")
    else
      # 7-Zip adds the folder with everything in it, in its own order.
      (cd -- "$stage" && 7z a -tzip -bso0 -bsp0 "$name.zip" "$name")
    fi ;;
esac

mv -f -- "$stage/$name.$ext" "$out/"
printf '%s\n' "$out/$name.$ext"
