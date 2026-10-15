#!/usr/bin/env bash
# Fails when third-party notices hold a placeholder where a copyright line belongs (#328): the
# release archives' (scripts/package-server.sh) and the launcher's (scripts/launcher-notices.sh).
#
#   scripts/check-notices.sh <notices-file>
#
# For a crate whose own licence text it can't find or read, cargo about puts in the licence's
# standard text, and says so only in its debug log: --fail lets it through. MIT's and BSD's
# standard texts leave the copyright line a template, "Copyright (c) <year> <copyright holders>",
# so the notices would ship a placeholder where the crate's copyright notice belongs. These are the
# fields of the copyright lines in the standard texts of the licences about.toml accepts, in any
# case and number, as other licences write them (MIT-0's "<YEAR> <COPYRIGHT HOLDER>"). Nothing
# broader: a crate's own text may have other things in angle brackets, such as an email address.
# When the notices hold one, each section that does is shown on stderr, with the line, and the
# script exits 1. Compatible with macOS's bash 3.2.
set -euo pipefail
export LC_ALL=C

[ $# -eq 1 ] || { echo "usage: $0 <notices-file>" >&2; exit 2; }
[ -f "$1" ] || { echo "check-notices.sh: no notices at $1" >&2; exit 2; }
fields='<(year|owner|copyright holders?)>'
if grep -q -i -E "$fields" "$1"; then
  awk -v fields="$fields" '
    /^=+$/ { heading = ""; in_heading = 1; shown = 0; next }
    in_heading && /^$/ { in_heading = 0; next }
    in_heading { heading = heading $0 "\n"; next }
    tolower($0) ~ fields {
      if (!shown) { printf "\n%s", heading; shown = 1 }
      printf "  line %d: %s\n", NR, $0
    }
  ' "$1" >&2
  exit 1
fi
