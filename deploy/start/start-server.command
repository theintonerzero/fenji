#!/bin/sh
# Starts the Fenji server in this folder, which holds its settings (fenji.toml) and its data
# (data/): the first start writes both. Finder opens this file in Terminal, which keeps its window
# open when the server stops with an error.
# A cd to a relative path goes where it says, not to a folder an exported CDPATH names.
unset CDPATH
cd "$(dirname "$0")" || exit 1
# The server needs a file for each connection, and macOS lets a program have only 256 open files by
# default: raise the soft limit to 4,096, or as far as the hard limit allows, and never lower either
# limit, quietly (Ruling HP11). ulimit's -n, -S and -H aren't POSIX, but every sh that runs this
# has them.
# shellcheck disable=SC3045
{
  n=$(ulimit -Sn 2>/dev/null)
  [ "$n" = unlimited ] || [ "$n" -ge 4096 ] 2>/dev/null ||
    ulimit -Sn 4096 2>/dev/null || ulimit -Sn "$(ulimit -Hn 2>/dev/null)" 2>/dev/null || true
}
exec ./fenji-server "$@"
