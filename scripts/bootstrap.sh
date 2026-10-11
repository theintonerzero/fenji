#!/usr/bin/env bash
# Fenji developer environment bootstrap.
# Supported: macOS (arm64/x86_64, Homebrew) and Fedora Linux (x86_64/arm64, dnf).
# Other Linux distros: prints the system packages to install, then does everything else.
#
#   scripts/bootstrap.sh            install/upgrade everything pinned in scripts/versions.env
#   scripts/bootstrap.sh --check    report installed vs pinned versions only; exit 1 on mismatch
#   scripts/bootstrap.sh --yes      don't prompt (assume yes to every optional step)
#
# Safe to re-run: every step checks what is already installed first.
# Never asks for, reads or stores Steam credentials; you type them into DepotDownloader yourself.
# Compatible with macOS's bash 3.2.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck source=versions.env
. "$ROOT/scripts/versions.env"

MODE=install
ASSUME_YES=0
for arg in "$@"; do
  case "$arg" in
    --check) MODE=check ;;
    --yes|-y) ASSUME_YES=1 ;;
    -h|--help) sed -n '2,12p' "$0"; exit 0 ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done

TOOLS_DIR="$HOME/.local/share/fenji/tools"
BIN_DIR="$HOME/.local/bin"
ENV_FILE="$HOME/.config/fenji/env.sh"
GHIDRA_DIR="$TOOLS_DIR/ghidra_${GHIDRA_VERSION}_PUBLIC"
DD_DIR="$TOOLS_DIR/DepotDownloader-$DEPOTDOWNLOADER_VERSION"
UMU_DIR="$TOOLS_DIR/umu-$UMU_LAUNCHER_VERSION"
JDK_DIR="$TOOLS_DIR/jdk-$TEMURIN_VERSION"
UNLUAC_JAR="$TOOLS_DIR/unluac/unluac_${UNLUAC_VERSION}.jar"
MISMATCH=0
export PATH="$BIN_DIR:$HOME/.cargo/bin:$PATH"

# ---------------------------------------------------------------- helpers

log()  { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33mwarn:\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

ask() {
  [ "$ASSUME_YES" -eq 1 ] && return 0
  local reply
  printf '%s [y/N] ' "$1"
  read -r reply </dev/tty || return 1
  case "$reply" in y|Y|yes|YES) return 0 ;; *) return 1 ;; esac
}

# version_ge A B: true when A >= B (dotted versions)
version_ge() {
  [ "$(printf '%s\n%s\n' "$2" "$1" | sort -V | head -n 1)" = "$2" ]
}

# report NAME PINNED FOUND: print a row; flag a mismatch when FOUND doesn't start with PINNED
report() {
  local status="ok"
  case "$3" in
    "$2"*) [ -n "$3" ] || { status="MISSING"; MISMATCH=1; } ;;
    *) status="MISMATCH"; MISMATCH=1 ;;
  esac
  printf '  %-18s pinned %-10s found %-24s %s\n' "$1" "$2" "${3:-missing}" "$status"
}

sha256_of() {
  if have sha256sum; then sha256sum "$1" | awk '{print $1}'; else shasum -a 256 "$1" | awk '{print $1}'; fi
}

# download_verified URL SHA256 DEST
download_verified() {
  curl -fsSL -o "$3" "$1"
  local got
  got=$(sha256_of "$3")
  [ "$got" = "$2" ] || die "checksum mismatch for $1 (expected $2, got $got)"
}

# ---------------------------------------------------------------- platform

OS=$(uname -s)
ARCH=$(uname -m)
case "$ARCH" in
  arm64|aarch64) DD_ARCH=arm64 ;;
  x86_64|amd64)  DD_ARCH=x64 ;;
  *) die "unsupported architecture: $ARCH" ;;
esac
case "$OS" in
  Darwin) PLATFORM=macos; DISTRO=macos ;;
  Linux)
    PLATFORM=linux
    DISTRO=other
    if [ -r /etc/os-release ]; then
      # shellcheck disable=SC1091
      . /etc/os-release
      case "${ID:-}" in fedora) DISTRO=fedora ;; esac
    fi
    [ -e /run/ostree-booted ] && DISTRO="$DISTRO-ostree"
    ;;
  *) die "unsupported OS: $OS (macOS and Linux only)" ;;
esac

if [ "$PLATFORM" = macos ]; then
  for p in /opt/homebrew/bin/brew /usr/local/bin/brew; do
    [ -x "$p" ] && ! have brew && eval "$("$p" shellenv)"
  done
  have brew && NODE_BIN="$(brew --prefix "node@$NODE_MAJOR" 2>/dev/null)/bin" && export PATH="$NODE_BIN:$PATH"
fi

java_home() {
  if [ "$PLATFORM" = macos ]; then
    # -F: fail instead of silently falling back to another installed JDK
    /usr/libexec/java_home -F -v "$JAVA_MAJOR" 2>/dev/null || true
  elif [ -x "$JDK_DIR/bin/javac" ]; then
    echo "$JDK_DIR"
  fi
  return 0
}

# cargo_tool_version CRATE [BINARY]: version from `cargo install --list`, else from `BINARY --version`
# (covers tools installed some other way, e.g. Homebrew). Never call an MCP server binary here:
# they wait on stdin.
cargo_tool_version() {
  local v
  v=$(cargo install --list 2>/dev/null | awk -v n="$1" '$1 == n { sub(/^v/, "", $2); sub(/:$/, "", $2); print $2 }' || true)
  if [ -z "$v" ] && [ -n "${2:-}" ] && have "$2"; then
    v=$("$2" --version </dev/null 2>/dev/null | grep -Eo '[0-9]+\.[0-9]+\.[0-9]+' | head -n 1 || true)
  fi
  echo "$v"
}

# Binary used for the --version fallback of each cargo tool.
tool_bin() {
  case "$1" in
    tauri-cli) echo cargo-tauri ;;
    git-cliff) echo git-cliff ;;
    cargo-deny) echo cargo-deny ;;
    sqlx-cli) echo sqlx ;;
    *) echo "" ;;
  esac
}

# ---------------------------------------------------------------- check mode

check_all() {
  log "Installed vs pinned ($DISTRO/$ARCH)"
  report claude "$CLAUDE_CODE_MIN" "$(claude --version 2>/dev/null | awk '{print $1}')"
  # rust-toolchain.toml needs a toolchain literally named $RUST_TOOLCHAIN, even if stable has the same version.
  report rust "$RUST_TOOLCHAIN" "$(rustup run "$RUST_TOOLCHAIN" rustc --version 2>/dev/null | awk '{print $2}')"
  report tauri-cli "$TAURI_CLI_VERSION" "$(cargo_tool_version tauri-cli cargo-tauri)"
  report git-cliff "$GIT_CLIFF_VERSION" "$(cargo_tool_version git-cliff git-cliff)"
  report cargo-deny "$CARGO_DENY_VERSION" "$(cargo_tool_version cargo-deny cargo-deny)"
  report sqlx-cli "$SQLX_CLI_VERSION" "$(cargo_tool_version sqlx-cli sqlx)"
  report bevy_brp_mcp "$BEVY_BRP_MCP_VERSION" "$(cargo_tool_version bevy_brp_mcp)"
  report node "$NODE_MAJOR" "$(node --version 2>/dev/null | sed 's/^v//')"
  report pnpm "$PNPM_VERSION" "$(pnpm --version 2>/dev/null)"
  local jh
  jh=$(java_home)
  report java "$JAVA_MAJOR" "$([ -n "$jh" ] && "$jh/bin/java" -version 2>&1 | awk -F'"' '/version/ {print $2}')"
  report ghidra "$GHIDRA_VERSION" "$([ -x "$GHIDRA_DIR/support/analyzeHeadless" ] && echo "$GHIDRA_VERSION")"
  report unluac "$UNLUAC_VERSION" "$([ -f "$UNLUAC_JAR" ] && echo "$UNLUAC_VERSION")"
  report DepotDownloader "$DEPOTDOWNLOADER_VERSION" "$([ -x "$DD_DIR/DepotDownloader" ] && echo "$DEPOTDOWNLOADER_VERSION")"
  if [ "$PLATFORM" = linux ]; then
    report umu-launcher "$UMU_LAUNCHER_VERSION" "$([ -n "$(find "$UMU_DIR" -name umu-run -type f 2>/dev/null | head -n 1)" ] && echo "$UMU_LAUNCHER_VERSION")"
    printf '  %-18s %s\n' tcpdump "$(tcpdump --version 2>&1 | head -n 1 || echo missing)"
    printf '  %-18s %s\n' tshark "$(tshark --version 2>/dev/null | head -n 1 || echo missing)"
  fi
  echo "  GHIDRA_INSTALL_DIR should be: $GHIDRA_DIR (env now: ${GHIDRA_INSTALL_DIR:-unset})"
  echo "  git hooks: $(git -C "$ROOT" config core.hooksPath || echo 'not set')"
  if have npx && have node; then
    log "REA readiness (read-only)"
    # shellcheck disable=SC2016 # the single-quoted program is JavaScript
    GHIDRA_INSTALL_DIR="$GHIDRA_DIR" npx -y "rea-agents@$REA_AGENTS_VERSION" doctor --client claude_code --json 2>/dev/null \
      | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{const d=JSON.parse(s);for(const c of d.checks||[]){if(/^registration:(?!claude_code)/.test(c.name))continue;console.log(`  ${c.ok?"ok ":"!! "} ${c.name}${c.ok?"":" - "+(c.remediation||c.detail||"")}`)}})' \
      || warn "REA doctor failed to run"
  fi
}

if [ "$MODE" = check ]; then
  check_all
  [ "$MISMATCH" -eq 0 ] || { warn "some tools differ from scripts/versions.env"; exit 1; }
  exit 0
fi

# ---------------------------------------------------------------- 1. system packages

FEDORA_PACKAGES="gcc gcc-c++ make pkgconf-pkg-config openssl-devel webkit2gtk4.1-devel libxdo-devel \
libappindicator-gtk3-devel librsvg2-devel alsa-lib-devel curl file unzip git tcpdump wireshark-cli bubblewrap \
nodejs${NODE_MAJOR} nodejs${NODE_MAJOR}-npm uv"

case "$DISTRO" in
  macos)
    xcode-select -p >/dev/null 2>&1 || die "Xcode Command Line Tools missing: run 'xcode-select --install' and re-run"
    if ! have brew; then
      ask "Homebrew is required on macOS. Install it now?" || die "install Homebrew from https://brew.sh and re-run"
      /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
      for p in /opt/homebrew/bin/brew /usr/local/bin/brew; do [ -x "$p" ] && eval "$("$p" shellenv)"; done
    fi
    log "Homebrew: temurin@$JAVA_MAJOR, node@$NODE_MAJOR"
    brew list --cask "temurin@$JAVA_MAJOR" >/dev/null 2>&1 || brew install --cask "temurin@$JAVA_MAJOR"
    brew list --versions "node@$NODE_MAJOR" >/dev/null 2>&1 || brew install "node@$NODE_MAJOR"
    brew list --versions uv >/dev/null 2>&1 || brew install uv
    NODE_BIN="$(brew --prefix "node@$NODE_MAJOR")/bin"
    export PATH="$NODE_BIN:$PATH"
    ;;
  fedora)
    log "Fedora packages (build tools, Tauri prerequisites, packet capture, Java $JAVA_MAJOR, Node $NODE_MAJOR)"
    echo "  sudo dnf install -y $FEDORA_PACKAGES"
    if ask "Run this with sudo?"; then
      # shellcheck disable=SC2086
      sudo dnf install -y $FEDORA_PACKAGES
    else
      warn "skipped system packages"
    fi
    ;;
  *-ostree)
    warn "Immutable (ostree) system: layer these with rpm-ostree or use a Fedora toolbox/distrobox, then re-run:"
    warn "  $FEDORA_PACKAGES"
    ;;
  *)
    warn "Untested distro. Install the equivalents of these Fedora packages, then re-run:"
    warn "  $FEDORA_PACKAGES"
    ;;
esac

if [ "$PLATFORM" = linux ]; then
  # Fedora's versioned Node streams may install node-24/npm-24 instead of node/npm.
  mkdir -p "$BIN_DIR"
  for t in node npm npx; do
    if have "$t-$NODE_MAJOR"; then ln -sf "$(command -v "$t-$NODE_MAJOR")" "$BIN_DIR/$t"; fi
  done
fi
have node || die "Node.js $NODE_MAJOR not found after system packages"
node_major=$(node --version | sed 's/^v//; s/\..*//')
[ "$node_major" = "$NODE_MAJOR" ] || warn "node on PATH is v$node_major, pinned major $NODE_MAJOR"

# ---------------------------------------------------------------- 2. Rust

if ! have rustup; then
  log "Installing rustup"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain none --no-modify-path
fi
log "Rust $RUST_TOOLCHAIN (rust-toolchain.toml)"
rustup toolchain install "$RUST_TOOLCHAIN" --profile minimal --component rustfmt,clippy,rust-analyzer

# ---------------------------------------------------------------- 3. cargo tools

cargo_tool() {
  local name=$1 version=$2
  shift 2
  if [ "$(cargo_tool_version "$name" "$(tool_bin "$name")")" = "$version" ]; then
    echo "  $name $version already installed"
  else
    log "cargo install $name $version"
    cargo "+$RUST_TOOLCHAIN" install --locked "$name" --version "$version" "$@"
  fi
}
cargo_tool tauri-cli "$TAURI_CLI_VERSION"
cargo_tool git-cliff "$GIT_CLIFF_VERSION"
cargo_tool cargo-deny "$CARGO_DENY_VERSION"
cargo_tool sqlx-cli "$SQLX_CLI_VERSION" --no-default-features --features rustls,sqlite
cargo_tool bevy_brp_mcp "$BEVY_BRP_MCP_VERSION"

# ---------------------------------------------------------------- 4. pnpm

if [ "$(pnpm --version 2>/dev/null || true)" != "$PNPM_VERSION" ]; then
  log "pnpm $PNPM_VERSION -> ~/.local"
  npm install -g --prefix "$HOME/.local" "pnpm@$PNPM_VERSION"
fi

# ---------------------------------------------------------------- 5. JDK (Linux), Ghidra, DepotDownloader, umu-launcher

mkdir -p "$TOOLS_DIR" "$BIN_DIR"

if [ "$PLATFORM" = linux ] && [ ! -x "$JDK_DIR/bin/javac" ]; then
  log "Temurin JDK $TEMURIN_VERSION (SHA-256 verified)"
  temurin_arch=$([ "$DD_ARCH" = arm64 ] && echo aarch64 || echo x64)
  sum_var="TEMURIN_SHA256_linux_${DD_ARCH}"
  sum=$(eval "echo \${$sum_var:-}")
  [ -n "$sum" ] || die "no pinned checksum for Temurin on linux-${DD_ARCH}"
  tmp=$(mktemp -d)
  download_verified \
    "https://github.com/adoptium/temurin${JAVA_MAJOR}-binaries/releases/download/jdk-${TEMURIN_VERSION/+/%2B}/OpenJDK${JAVA_MAJOR}U-jdk_${temurin_arch}_linux_hotspot_${TEMURIN_VERSION/+/_}.tar.gz" \
    "$sum" "$tmp/jdk.tar.gz"
  tar -xzf "$tmp/jdk.tar.gz" -C "$TOOLS_DIR"
  rm -rf "$tmp"
fi
[ -n "$(java_home)" ] || warn "Java $JAVA_MAJOR JDK not found; Ghidra (and REA's native provider) won't run"

if [ ! -x "$GHIDRA_DIR/support/analyzeHeadless" ]; then
  log "Ghidra $GHIDRA_VERSION (official release zip, SHA-256 verified)"
  tmp=$(mktemp -d)
  download_verified \
    "https://github.com/NationalSecurityAgency/ghidra/releases/download/Ghidra_${GHIDRA_VERSION}_build/ghidra_${GHIDRA_VERSION}_PUBLIC_${GHIDRA_RELEASE_DATE}.zip" \
    "$GHIDRA_SHA256" "$tmp/ghidra.zip"
  unzip -q -o "$tmp/ghidra.zip" -d "$TOOLS_DIR"
  rm -rf "$tmp"
fi
ln -sf "$GHIDRA_DIR/ghidraRun" "$BIN_DIR/ghidraRun"

GMCP_BASE="https://github.com/bethington/ghidra-mcp/releases/download/v${GHIDRA_MCP_VERSION}"
if ! uv tool list 2>/dev/null | grep -Eq "^ghidra[-_]mcp[-_]bridge v${GHIDRA_MCP_BRIDGE_VERSION}"; then
  log "ghidra-mcp bridge ${GHIDRA_MCP_BRIDGE_VERSION} (SHA-256 verified)"
  tmp=$(mktemp -d)
  whl="ghidra_mcp_bridge-${GHIDRA_MCP_BRIDGE_VERSION}-py3-none-any.whl"
  download_verified "$GMCP_BASE/$whl" "$GHIDRA_MCP_BRIDGE_SHA256" "$tmp/$whl"
  uv tool install --force --constraints "$ROOT/scripts/ghidra-mcp-bridge.constraints.txt" "$tmp/$whl"
  rm -rf "$tmp"
fi
if [ "$PLATFORM" = macos ]; then
  GHIDRA_USER_DIR="$HOME/Library/ghidra/ghidra_${GHIDRA_VERSION}_PUBLIC"
else
  GHIDRA_USER_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/ghidra/ghidra_${GHIDRA_VERSION}_PUBLIC"
fi
if [ ! -d "$GHIDRA_USER_DIR/Extensions/GhidraMCP" ]; then
  log "GhidraMCP extension ${GHIDRA_MCP_VERSION} (SHA-256 verified)"
  tmp=$(mktemp -d)
  download_verified "$GMCP_BASE/GhidraMCP-${GHIDRA_MCP_VERSION}.zip" "$GHIDRA_MCP_EXTENSION_SHA256" "$tmp/ext.zip"
  mkdir -p "$GHIDRA_USER_DIR/Extensions"
  unzip -q -o "$tmp/ext.zip" -d "$GHIDRA_USER_DIR/Extensions"
  rm -rf "$tmp"
fi

if [ ! -x "$DD_DIR/DepotDownloader" ]; then
  log "DepotDownloader $DEPOTDOWNLOADER_VERSION (SHA-256 verified)"
  sum_var="DEPOTDOWNLOADER_SHA256_${PLATFORM}_${DD_ARCH}"
  sum=$(eval "echo \${$sum_var:-}")
  [ -n "$sum" ] || die "no pinned checksum for DepotDownloader on ${PLATFORM}-${DD_ARCH}"
  tmp=$(mktemp -d)
  download_verified \
    "https://github.com/SteamRE/DepotDownloader/releases/download/DepotDownloader_${DEPOTDOWNLOADER_VERSION}/DepotDownloader-${PLATFORM}-${DD_ARCH}.zip" \
    "$sum" "$tmp/dd.zip"
  mkdir -p "$DD_DIR"
  unzip -q -o "$tmp/dd.zip" -d "$DD_DIR"
  chmod +x "$DD_DIR/DepotDownloader"
  rm -rf "$tmp"
fi
ln -sf "$DD_DIR/DepotDownloader" "$BIN_DIR/DepotDownloader"

if [ ! -f "$UNLUAC_JAR" ]; then
  log "unluac $UNLUAC_VERSION (SHA-256 verified)"
  mkdir -p "$(dirname "$UNLUAC_JAR")"
  download_verified \
    "https://downloads.sourceforge.net/project/unluac/Unstable/unluac_${UNLUAC_VERSION}.jar" \
    "$UNLUAC_SHA256" "$UNLUAC_JAR"
fi

if [ "$PLATFORM" = linux ]; then
  if [ ! -d "$UMU_DIR" ]; then
    log "umu-launcher $UMU_LAUNCHER_VERSION (zipapp, SHA-256 verified)"
    tmp=$(mktemp -d)
    download_verified \
      "https://github.com/Open-Wine-Components/umu-launcher/releases/download/${UMU_LAUNCHER_VERSION}/umu-launcher-${UMU_LAUNCHER_VERSION}-zipapp.tar" \
      "$UMU_LAUNCHER_SHA256" "$tmp/umu.tar"
    mkdir -p "$UMU_DIR"
    tar -xf "$tmp/umu.tar" -C "$UMU_DIR"
    rm -rf "$tmp"
  fi
  umu_run=$(find "$UMU_DIR" -type f -name umu-run | head -n 1)
  if [ -n "$umu_run" ]; then ln -sf "$umu_run" "$BIN_DIR/umu-run"; else warn "umu-run not found in $UMU_DIR"; fi
fi

# ---------------------------------------------------------------- 6. shell environment

mkdir -p "$(dirname "$ENV_FILE")"
{
  echo "# Generated by fenji/scripts/bootstrap.sh; re-run bootstrap to refresh."
  echo "export PATH=\"$BIN_DIR:\$HOME/.cargo/bin:${NODE_BIN:+$NODE_BIN:}\$PATH\""
  echo "export GHIDRA_INSTALL_DIR=\"$GHIDRA_DIR\""
  echo "export JAVA_HOME=\"$(java_home)\""
  echo "export FENJI_CLIENT_PATH=\"\${FENJI_CLIENT_PATH:-\$HOME/fenji-client}\""
} > "$ENV_FILE"
log "Wrote $ENV_FILE"

case "${SHELL:-}" in
  */zsh)  rc="$HOME/.zshrc" ;;
  */bash) rc="$HOME/.bashrc" ;;
  *)      rc="" ;;
esac
line="[ -f \"$ENV_FILE\" ] && . \"$ENV_FILE\""
if [ -n "$rc" ] && ! grep -qF "$ENV_FILE" "$rc" 2>/dev/null; then
  if ask "Add 'source $ENV_FILE' to $rc?"; then
    printf '\n# Fenji dev environment\n%s\n' "$line" >> "$rc"
  else
    warn "Add this line to your shell rc yourself: $line"
  fi
fi
# shellcheck disable=SC1090
. "$ENV_FILE"

if [ "$PLATFORM" = linux ] && have tcpdump; then
  echo "  Packet capture without sudo (optional): sudo setcap cap_net_raw,cap_net_admin=eip \"$(command -v tcpdump)\""
fi

# ---------------------------------------------------------------- 7. git hooks

log "Git hooks (.githooks: Conventional Commits + forbidden-file check)"
chmod +x "$ROOT/.githooks/"* "$ROOT/scripts/"*.sh
git -C "$ROOT" config core.hooksPath .githooks

# ---------------------------------------------------------------- 8. Claude Code: plugins, REA skill

if ! have claude; then
  warn "Claude Code not found; install it (https://code.claude.com) and re-run to set up plugins"
else
  have_cc=$(claude --version | awk '{print $1}')
  version_ge "$have_cc" "$CLAUDE_CODE_MIN" || warn "Claude Code $have_cc is older than $CLAUDE_CODE_MIN; run 'claude update'"

  log "Claude Code marketplaces and project-scope plugins (from .claude/settings.json)"
  (
    cd "$ROOT"
    claude plugin marketplace add anthropics/claude-plugins-official --scope project
    claude plugin marketplace add pbakaus/impeccable --scope project
    node -e 'const s=require("./.claude/settings.json");for(const [p,on] of Object.entries(s.enabledPlugins))if(on)console.log(p)' \
      | while IFS= read -r plugin; do
          claude plugin install "$plugin" --scope project || warn "failed to install $plugin"
        done
  )

  log "REA $REA_AGENTS_VERSION: registers the MCP for Claude Code and installs its matching skill"
  npx -y "rea-agents@$REA_AGENTS_VERSION" setup --client claude_code --dry-run
  if ask "Apply the REA setup shown above?"; then
    npx -y "rea-agents@$REA_AGENTS_VERSION" setup --client claude_code --yes
  else
    warn "skipped REA setup; the project .mcp.json still registers REA, but the reverse-engineer-anything skill won't be installed"
  fi
fi

# ---------------------------------------------------------------- done

echo
check_all || true
echo
log "Done. Open a new shell (or 'source $ENV_FILE'), then start Claude Code in $ROOT."
log "First launch: accept the workspace trust prompt and approve the project MCP servers (rea, brp)."
[ "$MISMATCH" -eq 0 ] || warn "Some versions differ from scripts/versions.env (see table above)."
