#!/usr/bin/env bash
# Writes the third-party notices that each of the launcher's bundles carries (#341).
#
#   scripts/launcher-notices.sh <target> <out-file>
#
# The target is the bundle's Rust target triple: x86_64-unknown-linux-gnu for the AppImage,
# x86_64-pc-windows-msvc for the Windows installer, aarch64-apple-darwin for the macOS app. The
# notices hold, in this order:
#   - the licence text of every crate the Tauri shell's build for the target uses, by cargo about,
#     over apps/launcher/src-tauri (a workspace of its own) with deploy/launcher's about.toml and
#     template;
#   - the licence files of each npm package the view's production build carries, as
#     `pnpm licenses list --prod` finds them in apps/launcher/node_modules (not an SPDX document);
#   - the OFL text of each font in apps/launcher/src/fonts, OFL-<Font>.txt, named for its font, with
#     the font files it covers (those whose names start with <Font>). A font file that none covers
#     stops it.
# scripts/check-notices.sh then refuses notices with a placeholder where a copyright line belongs.
# The notices are made beside the out-file, in a folder it makes if there's none, and renamed into
# place 0644 when whole: on any failure, the out-file is left as it was, and the exit isn't 0.
#
# Needs cargo with cargo-about (pinned in scripts/versions.env), and pnpm and node, after
# `pnpm install` in apps/launcher. Compatible with macOS's bash 3.2.
set -euo pipefail
export LC_ALL=C
# A cd to a relative path goes where it says, not to a folder an exported CDPATH names.
unset CDPATH

usage() {
  echo "usage: $0 <target> <out-file>" >&2
  exit 2
}

# fail MESSAGE [STATUS]: says MESSAGE, and exits with STATUS, 1 by default.
fail() {
  echo "launcher-notices.sh: $1" >&2
  exit "${2:-1}"
}

{ [ $# -eq 2 ] && [ -n "$2" ]; } || usage
target=$1 out=$2
triple='^[a-z0-9_]+(-[a-z0-9_.]+)+$'
[[ $target =~ $triple ]] || fail "not a target triple: '$target'" 2

root=$(cd -- "$(dirname -- "$0")/.." && pwd -P)
launcher=$root/apps/launcher
fonts=$launcher/src/fonts
# The template's rule between sections.
rule='=============================================================================='

# Made beside the out-file, so that the last move is a rename. Its folder's path is absolute, as
# cargo about runs from the repository.
dir=$(dirname -- "$out")
mkdir -p -- "$dir"
dir=$(cd -- "$dir" && pwd -P)
out=$dir/${out##*/}
notices=$(mktemp "$dir/.THIRD-PARTY-NOTICES.XXXXXX")
trap 'rm -f -- "$notices"' EXIT

# append FILE: FILE's text, ending with a line end.
append() {
  cat -- "$1"
  if [ -n "$(tail -c 1 "$1")" ]; then echo; fi
}

# The shell's crates, for the target, from its own lock.
(cd -- "$root" && cargo about generate --locked --fail -m apps/launcher/src-tauri/Cargo.toml \
  --target "$target" -c deploy/launcher/about.toml -o "$notices" deploy/launcher/THIRD-PARTY-NOTICES.hbs) ||
  fail "cargo about failed (scripts/bootstrap.sh installs cargo-about), so the third-party notices can't be made"

# The view's production packages: name, version, licence and folder, a line each, tab-separated.
report=$(cd -- "$launcher" && pnpm licenses list --prod --json) ||
  fail "pnpm licenses list failed in apps/launcher (pnpm install first), so the packages' notices can't be made"
# Windows paths get forward slashes, which Git Bash reads too.
packages=$(node -e '
  let text = "";
  process.stdin.setEncoding("utf8");
  process.stdin.on("data", (chunk) => { text += chunk; });
  process.stdin.on("end", () => {
    const rows = [];
    for (const [licence, packages] of Object.entries(JSON.parse(text))) {
      for (const p of packages) {
        p.versions.forEach((version, i) => {
          rows.push([p.name, version, licence, (p.paths[i] || "").replace(/\\/g, "/")].join("\t"));
        });
      }
    }
    rows.sort();
    process.stdout.write(rows.map((row) => row + "\n").join(""));
  });
' <<< "$report") || fail "pnpm's licence report couldn't be read, so the packages' notices can't be made"

tab=$(printf '\t')
while IFS=$tab read -r name version licence folder; do
  [ -n "$name" ] || continue
  [ -d "$folder" ] || fail "the npm package $name $version isn't installed at '$folder', so its notice can't be given"
  found=0
  for f in "$folder"/*; do
    [ -f "$f" ] || continue
    # Its licence texts, not an SPDX document about them (LICENSE.spdx).
    case $(printf '%s' "${f##*/}" | tr '[:upper:]' '[:lower:]') in
      *.spdx) continue ;;
      licen[cs]e* | copying* | notice*) ;;
      *) continue ;;
    esac
    found=1
    {
      printf '\n%s\n%s, for the npm package:\n  %s %s (%s)\n\n' "$rule" "${f##*/}" "$name" "$version" "$licence"
      append "$f"
    } >> "$notices"
  done
  [ "$found" = 1 ] || fail "the npm package $name $version has no licence file in '$folder', so its notice can't be given"
done <<< "$packages"

# The fonts' OFL texts, each with the font files it covers.
covered=
for ofl in "$fonts"/OFL-*.txt; do
  [ -f "$ofl" ] || continue
  stem=${ofl##*/OFL-}
  stem=${stem%.txt}
  # Its family's name, from the file's: NotoSerifSC is Noto Serif SC.
  family=$(printf '%s' "$stem" | sed -E 's/([a-z])([A-Z])/\1 \2/g')
  files=
  for font in "$fonts/$stem"*.woff2 "$fonts/$stem"*.woff "$fonts/$stem"*.ttf "$fonts/$stem"*.otf; do
    [ -f "$font" ] || continue
    files="$files  ${font##*/}
"
    covered="$covered/${font##*/}/"
  done
  [ -n "$files" ] || fail "${ofl##*/} covers no font in apps/launcher/src/fonts"
  {
    printf '\n%s\nSIL Open Font License 1.1 (OFL-1.1), for the font %s:\n%s\n' "$rule" "$family" "$files"
    append "$ofl"
  } >> "$notices"
done
for font in "$fonts"/*.woff2 "$fonts"/*.woff "$fonts"/*.ttf "$fonts"/*.otf; do
  [ -f "$font" ] || continue
  case $covered in
    */"${font##*/}"/*) ;;
    *) fail "${font##*/} in apps/launcher/src/fonts has no OFL-<Font>.txt beside it, so its notice can't be given" ;;
  esac
done

# No placeholder where a copyright line belongs (#328): scripts/check-notices.sh shows each section
# that has one.
"$root/scripts/check-notices.sh" "$notices" ||
  fail "the third-party notices have a placeholder where a copyright line belongs, above: a crate listed with it needs a clarification in deploy/launcher/about.toml, and a package or a font a licence file with its copyright line"

chmod 0644 "$notices"
mv -f -- "$notices" "$out"
