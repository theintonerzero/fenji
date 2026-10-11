#!/usr/bin/env bash
# Starts the REA MCP server for Claude Code (.mcp.json) with the Fenji environment from
# scripts/bootstrap.sh, so it works however Claude Code was launched (old shell, IDE, desktop app).
# stdout is the MCP stream: nothing else may write to it.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck source=versions.env
. "$ROOT/scripts/versions.env"
ENV_FILE="$HOME/.config/fenji/env.sh"
# shellcheck disable=SC1090
[ -f "$ENV_FILE" ] && . "$ENV_FILE"
[ -n "${GHIDRA_INSTALL_DIR:-}" ] || { echo "rea-mcp: GHIDRA_INSTALL_DIR unset; run scripts/bootstrap.sh" >&2; exit 1; }

exec npx -y "rea-agents@$REA_AGENTS_VERSION" mcp
