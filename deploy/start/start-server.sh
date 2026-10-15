#!/bin/sh
# Starts the Fenji server in this folder, which holds its settings (fenji.toml) and its data
# (data/): the first start writes both. Arguments go to the server: ./start-server.sh --help
cd "$(dirname "$0")" || exit 1
# The server needs a file for each connection, and some systems let a program have only 256 open
# files: raise the limit to 4,096, or as far as the system allows, quietly (Ruling HP11). ulimit's
# -n and -H aren't POSIX, but every sh that runs this has them: dash, bash, zsh and busybox.
# shellcheck disable=SC3045
ulimit -n 4096 2>/dev/null || ulimit -n "$(ulimit -Hn 2>/dev/null)" 2>/dev/null || true
exec ./fenji-server "$@"
